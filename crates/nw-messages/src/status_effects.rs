// SPDX-License-Identifier: AGPL-3.0-or-later
//! `StatusEffectsComponentReplicatedState` (type 4236, own player component 44): the status
//! effects of an entity (run-15 combat study: constructor `FUN_144014a60` of build 25810808 and
//! seven retail captures).
//!
//! The body starts with a group byte; bit 0 is no group. Each group present gives one child byte
//! (7 children, bit 7: another byte follows), then its children in order:
//!
//! - group 1 (`0x02`, the owner): child 0 `m_localEffectsMap` (key `u16`: [`LocalEffect`]),
//!   child 1 `m_lightweightLocalEffectsMap` (key `u16`: a `u64`, [`LightweightValue`]), child 2
//!   `m_activeTrayIcons` (never sent by retail);
//! - group 2 (`0x04`): territory effects, dynamic scaling and two update-count maps (retail sends
//!   them only in the open world and on mobs; not read here);
//! - group 3 (`0x08`, everyone else): child 0 `m_effectsMap` (never sent), child 1
//!   `m_remoteEffectsMap` (key `u16`: one byte, bits 0-6 the stacks, bit 7 a potency above 0).
//!
//! A map is the replicated list: the full form (`00`, the version flag and version, the count,
//! then key and value of each entry) or the delta form (a count above 0, then groups of up to 8
//! entries: a mask byte, then per entry the key, the version flag and version, and the value
//! when its mask bit is set; a clear bit removes the entry). Retail writes the version on a
//! delta's first entry and 0 on the others. All numbers are big-endian; the potency is an IEEE
//! `half`. A key is the place of the effect in the sorted CRC list of every status effect row
//! (`nw_content::status_effects`).

use crate::alc::{half_bits, half_value};
use crate::items::{FULL_FORM, FullList, VERSION_PRESENT};
use crate::slots::{read_children, read_slot_byte, write_known_children};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `StatusEffectsComponentReplicatedState`.
pub const STATUS_EFFECTS_TYPE_INDEX: u64 = 4236;
/// The own player component of the status effects (component-slots note, slot 44).
pub const STATUS_EFFECTS_COMPONENT: u64 = 44;

/// Group bits of the body.
const OWNER_GROUP: u8 = 0x02;
const TERRITORY_GROUP: u8 = 0x04;
const REMOTE_GROUP: u8 = 0x08;
/// The group byte names 4 groups (bit 0 is none).
const GROUPS: u8 = 4;
/// Children of each group.
const OWNER_CHILDREN: u8 = 3;
const REMOTE_CHILDREN: u8 = 2;
/// Children of the owner group.
const LOCAL_CHILD: u8 = 0;
const LIGHTWEIGHT_CHILD: u8 = 1;
/// Children of the remote group.
const REMOTE_EFFECTS_CHILD: u8 = 1;
/// Entries per delta group.
const DELTA_GROUP: usize = 8;
/// A remote entry: the stacks bits and the potency bit.
const REMOTE_STACK_BITS: u8 = 0x7f;
const REMOTE_POTENCY_BIT: u8 = 0x80;
/// The potency a remote entry with its potency bit gives back (the client reads 1.0 or 0).
const REMOTE_POTENCY: f32 = 1.0;
/// The smallest potency retail counts as one (`|potency| > 1.19e-7`, the `f32` epsilon).
const POTENCY_EPSILON: f32 = f32::EPSILON;
/// The most delta entries a body may hold (the server writer's delta limit).
const MAX_CHANGES: u64 = 16_383;
const FIELD: &str = "status_effects";

/// One effect of the owner's local map (12 bytes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalEffect {
    /// When it ends: ns since 1970-01-01 (Observed: retail `Status_OutOfCombat_PassiveRegen`
    /// ends 179.9 s after its add, `BaseDuration` 180); 0 for an effect without an end.
    pub end_unix_ns: u64,
    /// The sum of its stacks' potency.
    pub potency: f32,
    /// Its stacks.
    pub stacks: u8,
    /// Its applications so far (retail: 1 to 6 for a buff of 3 stacks applied 6 times).
    pub applications: u8,
}

impl LocalEffect {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            end_unix_ns: reader.u64()?,
            potency: half_value(reader.u16()?),
            stacks: reader.u8()?,
            applications: reader.u8()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer
            .u64(self.end_unix_ns)
            .u16(half_bits(self.potency))
            .u8(self.stacks)
            .u8(self.applications);
    }
}

