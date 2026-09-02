// SPDX-License-Identifier: AGPL-3.0-or-later
//! Wall-clock time in Unix seconds, and the date text of the game replies.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Milliseconds in one second.
pub(crate) const MILLIS_PER_SECOND: u64 = 1000;
/// 9999-12-31 23:59:59 GMT, the last time an HTTP date can show.
const LAST_HTTP_DATE_SECONDS: u64 = 253_402_300_799;

/// Seconds since the Unix epoch (0 if the clock is before 1970).
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

/// RFC 822 date text in GMT, for example `Tue, 14 Oct 2025 00:00:00 GMT`. The client parses this
/// form first (follow-up note section 6). Times before 1970 give 1970; times after 9999 give the
/// last second of 9999.
pub fn http_date(unix_seconds: i64) -> String {
    let seconds = u64::try_from(unix_seconds)
        .unwrap_or(0)
        .min(LAST_HTTP_DATE_SECONDS);
    httpdate::fmt_http_date(UNIX_EPOCH + Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEAP_DAY_2000: i64 = 951_782_400;
    const SAMPLE: i64 = 1_760_400_000;

    #[test]
    fn dates_are_rfc_822_in_gmt() {
        assert_eq!(http_date(0), "Thu, 01 Jan 1970 00:00:00 GMT");
        assert_eq!(http_date(LEAP_DAY_2000), "Tue, 29 Feb 2000 00:00:00 GMT");
        assert_eq!(http_date(SAMPLE), "Tue, 14 Oct 2025 00:00:00 GMT");
        assert_eq!(http_date(-1), "Thu, 01 Jan 1970 00:00:00 GMT");
        assert_eq!(http_date(i64::MAX), "Fri, 31 Dec 9999 23:59:59 GMT");
    }
}
