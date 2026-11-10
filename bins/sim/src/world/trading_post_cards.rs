// SPDX-License-Identifier: AGPL-3.0-or-later
//! The spawn groups of the settlements' trading posts (trading post card study, 2026-11-10).
//!
//! A settlement's trading post is not a level object: the retail server spawns it as an entity
//! of its own (slice asset [`TRADING_POST_ASSET`], an id made per world) and the client shows
//! its interaction only with that card. Without it a player found no trading post in a town and
//! the quest step `12A_MSQ_03_VaultTutorial_Trader` (open the `Contracts` screen) never ended
//! (live 2026-11-09). Observed in 932 retail cards of 13 settlements: the card, the buildable
//! controller (component 1), the place (2), the ownership (3), the trading post state (4), the
//! placement obstruction (5) and two interact states (6 and 7); retail sends them in any order.
//! On the same point retail spawns the trading post's building ([`BUILDING_ASSET`]).
//!
//! The trading post state holds the counts of the world's buy and sell orders. This server
//! keeps no such counts in the sim, so it sends 0 and 0 (Unknown: what the client shows from
//! them; the search answers carry the orders themselves).

use nw_messages::buildables::{BuildableControllerState, PlacementObstructionState};
use nw_messages::items::FIRST_VERSION;
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    AssetId, BundleMember, GdeMetadataReplicatedState, InteractState, MemberState,
    PositionInTheWorldReplicatedState, Rotation, RotationComponent, WorldPosition,
};
use nw_sim_core::Position;

use super::crafting::{
    CARD_COMPONENT, CHECK_FOUNDATION_EVENTS, Station, dynamic_ref, list_at, unowned,
};

/// The trading post slice of the asset catalog, as every retail card names it.
const TRADING_POST_ASSET: AssetId = AssetId {
    id: [
        0xd0, 0xb3, 0x6b, 0xdd, 0x2d, 0x6d, 0x55, 0x01, 0x8b, 0xba, 0x77, 0xd9, 0x7f, 0x35, 0xb3,
        0xee,
    ],
    sub_id: 2,
};
/// The trading post building, `craftingstation_t5_tradingpost_complete`: the second entity
/// retail spawns on a trading post's point, with a card and a place only (1,314 retail cards of
/// the same 13 settlements). It is what a player sees; without it the trading post did not show
/// (live 2026-11-10).
const BUILDING_ASSET: AssetId = AssetId {
    id: [
        0x8e, 0xf7, 0x68, 0x55, 0x69, 0x5d, 0x54, 0x52, 0x9c, 0x2c, 0xb3, 0xe7, 0x27, 0x28, 0x0b,
        0xbf,
    ],
    sub_id: 2,
};
/// Names the ids this server makes for trading posts.
const TRADING_POST_NAMESPACE: u64 = 0x7472_6164_696e_6770;
/// The components of the group's states.
const BUILDABLE_COMPONENT: u64 = 1;
const PLACE_COMPONENT: u64 = 2;
const OWNERSHIP_COMPONENT: u64 = 3;
const TRADING_POST_COMPONENT: u64 = 4;
const OBSTRUCTION_COMPONENT: u64 = 5;
const INTERACT_COMPONENTS: [u64; 2] = [6, 7];
/// `MB::TradingPostComponentReplicatedState`.
const TRADING_POST_CLASS: u64 = 3791;
/// Its first slot with both children set, `totalBuyContracts` and `totalSellContracts` 0.
const NO_ORDER_COUNTS: [u8; 10] = [1, 3, 0, 0, 0, 0, 0, 0, 0, 0];
/// The buildable state of a built structure (retail: 5 in every card).
const BUILT_STATE: u8 = 5;
/// Retail: `hasCompletionObstruction` 1 in 449 of 684 cards.
const COMPLETION_OBSTRUCTION: u8 = 1;
/// Retail: permission rule 1 and no structure name in all 648 ownership states.
const PERMISSION_RULE: u8 = 1;
/// Nobody uses it (retail: 0 in 698 of 963 interact states).
const NO_INTERACTORS: u32 = 0;

/// Where retail spawned a trading post, as the `PositionInTheWorld` states of the retail cards
/// carry it.
struct Place {
    x: f32,
    y: f32,
    /// The raw height.
    z_raw: u16,
    /// The raw z of its rotation (x and y are zero), and whether its w is negative.
    yaw_raw: u16,
    w_negative: bool,
    /// The place state of its building, as retail sent it (component 1): the building stands
    /// on the same point, turned its own way.
    building: &'static [u8],
}

