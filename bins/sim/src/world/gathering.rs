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
//!
//! A gather of an instanced object ([`nw_content::gathering::InstancedGathers`]: its slice names
//! an instanced loot entity) is the gatherer's own: its start counts no gatherer, it keeps no
//! work, and its end locks the object for its gatherer alone until its loot tracker entry's reset
//! ([`SimWorld::lock_of`]); the object stays for every other player. A gather of any other
//! object counts its node's gatherers, and its end sets the count back to 0
//! ([`super::statics::gather_ended`]). Observed (finding 2026-11-13 "gatherer counts and group
//! gathers"): 2,584 of 2,584 retail gathers of instanced objects sent 0, 795 of 795 of shared ones
//! sent 1.
//!
//! The node of a group gatherable (`NumRequiredToGather` above one) takes several gatherers, and
//! its work goes on only while enough of them gather it ([`GroupWork`]); each gatherer's own
//! state gets the node's new count when one comes or goes ([`super::statics::gatherers_changed`]).
//!
//! A stopped gather keeps its work done when its gatherable keeps it
//! ([`nw_content::gathering::Gatherables::keeps_progress`]: `ResetGatherTimeOnEndInteraction` 0
//! or empty) and its object is shared ([`KeptGathers`]); the next gather of the same object
//! starts with the amount left. Observed (finding 2026-11-13 "gather progress and quest
//! interacts"): the retail sun disk shield's gathers started with 20.0, 10.85 and 3.06 after two
//! interrupts, the work counted in whole steps of [`PROGRESS_STEP_MS`].
//!
//! A gather of an object whose variant is only on quest (`InteractOnlyOnQuest`,
//! [`nw_content::interact_tags::InteractTags::quest_gate`]) is refused to a player whose quests
//! have no active task that names its tag ([`crate::objectives::ObjectiveRun::enables_interact`]).
//! Observed (client `FUN_1435c3ed0`): the client offers such an interact only then; the object
//! itself is shown to every player alike. Inferred: the server refuses as the client does.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use bevy_ecs::entity::Entity;
use nw_content::chest_lockouts::CONTAINER_LOCKOUT_MS;
use nw_content::corpse_times::{DEFAULT_CORPSE_MS, SKINNED_CORPSE_MS, corpse_ms};
use nw_content::gathering::{Gatherable, InstancedObject, ONE_GATHERER};
use nw_content::loot::{LootContext, LootRoll, TABLE_PREFIX, loot_level};
use nw_content::resets::ResetCycle;
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
use super::loot_bag::{EntryRange, LimitsSent};
use super::slots::Slot;
use super::statics::{
    GatherCounters, GatherRun, gather_ended, gather_started, gatherers_changed,
    interactor_target, water_gather_ended,
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
/// The step of a gather's work: a stopped gather keeps the work of its whole steps, each step
/// of this much gather time taking the efficiency's share off the amount left. Observed
/// (`1624_0401_thedemonscauldron`, the sun disk shield, efficiency 1.2362): 7.43 s of gather
/// left 10.852 of 20 (74 steps), 6.34 s more left 3.064 (63 steps); 10 gathers stopped within
/// 0.1 s of their start kept nothing.
const PROGRESS_STEP_MS: u64 = 100;
/// The gatherer count a gather of a shared corpse or start object sends at its start, which one
/// player gathers at a time (Observed: retail 1 for 795 of 795 gathers of a shared open-world
/// object, [`nw_content::gathering::InstancedGathers`]); a node sends its own gatherers.
const SOLE_GATHERER: u8 = 1;
/// The gatherer count of a gather that counts none: an instanced object's (Observed: retail 0 at
/// the start of 2,584 of 2,584 such gathers), and a water gather's, which sends no start state.
const NO_GATHERERS: u8 = 0;
/// Milliseconds per second.
const MS_PER_SECOND: f32 = 1_000.0;
/// The respawn time (`MinRespawnRate`, `MaxRespawnRate`) of a gatherable without one, whose
/// instanced lock is [`CONTAINER_LOCKOUT_MS`] ([`SimWorld::lock_of`]).
const NO_RESPAWN_RATE: (u64, u64) = (0, 0);
/// The interact tag of a tracker entry of an object without one (Observed: retail 0).
const NO_TRACKED_TAG: u32 = 0;
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
    /// The player gathered this instanced object and its lock has not reset.
    Opened,
    /// The object is only on quest and no active task of the player's quests names its tag.
    NotOnQuest,
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
    /// When it started, ms.
    started_ms: u64,
    /// The work it had left at its start, ms of `BaseGatherTime` (less a kept gather's work).
    work_ms: u64,
    /// The work done per ms of gather.
    efficiency: f32,
    end_ms: u64,
    /// The gatherer count its start sent ([`NO_GATHERERS`] for a start that sent none).
    gatherers: u8,
    /// Its object is instanced ([`nw_content::gathering::InstancedGathers`]): the gather is the
    /// gatherer's own.
    instanced: bool,
    /// It gathers a group node ([`GroupWork`]), which holds its work and its end.
    group: bool,
}

impl OpenGather {
    /// The encounter start object (or gate switch) it gathers; `None` for another gather.
    pub(super) const fn object(&self) -> Option<u64> {
        self.object
    }

    /// The work it has left when it stops at `now_ms`: the work of its whole
    /// [`PROGRESS_STEP_MS`] steps is done.
    fn work_left_ms(&self, now_ms: u64) -> u64 {
        self.work_ms.saturating_sub(work_done_ms(
            now_ms.saturating_sub(self.started_ms),
            self.efficiency,
        ))
    }

    /// The own done state of the gather that ends (or stops) with the player's counters
    /// `counters`, after `ended_before` other gatherers of its group node ended in the same tick.
    fn done_state(&self, counters: &mut GatherCounters, ended_before: u32) -> BundleMember {
        if self.water {
            water_gather_ended(counters)
        } else {
            gather_ended(
                self.request.target_entity,
                counters,
                self.gatherers,
                ended_before,
            )
        }
    }
}

/// The lock an instanced object's gather leaves for its gatherer ([`SimWorld::lock_of`],
/// [`SimWorld::instanced_lock`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct GatherLock {
    /// When it ends, server ms.
    pub(super) reset_ms: u64,
    /// The loot type of the object's slice.
    loot_type: u32,
    /// The position of its tracker entry.
    at: Position,
    /// How its tracker entry follows the gatherer; `None` for a slice without an area of
    /// interest radius.
    range: Option<EntryRange>,
}

impl GatherLock {
    /// Lists the lock in the loot tracker of `player` for object `object` (interact tag `tag`),
    /// looted at server time `server_ms`: the tracker member goes out with the player's next
    /// bundle, and the player may not gather the object again before the reset
    /// ([`super::loot_bag::PlayerLoot::opened`]).
    pub(super) fn list_for(
        self,
        player: &mut Player,
        object: [u8; RAW_ID_LEN],
        tag: u32,
        server_ms: u64,
    ) {
        player.list_version += 1;
        let version = player.list_version;
        let tracked = player.loot.track_instanced(
            object,
            self.loot_type,
            tag,
            self.at,
            server_ms,
            self.reset_ms,
            self.range,
            version,
        );
        player.extra_members.push(tracked);
    }
}

