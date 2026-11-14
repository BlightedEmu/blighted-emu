// SPDX-License-Identifier: AGPL-3.0-or-later
//! The attacks of ranged and magic weapons (staffs, gauntlets, bows, muskets): the server casts
//! their projectile. A retail client sends no hit request for them, only its inputs (Observed:
//! the fire staff and bow capture holds 293 hits and no client message but inputs around them),
//! and the projectiles are server data: the attack's `CAGE-RangedAttack` clip or its spell
//! (`SpellData`), the ranged attack database's range and radius, the projectile slice's flight
//! ([`nw_content::player_attacks`]). So an attack flies along the player's aim: the first mob on
//! its line, where the client shows it ([`super::mob_trail`]), is hit once the projectile got
//! there.
//!
//! **Level stops.** A solid of the level on the way stops the shot there ([`LevelStop`]): it
//! meets no body behind it, and its projectile entity names the level entity of the solid as its
//! `collidedWithGDEId`, with no hit volume. Observed (`opr_que_and_lost`, projectile states of
//! class 16): 940 states of shots on the OPR Vertical outer perimeter wall `d2667a566f022663` (a
//! capital, [`nw_world::collision::BodyEntity`]) send children 0, 7 and 9 and no child 8; a hit of
//! the world (a solid of no entity) sends 0 and 9 only (projectile study 2026-11-11). A turret's
//! shot names the same id as its 2720 target with result `ff` (helpers B15 and B19). Inferred: the
//! shot's way to the solid is the straight line from its muzzle to where it ends (the body it
//! meets, else the end of its reach with its fall), and a splash meets the bodies around the
//! solid's point.

use nw_bus::ConnectionRef;
use nw_content::player_attacks::Flight;
use nw_sim_core::Position;

use super::SimWorld;
use super::attack::Pace;
use super::hit_shape::PLAYER_CAPSULE;
use super::mob_timeline::yaw_toward;
use crate::combat::{AttackOutcome, AttackRefusal};
use crate::mobs::{Capsule, spawn_index};

/// Milliseconds per second.
const MS_PER_SECOND: f32 = 1_000.0;
/// The top share of a body's height that is its head (Inferred: a 1.8 m body's head is about
/// 0.27 m; the retail head volumes hang on the skeleton's head joint, which the server does not
/// animate).
const HEAD_SHARE: f32 = 0.15;

/// How a projectile or hit scan of one attack meets bodies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Shot {
    /// `m_maxRange`.
    range_m: f32,
    /// `m_projectileRadius`; 0 for a hit scan.
    radius_m: f32,
    /// How it flies; `None` for a hit scan, which is there at once.
    flight: Option<Flight>,
    /// The radius around the body it meets in which every mob is hit; 0: that body only.
    splash_m: f32,
}

impl Shot {
    /// A projectile of `range_m` and `radius_m` that flies as `flight`
    /// ([`AttackHit::Projectile`]).
    pub(super) const fn projectile(range_m: f32, radius_m: f32, flight: Flight) -> Self {
        Self {
            range_m,
            radius_m,
            flight: Some(flight),
            splash_m: 0.0,
        }
    }

    /// A hit scan of `range_m` ([`AttackHit::HitScan`]).
    pub(super) const fn hit_scan(range_m: f32) -> Self {
        Self {
            range_m,
            radius_m: 0.0,
            flight: None,
            splash_m: 0.0,
        }
    }

    /// How far it goes when it meets nothing, and when it ends there (seconds): its range, or
    /// where its flight's lifetime ends before it.
    pub(super) fn end(&self) -> (f32, f32) {
        match &self.flight {
            None => (self.range_m, 0.0),
            Some(flight) => {
                let distance_m = self.range_m.min(flight.distance_at(flight.lifetime_s));
                let time_s = flight.time_to(distance_m).unwrap_or(flight.lifetime_s);
                (distance_m, time_s)
            }
        }
    }

    /// The shot, hitting every mob within `splash_m` of the body it meets.
    pub(super) const fn with_splash(self, splash_m: f32) -> Self {
        Self { splash_m, ..self }
    }
}

/// What a shot does to the mobs it hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ShotHit<'a> {
    /// The `DamageData` row of the hit; `None` for a spell of status effects alone, which hurts
    /// no one.
    pub(super) damage_id: Option<&'a str>,
    /// The status effects it puts on a mob it hurts, with the number of the player they come
    /// from (an ability spell's `StatusEffects`).
    pub(super) effects: Option<(&'a [String], u64)>,
    /// It meets the head of the mob it flies to ([`meets_head`]).
    pub(super) headshot: bool,
}

