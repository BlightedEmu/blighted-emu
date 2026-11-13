// SPDX-License-Identifier: AGPL-3.0-or-later
//! The open world's gather nodes (gather note, 2026-09-18): flint, bushes and the corpses to
//! skin. The client has the nodes in its level data and names one in its gather request, so a
//! node is known here from its first gather. Retail cards a node only once it left its default
//! state: after a gather it sends the node's card with its state Depleted and its interaction off,
//! and when the node grows back (60 to 90 s later for flint) its state Healthy and its interaction
//! on again. The Fire Cannon of a breaking walls vignette ([`NodeLayout::BreakingWallsCannon`])
//! holds its states in other slots and has a server timeline: at the gather's end its card shows
//! it Depleted with the timeline playing, which fires the cannon and breaks the wall (seen live on
//! 2026-11-12 with both cannons), the timeline stops at its end 30 s later, and the cannon grows
//! back after its respawn time.
//!
//! A gather with a tool runs for the gatherable's `BaseGatherTime` over the gather efficiency:
//! the tool's (`MinGatherEFF` to `MaxGatherEFF` over its gear score range) plus the gathering
//! efficiency of the tradeskill's rank (Inferred: added; retail skinned with the T1 knife, range
//! 2 to 2, at Skinning rank 0 with efficiency 2.0 in 7.58 s for 15 s of work). A gather by hand
//! runs at efficiency 1. A gatherable may ask for a tradeskill rank or an item in the bag
//! ([`nw_content::gathering::GatherRule`]); without them the request is refused. The gather's
//! game event gives its tradeskill experience ([`SimWorld::grant_event`]).
//!
//! A gather with a tool may be done in a single swing: with the chance of the gatherer's held
//! effects and attribute threshold effects for the tradeskill (`INS*`: Strength 300 "25% chance
//! to mine an ore with a single swing", Constitution 300 "... to chop down a tree in a single
//! swing", [`nw_content::status_effects::RewardMods::instant_gather`]) it ends at the server's
//! next gather step ([`GATHER_TICK_MS`]) instead of after its work. Inferred: the client times
//! the swing itself, as every mining and logging gatherable of build 25810808 has `WaitForImpact`
//! 1; no retail capture holds a gather of a player with such an effect.
//!
//! The gatherer's held status effects (status effect bonuses study 2026-11-11, [`LootBonuses`]):
//! the gather's loot roll adds their luck for the tradeskill (`ROL_*`; a gather without a
//! tradeskill, such as a chest, their `GlobalRollMod`) by each table's `RollBonusSetting`
//! ([`nw_content::loot::LootTables::roll_with_luck`]) and holds their `LootTags`; each stack the
//! roll gives grows by their yield share for the tradeskill (`MULT_*`: `Token_Proficiency_1`
//! 0.1), a fraction left rolled. Inferred: the form is the data's (a share added to 1, a bonus
//! added to the roll); no retail gather was measured with and without a yield or luck effect.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use bevy_ecs::entity::Entity;
use nw_content::chest_lockouts::Lockout;
use nw_content::corpse_times::{DEFAULT_CORPSE_MS, SKINNED_CORPSE_MS, corpse_ms};
use nw_content::gathering::Gatherable;
use nw_content::loot::{LootContext, LootRoll, TABLE_PREFIX, loot_level};
use nw_content::tradeskill_xp::NEUTRAL_MODIFIER;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::items::SYNC_MOVED;
use nw_messages::timeline_state::TimelineState;
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    BundleGroup, BundleMember, GatherableControllerState, GdeMetadataReplicatedState,
    InteractState, InteractionEndedMsg, InteractionRequest, MemberState, ServerMessage,
    entity_of_raw_id,
};
use nw_sim_core::Position;

use super::abilities::held_abilities;
use super::crafting::unowned;
use super::gates::timeline_member;
use super::loot_bag::LimitsSent;
use super::slots::Slot;
use super::statics::{
    GatherRun, chest_gather_ended, gather_started, interactor_target, water_gather_ended,
};
use super::tradeskill::EventScale;
use super::{Body, Player, SimWorld, gather_answer, gather_interrupted, send_now};
use crate::mobs::spawn_index;
use crate::objectives::{Event, ObjectiveRun};
use crate::server::SERVER_EPOCH_UNIX_MS;

/// First slot key of a gather node: its index is added. Keys are never sent; they stay above
/// every player and mob number and below the tutorial's static entities.
pub(super) const GATHERABLE_ENTITY_BASE: u64 = 1 << 39;
/// The components of a node's group: its card, its gatherable controller (class 12) and its
/// interact state (class 2930).
const CARD_COMPONENT: u64 = 0;
const CONTROLLER_COMPONENT: u64 = 3;
const INTERACT_COMPONENT: u64 = 4;
/// `gatherableState` values (client enum): Healthy 4, Depleted 8.
const HEALTHY: u8 = 4;
const DEPLETED: u8 = 8;

/// Where a node entity holds the states the server sends to the players near it. A node of no
/// layout gets none: a state in a slot that holds another class crashed the client
/// ([`Gatherable::plant_layout`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NodeLayout {
    /// The gatherable controller in slot 3 and the interact in slot 4.
    Plant,
    /// The Fire Cannon of a breaking walls vignette ([`Gatherable::breaking_walls_cannon`]).
    BreakingWallsCannon,
}

impl NodeLayout {
    /// The layout of the nodes of `gatherable`; `None` for one whose slots are Unknown.
    pub(super) fn of(gatherable: &Gatherable) -> Option<Self> {
        if gatherable.plant_layout() {
            Some(Self::Plant)
        } else if gatherable.breaking_walls_cannon() {
            Some(Self::BreakingWallsCannon)
        } else {
            None
        }
    }
}

/// The Fire Cannon's slots (gimmick scan of 2026-11-12, every one of the 19 retail cards of its
/// 4 placements): the gatherable controller in 2, the server timeline (`VFX Timeline (Server)`,
/// type 1647) in 3, the projectile spawner (type 57) in 5, the interact in 6 and the ownership
/// (type 3217) in 7.
const CANNON_CONTROLLER_COMPONENT: u64 = 2;
const CANNON_TIMELINE_COMPONENT: u64 = 3;
const CANNON_SPAWNER_COMPONENT: u64 = 5;
const CANNON_INTERACT_COMPONENT: u64 = 6;
const CANNON_OWNERSHIP_COMPONENT: u64 = 7;
/// The type index of `MB::ProjectileSpawnerReplicatedState`.
const PROJECTILE_SPAWNER_TYPE_INDEX: u64 = 57;
/// The projectile spawner's body on every retail card: `curAmmo` `0xffff` and
/// `isFiringBlocked` false.
const CANNON_SPAWNER_BODY: [u8; 5] = [0x01, 0x03, 0xff, 0xff, 0x00];
/// The ownership of every retail card: nobody owns it, permission rule 1, no structure name.
const CANNON_PERMISSION_RULE: u8 = 1;
const CANNON_STRUCTURE_NAME: &str = "";
/// How long the cannon's timeline plays from the gather's end: it ends 30 s later stopped at
/// its end (retail `12a_msq_08_destroyrune` 93.925 s to 123.890 s, play head 30.0; every use of
/// the gimmick scan stopped 32.05 to 32.18 s after its request). The timeline fires the cannon
/// and breaks the wall (its slice's timeline entities `Cannon_VFX_Fuse`, `Cannon_VFX_MuzzleFlash`,
/// `Wall Destroyed VFX` and `Debris_Collision`; seen live on 2026-11-12). The node grows back
/// with its `MinRespawnRate` of 31 s (retail 124.840 s; the scan 33.03 to 33.17 s after the
/// request).
const CANNON_TIMELINE_MS: u64 = 30_000;

/// The cannon timeline's play head at its end, seconds ([`CANNON_TIMELINE_MS`]).
#[allow(clippy::cast_precision_loss, reason = "a timeline of seconds")]
fn cannon_timeline_end_s() -> f32 {
    CANNON_TIMELINE_MS as f32 / MS_PER_SECOND
}

/// The status of a refused gather request (gather note: retail 2 for a node another player
/// gathers; Inferred the same for a depleted node).
pub(super) const GATHER_BUSY: u8 = 2;
/// The status of an accepted gather request.
pub(super) const GATHER_ACCEPTED: u8 = 1;
/// The name an open-world gather's accept carries (retail: empty).
const NO_NAME: &[u8] = &[];
/// The server tick a hand gather ends on (retail flint 0.25 s ended 300 to 307 ms after the
/// accept: the gather time rounded up to the tick, plus one).
const GATHER_TICK_MS: u64 = 33;
/// The efficiency of a gather by hand (retail flint, bushes and chests: 1.0), and of a tool
/// gather whose tool has no efficiency row, before the tradeskill's.
const HAND_EFFICIENCY: f32 = 1.0;
/// The gatherers of an open-world node (retail 1).
const ONE_GATHERER: u8 = 1;
/// Milliseconds per second.
const MS_PER_SECOND: f32 = 1_000.0;
/// The paperdoll slot of each tradeskill's tool (beach-quests note: retail equips of the
/// skinning knife, the pick and the hatchet; the client's `PaperdollSlotTypes` order puts
/// `CHOPPING_SLOT` at 33, `CUTTING_SLOT`, the sickle's, at 34, `DRESSING_SLOT` at 35 and
/// `MINING_SLOT` at 36, before `FISHING_SLOT` 37). A gatherable that needs its tool is gathered
/// only with an item in that slot.
const TOOL_SLOTS: [(&str, u32); 4] = [
    ("Skinning", 35),
    ("Mining", 36),
    ("Logging", 33),
    ("Harvesting", 34),
];
/// The paperdoll slot of the tool of `gatherable`'s tradeskill; `None` for a tradeskill
/// without a tool slot.
pub(super) fn tool_slot(gatherable: &Gatherable) -> Option<u32> {
    TOOL_SLOTS
        .iter()
        .find(|(tradeskill, _)| gatherable.tradeskill.as_deref() == Some(*tradeskill))
        .map(|(_, slot)| *slot)
}

/// The roll of a quest drop's chance, in percent.
const PERCENT: u32 = 100;
/// The firings of a gatherable's game event per gather.
const ONE_GATHER: u32 = 1;
/// The gatherable of a water gather (`GatherableData`: `BaseGatherTime` 3, loot table `Water`).
const WATER_GATHERABLE: &str = "Water";
/// How far from the player a node it gathers may be, metres, across the ground (Inferred: the
/// client's interaction reach is Unknown here; generous, so a large node such as a tree, whose
/// level point is its trunk, and a position a moment old still pass).
const GATHER_REACH_M: f32 = 15.0;

/// The stack a gather's yield share `scale` makes of a stack of `quantity`: the whole part of
/// `quantity x scale`, and one more when `below(n)` (a number below `n`) falls under its
/// fraction (Inferred: a yield of 10 % on a single item gives a second one a tenth of the time).
pub(super) fn with_yield(quantity: u32, scale: f64, below: &mut impl FnMut(u32) -> u32) -> u32 {
    let grown = f64::from(quantity) * scale.max(0.0);
    let whole = grown.floor();
    // Fractions in ten-thousandths.
    let fraction = ((grown - whole) * f64::from(YIELD_STEPS)).round() as u32;
    // A stack of a few hundred at most.
    let extra = u32::from(fraction > 0 && below(YIELD_STEPS) < fraction);
    (whole as u32).saturating_add(extra)
}

/// The resolution of a yield fraction's roll.
const YIELD_STEPS: u32 = 10_000;

/// The levels a gather's loot roll `context` holds: a gather of no mob (a node, a crate) takes
/// the gatherer's loot level as its content level (`MinContLevel`). Without it a supply crate
/// gives nothing once its gatherer is above loot level 7: build 25810808
/// `ContainerLootSmallLeveled_2` rolls on `Level` and from loot level 8 on rolls
/// `ContainerLootSmall`, whose buckets hold items tagged `MinContLevel` (`ContainerResources`,
/// `RecoveryItems`): no roll of it gives an item without a content level, and the alchemy
/// crates' `ContainerLootAlchemySmall` neither. Inferred: retail alchemy crate opens gave
/// `PotionHealthT3` (`MinContLevel:19-38`) in areas of levels 17 to 20 (`dtls` 2637 s) and 26
/// to 30 (`99b_msq_02_meetgalahad` 496 s, `dtls` 342 s) and `PotionHealthT4`
/// (`MinContLevel:39-48`) in that area of levels 26 to 30 too (`02a_side08_01_treasures`
/// 102 s): a level of the gatherer, which the area's level alone does not give. No capture names
/// the gatherer's level at the open. A skinned mob keeps its own level.
pub(super) fn gather_levels(context: LootContext<'_>) -> LootContext<'_> {
    LootContext {
        content: context.content.or(context.player),
        ..context
    }
}

/// What a player's held status effects give its loot rolls.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct LootBonuses {
    /// The luck added to the tables' rolls ([`super::status_effects::StatusEffects::luck`]).
    pub(super) luck: u32,
    /// The loot tags the rolls hold, with the mob's.
    pub(super) tags: Vec<String>,
    /// The multiple of each stack a gather of a tradeskill gives (`MULT_*`); 1 for another roll.
    pub(super) yield_scale: f64,
}

/// Why a gather request is refused besides a busy node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GatherRefusal {
    /// The gatherable needs its tradeskill's tool and none is worn.
    NoTool,
    /// The tradeskill's rank is below the gatherable's `RequiredTradeskillLevel`.
    Rank,
    /// The bag lacks the gatherable's `ItemRestrictionId`.
    Item,
    /// The player opened this loot container and its lockout has not reset.
    Opened,
}

