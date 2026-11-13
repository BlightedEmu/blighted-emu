// SPDX-License-Identifier: AGPL-3.0-or-later
//! The server's daily and weekly resets: the one clock that every daily or weekly limit,
//! cooldown, refresh and lock of the world reads ([`ResetClock`]).
//!
//! **Observed** (`lastDailyResetTime` of the own social state (4176) in 948 retail ledgers, 802
//! values, `local-data/scratch-work/d9-gather/scan_social_reset.py` and `region_resets.py`; the
//! region from the ledger's voice host): the daily reset is 05:00 local time of the region's
//! time zone, with that zone's daylight saving time:
//! - EU Central (voice host `nwxp0euc1`, and the ledgers without a voice host): 03:00 UTC in
//!   713 states up to 2026-10-24, 04:00 UTC in 40 states from 2026-10-25, the day European summer
//!   time ended;
//! - US East (voice host `nwxp`): 09:00 UTC in 44 states up to 2026-10-28, 10:00 UTC on
//!   2026-11-06, after US daylight saving time ended on 2026-11-01;
//! - US West (voice host `nwxp`): 12:00 UTC (05:00 PDT) on 2026-06-24, one state;
//! - AP Southeast (`nwxp0apse2`): 19:00 UTC (05:00 AEST) on 2026-06-10 and 2026-06-14;
//! - SA East (`nwxp0sae1`): 07:00 UTC (04:00 in São Paulo) on 2026-07-01, one state.
//!
//! The same hours end the daily loot limits, recipe cooldowns and instanced gather locks (the
//! 139 tracker entries capped at the reset: 114 at 03:00 UTC, 25 at 04:00 UTC from 2026-10-25),
//! each whole seconds after its start ([`ResetClock::end_ns`]). The weekly reset is on Tuesday
//! at the daily reset hour (853 weekly loot limit ends: Tuesday 03:00 UTC on the EU Central
//! server, Tuesday 09:00 UTC on the US East server).
//!
//! **Inferred**: the zone names (Europe/Berlin gives the same UTC times as 04:00 London time;
//! Australia/Sydney and Australia/Brisbane are the same in June), the weekly day of the AP
//! Southeast and SA East servers (Tuesday, as the others) and the daylight saving time of the
//! AP Southeast server (Sydney's). **Unknown**: why the SA East hour is 04:00 local.

use jiff::Timestamp;
use jiff::tz::{self, TimeZone};
use nw_common::time::SECONDS_PER_DAY;
use serde::Deserialize;

/// Hours in one day: an hour of the day is below it.
pub const HOURS_PER_DAY: u32 = 24;
const SECONDS_PER_HOUR: i64 = 3_600;
const DAYS_PER_WEEK: i64 = 7;
/// 1970-01-01 was a Thursday: the weekday index (Monday 0) of day 0.
const DAY_ZERO_WEEKDAY: i64 = 3;
/// The weekday index (Monday 0) of the weekly reset: Tuesday (Observed).
const WEEKLY_RESET_WEEKDAY: i64 = 1;
/// The local hour of the daily reset (Observed on the EU Central, US East, US West and AP
/// Southeast servers).
const RESET_HOUR: i64 = 5;
/// The local hour of the SA East server's daily reset (Observed, one state).
const SA_EAST_RESET_HOUR: i64 = 4;
/// Nanoseconds in one second.
const NS_PER_SECOND: u64 = 1_000_000_000;

static US_EAST: TimeZone = tz::get!("America/New_York");
static US_WEST: TimeZone = tz::get!("America/Los_Angeles");
static EU_CENTRAL: TimeZone = tz::get!("Europe/Berlin");
static SA_EAST: TimeZone = tz::get!("America/Sao_Paulo");
static AP_SOUTHEAST: TimeZone = tz::get!("Australia/Sydney");

/// The retail region a server runs in: it sets the time zone of the resets. In config text:
/// `us-east`, `us-west`, `eu-central`, `sa-east` or `ap-southeast`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServerRegion {
    /// US East.
    UsEast,
    /// US West.
    UsWest,
    /// EU Central.
    EuCentral,
    /// SA East.
    SaEast,
    /// AP Southeast.
    ApSoutheast,
}

impl ServerRegion {
    /// The time zone of its resets.
    fn time_zone(self) -> &'static TimeZone {
        match self {
            Self::UsEast => &US_EAST,
            Self::UsWest => &US_WEST,
            Self::EuCentral => &EU_CENTRAL,
            Self::SaEast => &SA_EAST,
            Self::ApSoutheast => &AP_SOUTHEAST,
        }
    }

    /// The local hour of its daily reset.
    const fn reset_hour(self) -> i64 {
        match self {
            Self::SaEast => SA_EAST_RESET_HOUR,
            Self::UsEast | Self::UsWest | Self::EuCentral | Self::ApSoutheast => RESET_HOUR,
        }
    }
}

