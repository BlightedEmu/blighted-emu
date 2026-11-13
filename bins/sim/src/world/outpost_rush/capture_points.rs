// SPDX-License-Identifier: AGPL-3.0-or-later
//! The capture point entities (333) of an Outpost Rush match (game mode helper B13; findings
//! `local-data/findings/2026-11-13-outpost-rush-shed-and-points.md`).
//!
//! Observed (all 12 Outpost Rush ledgers, `slotdump <ledger> 0a651831` and `93fc6ac0`):
//! - One entity per outpost: the slice `slices/modes/opr/outpostrush_capturepoint_no_terrain_snap`
//!   on Trench and Vertical, `slices/gamemodes/outpostrush_capturepoint` on the island. Its members:
//!   card (component 0), ownership (1: permission rule 3, the raid of the team that owns the
//!   outpost, 0 with no owner; `opr_match_4` 640.869 s raid 1790 at the capture), position (2: the
//!   same place and turn in every ledger of a map) and the capture point state (3).
//! - The cold data: place 0, 0, 0, number 0, flag 1 always; kind [`CAPTURE_POINT_OPEN`] (4),
//!   [`CAPTURE_POINT_CAPTURING`] (5) or [`PROTECTED_KIND`] (12); value = the raid of the
//!   capturing team, 0 with no capturer. The hot data is the outpost status' progress, sent with
//!   each change of it (16 to 33 ms before the game mode value of the same progress,
//!   `opr_match_2` 2.907 and 2.925 s).
//! - The kind (3,700 cold updates): capturing while players of the capturing team stand in the
//!   point (2,872 of the 2,903 rises; falls with them inside while the owner leads, 340), open
//!   while none does (432 falls). The kind follows a new value one update later: a capture's
//!   start sends the raid with the old kind 4 and kind 5 33 to 37 ms later (`opr_match_4`
//!   126.931 and 126.964 s); its end sends value 0 and hot 0 with kind 5 and kind 4 about 34 ms
//!   later (44 of 44 captures).
//! - Kind 12 while the owner's Protection Ward stands built: on at the ward's built state
//!   (`opr_match_4` A 648.067 s; `opr_que_and_lost` C 1406.125 s; `OPR-Coral-Divide-2` A
//!   431.100 s), off at its ruin (`opr_match_4` 920.060 s). The capturing team shows in the value
//!   while it holds and its progress stays (`opr_match_4` A team 1 at progress 0 from 731.330 to
//!   742.798 s). Client text `OR_Protection_Ward_Description`: "Prevents outpost capture".
//!
//! Inferred: the next update is the next sim tick (retail's 33 ms is about one server frame).

use nw_messages::position::{PositionInTheWorldReplicatedState, Rotation, RotationComponent, WorldPosition};
use nw_messages::war_states::{CAPTURE_POINT_CAPTURING, CAPTURE_POINT_FLAG, CAPTURE_POINT_OPEN, CapturePointCold};
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{AssetId, BundleMember, CapturePointState, DYNAMIC_SLICE_SUB_ID, GdeMetadataReplicatedState, MemberState};
use nw_sim_core::Position;

use super::super::crafting::{Station, dynamic_ref};
use super::super::gates::merge_members;
use super::super::outpost_rush_doors::{ISLAND_FOLDER, ownership_with};
use super::{NO_TEAM, OutpostState, SimWorld, TEAMS, TRENCH_FOLDER, VERTICAL_FOLDER};

/// The kind of a point whose owner's Protection Ward stands (Observed: 12, module doc).
pub(super) const PROTECTED_KIND: u8 = 12;
/// The components of a point entity (Observed in every ledger).
const CARD_COMPONENT: u64 = 0;
const OWNERSHIP_COMPONENT: u64 = 1;
const POSITION_COMPONENT: u64 = 2;
const POINT_COMPONENT: u64 = 3;
/// The permission rule of a point's ownership (Observed: 3 on every point).
const POINT_PERMISSION_RULE: u8 = 3;
/// The raid of a point's ownership with no owner (Observed: 0).
const NO_OWNER_RAID: u64 = 0;
/// The number of every point's cold data (Observed: 0).
const POINT_NUMBER: u32 = 0;
/// The place of every point's cold data (Observed: zero).
const COLD_PLACE: [f32; 3] = [0.0; 3];
/// The namespace of the points' `GdeRef`s ("oprpoint").
const POINT_NAMESPACE: u64 = 0x6f70_7270_6f69_6e74;