/// A gather that runs.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct OpenGather {
    number: u64,
    request: InteractionRequest,
    gatherable: Gatherable,
    /// The loot table it rolls: its crate variant's, else its `FinalLootTable`.
    table: Option<String>,
    /// The mob skinned, for a corpse.
    mob: Option<u64>,
    /// A water gather (kind 15): no node to deplete, and a done state that names no gatherable.
    water: bool,
    /// The encounter start object gathered ([`super::encounter_roots::EncounterObjects`]): the
    /// gather's end takes it away, and its stage counts it; or the switch of the instance's gates
    /// ([`super::gates`]) whose rules the gather's end fires.
    object: Option<u64>,
    end_ms: u64,
}

impl OpenGather {
    /// The own done state of the gather that ends (or stops) as the player's gather number
    /// `gather`.
    fn done_state(&self, gather: u32) -> BundleMember {
        if self.water {
            water_gather_ended(gather)
        } else {
            chest_gather_ended(self.request.target_entity, gather)
        }
    }
}

impl SimWorld {
    /// The nodes depleted since the last call, for the world database.
    pub(crate) fn take_depleted_nodes(&mut self) -> Vec<DepletedNode> {
        let (now_ms, server_ms) = (self.last_now_ms, self.server_ms);
        self.nodes.take_unsaved(|at_ms| {
            at_ms
                .saturating_sub(now_ms)
                .saturating_add(server_ms)
                .saturating_add(SERVER_EPOCH_UNIX_MS)
        })
    }

    /// Holds again the depleted nodes the world database kept for this level, at sim ms
    /// `now_ms` and Unix ms `now_unix_ms`: each grows back at its saved time.
    pub(crate) fn restore_depleted_nodes(
        &mut self,
        saved: &[DepletedNode],
        now_ms: u64,
        now_unix_ms: u64,
    ) {
        for node in saved {
            let left_ms = node.respawn_at_unix_ms.saturating_sub(now_unix_ms);
            let layout = self
                .content
                .gather_points
                .kind_of(node.point)
                .and_then(|crc| self.content.gatherables.get(crc))
                .and_then(NodeLayout::of);
            self.nodes
                .restore(node, now_ms.saturating_add(left_ms), layout);
        }
    }

