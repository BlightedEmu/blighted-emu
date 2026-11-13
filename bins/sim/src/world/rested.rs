// SPDX-License-Identifier: AGPL-3.0-or-later
//! A player's rested experience pool ([`nw_content::rested`], findings
//! `2026-11-13-rested-xp.md`): filled at a join from the database by the time away, spent by
//! each character experience gain, sent in the progression state (899 `restedExp`) and saved
//! with the progression.

use nw_content::progression::Progression;
use nw_content::rested::{pool_after_away, rested_bonus};

/// The pool, the total experience at which the top level starts (the pool is 0 there,
/// `FUN_143cf22e0`), and whether the pool changed since the last bundle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct RestedPool {
    experience: u32,
    top_experience: u32,
    changed: bool,
}

impl RestedPool {
    /// The pool of a player that joins with `experience` and the saved pool `pool` after
    /// `away_ns` nanoseconds away (0 on a handoff, which fills nothing).
    pub(super) fn at_join(
        progression: &Progression,
        experience: u32,
        pool: u32,
        away_ns: u64,
    ) -> Self {
        Self {
            experience: pool_after_away(progression, experience, pool, away_ns),
            top_experience: progression
                .experience_for_level(progression.max_level())
                .unwrap_or(u32::MAX),
            changed: false,
        }
    }

    /// The pool.
    pub(super) const fn experience(&self) -> u32 {
        self.experience
    }

    /// The bonus of a gain of `base` points to a total of `total` points: the share the pool
    /// gives ([`rested_bonus`]), taken from it; a gain that reaches the top level empties it.
    pub(super) fn take_bonus(&mut self, base: u32, total: u32) -> u32 {
        let bonus = rested_bonus(self.experience, base);
        let after = total.saturating_add(base).saturating_add(bonus);
        let left = if after >= self.top_experience {
            0
        } else {
            self.experience - bonus
        };
        if left != self.experience {
            self.experience = left;
            self.changed = true;
        }
        bonus
    }

    /// Whether the pool changed since this was last asked, and forgets it.
    pub(super) fn take_change(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retail `XPToLevel` of levels 0 to 5: level 5 (zero-based) is the top here.
    fn levels() -> Progression {
        const STARTS: [i64; 6] = [0, 150, 480, 1_080, 1_970, 3_070];
        Progression::build((0..).zip(STARTS), [], [])
    }

    /// Long enough away for the full 1.5 bars: 200 hours.
    const LONG_AWAY_NS: u64 = 720_000_000_000_000;
    /// Level 2 at its start, and its full pool: half of 600 + 445.
    const LEVEL_TWO: u32 = 480;
    const FULL_POOL: u32 = 522;

    #[test]
    fn a_join_after_a_long_time_away_fills_the_pool_and_a_handoff_keeps_it() {
        let levels = levels();
        let joined = RestedPool::at_join(&levels, LEVEL_TWO, 0, LONG_AWAY_NS);
        assert_eq!(joined.experience(), FULL_POOL);
        let handed = RestedPool::at_join(&levels, LEVEL_TWO, FULL_POOL, 0);
        assert_eq!(handed.experience(), FULL_POOL);
    }

    #[test]
    fn each_gain_takes_its_size_until_the_pool_is_empty() {
        /// A gain smaller than the pool, then one larger than what is left.
        const SMALL: u32 = 300;
        const LARGE: u32 = 400;
        let levels = levels();
        let mut pool = RestedPool::at_join(&levels, LEVEL_TWO, 0, LONG_AWAY_NS);
        assert_eq!(pool.take_bonus(SMALL, LEVEL_TWO), SMALL);
        assert_eq!(pool.experience(), FULL_POOL - SMALL);
        assert!(pool.take_change());
        assert!(!pool.take_change(), "asked once");
        assert_eq!(pool.take_bonus(LARGE, LEVEL_TWO), FULL_POOL - SMALL);
        assert_eq!(pool.experience(), 0);
        assert!(pool.take_change(), "the gain that empties it sends 0");
        assert_eq!(pool.take_bonus(LARGE, LEVEL_TWO), 0);
        assert!(!pool.take_change(), "an empty pool sends nothing more");
    }

    #[test]
    fn a_gain_that_reaches_the_top_level_empties_the_pool() {
        /// Just under the top level's start (3,070).
        const NEAR_TOP: u32 = 3_000;
        const GAIN: u32 = 50;
        let levels = levels();
        let mut pool = RestedPool::at_join(&levels, LEVEL_TWO, 0, LONG_AWAY_NS);
        assert_eq!(pool.take_bonus(GAIN, NEAR_TOP), GAIN);
        assert_eq!(pool.experience(), 0);
        assert!(pool.take_change());
    }
}
