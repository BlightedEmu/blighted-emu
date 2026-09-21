// SPDX-License-Identifier: AGPL-3.0-or-later
//! `javelin.dynamic-datatable-config`: datasheet cell overrides from the remote config layers,
//! applied to the content tables at load.
//!
//! **Observed** (client parser `FUN_145c669a0`, called by `FUN_145c5f6d0` for this key, build
//! 25810808; its error texts give the format): a line is
//! `DataTableFileName, DataTableType, RowName, ColumnName:CellValue, ColumnName:CellValue, ...`,
//! at least 4 comma-separated entries; the first three hold no `:`; each later one is a column
//! and a cell value split at `:`. A value wrapped in `---` may hold commas
//! (`ColumnName:---CellValue1, CellValue2---` is the value `CellValue1, CellValue2`); the
//! wraps must pair. Retail lines have spaces after the commas
//! (`JavelinData_LootLimits, LootLimitData, UmbralCurrency, CountLimit:100000`), so every entry
//! is trimmed (Inferred).
//!
//! The file name is the datasheet's file name without its folder and extension, in any case
//! (`JavelinData_LootLimits` is `.../javelindata_lootlimits.datasheet`); the row name is the
//! row's key cell (`LootLimitID`, `Rank`, `AfkModeId`). The client skips a line it cannot
//! parse; this server refuses to start instead, so no override is lost without notice. The
//! table types of the retail lines are supported: [`LootLimitData`],
//! [`CategoricalProgressionRankData`] and [`AfkModeData`]; a line for another type is refused.

use std::path::Path;

use crate::Tables;
use crate::generated::{AfkModeData, CategoricalProgressionRankData, LootLimitData};
use crate::table::{FromDatasheet, Table};

/// Separates the entries of a line.
const ENTRY_SEPARATOR: char = ',';
/// Separates a column from its value.
const CELL_SEPARATOR: char = ':';
/// Wraps a value that holds commas.
const WRAP: &str = "---";
/// Entries before the first column: file name, table type, row name.
const HEAD_ENTRIES: usize = 3;
/// The cell texts of a boolean column (any case).
const TRUE_TEXT: &str = "true";
const FALSE_TEXT: &str = "false";

/// One override line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatatableOverride {
    /// `DataTableFileName`.
    pub file: String,
    /// `DataTableType`.
    pub table_type: String,
    /// `RowName`.
    pub row: String,
    /// `ColumnName:CellValue` pairs, in order.
    pub cells: Vec<(String, String)>,
}

/// A line that cannot be parsed or applied.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DatatableOverrideError {
    /// The line does not follow the format.
    #[error("dynamic-datatable-config line {line:?}: {reason}")]
    Format {
        /// The line.
        line: String,
        /// What is wrong.
        reason: &'static str,
    },
    /// The table type is not one this server overrides.
    #[error("dynamic-datatable-config: table type {0} is not supported")]
    UnsupportedType(String),
    /// No table of that type has that file name.
    #[error("dynamic-datatable-config: no {table_type} table named {file}")]
    NoTable {
        /// The file name.
        file: String,
        /// The table type.
        table_type: String,
    },
    /// The table has no row with that name.
    #[error("dynamic-datatable-config: {file} has no row {row}")]
    NoRow {
        /// The file name.
        file: String,
        /// The row name.
        row: String,
    },
    /// The column is not one of the table type, or the value does not fit it.
    #[error("dynamic-datatable-config: {file} row {row}: {column}:{value} {reason}")]
    Cell {
        /// The file name.
        file: String,
        /// The row name.
        row: String,
        /// The column.
        column: String,
        /// The value.
        value: String,
        /// What is wrong.
        reason: &'static str,
    },
}