    /// Player `number` (`entity`) asks at `now_ms` to gather `request` in the open world: a node
    /// of the level (flint, a bush), a loot crate or a dead mob to skin, named by its gatherable
    /// and `name` (a crate's variant name, empty for a node). A known gatherable that nobody
    /// else gathers is accepted (3743, the running gather in the own states) and ends after its
    /// gather time ([`SimWorld::gathers_due`]); a busy one is refused (3743 status 2).
    /// [`SimWorld::interact`] passes each interaction once.
    pub(super) fn gather_open_world(
        &mut self,
        entity: Entity,
        number: u64,
        request: InteractionRequest,
        name: &[u8],
        now_ms: u64,
    ) {
        let Some(gatherable) = request
            .gatherable
            .and_then(|crc| self.content.gatherables.get(crc))
            .cloned()
        else {
            tracing::debug!(player = number, ?request, "gather of an unknown gatherable");
            return;
        };
        let Some(position) = self
            .world
            .get::<Body>(entity)
            .map(|body| body.motion.position())
        else {
            return;
        };
        let mob = entity_of_raw_id(&request.target).filter(|mob| spawn_index(*mob).is_some());
        let object = self
            .encounter_objects
            .at(&request.target, position, GATHER_REACH_M)
            .or_else(|| self.gate_switch_at(&request.target, position, GATHER_REACH_M));
        // The node must be one of the level's gatherables of that kind (or a dead mob to skin,
        // or an encounter's start object) within reach of the player: a client cannot gather
        // from anywhere, nor a node out of its time of day ([`super::day_night`]).
        let phase = self.day_phase();
        let point = match mob {
            None if object.is_some() => Some(None),
            Some(mob) => self
                .mobs
                .places(mob)
                .filter(|(at, _)| at.horizontal_distance(position) <= GATHER_REACH_M)
                .map(|_| None),
            None => request
                .gatherable
                .and_then(|crc| {
                    let near: Vec<(usize, Position)> = self
                        .content
                        .gather_points
                        .within(crc, position, GATHER_REACH_M)
                        .into_iter()
                        .filter(|(point, _)| {
                            self.content.time_of_day.gather_point_shows(*point, phase)
                        })
                        .collect();
                    self.nodes.point_for(&request.target, &near)
                })
                .map(Some),
        };
        let Some(point) = point else {
            tracing::debug!(player = number, gatherable = %gatherable.id, "gather out of reach of any such node");
            if let Some(player) = self.world.get::<Player>(entity) {
                let refusal = gather_answer(number, request, GATHER_BUSY, NO_NAME);
                send_now(&mut self.combat_messages, &player.connection, &[refusal]);
            }
            return;
        };
        let table = self.gather_table(&gatherable, request, name);
        let opened = gatherable.loot_container
            && self
                .world
                .get::<Player>(entity)
                .is_some_and(|player| player.loot.opened(&request.target));
        let allowed = if opened {
            Err(GatherRefusal::Opened)
        } else if self.holds_tool(entity, &gatherable) {
            self.meets_gather_rule(entity, &gatherable, request.gatherable)
        } else {
            Err(GatherRefusal::NoTool)
        };
        if let Err(refusal) = allowed {
            tracing::debug!(player = number, gatherable = %gatherable.id, ?refusal, "gather refused");
            if let Some(player) = self.world.get::<Player>(entity) {
                let refusal = gather_answer(number, request, GATHER_BUSY, NO_NAME);
                send_now(&mut self.combat_messages, &player.connection, &[refusal]);
            }
            return;
        }
        let tool_efficiency =
            (!gatherable.by_hand).then(|| self.gather_efficiency(entity, &gatherable));
        let single_swing = tool_efficiency.is_some() && {
            let chance = self.instant_gather_chance(entity, &gatherable);
            self.dice.chance(chance)
        };
        let started = match (mob, point) {
            (None, _) if let Some(object) = object => {
                // One player gathers a start object at a time.
                if self
                    .open_gathers
                    .iter()
                    .any(|open| open.object == Some(object))
                {
                    tracing::debug!(player = number, object, "start object gathered by another");
                    Err(())
                } else {
                    Ok(())
                }
            }
            (Some(mob), _) => self.start_skinning(mob, number),
            (None, Some((point, at))) => self
                .nodes
                .start(
                    request.target,
                    point,
                    at,
                    number,
                    NodeLayout::of(&gatherable),
                )
                .map_err(|busy| tracing::debug!(player = number, ?busy, "gather refused")),
            (None, None) => Err(()),
        };
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        if started.is_err() {
            let refusal = gather_answer(number, request, GATHER_BUSY, NO_NAME);
            send_now(&mut self.combat_messages, &player.connection, &[refusal]);
            return;
        }
        #[allow(clippy::cast_precision_loss, reason = "a gather time of seconds")]
        let amount = gatherable.gather_ms as f32 / MS_PER_SECOND;
        let (duration_ms, efficiency) = match tool_efficiency {
            Some(efficiency) => {
                #[allow(
                    clippy::cast_precision_loss,
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a gather time of seconds over a positive efficiency"
                )]
                let tool_ms = (gatherable.gather_ms as f32 / efficiency) as u64;
                (
                    if single_swing {
                        GATHER_TICK_MS
                    } else {
                        tool_ms
                    },
                    efficiency,
                )
            }
            None => (
                gatherable.gather_ms.div_ceil(GATHER_TICK_MS) * GATHER_TICK_MS + GATHER_TICK_MS,
                HAND_EFFICIENCY,
            ),
        };
        let run = GatherRun {
            amount,
            efficiency,
            gatherers: ONE_GATHERER,
        };
        let accept = gather_answer(number, request, GATHER_ACCEPTED, NO_NAME);
        send_now(&mut self.combat_messages, &player.connection, &[accept]);
        let started = gather_started(request.target_entity, player.gathers, run);
        player.extra_members.extend(started);
        tracing::debug!(player = number, gatherable = %gatherable.id, duration_ms, single_swing, "gather");
        self.open_gathers.push(OpenGather {
            number,
            request,
            gatherable,
            table,
            mob,
            water: false,
            object,
            end_ms: now_ms.saturating_add(duration_ms),
        });
    }

    /// Player `number` gathers water at `now_ms` (interaction kind 15; craft2 report, retail
    /// `Hive_Dye`, 2 gathers): the request names no gatherable and retail answers no 3743, only
    /// the own interactor's target (the player's own entity, 70.92 s); the gather of the `Water`
    /// gatherable ends after its `BaseGatherTime` (retail 3,144 ms for 3 s) with the interaction's
    /// end (3746), a done gather state ([`water_gather_ended`], 74.05 s) and its loot table's
    /// water (`WaterT1` 30), and the client's cancel that follows gets no 2724. There is no node:
    /// two retail gathers 3.7 s apart both gave water.
    pub(super) fn gather_water(&mut self, number: u64, request: InteractionRequest, now_ms: u64) {
        let Some(gatherable) = self
            .content
            .gatherables
            .get(lowercase_crc32(WATER_GATHERABLE))
            .cloned()
        else {
            tracing::debug!(player = number, "water gather without a water gatherable");
            return;
        };
        let duration_ms =
            gatherable.gather_ms.div_ceil(GATHER_TICK_MS) * GATHER_TICK_MS + GATHER_TICK_MS;
        tracing::debug!(player = number, duration_ms, "water gather");
        if let Some(entity) = self.player_by_number(number)
            && let Some(mut player) = self.world.get_mut::<Player>(entity)
        {
            player
                .extra_members
                .push(interactor_target(request.target_entity));
        }
        self.open_gathers.push(OpenGather {
            number,
            request,
            table: gatherable.loot_table.clone(),
            gatherable,
            mob: None,
            water: true,
            object: None,
            end_ms: now_ms.saturating_add(duration_ms),
        });
    }

    /// The loot table of a gather of `gatherable` that `request` names `name`: a loot crate's
    /// is its variant's ([`nw_content::gathering::CrateLoot`]), any other gatherable's its
    /// `FinalLootTable`. A crate whose variant is Unknown rolls its `FinalLootTable` (`Empty`).
    fn gather_table(
        &self,
        gatherable: &Gatherable,
        request: InteractionRequest,
        name: &[u8],
    ) -> Option<String> {
        let variant = gatherable
            .loot_container
            .then(|| {
                let name = std::str::from_utf8(name).ok()?;
                let crc = request.gatherable?;
                self.content.crates.table(request.target_entity, crc, name)
            })
            .flatten();
        if gatherable.loot_container && variant.is_none() {
            tracing::info!(gatherable = %gatherable.id, ?request, "crate of an unknown variant");
        }
        variant
            .map(str::to_owned)
            .or_else(|| gatherable.loot_table.clone())
    }

    /// What player `number` rolls loot with: its level, and for a mob `mob` (killed or skinned)
    /// the mob's level as the enemy and content level (Inferred: the content of a kill is the
    /// creature; nwdb.info lists creature loot by `{!clvl}` ranges around the creature's level)
    /// and its loot tags. Without a mob there is no enemy, and the POI level is Unknown; a gather
    /// adds its content level ([`gather_levels`]).
    /// `reached_limits` are the player's loot limits at their limit
    /// ([`SimWorld::reached_loot_limits`]).
    pub(super) fn loot_context<'a>(
        &mut self,
        number: u64,
        mob: Option<(u64, &'a [String])>,
        reached_limits: &'a [String],
    ) -> LootContext<'a> {
        let player = self
            .player_by_number(number)
            .and_then(|entity| self.world.get::<Player>(entity))
            .and_then(|player| {
                self.content
                    .player_loot_levels
                    .of(u64::from(player.experience))
            });
        let enemy = mob
            .and_then(|(mob, _)| self.mobs.level(mob))
            .map(loot_level);
        LootContext {
            enemy,
            content: enemy,
            player,
            poi: None,
            tags: mob.map_or(&[], |(_, tags)| tags),
            reached_limits,
        }
    }

    /// What the held status effects of player `number`, and the effects of its attribute
    /// thresholds, give a loot roll with the mob's loot tags `mob_tags`: of a gather of tradeskill
    /// `tradeskill`, their luck and yield for it (`ROL_*`: `ATB_Con_Bonus_50_2_Effect` 150 to each
    /// gathering tradeskill; `MULT_*`: `ATB_Str_Bonus_250_2_Effect` 0.1 to mining); of another
    /// roll (`None`: a kill, a chest), their `GlobalRollMod` and no yield. Their `LootTags` join
    /// the mob's.
    pub(super) fn loot_bonuses(
        &mut self,
        number: u64,
        mob_tags: &[String],
        tradeskill: Option<&str>,
    ) -> LootBonuses {
        let mut bonuses = LootBonuses {
            tags: mob_tags.to_vec(),
            yield_scale: NEUTRAL_MODIFIER,
            ..LootBonuses::default()
        };
        let Some(entity) = self.player_by_number(number) else {
            return bonuses;
        };
        let attribute_luck = self.attribute_effect_sum(entity, |rule| match tradeskill {
            Some(tradeskill) => rule.rewards.luck_of(tradeskill),
            None => rule.rewards.global_luck,
        });
        let attribute_yield = tradeskill.map_or(0.0, |tradeskill| {
            self.attribute_effect_sum(entity, |rule| rule.rewards.yield_share(tradeskill))
        });
        let rules = &self.content.status_effects;
        let Some(effects) = self
            .world
            .get::<Player>(entity)
            .map(|player| &player.status_effects)
        else {
            return bonuses;
        };
        // A luck of a few thousand on a 100,000 roll.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a luck of a few thousand, at least 0"
        )]
        let attribute_luck = attribute_luck.max(0.0).round() as u32;
        bonuses.luck = effects
            .luck(rules, tradeskill)
            .saturating_add(attribute_luck);
        bonuses.tags.extend(effects.loot_tags(rules));
        if let Some(tradeskill) = tradeskill {
            bonuses.yield_scale = effects.yield_scale(rules, tradeskill) + attribute_yield;
        }
        bonuses
    }

    /// The status effects that the attribute threshold abilities of player `entity` hold
    /// ([`nw_content::attribute_bonuses::AttributeBonuses::equip_effects`]).
    pub(super) fn attribute_effects(&self, entity: Entity) -> Vec<&str> {
        let Some(player) = self.world.get::<Player>(entity) else {
            return Vec::new();
        };
        let held = held_abilities(
            (
                &player.masteries,
                player.gear.as_ref(),
                &player.attributes,
                &player.status_effects,
            ),
            &self.content,
        );
        self.content.attribute_bonuses.equip_effects(held).collect()
    }

    /// The sum of `value` over the rules of the effects of player `entity`'s attribute
    /// thresholds ([`Self::attribute_effects`]), each at one stack's potency (Inferred: the
    /// effect is held once, as long as its ability, and no category mod of these effects exists).
    pub(super) fn attribute_effect_sum(
        &self,
        entity: Entity,
        value: impl Fn(&nw_content::status_effects::StatusEffectRule) -> f64,
    ) -> f64 {
        let rules = &self.content.status_effects;
        self.attribute_effects(entity)
            .into_iter()
            .filter_map(|effect| rules.by_id(effect))
            .map(value)
            .sum()
    }

    /// Whether player `entity` may gather `gatherable`: one that needs its tradeskill's tool
    /// (`Restriction` ToolRequired) only with an item in the tool's slot (Inferred: retail
    /// answered the skinning knife's absence with the client's "No equipped Knife" prompt, live
    /// 2026-09-18). A tradeskill without a known slot needs no tool.
    fn holds_tool(&self, entity: Entity, gatherable: &Gatherable) -> bool {
        let has_slot = TOOL_SLOTS
            .iter()
            .any(|(tradeskill, _)| gatherable.tradeskill.as_deref() == Some(*tradeskill));
        !(gatherable.tool_required && has_slot) || self.worn_tool(entity, gatherable).is_some()
    }

    /// Whether player `entity` meets the gather rule of `gatherable` (named by `crc` in the
    /// request): its tradeskill's rank at the rule's rank, and the rule's item in the bag
    /// (Unknown whether the gather takes the item; it stays in the bag here).
    fn meets_gather_rule(
        &self,
        entity: Entity,
        gatherable: &Gatherable,
        crc: Option<u32>,
    ) -> Result<(), GatherRefusal> {
        let rule = crc
            .map(|crc| self.content.gather_rules.of(crc))
            .unwrap_or_default();
        let rank = gatherable
            .tradeskill
            .as_deref()
            .map_or(0, |tradeskill| self.tradeskill_perks(entity, tradeskill).0);
        if u32::from(rank) < rule.required_rank {
            return Err(GatherRefusal::Rank);
        }
        let holds_item = rule.required_item.as_ref().is_none_or(|(item, quantity)| {
            self.world
                .get::<Player>(entity)
                .and_then(|player| player.gear.as_ref())
                .is_some_and(|gear| gear.bag_count(item) >= *quantity)
        });
        if holds_item {
            Ok(())
        } else {
            Err(GatherRefusal::Item)
        }
    }

    /// The gather efficiency of player `entity` with a tool on `gatherable`: its tradeskill's
    /// tool at its gear score (else [`HAND_EFFICIENCY`]), plus the gathering efficiency of the
    /// tradeskill's rank, of the player's held status effects and of the effects of its
    /// attribute thresholds (`EFF_*`: `ATB_Str_Bonus_25_2_Effect` 0.1 to mining).
    fn gather_efficiency(&self, entity: Entity, gatherable: &Gatherable) -> f32 {
        let tool = self.worn_tool(entity, gatherable).and_then(|item| {
            let efficiency = self.content.tools.of(item)?;
            let gear_score = self.content.kits.item(item).map_or(0, |kit| kit.gear_score);
            Some(efficiency.at(gear_score))
        });
        let rank = gatherable.tradeskill.as_deref().map_or(0.0, |tradeskill| {
            let effects = self.world.get::<Player>(entity).map_or(0.0, |player| {
                player
                    .status_effects
                    .gather_efficiency(&self.content.status_effects, tradeskill)
            });
            let attributes = self.attribute_effect_sum(entity, |rule| {
                rule.gather_efficiency
                    .iter()
                    .filter(|(skill, _)| skill.eq_ignore_ascii_case(tradeskill))
                    .map(|(_, value)| *value)
                    .sum()
            });
            self.tradeskill_perks(entity, tradeskill)
                .1
                .gathering_efficiency
                + effects
                + attributes
        });
        #[allow(
            clippy::cast_possible_truncation,
            reason = "efficiencies of a few units fit an f32"
        )]
        let efficiency = (tool.unwrap_or(f64::from(HAND_EFFICIENCY)) + rank) as f32;
        efficiency.max(HAND_EFFICIENCY)
    }

    /// The chance that a gather of `gatherable` by player `entity` is done in a single swing:
    /// the `INS*` chance of its held status effects and of the effects of its attribute
    /// thresholds for the gatherable's tradeskill (0 without a tradeskill).
    fn instant_gather_chance(&self, entity: Entity, gatherable: &Gatherable) -> f64 {
        let Some(tradeskill) = gatherable.tradeskill.as_deref() else {
            return 0.0;
        };
        let effects = self.world.get::<Player>(entity).map_or(0.0, |player| {
            player
                .status_effects
                .instant_gather_chance(&self.content.status_effects, tradeskill)
        });
        effects
            + self.attribute_effect_sum(entity, |rule| {
                rule.rewards.instant_gather_chance(tradeskill)
            })
    }

    /// The item player `entity` wears in the tool slot of `gatherable`'s tradeskill.
    fn worn_tool(&self, entity: Entity, gatherable: &Gatherable) -> Option<&str> {
        let slot = tool_slot(gatherable)?;
        self.world
            .get::<Player>(entity)
            .and_then(|player| player.gear.as_ref())
            .and_then(|gear| gear.worn_item(slot))
    }

    /// Player `number` starts to skin `mob`: only a dead mob that nobody skinned or skins.
    fn start_skinning(&self, mob: u64, number: u64) -> Result<(), ()> {
        let dead = self.mobs.health(mob) == Some(0);
        let taken = self.skinned.contains_key(&mob)
            || self
                .open_gathers
                .iter()
                .any(|gather| gather.mob == Some(mob) && gather.number != number);
        if dead && !taken {
            Ok(())
        } else {
            tracing::debug!(player = number, mob, dead, taken, "skinning refused");
            Err(())
        }
    }

    /// The gathers whose time came at `now_ms` end: the interaction ends (3746), the own gather
    /// state is done, the loot of the gatherable (less what the player's loot limits hold back)
    /// and its quest drops go into the bag and to the
    /// open-world quest, its game event goes to the quest (a quest object), and the node is
    /// depleted until its respawn (a corpse is skinned). The depleted nodes whose time came grow
    /// back.
    pub(super) fn gathers_due(&mut self, now_ms: u64) {
        self.nodes.tick(now_ms);
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.open_gathers)
            .into_iter()
            .partition(|gather| now_ms >= gather.end_ms);
        self.open_gathers = later;
        for gather in due {
            self.end_gather(&gather, now_ms);
        }
    }

    /// The corpses whose time came at `now_ms` go (`nw_content::corpse_times`): a
    /// skinned one [`SKINNED_CORPSE_MS`] after its skinning, one that can be skinned stays until
    /// then, any other after its creature's corpse time.
    pub(super) fn bury_corpses(&mut self, now_ms: u64) {
        let content = std::sync::Arc::clone(&self.content);
        let due: Vec<u64> = self
            .mobs
            .corpses()
            .filter(|&(mob, died_ms)| {
                let gone_at_ms = if let Some(&skinned_ms) = self.skinned.get(&mob) {
                    Some(skinned_ms + SKINNED_CORPSE_MS)
                } else {
                    let creature = self
                        .mobs
                        .vitals_of(mob)
                        .and_then(|vitals| content.mob_looks.look(vitals, self.mobs.asset(mob)))
                        .and_then(|look| content.creature_animations.get(&look.asset.id));
                    match creature {
                        Some(creature) if creature.skinnable() => None,
                        Some(creature) => Some(died_ms + corpse_ms(&creature.slice)),
                        None => Some(died_ms + DEFAULT_CORPSE_MS),
                    }
                };
                gone_at_ms.is_some_and(|gone_at_ms| now_ms >= gone_at_ms)
            })
            .map(|(mob, _)| mob)
            .collect();
        for mob in due {
            self.mobs.bury(mob);
        }
    }

    fn end_gather(&mut self, gather: &OpenGather, now_ms: u64) {
        let content = std::sync::Arc::clone(&self.content);
        let mob_tags = gather
            .mob
            .and_then(|mob| self.mobs.vitals_of(mob))
            .and_then(|vitals| content.mob_loot.of(vitals))
            .map_or(&[][..], |drop| drop.tags.as_slice());
        let bonuses = self.loot_bonuses(
            gather.number,
            mob_tags,
            gather.gatherable.tradeskill.as_deref(),
        );
        let reached = self.reached_loot_limits(gather.number);
        let levels = gather_levels(LootContext {
            tags: &bonuses.tags,
            ..self.loot_context(
                gather.number,
                gather.mob.map(|mob| (mob, mob_tags)),
                &reached,
            )
        });
        let dice = &mut self.dice;
        let rolled = gather
            .table
            .as_deref()
            .map(|table| {
                content
                    .loot
                    .roll_with_luck(table, levels, bonuses.luck, &mut |below| dice.below(below))
            })
            .map(|rolled| LootRoll {
                items: rolled
                    .items
                    .into_iter()
                    .map(|(item, quantity)| {
                        let grown = with_yield(quantity, bonuses.yield_scale, &mut |below| {
                            dice.below(below)
                        });
                        (item, grown)
                    })
                    .collect(),
                ..rolled
            })
            .unwrap_or_default();
        let hints = rolled.gear_scores.clone();
        let mut loot = self.limit_loot(gather.number, rolled);
        self.send_loot_limits(gather.number, LimitsSent::Changed);
        loot.extend(self.quest_drops(gather.number, gather.mob, &gather.gatherable));
        match gather.mob {
            Some(mob) => {
                self.skinned.insert(mob, now_ms);
            }
            None if gather.water => {}
            // A start object goes: its encounter's stage counts it. A switch of the instance's
            // gates ([`super::gates`]) stays: its rules change it.
            None if let Some(object) = gather.object => {
                self.encounter_objects.remove(object);
                self.gate_switch_used(object);
            }
            // A container stays for everyone else: only its opener's tracker locks it.
            None if self.lockout_of(gather).is_some() => self.nodes.release(&gather.request.target),
            None => {
                let (shortest, longest) = gather.gatherable.respawn_ms;
                let spread = u32::try_from(longest.saturating_sub(shortest)).unwrap_or(u32::MAX);
                let respawn_at_ms = now_ms
                    .saturating_add(shortest)
                    .saturating_add(u64::from(self.dice.below(spread.saturating_add(1))));
                self.nodes
                    .deplete(&gather.request.target, now_ms, respawn_at_ms);
            }
        }
        // A container's lockout ends at the next reset of its kind, from now.
        let opened = self.lockout_of(gather).and_then(|lockout| {
            let opened_unix_ms = self.server_ms.saturating_add(SERVER_EPOCH_UNIX_MS);
            let reset_ms = lockout
                .ends_at(opened_unix_ms)
                .saturating_sub(SERVER_EPOCH_UNIX_MS);
            Some((reset_ms, self.nodes.position_of(&gather.request.target)?))
        });
        let Some(entity) = self.player_by_number(gather.number) else {
            return;
        };
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let end = ServerMessage::InteractionEnded(InteractionEndedMsg {
            header: nw_messages::FacetHeader::of(
                nw_messages::facet::component::INTERACTOR,
                gather.number,
            ),
        });
        send_now(&mut self.combat_messages, &player.connection, &[end]);
        let ended = gather.done_state(player.gathers);
        player.extra_members.push(ended);
        player.gathers = player.gathers.saturating_add(1);
        if !gather.water {
            player.ended_gather = Some(gather.request);
        }
        if let Some((reset_ms, at)) = opened {
            player.list_version += 1;
            let version = player.list_version;
            let tracked = player.loot.track_opened(
                gather.request.target,
                at,
                self.server_ms,
                reset_ms,
                true,
                version,
            );
            player.extra_members.push(tracked);
        }
        let (items, rolls) = self.roll_drops(gather.number, gather.mob, loot, &hints);
        let gathered = items.clone();
        self.give_drops(
            gather.number,
            items.into_iter().zip(rolls).collect(),
            SYNC_MOVED,
            now_ms,
        );
        for (item, quantity) in &gathered {
            self.quest_event(
                gather.number,
                Event::Gathered {
                    item,
                    quantity: *quantity,
                },
                now_ms,
            );
        }
        if let Some(crc) = gather.request.gatherable {
            for (cyclic, state) in self.cyclic_gathers(entity, crc) {
                self.quest_event(
                    gather.number,
                    Event::CyclicGathered {
                        cyclic: &cyclic,
                        state,
                    },
                    now_ms,
                );
            }
        }
        self.wear_tool(entity, &gather.gatherable);
        if let Some(event) = &gather.gatherable.game_event {
            let scale = self.gather_scale(gather);
            self.grant_scaled_event(gather.number, event, ONE_GATHER, scale, now_ms);
            self.quest_event(gather.number, Event::GameEvent(event), now_ms);
        }
        if let Some(tradeskill) = &gather.gatherable.tradeskill {
            self.meta_event(
                entity,
                super::meta_achievements::MetaEvent::Gathered { tradeskill },
            );
        }
        let items = gathered
            .iter()
            .fold(0_u32, |sum, (_, quantity)| sum.saturating_add(*quantity));
        self.season_event(
            entity,
            super::seasons::SeasonEvent::Gathered {
                gatherable: &gather.gatherable.id,
                tradeskill: gather.gatherable.tradeskill.as_deref(),
                items,
            },
        );
    }

    /// The cyclic gatherables (variant id) and their states that player `entity` gathers with a
    /// gather of the gatherable whose lowercase id has the CRC-32 `crc`: the state the
    /// gatherable shows, else the player's turns of it so far (which this gather adds to).
    fn cyclic_gathers(&mut self, entity: Entity, crc: u32) -> Vec<(String, u32)> {
        let shown = self.content.cyclic_gatherables.of(crc).to_vec();
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return Vec::new();
        };
        shown
            .into_iter()
            .map(|shown| {
                let state = shown.state.unwrap_or_else(|| {
                    let turns = player
                        .cyclic_turns
                        .entry(shown.cyclic.to_lowercase())
                        .or_default();
                    let state = *turns % shown.states;
                    *turns = turns.wrapping_add(1);
                    state
                });
                (shown.cyclic, state)
            })
            .collect()
    }

    /// The lockout of the loot container `gather` opens; `None` for a gather of another kind
    /// ([`nw_content::chest_lockouts`]).
    fn lockout_of(&self, gather: &OpenGather) -> Option<Lockout> {
        if !gather.gatherable.loot_container || gather.mob.is_some() || gather.water {
            return None;
        }
        gather
            .request
            .gatherable
            .and_then(|crc| self.content.chest_lockouts.of(crc))
    }

    /// How the game event of `gather` scales: a skinned corpse by the `GatherableLevelModifier`
    /// of the mob's level in the tradeskill's rank table, a node not at all (craft2 report:
    /// retail boars of level 2 and 3 gave 198 and 208 for `Gather_SkinningT2` 95).
    fn gather_scale(&self, gather: &OpenGather) -> EventScale {
        let modifier = gather
            .mob
            .and_then(|mob| self.mobs.level(mob))
            .zip(gather.gatherable.tradeskill.as_deref())
            .map_or(NEUTRAL_MODIFIER, |(level, tradeskill)| {
                self.content
                    .tradeskill_curves
                    .level_modifier(tradeskill, level)
            });
        EventScale::gather(modifier)
    }

    /// The items the quests of player `number` drop for its gather of `gatherable`: the corpse
    /// of `mob` it skinned, a quest object, or a container. One of each waiting task's item whose
    /// enemy the mob is, or whose gatherable the object is, or that a container in the player's
    /// place drops, at the task's chance.
    pub(super) fn quest_drops(
        &mut self,
        number: u64,
        mob: Option<u64>,
        gatherable: &Gatherable,
    ) -> Vec<(String, u32)> {
        if let Some(mob) = mob {
            return self.mob_quest_drops(number, mob, ObjectiveRun::quest_drops);
        }
        // A quest object is most often a loot container too (346 of the 381 steps that drop
        // from a gatherable, `1225_Truffle` of Truffle Fever among them): it gives its own
        // drops and the drops of any container in a mission's place.
        let drops = self
            .quests_of_player(number)
            .iter()
            .flat_map(|quest| {
                let mut drops = quest.run.gather_drops(&gatherable.id);
                if gatherable.loot_container {
                    drops.extend(quest.run.chest_drops());
                }
                drops
            })
            .collect();
        self.roll_quest_drops(drops)
    }

    /// The items the quests of player `number` drop for mob `mob`: what `drops` gives for each
    /// quest run with the mob's vitals id, categories and spawn categories, each rolled at its
    /// chance ([`SimWorld::roll_quest_drops`]).
    pub(super) fn mob_quest_drops(
        &mut self,
        number: u64,
        mob: u64,
        drops: impl Fn(&ObjectiveRun, &str, &[String], &[u32]) -> Vec<(String, u32)>,
    ) -> Vec<(String, u32)> {
        let quests = self.quests_of_player(number);
        if quests.is_empty() {
            return Vec::new();
        }
        let Some((vitals_id, categories)) = self
            .mobs
            .vitals_of(mob)
            .and_then(|vitals| self.content.kill_types.of(vitals))
        else {
            return Vec::new();
        };
        let spawn_categories = self
            .content
            .kill_types
            .spawn_categories(self.mobs.category_of(mob));
        let drops = quests
            .iter()
            .flat_map(|quest| drops(&quest.run, vitals_id, categories, &spawn_categories))
            .collect();
        self.roll_quest_drops(drops)
    }

    /// One of each item of `drops` (item, chance in percent) that its roll gives.
    fn roll_quest_drops(&mut self, drops: Vec<(String, u32)>) -> Vec<(String, u32)> {
        drops
            .into_iter()
            .filter(|(_, percent)| self.dice.below(PERCENT) < *percent)
            .map(|(item, _)| (item, 1))
            .collect()
    }

    /// Gives `loot` to player `number` at `now_ms`: an entry `[LTID]X` rolls loot table `X` once
    /// per unit (`LootTables`, as a game event's item reward names it), an item that rolls gets
    /// its gear score and perks ([`SimWorld::roll_drops`]), and each goes into the bag; what does
    /// not fit drops at the player's feet ([`SimWorld::give_drops`]). Whether every item went
    /// into the bag.
    pub(super) fn give_loot(&mut self, number: u64, loot: Vec<(String, u32)>, now_ms: u64) -> bool {
        self.give_loot_as(number, loot, SYNC_MOVED, now_ms)
    }

    /// [`SimWorld::give_loot`] for a bag change of sync reason `reason` (a craft's output).
    pub(super) fn give_loot_as(
        &mut self,
        number: u64,
        loot: Vec<(String, u32)>,
        reason: u8,
        now_ms: u64,
    ) -> bool {
        let items = self.rolled_tables(number, loot);
        let (entries, values) = self.roll_drops(number, None, items, &BTreeMap::new());
        self.give_drops(
            number,
            entries.into_iter().zip(values).collect(),
            reason,
            now_ms,
        )
    }

    /// `loot` with each `[LTID]` entry rolled for player `number` (its quantity times) within its
    /// loot limits; the other entries as they are.
    fn rolled_tables(&mut self, number: u64, loot: Vec<(String, u32)>) -> Vec<(String, u32)> {
        let content = Arc::clone(&self.content);
        let mut items = Vec::new();
        for (item, quantity) in loot {
            let Some(table) = item.strip_prefix(TABLE_PREFIX) else {
                items.push((item, quantity));
                continue;
            };
            for _ in 0..quantity {
                let reached = self.reached_loot_limits(number);
                let context = self.loot_context(number, None, &reached);
                let dice = &mut self.dice;
                let rolled = content
                    .loot
                    .roll(table, context, &mut |below| dice.below(below));
                items.extend(self.limit_loot(number, rolled));
            }
        }
        items
    }

    /// The client of player `entity` ends interaction `interaction` at `now_ms`: a gather that
    /// still runs stops without a harvest, unless its end time has come (then it ends with its
    /// harvest) (the own gather state done, as retail sends it after a cancel);
    /// after a gather's own end the client's cancel is its acknowledgement and changes nothing.
    /// Both get the gatherable's 2724, as retail sends it (loot note: after 98 of 126 retail
    /// cancels of a running gather and 1,561 of 3,042 after the end; why not after the others
    /// is Unknown).
    pub(super) fn cancel_gather(&mut self, entity: Entity, interaction: u64, now_ms: u64) {
        let Some(number) = self.world.get::<Player>(entity).map(|player| player.number) else {
            return;
        };
        let mut running = self.open_gathers.iter().position(|gather| {
            gather.number == number && gather.request.interaction == interaction
        });
        // A cancel at or after the gather's end time comes as the client's own end, before the
        // tick that ends it here: the gather ends, it is not stopped, and the cancel is its
        // acknowledgement (live 2026-10-29: the watchtower crate's 2,541 ms gather got its
        // cancel at 2,571 ms and gave nothing).
        if let Some(at) = running
            && now_ms >= self.open_gathers[at].end_ms
        {
            let gather = self.open_gathers.remove(at);
            self.end_gather(&gather, now_ms);
            running = None;
        }
        let request = match running {
            Some(at) => {
                let gather = self.open_gathers.remove(at);
                if gather.mob.is_none() {
                    self.nodes.release(&gather.request.target);
                }
                if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                    let stopped = gather.done_state(player.gathers);
                    player.extra_members.push(stopped);
                    player.gathers = player.gathers.saturating_add(1);
                }
                gather.request
            }
            None => {
                let Some(mut player) = self.world.get_mut::<Player>(entity) else {
                    return;
                };
                match player.ended_gather {
                    Some(ended) if ended.interaction == interaction => {
                        player.ended_gather = None;
                        ended
                    }
                    _ => return,
                }
            }
        };
        if let Some(player) = self.world.get::<Player>(entity) {
            let interrupted = gather_interrupted(request);
            send_now(
                &mut self.combat_messages,
                &player.connection,
                &[interrupted],
            );
        }
    }
}

