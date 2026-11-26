// SPDX-License-Identifier: AGPL-3.0-or-later
//! The projectiles mobs fire. A `CAGE-RangedAttack` clip of the action a mob plays
//! ([`crate::mob_ai::RangedClip`]) fires the attack its `RangedAttackName` names in the ranged
//! profile of the mob's weapon (main hand, else mouth slot): the ranged attack database gives
//! its range and radius, the clip's projectile slice its flight. The mob aims at its target,
//! ahead of it when the clip leads (`AILeadTarget`, by at most `AILeadTargetMaxAngle`). The shot
//! lands where it aimed once it flew there, and hits the target when the target's body is then
//! within the projectile's radius of that point.
//!
//! Observed: retail Janszoon threw his anchor (`Officer_AnchorToss`, newchar-retail 1246273 ms,
//! the player 8.7 m away) and the player lost 40 health 1.53 s later, his table's `Anchor_Toss`
//! row (0.7 of the base 58 his axe hits show: `Damned_2H_GreatAxe_Melee_Light` 0.5 for 29,
//! `_Heavy` 1.2 for 69); the clip fires 1.27 s into the throw and the anchor slice flies at
//! 30 m/s. Inferred: the landing test on the body (the client's projectile collision is not
//! read), and a flight time of the slice's speed along the ground (its arc is not drawn).
//!
//! **Entity.** Every client sees a mob's projectile by its entity, as a player's
//! ([`super::shot_entity`]): when the clip fires, the server spawns a card of the clip's slice
//! with its ownership (when the slice has one) and its projectile state (the spawn point, the
//! velocity, the gravity of an arcing attack, the mob as its owner, the attack's index in the
//! ranged attack database, the CRC of the mob's damage table and the clip row's index in it),
//! each on the slice's own component. Observed (newchar-retail-2026-09-18): a grenadier's
//! `Drowned_Miner_Throw` at 800.471 s spawned `corrupted_miner_bomb` (`9bfbf59a…`) at 802.637 s,
//! the clip's 2.167 s, with ownership on 1 and the projectile on 2 (attack 198, table
//! `FTUEDamageTableMiner`, row 0, gravity 15); the first mate's anchor throw spawned
//! `corrupted_firstmate_anchor_projectile` at 1247.540 s, its projectile on 1 (attack 204, no
//! gravity). Before, the sim spawned none: a grenadier lit its bomb and threw nothing (live
//! 2026-11-26 01:50, 17 of 17 throws).
//!
//! **Arc.** An attack with arc settings ([`nw_ai::data::ArcTrajectory`]) is lobbed: it rises its
//! lob height under its gravity, both taken at the target's distance, and comes down on the
//! aim point; the flight is that throw's time. Observed: 9 of 12 retail grenadier bombs rose at
//! 3.46 m/s under a gravity of 15 (the 0.4 m lob of `Grenadier_Bomb` beyond 4 m), each one's
//! level speed its own; a bomb from 7.3 m and from 11.4 m hurt 1.16 s and 1.29 s after its
//! card (`12a_msq_15` 167.864 s, newchar-retail 802.637 s), where the sim's straight 20 m/s
//! flew 0.44 s. Inferred: a target above the thrower is cleared by the lob over its height (3
//! retail bombs rose faster, toward targets whose height is not known); an arc of no lob height
//! flies straight at the slice's speed (Observed: one retail bomb at 20 m/s down, with the
//! gravity of 10 of the near end).
//!
//! **Impact.** A slice that spawns a slice with a damage area where it hits
//! ([`nw_ai::data::ProjectileImpact`]) explodes where it lands, hit or miss: the impact's entity
//! (card, ownership, position and its delayed events) spawns there and its row hits each player
//! within its radius once. A shot of a row of coefficient 0 lands no hit of its own. Observed:
//! the grenadier bomb's own row `Drowned_Miner_Bomb` has coefficient 0 (live 2026-11-26: 17
//! hits of 0); retail sent its explosion's card (`corrupted_miner_bomb_explosion`) 36 to 58 ms
//! after the one hit the player took of `Drowned_Miner_Bomb_Explosion` (coefficient 1: 295.86,
//! a fifth of it 60.3 for the grenadier's torch swing, `12a_msq_22` 135.678 s), and 5 misses
//! each spawned an explosion. Inferred: a player the shot's own hit landed on takes no impact
//! hit (retail's anchor flew on past the player it hit and landed 5 m away, newchar-retail
//! 1247.990 s; the sim's shot ends at the player); the impact entity stays until its last
//! delayed event ends (the slice's `destroy`); an impact's hit needs its mob alive.

use nw_ai::data::{DelayedEvent, ProjectileEntity, ProjectileImpact, RangedAttackStream};
use nw_content::player_attacks::Flight;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::facet::facet_entity;
use nw_messages::projectile_state::ProjectileState;
use nw_messages::{
    BundleMember, DelayedEventState, MemberState, PositionInTheWorldReplicatedState, WorldPosition,
};
use nw_sim_core::Position;

use super::Body;
use super::Player;
use super::SimWorld;
use super::attack::MobHitStyle;
use super::crafting::unowned;
use super::hit_shape::{PLAYER_CAPSULE, turn};
use super::mob_timeline::yaw_toward;
use super::projectile::{AimLine, muzzle_of};
use super::shot_entity::{AFTER_END_MS, ProjectileSpawn, velocity};
use super::spell_areas::{PERMISSION_RULE, yaw_rotation};
use crate::combat::AttackOutcome;
use crate::mob_ai::{RangedClip, Strike};
use crate::mobs::{MobAttackIntent, MobReach};

/// Milliseconds per second.
const MS_PER_SECOND: f32 = 1_000.0;
/// The 2 of a throw's rise, `v² = 2 g h`.
const TWICE: f32 = 2.0;
/// An area hit reaches by its radius, not by a swing.
const AREA_REACH: MobReach = MobReach {
    lunge_max_m: 0.0,
    lunge_turn_rad: 0.0,
    reach_m: 0.0,
};