impl DatatableOverride {
    /// Parses one line.
    pub fn parse(line: &str) -> Result<Self, DatatableOverrideError> {
        let format = |reason| DatatableOverrideError::Format {
            line: line.to_owned(),
            reason,
        };
        let mut entries: Vec<String> = Vec::new();
        let mut open: Option<String> = None;
        for piece in line.split(ENTRY_SEPARATOR) {
            let joined = match open.take() {
                Some(mut start) => {
                    start.push(ENTRY_SEPARATOR);
                    start.push_str(piece);
                    start
                }
                None => piece.to_owned(),
            };
            if joined.matches(WRAP).count() % 2 == 0 {
                entries.push(joined.trim().to_owned());
            } else {
                open = Some(joined);
            }
        }
        if open.is_some() {
            return Err(format("has an unpaired '---'"));
        }
        if entries.len() <= HEAD_ENTRIES {
            return Err(format("needs at least 4 entries"));
        }
        let cells_entries = entries.split_off(HEAD_ENTRIES);
        if entries.iter().any(|entry| entry.contains(CELL_SEPARATOR)) {
            return Err(format("has ':' in one of its first three entries"));
        }
        let cells = cells_entries
            .iter()
            .map(|entry| {
                let (column, value) = entry
                    .split_once(CELL_SEPARATOR)
                    .ok_or_else(|| format("has an entry after the third without ':'"))?;
                let value = value.trim();
                let value = value
                    .strip_prefix(WRAP)
                    .and_then(|inner| inner.strip_suffix(WRAP))
                    .unwrap_or(value);
                Ok((column.trim().to_owned(), value.trim().to_owned()))
            })
            .collect::<Result<_, _>>()?;
        let [file, table_type, row]: [String; HEAD_ENTRIES] = entries
            .try_into()
            .map_err(|_| format("needs at least 4 entries"))?;
        Ok(Self {
            file,
            table_type,
            row,
            cells,
        })
    }
}

/// Applies every override to `tables`, in order.
pub fn apply(
    tables: &mut Tables,
    overrides: &[DatatableOverride],
) -> Result<(), DatatableOverrideError> {
    for line in overrides {
        match line.table_type.as_str() {
            LootLimitData::TYPE_NAME => apply_to(&mut tables.loot_limit_data, line),
            CategoricalProgressionRankData::TYPE_NAME => {
                apply_to(&mut tables.categorical_progression_rank_data, line)
            }
            AfkModeData::TYPE_NAME => apply_to(&mut tables.afk_mode_data, line),
            other => Err(DatatableOverrideError::UnsupportedType(other.to_owned())),
        }?;
    }
    Ok(())
}

fn apply_to<T: OverrideRow>(
    tables: &mut [Table<T>],
    line: &DatatableOverride,
) -> Result<(), DatatableOverrideError> {
    let table = tables
        .iter_mut()
        .find(|table| {
            Path::new(&table.source_path)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| stem.eq_ignore_ascii_case(&line.file))
        })
        .ok_or_else(|| DatatableOverrideError::NoTable {
            file: line.file.clone(),
            table_type: line.table_type.clone(),
        })?;
    let row = table
        .rows
        .iter_mut()
        .find(|row| row.key().eq_ignore_ascii_case(&line.row))
        .ok_or_else(|| DatatableOverrideError::NoRow {
            file: line.file.clone(),
            row: line.row.clone(),
        })?;
    for (column, value) in &line.cells {
        row.set(column, value)
            .map_err(|reason| DatatableOverrideError::Cell {
                file: line.file.clone(),
                row: line.row.clone(),
                column: column.clone(),
                value: value.clone(),
                reason,
            })?;
    }
    Ok(())
}

/// A row type whose cells an override can set.
trait OverrideRow {
    /// The row's key cell.
    fn key(&self) -> String;
    /// Sets column `column` to the cell text `value`.
    fn set(&mut self, column: &str, value: &str) -> Result<(), &'static str>;
}

const NOT_A_COLUMN: &str = "is not a column of this table type";

fn whole(value: &str) -> Result<i64, &'static str> {
    value.parse().map_err(|_| "is not a whole number")
}

fn optional_whole(value: &str) -> Result<Option<i64>, &'static str> {
    (!value.is_empty()).then(|| whole(value)).transpose()
}

fn decimal(value: &str) -> Result<f64, &'static str> {
    value.parse().map_err(|_| "is not a number")
}

