// SPDX-License-Identifier: AGPL-3.0-or-later
//! The siege structures of a war fort and their builds (siege structure study 2026-11-12; the
//! fort's structures and the structure kinds from [`nw_content::war_forts`], the build costs from
//! the blueprints, [`nw_content::war_forts::SiegeBlueprints`]).
//!
//! **The body of a structure** ([`StructureBody`], shared by the turrets of [`super::turrets`]
//! and the other structures): its vitals, its buildable controller with the committed
//! `SiegeSupply` of its health share, its interact, and its break at no health (the damage rules
//! are in the [`super::turrets`] module doc: Observed on the war turrets, Inferred the same for
//! every structure of the fort, as all of them carry the same `SiegeWeaponSmall` vitals row and
//! the same buildable controller).
//!
//! **The other structures** ([`StructureRun`]): the fire barrel droppers, horns of resilience and
//! supply generators retail showed in the frame spawn with the war's instance, each with its
//! retail first members and the ownership of its side's company (Observed: the T1 and T2
//! droppers of `joining_defending_war_2`, `CK_offensive_war`, the T3 droppers and horns of
//! `arvaysa_ES_attacker_war_45vs45` and `everfall_war_2` carry the same company ownership as the
//! fort's turrets). A supply generator names no company (Observed: the first ownership of every
//! `siege_supplygenerator_t1` of the five war ledgers with turret flows, `joining_defending_war_2`
//! 0.714 s, `everfall_war_2` 0.526 s, `nighthaven_defensive_war` 0.638 s, ES 929.552 s,
//! `CK_offensive_war` 946.444 s: the zero company id, no name, the white crest, no owner,
//! permission rule 4, raid 0, faction 0; [`generator_ownership`]). The uses of the droppers and
//! horns are in [`super::siege_areas`], the generators' supply in [`super::supply_generators`].
//!
//! **The war's run** (Observed, `joining_defending_war_2` 83.729 to 83.858 s, ES 925 s,
//! `CK_offensive_war` 942 s): at the start every turret, dropper and horn of the fort gets its
//! permission rule 4 ([`WAR_RUN_PERMISSION_RULE`]) and the raid id of its side (mask 0x50; the
//! defender's 536 on the 12 repeaters and 5 droppers of jdw2), and every generator the zero
//! company id again with the raid id (mask 0x41, rule 4 already); the cards sent while the war
//! runs carry both (jdw2 320.592 s, a generator with raid 536). At the result they go back
//! (ES 2141.7 s, `everfall_war_2` 258 s, jdw2 1883.719 s): rule 3 and raid 0 for the turrets,
//! droppers and horns, the zero company id and raid 0 for a generator
//! ([`SimWorld::show_war_run_ownership`]). Retail sent each change again in the next ticks
//! (jdw2: 5 to 7 times over 130 ms; Inferred: its replication's resend, the sim's channel
//! sends once). The fort gates and war camp gates of retail also change (rule 4 with the
//! defender's faction, Observed mask 0xd0), but the sim holds no gate.
//!
//! **A build** (Observed, `fort-equipements-territory`, 17 calls, one player of the fort's
//! company in the territory's open-world fort): the client's interaction of kind
//! [`OPEN_SELECTION_KIND`] on a defensive structure is accepted (3743, about 32 ms later), the
//! client sends the 2646 with its choice and cancels the interaction (2473) about 235 ms later.
//! A choice of another kind stops the old entity's slot and a new entity of the chosen slice
//! (a new `GdeRef`, the same place and facing) comes in the next frame, 0.80 to 1.94 s after the
//! call (26.024 -> 27.960 s, 32.359 -> 33.192 s, 37.691 -> 38.693 s, 41.957 -> 42.791 s,
//! 47.325 -> 48.123 s, 80.321 -> 81.437 s, 92.219 -> 93.322 s), whole (buildable state 5, full
//! vitals) with every member of its slice and the company's ownership; a choice of the kind
//! already there changes nothing (9 calls). Retail sent no answer to any 2646, and no 5361
//! (`OnCompletionBlockedStatusChanged`): the new structure is built at once. No bag, coin or
//! currency change of the player came with a build in the open world. Inferred: in a war the
//! build costs its blueprint's `SiegeSupply` (`StructureCost_Qty1`: 20, 60 or 120 by tier) from
//! the builder's bag, as the blueprints say and as the supply generators fill bags with it; the
//! new structure comes [`BUILD_DELAY_MS`] after the call (the middle of the Observed times); only
//! a player of the structure's side builds, never on a broken structure (its interact is off), and
//! one build at a time per structure; a refused build gets no answer, as no build did. A kind
//! with no retail template, or a turret kind whose shot the table does not hold
//! ([`nw_content::war_forts::WarForts::buildable`]), is not built.

use bevy_ecs::entity::Entity;
use nw_bus::ConnectionRef;
use nw_content::war_forts::{
    FortStructure, SIDE_ATTACKER, SIDE_DEFENDER, SIEGE_SUPPLY, StructureKind, StructureRole,
};
use nw_messages::buildables::{CommittedResources, GUILD_ID_LEN, GuildCrest, ResourceItem};
use nw_messages::item_descriptor::ItemDescriptor;
use nw_messages::items::SYNC_CRAFT_USED;
use nw_messages::keyed_list::KeyedList;
use nw_messages::position::POSITION_IN_THE_WORLD_TYPE_INDEX;
use nw_messages::turrets::BuildDefensiveStructureMsg;
use nw_messages::{
    AssetId, BUILDABLE_CONTROLLER_TYPE_INDEX, BuildableControllerState, BundleMember, ClaimOwner,
    DYNAMIC_SLICE_SUB_ID, FIRST_DAMAGE_FLAGS, GdeMetadataReplicatedState, INTERACT_TYPE_INDEX,
    InteractState, InteractionRequest, MemberState, OwnershipState, VITALS_TYPE_INDEX, VitalsData,
    VitalsSlot0, VitalsState,
};
use nw_sim_core::Position;
use uuid::Uuid;

use super::crafting::{Station, dynamic_ref};
use super::siege_areas::{UseState, apply_ownership_change};
use super::supply_generators::GeneratorRun;
use super::turrets::{USE_ACCEPTED, USE_REFUSED, turret_ownership};
use super::war_fight::{WarFight, replace_member, side_numbers};
use super::{Player, SimWorld, gather_answer, send_now};