/// A projectile a mob fired, on its way to where it aimed.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct MobShot {
    mob: u64,
    target: u64,
    /// The hit it lands.
    amount: u32,
    style: MobHitStyle,
    /// Where it lands.
    aim: Position,
    /// `m_projectileRadius`.
    radius_m: f32,
    /// When it lands.
    land_ms: u64,
    /// The slot key of its projectile entity, when it has one.
    entity: Option<u64>,
    /// What it spawns and hits with where it lands, and the yaw it flew along.
    impact: Option<(ProjectileImpact, f32)>,
}

/// How a lobbed throw flies ([`lob`]).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Lob {
    /// Its speed along the ground, m/s.
    level_mps: f32,
    /// Its speed up when it leaves, m/s.
    up_mps: f32,
    /// How long it flies, s.
    flight_s: f32,
}

/// The throw from `from` to `to` that rises `lob_m` over the higher of the two under
/// `gravity_mps2` (module doc **Arc**); `None` for no lob height or no gravity.
fn lob(from: Position, to: Position, lob_m: f32, gravity_mps2: f32) -> Option<Lob> {
    if lob_m <= 0.0 || gravity_mps2 <= 0.0 {
        return None;
    }
    let rise_m = to.z - from.z;
    let up_mps = (TWICE * gravity_mps2 * (lob_m + rise_m.max(0.0))).sqrt();
    let flight_s =
        (up_mps + (up_mps * up_mps - TWICE * gravity_mps2 * rise_m).sqrt()) / gravity_mps2;
    Some(Lob {
        level_mps: from.horizontal_distance(to) / flight_s,
        up_mps,
        flight_s,
    })
}

/// The flight of a projectile a clip names no slice for: straight at the clip's
/// `ProjectileSpeed`, the speed its aim leads with, until it passes the attack's range. Inferred:
/// the ranged attack database's own flight fields are not named (`AI_Risen_Vomit_Ball` `Basic`:
/// not a hit scan, range 100 m, radius 0.3 m, and unnamed speeds of 100). Without it the risen
/// regurgitator's `AVOMIT` and `Attack_Throw` clips (`ProjectileSpeed` 15 and 30, no
/// `SliceOverride`) fired nothing: "mob projectile flight missing" (live 2026-11-12 13:24:26).
fn straight_flight(speed_mps: f32, range_m: f32) -> Flight {
    Flight {
        base_speed_mps: speed_mps,
        extra_speed_mps: 0.0,
        time_for_max_speed_s: 0.0,
        gravity_mps2: 0.0,
        lifetime_s: range_m / speed_mps,
    }
}

/// The point a shot from `from` aims at for a target at `target` that moves at `velocity` (m/s
/// on the ground) and that the shot reaches after `flight_s`: where the target will be, turned
/// back toward the target by what passes `max_turn_rad`.
fn lead(
    from: Position,
    target: Position,
    velocity: [f32; 2],
    flight_s: f32,
    max_turn_rad: f32,
) -> Position {
    let ahead = Position {
        x: target.x + velocity[0] * flight_s,
        y: target.y + velocity[1] * flight_s,
        z: target.z,
    };
    let direct = yaw_toward(from, target);
    let led = turn(direct, yaw_toward(from, ahead));
    if led.abs() <= max_turn_rad {
        return ahead;
    }
    let yaw = direct + led.clamp(-max_turn_rad, max_turn_rad);
    let distance_m = from.horizontal_distance(ahead);
    // Yaw 0 faces +y; yaw grows toward -x (see `yaw_toward`).
    Position {
        x: from.x - yaw.sin() * distance_m,
        y: from.y + yaw.cos() * distance_m,
        z: target.z,
    }
}

/// A lifetime of `seconds` in whole milliseconds.
fn whole_ms(seconds: f32) -> u64 {
    // A flight or a lifetime of at most a few seconds.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a flight or a lifetime of a few seconds"
    )]
    let ms = (seconds.max(0.0) * MS_PER_SECOND).round() as u64;
    ms
}

/// The ownership of a mob's projectile or impact on `component`: no one's, as retail's
/// (`RETAIL_OWNERSHIP` of the tests).
fn ownership_member(component: u64) -> BundleMember {
    BundleMember {
        component,
        state: MemberState::Ownership(unowned(PERMISSION_RULE, "")),
    }
}

/// The states of the impact entity of `impact` at `at`, turned to `yaw`: its ownership, its
/// position and its delayed events, not yet run (Observed: `corrupted_miner_bomb_explosion`'s
/// delayed event `01 03 4500 00`, 5 s not completed, newchar-retail-2026-09-18 803.988 s).
/// Inferred: the entity is turned along the shot (retail's turn is not decoded).
fn impact_members(impact: &ProjectileImpact, at: WorldPosition, yaw: f32) -> Vec<BundleMember> {
    let ownership = impact.ownership_component.map(ownership_member);
    let position = impact.position_component.map(|component| BundleMember {
        component,
        state: MemberState::Position(PositionInTheWorldReplicatedState {
            position: Some(at),
            rotation: Some(yaw_rotation(yaw)),
            scale: None,
        }),
    });
    let events = impact.delayed_events.iter().map(|event| BundleMember {
        component: event.component,
        state: MemberState::DelayedEvent(DelayedEventState {
            total_delay_s: Some(event.delay_s),
            completed: Some(false),
        }),
    });
    ownership
        .into_iter()
        .chain(position)
        .chain(events)
        .collect()
}

/// How long the impact entity of `impact` stays: until its last delayed event ends.
fn impact_lasting_ms(impact: &ProjectileImpact) -> u64 {
    impact
        .delayed_events
        .iter()
        .map(|event: &DelayedEvent| whole_ms(event.delay_s))
        .max()
        // Inferred: one without a delayed event stays as a shot entity after its end.
        .unwrap_or(AFTER_END_MS)
}

/// A shot's way: where it leaves, how it leaves (its velocity and the gravity of an arcing
/// attack), how long it flies to where it lands, and how long its flight may last
/// (`m_flyingLifetime`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct ShotWay {
    muzzle: Position,
    velocity: [f32; 3],
    gravity_mps2: Option<f32>,
    flight_s: f32,
    lifetime_s: f32,
}