/// The slice of the Trench and Vertical points (Observed:
/// `outpostrush_capturepoint_no_terrain_snap`).
const MAP_POINT_ASSET: [u8; RAW_ID_LEN] = [
    0x0a, 0x65, 0x18, 0x31, 0xbd, 0x68, 0x56, 0x11, 0x99, 0x91, 0x60, 0x24, 0x97, 0xa9, 0xef, 0xe5,
];
/// The slice of the island's points (Observed: `outpostrush_capturepoint`).
const ISLAND_POINT_ASSET: [u8; RAW_ID_LEN] = [
    0x93, 0xfc, 0x6a, 0xc0, 0xa8, 0x91, 0x5b, 0x79, 0xbf, 0xad, 0x5b, 0x06, 0x50, 0xa3, 0xe3, 0x32,
];

/// The retail place of one point: east and north in metres, the raw height, the raw z of its
/// turn and the sign of its `w`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PointPlace {
    east: f32,
    north: f32,
    z_raw: u16,
    turn_raw: u16,
    w_negative: bool,
}

/// The points of one map, outposts A, B and C (Observed: the position member of every point).
struct PointMap {
    folder: &'static str,
    asset: [u8; RAW_ID_LEN],
    places: [PointPlace; 3],
}

const fn place(east: f32, north: f32, z_raw: u16, turn_raw: u16, w_negative: bool) -> PointPlace {
    PointPlace {
        east,
        north,
        z_raw,
        turn_raw,
        w_negative,
    }
}

/// Trench (`opr_match_2`, `OPR_BAGGUNS`, both `coral-divide` ledgers), Vertical (`opr_match_4`,
/// `opr_que_and_lost`, both `Tower` ledgers) and the island (`OPR_TEST3`, `OPR---ISLAND-2`,
/// `OPR_Original_map`).
const POINT_MAPS: [PointMap; 3] = [
    PointMap {
        folder: TRENCH_FOLDER,
        asset: MAP_POINT_ASSET,
        places: [
            place(982.8319, 885.0103, 4821, 41248, false),
            place(1042.1057, 1026.1812, 4899, 32767, false),
            place(1100.4292, 885.1312, 4824, 23076, false),
        ],
    },
    PointMap {
        folder: VERTICAL_FOLDER,
        asset: MAP_POINT_ASSET,
        places: [
            place(962.3824, 1077.4198, 7462, 45307, false),
            place(1_023.753_54, 1024.0962, 7852, 32767, false),
            place(1086.7117, 971.592_16, 7462, 62975, true),
        ],
    },
    PointMap {
        folder: ISLAND_FOLDER,
        asset: ISLAND_POINT_ASSET,
        places: [
            place(1367.3013, 10873.218, 5634, 27077, false),
            place(1272.0817, 11016.969, 5166, 35623, false),
            place(1174.1521, 11158.402, 4868, 65037, false),
        ],
    },
];

/// What a point shows: its kind, value (a raid) and progress.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Shown {
    pub(super) kind: u8,
    pub(super) value: u64,
    pub(super) hot: u8,
}

/// The kind a point should show (module doc): protected while its owner's ward stands, else
/// capturing while players of its capturing team stand in it.
pub(super) const fn wanted_kind(protected: bool, capturer_inside: bool) -> u8 {
    if protected {
        PROTECTED_KIND
    } else if capturer_inside {
        CAPTURE_POINT_CAPTURING
    } else {
        CAPTURE_POINT_OPEN
    }
}

