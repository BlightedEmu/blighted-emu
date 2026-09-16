// SPDX-License-Identifier: AGPL-3.0-or-later
//! The tutorial of one player (live-tutorial note): its quest's task tree, its fights and its
//! move into the water.
//!
//! The quest `01_SoleSurvivor2` runs as a task tree ([`crate::objectives`]); the achievements it
//! sets unlock the client's videos, cutscenes, hints and doors. Beside the tree, the server runs
//! what the level's encounters do on retail:
//!
//! - The ship fight (`ftue_v2_enc_boatwreck`): one corrupted sailor waits near the player until
//!   the client ends its quick-time lesson (6638), then fights; when it dies, two more come.
//! - While the ship-sinking video plays, the player moves into the water (retail: 13.5 s after
//!   the video started, with no loading screen).
//! - Later fights start when tasks of the tree are done: the legionnaire after the ruin door, the
//!   signifier at the first fight area, the corruption entity and its swarmers at the climb, and
//!   Thorpe after the fountain cutscene.
//! - Thorpe is not killed by a hit on retail (tutorial-ai note, section 6): at 60 % health he
//!   stops for an intermission, heals to full and comes back; at 50 % of his second health bar
//!   his scripted death comes, and the client starts the next cutscene. His death is no kill of
//!   the quest.
//! - The island's static entities (ftue-static-entities note) show after the move into the water.
//! - The beach bag: after the client takes all its items (2939), retail completes two of its
//!   delayed events and removes it.
//! - The ruin chest: the client starts the interaction in its input; the server accepts it, and
//!   after the gather time ends it and gives two health potions.
//!
//! Every mob of the tutorial is personal: only its player sees it. A dead mob's body stays for
//! [`CORPSE_MS`], then it goes.

use std::sync::Arc;

use nw_formats::objectstream::lowercase_crc32;
use nw_sim_core::Position;

use nw_content::ftue_entities::{FtueLevel, GDE_REF_PREFIX_LEN};
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{ClientFacetMsg, InteractionRequest};

use super::asset_script::{AssetScript, Step, Trigger};
use crate::mobs::{Mobs, Pace, VitalsSpawn};
use crate::objectives::{Effect, Event, KillTypes, ObjectiveRun, ObjectiveTree, TaskProgress};

/// When the first sailor comes after the quest starts (retail: its card came 15.9 s after the
/// player's first own state, 316 to 16198 ms, when the encounter's `Wave_01` spawns the mariner;
/// a card sent at once came before the client could show it, and the sailor stayed unseen).
pub(super) const FIRST_SAILOR_DELAY_MS: u64 = 15_882;
/// The level of every tutorial mob (retail cards).
const TUTORIAL_MOB_LEVEL: u32 = 1;
/// How long a dead mob's body stays (our choice: long enough for its death to play).
pub(super) const CORPSE_MS: u64 = 10_000;
/// The video during which the player moves into the water.
const SHIP_SINKING_VIDEO: &str = "ftue_intro3";
/// Time from the start of that video to the move (retail 13.5 s).
const WATER_MOVE_DELAY_MS: u64 = 13_500;
/// Where the player comes into the water (retail own position after the move).
const WATER_AT: Position = Position {
    x: 793.84,
    y: 337.42,
    z: 26.16,
};
/// Where the player faces in the water (retail own state 118546: rotation `66f6`, yaw 1.434 rad).
const WATER_FACING: f32 = 1.434;
/// Thorpe's intermission starts when his health falls to this part of his maximum (retail
/// `Boss_Phase` entry: health 60 %; he stopped at 59.5 %).
const CAPTAIN_INTERMISSION_FRACTION: f64 = 0.6;
/// Thorpe's boss phase in his intermission (retail 403 at 493283).
const CAPTAIN_INTERMISSION_PHASE: u32 = 1;
/// Thorpe's boss phase at his end (retail 403 at 546981, with his death).
const CAPTAIN_END_PHASE: u32 = 3;
/// When he heals to full after the intermission starts (retail: 493283 to 499917 ms). Until then
/// hits on him do no damage (retail: five 2071 with amount 0 from 494210 to 499506).
const CAPTAIN_INTERMISSION_MS: u64 = 6_634;
/// How long he stands in his phase transition before he fights again (retail fragment
/// `Phase_Transition_1_to_2` from 493283 to 503682 ms).
const CAPTAIN_TRANSITION_MS: u64 = 10_399;
/// After the intermission, Thorpe's scripted death comes at this part of his maximum (retail
/// `Boss_Phase` entry: health 50 %; he ended at 48.1 %).
const CAPTAIN_END_FRACTION: f64 = 0.5;
/// The first `GdeRef` bytes of the beach bag (`FTUE_Satchel`).
pub(super) const BAG_PREFIX: [u8; GDE_REF_PREFIX_LEN] =
    [0x2f, 0xb4, 0x00, 0x65, 0x01, 0x28, 0x98, 0xd5];
/// The bag's change groups in the table: its first and second delayed event completed.
const BAG_FIRST_EVENT_CHANGE: usize = 0;
const BAG_SECOND_EVENT_CHANGE: usize = 1;
/// Retail times from the take to those changes and to the bag's removal.
const BAG_FIRST_EVENT_MS: u64 = 134;
const BAG_SECOND_EVENT_MS: u64 = 1282;
const BAG_GONE_MS: u64 = 1298;
/// The first `GdeRef` bytes of the ruin chest (gatherable `LootContainer_AncientSmall`).
pub(super) const CHEST_PREFIX: [u8; GDE_REF_PREFIX_LEN] =
    [0xef, 0xa5, 0xa0, 0x9f, 0x81, 0x35, 0x70, 0x58];
/// The chest's change groups in the table: in use, then free again.
const CHEST_BUSY_CHANGE: usize = 0;
const CHEST_FREE_CHANGE: usize = 1;
/// Retail time from the server's accept to the end of the gather.
const CHEST_GATHER_MS: u64 = 2023;
/// The lore the player learns when the quest ends (retail achievement bits 21-25, 27-31 and
/// 54-60 at 653960; xp-potion-exit note, section 3.2).
const LEAVING_LORE: [&str; 17] = [
    "Lore_Unlock_DeathandRebirthLore1a",
    "Lore_Unlock_DeathandRebirthLore1b",
    "Lore_Unlock_DeathandRebirthLore1c",
    "Lore_Unlock_DeathandRebirthLore1d",
    "Lore_Unlock_DeathandRebirthLore2",
    "Lore_Unlock_DeathandRebirthLore4a",
    "Lore_Unlock_DeathandRebirthLore4b",
    "Lore_Unlock_DeathandRebirthLore4c",
    "Lore_Unlock_DeathandRebirthLore4d",
    "Lore_Unlock_SealedCommission",
    "Lore_Unlock_GettingStarted_All",
    "Lore_Unlock_Mechanics_All",
    "Lore_Unlock_Crafting_All",
    "Lore_Unlock_TradingPost_All",
    "Lore_Unlock_Progression_All",
    "Lore_Unlock_Companies_All",
    "Lore_Unlock_Combat_All",
];
/// What the chest gives (retail: two health potions).
const CHEST_ITEM: &str = "PotionHealthT1";
const CHEST_QUANTITY: u32 = 2;

