// SPDX-License-Identifier: AGPL-3.0-or-later
//! `SyncDayPhase` (bodies note, section 4247): two big-endian `u64`, then a big-endian `u32`.
//! Reader `FUN_1440716c0`.
//!
//! The world's day and night (Observed: 10,282 `SyncDayPhase` bodies of 607 retail ledgers,
//! each set against the `SyncTimeChange` anchor before it): one cycle is the day's real seconds
//! plus the night's real seconds of `SyncTimeChange` (3,600 + 1,800 = 5,400 s; a fit of every
//! body to cycles of 1,200 to 10,800 s puts 10,124 of them on the right side at 5,400 s and no
//! other length comes near). The anchor is the game time `SyncTimeChange` names in its first
//! float (6.0 h); the day runs from the sunrise to the sunset of its `u32`s (21,300 s and
//! 65,100 s of the game day) in the day's real seconds, and the night runs the rest of the game
//! day in the night's real seconds. So the day starts 24.66 s before each cycle of the anchor
//! and the night 3,575.34 s after it; the retail changes came 3,574 to 3,577 s and 5,374 to
//! 5,377 s into the cycle. Retail sends only [`NIGHT_PHASE`] and [`DAY_PHASE`] (no dawn 1 and
//! no dusk 3 in any body), on the answer to the client's time request and alone, to the same
//! time component, at each change (ledger `01-starting-zone/12-1_getoffbeach-Soul-Survivor`:
//! the answer at 1790513756231 ms with 2, then 1790513972453 ms with 0 alone, 3,575.6 s into
//! the cycle of anchor 842804996850616895).

use crate::time_change::{OBSERVED_TIME_CHANGE_F32S_AT_16, OBSERVED_TIME_CHANGE_U32S_AT_28};
use crate::wire::{BodyCodec, MessageError, Reader, Writer};

/// File slot of `SyncDayPhase`.
pub const SYNC_DAY_PHASE_MSG_TYPE_INDEX: u64 = 4247;
/// The phase of the night (the catch log's `Night`).
pub const NIGHT_PHASE: u32 = 0;
/// The phase of the day (the catch log's `Day`).
pub const DAY_PHASE: u32 = 2;

/// Milliseconds per second.
const MS_PER_S: u64 = 1_000;
/// Nanoseconds per millisecond.
const NS_PER_MS: u64 = 1_000_000;
/// Seconds of a game day.
const GAME_DAY_S: u64 = 86_400;
/// Seconds of a game hour.
const GAME_HOUR_S: f32 = 3_600.0;
/// Where `SyncTimeChange`'s `u32`s hold the sunrise, the sunset (game seconds), the day's and
/// the night's real seconds.
const SUNRISE_AT: usize = 0;
const SUNSET_AT: usize = 2;
const DAY_REAL_AT: usize = 4;
const NIGHT_REAL_AT: usize = 5;
/// Where its floats hold the game hour of the anchor.
const ANCHOR_HOUR_AT: usize = 0;

/// The world's day cycle, from the values `SyncTimeChange` sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayCycle {
    /// The game second of the anchor.
    anchor_game_s: u64,
    sunrise_s: u64,
    sunset_s: u64,
    day_ms: u64,
    night_ms: u64,
}

