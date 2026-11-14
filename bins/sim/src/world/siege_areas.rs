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
//! other member changing.
//!
//! **The cooldown** (Observed, every 2930 body of the capture index with child 2,
//! [`nw_messages::InteractCooldown`]): the interact change of a use carries the cooldown of the
//! used option (option 0): it ends 20 s after a dropper's or a vat's use ([`DROPPER_COOLDOWN_MS`])
//! and 90 s after a horn's blow (`invasion-71657c7d8159622a` 95.691 s: the horn's longest blow
//! timer); the entity cards after it keep the last cooldown (`joining_defending_war_2` 165.349
//! s). No later change of the use sends it (the release `01 02 00000000`, the horn's interact
//! on again). Inferred: a use while the cooldown runs is refused (the client shows
//! `ui_interactable_on_cooldown`, `FUN_1442a3bb0`), so a horn waits for its longest timer though
//! its interact is on again after its shortest; the list's version counts the uses
//! ([`FIRST_COOLDOWN_VERSION`] at the first; retail's step is not in the captures).
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
//! its status effects to the players of its side in its radius when it opens. A hit takes the
//! slice's `m_baseDamage` times the row's coefficient, on a player less its worn armor's share
//! against the row's damage category, with the row's damage type as its 3601 item tag
//! (Observed, `invasion-71657c7d8159622a` 862.680 s to 864.187 s: four barrel hits on the own
//! player 0.5 s apart, each `9:1125.1355:0.707632`, `Fire` 1,200 x `FireBarrelDropper_DoT` 1.325
//! x the 0.707632 the armor kept; the only capture of a player in a barrel's area, and no
//! capture has a shell's explosion on a player). Inferred: it meets what a turret's shot of the
//! same side meets (the mobs, the player's foes, the whole turrets of the other side); a mob or
//! a turret takes it whole, a turret of an Outpost Rush outpost as its outpost structure
//! ([`SimWorld::hurt_held_turret`]); a hit that lands counts as the user's
//! `DamageWithSiegeWeapons` on the Outpost Rush war board, as a turret's does.
//!
//! **The holder.** The trigger structures of a fight and what their uses opened
//! ([`SiegeUses`]: the barrels falling, the areas open and those that open at the next tick)
//! are the war fight's ([`super::war_fight::WarFight`]: its structures and uses) or the Outpost
//! Rush match's ([`super::outpost_rush_structures::OutpostStructures`]: its droppers and uses);
//! every use and tick reaches them through [`SimWorld::with_use_runs`], by the side of
//! [`SimWorld::turret_side`] (a war side, or an Outpost Rush team).
//!
//! **An Outpost Rush vat** (`or_turretfire`, `or_turretfire_trench`; a T1 dropper of the
//! structure kinds, [`nw_content::war_forts::StructureKind::is_war_siege`]): Observed, `OPR_TEST3`
//! slot 123 (built 1706.442 s): another player's use at 1714.326 s shows the user on the vat's
//! use interact (component 16, the slice's interact with `DropFlameBarrel`; its build interact
//! 19 stays disabled), names the user in its ownership (component 4: the user's company with its
//! name and crest, the user, the user's faction; children 0 to 3 and 7) and starts its pour
//! timer (component 18, 0.30 s); its trigger's 3135 follows (1714.344 s, header
//! `905e1783d0f85ce2` / `d08ea7b0680df518`, the slice's facet rule); the pour completes
//! (1714.624 s) and the war T1 barrel `dropper_t1_firebarrel_projectile` comes with the
//! spawner's 2900 (1714.642 s, header `30c6e9b8d2c0184e`) 2.78 m ahead of the vat along its
//! facing (the war's [`BARREL_AHEAD_M`]) with the ownership of the use in full (rule 4, the
//! team's raid, the user's company and faction); its area `dropper_t1_firebarrel_aoe` opens at
//! the same x and y (1715.608 s; timers 4, 15 and 4 s, the same ownership); the user is let go
//! 1.466 s after the use (1715.792 s; the war's [`DROPPER_USE_MS`] is 1.49 s). Inferred: only the
//! outpost's owner team uses a built vat; its health is its outpost structure's, so a ruin
//! stops a use (no barrel) and disables it until its rebuild; a new owner team takes it and
//! stops a use of the other team. The interact change of the use carries the vat's cooldown
//! (1714.326 s: option 0, version 62, ending 19.95 s after the use on that capture's clock), as
//! the war droppers' do (**The cooldown**).
//!
//! **Level structures** ([`LevelSiege`], batch E1): outside a fight the holder is the level's
//! own ([`super::turrets::TurretHost::Level`]): the open world's fire droppers (the five
//! `Dropper_Fire_POI_<n>` capitals of the Hedet canyon, [`nw_content::war_forts::LevelDropper`];
//! the quest SQ-Brimestone-Sands-2 sends the player to one) and the turrets an instance's
//! encounter spawns. Observed (the quest ledgers of `SQ-Brimestone-Sands-2`, 183.547 to
//! 720.941 s): a player near one gets its level card (no asset: the capital id and 8 zero bytes),
//! no place state, its vitals (71,996 of 72,000, invulnerable), its buildable state 5, its
//! delayed events, its projectile spawner (ammo `ffff`), its container and placement
//! obstruction, and an ownership of no company with rule 1 ([`LEVEL_PERMISSION_RULE`]) that
//! names the last user (kind 2); a dropper used before shows its use interact (component 16)
//! with the cooldown of that use, long ended. The slot stops when the player goes away, and the
//! same states come back (337.256 and 342.790 s). No ledger holds a use of one; its slice is a
//! dropper's (the `DropFlameBarrel` option on interact 16, `ProjectileSpawnDelay` 0.3 s on
//! component 18, `Dropper_T3_FireBarrel_Projectile`), so a use runs as a war dropper's (the
//! pour, the barrel, its `dropper_t3_firebarrel_aoe` area and its `FireBarrelDropper_DoT` hits on
//! the mobs and the player's foes, the 20 s cooldown after which it is ready again). Inferred:
//! every player uses it ([`LEVEL_SIDE`]), one at a time; it takes no harm (no capture shows a
//! hit on one), so it never breaks and needs no respawn; its use names the user as the owner,
//! as a war dropper's does.

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
    FullList, GdeMetadataReplicatedState, InteractCooldown, InteractCooldownState, InteractState,
    InteractionRequest, MemberState, OwnershipState,
    PositionInTheWorldReplicatedState, Rotation, ServerMessage, WorldPosition,
};
use nw_sim_core::Position;