/// The interaction kind that opens a structure's build choices (`InteractionUIActions`
/// `OpenTurretSelection`, 0x3e; Observed on every build of `fort-equipements-territory`).
pub(super) const OPEN_SELECTION_KIND: u8 = 62;
/// How long after a 2646 the new structure comes, ms (Inferred, module doc).
pub(super) const BUILD_DELAY_MS: u64 = 1_000;
/// The namespace of the `GdeRef`s of built structures.
const BUILD_NAMESPACE: u64 = 0x5345_4745_4255_494c;
/// The buildable state of a whole structure and of a broken one, and the ruin damage type of a
/// broken one (Observed on the war turrets, [`super::turrets`] module doc).
pub(super) const BUILT_STATE: u8 = 5;
pub(super) const BROKEN_STATE: u8 = 6;
pub(super) const BROKEN_RUIN_TYPE: u8 = 10;
/// The `invulnerability` of a structure open to damage and of one closed to it (Observed).
pub(super) const VULNERABLE: u8 = 0;
pub(super) const INVULNERABLE: u8 = 1;
/// The `healthChangeFlags` of a hit on a structure (Observed: 3 with each turret hit).
const HIT_FLAGS: u8 = FIRST_DAMAGE_FLAGS;
/// The interactors of a structure nobody uses.
pub(super) const NO_INTERACTORS: u32 = 0;
/// The permission rule of a fort structure while its war runs, and of a supply generator
/// always (Observed, module doc, **The war's run**).
pub(super) const WAR_RUN_PERMISSION_RULE: u8 = 4;
/// The raid id of a fort structure outside its war's run (Observed, module doc).
const NO_WAR_RAID: u64 = 0;
/// The company id of a structure no company owns (Observed: 16 zero bytes on the generators).
const NO_COMPANY: [u8; GUILD_ID_LEN] = [0; GUILD_ID_LEN];

/// A structure's health share of its committed resource ([`super::turrets`] module doc).
#[derive(Debug, Clone, PartialEq)]
struct Committed {
    /// The resource and its descriptor in the structure's first buildable state.
    item: ResourceItem,
    descriptor: ItemDescriptor,
    /// Its quantity when the structure is whole, and the quantity last sent.
    whole: u32,
    shown: u32,
    /// The version of the list last sent.
    version: u64,
}

/// What a hit did to a structure.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Hurt {
    /// It took the hit: the changed members.
    Hit(Vec<BundleMember>),
    /// It broke at no health: the changed members.
    Broke(Vec<BundleMember>),
}

/// The vitals, buildable and interact parts of one siege structure (module doc).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StructureBody {
    /// The structure as the fort or the build made it.
    pub(super) structure: FortStructure,
    /// Its slot key.
    pub(super) number: u64,
    /// The enabled child of its retail first interact.
    enabled: Option<bool>,
    /// Its health now and at most.
    pub(super) health: f32,
    pub(super) max_health: f32,
    /// Its retail first vitals and buildable members (the base of each full state it sends).
    vitals: Option<(u64, VitalsState)>,
    buildable: Option<(u64, BuildableControllerState)>,
    committed: Option<Committed>,
    /// Hits do not hurt it (before the start and from the result).
    pub(super) invulnerable: bool,
    /// It broke at 0 health.
    pub(super) broken: bool,
}

impl StructureBody {
    /// `structure` as it spawns: whole, invulnerable, with the interact, the vitals and the
    /// buildable state of its retail first members.
    pub(super) fn new(structure: FortStructure) -> Self {
        let vitals = retail_member(&structure, VITALS_TYPE_INDEX).and_then(|member| match member {
            BundleMember {
                component,
                state: MemberState::Vitals(state),
            } => Some((component, state)),
            _ => None,
        });
        let buildable =
            retail_member(&structure, BUILDABLE_CONTROLLER_TYPE_INDEX).and_then(|member| {
                match member {
                    BundleMember {
                        component,
                        state: MemberState::BuildableController(state),
                    } => Some((component, state)),
                    _ => None,
                }
            });
        let enabled = match retail_member(&structure, INTERACT_TYPE_INDEX) {
            Some(BundleMember {
                state: MemberState::Interact(state),
                ..
            }) => state.enabled,
            _ => None,
        };
        let committed = buildable.as_ref().and_then(|(_, state)| {
            let Some(KeyedList::Full(list)) = &state.committed_resources else {
                return None;
            };
            let (item, descriptor) = list.elements.first()?.clone();
            Some(Committed {
                whole: descriptor.quantity,
                shown: descriptor.quantity,
                version: list.version.unwrap_or_default(),
                item,
                descriptor,
            })
        });
        let max_health = vitals
            .as_ref()
            .and_then(|(_, state)| state.slot_0)
            .and_then(|slot| slot.health_max.or(slot.health_amount))
            .unwrap_or_default();
        Self {
            structure,
            number: 0,
            enabled,
            health: max_health,
            max_health,
            vitals,
            buildable,
            committed,
            invulnerable: true,
            broken: false,
        }
    }

    /// Its vitals block as it is now: the retail first block, dead while it is broken.
    pub(super) fn vitals_data(&self) -> Option<VitalsData> {
        let data = self.vitals.as_ref()?.1.slot_0?.vitals_data?;
        Some(if self.broken {
            data.broken()
        } else {
            data.whole()
        })
    }

    const fn invulnerability(&self) -> u8 {
        if self.invulnerable {
            INVULNERABLE
        } else {
            VULNERABLE
        }
    }

    /// Its full vitals member now.
    pub(super) fn vitals_member(&self) -> Option<BundleMember> {
        let (component, state) = self.vitals.as_ref()?;
        let slot = state.slot_0.unwrap_or_default();
        Some(BundleMember {
            component: *component,
            state: MemberState::Vitals(VitalsState {
                slot_0: Some(VitalsSlot0 {
                    health_amount: Some(self.health),
                    vitals_data: self.vitals_data(),
                    invulnerability: Some(self.invulnerability()),
                    ..slot
                }),
                ..state.clone()
            }),
        })
    }

    /// A vitals change of `slot` alone.
    pub(super) fn vitals_change(&self, slot: VitalsSlot0) -> Option<BundleMember> {
        Some(BundleMember {
            component: self.vitals.as_ref()?.0,
            state: MemberState::Vitals(VitalsState {
                slot_0: Some(slot),
                slot_1: None,
            }),
        })
    }

    /// The quantity of its committed resource its health stands for.
    fn committed_share(&self) -> u32 {
        let Some(committed) = &self.committed else {
            return 0;
        };
        if self.broken || self.max_health <= 0.0 {
            return 0;
        }
        // At most the whole cost, a few hundred.
        (committed.whole as f32 * self.health / self.max_health).ceil() as u32
    }

    /// Its committed resources at the version last sent.
    pub(super) fn committed_list(&self) -> Option<CommittedResources> {
        let committed = self.committed.as_ref()?;
        let entries = if committed.shown == 0 {
            Vec::new()
        } else {
            vec![(
                committed.item.clone(),
                ItemDescriptor {
                    quantity: committed.shown,
                    ..committed.descriptor.clone()
                },
            )]
        };
        Some(KeyedList::full(committed.version, entries))
    }

    /// The committed resources of its health share under a new version when the share changed.
    pub(super) fn next_committed(&mut self) -> Option<CommittedResources> {
        let share = self.committed_share();
        let committed = self.committed.as_mut()?;
        if committed.shown == share {
            return None;
        }
        committed.shown = share;
        committed.version = committed.version.saturating_add(1);
        self.committed_list()
    }

