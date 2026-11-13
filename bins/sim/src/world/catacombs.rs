// SPDX-License-Identifier: AGPL-3.0-or-later
//! The catacombs run (`GameModeID` `Catacombs`, map `NW_Catacomb_00`): the layout of each run
//! ([`nw_content::catacombs`]), the start spot of its players, its level frame, and what a
//! player gets when it arrives in the run.
//!
//! Observed (retail `CATACOMBS`, `CATACOMBS2`, `CATACOMBS5`, `CATACOMBS6`, `Hive catacomes test
//! 2`, `Hive catacombs test 4`; seconds from each ledger's first record):
//!
//! - The run's level is generated: the 1635 names `NW_Catacomb_00` with the origin 0, 0 and a
//!   width and height of whole tiles that differ per run (`CATACOMBS2` 15.671 s: 640 x 960;
//!   `CATACOMBS5` 79.345 s: 1088 x 640; `CATACOMBS6` 84.388 s: 1024 x 1088).
//! - The player appears in the start room, not at the map's `TeamTeleportData` (492, 761):
//!   `CATACOMBS2` at about (180, 768), next to the start tile at (192, 768).
//! - Right after the 1635 the player gets 1234 and 4970 of no game mode (`CATACOMBS2` 15.707 s,
//!   `CATACOMBS` 11.712 s, `CATACOMBS5` 79.365 s, `CATACOMBS6` 84.412 s).
//! - Each tile of the run is two entities at the tile's place, 120 m up: a card of the gameplay
//!   tile slice and a card of the art tile slice it spawns, each with its position state
//!   ([`nw_world::catacomb_tiles`]); an unturned tile's position has no rotation, a turned one's
//!   has (`CATACOMBS2` 17.292 to 17.375 s: tiles at x 64 to 384 m and y 576 to 896 m in steps of
//!   64 m, as the player came near them).
//!
//! - Each room's gameplay shows as the cards of the slices its gameplay tile's slots spawn, at
//!   the slots' places ([`nw_world::catacomb_tiles`]; `CATACOMBS2` 17.292 to 17.341 s): a named
//!   enemy room `dg_enc_catacombs_namedenemy_werebeast` at the centre, its point spawners'
//!   mobs, `catacombs_gatherable_levelup` and a checkpoint; a key room `dg_enc_catacombs_key`
//!   and a key gate on one side; a power-up room `tile_powerup_spawner_master` and six
//!   different power-ups at its six points; hallways `dg_enc_catacombs_hallway_waves` or
//!   `_surprise` and a small silver chest; wooden gates on the open sides; each room's
//!   checkpoint `interactobject_checkpoint_catacombs` at its `CheckpointTile`
//!   (12 m from the tile's centre in `tile3_a_gameplay`, turned with the tile).
//!
//! Inferred: the generator rule and its seed are the server's (the client holds only the table
//! readers); this sim seeds the layout with the run's instance id, so the open-world sim that
//! sends a player and the instance sim that runs the run make the same layout. Which slots of a
//! tile a room spawns is the server's rule too: the gameplay tile holds a slot for every kind of
//! room and its `graphBasedSpawning` entity has no fields. This sim spawns the slot named by the
//! room's `AliasCategory1` (`EncounterKey`, `EncounterBoss`, ...; [`room_slots`]) with the other
//! slots the retail rooms of that kind showed, a gate on each open side (a key gate, `LockGate`,
//! where the room or its neighbour is locked), and the checkpoint; it draws one entry of an
//! alias slot and the power-ups with the run's seed. A player arrives at the start room's
//! checkpoint (the retail arrival spot stands at a checkpoint, `CATACOMBS2` 17.292 s at
//! 180, 768).
//!
//! Progress (Observed in the ten retail catacomb ledgers: `CATACOMBS`, `CATACOMBS2`,
//! `CATACOMBS3`, `CATACOMBS5`, `CATACOMBS6`, `catacombshigh` and the four `Hive catacomes` /
//! `Hive catacombs test` ledgers; key names are the CRC-32 of the lowercase name where a name
//! matched):
//!
//! - Every gameplay tile entity holds a game mode state (2343, component 2) whose script state
//!   is its room's state ([`first_room_state`]); its other children are the run's participant
//!   lists, empty spawner, value, timer and raid lists and the linked mode 1 (`CATACOMBS2`
//!   17.307 s). The client's tile script (`CatacombsGameplayTileData`, `FUN_14080d900`) maps the
//!   room kinds `encounterboss`, `encounterenter`, `encounternamedenemy`, `encountergate`,
//!   `encounterkey`, `encounterreward`, `encounterhazard`, `hallway`, `parkour`,
//!   `encounterstart` and `encounterextractionstarttile` to its states.
//! - A room whose encounters end goes to state 7 (Observed: gate, key, hallway, named enemy and
//!   boss rooms; `CATACOMBS2` 53.029 s: the change alone, `010107`); hazard, parkour and reward
//!   rooms keep their state.
//! - The run has its own game mode entity (a card with no asset) whose state holds the run's
//!   script state, the participant lists, the values `dynamicdifficulty`,
//!   `catacombs_respawns_remaining` and one more, the timers and the map of the level (origin,
//!   tile size, size, one picture byte per tile, the visited tiles, the room icons). It starts
//!   in state 0; when the players left the start room (the client's catacombs script handles
//!   the event `AreaStartExited`, `FUN_1464857d0`) it goes to state 1, the start room's tile to
//!   state 0x0e (`CATACOMBS5` 111.065 s, `CATACOMBS6` 102.107 s, `Hive catacomes test 2`
//!   159.311 s) and every participant's flags gain `RestoreVitalsOnExitedGameMode` (3312 child
//!   1 only, `31 02 02`, the same seconds); a player that joins a started run keeps `11 02 02`
//!   (`CATACOMBS`, `CATACOMBS2`).
//! - A tile's bit in `tileVisited` is set when a player goes into it (`CATACOMBS2` 78.839 s: one
//!   byte changed). Icons are added as the rooms are found: the client's tile script adds
//!   `bossicon` and `keyroomicon` on `AreaDiscover` and `hazardroomicon` on `AreaTile`
//!   (`FUN_14080ad40`).

use bevy_ecs::entity::Entity;
use nw_content::catacombs::{CatacombLayout, EAST, NORTH, SIDES, SOUTH, WEST, cell_of, turned};
use nw_content::instances::{CATACOMBS_GAME_MODE, InstanceKind, InstanceMode};
use nw_messages::facet::NO_GAME_MODE;
use nw_messages::game_mode_participant::{GameModeParticipantState, PARTICIPANT_FLAGS_LEN};
use nw_messages::game_mode_state::{GameModeState, GeneratedMap, MapIcon};
use nw_messages::list_delta::{ListChange, ReplicatedList};
use nw_messages::position::{PositionInTheWorldReplicatedState, WorldPosition};
use nw_messages::{BundleMember, MemberState};
use nw_messages::{GameModeServerMsg, LeftGameModeQueueMsg, LevelSpec, ServerMessage};
use nw_sim_core::Position;
use nw_world::catacomb_tiles::{
    CatacombTile, CatacombTiles, TileEncounter, TilePiece, placed_encounter, turned_point,
};
use nw_world::dungeon_scripts::{Encounter, LevelScript};
use uuid::Uuid;

use super::crafting::{Station, dynamic_ref};
use super::dungeon_script::mixed;
use super::encounter_roots::{card, place_member};
use super::game_mode_entity::{
    FIRST_LIST_VERSION, GAME_MODE_COMPONENT, MatchEntry, MatchParticipant, PLAYING_STATUS, full,
    participant_lists,
};
use super::queue::participant_header;
use super::{Body, Player, SimWorld};
use crate::player_content::PlayerContent;

/// The namespace of the `GdeRef`s of the run's tile entities ("catatile").
const TILE_NAMESPACE: u64 = 0x6361_7461_7469_6c65;
/// Quarter turns in a whole turn.
const QUARTER_TURNS: u8 = 4;
/// The slot of a room's checkpoint.
const CHECKPOINT_SLOT: &str = "CheckpointTile";
/// The first part of the slot of a side's gate (`GateNorth`).
const GATE_SLOT: &str = "Gate";
/// The first part of the slot of a side's key gate (`LockGateSouth`).
const LOCK_GATE_SLOT: &str = "LockGate";
/// The last part of a side's slots, by side in the tile's frame (Observed: `GateNorth` at
/// (0, 16.5), `GateSouth` at (0, -16.5), `GateEast` at (16.5, 0) in `tile1_a_gameplay`).
const SIDE_NAMES: [(u8, &str); 4] = [
    (NORTH, "North"),
    (EAST, "East"),
    (SOUTH, "South"),
    (WEST, "West"),
];
/// The salt of the power-up draw of a room, beside its index.
const POWER_UP_SALT: u64 = 0x706f_7765_7275_7073;

