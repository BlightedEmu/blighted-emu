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
//! hit applied on a player sends it a 3601, and a death a 4299. They wait in the world until
//! [`SimWorld::take_messages`] or the tick takes them.
//!
//! [`SimWorld::attack_input`] turns the attack bits of a client's input records into attacks on
//! the nearest mob (M6-B spec section 6).

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use nw_bus::{BusId, ConnectionRef, Hit};
use nw_messages::{DamageDealtMsg, DeathRecapMsg, InputRecord, OnDamageMsg, ServerMessage};
use nw_sim_core::{Damage, Health, MobState, PlayerAttack, died, hurt, mob_hit, player_hit};

use super::{Body, MESSAGE_CHANNEL, Outgoing, Player, Position, SimWorld};
use crate::combat::{AttackOutcome, AttackRefusal, RemoteHit, RespawnRefusal};
use crate::mobs::{MobAttackIntent, spawn_index};

/// A player's health and attack times.
#[derive(Component, Debug, Clone, Copy)]
pub(super) struct Vitals {
    pub(super) health: Health,
    pub(super) dead_since_ms: Option<u64>,
    last_attack_ms: Option<u64>,
    /// The attack bits of the last input record.
    attack_bits: u8,
}

impl Vitals {
    pub(super) const fn new(health: Health) -> Self {
        Self {
            health,
            dead_since_ms: None,
            last_attack_ms: None,
            attack_bits: 0,
        }
    }
}

/// A health amount as the `f32` of a hit message.
#[allow(clippy::cast_precision_loss, reason = "health stays far below 2^24")]
const fn amount_of(amount: u32) -> f32 {
    amount as f32
}

/// What an attack sees of its target.
struct TargetView {
    position: Position,
    dead: bool,
    returning: bool,
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
        let reach_m = self.combat.brain.attack_reach_m + self.combat.reach_margin_m;
        if view.position.horizontal_distance(position) > reach_m {
            return Err(AttackRefusal::OutOfReach);
        }
        let too_fast = vitals
            .last_attack_ms
            .is_some_and(|last| now_ms.saturating_sub(last) < self.combat.min_attack_interval_ms);
        if too_fast {
            return Err(AttackRefusal::TooFast);
        }
        let weapon = self.combat.data.weapon();
        let attack = PlayerAttack {
            base_damage: weapon.base_damage,
            coefficient: row.coefficient,
            can_crit: row.can_crit,
            crit_chance: weapon.crit_chance,
            crit_multiplier: weapon.crit_multiplier,
        };
        let threat_base = row.threat;
        let hit = player_hit(&attack, self.combat.armor_share, &mut self.dice);
        let threat = threat_base.saturating_add(u64::from(hit.amount));
        if let Some(mut vitals) = self.world.get_mut::<Vitals>(entity) {
            vitals.last_attack_ms = Some(now_ms);
        }
        let outcome = match view.owner {
            None => self
                .apply_hit(target, number, hit.amount, threat, now_ms)
                .ok_or(AttackRefusal::TargetDead)?,
            Some(owner) => {
                AttackOutcome::Remote(self.remote_hit(owner, target, number, hit.amount, threat))
            }
        };
        self.send_reliable(
            attacker.clone(),
            &ServerMessage::DamageDealt(DamageDealtMsg::hit(target, number, amount_of(hit.amount))),
        );
        Ok(outcome)
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

    /// One result per rising edge of the attack bits in `records`, in record order: a record that
    /// sets any attack bit the record before it did not set (M6-B spec section 6.1). The first
    /// edge attacks; the others of the same message are [`AttackRefusal::TooFast`]. The bits of
    /// the last record carry over to the next message. Nothing for a connection without a player.
    pub fn attack_input(
        &mut self,
        connection: &ConnectionRef,
        records: &[InputRecord],
        now_ms: u64,
    ) -> Vec<Result<AttackOutcome, AttackRefusal>> {
        let Some(mut vitals) = self
            .by_connection
            .get(connection)
            .copied()
            .and_then(|entity| self.world.get_mut::<Vitals>(entity))
        else {
            return Vec::new();
        };
        let mut edges = 0;
        for bits in records.iter().map(InputRecord::attack_bits) {
            if bits & !vitals.attack_bits != 0 {
                edges += 1;
            }
            vitals.attack_bits = bits;
        }
        // Every edge of one message has the same receive time, so only the first can attack: the
        // others are too fast, and a client cannot make the loop search for targets per record.
        if edges == 0 {
            return Vec::new();
        }
        std::iter::once(self.attack_nearest(connection, now_ms))
            .chain((1..edges).map(|_| Err(AttackRefusal::TooFast)))
            .collect()
    }