    /// Its full buildable member now.
    pub(super) fn buildable_member(&self) -> Option<BundleMember> {
        let (component, state) = self.buildable.as_ref()?;
        Some(BundleMember {
            component: *component,
            state: MemberState::BuildableController(BuildableControllerState {
                current_state: Some(if self.broken {
                    BROKEN_STATE
                } else {
                    BUILT_STATE
                }),
                committed_resources: self.committed_list(),
                ruin_damage_type: if self.broken {
                    Some(BROKEN_RUIN_TYPE)
                } else {
                    state.ruin_damage_type
                },
                ..state.clone()
            }),
        })
    }

    /// A buildable change of `state` alone.
    pub(super) fn buildable_change(&self, state: BuildableControllerState) -> Option<BundleMember> {
        Some(BundleMember {
            component: self.buildable.as_ref()?.0,
            state: MemberState::BuildableController(state),
        })
    }

    /// Its interact member with `interactors`: disabled while it is broken.
    pub(super) fn interact_member(&self, interactors: u32) -> Option<BundleMember> {
        Some(BundleMember {
            component: self.structure.interact_component?,
            state: MemberState::Interact(InteractState {
                enabled: if self.broken {
                    Some(false)
                } else {
                    self.enabled
                },
                has_interactors: Some(interactors),
            }),
        })
    }

    /// Whether a hit of `amount` would break it.
    pub(super) fn breaks_at(&self, amount: u32) -> bool {
        !self.invulnerable && !self.broken && amount > 0 && self.health <= amount as f32
    }

    /// It takes `amount` health ([`super::turrets`] module doc): the changed members of the hit
    /// or of its break at 0; `None` when the hit does nothing (invulnerable, broken, 0).
    pub(super) fn take(&mut self, amount: u32) -> Option<Hurt> {
        if self.invulnerable || self.broken || amount == 0 {
            return None;
        }
        // A hit of a few thousand points at most.
        self.health = (self.health - amount as f32).max(0.0);
        if self.health > 0.0 {
            let mut partial: Vec<BundleMember> = self
                .vitals_change(VitalsSlot0 {
                    health_amount: Some(self.health),
                    health_change_flags: Some(HIT_FLAGS),
                    ..VitalsSlot0::default()
                })
                .into_iter()
                .collect();
            if let Some(list) = self.next_committed() {
                partial.extend(self.buildable_change(BuildableControllerState {
                    committed_resources: Some(list),
                    ..BuildableControllerState::default()
                }));
            }
            return Some(Hurt::Hit(partial));
        }
        self.broken = true;
        let committed = self.next_committed();
        let partial = [
            self.vitals_change(VitalsSlot0 {
                health_amount: Some(0.0),
                vitals_data: self.vitals_data(),
                ..VitalsSlot0::default()
            }),
            self.buildable_change(BuildableControllerState {
                current_state: Some(BROKEN_STATE),
                committed_resources: committed,
                ruin_damage_type: Some(BROKEN_RUIN_TYPE),
                ..BuildableControllerState::default()
            }),
            self.structure
                .interact_component
                .map(|component| BundleMember {
                    component,
                    state: MemberState::Interact(InteractState {
                        enabled: Some(false),
                        has_interactors: None,
                    }),
                }),
        ];
        Some(Hurt::Broke(partial.into_iter().flatten().collect()))
    }

    /// It gets `amount` health back up to its most: the changed members.
    pub(super) fn repair(&mut self, amount: f32) -> Vec<BundleMember> {
        self.health = (self.health + amount).min(self.max_health);
        let mut partial: Vec<BundleMember> = self
            .vitals_change(VitalsSlot0 {
                health_amount: Some(self.health),
                ..VitalsSlot0::default()
            })
            .into_iter()
            .collect();
        if let Some(list) = self.next_committed() {
            partial.extend(self.buildable_change(BuildableControllerState {
                committed_resources: Some(list),
                ..BuildableControllerState::default()
            }));
        }
        partial
    }

    /// The war started: it may be hurt; the changed members.
    pub(super) fn open_to_damage(&mut self) -> Vec<BundleMember> {
        self.invulnerable = false;
        self.vitals_change(VitalsSlot0 {
            invulnerability: Some(VULNERABLE),
            ..VitalsSlot0::default()
        })
        .into_iter()
        .collect()
    }

    /// The war has its result: it is whole and invulnerable again; the changed members.
    pub(super) fn restore(&mut self) -> Vec<BundleMember> {
        let was_broken = self.broken;
        self.broken = false;
        self.invulnerable = true;
        self.health = self.max_health;
        let committed = self.next_committed();
        let mut partial: Vec<BundleMember> = self
            .vitals_change(VitalsSlot0 {
                health_amount: Some(self.health),
                vitals_data: was_broken.then(|| self.vitals_data()).flatten(),
                invulnerability: Some(INVULNERABLE),
                ..VitalsSlot0::default()
            })
            .into_iter()
            .collect();
        if was_broken || committed.is_some() {
            partial.extend(self.buildable_change(BuildableControllerState {
                current_state: was_broken.then_some(BUILT_STATE),
                committed_resources: committed.or_else(|| self.committed_list()),
                ..BuildableControllerState::default()
            }));
        }
        if was_broken {
            partial.extend(self.interact_member(NO_INTERACTORS));
        }
        partial
    }

    /// Its place.
    pub(super) const fn place(&self) -> Position {
        let [x, y, z] = self.structure.place;
        Position { x, y, z }
    }
}

/// The ownership of a supply generator outside the war's run (Observed, module doc): no
/// company, no owner, [`WAR_RUN_PERMISSION_RULE`], raid 0, faction 0.
pub(super) fn generator_ownership() -> OwnershipState {
    OwnershipState {
        permission_rule: Some(WAR_RUN_PERMISSION_RULE),
        ..turret_ownership(&ClaimOwner::NONE)
    }
}

/// The part of `shown`, a fort structure's ownership at the war's start or result, that retail
/// sent then (Observed, module doc, **The war's run**): the zero company id of a structure whose
/// `resting` ownership names no company, the permission rule of one whose `resting` rule is not
/// [`WAR_RUN_PERMISSION_RULE`], and the raid id.
pub(super) fn war_run_change(resting: &OwnershipState, shown: &OwnershipState) -> OwnershipState {
    OwnershipState {
        guild_id: resting.guild_id.filter(|id| *id == NO_COMPANY),
        permission_rule: shown
            .permission_rule
            .filter(|_| resting.permission_rule != Some(WAR_RUN_PERMISSION_RULE)),
        raid_id: shown.raid_id,
        ..OwnershipState::default()
    }
}

/// The first retail member of class `class` of `structure`.
fn retail_member(structure: &FortStructure, class: u64) -> Option<BundleMember> {
    structure.members.iter().find_map(|bytes| {
        let (member, _) = BundleMember::read_from(bytes).ok()?;
        (member.state.type_index() == class).then_some(member)
    })
}