impl SimWorld {
    /// The mob of `intent` at `from` fires the projectile of `clip` at its target at `now_ms`,
    /// to land `amount` in `style` ([`SimWorld::mob_shots_due`]); its slice's entity shows it
    /// (module doc **Entity**). A shot whose attack, profile or flight the data does not have is
    /// not fired; a target beyond its range is missed.
    pub(super) fn fire_mob_shot(
        &mut self,
        intent: &MobAttackIntent,
        clip: &RangedClip,
        amount: u32,
        style: MobHitStyle,
        from: Position,
        now_ms: u64,
    ) {
        let data = self.mobs.ai().data();
        let weapon = intent
            .strike
            .as_ref()
            .and_then(|strike| strike.weapon.as_deref());
        let profile = weapon.and_then(|weapon| self.combat.data.ranged_profile_of_item(weapon));
        let attack = profile
            .and_then(|profile| data.ranged_attack(profile, &clip.attack))
            .copied();
        let Some(attack) = attack else {
            tracing::warn!(
                mob = intent.mob,
                attack = clip.attack,
                reason = "no ranged attack",
                ?weapon,
                ?clip,
                "mob attack not started"
            );
            return;
        };
        let stream = profile
            .and_then(|profile| data.ranged_attack_stream(profile, &clip.attack))
            .copied();
        let entity = clip
            .slice
            .as_deref()
            .and_then(|slice| data.projectile_entity(slice))
            .cloned();
        let flight = if attack.hit_scan {
            None
        } else {
            let flight = match clip.slice.as_deref() {
                Some(slice) => data.projectile_flight(slice),
                None => clip
                    .speed_mps
                    .map(|speed_mps| straight_flight(speed_mps, attack.range_m)),
            };
            if flight.is_none() {
                tracing::warn!(
                    mob = intent.mob,
                    attack = clip.attack,
                    reason = "no flight",
                    ?clip,
                    "mob attack not started"
                );
                return;
            }
            flight
        };
        let Some((target_at, velocity)) = self.shot_target(intent.target) else {
            return;
        };
        let flight_to =
            |distance_m: f32| flight.map_or(Some(0.0), |flight| flight.time_to(distance_m));
        let aim = match flight_to(from.horizontal_distance(target_at)) {
            Some(flight_s) if clip.lead => {
                lead(from, target_at, velocity, flight_s, clip.lead_max_rad)
            }
            _ => target_at,
        };
        let distance_m = from.horizontal_distance(aim);
        let flight_s = flight_to(distance_m).filter(|_| distance_m <= attack.range_m);
        let Some(flight_s) = flight_s else {
            tracing::debug!(
                mob = intent.mob,
                attack = clip.attack,
                reason = "out of range",
                distance = distance_m,
                "mob attack not started"
            );
            return;
        };
        let way = Self::shot_way(from, aim, flight, stream.as_ref(), flight_s);
        let flight_ms = whole_ms(way.flight_s);
        tracing::debug!(
            mob = intent.mob,
            attack = clip.attack,
            distance = distance_m,
            flight_ms,
            "mob shot"
        );
        // A hit scan flies no projectile.
        let spawned = match (&entity, &stream, flight) {
            (Some(entity), Some(stream), Some(_)) => {
                self.spawn_mob_shot_entity(intent, entity, stream, way, now_ms)
            }
            _ => None,
        };
        self.mob_shots.push(MobShot {
            mob: intent.mob,
            target: intent.target,
            amount,
            style: style.fired(),
            aim,
            radius_m: attack.radius_m,
            land_ms: now_ms.saturating_add(flight_ms),
            entity: spawned,
            impact: entity
                .and_then(|entity| entity.impact)
                .map(|impact| (impact, yaw_toward(way.muzzle, aim))),
        });
    }

    /// The way of a shot of `flight` (`None`: a hit scan) from a mob at `from` to `aim`, of the
    /// attack placed by `stream`, which flies `flight_s` straight: lobbed when the attack arcs
    /// (module doc **Arc**), else straight along the line from the middle of the mob's body
    /// at the flight's speed.
    fn shot_way(
        from: Position,
        aim: Position,
        flight: Option<Flight>,
        stream: Option<&RangedAttackStream>,
        flight_s: f32,
    ) -> ShotWay {
        let muzzle = muzzle_of(from);
        let arc = stream
            .and_then(|stream| stream.arc)
            .map(|arc| arc.at(muzzle.horizontal_distance(aim)));
        let lobbed = arc.and_then(|(lob_m, gravity_mps2)| lob(muzzle, aim, lob_m, gravity_mps2));
        let (velocity, flight_s) = match lobbed {
            Some(lob) => {
                let line = AimLine {
                    yaw: yaw_toward(muzzle, aim),
                    pitch: -lob.up_mps.atan2(lob.level_mps),
                };
                (
                    velocity(line, lob.level_mps.hypot(lob.up_mps)),
                    lob.flight_s,
                )
            }
            None => {
                let speed_mps = flight.map_or(0.0, |flight| flight.base_speed_mps);
                let line = AimLine::of(from, Some(aim), None, None);
                (
                    line.map_or([0.0; 3], |line| velocity(line, speed_mps)),
                    flight_s,
                )
            }
        };
        ShotWay {
            muzzle,
            velocity,
            gravity_mps2: arc.map(|(_, gravity_mps2)| gravity_mps2),
            flight_s,
            lifetime_s: flight.map_or(flight_s, |flight| flight.lifetime_s),
        }
    }

