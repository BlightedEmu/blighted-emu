// SPDX-License-Identifier: AGPL-3.0-or-later
//! The attacks of ranged and magic weapons (staffs, gauntlets, bows, muskets): the server casts
//! their projectile. A retail client sends no hit request for them, only its inputs (Observed:
//! the fire staff and bow capture holds 293 hits and no client message but inputs around them),
//! and the projectile spells are server data (`SpellData`: `CastingTypes` Projectile, the
//! `DamageTableRow` each hit deals). So an attack flies along the player's aim: the first mob on
//! its line, where the client shows it ([`super::mob_trail`]), is hit once the projectile got
//! there.

use nw_bus::ConnectionRef;
use nw_content::combat::{Projectile, WeaponKind};
use nw_sim_core::Position;

use super::SimWorld;
use super::attack::{Aim, AttackKind};
use super::mob_timeline::yaw_toward;
use crate::combat::{AttackOutcome, AttackRefusal};
use crate::mobs::Capsule;

/// How far apart in height a projectile and a body may be and still meet (our choice: the
/// projectile flies at the camera's aim, which the server reads as a yaw only).
const PROJECTILE_HEIGHT_M: f32 = 3.0;
/// Milliseconds per second.
const MS_PER_SECOND: f32 = 1_000.0;

/// One attack of a ranged or magic weapon.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Cast {
    /// The weapon's kind.
    pub(super) weapon: WeaponKind,
    /// How its projectile flies.
    pub(super) projectile: Projectile,
    /// Its light or heavy attack.
    pub(super) kind: AttackKind,
}

/// A projectile on its way to the mob it hits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProjectileFlight {
    attacker: ConnectionRef,
    target: u64,
    damage_id: String,
    /// When it gets there.
    land_ms: u64,
}

/// The first body of `bodies` (mob, place, capsule) that a projectile of `projectile` from
/// `from` along yaw `yaw` meets, with how far it flies to it; `None` when it meets none.
pub(super) fn first_on_line(
    projectile: &Projectile,
    from: Position,
    yaw: f32,
    bodies: &[(u64, Position, Capsule)],
) -> Option<(u64, f32)> {
    // Yaw 0 faces +y; yaw grows toward -x (see `yaw_toward`).
    let (east, north) = (-yaw.sin(), yaw.cos());
    bodies
        .iter()
        .filter_map(|(mob, at, body)| {
            let (dx, dy) = (at.x - from.x, at.y - from.y);
            let along = dx * east + dy * north;
            let aside = (dx * north - dy * east).abs();
            let meets = along >= 0.0
                && along <= projectile.range_m + body.radius_m
                && aside <= projectile.radius_m + body.radius_m
                && (at.z - from.z).abs() <= PROJECTILE_HEIGHT_M;
            meets.then_some((*mob, along))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

impl SimWorld {
    /// The player of `attacker` at `position` casts the projectile of its weapon's attack
    /// (`cast`) at `now_ms`, along its aim: toward the mob it locked on, else along its camera
    /// yaw, else toward the nearest mob of `places`. The mob it meets is hit when the projectile
    /// gets there ([`SimWorld::projectile_flights_due`]).
    pub(super) fn cast(
        &mut self,
        (attacker, aim): (&ConnectionRef, Aim),
        cast: Cast,
        position: Position,
        places: &[(u64, Position, Capsule)],
        now_ms: u64,
    ) -> Vec<Result<AttackOutcome, AttackRefusal>> {
        let Cast {
            weapon,
            projectile,
            kind,
        } = cast;
        let toward = |mob: u64| {
            places
                .iter()
                .find(|(place_mob, _, _)| *place_mob == mob)
                .map(|(_, at, _)| yaw_toward(position, *at))
        };
        let nearest = || {
            places
                .iter()
                .min_by(|a, b| {
                    a.1.horizontal_distance(position)
                        .total_cmp(&b.1.horizontal_distance(position))
                })
                .map(|(_, at, _)| yaw_toward(position, *at))
        };
        let yaw = aim
            .lock_target
            .and_then(toward)
            .or(aim.facing)
            .or_else(nearest);
        let Some(yaw) = yaw else {
            return vec![Err(AttackRefusal::NoTarget)];
        };
        let Some(damage_id) = self
            .combat
            .data
            .weapon_attack(weapon, kind == AttackKind::Heavy)
            .map(|row| row.damage_id.clone())
        else {
            return vec![Err(AttackRefusal::NotWeaponAttack)];
        };
        let met = first_on_line(&projectile, position, yaw, places);
        tracing::debug!(
            attacker = ?attacker,
            ?weapon,
            ?kind,
            yaw,
            met = ?met,
            "player cast"
        );
        let Some((target, along)) = met else {
            return vec![Err(AttackRefusal::NoTarget)];
        };
        // A flight of a few hundred ms.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a flight of at most a few seconds"
        )]
        let flight_ms = (along / projectile.speed_mps * MS_PER_SECOND) as u64;
        self.projectile_flights.push(ProjectileFlight {
            attacker: attacker.clone(),
            target,
            damage_id,
            land_ms: now_ms.saturating_add(flight_ms),
        });
        Vec::new()
    }

    /// Lands the projectiles that got to their mob at `now_ms`.
    pub(super) fn projectile_flights_due(
        &mut self,
        now_ms: u64,
    ) -> Vec<Result<AttackOutcome, AttackRefusal>> {
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.projectile_flights)
            .into_iter()
            .partition(|flight| now_ms >= flight.land_ms);
        self.projectile_flights = later;
        due.into_iter()
            .map(|flight| {
                self.hit(
                    &flight.attacker,
                    flight.target,
                    &flight.damage_id,
                    None,
                    false,
                    now_ms,
                )
            })
            .collect()
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
    const NORTH: f32 = 0.0;
    const BOLT: Projectile = Projectile {
        cast_ms: 250,
        range_m: 25.0,
        speed_mps: 40.0,
        radius_m: 0.4,
    };

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
        let far = [(5, at(0.0, BOLT.range_m + BODY.radius_m + 1.0), BODY)];
        assert_eq!(
            first_on_line(&BOLT, FROM, NORTH, &far),
            None,
            "out of range"
        );
        let high = [(
            6,
            Position {
                z: FROM.z + PROJECTILE_HEIGHT_M + 1.0,
                ..at(0.0, 5.0)
            },
            BODY,
        )];
        assert_eq!(first_on_line(&BOLT, FROM, NORTH, &high), None, "far above");
        let east = [(7, at(10.0, 0.0), BODY)];
        let yaw_east = yaw_toward(FROM, at(10.0, 0.0));
        assert_eq!(
            first_on_line(&BOLT, FROM, yaw_east, &east).map(|(mob, _)| mob),
            Some(7),
            "the yaw convention of yaw_toward"
        );
    }
}
