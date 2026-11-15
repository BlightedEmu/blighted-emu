// SPDX-License-Identifier: AGPL-3.0-or-later
//! The governor's desk of a town (territory upkeep study 2026-11-11): its governance data and the
//! upkeep payment; the territory planning board, the same kind of level entity whose 1746 also
//! carries the progression data ([`super::town_projects`]); and the town project board, whose
//! 1746 carries the progression data alone ([`BoardKind::TownBoard`]).
//!
//! Observed (ledgers `pay-territory-tax`, `territory-status`, `MSQ6-lv28`): the player uses the
//! desk (interaction kind [`GOVERNOR_DESK_KIND`], `@ui_governorsdesk`); retail accepts it (3743),
//! sends 2722 with the desk's territory ([`nw_messages::TerritoryFacetReadyMsg`]), and then the
//! desk entity's 1746 with the governance data (`pay-territory-tax` 1783335279055, 279058,
//! 279064). The desk is a level entity: retail's group holds a card with its `GdeRef` only (the
//! interaction's target), the interact state at component 2 and the 1746 at
//! [`DESK_TERRITORY_COMPONENT`]. At the desk's spawn retail sent the 1746 slot 0 alone and the
//! governance on the use; this server shows the desk entity from the use on (it knows the desk
//! from the use only), so its first group carries both. A level entity's interact state is not
//! sent (as for the crafting stations).
//!
//! The payment (2738, [`nw_messages::PayUpkeepCostMsg`]): this sim checks that the player has the
//! claim's desk open (Inferred: the payment is the desk's), that a company owns the claim, that
//! the upkeep is payable ([`super::land_claims::ClaimUpkeep::payable_ms`]) and that the cost is
//! the game data's ([`nw_content::territory_upkeep::UpkeepCosts`]); the social service takes it
//! from the payer's company treasury and the database moves the due time
//! ([`nw_bus::SocialInput::PayUpkeep`]). When the service says it is paid, this sim reads the
//! owners at once, and the open desks of the claim get the new governance data alone in a group
//! (Observed: the retail 1746 after the payment, 1783335287246, the times 7 days later).
//!
//! A refused payment (by this sim or by the service, [`nw_bus::UpkeepAnswer::paid`] false) shows
//! the payer's open desk its governance data again, alone in a group, with the due time that did
//! not move. Observed: retail has no upkeep result message (the client facet
//! `TerritoryInteractorComponentClientFacet` has 7 client messages, none for the upkeep; the facet
//! function `FUN_144817ba0` that takes 2738 only logs a refused request), and the governance data
//! is retail's one answer to a payment. Inferred: the same answer, unchanged, ends the payer's
//! wait for a refused one.

use bevy_ecs::entity::Entity;
use nw_bus::{ConnectionRef, SocialInput, UpkeepAnswer, UpkeepPayment};
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    ACCESS_OF_A_MEMBER, ACCESS_OF_NO_MEMBER, BundleGroup, BundleMember, CLAIM_KEYS,
    GdeMetadataReplicatedState, InteractionRequest, MemberState, PayUpkeepCostMsg, ServerMessage,
    TerritoryFacetReadyMsg, TerritoryInterfaceState,
};

use super::crafting::{CARD_COMPONENT, TERRITORY_REFS_SET};
use super::gathering::{GATHER_ACCEPTED, GATHER_BUSY};
use super::land_claims::{CLAIM_LEVEL, Territories};
use super::slots::Slot;
use super::{Player, SimWorld, gather_answer, send_now};
use crate::charting::ChartRules;
use nw_content::territory_projects::TerritoryProjects;

/// The interaction kind of a governor's desk (Observed: 3743 kind 54 `@ui_governorsdesk` in the
/// three desk ledgers).
pub(super) const GOVERNOR_DESK_KIND: u8 = 54;
/// First slot key of a player's open desk: the count of desks it opened is added. Keys are never
/// sent; they stay above every other entity range.
pub(super) const DESK_ENTITY_BASE: u64 = 1 << 45;
/// The desk's territory interface component (Observed: component 3 of the three retail desks).
const DESK_TERRITORY_COMPONENT: u64 = 3;
/// Metric: upkeep payments this sim refused, by `reason`.
const METRIC_REFUSED_PAYMENTS: &str = "sim_upkeep_payments_refused_total";
const REASON_LABEL: &str = "reason";

