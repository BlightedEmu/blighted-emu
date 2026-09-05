// SPDX-License-Identifier: AGPL-3.0-or-later
//! The world events of one gateway worker (M5 load spec section 3). A worker that falls behind
//! loses bundles, the unreliable state that the next tick sends again, so it never stops the link
//! or the other workers. It never loses a reliable message or a close: a lost login message would
//! leave a player who never appears. Every event that is kept stays in order.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::mpsc;

use crate::world::WorldEvent;

/// What became of one event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// The worker gets it.
    Queued,
    /// A bundle lost because the worker already has the most bundles waiting.
    Dropped,
    /// The worker is gone.
    Gone,
}

/// The sending side of one worker's queue.
#[derive(Debug, Clone)]
pub struct EventSender {
    events: mpsc::UnboundedSender<WorldEvent>,
    bundles: Arc<AtomicUsize>,
    max_bundles: usize,
}

/// The worker's side of its queue.
#[derive(Debug)]
pub struct EventReceiver {
    events: mpsc::UnboundedReceiver<WorldEvent>,
    bundles: Arc<AtomicUsize>,
}

/// A queue that holds at most `max_bundles` bundles, and every other event.
pub fn event_queue(max_bundles: usize) -> (EventSender, EventReceiver) {
    let (events, receiver) = mpsc::unbounded_channel();
    let bundles = Arc::new(AtomicUsize::new(0));
    (
        EventSender {
            events,
            bundles: Arc::clone(&bundles),
            max_bundles,
        },
        EventReceiver {
            events: receiver,
            bundles,
        },
    )
}

/// Whether the worker may lose `event`.
fn is_bundle(event: &WorldEvent) -> bool {
    matches!(
        event,
        WorldEvent::Messages {
            reliable: false,
            ..
        }
    )
}

impl EventSender {
    /// Queues `event`, or loses it when it is a bundle and the queue holds the most bundles.
    pub fn send(&self, event: WorldEvent) -> Delivery {
        if self.events.is_closed() {
            return Delivery::Gone;
        }
        let bundle = is_bundle(&event);
        if bundle && !self.take_bundle_place() {
            return Delivery::Dropped;
        }
        if self.events.send(event).is_err() {
            return Delivery::Gone;
        }
        Delivery::Queued
    }

    /// Whether the worker is gone.
    pub fn is_closed(&self) -> bool {
        self.events.is_closed()
    }

    fn take_bundle_place(&self) -> bool {
        self.bundles
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |waiting| {
                (waiting < self.max_bundles).then_some(waiting + 1)
            })
            .is_ok()
    }
}

impl EventReceiver {
    /// The next event; `None` when every sender is gone.
    pub async fn recv(&mut self) -> Option<WorldEvent> {
        let event = self.events.recv().await?;
        Some(self.taken(event))
    }

    /// The next event if one waits.
    pub fn try_recv(&mut self) -> Option<WorldEvent> {
        let event = self.events.try_recv().ok()?;
        Some(self.taken(event))
    }

    /// Gives the place of a received bundle back.
    fn taken(&self, event: WorldEvent) -> WorldEvent {
        if is_bundle(&event) {
            self.bundles.fetch_sub(1, Ordering::AcqRel);
        }
        event
    }
}

#[cfg(test)]
mod tests {
    use nw_bus::BusId;

    use super::*;

    const MAX_BUNDLES: usize = 2;
    const CONNECTION: u64 = 7;
    const CHANNEL: u8 = 1;

    fn messages(reliable: bool, envelope: u8) -> WorldEvent {
        WorldEvent::Messages {
            sim: BusId::try_from("sim-1".to_owned()).expect("id"),
            connection: CONNECTION,
            channel: CHANNEL,
            reliable,
            envelopes: vec![vec![envelope]],
        }
    }

    #[tokio::test]
    async fn a_full_queue_loses_bundles_but_keeps_every_other_event_in_order() {
        let (sender, mut receiver) = event_queue(MAX_BUNDLES);
        let close = WorldEvent::Close {
            connection: CONNECTION,
        };
        let sent = [
            (messages(false, 1), Delivery::Queued),
            (messages(true, 2), Delivery::Queued),
            (messages(false, 3), Delivery::Queued),
            (messages(false, 4), Delivery::Dropped),
            (messages(true, 5), Delivery::Queued),
            (close.clone(), Delivery::Queued),
        ];
        for (event, delivery) in sent {
            assert_eq!(sender.send(event), delivery);
        }
        for expected in [
            messages(false, 1),
            messages(true, 2),
            messages(false, 3),
            messages(true, 5),
            close,
        ] {
            assert_eq!(receiver.recv().await, Some(expected));
        }
        assert!(receiver.try_recv().is_none(), "the lost bundle never comes");
        assert_eq!(sender.send(messages(false, 6)), Delivery::Queued);
        assert_eq!(sender.send(messages(false, 7)), Delivery::Queued);
        assert_eq!(
            sender.send(messages(false, 8)),
            Delivery::Dropped,
            "a received bundle gives its place back"
        );
    }

    #[test]
    fn a_queue_without_its_worker_is_gone() {
        let (sender, receiver) = event_queue(MAX_BUNDLES);
        assert!(!sender.is_closed());
        drop(receiver);
        assert!(sender.is_closed());
        assert_eq!(sender.send(messages(true, 1)), Delivery::Gone);
        assert_eq!(sender.send(messages(false, 2)), Delivery::Gone);
    }
}