/// The room states of a gameplay tile (module text "Progress"). Observed per room kind over the
/// ten retail catacomb ledgers, by the slices each tile's slots spawned.
const GATE_ROOM_STATE: u8 = 0x00;
/// Observed: the tiles of the `hazardroomicon` icons (`CATACOMBS5` 1458.1 s at 640, 448).
const HAZARD_ROOM_STATE: u8 = 0x01;
/// Observed: the parkour tiles `tile2_d` and `tile3_d`.
const PARKOUR_ROOM_STATE: u8 = 0x02;
/// Observed: the tile of `dg_enc_catacombs_key` (`CATACOMBS2` at 256, 768).
const KEY_ROOM_STATE: u8 = 0x06;
/// Observed: the state of every gate, key, hallway, named enemy and boss room whose encounters
/// ended.
const CLEARED_ROOM_STATE: u8 = 0x07;
/// Observed: the tiles of `tile_powerup_spawner_master`.
const REWARD_ROOM_STATE: u8 = 0x09;
/// Observed: the tiles of `dg_enc_catacombs_hallway_waves`, `_surprise` and `_optional`.
const HALLWAY_STATE: u8 = 0x0a;
/// Observed: the tiles of `dg_enc_catacombs_namedenemy_*`.
const NAMED_ENEMY_ROOM_STATE: u8 = 0x0b;
/// Observed: the room of the boss fight (participant flag `IsInBossFight`, `CATACOMBS5`
/// 1576.1 s).
const BOSS_ROOM_STATE: u8 = 0x0c;
/// Observed: the start room's tile once the run started (`CATACOMBS5` 110.631 s).
const START_OPEN_STATE: u8 = 0x0e;
/// Observed: the start room's tile before the run started (`CATACOMBS5` 81.0 s; the tile
/// script keys it `encounterextractionstarttile`).
const START_ROOM_STATE: u8 = 0x10;
/// The state each `RoomType` starts its gameplay tile in.
const FIRST_ROOM_STATES: [(&str, u8); 9] = [
    ("EncounterStart", START_ROOM_STATE),
    ("EncounterBoss", BOSS_ROOM_STATE),
    ("EncounterNamedEnemy", NAMED_ENEMY_ROOM_STATE),
    ("EncounterGate", GATE_ROOM_STATE),
    ("EncounterKey", KEY_ROOM_STATE),
    ("EncounterHazard", HAZARD_ROOM_STATE),
    ("Hallway", HALLWAY_STATE),
    ("Parkour", PARKOUR_ROOM_STATE),
    ("Reward", REWARD_ROOM_STATE),
];
/// The room states that go to [`CLEARED_ROOM_STATE`] when the room's encounters end.
const CLEARABLE_STATES: [u8; 5] = [
    GATE_ROOM_STATE,
    KEY_ROOM_STATE,
    HALLWAY_STATE,
    NAMED_ENEMY_ROOM_STATE,
    BOSS_ROOM_STATE,
];
/// The linked mode of a gameplay tile's game mode state (Observed: 1 on every tile).
const TILE_LINKED_MODE: u8 = 1;
/// The linked mode of the run's own game mode state (Observed: 0).
const RUN_LINKED_MODE: u8 = 0;
/// The run's script state before its start (Observed: `CATACOMBS5` 81.014 s).
const RUN_WAITING_STATE: u8 = 0;
/// The run's script state after its start (Observed: `CATACOMBS5` 111.065 s).
const RUN_STARTED_STATE: u8 = 1;
/// The namespace of the `GdeRef` of the run's own game mode entity ("catarun").
const RUN_NAMESPACE: u64 = 0x6361_7461_7275_6e00;
/// The value `dynamicdifficulty` (Observed: 0 in every retail run).
const DYNAMIC_DIFFICULTY_VALUE: u32 = 0x1873_146b;
/// The value `catacombs_respawns_remaining` (Observed: 6 with two players, 9 with three; one
/// less at each death, `CATACOMBS5` 430.552 s).
const RESPAWNS_REMAINING_VALUE: u32 = 0xbb94_a80d;
/// A value whose name did not match a CRC-32 (Observed: 0 in every retail run).
const THIRD_RUN_VALUE: u32 = 0xb6be_e0a1;
/// The respawns the run holds for each participant at its start (Inferred from 6 for two
/// players and 9 for three).
const RESPAWNS_PER_PARTICIPANT: u64 = 3;
/// The timer of the instance's end (name Unknown; Observed: `CATACOMBS5` 1593.6 s, about 2400 s
/// after the instance opened: Inferred `OverrideDungeonTimeout`).
const INSTANCE_END_TIMER: u32 = 0xd815_d620;
/// The timer `catacombstimer`: the run's end (Observed: `CATACOMBS5` 111.065 s, 1800 s after
/// the start; the client's script ends the run `CATACOMBS_TIME_OUT` when it is gone,
/// `FUN_146484c80`).
const CATACOMBS_TIMER: u32 = 0x8bfd_6c55;
/// The timer `timelimit` (Observed: 1800 s before the instance's end timer in `CATACOMBS`,
/// `CATACOMBS2`, `Hive catacomes test 2` and `Hive catacombs test 4`).
const TIME_LIMIT_TIMER: u32 = 0x8f0e_9156;
/// How long a started run lasts (Observed: `catacombstimer` 1800 s after the start; the row's
/// `ProjectedDurationMinutes` is 30).
const RUN_DURATION_MS: u64 = 1_800_000;
/// The icon `bossicon` (Observed at the boss room's tile).
const BOSS_ICON: u32 = 0xfd06_5714;
/// The icon `keyroomicon`.
const KEY_ROOM_ICON: u32 = 0x3ce7_487f;
/// The icon `rewardroomicon`.
const REWARD_ROOM_ICON: u32 = 0xb486_91b9;
/// The icon `hazardroomicon`.
const HAZARD_ROOM_ICON: u32 = 0x5e15_1c36;
/// The icon of the start room's extraction (name Unknown; Observed at the tile that goes to
/// state 0x0e, `CATACOMBS2` 17.409 s at 128, 768).
const EXTRACTION_ICON: u32 = 0xfab6_c9fe;
/// How a room's icon shows: when a player is in a tile next to it or in it (`AreaDiscover`),
/// or only in it (`AreaTile`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IconShows {
    Discovered,
    Entered,
}
/// The icon of each `RoomType` that has one and how it shows (the client's tile script: boss
/// and key icons on `AreaDiscover`, the hazard icon on `AreaTile`; the reward icon: Inferred as
/// the boss and key icons, retail `CATACOMBS2` showed it beside the start room).
const ROOM_ICONS: [(&str, u32, IconShows); 4] = [
    ("EncounterBoss", BOSS_ICON, IconShows::Discovered),
    ("EncounterKey", KEY_ROOM_ICON, IconShows::Discovered),
    ("Reward", REWARD_ROOM_ICON, IconShows::Discovered),
    ("EncounterHazard", HAZARD_ROOM_ICON, IconShows::Entered),
];
/// The participant flags of a player that joins the run (`GameModeParticipantFlag` bits 0, 4,
/// 9 and 17; Observed: every retail join, `CATACOMBS2` 15.716 s).
const JOINED_FLAGS: [u8; PARTICIPANT_FLAGS_LEN] = [0x11, 0x02, 0x02, 0x00, 0x00, 0x00];
/// The flag the run's players gain at its start: bit 5 `RestoreVitalsOnExitedGameMode`, as its
/// byte and bit (Observed: `31 02 02`, `CATACOMBS5` 110.581 s).
const RESTORE_VITALS_FLAG: (usize, u8) = (0, 0x20);
/// The flag of a player in the boss fight: bit 13 `IsInBossFight` (Observed: `31 22 02` and
/// back to `31 02 02`, `CATACOMBS` 156.732 s to 275.126 s, in all ten ledgers; the client's
/// tile script sets it on `AreaStart` of a room in the boss state, `FUN_14080ad40`).
const BOSS_FIGHT_FLAG: (usize, u8) = (1, 0x20);
/// The map picture id of each gameplay tile (`tileUiFilenameIdAndRotation` above the two turn
/// bits). Observed in the ten retail ledgers' maps at the tiles of each slice, except
/// `tile4_b`, `tile4_c` and `tile4_ba`: Inferred from `tile4_ca` 37 and the other hallway
/// kinds, whose four variations hold four ids in a row in their `VariationAssetPaths` order.
/// `tile4_d` and `tile5_d` never stood in a retail run: their tiles go without a picture.
const TILE_PICTURES: [(&str, u8); 28] = [
    ("tile1_a", 47),
    ("tile1_b", 56),
    ("tile1_c", 57),
    ("tile1_ba", 58),
    ("tile1_ca", 59),
    ("tile1_d", 31),
    ("tile2_a", 13),
    ("tile2_b", 43),
    ("tile2_c", 44),
    ("tile2_ba", 45),
    ("tile2_ca", 46),
    ("tile2_d", 32),
    ("tile3_a", 14),
    ("tile3_b", 52),
    ("tile3_c", 53),
    ("tile3_ba", 54),
    ("tile3_ca", 55),
    ("tile3_d", 60),
    ("tile4_a", 33),
    ("tile4_b", 34),
    ("tile4_c", 35),
    ("tile4_ba", 36),
    ("tile4_ca", 37),
    ("tile5_a", 29),
    ("tile5_b", 24),
    ("tile5_c", 25),
    ("tile5_ba", 26),
    ("tile5_ca", 27),
];
/// The end of a gameplay tile's variation name.
const GAMEPLAY_SUFFIX: &str = "_gameplay";
/// The bits of a map byte that hold the tile's quarter turns.
const TURN_BITS: u32 = 2;
/// Tiles per byte of `tileVisited`.
const TILES_PER_BYTE: usize = 8;

/// The state the gameplay tile of a room of `room_type` starts in.
fn first_room_state(room_type: &str) -> Option<u8> {
    FIRST_ROOM_STATES
        .iter()
        .find(|(kind, _)| *kind == room_type)
        .map(|(_, state)| *state)
}

/// The map byte of a tile of `variation` turned `turns` quarter turns: its picture id above
/// the turns (Observed: the four-way tiles always 0, a dead end 0 to 3); 0 (no picture) for a
/// variation without a known id.
fn map_byte(variation: &str, turns: u8) -> u8 {
    let name = variation
        .rsplit('/')
        .next()
        .unwrap_or(variation)
        .to_lowercase();
    let name = name.strip_suffix(GAMEPLAY_SUFFIX).unwrap_or(&name);
    TILE_PICTURES
        .iter()
        .find(|(tile, _)| *tile == name)
        .map_or(0, |(_, picture)| {
            (picture << TURN_BITS) | (turns % QUARTER_TURNS)
        })
}

/// The slots a room of `room_type` spawns beside its gates and its checkpoint (module text):
/// the slot of its `AliasCategory1` and the slots the retail rooms of its kind showed.
///
/// - `EncounterStart`: `EncounterExtractionStartTile` or `EncounterExtraction` (Observed:
///   `dg_enc_catacombs_extraction_prototype` in the dead end beside the arrival, `CATACOMBS2`
///   17.292 s; Inferred: the start room's).
/// - `EncounterNamedEnemy`: also `LevelUp` (Observed: `catacombs_gatherable_levelup` in both
///   named rooms of `CATACOMBS2`).
/// - `EncounterGate`: also the levers `KeyLever` and `LeverKey` (Observed:
///   `interactobject_catacombs_lever_locked` beside `dg_enc_catacombs_key`, `CATACOMBS2` at 256,
///   768; Inferred: a gate room's).
/// - `Hallway`: also `ChestSilverSmall` (Observed: `catacombs_gatherable_chest_cursedsilver_s`
///   in three hallways of `CATACOMBS2`).
/// - `Reward`: also `PowerUps` and `PowerUpSpawner` (Observed: power-up spawners and six
///   power-ups in three rooms of `CATACOMBS2`; Inferred: the reward rooms').
pub(super) fn room_slots(room_type: &str) -> &'static [&'static str] {
    match room_type {
        "EncounterStart" => &["EncounterExtractionStartTile", "EncounterExtraction"],
        "EncounterBoss" => &["EncounterBoss"],
        "EncounterNamedEnemy" => &["EncounterNamedEnemy", "LevelUp"],
        "EncounterGate" => &["EncounterGate", "KeyLever", "LeverKey"],
        "EncounterKey" => &["EncounterKey"],
        "EncounterHazard" => &["EncounterHazard"],
        "Hallway" => &["Hallway", "ChestSilverSmall"],
        "Parkour" => &["Parkour"],
        "Reward" => &["EncounterReward", "PowerUps", "PowerUpSpawner"],
        _ => &[],
    }
}

/// The slots room `index` of `layout` spawns: those of its kind ([`room_slots`]), its
/// checkpoint and a gate on each open side, named by the side in the tile's frame; a side whose
/// room or neighbour is locked takes a key gate.
pub(super) fn slots_of(layout: &CatacombLayout, index: usize) -> Vec<String> {
    let Some(room) = layout.rooms.get(index) else {
        return Vec::new();
    };
    let mut slots: Vec<String> = room_slots(&room.room_type)
        .iter()
        .map(|slot| (*slot).to_owned())
        .collect();
    slots.push(CHECKPOINT_SLOT.to_owned());
    let back = (QUARTER_TURNS - room.turns % QUARTER_TURNS) % QUARTER_TURNS;
    for (side, step) in SIDES {
        if room.sides & side == 0 {
            continue;
        }
        let neighbour = layout.at([room.cell[0] + step[0], room.cell[1] + step[1]]);
        let locked = room.locked || neighbour.is_some_and(|neighbour| neighbour.locked);
        let tile_side = turned(side, back);
        let Some((_, name)) = SIDE_NAMES.iter().find(|(known, _)| *known == tile_side) else {
            continue;
        };
        let gate = if locked { LOCK_GATE_SLOT } else { GATE_SLOT };
        slots.push(format!("{gate}{name}"));
    }
    slots
}

/// Whether `tile` holds an encounter or a prop of the slot `slot`.
fn holds(tile: &CatacombTile, slot: &str) -> bool {
    tile.encounters
        .iter()
        .any(|entry| entry.encounter.name == slot)
        || tile.props.iter().any(|prop| prop.slot == slot)
}

/// The slots of room `index` that its tile spawns: of the start room's two extraction slots
/// only the first the tile holds ([`room_slots`]).
fn spawned_slots(layout: &CatacombLayout, index: usize, tile: &CatacombTile) -> Vec<String> {
    let mut slots = slots_of(layout, index);
    let extraction = room_slots("EncounterStart");
    if layout
        .rooms
        .get(index)
        .is_some_and(|room| room.room_type == "EncounterStart")
        && let Some(first) = extraction.iter().find(|slot| holds(tile, slot))
    {
        slots.retain(|slot| !extraction.contains(&slot.as_str()) || slot.as_str() == *first);
    }
    slots
}

/// The encounters of the slots `slots` of `tile` for room `index` of the run `seed`, in the
/// tile's frame: every encounter of a slot, and of an alias slot's entries (one spawn group)
/// the one the seed draws by the entries' weights.
fn slot_encounters(
    tile: &CatacombTile,
    slots: &[String],
    seed: u64,
    index: usize,
) -> Vec<Encounter> {
    let mut chosen = Vec::new();
    let mut groups: Vec<(&str, Vec<&TileEncounter>)> = Vec::new();
    for entry in tile
        .encounters
        .iter()
        .filter(|entry| slots.contains(&entry.encounter.name))
    {
        match entry.encounter.spawn_group.as_deref() {
            None => chosen.push(entry.encounter.clone()),
            Some(group) => match groups.iter_mut().find(|(known, _)| *known == group) {
                Some((_, members)) => members.push(entry),
                None => groups.push((group, vec![entry])),
            },
        }
    }
    let room = u64::try_from(index).unwrap_or(u64::MAX);
    for (salt, (_, members)) in (0_u64..).zip(&groups) {
        let total: u64 = members.iter().map(|member| u64::from(member.weight)).sum();
        let mut left = mixed(seed ^ (room << 32) ^ salt) % total.max(1);
        let Some(member) = members.iter().find(|member| {
            let weight = u64::from(member.weight);
            let hit = left < weight;
            left = left.saturating_sub(weight);
            hit
        }) else {
            continue;
        };
        let mut encounter = member.encounter.clone();
        encounter.spawn_group = None;
        chosen.push(encounter);
    }
    chosen
}

