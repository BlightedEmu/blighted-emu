// SPDX-License-Identifier: AGPL-3.0-or-later
//! Combat rules (M6-A spec section 4): health, the hit formulas, crits and threat. The formulas
//! are Inferred from the datasheet field names; the client holds none of them. Every random roll
//! comes from [`Dice`], seeded by the caller, so a run repeats exactly.

use std::collections::HashMap;

/// The smallest hit: a hit that lands always costs health.
pub const MIN_HIT: u32 = 1;

/// SplitMix64 increment.
const SPLITMIX_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
/// SplitMix64 mixing constants.
const SPLITMIX_MIX_1: u64 = 0xBF58_476D_1CE4_E5B9;
const SPLITMIX_MIX_2: u64 = 0x94D0_49BB_1331_11EB;
/// Bits of a roll that make a fraction from 0 to 1 (the mantissa of an `f64`).
const FRACTION_BITS: u32 = 53;

/// Seeded random rolls (SplitMix64).
#[derive(Debug, Clone)]
pub struct Dice {
    state: u64,
}

impl Dice {
    /// Dice that give the same rolls for the same `seed`.
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(SPLITMIX_GAMMA);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(SPLITMIX_MIX_1);
        value = (value ^ (value >> 27)).wrapping_mul(SPLITMIX_MIX_2);
        value ^ (value >> 31)
    }

    /// `true` with chance `p` (0 never, 1 always).
    pub fn chance(&mut self, p: f64) -> bool {
        let fraction =
            (self.next() >> (u64::BITS - FRACTION_BITS)) as f64 / (1_u64 << FRACTION_BITS) as f64;
        fraction < p
    }

    /// A roll from 0 to `n - 1`; 0 when `n` is 0.
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        // The remainder of a u64 by a u32 fits a u32.
        (self.next() % u64::from(n)) as u32
    }
}

/// What a hit did to a health.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Damage {
    /// Health is lower, above 0.
    Hurt,
    /// This hit took the last health.
    Killed,
    /// The health was already 0.
    AlreadyDead,
}

/// Health from 0 to its max; at 0 the entity is dead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Health {
    current: u32,
    max: u32,
}

impl Health {
    /// Health at its `max`.
    pub const fn full(max: u32) -> Self {
        Self { current: max, max }
    }

    /// Health at `current`, at most `max` (a player that arrives from another sim).
    pub fn at(current: u32, max: u32) -> Self {
        Self {
            current: current.min(max),
            max,
        }
    }

    /// Takes `amount` of health.
    pub fn take(&mut self, amount: u32) -> Damage {
        if self.current == 0 {
            return Damage::AlreadyDead;
        }
        self.current = self.current.saturating_sub(amount);
        if self.current == 0 {
            Damage::Killed
        } else {
            Damage::Hurt
        }
    }

    /// Whether the health is 0.
    pub const fn is_dead(&self) -> bool {
        self.current == 0
    }

    /// Back to the max.
    pub const fn refill(&mut self) {
        self.current = self.max;
    }

    /// Adds `amount`, at most to the max; the health after it.
    pub fn heal(&mut self, amount: u32) -> u32 {
        self.current = self.current.saturating_add(amount).min(self.max);
        self.current
    }

    /// The health now.
    pub const fn current(&self) -> u32 {
        self.current
    }

    /// The max.
    pub const fn max(&self) -> u32 {
        self.max
    }
}

/// Max health of a mob: the level's base health, its `HealthMod`, then its creature type's
/// `CategoryHealthMod`, each product without its fraction (the base too). Observed on all 14
/// kinds and levels of the retail cards of newchar-retail-2026-09-18, among them the tutorial
/// sailor (337.5, 1.05, 1.0: 353, where the plain product gives 354), the level-3 boar (412.5,
/// 1.75, 0.6: 432) and the named level-4 sailor (466.55, 1.3, 2.5: 605 x 2.5 = 1512).
pub fn mob_max_health(base_max_health: f64, health_mod: f64, category_mod: f64) -> u32 {
    let typed = (base_max_health.floor() * health_mod).floor();
    to_whole((typed * category_mod).floor())
}

/// A rounded, finite, non-negative value as a whole number.
fn to_whole(value: f64) -> u32 {
    if value.is_finite() && value > 0.0 {
        // Clamped to the u32 range first, so the cast only drops the fraction.
        value.round().min(f64::from(u32::MAX)) as u32
    } else {
        0
    }
}

/// One hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    /// Health it takes, at least [`MIN_HIT`].
    pub amount: u32,
    /// Whether it was a crit.
    pub crit: bool,
}

/// A player's attack with its weapon.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerAttack {
    /// The weapon's base damage.
    pub base_damage: f64,
    /// The attack's damage coefficient.
    pub coefficient: f64,
    /// Whether the attack can crit.
    pub can_crit: bool,
    /// The weapon's crit chance, 0 to 1.
    pub crit_chance: f64,
    /// The weapon's crit damage multiplier.
    pub crit_multiplier: f64,
}

