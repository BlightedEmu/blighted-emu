// SPDX-License-Identifier: AGPL-3.0-or-later
//! Open sessions (admin tools spec section 4.3): two slots, `main` and `compare`. Opening reads
//! the ledger and builds its index on a blocking thread; a newer open of a slot replaces an
//! older one, whichever finishes first.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use nw_capture::SessionIndex;

use crate::captures::catalog::SessionEntry;

/// The error of a slot whose indexer panicked.
const INDEXER_PANIC: &str = "the index build panicked on this ledger";

/// Reads a ledger file and builds its index.
pub type Indexer =
    Arc<dyn Fn(&Path) -> Result<(Arc<[u8]>, SessionIndex), String> + Send + Sync + 'static>;

/// A place for one open session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    /// The session in view.
    Main,
    /// The session to compare with.
    Compare,
}

impl Slot {
    /// The slot of a URL name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "main" => Some(Self::Main),
            "compare" => Some(Self::Compare),
            _ => None,
        }
    }
}

/// What a slot holds.
#[derive(Clone)]
pub enum SlotState {
    /// Nothing.
    Empty,
    /// A session being read and indexed.
    Indexing {
        /// Session id.
        id: String,
    },
    /// An indexed session.
    Ready {
        /// Session id.
        id: String,
        /// The index.
        index: Arc<SessionIndex>,
        /// The ledger bytes (for message bytes).
        ledger: Arc<[u8]>,
    },
    /// A session that could not be read.
    Failed {
        /// Session id.
        id: String,
        /// Why.
        error: String,
    },
}

struct SlotCell {
    ticket: u64,
    state: SlotState,
}

/// The two slots.
pub struct OpenSessions {
    indexer: Indexer,
    tickets: AtomicU64,
    main: Arc<Mutex<SlotCell>>,
    compare: Arc<Mutex<SlotCell>>,
}

impl std::fmt::Debug for OpenSessions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("OpenSessions")
    }
}

impl Default for OpenSessions {
    fn default() -> Self {
        Self::with_indexer(Arc::new(read_and_index))
    }
}