/// The power-ups at the `points` of a power-up spawner of room `index` of the run `seed`: each
/// point a different one while the kinds last (Observed: six different power-ups at the six
/// points, `CATACOMBS2` 17.307 to 17.341 s), in an order the seed draws.
fn power_up_order(power_ups: &[TilePiece], seed: u64, index: usize) -> Vec<&TilePiece> {
    let room = u64::try_from(index).unwrap_or(u64::MAX);
    let mut order: Vec<(u64, &TilePiece)> = (0_u64..)
        .zip(power_ups)
        .map(|(kind, piece)| (mixed(seed ^ POWER_UP_SALT ^ (room << 32) ^ kind), piece))
        .collect();
    order.sort_by_key(|(draw, _)| *draw);
    order.into_iter().map(|(_, piece)| piece).collect()
}

/// One entity of a run's tile as it stands in the run.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PlacedPiece {
    /// Its slice's asset id.
    asset: [u8; 16],
    /// The member number of its position state.
    position_component: u64,
    /// Where it stands: east, north, up (metres).
    position: [f32; 3],
    /// Its yaw, degrees counter-clockwise.
    yaw_deg: f32,
}

/// One run of the catacombs on its instance sim.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct CatacombRun {
    /// The run's tiles.
    layout: CatacombLayout,
    /// The level of the run's 1635: the instance level of the map with the layout's frame.
    level: LevelSpec,
    /// The entities of the run's tiles and of their spawned slots.
    pieces: Vec<PlacedPiece>,
    /// The encounters of the rooms' slots in the world: the run's level script.
    script: LevelScript,
    /// The slot keys of the placed tile entities.
    placed: Vec<u64>,
    /// The index in `pieces` of each room's gameplay tile.
    gameplay: Vec<usize>,
    /// Each room's progress, in layout order.
    rooms: Vec<RoomProgress>,
    /// The run's own game mode entity and what it shows.
    progress: RunProgress,
}

/// The progress of one room of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RoomProgress {
    /// The slot key of its gameplay tile entity, once placed.
    tile: Option<u64>,
    /// Its tile's script state.
    state: u8,
    /// Its encounters that did not end yet, by index in the run's script.
    open_encounters: Vec<usize>,
    /// Whether its icon is on the map.
    icon_shown: bool,
}

/// The size of a run's map: from the first tile at one tile from the origin to the largest cell
/// (Observed: `mapOrigin` 64, 64 and `mapSizeInTiles` 9 x 14 with the 640 x 960 frame,
/// `CATACOMBS2` 17.409 s; tiles row by row, the `tileVisited` bit of the tile at 256, 704 is
/// bit 5 of byte 11).
#[derive(Debug, Clone, Copy, PartialEq)]
struct RunMap {
    /// Tiles east and north.
    size: [u8; 2],
    /// The tile size, metres.
    tile_size_m: f32,
}

impl RunMap {
    /// The map of `layout`.
    fn of(layout: &CatacombLayout) -> Self {
        let largest = |axis: usize| {
            layout
                .rooms
                .iter()
                .map(|room| room.cell[axis])
                .max()
                .and_then(|cell| u8::try_from(cell).ok())
                .unwrap_or_default()
        };
        Self {
            size: [largest(0), largest(1)],
            tile_size_m: layout.tile_size_m,
        }
    }

    /// The tile index of `cell`, row by row from the first tile.
    fn index(&self, cell: [i32; 2]) -> Option<usize> {
        let [width, height] = self.size.map(i32::from);
        let [east, north] = [cell[0] - 1, cell[1] - 1];
        ((0..width).contains(&east) && (0..height).contains(&north))
            .then(|| usize::try_from(north * width + east).ok())
            .flatten()
    }

    /// Tiles on the map.
    fn tiles(&self) -> usize {
        usize::from(self.size[0]) * usize::from(self.size[1])
    }

    /// The map's first children: origin, tile size, size, each tile's picture byte, no visited
    /// tile and no icon.
    fn first_state(&self, layout: &CatacombLayout) -> GeneratedMap {
        let mut tiles = vec![0; self.tiles()];
        for room in &layout.rooms {
            if let Some(index) = self.index(room.cell) {
                tiles[index] = map_byte(&room.variation, room.turns);
            }
        }
        // A tile size is 64 m (`TileSize`).
        let tile = self.tile_size_m as u16;
        GeneratedMap {
            origin_m: Some([tile, tile]),
            tile_size_m: u8::try_from(tile).ok(),
            size_in_tiles: Some(self.size),
            tiles: Some(ReplicatedList::Full(full(FIRST_LIST_VERSION, tiles))),
            visited: Some(ReplicatedList::Full(full(
                FIRST_LIST_VERSION,
                vec![0; self.tiles().div_ceil(TILES_PER_BYTE)],
            ))),
            icons: Some(ReplicatedList::Full(full(FIRST_LIST_VERSION, Vec::new()))),
        }
    }

    /// The tile centre of `cell`, whole metres.
    fn centre_m(&self, cell: [i32; 2]) -> [u16; 2] {
        // A cell index is a few dozen and a tile 64 m.
        cell.map(|axis| (axis as f32 * self.tile_size_m) as u16)
    }
}

/// The run's own game mode entity and what it shows.
#[derive(Debug, Clone, PartialEq)]
struct RunProgress {
    /// Its slot key and raw id, once placed.
    entity: Option<(u64, [u8; nw_messages::RAW_ID_LEN])>,
    /// The CRC of the game mode and of its map.
    mode_crc: u32,
    map_crc: u32,
    /// The run's map.
    map: RunMap,
    /// The run's players, in join order.
    participants: Vec<MatchParticipant>,
    /// Each player's participant flags, in join order.
    flags: Vec<[u8; PARTICIPANT_FLAGS_LEN]>,
    /// The version of the last change of the run's game mode states.
    version: u64,
    /// Whether the players left the start room.
    started: bool,
    /// The `tileVisited` bytes.
    visited: Vec<u8>,
    /// The icons on the map.
    icons: Vec<MapIcon>,
}

impl CatacombRun {
    /// The level a player of the run loads.
    pub(super) const fn level(&self) -> LevelSpec {
        self.level
    }

    /// The run's level script: the encounters of its rooms.
    pub(super) const fn script(&self) -> &LevelScript {
        &self.script
    }

    /// The run's tiles (the tests compare the two sims' layouts).
    #[cfg(test)]
    pub(super) const fn layout(&self) -> &CatacombLayout {
        &self.layout
    }

    /// The slot keys of the placed tile entities, taken out.
    pub(super) fn take_placed(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.placed)
    }
}

/// `piece` of a tile whose centre stands at `centre` turned by `tile_yaw_deg`.
fn placed_piece(piece: &TilePiece, centre: [f32; 3], tile_yaw_deg: f32) -> PlacedPiece {
    PlacedPiece {
        asset: piece.asset,
        position_component: u64::from(piece.position_component),
        position: turned_point(piece.offset, centre, tile_yaw_deg),
        yaw_deg: tile_yaw_deg + piece.yaw_deg,
    }
}

/// What the tiles of a run spawn: the entities and the encounters in the world.
#[derive(Debug, Clone, PartialEq)]
struct TileSpawns {
    /// Every tile's gameplay tile and art tile, then its slots' slices and power-ups.
    pieces: Vec<PlacedPiece>,
    /// The encounters of the rooms' slots.
    encounters: Vec<Encounter>,
    /// The index in `pieces` of each room's gameplay tile.
    gameplay: Vec<usize>,
    /// The room of each encounter, by index.
    encounter_rooms: Vec<usize>,
}

/// What every tile of `layout` spawns in the run `seed` (module text); `None` when `tiles`
/// lacks a variation of the layout.
fn tile_spawns(layout: &CatacombLayout, tiles: &CatacombTiles, seed: u64) -> Option<TileSpawns> {
    let mut spawns = TileSpawns {
        pieces: Vec::with_capacity(layout.rooms.len() * 2),
        encounters: Vec::new(),
        gameplay: Vec::with_capacity(layout.rooms.len()),
        encounter_rooms: Vec::new(),
    };
    for (index, room) in layout.rooms.iter().enumerate() {
        let Some(tile) = tiles.get(&room.variation) else {
            tracing::warn!(variation = %room.variation, "catacombs tile not in the tile file");
            return None;
        };
        spawns.gameplay.push(spawns.pieces.len());
        for piece in [&tile.gameplay, &tile.art] {
            spawns
                .pieces
                .push(placed_piece(piece, room.position, room.yaw_deg));
        }
        let slots = spawned_slots(layout, index, tile);
        for prop in tile.props.iter().filter(|prop| slots.contains(&prop.slot)) {
            spawns
                .pieces
                .push(placed_piece(&prop.piece, room.position, room.yaw_deg));
            let order = power_up_order(&tiles.power_ups, seed, index);
            for (point, power_up) in prop.points.iter().zip(order) {
                let at_point = TilePiece {
                    offset: point.offset,
                    yaw_deg: point.yaw_deg,
                    ..power_up.clone()
                };
                spawns
                    .pieces
                    .push(placed_piece(&at_point, room.position, room.yaw_deg));
            }
        }
        let encounters = slot_encounters(tile, &slots, seed, index);
        spawns
            .encounter_rooms
            .extend(std::iter::repeat_n(index, encounters.len()));
        spawns.encounters.extend(
            encounters
                .iter()
                .map(|encounter| placed_encounter(encounter, room.position, room.yaw_deg)),
        );
    }
    Some(spawns)
}

/// The spawn members of `piece` with raw id `gde_ref`: its card and its position, with a
/// rotation only when it is turned (module text).
fn piece_members(piece: &PlacedPiece, gde_ref: [u8; nw_messages::RAW_ID_LEN]) -> Vec<BundleMember> {
    let [x, y, z] = piece.position;
    let position = if piece.yaw_deg == 0.0 {
        BundleMember {
            component: piece.position_component,
            state: MemberState::Position(PositionInTheWorldReplicatedState {
                position: WorldPosition::from_metres(x, y, z),
                rotation: None,
                scale: None,
            }),
        }
    } else {
        place_member(
            piece.position_component,
            piece.position,
            piece.yaw_deg.to_radians() / 2.0,
        )
    };
    vec![card(piece.asset, gde_ref), position]
}

/// The seed of the layout of the run `instance`: the two halves of its id joined (Inferred,
/// see the module text).
pub(super) fn run_seed(instance: Uuid) -> u64 {
    let (high, low) = instance.as_u64_pair();
    high ^ low
}

/// The catacombs game mode of `content`, when its catalog holds it.
fn catacombs_mode(content: &PlayerContent) -> Option<&InstanceMode> {
    content
        .instances
        .by_id(CATACOMBS_GAME_MODE)
        .filter(|mode| mode.kind == InstanceKind::Catacombs)
}

/// The layout of the run `instance`: the start room on the cell of the map's team teleport spot,
/// the seed [`run_seed`]; `None` when the content holds no catacombs or no layout fits.
pub(super) fn run_layout(content: &PlayerContent, instance: Uuid) -> Option<CatacombLayout> {
    let rules = content.catacombs.as_ref()?;
    let teleport = catacombs_mode(content)?.map.teleports.first()?;
    let start_cell = cell_of(teleport.position, rules.tile_size_m);
    match rules.generate(run_seed(instance), start_cell) {
        Ok(layout) => Some(layout),
        Err(error) => {
            tracing::warn!(%instance, %error, "no catacombs layout");
            None
        }
    }
}

/// Where a player of the run `layout` appears: the start room's checkpoint, turned with its
/// tile; the tile's centre when its tile holds no checkpoint (module text).
pub(super) fn start_spot(layout: &CatacombLayout, tiles: &CatacombTiles) -> Option<Position> {
    let room = layout.rooms.first()?;
    let checkpoint = tiles
        .get(&room.variation)
        .and_then(|tile| tile.props.iter().find(|prop| prop.slot == CHECKPOINT_SLOT));
    let [x, y, z] = checkpoint.map_or(room.position, |prop| {
        turned_point(prop.piece.offset, room.position, room.yaw_deg)
    });
    Some(Position { x, y, z })
}

/// Where a player of the run `instance` appears, for the sim that sends it there.
pub(super) fn run_start_spot(content: &PlayerContent, instance: Uuid) -> Option<Position> {
    start_spot(&run_layout(content, instance)?, &content.catacomb_tiles)
}