impl<'a> ShotHit<'a> {
    /// A hit of the row `damage_id` and nothing more.
    pub(super) const fn plain(damage_id: &'a str) -> Self {
        Self {
            damage_id: Some(damage_id),
            effects: None,
            headshot: false,
        }
    }
}

/// Where a shot meets the level's solids first ([`SimWorld::level_stop`], module doc **Level
/// stops**).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct LevelStop {
    /// Where it meets the solid.
    pub(super) at: Position,
    /// How far from where it left, in the ground plane (the distance of its flight).
    pub(super) along_m: f32,
    /// The GDE id of the level entity whose solid it is; `None` for a solid of no entity.
    pub(super) entity: Option<u64>,
}

/// Where a shot ends ([`SimWorld::shot_stop`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum ShotEnd {
    /// On the body `target`, after `flight_s` seconds.
    Body { target: u64, flight_s: f32 },
    /// On a solid of the level, after `flight_s` seconds.
    Level { stop: LevelStop, flight_s: f32 },
    /// Nowhere within its reach.
    Free,
}

/// What a projectile flies to.
#[derive(Debug, Clone, Copy, PartialEq)]
enum FlightEnd {
    /// The mob or PvP foe of this number.
    Body(u64),
    /// A solid of the level.
    Level(LevelStop),
}

/// A projectile on its way to what it hits.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ProjectileFlight {
    attacker: ConnectionRef,
    end: FlightEnd,
    /// [`ShotHit::damage_id`].
    damage_id: Option<String>,
    /// When it gets there.
    land_ms: u64,
    /// [`Shot::with_splash`].
    splash_m: f32,
    /// [`ShotHit::effects`].
    effects: Option<(Vec<String>, u64)>,
    /// [`ShotHit::headshot`].
    headshot: bool,
    /// The slot key of its projectile entity ([`super::shot_entity`]), when it has one.
    entity: Option<u64>,
}

/// A flight of `flight_s` seconds in whole milliseconds.
pub(super) fn flight_ms(flight_s: f32) -> u64 {
    // A flight of at most its lifetime, a few seconds.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a flight of at most a few seconds"
    )]
    let flight_ms = (flight_s.max(0.0) * MS_PER_SECOND) as u64;
    flight_ms
}

/// Where an attack is aimed: the camera's yaw and pitch (the input's view angles; a positive
/// pitch looks down, Observed: the retail `lookDir` z is `-sin(pitch)`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct AimLine {
    pub(super) yaw: f32,
    pub(super) pitch: f32,
}

/// Half of a height.
const HALF: f32 = 0.5;

impl AimLine {
    /// The line of a shot from a shooter standing at `from`: to the point under the crosshair
    /// `point` (the aim point of the client's input) when the input sent one, else along the
    /// camera's `yaw` and `pitch`; `None` without a yaw or a pitch. The camera stands over the
    /// shooter's shoulder, so its angles from the shooter's own place pass beside or under the
    /// body under the crosshair (live 2026-11-10: 13 of 69 life staff casts met no mob, with
    /// the crosshair on it); the line to the point leaves the middle of the shooter's body.
    pub(super) fn of(
        from: Position,
        point: Option<Position>,
        yaw: Option<f32>,
        pitch: Option<f32>,
    ) -> Option<Self> {
        point
            .and_then(|point| {
                let along_m = from.horizontal_distance(point);
                let rise_m = point.z - (from.z + PLAYER_CAPSULE.height_m * HALF);
                (along_m > f32::EPSILON).then(|| Self {
                    yaw: yaw_toward(from, point),
                    pitch: -(rise_m / along_m).atan(),
                })
            })
            .or_else(|| {
                Some(Self {
                    yaw: yaw?,
                    pitch: pitch?,
                })
            })
    }
}

/// Where `shot` from a shooter standing at `from` along `aim` passes a body at `at`: how far
/// along its line (m), how far beside it (m), how long it flies there (s) and how far its line
/// rose from where it left the shooter (m; below 0 it sank); `None` behind the shooter or
/// beyond its flight. It climbs or sinks with the aim's pitch and falls by its gravity.
fn line_at(
    shot: &Shot,
    from: Position,
    aim: AimLine,
    at: Position,
) -> Option<(f32, f32, f32, f32)> {
    let yaw = aim.yaw;
    // Yaw 0 faces +y; yaw grows toward -x (see `yaw_toward`).
    let (east, north) = (-yaw.sin(), yaw.cos());
    let (dx, dy) = (at.x - from.x, at.y - from.y);
    let along = dx * east + dy * north;
    if along < 0.0 {
        return None;
    }
    let aside = (dx * north - dy * east).abs();
    let (time_s, drop_m) = match &shot.flight {
        Some(flight) => {
            let time_s = flight.time_to(along)?;
            (time_s, flight.drop_at(time_s))
        }
        None => (0.0, 0.0),
    };
    Some((along, aside, time_s, along * (-aim.pitch).tan() - drop_m))
}