/// What a tutorial mob is to the script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// A sailor of the ship fight.
    Sailor,
    /// A mob of a later fight.
    Foe,
    /// Thorpe, the captain.
    Captain,
}

/// One mob a fight spawns.
#[derive(Debug, Clone, Copy)]
struct SpawnPoint {
    /// Its vitals row.
    vitals: &'static str,
    /// Its vitals category row; `None` sends category 0 (retail Thorpe card).
    category: Option<&'static str>,
    at: Position,
    role: Role,
    /// Its `CharacterComponent` constant, for the collision call at its death (live-tutorial
    /// note, section 7).
    collision: u64,
    /// How it fights.
    pace: Pace,
}

/// How the tutorial mobs fight (tutorial-ai and creature-animation session notes, live capture
/// 2026-09-12). The attack factor makes one landed hit as large as retail's on a player in
/// starter armor (keeping 0.6006): level 1 base damage 145.52 x the type's damage mod x factor x
/// 0.6006 = the retail 3601 amount. The swing gap is the AI's attack cooldown plus the attack's
/// length; the speed is its combat run speed; the hit delay is when retail's hit landed after the
/// attack started.
///
/// Stamina that a blocked swing takes per unit of its `BlockStaminaDmgMod`, with the warhammer of
/// the capture: 14.6 / 0.2 for the signifier's `Legionnaire_LightStab` (Observed). Thorpe's
/// attacks took 91.25 per unit; the cause of the difference is Unknown.
const BLOCK_STAMINA_PER_MOD: f32 = 73.0;
/// Ship sailors (damage mod 0.5): both landed hits did 12.97; cooldown 1-2 s + attack 1.5-2.0 s;
/// run 5.5 m/s; hit at 456 ms. Block stamina: no blocked sailor hit took stamina in the capture,
/// so it is [`BLOCK_STAMINA_PER_MOD`] x the club primary's `BlockStaminaDmgMod` 0.26 (Inferred).
const SAILOR_PACE: Pace = Pace {
    attack_factor: 0.2968,
    swing_gap_ms: (2_500, 4_000),
    speed_mps: 5.5,
    hit_delay_ms: 456,
    block_stamina: BLOCK_STAMINA_PER_MOD * 0.26,
};
/// Legionnaire and signifier (damage mod 0.7): 18.70 (262336); cooldown 1-1.5 s + stab about
/// 0.9-1.2 s; run 5.5 m/s; hit at 627 ms. A blocked light stab took 14.6 stamina (266620).
const LEGION_PACE: Pace = Pace {
    attack_factor: 0.3057,
    swing_gap_ms: (1_900, 2_700),
    speed_mps: 5.5,
    hit_delay_ms: 627,
    block_stamina: 14.6,
};
/// Risen corruption entity (damage mod 0.9): 18.24 (295343); cooldown 0.75-1 s + swipe about
/// 0.8-0.9 s; run 4.5 m/s; hit at 555 ms. Block stamina: [`BLOCK_STAMINA_PER_MOD`] x the
/// `Attack` row's 0.1 (Inferred).
const RISEN_PACE: Pace = Pace {
    attack_factor: 0.2319,
    swing_gap_ms: (1_550, 1_900),
    speed_mps: 4.5,
    hit_delay_ms: 555,
    block_stamina: BLOCK_STAMINA_PER_MOD * 0.1,
};
/// Corruption swarmers (damage mod 0.25): 7.07 (296477); rate limit 0.25 s + slash (length
/// Unknown; our choice 1 s); run 1.5 m/s; hit at 533 ms. Block stamina: [`BLOCK_STAMINA_PER_MOD`]
/// x the swipe's 0.1 (Inferred).
const SWARMER_PACE: Pace = Pace {
    attack_factor: 0.3236,
    swing_gap_ms: (1_250, 1_500),
    speed_mps: 1.5,
    hit_delay_ms: 533,
    block_stamina: BLOCK_STAMINA_PER_MOD * 0.1,
};
/// Thorpe (damage mod 1.125): 25.6 (457997); cooldown 1.15-2.25 s + attack 1.4-2.2 s; run
/// 4.5 m/s; hit at 611 ms (stab). A blocked stab took 25.55 stamina (481395, 489462).
const THORPE_PACE: Pace = Pace {
    attack_factor: 0.2604,
    swing_gap_ms: (2_550, 4_450),
    speed_mps: 4.5,
    hit_delay_ms: 611,
    block_stamina: 25.55,
};

/// Experience points per island foe killed (retail: 20 per legion or corrupted kill, 120 in all;
/// xp-potion-exit note, section 1).
const FOE_EXPERIENCE: u32 = 20;

/// The `CharacterComponent` constants of the tutorial mobs' prefabs.
const SAILOR_COLLISION: u64 = 0x2d2e_b7bf_87ae_c03f;
const LEGION_COLLISION: u64 = 0x8d6e_b762_2ca4_b0f0;
const SWARMER_COLLISION: u64 = 0x1283_1ae7_e65b_8fe9;
const CORRUPTION_COLLISION: u64 = 0x4885_e5b5_6a03_82e8;

const fn point(x: f32, y: f32, z: f32) -> Position {
    Position { x, y, z }
}

