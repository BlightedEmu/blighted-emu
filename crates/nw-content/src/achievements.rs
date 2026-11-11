// SPDX-License-Identifier: AGPL-3.0-or-later
//! The achievement bit of each achievement id (tutorial-intro note, section 1).
//!
//! A player's achievements go to the client as one bitset: bit `AchievementIndex` of the
//! `AchievementData` sheets, byte `index / 8`, bit `index % 8`. Client-side triggers test them,
//! for example the tutorial intro video plays only when `FTUE_Intro2_Cinematic_Unlock` is set and
//! `FTUE_Intro2_Cinematic_Watched` is not.

use std::collections::HashMap;

use crate::Tables;

/// The bit of every achievement id, without case: objective tasks name achievements in another
/// case than the sheets (Observed: `tutorial_gathering_bushflint` for
/// `Tutorial_Gathering_BushFlint`, 19 task rows in all), and no two ids of the sheets differ in
/// case alone (17,005 ids).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AchievementIndexes {
    /// Bits by lowercase id.
    by_id: HashMap<String, u32>,
    /// The `GameEventID` of each achievement that has one, by lowercase id (Observed, content
    /// bundle 25810808: the 1,342 `Lore_unlock_` journal rows name `Read_LoreItem`, 2 points of
    /// `UniversalExpAmount`).
    game_events: HashMap<String, String>,
}

impl AchievementIndexes {
    /// The indexes of `tables`; a row with an index outside `u32` is left out.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(
            tables
                .achievement_data
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    let index = u32::try_from(row.achievement_index).ok()?;
                    Some((row.achievement_id.clone(), index))
                }),
        )
        .with_game_events(
            tables
                .achievement_data
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    let event = row.game_event_id.as_deref()?.trim();
                    (!event.is_empty()).then(|| (row.achievement_id.clone(), event.to_owned()))
                }),
        )
    }

    /// The same indexes with the game events `events` (achievement id, event id).
    #[must_use]
    pub fn with_game_events(mut self, events: impl IntoIterator<Item = (String, String)>) -> Self {
        self.game_events = events
            .into_iter()
            .map(|(id, event)| (id.to_ascii_lowercase(), event))
            .collect();
        self
    }

    /// The game event achievement `id` (any case) names.
    pub fn game_event(&self, id: &str) -> Option<&str> {
        self.game_events
            .get(&id.to_ascii_lowercase())
            .map(String::as_str)
    }

    /// The indexes of `rows`: (achievement id, index).
    pub fn build(rows: impl IntoIterator<Item = (String, u32)>) -> Self {
        Self {
            by_id: rows
                .into_iter()
                .map(|(id, index)| (id.to_ascii_lowercase(), index))
                .collect(),
            game_events: HashMap::new(),
        }
    }

    /// The bit of achievement `id`, in any case.
    pub fn index(&self, id: &str) -> Option<u32> {
        self.by_id.get(&id.to_ascii_lowercase()).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTRO_UNLOCK: u32 = 14_804;
    const LAST: u32 = 17_266;

    #[test]
    fn ids_name_their_bits() {
        let indexes = AchievementIndexes::build([
            ("FTUE_Intro2_Cinematic_Unlock".to_owned(), INTRO_UNLOCK),
            ("Last".to_owned(), LAST),
        ]);
        assert_eq!(
            indexes.index("FTUE_Intro2_Cinematic_Unlock"),
            Some(INTRO_UNLOCK)
        );
        assert_eq!(indexes.index("Nothing"), None);
        assert_eq!(
            indexes.index("ftue_intro2_cinematic_unlock"),
            Some(INTRO_UNLOCK),
            "a task names an achievement in another case"
        );
        assert_eq!(indexes.index("Last"), Some(LAST));
        let with_event = indexes.with_game_events([(
            "Lore_unlock_Weavers_Title_01_01".to_owned(),
            "Read_LoreItem".to_owned(),
        )]);
        assert_eq!(
            with_event.game_event("lore_unlock_weavers_title_01_01"),
            Some("Read_LoreItem")
        );
        assert_eq!(with_event.game_event("Last"), None);
    }
}
