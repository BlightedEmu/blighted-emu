// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sets up structured logging for a program.
//!
//! The filter uses the `tracing` directive syntax, for example `info,nw_sim=debug`.
//!
//! A log line is never written on the thread that logs it: the line goes into a bounded queue
//! and one writer thread writes it to standard output. A sim logs from its tick thread, and a
//! blocking write there stopped the ticks while the disk was busy (2026-11-22: sim-1 missed up
//! to 347 ticks during builds while it wrote 20 to 30 GB of log an hour). When the queue is full,
//! the line is dropped and counted, and the writer reports the count.

use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread;
use std::time::Duration;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::ParseError;
use tracing_subscriber::fmt::MakeWriter;

/// Log lines the queue holds before a new line is dropped.
pub const LOG_QUEUE_LINES: usize = 16_384;
/// The longest wait of [`LogFlush`] for the writer to write every queued line.
pub const LOG_FLUSH_TIMEOUT: Duration = Duration::from_secs(5);
/// The name of the writer thread.
const WRITER_THREAD: &str = "log-writer";

/// Error returned by [`parse_log_filter`] and [`init_logging`].
#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    /// The filter text is not valid `tracing` directive syntax.
    #[error("invalid log filter `{filter}`: {source}")]
    InvalidFilter {
        /// The filter text that failed to parse.
        filter: String,
        /// The parser error.
        #[source]
        source: ParseError,
    },
    /// A global logger is already installed in this process.
    #[error("cannot install the global logger: {0}")]
    GlobalInstall(String),
    /// The writer thread could not start.
    #[error("cannot start the log writer thread: {0}")]
    WriterThread(io::Error),
}

/// Parses a log filter such as `info,nw_sim=debug`.
pub fn parse_log_filter(filter: &str) -> Result<EnvFilter, LoggingError> {
    EnvFilter::try_new(filter).map_err(|source| LoggingError::InvalidFilter {
        filter: filter.to_owned(),
        source,
    })
}

/// Installs the global logger with the given filter. Call it once at program start, and keep
/// the returned [`LogFlush`] until the program ends: its drop writes the lines still queued.
pub fn init_logging(filter: &str) -> Result<LogFlush, LoggingError> {
    let env_filter = parse_log_filter(filter)?;
    let (writer, flush) = spawn_queue_writer(io::stdout(), LOG_QUEUE_LINES)?;
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(writer)
        .try_init()
        .map_err(|error| LoggingError::GlobalInstall(error.to_string()))?;
    Ok(flush)
}

/// What the queue carries to the writer thread.
#[derive(Debug)]
enum LogMessage {
    /// One formatted log line.
    Line(Vec<u8>),
    /// Write every line queued before this one, then answer.
    Flush(mpsc::Sender<()>),
}

/// The logger's writer: each event formats into a [`QueuedLine`] that goes into the queue.
#[derive(Debug)]
pub struct QueueWriter {
    queue: SyncSender<LogMessage>,
    dropped: Arc<AtomicU64>,
}

/// One formatted event; it goes into the queue when it is dropped, or is counted as dropped
/// when the queue is full.
pub struct QueuedLine<'a> {
    bytes: Vec<u8>,
    writer: &'a QueueWriter,
}

impl Write for QueuedLine<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for QueuedLine<'_> {
    fn drop(&mut self) {
        if self.bytes.is_empty() {
            return;
        }
        let line = LogMessage::Line(std::mem::take(&mut self.bytes));
        if let Err(TrySendError::Full(_)) = self.writer.queue.try_send(line) {
            self.writer.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl<'a> MakeWriter<'a> for QueueWriter {
    type Writer = QueuedLine<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        QueuedLine {
            bytes: Vec::new(),
            writer: self,
        }
    }
}

/// Writes the lines still queued when it is dropped (at most [`LOG_FLUSH_TIMEOUT`]).
#[derive(Debug)]
pub struct LogFlush {
    queue: SyncSender<LogMessage>,
}

impl LogFlush {
    /// Waits until the writer has written every line queued before this call, at most
    /// `timeout`. Returns `false` when the writer did not answer in time.
    pub fn flush(&self, timeout: Duration) -> bool {
        let (answer, answered) = mpsc::channel();
        self.queue.send(LogMessage::Flush(answer)).is_ok() && answered.recv_timeout(timeout).is_ok()
    }
}

impl Drop for LogFlush {
    fn drop(&mut self) {
        self.flush(LOG_FLUSH_TIMEOUT);
    }
}

/// Starts the writer thread that writes queued lines to `sink`, with room for `capacity` lines.
fn spawn_queue_writer<W: Write + Send + 'static>(
    sink: W,
    capacity: usize,
) -> Result<(QueueWriter, LogFlush), LoggingError> {
    let (queue, lines) = mpsc::sync_channel(capacity);
    let dropped = Arc::new(AtomicU64::new(0));
    let writer_dropped = Arc::clone(&dropped);
    thread::Builder::new()
        .name(WRITER_THREAD.to_owned())
        .spawn(move || write_lines(sink, &lines, &writer_dropped))
        .map_err(LoggingError::WriterThread)?;
    Ok((
        QueueWriter {
            queue: queue.clone(),
            dropped,
        },
        LogFlush { queue },
    ))
}