/// The point `along_m` metres (in the ground plane) along the line of `shot` that leaves
/// `muzzle` along `aim`: it climbs or sinks with the aim's pitch and falls by its gravity until
/// then ([`line_at`]).
fn point_along(shot: &Shot, muzzle: Position, aim: AimLine, along_m: f32) -> Position {
    let (east, north) = (-aim.yaw.sin(), aim.yaw.cos());
    let drop_m = shot.flight.as_ref().map_or(0.0, |flight| {
        flight.drop_at(flight.time_to(along_m).unwrap_or(flight.lifetime_s))
    });
    Position {
        x: muzzle.x + east * along_m,
        y: muzzle.y + north * along_m,
        z: muzzle.z + along_m * (-aim.pitch).tan() - drop_m,
    }
}

/// How long `shot` flies to `along_m` metres (in the ground plane): 0 for a hit scan; the end of
/// its lifetime when its flight does not get that far.
fn flight_time_to(shot: &Shot, along_m: f32) -> f32 {
    shot.flight.as_ref().map_or(0.0, |flight| {
        flight.time_to(along_m).unwrap_or(flight.lifetime_s)
    })
}

/// The place a shot leaves from a shooter standing at `from`: the middle of its body.
pub(super) fn muzzle_of(from: Position) -> Position {
    Position {
        z: from.z + PLAYER_CAPSULE.height_m * HALF,
        ..from
    }
}