/// The run `instance` of `mode` on the instance level `level`: its layout and its 1635 level,
/// framed by the layout.
pub(super) fn open_run(
    content: &PlayerContent,
    mode: &InstanceMode,
    level: LevelSpec,
    instance: Uuid,
) -> Option<CatacombRun> {
    if mode.kind != InstanceKind::Catacombs {
        return None;
    }
    let layout = run_layout(content, instance)?;
    let spawns = tile_spawns(&layout, &content.catacomb_tiles, run_seed(instance))?;
    let [x, y, width, height] = layout.frame();
    let level = LevelSpec {
        phasing_origin: [x, y],
        phasing_bounds: [width, height],
        ..level
    };
    let rooms = layout
        .rooms
        .iter()
        .enumerate()
        .map(|(index, room)| RoomProgress {
            tile: None,
            state: first_room_state(&room.room_type).unwrap_or(HALLWAY_STATE),
            open_encounters: spawns
                .encounter_rooms
                .iter()
                .enumerate()
                .filter(|(_, of)| **of == index)
                .map(|(encounter, _)| encounter)
                .collect(),
            icon_shown: false,
        })
        .collect();
    let map = RunMap::of(&layout);
    Some(CatacombRun {
        level,
        pieces: spawns.pieces,
        script: LevelScript {
            level: mode.map.folder.clone(),
            script_slice: String::new(),
            encounters: spawns.encounters,
            chests: Vec::new(),
        },
        placed: Vec::new(),
        gameplay: spawns.gameplay,
        rooms,
        progress: RunProgress {
            entity: None,
            mode_crc: mode.crc,
            map_crc: mode.map.map_crc,
            map,
            participants: Vec::new(),
            flags: Vec::new(),
            version: FIRST_LIST_VERSION,
            started: false,
            visited: vec![0; map.tiles().div_ceil(TILES_PER_BYTE)],
            icons: Vec::new(),
        },
        layout,
    })
}

/// The children a gameplay tile and the run's own game mode state share: the mode and its map,
/// the participant lists at `version`, empty spawner, raid, value and timer lists and no
/// affliction (Observed: `CATACOMBS2` 17.307 s and 17.409 s).
fn shared_first_state(progress: &RunProgress) -> GameModeState {
    GameModeState {
        spawned: Some(full(FIRST_LIST_VERSION, Vec::new())),
        mode: Some(progress.mode_crc),
        map: Some(progress.map_crc),
        raids: Some(ReplicatedList::Full(full(FIRST_LIST_VERSION, Vec::new()))),
        values: Some(full(FIRST_LIST_VERSION, Vec::new())),
        timers: Some(full(FIRST_LIST_VERSION, Vec::new())),
        no_affliction: true,
        ..participant_lists(&progress.participants, progress.version, true)
    }
}

/// The first game mode state of a gameplay tile whose room is in `state`.
fn tile_first_state(progress: &RunProgress, state: u8) -> GameModeState {
    GameModeState {
        script_state: Some(state),
        linked_mode: Some(TILE_LINKED_MODE),
        ..shared_first_state(progress)
    }
}

/// The first game mode state of the run's own entity: waiting, with the map of `layout`.
fn run_first_state(progress: &RunProgress, layout: &CatacombLayout) -> GameModeState {
    GameModeState {
        script_state: Some(RUN_WAITING_STATE),
        level_map: progress.map.first_state(layout),
        linked_mode: Some(RUN_LINKED_MODE),
        ..shared_first_state(progress)
    }
}

impl SimWorld {
    /// The catacombs run of this instance sim.
    pub(super) fn catacomb_run(&self) -> Option<&CatacombRun> {
        self.instance_run.as_ref()?.catacomb()
    }

    /// The catacombs run of this instance sim, to change.
    fn catacomb_run_mut(&mut self) -> Option<&mut CatacombRun> {
        self.instance_run
            .as_mut()
            .and_then(super::instances::InstanceRun::catacomb_mut)
    }

    /// Places the tile entities of this sim's catacombs run (its opening): every gameplay tile
    /// with its room's game mode state, and the run's own game mode entity at the start room
    /// (module text).
    pub(super) fn place_catacomb_tiles(&mut self) {
        let Some((pieces, states, mode_state, start)) = self.catacomb_run().and_then(|run| {
            let mut states = vec![None; run.pieces.len()];
            for (room, piece) in run.rooms.iter().zip(&run.gameplay) {
                states[*piece] = Some(tile_first_state(&run.progress, room.state));
            }
            let [x, y, z] = run.layout.rooms.first()?.position;
            Some((
                run.pieces.clone(),
                states,
                run_first_state(&run.progress, &run.layout),
                Position { x, y, z },
            ))
        }) else {
            return;
        };
        let namespace = TILE_NAMESPACE ^ self.me.generation;
        let placed: Vec<u64> = pieces
            .iter()
            .zip(states)
            .map(|(piece, state)| {
                let [x, y, z] = piece.position;
                self.stations.place_camp(|number| {
                    // A slot key is far below `usize::MAX` on the 64-bit targets the server runs
                    // on.
                    let gde_ref = dynamic_ref(namespace, number as usize);
                    let mut members = piece_members(piece, gde_ref);
                    members.extend(state.map(|state| BundleMember {
                        component: GAME_MODE_COMPONENT,
                        state: MemberState::GameMode(state),
                    }));
                    Station {
                        gde_ref,
                        position: Position { x, y, z },
                        station_type: String::new(),
                        name: Vec::new(),
                        members,
                    }
                })
            })
            .collect();
        let mode_entity = self.place_game_mode_entity(RUN_NAMESPACE, None, start, mode_state);
        tracing::info!(entities = placed.len(), "catacombs tiles placed");
        if let Some(run) = self.catacomb_run_mut() {
            for (room, piece) in run.rooms.iter_mut().zip(&run.gameplay) {
                room.tile = placed.get(*piece).copied();
            }
            run.placed = placed;
            run.placed.push(mode_entity.0);
            run.progress.entity = Some(mode_entity);
        }
    }

    /// Takes the tile entities of the ended run `run` away.
    pub(super) fn remove_catacomb_tiles(&mut self, run: &mut CatacombRun) {
        for number in run.take_placed() {
            self.stations.remove_camp(number);
        }
    }

    /// Player `entity` arrived in this sim's catacombs run: its queue group goes away and it is
    /// in no game mode queue (1234, 4970 of no game mode); it joins the run's participant lists
    /// on the run's entity and on every gameplay tile, and its participant state names the run
    /// with the joined flags (module text).
    pub(super) fn join_catacombs(&mut self, entity: Entity) {
        let Some((number, character, eligible)) = self.world.get::<Player>(entity).map(|player| {
            (
                player.number,
                *player.character.as_bytes(),
                player.arena.queue_eligible().to_vec(),
            )
        }) else {
            return;
        };
        let header = participant_header(number);
        let messages = [
            ServerMessage::GameMode(GameModeServerMsg::QueueGroupRemoved { header }),
            ServerMessage::LeftGameModeQueue(LeftGameModeQueueMsg::of(header, NO_GAME_MODE)),
        ];
        tracing::info!(player = number, "player arrived in the catacombs run");
        self.send_to(entity, &messages);
        let phase = self.instance_run.as_ref().map_or(0, |run| run.phase());
        let server_ms = self.server_ms;
        let Some(run) = self.catacomb_run_mut() else {
            return;
        };
        let Some((_, gde_ref)) = run.progress.entity else {
            return;
        };
        if run
            .progress
            .participants
            .iter()
            .any(|participant| participant.number == number)
        {
            return;
        }
        run.progress.participants.push(MatchParticipant {
            number,
            entity,
            character,
            team: 0,
            place: 0,
            status: PLAYING_STATUS,
        });
        run.progress.flags.push(JOINED_FLAGS);
        run.progress.version += 1;
        let version = run.progress.version;
        let lists = participant_lists(&run.progress.participants, version, true);
        let entities = run.state_entities();
        let (mode, map) = (run.progress.mode_crc, run.progress.map_crc);
        for state_entity in entities {
            self.change_game_mode_state(state_entity, lists.clone());
        }
        self.put_match_joined(
            entity,
            MatchEntry {
                gde_ref,
                mode,
                map,
                phase,
                flags: JOINED_FLAGS,
                team: 0,
                list_version: version,
                eligible,
            },
            server_ms,
        );
    }

    /// One tick of this sim's catacombs run: the tiles its players went into are visited, the
    /// rooms they found show their icons, and the run starts once they all left the start room
    /// (module text).
    pub(super) fn tick_catacombs(&mut self) {
        if self.catacomb_run().is_none() {
            return;
        }
        let places: Vec<(Entity, Position)> = {
            let mut query = self.world.query::<(Entity, &Player, &Body)>();
            query
                .iter(&self.world)
                .map(|(entity, _, body)| (entity, body.motion.position()))
                .collect()
        };
        let ends_at_ms = self
            .instance_run
            .as_ref()
            .and_then(super::instances::InstanceRun::ends_at_ms);
        let server_ms = self.server_ms;
        let Some(run) = self.catacomb_run_mut() else {
            return;
        };
        let changes = run.progress_changes(&places, server_ms, ends_at_ms);
        for (entity, change) in changes.states {
            self.change_game_mode_state(entity, change);
        }
        for (entity, flags) in changes.flags {
            self.put_participant_state(
                entity,
                GameModeParticipantState {
                    flags: Some(flags),
                    ..GameModeParticipantState::default()
                },
            );
        }
    }

    /// The encounter `encounter` of this sim's catacombs run ended: when it was its room's last
    /// one, a room that clears goes to [`CLEARED_ROOM_STATE`] (module text).
    pub(super) fn catacomb_encounter_done(&mut self, encounter: usize) {
        let Some(run) = self.catacomb_run_mut() else {
            return;
        };
        let Some(change) = run.encounter_done(encounter) else {
            return;
        };
        self.change_game_mode_state(change.0, change.1);
    }
}

/// What one tick of a run changes.
#[derive(Debug, Default)]
struct ProgressChanges {
    /// The game mode state changes, by entity slot key.
    states: Vec<(u64, GameModeState)>,
    /// The players whose participant flags changed, with their flags.
    flags: Vec<(Entity, [u8; PARTICIPANT_FLAGS_LEN])>,
}

/// `flags` with the flag `(byte, bit)` set to `on`.
fn with_flag(
    mut flags: [u8; PARTICIPANT_FLAGS_LEN],
    (byte, bit): (usize, u8),
    on: bool,
) -> [u8; PARTICIPANT_FLAGS_LEN] {
    if on {
        flags[byte] |= bit;
    } else {
        flags[byte] &= !bit;
    }
    flags
}

impl CatacombRun {
    /// The slot keys of every entity that holds the run's participant lists: the run's own
    /// entity, then the gameplay tiles.
    fn state_entities(&self) -> Vec<u64> {
        self.progress
            .entity
            .map(|(entity, _)| entity)
            .into_iter()
            .chain(self.rooms.iter().filter_map(|room| room.tile))
            .collect()
    }