    /// Spawns the projectile entity of `entity`'s slice that the mob of `intent` fires along
    /// `way` at `now_ms` with the attack placed by `stream`; gives its slot key. `None` when the
    /// data has no index of the clip's row in the mob's damage table.
    fn spawn_mob_shot_entity(
        &mut self,
        intent: &MobAttackIntent,
        entity: &ProjectileEntity,
        stream: &RangedAttackStream,
        way: ShotWay,
        now_ms: u64,
    ) -> Option<u64> {
        let strike = intent.strike.as_ref()?;
        let data = self.mobs.ai().data();
        let Some(row) = data.damage_row_index(&strike.table, &strike.row) else {
            tracing::warn!(
                mob = intent.mob,
                table = strike.table,
                row = strike.row,
                "mob shot entity not spawned: row not in its table"
            );
            return None;
        };
        let state = ProjectileState {
            velocity: Some(way.velocity),
            gravity_override: way.gravity_mps2,
            owner: Some(facet_entity(intent.mob)),
            ranged_attack: Some(stream.index),
            damage_table: Some(lowercase_crc32(&strike.table)),
            damage_row: Some(row),
            ..ProjectileState::default()
        };
        let spawn = ProjectileSpawn {
            asset: entity.asset,
            component: entity.projectile_component,
            position_component: None,
            state,
            muzzle: way.muzzle,
            flying_ms: whole_ms(way.lifetime_s),
            stuck_ms: whole_ms(entity.stuck_lifetime_s),
        };
        let ownership = entity.ownership_component.map(ownership_member);
        self.spawn_projectile_with(spawn, ownership.into_iter().collect(), now_ms)
            .map(|(number, _)| number)
    }

