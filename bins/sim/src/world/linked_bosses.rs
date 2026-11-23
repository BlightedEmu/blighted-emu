// SPDX-License-Identifier: AGPL-3.0-or-later
//! The expedition boss rules that no behavior tree of ours plays: the boss intros and the linked
//! Goliaths of the Ennead (`DungeonBrimstoneSands00`). Both run on the shared boss rules: the
//! mob invulnerability ([`crate::mobs::Mobs::set_invulnerable`]) and the health floors
//! ([`super::health_floors`], owner [`OWNER`]).
//!
//! **Intros.** An expedition boss takes no hits from its first sight until its encounter's intro
//! is over ([`super::dungeon_script::DungeonRun::boss_intro_over`]): Observed in every retail
//! expedition fight (`invulnerability` 1 at the first card, 0 at the intro's end): the siren
//! 22.5 and 30.1 s (`EXP-Barnacles-and-Black-Powder` 1002.7 s, `Barnacles-7909...` 1298.2 s),
//! `Naga_AncientGuardian` 24.0 to 37.4 s, `Torso_Boss` 48.6 to 64.4 s, `Bog_Monster` 27.1 to
//! 70.4 s, `Naga_AngryEarth` 33.7 to 63.8 s, the gorilla 28.4 to 58.0 s, the mammoth 42.3 to
//! 56.9 s, `Legion_GeneralCrassus` 23.3 to 72.3 s (r2 FINDINGS item 3). An intro whose end event
//! our trees never send ends at the latest [`INTRO_LIMIT_MS`] after the first sight (Inferred:
//! the longest of those turns).
//!
//! **Goliaths.** Observed in both retail Ennead runs (`EXP-The-Ennead` 1565.5 to 1825.4 s,
//! `Ennead-c910...` 1150.6 to 1475.7 s): a Goliath at 1 health turns invulnerable and is set
//! back to a share of its max health; it does not die alone. The Brute fights first while Horus
//! takes no hits; [`HORUS_WAKE_MS`] after the Brute's first fall Horus takes hits; when Horus
//! falls the first time both take hits again [`REVIVE_MS`] later; from then on a fallen Goliath
//! stays invulnerable for [`FALLEN_MS`] and comes back, and when both are fallen at once both die
//! [`DEATH_DELAY_MS`] after the second fall.

use std::collections::HashMap;

use nw_formats::objectstream::lowercase_crc32;

use super::SimWorld;
use crate::mobs::Invulnerable;

/// The owner name of the health floors these rules set ([`super::health_floors`]).
pub(super) const OWNER: &str = "linked_bosses";
/// The longest intro of an expedition boss, from its first sight (Inferred: the longest retail
/// first-sight invulnerability, `Legion_GeneralCrassus` 1272.6 -> 1345.0 s in `EXP-The-Ennead`).
const INTRO_LIMIT_MS: u64 = 72_300;
/// The game modes whose bosses take these rules: the expeditions (their ids start so).
const EXPEDITION_MODE_PREFIX: &str = "Dungeon";
/// The Brute Goliath's vitals row.
const BRUTE: &str = "AnubianGuardian_Brute_Boss";
/// The Horus Goliath's vitals row.
const HORUS: &str = "AnubianGuardian_Horus_Boss";
/// The health a fallen Goliath stops at.
const FALLEN_HEALTH: u32 = 1;
/// A Goliath's first fall sets it back to this share of its max health. Inferred from a range:
/// the Brute 50.0 % (209,986 of 419,970, `Ennead-c910...` 1265.5 s) and 39.3 % (165,091,
/// `EXP-The-Ennead` 1691.5 s); Horus 50.0 % (165,781 of 331,560, 1371.5 s) and 37.5 % (124,386,
/// 1771.1 s).
const FIRST_RESET_SHARE: f64 = 0.5;
/// The Brute's later falls set it back to this share. Inferred from a range: 25.0 % (104,994,
/// `Ennead-c910...` 1398.2 and 1473.8 s) and 18.8 % (78,787, `EXP-The-Ennead` 1818.2 s).
const BRUTE_LATER_RESET_SHARE: f64 = 0.25;
/// Horus's later falls set it back to this share. Inferred from a range: 20.0 % (66,313,
/// `Ennead-c910...` 1471.3 s) and 19.7 % (65,161, `EXP-The-Ennead` 1823.4 s).
const HORUS_LATER_RESET_SHARE: f64 = 0.2;
/// From the Brute's first fall to Horus's first hit taken. Observed: 7.4 s (1691.4 -> 1698.8)
/// and 7.5 s (1265.4 -> 1272.9).
const HORUS_WAKE_MS: u64 = 7_450;
/// From Horus's first fall to both taking hits. Observed: 3.8 s (1771.0 -> 1774.8) and 3.7 s
/// (1371.5 -> 1375.2).
const REVIVE_MS: u64 = 3_750;
/// How long a Goliath that falls in the fight of both stays fallen. Observed: the Brute
/// 1398.1 -> 1415.0 s (16.9 s, `Ennead-c910...`).
const FALLEN_MS: u64 = 16_900;
/// From the second of two fallen Goliaths to their deaths. Observed: 2.1 s (1823.3 -> 1825.4
/// and 1473.6 -> 1475.7).
const DEATH_DELAY_MS: u64 = 2_100;

