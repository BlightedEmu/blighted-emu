// SPDX-License-Identifier: AGPL-3.0-or-later
//! The random numbers of the tree nodes.
//!
//! Observed: the UtilitySelector weighted draw (`FUN_1426355b0`) steps a 48-bit linear
//! congruential generator, `state = state * 0x5DEECE66D + 0xB mod 2^48`, and makes a float in
//! `[0, 1)` from 23 bits of it: `((state >> 16) & 0x7FFFFF | 0x3F800000) as f32 - 1.0`. The runtime
//! keeps one such generator per mob. Which generator the cooldown and limiter ranges use is
//! **Unknown**; they use the same one here.

/// `drand48` multiplier.
const MULTIPLIER: u64 = 0x5_DEEC_E66D;
/// `drand48` increment.
const INCREMENT: u64 = 0xB;
/// The state has 48 bits.
const STATE_MASK: u64 = (1 << 48) - 1;
/// The bits of the state that become the float.
const OUTPUT_SHIFT: u32 = 16;
const MANTISSA_MASK: u64 = 0x7F_FFFF;
/// The bits of 1.0f.
const ONE_BITS: u32 = 0x3F80_0000;

/// One mob's random number generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rng48 {
    state: u64,
}

impl Rng48 {
    /// A generator that starts at `seed`.
    pub const fn new(seed: u64) -> Self {
        Self {
            state: seed & STATE_MASK,
        }
    }

    /// The next float in `[0, 1)`.
    pub fn unit(&mut self) -> f32 {
        self.state = (self.state.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT)) & STATE_MASK;
        // 23 bits of the state fill the mantissa of a float in [1, 2).
        let mantissa = ((self.state >> OUTPUT_SHIFT) & MANTISSA_MASK) as u32;
        f32::from_bits(mantissa | ONE_BITS) - 1.0
    }

    /// A value between `min` and `max` (either order).
    pub fn range(&mut self, min: f64, max: f64) -> f64 {
        let (low, high) = if min <= max { (min, max) } else { (max, min) };
        low + f64::from(self.unit()) * (high - low)
    }

    /// A whole number from `min` to `max`, both included.
    pub fn whole(&mut self, min: i64, max: i64) -> i64 {
        let (low, high) = if min <= max { (min, max) } else { (max, min) };
        let span = (high - low + 1) as f64;
        // unit() is below 1, so the product is below span.
        low + (f64::from(self.unit()) * span) as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sequence_is_drand48() {
        let mut rng = Rng48::new(0);
        rng.unit();
        assert_eq!(rng.state, INCREMENT);
        rng.unit();
        assert_eq!(rng.state, (INCREMENT * MULTIPLIER + INCREMENT) & STATE_MASK);
    }

    #[test]
    fn values_stay_in_range() {
        let mut rng = Rng48::new(42);
        for _ in 0..10_000 {
            let unit = rng.unit();
            assert!((0.0..1.0).contains(&unit));
            let value = rng.range(2.0, 3.0);
            assert!((2.0..=3.0).contains(&value));
            assert!((2..=3).contains(&rng.whole(3, 2)));
        }
    }
}
