// SPDX-License-Identifier: AGPL-3.0-or-later
//! The bot's own achievements, as a client has them: the server sends them as one bitset on the
//! own player (`MB::AchievementComponentReplicatedState`, type 3210, [`nw_messages::achievements`]),
//! bit `AchievementIndex` of the achievement table. The play bot notes the set before each
//! quest and reports what the quest set ([`AchievementBook::set_since`]); a quest whose
//! objective row names an `AchievementId` (set at completion, the next quest of a chain asks
//! for it) is broken when the server did not set that one ([`AchievementBook::missing`]).

use std::collections::{BTreeSet, HashMap};

use nw_content::Tables;
use nw_messages::achievements::{AchievementBits, AchievementState};

/// Bits in one byte of the set.
const BYTE_BITS: u32 = 8;

/// The achievements of the game data: the id of each bit, and the achievement each objective
/// row names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AchievementBook {
    /// The achievement id of each bit, as the table writes it.
    ids: HashMap<u32, String>,
    /// The bit of each achievement, by lowercase id.
    bits: HashMap<String, u32>,
    /// The `AchievementId` of each objective row that names one, by lowercase objective id.
    of_quests: HashMap<String, String>,
}

impl AchievementBook {
    /// The book of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(
            tables
                .achievement_data
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    let bit = u32::try_from(row.achievement_index).ok()?;
                    Some((row.achievement_id.clone(), bit))
                }),
            tables
                .objectives
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    let achievement = row.achievement_id.as_deref()?.trim();
                    (!achievement.is_empty())
                        .then(|| (row.objective_id.clone(), achievement.to_owned()))
                }),
        )
    }

    /// The book of `achievements` (id, bit) and `quests` (objective id, its achievement).
    pub fn build(
        achievements: impl IntoIterator<Item = (String, u32)>,
        quests: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        let mut book = Self::default();
        for (id, bit) in achievements {
            book.bits.insert(id.to_lowercase(), bit);
            book.ids.insert(bit, id);
        }
        book.of_quests = quests
            .into_iter()
            .map(|(quest, achievement)| (quest.to_lowercase(), achievement))
            .collect();
        book
    }

    /// The ids of the achievements that `own` holds and `before` did not, in the order of their
    /// bits; a bit the table does not name is written as `bit <n>`.
    pub fn set_since(&self, before: &BTreeSet<u32>, own: &OwnAchievements) -> Vec<String> {
        own.bits()
            .difference(before)
            .map(|bit| {
                self.ids
                    .get(bit)
                    .cloned()
                    .unwrap_or_else(|| format!("bit {bit}"))
            })
            .collect()
    }

    /// The achievement that the objective row of quest `quest` names and `own` does not hold;
    /// `None` for a quest whose row names none, or one the table has no bit for.
    pub fn missing(&self, quest: &str, own: &OwnAchievements) -> Option<&str> {
        let achievement = self.of_quests.get(&quest.to_lowercase())?;
        let bit = self.bits.get(&achievement.to_lowercase())?;
        (!own.bits().contains(bit)).then_some(achievement.as_str())
    }
}

/// The bot's own achievement bitset, kept from each state the server sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnAchievements {
    bytes: Vec<u8>,
    /// The bits of the first state: what the character logged in with.
    first: Option<BTreeSet<u32>>,
}

impl OwnAchievements {
    /// Takes one more state: the whole set, or its changed bytes.
    pub fn apply(&mut self, state: &AchievementState) {
        match &state.bits {
            Some(AchievementBits::Full(list)) => self.bytes.clone_from(&list.elements),
            Some(AchievementBits::Changes(changes)) => {
                for change in changes {
                    let Ok(index) = usize::try_from(change.byte) else {
                        continue;
                    };
                    if index >= self.bytes.len() {
                        self.bytes.resize(index + 1, 0);
                    }
                    self.bytes[index] = change.value;
                }
            }
            None => return,
        }
        if self.first.is_none() {
            self.first = Some(self.bits());
        }
    }