    /// The changes of the run whose players stand at `places` at `server_ms` (ms since
    /// 2000-01-01), in an instance that ends at Unix ms `ends_at_ms`: visited tiles, found
    /// rooms' icons and the start.
    fn progress_changes(
        &mut self,
        places: &[(Entity, Position)],
        server_ms: u64,
        ends_at_ms: Option<u64>,
    ) -> ProgressChanges {
        let mut changes = ProgressChanges::default();
        let Some((mode_entity, _)) = self.progress.entity else {
            return changes;
        };
        let map = self.progress.map;
        // Each participant's cell, in join order; `None` for one not on this sim.
        let participant_cells: Vec<Option<[i32; 2]>> = self
            .progress
            .participants
            .iter()
            .map(|participant| {
                places
                    .iter()
                    .find(|(entity, _)| *entity == participant.entity)
                    .map(|(_, at)| cell_of([at.x, at.y, at.z], map.tile_size_m))
            })
            .collect();
        let cells: Vec<[i32; 2]> = participant_cells.iter().flatten().copied().collect();
        let mut visited = Vec::new();
        for index in cells.iter().filter_map(|cell| map.index(*cell)) {
            let (byte, bit) = (index / TILES_PER_BYTE, index % TILES_PER_BYTE);
            let Some(bits) = self.progress.visited.get_mut(byte) else {
                continue;
            };
            if *bits & 1 << bit == 0 {
                *bits |= 1 << bit;
                if !visited.contains(&byte) {
                    visited.push(byte);
                }
            }
        }
        let mut icons = Vec::new();
        for (room, progress) in self.layout.rooms.iter().zip(&mut self.rooms) {
            let Some((_, key, shows)) = ROOM_ICONS
                .iter()
                .find(|(room_type, _, _)| *room_type == room.room_type)
            else {
                continue;
            };
            let found = cells.iter().any(|cell| {
                let step = [cell[0] - room.cell[0], cell[1] - room.cell[1]];
                step == [0, 0]
                    || (*shows == IconShows::Discovered
                        && SIDES.iter().any(|(_, side_step)| *side_step == step))
            });
            if found && !progress.icon_shown {
                progress.icon_shown = true;
                icons.push(MapIcon {
                    key: *key,
                    position_m: map.centre_m(room.cell),
                });
            }
        }
        let start = self.layout.rooms.first().map(|room| room.cell);
        let start_now = !self.progress.started
            && !cells.is_empty()
            && cells.iter().all(|cell| Some(*cell) != start);
        let mut run_change = GameModeState::default();
        if start_now {
            self.progress.started = true;
            if let Some(cell) = start {
                icons.push(MapIcon {
                    key: EXTRACTION_ICON,
                    position_m: map.centre_m(cell),
                });
            }
            self.progress.version += 1;
            let version = self.progress.version;
            run_change.script_state = Some(RUN_STARTED_STATE);
            let participants = self.progress.participants.len() as u64;
            run_change.values = Some(full(
                version,
                vec![
                    (DYNAMIC_DIFFICULTY_VALUE, 0),
                    (
                        RESPAWNS_REMAINING_VALUE,
                        participants * RESPAWNS_PER_PARTICIPANT,
                    ),
                    (THIRD_RUN_VALUE, 0),
                ],
            ));
            run_change.timers = Some(full(version, run_timers(server_ms, ends_at_ms)));
            if let Some(room) = self.rooms.first_mut() {
                room.state = START_OPEN_STATE;
                if let Some(tile) = room.tile {
                    changes.states.push((
                        tile,
                        GameModeState {
                            script_state: Some(START_OPEN_STATE),
                            ..GameModeState::default()
                        },
                    ));
                }
            }
            for flags in &mut self.progress.flags {
                *flags = with_flag(*flags, RESTORE_VITALS_FLAG, true);
            }
        }
        let boss_cell = self
            .layout
            .rooms
            .iter()
            .zip(&self.rooms)
            .find(|(_, progress)| progress.state == BOSS_ROOM_STATE)
            .map(|(room, _)| room.cell);
        for ((participant, flags), cell) in self
            .progress
            .participants
            .iter()
            .zip(&mut self.progress.flags)
            .zip(&participant_cells)
        {
            let fighting = boss_cell.is_some() && *cell == boss_cell;
            let now = with_flag(*flags, BOSS_FIGHT_FLAG, fighting);
            if start_now || now != *flags {
                *flags = now;
                changes.flags.push((participant.entity, now));
            }
        }
        if !visited.is_empty() {
            self.progress.version += 1;
            let version = self.progress.version;
            run_change.level_map.visited = Some(ReplicatedList::Changes(
                visited
                    .iter()
                    .enumerate()
                    .map(|(position, byte)| ListChange {
                        // A map holds a few hundred tiles.
                        index: *byte as u32,
                        version: if position == 0 { version } else { 0 },
                        value: self.progress.visited[*byte],
                    })
                    .collect(),
            ));
        }
        if !icons.is_empty() {
            self.progress.version += 1;
            let version = self.progress.version;
            let first = self.progress.icons.len();
            run_change.level_map.icons = Some(ReplicatedList::Changes(
                icons
                    .iter()
                    .enumerate()
                    .map(|(position, icon)| ListChange {
                        // A run shows a few dozen icons.
                        index: (first + position) as u32,
                        version: if position == 0 { version } else { 0 },
                        value: *icon,
                    })
                    .collect(),
            ));
            self.progress.icons.extend(icons);
        }
        if run_change != GameModeState::default() {
            changes.states.insert(0, (mode_entity, run_change));
        }
        changes
    }

    /// The encounter `encounter` ended: the change of its room's tile when it was the room's
    /// last one and the room clears.
    fn encounter_done(&mut self, encounter: usize) -> Option<(u64, GameModeState)> {
        let room = self
            .rooms
            .iter_mut()
            .find(|room| room.open_encounters.contains(&encounter))?;
        room.open_encounters.retain(|open| *open != encounter);
        if !room.open_encounters.is_empty() || !CLEARABLE_STATES.contains(&room.state) {
            return None;
        }
        room.state = CLEARED_ROOM_STATE;
        Some((
            room.tile?,
            GameModeState {
                script_state: Some(CLEARED_ROOM_STATE),
                ..GameModeState::default()
            },
        ))
    }
}

/// The timers of a run that starts at `server_ms` (ms since 2000-01-01) in an instance that ends
/// at Unix ms `ends_at_ms`: the instance's end, `catacombstimer` and `timelimit` (Observed in
/// this order, `CATACOMBS2` 17.409 s); the run's timer alone when the instance has no end.
fn run_timers(server_ms: u64, ends_at_ms: Option<u64>) -> Vec<(u32, u64)> {
    let run_end = server_ms.saturating_add(RUN_DURATION_MS);
    match ends_at_ms.map(|end| end.saturating_sub(crate::server::SERVER_EPOCH_UNIX_MS)) {
        Some(end) => vec![
            (INSTANCE_END_TIMER, end),
            (CATACOMBS_TIMER, run_end),
            (TIME_LIMIT_TIMER, end.saturating_sub(RUN_DURATION_MS)),
        ],
        None => vec![(CATACOMBS_TIMER, run_end)],
    }
}

#[cfg(test)]
pub(in crate::world) mod tests {
    use nw_bus::{
        ArenaSeat, InstanceOpened, InstanceStay, MatchPlace, QueueCall, QueueNews, QueueNewsKind,
        StoredPoint,
    };
    use nw_content::catacombs::{
        CatacombRules, ClusterRule, EAST, GrammarRule, NORTH, RoomRule, SOUTH, SeedGraph, TileRule,
    };
    use nw_content::instances::{EntryCounter, InstanceMap, NO_INDEX, Teleport};
    use nw_formats::objectstream::lowercase_crc32;
    use nw_messages::game_mode_participant::{GameModeParticipantState, MatchmakingService};
    use nw_messages::{GameModeClientMsg, MemberState, QueueActivity};

    use super::*;
    use crate::world::tests::{
        NOW, SERVER_TIME, admit_at, connection, join, messages_to, settings, slot, tick,
        world_with_spawns,
    };

    /// The tile size of the retail catacombs tiles (`TileSize` 64).
    const TILE_SIZE_M: f32 = 64.0;
    /// The retail `TeamTeleportData` of the `Catacombs` map row.
    const TELEPORT: [f32; 3] = [492.0, 761.0, 120.0];

    /// The retail `Catacombs` mode on `NW_Catacomb_00` (made from its rows).
    pub(in crate::world) fn catacombs() -> InstanceMode {
        InstanceMode {
            id: CATACOMBS_GAME_MODE.to_owned(),
            crc: lowercase_crc32(CATACOMBS_GAME_MODE),
            kind: InstanceKind::Catacombs,
            map: InstanceMap {
                folder: "nw_catacomb_00".to_owned(),
                map_name: "NW_Catacomb_00".to_owned(),
                coatlicue_path: "coatlicue/NW_Catacomb_00".to_owned(),
                frame: [0.0, 0.0, 256.0, 256.0],
                teleports: vec![Teleport {
                    position: TELEPORT,
                    yaw_deg: 0.0,
                }],
                formation: Vec::new(),
                map_crc: lowercase_crc32(CATACOMBS_GAME_MODE),
            },
            capacity: nw_content::instances::CATACOMBS_TEAM_PLAYERS,
            min_group: 0,
            required_level: Some(65),
            remote_entry: true,
            respawns: true,
            mutable: false,
            timeout_seconds: Some(2400),
            victory_delay_seconds: Some(4),
            items_removed_on_exit: vec!["DungeonItem".to_owned()],
            required_item_classes: Vec::new(),
            entry_achievement: None,
            loot_tags: Vec::new(),
            container_level: Some(65),
            counter: EntryCounter::Uncounted,
            game_mode_index: NO_INDEX,
            generated: true,
            rotation: Vec::new(),
            rotation_span_seconds: None,
        }
    }

    /// A small rule set of the retail kinds: a start room, a hallway and a boss room in a row,
    /// the retail `Connector` row, and one straight, corner and dead-end tile per room kind.
    pub(in crate::world) fn rules() -> CatacombRules {
        let graph = |text: &str| SeedGraph::parse(text).expect("graph");
        let room = |id: &str, room_type: &str| {
            (
                id.to_owned(),
                RoomRule {
                    id: id.to_owned(),
                    room_type: room_type.to_owned(),
                    starting_state: format!("State{id}"),
                },
            )
        };
        let tile =
            |id: &str, sides: u8, rotations: u8, room_type: &str, variation: &str| TileRule {
                id: id.to_owned(),
                sides,
                rotations,
                room_types: vec![room_type.to_owned()],
                variations: vec![variation.to_owned()],
                weight: 1,
            };
        let mut tiles = Vec::new();
        for (room_type, letter) in [
            ("EncounterStart", "a"),
            ("EncounterBoss", "a"),
            ("Hallway", "b"),
        ] {
            tiles.push(tile(
                "Straight",
                NORTH | SOUTH,
                1,
                room_type,
                &format!("tile1_{letter}_gameplay"),
            ));
            tiles.push(tile(
                "Corner",
                EAST | SOUTH,
                3,
                room_type,
                &format!("tile2_{letter}_gameplay"),
            ));
            tiles.push(tile(
                "DeadEnd",
                SOUTH,
                3,
                room_type,
                &format!("tile4_{letter}_gameplay"),
            ));
        }
        CatacombRules {
            feature: CATACOMBS_GAME_MODE.to_owned(),
            clusters: vec![ClusterRule {
                id: "CatacombsLine".to_owned(),
                seed: graph("StartRoom_1-HallwayRoom_1-BossRoom_1"),
                replacement_count: 1,
                weight: 1,
            }],
            grammars: vec![GrammarRule {
                id: "Connector".to_owned(),
                replaces: "Connector".to_owned(),
                seed: graph("HallwayRoom"),
                themes: Vec::new(),
                weight: 1,
            }],
            rooms: [
                room("StartRoom", "EncounterStart"),
                room("HallwayRoom", "Hallway"),
                room("BossRoom", "EncounterBoss"),
            ]
            .into_iter()
            .collect(),
            tiles,
            tile_size_m: TILE_SIZE_M,
        }
    }

    /// The art tile's place in its tile in the test tiles: 2 m east (the retail art tiles stand
    /// at the tile's centre; the offset shows the turn).
    const ART_OFFSET: [f32; 3] = [2.0, 0.0, 0.0];

    /// The test tiles' checkpoint place (retail `CheckpointTile` of `tile1_a_gameplay`).
    const CHECKPOINT: [f32; 3] = [-12.0, 0.0, -2.0];
    /// The test tiles' gate slots and their places (retail `tile1_a_gameplay`).
    const GATES: [(&str, [f32; 3]); 8] = [
        ("GateNorth", [0.0, 16.5, -2.0]),
        ("GateEast", [16.5, 0.0, -2.0]),
        ("GateSouth", [0.0, -16.5, -2.0]),
        ("GateWest", [-16.5, 0.0, -2.0]),
        ("LockGateNorth", [0.0, 16.5, -2.0]),
        ("LockGateEast", [16.5, 0.0, -2.0]),
        ("LockGateSouth", [0.0, -16.5, -2.0]),
        ("LockGateWest", [-16.5, 0.0, -2.0]),
    ];
    /// The points of the test power-up spawner.
    const POWER_UP_POINTS: [[f32; 3]; 2] = [[-6.0, 0.0, -2.0], [6.0, 0.0, -2.0]];
    /// The test power-up kinds.
    const POWER_UP_KINDS: u8 = 3;
    /// The spawn group of the test boss slot's two entries.
    const BOSS_GROUP: &str = "slices/catacombs/gameplay/tiles/test#7";

    /// The plain slots of a test tile whose gameplay asset starts with `number`: the
    /// checkpoint, the gates, the level-up shrine and a power-up spawner; each asset's second
    /// byte is the slot's number from 2.
    fn test_props(number: u8) -> Vec<nw_world::catacomb_tiles::TileProp> {
        let piece = |slot: u8, offset: [f32; 3]| {
            let mut asset = [number; 16];
            asset[1] = slot;
            TilePiece {
                slice: format!("slices/test/slot{slot}.dynamicslice"),
                asset,
                position_component: 1,
                offset,
                yaw_deg: 0.0,
            }
        };
        let prop = |slot: &str, number: u8, offset: [f32; 3], points: Vec<[f32; 3]>| {
            nw_world::catacomb_tiles::TileProp {
                slot: slot.to_owned(),
                piece: piece(number, offset),
                points: points
                    .into_iter()
                    .map(|offset| nw_world::catacomb_tiles::TilePlace {
                        offset,
                        yaw_deg: 0.0,
                    })
                    .collect(),
            }
        };
        let mut props = vec![
            prop(CHECKPOINT_SLOT, 2, CHECKPOINT, Vec::new()),
            prop("LevelUp", 3, [0.0, 0.0, -2.0], Vec::new()),
            prop("PowerUps", 4, [0.0, 0.0, -2.0], POWER_UP_POINTS.to_vec()),
        ];
        props.extend(
            (5_u8..)
                .zip(GATES)
                .map(|(number, (slot, offset))| prop(slot, number, offset, Vec::new())),
        );
        props
    }

