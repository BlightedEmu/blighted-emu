// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rewards of the end event of a game mode match (capture the flag: `CaptureTheFlag_T4` for a
//! win, `_T2` for a loss, `_T3` for a tie).
//!
//! Each amount is the `RewardData` `War*` column of the event's `Level` row times the column of
//! the `RewardModifierData` row whose `Modifiers` is the event id; the event's own
//! `CurrencyReward`, `FactionTokens` and `UniversalExpAmount` cells are not used.
//!
//! Observed (retail ledgers `CTF01` 1222.234 s, `CTF02` 724.242 s, `CTF04` 754.390 s with
//! `CaptureTheFlag_T4`, `CTF03` 494.251 s with `CaptureTheFlag_T2`; content bundle 25810808):
//! - Coins: 25,000 for both events (the 497 entry's `value_a` and the wallet 22,100,427 to
//!   22,125,427): `WarCurrency` 9,000 times `Currency` 2.7778, rounded. The event's
//!   `CurrencyReward` is 1,800.
//! - PvP experience: +1,350 (T4) and +800 (T2): `WarPvpXp` 1 times `PvpXpModifier`.
//! - Faction tokens: +1,423 (T4, three ledgers) and +843 (T2): `WarFactionTokens` 800 times
//!   `FactionTokenModifier` 1.6875 and 1.0, times the same share of about 1.054 for both events
//!   (Inferred: the player's held effects with a `FactionTokensMod`, which the server applies
//!   at the gain). The event's `FactionTokens` (1,100 and 650) give no common share.
//! - Azoth: the 497 entry's `value_f` is 125 (T4) and 75 (T2): `WarAzoth` 250 times `Azoth` 0.5
//!   and 0.3. The player's azoth stood at its cap 2,500, so no gain shows.
//! - Umbral shards: the event's `UmbralCurrency` cell (150, 40; the 497 entry's `value_h`); it
//!   has no `War*` column. `CTF01` gave +147 to the cap 100,000 (150 under the remote
//!   multiplier 3.0 is 450).
//! - Character experience: no 899 change at the end of `CTF01` (level 69): `Experience` is 0.0
//!   for T4 and T2. For T3, `WarExperience` 2,400 times 1.1459 is 2,750, the event's own
//!   `UniversalExpAmount` (Inferred: the same rule).
//! - Items: the event's `ItemReward` loot table (`[LTID]CaptureTheFlagRewardT4`).
//!
//! Inferred: the event's `Level` (60 for every capture the flag event) picks the `RewardData`
//! row; the player was level 69 and rows 60 to 66 hold the same `War*` values, so no ledger
//! tells the two apart. `FactionReputationModifier` scales `WarFactionReputation` as the tokens
//! (no ledger shows a reputation gain: the player's reputation did not change).

use std::collections::HashMap;

use crate::Tables;
use crate::generated::{RewardData, RewardModifierData};

/// The quantity of an item reward without an `ItemRewardQty`.
const ONE_ITEM: u32 = 1;
/// The factor of a modifier cell that is empty.
const NO_FACTOR: f64 = 0.0;

/// What the end event of a match gives each player.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModeReward {
    /// Coins (`WarCurrency` x `Currency`, rounded).
    pub coins: u64,
    /// Character experience (`WarExperience` x `Experience`, rounded).
    pub experience: u32,
    /// `PvP_XP` points (`WarPvpXp` x `PvpXpModifier`).
    pub pvp_xp: f64,
    /// Faction reputation (`WarFactionReputation` x `FactionReputationModifier`).
    pub faction_reputation: f64,
    /// Faction tokens (`WarFactionTokens` x `FactionTokenModifier`).
    pub faction_tokens: f64,
    /// Azoth (`WarAzoth` x `Azoth`).
    pub azoth: f64,
    /// Umbral shards (the event's `UmbralCurrency`).
    pub umbral: f64,
    /// The event's `ItemReward` and `ItemRewardQty` (a `[LTID]` loot table or an item).
    pub item: Option<(String, u32)>,
}

/// The `War*` columns of one `RewardData` row.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WarAmounts {
    /// `WarCurrency`.
    pub currency: f64,
    /// `WarExperience`.
    pub experience: f64,
    /// `WarPvpXp`.
    pub pvp_xp: f64,
    /// `WarFactionReputation`.
    pub faction_reputation: f64,
    /// `WarFactionTokens`.
    pub faction_tokens: f64,
    /// `WarAzoth`.
    pub azoth: f64,
}

