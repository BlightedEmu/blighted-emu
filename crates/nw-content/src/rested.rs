// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rested experience pool (findings `2026-11-13-rested-xp.md`).
//!
//! **Values (Observed, `sharedassets/genericassets/playerbaseattributes.pbadb`, fields keyed by
//! the CRC-32 of their lowercase display name):** "Rested Exp Percentage Per Hour" 0.01, "Rested
//! Exp Max Percentage" 1.5, "Rested Exp Modifier" 1.0, "Rested Exp Threshold Hours" 8. The
//! client reads them from its global settings (`FUN_142783ba0`, getters 0x230, 0x238, 0x240 and
//! 0x248, in the field order).
//!
//! **Fill (Observed, `FUN_143ce8dc0`, handler of
//! `ProgressionComponentServerFacet_ComputeRestedExp`):** below the top level, after at least
//! 8 whole hours away, the hours past the threshold times 0.01 give level bars (at most 1.5);
//! their experience from the player's place in its level ([`experience_of_bars`]) becomes pool
//! at `modifier / (modifier + 1)` of it (half), and the pool stays at most the pool of 1.5 bars.
//! The pool holds the bonus only, so the bar it fills is twice the pool (HUD `FUN_143cfe8f0`).
//! Retail `Playthrough_Part_02` -> `Part_03`: 2.45 h away, the pool stays 0; `Part_03` ->
//! `Part_04`: 18.5 h after the last record, the login pool is 1,261 (the formula gives it for
//! 18.27 h; the log out came after the capture ended).
//!
//! **Spend (Observed, `FUN_143cf5ad0` and `FUN_143cf1b20`):** a gain of `base` takes
//! `min(pool, modifier x base)` from the pool and adds it to the gain. Retail `Part_02`: each
//! gain while the pool lasts is doubled (+116 = 58 + 58, pool -58); at 898.2 s the pool of 274
//! gives +588 = 314 + 274 and is 0. At the top level the pool is 0 (`FUN_143cf22e0`).
//!
//! **Which gains (Observed):** a game event spends the pool unless its `UseRestedExp` is 0; the
//! only rows with 0 give no experience, and `Chart_Location` (empty) is doubled in `Part_02`
//! (499.7 s +672 = 336 + 336). So every character experience gain spends it.
//!
//! **When the server fills it (Inferred):** retail never captured message 1047 (the catalog
//! wants it), so the time it measures from is Unknown; the server counts from the save of the
//! pool, which the log out writes.

use crate::progression::Progression;

/// "Rested Exp Percentage Per Hour": level bars per hour away past the threshold.
pub const RESTED_BARS_PER_HOUR: f32 = 0.01;
/// "Rested Exp Max Percentage": the most level bars a fill reaches.
pub const RESTED_MAX_BARS: f32 = 1.5;
/// "Rested Exp Modifier": the bonus per point of a gain while the pool lasts.
pub const RESTED_MODIFIER: f32 = 1.0;
/// "Rested Exp Threshold Hours": the whole hours away before the pool fills.
pub const RESTED_THRESHOLD_HOURS: u64 = 8;
/// Nanoseconds per hour (the client's `FUN_1417a02f0` and `FUN_1416b4410`).
const NS_PER_HOUR: u64 = 3_600_000_000_000;
/// Hours per nanosecond, as the client multiplies (`DAT_147f516c8`, `FUN_1417a0310`).
const HOURS_PER_NS: f64 = 2.777_777_777_777_778e-13;
/// The smallest share the client counts (`DAT_147f41070`, `FLT_EPSILON`).
const SMALLEST_SHARE: f32 = f32::EPSILON;
/// One level bar.
const ONE_BAR: f32 = 1.0;

