// SPDX-License-Identifier: AGPL-3.0-or-later
//! The stats of a game mode's war board: the slot of each stat on the wire, which stats the
//! board shows during a match and at its end, what each adds to the board score and which
//! stats sum others.
//!
//! Observed (client build 25810808):
//!
//! - The slot of a stat is its value in the client's enum `WarboardStatType` (`FUN_1442ff370`:
//!   `Score` 0, `TotalDamageDealt` 1, ..., `WeeklyCrownsRemaining` 0x39, `NumStats` 0x3a). The
//!   catacombs board slots 44 to 55 are `BossesKilled` to `NamedEnemiesCrowns` of the same enum.
//! - Each game mode names its `WarboardStatDefinitions` table (`ORWarboardStatDefinitions` for
//!   Outpost Rush, `javelindata_orwarboardaggregates.datasheet`): a row per stat with
//!   `InGameStat` and `EndGameStat`, the score per unit `AG_Score`, the range `BonusDistanceRange`
//!   (m) and its factor `InRangeBonusMultiplier`, and the `AG_*` flags that name the summing stat
//!   a row adds to (`AG_TotalDamageDealt`, `AG_HealingDone`, `AG_AITakedowns`,
//!   `AG_PlayerTakedowns`, `AG_TotalResourcesDeposited`, `AG_Kills`, `AG_DamageTaken`).
//! - The Outpost Rush score is the sum of each stat times its `AG_Score`, rounded down: the end
//!   values of `OPR---Tower` (853.3 for a board score of 853) and `OPR---Tower-2` (570.7 for
//!   570) fit with no range bonus; in the other ten ledgers the board score lies above that sum
//!   by less than the bonus of the in-range stats (`OPR_TEST3` 6,441 against 6,102.6 and at most
//!   975 of bonus). `PlayerTakedowns` (`AG_Score` 50 and its own `AG_PlayerTakedowns`) adds
//!   nothing: with it every ledger with takedowns lies far above its board score (`OPR_TEST3`
//!   by 2,750).
//!
//! Inferred: a stat adds its `AG_Score` once, at its own row; a summing stat takes the amount
//! with no score of its own, and a row that names itself as its sum adds nothing more.

use crate::Tables;
use crate::generated::WarboardStatDefinitions;

/// The war board table of Outpost Rush (its game mode row's `WarBoardStatDefinitions`).
pub const OUTPOST_RUSH_TABLE: &str = "ORWarboardStatDefinitions";
/// The count of stat slots (`NumStats`).
pub const STAT_SLOTS: u8 = 58;

/// The board score.
pub const SCORE: u8 = 0;
/// Every hit's damage (the sum of the `AG_TotalDamageDealt` rows).
pub const TOTAL_DAMAGE_DEALT: u8 = 1;
/// The damage the player took.
pub const DAMAGE_TAKEN: u8 = 2;
/// The kills of any kind.
pub const KILLS: u8 = 3;
/// The healing done (the sum of the `AG_HealingDone` rows).
pub const HEALING_DONE: u8 = 4;
/// The player's deaths.
pub const DEATHS: u8 = 6;
/// The damage to players of the other team.
pub const DAMAGE_TO_PLAYERS: u8 = 7;
/// The damage to the creatures of the match.
pub const DAMAGE_TO_AI: u8 = 8;
/// The damage to anything else (the outpost structures).
pub const DAMAGE_TO_OTHER: u8 = 9;
/// The damage of siege weapons.
pub const DAMAGE_WITH_SIEGE_WEAPONS: u8 = 10;
/// The healing of the player itself.
pub const HEALING_DONE_TO_SELF: u8 = 19;
/// The healing of the player's allies.
pub const HEALING_DONE_TO_ALLIES: u8 = 20;
/// The damage the player's blocks stopped.
pub const DAMAGE_BLOCKED: u8 = 21;
/// The kills of players.
pub const PLAYER_KILLS: u8 = 22;
/// The kills of creatures.
pub const AI_KILLS: u8 = 23;
/// The assists on kills of creatures.
pub const AI_ASSISTS: u8 = 24;
/// The assists on kills of players.
pub const PLAYER_ASSISTS: u8 = 25;
/// The creature kills and assists (the sum of the `AG_AITakedowns` rows).
pub const AI_TAKEDOWNS: u8 = 26;
/// The player kills and assists (the sum of the `AG_PlayerTakedowns` rows).
pub const PLAYER_TAKEDOWNS: u8 = 27;
/// The time spent in contested capture points.
pub const CAPTURE_POINT_CONTESTS: u8 = 28;
/// The infused wood a structure took from the player.
pub const INFUSED_WOOD_DEPOSITED: u8 = 29;
/// The infused ore a structure took from the player.
pub const INFUSED_ORE_DEPOSITED: u8 = 30;
/// The infused hide a structure took from the player.
pub const INFUSED_HIDE_DEPOSITED: u8 = 31;
/// The azoth essence a structure took from the player.
pub const AZOTH_ESSENCE_DEPOSITED: u8 = 32;
/// The infused wood the player gathered.
pub const INFUSED_WOOD_GATHERED: u8 = 33;
/// The infused ore the player gathered.
pub const INFUSED_ORE_GATHERED: u8 = 34;
/// The infused hide the player gathered.
pub const INFUSED_HIDE_GATHERED: u8 = 35;
/// The azoth essence the player gathered.
pub const AZOTH_ESSENCE_GATHERED: u8 = 36;
/// The resources the structures took (the sum of the `AG_TotalResourcesDeposited` rows).
pub const TOTAL_RESOURCES_DEPOSITED: u8 = 37;