/// A mob's attack.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MobAttack {
    /// The level's base damage.
    pub base_damage: f64,
    /// The type's damage mod.
    pub damage_mod: f64,
    /// The config factor for the attack row, which lives in the mob's action list.
    pub factor: f64,
}

/// A player's hit on a target with `armor_share` of the damage taken off.
pub fn player_hit(attack: &PlayerAttack, armor_share: f64, dice: &mut Dice) -> Hit {
    let crit = attack.can_crit && dice.chance(attack.crit_chance);
    let multiplier = if crit { attack.crit_multiplier } else { 1.0 };
    let raw = attack.base_damage * attack.coefficient * multiplier * (1.0 - armor_share);
    Hit {
        amount: to_whole(raw).max(MIN_HIT),
        crit,
    }
}

/// A mob's hit on a target with `armor_share` of the damage taken off. Mob attacks do not crit.
pub fn mob_hit(attack: &MobAttack, armor_share: f64) -> Hit {
    let raw = attack.base_damage * attack.damage_mod * attack.factor * (1.0 - armor_share);
    Hit {
        amount: to_whole(raw).max(MIN_HIT),
        crit: false,
    }
}

/// Threat of each attacker on one target.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Threat {
    by_attacker: HashMap<u64, u64>,
}

impl Threat {
    /// Adds `amount` to `attacker`'s threat.
    pub fn add(&mut self, attacker: u64, amount: u64) {
        let threat = self.by_attacker.entry(attacker).or_default();
        *threat = threat.saturating_add(amount);
    }

    /// Removes `attacker`.
    pub fn forget(&mut self, attacker: u64) {
        self.by_attacker.remove(&attacker);
    }

    /// The attacker with the most threat; a tie goes to the lower entity number.
    pub fn top(&self) -> Option<u64> {
        self.by_attacker
            .iter()
            .max_by(|(one, a), (two, b)| a.cmp(b).then(two.cmp(one)))
            .map(|(attacker, _)| *attacker)
    }