/// The experience of `bars` level bars from the place of a player with `experience` in its
/// level (`FUN_143cedb60`): each level's part of the bars times its experience, added and cut
/// to a whole number level by level, in 32-bit floats; nothing past the top level.
pub fn experience_of_bars(progression: &Progression, experience: u32, bars: f32) -> u32 {
    let level = progression.level_of(experience);
    let top = progression.max_level();
    let start = progression.experience_for_level(level).unwrap_or_default();
    let next = progression
        .experience_for_level(level.saturating_add(1))
        .unwrap_or(start);
    // The completion of the level (`+0x170`, `FUN_143cf5ad0`).
    let completion = if next > start {
        (experience.saturating_sub(start)) as f32 / (next - start) as f32
    } else {
        0.0
    };
    let mut place = level as f32 + completion;
    let mut left = bars;
    let mut total: i64 = 0;
    while left > SMALLEST_SHARE && place < top as f32 {
        let next_level = (place + ONE_BAR).floor();
        let bar = progression
            .experience_for_level(next_level as u32)
            .unwrap_or_default()
            .saturating_sub(
                progression
                    .experience_for_level(place as u32)
                    .unwrap_or_default(),
            );
        let part = (next_level - place).min(left);
        total = (bar as f32 * part + total as f32) as i64;
        left -= part;
        place = next_level;
    }
    u32::try_from(total).unwrap_or(u32::MAX)
}

/// The pool `bars` level bars give a player with `experience` (`FUN_143cf1a90`): the share
/// `modifier / (modifier + 1)` of their experience.
pub fn pool_of_bars(progression: &Progression, experience: u32, bars: f32) -> u32 {
    if RESTED_MODIFIER <= SMALLEST_SHARE {
        return 0;
    }
    let total = experience_of_bars(progression, experience, bars);
    ((total as f32 / (RESTED_MODIFIER + ONE_BAR)) * RESTED_MODIFIER) as u32
}

/// The pool of a player with `experience` and pool `pool` after `away_ns` nanoseconds away
/// (`FUN_143ce8dc0`); at the top level it is 0 (`FUN_143cf22e0`).
pub fn pool_after_away(progression: &Progression, experience: u32, pool: u32, away_ns: u64) -> u32 {
    if progression.level_of(experience) >= progression.max_level() {
        return 0;
    }
    if away_ns / NS_PER_HOUR < RESTED_THRESHOLD_HOURS {
        return pool;
    }
    let hours = away_ns.saturating_sub(RESTED_THRESHOLD_HOURS * NS_PER_HOUR) as f64 * HOURS_PER_NS;
    let bars = ((hours * f64::from(RESTED_BARS_PER_HOUR)) as f32).min(RESTED_MAX_BARS);
    let gained = pool_of_bars(progression, experience, bars);
    if gained == 0 {
        return pool;
    }
    pool.saturating_add(gained)
        .min(pool_of_bars(progression, experience, RESTED_MAX_BARS))
}