const fn place(
    x: f32,
    y: f32,
    z_raw: u16,
    yaw_raw: u16,
    w_negative: bool,
    building: &'static [u8],
) -> Place {
    Place {
        x,
        y,
        z_raw,
        yaw_raw,
        w_negative,
        building,
    }
}

/// The trading posts of the retail captures. A settlement that no capture shows has none here.
const TRADING_POSTS: [Place; 13] = [
    // 55 retail cards.
    // 189 retail building cards.
    place(
        7261.1323,
        5413.063,
        6293,
        10630,
        false,
        &[
            0x01, 0x0d, 0x01, 0x03, 0x45, 0xe2, 0xe9, 0x0f, 0x45, 0xa9, 0x28, 0x81, 0x18, 0x95,
            0x03, 0x85, 0x94,
        ],
    ),
    // 50 retail cards.
    // 74 retail building cards.
    place(
        7349.4575,
        3743.714,
        6600,
        14444,
        false,
        &[
            0x01, 0x0d, 0x01, 0x03, 0x45, 0xe5, 0xab, 0xa9, 0x45, 0x69, 0xfb, 0x6d, 0x19, 0xc8,
            0x03, 0x98, 0x6b,
        ],
    ),
    // 50 retail cards.
    // 72 retail building cards.
    place(
        11404.537,
        5338.3604,
        4455,
        19439,
        false,
        &[
            0x01, 0x0d, 0x01, 0x03, 0x46, 0x32, 0x32, 0x26, 0x45, 0xa6, 0xd2, 0xe2, 0x11, 0x67,
            0x03, 0xad, 0xde,
        ],
    ),
    // 42 retail cards.
    // 98 retail building cards.
    place(
        4789.825,
        6894.713,
        7989,
        55943,
        false,
        &[
            0x01, 0x0d, 0x01, 0x03, 0x45, 0x95, 0xae, 0x9a, 0x45, 0xd7, 0x75, 0xb4, 0x1f, 0x35,
            0x63,
        ],
    ),
    // 38 retail cards.
    // 167 retail building cards.
    place(
        6429.8887,
        8391.308,
        8254,
        59276,
        false,
        &[
            0x01, 0x0d, 0x01, 0x03, 0x45, 0xc8, 0xef, 0x1c, 0x46, 0x03, 0x1d, 0x3b, 0x20, 0x3e,
            0x43, 0xfe, 0x6b,
        ],
    ),
    // 31 retail cards.
    // 245 retail building cards.
    place(
        9321.5205,
        2708.3425,
        5698,
        15161,
        false,
        &[
            0x01, 0x0d, 0x01, 0x03, 0x46, 0x11, 0xa6, 0x15, 0x45, 0x29, 0x45, 0x7b, 0x16, 0x43,
            0x03, 0x9b, 0xb3,
        ],
    ),
    // 29 retail cards.
    // 27 retail building cards.
    place(
        13001.102,
        4499.4536,
        4936,
        62342,
        true,
        &[
            0x01, 0x0d, 0x01, 0x03, 0x46, 0x4b, 0x24, 0x68, 0x45, 0x8c, 0x9b, 0xa1, 0x13, 0x48,
            0x03, 0x55, 0x45,
        ],
    ),
    // 17 retail cards.
    // 116 retail building cards.
    place(
        8915.96,
        4234.1826,
        7448,
        65533,
        false,
        &[
            0x01, 0x0d, 0x01, 0x03, 0x46, 0x0b, 0x4f, 0xd7, 0x45, 0x84, 0x51, 0x76, 0x1d, 0x18,
            0x03, 0x24, 0xac,
        ],
    ),
    // 16 retail cards.
    // 59 retail building cards.
    place(
        13156.138,
        6958.0254,
        7770,
        29058,
        false,
        &[
            0x01, 0x0d, 0x01, 0x03, 0x46, 0x4d, 0x90, 0x8d, 0x45, 0xd9, 0x70, 0x34, 0x1e, 0x5a,
            0x03, 0xcf, 0xad,
        ],
    ),
    // 12 retail cards.
    // 84 retail building cards.
    place(
        10874.723,
        7926.699,
        8495,
        24286,
        false,
        &[
            0x01, 0x0d, 0x01, 0x03, 0x46, 0x29, 0xea, 0xe4, 0x45, 0xf7, 0xb5, 0x98, 0x21, 0x2f,
            0x03, 0xbf, 0xff,
        ],
    ),
    // 12 retail cards.
    // 28 retail building cards.
    place(
        10974.402,
        3341.7197,
        5768,
        20227,
        false,
        &[
            0x01, 0x0d, 0x01, 0x03, 0x46, 0x2b, 0x79, 0x9c, 0x45, 0x50, 0xdb, 0x84, 0x16, 0x88,
            0x03, 0xb0, 0xfb,
        ],
    ),
    // 11 retail cards.
    // 57 retail building cards.
    place(
        7945.4243,
        1918.5197,
        6580,
        65433,
        false,
        &[
            0x01, 0x0d, 0x01, 0x03, 0x45, 0xf8, 0x4b, 0x65, 0x44, 0xef, 0xd0, 0xa1, 0x19, 0xb5,
            0x03, 0x1e, 0xaa,
        ],
    ),
    // 2 retail cards.
    // 89 retail building cards.
    place(
        9563.292,
        6339.8364,
        10985,
        28774,
        false,
        &[
            0x01, 0x0d, 0x01, 0x03, 0x46, 0x15, 0x6d, 0x2b, 0x45, 0xc6, 0x1e, 0xb1, 0x2a, 0xe9,
            0x03, 0xce, 0xcd,
        ],
    ),
];

