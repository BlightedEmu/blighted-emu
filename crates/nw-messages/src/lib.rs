// SPDX-License-Identifier: AGPL-3.0-or-later
//! Body codecs of the game messages of M4 (M4 spec section 5).
//!
//! A body is the part of an envelope after the type index. Layouts come only from the RE notes:
//! `docs/re/2026-09-04-bootstrap-message-bodies.md` for the messages here. A field without a
//! name in the notes keeps a neutral name. `decode` fails on missing bytes and on bytes left over.

pub mod client_add_entry;
pub mod day_phase;
pub mod spawn_point;
pub mod time_synch;
pub mod wire;

pub use client_add_entry::{CLIENT_ADD_ENTRY_MSG_TYPE_INDEX, ClientAddEntryMsg};
pub use day_phase::{OBSERVED_DAY_PHASE, SYNC_DAY_PHASE_MSG_TYPE_INDEX, SyncDayPhaseMsg};
pub use spawn_point::{RECEIVE_PLAYER_SPAWN_POINT_MSG_TYPE_INDEX, ReceivePlayerSpawnPointMsg};
pub use time_synch::TimeSynchMsg;
pub use wire::{BodyCodec, MessageError, RAW_ID_LEN, Reader, Writer};