/// The writer thread: writes each queued line, flushes when the queue is empty, and reports
/// the lines dropped since its last report. A failed write (a closed output) is not retried.
fn write_lines<W: Write>(sink: W, lines: &Receiver<LogMessage>, dropped: &AtomicU64) {
    let mut out = io::BufWriter::new(sink);
    while let Ok(first) = lines.recv() {
        let mut next = Some(first);
        while let Some(message) = next {
            match message {
                LogMessage::Line(bytes) => {
                    let _ = out.write_all(&bytes);
                }
                LogMessage::Flush(answer) => {
                    let _ = out.flush();
                    let _ = answer.send(());
                }
            }
            next = lines.try_recv().ok();
        }
        let _ = out.flush();
        let count = dropped.swap(0, Ordering::Relaxed);
        if count > 0 {
            tracing::warn!(dropped = count, "log queue full; lines dropped");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const VALID_FILTER: &str = "info,nw_common=debug";
    const INVALID_LEVEL_FILTER: &str = "nw_common=loud";
    const TEST_CAPACITY: usize = 4;
    const TEST_TIMEOUT: Duration = Duration::from_secs(5);

    /// A sink the test can read after the writer wrote to it.
    #[derive(Clone, Default)]
    struct SharedSink(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedSink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("sink").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// A sink that blocks every write until the test opens its gate.
    struct GatedSink(Arc<Mutex<()>>);

    impl Write for GatedSink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let _open = self.0.lock().expect("gate");
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            let _open = self.0.lock().expect("gate");
            Ok(())
        }
    }

    fn log_line(writer: &QueueWriter, text: &str) {
        let mut line = writer.make_writer();
        line.write_all(text.as_bytes()).expect("write");
    }

    #[test]
    fn valid_filter_parses() {
        assert!(parse_log_filter(VALID_FILTER).is_ok());
    }

    #[test]
    fn unknown_level_is_rejected_with_the_filter_text() {
        let error = parse_log_filter(INVALID_LEVEL_FILTER).expect_err("loud is not a level");
        assert!(
            matches!(&error, LoggingError::InvalidFilter { filter, .. } if filter == INVALID_LEVEL_FILTER),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn init_rejects_invalid_filter_before_installing() {
        let error = init_logging(INVALID_LEVEL_FILTER).expect_err("invalid filter");
        assert!(
            matches!(error, LoggingError::InvalidFilter { .. }),
            "unexpected error: {error}"
        );
    }

    // The only test in this binary that installs the global logger.
    #[test]
    fn second_install_is_an_error() {
        let _flush = init_logging(VALID_FILTER).expect("first install succeeds");
        let error = init_logging(VALID_FILTER).expect_err("second install fails");
        assert!(
            matches!(error, LoggingError::GlobalInstall(_)),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn queued_lines_reach_the_sink_in_order_by_the_flush() {
        let sink = SharedSink::default();
        let (writer, flush) =
            spawn_queue_writer(sink.clone(), TEST_CAPACITY).expect("writer thread");
        log_line(&writer, "first\n");
        log_line(&writer, "second\n");
        assert!(flush.flush(TEST_TIMEOUT), "the writer answers the flush");
        let written = sink.0.lock().expect("sink").clone();
        assert_eq!(written, b"first\nsecond\n");
    }

    #[test]
    fn a_full_queue_drops_and_counts_lines_without_waiting() {
        let gate = Arc::new(Mutex::new(()));
        let closed = gate.lock().expect("gate");
        let (writer, _flush) =
            spawn_queue_writer(GatedSink(Arc::clone(&gate)), TEST_CAPACITY).expect("writer");
        let lines = TEST_CAPACITY * 4;
        for _ in 0..lines {
            log_line(&writer, "line\n");
        }
        // The writer holds at most one line and the queue `TEST_CAPACITY`: the rest dropped,
        // and the logging thread never waited for the blocked sink.
        let dropped = writer.dropped.load(Ordering::Relaxed);
        assert!(
            dropped >= (lines - TEST_CAPACITY - 1) as u64,
            "dropped {dropped} of {lines}"
        );
        drop(closed);
    }
}
