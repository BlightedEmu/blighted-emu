// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bus message types (M4 spec section 6.1). Game messages travel as envelope bytes; the gateway
//! does not read them.

use std::collections::BTreeMap;
use std::time::Duration;

use nw_messages::{LOOK_CHOICES, RAW_ID_LEN};
use nw_sim_core::Position;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::client::BusError;
use crate::id::BusId;

/// One player connection: the gateway and the gateway's connection id. Ids of two gateways never
/// meet.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionRef {
    /// The gateway.
    pub gateway: BusId,
    /// The connection id inside that gateway.
    pub connection: u64,
}

/// Gateway to director: a player with a checked ticket joins the world, or a player of a lost
/// sim resumes on another (M5 spec sections 3.1 and 3.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Join {
    /// The connection.
    pub connection: ConnectionRef,
    /// The account.
    pub account: Uuid,
    /// The character.
    pub character: Uuid,
    /// A resumed player; `None` for a new login.
    pub resume: Option<ResumeOf>,
}

/// What a resumed player keeps (M5 spec section 3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeOf {
    /// Its entity number.
    pub entity: u64,
    /// The handoff count its new sim starts from: far above the count of the lost sim, so a late
    /// route of the lost sim loses at the gateway.
    pub handoffs: u64,
    /// Bundles the gateway sent to the player so far; the new sim goes on with the sequence.
    pub bundles_sent: u64,
}

/// The answer to [`Join`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JoinReply {
    /// The character is in the world.
    Joined {
        /// The sim that has the player.
        sim: BusId,
        /// That sim's generation.
        generation: u64,
        /// The player's entity number.
        entity: u64,
        /// Whether the account has the game master rank: its client gets the GM flag of the
        /// registration response.
        game_master: bool,
    },
    /// The character cannot join; the gateway sends registration status 1.
    Refused {
        /// Why, for the log.
        reason: String,
    },
}

/// Gateway to sim, after a join.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SimInput {
    /// One client message envelope, unchanged.
    ClientMessage {
        /// The connection.
        connection: ConnectionRef,
        /// The envelope bytes.
        envelope: Vec<u8>,
    },
    /// The connection is gone.
    Left {
        /// The connection.
        connection: ConnectionRef,
    },
}

/// Sim to gateway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GatewayOutput {
    /// Server message envelopes for one connection, on one carrier channel.
    ServerMessages {
        /// The connection id inside the gateway.
        connection: u64,
        /// The carrier channel.
        channel: u8,
        /// Whether the carrier sends them reliably.
        reliable: bool,
        /// The envelope bytes, in order.
        envelopes: Vec<Vec<u8>>,
    },
    /// Close the connection.
    Close {
        /// The connection id inside the gateway.
        connection: u64,
    },
    /// The connection's player moved to another sim by a handoff (M5 spec section 3.3).
    Route {
        /// The connection id inside the gateway.
        connection: u64,
        /// The sim that has the player now.
        sim: BusId,
        /// That sim's generation.
        generation: u64,
        /// The player's handoff count; only a higher one changes the route.
        sequence: u64,
    },
}

/// A world position in metres, as `sim` keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// East.
    pub x: f32,
    /// North.
    pub y: f32,
    /// Up.
    pub z: f32,
}

