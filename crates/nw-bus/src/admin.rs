// SPDX-License-Identifier: AGPL-3.0-or-later
//! GM commands from the admin tool to the sim that owns the target (admin studio spec section
//! 8.1), request/reply on [`crate::subject::sim_admin`].

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::id::BusId;
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
    /// Saves the player and closes its connection.
    Kick {
        /// The player.
        entity: u64,
    },
    /// Fills the player's health; brings a dead player back.
    Heal {
        /// The player.
        entity: u64,
    },
    /// Takes all of the player's health.
    Kill {
        /// The player.
        entity: u64,
    },
    /// Turns damage to the player off or on.
    GodMode {
        /// The player.
        entity: u64,
        /// True: no damage.
        on: bool,
    },
    /// Puts items into the player's bag.
    GiveItem {
        /// The player.
        entity: u64,
        /// The item id (`MasterItemDefinitions` `ItemID`).
        item: String,
        /// How many.
        quantity: u32,
    },
    /// Starts a quest for the player (one at a time; a done quest only when repeatable).
    StartQuest {
        /// The player.
        entity: u64,
        /// The objective id (`Objectives` `ObjectiveID`).
        objective: String,
    },
    /// Takes every stack of an item from the player.
    TakeItem {
        /// The player.
        entity: u64,
        /// The item id.
        item: String,
    },
    /// Spawns `count` GM mobs of a mob type around a point.
    SpawnMobs {
        /// The vitals id (`VitalsBaseData` `VitalsID`).
        vitals: String,
        /// Their level.
        level: u32,
        /// Where, in metres.
        at: Point,
        /// How many.
        count: u32,
    },
    /// Removes GM mobs: these entities, or every GM mob of the sim when empty.
    DespawnMobs {
        /// The mobs; empty: all GM mobs here.
        entities: Vec<u64>,
    },
    /// Starts an event of the design data at a point (admin studio spec section 8.3).
    StartEvent {
        /// The event id.
        event: String,
        /// Its point, in metres.
        at: Point,
    },
    /// Stops a running event and removes its mobs.
    StopEvent {
        /// The event id.
        event: String,
    },
    /// The events running on the sim.
    ListEvents,
}

/// One running event of a sim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningEvent {
    /// The event id.
    pub event: String,
    /// Unix ms of its start.
    pub started_ms: u64,
    /// Its mobs alive now.
    pub mobs_alive: u32,
    /// Its waves still to spawn.
    pub waves_left: u32,
}

/// One block a running script passed (admin studio spec section 7.5, the run log).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptLogLine {
    /// Unix ms.
    pub time_ms: u64,
    /// The script id.
    pub script: String,
    /// The node id.
    pub node: String,
    /// The node's block.
    pub block: String,
    /// The run's player, when it has one.
    pub player: Option<u64>,
    /// What happened, when more than "passed".
    pub note: String,
}

/// Sim to admin: the run log lines of one tick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptLog {
    /// The sim.
    pub sim: BusId,
    /// The lines in order.
    pub lines: Vec<ScriptLogLine>,
}

/// The sim's answer to an [`AdminCommand`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdminReply {
    /// Done.
    Done,
    /// Done; the GM mobs it spawned.
    Spawned {
        /// Their entity numbers.
        entities: Vec<u64>,
    },
    /// The target is not on this sim (it left, or moved to another sim).
    NotMine,
    /// The sim refused the command.
    Failed {
        /// Why, for the page and the audit log.
        reason: String,
    },
    /// The events running on the sim.
    Events {
        /// The events.
        running: Vec<RunningEvent>,
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
            AdminReply::Spawned {
                entities: vec![1, 2],
            },
            AdminReply::Events {
                running: vec![RunningEvent {
                    event: "town_invasion".to_owned(),
                    started_ms: 5,
                    mobs_alive: 3,
                    waves_left: 1,
                }],
            },
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