/// The members of `body` at its spawn: its card, its retail members with the vitals, buildable
/// and interact states of `body` and those `own` gives (a turret's), its slice's use interact
/// when no retail member holds it (the use and its cooldown show there), and `ownership`.
pub(super) fn spawn_members(
    body: &StructureBody,
    own: impl Fn(&BundleMember) -> Option<BundleMember>,
    ownership: OwnershipState,
) -> Vec<BundleMember> {
    let structure = &body.structure;
    let card = BundleMember {
        component: structure.card_component,
        state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
            asset: Some(AssetId {
                id: structure.asset,
                sub_id: DYNAMIC_SLICE_SUB_ID,
            }),
            gde_ref: Some(structure.gde_ref),
        }),
    };
    let mut members = vec![card];
    for bytes in &structure.members {
        let Ok((member, _)) = BundleMember::read_from(bytes) else {
            tracing::warn!(structure = ?structure.gde_ref, "structure member does not read");
            continue;
        };
        let class = member.state.type_index();
        let state = if let Some(own) = own(&member) {
            own
        } else if class == VITALS_TYPE_INDEX {
            body.vitals_member().unwrap_or(member)
        } else if class == BUILDABLE_CONTROLLER_TYPE_INDEX {
            body.buildable_member().unwrap_or(member)
        } else if Some(member.component) == structure.interact_component {
            body.interact_member(NO_INTERACTORS).unwrap_or(member)
        } else {
            member
        };
        members.push(state);
    }
    let retail_interact = members
        .iter()
        .any(|member| Some(member.component) == structure.interact_component);
    if !retail_interact {
        members.extend(body.interact_member(NO_INTERACTORS));
    }
    if let Some(component) = structure.ownership_component {
        members.push(BundleMember {
            component,
            state: MemberState::Ownership(ownership),
        });
    }
    members
}

/// One siege structure of the fort that is not a turret (module doc).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StructureRun {
    pub(super) body: StructureBody,
    /// Its trigger use ([`super::siege_areas`]).
    pub(super) uses: UseState,
    /// Its supply generation, for a supply generator ([`super::supply_generators`]).
    pub(super) generation: Option<GeneratorRun>,
}

impl StructureRun {
    /// The vat of an Outpost Rush outpost structure (`structure`, at slot key `number`) as its
    /// build makes it ([`super::siege_areas`] module doc): free and open to damage; its vitals
    /// and buildable states are the outpost structure's ([`super::outpost_rush_structures`]),
    /// so `structure` carries none.
    pub(super) fn outpost_dropper(structure: FortStructure, number: u64) -> Self {
        let mut body = StructureBody::new(structure);
        body.number = number;
        body.invulnerable = false;
        Self {
            body,
            uses: UseState::default(),
            generation: None,
        }
    }

    /// Its full members that change: its interact, vitals and buildable states.
    fn full_members(&self) -> Vec<BundleMember> {
        self.resting_interact()
            .into_iter()
            .chain(self.body.vitals_member())
            .chain(self.body.buildable_member())
            .collect()
    }
}

/// A build that waits for its time (module doc).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PendingBuild {
    /// Sim ms the new structure comes.
    at_ms: u64,
    /// The `GdeRef` of the structure it replaces.
    old: [u8; nw_messages::RAW_ID_LEN],
    /// The kind it builds.
    kind: StructureKind,
}

/// Where a structure of a fort is in the fight's lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placed {
    Turret(usize),
    Other(usize),
}

/// `structure` of `kind` as a build makes it in the place of `old` with `gde_ref`: the kind's
/// template members with the old structure's place member, its place, facing and side.
fn built_structure(
    kind: &StructureKind,
    template: &FortStructure,
    old: &FortStructure,
    gde_ref: [u8; nw_messages::RAW_ID_LEN],
) -> FortStructure {
    let place_member = old.members.iter().find(|bytes| {
        BundleMember::read_from(bytes)
            .is_ok_and(|(member, _)| member.state.type_index() == POSITION_IN_THE_WORLD_TYPE_INDEX)
    });
    let members = template
        .members
        .iter()
        .map(|bytes| {
            let is_place = BundleMember::read_from(bytes).is_ok_and(|(member, _)| {
                member.state.type_index() == POSITION_IN_THE_WORLD_TYPE_INDEX
            });
            match place_member {
                Some(place) if is_place => place.clone(),
                _ => bytes.clone(),
            }
        })
        .collect();
    FortStructure {
        asset: kind.asset,
        gde_ref,
        place: old.place,
        yaw: old.yaw,
        side: old.side,
        members,
        ..template.clone()
    }
}

impl SimWorld {
    /// The ownership of a structure of `side` of `fight`'s war: its side company's (Observed,
    /// [`super::turrets`] module doc; `joining_defending_war_2` 125.039 s: the barrel of a use
    /// names the side's company, not the user's), with its name and crest from its claims or the
    /// world database ([`super::company_looks`]; the fight asks for both sides' companies), and
    /// while the war runs its rule and its side's raid id (module doc, **The war's run**).
    pub(super) fn structure_ownership(&self, fight: &WarFight, side: u8) -> OwnershipState {
        self.in_war_run(fight, side, self.side_resting(fight, side))
    }

    /// The ownership of a structure of `side` of `fight`'s war outside the war's run: its side
    /// company's with permission rule 3 and raid 0 ([`turret_ownership`]).
    fn side_resting(&self, fight: &WarFight, side: u8) -> OwnershipState {
        let company = self.side_company(fight, side);
        let look = self.company_look(*company.as_bytes());
        turret_ownership(&ClaimOwner {
            guild_id: *company.as_bytes(),
            guild_name: look.as_ref().map(|look| look.name.clone()).unwrap_or_default(),
            crest: look.map_or(GuildCrest::NONE, |look| look.crest),
            ..ClaimOwner::NONE
        })
    }

    /// The ownership of `structure` of `fight` outside the war's run: a supply generator's
    /// names no company ([`generator_ownership`]), every other structure's names its side's
    /// company ([`Self::side_resting`]).
    fn resting_ownership(&self, fight: &WarFight, structure: &FortStructure) -> OwnershipState {
        if self.is_generator(structure) {
            generator_ownership()
        } else {
            self.side_resting(fight, structure.side)
        }
    }

    /// The ownership of `structure` of `fight` as it is now ([`Self::resting_ownership`], with
    /// the war's run values while it runs).
    pub(super) fn fort_structure_ownership(
        &self,
        fight: &WarFight,
        structure: &FortStructure,
    ) -> OwnershipState {
        self.in_war_run(fight, structure.side, self.resting_ownership(fight, structure))
    }

    /// `resting`, an ownership of a structure of `side` outside the war's run, as it is now:
    /// with [`WAR_RUN_PERMISSION_RULE`] and the side's raid id while `fight` runs.
    fn in_war_run(&self, fight: &WarFight, side: u8, resting: OwnershipState) -> OwnershipState {
        if !fight.running() {
            return resting;
        }
        OwnershipState {
            permission_rule: Some(WAR_RUN_PERMISSION_RULE),
            raid_id: Some(self.side_raid(fight, side)),
            ..resting
        }
    }

    /// Whether `structure` is a supply generator.
    fn is_generator(&self, structure: &FortStructure) -> bool {
        self.content
            .war_forts
            .structure_kind(&structure.asset)
            .is_some_and(|kind| kind.role == StructureRole::SupplyGenerator)
    }

