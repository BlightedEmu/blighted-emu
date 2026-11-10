// SPDX-License-Identifier: AGPL-3.0-or-later
//! Spell areas of player abilities (abilities design note, 2026-10-31; the `SpellData` rows of
//! build 25810808). An area spell stays where it was cast while its `Duration` lasts (Sacred
//! Ground 12 s, Ice Storm 10 s, Meteor Storm 6 s); a `Duration` of -1 or 0 is one pass. Each tick
//! the targets in its radius hold its status effects: with `ClearOnExit` a target that leaves
//! loses them, with `ClearOnEnd` every target in it loses them when it ends. Its damage row hits
//! each enemy once, when it first stands in it (Inferred: a lasting area's repeated damage is its
//! status effects' own ticks, as `Status_FireStaff_MeteorShower_Damage` on Meteor Storm).
//!
//! When it opens, an area whose spell has a slice ([`nw_content::spell_slices`]) spawns its
//! spell entity: the card of the slice, the spell state (the spell, the area's centre, the
//! caster's turn and id), the caster's ownership when the slice holds one, and the slice's other
//! states. Every player near sees it (Sacred Ground's circle). It goes when the area ends, and
//! no sooner than [`SPELL_ENTITY_MIN_MS`].

use bevy_ecs::entity::Entity;
use nw_bus::ConnectionRef;
use nw_content::player_abilities::SpellRow;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::buildables::{NO_OWNER_KIND, OwnerIdentification};
use nw_messages::spell_state::{NO_ATTACHMENT, SPELL_REF_LEN, SpellState};
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    AssetId, BundleMember, DYNAMIC_SLICE_SUB_ID, GdeMetadataReplicatedState, MemberState, Rotation,
    WorldPosition, raw_id_of_entity,
};
use nw_sim_core::Position;

use super::abilities::{Caster, ENEMY_TARGETS, FRIEND_TARGETS, SELF_TARGET};
use super::crafting::{CARD_COMPONENT, Station, dynamic_ref, unowned};
use super::{Player, SimWorld};
use crate::combat::{AttackOutcome, AttackRefusal};

/// The radius of a spell row without one: its target only (Inferred: a point spell hits what is
/// on the point).
const POINT_RADIUS_M: f32 = 1.0;
/// A spell entity stays at least this long, so that the entity of a one-pass spell shows
/// (Inferred: when retail takes an impact's entity away is Unknown).
const SPELL_ENTITY_MIN_MS: u64 = 1_000;
/// Names the ids this server makes for spell entities.
const SPELL_ENTITY_NAMESPACE: u64 = 0x7370_656c_6c65_6e74;
/// The components of the spell state and of the ownership in a spell entity's group (retail:
/// 1 and 2 in every group of the 769 spells of the captures).
const SPELL_COMPONENT: u64 = 1;
const OWNERSHIP_COMPONENT: u64 = 2;
/// Retail: `m_aoeRadiusScaling` 1 and permission rule 1 in the spell groups read.
const RADIUS_SCALE: f32 = 1.0;
const PERMISSION_RULE: u8 = 1;

/// A spell area of a player.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SpellArea {
    caster: ConnectionRef,
    caster_entity: Entity,
    /// The caster's number, the source of the effects.
    source: u64,
    spell: SpellRow,
    centre: Position,
    /// When it opens: at the cast, or when the projectile that casts it gets there.
    starts_ms: u64,
    /// When it ends; its start for one pass.
    ends_ms: u64,
    /// The base of the heals of its effects ([`SimWorld::spell_heal_base`]).
    hit_amount: f64,
    /// The mobs its damage row hit.
    hit: Vec<u64>,
    /// The mobs that hold its effects.
    mobs_in: Vec<u64>,
    /// The players that hold its effects.
    players_in: Vec<Entity>,
    /// The yaw of the caster's look at the cast: the turn of its entity.
    yaw: f32,
    /// Its spell entity is in the world.
    shown: bool,
}

impl SpellArea {
    /// The area of `spell` of `caster` at `centre`, from `starts_ms`; `hit_amount` is the hit
    /// of the spell.
    pub(super) fn new(
        caster: &Caster,
        spell: &SpellRow,
        centre: Position,
        starts_ms: u64,
        hit_amount: f64,
    ) -> Self {
        Self {
            caster: caster.connection.clone(),
            caster_entity: caster.entity,
            source: caster.number,
            spell: spell.clone(),
            centre,
            starts_ms,
            ends_ms: starts_ms.saturating_add(spell.lasting_ms().unwrap_or_default()),
            hit_amount,
            hit: Vec::new(),
            mobs_in: Vec::new(),
            players_in: Vec::new(),
            yaw: caster.yaw(),
            shown: false,
        }
    }
}

