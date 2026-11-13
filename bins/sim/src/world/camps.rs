// SPDX-License-Identifier: AGPL-3.0-or-later
//! The camp a player places (camp study 2026-10-30): its entity in the world, a crafting station
//! of its own spawn group ([`crafting::Stations`]).
//!
//! Retail `Hivecamp`: the 2236 at 1781367544.661 was answered by 441 at 544.698, and at 544.743
//! the camp's spawn group came: the card of its slice asset with a dynamic `GdeRef`, the built
//! buildable controller (state 5, the committed costs of its blueprint, the station type
//! `camp<tier>`, the tier buildable `CampT<tier>`, and for a PvP camp `isPvPCamp`), its position
//! and rotation (the 2236's), the territory interface, the delayed events, the placement
//! obstruction, the interact (disabled), for a PvP camp the vitals and the damage receiver, and
//! the ownership (the owner's character id and name). The members' component numbers are its
//! slice's ([`nw_world::camps`]). The player's camping state (1292) names the camp from then on,
//! and its home points (3652) list the camp's respawn point.
//!
//! **Build stage** (camp build study 2026-11-12: 11 retail camps of 8 ledgers, from
//! `12a_msq_12_camptutorial` to `Playthrough_Part_18`). A placed camp is not built yet:
//! - Its group comes in its slice's `m_initialState` (3) with an empty committed resource
//!   list, a completion obstruction (1) and no interact member (`12a_msq_12_camptutorial`
//!   284.221 s); the owner's builder state (2270) counts the camp (284.402 s) and 5361 goes to
//!   the owner (284.418 s).
//! - State 4 follows: the camp takes resources (284.402 s, 181 ms after the group; 69 ms in
//!   `Hive_camping`).
//! - The owner's build interaction (kind 16, `ApplyAllResources`) is accepted 527 to 560 ms after
//!   its first request (3743; 285.750 to 286.277 s, `99b_msq_10_cleansefort`,
//!   `Hive_camping`): the camp's interact shows one interactor and the builder state is
//!   building (286.303 s).
//! - Every [`COMMIT_INTERVAL_MS`] one unit of the next resource leaves the bag and joins the
//!   committed list (287.054 s to 290.886 s: `WoodT1` 1 to 5, then `FlintT1` 1); the first commit
//!   sets `checkFoundationEvents`.
//! - After the last unit the builder state is idle, the interaction ends (3746, 290.960 s) and
//!   the interactors are 0; the camp is built later (state 5, no obstruction, interact
//!   disabled; 292.888 s) and its `BindCamp` game event moves the camp tutorial (3233, 3236 at
//!   292.868 s).
//! - A cancel of the interaction (2473) stops the commits: the builder state is idle and the
//!   interactors 0, no 3746 (`Playthrough_Part_05` 3209.61 s); the committed resources stay and
//!   a later build goes on (3241.744 s).
//!
//! Retail `Hive_camping` (a capture of 2026-06-30, the oldest of them) committed every resource
//! at the placement and built the camp 3.2 to 4.2 s later with no interaction: that older form
//! is not run here.
//!
//! The camp is an entity of its owner's sim while the owner is in it (Inferred: no capture shows
//! a camp after its owner left). A save keeps its place and tier, not its rotation or blueprint:
//! a camp that comes back with its owner stands unturned, of the owner's camp skin or the default
//! camp of its tier (Inferred).

use bevy_ecs::entity::Entity;
use nw_bus::ConnectionRef;
use nw_content::camps::{camp_station_type, default_camp};
use nw_formats::objectstream::lowercase_crc32;
use nw_formats::rigid::{IDENTITY_QUAT, quat_normalize, rotate};
use nw_messages::buildables::NO_OWNER_KIND;
use nw_messages::facet::{InteractionEndedMsg, component};
use nw_messages::gde::AssetId;
use nw_messages::keyed_list::{KeyedList, MapChange};
use nw_messages::position::{PositionInTheWorldReplicatedState, Rotation, WorldPosition};
use nw_messages::travel::BUILDABLE_CONTROLLER_COMPONENT;
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    BUILDER_BUILDING, BUILDER_IDLE, BuildableControllerState, BuilderState, BundleMember,
    CommittedResources, DamageReceiverState, DelayedEventState, FacetHeader,
    GdeMetadataReplicatedState, GuildCrest, InteractState, InteractionRequest, MemberState,
    OwnerIdentification, OwnershipState, PlacementObstructionState, ServerMessage,
    TerritoryInterfaceState, TravelServerMsg, VitalsData, VitalsSlot0, VitalsState,
    committed_resource,
};
use nw_sim_core::Position;
use nw_world::camps::CampSlice;
use uuid::Uuid;

use super::crafting::{
    CARD_COMPONENT, NO_FACTION, NO_GUILD_ID, NO_RAID, RUIN_COMPLETED, RUIN_DELAY, Station,
    TERRITORY_REFS_SET, committed,
};
use super::travel::BIND_CAMP_EVENT;
use super::{Player, SimWorld, gather_answer, send_now};

/// Names the camp ids this server makes.
const CAMP_ID_NAMESPACE: u64 = 0x6361_6d70_7369_7465;
/// `currentState` of a built camp (every retail camp once built).
const BUILT: u8 = 5;
/// `currentState` of a camp that takes resources (every retail build, 69 to 181 ms after its
/// group came in the slice's `m_initialState`).
const COMMITTABLE: u8 = 4;
/// How long after its placement a camp takes resources (Inferred: within the retail 69 to
/// 181 ms).
pub(super) const COMMITTABLE_DELAY_MS: u64 = 100;
/// `hasCompletionObstruction` of a camp that is not built (every retail placement) and of one
/// that is (every retail completion).
const COMPLETION_OBSTRUCTED: u8 = 1;
const COMPLETION_FREE: u8 = 0;
/// `checkFoundationEvents` of a camp built by its owner's commits (Observed: the first commit,
/// `12a_msq_12_camptutorial` 287.054 s, and the built camp that came back into view,
/// `12a_msq_16_fasttravel` 67.55 s).
const COMMITTED_CHECK_FOUNDATION_EVENTS: bool = true;
/// The interaction kind of a camp build (`InteractionUIActions` `ApplyAllResources`).
pub(super) const APPLY_ALL_RESOURCES_KIND: u8 = 16;
/// How long after the first build request the build starts (Inferred: within the retail 527
/// to 560 ms of three ledgers).
pub(super) const BUILD_ACCEPT_DELAY_MS: u64 = 530;
/// How long one unit of a resource takes to commit (Observed: 765 to 777 ms from the accept to
/// the first commit and 767 ms between commits, `12a_msq_12_camptutorial`,
/// `99b_msq_10_cleansefort`, `20261004-175416-488`).
pub(super) const COMMIT_INTERVAL_MS: u64 = 767;
/// How long after the last commit the camp is built (Inferred: the median of the retail 1,065
/// to 2,315 ms of six builds; what sets it is Unknown).
pub(super) const COMPLETION_DELAY_MS: u64 = 1_640;
/// One unit of a resource.
const ONE_UNIT: u32 = 1;
/// Interactors of a camp while its owner builds it, and after.
const ONE_INTERACTOR: u32 = 1;
const NO_INTERACTORS: u32 = 0;
/// The 3743 status of an accepted build, and of a build of a camp that is not the player's
/// (Inferred: the refusal status of a busy gather).
const BUILD_ACCEPTED: u8 = 1;
const BUILD_REFUSED: u8 = 2;
/// The completion is blocked (5361, every retail body).
const COMPLETION_BLOCKED: bool = true;
/// `Enabled` of a built camp's first interact (both retail camps).
const INTERACT_ENABLED: bool = false;
/// `permissionOptions` of a player's camp (both retail camps).
const CAMP_PERMISSION_OPTIONS: u8 = 3;
/// `ownerFFAFlag` (both retail camps).
const OWNER_FFA: bool = false;
/// The vitals of a PvP camp's group (retail `Hivecamp` 1781367544743: no health of its own in
/// the vitals, health 0 of 1, stamina 0 of 1, no ticks, no change flags).
const CAMP_HEALTH: f32 = 0.0;
const CAMP_HEALTH_MAX: f32 = 1.0;
const CAMP_MAX_HEALTH: u16 = 1;
const CAMP_TICK_RATE: u16 = 0;
const CAMP_HEALTH_CHANGE_FLAGS: u8 = 0;
/// The damage receiver of a PvP camp's group (retail `Hivecamp`: not blocking, slot alias 52;
/// the alias's meaning is Unknown).
const CAMP_BLOCK_ACTIVE: bool = false;
const CAMP_BLOCK_WEAPON_SLOT_ALIAS: u32 = 52;

