// SPDX-License-Identifier: AGPL-3.0-or-later
//! The hit reaction (stagger) of a player that a mob hits (player hit reaction study,
//! 2026-11-12; notes in `local-data/notes/2026-11-12-combat-left.md`).
//!
//! Observed (`playeractions_reaction.actionlist`, build 25810808), the two full-body actions the
//! server starts:
//!
//! - `Reaction`: a damaging hit (not a deflect) that is a blocked hit which took the last
//!   stamina (a guard break), an unblocked hit of power level 1 to 3, or a hit of level 0 when
//!   the grit broke; and no grit on with a value above 0 (unless stunned); and not a hit of level
//!   10 that may break out of a stun; and not `Uninterruptible` (unless the guard broke); and not
//!   `Frozen`.
//! - `Reaction_Knockdown`: an unblocked hit of power level 4 or more, not `Uninterruptible`, not
//!   `Frozen`.
//!
//! Both clear the attack, aim, ability and quick slot inputs on their start, take
//! `InstantSwap_Cooldown` off, put the sprint delay of the equip load on a melee hit that was not
//! blocked, hold the movement keys (`CharacterRelativeMovement` speed 0) and move the body by
//! the reaction's root motion (`ReactionMotion`). Their fragment is the first row of the player
//! reaction table (`playerreactiontableharness.actionlist`) whose power level and hit facts hold
//! ([`nw_content::player_attacks::Stance::reaction`]); they end at `IfFragmentDone`. The grid
//! (`player.grid`, `From="Reaction"`) lets the movement, the attacks, the dodge and the block
//! start again at the fragment's `CanTransition` marker, else at its end.
//!
//! - `UpperBodyReaction` (additive, holds nothing): a blocked hit that kept the guard, a hit of
//!   level 0 that broke no guard, a hit above level 0 while grit holds, or a hit of level 10
//!   while a reaction plays. It plays in the fourth slot (`super::slayer`, `upper_reaction`).
//!
//! A dodge breaks a reaction out (`Dodge_Stunlock`) once the reaction count reached its
//! breakout ([`ReactionCount`]). The level-10 breakout of `Reaction` (not `Equal 10` with
//! `IfCanBreakReaction StunToBreakout=1`) never decides: the same action takes only levels 1 to
//! 3 (Observed), so a hit of level 10 starts `Reaction_Knockdown` (`GreaterThanOrEqual 4`) with
//! the table's `r_R1` fragment, and in a reaction also the upper-body shake.

use std::sync::Arc;

use bevy_ecs::entity::Entity;
use nw_content::combat::ReactionSequence;
use nw_content::player_attacks::{AttackMotion, ReactionEntry, stance_key};
use nw_formats::mannequin::CageWorld;
use nw_formats::objectstream::lowercase_crc32;
use nw_sim_core::EquipLoad;

use super::attack::{Blow, INPUT_RECORD_MS, Vitals};
use super::gear::Gear;
use super::mob_timeline::HitSide;
use super::stamina::Guard;
use super::{Body, Player, Position, SimWorld};

/// The lowest power level whose unblocked hit knocks the player down (`Reaction_Knockdown`:
/// `IfDamagedByPowerLevel GreaterThanOrEqual 4`).
const KNOCKDOWN_POWER_LEVEL: u32 = 4;
/// The power levels whose unblocked hit staggers the player (`Reaction`: greater than 0, less
/// than 4).
const STAGGER_POWER_LEVELS: std::ops::RangeInclusive<u32> = 1..=3;
/// The power level whose hit staggers only once the grit broke (`Reaction`: `Equal 0` with
/// `GRIT_Broken`).
const GRIT_POWER_LEVEL: u32 = 0;
/// The power level of the breakout hit (`UpperBodyReaction`: `Equal 10` in a reaction).
const BREAKOUT_POWER_LEVEL: u32 = 10;
/// `StunToBreakout` of `Dodge_Stunlock` (`playeractions_dodge.actionlist`).
const DODGE_BREAKOUT: f32 = 2.0;

