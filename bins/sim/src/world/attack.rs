// SPDX-License-Identifier: AGPL-3.0-or-later
//! Attacks, hits and player health (M6-A spec sections 7 and 8).
//!
//! [`SimWorld::attack`] is the one attack of a player: it checks the attack (parent spec section
//! 9: range and attack rate), rolls the hit, and applies it with [`SimWorld::apply_hit`], which
//! mob attacks and hits from other sims use too. A player at 0 health is dead: it cannot move or
//! attack, every mob forgets it, its character is saved at once, and it lives again with full
//! health where it died when its client asks (M6-B spec section 8).
//!
//! Hits and deaths send messages (M6-B spec section 7): a hit made sends the attacker a 2071, a
//! hit applied on a player sends it a 3601, and a death a 4299. A mob's melee hit on a player
//! sends it a 2218; while the player holds block, the hit does no damage and the 2218 says so
//! (live-tutorial note, section 7). They wait in the world until
//! [`SimWorld::take_messages`] or the tick takes them.
//!
//! [`SimWorld::attack_input`] turns the attack bits of a client's input records into attacks on
//! the nearest mob (M6-B spec section 6).

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use nw_bus::{BusId, ConnectionRef, Hit};
use std::sync::Arc;

use nw_content::combat::{AttackRow, WeaponKind, WeaponRow};
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::paperdoll::slot;
use nw_messages::{
    AdjustFxScriptsMsg, ConsumableCooldown, DamageDealtMsg, DeathRecapMsg, InputRecord,
    MeleeDamage, MeleeHitMsg, OnDamageMsg, ServerMessage, SpreadshotProcessedMsg,
    SpreadshotRequestMsg,
};
use nw_sim_core::{Damage, Health, MobState, PlayerAttack, died, hurt, mob_hit, player_hit};

use super::gear::unix_ns;
use super::hit_shape::{MOB_SWING_HALF_ARC_RAD, PLAYER_CAPSULE, Wedge, homing, swing_shape, turn};
use super::mob_timeline::yaw_toward;
use super::stamina::Dodge;
use super::{Body, MESSAGE_CHANNEL, Outgoing, Player, Position, SimWorld};
use crate::combat::{AttackOutcome, AttackRefusal, RemoteHit, RespawnRefusal};
use crate::mobs::{Capsule, MobAttackIntent, MoveHit, MoveStrike, spawn_index};

/// How high above the sea a player still swims (Inferred: retail swam at 0.0 to 1.55 m above the
/// tutorial's sea and walked onto the beach at 3.5 m).
const SWIM_HEIGHT_M: f32 = 1.6;
/// How long a dodge roll keeps mob hits off (Inferred: no retail hit message fell inside a roll;
/// the roll length is Unknown; xp-potion-exit note, section 5.4).
const DODGE_MS: u64 = 500;

/// The fit of the damage share that a mob's armor takes off a player's hit (xp-potion-exit note,
/// section 4.2, Inferred from two points: retail tutorial hits kept 0.7011 with a gear score 60
/// weapon and 0.7541 with gear score 75, on level 1 mobs of armor rating 109.336):
/// `armor / (armor + SCALE × gear score ^ POWER)`.
const ARMOR_FIT_SCALE: f64 = 1.8748;
const ARMOR_FIT_POWER: f64 = 1.2013;

/// The share of a hit that a mob of armor rating `armor` takes off a weapon of `gear_score`.
fn armor_share(armor: f64, gear_score: u32) -> f64 {
    armor / (armor + ARMOR_FIT_SCALE * f64::from(gear_score).powf(ARMOR_FIT_POWER))
}

/// How long after the press a potion works (retail: the answer came 326 ms after the press;
/// Inferred: the drink animation).
const DRINK_MS: u64 = 326;
/// The visual script of a heal: crc32 of `potionheal` (retail 608 after the potion).
const HEAL_SCRIPT: u32 = 0xf7e1_e0d6;
/// Microseconds per millisecond.
const US_PER_MS: u64 = 1000;
/// Milliseconds per second.
const MS_PER_SECOND: u64 = 1000;

/// A player's health and attack times.
#[derive(Component, Debug, Clone, Copy)]
pub(super) struct Vitals {
    pub(super) health: Health,
    pub(super) dead_since_ms: Option<u64>,
    /// Spawn protection: `false` until the player's client is in the world. A fresh login starts
    /// `false`; the first movement the client sends ([`SimWorld::move_to`]) sets it `true`. While
    /// it is `false`, mobs cannot hit the player ([`SimWorld::mob_hits`]), so an idle character
    /// waiting at the loading screen is not killed before it spawns. A handed-off player keeps
    /// `true`.
    pub(super) spawned: bool,
    last_attack_ms: Option<u64>,
    /// The counter of the record whose attack key press still waits for its release.
    attack_pressed_at: Option<u16>,
    /// The counter (`u16` after the mask) of the newest input record read.
    input_counter: Option<u16>,
    /// The mob the player locked on in its newest input.
    lock_target: Option<u64>,
    /// When the player last fired a spread shot.
    last_shot_ms: Option<u64>,
    /// The player holds block (the newest input record, [`ClientInputMsg::blocking`]).
    ///
    /// [`ClientInputMsg::blocking`]: nw_messages::ClientInputMsg::blocking
    blocking: bool,
    /// The player held the dodge key in the newest input record.
    dodge_held: bool,
    /// The player pressed quick slot 1 in the newest input record.
    quick_slot_held: bool,
    /// A dodge roll runs until this time: mob hits miss.
    dodging_until_ms: u64,
}

impl Vitals {
    pub(super) const fn new(health: Health, spawned: bool) -> Self {
        Self {
            health,
            dead_since_ms: None,
            spawned,
            last_attack_ms: None,
            attack_pressed_at: None,
            input_counter: None,
            lock_target: None,
            last_shot_ms: None,
            blocking: false,
            dodge_held: false,
            quick_slot_held: false,
            dodging_until_ms: 0,
        }
    }
}

/// A health amount as the `f32` of the wire (hit messages, the vitals block).
#[allow(clippy::cast_precision_loss, reason = "health stays far below 2^24")]
pub(super) const fn amount_of(amount: u32) -> f32 {
    amount as f32
}

/// The most mobs one swing hits (our choice; retail's limit is Unknown, one retail swing hit
/// two).
const MAX_SWING_TARGETS: usize = 5;

/// The shortest time between two spread shots (our choice: retail's shortest gap was 266 ms).
const SPREADSHOT_MIN_INTERVAL_MS: u64 = 250;
/// How far a pellet reaches (our choice; the blunderbuss range is Unknown, and the client names
/// the targets it hit).
const SPREADSHOT_RANGE_M: f32 = 30.0;
/// The pellets of a shot that uses the eight-pellet config.
const EIGHT_PELLETS: usize = 8;
/// The pellet config id of the answer on six-pellet shots (ranged note).
const SIX_PELLET_CONFIG: u64 = 230;
/// The pellet config id of the answer on eight-pellet shots (ranged note).
const EIGHT_PELLET_CONFIG: u64 = 231;

/// A mob swing that has not hit yet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PendingMobSwing {
    /// When it hits.
    lands_ms: u64,
    /// The server time it started.
    started_ms: u64,
    swing: MobAttackIntent,
    /// The hit of its move, when the mob has moves.
    hit: Option<MoveHit>,
}

/// Where a player aims a swing.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Aim {
    /// The camera yaw of the input that swung.
    pub facing: Option<f32>,
    /// The mob the player locked on (the input's target row).
    pub lock_target: Option<u64>,
}

/// A player swing that has not hit yet.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PlayerSwing {
    attacker: ConnectionRef,
    aim: Aim,
    kind: AttackKind,
    /// When it hits.
    due_ms: u64,
}

/// Milliseconds per client input record (retail: 34 ms apart, median; input note).
const INPUT_RECORD_MS: u16 = 33;
/// The shortest hold of the attack key that is a heavy attack (input note: retail light holds
/// 35 to 314 ms, heavy holds from 432 ms; the limit between them is our choice).
const HEAVY_HOLD_MS: u16 = 400;

/// A player's weapon attack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackKind {
    /// A tap of the attack key.
    Light,
    /// A long hold of the attack key.
    Heavy,
}

impl AttackKind {
    /// The attack of a key held for `records` input records.
    const fn of_hold(records: u16) -> Self {
        if records.saturating_mul(INPUT_RECORD_MS) >= HEAVY_HOLD_MS {
            Self::Heavy
        } else {
            Self::Light
        }
    }
}

/// Whether input counter `counter` comes after `last`; the counter wraps at `u16::MAX`.
const fn counter_is_newer(counter: u16, last: u16) -> bool {
    (counter.wrapping_sub(last) as i16) > 0
}

/// One hit a player lands: who, on what, with which weapon and attack row.
struct Strike<'a> {
    entity: Entity,
    number: u64,
    attacker: &'a ConnectionRef,
    target: u64,
    view: &'a TargetView,
    /// The weapon in the hand and its gear score; `None`: the config's weapon.
    held: Option<(WeaponRow, u32)>,
    row: &'a AttackRow,
}

/// What an attack sees of its target.
struct TargetView {
    position: Position,
    dead: bool,
    returning: bool,
    /// Its level, for its armor.
    level: u32,
    /// The sim of a ghost target; `None` when the target is here.
    owner: Option<BusId>,
}

impl SimWorld {
    /// The player of `attacker` attacks `target` with its weapon's attack `damage_id`.
    ///
    /// # Errors
    ///
    /// Returns [`AttackRefusal`] when a check refuses the attack; nothing changes then.
    pub fn attack(
        &mut self,
        attacker: &ConnectionRef,
        target: u64,
        damage_id: &str,
        now_ms: u64,
    ) -> Result<AttackOutcome, AttackRefusal> {
        let reach_m = self.combat.brain.attack_reach_m + self.combat.reach_margin_m;
        self.hit(attacker, target, damage_id, Some(reach_m), false, now_ms)
    }

    /// The player of `attacker` hits `target` with its weapon's attack `damage_id`: `reach_m`
    /// caps the distance (`None`: a hit shape checked it); the attack interval since its last
    /// hit applies unless the hit is a later target of the same swing (`same_swing`).
    fn hit(
        &mut self,
        attacker: &ConnectionRef,
        target: u64,
        damage_id: &str,
        reach_m: Option<f32>,
        same_swing: bool,
        now_ms: u64,
    ) -> Result<AttackOutcome, AttackRefusal> {
        let entity = *self
            .by_connection
            .get(attacker)
            .ok_or(AttackRefusal::NotInWorld)?;
        let number = self
            .world
            .get::<Player>(entity)
            .ok_or(AttackRefusal::NotInWorld)?
            .number;
        let position = self
            .world
            .get::<Body>(entity)
            .ok_or(AttackRefusal::NotInWorld)?
            .motion
            .position();
        let vitals = *self
            .world
            .get::<Vitals>(entity)
            .ok_or(AttackRefusal::NotInWorld)?;
        if vitals.health.is_dead() {
            return Err(AttackRefusal::Dead);
        }
        if spawn_index(target).is_none() {
            return Err(AttackRefusal::UnknownTarget);
        }
        let view = self
            .target_view(target)
            .ok_or(AttackRefusal::UnknownTarget)?;
        if view.dead {
            return Err(AttackRefusal::TargetDead);
        }
        if view.returning {
            return Err(AttackRefusal::Returning);
        }
        let row = self
            .combat
            .data
            .attack(damage_id)
            .ok_or(AttackRefusal::NotWeaponAttack)?;
        if reach_m.is_some_and(|reach_m| view.position.horizontal_distance(position) > reach_m) {
            return Err(AttackRefusal::OutOfReach);
        }
        let too_fast = !same_swing
            && vitals.last_attack_ms.is_some_and(|last| {
                now_ms.saturating_sub(last) < self.combat.min_attack_interval_ms
            });
        if too_fast {
            return Err(AttackRefusal::TooFast);
        }
        // The weapon in the hand, else the config's; its gear score sets the armor share.
        let held = self.held_weapon(entity);
        let row = row.clone();
        let outcome = self.strike(
            Strike {
                entity,
                number,
                attacker,
                target,
                view: &view,
                held,
                row: &row,
            },
            now_ms,
            |amount, _, killed, _| DamageDealtMsg::hit(number, target, amount, killed),
        )?;
        Ok(outcome)
    }