    /// The player of `attacker` attacks the nearest living, not returning mob in reach, here or
    /// a ghost, with the first weapon attack (M6-B spec sections 6.2 and 6.3).
    ///
    /// # Errors
    ///
    /// Returns [`AttackRefusal::NoTarget`] without such a mob, or the refusal of
    /// [`SimWorld::attack`].
    pub fn attack_nearest(
        &mut self,
        attacker: &ConnectionRef,
        now_ms: u64,
    ) -> Result<AttackOutcome, AttackRefusal> {
        let entity = *self
            .by_connection
            .get(attacker)
            .ok_or(AttackRefusal::NotInWorld)?;
        if self
            .world
            .get::<Vitals>(entity)
            .is_some_and(|vitals| vitals.health.is_dead())
        {
            return Err(AttackRefusal::Dead);
        }
        let position = self
            .world
            .get::<Body>(entity)
            .ok_or(AttackRefusal::NotInWorld)?
            .motion
            .position();
        let reach_m = self.combat.brain.attack_reach_m + self.combat.reach_margin_m;
        let target = self
            .mob_states()
            .filter(|(_, _, state)| state.health > 0 && !state.returning)
            .map(|(mob, at, _)| (mob, at.horizontal_distance(position)))
            .filter(|(_, distance)| *distance <= reach_m)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(mob, _)| mob)
            .ok_or(AttackRefusal::NoTarget)?;
        let damage_id = self
            .combat
            .data
            .first_attack()
            .ok_or(AttackRefusal::NotWeaponAttack)?
            .damage_id
            .clone();
        self.attack(attacker, target, &damage_id, now_ms)
    }

    /// What an attack sees of the mob `target`: here, or a ghost of its owner.
    fn target_view(&mut self, target: u64) -> Option<TargetView> {
        if let Some(mob) = self.mobs.get_mut(target) {
            return Some(TargetView {
                position: mob.brain.position(),
                dead: mob.health.is_dead(),
                returning: mob.brain.state() == MobState::Return,
                owner: None,
            });
        }
        let (owner, ghost) = self.ghost(target)?;
        let state = ghost.mob?;
        Some(TargetView {
            position: ghost.position,
            dead: state.health == 0,
            returning: state.returning,
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
        if player.frozen {
            return None;
        }
        let connection = player.connection.clone();
        let mut vitals = self.world.get_mut::<Vitals>(entity)?;
        let damage = vitals.health.take(amount);
        if damage == Damage::Killed {
            vitals.dead_since_ms = Some(now_ms);
        }
        if damage == Damage::AlreadyDead {
            return None;
        }
        self.send_reliable(
            connection.clone(),
            &ServerMessage::OnDamage(OnDamageMsg::hit(target, attacker, amount_of(amount))),
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
            &ServerMessage::DeathRecap(DeathRecapMsg::of(target, attacker)),
        );
        Some(AttackOutcome::Killed { target })
    }

    /// The player with entity number `number`.
    pub(super) fn player_by_number(&mut self, number: u64) -> Option<Entity> {
        let mut query = self.world.query::<(Entity, &Player)>();
        query
            .iter(&self.world)
            .find(|(_, player)| player.number == number)
            .map(|(entity, _)| entity)
    }

    /// Applies the mobs' hits on the players of this sim; a hit on a player ghost goes to its
    /// sim.
    pub(super) fn mob_hits(
        &mut self,
        intents: Vec<MobAttackIntent>,
        now_ms: u64,
    ) -> Vec<AttackOutcome> {
        let armor_share = self.combat.player_armor_share;
        intents
            .into_iter()
            .filter_map(|intent| {
                let hit = mob_hit(&self.mobs.get_mut(intent.mob)?.attack, armor_share);
                if self.player_by_number(intent.target).is_some() {
                    return self.apply_hit(intent.target, intent.mob, hit.amount, 0, now_ms);
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
    use crate::mobs::mob_entity;
    use crate::world::tests::{
        BORDER_X, CELL_M, NOW, connection, join, slot, table, test_spawn, tick, world_with_spawns,
    };
    use crate::world::{MESSAGE_CHANNEL, MoveError, Outgoing, WorldSettings};
    use nw_bus::{Ghost, MobGhost, Point};
    use nw_messages::{DamageDealtMsg, DeathRecapMsg, OnDamageMsg, ServerMessage};
    use nw_reflect::Envelope;

    const LIGHT: &str = "1H_Sword_Attack1";
    const MOB_X: f32 = CELL_M / 2.0;
    /// 64 x 1.0 x (1 - 0.35), rounded.
    const SWORD_HIT: u32 = 42;
    const MOB_HEALTH: u32 = 1000;
    const INTERVAL_MS: u64 = 500;
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
        let mut world = world_with_spawns(vec![test_spawn(MOB_X)]);
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, MOB_X + 2.0);
        world
    }

    fn health_of(world: &mut SimWorld, number: u64) -> Health {
        let entity = world.player_by_number(number).expect("player");
        world.world.get::<Vitals>(entity).expect("vitals").health
    }

    fn mob_health(world: &mut SimWorld) -> Health {
        world.mobs.get_mut(mob()).expect("mob").health
    }

    /// Mask bits 9 and 10, the first two attack bits.
    const FIRST_ATTACK_BIT: u32 = 1 << 9;
    const SECOND_ATTACK_BIT: u32 = 1 << 10;

    fn record(mask: u32) -> InputRecord {
        InputRecord {
            mask,
            ..InputRecord::default()
        }
    }

    /// A fight after one tick, which wakes the mob.
    fn awake_fight() -> SimWorld {
        let mut world = fight();
        tick(&mut world, NOW);
        world
    }

    #[test]
    fn a_rising_edge_attacks_once() {
        let mut world = awake_fight();
        let results = world.attack_input(&connection(1), &[record(FIRST_ATTACK_BIT)], NOW);
        assert_eq!(
            results,
            vec![Ok(AttackOutcome::Hit {
                target: mob(),
                amount: SWORD_HIT
            })]
        );
    }

    #[test]
    fn a_held_bit_does_not_attack_again() {
        let mut world = awake_fight();
        let held = [record(FIRST_ATTACK_BIT)];
        assert_eq!(world.attack_input(&connection(1), &held, NOW).len(), 1);
        assert!(
            world
                .attack_input(&connection(1), &held, NOW + INTERVAL_MS)
                .is_empty()
        );
        let again = [record(0), record(FIRST_ATTACK_BIT)];
        assert_eq!(
            world
                .attack_input(&connection(1), &again, NOW + 2 * INTERVAL_MS)
                .len(),
            1
        );
    }

    #[test]
    fn edges_inside_one_message_count_once_each() {
        let mut world = awake_fight();
        let records = [
            record(FIRST_ATTACK_BIT | SECOND_ATTACK_BIT),
            record(0),
            record(FIRST_ATTACK_BIT),
            record(FIRST_ATTACK_BIT),
        ];
        let results = world.attack_input(&connection(1), &records, NOW);
        assert_eq!(results.len(), 2, "{results:?}");
        assert!(results[0].is_ok());
        assert_eq!(results[1], Err(AttackRefusal::TooFast));
    }

    #[test]
    fn extra_edges_in_one_message_are_too_fast_without_a_search() {
        let mut world = awake_fight();
        join(&mut world, 2, MOB_X + 10.0);
        let records = [
            record(FIRST_ATTACK_BIT),
            record(0),
            record(FIRST_ATTACK_BIT),
            record(0),
            record(FIRST_ATTACK_BIT),
        ];
        assert_eq!(
            world.attack_input(&connection(2), &records, NOW),
            vec![
                Err(AttackRefusal::NoTarget),
                Err(AttackRefusal::TooFast),
                Err(AttackRefusal::TooFast)
            ]
        );
    }

    #[test]
    fn no_mob_in_reach_is_no_target() {
        let mut world = awake_fight();
        join(&mut world, 2, MOB_X + 10.0);
        assert_eq!(
            world.attack_nearest(&connection(2), NOW),
            Err(AttackRefusal::NoTarget)
        );
        assert_eq!(
            world.attack_nearest(&connection(7), NOW),
            Err(AttackRefusal::NotInWorld)
        );
    }

    #[test]
    fn the_nearest_mob_is_the_target() {
        let mut world = world_with_spawns(vec![test_spawn(MOB_X), test_spawn(MOB_X + 3.0)]);
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, MOB_X + 2.0);
        tick(&mut world, NOW);
        assert!(matches!(
            world.attack_nearest(&connection(1), NOW),
            Ok(AttackOutcome::Hit { target, .. }) if target == mob_entity(1)
        ));
    }

    #[test]
    fn a_ghost_mob_in_reach_is_a_target() {
        let mut world = border();
        world.set_ghosts(
            &sim_b(),
            vec![mob_ghost(BORDER_X + 1.0, MOB_HEALTH, true)],
            NOW,
        );
        assert_eq!(
            world.attack_nearest(&connection(1), NOW),
            Err(AttackRefusal::NoTarget),
            "a returning mob is no target"
        );
        world.set_ghosts(
            &sim_b(),
            vec![mob_ghost(BORDER_X + 1.0, MOB_HEALTH, false)],
            NOW,
        );
        let Ok(AttackOutcome::Remote(remote)) = world.attack_nearest(&connection(1), NOW) else {
            panic!("a remote hit");
        };
        assert_eq!(remote.hit.target, mob());
    }

    /// The reliable messages of player `number` among `outgoing`.
    fn messages_to(outgoing: &[Outgoing], number: u64) -> Vec<ServerMessage> {
        outgoing
            .iter()
            .filter(|out| {
                out.connection == connection(number)
                    && out.reliable
                    && out.channel == MESSAGE_CHANNEL
            })
            .flat_map(|out| &out.envelopes)
            .map(|bytes| {
                let envelope = Envelope::decode(bytes).expect("envelope");
                ServerMessage::from_envelope(&envelope)
                    .expect("known")
                    .expect("decodes")
            })
            .collect()
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
                mob(),
                1,
                amount(SWORD_HIT)
            ))]
        );
        assert!(world.take_messages().is_empty(), "taken once");
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
                mob(),
                1,
                amount(SWORD_HIT)
            ))]
        );
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
                    messages.contains(&ServerMessage::OnDamage(OnDamageMsg::hit(
                        1,
                        mob(),
                        amount(*hit)
                    ))),
                    "{messages:?}"
                );
                return;
            }
            now += TICK_MS;
            assert!(now < NOW + LONG_AFTER_MS, "the mob never hit");
        }
    }

    #[test]
    fn a_remote_mob_hit_sends_the_player_a_3601() {
        let mut world = border();
        assert!(world.apply_hit(1, mob(), SWORD_HIT, 0, NOW).is_some());
        assert_eq!(
            messages_to(&world.take_messages(), 1),
            vec![ServerMessage::OnDamage(OnDamageMsg::hit(
                1,
                mob(),
                amount(SWORD_HIT)
            ))]
        );
    }

    #[test]
    fn a_death_sends_a_4299() {
        let mut world = fight();
        world.apply_hit(1, mob(), LETHAL, 0, NOW);
        let messages = messages_to(&world.take_messages(), 1);
        assert_eq!(
            messages.last(),
            Some(&ServerMessage::DeathRecap(DeathRecapMsg::of(1, mob())))
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
        next.take(&offer, NOW).expect("take");
        assert_eq!(
            health_of(&mut next, 1).current(),
            TEST_PLAYER_HEALTH - SWORD_HIT
        );
    }
}