const SAILOR_VITALS: &str = "Corrupted_Sailor_1H_Club_FTUE";
const SAILOR_CATEGORY: Option<&str> = Some("Corrupted_Sailor_1H_Club");
/// The first sailor (retail spawn point `sp01`).
const FIRST_SAILOR: SpawnPoint = SpawnPoint {
    vitals: SAILOR_VITALS,
    category: SAILOR_CATEGORY,
    at: point(467.617, 846.008, 90.16),
    role: Role::Sailor,
    collision: SAILOR_COLLISION,
    pace: SAILOR_PACE,
};
/// The two later sailors (retail `sp01` and `sp02` of the second wave).
const LATER_SAILORS: [SpawnPoint; 2] = [
    SpawnPoint {
        at: point(468.734, 847.221, 89.99),
        ..FIRST_SAILOR
    },
    SpawnPoint {
        at: point(468.734, 845.17, 89.99),
        ..FIRST_SAILOR
    },
];
const LEGION_VITALS: &str = "Legion_Legionnaire_FTUE";
const LEGION_CATEGORY: Option<&str> = Some("Legion_Legionnaire");
const CORRUPTION_CATEGORY: Option<&str> = Some("Corruption_Entity");
const SWARMER_VITALS: &str = "Corruption_Entity_Crawler_FTUE";
const SWARMER: SpawnPoint = SpawnPoint {
    vitals: SWARMER_VITALS,
    category: CORRUPTION_CATEGORY,
    at: point(580.06, 383.2, 21.74),
    role: Role::Foe,
    collision: SWARMER_COLLISION,
    pace: SWARMER_PACE,
};
/// The fights that start when a task is done, with the retail cards' positions.
const FIGHTS_AFTER: [(&str, &[SpawnPoint]); 4] = [
    (
        "01_SoleSurvivor2_UnlockDoor",
        &[SpawnPoint {
            vitals: LEGION_VITALS,
            category: LEGION_CATEGORY,
            at: point(668.27, 385.16, 27.35),
            role: Role::Foe,
            collision: LEGION_COLLISION,
            pace: LEGION_PACE,
        }],
    ),
    (
        "01_SoleSurvivor2_FTUE_First_Fight_Trigger",
        &[SpawnPoint {
            vitals: LEGION_VITALS,
            category: LEGION_CATEGORY,
            at: point(621.6, 419.5, 20.99),
            role: Role::Foe,
            collision: LEGION_COLLISION,
            pace: LEGION_PACE,
        }],
    ),
    (
        "01_SoleSurvivor2_Combat02_Trigger_Failsafe_ForClimb",
        &[
            SpawnPoint {
                vitals: "Corruption_Entity",
                category: CORRUPTION_CATEGORY,
                at: point(579.62, 381.91, 21.74),
                role: Role::Foe,
                collision: CORRUPTION_COLLISION,
                pace: RISEN_PACE,
            },
            SWARMER,
            SpawnPoint {
                at: point(580.67, 379.57, 21.74),
                ..SWARMER
            },
            SpawnPoint {
                at: point(578.21, 380.97, 21.81),
                ..SWARMER
            },
        ],
    ),
    (
        "01_SoleSurvivor2_FTUE_TF_cutsceneComplete",
        &[SpawnPoint {
            vitals: "Damned_Commander_FTUE",
            category: None,
            at: point(490.25, 209.89, 22.83),
            role: Role::Captain,
            collision: LEGION_COLLISION,
            pace: THORPE_PACE,
        }],
    ),
];

/// The spawn of `point`.
fn spawn_of(point: &SpawnPoint) -> VitalsSpawn {
    VitalsSpawn {
        vitals_id: lowercase_crc32(point.vitals),
        category_id: point.category.map_or(0, lowercase_crc32),
        level: TUTORIAL_MOB_LEVEL,
        position: [point.at.x, point.at.y, point.at.z],
        is_encounter: true,
        ..VitalsSpawn::default()
    }
}

/// Where Thorpe's script stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptainPhase {
    /// His first health bar.
    First,
    /// He stands still until `until_ms`, then heals to full.
    Intermission { until_ms: u64 },
    /// His second health bar.
    Second,
}

/// Where the ship fight stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShipWave {
    /// The first sailor waits for the end of the quick-time lesson.
    Waiting,
    /// The first sailor fights.
    First,
    /// The two later sailors fight, or the fight is over.
    Later,
}

/// What the tutorial asks of the server for its player.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Order {
    /// Set this achievement.
    Achievement(String),
    /// Remove this item.
    RemoveItem(String),
    /// Give this many of this item.
    GiveItem(String, u32),
    /// The player's potions of this item become a new stack of this many in quick slot 1, and
    /// its cooldowns end (retail when the quest ends).
    RefillPotions(String, u32),
    /// Move the player here, facing `facing` (yaw, radians).
    MoveTo(Position, f32),
    /// The player gets these experience points.
    Experience(u32),
    /// Clear this achievement.
    LockAchievement(String),
    /// Show the island's static entities.
    ShowIsland,
    /// Send this change group of the static entity with this `GdeRef` start.
    StaticChange([u8; GDE_REF_PREFIX_LEN], usize),
    /// Remove the static entity with this `GdeRef` start.
    HideStatic([u8; GDE_REF_PREFIX_LEN]),
    /// Accept this gather.
    AcceptGather(InteractionRequest),
    /// End this gather.
    EndGather(InteractionRequest),
    /// This mob picked the player as its target; a sailor's tutorial AI also saw it.
    Targeted {
        /// The mob's entity number.
        mob: u64,
        /// Whether it has the tutorial AI (the sailors).
        tutorial_ai: bool,
    },
    /// A group of this asset entity of the level table goes out.
    Asset(usize, Step),
    /// This boss mob is in boss phase `phase` now.
    BossPhase {
        /// The mob's entity number.
        mob: u64,
        /// Its phase.
        phase: u32,
    },
    /// This mob died: it drops the player as its target and stops colliding.
    Died {
        /// The mob's entity number.
        mob: u64,
        /// Its `CharacterComponent` constant.
        collision: u64,
    },
}

/// A living tutorial mob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fighter {
    entity: u64,
    vitals_id: u32,
    role: Role,
    collision: u64,
    /// The batch it spawned with.
    batch: usize,
}

/// The tutorial of one player.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Tutorial {
    owner: u64,
    run: ObjectiveRun,
    wave: ShipWave,
    /// Where Thorpe's script stands.
    captain: CaptainPhase,
    /// The end of the quest gave its rewards.
    rewarded: bool,
    fighters: Vec<Fighter>,
    /// Dead mobs and when their bodies go.
    corpses: Vec<(u64, u64)>,
    /// When the player moves into the water; `None` before the video or after the move.
    water_at_ms: Option<u64>,
    moved_to_water: bool,
    bag_taken: bool,
    /// Whether a chest gather runs.
    gathering: bool,
    /// The interaction id of the last accepted gather: the client repeats its last input records
    /// in later messages, so one press comes up to three times.
    last_interaction: Option<u64>,
    chest_looted: bool,
    /// Orders due later, with their times.
    later: Vec<(u64, Order)>,
    /// The level's asset entities.
    assets: AssetScript,
    /// Mobs spawned so far.
    spawned: usize,
    /// The spawn order places of each batch of mobs.
    batches: Vec<Vec<usize>>,
    /// The time of the call being handled.
    now_ms: u64,
    /// When the first sailor comes; `None` once it came.
    first_sailor_at_ms: Option<u64>,
}

impl Tutorial {
    /// The tutorial of player `owner` at `now_ms` in the level of `level`: the quest of `tree`
    /// starts and the first sailor waits in `mobs`. The orders of the quest's first tasks and of
    /// the level's first entities.
    pub(super) fn start(
        owner: u64,
        tree: Arc<ObjectiveTree>,
        level: &FtueLevel,
        mobs: &mut Mobs,
        now_ms: u64,
    ) -> (Self, Vec<Order>) {
        let (run, effects) = ObjectiveRun::start(tree, now_ms);
        let mut tutorial = Self {
            owner,
            run,
            wave: ShipWave::Waiting,
            captain: CaptainPhase::First,
            rewarded: false,
            fighters: Vec::new(),
            corpses: Vec::new(),
            water_at_ms: None,
            moved_to_water: false,
            bag_taken: false,
            gathering: false,
            last_interaction: None,
            chest_looted: false,
            later: Vec::new(),
            assets: AssetScript::of(level),
            spawned: 0,
            batches: Vec::new(),
            now_ms,
            first_sailor_at_ms: Some(now_ms.saturating_add(FIRST_SAILOR_DELAY_MS)),
        };
        tutorial.assets.on(Trigger::Start, now_ms);
        let mut orders = Vec::new();
        tracing::info!(player = owner, "tutorial: the quest starts");
        tutorial.apply(effects, mobs, &mut orders);
        (tutorial, orders)
    }

