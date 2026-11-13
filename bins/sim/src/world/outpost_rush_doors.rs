// SPDX-License-Identifier: AGPL-3.0-or-later
//! The fort barriers of an Outpost Rush match: the doors (`MB::DoorComponentReplicatedState`,
//! [`nw_messages::door`]) that keep each team in its fort during the pregame (door study
//! 2026-11-12).
//!
//! **The entities** (Observed, client slices): the game mode slice of each map places three
//! barriers per fort, `FortA_Barrier_01` to `_03` and `FortB_Barrier_01` to `_03`, spawners of
//! `slices/outpostrush/fort/or_fort_barriers.dynamicslice` (`GameMode_OutpostRush_005_Vertical`)
//! and `or_fort_barriers_trench.dynamicslice` (`OutpostRush_Layout_004_Trench`). Neither spawns
//! on its own (`m_spawnOnInitialize` and `m_spawnOnTrigger` are off): the game mode script spawns
//! them.
//!
//! **The spawn** (Observed, retail `opr_que_and_lost` on Vertical 578.667 s, `opr_match_4`,
//! `opr_match_2`, `OPR-coral-divide2`, `OPR-Coral-Divide-2`, `OPR_BAGGUNS`, `OPR---Tower`,
//! `OPR---Tower-2`: every barrier of both maps in 591 + 194 spawn groups): a card of the slice,
//! the ownership on component 1, the position on component 2 and the door on component 4, its
//! state `Closed` (`01 01 01`). The place is the spawner's place in the slice plus the slice's
//! placement in the level: x and y plus 1024 on both maps (12 of 12 barriers to the centimetre),
//! the height plus 31.98 m on Vertical and 40.98 m on Trench (the retail heights of the 12
//! barriers, each within one height step). The rotation is the quaternion of the spawner's yaw
//! with `w` not negative when the turn is less than 120 degrees from the slice's front and `z` not
//! negative past it (the matrix-to-quaternion rule; 12 of 12 retail rotations). The barriers are
//! there when the player arrives in the pregame (578.667 s, the match opened before).
//!
//! **The ownership** (Observed): no guild, white crest, no owner (kind 2), permission rule 4,
//! no structure name, faction 0, no PvP or FFA flag, options 0; the raid is 0 in the pregame
//! (`opr_que_and_lost` 578.667 to 626.237 s, `opr_match_4` 0.597 to 1.425 s, `OPR-Coral-Divide-2`
//! 47.793 s) and later the raid of the fort's team: `FortA` the first raid of the game mode
//! entity (`0x213` of `213 214` in `opr_que_and_lost`, `0x6fe` in `opr_match_4`, `0xb9d` in
//! `OPR-Coral-Divide-2`), `FortB` the second. The door state never changes and no barrier is
//! taken away during a match (8,852 door bodies in 148 ledgers, all `Closed`; each barrier
//! comes back after each slot stop of a player's view).
//!
//! Inferred: the raid turns to the team's raid when the pregame ends (the change lies between
//! 626.237 and 778.264 s in `opr_que_and_lost`, around the pregame's end at 665.446 s; no capture
//! sends the change itself), so a closed barrier stops every player in the pregame and lets its
//! own team's raid through once the match runs; the client applies the barrier's collision by
//! the door state and the ownership (the component's `m_collisionEntity`), and the sim checks no
//! collision of its own. The barriers go with the game mode entity at the end of `Cleanup`.
//!
//! **The island** (Observed): the island's layout slice `POIs/OutpostRush/OutpostRush_Layout_00`
//! (the open world capital at 1272.7565, 11016.635, 68.71661, no rotation) places the six fort
//! barriers of `or_fort_barriers.dynamicslice`; each retail barrier stood at its spawner's place
//! plus the capital's (`OPR_TEST3` 216.676 to 764.026 s and `OPR---ISLAND`: 6 of 6 to the
//! centimetre, the heights within one height step) with the team raid of its fort in the running
//! match (`OPR_TEST3` 216.676 s: `FortB_Barrier_03` with `0x1196`, the second of the raids
//! `1195 1196`).
//!
//! **The baroness barriers** (Observed, island only): the layout places `Barrier_Baroness_01`
//! and `_02` (`slices/outpostrush/fort/or_baroness_barriers.dynamicslice`) at the boss's arena.
//! A retail baroness barrier is a card of that slice, the ownership on component 1, the position
//! on component 2 with the spawner's rotation (`_01` turns about all three axes) and its scale
//! (1.657341 on every axis, tag 2) and the door on component 3, `Closed`; the ownership is the
//! fort barriers' with permission rule 1 and raid 0, also in the running match and after the
//! boss's death (`OPR---ISLAND-2` 651.006 s to 729.139 s, 4 lives of the same two ids, the boss
//! dead from 695.540 s; `OPR_TEST3` 808.489 to 854.175 s). They never change; each life ends
//! with a view change. Inferred: they stand from the match's opening to its end, as the fort
//! barriers do, and keep every player out of the arena by the client's collision of a closed
//! door (no capture shows a player inside the closed arena).

