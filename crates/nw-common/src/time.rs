// SPDX-License-Identifier: AGPL-3.0-or-later
//! The wall clock.

use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since 1970-01-01 UTC; 0 for a clock before that.
pub fn unix_ms() -> u64 {
    unix_ms_at(SystemTime::now())
}

/// Seconds since 1970-01-01 UTC; 0 for a clock before that.
pub fn unix_seconds() -> i64 {
    unix_seconds_at(SystemTime::now())
}

fn unix_seconds_at(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |since| {
        i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
    })
}

fn unix_ms_at(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |since| {
        u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const SECOND_MS: u64 = 1000;
    const SECONDS: u64 = 90;
    const SECONDS_I64: i64 = 90;

    #[test]
    fn seconds_count_from_1970_and_earlier_times_are_0() {
        assert_eq!(
            unix_seconds_at(UNIX_EPOCH + Duration::from_secs(SECONDS)),
            SECONDS_I64
        );
        assert_eq!(unix_seconds_at(UNIX_EPOCH - Duration::from_secs(1)), 0);
        assert!(unix_seconds() > 0);
    }

    #[test]
    fn times_count_from_1970_and_earlier_times_are_0() {
        assert_eq!(unix_ms_at(UNIX_EPOCH + Duration::from_secs(1)), SECOND_MS);
        assert_eq!(unix_ms_at(UNIX_EPOCH - Duration::from_secs(1)), 0);
        assert!(unix_ms() > 0);
    }
}
