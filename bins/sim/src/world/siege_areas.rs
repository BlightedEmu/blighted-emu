// SPDX-License-Identifier: AGPL-3.0-or-later
//! The uses of the fire barrel droppers and the horns of resilience of a war fort, the barrels
//! they drop, and the siege areas that open where a barrel or an explosive shell lands or where
//! a horn blows (siege structure study 2026-11-12; the slice values from
//! [`nw_content::war_forts::StructureKind`]: `trigger`, `use_timers`, `drop`, `area`, and
//! [`nw_content::war_forts::TurretKind::area`]).
//!
//! **A dropper's use** (Observed, `joining_defending_war_2` 124.671 s, the T1 dropper of
//! territory 11, and its uses at 165.245, 180.0 and 318.8 s; `invasion` 1336.4 s): the client's
//! interaction of kind [`nw_messages::client_input::TRIGGER_KIND`] with the name `Dropper_T<n>_FireBarrel` is accepted
//! (3743, 46 ms later); the dropper's ownership names the player as its owner (only the `owner`
//! child is sent; the full ownership keeps it, 165.349 s), its interact shows one interactor,
//! and its pour timer (`ProjectileSpawnDelay`, 0.3 s) starts; the 3135 of its trigger component
//! goes out 42 ms after the accept. When the pour ends (125.039 s) the timer completes, the
//! barrel's projectile entity comes (card, the company's ownership with the player as owner,
//! the projectile state: the spawn point, no velocity, the player, the ranged attack and the
//! damage row of the spawner) and the 2900 of the spawner goes out (125.058 s). The interactor
//! goes 1.49 s after the accept ([`DROPPER_USE_MS`], 126.207 s). The barrel falls straight down
//! and lands 1.47 s after it came (126.507 s: its state names the impact point and the collided
//! flag); its entity goes 1.07 s after the landing (127.576 s; [`super::shot_entity`]'s end).
//! Inferred: a barrel falls with the spawner's `m_projectileGravity` from
//! [`BARREL_AHEAD_M`] ahead of the dropper and [`BARREL_UP_M`] above it to the floor below (the
//! fit of 125.039 s; where the level has no height data, to the height of the dropper's own
//! place); only a player of the dropper's side uses it, one at a time, never while it is broken.
//!
//! **A horn's blow** (Observed, `invasion` 95.691 s, a T2 horn blown by another player of the
//! fort's side): the horn's ownership names the player as its owner, its interact turns off
//! with one interactor, and its four blow timers (`BlowDelay`: 7, 7, 10 and 90 s) start; the
//! 3135 of its trigger component goes out (95.707 s) and every player of the side near it holds
//! `HornOfResilience_T2` from 95.758 s (the own player's local map: 9.5 s left). The interactor
//! goes 98 ms after the blow ([`HORN_USE_MS`]); the two 7 s timers complete at 102.693 s with
//! the interact on again, the 10 s timer at 105.689 s and the 90 s timer at 185.687 s, with no
//! other member changing. Inferred: a horn can be blown again once its interact is on again
//! (its shortest blow timer).
//!
//! **A siege area** ([`AreaRun`]): the area slice of the structure (`<slice>_aoe`) spawns where
//! the barrel or the shell landed, or on the horn's place, with its card, the company's
//! ownership with the player as owner (rule 4, the player's war raid: Observed 536 on the
//! barrel's area), its position (the place and the structure's turn) and its timers started
//! (Observed, `joining_defending_war_2` 126.507 s: the barrel's area at the impact point with
//! the dropper's turn and its timers 4, 15 and 4 s; `invasion` 96.256 s: the horn's area on the
//! horn with its 1 s timer). A timer that ends completes (185.773 s, the 4 s timers), and the
//! area goes when its longest timer ends (196.771 s, 15.0 s after it opened). A damage volume
//! (the barrel's) hits every foe in its radius when it opens and every `m_damageRate` while it
//! lasts; an explosion (the shell's) hits once when it opens; an area cast (the horn's) gives
//! its status effects to the players of its side in its radius when it opens. Inferred: a hit
//! takes the slice's `m_baseDamage` times the row's coefficient (as a turret's shot takes the
//! level's base damage times its row's), and it meets what a turret's shot of the same side
//! meets (the mobs, the player's foes, the whole turrets of the other side).

use bevy_ecs::entity::Entity;
use nw_content::war_forts::{
    AreaDamage, FortStructure, SiegeArea, SiegeTimer, StructureKind, StructureRole,
};
use nw_messages::buildables::{NO_OWNER_KIND, OwnerIdentification};
use nw_messages::combat_effects::CombatEffectMsg;
use nw_messages::facet::facet_entity;
use nw_messages::projectile_state::ProjectileState;
use nw_messages::trigger_entity::SyncExecuteEntitiesMsg;
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    AssetId, BundleMember, DYNAMIC_SLICE_SUB_ID, DelayedEventState, FacetHeader,
    GdeMetadataReplicatedState, InteractState, InteractionRequest, MemberState, OwnershipState,
    PositionInTheWorldReplicatedState, Rotation, ServerMessage, WorldPosition,
};
use nw_sim_core::Position;

use super::crafting::{CARD_COMPONENT, Station, dynamic_ref};
use super::shot_entity::ProjectileSpawn;
use super::siege_structures::{NO_INTERACTORS, StructureRun};
use super::turrets::{ShotTarget, USE_ACCEPTED, USE_REFUSED};
use super::war_fight::{WarFight, replace_member};
use super::{Player, SimWorld, gather_answer, send_now};
use crate::combat::{AttackOutcome, AttackRefusal};
use crate::mobs::Capsule;