/// A player's camp entity: what its camping state, home points and respawn name.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CampEntity {
    /// Its slot key ([`crafting::Stations`]).
    pub(super) number: u64,
    /// Its `GdeRef`.
    pub(super) gde_ref: [u8; RAW_ID_LEN],
    /// Where its owner respawns at it.
    pub(super) spawn_point: Position,
}

impl CampEntity {
    /// Its facet entity: the first 8 bytes of its `GdeRef`, little-endian (Observed: retail
    /// `Hivecamp` camp `6e5ac4808181beaf…` was facet 12663701595423529582 in 1292, 3652 and the
    /// respawn request).
    pub(super) fn facet(&self) -> u64 {
        let mut bytes = [0; size_of::<u64>()];
        bytes.copy_from_slice(&self.gde_ref[..size_of::<u64>()]);
        u64::from_le_bytes(bytes)
    }
}

/// What a camp's group holds beyond its slice.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct CampSpawn<'a> {
    /// Its `GdeRef`.
    pub(super) gde_ref: [u8; RAW_ID_LEN],
    /// Where it stands.
    pub(super) position: Position,
    /// Its rotation (x, y, z, w).
    pub(super) rotation: [f32; 4],
    /// Its tier: the owner's `Camping` rank.
    pub(super) tier: u32,
    /// What its blueprint costs (item id, quantity).
    pub(super) costs: &'a [(String, u32)],
    /// The owner's character id and name.
    pub(super) character: Uuid,
    pub(super) name: &'a str,
    /// The owner's PvP flag.
    pub(super) owner_pvp: bool,
    /// The server time of the placement (ms since 2000-01-01).
    pub(super) server_ms: u64,
    /// Whether it stands built (a camp that comes back with its owner) or was just placed.
    pub(super) built: bool,
}

impl CampSpawn<'_> {
    /// The world point of the slice point `local` of the camp.
    fn world_point(&self, local: [f32; 3]) -> Position {
        let [x, y, z] = rotate(quat_normalize(self.rotation), local);
        Position {
            x: self.position.x + x,
            y: self.position.y + y,
            z: self.position.z + z,
        }
    }
}

/// The spawn group members of the camp of `slice` placed as `spawn`, with replicated lists at
/// `version`, by component number.
pub(super) fn camp_members(
    slice: &CampSlice,
    spawn: &CampSpawn<'_>,
    version: u64,
) -> Vec<BundleMember> {
    let station = &slice.station;
    let member = |component: u8, state| BundleMember {
        component: u64::from(component),
        state,
    };
    let mut members = vec![BundleMember {
        component: CARD_COMPONENT,
        state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
            asset: Some(AssetId {
                id: slice.asset,
                sub_id: slice.asset_sub_id,
            }),
            gde_ref: Some(spawn.gde_ref),
        }),
    }];
    let pvp = slice.vitals.is_some();
    if let Some((component, initial_state)) = station.buildable {
        let (current_state, check_foundation_events, resources) = if spawn.built {
            (
                BUILT,
                Some(COMMITTED_CHECK_FOUNDATION_EVENTS),
                committed(spawn.costs),
            )
        } else {
            (initial_state, None, Vec::new())
        };
        members.push(member(
            component,
            MemberState::BuildableController(BuildableControllerState {
                current_state: Some(current_state),
                check_foundation_events,
                committed_resources: Some(KeyedList::full(version, resources)),
                crafting_station_type_crc: Some(lowercase_crc32(&camp_station_type(spawn.tier))),
                camp_tier_buildable_id: Some(lowercase_crc32(&default_camp(spawn.tier))),
                is_pvp_camp: pvp.then_some(true),
                ..BuildableControllerState::default()
            }),
        ));
    }
    members.push(member(
        slice.position,
        MemberState::Position(PositionInTheWorldReplicatedState {
            position: WorldPosition::from_metres(
                spawn.position.x,
                spawn.position.y,
                spawn.position.z,
            ),
            rotation: Some(Rotation::of_quaternion(spawn.rotation)),
            scale: None,
        }),
    ));
    if let Some(component) = station.territory {
        members.push(member(
            component,
            MemberState::TerritoryInterface(TerritoryInterfaceState {
                functionality_down: None,
                refs_set: Some(TERRITORY_REFS_SET),
                ..TerritoryInterfaceState::default()
            }),
        ));
    }
    for component in &slice.delayed_events {
        members.push(member(
            *component,
            MemberState::DelayedEvent(DelayedEventState {
                total_delay_s: Some(RUIN_DELAY),
                completed: Some(RUIN_COMPLETED),
            }),
        ));
    }
    if let Some((component, completion)) = station.obstruction {
        let obstruction = if spawn.built {
            u8::from(completion)
        } else {
            COMPLETION_OBSTRUCTED
        };
        members.push(member(
            component,
            MemberState::PlacementObstruction(PlacementObstructionState {
                has_completion_obstruction: Some(obstruction),
            }),
        ));
    }
    if let Some(component) = slice.interact.filter(|_| spawn.built) {
        members.push(member(
            component,
            MemberState::Interact(InteractState {
                enabled: Some(INTERACT_ENABLED),
                has_interactors: None,
            }),
        ));
    }
    if let Some(vitals) = &slice.vitals {
        members.push(member(
            vitals.component,
            MemberState::Vitals(VitalsState {
                slot_0: Some(VitalsSlot0 {
                    health_amount: Some(CAMP_HEALTH),
                    stamina_amount: Some(CAMP_HEALTH),
                    vitals_data: Some(VitalsData::alive_since(spawn.server_ms)),
                    health_change_flags: Some(CAMP_HEALTH_CHANGE_FLAGS),
                    health_max: Some(CAMP_HEALTH_MAX),
                    health_tick_rate: Some(CAMP_TICK_RATE),
                    stamina_max: Some(CAMP_HEALTH_MAX),
                    stamina_tick_rate: Some(CAMP_TICK_RATE),
                    vitals_id: Some(lowercase_crc32(&vitals.row)),
                    max_health: Some(CAMP_MAX_HEALTH),
                    ..VitalsSlot0::default()
                }),
                slot_1: None,
            }),
        ));
    }
    if let Some(component) = slice.damage_receiver {
        members.push(member(
            component,
            MemberState::DamageReceiver(DamageReceiverState {
                is_block_active: Some(CAMP_BLOCK_ACTIVE),
                block_weapon_slot_alias: Some(CAMP_BLOCK_WEAPON_SLOT_ALIAS),
                debug_crit_chance: None,
            }),
        ));
    }
    if let Some((component, rule)) = station.ownership {
        // The owner's guild, crest and faction are the social service's; this sim has none of
        // them, so the camp is of no guild and faction.
        members.push(member(
            component,
            MemberState::Ownership(OwnershipState {
                guild_id: Some(NO_GUILD_ID),
                guild_owner: Some(Vec::new()),
                guild_crest: Some(GuildCrest::NONE),
                owner: Some(OwnerIdentification {
                    first: spawn.character.hyphenated().to_string().into_bytes(),
                    second: spawn.name.as_bytes().to_vec(),
                    // Retail sends the constructor's kind with an owner too (`Hivecamp`).
                    kind: NO_OWNER_KIND,
                }),
                permission_rule: Some(rule),
                structure_name: Some(station.structure_name.as_bytes().to_vec()),
                raid_id: Some(NO_RAID),
                faction_type: Some(NO_FACTION),
                owner_pvp: Some(spawn.owner_pvp),
                owner_ffa: Some(OWNER_FFA),
                permission_options: Some(CAMP_PERMISSION_OPTIONS),
            }),
        ));
    }
    members.sort_by_key(|member| member.component);
    members
}

