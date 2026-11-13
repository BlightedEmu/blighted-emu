// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Javelin::GameModeReplicatedState` (class 2343): the state of a game mode entity, the one
//! network entity of a duel, an arena match, an Outpost Rush match, a war or an expedition run.
//! Layout from the client's member table (30 children). The children Observed in the retail
//! game modes (game mode study 2026-11-11: 2 duels of `duel_capture`, 2 expedition entries):
//!
//! - the first state of a mode holds its script state, an empty spawner list, the mode's id
//!   twice (CRC-32 of the lowercase `GameModeId`), each participant (its entity's raw id, its
//!   facet entity and a second id), its status, its team and its character id, empty raid ids
//!   and values, its timers, the empty global affliction data and the linked mode 0;
//! - a later state holds the children that changed: a list in its full form with its new
//!   version (the timers and the values of a duel), or the changed elements of a list (one
//!   participant's status).
//!
//! - the state of a catacombs run (retail `CATACOMBS2` 17.409 s) also holds the map of its
//!   generated level (children 23 to 28, [`GeneratedMap`]): the map origin and the tile size,
//!   the map size in tiles, one byte per tile (its map picture and its quarter turns), one bit
//!   per tile a player went into, and the icons of the rooms.
//!
//! - the state of an Outpost Rush match also holds its event feed (children 12 to 21, `Event1`
//!   to `Event10`, [`GameModeEvent`]): a ring of ten slots that each new event fills in turn
//!   (Observed: `opr_que_and_lost` 578.634 s to 2087.883 s, `Event10` then `Event1` again); the
//!   first state holds every filled slot, a later state the new slot only.
//!
//! The sim writes this state; the captures' own states read through the member table.

use crate::afflictions::GlobalAffliction;
use crate::items::FullList;
use crate::list_delta::{ReplicatedList, write_child};
use crate::slots::{SLOT_0, write_known_wide_children};
use crate::wire::{MessageError, RAW_ID_LEN, Writer};

/// Type index of `Javelin::GameModeReplicatedState`.
pub const GAME_MODE_TYPE_INDEX: u64 = 2343;
/// The children of the class.
const CHILDREN: u8 = 30;
const SCRIPT_STATE_CHILD: u8 = 0;
const SPAWNED_CHILD: u8 = 2;
const MODE_CHILD: u8 = 3;
const MAP_CHILD: u8 = 4;
const PARTICIPANTS_CHILD: u8 = 5;
const STATUSES_CHILD: u8 = 6;
const TEAMS_CHILD: u8 = 7;
const CHARACTERS_CHILD: u8 = 8;
const RAIDS_CHILD: u8 = 9;
const VALUES_CHILD: u8 = 10;
const TIMERS_CHILD: u8 = 11;
/// `Event1`, the first slot of the event feed.
const FIRST_EVENT_CHILD: u8 = 12;
/// The slots of the event feed (`Event1` to `Event10`).
pub const EVENT_SLOTS: u8 = 10;
const AFFLICTION_CHILD: u8 = 22;
const MAP_ORIGIN_CHILD: u8 = 23;
const TILE_SIZE_CHILD: u8 = 24;
const MAP_SIZE_CHILD: u8 = 25;
const TILES_CHILD: u8 = 26;
const VISITED_CHILD: u8 = 27;
const ICONS_CHILD: u8 = 28;
const LINKED_MODE_CHILD: u8 = 29;
/// The flag of a participant's character id in its raw form (Observed: `05`, then 16 bytes).
const RAW_CHARACTER_ID: u8 = 0x05;

/// One participant of a game mode (`participantFacetRefs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParticipantRef {
    /// The raw id of its entity.
    pub raw_id: [u8; RAW_ID_LEN],
    /// Its facet entity.
    pub facet: u64,
    /// A second id (name Unknown; retail: constant per player over its game modes).
    pub second: u64,
}

