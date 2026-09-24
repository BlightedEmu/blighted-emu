// SPDX-License-Identifier: AGPL-3.0-or-later
//! The client's option calls and the live side of its social look-ups (run-15 core study):
//! the cull limit and the phasing acknowledgement change what the sim sends; the other options
//! have no server side here and are logged. Other players' data and the titles a player earned
//! come from its live state ([`crate::social`] builds the answers).

use std::collections::HashMap;

use nw_bus::ConnectionRef;
use nw_content::titles::{TitleProgress, TitleScope};
use nw_messages::{ClientOptionMsg, FULL_CULL_LIMIT_PERCENT, PlayerDataView};
use uuid::Uuid;

use super::{Player, SimWorld};
use crate::social::player_view;

/// The titles one character earned now, by whose they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EarnedTitles {
    /// The character.
    pub character: Uuid,
    /// The titles of its account it earned (`TitleType` `Account`).
    pub account: Vec<u32>,
    /// Its own titles (`TitleType` `Character`).
    pub character_titles: Vec<u32>,
}

/// The most other players a viewer with cull limit `percent` sees: that share of `max_visible`,
/// rounded up.
fn culled_visible(max_visible: usize, percent: u8) -> usize {
    max_visible
        .saturating_mul(usize::from(percent))
        .div_ceil(usize::from(FULL_CULL_LIMIT_PERCENT))
}

impl SimWorld {
    /// Takes the option call `option` of the client of `connection`.
    pub fn client_option(&mut self, connection: &ConnectionRef, option: &ClientOptionMsg) {
        let Some(&entity) = self.by_connection.get(connection) else {
            return;
        };
        let max_visible = self.settings.max_visible_players;
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        match option {
            ClientOptionMsg::CullLimit { percent, .. } => {
                let visible = culled_visible(max_visible, *percent);
                player.interest.set_max_visible(visible);
                tracing::debug!(player = player.number, percent, visible, "cull limit");
            }
            ClientOptionMsg::PhasingAck { sequence, .. } => player.flow.ack_phasing(*sequence),
            ClientOptionMsg::Settings { .. }
            | ClientOptionMsg::AutoTraversal { .. }
            | ClientOptionMsg::SpectatePublic { .. }
            | ClientOptionMsg::BandwidthMode { .. }
            | ClientOptionMsg::TelemetryOptions { .. } => {
                tracing::debug!(player = player.number, ?option, "client option");
            }
        }
    }

    /// The character of the player of `connection`.
    pub fn character_of(&self, connection: &ConnectionRef) -> Option<Uuid> {
        let entity = self.by_connection.get(connection)?;
        Some(self.world.get::<Player>(*entity)?.character)
    }

    /// The data of the players of `characters` that are in this sim, in world `world`.
    pub fn live_player_views(
        &self,
        characters: &[Uuid],
        world: Uuid,
    ) -> HashMap<Uuid, PlayerDataView> {
        let progression = &self.content.progression;
        self.by_connection
            .values()
            .filter_map(|entity| self.world.get::<Player>(*entity))
            .filter(|player| characters.contains(&player.character))
            .map(|player| {
                let level = progression.level_of(player.experience);
                let view = player_view(
                    player.character,
                    &player.name,
                    player.look.as_ref(),
                    level,
                    world,
                );
                (player.character, view)
            })
            .collect()
    }