use super::attack::MobHitStyle;
use super::crafting::{CARD_COMPONENT, Station, dynamic_ref};
use super::gates::merge_members;
use super::shot_entity::ProjectileSpawn;
use super::siege_structures::{NO_INTERACTORS, StructureBody, StructureRun, spawn_members};
use super::turrets::{ShotTarget, TurretHost, TurretRun, USE_ACCEPTED, USE_REFUSED, turret_ownership};
use super::war_fight::replace_member;
use super::{Player, SimWorld, gather_answer, send_now, unix_ns};
use crate::combat::{AttackOutcome, AttackRefusal};
use crate::mobs::Capsule;

/// How long after the accept a dropper's interactor goes, ms (Observed: 124.717 -> 126.207 s;
/// an Outpost Rush vat's 1.466 s, `OPR_TEST3` 1714.326 -> 1715.792 s. Inferred: one hold for
/// both, as their slices share the dropper's components and no slice value holds it).
pub(super) const DROPPER_USE_MS: u64 = 1_490;
/// How long after the accept a horn's interactor goes, ms (Observed: 95.691 -> 95.789 s).
const HORN_USE_MS: u64 = 98;
/// How long a dropper's or a vat's option stays on cooldown after its use, ms (module doc,
/// **The cooldown**; Observed: the end its use sends is the use time plus 19.95 s on the clock of
/// `OPR_TEST3` (1714.326 s) and 20.0 to 20.07 s in `joining_defending_war_2` (124.741, 180.041,
/// 1436.598, 1458.199 s); no slice value holds it. Inferred: the same for every dropper tier).
pub(super) const DROPPER_COOLDOWN_MS: u64 = 20_000;
/// The option a use puts on cooldown (Observed: 0 on every dropper, vat and horn use).
pub(super) const USE_OPTION: u32 = 0;
/// The version of a structure's cooldown list at its first use (Inferred, module doc).
pub(super) const FIRST_COOLDOWN_VERSION: u64 = 1;
/// How much the version rises at each later use (Inferred, module doc).
const COOLDOWN_VERSION_STEP: u64 = 1;
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
/// The side of every player and every level structure outside a fight (module doc, **Level
/// structures**).
pub(super) const LEVEL_SIDE: u8 = nw_content::war_forts::SIDE_DEFENDER;
/// The permission rule of a level structure's ownership (Observed: 1 on the Hedet canyon
/// droppers).
pub(super) const LEVEL_PERMISSION_RULE: u8 = 1;
/// The delay a completed timer shows (Observed: `total_delay_s` 0 with the completed flag).
pub(super) const COMPLETED_DELAY_S: f32 = 0.0;
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

/// The cooldown of a structure's use option (module doc, **The cooldown**).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UseCooldown {
    /// Sim ms it ends.
    ends_ms: u64,
    /// The entry its interact sends.
    entry: InteractCooldown,
    /// The version of its list.
    version: u64,
}

/// The trigger use state of one structure (module doc).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct UseState {
    using: Option<StructureUse>,
    timers: Vec<RunningTimer>,
    /// Sim ms a blown horn's interact is on again.
    enabled_at_ms: Option<u64>,
    /// The cooldown of its last use; `None` before its first.
    cooldown: Option<UseCooldown>,
}

impl UseState {
    /// The number of the player who uses it.
    pub(super) fn user_number(&self) -> Option<u64> {
        self.using.map(|using| using.number)
    }

    /// Whether a player used it since it stood (its last use's cooldown is kept).
    pub(super) const fn used(&self) -> bool {
        self.cooldown.is_some()
    }

    /// The interact state `interact` with the cooldown of the last use, when there was one.
    fn with_cooldown(&self, interact: InteractState) -> MemberState {
        match self.cooldown {
            None => MemberState::Interact(interact),
            Some(cooldown) => MemberState::InteractCooldown(InteractCooldownState {
                interact,
                cooldown_updates: FullList {
                    version: Some(cooldown.version),
                    elements: vec![cooldown.entry],
                },
            }),
        }
    }
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

/// The trigger structures and turrets of the level itself, with what their uses opened (module
/// doc, **Level structures**); the sim's wars state holds it ([`super::wars`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LevelSiege {
    /// The level's fire droppers.
    pub(super) droppers: Vec<StructureRun>,
    /// What their uses opened.
    pub(super) uses: SiegeUses,
    /// The turrets an encounter of the instance spawned ([`super::turrets`]).
    pub(super) turrets: Vec<TurretRun>,
    /// The level's droppers spawned.
    spawned: bool,
    /// How many encounter turrets it spawned (their `GdeRef`s count on).
    pub(super) turrets_made: usize,
}

/// What the uses of the trigger structures of one fight opened (module doc, **The holder**).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SiegeUses {
    /// The barrels falling, the areas open and those that open at the next tick.
    pub(super) drops: Vec<BarrelDrop>,
    pub(super) areas: Vec<AreaRun>,
    pub(super) opening: Vec<OpeningArea>,
}

