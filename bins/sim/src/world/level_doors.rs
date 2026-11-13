// SPDX-License-Identifier: AGPL-3.0-or-later
//! The doors of the level's capitals as network entities ([`nw_content::level_doors`], level
//! doors finding 2026-11-13; [`nw_messages::door`]).
//!
//! **The entity** (Observed, 148 retail ledgers, 328 door lives of 21 capitals): a player near a
//! door capital gets a card of the level form (no asset; the capital's id, then a phase id) and
//! the states of the capital slice's replicated components that differ from the client's start;
//! nothing of it changes while the player sees it, and the slot stops when the player goes away.
//! The retail phase id is the one of the phasing entry of the door's sector (1635: an instance
//! entry `3:0:9491a96ab8bb2c8b` in `99a_msq_15_ritual` at 122.513 s gave the Amrine barrier the
//! tail `9491a96ab8bb2c8b` from 122.564 s; zero on the Dunwood gates of unphased sectors). This
//! server runs one open-world phase whose level entities take the zero tail
//! ([`super::crafting::gde_ref_of`]), so the doors do too (Inferred).
//!
//! **The states** (Observed per component kind; the finding's table):
//! - the door: `Closed` (`01 01 01`) in all 5,012 door lives of the captures;
//! - a slayer script: `01 04 00 01 02 00` on the 7 Dunwood gate capitals (two slices);
//! - a timeline: stopped at 0 at version 3, and a delayed event: no delay, completed, both on
//!   `Gimmick_Door_Varlaam_A` (20 lives);
//! - a placement obstruction: no completion obstruction, on `CustomMQ_0517_AncientTowerDoor_01`;
//! - a gatherable: `Depleted` on the two swapped door slices (`CustomMQ_0517_AncientTowerDoor_02_Swapped`,
//!   `Master_Door_Swapped_3m`, 116 lives) and not sent on the other door slices with one
//!   (`Gimmick_Door_Varlaam_A`, `CustomMQ_0517_AncientTowerDoor_01`).
//!
//! Inferred: a door slice no capture shows gets the same state per kind, and a swapped slice
//! (its name holds `swapped`) the depleted gatherable.
//!
//! **The opening**: the players pass these doors with interaction 67
//! (`TeleportToOtherSideOfDoor`), which [`super::travel`] answers; the door state never changes
//! (Observed: no change in any capture).
//!
//! The doors are level entities of the stations' list ([`super::crafting::Stations`]): shown by
//! the interest radius as a station is, and found by their `GdeRef`.

use nw_content::level_doors::{DoorComponentKind, LevelDoor, LevelDoors};
use nw_messages::timeline_state::TimelineState;
use nw_messages::{
    BundleMember, DEPLETED_GATHERABLE, DelayedEventState, DoorComponentState, DoorState,
    GatherableControllerState, GdeMetadataReplicatedState, MemberState, PlacementObstructionState,
};
use nw_sim_core::Position;

use super::crafting::{CARD_COMPONENT, Station, Stations, gde_ref_of};
use super::encounter_roots::recorded;
use super::gates::timeline_member;

/// Type index of `MB::SlayerScriptReplicatedState`.
const SLAYER_SCRIPT_TYPE_INDEX: u64 = 3362;
/// The slayer script body of a door (Observed: the 7 Dunwood gate capitals, every life).
const RETAIL_SLAYER_BODY: [u8; 6] = [0x01, 0x04, 0x00, 0x01, 0x02, 0x00];
/// The version of a door's unplayed timelines (Observed: `Gimmick_Door_Varlaam_A`, both).
const RETAIL_TIMELINE_VERSION: u64 = 3;
/// A door's delayed event: no delay, completed (Observed: `Gimmick_Door_Varlaam_A`, both).
const RETAIL_DELAYED_EVENT: DelayedEventState = DelayedEventState {
    total_delay_s: Some(0.0),
    completed: Some(true),
};
/// A door's placement obstruction (Observed: `CustomMQ_0517_AncientTowerDoor_01`).
const RETAIL_OBSTRUCTION: PlacementObstructionState = PlacementObstructionState {
    has_completion_obstruction: Some(0),
};
/// The part of a swapped door slice's path (lower case) whose gatherable goes out `Depleted`.
const SWAPPED_SLICE_PART: &str = "swapped";

