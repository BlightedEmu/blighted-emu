// SPDX-License-Identifier: AGPL-3.0-or-later
//! GM commands from the admin tool to the sim that owns the target (admin studio spec section
//! 8.1), request/reply on [`crate::subject::sim_admin`].

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::message::Point;

/// One GM command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdminCommand {
    /// Id of the request, as in the audit log.
    pub request: Uuid,
    /// The account that gave the command; `None` in local mode.
    pub actor: Option<Uuid>,
    /// What to do.
    pub action: AdminAction,
}

/// What a GM command does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AdminAction {
    /// Moves the player with this entity number to `to`.
    TeleportPlayer {
        /// The player's entity number (as in the sim snapshot).
        entity: u64,
        /// Where to, in metres.
        to: Point,
    },
}

/// The sim's answer to an [`AdminCommand`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdminReply {
    /// Done.
    Done,
    /// The target is not on this sim (it left, or moved to another sim).
    NotMine,
    /// The sim refused the command.
    Failed {
        /// Why, for the page and the audit log.
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn commands_and_replies_round_trip() {
        let command = AdminCommand {
            request: Uuid::from_u128(1),
            actor: Some(Uuid::from_u128(2)),
            action: AdminAction::TeleportPlayer {
                entity: 7,
                to: Point {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
            },
        };
        assert_eq!(
            decode::<AdminCommand>(&encode(&command).unwrap()).unwrap(),
            command
        );
        for reply in [
            AdminReply::Done,
            AdminReply::NotMine,
            AdminReply::Failed {
                reason: "off the level".to_owned(),
            },
        ] {
            assert_eq!(
                decode::<AdminReply>(&encode(&reply).unwrap()).unwrap(),
                reply
            );
        }
    }
}