/// How long after the accept a dropper's interactor goes, ms (Observed: 124.717 -> 126.207 s).
const DROPPER_USE_MS: u64 = 1_490;
/// How long after the accept a horn's interactor goes, ms (Observed: 95.691 -> 95.789 s).
const HORN_USE_MS: u64 = 98;
/// Where a barrel comes from: ahead of the dropper along its facing and above its place, metres
/// (Observed fit, `joining_defending_war_2` 125.039 s: the T1 dropper at (13680.33, 6332.66)
/// facing 40°, its barrel 1.79 m west and 2.13 m north of it and 0.58 m higher: 2.79 m ahead,
/// 0.00 m aside; Inferred the same for the T2 and T3 droppers).
const BARREL_AHEAD_M: f32 = 2.79;
const BARREL_UP_M: f32 = 0.58;
/// The ownership of an area and of a barrel (Observed on the barrel's area of
/// `joining_defending_war_2` 126.507 s: rule 4, faction 0, no PvP or FFA flag, options 0).
const AREA_PERMISSION_RULE: u8 = 4;
const AREA_FACTION: u8 = 0;
const AREA_PERMISSION_OPTIONS: u8 = 0;
/// The raid of a user with no war raid seat.
const NO_RAID: u64 = 0;
/// The interactors of a structure in use.
const ONE_USER: u32 = 1;
/// The potency of an area's status effect (Observed: potency 1 on `HornOfResilience_T2`).
const EFFECT_POTENCY: f64 = 1.0;
/// The namespace of the `GdeRef`s of siege areas ("siegeare").
const AREA_NAMESPACE: u64 = 0x7369_6567_6561_7265;
/// The delay a completed timer shows (Observed: `total_delay_s` 0 with the completed flag).
const COMPLETED_DELAY_S: f32 = 0.0;
const MS_PER_SECOND: f32 = 1_000.0;
const TWICE: f32 = 2.0;

/// A use of a dropper or a horn in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StructureUse {
    /// The user and its number.
    entity: Entity,
    number: u64,
    /// Sim ms its interactor goes, and whether it went.
    release_ms: u64,
    released: bool,
    /// Sim ms a dropper's pour ends and its barrel falls; `None` once it fell (and for a horn).
    pour_ms: Option<u64>,
}

/// A running timer of a structure or an area: its component and the sim ms it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RunningTimer {
    component: u64,
    ends_ms: u64,
}

impl RunningTimer {
    fn of(timer: &SiegeTimer, now_ms: u64) -> Self {
        Self {
            component: timer.component,
            ends_ms: now_ms.saturating_add(timer.delay_ms()),
        }
    }
}

/// The trigger use state of one structure (module doc).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct UseState {
    using: Option<StructureUse>,
    timers: Vec<RunningTimer>,
    /// Sim ms a blown horn's interact is on again.
    enabled_at_ms: Option<u64>,
}

/// Who opened an area: the side, the player and its war raid, and the turn of the structure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AreaSource {
    side: u8,
    number: u64,
    raid: u64,
    rotation: Option<Rotation>,
}

/// A barrel on its way down.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BarrelDrop {
    /// The slot key of its projectile entity.
    entity: u64,
    land_ms: u64,
    at: Position,
    area: SiegeArea,
    source: AreaSource,
}

/// An area that opens at the next tick: a blown horn's.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OpeningArea {
    area: SiegeArea,
    centre: Position,
    source: AreaSource,
}

/// An open siege area (module doc).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AreaRun {
    /// Its slot key.
    number: u64,
    area: SiegeArea,
    source: AreaSource,
    centre: Position,
    ends_ms: u64,
    /// Sim ms of a lasting area's next hit.
    next_hit_ms: Option<u64>,
    timers: Vec<RunningTimer>,
}

/// The `DelayedEvent` member of a timer that starts with `delay_s` (`completed` as sent).
fn timer_member(component: u64, delay_s: f32, completed: Option<bool>) -> BundleMember {
    BundleMember {
        component,
        state: MemberState::DelayedEvent(DelayedEventState {
            total_delay_s: Some(delay_s),
            completed,
        }),
    }
}

/// The member of a timer that ended.
fn completed_member(component: u64) -> BundleMember {
    timer_member(component, COMPLETED_DELAY_S, Some(true))
}

/// The turn of `structure`: the rotation of its retail place member.
pub(super) fn rotation_of(structure: &FortStructure) -> Option<Rotation> {
    structure
        .members
        .iter()
        .find_map(|bytes| match BundleMember::read_from(bytes).ok()?.0.state {
            MemberState::Position(state) => state.rotation,
            _ => None,
        })
}

/// Whether a body of `capsule` at `at` stands in a sphere of `radius_m` around `centre`.
fn within(centre: Position, radius_m: f32, at: Position, capsule: Capsule) -> bool {
    centre.horizontal_distance(at) <= radius_m + capsule.radius_m
        && centre.z >= at.z - radius_m
        && centre.z <= at.z + capsule.height_m + radius_m
}

/// The health of a hit of an area of `damage` whose row has `coefficient` (module doc).
fn area_hit_amount(damage: &AreaDamage, coefficient: f64) -> u32 {
    // A hit of a few thousand points at most.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a hit of thousands"
    )]
    let amount = (f64::from(damage.base_damage) * coefficient)
        .round()
        .max(0.0) as u32;
    amount
}

/// The time a body falls `height_m` from rest under `gravity_mps2`, ms.
fn fall_ms(height_m: f32, gravity_mps2: f32) -> u64 {
    if height_m <= 0.0 || gravity_mps2 <= 0.0 {
        return 0;
    }
    // A fall of a few seconds at most.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a fall of seconds"
    )]
    let ms = ((TWICE * height_m / gravity_mps2).sqrt() * MS_PER_SECOND).round() as u64;
    ms
}

impl StructureRun {
    /// Its full interact member at `now_ms`: one interactor until its user is let go, turned
    /// off while a blown horn waits.
    fn use_interact(&self, now_ms: u64) -> Option<BundleMember> {
        let interactors = if self
            .uses
            .using
            .is_some_and(|using| !using.released && now_ms < using.release_ms)
        {
            ONE_USER
        } else {
            NO_INTERACTORS
        };
        let waiting = self.uses.enabled_at_ms.is_some_and(|at| now_ms < at);
        let mut member = self.body.interact_member(interactors)?;
        if waiting && let MemberState::Interact(state) = &mut member.state {
            state.enabled = Some(false);
        }
        Some(member)
    }

    /// The interact change `state` of its interact component.
    fn interact_change(&self, state: InteractState) -> Option<BundleMember> {
        Some(BundleMember {
            component: self.body.structure.interact_component.or_else(|| {
                self.body
                    .interact_member(NO_INTERACTORS)
                    .map(|member| member.component)
            })?,
            state: MemberState::Interact(state),
        })
    }
}

