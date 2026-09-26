// SPDX-License-Identifier: AGPL-3.0-or-later
//! What the server tells a client about the ground and the water under its own player: the own
//! ALC state's slot 1 children `distGround` and `waterDepth` (run-15 movement study,
//! 2026-09-22). The client's action lists read them as `HeightToGround` and `IfWaterDepth`: the
//! swim, the wade, the jump and the dodge depend on them. Retail sends each one in an own state
//! when its value changes (newchar tutorial: `waterDepth` 1 and then 2 as the player fell into
//! the sea, 1 and then 0 on the way out at the beach; `distGround` 193 to 195 while swimming
//! 7.6 m over the seabed, falling to 0 to 3 as the seabed rose under the feet). Our server sent
//! them only in the spawn state, 0 and 0, and its client never swam: it stood and walked at
//! the swim height, half out of the water.

use crate::movement::{Ground, Position};

/// The feet are at least this deep under the water for very shallow water, m (Observed: the
/// feet depth under the level's water surface at retail's `waterDepth` changes between 0 and 1
/// in 13 open-world captures, 163 changes: 0.20 m splits them with 8 on the wrong side, each a
/// state whose place lagged the change).
pub const VERY_SHALLOW_DEPTH_M: f32 = 0.20;
/// The feet are at least this deep for shallow water, m. Retail's way out of the tutorial beach
/// sent `waterDepth` 2 at 0.874 m and 1 at 0.842 m (newchar 2026-09-18). The wade used by
/// movement is deeper than this byte.
pub const SHALLOW_DEPTH_M: f32 = 0.874;
/// The farthest ground distance the state holds, m (`FUN_142a2bc70` reads `distGround` as a
/// byte over 0 to 10 m: range maximum `0x147f41134` 10.0, step `0x147f41088` 1/255).
pub const GROUND_DISTANCE_MAX_M: f32 = 10.0;
/// The steps of the ground distance byte.
const GROUND_DISTANCE_STEPS: f32 = 255.0;

/// How deep the water at the feet is (the client's `WaterDepth` enum, `FUN_142c55fb0`: `None` 0,
/// `VeryShallow` 1, `Shallow` 2, `Deep` 3). Retail's own states carry 0 to 2 only: 2 from
/// [`SHALLOW_DEPTH_M`] down, in the sea while swimming and while wading alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WaterDepth {
    /// No water, or less than [`VERY_SHALLOW_DEPTH_M`] over the feet.
    #[default]
    None,
    /// From [`VERY_SHALLOW_DEPTH_M`] to [`SHALLOW_DEPTH_M`].
    VeryShallow,
    /// From [`SHALLOW_DEPTH_M`] down.
    Shallow,
}

impl WaterDepth {
    /// The depth of water `feet_depth_m` metres over the feet.
    pub fn of(feet_depth_m: f32) -> Self {
        if feet_depth_m >= SHALLOW_DEPTH_M {
            Self::Shallow
        } else if feet_depth_m >= VERY_SHALLOW_DEPTH_M {
            Self::VeryShallow
        } else {
            Self::None
        }
    }

    /// The `waterDepth` byte.
    pub const fn wire(self) -> u8 {
        match self {
            Self::None => 0,
            Self::VeryShallow => 1,
            Self::Shallow => 2,
        }
    }
}

/// The ground and the water under a body's feet, as the own state carries them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Footing {
    /// The `distGround` byte: the height of the feet over the floor under them, in 1/255 of
    /// [`GROUND_DISTANCE_MAX_M`]; 255 for no floor within it.
    pub ground_distance: u8,
    /// The `waterDepth` byte's depth.
    pub water: WaterDepth,
}

impl Footing {
    /// What the spawn state carries: both children 0 (retail's most common first own state).
    pub const SPAWN: Self = Self {
        ground_distance: 0,
        water: WaterDepth::None,
    };