    /// The company of `side` of `fight`'s war; the nil id when the war is not known.
    fn side_company(&self, fight: &WarFight, side: u8) -> Uuid {
        self.wars
            .war(fight.war().war)
            .map(|war| {
                if side == SIDE_ATTACKER {
                    war.attacker.0
                } else {
                    war.defender.0
                }
            })
            .unwrap_or_default()
    }

    /// The raid id of `side` of `fight`'s war: its war side number ([`side_numbers`]);
    /// [`NO_WAR_RAID`] when the war is not known.
    fn side_raid(&self, fight: &WarFight, side: u8) -> u64 {
        self.wars
            .war(fight.war().war)
            .map_or(NO_WAR_RAID, |war| {
                let (attacker, defender) = side_numbers(war);
                if side == SIDE_ATTACKER {
                    attacker
                } else {
                    defender
                }
            })
    }

    /// Every turret and structure of `fight` shows the war's start or result (module doc,
    /// **The war's run**; `fight` already runs, or already has its result): the change of
    /// [`war_run_change`] goes to its viewers and into its full ownership, which keeps the rest
    /// (a used turret's user too).
    pub(super) fn show_war_run_ownership(&mut self, fight: &WarFight) {
        let turrets = fight.turrets.iter().map(|run| &run.body);
        let structures = fight.structures.iter().map(|run| &run.body);
        for body in turrets.chain(structures) {
            let Some(component) = body.structure.ownership_component else {
                continue;
            };
            let resting = self.resting_ownership(fight, &body.structure);
            let shown = self.in_war_run(fight, body.structure.side, resting.clone());
            let change = BundleMember {
                component,
                state: MemberState::Ownership(war_run_change(&resting, &shown)),
            };
            let full = change.clone();
            self.stations.change_camp(
                body.number,
                |members| apply_ownership_change(members, &full),
                vec![change],
            );
        }
    }

    /// The structures and turrets of the war fight's sides whose company is one of `read` show
    /// their side company's ownership again, with the look the world database just gave
    /// ([`super::company_looks`]: they spawn at the fight's first tick, before the answer). A
    /// turret or a structure that was used keeps its own: it names its user (Observed: the full
    /// ownership keeps the user after the use, `joining_defending_war_2` 165.349 s). A supply
    /// generator names no company (module doc), so it has no look to show.
    pub(super) fn show_side_looks(&mut self, read: &[Uuid]) {
        self.with_turrets(|world, fight| {
            for side in [SIDE_ATTACKER, SIDE_DEFENDER] {
                if !read.contains(&world.side_company(fight, side)) {
                    continue;
                }
                let ownership = world.structure_ownership(fight, side);
                let turrets = fight
                    .turrets
                    .iter()
                    .filter(|run| !run.used())
                    .map(|run| &run.body);
                let structures = fight
                    .structures
                    .iter()
                    .filter(|run| !run.uses.used())
                    .map(|run| &run.body);
                let shown: Vec<(u64, u64)> = turrets
                    .chain(structures)
                    .filter(|body| {
                        body.structure.side == side && !world.is_generator(&body.structure)
                    })
                    .filter_map(|body| Some((body.number, body.structure.ownership_component?)))
                    .collect();
                for (number, component) in shown {
                    let member = BundleMember {
                        component,
                        state: MemberState::Ownership(ownership.clone()),
                    };
                    let full = member.clone();
                    world.stations.change_camp(
                        number,
                        |members| replace_member(members, &full),
                        vec![member],
                    );
                }
            }
        });
    }

    /// Spawns `structure`, a fort structure that is not a turret, in `fight` (module doc). A
    /// dropper whose first retail state had no interact uses its slice's use interact
    /// ([`nw_content::war_forts::StructureKind::use_interact`]; Observed in the forts of the
    /// table: territories 2 and 5 each hold a dropper whose first state had interact 17, the
    /// slice's, and one whose first state had none; territory 10 one with none).
    pub(super) fn spawn_structure(&mut self, fight: &mut WarFight, structure: FortStructure) {
        let content = std::sync::Arc::clone(&self.content);
        let kind = content.war_forts.structure_kind(&structure.asset);
        let structure = FortStructure {
            interact_component: structure
                .interact_component
                .or(kind.and_then(|kind| kind.use_interact)),
            ..structure
        };
        let mut body = StructureBody::new(structure);
        body.invulnerable = !fight.running();
        let ownership = self.fort_structure_ownership(fight, &body.structure);
        let mut members = spawn_members(&body, |_| None, ownership);
        let generator_tier = kind
            .filter(|kind| kind.role == StructureRole::SupplyGenerator)
            .map(|kind| kind.tier);
        let generation = generator_tier
            .and_then(|tier| self.spawn_generator(&body.structure, tier, &mut members));
        let gde_ref = body.structure.gde_ref;
        let position = body.place();
        body.number = self.stations.place_camp(|_| Station {
            gde_ref,
            position,
            station_type: String::new(),
            name: Vec::new(),
            members,
        });
        fight.structures.push(StructureRun {
            body,
            uses: UseState::default(),
            generation,
        });
    }

    /// Sends the full state of the structure `run` to its spawn members and `partial` to its
    /// viewers.
    pub(super) fn sync_structure(&mut self, run: &StructureRun, partial: Vec<BundleMember>) {
        let full = run.full_members();
        self.stations.change_camp(
            run.body.number,
            |members| {
                for member in &full {
                    replace_member(members, member);
                }
            },
            partial,
        );
    }

    /// The 2646 of the client of `connection` at sim ms `now_ms` (module doc): a build of the
    /// chosen structure in the place of the named one.
    pub fn build_defensive_structure(
        &mut self,
        connection: &ConnectionRef,
        call: &BuildDefensiveStructureMsg,
        now_ms: u64,
    ) {
        let Some(&entity) = self.by_connection.get(connection) else {
            return;
        };
        let side = self.war_side(entity);
        let content = std::sync::Arc::clone(&self.content);
        let target = call.structure_id();
        let built = self.with_turrets(|world, fight| {
            let old = match fight.placed(&target)? {
                Placed::Turret(index) => &fight.turrets[index].body,
                Placed::Other(index) => &fight.structures[index].body,
            };
            if side != Some(old.structure.side) || old.broken || fight.building(&target) {
                return None;
            }
            let kind = content
                .war_forts
                .replacement(&old.structure.asset, call.choice)?;
            if kind.asset == old.structure.asset {
                return None;
            }
            if !content.war_forts.buildable(kind) {
                tracing::warn!(slice = %kind.slice, "the chosen structure has no template or shot");
                return None;
            }
            let cost = content.siege_blueprints.cost(kind)?;
            if !world.spend_siege_supply(entity, cost) {
                return None;
            }
            fight.builds.push(PendingBuild {
                at_ms: now_ms.saturating_add(BUILD_DELAY_MS),
                old: target,
                kind: kind.clone(),
            });
            Some(kind.slice.clone())
        });
        match built.flatten() {
            Some(slice) => tracing::info!(%slice, "defensive structure build"),
            None => tracing::debug!(choice = call.choice, "defensive structure build refused"),
        }
    }

