// SPDX-License-Identifier: AGPL-3.0-or-later
//! How long a player may not open a loot container (a chest) again (craft2 report, run 15).
//!
//! **Observed.** Retail changes no chest state when a player opens one: the player's own loot
//! tracker (982) gets an entry for the chest, looted (state 2), loot type 0 (`POILoot`), with a
//! reset time, at the gather's end. While the entry lives a new gather of that chest is refused
//! (3743 kind 14 status 2). The reset time:
//! - an elite chest (`GatherableData.IsElite`): the world's next daily reset, 03:00 UTC (94 of
//!   96 entries in 24 ledgers; the source of the hour is Unknown, no data or remote config key
//!   holds it);
//! - a container with a respawn rate: the open time plus `MaxRespawnRate`;
//! - another container: the open time plus 3,600 s (638 of 638 entries; no data column holds
//!   it).
//!
//! An elite chest's `MaxRespawnRate` (82,800 s) is not its lockout.

use std::collections::HashMap;

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;

/// Milliseconds of a day.
const DAY_MS: u64 = 86_400_000;
/// The hour (UTC) of the world's daily reset (Observed, 94 of 96 elite chest entries).
pub const DAILY_RESET_HOUR_UTC: u64 = 3;
/// Milliseconds of an hour.
const HOUR_MS: u64 = 3_600_000;
/// The lockout of a container without a respawn rate (Observed, 638 of 638 entries).
pub const CONTAINER_LOCKOUT_MS: u64 = 3_600_000;
/// Milliseconds per second.
const MS_PER_SECOND: f64 = 1_000.0;

/// How long one container stays locked for the player who opened it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lockout {
    /// Until the world's next daily reset.
    DailyReset,
    /// For this many milliseconds.
    For(u64),
}

impl Lockout {
    /// The unix time (ms) the lockout of a container opened at unix time `opened_ms` ends.
    pub fn ends_at(self, opened_ms: u64) -> u64 {
        match self {
            Self::DailyReset => {
                let reset = DAILY_RESET_HOUR_UTC * HOUR_MS;
                let since = opened_ms.saturating_sub(reset);
                (since / DAY_MS + 1) * DAY_MS + reset
            }
            Self::For(duration_ms) => opened_ms.saturating_add(duration_ms),
        }
    }
}

/// The lockout of every loot container gatherable, by the CRC-32 of its lowercase id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChestLockouts {
    by_crc: HashMap<u32, Lockout>,
}

/// One `GatherableData` row as the lockouts read it.
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerRow<'a> {
    /// `GatherableID`.
    pub id: &'a str,
    /// `IsLootContainer`.
    pub container: bool,
    /// `IsElite`.
    pub elite: bool,
    /// `MaxRespawnRate`, in seconds.
    pub max_respawn_s: f64,
}

impl ChestLockouts {
    /// The lockouts of the loot containers of `GatherableData` in `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(
            tables
                .gatherable_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| ContainerRow {
                    id: &row.gatherable_id,
                    container: row.is_loot_container.unwrap_or_default(),
                    elite: row.is_elite.unwrap_or_default(),
                    max_respawn_s: row.max_respawn_rate,
                }),
        )
    }

    /// The lockouts of `rows`; a row that is no loot container has none, the first row of an
    /// id wins.
    pub fn build<'a>(rows: impl IntoIterator<Item = ContainerRow<'a>>) -> Self {
        let mut by_crc = HashMap::new();
        for row in rows.into_iter().filter(|row| row.container) {
            let lockout = if row.elite {
                Lockout::DailyReset
            } else if row.max_respawn_s > 0.0 {
                // Respawn rates of hours at most.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "positive")]
                let duration_ms = (row.max_respawn_s * MS_PER_SECOND).round() as u64;
                Lockout::For(duration_ms)
            } else {
                Lockout::For(CONTAINER_LOCKOUT_MS)
            };
            by_crc.entry(lowercase_crc32(row.id)).or_insert(lockout);
        }
        Self { by_crc }
    }

    /// The lockout of the gatherable whose wire id is `crc`; `None` for one that is no loot
    /// container.
    pub fn of(&self, crc: u32) -> Option<Lockout> {
        self.by_crc.get(&crc).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-24 00:00:00 UTC in unix ms.
    const MIDNIGHT: u64 = 1_792_800_000_000;

    #[test]
    fn an_elite_chest_locks_until_the_next_daily_reset() {
        let reset = MIDNIGHT + DAILY_RESET_HOUR_UTC * HOUR_MS;
        assert_eq!(Lockout::DailyReset.ends_at(MIDNIGHT), reset);
        assert_eq!(Lockout::DailyReset.ends_at(reset - 1), reset);
        assert_eq!(Lockout::DailyReset.ends_at(reset), reset + DAY_MS);
        assert_eq!(Lockout::For(CONTAINER_LOCKOUT_MS).ends_at(MIDNIGHT), MIDNIGHT + HOUR_MS);
    }

    #[test]
    fn each_container_kind_has_its_lockout() {
        let row = |id, container, elite, max_respawn_s| ContainerRow {
            id,
            container,
            elite,
            max_respawn_s,
        };
        let lockouts = ChestLockouts::build([
            row("EliteChest", true, true, 82_800.0),
            row("Catacombs_LevelUp", true, false, 600.0),
            row("SupplyCrate", true, false, 0.0),
            row("IronVeinSmall", false, false, 900.0),
        ]);
        assert_eq!(
            lockouts.of(lowercase_crc32("elitechest")),
            Some(Lockout::DailyReset)
        );
        assert_eq!(
            lockouts.of(lowercase_crc32("catacombs_levelup")),
            Some(Lockout::For(600_000))
        );
        assert_eq!(
            lockouts.of(lowercase_crc32("supplycrate")),
            Some(Lockout::For(CONTAINER_LOCKOUT_MS))
        );
        assert_eq!(lockouts.of(lowercase_crc32("ironveinsmall")), None);
    }
}
