// SPDX-License-Identifier: AGPL-3.0-or-later
//! Territory influence from PvP faction missions (`nw_db::add_influence`).
//!
//! Observed (content bundle 25810808, `GameEventData`): the success event of a PvP faction
//! mission gives the player's faction `FactionInfluenceAmount` influence (50, 100 or 200,
//! `nw_content::rewards::EventGrant::faction_influence`). Inferred: the influence goes to the
//! mission's territory (the number its id names, [`mission_territory`]), the territory the
//! faction board offered it for. A player of no faction gains none (Inferred: the influence is a
//! faction's). The gain goes to the world database with the next territory job
//! ([`super::land_claims`]), so every sim shows it in the claims' influence at its next read.
//! There the faction's fort, tower and underdog bonus raise it, and the towers a faction holds
//! in the race window add their own influence every five seconds ([`nw_db::add_influence`],
//! [`super::control_points`]).

use super::SimWorld;
use super::faction_board::mission_territory;
use super::land_claims::InfluenceGain;
use crate::world::Player;

/// The faction byte of no faction.
const NO_FACTION: u8 = 0;

impl SimWorld {
    /// Player `number` finished the mission `mission` whose success event is `event`: its
    /// faction gains the event's influence in the mission's territory.
    pub(super) fn gain_mission_influence(&mut self, number: u64, mission: &str, event: &str) {
        let points = self
            .content
            .event_grants
            .get(event)
            .map_or(0, |grant| grant.faction_influence);
        if points == 0 {
            return;
        }
        let Some(territory) = mission_territory(mission) else {
            tracing::debug!(mission, "influence mission without a territory");
            return;
        };
        let Some((character, faction)) = self
            .player_by_number(number)
            .and_then(|entity| self.world.get::<Player>(entity))
            .map(|player| (player.character, player.faction.faction))
        else {
            return;
        };
        if faction == NO_FACTION {
            return;
        }
        let at_ms = self.unix_ms();
        self.territories.gain(InfluenceGain {
            territory,
            faction,
            character,
            points,
            at_ms,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nw_content::rewards::{EventGrant, EventGrants};

    use super::*;
    use crate::world::tests::{NOW, SERVER_TIME, admit_at, world_with_spawns};

    const PLAYER: u64 = 3;

    const EVENT: &str = "PVPFactionV2_12";
    const MISSION: &str = "FM_12_PVP_SpySpot_Covenant_quest";
    const POINTS: u64 = 100;
    const TERRITORY: u16 = 12;

    fn world_with_event(faction: u8) -> SimWorld {
        let mut world = world_with_spawns(Vec::new());
        let mut content = (*world.content).clone();
        content.event_grants = EventGrants::build([(
            EVENT.to_owned(),
            EventGrant {
                faction_influence: POINTS,
                ..EventGrant::default()
            },
        )]);
        world.content = Arc::new(content);
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
        let entity = world.player_by_number(PLAYER).expect("player");
        world
            .world
            .get_mut::<Player>(entity)
            .expect("player")
            .faction
            .faction = faction;
        world
    }

    #[test]
    fn a_mission_success_gives_its_faction_influence_in_its_territory() {
        let mut world = world_with_event(2);
        world.gain_mission_influence(PLAYER, MISSION, EVENT);
        let gains = world.territories.gains_for_test();
        assert_eq!(gains.len(), 1);
        assert_eq!(gains[0].territory, TERRITORY);
        assert_eq!(gains[0].faction, 2);
        assert_eq!(gains[0].points, POINTS);
    }

    #[test]
    fn no_faction_or_no_influence_gives_nothing() {
        let mut world = world_with_event(NO_FACTION);
        world.gain_mission_influence(PLAYER, MISSION, EVENT);
        assert!(world.territories.gains_for_test().is_empty());
        let mut world = world_with_event(1);
        world.gain_mission_influence(PLAYER, MISSION, "Another_Event");
        assert!(world.territories.gains_for_test().is_empty());
    }
}
