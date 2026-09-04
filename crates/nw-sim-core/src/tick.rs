// SPDX-License-Identifier: AGPL-3.0-or-later
//! The fixed 30 Hz tick (parent spec section 4.4, M4 spec section 7.1). Deadlines are computed
//! from the start time, so they do not drift. A late caller runs every due tick; ticks are never
//! skipped.

/// Ticks per second.
pub const TICKS_PER_SECOND: u64 = 30;
const MICROS_PER_SECOND: u64 = 1_000_000;

/// Ticks to run now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DueTicks {
    /// Ticks that are due; 0 before the next deadline.
    pub count: u64,
    /// Microseconds since the deadline of the first due tick; 0 when none is due.
    pub late_us: u64,
}

/// Deadlines of the fixed tick.
#[derive(Debug, Clone)]
pub struct TickClock {
    start_us: u64,
    next_tick: u64,
}

impl TickClock {
    /// A clock whose first tick is due at `start_us`.
    pub const fn new(start_us: u64) -> Self {
        Self {
            start_us,
            next_tick: 0,
        }
    }

    /// The deadline of tick `tick`.
    fn deadline_us(&self, tick: u64) -> u64 {
        self.start_us + tick.saturating_mul(MICROS_PER_SECOND) / TICKS_PER_SECOND
    }

    /// The deadline of the next tick.
    pub fn next_deadline_us(&self) -> u64 {
        self.deadline_us(self.next_tick)
    }

    /// The ticks due at `now_us`; they count as run.
    pub fn due(&mut self, now_us: u64) -> DueTicks {
        let first = self.next_deadline_us();
        if now_us < first {
            return DueTicks {
                count: 0,
                late_us: 0,
            };
        }
        let mut count = 0;
        while self.next_deadline_us() <= now_us {
            self.next_tick += 1;
            count += 1;
        }
        DueTicks {
            count,
            late_us: now_us - first,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: u64 = 5_000_000;
    const ONE_SECOND: u64 = 1_000_000;
    /// Three deadlines (0, 33.3 and 66.7 ms) are past; the fourth (100 ms) is not.
    const LATE: u64 = 99_000;

    #[test]
    fn thirty_ticks_fall_in_each_second_without_drift() {
        let mut clock = TickClock::new(START);
        let mut total = 0;
        for step in 0..=ONE_SECOND / 1000 {
            total += clock.due(START + step * 1000).count;
        }
        assert_eq!(total, TICKS_PER_SECOND + 1);
        assert_eq!(clock.next_deadline_us(), START + ONE_SECOND + 33_333);
    }

    #[test]
    fn a_late_caller_runs_every_due_tick() {
        let mut clock = TickClock::new(START);
        assert_eq!(clock.due(START - 1).count, 0);
        let late = clock.due(START + LATE);
        assert_eq!(late.count, 3);
        assert_eq!(late.late_us, LATE);
        assert_eq!(clock.due(START + LATE + 1).count, 0);
    }
}
