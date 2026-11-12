// SPDX-License-Identifier: AGPL-3.0-or-later
//! Faction boards (faction board study 2026-11-11: 29 opens in 5 retail ledgers, `factionboard`
//! of the capture tools). A player opens the board at its faction's representative (the board
//! interaction on an NPC). The server names the board in the player's objective interactor state
//! (3829) with the missions it offers and an expiration 300 s later
//! ([`BOARD_EXPIRATION_NS`]). Observed in retail `Hivefactionquests` at 40.974 s: six missions
//! of the player's faction and of the board's territory, three PvE (`FactionMission_C_06_EXP_*`)
//! then three PvP (`FM_06_PVP_*`), each `available` and not taken; two opens 35 s apart held
//! the same list and the same expiration, an open 4.3 h later a new list.
//!
//! A take names a PvE mission by its index in the list (3816) and a PvP mission by the CRC-32
//! of its id (3818). Observed 30 to 50 ms after each: the list's element alone, taken at the
//! server's time and no longer available; the mission starts as a town mission does.
//!
//! Which missions a board offers (Inferred from the offers and the data): each slot of the
//! board's bucket draws a goal type by the mission weights ([`nw_content::mission_weights`]) and
//! then one mission of that type, of the player's faction, of the board's territory and of the
//! player's level. A mission's territory is the number its id names (`FM_C_Hunt_0607`,
//! `FactionMission_C_06_EXP_01_quest`, `FM_C_CRX_Breach_TerritoryID_6`: every retail offer's id
//! named one territory per board). A mission with a task this server does not run is not
//! offered: it could never end.

use bevy_ecs::entity::Entity;
use nw_bus::{MISSION_BOARD_ID_LEN, MissionTake};
use nw_content::factions::faction_requirement_met;
use nw_content::mission_weights::{FACTION_PVE_BUCKET, FACTION_PVP_BUCKET, draw_goal_type};
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::NS_PER_MS;
use nw_messages::list_delta::{ListChange, ReplicatedList};
use nw_messages::objective_interactor::{
    BOARD_EXPIRATION_NS, MissionParams, OBJECTIVE_INTERACTOR_COMPONENT, ObjectiveInteractorState,
};
use nw_messages::{BundleMember, MemberState};

use super::gear::unix_ns;
use super::{Player, SimWorld};
use crate::player_content::MissionRow;

/// The marker of a territory number at the end of a mission id (`FM_C_CRX_Breach_TerritoryID_6`).
const TERRITORY_ID_MARK: &str = "territoryid_";
/// The digits of a territory number in a mission id (`06` of `FM_06_PVP_SpySpot_Covenant_quest`,
/// the first two of `0607` in `FM_C_Hunt_0607`).
const TERRITORY_DIGITS: usize = 2;
/// The digits of a territory number and a mission number together (`0607`).
const TERRITORY_AND_NUMBER_DIGITS: usize = 4;
/// Milliseconds per nanosecond step of the expiration.
const EXPIRATION_MS: u64 = BOARD_EXPIRATION_NS / NS_PER_MS;

/// The missions a faction board offers one player, until they expire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BoardOffers {
    /// The board.
    provider: [u8; MISSION_BOARD_ID_LEN],
    /// Sim ms at which the offers end.
    expires_ms: u64,
    /// The `expirationTime` the client has: ns of server time.
    expiration_ns: u64,
    /// The offered missions, in list order.
    missions: Vec<MissionParams>,
}

/// How the client names the mission it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Taken {
    /// Its index in the offered list (3816).
    Index(u8),
    /// The CRC-32 of its lowercase id (3818).
    Mission(u32),
}

