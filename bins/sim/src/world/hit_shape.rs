// SPDX-License-Identifier: AGPL-3.0-or-later
//! The shapes of melee swings (collision note, 2026-09-16): each retail weapon attack has a
//! `CAGE-Damage` clip in its animation database (a capsule or box on the weapon, live for about
//! 0.1 s) and a `Homing` clip that turns the attacker to a target in a radius and angle. The
//! server has no skeletons, so a swing here is a wedge in front of the body: it hits a body whose
//! capsule reaches into it. Scored on the retail tutorial swings, a 1.6 m wedge of ±45 degrees
//! kept all 67 retail hits and hit 1 of 20 mobs that retail did not.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use nw_content::combat::WeaponKind;
use nw_sim_core::Position;

use super::attack::AttackKind;
use super::mob_timeline::yaw_toward;
use crate::mobs::Capsule;

/// The player's body (collision note: the player slice's capsule).
pub(super) const PLAYER_CAPSULE: Capsule = Capsule {
    radius_m: 0.4,
    height_m: 2.0,
};

/// How far apart in height two bodies may stand and still touch (Inferred): the server's height
/// of a body on a ledge, a pool rim or a slope the height map does not hold can be off by a
/// metre (live 2026-09-18: the player on the rim of Thorpe's pool stood above his capsule on
/// 37% of the swings the client showed landing).
const STEP_SLACK_M: f32 = 1.0;

/// Half the arc of a mob swing in front of its body (our choice: retail mob hits found the player
/// within 18 degrees of the mob's facing on 16 of 17 hits).
pub(super) const MOB_SWING_HALF_ARC_RAD: f32 = FRAC_PI_4;

/// How a weapon attack of the player reaches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct SwingShape {
    /// When its damage clip is live, after the attack starts (Observed).
    window_ms: (u64, u64),
    /// How far in front of the body it reaches.
    reach_m: f32,
    /// Half its arc (radians).
    half_arc_rad: f32,
    /// Its homing turns the attacker to a target this near (Observed)...
    homing_radius_m: f32,
    /// ...and within this angle of the facing (radians, Observed).
    homing_angle_rad: f32,
}

impl SwingShape {
    /// When it hits, after the attack starts: the middle of its damage window.
    pub(super) const fn hit_ms(&self) -> u64 {
        self.window_ms.0.midpoint(self.window_ms.1)
    }
}

const fn degrees(value: f32) -> f32 {
    value * PI / 180.0
}