/// The `DelayedEvent` member of a timer that starts with `delay_s` (`completed` as sent).
pub(super) fn timer_member(component: u64, delay_s: f32, completed: Option<bool>) -> BundleMember {
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

/// The resting ownership of a level structure (module doc, **Level structures**): no company,
/// no owner, rule [`LEVEL_PERMISSION_RULE`], raid 0, faction 0.
pub(super) fn level_ownership() -> OwnershipState {
    OwnershipState {
        permission_rule: Some(LEVEL_PERMISSION_RULE),
        ..turret_ownership(&nw_messages::ClaimOwner::NONE)
    }
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

/// The ownership member of `members` on the component of `change` takes each child that
/// `change` sends, as the client's copy of the full state does.
pub(super) fn apply_ownership_change(members: &mut [BundleMember], change: &BundleMember) {
    let MemberState::Ownership(sent) = &change.state else {
        return;
    };
    let Some(MemberState::Ownership(full)) = members
        .iter_mut()
        .find(|member| member.component == change.component)
        .map(|member| &mut member.state)
    else {
        return;
    };
    let sent = sent.clone();
    *full = OwnershipState {
        guild_id: sent.guild_id.or(full.guild_id),
        guild_owner: sent.guild_owner.or_else(|| full.guild_owner.take()),
        guild_crest: sent.guild_crest.or(full.guild_crest),
        owner: sent.owner.or_else(|| full.owner.take()),
        permission_rule: sent.permission_rule.or(full.permission_rule),
        structure_name: sent.structure_name.or_else(|| full.structure_name.take()),
        raid_id: sent.raid_id.or(full.raid_id),
        faction_type: sent.faction_type.or(full.faction_type),
        owner_pvp: sent.owner_pvp.or(full.owner_pvp),
        owner_ffa: sent.owner_ffa.or(full.owner_ffa),
        permission_options: sent.permission_options.or(full.permission_options),
    };
}

/// Whether a body of `capsule` at `at` stands in a sphere of `radius_m` around `centre`.
fn within(centre: Position, radius_m: f32, at: Position, capsule: Capsule) -> bool {
    centre.horizontal_distance(at) <= radius_m + capsule.radius_m
        && centre.z >= at.z - radius_m
        && centre.z <= at.z + capsule.height_m + radius_m
}

/// The health of a hit of an area of `damage` whose row has `coefficient` before any armor
/// (module doc).
fn area_hit_health(damage: &AreaDamage, coefficient: f64) -> f64 {
    f64::from(damage.base_damage) * coefficient
}

/// The health of a hit of an area of `damage` whose row has `coefficient` on a turret, whole
/// points (module doc).
fn area_hit_amount(damage: &AreaDamage, coefficient: f64) -> u32 {
    // A hit of a few thousand points at most.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a hit of thousands"
    )]
    let amount = area_hit_health(damage, coefficient).round().max(0.0) as u32;
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
    /// off while a blown horn waits, with the cooldown of its last use.
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
        self.cooled_interact(interactors, waiting)
    }

    /// Its interact member with no interactor, turned off while a blown horn waits, with the
    /// cooldown of its last use: the resting state its cards show.
    pub(super) fn resting_interact(&self) -> Option<BundleMember> {
        self.cooled_interact(NO_INTERACTORS, self.uses.enabled_at_ms.is_some())
    }

    /// Its interact member with `interactors`, off when `waiting`, with the cooldown of its last
    /// use.
    fn cooled_interact(&self, interactors: u32, waiting: bool) -> Option<BundleMember> {
        let mut member = self.body.interact_member(interactors)?;
        if let MemberState::Interact(mut state) = member.state {
            if waiting {
                state.enabled = Some(false);
            }
            member.state = self.uses.with_cooldown(state);
        }
        Some(member)
    }

    /// The interact change `state` of its interact component.
    fn interact_change(&self, state: MemberState) -> Option<BundleMember> {
        Some(BundleMember {
            component: self.body.structure.interact_component.or_else(|| {
                self.body
                    .interact_member(NO_INTERACTORS)
                    .map(|member| member.component)
            })?,
            state,
        })
    }
}

impl SimWorld {
    /// Sends `partial` to the viewers of `run` and puts `full` (and its interact at `now_ms`)
    /// into its spawn members, also a member its first state had not (the retail vat's pour
    /// timer came with its first use, `OPR_TEST3` 1714.326 s).
    fn sync_use(
        &mut self,
        run: &StructureRun,
        full: Vec<BundleMember>,
        partial: Vec<BundleMember>,
        now_ms: u64,
    ) {
        let mut changed = full;
        changed.extend(run.use_interact(now_ms));
        self.stations.change_camp(
            run.body.number,
            |members| merge_members(members, &changed),
            partial,
        );
    }

    /// Runs `body` on the trigger structures of this sim's fight ([`SimWorld::turret_host`])
    /// and what their uses opened, and puts them back; `None` when it runs no fight. Only the
    /// two lists leave their fight while `body` runs (module doc, **The holder**): the war
    /// fight and the match stay in place.
    pub(super) fn with_use_runs<T>(
        &mut self,
        body: impl FnOnce(&mut Self, &mut Vec<StructureRun>, &mut SiegeUses) -> T,
    ) -> Option<T> {
        let host = self.turret_host()?;
        let (mut runs, mut uses) = match host {
            TurretHost::War => self
                .with_turrets(|_, fight| {
                    (
                        std::mem::take(&mut fight.structures),
                        std::mem::take(&mut fight.uses),
                    )
                })
                .unwrap_or_default(),
            TurretHost::OutpostRush => self
                .arena_matches
                .outpost_rush
                .as_mut()
                .map(|running| {
                    (
                        std::mem::take(&mut running.structures.droppers),
                        std::mem::take(&mut running.structures.uses),
                    )
                })
                .unwrap_or_default(),
            TurretHost::Level => (
                std::mem::take(&mut self.wars.level.droppers),
                std::mem::take(&mut self.wars.level.uses),
            ),
        };
        let result = body(self, &mut runs, &mut uses);
        match host {
            TurretHost::War => {
                self.with_turrets(|_, fight| {
                    fight.structures = runs;
                    fight.uses = uses;
                });
            }
            TurretHost::OutpostRush => {
                if let Some(running) = self.arena_matches.outpost_rush.as_mut() {
                    running.structures.droppers = runs;
                    running.structures.uses = uses;
                }
            }
            TurretHost::Level => {
                self.wars.level.droppers = runs;
                self.wars.level.uses = uses;
            }
        }
        Some(result)
    }

    /// The owner identification of player `user` (Observed on the uses of the war and the
    /// Outpost Rush: its character id and name, kind 2).
    pub(super) fn owner_of(&self, user: Entity) -> Option<OwnerIdentification> {
        self.world
            .get::<Player>(user)
            .map(|player| OwnerIdentification {
                first: player.character.hyphenated().to_string().into_bytes(),
                second: player.name.clone().into_bytes(),
                kind: NO_OWNER_KIND,
            })
    }