fn optional_decimal(value: &str) -> Result<Option<f64>, &'static str> {
    (!value.is_empty()).then(|| decimal(value)).transpose()
}

fn boolean(value: &str) -> Result<bool, &'static str> {
    if value.eq_ignore_ascii_case(TRUE_TEXT) {
        Ok(true)
    } else if value.eq_ignore_ascii_case(FALSE_TEXT) {
        Ok(false)
    } else {
        Err("is not true or false")
    }
}

fn optional_boolean(value: &str) -> Result<Option<bool>, &'static str> {
    (!value.is_empty()).then(|| boolean(value)).transpose()
}

fn optional_text(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

impl OverrideRow for LootLimitData {
    fn key(&self) -> String {
        self.loot_limit_id.clone()
    }

    fn set(&mut self, column: &str, value: &str) -> Result<(), &'static str> {
        match column {
            "AchievementAtLimit" => self.achievement_at_limit = optional_text(value),
            "CountLimit" => self.count_limit = whole(value)?,
            "IsAchievementBound" => self.is_achievement_bound = optional_boolean(value)?,
            "IsReplicated" => self.is_replicated = boolean(value)?,
            "LimitExpireSeconds" => self.limit_expire_seconds = whole(value)?,
            "LimitNotificationLocTag" => self.limit_notification_loc_tag = optional_text(value),
            "LootLimitID" => self.loot_limit_id = value.to_owned(),
            "LootTagValueOverrideAtLimit" => {
                self.loot_tag_value_override_at_limit = optional_whole(value)?;
            }
            "LootTagValuePerCount" => self.loot_tag_value_per_count = optional_whole(value)?,
            "MaxLimitMult" => self.max_limit_mult = whole(value)?,
            "MaxLimitSeconds" => self.max_limit_seconds = whole(value)?,
            "MinLimitMult" => self.min_limit_mult = whole(value)?,
            "MinLimitSeconds" => self.min_limit_seconds = whole(value)?,
            "PityTickerOddsMod" => self.pity_ticker_odds_mod = optional_decimal(value)?,
            _ => return Err(NOT_A_COLUMN),
        }
        Ok(())
    }
}

impl OverrideRow for CategoricalProgressionRankData {
    fn key(&self) -> String {
        self.rank.to_string()
    }

    fn set(&mut self, column: &str, value: &str) -> Result<(), &'static str> {
        match column {
            "AdditionalProgressionPoints" => {
                self.additional_progression_points = optional_whole(value)?;
            }
            "AvailableProgressionPoint1" => {
                self.available_progression_point1 = optional_text(value)
            }
            "AvailableProgressionPoint2" => {
                self.available_progression_point2 = optional_text(value)
            }
            "AvailableProgressionPoint3" => {
                self.available_progression_point3 = optional_text(value)
            }
            "AvailableProgressionPoint4" => {
                self.available_progression_point4 = optional_text(value)
            }
            "AzothCost" => self.azoth_cost = optional_whole(value)?,
            "AzothRespecCost" => self.azoth_respec_cost = optional_whole(value)?,
            "CurrencyCost" => self.currency_cost = optional_whole(value)?,
            "DisplayName" => self.display_name = optional_text(value),
            "EntitlementId" => self.entitlement_id = optional_whole(value)?,
            "GameEventId" => self.game_event_id = optional_text(value),
            "InfluenceCost" => self.influence_cost = optional_whole(value)?,
            "MaxMissionCountPVE" => self.max_mission_count_pve = optional_whole(value)?,
            "MaxMissionCountPvP" => self.max_mission_count_pv_p = optional_whole(value)?,
            "MaximumInfluence" => self.maximum_influence = whole(value)?,
            "MinMissionCountPVE" => self.min_mission_count_pve = optional_whole(value)?,
            "MinMissionCountPvP" => self.min_mission_count_pv_p = optional_whole(value)?,
            "Rank" => self.rank = whole(value)?,
            "RequiredLevel" => self.required_level = optional_whole(value)?,
            "SanityCheck" => self.sanity_check = optional_boolean(value)?,
            "XpReward" => self.xp_reward = optional_whole(value)?,
            _ => return Err(NOT_A_COLUMN),
        }
        Ok(())
    }
}

