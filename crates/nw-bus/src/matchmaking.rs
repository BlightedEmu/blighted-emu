// SPDX-License-Identifier: AGPL-3.0-or-later
//! The bus of the matchmaking queue (the 3v3 arena; arena flow study 2026-11-11): the players of
//! every open-world sim queue at the director, which forms matches, collects the accepts, opens
//! the arena instance on a free instance sim (the instance path of [`crate::instance`]) and
//! tells each player's sim where its player goes.
//!
//! - A sim sends its players' queue calls to the director ([`QueueCall`] on
//!   [`crate::subject::director_queue`], publish).
//! - The director sends what happened to a queued player to that player's sim ([`QueueNews`] on
//!   [`crate::subject::sim_queue`], publish): the pop, the accept count, the cancel of a match
//!   not every player accepted, the drop of a player who did not accept, the invite to an open
//!   Outpost Rush match, the match of a catacombs player with no pop, and the opened match.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::id::BusId;
use crate::message::SimSlot;

/// A player joins the queue of a game mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueJoin {
    /// The character.
    pub character: Uuid,
    /// The sim the player is on.
    pub sim: BusId,
    /// Its group, when it queued with one: a group plays in one team.
    pub group: Option<Uuid>,
    /// The role it picked (6272).
    pub role: u8,
    /// The game mode (`GameModeID`).
    pub game_mode: String,
    /// The level folder of the map the game mode plays now (its map rotation).
    pub level: String,
    /// The last byte of the 6254: the player waits for other queued players to fill its team.
    /// Observed: 1 in the 6 arena group joins and in the 4 catacombs joins that popped with 1
    /// to 3 players (3 at once, fewer about 60 s later); 0 in the 10 arena solo joins and in the
    /// catacombs solo join of `CATACOMBS2` (4.546 s), matched alone with no pop. Inferred: the
    /// meaning (a fill request); a mode with a fill wait reads it.
    pub fill: bool,
}

/// A sim to the director: one queue call of a player.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueueCall {
    /// The player queues.
    Join(QueueJoin),
    /// The player leaves the queue (1066, 2385, a logout or a move to another sim).
    Leave {
        /// The character.
        character: Uuid,
    },
    /// The player accepted the match (5741).
    Accept {
        /// The character.
        character: Uuid,
        /// The match.
        match_id: Uuid,
    },
}

/// Where the player of an opened match goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchPlace {
    /// The match.
    pub match_id: Uuid,
    /// The arena instance.
    pub instance: Uuid,
    /// The instance sim.
    pub sim: SimSlot,
    /// The instance's phase id.
    pub phase: u64,
    /// The level folder.
    pub level: String,
    /// The player's team (0 or 1).
    pub team: u8,
    /// The player's place in its team, from 0.
    pub place: u8,
}

/// What happened to a queued player, for its sim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueueNewsKind {
    /// A match was found: the player may accept it.
    Popped {
        /// The match.
        match_id: Uuid,
        /// Its players.
        size: u32,
    },
    /// `count` players of the player's match accepted it.
    Accepted {
        /// Players that accepted.
        count: u32,
    },
    /// The match was cancelled (not every player accepted); the player stays queued.
    Cancelled,
    /// The player did not accept in time: it left the queue.
    Dropped,
    /// The instance of a match whose players move one by one (Outpost Rush) is open: the
    /// player may accept now.
    Invited,
    /// The player was matched with no pop and its match is open: it may accept now (Observed:
    /// the catacombs solo joins, status 1 and the match id at once 4.8 and 5.6 s after the
    /// queue entry, `CATACOMBS2` 11.805 s, `CATACOMBS` 8.055 s).
    Matched {
        /// The match.
        match_id: Uuid,
    },
    /// The player may move to its match: every player accepted the arena match, or the player
    /// accepted the open Outpost Rush match.
    Ready(MatchPlace),
}

/// The director to a sim: what happened to its queued player `character`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueNews {
    /// The character.
    pub character: Uuid,
    /// The game mode of its queue.
    pub game_mode: String,
    /// What happened.
    pub kind: QueueNewsKind,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn queue_calls_and_news_round_trip() {
        let sim = BusId::try_from("sim-1".to_owned()).expect("id");
        let join = QueueCall::Join(QueueJoin {
            character: Uuid::from_u128(1),
            sim: sim.clone(),
            group: Some(Uuid::from_u128(2)),
            role: 3,
            game_mode: "Arena3v3".to_owned(),
            level: "nw_arena01".to_owned(),
            fill: true,
        });
        let bytes = encode(&join).expect("encode");
        assert_eq!(decode::<QueueCall>(&bytes).expect("decode"), join);
        let news = QueueNews {
            character: Uuid::from_u128(1),
            game_mode: "Arena3v3".to_owned(),
            kind: QueueNewsKind::Ready(MatchPlace {
                match_id: Uuid::from_u128(3),
                instance: Uuid::from_u128(4),
                sim: SimSlot {
                    id: sim,
                    generation: 7,
                },
                phase: 9,
                level: "nw_arena01".to_owned(),
                team: 1,
                place: 2,
            }),
        };
        let bytes = encode(&news).expect("encode");
        assert_eq!(decode::<QueueNews>(&bytes).expect("decode"), news);
        let matched = QueueNews {
            character: Uuid::from_u128(1),
            game_mode: "Catacombs".to_owned(),
            kind: QueueNewsKind::Matched {
                match_id: Uuid::from_u128(5),
            },
        };
        let bytes = encode(&matched).expect("encode");
        assert_eq!(decode::<QueueNews>(&bytes).expect("decode"), matched);
    }
}