/// Where the Goliaths' fight is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GoliathPhase {
    /// Their intro: neither takes hits.
    Intro,
    /// The Brute fights alone.
    Brute,
    /// The Brute fell; Horus wakes at `wakes_ms`.
    HorusWakes { wakes_ms: u64 },
    /// Horus fights alone.
    Horus,
    /// Horus fell; both take hits at `revives_ms`.
    Revive { revives_ms: u64 },
    /// Both fight.
    Both,
    /// Both fell; both die at `dies_ms`.
    Dying { dies_ms: u64 },
}

/// What one Goliath does at a tick.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct GoliathOrder {
    /// It takes no hits.
    invulnerable: bool,
    /// Its health goes to this share of its max health.
    reset_share: Option<f64>,
}

/// The fight of one Brute and one Horus.
#[derive(Debug, Clone, PartialEq)]
struct GoliathFight {
    brute: u64,
    horus: u64,
    phase: GoliathPhase,
    /// When each fell in the fight of both and is still fallen: (Brute, Horus).
    fallen_ms: (Option<u64>, Option<u64>),
    /// Who took each down last: (Brute, Horus); the deaths are theirs.
    last_attacker: (Option<u64>, Option<u64>),
}

/// What the fight does at a tick: an order for each and whether both die now.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct GoliathTick {
    brute: GoliathOrder,
    horus: GoliathOrder,
    both_die: bool,
}

impl GoliathFight {
    const fn new(brute: u64, horus: u64) -> Self {
        Self {
            brute,
            horus,
            phase: GoliathPhase::Intro,
            fallen_ms: (None, None),
            last_attacker: (None, None),
        }
    }

