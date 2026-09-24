// SPDX-License-Identifier: AGPL-3.0-or-later
//! `social` (run 15 social study): one per world. It keeps the world's social state
//! ([`nw_social::SocialWorld`]), takes the social calls and the presence the sims send, and
//! sends each sim the messages for its players.

pub mod config;
pub mod lists;
pub mod run;
