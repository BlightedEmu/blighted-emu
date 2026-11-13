// SPDX-License-Identifier: AGPL-3.0-or-later
//! The calls of a player who uses a siege turret (`PlayerTurretComponentServerFacet`), and the
//! projectile spawner state of a turret (turret study 2026-11-12: every call of the capture
//! index; ledgers `RS_offensive_war` (a cannon), `joining_defending_war_2` (a repeater),
//! `EXP-Barnacles-and-Black-Powder`, `Hive Sheeds Revenge short event`, `07-Tempests_Heart`,
//! `M2-TEMPEST-HEART`, `20261002-184508-050`).
//!
//! Every call starts with a facet header: the player's turret component and the player's
//! entity. After the header comes the [`TurretAim`]:
//!
//! - the turret's `GdeRef` (its 16-byte raw id and its facet entity, big-endian),
//! - the facet target of the turret's `TurretComponent` (the 2720 header names the same),
//! - the time of the input: a synced clock in nanoseconds (Observed: the first four bytes are
//!   those of the ledger's 335 time sync, and the value rises about 35 ms a call),
//! - the yaw and the pitch, `f32` radians from the turret's facing (Observed,
//!   `joining_defending_war_2` 195.4 to 199.3 s: the 2720 hit point lies on the heading of the
//!   turret's yaw plus the call's yaw, 12 of 12 shots; a negative pitch looks down, the hits fell
//!   on the ground 55 to 92 m from a turret 12 m above it).
//!
//! Then:
//!
//! - 4273 `ReceiveInput` (2,653 calls): one byte (Observed: 0 in every call).
//! - 4274 `FireProjectile` (25 calls): two bytes (Observed: 0 and 1 in every call).
//! - 4145 `ReceiveRepeaterInput` (3,238 calls): the [`RepeaterFlags`] byte and the client's
//!   frame time in seconds (Observed: `1/30` in every call).
//!
//! The client sends no call to leave: the interaction ends ([`crate::facet`] 2473) or the
//! client stops naming it in its input.
//!
//! The build call of a fort's defensive structure (2646
//! `BuilderComponentServerFacet_RequestBuildDefensiveStructure`, 17 calls, all in
//! `fort-equipements-territory`) is [`BuildDefensiveStructureMsg`]: the header (the player's
//! builder component and entity), the structure's `GdeRef` (its raw id and its facet entity,
//! as in [`TurretAim`]), a facet target (meaning Unknown: it differs on each structure and is not
//! the facet rule of any of its slice's components that this table holds), and the choice, a
//! big-endian `u32` (Observed 0 to 4: the ballista, the repeater, the explosive launcher, the
//! fire barrel dropper and the horn of resilience of the socket's tier). The client sends it
//! after the server accepted its interaction of kind 62 (`OpenTurretSelection`) on the structure,
//! and cancels the interaction (2473) about 235 ms later; retail sent no answer to any of the 17.

use crate::facet::FacetHeader;
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// Type index of `PlayerTurretComponentServerFacet_ReceiveInput`.
pub const TURRET_INPUT_TYPE_INDEX: u64 = 4273;
/// Type index of `PlayerTurretComponentServerFacet_FireProjectile`.
pub const TURRET_FIRE_PROJECTILE_TYPE_INDEX: u64 = 4274;
/// Type index of `PlayerTurretComponentServerFacet_ReceiveRepeaterInput`.
pub const TURRET_REPEATER_INPUT_TYPE_INDEX: u64 = 4145;
/// Type index of `MB::ProjectileSpawnerReplicatedState`.
pub const PROJECTILE_SPAWNER_TYPE_INDEX: u64 = 57;
/// Type index of `MB::ItemGenerationComponentReplicatedState` (a supply generator's).
pub const ITEM_GENERATION_TYPE_INDEX: u64 = 3002;
/// The children of an item generation state: `active` (u8) and `itemGenerationTime` (u64).
const GENERATION_ACTIVE_CHILD: u8 = 0x01;
const GENERATION_TIME_CHILD: u8 = 0x02;