/// The external conditions of the player reaction table: what the engine knows of the hit
/// (the harness's `<Conditions>`).
mod facts {
    /// `blocked` and `damage_blocked`: `IfDamaged RequireDamageWasBlocked`.
    pub(super) const BLOCKED: &str = "blocked";
    pub(super) const DAMAGE_BLOCKED: &str = "damage_blocked";
    /// `last_hit_depleted_stamina`: `IfDamaged RequireDamageDepletedStamina` (a guard break).
    pub(super) const DEPLETED: &str = "last_hit_depleted_stamina";
    /// The side of the hit by the player's facing (`IfDamagedByAngle`: front within 45 degrees,
    /// back past 135, the sides between; [`super::HitSide`]).
    pub(super) const FRONT: &str = "damaged_front";
    pub(super) const BACK: &str = "damaged_back";
    pub(super) const LEFT: &str = "damaged_left";
    pub(super) const RIGHT: &str = "damaged_right";
    /// `GRIT_Broken`: grit on with a value of 0 or less.
    pub(super) const GRIT_BROKEN: &str = "GRIT_Broken";
    /// `Stagger_Resisted`: grit on with a value above 0, or `Uninterruptible`, or `Frozen`; and
    /// not stunned.
    pub(super) const STAGGER_RESISTED: &str = "Stagger_Resisted";
    /// `allow_additive`: no `AttackAction`, `AimDownSights`, `HipAim` or `Reaction` tag.
    pub(super) const ALLOW_ADDITIVE: &str = "allow_additive";
}

/// The status effect categories the reaction actions read (lowercase, as the rules hold them).
mod category {
    /// `IfHasStatusEffect StatusEffectCategory="Stun"`.
    pub(super) const STUN: &str = "stun";
    /// `Uninterruptible` (the named condition: `IfHasStatusEffect StatusEffectCategory`).
    pub(super) const UNINTERRUPTIBLE: &str = "uninterruptible";
    /// `IfHasStatusEffect StatusEffectCategory="Frozen"`.
    pub(super) const FROZEN: &str = "frozen";
}

/// The status effects the reaction actions change.
mod effect {
    /// Taken off on the start (`EffectsToRemoveOnEnter`).
    pub(super) const INSTANT_SWAP_COOLDOWN: &str = "InstantSwap_Cooldown";
    /// The stagger's sprint delays by equip load: fast, normal, slow.
    pub(super) const STAGGER_SPRINT_DELAYS: [&str; 3] = [
        "Sprint_Delay_Combat_Timer_Light",
        "Sprint_Delay_Combat_Timer_Medium",
        "Sprint_Delay_Combat_Timer_Heavy",
    ];
    /// The knockdown's: fast and normal (it has none for a slow load).
    pub(super) const KNOCKDOWN_SPRINT_DELAYS: [&str; 2] = [
        "Sprint_Delay_Combat_Timer_Light",
        "Sprint_Delay_Combat_Timer_Normal",
    ];
    /// The potency of an action's `EffectsToApplyOnEnter` effect.
    pub(super) const POTENCY: f64 = 1.0;
}

/// The player's grit: its `GritComponent` of `slices/player.dynamicslice` (Observed fields of
/// the client's reflection `FUN_1468e90f0`: `m_initMax` 10,000, `m_maxNoHitTime` 30,
/// `m_useStaminaAsValue` false, `m_setNoReactionWhenActive` true; `m_min` 1,000, whose use is
/// Unknown). Inferred: a hit while grit is on takes its stagger from the value, and the value is
/// whole again `m_maxNoHitTime` seconds after the last such hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Grit {
    value: f32,
    last_hit_ms: Option<u64>,
}

/// `m_initMax`.
const GRIT_MAX: f32 = 10_000.0;
/// `m_maxNoHitTime`, ms.
const GRIT_REFILL_AFTER_MS: u64 = 30_000;

impl Grit {
    /// A whole grit.
    pub(super) const FULL: Self = Self {
        value: GRIT_MAX,
        last_hit_ms: None,
    };

    /// Its value at `now_ms`.
    fn value_at(self, now_ms: u64) -> f32 {
        match self.last_hit_ms {
            Some(hit_ms) if now_ms.saturating_sub(hit_ms) < GRIT_REFILL_AFTER_MS => self.value,
            _ => GRIT_MAX,
        }
    }