/// Sword light attack: window 333-433 ms; reach our choice in retail's 1.5-1.7 m band; homing
/// 3.75 m, 60 degrees.
const SWORD_LIGHT: SwingShape = SwingShape {
    window_ms: (333, 433),
    reach_m: 1.6,
    half_arc_rad: FRAC_PI_4,
    homing_radius_m: 3.75,
    homing_angle_rad: degrees(60.0),
};
/// Sword heavy attack: window 200-333 ms; a second capsule 1.9 m in front gives a reach of about
/// 2.6 m (Inferred); arc our choice.
const SWORD_HEAVY: SwingShape = SwingShape {
    window_ms: (200, 333),
    reach_m: 2.6,
    half_arc_rad: degrees(30.0),
    homing_radius_m: 3.75,
    homing_angle_rad: degrees(45.0),
};
/// Hatchet: light window 200-300 ms, heavy 300-400 ms; reach Inferred from its capsule.
const HATCHET_LIGHT: SwingShape = SwingShape {
    window_ms: (200, 300),
    reach_m: 1.7,
    half_arc_rad: FRAC_PI_4,
    homing_radius_m: 3.75,
    homing_angle_rad: degrees(60.0),
};
const HATCHET_HEAVY: SwingShape = SwingShape {
    window_ms: (300, 400),
    ..HATCHET_LIGHT
};
/// Rapier: window 333-433 ms; reach Inferred; homing 3.75 m, 45 degrees.
const RAPIER: SwingShape = SwingShape {
    window_ms: (333, 433),
    reach_m: 1.75,
    half_arc_rad: degrees(30.0),
    homing_radius_m: 3.75,
    homing_angle_rad: FRAC_PI_4,
};
/// Spear: light window 400-500 ms, heavy 500-600 ms; reach our choice; homing 4.5 m, 45 degrees.
const SPEAR_LIGHT: SwingShape = SwingShape {
    window_ms: (400, 500),
    reach_m: 2.5,
    half_arc_rad: degrees(30.0),
    homing_radius_m: 4.5,
    homing_angle_rad: FRAC_PI_4,
};
const SPEAR_HEAVY: SwingShape = SwingShape {
    window_ms: (500, 600),
    ..SPEAR_LIGHT
};
/// Great axe: window 300-400 ms; reach our choice; homing 4.5 m, 60 degrees.
const GREAT_AXE: SwingShape = SwingShape {
    window_ms: (300, 400),
    reach_m: 1.9,
    half_arc_rad: degrees(60.0),
    homing_radius_m: 4.5,
    homing_angle_rad: degrees(60.0),
};
/// War hammer: light window 400-533 ms, heavy 467-567 ms; reach our choice; homing 4.5 m, 45
/// degrees.
const WAR_HAMMER_LIGHT: SwingShape = SwingShape {
    window_ms: (400, 533),
    reach_m: 1.8,
    half_arc_rad: FRAC_PI_4,
    homing_radius_m: 4.5,
    homing_angle_rad: FRAC_PI_4,
};
const WAR_HAMMER_HEAVY: SwingShape = SwingShape {
    window_ms: (467, 567),
    ..WAR_HAMMER_LIGHT
};
/// Greatsword: window 433-533 ms; homing 4.5 m, 60 degrees. Reach Inferred: the live
/// greatsword landed on Thorpe at 3 to 4 m between the two bodies' middles (2026-09-18), and the
/// heavy attack steps further in.
const GREAT_SWORD_LIGHT: SwingShape = SwingShape {
    window_ms: (433, 533),
    reach_m: 3.0,
    half_arc_rad: degrees(60.0),
    homing_radius_m: 4.5,
    homing_angle_rad: degrees(60.0),
};
const GREAT_SWORD_HEAVY: SwingShape = SwingShape {
    reach_m: 3.3,
    ..GREAT_SWORD_LIGHT
};
/// Flail: its clips were not extracted; window, reach and arc our choice, as a one-handed
/// weapon of the hatchet's length.
const FLAIL: SwingShape = HATCHET_LIGHT;

/// The shape of attack `attack` with a weapon of `kind`; an item of another kind swings as a
/// sword (our choice); `None` for a ranged or magic weapon, which does not swing. The heavy
/// attacks of the rapier and great axe were not extracted: they swing as their light attack.
pub(super) const fn swing_shape(kind: WeaponKind, attack: AttackKind) -> Option<SwingShape> {
    Some(match (kind, attack) {
        (WeaponKind::Sword | WeaponKind::Other, AttackKind::Light) => SWORD_LIGHT,
        (WeaponKind::Sword | WeaponKind::Other, AttackKind::Heavy) => SWORD_HEAVY,
        (WeaponKind::Hatchet, AttackKind::Light) => HATCHET_LIGHT,
        (WeaponKind::Hatchet, AttackKind::Heavy) => HATCHET_HEAVY,
        (WeaponKind::Rapier, _) => RAPIER,
        (WeaponKind::Flail, _) => FLAIL,
        (WeaponKind::Spear, AttackKind::Light) => SPEAR_LIGHT,
        (WeaponKind::Spear, AttackKind::Heavy) => SPEAR_HEAVY,
        (WeaponKind::GreatAxe, _) => GREAT_AXE,
        (WeaponKind::WarHammer, AttackKind::Light) => WAR_HAMMER_LIGHT,
        (WeaponKind::WarHammer, AttackKind::Heavy) => WAR_HAMMER_HEAVY,
        (WeaponKind::GreatSword, AttackKind::Light) => GREAT_SWORD_LIGHT,
        (WeaponKind::GreatSword, AttackKind::Heavy) => GREAT_SWORD_HEAVY,
        (
            WeaponKind::Bow
            | WeaponKind::Musket
            | WeaponKind::Blunderbuss
            | WeaponKind::FireStaff
            | WeaponKind::LifeStaff
            | WeaponKind::IceGauntlet
            | WeaponKind::VoidGauntlet,
            _,
        ) => return None,
    })
}