/// One gather node that left its default state.
#[derive(Debug, Clone, PartialEq)]
struct Node {
    gde_ref: [u8; RAW_ID_LEN],
    /// The level gatherable point it stands at.
    point: usize,
    /// Where its first gatherer stood (Inferred: a node is within reach of its gatherer).
    position: Position,
    /// The player who gathers it now.
    gatherer: Option<u64>,
    /// When it grows back, while it is depleted.
    respawn_at_ms: Option<u64>,
    /// Where its entity holds the states sent to the players near it; `None`: none is sent.
    layout: Option<NodeLayout>,
    /// When the timeline of a [`NodeLayout::BreakingWallsCannon`] stops, while it plays.
    timeline_ends_at_ms: Option<u64>,
    /// The node version at which its timeline last started or stopped.
    timeline_version: u32,
    /// Counts its state changes, so each player gets each one once.
    version: u32,
    /// Its depletion is not saved yet.
    unsaved: bool,
}

/// A depleted gather node as the world database keeps it, so a sim restart does not grow it
/// back before its time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DepletedNode {
    /// Its id (`GdeRef`).
    pub gde_ref: [u8; RAW_ID_LEN],
    /// The level gatherable point it stands at.
    pub point: usize,
    /// Where its first gatherer stood.
    pub position: Position,
    /// When it grows back, Unix ms.
    pub respawn_at_unix_ms: u64,
}

impl Node {
    const fn depleted(&self) -> bool {
        self.respawn_at_ms.is_some()
    }

    /// Its card and states (`with_card`), or its states alone, in the slots of its layout.
    fn members(&self, layout: NodeLayout, with_card: bool) -> Vec<BundleMember> {
        let (state, enabled) = if self.depleted() {
            (DEPLETED, false)
        } else {
            (HEALTHY, true)
        };
        let (controller, interact) = match layout {
            NodeLayout::Plant => (CONTROLLER_COMPONENT, INTERACT_COMPONENT),
            NodeLayout::BreakingWallsCannon => {
                (CANNON_CONTROLLER_COMPONENT, CANNON_INTERACT_COMPONENT)
            }
        };
        let card = with_card.then_some(BundleMember {
            component: CARD_COMPONENT,
            state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                asset: None,
                gde_ref: Some(self.gde_ref),
            }),
        });
        let states = [
            BundleMember {
                component: controller,
                state: MemberState::GatherableController(GatherableControllerState {
                    gatherable_state: Some(state),
                }),
            },
            BundleMember {
                component: interact,
                state: MemberState::Interact(InteractState {
                    enabled: Some(enabled),
                    has_interactors: None,
                }),
            },
        ];
        let cannon = match layout {
            NodeLayout::Plant => Vec::new(),
            NodeLayout::BreakingWallsCannon => self.cannon_members(with_card),
        };
        card.into_iter().chain(states).chain(cannon).collect()
    }

    /// The Fire Cannon's own states: its timeline, and with its card the projectile spawner and
    /// the ownership, as every retail card sent them (gimmick scan of 2026-11-12).
    fn cannon_members(&self, with_card: bool) -> Vec<BundleMember> {
        let version = u64::from(self.timeline_version);
        let timeline = if self.timeline_ends_at_ms.is_some() {
            TimelineState::start(version)
        } else {
            TimelineState::end(version, cannon_timeline_end_s())
        };
        let card_states = with_card.then(|| {
            [
                BundleMember {
                    component: CANNON_SPAWNER_COMPONENT,
                    state: MemberState::Recorded {
                        class: PROJECTILE_SPAWNER_TYPE_INDEX,
                        body: CANNON_SPAWNER_BODY.to_vec(),
                    },
                },
                BundleMember {
                    component: CANNON_OWNERSHIP_COMPONENT,
                    state: MemberState::Ownership(unowned(
                        CANNON_PERMISSION_RULE,
                        CANNON_STRUCTURE_NAME,
                    )),
                },
            ]
        });
        timeline_member(CANNON_TIMELINE_COMPONENT, &timeline)
            .into_iter()
            .chain(card_states.into_iter().flatten())
            .collect()
    }
}

/// Why a gather request is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NodeBusy {
    /// Another player gathers it.
    Gathered,
    /// It is depleted.
    Depleted,
    /// The request names the level point by another node id than the one it first had, or the
    /// node id of another point.
    OtherNode,
}

/// The gather nodes of one sim.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Nodes {
    nodes: Vec<Node>,
    by_ref: HashMap<[u8; RAW_ID_LEN], usize>,
    /// The node of each level gatherable point (its index in
    /// [`crate::player_content::PlayerContent::gather_points`]).
    by_point: HashMap<usize, usize>,
}