    /// A hit of `stagger` at `now_ms` while grit is on: the value left.
    fn take(&mut self, stagger: f32, now_ms: u64) -> f32 {
        self.value = (self.value_at(now_ms) - stagger).max(0.0);
        self.last_hit_ms = Some(now_ms);
        self.value
    }
}

/// A player's reaction count (`IfCanBreakReaction StunToBreakout`). Observed (action lists of
/// build 25810808): `Dodge_Stunlock` (the dodge out of a reaction) needs `StunToBreakout=2`, the
/// only
/// `ResetReactionCount` is the dodge's (`Reset` on its start and its end), and the damage rows'
/// `StunBreakoutIncrement` cells are 0 to 1. Inferred: the count is the sum of the increments of
/// the hits that started a full-body reaction, and `StunToBreakout=n` holds from a count of
/// `n` on. Unknown: the client's test itself (the `re-sweep` holds only the readers of
/// `StunToBreakout`, `FUN_142bfd450`, and of `ResetReactionCount`, `FUN_142b52090`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(super) struct ReactionCount(f32);

impl ReactionCount {
    /// No hit since the last dodge.
    pub(super) const NONE: Self = Self(0.0);

    /// A full-body reaction of a hit of `increment` started.
    fn add(&mut self, increment: f32) {
        self.0 += increment;
    }

    /// Whether a dodge breaks the reaction out (`StunToBreakout=2`).
    pub(super) fn dodge_breaks_out(self) -> bool {
        self.0 >= DODGE_BREAKOUT
    }
}

/// A full-body hit reaction that holds the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PlayerReaction {
    /// When its keys may start the movement, the attacks, the dodge and the block again
    /// ([`reaction_times`]).
    pub(super) free_ms: u64,
}

/// Which full-body reaction action starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReactionAction {
    /// `Reaction`.
    Stagger,
    /// `Reaction_Knockdown`.
    Knockdown,
}

/// What the engine knows of one hit on a player: the reaction table's external conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HitFacts {
    blocked: bool,
    /// The block took the last stamina.
    depleted: bool,
    side: HitSide,
    /// Grit is on with a value above 0.
    grit_holds: bool,
    /// Grit is on with a value of 0.
    grit_broken: bool,
    stunned: bool,
    uninterruptible: bool,
    frozen: bool,
    /// A full-body reaction plays (the `Reaction` tag).
    reacting: bool,
    /// `allow_additive` holds: no attack, aim or full-body reaction plays.
    additive_allowed: bool,
}

impl HitFacts {
    /// The full-body action a hit of `power_level` starts; `None`: none (an upper-body shake
    /// or nothing).
    fn action(self, power_level: u32) -> Option<ReactionAction> {
        let not_blocked = !self.blocked;
        if power_level >= KNOCKDOWN_POWER_LEVEL
            && not_blocked
            && !self.uninterruptible
            && !self.frozen
        {
            return Some(ReactionAction::Knockdown);
        }
        let shakes = (self.blocked && self.depleted)
            || (not_blocked && STAGGER_POWER_LEVELS.contains(&power_level))
            || (power_level == GRIT_POWER_LEVEL && self.grit_broken);
        let unresisted = !self.grit_holds || self.stunned;
        let interruptible = !self.uninterruptible || (self.blocked && self.depleted);
        (shakes && unresisted && interruptible && !self.frozen).then_some(ReactionAction::Stagger)
    }

    /// Whether a hit of `power_level` that starts no full-body action plays the upper-body
    /// reaction (`UpperBodyReaction`'s activation).
    fn upper_body(self, power_level: u32) -> bool {
        let guard_broke = self.blocked && self.depleted;
        (self.blocked && !self.depleted)
            || (power_level == GRIT_POWER_LEVEL && !guard_broke)
            || (power_level > GRIT_POWER_LEVEL && self.grit_holds)
            || (power_level == BREAKOUT_POWER_LEVEL && self.reacting)
    }
}

impl CageWorld for HitFacts {
    fn tag(&self, _tag: &str) -> bool {
        false
    }

    fn marker(&self, _marker: &str) -> bool {
        false
    }

    fn input(&self, _input: &str) -> bool {
        false
    }

