// SPDX-License-Identifier: AGPL-3.0-or-later
//! The ledger format: per record `NWDL`, `time_ms` u64 LE (Unix milliseconds), `direction`
//! u32 LE, `context` u64 LE, `len` u32 LE, then one decrypted carrier datagram.

use std::io::{self, Write};
use std::path::PathBuf;

use nw_carrier::{SYSTEM_CHANNEL, SystemMessage, decode_datagram, decode_frames};

/// First bytes of every ledger record.
pub const LEDGER_MAGIC: [u8; 4] = *b"NWDL";
/// Ledger direction: server to client.
pub const SERVER_TO_CLIENT: u32 = 0;
/// Ledger direction: client to server.
pub const CLIENT_TO_SERVER: u32 = 1;
/// Bytes of a ledger record header.
pub const RECORD_HEADER_LEN: usize = 28;

const MAGIC_END: usize = 4;
const TIME_END: usize = 12;
const DIRECTION_END: usize = 16;
const CONTEXT_END: usize = 24;
const BITS_PER_BYTE: u32 = 8;

/// Failure to read or write a ledger or the type name files.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    /// A file cannot be read or written.
    #[error("cannot read {path}: {source}")]
    Io {
        /// File.
        path: PathBuf,
        /// Cause.
        source: io::Error,
    },
    /// A ledger record is cut short or does not start with `NWDL`.
    #[error("bad ledger record at byte {offset}")]
    BadRecord {
        /// Byte offset of the record.
        offset: usize,
    },
    /// A ledger record has a direction other than 0 or 1.
    #[error("ledger record at byte {offset} has direction {direction}")]
    BadDirection {
        /// Byte offset of the record.
        offset: usize,
        /// Direction value.
        direction: u32,
    },
    /// A record did not deliver the envelope asked for.
    #[error("record {record} has no message {slot}")]
    NoMessage {
        /// Record number.
        record: u32,
        /// Envelope number.
        slot: u16,
    },
    /// A type name file is not valid JSON of the expected shape.
    #[error("cannot parse {path}: {source}")]
    Json {
        /// File.
        path: PathBuf,
        /// Cause.
        source: serde_json::Error,
    },
}

/// One ledger record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerRecord<'a> {
    /// Unix milliseconds.
    pub time_ms: u64,
    /// [`SERVER_TO_CLIENT`] or [`CLIENT_TO_SERVER`].
    pub direction: u32,
    /// Connection context; one file can hold more than one connection.
    pub context: u64,
    /// One carrier datagram.
    pub datagram: &'a [u8],
}

/// The records of a ledger, in order. After the first error it ends.
#[derive(Debug, Clone)]
pub struct LedgerRecords<'a> {
    bytes: &'a [u8],
    offset: usize,
    failed: bool,
}

/// The records of a ledger as an iterator; a broken record is the last item.
pub fn ledger_records(bytes: &[u8]) -> LedgerRecords<'_> {
    LedgerRecords {
        bytes,
        offset: 0,
        failed: false,
    }
}

impl<'a> Iterator for LedgerRecords<'a> {
    type Item = Result<LedgerRecord<'a>, CaptureError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.offset >= self.bytes.len() {
            return None;
        }
        let result = read_record(self.bytes, self.offset);
        match &result {
            Ok((_, end)) => self.offset = *end,
            Err(_) => self.failed = true,
        }
        Some(result.map(|(record, _)| record))
    }
}

/// The record at `offset` and the offset after it.
fn read_record(bytes: &[u8], offset: usize) -> Result<(LedgerRecord<'_>, usize), CaptureError> {
    let header = bytes
        .get(offset..offset + RECORD_HEADER_LEN)
        .filter(|header| header[..MAGIC_END] == LEDGER_MAGIC)
        .ok_or(CaptureError::BadRecord { offset })?;
    let time_ms = le_u64(&header[MAGIC_END..TIME_END]);
    let direction = le_u32(&header[TIME_END..DIRECTION_END]);
    let context = le_u64(&header[DIRECTION_END..CONTEXT_END]);
    let length = le_u32(&header[CONTEXT_END..RECORD_HEADER_LEN]);
    if direction != SERVER_TO_CLIENT && direction != CLIENT_TO_SERVER {
        return Err(CaptureError::BadDirection { offset, direction });
    }
    let start = offset + RECORD_HEADER_LEN;
    let end = usize::try_from(length)
        .ok()
        .and_then(|length| start.checked_add(length))
        .filter(|end| *end <= bytes.len())
        .ok_or(CaptureError::BadRecord { offset })?;
    let record = LedgerRecord {
        time_ms,
        direction,
        context,
        datagram: &bytes[start..end],
    };
    Ok((record, end))
}

/// Splits a ledger into its records.
///
/// # Errors
///
/// Returns [`CaptureError`] for a record that is cut short, lacks the magic, or has an unknown
/// direction.
pub fn parse_ledger(bytes: &[u8]) -> Result<Vec<LedgerRecord<'_>>, CaptureError> {
    ledger_records(bytes).collect()
}

/// Whether a datagram carries a carrier connect request.
pub fn is_connect_request(datagram: &[u8]) -> bool {
    let Ok(datagram) = decode_datagram(datagram) else {
        return false;
    };
    let Ok(frames) = decode_frames(&datagram.body) else {
        return false;
    };
    frames.iter().any(|frame| {
        frame.channel == SYSTEM_CHANNEL
            && matches!(
                SystemMessage::decode(&frame.payload),
                Ok(SystemMessage::ConnectRequest { .. })
            )
    })
}