use nw_formats::rigid::quat_from_rows;
use nw_messages::buildables::{GuildCrest, OwnerIdentification};
use nw_messages::position::{PositionInTheWorldReplicatedState, Rotation, Scale, WorldPosition};
use nw_messages::{BundleMember, DoorComponentState, DoorState, MemberState, OwnershipState};
use nw_sim_core::Position;

use super::SimWorld;
use super::crafting::{Station, dynamic_ref};
use super::encounter_roots::{card, place_member};

/// Names the ids this server makes for fort barriers ("oprbarri").
const BARRIER_NAMESPACE: u64 = 0x6f70_7262_6172_7269;
/// Names the ids this server makes for baroness barriers ("oprbaron").
const BARONESS_NAMESPACE: u64 = 0x6f70_7262_6172_6f6e;
/// The component of a baroness barrier's door (Observed).
const BARONESS_DOOR_COMPONENT: u64 = 3;
/// The permission rule of a baroness barrier's ownership (Observed: 1 on both).
const BARONESS_PERMISSION_RULE: u8 = 1;
/// The island's level folder: the open world's (its matches run on a frame of it).
pub(super) const ISLAND_FOLDER: &str = nw_messages::level_info::OPEN_WORLD_LEVEL.folder;
/// The placement of the island's layout slice: its open world capital (Observed).
const ISLAND_PLACEMENT: [f32; 3] = [1272.7565, 11016.635, 68.716_61];
/// The component of a barrier's ownership (Observed).
const OWNERSHIP_COMPONENT: u64 = 1;
/// The component of a barrier's position (Observed).
const POSITION_COMPONENT: u64 = 2;
/// The component of a barrier's door (Observed).
const DOOR_COMPONENT: u64 = 4;
/// The permission rule of a barrier's ownership (Observed: 4 on every barrier).
const BARRIER_PERMISSION_RULE: u8 = 4;
/// The raid of a barrier's ownership in the pregame (Observed: 0).
const PREGAME_RAID: u64 = 0;
/// The faction of a barrier's ownership (Observed: 0).
const BARRIER_FACTION: u8 = 0;
/// The permission options of a barrier's ownership (Observed: 0).
const BARRIER_PERMISSION_OPTIONS: u8 = 0;
/// The length of a guild id.
const GUILD_ID_LEN: usize = 16;
/// The cosine of the yaw at which the matrix-to-quaternion rule turns from a non-negative `w`
/// to a non-negative `z`: the trace `2 cos + 1` of a turn about the up axis is 0 there.
const TRACE_TURN_COSINE: f32 = -0.5;
/// Half of an angle.
const HALF: f32 = 0.5;
/// The team of the `FortA` barriers, and of the `FortB` barriers (Observed by their raids).
const FORT_A_TEAM: u8 = 0;
const FORT_B_TEAM: u8 = 1;

/// One barrier spawner of a game mode slice: its name, its team, its place in the slice and its
/// yaw (radians, from the spawner's basis).
#[derive(Debug, Clone, Copy, PartialEq)]
struct BarrierSpawner {
    name: &'static str,
    team: u8,
    slice_place: [f32; 3],
    yaw: f32,
}

/// The barriers of one map: the map folder, the barrier slice's asset, the slice's placement in
/// the level and the spawners.
#[derive(Debug, Clone, Copy, PartialEq)]
struct BarrierMap {
    folder: &'static str,
    asset: [u8; 16],
    placement: [f32; 3],
    spawners: [BarrierSpawner; 6],
}

