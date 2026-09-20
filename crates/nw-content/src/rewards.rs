// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rewards of game events (beach-quests note, section 1): a quest's `SuccessGameEventID`
//! names a `GameEventData` row whose `CurrencyReward` coins, `UniversalExpAmount` experience and
//! `ItemReward` the player gets at the turn-in (retail: 150 + 225 coins after the first two beach
//! quests). An event can also name a whisper, a voice line the client plays (retail 2026-09-18:
//! `01_IsabellasFirstWhisper_Isabella_01` in Amidst the Wreckage).

use std::collections::HashMap;

use crate::Tables;

/// What one game event gives.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reward {
    /// `CurrencyReward`: coins.
    pub coins: u64,
    /// `UniversalExpAmount`: experience points.
    pub experience: u32,
    /// `ItemReward` and `ItemRewardQty`.
    pub item: Option<(String, u32)>,
    /// `WhisperID`, else `WhisperId`: the whisper the client plays.
    pub whisper: Option<String>,
}

/// Every game event's reward by lowercase event id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rewards {
    by_id: HashMap<String, Reward>,
}

/// A number cell (`"225"`); 0 for an empty or other cell.
fn number_of<T: std::str::FromStr + Default>(cell: Option<&str>) -> T {
    cell.and_then(|text| text.trim().parse().ok())
        .unwrap_or_default()
}

impl Rewards {
    /// The rewards of `GameEventData` in `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(
            tables
                .game_event_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| {
                    let item =
                        row.item_reward
                            .clone()
                            .filter(|item| !item.is_empty())
                            .map(|item| {
                                let quantity = row
                                    .item_reward_qty
                                    .and_then(|quantity| u32::try_from(quantity).ok())
                                    .filter(|quantity| *quantity > 0)
                                    .unwrap_or(1);
                                (item, quantity)
                            });
                    (
                        row.event_id.clone(),
                        Reward {
                            coins: number_of(row.currency_reward.as_deref()),
                            experience: number_of(row.universal_exp_amount.as_deref()),
                            item,
                            whisper: row
                                .whisper_id
                                .clone()
                                .or_else(|| row.whisper_id_2.clone())
                                .filter(|whisper| !whisper.is_empty()),
                        },
                    )
                }),
        )
    }

    /// The rewards of `events` (event id, reward); the first row of an id wins.
    pub fn build(events: impl IntoIterator<Item = (String, Reward)>) -> Self {
        let mut by_id = HashMap::new();
        for (id, reward) in events {
            by_id.entry(id.to_lowercase()).or_insert(reward);
        }
        Self { by_id }
    }

    /// The reward of event `id` (any case).
    pub fn get(&self, id: &str) -> Option<&Reward> {
        self.by_id.get(&id.to_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_gives_its_coins_experience_and_item() {
        let rewards = Rewards::build([(
            "01_skinningknife_success".to_owned(),
            Reward {
                coins: number_of(Some("225")),
                experience: number_of(Some("275")),
                item: Some(("QuestReward_MB_Meat_Pouch".to_owned(), 1)),
                whisper: None,
            },
        )]);
        let reward = rewards.get("01_SkinningKnife_Success").expect("reward");
        assert_eq!(reward.coins, 225);
        assert_eq!(reward.experience, 275);
        assert_eq!(number_of::<u64>(Some("")), 0);
        assert!(rewards.get("other").is_none());
    }
}