    /// The first sailor comes when it is due at `now_ms`, or at once when `now`: it waits, held,
    /// for the end of the quick-time lesson.
    fn first_sailor_comes(&mut self, mobs: &mut Mobs, now: bool, orders: &mut Vec<Order>) {
        if self
            .first_sailor_at_ms
            .is_some_and(|at| now || self.now_ms >= at)
        {
            self.first_sailor_at_ms = None;
            self.spawn_batch(&[FIRST_SAILOR], mobs, true, orders);
            tracing::info!(
                player = self.owner,
                "tutorial: the first sailor comes and waits"
            );
        }
    }

    /// Spawns the mobs of `points` as one batch, held idle when `held`; a mob that fights at once
    /// targets the player (an order in `orders`).
    fn spawn_batch(
        &mut self,
        points: &[SpawnPoint],
        mobs: &mut Mobs,
        held: bool,
        orders: &mut Vec<Order>,
    ) {
        let batch = self.batches.len();
        self.batches.push(Vec::new());
        for point in points {
            self.spawn(point, batch, mobs, held, orders);
        }
    }

    fn spawn(
        &mut self,
        point: &SpawnPoint,
        batch: usize,
        mobs: &mut Mobs,
        held: bool,
        orders: &mut Vec<Order>,
    ) {
        let spawn = spawn_of(point);
        let vitals_id = spawn.vitals_id;
        let Some(entity) =
            mobs.spawn_personal(spawn, self.owner, held, Some(point.pace), self.now_ms)
        else {
            tracing::warn!(vitals = point.vitals, "tutorial mob without combat rows");
            return;
        };
        let order = self.spawned;
        self.spawned += 1;
        self.batches[batch].push(order);
        self.assets.on(Trigger::MobSpawned(order), self.now_ms);
        self.fighters.push(Fighter {
            entity,
            vitals_id,
            role: point.role,
            collision: point.collision,
            batch,
        });
        // Thorpe fights the player from his spawn (retail 2415 at 451137 with the player 26.5 m
        // away, beyond the brain's sight).
        if point.role == Role::Captain
            && !held
            && let Some(mob) = mobs.get_mut(entity)
        {
            nw_sim_core::hurt(&mut mob.brain, self.owner, 0, self.now_ms);
        }
        if !held {
            orders.push(Order::Targeted {
                mob: entity,
                tutorial_ai: point.role == Role::Sailor,
            });
        }
    }

    /// The quick-time lesson ended at `now_ms`: the first sailor fights and targets the player.
    pub(super) fn lesson_done(&mut self, mobs: &mut Mobs, now_ms: u64) -> Vec<Order> {
        if self.wave != ShipWave::Waiting {
            return Vec::new();
        }
        self.now_ms = now_ms;
        // A lesson that ends before the first sailor's time brings it at once.
        let mut orders = Vec::new();
        self.first_sailor_comes(mobs, true, &mut orders);
        mobs.release(self.owner, now_ms);
        self.wave = ShipWave::First;
        tracing::info!(player = self.owner, "tutorial: the first sailor fights");
        orders.extend(
            self.fighters
                .iter()
                .filter(|fighter| fighter.role == Role::Sailor)
                .map(|fighter| Order::Targeted {
                    mob: fighter.entity,
                    tutorial_ai: true,
                }),
        );
        orders
    }

    /// The player takes all items of container `container` at `now_ms`: whether it is the beach
    /// bag and still held them (once per tutorial). The bag then goes as on retail.
    pub(super) fn take_bag(&mut self, container: &[u8; RAW_ID_LEN], now_ms: u64) -> bool {
        if container[..GDE_REF_PREFIX_LEN] != BAG_PREFIX || self.bag_taken {
            return false;
        }
        self.bag_taken = true;
        self.later.extend([
            (
                now_ms + BAG_FIRST_EVENT_MS,
                Order::StaticChange(BAG_PREFIX, BAG_FIRST_EVENT_CHANGE),
            ),
            (
                now_ms + BAG_SECOND_EVENT_MS,
                Order::StaticChange(BAG_PREFIX, BAG_SECOND_EVENT_CHANGE),
            ),
            (now_ms + BAG_GONE_MS, Order::HideStatic(BAG_PREFIX)),
        ]);
        true
    }

    /// The client starts interaction `request` at `now_ms`: a use of the chest is accepted, and
    /// the gather ends after the retail gather time, with the potions the first time.
    pub(super) fn interact(&mut self, request: InteractionRequest, now_ms: u64) -> Vec<Order> {
        if request.target[..GDE_REF_PREFIX_LEN] != CHEST_PREFIX
            || self.gathering
            || self.last_interaction == Some(request.interaction)
        {
            return Vec::new();
        }
        self.gathering = true;
        self.last_interaction = Some(request.interaction);
        let done_at = now_ms + CHEST_GATHER_MS;
        self.later.push((done_at, Order::EndGather(request)));
        self.later.push((
            done_at,
            Order::StaticChange(CHEST_PREFIX, CHEST_FREE_CHANGE),
        ));
        if !std::mem::replace(&mut self.chest_looted, true) {
            self.later.push((
                done_at,
                Order::GiveItem(CHEST_ITEM.to_owned(), CHEST_QUANTITY),
            ));
        }
        tracing::info!(player = self.owner, "tutorial: the chest is opened");
        vec![
            Order::AcceptGather(request),
            Order::StaticChange(CHEST_PREFIX, CHEST_BUSY_CHANGE),
        ]
    }