    /// The ownership of a siege structure, area or barrel of `side` with player `user` as its
    /// owner in `raid` (module doc): in a war its side company's with the war's values; in an
    /// Outpost Rush match its outpost team's with the user's company and faction
    /// ([`SimWorld::outpost_use_ownership`]).
    fn owned_ownership(&self, side: u8, user: Entity, raid: u64) -> OwnershipState {
        if self.turret_host() == Some(TurretHost::OutpostRush) {
            return self.outpost_use_ownership(user, raid);
        }
        OwnershipState {
            owner: self.owner_of(user),
            permission_rule: Some(AREA_PERMISSION_RULE),
            raid_id: Some(raid),
            faction_type: Some(AREA_FACTION),
            permission_options: Some(AREA_PERMISSION_OPTIONS),
            ..self.war_fight().map_or_else(level_ownership, |fight| {
                self.structure_ownership(fight, side)
            })
        }
    }

    /// The part of the ownership `full` that a use sends (module doc): the owner alone in a war
    /// (Observed, `joining_defending_war_2` 124.717 s); the company (id, name, crest), the owner
    /// and the faction in an Outpost Rush match (Observed, `OPR_TEST3` 1714.326 s).
    fn use_ownership_change(&self, full: &OwnershipState) -> OwnershipState {
        let owner = OwnershipState {
            owner: full.owner.clone(),
            ..OwnershipState::default()
        };
        if self.turret_host() != Some(TurretHost::OutpostRush) {
            return owner;
        }
        OwnershipState {
            guild_id: full.guild_id,
            guild_owner: full.guild_owner.clone(),
            guild_crest: full.guild_crest,
            faction_type: full.faction_type,
            ..owner
        }
    }

    /// A use of the structure of slot key `number` of `side`, with its ownership on `component`,
    /// by player `user` in `raid` names the user (module doc; a turret's use, [`super::turrets`]
    /// module doc, **Use**): the part of the use's ownership that the use sends goes out, and
    /// the structure's card takes it, as the client's copy does, and keeps it after the use.
    pub(super) fn name_use_owner(
        &mut self,
        number: u64,
        component: u64,
        (side, user, raid): (u8, Entity, u64),
    ) {
        let change = BundleMember {
            component,
            state: MemberState::Ownership(
                self.use_ownership_change(&self.owned_ownership(side, user, raid)),
            ),
        };
        self.stations.change_camp(
            number,
            |members| apply_ownership_change(members, &change),
            vec![change.clone()],
        );
    }

    /// The source of an area of `side` that player `number` opened from a structure turned
    /// `rotation`: the player's raid with it ([`SimWorld::use_raid_of`]).
    pub(super) fn area_source(
        &mut self,
        side: u8,
        number: u64,
        rotation: Option<Rotation>,
    ) -> AreaSource {
        let raid = self
            .player_by_number(number)
            .map_or(NO_RAID, |entity| self.use_raid_of(entity));
        AreaSource {
            side,
            number,
            raid,
            rotation,
        }
    }

    /// The raid a use of player `entity` names: its war raid, else its Outpost Rush team's
    /// raid (Observed: the barrel of `OPR_TEST3` 1714.642 s names the user's team raid).
    pub(super) fn use_raid_of(&self, entity: Entity) -> u64 {
        self.world
            .get::<Player>(entity)
            .and_then(|player| self.wars.raid_seat(player.character))
            .map(|(raid, _)| raid.raid)
            .or_else(|| self.outpost_rush_raid_of(entity))
            .unwrap_or(NO_RAID)
    }

    /// The interaction `request` of kind [`nw_messages::client_input::TRIGGER_KIND`] of player
    /// `entity` (`number`) on a dropper or a horn of this sim's fight (module doc): `false` when
    /// it names none.
    pub(super) fn structure_trigger(
        &mut self,
        entity: Entity,
        number: u64,
        request: InteractionRequest,
        name: &[u8],
        now_ms: u64,
    ) -> bool {
        let side = self.turret_side(entity);
        let raid = self.use_raid_of(entity);
        let Some(connection) = self
            .world
            .get::<Player>(entity)
            .map(|player| player.connection.clone())
        else {
            return false;
        };
        let content = std::sync::Arc::clone(&self.content);
        self.with_use_runs(|world, runs, uses| {
            let Some(index) = runs
                .iter()
                .position(|run| run.body.structure.gde_ref == request.target)
            else {
                return false;
            };
            let Some(kind) = content
                .war_forts
                .use_kind(&runs[index].body.structure)
                .filter(|kind| kind.trigger.is_some())
            else {
                return false;
            };
            let run = &runs[index];
            let free = run.uses.using.is_none()
                && !run.body.broken
                && run
                    .uses
                    .cooldown
                    .is_none_or(|cooldown| now_ms >= cooldown.ends_ms);
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
            world.start_use(uses, &mut runs[index], kind, (entity, number, raid), now_ms);
            true
        })
        .unwrap_or(false)
    }

