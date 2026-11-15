// SPDX-License-Identifier: AGPL-3.0-or-later
//! The sign boards of the towns as network entities: the war boards and the town project boards
//! (town board and war board note, 2026-11-14).
//!
//! **War board** (Observed, `sign_up_for_invasion` 24.811 s, `everfall_war` 0.578 s, `aasdf`
//! 1137.842 s): a player near it gets a card of the level form (the board's id, no asset) and
//! its territory interface state at component [`TERRITORY_COMPONENT`] with its references set;
//! the use (interaction kind 61, `@ui_territory_board_war`) came 1.2 to 35 s later. A client
//! that never got that state offers no use (live 2026-11-14). The use is answered in
//! [`super::wars`].
//!
//! **Town project board** (Observed, `SQ-Monarchs-Bluffs` 36.732 s, the Monarch's Bluffs board
//! 2.3 s before its use; `Hivetownboardmissions` 577.383 s): the same card and territory
//! interface state, and between them its interact state at component [`INTERACT_COMPONENT`]
//! with no interactor. At the use the board shows the claim's progression data in its own slot
//! ([`super::governance::BoardKind::TownBoard`]).
//!
//! The boards are level entities of the stations' list ([`super::crafting::Stations`]): shown by
//! the interest radius as a station is, and found by their `GdeRef`.

use nw_messages::{
    BundleMember, GdeMetadataReplicatedState, InteractState, MemberState, TerritoryInterfaceState,
};

use super::crafting::{CARD_COMPONENT, Station, Stations, TERRITORY_REFS_SET, gde_ref_of};
use crate::player_content::SignBoard;

/// A board's territory interface component (Observed: component 3 on every war board and town
/// project board of the captures).
const TERRITORY_COMPONENT: u64 = 3;
/// A town project board's interact component (Observed: component 2).
const INTERACT_COMPONENT: u64 = 2;
/// A board no one uses (Observed: `has_interactors` 0 at its view entry).
const NO_INTERACTORS: u32 = 0;

/// Adds the war boards `war_boards` and the town project boards `town_boards` to `stations` as
/// level entities.
pub(super) fn add_to(stations: &mut Stations, war_boards: &[SignBoard], town_boards: &[SignBoard]) {
    stations.add_level(
        war_boards
            .iter()
            .map(|board| station(board, false))
            .chain(town_boards.iter().map(|board| station(board, true)))
            .collect(),
    );
}