/// The caster id of a spell state: the raw id of entity `number`, then its entity id, the raw
/// id's first half, big-endian (Observed: retail `c86f8015f81f3a19 ... 193a1ff815806fc8`).
fn caster_ref(number: u64) -> [u8; SPELL_REF_LEN] {
    let raw = raw_id_of_entity(number);
    let mut caster = [0; SPELL_REF_LEN];
    caster[..RAW_ID_LEN].copy_from_slice(&raw);
    let (entity, _) = raw.split_at(SPELL_REF_LEN - RAW_ID_LEN);
    for (to, from) in caster[RAW_ID_LEN..].iter_mut().zip(entity.iter().rev()) {
        *to = *from;
    }
    caster
}

impl SimWorld {
    /// Opens `area`; it takes effect from its start ([`SimWorld::tick_spell_areas`]).
    pub(super) fn open_spell_area(&mut self, area: SpellArea) {
        tracing::debug!(
            source = area.source,
            spell = %area.spell.id,
            x = area.centre.x,
            y = area.centre.y,
            starts_ms = area.starts_ms,
            ends_ms = area.ends_ms,
            hit_amount = area.hit_amount,
            "ability spell area"
        );
        self.spell_areas.push(area);
    }

    /// Runs the spell areas open at `now_ms` and closes the ones that end; gives their hits.
    pub(super) fn tick_spell_areas(
        &mut self,
        now_ms: u64,
    ) -> Vec<Result<AttackOutcome, AttackRefusal>> {
        let mut results = Vec::new();
        let mut open = Vec::new();
        for mut area in std::mem::take(&mut self.spell_areas) {
            if now_ms < area.starts_ms {
                open.push(area);
                continue;
            }
            if !area.shown {
                area.shown = true;
                self.spawn_spell_entity(&area);
            }
            results.extend(self.area_pass(&mut area, now_ms));
            if now_ms < area.ends_ms {
                open.push(area);
            } else if area.spell.lasting_ms().is_some() && area.spell.clear_on_end {
                self.clear_area(&area, &area.mobs_in, &area.players_in);
            }
        }
        self.spell_areas = open;
        let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.spell_entities)
            .into_iter()
            .partition(|(_, until_ms)| now_ms >= *until_ms);
        self.spell_entities = kept;
        for (number, _) in gone {
            self.stations.remove_camp(number);
        }
        results
    }

    /// Spawns the spell entity of `area`, when its spell has a slice and its caster is here.
    fn spawn_spell_entity(&mut self, area: &SpellArea) {
        let spell = lowercase_crc32(&area.spell.id);
        let Some(slice) = self.content.spell_slices.of(spell).cloned() else {
            return;
        };
        let Some(start) = WorldPosition::from_metres(area.centre.x, area.centre.y, area.centre.z)
        else {
            return;
        };
        let Some(owner) =
            self.world
                .get::<Player>(area.caster_entity)
                .map(|player| OwnerIdentification {
                    first: player.character.to_string().into_bytes(),
                    second: player.name.clone().into_bytes(),
                    kind: NO_OWNER_KIND,
                })
        else {
            return;
        };
        let Ok(states) = slice
            .members
            .iter()
            .map(|bytes| BundleMember::read_from(bytes).map(|(member, _)| member))
            .collect::<Result<Vec<_>, _>>()
        else {
            return;
        };
        let half = area.yaw / 2.0;
        let spell_state = SpellState {
            spell: Some(spell),
            start: Some(start),
            start_rotation: Some(Rotation::of_quaternion([0.0, 0.0, half.sin(), half.cos()])),
            attachment: Some(NO_ATTACHMENT),
            caster: Some(caster_ref(area.source)),
            radius_scale: Some(RADIUS_SCALE),
            ..SpellState::default()
        };
        let namespace = SPELL_ENTITY_NAMESPACE ^ self.me.generation;
        let position = area.centre;
        let number = self.stations.place_camp(|number| {
            // A slot key is far below `usize::MAX` on the 64-bit targets the server runs on.
            let gde_ref = dynamic_ref(namespace, number as usize);
            let member = |component, state| BundleMember { component, state };
            let mut members = vec![
                member(
                    CARD_COMPONENT,
                    MemberState::GdeMetadata(GdeMetadataReplicatedState {
                        asset: Some(AssetId {
                            id: slice.asset,
                            sub_id: DYNAMIC_SLICE_SUB_ID,
                        }),
                        gde_ref: Some(gde_ref),
                    }),
                ),
                member(SPELL_COMPONENT, MemberState::Spell(spell_state)),
            ];
            if slice.owned {
                let mut ownership = unowned(PERMISSION_RULE, "");
                ownership.owner = Some(owner);
                members.push(member(
                    OWNERSHIP_COMPONENT,
                    MemberState::Ownership(ownership),
                ));
            }
            members.extend(states);
            Station {
                gde_ref,
                position,
                station_type: String::new(),
                name: Vec::new(),
                members,
            }
        });
        let until_ms = area
            .ends_ms
            .max(area.starts_ms.saturating_add(SPELL_ENTITY_MIN_MS));
        self.spell_entities.push((number, until_ms));
    }

    /// One pass of `area` at `now_ms`: the enemies first in it take its damage row; the targets
    /// in it hold its effects, and with `ClearOnExit` the ones that left lose them.
    fn area_pass(
        &mut self,
        area: &mut SpellArea,
        now_ms: u64,
    ) -> Vec<Result<AttackOutcome, AttackRefusal>> {
        let radius = area.spell.radius_m.unwrap_or(POINT_RADIUS_M);
        let targets = area.spell.target_types.clone().unwrap_or_default();
        let effects = area.spell.status_effects.clone();
        let mut results = Vec::new();
        if ENEMY_TARGETS.contains(&targets.as_str()) {
            let inside: Vec<u64> = self
                .mob_states()
                .filter(|(_, at, state)| {
                    state.health > 0 && at.horizontal_distance(area.centre) <= radius
                })
                .map(|(mob, _, _)| mob)
                .collect();
            for mob in &inside {
                let first = !area.hit.contains(mob);
                if first {
                    area.hit.push(*mob);
                    let hurt = match area.spell.damage_row.as_deref() {
                        Some(row) => {
                            let result =
                                self.hit_paced(&area.caster, *mob, row, None, false, now_ms);
                            let hurt = matches!(result, Ok(AttackOutcome::Hit { .. }));
                            results.push(result);
                            hurt
                        }
                        None => true,
                    };
                    if hurt && !effects.is_empty() {
                        area.mobs_in.push(*mob);
                    }
                }
                if area.mobs_in.contains(mob) {
                    self.spell_effects_on_mob(*mob, &effects, area.source, !first);
                }
            }
            if area.spell.clear_on_exit {
                let left: Vec<u64> = area
                    .mobs_in
                    .iter()
                    .copied()
                    .filter(|mob| !inside.contains(mob))
                    .collect();
                self.clear_area(area, &left, &[]);
                area.mobs_in.retain(|mob| inside.contains(mob));
            }
        }
        if !effects.is_empty() && FRIEND_TARGETS.contains(&targets.as_str()) {
            let inside = if targets == SELF_TARGET {
                vec![area.caster_entity]
            } else {
                self.players_within(area.centre, radius)
            };
            for player in &inside {
                let held = area.players_in.contains(player);
                if !held {
                    area.players_in.push(*player);
                }
                self.spell_effects_on_player(
                    *player,
                    &effects,
                    (area.source, area.hit_amount),
                    held,
                );
            }
            if area.spell.clear_on_exit {
                let left: Vec<Entity> = area
                    .players_in
                    .iter()
                    .copied()
                    .filter(|player| !inside.contains(player))
                    .collect();
                self.clear_area(area, &[], &left);
                area.players_in.retain(|player| inside.contains(player));
            }
        }
        results
    }

    /// Takes the status effects of `area` off `mobs` and `players`.
    fn clear_area(&mut self, area: &SpellArea, mobs: &[u64], players: &[Entity]) {
        for mob in mobs {
            self.clear_spell_effects_on_mob(*mob, &area.spell.status_effects);
        }
        for player in players {
            self.clear_spell_effects_on_player(*player, &area.spell.status_effects);
        }
    }
}