/// The length of an entity's `GdeRef` in these calls: the raw id and the facet entity.
pub const TURRET_GDE_REF_LEN: usize = RAW_ID_LEN + size_of::<u64>();

/// The byte after the aim of every retail 4273 (Observed: 0).
pub const INPUT_TAIL: u8 = 0;
/// The two bytes after the aim of every retail 4274 (Observed: 0, then 1).
pub const FIRE_TAIL: [u8; 2] = [0, 1];

/// The `GdeRef` of entity number `number` in the wire form of these calls and of the turret's
/// interactor (Observed: the player's raw id, then its facet entity big-endian, in the 4276
/// interactor and the 4273 turret field).
pub fn gde_ref_of(raw_id: [u8; RAW_ID_LEN], facet_entity: u64) -> [u8; TURRET_GDE_REF_LEN] {
    let mut gde_ref = [0; TURRET_GDE_REF_LEN];
    gde_ref[..RAW_ID_LEN].copy_from_slice(&raw_id);
    gde_ref[RAW_ID_LEN..].copy_from_slice(&facet_entity.to_be_bytes());
    gde_ref
}

/// The aim of a turret call (module doc).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurretAim {
    /// The turret's `GdeRef`.
    pub turret: [u8; TURRET_GDE_REF_LEN],
    /// The facet target of the turret's `TurretComponent`.
    pub turret_facet: u64,
    /// The input's synced time, ns.
    pub time_ns: u64,
    /// The yaw from the turret's facing, radians.
    pub yaw: f32,
    /// The pitch, radians; negative looks down.
    pub pitch: f32,
}

impl TurretAim {
    /// The turret's raw id.
    pub fn turret_id(&self) -> [u8; RAW_ID_LEN] {
        let mut id = [0; RAW_ID_LEN];
        id.copy_from_slice(&self.turret[..RAW_ID_LEN]);
        id
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            turret: reader.array()?,
            turret_facet: reader.u64()?,
            time_ns: reader.u64()?,
            yaw: reader.f32()?,
            pitch: reader.f32()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer
            .bytes(&self.turret)
            .u64(self.turret_facet)
            .u64(self.time_ns)
            .f32(self.yaw)
            .f32(self.pitch);
    }
}

/// The flags byte of a 4145 (Observed in `joining_defending_war_2`: 7 on the first frame of a
/// pull of the trigger, 2 while it is held, 6 on each frame the client fired a shot, 0 while it
/// is up; each frame with [`RepeaterFlags::FIRED`] got one 2720 about 85 ms later).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepeaterFlags(pub u8);

impl RepeaterFlags {
    /// The trigger went down this frame.
    pub const PRESSED: u8 = 0x01;
    /// The trigger is held.
    pub const HELD: u8 = 0x02;
    /// The client fired a shot this frame.
    pub const FIRED: u8 = 0x04;

    /// Whether the trigger is held.
    pub const fn held(self) -> bool {
        self.0 & Self::HELD != 0
    }

    /// Whether the client fired a shot this frame.
    pub const fn fired(self) -> bool {
        self.0 & Self::FIRED != 0
    }
}

/// A turret call of the client.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TurretClientMsg {
    /// 4273: the aim of a turret that fires projectiles.
    Input {
        /// The header.
        header: FacetHeader,
        /// The aim.
        aim: TurretAim,
        /// The byte after it ([`INPUT_TAIL`]).
        tail: u8,
    },
    /// 4274: a shot of a turret that fires projectiles, along the aim.
    FireProjectile {
        /// The header.
        header: FacetHeader,
        /// The aim.
        aim: TurretAim,
        /// The bytes after it ([`FIRE_TAIL`]; meaning Unknown).
        tail: [u8; 2],
    },
    /// 4145: the aim and the trigger of a repeater.
    RepeaterInput {
        /// The header.
        header: FacetHeader,
        /// The aim.
        aim: TurretAim,
        /// The trigger.
        flags: RepeaterFlags,
        /// The client's frame time, seconds.
        frame_s: f32,
    },
}

