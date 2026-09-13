// SPDX-License-Identifier: AGPL-3.0-or-later
//! The content a sim needs for its players besides combat: the starter kit of each archetype
//! pick and the bit of each achievement.

use nw_content::Tables;
use nw_content::achievements::AchievementIndexes;
use nw_content::starter::StarterKits;

/// The player content of one content bundle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerContent {
    /// The login kit of each archetype pick.
    pub kits: StarterKits,
    /// The bit of each achievement.
    pub achievements: AchievementIndexes,
}

impl PlayerContent {
    /// The player content of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self {
            kits: StarterKits::from_tables(tables),
            achievements: AchievementIndexes::from_tables(tables),
        }
    }
}