impl OverrideRow for AfkModeData {
    fn key(&self) -> String {
        self.afk_mode_id.clone()
    }

    fn set(&mut self, column: &str, value: &str) -> Result<(), &'static str> {
        match column {
            "AfkModeId" => self.afk_mode_id = value.to_owned(),
            "DisconnectSeconds" => self.disconnect_seconds = whole(value)?,
            "ResetMinMove" => self.reset_min_move = decimal(value)?,
            "ScaleAfkTimerAmount" => self.scale_afk_timer_amount = whole(value)?,
            "UpdatePlayerPopulationCheckDelaySeconds" => {
                self.update_player_population_check_delay_seconds = whole(value)?;
            }
            "WarnSeconds" => self.warn_seconds = whole(value)?,
            _ => return Err(NOT_A_COLUMN),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOOT_LIMITS: &str = "datatables/javelindata_lootlimits.datasheet";
    const UMBRAL_RANKS: &str = "datatables/javelindata_umbralcurrency.datasheet";
    const AFK_MODES: &str = "datatables/javelindata_afkmodes.datasheet";

    fn limit(id: &str, count_limit: i64) -> LootLimitData {
        LootLimitData {
            achievement_at_limit: None,
            count_limit,
            is_achievement_bound: None,
            is_replicated: false,
            limit_expire_seconds: 64_800,
            limit_notification_loc_tag: None,
            loot_limit_id: id.to_owned(),
            loot_tag_value_override_at_limit: None,
            loot_tag_value_per_count: None,
            max_limit_mult: 64_800,
            max_limit_seconds: 0,
            min_limit_mult: 64_800,
            min_limit_seconds: 0,
            pity_ticker_odds_mod: None,
        }
    }

    fn table<T>(source_path: &str, rows: Vec<T>) -> Table<T> {
        Table {
            source_path: source_path.to_owned(),
            unique_id: String::new(),
            rows,
        }
    }

    /// The retail tables (content bundle 25810808) of the retail override lines.
    fn tables() -> Tables {
        Tables {
            loot_limit_data: vec![table(
                LOOT_LIMITS,
                vec![limit("GypsumGreen", 2), limit("UmbralCurrency", 10_000)],
            )],
            categorical_progression_rank_data: vec![table(
                UMBRAL_RANKS,
                vec![CategoricalProgressionRankData {
                    additional_progression_points: None,
                    available_progression_point1: None,
                    available_progression_point2: None,
                    available_progression_point3: None,
                    available_progression_point4: None,
                    azoth_cost: None,
                    azoth_respec_cost: None,
                    currency_cost: Some(0),
                    display_name: Some("@ui_umbral_currency_name".to_owned()),
                    entitlement_id: None,
                    game_event_id: None,
                    influence_cost: Some(0),
                    max_mission_count_pve: None,
                    max_mission_count_pv_p: None,
                    maximum_influence: 10_000,
                    min_mission_count_pve: None,
                    min_mission_count_pv_p: None,
                    rank: 0,
                    required_level: Some(0),
                    sanity_check: None,
                    xp_reward: None,
                }],
            )],
            afk_mode_data: vec![table(
                AFK_MODES,
                vec![AfkModeData {
                    afk_mode_id: "Default".to_owned(),
                    disconnect_seconds: 86_400,
                    reset_min_move: 0.01,
                    scale_afk_timer_amount: 1,
                    update_player_population_check_delay_seconds: 10,
                    warn_seconds: 82_800,
                }],
            )],
            ..Tables::default()
        }
    }

    fn parsed(lines: &[&str]) -> Vec<DatatableOverride> {
        lines
            .iter()
            .map(|line| DatatableOverride::parse(line).expect("line"))
            .collect()
    }

    #[test]
    fn the_retail_lines_change_their_cells() {
        let mut tables = tables();
        let lines = parsed(&[
            "JavelinData_LootLimits,LootLimitData,GypsumGreen,CountLimit:30",
            "JavelinData_LootLimits, LootLimitData, UmbralCurrency, CountLimit:100000",
            "JavelinData_UmbralCurrency, CategoricalProgressionRankData, 0, MaximumInfluence:100000",
            "JavelinData_AfkModes, AfkModeData, Default, WarnSeconds:82800, DisconnectSeconds:86400",
        ]);
        apply(&mut tables, &lines).expect("applied");
        let limits = &tables.loot_limit_data[0].rows;
        assert_eq!(
            (limits[0].count_limit, limits[1].count_limit),
            (30, 100_000)
        );
        assert_eq!(
            tables.categorical_progression_rank_data[0].rows[0].maximum_influence,
            100_000
        );
        let afk = &tables.afk_mode_data[0].rows[0];
        assert_eq!((afk.warn_seconds, afk.disconnect_seconds), (82_800, 86_400));
    }

    #[test]
    fn a_wrapped_value_keeps_its_commas() {
        let line = DatatableOverride::parse(
            "JavelinData_LootLimits, LootLimitData, GypsumGreen, LimitNotificationLocTag:---a, b---, CountLimit:4",
        )
        .expect("line");
        assert_eq!(
            line.cells,
            [
                ("LimitNotificationLocTag".to_owned(), "a, b".to_owned()),
                ("CountLimit".to_owned(), "4".to_owned())
            ]
        );
        let mut tables = tables();
        apply(&mut tables, &[line]).expect("applied");
        let row = &tables.loot_limit_data[0].rows[0];
        assert_eq!(row.limit_notification_loc_tag.as_deref(), Some("a, b"));
        assert_eq!(row.count_limit, 4);
    }

    #[test]
    fn a_line_out_of_the_format_is_refused() {
        let reason = |line: &str| match DatatableOverride::parse(line) {
            Err(DatatableOverrideError::Format { reason, .. }) => reason,
            other => panic!("not refused: {other:?}"),
        };
        assert_eq!(
            reason("JavelinData_LootLimits,LootLimitData,GypsumGreen"),
            "needs at least 4 entries"
        );
        assert_eq!(
            reason("JavelinData_LootLimits,LootLimitData,Gypsum:Green,CountLimit:3"),
            "has ':' in one of its first three entries"
        );
        assert_eq!(
            reason("JavelinData_LootLimits,LootLimitData,GypsumGreen,CountLimit"),
            "has an entry after the third without ':'"
        );
        assert_eq!(
            reason("JavelinData_LootLimits,LootLimitData,GypsumGreen,Tag:---a, b"),
            "has an unpaired '---'"
        );
    }

    #[test]
    fn a_line_this_server_cannot_apply_is_refused() {
        let refused = |line: &str| {
            let mut tables = tables();
            apply(&mut tables, &parsed(&[line])).expect_err("refused")
        };
        assert_eq!(
            refused("JavelinData_Gatherables,GatherableData,Oak,MinRespawnRate:1"),
            DatatableOverrideError::UnsupportedType("GatherableData".to_owned())
        );
        assert!(matches!(
            refused("JavelinData_Other,LootLimitData,GypsumGreen,CountLimit:1"),
            DatatableOverrideError::NoTable { .. }
        ));
        assert!(matches!(
            refused("JavelinData_LootLimits,LootLimitData,GypsumNone,CountLimit:1"),
            DatatableOverrideError::NoRow { .. }
        ));
        assert!(matches!(
            refused("JavelinData_LootLimits,LootLimitData,GypsumGreen,Colour:1"),
            DatatableOverrideError::Cell {
                reason: NOT_A_COLUMN,
                ..
            }
        ));
        assert!(matches!(
            refused("JavelinData_LootLimits,LootLimitData,GypsumGreen,CountLimit:many"),
            DatatableOverrideError::Cell { .. }
        ));
        assert!(matches!(
            refused("JavelinData_LootLimits,LootLimitData,GypsumGreen,IsReplicated:maybe"),
            DatatableOverrideError::Cell { .. }
        ));
    }
}
