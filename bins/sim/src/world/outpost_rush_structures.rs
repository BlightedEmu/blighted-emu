// SPDX-License-Identifier: AGPL-3.0-or-later
//! The structures of the Outpost Rush outposts (game mode helper B10, findings
//! `2026-11-13-outpost-rush-structures.md`; the spawners and the retail members from
//! [`nw_content::outpost_structures`], the costs from the blueprints'
//! [`nw_content::crafting::StructureCosts`], the health and build time from
//! [`nw_content::outpost_structures::OutpostBlueprints`]).
//!
//! **The spawn** (Observed, all 12 OPR ledgers): every structure spawner of the map's layout
//! slice spawns with the match: a card of its slice with a new `GdeRef`, its place (the
//! spawner's place plus the layout's placement, [`super::outpost_rush_doors::layout_placement`];
//! the retail gates, command posts and turrets of `opr_que_and_lost`, `opr_match_2` and
//! `OPR_TEST3` stood there to the centimetre), its slice's first members and the ownership of
//! the fort barriers with raid 0. The armory stands built (buildable state 5, its status 1 in
//! the first state of every ledger); the command post, the turrets, the gates and the ward are
//! foundations (state 4); the storage shed and the AI objective manager have no buildable state.
//!
//! **The ownership** (Observed, `OPR_TEST3` 298.3 s): raid 0 while no team owns the outpost,
//! the owner team's raid after. A new owner sends the zero company and the raid (`OPR_TEST3`
//! 298.261 s: 42 bodies; 1733.622 s: 11), and the raid alone to a turret and a vat whose
//! ownership a use named (1733.622 s the vat of slot 123, 1733.638 s the repeater of slot 79,
//! both used before): such a structure keeps its last user's company (Inferred: a use's
//! company stays until the next use). Inferred: only the owner team builds, upgrades and
//! repairs, and only the other team hurts a built structure.
//!
//! **A build** (Observed, `OPR_TEST3` 1032.712 s): the interaction of kind
//! [`APPLY_ALL_RESOURCES_KIND`] is accepted [`ACCEPT_DELAY_MS`] after its first request (3743
//! status 1) and ends 10 ms later (3746). The structure's health then rises at its blueprint's
//! `HP / BuildingTime` a second, sent about every [`BUILD_SEND_MS`] (gate t1 9,000 a step, command
//! post 5,640 to 5,700), up to the share of its cost committed (command post t1 15,000 of 30,000
//! at 556.6 s; finished 1049.8 to 1052.2 s); at full health it is built (state 5) and its outpost
//! status shows its tier. The committed resource list stays empty in every retail state.
//! Inferred: the accept takes from the bag every resource of the cost not yet committed (as much
//! as the bag holds); a request that can commit nothing is refused (status 2) and ends.
//!
//! **An upgrade** (Observed, `OPR_TEST3` command post 1052.2 to 1060.1 s): the built tier turns
//! to state 7 and the next tier's slice spawns at the same place as a foundation with health 0;
//! its build is a build as above; when it is built the old tier leaves and the status shows the
//! new tier (`CommandPost` 1, 2, 3 at 1052.250, 1060.130, 1068.182 s). The client asks with
//! [`UPGRADE_BUILDABLE_KIND`] (`opr_match_4`, 4 requests). Inferred: only a whole built structure
//! with a next tier in the table upgrades.
//!
//! **A ruin** (Observed): at 0 health the structure turns to state 6 with a ruin damage type
//! (10, `OPR_TEST3` 1236.1 s) and its status gets [`RUIN_BIT`] with its tier kept (gate 1 to 5,
//! 3 to 7); a rebuild raises its health from 0 like a build (9,000 a step, 1013.7 s) and the
//! status loses the bit when it is whole (5 to 1 at 1016.780 s; 7 to 3 in
//! `OPR-Coral-Divide-2` 1009.255 s). Inferred: a hit takes the committed share its health lost,
//! so a repair of a damaged or ruined structure costs that share again; a hit takes the level
//! amount of the attack as on the war structures ([`super::turrets`]) at the turret body.
//!
//! **A turret** (the repeater, ballista and cannon slices; [`super::turrets`] module doc): at
//! its first build it becomes a turret of the match ([`OutpostStructures::turrets`]) of its
//! outpost's owner team, with the turret (4276), use interact (2930) and projectile spawner (57)
//! states of its slice's built state, which the turret sends as its own (idle, no interactor).
//! Observed (`opr_que_and_lost`): a built repeater has two interacts, the build's (component
//! 15, disabled at the build, 1708.830 s) and the use's (component 11, one interactor while a
//! player used it, 1710.646 s; the ballista's 16 and 12, 1757.994 s): the use's is the one the
//! build does not disable. Inferred: the use interact is enabled at each build. A ruin breaks
//! the turret, a rebuild mends it, a new owner team takes it.
//!
//! **A burning oil vat** (`or_turretfire`, `or_turretfire_trench`; [`super::siege_areas`] module
//! doc): at its first build it becomes a fire barrel dropper of the match
//! ([`OutpostStructures::droppers`]) of its outpost's owner team, with the projectile spawner
//! (57) of its slice's built state and its slice's use interact
//! ([`nw_content::war_forts::StructureKind::use_interact`], component 16; Observed: the built
//! state of `OPR_TEST3` 1706.442 s holds only the build interact 19, disabled, and the use
//! interact first comes with the user at 1714.326 s); its uses, barrels and areas are the war
//! dropper's ([`OutpostStructures::uses`]). Inferred: its use interact is enabled with no
//! interactor at each build, as a turret's; a ruin stops its use and disables it, a rebuild
//! frees it, a new owner team takes it.

use bevy_ecs::entity::Entity;
use nw_content::outpost_structures::{
    OUTPOSTS, StructureRole, StructureSlice, StructureSpawner, Template,
};
use nw_content::war_forts::FortStructure;
use nw_messages::buildables::GUILD_ID_LEN;
use nw_messages::entity_ids::facet_of_raw_id;
use nw_messages::facet::{InteractionEndedMsg, component};
use nw_messages::items::SYNC_CRAFT_USED;
use nw_messages::turrets::PROJECTILE_SPAWNER_TYPE_INDEX;
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    BUILDABLE_CONTROLLER_TYPE_INDEX, BuildableControllerState, BundleMember, FacetHeader,
    InteractState, InteractionRequest, MemberState, OwnershipState, ServerMessage,
    VITALS_TYPE_INDEX, VitalsSlot0, VitalsState,
};
use nw_sim_core::Position;

use super::camps::APPLY_ALL_RESOURCES_KIND;
use super::crafting::{Station, dynamic_ref};
use super::encounter_roots::{card, place_member};
use super::gates::merge_members;
use super::outpost_rush_doors::{layout_placement, retail_half_yaw, team_ownership_on};
use super::outpost_rush::NO_TEAM;
use super::siege_areas::{SiegeUses, apply_ownership_change};
use super::siege_structures::{
    BROKEN_RUIN_TYPE, BROKEN_STATE, BUILT_STATE, NO_INTERACTORS, StructureRun,
};
use super::turrets::{TurretRun, USE_ACCEPTED, USE_REFUSED};
use super::{Player, SimWorld, gather_answer};

/// The interaction kind of an upgrade (`InteractionUIActions` `UpgradeBuildable` 0x25, client
/// `FUN_1435eaa20`); a build or repair is the camps' `ApplyAllResources`
/// ([`APPLY_ALL_RESOURCES_KIND`]).
pub(super) const UPGRADE_BUILDABLE_KIND: u8 = 37;
/// The buildable states of a foundation and of a tier an upgrade replaces (Observed).
const FOUNDATION_STATE: u8 = 4;
const REPLACED_STATE: u8 = 7;
/// How long after its first request a build is accepted, ms (Observed 1032.712 -> 1033.269 s).
pub(super) const ACCEPT_DELAY_MS: u64 = 557;
/// How often a rising health goes out, ms (Observed: about 0.8 s a step).
pub(super) const BUILD_SEND_MS: u64 = 800;
/// The bits of a structure in an outpost status, and the bit of a ruin (Observed).
const STATUS_BITS: u32 = 3;
pub(super) const RUIN_BIT: u64 = 4;
/// The raid of a structure whose outpost no team owns (Observed).
const NO_TEAM_RAID: u64 = 0;
/// Names the ids this server makes for outpost structures ("oprstruc").
const STRUCTURE_NAMESPACE: u64 = 0x6f70_7273_7472_7563;
const MS_PER_SECOND: f32 = 1_000.0;

/// What a structure is now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    /// A foundation (state 4): not built yet, or its rebuild runs after a ruin.
    Foundation,
    /// Built (state 5).
    Built,
    /// At 0 health (state 6).
    Ruin,
    /// A built tier whose upgrade builds (state 7).
    Replaced,
}

/// One structure of the match.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct OutpostStructure {
    /// Its slot key.
    pub(super) number: u64,
    pub(super) outpost: u8,
    role: StructureRole,
    /// Its status slot in its outpost's status.
    slot: Option<u32>,
    pub(super) asset: [u8; 16],
    pub(super) gde_ref: [u8; RAW_ID_LEN],
    pub(super) place: [f32; 3],
    /// Its facing about the up axis, radians, and the half of it retail sends.
    yaw: f32,
    half_yaw: f32,
    blueprint: Option<String>,
    tier: u8,
    /// The tier its status shows: its own when built, the tier it upgrades from while its
    /// build runs, 0 for a first build.
    shown_tier: u8,
    /// It was ruined and is not whole again yet.
    ruined: bool,
    pub(super) phase: Phase,
    template: Template,
    /// The members its slice's built state has that its foundation lacks.
    built_extras: Vec<BundleMember>,
    /// The components of its vitals and buildable states, and its retail vitals.
    vitals: Option<(u64, VitalsState)>,
    buildable: Option<(u64, BuildableControllerState)>,
    pub(super) health: f32,
    pub(super) max_health: f32,
    rate_per_s: f32,
    /// Its blueprint's cost and the part of each resource committed.
    cost: Vec<(String, u32)>,
    pub(super) committed: Vec<u32>,
    raid: u64,
    last_send_ms: u64,
    /// The slot key of the tier its upgrade replaces.
    replaces: Option<u64>,
}

/// A build or upgrade request waiting for its accept.
#[derive(Debug, Clone, PartialEq)]
struct PendingRequest {
    entity: Entity,
    number: u64,
    request: InteractionRequest,
    name: Vec<u8>,
    due_ms: u64,
}

/// The structures of the match and the requests on them.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct OutpostStructures {
    pub(super) structures: Vec<OutpostStructure>,
    pending: Vec<PendingRequest>,
    /// The resources the structures took since the last tick: player, item and quantity, for
    /// the war board (`outpost_rush::board`).
    pub(super) deposits: Vec<(u64, String, u32)>,
    /// The turrets of the turret structures built once ([`super::turrets`]), by slot key.
    pub(super) turrets: Vec<TurretRun>,
    /// The droppers of the vats built once ([`super::siege_areas`]), by slot key, and what
    /// their uses opened.
    pub(super) droppers: Vec<StructureRun>,
    pub(super) uses: SiegeUses,
}

/// The first retail member of class `class` in `template`, with its component.
fn template_member(template: &Template, class: u64) -> Option<BundleMember> {
    template.members.iter().find_map(|bytes| {
        let (member, _) = BundleMember::read_from(bytes).ok()?;
        (member.state.type_index() == class).then_some(member)
    })
}

/// The members of `template`.
pub(super) fn template_members(template: &Template) -> Vec<BundleMember> {
    template
        .members
        .iter()
        .filter_map(|bytes| BundleMember::read_from(bytes).ok().map(|(member, _)| member))
        .collect()
}

/// The members of `slice`'s built state whose components its foundation lacks.
fn built_extras(slice: &StructureSlice) -> Vec<BundleMember> {
    let (Some(foundation), Some(built)) = (&slice.foundation, &slice.built) else {
        return Vec::new();
    };
    let known: Vec<u64> = template_members(foundation)
        .iter()
        .map(|member| member.component)
        .collect();
    template_members(built)
        .into_iter()
        .filter(|member| !known.contains(&member.component))
        .collect()
}