    /// The interaction `request` of kind [`OPEN_SELECTION_KIND`] of player `entity` (`number`)
    /// on a structure of this sim's war (module doc): accepted (3743) for a player of the
    /// structure's side on a whole structure, else refused (Inferred, the status of a refused
    /// turret use); `false` when it names no structure.
    pub(super) fn open_structure_selection(
        &mut self,
        entity: Entity,
        number: u64,
        request: InteractionRequest,
        name: &[u8],
    ) -> bool {
        let side = self.war_side(entity);
        let Some(connection) = self
            .world
            .get::<Player>(entity)
            .map(|player| player.connection.clone())
        else {
            return false;
        };
        let status = self.war_fight().and_then(|fight| {
            let body = match fight.placed(&request.target)? {
                Placed::Turret(index) => &fight.turrets[index].body,
                Placed::Other(index) => &fight.structures[index].body,
            };
            Some(if side == Some(body.structure.side) && !body.broken {
                USE_ACCEPTED
            } else {
                USE_REFUSED
            })
        });
        let Some(status) = status else {
            return false;
        };
        let answer = gather_answer(number, request, status, name);
        send_now(&mut self.combat_messages, &connection, &[answer]);
        true
    }

    /// Player `entity` pays `cost` `SiegeSupply` from its bag; whether it held that many.
    fn spend_siege_supply(&mut self, entity: Entity, cost: u32) -> bool {
        self.world
            .get_mut::<Player>(entity)
            .and_then(|mut player| {
                Some(
                    player
                        .gear
                        .as_mut()?
                        .take_as(SIEGE_SUPPLY, cost, SYNC_CRAFT_USED),
                )
            })
            .unwrap_or(false)
    }

    /// The builds of `fight` whose time came at sim ms `now_ms` (module doc): the old structure
    /// leaves and the new one comes in its place.
    pub(super) fn tick_siege_builds(&mut self, fight: &mut WarFight, now_ms: u64) {
        let (due, waiting): (Vec<PendingBuild>, Vec<PendingBuild>) =
            std::mem::take(&mut fight.builds)
                .into_iter()
                .partition(|build| now_ms >= build.at_ms);
        fight.builds = waiting;
        for build in due {
            self.replace_structure(fight, &build);
        }
    }

    fn replace_structure(&mut self, fight: &mut WarFight, build: &PendingBuild) {
        let (Some(placed), Some(template)) = (fight.placed(&build.old), &build.kind.template)
        else {
            return;
        };
        let old = match placed {
            Placed::Turret(index) => {
                let mut run = fight.turrets.remove(index);
                if run.in_use() {
                    self.release_turret(&mut run);
                }
                run.body
            }
            Placed::Other(index) => fight.structures.remove(index).body,
        };
        self.stations.remove_camp(old.number);
        let old = old.structure;
        let (war_high, _) = fight.war().war.as_u64_pair();
        let namespace = BUILD_NAMESPACE ^ self.me.generation ^ war_high;
        let gde_ref = dynamic_ref(namespace, fight.built);
        fight.built = fight.built.saturating_add(1);
        let structure = built_structure(&build.kind, template, &old, gde_ref);
        let content = std::sync::Arc::clone(&self.content);
        match (
            structure.turret_component,
            content.war_forts.kind(&structure.asset),
        ) {
            (Some(_), Some(kind)) => self.spawn_turret(fight, structure, kind.clone()),
            _ => self.spawn_structure(fight, structure),
        }
    }
}

impl WarFight {
    /// Where the structure of `gde_ref` is in the fight's lists.
    fn placed(&self, gde_ref: &[u8; nw_messages::RAW_ID_LEN]) -> Option<Placed> {
        self.turrets
            .iter()
            .position(|run| &run.body.structure.gde_ref == gde_ref)
            .map(Placed::Turret)
            .or_else(|| {
                self.structures
                    .iter()
                    .position(|run| &run.body.structure.gde_ref == gde_ref)
                    .map(Placed::Other)
            })
    }

