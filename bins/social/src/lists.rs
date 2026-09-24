// SPDX-License-Identifier: AGPL-3.0-or-later
//! The start-up data of the service: the profanity lists of the game paks and the icons of the
//! stored characters.

use std::collections::HashMap;
use std::path::Path;

use nw_db::SocialCharacter;
use nw_formats::pak::{PakArchive, PakError, find_paks};
use nw_login::look::saved_look;
use nw_messages::appearance::PlayerIconData;
use nw_messages::social::{ICON_LEN, icon_bytes};
use nw_oodle::KrakenDecompressor;
use nw_social::ProfanityFilter;
use uuid::Uuid;

/// The folder of the language dictionaries in the paks.
const DICTIONARY_PREFIX: &str = "dictionary/";
/// The file name of a profanity list.
const PROFANITY_FILE: &str = "/profanity.csv";

/// The profanity filter of every `dictionary/<locale>/profanity.csv` of the paks of
/// `game_dir`, in pak and path order.
///
/// # Errors
///
/// Returns [`PakError`] when the paks cannot be listed or a list cannot be read.
pub fn profanity_filter(game_dir: &Path) -> Result<ProfanityFilter, PakError> {
    let mut lists = Vec::new();
    for pak in find_paks(game_dir)? {
        let mut archive = PakArchive::open(&pak)?;
        let mut entries: Vec<_> = archive
            .entries()?
            .into_iter()
            .filter(|entry| {
                let path = entry.path.to_lowercase();
                path.starts_with(DICTIONARY_PREFIX) && path.ends_with(PROFANITY_FILE)
            })
            .collect();
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        for entry in entries {
            lists.push(archive.read(&entry, &KrakenDecompressor)?);
        }
    }
    Ok(ProfanityFilter::of_lists(lists.iter().map(Vec::as_slice)))
}

/// The icon of each character whose saved look reads.
pub fn saved_icons(characters: &[SocialCharacter]) -> HashMap<Uuid, [u8; ICON_LEN]> {
    characters
        .iter()
        .filter_map(|character| {
            saved_look(&character.appearance)
                .map(|look| (character.id, icon_bytes(&PlayerIconData::of(&look))))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_character_without_a_saved_look_has_no_icon() {
        let characters = [SocialCharacter {
            id: Uuid::from_u128(1),
            account: Uuid::from_u128(2),
            name: "Ann".to_owned(),
            appearance: serde_json::json!({}),
        }];
        assert!(saved_icons(&characters).is_empty());
    }
}
