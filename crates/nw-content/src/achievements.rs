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
    }

    /// The indexes of `rows`: (achievement id, index).
    pub fn build(rows: impl IntoIterator<Item = (String, u32)>) -> Self {
        Self {
            by_id: rows
                .into_iter()
                .map(|(id, index)| (id.to_ascii_lowercase(), index))
                .collect(),
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
    }
}
