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
//! caster's turn and id), the caster's ownership and the entity's position when the slice holds
//! them, and the slice's other states, each on the slice's own component number. Every player
//! near sees it (Sacred Ground's circle). It goes when the area ends, and no sooner than
//! [`SPELL_ENTITY_MIN_MS`]. A slice with a state the server cannot fill spawns no entity.
//!
//! A creature's `CAGE-CastSpell` clip casts its spell row the same way
//! ([`SimWorld::cast_mob_spells`]): its enemies are the players, its friends the mobs (its self
//! heals and buffs). With `AIAimAtTarget` it opens on its target, within its `CastDistance`;
//! a spell that flies (a `RangedAttackProfile`) opens when its projectile gets there at the
//! clip's `ProjectileSpeed` (Inferred: the flight's own path is in the ranged attack database,
//! which the sim does not read). Its damage row is a row of the creature's damage table, and it
//! hits each player once, as a hit of the creature.

use bevy_ecs::entity::Entity;
use nw_bus::ConnectionRef;
use nw_content::player_abilities::SpellRow;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::buildables::{NO_OWNER_KIND, OwnerIdentification};
use nw_messages::spell_state::{NO_ATTACHMENT, SPELL_REF_LEN, SpellState};
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    AssetId, BundleMember, DYNAMIC_SLICE_SUB_ID, GdeMetadataReplicatedState, MemberState,
    PositionInTheWorldReplicatedState, Rotation, WorldPosition, raw_id_of_entity,
};
use nw_sim_core::{Position, mob_hit};

use super::abilities::{Caster, ENEMY_TARGETS, FRIEND_TARGETS, SELF_TARGET};
use super::attack::Pace;
use super::crafting::{CARD_COMPONENT, Station, dynamic_ref, unowned};
use super::{Player, SimWorld};
use crate::combat::{AttackOutcome, AttackRefusal};
use crate::mob_ai::{SpellCast, Strike};
use crate::mobs::{MobAttackIntent, MobReach};

/// The radius of a spell row without one: its target only (Inferred: a point spell hits what is
/// on the point).
const POINT_RADIUS_M: f32 = 1.0;
/// A spell entity stays at least this long, so that the entity of a one-pass spell shows
/// (Inferred: when retail takes an impact's entity away is Unknown).
const SPELL_ENTITY_MIN_MS: u64 = 1_000;
/// Names the ids this server makes for spell entities.
const SPELL_ENTITY_NAMESPACE: u64 = 0x7370_656c_6c65_6e74;
/// Retail: `m_aoeRadiusScaling` 1 and permission rule 1 in the spell groups read.
const RADIUS_SCALE: f32 = 1.0;
const PERMISSION_RULE: u8 = 1;
/// A spell area reaches by its radius, not by a swing.
const AREA_REACH: MobReach = MobReach {
    lunge_max_m: 0.0,
    lunge_turn_rad: 0.0,
    reach_m: 0.0,
};
/// The heal base of a mob's spell is its plain hit, before any armor (Inferred: a player's is
/// its weapon's damage, [`SimWorld::spell_heal_base`]).
const NO_ARMOR_SHARE: f64 = 0.0;
/// Milliseconds per second of a flight.
const MS_PER_S: f32 = 1_000.0;

/// Who cast a spell area.
#[derive(Debug, Clone, PartialEq)]
enum AreaCaster {
    /// A player of this sim.
    Player {
        connection: ConnectionRef,
        entity: Entity,
    },
    /// A mob; `table` is the unique id of its damage table.
    Mob { table: Option<String> },
}