/// One icon of a generated level's map (`icons`): the CRC-32 of its name and its place (Observed:
/// `bossicon` at the boss room's tile centre, `CATACOMBS2` 17.409 s; the place is written as two
/// 32-bit floats, east then north).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapIcon {
    /// CRC-32 of the icon's lowercase name.
    pub key: u32,
    /// Its place east and north, whole metres (retail icons stand at tile centres).
    pub position_m: [u16; 2],
}

/// The map of a generated level (children 23 to 28). A `None` child is not sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GeneratedMap {
    /// Child 23, `mapOrigin`: the centre of the first tile, east then north, metres (Observed:
    /// `0040 0040` in every retail run).
    pub origin_m: Option<[u16; 2]>,
    /// Child 24, `tileSizeMeters`.
    pub tile_size_m: Option<u8>,
    /// Child 25, `mapSizeInTiles`: east then north.
    pub size_in_tiles: Option<[u8; 2]>,
    /// Child 26, `tileUiFilenameIdAndRotation`: per tile, row by row from the origin, its map
    /// picture id shifted left by two and its quarter turns in the low two bits; 0 for no tile.
    pub tiles: Option<ReplicatedList<u8>>,
    /// Child 27, `tileVisited`: one bit per tile in the order of `tiles`, lowest bit first.
    pub visited: Option<ReplicatedList<u8>>,
    /// Child 28, `icons`.
    pub icons: Option<ReplicatedList<MapIcon>>,
}

/// One event of the feed (`Event1` to `Event10`): the CRC-32 of its lowercase name, the server ms
/// since 2000-01-01 it happened and its number (the member table: 4, 8 and 4 bytes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GameModeEvent {
    /// CRC-32 of the event's lowercase name.
    pub name: u32,
    /// Server ms since 2000-01-01 of the event.
    pub at_ms: u64,
    /// The event's number (its meaning is the event's).
    pub number: u32,
}

/// A keyed number of a game mode: the CRC-32 of its name and its value (`values`: a count;
/// `syncedTimers`: server ms since 2000-01-01).
pub type Keyed = (u32, u64);

/// The state of a game mode entity. A `None` child is not sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameModeState {
    /// Child 0, `curScriptStateId`.
    pub script_state: Option<u8>,
    /// Child 2, `spawnedEntityIdsBySpawnerId`: each spawner id and its entity.
    pub spawned: Option<FullList<Keyed>>,
    /// Child 3, `gameModeId`: CRC-32 of the lowercase `GameModeId`.
    pub mode: Option<u32>,
    /// Child 4, `gameModeMapId` (Observed: the same value as the mode's id).
    pub map: Option<u32>,
    /// Child 5, `participantFacetRefs`.
    pub participants: Option<ReplicatedList<ParticipantRef>>,
    /// Child 6, `participantStatuses`, in participant order.
    pub statuses: Option<ReplicatedList<u8>>,
    /// Child 7, `participantTeamIndexes`, in participant order.
    pub teams: Option<ReplicatedList<u8>>,
    /// Child 8, `participantCharacterIds`, in participant order.
    pub characters: Option<ReplicatedList<[u8; RAW_ID_LEN]>>,
    /// Child 9, `raidIds`.
    pub raids: Option<ReplicatedList<u64>>,
    /// Child 10, `values`.
    pub values: Option<FullList<Keyed>>,
    /// Child 11, `syncedTimers`.
    pub timers: Option<FullList<Keyed>>,
    /// Children 12 to 21, `Event1` to `Event10`: each slot sent (0 to 9) and its event.
    pub events: Vec<(u8, GameModeEvent)>,
    /// Child 22, `globalAfflictionData`: sent empty with the first state when `affliction` is
    /// `None` (Observed in every first state: [`GlobalAffliction::NONE`]).
    pub no_affliction: bool,
    /// Child 22, `globalAfflictionData`, with a meter: the mutation curse meter of a mutated
    /// expedition (Observed: retail `20261002-184508-050`); it goes before `no_affliction`.
    pub affliction: Option<GlobalAffliction>,
    /// Children 23 to 28: the map of a generated level.
    pub level_map: GeneratedMap,
    /// Child 29, `linkedMode`.
    pub linked_mode: Option<u8>,
}

