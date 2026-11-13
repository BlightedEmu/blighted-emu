// SPDX-License-Identifier: AGPL-3.0-or-later
//! The darkness of the territories and the invasions it brings (invasion study 2026-11-12, plan
//! `local-data/notes/2026-11-12-big-systems-plan.md` item 6). The director is the one writer: at
//! each war read ([`crate::instance_loop`]) it steps the darkness of every owned territory whose
//! fort has an invasion ([`nw_content::invasion_forts`]), schedules the invasions, and fills the
//! roster of each invasion whose instance is open or about to open.
//!
//! Observed (the claims 3086 and the war data 1739 of `invasion`, `Reekwater_Invasion`,
//! `Playthrough_Part_21` and `sign_up_for_invasion`):
//!
//! - every claim carries a darkness threshold of 0 to 3 and a darkness cycle end time; every
//!   invaded territory showed 3;
//! - the cycle ends fall on one daily clock, 82,843 to 82,861 s after midnight UTC
//!   ([`DARKNESS_CLOCK_MS`]), whole days apart between territories;
//! - the invasions started on the hour (19:00 and 20:00 UTC) of the day whose cycle end follows,
//!   3 to 4 hours before it, several territories at once.
//!
//! Inferred (no capture shows the level change): the level rises one step at each cycle end, a
//! cycle is one day ([`DARKNESS_CYCLE_MS`]); when it reaches [`DARKNESS_INVASION_LEVEL`] the
//! territory's invasion is scheduled at its owner's siege hour, the last one that lets the
//! invasion and its result time end before the cycle end ([`invasion_start`]); the next cycle
//! end takes the level back to 0. A territory with a war or an invasion without result gets no
//! new invasion; its level runs on. A cycle end the director missed (it did not run) is not made
//! up: the level steps once, to the next cycle end after now.
//!
//! Inferred (`Playthrough_Part_21`: a sign-up 463.8 s before the start got its invite 13.9 s
//! later, with no roster call between): from the instance lead before the start
//! ([`crate::war_instances::INVASION_INSTANCE_LEAD_MS`]) to the end, the invasion's sign-ups
//! without a roster place get the free places ([`nw_db::fill_invasion_roster`]). A sign-up after
//! the instance opened gets a place but no move (as a late war roster player).

use std::collections::BTreeSet;

use nw_db::{
    DARKNESS_INVASION_LEVEL, DarknessRow, DarknessStep, DbError, PgPool, TerritoryDarkness,
    TerritoryWar, WAR_RESULT_SHOWN_MS, apply_darkness, darkness_rows, fill_invasion_roster,
};

use crate::war_instances::instance_lead_ms;

/// One day, ms.
const DAY_MS: u64 = 86_400_000;

/// One hour, ms.
const HOUR_MS: f64 = 3_600_000.0;

/// The hours of a day.
const DAY_HOURS: f32 = 24.0;

/// When in the UTC day a darkness cycle ends, ms after midnight (Observed: the cycle ends of the
/// retail claims, 82,843 to 82,861 s).
pub const DARKNESS_CLOCK_MS: u64 = 82_850_000;

/// How long a darkness cycle lasts (Inferred: the retail cycle ends lie whole days apart).
pub const DARKNESS_CYCLE_MS: u64 = DAY_MS;

/// The first cycle end after Unix ms `now_ms`.
pub const fn next_cycle_end(now_ms: u64) -> u64 {
    let day_start = now_ms - now_ms % DAY_MS;
    let today = day_start + DARKNESS_CLOCK_MS;
    if today > now_ms {
        today
    } else {
        today + DAY_MS
    }
}

/// The ms after midnight UTC of siege hour `hour` (any value: it wraps into the day).
fn hour_ms(hour: f32) -> u64 {
    let hour = f64::from(hour.rem_euclid(DAY_HOURS));
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the hour is 0 to 24 after the wrap: its ms fit"
    )]
    let ms = (hour * HOUR_MS).round() as u64;
    ms
}

/// The start of the invasion of a cycle that ends at Unix ms `cycle_end_ms`, for an owner of
/// siege hour `siege_hour`: the last time at that hour from which the invasion and its result
/// time end before the cycle end (module doc).
pub fn invasion_start(cycle_end_ms: u64, siege_hour: f32) -> u64 {
    let day_start = cycle_end_ms - cycle_end_ms % DAY_MS;
    let mut start = day_start + hour_ms(siege_hour);
    let lasts = nw_db::INVASION_LENGTH_MS + WAR_RESULT_SHOWN_MS;
    while start.saturating_add(lasts) > cycle_end_ms {
        start = start.saturating_sub(DAY_MS);
    }
    start
}