/// The factors of one `RewardModifierData` row (an empty cell is 0).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ModeModifiers {
    /// `Currency`.
    pub currency: f64,
    /// `Experience`.
    pub experience: f64,
    /// `PvpXpModifier`.
    pub pvp_xp: f64,
    /// `FactionReputationModifier`.
    pub faction_reputation: f64,
    /// `FactionTokenModifier`.
    pub faction_tokens: f64,
    /// `Azoth`.
    pub azoth: f64,
}

/// An `i64` table cell as a float (table amounts are far below 2^53).
#[allow(
    clippy::cast_precision_loss,
    reason = "table amounts are far below 2^53"
)]
fn amount(cell: i64) -> f64 {
    cell as f64
}

/// A rounded, non-negative amount.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to the target range first"
)]
fn whole<T: TryFrom<u64> + Default>(raw: f64) -> T {
    let rounded = raw.round().clamp(0.0, u64::MAX as f64) as u64;
    T::try_from(rounded).unwrap_or_default()
}

impl WarAmounts {
    fn of(row: &RewardData) -> Self {
        Self {
            currency: amount(row.war_currency),
            experience: amount(row.war_experience),
            pvp_xp: amount(row.war_pvp_xp),
            faction_reputation: amount(row.war_faction_reputation),
            faction_tokens: amount(row.war_faction_tokens),
            azoth: amount(row.war_azoth),
        }
    }
}

impl ModeModifiers {
    fn of(row: &RewardModifierData) -> Self {
        let factor = |cell: Option<f64>| cell.unwrap_or(NO_FACTOR);
        Self {
            currency: factor(row.currency),
            experience: row.experience,
            pvp_xp: factor(row.pvp_xp_modifier),
            faction_reputation: factor(row.faction_reputation_modifier),
            faction_tokens: factor(row.faction_token_modifier),
            azoth: factor(row.azoth),
        }
    }
}

impl ModeReward {
    /// The reward of `war` under `modifiers`, with the event's umbral shards and item.
    pub fn of(
        war: WarAmounts,
        modifiers: ModeModifiers,
        umbral: f64,
        item: Option<(String, u32)>,
    ) -> Self {
        Self {
            coins: whole(war.currency * modifiers.currency),
            experience: whole(war.experience * modifiers.experience),
            pvp_xp: war.pvp_xp * modifiers.pvp_xp,
            faction_reputation: war.faction_reputation * modifiers.faction_reputation,
            faction_tokens: war.faction_tokens * modifiers.faction_tokens,
            azoth: war.azoth * modifiers.azoth,
            umbral,
            item,
        }
    }
}

/// The rewards of the events that have a `RewardModifierData` row of their own id and a
/// `RewardData` row at their `Level`, by lowercase event id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModeRewards {
    by_id: HashMap<String, ModeReward>,
}

