// SPDX-License-Identifier: AGPL-3.0-or-later
//! `capture-check`: decodes local capture ledgers with `nw-carrier` and `nw-reflect` (M3 spec
//! section 9; carrier captures note).
//!
//! The ledgers hold other players' data. This module keeps counts, type indexes, and the
//! registration response values that the RE note records. It never keeps payload bytes, ids,
//! or tokens.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use nw_carrier::{Inbound, ReceiveLimits, Receiver, decode_datagram};
use nw_reflect::{
    Envelope, REGISTRATION_RESPONSE_MSG_TYPE_INDEX, RegistrationResponseMsg, decode_client_message,
    decode_server_messages,
};
use serde::Deserialize;

/// First bytes of every ledger record.
pub const LEDGER_MAGIC: [u8; 4] = *b"NWDL";
/// Ledger direction: server to client.
pub const SERVER_TO_CLIENT: u32 = 0;
/// Ledger direction: client to server.
pub const CLIENT_TO_SERVER: u32 = 1;
/// Bytes of a ledger record header.
pub const RECORD_HEADER_LEN: usize = 28;
/// Unix milliseconds at 2000-01-01 00:00:00 UTC, the zero of the registration clock.
pub const CLOCK_EPOCH_UNIX_MS: u64 = 946_684_800_000;
/// Nanoseconds per millisecond.
pub const NANOS_PER_MILLI: u64 = 1_000_000;

const MAGIC_END: usize = 4;
const TIME_END: usize = 12;
const DIRECTION_END: usize = 16;
const CONTEXT_END: usize = 24;
const DIRECTIONS: usize = 2;
const BITS_PER_BYTE: u32 = 8;

/// Failure to read a ledger or the type name files.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    /// A file cannot be read.
    #[error("cannot read {path}: {source}")]
    Io {
        /// File.
        path: PathBuf,
        /// Cause.
        source: std::io::Error,
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

/// Splits a ledger into its records.
///
/// # Errors
///
/// Returns [`CaptureError`] for a record that is cut short, lacks the magic, or has an unknown
/// direction.
pub fn parse_ledger(bytes: &[u8]) -> Result<Vec<LedgerRecord<'_>>, CaptureError> {
    let mut records = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
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
        records.push(LedgerRecord {
            time_ms,
            direction,
            context,
            datagram: &bytes[start..end],
        });
        offset = end;
    }
    Ok(records)
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

/// Class names by type index, from the client type table and the Aeternum-World name map.
#[derive(Debug, Default)]
pub struct TypeNames {
    names: Vec<Option<String>>,
}

#[derive(Deserialize)]
struct TypeIndexFile {
    #[serde(rename = "typeIndex")]
    type_index: Vec<String>,
}

#[derive(Deserialize)]
struct NameMapFile {
    entries: HashMap<String, NameEntry>,
}

#[derive(Deserialize)]
struct NameEntry {
    name: Option<String>,
}

impl TypeNames {
    /// Joins `typeindex.json` (list of UUIDs) and `uuid_to_class.json` (UUID to name) by UUID.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError`] when a file cannot be read or parsed.
    pub fn load(type_index: &Path, name_map: &Path) -> Result<Self, CaptureError> {
        let type_index: TypeIndexFile = read_json(type_index)?;
        let name_map: NameMapFile = read_json(name_map)?;
        let by_uuid: HashMap<String, String> = name_map
            .entries
            .into_iter()
            .filter_map(|(uuid, entry)| entry.name.map(|name| (normalize_uuid(&uuid), name)))
            .collect();
        Ok(Self {
            names: type_index
                .type_index
                .iter()
                .map(|uuid| by_uuid.get(&normalize_uuid(uuid)).cloned())
                .collect(),
        })
    }

    /// The class name of a type index.
    pub fn name(&self, index: u64) -> Option<&str> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.names.get(index))
            .and_then(Option::as_deref)
    }
}