impl DayCycle {
    /// The retail cycle: the values of every retail `SyncTimeChange`.
    #[must_use]
    pub fn retail() -> Self {
        let u32s = OBSERVED_TIME_CHANGE_U32S_AT_28;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "6.0 game hours, a whole game second"
        )]
        let anchor_game_s = (OBSERVED_TIME_CHANGE_F32S_AT_16[ANCHOR_HOUR_AT] * GAME_HOUR_S) as u64;
        Self {
            anchor_game_s,
            sunrise_s: u64::from(u32s[SUNRISE_AT]),
            sunset_s: u64::from(u32s[SUNSET_AT]),
            day_ms: u64::from(u32s[DAY_REAL_AT]) * MS_PER_S,
            night_ms: u64::from(u32s[NIGHT_REAL_AT]) * MS_PER_S,
        }
    }

    /// The real milliseconds of one day and night.
    #[must_use]
    pub const fn cycle_ms(&self) -> u64 {
        self.day_ms + self.night_ms
    }

    /// The real milliseconds from the start of a day to the game second `game_s`.
    fn since_sunrise_ms(&self, game_s: u64) -> u64 {
        let day_game_s = self.sunset_s - self.sunrise_s;
        let night_game_s = GAME_DAY_S - day_game_s;
        let from_sunrise = (game_s + GAME_DAY_S - self.sunrise_s) % GAME_DAY_S;
        if from_sunrise < day_game_s {
            from_sunrise * self.day_ms / day_game_s
        } else {
            self.day_ms + (from_sunrise - day_game_s) * self.night_ms / night_game_s
        }
    }

    /// The real milliseconds into its day and night at server time `now_ms` (ms since
    /// 2000-01-01) for the anchor `anchor_ns` (`SyncTimeChange`'s time, ns since 2000-01-01),
    /// counted from the start of a day.
    #[must_use]
    pub fn into_cycle_ms(&self, anchor_ns: u64, now_ms: u64) -> u64 {
        let elapsed = now_ms.saturating_sub(anchor_ns / NS_PER_MS);
        (self.since_sunrise_ms(self.anchor_game_s) + elapsed) % self.cycle_ms()
    }

    /// The day phase at server time `now_ms` for the anchor `anchor_ns`.
    #[must_use]
    pub fn phase(&self, anchor_ns: u64, now_ms: u64) -> u32 {
        if self.into_cycle_ms(anchor_ns, now_ms) < self.day_ms {
            DAY_PHASE
        } else {
            NIGHT_PHASE
        }
    }

    /// The game second of the day at server time `now_ms` for the anchor `anchor_ns`.
    #[must_use]
    pub fn game_second(&self, anchor_ns: u64, now_ms: u64) -> u64 {
        let into = self.into_cycle_ms(anchor_ns, now_ms);
        let day_game_s = self.sunset_s - self.sunrise_s;
        let night_game_s = GAME_DAY_S - day_game_s;
        let from_sunrise = if into < self.day_ms {
            into * day_game_s / self.day_ms
        } else {
            day_game_s + (into - self.day_ms) * night_game_s / self.night_ms
        };
        (self.sunrise_s + from_sunrise) % GAME_DAY_S
    }

    /// The anchor that puts game second `game_s` of the day at server time `now_ms`: a time set
    /// by a game master.
    #[must_use]
    pub fn anchor_for(&self, game_s: u64, now_ms: u64) -> u64 {
        let wanted = self.since_sunrise_ms(game_s % GAME_DAY_S);
        let at_anchor = self.since_sunrise_ms(self.anchor_game_s);
        let back = (wanted + self.cycle_ms() - at_anchor) % self.cycle_ms();
        now_ms.saturating_sub(back) * NS_PER_MS
    }
}

/// The day phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncDayPhaseMsg {
    /// Offset 0; varies per session.
    pub component: u64,
    /// Offset 8; varies per session.
    pub u64_at_8: u64,
    /// Offset 16.
    pub u32_at_16: u32,
}

