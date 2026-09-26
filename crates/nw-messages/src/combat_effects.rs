// SPDX-License-Identifier: AGPL-3.0-or-later
//! Combat effect pushes of the server (retail `HiveDynastiHealer`, `Hive catacomes`,
//! `dungeon_dynasty_tank_`, `arvaysa_ES_attacker_war_45vs45`,
//! `Hive_gorgon_raid_second_boss` ledgers).
//!
//! Every body starts with a facet header. After the header:
//!
//! - 2832 (hitscan effects, 77 bytes in every retail body): two positions (three big-endian
//!   `f32` each), one `u32` id (meaning Unknown), 16 bytes (meaning Unknown), the shooter's
//!   facet entity (`u64`), 8 bytes (meaning Unknown), a `u32` (0), a `u32` (`u32::MAX`), a
//!   `u64` and one byte (0). See [`HitScanEffects`].
//! - 1924: one big-endian `u64`, the attacker (a facet entity in retail).
//! - 2900, 38, 3034, 3031, 2863: the header alone.
//! - 2909: a 16-byte id, a `u64`, a second 16-byte id and a `u64` (meanings Unknown).
//! - 5322: two big-endian `u32` and two bytes (the last is 1 in retail; meanings Unknown).
//! - 2720: a position (three `f32`), a target `u64` (`u64::MAX` shifted into the high bytes
//!   when there is none, as retail sends it) and one result byte.
//! - 5269: a big-endian `u16` (0x0800 in retail) and a variable part kept whole (its inner
//!   layout is not fully traced; 10 bytes in retail).

use crate::facet::FacetHeader;
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// Type index of `ActionListComponentClientFacet_PlayHitScanEffects`.
pub const PLAY_HITSCAN_EFFECTS_TYPE_INDEX: u64 = 2832;
/// Type index of `DamageReceiverComponentClientFacet_DisplayImmuneText`.
pub const DISPLAY_IMMUNE_TEXT_TYPE_INDEX: u64 = 1924;
/// Type index of `ProjectileSpawnerComponentClientFacet_OnServerSpawn`.
pub const PROJECTILE_SERVER_SPAWN_TYPE_INDEX: u64 = 2900;
/// Type index of `AutoSpellComponentClientFacet_OnSpellCast`.
pub const AUTO_SPELL_CAST_TYPE_INDEX: u64 = 38;
/// Type index of `OffenseCollisionComponentClientFacet_OnDamageDamageable`.
pub const DAMAGE_DAMAGEABLE_TYPE_INDEX: u64 = 3034;
/// Type index of `SpellComponentClientFacet_ClientSpawnSiphon`.
pub const CLIENT_SPAWN_SIPHON_TYPE_INDEX: u64 = 2909;
/// Type index of `SlayerScriptClientFacet_SendEntityEvent`.
pub const SEND_ENTITY_EVENT_TYPE_INDEX: u64 = 5322;
/// Type index of `MomentaryOffenseComponentClientFacet_OnDamageDealt`.
pub const MOMENTARY_DAMAGE_DEALT_TYPE_INDEX: u64 = 3031;
/// Type index of `AutoSpellComponentClientFacet_OnLifetimeExpire`.
pub const AUTO_SPELL_LIFETIME_EXPIRE_TYPE_INDEX: u64 = 2863;
/// Type index of `TurretComponentClientFacet_OnHitscanServerResult`.
pub const TURRET_HITSCAN_RESULT_TYPE_INDEX: u64 = 2720;
/// Type index of `VitalsComponentClientFacet_NotifyDamageNumberBusOnTrueDamageReceived`.
pub const TRUE_DAMAGE_RECEIVED_TYPE_INDEX: u64 = 5269;

/// Bytes after the header of every retail 2832.
const HITSCAN_PAYLOAD_LEN: usize = 77;
/// The `u32` at offset 60 of every retail 2832.
const HITSCAN_ZERO: u32 = 0;
/// The `u32` at offset 64 of every retail 2832.
const HITSCAN_MAX: u32 = u32::MAX;
/// The last byte of every retail 2832.
const HITSCAN_LAST: u8 = 0;