impl SimWorld {
    /// Sends `partial` to the viewers of `run` and puts `full` (and its interact at `now_ms`)
    /// into its spawn members.
    fn sync_use(
        &mut self,
        run: &StructureRun,
        full: Vec<BundleMember>,
        partial: Vec<BundleMember>,
        now_ms: u64,
    ) {
        let interact = run.use_interact(now_ms);
        self.stations.change_camp(
            run.body.number,
            |members| {
                for member in full.iter().chain(interact.iter()) {
                    replace_member(members, member);
                }
            },
            partial,
        );
    }

    /// The ownership of a siege structure, area or barrel of `side` with player `user` as its
    /// owner in `raid` (module doc).
    fn owned_ownership(
        &self,
        fight: &WarFight,
        side: u8,
        user: Entity,
        raid: u64,
    ) -> OwnershipState {
        let owner = self
            .world
            .get::<Player>(user)
            .map(|player| OwnerIdentification {
                first: player.character.hyphenated().to_string().into_bytes(),
                second: player.name.clone().into_bytes(),
                kind: NO_OWNER_KIND,
            });
        OwnershipState {
            owner,
            permission_rule: Some(AREA_PERMISSION_RULE),
            raid_id: Some(raid),
            faction_type: Some(AREA_FACTION),
            permission_options: Some(AREA_PERMISSION_OPTIONS),
            ..self.structure_ownership(fight, side)
        }
    }

    /// The source of an area of `side` that player `number` opened from a structure turned
    /// `rotation`: the player's war raid with it.
    pub(super) fn area_source(
        &mut self,
        side: u8,
        number: u64,
        rotation: Option<Rotation>,
    ) -> AreaSource {
        let raid = self
            .player_by_number(number)
            .map_or(NO_RAID, |entity| self.war_raid_of(entity));
        AreaSource {
            side,
            number,
            raid,
            rotation,
        }
    }

    /// The war raid of player `entity`.
    fn war_raid_of(&self, entity: Entity) -> u64 {
        self.world
            .get::<Player>(entity)
            .and_then(|player| self.wars.raid_seat(player.character))
            .map_or(NO_RAID, |(raid, _)| raid.raid)
    }

    /// The interaction `request` of kind [`nw_messages::client_input::TRIGGER_KIND`] of player `entity` (`number`) on a
    /// dropper or a horn of this sim's war (module doc): `false` when it names none.
    pub(super) fn structure_trigger(
        &mut self,
        entity: Entity,
        number: u64,
        request: InteractionRequest,
        name: &[u8],
        now_ms: u64,
    ) -> bool {
        let side = self.war_side(entity);
        let raid = self.war_raid_of(entity);
        let Some(connection) = self
            .world
            .get::<Player>(entity)
            .map(|player| player.connection.clone())
        else {
            return false;
        };
        let content = std::sync::Arc::clone(&self.content);
        self.with_turrets(|world, fight| {
            let Some(index) = fight
                .structures
                .iter()
                .position(|run| run.body.structure.gde_ref == request.target)
            else {
                return false;
            };
            let Some(kind) = content
                .war_forts
                .structure_kind(&fight.structures[index].body.structure.asset)
                .filter(|kind| kind.trigger.is_some())
            else {
                return false;
            };
            let run = &fight.structures[index];
            let free = run.uses.using.is_none()
                && !run.body.broken
                && run.uses.enabled_at_ms.is_none_or(|at| now_ms >= at);
            let usable = match kind.role {
                StructureRole::FireBarrelDropper => kind.drop.is_some() && kind.area.is_some(),
                StructureRole::HornOfResilience => kind.area.is_some(),
                _ => false,
            };
            if !free || !usable || side != Some(run.body.structure.side) {
                let refused = gather_answer(number, request, USE_REFUSED, name);
                send_now(&mut world.combat_messages, &connection, &[refused]);
                return true;
            }
            let accepted = gather_answer(number, request, USE_ACCEPTED, name);
            send_now(&mut world.combat_messages, &connection, &[accepted]);
            let mut run = fight.structures[index].clone();
            world.start_use(fight, &mut run, kind, (entity, number, raid), now_ms);
            fight.structures[index] = run;
            true
        })
        .unwrap_or(false)
    }

    /// Starts the use of `run` of `kind` by `user` (entity, number, war raid) at `now_ms`
    /// (module doc).
    fn start_use(
        &mut self,
        fight: &mut WarFight,
        run: &mut StructureRun,
        kind: &StructureKind,
        (entity, number, raid): (Entity, u64, u64),
        now_ms: u64,
    ) {
        let horn = kind.role == StructureRole::HornOfResilience;
        let structure = &run.body.structure;
        let side = structure.side;
        let facet = structure.facet_entity();
        let place = run.body.place();
        let rotation = rotation_of(structure);
        let ownership = self.owned_ownership(fight, side, entity, raid);
        let mut timers: Vec<&SiegeTimer> = kind.use_timers.iter().collect();
        let pour = kind.drop.as_ref().map(|drop| &drop.pour);
        timers.extend(pour);
        run.uses.timers = timers
            .iter()
            .map(|timer| RunningTimer::of(timer, now_ms))
            .collect();
        run.uses.using = Some(StructureUse {
            entity,
            number,
            release_ms: now_ms.saturating_add(if horn { HORN_USE_MS } else { DROPPER_USE_MS }),
            released: false,
            pour_ms: pour.map(|timer| now_ms.saturating_add(timer.delay_ms())),
        });
        if horn {
            run.uses.enabled_at_ms = kind
                .use_timers
                .iter()
                .map(SiegeTimer::delay_ms)
                .min()
                .map(|delay| now_ms.saturating_add(delay));
        }
        let started: Vec<BundleMember> = timers
            .iter()
            .map(|timer| timer_member(timer.component, timer.delay_s, None))
            .collect();
        let mut full = started.clone();
        let mut partial = started;
        partial.extend(run.interact_change(InteractState {
            enabled: horn.then_some(false),
            has_interactors: Some(ONE_USER),
        }));
        if let Some(component) = structure.ownership_component {
            full.push(BundleMember {
                component,
                state: MemberState::Ownership(ownership.clone()),
            });
            partial.push(BundleMember {
                component,
                state: MemberState::Ownership(OwnershipState {
                    owner: ownership.owner,
                    ..OwnershipState::default()
                }),
            });
        }
        self.sync_use(run, full, partial, now_ms);
        if let Some(trigger) = kind.trigger {
            let message = ServerMessage::SyncExecuteEntities(SyncExecuteEntitiesMsg {
                header: FacetHeader {
                    target: trigger.target(facet),
                    entity: facet,
                },
            });
            self.send_to_all(&[message]);
        }
        if horn && let Some(area) = &kind.area {
            fight.opening.push(OpeningArea {
                area: area.clone(),
                centre: place,
                source: AreaSource {
                    side,
                    number,
                    raid,
                    rotation,
                },
            });
        }
    }