/// The darkness steps due at Unix ms `now_ms` for the owned territories `rows`; `open_wars` are
/// the territories with a war or an invasion without result, `invadable` the territories whose
/// fort has an invasion (module doc).
pub fn darkness_steps(
    rows: &[DarknessRow],
    open_wars: &BTreeSet<u16>,
    invadable: &BTreeSet<u16>,
    now_ms: u64,
) -> Vec<DarknessStep> {
    let mut steps = Vec::new();
    for row in rows.iter().filter(|row| invadable.contains(&row.territory)) {
        let next_end = next_cycle_end(now_ms);
        let Some(darkness) = row.darkness else {
            steps.push(DarknessStep {
                darkness: TerritoryDarkness {
                    territory: row.territory,
                    level: 0,
                    cycle_end_ms: next_end,
                },
                invasion_starts_ms: None,
            });
            continue;
        };
        if now_ms < darkness.cycle_end_ms {
            continue;
        }
        let level = if darkness.level >= DARKNESS_INVASION_LEVEL {
            0
        } else {
            darkness.level + 1
        };
        let invasion_starts_ms = (level == DARKNESS_INVASION_LEVEL
            && !open_wars.contains(&row.territory))
        .then(|| invasion_start(next_end, row.siege_hour))
        .filter(|starts_ms| *starts_ms > now_ms);
        steps.push(DarknessStep {
            darkness: TerritoryDarkness {
                territory: row.territory,
                level,
                cycle_end_ms: next_end,
            },
            invasion_starts_ms,
        });
    }
    steps
}

/// The invasions of `wars` whose roster fills at Unix ms `now_ms`: from the instance lead before
/// the start to the end, without result.
pub fn filling(wars: &[TerritoryWar], now_ms: u64) -> impl Iterator<Item = &TerritoryWar> {
    wars.iter().filter(move |war| {
        war.is_invasion()
            && war.result.is_none()
            && now_ms >= war.starts_ms.saturating_sub(instance_lead_ms(war))
            && now_ms < war.ends_ms()
    })
}