impl TurretClientMsg {
    /// Decodes `body` when `type_index` is a turret call; `None` for another type.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        if !matches!(
            type_index,
            TURRET_INPUT_TYPE_INDEX
                | TURRET_FIRE_PROJECTILE_TYPE_INDEX
                | TURRET_REPEATER_INPUT_TYPE_INDEX
        ) {
            return None;
        }
        let mut reader = Reader::new(body);
        let read = |reader: &mut Reader<'_>| -> Result<Self, MessageError> {
            let header = FacetHeader::read(reader)?;
            let aim = TurretAim::read(reader)?;
            Ok(match type_index {
                TURRET_INPUT_TYPE_INDEX => Self::Input {
                    header,
                    aim,
                    tail: reader.u8()?,
                },
                TURRET_FIRE_PROJECTILE_TYPE_INDEX => Self::FireProjectile {
                    header,
                    aim,
                    tail: reader.array()?,
                },
                _ => Self::RepeaterInput {
                    header,
                    aim,
                    flags: RepeaterFlags(reader.u8()?),
                    frame_s: reader.f32()?,
                },
            })
        };
        Some(read(&mut reader).and_then(|call| reader.finish().map(|()| call)))
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::Input { .. } => TURRET_INPUT_TYPE_INDEX,
            Self::FireProjectile { .. } => TURRET_FIRE_PROJECTILE_TYPE_INDEX,
            Self::RepeaterInput { .. } => TURRET_REPEATER_INPUT_TYPE_INDEX,
        }
    }

    /// The aim of the call.
    pub const fn aim(&self) -> TurretAim {
        match self {
            Self::Input { aim, .. }
            | Self::FireProjectile { aim, .. }
            | Self::RepeaterInput { aim, .. } => *aim,
        }
    }

    /// The body bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Self::Input { header, aim, tail } => {
                header.write(&mut writer);
                aim.write(&mut writer);
                writer.u8(*tail);
            }
            Self::FireProjectile { header, aim, tail } => {
                header.write(&mut writer);
                aim.write(&mut writer);
                writer.bytes(tail);
            }
            Self::RepeaterInput {
                header,
                aim,
                flags,
                frame_s,
            } => {
                header.write(&mut writer);
                aim.write(&mut writer);
                writer.u8(flags.0).f32(*frame_s);
            }
        }
        writer.finish()
    }
}

/// Type index of `BuilderComponentServerFacet_RequestBuildDefensiveStructure`.
pub const BUILD_DEFENSIVE_STRUCTURE_TYPE_INDEX: u64 = 2646;

/// 2646: build the defensive structure of `choice` in the place of `structure` (module doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildDefensiveStructureMsg {
    /// The header.
    pub header: FacetHeader,
    /// The structure's `GdeRef`.
    pub structure: [u8; TURRET_GDE_REF_LEN],
    /// A facet target (meaning Unknown).
    pub facet: u64,
    /// The choice.
    pub choice: u32,
}

impl BuildDefensiveStructureMsg {
    /// Decodes `body` when `type_index` is 2646; `None` for another type.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        if type_index != BUILD_DEFENSIVE_STRUCTURE_TYPE_INDEX {
            return None;
        }
        let mut reader = Reader::new(body);
        let read = |reader: &mut Reader<'_>| -> Result<Self, MessageError> {
            Ok(Self {
                header: FacetHeader::read(reader)?,
                structure: reader.array()?,
                facet: reader.u64()?,
                choice: reader.u32()?,
            })
        };
        Some(read(&mut reader).and_then(|call| reader.finish().map(|()| call)))
    }

    /// The structure's raw id.
    pub fn structure_id(&self) -> [u8; RAW_ID_LEN] {
        let mut id = [0; RAW_ID_LEN];
        id.copy_from_slice(&self.structure[..RAW_ID_LEN]);
        id
    }

    /// The body bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        writer
            .bytes(&self.structure)
            .u64(self.facet)
            .u32(self.choice);
        writer.finish()
    }
}