/// The level entity of board `board`: its card, its interact state when `with_interact` (a town
/// project board) and its territory interface state.
fn station(board: &SignBoard, with_interact: bool) -> Station {
    let gde_ref = gde_ref_of(board.id);
    let card = BundleMember {
        component: CARD_COMPONENT,
        state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
            asset: None,
            gde_ref: Some(gde_ref),
        }),
    };
    let interact = with_interact.then_some(BundleMember {
        component: INTERACT_COMPONENT,
        state: MemberState::Interact(InteractState {
            enabled: None,
            has_interactors: Some(NO_INTERACTORS),
        }),
    });
    let territory = BundleMember {
        component: TERRITORY_COMPONENT,
        state: MemberState::TerritoryInterface(TerritoryInterfaceState {
            refs_set: Some(TERRITORY_REFS_SET),
            ..TerritoryInterfaceState::default()
        }),
    };
    Station {
        gde_ref,
        position: board.position,
        station_type: String::new(),
        name: Vec::new(),
        members: std::iter::once(card)
            .chain(interact)
            .chain(std::iter::once(territory))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use nw_messages::client_input::{InteractionRequest, WAR_BOARD_KIND};
    use nw_messages::{BodyCodec, ServerMessage};
    use nw_sim_core::Position;

    use super::*;

    /// The retail board of `sign_up_for_invasion`, as its card names it.
    const RETAIL_BOARD: u64 = u64::from_le_bytes([0xc7, 0xb6, 0x4c, 0x13, 0x11, 0x9d, 0x40, 0xed]);
    /// The retail group of that board (24.811 s): its card, then the territory interface state
    /// with the references set.
    const RETAIL_CARD: [u8; 16] = [
        0xc7, 0xb6, 0x4c, 0x13, 0x11, 0x9d, 0x40, 0xed, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    #[test]
    fn a_war_board_stands_with_its_card_and_its_territory_interface_state() {
        let board = SignBoard {
            id: RETAIL_BOARD,
            position: Position {
                x: 13_019.0,
                y: 4_371.3,
                z: 78.4,
            },
        };
        let station = station(&board, false);
        assert_eq!(station.gde_ref, RETAIL_CARD);
        let [card, territory] = station.members.as_slice() else {
            panic!("two members: {:?}", station.members);
        };
        assert_eq!(card.component, CARD_COMPONENT);
        assert!(matches!(
            &card.state,
            MemberState::GdeMetadata(GdeMetadataReplicatedState { asset: None, gde_ref: Some(id) })
                if *id == RETAIL_CARD
        ));
        assert_eq!(territory.component, TERRITORY_COMPONENT);
        let MemberState::TerritoryInterface(state) = &territory.state else {
            panic!("territory interface");
        };
        assert_eq!(state.refs_set, Some(true));
        assert_eq!(
            (state.progression.as_ref(), state.governance.as_ref()),
            (None, None)
        );
    }

    #[test]
    fn a_town_board_stands_with_its_card_its_idle_interact_and_its_territory_interface_state() {
        /// The Monarch's Bluffs board of `SQ-Monarchs-Bluffs` (36.732 s).
        const MONARCHS_BLUFFS_BOARD: [u8; 8] = [0x17, 0x28, 0xe7, 0xba, 0x8e, 0xf6, 0x90, 0x95];
        let board = SignBoard {
            id: u64::from_le_bytes(MONARCHS_BLUFFS_BOARD),
            position: Position {
                x: 7_328.3,
                y: 3_770.9,
                z: 123.3,
            },
        };
        let station = station(&board, true);
        assert_eq!(station.gde_ref[..8], MONARCHS_BLUFFS_BOARD);
        let components: Vec<u64> = station
            .members
            .iter()
            .map(|member| member.component)
            .collect();
        assert_eq!(
            components,
            [CARD_COMPONENT, INTERACT_COMPONENT, TERRITORY_COMPONENT],
            "the retail order"
        );
        assert!(matches!(
            &station.members[1].state,
            MemberState::Interact(InteractState {
                enabled: None,
                has_interactors: Some(NO_INTERACTORS)
            })
        ));
        assert!(matches!(
            &station.members[2].state,
            MemberState::TerritoryInterface(TerritoryInterfaceState {
                refs_set: Some(true),
                progression: None,
                governance: None,
                ..
            })
        ));
    }

    #[test]
    fn a_town_board_use_shows_the_claim_s_projects_in_the_board_s_slot() {
        use crate::world::tests::{NOW, connection, join, world_with_spawns};
        const CLAIM: u16 = 12;
        let board = SignBoard {
            id: RETAIL_BOARD,
            position: Position {
                x: 0.0,
                y: 10.0,
                z: 50.0,
            },
        };
        let mut world = world_with_spawns(Vec::new());
        world.stations.add_level(vec![station(&board, true)]);
        join(&mut world, 1, 0.0);
        let entity = world.player_by_number(1).expect("player");
        world
            .world
            .get_mut::<crate::world::Player>(entity)
            .expect("player")
            .territories = vec![CLAIM];
        let request = InteractionRequest {
            target: RETAIL_CARD,
            target_entity: nw_messages::facet_of_raw_id(&RETAIL_CARD),
            interaction: 1,
            kind: nw_messages::client_input::TOWN_BOARD_KIND,
            gatherable: None,
        };
        world.interact(
            &connection(1),
            request,
            nw_messages::client_input::TOWN_BOARD_NAME,
            NOW,
        );
        let level_number = world.stations.number_of(&RETAIL_CARD).expect("level slot");
        let mut player = world
            .world
            .get_mut::<crate::world::Player>(entity)
            .expect("player");
        assert_eq!(
            player.desk.number_in(Some(CLAIM)),
            Some(level_number),
            "the view is the board's level slot, not a second card of it"
        );
        let board_state = player
            .extra_members
            .iter()
            .find_map(|member| match &member.state {
                MemberState::ObjectiveInteractor(state) => Some(state.clone()),
                _ => None,
            })
            .expect("the objective interactor state");
        assert_eq!(
            board_state.expiration_time,
            Some(0),
            "a town board names expiration 0 as retail, not a faction board's expiry"
        );
    }

    #[test]
    fn a_war_board_use_in_a_claim_is_accepted_with_the_claim_and_no_side() {
        use crate::world::tests::{NOW, connection, join, messages_to, world_with_spawns};
        const CLAIM: u16 = 12;
        let mut world = world_with_spawns(Vec::new());
        join(&mut world, 1, 0.0);
        let entity = world.player_by_number(1).expect("player");
        world
            .world
            .get_mut::<crate::world::Player>(entity)
            .expect("player")
            .territories = vec![CLAIM];
        world.take_messages();
        let request = InteractionRequest {
            target: RETAIL_CARD,
            target_entity: nw_messages::facet_of_raw_id(&RETAIL_CARD),
            interaction: 1,
            kind: WAR_BOARD_KIND,
            gatherable: None,
        };
        world.interact(
            &connection(1),
            request,
            nw_messages::client_input::WAR_BOARD_NAME,
            NOW,
        );
        let sent = messages_to(&world.take_messages(), 1);
        assert!(sent.iter().any(|message| matches!(
            message,
            ServerMessage::InteractionValidated(answer)
                if answer.status == crate::world::gathering::GATHER_ACCEPTED
                    && answer.kind == WAR_BOARD_KIND
        )));
        let ready = sent
            .iter()
            .find_map(|message| match message {
                ServerMessage::TerritoryFacetReady(ready) => Some(*ready),
                _ => None,
            })
            .expect("the 2722");
        assert_eq!((ready.territory, ready.access), (CLAIM, 0), "retail's form");
        assert!(ready.encode().is_ok());
    }

    #[test]
    fn a_war_board_use_outside_a_claim_is_refused() {
        use crate::world::tests::{NOW, connection, join, messages_to, world_with_spawns};
        let mut world = world_with_spawns(Vec::new());
        join(&mut world, 1, 0.0);
        world.take_messages();
        let request = InteractionRequest {
            target: RETAIL_CARD,
            target_entity: nw_messages::facet_of_raw_id(&RETAIL_CARD),
            interaction: 1,
            kind: WAR_BOARD_KIND,
            gatherable: None,
        };
        world.interact(
            &connection(1),
            request,
            nw_messages::client_input::WAR_BOARD_NAME,
            NOW,
        );
        let sent = messages_to(&world.take_messages(), 1);
        assert!(sent.iter().any(|message| matches!(
            message,
            ServerMessage::InteractionValidated(answer)
                if answer.status == crate::world::gathering::GATHER_BUSY
        )));
        assert!(
            !sent
                .iter()
                .any(|message| matches!(message, ServerMessage::TerritoryFacetReady(_)))
        );
    }
}

/// The sign boards of the open world level as a live sim loads it (`local-data/content`).
#[cfg(all(test, feature = "local-data"))]
mod retail_tests {
    use crate::player_content::SignBoard;
    use crate::world::SimWorld;

    /// The `war_board` slices of the open world (build 25810808).
    const LEVEL_WAR_BOARDS: usize = 39;
    /// The `town_project_board` slices of the open world (build 25810808).
    const LEVEL_TOWN_BOARDS: usize = 13;
    /// The retail war boards of `sign_up_for_invasion`, `everfall_war` and `aasdf`, as their
    /// cards name them.
    const RETAIL_WAR_BOARDS: [[u8; 8]; 2] = [
        [0xc7, 0xb6, 0x4c, 0x13, 0x11, 0x9d, 0x40, 0xed],
        [0x8f, 0xbc, 0x68, 0x83, 0x90, 0x25, 0x09, 0xed],
    ];
    /// The retail town project boards of `SQ-Monarchs-Bluffs` (Monarch's Bluffs),
    /// `Hivetownboardmissions`, `Playthrough_Part_04` and `Playthrough_Part_06`.
    const RETAIL_TOWN_BOARDS: [[u8; 8]; 4] = [
        [0x17, 0x28, 0xe7, 0xba, 0x8e, 0xf6, 0x90, 0x95],
        [0x87, 0x25, 0x09, 0xc1, 0x0a, 0xaf, 0xf0, 0x95],
        [0xc7, 0x54, 0xa9, 0xfd, 0x18, 0x02, 0x8a, 0x95],
        [0x4f, 0x21, 0x47, 0x14, 0x15, 0xb4, 0xd0, 0x95],
    ];

    /// Each of `boards` is a level entity, and each of `retail` is among them.
    fn shown_with_retail_ids(world: &SimWorld, boards: &[SignBoard], retail: &[[u8; 8]]) {
        for board in boards {
            let gde_ref = super::gde_ref_of(board.id);
            assert!(world.stations.named(&gde_ref).is_some(), "{gde_ref:02x?}");
        }
        for id in retail {
            let id = u64::from_le_bytes(*id);
            assert!(boards.iter().any(|board| board.id == id), "{id:016x}");
        }
    }

    #[test]
    fn every_level_sign_board_is_a_level_entity_with_its_retail_card() {
        let (_level, world) = crate::quest_walk::tests::retail_world("sign-boards");
        assert_eq!(world.content.war_boards.len(), LEVEL_WAR_BOARDS);
        assert_eq!(world.content.town_boards.len(), LEVEL_TOWN_BOARDS);
        shown_with_retail_ids(&world, &world.content.war_boards, &RETAIL_WAR_BOARDS);
        shown_with_retail_ids(&world, &world.content.town_boards, &RETAIL_TOWN_BOARDS);
    }

    /// Monarch's Bluffs, its claim key.
    const MONARCHS_BLUFFS: u16 = 12;
    /// The Monarch's Bluffs town area a client names after a login inside the town (live
    /// 2026-11-14: the only area the client named, so the boards found no claim).
    const MONARCHS_BLUFFS_TOWN_AREA: u16 = 30_155;

    #[test]
    fn a_town_area_alone_names_its_claim() {
        let (_level, world) = crate::quest_walk::tests::retail_world("town-area-claim");
        assert_eq!(
            super::super::governance::claim_of(
                &[MONARCHS_BLUFFS_TOWN_AREA],
                &world.content.charting
            ),
            Some(MONARCHS_BLUFFS)
        );
    }
}