    /// The bits of the first state the server sent: the achievements the character logged in
    /// with; none before a state came.
    pub fn at_login(&self) -> BTreeSet<u32> {
        self.first.clone().unwrap_or_default()
    }

    /// The bits that are set.
    pub fn bits(&self) -> BTreeSet<u32> {
        (0_u32..)
            .zip(&self.bytes)
            .flat_map(|(index, byte)| {
                (0..BYTE_BITS)
                    .filter(move |bit| byte & (1 << bit) != 0)
                    .map(move |bit| index * BYTE_BITS + bit)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use nw_messages::achievements::ByteChange;
    use nw_messages::items::FullList;

    use super::*;

    const QUEST: &str = "12A_MSQ_09_ReturnRegent";
    const QUEST_ACHIEVEMENT: &str = "MB_MSQ_09_Complete";
    const QUEST_BIT: u32 = 9;
    const CRAFT_ACHIEVEMENT: &str = "FC_IngotT2";
    const CRAFT_BIT: u32 = 2;
    const UNNAMED_BIT: u32 = 20;

    fn book() -> AchievementBook {
        AchievementBook::build(
            [
                (QUEST_ACHIEVEMENT.to_owned(), QUEST_BIT),
                (CRAFT_ACHIEVEMENT.to_owned(), CRAFT_BIT),
            ],
            [(QUEST.to_owned(), QUEST_ACHIEVEMENT.to_owned())],
        )
    }

    fn change(bit: u32, value: u8) -> AchievementState {
        AchievementState {
            bits: Some(AchievementBits::Changes(vec![ByteChange {
                byte: bit / BYTE_BITS,
                version: 2,
                value,
            }])),
        }
    }

    #[test]
    fn the_own_set_is_the_full_state_then_each_changed_byte() {
        let mut own = OwnAchievements::default();
        own.apply(&AchievementState {
            bits: Some(AchievementBits::Full(FullList::first(vec![1 << CRAFT_BIT]))),
        });
        assert_eq!(own.bits(), BTreeSet::from([CRAFT_BIT]));
        own.apply(&AchievementState::default());
        assert_eq!(
            own.bits(),
            BTreeSet::from([CRAFT_BIT]),
            "no slot: no change"
        );
        assert_eq!(own.at_login(), BTreeSet::from([CRAFT_BIT]));
        // A change of a byte past the set's end makes the set longer.
        own.apply(&change(UNNAMED_BIT, 1 << (UNNAMED_BIT % BYTE_BITS)));
        own.apply(&change(QUEST_BIT, 1 << (QUEST_BIT % BYTE_BITS)));
        assert_eq!(
            own.bits(),
            BTreeSet::from([CRAFT_BIT, QUEST_BIT, UNNAMED_BIT])
        );
        assert_eq!(
            own.at_login(),
            BTreeSet::from([CRAFT_BIT]),
            "the login set stays the first state's"
        );
    }

    #[test]
    fn a_quest_names_what_it_set_and_the_achievement_of_its_row_that_is_missing() {
        let book = book();
        let mut own = OwnAchievements::default();
        own.apply(&change(CRAFT_BIT, 1 << CRAFT_BIT));
        let before = own.bits();
        assert_eq!(book.missing(QUEST, &own), Some(QUEST_ACHIEVEMENT));
        assert_eq!(book.missing("trade:Smelting", &own), None, "no row");
        own.apply(&change(QUEST_BIT, 1 << (QUEST_BIT % BYTE_BITS)));
        own.apply(&change(UNNAMED_BIT, 1 << (UNNAMED_BIT % BYTE_BITS)));
        assert_eq!(
            book.set_since(&before, &own),
            [QUEST_ACHIEVEMENT.to_owned(), format!("bit {UNNAMED_BIT}")],
            "the craft achievement was set before"
        );
        assert_eq!(book.missing(&QUEST.to_lowercase(), &own), None);
    }
}