/// Steps the darkness of world database `db` at Unix ms `now_ms` with the wars `wars` just read,
/// and fills the rosters of the invasions due (module doc); true when a roster changed (the
/// wars are then read again, so the opening has the filled roster).
///
/// # Errors
///
/// Returns [`DbError`] when a read or a write fails.
pub async fn step(
    db: &PgPool,
    wars: &[TerritoryWar],
    invadable: &BTreeSet<u16>,
    now_ms: u64,
) -> Result<bool, DbError> {
    let open_wars: BTreeSet<u16> = wars
        .iter()
        .filter(|war| war.result.is_none())
        .map(|war| war.territory)
        .collect();
    let rows = darkness_rows(db).await?;
    let steps = darkness_steps(&rows, &open_wars, invadable, now_ms);
    if !steps.is_empty() {
        for step in &steps {
            tracing::info!(
                territory = step.darkness.territory,
                level = step.darkness.level,
                invasion = ?step.invasion_starts_ms,
                "darkness step"
            );
        }
        apply_darkness(db, &steps).await?;
    }
    let mut filled = false;
    for war in filling(wars, now_ms) {
        let placed = fill_invasion_roster(db, war.war).await?;
        if placed > 0 {
            tracing::info!(war = %war.war, placed, "invasion roster filled");
            filled = true;
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use nw_db::{NO_ATTACKER, Uuid};

    use super::*;

    /// 2026-06-14 18:58:30 UTC: the `invasion` ledger's first record (Unix 1781463510).
    const NOW_MS: u64 = 1_781_463_510_000;
    /// The retail cycle end of that day's invaded territories (Unix 1781478048).
    const CYCLE_END_MS: u64 = 1_781_478_050_000;
    const TERRITORY: u16 = 16;
    const SIEGE_HOUR: f32 = 19.0;

    fn row(darkness: Option<(u8, u64)>) -> DarknessRow {
        DarknessRow {
            territory: TERRITORY,
            siege_hour: SIEGE_HOUR,
            darkness: darkness.map(|(level, cycle_end_ms)| TerritoryDarkness {
                territory: TERRITORY,
                level,
                cycle_end_ms,
            }),
        }
    }

    fn invadable() -> BTreeSet<u16> {
        BTreeSet::from([TERRITORY])
    }

    #[test]
    fn the_cycle_ends_on_the_daily_clock_and_the_invasion_at_the_siege_hour_before_it() {
        assert_eq!(next_cycle_end(NOW_MS), CYCLE_END_MS);
        assert_eq!(next_cycle_end(CYCLE_END_MS), CYCLE_END_MS + DAY_MS);
        // Observed: the invasion of territory 16 started at 19:00 UTC (Unix 1781463600).
        assert_eq!(invasion_start(CYCLE_END_MS, SIEGE_HOUR), 1_781_463_600_000);
        // Observed: territory 4's started at 20:00 UTC (Unix 1781467200) on the same cycle.
        assert_eq!(invasion_start(CYCLE_END_MS, 20.0), 1_781_467_200_000);
        // A siege hour too late for the cycle moves to the day before.
        assert_eq!(invasion_start(CYCLE_END_MS, 23.0), 1_781_391_600_000);
    }

    #[test]
    fn the_level_rises_each_cycle_and_its_third_step_schedules_the_invasion() {
        let open = BTreeSet::new();
        let first = darkness_steps(&[row(None)], &open, &invadable(), NOW_MS);
        assert_eq!(first[0].darkness.level, 0);
        assert_eq!(first[0].darkness.cycle_end_ms, CYCLE_END_MS);
        assert!(
            darkness_steps(&[row(Some((1, CYCLE_END_MS)))], &open, &invadable(), NOW_MS).is_empty()
        );
        let before_day = CYCLE_END_MS - DAY_MS;
        let rise = darkness_steps(&[row(Some((1, before_day)))], &open, &invadable(), NOW_MS);
        assert_eq!(
            (rise[0].darkness.level, rise[0].invasion_starts_ms),
            (2, None)
        );
        let third = darkness_steps(
            &[row(Some((2, before_day)))],
            &open,
            &invadable(),
            NOW_MS - 3_600_000,
        );
        assert_eq!(third[0].darkness.level, DARKNESS_INVASION_LEVEL);
        assert_eq!(third[0].invasion_starts_ms, Some(1_781_463_600_000));
        let reset = darkness_steps(&[row(Some((3, before_day)))], &open, &invadable(), NOW_MS);
        assert_eq!(
            (reset[0].darkness.level, reset[0].invasion_starts_ms),
            (0, None)
        );
    }

    #[test]
    fn a_war_a_past_start_or_a_fort_without_invasion_schedules_none() {
        let before_day = CYCLE_END_MS - DAY_MS;
        let at_war = BTreeSet::from([TERRITORY]);
        let early = NOW_MS - 3_600_000;
        let steps = darkness_steps(&[row(Some((2, before_day)))], &at_war, &invadable(), early);
        assert_eq!(
            (steps[0].darkness.level, steps[0].invasion_starts_ms),
            (3, None)
        );
        let late = darkness_steps(
            &[row(Some((2, before_day)))],
            &BTreeSet::new(),
            &invadable(),
            NOW_MS + 3_600_000,
        );
        assert_eq!(late[0].invasion_starts_ms, None);
        assert!(
            darkness_steps(&[row(None)], &BTreeSet::new(), &BTreeSet::new(), NOW_MS).is_empty()
        );
    }

    #[test]
    fn an_invasion_fills_from_the_lead_to_its_end_and_a_war_never() {
        let invasion = TerritoryWar {
            war: Uuid::from_u128(1),
            territory: TERRITORY,
            attacker: NO_ATTACKER,
            defender: (Uuid::from_u128(2), 1),
            number: 7,
            starts_ms: NOW_MS,
            result: None,
            signups: Vec::new(),
        };
        let war = TerritoryWar {
            attacker: (Uuid::from_u128(3), 2),
            ..invasion.clone()
        };
        let wars = [invasion, war];
        let at = |ms| filling(&wars, ms).count();
        let lead = crate::war_instances::INVASION_INSTANCE_LEAD_MS;
        assert_eq!(at(NOW_MS - lead - 1), 0);
        assert_eq!(at(NOW_MS - lead), 1);
        assert_eq!(at(NOW_MS + nw_db::INVASION_LENGTH_MS - 1), 1);
        assert_eq!(at(NOW_MS + nw_db::INVASION_LENGTH_MS), 0);
    }
}