/// Why this sim refused an upkeep payment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refusal {
    /// The player has not that claim's desk open.
    NoDesk,
    /// No company owns the claim.
    NoOwner,
    /// The upkeep is not payable yet.
    NotPayable,
    /// The cost is not the game data's.
    WrongCost,
}

impl Refusal {
    const fn label(self) -> &'static str {
        match self {
            Self::NoDesk => "no_desk",
            Self::NoOwner => "no_owner",
            Self::NotPayable => "not_payable",
            Self::WrongCost => "wrong_cost",
        }
    }
}

/// Which board of a town a desk view shows: what its 1746 carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BoardKind {
    /// The governor's desk: the governance data.
    GovernorDesk,
    /// The territory planning board: the progression data and the governance data.
    PlanningBoard,
    /// The town project board: the progression data alone (Observed: `Hivetownboardmissions`
    /// 21.342 s, the board's 1746 at the use; the client offers the town board missions of the
    /// projects it lists, and with none it shows no active project and no mission, live
    /// 2026-11-14).
    TownBoard,
}

impl BoardKind {
    /// The board kind of interaction kind `kind`: a planning board, else the governor's desk.
    fn of_desk_kind(kind: u8) -> Self {
        if kind == super::town_projects::PLANNING_BOARD_KIND {
            Self::PlanningBoard
        } else {
            Self::GovernorDesk
        }
    }

    const fn shows_progression(self) -> bool {
        matches!(self, Self::PlanningBoard | Self::TownBoard)
    }

    const fn shows_governance(self) -> bool {
        matches!(self, Self::GovernorDesk | Self::PlanningBoard)
    }
}

/// The desk a player has open.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DeskView {
    /// Its slot key.
    number: u64,
    /// Its territory.
    territory: u16,
    /// Its entity id (the card's `GdeRef`).
    target: [u8; RAW_ID_LEN],
    /// Which board it is.
    kind: BoardKind,
    /// Its slot key is the board's level entity slot ([`super::level_boards`]): the level slot
    /// sends its card, and the view adds its data alone.
    level_slot: bool,
    /// The upkeep version its client has; `None` before its first group.
    sent: Option<u64>,
}

/// A player's governor's desk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct PlayerDesk {
    view: Option<DeskView>,
    /// Desks opened on this sim: the next one gets a new slot key, so a new slot and card.
    opened: u64,
}

impl PlayerDesk {
    /// Shows the board `target` of kind `kind` of `territory` (the same board keeps its slot): in
    /// the slot of its level entity `level_number` when it is one, else in a slot of its own.
    pub(super) fn open(
        &mut self,
        territory: u16,
        target: [u8; RAW_ID_LEN],
        kind: BoardKind,
        level_number: Option<u64>,
    ) {
        if self
            .view
            .as_ref()
            .is_some_and(|view| view.target == target && view.territory == territory)
        {
            return;
        }
        let number = level_number.unwrap_or_else(|| {
            self.opened = self.opened.wrapping_add(1);
            DESK_ENTITY_BASE + self.opened
        });
        self.view = Some(DeskView {
            number,
            territory,
            target,
            kind,
            level_slot: level_number.is_some(),
            sent: None,
        });
    }

    /// The territory of the open planning board.
    pub(super) fn planning_territory(&self) -> Option<u16> {
        self.view
            .as_ref()
            .filter(|view| view.kind == BoardKind::PlanningBoard)
            .map(|view| view.territory)
    }

    /// Shows the open desk its governance data again with the next bundle.
    pub(super) fn show_again(&mut self) {
        if let Some(view) = self.view.as_mut() {
            view.sent = None;
        }
    }

    /// The territory of the open desk.
    fn territory(&self) -> Option<u16> {
        self.view.as_ref().map(|view| view.territory)
    }

    /// The slot key of the open desk while the player is in its territory (`claim`, the claim the
    /// player is in: [`claim_of`]); a player that left the territory closes it.
    pub(super) fn number_in(&mut self, claim: Option<u16>) -> Option<u64> {
        let view = self.view.as_ref()?;
        if claim == Some(view.territory) {
            return Some(view.number);
        }
        self.view = None;
        None
    }