/// Which reset ends a count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetCycle {
    /// The next daily reset.
    Daily,
    /// The next weekly reset.
    Weekly,
}

/// The server's resets (`[resets]` of `config/sim.toml`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetClock {
    /// The region of the server's world (the same as the world's `region` in the edge config).
    pub region: ServerRegion,
}

impl ResetClock {
    /// The first reset of `cycle` after Unix second `now_s`.
    pub fn next(&self, cycle: ResetCycle, now_s: i64) -> i64 {
        let local_day = (now_s + self.offset_s(now_s)).div_euclid(SECONDS_PER_DAY);
        let mut day = if self.reset_on(local_day) > now_s {
            local_day
        } else {
            local_day + 1
        };
        if cycle == ResetCycle::Weekly {
            let weekday = (day + DAY_ZERO_WEEKDAY).rem_euclid(DAYS_PER_WEEK);
            day += (WEEKLY_RESET_WEEKDAY - weekday).rem_euclid(DAYS_PER_WEEK);
        }
        self.reset_on(day)
    }

    /// The end of a count of `cycle` that starts at Unix nanosecond `now_ns`: whole seconds
    /// after it, at the first reset after it (Observed: loot limit ends and the capped gather
    /// locks keep the start's sub-second part).
    pub fn end_ns(&self, cycle: ResetCycle, now_ns: u64) -> u64 {
        let now_s = i64::try_from(now_ns / NS_PER_SECOND).unwrap_or(i64::MAX);
        let ahead_s = u64::try_from(self.next(cycle, now_s) - now_s).unwrap_or_default();
        now_ns.saturating_add(ahead_s.saturating_mul(NS_PER_SECOND))
    }

    /// The Unix second of the reset on local day `day` (days from 1970-01-01 in the region's
    /// zone). The reset hour is hours away from every change of these zones' offsets, so the
    /// offset at the first guess (at most the largest offset change off) is the reset's own.
    fn reset_on(&self, day: i64) -> i64 {
        let wall_s = day * SECONDS_PER_DAY + self.region.reset_hour() * SECONDS_PER_HOUR;
        let guess_s = wall_s - self.offset_s(wall_s);
        wall_s - self.offset_s(guess_s)
    }

