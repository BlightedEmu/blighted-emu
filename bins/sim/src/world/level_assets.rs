// SPDX-License-Identifier: AGPL-3.0-or-later
//! The tutorial's asset entities that one player sees (ftue-level-entities note): the
//! encounters, their spawn markers, lightning slices, loot sacks, teleporters and the fog wall.
//!
//! Each card names the entity's slice asset and a `GdeRef` that is new per session; the server
//! makes one per player and entity ([`asset_gde_ref`]). The states are the retail ones of the
//! level table ([`FtueLevel::assets`]); their changes come on the tutorial's events
//! ([`super::encounter`]).

use std::collections::{BTreeMap, VecDeque};

use nw_content::ftue_entities::{AssetGroup, FtueLevel, GDE_REF_LEN, RefTarget, SessionIds};
use nw_messages::{BundleGroup, BundleMember, MemberState, as_dynamic_raw_id, raw_id_of_entity};
use uuid::Uuid;

use super::asset_script::SpellCast;
use super::slots::Slot;

/// First slot key of an asset entity: its index in the table is added. Keys are never sent; they
/// stay above every player, mob and static entity number.
pub(super) const ASSET_ENTITY_BASE: u64 = 1 << 41;
/// Names the `GdeRef`s this server makes for asset entities.
const ASSET_GDE_REF_NAMESPACE: u64 = 0x6173_7365_7473;

/// The `GdeRef` of asset entity `index` for player `player`: the same on every call, other for
/// every player and entity.
pub(super) fn asset_gde_ref(player: u64, index: usize) -> [u8; GDE_REF_LEN] {
    let namespace = Uuid::from_u64_pair(ASSET_GDE_REF_NAMESPACE, player);
    as_dynamic_raw_id(*Uuid::new_v5(&namespace, &(index as u64).to_be_bytes()).as_bytes())
}

/// The `GdeRef` of cast `number` of spell slice `index` for player `player`: new per cast
/// (boss note: retail made one per cast), and never an [`asset_gde_ref`].
pub(super) fn spell_gde_ref(player: u64, index: usize, number: u32) -> [u8; GDE_REF_LEN] {
    let namespace = Uuid::from_u64_pair(ASSET_GDE_REF_NAMESPACE, player);
    let mut name = (index as u64).to_be_bytes().to_vec();
    name.extend(number.to_be_bytes());
    as_dynamic_raw_id(*Uuid::new_v5(&namespace, &name).as_bytes())
}

/// The class of a spell slice's spell state (2912), whose body holds where the spell aims: the
/// `f32` x and y at these offsets (boss note: retail the player's place at the cast).
const SPELL_CLASS: u64 = 2912;
const SPELL_X_AT: usize = 6;
const SPELL_Y_AT: usize = 10;
const F32_LEN: usize = 4;

/// One shown asset entity.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AssetView {
    gde_ref: [u8; GDE_REF_LEN],
    /// Its change groups to send, by index in the table, in order.
    pending: VecDeque<usize>,
    /// The cast of a spell slice.
    cast: Option<SpellCast>,
}

/// The asset entities one player sees, with the changes not sent yet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct AssetViews {
    shown: BTreeMap<usize, AssetView>,
}

impl AssetViews {
    /// Shows asset entity `index` with `GdeRef` `gde_ref`.
    pub(super) fn show(&mut self, index: usize, gde_ref: [u8; GDE_REF_LEN]) {
        self.shown.insert(
            index,
            AssetView {
                gde_ref,
                pending: VecDeque::new(),
                cast: None,
            },
        );
    }

    /// Shows spell slice `index` with `GdeRef` `gde_ref` for `cast`: its bodies name the caster
    /// and where it aims.
    pub(super) fn cast(&mut self, index: usize, gde_ref: [u8; GDE_REF_LEN], cast: SpellCast) {
        self.shown.insert(
            index,
            AssetView {
                gde_ref,
                pending: VecDeque::new(),
                cast: Some(cast),
            },
        );
    }

    /// Queues change group `change` of shown asset entity `index`.
    pub(super) fn change(&mut self, index: usize, change: usize) {
        if let Some(view) = self.shown.get_mut(&index) {
            view.pending.push_back(change);
        }
    }

