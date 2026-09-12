// SPDX-License-Identifier: AGPL-3.0-or-later
//! Grounds for tests (feature `test-util`).

use crate::movement::Ground;

/// A level without height data: every ground check is skipped.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoGround;

impl Ground for NoGround {
    fn height_at(&self, _: f32, _: f32) -> Option<f32> {
        None
    }
}

/// A level with flat ground at one height everywhere.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlatGround(pub f32);

impl Ground for FlatGround {
    fn height_at(&self, _: f32, _: f32) -> Option<f32> {
        Some(self.0)
    }
}