/// A little-endian number from up to 8 bytes.
fn le_u64(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .rev()
        .fold(0, |value, byte| value << BITS_PER_BYTE | u64::from(*byte))
}

/// A little-endian number from up to 4 bytes.
fn le_u32(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .rev()
        .fold(0, |value, byte| value << BITS_PER_BYTE | u32::from(*byte))
}

/// Writes ledger records.
#[derive(Debug)]
pub struct LedgerWriter<W: Write> {
    out: W,
}

impl<W: Write> LedgerWriter<W> {
    /// A writer into `out`.
    pub fn new(out: W) -> Self {
        Self { out }
    }

    /// Writes one record.
    ///
    /// # Errors
    ///
    /// Returns the write error, or [`io::ErrorKind::InvalidInput`] for a datagram longer than
    /// `u32::MAX` bytes.
    pub fn record(
        &mut self,
        time_ms: u64,
        direction: u32,
        context: u64,
        datagram: &[u8],
    ) -> io::Result<()> {
        let length = u32::try_from(datagram.len())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        let mut header = [0_u8; RECORD_HEADER_LEN];
        header[..MAGIC_END].copy_from_slice(&LEDGER_MAGIC);
        header[MAGIC_END..TIME_END].copy_from_slice(&time_ms.to_le_bytes());
        header[TIME_END..DIRECTION_END].copy_from_slice(&direction.to_le_bytes());
        header[DIRECTION_END..CONTEXT_END].copy_from_slice(&context.to_le_bytes());
        header[CONTEXT_END..].copy_from_slice(&length.to_le_bytes());
        self.out.write_all(&header)?;
        self.out.write_all(datagram)
    }

    /// Flushes the output.
    ///
    /// # Errors
    ///
    /// Returns the flush error.
    pub fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }

    /// The output.
    pub fn into_inner(self) -> W {
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTEXT: u64 = 7;
    const OTHER_CONTEXT: u64 = 9;
    const TIME_MS: u64 = 1_782_067_831_638;
    const BAD_DIRECTION: u32 = 2;
    const CUT_BYTES: usize = 10;
    const DIRECTION_AT: std::ops::Range<usize> = 12..16;

    fn written(records: &[(u64, u32, u64, &[u8])]) -> Vec<u8> {
        let mut writer = LedgerWriter::new(Vec::new());
        for (time_ms, direction, context, datagram) in records {
            writer
                .record(*time_ms, *direction, *context, datagram)
                .expect("write");
        }
        writer.into_inner()
    }

    #[test]
    fn a_written_ledger_reads_back() {
        let ledger = written(&[
            (TIME_MS, SERVER_TO_CLIENT, CONTEXT, b"one"),
            (TIME_MS + 1, CLIENT_TO_SERVER, CONTEXT, b""),
            (TIME_MS + 2, SERVER_TO_CLIENT, OTHER_CONTEXT, b"three"),
        ]);
        let records = parse_ledger(&ledger).expect("parse");
        assert_eq!(
            records,
            vec![
                LedgerRecord {
                    time_ms: TIME_MS,
                    direction: SERVER_TO_CLIENT,
                    context: CONTEXT,
                    datagram: b"one",
                },
                LedgerRecord {
                    time_ms: TIME_MS + 1,
                    direction: CLIENT_TO_SERVER,
                    context: CONTEXT,
                    datagram: b"",
                },
                LedgerRecord {
                    time_ms: TIME_MS + 2,
                    direction: SERVER_TO_CLIENT,
                    context: OTHER_CONTEXT,
                    datagram: b"three",
                },
            ]
        );
    }

    #[test]
    fn parses_ledger_records() {
        let ledger = written(&[
            (TIME_MS, SERVER_TO_CLIENT, CONTEXT, b"server"),
            (TIME_MS, CLIENT_TO_SERVER, CONTEXT, b"client"),
        ]);
        assert_eq!(parse_ledger(&ledger).expect("parse").len(), 2);
        assert!(matches!(
            parse_ledger(&ledger[..ledger.len() - 1]),
            Err(CaptureError::BadRecord { .. })
        ));
        let mut bad = ledger.clone();
        bad[DIRECTION_AT].copy_from_slice(&BAD_DIRECTION.to_le_bytes());
        assert!(matches!(
            parse_ledger(&bad),
            Err(CaptureError::BadDirection { .. })
        ));
    }

    #[test]
    fn a_cut_ledger_gives_the_records_before_the_cut() {
        let mut ledger = written(&[
            (TIME_MS, SERVER_TO_CLIENT, CONTEXT, b"one"),
            (TIME_MS, CLIENT_TO_SERVER, CONTEXT, b"two"),
        ]);
        let third = written(&[(TIME_MS, SERVER_TO_CLIENT, CONTEXT, b"three")]);
        ledger.extend_from_slice(&third[..CUT_BYTES]);
        let mut records = ledger_records(&ledger);
        assert!(records.next().expect("first").is_ok());
        assert!(records.next().expect("second").is_ok());
        assert!(matches!(
            records.next(),
            Some(Err(CaptureError::BadRecord { .. }))
        ));
        assert!(records.next().is_none());
    }

    #[test]
    fn a_wrong_magic_is_an_error() {
        let mut ledger = written(&[(TIME_MS, SERVER_TO_CLIENT, CONTEXT, b"one")]);
        ledger[0] = b'X';
        assert!(matches!(
            parse_ledger(&ledger),
            Err(CaptureError::BadRecord { offset: 0 })
        ));
    }
}