    /// The encounters of a test tile: two entries of an alias boss slot, a hallway and the
    /// start room's extraction.
    fn test_encounters() -> Vec<TileEncounter> {
        let encounter = |name: &str, position: [f32; 3], group: Option<&str>| TileEncounter {
            weight: 1,
            encounter: Encounter {
                name: name.to_owned(),
                slice: format!("slices/test/{name}.dynamicslice"),
                position,
                spawn_group: group.map(ToOwned::to_owned),
                ..Encounter::default()
            },
        };
        vec![
            encounter("EncounterBoss", [1.0, 0.0, -2.0], Some(BOSS_GROUP)),
            encounter("EncounterBoss", [2.0, 0.0, -2.0], Some(BOSS_GROUP)),
            encounter("Hallway", [0.0, 4.0, -2.0], None),
            encounter("EncounterExtraction", [0.0, 0.0, -2.0], None),
        ]
    }

    /// A tile file with a gameplay and an art piece, the test slots and encounters for every
    /// variation of [`rules`]: the asset id's first byte is the variation's number, the art's
    /// second byte 1; and three power-ups.
    fn tiles(rules: &CatacombRules) -> CatacombTiles {
        let piece = |slice: String, asset: [u8; 16], offset: [f32; 3]| TilePiece {
            slice,
            asset,
            position_component: 1,
            offset,
            yaw_deg: 0.0,
        };
        CatacombTiles {
            by_variation: rules
                .tiles
                .iter()
                .flat_map(|tile| &tile.variations)
                .zip(1_u8..)
                .map(|(variation, number)| {
                    let mut art = [number; 16];
                    art[1] = 1;
                    (
                        variation.to_lowercase(),
                        nw_world::catacomb_tiles::CatacombTile {
                            gameplay: piece(
                                format!("slices/{variation}.dynamicslice"),
                                [number; 16],
                                [0.0; 3],
                            ),
                            art: piece(
                                format!("slices/{variation}_art.dynamicslice"),
                                art,
                                ART_OFFSET,
                            ),
                            props: test_props(number),
                            encounters: test_encounters(),
                        },
                    )
                })
                .collect(),
            power_ups: (0..POWER_UP_KINDS)
                .map(|kind| {
                    piece(
                        format!("slices/catacombs/gameplay/powerups/kind{kind}.dynamicslice"),
                        [0xf0 + kind; 16],
                        [0.0; 3],
                    )
                })
                .collect(),
        }
    }

    /// Player content with the catacombs mode, rules and tiles.
    pub(in crate::world) fn content() -> PlayerContent {
        let rules = rules();
        PlayerContent {
            instances: nw_content::instances::InstanceCatalog::build([catacombs()]),
            catacomb_tiles: tiles(&rules),
            catacombs: Some(rules),
            ..PlayerContent::default()
        }
    }

    #[test]
    fn a_run_has_one_layout_for_both_sims_from_its_instance_id() {
        let content = content();
        let instance = Uuid::from_u128(0x1234_5678_9abc_def0_0fed_cba9_8765_4321);
        let first = run_layout(&content, instance).expect("layout");
        assert_eq!(run_layout(&content, instance), Some(first.clone()));
        assert_eq!(first.rooms[0].room_id, "StartRoom");
        assert_eq!(
            first.rooms[0].cell,
            cell_of(TELEPORT, TILE_SIZE_M),
            "the start room on the teleport's cell"
        );
        let [x, y, z] = turned_point(CHECKPOINT, first.rooms[0].position, first.rooms[0].yaw_deg);
        assert_eq!(
            run_start_spot(&content, instance),
            Some(Position { x, y, z }),
            "the start room's checkpoint, 12 m from its tile's centre"
        );
        assert_eq!(
            run_seed(instance),
            0x1234_5678_9abc_def0 ^ 0x0fed_cba9_8765_4321
        );
        assert!(
            run_layout(&PlayerContent::default(), instance).is_none(),
            "no catacombs without the rules"
        );
    }

    #[test]
    fn a_run_level_is_the_map_level_in_the_layout_frame() {
        let content = content();
        let mode = catacombs();
        let map = LevelSpec::instance(
            "nw_catacomb_00",
            "NW_Catacomb_00",
            "coatlicue/NW_Catacomb_00",
            mode.map.frame,
        );
        let instance = Uuid::from_u128(7);
        let run = open_run(&content, &mode, map, instance).expect("run");
        let [x, y, width, height] = run.layout().frame();
        assert_eq!(run.level().phasing_origin, [x, y]);
        assert_eq!(run.level().phasing_bounds, [width, height]);
        assert_eq!(run.level().map_name, b"NW_Catacomb_00");
        assert!(run.level().private);
        let mut expedition = mode.clone();
        expedition.kind = InstanceKind::Expedition;
        assert!(open_run(&content, &expedition, map, instance).is_none());
    }

    /// The test player.
    const PLAYER: u64 = 41;
    /// The test run.
    const INSTANCE: u128 = 0x51;
    /// The test run's phase.
    const PHASE: u64 = 9;

    /// The participant states queued for player `entity` since the last call.
    fn participant_states(world: &mut SimWorld, entity: Entity) -> Vec<GameModeParticipantState> {
        let Some(mut player) = world.world.get_mut::<Player>(entity) else {
            return Vec::new();
        };
        std::mem::take(&mut player.extra_members)
            .into_iter()
            .filter_map(|member| match member.state {
                MemberState::GameModeParticipant(state) => Some(state),
                _ => None,
            })
            .collect()
    }

    fn call(world: &mut SimWorld, call: GameModeClientMsg) -> Vec<ServerMessage> {
        world.game_mode_call(&connection(PLAYER), call);
        messages_to(&world.take_messages(), PLAYER)
    }

    /// A sim whose `closed_queues` holds the catacombs answers a catacombs join as a mode that
    /// does not run here (1234).
    #[test]
    fn a_closed_catacombs_queue_refuses_the_join() {
        let mut world = world_with_spawns(Vec::new());
        world.content = std::sync::Arc::new(content());
        world.settings.closed_queues =
            crate::config::ClosedQueues::of(&[crate::config::QueuedMode::Catacombs]);
        join(&mut world, PLAYER, 100.0);
        let header = participant_header(PLAYER);
        assert_eq!(
            call(
                &mut world,
                GameModeClientMsg::JoinQueueForActivities {
                    header,
                    activities: vec![QueueActivity {
                        activity: super::super::queue::CATACOMBS_ACTIVITY,
                        extra: 1,
                        flag: 0,
                    }],
                },
            ),
            [ServerMessage::GameMode(
                GameModeServerMsg::QueueGroupRemoved { header }
            )]
        );
    }

    /// Retail `CATACOMBS2`: 6254 `01 0c16 0001 00` and two 6272 queue the player with activity
    /// 6; the director's match with no pop gives status 1 and the match id; the accept, 5449 and
    /// 5450 move the player into the run's start room.
    #[test]
    fn a_catacombs_join_queues_is_matched_with_no_pop_and_moves_into_the_start_room() {
        let mut world = world_with_spawns(Vec::new());
        world.content = std::sync::Arc::new(content());
        join(&mut world, PLAYER, 100.0);
        let entity = world.player_by_number(PLAYER).expect("player");
        let header = participant_header(PLAYER);
        assert_eq!(
            call(
                &mut world,
                GameModeClientMsg::JoinQueueForActivities {
                    header,
                    activities: vec![QueueActivity {
                        activity: super::super::queue::CATACOMBS_ACTIVITY,
                        extra: 1,
                        flag: 0,
                    }],
                },
            ),
            [ServerMessage::GameMode(
                GameModeServerMsg::StartMatchmakingRoleSelection {
                    header,
                    flag: 1,
                    activity: super::super::queue::CATACOMBS_ACTIVITY,
                    extra: 1,
                }
            )]
        );
        for role in [1, 3] {
            call(
                &mut world,
                GameModeClientMsg::SetMatchmakingRole { header, role },
            );
        }
        let calls = world.take_queue_calls();
        let [QueueCall::Join(queued)] = calls.as_slice() else {
            panic!("one join: {calls:?}");
        };
        assert_eq!(
            (
                queued.game_mode.as_str(),
                queued.level.as_str(),
                queued.fill
            ),
            (CATACOMBS_GAME_MODE, "nw_catacomb_00", false)
        );
        let states = participant_states(&mut world, entity);
        let entry = states.last().expect("the queue state");
        assert_eq!(entry.service.activity, Some(6));
        assert_eq!(
            entry.queuing.as_ref().map(|list| list
                .elements
                .iter()
                .map(|entry| entry.mode)
                .collect::<Vec<_>>()),
            Some(vec![0x2736_ba35]),
            "the retail entry names 2736ba35"
        );
        let match_id = Uuid::from_u128(5);
        world.queue_news(QueueNews {
            character: queued.character,
            game_mode: CATACOMBS_GAME_MODE.to_owned(),
            kind: QueueNewsKind::Matched { match_id },
        });
        let matched = participant_states(&mut world, entity);
        assert_eq!(
            matched.last().map(|state| state.service.clone()),
            Some(MatchmakingService {
                status: Some(1),
                match_id: Some(match_id.hyphenated().to_string()),
                ..MatchmakingService::default()
            }),
            "status 1 and the match id, no pop"
        );
        call(
            &mut world,
            GameModeClientMsg::AcceptMatch {
                header,
                match_id: match_id.hyphenated().to_string(),
                flag: 1,
            },
        );
        assert_eq!(
            world.take_queue_calls(),
            [QueueCall::Accept {
                character: queued.character,
                match_id
            }]
        );
        let place = MatchPlace {
            match_id,
            instance: Uuid::from_u128(INSTANCE),
            sim: slot("inst-1"),
            phase: PHASE,
            level: "nw_catacomb_00".to_owned(),
            team: 0,
            place: 0,
        };
        world.queue_news(QueueNews {
            character: queued.character,
            game_mode: CATACOMBS_GAME_MODE.to_owned(),
            kind: QueueNewsKind::Ready(place),
        });
        assert_eq!(
            messages_to(&world.take_messages(), PLAYER),
            [ServerMessage::GameMode(
                GameModeServerMsg::MatchmakingLoginRequested { header }
            )]
        );
        call(
            &mut world,
            GameModeClientMsg::MatchmakingLoginConfirmed { header },
        );
        tick(&mut world, NOW + 1);
        let out = tick(
            &mut world,
            NOW + 2 + super::super::PHASING_BEFORE_HANDOFF_MS,
        );
        let [start] = out.handoffs.as_slice() else {
            panic!("one handoff");
        };
        let spot = run_start_spot(&world.content, Uuid::from_u128(INSTANCE)).expect("spot");
        assert_eq!(
            (
                start.offer.position.x,
                start.offer.position.y,
                start.offer.position.z
            ),
            (spot.x, spot.y, spot.z),
            "the start room, not the map's teleport spot"
        );
    }

    /// The instance sim of a catacombs run: its arrival level has the layout's frame, and an
    /// arriving player gets 1234 and 4970 of no game mode (retail `CATACOMBS2` 15.707 s).
    /// An instance sim of the test run with `PLAYER` arrived in its start room, and the
    /// messages it was sent.
    fn arrived_run() -> (SimWorld, Vec<ServerMessage>) {
        let mode = catacombs();
        let mut settings = settings(4, 4);
        settings.instance_role = true;
        settings.level = LevelSpec::instance(
            &mode.map.folder,
            &mode.map.map_name,
            &mode.map.coatlicue_path,
            mode.map.frame,
        );
        let mut world = SimWorld::new(
            settings,
            crate::login::WorldIds::random(0).expect("ids"),
            std::sync::Arc::new(nw_sim_core::test_support::NoGround),
            slot("inst-1"),
            crate::combat::tests::test_combat(),
            std::sync::Arc::new(content()),
        );
        let mut admit = admit_at(PLAYER, 492.0, false);
        admit.progression.arena.stay = Some(InstanceStay {
            instance: Uuid::from_u128(INSTANCE),
            game_mode: mode.id.clone(),
            phase: PHASE,
            return_level: "newworld_vitaeeterna".to_owned(),
            return_position: StoredPoint::default(),
            activated_ns: 1,
            mutation_difficulty: None,
            seat: Some(ArenaSeat { team: 0, place: 0 }),
        });
        assert_eq!(
            world.open_instance(
                Uuid::from_u128(INSTANCE),
                &mode.id,
                PHASE,
                vec![admit.character],
                None,
                NOW
            ),
            InstanceOpened::Opened
        );
        world.admit(&admit, NOW, SERVER_TIME).expect("admit");
        let sent = messages_to(&world.take_messages(), PLAYER);
        (world, sent)
    }