impl OutpostStructure {
    /// The structure of `spawner` made of `slice` at the world place `place` with id `gde_ref`:
    /// built when `built` (or when its slice has no buildable state), else a foundation with
    /// health 0, of the health and build rate of its blueprint and of `cost`. A slice with a
    /// built template alone (the armory tiers: no capture shows an armory foundation) makes its
    /// foundation of that template in the foundation state (Inferred: as the command post's
    /// upgrade, Observed, starts as a foundation of its next tier).
    fn new(
        spawner: &StructureSpawner,
        slice: &StructureSlice,
        place: [f32; 3],
        gde_ref: [u8; RAW_ID_LEN],
        built: bool,
        values: (f32, f32, Vec<(String, u32)>),
    ) -> Option<Self> {
        let (template, phase) = match (&slice.foundation, &slice.built) {
            (_, Some(whole)) if built => (whole.clone(), Phase::Built),
            (Some(foundation), _) => (foundation.clone(), Phase::Foundation),
            (None, Some(whole)) => (whole.clone(), Phase::Foundation),
            (None, None) => return None,
        };
        let vitals = match template_member(&template, VITALS_TYPE_INDEX) {
            Some(BundleMember {
                component,
                state: MemberState::Vitals(state),
            }) => Some((component, state)),
            _ => None,
        };
        let buildable = match template_member(&template, BUILDABLE_CONTROLLER_TYPE_INDEX) {
            Some(BundleMember {
                component,
                state: MemberState::BuildableController(state),
            }) => Some((component, state)),
            _ => None,
        };
        let phase = if buildable.is_none() {
            Phase::Built
        } else {
            phase
        };
        let (max_health, build_time_s, cost) = values;
        let committed = cost
            .iter()
            .map(|(_, whole)| if phase == Phase::Built { *whole } else { 0 })
            .collect();
        Some(Self {
            number: 0,
            outpost: spawner.outpost,
            role: spawner.role,
            slot: spawner.role.status_slot(spawner.number),
            asset: slice.asset,
            gde_ref,
            place,
            yaw: spawner.yaw,
            half_yaw: retail_half_yaw(spawner.yaw),
            blueprint: slice.blueprint.clone(),
            tier: slice.tier,
            shown_tier: if phase == Phase::Built { slice.tier } else { 0 },
            ruined: false,
            phase,
            built_extras: built_extras(slice),
            template,
            vitals,
            buildable,
            health: if phase == Phase::Built { max_health } else { 0.0 },
            max_health,
            rate_per_s: if build_time_s > 0.0 {
                max_health / build_time_s
            } else {
                max_health
            },
            cost,
            committed,
            raid: NO_TEAM_RAID,
            last_send_ms: 0,
            replaces: None,
        })
    }

    /// Whether it has a buildable state.
    const fn buildable(&self) -> bool {
        self.buildable.is_some()
    }

    /// Its code in its outpost's status: the tier shown, with [`RUIN_BIT`] while ruined.
    pub(super) fn status_code(&self) -> u64 {
        let ruin = if self.ruined || self.phase == Phase::Ruin {
            RUIN_BIT
        } else {
            0
        };
        u64::from(self.shown_tier) | ruin
    }

    /// The share of its cost committed, 0 to 1.
    fn committed_share(&self) -> f32 {
        let whole: u32 = self.cost.iter().map(|(_, quantity)| quantity).sum();
        if whole == 0 {
            return 1.0;
        }
        let done: u32 = self.committed.iter().sum();
        done as f32 / whole as f32
    }

    /// The health its committed share stands for.
    fn target_health(&self) -> f32 {
        self.max_health * self.committed_share()
    }

    /// The resources a build still needs: (index in the cost, item, quantity).
    fn needed(&self) -> Vec<(usize, String, u32)> {
        self.cost
            .iter()
            .zip(&self.committed)
            .enumerate()
            .filter_map(|(index, ((item, whole), done))| {
                let left = whole.saturating_sub(*done);
                (left > 0).then(|| (index, item.clone(), left))
            })
            .collect()
    }

    /// Whether a build of the team of raid `raid` can commit to it now.
    fn takes_resources(&self, raid: u64) -> bool {
        raid != NO_TEAM_RAID
            && self.raid == raid
            && self.buildable()
            && self.phase != Phase::Replaced
            && !self.needed().is_empty()
    }

    /// Its full vitals member with `health`.
    fn vitals_member(&self, health: f32) -> Option<BundleMember> {
        let (component, state) = self.vitals.as_ref()?;
        let slot = state.slot_0.unwrap_or_default();
        Some(BundleMember {
            component: *component,
            state: MemberState::Vitals(VitalsState {
                slot_0: Some(VitalsSlot0 {
                    health_amount: Some(health),
                    ..slot
                }),
                ..state.clone()
            }),
        })
    }

    /// The change of its health alone (Observed: a rising or falling health goes out alone).
    fn health_change(&self) -> Option<BundleMember> {
        Some(BundleMember {
            component: self.vitals.as_ref()?.0,
            state: MemberState::Vitals(VitalsState {
                slot_0: Some(VitalsSlot0 {
                    health_amount: Some(self.health),
                    ..VitalsSlot0::default()
                }),
                slot_1: None,
            }),
        })
    }

    /// Its buildable member in its phase.
    fn buildable_member(&self) -> Option<BundleMember> {
        let (component, state) = self.buildable.as_ref()?;
        let (current, ruin) = match self.phase {
            Phase::Foundation => (FOUNDATION_STATE, None),
            Phase::Built => (BUILT_STATE, None),
            Phase::Ruin => (BROKEN_STATE, Some(BROKEN_RUIN_TYPE)),
            Phase::Replaced => (REPLACED_STATE, None),
        };
        Some(BundleMember {
            component: *component,
            state: MemberState::BuildableController(BuildableControllerState {
                current_state: Some(current),
                ruin_damage_type: ruin,
                ..state.clone()
            }),
        })
    }

    /// Its ownership member, when its slice has one.
    fn ownership_member(&self) -> Option<BundleMember> {
        self.template
            .ownership_component
            .map(|component| team_ownership_on(component, self.raid))
    }

    /// The ownership change that its outpost's new owner raid sends (module doc, **The
    /// ownership**): the raid alone for a structure whose ownership a use named (`used`), else
    /// the zero company and the raid; `None` when its slice has no ownership.
    fn raid_change(&self, used: bool) -> Option<BundleMember> {
        let component = self.template.ownership_component?;
        Some(BundleMember {
            component,
            state: MemberState::Ownership(OwnershipState {
                guild_id: (!used).then_some([0; GUILD_ID_LEN]),
                raid_id: Some(self.raid),
                ..OwnershipState::default()
            }),
        })
    }

    /// Its members at its spawn (module doc).
    fn spawn_members(&self) -> Vec<BundleMember> {
        let mut members = vec![
            BundleMember {
                component: self.template.card_component,
                ..card(self.asset, self.gde_ref)
            },
            place_member(self.template.place_component, self.place, self.half_yaw),
        ];
        members.extend(template_members(&self.template));
        if self.phase == Phase::Built {
            merge_members(&mut members, &self.built_extras);
        }
        let own: Vec<BundleMember> = self
            .buildable_member()
            .into_iter()
            .chain(self.ownership_member())
            .chain(
                (self.phase != Phase::Foundation || self.health > 0.0)
                    .then(|| self.vitals_member(self.health))
                    .flatten(),
            )
            .collect();
        merge_members(&mut members, &own);
        members
    }

    /// Its siege part of the owner team `side` (module doc): a turret ([`super::turrets`]) or a
    /// vat's dropper ([`super::siege_areas`]): its card, place, facing and ownership component
    /// with the turret (4276) and projectile spawner (57) members of its slice's built state
    /// and its use interact (2930), enabled with no interactor: the interact on component
    /// `use_interact`, else the built state's interact that its build does not disable. `None`
    /// for a structure whose built state has neither a turret nor a use interact.
    fn siege_structure(&self, side: u8, use_interact: Option<u64>) -> Option<FortStructure> {
        let enabled = |component: u64| BundleMember {
            component,
            state: MemberState::Interact(InteractState {
                enabled: Some(true),
                has_interactors: Some(NO_INTERACTORS),
            }),
        };
        let mut turret = None;
        let mut interact = use_interact.map(enabled);
        let mut spawner = None;
        for member in &self.built_extras {
            match &member.state {
                MemberState::Turret(_) => turret = Some(member.clone()),
                MemberState::Interact(state)
                    if use_interact.is_none() && state.enabled != Some(false) =>
                {
                    interact = Some(enabled(member.component));
                }
                state if state.type_index() == PROJECTILE_SPAWNER_TYPE_INDEX => {
                    spawner = Some(member.clone());
                }
                _ => {}
            }
        }
        if turret.is_none() && interact.is_none() {
            return None;
        }
        let members = [turret.as_ref(), interact.as_ref(), spawner.as_ref()]
            .into_iter()
            .flatten()
            .filter_map(|member| member.encode().ok())
            .collect();
        Some(FortStructure {
            asset: self.asset,
            gde_ref: self.gde_ref,
            place: self.place,
            yaw: self.yaw,
            side,
            card_component: self.template.card_component,
            turret_component: turret.as_ref().map(|member| member.component),
            interact_component: interact.as_ref().map(|member| member.component),
            spawner_component: spawner.as_ref().map(|member| member.component),
            ownership_component: self.template.ownership_component,
            generator_component: None,
            container_component: None,
            members,
        })
    }

    /// Whether a hit of a player of the team of raid `raid` meets it: built, with vitals, of an
    /// outpost the other team owns.
    fn open_to(&self, raid: u64) -> bool {
        self.phase == Phase::Built
            && self.vitals.is_some()
            && self.raid != NO_TEAM_RAID
            && self.raid != raid
    }

    /// Its place in the world.
    const fn position(&self) -> Position {
        let [x, y, z] = self.place;
        Position { x, y, z }
    }
}

impl OutpostStructures {
    /// The status values of the three outposts (three bits a structure; module doc).
    pub(super) fn statuses(&self) -> [u64; OUTPOSTS] {
        let mut statuses = [0; OUTPOSTS];
        for structure in &self.structures {
            let (Some(slot), Some(status)) =
                (structure.slot, statuses.get_mut(usize::from(structure.outpost)))
            else {
                continue;
            };
            if structure.phase == Phase::Replaced {
                continue;
            }
            *status |= structure.status_code() << (slot * STATUS_BITS);
        }
        statuses
    }

    /// Player `entity` (`number`) asked for a build or upgrade `request` (named `name`) at
    /// `now_ms`: it waits for its accept ([`ACCEPT_DELAY_MS`]); `false` when it names no
    /// structure of the match.
    pub(super) fn ask(
        &mut self,
        entity: Entity,
        number: u64,
        request: InteractionRequest,
        name: &[u8],
        now_ms: u64,
    ) -> bool {
        if self.index_of_ref(&request.target).is_none() {
            return false;
        }
        self.pending.push(PendingRequest {
            entity,
            number,
            request,
            name: name.to_vec(),
            due_ms: now_ms.saturating_add(ACCEPT_DELAY_MS),
        });
        true
    }

    /// Whether the structure of slot key `number` is one of the match.
    pub(super) fn holds(&self, number: u64) -> bool {
        self.index_of(number).is_some()
    }

    /// The structure of slot key `number`.
    fn index_of(&self, number: u64) -> Option<usize> {
        self.structures
            .iter()
            .position(|structure| structure.number == number)
    }

    /// The structure whose id is `gde_ref`.
    fn index_of_ref(&self, gde_ref: &[u8; 16]) -> Option<usize> {
        self.structures
            .iter()
            .position(|structure| &structure.gde_ref == gde_ref)
    }

    /// The raid that owns the built structure of role `role` whose id is `gde_ref` (the armory
    /// shop of `outpost_rush`).
    pub(super) fn built_raid(&self, gde_ref: &[u8; RAW_ID_LEN], role: StructureRole) -> Option<u64> {
        let structure = &self.structures[self.index_of_ref(gde_ref)?];
        (structure.role == role && structure.phase == Phase::Built).then_some(structure.raid)
    }

    /// The structures a player of the team of raid `raid` can hit: the whole built ones of the
    /// outposts the other team owns, by slot key and place.
    pub(super) fn targets_of(&self, raid: u64) -> Vec<(u64, Position)> {
        self.structures
            .iter()
            .filter(|structure| structure.open_to(raid))
            .map(|structure| (structure.number, structure.position()))
            .collect()
    }

    /// The structures a turret shot of a player of the team of raid `raid` meets
    /// ([`super::turrets`] module doc, **Shots on structures**): the whole built ones with
    /// vitals of the owned outposts that are no turret of slot keys `turrets`, by slot key,
    /// facet entity and place, with whether the other team owns their outpost.
    pub(super) fn shot_structures(&self, raid: u64, turrets: &[u64]) -> Vec<(u64, u64, Position, bool)> {
        self.structures
            .iter()
            .filter(|structure| {
                structure.phase == Phase::Built
                    && structure.vitals.is_some()
                    && structure.raid != NO_TEAM_RAID
                    && !turrets.contains(&structure.number)
            })
            .map(|structure| {
                (
                    structure.number,
                    facet_of_raw_id(&structure.gde_ref),
                    structure.position(),
                    structure.raid != raid,
                )
            })
            .collect()
    }
}

impl SimWorld {
    /// The structures of the Outpost Rush map in `folder`, spawned for the viewers of the match
    /// whose ids take `salt` (module doc); none for a map the table does not hold.
    pub(super) fn spawn_outpost_structures(&mut self, folder: &str, salt: u64) -> OutpostStructures {
        let content = std::sync::Arc::clone(&self.content);
        let (Some(map), Some(placement)) = (
            content.outpost_structures.map(folder),
            layout_placement(folder),
        ) else {
            return OutpostStructures::default();
        };
        let namespace = STRUCTURE_NAMESPACE ^ self.me.generation ^ salt;
        let mut set = OutpostStructures::default();
        for (index, spawner) in map.spawners.iter().enumerate() {
            let Some(slice) = content.outpost_structures.slice(&spawner.asset) else {
                tracing::warn!(spawner = %spawner.name, "outpost structure of no known slice");
                continue;
            };
            let [x, y, z] = spawner.place;
            let [east, north, up] = placement;
            let place = [x + east, y + north, z + up];
            let built = spawner.role == StructureRole::Armory;
            let gde_ref = dynamic_ref(namespace, index);
            let values = self.outpost_blueprint_values(slice.blueprint.as_deref());
            let Some(structure) =
                OutpostStructure::new(spawner, slice, place, gde_ref, built, values)
            else {
                tracing::warn!(spawner = %spawner.name, "outpost structure slice without members");
                continue;
            };
            set.structures.push(self.place_outpost_structure(structure));
        }
        set
    }

