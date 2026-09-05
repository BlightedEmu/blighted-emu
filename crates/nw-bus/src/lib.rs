// SPDX-License-Identifier: AGPL-3.0-or-later
//! The bus between `rep-gateway`, `sim` and `world-director` (M4 spec section 6, M5 spec
//! section 5, parent spec section 7.3): every
//! NATS subject and every bus message type is defined here, so both sides agree. Core NATS only,
//! with request/reply for `join`. Bus messages are `postcard` bytes.

pub mod client;
pub mod id;
pub mod message;
pub mod publisher;
pub mod subject;
#[cfg(any(test, feature = "test-util"))]
pub mod test_support;

pub use client::{Bus, BusError, Incoming, Subscription};
pub use id::{BusId, BusIdError, MAX_BUS_ID_LEN};
pub use message::{
    Admit, AdmitReply, CellTable, ConnectionRef, GatewayOutput, Ghost, Ghosts, HandoffMessage,
    HandoffOffer, HandoffReply, Heartbeat, InputBatch, Join, JoinReply, MAX_BATCH_BYTES,
    MAX_BUS_MESSAGE_BYTES, MAX_GHOST_BYTES, OutputBatch, Point, ResumeOf, SimHeartbeat, SimInput,
    SimSlot, decode, encode, heartbeat_silence, split_batches,
};
pub use publisher::Publisher;