/// A spell area of a player or a mob.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SpellArea {
    caster: AreaCaster,
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
    /// The mobs (of a player's area) or the players (of a mob's) its damage row hit.
    hit: Vec<u64>,
    /// The mobs that hold its effects.
    mobs_in: Vec<u64>,
    /// The players that hold its effects.
    players_in: Vec<Entity>,
    /// The PvP foes of a player's area that hold its effects.
    foes_in: Vec<Entity>,
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
        Self::of(
            AreaCaster::Player {
                connection: caster.connection.clone(),
                entity: caster.entity,
            },
            caster.number,
            spell,
            (centre, caster.yaw()),
            starts_ms,
            hit_amount,
        )
    }

    /// The area of `spell` of mob `mob`, whose damage table is `table`, at `place` (centre and
    /// yaw), from `starts_ms`; `hit_amount` is the mob's plain hit.
    pub(super) fn of_mob(
        mob: u64,
        table: Option<String>,
        spell: &SpellRow,
        place: (Position, f32),
        starts_ms: u64,
        hit_amount: f64,
    ) -> Self {
        Self::of(
            AreaCaster::Mob { table },
            mob,
            spell,
            place,
            starts_ms,
            hit_amount,
        )
    }

    fn of(
        caster: AreaCaster,
        source: u64,
        spell: &SpellRow,
        (centre, yaw): (Position, f32),
        starts_ms: u64,
        hit_amount: f64,
    ) -> Self {
        Self {
            caster,
            source,
            spell: spell.clone(),
            centre,
            starts_ms,
            ends_ms: starts_ms.saturating_add(spell.lasting_ms().unwrap_or_default()),
            hit_amount,
            hit: Vec::new(),
            mobs_in: Vec::new(),
            players_in: Vec::new(),
            foes_in: Vec::new(),
            yaw,
            shown: false,
        }
    }
}