/// The bonus a gain of `base` points takes from pool `pool` (`FUN_143cf1b20`): the modifier
/// times the gain, at most the pool.
pub fn rested_bonus(pool: u32, base: u32) -> u32 {
    ((RESTED_MODIFIER * base as f32) as u32).min(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retail `XPToLevel` of levels 0 to 5 and 37 to 41 (zero-based), filled in between so the
    /// table stays rising, and the top level 69.
    fn retail_levels() -> Progression {
        const KNOWN: [(i64, i64); 11] = [
            (0, 0),
            (1, 150),
            (2, 480),
            (3, 1_080),
            (4, 1_970),
            (5, 3_070),
            (37, 388_400),
            (38, 412_290),
            (39, 436_850),
            (40, 462_050),
            (69, 2_718_521),
        ];
        let mut levels: Vec<(i64, i64)> = Vec::new();
        for pair in KNOWN.windows(2) {
            let ((from, start), (to, end)) = (pair[0], pair[1]);
            for level in from..to {
                levels.push((level, start + (end - start) * (level - from) / (to - from)));
            }
        }
        levels.push(KNOWN[KNOWN.len() - 1]);
        Progression::build(levels, [], [])
    }

    /// The retail `Playthrough_Part_04` login: level 38 at 425,671 experience.
    const PART_04_EXPERIENCE: u32 = 425_671;
    /// Its login pool.
    const PART_04_POOL: u32 = 1_261;
    /// The time away that gives it: 18.27 hours.
    const PART_04_AWAY_NS: u64 = 65_772_000_000_000;

    #[test]
    fn a_long_time_away_fills_half_the_experience_of_its_bars() {
        let levels = retail_levels();
        assert_eq!(
            pool_after_away(&levels, PART_04_EXPERIENCE, 0, PART_04_AWAY_NS),
            PART_04_POOL
        );
    }

    #[test]
    fn less_than_the_threshold_away_keeps_the_pool() {
        /// Retail `Part_02` -> `Part_03`: 2.45 hours away.
        const SHORT_AWAY_NS: u64 = 8_820_000_000_000;
        /// Seven hours and 59 minutes: still under 8 whole hours.
        const UNDER_THRESHOLD_NS: u64 = 28_740_000_000_000;
        let levels = retail_levels();
        assert_eq!(
            pool_after_away(&levels, PART_04_EXPERIENCE, 0, SHORT_AWAY_NS),
            0
        );
        assert_eq!(
            pool_after_away(
                &levels,
                PART_04_EXPERIENCE,
                PART_04_POOL,
                UNDER_THRESHOLD_NS
            ),
            PART_04_POOL
        );
    }

    #[test]
    fn the_pool_stops_at_one_and_a_half_bars() {
        /// Long enough for more than 1.5 bars: 200 hours.
        const LONG_AWAY_NS: u64 = 720_000_000_000_000;
        /// Level 2 at its start: its bar is 600, the next one 890.
        const LEVEL_TWO: u32 = 480;
        /// 1.5 bars from there: 600 + 445 = 1045, half of it.
        const FULL_POOL: u32 = 522;
        let levels = retail_levels();
        assert_eq!(pool_of_bars(&levels, LEVEL_TWO, RESTED_MAX_BARS), FULL_POOL);
        assert_eq!(
            pool_after_away(&levels, LEVEL_TWO, 0, LONG_AWAY_NS),
            FULL_POOL
        );
        assert_eq!(
            pool_after_away(&levels, LEVEL_TWO, FULL_POOL, LONG_AWAY_NS),
            FULL_POOL,
            "a full pool fills no further"
        );
    }

    #[test]
    fn the_bars_count_from_the_place_in_the_level_and_stop_at_the_top() {
        /// Half way into level 0 (bar 150): 75 of it, then a quarter of level 1 (bar 330).
        const HALF_LEVEL_ZERO: u32 = 75;
        const BARS: f32 = 0.75;
        const EXPECTED: u32 = 75 + 82;
        /// The top level's start.
        const TOP: u32 = 2_718_521;
        let levels = retail_levels();
        assert_eq!(experience_of_bars(&levels, HALF_LEVEL_ZERO, BARS), EXPECTED);
        assert_eq!(experience_of_bars(&levels, TOP, BARS), 0);
        assert_eq!(
            pool_after_away(&levels, TOP, PART_04_POOL, PART_04_AWAY_NS),
            0
        );
    }

    #[test]
    fn a_gain_takes_its_own_size_from_the_pool_at_most_what_is_left() {
        /// Retail `Part_02`: a level 17 kill of 58 while the pool lasts.
        const KILL: u32 = 58;
        /// Retail `Part_02` 898.2 s: a gain of 314 with 274 left.
        const GAIN: u32 = 314;
        const LEFT: u32 = 274;
        assert_eq!(rested_bonus(PART_04_POOL, KILL), KILL);
        assert_eq!(rested_bonus(LEFT, GAIN), LEFT);
        assert_eq!(rested_bonus(0, GAIN), 0);
    }
}