impl Nodes {
    /// Player `player` starts to gather node `gde_ref`, the level's gatherable point `point` at
    /// `position`; refused while another player gathers it, while it is depleted, and when the
    /// point and the node id do not name the same node they named first (a new id cannot skip a
    /// depleted node). `layout`: where its entity holds the states sent to the players near it.
    pub(super) fn start(
        &mut self,
        gde_ref: [u8; RAW_ID_LEN],
        point: usize,
        position: Position,
        player: u64,
        layout: Option<NodeLayout>,
    ) -> Result<(), NodeBusy> {
        let index = match (self.by_ref.get(&gde_ref), self.by_point.get(&point)) {
            (Some(by_ref), Some(by_point)) if by_ref == by_point => *by_ref,
            (None, None) => {
                self.nodes.push(Node {
                    gde_ref,
                    point,
                    position,
                    gatherer: None,
                    respawn_at_ms: None,
                    layout,
                    timeline_ends_at_ms: None,
                    timeline_version: 0,
                    version: 0,
                    unsaved: false,
                });
                let index = self.nodes.len() - 1;
                self.by_ref.insert(gde_ref, index);
                self.by_point.insert(point, index);
                index
            }
            _ => return Err(NodeBusy::OtherNode),
        };
        let node = &mut self.nodes[index];
        if node.depleted() {
            return Err(NodeBusy::Depleted);
        }
        if node.gatherer.is_some_and(|gatherer| gatherer != player) {
            return Err(NodeBusy::Gathered);
        }
        node.gatherer = Some(player);
        Ok(())
    }

    /// The gather of node `gde_ref` ends without a harvest (a cancel).
    pub(super) fn release(&mut self, gde_ref: &[u8; RAW_ID_LEN]) {
        if let Some(node) = self
            .by_ref
            .get(gde_ref)
            .map(|index| &mut self.nodes[*index])
        {
            node.gatherer = None;
        }
    }

    /// The level point that node id `gde_ref` names among the points `near` (in reach, nearest
    /// first): the point it named first when that one is near, else, for an id not seen yet,
    /// the nearest point no other id holds. `None` when it names no near point.
    pub(super) fn point_for(
        &self,
        gde_ref: &[u8; RAW_ID_LEN],
        near: &[(usize, Position)],
    ) -> Option<(usize, Position)> {
        match self.by_ref.get(gde_ref) {
            Some(node) => near
                .iter()
                .find(|(point, _)| self.by_point.get(point) == Some(node))
                .copied(),
            None => near
                .iter()
                .find(|(point, _)| !self.by_point.contains_key(point))
                .copied(),
        }
    }

    /// Where node `gde_ref` stands; `None` for a node nobody gathered.
    pub(super) fn position_of(&self, gde_ref: &[u8; RAW_ID_LEN]) -> Option<Position> {
        self.by_ref
            .get(gde_ref)
            .map(|index| self.nodes[*index].position)
    }

    /// Node `gde_ref` was harvested at `now_ms`: depleted until `respawn_at_ms`. A Fire Cannon's
    /// timeline plays from now for [`CANNON_TIMELINE_MS`].
    pub(super) fn deplete(&mut self, gde_ref: &[u8; RAW_ID_LEN], now_ms: u64, respawn_at_ms: u64) {
        if let Some(node) = self
            .by_ref
            .get(gde_ref)
            .map(|index| &mut self.nodes[*index])
        {
            node.gatherer = None;
            node.respawn_at_ms = Some(respawn_at_ms);
            node.version += 1;
            node.unsaved = true;
            if node.layout == Some(NodeLayout::BreakingWallsCannon) {
                node.timeline_ends_at_ms = Some(now_ms.saturating_add(CANNON_TIMELINE_MS));
                node.timeline_version = node.version;
            }
        }
    }

    /// The depletions since the last call, for the world database; `unix_of` turns a sim ms
    /// into Unix ms.
    pub(super) fn take_unsaved(&mut self, unix_of: impl Fn(u64) -> u64) -> Vec<DepletedNode> {
        self.nodes
            .iter_mut()
            .filter_map(|node| {
                if !std::mem::take(&mut node.unsaved) {
                    return None;
                }
                Some(DepletedNode {
                    gde_ref: node.gde_ref,
                    point: node.point,
                    position: node.position,
                    respawn_at_unix_ms: unix_of(node.respawn_at_ms?),
                })
            })
            .collect()
    }

    /// Holds again the saved depleted node `saved`, which grows back at sim ms `respawn_at_ms`;
    /// a node or point this sim already holds is left as it is. Its depleted state goes to the
    /// players near it in the slots of `layout`, its gatherable's.
    pub(super) fn restore(
        &mut self,
        saved: &DepletedNode,
        respawn_at_ms: u64,
        layout: Option<NodeLayout>,
    ) {
        if self.by_ref.contains_key(&saved.gde_ref) || self.by_point.contains_key(&saved.point) {
            return;
        }
        self.nodes.push(Node {
            gde_ref: saved.gde_ref,
            point: saved.point,
            position: saved.position,
            gatherer: None,
            respawn_at_ms: Some(respawn_at_ms),
            layout,
            timeline_ends_at_ms: None,
            timeline_version: 1,
            version: 1,
            unsaved: false,
        });
        let index = self.nodes.len() - 1;
        self.by_ref.insert(saved.gde_ref, index);
        self.by_point.insert(saved.point, index);
    }

    /// The depleted nodes whose time came at `now_ms` grow back, and the Fire Cannon timelines
    /// whose time came stop at their end.
    pub(super) fn tick(&mut self, now_ms: u64) {
        for node in &mut self.nodes {
            if node.timeline_ends_at_ms.is_some_and(|at| now_ms >= at) {
                node.timeline_ends_at_ms = None;
                node.version += 1;
                node.timeline_version = node.version;
            }
            if node.respawn_at_ms.is_some_and(|at| now_ms >= at) {
                node.respawn_at_ms = None;
                node.version += 1;
            }
        }
    }