/// The territory a mission id names: the number after `TerritoryID_`, else the first part of
/// the id (between `_`) that is two digits, or the first two of four digits.
pub(super) fn mission_territory(id: &str) -> Option<u16> {
    let lower = id.to_lowercase();
    if let Some((_, number)) = lower.rsplit_once(TERRITORY_ID_MARK) {
        return number.parse().ok();
    }
    lower
        .split('_')
        .filter(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
        .find(|part| part.len() == TERRITORY_DIGITS || part.len() == TERRITORY_AND_NUMBER_DIGITS)
        .and_then(|part| part[..TERRITORY_DIGITS].parse().ok())
}

impl SimWorld {
    /// The offers of the faction board `provider` for player `entity` at `now_ms`: the ones
    /// it has while they have not expired, else new ones. Gives the expiration and the list.
    pub(super) fn faction_offers(
        &mut self,
        entity: Entity,
        provider: [u8; MISSION_BOARD_ID_LEN],
        now_ms: u64,
    ) -> (u64, Vec<MissionParams>) {
        let held = self
            .world
            .get::<Player>(entity)
            .and_then(|player| player.faction_offers.clone())
            .filter(|offers| offers.provider == provider && now_ms < offers.expires_ms);
        if let Some(offers) = held {
            return (offers.expiration_ns, offers.missions);
        }
        let missions: Vec<MissionParams> = self
            .draw_faction_missions(entity)
            .into_iter()
            .map(|mission| MissionParams::offered(self.mission_instance(), mission, provider))
            .collect();
        let expiration_ns = self
            .server_ms
            .saturating_mul(NS_PER_MS)
            .saturating_add(BOARD_EXPIRATION_NS);
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.faction_offers = Some(BoardOffers {
                provider,
                expires_ms: now_ms.saturating_add(EXPIRATION_MS),
                expiration_ns,
                missions: missions.clone(),
            });
        }
        (expiration_ns, missions)
    }

    /// A new mission instance: a random number, as retail's are.
    fn mission_instance(&mut self) -> u64 {
        let half = |dice: &mut nw_sim_core::Dice| u64::from(dice.below(u32::MAX));
        (half(&mut self.dice) << u32::BITS) | half(&mut self.dice)
    }

    /// Draws the missions a faction board offers player `entity` (the CRC-32 of each lowercase
    /// id): the PvE slots, then the PvP slots. No mission is offered twice; a slot with no
    /// mission left offers none.
    fn draw_faction_missions(&mut self, entity: Entity) -> Vec<u32> {
        let content = std::sync::Arc::clone(&self.content);
        let Some(player) = self.world.get::<Player>(entity) else {
            return Vec::new();
        };
        let faction = player.faction.faction;
        // The progression counts levels from 0; the mission rows from 1.
        let level = content.progression.level_of(player.experience) + 1;
        let territories: Vec<u32> = player
            .territories
            .iter()
            .map(|area| {
                let area = u32::from(*area);
                content.charting.territory_of(area).unwrap_or(area)
            })
            .collect();
        let fits = |mission: &MissionRow| {
            mission.required_faction.is_some()
                && faction_requirement_met(mission.required_faction.as_deref(), faction)
                && mission.levels.0.is_none_or(|min| level >= min)
                && mission.levels.1.is_none_or(|max| level <= max)
                && mission_territory(&mission.id)
                    .is_some_and(|territory| territories.contains(&u32::from(territory)))
                && content
                    .quests
                    .quest(&mission.id)
                    .is_some_and(|quest| quest.tree.runs())
        };
        let mut pool: Vec<&MissionRow> =
            content.quests.missions().filter(|row| fits(row)).collect();
        // The same order on every draw: the roll alone picks.
        pool.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        let mut offered = Vec::new();
        for (bucket, pvp) in [(FACTION_PVE_BUCKET, false), (FACTION_PVP_BUCKET, true)] {
            for slot in content.mission_weights.slots(bucket) {
                let open = |goal_type: &str| {
                    pool.iter()
                        .any(|mission| mission.pvp == pvp && mission.goal_type == goal_type)
                };
                let dice = &mut self.dice;
                let Some(goal_type) = draw_goal_type(slot, open, |total| dice.below(total)) else {
                    continue;
                };
                let of_type: Vec<usize> = (0..pool.len())
                    .filter(|index| pool[*index].pvp == pvp && pool[*index].goal_type == goal_type)
                    .collect();
                // A pool of at most a few hundred missions.
                let picked = of_type[self.dice.below(of_type.len() as u32) as usize];
                offered.push(lowercase_crc32(&pool.remove(picked).id));
            }
        }
        offered
    }

    /// Player `number` takes the faction mission `taken` of the board it has open, at `now_ms`:
    /// the mission starts, and the offer's element goes out taken.
    pub(super) fn take_faction_mission(&mut self, number: u64, taken: Taken, now_ms: u64) {
        let Some(entity) = self.player_by_number(number) else {
            return;
        };
        let offers = self
            .world
            .get::<Player>(entity)
            .and_then(|player| player.faction_offers.clone())
            .filter(|offers| now_ms < offers.expires_ms);
        let found = offers.as_ref().and_then(|offers| {
            let index = match taken {
                Taken::Index(index) => usize::from(index),
                Taken::Mission(mission) => offers
                    .missions
                    .iter()
                    .position(|offer| offer.mission == mission)?,
            };
            let offer = offers.missions.get(index).filter(|offer| offer.available)?;
            Some((index, offer.clone(), offers.provider))
        });
        let Some((index, offer, provider)) = found else {
            tracing::debug!(
                player = number,
                ?taken,
                "take of a mission the board does not offer"
            );
            return;
        };
        let Some(id) = self
            .content
            .quests
            .mission_id_by_crc(offer.mission)
            .map(str::to_owned)
        else {
            return;
        };
        let accepted_ns = unix_ns(self.server_ms);
        let take = MissionTake {
            board: provider,
            accepted_ns,
            territory: mission_territory(&id).unwrap_or_default(),
        };
        if let Err(refusal) = self.accept_quest(number, &id, Some(take), now_ms) {
            tracing::debug!(player = number, mission = %id, %refusal, "faction mission refused");
            return;
        }
        tracing::info!(player = number, mission = %id, "faction mission taken");
        let taken_offer = offer.taken(accepted_ns);
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        if let Some(offers) = &mut player.faction_offers {
            offers.missions[index] = taken_offer.clone();
        }
        player.board_version += 1;
        let state = ObjectiveInteractorState {
            provider: None,
            expiration_time: None,
            missions: Some(ReplicatedList::Changes(vec![ListChange {
                // A list of at most a dozen missions.
                index: index as u32,
                version: player.board_version,
                value: taken_offer,
            }])),
            community_goals: None,
        };
        player.extra_members.push(BundleMember {
            component: OBJECTIVE_INTERACTOR_COMPONENT,
            state: MemberState::ObjectiveInteractor(state),
        });
        self.update_quest_tracker(number);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mission_id_names_its_territory() {
        // The ids of the retail offers of one board (territory 6).
        for id in [
            "FactionMission_C_06_EXP_03_quest",
            "FM_C_CRX_Breach_TerritoryID_6",
            "FM_C_Hunt_0607",
            "FM_06_PVP_Control_Point_Covenant_quest",
            "FM_06_PVP_SpySpot_Covenant_quest",
        ] {
            assert_eq!(mission_territory(id), Some(6), "{id}");
        }
        assert_eq!(
            mission_territory("FactionMission_C_16_EXP_06_quest"),
            Some(16)
        );
        assert_eq!(
            mission_territory("FM_C_CRX_Breach_TerritoryID_12"),
            Some(12)
        );
        assert_eq!(mission_territory("M_gather_wood"), None, "no number");
        assert_eq!(
            mission_territory("FM_C_Raid_R1"),
            None,
            "no territory number"
        );
    }
}