impl Point {
    /// Whether every coordinate is a finite number.
    pub const fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

impl From<Position> for Point {
    fn from(position: Position) -> Self {
        Self {
            x: position.x,
            y: position.y,
            z: position.z,
        }
    }
}

impl From<Point> for Position {
    fn from(point: Point) -> Self {
        Self {
            x: point.x,
            y: point.y,
            z: point.z,
        }
    }
}

/// Director to sim: take a player (M5 spec section 3.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Admit {
    /// The connection.
    pub connection: ConnectionRef,
    /// The account.
    pub account: Uuid,
    /// The character.
    pub character: Uuid,
    /// The character's name, for the admin snapshot.
    pub name: String,
    /// The entity number the director chose.
    pub entity: u64,
    /// Where the player appears.
    pub position: Point,
    /// Whether to send the login messages; a resumed player already has them.
    pub login: bool,
    /// The handoff count the player starts with: 0, or the count of a resume.
    pub handoffs: u64,
    /// The bundle sequence the player starts with: 0, or the count of a resume.
    pub bundles_sent: u64,
    /// The character's saved look (`nw_messages::PlayerLook` choices); `None` when the saved
    /// creation settings do not give one.
    pub look: Option<[u8; LOOK_CHOICES]>,
    /// The backstory id of the character's archetype pick (`Archetype_Mystic`, ...), which
    /// names its starter kit; `None` when the saved creation settings do not give one.
    pub backstory: Option<String>,
    /// The character's saved items; `None` when it never saved items (it gets its starter kit).
    pub items: Option<Vec<CarriedItem>>,
    /// The character's saved quests; empty when it never saved any.
    pub quests: PlayerQuests,
    /// The character's saved progression; empty when it never saved any.
    #[serde(default)]
    pub progression: PlayerProgression,
    /// Whether the account has the game master rank: the sim takes its client's GM messages.
    #[serde(default)]
    pub game_master: bool,
}

/// The answer to [`Admit`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdmitReply {
    /// The player is in the world.
    Admitted,
    /// The sim cannot take the player.
    Refused {
        /// Why, for the log.
        reason: String,
    },
}

/// A live sim in the cell table. `generation` is its `started_at_ms`; a restart is a new one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SimSlot {
    /// The sim.
    pub id: BusId,
    /// Its start time, Unix milliseconds.
    pub generation: u64,
}

/// The director's cell table (M5 spec section 4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellTable {
    /// Rises with every change of one director run.
    pub version: u64,
    /// Edge length of a cell, in metres.
    pub cell_size_m: f32,
    /// Columns of the grid.
    pub columns: u32,
    /// Rows of the grid.
    pub rows: u32,
    /// The live sims.
    pub sims: Vec<SimSlot>,
    /// The owner of every cell, row by row, as an index into `sims`.
    pub owners: Vec<Option<u16>>,
}

/// Where a carried item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemPlace {
    /// Worn in this paperdoll slot.
    Worn(u32),
    /// In this bag slot.
    Bag(u16),
}

/// One item stack a player carries to another sim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarriedItem {
    /// The item id (`MasterItemDefinitions` `ItemID`).
    pub item_id: String,
    /// Stack size.
    pub quantity: u32,
    /// Where it is.
    pub place: ItemPlace,
}

/// One player moving to another sim (M5 spec section 3.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandoffOffer {
    /// The connection.
    pub connection: ConnectionRef,
    /// The character.
    pub character: Uuid,
    /// The character's name, for the admin snapshot.
    pub name: String,
    /// The entity number; it stays the same.
    pub entity: u64,
    /// The entity's raw id (`GdeRef`), which the client knows it by; it stays the same.
    pub gde_ref: [u8; RAW_ID_LEN],
    /// The last good position.
    pub position: Point,
    /// Milliseconds since the last good move.
    pub move_age_ms: u64,
    /// The movement budget left, in metres.
    pub budget_m: f32,
    /// Bundles sent to the player so far; the sequence goes on.
    pub bundles_sent: u64,
    /// The client's entity slots: each entity in view and its slot (`interestId`). The taker
    /// keeps them, so the client's slots stay tied to the same entities.
    pub slots: Vec<(u64, u16)>,
    /// Slots that left the view and are not stopped on the client yet.
    pub pending_stops: Vec<u16>,
    /// Milliseconds since the last `TimeSynchMsg`.
    pub time_synch_age_ms: u64,
    /// Handoffs of this player so far, this one included; the gateway's route sequence.
    pub handoffs: u64,
    /// The player's health; a handoff never heals (M6-A).
    pub health: u32,
    /// The character's look, as in [`Admit::look`].
    pub look: Option<[u8; LOOK_CHOICES]>,
    /// The character's backstory, as in [`Admit::backstory`].
    pub backstory: Option<String>,
    /// The sequence of the player's last phasing update; the next one goes on from it.
    pub phasing_sequence: u8,
    /// The client-context id of the player's current level; every bundle carries it.
    pub client_context: u8,
    /// Whether the player enters the taker's level from another level (the end of the
    /// tutorial): the taker sends the level change and a new spawn state, with the next
    /// client-context id.
    pub enter_level: bool,
    /// The items the player carries; `None` when the sender kept none, so the taker gives the
    /// login kit of the backstory.
    pub items: Option<Vec<CarriedItem>>,
    /// The main-hand option slot the player holds drawn, when it holds one (live 2026-09-18:
    /// without it the open world took the slot-24 blunderbuss as the weapon in the hand and
    /// dropped every greatsword swing).
    #[serde(default)]
    pub drawn_weapon: Option<u32>,
    /// Whether the account has the game master rank, as in [`Admit::game_master`].
    #[serde(default)]
    pub game_master: bool,
    /// The player's quests: the active one with its run, and the finished ones.
    #[serde(default)]
    pub quests: PlayerQuests,
    /// The player's experience, coins, achievements and loot limit counts. A taker that changes
    /// the level keeps the achievements (retail: the open world's first full achievement state
    /// held the tutorial's 54 bits).
    #[serde(default)]
    pub progression: PlayerProgression,
    /// The version of the newest achievement state the client has; the taker's first full state
    /// goes on above it (Inferred, as for the quest tracker: the tutorial's changes ended at
    /// version 25 and the beach sim sent its first full state at 3; retail's versions only rise,
    /// 12065 in the tutorial and 13586 on the beach, live 2026-09-18).
    #[serde(default)]
    pub achievement_version: u64,
}