    /// Removes every attacker.
    pub fn clear(&mut self) {
        self.by_attacker.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: u64 = 7;
    const LEVEL_20_HEALTH: f64 = 1872.2;
    const HALF: f64 = 0.5;
    const SWORD_BASE: f64 = 64.0;
    const HEAVY_COEFFICIENT: f64 = 1.2;
    const ARMOR_SHARE: f64 = 0.35;
    const CRIT_MULTIPLIER: f64 = 1.3;
    const ROLLS: usize = 1000;

    fn sword(can_crit: bool, crit_chance: f64) -> PlayerAttack {
        PlayerAttack {
            base_damage: SWORD_BASE,
            coefficient: HEAVY_COEFFICIENT,
            can_crit,
            crit_chance,
            crit_multiplier: CRIT_MULTIPLIER,
        }
    }

    #[test]
    fn a_heal_stops_at_the_max() {
        let mut health = Health::at(500, 856);
        assert_eq!(health.heal(300), 800);
        assert_eq!(health.heal(486), 856);
    }

    #[test]
    fn mob_health_is_the_whole_base_times_mod_without_the_fraction() {
        /// A product just below 371.
        const NEAR_371: f64 = 370.9;
        assert_eq!(mob_max_health(LEVEL_20_HEALTH, HALF, 1.0), 936);
        assert_eq!(mob_max_health(LEVEL_20_HEALTH, 1.0, 1.0), 1872);
        assert_eq!(mob_max_health(NEAR_371, 1.0, 1.0), 370);
    }

    /// The retail cards of newchar-retail-2026-09-18 (vitals `maxHealth`; `VitalsLevelData`
    /// `BaseMaxHealth`, `HealthMod`, `CategoryHealthMod` of the creature type).
    #[test]
    fn mob_health_matches_the_retail_cards() {
        const LEVEL_1: f64 = 337.5;
        const LEVEL_2: f64 = 375.0;
        const LEVEL_3: f64 = 412.5;
        const LEVEL_4: f64 = 466.55;
        // Corrupted_Sailor_1H_Club_FTUE (card 0x0161), Legion_Legionnaire_FTUE.
        assert_eq!(mob_max_health(LEVEL_1, 1.05, 1.0), 353);
        assert_eq!(mob_max_health(LEVEL_1, 1.1, 1.0), 370);
        // Boar_StarterBeach, Solo-_StarterBeach.
        assert_eq!(mob_max_health(LEVEL_2, 1.75, 0.6), 393);
        assert_eq!(mob_max_health(LEVEL_3, 1.75, 0.6), 432);
        // Corrupted_Sailor_1H_Club, _1H_Sword, _2H_Axe, Solo_StarterBeach.
        assert_eq!(mob_max_health(LEVEL_2, 0.6, 1.0), 225);
        assert_eq!(mob_max_health(LEVEL_3, 0.6, 1.0), 247);
        assert_eq!(mob_max_health(LEVEL_3, 0.7, 1.0), 288);
        assert_eq!(mob_max_health(LEVEL_4, 0.6, 1.0), 279);
        assert_eq!(mob_max_health(LEVEL_4, 0.7, 1.0), 326);
        // Named_00 sailors, Solo+_StarterBeach; the named beach captain, Named_Solo+.
        assert_eq!(mob_max_health(LEVEL_2, 0.75, 2.0), 562);
        assert_eq!(mob_max_health(LEVEL_3, 0.75, 2.0), 618);
        assert_eq!(mob_max_health(LEVEL_4, 1.3, 2.5), 1512);
    }

    #[test]
    fn a_player_hit_uses_weapon_coefficient_and_armor() {
        let hit = player_hit(&sword(false, 0.0), ARMOR_SHARE, &mut Dice::new(SEED));
        assert_eq!(
            hit,
            Hit {
                amount: 50,
                crit: false
            }
        );
    }

    #[test]
    fn a_hit_is_at_least_one() {
        let weak = PlayerAttack {
            base_damage: 0.1,
            ..sword(false, 0.0)
        };
        assert_eq!(
            player_hit(&weak, ARMOR_SHARE, &mut Dice::new(SEED)).amount,
            MIN_HIT
        );
        let mob = MobAttack {
            base_damage: 0.0,
            damage_mod: 1.0,
            factor: 1.0,
        };
        assert_eq!(mob_hit(&mob, 0.0).amount, MIN_HIT);
    }

    #[test]
    fn a_crit_multiplies_only_when_the_attack_can_crit() {
        let always = player_hit(&sword(true, 1.0), ARMOR_SHARE, &mut Dice::new(SEED));
        assert_eq!(
            always,
            Hit {
                amount: 65,
                crit: true
            }
        );
        let never = player_hit(&sword(false, 1.0), ARMOR_SHARE, &mut Dice::new(SEED));
        assert!(!never.crit);
        let mut dice = Dice::new(SEED);
        let crits = (0..ROLLS)
            .filter(|_| player_hit(&sword(true, HALF), ARMOR_SHARE, &mut dice).crit)
            .count();
        assert!((400..600).contains(&crits), "{crits} of {ROLLS}");
    }

    #[test]
    fn a_mob_hit_uses_level_damage_mod_and_factor() {
        let attack = MobAttack {
            base_damage: 686.1,
            damage_mod: HALF,
            factor: 2.0,
        };
        assert_eq!(
            mob_hit(&attack, 0.0),
            Hit {
                amount: 686,
                crit: false
            }
        );
        assert_eq!(mob_hit(&attack, HALF).amount, 343);
    }

    #[test]
    fn health_takes_hits_and_dies_once() {
        let mut health = Health::full(100);
        assert_eq!(health.take(30), Damage::Hurt);
        assert_eq!(health.current(), 70);
        assert_eq!(health.take(500), Damage::Killed);
        assert!(health.is_dead());
        assert_eq!(health.current(), 0);
        assert_eq!(health.take(1), Damage::AlreadyDead);
        health.refill();
        assert_eq!(health.current(), health.max());
    }

    #[test]
    fn a_health_can_start_below_its_max() {
        let health = Health::at(30, 100);
        assert_eq!((health.current(), health.max()), (30, 100));
        assert_eq!(Health::at(500, 100).current(), 100, "never above the max");
        assert!(Health::at(0, 100).is_dead());
    }

    #[test]
    fn threat_tops_the_largest_and_forgets() {
        let mut threat = Threat::default();
        assert_eq!(threat.top(), None);
        threat.add(1, 10);
        threat.add(2, 15);
        threat.add(1, 10);
        assert_eq!(threat.top(), Some(1));
        threat.forget(1);
        assert_eq!(threat.top(), Some(2));
        threat.add(3, 15);
        assert_eq!(threat.top(), Some(2), "a tie goes to the lower number");
        threat.clear();
        assert_eq!(threat.top(), None);
    }

    #[test]
    fn the_same_seed_gives_the_same_dice() {
        let mut one = Dice::new(SEED);
        let mut two = Dice::new(SEED);
        let first: Vec<u32> = (0..10).map(|_| one.below(1000)).collect();
        let second: Vec<u32> = (0..10).map(|_| two.below(1000)).collect();
        assert_eq!(first, second);
        assert!(first.iter().all(|&roll| roll < 1000));
        assert_ne!(
            first,
            (0..10)
                .map(|_| Dice::new(SEED + 1).below(1000))
                .collect::<Vec<_>>()
        );
    }
}