/// The slot byte of a class with slot 0 only.
const SLOT_0: u8 = 0x01;
/// The children of the projectile spawner state.
const AMMO_CHILD: u8 = 0x01;
const BLOCKED_CHILD: u8 = 0x02;

/// `MB::ProjectileSpawnerReplicatedState` (class 57; layout from the client's member table:
/// `curAmmo` `u16`, `isFiringBlocked` byte). Observed: every retail turret card sends both (a
/// repeater 200 and false, `joining_defending_war_2`; a cannon 15 and false,
/// `RS_offensive_war`); each repeater shot lowers the ammo by one (195.509 s 200 -> 199); a
/// cannon shot lowers it and blocks firing (860.024 s), and 3.0 s later firing is free again
/// (863.057 s, the ammo not sent).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectileSpawnerState {
    /// `curAmmo`.
    pub ammo: Option<u16>,
    /// `isFiringBlocked`.
    pub firing_blocked: Option<bool>,
}

impl ProjectileSpawnerState {
    /// The member body (the slot byte, the children byte and the children).
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        let children =
            self.ammo.map_or(0, |_| AMMO_CHILD) | self.firing_blocked.map_or(0, |_| BLOCKED_CHILD);
        if children == 0 {
            writer.u8(0);
            return writer.finish();
        }
        writer.u8(SLOT_0).u8(children);
        if let Some(ammo) = self.ammo {
            writer.u16(ammo);
        }
        if let Some(blocked) = self.firing_blocked {
            writer.boolean(blocked);
        }
        writer.finish()
    }
}

/// The item generation state of a supply generator (class 3002; members `active` and
/// `itemGenerationTime`, persisted as `m_itemGenerationTime` and `m_itemGenerationCycleTime` by
/// `FUN_142e88010`). Observed bodies: `01 03 <active> <time>` (both children) and `01 02 <time>`
/// (the time alone), the time a big-endian u64 of nanoseconds since 2000-01-01 UTC (the first
/// generator of `joining_defending_war_2` 0.602 s held `0b97914c9f293993`, 1.1 s before the
/// ledger's first record at Unix 1781981916.737 s; its next times are 10.0 s apart, the
/// recipe's `Tickrate`, 980.336 -> 990.345 s).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ItemGenerationState {
    /// `active`: the generator makes items (Observed 0 while its container is full, 1 from the
    /// take that made room, `joining_defending_war_2` 980.336 s).
    pub active: Option<bool>,
    /// `itemGenerationTime`: when the running cycle started, ns since 2000-01-01 UTC.
    pub generation_time_ns: Option<u64>,
}

impl ItemGenerationState {
    /// The member body (the slot byte, the children byte and the children).
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        let children = self.active.map_or(0, |_| GENERATION_ACTIVE_CHILD)
            | self.generation_time_ns.map_or(0, |_| GENERATION_TIME_CHILD);
        if children == 0 {
            writer.u8(0);
            return writer.finish();
        }
        writer.u8(SLOT_0).u8(children);
        if let Some(active) = self.active {
            writer.boolean(active);
        }
        if let Some(time) = self.generation_time_ns {
            writer.u64(time);
        }
        writer.finish()
    }
}

