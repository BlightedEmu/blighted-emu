// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shared protocol types for the Blighted New World emulator.
//!
//! `launcher_api` is the JSON between our launcher and `edge-http`. `game_api` is the JSON the
//! game client reads and writes before character select.
//!
//! The login-queue body is the JSON object `LoginQueueResponse`.
//! `RepAddress` and `Signature` are strings on the nested `Token` object.
//! Absent keys are left out of the JSON. The client keeps a field only when the key is present.
//!
//! Messages inside the carrier (type index, registration) are in `nw-reflect`.

pub mod game_api;
pub mod launcher_api;
pub mod login_queue;

pub use login_queue::{
    LoginQueueReply, LoginQueueRequest, LoginQueueRequestBody, LoginQueueResponse, LoginQueueToken,
    parse_login_queue_request,
};