    /// The slot keys of the nodes shown to a player at `position`: those that left their default
    /// state, within `radius_m` (the interest radius).
    pub(super) fn numbers_near(
        &self,
        position: Position,
        radius_m: f32,
    ) -> impl Iterator<Item = u64> + '_ {
        (0_u64..)
            .zip(&self.nodes)
            .filter(move |(_, node)| {
                node.layout.is_some()
                    && node.version > 0
                    && node.position.horizontal_distance(position) <= radius_m
            })
            .map(|(index, _)| GATHERABLE_ENTITY_BASE + index)
    }

    /// The group of the node of slot key `number` in `slot` for a player who got its version
    /// `sent`: its card and states when the slot is new, its states after a change, else none.
    /// Updates `sent`.
    pub(super) fn group(
        &self,
        number: u64,
        slot: Slot,
        sent: &mut HashMap<u64, u32>,
    ) -> Option<BundleGroup> {
        let index = usize::try_from(number.checked_sub(GATHERABLE_ENTITY_BASE)?).ok()?;
        let node = self.nodes.get(index)?;
        let layout = node.layout?;
        if !slot.new && sent.get(&number) == Some(&node.version) {
            return None;
        }
        sent.insert(number, node.version);
        Some(BundleGroup {
            interest: u64::from(slot.interest),
            members: node.members(layout, slot.new),
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use nw_content::gathering::{
        CyclicGatherables, GatherRule, GatherRules, Gatherables, ToolEfficiencies, ToolEfficiency,
    };
    use nw_content::progression::{Progression, RankPerks, TradeskillRankRow, TradeskillSource};
    use nw_content::rewards::{EventGrant, EventGrants};
    use nw_formats::objectstream::lowercase_crc32;

    use super::*;
    use crate::placed_points::PlacedPoints;
    use crate::player_content::PlayerContent;
    use crate::world::gear;
    use crate::world::tests::{NOW, SERVER_TIME, admit_at, connection, world_with_spawns};

    /// The retail threshold effects of Strength 25 and 250 and Constitution 50 (build 25810808)
    /// add mining efficiency, yield and luck to a player that holds their abilities.
    #[test]
    fn the_attribute_threshold_effects_add_gathering_efficiency_yield_and_luck() {
        use nw_content::attribute_bonuses::AttributeBonuses;
        use nw_content::character_points::{CONSTITUTION, STRENGTH};
        use nw_content::generated::{StatusEffectData, Tables};
        use nw_content::status_effects::StatusEffectRules;
        use nw_content::table::Table;

        const PLAYER: u64 = 7;
        const MINING: &str = "Mining";
        /// A threshold every attribute value reaches, so the admitted player holds them.
        const ANY_VALUE: u32 = 0;
        let effect = |row: serde_json::Value| -> StatusEffectData {
            serde_json::from_value(row).expect("effect row")
        };
        let effects = vec![
            effect(
                serde_json::json!({"status_id": "ATB_Str_Bonus_25_2_Effect", "eff_mining": 0.1}),
            ),
            effect(
                serde_json::json!({"status_id": "ATB_Str_Bonus_250_2_Effect", "mult_mining": 0.1}),
            ),
            effect(
                serde_json::json!({"status_id": "ATB_Con_Bonus_50_2_Effect", "rol_mining": 150}),
            ),
        ];
        let mut world = world_with_spawns(Vec::new());
        let mut content = (*world.content).clone();
        content.status_effects = StatusEffectRules::from_tables(&Tables {
            status_effect_data: vec![Table {
                source_path: String::new(),
                unique_id: String::new(),
                rows: effects,
            }],
            ..Tables::default()
        });
        let ability = |id: &str| id.to_owned();
        content.attribute_bonuses = AttributeBonuses::build(
            [
                (
                    STRENGTH,
                    ANY_VALUE,
                    ability("Str_Bonus_25_2,Str_Bonus_250_2"),
                ),
                (CONSTITUTION, ANY_VALUE, ability("Con_Bonus_50_2")),
            ],
            [],
        )
        .with_equip_effects([
            (
                ability("Str_Bonus_25_2"),
                ability("ATB_Str_Bonus_25_2_Effect"),
            ),
            (
                ability("Str_Bonus_250_2"),
                ability("ATB_Str_Bonus_250_2_Effect"),
            ),
            (
                ability("Con_Bonus_50_2"),
                ability("ATB_Con_Bonus_50_2_Effect"),
            ),
        ]);
        world.content = Arc::new(content);
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
        let entity = world.by_connection[&connection(PLAYER)];
        let efficiency = world.attribute_effect_sum(entity, |rule| {
            rule.gather_efficiency
                .iter()
                .filter(|(skill, _)| skill.eq_ignore_ascii_case(MINING))
                .map(|(_, value)| *value)
                .sum()
        });
        assert!((efficiency - 0.1).abs() < 1e-9);
        let bonuses = world.loot_bonuses(PLAYER, &[], Some(MINING));
        assert_eq!(bonuses.luck, 150);
        assert!((bonuses.yield_scale - 1.1).abs() < 1e-9);
        let kill = world.loot_bonuses(PLAYER, &[], None);
        assert_eq!(kill.luck, 0, "no global luck");
    }

    #[test]
    fn a_yield_share_grows_each_stack_and_rolls_its_fraction() {
        // `Token_Proficiency_1` `MULT_*` 0.1.
        const PROFICIENCY: f64 = 1.1;
        let mut low = |_: u32| 0;
        let mut high = |n: u32| n - 1;
        assert_eq!(
            with_yield(10, PROFICIENCY, &mut high),
            11,
            "no fraction to roll"
        );
        assert_eq!(
            with_yield(1, PROFICIENCY, &mut low),
            2,
            "a roll under a tenth"
        );
        assert_eq!(with_yield(1, PROFICIENCY, &mut high), 1);
        assert_eq!(
            with_yield(4, NEUTRAL_MODIFIER, &mut low),
            4,
            "no share, no roll"
        );
        assert_eq!(with_yield(3, -1.0, &mut low), 0);
    }

    const FLINT: [u8; RAW_ID_LEN] = [0x5b; RAW_ID_LEN];
    /// The level point of the flint node.
    const FLINT_POINT: usize = 0;
    /// The node state of the test nodes goes to the players near them.
    const SENT: Option<NodeLayout> = Some(NodeLayout::Plant);
    /// When the test nodes are harvested.
    const HARVESTED_AT_MS: u64 = 1_500;
    /// The interest radius of the test views.
    const RADIUS_M: f32 = 100.0;
    const AT: Position = Position {
        x: 7305.0,
        y: 3093.0,
        z: 20.0,
    };
    const PLAYER: u64 = 7;
    const RESPAWN_AT_MS: u64 = 61_500;

    fn state_of(group: &BundleGroup) -> Option<u8> {
        group.members.iter().find_map(|member| match &member.state {
            MemberState::GatherableController(state) => state.gatherable_state,
            _ => None,
        })
    }

    #[test]
    fn a_level_point_keeps_the_node_id_it_first_had() {
        const OTHER: [u8; RAW_ID_LEN] = [0x6c; RAW_ID_LEN];
        let mut nodes = Nodes::default();
        assert_eq!(nodes.start(FLINT, FLINT_POINT, AT, PLAYER, SENT), Ok(()));
        nodes.deplete(&FLINT, HARVESTED_AT_MS, RESPAWN_AT_MS);
        assert_eq!(
            nodes.start(OTHER, FLINT_POINT, AT, PLAYER, SENT),
            Err(NodeBusy::OtherNode),
            "a new id does not skip the depleted node"
        );
        assert_eq!(
            nodes.start(FLINT, FLINT_POINT + 1, AT, PLAYER, SENT),
            Err(NodeBusy::OtherNode),
            "the id of one node does not gather another"
        );
        assert_eq!(
            nodes.start(OTHER, FLINT_POINT + 1, AT, PLAYER, SENT),
            Ok(())
        );
    }

    #[test]
    fn a_node_of_another_layout_is_never_sent() {
        let mut nodes = Nodes::default();
        assert_eq!(nodes.start(FLINT, FLINT_POINT, AT, PLAYER, None), Ok(()));
        nodes.deplete(&FLINT, HARVESTED_AT_MS, RESPAWN_AT_MS);
        assert_eq!(
            nodes.start(FLINT, FLINT_POINT, AT, PLAYER, None),
            Err(NodeBusy::Depleted),
            "the server still keeps it depleted"
        );
        assert_eq!(
            nodes.numbers_near(AT, RADIUS_M).count(),
            0,
            "no card for a node whose slots the server does not know"
        );
        nodes.tick(RESPAWN_AT_MS);
        assert_eq!(
            nodes.numbers_near(AT, RADIUS_M).count(),
            0,
            "nor after it grows back"
        );
    }

    #[test]
    fn a_depletion_is_saved_once_and_a_restored_node_stays_depleted_until_its_time() {
        const UNIX_OF_ZERO_MS: u64 = 1_798_761_600_000;
        let mut nodes = Nodes::default();
        assert_eq!(nodes.start(FLINT, FLINT_POINT, AT, PLAYER, SENT), Ok(()));
        nodes.deplete(&FLINT, HARVESTED_AT_MS, RESPAWN_AT_MS);
        let saved = nodes.take_unsaved(|at_ms| at_ms + UNIX_OF_ZERO_MS);
        assert_eq!(
            saved,
            [DepletedNode {
                gde_ref: FLINT,
                point: FLINT_POINT,
                position: AT,
                respawn_at_unix_ms: RESPAWN_AT_MS + UNIX_OF_ZERO_MS,
            }]
        );
        assert!(nodes.take_unsaved(|at_ms| at_ms).is_empty(), "saved once");
        // A new sim holds it again: depleted, shown, and refused until its time.
        let mut restarted = Nodes::default();
        restarted.restore(&saved[0], RESPAWN_AT_MS, SENT);
        assert_eq!(
            restarted.start(FLINT, FLINT_POINT, AT, PLAYER, SENT),
            Err(NodeBusy::Depleted)
        );
        assert_eq!(restarted.numbers_near(AT, RADIUS_M).count(), 1);
        restarted.tick(RESPAWN_AT_MS);
        assert_eq!(
            restarted.start(FLINT, FLINT_POINT, AT, PLAYER, SENT),
            Ok(())
        );
        assert!(
            restarted.take_unsaved(|at_ms| at_ms).is_empty(),
            "a load is no new save"
        );
    }

    #[test]
    fn a_node_is_gathered_once_until_it_grows_back() {
        let mut nodes = Nodes::default();
        assert_eq!(nodes.start(FLINT, FLINT_POINT, AT, PLAYER, SENT), Ok(()));
        assert_eq!(
            nodes.start(FLINT, FLINT_POINT, AT, PLAYER + 1, SENT),
            Err(NodeBusy::Gathered)
        );
        assert_eq!(
            nodes.numbers_near(AT, RADIUS_M).count(),
            0,
            "a node in its default state is not shown"
        );
        nodes.deplete(&FLINT, HARVESTED_AT_MS, RESPAWN_AT_MS);
        assert_eq!(
            nodes.start(FLINT, FLINT_POINT, AT, PLAYER, SENT),
            Err(NodeBusy::Depleted)
        );
        let number = GATHERABLE_ENTITY_BASE;
        assert_eq!(
            nodes.numbers_near(AT, RADIUS_M).collect::<Vec<_>>(),
            [number]
        );
        let mut sent = HashMap::new();
        let new = Slot {
            interest: 9,
            new: true,
        };
        let card = nodes.group(number, new, &mut sent).expect("card");
        assert_eq!(card.members.len(), 3, "card, controller, interact");
        assert_eq!(state_of(&card), Some(DEPLETED));
        let kept = Slot { new: false, ..new };
        assert_eq!(nodes.group(number, kept, &mut sent), None, "sent once");
        nodes.tick(RESPAWN_AT_MS - 1);
        assert_eq!(
            nodes.start(FLINT, FLINT_POINT, AT, PLAYER, SENT),
            Err(NodeBusy::Depleted)
        );
        nodes.tick(RESPAWN_AT_MS);
        let grown = nodes.group(number, kept, &mut sent).expect("change");
        assert_eq!(grown.members.len(), 2, "the states without the card");
        assert_eq!(state_of(&grown), Some(HEALTHY));
        assert_eq!(
            nodes.start(FLINT, FLINT_POINT, AT, PLAYER + 1, SENT),
            Ok(())
        );
        nodes.release(&FLINT);
        assert_eq!(
            nodes.start(FLINT, FLINT_POINT, AT, PLAYER, SENT),
            Ok(()),
            "a cancel frees it"
        );
        let far = Position {
            x: AT.x + RADIUS_M + 1.0,
            ..AT
        };
        assert_eq!(nodes.numbers_near(far, RADIUS_M).count(), 0);
    }

    /// The player's number.
    const NUMBER: u64 = 1;
    /// A mining node, a node that asks for Mining rank 5 and a barrel that asks for a key.
    const VEIN: &str = "IronVeinSmall";
    const HIGH_VEIN: &str = "OreVeinOrichalcumSmall";
    const BARREL: &str = "0402_GruelBarrel";
    const KEY: &str = "0402_0401_BileSoakedMeat";
    const PICK: &str = "2hPickT1";
    const MINING_EVENT: &str = "Gather_MiningT1";
    /// `Gather_MiningT1`'s Mining experience (build 25810808).
    const MINING_REWARD: f64 = 64.0;
    /// The vein's `BaseGatherTime` and the pick's efficiency.
    const VEIN_MS: u64 = 8_000;
    /// The client's cancel after a water gather's end (live 2026-11-03: 22 ms; retail 20 ms).
    const CLIENT_CANCEL_MS: u64 = 22;
    const PICK_EFFICIENCY: f64 = 2.0;
    /// The Mining slot of the paperdoll ([`TOOL_SLOTS`]).
    const MINING_SLOT: u32 = 36;

    fn gatherable(id: &str, by_hand: bool) -> Gatherable {
        Gatherable {
            id: id.to_owned(),
            gather_ms: VEIN_MS,
            respawn_ms: (RESPAWN_AT_MS, RESPAWN_AT_MS),
            loot_table: None,
            loot_container: false,
            tradeskill: (!by_hand).then(|| "Mining".to_owned()),
            tool_required: !by_hand,
            by_hand,
            game_event: (!by_hand).then(|| MINING_EVENT.to_owned()),
        }
    }

    fn content() -> PlayerContent {
        let rank = |rank: i64, cap: f64| TradeskillRankRow {
            rank,
            cap,
            perks: RankPerks {
                gathering_efficiency: 0.0,
                recipe_level: 0,
                ..RankPerks::default()
            },
        };
        PlayerContent {
            kits: gear::tests::kits_with(&[
                (PICK, "EquippableTool+Axe+PickAxe"),
                (KEY, "Resource+Quest"),
            ]),
            gatherables: Gatherables::build([
                gatherable(VEIN, false),
                gatherable(HIGH_VEIN, false),
                gatherable(BARREL, true),
            ]),
            gather_rules: GatherRules::build([
                (
                    HIGH_VEIN.to_owned(),
                    GatherRule {
                        required_rank: 5,
                        required_item: None,
                    },
                ),
                (
                    BARREL.to_owned(),
                    GatherRule {
                        required_rank: 0,
                        required_item: Some((KEY.to_owned(), 1)),
                    },
                ),
            ]),
            tools: ToolEfficiencies::build([(
                PICK.to_owned(),
                ToolEfficiency {
                    efficiency: (PICK_EFFICIENCY, PICK_EFFICIENCY),
                    gear_score: (0, 0),
                },
            )]),
            progression: Progression::default().with_tradeskills([TradeskillSource {
                id: "Mining".to_owned(),
                max_levels: vec![200],
                ranks: vec![rank(0, 100.0), rank(1, 105.0)],
            }]),
            event_grants: EventGrants::build([(
                MINING_EVENT.to_owned(),
                EventGrant {
                    progression: Some(("Mining".to_owned(), MINING_REWARD)),
                    coins: (0, 0),
                    territory_standing: 0.0,
                    achievement: None,
                    ..EventGrant::default()
                },
            )]),
            ..PlayerContent::default()
        }
    }

    /// Level points of each of `kinds` at `at`, [`NODES_PER_KIND`] of each a metre apart.
    pub(crate) fn points_at(kinds: &[&str], at: Position) -> PlacedPoints {
        PlacedPoints::of(kinds.iter().flat_map(|kind| {
            (0..NODES_PER_KIND).map(move |step| {
                #[allow(clippy::cast_precision_loss, reason = "a few metres")]
                let east = step as f32;
                (
                    lowercase_crc32(kind),
                    Position {
                        x: at.x + east,
                        ..at
                    },
                )
            })
        }))
    }

    /// Nodes of one kind the test level has at the player.
    const NODES_PER_KIND: usize = 3;

    /// The level of `world` gets nodes of each of `kinds` at the feet of player `number`.
    pub(crate) fn place_nodes(world: &mut SimWorld, number: u64, kinds: &[&str]) {
        let at = world
            .player_by_number(number)
            .and_then(|entity| world.world.get::<Body>(entity))
            .map(|body| body.motion.position())
            .expect("body");
        world.content = Arc::new(PlayerContent {
            gather_points: points_at(kinds, at),
            ..(*world.content).clone()
        });
    }

    /// A world with the test content and one player wearing the pick, with the test nodes at
    /// its feet.
    fn admitted() -> (SimWorld, Entity) {
        let mut world = world_with_spawns(Vec::new());
        world.content = Arc::new(content());
        let mut admit = admit_at(NUMBER, 0.0, true);
        admit.backstory = Some(gear::tests::BACKSTORY.to_owned());
        world.admit(&admit, NOW, SERVER_TIME).expect("admit");
        let entity = world.by_connection[&connection(NUMBER)];
        place_nodes(&mut world, NUMBER, &[VEIN, HIGH_VEIN, BARREL]);
        let pick = world.content.kits.item(PICK).cloned().expect("pick");
        {
            let mut player = world.world.get_mut::<Player>(entity).expect("player");
            player
                .gear
                .as_mut()
                .expect("gear")
                .renew(&pick, 1, MINING_SLOT, 0);
        }
        (world, entity)
    }

    fn request(interaction: u64, target: u8, gatherable: &str) -> InteractionRequest {
        InteractionRequest {
            target: [target; RAW_ID_LEN],
            target_entity: interaction,
            interaction,
            kind: nw_messages::client_input::GATHER_KIND,
            gatherable: Some(lowercase_crc32(gatherable)),
        }
    }

    fn mining(world: &SimWorld, entity: Entity) -> u32 {
        let mastery = world
            .content
            .progression
            .tradeskill("Mining")
            .expect("mining");
        world
            .world
            .get::<Player>(entity)
            .map_or(0, |player| player.masteries.experience(mastery))
    }

    #[test]
    fn a_tool_gather_runs_at_the_tools_efficiency_and_gives_its_tradeskill_experience() {
        let (mut world, entity) = admitted();
        world.gather_open_world(entity, NUMBER, request(1, 0x11, VEIN), &[], NOW);
        let [gather] = world.open_gathers.as_slice() else {
            panic!("one gather runs: {:?}", world.open_gathers);
        };
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "a gather time of seconds"
        )]
        let expected_ms = (VEIN_MS as f64 / PICK_EFFICIENCY) as u64;
        assert_eq!(
            gather.end_ms,
            NOW + expected_ms,
            "8 s of work at efficiency 2"
        );
        world.gathers_due(NOW + expected_ms);
        assert!(world.open_gathers.is_empty());
        assert_eq!(
            mining(&world, entity),
            64,
            "Gather_MiningT1 under a neutral multiplier"
        );
    }

    /// A gather with the Strength 300 threshold's single swing chance at certainty ends at the
    /// next gather step; its loot and experience come as after a full gather.
    #[test]
    fn a_single_swing_chance_ends_the_tool_gather_at_the_next_step() {
        use nw_content::attribute_bonuses::AttributeBonuses;
        use nw_content::character_points::STRENGTH;
        use nw_content::generated::{StatusEffectData, Tables};
        use nw_content::status_effects::StatusEffectRules;
        use nw_content::table::Table;

        /// Retail `Str_Bonus_300_2` and its effect (build 25810808), here with a certain chance.
        const ABILITY: &str = "Str_Bonus_300_2";
        const EFFECT: &str = "ATB_Str_Bonus_300_2_Effect";
        const CERTAIN: f64 = 1.0;
        /// A threshold every Strength value reaches, so the admitted player holds it.
        const ANY_VALUE: u32 = 0;
        let (mut world, entity) = admitted();
        let effect: StatusEffectData =
            serde_json::from_value(serde_json::json!({"status_id": EFFECT, "ins_mining": CERTAIN}))
                .expect("effect row");
        let mut content = (*world.content).clone();
        content.status_effects = StatusEffectRules::from_tables(&Tables {
            status_effect_data: vec![Table {
                source_path: String::new(),
                unique_id: String::new(),
                rows: vec![effect],
            }],
            ..Tables::default()
        });
        content.attribute_bonuses =
            AttributeBonuses::build([(STRENGTH, ANY_VALUE, ABILITY.to_owned())], [])
                .with_equip_effects([(ABILITY.to_owned(), EFFECT.to_owned())]);
        world.content = Arc::new(content);
        let vein = world
            .content
            .gatherables
            .get(lowercase_crc32(VEIN))
            .cloned();
        assert_eq!(
            world.instant_gather_chance(entity, &vein.expect("vein")),
            CERTAIN
        );
        world.gather_open_world(entity, NUMBER, request(1, 0x11, VEIN), &[], NOW);
        let [gather] = world.open_gathers.as_slice() else {
            panic!("one gather runs: {:?}", world.open_gathers);
        };
        assert_eq!(gather.end_ms, NOW + GATHER_TICK_MS);
        world.gathers_due(NOW + GATHER_TICK_MS);
        assert!(world.open_gathers.is_empty());
        assert_eq!(mining(&world, entity), 64, "the gather's experience");
    }

    #[test]
    fn a_harvest_takes_the_sickle_slot_and_each_tool_its_own() {
        let of = |tradeskill: &str| {
            tool_slot(&Gatherable {
                tradeskill: Some(tradeskill.to_owned()),
                ..gatherable(VEIN, false)
            })
        };
        assert_eq!(
            [
                of("Logging"),
                of("Harvesting"),
                of("Skinning"),
                of("Mining")
            ],
            [Some(33), Some(34), Some(35), Some(36)],
            "CHOPPING, CUTTING, DRESSING and MINING_SLOT"
        );
        assert_eq!(of("Fishing"), None, "the pole has its own path");
    }

    #[test]
    fn a_cancel_at_the_gathers_end_time_ends_it_with_its_harvest() {
        // Live 2026-10-29: the watchtower crate's cancel came 30 ms after its end time, before
        // the tick that ends it, and the crate gave nothing.
        let (mut world, entity) = admitted();
        world.gather_open_world(entity, NUMBER, request(1, 0x11, VEIN), &[], NOW);
        let end_ms = world.open_gathers.first().expect("the gather runs").end_ms;
        world.cancel_gather(entity, 1, end_ms - 1);
        assert_eq!(world.open_gathers.len(), 0, "stopped before its end");
        assert_eq!(mining(&world, entity), 0, "no harvest before its end");
        world.gather_open_world(entity, NUMBER, request(2, 0x12, VEIN), &[], NOW);
        let end_ms = world.open_gathers.first().expect("the gather runs").end_ms;
        world.cancel_gather(entity, 2, end_ms);
        assert!(world.open_gathers.is_empty());
        assert!(
            mining(&world, entity) > 0,
            "the harvest of a gather whose time came"
        );
    }

    #[test]
    fn a_gather_needs_a_level_node_of_its_kind_in_reach() {
        let (mut world, entity) = admitted();
        world.gather_open_world(entity, NUMBER, request(1, 0x12, "SomeOtherVein"), &[], NOW);
        assert!(
            world.open_gathers.is_empty(),
            "a kind the content does not know"
        );
        place_nodes(&mut world, NUMBER, &[]);
        world.gather_open_world(entity, NUMBER, request(2, 0x13, BARREL), &[], NOW);
        assert!(
            world.open_gathers.is_empty(),
            "no barrel of the level lies in reach: a client cannot gather from anywhere"
        );
    }

    #[test]
    fn a_node_out_of_its_time_of_day_takes_no_gather() {
        use nw_world::time_of_day::{TimeOfDayPoints, TimedPoint};

        use crate::world::day_night::TimedPlaces;
        use crate::world::day_night::tests::{DAY_MS, NIGHT_MS, NIGHT_ONLY};
        let (mut world, entity) = admitted();
        world.ids.time_change_time = 0;
        let night_veins = nw_world::WorldFile {
            time_of_day: TimeOfDayPoints {
                gatherables: (0..NODES_PER_KIND)
                    .map(|index| TimedPoint {
                        index: u32::try_from(index).expect("index"),
                        window: NIGHT_ONLY,
                    })
                    .collect(),
                ..TimeOfDayPoints::default()
            },
            ..nw_world::WorldFile::default()
        };
        world.content = Arc::new(PlayerContent {
            time_of_day: TimedPlaces::of(&night_veins),
            ..(*world.content).clone()
        });
        world.server_ms = DAY_MS;
        world.gather_open_world(entity, NUMBER, request(1, 0x12, VEIN), &[], NOW);
        assert!(world.open_gathers.is_empty(), "a night node by day");
        world.server_ms = NIGHT_MS;
        world.gather_open_world(entity, NUMBER, request(2, 0x13, VEIN), &[], NOW);
        assert_eq!(world.open_gathers.len(), 1, "the night node at night");
    }

    #[test]
    fn items_that_do_not_fit_the_bag_drop_at_the_players_feet() {
        let (mut world, entity) = admitted();
        {
            let content = Arc::clone(&world.content);
            let pick = content.kits.item(PICK).expect("pick");
            let mut player = world.world.get_mut::<Player>(entity).expect("player");
            let gear = player.gear.as_mut().expect("gear");
            while gear.has_room_for(&[(pick, 1, true)]) {
                assert!(gear.add_with_values(
                    &content.kits,
                    pick,
                    1,
                    &nw_bus::ItemState::default(),
                    SYNC_MOVED
                ));
            }
        }
        let bags_before = world.bags.counts();
        assert!(
            !world.give_loot(NUMBER, vec![(KEY.to_owned(), 1)], NOW),
            "not every item went into the bag"
        );
        assert_ne!(
            world.bags.counts(),
            bags_before,
            "a bag of dropped items lies there"
        );
        assert!(
            !world.give_loot(NUMBER, vec![("NoSuchItem".to_owned(), 1)], NOW),
            "an unknown item is never given"
        );
    }

    #[test]
    fn a_gather_asks_for_its_rank_and_its_item() {
        let (mut world, entity) = admitted();
        world.gather_open_world(entity, NUMBER, request(1, 0x12, HIGH_VEIN), &[], NOW);
        assert!(world.open_gathers.is_empty(), "Mining rank 0 is below 5");
        world.gather_open_world(entity, NUMBER, request(2, 0x13, BARREL), &[], NOW);
        assert!(world.open_gathers.is_empty(), "no key in the bag");
        let key = world.content.kits.item(KEY).cloned().expect("key");
        {
            let mut player = world.world.get_mut::<Player>(entity).expect("player");
            assert!(player.gear.as_mut().expect("gear").add(&key, 1));
        }
        world.gather_open_world(entity, NUMBER, request(3, 0x13, BARREL), &[], NOW);
        let [gather] = world.open_gathers.as_slice() else {
            panic!("the barrel is gathered: {:?}", world.open_gathers);
        };
        assert_eq!(
            gather.end_ms,
            NOW + VEIN_MS.div_ceil(GATHER_TICK_MS) * GATHER_TICK_MS + GATHER_TICK_MS,
            "by hand: the tick after its time"
        );
    }

    #[test]
    fn a_quest_object_that_is_a_loot_container_gives_its_quest_drop() {
        // Live play test 2026-10-29: Truffle Fever's truffle (`1225_Truffle`, `IsLootContainer`)
        // gave no `1225_0101_Truffle`. Client rows of the quest's gather step.
        use crate::objectives::{TaskBook, TaskRow};
        use crate::player_content::{ObjectiveRow, QuestBook};
        const TRUFFLE: &str = "1225_Truffle";
        const QUEST_TRUFFLE: &str = "1225_0101_Truffle";
        const QUEST: &str = "1225_0101_truffles";
        let (mut world, entity) = admitted();
        let mut content = (*world.content).clone();
        content.gatherables = Gatherables::build([Gatherable {
            loot_container: true,
            ..gatherable(TRUFFLE, true)
        }]);
        content.kits = gear::tests::kits_with(&[
            (PICK, "EquippableTool+Axe+PickAxe"),
            (QUEST_TRUFFLE, "Resource+Quest"),
        ]);
        content.quests = QuestBook::build(
            [ObjectiveRow {
                id: QUEST.to_owned(),
                root: "Truffles".to_owned(),
                ..ObjectiveRow::default()
            }],
            TaskBook::new(vec![TaskRow {
                task_id: "Truffles".to_owned(),
                kind: "TaskGatherAndReturnItems".to_owned(),
                item_name: Some(QUEST_TRUFFLE.to_owned()),
                item_drop_vc: Some(TRUFFLE.to_owned()),
                item_drop_probability: Some(100),
                target_qty: Some(5),
                ..TaskRow::default()
            }]),
        );
        world.content = Arc::new(content);
        place_nodes(&mut world, NUMBER, &[TRUFFLE]);
        world.start_quest(NUMBER, QUEST, NOW).expect("quest");
        world.gather_open_world(entity, NUMBER, request(1, 0x16, TRUFFLE), &[], NOW);
        let end_ms = world
            .open_gathers
            .first()
            .expect("the truffle is gathered")
            .end_ms;
        world.gathers_due(end_ms);
        let truffles: u32 = world
            .world
            .get::<Player>(entity)
            .and_then(|player| player.gear.as_ref())
            .map(super::super::Gear::carried)
            .unwrap_or_default()
            .iter()
            .filter(|item| item.item_id == QUEST_TRUFFLE)
            .map(|item| item.quantity)
            .sum();
        assert_eq!(truffles, 1, "the quest truffle is in the bag");
    }

    #[test]
    fn a_cyclic_gather_names_its_state_from_its_gatherable_or_the_turns() {
        // Client rows: a puzzle stone shows one gatherable in its 4 states, the staff ritual one
        // gatherable per state; retail `99c_msq_03_ancientbridge` turned a stone from state 0.
        const STONE: &str = "Cyclic_99C_MSQ_puzzle1_stone2";
        const RITUAL: &str = "Cyclic_09A_M13_ArcaneRitual";
        const RITUAL_1: &str = "09A_M13_ArcaneRitual_1";
        let (mut world, entity) = admitted();
        let mut content = (*world.content).clone();
        content.gatherables = Gatherables::build([gatherable(STONE, true)]);
        content.cyclic_gatherables = CyclicGatherables::build([
            (STONE.to_owned(), vec![Some(STONE.to_owned()); 4]),
            (
                RITUAL.to_owned(),
                vec![None, Some(RITUAL_1.to_owned()), None],
            ),
        ]);
        world.content = Arc::new(content);
        place_nodes(&mut world, NUMBER, &[STONE]);
        world.gather_open_world(entity, NUMBER, request(1, 0x15, STONE), &[], NOW);
        let end_ms = world.open_gathers.first().expect("the stone turns").end_ms;
        world.gathers_due(end_ms);
        let turns = |world: &SimWorld| {
            world
                .world
                .get::<Player>(entity)
                .expect("player")
                .cyclic_turns[&STONE.to_lowercase()]
        };
        assert_eq!(turns(&world), 1, "the gather end turned it");
        let states: Vec<u32> = (0..4)
            .flat_map(|_| world.cyclic_gathers(entity, lowercase_crc32(STONE)))
            .map(|(_, state)| state)
            .collect();
        assert_eq!(states, [1, 2, 3, 0], "the turns go round its 4 states");
        assert_eq!(
            world.cyclic_gathers(entity, lowercase_crc32(RITUAL_1)),
            [(RITUAL.to_owned(), 1)]
        );
    }

    #[test]
    fn a_water_gather_targets_the_player_and_ends_with_a_done_state_and_no_2724() {
        let (mut world, entity) = admitted();
        let mut content = (*world.content).clone();
        content.gatherables = Gatherables::build([gatherable(WATER_GATHERABLE, true)]);
        world.content = Arc::new(content);
        let water = InteractionRequest {
            kind: nw_messages::client_input::WATER_GATHER_KIND,
            gatherable: None,
            ..request(1, 0x14, WATER_GATHERABLE)
        };
        world.gather_water(NUMBER, water, NOW);
        let player = world.world.get::<Player>(entity).expect("player");
        assert_eq!(
            player.extra_members.last(),
            Some(&interactor_target(water.target_entity)),
            "the own interactor's target, no running gather (retail Hive_Dye 70.92 s)"
        );
        let [gather] = world.open_gathers.as_slice() else {
            panic!("one water gather runs: {:?}", world.open_gathers);
        };
        assert!(gather.water);
        let end_ms = gather.end_ms;
        assert_eq!(
            end_ms,
            NOW + VEIN_MS.div_ceil(GATHER_TICK_MS) * GATHER_TICK_MS + GATHER_TICK_MS
        );
        let gathers = world.world.get::<Player>(entity).expect("player").gathers;
        world.gathers_due(end_ms);
        assert!(world.open_gathers.is_empty());
        let player = world.world.get::<Player>(entity).expect("player");
        assert_eq!(player.gathers, gathers + 1);
        assert_eq!(
            player.extra_members.last(),
            Some(&water_gather_ended(gathers)),
            "the done state (retail Hive_Dye 74.05 s: status 3, no gatherable)"
        );
        assert_eq!(world.nodes.numbers_near(AT, RADIUS_M).count(), 0, "no node");
        let sent = world.combat_messages.len();
        world.cancel_gather(entity, water.interaction, end_ms + CLIENT_CANCEL_MS);
        assert_eq!(
            world.combat_messages.len(),
            sent,
            "the client's cancel after the end gets no 2724"
        );
    }

    #[test]
    fn a_second_water_gather_on_the_same_interaction_id_after_a_pause_runs_again() {
        /// Retail `Hive_Dye`: the client repeats every 50 ms; the second gather came 0.5 s
        /// after the first's last request, with the same id.
        const REPEAT_MS: u64 = 50;
        const PAUSE_MS: u64 = 500;
        let (mut world, _) = admitted();
        let mut content = (*world.content).clone();
        content.gatherables = Gatherables::build([gatherable(WATER_GATHERABLE, true)]);
        world.content = Arc::new(content);
        let water = InteractionRequest {
            kind: nw_messages::client_input::WATER_GATHER_KIND,
            gatherable: None,
            ..request(1, 0x14, WATER_GATHERABLE)
        };
        let who = connection(NUMBER);
        world.interact(&who, water, b"", NOW);
        let end_ms = world.open_gathers[0].end_ms;
        let mut at_ms = NOW + REPEAT_MS;
        while at_ms < end_ms {
            world.interact(&who, water, b"", at_ms);
            at_ms += REPEAT_MS;
        }
        assert_eq!(
            world.open_gathers.len(),
            1,
            "the repeats are the same press"
        );
        world.gathers_due(end_ms);
        let cancel = nw_messages::ClientFacetMsg::CancelInteraction {
            header: nw_messages::FacetHeader::of(0, NUMBER),
            target: water.target,
            target_entity: water.target_entity,
            interaction: water.interaction,
        };
        world.facet_message(&who, cancel, end_ms).expect("cancel");
        // Retail: the client still repeats for about 100 ms after its cancel.
        world.interact(&who, water, b"", at_ms);
        world.interact(&who, water, b"", at_ms + REPEAT_MS);
        assert!(
            world.open_gathers.is_empty(),
            "the held press ended with the gather"
        );
        world.interact(&who, water, b"", at_ms + REPEAT_MS + PAUSE_MS);
        assert_eq!(world.open_gathers.len(), 1, "after a pause: a new press");
    }

    #[test]
    fn an_opened_chest_is_locked_for_its_opener_only() {
        use nw_content::chest_lockouts::{ChestLockouts, ContainerRow};
        const CHEST: &str = "SupplyCrate";
        let (mut world, entity) = admitted();
        let mut content = (*world.content).clone();
        content.gatherables = Gatherables::build([Gatherable {
            loot_container: true,
            ..gatherable(CHEST, true)
        }]);
        content.chest_lockouts = ChestLockouts::build([ContainerRow {
            id: CHEST,
            container: true,
            elite: false,
            max_respawn_s: 0.0,
        }]);
        world.content = Arc::new(content);
        place_nodes(&mut world, NUMBER, &[CHEST]);
        world.gather_open_world(entity, NUMBER, request(1, 0x15, CHEST), &[], NOW);
        let end_ms = world
            .open_gathers
            .first()
            .expect("the chest is opened")
            .end_ms;
        world.gathers_due(end_ms);
        let target = [0x15; RAW_ID_LEN];
        let player = world.world.get::<Player>(entity).expect("player");
        assert!(player.loot.opened(&target), "the opener's tracker holds it");
        assert_eq!(
            world.nodes.start(target, FLINT_POINT, AT, PLAYER, SENT),
            Ok(()),
            "the chest is not depleted for others"
        );
        world.nodes.release(&target);
        world.gather_open_world(entity, NUMBER, request(2, 0x15, CHEST), &[], end_ms);
        assert!(world.open_gathers.is_empty(), "locked for its opener");
    }

    /// The gatherable controller and the interact of `group` are in the Fire Cannon's slots.
    fn in_cannon_slots(group: &BundleGroup) -> bool {
        group.members.iter().all(|member| match member.state {
            MemberState::GatherableController(_) => member.component == CANNON_CONTROLLER_COMPONENT,
            MemberState::Interact(_) => member.component == CANNON_INTERACT_COMPONENT,
            _ => true,
        })
    }

    /// The members of a node's group as retail writes them (component, class and body).
    fn member_bytes(group: &BundleGroup) -> Vec<String> {
        group
            .members
            .iter()
            .map(|member| hex::encode(member.encode().expect("encodes")))
            .collect()
    }

    #[test]
    fn a_fire_cannon_plays_its_timeline_and_grows_back_in_its_own_slots() {
        const CANNON: &str = "Gimmick_BreakingWallsCannon_Vignette";
        const CANNON_EVENT: &str = "IO_Gimmick_BreakingWallsCannon_interact";
        /// `BaseGatherTime` 2 and `MinRespawnRate` 31 (build 25810808).
        const CANNON_GATHER_MS: u64 = 2_000;
        const CANNON_RESPAWN_MS: u64 = 31_000;
        // Retail `12a_msq_08_destroyrune` 93.857 s, the first card of the cannon after its use:
        // controller Depleted, interact off, projectile spawner and ownership.
        const RETAIL_CARD_STATES: [&str; 4] = [
            "020c010108",
            "06b22d010100",
            "05390103ffff00",
            "07913201ff000000000000000000000000000000000000003f8000003f8000003f8000003f8000000000\
             3f8000003f8000003f8000003f800000000002010000000000000000000f00000000",
        ];
        // Retail 124.840 s: grown back, the interact on.
        const RETAIL_GROWN: [&str; 2] = ["020c010104", "06b22d010101"];
        let (mut world, entity) = admitted();
        let mut content = (*world.content).clone();
        content.gatherables = Gatherables::build([Gatherable {
            gather_ms: CANNON_GATHER_MS,
            respawn_ms: (CANNON_RESPAWN_MS, CANNON_RESPAWN_MS),
            tradeskill: Some("None".to_owned()),
            game_event: Some(CANNON_EVENT.to_owned()),
            ..gatherable(CANNON, true)
        }]);
        world.content = Arc::new(content);
        place_nodes(&mut world, NUMBER, &[CANNON]);
        world.gather_open_world(entity, NUMBER, request(1, 0x3c, CANNON), &[], NOW);
        let end_ms = world
            .open_gathers
            .first()
            .expect("the cannon is used")
            .end_ms;
        world.gathers_due(end_ms);
        let at = world
            .world
            .get::<Body>(entity)
            .map(|body| body.motion.position())
            .expect("body");
        let number = world
            .nodes
            .numbers_near(at, RADIUS_M)
            .next()
            .expect("the used cannon is shown");
        let mut sent = HashMap::new();
        let new = Slot {
            interest: 9,
            new: true,
        };
        let card = world.nodes.group(number, new, &mut sent).expect("card");
        let node = &world.nodes.nodes[0];
        let playing = timeline_member(
            CANNON_TIMELINE_COMPONENT,
            &TimelineState::start(u64::from(node.timeline_version)),
        )
        .expect("timeline");
        let mut expected: Vec<String> = RETAIL_CARD_STATES
            .iter()
            .map(|hex| (*hex).to_owned())
            .collect();
        expected.push(hex::encode(playing.encode().expect("encodes")));
        let mut bytes = member_bytes(&card);
        assert!(
            matches!(card.members[0].state, MemberState::GdeMetadata(_)),
            "the card first"
        );
        bytes.remove(0);
        bytes.sort_unstable();
        expected.sort_unstable();
        assert_eq!(
            bytes, expected,
            "retail's slots and bodies, the timeline playing"
        );
        let kept = Slot { new: false, ..new };
        world.gathers_due(end_ms + CANNON_TIMELINE_MS - 1);
        assert_eq!(
            world.nodes.group(number, kept, &mut sent),
            None,
            "still firing"
        );
        world.gathers_due(end_ms + CANNON_TIMELINE_MS);
        let stopped = world
            .nodes
            .group(number, kept, &mut sent)
            .expect("timeline end");
        let node = &world.nodes.nodes[0];
        let ended = timeline_member(
            CANNON_TIMELINE_COMPONENT,
            &TimelineState::end(u64::from(node.timeline_version), cannon_timeline_end_s()),
        )
        .expect("timeline");
        assert!(stopped.members.contains(&ended), "stopped at its end");
        assert!(node.depleted(), "not grown back yet");
        world.gathers_due(end_ms + CANNON_RESPAWN_MS);
        let grown = world
            .nodes
            .group(number, kept, &mut sent)
            .expect("grown back");
        let bytes = member_bytes(&grown);
        for retail in RETAIL_GROWN {
            assert!(
                bytes.iter().any(|ours| ours == retail),
                "{retail} in {bytes:?}"
            );
        }
        assert!(in_cannon_slots(&grown), "nothing in the plant slots");
    }

    #[test]
    fn a_restored_node_keeps_the_slots_of_its_gatherable() {
        let mut nodes = Nodes::default();
        let saved = DepletedNode {
            gde_ref: FLINT,
            point: FLINT_POINT,
            position: AT,
            respawn_at_unix_ms: RESPAWN_AT_MS,
        };
        nodes.restore(&saved, RESPAWN_AT_MS, Some(NodeLayout::BreakingWallsCannon));
        let number = nodes.numbers_near(AT, RADIUS_M).next().expect("shown");
        let new = Slot {
            interest: 9,
            new: true,
        };
        let card = nodes.group(number, new, &mut HashMap::new()).expect("card");
        assert!(
            in_cannon_slots(&card),
            "a restored cannon gets no state in the plant slots"
        );
        let mut unsent = Nodes::default();
        unsent.restore(&saved, RESPAWN_AT_MS, None);
        assert_eq!(
            unsent.numbers_near(AT, RADIUS_M).count(),
            0,
            "no layout, no card"
        );
    }

    #[test]
    fn a_crate_rolls_its_content_level_items_at_the_gatherers_loot_level() {
        use nw_content::loot::{BucketItem, LootRow, LootSource, LootTables, PlayerLootLevels};
        // Build 25810808 `ContainerLootSmall` cut to its `ContainerResources` bucket: every
        // item asks for a content level (`MinContLevel`).
        const CRATE: &str = "LootMediumT1";
        const TABLE: &str = "ContainerLootSmall";
        const BUCKET: &str = "ContainerResources";
        const TIER_TAG: &str = "MinContLevel:19-38";
        const LOOT_LEVEL: u32 = 20;
        const MAX_ROLL: i64 = 100_000;
        let row = |id: &str, item: &str, and_or: Option<&str>, max_roll: Option<i64>| LootRow {
            id: id.to_owned(),
            and_or: and_or.map(str::to_owned),
            max_roll,
            cells: [(1, item.to_owned())].into(),
            ..LootRow::default()
        };
        let (mut world, entity) = admitted();
        let mut content = (*world.content).clone();
        content.gatherables = Gatherables::build([Gatherable {
            loot_table: Some(TABLE.to_owned()),
            ..gatherable(CRATE, true)
        }]);
        content.loot = LootTables::build(LootSource {
            rows: vec![
                row(TABLE, &format!("[LBID]{BUCKET}"), Some("OR"), None),
                row(&format!("{TABLE}_Qty"), "1", None, None),
                row(&format!("{TABLE}_Probs"), "0", None, Some(MAX_ROLL)),
            ],
            bucket_items: vec![BucketItem {
                bucket: BUCKET.to_owned(),
                item: KEY.to_owned(),
                quantity: Some("1".to_owned()),
                tags: Some(TIER_TAG.to_owned()),
                match_one: true,
            }],
        });
        content.player_loot_levels = PlayerLootLevels::build([(0, LOOT_LEVEL)]);
        world.content = Arc::new(content);
        place_nodes(&mut world, NUMBER, &[CRATE]);
        world.gather_open_world(entity, NUMBER, request(1, 0x15, CRATE), &[], NOW);
        let end_ms = world
            .open_gathers
            .first()
            .expect("the crate is opened")
            .end_ms;
        world.gathers_due(end_ms);
        let keys: u32 = world
            .world
            .get::<Player>(entity)
            .and_then(|player| player.gear.as_ref())
            .map(gear::Gear::carried)
            .unwrap_or_default()
            .iter()
            .filter(|item| item.item_id == KEY)
            .map(|item| item.quantity)
            .sum();
        assert_eq!(keys, 1, "the crate gives the item of the gatherer's tier");
    }
}