/// The named stats of the enum (Observed: 0x2b has no name).
const NAMED_STATS: usize = 57;
/// The client's `WarboardStatType` names and values (`FUN_1442ff370`, in its order).
pub const STAT_TYPES: [(&str, u8); NAMED_STATS] = [
    ("DamageToPlayers", DAMAGE_TO_PLAYERS),
    ("DamageToAI", DAMAGE_TO_AI),
    ("DamageToOther", DAMAGE_TO_OTHER),
    ("DamageWithSiegeWeapons", DAMAGE_WITH_SIEGE_WEAPONS),
    ("DamageFromWeapons", 0x0d),
    ("DamageFromSiegeWeapons", 0x0e),
    ("KillsWithWeapons", 0x0b),
    ("KillsWithSiegeWeapons", 0x0c),
    ("SiegeWeaponsDestroyed", 0x0f),
    ("HealingDoneToSelf", HEALING_DONE_TO_SELF),
    ("HealingDoneToAllies", HEALING_DONE_TO_ALLIES),
    ("HealingDone", HEALING_DONE),
    ("AlliesRevived", 0x11),
    ("RepairsDone", 0x12),
    ("SiegeWeaponReloaded", 0x10),
    ("Deaths", DEATHS),
    ("Assists", 5),
    ("PlayerKills", PLAYER_KILLS),
    ("AIKills", AI_KILLS),
    ("InfusedWoodGathered", INFUSED_WOOD_GATHERED),
    ("InfusedOreGathered", INFUSED_ORE_GATHERED),
    ("InfusedHideGathered", INFUSED_HIDE_GATHERED),
    ("AzothEssenceGathered", AZOTH_ESSENCE_GATHERED),
    ("InfusedWoodDeposited", INFUSED_WOOD_DEPOSITED),
    ("InfusedOreDeposited", INFUSED_ORE_DEPOSITED),
    ("InfusedHideDeposited", INFUSED_HIDE_DEPOSITED),
    ("AzothEssenceDeposited", AZOTH_ESSENCE_DEPOSITED),
    ("AIAssists", AI_ASSISTS),
    ("PlayerAssists", PLAYER_ASSISTS),
    ("TeamFlagCapture", 0x26),
    ("NeutralFlagCapture", 0x27),
    ("FlagCarrierKilled", 0x2a),
    ("FlagReturnedToBase", 0x29),
    ("FlagsTaken", 0x28),
    ("BossesKilled", 0x2c),
    ("NamedEnemiesKilled", 0x2d),
    ("ChestsOpened", 0x2e),
    ("Extracted", 0x2f),
    ("SilversExtracted", 0x30),
    ("CrownsExtracted", 0x31),
    ("ChestsOpenedSilvers", 0x32),
    ("ChestsOpenedCrowns", 0x33),
    ("BossesSilvers", 0x34),
    ("BossesCrowns", 0x35),
    ("NamedEnemiesSilvers", 0x36),
    ("NamedEnemiesCrowns", 0x37),
    ("CrownsDeducted", 0x38),
    ("WeeklyCrownsRemaining", 0x39),
    ("Score", SCORE),
    ("TotalDamageDealt", TOTAL_DAMAGE_DEALT),
    ("Kills", KILLS),
    ("DamageTaken", DAMAGE_TAKEN),
    ("AITakedowns", AI_TAKEDOWNS),
    ("PlayerTakedowns", PLAYER_TAKEDOWNS),
    ("CapturePointContests", CAPTURE_POINT_CONTESTS),
    ("TotalResourcesDeposited", TOTAL_RESOURCES_DEPOSITED),
    ("DamageBlocked", DAMAGE_BLOCKED),
];