    fn external(&self, name: &str) -> bool {
        match name {
            facts::BLOCKED | facts::DAMAGE_BLOCKED => self.blocked,
            facts::DEPLETED => self.blocked && self.depleted,
            facts::FRONT => self.side == HitSide::Front,
            facts::BACK => self.side == HitSide::Back,
            facts::LEFT => self.side == HitSide::Left,
            facts::RIGHT => self.side == HitSide::Right,
            facts::GRIT_BROKEN => self.grit_broken,
            facts::STAGGER_RESISTED => {
                (self.grit_holds || self.uninterruptible || self.frozen) && !self.stunned
            }
            facts::ALLOW_ADDITIVE => self.additive_allowed,
            _ => false,
        }
    }
}

/// When a reaction that stuns for `stun_ms` ends on the wire (`16=2d`) and when its records
/// start states again, ms after the hit: its end state goes out in the record before the stun
/// is over (the hit's record counts as the stun's first), and the press of that end record
/// starts the attack (Observed, newchar 2026-09-18: `2960` of a `HitStun` 18 hit ended 17
/// records after its start, `17=1e13`, and the press in record 7699 started the press start in
/// the state after; the guard break `2125` ended 14 records after its start, its marker at 15
/// frames, and the press of the end record 11794 started the light attack).
fn reaction_times(stun_ms: u64) -> (u64, u64) {
    let frame = u64::from(INPUT_RECORD_MS);
    let end_after_ms = stun_ms.saturating_sub(frame);
    (end_after_ms, end_after_ms.saturating_sub(frame))
}

/// The motion of a reaction entry that holds the keys for `held_ms`: its root motion, else a
/// still body for that time (the keys stay held all the same).
fn reaction_motion(entry: &ReactionEntry, held_ms: u64) -> Arc<AttackMotion> {
    Arc::new(entry.motion.clone().unwrap_or_else(|| AttackMotion {
        // A reaction holds for a few seconds at most.
        #[allow(clippy::cast_precision_loss)]
        step_ms: held_ms as f32,
        offsets: vec![[0.0; 2]; 2],
        inherit_velocity: false,
        inherit_max_mps: None,
        decel_mps2: [0.0; 2],
    }))
}