/// The value of a lightweight effect (Observed, 52 effects in seven retail captures): the end in
/// ns since 1970-01-01 for an effect that the client does not predict, else a 30 Hz tick below
/// 2^16 (the end tick, or for an effect without an end the tick of its add less 30), and 0 for
/// an effect without an end that the client does not predict.
pub type LightweightValue = u64;

/// One entry of a remote map: the stacks and whether the potency is above 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteEffect {
    /// Its stacks (0 to 127).
    pub stacks: u8,
    /// Its potency is above 0.
    pub potent: bool,
}

impl RemoteEffect {
    /// The remote entry of an effect of `stacks` and `potency`.
    pub fn of(stacks: u8, potency: f32) -> Self {
        Self {
            stacks: stacks & REMOTE_STACK_BITS,
            potent: potency.abs() > POTENCY_EPSILON,
        }
    }

    /// The potency the client gives it back.
    pub const fn potency(&self) -> f32 {
        if self.potent { REMOTE_POTENCY } else { 0.0 }
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let byte = reader.u8()?;
        Ok(Self {
            stacks: byte & REMOTE_STACK_BITS,
            potent: byte & REMOTE_POTENCY_BIT != 0,
        })
    }

    fn write(&self, writer: &mut Writer) {
        let potent = if self.potent { REMOTE_POTENCY_BIT } else { 0 };
        writer.u8((self.stacks & REMOTE_STACK_BITS) | potent);
    }
}

/// One delta entry of a map: the key, its version (`None`: flag 0), and its new value
/// (`None`: removed).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapChange<V> {
    /// The effect key.
    pub key: u16,
    /// The version; retail sends it on the first entry and 0 on the others.
    pub version: Option<u64>,
    /// The new value; `None` removes the entry.
    pub value: Option<V>,
}

/// A map child in its full form or as changes.
#[derive(Debug, Clone, PartialEq)]
pub enum EffectMap<V> {
    /// The whole map: (key, value) entries.
    Full(FullList<(u16, V)>),
    /// Changed, added and removed entries.
    Changes(Vec<MapChange<V>>),
}

impl<V: Copy> EffectMap<V> {
    /// The whole map of `entries` at `version`.
    pub fn full(version: u64, entries: Vec<(u16, V)>) -> Self {
        Self::Full(FullList {
            version: Some(version),
            elements: entries,
        })
    }

    fn read(
        reader: &mut Reader<'_>,
        mut read_value: impl FnMut(&mut Reader<'_>) -> Result<V, MessageError>,
    ) -> Result<Self, MessageError> {
        if Reader::new(reader.rest()).wide_prefixed()? == FULL_FORM {
            return FullList::read(reader, |reader| Ok((reader.u16()?, read_value(reader)?)))
                .map(Self::Full);
        }
        let count = reader.wide_prefixed()?;
        if count > MAX_CHANGES {
            return Err(MessageError::CountTooLarge {
                count,
                limit: MAX_CHANGES,
            });
        }
        // At most `MAX_CHANGES` entries.
        let mut left = count as usize;
        let mut changes = Vec::with_capacity(left);
        while left > 0 {
            let mask = reader.u8()?;
            let in_group = left.min(DELTA_GROUP);
            for entry in 0..in_group {
                let key = reader.u16()?;
                let version = read_version(reader)?;
                let value = if mask & (1 << entry) != 0 {
                    Some(read_value(reader)?)
                } else {
                    None
                };
                changes.push(MapChange {
                    key,
                    version,
                    value,
                });
            }
            left -= in_group;
        }
        Ok(Self::Changes(changes))
    }

    fn write(
        &self,
        writer: &mut Writer,
        mut write_value: impl FnMut(&mut Writer, &V),
    ) -> Result<(), MessageError> {
        match self {
            Self::Full(list) => list.write(writer, |writer, (key, value)| {
                writer.u16(*key);
                write_value(writer, value);
                Ok(())
            }),
            Self::Changes(changes) => {
                writer.wide_prefixed(changes.len() as u64);
                for group in changes.chunks(DELTA_GROUP) {
                    let mask = group
                        .iter()
                        .enumerate()
                        .filter(|(_, change)| change.value.is_some())
                        .fold(0_u8, |mask, (entry, _)| mask | (1 << entry));
                    writer.u8(mask);
                    for change in group {
                        writer.u16(change.key);
                        match change.version {
                            Some(version) => writer.u8(VERSION_PRESENT).wide_prefixed(version),
                            None => writer.u8(0),
                        };
                        if let Some(value) = &change.value {
                            write_value(writer, value);
                        }
                    }
                }
                Ok(())
            }
        }
    }
}

/// Reads a version: the flag byte, then the version when the flag is 1.
fn read_version(reader: &mut Reader<'_>) -> Result<Option<u64>, MessageError> {
    match reader.u8()? {
        0 => Ok(None),
        VERSION_PRESENT => Ok(Some(reader.wide_prefixed()?)),
        value => Err(MessageError::NotBoolean { value }),
    }
}

/// The status effects state; a `None` child is not sent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatusEffectsState {
    /// The owner's effects (`m_localEffectsMap`).
    pub local: Option<EffectMap<LocalEffect>>,
    /// The owner's lightweight effects (`m_lightweightLocalEffectsMap`).
    pub lightweight: Option<EffectMap<LightweightValue>>,
    /// The effects others see (`m_remoteEffectsMap`).
    pub remote: Option<EffectMap<RemoteEffect>>,
}