/// 2832: the effect of an instant-hit shot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitScanEffects {
    /// Where the shot starts (metres).
    pub from: [f32; 3],
    /// Where the shot lands (metres).
    pub to: [f32; 3],
    /// An id after the positions (meaning Unknown).
    pub second_id: u32,
    /// Sixteen bytes of Unknown meaning.
    pub unknown_16: [u8; RAW_ID_LEN],
    /// The shooter's facet entity.
    pub shooter: u64,
    /// Eight bytes of Unknown meaning.
    pub unknown_8: [u8; 8],
    /// The `u64` before the last byte (meaning Unknown).
    pub unknown_u64: u64,
}

impl HitScanEffects {
    /// The effects of a shot of `shooter` (a facet entity) from `from` to `to`, with the
    /// unknown fields of the retail bodies.
    pub fn shot(shooter: u64, from: [f32; 3], to: [f32; 3]) -> Self {
        Self {
            from,
            to,
            second_id: 0,
            unknown_16: [0; RAW_ID_LEN],
            shooter,
            unknown_8: [0; 8],
            unknown_u64: 0,
        }
    }

    fn write(&self, writer: &mut Writer) {
        for value in self.from.iter().chain(&self.to) {
            writer.f32(*value);
        }
        writer
            .u32(self.second_id)
            .bytes(&self.unknown_16)
            .u64(self.shooter)
            .bytes(&self.unknown_8)
            .u32(HITSCAN_ZERO)
            .u32(HITSCAN_MAX)
            .u64(self.unknown_u64)
            .u8(HITSCAN_LAST);
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut position = || -> Result<[f32; 3], MessageError> {
            Ok([reader.f32()?, reader.f32()?, reader.f32()?])
        };
        let from = position()?;
        let to = position()?;
        Ok(Self {
            from,
            to,
            second_id: reader.u32()?,
            unknown_16: reader.raw_id()?,
            shooter: reader.u64()?,
            unknown_8: reader.array()?,
            unknown_u64: {
                reader.u32()?;
                reader.u32()?;
                let value = reader.u64()?;
                reader.u8()?;
                value
            },
        })
    }
}

/// A combat effect push of the server.
#[derive(Debug, Clone, PartialEq)]
pub enum CombatEffectMsg {
    /// 2832: play the effect of an instant-hit shot.
    HitScanEffects {
        /// The header.
        header: FacetHeader,
        /// The shot.
        effects: HitScanEffects,
    },
    /// 1924: show the Immune text; the `u64` is the attacker (a facet entity in retail).
    DisplayImmuneText {
        /// The header.
        header: FacetHeader,
        /// The attacker.
        attacker: u64,
    },
    /// 2900: the server spawned a projectile.
    ProjectileServerSpawn {
        /// The header.
        header: FacetHeader,
    },
    /// 38: a placed thing cast its spell on its own.
    AutoSpellCast {
        /// The header.
        header: FacetHeader,
    },
    /// 3034: an attack hit a damageable thing.
    DamageDamageable {
        /// The header.
        header: FacetHeader,
    },
    /// 2909: start a siphon.
    ClientSpawnSiphon {
        /// The header.
        header: FacetHeader,
        /// A 16-byte id (meaning Unknown).
        first_id: [u8; RAW_ID_LEN],
        /// A value after it (meaning Unknown).
        first: u64,
        /// A second 16-byte id (meaning Unknown).
        second_id: [u8; RAW_ID_LEN],
        /// A value after it (meaning Unknown).
        second: u64,
    },
    /// 5322: an entity event of a script.
    SendEntityEvent {
        /// The header.
        header: FacetHeader,
        /// The event (meaning Unknown).
        event: u32,
        /// A second value (0 in retail; meaning Unknown).
        extra: u32,
        /// A byte (0 in retail; meaning Unknown).
        flag_a: u8,
        /// A byte (1 in retail; meaning Unknown).
        flag_b: u8,
    },
    /// 3031: damage the player just dealt.
    MomentaryDamageDealt {
        /// The header.
        header: FacetHeader,
    },
    /// 2863: a placed thing ran out of time.
    AutoSpellLifetimeExpire {
        /// The header.
        header: FacetHeader,
    },
    /// 2720: the result of a shot of a mounted weapon.
    TurretHitscanResult {
        /// The header.
        header: FacetHeader,
        /// Where the shot landed (metres).
        position: [f32; 3],
        /// The target (a facet entity; no target in retail is `0x0000_0000_ffff_ffff`).
        target: u64,
        /// The result byte (meaning Unknown).
        result: u8,
    },
    /// 5269: true damage the player took. The part after the `u16` is kept whole.
    TrueDamageReceived {
        /// The header.
        header: FacetHeader,
        /// The first value (0x0800 in retail; meaning Unknown).
        flags: u16,
        /// The bytes after the `u16` (kept whole; inner layout not fully traced).
        payload: Vec<u8>,
    },
}