    /// One step at `now_ms`: `intro_over` once their encounter's intro ended, and which of
    /// them a hit took down to [`FALLEN_HEALTH`] since the last step (Brute, Horus).
    fn step(&mut self, now_ms: u64, intro_over: bool, fell: (bool, bool)) -> GoliathTick {
        let mut tick = GoliathTick::default();
        match self.phase {
            GoliathPhase::Intro if intro_over => self.phase = GoliathPhase::Brute,
            GoliathPhase::Brute if fell.0 => {
                tick.brute.reset_share = Some(FIRST_RESET_SHARE);
                self.phase = GoliathPhase::HorusWakes {
                    wakes_ms: now_ms + HORUS_WAKE_MS,
                };
            }
            GoliathPhase::HorusWakes { wakes_ms } if now_ms >= wakes_ms => {
                self.phase = GoliathPhase::Horus;
            }
            GoliathPhase::Horus if fell.1 => {
                tick.horus.reset_share = Some(FIRST_RESET_SHARE);
                self.phase = GoliathPhase::Revive {
                    revives_ms: now_ms + REVIVE_MS,
                };
            }
            GoliathPhase::Revive { revives_ms } if now_ms >= revives_ms => {
                self.phase = GoliathPhase::Both;
            }
            GoliathPhase::Both => {
                let expired = |fallen: Option<u64>| {
                    fallen.filter(|since| now_ms < since.saturating_add(FALLEN_MS))
                };
                self.fallen_ms = (expired(self.fallen_ms.0), expired(self.fallen_ms.1));
                if fell.0 {
                    self.fallen_ms.0 = Some(now_ms);
                    tick.brute.reset_share = Some(BRUTE_LATER_RESET_SHARE);
                }
                if fell.1 {
                    self.fallen_ms.1 = Some(now_ms);
                    tick.horus.reset_share = Some(HORUS_LATER_RESET_SHARE);
                }
                if self.fallen_ms.0.is_some() && self.fallen_ms.1.is_some() {
                    self.phase = GoliathPhase::Dying {
                        dies_ms: now_ms + DEATH_DELAY_MS,
                    };
                }
            }
            GoliathPhase::Dying { dies_ms } if now_ms >= dies_ms => tick.both_die = true,
            _ => {}
        }
        let (brute, horus) = match self.phase {
            GoliathPhase::Intro
            | GoliathPhase::HorusWakes { .. }
            | GoliathPhase::Revive { .. }
            | GoliathPhase::Dying { .. } => (true, true),
            GoliathPhase::Brute => (false, true),
            GoliathPhase::Horus => (true, false),
            GoliathPhase::Both => (self.fallen_ms.0.is_some(), self.fallen_ms.1.is_some()),
        };
        tick.brute.invulnerable = brute;
        tick.horus.invulnerable = horus;
        tick
    }
}

/// The linked boss rules of the sim.
#[derive(Debug, Default)]
pub(super) struct LinkedBosses {
    /// When each boss with an intro was first seen, while its intro runs.
    intros: HashMap<u64, u64>,
    /// The bosses whose intro ended: they take hits by their own rules again.
    intro_ended: Vec<u64>,
    /// The Goliaths' fight, while it runs.
    goliaths: Option<GoliathFight>,
}

impl SimWorld {
    /// The linked boss rules at `now_ms` (module text): the intros of the instance's expedition
    /// bosses and the Goliaths' fight.
    pub(super) fn tick_linked_bosses(&mut self, now_ms: u64) {
        let expedition = self
            .instance_run
            .as_ref()
            .is_some_and(|run| run.game_mode().starts_with(EXPEDITION_MODE_PREFIX));
        let Some(run) = self.dungeon_run.as_ref().filter(|_| expedition) else {
            return;
        };
        let bosses: Vec<(u64, bool)> = run
            .mobs()
            .filter_map(|mob| Some((mob, run.boss_intro_over(mob)?)))
            .filter(|(mob, _)| self.mobs.health(*mob).is_some_and(|health| health > 0))
            .collect();
        let brute_row = lowercase_crc32(BRUTE);
        let horus_row = lowercase_crc32(HORUS);
        let of_row = |row: u32| {
            bosses
                .iter()
                .find(|(mob, _)| self.mobs.vitals_of(*mob) == Some(row))
                .copied()
        };
        let pair = of_row(brute_row).zip(of_row(horus_row));
        for &(boss, intro_over) in &bosses {
            if pair.is_some_and(|((brute, _), (horus, _))| boss == brute || boss == horus) {
                continue;
            }
            self.boss_intro(boss, intro_over, now_ms);
        }
        let living: Vec<u64> = bosses.iter().map(|(mob, _)| *mob).collect();
        self.linked_bosses
            .intros
            .retain(|mob, _| living.contains(mob));
        self.linked_bosses
            .intro_ended
            .retain(|mob| living.contains(mob));
        match pair {
            Some(((brute, brute_intro), (horus, _))) => {
                let over = self.intro_due(brute, brute_intro, now_ms);
                self.goliaths_tick(brute, horus, over, now_ms);
            }
            None => self.end_goliaths(),
        }
    }

