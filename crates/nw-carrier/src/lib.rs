// SPDX-License-Identifier: AGPL-3.0-or-later
//! The GridMate carrier of the New World client as a library with no sockets (M3 spec section 5).
//!
//! Facts come from `docs/re/2026-09-02-carrier-captures.md`. Callers pass in received bytes and
//! the time, and take out bytes to send and delivered messages. Nothing here reads a clock,
//! opens a socket, or starts a task.

pub mod connection;
pub mod datagram;
pub mod error;
pub mod frame;
pub mod receiver;
pub mod sender;
pub mod system;

pub use connection::{Connection, ConnectionState, Event, Role};
pub use datagram::{Datagram, MAX_DATAGRAM_LEN, decode_datagram, encode_datagram};
pub use error::CarrierError;
pub use frame::{Frame, MESSAGE_CHANNEL, SYSTEM_CHANNEL, decode_frames, encode_frames};
pub use receiver::{Inbound, Message, ReceiveLimits, Receiver};
pub use sender::{Delivery, Sender};
pub use system::{Acks, SystemMessage};