    /// The bundle group of desk `number` in `slot` at Unix ms `now_ms`: in a new slot the card and
    /// the 1746 with slot 0 and the data of its kind ([`BoardKind`]: the governance data, the
    /// progression data of the town projects `projects`); after a change of the claims' upkeep or
    /// projects that data alone; else nothing.
    pub(super) fn group(
        &mut self,
        number: u64,
        slot: Slot,
        territories: &Territories,
        projects: &TerritoryProjects,
        now_ms: u64,
    ) -> Option<BundleGroup> {
        let view = self.view.as_mut().filter(|view| view.number == number)?;
        let version = territories.upkeep_version();
        if !slot.new && view.sent == Some(version) {
            return None;
        }
        view.sent = Some(version);
        let governance = view
            .kind
            .shows_governance()
            .then(|| territories.governance(view.territory, now_ms))
            .flatten();
        let mut members = Vec::new();
        let own_card = slot.new && !view.level_slot;
        if own_card {
            members.push(BundleMember {
                component: CARD_COMPONENT,
                state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                    asset: None,
                    gde_ref: Some(view.target),
                }),
            });
        } else if view.kind.shows_governance() && governance.is_none() {
            // The owner is gone: the client keeps the last data (no retail form takes it away).
            return None;
        }
        let dominion = territories.dominion();
        let progression = view.kind.shows_progression().then(|| {
            dominion
                .projects()
                .progression_data(view.territory, projects, |upgrade| {
                    dominion.tier(view.territory, upgrade)
                })
        });
        members.push(BundleMember {
            component: DESK_TERRITORY_COMPONENT,
            state: MemberState::TerritoryInterface(TerritoryInterfaceState {
                refs_set: own_card.then_some(TERRITORY_REFS_SET),
                progression,
                governance,
                ..TerritoryInterfaceState::default()
            }),
        });
        Some(BundleGroup {
            interest: u64::from(slot.interest),
            members,
        })
    }
}

/// The claim the player is in: the claim of the last of its territory areas (innermost last)
/// that is a claim or lies in one (`charting`). Live 2026-11-14: a client that logs in inside a
/// town names only the town's areas (Monarch's Bluffs area 30155), never the claim itself.
pub(super) fn claim_of(territories: &[u16], charting: &ChartRules) -> Option<u16> {
    territories.iter().rev().find_map(|area| {
        let claim = if CLAIM_KEYS.contains(area) {
            Some(*area)
        } else {
            charting
                .territory_of(u32::from(*area))
                .and_then(|territory| u16::try_from(territory).ok())
        };
        claim.filter(|claim| CLAIM_KEYS.contains(claim))
    })
}

impl SimWorld {
    /// Player `number` (`entity`) uses the governor's desk or the territory planning board of
    /// `request` (its name `name`): accepted (3743) with the 2722 of the claim the player is in,
    /// and the desk entity shows with the claim's governance data (a planning board's with the
    /// progression data too). A player in no claim is refused (Inferred: every desk stands in a
    /// town of a claim; retail's refusal status, `MSQ6-lv28`).
    pub(super) fn open_governor_desk(
        &mut self,
        entity: Entity,
        number: u64,
        request: InteractionRequest,
        name: &[u8],
    ) {
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let Some(territory) = claim_of(&player.territories, &self.content.charting) else {
            let refusal = gather_answer(number, request, GATHER_BUSY, &[]);
            send_now(&mut self.combat_messages, &player.connection, &[refusal]);
            return;
        };
        // Observed: 2 for a governor and a consul of a company, 0 for a player in none (Inferred
        // for a member of another company than the owner).
        let access = if player.social.company().is_some() {
            ACCESS_OF_A_MEMBER
        } else {
            ACCESS_OF_NO_MEMBER
        };
        let answers = [
            gather_answer(number, request, GATHER_ACCEPTED, name),
            ServerMessage::TerritoryFacetReady(TerritoryFacetReadyMsg::of(
                number, territory, access,
            )),
        ];
        send_now(&mut self.combat_messages, &player.connection, &answers);
        player.desk.open(
            territory,
            request.target,
            BoardKind::of_desk_kind(request.kind),
            None,
        );
    }