/// The barriers of both maps (Observed: the spawners of the client slices; the yaw is
/// `atan2(basis[1][0], basis[0][0])` of each spawner's world basis).
const BARRIER_MAPS: [BarrierMap; 3] = [
    BarrierMap {
        folder: "nw_opr_004_trench",
        // `slices/outpostrush/fort/or_fort_barriers_trench.dynamicslice`.
        asset: [
            0xd1, 0x6d, 0x14, 0xbc, 0xd4, 0x74, 0x52, 0xd1, 0xaa, 0xee, 0x1c, 0x3b, 0x71, 0x05,
            0xbc, 0x87,
        ],
        placement: [1024.0, 1024.0, 40.98],
        spawners: [
            BarrierSpawner {
                name: "FortA_Barrier_01",
                team: FORT_A_TEAM,
                slice_place: [-94.066, -13.524, 8.498],
                yaw: -0.181_457,
            },
            BarrierSpawner {
                name: "FortA_Barrier_02",
                team: FORT_A_TEAM,
                slice_place: [-78.421, -33.610, 8.558],
                yaw: -0.954_602,
            },
            BarrierSpawner {
                name: "FortA_Barrier_03",
                team: FORT_A_TEAM,
                slice_place: [-78.623, -59.397, 8.592],
                yaw: 3.134_112,
            },
            BarrierSpawner {
                name: "FortB_Barrier_01",
                team: FORT_B_TEAM,
                slice_place: [136.545, -60.983, 8.338],
                yaw: -0.000_001,
            },
            BarrierSpawner {
                name: "FortB_Barrier_02",
                team: FORT_B_TEAM,
                slice_place: [114.261, -22.151, 8.299],
                yaw: -2.007_129,
            },
            BarrierSpawner {
                name: "FortB_Barrier_03",
                team: FORT_B_TEAM,
                slice_place: [129.379, -6.096, 9.721],
                yaw: -1.745_331,
            },
        ],
    },
    BarrierMap {
        folder: "nw_opr_005_vertical",
        // `slices/outpostrush/fort/or_fort_barriers.dynamicslice`.
        asset: [
            0x42, 0xfc, 0x3c, 0x15, 0x8e, 0xf8, 0x50, 0xae, 0xac, 0x8a, 0x39, 0x81, 0xd8, 0x5b,
            0x10, 0x39,
        ],
        placement: [1024.0, 1024.0, 31.98],
        spawners: [
            BarrierSpawner {
                name: "FortA_Barrier_01",
                team: FORT_A_TEAM,
                slice_place: [-0.047, 137.052, 120.000],
                yaw: -3.133_287,
            },
            BarrierSpawner {
                name: "FortA_Barrier_02",
                team: FORT_A_TEAM,
                slice_place: [38.321, 146.857, 121.000],
                yaw: -1.562_49,
            },
            BarrierSpawner {
                name: "FortA_Barrier_03",
                team: FORT_A_TEAM,
                slice_place: [-10.158, 72.563, 14.199],
                yaw: -3.133_287,
            },
            BarrierSpawner {
                name: "FortB_Barrier_01",
                team: FORT_B_TEAM,
                slice_place: [0.035, -136.908, 121.001],
                yaw: -0.000_002,
            },
            BarrierSpawner {
                name: "FortB_Barrier_02",
                team: FORT_B_TEAM,
                slice_place: [-36.752, -144.261, 121.207],
                yaw: 1.331_451,
            },
            BarrierSpawner {
                name: "FortB_Barrier_03",
                team: FORT_B_TEAM,
                slice_place: [35.088, -78.897, -0.003],
                yaw: 3.141_591,
            },
        ],
    },
    BarrierMap {
        folder: ISLAND_FOLDER,
        // `slices/outpostrush/fort/or_fort_barriers.dynamicslice`.
        asset: [
            0x42, 0xfc, 0x3c, 0x15, 0x8e, 0xf8, 0x50, 0xae, 0xac, 0x8a, 0x39, 0x81, 0xd8, 0x5b,
            0x10, 0x39,
        ],
        placement: ISLAND_PLACEMENT,
        spawners: [
            BarrierSpawner {
                name: "FortA_Barrier_01",
                team: FORT_A_TEAM,
                slice_place: [-31.280, -238.100, 30.160],
                yaw: 0.0,
            },
            BarrierSpawner {
                name: "FortA_Barrier_02",
                team: FORT_A_TEAM,
                slice_place: [-0.028, -285.429, 30.080],
                yaw: std::f32::consts::FRAC_PI_2,
            },
            BarrierSpawner {
                name: "FortA_Barrier_03",
                team: FORT_A_TEAM,
                slice_place: [-62.659, -286.613, 30.079],
                yaw: -std::f32::consts::FRAC_PI_2,
            },
            BarrierSpawner {
                name: "FortB_Barrier_01",
                team: FORT_B_TEAM,
                slice_place: [31.400, 232.000, -3.625],
                yaw: -0.261_800,
            },
            BarrierSpawner {
                name: "FortB_Barrier_02",
                team: FORT_B_TEAM,
                slice_place: [85.758, 282.433, -3.880],
                yaw: -1.832_597,
            },
            BarrierSpawner {
                name: "FortB_Barrier_03",
                team: FORT_B_TEAM,
                slice_place: [4.099, 293.550, -2.640],
                yaw: -1.832_597,
            },
        ],
    },
];