/// The `GdeRef` of the camp of slot key `number` of the sim started at `generation` (Unix ms):
/// a dynamic id (retail camps have an even first byte, as every dynamic spawn).
fn camp_id(generation: u64, number: u64) -> [u8; RAW_ID_LEN] {
    let namespace = Uuid::from_u64_pair(CAMP_ID_NAMESPACE, generation);
    nw_messages::as_dynamic_raw_id(*Uuid::new_v5(&namespace, &number.to_be_bytes()).as_bytes())
}

/// Where a camp goes and what it is.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct CampPlacement {
    /// The camp blueprint placed (`CampT5`, `CampSkin_MetalFest02_A_PVP`).
    pub(super) blueprint: String,
    /// Where it stands.
    pub(super) position: Position,
    /// Its rotation (x, y, z, w).
    pub(super) rotation: [f32; 4],
    /// Its tier.
    pub(super) tier: u32,
}

impl SimWorld {
    /// Spawns the camp `placement` of player `entity` in the world, `built` or just placed: its
    /// group goes to every viewer near it and it is a crafting station of its tier. `None` when
    /// the content has no slice of the blueprint or the player is not here.
    pub(super) fn spawn_camp(
        &mut self,
        entity: Entity,
        placement: &CampPlacement,
        built: bool,
    ) -> Option<CampEntity> {
        let slice = self.content.camp_slices.get(&placement.blueprint)?;
        let player = self.world.get::<Player>(entity)?;
        let generation = self.me.generation;
        let server_ms = self.server_ms;
        let costs = self.content.structure_costs.of(&placement.blueprint);
        let mut camp = None;
        self.stations.place_camp(|number| {
            let spawn = CampSpawn {
                gde_ref: camp_id(generation, number),
                position: placement.position,
                rotation: placement.rotation,
                tier: placement.tier,
                costs,
                character: player.character,
                name: &player.name,
                owner_pvp: player.pvp_flagged,
                server_ms,
                built,
            };
            camp = Some(CampEntity {
                number,
                gde_ref: spawn.gde_ref,
                spawn_point: spawn.world_point(slice.spawn_point),
            });
            Station {
                gde_ref: spawn.gde_ref,
                position: placement.position,
                station_type: camp_station_type(placement.tier),
                name: slice.station.name.as_bytes().to_vec(),
                members: camp_members(slice, &spawn, nw_messages::items::FIRST_VERSION),
            }
        });
        tracing::info!(
            player = player.number,
            blueprint = %placement.blueprint,
            tier = placement.tier,
            "camp spawned"
        );
        camp
    }

    /// Takes the camp entity of player `entity` out of the world, when it has one, with its
    /// build: a build that ran leaves the builder state idle.
    pub(super) fn despawn_camp(&mut self, entity: Entity) {
        let camp = self.world.get_mut::<Player>(entity).and_then(|mut player| {
            let build = player.travel.camp_build.take();
            if build.is_some_and(|build| matches!(build.stage, BuildStage::Committing { .. })) {
                player.travel.builder_changed(building(false));
            }
            player.travel.camp.take()
        });
        if let Some(camp) = camp {
            self.stations.remove_camp(camp.number);
        }
    }

    /// The camp of player `entity` comes with it into this sim: a saved camp stands again, of
    /// its saved blueprint and rotation; a camp saved before they were kept (world migration
    /// 0027) is of the player's camp skin or the default camp of its tier, unturned.
    pub(super) fn camp_arrived(&mut self, entity: Entity) {
        if self.is_tutorial() {
            return;
        }
        let Some((saved, skin)) = self.world.get::<Player>(entity).and_then(|player| {
            let travel = player.travel.carried();
            Some((travel.placed_camp?, travel.camp_skin))
        }) else {
            return;
        };
        let tier = u32::from(saved.tier);
        let camps = &self.content.camps;
        let blueprint = camps
            .blueprint_of_crc(saved.blueprint)
            .or_else(|| {
                skin.and_then(|skin| camps.skin_of_crc(skin))
                    .and_then(|skin| camps.blueprint(&skin.id))
            })
            .map_or_else(|| default_camp(tier), |blueprint| blueprint.id.clone());
        let nw_bus::Point { x, y, z } = saved.position.point();
        let rotation = if saved.rotation == [0; 4] {
            IDENTITY_QUAT
        } else {
            saved.rotation.map(f32::from_bits)
        };
        let placement = CampPlacement {
            blueprint,
            position: Position { x, y, z },
            rotation,
            tier,
        };
        let camp = self.spawn_camp(entity, &placement, true);
        let Some(camp) = camp else {
            return;
        };
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.travel.camp = Some(camp);
            player.travel.camp_blocked_due = true;
            player.travel.builder_changed(placed_deployables(true));
        }
    }
}