    /// The upkeep payment `call` of the player of `connection` at Unix ms `now_ms`: the social
    /// service's input that pays it, or `None` when this sim refuses it: the payer's desk then
    /// shows its governance data again (module doc).
    pub fn pay_upkeep_call(
        &mut self,
        connection: &ConnectionRef,
        call: &PayUpkeepCostMsg,
        now_ms: u64,
    ) -> Option<SocialInput> {
        let entity = *self.by_connection.get(connection)?;
        let player = self.world.get::<Player>(entity)?;
        let checked = payable(
            player.desk.territory(),
            &self.territories,
            self.content.upkeep_costs.cost(call.territory, CLAIM_LEVEL),
            call,
            now_ms,
        );
        match checked {
            Ok(()) => Some(SocialInput::PayUpkeep(UpkeepPayment {
                character: player.character,
                territory: call.territory,
                cost: call.cost,
                at_ms: now_ms,
            })),
            Err(refusal) => {
                metrics::counter!(METRIC_REFUSED_PAYMENTS, REASON_LABEL => refusal.label())
                    .increment(1);
                tracing::info!(
                    player = player.number,
                    territory = call.territory,
                    cost = call.cost,
                    ?refusal,
                    "upkeep payment refused"
                );
                self.world.get_mut::<Player>(entity)?.desk.show_again();
                None
            }
        }
    }

    /// The social service answered the upkeep payments `answers`. After a paid one the owners
    /// are read at once, and the open desks get the new due time when the read changed it; a
    /// refused one shows the payer's desk, when the payer is here, its governance data again.
    pub(super) fn upkeep_answered(&mut self, answers: &[UpkeepAnswer]) {
        for answer in answers {
            if answer.paid {
                tracing::info!(character = %answer.character, territory = answer.territory,
                    "upkeep paid");
                self.territories.read_soon();
                continue;
            }
            tracing::info!(character = %answer.character, territory = answer.territory,
                "upkeep not paid");
            let Some(entity) = self.player_of_character(answer.character) else {
                continue;
            };
            if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                player.desk.show_again();
            }
        }
    }
}