/// The first body of `bodies` (mob, place, capsule) that `shot` from a shooter standing at `from`
/// along `aim` meets, with how long it flies to it (seconds); `None` when it meets none. It meets
/// a body whose edge is within its range and its radius of its line, before its flight ends,
/// while it is level with the body: it leaves within the shooter's body height ([`line_at`]).
pub(super) fn first_on_line(
    shot: &Shot,
    from: Position,
    aim: AimLine,
    bodies: &[(u64, Position, Capsule)],
) -> Option<(u64, f32)> {
    bodies
        .iter()
        .filter_map(|(mob, at, body)| {
            let (along, aside, time_s, rise_m) = line_at(shot, from, aim, *at)?;
            if along > shot.range_m + body.radius_m || aside > shot.radius_m + body.radius_m {
                return None;
            }
            let low = from.z + rise_m - shot.radius_m;
            let high = from.z + PLAYER_CAPSULE.height_m + rise_m + shot.radius_m;
            let level = low < at.z + body.height_m && at.z < high;
            level.then_some((*mob, along, time_s))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(mob, _, time_s)| (mob, time_s))
}

/// Whether `shot` from `from` along `aim` meets mob `target` of `bodies` on its head: its
/// line, which leaves the middle of the shooter's body, passes in the top [`HEAD_SHARE`] of the
/// body.
pub(super) fn meets_head(
    shot: &Shot,
    (from, aim): (Position, AimLine),
    target: u64,
    bodies: &[(u64, Position, Capsule)],
) -> bool {
    bodies
        .iter()
        .find(|(mob, _, _)| *mob == target)
        .is_some_and(|(_, at, body)| {
            line_at(shot, from, aim, *at).is_some_and(|(_, _, _, rise_m)| {
                let height = from.z + PLAYER_CAPSULE.height_m * HALF + rise_m;
                height >= at.z + body.height_m * (1.0 - HEAD_SHARE)
            })
        })
}

impl SimWorld {
    /// The player of `attacker` at `position` fires `shot` of the attack whose hit is `hit` at
    /// `now_ms`, along `aim`. The mob it meets of `places` is hit when the shot gets there
    /// ([`SimWorld::projectile_flights_due`]).
    pub(super) fn cast(
        &mut self,
        attacker: &ConnectionRef,
        (shot, hit): (Shot, ShotHit<'_>),
        (position, aim): (Position, AimLine),
        places: &[(u64, Position, Capsule)],
        now_ms: u64,
    ) -> Vec<Result<AttackOutcome, AttackRefusal>> {
        let met = first_on_line(&shot, position, aim, places);
        let end = self.shot_stop(&shot, (position, aim), met, places);
        tracing::debug!(attacker = ?attacker, damage_id = ?hit.damage_id, ?aim, end = ?end, "player cast");
        let entity = self.shot_entity_of(attacker, &shot, &hit, (position, aim), now_ms);
        match end {
            ShotEnd::Body { target, flight_s } => {
                let headshot = meets_head(&shot, (position, aim), target, places);
                self.launch(
                    attacker,
                    (shot, ShotHit { headshot, ..hit }),
                    (target, entity),
                    flight_s,
                    now_ms,
                );
                Vec::new()
            }
            ShotEnd::Level { stop, flight_s } => {
                self.launch_to_level(attacker, (shot, hit), (stop, entity), flight_s, now_ms);
                vec![Ok(AttackOutcome::Missed)]
            }
            ShotEnd::Free => vec![Ok(AttackOutcome::Missed)],
        }
    }

    /// The first solid of the level on the straight line from `from` to `to` (module doc
    /// **Level stops**); `None` when the line meets none.
    pub(super) fn level_stop(&self, from: Position, to: Position) -> Option<LevelStop> {
        let (at, entity) = self.ground.first_solid(from, to)?;
        Some(LevelStop {
            at,
            along_m: from.horizontal_distance(at),
            entity,
        })
    }

    /// Where `shot` from a shooter standing at `position` along `aim` ends: on the body `met`
    /// ([`first_on_line`] of `places`) unless a solid of the level stands on the way to it,
    /// else on the first solid within its reach (module doc **Level stops**).
    pub(super) fn shot_stop(
        &self,
        shot: &Shot,
        (position, aim): (Position, AimLine),
        met: Option<(u64, f32)>,
        places: &[(u64, Position, Capsule)],
    ) -> ShotEnd {
        let muzzle = muzzle_of(position);
        // The shot flies along its own line ([`point_along`]) up to the body it met, or to the
        // end of its reach: a solid of the level on that way stops it first.
        let body = met.and_then(|(target, flight_s)| {
            places
                .iter()
                .find(|(number, _, _)| *number == target)
                .map(|(_, at, _)| (target, flight_s, muzzle.horizontal_distance(*at)))
        });
        let way_m = body.map_or(shot.end().0, |(_, _, along_m)| along_m);
        if let Some(stop) = self.level_stop(muzzle, point_along(shot, muzzle, aim, way_m)) {
            return ShotEnd::Level {
                stop,
                flight_s: flight_time_to(shot, stop.along_m),
            };
        }
        body.map_or(ShotEnd::Free, |(target, flight_s, _)| ShotEnd::Body {
            target,
            flight_s,
        })
    }

    /// Spawns the projectile entity of `shot` with the hit `hit`, which the player of
    /// `attacker` fires from `position` along `aim` at `now_ms` ([`super::shot_entity`]); gives
    /// its slot key. A hit scan and a shot of effects alone have none.
    pub(super) fn shot_entity_of(
        &mut self,
        attacker: &ConnectionRef,
        shot: &Shot,
        hit: &ShotHit<'_>,
        origin: (Position, AimLine),
        now_ms: u64,
    ) -> Option<u64> {
        shot.flight?;
        let shooter = self.by_connection.get(attacker).copied()?;
        self.spawn_shot_entity(shooter, hit.damage_id?, origin, now_ms)
    }

    /// Sends `shot` of `attacker` with the hit `hit` to `target`, which it gets to after
    /// `flight_s` from `now_ms`; `entity` is the slot key of its projectile entity.
    pub(super) fn launch(
        &mut self,
        attacker: &ConnectionRef,
        (shot, hit): (Shot, ShotHit<'_>),
        (target, entity): (u64, Option<u64>),
        flight_s: f32,
        now_ms: u64,
    ) {
        self.push_flight(
            attacker,
            (shot, hit),
            (FlightEnd::Body(target), entity),
            flight_s,
            now_ms,
        );
    }

    /// Sends `shot` of `attacker` with the hit `hit` to the solid of the level at `stop`, which
    /// it gets to after `flight_s` from `now_ms`; `entity` is the slot key of its projectile
    /// entity. A shot with neither an entity nor a splash leaves nothing to show.
    pub(super) fn launch_to_level(
        &mut self,
        attacker: &ConnectionRef,
        (shot, hit): (Shot, ShotHit<'_>),
        (stop, entity): (LevelStop, Option<u64>),
        flight_s: f32,
        now_ms: u64,
    ) {
        if entity.is_none() && shot.splash_m <= 0.0 {
            return;
        }
        self.push_flight(
            attacker,
            (shot, hit),
            (FlightEnd::Level(stop), entity),
            flight_s,
            now_ms,
        );
    }

    fn push_flight(
        &mut self,
        attacker: &ConnectionRef,
        (shot, hit): (Shot, ShotHit<'_>),
        (end, entity): (FlightEnd, Option<u64>),
        flight_s: f32,
        now_ms: u64,
    ) {
        self.projectile_flights.push(ProjectileFlight {
            attacker: attacker.clone(),
            end,
            entity,
            damage_id: hit.damage_id.map(str::to_owned),
            land_ms: now_ms.saturating_add(flight_ms(flight_s)),
            splash_m: shot.splash_m,
            effects: hit
                .effects
                .map(|(effects, source)| (effects.to_vec(), source)),
            headshot: hit.headshot,
        });
    }

    /// The yaw of an attack from `from` with the aim of a lock on `lock_target` (toward it when it
    /// is among `places`) or the camera yaw `facing`.
    pub(super) fn aim_yaw(
        lock_target: Option<u64>,
        facing: Option<f32>,
        from: Position,
        places: &[(u64, Position, Capsule)],
    ) -> Option<f32> {
        lock_target
            .and_then(|target| {
                places
                    .iter()
                    .find(|(mob, _, _)| *mob == target)
                    .map(|(_, at, _)| yaw_toward(from, *at))
            })
            .or(facing)
    }

    /// Lands the projectiles that got to their target (a mob, a PvP foe or a solid of the level)
    /// at `now_ms`: a splash also hits the other living mobs and foes within its radius of that
    /// target, and each one it hurts takes its effects; a projectile of effects alone puts them
    /// on each living target it reaches. One on the level hurts only by its splash.
    pub(super) fn projectile_flights_due(
        &mut self,
        now_ms: u64,
    ) -> Vec<Result<AttackOutcome, AttackRefusal>> {
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.projectile_flights)
            .into_iter()
            .partition(|flight| now_ms >= flight.land_ms);
        self.projectile_flights = later;
        let mut results = Vec::new();
        for flight in due {
            let (first, centre) = match flight.end {
                FlightEnd::Body(target) => {
                    let at = self.target_at(target);
                    // The projectile entity shows the hit where its target is now.
                    if let Some((entity, at)) = flight.entity.zip(at) {
                        let impact = Position {
                            z: at.z + PLAYER_CAPSULE.height_m * HALF,
                            ..at
                        };
                        self.shot_entity_hit(entity, target, impact, now_ms);
                    }
                    (Some(target), at)
                }
                FlightEnd::Level(stop) => {
                    if let Some(entity) = flight.entity {
                        self.shot_entity_on_level(entity, stop.entity, stop.at, now_ms);
                    }
                    (None, Some(stop.at))
                }
            };
            let splashed = centre.map_or_else(Vec::new, |centre| {
                self.splashed(&flight, centre, first)
            });
            let first = first.into_iter().map(|target| (target, true));
            for (target, paced) in first.chain(splashed.into_iter().map(|other| (other, false))) {
                let Some(damage_id) = &flight.damage_id else {
                    if let Some((effects, source)) = &flight.effects {
                        self.spell_effects_on_target(&flight.attacker, target, effects, *source);
                    }
                    continue;
                };
                // Only the target it flew to takes it on the head; the splash does not.
                let pace = Pace {
                    paced,
                    headshot: paced && flight.headshot,
                };
                let result =
                    self.hit_paced(&flight.attacker, target, damage_id, None, pace, now_ms);
                if let (Ok(AttackOutcome::Hit { target, .. }), Some((effects, source))) =
                    (&result, &flight.effects)
                {
                    self.spell_effects_on_target(&flight.attacker, *target, effects, *source);
                }
                results.push(result);
            }
        }
        results
    }

    /// The living targets within the splash of `flight` around `centre`, besides the target it
    /// flew to (`flown_to`): mobs, and the PvP foes of its attacker.
    fn splashed(
        &mut self,
        flight: &ProjectileFlight,
        centre: Position,
        flown_to: Option<u64>,
    ) -> Vec<u64> {
        if flight.splash_m <= 0.0 {
            return Vec::new();
        }
        let foes = self
            .by_connection
            .get(&flight.attacker)
            .copied()
            .map_or_else(Vec::new, |attacker| self.foe_places(attacker));
        let mobs = self
            .mob_states()
            .filter(|(_, _, state)| state.health > 0)
            .map(|(mob, at, _)| (mob, at));
        mobs.chain(foes.into_iter().map(|(foe, at, _)| (foe, at)))
            .filter(|(other, at)| {
                Some(*other) != flown_to && at.horizontal_distance(centre) <= flight.splash_m
            })
            .map(|(other, _)| other)
            .collect()
    }

    /// Puts the spell `effects` of player `source`, whose connection is `attacker`, on the
    /// living target `target` of its attack: a mob, or a player.
    pub(super) fn spell_effects_on_target(
        &mut self,
        attacker: &ConnectionRef,
        target: u64,
        effects: &[String],
        source: u64,
    ) {
        if spawn_index(target).is_some() {
            let alive = self
                .mob_states()
                .any(|(mob, _, state)| mob == target && state.health > 0);
            if alive {
                self.spell_effects_on_mob(target, effects, source, false);
            }
            return;
        }
        let Some(entity) = self.player_by_number(target) else {
            return;
        };
        let hit_amount = self
            .by_connection
            .get(attacker)
            .copied()
            .map_or(0.0, |caster| self.spell_heal_base(caster));
        self.spell_effects_on_player(entity, effects, (source, hit_amount), false);
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
    const BODY: Capsule = Capsule {
        radius_m: 0.5,
        height_m: 1.8,
    };
    const NORTH: AimLine = AimLine {
        yaw: 0.0,
        pitch: 0.0,
    };
    /// The ice gauntlet's light bolt (`IceMagic` / `Primary`: 50 m, radius 0.15;
    /// `icemagic_primary_proj`: 100 m/s, 25 more over 1 s, no gravity, 0.5 s).
    const BOLT: Shot = Shot::projectile(
        50.0,
        0.15,
        Flight {
            base_speed_mps: 100.0,
            extra_speed_mps: 25.0,
            time_for_max_speed_s: 1.0,
            gravity_mps2: 0.0,
            lifetime_s: 0.5,
        },
    );
    /// The bow's hip shot (`Bow` / `Basic`: 100 m, radius 0.15; `basic_arrow`: gravity 9.8, 10 s).
    const ARROW: Shot = Shot::projectile(
        100.0,
        0.15,
        Flight {
            base_speed_mps: 100.0,
            extra_speed_mps: 25.0,
            time_for_max_speed_s: 1.0,
            gravity_mps2: 9.8,
            lifetime_s: 10.0,
        },
    );

    fn at(dx: f32, dy: f32) -> Position {
        Position {
            x: FROM.x + dx,
            y: FROM.y + dy,
            ..FROM
        }
    }

    #[test]
    fn a_projectile_meets_the_first_body_on_its_line() {
        let bodies = [
            (1, at(0.0, 15.0), BODY),
            (2, at(0.3, 8.0), BODY),
            (3, at(3.0, 5.0), BODY),
            (4, at(0.0, -5.0), BODY),
        ];
        assert_eq!(
            first_on_line(&BOLT, FROM, NORTH, &bodies).map(|(mob, _)| mob),
            Some(2),
            "the nearer one on the line; not the one aside nor behind"
        );
        let east = [(7, at(10.0, 0.0), BODY)];
        let yaw_east = AimLine {
            yaw: yaw_toward(FROM, at(10.0, 0.0)),
            pitch: 0.0,
        };
        assert_eq!(
            first_on_line(&BOLT, FROM, yaw_east, &east).map(|(mob, _)| mob),
            Some(7),
            "the yaw convention of yaw_toward"
        );
    }

    #[test]
    fn a_bolt_in_the_top_of_a_body_is_a_head_shot() {
        // The bolt's line leaves 0.9 m over the shooter's feet and stays level: on a body whose
        // feet are 0.72 m lower it passes in the top 0.27 m; on a level body, at its middle.
        let low = Position {
            z: FROM.z - 0.72,
            ..at(0.0, 20.0)
        };
        let head = [(1, low, BODY)];
        assert_eq!(
            first_on_line(&BOLT, FROM, NORTH, &head).map(|(mob, _)| mob),
            Some(1)
        );
        assert!(meets_head(&BOLT, (FROM, NORTH), 1, &head));
        let level = [(1, at(0.0, 20.0), BODY)];
        assert!(!meets_head(&BOLT, (FROM, NORTH), 1, &level));
        assert!(!meets_head(&BOLT, (FROM, NORTH), 2, &head), "no such mob");
    }

    #[test]
    fn a_projectile_flies_until_its_lifetime_ends() {
        // 0.5 s of the bolt: 50 m plus 3.125 m of its gained speed.
        let (_, time_s) =
            first_on_line(&BOLT, FROM, NORTH, &[(1, at(0.0, 40.0), BODY)]).expect("in reach");
        // 100 t + 12.5 t^2 = 40: t = 0.382 s.
        assert!((time_s - 0.382).abs() < 0.002, "{time_s}");
        assert_eq!(
            first_on_line(&BOLT, FROM, NORTH, &[(1, at(0.0, 54.0), BODY)]),
            None,
            "beyond its flight"
        );
    }

    #[test]
    fn a_falling_projectile_passes_under_a_high_body() {
        // An arrow at 80 m flies about 0.73 s and falls about 2.6 m: aimed level it passes under
        // a level body, aimed up by 2.6 m over 80 m it meets it.
        let up = AimLine {
            yaw: 0.0,
            pitch: -(2.6_f32 / 80.0).atan(),
        };
        let level = [(1, at(0.0, 80.0), BODY)];
        assert_eq!(first_on_line(&ARROW, FROM, NORTH, &level), None);
        assert!(first_on_line(&ARROW, FROM, up, &level).is_some());
        let high = [(
            1,
            Position {
                z: FROM.z + PLAYER_CAPSULE.height_m + BODY.radius_m,
                ..at(0.0, 80.0)
            },
            BODY,
        )];
        let below = [(
            1,
            Position {
                z: FROM.z - 3.0,
                ..at(0.0, 80.0)
            },
            BODY,
        )];
        assert_eq!(first_on_line(&ARROW, FROM, up, &high), None);
        assert!(
            first_on_line(&ARROW, FROM, NORTH, &below).is_some(),
            "it fell to the body"
        );
        let bolt_below = [(
            1,
            Position {
                z: FROM.z - 3.0,
                ..at(0.0, 40.0)
            },
            BODY,
        )];
        assert_eq!(
            first_on_line(&BOLT, FROM, NORTH, &bolt_below),
            None,
            "a bolt does not fall"
        );
    }

    #[test]
    fn a_shot_flies_to_the_point_under_the_crosshair() {
        // A mob 8 m ahead whose middle is 0.9 m beside the shooter's own line of the camera's
        // yaw (the camera stands over the shoulder): the camera's angles from the shooter's
        // place pass beside it, the line to the aim point on it meets it.
        const DOWN: f32 = 0.1;
        let mob = at(0.9, 8.0);
        let bodies = [(1, mob, BODY)];
        let point = Position {
            z: mob.z + BODY.height_m * HALF,
            ..mob
        };
        let camera = AimLine::of(FROM, None, Some(0.0), Some(DOWN)).expect("angles");
        assert_eq!(first_on_line(&BOLT, FROM, camera, &bodies), None);
        let aimed = AimLine::of(FROM, Some(point), Some(0.0), Some(DOWN)).expect("point");
        assert!(first_on_line(&BOLT, FROM, aimed, &bodies).is_some());
        assert_eq!(AimLine::of(FROM, None, None, Some(DOWN)), None);
        assert_eq!(
            AimLine::of(FROM, Some(FROM), Some(0.0), Some(DOWN)),
            Some(camera)
        );
    }

    /// How far north of [`FROM`] the test wall stands, m.
    const WALL_M: f32 = 10.0;
    /// The level entity of the test wall: the capital id of the OPR Vertical outer perimeter
    /// (the retail 2720 target).
    const PERIMETER: u64 = 0xd266_7a56_6f02_2663;

    /// A flat level with one wall across the north at [`WALL_M`], a solid of [`PERIMETER`].
    struct WallGround;

    impl nw_sim_core::Ground for WallGround {
        fn height_at(&self, _x: f32, _y: f32) -> Option<f32> {
            Some(0.0)
        }

        fn first_solid(&self, from: Position, to: Position) -> Option<(Position, Option<u64>)> {
            let wall = FROM.y + WALL_M;
            let rise = to.y - from.y;
            if rise.abs() <= f32::EPSILON || (from.y - wall) * (to.y - wall) > 0.0 {
                return None;
            }
            let share = (wall - from.y) / rise;
            let at = Position {
                x: from.x + (to.x - from.x) * share,
                y: wall,
                z: from.z + (to.z - from.z) * share,
            };
            Some((at, Some(PERIMETER)))
        }
    }

    /// A world on the level of [`WallGround`].
    fn wall_world() -> SimWorld {
        use std::sync::Arc;

        use super::super::tests::{MANY, MAX_PLAYERS, settings, slot};
        use crate::combat::tests::test_combat;
        use crate::login::WorldIds;

        SimWorld::new(
            settings(MANY, MAX_PLAYERS),
            WorldIds::random(0).expect("ids"),
            Arc::new(WallGround),
            slot("sim-a"),
            test_combat(),
            Arc::default(),
        )
    }

    #[test]
    fn a_wall_of_the_level_stops_a_shot_before_the_body_behind_it() {
        let world = wall_world();
        let behind = [(1, at(0.0, 15.0), BODY)];
        let met = first_on_line(&BOLT, FROM, NORTH, &behind);
        assert!(met.is_some(), "the body is on the bolt's line");
        let ShotEnd::Level { stop, flight_s } = world.shot_stop(&BOLT, (FROM, NORTH), met, &behind)
        else {
            panic!("the wall stops the bolt");
        };
        assert_eq!(stop.entity, Some(PERIMETER));
        assert!((stop.along_m - WALL_M).abs() < 1e-3, "{stop:?}");
        assert!(
            (stop.at.z - (FROM.z + PLAYER_CAPSULE.height_m * HALF)).abs() < 1e-3,
            "the bolt leaves the middle of the shooter's body and stays level: {stop:?}"
        );
        // 100 t + 12.5 t^2 = 10: t = 0.0990 s.
        assert!((flight_s - 0.099).abs() < 0.001, "{flight_s}");
        let before = [(2, at(0.0, 8.0), BODY)];
        let met = first_on_line(&BOLT, FROM, NORTH, &before);
        assert_eq!(
            world.shot_stop(&BOLT, (FROM, NORTH), met, &before),
            ShotEnd::Body {
                target: 2,
                flight_s: met.expect("met").1
            },
            "a body before the wall takes the bolt"
        );
    }

    #[test]
    fn a_shot_that_meets_no_body_ends_on_the_wall_within_its_reach() {
        let world = wall_world();
        assert!(matches!(
            world.shot_stop(&BOLT, (FROM, NORTH), None, &[]),
            ShotEnd::Level { stop, .. } if stop.entity == Some(PERIMETER)
        ));
        let south = AimLine {
            yaw: std::f32::consts::PI,
            pitch: 0.0,
        };
        assert_eq!(
            world.shot_stop(&BOLT, (FROM, south), None, &[]),
            ShotEnd::Free,
            "away from the wall"
        );
        let short = Shot::hit_scan(WALL_M / 2.0);
        assert_eq!(
            world.shot_stop(&short, (FROM, NORTH), None, &[]),
            ShotEnd::Free,
            "the wall is beyond its reach"
        );
    }

    #[test]
    fn a_falling_shot_reaches_the_place_of_its_flight_and_its_time() {
        // The arrow at 80 m: 0.73 s and a fall of about 2.6 m.
        let muzzle = muzzle_of(FROM);
        let end = point_along(&ARROW, muzzle, NORTH, 80.0);
        assert!((end.y - (FROM.y + 80.0)).abs() < 1e-3, "{end:?}");
        assert!((muzzle.z - end.z - 2.6).abs() < 0.1, "{end:?}");
        assert!((flight_time_to(&ARROW, 80.0) - 0.73).abs() < 0.01);
        assert_eq!(flight_time_to(&Shot::hit_scan(14.0), 10.0), 0.0);
        assert_eq!(
            flight_time_to(&BOLT, 1_000.0),
            0.5,
            "a flight that does not get there ends with its lifetime"
        );
    }

    #[test]
    fn a_shot_on_the_level_with_nothing_to_show_flies_nowhere() {
        let mut world = wall_world();
        let attacker = super::super::tests::connection(1);
        let stop = LevelStop {
            at: at(0.0, WALL_M),
            along_m: WALL_M,
            entity: Some(PERIMETER),
        };
        world.launch_to_level(
            &attacker,
            (BOLT, ShotHit::plain("Test_Row")),
            (stop, None),
            0.1,
            0,
        );
        assert!(world.projectile_flights.is_empty(), "no entity, no splash");
        world.launch_to_level(
            &attacker,
            (BOLT.with_splash(3.0), ShotHit::plain("Test_Row")),
            (stop, None),
            0.1,
            0,
        );
        assert_eq!(world.projectile_flights.len(), 1, "its splash lands");
        assert!(
            world.projectile_flights_due(MS_PER_SECOND as u64).is_empty(),
            "no body stands in the splash"
        );
        assert!(world.projectile_flights.is_empty());
    }

    #[test]
    fn a_hit_scan_is_there_at_once_within_its_range() {
        // The blunderbuss's spread (`BlunderbussPrimaryFire_Spread`: hit scan, 14 m).
        let spread = Shot::hit_scan(14.0);
        assert_eq!(
            first_on_line(&spread, FROM, NORTH, &[(1, at(0.0, 10.0), BODY)]),
            Some((1, 0.0))
        );
        assert_eq!(
            first_on_line(&spread, FROM, NORTH, &[(1, at(0.0, 15.0), BODY)]),
            None
        );
    }
}
