// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::ProjectileReplicatedState` (class 16): the state of a projectile entity, the slice a
//! shot spawns (an arrow, a musket ball, a staff's or gauntlet's bolt, a thrown hatchet).
//! Layout from the client's member table; the children Observed in the retail shots
//! (projectile study 2026-11-11, 203,948 projectiles of 670 ledgers): the card of a fresh shot
//! holds the spawn point, the velocity, the owner, the ranged attack, the damage table and its
//! row (and the weapon effect for a bow, a musket and a thrown weapon); the time anchor comes
//! about 66 ms later; a hit sends the impact point, the target, its hit volume and the flags.
//!
//! The sim writes this state; the captures' own states read through the member table, which
//! also holds the children the sim does not send (the piercing hits, the magnet target).

use crate::position::WorldPosition;
use crate::slots::{children_mask, write_known};
use crate::wire::{MessageError, Writer};

/// Type index of `MB::ProjectileReplicatedState`.
pub const PROJECTILE_TYPE_INDEX: u64 = 16;
/// CRC-32 of `damagetable`: the `damageTableId` of every player shot.
pub const PLAYER_DAMAGE_TABLE: u32 = 0x22e1_6917;
/// CRC-32 of `default`: the `weaponEffectCRC` of a weapon with no gem.
pub const DEFAULT_WEAPON_EFFECT: u32 = 0xe35e_00df;
/// The `flags` of a projectile that hit (an entity or the world).
pub const COLLIDED_FLAGS: u8 = 0x01;
/// The children of the class.
const CHILDREN: u8 = 20;

/// The state of a projectile entity. A `None` child is not sent.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ProjectileState {
    /// Child 0, `positionAnchor`: where it spawned, then where it hit.
    pub position: Option<WorldPosition>,
    /// Child 2, `spawnVelocity`, m/s: its direction times its slice's `m_baseSpeed`.
    pub velocity: Option<[f32; 3]>,
    /// Child 4, `timeAnchorMicroseconds`: microseconds since 2000-01-01 (Inferred; which
    /// clock is Unknown).
    pub time_anchor_us: Option<u64>,
    /// Child 7, `collidedWithGDEId`: the facet entity of what it hit.
    pub collided_with: Option<u64>,
    /// Child 8, `collidedWithHitVolumeIndex`.
    pub hit_volume: Option<u8>,
    /// Child 9, `flags` ([`COLLIDED_FLAGS`] after a hit).
    pub flags: Option<u8>,
    /// Child 10, `owner`: the shooter's facet entity.
    pub owner: Option<u64>,
    /// Child 11, `rangedAttackIndex`: the flat index of the attack in the ranged attack
    /// database, in stream order.
    pub ranged_attack: Option<u64>,
    /// Child 12, `weaponEffectCRC`: CRC-32 of the weapon's damage effect name.
    pub weapon_effect: Option<u32>,
    /// Child 14, `damageTableId`: CRC-32 of the lowercase damage table name.
    pub damage_table: Option<u32>,
    /// Child 15, `damageTableRowIndex`: the row of the hit in its damage table.
    pub damage_row: Option<u16>,
}

/// The bits of a half float.
const HALF_SIGN: u16 = 0x8000;
const HALF_EXPONENT_BIAS: i32 = 15;
const HALF_EXPONENT_MAX: i32 = 31;
const HALF_MANTISSA_BITS: u32 = 10;
const HALF_INFINITY: u16 = 0x7c00;
/// The bits of a single float.
const SINGLE_EXPONENT_BIAS: i32 = 127;
const SINGLE_EXPONENT_MASK: u32 = 0xff;
const SINGLE_MANTISSA_BITS: u32 = 23;
const SINGLE_MANTISSA_MASK: u32 = 0x007f_ffff;

/// The half float nearest to `value` (a velocity of a few hundred m/s; a value beyond the
/// half range is its infinity, one below it is 0).
fn half_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = if value.is_sign_negative() {
        HALF_SIGN
    } else {
        0
    };
    // The exponent field is 8 bits.
    let exponent = ((bits >> SINGLE_MANTISSA_BITS) & SINGLE_EXPONENT_MASK) as i32
        - SINGLE_EXPONENT_BIAS
        + HALF_EXPONENT_BIAS;
    if exponent >= HALF_EXPONENT_MAX {
        return sign | HALF_INFINITY;
    }
    if exponent <= 0 {
        return sign;
    }
    let shift = SINGLE_MANTISSA_BITS - HALF_MANTISSA_BITS;
    let mantissa = bits & SINGLE_MANTISSA_MASK;
    let rounded = (mantissa + (1 << (shift - 1))) >> shift;
    // The exponent is 1 to 30 and the rounded mantissa at most 1 << 10: the sum carries into
    // the exponent, which stays in 16 bits.
    let magnitude = ((exponent as u32) << HALF_MANTISSA_BITS) + rounded;
    sign | magnitude as u16
}