    /// One tick of the uses, barrels and areas of `fight` at sim ms `now_ms` (module doc); the
    /// hits go into `outcomes`.
    pub(super) fn tick_siege_uses(
        &mut self,
        fight: &mut WarFight,
        now_ms: u64,
        outcomes: &mut Vec<Result<AttackOutcome, AttackRefusal>>,
    ) {
        let content = std::sync::Arc::clone(&self.content);
        for index in 0..fight.structures.len() {
            let mut run = fight.structures[index].clone();
            if let Some(kind) = content.war_forts.structure_kind(&run.body.structure.asset) {
                self.tick_use(fight, &mut run, kind, now_ms);
            }
            fight.structures[index] = run;
        }
        for opening in std::mem::take(&mut fight.opening) {
            self.open_siege_area(
                fight,
                &opening.area,
                opening.centre,
                opening.source,
                now_ms,
                outcomes,
            );
        }
        let (landed, falling): (Vec<_>, Vec<_>) = std::mem::take(&mut fight.drops)
            .into_iter()
            .partition(|drop| now_ms >= drop.land_ms);
        fight.drops = falling;
        for drop in landed {
            self.shot_entity_impact(drop.entity, None, drop.at, now_ms);
            self.open_siege_area(fight, &drop.area, drop.at, drop.source, now_ms, outcomes);
        }
        let areas = std::mem::take(&mut fight.areas);
        for mut area in areas {
            if self.tick_area(fight, &mut area, now_ms, outcomes) {
                fight.areas.push(area);
            }
        }
    }

    /// The timers, the release and the pour of `run` of `kind` at `now_ms`.
    fn tick_use(
        &mut self,
        fight: &mut WarFight,
        run: &mut StructureRun,
        kind: &StructureKind,
        now_ms: u64,
    ) {
        let (ended, running): (Vec<_>, Vec<_>) = std::mem::take(&mut run.uses.timers)
            .into_iter()
            .partition(|timer| now_ms >= timer.ends_ms);
        run.uses.timers = running;
        let full: Vec<BundleMember> = ended
            .iter()
            .map(|timer| completed_member(timer.component))
            .collect();
        let mut partial = full.clone();
        if run.uses.enabled_at_ms.is_some_and(|at| now_ms >= at) {
            run.uses.enabled_at_ms = None;
            partial.extend(run.interact_change(InteractState {
                enabled: Some(true),
                has_interactors: None,
            }));
        }
        if let Some(using) = run.uses.using {
            if using.pour_ms.is_some_and(|at| now_ms >= at) {
                self.drop_barrel(fight, run, kind, using, now_ms);
            }
            let poured = using.pour_ms.is_none_or(|at| now_ms >= at);
            let released = using.released || now_ms >= using.release_ms;
            if released && !using.released {
                partial.extend(run.interact_change(InteractState {
                    enabled: None,
                    has_interactors: Some(NO_INTERACTORS),
                }));
            }
            run.uses.using = (!(poured && released)).then_some(StructureUse {
                pour_ms: using.pour_ms.filter(|_| !poured),
                released,
                ..using
            });
        }
        if !partial.is_empty() {
            self.sync_use(run, full, partial, now_ms);
        }
    }

    /// The pour of `run` ended: its barrel comes and starts to fall (module doc).
    fn drop_barrel(
        &mut self,
        fight: &mut WarFight,
        run: &StructureRun,
        kind: &StructureKind,
        using: StructureUse,
        now_ms: u64,
    ) {
        let (Some(drop), Some(area)) = (&kind.drop, &kind.area) else {
            return;
        };
        let structure = &run.body.structure;
        let place = run.body.place();
        let spawn = Position {
            x: place.x - structure.yaw.sin() * BARREL_AHEAD_M,
            y: place.y + structure.yaw.cos() * BARREL_AHEAD_M,
            z: place.z + BARREL_UP_M,
        };
        let floor = self
            .ground
            .floor_at(spawn.x, spawn.y, spawn.z)
            .unwrap_or(place.z);
        let falling_ms = fall_ms(spawn.z - floor, drop.gravity_mps2);
        let projectile = ProjectileSpawn {
            asset: drop.projectile.asset,
            component: drop.projectile.component,
            position_component: None,
            state: ProjectileState {
                velocity: Some([0.0; 3]),
                owner: Some(facet_entity(using.number)),
                ranged_attack: Some(drop.projectile.ranged_attack),
                damage_table: Some(drop.projectile.damage_table_crc),
                damage_row: Some(drop.projectile.row_index),
                ..ProjectileState::default()
            },
            muzzle: spawn,
            flying_ms: falling_ms,
            stuck_ms: 0,
        };
        let Some((entity, _)) = self.spawn_projectile(projectile, now_ms) else {
            return;
        };
        let raid = self.war_raid_of(using.entity);
        if let Some(component) = drop.ownership_component {
            let ownership = BundleMember {
                component,
                state: MemberState::Ownership(self.owned_ownership(
                    fight,
                    structure.side,
                    using.entity,
                    raid,
                )),
            };
            let shown = ownership.clone();
            self.stations
                .change_camp(entity, |members| members.push(shown), vec![ownership]);
        }
        let facet = structure.facet_entity();
        self.send_to_all(&[ServerMessage::CombatEffect(
            CombatEffectMsg::ProjectileServerSpawn {
                header: FacetHeader {
                    target: drop.spawner_facet.target(facet),
                    entity: facet,
                },
            },
        )]);
        fight.drops.push(BarrelDrop {
            entity,
            land_ms: now_ms.saturating_add(falling_ms),
            at: Position { z: floor, ..spawn },
            area: area.clone(),
            source: AreaSource {
                side: structure.side,
                number: using.number,
                raid,
                rotation: rotation_of(structure),
            },
        });
    }