/// Where the build of a placed camp stands.
#[derive(Debug, Clone, Copy, PartialEq)]
enum BuildStage {
    /// Just placed: it takes resources from `committable_ms` (sim ms) on.
    Placed { committable_ms: u64 },
    /// It takes resources; nobody builds it.
    Committable,
    /// Its owner asked to build it with `request`: the build starts at `accept_ms`.
    Asked {
        request: InteractionRequest,
        accept_ms: u64,
    },
    /// Its owner builds it with interaction `interaction`: the next unit goes at `commit_ms`.
    Committing { interaction: u64, commit_ms: u64 },
    /// Every resource is in: it is built at `complete_ms`.
    Completing { complete_ms: u64 },
}

/// The build of a placed camp ([module doc](self)): what it costs, what is in, its list version
/// and stage. A build is not saved: a camp that is not built yet does not come back with its
/// owner (Inferred: no capture shows an owner leave during a build).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct CampBuild {
    /// What its blueprint costs (item id, quantity), in commit order.
    costs: Vec<(String, u32)>,
    /// What is committed (item id, quantity), in commit order.
    committed: Vec<(String, u32)>,
    /// The version of its committed resource list.
    list_version: u64,
    stage: BuildStage,
}

impl CampBuild {
    /// The build of a camp of `costs` placed at `now_ms`, whose group holds its list at
    /// `list_version`.
    fn placed(costs: &[(String, u32)], list_version: u64, now_ms: u64) -> Self {
        Self {
            costs: costs.to_vec(),
            committed: Vec::new(),
            list_version,
            stage: BuildStage::Placed {
                committable_ms: now_ms.saturating_add(COMMITTABLE_DELAY_MS),
            },
        }
    }

    /// The item of the next unit to commit: the first cost not all in.
    fn next_item(&self) -> Option<&str> {
        self.costs
            .iter()
            .find(|(item, quantity)| self.quantity_of(item) < *quantity)
            .map(|(item, _)| item.as_str())
    }

    /// How much of `item` is committed.
    fn quantity_of(&self, item: &str) -> u32 {
        self.committed
            .iter()
            .find(|(committed, _)| committed == item)
            .map_or(0, |(_, quantity)| *quantity)
    }

    /// One more unit of `item` is in; the list's version goes up.
    fn commit(&mut self, item: &str) {
        match self
            .committed
            .iter_mut()
            .find(|(committed, _)| committed == item)
        {
            Some((_, quantity)) => *quantity = quantity.saturating_add(ONE_UNIT),
            None => self.committed.push((item.to_owned(), ONE_UNIT)),
        }
        self.list_version = self.list_version.saturating_add(1);
    }

    /// The committed list in its full form.
    fn full_list(&self) -> CommittedResources {
        KeyedList::full(self.list_version, committed(&self.committed))
    }

    /// The change of the committed list after a commit of `item`: the full list, or the delta
    /// of `item` alone when it is shorter. Observed: retail sent the full list while it held
    /// one resource (`12a_msq_12_camptutorial` 287.054 to 290.120 s, versions 9 to 13: the
    /// delta is as long) and the delta of `FlintT1` when it added a second (290.886 s).
    fn list_change(&self, item: &str) -> CommittedResources {
        let full = self.full_list();
        let (key, value) = committed_resource(item, lowercase_crc32(item), self.quantity_of(item));
        let delta = KeyedList::Changes(vec![MapChange {
            key,
            version: Some(self.list_version),
            value: Some(value),
        }]);
        if encoded_len(&delta) < encoded_len(&full) {
            delta
        } else {
            full
        }
    }
}

/// The bytes of committed resource list `list` on the wire.
fn encoded_len(list: &CommittedResources) -> usize {
    let mut writer = nw_messages::Writer::new();
    // A list of blueprint item ids fits its prefixes.
    let _ = list.write(&mut writer, |writer, descriptor| descriptor.write(writer));
    writer.finish().len()
}

/// The 5361 of camp `camp` to its owner ([`TravelServerMsg::CompletionBlocked`]).
pub(super) fn completion_blocked(camp: &CampEntity) -> ServerMessage {
    ServerMessage::Travel(TravelServerMsg::CompletionBlocked {
        header: FacetHeader {
            target: camp.facet().wrapping_add(BUILDABLE_CONTROLLER_COMPONENT),
            entity: camp.facet(),
        },
        blocked: COMPLETION_BLOCKED,
    })
}

/// The builder state (2270) that counts `placed` camps of a player.
pub(super) const fn placed_deployables(placed: bool) -> BuilderState {
    BuilderState {
        builder: None,
        placed: Some(if placed { 1 } else { 0 }),
    }
}

/// The builder state (2270) of a player who builds (`building`) or not.
const fn building(building: bool) -> BuilderState {
    BuilderState {
        builder: Some(if building {
            BUILDER_BUILDING
        } else {
            BUILDER_IDLE
        }),
        placed: None,
    }
}