/// Whether the payment `call` is taken at Unix ms `now_ms`, for a player with the desk of
/// `desk` open, under the claims `territories` and the game data's cost `cost` of the claim.
fn payable(
    desk: Option<u16>,
    territories: &Territories,
    cost: Option<u64>,
    call: &PayUpkeepCostMsg,
    now_ms: u64,
) -> Result<(), Refusal> {
    if desk != Some(call.territory) {
        return Err(Refusal::NoDesk);
    }
    let upkeep = territories.upkeep(call.territory).ok_or(Refusal::NoOwner)?;
    if now_ms < upkeep.payable_ms {
        return Err(Refusal::NotPayable);
    }
    if cost != Some(call.cost) {
        return Err(Refusal::WrongCost);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nw_messages::{FacetHeader, TERRITORY_INTERACTOR_COMPONENT};

    /// A made-up claim, desk and player.
    const TERRITORY: u16 = 6;
    const AREA: u16 = 40_148;
    const DESK: [u8; RAW_ID_LEN] = [0x5f; RAW_ID_LEN];
    const OTHER_DESK: [u8; RAW_ID_LEN] = [0x6f; RAW_ID_LEN];
    const PLAYER: u64 = 3;
    const INTEREST: u16 = 40;
    /// The retail cost of the claims (`UpkeepCost` 1,000,000 x `UpkeepModifier` 0.1).
    const COST: u64 = 100_000;
    const NOW_MS: u64 = 1_783_335_287_110;

    const NEW: Slot = Slot {
        interest: INTEREST,
        new: true,
    };
    const KNOWN: Slot = Slot {
        interest: INTEREST,
        new: false,
    };

    fn call(territory: u16, cost: u64) -> PayUpkeepCostMsg {
        PayUpkeepCostMsg {
            header: FacetHeader::of(TERRITORY_INTERACTOR_COMPONENT, PLAYER),
            territory,
            flag: true,
            cost,
        }
    }

    /// The owner row of the claim: a made-up company whose upkeep is due at Unix ms `due_ms`.
    fn owner(due_ms: u64) -> nw_db::TerritoryOwner {
        nw_db::TerritoryOwner {
            territory: TERRITORY,
            company: uuid::Uuid::from_u128(0x1111),
            name: "Made Up Company".to_owned(),
            faction: 1,
            crest: [0; nw_db::CREST_LEN],
            since_ms: NOW_MS - nw_db::UPKEEP_INTERVAL_MS,
            upkeep_due_ms: due_ms,
        }
    }

    /// The claim owned by [`owner`].
    fn owned(due_ms: u64) -> Territories {
        Territories::read_for_test(vec![owner(due_ms)])
    }

    #[test]
    fn the_desk_claim_is_the_claim_of_the_innermost_area() {
        let unknown = ChartRules::default();
        assert_eq!(claim_of(&[TERRITORY, AREA], &unknown), Some(TERRITORY));
        assert_eq!(claim_of(&[AREA], &unknown), None);
        assert_eq!(claim_of(&[], &unknown), None);
        // Live 2026-11-14: a client that logs in inside a town names only the town's areas.
        let lists = [vec![TERRITORY, AREA]];
        let experience = crate::charting::TerritoryExperience {
            chart_amount: 0,
            standing_modifier: 1.0,
        };
        let charting = ChartRules::build([], [(u32::from(TERRITORY), experience)], &lists);
        assert_eq!(claim_of(&[AREA], &charting), Some(TERRITORY));
    }

    #[test]
    fn an_open_desk_shows_once_with_its_card_then_after_each_upkeep_change() {
        let mut desk = PlayerDesk::default();
        desk.open(TERRITORY, DESK, BoardKind::GovernorDesk, None);
        let number = desk.number_in(Some(TERRITORY)).expect("shown");
        let mut territories = owned(NOW_MS);
        let first = desk
            .group(
                number,
                NEW,
                &territories,
                &TerritoryProjects::default(),
                NOW_MS,
            )
            .expect("the spawn");
        assert_eq!(first.members[0].component, CARD_COMPONENT);
        let MemberState::GdeMetadata(card) = &first.members[0].state else {
            panic!("card");
        };
        assert_eq!(card.gde_ref, Some(DESK));
        let MemberState::TerritoryInterface(state) = &first.members[1].state else {
            panic!("territory interface");
        };
        assert_eq!(first.members[1].component, DESK_TERRITORY_COMPONENT);
        assert_eq!(state.refs_set, Some(TERRITORY_REFS_SET));
        assert_eq!(
            state.governance,
            territories.governance(TERRITORY, NOW_MS),
            "the owner's governance data"
        );
        assert_eq!(state.progression, None, "a desk shows no progression data");
        assert_eq!(desk.planning_territory(), None);
        assert!(
            desk.group(
                number,
                KNOWN,
                &territories,
                &TerritoryProjects::default(),
                NOW_MS
            )
            .is_none()
        );
        desk.show_again();
        let again = desk
            .group(
                number,
                KNOWN,
                &territories,
                &TerritoryProjects::default(),
                NOW_MS,
            )
            .expect("a refused payment shows the data again");
        let [member] = again.members.as_slice() else {
            panic!("the 1746 alone: {again:?}");
        };
        let MemberState::TerritoryInterface(state) = &member.state else {
            panic!("territory interface");
        };
        assert_eq!(state.governance, territories.governance(TERRITORY, NOW_MS));
        assert!(
            desk.group(
                number,
                KNOWN,
                &territories,
                &TerritoryProjects::default(),
                NOW_MS
            )
            .is_none()
        );
        // The read after the payment moves the due time on the same claims.
        territories.read_again_for_test(vec![owner(NOW_MS + nw_db::UPKEEP_INTERVAL_MS)]);
        let change = desk
            .group(
                number,
                KNOWN,
                &territories,
                &TerritoryProjects::default(),
                NOW_MS,
            )
            .expect("the new due time");
        let [member] = change.members.as_slice() else {
            panic!("the 1746 alone: {change:?}");
        };
        let MemberState::TerritoryInterface(state) = &member.state else {
            panic!("territory interface");
        };
        assert_eq!(state.refs_set, None);
        assert_eq!(state.governance, territories.governance(TERRITORY, NOW_MS));
        desk.open(TERRITORY, DESK, BoardKind::GovernorDesk, None);
        assert_eq!(
            desk.number_in(Some(TERRITORY)),
            Some(number),
            "the same desk"
        );
        desk.open(TERRITORY, OTHER_DESK, BoardKind::GovernorDesk, None);
        assert_ne!(
            desk.number_in(Some(TERRITORY)),
            Some(number),
            "another desk"
        );
        assert_eq!(desk.number_in(None), None, "the player left the claim");
        assert_eq!(desk.territory(), None);
    }

    #[test]
    fn a_planning_board_shows_the_progression_data_with_the_governance_data() {
        let mut desk = PlayerDesk::default();
        desk.open(TERRITORY, DESK, BoardKind::PlanningBoard, None);
        assert_eq!(desk.planning_territory(), Some(TERRITORY));
        let number = desk.number_in(Some(TERRITORY)).expect("shown");
        let territories = owned(NOW_MS);
        let group = desk
            .group(
                number,
                NEW,
                &territories,
                &TerritoryProjects::default(),
                NOW_MS,
            )
            .expect("the spawn");
        let MemberState::TerritoryInterface(state) = &group.members[1].state else {
            panic!("territory interface");
        };
        let progression = state.progression.as_ref().expect("progression data");
        assert_eq!(progression.slots.len(), 5, "one slot per progression type");
        assert!(state.governance.is_some());
    }

    #[test]
    fn a_town_board_shows_the_progression_data_alone_in_its_level_slot_and_again_after_a_change() {
        /// The slot key of the board's level entity.
        const LEVEL_NUMBER: u64 = crate::world::crafting::STATION_ENTITY_BASE + 7;
        let mut desk = PlayerDesk::default();
        desk.open(TERRITORY, DESK, BoardKind::TownBoard, Some(LEVEL_NUMBER));
        assert_eq!(desk.planning_territory(), None, "no project starts there");
        assert_eq!(desk.number_in(Some(TERRITORY)), Some(LEVEL_NUMBER));
        let territories = owned(NOW_MS);
        let projects = TerritoryProjects::default();
        let first = desk
            .group(LEVEL_NUMBER, NEW, &territories, &projects, NOW_MS)
            .expect("the data with the level entity's spawn");
        let [member] = first.members.as_slice() else {
            panic!("the 1746 alone: the level slot sends the card: {first:?}");
        };
        assert_eq!(member.component, DESK_TERRITORY_COMPONENT);
        let MemberState::TerritoryInterface(state) = &member.state else {
            panic!("territory interface");
        };
        assert_eq!(
            state.refs_set, None,
            "the level entity's spawn sets the references"
        );
        assert_eq!(
            state.progression.as_ref().map(|data| data.slots.len()),
            Some(5),
            "one slot per progression type"
        );
        assert_eq!(state.governance, None, "retail's town board 1746 has none");
        assert!(
            desk.group(LEVEL_NUMBER, KNOWN, &territories, &projects, NOW_MS)
                .is_none()
        );
        desk.show_again();
        let again = desk
            .group(LEVEL_NUMBER, KNOWN, &territories, &projects, NOW_MS)
            .expect("a change shows the progression data again, with no governance data");
        let [member] = again.members.as_slice() else {
            panic!("the 1746 alone: {again:?}");
        };
        let MemberState::TerritoryInterface(state) = &member.state else {
            panic!("territory interface");
        };
        assert!(state.progression.is_some());
    }

    #[test]
    fn a_payment_needs_the_open_desk_an_owner_the_time_and_the_cost() {
        let territories = owned(NOW_MS + nw_db::UPKEEP_PAYABLE_BEFORE_MS);
        let paying = |desk, cost, call: &PayUpkeepCostMsg, now_ms| {
            payable(desk, &territories, cost, call, now_ms)
        };
        let good = call(TERRITORY, COST);
        assert_eq!(paying(Some(TERRITORY), Some(COST), &good, NOW_MS), Ok(()));
        assert_eq!(
            paying(None, Some(COST), &good, NOW_MS),
            Err(Refusal::NoDesk)
        );
        assert_eq!(
            paying(Some(TERRITORY), Some(COST), &good, NOW_MS - 1),
            Err(Refusal::NotPayable)
        );
        assert_eq!(
            paying(
                Some(TERRITORY),
                Some(COST),
                &call(TERRITORY, COST - 1),
                NOW_MS
            ),
            Err(Refusal::WrongCost)
        );
        assert_eq!(
            paying(Some(TERRITORY), None, &good, NOW_MS),
            Err(Refusal::WrongCost),
            "a claim without game data"
        );
        let other = call(TERRITORY + 1, COST);
        assert_eq!(
            payable(
                Some(TERRITORY + 1),
                &territories,
                Some(COST),
                &other,
                NOW_MS
            ),
            Err(Refusal::NoOwner)
        );
    }
}