    /// Lands the mob shots that got where they aimed at `now_ms`: each hits its target when the
    /// target's body is within its radius of that point, and one with an impact explodes there
    /// (module doc **Impact**).
    pub(super) fn mob_shots_due(&mut self, now_ms: u64) -> Vec<AttackOutcome> {
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.mob_shots)
            .into_iter()
            .partition(|shot| now_ms >= shot.land_ms);
        self.mob_shots = later;
        let mut outcomes = Vec::new();
        for shot in due {
            let met = self.shot_meets(&shot);
            if let Some(entity) = shot.entity {
                match met {
                    Some(body) => {
                        self.shot_entity_hit(entity, shot.target, muzzle_of(body), now_ms);
                    }
                    None => self.shot_entity_impact(entity, None, shot.aim, now_ms),
                }
            }
            let own_hit = met
                .filter(|_| shot.impact.is_none() || !shot.style.is_harmless())
                .and_then(|_| {
                    self.land_mob_hit(shot.target, shot.mob, shot.amount, shot.style, now_ms)
                });
            let spared = own_hit
                .as_ref()
                .filter(|outcome| matches!(outcome, AttackOutcome::Hit { .. }))
                .map(|_| shot.target);
            outcomes.extend(own_hit);
            if let Some((impact, yaw)) = &shot.impact {
                outcomes.extend(self.land_impact(&shot, (impact, *yaw), spared, now_ms));
            }
        }
        outcomes
    }

    /// Where the target of `shot` stands when its body is within the shot's radius of where it
    /// landed and level with it; `None` when the shot missed it.
    fn shot_meets(&mut self, shot: &MobShot) -> Option<Position> {
        let (body, _) = self.shot_target(shot.target)?;
        let wide_m = shot.radius_m + PLAYER_CAPSULE.radius_m;
        let level =
            (shot.aim.z - shot.radius_m..=shot.aim.z + PLAYER_CAPSULE.height_m).contains(&body.z);
        if body.horizontal_distance(shot.aim) > wide_m || !level {
            tracing::debug!(
                mob = shot.mob,
                aside = body.horizontal_distance(shot.aim),
                dz = body.z - shot.aim.z,
                "mob shot missed"
            );
            return None;
        }
        Some(body)
    }

    /// The impact of `shot` where it landed, turned to `yaw`, at `now_ms`: its entity spawns
    /// and its row hits each player within its radius but `spared`.
    fn land_impact(
        &mut self,
        shot: &MobShot,
        (impact, yaw): (&ProjectileImpact, f32),
        spared: Option<u64>,
        now_ms: u64,
    ) -> Vec<AttackOutcome> {
        if let Some(at) = WorldPosition::from_metres(shot.aim.x, shot.aim.y, shot.aim.z) {
            let members = impact_members(impact, at, yaw);
            self.spawn_impact_entity(
                impact.asset,
                members,
                shot.aim,
                (impact_lasting_ms(impact), now_ms),
            );
        }
        let data = self.mobs.ai().data();
        let table = data.name(impact.table).to_owned();
        let row = data.name(impact.row).to_owned();
        let inside: Vec<u64> = self
            .players_within(shot.aim, impact.radius_m)
            .into_iter()
            .filter_map(|entity| self.world.get::<Player>(entity).map(|player| player.number))
            .filter(|number| Some(*number) != spared)
            .collect();
        inside
            .into_iter()
            .filter_map(|target| {
                self.mob_area_hit(shot.mob, (target, shot.aim), (&table, &row), now_ms)
            })
            .collect()
    }

    /// The hit of the damage row `(table, row)` of mob `mob` on player `target`, from an area
    /// at `at`: a hit of the mob, which the player's block and dodge take as any other.
    pub(super) fn mob_area_hit(
        &mut self,
        mob: u64,
        (target, at): (u64, Position),
        (table, row): (&str, &str),
        now_ms: u64,
    ) -> Option<AttackOutcome> {
        let intent = MobAttackIntent {
            mob,
            target,
            target_at: at,
            mob_move: None,
            strike: Some(Strike {
                table: table.to_owned(),
                row: row.to_owned(),
                weapon: None,
                reach: AREA_REACH,
                ranged: None,
                open_until_ms: None,
            }),
        };
        let defender = self.player_by_number(target);
        let (amount, style, _) = self.mob_intent_hit(&intent, None, defender)?;
        self.land_mob_hit(target, mob, amount, style, now_ms)
    }

    /// Where the player `target` is and how it moves (m/s on the ground): a player of this sim
    /// by its body, a player ghost by its last place, standing.
    fn shot_target(&mut self, target: u64) -> Option<(Position, [f32; 2])> {
        if let Some(entity) = self.player_by_number(target) {
            let motion = self.world.get::<Body>(entity)?.motion;
            return Some((motion.position(), motion.velocity()));
        }
        let (_, ghost) = self.ghost(target)?;
        ghost.mob.is_none().then_some((ghost.position, [0.0; 2]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FROM: Position = Position {
        x: 100.0,
        y: 100.0,
        z: 20.0,
    };
    const TOLERANCE: f32 = 1e-4;
    /// A target 8 m north of [`FROM`].
    const TARGET: Position = Position { y: 108.0, ..FROM };
    const FLIGHT_S: f32 = 0.5;

    #[test]
    fn a_lead_aims_where_the_target_will_be_within_its_angle() {
        // Running east at 4 m/s: 2 m ahead in the flight, 14 degrees off the direct line.
        let east = [4.0, 0.0];
        let wide = lead(FROM, TARGET, east, FLIGHT_S, 20_f32.to_radians());
        assert!((wide.x - 102.0).abs() < TOLERANCE && (wide.y - 108.0).abs() < TOLERANCE);
        // A 10 degree limit turns the aim back to 10 degrees east of the line, as far out.
        let narrow = lead(FROM, TARGET, east, FLIGHT_S, 10_f32.to_radians());
        let distance = FROM.horizontal_distance(wide);
        assert!((FROM.horizontal_distance(narrow) - distance).abs() < TOLERANCE);
        let off = turn(yaw_toward(FROM, TARGET), yaw_toward(FROM, narrow));
        assert!((off.abs() - 10_f32.to_radians()).abs() < TOLERANCE);
        assert!(narrow.x > FROM.x, "east of the line");
        assert_eq!(
            lead(FROM, TARGET, [0.0; 2], FLIGHT_S, 0.0),
            TARGET,
            "a still target"
        );
    }

    use nw_sim_core::Motion;

    use crate::combat::test_support::TEST_RANGED_ITEM;
    use crate::mob_ai::Strike;
    use crate::mob_ai::test_support::{
        TEST_MOB_ROW, TEST_MOB_TABLE, TEST_RANGED_ATTACK, TEST_SHOT_RANGE_M, TEST_SHOT_SLICE,
        TEST_SHOT_SPEED_MPS,
    };
    use crate::mobs::{MobReach, mob_entity};
    use crate::world::Vitals;
    use crate::world::tests::{NOW, join, table, test_spawn, world_with_spawns};

    const MOB_X: f32 = 1024.0;
    /// The player stands this far east of the mob.
    const SHOT_M: f32 = 6.0;

    /// A world with one mob and player 1 [`SHOT_M`] east of it.
    fn range() -> SimWorld {
        let mut world = world_with_spawns(vec![test_spawn(MOB_X)]);
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, MOB_X + SHOT_M);
        world
    }

    /// The test mob's ranged clip at player 1, from the weapon item `weapon`, with the
    /// projectile slice `slice`.
    fn shot_from(weapon: Option<&str>, slice: Option<&str>) -> MobAttackIntent {
        MobAttackIntent {
            mob: mob_entity(0),
            target: 1,
            target_at: Position {
                x: MOB_X + SHOT_M,
                y: 10.0,
                z: 50.0,
            },
            mob_move: None,
            strike: Some(Strike {
                table: TEST_MOB_TABLE.to_owned(),
                row: TEST_MOB_ROW.to_owned(),
                weapon: weapon.map(str::to_owned),
                reach: MobReach {
                    lunge_max_m: 0.0,
                    lunge_turn_rad: 0.0,
                    reach_m: 0.0,
                },
                ranged: Some(RangedClip {
                    attack: TEST_RANGED_ATTACK.to_owned(),
                    slice: slice.map(str::to_owned),
                    lead: false,
                    lead_max_rad: 0.0,
                    speed_mps: Some(TEST_SHOT_SPEED_MPS),
                }),
                open_until_ms: None,
            }),
        }
    }

    /// The test mob's ranged clip with its projectile slice at player 1, from `weapon`.
    fn shot_at(weapon: Option<&str>) -> MobAttackIntent {
        shot_from(weapon, Some(TEST_SHOT_SLICE))
    }

    fn health_of(world: &mut SimWorld) -> u32 {
        let entity = world.player_by_number(1).expect("player");
        world
            .world
            .get::<Vitals>(entity)
            .expect("vitals")
            .health
            .current()
    }

    /// Puts player 1 at `x` on the mob's line.
    fn move_player(world: &mut SimWorld, x: f32, y: f32) {
        let entity = world.player_by_number(1).expect("player");
        let limits = world.settings.limits;
        world.world.get_mut::<Body>(entity).expect("body").motion =
            Motion::new(Position { x, y, z: 50.0 }, NOW, &limits);
    }

    #[test]
    fn a_mob_shot_flies_and_hits_the_player_where_it_aimed() {
        let mut world = range();
        let full = health_of(&mut world);
        assert!(
            world
                .mob_hits(vec![(shot_at(Some(TEST_RANGED_ITEM)), None)], NOW)
                .is_empty(),
            "on its way"
        );
        // 6 m at 20 m/s.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "300 ms"
        )]
        let flight_ms = (SHOT_M / TEST_SHOT_SPEED_MPS * MS_PER_SECOND).round() as u64;
        assert!(world.mob_shots_due(NOW + flight_ms - 1).is_empty());
        assert_eq!(world.mob_shots_due(NOW + flight_ms).len(), 1);
        assert!(health_of(&mut world) < full);
        assert!(world.mob_shots.is_empty());
    }

    /// A clip without a projectile slice flies at its own `ProjectileSpeed` (the risen
    /// regurgitator's `AVOMIT`, live 2026-11-12: "mob projectile flight missing" and no shot).
    #[test]
    fn a_shot_without_a_slice_flies_at_its_clip_speed() {
        let mut world = range();
        let full = health_of(&mut world);
        world.mob_hits(vec![(shot_from(Some(TEST_RANGED_ITEM), None), None)], NOW);
        assert_eq!(world.mob_shots.len(), 1, "fired");
        // 6 m at 20 m/s, to the millisecond of the flight's time search.
        const FLIGHT_MS: u64 = 300;
        let land_ms = world.mob_shots[0].land_ms;
        assert!(land_ms.abs_diff(NOW + FLIGHT_MS) <= 1, "{land_ms}");
        assert!(world.mob_shots_due(land_ms - 1).is_empty());
        assert_eq!(world.mob_shots_due(land_ms).len(), 1);
        assert!(health_of(&mut world) < full);
        let flight = straight_flight(TEST_SHOT_SPEED_MPS, TEST_SHOT_RANGE_M);
        assert_eq!(
            flight.time_to(TEST_SHOT_RANGE_M + 1.0),
            None,
            "gone past its range"
        );
    }

    #[test]
    fn a_mob_shot_misses_a_player_who_stepped_aside() {
        let mut world = range();
        let full = health_of(&mut world);
        world.mob_hits(vec![(shot_at(Some(TEST_RANGED_ITEM)), None)], NOW);
        // 1 m aside: beyond the 0.5 m projectile and the body's radius.
        move_player(&mut world, MOB_X + SHOT_M, 11.0);
        assert!(world.mob_shots_due(NOW + 1_000).is_empty());
        assert_eq!(health_of(&mut world), full);
    }

    #[test]
    fn a_mob_fires_no_shot_beyond_its_range_or_without_its_profile() {
        let mut world = range();
        move_player(&mut world, MOB_X + TEST_SHOT_RANGE_M + 1.0, 10.0);
        world.mob_hits(vec![(shot_at(Some(TEST_RANGED_ITEM)), None)], NOW);
        assert!(world.mob_shots.is_empty(), "out of range");
        move_player(&mut world, MOB_X + SHOT_M, 10.0);
        world.mob_hits(vec![(shot_at(None), None)], NOW);
        assert!(world.mob_shots.is_empty(), "no weapon, no ranged profile");
    }

    use std::sync::Arc;

    use nw_ai::data::{AiData, ArcTrajectory, DamageTableRows};
    use nw_content::combat::{AttackRow, CombatData, CombatRows, DamageCategory, RuneChargeRow};

    use crate::combat::test_support::{TEST_MOB, test_player_attacks, test_source};
    use crate::config::tests::committed;
    use crate::mob_ai::test_support::{TEST_RANGED_PROFILE, test_ai_data, test_ai_with};
    use crate::world::tests::{MANY, MAX_PLAYERS, settings, world_with_combat};

    /// The grenadier's bomb row (coefficient 0) and its explosion's (coefficient 1), made-up
    /// names in the test table.
    const BOMB_ROW: &str = "Test_Bomb";
    const EXPLOSION_ROW: &str = "Test_Bomb_Explosion";
    /// `corrupted_miner_bomb.dynamicslice` and its explosion (asset GUIDs of build 25810808).
    const BOMB_ASSET: [u8; 16] = [
        0x9b, 0xfb, 0xf5, 0x9a, 0x8d, 0xd1, 0x5f, 0x24, 0x80, 0x48, 0xcf, 0x4a, 0x38, 0x57, 0x95,
        0x6f,
    ];
    const EXPLOSION_ASSET: [u8; 16] = [
        0xbc, 0xc4, 0x47, 0x3a, 0x21, 0x96, 0x59, 0x2c, 0xab, 0x4e, 0x4b, 0xb5, 0x98, 0x62, 0xbe,
        0x68,
    ];
    /// `corrupted_firstmate_anchor_projectile.dynamicslice`.
    const ANCHOR_ASSET: [u8; 16] = [
        0x89, 0x7f, 0xce, 0xa1, 0xee, 0x53, 0x59, 0x07, 0xb2, 0x8d, 0xaf, 0x77, 0x9e, 0xf9, 0xed,
        0xc7,
    ];
    /// `AI_Undead/Grenadier_Bomb`'s place in the ranged attack database.
    const GRENADIER_BOMB_INDEX: u64 = 198;
    /// `AI_Undead/Grenadier_Bomb`'s arc settings.
    const GRENADIER_ARC: ArcTrajectory = ArcTrajectory {
        lob_height_m: [0.0, 0.4],
        gravity_mps2: [10.0, 15.0],
        distance_m: [3.0, 4.0],
    };
    /// The explosion's radius and its delayed event.
    const EXPLOSION_RADIUS_M: f32 = 2.0;
    const EXPLOSION_DELAY_S: f32 = 5.0;
    /// The ownership member of the retail bomb and its explosion (component 1, class 3217),
    /// and the explosion's delayed event (component 3, class 3139), newchar-retail-2026-09-18
    /// 802.637 s and 803.988 s.
    const RETAIL_OWNERSHIP: &str = "01913201ff000000000000000000000000000000000000003f8000003f8000003f8000003f80000000003f8000003f8000003f8000003f800000000002010000000000000000000f00000000";
    const RETAIL_DELAYED_EVENT: &str = "0383310103450000";

    fn row(damage_id: &str, coefficient: f64) -> AttackRow {
        AttackRow {
            damage_id: damage_id.to_owned(),
            coefficient,
            can_crit: false,
            threat: 0,
            damage_type: None,
            table: TEST_MOB_TABLE.to_owned(),
            block_stamina_mod: 0.0,
            blockable: true,
            category: DamageCategory::Physical,
            affix: None,
            backstab: false,
            headshot: false,
            rune_charge: RuneChargeRow::default(),
            power_level: 0,
            hit_stun: 0,
            creature_hit_stun: 0,
            stagger_mod: 0.0,
            stun_breakout: 0.0,
        }
    }

    /// The test AI data whose shot slice is `entity` (its names interned), whose test attack
    /// arcs by `arc`, and whose test table holds the bomb and explosion rows.
    fn data_with(
        entity: impl FnOnce(&mut AiData) -> ProjectileEntity,
        arc: Option<ArcTrajectory>,
    ) -> AiData {
        let mut data = test_ai_data();
        let entity = entity(&mut data);
        data.projectile_entities = vec![entity];
        data.ranged_attack_streams = vec![RangedAttackStream {
            profile: data.intern(TEST_RANGED_PROFILE),
            attack: data.intern(TEST_RANGED_ATTACK),
            index: GRENADIER_BOMB_INDEX,
            arc,
        }];
        data.damage_table_rows = vec![DamageTableRows {
            table: data.intern(TEST_MOB_TABLE),
            rows: vec![
                data.intern(BOMB_ROW),
                data.intern(EXPLOSION_ROW),
                data.intern(TEST_MOB_ROW),
            ],
        }];
        data
    }

    /// The grenadier bomb: ownership on 1, the projectile on 2, its explosion.
    fn bomb(data: &mut AiData) -> ProjectileEntity {
        ProjectileEntity {
            slice: data.intern(TEST_SHOT_SLICE),
            asset: BOMB_ASSET,
            projectile_component: 2,
            stuck_lifetime_s: 0.7,
            ownership_component: Some(1),
            impact: Some(ProjectileImpact {
                asset: EXPLOSION_ASSET,
                table: data.intern(TEST_MOB_TABLE),
                row: data.intern(EXPLOSION_ROW),
                radius_m: EXPLOSION_RADIUS_M,
                ownership_component: Some(1),
                position_component: Some(2),
                delayed_events: vec![DelayedEvent {
                    component: 3,
                    delay_s: EXPLOSION_DELAY_S,
                }],
            }),
        }
    }

    /// The first mate's anchor: the projectile on 1, no ownership, no impact hit here.
    fn anchor(data: &mut AiData) -> ProjectileEntity {
        ProjectileEntity {
            slice: data.intern(TEST_SHOT_SLICE),
            asset: ANCHOR_ASSET,
            projectile_component: 1,
            stuck_lifetime_s: 2.5,
            ownership_component: None,
            impact: None,
        }
    }

    /// A world of [`range`]'s place whose mobs think with `data` and whose test table holds the
    /// bomb (0) and explosion (1) rows.
    fn range_with(data: AiData) -> SimWorld {
        let mut source = test_source();
        source
            .attacks
            .extend([row(BOMB_ROW, 0.0), row(EXPLOSION_ROW, 1.0)]);
        let combat_data = CombatData::build(source, &CombatRows::default())
            .expect("combat data")
            .with_player_attacks(test_player_attacks());
        let ai = Arc::new(test_ai_with(data, &[TEST_MOB]));
        let mut combat = committed().combat_settings(Arc::new(combat_data), Arc::from([]), ai);
        combat.spawns = vec![test_spawn(MOB_X)].into();
        let mut world = world_with_combat(settings(MANY, MAX_PLAYERS), combat);
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, MOB_X + SHOT_M);
        world
    }

    /// The bomb's throw at player 1: the clip's row is the bomb's.
    fn bomb_throw() -> MobAttackIntent {
        let mut intent = shot_at(Some(TEST_RANGED_ITEM));
        if let Some(strike) = &mut intent.strike {
            strike.row = BOMB_ROW.to_owned();
        }
        intent
    }

    /// The members of the shot entity of slot key `number`, by component, encoded.
    fn members_of(world: &SimWorld, number: u64) -> Vec<(u64, Vec<u8>)> {
        world
            .stations
            .members(number)
            .expect("an entity")
            .iter()
            .map(|member| (member.component, member.encode().expect("encodes")))
            .collect()
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// The card asset of the entity of slot key `number`.
    fn card_asset(world: &SimWorld, number: u64) -> Option<[u8; 16]> {
        world
            .stations
            .members(number)?
            .iter()
            .find_map(|member| match &member.state {
                MemberState::GdeMetadata(card) => card.asset.map(|asset| asset.id),
                _ => None,
            })
    }

    #[test]
    fn a_lob_rises_its_height_and_comes_down_on_its_aim() {
        let from = Position { z: 50.0, ..FROM };
        // Level, 8 m: up 3.46 m/s (the retail grenadier bombs), 0.46 s.
        let level = lob(from, Position { y: 108.0, ..from }, 0.4, 15.0).expect("a lob");
        assert!((level.up_mps - 12_f32.sqrt()).abs() < TOLERANCE);
        assert!((level.flight_s - 2.0 * 12_f32.sqrt() / 15.0).abs() < TOLERANCE);
        assert!((level.level_mps * level.flight_s - 8.0).abs() < TOLERANCE);
        // 4.8 m down: the same rise, a longer fall.
        let down = lob(
            from,
            Position {
                y: 108.0,
                z: 45.2,
                ..from
            },
            0.4,
            15.0,
        )
        .expect("a lob");
        assert!((down.up_mps - level.up_mps).abs() < TOLERANCE);
        assert!((down.flight_s - (12_f32.sqrt() + 156_f32.sqrt()) / 15.0).abs() < TOLERANCE);
        assert!(
            lob(from, TARGET, 0.0, 10.0).is_none(),
            "no lob height: straight"
        );
    }

    /// The bomb lands no hit of its own row (coefficient 0); its explosion hits the player once
    /// where it stood, 1.6 m away, and not 3 m away.
    #[test]
    fn a_bomb_hurts_by_its_explosion_only_within_its_radius() {
        let explosion_hits = |aside_m: Option<f32>| {
            let mut world = range_with(data_with(bomb, Some(GRENADIER_ARC)));
            let full = health_of(&mut world);
            world.mob_hits(vec![(bomb_throw(), None)], NOW);
            let land_ms = world.mob_shots[0].land_ms;
            if let Some(aside_m) = aside_m {
                move_player(&mut world, MOB_X + SHOT_M, 10.0 + aside_m);
            }
            let outcomes = world.mob_shots_due(land_ms);
            let hurt = health_of(&mut world) < full;
            // A miss explodes too: its entity is there.
            let explosion = world
                .live_shots
                .iter()
                .any(|shot| card_asset(&world, shot.number) == Some(EXPLOSION_ASSET));
            assert!(explosion, "an explosion {aside_m:?}");
            (outcomes.len(), hurt)
        };
        assert_eq!(explosion_hits(None), (1, true), "one hit, the explosion's");
        assert_eq!(explosion_hits(Some(1.6)), (1, true), "within 2 m");
        assert_eq!(explosion_hits(Some(3.0)), (0, false), "beyond 2 m");
    }

    /// The bomb flies the lob's time, and its entity holds the card of its slice, the retail
    /// ownership on 1 and the projectile state on 2: the retail children (spawn point,
    /// velocity, gravity 15, owner, attack 198, table and row 0) with the test mob and table.
    #[test]
    fn a_thrown_bomb_spawns_its_slice_entity_with_the_retail_states() {
        let mut world = range_with(data_with(bomb, Some(GRENADIER_ARC)));
        world.mob_hits(vec![(bomb_throw(), None)], NOW);
        let shot = world.mob_shots[0].clone();
        let number = shot.entity.expect("an entity");
        assert_eq!(card_asset(&world, number), Some(BOMB_ASSET));
        let members = members_of(&world, number);
        assert_eq!(members[1], (1, hex_bytes(RETAIL_OWNERSHIP)));
        let (component, projectile) = &members[2];
        assert_eq!(*component, 2);
        let tail = format!(
            "4b8098{:016x}8603{:02x}{:08x}0000",
            facet_entity(mob_entity(0)),
            3,
            lowercase_crc32(TEST_MOB_TABLE)
        );
        assert!(
            hex(projectile).starts_with("0210018d"),
            "{}",
            hex(projectile)
        );
        assert!(hex(projectile).ends_with(&tail), "{}", hex(projectile));
        let MemberState::Projectile(state) =
            &world.stations.members(number).expect("members")[2].state
        else {
            panic!("a projectile state");
        };
        let up_mps = state.velocity.expect("a velocity")[2];
        assert!((up_mps - 12_f32.sqrt()).abs() < TOLERANCE, "{up_mps}");
        let lobbed = lob(
            muzzle_of(world.mobs.get(mob_entity(0)).expect("the mob").position),
            shot.aim,
            0.4,
            15.0,
        )
        .expect("a lob");
        assert!(shot.land_ms.abs_diff(NOW + whole_ms(lobbed.flight_s)) <= 1);
        world.mob_shots_due(shot.land_ms);
        let explosion = world
            .live_shots
            .iter()
            .find(|live| card_asset(&world, live.number) == Some(EXPLOSION_ASSET))
            .map(|live| live.number)
            .expect("the explosion");
        let members = members_of(&world, explosion);
        assert_eq!(members[1], (1, hex_bytes(RETAIL_OWNERSHIP)));
        assert_eq!(members[3], (3, hex_bytes(RETAIL_DELAYED_EVENT)));
    }

    /// The first mate's anchor shows by its entity too: its slice's card and its projectile
    /// state on 1, without a gravity; its own row hits as before.
    #[test]
    fn an_anchor_throw_spawns_its_entity_and_hits_by_its_row() {
        let mut world = range_with(data_with(anchor, None));
        let full = health_of(&mut world);
        world.mob_hits(vec![(shot_at(Some(TEST_RANGED_ITEM)), None)], NOW);
        let shot = world.mob_shots[0].clone();
        let number = shot.entity.expect("an entity");
        assert_eq!(card_asset(&world, number), Some(ANCHOR_ASSET));
        let members = members_of(&world, number);
        assert_eq!(members.len(), 2, "the card and the projectile");
        let (component, projectile) = &members[1];
        assert_eq!(*component, 1);
        assert!(
            hex(projectile).starts_with("01100185"),
            "{}",
            hex(projectile)
        );
        assert_eq!(world.mob_shots_due(shot.land_ms).len(), 1);
        assert!(health_of(&mut world) < full);
    }

    fn hex_bytes(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex"))
            .collect()
    }
}