/// Reads `path` and builds its index.
///
/// # Errors
///
/// Returns the read error as text.
pub fn read_and_index(path: &Path) -> Result<(Arc<[u8]>, SessionIndex), String> {
    let ledger: Arc<[u8]> = std::fs::read(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .into();
    let index = SessionIndex::build(&ledger);
    Ok((ledger, index))
}

impl OpenSessions {
    /// Slots that open sessions with `indexer`.
    pub fn with_indexer(indexer: Indexer) -> Self {
        let empty = || {
            Arc::new(Mutex::new(SlotCell {
                ticket: 0,
                state: SlotState::Empty,
            }))
        };
        Self {
            indexer,
            tickets: AtomicU64::new(0),
            main: empty(),
            compare: empty(),
        }
    }

    fn cell(&self, slot: Slot) -> &Arc<Mutex<SlotCell>> {
        match slot {
            Slot::Main => &self.main,
            Slot::Compare => &self.compare,
        }
    }

    /// Starts reading `entry` into `slot`; returns its ticket. Needs a tokio runtime.
    pub fn open(&self, slot: Slot, entry: SessionEntry) -> u64 {
        let ticket = self.tickets.fetch_add(1, Ordering::Relaxed) + 1;
        let cell = Arc::clone(self.cell(slot));
        {
            let mut guard = cell.lock().unwrap_or_else(PoisonError::into_inner);
            guard.ticket = ticket;
            guard.state = SlotState::Indexing {
                id: entry.id.clone(),
            };
        }
        let indexer = Arc::clone(&self.indexer);
        tokio::task::spawn_blocking(move || {
            // A panic on a broken ledger must end in `Failed`, not leave the slot indexing.
            let built =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| indexer(&entry.path)))
                    .unwrap_or_else(|_| Err(INDEXER_PANIC.to_owned()));
            let state = match built {
                Ok((ledger, index)) => SlotState::Ready {
                    id: entry.id,
                    index: Arc::new(index),
                    ledger,
                },
                Err(error) => SlotState::Failed {
                    id: entry.id,
                    error,
                },
            };
            let mut guard = cell.lock().unwrap_or_else(PoisonError::into_inner);
            if guard.ticket == ticket {
                guard.state = state;
            }
        });
        ticket
    }

    /// The state of `slot`.
    pub fn state(&self, slot: Slot) -> SlotState {
        self.cell(slot)
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .state
            .clone()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::time::Duration;

    use nw_capture::SERVER_TO_CLIENT;
    use nw_capture::test_support::{envelope, record, server_datagram};
    use nw_reflect::PING_MSG_TYPE_INDEX;

    use super::*;

    const PING_BODY: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 1];
    const WAIT_STEP: Duration = Duration::from_millis(5);
    const SLOW: Duration = Duration::from_millis(200);

    /// A ledger file with one ping in `dir`; its entry.
    pub(crate) fn ping_session(dir: &Path, id: &str) -> SessionEntry {
        let ledger = record(
            SERVER_TO_CLIENT,
            &server_datagram(&[envelope(PING_MSG_TYPE_INDEX, PING_BODY.to_vec())]),
        );
        let path = dir.join(id);
        std::fs::write(&path, ledger).expect("ledger");
        SessionEntry {
            id: id.to_owned(),
            path,
            bytes: 0,
            name: None,
            started_at: None,
            post_title: None,
        }
    }

    pub(crate) async fn settled(open: &OpenSessions, slot: Slot) -> SlotState {
        loop {
            let state = open.state(slot);
            if !matches!(state, SlotState::Indexing { .. }) {
                return state;
            }
            tokio::time::sleep(WAIT_STEP).await;
        }
    }

    #[tokio::test]
    async fn opening_a_session_indexes_it() {
        let dir = tempfile::tempdir().expect("dir");
        let open = OpenSessions::default();
        assert!(matches!(open.state(Slot::Main), SlotState::Empty));
        open.open(Slot::Main, ping_session(dir.path(), "a.bin"));
        let SlotState::Ready { id, index, .. } = settled(&open, Slot::Main).await else {
            panic!("ready");
        };
        assert_eq!(id, "a.bin");
        assert_eq!(index.summary.messages, 1);
        assert!(matches!(open.state(Slot::Compare), SlotState::Empty));
    }

    #[tokio::test]
    async fn the_last_open_wins() {
        let dir = tempfile::tempdir().expect("dir");
        let open = OpenSessions::with_indexer(Arc::new(|path: &Path| {
            if path.ends_with("slow.bin") {
                std::thread::sleep(SLOW);
            }
            read_and_index(path)
        }));
        open.open(Slot::Main, ping_session(dir.path(), "slow.bin"));
        open.open(Slot::Main, ping_session(dir.path(), "fast.bin"));
        let SlotState::Ready { id, .. } = settled(&open, Slot::Main).await else {
            panic!("ready");
        };
        assert_eq!(id, "fast.bin");
        tokio::time::sleep(SLOW * 2).await;
        let SlotState::Ready { id, .. } = open.state(Slot::Main) else {
            panic!("still ready");
        };
        assert_eq!(id, "fast.bin", "the slow first open did not replace it");
    }

    #[tokio::test]
    async fn a_missing_file_fails_with_its_error() {
        let dir = tempfile::tempdir().expect("dir");
        let open = OpenSessions::default();
        let mut entry = ping_session(dir.path(), "gone.bin");
        entry.path = dir.path().join("not-there.bin");
        open.open(Slot::Main, entry);
        assert!(matches!(
            settled(&open, Slot::Main).await,
            SlotState::Failed { .. }
        ));
    }

    #[tokio::test]
    async fn a_panic_while_indexing_fails_the_slot() {
        let dir = tempfile::tempdir().expect("dir");
        let open = OpenSessions::with_indexer(Arc::new(|_: &Path| panic!("broken index")));
        open.open(Slot::Main, ping_session(dir.path(), "a.bin"));
        let deadline = tokio::time::Instant::now() + SLOW * 5;
        loop {
            match open.state(Slot::Main) {
                SlotState::Failed { error, .. } => {
                    assert!(error.contains("panicked"), "{error}");
                    return;
                }
                _ if tokio::time::Instant::now() > deadline => panic!("still not failed"),
                _ => tokio::time::sleep(WAIT_STEP).await,
            }
        }
    }

    #[test]
    fn slots_parse_from_their_names() {
        assert_eq!(Slot::from_name("main"), Some(Slot::Main));
        assert_eq!(Slot::from_name("compare"), Some(Slot::Compare));
        assert_eq!(Slot::from_name("other"), None);
    }
}