fn write_keyed(writer: &mut Writer, (key, value): &Keyed) {
    writer.u32(*key).u64(*value);
}

fn write_keyed_list(
    writer: &mut Writer,
    list: Option<&FullList<Keyed>>,
) -> Result<(), MessageError> {
    list.map_or(Ok(()), |list| {
        list.write(writer, |writer, entry| {
            write_keyed(writer, entry);
            Ok(())
        })
    })
}

impl GameModeState {
    /// Writes the state for the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a list does not write.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let present = [
            (SCRIPT_STATE_CHILD, self.script_state.is_some()),
            (SPAWNED_CHILD, self.spawned.is_some()),
            (MODE_CHILD, self.mode.is_some()),
            (MAP_CHILD, self.map.is_some()),
            (PARTICIPANTS_CHILD, self.participants.is_some()),
            (STATUSES_CHILD, self.statuses.is_some()),
            (TEAMS_CHILD, self.teams.is_some()),
            (CHARACTERS_CHILD, self.characters.is_some()),
            (RAIDS_CHILD, self.raids.is_some()),
            (VALUES_CHILD, self.values.is_some()),
            (TIMERS_CHILD, self.timers.is_some()),
            (
                AFFLICTION_CHILD,
                self.no_affliction || self.affliction.is_some(),
            ),
            (MAP_ORIGIN_CHILD, self.level_map.origin_m.is_some()),
            (TILE_SIZE_CHILD, self.level_map.tile_size_m.is_some()),
            (MAP_SIZE_CHILD, self.level_map.size_in_tiles.is_some()),
            (TILES_CHILD, self.level_map.tiles.is_some()),
            (VISITED_CHILD, self.level_map.visited.is_some()),
            (ICONS_CHILD, self.level_map.icons.is_some()),
            (LINKED_MODE_CHILD, self.linked_mode.is_some()),
        ];
        let mask = present
            .iter()
            .filter(|(_, present)| *present)
            .map(|(child, _)| *child)
            .chain(
                self.events
                    .iter()
                    .filter(|(slot, _)| *slot < EVENT_SLOTS)
                    .map(|(slot, _)| FIRST_EVENT_CHILD + slot),
            )
            .fold(0_u32, |mask, child| mask | 1 << child);
        if mask == 0 {
            writer.u8(0);
            return Ok(());
        }
        writer.u8(SLOT_0);
        let mut result = Ok(());
        write_known_wide_children(writer, mask, CHILDREN, |writer, child| {
            let written = match child {
                SCRIPT_STATE_CHILD => {
                    self.script_state.map(|state| writer.u8(state));
                    Ok(())
                }
                SPAWNED_CHILD => write_keyed_list(writer, self.spawned.as_ref()),
                MODE_CHILD => {
                    self.mode.map(|mode| writer.u32(mode));
                    Ok(())
                }
                MAP_CHILD => {
                    self.map.map(|map| writer.u32(map));
                    Ok(())
                }
                PARTICIPANTS_CHILD => {
                    write_child(writer, self.participants.as_ref(), |writer, participant| {
                        writer
                            .bytes(&participant.raw_id)
                            .u64(participant.facet)
                            .u64(participant.second);
                    })
                }
                STATUSES_CHILD => write_child(writer, self.statuses.as_ref(), |writer, status| {
                    writer.u8(*status);
                }),
                TEAMS_CHILD => write_child(writer, self.teams.as_ref(), |writer, team| {
                    writer.u8(*team);
                }),
                CHARACTERS_CHILD => {
                    write_child(writer, self.characters.as_ref(), |writer, character| {
                        writer.u8(RAW_CHARACTER_ID).bytes(character);
                    })
                }
                RAIDS_CHILD => write_child(writer, self.raids.as_ref(), |writer, raid| {
                    writer.u64(*raid);
                }),
                VALUES_CHILD => write_keyed_list(writer, self.values.as_ref()),
                TIMERS_CHILD => write_keyed_list(writer, self.timers.as_ref()),
                AFFLICTION_CHILD => {
                    self.affliction
                        .unwrap_or(GlobalAffliction::NONE)
                        .write(writer);
                    Ok(())
                }
                MAP_ORIGIN_CHILD => {
                    if let Some([east, north]) = self.level_map.origin_m {
                        writer.u16(east).u16(north);
                    }
                    Ok(())
                }
                TILE_SIZE_CHILD => {
                    self.level_map.tile_size_m.map(|size| writer.u8(size));
                    Ok(())
                }
                MAP_SIZE_CHILD => {
                    if let Some([east, north]) = self.level_map.size_in_tiles {
                        writer.u8(east).u8(north);
                    }
                    Ok(())
                }
                TILES_CHILD => {
                    write_child(writer, self.level_map.tiles.as_ref(), |writer, tile| {
                        writer.u8(*tile);
                    })
                }
                VISITED_CHILD => {
                    write_child(writer, self.level_map.visited.as_ref(), |writer, bits| {
                        writer.u8(*bits);
                    })
                }
                ICONS_CHILD => {
                    write_child(writer, self.level_map.icons.as_ref(), |writer, icon| {
                        let [east, north] = icon.position_m;
                        writer
                            .u32(icon.key)
                            .f32(f32::from(east))
                            .f32(f32::from(north));
                    })
                }
                child if (FIRST_EVENT_CHILD..FIRST_EVENT_CHILD + EVENT_SLOTS).contains(&child) => {
                    let slot = child - FIRST_EVENT_CHILD;
                    if let Some((_, event)) = self.events.iter().find(|(at, _)| *at == slot) {
                        writer.u32(event.name).u64(event.at_ms).u32(event.number);
                    }
                    Ok(())
                }
                _ => {
                    self.linked_mode.map(|mode| writer.u8(mode));
                    Ok(())
                }
            };
            if result.is_ok() {
                result = written;
            }
        });
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::list_delta::ListChange;

    /// CRC-32 of `duel`: the mode of a duel.
    const DUEL: u32 = 0x9bb4_a762;
    /// The keys of a duel's timers (names Unknown): the countdown's end, the fight's end.
    const COUNTDOWN_TIMER: u32 = 0xf920_f724;
    const FIGHT_TIMER: u32 = 0x47d6_e652;
    /// The key of a duel's end value (name Unknown).
    const END_VALUE: u32 = 0x428a_d202;
    /// Made-up participants.
    const FIRST: ParticipantRef = ParticipantRef {
        raw_id: [0xa1; RAW_ID_LEN],
        facet: 0xa2a2_a2a2_a2a2_a2a2,
        second: 0xa3a3_a3a3_a3a3_a3a3,
    };
    const SECOND: ParticipantRef = ParticipantRef {
        raw_id: [0xb1; RAW_ID_LEN],
        facet: 0xb2b2_b2b2_b2b2_b2b2,
        second: 0xb3b3_b3b3_b3b3_b3b3,
    };
    const FIRST_CHARACTER: [u8; RAW_ID_LEN] = [0xc1; RAW_ID_LEN];
    const SECOND_CHARACTER: [u8; RAW_ID_LEN] = [0xc2; RAW_ID_LEN];

    fn hex(state: &GameModeState) -> String {
        let mut writer = Writer::new();
        state.write(&mut writer).expect("the state writes");
        writer
            .finish()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn full<T>(version: u64, elements: Vec<T>) -> FullList<T> {
        FullList {
            version: Some(version),
            elements,
        }
    }

    /// Retail `duel_capture` at 67.299 s: the first state of a duel (192 bytes), with made-up
    /// participant and character ids.
    #[test]
    fn the_first_state_of_a_retail_duel_writes_the_same_bytes() {
        let first = GameModeState {
            script_state: Some(1),
            spawned: Some(full(2, Vec::new())),
            mode: Some(DUEL),
            map: Some(DUEL),
            participants: Some(ReplicatedList::Full(full(2, vec![FIRST, SECOND]))),
            statuses: Some(ReplicatedList::Full(full(2, vec![3, 3]))),
            teams: Some(ReplicatedList::Full(full(2, vec![0, 1]))),
            characters: Some(ReplicatedList::Full(full(
                2,
                vec![FIRST_CHARACTER, SECOND_CHARACTER],
            ))),
            raids: Some(ReplicatedList::Full(full(2, Vec::new()))),
            values: Some(full(2, Vec::new())),
            timers: Some(full(2, vec![(COUNTDOWN_TIMER, 0x00c2_58a8_cef9)])),
            no_affliction: true,
            linked_mode: Some(0),
            ..GameModeState::default()
        };
        let expected = [
            "01fd01000102009bb4a7629bb4a76200010202",
            "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a2a2a2a2a2a2a2a2a3a3a3a3a3a3a3a3",
            "b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b2b2b2b2b2b2b2b2b3b3b3b3b3b3b3b3",
            "0001020203039f0001020200010001020205c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1",
            "05c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2000102000001020000010201",
            "f920f724000000c258a8cef98082",
            "00000000000000000000000000000000000000000000000000000000ff000200",
        ]
        .concat();
        assert_eq!(hex(&first), expected);
    }

    /// Retail `duel_capture`: the fight's start at 73.315 s, the duel's end at 83.914 s and the
    /// loser's status back at 86.947 s.
    #[test]
    fn the_later_states_of_a_retail_duel_write_the_same_bytes() {
        let fight = GameModeState {
            script_state: Some(3),
            timers: Some(full(4, vec![(FIGHT_TIMER, 0x00c2_58aa_2e96)])),
            ..GameModeState::default()
        };
        assert_eq!(hex(&fight), "018103100001040147d6e652000000c258aa2e96");
        let status = |version, value| {
            Some(ReplicatedList::Changes(vec![ListChange {
                index: 1,
                version,
                value,
            }]))
        };
        let end = GameModeState {
            script_state: Some(0),
            statuses: status(0x12, 0x0b),
            values: Some(full(0x12, vec![(END_VALUE, 0)])),
            timers: Some(full(0x12, vec![(FIGHT_TIMER, 0x00c2_58aa_57ec)])),
            ..GameModeState::default()
        };
        assert_eq!(
            hex(&end),
            "01c10001010101120b1800011201428ad20200000000000000000001120147d6e652000000c258aa57ec"
        );
        let back = GameModeState {
            statuses: status(0x13, 3),
            ..GameModeState::default()
        };
        assert_eq!(hex(&back), "0140010101011303");
        assert_eq!(hex(&GameModeState::default()), "00", "no child: no slot");
    }

    /// Retail `20261002-184508-050` at Unix ms 1790964499092: the change of the Fiendish meter
    /// alone, 1 held for 9,999 s (child 22 only).
    #[test]
    fn a_retail_curse_meter_change_writes_the_same_bytes() {
        let change = GameModeState {
            affliction: Some(GlobalAffliction::of(
                crate::afflictions::MeterLine {
                    last: 1.0,
                    last_ns: 844_279_689_985_784_851,
                    target: 1.0,
                    target_ns: 844_289_688_985_784_851,
                },
                45.0,
                11,
                false,
            )),
            ..GameModeState::default()
        };
        assert_eq!(
            hex(&change),
            "01808080023f8000000bb77be7a4adfc133f8000000bb784ffb785d213423400000b00"
        );
    }

    /// Retail `opr_que_and_lost`: three later states of the Outpost Rush match that hold one
    /// slot of the event feed each, `outpostattacked` (`8d700253`) with outpost 2: `Event6` at
    /// 1021.875 s, `Event10` at 1289.679 s and `Event2` after the ring turned (1354.779 s).
    #[test]
    fn a_retail_outpost_rush_event_writes_the_same_bytes() {
        const OUTPOST_ATTACKED: u32 = 0x8d70_0253;
        let event = |slot, at_ms| GameModeState {
            events: vec![(
                slot,
                GameModeEvent {
                    name: OUTPOST_ATTACKED,
                    at_ms,
                    number: 2,
                },
            )],
            ..GameModeState::default()
        };
        assert_eq!(
            hex(&event(5, 0x00c2_766c_a55f)),
            "018080088d700253000000c2766ca55f00000002"
        );
        assert_eq!(
            hex(&event(9, 0x00c2_7670_bb76)),
            "01808080018d700253000000c27670bb7600000002"
        );
        assert_eq!(
            hex(&event(1, 0x00c2_7671_b9d3)),
            "0180408d700253000000c27671b9d300000002"
        );
        assert_eq!(hex(&event(EVENT_SLOTS, 0)), "00", "no slot past the ring");
    }

    /// Retail `CATACOMBS2` 17.409 s: the map children of the run's first state (9 x 14 tiles of
    /// 64 m from (64, 64), 30 tiles with a picture, 5 visited, 4 icons), with the affliction and
    /// the linked mode around them; the other children left out.
    #[test]
    fn the_map_of_a_retail_catacombs_run_writes_the_same_bytes() {
        const BOSS_ICON: u32 = 0xfd06_5714;
        const EXTRACTION_ICON: u32 = 0xfab6_c9fe;
        const KEY_ROOM_ICON: u32 = 0x3ce7_487f;
        const REWARD_ROOM_ICON: u32 = 0xb486_91b9;
        const TILES: &str = "000000000000b73600000000000000e4bc00000000000000e83436000000000086acebf20000afebd7ddf3d3d2003774bf3500bcec840080608586003439850000dce73a83d785000000ac3b7439d13600000000d4f1bfbf3500000086d4ae00000000008638ade400000000876874e7b500000000008035000000000000";
        let tiles = (0..TILES.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&TILES[at..at + 2], 16).expect("hex"))
            .collect();
        let mut visited = vec![0; 16];
        visited[12] = 0x70;
        visited[13] = 0x60;
        let icon = |key, east, north| MapIcon {
            key,
            position_m: [east, north],
        };
        let state = GameModeState {
            no_affliction: true,
            level_map: GeneratedMap {
                origin_m: Some([64, 64]),
                tile_size_m: Some(64),
                size_in_tiles: Some([9, 14]),
                tiles: Some(ReplicatedList::Full(full(4, tiles))),
                visited: Some(ReplicatedList::Full(full(14, visited))),
                icons: Some(ReplicatedList::Full(full(
                    11,
                    vec![
                        icon(BOSS_ICON, 192, 704),
                        icon(EXTRACTION_ICON, 128, 768),
                        icon(KEY_ROOM_ICON, 256, 768),
                        icon(REWARD_ROOM_ICON, 192, 832),
                    ],
                ))),
            },
            linked_mode: Some(0),
            ..GameModeState::default()
        };
        let expected = [
            "01808080fe",
            "00000000000000000000000000000000000000000000000000000000ff00",
            "00400040",
            "40",
            "090e",
            "0001047e",
            TILES,
            "00010e10",
            "00000000000000000000000070600000",
            "03",
            "00010b04fd0657144340000044300000fab6c9fe4300000044400000",
            "3ce7487f4380000044400000b48691b94340000044500000",
            "00",
        ]
        .concat();
        assert_eq!(hex(&state), expected);
        let visit = GameModeState {
            level_map: GeneratedMap {
                visited: Some(ReplicatedList::Changes(vec![ListChange {
                    index: 11,
                    version: 0x11,
                    value: 0x20,
                }])),
                ..GeneratedMap::default()
            },
            ..GameModeState::default()
        };
        assert_eq!(
            hex(&visit),
            "018080804001010b011120",
            "retail 78.839 s: one tile visited"
        );
    }
}