    /// The client sent `message` at `now_ms`: the level's asset entities that wait for it.
    pub(super) fn client_message(&mut self, message: &ClientFacetMsg, now_ms: u64) {
        let call = |message_type, id| Trigger::Client { message_type, id };
        let trigger = match *message {
            ClientFacetMsg::CinematicStarted { cinematic, .. } => call(
                nw_messages::facet::CINEMATIC_STARTED_TYPE_INDEX,
                Some(cinematic),
            ),
            ClientFacetMsg::CinematicEnded { cinematic, .. } => call(
                nw_messages::facet::CINEMATIC_ENDED_TYPE_INDEX,
                Some(cinematic),
            ),
            ClientFacetMsg::RequestCutscene { cutscene, .. } => call(
                nw_messages::facet::REQUEST_CUTSCENE_TYPE_INDEX,
                Some(cutscene),
            ),
            ClientFacetMsg::CutsceneEnded { .. } => {
                call(nw_messages::facet::CUTSCENE_ENDED_TYPE_INDEX, None)
            }
            ClientFacetMsg::QteCompleted { sequence, .. } => {
                call(nw_messages::facet::QTE_COMPLETED_TYPE_INDEX, Some(sequence))
            }
            ClientFacetMsg::TakeAllItems { .. } => {
                call(nw_messages::facet::TAKE_ALL_ITEMS_TYPE_INDEX, None)
            }
            ClientFacetMsg::EquipItem { .. } => {
                call(nw_messages::facet::EQUIP_ITEM_TYPE_INDEX, None)
            }
            ClientFacetMsg::ReturnToMainWorld { .. } => {
                call(nw_messages::facet::RETURN_TO_MAIN_WORLD_TYPE_INDEX, None)
            }
            ClientFacetMsg::TriggerAreaPassthrough { entity, .. } => {
                let mut prefix = [0; GDE_REF_PREFIX_LEN];
                prefix.copy_from_slice(&entity[..GDE_REF_PREFIX_LEN]);
                Trigger::Passthrough(prefix)
            }
            ClientFacetMsg::AckLevelInfo { .. } | ClientFacetMsg::UiScreenOpened { .. } => {
                return;
            }
        };
        self.assets.on(trigger, now_ms);
    }

    /// The server set achievement `bit` at `now_ms`.
    pub(super) fn achievement(&mut self, bit: u32, now_ms: u64) {
        self.assets.on(Trigger::Achievement(bit), now_ms);
    }

    /// The orders of the quest's waiting item tasks that the player's items `held` (item id,
    /// worn) already meet, at `now_ms`.
    pub(super) fn sync_held(
        &mut self,
        held: &[(&str, bool)],
        mobs: &mut Mobs,
        now_ms: u64,
    ) -> Vec<Order> {
        self.now_ms = now_ms;
        let mut orders = Vec::new();
        let effects = self.run.sync_held(held, now_ms);
        self.apply(effects, mobs, &mut orders);
        orders
    }

    /// The orders of game event `event` at `now_ms`.
    pub(super) fn event(&mut self, event: Event<'_>, mobs: &mut Mobs, now_ms: u64) -> Vec<Order> {
        self.now_ms = now_ms;
        let sinking = lowercase_crc32(SHIP_SINKING_VIDEO);
        let mut orders = Vec::new();
        match event {
            Event::CinematicStarted(video) if video == sinking && !self.moved_to_water => {
                self.water_at_ms = Some(now_ms.saturating_add(WATER_MOVE_DELAY_MS));
            }
            Event::CinematicEnded(video) if video == sinking => self.move_to_water(&mut orders),
            _ => {}
        }
        let effects = self.run.event(event, now_ms);
        self.apply(effects, mobs, &mut orders);
        orders
    }

    fn move_to_water(&mut self, orders: &mut Vec<Order>) {
        if !std::mem::replace(&mut self.moved_to_water, true) {
            self.water_at_ms = None;
            orders.push(Order::MoveTo(WATER_AT, WATER_FACING));
            orders.push(Order::ShowIsland);
            self.assets.on(Trigger::WaterMove, self.now_ms);
            tracing::info!(player = self.owner, "tutorial: into the water");
        }
    }