impl CombatEffectMsg {
    /// Whether `type_index` is a combat effect push.
    pub const fn is_type(type_index: u64) -> bool {
        matches!(
            type_index,
            PLAY_HITSCAN_EFFECTS_TYPE_INDEX
                | DISPLAY_IMMUNE_TEXT_TYPE_INDEX
                | PROJECTILE_SERVER_SPAWN_TYPE_INDEX
                | AUTO_SPELL_CAST_TYPE_INDEX
                | DAMAGE_DAMAGEABLE_TYPE_INDEX
                | CLIENT_SPAWN_SIPHON_TYPE_INDEX
                | SEND_ENTITY_EVENT_TYPE_INDEX
                | MOMENTARY_DAMAGE_DEALT_TYPE_INDEX
                | AUTO_SPELL_LIFETIME_EXPIRE_TYPE_INDEX
                | TURRET_HITSCAN_RESULT_TYPE_INDEX
                | TRUE_DAMAGE_RECEIVED_TYPE_INDEX
        )
    }

    /// The type index of the push.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::HitScanEffects { .. } => PLAY_HITSCAN_EFFECTS_TYPE_INDEX,
            Self::DisplayImmuneText { .. } => DISPLAY_IMMUNE_TEXT_TYPE_INDEX,
            Self::ProjectileServerSpawn { .. } => PROJECTILE_SERVER_SPAWN_TYPE_INDEX,
            Self::AutoSpellCast { .. } => AUTO_SPELL_CAST_TYPE_INDEX,
            Self::DamageDamageable { .. } => DAMAGE_DAMAGEABLE_TYPE_INDEX,
            Self::ClientSpawnSiphon { .. } => CLIENT_SPAWN_SIPHON_TYPE_INDEX,
            Self::SendEntityEvent { .. } => SEND_ENTITY_EVENT_TYPE_INDEX,
            Self::MomentaryDamageDealt { .. } => MOMENTARY_DAMAGE_DEALT_TYPE_INDEX,
            Self::AutoSpellLifetimeExpire { .. } => AUTO_SPELL_LIFETIME_EXPIRE_TYPE_INDEX,
            Self::TurretHitscanResult { .. } => TURRET_HITSCAN_RESULT_TYPE_INDEX,
            Self::TrueDamageReceived { .. } => TRUE_DAMAGE_RECEIVED_TYPE_INDEX,
        }
    }

    /// The header of the push.
    pub const fn header(&self) -> FacetHeader {
        match self {
            Self::HitScanEffects { header, .. }
            | Self::DisplayImmuneText { header, .. }
            | Self::ProjectileServerSpawn { header }
            | Self::AutoSpellCast { header }
            | Self::DamageDamageable { header }
            | Self::ClientSpawnSiphon { header, .. }
            | Self::SendEntityEvent { header, .. }
            | Self::MomentaryDamageDealt { header }
            | Self::AutoSpellLifetimeExpire { header }
            | Self::TurretHitscanResult { header, .. }
            | Self::TrueDamageReceived { header, .. } => *header,
        }
    }

    /// Decodes the body of the push of `type_index`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not match its layout or `type_index` is not
    /// a combat effect push.
    pub fn decode(type_index: u64, body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let message = match type_index {
            PLAY_HITSCAN_EFFECTS_TYPE_INDEX => Self::HitScanEffects {
                header,
                effects: HitScanEffects::read(&mut reader)?,
            },
            DISPLAY_IMMUNE_TEXT_TYPE_INDEX => Self::DisplayImmuneText {
                header,
                attacker: reader.u64()?,
            },
            PROJECTILE_SERVER_SPAWN_TYPE_INDEX => Self::ProjectileServerSpawn { header },
            AUTO_SPELL_CAST_TYPE_INDEX => Self::AutoSpellCast { header },
            DAMAGE_DAMAGEABLE_TYPE_INDEX => Self::DamageDamageable { header },
            CLIENT_SPAWN_SIPHON_TYPE_INDEX => Self::ClientSpawnSiphon {
                header,
                first_id: reader.raw_id()?,
                first: reader.u64()?,
                second_id: reader.raw_id()?,
                second: reader.u64()?,
            },
            SEND_ENTITY_EVENT_TYPE_INDEX => Self::SendEntityEvent {
                header,
                event: reader.u32()?,
                extra: reader.u32()?,
                flag_a: reader.u8()?,
                flag_b: reader.u8()?,
            },
            MOMENTARY_DAMAGE_DEALT_TYPE_INDEX => Self::MomentaryDamageDealt { header },
            AUTO_SPELL_LIFETIME_EXPIRE_TYPE_INDEX => Self::AutoSpellLifetimeExpire { header },
            TURRET_HITSCAN_RESULT_TYPE_INDEX => Self::TurretHitscanResult {
                header,
                position: [reader.f32()?, reader.f32()?, reader.f32()?],
                target: reader.u64()?,
                result: reader.u8()?,
            },
            TRUE_DAMAGE_RECEIVED_TYPE_INDEX => Self::TrueDamageReceived {
                header,
                flags: reader.u16()?,
                payload: reader.rest().to_vec(),
            },
            _ => {
                return Err(MessageError::BadTag {
                    field: "combat effect",
                    tag: u8::try_from(type_index).unwrap_or(u8::MAX),
                });
            }
        };
        match &message {
            Self::TrueDamageReceived { .. } => {}
            _ => reader.finish()?,
        }
        Ok(message)
    }

    /// The body bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        self.header().write(&mut writer);
        match self {
            Self::HitScanEffects { effects, .. } => effects.write(&mut writer),
            Self::DisplayImmuneText { attacker, .. } => {
                writer.u64(*attacker);
            }
            Self::ProjectileServerSpawn { .. }
            | Self::AutoSpellCast { .. }
            | Self::DamageDamageable { .. }
            | Self::MomentaryDamageDealt { .. }
            | Self::AutoSpellLifetimeExpire { .. } => {}
            Self::ClientSpawnSiphon {
                first_id,
                first,
                second_id,
                second,
                ..
            } => {
                writer
                    .bytes(first_id)
                    .u64(*first)
                    .bytes(second_id)
                    .u64(*second);
            }
            Self::SendEntityEvent {
                event,
                extra,
                flag_a,
                flag_b,
                ..
            } => {
                writer.u32(*event).u32(*extra).u8(*flag_a).u8(*flag_b);
            }
            Self::TurretHitscanResult {
                position,
                target,
                result,
                ..
            } => {
                for value in position {
                    writer.f32(*value);
                }
                writer.u64(*target).u8(*result);
            }
            Self::TrueDamageReceived { flags, payload, .. } => {
                writer.u16(*flags).bytes(payload);
            }
        }
        writer.finish()
    }
}