/// Where and when the spell `cast` of a mob at `at` opens at `now_ms`: on the mob, or with
/// `AIAimAtTarget` on its target, no farther than the spell's `CastDistance`; a spell that
/// `flies` opens when its projectile gets there.
fn mob_spell_place(
    at: Position,
    cast: &SpellCast,
    spell: &SpellRow,
    flies: bool,
    now_ms: u64,
) -> (Position, u64) {
    let aimed = cast
        .target
        .filter(|_| cast.aim && spell.target_types.as_deref() != Some(SELF_TARGET));
    let Some((_, target_at)) = aimed else {
        return (at, now_ms);
    };
    let distance = at.horizontal_distance(target_at);
    let reach = spell
        .cast_distance_m
        .filter(|reach| *reach > 0.0)
        .map_or(distance, |reach| reach.min(distance));
    let centre = if distance > reach {
        let share = reach / distance;
        Position {
            x: at.x + (target_at.x - at.x) * share,
            y: at.y + (target_at.y - at.y) * share,
            z: target_at.z,
        }
    } else {
        target_at
    };
    if !flies || cast.speed_mps <= 0.0 {
        return (centre, now_ms);
    }
    // A flight of a few seconds.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a flight of a few seconds"
    )]
    let flight_ms = (reach / cast.speed_mps * MS_PER_S).round() as u64;
    (centre, now_ms.saturating_add(flight_ms))
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
    /// Casts the spells the mobs' `CAGE-CastSpell` clips cast this tick: each opens its area
    /// ([`mob_spell_place`]). A spell name without a row is left out (Observed: 21 of the 765
    /// names of the creature clips have none).
    pub(super) fn cast_mob_spells(&mut self, now_ms: u64) {
        for (mob, cast) in self.mobs.take_casts() {
            let Some(spell) = self.combat.data.mob_spell(&cast.spell).cloned() else {
                tracing::debug!(mob, spell = %cast.spell, "mob spell without a row");
                continue;
            };
            let Some(state) = self.mobs.get(mob) else {
                continue;
            };
            let at = state.position;
            let hit_amount = f64::from(mob_hit(&state.attack, NO_ARMOR_SHARE).amount);
            let yaw = self.mobs.facing(mob).unwrap_or_default();
            let (centre, starts_ms) = mob_spell_place(at, &cast, &spell.row, spell.flies, now_ms);
            self.open_spell_area(SpellArea::of_mob(
                mob,
                cast.table,
                &spell.row,
                (centre, yaw),
                starts_ms,
                hit_amount,
            ));
        }
    }

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
            "spell area"
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
                self.clear_area(&area, &[], &area.foes_in);
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

    /// Spawns the spell entity of `area`, when its spell has a slice that the server can fill
    /// and its caster is here.
    fn spawn_spell_entity(&mut self, area: &SpellArea) {
        let spell = lowercase_crc32(&area.spell.id);
        let Some(slice) = self.content.spell_slices.of(spell).cloned() else {
            return;
        };
        if !slice.unfilled.is_empty() {
            tracing::debug!(
                spell = %area.spell.id,
                unfilled = ?slice.unfilled,
                "spell entity not spawned: states without bytes"
            );
            return;
        }
        let Some(start) = WorldPosition::from_metres(area.centre.x, area.centre.y, area.centre.z)
        else {
            return;
        };
        // A mob's spell has no owner (Inferred: the ownership names a player's character).
        let owner = match &area.caster {
            AreaCaster::Player { entity, .. } => {
                let Some(owner) =
                    self.world
                        .get::<Player>(*entity)
                        .map(|player| OwnerIdentification {
                            first: player.character.to_string().into_bytes(),
                            second: player.name.clone().into_bytes(),
                            kind: NO_OWNER_KIND,
                        })
                else {
                    return;
                };
                Some(owner)
            }
            AreaCaster::Mob { .. } => None,
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
        let rotation = Rotation::of_quaternion([0.0, 0.0, half.sin(), half.cos()]);
        let spell_state = SpellState {
            spell: Some(spell),
            start: Some(start),
            start_rotation: Some(rotation),
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
                member(slice.spell_component, MemberState::Spell(spell_state)),
            ];
            // Retail: the same place and turn as the spell state's start (the upgraded Flail
            // Slam, 59 of 59 groups of the survey).
            if let Some(component) = slice.position_component {
                members.push(member(
                    component,
                    MemberState::Position(PositionInTheWorldReplicatedState {
                        position: Some(start),
                        rotation: Some(rotation),
                        scale: None,
                    }),
                ));
            }
            if let (Some(component), Some(owner)) = (slice.ownership_component, owner) {
                let mut ownership = unowned(PERMISSION_RULE, "");
                ownership.owner = Some(owner);
                members.push(member(component, MemberState::Ownership(ownership)));
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
    /// in it hold its effects, and with `ClearOnExit` the ones that left lose them. The hits of
    /// a mob's area on players go to [`SimWorld::take_spell_area_mob_hits`].
    fn area_pass(
        &mut self,
        area: &mut SpellArea,
        now_ms: u64,
    ) -> Vec<Result<AttackOutcome, AttackRefusal>> {
        match area.caster.clone() {
            AreaCaster::Player { connection, entity } => {
                self.player_area_pass(area, (&connection, entity), now_ms)
            }
            AreaCaster::Mob { table } => {
                self.mob_area_pass(area, table.as_deref(), now_ms);
                Vec::new()
            }
        }
    }

    /// A pass of the area of the player `caster` (its connection and entity): the mobs and its
    /// PvP foes ([`SimWorld::pvp_foes`]) are its enemies, the other players its friends.
    fn player_area_pass(
        &mut self,
        area: &mut SpellArea,
        (connection, caster): (&ConnectionRef, Entity),
        now_ms: u64,
    ) -> Vec<Result<AttackOutcome, AttackRefusal>> {
        let radius = area.spell.radius_m.unwrap_or(POINT_RADIUS_M);
        let targets = area.spell.target_types.clone().unwrap_or_default();
        let effects = area.spell.status_effects.clone();
        let mut results = Vec::new();
        if ENEMY_TARGETS.contains(&targets.as_str()) {
            results.extend(self.foe_area_pass(area, (connection, caster), now_ms));
            let inside = self.living_mobs_within(area.centre, radius);
            for mob in &inside {
                let first = !area.hit.contains(mob);
                if first {
                    area.hit.push(*mob);
                    let hurt = match area.spell.damage_row.as_deref() {
                        Some(row) => {
                            let result = self.hit_paced(
                                connection,
                                *mob,
                                row,
                                None,
                                Pace::of(false),
                                now_ms,
                            );
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
                self.clear_area(area, &left(&area.mobs_in, &inside), &[]);
                area.mobs_in.retain(|mob| inside.contains(mob));
            }
        }
        if !effects.is_empty() && FRIEND_TARGETS.contains(&targets.as_str()) {
            let inside = if targets == SELF_TARGET {
                vec![caster]
            } else {
                let mut inside = self.players_within(area.centre, radius);
                inside.retain(|player| !self.pvp_foes(caster, *player));
                inside
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
                self.clear_area(area, &[], &left(&area.players_in, &inside));
                area.players_in.retain(|player| inside.contains(player));
            }
        }
        results
    }

    /// The part of a pass of the area of the player `caster` on its PvP foes inside it: each
    /// takes the damage row once, and holds the effects while it stands in the area, as a mob
    /// does.
    fn foe_area_pass(
        &mut self,
        area: &mut SpellArea,
        (connection, caster): (&ConnectionRef, Entity),
        now_ms: u64,
    ) -> Vec<Result<AttackOutcome, AttackRefusal>> {
        let radius = area.spell.radius_m.unwrap_or(POINT_RADIUS_M);
        let effects = area.spell.status_effects.clone();
        let inside: Vec<(u64, Entity)> = self
            .foe_places(caster)
            .into_iter()
            .filter(|(_, at, _)| at.horizontal_distance(area.centre) <= radius)
            .filter_map(|(number, _, _)| Some((number, self.player_by_number(number)?)))
            .collect();
        let mut results = Vec::new();
        for (number, foe) in &inside {
            let first = !area.hit.contains(number);
            if first {
                area.hit.push(*number);
                let hurt = match area.spell.damage_row.as_deref() {
                    Some(row) => {
                        let result =
                            self.hit_paced(connection, *number, row, None, Pace::of(false), now_ms);
                        let hurt = matches!(result, Ok(AttackOutcome::Hit { .. }));
                        results.push(result);
                        hurt
                    }
                    None => true,
                };
                if hurt && !effects.is_empty() {
                    area.foes_in.push(*foe);
                }
            }
            if area.foes_in.contains(foe) {
                self.spell_effects_on_player(
                    *foe,
                    &effects,
                    (area.source, area.hit_amount),
                    !first,
                );
            }
        }
        if area.spell.clear_on_exit {
            let here: Vec<Entity> = inside.iter().map(|(_, foe)| *foe).collect();
            self.clear_area(area, &[], &left(&area.foes_in, &here));
            area.foes_in.retain(|foe| here.contains(foe));
        }
        results
    }

    /// A pass of the area of a mob, whose damage table is `table`: the players are its
    /// enemies, the mobs its friends (a self heal or buff holds on the mob alone).
    fn mob_area_pass(&mut self, area: &mut SpellArea, table: Option<&str>, now_ms: u64) {
        let radius = area.spell.radius_m.unwrap_or(POINT_RADIUS_M);
        let targets = area.spell.target_types.clone().unwrap_or_default();
        let effects = area.spell.status_effects.clone();
        if ENEMY_TARGETS.contains(&targets.as_str()) {
            let inside = self.players_within(area.centre, radius);
            for player in &inside {
                let Some(number) = self
                    .world
                    .get::<Player>(*player)
                    .map(|player| player.number)
                else {
                    continue;
                };
                let first = !area.hit.contains(&number);
                if first {
                    area.hit.push(number);
                    let row = area.spell.damage_row.as_deref();
                    let hurt = match row.zip(table) {
                        Some((row, table)) => {
                            let outcome = self.mob_spell_hit(area, number, (table, row), now_ms);
                            let hurt = matches!(outcome, Some(AttackOutcome::Hit { .. }));
                            self.spell_area_mob_hits.extend(outcome);
                            hurt
                        }
                        None => row.is_none(),
                    };
                    if hurt && !effects.is_empty() {
                        area.players_in.push(*player);
                    }
                }
                if area.players_in.contains(player) {
                    self.spell_effects_on_player(
                        *player,
                        &effects,
                        (area.source, area.hit_amount),
                        !first,
                    );
                }
            }
            if area.spell.clear_on_exit {
                self.clear_area(area, &[], &left(&area.players_in, &inside));
                area.players_in.retain(|player| inside.contains(player));
            }
        }
        if !effects.is_empty() && FRIEND_TARGETS.contains(&targets.as_str()) {
            let inside = if targets == SELF_TARGET {
                vec![area.source]
            } else {
                self.living_mobs_within(area.centre, radius)
            };
            for mob in &inside {
                let held = area.mobs_in.contains(mob);
                if !held {
                    area.mobs_in.push(*mob);
                }
                self.spell_effects_on_mob(*mob, &effects, area.source, held);
            }
            if area.spell.clear_on_exit {
                self.clear_area(area, &left(&area.mobs_in, &inside), &[]);
                area.mobs_in.retain(|mob| inside.contains(mob));
            }
        }
    }

    /// The hit of the damage row `(table, row)` of the mob area `area` on player `target`: a
    /// hit of the mob that cast it, which the player's block and dodge take as any other.
    fn mob_spell_hit(
        &mut self,
        area: &SpellArea,
        target: u64,
        (table, row): (&str, &str),
        now_ms: u64,
    ) -> Option<AttackOutcome> {
        let intent = MobAttackIntent {
            mob: area.source,
            target,
            target_at: area.centre,
            mob_move: None,
            strike: Some(Strike {
                table: table.to_owned(),
                row: row.to_owned(),
                weapon: None,
                reach: AREA_REACH,
                ranged: None,
            }),
        };
        let defender = self.player_by_number(target);
        let (amount, style, _) = self.mob_intent_hit(&intent, None, defender)?;
        self.land_mob_hit(target, area.source, amount, style, now_ms)
    }

    /// The hits of the mobs' spell areas on players since the last call.
    pub(super) fn take_spell_area_mob_hits(&mut self) -> Vec<AttackOutcome> {
        std::mem::take(&mut self.spell_area_mob_hits)
    }

    /// The living mobs within `radius` of `centre`.
    fn living_mobs_within(&self, centre: Position, radius: f32) -> Vec<u64> {
        self.mob_states()
            .filter(|(_, at, state)| state.health > 0 && at.horizontal_distance(centre) <= radius)
            .map(|(mob, _, _)| mob)
            .collect()
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

/// The holders of `held` that are not `inside`: the ones that left an area.
fn left<T: Copy + PartialEq>(held: &[T], inside: &[T]) -> Vec<T> {
    held.iter()
        .copied()
        .filter(|holder| !inside.contains(holder))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOB_AT: Position = Position {
        x: 0.0,
        y: 0.0,
        z: 10.0,
    };
    const TARGET_X: f32 = 20.0;
    const REACH_M: f32 = 12.0;
    const SPEED_MPS: f32 = 6.0;
    const NOW: u64 = 5_000;
    const TARGET: u64 = 7;

    fn cast(aim: bool) -> SpellCast {
        SpellCast {
            spell: "Dryad_Bomb".to_owned(),
            target: Some((
                TARGET,
                Position {
                    x: TARGET_X,
                    y: 0.0,
                    z: 11.0,
                },
            )),
            aim,
            speed_mps: SPEED_MPS,
            table: None,
        }
    }

    fn bomb(targets: &str) -> SpellRow {
        SpellRow {
            id: "Dryad_Bomb".to_owned(),
            types: None,
            shape: None,
            radius_m: Some(3.0),
            cast_distance_m: Some(REACH_M),
            duration_ms: Some(25_000),
            damage_row: None,
            status_effects: Vec::new(),
            target_types: Some(targets.to_owned()),
            mana_cost: None,
            projectile: None,
            clear_on_exit: false,
            clear_on_end: false,
            secondary: None,
        }
    }

    #[test]
    fn an_aimed_mob_spell_opens_toward_its_target_within_its_cast_distance_after_its_flight() {
        let (centre, opens_ms) = mob_spell_place(MOB_AT, &cast(true), &bomb("Enemy"), true, NOW);
        assert!((centre.x - REACH_M).abs() < 1e-4, "{centre:?}");
        assert_eq!(opens_ms, NOW + 2_000, "12 m at 6 m/s");
        let (_, at_once) = mob_spell_place(MOB_AT, &cast(true), &bomb("Enemy"), false, NOW);
        assert_eq!(at_once, NOW, "a spell that does not fly opens at once");
    }

    #[test]
    fn a_self_spell_or_an_unaimed_one_opens_on_the_mob() {
        for (spell, aim) in [(bomb(SELF_TARGET), true), (bomb("Enemy"), false)] {
            let (centre, opens_ms) = mob_spell_place(MOB_AT, &cast(aim), &spell, true, NOW);
            assert_eq!((centre, opens_ms), (MOB_AT, NOW));
        }
    }
}