/// The turn from yaw `facing` to yaw `toward`, -π to π.
pub(super) fn turn(facing: f32, toward: f32) -> f32 {
    (toward - facing + PI).rem_euclid(TAU) - PI
}

/// A wedge in front of a body at `from` (of capsule `own`), facing yaw `facing`, `reach_m` deep
/// in front of the body and `half_arc_rad` to each side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Wedge {
    pub(super) from: Position,
    pub(super) own: Capsule,
    pub(super) facing: f32,
    pub(super) reach_m: f32,
    pub(super) half_arc_rad: f32,
}

impl Wedge {
    /// The wedge of the swing `shape` of a player at `from` facing `facing`.
    pub(super) const fn of_swing(shape: &SwingShape, from: Position, facing: f32) -> Self {
        Self {
            from,
            own: PLAYER_CAPSULE,
            facing,
            reach_m: shape.reach_m,
            half_arc_rad: shape.half_arc_rad,
        }
    }

    /// How far off its middle a body of capsule `body` at `at` is (radians), when the body
    /// reaches into it: the body's edge is within the reach, the arc widens by the angle the
    /// body's radius takes at its distance, and the two capsules overlap in height.
    pub(super) fn touches(&self, at: Position, body: Capsule) -> Option<f32> {
        let distance = at.horizontal_distance(self.from);
        if distance > self.reach_m + body.radius_m {
            return None;
        }
        let overlaps = at.z < self.from.z + self.own.height_m + STEP_SLACK_M
            && self.from.z < at.z + body.height_m + STEP_SLACK_M;
        if !overlaps {
            return None;
        }
        let widen = if distance > 0.0 {
            (body.radius_m / distance).min(1.0).asin()
        } else {
            FRAC_PI_2
        };
        let off = turn(self.facing, yaw_toward(self.from, at)).abs();
        (off <= self.half_arc_rad + widen).then_some(off)
    }
}