    #[test]
    fn an_arriving_player_gets_the_run_frame_and_leaves_its_queue_group() {
        let (world, sent) = arrived_run();
        let run = world.catacomb_run().expect("run").clone();
        assert_eq!(world.arrival_level(), run.level());
        assert_eq!(
            run.layout(),
            &run_layout(&world.content, Uuid::from_u128(INSTANCE)).expect("layout")
        );
        let header = participant_header(PLAYER);
        let left = [
            ServerMessage::GameMode(GameModeServerMsg::QueueGroupRemoved { header }),
            ServerMessage::LeftGameModeQueue(LeftGameModeQueueMsg::of(header, NO_GAME_MODE)),
        ];
        assert!(
            sent.windows(left.len()).any(|pair| pair == left),
            "1234 and 4970 after the arrival: {sent:?}"
        );
    }

    /// A generated level whose layout cannot be made is refused (no rules in the content).
    #[test]
    fn a_run_without_rules_is_refused() {
        let mode = catacombs();
        let mut settings = settings(4, 4);
        settings.instance_role = true;
        settings.level = LevelSpec::instance(
            &mode.map.folder,
            &mode.map.map_name,
            &mode.map.coatlicue_path,
            mode.map.frame,
        );
        let mut world = SimWorld::new(
            settings,
            crate::login::WorldIds::random(0).expect("ids"),
            std::sync::Arc::new(nw_sim_core::test_support::NoGround),
            slot("inst-1"),
            crate::combat::tests::test_combat(),
            std::sync::Arc::new(PlayerContent {
                instances: nw_content::instances::InstanceCatalog::build([mode.clone()]),
                ..PlayerContent::default()
            }),
        );
        assert!(matches!(
            world.open_instance(
                Uuid::from_u128(INSTANCE),
                &mode.id,
                PHASE,
                Vec::new(),
                None,
                NOW
            ),
            InstanceOpened::Refused { .. }
        ));
    }

    #[test]
    fn a_piece_turns_with_its_tile() {
        let piece = TilePiece {
            slice: String::new(),
            asset: [3; 16],
            position_component: 1,
            offset: ART_OFFSET,
            yaw_deg: 0.0,
        };
        let placed = placed_piece(&piece, [128.0, 768.0, 120.0], 90.0);
        assert!((placed.position[0] - 128.0).abs() < 1e-3, "{placed:?}");
        assert!(
            (placed.position[1] - 770.0).abs() < 1e-3,
            "2 m east turns to 2 m north"
        );
        assert_eq!((placed.position[2], placed.yaw_deg), (120.0, 90.0));
        let unturned = placed_piece(&piece, [128.0, 768.0, 120.0], 0.0);
        let members = piece_members(&unturned, [9; nw_messages::RAW_ID_LEN]);
        assert!(matches!(
            &members[1].state,
            MemberState::Position(PositionInTheWorldReplicatedState { rotation: None, .. })
        ));
        assert!(matches!(
            &piece_members(&placed, [9; nw_messages::RAW_ID_LEN])[1].state,
            MemberState::Position(PositionInTheWorldReplicatedState {
                rotation: Some(_),
                ..
            })
        ));
    }

    /// The opening places the gameplay and the art entity of every tile; the end of the run
    /// takes them away; a layout with a variation the tile file lacks is refused.
    #[test]
    fn a_run_places_its_tiles_and_its_end_takes_them_away() {
        let mode = catacombs();
        let mut settings = settings(4, 4);
        settings.instance_role = true;
        settings.level = LevelSpec::instance(
            &mode.map.folder,
            &mode.map.map_name,
            &mode.map.coatlicue_path,
            mode.map.frame,
        );
        let content = content();
        let mut world = SimWorld::new(
            settings,
            crate::login::WorldIds::random(0).expect("ids"),
            std::sync::Arc::new(nw_sim_core::test_support::NoGround),
            slot("inst-1"),
            crate::combat::tests::test_combat(),
            std::sync::Arc::new(content.clone()),
        );
        let instance = Uuid::from_u128(INSTANCE);
        assert_eq!(
            world.open_instance(instance, &mode.id, PHASE, Vec::new(), None, NOW),
            InstanceOpened::Opened
        );
        let run = world.catacomb_run().expect("run").clone();
        assert_eq!(
            run.placed.len(),
            run.pieces.len() + 1,
            "and the run's own entity"
        );
        assert!(
            run.placed.len() > run.layout().rooms.len() * 2,
            "the slots' slices too"
        );
        let first = world
            .stations
            .members(run.placed[0])
            .expect("gameplay tile");
        let variation = &run.layout().rooms[0].variation;
        let tile = content.catacomb_tiles.get(variation).expect("tile");
        assert!(matches!(
            &first[0].state,
            MemberState::GdeMetadata(card) if card.asset.map(|asset| asset.id) == Some(tile.gameplay.asset)
        ));
        let art = world.stations.members(run.placed[1]).expect("art tile");
        assert!(matches!(
            &art[0].state,
            MemberState::GdeMetadata(card) if card.asset.map(|asset| asset.id) == Some(tile.art.asset)
        ));
        world.close_instance(instance);
        assert!(world.catacomb_run().is_none());
        assert!(
            run.placed
                .iter()
                .all(|number| world.stations.members(*number).is_none()),
            "the tiles left with the run"
        );
        let mut missing = content;
        missing.catacomb_tiles = CatacombTiles::default();
        let mut world = SimWorld::new(
            settings,
            crate::login::WorldIds::random(0).expect("ids"),
            std::sync::Arc::new(nw_sim_core::test_support::NoGround),
            slot("inst-1"),
            crate::combat::tests::test_combat(),
            std::sync::Arc::new(missing),
        );
        assert!(matches!(
            world.open_instance(instance, &mode.id, PHASE, Vec::new(), None, NOW),
            InstanceOpened::Refused { .. }
        ));
    }

    /// A room of the test rules at `cell` with the open sides `sides` (world) after `turns`.
    fn room(
        room_type: &str,
        cell: [i32; 2],
        sides: u8,
        turns: u8,
        locked: bool,
    ) -> nw_content::catacombs::PlacedRoom {
        nw_content::catacombs::PlacedRoom {
            room_id: room_type.to_owned(),
            room_type: room_type.to_owned(),
            connector: false,
            locked,
            lock_link: None,
            cell,
            sides,
            tile_id: "Straight".to_owned(),
            variation: "tile1_a_gameplay".to_owned(),
            turns,
            position: [
                cell[0] as f32 * TILE_SIZE_M,
                cell[1] as f32 * TILE_SIZE_M,
                120.0,
            ],
            yaw_deg: f32::from(turns) * 90.0,
        }
    }

    /// A reward room turned a quarter (its world north is its tile's east) below a locked boss
    /// room.
    fn reward_and_boss() -> CatacombLayout {
        CatacombLayout {
            cluster: "Test".to_owned(),
            theme: None,
            rooms: vec![
                room("Reward", [2, 2], NORTH | SOUTH, 1, false),
                room("EncounterBoss", [2, 3], SOUTH, 0, true),
            ],
            tile_size_m: TILE_SIZE_M,
        }
    }

    /// Each room kind spawns the slot of its `AliasCategory1`, its checkpoint and a gate on
    /// each open side named in the tile's frame; a side toward a locked room takes a key gate.
    #[test]
    fn a_room_spawns_its_kind_slots_checkpoint_and_side_gates() {
        let layout = reward_and_boss();
        let reward = slots_of(&layout, 0);
        for slot in [
            "EncounterReward",
            "PowerUps",
            "PowerUpSpawner",
            CHECKPOINT_SLOT,
        ] {
            assert!(reward.contains(&slot.to_owned()), "{slot} in {reward:?}");
        }
        assert!(
            reward.contains(&"LockGateEast".to_owned()),
            "world north (the locked boss) is the turned tile's east: {reward:?}"
        );
        assert!(
            reward.contains(&"GateWest".to_owned()),
            "world south is the tile's west"
        );
        assert_eq!(
            reward.iter().filter(|slot| slot.contains("Gate")).count(),
            2
        );
        let boss = slots_of(&layout, 1);
        assert!(boss.contains(&"EncounterBoss".to_owned()));
        assert!(
            boss.contains(&"LockGateSouth".to_owned()),
            "a locked room's own gate"
        );
        assert_eq!(
            room_slots("EncounterNamedEnemy"),
            ["EncounterNamedEnemy", "LevelUp"]
        );
        assert_eq!(room_slots("EncounterKey"), ["EncounterKey"]);
        assert_eq!(room_slots("EncounterHazard"), ["EncounterHazard"]);
        assert_eq!(
            room_slots("EncounterGate"),
            ["EncounterGate", "KeyLever", "LeverKey"]
        );
        assert_eq!(room_slots("Hallway"), ["Hallway", "ChestSilverSmall"]);
        assert!(slots_of(&layout, 2).is_empty(), "no such room");
    }

    /// The reward room's power-up spawner places a different power-up at each of its points;
    /// the boss room draws one entry of its alias slot, turned into the world; the same seed
    /// spawns the same, and a slot of another kind spawns nothing.
    #[test]
    fn a_room_spawns_its_slots_power_ups_and_one_alias_entry() {
        let layout = reward_and_boss();
        let tiles = tiles(&rules());
        let spawns = tile_spawns(&layout, &tiles, 11).expect("spawns");
        assert_eq!(tile_spawns(&layout, &tiles, 11), Some(spawns.clone()));
        let power_ups: Vec<&PlacedPiece> = spawns
            .pieces
            .iter()
            .filter(|piece| piece.asset[0] >= 0xf0)
            .collect();
        assert_eq!(power_ups.len(), POWER_UP_POINTS.len(), "one per point");
        assert_ne!(power_ups[0].asset, power_ups[1].asset, "different kinds");
        let centre = layout.rooms[0].position;
        let first_point = turned_point(POWER_UP_POINTS[0], centre, 90.0);
        assert!(
            power_ups
                .iter()
                .any(|piece| (piece.position[1] - first_point[1]).abs() < 1e-3),
            "at the points turned with the tile"
        );
        let level_ups = spawns
            .pieces
            .iter()
            .filter(|piece| piece.asset[1] == 3)
            .count();
        assert_eq!(level_ups, 0, "a reward room has no level-up shrine");
        let checkpoints = spawns
            .pieces
            .iter()
            .filter(|piece| piece.asset[1] == 2)
            .count();
        assert_eq!(checkpoints, 2, "a checkpoint per room");
        let bosses: Vec<&Encounter> = spawns
            .encounters
            .iter()
            .filter(|encounter| encounter.name == "EncounterBoss")
            .collect();
        assert_eq!(bosses.len(), 1, "one entry of the alias slot");
        assert_eq!(bosses[0].spawn_group, None);
        let boss_centre = layout.rooms[1].position;
        assert!(
            [1.0, 2.0]
                .iter()
                .any(|east| (bosses[0].position[0] - boss_centre[0] - east).abs() < 1e-3),
            "{:?}",
            bosses[0].position
        );
        assert!(
            spawns
                .encounters
                .iter()
                .all(|encounter| encounter.name == "EncounterBoss"),
            "a reward room has no encounter slot of the test tiles"
        );
        let picks: std::collections::BTreeSet<u32> = (0..32)
            .filter_map(|seed| tile_spawns(&layout, &tiles, seed))
            .flat_map(|spawns| spawns.encounters)
            .map(|encounter| encounter.position[0].to_bits())
            .collect();
        assert_eq!(picks.len(), 2, "both entries are drawn over many runs");
        let mut heavy = tiles.clone();
        for tile in heavy.by_variation.values_mut() {
            tile.encounters[0].weight = 0;
        }
        let only: std::collections::BTreeSet<u32> = (0..32)
            .filter_map(|seed| tile_spawns(&layout, &heavy, seed))
            .flat_map(|spawns| spawns.encounters)
            .map(|encounter| encounter.position[0].to_bits())
            .collect();
        assert_eq!(only.len(), 1, "an entry of no weight is never drawn");
        assert!(tile_spawns(&layout, &CatacombTiles::default(), 11).is_none());
    }

    /// The run's encounters are the instance sim's dungeon script: the start room spawns its
    /// extraction slot, the hallway and the boss their slots.
    #[test]
    fn a_run_script_is_the_rooms_encounters() {
        let mode = catacombs();
        let content = content();
        let map = LevelSpec::instance(
            "nw_catacomb_00",
            "NW_Catacomb_00",
            "coatlicue/NW_Catacomb_00",
            mode.map.frame,
        );
        let run = open_run(&content, &mode, map, Uuid::from_u128(INSTANCE)).expect("run");
        let names: Vec<&str> = run
            .script()
            .encounters
            .iter()
            .map(|encounter| encounter.name.as_str())
            .collect();
        assert_eq!(
            names
                .iter()
                .filter(|name| **name == "EncounterExtraction")
                .count(),
            1
        );
        assert_eq!(
            names
                .iter()
                .filter(|name| **name == "EncounterBoss")
                .count(),
            1
        );
        assert!(names.contains(&"Hallway"));
        assert_eq!(run.script().level, mode.map.folder);
        let start = &run.layout().rooms[0];
        let extraction = run
            .script()
            .encounters
            .iter()
            .find(|encounter| encounter.name == "EncounterExtraction")
            .expect("extraction");
        assert_eq!(
            extraction.position,
            turned_point([0.0, 0.0, -2.0], start.position, start.yaw_deg)
        );
    }

