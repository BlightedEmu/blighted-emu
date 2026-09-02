// SPDX-License-Identifier: AGPL-3.0-or-later
//! Wall-clock time in Unix seconds.

use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds in one second.
pub(crate) const MILLIS_PER_SECOND: u64 = 1000;

/// Seconds since the Unix epoch (0 if the clock is before 1970).
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}