    /// Boss `boss` takes no hits from its first sight until its intro is over (`intro_over`)
    /// or [`INTRO_LIMIT_MS`] passed, then takes hits for good.
    fn boss_intro(&mut self, boss: u64, intro_over: bool, now_ms: u64) {
        if self.linked_bosses.intro_ended.contains(&boss) {
            return;
        }
        if self.intro_due(boss, intro_over, now_ms) {
            self.linked_bosses.intro_ended.push(boss);
            self.mobs.set_invulnerable(boss, None);
            tracing::info!(boss, intro_over, "boss intro over");
        } else if self.mobs.invulnerable(boss).is_none() {
            self.mobs.set_invulnerable(
                boss,
                Some(Invulnerable {
                    display_immune: true,
                }),
            );
        }
    }

    /// Whether the intro of boss `boss` is over at `now_ms`: its encounter says so
    /// (`intro_over`), or [`INTRO_LIMIT_MS`] passed since its first sight.
    fn intro_due(&mut self, boss: u64, intro_over: bool, now_ms: u64) -> bool {
        let seen_ms = *self.linked_bosses.intros.entry(boss).or_insert(now_ms);
        intro_over || now_ms >= seen_ms.saturating_add(INTRO_LIMIT_MS)
    }

    /// One step of the Goliaths' fight of `brute` and `horus` at `now_ms`.
    fn goliaths_tick(&mut self, brute: u64, horus: u64, intro_over: bool, now_ms: u64) {
        let fresh = self
            .linked_bosses
            .goliaths
            .as_ref()
            .is_none_or(|fight| fight.brute != brute || fight.horus != horus);
        if fresh {
            self.end_goliaths();
            for goliath in [brute, horus] {
                let max = self.mobs.get(goliath).map_or(0, |mob| mob.health.max());
                self.health_floors.set(
                    goliath,
                    f64::from(FALLEN_HEALTH) / f64::from(max.max(1)),
                    OWNER,
                );
            }
            self.linked_bosses.goliaths = Some(GoliathFight::new(brute, horus));
            let places = self.goliath_places(brute, horus);
            tracing::info!(brute, horus, ?places, "goliaths fight starts");
        }
        let reached = self.health_floors.take_reached(OWNER);
        let Some(fight) = self.linked_bosses.goliaths.as_mut() else {
            return;
        };
        let mut fell = (false, false);
        for reach in &reached {
            if reach.mob == brute {
                fell.0 = true;
                fight.last_attacker.0 = Some(reach.attacker);
            } else if reach.mob == horus {
                fell.1 = true;
                fight.last_attacker.1 = Some(reach.attacker);
            }
        }
        let before = fight.phase;
        let tick = fight.step(now_ms, intro_over, fell);
        let (phase, attackers) = (fight.phase, fight.last_attacker);
        if phase != before {
            let places = self.goliath_places(brute, horus);
            tracing::info!(?phase, ?places, "goliaths phase");
        }
        for (goliath, order) in [(brute, tick.brute), (horus, tick.horus)] {
            if let Some(share) = order.reset_share {
                self.reset_health(goliath, share);
            }
            let wanted = order.invulnerable.then_some(Invulnerable {
                display_immune: false,
            });
            if self.mobs.invulnerable(goliath) != wanted {
                self.mobs.set_invulnerable(goliath, wanted);
            }
        }
        if tick.both_die {
            self.end_goliaths();
            for (goliath, attacker) in [(brute, attackers.0), (horus, attackers.1)] {
                let health = self.mobs.health(goliath).unwrap_or_default();
                let attacker = attacker.or(attackers.0).or(attackers.1).unwrap_or_default();
                self.apply_hit(goliath, attacker, health, 0, now_ms);
            }
            tracing::info!(brute, horus, "goliaths die together");
        }
    }