    /// The titles the player of `connection` earned now by its level, ranks, achievements and
    /// meta achievements.
    pub fn earned_titles(&self, connection: &ConnectionRef) -> Option<EarnedTitles> {
        let entity = self.by_connection.get(connection)?;
        let player = self.world.get::<Player>(*entity)?;
        let content = &self.content;
        let achievements = player.progression().achievements;
        // Weapon masteries and trade skills are categorical progressions of one list.
        let rank = |id: &str| {
            content
                .progression
                .categoricals()
                .find(|categorical| categorical.id.eq_ignore_ascii_case(id))
                .map_or(0, |categorical| {
                    u32::from(
                        categorical
                            .rank_of(player.masteries.experience(categorical))
                            .0,
                    )
                })
        };
        let achievement = |id: &str| {
            content
                .achievements
                .index(id)
                .is_some_and(|bit| achievements.binary_search(&bit).is_ok())
        };
        let meta_achievement = |id: &str| player.meta.is_completed(id);
        let progress = TitleProgress {
            level: content.progression.level_of(player.experience),
            rank: &rank,
            achievement: &achievement,
            meta_achievement: &meta_achievement,
        };
        let mut earned = EarnedTitles {
            character: player.character,
            account: Vec::new(),
            character_titles: Vec::new(),
        };
        for title in content.titles.earned(&progress) {
            match title.scope {
                TitleScope::Account => earned.account.push(title.id),
                TitleScope::Character => earned.character_titles.push(title.id),
            }
        }
        Some(earned)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nw_content::titles::{TitleRequirement, TitleRules};
    use nw_formats::objectstream::lowercase_crc32;
    use nw_messages::FacetHeader;

    use super::*;
    use crate::player_content::PlayerContent;
    use crate::world::tests::{NOW, SERVER_TIME, admit_at, connection, world_with_spawns};

    const PLAYER: u64 = 3;
    const OTHER: u64 = 4;
    const X: f32 = 100.0;
    const HEADER: FacetHeader = FacetHeader {
        target: 0,
        entity: 0,
    };
    const LEVEL_TITLE: &str = "PlayerTitle_PlayerProgression1";
    const QUEST_TITLE: &str = "PlayerTitle_CultInitiate";
    const META_TITLE: &str = "PlayerTitle_WeaponMaster_Sword2";

    fn world() -> SimWorld {
        let mut world = world_with_spawns(Vec::new());
        world.content = Arc::new(PlayerContent {
            titles: TitleRules::build([
                (
                    LEVEL_TITLE,
                    TitleScope::Account,
                    TitleRequirement::PlayerLevel(0),
                ),
                (
                    QUEST_TITLE,
                    TitleScope::Character,
                    TitleRequirement::Achievement("14_Quest_20".to_owned()),
                ),
                (
                    META_TITLE,
                    TitleScope::Account,
                    TitleRequirement::MetaAchievement(vec!["WeaponMastery_Sword2".to_owned()]),
                ),
            ]),
            ..PlayerContent::default()
        });
        for number in [PLAYER, OTHER] {
            world
                .admit(&admit_at(number, X, true), NOW, SERVER_TIME)
                .expect("admit");
        }
        world
    }

    #[test]
    fn players_of_this_sim_answer_from_their_live_state() {
        let world = world();
        let character = world.character_of(&connection(PLAYER)).expect("here");
        let world_id = Uuid::from_u128(1);
        let views = world.live_player_views(&[character, Uuid::from_u128(2)], world_id);
        let view = views.get(&character).expect("its view");
        assert_eq!(views.len(), 1, "only players of this sim");
        assert_eq!(view.name, format!("Player{PLAYER}").as_bytes());
        assert_eq!(view.world, world_id.hyphenated().to_string().as_bytes());
        assert_eq!(world.character_of(&connection(OTHER + 1)), None);
    }

    #[test]
    fn earned_titles_are_split_by_whose_they_are() {
        let world = world();
        let earned = world.earned_titles(&connection(PLAYER)).expect("here");
        assert_eq!(earned.account, [lowercase_crc32(LEVEL_TITLE)]);
        assert!(
            earned.character_titles.is_empty(),
            "the quest achievement is not set; a meta achievement title needs a grant"
        );
        assert_eq!(
            Some(earned.character),
            world.character_of(&connection(PLAYER))
        );
    }

    #[test]
    fn the_phasing_acknowledgement_reaches_the_players_flow() {
        let mut world = world();
        let entity = world.player_by_number(PLAYER).expect("player");
        let sequence = world
            .world
            .get::<Player>(entity)
            .expect("player")
            .flow
            .phasing_sequence();
        world.client_option(
            &connection(PLAYER),
            &ClientOptionMsg::PhasingAck {
                header: HEADER,
                sequence,
            },
        );
        let player = world.world.get::<Player>(entity).expect("player");
        assert!(player.flow.phasing_acknowledged());
        // A call of a connection that has no player here changes nothing.
        world.client_option(
            &connection(OTHER + 1),
            &ClientOptionMsg::CullLimit {
                header: HEADER,
                percent: FULL_CULL_LIMIT_PERCENT,
            },
        );
    }

    /// The default `max_visible_players` (replication note, section 3).
    const MAX_VISIBLE: usize = 200;
    /// Cull limits the retail client sent as its frame rate changed.
    const RETAIL_LIMITS: [u8; 6] = [100, 96, 90, 83, 76, 70];

    #[test]
    fn the_cull_limit_takes_its_share_of_the_visible_players() {
        let visible: Vec<usize> = RETAIL_LIMITS
            .iter()
            .map(|percent| culled_visible(MAX_VISIBLE, *percent))
            .collect();
        assert_eq!(visible, [200, 192, 180, 166, 152, 140]);
        assert_eq!(culled_visible(1, 1), 1, "rounded up");
        assert_eq!(culled_visible(MAX_VISIBLE, 0), 0);
    }
}