    /// The flags of a player present at the run's start (retail `CATACOMBS5` 110.581 s).
    const STARTED_FLAGS: [u8; PARTICIPANT_FLAGS_LEN] = [0x31, 0x02, 0x02, 0x00, 0x00, 0x00];
    /// The flags of that player in the boss fight (retail `CATACOMBS5` 815.986 s).
    const BOSS_FIGHT_FLAGS: [u8; PARTICIPANT_FLAGS_LEN] = [0x31, 0x22, 0x02, 0x00, 0x00, 0x00];

    /// The game mode state the entity of slot key `entity` holds (its merged state).
    fn held_state(world: &SimWorld, entity: u64) -> GameModeState {
        world
            .stations
            .members(entity)
            .expect("entity")
            .iter()
            .find_map(|member| match &member.state {
                MemberState::GameMode(state) => Some(state.clone()),
                _ => None,
            })
            .expect("a game mode state")
    }

    /// Puts player `PLAYER` at the centre of the room `index` of the run.
    fn stand_in_room(world: &mut SimWorld, index: usize) {
        let [x, y, z] = world.catacomb_run().expect("run").layout.rooms[index].position;
        let entity = world.player_by_number(PLAYER).expect("player");
        let limits = world.settings.limits;
        if let Some(mut body) = world.world.get_mut::<Body>(entity) {
            body.motion = super::super::Motion::new(Position { x, y, z }, NOW, &limits);
        }
    }

    /// The retail room states: each `RoomType` starts in its state; the map byte holds the
    /// tile's picture id over its turns (`CATACOMBS2` 17.409 s: `tile4_a` at 128, 768 turned
    /// twice is 0x86, the four-way `tile5_a` 0x74); a tile with no known picture has none.
    #[test]
    fn rooms_start_in_their_retail_states_and_tiles_show_their_pictures() {
        assert_eq!(first_room_state("EncounterNamedEnemy"), Some(0x0b));
        assert_eq!(first_room_state("Hallway"), Some(0x0a));
        assert_eq!(first_room_state("EncounterStart"), Some(0x10));
        assert_eq!(first_room_state("EncounterHazard"), Some(0x01));
        assert_eq!(first_room_state("Unknown"), None);
        assert_eq!(
            map_byte("Catacombs/Gameplay/Tiles/tile4_a_gameplay", 2),
            0x86
        );
        assert_eq!(map_byte("tile5_a_gameplay", 0), 0x74);
        assert_eq!(map_byte("tile3_a_gameplay", 4), 0x38, "four turns are none");
        assert_eq!(map_byte("tile5_d_gameplay", 1), 0, "no retail picture");
    }

    /// Retail `CATACOMBS2` 17.307 s and 17.409 s: every gameplay tile holds its room's state with
    /// the linked mode 1; the run's own entity waits (state 0) with the map of the level, row by
    /// row from the tile at (64, 64).
    #[test]
    fn a_run_tile_holds_its_room_state_and_the_run_holds_the_map() {
        let (world, _) = arrived_run();
        let run = world.catacomb_run().expect("run").clone();
        for (room, progress) in run.layout.rooms.iter().zip(&run.rooms) {
            let state = held_state(&world, progress.tile.expect("placed"));
            assert_eq!(state.script_state, first_room_state(&room.room_type));
            assert_eq!(state.linked_mode, Some(TILE_LINKED_MODE));
            assert_eq!(state.mode, Some(catacombs().crc));
            assert!(state.level_map.tiles.is_none(), "a tile holds no map");
        }
        let (mode_entity, _) = run.progress.entity.expect("run entity");
        let state = held_state(&world, mode_entity);
        assert_eq!(state.script_state, Some(RUN_WAITING_STATE));
        assert_eq!(state.linked_mode, Some(RUN_LINKED_MODE));
        let map = &state.level_map;
        assert_eq!(map.origin_m, Some([64, 64]));
        assert_eq!(map.tile_size_m, Some(64));
        let size = map.size_in_tiles.expect("size");
        let tiles = map
            .tiles
            .as_ref()
            .and_then(ReplicatedList::full)
            .expect("tiles");
        assert_eq!(
            tiles.elements.len(),
            usize::from(size[0]) * usize::from(size[1])
        );
        for room in &run.layout.rooms {
            let index = run.progress.map.index(room.cell).expect("on the map");
            assert_eq!(tiles.elements[index], map_byte(&room.variation, room.turns));
            assert_ne!(tiles.elements[index], 0, "every test tile has a picture");
        }
        assert_eq!(
            tiles.elements.iter().filter(|tile| **tile != 0).count(),
            run.layout.rooms.len()
        );
    }

    /// Retail `CATACOMBS2` 15.716 s: an arriving player joins the run's participant lists (on the
    /// run's entity and on every tile) and its participant state names the run's entity with
    /// the flags `11 02 02`.
    #[test]
    fn an_arriving_player_joins_the_run_lists_with_the_joined_flags() {
        let (mut world, _) = arrived_run();
        let entity = world.player_by_number(PLAYER).expect("player");
        let run = world.catacomb_run().expect("run").clone();
        let (_, gde_ref) = run.progress.entity.expect("run entity");
        for state_entity in run.state_entities() {
            let state = held_state(&world, state_entity);
            let participants = state
                .participants
                .as_ref()
                .and_then(ReplicatedList::full)
                .expect("participants");
            assert_eq!(participants.elements.len(), 1);
            assert_eq!(
                state
                    .statuses
                    .as_ref()
                    .and_then(ReplicatedList::full)
                    .map(|list| list.elements.clone()),
                Some(vec![PLAYING_STATUS])
            );
        }
        let joined = participant_states(&mut world, entity)
            .into_iter()
            .find(|state| state.active.is_some())
            .expect("the joined state");
        assert_eq!(joined.flags, Some(JOINED_FLAGS));
        let active = joined.active.expect("active modes");
        assert_eq!(active.elements[0].raw_id, gde_ref);
        assert_eq!(active.elements[0].mode, catacombs().crc);
    }

    /// Retail `CATACOMBS5` 110.581 to 111.065 s: when the players left the start room the run
    /// starts (state 1, 1800 s run timer, three respawns per player), the start tile goes to
    /// 0x0e, the flags gain `RestoreVitalsOnExitedGameMode`; the tile a player goes into is
    /// visited and a boss room next to it shows its icon.
    #[test]
    fn leaving_the_start_room_starts_the_run_and_marks_the_map() {
        let (mut world, _) = arrived_run();
        let entity = world.player_by_number(PLAYER).expect("player");
        stand_in_room(&mut world, 0);
        world.tick_catacombs();
        let run = world.catacomb_run().expect("run").clone();
        let (mode_entity, _) = run.progress.entity.expect("run entity");
        assert!(!run.progress.started, "in the start room the run waits");
        assert_eq!(
            held_state(&world, mode_entity).script_state,
            Some(RUN_WAITING_STATE)
        );
        let hallway = run
            .layout
            .rooms
            .iter()
            .position(|room| room.room_type == "Hallway")
            .expect("a hallway");
        drop(participant_states(&mut world, entity));
        stand_in_room(&mut world, hallway);
        let server_ms = world.server_ms;
        world.tick_catacombs();
        let state = held_state(&world, mode_entity);
        assert_eq!(state.script_state, Some(RUN_STARTED_STATE));
        let values = state.values.expect("values").elements;
        assert!(values.contains(&(RESPAWNS_REMAINING_VALUE, RESPAWNS_PER_PARTICIPANT)));
        let timers = state.timers.expect("timers").elements;
        assert!(timers.contains(&(CATACOMBS_TIMER, server_ms + RUN_DURATION_MS)));
        let run = world.catacomb_run().expect("run").clone();
        let start_tile = run.rooms[0].tile.expect("start tile");
        assert_eq!(
            held_state(&world, start_tile).script_state,
            Some(START_OPEN_STATE)
        );
        assert!(
            participant_states(&mut world, entity)
                .iter()
                .any(|state| state.flags == Some(STARTED_FLAGS) && state.active.is_none()),
            "the flags change alone"
        );
        let index = run
            .progress
            .map
            .index(run.layout.rooms[hallway].cell)
            .expect("tile");
        let visited = state
            .level_map
            .visited
            .as_ref()
            .and_then(ReplicatedList::full)
            .expect("visited");
        assert_ne!(
            visited.elements[index / TILES_PER_BYTE] & 1 << (index % TILES_PER_BYTE),
            0
        );
        let icons = state
            .level_map
            .icons
            .as_ref()
            .and_then(ReplicatedList::full)
            .expect("icons");
        let start_cell = run.layout.rooms[0].cell;
        assert!(icons.elements.contains(&MapIcon {
            key: EXTRACTION_ICON,
            position_m: run.progress.map.centre_m(start_cell),
        }));
        let boss = run
            .layout
            .rooms
            .iter()
            .find(|room| room.room_type == "EncounterBoss")
            .expect("a boss room");
        let next_to_boss = SIDES.iter().any(|(_, step)| {
            [boss.cell[0] - step[0], boss.cell[1] - step[1]] == run.layout.rooms[hallway].cell
        });
        assert_eq!(
            icons.elements.iter().any(|icon| icon.key == BOSS_ICON),
            next_to_boss,
            "the boss icon shows from the next tile"
        );
        world.tick_catacombs();
        assert_eq!(
            held_state(&world, mode_entity).level_map.icons,
            state.level_map.icons,
            "an icon shows once"
        );
    }

    /// Retail `CATACOMBS2` 53.029 s: a room's tile goes to 7 when its last encounter ends, in a
    /// room kind that clears; a reward room keeps its state.
    #[test]
    fn a_room_whose_last_encounter_ends_is_cleared() {
        let (mut world, _) = arrived_run();
        let run = world.catacomb_run().expect("run").clone();
        let (index, room) = run
            .rooms
            .iter()
            .enumerate()
            .find(|(_, room)| {
                !room.open_encounters.is_empty() && CLEARABLE_STATES.contains(&room.state)
            })
            .expect("a room with encounters");
        let tile = room.tile.expect("tile");
        let (last, others) = room.open_encounters.split_last().expect("encounters");
        for encounter in others {
            world.catacomb_encounter_done(*encounter);
        }
        if !others.is_empty() {
            assert_eq!(
                held_state(&world, tile).script_state,
                Some(room.state),
                "one is left"
            );
        }
        world.catacomb_encounter_done(*last);
        assert_eq!(
            held_state(&world, tile).script_state,
            Some(CLEARED_ROOM_STATE)
        );
        assert_eq!(
            world.catacomb_run().expect("run").rooms[index].state,
            CLEARED_ROOM_STATE
        );
        let mut reward = world.catacomb_run().expect("run").clone();
        reward.rooms[index].state = REWARD_ROOM_STATE;
        reward.rooms[index].open_encounters = vec![*last];
        assert_eq!(
            reward.encounter_done(*last),
            None,
            "a reward room keeps its state"
        );
    }

    /// Retail `CATACOMBS5` 815.986 s: a player in the boss room while the boss lives is in the
    /// boss fight (`31 22 02`); the flag goes when the boss room is cleared.
    #[test]
    fn a_player_in_the_boss_room_is_in_the_boss_fight_until_it_is_cleared() {
        let (mut world, _) = arrived_run();
        let entity = world.player_by_number(PLAYER).expect("player");
        let run = world.catacomb_run().expect("run").clone();
        let boss = run
            .layout
            .rooms
            .iter()
            .position(|room| room.room_type == "EncounterBoss")
            .expect("a boss room");
        stand_in_room(&mut world, boss);
        world.tick_catacombs();
        let flags: Vec<_> = participant_states(&mut world, entity)
            .into_iter()
            .filter_map(|state| state.flags)
            .collect();
        assert_eq!(flags.last(), Some(&BOSS_FIGHT_FLAGS), "{flags:x?}");
        for encounter in run.rooms[boss].open_encounters.clone() {
            world.catacomb_encounter_done(encounter);
        }
        world.tick_catacombs();
        let flags: Vec<_> = participant_states(&mut world, entity)
            .into_iter()
            .filter_map(|state| state.flags)
            .collect();
        assert_eq!(flags, vec![STARTED_FLAGS]);
        world.tick_catacombs();
        assert!(
            participant_states(&mut world, entity).is_empty(),
            "no change, no state"
        );
    }
}
