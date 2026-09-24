// SPDX-License-Identifier: AGPL-3.0-or-later
//! The title a player shows (2474, progression note section 10): a title the player earned
//! (its level, ranks, achievements or meta achievements earn it, [`SimWorld::earned_titles`]),
//! or none (0). The next own bundle carries it in the social state (type 4176), and the save
//! keeps it.

use nw_bus::ConnectionRef;
use nw_messages::SetTitleRequest;

use super::{Player, SimWorld};

/// The title id that shows none.
const NO_TITLE: u32 = 0;

impl SimWorld {
    /// The client of `connection` asks to show title `request.title`; a title the player did
    /// not earn is refused (logged) and the shown title stays.
    pub fn set_title(&mut self, connection: &ConnectionRef, request: &SetTitleRequest) {
        let Some(earned) = self.earned_titles(connection) else {
            return;
        };
        let title = request.title;
        let allowed = title == NO_TITLE
            || earned.account.contains(&title)
            || earned.character_titles.contains(&title);
        let Some(&entity) = self.by_connection.get(connection) else {
            return;
        };
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        if !allowed {
            tracing::info!(player = player.number, title, "title not earned: not shown");
            return;
        }
        player.title = (title != NO_TITLE).then_some(title);
        player.title_changed = true;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nw_content::titles::{TitleRequirement, TitleRules, TitleScope};
    use nw_formats::objectstream::lowercase_crc32;
    use nw_messages::FacetHeader;

    use super::*;
    use crate::player_content::PlayerContent;
    use crate::world::tests::{NOW, SERVER_TIME, admit_at, connection, world_with_spawns};

    const PLAYER: u64 = 7;
    const LEVEL_TITLE: &str = "PlayerTitle_PlayerProgression1";
    const GRANTED_TITLE: &str = "PlayerTitle_Stoic";
    const HEADER: FacetHeader = FacetHeader {
        target: 0,
        entity: 0,
    };

    fn shown(world: &mut SimWorld) -> (Option<u32>, bool) {
        let entity = world.player_by_number(PLAYER).expect("player");
        let player = world.world.get::<Player>(entity).expect("player");
        (player.title, player.title_changed)
    }

    #[test]
    fn only_an_earned_title_is_shown_and_saved() {
        let mut world = world_with_spawns(Vec::new());
        world.content = Arc::new(PlayerContent {
            titles: TitleRules::build([
                (
                    LEVEL_TITLE,
                    TitleScope::Account,
                    TitleRequirement::PlayerLevel(0),
                ),
                (GRANTED_TITLE, TitleScope::Account, TitleRequirement::Granted),
            ]),
            ..PlayerContent::default()
        });
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
        assert_eq!(shown(&mut world), (None, false));
        let ask = |title| SetTitleRequest {
            header: HEADER,
            title,
        };
        world.set_title(&connection(PLAYER), &ask(lowercase_crc32(GRANTED_TITLE)));
        assert_eq!(shown(&mut world), (None, false), "a grant this server never stored");
        world.set_title(&connection(PLAYER), &ask(lowercase_crc32(LEVEL_TITLE)));
        assert_eq!(shown(&mut world), (Some(lowercase_crc32(LEVEL_TITLE)), true));
        let saved = world
            .positions()
            .into_iter()
            .find_map(|departure| departure.progression)
            .expect("a save");
        assert_eq!(saved.title, Some(lowercase_crc32(LEVEL_TITLE)));
        world.set_title(&connection(PLAYER), &ask(NO_TITLE));
        assert_eq!(shown(&mut world).0, None);
    }
}