    /// Starts the use of `run` of `kind` by `user` (entity, number, raid) at `now_ms` (module
    /// doc); a horn's area goes into `uses`.
    fn start_use(
        &mut self,
        uses: &mut SiegeUses,
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
        if let Some(component) = structure.ownership_component {
            self.name_use_owner(run.body.number, component, (side, entity, raid));
        }
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
        let blow_delays = kind.use_timers.iter().map(SiegeTimer::delay_ms);
        if horn {
            run.uses.enabled_at_ms = blow_delays
                .clone()
                .min()
                .map(|delay| now_ms.saturating_add(delay));
        }
        let cooldown_ms = if horn {
            blow_delays.max().unwrap_or_default()
        } else {
            DROPPER_COOLDOWN_MS
        };
        run.uses.cooldown = Some(UseCooldown {
            ends_ms: now_ms.saturating_add(cooldown_ms),
            entry: InteractCooldown {
                option: USE_OPTION,
                ends_unix_ns: unix_ns(self.server_ms.saturating_add(cooldown_ms)),
            },
            version: run
                .uses
                .cooldown
                .map_or(FIRST_COOLDOWN_VERSION, |last| {
                last.version.saturating_add(COOLDOWN_VERSION_STEP)
            }),
        });
        let started: Vec<BundleMember> = timers
            .iter()
            .map(|timer| timer_member(timer.component, timer.delay_s, None))
            .collect();
        let full = started.clone();
        let mut partial = started;
        partial.extend(run.interact_change(run.uses.with_cooldown(InteractState {
            enabled: horn.then_some(false),
            has_interactors: Some(ONE_USER),
        })));
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
            uses.opening.push(OpeningArea {
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

    /// One tick of the uses of `runs` and of the barrels and areas of `uses` at sim ms `now_ms`
    /// (module doc); the hits go into `outcomes`.
    pub(super) fn tick_siege_uses(
        &mut self,
        runs: &mut [StructureRun],
        uses: &mut SiegeUses,
        now_ms: u64,
        outcomes: &mut Vec<Result<AttackOutcome, AttackRefusal>>,
    ) {
        let content = std::sync::Arc::clone(&self.content);
        for run in runs.iter_mut() {
            if let Some(kind) = content.war_forts.use_kind(&run.body.structure) {
                self.tick_use(uses, run, kind, now_ms);
            }
        }
        for opening in std::mem::take(&mut uses.opening) {
            self.open_siege_area(
                uses,
                &opening.area,
                opening.centre,
                opening.source,
                now_ms,
                outcomes,
            );
        }
        let (landed, falling): (Vec<_>, Vec<_>) = std::mem::take(&mut uses.drops)
            .into_iter()
            .partition(|drop| now_ms >= drop.land_ms);
        uses.drops = falling;
        for drop in landed {
            self.shot_entity_impact(drop.entity, None, drop.at, now_ms);
            self.open_siege_area(uses, &drop.area, drop.at, drop.source, now_ms, outcomes);
        }
        let areas = std::mem::take(&mut uses.areas);
        for mut area in areas {
            if self.tick_area(&mut area, now_ms, outcomes) {
                uses.areas.push(area);
            }
        }
    }

    /// The timers, the release and the pour of `run` of `kind` at `now_ms`; a barrel goes into
    /// `uses`.
    fn tick_use(
        &mut self,
        uses: &mut SiegeUses,
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
            partial.extend(run.interact_change(MemberState::Interact(InteractState {
                enabled: Some(true),
                has_interactors: None,
            })));
        }
        if let Some(using) = run.uses.using {
            if using.pour_ms.is_some_and(|at| now_ms >= at) {
                self.drop_barrel(uses, run, kind, using, now_ms);
            }
            let poured = using.pour_ms.is_none_or(|at| now_ms >= at);
            let released = using.released || now_ms >= using.release_ms;
            if released && !using.released {
                partial.extend(run.interact_change(MemberState::Interact(InteractState {
                    enabled: None,
                    has_interactors: Some(NO_INTERACTORS),
                })));
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

    /// The pour of `run` ended: its barrel comes and starts to fall into `uses` (module doc).
    fn drop_barrel(
        &mut self,
        uses: &mut SiegeUses,
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
        let raid = self.use_raid_of(using.entity);
        if let Some(component) = drop.ownership_component {
            let ownership = BundleMember {
                component,
                state: MemberState::Ownership(self.owned_ownership(
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
        uses.drops.push(BarrelDrop {
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

    /// Opens `area` of `source` at `centre` at `now_ms` into `uses` (module doc); its first hits
    /// go into `outcomes`.
    pub(super) fn open_siege_area(
        &mut self,
        uses: &mut SiegeUses,
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
        let ownership = owner.map(|entity| self.owned_ownership(source.side, entity, source.raid));
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
            // A timer that starts on a trigger of the slice opens at 0 (Observed: the cow
            // launcher's area, `invasion` 335.079 s; [`SiegeTimer::on_open`]).
            members.extend(
                area.timers
                    .iter()
                    .map(|timer| timer_member(timer.component, timer.open_delay_s(), Some(false))),
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
                .filter(|timer| timer.on_open)
                .map(|timer| RunningTimer::of(timer, now_ms))
                .collect(),
        };
        self.give_area_effects(&run, now_ms);
        if self.tick_area(&mut run, now_ms, outcomes) {
            uses.areas.push(run);
        }
    }

    /// One tick of `run` at `now_ms`: its hits and its timers; `false` once it ended (its entity
    /// is gone).
    fn tick_area(
        &mut self,
        run: &mut AreaRun,
        now_ms: u64,
        outcomes: &mut Vec<Result<AttackOutcome, AttackRefusal>>,
    ) -> bool {
        if run.next_hit_ms.is_some_and(|at| now_ms >= at) && now_ms <= run.ends_ms {
            self.area_hits(run, now_ms, outcomes);
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

    /// The hits of `run` on what stands in it (module doc): the targets of a turret shot of its
    /// side ([`SimWorld::shot_targets`]); a turret takes its hit as a turret shot's
    /// ([`SimWorld::hurt_held_turret`]).
    fn area_hits(
        &mut self,
        run: &AreaRun,
        now_ms: u64,
        outcomes: &mut Vec<Result<AttackOutcome, AttackRefusal>>,
    ) {
        let Some(damage) = &run.area.damage else {
            return;
        };
        let row = self.combat.data.table_attack(&damage.table, &damage.row);
        let coefficient = row.map_or(0.0, |row| row.coefficient);
        let style = row.map(MobHitStyle::of_area);
        let health = area_hit_health(damage, coefficient);
        let amount = area_hit_amount(damage, coefficient);
        let AreaSource { side, number, .. } = run.source;
        let targets = self
            .with_turret_runs(|world, turrets| world.shot_targets(number, turrets, side))
            .unwrap_or_default();
        for (target, at, capsule) in targets {
            if !within(run.centre, run.area.radius_m, at, capsule) {
                continue;
            }
            match target {
                ShotTarget::Body(body) => {
                    let Some(outcome) = self.area_hit(body, number, health, style.as_ref(), now_ms)
                    else {
                        continue;
                    };
                    self.count_siege_hit(&outcome, number, amount);
                    outcomes.push(Ok(outcome));
                }
                ShotTarget::Turret { index, .. } => self.hurt_held_turret(index, amount, number),
            }
        }
    }

    /// The status effects of `run` on the living players of its side in its radius.
    fn give_area_effects(&mut self, run: &AreaRun, now_ms: u64) {
        if run.area.effects.is_empty() {
            return;
        }
        let content = std::sync::Arc::clone(&self.content);
        for entity in self.players_within(run.centre, run.area.radius_m) {
            if self.turret_side(entity) != Some(run.source.side) {
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

    /// Spawns the level's fire droppers once, on an open-world sim (module doc, **Level
    /// structures**): each with its retail first members, its level card (no asset), its slice's
    /// use interact when its first state had none, and the resting level ownership.
    pub(super) fn spawn_level_droppers(&mut self) {
        if self.instance_sim || self.wars.level.spawned {
            return;
        }
        self.wars.level.spawned = true;
        let content = std::sync::Arc::clone(&self.content);
        for dropper in content.war_forts.level_droppers() {
            let structure = FortStructure {
                interact_component: dropper
                    .structure
                    .interact_component
                    .or(dropper.kind.use_interact),
                side: LEVEL_SIDE,
                ..dropper.structure.clone()
            };
            let mut body = StructureBody::new(structure);
            let gde_ref = body.structure.gde_ref;
            let mut members = spawn_members(&body, |_| None, level_ownership());
            if let Some(card) = members
                .iter_mut()
                .find(|member| member.component == body.structure.card_component)
            {
                card.state = MemberState::GdeMetadata(GdeMetadataReplicatedState {
                    asset: None,
                    gde_ref: Some(gde_ref),
                });
            }
            let position = body.place();
            body.number = self.stations.place_camp(|_| Station {
                gde_ref,
                position,
                station_type: String::new(),
                name: Vec::new(),
                members,
            });
            self.wars.level.droppers.push(StructureRun {
                body,
                uses: UseState::default(),
                generation: None,
            });
        }
    }

    /// The areas of `uses` leave every view (an Outpost Rush match's end; the barrels' entities
    /// end with their flight, [`super::shot_entity`]).
    pub(super) fn remove_siege_uses(&mut self, uses: &SiegeUses) {
        for area in &uses.areas {
            self.stations.remove_camp(area.number);
        }
    }

    /// The use of `run` stops at once, and it shows no user (module doc: an Outpost Rush vat's
    /// ruin or new owner team): its pour drops no barrel (Inferred).
    fn stop_use(&mut self, run: &mut StructureRun) {
        run.uses = UseState::default();
        let partial = run.body.interact_member(NO_INTERACTORS).into_iter().collect();
        self.sync_structure(run, partial);
    }

    /// The outpost structure of Outpost Rush vat `run` turned to a ruin (module doc): its use
    /// stops and it is disabled until its rebuild.
    pub(super) fn break_outpost_dropper(&mut self, run: &mut StructureRun) {
        run.body.broken = true;
        self.stop_use(run);
    }

    /// Outpost Rush vat `run` stands built again after its ruin (module doc): free and enabled,
    /// as its first build made it.
    pub(super) fn mend_outpost_dropper(&mut self, run: &mut StructureRun) {
        run.body.broken = false;
        self.stop_use(run);
    }

    /// The outpost of Outpost Rush vat `run` has the owner team `side` now (module doc): a use
    /// of a player of another team stops.
    pub(super) fn own_outpost_dropper(&mut self, run: &mut StructureRun, side: u8) {
        run.body.structure.side = side;
        let user_team = run.uses.user_number().and_then(|number| {
            self.arena_matches
                .outpost_rush
                .as_ref()
                .and_then(|running| running.team_of(number))
        });
        if run.uses.user_number().is_some() && user_team != Some(side) {
            self.stop_use(run);
        }
    }
}

#[cfg(test)]
mod tests {
    use nw_content::status_effects::{EffectSourceRow, StatusEffectRules, StatusEffectSource};
    use nw_content::war_forts::{SIDE_ATTACKER, SIDE_DEFENDER, WarForts};
    use nw_formats::objectstream::lowercase_crc32;

    use nw_messages::client_input::TRIGGER_KIND;

    use super::*;
    use crate::player_content::PlayerContent;
    use crate::world::tests::{NOW, messages_to};
    use crate::world::turrets::tests::{
        CANNON_ASSET, CANNON_REF, DEFENDER, TABLE, cannon_call, forts_with_cannon_of, use_request,
        war_world_with,
    };

    impl SiegeUses {
        /// The first falling barrel: its entity's slot key, its sim ms of landing and where it
        /// lands.
        pub(in crate::world) fn first_drop(&self) -> Option<(u64, u64, Position)> {
            self.drops
                .first()
                .map(|drop| (drop.entity, drop.land_ms, drop.at))
        }

        /// The first open area: its slot key and its centre.
        pub(in crate::world) fn first_area(&self) -> Option<(u64, Position)> {
            self.areas.first().map(|area| (area.number, area.centre))
        }
    }

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
    /// The horn's longest blow timer.
    const HORN_COOLDOWN_MS: u64 = 90_000;
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
        use_world_with(|_| {})
    }

    /// The world of [`use_world`] with `change` made to its table.
    fn use_world_with(change: impl FnOnce(&mut serde_json::Value)) -> (SimWorld, Entity) {
        let mut table: serde_json::Value = serde_json::from_str(TABLE).expect("test table");
        table["forts"][0]["structures"]
            .as_array_mut()
            .expect("structures")
            .extend(structures().as_array().expect("rows").iter().cloned());
        table["structure_kinds"] = kinds();
        change(&mut table);
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
                MemberState::InteractCooldown(state) => Some(state.interact),
                _ => None,
            })
            .expect("interact")
    }

    /// The cooldowns an interact member of `members` carries; `None` for one without.
    fn cooldowns_of(members: &[BundleMember]) -> Option<FullList<InteractCooldown>> {
        members.iter().find_map(|member| match &member.state {
            MemberState::InteractCooldown(state) => Some(state.cooldown_updates.clone()),
            _ => None,
        })
    }

    /// The cooldown list of a use at server ms `server_ms` that lasts `cooldown_ms`.
    fn cooldown_list(version: u64, server_ms: u64, cooldown_ms: u64) -> FullList<InteractCooldown> {
        FullList {
            version: Some(version),
            elements: vec![InteractCooldown {
                option: USE_OPTION,
                ends_unix_ns: unix_ns(server_ms + cooldown_ms),
            }],
        }
    }

    /// The interact change with a cooldown that goes to the viewers of station `number`.
    fn cooldown_change(world: &SimWorld, number: u64) -> Option<InteractCooldownState> {
        const KEPT: crate::world::slots::Slot = crate::world::slots::Slot {
            interest: 0,
            new: false,
        };
        world
            .stations
            .group(number, KEPT)?
            .members
            .into_iter()
            .find_map(|member| match member.state {
                MemberState::InteractCooldown(state) => Some(state),
                _ => None,
            })
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
        let server_ms = world.server_ms;
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
        let cooldown = cooldown_list(FIRST_COOLDOWN_VERSION, server_ms, DROPPER_COOLDOWN_MS);
        assert_eq!(
            cooldown_change(&world, number),
            Some(InteractCooldownState {
                interact: InteractState {
                    enabled: None,
                    has_interactors: Some(ONE_USER),
                },
                cooldown_updates: cooldown.clone(),
            }),
            "the use's change carries the 20 s cooldown of option 0 (retail `01 06`)"
        );
        assert_eq!(cooldowns_of(&shown), Some(cooldown.clone()));
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
            .uses
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
        assert!(fight.uses.drops.is_empty());
        let area = fight.uses.areas.first().cloned().expect("the barrel's area");
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
        let shown = members(&world, number);
        assert_eq!(interact_of(&shown).has_interactors, Some(NO_INTERACTORS));
        assert_eq!(
            cooldowns_of(&shown),
            Some(cooldown),
            "its cards keep the cooldown"
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
        assert!(world.war_fight().expect("fight").uses.areas.is_empty());
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
            NOW + DROPPER_COOLDOWN_MS - 1
        ));
        assert!(
            !accepted(&messages_to(&world.take_messages(), DEFENDER)),
            "on cooldown"
        );
        assert!(world.turret_interaction(
            entity,
            DEFENDER,
            trigger_request(DROPPER_REF),
            b"",
            NOW + DROPPER_COOLDOWN_MS
        ));
        assert!(
            accepted(&messages_to(&world.take_messages(), DEFENDER)),
            "free again after its cooldown"
        );
        assert_eq!(
            cooldowns_of(&members(&world, number)).and_then(|list| list.version),
            Some(FIRST_COOLDOWN_VERSION + COOLDOWN_VERSION_STEP),
            "the second use's list"
        );
    }

    /// A dropper whose first state had no interact shows its user on its slice's use interact
    /// (Observed: the T1 dropper's interact is component 17 in the war captures and the slice).
    #[test]
    fn a_dropper_with_no_retail_interact_shows_its_user_on_the_slices_use_interact() {
        const USE_INTERACT: u64 = 17;
        let (mut world, entity) = use_world_with(|table| {
            let structures = table["forts"][0]["structures"]
                .as_array_mut()
                .expect("structures");
            let dropper = structures
                .iter_mut()
                .find(|row| row["asset"] == DROPPER_ASSET)
                .expect("the dropper row");
            dropper["interact_component"] = serde_json::Value::Null;
            dropper["members"]
                .as_array_mut()
                .expect("members")
                .retain(|member| *member != INTERACT);
            table["structure_kinds"][0]["use_interact"] = serde_json::json!(USE_INTERACT);
        });
        assert_eq!(
            structure(&world, &DROPPER_REF).body.structure.interact_component,
            Some(USE_INTERACT)
        );
        assert!(world.turret_interaction(
            entity,
            DEFENDER,
            trigger_request(DROPPER_REF),
            b"",
            NOW
        ));
        let number = structure(&world, &DROPPER_REF).body.number;
        let shown = members(&world, number);
        let interact = shown
            .iter()
            .find(|member| member.component == USE_INTERACT)
            .expect("the use interact");
        assert_eq!(
            interact_of(std::slice::from_ref(interact)).has_interactors,
            Some(ONE_USER),
            "the user, with the use's cooldown"
        );
    }

    #[test]
    fn a_horn_blow_gives_its_side_the_effect_and_waits_for_its_longest_timer() {
        let (mut world, entity) = use_world();
        let server_ms = world.server_ms;
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
        assert_eq!(
            cooldown_change(&world, number),
            Some(InteractCooldownState {
                interact: InteractState {
                    enabled: Some(false),
                    has_interactors: Some(ONE_USER),
                },
                cooldown_updates: cooldown_list(FIRST_COOLDOWN_VERSION, server_ms, HORN_COOLDOWN_MS),
            }),
            "the blow's change carries the 90 s cooldown (retail `01 07`, `invasion` 95.691 s)"
        );
        world.tick_turrets(NOW + 1);
        assert!(
            !world.war_fight().expect("fight").uses.areas.is_empty(),
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
            .uses
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
            world.war_fight().expect("fight").uses.areas.is_empty(),
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
        assert!(
            !accepted(&messages_to(&world.take_messages(), DEFENDER)),
            "on again, but on cooldown until its longest timer"
        );
        assert!(world.turret_interaction(
            entity,
            DEFENDER,
            trigger_request(HORN_REF),
            b"",
            NOW + HORN_COOLDOWN_MS
        ));
        assert!(accepted(&messages_to(&world.take_messages(), DEFENDER)));
    }

    /// A turret's shot meets the war's structures of both sides: one of the other side takes
    /// its health, one of the shooter's side stops it unharmed (turrets module doc, **Shots on
    /// structures**; `OPR_TEST3` 1719.807 s).
    #[test]
    fn a_turret_shot_meets_the_structures_of_both_sides_and_hurts_a_foes_only() {
        use crate::world::turrets::{HeldHit, TurretShotTarget};
        const HIT: u32 = 1_000;
        let (mut world, _) = use_world();
        let mut fight = world
            .instance_run
            .as_mut()
            .and_then(|run| run.take_war_fight())
            .expect("fight");
        world.turrets_at_start(&mut fight);
        if let Some(run) = world.instance_run.as_mut() {
            run.put_war_fight(fight);
        }
        let turrets = world.war_fight().expect("fight").turrets.clone();
        let dropper = structure(&world, &DROPPER_REF).body.clone();
        let met = |world: &mut SimWorld, side| {
            world
                .turret_shot_targets(DEFENDER, &turrets, side)
                .into_iter()
                .find_map(|(target, _, _)| {
                    matches!(target, TurretShotTarget::Structure { number, .. } if number == dropper.number)
                        .then_some(target)
                })
                .expect("the dropper")
        };
        let own = met(&mut world, SIDE_DEFENDER);
        assert_eq!(
            own,
            TurretShotTarget::Structure {
                number: dropper.number,
                facet: dropper.structure.facet_entity(),
                foe: false,
            }
        );
        assert_eq!(own.held(), None, "its own side's stops the shot unharmed");
        let foe = met(&mut world, SIDE_ATTACKER);
        assert_eq!(foe.held(), Some(HeldHit::Structure(dropper.number)));
        world.hurt_held(HeldHit::Structure(dropper.number), HIT, DEFENDER);
        assert!(
            (structure(&world, &DROPPER_REF).body.health - (dropper.health - HIT as f32)).abs()
                < f32::EPSILON * dropper.health,
            "the foe's dropper takes the hit"
        );
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
        let opened = fight.uses.areas.first().expect("the explosion");
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

    /// The retail hits of siege areas on the own player: the area's health times the share the
    /// armor kept (the 3601 item's scale) is the item's amount.
    #[test]
    fn a_siege_area_hit_on_a_player_is_its_health_times_what_the_armor_kept() {
        // `invasion-71657c7d8159622a` 862.680 s: a T1 barrel's burn, item `9:1125.1355:0.707632`.
        const BARREL_KEPT: f64 = 0.707_632;
        const BARREL_HIT: f64 = 1_125.135_5;
        // `CK_offensive_war_2` 1462.586 s: a T2 shell's explosion, item `6:2824.2261:0.536361`.
        const SHELL_KEPT: f64 = 0.536_361;
        const SHELL_HIT: f64 = 2_824.226_1;
        const RETAIL_PRECISION: f64 = 0.05;
        let barrel = AreaDamage {
            table: "DamageTable_Structures".to_owned(),
            row: "FireBarrelDropper_DoT".to_owned(),
            base_damage: 1_200,
            rate_s: 0.5,
        };
        let barrel_hit = area_hit_health(&barrel, 1.325) * BARREL_KEPT;
        assert!(
            (barrel_hit - BARREL_HIT).abs() < RETAIL_PRECISION,
            "{barrel_hit}"
        );
        let shell = AreaDamage {
            table: "DamageTable_Structures".to_owned(),
            row: "BombTurret_Explosion".to_owned(),
            base_damage: 4_388,
            rate_s: 5.0,
        };
        let shell_hit = area_hit_health(&shell, 1.2) * SHELL_KEPT;
        assert!(
            (shell_hit - SHELL_HIT).abs() < RETAIL_PRECISION,
            "{shell_hit}"
        );
    }

    /// The Hedet canyon dropper `Dropper_Fire_POI_2209365890` (capital id `c6822e7ecf34dcb3`) as a
    /// level dropper with the test dropper's members and use parts (its own slice holds the same
    /// dropper parts: the `DropFlameBarrel` option, a 0.3 s pour, a fire barrel and its area).
    const LEVEL_REF: [u8; 16] =
        *b"\xb3\xdc\x34\xcf\x7e\x2e\x82\xc6\x00\x00\x00\x00\x00\x00\x00\x00";

    /// An open-world sim with the level dropper and one player near it.
    fn level_world() -> (SimWorld, Entity) {
        use crate::world::tests::{SERVER_TIME, admit_at, world_with_spawns};
        let mut structure = structures()[0].clone();
        structure["gde_ref"] = serde_json::json!(hex::encode(LEVEL_REF));
        structure["place"] = serde_json::json!([5.0, 0.0, 0.0]);
        let mut kind = kinds()[0].clone();
        kind["slice"] = serde_json::json!("coatgen/a4041713/dropper_fire_poi_2209365890.dynamicslice");
        let table = serde_json::json!({"forts": [], "level_droppers": [{
            "capital": "b212b88f-c633-a507-7490-96f1090779b4", "members_inferred": false,
            "structure": structure, "kind": kind}]});
        let forts = WarForts::parse(&table.to_string()).expect("table");
        let mut world = world_with_spawns(Vec::new());
        world.content = std::sync::Arc::new(PlayerContent {
            war_forts: forts,
            ..PlayerContent::default()
        });
        let admit = admit_at(DEFENDER, 0.0, true);
        world.admit(&admit, NOW, SERVER_TIME).expect("admit");
        let entity = world.player_by_number(DEFENDER).expect("player");
        world.tick_turrets(NOW);
        world.take_messages();
        (world, entity)
    }

    fn level_dropper(world: &SimWorld) -> &StructureRun {
        world
            .wars
            .level
            .droppers
            .iter()
            .find(|run| run.body.structure.gde_ref == LEVEL_REF)
            .expect("the level dropper spawned")
    }

    /// Module doc, **Level structures**: the level dropper spawns once on an open-world sim
    /// with its level card (no asset) and the resting level ownership; any player uses it, its
    /// pour drops the barrel, and its 20 s cooldown refuses another use.
    #[test]
    fn a_level_dropper_shows_its_level_card_and_drops_a_barrel_for_any_player() {
        let (mut world, entity) = level_world();
        assert_eq!(world.turret_host(), Some(TurretHost::Level));
        assert_eq!(world.turret_side(entity), Some(LEVEL_SIDE));
        let number = level_dropper(&world).body.number;
        let shown = members(&world, number);
        assert!(shown.iter().any(|member| matches!(&member.state,
            MemberState::GdeMetadata(card) if card.asset.is_none() && card.gde_ref == Some(LEVEL_REF))));
        let ownership = shown.iter().find_map(|member| match &member.state {
            MemberState::Ownership(state) => Some(state.clone()),
            _ => None,
        });
        assert_eq!(ownership, Some(level_ownership()));
        world.tick_turrets(NOW);
        assert_eq!(world.wars.level.droppers.len(), 1, "spawned once");
        assert!(world.turret_interaction(
            entity,
            DEFENDER,
            trigger_request(LEVEL_REF),
            b"Dropper_Fire_POI",
            NOW
        ));
        let sent = messages_to(&world.take_messages(), DEFENDER);
        assert!(accepted(&sent), "{sent:?}");
        world.tick_turrets(NOW + POUR_MS);
        assert!(world.wars.level.uses.first_drop().is_some(), "the barrel falls");
        world.tick_turrets(NOW + DROPPER_USE_MS);
        assert!(world.turret_interaction(
            entity,
            DEFENDER,
            trigger_request(LEVEL_REF),
            b"Dropper_Fire_POI",
            NOW + DROPPER_USE_MS + 1
        ));
        let sent = messages_to(&world.take_messages(), DEFENDER);
        assert!(!accepted(&sent), "the cooldown runs");
        assert!(level_dropper(&world).body.invulnerable, "no harm in the open world");
    }
}