impl SimWorld {
    /// The camp `camp` of player `entity`, placed at `now_ms` and costing `costs`, starts its
    /// build.
    pub(super) fn start_camp_build(
        &mut self,
        entity: Entity,
        costs: &[(String, u32)],
        now_ms: u64,
    ) {
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.travel.camp_build = Some(CampBuild::placed(
                costs,
                nw_messages::items::FIRST_VERSION,
                now_ms,
            ));
            player.travel.camp_blocked_due = true;
            player.travel.builder_changed(placed_deployables(true));
        }
    }

    /// Player `number` asks to build the camp of `request` (kind
    /// [`APPLY_ALL_RESOURCES_KIND`]) at `now_ms`: its own camp that is not built starts its
    /// build after [`BUILD_ACCEPT_DELAY_MS`] (or when it takes resources, if later); a camp
    /// whose resources are all in is accepted and ended at once (Observed: `Hive_camping` 59.49
    /// and 59.50 s); any other camp is refused.
    pub(super) fn ask_camp_build(
        &mut self,
        entity: Entity,
        number: u64,
        request: InteractionRequest,
        now_ms: u64,
    ) {
        let own = self.world.get::<Player>(entity).is_some_and(|player| {
            player
                .travel
                .camp
                .is_some_and(|camp| camp.gde_ref == request.target)
                && player.travel.camp_build.is_some()
        });
        if !own {
            self.send_travel(
                entity,
                &[gather_answer(number, request, BUILD_REFUSED, &[])],
            );
            return;
        }
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let Some(build) = player.travel.camp_build.as_mut() else {
            return;
        };
        let accept_ms = now_ms.saturating_add(BUILD_ACCEPT_DELAY_MS);
        match build.stage {
            BuildStage::Placed { committable_ms } => {
                build.stage = BuildStage::Asked {
                    request,
                    accept_ms: accept_ms.max(committable_ms),
                };
            }
            BuildStage::Committable => {
                build.stage = BuildStage::Asked { request, accept_ms };
            }
            BuildStage::Completing { .. } => {
                let ended = ServerMessage::InteractionEnded(InteractionEndedMsg {
                    header: FacetHeader::of(component::INTERACTOR, number),
                });
                self.send_travel(
                    entity,
                    &[gather_answer(number, request, BUILD_ACCEPTED, &[]), ended],
                );
            }
            BuildStage::Asked { .. } | BuildStage::Committing { .. } => {}
        }
    }

    /// The client of player `entity` cancelled interaction `interaction` (2473): a build that
    /// runs with it stops, its committed resources stay.
    pub(super) fn cancel_camp_build(&mut self, entity: Entity, interaction: u64) {
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let Some(build) = player.travel.camp_build.as_mut() else {
            return;
        };
        let stopped = match build.stage {
            BuildStage::Committing {
                interaction: running,
                ..
            } => running == interaction,
            BuildStage::Asked { request, .. } => request.interaction == interaction,
            _ => false,
        };
        if !stopped {
            return;
        }
        let was_committing = matches!(build.stage, BuildStage::Committing { .. });
        build.stage = BuildStage::Committable;
        if was_committing {
            player.travel.builder_changed(building(false));
            if let Some(camp) = player.travel.camp {
                self.change_interactors(&camp, NO_INTERACTORS);
            }
        }
    }

    /// The camps at `now_ms`: each owner whose client got its camp's group since the last tick
    /// gets the 5361 ([`completion_blocked`]; retail 17 to 197 ms after the group), and every
    /// build whose time came runs its next stage (the [module doc](self) stages).
    pub(super) fn tick_camps(&mut self, now_ms: u64) {
        let blocked: Vec<(ConnectionRef, CampEntity)> = self
            .world
            .query::<&mut Player>()
            .iter_mut(&mut self.world)
            .filter_map(|mut player| {
                let camp = player.travel.camp?;
                let shown = player.slots.assigned().contains_key(&camp.number);
                (player.travel.camp_blocked_due && shown).then(|| {
                    player.travel.camp_blocked_due = false;
                    (player.connection.clone(), camp)
                })
            })
            .collect();
        for (connection, camp) in blocked {
            send_now(
                &mut self.combat_messages,
                &connection,
                &[completion_blocked(&camp)],
            );
        }
        let due: Vec<(Entity, u64, CampEntity, BuildStage)> = self
            .world
            .query::<(Entity, &Player)>()
            .iter(&self.world)
            .filter_map(|(entity, player)| {
                let build = player.travel.camp_build.as_ref()?;
                let ready = match build.stage {
                    BuildStage::Placed { committable_ms } => committable_ms,
                    BuildStage::Asked { accept_ms, .. } => accept_ms,
                    BuildStage::Committing { commit_ms, .. } => commit_ms,
                    BuildStage::Completing { complete_ms } => complete_ms,
                    BuildStage::Committable => return None,
                };
                let camp = player.travel.camp?;
                (now_ms >= ready).then_some((entity, player.number, camp, build.stage))
            })
            .collect();
        for (entity, number, camp, stage) in due {
            match stage {
                BuildStage::Placed { .. } => self.camp_committable(entity, &camp),
                BuildStage::Asked { request, .. } => {
                    self.accept_camp_build(entity, number, &camp, request, now_ms);
                }
                BuildStage::Committing { interaction, .. } => {
                    self.commit_camp_unit(entity, number, &camp, interaction, now_ms);
                }
                BuildStage::Completing { .. } => self.complete_camp(entity, number, &camp, now_ms),
                BuildStage::Committable => {}
            }
        }
    }

    /// The camp `camp` of player `entity` takes resources (state 4).
    fn camp_committable(&mut self, entity: Entity, camp: &CampEntity) {
        if let Some(mut player) = self.world.get_mut::<Player>(entity)
            && let Some(build) = player.travel.camp_build.as_mut()
            && let BuildStage::Placed { .. } = build.stage
        {
            build.stage = BuildStage::Committable;
        }
        self.change_buildable(camp, |state| state.current_state = Some(COMMITTABLE), None);
    }

    /// The build `request` of player `number` starts: accepted (3743), one interactor, the
    /// builder state building, the first unit [`COMMIT_INTERVAL_MS`] on.
    fn accept_camp_build(
        &mut self,
        entity: Entity,
        number: u64,
        camp: &CampEntity,
        request: InteractionRequest,
        now_ms: u64,
    ) {
        if let Some(mut player) = self.world.get_mut::<Player>(entity)
            && let Some(build) = player.travel.camp_build.as_mut()
        {
            build.stage = BuildStage::Committing {
                interaction: request.interaction,
                commit_ms: now_ms.saturating_add(COMMIT_INTERVAL_MS),
            };
            player.travel.builder_changed(building(true));
        }
        self.send_travel(
            entity,
            &[gather_answer(number, request, BUILD_ACCEPTED, &[])],
        );
        self.change_interactors(camp, ONE_INTERACTOR);
    }

    /// One unit of the next resource of the camp `camp` leaves the bag of player `number` and
    /// joins the committed list. The build ends after the last unit, or when the bag does not
    /// hold the next one (Inferred: no capture shows a bag run out during a build).
    fn commit_camp_unit(
        &mut self,
        entity: Entity,
        number: u64,
        camp: &CampEntity,
        interaction: u64,
        now_ms: u64,
    ) {
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let player = &mut *player;
        let Some(build) = player.travel.camp_build.as_mut() else {
            return;
        };
        let Some(item) = build.next_item().map(str::to_owned) else {
            return;
        };
        let taken = player
            .gear
            .as_mut()
            .is_some_and(|gear| gear.take_as(&item, ONE_UNIT, nw_messages::items::SYNC_CRAFT_USED));
        let first = build.committed.is_empty();
        let change = taken.then(|| {
            build.commit(&item);
            (build.list_change(&item), build.full_list())
        });
        let done = build.next_item().is_none();
        build.stage = match (taken, done) {
            (true, true) => BuildStage::Completing {
                complete_ms: now_ms.saturating_add(COMPLETION_DELAY_MS),
            },
            (true, false) => BuildStage::Committing {
                interaction,
                commit_ms: now_ms.saturating_add(COMMIT_INTERVAL_MS),
            },
            (false, _) => BuildStage::Committable,
        };
        if !taken || done {
            player.travel.builder_changed(building(false));
        }
        if let Some((list_change, full)) = change {
            let check = first.then_some(COMMITTED_CHECK_FOUNDATION_EVENTS);
            self.change_buildable(
                camp,
                |state| {
                    state.committed_resources = Some(full);
                    state.check_foundation_events = check.or(state.check_foundation_events);
                },
                Some(BuildableControllerState {
                    check_foundation_events: check,
                    committed_resources: Some(list_change),
                    ..BuildableControllerState::default()
                }),
            );
        }
        if !taken || done {
            let ended = ServerMessage::InteractionEnded(InteractionEndedMsg {
                header: FacetHeader::of(component::INTERACTOR, number),
            });
            self.send_travel(entity, &[ended]);
            self.change_interactors(camp, NO_INTERACTORS);
        }
    }

    /// The camp `camp` of player `number` is built: state 5, no obstruction, its interact off,
    /// and its `BindCamp` game event fires (the camp tutorial's `TaskGameEvent`).
    fn complete_camp(&mut self, entity: Entity, number: u64, camp: &CampEntity, now_ms: u64) {
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.travel.camp_build = None;
        }
        self.change_buildable(camp, |state| state.current_state = Some(BUILT), None);
        let Some(slice) = self.camp_slice_of(camp) else {
            return;
        };
        let mut changes = Vec::new();
        if let Some((component, _)) = slice.station.obstruction {
            changes.push(BundleMember {
                component: u64::from(component),
                state: MemberState::PlacementObstruction(PlacementObstructionState {
                    has_completion_obstruction: Some(COMPLETION_FREE),
                }),
            });
        }
        if let Some(component) = slice.interact {
            changes.push(BundleMember {
                component: u64::from(component),
                state: MemberState::Interact(InteractState {
                    enabled: Some(INTERACT_ENABLED),
                    has_interactors: None,
                }),
            });
        }
        self.change_camp_members(camp, changes);
        tracing::info!(player = number, "camp built");
        self.fire_game_event(number, BIND_CAMP_EVENT, now_ms);
    }

    /// The slice of the camp `camp`: the blueprint its card names.
    fn camp_slice_of(&self, camp: &CampEntity) -> Option<CampSlice> {
        let members = self.stations.members(camp.number)?;
        let asset = members.iter().find_map(|member| match &member.state {
            MemberState::GdeMetadata(card) => card.asset,
            _ => None,
        })?;
        self.content
            .camp_slices
            .by_blueprint
            .values()
            .find(|slice| slice.asset == asset.id && slice.asset_sub_id == asset.sub_id)
            .cloned()
    }

    /// The buildable controller of the camp `camp` changes: `merge` puts it into the group's
    /// state, `change` goes to the viewers (the merged state when `None`).
    fn change_buildable(
        &mut self,
        camp: &CampEntity,
        merge: impl FnOnce(&mut BuildableControllerState),
        change: Option<BuildableControllerState>,
    ) {
        let Some(members) = self.stations.members(camp.number) else {
            return;
        };
        let Some((component, mut state)) = members.iter().find_map(|member| match &member.state {
            MemberState::BuildableController(state) => Some((member.component, state.clone())),
            _ => None,
        }) else {
            return;
        };
        let before = state.clone();
        merge(&mut state);
        let sent = change.unwrap_or_else(|| BuildableControllerState {
            current_state: state
                .current_state
                .filter(|_| state.current_state != before.current_state),
            ..BuildableControllerState::default()
        });
        let merged = BundleMember {
            component,
            state: MemberState::BuildableController(state),
        };
        self.stations.change_camp(
            camp.number,
            |members| put_member(members, merged),
            vec![BundleMember {
                component,
                state: MemberState::BuildableController(sent),
            }],
        );
    }

    /// The interact of the camp `camp` shows `interactors` interactors.
    fn change_interactors(&mut self, camp: &CampEntity, interactors: u32) {
        let Some(component) = self.camp_slice_of(camp).and_then(|slice| slice.interact) else {
            return;
        };
        self.change_camp_members(
            camp,
            vec![BundleMember {
                component: u64::from(component),
                state: MemberState::Interact(InteractState {
                    enabled: None,
                    has_interactors: Some(interactors),
                }),
            }],
        );
    }

    /// The members `changes` of the camp `camp` change: each goes to the viewers and into the
    /// group's state (over the member of its component, or added).
    fn change_camp_members(&mut self, camp: &CampEntity, changes: Vec<BundleMember>) {
        if changes.is_empty() {
            return;
        }
        let merged = changes.clone();
        self.stations.change_camp(
            camp.number,
            |members| {
                for change in merged {
                    put_member(members, change);
                }
            },
            changes,
        );
    }
}

