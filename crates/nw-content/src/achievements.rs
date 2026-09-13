// SPDX-License-Identifier: AGPL-3.0-or-later
//! The achievement bit of each achievement id (tutorial-intro note, section 1).
//!
//! A player's achievements go to the client as one bitset: bit `AchievementIndex` of the
//! `AchievementData` sheets, byte `index / 8`, bit `index % 8`. Client-side triggers test them,
//! for example the tutorial intro video plays only when `FTUE_Intro2_Cinematic_Unlock` is set and
//! `FTUE_Intro2_Cinematic_Watched` is not.

use std::collections::HashMap;

use crate::Tables;

/// The bit of every achievement id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AchievementIndexes {
    by_id: HashMap<String, u32>,
    bitset_bytes: usize,
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
        let by_id: HashMap<String, u32> = rows.into_iter().collect();
        let bitset_bytes = by_id
            .values()
            .max()
            .map_or(0, |max| *max as usize / u8::BITS as usize + 1);
        Self {
            by_id,
            bitset_bytes,
        }
    }

    /// The bit of achievement `id`.
    pub fn index(&self, id: &str) -> Option<u32> {
        self.by_id.get(id).copied()
    }

    /// Bytes of a bitset that holds every achievement.
    pub const fn bitset_bytes(&self) -> usize {
        self.bitset_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTRO_UNLOCK: u32 = 14_804;
    const LAST: u32 = 17_266;

    #[test]
    fn ids_name_their_bits_and_the_bitset_holds_the_last_one() {
        let indexes = AchievementIndexes::build([
            ("FTUE_Intro2_Cinematic_Unlock".to_owned(), INTRO_UNLOCK),
            ("Last".to_owned(), LAST),
        ]);
        assert_eq!(
            indexes.index("FTUE_Intro2_Cinematic_Unlock"),
            Some(INTRO_UNLOCK)
        );
        assert_eq!(indexes.index("Nothing"), None);
        assert_eq!(indexes.bitset_bytes(), 2159);
        assert_eq!(AchievementIndexes::default().bitset_bytes(), 0);
    }
}