/// The trading posts and their buildings as stations of the level's station list: each has a
/// card and a place, and comes into a view as a crafting station does. They have no station
/// type, so no recipe names them.
pub(super) fn stations() -> Vec<Station> {
    TRADING_POSTS
        .iter()
        .enumerate()
        .flat_map(|(index, post)| {
            let at = WorldPosition {
                x: post.x,
                y: post.y,
                z_raw: post.z_raw,
            };
            let position = Position {
                x: post.x,
                y: post.y,
                z: at.z(),
            };
            let station = |gde_ref, members| Station {
                gde_ref,
                position,
                station_type: String::new(),
                name: Vec::new(),
                members,
            };
            let (post_ref, building_ref) = (gde_ref(index), gde_ref(TRADING_POSTS.len() + index));
            [
                station(post_ref, members(post_ref, at, post)),
                station(building_ref, building_members(building_ref, post.building)),
            ]
        })
        .collect()
}

/// The spawn group of the trading post building `gde_ref`, whose retail place state is `place`.
fn building_members(gde_ref: [u8; RAW_ID_LEN], place: &[u8]) -> Vec<BundleMember> {
    let card = BundleMember {
        component: CARD_COMPONENT,
        state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
            asset: Some(BUILDING_ASSET),
            gde_ref: Some(gde_ref),
        }),
    };
    BundleMember::read_from(place)
        .map(|(place, _)| vec![card.clone(), place])
        .unwrap_or_else(|_| vec![card])
}

/// The id of trading post entity `index` (the trading posts, then their buildings): the same on
/// every start, in the dynamic form.
fn gde_ref(index: usize) -> [u8; RAW_ID_LEN] {
    dynamic_ref(TRADING_POST_NAMESPACE, index)
}