    /// One tick at `now_ms` with the player at `position`: deaths count as kills, the next
    /// sailors come, Thorpe ends, bodies go, timers and later orders run, and areas are checked.
    pub(super) fn tick(
        &mut self,
        mobs: &mut Mobs,
        kill_types: &KillTypes,
        position: Position,
        now_ms: u64,
    ) -> Vec<Order> {
        self.now_ms = now_ms;
        let mut orders = Vec::new();
        self.first_sailor_comes(mobs, false, &mut orders);
        self.end_captain(mobs, now_ms, &mut orders);
        let (dead, living): (Vec<Fighter>, Vec<Fighter>) = self
            .fighters
            .iter()
            .partition(|fighter| mobs.health(fighter.entity).is_none_or(|health| health == 0));
        self.fighters = living;
        for fighter in &dead {
            tracing::info!(
                player = self.owner,
                mob = fighter.entity,
                "tutorial: a mob died"
            );
            self.corpses
                .push((fighter.entity, now_ms.saturating_add(CORPSE_MS)));
            orders.push(Order::Died {
                mob: fighter.entity,
                collision: fighter.collision,
            });
            let batch_dead = !self
                .fighters
                .iter()
                .any(|living| living.batch == fighter.batch);
            if batch_dead {
                for order in self.batches[fighter.batch].clone() {
                    self.assets.on(Trigger::BatchDead(order), now_ms);
                }
            }
            // Thorpe's end is scripted, not a kill.
            if fighter.role == Role::Captain {
                continue;
            }
            // Retail gives experience for the island's foes only, not for the sailors.
            if fighter.role == Role::Foe {
                orders.push(Order::Experience(FOE_EXPERIENCE));
            }
            if let Some((vitals_id, categories)) = kill_types.of(fighter.vitals_id) {
                let effects = self.run.event(
                    Event::Killed {
                        vitals_id,
                        categories,
                    },
                    now_ms,
                );
                self.apply(effects, mobs, &mut orders);
            }
        }
        if self.wave == ShipWave::First && dead.iter().any(|dead| dead.role == Role::Sailor) {
            self.wave = ShipWave::Later;
            self.spawn_batch(&LATER_SAILORS, mobs, false, &mut orders);
            tracing::info!(player = self.owner, "tutorial: two more sailors come");
        }
        self.corpses.retain(|(mob, gone_at_ms)| {
            let stays = now_ms < *gone_at_ms;
            if !stays {
                mobs.remove_personal(*mob);
            }
            stays
        });
        if self.water_at_ms.is_some_and(|at| now_ms >= at) {
            self.move_to_water(&mut orders);
        }
        let effects = self.run.tick(now_ms);
        self.apply(effects, mobs, &mut orders);
        let effects = self.run.event(Event::Moved(position), now_ms);
        self.apply(effects, mobs, &mut orders);
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.later)
            .into_iter()
            .partition(|(at, _)| now_ms >= *at);
        self.later = later;
        for (_, order) in due {
            if matches!(order, Order::EndGather(_)) {
                self.gathering = false;
            }
            orders.push(order);
        }
        orders.extend(
            self.assets
                .take_due(now_ms)
                .into_iter()
                .map(|(asset, step)| Order::Asset(asset, step)),
        );
        orders
    }

    /// Runs Thorpe's script at `now_ms`: the intermission at 60 % health, the full heal after
    /// it, and the end at 50 % of his second health bar; the intermission and the end set his
    /// boss phase.
    fn end_captain(&mut self, mobs: &mut Mobs, now_ms: u64, orders: &mut Vec<Order>) {
        let captains: Vec<u64> = self
            .fighters
            .iter()
            .filter(|fighter| fighter.role == Role::Captain)
            .map(|fighter| fighter.entity)
            .collect();
        for captain in captains {
            let Some(mob) = mobs.get_mut(captain) else {
                continue;
            };
            let (current, max) = (mob.health.current(), mob.health.max());
            let below =
                |fraction: f64| current > 0 && f64::from(current) <= f64::from(max) * fraction;
            match self.captain {
                CaptainPhase::First if below(CAPTAIN_INTERMISSION_FRACTION) => {
                    let until_ms = now_ms + CAPTAIN_INTERMISSION_MS;
                    self.captain = CaptainPhase::Intermission { until_ms };
                    mobs.freeze(captain, now_ms + CAPTAIN_TRANSITION_MS);
                    mobs.absorb(captain, until_ms);
                    orders.push(Order::BossPhase {
                        mob: captain,
                        phase: CAPTAIN_INTERMISSION_PHASE,
                    });
                    tracing::info!(player = self.owner, "tutorial: Thorpe's intermission");
                }
                CaptainPhase::Intermission { until_ms } if now_ms >= until_ms => {
                    mob.health.refill();
                    self.captain = CaptainPhase::Second;
                }
                CaptainPhase::Second if below(CAPTAIN_END_FRACTION) => {
                    mob.health.take(current);
                    // A scripted end: the body neither chases nor swings any more.
                    nw_sim_core::died(&mut mob.brain, now_ms);
                    orders.push(Order::BossPhase {
                        mob: captain,
                        phase: CAPTAIN_END_PHASE,
                    });
                    tracing::info!(player = self.owner, "tutorial: Thorpe is beaten");
                }
                _ => {}
            }
        }
    }

    /// Turns `effects` into orders, and starts the fights of the tasks they finish.
    fn apply(&mut self, effects: Vec<Effect>, mobs: &mut Mobs, orders: &mut Vec<Order>) {
        for effect in effects {
            match effect {
                Effect::Achievement(id) => orders.push(Order::Achievement(id)),
                Effect::LockAchievement(id) => orders.push(Order::LockAchievement(id)),
                Effect::RemoveItem(id) => orders.push(Order::RemoveItem(id)),
                Effect::Done(task) => {
                    let fights = FIGHTS_AFTER
                        .iter()
                        .filter(|(after, _)| after.eq_ignore_ascii_case(&task));
                    for (_, points) in fights {
                        self.spawn_batch(points, mobs, false, orders);
                    }
                    tracing::debug!(player = self.owner, %task, "tutorial task done");
                }
            }
        }
        if self.run.finished() && !std::mem::replace(&mut self.rewarded, true) {
            orders.extend(
                LEAVING_LORE
                    .iter()
                    .map(|lore| Order::Achievement((*lore).to_owned())),
            );
            orders.push(Order::RefillPotions(CHEST_ITEM.to_owned(), CHEST_QUANTITY));
            tracing::info!(player = self.owner, "tutorial: the quest is done");
        }
    }

    /// The quest's task list for the quest tracker.
    pub(super) fn progress(&self) -> Vec<TaskProgress> {
        self.run.progress()
    }

    /// Whether the quest is done.
    pub(super) fn finished(&self) -> bool {
        self.run.finished()
    }

    /// Removes every mob and body of the tutorial (the player left).
    pub(super) fn end(self, mobs: &mut Mobs) {
        let fighters = self.fighters.into_iter().map(|fighter| fighter.entity);
        let corpses = self.corpses.into_iter().map(|(mob, _)| mob);
        for mob in fighters.chain(corpses) {
            mobs.remove_personal(mob);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use nw_content::combat::{CombatData, CombatRows, LevelRow, MobType};

    use super::*;
    use crate::combat::test_support::{TEST_LEVEL_DAMAGE, TEST_LEVEL_HEALTH, test_source};
    use crate::config::tests::committed;
    use crate::objectives::TaskRow;
    use crate::player_content::TUTORIAL_QUEST_ROOT;
    use nw_sim_core::MobState;

    const OWNER: u64 = 1;
    const NOW: u64 = 1000;
    const RETAIL_SAILOR_VITALS: u32 = 0x8869_82c9;
    const RETAIL_SAILOR_CATEGORY: u32 = 0xa452_daaa;
    const RETAIL_LEGION_CATEGORY: u32 = 0x9ccc_23f1;
    const SHIP: Position = point(478.5, 849.7, 89.59);
    const CHEST_LOOT_TASK: &str = "01_SoleSurvivor2_Tutorial_TakeChestLoot";
    const CORRUPTED: &str = "Corrupted";

    /// The vitals id of the ship's sailors.
    pub(crate) fn sailor_vitals() -> u32 {
        lowercase_crc32(SAILOR_VITALS)
    }

    /// Combat settings whose rows know every tutorial mob at its level.
    pub(crate) fn sailor_combat() -> crate::combat::CombatSettings {
        let mut source = test_source();
        let vitals = [
            SAILOR_VITALS,
            LEGION_VITALS,
            "Corruption_Entity",
            SWARMER_VITALS,
            "Damned_Commander_FTUE",
        ];
        for vitals in vitals {
            source.base_types.push((vitals.to_owned(), MobType::PLAIN));
        }
        source.levels.push((
            TUTORIAL_MOB_LEVEL,
            LevelRow {
                base_max_health: TEST_LEVEL_HEALTH,
                base_damage: TEST_LEVEL_DAMAGE,
                physical_armor_rating: 0.0,
            },
        ));
        let data = CombatData::build(source, &CombatRows::default()).expect("rows");
        committed().combat_settings(Arc::new(data), Arc::from([]))
    }

    /// Every tutorial mob is corrupted.
    pub(crate) fn kill_types() -> KillTypes {
        KillTypes::build(
            [SAILOR_VITALS, LEGION_VITALS, SWARMER_VITALS]
                .into_iter()
                .map(|vitals| (vitals.to_owned(), format!("{vitals},Hostile,{CORRUPTED}"))),
        )
    }

    fn task(task_id: &str, kind: &str) -> TaskRow {
        TaskRow {
            task_id: task_id.to_owned(),
            kind: kind.to_owned(),
            ..TaskRow::default()
        }
    }

    /// A tree in the shape of the tutorial: the intro unlock, one then two corrupted kills, the
    /// third video unlock, the sword removal, the door, and the chest's potions.
    pub(crate) fn tree() -> Arc<ObjectiveTree> {
        let rows = vec![
            TaskRow {
                sub_tasks: [
                    "Intro",
                    "Sailor",
                    "Sailors",
                    "Intro3",
                    "Sword",
                    "01_SoleSurvivor2_UnlockDoor",
                    CHEST_LOOT_TASK,
                ]
                .map(str::to_owned)
                .to_vec(),
                ..task(TUTORIAL_QUEST_ROOT, "ConsecutiveTaskContainer")
            },
            TaskRow {
                item_name: Some("FTUE_Intro2_Cinematic_Unlock".to_owned()),
                ..task("Intro", "TaskUnlockAchievement")
            },
            TaskRow {
                kill_enemy_type: Some(CORRUPTED.to_owned()),
                ..task("Sailor", "TaskKillContribution")
            },
            TaskRow {
                kill_enemy_type: Some(CORRUPTED.to_owned()),
                target_qty: Some(2),
                ..task("Sailors", "TaskKillContribution")
            },
            TaskRow {
                item_name: Some("FTUE_Intro3_Cinematic_Unlock".to_owned()),
                ..task("Intro3", "TaskUnlockAchievement")
            },
            TaskRow {
                item_name: Some("1hSwordT2_FTUE".to_owned()),
                ..task("Sword", "TaskRemoveItem")
            },
            TaskRow {
                item_name: Some("FTUE_AncientRuins_Door".to_owned()),
                ..task("01_SoleSurvivor2_UnlockDoor", "TaskUnlockAchievement")
            },
            TaskRow {
                item_name: Some(CHEST_ITEM.to_owned()),
                ..task(CHEST_LOOT_TASK, "TaskHaveAndReturnItems")
            },
        ];
        Arc::new(ObjectiveTree::build(&rows, TUTORIAL_QUEST_ROOT).expect("tree"))
    }

    fn kill_all(mobs: &mut Mobs, tutorial: &Tutorial) {
        for fighter in &tutorial.fighters {
            let mob = mobs.get_mut(fighter.entity).expect("mob");
            let max = mob.health.max();
            mob.health.take(max);
        }
    }

    #[test]
    fn the_mob_rows_are_the_retail_ids() {
        let spawn = spawn_of(&FIRST_SAILOR);
        assert_eq!(
            (spawn.vitals_id, spawn.category_id),
            (RETAIL_SAILOR_VITALS, RETAIL_SAILOR_CATEGORY)
        );
        assert_eq!(
            spawn_of(&FIGHTS_AFTER[0].1[0]).category_id,
            RETAIL_LEGION_CATEGORY
        );
        assert_eq!(spawn_of(&FIGHTS_AFTER[3].1[0]).category_id, 0);
    }

    #[test]
    fn the_quest_runs_the_ship_fight_and_the_later_fights() {
        let mut mobs = Mobs::new(Vec::new().into(), &sailor_combat());
        let (mut tutorial, orders) =
            Tutorial::start(OWNER, tree(), &FtueLevel::default(), &mut mobs, NOW);
        assert_eq!(
            orders,
            [Order::Achievement(
                "FTUE_Intro2_Cinematic_Unlock".to_owned()
            )]
        );
        assert!(tutorial.fighters.is_empty(), "the first sailor comes later");
        tutorial.tick(
            &mut mobs,
            &KillTypes::default(),
            SHIP,
            NOW + FIRST_SAILOR_DELAY_MS - 1,
        );
        assert!(tutorial.fighters.is_empty());
        tutorial.tick(
            &mut mobs,
            &KillTypes::default(),
            SHIP,
            NOW + FIRST_SAILOR_DELAY_MS,
        );
        let first = tutorial.fighters[0].entity;
        assert_eq!(mobs.owner(first), Some(OWNER));
        assert_eq!(
            tutorial.lesson_done(&mut mobs, NOW),
            [Order::Targeted {
                mob: first,
                tutorial_ai: true
            }],
            "the first sailor targets the player"
        );
        assert!(tutorial.lesson_done(&mut mobs, NOW).is_empty(), "once");
        kill_all(&mut mobs, &tutorial);
        let orders = tutorial.tick(&mut mobs, &kill_types(), SHIP, NOW);
        assert_eq!(tutorial.fighters.len(), 2, "two more sailors come");
        let later: Vec<u64> = tutorial
            .fighters
            .iter()
            .map(|fighter| fighter.entity)
            .collect();
        assert_eq!(
            orders,
            [
                Order::Died {
                    mob: first,
                    collision: SAILOR_COLLISION
                },
                Order::Targeted {
                    mob: later[0],
                    tutorial_ai: true
                },
                Order::Targeted {
                    mob: later[1],
                    tutorial_ai: true
                },
            ]
        );
        kill_all(&mut mobs, &tutorial);
        let orders = tutorial.tick(&mut mobs, &kill_types(), SHIP, NOW + 1);
        let legionnaire = tutorial.fighters[0].entity;
        assert_eq!(
            orders,
            [
                Order::Died {
                    mob: later[0],
                    collision: SAILOR_COLLISION
                },
                Order::Died {
                    mob: later[1],
                    collision: SAILOR_COLLISION
                },
                Order::Achievement("FTUE_Intro3_Cinematic_Unlock".to_owned()),
                Order::RemoveItem("1hSwordT2_FTUE".to_owned()),
                Order::Achievement("FTUE_AncientRuins_Door".to_owned()),
                Order::Targeted {
                    mob: legionnaire,
                    tutorial_ai: false
                },
            ]
        );
        assert_eq!(
            tutorial.fighters.len(),
            1,
            "the door brings the legionnaire"
        );
        assert!(mobs.health(first).is_some(), "the body stays a while");
        tutorial.tick(&mut mobs, &kill_types(), SHIP, NOW + CORPSE_MS);
        assert_eq!(mobs.health(first), None, "then it goes");
        kill_all(&mut mobs, &tutorial);
        let orders = tutorial.tick(&mut mobs, &kill_types(), SHIP, NOW + CORPSE_MS + 1);
        assert!(
            orders.contains(&Order::Experience(FOE_EXPERIENCE)),
            "an island foe gives experience: {orders:?}"
        );
        let orders = tutorial.event(Event::ItemGained(CHEST_ITEM), &mut mobs, NOW);
        assert!(tutorial.run.finished());
        let mut rewards: Vec<Order> = LEAVING_LORE
            .iter()
            .map(|lore| Order::Achievement((*lore).to_owned()))
            .collect();
        rewards.push(Order::RefillPotions(CHEST_ITEM.to_owned(), CHEST_QUANTITY));
        assert_eq!(orders, rewards, "the end gives the lore and fresh potions");
        assert!(
            tutorial
                .event(Event::ItemGained(CHEST_ITEM), &mut mobs, NOW)
                .is_empty(),
            "once"
        );
        tutorial.end(&mut mobs);
        assert_eq!(mobs.health(legionnaire), None);
    }

    #[test]
    fn the_player_moves_into_the_water_during_the_sinking_video() {
        let mut mobs = Mobs::new(Vec::new().into(), &sailor_combat());
        let (mut tutorial, _) =
            Tutorial::start(OWNER, tree(), &FtueLevel::default(), &mut mobs, NOW);
        let sinking = lowercase_crc32(SHIP_SINKING_VIDEO);
        tutorial.event(Event::CinematicStarted(sinking), &mut mobs, NOW);
        let early = tutorial.tick(
            &mut mobs,
            &kill_types(),
            SHIP,
            NOW + WATER_MOVE_DELAY_MS - 1,
        );
        assert!(!early.contains(&Order::MoveTo(WATER_AT, WATER_FACING)));
        let due = tutorial.tick(&mut mobs, &kill_types(), SHIP, NOW + WATER_MOVE_DELAY_MS);
        assert!(due.contains(&Order::MoveTo(WATER_AT, WATER_FACING)));
        let ended = tutorial.event(Event::CinematicEnded(sinking), &mut mobs, NOW);
        assert!(
            !ended.contains(&Order::MoveTo(WATER_AT, WATER_FACING)),
            "once only"
        );
    }

    #[test]
    fn the_chest_gives_its_potions_once_after_the_gather_time() {
        let mut mobs = Mobs::new(Vec::new().into(), &sailor_combat());
        let (mut tutorial, _) =
            Tutorial::start(OWNER, tree(), &FtueLevel::default(), &mut mobs, NOW);
        let mut target = [0; RAW_ID_LEN];
        target[..GDE_REF_PREFIX_LEN].copy_from_slice(&CHEST_PREFIX);
        let request = InteractionRequest {
            target,
            target_entity: 1,
            interaction: 2,
            kind: 14,
        };
        assert_eq!(
            tutorial.interact(request, NOW),
            [
                Order::AcceptGather(request),
                Order::StaticChange(CHEST_PREFIX, CHEST_BUSY_CHANGE),
            ]
        );
        assert!(
            tutorial.interact(request, NOW).is_empty(),
            "one gather at a time"
        );
        let early = tutorial.tick(&mut mobs, &kill_types(), SHIP, NOW + CHEST_GATHER_MS - 1);
        assert!(!early.contains(&Order::EndGather(request)));
        let done = tutorial.tick(&mut mobs, &kill_types(), SHIP, NOW + CHEST_GATHER_MS);
        assert!(done.contains(&Order::EndGather(request)));
        assert!(done.contains(&Order::GiveItem(CHEST_ITEM.to_owned(), CHEST_QUANTITY)));
        assert!(
            tutorial.interact(request, NOW + CHEST_GATHER_MS).is_empty(),
            "a repeated record of the same press starts nothing"
        );
        let press_again = InteractionRequest {
            interaction: request.interaction + 1,
            ..request
        };
        assert!(
            !tutorial
                .interact(press_again, NOW + CHEST_GATHER_MS)
                .is_empty()
        );
        let again = tutorial.tick(&mut mobs, &kill_types(), SHIP, NOW + 2 * CHEST_GATHER_MS);
        assert!(again.contains(&Order::EndGather(press_again)));
        assert!(
            !again.contains(&Order::GiveItem(CHEST_ITEM.to_owned(), CHEST_QUANTITY)),
            "the potions come once"
        );
        let other = InteractionRequest {
            target: [0; RAW_ID_LEN],
            ..request
        };
        assert!(tutorial.interact(other, NOW).is_empty());
    }

    #[test]
    fn the_bag_goes_after_its_take() {
        let mut mobs = Mobs::new(Vec::new().into(), &sailor_combat());
        let (mut tutorial, _) =
            Tutorial::start(OWNER, tree(), &FtueLevel::default(), &mut mobs, NOW);
        let mut bag = [0; RAW_ID_LEN];
        bag[..GDE_REF_PREFIX_LEN].copy_from_slice(&BAG_PREFIX);
        assert!(
            !tutorial.take_bag(&[0; RAW_ID_LEN], NOW),
            "another container"
        );
        assert!(tutorial.take_bag(&bag, NOW));
        assert!(!tutorial.take_bag(&bag, NOW), "the bag gives once");
        let orders = tutorial.tick(&mut mobs, &kill_types(), SHIP, NOW + BAG_GONE_MS);
        assert_eq!(
            orders,
            [
                Order::StaticChange(BAG_PREFIX, BAG_FIRST_EVENT_CHANGE),
                Order::StaticChange(BAG_PREFIX, BAG_SECOND_EVENT_CHANGE),
                Order::HideStatic(BAG_PREFIX),
            ]
        );
    }

    #[test]
    fn thorpe_pauses_heals_and_ends_at_half_of_his_second_bar() {
        let mut mobs = Mobs::new(Vec::new().into(), &sailor_combat());
        let (mut tutorial, _) =
            Tutorial::start(OWNER, tree(), &FtueLevel::default(), &mut mobs, NOW);
        tutorial.spawn_batch(FIGHTS_AFTER[3].1, &mut mobs, false, &mut Vec::new());
        let thorpe = tutorial.fighters[0].entity;
        let max = mobs.get_mut(thorpe).expect("Thorpe").health.max();
        let hurt_to = |mobs: &mut Mobs, share: f64| {
            let mob = mobs.get_mut(thorpe).expect("Thorpe");
            let keep = (f64::from(max) * share) as u32;
            let current = mob.health.current();
            mob.health.take(current - keep);
        };
        hurt_to(&mut mobs, 0.55);
        let orders = tutorial.tick(&mut mobs, &KillTypes::default(), SHIP, NOW);
        assert!(orders.contains(&Order::BossPhase {
            mob: thorpe,
            phase: CAPTAIN_INTERMISSION_PHASE
        }));
        assert!(matches!(
            tutorial.captain,
            CaptainPhase::Intermission { .. }
        ));
        assert!(mobs.absorbing(thorpe, NOW), "no damage in his intermission");
        assert!(!mobs.absorbing(thorpe, NOW + CAPTAIN_INTERMISSION_MS));
        assert!(
            mobs.health(thorpe).is_some_and(|health| health > 0),
            "no death yet"
        );
        tutorial.tick(
            &mut mobs,
            &KillTypes::default(),
            SHIP,
            NOW + CAPTAIN_INTERMISSION_MS,
        );
        assert_eq!(mobs.health(thorpe), Some(max), "healed to full");
        hurt_to(&mut mobs, 0.45);
        let orders = tutorial.tick(
            &mut mobs,
            &KillTypes::default(),
            SHIP,
            NOW + CAPTAIN_INTERMISSION_MS + 1,
        );
        assert_eq!(mobs.health(thorpe), Some(0));
        assert_eq!(
            mobs.brain_state(thorpe),
            Some(MobState::Dead),
            "his body stops at his scripted end"
        );
        assert!(orders.contains(&Order::BossPhase {
            mob: thorpe,
            phase: CAPTAIN_END_PHASE
        }));
        tutorial.tick(
            &mut mobs,
            &KillTypes::default(),
            SHIP,
            NOW + CAPTAIN_INTERMISSION_MS + 2,
        );
        assert!(
            tutorial
                .fighters
                .iter()
                .all(|fighter| fighter.entity != thorpe)
        );
    }
}