/// The next update of a point that shows `shown` and should show `wanted`: a new value goes
/// out with the kind it showed, the kind follows in the next one (module doc).
pub(super) fn next_shown(shown: Shown, wanted: Shown) -> Shown {
    if wanted.value == shown.value {
        wanted
    } else {
        Shown {
            kind: shown.kind,
            ..wanted
        }
    }
}

/// One point entity of the match.
#[derive(Debug, Clone, PartialEq)]
struct PointEntity {
    /// Its slot key.
    number: u64,
    shown: Shown,
    owner_raid: u64,
}

/// The capture point entities of the match and who stood in them at the last capture tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct CapturePoints {
    points: Vec<PointEntity>,
    /// The living participants inside each outpost's point at the last capture tick, by team.
    pub(super) present: [[usize; TEAMS]; 3],
    /// The participants inside an outpost's point at the last capture tick (the war board's
    /// contests, [`super::board`]).
    pub(super) inside: Vec<u64>,
}

impl CapturePoints {
    /// The slot keys of the point entities.
    pub(super) fn numbers(&self) -> impl Iterator<Item = u64> + '_ {
        self.points.iter().map(|point| point.number)
    }

    /// Notes who stands in the points at a capture tick: each living participant (number,
    /// team) inside the point of outpost `index`.
    pub(super) fn note_presence(&mut self, inside: &[(u64, u8, usize)]) {
        self.present = [[0; TEAMS]; 3];
        self.inside.clear();
        for &(number, team, index) in inside {
            if let Some(count) = self
                .present
                .get_mut(index)
                .and_then(|present| present.get_mut(usize::from(team.min(1))))
            {
                *count += 1;
            }
            self.inside.push(number);
        }
    }
}

/// The point member that shows `shown`, with its hot data when `with_hot`.
fn point_member(shown: Shown, with_hot: bool) -> BundleMember {
    BundleMember {
        component: POINT_COMPONENT,
        state: MemberState::CapturePoint(CapturePointState {
            cold: Some(CapturePointCold {
                place: COLD_PLACE,
                number: POINT_NUMBER,
                kind: shown.kind,
                value: shown.value,
                flag: CAPTURE_POINT_FLAG,
            }),
            hot: with_hot.then_some(shown.hot),
        }),
    }
}