    /// Hides asset entity `index`.
    pub(super) fn hide(&mut self, index: usize) {
        self.shown.remove(&index);
    }

    /// The slot keys of the shown entities.
    pub(super) fn numbers(&self) -> impl Iterator<Item = u64> + '_ {
        self.shown
            .keys()
            .map(|index| ASSET_ENTITY_BASE + *index as u64)
    }

    /// The group of the entity of slot key `number` in `slot`: its first spawn states when the
    /// slot is new, else its next change; `None` for no shown entity or nothing to send. The
    /// bodies carry the ids of `ids` and the `GdeRef`s of the shown entities they name.
    pub(super) fn group(
        &mut self,
        number: u64,
        slot: Slot,
        level: &FtueLevel,
        ids: &SessionIds,
    ) -> Option<BundleGroup> {
        let index = usize::try_from(number.checked_sub(ASSET_ENTITY_BASE)?).ok()?;
        let entity = level.assets.get(index)?;
        let group: &AssetGroup = if slot.new {
            entity.spawns.first()?
        } else {
            let change = self.shown.get_mut(&index)?.pending.pop_front()?;
            entity.changes.get(change)?
        };
        let cast = self.shown.get(&index).and_then(|view| view.cast);
        let mut bodies = group.patched_bodies(ids, |target| match target {
            RefTarget::Asset(other) => self.shown.get(&other).map(|view| view.gde_ref),
            // No asset body of the tutorial names a mob that is not a spell's caster.
            RefTarget::Mob(_) => cast.map(|cast| raw_id_of_entity(cast.caster)),
        });
        if let Some(cast) = cast {
            let spells = group
                .group
                .members
                .iter()
                .zip(&mut bodies)
                .filter(|(member, _)| member.class == SPELL_CLASS);
            for (_, body) in spells {
                for (at, bits) in [SPELL_X_AT, SPELL_Y_AT].into_iter().zip(cast.at_bits) {
                    if let Some(field) = body.get_mut(at..at + F32_LEN) {
                        field.copy_from_slice(&bits.to_be_bytes());
                    }
                }
            }
        }
        Some(BundleGroup {
            interest: u64::from(slot.interest),
            members: group
                .group
                .members
                .iter()
                .zip(bodies)
                .map(|(member, body)| BundleMember {
                    component: member.component,
                    state: MemberState::Recorded {
                        class: member.class,
                        body,
                    },
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use nw_content::ftue_entities::{
        AssetEntity, BodyRef, EntityGroup, EntityMember, FtueEntities, IdForm,
    };

    use super::*;

    const PLAYER: u64 = 7;
    const CARD_CLASS: u64 = 10;
    const ENCOUNTER_CLASS: u64 = 2133;
    const GDE_REF_AT: usize = 22;

    fn group(class: u64, body: Vec<u8>, refs: Vec<BodyRef>) -> AssetGroup {
        AssetGroup {
            group: EntityGroup {
                time_ms: 0,
                interest: 16,
                members: vec![EntityMember {
                    component: 0,
                    class,
                    body,
                    player_gde_ref_at: Vec::new(),
                    player_facet_at: Vec::new(),
                    phase_suffix_at: Vec::new(),
                }],
            },
            refs,
            cause: None,
        }
    }

    /// One encounter: a card with its own `GdeRef` at 22, and one wave change.
    fn level() -> FtueLevel {
        let card = vec![0xcd; GDE_REF_AT + GDE_REF_LEN];
        FtueLevel {
            statics: FtueEntities::default(),
            static_causes: Vec::new(),
            assets: vec![AssetEntity {
                asset: [4; 16],
                asset_sub_id: 2,
                retail_gde_ref: [0xcd; GDE_REF_LEN],
                position: Some([484.35, 848.6, 91.37]),
                spawns: vec![group(
                    CARD_CLASS,
                    card,
                    vec![BodyRef {
                        member: 0,
                        at: GDE_REF_AT,
                        form: IdForm::GdeRef,
                        target: RefTarget::Asset(0),
                    }],
                )],
                changes: vec![group(ENCOUNTER_CLASS, vec![1, 1, 3], Vec::new())],
                despawns: Vec::new(),
            }],
            mobs: Vec::new(),
        }
    }

    #[test]
    fn a_spell_names_its_caster_and_aims_at_the_cast_place() {
        const CASTER_AT: usize = 44;
        const THORPE: u64 = 0x4_0000_0009;
        let spell = vec![0xee; CASTER_AT + GDE_REF_LEN];
        let mut level = level();
        level.assets[0].spawns = vec![group(
            SPELL_CLASS,
            spell,
            vec![BodyRef {
                member: 0,
                at: CASTER_AT,
                form: IdForm::GdeRef,
                target: RefTarget::Mob(9),
            }],
        )];
        let cast = SpellCast {
            caster: THORPE,
            at_bits: [480.85_f32.to_bits(), 227.1_f32.to_bits()],
            number: 2,
        };
        let gde_ref = spell_gde_ref(PLAYER, 0, cast.number);
        assert_ne!(
            gde_ref,
            spell_gde_ref(PLAYER, 0, cast.number + 1),
            "per cast"
        );
        assert_ne!(gde_ref, asset_gde_ref(PLAYER, 0));
        let mut views = AssetViews::default();
        views.cast(0, gde_ref, cast);
        let new = Slot {
            interest: 16,
            new: true,
        };
        let shown = views
            .group(ASSET_ENTITY_BASE, new, &level, &ids())
            .expect("spell");
        let MemberState::Recorded { body, .. } = &shown.members[0].state else {
            panic!("recorded");
        };
        assert_eq!(body[CASTER_AT..], raw_id_of_entity(THORPE));
        assert_eq!(
            body[SPELL_X_AT..SPELL_X_AT + F32_LEN],
            480.85_f32.to_be_bytes()
        );
        assert_eq!(
            body[SPELL_Y_AT..SPELL_Y_AT + F32_LEN],
            227.1_f32.to_be_bytes()
        );
    }

    fn ids() -> SessionIds {
        SessionIds {
            player_gde_ref: [1; GDE_REF_LEN],
            player_facet: [2; 8],
            phase_id: 3,
        }
    }

    #[test]
    fn asset_and_spell_ids_are_dynamic_like_retail() {
        // Retail's 26 tutorial asset entities and every dynamic card keep bit 0 of the first
        // byte clear; with it set the client takes the id as a level object.
        const INDEXES: usize = 64;
        for index in 0..INDEXES {
            assert!(nw_messages::is_dynamic_raw_id(&asset_gde_ref(
                PLAYER, index
            )));
            let cast = u32::try_from(index).expect("cast");
            assert!(nw_messages::is_dynamic_raw_id(&spell_gde_ref(
                PLAYER, 0, cast
            )));
        }
    }

    #[test]
    fn a_card_names_our_gde_ref_and_a_change_follows() {
        let level = level();
        let mut views = AssetViews::default();
        let gde_ref = asset_gde_ref(PLAYER, 0);
        assert_ne!(gde_ref, asset_gde_ref(PLAYER + 1, 0), "per player");
        assert_ne!(gde_ref, asset_gde_ref(PLAYER, 1), "per entity");
        views.show(0, gde_ref);
        views.change(0, 0);
        let new = Slot {
            interest: 16,
            new: true,
        };
        let card = views
            .group(ASSET_ENTITY_BASE, new, &level, &ids())
            .expect("card");
        let MemberState::Recorded { body, .. } = &card.members[0].state else {
            panic!("recorded");
        };
        assert_eq!(body[GDE_REF_AT..], gde_ref);
        let kept = Slot { new: false, ..new };
        let change = views
            .group(ASSET_ENTITY_BASE, kept, &level, &ids())
            .expect("change");
        assert_eq!(change.members[0].state.type_index(), ENCOUNTER_CLASS);
        assert_eq!(views.group(ASSET_ENTITY_BASE, kept, &level, &ids()), None);
        views.hide(0);
        assert_eq!(views.numbers().count(), 0);
    }
}