/// The slot of stat `name` (any case); `None` for a name the enum lacks.
pub fn stat_slot(name: &str) -> Option<u8> {
    STAT_TYPES
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
        .map(|(_, slot)| *slot)
}

/// The rule of one stat of a board.
#[derive(Debug, Clone, PartialEq)]
pub struct StatRule {
    /// Its slot.
    pub slot: u8,
    /// The board shows it during the match (`InGameStat`).
    pub in_game: bool,
    /// The board shows it at the match's end (`EndGameStat`).
    pub end_game: bool,
    /// The score of one unit (`AG_Score`).
    pub score: f64,
    /// The range of its bonus, m (`BonusDistanceRange`; 0: no bonus).
    pub bonus_range_m: f32,
    /// The factor of its score in range (`InRangeBonusMultiplier`).
    pub in_range_multiplier: f64,
    /// The summing stats it adds to (its `AG_*` flags), itself left out.
    pub sums: Vec<u8>,
}

/// The factor of a stat with no range bonus.
const NO_BONUS: f64 = 1.0;
/// An `AG_*` flag that is set.
const FLAG_SET: i64 = 1;

impl StatRule {
    /// The rule of `row`; `None` for a stat the client enum lacks.
    fn of(row: &WarboardStatDefinitions) -> Option<Self> {
        let slot = stat_slot(&row.warboard_stat_id)?;
        let flagged = [
            (row.ag_total_damage_dealt, TOTAL_DAMAGE_DEALT),
            (row.ag_healing_done, HEALING_DONE),
            (row.ag_ai_takedowns, AI_TAKEDOWNS),
            (row.ag_player_takedowns, PLAYER_TAKEDOWNS),
            (row.ag_total_resources_deposited, TOTAL_RESOURCES_DEPOSITED),
            (row.ag_kills, KILLS),
            (row.ag_damage_taken, DAMAGE_TAKEN),
        ];
        let sums = flagged
            .into_iter()
            .filter(|(flag, sum)| *flag == Some(FLAG_SET) && *sum != slot)
            .map(|(_, sum)| sum)
            .collect();
        // A range of a few metres.
        #[allow(clippy::cast_precision_loss, reason = "a range of a few metres")]
        let bonus_range_m = row.bonus_distance_range.unwrap_or_default().max(0) as f32;
        Some(Self {
            slot,
            in_game: row.in_game_stat,
            end_game: row.end_game_stat,
            score: row.ag_score.unwrap_or_default(),
            bonus_range_m,
            in_range_multiplier: row.in_range_bonus_multiplier.unwrap_or(NO_BONUS),
            sums,
        })
    }

    /// The score of `amount` units, `in_range` of the bonus or not.
    pub fn score_of(&self, amount: u32, in_range: bool) -> f64 {
        let factor = if in_range && self.bonus_range_m > 0.0 {
            self.in_range_multiplier
        } else {
            NO_BONUS
        };
        self.score * f64::from(amount) * factor
    }
}

/// The stat rules of one board, by slot.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BoardStats {
    rules: Vec<StatRule>,
}