    /// The health, build time and cost of blueprint `blueprint` (none: 0 and no cost).
    fn outpost_blueprint_values(&self, blueprint: Option<&str>) -> (f32, f32, Vec<(String, u32)>) {
        let Some(id) = blueprint else {
            return (0.0, 0.0, Vec::new());
        };
        let row = self.content.outpost_blueprints.of(id);
        (
            row.map_or(0.0, |row| row.health),
            row.map_or(0.0, |row| row.build_time_s),
            self.content.structure_costs.of(id).to_vec(),
        )
    }

    /// Puts `structure` in the world: its slot key.
    fn place_outpost_structure(&mut self, mut structure: OutpostStructure) -> OutpostStructure {
        let members = structure.spawn_members();
        let (gde_ref, position) = (structure.gde_ref, structure.position());
        structure.number = self.stations.place_camp(|_| Station {
            gde_ref,
            position,
            station_type: String::new(),
            name: Vec::new(),
            members,
        });
        structure
    }

    /// Sends `changed` of `structure` to its viewers and keeps them in its spawn members.
    fn change_outpost_structure(&mut self, structure: &OutpostStructure, changed: Vec<BundleMember>) {
        if changed.is_empty() {
            return;
        }
        self.stations.change_camp(
            structure.number,
            |members| merge_members(members, &changed),
            changed.clone(),
        );
    }

    /// The structures of the match and the areas of its vats leave every view.
    pub(super) fn remove_outpost_structures(&mut self, set: &OutpostStructures) {
        for structure in &set.structures {
            self.stations.remove_camp(structure.number);
        }
        self.remove_siege_uses(&set.uses);
    }

    /// Each structure takes the raid of its outpost's owner (`raids` by outpost; 0: no owner).
    pub(super) fn own_outpost_structures(&mut self, set: &mut OutpostStructures, raids: [u64; OUTPOSTS]) {
        for index in 0..set.structures.len() {
            let raid = raids
                .get(usize::from(set.structures[index].outpost))
                .copied()
                .unwrap_or(NO_TEAM_RAID);
            if set.structures[index].raid == raid {
                continue;
            }
            set.structures[index].raid = raid;
            let structure = set.structures[index].clone();
            let used = set.turrets.iter().any(|run| run.body.number == structure.number && run.used())
                || set
                    .droppers
                    .iter()
                    .any(|run| run.body.number == structure.number && run.uses.used());
            if let Some(change) = structure.raid_change(used) {
                self.stations.change_camp(
                    structure.number,
                    |members| apply_ownership_change(members, &change),
                    vec![change.clone()],
                );
            }
            let side = self.outpost_team_of_raid(raid);
            if let Some(run) = set.turrets.iter_mut().find(|run| run.body.number == structure.number) {
                self.own_outpost_turret(run, side);
            }
            if let Some(run) = set.droppers.iter_mut().find(|run| run.body.number == structure.number) {
                self.own_outpost_dropper(run, side);
            }
        }
    }

    /// The ownership a use of a vat by player `user` of the team raid `raid` gives the vat, its
    /// barrel and the barrel's area ([`super::siege_areas`] module doc). Observed (`OPR_TEST3`
    /// 1714.326 and 1714.642 s): the outpost structures' team ownership (rule 4, the team's raid,
    /// no PvP or FFA flag, options 0) with the user's company (id, name and crest), the user as
    /// owner (kind 2) and the user's faction (1 there; the first state of a repeater at
    /// 1626.899 s named another player, another company and faction 3). The company's name and
    /// crest come from its claims or the world database ([`super::company_looks`]; the match
    /// asks for its participants' companies at each tick); a company whose look is not known
    /// yet is left out (Inferred).
    pub(super) fn outpost_use_ownership(&self, user: Entity, raid: u64) -> OwnershipState {
        let MemberState::Ownership(team) = team_ownership_on(0, raid).state else {
            return OwnershipState::default();
        };
        let Some(player) = self.world.get::<Player>(user) else {
            return team;
        };
        let ownership = OwnershipState {
            owner: self.owner_of(user),
            faction_type: Some(player.faction.faction),
            ..team
        };
        match self.company_look_of(user) {
            Some((id, look)) => OwnershipState {
                guild_id: Some(id),
                guild_owner: Some(look.name),
                guild_crest: Some(look.crest),
                ..ownership
            },
            None => ownership,
        }
    }

    /// The team whose raid is `raid` in the match, [`NO_TEAM`] for none.
    fn outpost_team_of_raid(&self, raid: u64) -> u8 {
        self.arena_matches
            .outpost_rush
            .as_ref()
            .and_then(|running| running.team_of_raid(raid))
            .unwrap_or(NO_TEAM)
    }

    /// The turret or vat dropper of `structure`, built now, in `set` (module doc): a new one at
    /// its first build, the broken one mended after a ruin. The members of `structure`'s built
    /// state with a turret's own turret, interact and spawner states in the place of the retail
    /// ones, or with a dropper's use interact; the retail members for a structure that is
    /// neither of a known kind.
    fn outpost_siege_built(&mut self, set: &mut OutpostStructures, structure: &OutpostStructure) -> Vec<BundleMember> {
        let extras = &structure.built_extras;
        let built_dropper = |run: &StructureRun| {
            let mut members = extras.clone();
            members.extend(run.body.interact_member(NO_INTERACTORS));
            members
        };
        if let Some(run) = set.droppers.iter_mut().find(|run| run.body.number == structure.number) {
            self.mend_outpost_dropper(run);
            return built_dropper(&*run);
        }
        let content = std::sync::Arc::clone(&self.content);
        let side = self.outpost_team_of_raid(structure.raid);
        let index = if let Some(index) = set.turrets.iter().position(|run| run.body.number == structure.number) {
            set.turrets[index].mend();
            index
        } else if let Some(kind) = content.war_forts.kind(&structure.asset) {
            let Some(run) = structure
                .siege_structure(side, None)
                .and_then(|turret| TurretRun::outpost_turret(turret, kind.clone(), structure.number))
            else {
                return extras.clone();
            };
            set.turrets.push(run);
            set.turrets.len() - 1
        } else {
            let dropper = content
                .war_forts
                .structure_kind(&structure.asset)
                .filter(|kind| kind.role == nw_content::war_forts::StructureRole::FireBarrelDropper)
                .and_then(|kind| structure.siege_structure(side, kind.use_interact))
                .map(|dropper| StructureRun::outpost_dropper(dropper, structure.number));
            let Some(run) = dropper else {
                return extras.clone();
            };
            let members = built_dropper(&run);
            set.droppers.push(run);
            return members;
        };
        let run = &set.turrets[index];
        extras
            .iter()
            .map(|member| run.own_member(member).unwrap_or_else(|| member.clone()))
            .collect()
    }

    /// Runs the structures at `now_ms`: the accepts that are due (`raid_of` gives a player's
    /// team raid) and the rising health of the builds. `true` when a status changed.
    pub(super) fn tick_outpost_structures(
        &mut self,
        set: &mut OutpostStructures,
        raid_of: impl Fn(u64) -> Option<u64>,
        now_ms: u64,
        elapsed_ms: u64,
    ) -> bool {
        let before = set.statuses();
        let (due, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut set.pending)
            .into_iter()
            .partition(|pending| now_ms >= pending.due_ms);
        set.pending = waiting;
        for pending in due {
            let raid = raid_of(pending.number).unwrap_or(NO_TEAM_RAID);
            let accepted = match pending.request.kind {
                APPLY_ALL_RESOURCES_KIND => self.apply_resources(set, &pending, raid),
                UPGRADE_BUILDABLE_KIND => self.upgrade_structure(set, &pending, raid),
                _ => false,
            };
            let status = if accepted { USE_ACCEPTED } else { USE_REFUSED };
            let answer = gather_answer(pending.number, pending.request, status, &pending.name);
            let ended = ServerMessage::InteractionEnded(InteractionEndedMsg {
                header: FacetHeader::of(component::INTERACTOR, pending.number),
            });
            self.send_travel(pending.entity, &[answer, ended]);
        }
        for index in 0..set.structures.len() {
            self.raise_structure(set, index, now_ms, elapsed_ms);
        }
        before != set.statuses()
    }

    /// A build or repair of `pending`'s player: its bag gives what the structure still needs.
    fn apply_resources(&mut self, set: &mut OutpostStructures, pending: &PendingRequest, raid: u64) -> bool {
        let Some(index) = set.index_of_ref(&pending.request.target) else {
            return false;
        };
        if !set.structures[index].takes_resources(raid) {
            return false;
        }
        let needed = set.structures[index].needed();
        let Some(mut player) = self.world.get_mut::<Player>(pending.entity) else {
            return false;
        };
        let Some(gear) = player.gear.as_mut() else {
            return false;
        };
        let mut taken_any = false;
        for (slot, item, left) in needed {
            let quantity = gear.bag_count(&item).min(left);
            if quantity > 0 && gear.take_as(&item, quantity, SYNC_CRAFT_USED) {
                set.structures[index].committed[slot] += quantity;
                set.deposits.push((pending.number, item.clone(), quantity));
                taken_any = true;
            }
        }
        if taken_any {
            let structure = &mut set.structures[index];
            if structure.phase == Phase::Ruin {
                structure.phase = Phase::Foundation;
                let structure = structure.clone();
                self.change_outpost_structure(&structure, structure.buildable_member().into_iter().collect());
            }
        }
        taken_any
    }

    /// An upgrade of `pending`'s player: the built tier is replaced by the next tier's
    /// foundation (module doc).
    fn upgrade_structure(&mut self, set: &mut OutpostStructures, pending: &PendingRequest, raid: u64) -> bool {
        let Some(index) = set.index_of_ref(&pending.request.target) else {
            return false;
        };
        let old = set.structures[index].clone();
        let whole = old.phase == Phase::Built && old.health >= old.max_health;
        if raid == NO_TEAM_RAID || old.raid != raid || !whole || !old.buildable() {
            return false;
        }
        let content = std::sync::Arc::clone(&self.content);
        let Some(next) = content.outpost_structures.next_tier(&old.asset) else {
            return false;
        };
        let spawner = StructureSpawner {
            name: String::new(),
            outpost: old.outpost,
            role: old.role,
            number: 1,
            asset: next.asset,
            place: old.place,
            yaw: 0.0,
        };
        let values = self.outpost_blueprint_values(next.blueprint.as_deref());
        let namespace = STRUCTURE_NAMESPACE ^ self.me.generation;
        let gde_ref = dynamic_ref(namespace ^ old.number, usize::from(next.tier));
        let Some(mut upgrade) = OutpostStructure::new(&spawner, next, old.place, gde_ref, false, values)
        else {
            return false;
        };
        upgrade.slot = old.slot;
        upgrade.yaw = old.yaw;
        upgrade.half_yaw = old.half_yaw;
        upgrade.shown_tier = old.shown_tier;
        upgrade.raid = old.raid;
        upgrade.replaces = Some(old.number);
        let upgrade = self.place_outpost_structure(upgrade);
        set.structures[index].phase = Phase::Replaced;
        let replaced = set.structures[index].clone();
        self.change_outpost_structure(&replaced, replaced.buildable_member().into_iter().collect());
        set.structures.push(upgrade);
        true
    }

    /// The health of structure `index` rises toward its committed share over `elapsed_ms`; at
    /// full health with its cost committed it is built.
    fn raise_structure(&mut self, set: &mut OutpostStructures, index: usize, now_ms: u64, elapsed_ms: u64) {
        let structure = &mut set.structures[index];
        let target = structure.target_health();
        if !matches!(structure.phase, Phase::Foundation | Phase::Built) || structure.health >= target {
            return;
        }
        let step = structure.rate_per_s * elapsed_ms as f32 / MS_PER_SECOND;
        structure.health = (structure.health + step).min(target);
        let reached = structure.health >= target;
        let built = reached && structure.needed().is_empty();
        if !reached && now_ms.saturating_sub(structure.last_send_ms) < BUILD_SEND_MS {
            return;
        }
        structure.last_send_ms = now_ms;
        let mut changed: Vec<BundleMember> = structure.health_change().into_iter().collect();
        let mut replaced = None;
        let finished = built && structure.phase == Phase::Foundation;
        if finished {
            structure.phase = Phase::Built;
            structure.shown_tier = structure.tier;
            structure.ruined = false;
            replaced = structure.replaces.take();
            changed.extend(structure.buildable_member());
        } else if built {
            structure.ruined = false;
        }
        let structure = structure.clone();
        if finished {
            changed.extend(self.outpost_siege_built(set, &structure));
        }
        self.change_outpost_structure(&structure, changed);
        if let Some(old) = replaced.and_then(|number| set.index_of(number)) {
            let old = set.structures.remove(old);
            self.stations.remove_camp(old.number);
        }
    }