impl SimWorld {
    /// Plays the hit reaction of the hit `blow` of `attacker` (a mob, or a player's PvP hit)
    /// that landed on player `entity` at `now_ms`, which `guard` met
    /// ([`super::attack::SimWorld::defend`]). A full-body reaction: the reaction state on the
    /// wire ([`super::slayer::SlayerTrack::reacted`]), the held keys and the motion, the
    /// dropped combo step and the cancelled quick slot use, the sprint delay, the reaction count
    /// ([`ReactionCount`]) and, for a player who blocked, no stamina regen until its end. An
    /// upper-body reaction: the fourth slot ([`super::slayer::SlayerTrack::shook`]) and the
    /// sprint delay; it holds nothing. The entry's sequence is its mined one, else the nearest
    /// mined entry's (Inferred, [`nw_content::combat::CombatData::reaction_sequence`]). A hit
    /// with no reaction entry in the stance (no player database has one) plays none (logged).
    pub(super) fn react_to_hit(
        &mut self,
        entity: Entity,
        attacker: u64,
        blow: Blow,
        guard: Option<Guard>,
        now_ms: u64,
    ) {
        let data = Arc::clone(&self.combat.data);
        let [x, y, z] = self.attacker_position(attacker);
        let from = Position { x, y, z };
        let rules = &self.content.status_effects;
        let Some(player) = self.world.get::<Player>(entity) else {
            return;
        };
        let number = player.number;
        let held = player.status_effects.held_view(rules);
        let has = |name: &str| held.categories.iter().any(|own| own == name);
        let (stunned, uninterruptible, frozen) = (
            has(category::STUN) || held.control.stunned,
            has(category::UNINTERRUPTIBLE),
            has(category::FROZEN),
        );
        let attacking = player.attack_playing.is_some();
        let grit_on = player
            .attack_playing
            .as_ref()
            .is_some_and(|playing| playing.grit_on(now_ms));
        let facing = player.view.map(|view| view.yaw);
        // The stance of the drawn weapon; a sheathed one plays the reactions of no weapon.
        let drawn = player.gear.as_ref().and_then(Gear::drawn).is_some();
        let stance_key = if drawn {
            self.held_weapon(entity)
                .map_or(data.weapon().stance, |(weapon, _)| weapon.stance)
        } else {
            stance_key("")
        };
        let Some(stance) = data.stance(stance_key) else {
            tracing::debug!(player = number, stance_key, "no stance for the reaction");
            return;
        };
        let at = self
            .world
            .get::<Body>(entity)
            .map(|body| body.motion.position());
        let side = at.map_or(HitSide::Front, |at| HitSide::of(at, facing, from));
        let Some(mut vitals) = self.world.get_mut::<Vitals>(entity) else {
            return;
        };
        let grit_left = grit_on.then(|| vitals.grit.take(blow.stagger, now_ms));
        let reacting = vitals.reaction.is_some();
        // A ranged stance aims with the right-click (`AimDownSights`, Inferred: the key the
        // aimed attack reads).
        let aiming = vitals.block_key && stance.aimed.is_some();
        let hit = HitFacts {
            blocked: guard.is_some(),
            depleted: guard == Some(Guard::Broken),
            side,
            grit_holds: grit_left.is_some_and(|left| left > 0.0),
            grit_broken: grit_left.is_some_and(|left| left <= 0.0),
            stunned,
            uninterruptible,
            frozen,
            reacting,
            additive_allowed: !attacking && !aiming && !reacting,
        };
        let action = hit.action(blow.power_level);
        if action.is_none() && !hit.upper_body(blow.power_level) {
            return;
        }
        let Some(row) = stance.reaction(blow.power_level, |test| test.holds_in(&hit)) else {
            tracing::debug!(
                player = number,
                power_level = blow.power_level,
                ?side,
                "no reaction row"
            );
            return;
        };
        let Some(entry) = row.entry.as_ref() else {
            tracing::debug!(
                player = number,
                fragment = row.fragment,
                "reaction without an entry"
            );
            return;
        };
        let Some(ReactionSequence { sequence, inferred }) =
            data.reaction_sequence(stance_key, stance, row)
        else {
            tracing::debug!(
                player = number,
                fragment = row.fragment,
                animations = entry.animations,
                "reaction without a sequence"
            );
            return;
        };
        let melee_unblocked = !blow.ranged && guard.is_none();
        let Some(action) = action else {
            // `UpperBodyReaction`: its end goes out a record before the fragment is over.
            let (end_after_ms, _) = reaction_times(entry.length_ms);
            tracing::debug!(
                player = number,
                fragment = row.fragment,
                sequence,
                inferred,
                end_after_ms,
                "upper-body hit reaction"
            );
            let Some(mut player) = self.world.get_mut::<Player>(entity) else {
                return;
            };
            player.slayer.shook(sequence, now_ms, end_after_ms);
            if melee_unblocked {
                self.add_sprint_delay(entity, attacker, &effect::STAGGER_SPRINT_DELAYS, now_ms);
            }
            return;
        };
        let (end_after_ms, free_after_ms) = reaction_times(entry.stun_ms(blow.hit_stun));
        vitals.reaction = Some(PlayerReaction {
            free_ms: now_ms.saturating_add(free_after_ms),
        });
        vitals.reaction_count.add(blow.breakout);
        // `ClearInput`: the attack key's charge and buffered step, a waiting dodge.
        vitals.drop_attack_keys();
        tracing::debug!(
            player = number,
            fragment = row.fragment,
            sequence,
            inferred,
            free_after_ms,
            end_after_ms,
            "hit reaction"
        );
        let motion = reaction_motion(entry, free_after_ms);
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let player = &mut *player;
        player.attack_playing = None;
        player
            .slayer
            .reacted(sequence, now_ms, free_after_ms, end_after_ms);
        // A fragment's time is a few seconds of milliseconds.
        #[allow(clippy::cast_precision_loss)]
        player.tracked.start_reaction(motion, free_after_ms as f32);
        player
            .status_effects
            .remove(lowercase_crc32(effect::INSTANT_SWAP_COOLDOWN));
        if action == ReactionAction::Stagger && guard.is_some() {
            // `BlockingReaction`: the player blocked as the hit came (its `Blocking` tag).
            player
                .stamina
                .hold_regen_until(now_ms.saturating_add(end_after_ms));
        }
        let connection = player.connection.clone();
        if melee_unblocked {
            let delays: &[&str] = match action {
                ReactionAction::Stagger => &effect::STAGGER_SPRINT_DELAYS,
                ReactionAction::Knockdown => &effect::KNOCKDOWN_SPRINT_DELAYS,
            };
            self.add_sprint_delay(entity, attacker, delays, now_ms);
        }
        // `ClearInput` of the quick slots: a use not taken yet is dropped; the attack that ran
        // stops before its hit.
        self.drinks.retain(|(drinker, _, _)| *drinker != number);
        self.player_swings
            .retain(|swing| !swing.is_of(&connection) || swing.opened(now_ms));
    }