/// The wire form of a turret's yaw, pitch or heat (Observed: binary16; the cannon of
/// `RS_offensive_war` left at yaw 0.153 and pitch 0.2597 rad, its last 4273, and the 4276 sent
/// `30e5` and `3428`).
pub fn turret_half(value: f32) -> u16 {
    crate::alc::half_bits(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::tests::hex;

    /// Retail `RS_offensive_war` 858.055 s: the first aim of the cannon.
    const INPUT: &str = "e8ceceac05c3d8e82896c8c14210296ec8e5b15b50b29d5867767e3572dd4a63589db2505bb1e5c8e923195be88fa16a0b9832d35e03a10b000000000000000000";
    /// Retail `RS_offensive_war` 859.971 s: a cannon shot.
    const FIRE: &str = "e8ceceac05c3d8e82896c8c14210296ec8e5b15b50b29d5867767e3572dd4a63589db2505bb1e5c8e923195be88fa16a0b9832d3d1101977bbbc51803df98d3c0001";
    /// Retail `joining_defending_war_2` 195.438 s: the first frame of a pull of the trigger.
    const REPEATER: &str = "e8ceceac05c3d8e82896c8c14210296efe2d8806c5c5b3da3218b9ad4b3dcb4fdab3c5c506882dfe12fbfc0a8ffca36e0b97927abd0adcadbe36a2c3be25b173073d088889";

    /// Retail item generation bodies of the first T1 generator of `joining_defending_war_2`:
    /// full and idle (0.602 s), running from a take (980.336 s), the next cycle (990.345 s).
    #[test]
    fn the_item_generation_state_writes_the_retail_bodies() {
        let idle = ItemGenerationState {
            active: Some(false),
            generation_time_ns: Some(0x0b97_914c_9f29_3993),
        };
        assert_eq!(idle.encode(), hex("0103000b97914c9f293993"));
        let running = ItemGenerationState {
            active: Some(true),
            generation_time_ns: Some(0x0b97_9333_d069_ed89),
        };
        assert_eq!(running.encode(), hex("0103010b979333d069ed89"));
        let next = ItemGenerationState {
            active: None,
            generation_time_ns: Some(0x0b97_9336_24b2_8e65),
        };
        assert_eq!(next.encode(), hex("01020b97933624b28e65"));
        assert_eq!(ItemGenerationState::default().encode(), [0]);
    }

    #[test]
    fn the_retail_calls_read_and_write_back() {
        for (index, body) in [
            (TURRET_INPUT_TYPE_INDEX, INPUT),
            (TURRET_FIRE_PROJECTILE_TYPE_INDEX, FIRE),
            (TURRET_REPEATER_INPUT_TYPE_INDEX, REPEATER),
        ] {
            let bytes = hex(body);
            let call = TurretClientMsg::decode(index, &bytes)
                .expect("a turret call")
                .expect("reads");
            assert_eq!(call.type_index(), index);
            assert_eq!(call.encode(), bytes, "type {index}");
        }
        assert!(TurretClientMsg::decode(2720, &hex(INPUT)).is_none());
    }

    #[test]
    fn the_aim_names_the_turret_its_facet_and_the_angles() {
        let call = TurretClientMsg::decode(TURRET_REPEATER_INPUT_TYPE_INDEX, &hex(REPEATER))
            .expect("call")
            .expect("reads");
        let aim = call.aim();
        assert_eq!(
            aim.turret_id(),
            *b"\xfe\x2d\x88\x06\xc5\xc5\xb3\xda\x32\x18\xb9\xad\x4b\x3d\xcb\x4f"
        );
        assert_eq!(
            aim.turret[RAW_ID_LEN..],
            0xdab3_c5c5_0688_2dfe_u64.to_be_bytes()
        );
        assert_eq!(aim.turret_facet, 0x12fb_fc0a_8ffc_a36e);
        assert!((aim.yaw.to_degrees() - (-10.2)).abs() < 0.1);
        assert!((aim.pitch.to_degrees() - (-9.3)).abs() < 0.1);
        let TurretClientMsg::RepeaterInput { flags, frame_s, .. } = call else {
            panic!("a repeater input");
        };
        assert!(flags.held() && flags.fired());
        assert!((frame_s - 1.0 / 30.0).abs() < 1e-6);
        let fire = TurretClientMsg::decode(TURRET_FIRE_PROJECTILE_TYPE_INDEX, &hex(FIRE))
            .expect("call")
            .expect("reads");
        assert!(matches!(fire, TurretClientMsg::FireProjectile { tail, .. } if tail == FIRE_TAIL));
    }

    #[test]
    fn a_short_or_long_body_is_refused() {
        let mut bytes = hex(INPUT);
        bytes.push(0);
        assert!(
            TurretClientMsg::decode(TURRET_INPUT_TYPE_INDEX, &bytes)
                .expect("a turret call")
                .is_err()
        );
        bytes.truncate(40);
        assert!(
            TurretClientMsg::decode(TURRET_INPUT_TYPE_INDEX, &bytes)
                .expect("a turret call")
                .is_err()
        );
    }

    /// Retail bodies of the 57 member (after its component and class bytes).
    #[test]
    fn the_spawner_state_writes_the_retail_bodies() {
        let full = ProjectileSpawnerState {
            ammo: Some(200),
            firing_blocked: Some(false),
        };
        assert_eq!(full.encode(), hex("010300c800"));
        let shot = ProjectileSpawnerState {
            ammo: Some(14),
            firing_blocked: Some(true),
        };
        assert_eq!(shot.encode(), hex("0103000e01"));
        let free = ProjectileSpawnerState {
            ammo: None,
            firing_blocked: Some(false),
        };
        assert_eq!(free.encode(), hex("010200"));
        let repeater_shot = ProjectileSpawnerState {
            ammo: Some(199),
            firing_blocked: None,
        };
        assert_eq!(repeater_shot.encode(), hex("010100c7"));
    }

    /// Retail `fort-equipements-territory` 32.359 s: choice 1 on the new ballista `da45db55...`.
    const BUILD: &str = "f47a035b15f3988b6571baefe7126f62da45db55a31fca97beca772ac68911b697ca1fa355db45da741cf34403f53d5e00000001";

    #[test]
    fn the_retail_build_call_reads_and_writes_back() {
        let bytes = hex(BUILD);
        let call = BuildDefensiveStructureMsg::decode(BUILD_DEFENSIVE_STRUCTURE_TYPE_INDEX, &bytes)
            .expect("a build call")
            .expect("reads");
        assert_eq!(call.choice, 1);
        assert_eq!(call.structure_id()[..4], [0xda, 0x45, 0xdb, 0x55]);
        assert_eq!(
            call.structure[RAW_ID_LEN..],
            0x97ca_1fa3_55db_45da_u64.to_be_bytes()
        );
        assert_eq!(call.encode(), bytes);
        assert!(BuildDefensiveStructureMsg::decode(TURRET_INPUT_TYPE_INDEX, &bytes).is_none());
        assert!(
            BuildDefensiveStructureMsg::decode(BUILD_DEFENSIVE_STRUCTURE_TYPE_INDEX, &bytes[..50])
                .expect("a build call")
                .is_err()
        );
    }

    #[test]
    fn the_angles_go_out_as_the_retail_half_floats() {
        assert_eq!(turret_half(f32::from_bits(0x3e1c_ae5c)), 0x30e5);
        assert_eq!(turret_half(f32::from_bits(0x3e84_fbb5)), 0x3428);
        assert_eq!(turret_half(0.0), 0);
    }

    #[test]
    fn the_gde_ref_is_the_raw_id_and_the_facet_entity() {
        let raw = *b"\x6e\x29\x10\x42\xc1\xc8\x96\x28\xb2\x41\x02\xa9\x38\x46\x64\xdb";
        assert_eq!(
            gde_ref_of(raw, 0x2896_c8c1_4210_296e).to_vec(),
            hex("6e291042c1c89628b24102a9384664db2896c8c14210296e")
        );
    }
}