    /// A hit of `amount` by a player of the team of raid `raid` on the structure of slot key
    /// `target` (module doc): `None` when `target` is no structure a hit of that team meets;
    /// `Some(true)` when the hit ruined it.
    pub(super) fn hurt_outpost_structure(
        &mut self,
        set: &mut OutpostStructures,
        raid: u64,
        target: u64,
        amount: u32,
    ) -> Option<bool> {
        let index = set.index_of(target)?;
        let structure = &mut set.structures[index];
        if !structure.open_to(raid) {
            return None;
        }
        structure.health = (structure.health - amount as f32).max(0.0);
        let share = if structure.max_health > 0.0 {
            structure.health / structure.max_health
        } else {
            0.0
        };
        for (done, (_, whole)) in structure.committed.iter_mut().zip(&structure.cost) {
            // At most the whole cost, a few hundred.
            *done = (*whole as f32 * share).floor() as u32;
        }
        let ruined = structure.health <= 0.0;
        let mut changed: Vec<BundleMember> = structure.health_change().into_iter().collect();
        if ruined {
            structure.phase = Phase::Ruin;
            structure.ruined = true;
            changed.extend(structure.buildable_member());
        }
        let structure = structure.clone();
        self.change_outpost_structure(&structure, changed);
        if ruined && let Some(run) = set.turrets.iter_mut().find(|run| run.body.number == target) {
            self.break_outpost_turret(run);
        }
        if ruined && let Some(run) = set.droppers.iter_mut().find(|run| run.body.number == target) {
            self.break_outpost_dropper(run);
        }
        Some(ruined)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use nw_content::outpost_structures::OutpostStructures as Table;

    /// A small table of the retail forms on the map in `folder`: the armory A (built, no
    /// vitals), the gate A1 (a t1 foundation with vitals and buildable members) and its t2.
    pub(in crate::world) fn table_on(folder: &str) -> Table {
        let vitals = BundleMember {
            component: 1,
            state: MemberState::Vitals(VitalsState {
                slot_0: Some(VitalsSlot0 {
                    health_amount: Some(GATE_HEALTH),
                    health_max: Some(GATE_HEALTH),
                    ..VitalsSlot0::default()
                }),
                slot_1: None,
            }),
        };
        let buildable = |state: u8| BundleMember {
            component: 3,
            state: MemberState::BuildableController(BuildableControllerState {
                current_state: Some(state),
                ..BuildableControllerState::default()
            }),
        };
        let encoded = |member: &BundleMember| hex::encode(member.encode().expect("encode"));
        let gate = format!(
            r#"{{"card_component": 0, "place_component": 5, "ownership_component": 9, "members": ["{}", "{}"]}}"#,
            encoded(&vitals),
            encoded(&buildable(FOUNDATION_STATE))
        );
        let armory = format!(
            r#"{{"card_component": 0, "place_component": 4, "ownership_component": 1, "members": ["{}"]}}"#,
            encoded(&buildable(BUILT_STATE))
        );
        let text = format!(
            r#"{{"kinds": [
            {{"asset": "{ARMORY}", "slice": "slices/outpostrush/structures/or_armory_t1.dynamicslice",
             "blueprint": "OR_Armory_T1", "tier": 1, "foundation": null, "built": {armory}}},
            {{"asset": "{T1}", "slice": "slices/outpostrush/structures/or_outpost_gate_t1.dynamicslice",
             "blueprint": "OR_Outpost_Gate_T1", "tier": 1, "foundation": {gate}, "built": null}},
            {{"asset": "{T2}", "slice": "slices/outpostrush/structures/or_outpost_gate_t2.dynamicslice",
             "blueprint": "OR_Outpost_Gate_T2", "tier": 2, "foundation": {gate}, "built": null}}],
            "maps": [{{"folder": "{folder}", "spawners": [
              {{"name": "ArmoryA", "outpost": 0, "role": "armory", "number": 1, "asset": "{ARMORY}",
                "place": [-78.536, 46.391, 107.125], "yaw": 0.0}},
              {{"name": "GateA1", "outpost": 0, "role": "gate", "number": 1, "asset": "{T1}",
                "place": [-52.465, 71.334, 107.125], "yaw": 0.0}}]}}]}}"#
        );
        Table::parse(&text).expect("table")
    }

    /// The table on Vertical.
    fn table() -> Table {
        table_on(VERTICAL)
    }

    /// The retail values of `OR_Outpost_Gate_T1` and `OR_Outpost_Gate_T2`: health, build time
    /// and cost.
    pub(in crate::world) const GATE_HEALTH: f32 = 45_000.0;
    pub(in crate::world) const GATE_BUILD_S: f32 = 4.0;
    pub(in crate::world) const GATE_T2_HEALTH: f32 = 90_000.0;
    pub(in crate::world) const GATE_T2_BUILD_S: f32 = 5.0;
    pub(in crate::world) const WOOD: &str = "OutpostRushWood";
    pub(in crate::world) const GATE_WOOD: u32 = 40;
    pub(in crate::world) const GATE_T2_ORE: u32 = 40;
    pub(in crate::world) const ORE: &str = "OutpostRushOre";
    /// The item classes of the Outpost Rush wood and ore (`MasterItemDefinitions_Crafting`).
    pub(in crate::world) const WOOD_CLASS: &str = "Resource+Wood+OutpostRushOnly";
    pub(in crate::world) const ORE_CLASS: &str = "Resource+Ore+OutpostRushOnly";
    const VERTICAL: &str = "nw_opr_005_vertical";
    /// The raids of the two test teams (retail `opr_que_and_lost` 213 and 214).
    const TEAM_RAID: u64 = 0x213;
    const OTHER_RAID: u64 = 0x214;
    const ARMORY: &str = "5920b66431e155ddb3a4db1b26e403c0";
    const T1: &str = "1de81d2cbd8d5460a4801272beac509d";
    const T2: &str = "ae9e66eb26df50a790dc6060173e3cfa";

    /// The blueprint rows of the test gate tiers.
    pub(in crate::world) fn gate_blueprints() -> nw_content::outpost_structures::OutpostBlueprints {
        use nw_content::outpost_structures::{OutpostBlueprint, OutpostBlueprints};
        OutpostBlueprints::build([
            (
                "OR_Outpost_Gate_T1".to_owned(),
                OutpostBlueprint {
                    health: GATE_HEALTH,
                    build_time_s: GATE_BUILD_S,
                },
            ),
            (
                "OR_Outpost_Gate_T2".to_owned(),
                OutpostBlueprint {
                    health: GATE_T2_HEALTH,
                    build_time_s: GATE_T2_BUILD_S,
                },
            ),
        ])
    }

    /// The costs of the test gate tiers.
    pub(in crate::world) fn gate_costs() -> nw_content::crafting::StructureCosts {
        nw_content::crafting::StructureCosts::build([
            (
                "OR_Outpost_Gate_T1".to_owned(),
                vec![(WOOD.to_owned(), GATE_WOOD)],
            ),
            (
                "OR_Outpost_Gate_T2".to_owned(),
                vec![(WOOD.to_owned(), GATE_WOOD), (ORE.to_owned(), GATE_T2_ORE)],
            ),
        ])
    }

    fn gate(table: &Table) -> OutpostStructure {
        let map = table.map(VERTICAL).expect("map");
        let spawner = &map.spawners[1];
        let slice = table.slice(&spawner.asset).expect("slice");
        OutpostStructure::new(
            spawner,
            slice,
            spawner.place,
            [1; RAW_ID_LEN],
            false,
            (GATE_HEALTH, GATE_BUILD_S, vec![(WOOD.to_owned(), GATE_WOOD)]),
        )
        .expect("gate")
    }

    #[test]
    fn a_foundation_shows_no_tier_and_a_ruin_keeps_its_tier_with_the_ruin_bit() {
        let table = table();
        let mut set = OutpostStructures::default();
        let mut structure = gate(&table);
        assert_eq!(structure.phase, Phase::Foundation);
        assert_eq!(structure.health, 0.0);
        set.structures.push(structure.clone());
        assert_eq!(set.statuses(), [0, 0, 0]);
        structure.phase = Phase::Built;
        structure.shown_tier = 3;
        set.structures[0] = structure.clone();
        // Gate1 is status slot 6: tier 3 (Observed `OPR_TEST3` 1043.716 s).
        assert_eq!(set.statuses()[0], 3 << 18);
        structure.phase = Phase::Ruin;
        set.structures[0] = structure;
        // Ruined at tier 3: 7 (Observed 1151.810 s).
        assert_eq!(set.statuses()[0], 7 << 18);
    }

    #[test]
    fn a_build_takes_what_is_missing_and_only_the_owner_team_builds() {
        let table = table();
        let mut structure = gate(&table);
        assert_eq!(structure.needed(), vec![(0, WOOD.to_owned(), GATE_WOOD)]);
        structure.committed[0] = GATE_WOOD / 2;
        assert_eq!(structure.target_health(), GATE_HEALTH / 2.0);
        assert_eq!(structure.needed()[0].2, GATE_WOOD / 2);
        structure.raid = TEAM_RAID;
        assert!(structure.takes_resources(TEAM_RAID));
        assert!(!structure.takes_resources(OTHER_RAID));
        assert!(!structure.takes_resources(NO_TEAM_RAID));
        structure.committed[0] = GATE_WOOD;
        assert!(!structure.takes_resources(TEAM_RAID));
    }

    #[test]
    fn the_health_rises_at_the_blueprint_rate() {
        let table = table();
        let structure = gate(&table);
        // 45,000 in 4 s: 9,000 a 0.8 s step (Observed `OPR_TEST3` 1013.7 to 1016.8 s).
        let step = structure.rate_per_s * BUILD_SEND_MS as f32 / MS_PER_SECOND;
        assert_eq!(step, 9_000.0);
    }

    #[test]
    fn the_spawn_members_hold_card_place_ownership_and_the_foundation() {
        let table = table();
        let mut structure = gate(&table);
        structure.raid = TEAM_RAID;
        let members = structure.spawn_members();
        let components: Vec<u64> = members.iter().map(|member| member.component).collect();
        assert!(components.contains(&0) && components.contains(&5) && components.contains(&9));
        let state = members.iter().find_map(|member| match &member.state {
            MemberState::BuildableController(state) => state.current_state,
            _ => None,
        });
        assert_eq!(state, Some(FOUNDATION_STATE));
        assert_eq!(
            members.iter().find(|member| member.component == 9),
            Some(&team_ownership_on(9, TEAM_RAID))
        );
        assert_eq!(table.next_tier(&structure.asset).map(|slice| slice.tier), Some(2));
    }

    /// An armory tier with a built template alone (the inferred `or_armory_t2`).
    fn armory_upgrade_table() -> Table {
        let buildable = BundleMember {
            component: 2,
            state: MemberState::BuildableController(BuildableControllerState {
                current_state: Some(BUILT_STATE),
                ..BuildableControllerState::default()
            }),
        };
        let built = format!(
            r#"{{"card_component": 0, "place_component": 4, "ownership_component": 1, "members": ["{}"]}}"#,
            hex::encode(buildable.encode().expect("encode"))
        );
        let text = format!(
            r#"{{"kinds": [
            {{"asset": "399d61467f4c58319ff86ebb79920a98", "slice": "slices/outpostrush/structures/or_armory_t2.dynamicslice",
             "blueprint": "OR_Armory_T2", "tier": 2, "foundation": null, "built": {built}, "template_inferred": true}}],
            "maps": []}}"#
        );
        Table::parse(&text).expect("table")
    }

    #[test]
    fn an_upgrade_tier_with_a_built_template_alone_starts_as_a_foundation() {
        let table = armory_upgrade_table();
        let asset: [u8; 16] = hex::decode("399d61467f4c58319ff86ebb79920a98")
            .expect("hex")
            .try_into()
            .expect("16 bytes");
        let slice = table.slice(&asset).expect("the tier");
        assert!(slice.template_inferred);
        let spawner = StructureSpawner {
            name: String::new(),
            outpost: 0,
            role: StructureRole::Armory,
            number: 1,
            asset: slice.asset,
            place: [0.0; 3],
            yaw: 0.0,
        };
        // `OR_Armory_T2`: 30,000 health, 4 s, 200 wood and 100 rawhide.
        let values = (30_000.0, 4.0, vec![(WOOD.to_owned(), 200), ("OutpostRushRawhide".to_owned(), 100)]);
        let upgrade = OutpostStructure::new(&spawner, slice, [0.0; 3], [2; RAW_ID_LEN], false, values.clone())
            .expect("the inferred tier");
        assert_eq!(upgrade.phase, Phase::Foundation, "an upgrade builds");
        assert_eq!((upgrade.health, upgrade.shown_tier), (0.0, 0));
        assert_eq!(upgrade.needed().len(), 2, "its whole cost");
        let MemberState::BuildableController(state) =
            upgrade.buildable_member().expect("a buildable state").state
        else {
            panic!("a buildable state");
        };
        assert_eq!(state.current_state, Some(FOUNDATION_STATE));
        let spawned = OutpostStructure::new(&spawner, slice, [0.0; 3], [3; RAW_ID_LEN], true, values)
            .expect("the tier built");
        assert_eq!(spawned.phase, Phase::Built);
    }

    /// The Outpost Rush turret tests ([`super::super::turrets`] module doc).
    mod turrets {
        use std::sync::Arc;

        use bevy_ecs::entity::Entity;
        use nw_content::outpost_structures::{StructureRole, StructureSpawner};
        use nw_content::war_forts::WarForts;
        use nw_messages::combat_effects::CombatEffectMsg;
        use nw_messages::turrets::{
            ProjectileSpawnerState, RepeaterFlags, TurretAim, TurretClientMsg, gde_ref_of,
        };
        use nw_messages::{FacetHeader, InteractState, MemberState, ServerMessage, TurretState};

        use super::super::*;
        use super::Table;
        use crate::player_content::PlayerContent;
        use crate::world::outpost_rush::tests::{FIRST, SECOND, outpost_rush, own_outpost, rush_sim_on};
        use crate::world::siege_areas::{
            DROPPER_COOLDOWN_MS, DROPPER_USE_MS, FIRST_COOLDOWN_VERSION, USE_OPTION,
        };
        use crate::world::tests::{NOW, messages_to};
        use crate::world::turrets::tests::use_request;

        /// The retail slice assets of the Outpost Rush repeater and ballista.
        const REPEATER: &str = "abc32326a6d15e588d229ae7d8232664";
        const BALLISTA: &str = "b75b3108c6d15e7eab232d70043fc103";
        /// The retail components of their built states (Observed, `opr_que_and_lost`): vitals,
        /// buildable, turret, use interact, build interact and projectile spawner.
        const VITALS: u64 = 1;
        const BUILDABLE: u64 = 5;
        const REPEATER_PARTS: [u64; 4] = [10, 11, 15, 14];
        const BALLISTA_PARTS: [u64; 4] = [14, 12, 16, 11];
        /// The blueprint health and build time of the turrets (`OR_Turret*`: 30,000, 3 s).
        const TURRET_HEALTH: f32 = 30_000.0;
        const TURRET_BUILD_MS: u64 = 3_000;
        /// A place far north of the players, the turrets facing away from them.
        const REPEATER_PLACE: [f32; 3] = [1_300.0, 1_250.0, 50.0];
        const BALLISTA_PLACE: [f32; 3] = [1_320.0, 1_250.0, 50.0];
        /// The ammo of a spawner that uses none (Observed `ffff`).
        const UNCOUNTED: u16 = u16::MAX;
        /// The no-interactor form (Observed: 20 zero bytes, then `ffffffff`).
        const NO_INTERACTOR_TAIL: usize = 4;
        /// The client clock of the first shot, ns.
        const SHOT_NS: u64 = 0x0b97_927a_bd0a_dcad;

        fn asset(hex_text: &str) -> [u8; 16] {
            hex::decode(hex_text).expect("hex").try_into().expect("16 bytes")
        }

        /// A built template of a turret with `parts` (turret, use interact, build interact,
        /// spawner), its retail states with a player in the turret and one interactor (as the
        /// first group of a turret in use came, `opr_que_and_lost` 1757.994 s).
        fn template(parts: [u64; 4], built: bool) -> String {
            let [turret, use_interact, build_interact, spawner] = parts;
            let mut members = vec![
                BundleMember {
                    component: VITALS,
                    state: MemberState::Vitals(VitalsState {
                        slot_0: Some(VitalsSlot0 {
                            health_amount: Some(TURRET_HEALTH),
                            health_max: Some(TURRET_HEALTH),
                            ..VitalsSlot0::default()
                        }),
                        slot_1: None,
                    }),
                },
                BundleMember {
                    component: BUILDABLE,
                    state: MemberState::BuildableController(BuildableControllerState {
                        current_state: Some(if built { BUILT_STATE } else { FOUNDATION_STATE }),
                        ..BuildableControllerState::default()
                    }),
                },
            ];
            if built {
                members.extend([
                    BundleMember {
                        component: turret,
                        state: MemberState::Turret(TurretState {
                            yaw: Some(0),
                            pitch: Some(0),
                            heat: Some(0),
                            orientation: None,
                            interactor: Some([7; nw_messages::war_states::GDE_REF_LEN]),
                            overheated: Some(false),
                            blocked: Some(false),
                        }),
                    },
                    BundleMember {
                        component: use_interact,
                        state: MemberState::Interact(InteractState {
                            enabled: None,
                            has_interactors: Some(1),
                        }),
                    },
                    BundleMember {
                        component: build_interact,
                        state: MemberState::Interact(InteractState {
                            enabled: Some(false),
                            has_interactors: Some(0),
                        }),
                    },
                    BundleMember {
                        component: spawner,
                        state: MemberState::Recorded {
                            class: nw_messages::turrets::PROJECTILE_SPAWNER_TYPE_INDEX,
                            body: ProjectileSpawnerState {
                                ammo: Some(UNCOUNTED),
                                firing_blocked: Some(false),
                            }
                            .encode(),
                        },
                    },
                ]);
            }
            let encoded: Vec<String> = members
                .iter()
                .map(|member| format!("\"{}\"", hex::encode(member.encode().expect("encode"))))
                .collect();
            format!(
                r#"{{"card_component": 0, "place_component": 7, "ownership_component": 4, "members": [{}]}}"#,
                encoded.join(", ")
            )
        }

        /// The table of the two turret slices.
        fn turret_table() -> Table {
            let text = format!(
                r#"{{"kinds": [
                {{"asset": "{REPEATER}", "slice": "slices/outpostrush/structures/or_turretrepeater.dynamicslice",
                 "blueprint": null, "tier": 1, "foundation": {}, "built": {}}},
                {{"asset": "{BALLISTA}", "slice": "slices/outpostrush/structures/or_turretballista.dynamicslice",
                 "blueprint": null, "tier": 1, "foundation": {}, "built": {}}}],
                "maps": []}}"#,
                template(REPEATER_PARTS, false),
                template(REPEATER_PARTS, true),
                template(BALLISTA_PARTS, false),
                template(BALLISTA_PARTS, true),
            );
            Table::parse(&text).expect("table")
        }

        /// The kinds of the two turrets: the repeater and ballista values of their Outpost Rush
        /// slices (`m_usesAmmo` off; the ballista's `m_fireCooldown` 2.5 s and projectile).
        const KINDS: &str = r#"{"forts": [], "turret_kinds": [
           {"asset": "abc32326a6d15e588d229ae7d8232664", "damage_row": "RepeaterTurret_Projectile",
            "damage_table": "DamageTable_Structures", "fire_cooldown_s": 0.0, "fire_speed_mps": 1000.0,
            "gravity_mps2": 13.0, "heat_capacity": 100.0, "heat_gain_per_s": 15.384, "heat_loss_delay_s": 0.4,
            "heat_loss_per_s": 40.0, "hold_to_fire": true, "max_ammo": 200, "ammo_refill": 200,
            "repair_health": 5000.0, "pitch_max_deg": 30.0, "pitch_min_deg": -40.0, "projectile": null,
            "radius_m": 0.2, "range_m": 125.0, "repeat_fire_delay_s": 0.2,
            "slice": "slices/outpostrush/structures/or_turretrepeater.dynamicslice",
            "spawner_facet": {"component": "372fc35a6778d259", "xor": "06f03eba159de653"},
            "turret_facet": {"component": "36b800a37ce6d7c1", "xor": "06f03ea2159de653"},
            "uses_ammo": false, "yaw_max_deg": 1800.0},
           {"asset": "b75b3108c6d15e7eab232d70043fc103", "damage_row": "Ballista_Projectile",
            "damage_table": "DamageTable_Structures", "fire_cooldown_s": 2.5, "fire_speed_mps": 60.0,
            "gravity_mps2": 13.0, "heat_capacity": 0.0, "heat_gain_per_s": 0.0, "heat_loss_delay_s": 0.0,
            "heat_loss_per_s": 0.0, "hold_to_fire": false, "max_ammo": 15, "ammo_refill": 15,
            "repair_health": 5000.0, "pitch_max_deg": 30.0, "pitch_min_deg": -40.0,
            "projectile": {"asset": "30a2e9cadf6b5622a0571e468690f128", "component": 1, "ranged_attack": 261,
                           "damage_table_crc": 81017683, "row_index": 6},
            "radius_m": 0.3, "range_m": 115.0, "repeat_fire_delay_s": 0.0,
            "slice": "slices/outpostrush/structures/or_turretballista.dynamicslice",
            "spawner_facet": {"component": "603842316ab22336", "xor": "9aaa09ee99dfb635"},
            "turret_facet": {"component": "26eb5da726214d6d", "xor": "9aaa09e499dfb635"},
            "uses_ammo": false, "yaw_max_deg": 75.0}]}"#;

        /// A test match: the world, its two players and its two turrets' (slot key, id).
        type TurretWorld = (SimWorld, [Entity; 2], [(u64, [u8; 16]); 2]);

        /// The Outpost Rush match of two players (one a team) with the turret kinds, and its two
        /// turret foundations of outpost A owned by the first player's team: the world, the
        /// players and the turrets' (slot key, id).
        fn turret_world() -> TurretWorld {
            let mut world = rush_sim_on(outpost_rush());
            world.content = Arc::new(PlayerContent {
                war_forts: WarForts::parse(KINDS).expect("kinds"),
                ..(*world.content).clone()
            });
            let team = running(&world).team_of(FIRST).expect("a team");
            own_outpost(&mut world, 0, team);
            let raid = running(&world).raid_seat_of(FIRST).expect("a seat").0;
            let table = turret_table();
            let mut made = Vec::new();
            for (index, (kind, place)) in [(REPEATER, REPEATER_PLACE), (BALLISTA, BALLISTA_PLACE)]
                .into_iter()
                .enumerate()
            {
                let spawner = StructureSpawner {
                    name: format!("DefensiveStructureA{}", index + 1),
                    outpost: 0,
                    role: StructureRole::DefensiveStructure,
                    number: u8::try_from(index + 1).expect("a number"),
                    asset: asset(kind),
                    place,
                    yaw: 0.0,
                };
                let slice = table.slice(&spawner.asset).expect("slice");
                let gde_ref = [0x40 + u8::try_from(index).expect("an index"); RAW_ID_LEN];
                let mut structure = OutpostStructure::new(
                    &spawner,
                    slice,
                    place,
                    gde_ref,
                    false,
                    (TURRET_HEALTH, TURRET_BUILD_MS as f32 / MS_PER_SECOND, Vec::new()),
                )
                .expect("a turret structure");
                structure.raid = raid;
                let structure = world.place_outpost_structure(structure);
                made.push((structure.number, gde_ref));
                running_mut(&mut world).structures.structures.push(structure);
            }
            let first = world.player_by_number(FIRST).expect("first");
            let second = world.player_by_number(SECOND).expect("second");
            (world, [first, second], [made[0], made[1]])
        }

        fn running(world: &SimWorld) -> &crate::world::outpost_rush::OutpostRushMatch {
            crate::world::outpost_rush::tests::running(world)
        }

        fn running_mut(world: &mut SimWorld) -> &mut crate::world::outpost_rush::OutpostRushMatch {
            world.arena_matches.outpost_rush.as_mut().expect("the match")
        }

        /// Runs `change` on the match's structures, taken out as the match's tick does.
        fn with_set<T>(world: &mut SimWorld, change: impl FnOnce(&mut SimWorld, &mut OutpostStructures) -> T) -> T {
            let mut set = std::mem::take(&mut running_mut(world).structures);
            let result = change(world, &mut set);
            running_mut(world).structures = set;
            result
        }

        /// The structures rise for their whole build time at `now_ms`.
        fn build_all(world: &mut SimWorld, now_ms: u64) {
            with_set(world, |world, set| {
                world.tick_outpost_structures(set, |_| None, now_ms, TURRET_BUILD_MS);
            });
        }

        fn turret_of(world: &SimWorld, number: u64) -> Option<&TurretRun> {
            running(world)
                .structures
                .turrets
                .iter()
                .find(|run| run.body.number == number)
        }

        fn member_of(world: &SimWorld, number: u64, component: u64) -> MemberState {
            world
                .stations
                .members(number)
                .and_then(|members| members.iter().find(|member| member.component == component))
                .map(|member| member.state.clone())
                .expect("the member")
        }

        fn connection_of(world: &SimWorld, entity: Entity) -> nw_bus::ConnectionRef {
            world
                .world
                .get::<Player>(entity)
                .expect("player")
                .connection
                .clone()
        }

        fn answered(outgoing: &[crate::world::Outgoing], number: u64, status: u8) -> bool {
            messages_to(outgoing, number).iter().any(|message| {
                matches!(message, ServerMessage::InteractionValidated(answer) if answer.status == status)
            })
        }

        fn aim_at(gde_ref: [u8; 16]) -> TurretAim {
            let entity = u64::from_le_bytes(gde_ref[..8].try_into().expect("8 bytes"));
            TurretAim {
                turret: gde_ref_of(gde_ref, entity),
                turret_facet: 0,
                time_ns: SHOT_NS,
                yaw: 0.0,
                pitch: 0.0,
            }
        }

        #[test]
        fn a_built_turret_becomes_a_turret_of_its_owner_team_with_its_own_idle_members() {
            let (mut world, _, [(repeater, _), _]) = turret_world();
            assert!(turret_of(&world, repeater).is_none(), "a foundation is no turret");
            build_all(&mut world, NOW);
            let run = turret_of(&world, repeater).expect("the built repeater is a turret");
            assert_eq!(run.body.structure.side, 0, "the first player's team owns outpost A");
            assert!(!run.in_use() && !run.body.broken && !run.body.invulnerable);
            let [turret, use_interact, build_interact, spawner] = REPEATER_PARTS;
            let MemberState::Turret(state) = member_of(&world, repeater, turret) else {
                panic!("a turret state");
            };
            let interactor = state.interactor.expect("an interactor child");
            assert!(
                interactor[..interactor.len() - NO_INTERACTOR_TAIL].iter().all(|byte| *byte == 0)
                    && interactor[interactor.len() - NO_INTERACTOR_TAIL..].iter().all(|byte| *byte == u8::MAX),
                "no interactor, not the retail player of the template"
            );
            assert_eq!(
                member_of(&world, repeater, use_interact),
                MemberState::Interact(InteractState {
                    enabled: Some(true),
                    has_interactors: Some(0),
                })
            );
            assert_eq!(
                member_of(&world, repeater, build_interact),
                MemberState::Interact(InteractState {
                    enabled: Some(false),
                    has_interactors: Some(0),
                }),
                "the build's interact stays as retail sent it"
            );
            let MemberState::Recorded { body, .. } = member_of(&world, repeater, spawner) else {
                panic!("a spawner state");
            };
            assert_eq!(
                body,
                ProjectileSpawnerState {
                    ammo: Some(UNCOUNTED),
                    firing_blocked: Some(false),
                }
                .encode(),
                "a spawner that uses no ammo shows ffff"
            );
        }

        #[test]
        fn the_owner_team_uses_and_fires_an_outpost_turret_and_the_other_team_is_refused() {
            let (mut world, [first, second], [(repeater, repeater_ref), _]) = turret_world();
            build_all(&mut world, NOW);
            world.take_messages();
            assert!(world.turret_interaction(second, SECOND, use_request(repeater_ref), &[], NOW));
            assert!(answered(&world.take_messages(), SECOND, USE_REFUSED), "the other team");
            assert!(world.turret_interaction(first, FIRST, use_request(repeater_ref), &[], NOW));
            assert!(answered(&world.take_messages(), FIRST, USE_ACCEPTED), "the owner team");
            assert!(turret_of(&world, repeater).is_some_and(TurretRun::in_use));
            let call = TurretClientMsg::RepeaterInput {
                header: FacetHeader::of(0, FIRST),
                aim: aim_at(repeater_ref),
                flags: RepeaterFlags(RepeaterFlags::HELD | RepeaterFlags::FIRED),
                frame_s: 1.0 / 30.0,
            };
            let connection = connection_of(&world, first);
            world.turret_call(&connection, call, NOW + 10);
            let outgoing = world.take_messages();
            for number in [FIRST, SECOND] {
                assert!(
                    messages_to(&outgoing, number).iter().any(|message| matches!(
                        message,
                        ServerMessage::CombatEffect(CombatEffectMsg::TurretHitscanResult { .. })
                    )),
                    "every player gets the shot's 2720"
                );
            }
            let [_, _, _, spawner] = REPEATER_PARTS;
            let MemberState::Recorded { body, .. } = member_of(&world, repeater, spawner) else {
                panic!("a spawner state");
            };
            assert_eq!(
                body,
                ProjectileSpawnerState {
                    ammo: Some(UNCOUNTED),
                    firing_blocked: Some(false),
                }
                .encode(),
                "no ammo is spent"
            );
        }

        #[test]
        fn a_ballista_shot_tells_the_other_players_with_the_spawner_2900() {
            let (mut world, [first, _], [_, (_, ballista_ref)]) = turret_world();
            build_all(&mut world, NOW);
            assert!(world.turret_interaction(first, FIRST, use_request(ballista_ref), &[], NOW));
            world.take_messages();
            let call = TurretClientMsg::FireProjectile {
                header: FacetHeader::of(0, FIRST),
                aim: aim_at(ballista_ref),
                tail: nw_messages::turrets::FIRE_TAIL,
            };
            let connection = connection_of(&world, first);
            world.turret_call(&connection, call, NOW + 10);
            let outgoing = world.take_messages();
            let spawned = |number: u64| {
                messages_to(&outgoing, number).iter().any(|message| matches!(
                    message,
                    ServerMessage::CombatEffect(CombatEffectMsg::ProjectileServerSpawn { .. })
                ))
            };
            let told = messages_to(&outgoing, FIRST).iter().any(|message| matches!(
                message,
                ServerMessage::CombatEffect(CombatEffectMsg::AuthoritativeProjectile { .. })
            ));
            assert!(told && !spawned(FIRST), "the shooter gets its 327 alone");
            assert!(spawned(SECOND), "the other player gets the 2900");
        }

        #[test]
        fn a_ruin_breaks_the_turret_and_lets_its_user_go_and_a_rebuild_mends_it() {
            let (mut world, [first, _], [(repeater, repeater_ref), _]) = turret_world();
            build_all(&mut world, NOW);
            assert!(world.turret_interaction(first, FIRST, use_request(repeater_ref), &[], NOW));
            let foe = running(&world).raid_seat_of(SECOND).expect("a seat").0;
            let ruined = with_set(&mut world, |world, set| {
                world.hurt_outpost_structure(set, foe, repeater, TURRET_HEALTH as u32)
            });
            assert_eq!(ruined, Some(true));
            let run = turret_of(&world, repeater).expect("the turret");
            assert!(run.body.broken && !run.in_use(), "broken, its user let go");
            let [_, use_interact, _, _] = REPEATER_PARTS;
            assert_eq!(
                member_of(&world, repeater, use_interact),
                MemberState::Interact(InteractState {
                    enabled: Some(false),
                    has_interactors: Some(0),
                }),
                "Observed at the ruin: the use disabled, no interactor"
            );
            with_set(&mut world, |_, set| {
                let structure = set
                    .structures
                    .iter_mut()
                    .find(|structure| structure.number == repeater)
                    .expect("the structure");
                structure.phase = Phase::Foundation;
            });
            build_all(&mut world, NOW + TURRET_BUILD_MS);
            let run = turret_of(&world, repeater).expect("the turret");
            assert!(!run.body.broken, "mended at the rebuild");
            assert_eq!(
                running(&world).structures.turrets.len(),
                2,
                "one turret a structure"
            );
            world.take_messages();
            assert!(world.turret_interaction(first, FIRST, use_request(repeater_ref), &[], NOW));
            assert!(answered(&world.take_messages(), FIRST, USE_ACCEPTED), "usable again");
        }

        #[test]
        fn a_new_owner_team_takes_the_turret_and_lets_the_other_teams_user_go() {
            let (mut world, [first, second], [(repeater, repeater_ref), _]) = turret_world();
            build_all(&mut world, NOW);
            assert!(world.turret_interaction(first, FIRST, use_request(repeater_ref), &[], NOW));
            let foe = running(&world).raid_seat_of(SECOND).expect("a seat").0;
            with_set(&mut world, |world, set| {
                world.own_outpost_structures(set, [foe, NO_TEAM_RAID, NO_TEAM_RAID]);
            });
            let run = turret_of(&world, repeater).expect("the turret");
            assert_eq!(run.body.structure.side, 1, "the second player's team");
            assert!(!run.in_use(), "the first team's user is let go");
            world.take_messages();
            assert!(world.turret_interaction(first, FIRST, use_request(repeater_ref), &[], NOW));
            assert!(answered(&world.take_messages(), FIRST, USE_REFUSED));
            assert!(world.turret_interaction(second, SECOND, use_request(repeater_ref), &[], NOW));
            assert!(answered(&world.take_messages(), SECOND, USE_ACCEPTED));
            assert_eq!(world.turret_side(second), Some(1));
        }

        /// The ownership component of the turret templates ([`template`]).
        const TURRET_OWNERSHIP: u64 = 4;
        /// The ownership children a use of an Outpost Rush structure sends (Observed, `OPR_TEST3`
        /// 1718.191 s: children 0 to 3 and 7).
        const USE_CHILDREN: [bool; 11] = [true, true, true, true, false, false, false, true, false, false, false];

        /// The last ownership on `component` of the state change of slot key `number` that goes
        /// out to the players who see it already.
        fn sent_ownership(world: &SimWorld, number: u64, component: u64) -> Option<OwnershipState> {
            let slot = crate::world::slots::Slot {
                interest: 1,
                new: false,
            };
            world.stations.group(number, slot)?.members.into_iter().rev().find_map(|member| {
                match member.state {
                    MemberState::Ownership(state) if member.component == component => Some(state),
                    _ => None,
                }
            })
        }

        /// Which children of `state` are sent.
        fn children(state: &OwnershipState) -> [bool; 11] {
            [
                state.guild_id.is_some(),
                state.guild_owner.is_some(),
                state.guild_crest.is_some(),
                state.owner.is_some(),
                state.permission_rule.is_some(),
                state.structure_name.is_some(),
                state.raid_id.is_some(),
                state.faction_type.is_some(),
                state.owner_pvp.is_some(),
                state.owner_ffa.is_some(),
                state.permission_options.is_some(),
            ]
        }

        /// Observed (`OPR_TEST3`, the repeater of slot 79): the use (1718.191 s) sends the
        /// user's company with its name and crest, the user and the user's faction; the release
        /// (1725.923 s) sends no ownership; the new owner (1733.638 s) sends the raid alone to
        /// the used turret, the zero company and the raid to a structure no use named.
        #[test]
        fn a_turret_use_names_the_users_company_and_a_new_owner_sends_the_raid_alone() {
            use crate::world::company_looks::tests::{join_company, listing};
            const COMPANY: uuid::Uuid = uuid::Uuid::from_u128(0xc0_ffee);
            const COMPANY_NAME: &str = "Test Company";
            const USER_FACTION: u8 = 1;
            let (mut world, [first, second], [(repeater, repeater_ref), (ballista, _)]) = turret_world();
            if let Some(mut player) = world.world.get_mut::<Player>(first) {
                player.faction.faction = USER_FACTION;
            }
            join_company(&mut world, first, COMPANY);
            world.tick_outpost_rush();
            world.companies_read(&[listing(COMPANY, COMPANY_NAME)]);
            build_all(&mut world, NOW);
            assert!(world.turret_interaction(first, FIRST, use_request(repeater_ref), &[], NOW));
            let sent = sent_ownership(&world, repeater, TURRET_OWNERSHIP).expect("the use's ownership");
            assert_eq!(children(&sent), USE_CHILDREN, "the retail children of a use");
            let character = world.world.get::<Player>(first).expect("player").character;
            assert_eq!(
                sent.owner.map(|owner| owner.first),
                Some(character.hyphenated().to_string().into_bytes()),
                "the user"
            );
            assert_eq!(
                (sent.guild_id, sent.guild_owner, sent.faction_type),
                (Some(*COMPANY.as_bytes()), Some(COMPANY_NAME.as_bytes().to_vec()), Some(USER_FACTION)),
                "the user's company and faction"
            );
            let raid = running(&world).raid_seat_of(FIRST).expect("a seat").0;
            let owned = ownership_of(&world, repeater, TURRET_OWNERSHIP);
            assert_eq!(
                (owned.guild_id, owned.raid_id, owned.permission_rule),
                (Some(*COMPANY.as_bytes()), Some(raid), Some(4)),
                "the card keeps the use on the team's ownership"
            );

            world.tick_turrets(NOW + crate::world::turrets::USE_TIMEOUT_MS + 1);
            assert!(turret_of(&world, repeater).is_some_and(|run| !run.in_use() && run.used()));
            assert_eq!(ownership_of(&world, repeater, TURRET_OWNERSHIP), owned, "the release keeps it");

            let foe = running(&world).raid_seat_of(SECOND).expect("a seat").0;
            with_set(&mut world, |world, set| {
                world.own_outpost_structures(set, [foe, NO_TEAM_RAID, NO_TEAM_RAID]);
            });
            let used = sent_ownership(&world, repeater, TURRET_OWNERSHIP).expect("the raid change");
            assert_eq!(
                used,
                OwnershipState {
                    raid_id: Some(foe),
                    ..OwnershipState::default()
                },
                "the raid alone to the used turret"
            );
            assert_eq!(
                ownership_of(&world, repeater, TURRET_OWNERSHIP),
                OwnershipState {
                    raid_id: Some(foe),
                    ..owned
                },
                "its card keeps the last user's company"
            );
            let unused = sent_ownership(&world, ballista, TURRET_OWNERSHIP).expect("the raid change");
            assert_eq!(
                unused,
                OwnershipState {
                    guild_id: Some([0; GUILD_ID_LEN]),
                    raid_id: Some(foe),
                    ..OwnershipState::default()
                },
                "the zero company and the raid to the unused ballista"
            );
            assert_eq!(world.turret_side(second), Some(1));
        }

        /// The hit of the siege test: a few thousand points, as a repeater's hit.
        const SIEGE_HIT: u32 = 2_000;
        /// Its score in the board's test table: 2,000 x `DamageToOther`'s 0.0025.
        const SIEGE_HIT_SCORE: u32 = 5;

        /// The running match of [`turret_world`] with its turrets built, the ballista of
        /// the second player's team: the world, the first player and the turrets.
        fn foe_ballista_world() -> (SimWorld, Entity, [(u64, [u8; 16]); 2]) {
            use crate::world::arena_match::PREGAME_MS;
            use crate::world::outpost_rush::tests::{START_MS, tick_at, with_board_rules};
            let (mut world, [first, _], turrets) = turret_world();
            with_board_rules(&mut world);
            tick_at(&mut world, START_MS + PREGAME_MS);
            build_all(&mut world, NOW);
            let [_, (ballista, _)] = turrets;
            let foe_raid = running(&world).raid_seat_of(SECOND).expect("a seat").0;
            let foe_team = running(&world).team_of(SECOND).expect("a team");
            let set = &mut running_mut(&mut world).structures;
            if let Some(structure) = set.structures.iter_mut().find(|structure| structure.number == ballista) {
                structure.raid = foe_raid;
            }
            if let Some(run) = set.turrets.iter_mut().find(|run| run.body.number == ballista) {
                run.body.structure.side = foe_team;
            }
            (world, first, turrets)
        }

        /// How far the repeater's muzzle (3.3 m over its place) stands over the middle of a turret
        /// body (3 m high) on the same ground: a level shot passes over the body.
        const MUZZLE_OVER_BODY_MIDDLE_M: f32 = 1.8;

        #[test]
        fn a_repeater_shot_meets_the_foe_turret_and_names_it_with_no_hit_volume() {
            let (mut world, first, [(_, repeater_ref), (_, ballista_ref)]) = foe_ballista_world();
            assert!(world.turret_interaction(first, FIRST, use_request(repeater_ref), &[], NOW));
            world.take_messages();
            // The ballista stands 20 m east of the repeater: a yaw of a quarter turn right, and a
            // look down (a negative pitch) at the middle of its body.
            let east_m = BALLISTA_PLACE[0] - REPEATER_PLACE[0];
            let call = TurretClientMsg::RepeaterInput {
                header: FacetHeader::of(0, FIRST),
                aim: TurretAim {
                    yaw: -std::f32::consts::FRAC_PI_2,
                    pitch: -(MUZZLE_OVER_BODY_MIDDLE_M / east_m).atan(),
                    ..aim_at(repeater_ref)
                },
                flags: RepeaterFlags(RepeaterFlags::HELD | RepeaterFlags::FIRED),
                frame_s: 1.0 / 30.0,
            };
            let connection = connection_of(&world, first);
            world.turret_call(&connection, call, NOW + 10);
            let outgoing = world.take_messages();
            let ballista_facet = u64::from_le_bytes(ballista_ref[..8].try_into().expect("8 bytes"));
            assert!(
                messages_to(&outgoing, FIRST).iter().any(|message| matches!(
                    message,
                    ServerMessage::CombatEffect(CombatEffectMsg::TurretHitscanResult { target, result, .. })
                        if *target == ballista_facet && *result == crate::world::turrets::NO_HIT_VOLUME
                )),
                "the 2720 names the foe turret and no hit volume"
            );
        }

        #[test]
        fn a_turret_hit_on_a_foe_turret_hurts_it_and_counts_as_siege_damage_once() {
            use nw_content::warboard_stats::{
                DAMAGE_TO_OTHER, DAMAGE_WITH_SIEGE_WEAPONS, SCORE, TOTAL_DAMAGE_DEALT,
            };
            use crate::world::outpost_rush::tests::board_value;
            let (mut world, _, [_, (ballista, _)]) = foe_ballista_world();
            let index = running(&world)
                .structures
                .turrets
                .iter()
                .position(|run| run.body.number == ballista)
                .expect("the ballista's turret");
            let score_before = board_value(&world, FIRST, SCORE);
            world.hurt_held_turret(index, SIEGE_HIT, FIRST);
            let health = running(&world)
                .structures
                .structures
                .iter()
                .find(|structure| structure.number == ballista)
                .map(|structure| structure.health)
                .expect("the ballista");
            assert!((health - (TURRET_HEALTH - SIEGE_HIT as f32)).abs() < 1e-3, "the structure's health");
            assert_eq!(board_value(&world, FIRST, DAMAGE_TO_OTHER), SIEGE_HIT);
            assert_eq!(board_value(&world, FIRST, DAMAGE_WITH_SIEGE_WEAPONS), SIEGE_HIT);
            assert_eq!(board_value(&world, FIRST, TOTAL_DAMAGE_DEALT), SIEGE_HIT, "once in the total");
            assert_eq!(
                board_value(&world, FIRST, SCORE) - score_before,
                SIEGE_HIT_SCORE,
                "once in the score"
            );
        }

        /// The retail Outpost Rush vat `or_turretfire` (asset `1326dd54..`; Observed, `OPR_TEST3`
        /// slot 123): the components of its built state (vitals 1, buildable 5, projectile
        /// spawner 15 with ammo `ffff`, build interact 19 disabled; built at 1706.442 s) and of
        /// its use (use interact 16, pour timer 18, ownership 4; 1714.326 s); its place and
        /// facing (1375.908, 10886.467, the height of z raw 5773; turned 0.436 rad) and the
        /// floor its barrel's area opened on (z raw 5630, 1715.608 s).
        const VAT: &str = "1326dd54e1345e24a15063bf88cfcff4";
        const VAT_SPAWNER: u64 = 15;
        const VAT_BUILD_INTERACT: u64 = 19;
        const VAT_USE_INTERACT: u64 = 16;
        const VAT_POUR: u64 = 18;
        const VAT_OWNERSHIP: u64 = 4;
        const VAT_PLACE: [f32; 3] = [1_375.908, 10_886.467, 84.988];
        const VAT_YAW: f32 = 0.436_332_36;
        const FLOOR_Z: f32 = 80.405;
        /// The retail barrel and area (`OPR_TEST3` 1714.642 and 1715.608 s): where the barrel
        /// came, the war T1 barrel and area slices, and the times from the use.
        const RETAIL_BARREL: [f32; 2] = [1_374.731, 10_888.99];
        const BARREL_ASSET: &str = "05932228880d5bdf8fb1cc50513e7c62";
        const AREA_ASSET: &str = "b488e3f31cb65871baaf312aa2a52dc4";
        const POUR_MS: u64 = 300;
        const RETAIL_BARREL_MS: u64 = 316;
        const RETAIL_AREA_MS: u64 = 1_282;
        const RETAIL_RELEASE_MS: u64 = 1_466;
        /// How far a sim time may lie from a retail one: the retail sends of one state repeat
        /// 14 to 20 ms apart (`OPR_TEST3` 1714.326 to 1714.392 s), and an impact goes out with
        /// a later frame (Inferred: the retail area came 75 ms after the fall of the barrel
        /// from its 0.58 m over the vat to the floor under 13 m/s²).
        const RETAIL_SLACK_MS: u64 = 100;
        /// How near the barrel comes to the retail one, metres (the retail positions have
        /// three decimals).
        const PLACE_PRECISION_M: f32 = 0.02;
        /// The user's faction (Observed 1 on the retail use).
        const USER_FACTION: u8 = 1;
        /// The kind of the vat with the values of its slice (as the miner gives them: the use
        /// interact, the trigger and spawner facet rules, the pour, the war T1 barrel and its
        /// area).
        const VAT_KINDS: &str = r#"{"forts": [], "structure_kinds": [
            {"asset": "1326dd54e1345e24a15063bf88cfcff4", "slice": "slices/outpostrush/structures/or_turretfire.dynamicslice",
             "template": null, "template_inferred": false, "use_interact": 16,
             "trigger": {"xor": "0000022e8d2bb976", "component": "bfcf71e4ebd21074"}, "use_timers": [],
             "drop": {"projectile": {"asset": "05932228880d5bdf8fb1cc50513e7c62", "component": 2, "ranged_attack": 275,
                      "damage_table_crc": 81017683, "row_index": 17}, "ownership_component": 1, "gravity_mps2": 13.0,
                      "spawner_facet": {"xor": "0000003700000000", "component": "603842316ab22336"},
                      "pour": {"component": 18, "delay_s": 0.3}},
             "area": {"asset": "b488e3f31cb65871baaf312aa2a52dc4", "ownership_component": 3, "position_component": 1,
                      "timers": [{"component": 4, "delay_s": 4.0}, {"component": 5, "delay_s": 15.0}, {"component": 6, "delay_s": 4.0}],
                      "radius_m": 5.0, "once": false,
                      "damage": {"table": "DamageTable_Structures", "row": "FireBarrelDropper_DoT", "base_damage": 1200, "rate_s": 0.5},
                      "effects": []}}]}"#;

        /// A template of the vat: a foundation, or its built state with the spawner and the
        /// disabled build interact.
        fn vat_template(built: bool) -> String {
            let mut members = vec![
                BundleMember {
                    component: VITALS,
                    state: MemberState::Vitals(VitalsState {
                        slot_0: Some(VitalsSlot0 {
                            health_amount: Some(TURRET_HEALTH),
                            health_max: Some(TURRET_HEALTH),
                            ..VitalsSlot0::default()
                        }),
                        slot_1: None,
                    }),
                },
                BundleMember {
                    component: BUILDABLE,
                    state: MemberState::BuildableController(BuildableControllerState {
                        current_state: Some(if built { BUILT_STATE } else { FOUNDATION_STATE }),
                        ..BuildableControllerState::default()
                    }),
                },
            ];
            if built {
                members.extend([
                    BundleMember {
                        component: VAT_SPAWNER,
                        state: MemberState::Recorded {
                            class: nw_messages::turrets::PROJECTILE_SPAWNER_TYPE_INDEX,
                            body: ProjectileSpawnerState {
                                ammo: Some(UNCOUNTED),
                                firing_blocked: Some(false),
                            }
                            .encode(),
                        },
                    },
                    BundleMember {
                        component: VAT_BUILD_INTERACT,
                        state: MemberState::Interact(InteractState {
                            enabled: Some(false),
                            has_interactors: Some(0),
                        }),
                    },
                ]);
            }
            let encoded: Vec<String> = members
                .iter()
                .map(|member| format!("\"{}\"", hex::encode(member.encode().expect("encode"))))
                .collect();
            format!(
                r#"{{"card_component": 0, "place_component": 7, "ownership_component": {VAT_OWNERSHIP}, "members": [{}]}}"#,
                encoded.join(", ")
            )
        }

        /// The Outpost Rush match of [`turret_world`]'s players with the vat kind and a vat
        /// foundation of outpost A owned by the first player's team at the retail place, over
        /// the retail floor; the first player of the retail faction. The world, the players and
        /// the vat's (slot key, id).
        fn vat_world() -> (SimWorld, [Entity; 2], (u64, [u8; 16])) {
            let mut world = rush_sim_on(outpost_rush());
            world.content = Arc::new(PlayerContent {
                war_forts: WarForts::parse(VAT_KINDS).expect("kinds"),
                ..(*world.content).clone()
            });
            world.ground = Arc::new(nw_sim_core::test_support::FlatGround(FLOOR_Z));
            let table = Table::parse(&format!(
                r#"{{"kinds": [
                {{"asset": "{VAT}", "slice": "slices/outpostrush/structures/or_turretfire.dynamicslice",
                 "blueprint": null, "tier": 1, "foundation": {}, "built": {}}}], "maps": []}}"#,
                vat_template(false),
                vat_template(true),
            ))
            .expect("table");
            let team = running(&world).team_of(FIRST).expect("a team");
            own_outpost(&mut world, 0, team);
            let raid = running(&world).raid_seat_of(FIRST).expect("a seat").0;
            let spawner = StructureSpawner {
                name: "DefensiveStructureA1".to_owned(),
                outpost: 0,
                role: StructureRole::DefensiveStructure,
                number: 1,
                asset: asset(VAT),
                place: VAT_PLACE,
                yaw: VAT_YAW,
            };
            let slice = table.slice(&spawner.asset).expect("slice");
            let gde_ref = [0x60; RAW_ID_LEN];
            let mut structure = OutpostStructure::new(
                &spawner,
                slice,
                VAT_PLACE,
                gde_ref,
                false,
                (TURRET_HEALTH, TURRET_BUILD_MS as f32 / MS_PER_SECOND, Vec::new()),
            )
            .expect("a vat structure");
            structure.raid = raid;
            let structure = world.place_outpost_structure(structure);
            let number = structure.number;
            running_mut(&mut world).structures.structures.push(structure);
            let first = world.player_by_number(FIRST).expect("first");
            let second = world.player_by_number(SECOND).expect("second");
            if let Some(mut player) = world.world.get_mut::<Player>(first) {
                player.faction.faction = USER_FACTION;
            }
            (world, [first, second], (number, gde_ref))
        }

        fn dropper_of(world: &SimWorld, number: u64) -> Option<&StructureRun> {
            running(world)
                .structures
                .droppers
                .iter()
                .find(|run| run.body.number == number)
        }

        fn trigger_request(target: [u8; 16]) -> nw_messages::InteractionRequest {
            nw_messages::InteractionRequest {
                kind: nw_messages::client_input::TRIGGER_KIND,
                ..use_request(target)
            }
        }

        fn ownership_of(world: &SimWorld, number: u64, component: u64) -> OwnershipState {
            match member_of(world, number, component) {
                MemberState::Ownership(state) => state,
                other => panic!("an ownership, not {other:?}"),
            }
        }

        fn card_of(world: &SimWorld, number: u64) -> Option<String> {
            world.stations.members(number)?.iter().find_map(|member| match &member.state {
                MemberState::GdeMetadata(card) => card.asset.map(|asset| hex::encode(asset.id)),
                _ => None,
            })
        }

        fn idle_use_interact(enabled: bool) -> MemberState {
            MemberState::Interact(InteractState {
                enabled: Some(enabled),
                has_interactors: Some(NO_INTERACTORS),
            })
        }

        /// The vat's use interact with `interactors` and the cooldown of a first use at server
        /// ms `used_ms` (`OPR_TEST3` 1714.326 s: option 0, 20 s).
        fn cooled_use_interact(interactors: u32, used_ms: u64) -> MemberState {
            MemberState::InteractCooldown(nw_messages::InteractCooldownState {
                interact: InteractState {
                    enabled: Some(true),
                    has_interactors: Some(interactors),
                },
                cooldown_updates: nw_messages::items::FullList {
                    version: Some(FIRST_COOLDOWN_VERSION),
                    elements: vec![nw_messages::InteractCooldown {
                        option: USE_OPTION,
                        ends_unix_ns: crate::world::unix_ns(used_ms + DROPPER_COOLDOWN_MS),
                    }],
                },
            })
        }

        #[test]
        fn a_built_vat_is_a_dropper_of_its_owner_team_with_its_use_interact_enabled() {
            let (mut world, _, (vat, _)) = vat_world();
            assert!(dropper_of(&world, vat).is_none(), "a foundation is no dropper");
            build_all(&mut world, NOW);
            let run = dropper_of(&world, vat).expect("the built vat is a dropper");
            assert_eq!(run.body.structure.side, 0, "the first player's team owns outpost A");
            assert!(!run.body.broken);
            assert_eq!(member_of(&world, vat, VAT_USE_INTERACT), idle_use_interact(true));
            assert_eq!(
                member_of(&world, vat, VAT_BUILD_INTERACT),
                MemberState::Interact(InteractState {
                    enabled: Some(false),
                    has_interactors: Some(0),
                }),
                "the build's interact stays as retail sent it"
            );
            assert!(running(&world).structures.turrets.is_empty(), "no turret");
        }

        /// The retail use of `OPR_TEST3` (1714.326 to 1715.792 s) on the retail vat.
        #[test]
        fn the_owner_team_uses_a_vat_that_pours_drops_the_war_barrel_ahead_and_opens_its_fire_area() {
            let (mut world, [first, second], (vat, vat_ref)) = vat_world();
            build_all(&mut world, NOW);
            world.take_messages();
            let used_ms = world.server_ms;
            assert!(world.turret_interaction(second, SECOND, trigger_request(vat_ref), &[], NOW));
            assert!(answered(&world.take_messages(), SECOND, USE_REFUSED), "the other team");
            assert!(world.turret_interaction(first, FIRST, trigger_request(vat_ref), &[], NOW));
            let outgoing = world.take_messages();
            assert!(answered(&outgoing, FIRST, USE_ACCEPTED), "the owner team");
            let content = Arc::clone(&world.content);
            let kind = content.war_forts.structure_kind(&asset(VAT)).expect("the vat kind");
            let facet = dropper_of(&world, vat).expect("dropper").body.structure.facet_entity();
            let trigger = kind.trigger.expect("a trigger").target(facet);
            assert!(
                messages_to(&outgoing, FIRST).iter().any(|message| matches!(message,
                    ServerMessage::SyncExecuteEntities(call) if call.header.entity == facet && call.header.target == trigger)),
                "the 3135 of its trigger"
            );
            assert_eq!(
                member_of(&world, vat, VAT_USE_INTERACT),
                cooled_use_interact(1, used_ms),
                "the user shows on the use interact, with the vat's cooldown"
            );
            assert_eq!(
                member_of(&world, vat, VAT_POUR),
                MemberState::DelayedEvent(nw_messages::DelayedEventState {
                    total_delay_s: Some(0.3),
                    completed: None,
                }),
                "the pour starts"
            );
            let raid = running(&world).raid_seat_of(FIRST).expect("a seat").0;
            let character = world.world.get::<Player>(first).expect("player").character;
            let owned = ownership_of(&world, vat, VAT_OWNERSHIP);
            assert_eq!(
                owned.owner.map(|owner| owner.first),
                Some(character.hyphenated().to_string().into_bytes()),
                "the user owns it"
            );
            assert_eq!(
                (owned.raid_id, owned.permission_rule, owned.faction_type),
                (Some(raid), Some(4), Some(USER_FACTION)),
                "the team's raid, rule 4 and the user's faction"
            );
            assert_eq!(owned.guild_id, Some([0; 16]), "a user with no company names none");

            world.tick_turrets(NOW + POUR_MS - 1);
            assert!(running(&world).structures.uses.first_drop().is_none(), "still pouring");
            world.take_messages();
            world.tick_turrets(NOW + POUR_MS);
            assert!(POUR_MS.abs_diff(RETAIL_BARREL_MS) <= RETAIL_SLACK_MS);
            let spawner = kind.drop.as_ref().expect("a drop").spawner_facet.target(facet);
            assert!(
                messages_to(&world.take_messages(), SECOND).iter().any(|message| matches!(message,
                    ServerMessage::CombatEffect(CombatEffectMsg::ProjectileServerSpawn { header }) if header.target == spawner)),
                "the spawner's 2900"
            );
            assert_eq!(
                member_of(&world, vat, VAT_POUR),
                MemberState::DelayedEvent(nw_messages::DelayedEventState {
                    total_delay_s: Some(0.0),
                    completed: Some(true),
                }),
                "the pour completes"
            );
            let (barrel, land_ms, at) = running(&world).structures.uses.first_drop().expect("a falling barrel");
            assert_eq!(card_of(&world, barrel).as_deref(), Some(BARREL_ASSET), "the war T1 barrel");
            assert!(
                (at.x - RETAIL_BARREL[0]).abs() < PLACE_PRECISION_M && (at.y - RETAIL_BARREL[1]).abs() < PLACE_PRECISION_M,
                "2.79 m ahead of the vat as retail: {at:?}"
            );
            assert!((at.z - FLOOR_Z).abs() < f32::EPSILON, "it falls to the floor");
            let barrel_owner = ownership_of(&world, barrel, 1);
            assert_eq!((barrel_owner.raid_id, barrel_owner.faction_type), (Some(raid), Some(USER_FACTION)));
            assert!(
                (land_ms - NOW).abs_diff(RETAIL_AREA_MS) <= RETAIL_SLACK_MS,
                "the area opens about when retail's did: {} ms",
                land_ms - NOW
            );

            world.tick_turrets(land_ms);
            let (area, centre) = running(&world).structures.uses.first_area().expect("the fire area");
            assert!(running(&world).structures.uses.first_drop().is_none());
            assert_eq!(card_of(&world, area).as_deref(), Some(AREA_ASSET), "the war T1 area");
            assert!((centre.x - at.x).abs() < f32::EPSILON && (centre.y - at.y).abs() < f32::EPSILON, "where it landed");
            assert_eq!(ownership_of(&world, area, 3).raid_id, Some(raid));

            world.tick_turrets(NOW + DROPPER_USE_MS - 1);
            assert_eq!(
                dropper_of(&world, vat).and_then(|run| run.uses.user_number()),
                Some(FIRST),
                "held until its time"
            );
            world.tick_turrets(NOW + DROPPER_USE_MS);
            assert!(DROPPER_USE_MS.abs_diff(RETAIL_RELEASE_MS) <= RETAIL_SLACK_MS);
            assert_eq!(
                member_of(&world, vat, VAT_USE_INTERACT),
                cooled_use_interact(NO_INTERACTORS, used_ms),
                "the user is let go; the cooldown stays"
            );
            assert!(dropper_of(&world, vat).is_some_and(|run| run.uses.user_number().is_none()));
            world.take_messages();
            assert!(world.turret_interaction(first, FIRST, trigger_request(vat_ref), &[], NOW + DROPPER_USE_MS));
            assert!(answered(&world.take_messages(), FIRST, USE_REFUSED), "on cooldown");
            assert!(world.turret_interaction(first, FIRST, trigger_request(vat_ref), &[], NOW + DROPPER_COOLDOWN_MS));
            assert!(answered(&world.take_messages(), FIRST, USE_ACCEPTED), "free again after 20 s");
        }

        /// A turret shot of the vat's own team meets the built vat and stops there unharmed; a
        /// shot of the other team meets it as a foe's (`OPR_TEST3` 1719.807 s: three repeater
        /// shots of the vat's team on it, result `ff`, no health change).
        #[test]
        fn a_turret_shot_meets_the_built_vat_as_its_own_teams_or_a_foes() {
            use crate::world::turrets::{HeldHit, TurretShotTarget};
            let (mut world, _, (vat, vat_ref)) = vat_world();
            let met = |world: &mut SimWorld, shooter| {
                world.turret_shot_targets(shooter, &[], 0).into_iter().find_map(|(target, _, _)| {
                    matches!(target, TurretShotTarget::Structure { number, .. } if number == vat).then_some(target)
                })
            };
            assert_eq!(met(&mut world, FIRST), None, "a foundation stops no shot");
            build_all(&mut world, NOW);
            let own = met(&mut world, FIRST).expect("the vat");
            assert_eq!(
                own,
                TurretShotTarget::Structure { number: vat, facet: facet_of_raw_id(&vat_ref), foe: false }
            );
            assert_eq!(own.held(), None, "its own team's shot does it no harm");
            let foe = met(&mut world, SECOND).expect("the vat");
            assert_eq!(foe.held(), Some(HeldHit::Structure(vat)));
        }

        /// The vat's use names the user's company with the name and crest the world database
        /// gave, though the company holds no claim (`OPR_TEST3` 1714.326 s).
        #[test]
        fn a_vat_use_names_the_users_company_read_from_the_world_database() {
            use crate::world::company_looks::tests::{join_company, listing};
            const COMPANY: uuid::Uuid = uuid::Uuid::from_u128(0xc0_ffee);
            const COMPANY_NAME: &str = "Test Company";
            let (mut world, [first, _], (vat, vat_ref)) = vat_world();
            join_company(&mut world, first, COMPANY);
            world.tick_outpost_rush();
            assert_eq!(world.take_company_reads(), vec![COMPANY], "the match asks for it");
            world.companies_read(&[listing(COMPANY, COMPANY_NAME)]);
            build_all(&mut world, NOW);
            assert!(world.turret_interaction(first, FIRST, trigger_request(vat_ref), &[], NOW));
            let owned = ownership_of(&world, vat, VAT_OWNERSHIP);
            assert_eq!(owned.guild_id, Some(*COMPANY.as_bytes()));
            assert_eq!(owned.guild_owner, Some(COMPANY_NAME.as_bytes().to_vec()));
            assert!(owned.guild_crest.is_some(), "its crest");
        }

        #[test]
        fn a_ruin_stops_the_vats_use_and_disables_it_and_a_rebuild_frees_it() {
            let (mut world, [first, _], (vat, vat_ref)) = vat_world();
            build_all(&mut world, NOW);
            assert!(world.turret_interaction(first, FIRST, trigger_request(vat_ref), &[], NOW));
            let foe = running(&world).raid_seat_of(SECOND).expect("a seat").0;
            let ruined = with_set(&mut world, |world, set| {
                world.hurt_outpost_structure(set, foe, vat, TURRET_HEALTH as u32)
            });
            assert_eq!(ruined, Some(true));
            let run = dropper_of(&world, vat).expect("the dropper");
            assert!(run.body.broken && run.uses.user_number().is_none(), "broken, its use stopped");
            assert_eq!(member_of(&world, vat, VAT_USE_INTERACT), idle_use_interact(false));
            world.tick_turrets(NOW + POUR_MS);
            assert!(running(&world).structures.uses.first_drop().is_none(), "no barrel");
            world.take_messages();
            assert!(world.turret_interaction(first, FIRST, trigger_request(vat_ref), &[], NOW + POUR_MS));
            assert!(answered(&world.take_messages(), FIRST, USE_REFUSED), "disabled");
            with_set(&mut world, |_, set| {
                let structure = set
                    .structures
                    .iter_mut()
                    .find(|structure| structure.number == vat)
                    .expect("the structure");
                structure.phase = Phase::Foundation;
            });
            build_all(&mut world, NOW + TURRET_BUILD_MS);
            assert!(dropper_of(&world, vat).is_some_and(|run| !run.body.broken), "freed at the rebuild");
            assert_eq!(running(&world).structures.droppers.len(), 1, "one dropper a vat");
            assert_eq!(member_of(&world, vat, VAT_USE_INTERACT), idle_use_interact(true));
            world.take_messages();
            assert!(world.turret_interaction(first, FIRST, trigger_request(vat_ref), &[], NOW + TURRET_BUILD_MS));
            assert!(answered(&world.take_messages(), FIRST, USE_ACCEPTED), "usable again");
        }

        #[test]
        fn a_new_owner_team_takes_the_vat_and_stops_the_other_teams_use() {
            let (mut world, [first, second], (vat, vat_ref)) = vat_world();
            build_all(&mut world, NOW);
            assert!(world.turret_interaction(first, FIRST, trigger_request(vat_ref), &[], NOW));
            let foe = running(&world).raid_seat_of(SECOND).expect("a seat").0;
            with_set(&mut world, |world, set| {
                world.own_outpost_structures(set, [foe, NO_TEAM_RAID, NO_TEAM_RAID]);
            });
            let run = dropper_of(&world, vat).expect("the dropper");
            assert_eq!(run.body.structure.side, 1, "the second player's team");
            assert!(run.uses.user_number().is_none(), "the first team's use stops");
            world.tick_turrets(NOW + POUR_MS);
            assert!(running(&world).structures.uses.first_drop().is_none(), "no barrel");
            world.take_messages();
            assert!(world.turret_interaction(first, FIRST, trigger_request(vat_ref), &[], NOW + POUR_MS));
            assert!(answered(&world.take_messages(), FIRST, USE_REFUSED));
            assert!(world.turret_interaction(second, SECOND, trigger_request(vat_ref), &[], NOW + POUR_MS));
            assert!(answered(&world.take_messages(), SECOND, USE_ACCEPTED));
        }
    }
}