/// The retail AI data's mob projectile entities.
#[cfg(all(test, feature = "local-data"))]
mod retail_tests {
    use nw_ai::data::AiData;

    fn retail_data() -> AiData {
        AiData::load(&nw_content::layout::repo_layout()).expect("run `content-tool ai-data` first")
    }

    /// The grenadier bomb explodes with `Drowned_Miner_Bomb_Explosion` in 2 m, ownership on 1,
    /// the position on 2 and a 5 s delayed event on 3; its projectile is on 2 with ownership on
    /// 1 (newchar-retail-2026-09-18 802.637 s and 803.988 s). The first mate's anchor is on 1
    /// with no ownership; `Grenadier_Bomb` is attack 198 and arcs, `Officer_Anchor` 204 and does
    /// not; `Drowned_Miner_Bomb` is row 0 of `FTUEDamageTableMiner`.
    #[test]
    fn the_retail_bomb_and_anchor_slices_name_their_entities_and_impacts() {
        let data = retail_data();
        let bomb = data
            .projectile_entity("slices/items/weapons/corrupted_miner_bomb.dynamicslice")
            .expect("the bomb");
        assert_eq!(
            (bomb.projectile_component, bomb.ownership_component),
            (2, Some(1))
        );
        let impact = bomb.impact.as_ref().expect("an explosion");
        assert_eq!(data.name(impact.row), "Drowned_Miner_Bomb_Explosion");
        assert_eq!(data.name(impact.table), "UndeadGrenadierDamageTable");
        assert!((impact.radius_m - 2.0).abs() < f32::EPSILON);
        assert_eq!(
            (impact.ownership_component, impact.position_component),
            (Some(1), Some(2))
        );
        assert_eq!(impact.delayed_events.len(), 1);
        assert_eq!(impact.delayed_events[0].component, 3);
        assert!((impact.delayed_events[0].delay_s - 5.0).abs() < f32::EPSILON);
        let drowned = data
            .projectile_entity("slices/items/weapons/drowned_miner_bomb.dynamicslice")
            .and_then(|entity| entity.impact.as_ref())
            .expect("the drowned bomb's explosion");
        assert_eq!(data.name(drowned.row), "Drowned_Miner_Bomb_Explosion");
        let anchor = data
            .projectile_entity(
                "slices/characters/spellslices/corrupted_firstmate_anchor_projectile.dynamicslice",
            )
            .expect("the anchor");
        assert_eq!(
            (anchor.projectile_component, anchor.ownership_component),
            (1, None)
        );
        let bomb_attack = data
            .ranged_attack_stream("AI_Undead", "Grenadier_Bomb")
            .expect("Grenadier_Bomb");
        assert_eq!(bomb_attack.index, 198);
        assert!(bomb_attack.arc.is_some());
        let anchor_attack = data
            .ranged_attack_stream("AI_Undead", "Officer_Anchor")
            .expect("Officer_Anchor");
        assert_eq!((anchor_attack.index, anchor_attack.arc), (204, None));
        assert_eq!(
            data.damage_row_index("FTUEDamageTableMiner", "Drowned_Miner_Bomb"),
            Some(0)
        );
    }
}
