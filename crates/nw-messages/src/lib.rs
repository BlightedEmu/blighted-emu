// SPDX-License-Identifier: AGPL-3.0-or-later
//! Body codecs of the game messages of M4 (M4 spec section 5).
//!
//! A body is the part of an envelope after the type index. Layouts come only from the RE notes:
//! `docs/re/2026-09-04-bootstrap-message-bodies.md` for the messages and
//! `docs/re/2026-09-02-replication-and-movement.md` for the replicated states. A field without a
//! name in the notes keeps a neutral name. `decode` fails on missing bytes and on bytes left over.

pub mod bundle;
pub mod client_add_entry;
pub mod client_input;
pub mod connected;
pub mod day_phase;
pub mod payload;
pub mod position;
pub mod server;
pub mod spawn_point;
#[cfg(any(test, feature = "test-util"))]
pub mod test_support;
pub mod time_change;
pub mod time_synch;
pub mod wire;

pub use bundle::{
    IndexBlock, MAX_BUNDLE_INDEXES, OBSERVED_BUNDLE_BYTE_3, OBSERVED_BUNDLE_BYTE_4,
    OBSERVED_BUNDLE_GROUP, REPLICATED_STATE_BUNDLE_TYPE_INDEX, ReplicatedStateBundle,
};
pub use client_add_entry::{CLIENT_ADD_ENTRY_MSG_TYPE_INDEX, ClientAddEntryMsg};
pub use client_input::{
    Bit8, ClientInputMsg, HeadStep9, INPUT_Z_MAX, INPUT_Z_MIN, InputHead, InputPosition,
    InputRecord, InputSlots, RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX,
};
pub use connected::{
    Block1415acc30, CALENDAR_CONNECTED_MSG_TYPE_INDEX, CalendarConnectedMsg, ConnectedBody,
    TIMING_CONFIGURATION_CONNECTED_MSG_TYPE_INDEX, TimingConfigurationConnectedMsg,
};
pub use day_phase::{OBSERVED_DAY_PHASE, SYNC_DAY_PHASE_MSG_TYPE_INDEX, SyncDayPhaseMsg};
pub use payload::{BundleGroup, BundleMember, BundlePayload, KnownMembers, MemberState};
pub use position::{
    POSITION_IN_THE_WORLD_TYPE_INDEX, POSITION_Z_MAX, POSITION_Z_MIN,
    PositionInTheWorldReplicatedState, Rotation, RotationComponent, Scale, WorldPosition,
};
pub use server::{LOGIN_MESSAGE_TYPES, SERVER_MESSAGE_TYPES, ServerMessage};
pub use spawn_point::{RECEIVE_PLAYER_SPAWN_POINT_MSG_TYPE_INDEX, ReceivePlayerSpawnPointMsg};
pub use time_change::{
    OBSERVED_TIME_CHANGE_F32S_AT_16, OBSERVED_TIME_CHANGE_F32S_AT_60,
    OBSERVED_TIME_CHANGE_U32S_AT_28, SYNC_TIME_CHANGE_MSG_TYPE_INDEX, SyncTimeChangeMsg,
};
pub use time_synch::TimeSynchMsg;
pub use wire::{BodyCodec, MessageError, RAW_ID_LEN, Reader, Writer};