impl SimWorld {
    /// The point entities of the Outpost Rush map in `folder` for the match whose ids take
    /// `salt`, open and unowned (module doc); none for another map.
    pub(super) fn spawn_capture_points(&mut self, folder: &str, salt: u64) -> CapturePoints {
        let Some(map) = POINT_MAPS.iter().find(|map| map.folder.eq_ignore_ascii_case(folder)) else {
            return CapturePoints::default();
        };
        let namespace = POINT_NAMESPACE ^ self.me.generation ^ salt;
        let shown = Shown {
            kind: CAPTURE_POINT_OPEN,
            ..Shown::default()
        };
        let points = map
            .places
            .iter()
            .enumerate()
            .map(|(index, at)| {
                let gde_ref = dynamic_ref(namespace, index);
                let position = WorldPosition {
                    x: at.east,
                    y: at.north,
                    z_raw: at.z_raw,
                };
                let members = vec![
                    BundleMember {
                        component: CARD_COMPONENT,
                        state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                            asset: Some(AssetId {
                                id: map.asset,
                                sub_id: DYNAMIC_SLICE_SUB_ID,
                            }),
                            gde_ref: Some(gde_ref),
                        }),
                    },
                    ownership_with(OWNERSHIP_COMPONENT, POINT_PERMISSION_RULE, NO_OWNER_RAID),
                    BundleMember {
                        component: POSITION_COMPONENT,
                        state: MemberState::Position(PositionInTheWorldReplicatedState {
                            position: Some(position),
                            rotation: Some(Rotation {
                                components: [
                                    RotationComponent::Zero,
                                    RotationComponent::Zero,
                                    RotationComponent::Value(at.turn_raw),
                                ],
                                w_negative: at.w_negative,
                            }),
                            ..PositionInTheWorldReplicatedState::default()
                        }),
                    },
                    point_member(shown, true),
                ];
                let number = self.stations.place_camp(|_| Station {
                    gde_ref,
                    position: Position {
                        x: at.east,
                        y: at.north,
                        z: position.z(),
                    },
                    station_type: String::new(),
                    name: Vec::new(),
                    members,
                });
                PointEntity {
                    number,
                    shown,
                    owner_raid: NO_OWNER_RAID,
                }
            })
            .collect();
        CapturePoints {
            points,
            ..CapturePoints::default()
        }
    }

    /// The point entities leave every view.
    pub(super) fn remove_capture_points(&mut self, points: &CapturePoints) {
        for number in points.numbers() {
            self.stations.remove_camp(number);
        }
    }

    /// Sends the next update of each point of the running match (module doc), with the
    /// outposts whose owner's ward stands (`protected`); each sim tick.
    pub(super) fn show_capture_points(&mut self, protected: [bool; 3]) {
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        let mut points = std::mem::take(&mut running.capture_points);
        let (states, raids) = (running.points, running.raids);
        self.sync_capture_points(&mut points, &states, &raids, protected);
        if let Some(running) = self.arena_matches.outpost_rush.as_mut() {
            running.capture_points = points;
        }
    }

    /// Sends each point's next update (module doc) for the outposts `states`, the team raids
    /// `raids` and the outposts whose owner's ward stands (`protected`).
    fn sync_capture_points(
        &mut self,
        points: &mut CapturePoints,
        states: &[OutpostState; 3],
        raids: &[u64; TEAMS],
        protected: [bool; 3],
    ) {
        let raid_of = |team: u8| {
            (team != NO_TEAM)
                .then(|| raids.get(usize::from(team)).copied())
                .flatten()
                .unwrap_or(NO_OWNER_RAID)
        };
        for (index, point) in points.points.iter_mut().enumerate() {
            let (Some(state), Some(present)) = (states.get(index), points.present.get(index)) else {
                continue;
            };
            let capturer_inside = state.capturer != NO_TEAM
                && present.get(usize::from(state.capturer)).is_some_and(|count| *count > 0);
            let wanted = Shown {
                kind: wanted_kind(protected[index], capturer_inside),
                value: raid_of(state.capturer),
                hot: state.shown_progress(),
            };
            let next = next_shown(point.shown, wanted);
            let mut changed = Vec::new();
            if next != point.shown {
                changed.push(point_member(next, next.hot != point.shown.hot));
            }
            let owner_raid = raid_of(state.owner);
            if owner_raid != point.owner_raid {
                changed.push(ownership_with(OWNERSHIP_COMPONENT, POINT_PERMISSION_RULE, owner_raid));
            }
            if changed.is_empty() {
                continue;
            }
            let held: Vec<BundleMember> = changed
                .iter()
                .map(|member| match &member.state {
                    MemberState::CapturePoint(_) => point_member(next, true),
                    _ => member.clone(),
                })
                .collect();
            self.stations
                .change_camp(point.number, |members| merge_members(members, &held), changed);
            point.shown = next;
            point.owner_raid = owner_raid;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAID: u64 = 1790;

    #[test]
    fn the_kind_is_protected_then_capturing_then_open() {
        assert_eq!(wanted_kind(true, true), PROTECTED_KIND);
        assert_eq!(wanted_kind(false, true), CAPTURE_POINT_CAPTURING);
        assert_eq!(wanted_kind(false, false), CAPTURE_POINT_OPEN);
    }

    #[test]
    fn a_new_value_goes_out_with_the_old_kind_and_the_kind_follows() {
        let open = Shown {
            kind: CAPTURE_POINT_OPEN,
            value: 0,
            hot: 0,
        };
        let capturing = Shown {
            kind: CAPTURE_POINT_CAPTURING,
            value: RAID,
            hot: 0,
        };
        let first = next_shown(open, capturing);
        assert_eq!(first, Shown { kind: CAPTURE_POINT_OPEN, ..capturing }, "retail 126.931 s");
        assert_eq!(next_shown(first, capturing), capturing, "retail 126.964 s");
        let captured = Shown {
            kind: CAPTURE_POINT_OPEN,
            value: 0,
            hot: 0,
        };
        let end = next_shown(Shown { hot: 99, ..capturing }, captured);
        assert_eq!(end, Shown { kind: CAPTURE_POINT_CAPTURING, value: 0, hot: 0 }, "kind 5 value 0");
        assert_eq!(next_shown(end, captured), captured, "kind 4 one update later");
    }

    #[test]
    fn every_map_has_three_points_of_its_slice() {
        for map in &POINT_MAPS {
            assert_eq!(map.places.len(), 3);
        }
        assert_eq!(POINT_MAPS[2].asset, ISLAND_POINT_ASSET);
        assert_eq!(POINT_MAPS[0].asset, MAP_POINT_ASSET);
    }

    mod in_a_match {
        use super::super::super::tests::{
            FIRST, SECOND, START_MS, capture_ticks, outpost_rush, running, rush_sim_with, shed_and_ward_table,
            stand_in_outpost, tick_at,
        };
        use super::super::super::{CAPTURE_TICK_MS, PREGAME_MS};
        use super::*;

        /// The outposts A and C.
        const A: usize = 0;
        const C: usize = 2;

        fn started() -> (SimWorld, u64) {
            let mode = outpost_rush();
            let table = shed_and_ward_table(&mode.map.folder);
            let mut world = rush_sim_with(mode, table);
            let start_ms = START_MS + PREGAME_MS;
            tick_at(&mut world, start_ms);
            (world, start_ms)
        }

        fn shown(world: &SimWorld, index: usize) -> (Shown, u64) {
            let point = &running(world).capture_points.points[index];
            (point.shown, point.owner_raid)
        }

        #[test]
        fn a_capture_sends_the_raid_then_capturing_then_open_with_the_owner_raid() {
            let (mut world, start_ms) = started();
            let raid = running(&world).raids[0];
            assert_eq!(shown(&world, A), (Shown { kind: CAPTURE_POINT_OPEN, value: 0, hot: 0 }, 0));
            stand_in_outpost(&mut world, FIRST, A);
            tick_at(&mut world, start_ms + CAPTURE_TICK_MS);
            assert_eq!(shown(&world, A).0, Shown { kind: CAPTURE_POINT_OPEN, value: raid, hot: 1 });
            tick_at(&mut world, start_ms + CAPTURE_TICK_MS + 1);
            assert_eq!(shown(&world, A).0.kind, CAPTURE_POINT_CAPTURING, "the kind follows");
            let captured_ms = start_ms + capture_ticks(1) * CAPTURE_TICK_MS;
            tick_at(&mut world, captured_ms);
            assert_eq!(
                shown(&world, A),
                (Shown { kind: CAPTURE_POINT_CAPTURING, value: 0, hot: 0 }, raid),
                "retail: kind 5 value 0 hot 0 and the owner's raid"
            );
            tick_at(&mut world, captured_ms + 1);
            assert_eq!(shown(&world, A).0.kind, CAPTURE_POINT_OPEN);
            assert_eq!(running(&world).capture_points.inside, vec![FIRST], "the board's contests");
        }

        #[test]
        fn a_standing_ward_shows_protected_and_holds_the_capture() {
            let (mut world, start_ms) = started();
            let raid = running(&world).raids[1];
            stand_in_outpost(&mut world, SECOND, C);
            for tick in 1..=3 {
                tick_at(&mut world, start_ms + tick * CAPTURE_TICK_MS);
                tick_at(&mut world, start_ms + tick * CAPTURE_TICK_MS + 1);
            }
            assert_eq!(shown(&world, C).0, Shown { kind: PROTECTED_KIND, value: raid, hot: 0 });
            assert_eq!(running(&world).points[C].progress, 0, "retail: progress stays");
        }
    }
}
