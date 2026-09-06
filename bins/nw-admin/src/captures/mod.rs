// SPDX-License-Identifier: AGPL-3.0-or-later
//! The capture viewer (admin tools spec section 4): the session list and the type names.

pub mod api;
pub mod azoth;
pub mod catalog;
pub mod open;

use std::path::Path;

use nw_capture::TypeNames;

use crate::config::CapturesConfig;
use azoth::{AzothTypes, TypeLabels};
use catalog::{SessionEntry, scan};

/// The sessions and the type labels, read at start.
#[derive(Debug, Default)]
pub struct Captures {
    /// Every session, sorted by id.
    pub sessions: Vec<SessionEntry>,
    /// Type labels.
    pub labels: TypeLabels,
    /// The open sessions.
    pub open: open::OpenSessions,
}

impl Captures {
    /// Scans the captures folder and reads the type names. Our names need the install folder
    /// (`game_dir`, for its `typeindex.json`); without it, or when a file cannot be read, only
    /// the Azoth catalog's names are used.
    pub fn load(config: &CapturesConfig, game_dir: Option<&Path>) -> Self {
        let ours = game_dir.and_then(|dir| {
            TypeNames::load(&dir.join(&config.type_index), &config.type_names)
                .inspect_err(|error| tracing::warn!(%error, "our type names skipped"))
                .ok()
        });
        let sessions = scan(&config.captures_dir, &config.discord_dir);
        tracing::info!(sessions = sessions.len(), "capture sessions found");
        Self {
            sessions,
            labels: TypeLabels::new(ours, AzothTypes::load(&config.azoth_dir)),
            open: open::OpenSessions::default(),
        }
    }
}