/// The yaw a swing of `shape` from `from` turns to: toward the target of `targets` (positions)
/// that its homing picks, the one with the lowest share of the homing radius plus share of the
/// homing angle; `facing` when none is in the homing's reach.
pub(super) fn homing(
    shape: &SwingShape,
    from: Position,
    facing: f32,
    targets: impl Iterator<Item = Position>,
) -> f32 {
    targets
        .filter_map(|at| {
            let distance = at.horizontal_distance(from);
            let toward = yaw_toward(from, at);
            let off = turn(facing, toward).abs();
            (distance <= shape.homing_radius_m && off <= shape.homing_angle_rad).then(|| {
                let score = distance / shape.homing_radius_m + off / shape.homing_angle_rad;
                (score, toward)
            })
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map_or(facing, |(_, toward)| toward)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: Position = Position {
        x: 100.0,
        y: 100.0,
        z: 20.0,
    };
    const BODY: Capsule = Capsule {
        radius_m: 0.5,
        height_m: 1.8,
    };
    /// Yaw 0 faces north (+y).
    const NORTH: f32 = 0.0;

    fn at(dx: f32, dy: f32) -> Position {
        Position {
            x: ORIGIN.x + dx,
            y: ORIGIN.y + dy,
            ..ORIGIN
        }
    }

    fn sword() -> Wedge {
        Wedge::of_swing(&SWORD_LIGHT, ORIGIN, NORTH)
    }

    #[test]
    fn a_body_whose_edge_is_in_reach_is_hit() {
        assert_eq!(sword().touches(at(0.0, 2.1), BODY), Some(0.0));
        assert_eq!(
            sword().touches(at(0.0, 2.2), BODY),
            None,
            "edge past the reach"
        );
    }

    #[test]
    fn the_arc_widens_by_the_body_radius() {
        // 50 degrees off at 1 m: outside ±45, but the radius takes 30 degrees there.
        let off = degrees(50.0);
        let near = at(-off.sin(), off.cos());
        assert!(sword().touches(near, BODY).is_some());
        let behind = at(0.0, -1.0);
        assert_eq!(sword().touches(behind, BODY), None);
    }

    #[test]
    fn a_body_above_or_below_is_not_hit() {
        let above = Position {
            z: ORIGIN.z + PLAYER_CAPSULE.height_m + STEP_SLACK_M + 0.1,
            ..at(0.0, 1.0)
        };
        assert_eq!(sword().touches(above, BODY), None);
        let below = Position {
            z: ORIGIN.z - BODY.height_m - STEP_SLACK_M - 0.1,
            ..at(0.0, 1.0)
        };
        assert_eq!(sword().touches(below, BODY), None);
        let on_a_ledge = Position {
            z: ORIGIN.z - BODY.height_m - STEP_SLACK_M + 0.1,
            ..at(0.0, 1.0)
        };
        assert!(
            sword().touches(on_a_ledge, BODY).is_some(),
            "a body a step below still touches"
        );
    }

    #[test]
    fn homing_turns_to_the_best_target_in_its_reach() {
        let east = at(3.0, 0.0);
        let north_east = at(1.0, 1.0);
        let turned = homing(&SWORD_LIGHT, ORIGIN, NORTH, [east, north_east].into_iter());
        assert!((turned - yaw_toward(ORIGIN, north_east)).abs() < 1e-5);
        let far = at(0.0, 5.0);
        assert_eq!(
            homing(&SWORD_LIGHT, ORIGIN, NORTH, [far].into_iter()),
            NORTH
        );
    }

    #[test]
    fn each_weapon_hits_inside_its_damage_window() {
        let hit_ms = |kind| swing_shape(kind, AttackKind::Light).map(|shape| shape.hit_ms());
        assert_eq!(hit_ms(WeaponKind::Sword), Some(383));
        assert_eq!(hit_ms(WeaponKind::WarHammer), Some(466));
        assert_eq!(
            swing_shape(WeaponKind::Other, AttackKind::Heavy),
            Some(SWORD_HEAVY)
        );
    }

    #[test]
    fn every_melee_kind_swings_and_no_ranged_or_magic_kind_does() {
        let kinds = [
            WeaponKind::Sword,
            WeaponKind::Hatchet,
            WeaponKind::Rapier,
            WeaponKind::Flail,
            WeaponKind::Spear,
            WeaponKind::GreatAxe,
            WeaponKind::WarHammer,
            WeaponKind::GreatSword,
            WeaponKind::Bow,
            WeaponKind::Musket,
            WeaponKind::Blunderbuss,
            WeaponKind::FireStaff,
            WeaponKind::LifeStaff,
            WeaponKind::IceGauntlet,
            WeaponKind::VoidGauntlet,
            WeaponKind::Other,
        ];
        for kind in kinds {
            for attack in [AttackKind::Light, AttackKind::Heavy] {
                assert_eq!(
                    swing_shape(kind, attack).is_some(),
                    kind.swings(),
                    "{kind:?} {attack:?}"
                );
            }
        }
        let reach = |attack| swing_shape(WeaponKind::GreatSword, attack).map(|shape| shape.reach_m);
        assert!(reach(AttackKind::Heavy) > reach(AttackKind::Light));
    }
}