impl ModeRewards {
    /// The rewards of the game events in `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        let war_by_level: HashMap<i64, WarAmounts> = tables
            .reward_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| (row.level, WarAmounts::of(row)))
            .collect();
        let modifiers: HashMap<String, ModeModifiers> = tables
            .reward_modifier_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| (row.modifiers.to_lowercase(), ModeModifiers::of(row)))
            .collect();
        Self::build(
            tables
                .game_event_data
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    let id = row.event_id.to_lowercase();
                    let war = *war_by_level.get(&row.level?)?;
                    let modifiers = *modifiers.get(&id)?;
                    let item =
                        row.item_reward
                            .clone()
                            .filter(|item| !item.is_empty())
                            .map(|item| {
                                let quantity = row
                                    .item_reward_qty
                                    .and_then(|quantity| u32::try_from(quantity).ok())
                                    .unwrap_or(ONE_ITEM);
                                (item, quantity)
                            });
                    let umbral = row.umbral_currency.map_or(NO_FACTOR, amount);
                    Some((
                        row.event_id.clone(),
                        ModeReward::of(war, modifiers, umbral, item),
                    ))
                }),
        )
    }

    /// Rewards from `(event id, reward)` pairs.
    pub fn build(rewards: impl IntoIterator<Item = (String, ModeReward)>) -> Self {
        Self {
            by_id: rewards
                .into_iter()
                .map(|(id, reward)| (id.to_lowercase(), reward))
                .collect(),
        }
    }

    /// The reward of event `id` (any case).
    pub fn get(&self, id: &str) -> Option<&ModeReward> {
        self.by_id.get(&id.to_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retail `RewardData` level 60 (content bundle 25810808).
    const LEVEL_60: WarAmounts = WarAmounts {
        currency: 9_000.0,
        experience: 2_400.0,
        pvp_xp: 1.0,
        faction_reputation: 800.0,
        faction_tokens: 800.0,
        azoth: 250.0,
    };
    /// Retail `RewardModifierData` `CaptureTheFlag_T4`, `_T2` and `_T3`.
    const WIN: ModeModifiers = ModeModifiers {
        currency: 2.7778,
        experience: 0.0,
        pvp_xp: 1_350.0,
        faction_reputation: 1.375,
        faction_tokens: 1.6875,
        azoth: 0.5,
    };
    const LOSS: ModeModifiers = ModeModifiers {
        currency: 2.7778,
        experience: 0.0,
        pvp_xp: 800.0,
        faction_reputation: 0.6875,
        faction_tokens: 1.0,
        azoth: 0.3,
    };
    const TIE: ModeModifiers = ModeModifiers {
        currency: 2.7778,
        experience: 1.1459,
        pvp_xp: 1_100.0,
        faction_reputation: 1.0,
        faction_tokens: 1.375,
        azoth: 0.4,
    };

    #[test]
    fn the_war_amounts_times_the_modifiers_give_the_ledgers_numbers() {
        let item = Some(("[LTID]CaptureTheFlagRewardT4".to_owned(), ONE_ITEM));
        let win = ModeReward::of(LEVEL_60, WIN, 150.0, item.clone());
        assert_eq!(win.coins, 25_000, "CTF01 wallet +25,000");
        assert_eq!(win.experience, 0, "no 899 change in CTF01");
        assert!(
            (win.pvp_xp - 1_350.0).abs() < f64::EPSILON,
            "PvP_XP 6,993 -> 8,343"
        );
        assert!((win.faction_tokens - 1_350.0).abs() < f64::EPSILON);
        assert!((win.azoth - 125.0).abs() < f64::EPSILON, "value_f 125");
        assert!((win.umbral - 150.0).abs() < f64::EPSILON, "value_h 150");
        assert_eq!(win.item, item);
        let loss = ModeReward::of(LEVEL_60, LOSS, 40.0, None);
        assert_eq!(loss.coins, 25_000, "CTF03 value_a 25,000");
        assert!(
            (loss.pvp_xp - 800.0).abs() < f64::EPSILON,
            "CTF03 PvP_XP +800"
        );
        assert!((loss.faction_tokens - 800.0).abs() < f64::EPSILON);
        assert!((loss.azoth - 75.0).abs() < f64::EPSILON, "value_f 75");
        let tie = ModeReward::of(LEVEL_60, TIE, 150.0, None);
        assert_eq!(tie.experience, 2_750, "T3 UniversalExpAmount");
    }

    #[test]
    fn rewards_are_found_in_any_case_and_only_for_known_events() {
        let rewards = ModeRewards::build([(
            "CaptureTheFlag_T4".to_owned(),
            ModeReward::of(LEVEL_60, WIN, 150.0, None),
        )]);
        assert_eq!(
            rewards.get("capturetheflag_t4").map(|reward| reward.coins),
            Some(25_000)
        );
        assert!(rewards.get("CaptureTheFlag_T1").is_none());
    }

    #[test]
    #[ignore = "needs the build 25810808 content bundle in NW_CONTENT_DIR"]
    fn the_retail_capture_the_flag_events_give_the_ledgers_numbers() {
        let dir = std::env::var("NW_CONTENT_DIR").expect("NW_CONTENT_DIR");
        let bundle = crate::bundle::ContentBundle::load(
            &crate::bundle::bundle_path(std::path::Path::new(&dir), "25810808"),
            "25810808",
        )
        .expect("bundle");
        let rewards = ModeRewards::from_tables(&bundle.tables);
        let win = rewards.get("CaptureTheFlag_T4").expect("win");
        assert_eq!(win.coins, 25_000);
        assert!((win.faction_tokens - 1_350.0).abs() < f64::EPSILON);
        assert!((win.pvp_xp - 1_350.0).abs() < f64::EPSILON);
        assert_eq!(
            win.item,
            Some(("[LTID]CaptureTheFlagRewardT4".to_owned(), ONE_ITEM))
        );
        let loss = rewards.get("CaptureTheFlag_T2").expect("loss");
        assert_eq!(loss.coins, 25_000);
        assert!((loss.umbral - 40.0).abs() < f64::EPSILON);
        let tie = rewards.get("CaptureTheFlag_T3").expect("tie");
        assert_eq!(tie.experience, 2_750);
    }
}