/// The spawn member of `door`'s component `component` of `kind`; `None` for a gatherable that
/// keeps the client's start, or a body that does not encode.
fn member(door: &LevelDoor, component: u64, kind: DoorComponentKind) -> Option<BundleMember> {
    let state = match kind {
        DoorComponentKind::Door => MemberState::Door(DoorComponentState::of(DoorState::Closed)),
        DoorComponentKind::SlayerScript => {
            return Some(recorded(
                component,
                SLAYER_SCRIPT_TYPE_INDEX,
                RETAIL_SLAYER_BODY.to_vec(),
            ));
        }
        DoorComponentKind::Timeline => {
            return timeline_member(component, &TimelineState::unplayed(RETAIL_TIMELINE_VERSION));
        }
        DoorComponentKind::DelayedEvent => MemberState::DelayedEvent(RETAIL_DELAYED_EVENT),
        DoorComponentKind::PlacementObstruction => {
            MemberState::PlacementObstruction(RETAIL_OBSTRUCTION)
        }
        DoorComponentKind::Gatherable => {
            if !door.slice.contains(SWAPPED_SLICE_PART) {
                return None;
            }
            MemberState::GatherableController(GatherableControllerState {
                gatherable_state: Some(DEPLETED_GATHERABLE),
            })
        }
    };
    Some(BundleMember { component, state })
}

/// The level entity of `door`: its card and its components' spawn states.
fn station(door: &LevelDoor) -> Station {
    let gde_ref = gde_ref_of(door.capital);
    let [x, y, z] = door.position;
    let card = BundleMember {
        component: CARD_COMPONENT,
        state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
            asset: None,
            gde_ref: Some(gde_ref),
        }),
    };
    Station {
        gde_ref,
        position: Position { x, y, z },
        station_type: String::new(),
        name: Vec::new(),
        members: std::iter::once(card)
            .chain(
                door.components
                    .iter()
                    .filter_map(|component| member(door, component.component, component.kind)),
            )
            .collect(),
    }
}

/// Adds the door capitals of `doors` to `stations`; a capital that is already one of its level
/// entities (a station or prop of the same `GdeRef`) keeps that one.
pub(super) fn add_to(stations: &mut Stations, doors: &LevelDoors) {
    for door in &doors.doors {
        if stations.number_of(&gde_ref_of(door.capital)).is_none() {
            stations.add_level_entity(station(door));
        }
    }
}

#[cfg(test)]
mod tests {
    use nw_content::level_doors::DoorComponent;
    use nw_messages::timeline_state::TIMELINE_TYPE_INDEX;

    use super::*;

    fn hex(member: &BundleMember) -> String {
        hex::encode(member.encode().expect("member"))
    }

    fn door(slice: &str, components: &[(u64, DoorComponentKind)]) -> LevelDoor {
        LevelDoor {
            // Observed: the 17DW gate capital whose card starts `abb119ca31770b18`.
            capital: 0x180b_7731_ca19_b1ab,
            position: [3402.029, 5808.956, 240.55014],
            slice: slice.to_owned(),
            components: components
                .iter()
                .map(|(component, kind)| DoorComponent {
                    component: *component,
                    kind: *kind,
                })
                .collect(),
        }
    }