#[cfg(all(test, feature = "local-data"))]
mod retail_tests {
    use std::path::Path;

    use nw_content::ContentBundle;
    use nw_content::bundle::bundle_path;
    use nw_content::client_build::load_client_build;
    use nw_content::gathering::{CrateLoot, CrateVariant};
    use nw_content::loot::{LootContext, LootTables};
    use nw_formats::objectstream::lowercase_crc32;

    use super::gather_levels;

    const CLIENT_BUILD_CONFIG: &str = "config/client-build.toml";
    /// The supply crate of the live play (2026-11-12, Cynefin Mine) and the name its gather
    /// request carries.
    const CRATE: &str = "LootMediumT1";
    const SUPPLIES: &str = "@LootContainer_SuppliesMedium_DisplayName";
    /// A gatherer above loot level 7, where `ContainerLootSmallLeveled_2` rolls
    /// `ContainerLootSmall`.
    const LOOT_LEVEL: u32 = 20;
    /// No facet entity: the crate's table comes from its gatherable and name.
    const NO_FACET: u64 = 0;

    #[test]
    fn the_retail_supply_crate_gives_items_only_with_a_content_level() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let build = load_client_build(&repo.join(CLIENT_BUILD_CONFIG)).expect("client build");
        let dir = repo.join("local-data/content");
        let bundle = ContentBundle::load(&bundle_path(&dir, &build), &build).expect("bundle");
        let loot = LootTables::from_tables(&bundle.tables);
        let crates = CrateLoot::build(
            CrateVariant::from_tables(&bundle.tables),
            std::iter::empty(),
            std::iter::empty(),
        );
        let table = crates
            .table(NO_FACET, lowercase_crc32(CRATE), SUPPLIES)
            .expect("the supply crate's table");
        assert_eq!(table, "ContainerLootSmallLeveled");
        let plain = LootContext {
            enemy: None,
            content: None,
            player: Some(LOOT_LEVEL),
            poi: None,
            tags: &[],
            reached_limits: &[],
        };
        // The middle of every roll.
        let mut middle = |below: u32| below / 2;
        assert!(
            loot.roll(table, plain, &mut middle).items.is_empty(),
            "without a content level the crate gives nothing"
        );
        let gathered = loot.roll(table, gather_levels(plain), &mut middle).items;
        assert!(
            !gathered.is_empty(),
            "the gather's content level gives the crate's items"
        );
    }
}