    /// Puts on player `entity` the sprint delay of `delays` (fast, normal, slow) for its equip
    /// load (none when overburdened, or when `delays` has none for its load), from `attacker`'s
    /// hit at `now_ms`.
    fn add_sprint_delay(&mut self, entity: Entity, attacker: u64, delays: &[&str], now_ms: u64) {
        let rules = &self.content.status_effects;
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let delay = match player.tracked.equip_load() {
            EquipLoad::Fast => delays.first(),
            EquipLoad::Normal => delays.get(1),
            EquipLoad::Slow => delays.get(2),
            EquipLoad::Overburdened => None,
        };
        let number = player.number;
        if let Some(delay) = delay
            && !player
                .status_effects
                .add_by_id(rules, delay, effect::POTENCY, attacker, now_ms)
        {
            tracing::debug!(player = number, delay, "sprint delay not in the data");
        }
    }

    /// Lets player `entity` act again when its hit reaction reached its free point by `now_ms`
    /// (each input message asks before its keys count).
    pub(super) fn end_free_reaction(&mut self, entity: Entity, now_ms: u64) {
        if let Some(mut vitals) = self.world.get_mut::<Vitals>(entity)
            && vitals
                .reaction
                .is_some_and(|reaction| now_ms >= reaction.free_ms)
        {
            vitals.reaction = None;
        }
    }
}

/// The great sword's reaction rows that the tutorial's hits pick, for the tests.
#[cfg(test)]
pub(super) mod test_support {
    use nw_content::combat::CombatData;
    use nw_content::player_attacks::{HitReaction, PlayerAttacks, ReactionEntry, Stance};
    use nw_content::player_sequences::ReactionPlay;
    use nw_formats::mannequin::Condition;

    use super::facts;