    /// Where the Goliaths `brute` and `horus` stand and whether they think now (a mob that does
    /// not is no swing target and no card), for the fight's log lines.
    fn goliath_places(&self, brute: u64, horus: u64) -> [(Option<[f32; 3]>, bool); 2] {
        [brute, horus].map(|goliath| {
            let at = self
                .mobs
                .get(goliath)
                .map(|mob| [mob.position.x, mob.position.y, mob.position.z]);
            let awake = self.mobs.awake_states().any(|(mob, _, _)| mob == goliath);
            (at, awake)
        })
    }

    /// Mob `mob`'s health goes to `share` of its max health.
    fn reset_health(&mut self, mob: u64, share: f64) {
        let Some(health) = self.mobs.get_mut(mob).map(|mob| &mut mob.health) else {
            return;
        };
        // A share of a health of millions at most.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a share of the max health"
        )]
        let wanted = (f64::from(health.max()) * share).round() as u32;
        health.heal(wanted.saturating_sub(health.current()));
    }

    /// The Goliaths' fight ends: their floors and invulnerability go.
    fn end_goliaths(&mut self) {
        let Some(fight) = self.linked_bosses.goliaths.take() else {
            return;
        };
        for goliath in [fight.brute, fight.horus] {
            self.health_floors.clear(goliath);
            self.mobs.set_invulnerable(goliath, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BRUTE_MOB: u64 = 1;
    const HORUS_MOB: u64 = 2;
    const T0: u64 = 1_000_000;

    fn invulnerable(tick: &GoliathTick) -> (bool, bool) {
        (tick.brute.invulnerable, tick.horus.invulnerable)
    }

    /// The retail Ennead order (`Ennead-c910...` 1150.6 to 1475.7 s): intro, the Brute alone,
    /// Horus alone, both, and both die together only once both are fallen at once.
    #[test]
    fn the_goliaths_fall_and_come_back_until_both_are_fallen_at_once() {
        let mut fight = GoliathFight::new(BRUTE_MOB, HORUS_MOB);
        let none = (false, false);
        assert_eq!(
            invulnerable(&fight.step(T0, false, none)),
            (true, true),
            "intro"
        );
        assert_eq!(invulnerable(&fight.step(T0 + 1, true, none)), (false, true));
        let fell = fight.step(T0 + 2, true, (true, false));
        assert_eq!(fell.brute.reset_share, Some(FIRST_RESET_SHARE));
        assert_eq!(invulnerable(&fell), (true, true), "Horus still sleeps");
        let woke = fight.step(T0 + 2 + HORUS_WAKE_MS, true, none);
        assert_eq!(invulnerable(&woke), (true, false), "Horus alone");
        let fell = fight.step(T0 + 3 + HORUS_WAKE_MS, true, (false, true));
        assert_eq!(fell.horus.reset_share, Some(FIRST_RESET_SHARE));
        assert_eq!(invulnerable(&fell), (true, true));
        let both_at = T0 + 3 + HORUS_WAKE_MS + REVIVE_MS;
        assert_eq!(
            invulnerable(&fight.step(both_at, true, none)),
            (false, false)
        );
        let brute_fell = fight.step(both_at + 1, true, (true, false));
        assert_eq!(brute_fell.brute.reset_share, Some(BRUTE_LATER_RESET_SHARE));
        assert_eq!(
            invulnerable(&brute_fell),
            (true, false),
            "the Brute alone fell"
        );
        let back = fight.step(both_at + 1 + FALLEN_MS, true, none);
        assert_eq!(invulnerable(&back), (false, false), "the Brute came back");
        let again = both_at + 2 + FALLEN_MS;
        fight.step(again, true, (true, false));
        let second = fight.step(again + 1, true, (false, true));
        assert_eq!(second.horus.reset_share, Some(HORUS_LATER_RESET_SHARE));
        assert!(!second.both_die, "they die a moment later");
        assert_eq!(invulnerable(&second), (true, true));
        let died = fight.step(again + 1 + DEATH_DELAY_MS, true, none);
        assert!(died.both_die);
    }
}