    /// Whether a build of the structure of `gde_ref` waits.
    fn building(&self, gde_ref: &[u8; nw_messages::RAW_ID_LEN]) -> bool {
        self.builds.iter().any(|build| &build.old == gde_ref)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use nw_content::war_forts::{SIDE_ATTACKER, SIDE_DEFENDER, SiegeBlueprints, WarForts};
    use nw_messages::FacetHeader;
    use nw_messages::turrets::gde_ref_of;

    use super::*;
    use crate::player_content::PlayerContent;
    use crate::world::turrets::tests::{
        DEFENDER, KIT_CLASS, REPEATER_REF, TABLE, answer_status, bag_count, give, use_request,
        war_world_of,
    };
    use crate::world::turrets::{AMMO_KIT, REPAIR_KIT};

    /// The retail T2 repeater slice and a made-up T2 horn template (the repeater's place,
    /// vitals, damage receiver, container, obstruction, interact, territory and buildable
    /// members; the horn of `CK_offensive_war` has these and its own delayed events).
    pub(crate) const REPEATER_ASSET: &str = "d1000d79ba715eae9e2382d252d9e6d5";
    pub(crate) const HORN_ASSET: [u8; 16] =
        *b"\x09\xea\x04\x4a\x0f\x52\x5b\xba\xdd\xbc\xa0\xc0\x9b\x9e\x51\x06";
    const KINDS: &str = r#", "structure_kinds": [
        {"asset": "d1000d79ba715eae9e2382d252d9e6d5", "slice": "slices/structures/siege/turret/turret_t2_repeater.dynamicslice",
         "template": {"asset": "d1000d79ba715eae9e2382d252d9e6d5", "gde_ref": "fe2d8806c5c5b3da3218b9ad4b3dcb4f", "card_component": 0,
           "turret_component": 15, "interact_component": 13, "spawner_component": 12, "ownership_component": 4,
           "members": ["010d010346561b6a45c6e19412f2037a6a",
           "020f01ff47afc80000000000000b9790c0aee250f80000000000000000ff0047afc80000003f8000005300001eaefb73015f90",
           "03b81701030000000020", "079b1b01140000000000", "0a8b22010100", "0c39010300c800", "0db22d010200000001",
           "0e921b010201", "0fb442017f00000000000066770000000000000000000000000000000000000000ffffffff0000",
           "05ba1801070500000103010b5369656765537570706c79573a918000000000003c00ffffffff00000033000000000000"]}},
        {"asset": "09ea044a0f525bbaddbca0c09b9e5106", "slice": "slices/structures/siege/support/support_t2_hornofresilience.dynamicslice",
         "template": {"asset": "09ea044a0f525bbaddbca0c09b9e5106", "gde_ref": "77777777777777777777777777777777", "card_component": 0,
           "interact_component": 13, "spawner_component": null, "ownership_component": 4,
           "members": ["010d01034500000045000000000003a6a6",
           "020f01ff47afc80000000000000b9790c0aee250f80000000000000000ff0047afc80000003f8000005300001eaefb73015f90",
           "03b81701030000000020", "079b1b01140000000000", "0a8b22010100", "0db22d010200000001", "0e921b010201",
           "05ba1801070500000103010b5369656765537570706c79573a918000000000003c00ffffffff00000033000000000000"]}},
        {"asset": "901505f41948592e9d6b3ac74393efa4", "slice": "slices/structures/siege/turret/turret_t2_explosivelauncher.dynamicslice",
         "template": null}]}"#;
    /// The blueprint costs of the T2 kinds (`BlueprintItemDefinitions`).
    const T2_COST: u32 = 60;
    const CHOICE_REPEATER: u32 = 1;
    const CHOICE_EXPLOSIVE: u32 = 2;
    const CHOICE_HORN: u32 = 4;
    const LATER_MS: u64 = 10;
    /// The level of a player with no experience.
    const FIRST_LEVEL: u32 = 1;

    /// The turret test world with the T2 kinds, their costs and the `SiegeSupply` item.
    pub(crate) fn siege_world(side: u8) -> (SimWorld, Entity) {
        let (mut world, entity) = war_world_of(side);
        let table = format!("{}{KINDS}", &TABLE[..TABLE.len() - 1]);
        world.content = Arc::new(PlayerContent {
            war_forts: WarForts::parse(&table).expect("table"),
            kits: crate::world::gear::tests::kits_with(&[
                (AMMO_KIT, KIT_CLASS),
                (REPAIR_KIT, KIT_CLASS),
                (SIEGE_SUPPLY, KIT_CLASS),
            ]),
            siege_blueprints: SiegeBlueprints::build(
                [
                    ("TurretRepeaterT2".to_owned(), T2_COST),
                    ("SupportHornOfResilienceT2".to_owned(), T2_COST),
                    ("TurretExplosiveT2".to_owned(), T2_COST),
                ],
                [],
            ),
            ..(*world.content).clone()
        });
        (world, entity)
    }

    pub(crate) fn build_call(target: [u8; 16], choice: u32) -> BuildDefensiveStructureMsg {
        let entity = u64::from_le_bytes(target[..8].try_into().expect("8 bytes"));
        BuildDefensiveStructureMsg {
            header: FacetHeader::of(0, DEFENDER),
            structure: gde_ref_of(target, entity),
            facet: 0,
            choice,
        }
    }

    pub(crate) fn connection_of(world: &SimWorld, entity: Entity) -> ConnectionRef {
        world
            .world
            .get::<Player>(entity)
            .expect("player")
            .connection
            .clone()
    }

    fn horn(world: &SimWorld) -> Option<&StructureRun> {
        world
            .war_fight()?
            .structures
            .iter()
            .find(|run| run.body.structure.asset == HORN_ASSET)
    }

    fn has_repeater(world: &SimWorld) -> bool {
        world.war_fight().is_some_and(|fight| {
            fight
                .turrets
                .iter()
                .any(|run| run.body.structure.gde_ref == REPEATER_REF)
        })
    }

    /// B2c: the content table (`NW_CONTENT_DIR`) builds the T1 and T2 explosive launchers no
    /// capture shows from the T3 template with their own tier's vitals and cost (Inferred), and
    /// their slices give them a shell and its explosion: a T1 or T2 socket builds them.
    #[test]
    fn the_inferred_t1_and_t2_explosive_launchers_are_buildable_and_fire_their_shells() {
        let dir = std::env::var("NW_CONTENT_DIR").expect("NW_CONTENT_DIR");
        let forts = WarForts::load(
            std::path::Path::new(&dir),
            crate::world::turrets::tests::CONTENT_BUILD,
        )
        .expect("content table");
        for tier in [1, 2] {
            let socket = forts
                .structure_kinds()
                .find(|kind| {
                    kind.role == nw_content::war_forts::StructureRole::Ballista && kind.tier == tier
                })
                .expect("a ballista of the tier");
            let built = forts
                .replacement(&socket.asset, CHOICE_EXPLOSIVE)
                .expect("the tier's launcher");
            assert!(built.template_inferred, "T{tier}: no capture holds it");
            assert!(forts.buildable(built), "T{tier}");
            let turret = forts
                .kind(&built.asset)
                .expect("its turret kind from the slices");
            assert!(
                turret.projectile.is_some() && turret.area.is_some(),
                "T{tier}: its shell and explosion"
            );
        }
    }

    #[test]
    fn a_build_puts_the_chosen_structure_in_the_old_ones_place_after_its_delay() {
        let (mut world, entity) = siege_world(SIDE_DEFENDER);
        give(&mut world, entity, SIEGE_SUPPLY, T2_COST);
        let connection = connection_of(&world, entity);
        let old_number = world
            .war_fight()
            .and_then(|fight| {
                fight
                    .turrets
                    .iter()
                    .find(|run| run.body.structure.gde_ref == REPEATER_REF)
            })
            .map(|run| run.body.number)
            .expect("repeater");
        world.build_defensive_structure(
            &connection,
            &build_call(REPEATER_REF, CHOICE_HORN),
            NOW_MS,
        );
        assert_eq!(bag_count(&world, entity, SIEGE_SUPPLY), 0, "the T2 cost");
        world.tick_turrets(NOW_MS + BUILD_DELAY_MS - LATER_MS);
        assert!(
            has_repeater(&world) && horn(&world).is_none(),
            "not before its delay"
        );
        world.tick_turrets(NOW_MS + BUILD_DELAY_MS);
        assert!(!has_repeater(&world), "the old one left");
        assert!(
            world.stations.members(old_number).is_none(),
            "its slot stopped"
        );
        let horn = horn(&world).expect("the horn came");
        assert_ne!(horn.body.structure.gde_ref, REPEATER_REF, "a new GdeRef");
        assert_ne!(
            horn.body.structure.gde_ref, [0x77; 16],
            "not the template's"
        );
        let members = world.stations.members(horn.body.number).expect("spawned");
        let place = members
            .iter()
            .find(|member| member.state.type_index() == POSITION_IN_THE_WORLD_TYPE_INDEX)
            .and_then(|member| member.encode().ok());
        assert_eq!(
            place.map(hex::encode).as_deref(),
            Some("010d010346561b6a45c6e19412f2037a6a"),
            "the repeater's place"
        );
        assert!(members.iter().any(|member| matches!(&member.state,
            MemberState::GdeMetadata(card) if card.asset.map(|asset| asset.id) == Some(HORN_ASSET))));
        assert!(members.iter().any(|member| matches!(&member.state,
            MemberState::Ownership(owner) if owner.guild_id == Some(*uuid::Uuid::from_u128(0xd1).as_bytes()))));
        assert!(members.iter().any(|member| matches!(&member.state,
            MemberState::BuildableController(state) if state.current_state == Some(BUILT_STATE))));
        assert_eq!(horn.body.structure.side, SIDE_DEFENDER);
        assert!(horn.body.invulnerable, "the war has not started");
    }

    #[test]
    fn a_build_of_another_side_without_supply_of_the_same_kind_or_with_no_template_is_refused() {
        let (mut world, entity) = siege_world(SIDE_ATTACKER);
        give(&mut world, entity, SIEGE_SUPPLY, T2_COST);
        let connection = connection_of(&world, entity);
        world.build_defensive_structure(
            &connection,
            &build_call(REPEATER_REF, CHOICE_HORN),
            NOW_MS,
        );
        assert_eq!(
            bag_count(&world, entity, SIEGE_SUPPLY),
            T2_COST,
            "an attacker"
        );

        let (mut world, entity) = siege_world(SIDE_DEFENDER);
        let connection = connection_of(&world, entity);
        world.build_defensive_structure(
            &connection,
            &build_call(REPEATER_REF, CHOICE_HORN),
            NOW_MS,
        );
        give(&mut world, entity, SIEGE_SUPPLY, T2_COST);
        for choice in [CHOICE_REPEATER, CHOICE_EXPLOSIVE] {
            world.build_defensive_structure(&connection, &build_call(REPEATER_REF, choice), NOW_MS);
        }
        assert_eq!(
            bag_count(&world, entity, SIEGE_SUPPLY),
            T2_COST,
            "the same kind, no template"
        );
        world.tick_turrets(NOW_MS + BUILD_DELAY_MS);
        assert!(has_repeater(&world) && horn(&world).is_none(), "no build");

        world.build_defensive_structure(
            &connection,
            &build_call(REPEATER_REF, CHOICE_HORN),
            NOW_MS,
        );
        give(&mut world, entity, SIEGE_SUPPLY, T2_COST);
        world.build_defensive_structure(
            &connection,
            &build_call(REPEATER_REF, CHOICE_HORN),
            NOW_MS,
        );
        assert_eq!(
            bag_count(&world, entity, SIEGE_SUPPLY),
            T2_COST,
            "one build at a time"
        );
    }

    #[test]
    fn a_structure_built_into_a_turret_is_a_turret_again() {
        let (mut world, entity) = siege_world(SIDE_DEFENDER);
        give(&mut world, entity, SIEGE_SUPPLY, T2_COST * 2);
        let connection = connection_of(&world, entity);
        world.build_defensive_structure(
            &connection,
            &build_call(REPEATER_REF, CHOICE_HORN),
            NOW_MS,
        );
        world.tick_turrets(NOW_MS + BUILD_DELAY_MS);
        let horn_ref = horn(&world).expect("horn").body.structure.gde_ref;
        let later = NOW_MS + BUILD_DELAY_MS;
        world.build_defensive_structure(&connection, &build_call(horn_ref, CHOICE_REPEATER), later);
        world.tick_turrets(later + BUILD_DELAY_MS);
        assert!(horn(&world).is_none());
        let fight = world.war_fight().expect("fight");
        let repeater = fight
            .turrets
            .iter()
            .find(|run| {
                run.body.structure.asset.to_vec() == hex::decode(REPEATER_ASSET).expect("hex")
            })
            .expect("a repeater again");
        assert!(!repeater.in_use());
        assert_ne!(repeater.body.structure.gde_ref, horn_ref);
        assert_eq!(bag_count(&world, entity, SIEGE_SUPPLY), 0);
    }

    /// A player weapon attack row of the test combat data.
    const SWORD: &str = "1H_Sword_Attack1";

    fn repeater_body(world: &SimWorld) -> StructureBody {
        world
            .war_fight()
            .and_then(|fight| {
                fight
                    .turrets
                    .iter()
                    .find(|run| run.body.structure.gde_ref == REPEATER_REF)
            })
            .map(|run| run.body.clone())
            .expect("repeater")
    }

    #[test]
    fn a_weapon_hit_hurts_a_structure_of_the_other_side_only_while_the_war_runs() {
        let (mut world, attacker) = siege_world(SIDE_ATTACKER);
        // The base damage of the attacker's level (level 1: no experience).
        let mut source = crate::combat::test_support::test_source();
        let (_, level_row) = source.levels[0];
        source.levels.push((FIRST_LEVEL, level_row));
        world.combat.data = std::sync::Arc::new(
            nw_content::combat::CombatData::build(
                source,
                &nw_content::combat::CombatRows::default(),
            )
            .expect("data")
            .with_player_attacks(crate::combat::test_support::test_player_attacks()),
        );
        let number = repeater_body(&world).number;
        assert!(
            world.structure_places(attacker).is_empty(),
            "invulnerable before the start"
        );
        assert!(matches!(
            world.structure_hit(attacker, DEFENDER, number, SWORD),
            Some(Err(crate::combat::AttackRefusal::UnknownTarget))
        ));
        world.with_turrets(|world, fight| world.turrets_at_start(fight));
        assert!(
            world
                .structure_places(attacker)
                .iter()
                .any(|(place, _, _)| *place == number),
            "a defender structure in reach"
        );
        let before = repeater_body(&world).health;
        let outcome = world.structure_hit(attacker, DEFENDER, number, SWORD);
        let Some(Ok(crate::combat::AttackOutcome::Hit { target, amount })) = outcome else {
            panic!("a hit: {outcome:?}");
        };
        assert_eq!(target, number);
        assert!(amount > 0);
        assert!((before - repeater_body(&world).health - amount as f32).abs() < 1.0);
        assert!(
            world
                .structure_hit(attacker, DEFENDER, u64::MAX, SWORD)
                .is_none(),
            "no structure: the hit goes on to the mobs and players"
        );

        let (mut world, defender) = siege_world(SIDE_DEFENDER);
        world.with_turrets(|world, fight| world.turrets_at_start(fight));
        assert!(
            world
                .structure_places(defender)
                .iter()
                .all(|(place, _, _)| *place != number),
            "its own side"
        );
        assert!(matches!(
            world.structure_hit(defender, DEFENDER, number, SWORD),
            Some(Err(crate::combat::AttackRefusal::UnknownTarget))
        ));
    }

    /// Retail `fort-equipements-territory` 24.974 -> 25.006 s: the kind 62 interaction is
    /// accepted before the 2646.
    #[test]
    fn the_build_choices_open_for_the_structures_side_only() {
        let selection = InteractionRequest {
            kind: OPEN_SELECTION_KIND,
            ..use_request(REPEATER_REF)
        };
        for (side, status) in [(SIDE_DEFENDER, USE_ACCEPTED), (SIDE_ATTACKER, USE_REFUSED)] {
            let (mut world, entity) = siege_world(side);
            assert!(world.turret_interaction(entity, DEFENDER, selection, &[], NOW_MS));
            assert_eq!(answer_status(&mut world), Some(status), "side {side}");
        }
        let (mut world, entity) = siege_world(SIDE_DEFENDER);
        let elsewhere = InteractionRequest {
            target: [0x42; 16],
            ..selection
        };
        assert!(!world.turret_interaction(entity, DEFENDER, elsewhere, &[], NOW_MS));
    }

    /// The sim ms of the tests' calls.
    const NOW_MS: u64 = 1_000;
}