impl BodyCodec for SyncDayPhaseMsg {
    const TYPE_INDEX: u64 = SYNC_DAY_PHASE_MSG_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        Ok(Writer::new()
            .u64(self.component)
            .u64(self.u64_at_8)
            .u32(self.u32_at_16)
            .finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let message = Self {
            component: reader.u64()?,
            u64_at_8: reader.u64()?,
            u32_at_16: reader.u32()?,
        };
        reader.finish()?;
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY_LEN: usize = 20;

    #[test]
    fn the_body_is_two_u64_and_one_u32() {
        let message = SyncDayPhaseMsg {
            component: 1,
            u64_at_8: 2,
            u32_at_16: DAY_PHASE,
        };
        let body = message.encode().expect("encode");
        assert_eq!(body.len(), BODY_LEN);
        assert_eq!(body[BODY_LEN - 1], 2);
        assert_eq!(SyncDayPhaseMsg::decode(&body), Ok(message));
        assert!(SyncDayPhaseMsg::decode(&body[..BODY_LEN - 1]).is_err());
    }

    /// The anchor of the retail ledger `12-1_getoffbeach-Soul-Survivor`.
    const RETAIL_ANCHOR_NS: u64 = 842_804_996_850_616_895;
    /// Unix ms of 2000-01-01.
    const SERVER_EPOCH_UNIX_MS: u64 = 946_684_800_000;
    /// Its time answer (day) and its lone night change, Unix ms.
    const RETAIL_ANSWER_UNIX_MS: u64 = 1_790_513_756_231;
    const RETAIL_NIGHT_UNIX_MS: u64 = 1_790_513_972_453;
    /// How late the retail change came after the computed one (a tick and the network).
    const CHANGE_SLACK_MS: u64 = 1_000;

    #[test]
    fn the_retail_ledger_s_phases_and_its_change_fall_where_the_cycle_puts_them() {
        let cycle = DayCycle::retail();
        let answer = RETAIL_ANSWER_UNIX_MS - SERVER_EPOCH_UNIX_MS;
        let night = RETAIL_NIGHT_UNIX_MS - SERVER_EPOCH_UNIX_MS;
        assert_eq!(cycle.phase(RETAIL_ANCHOR_NS, answer), DAY_PHASE);
        assert_eq!(cycle.phase(RETAIL_ANCHOR_NS, night), NIGHT_PHASE);
        assert_eq!(
            cycle.phase(RETAIL_ANCHOR_NS, night - CHANGE_SLACK_MS),
            DAY_PHASE,
            "the night began less than a second before the retail change"
        );
    }

    #[test]
    fn a_cycle_is_an_hour_of_day_and_half_an_hour_of_night() {
        const ANCHOR_NS: u64 = 0;
        const DAY_START_MS: u64 = 5_375_343;
        const NIGHT_START_MS: u64 = 3_575_343;
        const ONE_MS: u64 = 1;
        let cycle = DayCycle::retail();
        assert_eq!(cycle.cycle_ms(), 5_400_000);
        assert_eq!(cycle.phase(ANCHOR_NS, 0), DAY_PHASE, "06:00 is day");
        assert_eq!(cycle.phase(ANCHOR_NS, NIGHT_START_MS - ONE_MS), DAY_PHASE);
        assert_eq!(cycle.phase(ANCHOR_NS, NIGHT_START_MS), NIGHT_PHASE, "18:05");
        assert_eq!(cycle.phase(ANCHOR_NS, DAY_START_MS - ONE_MS), NIGHT_PHASE);
        assert_eq!(cycle.phase(ANCHOR_NS, DAY_START_MS), DAY_PHASE, "05:55");
    }

    #[test]
    fn the_game_clock_reads_the_anchor_hour_and_a_set_time_holds() {
        const SIX_H_S: u64 = 21_600;
        const MIDNIGHT_S: u64 = 0;
        const NOW_MS: u64 = 845_000_000_000;
        const ROUNDING_S: u64 = 1;
        let cycle = DayCycle::retail();
        assert!(SIX_H_S.abs_diff(cycle.game_second(NOW_MS * NS_PER_MS, NOW_MS)) <= ROUNDING_S);
        let anchor = cycle.anchor_for(MIDNIGHT_S, NOW_MS);
        assert!(cycle.game_second(anchor, NOW_MS) <= MIDNIGHT_S + ROUNDING_S);
        assert_eq!(cycle.phase(anchor, NOW_MS), NIGHT_PHASE);
    }
}