fn normalize_uuid(uuid: &str) -> String {
    uuid.chars()
        .filter(char::is_ascii_hexdigit)
        .map(|digit| digit.to_ascii_uppercase())
        .collect()
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, CaptureError> {
    let bytes = std::fs::read(path).map_err(|source| CaptureError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| CaptureError::Json {
        path: path.to_path_buf(),
        source,
    })
}

/// A decode failure: the stream of that connection and direction stops there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeFailure {
    /// Ledger file.
    pub file: PathBuf,
    /// Record number in the file.
    pub record: usize,
    /// What failed.
    pub reason: String,
}

/// Values of the registration responses (carrier captures note section 7).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RegistrationSummary {
    /// Responses decoded.
    pub responses: u64,
    /// Count per status value.
    pub statuses: BTreeMap<u32, u64>,
    /// Count per four-byte value.
    pub flags: BTreeMap<[u8; 4], u64>,
    /// Count per server version text.
    pub versions: BTreeMap<String, u64>,
    /// Largest distance between the response clock and the record time, in milliseconds.
    pub max_clock_offset_ms: u64,
    /// Count per session token length.
    pub token_lengths: BTreeMap<usize, u64>,
}

/// Totals over every checked ledger.
#[derive(Debug, Default)]
pub struct CaptureReport {
    /// Ledger files read.
    pub files: usize,
    /// Datagrams by direction.
    pub datagrams: [u64; DIRECTIONS],
    /// Messages by direction.
    pub messages: [u64; DIRECTIONS],
    /// Messages by direction and type index.
    pub types: BTreeMap<(u32, u64), u64>,
    /// Messages whose type is an inline UUID.
    pub inline_uuid_messages: u64,
    /// Registration responses.
    pub registration: RegistrationSummary,
    /// Decode failures.
    pub failures: Vec<DecodeFailure>,
}

impl CaptureReport {
    /// Decodes every record of one ledger into this report.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError`] when the ledger structure itself is broken. Carrier and message
    /// failures are recorded in [`CaptureReport::failures`].
    pub fn check_ledger(&mut self, file: &Path, bytes: &[u8]) -> Result<(), CaptureError> {
        self.files += 1;
        let mut streams: HashMap<(u64, u32), Option<Receiver>> = HashMap::new();
        for (index, record) in parse_ledger(bytes)?.into_iter().enumerate() {
            let direction = record.direction as usize;
            self.datagrams[direction] += 1;
            let stream = streams
                .entry((record.context, record.direction))
                .or_insert_with(|| Some(Receiver::mid_stream(ReceiveLimits::SESSION)));
            let Some(receiver) = stream else {
                continue;
            };
            if let Err(reason) = self.check_record(receiver, &record) {
                self.failures.push(DecodeFailure {
                    file: file.to_path_buf(),
                    record: index,
                    reason,
                });
                *stream = None;
            }
        }
        Ok(())
    }

    fn check_record(
        &mut self,
        receiver: &mut Receiver,
        record: &LedgerRecord<'_>,
    ) -> Result<(), String> {
        let datagram = decode_datagram(record.datagram).map_err(|error| error.to_string())?;
        let delivered = receiver
            .receive(&datagram)
            .map_err(|error| error.to_string())?;
        for item in delivered {
            let Inbound::Message(message) = item else {
                continue;
            };
            let envelopes = if record.direction == CLIENT_TO_SERVER {
                vec![
                    decode_client_message(&message.payload)
                        .map_err(|error| error.to_string())?
                        .envelope,
                ]
            } else {
                decode_server_messages(&message.payload).map_err(|error| error.to_string())?
            };
            for bytes in envelopes {
                let envelope = Envelope::decode(bytes).map_err(|error| error.to_string())?;
                self.count(record, &envelope)?;
            }
        }
        Ok(())
    }