    #[test]
    fn a_dunwood_gate_spawns_with_the_retail_card_slayer_script_and_closed_door() {
        let gate = door(
            "coatgen/f46c8fa3/master_simpledoor_teleport_a_17dw_msq_gate_3362621797.dynamicslice",
            &[
                (2, DoorComponentKind::SlayerScript),
                (5, DoorComponentKind::Door),
            ],
        );
        let station = station(&gate);
        assert_eq!(
            station.gde_ref,
            [
                0xab, 0xb1, 0x19, 0xca, 0x31, 0x77, 0x0b, 0x18, 0, 0, 0, 0, 0, 0, 0, 0
            ]
        );
        assert_eq!(station.members.len(), 3);
        assert_eq!(
            station.members[1],
            recorded(
                2,
                SLAYER_SCRIPT_TYPE_INDEX,
                hex::decode("010400010200").expect("hex")
            )
        );
        // Observed: the retail member `059a24010101`.
        assert_eq!(hex(&station.members[2]), "059a24010101");
    }

    #[test]
    fn the_varlaam_door_spawns_its_timelines_and_delayed_events_as_retail() {
        let varlaam = door(
            "slices/worldevents/universal/gimmicks/doors/gimmick_door_varlaam_a.dynamicslice",
            &[
                (2, DoorComponentKind::Gatherable),
                (4, DoorComponentKind::Timeline),
                (5, DoorComponentKind::DelayedEvent),
                (6, DoorComponentKind::DelayedEvent),
                (7, DoorComponentKind::Timeline),
                (11, DoorComponentKind::Door),
            ],
        );
        let members = station(&varlaam).members;
        // Card, two timelines, two delayed events, the door; no gatherable (Observed).
        assert_eq!(members.len(), 6);
        let timeline = MemberState::Recorded {
            class: TIMELINE_TYPE_INDEX,
            body: hex::decode("010300010301000001030100000000").expect("hex"),
        };
        assert_eq!(members[1].state, timeline);
        assert_eq!(members[1].component, 4);
        assert_eq!(hex(&members[2]), "0583310103000001");
        assert_eq!(hex(&members[3]), "0683310103000001");
        assert_eq!(members[4].state, timeline);
        assert_eq!(hex(&members[5]), "0b9a24010101");
    }

    #[test]
    fn a_swapped_barrier_spawns_depleted_and_a_tower_door_its_obstruction() {
        let barrier = door(
            "slices/worldevents/mainstoryquest/amrineazothbarrier/custommq_0517_ancienttowerdoor_02_swapped.dynamicslice",
            &[
                (3, DoorComponentKind::Gatherable),
                (7, DoorComponentKind::Door),
            ],
        );
        let members = station(&barrier).members;
        assert_eq!(hex(&members[1]), "030c010108");
        assert_eq!(hex(&members[2]), "079a24010101");
        let tower = door(
            "slices/worldevents/reekwater/interact/ancientstatuewdoor/custommq_0517_ancienttowerdoor_01.dynamicslice",
            &[
                (3, DoorComponentKind::Gatherable),
                (7, DoorComponentKind::PlacementObstruction),
                (8, DoorComponentKind::Door),
            ],
        );
        let members = station(&tower).members;
        assert_eq!(members.len(), 3);
        assert_eq!(hex(&members[1]), "078b22010100");
        assert_eq!(hex(&members[2]), "089a24010101");
    }

    #[test]
    fn a_door_capital_that_is_already_a_level_entity_is_not_added_twice() {
        let doors = LevelDoors {
            source: String::new(),
            level: String::new(),
            doors: vec![door(
                "slices/door.dynamicslice",
                &[(3, DoorComponentKind::Door)],
            )],
        };
        let mut stations = Stations::default();
        add_to(&mut stations, &doors);
        let number = stations
            .number_of(&gde_ref_of(doors.doors[0].capital))
            .expect("added");
        add_to(&mut stations, &doors);
        assert_eq!(
            stations.number_of(&gde_ref_of(doors.doors[0].capital)),
            Some(number)
        );
        assert_eq!(stations.members(number).map(<[_]>::len), Some(2));
        assert!(stations.members(number + 1).is_none());
    }
}

