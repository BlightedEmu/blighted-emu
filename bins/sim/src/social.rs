// SPDX-License-Identifier: AGPL-3.0-or-later
//! The answers of the client's social look-ups (run-15 core study): other players' data (4150 →
//! 4140) and the title list (4143 → 4147). A player of this sim answers from its live state;
//! another character from the world database.

use std::collections::HashMap;

use nw_content::progression::Progression;
use nw_db::{AccountTitle, CompanyListing, StoredPlayer};
use nw_login::look::saved_look;
use nw_messages::appearance::PlayerIconData;
use nw_messages::social::players::FAILURE_NO_DATA;
use nw_messages::social::RequestFailedMsg;
use nw_messages::{
    AvailableTitlesMsg, FacetHeader, GuildsRequest, NO_GUILD, NO_PLATFORM, NO_PLATFORM_PRESENCE,
    OtherGuildData, PlayerDataRequest, PlayerDataView, PlayerLook, ServerMessage,
};
use uuid::Uuid;

/// The pronoun of a character that never chose one: this server has no pronoun choice, and 0
/// is the value of 251 of 350 retail entries.
pub const DEFAULT_PRONOUN: u8 = 0;
/// The faction of a character in none (the faction state every new character starts with).
pub const NO_FACTION: u8 = 0;

/// 4147: the account titles, then the character titles not already in that list. A new mark stays
/// on the account title that has it.
pub fn titles_answer(
    header: FacetHeader,
    account: &[AccountTitle],
    character_titles: &[u32],
) -> ServerMessage {
    let mut available: Vec<u32> = account.iter().map(|title| title.title).collect();
    for title in character_titles {
        if !available.contains(title) {
            available.push(*title);
        }
    }
    let new: Vec<u32> = account
        .iter()
        .filter(|title| title.is_new)
        .map(|title| title.title)
        .collect();
    ServerMessage::AvailableTitles(AvailableTitlesMsg {
        header,
        available,
        new,
    })
}

/// The character ids of `request` that are UUID text, each once, in the request's order.
pub fn requested_characters(request: &PlayerDataRequest) -> Vec<Uuid> {
    let mut characters: Vec<Uuid> = Vec::new();
    for text in &request.characters {
        match std::str::from_utf8(text)
            .ok()
            .and_then(|text| Uuid::parse_str(text).ok())
        {
            Some(character) if !characters.contains(&character) => characters.push(character),
            Some(_) => {}
            None => tracing::debug!(
                id = %String::from_utf8_lossy(text),
                "player data request for an id that is no character id"
            ),
        }
    }
    characters
}

/// The data of character `character` named `name` with the look `look` at zero-based level
/// `level`, in world `world`. Characters of this server have no guild, faction, pronoun choice
/// or platform account.
pub fn player_view(
    character: Uuid,
    name: &str,
    look: Option<&PlayerLook>,
    level: u32,
    world: Uuid,
) -> PlayerDataView {
    PlayerDataView {
        character: character.hyphenated().to_string().into_bytes(),
        name: name.as_bytes().to_vec(),
        pronoun: DEFAULT_PRONOUN,
        icon: look.map_or_else(PlayerIconData::default, PlayerIconData::of),
        guild: NO_GUILD,
        level,
        faction: NO_FACTION,
        world: world.hyphenated().to_string().into_bytes(),
        presence: NO_PLATFORM_PRESENCE,
        platform: NO_PLATFORM,
        platform_user: Vec::new(),
    }
}

/// The data of the stored character `stored` in world `world`; its level from its saved
/// experience.
pub fn stored_view(
    stored: &StoredPlayer,
    progression: &Progression,
    world: Uuid,
) -> PlayerDataView {
    player_view(
        stored.id,
        &stored.name,
        saved_look(&stored.appearance).as_ref(),
        progression.level_of(stored.experience),
        world,
    )
}

/// The answer to `request` with the players of `found`, in the order of `characters`; a
/// character not found is left out, as retail did. When none is found, the answer is a 1926 with
/// reason 2, as retail answered a look-up of an unknown character.
pub fn player_data_answer(
    request: &PlayerDataRequest,
    characters: &[Uuid],
    mut found: HashMap<Uuid, PlayerDataView>,
) -> ServerMessage {
    let players: Vec<PlayerDataView> = characters
        .iter()
        .filter_map(|character| found.remove(character))
        .collect();
    if players.is_empty() {
        return ServerMessage::RequestFailed(RequestFailedMsg {
            header: request.header,
            request: request.request,
            reason: FAILURE_NO_DATA,
        });
    }
    ServerMessage::PlayerData(request.answer(players))
}

/// The answers to company look-up `request` with the companies of `listings`, in the order of
/// the request: the 3610 twice when it holds a company (retail sent every such answer twice,
/// the same bytes), else once.
pub fn guilds_answers(request: &GuildsRequest, listings: &[CompanyListing]) -> Vec<ServerMessage> {
    let guilds: Vec<OtherGuildData> = request
        .guilds
        .iter()
        .filter_map(|id| {
            listings
                .iter()
                .find(|listing| listing.id.as_bytes() == id)
                .map(nw_social::other_guild)
        })
        .collect();
    let copies = if guilds.is_empty() { 1 } else { RETAIL_GUILD_ANSWER_COPIES };
    let answer = ServerMessage::UnorderedGuilds(request.answer(guilds));
    vec![answer; copies]
}