impl StatusEffectsState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, or sends the territory group, the tray
    /// icons or `m_effectsMap` (never sent by retail to a player of a fresh character; not sized
    /// here).
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let groups = read_slot_byte(reader, FIELD, GROUPS)?;
        if groups & (TERRITORY_GROUP | 1) != 0 {
            return Err(MessageError::Unsized {
                field: FIELD,
                byte: groups,
            });
        }
        let mut state = Self::default();
        if groups & OWNER_GROUP != 0 {
            read_children(reader, FIELD, OWNER_CHILDREN, |reader, child| {
                match child {
                    LOCAL_CHILD => state.local = Some(EffectMap::read(reader, LocalEffect::read)?),
                    LIGHTWEIGHT_CHILD => {
                        state.lightweight = Some(EffectMap::read(reader, |reader| reader.u64())?);
                    }
                    other => {
                        return Err(MessageError::Unsized {
                            field: FIELD,
                            byte: other,
                        });
                    }
                }
                Ok(())
            })?;
        }
        if groups & REMOTE_GROUP != 0 {
            read_children(reader, FIELD, REMOTE_CHILDREN, |reader, child| {
                if child != REMOTE_EFFECTS_CHILD {
                    return Err(MessageError::Unsized {
                        field: FIELD,
                        byte: child,
                    });
                }
                state.remote = Some(EffectMap::read(reader, RemoteEffect::read)?);
                Ok(())
            })?;
        }
        Ok(state)
    }

    /// Writes the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a map cannot be written.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let owner = self.local.is_some() || self.lightweight.is_some();
        let owner_bit = if owner { OWNER_GROUP } else { 0 };
        let remote_bit = if self.remote.is_some() {
            REMOTE_GROUP
        } else {
            0
        };
        writer.u8(owner_bit | remote_bit);
        let mut result = Ok(());
        if owner {
            let children = (u16::from(self.local.is_some()) << LOCAL_CHILD)
                | (u16::from(self.lightweight.is_some()) << LIGHTWEIGHT_CHILD);
            write_known_children(writer, children, OWNER_CHILDREN, |writer, child| {
                let written = match (child, &self.local, &self.lightweight) {
                    (LOCAL_CHILD, Some(map), _) => {
                        map.write(writer, |writer, effect| effect.write(writer))
                    }
                    (LIGHTWEIGHT_CHILD, _, Some(map)) => map.write(writer, |writer, value| {
                        writer.u64(*value);
                    }),
                    _ => Ok(()),
                };
                if result.is_ok() {
                    result = written;
                }
            });
        }
        if let Some(map) = &self.remote {
            write_known_children(
                writer,
                1 << REMOTE_EFFECTS_CHILD,
                REMOTE_CHILDREN,
                |writer, _| {
                    let written = map.write(writer, |writer, effect| effect.write(writer));
                    if result.is_ok() {
                        result = written;
                    }
                },
            );
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The key of `EquipLoad_CCDurReduction_Heavy` and of `EquipLoad_CritDmgReduction_Heavy`
    /// (build 25810808 tables).
    const CC_HEAVY: u16 = 535;
    const CRIT_HEAVY: u16 = 2133;
    /// The lightweight value of a predicted effect without an end added at tick 1: 1 - 30.
    const TICK_ONE_LESS_30: u64 = 65_507;

    /// The first own 4236 body of the retail newchar capture (2026-09-18): the owner group, the
    /// lightweight child alone, full form, version 4, the two equip-load effects.
    const RETAIL_SPAWN: [u8; 26] = [
        0x02, 0x02, 0x00, 0x01, 0x04, 0x02, 0x02, 0x17, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff,
        0xe3, 0x08, 0x55, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xe3,
    ];

    fn round(state: &StatusEffectsState) -> Vec<u8> {
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        let body = writer.finish();
        let mut reader = Reader::new(&body);
        let back = StatusEffectsState::read(&mut reader).expect("read");
        reader.finish().expect("whole");
        assert_eq!(&back, state);
        body
    }

    #[test]
    fn the_retail_spawn_body_reads_and_writes_the_same_bytes() {
        let body = RETAIL_SPAWN.as_slice();
        let mut reader = Reader::new(body);
        let state = StatusEffectsState::read(&mut reader).expect("read");
        reader.finish().expect("whole");
        assert_eq!(
            state.lightweight,
            Some(EffectMap::full(
                4,
                vec![(CC_HEAVY, TICK_ONE_LESS_30), (CRIT_HEAVY, TICK_ONE_LESS_30)]
            ))
        );
        assert_eq!(state.local, None);
        assert_eq!(round(&state), body);
    }

    #[test]
    fn a_local_effect_carries_its_end_half_potency_stacks_and_applications() {
        let state = StatusEffectsState {
            local: Some(EffectMap::full(
                854,
                vec![(
                    2286,
                    LocalEffect {
                        end_unix_ns: 0x18dd_339a_3734_0864,
                        potency: 2.0,
                        stacks: 2,
                        applications: 2,
                    },
                )],
            )),
            ..StatusEffectsState::default()
        };
        let body = round(&state);
        // Group, child byte, full form, version 854 (two bytes), count 1, key, end, half 2.0.
        assert_eq!(&body[..3], &[OWNER_GROUP, 1 << LOCAL_CHILD, 0x00]);
        assert_eq!(&body[body.len() - 4..], &[0x40, 0x00, 2, 2]);
    }

    #[test]
    fn a_delta_removes_an_entry_with_its_mask_bit_clear() {
        let state = StatusEffectsState {
            local: Some(EffectMap::Changes(vec![
                MapChange {
                    key: 2286,
                    version: Some(33),
                    value: None,
                },
                MapChange {
                    key: 4425,
                    version: Some(0),
                    value: Some(LocalEffect {
                        end_unix_ns: 0,
                        potency: 9.0,
                        stacks: 1,
                        applications: 0,
                    }),
                },
            ])),
            ..StatusEffectsState::default()
        };
        let body = round(&state);
        // Count 2, mask: only the second entry has a value.
        assert_eq!(&body[2..4], &[2, 0b10]);
    }

    #[test]
    fn the_empty_remote_map_of_a_mob_spawn_is_the_retail_form() {
        let state = StatusEffectsState {
            remote: Some(EffectMap::full(1005, Vec::new())),
            ..StatusEffectsState::default()
        };
        let body = round(&state);
        let mut version = Writer::new();
        version.wide_prefixed(1005);
        let mut expected = vec![
            REMOTE_GROUP,
            1 << REMOTE_EFFECTS_CHILD,
            0x00,
            VERSION_PRESENT,
        ];
        expected.extend(version.finish());
        expected.push(0);
        assert_eq!(body, expected);
    }

    #[test]
    fn a_remote_entry_keeps_the_stacks_and_whether_it_has_potency() {
        let entry = RemoteEffect::of(3, 2.5);
        assert_eq!(
            (entry.stacks, entry.potent, entry.potency()),
            (3, true, 1.0)
        );
        assert!(!RemoteEffect::of(1, 0.0).potent);
        let mut writer = Writer::new();
        entry.write(&mut writer);
        assert_eq!(writer.finish(), [0x83]);
    }

    #[test]
    fn the_territory_group_is_not_read() {
        let mut reader = Reader::new(&[TERRITORY_GROUP, 0x01, 0x00, 0x01, 0x05, 0x00]);
        assert!(StatusEffectsState::read(&mut reader).is_err());
    }
}
