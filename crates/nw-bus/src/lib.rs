// SPDX-License-Identifier: AGPL-3.0-or-later
//! The bus between `rep-gateway` and `sim` (M4 spec section 6, parent spec section 7.3): every
//! NATS subject and every bus message type is defined here, so both sides agree. Core NATS only,
//! with request/reply for `join`. Bus messages are `postcard` bytes.

pub mod client;
pub mod id;
pub mod message;
pub mod subject;
#[cfg(any(test, feature = "test-util"))]
pub mod test_support;

pub use client::{Bus, BusError, Incoming, Subscription};
pub use id::{BusId, BusIdError, MAX_BUS_ID_LEN};
pub use message::{
    ConnectionRef, GatewayOutput, Heartbeat, Join, JoinReply, SimInput, decode, encode,
};