#[cfg(all(test, feature = "local-data"))]
mod retail_tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use nw_content::client_build::load_client_build;

    use super::*;

    const CLIENT_BUILD_CONFIG: &str = "config/client-build.toml";
    const OPEN_WORLD: &str = "newworld_vitaeeterna";

    /// The first 8 `GdeRef` bytes of the 21 door capitals of the retail captures (big-endian
    /// hex of the little-endian id) and each one's spawn members after the card
    /// (component:member hex; Observed, level doors finding 2026-11-13).
    const RETAIL_DOORS: [(&str, &[&str]); 21] = [
        ("03e02ba9fbd73466", &["049a24010101"]),
        ("0bf4825c193d882d", &["030c010108", "079a24010101"]),
        ("13e2f2a7c0b8cdaa", &["slayer:2", "059a24010101"]),
        ("23617ba8025a804f", &["039a24010101"]),
        ("2b29af4655c9a731", &["slayer:2", "059a24010101"]),
        ("63ed116ead2b8a30", &["049a24010101"]),
        ("6b2e5fdc5d0690ff", &["078b22010100", "089a24010101"]),
        ("7bf2a93e60ceed0d", &["039a24010101"]),
        ("83d43ee38d9564b9", &["slayer:2", "059a24010101"]),
        ("9be42fbe33c130b4", &["slayer:2", "059a24010101"]),
        ("abb119ca31770b18", &["slayer:2", "059a24010101"]),
        ("b3fb15a48f3f904e", &["slayer:2", "059a24010101"]),
        ("d308d6aa9602a8e2", &["049a24010101"]),
        ("d3a87f29c4964a72", &["030c010108", "079a24010101"]),
        ("d3abfa70860c8514", &["049a24010101", "059a24010101"]),
        ("db0d9c32381e3154", &["slayer:2", "059a24010101"]),
        ("e33c2dbe51c6e687", &["049a24010101"]),
        ("f386670f603a9ffb", &["049a24010101"]),
        (
            "f3dad636347e893a",
            &[
                "timeline:4",
                "0583310103000001",
                "0683310103000001",
                "timeline:7",
                "0b9a24010101",
            ],
        ),
        ("f3dd27ff29c76c58", &["049a24010101"]),
        ("fbe27be23e0a0e51", &["049a24010101"]),
    ];

    /// `member` as the table writes it: a recorded slayer script or timeline by its component,
    /// any other state by its encoded bytes.
    fn described(member: &BundleMember) -> String {
        match &member.state {
            MemberState::Recorded { class, body } if *class == SLAYER_SCRIPT_TYPE_INDEX => {
                assert_eq!(body.as_slice(), RETAIL_SLAYER_BODY);
                format!("slayer:{}", member.component)
            }
            MemberState::Recorded { body, .. } => {
                assert_eq!(hex::encode(body), "010300010301000001030100000000");
                format!("timeline:{}", member.component)
            }
            _ => hex::encode(member.encode().expect("member")),
        }
    }

    #[test]
    fn every_retail_door_capital_spawns_with_its_retail_members() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let build = load_client_build(&repo.join(CLIENT_BUILD_CONFIG)).expect("client build");
        let dir = std::env::var("NW_CONTENT_DIR")
            .map_or_else(|_| repo.join("local-data/content"), Into::into);
        let doors = LevelDoors::load(&dir, &build, OPEN_WORLD).expect("level doors file");
        let mut stations = Stations::default();
        add_to(&mut stations, &doors);
        let by_prefix: BTreeMap<String, Vec<String>> = doors
            .doors
            .iter()
            .filter_map(|door| {
                let gde_ref = gde_ref_of(door.capital);
                let members = stations.members(stations.number_of(&gde_ref)?)?;
                Some((
                    hex::encode(&gde_ref[..8]),
                    members.iter().skip(1).map(described).collect(),
                ))
            })
            .collect();
        for (prefix, retail) in RETAIL_DOORS {
            assert_eq!(
                by_prefix.get(prefix).map(Vec::as_slice),
                Some(
                    retail
                        .iter()
                        .map(|member| (*member).to_owned())
                        .collect::<Vec<_>>()
                        .as_slice()
                ),
                "door capital {prefix}"
            );
        }
        assert!(doors.len() >= RETAIL_DOORS.len());
    }
}
