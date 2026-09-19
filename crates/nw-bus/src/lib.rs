// SPDX-License-Identifier: AGPL-3.0-or-later
//! The bus between `rep-gateway`, `sim` and `world-director` (M4 spec section 6, M5 spec
//! section 5, parent spec section 7.3): every
//! NATS subject and every bus message type is defined here, so both sides agree. Core NATS only,
//! with request/reply for `join`. Bus messages are `postcard` bytes.

pub mod admin;
pub mod client;
pub mod id;
pub mod message;
pub mod publisher;
pub mod snapshot;
pub mod subject;
#[cfg(any(test, feature = "test-util"))]
pub mod test_support;

pub use admin::{AdminAction, AdminCommand, AdminReply};
pub use client::{Bus, BusError, Incoming, Subscription};
pub use id::{BusId, BusIdError, MAX_BUS_ID_LEN};
pub use message::{
    Admit, AdmitReply, CarriedItem, CellTable, ConnectionRef, GatewayOutput, Ghost, Ghosts,
    HandoffMessage, HandoffOffer, HandoffReply, Heartbeat, Hit, InputBatch, ItemPlace, Join,
    JoinReply, MAX_BATCH_BYTES, MAX_BUS_MESSAGE_BYTES, MAX_GHOST_BYTES, MOB_ENTITY_BASE, MobGhost,
    OutputBatch, PlayerQuests, Point, QuestRun, QuestTask, ResumeOf, SimHeartbeat, SimInput,
    SimSlot, decode, encode, heartbeat_silence, split_batches,
};
pub use publisher::Publisher;
pub use snapshot::{
    MOB_STATES, SNAPSHOT_PART_ENTITIES, SimSnapshot, SnapMob, SnapPlayer, snapshot_parts,
};