/// A player's quests (admin studio spec section 6.3), as a join or a handoff carries them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerQuests {
    /// The active objective (its id) and its run.
    pub active: Option<(String, QuestRun)>,
    /// The ids of the finished objectives.
    pub done: Vec<String>,
    /// The version of the newest quest tracker lists the client has; the next sim's lists go on
    /// above it, with or without an active quest (live 2026-09-18: the tutorial's lists ended at
    /// version 44 and the beach sim started at 2, so the client dropped every beach quest list,
    /// never tracked the accepted quest and never offered its campfire).
    #[serde(default)]
    pub tracker_version: u64,
}

/// A player's progression besides its items and quests (world migration 0006), as a join or a
/// handoff carries it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerProgression {
    /// Its experience points (kills and quest rewards give some).
    pub experience: u32,
    /// Its coins (quest rewards give some).
    pub coins: u64,
    /// Its set achievement bits, ascending.
    pub achievements: Vec<u32>,
    /// Its loot limit counts, ended ones too, by limit id.
    pub loot_limits: Vec<LootLimitCount>,
    /// The total experience of each weapon mastery, by its `CategoricalProgressionData` id
    /// (world migration 0007).
    pub masteries: BTreeMap<String, u32>,
}

/// One loot limit's count of a player (`nw_content::loot_limits`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LootLimitCount {
    /// The lowercase `LootLimitID`.
    pub limit_id: String,
    /// The count.
    pub count: u64,
    /// Unix nanoseconds of its last count.
    pub counted_ns: u64,
    /// Unix nanoseconds when it goes back to 0.
    pub ends_ns: u64,
}

/// A player's run of a quest, as a handoff carries it to the next sim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestRun {
    /// The state of each task of the quest's tree, in tree order.
    pub tasks: Vec<QuestTask>,
    /// The tasks with progress (their tree index), in the order they first had it.
    pub listed: Vec<u32>,
}

/// The state of one task of a [`QuestRun`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuestTask {
    /// Not started, or stopped.
    Idle,
    /// Waiting: kills or items so far, and the time left on its timer.
    Active {
        /// Kills or items so far.
        progress: u32,
        /// Milliseconds until its timer ends; `None` without a timer.
        due_in_ms: Option<u64>,
    },
    /// Done.
    Done,
}

/// Sim to sim, on the taker's handoff subject. Both go on one subject, so NATS keeps their order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HandoffMessage {
    /// Take this player (request/reply).
    Offer(Box<HandoffOffer>),
    /// The offer of this connection timed out or its player left; undo a late take.
    Cancel {
        /// The connection.
        connection: ConnectionRef,
        /// The `handoffs` of that offer; a take of another attempt stays.
        handoffs: u64,
    },
}