/// The work `elapsed_ms` of gather at `efficiency` did: that of its whole [`PROGRESS_STEP_MS`]
/// steps, ms of `BaseGatherTime`.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a gather's ms of a few minutes at a positive efficiency"
)]
fn work_done_ms(elapsed_ms: u64, efficiency: f32) -> u64 {
    let steps = elapsed_ms / PROGRESS_STEP_MS;
    (steps.saturating_mul(PROGRESS_STEP_MS) as f32 * efficiency) as u64
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
        let instanced = mob.is_none()
            && self
                .content
                .instanced_gathers
                .instanced(request.target_entity);
        // An instanced object stays locked for its gatherer until its tracker entry's reset.
        let opened = instanced
            && self
                .world
                .get::<Player>(entity)
                .is_some_and(|player| player.loot.opened(&request.target));
        let allowed = if opened {
            Err(GatherRefusal::Opened)
        } else if !self.quest_opens(number, &request) {
            Err(GatherRefusal::NotOnQuest)
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
        let needed = request.gatherable.map_or(ONE_GATHERER, |crc| {
            self.content.gatherables.gatherers_needed(crc)
        });
        // A group gatherable's node holds the work of all its gatherers.
        let group = (needed > ONE_GATHERER && !instanced && mob.is_none() && object.is_none())
            .then(|| GroupWork::new(needed, gatherable.gather_ms));
        // Inferred: a group node's work, not one swing, ends its gathers.
        let single_swing = tool_efficiency.is_some() && group.is_none() && {
            let chance = self.instant_gather_chance(entity, &gatherable);
            self.dice.chance(chance)
        };
        let started: Result<u8, ()> = match (mob, point) {
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
                    Ok(SOLE_GATHERER)
                }
            }
            (Some(mob), _) => self.start_skinning(mob, number).map(|()| SOLE_GATHERER),
            (None, Some((point, at))) => self
                .nodes
                .join(
                    request.target,
                    point,
                    at,
                    number,
                    NodeLayout::of(&gatherable),
                    group,
                )
                .map(|gatherers| u8::try_from(gatherers).unwrap_or(u8::MAX))
                .map_err(|busy| tracing::debug!(player = number, ?busy, "gather refused")),
            (None, None) => Err(()),
        };
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let Ok(gatherers) = started else {
            let refusal = gather_answer(number, request, GATHER_BUSY, NO_NAME);
            send_now(&mut self.combat_messages, &player.connection, &[refusal]);
            return;
        };
        let gatherers = if instanced {
            NO_GATHERERS
        } else {
            gatherers
        };
        // A group node holds its work; a stopped gather of another object left its work for the
        // next one, whoever gathers it.
        let work_ms = match group {
            Some(_) => self.nodes.group_work_left(&request.target, now_ms),
            None => self.kept_gathers.take(&request.target),
        }
        .unwrap_or(gatherable.gather_ms);
        #[allow(clippy::cast_precision_loss, reason = "a gather time of seconds")]
        let amount = work_ms as f32 / MS_PER_SECOND;
        let (duration_ms, efficiency) = match tool_efficiency {
            Some(efficiency) => {
                #[allow(
                    clippy::cast_precision_loss,
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a gather time of seconds over a positive efficiency"
                )]
                let tool_ms = (work_ms as f32 / efficiency) as u64;
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
                work_ms.div_ceil(GATHER_TICK_MS) * GATHER_TICK_MS + GATHER_TICK_MS,
                HAND_EFFICIENCY,
            ),
        };
        let run = GatherRun {
            amount,
            efficiency,
            gatherers,
        };
        let accept = gather_answer(number, request, GATHER_ACCEPTED, NO_NAME);
        send_now(&mut self.combat_messages, &player.connection, &[accept]);
        let started = gather_started(request.target_entity, &mut player.gathers, run);
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
            started_ms: now_ms,
            work_ms,
            efficiency,
            end_ms: now_ms.saturating_add(duration_ms),
            gatherers,
            instanced,
            group: group.is_some(),
        });
        if group.is_some() {
            let keeps = self.keeps_progress(&request);
            self.regroup(&request.target, keeps, now_ms, Some(number));
        }
        // A switch of the instance's gates fires its rules that wait for the pull's start
        // ([`super::gates`]); a start object counts its interactor
        // ([`super::encounter_roots::EncounterObjects::gather_started`]).
        if let Some(object) = object {
            self.gate_switch_started(object);
            self.encounter_objects.gather_started(object);
        }
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
        let work_ms = gatherable.gather_ms;
        let duration_ms = work_ms.div_ceil(GATHER_TICK_MS) * GATHER_TICK_MS + GATHER_TICK_MS;
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
            started_ms: now_ms,
            work_ms,
            efficiency: HAND_EFFICIENCY,
            end_ms: now_ms.saturating_add(duration_ms),
            gatherers: NO_GATHERERS,
            instanced: false,
            group: false,
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

    /// Whether the quests of player `number` open the object that `request` gathers: one whose
    /// variant is only on quest ([`nw_content::interact_tags::InteractTags::quest_gate`]) only
    /// while an active task of one of them names its tag; any other object always.
    fn quest_opens(&self, number: u64, request: &InteractionRequest) -> bool {
        let gate = self
            .content
            .interact_tags
            .quest_gate(request.target_entity, request.gatherable);
        gate.is_empty()
            || self
                .quests_of_player(number)
                .iter()
                .any(|quest| gate.iter().any(|tag| quest.run.enables_interact(tag)))
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

    /// Whether the gatherable that `request` gathers keeps the work of a stopped gather
    /// ([`nw_content::gathering::Gatherables::keeps_progress`]).
    fn keeps_progress(&self, request: &InteractionRequest) -> bool {
        request
            .gatherable
            .is_some_and(|crc| self.content.gatherables.keeps_progress(crc))
    }

    /// The gather `gather` stops at `now_ms` without its harvest: the work it has left stays for
    /// the next gather of its object when its gatherable keeps it and its object is shared
    /// (Observed: no retail gather of an instanced object kept its work, the well
    /// `98_Syndicate_03_Well_02` among them; the shared Brimstone glyph locks, loot containers,
    /// kept theirs). A water gather has no object; a group node holds its own work
    /// ([`SimWorld::regroup`]).
    fn keep_work_left(&mut self, gather: &OpenGather, now_ms: u64) {
        let keeps = !gather.water
            && !gather.instanced
            && !gather.group
            && self.keeps_progress(&gather.request);
        if !keeps {
            return;
        }
        let start_object = gather
            .object
            .filter(|object| self.encounter_objects.stands(*object));
        self.kept_gathers.keep(
            gather.request.target,
            KeptGather {
                work_ms: gather.work_left_ms(now_ms),
                mob: gather.mob,
                start_object,
            },
        );
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
    /// back, and the gathered start objects whose delayed event ended go
    /// ([`super::encounter_roots::EncounterObjects::tick`]).
    pub(super) fn gathers_due(&mut self, now_ms: u64) {
        self.nodes.tick(now_ms);
        self.encounter_objects.tick(now_ms);
        let (mobs, objects) = (&self.mobs, &self.encounter_objects);
        self.kept_gathers.retain(|kept| {
            kept.mob.is_none_or(|mob| mobs.health(mob) == Some(0))
                && kept.start_object.is_none_or(|object| objects.stands(object))
        });
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.open_gathers)
            .into_iter()
            .partition(|gather| now_ms >= gather.end_ms);
        self.open_gathers = later;
        // The gathers of each group node that ended before in this tick (`GatherCounters`).
        let mut group_ends: HashMap<[u8; RAW_ID_LEN], u32> = HashMap::new();
        for gather in due {
            let ended_before = if gather.group {
                let ended = group_ends.entry(gather.request.target).or_default();
                let before = *ended;
                *ended += 1;
                before
            } else {
                0
            };
            self.end_gather(&gather, ended_before, now_ms);
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

    /// `gather` ends at `now_ms`, after `ended_before` other gatherers of its group node ended
    /// in the same tick.
    fn end_gather(&mut self, gather: &OpenGather, ended_before: u32, now_ms: u64) {
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
            // A catacombs chest stays, emptied, and feeds the run's pool ([`super::catacombs`]).
            // Another start object goes (the holdout glyph at its delayed event's end): its
            // encounter's stage counts it. A switch of the
            // instance's gates ([`super::gates`]) stays: its rules change it. An erosion shrine
            // adds its effect ([`super::afflictions`]).
            None if let Some(object) = gather.object => {
                if !self.catacomb_chest_opened(object) {
                    self.encounter_objects.gathered(object, now_ms);
                    self.gate_switch_used(object);
                }
                self.erosion_gathered(gather.number, &gather.gatherable.id);
            }
            // An instanced object stays for everyone else: only its gatherer's tracker locks it.
            None if gather.instanced => self
                .nodes
                .release(&gather.request.target, gather.number),
            None => {
                let respawn_at_ms = now_ms.saturating_add(self.respawn_roll_ms(&gather.gatherable));
                self.nodes
                    .deplete(&gather.request.target, now_ms, respawn_at_ms);
            }
        }
        let locked = self.lock_of(gather);
        let tag = self.tracked_tag(gather.request.target_entity, gather.request.gatherable);
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
        let ended = gather.done_state(&mut player.gathers, ended_before);
        player.extra_members.push(ended);
        if !gather.water {
            player.ended_gather = Some(gather.request);
        }
        if let Some(lock) = locked {
            lock.list_for(&mut player, gather.request.target, tag, self.server_ms);
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
            self.outpost_rush_gathered(gather.number, item, *quantity);
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

    /// A roll of the respawn time of `gatherable`: from its `MinRespawnRate` to its
    /// `MaxRespawnRate`, ms.
    fn respawn_roll_ms(&mut self, gatherable: &Gatherable) -> u64 {
        let (shortest, longest) = gatherable.respawn_ms;
        let spread = u32::try_from(longest.saturating_sub(shortest)).unwrap_or(u32::MAX);
        shortest.saturating_add(u64::from(self.dice.below(spread.saturating_add(1))))
    }

    /// The lock that `gather` leaves on its instanced object for its gatherer
    /// ([`Self::instanced_lock`], at the node's point: Inferred, retail's entry lies within 2 m
    /// of the well's capital); `None` for a gather of a shared object, a corpse, water or an
    /// encounter's object.
    fn lock_of(&mut self, gather: &OpenGather) -> Option<GatherLock> {
        if !gather.instanced || gather.mob.is_some() || gather.water || gather.object.is_some() {
            return None;
        }
        let at = self.nodes.position_of(&gather.request.target)?;
        let object = self
            .content
            .instanced_gathers
            .of(gather.request.target_entity)?;
        self.instanced_lock(object, &gather.gatherable, gather.request.gatherable, at)
    }

    /// The lock that a gather of instanced object `object` (its slice's values) of gatherable
    /// `gatherable` (CRC `crc`), ended now, leaves for its gatherer, its tracker entry at `at`;
    /// `None` without the server's reset clock (the loot limit rules carry it).
    ///
    /// Observed (finding D8 "per-player gather lock", 2,122 tracker entries of retail gathers of
    /// instanced objects, loot containers and others): the entry's reset is the gather's end plus
    /// a roll of the gatherable's respawn time (1,047 entries strictly between a
    /// `MinRespawnRate` and a `MaxRespawnRate` that differ: the Syndicate wells 228.3 s and
    /// 226.3 s of 200 to 300, the whisper plants 33.0 to 57.3 s of 30 to 60;
    /// `Catacombs_LevelUp` 0.1 s, 28 of 28), 3,600 s for a gatherable without one (842 of 842
    /// chests of 0 and 0), and never after the world's next daily reset, whole seconds after the
    /// end (139 entries that end there, the respawn time longer: the vistas of 86,400 s, the
    /// catacomb chests of 64,000 s; [`nw_content::resets`]); an elite chest's is the daily reset
    /// ([`nw_content::chest_lockouts`]). Observed (finding D11, retail quest objects): the same
    /// rule holds for the whole gathers of instanced quest objects, 232 of 232 proximity entries
    /// (213 inside the respawn range, 19 capped at the daily reset: the trails of 86,400 s) and
    /// 139 of 139 damageable ones. The entry's loot type and range are the object's slice's
    /// ([`nw_content::gathering::InstancedGathers::of`], [`EntryRange`]), the range's daily
    /// reset set when the lock ends there.
    pub(super) fn instanced_lock(
        &mut self,
        object: InstancedObject,
        gatherable: &Gatherable,
        crc: Option<u32>,
        at: Position,
    ) -> Option<GatherLock> {
        let clock = self.content.loot_limits.as_ref()?.clock;
        let opened_unix_ms = self.server_ms.saturating_add(SERVER_EPOCH_UNIX_MS);
        let daily_reset_unix_ms = clock.end_ns(
            ResetCycle::Daily,
            opened_unix_ms.saturating_mul(nw_messages::NS_PER_MS),
        ) / nw_messages::NS_PER_MS;
        let elite = crc.is_some_and(|crc| self.content.elite_chests.contains(crc));
        let rolled_unix_ms = if elite {
            daily_reset_unix_ms
        } else {
            let span_ms = if gatherable.respawn_ms == NO_RESPAWN_RATE {
                CONTAINER_LOCKOUT_MS
            } else {
                self.respawn_roll_ms(gatherable)
            };
            opened_unix_ms.saturating_add(span_ms)
        };
        let reset_unix_ms = rolled_unix_ms.min(daily_reset_unix_ms);
        Some(GatherLock {
            reset_ms: reset_unix_ms.saturating_sub(SERVER_EPOCH_UNIX_MS),
            loot_type: object.loot_type,
            at,
            range: object.aoi_radius_m.map(|radius_m| EntryRange {
                radius_m,
                daily_reset: reset_unix_ms == daily_reset_unix_ms,
            }),
        })
    }

    /// The interact tag its tracker entry gives the object of facet entity `facet` that gathers
    /// `gatherable` (a CRC): the CRC-32 of its lowercase tag (Observed:
    /// `IO_98_Syndicate_03_Well_01` 0x0f02dfa8; 1,180 of 1,180 retail entries with a tag carry
    /// one of their gatherable's variant tags, 847 of 847 whose variants have none carry 0;
    /// finding D11: 232 of 232 quest proximity entries and 139 of 149 damageable ones carry their
    /// capital's variant tag, `IO_17_IQ_CorruptionTrail_02` of a variant of gatherable
    /// `17_IQ_CorruptionTrail_01` among them), [`NO_TRACKED_TAG`] for an object without one.
    /// Inferred: an object whose own slice holds no tag carries the first tag of its
    /// gatherable's variants.
    pub(super) fn tracked_tag(&self, facet: u64, gatherable: Option<u32>) -> u32 {
        self.content
            .interact_tags
            .of_interaction(facet, gatherable)
            .first()
            .map_or(NO_TRACKED_TAG, |tag| lowercase_crc32(tag))
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
        self.give_loot_rolled(number, loot, reason, now_ms).0
    }

    /// [`SimWorld::give_loot`] that also tells the items rolled and given (item id, quantity),
    /// for a game event's entry ([`super::game_event_log`]).
    pub(super) fn give_loot_listed(
        &mut self,
        number: u64,
        loot: Vec<(String, u32)>,
        now_ms: u64,
    ) -> Vec<(String, u32)> {
        self.give_loot_rolled(number, loot, SYNC_MOVED, now_ms).1
    }

    /// Rolls `loot` for player `number` and gives it in a bag change of sync reason `reason`:
    /// whether every item went into the bag, and the items rolled.
    fn give_loot_rolled(
        &mut self,
        number: u64,
        loot: Vec<(String, u32)>,
        reason: u8,
        now_ms: u64,
    ) -> (bool, Vec<(String, u32)>) {
        let items = self.rolled_tables(number, loot);
        let (entries, values) = self.roll_drops(number, None, items, &BTreeMap::new());
        let rolled = entries.clone();
        let whole = self.give_drops(
            number,
            entries.into_iter().zip(values).collect(),
            reason,
            now_ms,
        );
        (whole, rolled)
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

    /// The gatherers of group node `target` changed at `now_ms`: the work done so far stays on it,
    /// its work goes on at the mean efficiency of its gatherers while enough of them gather
    /// ([`GroupWork`]), each of its gathers ends when its work does, and each of its gatherers
    /// but `joined` (whose start state holds the same) gets the new count, with the work left
    /// when work was counted ([`gatherers_changed`]). A node nobody gathers any more starts over
    /// when its gatherable does not keep its progress (`keeps`; Inferred: as a stopped gather of
    /// such a gatherable does; the 6 group gatherables of build 25810808 keep it).
    fn regroup(
        &mut self,
        target: &[u8; RAW_ID_LEN],
        keeps: bool,
        now_ms: u64,
        joined: Option<u64>,
    ) {
        let gathers: Vec<usize> = self
            .open_gathers
            .iter()
            .enumerate()
            .filter(|(_, gather)| gather.group && gather.request.target == *target)
            .map(|(index, _)| index)
            .collect();
        let efficiencies: f32 = gathers
            .iter()
            .map(|index| self.open_gathers[*index].efficiency)
            .sum();
        #[allow(clippy::cast_precision_loss, reason = "a few gatherers")]
        let rate = if gathers.is_empty() {
            HAND_EFFICIENCY
        } else {
            efficiencies / gathers.len() as f32
        };
        let Some(work) = self.nodes.group_work(target) else {
            return;
        };
        let counted = work.change(now_ms, gathers.len(), rate);
        if gathers.is_empty() && !keeps {
            work.reset();
        }
        #[allow(clippy::cast_precision_loss, reason = "a gather time of seconds")]
        let amount = counted.then(|| work.left_ms as f32 / MS_PER_SECOND);
        let end_ms = work.end_ms().unwrap_or(u64::MAX);
        let gatherers = u8::try_from(gathers.len()).unwrap_or(u8::MAX);
        let mut told = Vec::new();
        for index in gathers {
            let gather = &mut self.open_gathers[index];
            gather.end_ms = end_ms;
            if Some(gather.number) != joined {
                told.push(gather.number);
            }
        }
        for number in told {
            if let Some(entity) = self.player_by_number(number)
                && let Some(mut player) = self.world.get_mut::<Player>(entity)
            {
                let changed = gatherers_changed(&mut player.gathers, gatherers, amount);
                player.extra_members.push(changed);
            }
        }
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
            self.end_gather(&gather, 0, now_ms);
            running = None;
        }
        let request = match running {
            Some(at) => {
                let gather = self.open_gathers.remove(at);
                self.keep_work_left(&gather, now_ms);
                if gather.mob.is_none() {
                    self.nodes.release(&gather.request.target, number);
                }
                if gather.group {
                    let keeps = self.keeps_progress(&gather.request);
                    self.regroup(&gather.request.target, keeps, now_ms, None);
                }
                if let Some(object) = gather.object {
                    self.encounter_objects.gather_stopped(object);
                }
                if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                    let stopped = gather.done_state(&mut player.gathers, 0);
                    player.extra_members.push(stopped);
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

/// The work left of a stopped gather, for the next gather of its object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct KeptGather {
    /// The work left, ms of `BaseGatherTime`.
    work_ms: u64,
    /// The mob whose corpse was skinned: the work stays while the mob is dead.
    mob: Option<u64>,
    /// The encounter start object gathered: the work stays while it stands.
    start_object: Option<u64>,
}

/// The work left of the stopped gathers that keep it, by the raw id of the object they gathered.
///
/// Observed (finding 2026-11-13 "gather progress and quest interacts"): the work belongs to the
/// object, not to the player's last gather (a `PigmentPlantTan` kept 8.87 left over 239 s with
/// a wolf skinned between, a `TreeHardwoodMedium` across a `TreeHardwoodLarge` gather). Inferred:
/// the next gather of the object starts with it whoever gathers it (one shared world object; no
/// retail capture has a second player on a stopped node), and it stays until that gather, the
/// longest pause of a retail capture being 239 s.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct KeptGathers {
    by_target: HashMap<[u8; RAW_ID_LEN], KeptGather>,
}

impl KeptGathers {
    /// The gather of object `target` stopped with `kept` left.
    fn keep(&mut self, target: [u8; RAW_ID_LEN], kept: KeptGather) {
        self.by_target.insert(target, kept);
    }

    /// The work left on object `target` for the gather that starts there now, which takes it.
    fn take(&mut self, target: &[u8; RAW_ID_LEN]) -> Option<u64> {
        self.by_target.remove(target).map(|kept| kept.work_ms)
    }

    /// Keeps only the work for which `stays` holds.
    fn retain(&mut self, stays: impl Fn(&KeptGather) -> bool) {
        self.by_target.retain(|_, kept| stays(kept));
    }
}

/// What the last change of a node set.
///
/// Observed (interact states of the gatherables of 513 retail ledgers): a gatherable counts its
/// gatherers in `HasInteractors`, as a change of that child alone at each gather's start
/// (`010200000001`) and stop (`010200000000`, 222 slots); the harvest that depletes it sends
/// `Enabled` off with `HasInteractors` 0 (`01030000000000`, 1,483 slots).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeChange {
    /// Its gatherable or interact state or its timeline (a growth, a timeline's end).
    States,
    /// Its interactor count alone (a gather's start or stop).
    Interactors,
    /// The harvest that depleted it and ended its gather.
    Harvest,
}

/// The interact state's component of a node of `layout`.
const fn interact_component(layout: NodeLayout) -> u64 {
    match layout {
        NodeLayout::Plant => INTERACT_COMPONENT,
        NodeLayout::BreakingWallsCannon => CANNON_INTERACT_COMPONENT,
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
    /// The players who gather it now: one at most, but for a group node.
    gatherers: Vec<u64>,
    /// The work of a group node.
    group: Option<GroupWork>,
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
    /// What its last change set.
    last_change: NodeChange,
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

    /// Its interactor count changed: a new version that sets the count alone.
    fn interactors_changed(&mut self) {
        self.version += 1;
        self.last_change = NodeChange::Interactors;
    }

    /// Its interact state's `HasInteractors`: its gatherers (Observed: 1 while a player gathers
    /// a node; Inferred: each gatherer of a group node counts).
    fn interactors(&self) -> u32 {
        u32::try_from(self.gatherers.len()).unwrap_or(u32::MAX)
    }

    /// The `HasInteractors` of its whole interact state: in a card only while a player gathers
    /// it (Inferred: a card holds the states that left their default; Observed: the Fire
    /// Cannon's card after its use `06b22d010100`), none after a change of its other states
    /// (Observed: its growth `06b22d010101`), 0 after the harvest that ends its gather, its
    /// count after a gather's start or stop.
    fn whole_interactors(&self, with_card: bool) -> Option<u32> {
        if with_card {
            return (self.interactors() > 0).then(|| self.interactors());
        }
        match self.last_change {
            NodeChange::States => None,
            NodeChange::Harvest => Some(0),
            NodeChange::Interactors => Some(self.interactors()),
        }
    }

    /// The interact state member of its layout with its interactor count alone.
    fn interactors_member(&self, layout: NodeLayout) -> BundleMember {
        BundleMember {
            component: interact_component(layout),
            state: MemberState::Interact(InteractState {
                enabled: None,
                has_interactors: Some(self.interactors()),
            }),
        }
    }

    /// Its card and states (`with_card`), or its states alone, in the slots of its layout.
    fn members(&self, layout: NodeLayout, with_card: bool) -> Vec<BundleMember> {
        let (state, enabled) = if self.depleted() {
            (DEPLETED, false)
        } else {
            (HEALTHY, true)
        };
        let (controller, interact) = match layout {
            NodeLayout::Plant => (CONTROLLER_COMPONENT, interact_component(layout)),
            NodeLayout::BreakingWallsCannon => {
                (CANNON_CONTROLLER_COMPONENT, interact_component(layout))
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
                    has_interactors: self.whole_interactors(with_card),
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

/// The work of the node of a group gatherable (`NumRequiredToGather` above one,
/// [`nw_content::gathering::Gatherables::gatherers_needed`]): it goes on only while at least
/// `needed` players gather the node, and the work done stays on the node when a gatherer leaves.
///
/// Observed (finding 2026-11-13 "gatherer counts and group gathers": 4 retail sequences of
/// `DungeonRestlessShores_MinorMonolith`, needed 3, every gatherer at efficiency 1.0):
/// - `EXP-The-Depths`: 10.0 left with 2 gatherers from 730.93 s, a third at 735.30 s; the POV's
///   stop at 735.64 s left 9.70 (3 steps); it came back at 736.20 s with 9.70 and 3 gatherers and
///   ended at 745.89 s, 9.70 s later.
/// - `EXP-The-Ennead`: 2 gatherers from 994.85 s, a third at 999.00 s, a stop at 1000.32 s left
///   8.70; 50 s later a gather started with 8.70 alone, others came at 1052.45 s and 1052.52 s, and
///   it ended at 1061.22 s.
/// - `HiveNormaleTheDepths`: 3 gatherers from 1685.04 s; one left at 1687.61 s and the others got
///   7.5 left and 2 gatherers; 35.7 s with 2 gatherers did no work; with a third at 1746.54 s it
///   ended at 1754.06 s, 7.5 s later.
///
/// Inferred: the work goes on at the mean efficiency of the gatherers (the totem
/// `RW_Siege_Pool_Blighted_Totem`'s gathers with 3 gatherers did 1.07 to 1.18 of work per second
/// while the POV's own efficiency was 1.0; the other gatherers' efficiencies are in no ledger).
/// Unknown: the totem's own state got 6.886 and 6.750 left when its count rose to 3 and to 2
/// (`Playthrough_Part_18` 3334.58 s and 3466.34 s), work with fewer than 3 gatherers or another
/// gatherer's; its two gathers with 1 and 2 gatherers at 3481.30 s and 3485.87 s started from full,
/// as no work went on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct GroupWork {
    /// The gatherers its work needs.
    needed: usize,
    /// Its gatherable's `BaseGatherTime`, ms.
    full_ms: u64,
    /// The work it had left when it last changed, ms of `BaseGatherTime`.
    left_ms: u64,
    /// While its work goes on: since when, and at which efficiency.
    running: Option<(u64, f32)>,
}

impl GroupWork {
    /// The work of a node that needs `needed` gatherers and `full_ms` of work.
    pub(super) fn new(needed: u32, full_ms: u64) -> Self {
        Self {
            needed: usize::try_from(needed).unwrap_or(usize::MAX),
            full_ms,
            left_ms: full_ms,
            running: None,
        }
    }

    /// The work left at `now_ms`: that of its whole [`PROGRESS_STEP_MS`] steps since its work
    /// went on is done.
    fn left_at(&self, now_ms: u64) -> u64 {
        match self.running {
            Some((since_ms, rate)) => self
                .left_ms
                .saturating_sub(work_done_ms(now_ms.saturating_sub(since_ms), rate)),
            None => self.left_ms,
        }
    }

    /// Its gatherers changed at `now_ms` to `gatherers`, whose mean efficiency is `rate`: the
    /// work done so far stays, and the work goes on from now while enough of them gather.
    /// Whether work went on before (so the work left was counted).
    fn change(&mut self, now_ms: u64, gatherers: usize, rate: f32) -> bool {
        let counted = self.running.is_some();
        self.left_ms = self.left_at(now_ms);
        self.running = (gatherers >= self.needed).then_some((now_ms, rate));
        counted
    }

    /// When its work ends while it goes on; `None` while it waits for gatherers.
    fn end_ms(&self) -> Option<u64> {
        self.running.map(|(since_ms, rate)| {
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a gather time of seconds over a positive efficiency"
            )]
            let run_ms = (self.left_ms as f32 / rate) as u64;
            since_ms.saturating_add(run_ms)
        })
    }

    /// Its work starts over: all of it is left, and it waits for gatherers.
    fn reset(&mut self) {
        self.left_ms = self.full_ms;
        self.running = None;
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
    /// A node of a group gatherable (`group`: its work when nobody worked on it yet) takes every
    /// player who comes (Observed: 2 and 3 players gathered the group monolith at once, and no
    /// other gatherable had a second gatherer in 694 retail ledgers). Its gatherers once the
    /// player joined.
    pub(super) fn join(
        &mut self,
        gde_ref: [u8; RAW_ID_LEN],
        point: usize,
        position: Position,
        player: u64,
        layout: Option<NodeLayout>,
        group: Option<GroupWork>,
    ) -> Result<usize, NodeBusy> {
        let index = match (self.by_ref.get(&gde_ref), self.by_point.get(&point)) {
            (Some(by_ref), Some(by_point)) if by_ref == by_point => *by_ref,
            (None, None) => {
                self.nodes.push(Node {
                    gde_ref,
                    point,
                    position,
                    gatherers: Vec::new(),
                    group: None,
                    respawn_at_ms: None,
                    layout,
                    timeline_ends_at_ms: None,
                    timeline_version: 0,
                    version: 0,
                    last_change: NodeChange::States,
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
        if node.group.is_none() {
            node.group = group;
        }
        if node.group.is_none() && node.gatherers.iter().any(|gatherer| *gatherer != player) {
            return Err(NodeBusy::Gathered);
        }
        if !node.gatherers.contains(&player) {
            node.gatherers.push(player);
            node.interactors_changed();
        }
        Ok(node.gatherers.len())
    }

    /// The gather of node `gde_ref` by player `player` ends without a harvest (a cancel).
    pub(super) fn release(&mut self, gde_ref: &[u8; RAW_ID_LEN], player: u64) {
        if let Some(node) = self
            .by_ref
            .get(gde_ref)
            .map(|index| &mut self.nodes[*index])
            && let Some(at) = node.gatherers.iter().position(|gatherer| *gatherer == player)
        {
            node.gatherers.remove(at);
            node.interactors_changed();
        }
    }

    /// The work group node `gde_ref` has left at `now_ms`; `None` for another node.
    pub(super) fn group_work_left(&self, gde_ref: &[u8; RAW_ID_LEN], now_ms: u64) -> Option<u64> {
        let index = self.by_ref.get(gde_ref)?;
        self.nodes[*index].group.map(|work| work.left_at(now_ms))
    }

    /// The work of group node `gde_ref`; `None` for another node.
    fn group_work(&mut self, gde_ref: &[u8; RAW_ID_LEN]) -> Option<&mut GroupWork> {
        let index = self.by_ref.get(gde_ref)?;
        self.nodes[*index].group.as_mut()
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

    /// Node `gde_ref` was harvested at `now_ms`: depleted until `respawn_at_ms`, and a group
    /// node's work starts over. A Fire Cannon's timeline plays from now for
    /// [`CANNON_TIMELINE_MS`]. The other gathers of a group node that end with the same harvest
    /// change nothing more.
    pub(super) fn deplete(&mut self, gde_ref: &[u8; RAW_ID_LEN], now_ms: u64, respawn_at_ms: u64) {
        if let Some(node) = self
            .by_ref
            .get(gde_ref)
            .map(|index| &mut self.nodes[*index])
            && !node.depleted()
        {
            node.gatherers.clear();
            if let Some(work) = node.group.as_mut() {
                work.reset();
            }
            node.respawn_at_ms = Some(respawn_at_ms);
            node.version += 1;
            node.last_change = NodeChange::Harvest;
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
            gatherers: Vec::new(),
            group: None,
            respawn_at_ms: Some(respawn_at_ms),
            layout,
            timeline_ends_at_ms: None,
            timeline_version: 1,
            version: 1,
            last_change: NodeChange::States,
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
                node.last_change = NodeChange::States;
            }
            if node.respawn_at_ms.is_some_and(|at| now_ms >= at) {
                node.respawn_at_ms = None;
                node.version += 1;
                node.last_change = NodeChange::States;
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
        let known = sent.insert(number, node.version);
        if !slot.new && known == Some(node.version) {
            return None;
        }
        let members = if !slot.new
            && node.last_change == NodeChange::Interactors
            && known == node.version.checked_sub(1)
        {
            vec![node.interactors_member(layout)]
        } else {
            node.members(layout, slot.new)
        };
        Some(BundleGroup {
            interest: u64::from(slot.interest),
            members,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use nw_content::gathering::{
        CyclicGatherables, GatherRule, GatherRules, Gatherables, InstancedGathers,
        InstancedObject, ToolEfficiencies, ToolEfficiency,
    };
    use nw_content::progression::{Progression, RankPerks, TradeskillRankRow, TradeskillSource};
    use nw_content::rewards::{EventGrant, EventGrants};
    use nw_formats::objectstream::lowercase_crc32;
    use nw_messages::GatheringState;

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
        assert_eq!(nodes.join(FLINT, FLINT_POINT, AT, PLAYER, SENT, None), Ok(1));
        nodes.deplete(&FLINT, HARVESTED_AT_MS, RESPAWN_AT_MS);
        assert_eq!(
            nodes.join(OTHER, FLINT_POINT, AT, PLAYER, SENT, None),
            Err(NodeBusy::OtherNode),
            "a new id does not skip the depleted node"
        );
        assert_eq!(
            nodes.join(FLINT, FLINT_POINT + 1, AT, PLAYER, SENT, None),
            Err(NodeBusy::OtherNode),
            "the id of one node does not gather another"
        );
        assert_eq!(
            nodes.join(OTHER, FLINT_POINT + 1, AT, PLAYER, SENT, None),
            Ok(1)
        );
    }

    #[test]
    fn a_node_of_another_layout_is_never_sent() {
        let mut nodes = Nodes::default();
        assert_eq!(nodes.join(FLINT, FLINT_POINT, AT, PLAYER, None, None), Ok(1));
        nodes.deplete(&FLINT, HARVESTED_AT_MS, RESPAWN_AT_MS);
        assert_eq!(
            nodes.join(FLINT, FLINT_POINT, AT, PLAYER, None, None),
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
        assert_eq!(nodes.join(FLINT, FLINT_POINT, AT, PLAYER, SENT, None), Ok(1));
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
            restarted.join(FLINT, FLINT_POINT, AT, PLAYER, SENT, None),
            Err(NodeBusy::Depleted)
        );
        assert_eq!(restarted.numbers_near(AT, RADIUS_M).count(), 1);
        restarted.tick(RESPAWN_AT_MS);
        assert_eq!(
            restarted.join(FLINT, FLINT_POINT, AT, PLAYER, SENT, None),
            Ok(1)
        );
        assert!(
            restarted.take_unsaved(|at_ms| at_ms).is_empty(),
            "a load is no new save"
        );
    }

    /// Retail gatherables count their gatherers: `HasInteractors` alone at a gather's start
    /// (`010200000001`) and stop (`010200000000`), and `Enabled` off with `HasInteractors` 0 at
    /// the harvest that depletes them (`01030000000000`).
    #[test]
    fn a_node_counts_its_gatherer_from_the_gathers_start_to_its_stop_or_harvest() {
        let interact_bytes = |group: &BundleGroup| -> Vec<String> {
            group
                .members
                .iter()
                .filter(|member| matches!(member.state, MemberState::Interact(_)))
                .map(|member| hex::encode(member.encode().expect("encodes")))
                .collect()
        };
        let mut nodes = Nodes::default();
        let number = GATHERABLE_ENTITY_BASE;
        let mut sent = HashMap::new();
        let new = Slot {
            interest: 9,
            new: true,
        };
        let kept = Slot { new: false, ..new };
        assert_eq!(nodes.join(FLINT, FLINT_POINT, AT, PLAYER, SENT, None), Ok(1));
        let card = nodes.group(number, new, &mut sent).expect("card");
        assert_eq!(interact_bytes(&card), ["04b22d01030100000001"], "the card with its gatherer");
        nodes.release(&FLINT, PLAYER);
        let stopped = nodes.group(number, kept, &mut sent).expect("stop");
        assert_eq!(stopped.members.len(), 1, "the count alone");
        assert_eq!(interact_bytes(&stopped), ["04b22d010200000000"]);
        nodes.release(&FLINT, PLAYER);
        assert_eq!(nodes.group(number, kept, &mut sent), None, "no gatherer to stop");
        assert_eq!(nodes.join(FLINT, FLINT_POINT, AT, PLAYER, SENT, None), Ok(1));
        assert_eq!(nodes.join(FLINT, FLINT_POINT, AT, PLAYER, SENT, None), Ok(1));
        let started = nodes.group(number, kept, &mut sent).expect("start");
        assert_eq!(interact_bytes(&started), ["04b22d010200000001"]);
        assert_eq!(nodes.group(number, kept, &mut sent), None, "one start, one change");
        nodes.deplete(&FLINT, HARVESTED_AT_MS, RESPAWN_AT_MS);
        let harvested = nodes.group(number, kept, &mut sent).expect("harvest");
        assert_eq!(interact_bytes(&harvested), ["04b22d01030000000000"]);
        assert_eq!(state_of(&harvested), Some(DEPLETED));
        nodes.tick(RESPAWN_AT_MS);
        let grown = nodes.group(number, kept, &mut sent).expect("growth");
        assert_eq!(interact_bytes(&grown), ["04b22d010101"]);
    }

    #[test]
    fn a_node_is_gathered_once_until_it_grows_back() {
        let mut nodes = Nodes::default();
        assert_eq!(nodes.join(FLINT, FLINT_POINT, AT, PLAYER, SENT, None), Ok(1));
        assert_eq!(
            nodes.join(FLINT, FLINT_POINT, AT, PLAYER + 1, SENT, None),
            Err(NodeBusy::Gathered)
        );
        assert_eq!(
            nodes.numbers_near(AT, RADIUS_M).count(),
            1,
            "a node in gather shows its gatherer"
        );
        nodes.deplete(&FLINT, HARVESTED_AT_MS, RESPAWN_AT_MS);
        assert_eq!(
            nodes.join(FLINT, FLINT_POINT, AT, PLAYER, SENT, None),
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
            nodes.join(FLINT, FLINT_POINT, AT, PLAYER, SENT, None),
            Err(NodeBusy::Depleted)
        );
        nodes.tick(RESPAWN_AT_MS);
        let grown = nodes.group(number, kept, &mut sent).expect("change");
        assert_eq!(grown.members.len(), 2, "the states without the card");
        assert_eq!(state_of(&grown), Some(HEALTHY));
        assert_eq!(
            nodes.join(FLINT, FLINT_POINT, AT, PLAYER + 1, SENT, None),
            Ok(1)
        );
        nodes.release(&FLINT, PLAYER + 1);
        assert_eq!(
            nodes.join(FLINT, FLINT_POINT, AT, PLAYER, SENT, None),
            Ok(1),
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

    /// A gather of an instance gate's switch fires the rules of its start when it begins and
    /// those of its use when it ends (the catacombs' secret lever: its door opens at the pull's
    /// start, the lever moves at its end, `CATACOMBS2` 431.572 and 433.574 s).
    #[test]
    fn a_gate_switch_gather_fires_its_start_then_its_use() {
        use nw_content::dungeon_gates::{
            Entry, Gate, GateAction, GateLevel, GateRule, GateRuleSet, LevelRules, Trigger,
        };
        use nw_messages::{DEPLETED_GATHERABLE, HEALTHY_GATHERABLE};

        use super::super::gates::{GateChange, LevelGates, gatherable_member};
        /// The lever's gatherable (`catacombs_lever_wall_torch`).
        const SWITCH: &str = "Raid_CutlassKeys00_SplitSwitch";
        const LEVER: &str = "SecretLeverA@0";
        const DOOR: &str = "SecretDoorA@0";
        const GATHERABLE: u64 = 2;
        const LEVER_NUMBER: u64 = 70;
        const DOOR_NUMBER: u64 = 71;
        let (mut world, entity) = admitted();
        let mut content = (*world.content).clone();
        content.gatherables = Gatherables::build([gatherable(SWITCH, true)]);
        world.content = Arc::new(content);
        let at = world
            .world
            .get::<Body>(entity)
            .map(|body| body.motion.position())
            .expect("body");
        let gate = |name: &str| Gate {
            name: name.to_owned(),
            entity: 1,
            slice: format!("slices/test/{name}.dynamicslice"),
            asset: [5; 16],
            position: [at.x, at.y, at.z],
            yaw_deg: 0.0,
            position_component: Some(1),
            gatherable_component: Some(GATHERABLE),
            timelines: Vec::new(),
            interact_component: None,
        };
        let opens = |gate: &str, when: Trigger| GateRuleSet {
            gate: gate.to_owned(),
            entry: Entry::Present {
                state: Some(HEALTHY_GATHERABLE),
            },
            rules: vec![GateRule {
                when,
                then: GateAction::Set {
                    state: DEPLETED_GATHERABLE,
                    timeline: None,
                },
            }],
        };
        let level = GateLevel {
            gates: vec![gate(LEVER), gate(DOOR)],
            ..GateLevel::default()
        };
        let rules = LevelRules {
            gates: vec![
                opens(LEVER, Trigger::SwitchUsed { switch: LEVER.to_owned() }),
                opens(DOOR, Trigger::SwitchStarted { switch: LEVER.to_owned() }),
            ],
            ..LevelRules::default()
        };
        let (mut gates, _) = LevelGates::new(&level, Some(&rules), 3);
        gates.set_number(0, Some(LEVER_NUMBER));
        gates.set_number(1, Some(DOOR_NUMBER));
        let (lever_ref, _) = gates.identity(0).expect("lever");
        world.dungeon_gates = Some(gates);
        let pull = InteractionRequest {
            target: lever_ref,
            ..request(1, 0, SWITCH)
        };
        world.gather_open_world(entity, NUMBER, pull, &[], NOW);
        let end_ms = world.open_gathers.first().expect("the pull runs").end_ms;
        let opened = world
            .dungeon_gates
            .as_mut()
            .map(|gates| gates.step(&[], NOW))
            .unwrap_or_default();
        let open = gatherable_member(GATHERABLE, DEPLETED_GATHERABLE);
        let changed = |changes: &[GateChange], wanted: usize| {
            changes.iter().any(|change| {
                matches!(change, GateChange::Change { gate, members }
                    if *gate == wanted && members.contains(&open))
            })
        };
        assert!(changed(&opened, 1) && !changed(&opened, 0), "{opened:?}");
        world.gathers_due(end_ms);
        let pulled = world
            .dungeon_gates
            .as_mut()
            .map(|gates| gates.step(&[], end_ms))
            .unwrap_or_default();
        assert!(changed(&pulled, 0), "{pulled:?}");
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

    /// The amount left that the own running gather state of player `entity` last sent.
    fn amount_sent(world: &SimWorld, entity: Entity) -> Option<f32> {
        world
            .world
            .get::<Player>(entity)?
            .extra_members
            .iter()
            .filter_map(|member| match &member.state {
                MemberState::Gathering(state) => state.amount_remaining,
                _ => None,
            })
            .next_back()
    }

    /// Vein `0x11`'s target, the object whose work the tests keep.
    const KEPT_VEIN: [u8; RAW_ID_LEN] = [0x11; RAW_ID_LEN];

    /// The object keys of the test requests' facets ([`request`]: interactions 0 to 7).
    const TEST_REQUESTS_OBJECT: u64 = 0;

    /// The test content's vein keeps the work of its stopped gathers; the objects of the test
    /// requests are `instanced` or shared.
    fn with_kept_vein(world: &mut SimWorld, instanced: bool) {
        world.content = Arc::new(PlayerContent {
            gatherables: Gatherables::build([gatherable(VEIN, false)]).with_kept_progress([VEIN]),
            instanced_gathers: instanced_objects(instanced),
            ..(*world.content).clone()
        });
    }

    /// The objects of the test requests when they are `instanced`, else none; their slice has
    /// no area of interest radius, so their tracker entries stay listed at any distance.
    fn instanced_objects(instanced: bool) -> InstancedGathers {
        if instanced {
            instanced_within(None)
        } else {
            InstancedGathers::default()
        }
    }

    /// The objects of the test requests as instanced quest objects of area of interest radius
    /// `aoi_radius_m` (`None`: none).
    fn instanced_within(aoi_radius_m: Option<f32>) -> InstancedGathers {
        InstancedGathers::build(
            [],
            [(
                TEST_REQUESTS_OBJECT,
                InstancedObject {
                    loot_type: QUEST_LOOT,
                    aoi_radius_m,
                },
            )],
        )
    }

    /// `InstancedLootType::Quest`: the loot type of the test requests' instanced objects (the
    /// type of `master_quest_instanced`).
    const QUEST_LOOT: u32 = 4;

    /// The sun disk shield's three gathers of `1624_0401_thedemonscauldron` (20.0, then 10.85 and
    /// 3.06 left): a stopped gather of a gatherable that keeps its work leaves the work of its
    /// whole 100 ms steps done, the next gather of the object starts with the rest, a gather of
    /// another object between changes nothing, and the harvest takes it.
    #[test]
    fn a_stopped_gather_that_keeps_its_work_resumes_with_what_is_left() {
        /// 1,250 ms of gather: 12 whole steps of 100 ms at efficiency 2 did 2,400 ms of work.
        const STOPPED_AFTER_MS: u64 = 1_250;
        const LEFT_MS: u64 = VEIN_MS - 2_400;
        const RESUMED_AT_MS: u64 = 3_000;
        let (mut world, entity) = admitted();
        with_kept_vein(&mut world, false);
        world.gather_open_world(entity, NUMBER, request(1, 0x11, VEIN), &[], NOW);
        world.cancel_gather(entity, 1, NOW + STOPPED_AFTER_MS);
        assert!(world.open_gathers.is_empty(), "stopped");
        assert_eq!(mining(&world, entity), 0, "no harvest");
        // Another vein, stopped within its first step: all its work is left.
        world.gather_open_world(entity, NUMBER, request(2, 0x12, VEIN), &[], NOW + 2_000);
        world.cancel_gather(entity, 2, NOW + 2_050);
        world.gather_open_world(entity, NUMBER, request(3, 0x11, VEIN), &[], NOW + RESUMED_AT_MS);
        let [gather] = world.open_gathers.as_slice() else {
            panic!("one gather runs: {:?}", world.open_gathers);
        };
        assert_eq!(gather.work_ms, LEFT_MS, "the first vein's work left");
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "a gather time of seconds"
        )]
        let left_at_tool_ms = (LEFT_MS as f64 / PICK_EFFICIENCY) as u64;
        assert_eq!(gather.end_ms, NOW + RESUMED_AT_MS + left_at_tool_ms);
        #[allow(clippy::cast_precision_loss, reason = "a gather time of seconds")]
        let left_s = LEFT_MS as f32 / MS_PER_SECOND;
        assert!(
            amount_sent(&world, entity).is_some_and(|amount| (amount - left_s).abs() < f32::EPSILON),
            "the own state's amount left: {:?}",
            amount_sent(&world, entity)
        );
        world.gathers_due(NOW + RESUMED_AT_MS + left_at_tool_ms);
        assert!(mining(&world, entity) > 0, "the harvest");
        assert_eq!(world.kept_gathers.take(&KEPT_VEIN), None, "the harvest took it");
        assert_eq!(
            world.kept_gathers.take(&[0x12; RAW_ID_LEN]),
            Some(VEIN_MS),
            "a stop within the first step did no work"
        );
    }

    /// Every retail gather of a gatherable whose `ResetGatherTimeOnEndInteraction` is 1 started
    /// over after an interrupt, and so did the gathers of instanced objects (the well
    /// `98_Syndicate_03_Well_02`, which keeps its work by its cell).
    #[test]
    fn a_stopped_gather_that_starts_over_keeps_nothing() {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "a gather time of seconds"
        )]
        let full_ms = (VEIN_MS as f64 / PICK_EFFICIENCY) as u64;
        let (mut world, entity) = admitted();
        world.gather_open_world(entity, NUMBER, request(1, 0x11, VEIN), &[], NOW);
        world.cancel_gather(entity, 1, NOW + 1_250);
        world.gather_open_world(entity, NUMBER, request(2, 0x11, VEIN), &[], NOW + 2_000);
        let end_ms = world.open_gathers.first().expect("the gather runs").end_ms;
        assert_eq!(end_ms, NOW + 2_000 + full_ms, "a gatherable that starts over");
        world.cancel_gather(entity, 2, NOW + 2_500);
        let (mut world, entity) = admitted();
        with_kept_vein(&mut world, true);
        world.gather_open_world(entity, NUMBER, request(1, 0x11, VEIN), &[], NOW);
        world.cancel_gather(entity, 1, NOW + 1_250);
        assert_eq!(world.kept_gathers.take(&KEPT_VEIN), None, "an instanced object");
    }

    /// The work left on a corpse goes when its mob is no longer dead, on a start object when it
    /// no longer stands; any other stays.
    #[test]
    fn kept_work_goes_with_its_corpse_or_its_start_object() {
        const NO_SUCH_MOB: u64 = 0x7777;
        const NO_SUCH_OBJECT: u64 = 0x8888;
        let (mut world, _) = admitted();
        let kept = |mob, start_object| KeptGather {
            work_ms: VEIN_MS,
            mob,
            start_object,
        };
        world.kept_gathers.keep([1; RAW_ID_LEN], kept(Some(NO_SUCH_MOB), None));
        world.kept_gathers.keep([2; RAW_ID_LEN], kept(None, Some(NO_SUCH_OBJECT)));
        world.kept_gathers.keep(KEPT_VEIN, kept(None, None));
        world.gathers_due(NOW);
        assert_eq!(world.kept_gathers.take(&[1; RAW_ID_LEN]), None, "no dead mob");
        assert_eq!(world.kept_gathers.take(&[2; RAW_ID_LEN]), None, "no standing object");
        assert_eq!(world.kept_gathers.take(&KEPT_VEIN), Some(VEIN_MS), "a level node's");
    }

    /// `0513_LighthouseLure` (`InteractOnlyOnQuest` 1, tag `IO_0513_LighthouseLure`): a gather of
    /// an object that is only on quest is refused until an active task of the player's quest
    /// names its tag, and refused again when that task is done.
    #[test]
    fn a_quest_only_object_is_gathered_only_while_a_task_names_its_tag() {
        use nw_content::interact_tags::{InteractTags, VariantTag};

        use crate::objectives::{TaskBook, TaskRow};
        use crate::player_content::{ObjectiveRow, QuestBook};
        const LURE_TAG: &str = "IO_0513_LighthouseLure";
        const LURE_QUEST: &str = "0511_0101";
        const LURE_EVENT: &str = "0513_LighthouseLure_interact";
        let (mut world, entity) = admitted();
        let task = |id: &str, kind: &str| TaskRow {
            task_id: id.to_owned(),
            kind: kind.to_owned(),
            ..TaskRow::default()
        };
        let quests = QuestBook::build(
            [ObjectiveRow {
                id: LURE_QUEST.to_owned(),
                root: "lure_root".to_owned(),
                ..ObjectiveRow::default()
            }],
            TaskBook::new(vec![
                TaskRow {
                    sub_tasks: vec!["lure_use".to_owned(), "lure_after".to_owned()],
                    ..task("lure_root", "ConsecutiveTaskContainer")
                },
                TaskRow {
                    kill_enemy_type: Some(LURE_EVENT.to_owned()),
                    interact_tag: Some(LURE_TAG.to_owned()),
                    target_qty: Some(1),
                    ..task("lure_use", "TaskGameEvent")
                },
                TaskRow {
                    item_name: Some(PICK.to_owned()),
                    ..task("lure_after", "TaskEquipItem")
                },
            ]),
        );
        let interact_tags = InteractTags::build(&HashMap::new(), [], []).with_gatherables([(
            lowercase_crc32(VEIN),
            VariantTag {
                tag: LURE_TAG.to_owned(),
                only_on_quest: true,
            },
        )]);
        world.content = Arc::new(PlayerContent {
            quests,
            interact_tags,
            ..(*world.content).clone()
        });
        world.gather_open_world(entity, NUMBER, request(1, 0x11, VEIN), &[], NOW);
        assert!(world.open_gathers.is_empty(), "no quest names the tag");
        world.start_quest(NUMBER, LURE_QUEST, NOW).expect("start");
        world.gather_open_world(entity, NUMBER, request(2, 0x11, VEIN), &[], NOW);
        assert_eq!(world.open_gathers.len(), 1, "the lure task is active");
        world.cancel_gather(entity, 2, NOW + 1);
        world.quest_event(NUMBER, Event::GameEvent(LURE_EVENT), NOW + 2);
        world.gather_open_world(entity, NUMBER, request(3, 0x11, VEIN), &[], NOW + 3);
        assert!(world.open_gathers.is_empty(), "the lure task is done");
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
        let mut gathers = world.world.get::<Player>(entity).expect("player").gathers;
        world.gathers_due(end_ms);
        assert!(world.open_gathers.is_empty());
        let player = world.world.get::<Player>(entity).expect("player");
        let done = water_gather_ended(&mut gathers);
        assert_eq!(player.gathers, gathers, "one whole gather counted");
        assert_eq!(
            player.extra_members.last(),
            Some(&done),
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

    /// An instanced chest (`master_lootcontainer`) is opened for its opener alone.
    #[test]
    fn an_opened_chest_is_locked_for_its_opener_only() {
        const CHEST: &str = "SupplyCrate";
        let (mut world, entity) = admitted();
        let mut content = (*world.content).clone();
        content.gatherables = Gatherables::build([Gatherable {
            loot_container: true,
            ..gatherable(CHEST, true)
        }]);
        with_resets(&mut content);
        content.instanced_gathers = instanced_objects(true);
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
            world.nodes.join(target, FLINT_POINT, AT, PLAYER, SENT, None),
            Ok(1),
            "the chest is not depleted for others"
        );
        world.nodes.release(&target, PLAYER);
        world.gather_open_world(entity, NUMBER, request(2, 0x15, CHEST), &[], end_ms);
        assert!(world.open_gathers.is_empty(), "locked for its opener");
    }

    /// Gives `content` the reset clock of the EU Central server (the well ledgers' server).
    pub(crate) fn with_resets(content: &mut PlayerContent) {
        use nw_content::loot_limits::{LootLimitRules, LootLimits};
        use nw_content::resets::{ResetClock, ServerRegion};
        content.loot_limits = Some(LootLimitRules {
            limits: LootLimits::default(),
            clock: ResetClock {
                region: ServerRegion::EuCentral,
            },
        });
    }

    /// The respawn time of the test well (retail `98_Syndicate_03_Well_01`: 200 to 300 s).
    const WELL_RESPAWN_MS: (u64, u64) = (200_000, 300_000);
    /// The test well's gatherable and its variant's interact tag.
    const WELL: &str = "98_Syndicate_03_Well_01";
    const WELL_TAG: &str = "IO_98_Syndicate_03_Well_01";
    /// The test well's target.
    const WELL_TARGET: [u8; RAW_ID_LEN] = [0x17; RAW_ID_LEN];
    /// Sunday 2026-10-25 04:00 UTC, Unix ms: the EU Central reset (05:00 CET) of the day
    /// European summer time ended (retail: 25 capped entries at 04:00 UTC from then).
    const WINTER_RESET_UNIX_MS: u64 = 1_792_900_800_000;

    /// A world whose instanced test well (no loot container, slice radius `aoi_radius_m`) is at
    /// the player's feet, its end at `server_ms`.
    fn with_instanced_well(server_ms: u64, aoi_radius_m: Option<f32>) -> (SimWorld, Entity) {
        use nw_content::interact_tags::{InteractTags, VariantTag};
        let (mut world, entity) = admitted();
        let mut content = (*world.content).clone();
        content.gatherables = Gatherables::build([Gatherable {
            respawn_ms: WELL_RESPAWN_MS,
            ..gatherable(WELL, true)
        }]);
        with_resets(&mut content);
        content.instanced_gathers = instanced_within(aoi_radius_m);
        content.interact_tags = InteractTags::build(&HashMap::new(), [], []).with_gatherables([(
            lowercase_crc32(WELL),
            VariantTag {
                tag: WELL_TAG.to_owned(),
                only_on_quest: false,
            },
        )]);
        world.content = Arc::new(content);
        place_nodes(&mut world, NUMBER, &[WELL]);
        world.gather_open_world(entity, NUMBER, request(1, 0x17, WELL), &[], NOW);
        let end_ms = world.open_gathers.first().expect("the well is gathered").end_ms;
        world.server_ms = server_ms;
        world.gathers_due(end_ms);
        (world, entity)
    }

    /// Retail `Playthrough_Part_21` 1341.31 s: the instanced well `98_Syndicate_03_Well_01`
    /// gave its gatherer a quest entry (type 4, its variant's tag `0x0f02dfa8`) whose reset came
    /// 228.3 s later, of its respawn time of 200 to 300 s; the well stayed for the others, and
    /// its gatherer could not gather it again while the entry lived.
    #[test]
    fn an_instanced_object_is_locked_for_its_gatherer_until_a_roll_of_its_respawn_time() {
        /// Midnight UTC (the server epoch, a winter day): the next daily reset is 4 h away
        /// (05:00 CET).
        const MIDNIGHT_SERVER_MS: u64 = 0;
        let (mut world, entity) = with_instanced_well(MIDNIGHT_SERVER_MS, None);
        {
            let mut player = world.world.get_mut::<Player>(entity).expect("player");
            assert_eq!(
                player.loot.opened_kind(&WELL_TARGET),
                Some((QUEST_LOOT, lowercase_crc32(WELL_TAG))),
                "the slice's loot type and the variant's tag"
            );
            assert!(
                player
                    .loot
                    .due(AT, MIDNIGHT_SERVER_MS + WELL_RESPAWN_MS.0 - 1, None, 1)
                    .is_none(),
                "locked for at least the shortest respawn time"
            );
        }
        assert_eq!(
            world
                .nodes
                .join(WELL_TARGET, FLINT_POINT, AT, PLAYER, SENT, None),
            Ok(1),
            "the well is not depleted for the others"
        );
        world.nodes.release(&WELL_TARGET, PLAYER);
        world.gather_open_world(entity, NUMBER, request(2, 0x17, WELL), &[], NOW);
        assert!(world.open_gathers.is_empty(), "locked for its gatherer");
        let mut player = world.world.get_mut::<Player>(entity).expect("player");
        assert!(
            player
                .loot
                .due(AT, MIDNIGHT_SERVER_MS + WELL_RESPAWN_MS.1, None, 2)
                .is_some(),
            "the lock ends by the longest respawn time"
        );
        assert!(!player.loot.opened(&WELL_TARGET), "free again");
    }

    /// Retail (139 entries, the vistas of 86,400 s and the catacomb chests of 64,000 s among
    /// them): an instanced object's lock never outlasts the world's next daily reset.
    #[test]
    fn an_instanced_lock_ends_at_the_next_daily_reset_at_the_latest() {
        /// The time left to the daily reset, shorter than the well's shortest respawn time.
        const BEFORE_RESET_MS: u64 = 100_000;
        let reset_server_ms = WINTER_RESET_UNIX_MS - SERVER_EPOCH_UNIX_MS;
        let (mut world, entity) = with_instanced_well(reset_server_ms - BEFORE_RESET_MS, None);
        let mut player = world.world.get_mut::<Player>(entity).expect("player");
        assert!(player.loot.opened(&WELL_TARGET), "locked");
        assert!(
            player.loot.due(AT, reset_server_ms - 1, None, 1).is_none(),
            "locked until the reset"
        );
        assert!(
            player.loot.due(AT, reset_server_ms, None, 2).is_some(),
            "free at the daily reset"
        );
    }

    /// Finding D9 "tracker entries out of range" (`master_quest_instanced`, radius 100 m: 1,481
    /// retail removals of median 99.5 m, 241 re-adds of median 98.5 m) and D10 (12 of 12
    /// re-adds of capped entries): the well's entry leaves its gatherer's list beyond its slice's
    /// radius while the lock stays, and comes back within it looted at the return; its lock,
    /// capped at the daily reset, then ends whole seconds after the return at that reset.
    #[test]
    fn an_instanced_objects_entry_follows_its_gatherer_by_its_slice_radius() {
        /// The `m_aoiRadius` of `master_quest_instanced`, m.
        const QUEST_AOI_RADIUS_M: f32 = 100.0;
        /// The time left to the daily reset at the gather's end, shorter than the well's
        /// shortest respawn time: its lock is capped there.
        const BEFORE_RESET_MS: u64 = 100_000;
        /// When the gatherer is far, ms after the gather's end.
        const FAR_AFTER_MS: u64 = 1_000;
        /// When the gatherer is back, ms after the gather's end (the end is a whole second),
        /// and its sub-second part.
        const BACK_AFTER_MS: u64 = 21_250;
        const BACK_SUBSECOND_MS: u64 = 250;
        let reset_server_ms = WINTER_RESET_UNIX_MS - SERVER_EPOCH_UNIX_MS;
        let ended_ms = reset_server_ms - BEFORE_RESET_MS;
        let (mut world, entity) = with_instanced_well(ended_ms, Some(QUEST_AOI_RADIUS_M));
        let clock = world.content.loot_limits.as_ref().map(|rules| rules.clock);
        let feet = world
            .world
            .get::<Body>(entity)
            .expect("body")
            .motion
            .position();
        // Twice the radius: beyond it from each of the test nodes, a metre apart.
        let far = Position {
            x: feet.x + 2.0 * QUEST_AOI_RADIUS_M,
            ..feet
        };
        let mut player = world.world.get_mut::<Player>(entity).expect("player");
        assert!(
            player.loot.due(feet, ended_ms + 1, clock, 1).is_none(),
            "listed within the radius"
        );
        assert!(
            player
                .loot
                .due(far, ended_ms + FAR_AFTER_MS, clock, 2)
                .is_some()
        );
        assert_eq!(
            player.loot.listed(&WELL_TARGET),
            None,
            "off the list beyond it"
        );
        assert!(player.loot.opened(&WELL_TARGET), "still locked");
        let back_ms = ended_ms + BACK_AFTER_MS;
        assert!(player.loot.due(feet, back_ms, clock, 3).is_some());
        let entry = player.loot.listed(&WELL_TARGET).expect("listed again");
        assert_eq!(
            entry.looted_ns,
            gear::unix_ns(back_ms),
            "looted at the return"
        );
        assert_eq!(
            entry.reset_ns,
            gear::unix_ns(reset_server_ms + BACK_SUBSECOND_MS),
            "the daily reset with the return's sub-second part"
        );
        assert!(
            player
                .loot
                .due(feet, reset_server_ms + BACK_SUBSECOND_MS, clock, 4)
                .is_some(),
            "free at its reset"
        );
        assert!(!player.loot.opened(&WELL_TARGET));
    }

    /// Retail: 842 of 842 entries of chests without a respawn time reset 3,600 s after the open.
    #[test]
    fn an_instanced_object_without_a_respawn_time_is_locked_an_hour() {
        let (mut world, entity) = admitted();
        let mut content = (*world.content).clone();
        content.gatherables = Gatherables::build([Gatherable {
            respawn_ms: NO_RESPAWN_RATE,
            ..gatherable(WELL, true)
        }]);
        with_resets(&mut content);
        content.instanced_gathers = instanced_objects(true);
        world.content = Arc::new(content);
        place_nodes(&mut world, NUMBER, &[WELL]);
        world.gather_open_world(entity, NUMBER, request(1, 0x17, WELL), &[], NOW);
        let end_ms = world.open_gathers.first().expect("gathered").end_ms;
        world.server_ms = 0;
        world.gathers_due(end_ms);
        let mut player = world.world.get_mut::<Player>(entity).expect("player");
        assert_eq!(
            player.loot.opened_kind(&WELL_TARGET),
            Some((QUEST_LOOT, NO_TRACKED_TAG)),
            "no tag"
        );
        assert!(
            player
                .loot
                .due(AT, CONTAINER_LOCKOUT_MS - 1, None, 1)
                .is_none()
        );
        assert!(player.loot.due(AT, CONTAINER_LOCKOUT_MS, None, 2).is_some());
    }

    /// Retail (94 of 96 elite chest entries): an elite chest stays locked for its opener until
    /// the server's next daily reset, not for its hour.
    #[test]
    fn an_elite_chest_is_locked_until_the_next_daily_reset() {
        use nw_content::chest_lockouts::{ContainerRow, EliteChests};
        const CHEST: &str = "EliteChest";
        /// Saturday 2026-10-24 12:00 UTC, Unix ms.
        const OPENED_UNIX_MS: u64 = 1_792_843_200_000;
        /// Sunday 2026-10-25 04:00 UTC, Unix ms: the next EU Central reset (05:00 CET).
        const RESET_UNIX_MS: u64 = 1_792_900_800_000;
        let (mut world, entity) = admitted();
        let mut content = (*world.content).clone();
        content.gatherables = Gatherables::build([Gatherable {
            loot_container: true,
            respawn_ms: NO_RESPAWN_RATE,
            ..gatherable(CHEST, true)
        }]);
        content.elite_chests = EliteChests::build([ContainerRow {
            id: CHEST,
            container: true,
            elite: true,
        }]);
        with_resets(&mut content);
        content.instanced_gathers = instanced_objects(true);
        world.content = Arc::new(content);
        place_nodes(&mut world, NUMBER, &[CHEST]);
        world.gather_open_world(entity, NUMBER, request(1, 0x15, CHEST), &[], NOW);
        let end_ms = world.open_gathers.first().expect("the chest is opened").end_ms;
        let opened_server_ms = OPENED_UNIX_MS - SERVER_EPOCH_UNIX_MS;
        let reset_server_ms = RESET_UNIX_MS - SERVER_EPOCH_UNIX_MS;
        world.server_ms = opened_server_ms;
        world.gathers_due(end_ms);
        let mut player = world.world.get_mut::<Player>(entity).expect("player");
        assert!(
            player
                .loot
                .due(AT, opened_server_ms + CONTAINER_LOCKOUT_MS, None, 1)
                .is_none(),
            "still locked after the hour of a common chest"
        );
        assert!(player.loot.due(AT, reset_server_ms - 1, None, 2).is_none());
        assert!(
            player.loot.due(AT, reset_server_ms, None, 3).is_some(),
            "free at the daily reset"
        );
    }

    /// The own gather states that player `entity` got, in order.
    fn gathering_states(world: &SimWorld, entity: Entity) -> Vec<GatheringState> {
        world
            .world
            .get::<Player>(entity)
            .map(|player| {
                player
                    .extra_members
                    .iter()
                    .filter_map(|member| match member.state {
                        MemberState::Gathering(state) => Some(state),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Retail `Playthrough_Part_04`: a shared object's gather counts its gatherer at its start
    /// (4540.67 s, 4617.04 s) and sets the count back to 0 at its end (4545.40 s, 4620.13 s); an
    /// instanced quest object's counts none and its end sends no count (110.99 s, 114.01 s).
    #[test]
    fn a_gather_counts_its_gatherer_unless_its_object_is_instanced() {
        for (instanced, start, end) in [(false, Some(1), Some(0)), (true, Some(0), None)] {
            let (mut world, entity) = admitted();
            with_kept_vein(&mut world, instanced);
            world.gather_open_world(entity, NUMBER, request(1, 0x11, VEIN), &[], NOW);
            let end_ms = world.open_gathers.first().expect("the gather runs").end_ms;
            world.gathers_due(end_ms);
            let states = gathering_states(&world, entity);
            let counts: Vec<Option<u8>> = states.iter().map(|state| state.gatherer_count).collect();
            assert_eq!(counts, [start, end], "instanced {instanced}");
        }
    }

    /// The Brimstone glyph locks (`16BS_AncientGlyphLock_Chest`, a shared loot container whose
    /// cell keeps its work) kept their work after a stop and gave no loot tracker entry: a shared
    /// container is depleted for everyone, an instanced one locks for its opener alone.
    #[test]
    fn a_shared_loot_container_keeps_its_work_and_is_depleted_for_everyone() {
        const LOCK: &str = "16BS_AncientGlyphLock_Chest";
        let (mut world, entity) = admitted();
        let mut content = (*world.content).clone();
        content.gatherables = Gatherables::build([Gatherable {
            loot_container: true,
            ..gatherable(LOCK, true)
        }])
        .with_kept_progress([LOCK]);
        with_resets(&mut content);
        world.content = Arc::new(content);
        place_nodes(&mut world, NUMBER, &[LOCK]);
        let target = [0x16; RAW_ID_LEN];
        world.gather_open_world(entity, NUMBER, request(1, 0x16, LOCK), &[], NOW);
        world.cancel_gather(entity, 1, NOW + 1_000);
        world.gather_open_world(entity, NUMBER, request(2, 0x16, LOCK), &[], NOW + 2_000);
        let gather = world.open_gathers.first().expect("the gather runs");
        assert_eq!(gather.work_ms, VEIN_MS - 1_000, "the work left of the stop");
        let end_ms = gather.end_ms;
        world.gathers_due(end_ms);
        let player = world.world.get::<Player>(entity).expect("player");
        assert!(!player.loot.opened(&target), "no tracker entry");
        assert_eq!(
            world.nodes.join(target, FLINT_POINT, AT, PLAYER, SENT, None),
            Err(NodeBusy::Depleted),
            "depleted for everyone"
        );
    }

    /// The group monolith of `EXP-The-Depths` (needs 3): no work with 2 gatherers, a third's
    /// stop 340 ms after it came leaves 3 steps done, and the other two get the work left with
    /// their new count; with 3 again the work goes on and every gather ends with it.
    #[test]
    fn a_group_node_works_only_while_enough_players_gather_it() {
        const MONOLITH: &str = "DungeonRestlessShores_MinorMonolith";
        const MONOLITH_MS: u64 = 10_000;
        const NEEDED: u32 = 3;
        const THIRD_AT_MS: u64 = 4_370;
        const THIRD_STOP_AT_MS: u64 = 4_710;
        const THIRD_BACK_AT_MS: u64 = 5_270;
        const LEFT_MS: u64 = MONOLITH_MS - 300;
        let (mut world, first) = admitted();
        world.content = Arc::new(PlayerContent {
            gatherables: Gatherables::build([Gatherable {
                gather_ms: MONOLITH_MS,
                ..gatherable(MONOLITH, true)
            }])
            .with_kept_progress([MONOLITH])
            .with_gatherers_needed([(MONOLITH, NEEDED)]),
            ..(*world.content).clone()
        });
        place_nodes(&mut world, NUMBER, &[MONOLITH]);
        let mut players = vec![(NUMBER, first)];
        for number in [NUMBER + 1, NUMBER + 2] {
            world
                .admit(&admit_at(number, 0.0, true), NOW, SERVER_TIME)
                .expect("admit");
            players.push((number, world.by_connection[&connection(number)]));
        }
        let gather = |world: &mut SimWorld, at: usize, interaction: u64, at_ms: u64| {
            let (number, entity) = players[at];
            let request = request(interaction, 0x30, MONOLITH);
            world.gather_open_world(entity, number, request, &[], NOW + at_ms);
        };
        gather(&mut world, 0, 1, 0);
        gather(&mut world, 1, 2, 100);
        assert!(
            world.open_gathers.iter().all(|gather| gather.end_ms == u64::MAX),
            "no work with 2 gatherers"
        );
        gather(&mut world, 2, 3, THIRD_AT_MS);
        assert!(
            world
                .open_gathers
                .iter()
                .all(|gather| gather.end_ms == NOW + THIRD_AT_MS + MONOLITH_MS),
            "3 gatherers: the work goes on"
        );
        let (_, third) = players[2];
        world.cancel_gather(third, 3, NOW + THIRD_STOP_AT_MS);
        #[allow(clippy::cast_precision_loss, reason = "a gather time of seconds")]
        let left_s = LEFT_MS as f32 / MS_PER_SECOND;
        let (_, firsts) = players[0];
        let told: Vec<(Option<u8>, Option<f32>)> = gathering_states(&world, firsts)
            .iter()
            .map(|state| (state.gatherer_count, state.amount_remaining))
            .collect();
        assert_eq!(
            told,
            [
                (Some(1), Some(10.0)),
                (Some(2), None),
                (Some(3), None),
                (Some(2), Some(left_s))
            ],
            "the first gatherer's start and the counts of the others' comings and goings"
        );
        gather(&mut world, 2, 4, THIRD_BACK_AT_MS);
        assert_eq!(
            gathering_states(&world, third)
                .last()
                .map(|state| (state.gatherer_count, state.amount_remaining)),
            Some((Some(3), Some(left_s))),
            "the third comes back to the work left"
        );
        let end_ms = NOW + THIRD_BACK_AT_MS + LEFT_MS;
        assert!(world.open_gathers.iter().all(|gather| gather.end_ms == end_ms));
        world.gathers_due(end_ms);
        assert!(world.open_gathers.is_empty(), "every gather ends with the work");
        assert_eq!(
            world.nodes.group_work_left(&[0x30; RAW_ID_LEN], end_ms),
            Some(MONOLITH_MS),
            "the harvest starts the work over"
        );
        // Retail monolith ends in one tick: +2 for the first ender, +3 for the second
        // (`HiveNormaleTheDepths` 1754.06 s), +4 for the third (`the_depths_group_run_tank_pov`
        // 1708.60 s).
        let end_steps: Vec<u32> = players
            .iter()
            .map(|(_, entity)| {
                let refs: Vec<u32> = gathering_states(&world, *entity)
                    .iter()
                    .filter_map(|state| state.gather_ref_count)
                    .collect();
                refs[refs.len() - 1] - refs[refs.len() - 2]
            })
            .collect();
        assert_eq!(end_steps, [2, 3, 4]);
        let first_refs: Vec<u32> = gathering_states(&world, firsts)
            .iter()
            .filter_map(|state| state.gather_ref_count)
            .collect();
        assert_eq!(
            first_refs,
            [2, 3, 4, 5, 6, 8],
            "a shared start, a step for each count change, the end"
        );
    }

    /// A group node's work counts whole 100 ms steps at its gatherers' mean efficiency, only
    /// while enough of them gather; with nobody left the work done stays.
    #[test]
    fn group_work_goes_on_only_while_enough_gather() {
        const RATE: f32 = 1.5;
        let mut work = GroupWork::new(3, 8_000);
        assert!(!work.change(0, 2, RATE), "no work before");
        assert_eq!(work.end_ms(), None, "2 of 3 wait");
        assert_eq!(work.left_at(5_000), 8_000);
        assert!(!work.change(5_000, 3, RATE));
        assert_eq!(work.left_at(5_250), 8_000 - 300, "2 whole steps at 1.5");
        assert_eq!(work.end_ms(), Some(5_000 + 5_333));
        assert!(work.change(5_250, 0, RATE), "the work done was counted");
        assert_eq!(work.left_at(60_000), 7_700, "kept while nobody gathers");
        work.reset();
        assert_eq!(work.left_at(60_000), 8_000);
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