    /// Opens `area` of `source` at `centre` at `now_ms` (module doc); its first hits go into
    /// `outcomes`.
    pub(super) fn open_siege_area(
        &mut self,
        fight: &mut WarFight,
        area: &SiegeArea,
        centre: Position,
        source: AreaSource,
        now_ms: u64,
        outcomes: &mut Vec<Result<AttackOutcome, AttackRefusal>>,
    ) {
        let Some(place) = WorldPosition::from_metres(centre.x, centre.y, centre.z) else {
            return;
        };
        let owner = self.player_by_number(source.number);
        let ownership =
            owner.map(|entity| self.owned_ownership(fight, source.side, entity, source.raid));
        let namespace = AREA_NAMESPACE ^ self.me.generation;
        let number = self.stations.place_camp(|number| {
            // A slot key is far below `usize::MAX` on the 64-bit targets the server runs on.
            let gde_ref: [u8; RAW_ID_LEN] = dynamic_ref(namespace, number as usize);
            let mut members = vec![BundleMember {
                component: CARD_COMPONENT,
                state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                    asset: Some(AssetId {
                        id: area.asset,
                        sub_id: DYNAMIC_SLICE_SUB_ID,
                    }),
                    gde_ref: Some(gde_ref),
                }),
            }];
            if let (Some(component), Some(state)) = (area.ownership_component, ownership) {
                members.push(BundleMember {
                    component,
                    state: MemberState::Ownership(state),
                });
            }
            if let Some(component) = area.position_component {
                members.push(BundleMember {
                    component,
                    state: MemberState::Position(PositionInTheWorldReplicatedState {
                        position: Some(place),
                        rotation: source.rotation,
                        scale: None,
                    }),
                });
            }
            members.extend(
                area.timers
                    .iter()
                    .map(|timer| timer_member(timer.component, timer.delay_s, Some(false))),
            );
            Station {
                gde_ref,
                position: centre,
                station_type: String::new(),
                name: Vec::new(),
                members,
            }
        });
        let mut run = AreaRun {
            number,
            area: area.clone(),
            source,
            centre,
            ends_ms: now_ms.saturating_add(area.life_ms()),
            next_hit_ms: area.damage.as_ref().map(|_| now_ms),
            timers: area
                .timers
                .iter()
                .map(|timer| RunningTimer::of(timer, now_ms))
                .collect(),
        };
        self.give_area_effects(fight, &run, now_ms);
        if self.tick_area(fight, &mut run, now_ms, outcomes) {
            fight.areas.push(run);
        }
    }

    /// One tick of `run` at `now_ms`: its hits and its timers; `false` once it ended (its entity
    /// is gone).
    fn tick_area(
        &mut self,
        fight: &mut WarFight,
        run: &mut AreaRun,
        now_ms: u64,
        outcomes: &mut Vec<Result<AttackOutcome, AttackRefusal>>,
    ) -> bool {
        if run.next_hit_ms.is_some_and(|at| now_ms >= at) && now_ms <= run.ends_ms {
            self.area_hits(fight, run, now_ms, outcomes);
            run.next_hit_ms = match &run.area.damage {
                Some(damage) if !run.area.once => Some(now_ms.saturating_add(damage.rate_ms())),
                _ => None,
            };
        }
        let (ended, running): (Vec<_>, Vec<_>) = std::mem::take(&mut run.timers)
            .into_iter()
            .partition(|timer| now_ms >= timer.ends_ms);
        run.timers = running;
        if now_ms >= run.ends_ms {
            self.stations.remove_camp(run.number);
            return false;
        }
        if !ended.is_empty() {
            let changed: Vec<BundleMember> = ended
                .iter()
                .map(|timer| completed_member(timer.component))
                .collect();
            let full = changed.clone();
            self.stations.change_camp(
                run.number,
                |members| {
                    for member in &full {
                        replace_member(members, member);
                    }
                },
                changed,
            );
        }
        true
    }

    /// The hits of `run` on what stands in it (module doc).
    fn area_hits(
        &mut self,
        fight: &mut WarFight,
        run: &AreaRun,
        now_ms: u64,
        outcomes: &mut Vec<Result<AttackOutcome, AttackRefusal>>,
    ) {
        let Some(damage) = &run.area.damage else {
            return;
        };
        let coefficient = self
            .combat
            .data
            .table_attack(&damage.table, &damage.row)
            .map_or(0.0, |row| row.coefficient);
        let amount = area_hit_amount(damage, coefficient);
        let targets = self.shot_targets(run.source.number, &fight.turrets, run.source.side);
        for (target, at, capsule) in targets {
            if !within(run.centre, run.area.radius_m, at, capsule) {
                continue;
            }
            match target {
                ShotTarget::Body(number) => {
                    outcomes.extend(
                        self.apply_hit(number, run.source.number, amount, 0, now_ms)
                            .map(Ok),
                    );
                }
                ShotTarget::Turret { index, .. } => {
                    if let Some(mut turret) = fight.turrets.get(index).cloned() {
                        self.hurt_turret(&mut turret, amount);
                        fight.turrets[index] = turret;
                    }
                }
            }
        }
    }

    /// The status effects of `run` of `fight` on the living players of its side in its radius.
    fn give_area_effects(&mut self, fight: &WarFight, run: &AreaRun, now_ms: u64) {
        if run.area.effects.is_empty() {
            return;
        }
        let content = std::sync::Arc::clone(&self.content);
        for entity in self.players_within(run.centre, run.area.radius_m) {
            if self.side_in(fight, entity) != Some(run.source.side) {
                continue;
            }
            if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                for id in &run.area.effects {
                    player.status_effects.add_by_id(
                        &content.status_effects,
                        id,
                        EFFECT_POTENCY,
                        run.source.number,
                        now_ms,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use nw_content::status_effects::{EffectSourceRow, StatusEffectRules, StatusEffectSource};
    use nw_content::war_forts::{SIDE_DEFENDER, WarForts};
    use nw_formats::objectstream::lowercase_crc32;

    use nw_messages::client_input::TRIGGER_KIND;

    use super::*;
    use crate::player_content::PlayerContent;
    use crate::world::tests::{NOW, messages_to};
    use crate::world::turrets::tests::{
        CANNON_ASSET, CANNON_REF, DEFENDER, TABLE, cannon_call, forts_with_cannon_of, use_request,
        war_world_with,
    };

    /// A made-up T1 dropper and T2 horn of the defenders in the test fort, with the retail
    /// members of the T1 dropper of `joining_defending_war_2` (place and turn, spawner, pour
    /// timer) and of the T2 horn of `invasion` (its blow timers), the test repeater's vitals and
    /// buildable members, and their kinds with the slice values (`ProjectileSpawnDelay` 0.3 s,
    /// `BlowDelay` 7, 7, 10 and 90 s, the barrel area's 1,200 `FireBarrelDropper_DoT` every
    /// 0.5 s in 5 m for 15 s, the horn area's `HornOfResilience_T2` in 40 m for 1 s; made-up
    /// facet rules).
    const DROPPER_ASSET: &str = "4906e0ed66b052588dfadb3b8bb8988e";
    const HORN_ASSET: &str = "09ea044a0f525bcbaaddc0b5f9e51067";
    const DROPPER_REF: [u8; 16] = [0x72; 16];
    const HORN_REF: [u8; 16] = [0x7c; 16];
    const BARREL_ASSET: &str = "05932228880d5bdf8fb1cc50513e7c62";
    const BARREL_AREA_ASSET: &str = "b488e3f31cb65871baaf312aa2a52dc4";
    const HORN_AREA_ASSET: &str = "2b622d29df025e9c91825ed757b1245e";
    const SHELL_AREA_ASSET: &str = "4b0db0bfcbfc5a1f899578633f3e0cc1";
    const HORN_EFFECT: &str = "HornOfResilience_T2";
    const VITALS: &str = "020f01ff47afc80000000000000b9790c0aee250f80000000000000000ff0047afc80000003f8000005300001eaefb73015f90";
    const BUILDABLE: &str = "05ba1801070500000103010b5369656765537570706c79573a918000000000003c00ffffffff00000033000000000000";
    const INTERACT: &str = "11b22d01030100000000";
    /// The horn's interact is on component 15 (Observed: `invasion` 95.691 s).
    const HORN_INTERACT: &str = "0fb22d01030100000000";
    const POUR_COMPONENT: u64 = 19;
    const TRIGGER: (u64, u64) = (0x1000, 0x20);
    const SPAWNER: (u64, u64) = (0x3000, 0x40);
    const POUR_MS: u64 = 300;
    const AREA_LIFE_MS: u64 = 15_000;
    const SHORT_TIMER_MS: u64 = 4_000;
    const BLOW_WAIT_MS: u64 = 7_000;
    const HORN_AREA_MS: u64 = 1_000;
    /// `BaseDuration` of `HornOfResilience_T2`.
    const HORN_EFFECT_S: f64 = 10.0;

    fn structures() -> serde_json::Value {
        serde_json::json!([
            {"asset": DROPPER_ASSET, "gde_ref": hex::encode(DROPPER_REF), "card_component": 0,
             "interact_component": 17, "spawner_component": 16, "ownership_component": 4,
             "place": [13680.326, 6332.656, 57.43], "side": SIDE_DEFENDER, "yaw": 0.698,
             "members": ["010d01034655c14e45c5e53f133103abc6", VITALS, BUILDABLE, "10390103ffff00",
                         "1383310103000000", INTERACT]},
            {"asset": HORN_ASSET, "gde_ref": hex::encode(HORN_REF), "card_component": 0,
             "interact_component": 15, "spawner_component": null, "ownership_component": 4,
             "place": [13670.0, 6320.0, 50.6], "side": SIDE_DEFENDER, "yaw": 0.0,
             "members": ["010d010345b381aa45e6f2f5160903b617", VITALS, BUILDABLE, "1183310103000000",
                         "1283310103000000", "1383310103000000", "1483310103000000", HORN_INTERACT]}
        ])
    }

    fn kinds() -> serde_json::Value {
        let facet = |(xor, component): (u64, u64)| serde_json::json!({"xor": format!("{xor:016x}"), "component": format!("{component:016x}")});
        serde_json::json!([
            {"asset": DROPPER_ASSET, "slice": "slices/structures/siege/dropper/dropper_t1_firebarrel.dynamicslice",
             "template": null, "trigger": facet(TRIGGER), "use_timers": [],
             "drop": {"projectile": {"asset": BARREL_ASSET, "component": 2, "ranged_attack": 531,
                      "damage_table_crc": 81017683, "row_index": 17}, "ownership_component": 1, "gravity_mps2": 13.0,
                      "spawner_facet": facet(SPAWNER), "pour": {"component": POUR_COMPONENT, "delay_s": 0.3}},
             "area": {"asset": BARREL_AREA_ASSET, "ownership_component": 3, "position_component": 1,
                      "timers": [{"component": 4, "delay_s": 4.0}, {"component": 5, "delay_s": 15.0}, {"component": 6, "delay_s": 4.0}],
                      "radius_m": 5.0, "once": false,
                      "damage": {"table": "DamageTable_Structures", "row": "FireBarrelDropper_DoT", "base_damage": 1200, "rate_s": 0.5},
                      "effects": []}},
            {"asset": HORN_ASSET, "slice": "slices/structures/siege/support/support_t2_hornofresilience.dynamicslice",
             "template": null, "trigger": facet(TRIGGER),
             "use_timers": [{"component": 17, "delay_s": 7.0}, {"component": 18, "delay_s": 7.0},
                            {"component": 19, "delay_s": 10.0}, {"component": 20, "delay_s": 90.0}],
             "drop": null,
             "area": {"asset": HORN_AREA_ASSET, "ownership_component": 1, "position_component": 2,
                      "timers": [{"component": 3, "delay_s": 1.0}], "radius_m": 40.0, "once": true, "damage": null,
                      "effects": [HORN_EFFECT]}}
        ])
    }

    /// The war test world with the dropper and the horn, and the horn's effect in the data.
    fn use_world() -> (SimWorld, Entity) {
        let mut table: serde_json::Value = serde_json::from_str(TABLE).expect("test table");
        table["forts"][0]["structures"]
            .as_array_mut()
            .expect("structures")
            .extend(structures().as_array().expect("rows").iter().cloned());
        table["structure_kinds"] = kinds();
        let forts = WarForts::parse(&table.to_string()).expect("table");
        war_world_with(forts, SIDE_DEFENDER, |content| PlayerContent {
            status_effects: StatusEffectRules::build(StatusEffectSource {
                effects: vec![EffectSourceRow {
                    status_id: HORN_EFFECT.to_owned(),
                    base_duration_s: Some(HORN_EFFECT_S),
                    stack_max: Some(1),
                    ..EffectSourceRow::default()
                }],
                ..StatusEffectSource::default()
            }),
            ..content
        })
    }

    fn content_holds_effect(world: &SimWorld) -> bool {
        world.content.status_effects.by_id(HORN_EFFECT).is_some()
    }

    fn trigger_request(target: [u8; 16]) -> InteractionRequest {
        InteractionRequest {
            kind: TRIGGER_KIND,
            ..use_request(target)
        }
    }

    fn structure<'a>(world: &'a SimWorld, gde_ref: &[u8; 16]) -> &'a StructureRun {
        world
            .war_fight()
            .expect("fight")
            .structures
            .iter()
            .find(|run| &run.body.structure.gde_ref == gde_ref)
            .expect("structure")
    }

    fn members(world: &SimWorld, number: u64) -> Vec<BundleMember> {
        world.stations.members(number).expect("members").to_vec()
    }

    fn interact_of(members: &[BundleMember]) -> InteractState {
        members
            .iter()
            .find_map(|member| match &member.state {
                MemberState::Interact(state) => Some(*state),
                _ => None,
            })
            .expect("interact")
    }

    fn timer_of(members: &[BundleMember], component: u64) -> DelayedEventState {
        members
            .iter()
            .find_map(|member| match &member.state {
                MemberState::DelayedEvent(state) if member.component == component => Some(*state),
                _ => None,
            })
            .expect("timer")
    }

    fn card_asset(members: &[BundleMember]) -> Option<String> {
        members.iter().find_map(|member| match &member.state {
            MemberState::GdeMetadata(card) => card.asset.map(|asset| hex::encode(asset.id)),
            _ => None,
        })
    }

    fn accepted(sent: &[ServerMessage]) -> bool {
        sent.iter().any(|message| {
            matches!(message, ServerMessage::InteractionValidated(answer) if answer.status == USE_ACCEPTED)
        })
    }

    #[test]
    fn a_dropper_use_pours_drops_its_barrel_and_opens_its_area_where_it_lands() {
        let (mut world, entity) = use_world();
        assert!(world.turret_interaction(
            entity,
            DEFENDER,
            trigger_request(DROPPER_REF),
            b"Dropper_T1_FireBarrel",
            NOW
        ));
        let sent = messages_to(&world.take_messages(), DEFENDER);
        assert!(accepted(&sent), "{sent:?}");
        let facet = structure(&world, &DROPPER_REF)
            .body
            .structure
            .facet_entity();
        assert!(sent.iter().any(|message| matches!(message,
            ServerMessage::SyncExecuteEntities(call) if call.header.entity == facet
                && call.header.target == (facet ^ TRIGGER.0).wrapping_add(TRIGGER.1))));
        let number = structure(&world, &DROPPER_REF).body.number;
        let shown = members(&world, number);
        assert_eq!(interact_of(&shown).has_interactors, Some(ONE_USER));
        assert_eq!(timer_of(&shown, POUR_COMPONENT).total_delay_s, Some(0.3));
        let owner = shown.iter().find_map(|member| match &member.state {
            MemberState::Ownership(state) => state.owner.clone(),
            _ => None,
        });
        assert!(
            owner.is_some_and(|owner| !owner.first.is_empty()),
            "the user owns it"
        );
        assert!(world.turret_interaction(
            entity,
            DEFENDER,
            trigger_request(DROPPER_REF),
            b"",
            NOW + 1
        ));
        assert!(
            !accepted(&messages_to(&world.take_messages(), DEFENDER)),
            "one use at a time"
        );

        world.tick_turrets(NOW + POUR_MS);
        let sent = messages_to(&world.take_messages(), DEFENDER);
        assert!(sent.iter().any(|message| matches!(message,
            ServerMessage::CombatEffect(CombatEffectMsg::ProjectileServerSpawn { header })
                if header.target == (facet ^ SPAWNER.0).wrapping_add(SPAWNER.1))));
        assert_eq!(
            timer_of(&members(&world, number), POUR_COMPONENT).completed,
            Some(true)
        );
        let drop = world
            .war_fight()
            .expect("fight")
            .drops
            .first()
            .cloned()
            .expect("a falling barrel");
        let barrel = members(&world, drop.entity);
        assert_eq!(card_asset(&barrel).as_deref(), Some(BARREL_ASSET));
        assert!(barrel.iter().any(
            |member| member.component == 1 && matches!(member.state, MemberState::Ownership(_))
        ));
        assert_eq!(
            drop.land_ms,
            NOW + POUR_MS + fall_ms(BARREL_UP_M, 13.0),
            "from its spawn point to the dropper's height"
        );

        world.tick_turrets(drop.land_ms);
        let fight = world.war_fight().expect("fight");
        assert!(fight.drops.is_empty());
        let area = fight.areas.first().cloned().expect("the barrel's area");
        let shown = members(&world, area.number);
        assert_eq!(card_asset(&shown).as_deref(), Some(BARREL_AREA_ASSET));
        assert_eq!(
            timer_of(&shown, 5),
            DelayedEventState {
                total_delay_s: Some(15.0),
                completed: Some(false)
            }
        );

        world.tick_turrets(NOW + DROPPER_USE_MS);
        assert_eq!(
            interact_of(&members(&world, number)).has_interactors,
            Some(NO_INTERACTORS)
        );
        world.tick_turrets(drop.land_ms + SHORT_TIMER_MS);
        assert_eq!(
            timer_of(&members(&world, area.number), 4).completed,
            Some(true)
        );
        assert_eq!(
            timer_of(&members(&world, area.number), 5).completed,
            Some(false)
        );
        world.tick_turrets(drop.land_ms + AREA_LIFE_MS);
        assert!(world.war_fight().expect("fight").areas.is_empty());
        assert!(
            world.stations.members(area.number).is_none(),
            "its entity went"
        );
        world.take_messages();
        assert!(world.turret_interaction(
            entity,
            DEFENDER,
            trigger_request(DROPPER_REF),
            b"",
            NOW + AREA_LIFE_MS * 2
        ));
        assert!(
            accepted(&messages_to(&world.take_messages(), DEFENDER)),
            "free again"
        );
    }

    #[test]
    fn a_horn_blow_gives_its_side_the_effect_and_waits_for_its_shortest_timer() {
        let (mut world, entity) = use_world();
        // The player stands 5 m from the horn, within its area's 40 m.
        let near = Position {
            x: 13_675.0,
            y: 6_320.0,
            z: 50.6,
        };
        world.place_player(entity, near, None, crate::world::Placement::Stand, NOW);
        assert!(world.turret_interaction(entity, DEFENDER, trigger_request(HORN_REF), b"", NOW));
        assert!(accepted(&messages_to(&world.take_messages(), DEFENDER)));
        let number = structure(&world, &HORN_REF).body.number;
        let shown = members(&world, number);
        assert_eq!(interact_of(&shown).enabled, Some(false));
        assert_eq!(timer_of(&shown, 20).total_delay_s, Some(90.0));
        world.tick_turrets(NOW + 1);
        assert!(
            !world.war_fight().expect("fight").areas.is_empty(),
            "the horn's area opened"
        );
        assert!(
            world.players_within(near, 1.0).contains(&entity),
            "the player stands near"
        );
        assert!(content_holds_effect(&world), "the data has the effect");
        let effect = lowercase_crc32(HORN_EFFECT);
        assert!(
            world
                .world
                .get::<Player>(entity)
                .expect("player")
                .status_effects
                .holds(effect)
        );
        let area = world
            .war_fight()
            .expect("fight")
            .areas
            .first()
            .cloned()
            .expect("the horn's area");
        assert_eq!(
            card_asset(&members(&world, area.number)).as_deref(),
            Some(HORN_AREA_ASSET)
        );
        world.tick_turrets(NOW + HORN_USE_MS);
        assert_eq!(
            interact_of(&members(&world, number)).has_interactors,
            Some(NO_INTERACTORS)
        );
        world.tick_turrets(NOW + 1 + HORN_AREA_MS);
        assert!(
            world.war_fight().expect("fight").areas.is_empty(),
            "its area lasts its timer"
        );
        world.take_messages();
        assert!(world.turret_interaction(
            entity,
            DEFENDER,
            trigger_request(HORN_REF),
            b"",
            NOW + BLOW_WAIT_MS - 1
        ));
        assert!(
            !accepted(&messages_to(&world.take_messages(), DEFENDER)),
            "off until its shortest timer"
        );
        world.tick_turrets(NOW + BLOW_WAIT_MS);
        let shown = members(&world, number);
        assert_eq!(interact_of(&shown).enabled, Some(true));
        assert_eq!(timer_of(&shown, 17).completed, Some(true));
        assert_eq!(
            timer_of(&shown, 20).completed,
            None,
            "the 90 s timer still runs"
        );
        world.take_messages();
        assert!(world.turret_interaction(
            entity,
            DEFENDER,
            trigger_request(HORN_REF),
            b"",
            NOW + BLOW_WAIT_MS
        ));
        assert!(accepted(&messages_to(&world.take_messages(), DEFENDER)));
    }

    #[test]
    fn an_explosive_shell_opens_its_slice_area_once_where_it_lands() {
        let forts = forts_with_cannon_of("turret_t3_explosivelauncher.dynamicslice");
        let kind = forts.kind(&CANNON_ASSET).cloned().expect("kind");
        let area = kind.area.clone().expect("the shell's area from the slices");
        assert_eq!(hex::encode(area.asset), SHELL_AREA_ASSET);
        assert!(area.once, "an explosion");
        let (mut world, entity) =
            war_world_with(forts, nw_content::war_forts::SIDE_ATTACKER, |content| {
                content
            });
        assert!(world.turret_interaction(entity, DEFENDER, use_request(CANNON_REF), &[], NOW));
        let connection = world
            .world
            .get::<Player>(entity)
            .expect("player")
            .connection
            .clone();
        world.turret_call(&connection, cannon_call(), NOW + 10);
        let land_ms = world
            .war_fight()
            .expect("fight")
            .turrets
            .iter()
            .find_map(|run| run.balls.first().map(|ball| ball.land_ms))
            .expect("a shell in flight");
        world.tick_turrets(land_ms);
        let fight = world.war_fight().expect("fight");
        let opened = fight.areas.first().expect("the explosion");
        assert_eq!(opened.next_hit_ms, None, "it hit once when it opened");
        assert_eq!(
            card_asset(&members(&world, opened.number)).as_deref(),
            Some(SHELL_AREA_ASSET)
        );
    }

    #[test]
    fn an_area_hit_takes_its_base_damage_times_the_row_and_reaches_its_radius() {
        let damage = AreaDamage {
            table: "DamageTable_Structures".to_owned(),
            row: "BombTurret_Explosion".to_owned(),
            base_damage: 5_265,
            rate_s: 5.0,
        };
        assert_eq!(area_hit_amount(&damage, 0.5), 2_633);
        assert_eq!(damage.rate_ms(), 5_000);
        let body = Capsule {
            radius_m: 0.5,
            height_m: 2.0,
        };
        let centre = Position {
            x: 0.0,
            y: 0.0,
            z: 10.0,
        };
        assert!(within(
            centre,
            5.0,
            Position {
                x: 5.4,
                y: 0.0,
                z: 9.0
            },
            body
        ));
        assert!(
            !within(
                centre,
                5.0,
                Position {
                    x: 5.6,
                    y: 0.0,
                    z: 9.0
                },
                body
            ),
            "out of reach"
        );
        assert!(
            !within(
                centre,
                5.0,
                Position {
                    x: 0.0,
                    y: 0.0,
                    z: 16.0
                },
                body
            ),
            "above it"
        );
        assert_eq!(
            fall_ms(12.6, 13.0),
            1_392,
            "the retail barrel's fall of 12.6 m"
        );
    }
}