/// The answer to [`HandoffMessage::Offer`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HandoffReply {
    /// The player is in the taker's world.
    Accepted,
    /// The taker cannot take the player.
    Refused {
        /// Why, for the log.
        reason: String,
    },
}

/// Largest bus message: the NATS default payload limit (M4 spec section 6.3).
pub const MAX_BUS_MESSAGE_BYTES: usize = 1024 * 1024;

/// First entity number of mobs (M6-A spec section 6.1): a mob is this plus its spawn index, and
/// players stay below it. Every mob number stays below 2^35, the most that the 5-byte prefix code
/// of a bundle member holds.
pub const MOB_ENTITY_BASE: u64 = 1 << 34;

/// Most bytes of one [`Ghost`] in `postcard`: a 10-byte entity number, three 4-byte floats, two
/// option tags, and the larger of a mob's state (five 5-byte numbers and a 1-byte flag) and a
/// player's ([`PlayerGhost`]: about 236 bytes with all its visible items, its view, target and
/// action).
pub const MAX_GHOST_BYTES: usize = 288;

/// The paperdoll slots that show on a character (armor and weapon options; the
/// `nw_messages::is_visible_slot` slots): a [`PlayerGhost`] carries at most one item for each.
pub const GHOST_VISIBLE_ITEMS: usize = 11;

/// One worn item that shows on a player ghost: its slot and item, all that the public paperdoll
/// of another player names (retail sends other players the public slot only: no durability,
/// gear score or instance; replication note).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GhostItem {
    /// The paperdoll slot.
    pub slot: u8,
    /// The client item index.
    pub item_index: u32,
}

/// What other players see of a player near a border (replication note): its look, health,
/// stance, block, facing, clock and the items that show on it, so the players of the other sim
/// see it move, turn, fight and change weapons, not a statue.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlayerGhost {
    /// The character.
    pub character: Uuid,
    /// The saved look.
    pub look: Option<[u8; LOOK_CHOICES]>,
    /// Health now.
    pub health: u32,
    /// Max health.
    pub max_health: u32,
    /// Its facing (yaw, radians), when known.
    pub facing: Option<f32>,
    /// The pitch of its view (radians, positive looks down), when known.
    pub pitch: Option<f32>,
    /// The entity it locks on or homes to (its newest input), when it has one.
    pub target: Option<u64>,
    /// The number of its ALC states from its client's input and the server time (ms since
    /// 2000-01-01) of the first state with it, while its client numbers its input records.
    pub state_number: Option<(u16, u64)>,
    /// It holds block.
    pub blocking: bool,
    /// The server time of its first own state (ms since 2000-01-01): its ALC clock.
    pub spawned_ms: u64,
    /// The version of its gear: a new one is a gear change.
    pub gear_version: u64,
    /// The main-hand option slot drawn.
    pub drawn: Option<u32>,
    /// The drawn weapon is in the hand.
    pub attached: bool,
    /// The items that show on it.
    pub items: [Option<GhostItem>; GHOST_VISIBLE_ITEMS],
    /// The action it plays (its slayer state), when it plays one.
    pub action: Option<GhostSlayer>,
}

/// The slayer state of a player ghost: the action it plays (ALC children 13 to 18; slayer note).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GhostSlayer {
    /// `slayerStateId`, when it changed with the action.
    pub state: Option<u64>,
    /// The ALC state number in which the action started.
    pub started: u16,
    /// `slayerSequenceId`.
    pub sequence: u64,
    /// The second slot's start, when it goes with the action.
    pub second_started: Option<u16>,
    /// The second slot's state (as on the wire) and sequence, when it plays one.
    pub second_playing: Option<(u64, u64)>,
}

/// The state of a mob ghost (M6-A spec section 6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobGhost {
    /// Health now.
    pub health: u32,
    /// Max health.
    pub max_health: u32,
    /// The mob walks back to its spawn and takes no damage.
    pub returning: bool,
    /// crc32 of the lower-case vitals id (the world file's value).
    pub vitals_id: u32,
    /// crc32 of the lower-case vitals category id.
    pub category_id: u32,
    /// The spawn level.
    pub level: u32,
}