/// The retail payload length check of a 2832 (used by the tests).
pub const HITSCAN_BODY_LEN: usize = HITSCAN_PAYLOAD_LEN + 16;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::tests::hex;

    /// Retail `HiveDynastiHealer`: a hitscan effect, an immune text, a projectile spawn and a
    /// damage-damageable, each with its header.
    const HITSCAN: &str = "e2507c6d8633e1c4e20dadacacb6ac3645a714464583e680423592b94583d1e1423f37411923ca205af2839ccc74a944f3144ee35ec8a83366183964e34e14f344a974cc56c4c3331515667300000000ffffffff309079755300000000";
    const IMMUNE: &str = "b01294861fa0348de34e14f344a974cc2ebceaedd57c7b16";
    const SPAWN: &str = "fcf79ce85d358f6057c6de6488f3b132";
    const DAMAGEABLE: &str = "ef5e51a5066ff189fa8cac518ff0018c";
    /// Retail `Hive catacomes`: an entity event and a momentary damage.
    const EVENT: &str = "d031a70a26b37b7e17dac059d4b3c078fab6c9fe000000000001";
    const MOMENTARY: &str = "acff22485b37b72341e2606d20968b32";
    /// Retail `Hive HEALER ABILITYS`: a siphon and an auto spell cast.
    const SIPHON: &str = "ba802e4517d07334d75ad10de51c96527076546a45b286a635b1a4ecbc63725aa686b2456a5476708899ac23a7a5049f4e21845eebf40a679f04a5a723ac9988";
    const AUTO_CAST: &str = "c5ef88eb137436f7f6fd4df123504ade";
    /// Retail war: a turret result with no target and one with a target.
    const TURRET_NONE: &str =
        "b7e7192ce31a9c7d4cd31e6478d6829e45baa6d145a71f6a4262d8c000000000ffffffffff";
    const TURRET_HIT: &str =
        "b7e7192ce31a9c7d4cd31e6478d6829e45ba79a145a736b642679668a944a4750a5b26ec04";
    /// Retail `Hive_gorgon_raid_second_boss`: true damage.
    const TRUE_DAMAGE: &str = "ced4d31dedf93e30e34e14f344a974cc0800010443acf7983f800000";

    #[test]
    fn the_retail_bodies_read_and_write_back() {
        for (type_index, body) in [
            (PLAY_HITSCAN_EFFECTS_TYPE_INDEX, HITSCAN),
            (DISPLAY_IMMUNE_TEXT_TYPE_INDEX, IMMUNE),
            (PROJECTILE_SERVER_SPAWN_TYPE_INDEX, SPAWN),
            (DAMAGE_DAMAGEABLE_TYPE_INDEX, DAMAGEABLE),
            (SEND_ENTITY_EVENT_TYPE_INDEX, EVENT),
            (MOMENTARY_DAMAGE_DEALT_TYPE_INDEX, MOMENTARY),
            (CLIENT_SPAWN_SIPHON_TYPE_INDEX, SIPHON),
            (AUTO_SPELL_CAST_TYPE_INDEX, AUTO_CAST),
            (TURRET_HITSCAN_RESULT_TYPE_INDEX, TURRET_NONE),
            (TURRET_HITSCAN_RESULT_TYPE_INDEX, TURRET_HIT),
            (TRUE_DAMAGE_RECEIVED_TYPE_INDEX, TRUE_DAMAGE),
        ] {
            let push = CombatEffectMsg::decode(type_index, &hex(body)).expect("reads");
            assert_eq!(push.type_index(), type_index);
            assert!(CombatEffectMsg::is_type(type_index));
            assert_eq!(push.encode(), hex(body), "{type_index}");
        }
        assert_eq!(hex(HITSCAN).len(), HITSCAN_BODY_LEN);
        let CombatEffectMsg::HitScanEffects { effects, .. } =
            CombatEffectMsg::decode(PLAY_HITSCAN_EFFECTS_TYPE_INDEX, &hex(HITSCAN)).expect("reads")
        else {
            panic!("hitscan");
        };
        assert_eq!(effects.shooter, 0xe34e_14f3_44a9_74cc);
        assert!(effects.from[0] > 5_000.0 && effects.to[0] > 4_000.0);
        let CombatEffectMsg::SendEntityEvent { event, flag_b, .. } =
            CombatEffectMsg::decode(SEND_ENTITY_EVENT_TYPE_INDEX, &hex(EVENT)).expect("reads")
        else {
            panic!("event");
        };
        assert_eq!((event, flag_b), (0xfab6_c9fe, 1));
    }

    #[test]
    fn a_shot_builds_a_retail_shaped_body() {
        let header = FacetHeader {
            target: 3,
            entity: 4,
        };
        let push = CombatEffectMsg::HitScanEffects {
            header,
            effects: HitScanEffects::shot(4, [1.0, 2.0, 3.0], [4.0, 5.0, 6.0]),
        };
        let body = push.encode();
        assert_eq!(body.len(), HITSCAN_BODY_LEN);
        assert_eq!(
            CombatEffectMsg::decode(PLAY_HITSCAN_EFFECTS_TYPE_INDEX, &body),
            Ok(push)
        );
    }

    #[test]
    fn other_types_and_short_bodies_are_refused() {
        assert!(matches!(
            CombatEffectMsg::decode(1, &hex(IMMUNE)),
            Err(MessageError::BadTag { .. })
        ));
        assert!(CombatEffectMsg::decode(DISPLAY_IMMUNE_TEXT_TYPE_INDEX, &hex(SPAWN)).is_err());
    }
}
