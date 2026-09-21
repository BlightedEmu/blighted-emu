// SPDX-License-Identifier: AGPL-3.0-or-later
//! The GM calls of a game master's client (docs/re/2026-09-18-progression-and-gm-mode.md
//! section 5): its level, coins, weapon masteries and teleports. A call from a player without
//! the game master rank is refused with a log line; the client checks nothing itself. Each call
//! changes only the sender.

use nw_bus::{AdminReply, ConnectionRef, Point};
use nw_messages::GmClientMsg;

use super::{Player, SimWorld};

impl SimWorld {
    /// Carries out the GM call `call` of the client of `connection` at `now_ms`.
    pub fn gm_call(&mut self, connection: &ConnectionRef, call: GmClientMsg, now_ms: u64) {
        let Some(&entity) = self.by_connection.get(connection) else {
            return;
        };
        let Some((number, game_master)) = self
            .world
            .get::<Player>(entity)
            .map(|player| (player.number, player.game_master))
        else {
            return;
        };
        let call_type = call.type_index();
        if !game_master {
            // Debug only: a changed client could send these without end.
            tracing::debug!(
                entity = number,
                call_type,
                "GM call refused: no game master rank"
            );
            return;
        }
        let reply = self.gm_reply(number, call, now_ms);
        tracing::info!(entity = number, call_type, ?call, ?reply, "GM call");
    }

    fn gm_reply(&mut self, number: u64, call: GmClientMsg, now_ms: u64) -> AdminReply {
        match call {
            GmClientMsg::RequestGmPermissions { .. } => AdminReply::Done,
            GmClientMsg::SetPlayerLevel { level, .. } => self.set_level(number, level),
            GmClientMsg::SetCurrency { amount, .. } => self.set_coins(number, amount),
            GmClientMsg::ModifyProgression {
                amount,
                progression,
                ..
            } => match self.mastery_id(progression) {
                Some(id) => self.give_mastery_experience(
                    number,
                    &id,
                    u32::try_from(amount).unwrap_or(u32::MAX),
                ),
                None => unknown_progression(progression),
            },
            GmClientMsg::SetProgressionRank {
                rank, progression, ..
            } => match self.mastery_id(progression) {
                Some(id) => self.set_mastery_rank(number, Some(&id), rank),
                None => unknown_progression(progression),
            },
            GmClientMsg::Teleport { x, y, z, .. } => {
                self.teleport_player(number, Point { x, y, z }, now_ms)
            }
        }
    }

    fn mastery_id(&self, progression: u32) -> Option<String> {
        self.content
            .progression
            .mastery_by_crc(progression)
            .map(|mastery| mastery.id.clone())
    }
}

fn unknown_progression(progression: u32) -> AdminReply {
    AdminReply::Failed {
        reason: format!("no weapon mastery with the id {progression:#010x}"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nw_content::progression::Progression;
    use nw_messages::FacetHeader;

    use super::*;
    use crate::player_content::PlayerContent;
    use crate::world::tests::{NOW, SERVER_TIME, admit_at, connection, world_with_spawns};

    const GM: u64 = 41;
    const PLAYER: u64 = 42;
    const SWORD: &str = "GreatSwordAbilityTable";
    const HEADER: FacetHeader = FacetHeader {
        target: 0,
        entity: 0,
    };

    fn world() -> SimWorld {
        let mut world = world_with_spawns(Vec::new());
        world.content = Arc::new(PlayerContent {
            progression: Progression::build([(0, 0), (1, 150)], [(0, 10), (1, 50)], [(SWORD, 2)]),
            ..PlayerContent::default()
        });
        let gm = nw_bus::Admit {
            game_master: true,
            ..admit_at(GM, 0.0, true)
        };
        world.admit(&gm, NOW, SERVER_TIME).expect("gm");
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("player");
        world
    }

    fn progress(world: &mut SimWorld, number: u64) -> Option<nw_bus::PlayerProgression> {
        world
            .positions()
            .into_iter()
            .find(|departure| departure.connection == connection(number))
            .and_then(|departure| departure.progression)
    }

    #[test]
    fn a_game_masters_calls_change_only_its_own_progress() {
        let mut world = world();
        let sword = world
            .content
            .progression
            .mastery(SWORD)
            .expect("sword")
            .clone();
        for number in [GM, PLAYER] {
            for call in [
                GmClientMsg::SetPlayerLevel {
                    header: HEADER,
                    level: 1,
                },
                GmClientMsg::SetCurrency {
                    header: HEADER,
                    amount: 9,
                },
                GmClientMsg::ModifyProgression {
                    header: HEADER,
                    flag: 0,
                    amount: 20,
                    progression: sword.crc,
                },
            ] {
                world.gm_call(&connection(number), call, NOW);
            }
        }
        let gm = progress(&mut world, GM).expect("the GM's progress changed");
        assert_eq!((gm.experience, gm.coins), (150, 9));
        assert_eq!(gm.masteries.get(SWORD), Some(&20));
        assert_eq!(
            progress(&mut world, PLAYER),
            None,
            "refused without the rank"
        );
    }

    #[test]
    fn a_rank_call_for_an_unknown_progression_changes_nothing() {
        let mut world = world();
        world.gm_call(
            &connection(GM),
            GmClientMsg::SetProgressionRank {
                header: HEADER,
                rank: 1,
                progression: 0,
            },
            NOW,
        );
        assert_eq!(progress(&mut world, GM), None);
    }
}