/// One baroness barrier spawner of the island's layout: its name, its place in the slice and
/// its world basis (rows, with the slice's scale in it).
#[derive(Debug, Clone, Copy, PartialEq)]
struct BaronessSpawner {
    name: &'static str,
    slice_place: [f32; 3],
    basis: [[f32; 3]; 3],
}

/// `slices/outpostrush/fort/or_baroness_barriers.dynamicslice`.
const BARONESS_ASSET: [u8; 16] = [
    0xa5, 0xf6, 0x92, 0x5c, 0xdf, 0x71, 0x52, 0x5b, 0xa9, 0x9b, 0xd4, 0xd4, 0xfc, 0x70, 0xcd, 0x71,
];

/// The baroness barriers of the island (Observed: the spawners of its layout slice).
const BARONESS_SPAWNERS: [BaronessSpawner; 2] = [
    BaronessSpawner {
        name: "Barrier_Baroness_01",
        slice_place: [186.199, 31.049, -4.760],
        basis: [
            [1.400_967_8, -0.882_971_9, -0.065_526_19],
            [0.885_143_8, 1.399_671_2, 0.063_912_64],
            [0.021_288_87, -0.089_024_12, 1.654_770_2],
        ],
    },
    BaronessSpawner {
        name: "Barrier_Baroness_02",
        slice_place: [146.426, 52.399, -6.0],
        basis: [
            [0.566_844_76, 1.557_391_2, 0.0],
            [-1.557_391_2, 0.566_844_76, 0.0],
            [0.0, 0.0, 1.657_341],
        ],
    },
];

/// One placed barrier of the running match: its slot key and its team.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FortBarrier {
    pub(super) number: u64,
    pub(super) team: u8,
}

/// The place of `spawner` in a level whose barrier slice stands at `placement`.
fn barrier_place(spawner: &BarrierSpawner, placement: [f32; 3]) -> [f32; 3] {
    let [x, y, z] = spawner.slice_place;
    let [east, north, up] = placement;
    [x + east, y + north, z + up]
}

/// The half yaw whose quaternion the retail server sends for a turn of `yaw` about the up axis:
/// `w` not negative while the trace is above 0, else `z` not negative.
pub(super) fn retail_half_yaw(yaw: f32) -> f32 {
    let half = yaw * HALF;
    let flip = if yaw.cos() > TRACE_TURN_COSINE {
        half.cos() < 0.0
    } else {
        half.sin() < 0.0
    };
    if flip {
        half + std::f32::consts::PI
    } else {
        half
    }
}

/// The ownership of a fort barrier whose raid is `raid`.
fn barrier_ownership(raid: u64) -> BundleMember {
    team_ownership_on(OWNERSHIP_COMPONENT, raid)
}

/// The ownership on `component` of a game mode part of the team whose raid is `raid` (0: no
/// team): the fort barriers' body (Observed: the outpost structures carry the same body, raid 0
/// while no team owns their outpost and the owner team's raid after, `OPR_TEST3` 333.555 and
/// 1087.165 s).
pub(super) fn team_ownership_on(component: u64, raid: u64) -> BundleMember {
    ownership_with(component, BARRIER_PERMISSION_RULE, raid)
}

/// The place of the layout slice of the Outpost Rush map in `folder` in its level (the fort
/// barriers' placement); `None` for another level.
pub(super) fn layout_placement(folder: &str) -> Option<[f32; 3]> {
    BARRIER_MAPS
        .iter()
        .find(|map| map.folder.eq_ignore_ascii_case(folder))
        .map(|map| map.placement)
}