/// A read-only copy of an entity near a border.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Ghost {
    /// The entity number.
    pub entity: u64,
    /// Its position.
    pub position: Point,
    /// A mob's state; `None` for a player.
    pub mob: Option<MobGhost>,
    /// A player's state, for the players who see it.
    #[serde(default)]
    pub player: Option<PlayerGhost>,
}

/// A hit on an entity of another sim (M6-A spec section 7.3), sent to the sim that owns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hit {
    /// The target's entity number.
    pub target: u64,
    /// The attacker's entity number.
    pub attacker: u64,
    /// The attacker's sim.
    pub attacker_sim: BusId,
    /// Health it takes.
    pub amount: u32,
    /// Threat it adds.
    pub threat: u64,
}

/// The ghosts of one sim for another, every tick (M5 spec section 3.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ghosts {
    /// The sender.
    pub from: BusId,
    /// Its players near the receiver's cells; an empty list clears the last one.
    pub entities: Vec<Ghost>,
    /// The raw id (`GdeRef`) of each entity in [`Ghosts::entities`], by entity number: the
    /// receiver's clients see the entity by the same id as the owner's.
    pub gde_refs: Vec<(u64, [u8; RAW_ID_LEN])>,
}

/// A sim is alive, with its load for the director.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimHeartbeat {
    /// The sim.
    pub id: BusId,
    /// Unix milliseconds when the sim started.
    pub started_at_ms: u64,
    /// Players in its world.
    pub players: u32,
    /// Players it takes at most.
    pub max_players: u32,
    /// The level it runs (its world-file folder name, for example `newworld_vitaeeterna`). The
    /// director splits the open map over the sims of its own level and sends new characters to
    /// the sims of the tutorial level.
    pub level: String,
}

/// Largest batch, in envelope bytes: well below [`MAX_BUS_MESSAGE_BYTES`] (M5 load spec
/// section 4).
pub const MAX_BATCH_BYTES: usize = 256 * 1024;
/// Bytes of one batch item besides its envelopes: ids, flags and lengths.
const ITEM_OVERHEAD_BYTES: usize = 96;

/// Sim to gateway: the outputs of one tick for one gateway, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputBatch {
    /// The sim that sends them; it hears `Left` for a connection the gateway no longer has.
    pub sim: BusId,
    /// The outputs.
    pub outputs: Vec<GatewayOutput>,
}

/// Gateway (or an old sim, after a handoff) to sim: inputs in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputBatch {
    /// Inputs that an old sim sends on after a handoff; a client message among them is dropped
    /// once the new sim has a direct input of that connection.
    pub forwarded: bool,
    /// The inputs.
    pub inputs: Vec<SimInput>,
}

impl GatewayOutput {
    /// About the bytes of this output in a batch.
    pub fn size_bytes(&self) -> usize {
        let envelopes = match self {
            Self::ServerMessages { envelopes, .. } => envelopes.iter().map(Vec::len).sum(),
            Self::Close { .. } | Self::Route { .. } => 0,
        };
        envelopes + ITEM_OVERHEAD_BYTES
    }
}

impl SimInput {
    /// About the bytes of this input in a batch.
    pub fn size_bytes(&self) -> usize {
        let envelope = match self {
            Self::ClientMessage { envelope, .. } => envelope.len(),
            Self::Left { .. } => 0,
        };
        envelope + ITEM_OVERHEAD_BYTES
    }
}

/// `items` in order, cut into batches of at most [`MAX_BATCH_BYTES`] by `size`. An item larger
/// than that is a batch of its own.
pub fn split_batches<T>(items: Vec<T>, size: impl Fn(&T) -> usize) -> Vec<Vec<T>> {
    let mut batches: Vec<Vec<T>> = Vec::new();
    let mut bytes = 0;
    for item in items {
        let item_bytes = size(&item);
        match batches.last_mut() {
            Some(batch) if bytes + item_bytes <= MAX_BATCH_BYTES => {
                bytes += item_bytes;
                batch.push(item);
            }
            _ => {
                bytes = item_bytes;
                batches.push(vec![item]);
            }
        }
    }
    batches
}