/// How often retail sent a company look-up answer that held a company.
const RETAIL_GUILD_ANSWER_COPIES: usize = 2;

#[cfg(test)]
mod tests {
    use super::*;
    use nw_messages::PlayerDataResponseMsg;

    const WORLD: Uuid = Uuid::from_u128(0x1111_2222_3333_4444_5555_6666_7777_8888);
    const ADA: Uuid = Uuid::from_u128(1);
    const BEA: Uuid = Uuid::from_u128(2);
    const LEVEL: u32 = 4;
    /// CRC-32 of `playertitle_stoic` and `playertitle_freshstart`, and a character title.
    const STOIC: u32 = 0x431d_f4ea;
    const FRESH_START: u32 = 0xb187_3b49;
    const CULT_INITIATE: u32 = 7;

    fn request(ids: &[&str]) -> PlayerDataRequest {
        PlayerDataRequest {
            header: FacetHeader {
                target: 1,
                entity: 2,
            },
            request: [3; nw_messages::RAW_ID_LEN],
            characters: ids.iter().map(|id| id.as_bytes().to_vec()).collect(),
        }
    }

    #[test]
    fn requested_ids_are_parsed_once_in_order() {
        let ada = ADA.hyphenated().to_string();
        let bea = BEA.hyphenated().to_string();
        let request = request(&[&bea, "not an id", &ada, &bea]);
        assert_eq!(requested_characters(&request), [BEA, ADA]);
    }

    #[test]
    fn the_answer_holds_the_found_players_in_request_order() {
        let ada = ADA.hyphenated().to_string();
        let bea = BEA.hyphenated().to_string();
        let request = request(&[&ada, &bea]);
        let look = PlayerLook::default();
        let found = HashMap::from([(BEA, player_view(BEA, "Bea", Some(&look), LEVEL, WORLD))]);
        let ServerMessage::PlayerData(PlayerDataResponseMsg {
            request: id,
            players,
            ..
        }) = player_data_answer(&request, &[ADA, BEA], found)
        else {
            panic!("a player data answer");
        };
        assert_eq!(id, request.request);
        let [bea_view] = players.as_slice() else {
            panic!("only the found player: {players:?}");
        };
        assert_eq!(bea_view.character, bea.as_bytes());
        assert_eq!(bea_view.world, WORLD.hyphenated().to_string().as_bytes());
        assert_eq!(
            (bea_view.level, bea_view.platform, bea_view.guild),
            (LEVEL, NO_PLATFORM, NO_GUILD)
        );
    }

    #[test]
    fn a_look_up_that_finds_nobody_is_the_retail_failure() {
        let ada = ADA.hyphenated().to_string();
        let request = request(&[&ada]);
        let ServerMessage::RequestFailed(failed) =
            player_data_answer(&request, &[ADA], HashMap::new())
        else {
            panic!("a request failure");
        };
        assert_eq!(
            (failed.header, failed.request, failed.reason),
            (request.header, request.request, FAILURE_NO_DATA)
        );
    }

    #[test]
    fn a_found_company_is_answered_twice() {
        let id = Uuid::from_u128(9);
        let request = GuildsRequest {
            header: FacetHeader {
                target: 1,
                entity: 2,
            },
            request: [4; nw_messages::RAW_ID_LEN],
            guilds: vec![*id.as_bytes(), [5; nw_messages::RAW_ID_LEN]],
        };
        let listing = CompanyListing {
            id,
            name: "Alpha".to_owned(),
            governor: Some(ADA),
            faction: 1,
            crest: [0; nw_messages::social::CREST_LEN],
            members: 3,
            siege_hour: 18.0,
        };
        let answers = guilds_answers(&request, std::slice::from_ref(&listing));
        assert_eq!(answers.len(), RETAIL_GUILD_ANSWER_COPIES);
        let ServerMessage::UnorderedGuilds(answer) = &answers[0] else {
            panic!("a company answer");
        };
        assert_eq!(answer.guilds.len(), 1);
        assert_eq!(answer.guilds[0].name, b"Alpha");
        assert_eq!(answer.guilds[0].master, ADA.hyphenated().to_string().as_bytes());
        assert_eq!(answers[0], answers[1]);
        assert_eq!(guilds_answers(&request, &[]).len(), 1, "no company: one empty answer");
    }

    #[test]
    fn the_title_list_joins_account_and_character_titles() {
        let header = FacetHeader {
            target: 1,
            entity: 2,
        };
        let account = [
            AccountTitle {
                title: STOIC,
                is_new: false,
            },
            AccountTitle {
                title: FRESH_START,
                is_new: true,
            },
        ];
        let ServerMessage::AvailableTitles(list) =
            titles_answer(header, &account, &[CULT_INITIATE, STOIC])
        else {
            panic!("a title list");
        };
        assert_eq!(list.available, [STOIC, FRESH_START, CULT_INITIATE]);
        assert_eq!(list.new, [FRESH_START]);
        assert_eq!(list.header, header);
    }
}