impl ProjectileState {
    /// Writes the state for the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Prefix`] when the ranged attack index does not fit its code.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let ranged_attack = self
            .ranged_attack
            .map(|index| {
                let mut coded = Writer::new();
                coded.prefixed(index)?;
                Ok::<_, MessageError>(coded.finish())
            })
            .transpose()?;
        let mask = children_mask(&[
            self.position.is_some(),
            false,
            self.velocity.is_some(),
            false,
            self.time_anchor_us.is_some(),
            false,
            false,
            self.collided_with.is_some(),
            self.hit_volume.is_some(),
            self.flags.is_some(),
            self.owner.is_some(),
            ranged_attack.is_some(),
            self.weapon_effect.is_some(),
            false,
            self.damage_table.is_some(),
            self.damage_row.is_some(),
        ]);
        write_known(writer, mask, CHILDREN, |writer, child| match child {
            0 => drop(
                self.position
                    .map(|at| writer.f32(at.x).f32(at.y).u16(at.z_raw)),
            ),
            2 => {
                for axis in self.velocity.unwrap_or_default() {
                    writer.u16(half_bits(axis));
                }
            }
            4 => drop(self.time_anchor_us.map(|time| writer.u64(time))),
            7 => drop(self.collided_with.map(|target| writer.u64(target))),
            8 => drop(self.hit_volume.map(|volume| writer.u8(volume))),
            9 => drop(self.flags.map(|flags| writer.u8(flags))),
            10 => drop(self.owner.map(|owner| writer.u64(owner))),
            11 => drop(ranged_attack.as_deref().map(|coded| writer.bytes(coded))),
            12 => drop(self.weapon_effect.map(|effect| writer.u32(effect))),
            14 => drop(self.damage_table.map(|table| writer.u32(table))),
            _ => drop(self.damage_row.map(|row| writer.u16(row))),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up shooter facet entity and target facet entity.
    const SHOOTER: u64 = 0x8070_6050_4030_2010;
    const TARGET: u64 = 0x0102_0304_0506_0708;

    fn hex(state: &ProjectileState) -> String {
        let mut writer = Writer::new();
        state.write(&mut writer).expect("the state writes");
        writer
            .finish()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn at(x: u32, y: u32, z_raw: u16) -> WorldPosition {
        WorldPosition {
            x: f32::from_bits(x),
            y: f32::from_bits(y),
            z_raw,
        }
    }

    /// Retail `Hive_bow_and_spear_in_duels` at 30,450 ms: the card state of an arrow (ranged
    /// attack 255 `Bow/PenetratingShot`, row 188), with a made-up owner.
    #[test]
    fn the_card_state_of_a_retail_arrow_writes_the_same_bytes() {
        let arrow = ProjectileState {
            position: Some(at(0x45e5_4e7d, 0x45a6_5f65, 0x1597)),
            velocity: Some([38.97, 92.12, 0.012_28]),
            owner: Some(SHOOTER),
            ranged_attack: Some(255),
            weapon_effect: Some(DEFAULT_WEAPON_EFFECT),
            damage_table: Some(PLAYER_DAMAGE_TABLE),
            damage_row: Some(188),
            ..ProjectileState::default()
        };
        assert_eq!(
            hex(&arrow),
            "018545e54e7d45a65f65159750df55c2224ab88070605040302010bf03e35e00df0322e1691700bc"
        );
    }

    /// Retail `EXP-Empyrian-Forge-M1` at 2,093,182 ms: a fire staff bolt's time anchor and its
    /// hit on an entity (made-up target).
    #[test]
    fn the_time_anchor_and_the_hit_of_a_retail_bolt_write_the_same_bytes() {
        let anchor = ProjectileState {
            time_anchor_us: Some(0x0003_011a_423d_0af8),
            ..ProjectileState::default()
        };
        assert_eq!(hex(&anchor), "01100003011a423d0af8");
        let hit = ProjectileState {
            position: Some(at(0x445b_95aa, 0x4400_773b, 0x26f6)),
            collided_with: Some(TARGET),
            hit_volume: Some(9),
            flags: Some(COLLIDED_FLAGS),
            ..ProjectileState::default()
        };
        assert_eq!(hex(&hit), "0181445b95aa4400773b26f60701020304050607080901");
        let world_hit = ProjectileState {
            position: Some(at(0x45e6_6ec8, 0x45a9_08ae, 0x1561)),
            flags: Some(COLLIDED_FLAGS),
            ..ProjectileState::default()
        };
        assert_eq!(hex(&world_hit), "018145e66ec845a908ae15610401");
    }

    #[test]
    fn a_half_float_is_the_nearest_to_its_value() {
        assert_eq!(half_bits(1.0), 0x3c00);
        assert_eq!(half_bits(-2.0), 0xc000);
        assert_eq!(half_bits(0.0), 0);
        assert_eq!(half_bits(100.0), 0x5640);
        assert_eq!(half_bits(1.0e9), HALF_INFINITY);
        assert_eq!(half_bits(1.0e-9), 0);
    }
}
