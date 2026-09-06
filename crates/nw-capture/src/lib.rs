// SPDX-License-Identifier: AGPL-3.0-or-later
//! Capture ledgers: records of decrypted carrier datagrams with their time, direction and
//! connection context (carrier captures note), and the class names of message types.
//!
//! The ledgers hold other players' data. Code here reads them; it prints nothing.

pub mod ledger;
pub mod names;

pub use ledger::{
    CLIENT_TO_SERVER, CaptureError, LEDGER_MAGIC, LedgerRecord, LedgerRecords, LedgerWriter,
    RECORD_HEADER_LEN, SERVER_TO_CLIENT, is_connect_request, ledger_records, parse_ledger,
};
pub use names::TypeNames;