/// The ownership on `component` with the permission rule `rule` and the raid `raid`.
pub(super) fn ownership_with(component: u64, rule: u8, raid: u64) -> BundleMember {
    BundleMember {
        component,
        state: MemberState::Ownership(OwnershipState {
            guild_id: Some([0; GUILD_ID_LEN]),
            guild_owner: Some(Vec::new()),
            guild_crest: Some(GuildCrest::NONE),
            owner: Some(OwnerIdentification::none()),
            permission_rule: Some(rule),
            structure_name: Some(Vec::new()),
            raid_id: Some(raid),
            faction_type: Some(BARRIER_FACTION),
            owner_pvp: Some(false),
            owner_ffa: Some(false),
            permission_options: Some(BARRIER_PERMISSION_OPTIONS),
        }),
    }
}

/// The spawn group members of `spawner` of `map` with the id `gde_ref`: card, ownership of the
/// pregame, position and the closed door.
fn barrier_members(
    map: &BarrierMap,
    spawner: &BarrierSpawner,
    gde_ref: [u8; nw_messages::RAW_ID_LEN],
) -> Vec<BundleMember> {
    vec![
        card(map.asset, gde_ref),
        barrier_ownership(PREGAME_RAID),
        place_member(
            POSITION_COMPONENT,
            barrier_place(spawner, map.placement),
            retail_half_yaw(spawner.yaw),
        ),
        closed_door(),
    ]
}

/// The closed door of a barrier on its component (Observed: every retail barrier of Outpost
/// Rush and of capture the flag).
pub(super) fn closed_door() -> BundleMember {
    closed_door_on(DOOR_COMPONENT)
}

/// A closed door on `component`.
fn closed_door_on(component: u64) -> BundleMember {
    BundleMember {
        component,
        state: MemberState::Door(DoorComponentState::of(DoorState::Closed)),
    }
}

/// The world place of a baroness barrier.
fn baroness_place(spawner: &BaronessSpawner) -> [f32; 3] {
    let [x, y, z] = spawner.slice_place;
    let [east, north, up] = ISLAND_PLACEMENT;
    [x + east, y + north, z + up]
}

/// The position member of a baroness barrier: its place, the rotation of its basis and the
/// scale of each axis (the length of each basis column).
fn baroness_position(spawner: &BaronessSpawner) -> BundleMember {
    let basis = spawner.basis;
    let scale: [f32; 3] = std::array::from_fn(|column| {
        basis
            .iter()
            .map(|row| row[column] * row[column])
            .sum::<f32>()
            .sqrt()
    });
    let rows = basis.map(|row| std::array::from_fn(|column| row[column] / scale[column]));
    let [x, y, z] = baroness_place(spawner);
    BundleMember {
        component: POSITION_COMPONENT,
        state: MemberState::Position(PositionInTheWorldReplicatedState {
            position: WorldPosition::from_metres(x, y, z),
            rotation: Some(Rotation::of_quaternion(quat_from_rows(rows))),
            scale: Some(Scale::per_axis(scale)),
        }),
    }
}

/// The spawn group members of baroness barrier `spawner` with the id `gde_ref`: card,
/// ownership (rule 1, no raid), position and the closed door.
fn baroness_members(
    spawner: &BaronessSpawner,
    gde_ref: [u8; nw_messages::RAW_ID_LEN],
) -> Vec<BundleMember> {
    vec![
        card(BARONESS_ASSET, gde_ref),
        ownership_with(OWNERSHIP_COMPONENT, BARONESS_PERMISSION_RULE, PREGAME_RAID),
        baroness_position(spawner),
        closed_door_on(BARONESS_DOOR_COMPONENT),
    ]
}

impl SimWorld {
    /// The fort barriers of the Outpost Rush map in `folder`, placed for the viewers of the
    /// instance whose ids take `salt`; none for a map without barriers.
    pub(super) fn spawn_fort_barriers(&mut self, folder: &str, salt: u64) -> Vec<FortBarrier> {
        let Some(map) = BARRIER_MAPS.iter().find(|map| map.folder == folder) else {
            return Vec::new();
        };
        let namespace = BARRIER_NAMESPACE ^ self.me.generation ^ salt;
        map.spawners
            .iter()
            .enumerate()
            .map(|(index, spawner)| {
                let gde_ref = dynamic_ref(namespace, index);
                let [x, y, z] = barrier_place(spawner, map.placement);
                let number = self.stations.place_camp(|_| Station {
                    gde_ref,
                    position: Position { x, y, z },
                    station_type: String::new(),
                    name: spawner.name.as_bytes().to_vec(),
                    members: barrier_members(map, spawner, gde_ref),
                });
                FortBarrier {
                    number,
                    team: spawner.team,
                }
            })
            .collect()
    }