    /// The footing of a body whose feet are at `at` on `ground`, with the sea at `sea_level_m`:
    /// the floor under it is the ground's, not the water (retail: 193 swimming 7.57 m over the
    /// seabed), and the water is the local water's ([`Ground::water_surface`]).
    pub fn of(ground: &(impl Ground + ?Sized), at: Position, sea_level_m: f32) -> Self {
        let ground_distance = ground.floor_at(at.x, at.y, at.z).map_or(u8::MAX, |floor| {
            let metres = (at.z - floor).clamp(0.0, GROUND_DISTANCE_MAX_M);
            let steps = (metres / GROUND_DISTANCE_MAX_M * GROUND_DISTANCE_STEPS).round();
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped to 0 to 255 steps"
            )]
            let byte = steps as u8;
            byte
        });
        let water = ground
            .water_surface(at.x, at.y, sea_level_m)
            .map_or(WaterDepth::None, |surface| WaterDepth::of(surface - at.z));
        Self {
            ground_distance,
            water,
        }
    }

    /// What changed from `sent`, the footing the client has: the ground distance and the water
    /// depth bytes when they differ.
    pub fn change_since(self, sent: Self) -> (Option<u8>, Option<u8>) {
        (
            (self.ground_distance != sent.ground_distance).then_some(self.ground_distance),
            (self.water != sent.water).then_some(self.water.wire()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tutorial sea (`ftue_v2` ocean level), m.
    const TUTORIAL_SEA_M: f32 = 20.0;

    /// Flat ground at the height it holds.
    struct Seabed(f32);

    impl Ground for Seabed {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            Some(self.0)
        }
    }

    /// No floor anywhere.
    struct Void;

    impl Ground for Void {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            None
        }
    }

    fn feet(z: f32) -> Position {
        Position {
            x: 793.372,
            y: 338.046,
            z,
        }
    }

    #[test]
    fn the_retail_fall_into_the_tutorial_sea_gives_retail_s_bytes() {
        // Newchar retail 2026-09-18, the fall into the sea at the wreck: the seabed at 10.92 m
        // (replay grid) and the own states' places and slot 1 bytes.
        const SEABED_M: f32 = 10.92;
        let ground = Seabed(SEABED_M);
        // The first state in the water lagged its place: 1.10 m deep it still said 1.
        let cases = [
            (19.290, None, Some(WaterDepth::VeryShallow)),
            (18.898, Some(203), None),
            (18.490, Some(193), Some(WaterDepth::Shallow)),
            (18.538, Some(194), Some(WaterDepth::Shallow)),
        ];
        for (z, distance, water) in cases {
            let footing = Footing::of(&ground, feet(z), TUTORIAL_SEA_M);
            if let Some(distance) = distance {
                assert_eq!(footing.ground_distance, distance, "{z}");
            }
            if let Some(water) = water {
                assert_eq!(footing.water, water, "{z}");
            }
        }
    }

    #[test]
    fn the_retail_way_out_at_the_beach_steps_down_the_depths() {
        // The same capture on the way out: 2 at a feet depth of 0.874 m, 1 at 0.842 m, 0 out of
        // the water.
        assert_eq!(WaterDepth::of(0.874), WaterDepth::Shallow);
        assert_eq!(WaterDepth::of(0.842), WaterDepth::VeryShallow);
        assert_eq!(WaterDepth::of(0.126), WaterDepth::None);
        assert_eq!(WaterDepth::of(-1.0), WaterDepth::None);
        assert_eq!(
            [
                WaterDepth::None,
                WaterDepth::VeryShallow,
                WaterDepth::Shallow
            ]
            .map(WaterDepth::wire),
            [0, 1, 2]
        );
    }

    #[test]
    fn a_body_on_its_floor_is_0_and_one_with_none_under_it_is_255() {
        let on_floor = Footing::of(&Seabed(89.38), feet(89.38), TUTORIAL_SEA_M);
        assert_eq!(
            on_floor,
            Footing::SPAWN,
            "the deck: what the spawn state says"
        );
        let high = Footing::of(&Seabed(10.92), feet(26.16), TUTORIAL_SEA_M);
        assert_eq!(high.ground_distance, u8::MAX, "over 10 m");
        assert_eq!(
            Footing::of(&Void, feet(5.0), TUTORIAL_SEA_M).ground_distance,
            u8::MAX
        );
    }

    #[test]
    fn only_what_changed_goes_out() {
        let swimming = Footing::of(&Seabed(10.92), feet(18.49), TUTORIAL_SEA_M);
        assert_eq!(swimming.change_since(Footing::SPAWN), (Some(193), Some(2)));
        assert_eq!(swimming.change_since(swimming), (None, None));
        let deeper = Footing {
            ground_distance: 194,
            ..swimming
        };
        assert_eq!(deeper.change_since(swimming), (Some(194), None));
    }
}