/// Puts `member` into `members`: over the member of its component (its set fields over the
/// old ones for an interact), else added in component order.
fn put_member(members: &mut Vec<BundleMember>, member: BundleMember) {
    match members
        .iter_mut()
        .find(|existing| existing.component == member.component)
    {
        Some(existing) => match (&mut existing.state, member.state) {
            (MemberState::Interact(old), MemberState::Interact(new)) => {
                old.enabled = new.enabled.or(old.enabled);
                old.has_interactors = new.has_interactors.or(old.has_interactors);
            }
            (state, new) => *state = new,
        },
        None => {
            members.push(member);
            members.sort_by_key(|member| member.component);
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use nw_world::camps::CampVitals;
    use nw_world::stations::StationSlice;

    use super::*;

    /// Retail `Hivecamp` 1781367544743: the camp group's members (component, class and body
    /// bytes) in wire order, and its owner.
    const RETAIL_GROUP: [&str; 12] = [
        "01ba1801c705000001020206576f6f6454318a91442700000000000500ffffffff0000003300000000000007466c696e7454310c9280c800000000000100ffffffff00000033000000000000a2a98f40031badbdf501",
        "0d83310103000000",
        "070f01ff0000000000000000000b956388ac0fc1090000000000000000ff003f80000000003f8000004300001eaefb730001",
        "0483310103000000",
        "000a0103a0ba3a561e7e5ba0b3439694e4c33217000000026e5ac4808181beaff1f846dcd11b879d",
        "08b81701030000000034",
        "0e83310103000000",
        "03921b010201",
        "058b22010101",
        "06b22d010100",
        "0b913201ff33a71b99a90145c881c6a40d3e2c8d69094e57666f726576657200f83f09898a3ebebebf3d50d0d23f80000000c13e78f8fa3da0a0a13de0e0e23f8000002436666562363336372d393732662d343963642d623166622d3333396335626465626362310a48697665736861646f7702050000000000000000000f03010003",
        "020d0103458d535345e2779a1f9443efed",
    ];
    const RETAIL_OWNER: &str = "6feb6367-972f-49cd-b1fb-339c5bdebcb1";
    const RETAIL_NAME: &str = "Hiveshadow";
    /// The retail camp's `GdeRef`, list version and placement (2236 at 1781367544.661).
    pub(in crate::world) const RETAIL_CAMP: [u8; RAW_ID_LEN] = [
        0x6e, 0x5a, 0xc4, 0x80, 0x81, 0x81, 0xbe, 0xaf, 0xf1, 0xf8, 0x46, 0xdc, 0xd1, 0x1b, 0x87,
        0x9d,
    ];
    const RETAIL_LIST_VERSION: u64 = 2;
    pub(in crate::world) const RETAIL_AT: Position = Position {
        x: 4_522.415_5,
        y: 7_246.95,
        z: 159.065_1,
    };
    pub(in crate::world) const RETAIL_ROTATION: [u32; 4] = [0, 0, 0x3f5f_dd61, 0xbef8_5bf1];
    /// The server time (ms since 2000) of the camp's vitals block `0b956388ac0fc109`.
    const RETAIL_SERVER_MS: u64 = 834_682_744_600;
    const RETAIL_TIER: u32 = 5;
    /// The retail respawn place in the camp's 3652 element (`458d47c9 45e27ecd 431f43df`).
    pub(in crate::world) const RETAIL_SPAWN_POINT: [u32; 3] =
        [0x458d_47c9, 0x45e2_7ecd, 0x431f_43df];
    /// The largest difference from a retail float of the respawn point, metres.
    pub(in crate::world) const SPAWN_POINT_TOLERANCE_M: f32 = 0.01;

    /// The camp slice `campskin_metalfest02_a_pvp` (`camp-slices-25810808.json`).
    pub(in crate::world) fn pvp_slice() -> CampSlice {
        CampSlice {
            slice: "slices/structures/camps/campvariants/campskin_metalfest02_a_pvp.dynamicslice"
                .to_owned(),
            asset: [
                0xa0, 0xba, 0x3a, 0x56, 0x1e, 0x7e, 0x5b, 0xa0, 0xb3, 0x43, 0x96, 0x94, 0xe4, 0xc3,
                0x32, 0x17,
            ],
            asset_sub_id: 2,
            station: StationSlice {
                station_type: "Camp1".to_owned(),
                name: String::new(),
                buildable: Some((1, 3)),
                structure: "CampSkin_MetalFest02_A_PVP".to_owned(),
                ownership: Some((11, 5)),
                structure_name: String::new(),
                obstruction: Some((5, true)),
                territory: Some(3),
                grid: None,
                delayed_event: Some(4),
            },
            position: 2,
            interact: Some(6),
            vitals: Some(CampVitals {
                component: 7,
                row: "SiegeWeaponSmall".to_owned(),
            }),
            damage_receiver: Some(8),
            delayed_events: vec![4, 13, 14],
            spawn_point: [0.0, -1.7, 0.2],
        }
    }

    pub(in crate::world) fn costs() -> Vec<(String, u32)> {
        vec![("WoodT1".to_owned(), 5), ("FlintT1".to_owned(), 1)]
    }

    pub(in crate::world) fn retail_rotation() -> [f32; 4] {
        RETAIL_ROTATION.map(f32::from_bits)
    }

    fn retail_members() -> Vec<BundleMember> {
        let mut members: Vec<BundleMember> = RETAIL_GROUP
            .iter()
            .map(|bytes| {
                let bytes = hex::decode(bytes).expect("hex");
                let (member, read) = BundleMember::read_from(&bytes).expect("retail member");
                assert_eq!(read, bytes.len(), "the whole member reads");
                member
            })
            .collect();
        members.sort_by_key(|member| member.component);
        members
    }

    #[test]
    fn the_camp_group_holds_the_retail_members() {
        let costs = costs();
        let spawn = CampSpawn {
            gde_ref: RETAIL_CAMP,
            position: RETAIL_AT,
            rotation: retail_rotation(),
            tier: RETAIL_TIER,
            costs: &costs,
            character: Uuid::parse_str(RETAIL_OWNER).expect("uuid"),
            name: RETAIL_NAME,
            owner_pvp: true,
            server_ms: RETAIL_SERVER_MS,
            built: true,
        };
        let members = camp_members(&pvp_slice(), &spawn, RETAIL_LIST_VERSION);
        let retail = retail_members();
        let classes = |members: &[BundleMember]| -> Vec<(u64, u64)> {
            members
                .iter()
                .map(|member| (member.component, member.state.type_index()))
                .collect()
        };
        assert_eq!(classes(&members), classes(&retail));
        for (ours, theirs) in members.iter().zip(&retail) {
            match (&ours.state, &theirs.state) {
                (MemberState::Position(ours), MemberState::Position(theirs)) => {
                    let (ours_at, theirs_at) = (
                        ours.position.expect("ours"),
                        theirs.position.expect("retail"),
                    );
                    assert_eq!((ours_at.x, ours_at.y), (theirs_at.x, theirs_at.y));
                    // Whether retail rounds the height down or puts the camp on the ground is
                    // Unknown: its raw height is one step below the 2236 height's.
                    assert!(ours_at.z_raw.abs_diff(theirs_at.z_raw) <= 1);
                    assert_eq!(ours.rotation, theirs.rotation);
                }
                (MemberState::Ownership(ours), MemberState::Ownership(theirs)) => {
                    // The retail owner's guild, crest and faction are not this sim's.
                    assert_eq!(ours.owner, theirs.owner);
                    assert_eq!(
                        (
                            ours.permission_rule,
                            &ours.structure_name,
                            ours.raid_id,
                            ours.owner_pvp,
                            ours.owner_ffa,
                            ours.permission_options,
                        ),
                        (
                            theirs.permission_rule,
                            &theirs.structure_name,
                            theirs.raid_id,
                            theirs.owner_pvp,
                            theirs.owner_ffa,
                            theirs.permission_options,
                        )
                    );
                }
                (MemberState::Vitals(ours), MemberState::Vitals(theirs)) => {
                    // The retail block holds the placement's server time in ns; this server's
                    // clock counts ms.
                    let time = |state: &VitalsState| {
                        let mut slot = state.slot_0.expect("slot 0");
                        let data = slot.vitals_data.as_mut().expect("data");
                        data.a /= nw_messages::NS_PER_MS;
                        slot
                    };
                    assert_eq!(time(ours), time(theirs));
                }
                (
                    MemberState::BuildableController(ours),
                    MemberState::BuildableController(theirs),
                ) => {
                    // `Hivecamp` (2026-06) built its camps on its own and sent no foundation
                    // check; a camp built by its owner's commits sends it
                    // (`12a_msq_16_fasttravel` 67.55 s).
                    assert_eq!(
                        ours.check_foundation_events,
                        Some(COMMITTED_CHECK_FOUNDATION_EVENTS)
                    );
                    assert_eq!(
                        BuildableControllerState {
                            check_foundation_events: theirs.check_foundation_events,
                            ..ours.clone()
                        },
                        *theirs
                    );
                }
                (ours, theirs) => assert_eq!(ours, theirs),
            }
        }
    }

    #[test]
    fn the_respawn_point_is_the_slice_point_turned_by_the_camp() {
        let costs = costs();
        let spawn = CampSpawn {
            gde_ref: RETAIL_CAMP,
            position: RETAIL_AT,
            rotation: retail_rotation(),
            tier: RETAIL_TIER,
            costs: &costs,
            character: Uuid::nil(),
            name: "",
            owner_pvp: false,
            server_ms: 0,
            built: true,
        };
        let at = spawn.world_point(pvp_slice().spawn_point);
        let retail = RETAIL_SPAWN_POINT.map(f32::from_bits);
        for (ours, theirs) in [at.x, at.y, at.z].into_iter().zip(retail) {
            assert!(
                (ours - theirs).abs() <= SPAWN_POINT_TOLERANCE_M,
                "{ours} vs {theirs}"
            );
        }
        let camp = CampEntity {
            number: 0,
            gde_ref: RETAIL_CAMP,
            spawn_point: at,
        };
        assert_eq!(camp.facet(), 12_663_701_595_423_529_582);
    }

    /// Retail `12a_msq_12_camptutorial`: the buildable controller bodies (after the class
    /// bytes) of the first commit (287.054 s, list version 9, with the foundation check), the
    /// second (287.818 s, version 10) and the last, which adds `FlintT1` (290.886 s, version
    /// 14).
    const RETAIL_FIRST_COMMIT: &str =
        "0106010001090106576f6f6454318a91442700000000000100ffffffff00000033000000000000";
    const RETAIL_SECOND_COMMIT: &str =
        "010400010a0106576f6f6454318a91442700000000000200ffffffff00000033000000000000";
    const RETAIL_FLINT_COMMIT: &str =
        "0104010107466c696e745431010e0c9280c800000000000100ffffffff00000033000000000000";
    /// The list version of the retail camp before its first commit.
    const RETAIL_VERSION_BEFORE: u64 = 8;

    fn controller_body(state: BuildableControllerState) -> String {
        let mut writer = nw_messages::Writer::new();
        state.write(&mut writer).expect("write");
        hex::encode(writer.finish())
    }

    #[test]
    fn the_commits_send_the_retail_list_forms() {
        let mut build = CampBuild::placed(&costs(), RETAIL_VERSION_BEFORE, 0);
        let mut bodies = Vec::new();
        for _ in 0..6 {
            let item = build.next_item().expect("an item").to_owned();
            let first = build.committed.is_empty();
            build.commit(&item);
            bodies.push(controller_body(BuildableControllerState {
                check_foundation_events: first.then_some(COMMITTED_CHECK_FOUNDATION_EVENTS),
                committed_resources: Some(build.list_change(&item)),
                ..BuildableControllerState::default()
            }));
        }
        assert_eq!(build.next_item(), None, "every unit is in");
        assert_eq!(bodies[0], RETAIL_FIRST_COMMIT);
        assert_eq!(bodies[1], RETAIL_SECOND_COMMIT);
        assert_eq!(bodies[5], RETAIL_FLINT_COMMIT);
    }

    #[test]
    fn a_camp_id_has_the_dynamic_form() {
        let id = camp_id(1, 2);
        assert!(nw_messages::is_dynamic_raw_id(&id));
        assert_ne!(id, camp_id(1, 3));
    }
}

/// The camp rows and slices of the content of `local-data/content` (run `content-tool build` and
/// `content-tool camp-slices` first).
#[cfg(all(test, feature = "local-data"))]
mod retail_tests {
    use std::path::{Path, PathBuf};

    use nw_content::ContentBundle;
    use nw_content::bundle::bundle_path;
    use nw_content::camps::CampRules;
    use nw_content::client_build::load_client_build;
    use nw_content::crafting::StructureCosts;
    use nw_world::camps::{CampSlices, camp_slices_path};

    use super::tests::{RETAIL_AT, RETAIL_SPAWN_POINT, SPAWN_POINT_TOLERANCE_M, retail_rotation};
    use super::*;

    const CLIENT_BUILD_CONFIG: &str = "config/client-build.toml";
    /// `campt1.dynamicslice` and `campskin_metalfest02_a_pvp.dynamicslice` in the asset catalog
    /// (the second is the card asset of the retail `Hivecamp` camp).
    const CAMP_T1_ASSET: &str = "1b66134908d75b12963ab8e320c3342a";
    const PVP_SKIN_ASSET: &str = "a0ba3a561e7e5ba0b3439694e4c33217";
    /// `HP` of `CampT1` and of `CampT1_PVP` (`BlueprintItemDefinitions`).
    const CAMP_T1_HP: u32 = 300;
    const CAMP_T1_PVP_HP: u32 = 2_000;

    fn content_dir() -> (PathBuf, String) {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let build = load_client_build(&repo.join(CLIENT_BUILD_CONFIG)).expect("client build");
        (repo.join("local-data/content"), build)
    }

    #[test]
    fn the_retail_camp_t1_row_and_slices_load() {
        let (dir, build) = content_dir();
        let bundle = ContentBundle::load(&bundle_path(&dir, &build), &build).expect("bundle");
        let camps = CampRules::from_tables(&bundle.tables);
        let camp = camps.blueprint("CampT1").expect("CampT1");
        assert_eq!(
            (camp.hp, camp.pvp, camp.pvp_variant.as_deref()),
            (CAMP_T1_HP, false, Some("CampT1_PVP"))
        );
        assert_eq!(camps.placed(camp, true).hp, CAMP_T1_PVP_HP);
        let costs = StructureCosts::from_tables(&bundle.tables);
        assert_eq!(
            costs.of("CampT1"),
            [("WoodT1".to_owned(), 5), ("FlintT1".to_owned(), 1)]
        );
        let pvp = camps
            .blueprint_of_crc(0x89a7_af2d)
            .expect("the Hivecamp blueprint");
        assert_eq!(
            camps
                .skin_of(pvp)
                .map(|skin| (skin.id.as_str(), skin.enabled)),
            Some(("CampSkin_MetalFest02_A", true))
        );
        let slices = CampSlices::load(&camp_slices_path(&dir, &build)).expect("camp slices");
        let t1 = slices.get("CampT1").expect("CampT1 slice");
        assert_eq!(hex::encode(t1.asset), CAMP_T1_ASSET);
        assert_eq!(t1.station.ownership, Some((7, 1)));
        let slice = slices.get(&pvp.id).expect("PvP skin slice");
        assert_eq!(hex::encode(slice.asset), PVP_SKIN_ASSET);
        assert_eq!(slice.delayed_events, [4, 13, 14]);
        let spawn = CampSpawn {
            gde_ref: [0; RAW_ID_LEN],
            position: RETAIL_AT,
            rotation: retail_rotation(),
            tier: 5,
            costs: &[],
            character: Uuid::nil(),
            name: "",
            owner_pvp: true,
            server_ms: 0,
            built: true,
        };
        let at = spawn.world_point(slice.spawn_point);
        for (ours, theirs) in [at.x, at.y, at.z]
            .into_iter()
            .zip(RETAIL_SPAWN_POINT.map(f32::from_bits))
        {
            assert!((ours - theirs).abs() <= SPAWN_POINT_TOLERANCE_M);
        }
    }
}