    /// The pregame ended: each barrier takes the raid of its team (`raids`, by team).
    pub(super) fn open_fort_barriers(&mut self, barriers: &[FortBarrier], raids: &[u64]) {
        for barrier in barriers {
            let Some(raid) = raids.get(usize::from(barrier.team)) else {
                continue;
            };
            let ownership = barrier_ownership(*raid);
            self.stations.change_camp(
                barrier.number,
                |members| super::gates::merge_members(members, std::slice::from_ref(&ownership)),
                vec![ownership.clone()],
            );
        }
    }

    /// The match's instance closes: its barriers leave every view.
    pub(super) fn remove_fort_barriers(&mut self, barriers: &[FortBarrier]) {
        for barrier in barriers {
            self.stations.remove_camp(barrier.number);
        }
    }

    /// The baroness barriers of the Outpost Rush map in `folder` (the island only), placed for
    /// the viewers of the instance whose ids take `salt`: their slot keys.
    pub(super) fn spawn_baroness_barriers(&mut self, folder: &str, salt: u64) -> Vec<u64> {
        if folder != ISLAND_FOLDER {
            return Vec::new();
        }
        let namespace = BARONESS_NAMESPACE ^ self.me.generation ^ salt;
        BARONESS_SPAWNERS
            .iter()
            .enumerate()
            .map(|(index, spawner)| {
                let gde_ref = dynamic_ref(namespace, index);
                let [x, y, z] = baroness_place(spawner);
                self.stations.place_camp(|_| Station {
                    gde_ref,
                    position: Position { x, y, z },
                    station_type: String::new(),
                    name: spawner.name.as_bytes().to_vec(),
                    members: baroness_members(spawner, gde_ref),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use nw_messages::position::RotationComponent;

    use super::*;

    /// A barrier id for the tests.
    const GDE_REF: [u8; nw_messages::RAW_ID_LEN] = [7; nw_messages::RAW_ID_LEN];
    /// The team raids of `opr_que_and_lost` (Observed: `213 214`).
    const RETAIL_RAIDS: [u64; 2] = [0x213, 0x214];
    /// The retail ownership body of a Vertical `FortA` barrier in the running match
    /// (`opr_que_and_lost`, raid `0x213`).
    const RETAIL_RUNNING_OWNERSHIP: &str = "01ff000000000000000000000000000000000000003f8000003f8000003f8000003f80000000003f8000003f8000003f8000003f800000000002040000000000000002130f00000000";
    /// The retail ownership body of a barrier in the pregame (raid 0).
    const RETAIL_PREGAME_OWNERSHIP: &str = "01ff000000000000000000000000000000000000003f8000003f8000003f8000003f80000000003f8000003f8000003f8000003f800000000002040000000000000000000f00000000";

    fn encoded(member: &BundleMember) -> Vec<u8> {
        member.encode().expect("encode")
    }

    fn bytes(text: &str) -> Vec<u8> {
        hex::decode(text).expect("hex")
    }

    /// The retail position of a barrier: x, y, the height step and the rotation (Observed).
    struct RetailPlace {
        name: &'static str,
        folder: &'static str,
        x: f32,
        y: f32,
        z_raw: u16,
        rotation: Rotation,
    }

    fn z_only(component: RotationComponent, w_negative: bool) -> Rotation {
        Rotation {
            components: [RotationComponent::Zero, RotationComponent::Zero, component],
            w_negative,
        }
    }

    fn retail_places() -> Vec<RetailPlace> {
        vec![
            // `opr_que_and_lost` 578.667 s.
            RetailPlace {
                name: "FortA_Barrier_02",
                folder: "nw_opr_005_vertical",
                x: 1062.3206,
                y: 1170.8573,
                z_raw: 7895,
                rotation: z_only(RotationComponent::Value(9693), false),
            },
            // `opr_que_and_lost` 578.732 s.
            RetailPlace {
                name: "FortA_Barrier_03",
                folder: "nw_opr_005_vertical",
                x: 1013.8423,
                y: 1096.5632,
                z_raw: 4562,
                rotation: z_only(RotationComponent::Value(65534), true),
            },
            RetailPlace {
                name: "FortA_Barrier_01",
                folder: "nw_opr_005_vertical",
                x: 1023.95325,
                y: 1161.0525,
                z_raw: 7864,
                rotation: z_only(RotationComponent::Value(65534), true),
            },
            // The island, `OPR_TEST3` 216.676 s to 764.026 s.
            RetailPlace {
                name: "FortB_Barrier_01",
                folder: ISLAND_FOLDER,
                x: 1304.1565,
                y: 11248.635,
                z_raw: 5152,
                rotation: z_only(RotationComponent::Value(28490), false),
            },
            RetailPlace {
                name: "FortB_Barrier_02",
                folder: ISLAND_FOLDER,
                x: 1358.5143,
                y: 11299.067,
                z_raw: 5144,
                rotation: z_only(RotationComponent::Value(6771), false),
            },
            RetailPlace {
                name: "FortB_Barrier_03",
                folder: ISLAND_FOLDER,
                x: 1276.8555,
                y: 11310.185,
                z_raw: 5182,
                rotation: z_only(RotationComponent::Value(6771), false),
            },
            RetailPlace {
                name: "FortA_Barrier_02",
                folder: ISLAND_FOLDER,
                x: 1272.7285,
                y: 10731.206,
                z_raw: 6203,
                rotation: z_only(RotationComponent::Value(55937), false),
            },
        ]
    }

    /// The retail ownership body of a baroness barrier (`OPR---ISLAND-2` 651.006 s): rule 1, no
    /// raid.
    const RETAIL_BARONESS_OWNERSHIP: &str = "01ff000000000000000000000000000000000000003f8000003f8000003f8000003f80000000003f8000003f8000003f8000003f800000000002010000000000000000000f00000000";

    /// The retail positions of the baroness barriers (`OPR---ISLAND-2` 651.006 s and 655.342 s).
    fn retail_baroness_positions() -> [(&'static str, PositionInTheWorldReplicatedState); 2] {
        let scale = Some(Scale::PerAxis {
            tag: 2,
            values: [16033; 3],
        });
        [
            (
                "Barrier_Baroness_01",
                PositionInTheWorldReplicatedState {
                    position: Some(WorldPosition {
                        x: 1458.9554,
                        y: 11047.684,
                        z_raw: 5116,
                    }),
                    rotation: Some(Rotation {
                        components: [
                            RotationComponent::Value(31980),
                            RotationComponent::Value(32320),
                            RotationComponent::Value(41868),
                        ],
                        w_negative: false,
                    }),
                    scale,
                },
            ),
            (
                "Barrier_Baroness_02",
                PositionInTheWorldReplicatedState {
                    position: Some(WorldPosition {
                        x: 1419.1829,
                        y: 11069.034,
                        z_raw: 5077,
                    }),
                    rotation: Some(z_only(RotationComponent::Value(13972), false)),
                    scale,
                },
            ),
        ]
    }

    /// Whether two rotation components are the same within `steps` steps.
    fn near(ours: RotationComponent, theirs: RotationComponent, steps: u16) -> bool {
        match (ours, theirs) {
            (RotationComponent::Value(ours), RotationComponent::Value(theirs)) => {
                ours.abs_diff(theirs) <= steps
            }
            (ours, theirs) => ours == theirs,
        }
    }

    #[test]
    fn the_baroness_barriers_stand_turn_and_scale_as_retail() {
        /// The rotation steps a recomputed quaternion may miss by (the float basis of the slice).
        const ROTATION_STEPS: u16 = 8;
        for (name, retail) in retail_baroness_positions() {
            let spawner = BARONESS_SPAWNERS
                .iter()
                .find(|spawner| spawner.name == name)
                .expect("spawner");
            let MemberState::Position(ours) = baroness_position(spawner).state else {
                panic!("a position");
            };
            let (Some(at), Some(theirs)) = (ours.position, retail.position) else {
                panic!("{name} places");
            };
            assert!((at.x - theirs.x).abs() < 0.01, "{name} x");
            assert!((at.y - theirs.y).abs() < 0.01, "{name} y");
            assert!(at.z_raw.abs_diff(theirs.z_raw) <= 1, "{name} height step");
            let (Some(ours_turn), Some(theirs_turn)) = (ours.rotation, retail.rotation) else {
                panic!("{name} rotations");
            };
            for (ours, theirs) in ours_turn.components.into_iter().zip(theirs_turn.components) {
                assert!(
                    near(ours, theirs, ROTATION_STEPS),
                    "{name} {ours:?} {theirs:?}"
                );
            }
            assert_eq!(ours_turn.w_negative, theirs_turn.w_negative, "{name}");
            assert_eq!(ours.scale, retail.scale, "{name} scale");
        }
    }

    #[test]
    fn a_baroness_barrier_is_a_closed_door_on_component_three_with_rule_one_and_no_raid() {
        let members = baroness_members(&BARONESS_SPAWNERS[0], GDE_REF);
        let components: Vec<u64> = members.iter().map(|member| member.component).collect();
        assert_eq!(
            components,
            [
                0,
                OWNERSHIP_COMPONENT,
                POSITION_COMPONENT,
                BARONESS_DOOR_COMPONENT
            ]
        );
        assert!(matches!(
            &members[0].state,
            MemberState::GdeMetadata(card)
                if card.asset.is_some_and(|asset| asset.id == BARONESS_ASSET)
        ));
        assert!(
            encoded(&members[1]).ends_with(&bytes(RETAIL_BARONESS_OWNERSHIP)),
            "the retail baroness ownership"
        );
        assert!(
            encoded(&members[3]).ends_with(&[0x01, 0x01, 0x01]),
            "Closed"
        );
    }

    #[test]
    fn a_barrier_spawns_with_card_pregame_ownership_place_and_closed_door() {
        let map = &BARRIER_MAPS[1];
        let members = barrier_members(map, &map.spawners[1], GDE_REF);
        let components: Vec<u64> = members.iter().map(|member| member.component).collect();
        assert_eq!(
            components,
            [0, OWNERSHIP_COMPONENT, POSITION_COMPONENT, DOOR_COMPONENT]
        );
        assert!(matches!(
            &members[0].state,
            MemberState::GdeMetadata(card)
                if card.asset.is_some_and(|asset| asset.id == map.asset)
                    && card.gde_ref == Some(GDE_REF)
        ));
        assert!(
            encoded(&members[1]).ends_with(&bytes(RETAIL_PREGAME_OWNERSHIP)),
            "the retail pregame ownership"
        );
        assert!(
            encoded(&members[3]).ends_with(&[0x01, 0x01, 0x01]),
            "Closed"
        );
    }

    #[test]
    fn the_running_ownership_is_the_retail_body_with_the_team_raid() {
        assert!(
            encoded(&barrier_ownership(RETAIL_RAIDS[0]))
                .ends_with(&bytes(RETAIL_RUNNING_OWNERSHIP))
        );
    }

    #[test]
    fn the_barriers_stand_where_and_how_retail_placed_them() {
        for retail in retail_places() {
            let map = BARRIER_MAPS
                .iter()
                .find(|map| map.folder == retail.folder)
                .expect("map");
            let spawner = map
                .spawners
                .iter()
                .find(|spawner| spawner.name == retail.name)
                .expect("spawner");
            let member = place_member(
                POSITION_COMPONENT,
                barrier_place(spawner, map.placement),
                retail_half_yaw(spawner.yaw),
            );
            let MemberState::Position(PositionInTheWorldReplicatedState {
                position: Some(position),
                rotation: Some(rotation),
                ..
            }) = member.state
            else {
                panic!("a position");
            };
            assert!((position.x - retail.x).abs() < 0.01, "{} x", retail.name);
            assert!((position.y - retail.y).abs() < 0.01, "{} y", retail.name);
            assert!(
                position.z_raw.abs_diff(retail.z_raw) <= 1,
                "{} height step {} against {}",
                retail.name,
                position.z_raw,
                retail.z_raw
            );
            let (RotationComponent::Value(ours), RotationComponent::Value(theirs)) =
                (rotation.components[2], retail.rotation.components[2])
            else {
                panic!("{} turns about the up axis", retail.name);
            };
            assert!(
                ours.abs_diff(theirs) <= 8,
                "{} z {ours} {theirs}",
                retail.name
            );
            assert_eq!(
                rotation.w_negative, retail.rotation.w_negative,
                "{}",
                retail.name
            );
        }
    }

    #[test]
    fn each_map_has_three_barriers_per_team() {
        for map in &BARRIER_MAPS {
            for team in [FORT_A_TEAM, FORT_B_TEAM] {
                let count = map
                    .spawners
                    .iter()
                    .filter(|spawner| spawner.team == team)
                    .count();
                assert_eq!(count, 3, "{} team {team}", map.folder);
            }
        }
    }
}