/// A process is alive. `started_at_ms` changes when the process restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {
    /// The process: a gateway id or a sim id.
    pub id: BusId,
    /// Unix milliseconds when the process started.
    pub started_at_ms: u64,
}

/// How long a process may stay silent before it counts as gone: `misses` heartbeat intervals. A
/// huge setting saturates instead of overflowing.
pub fn heartbeat_silence(interval: Duration, misses: u32) -> Duration {
    interval.saturating_mul(misses)
}

/// The `postcard` bytes of a bus message.
///
/// # Errors
///
/// Returns [`BusError::Encode`] when `postcard` fails.
pub fn encode<T: Serialize>(message: &T) -> Result<Vec<u8>, BusError> {
    postcard::to_allocvec(message).map_err(BusError::Encode)
}

/// A bus message from its `postcard` bytes.
///
/// # Errors
///
/// Returns [`BusError::Decode`] for bytes that are not that message.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, BusError> {
    postcard::from_bytes(bytes).map_err(BusError::Decode)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONNECTION: u64 = 7;
    /// A raw id of the dynamic form (bit 0 of the first byte clear).
    const GDE_REF: [u8; RAW_ID_LEN] = [
        0x42, 0x9c, 0x17, 0xe3, 0x05, 0xb8, 0x6a, 0xd1, 0x3f, 0x80, 0x2e, 0xc4, 0x91, 0x0b, 0x7d,
        0x56,
    ];
    const STARTED_AT_MS: u64 = 1_800_000_000_000;
    const MISSES: u32 = 3;

    fn connection() -> ConnectionRef {
        ConnectionRef {
            gateway: BusId::try_from("gw-1".to_owned()).expect("id"),
            connection: CONNECTION,
        }
    }

    fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(message: T) {
        let bytes = encode(&message).expect("encode");
        assert_eq!(decode::<T>(&bytes).expect("decode"), message);
    }

    #[test]
    fn every_bus_message_round_trips() {
        round_trip(Join {
            connection: connection(),
            account: Uuid::new_v4(),
            character: Uuid::new_v4(),
            resume: Some(ResumeOf {
                entity: CONNECTION,
                handoffs: 1 << 32,
                bundles_sent: 7,
            }),
        });
        round_trip(JoinReply::Joined {
            sim: connection().gateway,
            generation: STARTED_AT_MS,
            entity: CONNECTION,
            game_master: true,
        });
        round_trip(JoinReply::Refused {
            reason: "no position".to_owned(),
        });
        round_trip(SimInput::ClientMessage {
            connection: connection(),
            envelope: vec![1, 2, 3],
        });
        round_trip(SimInput::Left {
            connection: connection(),
        });
        round_trip(GatewayOutput::ServerMessages {
            connection: CONNECTION,
            channel: 1,
            reliable: false,
            envelopes: vec![vec![4], vec![5, 6]],
        });
        round_trip(GatewayOutput::Close {
            connection: CONNECTION,
        });
        round_trip(GatewayOutput::Route {
            connection: CONNECTION,
            sim: connection().gateway,
            generation: STARTED_AT_MS,
            sequence: 2,
        });
        round_trip(Heartbeat {
            id: connection().gateway,
            started_at_ms: STARTED_AT_MS,
        });
    }

    fn sim() -> BusId {
        BusId::try_from("sim-1".to_owned()).expect("id")
    }

    const LOOK: [u8; LOOK_CHOICES] = [0, 4, 0, 64, 43, 4, 4, 11, 0, 5, 25, 14];

    const POINT: Point = Point {
        x: 1.5,
        y: -2.0,
        z: 300.25,
    };

    /// A progression with every field set: the tutorial's experience and achievements, and the
    /// retail newchar loot limit count.
    fn progression() -> PlayerProgression {
        PlayerProgression {
            experience: 1_250,
            coins: 375,
            achievements: vec![14_809, 15_317],
            loot_limits: vec![LootLimitCount {
                limit_id: "2hwarhammer_ftue_corruptedsailer_v2".to_owned(),
                count: 1,
                counted_ns: 1_792_696_951_894_621_000,
                ends_ns: 1_793_091_600_894_621_000,
            }],
            masteries: BTreeMap::from([("GreatSwordAbilityTable".to_owned(), 20)]),
        }
    }

    #[test]
    fn every_m5_message_round_trips() {
        round_trip(Admit {
            connection: connection(),
            account: Uuid::new_v4(),
            character: Uuid::new_v4(),
            name: "Ada".to_owned(),
            entity: CONNECTION,
            position: POINT,
            login: true,
            handoffs: 0,
            bundles_sent: 0,
            look: Some(LOOK),
            backstory: Some("Archetype_Mystic".to_owned()),
            items: Some(vec![CarriedItem {
                item_id: "PotionHealthT1".to_owned(),
                quantity: 5,
                place: ItemPlace::Bag(0),
            }]),
            quests: PlayerQuests::default(),
            progression: progression(),
            game_master: false,
        });
        round_trip(AdmitReply::Admitted);
        round_trip(AdmitReply::Refused {
            reason: "full".to_owned(),
        });
        round_trip(CellTable {
            version: 3,
            cell_size_m: 2048.0,
            columns: 2,
            rows: 1,
            sims: vec![SimSlot {
                id: sim(),
                generation: STARTED_AT_MS,
            }],
            owners: vec![Some(0), None],
        });
        let offer = HandoffOffer {
            connection: connection(),
            character: Uuid::new_v4(),
            name: "Ada".to_owned(),
            entity: CONNECTION,
            gde_ref: GDE_REF,
            position: POINT,
            move_age_ms: 40,
            budget_m: 3.5,
            bundles_sent: 900,
            slots: vec![(CONNECTION + 1, 3)],
            pending_stops: vec![4],
            time_synch_age_ms: 1200,
            handoffs: 2,
            health: 500,
            look: None,
            backstory: None,
            phasing_sequence: 2,
            client_context: 1,
            enter_level: false,
            progression: progression(),
            game_master: true,
            achievement_version: 25,
            items: Some(vec![CarriedItem {
                item_id: "PotionHealthT1".to_owned(),
                quantity: 2,
                place: ItemPlace::Worn(14),
            }]),
            drawn_weapon: Some(25),
            quests: PlayerQuests {
                active: Some((
                    "12_GetOffBeach".to_owned(),
                    QuestRun {
                        tasks: vec![
                            QuestTask::Done,
                            QuestTask::Active {
                                progress: 1,
                                due_in_ms: Some(500),
                            },
                            QuestTask::Idle,
                        ],
                        listed: vec![0, 1],
                    },
                )),
                done: vec!["01_SoleSurvivor2".to_owned()],
                tracker_version: 7,
            },
        };
        round_trip(HandoffMessage::Offer(Box::new(offer)));
        round_trip(HandoffMessage::Cancel {
            connection: connection(),
            handoffs: 2,
        });
        round_trip(HandoffReply::Accepted);
        round_trip(HandoffReply::Refused {
            reason: "already here".to_owned(),
        });
        round_trip(Ghosts {
            from: sim(),
            entities: vec![Ghost {
                entity: CONNECTION,
                position: POINT,
                mob: None,
                player: None,
            }],
            gde_refs: vec![(CONNECTION, GDE_REF)],
        });
        round_trip(SimHeartbeat {
            id: sim(),
            started_at_ms: STARTED_AT_MS,
            players: 10,
            max_players: 2000,
            level: "newworld_vitaeeterna".to_owned(),
        });
    }

    #[test]
    fn batches_round_trip() {
        round_trip(OutputBatch {
            sim: sim(),
            outputs: vec![GatewayOutput::Close {
                connection: CONNECTION,
            }],
        });
        round_trip(InputBatch {
            forwarded: true,
            inputs: vec![SimInput::Left {
                connection: connection(),
            }],
        });
    }

    fn output_of(bytes: usize, connection: u64) -> GatewayOutput {
        GatewayOutput::ServerMessages {
            connection,
            channel: 1,
            reliable: false,
            envelopes: vec![vec![0; bytes]],
        }
    }

    #[test]
    fn a_batch_splits_in_order() {
        let third = MAX_BATCH_BYTES / 3;
        let outputs: Vec<GatewayOutput> = (0..7).map(|n| output_of(third, n)).collect();
        let batches = split_batches(outputs.clone(), GatewayOutput::size_bytes);
        assert_eq!(batches.len(), 4, "two outputs per batch");
        assert!(batches.iter().all(|batch| {
            batch.iter().map(GatewayOutput::size_bytes).sum::<usize>() <= MAX_BATCH_BYTES
        }));
        assert_eq!(batches.concat(), outputs, "the order stays");
        let huge = vec![output_of(MAX_BATCH_BYTES * 2, 0), output_of(1, 1)];
        assert_eq!(
            split_batches(huge.clone(), GatewayOutput::size_bytes).len(),
            2
        );
        assert!(split_batches(Vec::<GatewayOutput>::new(), GatewayOutput::size_bytes).is_empty());
        let input = SimInput::ClientMessage {
            connection: connection(),
            envelope: vec![0; third],
        };
        assert!(input.size_bytes() >= third);
    }

    #[test]
    fn the_largest_ghost_fits_its_byte_limit() {
        let ghost = Ghost {
            entity: u64::MAX,
            position: Point {
                x: f32::MAX,
                y: f32::MIN,
                z: f32::NAN,
            },
            mob: Some(MobGhost {
                health: u32::MAX,
                max_health: u32::MAX,
                returning: true,
                vitals_id: u32::MAX,
                category_id: u32::MAX,
                level: u32::MAX,
            }),
            player: None,
        };
        assert!(encode(&ghost).expect("encode").len() <= MAX_GHOST_BYTES);
        round_trip(Ghost {
            position: POINT,
            ..ghost
        });
        let item = GhostItem {
            slot: u8::MAX,
            item_index: u32::MAX,
        };
        let player = Ghost {
            mob: None,
            player: Some(PlayerGhost {
                character: Uuid::from_u128(u128::MAX),
                look: Some([u8::MAX; LOOK_CHOICES]),
                health: u32::MAX,
                max_health: u32::MAX,
                facing: Some(f32::MAX),
                pitch: Some(f32::MAX),
                target: Some(u64::MAX),
                state_number: Some((u16::MAX, u64::MAX)),
                blocking: true,
                spawned_ms: u64::MAX,
                gear_version: u64::MAX,
                drawn: Some(u32::MAX),
                attached: true,
                items: [Some(item); GHOST_VISIBLE_ITEMS],
                action: Some(GhostSlayer {
                    state: Some(u64::MAX),
                    started: u16::MAX,
                    sequence: u64::MAX,
                    second_started: Some(u16::MAX),
                    second_playing: Some((u64::MAX, u64::MAX)),
                }),
            }),
            ..ghost
        };
        assert!(encode(&player).expect("encode").len() <= MAX_GHOST_BYTES);
        round_trip(Ghost {
            position: POINT,
            ..player
        });
    }

    #[test]
    fn hits_round_trip() {
        round_trip(Hit {
            target: MOB_ENTITY_BASE + 7,
            attacker: CONNECTION,
            attacker_sim: sim(),
            amount: 42,
            threat: 43,
        });
    }

    #[test]
    fn a_point_and_a_position_convert_both_ways() {
        let position = Position::from(POINT);
        assert_eq!(
            (position.x, position.y, position.z),
            (POINT.x, POINT.y, POINT.z)
        );
        assert_eq!(Point::from(position), POINT);
    }

    #[test]
    fn a_point_is_finite_only_with_three_numbers() {
        assert!(POINT.is_finite());
        assert!(
            !Point {
                z: f32::INFINITY,
                ..POINT
            }
            .is_finite()
        );
    }

    #[test]
    fn the_silence_limit_is_the_missed_intervals_and_saturates() {
        let second = Duration::from_secs(1);
        assert_eq!(heartbeat_silence(second, MISSES), second * MISSES);
        assert_eq!(heartbeat_silence(Duration::MAX, MISSES), Duration::MAX);
    }

    #[test]
    fn a_bad_gateway_id_or_other_bytes_do_not_decode() {
        let bytes = encode(&"a.b".to_owned()).expect("encode");
        assert!(decode::<BusId>(&bytes).is_err());
        assert!(decode::<Join>(&[0xFF; 3]).is_err());
    }
}