    /// Lands one hit of `strike` at `now_ms`: rolls the damage of its weapon (else the config's)
    /// and row against the target's level, applies it here or sends it to the target's sim, and
    /// tells the attacker with the message `dealt` makes of the amount, the crit, the kill and the
    /// share of the damage the armor let through. A mob that absorbs hits (Thorpe in his
    /// intermission) takes none.
    fn strike(
        &mut self,
        strike: Strike<'_>,
        now_ms: u64,
        dealt: impl Fn(f32, bool, bool, f32) -> DamageDealtMsg,
    ) -> Result<AttackOutcome, AttackRefusal> {
        let Strike {
            entity,
            number,
            attacker,
            target,
            view,
            held,
            row,
        } = strike;
        let weapon = held.map_or_else(|| *self.combat.data.weapon(), |(weapon, _)| weapon);
        let armor_share = held
            .zip(self.combat.data.level(view.level))
            .map_or(self.combat.armor_share, |((_, gear_score), level)| {
                armor_share(level.physical_armor_rating, gear_score)
            });
        let attack = PlayerAttack {
            base_damage: weapon.base_damage,
            coefficient: row.coefficient,
            can_crit: row.can_crit,
            crit_chance: weapon.crit_chance,
            crit_multiplier: weapon.crit_multiplier,
        };
        if let Some(mut vitals) = self.world.get_mut::<Vitals>(entity) {
            vitals.last_attack_ms = Some(now_ms);
        }
        if view.owner.is_none() && self.mobs.absorbing(target, now_ms) {
            if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                player.combat.hit(now_ms, false);
            }
            self.send_reliable(
                attacker.clone(),
                &ServerMessage::DamageDealt(DamageDealtMsg::absorbed(number, target)),
            );
            return Ok(AttackOutcome::Hit { target, amount: 0 });
        }
        let hit = player_hit(&attack, armor_share, &mut self.dice);
        let threat = row.threat.saturating_add(u64::from(hit.amount));
        let outcome = match &view.owner {
            None => self
                .apply_hit(target, number, hit.amount, threat, now_ms)
                .ok_or(AttackRefusal::TargetDead)?,
            Some(owner) => AttackOutcome::Remote(self.remote_hit(
                owner.clone(),
                target,
                number,
                hit.amount,
                threat,
            )),
        };
        let killed = matches!(outcome, AttackOutcome::Killed { .. });
        if matches!(outcome, AttackOutcome::Hit { .. }) {
            self.mob_timelines.hurt(target);
        }
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.combat.hit(now_ms, killed);
        }
        self.send_reliable(
            attacker.clone(),
            &ServerMessage::DamageDealt(dealt(
                amount_of(hit.amount),
                hit.crit,
                killed,
                (1.0 - armor_share) as f32,
            )),
        );
        Ok(outcome)
    }

    /// The player of `shooter` fires the spread shot `request` at `now_ms` with the spread-shot
    /// weapon it wears (ranged note): the answer goes to the shooter at once, then each pellet that
    /// hit a living mob in range lands as one hit. Retail trusts the targets the client names.
    pub fn spreadshot(
        &mut self,
        shooter: &ConnectionRef,
        request: &SpreadshotRequestMsg,
        now_ms: u64,
    ) -> Vec<Result<AttackOutcome, AttackRefusal>> {
        let refused = |refusal| vec![Err(refusal)];
        let Some(entity) = self.by_connection.get(shooter).copied() else {
            return refused(AttackRefusal::NotInWorld);
        };
        let (Some(player), Some(body), Some(vitals)) = (
            self.world.get::<Player>(entity),
            self.world.get::<Body>(entity),
            self.world.get::<Vitals>(entity),
        ) else {
            return refused(AttackRefusal::NotInWorld);
        };
        if vitals.health.is_dead() {
            return refused(AttackRefusal::Dead);
        }
        if vitals
            .last_shot_ms
            .is_some_and(|last| now_ms.saturating_sub(last) < SPREADSHOT_MIN_INTERVAL_MS)
        {
            return refused(AttackRefusal::TooFast);
        }
        let number = player.number;
        let position = body.motion.position();
        let data = Arc::clone(&self.combat.data);
        let held = player.gear.as_ref().and_then(|gear| {
            let (item, gear_score) = gear.main_weapon()?;
            let weapon = *data.weapon_of_item(item)?;
            weapon.spread_shot.then_some((weapon, gear_score))
        });
        let Some(held) = held else {
            return refused(AttackRefusal::NotWeaponAttack);
        };
        let fired = self
            .world
            .get_mut::<Player>(entity)
            .and_then(|mut player| Some(player.gear.as_mut()?.fire(now_ms)))
            .unwrap_or(false);
        if !fired {
            return refused(AttackRefusal::NoAmmo);
        }
        if let Some(mut vitals) = self.world.get_mut::<Vitals>(entity) {
            vitals.last_shot_ms = Some(now_ms);
        }
        let row = data.spreadshot_attack();
        let config = if request.pellets.len() == EIGHT_PELLETS {
            EIGHT_PELLET_CONFIG
        } else {
            SIX_PELLET_CONFIG
        };
        self.send_reliable(
            shooter.clone(),
            &ServerMessage::SpreadshotProcessed(SpreadshotProcessedMsg {
                request: request.clone(),
                damage: lowercase_crc32(&row.damage_id),
                config,
            }),
        );
        request
            .pellet_targets()
            .into_iter()
            .map(|target| {
                if spawn_index(target).is_none() {
                    return Err(AttackRefusal::UnknownTarget);
                }
                let view = self
                    .target_view(target)
                    .ok_or(AttackRefusal::UnknownTarget)?;
                if view.dead {
                    return Err(AttackRefusal::TargetDead);
                }
                if view.returning {
                    return Err(AttackRefusal::Returning);
                }
                if view.position.horizontal_distance(position) > SPREADSHOT_RANGE_M {
                    return Err(AttackRefusal::OutOfReach);
                }
                self.strike(
                    Strike {
                        entity,
                        number,
                        attacker: shooter,
                        target,
                        view: &view,
                        held: Some(held),
                        row,
                    },
                    now_ms,
                    |amount, crit, killed, scale| {
                        DamageDealtMsg::pellet(number, target, amount, scale, crit, killed)
                    },
                )
            })
            .collect()
    }

    /// The hit and death messages since the last call.
    pub fn take_messages(&mut self) -> Vec<Outgoing> {
        std::mem::take(&mut self.combat_messages)
    }

    /// Queues `message` for `connection`, reliable on the message channel.
    fn send_reliable(&mut self, connection: ConnectionRef, message: &ServerMessage) {
        match message.envelope() {
            Ok(envelope) => self.combat_messages.push(Outgoing {
                connection,
                channel: MESSAGE_CHANNEL,
                reliable: true,
                envelopes: vec![envelope],
            }),
            Err(error) => tracing::error!(%error, "combat message not encoded"),
        }
    }

    /// Drinks the potions whose press is due at `now_ms`, at server time `server_ms` (ms since
    /// 2000-01-01).
    pub(super) fn drink_due(&mut self, now_ms: u64, server_ms: u64) {
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.drinks)
            .into_iter()
            .partition(|(_, at)| now_ms >= *at);
        self.drinks = later;
        for (number, _) in due {
            self.drink(number, server_ms);
        }
    }

    /// The player `number` drinks the potion of its quick slot 1, unless the slot is empty, holds
    /// no healing item, or the item's cooldown runs (xp-potion-exit note, section 2.5): one less
    /// in the slot, the cooldown, the heal (`health` + `health share` x max, at most to the max)
    /// and the heal's visual script.
    fn drink(&mut self, number: u64, server_ms: u64) {
        let Some(entity) = self.player_by_number(number) else {
            return;
        };
        let data = Arc::clone(&self.combat.data);
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let player = &mut *player;
        let Some(gear) = player.gear.as_mut() else {
            tracing::debug!(player = number, "drink: no gear");
            return;
        };
        let Some(item) = gear.worn_item(slot::QUICK_SLOT_1).map(str::to_owned) else {
            tracing::debug!(player = number, "drink: quick slot 1 is empty");
            return;
        };
        let Some(heal) = data.heal_of_item(&item) else {
            tracing::debug!(player = number, item, "drink: the item does not heal");
            return;
        };
        let group = lowercase_crc32(&heal.cooldown_id);
        let now_us = server_ms.saturating_mul(US_PER_MS);
        player.cooldowns.retain(|cooldown| cooldown.end_us > now_us);
        if player
            .cooldowns
            .iter()
            .any(|cooldown| cooldown.group == group)
        {
            tracing::debug!(player = number, item, "drink: the cooldown runs");
            return;
        }
        gear.use_one(slot::QUICK_SLOT_1);
        let cooldown_ms = u64::from(heal.cooldown_s) * MS_PER_SECOND;
        player.cooldowns.push(ConsumableCooldown {
            consumable: lowercase_crc32(&item),
            end_us: now_us.saturating_add(cooldown_ms * US_PER_MS),
            group,
        });
        player.cooldowns_changed = true;
        let connection = player.connection.clone();
        let Some(mut vitals) = self.world.get_mut::<Vitals>(entity) else {
            return;
        };
        let max = f64::from(vitals.health.max());
        // The heal fits a `u32`: it is below the max health and a flat amount.
        let amount = (heal.health + heal.health_share * max).round() as u32;
        let healed = vitals.health.heal(amount);
        tracing::debug!(player = number, item, amount, healed, "drink");
        self.send_reliable(
            connection,
            &ServerMessage::AdjustFxScripts(AdjustFxScriptsMsg::on(number, HEAL_SCRIPT)),
        );
    }

    /// The dead player of `connection` lives again with full health where it died, when
    /// `min_respawn_ms` have passed since its death (M6-B spec section 8.2).
    ///
    /// # Errors
    ///
    /// Returns [`RespawnRefusal`] when there is no player, it is alive, or it died too recently.
    pub fn request_respawn(
        &mut self,
        connection: &ConnectionRef,
        now_ms: u64,
    ) -> Result<(), RespawnRefusal> {
        let min_ms = self.combat.min_respawn_ms;
        let mut vitals = self
            .by_connection
            .get(connection)
            .copied()
            .and_then(|entity| self.world.get_mut::<Vitals>(entity))
            .ok_or(RespawnRefusal::NotInWorld)?;
        let since = vitals.dead_since_ms.ok_or(RespawnRefusal::NotDead)?;
        if now_ms.saturating_sub(since) < min_ms {
            return Err(RespawnRefusal::TooSoon);
        }
        vitals.health.refill();
        vitals.dead_since_ms = None;
        Ok(())
    }

    /// Keeps whether the player holds block, starts a dodge roll on a new press of the dodge key
    /// and a drink on a new press of quick slot 1 (the first, newest record), then reads the
    /// attacks (a press and its release) in the records of `records` not read before, oldest
    /// first. The first attack swings: it hits in the middle of its weapon's damage window
    /// after its start, at the latest at `now_ms` ([`SimWorld::player_swings_due`]). The others of the same message are
    /// [`AttackRefusal::TooFast`], which this returns. The counter of the newest record carries
    /// over to the next message. Nothing for a connection without a player.
    pub fn attack_input(
        &mut self,
        connection: &ConnectionRef,
        records: &[InputRecord],
        now_ms: u64,
    ) -> Vec<Result<AttackOutcome, AttackRefusal>> {
        let Some(entity) = self.by_connection.get(connection).copied() else {
            return Vec::new();
        };
        let Some(number) = self.world.get::<Player>(entity).map(|player| player.number) else {
            return Vec::new();
        };
        let swimming = self.world.get::<Body>(entity).is_some_and(|body| {
            body.motion.position().z <= self.settings.ocean_level_m + SWIM_HEIGHT_M
        });
        if let Some(newest) = records.first() {
            self.stamina_input(entity, number, newest, swimming, now_ms);
        }
        let Some(mut vitals) = self.world.get_mut::<Vitals>(entity) else {
            return Vec::new();
        };
        // The client sends the newest record first, then older copies with fewer fields (swing
        // note: counters 7290, 7289, 7288); each record counts once, oldest first. An attack is a
        // press and its release: a short hold is the light attack, a long one the heavy attack.
        let mut attacks = Vec::new();
        let mut weapon_key = None;
        for record in records.iter().rev() {
            let counter = record.u16_after_mask;
            if vitals
                .input_counter
                .is_some_and(|last| !counter_is_newer(counter, last))
            {
                continue;
            }
            vitals.input_counter = Some(counter);
            weapon_key = record.weapon_key_pressed().or(weapon_key);
            if record.attack_pressed() {
                vitals.attack_pressed_at = Some(counter);
            }
            if record.attack_released() {
                let held_records = vitals
                    .attack_pressed_at
                    .take()
                    .map_or(0, |pressed| counter.wrapping_sub(pressed));
                attacks.push((
                    AttackKind::of_hold(held_records),
                    u64::from(held_records.saturating_mul(INPUT_RECORD_MS)),
                ));
            }
        }
        let lock_target = records.first().and_then(InputRecord::lock_target);
        if std::mem::replace(&mut vitals.lock_target, lock_target) != lock_target {
            tracing::info!(player = number, target = ?lock_target, "lock target");
        }
        if let Some(key) = weapon_key
            && let Some(mut player) = self.world.get_mut::<Player>(entity)
            && let Some(gear) = player.gear.as_mut()
        {
            gear.draw(key, now_ms, unix_ns(self.server_ms));
        }
        let weapon = self.held_weapon(entity).map(|(weapon, _)| weapon);
        // A spread-shot weapon fires through its own request (2719), not the attack key.
        if weapon.is_some_and(|weapon| weapon.spread_shot) {
            return Vec::new();
        }
        // Every attack of one message has the same receive time, so only the first can hit: the
        // others are too fast, and a client cannot make the loop search for targets per record.
        let Some(&(kind, held_ms)) = attacks.first() else {
            return Vec::new();
        };
        let facing = records
            .first()
            .and_then(|newest| newest.bit_6)
            .map(|[yaw, _]| yaw);
        // A light attack starts at its press, a heavy one at its release (retail: the hit came
        // 0.44 s after a light attack's press).
        let started_ms = match kind {
            AttackKind::Light => now_ms.saturating_sub(held_ms),
            AttackKind::Heavy => now_ms,
        };
        let shape = swing_shape(weapon.map_or(WeaponKind::Other, |weapon| weapon.kind), kind);
        self.player_swings.push(PlayerSwing {
            attacker: connection.clone(),
            aim: Aim {
                facing,
                lock_target,
            },
            kind,
            due_ms: started_ms.saturating_add(shape.hit_ms()).max(now_ms),
        });
        attacks[1..]
            .iter()
            .map(|_| Err(AttackRefusal::TooFast))
            .collect()
    }

    /// Lands the player swings due at `now_ms` ([`SimWorld::swing`]).
    pub(super) fn player_swings_due(
        &mut self,
        now_ms: u64,
    ) -> Vec<Result<AttackOutcome, AttackRefusal>> {
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.player_swings)
            .into_iter()
            .partition(|swing| now_ms >= swing.due_ms);
        self.player_swings = later;
        due.into_iter()
            .flat_map(|swing| self.swing(&swing.attacker, swing.aim, swing.kind, now_ms))
            .collect()
    }

    /// The block, dodge and quick slot keys of the newest input record `newest` of player
    /// `number`: a release of block starts its stamina's regen delay again; a new press of dodge
    /// starts a roll, or a dash when `swimming`, if its stamina pays for it; a new press of quick
    /// slot 1 drinks.
    fn stamina_input(
        &mut self,
        entity: Entity,
        number: u64,
        newest: &InputRecord,
        swimming: bool,
        now_ms: u64,
    ) {
        let Some(mut vitals) = self.world.get_mut::<Vitals>(entity) else {
            return;
        };
        let blocking = newest.blocking();
        let block_released = vitals.blocking && !blocking;
        vitals.blocking = blocking;
        let dodge = newest.dodging();
        let dodge_pressed = dodge && !vitals.dodge_held;
        vitals.dodge_held = dodge;
        let quick = newest.using_quick_slot_1();
        if quick && !vitals.quick_slot_held {
            self.drinks.push((number, now_ms.saturating_add(DRINK_MS)));
        }
        vitals.quick_slot_held = quick;
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        if block_released {
            player.stamina.block_released();
        }
        let kind = if swimming {
            Dodge::SwimDash
        } else {
            Dodge::Roll
        };
        if dodge_pressed
            && player.stamina.dodge(kind, now_ms)
            && let Some(mut vitals) = self.world.get_mut::<Vitals>(entity)
        {
            vitals.dodging_until_ms = now_ms.saturating_add(DODGE_MS);
        }
    }

    /// Runs the stamina of every player one tick at `now_ms`.
    pub(super) fn tick_stamina(&mut self, now_ms: u64) {
        let mut query = self.world.query::<&mut Player>();
        for mut player in query.iter_mut(&mut self.world) {
            player.stamina.tick(now_ms);
        }
    }

    /// Moves every player's hands on to `now_ms` at server time `server_ms`: a new weapon in a
    /// main-hand option gets its shots, a drawn weapon reaches the hand, an empty one is loaded.
    pub(super) fn tick_hands(&mut self, now_ms: u64, server_ms: u64) {
        let data = Arc::clone(&self.combat.data);
        let weapon_of = |item: &str| {
            let weapon = data.weapon_of_item(item)?;
            Some((weapon.max_loaded_ammo, weapon.reload_ms))
        };
        let mut query = self.world.query::<&mut Player>();
        for mut player in query.iter_mut(&mut self.world) {
            if let Some(gear) = player.gear.as_mut() {
                gear.arm(weapon_of, unix_ns(server_ms));
                gear.tick_hands(now_ms);
            }
        }
    }

    /// The weapon in the hand of the player `entity` with its gear score, when its row is known.
    fn held_weapon(&self, entity: Entity) -> Option<(WeaponRow, u32)> {
        let (item, gear_score) = self
            .world
            .get::<Player>(entity)?
            .gear
            .as_ref()?
            .main_weapon()?;
        Some((*self.combat.data.weapon_of_item(item)?, gear_score))
    }

    /// The swing `kind` of the player of `attacker` with the aim `aim` lands at `now_ms`. Its
    /// homing turns it to the locked target, else to the best mob in its homing reach, else to
    /// the nearest mob when the input had no camera yaw. It hits every living, not returning
    /// mob, here or a ghost, whose body reaches into the wedge of its weapon's swing, nearest to
    /// the middle first, at most [`MAX_SWING_TARGETS`] (collision note).
    pub fn swing(
        &mut self,
        attacker: &ConnectionRef,
        aim: Aim,
        kind: AttackKind,
        now_ms: u64,
    ) -> Vec<Result<AttackOutcome, AttackRefusal>> {
        let refused = |refusal| vec![Err(refusal)];
        let Some(entity) = self.by_connection.get(attacker).copied() else {
            return refused(AttackRefusal::NotInWorld);
        };
        if self
            .world
            .get::<Vitals>(entity)
            .is_some_and(|vitals| vitals.health.is_dead())
        {
            return refused(AttackRefusal::Dead);
        }
        let Some(position) = self
            .world
            .get::<Body>(entity)
            .map(|body| body.motion.position())
        else {
            return refused(AttackRefusal::NotInWorld);
        };
        let weapon = self
            .held_weapon(entity)
            .map_or(WeaponKind::Other, |(weapon, _)| weapon.kind);
        let shape = swing_shape(weapon, kind);
        let candidates: Vec<(u64, Position, Capsule)> = self
            .mob_states()
            .filter(|(_, _, state)| state.health > 0 && !state.returning)
            .map(|(mob, at, _)| (mob, at, self.mobs.capsule(mob)))
            .collect();
        let locked = aim
            .lock_target
            .and_then(|target| candidates.iter().find(|(mob, _, _)| *mob == target))
            .map(|(_, at, _)| yaw_toward(position, *at));
        let nearest = || {
            candidates
                .iter()
                .min_by(|a, b| {
                    a.1.horizontal_distance(position)
                        .total_cmp(&b.1.horizontal_distance(position))
                })
                .map(|(_, at, _)| yaw_toward(position, *at))
        };
        let facing = locked.or_else(|| match aim.facing {
            Some(facing) => Some(homing(
                &shape,
                position,
                facing,
                candidates.iter().map(|(_, at, _)| *at),
            )),
            None => nearest(),
        });
        let Some(facing) = facing else {
            return refused(AttackRefusal::NoTarget);
        };
        let wedge = Wedge::of_swing(&shape, position, facing);
        let mut hits: Vec<(f32, u64)> = candidates
            .iter()
            .filter_map(|(mob, at, body)| Some((wedge.touches(*at, *body)?, *mob)))
            .collect();
        hits.sort_by(|a, b| a.0.total_cmp(&b.0));
        hits.truncate(MAX_SWING_TARGETS);
        tracing::debug!(
            attacker = ?attacker,
            x = position.x,
            y = position.y,
            facing,
            hits = ?hits,
            "player swing"
        );
        if hits.is_empty() {
            return refused(AttackRefusal::NoTarget);
        }
        let row = self
            .combat
            .data
            .weapon_attack(weapon, kind == AttackKind::Heavy);
        let Some(damage_id) = row.map(|row| row.damage_id.clone()) else {
            return refused(AttackRefusal::NotWeaponAttack);
        };
        hits.iter()
            .enumerate()
            .map(|(index, (_, mob))| self.hit(attacker, *mob, &damage_id, None, index > 0, now_ms))
            .collect()
    }

    /// What an attack sees of the mob `target`: here, or a ghost of its owner.
    fn target_view(&mut self, target: u64) -> Option<TargetView> {
        if let Some(mob) = self.mobs.get_mut(target) {
            let view = TargetView {
                position: mob.brain.position(),
                dead: mob.health.is_dead(),
                returning: mob.brain.state() == MobState::Return,
                level: 0,
                owner: None,
            };
            return Some(TargetView {
                level: self.mobs.level(target).unwrap_or_default(),
                ..view
            });
        }
        let (owner, ghost) = self.ghost(target)?;
        let state = ghost.mob?;
        Some(TargetView {
            position: ghost.position,
            dead: state.health == 0,
            returning: state.returning,
            level: state.level,
            owner: Some(owner.clone()),
        })
    }

    /// A hit for `owner`, the sim of `target`.
    fn remote_hit(
        &self,
        owner: BusId,
        target: u64,
        attacker: u64,
        amount: u32,
        threat: u64,
    ) -> RemoteHit {
        RemoteHit {
            owner,
            hit: Hit {
                target,
                attacker,
                attacker_sim: self.me.id.clone(),
                amount,
                threat,
            },
        }
    }

    /// Applies a hit of `amount` with `threat` from `attacker` on `target`, a mob or a player of
    /// this sim. Returns `None` when the target is not here, is dead, (a mob) is returning, or
    /// (a player) is frozen for a handoff, whose offer already carries its health.
    pub fn apply_hit(
        &mut self,
        target: u64,
        attacker: u64,
        amount: u32,
        threat: u64,
        now_ms: u64,
    ) -> Option<AttackOutcome> {
        self.apply_hit_with(target, attacker, amount, threat, now_ms, None)
    }

    /// [`SimWorld::apply_hit`] of a hit of a mob move `strike`: a player's 3601 carries the
    /// move's item tag and, in a second phase, the fire part as a second item.
    fn apply_hit_with(
        &mut self,
        target: u64,
        attacker: u64,
        amount: u32,
        threat: u64,
        now_ms: u64,
        strike: Option<&MoveStrike>,
    ) -> Option<AttackOutcome> {
        if spawn_index(target).is_some() {
            let mob = self.mobs.get_mut(target)?;
            if mob.health.is_dead() || !hurt(&mut mob.brain, attacker, threat, now_ms) {
                return None;
            }
            let outcome = match mob.health.take(amount) {
                Damage::Killed => {
                    died(&mut mob.brain, now_ms);
                    AttackOutcome::Killed { target }
                }
                Damage::Hurt | Damage::AlreadyDead => AttackOutcome::Hit { target, amount },
            };
            self.mobs.wake(target);
            return Some(outcome);
        }
        let entity = self.player_by_number(target)?;
        let player = self.world.get::<Player>(entity)?;
        if player.frozen || self.is_god(entity) {
            return None;
        }
        let connection = player.connection.clone();
        let attacker_at = self.attacker_position(attacker);
        let (tag, scale) = self
            .mobs
            .hit_style(attacker, (1.0 - self.combat.player_armor_share) as f32);
        let mut vitals = self.world.get_mut::<Vitals>(entity)?;
        let max_health = amount_of(vitals.health.max());
        let damage = vitals.health.take(amount);
        if damage == Damage::Killed {
            vitals.dead_since_ms = Some(now_ms);
        }
        if damage == Damage::AlreadyDead {
            return None;
        }
        let total = amount_of(amount);
        let fire_share = strike
            .filter(|strike| strike.phase_two.is_some())
            .map_or(0.0, |strike| strike.hit.fire_share as f32);
        let melee = MeleeDamage {
            attacker,
            amount: total * (1.0 - fire_share),
            max_health,
            attacker_at,
            tag: strike.map_or(tag, |strike| strike.tag),
            scale,
            fire: strike
                .and_then(|strike| strike.phase_two)
                .filter(|_| fire_share > 0.0)
                .map(|(_, fire_tag)| (fire_tag, total * fire_share)),
        };
        self.send_reliable(
            connection.clone(),
            &ServerMessage::OnDamage(OnDamageMsg::melee(target, &melee)),
        );
        if damage == Damage::Hurt {
            return Some(AttackOutcome::Hit { target, amount });
        }
        self.mobs.forget(target);
        if let Some(departure) = self.departure(entity) {
            self.deaths.push(departure);
        }
        self.send_reliable(
            connection,
            &ServerMessage::DeathRecap(DeathRecapMsg::of(target)),
        );
        Some(AttackOutcome::Killed { target })
    }

    /// Where mob `attacker` stands, here or as a ghost; the origin when it is neither (retail
    /// always names the creature's position in a hit on a player).
    fn attacker_position(&mut self, attacker: u64) -> [f32; 3] {
        let at = match self.mobs.get_mut(attacker) {
            Some(mob) => Some(mob.brain.position()),
            None => self.ghost(attacker).map(|(_, ghost)| ghost.position),
        };
        at.map_or([0.0; 3], |at| [at.x, at.y, at.z])
    }

    /// The player with entity number `number`.
    pub(super) fn player_by_number(&mut self, number: u64) -> Option<Entity> {
        let mut query = self.world.query::<(Entity, &Player)>();
        query
            .iter(&self.world)
            .find(|(_, player)| player.number == number)
            .map(|(entity, _)| entity)
    }

    /// Starts the swings `started` at `now_ms`, server time `server_ms` (their animation) and
    /// gives the swings that hit now: each lands its mob's hit delay after its start (retail: the
    /// hit came a fixed time after the attack started, per creature; creature-animation note,
    /// section 5.1).
    pub(super) fn swings_landing(
        &mut self,
        started: Vec<MobAttackIntent>,
        now_ms: u64,
        server_ms: u64,
    ) -> Vec<(MobAttackIntent, Option<MoveHit>)> {
        for swing in started {
            let pending = |lands_after_ms: u64, hit| PendingMobSwing {
                lands_ms: now_ms.saturating_add(lands_after_ms),
                started_ms: server_ms,
                swing,
                hit,
            };
            match swing.mob_move {
                Some(mob_move) => {
                    self.mob_timelines
                        .play(swing.mob, mob_move.fragment, mob_move.length_ms);
                    for hit in mob_move.hits {
                        self.swings.push(pending(hit.at_ms, Some(*hit)));
                    }
                }
                None => {
                    self.mob_timelines.attacked(swing.mob);
                    self.swings
                        .push(pending(self.mobs.hit_delay_ms(swing.mob), None));
                }
            }
        }
        let (landing, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.swings)
            .into_iter()
            .partition(|pending| now_ms >= pending.lands_ms);
        self.swings = later;
        landing
            .into_iter()
            // A mob that died or reeled from a player's hit before its swing landed does not hit.
            .filter(|pending| {
                self.mobs
                    .health(pending.swing.mob)
                    .is_some_and(|health| health > 0)
                    && !self
                        .mob_timelines
                        .reeled_since(pending.swing.mob, pending.started_ms)
            })
            .map(|pending| (pending.swing, pending.hit))
            .collect()
    }

    /// Whether the swing `intent` of a mob at `at` reaches its target's body at `body`. A mob
    /// with a reach lunges first, when the target is within its lunge turn of where it aimed:
    /// toward the body, up to its longest lunge, to where it stops or the bodies touch; then
    /// its wedge must touch the body (collision note: every retail mob hit landed 0.79 to
    /// 1.65 m from the player). A mob without one hits within its gait's swing start plus
    /// [`crate::mobs::SWING_HIT_MARGIN_M`].
    fn mob_swing_reaches(
        &mut self,
        intent: &MobAttackIntent,
        at: Position,
        body: Position,
    ) -> bool {
        let Some(reach) = self.mobs.reach(intent.mob) else {
            return body.horizontal_distance(at) <= self.mobs.hit_reach_m(intent.mob);
        };
        let capsule = self.mobs.capsule(intent.mob);
        let aim = yaw_toward(at, intent.target_at);
        let toward = yaw_toward(at, body);
        let (from, facing) = if turn(aim, toward).abs() <= reach.lunge_turn_rad {
            let stop_m = reach
                .lunge_stop_m
                .max(capsule.radius_m + PLAYER_CAPSULE.radius_m);
            let lunge_m = (body.horizontal_distance(at) - stop_m).clamp(0.0, reach.lunge_max_m);
            let from = self.mobs.lunge(intent.mob, body, lunge_m).unwrap_or(at);
            (from, toward)
        } else {
            (at, aim)
        };
        Wedge {
            from,
            own: capsule,
            facing,
            reach_m: reach.reach_m,
            half_arc_rad: MOB_SWING_HALF_ARC_RAD,
        }
        .touches(body, PLAYER_CAPSULE)
        .is_some()
    }

    /// Applies the mobs' hits on the players of this sim; a hit on a player ghost goes to its
    /// sim.
    pub(super) fn mob_hits(
        &mut self,
        intents: Vec<(MobAttackIntent, Option<MoveHit>)>,
        now_ms: u64,
    ) -> Vec<AttackOutcome> {
        let armor_share = self.combat.player_armor_share;
        intents
            .into_iter()
            .filter_map(|(intent, move_hit)| {
                let mob = self.mobs.get_mut(intent.mob)?;
                let at = mob.brain.position();
                let mut hit = mob_hit(&mob.attack, armor_share);
                let strike = move_hit
                    .zip(intent.mob_move)
                    .map(|(hit, mob_move)| MoveStrike {
                        hit,
                        tag: mob_move.tag,
                        phase_two: self
                            .mobs
                            .phase_two(intent.mob)
                            .map(|phase| (phase.damage_mult, phase.fire_tag)),
                    });
                if let Some(strike) = &strike {
                    let mult = strike.phase_two.map_or(1.0, |(mult, _)| mult);
                    let scaled = f64::from(hit.amount) * strike.hit.damage_scale * mult;
                    // A hit of a few hundred points.
                    hit.amount = scaled.round() as u32;
                }
                tracing::debug!(
                    mob = intent.mob,
                    target = intent.target,
                    x = at.x,
                    y = at.y,
                    z = at.z,
                    amount = hit.amount,
                    "mob attack"
                );
                if let Some(entity) = self.player_by_number(intent.target) {
                    let vitals = *self.world.get::<Vitals>(entity)?;
                    // A swing at a player who moved away misses: retail sends no hit message.
                    let body = self.world.get::<Body>(entity)?.motion.position();
                    if !self.mob_swing_reaches(&intent, at, body) {
                        return None;
                    }
                    // Spawn protection: a mob cannot hit a player whose client has not spawned
                    // into the world yet (it would die at the loading screen otherwise).
                    if !vitals.spawned {
                        return None;
                    }
                    // A swing into a dodge roll misses: retail sends no hit message then.
                    if now_ms < vitals.dodging_until_ms {
                        return None;
                    }
                    let block_stamina = strike.map_or_else(
                        || self.mobs.block_stamina(intent.mob),
                        |strike| strike.hit.block_stamina,
                    );
                    let mut player = self.world.get_mut::<Player>(entity)?;
                    // A block holds while the player has stamina; the hit takes some of it (Inferred:
                    // retail never let a blocked hit find the stamina at 0).
                    let blocked = vitals.blocking && player.stamina.current() > 0.0;
                    if blocked {
                        player.stamina.blocked_hit(block_stamina);
                    }
                    let connection = player.connection.clone();
                    self.send_reliable(
                        connection,
                        &ServerMessage::MeleeHit(MeleeHitMsg::on(intent.target, blocked)),
                    );
                    if blocked {
                        return None;
                    }
                    return self.apply_hit_with(
                        intent.target,
                        intent.mob,
                        hit.amount,
                        0,
                        now_ms,
                        strike.as_ref(),
                    );
                }
                let (owner, ghost) = self.ghost(intent.target)?;
                let owner = owner.clone();
                ghost.mob.is_none().then(|| {
                    AttackOutcome::Remote(self.remote_hit(
                        owner,
                        intent.target,
                        intent.mob,
                        hit.amount,
                        0,
                    ))
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use nw_sim_core::{Dice, MobState, brain_step, hurt};

    use super::*;
    use crate::combat::RespawnRefusal;
    use crate::combat::test_support::TEST_PLAYER_HEALTH;
    use crate::combat::tests::test_combat;
    use crate::config::tests::committed;
    use crate::mobs::mob_entity;
    use crate::world::gear::{self, Gear};
    use crate::world::tests::{
        BORDER_X, CELL_M, NOW, SERVER_TIME, admit_at, connection, join, messages_to, slot, table,
        test_spawn, tick, world_with_spawns,
    };
    use crate::world::{MoveError, WorldSettings};
    use nw_bus::{Ghost, MobGhost, Point};
    use nw_content::combat::{CombatData, CombatRows, DEFAULT_SPREADSHOT_DAMAGE_ID};
    use nw_messages::{
        BodyCodec, DamageDealtMsg, DeathRecapMsg, FacetHeader, RAW_ID_LEN, ServerMessage, SpreadHit,
    };

    const LIGHT: &str = "1H_Sword_Attack1";
    const MOB_X: f32 = CELL_M / 2.0;
    /// 64 x 1.0 x (1 - 0.35), rounded.
    const SWORD_HIT: u32 = 42;
    const MOB_HEALTH: u32 = 1000;
    const INTERVAL_MS: u64 = crate::config::DEFAULT_MIN_ATTACK_INTERVAL_MS;
    const TICK_MS: u64 = 33;
    const MOB_RESPAWN_MS: u64 = 360_000;
    /// The committed `min_respawn_ms`.
    const MIN_RESPAWN_MS: u64 = 2_000;
    /// Far beyond any wait.
    const LONG_AFTER_MS: u64 = 60_000;
    const LETHAL: u32 = 100_000;

    fn mob() -> u64 {
        mob_entity(0)
    }

    /// A world with one mob and player 1 two metres from it.
    fn fight() -> SimWorld {
        fight_at(MOB_X + 2.0)
    }

    /// A world with one mob and player 1 at `x`.
    fn fight_at(x: f32) -> SimWorld {
        let mut world = world_with_spawns(vec![test_spawn(MOB_X)]);
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, x);
        world
    }

    fn health_of(world: &mut SimWorld, number: u64) -> Health {
        let entity = world.player_by_number(number).expect("player");
        world.world.get::<Vitals>(entity).expect("vitals").health
    }

    fn mob_health(world: &mut SimWorld) -> Health {
        world.mobs.get_mut(mob()).expect("mob").health
    }

    /// What one input record of a test does with the attack key.
    #[derive(Clone, Copy)]
    enum Key {
        Press,
        Hold,
        Release,
    }

    /// The records of one message in the client's order, newest first, from `keys` oldest first,
    /// with counters from `first_counter` up (retail press: value 2 bit 40; release: value 2
    /// bit 31).
    fn message(first_counter: u16, keys: &[Key]) -> Vec<InputRecord> {
        const ACTION_VALUES: usize = 10;
        let mut records: Vec<InputRecord> = keys
            .iter()
            .zip(first_counter..)
            .map(|(key, counter)| {
                let mut values = vec![0; ACTION_VALUES];
                values[2] = match key {
                    Key::Press => 1 << 40,
                    Key::Hold => 0,
                    Key::Release => 1 << 31,
                };
                InputRecord {
                    u16_after_mask: counter,
                    slots: nw_messages::InputSlots {
                        count: 19,
                        values,
                        bytes: [128; 4],
                    },
                    ..InputRecord::default()
                }
            })
            .collect();
        records.reverse();
        records
    }

    /// When a sword light attack hits after its press.
    fn light_hit_ms() -> u64 {
        swing_shape(WeaponKind::Other, AttackKind::Light).hit_ms()
    }

    /// A fight after one tick, which wakes the mob.
    fn awake_fight() -> SimWorld {
        let mut world = fight();
        tick(&mut world, NOW);
        world
    }

    #[test]
    fn a_tap_is_one_light_attack_that_hits_after_the_swing_time() {
        // The tap's press came one record before its release.
        const HELD_MS: u64 = INPUT_RECORD_MS as u64;
        let mut world = awake_fight();
        let refused = world.attack_input(
            &connection(1),
            &message(1, &[Key::Press, Key::Release]),
            NOW,
        );
        assert!(refused.is_empty());
        let due = NOW - HELD_MS + light_hit_ms();
        assert!(
            world.player_swings_due(due - 1).is_empty(),
            "still swinging"
        );
        assert_eq!(
            world.player_swings_due(due),
            vec![Ok(AttackOutcome::Hit {
                target: mob(),
                amount: SWORD_HIT
            })]
        );
        assert!(world.player_swings_due(due).is_empty(), "it hits once");
    }

    #[test]
    fn a_long_hold_hits_at_its_release() {
        const HOLD_RECORDS: usize = 20;
        let mut world = awake_fight();
        let mut keys = vec![Key::Press];
        keys.extend(std::iter::repeat_n(Key::Hold, HOLD_RECORDS));
        keys.push(Key::Release);
        world.attack_input(&connection(1), &message(1, &keys), NOW);
        let heavy_ms = swing_shape(WeaponKind::Other, AttackKind::Heavy).hit_ms();
        assert!(world.player_swings_due(NOW + heavy_ms - 1).is_empty());
        assert_eq!(world.player_swings_due(NOW + heavy_ms).len(), 1);
    }

    #[test]
    fn the_hold_picks_the_light_or_the_heavy_attack() {
        // Retail light holds 35 to 314 ms, heavy holds from 432 ms.
        assert_eq!(AttackKind::of_hold(9), AttackKind::Light, "297 ms");
        assert_eq!(AttackKind::of_hold(13), AttackKind::Heavy, "429 ms");
    }

    #[test]
    fn a_press_attacks_at_its_release_in_a_later_message() {
        let mut world = awake_fight();
        let pressed = message(1, &[Key::Press, Key::Hold]);
        world.attack_input(&connection(1), &pressed, NOW);
        assert!(world.player_swings_due(NOW + light_hit_ms()).is_empty());
        let released = message(3, &[Key::Release]);
        world.attack_input(&connection(1), &released, NOW + INTERVAL_MS);
        assert_eq!(
            world
                .player_swings_due(NOW + INTERVAL_MS + light_hit_ms())
                .len(),
            1
        );
    }

    #[test]
    fn an_older_copy_is_not_read_again() {
        let mut world = awake_fight();
        let tap = message(5, &[Key::Press, Key::Release]);
        world.attack_input(&connection(1), &tap, NOW);
        assert_eq!(world.player_swings_due(NOW + light_hit_ms()).len(), 1);
        // The next message repeats records 5 and 6 after the new record 7.
        let mut next = message(7, &[Key::Hold]);
        next.extend(tap);
        world.attack_input(&connection(1), &next, NOW + INTERVAL_MS);
        assert!(
            world
                .player_swings_due(NOW + INTERVAL_MS + light_hit_ms())
                .is_empty()
        );
    }

    #[test]
    fn extra_attacks_in_one_message_are_too_fast_without_a_search() {
        let mut world = awake_fight();
        join(&mut world, 2, MOB_X + 10.0);
        let records = message(
            1,
            &[
                Key::Press,
                Key::Release,
                Key::Press,
                Key::Release,
                Key::Press,
                Key::Release,
            ],
        );
        assert_eq!(
            world.attack_input(&connection(2), &records, NOW),
            vec![Err(AttackRefusal::TooFast), Err(AttackRefusal::TooFast)]
        );
        assert_eq!(
            world.player_swings_due(NOW + light_hit_ms()),
            vec![Err(AttackRefusal::NoTarget)]
        );
    }

    #[test]
    fn no_mob_in_reach_is_no_target() {
        let mut world = awake_fight();
        join(&mut world, 2, MOB_X + 10.0);
        assert_eq!(
            world.swing(&connection(2), Aim::default(), AttackKind::Light, NOW),
            vec![Err(AttackRefusal::NoTarget)]
        );
        assert_eq!(
            world.swing(&connection(7), Aim::default(), AttackKind::Light, NOW),
            vec![Err(AttackRefusal::NotInWorld)]
        );
    }

    #[test]
    fn the_nearest_mob_is_the_target() {
        let mut world = world_with_spawns(vec![test_spawn(MOB_X), test_spawn(MOB_X + 3.0)]);
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, MOB_X + 2.0);
        tick(&mut world, NOW);
        assert!(matches!(
            world.swing(&connection(1), Aim::default(), AttackKind::Light, NOW).as_slice(),
            [Ok(AttackOutcome::Hit { target, .. })] if *target == mob_entity(1)
        ));
    }

    /// Two mobs one metre to each side of player 1, with yaw `facing` and the lock `lock`.
    fn two_mobs_aside(facing: Option<f32>, lock: Option<u64>) -> Vec<u64> {
        let mut world = world_with_spawns(vec![test_spawn(MOB_X), test_spawn(MOB_X + 2.0)]);
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, MOB_X + 1.0);
        tick(&mut world, NOW);
        let aim = Aim {
            facing,
            lock_target: lock,
        };
        world
            .swing(&connection(1), aim, AttackKind::Light, NOW)
            .into_iter()
            .filter_map(|result| match result {
                Ok(AttackOutcome::Hit { target, .. }) => Some(target),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_swing_hits_every_body_in_its_wedge() {
        // Facing east, mobs 1.0 and 1.8 m ahead: both bodies reach into the 1.6 m wedge.
        const FACING_EAST: f32 = -std::f32::consts::FRAC_PI_2;
        let mut world = world_with_spawns(vec![test_spawn(MOB_X), test_spawn(MOB_X + 0.8)]);
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, MOB_X - 1.0);
        tick(&mut world, NOW);
        let aim = Aim {
            facing: Some(FACING_EAST),
            lock_target: None,
        };
        let results = world.swing(&connection(1), aim, AttackKind::Light, NOW);
        assert!(
            matches!(
                results.as_slice(),
                [Ok(AttackOutcome::Hit { target: first, .. }), Ok(AttackOutcome::Hit { target: second, .. })]
                    if *first == mob_entity(0) && *second == mob_entity(1)
            ),
            "{results:?}"
        );
    }

    #[test]
    fn a_locked_target_turns_the_swing_to_it() {
        // Facing east would hit the east mob; the lock on the west mob turns the swing there.
        const FACING_EAST: f32 = -std::f32::consts::FRAC_PI_2;
        assert_eq!(two_mobs_aside(Some(FACING_EAST), None), vec![mob_entity(1)]);
        assert_eq!(
            two_mobs_aside(Some(FACING_EAST), Some(mob_entity(0))),
            vec![mob_entity(0)]
        );
    }

    #[test]
    fn only_a_mob_in_front_is_the_target() {
        // Yaw -90 degrees faces +x (forward = (-sin a, cos a)).
        const FACING_EAST: f32 = -std::f32::consts::FRAC_PI_2;
        let mut world = world_with_spawns(vec![test_spawn(MOB_X), test_spawn(MOB_X + 3.0)]);
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, MOB_X + 2.0);
        tick(&mut world, NOW);
        let aim = |facing| Aim {
            facing: Some(facing),
            lock_target: None,
        };
        assert!(matches!(
            world.swing(&connection(1), aim(FACING_EAST), AttackKind::Light, NOW).as_slice(),
            [Ok(AttackOutcome::Hit { target, .. })] if *target == mob_entity(1)
        ));
        assert!(
            matches!(
                world.swing(&connection(1), aim(-FACING_EAST), AttackKind::Light, NOW + INTERVAL_MS).as_slice(),
                [Ok(AttackOutcome::Hit { target, .. })] if *target == mob_entity(0)
            ),
            "facing west, the mob behind before is in front"
        );
        assert_eq!(
            world.swing(
                &connection(1),
                aim(0.0),
                AttackKind::Light,
                NOW + 2 * INTERVAL_MS
            ),
            vec![Err(AttackRefusal::NoTarget)],
            "facing north, both mobs are to the side and beyond the homing angle"
        );
    }

    #[test]
    fn a_ghost_mob_in_reach_is_a_target() {
        let mut world = border();
        world.set_ghosts(
            &sim_b(),
            vec![mob_ghost(BORDER_X + 0.5, MOB_HEALTH, true)],
            NOW,
        );
        assert_eq!(
            world.swing(&connection(1), Aim::default(), AttackKind::Light, NOW),
            vec![Err(AttackRefusal::NoTarget)],
            "a returning mob is no target"
        );
        world.set_ghosts(
            &sim_b(),
            vec![mob_ghost(BORDER_X + 0.5, MOB_HEALTH, false)],
            NOW,
        );
        let results = world.swing(&connection(1), Aim::default(), AttackKind::Light, NOW);
        let [Ok(AttackOutcome::Remote(remote))] = results.as_slice() else {
            panic!("a remote hit");
        };
        assert_eq!(remote.hit.target, mob());
    }

    fn amount(hit: u32) -> f32 {
        f32::from(u16::try_from(hit).expect("small"))
    }

    #[test]
    fn a_hit_sends_the_attacker_a_2071() {
        let mut world = fight();
        world
            .attack(&connection(1), mob(), LIGHT, NOW)
            .expect("hit");
        assert_eq!(
            messages_to(&world.take_messages(), 1),
            vec![ServerMessage::DamageDealt(DamageDealtMsg::hit(
                1,
                mob(),
                amount(SWORD_HIT),
                false
            ))]
        );
        assert!(world.take_messages().is_empty(), "taken once");
        let entity = world.by_connection[&connection(1)];
        let mut player = world.world.get_mut::<Player>(entity).expect("player");
        let status = player.combat.take_change(NOW, NOW).expect("in combat");
        assert_eq!(status.in_combat, Some(true), "the hit starts the combat");
    }

    /// The shots and reload time of the test blunderbuss (retail `BlunderbussT2`: 2, 1.46 s).
    const BLUNDERBUSS_SHOTS: u8 = 2;
    const BLUNDERBUSS_RELOAD_MS: u64 = 1_460;

    /// [`fight_at`] `x` with player 1 wearing the kit sword, whose stats are a spread-shot weapon.
    fn shooter_at(x: f32) -> SimWorld {
        const BLUNDERBUSS: &str = "BlunderbussT2";
        let mut source = crate::combat::test_support::test_source();
        let sword = source.weapons[0].1;
        source.weapons.push((
            BLUNDERBUSS.to_owned(),
            WeaponRow {
                spread_shot: true,
                max_loaded_ammo: BLUNDERBUSS_SHOTS,
                reload_ms: BLUNDERBUSS_RELOAD_MS,
                ..sword
            },
        ));
        source
            .item_stats
            .push((gear::tests::SWORD.to_owned(), BLUNDERBUSS.to_owned()));
        let data = CombatData::build(source, &CombatRows::default()).expect("data");
        let mut world = fight_at(x);
        world.combat = committed().combat_settings(Arc::new(data), Arc::from([]));
        let entity = world.by_connection[&connection(1)];
        world.world.get_mut::<Player>(entity).expect("player").gear = Gear::starter(
            &gear::tests::kits(),
            gear::tests::CHARACTER,
            Some(gear::tests::BACKSTORY),
        );
        world.tick_hands(NOW, SERVER_TIME);
        world
    }

    /// A shot of `pellets` pellets on [`mob`], whose last pellet missed.
    fn shot(pellets: usize) -> SpreadshotRequestMsg {
        const HIT: [u8; 2] = [0x00, 0x10];
        const MISS: [u8; 2] = [0xff, 0xfe];
        let mut pellets = vec![HIT; pellets];
        if let Some(last) = pellets.last_mut() {
            *last = MISS;
        }
        SpreadshotRequestMsg {
            header: FacetHeader::of(0, 1),
            frame: 0,
            flag: 1,
            seed: 0,
            origin: [MOB_X + 2.0, 0.0, 0.0],
            direction: [0; 6],
            pellets,
            hits: vec![SpreadHit {
                raw_id: [0; RAW_ID_LEN],
                facet_entity: mob().swap_bytes(),
                extra: 0,
            }],
            times: vec![0],
        }
    }

    #[test]
    fn each_pellet_that_hit_is_one_hit_after_the_answer() {
        const SIX: usize = 6;
        const EIGHT: usize = 8;
        let mut world = shooter_at(MOB_X + 2.0);
        let request = shot(SIX);
        let results = world.spreadshot(&connection(1), &request, NOW);
        assert_eq!(results.len(), SIX - 1, "the missed pellet does nothing");
        assert!(results.iter().all(|result| matches!(
            result,
            Ok(AttackOutcome::Hit { target, .. }) if *target == mob()
        )));
        let messages = messages_to(&world.take_messages(), 1);
        let expected = SpreadshotProcessedMsg {
            request,
            damage: lowercase_crc32(DEFAULT_SPREADSHOT_DAMAGE_ID),
            config: SIX_PELLET_CONFIG,
        };
        let ServerMessage::SpreadshotProcessed(answer) = &messages[0] else {
            panic!("the answer comes first: {:?}", messages[0]);
        };
        // The answer does not carry the frame, origin and times, so compare its bytes.
        assert_eq!(answer.encode(), expected.encode());
        // 64 x 1.0: the held weapon's gear score against armor rating 0 lets all of it through.
        const PELLET_HIT: u32 = 64;
        const ALL_THROUGH: f32 = 1.0;
        let pellet =
            DamageDealtMsg::pellet(1, mob(), amount(PELLET_HIT), ALL_THROUGH, false, false);
        assert_eq!(
            messages[1..],
            vec![ServerMessage::DamageDealt(pellet); SIX - 1]
        );
        assert_eq!(
            world
                .spreadshot(
                    &connection(1),
                    &shot(EIGHT),
                    NOW + SPREADSHOT_MIN_INTERVAL_MS - 1
                )
                .as_slice(),
            [Err(AttackRefusal::TooFast)]
        );
        world.spreadshot(
            &connection(1),
            &shot(EIGHT),
            NOW + SPREADSHOT_MIN_INTERVAL_MS,
        );
        assert!(matches!(
            &messages_to(&world.take_messages(), 1)[0],
            ServerMessage::SpreadshotProcessed(answer) if answer.config == EIGHT_PELLET_CONFIG
        ));
    }

    #[test]
    fn a_shot_without_a_spread_shot_weapon_is_refused() {
        let mut world = fight();
        assert_eq!(
            world.spreadshot(&connection(1), &shot(1), NOW).as_slice(),
            [Err(AttackRefusal::NotWeaponAttack)]
        );
        assert!(messages_to(&world.take_messages(), 1).is_empty());
    }

    #[test]
    fn a_pellet_out_of_range_is_refused() {
        let mut world = shooter_at(MOB_X + SPREADSHOT_RANGE_M + 1.0);
        assert_eq!(
            world.spreadshot(&connection(1), &shot(2), NOW).as_slice(),
            [Err(AttackRefusal::OutOfReach)]
        );
    }

    #[test]
    fn a_remote_hit_sends_the_attacker_a_2071() {
        let mut world = border();
        world.set_ghosts(
            &sim_b(),
            vec![mob_ghost(BORDER_X + 1.0, MOB_HEALTH, false)],
            NOW,
        );
        world
            .attack(&connection(1), mob(), LIGHT, NOW)
            .expect("remote hit");
        assert_eq!(
            messages_to(&world.take_messages(), 1),
            vec![ServerMessage::DamageDealt(DamageDealtMsg::hit(
                1,
                mob(),
                amount(SWORD_HIT),
                false
            ))]
        );
    }

    #[test]
    fn a_blocked_mob_hit_does_no_damage() {
        let mut world = fight();
        let blocking = InputRecord {
            slots: nw_messages::InputSlots {
                count: 19,
                values: vec![0, 1 << 62],
                bytes: [0; 4],
            },
            ..InputRecord::default()
        };
        assert!(
            world
                .attack_input(&connection(1), &[blocking], NOW)
                .is_empty()
        );
        let mut now = NOW;
        while now < NOW + LONG_AFTER_MS {
            let output = tick(&mut world, now);
            assert!(output.mob_hits.is_empty(), "a blocked hit did damage");
            let messages = messages_to(&output.messages, 1);
            if messages.contains(&ServerMessage::MeleeHit(MeleeHitMsg::on(1, true))) {
                assert_eq!(health_of(&mut world, 1).current(), TEST_PLAYER_HEALTH);
                let stamina = player_stamina(&mut world, 1);
                assert!(stamina < 100.0, "the block took stamina: {stamina}");
                return;
            }
            now += TICK_MS;
        }
        panic!("the mob never swung");
    }

    fn player_stamina(world: &mut SimWorld, number: u64) -> f32 {
        let entity = world.player_by_number(number).expect("player");
        world
            .world
            .get::<Player>(entity)
            .expect("player")
            .stamina
            .current()
    }

    #[test]
    fn a_block_without_stamina_lets_the_hit_through() {
        let mut world = fight();
        let entity = world.player_by_number(1).expect("player");
        world
            .world
            .get_mut::<Player>(entity)
            .expect("player")
            .stamina
            .blocked_hit(LETHAL as f32);
        let blocking = InputRecord {
            slots: nw_messages::InputSlots {
                count: 19,
                values: vec![0, 1 << 62],
                bytes: [0; 4],
            },
            ..InputRecord::default()
        };
        world.attack_input(&connection(1), &[blocking], NOW);
        let mut now = NOW;
        while now < NOW + LONG_AFTER_MS {
            let output = tick(&mut world, now);
            let messages = messages_to(&output.messages, 1);
            assert!(
                !messages.contains(&ServerMessage::MeleeHit(MeleeHitMsg::on(1, true))),
                "no block at 0 stamina"
            );
            if !output.mob_hits.is_empty() {
                assert!(health_of(&mut world, 1).current() < TEST_PLAYER_HEALTH);
                return;
            }
            now += TICK_MS;
        }
        panic!("the mob never hit");
    }

    #[test]
    fn the_armor_fit_gives_the_retail_tutorial_shares() {
        const LEVEL_1_ARMOR: f64 = 109.336;
        const KEPT_GS_60: f64 = 0.7011;
        const KEPT_GS_75: f64 = 0.7541;
        assert!((1.0 - armor_share(LEVEL_1_ARMOR, 60) - KEPT_GS_60).abs() < 1e-3);
        assert!((1.0 - armor_share(LEVEL_1_ARMOR, 75) - KEPT_GS_75).abs() < 1e-3);
    }

    #[test]
    fn a_paced_swing_lands_after_its_hit_delay() {
        const DELAY_MS: u64 = 456;
        let mut world = fight();
        let pace = crate::mobs::Pace {
            attack_factor: 1.0,
            swing_gap_ms: (2_500, 4_000),
            speed_mps: 5.5,
            hit_delay_ms: DELAY_MS,
            block_stamina: crate::mobs::DEFAULT_BLOCK_STAMINA,
            gait: nw_sim_core::Gait::uniform(3.0, 5.5),
            sight_m: 20.0,
            hit_tag: crate::mobs::DEFAULT_HIT_TAG,
            hit_scale: 1.0,
            capsule: crate::mobs::DEFAULT_MOB_CAPSULE,
            reach: None,
            moves: &[],
            phase_two: None,
        };
        let mob = world
            .mobs
            .spawn_personal(test_spawn(MOB_X), 1, false, Some(pace), None, NOW)
            .expect("rows");
        let swing = MobAttackIntent {
            mob,
            target: 1,
            target_at: Position {
                x: MOB_X + 2.0,
                y: 10.0,
                z: 50.0,
            },
            mob_move: None,
        };
        assert!(
            world
                .swings_landing(vec![swing], NOW, SERVER_TIME)
                .is_empty()
        );
        assert!(
            world
                .swings_landing(Vec::new(), NOW + DELAY_MS - 1, SERVER_TIME)
                .is_empty()
        );
        assert_eq!(
            world.swings_landing(Vec::new(), NOW + DELAY_MS, SERVER_TIME),
            [(swing, None)]
        );
        assert!(
            world
                .swings_landing(Vec::new(), NOW + DELAY_MS + 1, SERVER_TIME)
                .is_empty(),
            "once"
        );
    }

    #[test]
    fn a_swing_at_a_player_out_of_reach_misses() {
        let mut world = fight();
        join(&mut world, 2, MOB_X + 10.0);
        let swing = |target, x| MobAttackIntent {
            mob: mob(),
            target,
            target_at: Position {
                x,
                y: 10.0,
                z: 50.0,
            },
            mob_move: None,
        };
        assert!(
            world
                .mob_hits(vec![(swing(2, MOB_X + 10.0), None)], NOW)
                .is_empty(),
            "10 m away"
        );
        assert!(messages_to(&world.take_messages(), 2).is_empty());
        assert_eq!(
            world
                .mob_hits(vec![(swing(1, MOB_X + 2.0), None)], NOW)
                .len(),
            1,
            "2 m away"
        );
    }

    #[test]
    fn a_mob_with_a_reach_lunges_to_the_player_and_hits() {
        const REACH: crate::mobs::MobReach = crate::mobs::MobReach {
            lunge_stop_m: 0.5,
            lunge_max_m: 2.0,
            lunge_turn_rad: std::f32::consts::FRAC_PI_4,
            reach_m: 1.4,
        };
        let pace = crate::mobs::Pace {
            attack_factor: 1.0,
            swing_gap_ms: (2_500, 4_000),
            speed_mps: 5.5,
            hit_delay_ms: 0,
            block_stamina: crate::mobs::DEFAULT_BLOCK_STAMINA,
            gait: nw_sim_core::Gait::uniform(3.0, 5.5),
            sight_m: 20.0,
            hit_tag: crate::mobs::DEFAULT_HIT_TAG,
            hit_scale: 1.0,
            capsule: crate::mobs::DEFAULT_MOB_CAPSULE,
            reach: Some(REACH),
            moves: &[],
            phase_two: None,
        };
        // Player 1 stands 3 m east of the paced mob: out of its 1.4 m reach until it lunges.
        let mut world = fight_at(MOB_X + 3.0);
        let mob = world
            .mobs
            .spawn_personal(test_spawn(MOB_X), 1, false, Some(pace), None, NOW)
            .expect("rows");
        let at = |x| Position {
            x,
            y: 10.0,
            z: 50.0,
        };
        let swing = |aimed_x| MobAttackIntent {
            mob,
            target: 1,
            target_at: at(aimed_x),
            mob_move: None,
        };
        assert_eq!(
            world.mob_hits(vec![(swing(MOB_X + 3.0), None)], NOW).len(),
            1
        );
        let lunged = world.mobs.get_mut(mob).expect("mob").brain.position();
        // It lunges its longest 2 m, which leaves 1 m to the player.
        assert!((at(MOB_X + 3.0).horizontal_distance(lunged) - 1.0).abs() < 1e-3);
        // Next to the player, it stops where the bodies touch: 0.5 + 0.4 m.
        assert_eq!(
            world.mob_hits(vec![(swing(MOB_X + 3.0), None)], NOW).len(),
            1
        );
        let touching = world.mobs.get_mut(mob).expect("mob").brain.position();
        assert!((at(MOB_X + 3.0).horizontal_distance(touching) - 0.9).abs() < 1e-3);
        // Aimed west of itself, the player east is beyond its lunge turn and behind its wedge.
        assert!(
            world
                .mob_hits(vec![(swing(MOB_X - 10.0), None)], NOW)
                .is_empty()
        );
    }

    #[test]
    fn a_swing_into_a_dodge_roll_misses() {
        let mut world = fight();
        let dodge = InputRecord {
            slots: nw_messages::InputSlots {
                count: 19,
                values: vec![0, 1 << 3],
                bytes: [0; 4],
            },
            ..InputRecord::default()
        };
        let mut now = NOW;
        assert!(
            world
                .attack_input(&connection(1), std::slice::from_ref(&dodge), now)
                .is_empty()
        );
        while now < NOW + DODGE_MS {
            let output = tick(&mut world, now);
            assert!(output.mob_hits.is_empty(), "a hit landed in the roll");
            assert!(
                messages_to(&output.messages, 1)
                    .iter()
                    .all(|message| !matches!(message, ServerMessage::MeleeHit(_)))
            );
            now += TICK_MS;
        }
        // Holding the key does not roll again; only a new press does.
        assert!(world.attack_input(&connection(1), &[dodge], now).is_empty());
        while now < NOW + LONG_AFTER_MS {
            if !tick(&mut world, now).mob_hits.is_empty() {
                return;
            }
            now += TICK_MS;
        }
        panic!("the mob never hit after the roll");
    }

    #[test]
    fn a_mob_hit_sends_the_player_a_3601() {
        let mut world = fight();
        let mut now = NOW;
        loop {
            let output = tick(&mut world, now);
            if let Some(AttackOutcome::Hit {
                target,
                amount: hit,
            }) = output.mob_hits.first()
            {
                assert_eq!(*target, 1);
                let messages = messages_to(&output.messages, 1);
                assert!(
                    messages.iter().any(|message| matches!(
                        message,
                        ServerMessage::OnDamage(damage)
                            if damage.items[0].amount == amount(*hit)
                                && damage.u64_c == mob().swap_bytes()
                    )),
                    "{messages:?}"
                );
                assert!(
                    messages.contains(&ServerMessage::MeleeHit(MeleeHitMsg::on(1, false))),
                    "{messages:?}"
                );
                return;
            }
            now += TICK_MS;
            assert!(now < NOW + LONG_AFTER_MS, "the mob never hit");
        }
    }

    /// A fight with a boss beside player 1: one move of two hits, and a second phase with
    /// double damage and a fire half.
    fn boss_fight() -> (SimWorld, u64, MobAttackIntent) {
        const HITS: [MoveHit; 2] = [
            MoveHit {
                at_ms: 300,
                damage_scale: 1.0,
                fire_share: 0.5,
                block_stamina: 10.0,
            },
            MoveHit {
                at_ms: 900,
                damage_scale: 0.5,
                fire_share: 0.5,
                block_stamina: 10.0,
            },
        ];
        const MOVE: crate::mobs::MobMove = crate::mobs::MobMove {
            fragment: crate::mobs::MoveFragment {
                fragment: 53,
                action: 19,
                tail: 0x8e,
            },
            length_ms: 2_800,
            hits: &HITS,
            tag: 4,
        };
        const MOVES: [crate::mobs::MobMove; 1] = [MOVE];
        let pace = crate::mobs::Pace {
            attack_factor: 1.0,
            swing_gap_ms: (1_000, 1_000),
            speed_mps: 5.5,
            hit_delay_ms: 0,
            block_stamina: crate::mobs::DEFAULT_BLOCK_STAMINA,
            gait: nw_sim_core::Gait::uniform(3.0, 5.5),
            sight_m: 20.0,
            hit_tag: crate::mobs::DEFAULT_HIT_TAG,
            hit_scale: 1.0,
            capsule: crate::mobs::DEFAULT_MOB_CAPSULE,
            reach: None,
            moves: &MOVES,
            phase_two: Some(crate::mobs::PhaseTwo {
                moves: &MOVES,
                damage_mult: 2.0,
                fire_tag: 9,
            }),
        };
        let mut world = fight();
        let boss = world
            .mobs
            .spawn_personal(test_spawn(MOB_X), 1, false, Some(pace), None, NOW)
            .expect("rows");
        let swing = MobAttackIntent {
            mob: boss,
            target: 1,
            target_at: Position {
                x: MOB_X + 2.0,
                y: 10.0,
                z: 50.0,
            },
            mob_move: Some(MOVE),
        };
        (world, boss, swing)
    }

    #[test]
    fn each_hit_of_a_move_lands_at_its_time() {
        let (mut world, _, swing) = boss_fight();
        assert!(
            world
                .swings_landing(vec![swing], NOW, SERVER_TIME)
                .is_empty()
        );
        let first = world.swings_landing(Vec::new(), NOW + 300, SERVER_TIME);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].1.map(|hit| hit.at_ms), Some(300));
        assert!(
            world
                .swings_landing(Vec::new(), NOW + 899, SERVER_TIME)
                .is_empty()
        );
        let second = world.swings_landing(Vec::new(), NOW + 900, SERVER_TIME);
        assert_eq!(second[0].1.map(|hit| hit.at_ms), Some(900));
    }

    #[test]
    fn a_second_phase_hit_is_larger_and_half_fire() {
        let (mut world, boss, swing) = boss_fight();
        let hit = swing.mob_move.expect("move").hits[0];
        let first_phase = world.mob_hits(vec![(swing, Some(hit))], NOW);
        let [AttackOutcome::Hit { amount: base, .. }] = first_phase.as_slice() else {
            panic!("a hit: {first_phase:?}");
        };
        let damage_items = |world: &mut SimWorld| -> Vec<(u8, f32)> {
            messages_to(&world.take_messages(), 1)
                .into_iter()
                .find_map(|message| match message {
                    ServerMessage::OnDamage(damage) => Some(
                        damage
                            .items
                            .iter()
                            .map(|item| (item.tag, item.amount))
                            .collect(),
                    ),
                    _ => None,
                })
                .expect("a 3601")
        };
        assert_eq!(damage_items(&mut world), vec![(4, amount(*base))]);
        world.mobs.enter_phase_two(boss);
        let second_phase = world.mob_hits(vec![(swing, Some(hit))], NOW);
        let [
            AttackOutcome::Hit {
                amount: doubled, ..
            },
        ] = second_phase.as_slice()
        else {
            panic!("a hit: {second_phase:?}");
        };
        assert_eq!(*doubled, 2 * base);
        let half = amount(*doubled) / 2.0;
        assert_eq!(damage_items(&mut world), vec![(4, half), (9, half)]);
    }

    #[test]
    fn spawn_protection_stops_mobs_until_the_player_moves() {
        let hit_on_one = |outcome: &AttackOutcome| {
            matches!(outcome,
                AttackOutcome::Hit { target, .. } | AttackOutcome::Killed { target } if *target == 1)
        };
        let mut world = world_with_spawns(vec![test_spawn(MOB_X)]);
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        // A fresh login (not a handoff): spawn-protected until its client moves.
        world
            .admit(&admit_at(1, MOB_X + 2.0, true), NOW, SERVER_TIME)
            .expect("admit");
        let mut now = NOW;
        while now < NOW + LONG_AFTER_MS {
            let output = tick(&mut world, now);
            assert!(
                !output.mob_hits.iter().any(hit_on_one),
                "a mob hit the player before it spawned",
            );
            now += TICK_MS;
        }
        assert_eq!(
            health_of(&mut world, 1).current(),
            TEST_PLAYER_HEALTH,
            "the protected player kept full health",
        );
        // The first movement ends the protection, so the same mob can now hit it.
        let next = Position {
            x: MOB_X + 2.0,
            y: 10.0,
            z: 50.0,
        };
        let _ = world.move_to(&connection(1), next, now);
        loop {
            let output = tick(&mut world, now);
            if output.mob_hits.iter().any(hit_on_one) {
                return;
            }
            now += TICK_MS;
            assert!(
                now < NOW + 2 * LONG_AFTER_MS,
                "the mob never hit after the spawn"
            );
        }
    }

    #[test]
    fn a_remote_mob_hit_sends_the_player_a_3601() {
        let mut world = border();
        assert!(world.apply_hit(1, mob(), SWORD_HIT, 0, NOW).is_some());
        let messages = messages_to(&world.take_messages(), 1);
        let [ServerMessage::OnDamage(damage)] = messages.as_slice() else {
            panic!("{messages:?}");
        };
        assert_eq!(damage.items[0].amount, amount(SWORD_HIT));
        assert_eq!(
            damage.u64_c,
            mob().swap_bytes(),
            "the attacker's facet entity"
        );
        assert_eq!(damage.raw_2, [0x02, 0x00], "a direct hit");
    }

    #[test]
    fn a_death_sends_a_4299() {
        let mut world = fight();
        world.apply_hit(1, mob(), LETHAL, 0, NOW);
        let messages = messages_to(&world.take_messages(), 1);
        assert_eq!(
            messages.last(),
            Some(&ServerMessage::DeathRecap(DeathRecapMsg::of(1)))
        );
        assert!(matches!(messages[0], ServerMessage::OnDamage(_)));
    }

    /// A fight whose player died at `NOW`.
    fn dead_fight() -> SimWorld {
        let mut world = fight();
        world.apply_hit(1, mob(), LETHAL, 0, NOW);
        world.take_messages();
        world
    }

    #[test]
    fn a_request_after_the_wait_respawns() {
        let mut world = dead_fight();
        assert_eq!(
            world.request_respawn(&connection(1), NOW + MIN_RESPAWN_MS),
            Ok(())
        );
        assert_eq!(health_of(&mut world, 1).current(), TEST_PLAYER_HEALTH);
    }

    #[test]
    fn a_request_too_soon_is_refused() {
        let mut world = dead_fight();
        assert_eq!(
            world.request_respawn(&connection(1), NOW + MIN_RESPAWN_MS - 1),
            Err(RespawnRefusal::TooSoon)
        );
        assert!(health_of(&mut world, 1).is_dead());
    }

    #[test]
    fn a_request_while_alive_is_refused() {
        let mut world = fight();
        assert_eq!(
            world.request_respawn(&connection(1), NOW),
            Err(RespawnRefusal::NotDead)
        );
        assert_eq!(
            world.request_respawn(&connection(7), NOW),
            Err(RespawnRefusal::NotInWorld)
        );
    }

    #[test]
    fn a_second_request_does_not_respawn_twice() {
        let mut world = dead_fight();
        let later = NOW + MIN_RESPAWN_MS;
        world
            .request_respawn(&connection(1), later)
            .expect("respawn");
        world.apply_hit(1, mob(), SWORD_HIT, 0, later);
        assert_eq!(
            world.request_respawn(&connection(1), later),
            Err(RespawnRefusal::NotDead)
        );
        assert_eq!(
            health_of(&mut world, 1).current(),
            TEST_PLAYER_HEALTH - SWORD_HIT,
            "no refill"
        );
    }

    #[test]
    fn a_refused_attack_sends_no_hit_message() {
        let mut world = fight();
        join(&mut world, 2, MOB_X + 4.5);
        assert_eq!(
            world.attack(&connection(2), mob(), LIGHT, NOW),
            Err(AttackRefusal::OutOfReach)
        );
        world.apply_hit(mob(), 9, LETHAL, 0, NOW);
        assert_eq!(
            world.attack(&connection(1), mob(), LIGHT, NOW),
            Err(AttackRefusal::TargetDead)
        );
        assert!(world.take_messages().is_empty());
    }

    #[test]
    fn a_dead_player_cannot_attack() {
        let mut world = fight();
        assert!(matches!(
            world.apply_hit(1, mob(), LETHAL, 0, NOW),
            Some(AttackOutcome::Killed { .. })
        ));
        assert_eq!(
            world.attack(&connection(1), mob(), LIGHT, NOW),
            Err(AttackRefusal::Dead)
        );
    }

    #[test]
    fn an_unknown_target_is_refused() {
        let mut world = fight();
        assert_eq!(
            world.attack(&connection(1), mob_entity(99), LIGHT, NOW),
            Err(AttackRefusal::UnknownTarget)
        );
        assert_eq!(
            world.attack(&connection(7), mob(), LIGHT, NOW),
            Err(AttackRefusal::NotInWorld)
        );
    }

    #[test]
    fn a_returning_mob_is_refused() {
        let mut world = fight();
        let settings = test_combat().brain;
        let brain = &mut world.mobs.get_mut(mob()).expect("mob").brain;
        hurt(brain, 99, 1, NOW);
        brain_step(brain, &[], NOW + TICK_MS, &settings, &mut Dice::new(1));
        assert_eq!(brain.state(), MobState::Return);
        assert_eq!(
            world.attack(&connection(1), mob(), LIGHT, NOW + TICK_MS),
            Err(AttackRefusal::Returning)
        );
    }

    #[test]
    fn an_attack_not_of_the_weapon_is_refused() {
        let mut world = fight();
        assert_eq!(
            world.attack(&connection(1), mob(), "Spoon_Attack", NOW),
            Err(AttackRefusal::NotWeaponAttack)
        );
    }

    #[test]
    fn an_attack_out_of_reach_is_refused() {
        let mut world = fight();
        join(&mut world, 2, MOB_X + 4.5);
        join(&mut world, 3, MOB_X + 3.9);
        assert_eq!(
            world.attack(&connection(2), mob(), LIGHT, NOW),
            Err(AttackRefusal::OutOfReach)
        );
        assert!(
            world.attack(&connection(3), mob(), LIGHT, NOW).is_ok(),
            "reach plus margin"
        );
    }

    #[test]
    fn attacks_faster_than_the_interval_are_refused() {
        let mut world = fight();
        assert!(world.attack(&connection(1), mob(), LIGHT, NOW).is_ok());
        assert_eq!(
            world.attack(&connection(1), mob(), LIGHT, NOW + INTERVAL_MS - 1),
            Err(AttackRefusal::TooFast)
        );
        assert!(
            world
                .attack(&connection(1), mob(), LIGHT, NOW + INTERVAL_MS)
                .is_ok()
        );
    }

    #[test]
    fn a_player_cannot_attack_a_player() {
        let mut world = fight();
        join(&mut world, 2, MOB_X + 1.0);
        assert_eq!(
            world.attack(&connection(1), 2, LIGHT, NOW),
            Err(AttackRefusal::UnknownTarget)
        );
        assert_eq!(
            world.attack(&connection(1), 1, LIGHT, NOW),
            Err(AttackRefusal::UnknownTarget)
        );
    }

    /// Player `number` hits the mob every interval from `from` until it dies; returns the time
    /// of the killing hit and the number of hits.
    fn kill(world: &mut SimWorld, number: u64, from: u64) -> (u64, u32) {
        let mut now = from;
        for hits in 1..=MOB_HEALTH {
            match world
                .attack(&connection(number), mob(), LIGHT, now)
                .expect("hit")
            {
                AttackOutcome::Killed { target } => {
                    assert_eq!(target, mob());
                    return (now, hits);
                }
                AttackOutcome::Hit { amount, .. } => assert_eq!(amount, SWORD_HIT),
                AttackOutcome::Remote(remote) => panic!("a local mob, not {remote:?}"),
            }
            now += INTERVAL_MS;
        }
        panic!("the mob never died");
    }

    #[test]
    fn a_player_kills_a_mob_and_it_respawns() {
        let mut world = fight();
        let (killed_at, hits) = kill(&mut world, 1, NOW);
        assert_eq!(hits, MOB_HEALTH.div_ceil(SWORD_HIT));
        assert_eq!(
            world.attack(&connection(1), mob(), LIGHT, killed_at + INTERVAL_MS),
            Err(AttackRefusal::TargetDead)
        );
        tick(&mut world, killed_at + MOB_RESPAWN_MS - 1);
        assert!(mob_health(&mut world).is_dead());
        tick(&mut world, killed_at + MOB_RESPAWN_MS);
        assert_eq!(mob_health(&mut world).current(), MOB_HEALTH);
    }

    #[test]
    fn hits_on_a_dying_mob_kill_it_once() {
        let mut world = fight();
        join(&mut world, 2, MOB_X - 2.0);
        let mut now = NOW;
        while mob_health(&mut world).current() > SWORD_HIT {
            world
                .attack(&connection(1), mob(), LIGHT, now)
                .expect("hit");
            now += INTERVAL_MS;
        }
        assert_eq!(
            world.attack(&connection(1), mob(), LIGHT, now),
            Ok(AttackOutcome::Killed { target: mob() })
        );
        assert_eq!(
            world.attack(&connection(2), mob(), LIGHT, now),
            Err(AttackRefusal::TargetDead)
        );
        assert_eq!(
            world.apply_hit(mob(), 2, SWORD_HIT, 0, now),
            None,
            "a late remote hit too"
        );
    }

    #[test]
    fn a_mob_kills_a_player_who_then_respawns_where_it_died() {
        let mut world = fight();
        let mut deaths = Vec::new();
        let mut now = NOW;
        while deaths.is_empty() && now < NOW + 60_000 {
            deaths = tick(&mut world, now).deaths;
            now += TICK_MS;
        }
        assert_eq!(deaths.len(), 1, "killed once");
        assert_eq!(deaths[0].connection, connection(1));
        assert_eq!(deaths[0].position.x, MOB_X + 2.0, "saved where it died");
        let died_at = now - TICK_MS;
        let later = died_at + LONG_AFTER_MS;
        tick(&mut world, later);
        assert!(
            health_of(&mut world, 1).is_dead(),
            "no respawn without a request"
        );
        world
            .request_respawn(&connection(1), later)
            .expect("respawn");
        assert_eq!(health_of(&mut world, 1).current(), TEST_PLAYER_HEALTH);
        let entity = world.player_by_number(1).expect("player");
        let body = world.world.get::<Body>(entity).expect("body");
        assert_eq!(body.motion.position().x, MOB_X + 2.0, "where it died");
    }

    #[test]
    fn a_dead_player_cannot_move() {
        let mut world = fight();
        world.apply_hit(1, mob(), LETHAL, 0, NOW);
        let next = Position {
            x: MOB_X + 2.5,
            y: 10.0,
            z: 50.0,
        };
        assert_eq!(
            world.move_to(&connection(1), next, NOW + 100),
            Err(MoveError::Dead)
        );
    }

    #[test]
    fn a_dead_player_is_forgotten_by_mobs() {
        let mut world = fight();
        world
            .attack(&connection(1), mob(), LIGHT, NOW)
            .expect("hit");
        assert_eq!(
            world.mobs.get_mut(mob()).expect("mob").brain.target(),
            Some(1)
        );
        world.apply_hit(1, mob(), LETHAL, 0, NOW);
        tick(&mut world, NOW + TICK_MS);
        let mob = world.mobs.get_mut(mob()).expect("mob");
        assert_eq!(mob.brain.target(), None);
        assert_eq!(mob.brain.state(), MobState::Return);
    }

    #[test]
    fn a_death_is_saved_at_once() {
        let mut world = fight();
        world.apply_hit(1, mob(), LETHAL, 0, NOW);
        let deaths = tick(&mut world, NOW + TICK_MS).deaths;
        assert_eq!(deaths.len(), 1);
        assert_eq!(deaths[0].connection, connection(1));
        assert!(
            tick(&mut world, NOW + 2 * TICK_MS).deaths.is_empty(),
            "once"
        );
    }

    fn sim_b() -> BusId {
        slot("sim-b").id
    }

    /// A mob ghost of sim-b at `x`.
    fn mob_ghost(x: f32, health: u32, returning: bool) -> Ghost {
        Ghost {
            entity: mob(),
            position: Point {
                x,
                y: 10.0,
                z: 50.0,
            },
            mob: Some(MobGhost {
                health,
                max_health: MOB_HEALTH,
                returning,
                vitals_id: 0,
                category_id: 0,
                level: 0,
            }),
        }
    }

    /// A world of sim-a with player 1 just west of the border.
    fn border() -> SimWorld {
        let mut world = world_with_spawns(Vec::new());
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, BORDER_X - 1.5);
        world
    }

    #[test]
    fn an_attack_on_a_ghost_goes_to_its_owner() {
        let mut world = border();
        world.set_ghosts(
            &sim_b(),
            vec![mob_ghost(BORDER_X + 1.0, MOB_HEALTH, false)],
            NOW,
        );
        let outcome = world
            .attack(&connection(1), mob(), LIGHT, NOW)
            .expect("remote hit");
        let AttackOutcome::Remote(remote) = outcome else {
            panic!("not remote: {outcome:?}");
        };
        assert_eq!(remote.owner, sim_b());
        assert_eq!((remote.hit.target, remote.hit.attacker), (mob(), 1));
        assert_eq!(remote.hit.amount, SWORD_HIT);
        assert_eq!(remote.hit.attacker_sim, slot("sim-a").id);
    }

    #[test]
    fn a_ghost_mob_is_checked_like_a_local_one() {
        let mut world = border();
        world.set_ghosts(
            &sim_b(),
            vec![mob_ghost(BORDER_X + 1.0, MOB_HEALTH, true)],
            NOW,
        );
        assert_eq!(
            world.attack(&connection(1), mob(), LIGHT, NOW),
            Err(AttackRefusal::Returning)
        );
        world.set_ghosts(&sim_b(), vec![mob_ghost(BORDER_X + 1.0, 0, false)], NOW);
        assert_eq!(
            world.attack(&connection(1), mob(), LIGHT, NOW),
            Err(AttackRefusal::TargetDead)
        );
        world.set_ghosts(
            &sim_b(),
            vec![mob_ghost(BORDER_X + 9.0, MOB_HEALTH, false)],
            NOW,
        );
        assert_eq!(
            world.attack(&connection(1), mob(), LIGHT, NOW),
            Err(AttackRefusal::OutOfReach)
        );
    }

    #[test]
    fn a_mob_hits_a_ghost_player_through_its_sim() {
        let mut world = world_with_spawns(vec![test_spawn(BORDER_X - 1.0)]);
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        let player = Ghost {
            entity: 9,
            position: Point {
                x: BORDER_X + 1.0,
                y: 10.0,
                z: 50.0,
            },
            mob: None,
        };
        let mut remote = None;
        for tick_number in 0..10 {
            let now = NOW + tick_number * TICK_MS;
            world.set_ghosts(&sim_b(), vec![player], now);
            remote = tick(&mut world, now)
                .mob_hits
                .into_iter()
                .find_map(|outcome| match outcome {
                    AttackOutcome::Remote(remote) => Some(remote),
                    _ => None,
                });
            if remote.is_some() {
                break;
            }
        }
        let remote = remote.expect("the mob hit the ghost player");
        assert_eq!(remote.owner, sim_b());
        assert_eq!((remote.hit.target, remote.hit.attacker), (9, mob()));
    }

    #[test]
    fn a_hit_for_a_mob_of_a_lost_cell_is_dropped() {
        let mut world = world_with_spawns(vec![test_spawn(MOB_X)]);
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        world.set_table(&table(&["sim-b", "sim-a"])).expect("table");
        assert_eq!(world.apply_hit(mob(), 1, SWORD_HIT, 0, NOW), None);
    }

    #[test]
    fn a_hit_on_a_dead_mob_is_dropped() {
        let mut world = fight();
        world.apply_hit(mob(), 1, LETHAL, 0, NOW);
        assert_eq!(world.apply_hit(mob(), 1, SWORD_HIT, 0, NOW), None);
    }

    /// Ground at one height everywhere.
    struct Flat;

    impl nw_sim_core::Ground for Flat {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            Some(STRESS_HEIGHT)
        }
    }

    const STRESS_HEIGHT: f32 = 50.0;
    const STRESS_FIGHTS: u32 = 1000;
    /// Fights along one row, this far apart: a mob sees the players of its neighbours too.
    const STRESS_SPACING_M: f32 = 14.0;
    const STRESS_TICKS: u64 = 1800;
    const STRESS_THREADS: usize = 4;
    const STRESS_MOB_RESPAWN_MS: u64 = 5_000;
    const STRESS_VISIBLE: usize = 47;
    const ONE_TICK_MS: f64 = 33.0;
    const P99: f64 = 0.99;

    /// M6-A spec section 12.2: run with
    /// `cargo test --release -p sim fight_stress -- --ignored --nocapture`.
    #[test]
    #[ignore = "release-mode stress test of the M6-A exit"]
    fn fight_stress_keeps_ticks_under_one_tick() {
        let mut combat = test_combat();
        combat.brain.mob_respawn_ms = STRESS_MOB_RESPAWN_MS;
        let x_of = |fight: u32| STRESS_SPACING_M * (1.0 + fight as f32);
        combat.spawns = (0..STRESS_FIGHTS)
            .map(|fight| test_spawn(x_of(fight)))
            .collect();
        let mut world = SimWorld::new(
            WorldSettings {
                tick_threads: STRESS_THREADS,
                ..crate::world::tests::settings(STRESS_VISIBLE, STRESS_FIGHTS * 2)
            },
            crate::login::WorldIds::random(0).expect("ids"),
            std::sync::Arc::new(Flat),
            slot("sim-a"),
            combat,
            std::sync::Arc::default(),
        );
        world.set_table(&table(&["sim-a"])).expect("table");
        for fight in 0..STRESS_FIGHTS {
            join(&mut world, u64::from(fight) + 1, x_of(fight) + 2.0);
        }
        let mut next_attack = vec![NOW; STRESS_FIGHTS as usize];
        let mut times: Vec<f64> = Vec::new();
        let (mut hits, mut kills, mut respawns) = (0_u64, 0_u64, 0_u64);
        for tick_number in 0..STRESS_TICKS {
            let now = NOW + tick_number * TICK_MS;
            let started = std::time::Instant::now();
            for fight in 0..STRESS_FIGHTS {
                if next_attack[fight as usize] > now {
                    continue;
                }
                next_attack[fight as usize] = now + INTERVAL_MS;
                let attacker = connection(u64::from(fight) + 1);
                match world.attack(&attacker, mob_entity(fight), LIGHT, now) {
                    Ok(AttackOutcome::Killed { .. }) => kills += 1,
                    Ok(_) => hits += 1,
                    Err(AttackRefusal::Dead) => {
                        respawns += u64::from(world.request_respawn(&attacker, now).is_ok());
                    }
                    Err(_) => {}
                }
            }
            world.tick(now, 0).expect("tick");
            world.take_messages();
            times.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        let at = |share: f64| times[((times.len() - 1) as f64 * share) as usize];
        let (p50, p99, max) = (at(0.5), at(P99), times[times.len() - 1]);
        println!(
            "fight stress: {STRESS_FIGHTS} fights, {STRESS_TICKS} ticks, {hits} hits, {kills} kills, {respawns} respawns; tick ms p50 {p50:.2}, p99 {p99:.2}, max {max:.2}"
        );
        assert!(kills > 0, "mobs died and respawned");
        assert!(p99 < ONE_TICK_MS, "tick p99 {p99:.2} ms");
    }

    #[test]
    fn a_dead_player_is_not_handed_off() {
        let mut world = world_with_spawns(Vec::new());
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, BORDER_X + 50.0);
        world.apply_hit(1, mob(), LETHAL, 0, NOW);
        assert!(
            tick(&mut world, NOW).handoffs.is_empty(),
            "it stays until its respawn"
        );
        world
            .request_respawn(&connection(1), NOW + MIN_RESPAWN_MS)
            .expect("respawn");
        assert_eq!(tick(&mut world, NOW + MIN_RESPAWN_MS).handoffs.len(), 1);
    }

    #[test]
    fn a_hit_on_a_frozen_player_is_dropped() {
        let mut world = world_with_spawns(Vec::new());
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, BORDER_X + 50.0);
        let offer = tick(&mut world, NOW).handoffs[0].offer.clone();
        assert_eq!(
            world.apply_hit(1, mob(), LETHAL, 0, NOW),
            None,
            "the offer has its health"
        );
        assert_eq!(health_of(&mut world, 1).current(), offer.health);
    }

    #[test]
    fn a_dead_player_is_no_ghost() {
        let mut world = world_with_spawns(Vec::new());
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, BORDER_X - 50.0);
        assert_eq!(tick(&mut world, NOW).ghosts[0].1.len(), 1);
        world.apply_hit(1, mob(), LETHAL, 0, NOW);
        let ghosts = tick(&mut world, NOW + TICK_MS).ghosts;
        assert!(
            ghosts.iter().all(|(_, entities)| entities.is_empty()),
            "{ghosts:?}"
        );
    }

    #[test]
    fn a_handoff_keeps_the_players_health() {
        let mut world = world_with_spawns(Vec::new());
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, BORDER_X + 50.0);
        world.apply_hit(1, mob(), SWORD_HIT, 0, NOW);
        let offer = tick(&mut world, NOW).handoffs[0].offer.clone();
        assert_eq!(offer.health, TEST_PLAYER_HEALTH - SWORD_HIT);
        let mut next = world_with_spawns(Vec::new());
        next.take(&offer, NOW, SERVER_TIME).expect("take");
        assert_eq!(
            health_of(&mut next, 1).current(),
            TEST_PLAYER_HEALTH - SWORD_HIT
        );
    }
}