/// The spawn group of the trading post `gde_ref` at `at`, turned as `post`, by component.
fn members(gde_ref: [u8; RAW_ID_LEN], at: WorldPosition, post: &Place) -> Vec<BundleMember> {
    let member = |component, state| BundleMember { component, state };
    let interact = |component| {
        member(
            component,
            MemberState::Interact(InteractState {
                enabled: None,
                has_interactors: Some(NO_INTERACTORS),
            }),
        )
    };
    vec![
        member(
            CARD_COMPONENT,
            MemberState::GdeMetadata(GdeMetadataReplicatedState {
                asset: Some(TRADING_POST_ASSET),
                gde_ref: Some(gde_ref),
            }),
        ),
        member(
            BUILDABLE_COMPONENT,
            MemberState::BuildableController(BuildableControllerState {
                current_state: Some(BUILT_STATE),
                check_foundation_events: Some(CHECK_FOUNDATION_EVENTS),
                committed_resources: Some(list_at(FIRST_VERSION, Vec::new())),
                ..BuildableControllerState::default()
            }),
        ),
        member(
            PLACE_COMPONENT,
            MemberState::Position(PositionInTheWorldReplicatedState {
                position: Some(at),
                rotation: Some(Rotation {
                    components: [
                        RotationComponent::Zero,
                        RotationComponent::Zero,
                        RotationComponent::Value(post.yaw_raw),
                    ],
                    w_negative: post.w_negative,
                }),
                scale: None,
            }),
        ),
        member(
            OWNERSHIP_COMPONENT,
            MemberState::Ownership(unowned(PERMISSION_RULE, "")),
        ),
        member(
            TRADING_POST_COMPONENT,
            MemberState::Recorded {
                class: TRADING_POST_CLASS,
                body: NO_ORDER_COUNTS.to_vec(),
            },
        ),
        member(
            OBSTRUCTION_COMPONENT,
            MemberState::PlacementObstruction(PlacementObstructionState {
                has_completion_obstruction: Some(COMPLETION_OBSTRUCTION),
            }),
        ),
        interact(INTERACT_COMPONENTS[0]),
        interact(INTERACT_COMPONENTS[1]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Prydwen trading post.
    const PRYDWEN: (f32, f32) = (7349.4575, 3743.714);
    /// A trading post whose rotation has a negative w (retail `134843f386`).
    const NEGATIVE_W: (f32, f32) = (13001.102, 4499.4536);

    fn hex_of(member: &BundleMember) -> String {
        hex::encode(member.encode().expect("encode"))
    }

    fn group_at(at: (f32, f32)) -> Vec<String> {
        group_of(at, TRADING_POST_ASSET)
    }

    /// The group of the entity of slice `asset` at `at`.
    fn group_of(at: (f32, f32), asset: AssetId) -> Vec<String> {
        let card = hex::encode(asset.id);
        stations()
            .iter()
            .filter(|station| hex_of(&station.members[0]).contains(&card))
            .find(|station| (station.position.x, station.position.y) == at)
            .expect("a trading post there")
            .members
            .iter()
            .map(hex_of)
            .collect()
    }

    /// The states of the retail card of `12a_msq_09_returnregent` at 91.817 s, byte for byte;
    /// the card's id and the order counts are this server's own.
    #[test]
    fn the_prydwen_trading_post_group_is_the_retail_one() {
        let hex = group_at(PRYDWEN);
        assert!(hex[0].starts_with("000a0103d0b36bdd2d6d55018bba77d97f35b3ee00000002"));
        assert!(hex[1].starts_with("01ba1801070500"), "{}", hex[1]);
        assert_eq!(hex[2], "020d010345e5aba94569fb6d19c803386c");
        assert_eq!(
            hex[3],
            "03913201ff000000000000000000000000000000000000003f8000003f8000003f8000003f8000\
             0000003f8000003f8000003f8000003f800000000002010000000000000000000f00000000"
        );
        assert_eq!(hex[4], "048f3b01030000000000000000");
        assert_eq!(hex[5], "058b22010101");
        assert_eq!(hex[6], "06b22d010200000000");
        assert_eq!(hex[7], "07b22d010200000000");
    }

    #[test]
    fn a_rotation_with_a_negative_w_is_the_retail_one() {
        assert_eq!(
            group_at(NEGATIVE_W)[2],
            "020d0103464b2468458c9ba1134843f386"
        );
    }

    /// The building group of `12a_msq_16_fasttravel` at 113.8 s: the card, then its place.
    #[test]
    fn the_prydwen_trading_post_building_is_the_retail_one() {
        let hex = group_of(PRYDWEN, BUILDING_ASSET);
        assert_eq!(hex.len(), 2);
        assert!(hex[0].starts_with("000a01038ef76855695d54529c2cb3e727280bbf00000002"));
        assert_eq!(hex[1], "010d010345e5aba94569fb6d19c803986b");
    }

    #[test]
    fn every_building_place_reads() {
        assert!(stations().iter().all(|station| station.members.len() >= 2));
    }

    #[test]
    fn each_trading_post_has_its_own_dynamic_id() {
        let mut ids: Vec<_> = stations().iter().map(|station| station.gde_ref).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 2 * TRADING_POSTS.len());
        assert!(
            ids.iter()
                .all(|id| nw_messages::as_dynamic_raw_id(*id) == *id)
        );
    }
}