    /// One row: power level (`None`: every level), fragment, facts, the entry's animations,
    /// length and marker, and its retail sequence (`player-attacks-25810808.json` and the mined
    /// sequence table, 2026-11-12).
    type Row = (
        Option<u32>,
        &'static str,
        &'static [&'static str],
        &'static str,
        u64,
        Option<u64>,
        u64,
    );

    /// The great sword's rows of the guard break, the held block, the front reactions of levels
    /// 1 to 4 and the level 0 shake (the upper-body sequences Observed in newchar 2026-09-18,
    /// [`super::super::slayer`] `upper_reaction`).
    pub(crate) const GREATSWORD_ROWS: [Row; 7] = [
        (
            None,
            "r_B0_Break",
            &[facts::BLOCKED, facts::DEPLETED],
            "grtswrd_block_break",
            1_167,
            Some(500),
            2125,
        ),
        (
            None,
            "r_B0",
            &[facts::BLOCKED],
            "grtswrd_b0_f",
            667,
            None,
            1898,
        ),
        (
            Some(1),
            "r_R1_F",
            &[facts::FRONT],
            "unarmed_r1_f+grtswrd_r1_f",
            567,
            None,
            3446,
        ),
        (
            Some(2),
            "r_R2_F",
            &[facts::FRONT],
            "unarmed_r2_f+grtswrd_r2_f",
            800,
            None,
            2960,
        ),
        (
            Some(3),
            "r_R3_F",
            &[facts::FRONT],
            "unarmed_r3_f+grtswrd_r3_f",
            1_000,
            None,
            3528,
        ),
        (
            Some(4),
            "r_R4_F",
            &[facts::FRONT],
            "unarmed_r4_f+unarmed_prn_idle_to_idle",
            2_467,
            Some(1_667),
            3486,
        ),
        (
            Some(0),
            "r_R0_F",
            &[facts::FRONT, facts::ALLOW_ADDITIVE],
            "unarmed_r0_f",
            567,
            None,
            3317,
        ),
    ];

    /// `data` with [`GREATSWORD_ROWS`] in every stance of the test attack table and their
    /// sequences in the sequence table.
    pub(crate) fn with_reactions(data: CombatData) -> CombatData {
        let rows: Vec<HitReaction> = GREATSWORD_ROWS
            .iter()
            .map(
                |&(power_level, fragment, tests, animations, length_ms, can_transition_ms, _)| {
                    HitReaction {
                        power_level,
                        fragment: fragment.to_owned(),
                        condition: Condition::All(
                            tests
                                .iter()
                                .map(|name| Condition::External((*name).to_owned()))
                                .collect(),
                        ),
                        entry: Some(ReactionEntry {
                            animations: animations.to_owned(),
                            length_ms,
                            can_transition_ms,
                            motion: None,
                        }),
                    }
                },
            )
            .collect();
        let stances: Vec<Stance> =
            serde_json::from_str(crate::combat::test_support::TEST_PLAYER_ATTACKS)
                .expect("player attack table");
        let stances = stances
            .into_iter()
            .map(|stance| Stance {
                reactions: rows.clone(),
                ..stance
            })
            .collect();
        let sequences = crate::combat::test_support::test_player_sequences().with_reactions(
            GREATSWORD_ROWS
                .iter()
                .map(|&(_, _, _, animations, _, _, sequence)| ReactionPlay {
                    animations: animations.to_owned(),
                    sequence,
                })
                .collect(),
        );
        data.with_player_attacks(PlayerAttacks::of_stances(stances))
            .with_player_sequences(sequences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nw_formats::mannequin::Condition;

    /// A front hit that no block, grit or effect met.
    const PLAIN: HitFacts = HitFacts {
        blocked: false,
        depleted: false,
        side: HitSide::Front,
        grit_holds: false,
        grit_broken: false,
        stunned: false,
        uninterruptible: false,
        frozen: false,
        reacting: false,
        additive_allowed: true,
    };

    #[test]
    fn the_upper_body_reaction_starts_as_the_action_list_says() {
        assert!(PLAIN.upper_body(0), "a level 0 hit");
        assert!(!PLAIN.upper_body(2), "a stagger's level is no shake");
        let blocked = HitFacts {
            blocked: true,
            ..PLAIN
        };
        assert!(blocked.upper_body(2), "a block that held");
        assert!(
            !HitFacts {
                depleted: true,
                ..blocked
            }
            .upper_body(0),
            "a guard break is the full reaction"
        );
        assert!(
            HitFacts {
                grit_holds: true,
                ..PLAIN
            }
            .upper_body(3),
            "grit holds the stagger to a shake"
        );
        let reacting = HitFacts {
            reacting: true,
            ..PLAIN
        };
        assert!(reacting.upper_body(BREAKOUT_POWER_LEVEL));
        assert!(!PLAIN.upper_body(BREAKOUT_POWER_LEVEL));
        assert_eq!(
            PLAIN.action(BREAKOUT_POWER_LEVEL),
            Some(ReactionAction::Knockdown),
            "level 10 is past the knockdown's level"
        );
        assert!(
            !HitFacts {
                additive_allowed: false,
                ..PLAIN
            }
            .external(facts::ALLOW_ADDITIVE)
        );
        assert!(PLAIN.external(facts::ALLOW_ADDITIVE));
    }

    #[test]
    fn the_reaction_count_lets_the_dodge_break_out_from_two() {
        let mut count = ReactionCount::NONE;
        count.add(1.0);
        count.add(0.5);
        assert!(!count.dodge_breaks_out());
        count.add(0.5);
        assert!(count.dodge_breaks_out());
    }

    #[test]
    fn the_reaction_actions_start_as_the_action_list_says() {
        assert_eq!(PLAIN.action(0), None, "level 0 shakes the upper body only");
        assert_eq!(PLAIN.action(2), Some(ReactionAction::Stagger));
        assert_eq!(PLAIN.action(4), Some(ReactionAction::Knockdown));
        let blocked = HitFacts {
            blocked: true,
            ..PLAIN
        };
        assert_eq!(blocked.action(2), None, "a held block");
        assert_eq!(blocked.action(5), None, "no knockdown through a block");
        let broken = HitFacts {
            depleted: true,
            ..blocked
        };
        assert_eq!(
            broken.action(0),
            Some(ReactionAction::Stagger),
            "a guard break"
        );
        let gritty = HitFacts {
            grit_holds: true,
            ..PLAIN
        };
        assert_eq!(gritty.action(3), None, "grit holds the stagger");
        assert_eq!(
            HitFacts {
                stunned: true,
                ..gritty
            }
            .action(3),
            Some(ReactionAction::Stagger),
            "not while stunned"
        );
        assert_eq!(
            gritty.action(4),
            Some(ReactionAction::Knockdown),
            "grit holds no knockdown"
        );
        let grit_broken = HitFacts {
            grit_broken: true,
            ..PLAIN
        };
        assert_eq!(grit_broken.action(0), Some(ReactionAction::Stagger));
        let uninterruptible = HitFacts {
            uninterruptible: true,
            ..PLAIN
        };
        assert_eq!(uninterruptible.action(2), None);
        assert_eq!(uninterruptible.action(4), None);
        assert_eq!(
            HitFacts {
                uninterruptible: true,
                ..broken
            }
            .action(0),
            Some(ReactionAction::Stagger),
            "a guard break interrupts anyway"
        );
        assert_eq!(
            HitFacts {
                frozen: true,
                ..PLAIN
            }
            .action(2),
            None
        );
    }

    #[test]
    fn the_hit_facts_answer_the_reaction_tables_conditions() {
        let left = HitFacts {
            side: HitSide::Left,
            ..PLAIN
        };
        let holds =
            |facts: &HitFacts, name: &str| Condition::External(name.to_owned()).holds_in(facts);
        assert!(holds(&left, facts::LEFT));
        assert!(!holds(&left, facts::FRONT));
        let broken = HitFacts {
            blocked: true,
            depleted: true,
            ..PLAIN
        };
        assert!(holds(&broken, facts::BLOCKED) && holds(&broken, facts::DEPLETED));
        assert!(holds(&broken, facts::DAMAGE_BLOCKED));
        assert!(!holds(&PLAIN, facts::DEPLETED));
        let gritty = HitFacts {
            grit_holds: true,
            ..PLAIN
        };
        assert!(holds(&gritty, facts::STAGGER_RESISTED));
        assert!(!holds(
            &HitFacts {
                stunned: true,
                ..gritty
            },
            facts::STAGGER_RESISTED
        ));
        assert!(holds(&PLAIN, facts::ALLOW_ADDITIVE));
        assert!(!holds(&PLAIN, "unknown_condition"));
    }

    #[test]
    fn grit_takes_each_hits_stagger_and_is_whole_again_after_a_quiet_spell() {
        const STAGGER: f32 = 4_000.0;
        let mut grit = Grit::FULL;
        assert!((grit.take(STAGGER, 0) - (GRIT_MAX - STAGGER)).abs() < f32::EPSILON);
        assert!(grit.take(STAGGER, 1_000) > 0.0);
        assert!(grit.take(STAGGER, 2_000) <= 0.0, "the third breaks it");
        assert!(
            (grit.take(STAGGER, 2_000 + GRIT_REFILL_AFTER_MS) - (GRIT_MAX - STAGGER)).abs()
                < f32::EPSILON,
            "whole again after the no-hit time"
        );
    }

    #[test]
    fn a_stun_of_18_frames_ends_in_its_17th_record_whose_keys_start_again() {
        let (end_after_ms, free_after_ms) = reaction_times(600);
        assert_eq!(end_after_ms, 567);
        assert_eq!(free_after_ms, 534);
    }

    #[test]
    fn a_reaction_without_root_motion_holds_the_body_still_for_its_length() {
        let entry = ReactionEntry {
            animations: "unarmed_r2_f".to_owned(),
            length_ms: 800,
            can_transition_ms: None,
            motion: None,
        };
        let motion = reaction_motion(&entry, 600);
        assert!((motion.length_ms() - 600.0).abs() < f32::EPSILON);
        assert_eq!(motion.offset_at(300.0), [0.0; 2]);
    }
}