    /// The region zone's offset from UTC, in seconds, at Unix second `unix_s` (a second out of
    /// the time range takes the range's nearest end).
    fn offset_s(&self, unix_s: i64) -> i64 {
        let at = Timestamp::from_second(unix_s).unwrap_or(if unix_s < 0 {
            Timestamp::MIN
        } else {
            Timestamp::MAX
        });
        i64::from(self.region.time_zone().to_offset(at).seconds())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn clock(region: ServerRegion) -> ResetClock {
        ResetClock { region }
    }

    const EU_CENTRAL_CLOCK: ResetClock = clock(ServerRegion::EuCentral);
    const US_EAST_CLOCK: ResetClock = clock(ServerRegion::UsEast);
    const WEEK_S: i64 = DAYS_PER_WEEK * SECONDS_PER_DAY;

    /// Saturday 2026-10-24 12:00 UTC.
    const EU_SUMMER_LAST_DAY_S: i64 = 1_792_843_200;
    /// Saturday 2026-10-24 03:00 UTC (05:00 CEST).
    const EU_SUMMER_RESET_S: i64 = 1_792_810_800;
    /// Sunday 2026-10-25 04:00 UTC (05:00 CET): European summer time ended at 01:00 UTC.
    const EU_WINTER_RESET_S: i64 = 1_792_900_800;
    /// Saturday 2026-03-28 12:00 UTC.
    const EU_WINTER_LAST_DAY_S: i64 = 1_774_699_200;
    /// Sunday 2026-03-29 03:00 UTC (05:00 CEST): summer time started at 01:00 UTC.
    const EU_SPRING_RESET_S: i64 = 1_774_753_200;
    /// Saturday 2026-10-31 12:00 UTC.
    const US_SUMMER_LAST_DAY_S: i64 = 1_793_448_000;
    /// Sunday 2026-11-01 10:00 UTC (05:00 EST): US daylight saving time ended at 06:00 UTC.
    const US_WINTER_RESET_S: i64 = 1_793_527_200;
    /// Saturday 2026-03-07 12:00 UTC.
    const US_WINTER_LAST_DAY_S: i64 = 1_772_884_800;
    /// Sunday 2026-03-08 09:00 UTC (05:00 EDT): daylight saving time started at 07:00 UTC.
    const US_SPRING_RESET_S: i64 = 1_772_960_400;
    /// Tuesday 2026-10-27 09:00 UTC (05:00 EDT), a retail weekly loot limit end.
    const US_SUMMER_WEEKLY_S: i64 = 1_793_091_600;
    /// Tuesday 2026-11-03 10:00 UTC (05:00 EST).
    const US_WINTER_WEEKLY_S: i64 = 1_793_700_000;
    /// Tuesday 2026-10-27 04:00 UTC (05:00 CET).
    const EU_WINTER_WEEKLY_S: i64 = 1_793_073_600;

    #[test]
    fn the_eu_central_reset_moves_with_european_summer_time() {
        assert_eq!(
            EU_CENTRAL_CLOCK.next(ResetCycle::Daily, EU_SUMMER_RESET_S - 1),
            EU_SUMMER_RESET_S,
            "03:00 UTC in summer (713 retail states)"
        );
        assert_eq!(
            EU_CENTRAL_CLOCK.next(ResetCycle::Daily, EU_SUMMER_LAST_DAY_S),
            EU_WINTER_RESET_S,
            "04:00 UTC from 2026-10-25 (40 retail states)"
        );
        assert_eq!(
            EU_CENTRAL_CLOCK.next(ResetCycle::Daily, EU_WINTER_LAST_DAY_S),
            EU_SPRING_RESET_S
        );
        assert_eq!(
            EU_CENTRAL_CLOCK.next(ResetCycle::Weekly, EU_SUMMER_LAST_DAY_S),
            EU_WINTER_WEEKLY_S
        );
    }

    #[test]
    fn the_us_east_reset_moves_with_us_daylight_saving_time() {
        assert_eq!(
            US_EAST_CLOCK.next(ResetCycle::Daily, US_SUMMER_LAST_DAY_S),
            US_WINTER_RESET_S,
            "10:00 UTC after 2026-11-01 (the 2026-11-06 retail state)"
        );
        assert_eq!(
            US_EAST_CLOCK.next(ResetCycle::Daily, US_WINTER_LAST_DAY_S),
            US_SPRING_RESET_S
        );
        assert_eq!(
            US_EAST_CLOCK.next(ResetCycle::Daily, EU_SUMMER_LAST_DAY_S),
            EU_SUMMER_LAST_DAY_S + 21 * SECONDS_PER_HOUR,
            "the US reset stays at 09:00 UTC after the European change"
        );
    }

    #[test]
    fn a_reset_moment_gives_the_next_cycle() {
        assert_eq!(
            US_EAST_CLOCK.next(ResetCycle::Daily, US_SPRING_RESET_S),
            US_SPRING_RESET_S + SECONDS_PER_DAY
        );
        assert_eq!(
            US_EAST_CLOCK.next(ResetCycle::Weekly, US_SUMMER_WEEKLY_S),
            US_WINTER_WEEKLY_S,
            "a week later, one hour later in UTC"
        );
        assert_eq!(
            US_EAST_CLOCK.next(ResetCycle::Weekly, US_WINTER_WEEKLY_S),
            US_WINTER_WEEKLY_S + WEEK_S
        );
    }

    #[test]
    fn every_region_resets_at_its_captured_utc_hour() {
        // (region, a moment, the captured reset after it)
        let captured = [
            // Wednesday 2026-06-24 00:00 UTC -> 12:00 UTC.
            (ServerRegion::UsWest, 1_782_259_200, 1_782_302_400),
            // Wednesday 2026-06-10 12:00 UTC -> 19:00 UTC (05:00 AEST on 2026-06-11).
            (ServerRegion::ApSoutheast, 1_781_092_800, 1_781_118_000),
            // Wednesday 2026-07-01 00:00 UTC -> 07:00 UTC.
            (ServerRegion::SaEast, 1_782_864_000, 1_782_889_200),
        ];
        for (region, now_s, reset_s) in captured {
            assert_eq!(clock(region).next(ResetCycle::Daily, now_s), reset_s, "{region:?}");
        }
    }

    #[test]
    fn the_weekly_reset_is_on_the_regions_local_tuesday() {
        // Monday 2026-06-08 12:00 UTC -> Monday 19:00 UTC, Tuesday 05:00 AEST.
        assert_eq!(
            clock(ServerRegion::ApSoutheast).next(ResetCycle::Weekly, 1_780_920_000),
            1_780_945_200
        );
    }

    #[test]
    fn an_end_keeps_the_starts_sub_second_part() {
        const SUB_SECOND_NS: u64 = 894_621_000;
        let start_ns = u64::try_from(US_SUMMER_LAST_DAY_S).expect("positive") * NS_PER_SECOND
            + SUB_SECOND_NS;
        let reset_ns = u64::try_from(US_WINTER_RESET_S).expect("positive") * NS_PER_SECOND;
        assert_eq!(
            US_EAST_CLOCK.end_ns(ResetCycle::Daily, start_ns),
            reset_ns + SUB_SECOND_NS
        );
    }

    #[test]
    fn a_config_names_the_region() {
        #[derive(Deserialize)]
        struct Config {
            resets: ResetClock,
        }
        let config: Config =
            serde_json::from_str(r#"{"resets": {"region": "eu-central"}}"#).expect("config");
        assert_eq!(config.resets, EU_CENTRAL_CLOCK);
        assert!(
            serde_json::from_str::<Config>(r#"{"resets": {"region": "moon"}}"#).is_err(),
            "an unknown region is refused"
        );
    }
}