impl BoardStats {
    /// The rules of the table `unique_id` of `tables`; empty when the tables lack it.
    pub fn from_tables(tables: &Tables, unique_id: &str) -> Self {
        Self::build(
            tables
                .warboard_stat_definitions
                .iter()
                .filter(|table| table.unique_id.eq_ignore_ascii_case(unique_id))
                .flat_map(|table| &table.rows)
                .filter_map(StatRule::of),
        )
    }

    /// The board of `rules`, in slot order.
    pub fn build(rules: impl IntoIterator<Item = StatRule>) -> Self {
        let mut rules: Vec<StatRule> = rules.into_iter().collect();
        rules.sort_by_key(|rule| rule.slot);
        rules.dedup_by_key(|rule| rule.slot);
        Self { rules }
    }

    /// The rule of `slot`.
    pub fn rule(&self, slot: u8) -> Option<&StatRule> {
        self.rules
            .binary_search_by_key(&slot, |rule| rule.slot)
            .ok()
            .map(|index| &self.rules[index])
    }

    /// The slots the board shows during the match, in slot order.
    pub fn in_game_slots(&self) -> impl Iterator<Item = u8> + '_ {
        self.rules
            .iter()
            .filter(|rule| rule.in_game)
            .map(|rule| rule.slot)
    }

    /// Whether the board shows `slot` at the match's end.
    pub fn shows_at_end(&self, slot: u8) -> bool {
        self.rule(slot).is_some_and(|rule| rule.end_game)
    }

    /// Whether the board has no stat.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, in_game: bool, score: f64, range: i64) -> WarboardStatDefinitions {
        WarboardStatDefinitions {
            ag_ai_takedowns: None,
            ag_damage_taken: None,
            ag_healing_done: None,
            ag_kills: None,
            ag_player_takedowns: None,
            ag_score: Some(score),
            ag_total_damage_dealt: None,
            ag_total_resources_deposited: None,
            bonus_distance_range: Some(range),
            distance_from_any_flag: None,
            end_game_stat: true,
            in_game_stat: in_game,
            in_range_bonus_multiplier: Some(1.25),
            in_range_of_flag_multiplier: None,
            warboard_stat_id: id.to_owned(),
        }
    }

    #[test]
    fn the_enum_names_its_slots_as_the_client() {
        assert_eq!(stat_slot("Score"), Some(SCORE));
        assert_eq!(stat_slot("capturepointcontests"), Some(CAPTURE_POINT_CONTESTS));
        assert_eq!(stat_slot("BossesKilled"), Some(44));
        assert_eq!(stat_slot("WeeklyCrownsRemaining"), Some(0x39));
        assert_eq!(stat_slot("NoSuchStat"), None);
        const UNNAMED: u8 = 0x2b;
        let mut slots: Vec<u8> = STAT_TYPES.iter().map(|(_, slot)| *slot).collect();
        slots.sort_unstable();
        let expected: Vec<u8> = (0..STAT_SLOTS).filter(|slot| *slot != UNNAMED).collect();
        assert_eq!(slots, expected, "every slot once");
    }

    #[test]
    fn a_row_gives_its_slot_its_score_and_its_sums() {
        let mut kills = row("PlayerKills", true, 50.0, 14);
        kills.ag_player_takedowns = Some(1);
        let mut takedowns = row("PlayerTakedowns", false, 50.0, 0);
        takedowns.ag_player_takedowns = Some(1);
        let board = BoardStats::build(
            [kills, takedowns, row("Score", true, 0.0, 0)]
                .iter()
                .filter_map(StatRule::of),
        );
        let rule = board.rule(PLAYER_KILLS).expect("kills");
        assert_eq!(rule.sums, vec![PLAYER_TAKEDOWNS]);
        assert!((rule.score_of(2, false) - 100.0).abs() < f64::EPSILON);
        assert!((rule.score_of(2, true) - 125.0).abs() < f64::EPSILON);
        assert!(board.rule(PLAYER_TAKEDOWNS).expect("takedowns").sums.is_empty());
        assert_eq!(board.in_game_slots().collect::<Vec<_>>(), vec![SCORE, PLAYER_KILLS]);
        assert!(board.shows_at_end(PLAYER_TAKEDOWNS));
        let flat = board.rule(SCORE).expect("score");
        assert!((flat.score_of(3, true) - 0.0).abs() < f64::EPSILON);
    }
}