    fn count(&mut self, record: &LedgerRecord<'_>, envelope: &Envelope) -> Result<(), String> {
        self.messages[record.direction as usize] += 1;
        let Some(index) = envelope.type_index() else {
            self.inline_uuid_messages += 1;
            return Ok(());
        };
        *self.types.entry((record.direction, index)).or_default() += 1;
        if record.direction == SERVER_TO_CLIENT && index == REGISTRATION_RESPONSE_MSG_TYPE_INDEX {
            let response = RegistrationResponseMsg::decode(&envelope.body)
                .map_err(|error| error.to_string())?;
            let summary = &mut self.registration;
            summary.responses += 1;
            *summary.statuses.entry(response.uint32_at_0x8).or_default() += 1;
            *summary.flags.entry(response.bytes_at_0x58).or_default() += 1;
            let version = String::from_utf8_lossy(&response.bytes_at_0x38).into_owned();
            *summary.versions.entry(version).or_default() += 1;
            *summary
                .token_lengths
                .entry(response.bytes_at_0x18.len())
                .or_default() += 1;
            let clock_ms = response.uint64 / NANOS_PER_MILLI + CLOCK_EPOCH_UNIX_MS;
            summary.max_clock_offset_ms = summary
                .max_clock_offset_ms
                .max(clock_ms.abs_diff(record.time_ms));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use nw_carrier::{Frame, encode_datagram, encode_frames};
    use nw_reflect::{
        PING_MSG_TYPE_INDEX, REGISTRATION_RESPONSE_OBSERVED_FLAGS, REGISTRATION_STATUS_AUTHORIZED,
        TypeId, encode_client_message, encode_server_messages,
    };

    use super::*;

    const CONTEXT: u64 = 7;
    const TIME_MS: u64 = 1_782_067_831_638;
    const DATAGRAM_SEQUENCE: u16 = 2;
    const RELIABLE_START: u16 = 10;
    const PING_BODY: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 1];
    const ZERO_SESSION: [u8; 16] = [0; 16];
    const TOKEN: &[u8; 32] = b"0123456789ABCDEF0123456789ABCDEF";
    const VERSION: &str = "[TEST].Javelin.1.365.6031.1";
    const CLOCK_OFFSET_MS: u64 = 250;
    const BAD_DIRECTION: u32 = 2;
    const NAMED_SLOT: u64 = 2;
    const UNNAMED_SLOT: u64 = 1;

    fn record(direction: u32, datagram: &[u8]) -> Vec<u8> {
        let mut out = LEDGER_MAGIC.to_vec();
        out.extend_from_slice(&TIME_MS.to_le_bytes());
        out.extend_from_slice(&direction.to_le_bytes());
        out.extend_from_slice(&CONTEXT.to_le_bytes());
        out.extend_from_slice(&u32::try_from(datagram.len()).expect("small").to_le_bytes());
        out.extend_from_slice(datagram);
        out
    }

    fn reliable_datagram(payload: Vec<u8>) -> Vec<u8> {
        let frame = Frame {
            channel: 0,
            reliable: true,
            connecting: false,
            chunks_remaining: None,
            sequence: RELIABLE_START,
            reliable_sequence: RELIABLE_START,
            payload,
        };
        encode_datagram(DATAGRAM_SEQUENCE, &encode_frames(&[frame]).expect("frames"))
    }

    fn envelope(index: u64, body: Vec<u8>) -> Vec<u8> {
        Envelope::message(TypeId::FileSlot(index), body)
            .encode()
            .expect("envelope")
    }

    fn sample_ledger() -> Vec<u8> {
        let response = RegistrationResponseMsg {
            uint32_at_0x8: REGISTRATION_STATUS_AUTHORIZED,
            uint64: (TIME_MS - CLOCK_EPOCH_UNIX_MS + CLOCK_OFFSET_MS) * NANOS_PER_MILLI,
            bytes_at_0x18: TOKEN.to_vec(),
            bytes_at_0x38: VERSION.as_bytes().to_vec(),
            bytes_at_0x58: REGISTRATION_RESPONSE_OBSERVED_FLAGS,
        };
        let registration = envelope(
            REGISTRATION_RESPONSE_MSG_TYPE_INDEX,
            response.encode().expect("response"),
        );
        let ping = envelope(PING_MSG_TYPE_INDEX, PING_BODY.to_vec());
        let server = encode_server_messages(&[&registration, &ping]).expect("server");
        let client = encode_client_message(&ZERO_SESSION, &ping).expect("client");
        let mut ledger = record(SERVER_TO_CLIENT, &reliable_datagram(server));
        ledger.extend(record(CLIENT_TO_SERVER, &reliable_datagram(client)));
        ledger
    }

    #[test]
    fn parses_ledger_records() {
        let ledger = sample_ledger();
        let records = parse_ledger(&ledger).expect("parse");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].time_ms, TIME_MS);
        assert_eq!(records[0].direction, SERVER_TO_CLIENT);
        assert_eq!(records[1].context, CONTEXT);
        assert!(matches!(
            parse_ledger(&ledger[..ledger.len() - 1]),
            Err(CaptureError::BadRecord { .. })
        ));
        let mut bad = ledger.clone();
        bad[TIME_END..DIRECTION_END].copy_from_slice(&BAD_DIRECTION.to_le_bytes());
        assert!(matches!(
            parse_ledger(&bad),
            Err(CaptureError::BadDirection { .. })
        ));
    }

    #[test]
    fn counts_messages_and_registration_values() {
        let mut report = CaptureReport::default();
        report
            .check_ledger(Path::new("sample"), &sample_ledger())
            .expect("check");
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        assert_eq!(report.datagrams, [1, 1]);
        assert_eq!(report.messages, [2, 1]);
        assert_eq!(report.types[&(CLIENT_TO_SERVER, PING_MSG_TYPE_INDEX)], 1);
        let summary = &report.registration;
        assert_eq!(summary.responses, 1);
        assert_eq!(summary.statuses[&REGISTRATION_STATUS_AUTHORIZED], 1);
        assert_eq!(summary.flags[&REGISTRATION_RESPONSE_OBSERVED_FLAGS], 1);
        assert_eq!(summary.versions[VERSION], 1);
        assert_eq!(summary.token_lengths[&TOKEN.len()], 1);
        assert_eq!(summary.max_clock_offset_ms, CLOCK_OFFSET_MS);
    }

    #[test]
    fn a_bad_crc_is_a_failure_and_stops_that_stream() {
        let ping = envelope(PING_MSG_TYPE_INDEX, PING_BODY.to_vec());
        let mut client = encode_client_message(&ZERO_SESSION, &ping).expect("client");
        client[0] ^= 0xFF;
        let mut ledger = record(CLIENT_TO_SERVER, &reliable_datagram(client));
        ledger.extend(record(CLIENT_TO_SERVER, &reliable_datagram(Vec::new())));
        let mut report = CaptureReport::default();
        report
            .check_ledger(Path::new("bad"), &ledger)
            .expect("check");
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].record, 0);
        assert!(report.failures[0].reason.contains("CRC"));
        assert_eq!(report.messages, [0, 0]);
    }

    #[test]
    fn joins_type_names_by_uuid() {
        let dir = tempfile::tempdir().expect("dir");
        let type_index = dir.path().join("typeindex.json");
        let names = dir.path().join("uuid_to_class.json");
        std::fs::write(
            &type_index,
            r#"{"typeIndex":["00000000000000000000000000000000","AAAAAAAA000000000000000000000001","6A379FB80BDD43A1AB3E9843D7BE8CD3"]}"#,
        )
        .expect("write");
        std::fs::write(
            &names,
            r#"{"entries":{"6a379fb8-0bdd-43a1-ab3e-9843d7be8cd3":{"name":"REPClient::PingMsg"}}}"#,
        )
        .expect("write");
        let names = TypeNames::load(&type_index, &names).expect("load");
        assert_eq!(names.name(NAMED_SLOT), Some("REPClient::PingMsg"));
        assert_eq!(names.name(UNNAMED_SLOT), None);
        assert_eq!(names.name(u64::MAX), None);
    }
}
