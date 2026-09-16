// SPDX-License-Identifier: AGPL-3.0-or-later
//! The world state of `sim` (M4 spec sections 3.1, 7.3; M5 spec sections 3.3, 3.4, 7), in a
//! `bevy_ecs` world, with no I/O: the caller passes times in and sends what a tick returns.
//!
//! Every tick, each player gets replication bundles on channel 1, unreliable, as in the captures:
//! the player's own position first, then the nearest players and ghosts inside the interest
//! radius, by entity number, at most [`MAX_BUNDLE_MEMBERS`] members per bundle and one group per
//! bundle. The full position goes out every tick, so a lost datagram costs nothing. How the retail
//! service removes an entity from a client is Unknown, so a player who leaves the radius is simply
//! not in the next bundles.
//!
//! With a cell table, a tick also starts the handoff of each player that is past the margin into
//! another sim's cells (the player is frozen until the answer), and lists the players near each
//! other sim's cells as ghosts for that sim.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use bevy_ecs::change_detection::Mut;
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use nw_bus::{
    Admit, BusId, CarriedItem, CellTable, ConnectionRef, Ghost, HandoffOffer, ItemPlace,
    MOB_STATES, MobGhost, Point, QuestRun, SimSlot, SnapMob, SnapPlayer,
};
use nw_messages::items::{FIRST_VERSION, FullList};
use nw_messages::paperdoll::slot::QUICK_SLOT_1;
use nw_messages::{
    BundleGroup, BundleMember, ClientFacetMsg, ConsumableCooldown, CooldownsState,
    DisableCollisionMsg, GatheringInterruptedMsg, InteractionEndedMsg, InteractionRequest,
    InteractionValidatedMsg, LOGIN_CLIENT_CONTEXT, LevelInfoChangedMsg, LevelSpec, MessageError,
    PlayerLook, PlayerSeenByAiMsg, PlayerStates, ProgressionState, SelectedAsTargetMsg,
    ServerMessage, TUTORIAL_LEVEL, TargetMsg, TeleportStep, TimeSynchMsg, UnselectedAsTargetMsg,
    WorldPosition, facet_entity,
};
use nw_sim_core::{
    CellBounds, CellGrid, CellOwners, Dice, Ground, GroundCheck, Health, Interest, MobState,
    Motion, MotionState, MoveLimits, MoveRefusal, OPEN_WORLD_START, Position, Seen, SpatialGrid,
    at_tutorial_exit,
};
use thiserror::Error;
use uuid::Uuid;

mod asset_script;
mod attack;
mod combat_status;
mod encounter;
mod gear;
mod level_assets;
mod level_flow;
mod members;
mod mob_timeline;
mod quest_tracker;
mod slots;
mod stamina;
mod statics;

use asset_script::Step;
use combat_status::CombatTimer;
use encounter::{BAG_PREFIX, Order, Tutorial};
use gear::Gear;
use level_assets::{ASSET_ENTITY_BASE, AssetViews, asset_gde_ref};
use level_flow::{AchievementRules, LevelFlow};
use members::{
    CARD_REFRESH_BUNDLES, EntityKind, MobCard, OwnCard, OwnTeleport, bundles, card_of,
    log_mob_group, mob_group, own_group, remote_group,
};
use mob_timeline::{MobPose, MobTimelines, animations_of};
use quest_tracker::QuestTracker;
use slots::Slots;
use stamina::Stamina;
use statics::{STATIC_ENTITY_BASE, StaticViews, bag_emptied, bag_loot, interactor_cleared};

use attack::Vitals;

use crate::combat::{AttackOutcome, CombatSettings};
use crate::login::{
    SessionIds, WorldIds, level_change_messages, login_level_info, login_messages, phase_id,
};
use crate::mobs::{MobAttackIntent, MobChanges, Mobs};
use crate::objectives::{Effect, Event, ObjectiveRun};
use crate::player_content::{MobLooks, OPEN_WORLD_OBJECTIVE, PlayerContent, TUTORIAL_OBJECTIVE};
use nw_content::ftue_entities::{self, FACET_LEN, FtueLevel};
use nw_messages::raw_id_of_entity;

/// Carrier channel of the login messages and the time messages.
pub const MESSAGE_CHANNEL: u8 = 0;
/// Carrier channel of the replication bundles.
pub const REPLICATION_CHANNEL: u8 = 1;

/// First replication-bundle sequence of a fresh login. The real client drops a bundle whose
/// sequence is not greater than its `m_lastUnreliableReplicationSeq`, which is already 1 right
/// after `LevelInfoChanged`, so a login that started its sequence at 0 had its first bundles
/// rejected and never spawned. A fresh login starts above that mark; a handed-off player keeps
/// its own running sequence (see [`SimWorld::take`]).
const FIRST_LOGIN_BUNDLE_SEQUENCE: u64 = 2;

/// Wait after a fresh login before the one-time own spawn card goes out. The card is sent once
/// before the client spawns (see [`Neighbourhood`]); a card sent on the login tick reaches the
/// client before it has applied `LevelInfoChanged` (channel 0), so the client drops it as a
/// bundle "for a previous ClientContext", never loads the level, and never moves to spawn.
const OWN_CARD_DELAY_MS: u64 = 1_000;
/// Milliseconds per second, for speeds.
const MS_PER_SECOND_F32: f32 = 1_000.0;
/// How long a mob stands from the start of its alert before it moves (retail sailors: 2.0 s from
/// sailor 1's `Alert_Turn` at 73978 to its run at 76014; 2.8-3.1 s for sailors 2 and 3).
const ALERT_STAND_MS: u64 = 2_000;
/// How long a player who asked to leave the tutorial (502) stays: its last states (the quest's
/// end, the lore, the potions) go out before the handoff (retail: 3951 at 653944, the bundles to
/// 654011, the empty 6310 at 654271, 361 ms after the 502).
const LEAVE_DELAY_MS: u64 = 361;

/// How long a row of refused moves may run before the server moves the player back to the last
/// good position. A refused move leaves the client where it was — but it never sees that, because
/// it only applies the teleport-numbered own state (own-player-position note). Without this
/// move-back the client keeps walking on its own while the server keeps the stale position:
/// later moves miss the distance check too, and area tasks never see the player (live run of
/// 2026-09-14). The honest case — one unlucky refused move — resynchronises on the next input.
const REFUSED_RESYNC_MS: u64 = 1_000;

/// Debug bisection knob (env `NW_RELIABLE_BUNDLES`): send the replication bundles reliably on
/// channel 1 instead of unreliably. Off by default; used to test a client that ignores our
/// unreliable channel-1 frames. The name is outside the `NW_SIM_` prefix on purpose, because the
/// sim config parser rejects unknown `NW_SIM_*` env vars. Read once.
fn reliable_bundles() -> bool {
    use std::sync::OnceLock;
    static RELIABLE: OnceLock<bool> = OnceLock::new();
    *RELIABLE.get_or_init(|| std::env::var_os("NW_RELIABLE_BUNDLES").is_some())
}

/// Work-in-progress knob (env `NW_LOCAL_PLAYER_CARD`): send the client its own player as a spawn
/// card (player prefab, slot 1) with a connected type-3935 state and empty states of the other
/// player components (local-player-check note). Off by default until a live InGame check passes.
/// Overnight 2026-09-10: do not make this return true. Without the card the client loads the
/// level and waits at `WaitingForPlayerSpawn`. Read once.
pub(crate) fn local_player_card() -> bool {
    use std::sync::OnceLock;
    static CARD: OnceLock<bool> = OnceLock::new();
    *CARD.get_or_init(|| std::env::var_os("NW_LOCAL_PLAYER_CARD").is_some())
}

/// Debug bisection knob (env `NW_POSITIONS_ONLY`): put only the own player's group in the
/// bundles and omit every other entity, to test the local-player spawn with nothing else in the
/// bundles. Off by default. Read once.
fn positions_only() -> bool {
    use std::sync::OnceLock;
    static POSITIONS_ONLY: OnceLock<bool> = OnceLock::new();
    *POSITIONS_ONLY.get_or_init(|| std::env::var_os("NW_POSITIONS_ONLY").is_some())
}
/// Time between two `TimeSynchMsg` of one player (about 3 s in the captures).
pub const TIME_SYNCH_INTERVAL_MS: u64 = 3000;
/// Most members in one bundle. An unreliable carrier message must fit one datagram (it is never
/// chunked), and a member takes at most 18 bytes (a 5-byte entity number, a 1-byte class, a
/// 12-byte position), so 48 members leave room for the headers.
pub const MAX_BUNDLE_MEMBERS: usize = 48;

/// Settings of the world state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldSettings {
    /// Players see each other inside this radius, in metres.
    pub interest_radius_m: f32,
    /// Most other players and ghosts in a player's bundles: the nearest.
    pub max_visible_players: usize,
    /// Most players in this world.
    pub max_players: u32,
    /// The movement limits.
    pub limits: MoveLimits,
    /// The edges of the level.
    pub bounds: CellBounds,
    /// How far into another sim's cells a player goes before its handoff starts.
    pub handoff_margin_m: f32,
    /// Wait after a refused or late handoff, and after a take, before the next handoff.
    pub handoff_retry_ms: u64,
    /// Ghosts not renewed for this long are dropped.
    pub ghost_ttl_ms: u64,
    /// Threads of the bundle step.
    pub tick_threads: usize,
    /// Whether a client gets its own player as a spawn card ([`local_player_card`]).
    pub local_player_card: bool,
    /// The level this sim runs. The tutorial is a set of private instances: it hands players
    /// on only at its exit, into the open world (new-player-start note).
    pub level: LevelSpec,
    /// The height of the level's sea (`terrain.json` `oceanLevel`), in metres: a player near it
    /// swims.
    pub ocean_level_m: f32,
}

/// Fewest players for which the bundle step uses more than one thread; below that, starting
/// threads costs more than it saves.
pub const MIN_PLAYERS_PER_THREAD: usize = 64;

/// Why a move did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MoveError {
    /// The connection has no player in the world.
    #[error("no player on this connection")]
    NotInWorld,
    /// The player waits for the answer to its handoff; its moves are ignored.
    #[error("the player is frozen for a handoff")]
    Frozen,
    /// The movement checks refused the move; the player keeps its last good position.
    #[error("move refused: {}", .0.reason())]
    Refused(MoveRefusal),
    /// The player is dead.
    #[error("the player is dead")]
    Dead,
}

/// Messages for one connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    /// The connection.
    pub connection: ConnectionRef,
    /// The carrier channel.
    pub channel: u8,
    /// Whether the carrier sends them reliably.
    pub reliable: bool,
    /// The envelopes.
    pub envelopes: Vec<Vec<u8>>,
}

/// A player who left, for the last save.
#[derive(Debug, Clone, PartialEq)]
pub struct Departure {
    /// The connection the player had.
    pub connection: ConnectionRef,
    /// The character.
    pub character: Uuid,
    /// The last good position.
    pub position: Position,
    /// Server time of the last good move: a save of an older move never overwrites a newer one.
    pub moved_at_ms: u64,
}

/// A handoff that a tick started: the player is frozen until [`SimWorld::handoff_done`].
#[derive(Debug, Clone, PartialEq)]
pub struct HandoffStart {
    /// The sim that owns the player's cell.
    pub target: SimSlot,
    /// What the target needs.
    pub offer: HandoffOffer,
}

/// The end of a handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffEnd {
    /// The target took the player; it is gone from this world.
    Moved,
    /// The target refused or did not answer; the player stays and may move again.
    Kept,
    /// The player left during the handoff.
    Gone,
}

/// What one tick sends.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TickOutput {
    /// Hit and death messages, then bundles and time messages.
    pub messages: Vec<Outgoing>,
    /// Handoffs to ask for.
    pub handoffs: Vec<HandoffStart>,
    /// Ghost lists for other sims; an empty list clears the receiver's last one.
    pub ghosts: Vec<(BusId, Vec<Ghost>)>,
    /// What the mobs' hits on players did this tick.
    pub mob_hits: Vec<AttackOutcome>,
    /// Players who died since the last tick, to save at once (M6-A spec section 8).
    pub deaths: Vec<Departure>,
    /// Time of the mob part of the tick, in seconds.
    pub mob_seconds: f64,
    /// Members put in this tick's bundles: each player's own member and every other entity.
    pub bundle_members: u64,
}

/// The bundle messages of some players and the members they hold.
#[derive(Debug, Default)]
struct Bundled {
    outgoing: Vec<Outgoing>,
    members: u64,
}

/// A refused world change.
#[derive(Debug, Error)]
pub enum WorldError {
    /// The connection is already in the world.
    #[error("connection {0:?} is already in the world")]
    AlreadyJoined(ConnectionRef),
    /// The position is not a finite point.
    #[error("position is not finite")]
    NotFinite,
    /// The character is already in the world on another connection.
    #[error("character {0} is already in the world")]
    CharacterInWorld(Uuid),
    /// The world has `max_players` players.
    #[error("the sim is full")]
    Full,
    /// No random bytes for the session ids.
    #[error("no random bytes: {0}")]
    Random(#[from] getrandom::Error),
    /// A message cannot be encoded.
    #[error(transparent)]
    Message(#[from] MessageError),
}

/// A cell table that does not fit the level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TableError {
    /// Its grid is not the grid of this level, or its owner list has another length.
    #[error("the cell table does not fit the level")]
    WrongSize,
}

#[derive(Component)]
struct Player {
    connection: ConnectionRef,
    character: Uuid,
    name: String,
    /// The saved look, sent with the own spawn card.
    look: Option<PlayerLook>,
    /// The backstory of the archetype pick; it goes on with a handoff.
    backstory: Option<String>,
    /// The worn and carried items, sent with the own spawn card and after each change.
    gear: Option<Gear>,
    number: u64,
    bundles_sent: u64,
    last_time_synch_ms: u64,
    interest: Interest<u64>,
    /// The client's entity slots.
    slots: Slots,
    /// Whether the own spawn card went out from this sim.
    own_card_sent: bool,
    /// Earliest time the own spawn card may go out.
    own_card_due_ms: u64,
    /// The client-context id of the player's current level; every bundle carries it.
    client_context: u8,
    /// What the client and this server still say to each other in this level.
    flow: LevelFlow,
    /// When the player leaves the tutorial, once its client asked to (502).
    leave_at_ms: Option<u64>,
    /// Whether the server moved the player: its next own state moves it ([`OwnTeleport`]).
    moved: bool,
    /// The client's last camera point and when it came, for the player's speed.
    last_camera: Option<(Position, u64)>,
    /// Where the player faces after the server's move, when the move says so.
    move_facing: Option<f32>,
    /// The next state of a server move still to send.
    teleport_step: Option<TeleportStep>,
    /// Server time of the first refused move in the current row; `None` after a good move.
    /// A row of [`REFUSED_RESYNC_MS`] sets `moved`, so the client returns to the last good
    /// position it could not otherwise see.
    refused_since_ms: Option<u64>,
    /// Server time of the first own state, the start of the own clock.
    own_spawned_ms: Option<u64>,
    /// Moves of the player by the server so far.
    teleports: u8,
    /// The quest tracker of its tutorial quest.
    quest: Option<QuestTracker>,
    /// Its experience points, and whether they changed since the last bundle.
    experience: u32,
    experience_changed: bool,
    /// Its running consumable cooldowns, and whether they changed since the last bundle.
    cooldowns: Vec<ConsumableCooldown>,
    cooldowns_changed: bool,
    /// Its new health after a heal, for the next bundle.
    healed_to: Option<u32>,
    /// Its stamina.
    stamina: Stamina,
    /// Its combat status timer.
    combat: CombatTimer,
    /// The version of its replicated lists that this server sends besides the gear.
    list_version: u64,
    /// Own-group members to append to the next bundle (the take's cleared interactor target).
    extra_members: Vec<BundleMember>,
    /// The tutorial's static level entities it sees.
    statics: StaticViews,
    /// The tutorial's asset entities it sees.
    assets: AssetViews,
    frozen: bool,
    handoffs: u64,
    no_handoff_before_ms: u64,
}

/// What a client already has of the replication stream: it goes on across a handoff.
struct Replication {
    bundles_sent: u64,
    slots: Slots,
}

#[derive(Component, Clone, Copy)]
struct Body {
    motion: Motion,
}

/// The cells as this sim sees them.
struct Cells {
    owners: CellOwners<u16>,
    sims: Vec<SimSlot>,
    me: u16,
}

struct ReceivedGhosts {
    received_ms: u64,
    entities: Vec<ReceivedGhost>,
}

/// One entity of another sim.
#[derive(Clone, Copy)]
struct ReceivedGhost {
    entity: u64,
    position: Position,
    /// A mob's state; `None` for a player.
    mob: Option<MobGhost>,
}

/// The players of one sim.
pub struct SimWorld {
    world: World,
    by_connection: HashMap<ConnectionRef, Entity>,
    settings: WorldSettings,
    ids: WorldIds,
    ground: Arc<dyn Ground + Send + Sync>,
    me: SimSlot,
    cells: Option<Cells>,
    /// Tutorial sims only: the open-world sim that owns [`OPEN_WORLD_START`], where the exit
    /// hands players.
    exit_target: Option<SimSlot>,
    ghosts: HashMap<BusId, ReceivedGhosts>,
    ghost_targets: BTreeSet<BusId>,
    mobs: Mobs,
    mob_changes: MobChanges,
    combat: CombatSettings,
    /// The starter kits and achievement bits.
    content: Arc<PlayerContent>,
    /// The achievement bits players start with and the tutorial flow sets.
    achievement_rules: AchievementRules,
    /// Tutorial sims only: the tutorial of each player, by entity number.
    tutorials: HashMap<u64, Tutorial>,
    /// Open-world sims: the quest of each player who came from the tutorial, by entity number.
    quests: HashMap<u64, ObjectiveRun>,
    /// The state numbers, clock and animation of each shown mob.
    mob_timelines: MobTimelines,
    /// The health each shown mob had in the last bundle step.
    mob_health_shown: HashMap<u64, u32>,
    /// The boss phase of each boss mob that left phase 0.
    boss_phases: HashMap<u64, u32>,
    /// The boss phase each shown boss mob last sent.
    boss_phases_shown: HashMap<u64, u32>,
    /// The last player each shown mob picked as its target, by entity number.
    look_targets: HashMap<u64, u64>,
    dice: Dice,
    deaths: Vec<Departure>,
    /// Hit, death and facet messages since the last [`SimWorld::take_messages`].
    combat_messages: Vec<Outgoing>,
    /// Potions pressed: the player number and when the drink works.
    drinks: Vec<(u64, u64)>,
    /// Mob swings that have not hit yet, with the time they hit.
    swings: Vec<(u64, MobAttackIntent)>,
}

/// A new player before it is in the world.
struct Arrival {
    connection: ConnectionRef,
    character: Uuid,
    name: String,
    look: Option<PlayerLook>,
    backstory: Option<String>,
    number: u64,
    position: Position,
    health: Health,
    /// Whether the player is already in the world (spawn protection off). A fresh login is
    /// `false`; a handed-off player is `true`. See [`attack::Vitals`].
    spawned: bool,
    /// Earliest time the own spawn card may go out ([`OWN_CARD_DELAY_MS`]).
    own_card_due_ms: u64,
    /// The client-context id of the player's current level.
    client_context: u8,
    /// The flow of the level it arrives in.
    flow: LevelFlow,
    /// The items another sim sent; `None` gives the login kit.
    items: Option<Vec<CarriedItem>>,
    /// Its experience points.
    experience: u32,
}

impl SimWorld {
    /// An empty world of the sim `me` on the ground of `ground`, with the mobs of `combat`.
    pub fn new(
        settings: WorldSettings,
        ids: WorldIds,
        ground: Arc<dyn Ground + Send + Sync>,
        me: SimSlot,
        combat: CombatSettings,
        content: Arc<PlayerContent>,
    ) -> Self {
        Self {
            achievement_rules: AchievementRules::of(
                &content.achievements,
                &content.archetype_unlocks,
            ),
            tutorials: HashMap::new(),
            quests: HashMap::new(),
            mob_timelines: MobTimelines::default(),
            mob_health_shown: HashMap::new(),
            boss_phases: HashMap::new(),
            boss_phases_shown: HashMap::new(),
            look_targets: HashMap::new(),
            content,
            mobs: Mobs::new(Arc::clone(&combat.spawns), &combat),
            mob_changes: MobChanges::default(),
            dice: Dice::new(combat.seed),
            deaths: Vec::new(),
            combat_messages: Vec::new(),
            drinks: Vec::new(),
            swings: Vec::new(),
            combat,
            world: World::new(),
            by_connection: HashMap::new(),
            settings,
            ids,
            ground,
            me,
            cells: None,
            exit_target: None,
            ghosts: HashMap::new(),
            ghost_targets: BTreeSet::new(),
        }
    }

    /// Whether the player of `connection` is here.
    pub fn has(&self, connection: &ConnectionRef) -> bool {
        self.by_connection.contains_key(connection)
    }

    /// Players in the world.
    pub fn players(&self) -> usize {
        self.by_connection.len()
    }

    /// Adds the player of `admit` and returns its login messages, or `None` for a resumed
    /// player, which has them already. A resume of a player that this sim already has on that
    /// connection with that entity number (it took the player in a handoff that its lost sender
    /// never ended) keeps the player as it is.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError`] when the world is full, the connection or the character is already
    /// in the world, or the position is not finite.
    pub fn admit(
        &mut self,
        admit: &Admit,
        now_ms: u64,
        server_time: u64,
    ) -> Result<Option<Outgoing>, WorldError> {
        let here = self
            .by_connection
            .get(&admit.connection)
            .and_then(|entity| self.world.get::<Player>(*entity))
            .is_some_and(|player| player.number == admit.entity);
        if here && !admit.login {
            return Ok(None);
        }
        let level = self.settings.level;
        let level_info = if admit.login {
            Some(login_level_info(level, server_time, phase_id(level)?))
        } else {
            None
        };
        let flow = match &level_info {
            Some(level_info) => LevelFlow::after_level_info(
                admit.entity,
                level_info.clone(),
                now_ms,
                0,
                &self.achievement_rules,
                self.is_tutorial(),
                admit.backstory.as_deref(),
                &[],
            ),
            None => LevelFlow::resumed(admit.entity, 0),
        };
        let arrival = Arrival {
            connection: admit.connection.clone(),
            character: admit.character,
            name: admit.name.clone(),
            look: admit.look.map(PlayerLook::of_choices),
            backstory: admit.backstory.clone(),
            number: admit.entity,
            // A saved spot can be in the air or under the ground (a save during a jump, or a
            // height from another world file); the player arrives on the ground.
            position: self
                .settings
                .limits
                .onto_ground(&*self.ground, Position::from(admit.position)),
            health: Health::full(self.combat.data.player_max_health()),
            // A fresh login gets spawn protection until its client spawns; a resume was already
            // in the world.
            spawned: !admit.login,
            own_card_due_ms: if admit.login {
                now_ms.saturating_add(OWN_CARD_DELAY_MS)
            } else {
                now_ms
            },
            client_context: LOGIN_CLIENT_CONTEXT,
            flow,
            items: None,
            experience: 0,
        };
        self.check(&arrival)?;
        let login = if let Some(level_info) = &level_info {
            let envelopes = login_messages(
                &self.ids,
                SessionIds::random()?,
                admit.entity,
                server_time,
                level_info,
            )?;
            Some(Outgoing {
                connection: admit.connection.clone(),
                channel: MESSAGE_CHANNEL,
                reliable: true,
                envelopes,
            })
        } else {
            None
        };
        let motion = Motion::new(arrival.position, now_ms, &self.settings.limits);
        // A fresh login must start its bundle sequence above the client's mark; a resume keeps the
        // sequence it already carried.
        let first_sequence = if admit.login {
            admit.bundles_sent.max(FIRST_LOGIN_BUNDLE_SEQUENCE)
        } else {
            admit.bundles_sent
        };
        let replication = Replication {
            bundles_sent: first_sequence,
            slots: Slots::new(),
        };
        self.spawn(arrival, motion, replication, now_ms, admit.handoffs, now_ms);
        if admit.login && self.is_tutorial() {
            self.start_tutorial(admit.entity, now_ms);
        }
        Ok(login)
    }

    /// Takes the player of `offer` from another sim, with the same entity number, bundle
    /// sequence and movement budget. A player that enters this level from another one (the end
    /// of the tutorial) gets the level change messages, which this returns, the next
    /// client-context id, and a new spawn as at a login; it lands on the ground.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError`] as [`SimWorld::admit`] does.
    pub fn take(
        &mut self,
        offer: &HandoffOffer,
        now_ms: u64,
        server_time: u64,
    ) -> Result<Option<Outgoing>, WorldError> {
        let enters = offer.enter_level;
        let position = Position::from(offer.position);
        let client_context = if enters {
            offer.client_context.saturating_add(1)
        } else {
            offer.client_context
        };
        let level = self.settings.level;
        let level_info = if enters {
            Some(LevelInfoChangedMsg::of_level(
                level,
                client_context,
                server_time,
                phase_id(level)?,
            ))
        } else {
            None
        };
        let flow = match &level_info {
            Some(level_info) => LevelFlow::after_level_info(
                offer.entity,
                level_info.clone(),
                now_ms,
                offer.phasing_sequence,
                &self.achievement_rules,
                self.is_tutorial(),
                offer.backstory.as_deref(),
                &offer.achievements,
            ),
            None => LevelFlow::resumed(offer.entity, offer.phasing_sequence),
        };
        let arrival = Arrival {
            connection: offer.connection.clone(),
            character: offer.character,
            name: offer.name.clone(),
            look: offer.look.map(PlayerLook::of_choices),
            backstory: offer.backstory.clone(),
            number: offer.entity,
            position: if enters {
                self.settings.limits.onto_ground(&*self.ground, position)
            } else {
                position
            },
            health: Health::at(offer.health, self.combat.data.player_max_health()),
            // A handed-off player was already in the world; one that enters a new level spawns
            // again once its client has loaded it.
            spawned: !enters,
            own_card_due_ms: if enters {
                now_ms.saturating_add(OWN_CARD_DELAY_MS)
            } else {
                now_ms
            },
            client_context,
            flow,
            items: offer.items.clone(),
            experience: offer.experience,
        };
        self.check(&arrival)?;
        let level_change = if let Some(level_info) = &level_info {
            let envelopes = level_change_messages(offer.entity, level_info)?;
            Some(Outgoing {
                connection: offer.connection.clone(),
                channel: MESSAGE_CHANNEL,
                reliable: true,
                envelopes,
            })
        } else {
            None
        };
        let state = MotionState {
            position: arrival.position,
            move_age_ms: offer.move_age_ms,
            budget_m: offer.budget_m,
        };
        let motion = Motion::import(state, now_ms, &self.settings.limits);
        let retry_at = now_ms.saturating_add(self.settings.handoff_retry_ms);
        // A new level starts with no entity slots on the client.
        let slots = if enters {
            Slots::new()
        } else {
            Slots::restored(
                offer.slots.iter().copied().collect(),
                offer.pending_stops.clone(),
            )
        };
        let replication = Replication {
            bundles_sent: offer.bundles_sent,
            slots,
        };
        self.spawn(
            arrival,
            motion,
            replication,
            now_ms.saturating_sub(offer.time_synch_age_ms),
            offer.handoffs,
            retry_at,
        );
        if enters && !self.is_tutorial() {
            self.start_open_quest(offer.entity, now_ms);
        } else if let Some(quest) = &offer.quest {
            self.resume_open_quest(offer.entity, quest, now_ms);
        }
        Ok(level_change)
    }

    /// Notes that the client of `connection` sent a message. A client sends nothing between the
    /// login and its spawn: its first message after the own spawn card comes in the same moment it
    /// logs `player spawn succeeds, in game` (live 2026-09-10). So that message ends the hold on
    /// the replication stream and the spawn protection. Before the card it means nothing, and a
    /// player who already spawned is not changed.
    pub fn heard_from(&mut self, connection: &ConnectionRef) {
        let Some(&entity) = self.by_connection.get(connection) else {
            return;
        };
        if !self
            .world
            .get::<Player>(entity)
            .is_some_and(|player| player.own_card_sent)
        {
            return;
        }
        if let Some(mut vitals) = self.world.get_mut::<Vitals>(entity) {
            vitals.spawned = true;
        }
    }

    /// Moves the player of `connection` to `next`, received at `now_ms`, when the movement checks
    /// (M4 spec section 7.2 with sections 15 and 16) allow it.
    ///
    /// # Errors
    ///
    /// Returns [`MoveError`] when there is no such player, it is frozen, or a check refuses the
    /// move; then the player keeps its last good position. When moves stay refused for
    /// [`REFUSED_RESYNC_MS`] the player is moved back there.
    pub fn move_to(
        &mut self,
        connection: &ConnectionRef,
        next: Position,
        now_ms: u64,
    ) -> Result<GroundCheck, MoveError> {
        let entity = *self
            .by_connection
            .get(connection)
            .ok_or(MoveError::NotInWorld)?;
        if self
            .world
            .get::<Player>(entity)
            .is_some_and(|player| player.frozen)
        {
            return Err(MoveError::Frozen);
        }
        // The client sends movement only once it has spawned into the world, so the first move
        // ends the player's spawn protection (see [`attack::Vitals`]).
        if let Some(mut vitals) = self.world.get_mut::<Vitals>(entity) {
            vitals.spawned = true;
        }
        if self
            .world
            .get::<Vitals>(entity)
            .is_some_and(|vitals| vitals.health.is_dead())
        {
            return Err(MoveError::Dead);
        }
        let (result, last_good) = {
            let mut body = self
                .world
                .get_mut::<Body>(entity)
                .ok_or(MoveError::NotInWorld)?;
            (
                body.motion.try_move(
                    &self.settings.limits,
                    &*self.ground,
                    &self.settings.bounds,
                    next,
                    now_ms,
                ),
                body.motion.position(),
            )
        };
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return result.map_err(MoveError::Refused);
        };
        match result {
            Err(refusal) => {
                let since = player.refused_since_ms.get_or_insert(now_ms);
                if now_ms.saturating_sub(*since) >= REFUSED_RESYNC_MS {
                    player.moved = true;
                    player.move_facing = None;
                    player.refused_since_ms = None;
                    tracing::debug!(
                        player = player.number,
                        x = last_good.x,
                        y = last_good.y,
                        z = last_good.z,
                        "moves refused too long: player moved back"
                    );
                }
                Err(MoveError::Refused(refusal))
            }
            Ok(check) => {
                player.refused_since_ms = None;
                Ok(check)
            }
        }
    }

    /// Moves the player of `connection` to the body under its client camera at `camera`,
    /// looking along `yaw` (radians) when the input says ([`crate::camera::body_of`]; without a
    /// yaw the camera point is taken as the body), received at `now_ms`. Gives the result and the
    /// body position it checked.
    pub fn move_from_camera(
        &mut self,
        connection: &ConnectionRef,
        camera: Position,
        yaw: Option<f32>,
        now_ms: u64,
    ) -> (Result<GroundCheck, MoveError>, Position) {
        let last = self
            .by_connection
            .get(connection)
            .copied()
            .and_then(|entity| self.world.get_mut::<Player>(entity))
            .and_then(|mut player| player.last_camera.replace((camera, now_ms)));
        let next = yaw.map_or(camera, |yaw| {
            let speed_mps = last.map_or(0.0, |(at, then)| {
                let elapsed_ms = now_ms.saturating_sub(then);
                if elapsed_ms == 0 {
                    0.0
                } else {
                    #[allow(clippy::cast_precision_loss, reason = "a gap of a few seconds")]
                    let seconds = elapsed_ms as f32 / MS_PER_SECOND_F32;
                    at.horizontal_distance(camera) / seconds
                }
            });
            crate::camera::body_of(camera, yaw, speed_mps)
        });
        (self.move_to(connection, next, now_ms), next)
    }

    /// Removes a player.
    pub fn leave(&mut self, connection: &ConnectionRef) -> Option<Departure> {
        let entity = *self.by_connection.get(connection)?;
        let departure = self.departure(entity);
        self.remove_player(connection, entity);
        departure
    }

    /// Takes the player `entity` of `connection` out of this world, with its tutorial.
    fn remove_player(&mut self, connection: &ConnectionRef, entity: Entity) {
        self.by_connection.remove(connection);
        if let Some(number) = self.world.get::<Player>(entity).map(|player| player.number) {
            self.quests.remove(&number);
            if let Some(tutorial) = self.tutorials.remove(&number) {
                tutorial.end(&mut self.mobs);
            }
        }
        self.world.despawn(entity);
    }

    /// Removes every player of a gateway.
    pub fn leave_gateway(&mut self, gateway: &BusId) -> Vec<Departure> {
        let connections: Vec<ConnectionRef> = self
            .by_connection
            .keys()
            .filter(|connection| connection.gateway == *gateway)
            .cloned()
            .collect();
        self.leave_all(&connections)
    }

    /// Removes a player that this sim took by the handoff `handoffs` that the sender then
    /// cancelled. The sender kept the player, so nothing is saved here. A player of another
    /// attempt stays. Returns whether it was removed.
    pub fn cancel(&mut self, connection: &ConnectionRef, handoffs: u64) -> bool {
        let Some(&entity) = self.by_connection.get(connection) else {
            return false;
        };
        let same_attempt = self
            .world
            .get::<Player>(entity)
            .is_some_and(|player| player.handoffs == handoffs);
        if same_attempt {
            self.remove_player(connection, entity);
        }
        same_attempt
    }

    /// The answer to the handoff of the player of `connection` at `now_ms`.
    pub fn handoff_done(
        &mut self,
        connection: &ConnectionRef,
        accepted: bool,
        now_ms: u64,
    ) -> HandoffEnd {
        let Some(&entity) = self.by_connection.get(connection) else {
            return HandoffEnd::Gone;
        };
        if accepted {
            self.remove_player(connection, entity);
            return HandoffEnd::Moved;
        }
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.frozen = false;
            player.no_handoff_before_ms = now_ms.saturating_add(self.settings.handoff_retry_ms);
        }
        HandoffEnd::Kept
    }

    /// Takes a new cell table. When it does not name this sim with its generation, the sim owns
    /// no cell and every player leaves (M5 spec section 3.6); their departures are returned.
    ///
    /// # Errors
    ///
    /// Returns [`TableError::WrongSize`] for a table that does not fit the level; the last table
    /// stays.
    pub fn set_table(&mut self, table: &CellTable) -> Result<Vec<Departure>, TableError> {
        if self.is_tutorial() {
            // A tutorial sim owns its whole private level and is in no open-world table; from the
            // table it needs only the sim its exit hands players to.
            self.mob_changes.add(self.mobs.set_cells(|_| true));
            self.exit_target = owner_in(table, OPEN_WORLD_START);
            return Ok(Vec::new());
        }
        let bounds = self.settings.bounds;
        let grid = CellGrid::new(bounds.max_x, bounds.max_y, table.cell_size_m)
            .ok()
            .filter(|grid| grid.columns() == table.columns && grid.rows() == table.rows)
            .ok_or(TableError::WrongSize)?;
        let owners =
            CellOwners::from_cells(grid, table.owners.clone()).ok_or(TableError::WrongSize)?;
        let me = table
            .sims
            .iter()
            .position(|slot| *slot == self.me)
            .and_then(|index| u16::try_from(index).ok());
        let Some(me) = me else {
            self.cells = None;
            self.mob_changes.add(self.mobs.set_cells(|_| false));
            let everyone: Vec<ConnectionRef> = self.by_connection.keys().cloned().collect();
            return Ok(self.leave_all(&everyone));
        };
        self.mob_changes.add(
            self.mobs
                .set_cells(|position| owners.owner_at(position) == Some(me)),
        );
        self.cells = Some(Cells {
            owners,
            sims: table.sims.clone(),
            me,
        });
        Ok(Vec::new())
    }

    /// The ghosts of another sim, received at `now_ms`; positions that are not finite are left
    /// out.
    pub fn set_ghosts(&mut self, from: &BusId, entities: Vec<Ghost>, now_ms: u64) {
        if *from == self.me.id {
            return;
        }
        let entities = entities
            .into_iter()
            .filter(|ghost| ghost.position.is_finite())
            .map(|ghost| ReceivedGhost {
                entity: ghost.entity,
                position: Position::from(ghost.position),
                mob: ghost.mob,
            })
            .collect();
        self.ghosts.insert(
            from.clone(),
            ReceivedGhosts {
                received_ms: now_ms,
                entities,
            },
        );
    }

    /// The ghost `entity` and the sim it belongs to.
    fn ghost(&self, entity: u64) -> Option<(&BusId, ReceivedGhost)> {
        self.ghosts.iter().find_map(|(owner, ghosts)| {
            ghosts
                .entities
                .iter()
                .find(|ghost| ghost.entity == entity)
                .map(|ghost| (owner, *ghost))
        })
    }

    /// The admin snapshot: every player of this sim, every awake mob, and mobs by state (admin
    /// tools spec section 5).
    pub fn snapshot(&mut self) -> (Vec<SnapPlayer>, Vec<SnapMob>, [u32; MOB_STATES]) {
        let mut query = self.world.query::<(&Player, &Body, &Vitals)>();
        let players = query
            .iter(&self.world)
            .map(|(player, body, vitals)| {
                let at = body.motion.position();
                SnapPlayer {
                    entity: player.number,
                    name: player.name.clone(),
                    x: at.x,
                    y: at.y,
                    z: at.z,
                    dead: vitals.health.is_dead(),
                    frozen: player.frozen,
                }
            })
            .collect();
        let (mobs, counts) = self.mobs.snapshot();
        (players, mobs, counts)
    }

    /// Every awake mob of this sim and every mob ghost: entity number, position and state. An
    /// asleep mob has no player within its wake radius, so it is in no player's view or reach.
    fn mob_states(&self) -> impl Iterator<Item = (u64, Position, MobGhost)> + '_ {
        self.ghosts
            .values()
            .flat_map(|ghosts| &ghosts.entities)
            .filter_map(|ghost| ghost.mob.map(|state| (ghost.entity, ghost.position, state)))
            .chain(self.mobs.awake_states())
    }

    /// What the cell tables did to the mobs since the last call.
    pub fn take_mob_changes(&mut self) -> MobChanges {
        std::mem::take(&mut self.mob_changes)
    }

    /// Brain steps and player wake scans of the mobs since the start.
    pub const fn mob_work(&self) -> (u64, u64) {
        (self.mobs.brain_runs(), self.mobs.wake_scans())
    }

    /// Mobs by state.
    pub fn mob_counts(&self) -> [(MobState, usize); MobState::ALL.len()] {
        self.mobs.counts()
    }

    /// Ghosts of other sims.
    pub fn ghosts(&self) -> usize {
        self.ghosts
            .values()
            .map(|ghosts| ghosts.entities.len())
            .sum()
    }

    /// Every player's character and position, for the periodic save.
    pub fn positions(&mut self) -> Vec<Departure> {
        let mut query = self.world.query::<(&Player, &Body)>();
        query
            .iter(&self.world)
            .map(|(player, body)| departure_of(player, body))
            .collect()
    }

    /// One tick: handoffs to start, ghost lists for other sims, a bundle for every player that
    /// is not frozen, and a `TimeSynchMsg` when one is due.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a message cannot be encoded.
    pub fn tick(&mut self, now_ms: u64, server_time: u64) -> Result<TickOutput, MessageError> {
        let ttl = self.settings.ghost_ttl_ms;
        self.ghosts
            .retain(|_, ghosts| now_ms.saturating_sub(ghosts.received_ms) <= ttl);
        let handoffs = self.start_handoffs(now_ms);
        let ghosts = self.ghosts_out();
        let mobs_started = Instant::now();
        let swings = self.tick_mobs(now_ms);
        let intents = self.swings_landing(swings, now_ms);
        let mob_hits = self.mob_hits(intents, now_ms);
        let mob_seconds = mobs_started.elapsed().as_secs_f64();
        self.tick_tutorials(now_ms);
        self.tick_quests(now_ms);
        self.drink_due(now_ms, server_time);
        self.tick_stamina(now_ms);
        let mut messages = self.take_messages();
        let bundled = self.bundles(now_ms, server_time)?;
        messages.extend(bundled.outgoing);
        Ok(TickOutput {
            messages,
            handoffs,
            ghosts,
            mob_hits,
            deaths: std::mem::take(&mut self.deaths),
            mob_seconds,
            bundle_members: bundled.members,
        })
    }

    /// Runs the mobs with the living players of this sim and the player ghosts of others.
    fn tick_mobs(&mut self, now_ms: u64) -> Vec<MobAttackIntent> {
        let mut seen: Vec<Seen> = Vec::new();
        let mut query = self.world.query::<(&Player, &Body, &Vitals)>();
        for (player, body, vitals) in query.iter(&self.world) {
            if vitals.health.is_dead() {
                continue;
            }
            seen.push(Seen {
                entity: player.number,
                position: body.motion.position(),
            });
        }
        let ghosts = self.ghosts.values().flat_map(|ghosts| &ghosts.entities);
        for ghost in ghosts.filter(|ghost| ghost.mob.is_none()) {
            seen.push(Seen {
                entity: ghost.entity,
                position: ghost.position,
            });
        }
        self.mobs.tick(&seen, now_ms, &*self.ground)
    }

    /// Starts the tutorial of player `number` at `now_ms`; without the quest in the content
    /// there is none, and the tutorial ends at its exit.
    fn start_tutorial(&mut self, number: u64, now_ms: u64) {
        let Some(quest) = self.content.tutorial_quest.clone() else {
            return;
        };
        let (mut tutorial, orders) = Tutorial::start(
            number,
            quest,
            &self.content.tutorial_level,
            &mut self.mobs,
            now_ms,
        );
        // The expansion is in the player's first achievement state.
        if let Some(bit) = self.achievement_rules.expansion() {
            tutorial.achievement(bit, now_ms);
        }
        if let Some(old) = self.tutorials.insert(number, tutorial) {
            old.end(&mut self.mobs);
        }
        self.carry_out(number, orders, now_ms);
    }

    /// Runs the tutorials with their players' positions.
    fn tick_tutorials(&mut self, now_ms: u64) {
        let numbers: Vec<u64> = self.tutorials.keys().copied().collect();
        for number in numbers {
            let position = self
                .player_by_number(number)
                .and_then(|entity| self.world.get::<Body>(entity))
                .map(|body| body.motion.position());
            let (Some(position), Some(tutorial)) = (position, self.tutorials.get_mut(&number))
            else {
                continue;
            };
            let mut orders =
                tutorial.tick(&mut self.mobs, &self.content.kill_types, position, now_ms);
            // Items the player already holds meet the item tasks that start later.
            let carried = self
                .player_by_number(number)
                .and_then(|entity| self.world.get::<Player>(entity))
                .and_then(|player| player.gear.as_ref().map(Gear::carried))
                .unwrap_or_default();
            let held: Vec<(&str, bool)> = carried
                .iter()
                .map(|item| {
                    (
                        item.item_id.as_str(),
                        matches!(item.place, ItemPlace::Worn(_)),
                    )
                })
                .collect();
            if let Some(tutorial) = self.tutorials.get_mut(&number) {
                orders.extend(tutorial.sync_held(&held, &mut self.mobs, now_ms));
            }
            self.carry_out(number, orders, now_ms);
            self.update_quest_tracker(number);
        }
    }

    /// Gives the quest tracker of player `number` its tutorial's or its quest's task list.
    fn update_quest_tracker(&mut self, number: u64) {
        let Some(entity) = self.player_by_number(number) else {
            return;
        };
        let (tasks, finished) = if let Some(tutorial) = self.tutorials.get(&number) {
            (tutorial.progress(), tutorial.finished())
        } else if let Some(quest) = self.quests.get(&number) {
            (quest.progress(), quest.finished())
        } else {
            return;
        };
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player
                .quest
                .get_or_insert_with(|| QuestTracker::new(TUTORIAL_OBJECTIVE, number))
                .update(tasks, finished);
        }
    }

    /// Starts the first open-world quest of player `number`, who just left the tutorial
    /// (retail: `12_GetOffBeach` is in its first open-world state, with the tutorial completed).
    fn start_open_quest(&mut self, number: u64, now_ms: u64) {
        let Some(tree) = self.content.open_world_quest.clone() else {
            return;
        };
        let Some(entity) = self.player_by_number(number) else {
            return;
        };
        let (run, effects) = ObjectiveRun::start(tree, now_ms);
        self.quests.insert(number, run);
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.quest = Some(
                QuestTracker::new(OPEN_WORLD_OBJECTIVE, number)
                    .after(&[TUTORIAL_OBJECTIVE], number),
            );
        }
        self.quest_effects(number, effects, now_ms);
        self.update_quest_tracker(number);
    }

    /// Goes on with the open-world quest run `run` of player `number`, who came from another sim
    /// of this level: its tracker's lists go on above the versions the client has.
    fn resume_open_quest(&mut self, number: u64, run: &QuestRun, now_ms: u64) {
        let Some(tree) = self.content.open_world_quest.clone() else {
            return;
        };
        let Some(quest) = ObjectiveRun::resume(tree, run, now_ms) else {
            tracing::warn!(
                player = number,
                "handed-off quest does not fit the quest tree"
            );
            return;
        };
        let Some(entity) = self.player_by_number(number) else {
            return;
        };
        let tasks = quest.progress();
        // A finished quest's end states went out from the other sim; its tracker stays empty.
        let finished = quest.finished();
        self.quests.insert(number, quest);
        if finished {
            return;
        }
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            let mut tracker = QuestTracker::new(OPEN_WORLD_OBJECTIVE, number)
                .after(&[TUTORIAL_OBJECTIVE], number)
                .above(run.tracker_version);
            tracker.update(tasks, false);
            // The first own card on this sim sends the whole quest; nothing more is due.
            while tracker.take_change().is_some() {}
            player.quest = Some(tracker);
        }
    }

    /// Feeds game event `event` to the open-world quest of player `number`.
    fn quest_event(&mut self, number: u64, event: Event<'_>, now_ms: u64) {
        let Some(quest) = self.quests.get_mut(&number) else {
            return;
        };
        let effects = quest.event(event, now_ms);
        self.quest_effects(number, effects, now_ms);
    }

    /// Carries out the `effects` of player `number`'s open-world quest: achievements are set or
    /// cleared (a set one is an event of the quest again), items removed.
    fn quest_effects(&mut self, number: u64, effects: Vec<Effect>, now_ms: u64) {
        let Some(entity) = self.player_by_number(number) else {
            return;
        };
        let mut pending = effects;
        while !pending.is_empty() {
            let mut set = Vec::new();
            if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                for effect in pending.drain(..) {
                    match effect {
                        Effect::Achievement(id) => {
                            player.flow.set_achievement(&self.achievement_rules, &id);
                            set.push(id);
                        }
                        Effect::LockAchievement(id) => {
                            player.flow.clear_achievement(&self.achievement_rules, &id);
                        }
                        Effect::RemoveItem(id) => {
                            if let Some(gear) = player.gear.as_mut() {
                                gear.remove(&id);
                            }
                        }
                        Effect::Done(_) => {}
                    }
                }
            }
            let Some(quest) = self.quests.get_mut(&number) else {
                return;
            };
            for id in set {
                pending.extend(quest.event(Event::AchievementSet(&id), now_ms));
            }
        }
    }

    /// Runs the open-world quests: their timers and the players' places.
    fn tick_quests(&mut self, now_ms: u64) {
        let numbers: Vec<u64> = self.quests.keys().copied().collect();
        for number in numbers {
            let position = self
                .player_by_number(number)
                .and_then(|entity| self.world.get::<Body>(entity))
                .map(|body| body.motion.position());
            let (Some(position), Some(quest)) = (position, self.quests.get_mut(&number)) else {
                continue;
            };
            let mut effects = quest.tick(now_ms);
            effects.extend(quest.event(Event::Moved(position), now_ms));
            self.quest_effects(number, effects, now_ms);
            self.update_quest_tracker(number);
        }
    }

    /// Carries out the tutorial `orders` for player `number` at `now_ms`; an item it gets goes
    /// back to its tutorial as an event, whose orders are carried out too.
    fn carry_out(&mut self, number: u64, orders: Vec<Order>, now_ms: u64) {
        let Some(entity) = self.player_by_number(number) else {
            return;
        };
        let mut pending = orders;
        while !pending.is_empty() {
            let mut gained = Vec::new();
            let mut set_bits = Vec::new();
            for order in pending.drain(..) {
                if let Order::MoveTo(position, facing) = order {
                    if let Some(mut body) = self.world.get_mut::<Body>(entity) {
                        body.motion = Motion::new(position, now_ms, &self.settings.limits);
                    }
                    if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                        player.moved = true;
                        player.move_facing = Some(facing);
                    }
                    continue;
                }
                let Some(mut player) = self.world.get_mut::<Player>(entity) else {
                    return;
                };
                match order {
                    Order::Achievement(id) => {
                        player.flow.set_achievement(&self.achievement_rules, &id);
                        set_bits.extend(self.achievement_rules.bit(&id));
                    }
                    Order::Asset(asset, step) => match step {
                        Step::Show => player.assets.show(asset, asset_gde_ref(number, asset)),
                        Step::Change(change) => player.assets.change(asset, change),
                        Step::Hide => player.assets.hide(asset),
                    },
                    Order::RemoveItem(id) => {
                        if let Some(gear) = player.gear.as_mut() {
                            gear.remove(&id);
                        }
                    }
                    Order::RefillPotions(id, quantity) => {
                        let player = &mut *player;
                        if let (Some(item), Some(gear)) =
                            (self.content.kits.item(&id), player.gear.as_mut())
                        {
                            gear.renew(item, quantity, QUICK_SLOT_1);
                        }
                        player.cooldowns.clear();
                        player.cooldowns_changed = true;
                    }
                    Order::GiveItem(id, quantity) => {
                        let item = self.content.kits.item(&id);
                        let given = item
                            .zip(player.gear.as_mut())
                            .is_some_and(|(item, gear)| gear.add(item, quantity));
                        if given {
                            gained.push(id);
                        } else {
                            tracing::warn!(player = number, item = %id, "tutorial item not given");
                        }
                    }
                    Order::ShowIsland => {
                        let statics = &self.content.tutorial_level.statics;
                        player.statics.show_island(statics);
                        // Retail sends the bag's container contents ~300 ms after its card;
                        // the client refuses a take-all on an empty container.
                        let items = player
                            .backstory
                            .as_deref()
                            .map(|backstory| self.content.kits.bag(backstory))
                            .unwrap_or_default();
                        let loot = bag_loot(player.character, items);
                        player.statics.queue(statics, &BAG_PREFIX, loot);
                    }
                    Order::StaticChange(prefix, change) => {
                        player.statics.change(
                            &self.content.tutorial_level.statics,
                            &prefix,
                            change,
                        );
                    }
                    Order::HideStatic(prefix) => {
                        player
                            .statics
                            .hide(&self.content.tutorial_level.statics, &prefix);
                    }
                    Order::AcceptGather(request) => {
                        let accept = gather_accept(number, request);
                        send_now(&mut self.combat_messages, &player.connection, &[accept]);
                    }
                    Order::EndGather(request) => {
                        let end = gather_end(number, request);
                        send_now(&mut self.combat_messages, &player.connection, &end);
                    }
                    Order::Targeted { mob, tutorial_ai } => {
                        let messages = targeted(number, mob, tutorial_ai);
                        send_now(&mut self.combat_messages, &player.connection, &messages);
                    }
                    Order::BossPhase { mob, phase } => {
                        self.boss_phases.insert(mob, phase);
                    }
                    Order::Died { mob, collision } => {
                        let messages = died(number, mob, collision);
                        send_now(&mut self.combat_messages, &player.connection, &messages);
                    }
                    Order::LockAchievement(id) => {
                        player.flow.clear_achievement(&self.achievement_rules, &id);
                    }
                    Order::Experience(points) => {
                        player.experience = player.experience.saturating_add(points);
                        player.experience_changed = true;
                    }
                    Order::MoveTo(..) => {}
                }
            }
            let Some(tutorial) = self.tutorials.get_mut(&number) else {
                break;
            };
            for bit in set_bits {
                tutorial.achievement(bit, now_ms);
            }
            for id in &gained {
                pending.extend(tutorial.event(Event::ItemGained(id), &mut self.mobs, now_ms));
            }
        }
    }

    /// Whether this sim runs the tutorial.
    fn is_tutorial(&self) -> bool {
        self.settings.level == TUTORIAL_LEVEL
    }

    /// The client of `connection` starts interaction `request` at `now_ms` (the use of a static
    /// entity, from its input): its tutorial answers a use of the chest.
    pub fn interact(
        &mut self,
        connection: &ConnectionRef,
        request: InteractionRequest,
        now_ms: u64,
    ) {
        let Some(number) = self
            .by_connection
            .get(connection)
            .and_then(|entity| self.world.get::<Player>(*entity))
            .map(|player| player.number)
        else {
            return;
        };
        let Some(tutorial) = self.tutorials.get_mut(&number) else {
            return;
        };
        let orders = tutorial.interact(request, now_ms);
        self.carry_out(number, orders, now_ms);
    }

    /// Answers a facet call of the client of `connection` at `now_ms` (tutorial-intro note,
    /// section 4), and gives it to the player's tutorial: videos, cutscenes, the quick-time
    /// lesson, UI screens, the beach bag and equips (live-tutorial note). In the tutorial, a
    /// request to leave (502) shows the loading screen and hands the player to the open world at
    /// the next tick, as the exit does.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when an answer cannot be encoded.
    pub fn facet_message(
        &mut self,
        connection: &ConnectionRef,
        message: ClientFacetMsg,
        now_ms: u64,
    ) -> Result<(), MessageError> {
        let Some(&entity) = self.by_connection.get(connection) else {
            return Ok(());
        };
        let tutorial = self.is_tutorial();
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return Ok(());
        };
        let number = player.number;
        let ended_cutscene = player.flow.cutscene();
        let equipped = match message {
            ClientFacetMsg::EquipItem {
                bag_slot,
                equip_slot,
                quantity,
                instance,
                ..
            } => player.gear.as_mut().and_then(|gear| {
                gear.equip(bag_slot, equip_slot, quantity, instance)
                    .inspect_err(|refusal| {
                        tracing::debug!(?refusal, bag_slot, equip_slot, "equip refused");
                    })
                    .ok()
            }),
            _ => None,
        };
        let backstory = player.backstory.clone();
        let answers = match message {
            ClientFacetMsg::ReturnToMainWorld { .. }
                if tutorial && player.leave_at_ms.is_none() =>
            {
                player.leave_at_ms = Some(now_ms.saturating_add(LEAVE_DELAY_MS));
                player.flow.leave()
            }
            _ => {
                let client_context = player.client_context;
                player.flow.answer(message, client_context)
            }
        };
        send_now(&mut self.combat_messages, connection, &answers);
        if !self.tutorials.contains_key(&number) {
            let event = match message {
                ClientFacetMsg::CinematicStarted { cinematic, .. } => {
                    Some(Event::CinematicStarted(cinematic))
                }
                ClientFacetMsg::CinematicEnded { cinematic, .. } => {
                    Some(Event::CinematicEnded(cinematic))
                }
                ClientFacetMsg::CutsceneEnded { .. } => ended_cutscene.map(Event::CutsceneEnded),
                ClientFacetMsg::UiScreenOpened { screen, .. } => {
                    Some(Event::UiScreenOpened(screen))
                }
                _ => None,
            };
            if let Some(event) = event {
                self.quest_event(number, event, now_ms);
            }
            return Ok(());
        }
        let Some(run) = self.tutorials.get_mut(&number) else {
            return Ok(());
        };
        run.client_message(&message, now_ms);
        let mobs = &mut self.mobs;
        let orders = match message {
            ClientFacetMsg::QteCompleted { .. } => run.lesson_done(mobs, now_ms),
            ClientFacetMsg::CinematicStarted { cinematic, .. } => {
                run.event(Event::CinematicStarted(cinematic), mobs, now_ms)
            }
            ClientFacetMsg::CinematicEnded { cinematic, .. } => {
                run.event(Event::CinematicEnded(cinematic), mobs, now_ms)
            }
            ClientFacetMsg::CutsceneEnded { .. } => ended_cutscene
                .map(|cutscene| run.event(Event::CutsceneEnded(cutscene), mobs, now_ms))
                .unwrap_or_default(),
            ClientFacetMsg::UiScreenOpened { screen, .. } => {
                run.event(Event::UiScreenOpened(screen), mobs, now_ms)
            }
            ClientFacetMsg::EquipItem { .. } => equipped
                .map(|item| run.event(Event::Equipped(&item), mobs, now_ms))
                .unwrap_or_default(),
            ClientFacetMsg::TakeAllItems { container, .. } if run.take_bag(&container, now_ms) => {
                let items = backstory
                    .as_deref()
                    .map(|backstory| self.content.kits.bag(backstory))
                    .unwrap_or_default();
                player.statics.queue(
                    &self.content.tutorial_level.statics,
                    &BAG_PREFIX,
                    bag_emptied(items.len()),
                );
                player.extra_members.push(interactor_cleared(&BAG_PREFIX));
                let mut orders: Vec<Order> = items
                    .iter()
                    .map(|item| Order::GiveItem(item.item_id.clone(), 1))
                    .collect();
                orders.extend(run.event(Event::ContainerLooted, mobs, now_ms));
                orders
            }
            _ => Vec::new(),
        };
        self.carry_out(number, orders, now_ms);
        Ok(())
    }

    fn start_handoffs(&mut self, now_ms: u64) -> Vec<HandoffStart> {
        if self.is_tutorial() {
            return self.tutorial_exits(now_ms);
        }
        let Some(cells) = &self.cells else {
            return Vec::new();
        };
        let margin = self.settings.handoff_margin_m;
        let quests = &self.quests;
        let mut query = self.world.query::<(&mut Player, &Body, &Vitals)>();
        let mut starts = Vec::new();
        for (mut player, body, vitals) in query.iter_mut(&mut self.world) {
            // A dead player stays until its respawn: the next sim could not revive it.
            if player.frozen || now_ms < player.no_handoff_before_ms || vitals.health.is_dead() {
                continue;
            }
            let position = body.motion.position();
            let target = cells
                .owners
                .handoff_target(cells.me, position, margin)
                .and_then(|owner| cells.sims.get(usize::from(owner)));
            let Some(target) = target else {
                continue;
            };
            player.frozen = true;
            let tracker_version = player.quest.as_ref().map_or(0, QuestTracker::version);
            let quest = quests
                .get(&player.number)
                .map(|run| run.export(now_ms, tracker_version));
            starts.push(HandoffStart {
                target: target.clone(),
                offer: handoff_offer(&player, body, vitals, now_ms, None, quest),
            });
        }
        starts
    }

    /// The tutorial players at the exit or whose client asked to leave, handed to the open-world
    /// sim that owns the beach start with the level change (new-player-start note). Only a player
    /// whose client has spawned leaves; without a known open-world sim everyone stays.
    fn tutorial_exits(&mut self, now_ms: u64) -> Vec<HandoffStart> {
        let Some(target) = self.exit_target.clone() else {
            return Vec::new();
        };
        let tutorials = &self.tutorials;
        let mut query = self.world.query::<(&mut Player, &Body, &Vitals)>();
        let mut starts = Vec::new();
        for (mut player, body, vitals) in query.iter_mut(&mut self.world) {
            // With the quest running, the client asks to leave after the last video; without
            // it, the exit area ends the tutorial.
            let at_exit =
                !tutorials.contains_key(&player.number) && at_tutorial_exit(body.motion.position());
            let leaves = player.leave_at_ms.is_some_and(|at| now_ms >= at);
            if player.frozen
                || now_ms < player.no_handoff_before_ms
                || !vitals.spawned
                || vitals.health.is_dead()
                || !(leaves || at_exit)
            {
                continue;
            }
            player.frozen = true;
            if leaves {
                let left = player.flow.left();
                send_now(&mut self.combat_messages, &player.connection, &[left]);
            }
            starts.push(HandoffStart {
                target: target.clone(),
                offer: handoff_offer(&player, body, vitals, now_ms, Some(OPEN_WORLD_START), None),
            });
        }
        starts
    }

    fn ghosts_out(&mut self) -> Vec<(BusId, Vec<Ghost>)> {
        let mut lists: BTreeMap<BusId, Vec<Ghost>> = BTreeMap::new();
        if let Some(cells) = &self.cells {
            let radius = self.settings.interest_radius_m;
            let mut query = self.world.query::<(&Player, &Body, &Vitals)>();
            // A dead player is no ghost: the mobs of other sims forget it too.
            for (player, body, _) in query
                .iter(&self.world)
                .filter(|(_, _, vitals)| !vitals.health.is_dead())
            {
                let position = body.motion.position();
                for owner in cells.owners.others_within(cells.me, position, radius) {
                    if let Some(sim) = cells.sims.get(usize::from(owner)) {
                        lists.entry(sim.id.clone()).or_default().push(Ghost {
                            entity: player.number,
                            position: Point::from(position),
                            mob: None,
                        });
                    }
                }
            }
            for (entity, position, state) in self.mobs.awake_states() {
                for owner in cells.owners.others_within(cells.me, position, radius) {
                    if let Some(sim) = cells.sims.get(usize::from(owner)) {
                        lists.entry(sim.id.clone()).or_default().push(Ghost {
                            entity,
                            position: Point::from(position),
                            mob: Some(state),
                        });
                    }
                }
            }
        }
        for target in &self.ghost_targets {
            lists.entry(target.clone()).or_default();
        }
        self.ghost_targets = lists
            .iter()
            .filter(|(_, ghosts)| !ghosts.is_empty())
            .map(|(target, _)| target.clone())
            .collect();
        lists.into_iter().collect()
    }

    /// A bundle for every player that is not frozen, on `tick_threads` threads for a large world.
    fn bundles(&mut self, now_ms: u64, server_time: u64) -> Result<Bundled, MessageError> {
        let mut bodies = self.world.query::<(&Player, &Body)>();
        let mut everyone: Vec<(u64, Position)> = bodies
            .iter(&self.world)
            .map(|(player, body)| (player.number, body.motion.position()))
            .collect();
        let own: HashSet<u64> = everyone.iter().map(|(number, _)| *number).collect();
        let mut cards: HashMap<u64, MobCard> = HashMap::new();
        for ghost in self.ghosts.values().flat_map(|ghosts| &ghosts.entities) {
            if ghost.mob.is_none() && !own.contains(&ghost.entity) {
                everyone.push((ghost.entity, ghost.position));
            }
        }
        // A dead mob is sent only with a prefab, whose death states play its death; without one
        // it is not sent (M6-B spec section 5.6).
        let content = Arc::clone(&self.content);
        let shown = |state: &MobGhost| {
            state.health > 0 || content.mob_looks.look(state.vitals_id, None).is_some()
        };
        let mobs: Vec<(u64, Position, MobGhost)> = self
            .mob_states()
            .filter(|(_, _, state)| shown(state))
            .collect();
        // A personal mob's timeline starts when its owner's client can see it: the first state
        // then carries the spawn fragment, as on retail (live 2026-09-12: a sailor whose first
        // state came after the spawn fragment ended stayed invisible).
        let spawned: HashSet<u64> = self
            .world
            .query::<(&Player, &Vitals)>()
            .iter(&self.world)
            .filter(|(_, vitals)| vitals.spawned)
            .map(|(player, _)| player.number)
            .collect();
        // Where each player is, for the facing of the mobs that fight it.
        let positions: HashMap<u64, Position> = everyone.iter().copied().collect();
        for (entity, position, state) in mobs {
            everyone.push((entity, position));
            let owner = self.mobs.owner(entity);
            let mut card = card_of(entity, position, state, owner);
            card.asset = self.mobs.asset(entity);
            card.server_ms = server_time;
            if let Some(target) = self.mobs.target(entity) {
                card.look_target_changed = self.look_targets.insert(entity, target) != Some(target);
            }
            card.look_target = self.look_targets.get(&entity).copied().map(facet_entity);
            let previous = self.mob_health_shown.insert(entity, state.health);
            card.health_changed = previous != Some(state.health);
            card.first_damage = card.health_changed
                && previous == Some(state.max_health)
                && state.health < state.max_health;
            if let Some(&phase) = self.boss_phases.get(&entity) {
                let shown = self.boss_phases_shown.insert(entity, phase);
                card.boss_phase = Some(phase);
                card.boss_phase_changed = shown != Some(phase);
            }
            let unseen = owner.is_some_and(|owner| !spawned.contains(&owner));
            if let Some(look) = content
                .mob_looks
                .look(state.vitals_id, card.asset)
                .filter(|look| look.alc.is_some() && !unseen)
            {
                let target = self
                    .mobs
                    .target(entity)
                    .and_then(|target| positions.get(&target).copied());
                let pose = MobPose {
                    position,
                    brain: self.mobs.brain_state(entity),
                    dead: state.health == 0,
                    target,
                };
                let frame = self.mob_timelines.advance(
                    entity,
                    server_time,
                    &pose,
                    animations_of(&look.asset),
                );
                if frame.alert_started {
                    self.mobs.freeze(entity, now_ms + ALERT_STAND_MS);
                }
                card.alc = Some(frame);
            }
            cards.insert(entity, card);
        }
        let shown: HashSet<u64> = cards.keys().copied().collect();
        self.mob_timelines.keep_only(&shown);
        self.mob_health_shown
            .retain(|entity, _| shown.contains(entity));
        self.boss_phases_shown
            .retain(|entity, _| shown.contains(entity));
        self.look_targets.retain(|entity, _| shown.contains(entity));
        let mobs = &self.mobs;
        self.boss_phases
            .retain(|entity, _| mobs.health(*entity).is_some());
        // The radius is checked at start to be a finite number above 0.
        let Some(mut grid) = SpatialGrid::new(self.settings.interest_radius_m) else {
            return Ok(Bundled::default());
        };
        for (number, position) in &everyone {
            grid.insert(*number, *position);
        }
        let near = Neighbourhood {
            grid: &grid,
            positions: everyone.iter().copied().collect(),
            cards,
            mob_looks: &content.mob_looks,
            level: &content.tutorial_level,
            local_player_card: self.settings.local_player_card,
            now_ms,
            server_time,
        };
        let mut query = self.world.query::<(&mut Player, &Body, &Vitals)>();
        let mut players: Vec<(Mut<Player>, &Body, &Vitals)> =
            query.iter_mut(&mut self.world).collect();
        let threads = self
            .settings
            .tick_threads
            .min(players.len() / MIN_PLAYERS_PER_THREAD)
            .max(1);
        if threads == 1 {
            return near.bundles_of(&mut players);
        }
        let chunk = players.len().div_ceil(threads);
        let parts: Vec<Result<Bundled, MessageError>> = std::thread::scope(|scope| {
            let handles: Vec<_> = players
                .chunks_mut(chunk)
                .map(|part| scope.spawn(|| near.bundles_of(part)))
                .collect();
            handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                })
                .collect()
        });
        let mut bundled = Bundled::default();
        for part in parts {
            let part = part?;
            bundled.outgoing.extend(part.outgoing);
            bundled.members += part.members;
        }
        Ok(bundled)
    }

    /// Refuses a new player that would break a world rule.
    fn check(&mut self, arrival: &Arrival) -> Result<(), WorldError> {
        if self.by_connection.contains_key(&arrival.connection) {
            return Err(WorldError::AlreadyJoined(arrival.connection.clone()));
        }
        if self.player_of(arrival.character).is_some() {
            return Err(WorldError::CharacterInWorld(arrival.character));
        }
        if self.players() >= self.settings.max_players as usize {
            return Err(WorldError::Full);
        }
        let position = arrival.position;
        if WorldPosition::from_metres(position.x, position.y, position.z).is_none() {
            return Err(WorldError::NotFinite);
        }
        Ok(())
    }

    fn spawn(
        &mut self,
        arrival: Arrival,
        motion: Motion,
        replication: Replication,
        last_time_synch_ms: u64,
        handoffs: u64,
        no_handoff_before_ms: u64,
    ) {
        let entity = self
            .world
            .spawn((
                Player {
                    connection: arrival.connection.clone(),
                    character: arrival.character,
                    name: arrival.name,
                    look: arrival.look,
                    gear: match &arrival.items {
                        Some(items) => Some(Gear::carried_in(
                            &self.content.kits,
                            arrival.character,
                            items,
                        )),
                        None => Gear::starter(
                            &self.content.kits,
                            arrival.character,
                            arrival.backstory.as_deref(),
                        ),
                    },
                    backstory: arrival.backstory,
                    number: arrival.number,
                    bundles_sent: replication.bundles_sent,
                    last_time_synch_ms,
                    interest: Interest::new(
                        self.settings.interest_radius_m,
                        self.settings.max_visible_players,
                    ),
                    slots: replication.slots,
                    own_card_sent: false,
                    own_card_due_ms: arrival.own_card_due_ms,
                    client_context: arrival.client_context,
                    flow: arrival.flow,
                    leave_at_ms: None,
                    moved: false,
                    move_facing: None,
                    last_camera: None,
                    teleport_step: None,
                    refused_since_ms: None,
                    own_spawned_ms: None,
                    teleports: 0,
                    quest: None,
                    experience: arrival.experience,
                    experience_changed: false,
                    cooldowns: Vec::new(),
                    cooldowns_changed: false,
                    healed_to: None,
                    stamina: Stamina::default(),
                    combat: CombatTimer::default(),
                    list_version: FIRST_VERSION,
                    extra_members: Vec::new(),
                    statics: StaticViews::default(),
                    assets: AssetViews::default(),
                    frozen: false,
                    handoffs,
                    no_handoff_before_ms,
                },
                Body { motion },
                Vitals::new(arrival.health, arrival.spawned),
            ))
            .id();
        self.by_connection.insert(arrival.connection, entity);
    }

    fn leave_all(&mut self, connections: &[ConnectionRef]) -> Vec<Departure> {
        connections
            .iter()
            .filter_map(|connection| self.leave(connection))
            .collect()
    }

    fn player_of(&mut self, character: Uuid) -> Option<Entity> {
        let mut query = self.world.query::<(Entity, &Player)>();
        query
            .iter(&self.world)
            .find(|(_, player)| player.character == character)
            .map(|(entity, _)| entity)
    }

    fn departure(&self, entity: Entity) -> Option<Departure> {
        let player = self.world.get::<Player>(entity)?;
        let body = self.world.get::<Body>(entity)?;
        Some(departure_of(player, body))
    }
}

/// Queues `messages` for the client of `connection` on the reliable message channel; a message
/// that cannot be encoded is logged and left out.
fn send_now(outgoing: &mut Vec<Outgoing>, connection: &ConnectionRef, messages: &[ServerMessage]) {
    let envelopes: Vec<Vec<u8>> = messages
        .iter()
        .filter_map(|message| {
            message
                .envelope()
                .inspect_err(|error| tracing::warn!(%error, "message not sent"))
                .ok()
        })
        .collect();
    if !envelopes.is_empty() {
        outgoing.push(Outgoing {
            connection: connection.clone(),
            channel: MESSAGE_CHANNEL,
            reliable: true,
            envelopes,
        });
    }
}

/// The messages when mob `mob` picks player `number` as its target (2415); a sailor's tutorial
/// AI also saw the player (955; live-tutorial note, section 7).
fn targeted(number: u64, mob: u64, tutorial_ai: bool) -> Vec<ServerMessage> {
    use nw_messages::facet::component;
    let mut messages = vec![ServerMessage::SelectedAsTarget(SelectedAsTargetMsg(
        TargetMsg {
            header: nw_messages::FacetHeader::of(component::AI_TARGETABLE, number),
            creature: facet_entity(mob),
        },
    ))];
    if tutorial_ai {
        messages.push(ServerMessage::PlayerSeenByAi(PlayerSeenByAiMsg {
            header: nw_messages::FacetHeader::of_child(
                component::TUTORIAL_AI,
                component::TUTORIAL_AI_CHILD,
                mob,
            ),
        }));
    }
    messages
}

/// The messages when mob `mob`, whose `CharacterComponent` is `collision`, dies near player
/// `number`: it drops the player as its target (3916) and stops colliding (1793).
fn died(number: u64, mob: u64, collision: u64) -> [ServerMessage; 2] {
    use nw_messages::facet::component;
    [
        ServerMessage::UnselectedAsTarget(UnselectedAsTargetMsg(TargetMsg {
            header: nw_messages::FacetHeader::of(component::AI_TARGETABLE, number),
            creature: facet_entity(mob),
        })),
        ServerMessage::DisableCollision(DisableCollisionMsg {
            header: nw_messages::FacetHeader::of(collision, mob),
        }),
    ]
}

/// The display name string id of the ruin chest (the retail answer).
const CHEST_DISPLAY_NAME: &[u8] = b"@LootContainer_AncientSmall_DisplayName";
/// The validation result of an accepted interaction.
const INTERACTION_ACCEPTED: u8 = 1;

/// The entity number whose facet entity is that of the static entity `target` (its first eight
/// bytes reversed, as for a player).
fn facet_number(target: &[u8; nw_messages::wire::RAW_ID_LEN]) -> u64 {
    let mut first = [0; 8];
    first.copy_from_slice(&target[..8]);
    u64::from_be_bytes(first)
}

/// The accept of player `number`'s gather `request` (3743; ftue-static-entities note, section 5).
fn gather_accept(number: u64, request: InteractionRequest) -> ServerMessage {
    ServerMessage::InteractionValidated(InteractionValidatedMsg {
        header: nw_messages::FacetHeader::of(nw_messages::facet::component::INTERACTOR, number),
        target: request.target,
        target_entity: request.target_entity,
        interaction: request.interaction,
        kind: request.kind,
        status: INTERACTION_ACCEPTED,
        name: CHEST_DISPLAY_NAME.to_vec(),
        extra: 0,
        // An id of unknown meaning: one per player and interaction.
        id: *Uuid::new_v5(
            &Uuid::from_u64_pair(number, request.interaction),
            &request.target,
        )
        .as_bytes(),
    })
}

/// The end of player `number`'s gather `request`: the interaction ends (3746) and the chest's
/// gathering stops (2724).
fn gather_end(number: u64, request: InteractionRequest) -> [ServerMessage; 2] {
    [
        ServerMessage::InteractionEnded(InteractionEndedMsg {
            header: nw_messages::FacetHeader::of(nw_messages::facet::component::INTERACTOR, number),
        }),
        ServerMessage::GatheringInterrupted(GatheringInterruptedMsg {
            header: nw_messages::FacetHeader::of(
                nw_messages::facet::component::CHEST_GATHERABLE_CONTROLLER,
                facet_number(&request.target),
            ),
        }),
    ]
}

fn departure_of(player: &Player, body: &Body) -> Departure {
    Departure {
        connection: player.connection.clone(),
        character: player.character,
        position: body.motion.position(),
        moved_at_ms: body.motion.moved_ms(),
    }
}

/// What the bundle step of one tick reads: everyone's position, in a grid, and the cards of the
/// living mobs.
struct Neighbourhood<'a> {
    grid: &'a SpatialGrid<u64>,
    positions: HashMap<u64, Position>,
    cards: HashMap<u64, MobCard>,
    mob_looks: &'a MobLooks,
    /// The tutorial level's static and asset entities.
    level: &'a FtueLevel,
    local_player_card: bool,
    now_ms: u64,
    server_time: u64,
}

impl Neighbourhood<'_> {
    /// The bundles and time messages of `players`, in their order.
    fn bundles_of(
        &self,
        players: &mut [(Mut<Player>, &Body, &Vitals)],
    ) -> Result<Bundled, MessageError> {
        let mut outgoing = Vec::new();
        let mut counted = 0_u64;
        for (player, body, vitals) in players.iter_mut() {
            if player.frozen {
                continue;
            }
            let own = player.number;
            let own_position = body.motion.position();
            let others = self
                .grid
                .near(own_position)
                .filter(|(number, _)| *number != own);
            let seen = player.interest.update(own_position, others);
            // Until the client has spawned, other entities share the offline cache with the
            // own player (local-player-check note). Hold them until the first movement.
            let hold_others = self.local_player_card && !vitals.spawned;
            let mut numbers: Vec<u64> = if positions_only() || hold_others {
                Vec::new()
            } else {
                seen.entered
                    .into_iter()
                    .chain(seen.stayed)
                    .filter(|number| self.cards.get(number).is_none_or(|card| card.seen_by(own)))
                    .collect()
            };
            // The static entities come with the other entities, once the client has spawned.
            if !(positions_only() || hold_others) {
                numbers.extend(player.statics.numbers());
                numbers.extend(player.assets.numbers());
            }
            numbers.sort_unstable();
            player.slots.keep_only(&numbers);
            // Until its client spawned the player, the client keeps the own states in its
            // offline portrayal cache and merges every later record of the slot into the cached
            // one; such a merge crashes it (player-component note, crashes 2 and 3). So until
            // then the own group goes out once, whole, with the card; after the spawn the card
            // comes again once per refresh and the position every tick.
            let first_card = !player.own_card_sent
                && self.local_player_card
                && self.now_ms >= player.own_card_due_ms;
            if first_card {
                player.own_card_sent = true;
                player.own_spawned_ms = Some(self.server_time);
            }
            let own_carded = first_card
                || (self.local_player_card
                    && vitals.spawned
                    && player.bundles_sent % CARD_REFRESH_BUNDLES == 0);
            let own_sent = first_card || vitals.spawned || !self.local_player_card;
            let own_card = if first_card {
                OwnCard::Spawn
            } else if own_carded {
                OwnCard::Refresh
            } else {
                OwnCard::Without
            };
            // A move by the server goes once the client has spawned, in three states, one per
            // bundle (retail; [`TeleportStep`]).
            let can_move = own_card != OwnCard::Spawn && vitals.spawned;
            let step = if can_move && std::mem::take(&mut player.moved) {
                player.teleports = player.teleports.wrapping_add(1);
                Some(TeleportStep::Place)
            } else if can_move {
                player.teleport_step.take()
            } else {
                None
            };
            let teleport = step.map(|step| {
                player.teleport_step = step.next();
                let since_spawn_ms = player
                    .own_spawned_ms
                    .map_or(0, |spawned| self.server_time.saturating_sub(spawned));
                tracing::debug!(
                    player = own,
                    teleports = player.teleports,
                    since_spawn_ms,
                    ?step,
                    "own teleport"
                );
                OwnTeleport::of(player.teleports, since_spawn_ms, player.move_facing, step)
            });
            let mut groups: Vec<BundleGroup> = Vec::new();
            if own_sent {
                // The whole gear and achievements go with the first card; later the
                // achievements send their changes, and the gear its whole states again after a
                // change.
                let first = own_card == OwnCard::Spawn;
                let achievements = if first {
                    player.flow.first_achievements()
                } else {
                    player.flow.take_achievement_changes()
                };
                let gear = player.gear.as_mut().and_then(|gear| {
                    if first {
                        Some(gear.states())
                    } else {
                        gear.take_change()
                    }
                });
                // The quest tracker: the whole quest with the first card, then its changes.
                let objectives = player.quest.as_mut().and_then(|quest| {
                    if first {
                        Some(quest.first_state())
                    } else {
                        quest.take_change()
                    }
                });
                // The experience: whole with the first card, then after each change.
                let progression = if first {
                    Some(ProgressionState::login(player.experience))
                } else {
                    std::mem::take(&mut player.experience_changed)
                        .then(|| ProgressionState::experience(player.experience))
                };
                // The cooldowns and a heal after a potion.
                let cooldowns = std::mem::take(&mut player.cooldowns_changed).then(|| {
                    player.list_version += 1;
                    CooldownsState {
                        consumables: Some(FullList {
                            version: Some(player.list_version),
                            elements: player.cooldowns.clone(),
                        }),
                    }
                });
                let health_change = player.healed_to.take();
                let stamina = player.stamina.take_change();
                player.combat.tick(self.now_ms);
                let combat_status = player.combat.take_change(self.now_ms, self.server_time);
                let states = PlayerStates {
                    character_id: *player.character.as_bytes(),
                    look: player.look,
                    health: Some((vitals.health.current(), vitals.health.max())),
                    gear: gear.as_ref(),
                    achievements: achievements.as_ref(),
                    objectives: objectives.as_ref(),
                    progression,
                    health_change,
                    cooldowns: cooldowns.as_ref(),
                    stamina,
                    combat_status,
                };
                if let Some(mut group) = own_group(own, states, own_position, own_card, teleport)? {
                    group.members.append(&mut player.extra_members);
                    groups.push(group);
                }
            }
            if let Some(level_info) = player.flow.level_info_due(self.now_ms) {
                outgoing.push(Outgoing {
                    connection: player.connection.clone(),
                    channel: MESSAGE_CHANNEL,
                    reliable: true,
                    envelopes: vec![level_info.envelope()?],
                });
            }
            let statics_ids = player.flow.phase_id().map(|phase_id| {
                let player_gde_ref = raw_id_of_entity(own);
                let mut player_facet = [0; FACET_LEN];
                player_facet.copy_from_slice(&player_gde_ref[..FACET_LEN]);
                player_facet.reverse();
                ftue_entities::SessionIds {
                    player_gde_ref,
                    player_facet,
                    phase_id,
                }
            });
            for number in numbers {
                if number >= STATIC_ENTITY_BASE {
                    let (Some(ids), Some(slot)) = (statics_ids, player.slots.slot(number)) else {
                        continue;
                    };
                    groups.extend(if number >= ASSET_ENTITY_BASE {
                        player.assets.group(number, slot, self.level, &ids)
                    } else {
                        player
                            .statics
                            .group(number, slot, &self.level.statics, &ids)
                    });
                    continue;
                }
                let seen = match (self.cards.get(&number), self.positions.get(&number)) {
                    (Some(card), _) => Ok(card),
                    (None, Some(position)) => Err(*position),
                    (None, None) => continue,
                };
                // Every slot in use: the entity is not sent until one is free.
                let Some(slot) = player.slots.slot(number) else {
                    continue;
                };
                // Staggered by the entity number, so the cards of a crowd do not all come in one
                // bundle.
                let refresh = player.bundles_sent.wrapping_add(number) % CARD_REFRESH_BUNDLES == 0;
                let carded = slot.new || refresh;
                // A mob card, else a player's position.
                groups.extend(match seen {
                    Ok(card) => {
                        let look = self.mob_looks.look(card.state.vitals_id, card.asset);
                        // A mob with a prefab gets its card once, as on retail: the card makes
                        // the client load the creature slice, and a card again starts that
                        // load over, so the mob never shows (live 2026-09-12).
                        let carded = if look.is_some() { slot.new } else { carded };
                        let group = mob_group(slot.interest, card, look, carded)?;
                        if let Some(group) = &group {
                            log_mob_group(number, carded, group);
                        }
                        group
                    }
                    Err(position) => {
                        remote_group(slot.interest, number, EntityKind::PLAYER, position, carded)?
                    }
                });
            }
            counted += groups
                .iter()
                .map(|group| group.members.len() as u64)
                .sum::<u64>();
            for bundle in bundles(
                player.bundles_sent,
                player.client_context,
                &mut player.slots,
                groups,
            )? {
                player.bundles_sent = player.bundles_sent.saturating_add(1);
                outgoing.push(Outgoing {
                    connection: player.connection.clone(),
                    channel: REPLICATION_CHANNEL,
                    reliable: reliable_bundles(),
                    envelopes: vec![bundle],
                });
            }
            if self.now_ms.saturating_sub(player.last_time_synch_ms) >= TIME_SYNCH_INTERVAL_MS {
                player.last_time_synch_ms = self.now_ms;
                outgoing.push(Outgoing {
                    connection: player.connection.clone(),
                    channel: MESSAGE_CHANNEL,
                    reliable: true,
                    envelopes: vec![
                        ServerMessage::TimeSynch(TimeSynchMsg::at_server_ms(self.server_time))
                            .envelope()?,
                    ],
                });
            }
        }
        Ok(Bundled {
            outgoing,
            members: counted,
        })
    }
}

/// The offer that hands `player` to another sim, with its run of its open-world `quest`.
/// `enter_at` is `Some` for a player that leaves this level for another one (the end of the
/// tutorial): it appears there at that position, the taker sends the level change, and the
/// client's entity slots start empty.
fn handoff_offer(
    player: &Player,
    body: &Body,
    vitals: &Vitals,
    now_ms: u64,
    enter_at: Option<Position>,
    quest: Option<QuestRun>,
) -> HandoffOffer {
    let state = body.motion.export(now_ms);
    let enters = enter_at.is_some();
    HandoffOffer {
        connection: player.connection.clone(),
        character: player.character,
        name: player.name.clone(),
        entity: player.number,
        position: Point::from(enter_at.unwrap_or(state.position)),
        move_age_ms: state.move_age_ms,
        budget_m: state.budget_m,
        bundles_sent: player.bundles_sent,
        slots: if enters {
            Vec::new()
        } else {
            player
                .slots
                .assigned()
                .iter()
                .map(|(entity, interest)| (*entity, *interest))
                .collect()
        },
        pending_stops: if enters {
            Vec::new()
        } else {
            player.slots.pending_stops().to_vec()
        },
        time_synch_age_ms: now_ms.saturating_sub(player.last_time_synch_ms),
        handoffs: player.handoffs.saturating_add(1),
        health: vitals.health.current(),
        look: player.look.map(|look| look.choices()),
        backstory: player.backstory.clone(),
        phasing_sequence: player.flow.phasing_sequence(),
        client_context: player.client_context,
        enter_level: enters,
        items: player.gear.as_ref().map(Gear::carried),
        experience: player.experience,
        quest,
        achievements: player.flow.achievement_bits(),
    }
}

/// The sim of `table` that owns the cell at `position`; `None` when the table does not fit its
/// own size or no live sim owns that cell.
fn owner_in(table: &CellTable, position: Position) -> Option<SimSlot> {
    let cell = table.cell_size_m;
    let grid = CellGrid::new(table.columns as f32 * cell, table.rows as f32 * cell, cell).ok()?;
    let owners = CellOwners::from_cells(grid, table.owners.clone())?;
    let index = owners.owner_at(position)?;
    table.sims.get(usize::from(index)).cloned()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use nw_bus::CellTable;
    use nw_carrier::sender::MAX_CHUNK_PAYLOAD;
    use nw_messages::{
        ALC_TYPE_INDEX, GDE_METADATA_TYPE_INDEX, LOCAL_PLAYER_INTEREST, LOGIN_MESSAGE_TYPES,
        LOOK_CHOICES, MemberState, OPEN_WORLD_LEVEL, PAPERDOLL_TYPE_INDEX,
        PLAYER_APPEARANCE_TYPE_INDEX, PLAYER_EMPTY_COMPONENTS, PlayerAppearanceState,
        ReplicatedStateBundle, SlotRead, SlotUpdate, VITALS_TYPE_INDEX, VitalsState,
    };
    use nw_reflect::{Envelope, encode_server_messages};
    use nw_sim_core::test_support::{FlatGround, NoGround};
    use nw_sim_core::{
        CellGrid, CellOwners, GroundCheck, MoveLimits, MoveRefusal, TUTORIAL_EXIT, TUTORIAL_START,
    };

    use super::*;
    use crate::combat::test_support::{TEST_LEVEL, TEST_MOB};
    use crate::combat::tests::test_combat;
    use crate::mobs::VitalsSpawn;

    /// The largest entity number that fits the 5-byte prefix code (35 bits).
    const LARGEST_NUMBER_SHIFT: u32 = 29;

    const RADIUS: f32 = 100.0;
    pub(super) const NOW: u64 = 10_000;
    pub(super) const SERVER_TIME: u64 = 7;
    /// A saved look: male, face05, skin01, hairstyle65m (player-appearance note).
    const SAVED_LOOK: [u8; LOOK_CHOICES] = [0, 5, 0, 65, 44, 4, 4, 11, 0, 6, 26, 14];
    /// 5 m/s with no margin and a burst of 1 s: at most 5 m in one move.
    const SPEED_MPS: f32 = 5.0;
    const BURST_SECONDS: f32 = 1.0;
    const GROUND_TOLERANCE_M: f32 = 2.0;
    const MAX_ABOVE_GROUND_M: f32 = 150.0;
    const LEVEL_M: f32 = 14_336.0;
    pub(super) const CELL_M: f32 = 2048.0;
    /// The first column of the second sim of two (columns 3 to 6).
    pub(super) const BORDER_X: f32 = 3.0 * CELL_M;
    const MARGIN_M: f32 = 5.0;
    const RETRY_MS: u64 = 1000;
    const GHOST_TTL_MS: u64 = 1000;
    const MAX_PLAYERS: u32 = 3;
    const MANY: usize = 1000;
    const TWO: usize = 2;
    const ONE_SECOND_MS: u64 = 1000;
    const A_BIT_LATER_MS: u64 = 100;
    const CLOSE_M: f32 = 0.01;
    const GENERATION: u64 = 5;
    const BUNDLES_BEFORE: u64 = 900;
    const GHOST_NUMBER: u64 = 99;
    const RESUME_BASE: u64 = 1 << 32;

    pub(super) fn slot(name: &str) -> SimSlot {
        SimSlot {
            id: BusId::try_from(name.to_owned()).expect("id"),
            generation: GENERATION,
        }
    }

    /// The sea height of the test level: below the test players.
    pub(super) const OCEAN_LEVEL_M: f32 = 20.0;

    pub(super) fn settings(max_visible_players: usize, max_players: u32) -> WorldSettings {
        WorldSettings {
            interest_radius_m: RADIUS,
            max_visible_players,
            max_players,
            limits: MoveLimits::new(
                SPEED_MPS,
                0.0,
                BURST_SECONDS,
                GROUND_TOLERANCE_M,
                MAX_ABOVE_GROUND_M,
            )
            .expect("limits"),
            bounds: grid().bounds(),
            handoff_margin_m: MARGIN_M,
            handoff_retry_ms: RETRY_MS,
            ghost_ttl_ms: GHOST_TTL_MS,
            tick_threads: 1,
            local_player_card: true,
            level: OPEN_WORLD_LEVEL,
            ocean_level_m: OCEAN_LEVEL_M,
        }
    }

    fn world_of(name: &str, settings: WorldSettings) -> SimWorld {
        SimWorld::new(
            settings,
            WorldIds::random(0).expect("ids"),
            Arc::new(NoGround),
            slot(name),
            test_combat(),
            Arc::default(),
        )
    }

    fn world() -> SimWorld {
        world_of("sim-a", settings(MANY, MAX_PLAYERS))
    }

    /// A spawn of the test mob type at (`x`, 10).
    pub(super) fn test_spawn(x: f32) -> VitalsSpawn {
        VitalsSpawn {
            vitals_id: nw_formats::objectstream::lowercase_crc32(TEST_MOB),
            level: TEST_LEVEL,
            position: [x, 10.0, 50.0],
            ..VitalsSpawn::default()
        }
    }

    /// A world of `sim-a` with the test mob spawns `spawns`.
    pub(super) fn world_with_spawns(spawns: Vec<VitalsSpawn>) -> SimWorld {
        let mut combat = test_combat();
        combat.spawns = spawns.into();
        SimWorld::new(
            settings(MANY, MAX_PLAYERS),
            WorldIds::random(0).expect("ids"),
            Arc::new(NoGround),
            slot("sim-a"),
            combat,
            Arc::default(),
        )
    }

    fn grid() -> CellGrid {
        CellGrid::new(LEVEL_M, LEVEL_M, CELL_M).expect("grid")
    }

    /// The table of `names` in column stripes.
    pub(super) fn table(names: &[&str]) -> CellTable {
        let indexes: Vec<u16> = (0..).take(names.len()).collect();
        let owners = CellOwners::columns(grid(), &indexes);
        CellTable {
            version: 1,
            cell_size_m: CELL_M,
            columns: grid().columns(),
            rows: grid().rows(),
            sims: names.iter().map(|name| slot(name)).collect(),
            owners: owners.cells().to_vec(),
        }
    }

    pub(super) fn connection(number: u64) -> ConnectionRef {
        ConnectionRef {
            gateway: BusId::try_from("gw-1".to_owned()).expect("id"),
            connection: number,
        }
    }

    fn at(x: f32) -> Position {
        Position {
            x,
            y: 10.0,
            z: 50.0,
        }
    }

    /// The character name of test player `number`.
    fn player_name(number: u64) -> String {
        format!("Player{number}")
    }

    pub(super) fn admit_at(number: u64, x: f32, login: bool) -> Admit {
        Admit {
            connection: connection(number),
            account: Uuid::new_v4(),
            character: Uuid::new_v4(),
            name: player_name(number),
            entity: number,
            position: Point {
                x,
                y: 10.0,
                z: 50.0,
            },
            login,
            handoffs: 0,
            bundles_sent: 0,
            look: None,
            backstory: None,
        }
    }

    pub(super) fn join(world: &mut SimWorld, number: u64, x: f32) {
        world
            .admit(&admit_at(number, x, true), NOW, SERVER_TIME)
            .expect("admit");
        // The combat tests treat a joined player as already in the world, so end its spawn
        // protection (a real client does this by sending its first movement).
        let entity = world.player_by_number(number).expect("player");
        world
            .world
            .get_mut::<Vitals>(entity)
            .expect("vitals")
            .spawned = true;
    }

    /// The entity numbers in the replication bundles of one connection this tick.
    fn seen_by(world: &SimWorld, outgoing: &[Outgoing], who: &ConnectionRef) -> Vec<u64> {
        positions_seen_by(world, outgoing, who)
            .into_iter()
            .map(|(number, _)| number)
            .collect()
    }

    fn bundle_of(outgoing: &[Outgoing], who: &ConnectionRef) -> Option<ReplicatedStateBundle> {
        let message = outgoing
            .iter()
            .find(|out| out.connection == *who && out.channel == REPLICATION_CHANNEL)?;
        Some(bundle_in(&message.envelopes[0]))
    }

    fn bundle_in(bytes: &[u8]) -> ReplicatedStateBundle {
        let envelope = Envelope::decode(bytes).expect("envelope");
        let Some(Ok(ServerMessage::Bundle(bundle))) = ServerMessage::from_envelope(&envelope)
        else {
            panic!("expected a bundle");
        };
        bundle
    }

    /// The entity in slot `interest` of `who`, as this sim gave the slots.
    fn entity_in_slot(world: &SimWorld, who: &ConnectionRef, interest: u16) -> Option<u64> {
        let player = world.world.get::<Player>(*world.by_connection.get(who)?)?;
        if interest == LOCAL_PLAYER_INTEREST {
            return Some(player.number);
        }
        player
            .slots
            .assigned()
            .iter()
            .find(|(_, slot)| **slot == interest)
            .map(|(entity, _)| *entity)
    }

    /// The groups of `bundle` of `who`, each with the entity of its slot.
    fn groups_in(
        world: &SimWorld,
        who: &ConnectionRef,
        bundle: &ReplicatedStateBundle,
    ) -> Vec<(u64, SlotUpdate)> {
        SlotRead::of(bundle)
            .expect("read")
            .updates
            .into_iter()
            .map(|update| {
                let entity = update
                    .entity
                    .or_else(|| entity_in_slot(world, who, update.interest))
                    .expect("every slot is tied");
                (entity, update)
            })
            .collect()
    }

    /// The entity numbers and x positions in every replication bundle of one connection this tick.
    fn positions_seen_by(
        world: &SimWorld,
        outgoing: &[Outgoing],
        who: &ConnectionRef,
    ) -> Vec<(u64, f32)> {
        outgoing
            .iter()
            .filter(|out| out.connection == *who && out.channel == REPLICATION_CHANNEL)
            .flat_map(|out| {
                out.envelopes.iter().flat_map(|bytes| {
                    groups_in(world, who, &bundle_in(bytes))
                        .into_iter()
                        .filter_map(|(entity, update)| Some((entity, update.position?.x)))
                })
            })
            .collect()
    }

    pub(super) fn tick(world: &mut SimWorld, now: u64) -> TickOutput {
        world.tick(now, SERVER_TIME).expect("tick")
    }

    #[test]
    fn the_cell_table_spawns_the_mobs_of_this_sims_cells() {
        let west = CELL_M / 2.0;
        let east = LEVEL_M - CELL_M / 2.0;
        let spawns = [west, west + 1.0, east].map(test_spawn).to_vec();
        let mut world = world_with_spawns(spawns);
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        let changes = world.take_mob_changes();
        assert_eq!((changes.spawned, changes.removed), (2, 0));
        let total: usize = world.mob_counts().iter().map(|(_, count)| count).sum();
        assert_eq!(total, 2);
        world.set_table(&table(&["sim-b", "sim-a"])).expect("table");
        let changes = world.take_mob_changes();
        assert_eq!((changes.spawned, changes.removed), (1, 2));
    }

    #[test]
    fn a_border_mob_is_a_ghost_next_door() {
        let mut world = world_with_spawns(vec![test_spawn(BORDER_X - 20.0)]);
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, BORDER_X - 60.0);
        tick(&mut world, NOW);
        let ghosts = tick(&mut world, NOW + 33).ghosts;
        let (target, entities) = &ghosts[0];
        assert_eq!(target, &slot("sim-b").id);
        let mob = entities
            .iter()
            .find(|ghost| ghost.mob.is_some())
            .expect("the mob is a ghost");
        assert_eq!(mob.entity, crate::mobs::mob_entity(0));
        assert_eq!(mob.mob.map(|state| state.returning), Some(false));
    }

    /// The member classes of `entity` in every bundle of `who`, in order.
    fn classes_of(
        world: &SimWorld,
        outgoing: &[Outgoing],
        who: &ConnectionRef,
        entity: u64,
    ) -> Vec<u64> {
        outgoing
            .iter()
            .filter(|out| out.connection == *who && out.channel == REPLICATION_CHANNEL)
            .flat_map(|out| &out.envelopes)
            .flat_map(|bytes| groups_in(world, who, &bundle_in(bytes)))
            .filter(|(number, _)| *number == entity)
            .flat_map(|(_, update)| update.members)
            .map(|member| member.state.type_index())
            .collect()
    }

    /// [`classes_of`] the bundles of the next tick.
    fn next_classes_of(
        world: &mut SimWorld,
        now: u64,
        who: &ConnectionRef,
        entity: u64,
    ) -> Vec<u64> {
        let messages = tick(world, now).messages;
        classes_of(world, &messages, who, entity)
    }

    const CARD_CLASSES: [u64; 2] = [GDE_METADATA_TYPE_INDEX, ALC_TYPE_INDEX];

    #[test]
    fn a_mob_entering_view_sends_its_card_and_alc_once() {
        let x = CELL_M / 2.0;
        let mut world = world_with_spawns(vec![test_spawn(x)]);
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, x + 5.0);
        let mob = crate::mobs::mob_entity(0);
        assert_eq!(
            next_classes_of(&mut world, NOW, &connection(1), mob),
            CARD_CLASSES
        );
        assert_eq!(
            next_classes_of(&mut world, NOW + 33, &connection(1), mob),
            [ALC_TYPE_INDEX]
        );
    }

    #[test]
    fn the_world_snapshot_has_players_and_awake_mobs() {
        const FAR_M: f32 = 500.0;
        let x = CELL_M / 2.0;
        let mut world = world_with_spawns(vec![test_spawn(x), test_spawn(x + FAR_M)]);
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, x + 5.0);
        tick(&mut world, NOW);
        let (players, mobs, counts) = world.snapshot();
        assert_eq!(players.len(), 1);
        assert_eq!((players[0].entity, players[0].dead), (1, false));
        assert_eq!(players[0].name, player_name(1));
        assert_eq!(
            mobs.iter().map(|mob| mob.entity).collect::<Vec<_>>(),
            vec![crate::mobs::mob_entity(0)],
            "only the awake mob"
        );
        assert_eq!(counts[0], 1, "the far mob sleeps and is only counted");
        assert_eq!(counts.iter().sum::<u32>(), 2);
    }

    #[test]
    fn a_dead_player_is_marked() {
        const LETHAL: u32 = 100_000;
        let mut world = world_with_spawns(Vec::new());
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, CELL_M / 2.0);
        world.apply_hit(1, crate::mobs::mob_entity(0), LETHAL, 0, NOW);
        assert!(world.snapshot().0[0].dead);
    }

    #[test]
    fn bundle_members_count_self_and_others() {
        let mut world = world_with_spawns(Vec::new());
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        for number in 1..=3 {
            join(&mut world, number, CELL_M / 2.0 + number as f32);
        }
        assert_eq!(
            tick(&mut world, NOW + OWN_CARD_DELAY_MS).bundle_members,
            9 * (2 + PLAYER_EMPTY_COMPONENTS.len() as u64),
            "3 bundles of 3 groups, each a card, the empty player components and a position"
        );
    }

    #[test]
    fn before_its_client_spawned_the_own_player_goes_out_only_whole() {
        let mut world = world_with_spawns(Vec::new());
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        world
            .admit(&admit_at(1, CELL_M / 2.0, true), NOW, SERVER_TIME)
            .expect("admit");
        let own_groups = |world: &mut SimWorld, now: u64| {
            let messages = tick(world, now).messages;
            bundle_of(&messages, &connection(1))
                .map(|bundle| SlotRead::of(&bundle).expect("read").updates)
                .unwrap_or_default()
                .into_iter()
                .filter(SlotUpdate::is_local_player)
                .map(|update| update.card.is_some())
                .collect::<Vec<bool>>()
        };
        assert!(
            own_groups(&mut world, NOW).is_empty(),
            "no card on the login tick: the client has not applied LevelInfoChanged yet"
        );
        let card_at = NOW + OWN_CARD_DELAY_MS;
        assert_eq!(
            own_groups(&mut world, card_at),
            vec![true],
            "the whole card once the login has settled"
        );
        for step in 1..=CARD_REFRESH_BUNDLES {
            assert!(
                own_groups(&mut world, card_at + 33 * step).is_empty(),
                "nothing more for the own slot before the spawn, not even a card refresh"
            );
        }
        let entity = world.player_by_number(1).expect("player");
        world
            .world
            .get_mut::<Vitals>(entity)
            .expect("vitals")
            .spawned = true;
        assert_eq!(
            own_groups(&mut world, card_at + 33 * (CARD_REFRESH_BUNDLES + 1)),
            vec![false],
            "the position every tick after the spawn"
        );
    }

    #[test]
    fn other_entities_wait_until_the_client_spawned() {
        let mut world = world_with_spawns(Vec::new());
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        world
            .admit(&admit_at(1, CELL_M / 2.0, true), NOW, SERVER_TIME)
            .expect("admit");
        world
            .admit(&admit_at(2, CELL_M / 2.0 + 1.0, true), NOW, SERVER_TIME)
            .expect("admit");
        let messages = tick(&mut world, NOW + OWN_CARD_DELAY_MS).messages;
        let bundle = bundle_of(&messages, &connection(1)).expect("bundle");
        let updates = SlotRead::of(&bundle).expect("read").updates;
        assert_eq!(updates.len(), 1, "only the own player before spawn");
        assert!(
            updates.iter().all(SlotUpdate::is_local_player),
            "other entities share the offline cache until spawn"
        );
    }

    fn tutorial_world() -> SimWorld {
        let mut tutorial = settings(MANY, MAX_PLAYERS);
        tutorial.level = TUTORIAL_LEVEL;
        world_of("tut-1", tutorial)
    }

    fn level_info_in(outgoing: &Outgoing) -> nw_messages::LevelInfoChangedMsg {
        outgoing
            .envelopes
            .iter()
            .find_map(|bytes| {
                match ServerMessage::from_envelope(&Envelope::decode(bytes).expect("envelope")) {
                    Some(Ok(ServerMessage::LevelInfoChanged(level))) => Some(level),
                    _ => None,
                }
            })
            .expect("a level message")
    }

    #[test]
    fn a_tutorial_sim_logs_players_into_the_tutorial_level() {
        let mut world = tutorial_world();
        let login = world
            .admit(
                &admit_at(1, nw_sim_core::TUTORIAL_START.x, true),
                NOW,
                SERVER_TIME,
            )
            .expect("admit")
            .expect("login messages");
        let level = level_info_in(&login);
        assert_eq!(level.level, TUTORIAL_LEVEL);
        assert_eq!(level.client_context, LOGIN_CLIENT_CONTEXT);
    }

    /// The messages of `outgoing` for `who` on the message channel.
    fn messages_for(outgoing: &[Outgoing], who: &ConnectionRef) -> Vec<ServerMessage> {
        outgoing
            .iter()
            .filter(|out| out.connection == *who && out.channel == MESSAGE_CHANNEL)
            .flat_map(|out| &out.envelopes)
            .filter_map(|bytes| {
                ServerMessage::from_envelope(&Envelope::decode(bytes).expect("envelope"))
            })
            .map(|message| message.expect("message"))
            .collect()
    }

    #[test]
    fn the_tutorial_quest_unlocks_the_intro_the_level_is_answered_and_leaving_works() {
        const INTRO_UNLOCK_BIT: u32 = 14_804;
        let mut tutorial_settings = settings(MANY, MAX_PLAYERS);
        tutorial_settings.level = TUTORIAL_LEVEL;
        let mut world = SimWorld::new(
            tutorial_settings,
            WorldIds::random(0).expect("ids"),
            Arc::new(NoGround),
            slot("tut-1"),
            test_combat(),
            Arc::new(PlayerContent {
                achievements: nw_content::achievements::AchievementIndexes::build([(
                    "FTUE_Intro2_Cinematic_Unlock".to_owned(),
                    INTRO_UNLOCK_BIT,
                )]),
                tutorial_quest: Some(encounter::tests::tree()),
                ..PlayerContent::default()
            }),
        );
        world.set_table(&table(&["sim-a"])).expect("table");
        let who = connection(1);
        world
            .admit(
                &admit_at(1, nw_sim_core::TUTORIAL_START.x, true),
                NOW,
                SERVER_TIME,
            )
            .expect("admit");
        let card_at = NOW + OWN_CARD_DELAY_MS;
        let bundle = bundle_of(&tick(&mut world, card_at).messages, &who).expect("card");
        let own = SlotRead::of(&bundle)
            .expect("read")
            .updates
            .into_iter()
            .find(SlotUpdate::is_local_player)
            .expect("own group");
        let achievements = own.members.iter().find_map(|member| match &member.state {
            MemberState::Achievement(state) => state.bits.clone(),
            _ => None,
        });
        let Some(nw_messages::AchievementBits::Full(bits)) = achievements else {
            panic!("the whole bitset with the card: {achievements:?}");
        };
        assert_ne!(
            bits.elements[(INTRO_UNLOCK_BIT / 8) as usize] & 1 << (INTRO_UNLOCK_BIT % 8),
            0,
            "the intro is unlocked"
        );
        world.heard_from(&who);

        let again = tick(&mut world, NOW + level_flow::LEVEL_INFO_AGAIN_MS).messages;
        assert!(
            messages_for(&again, &who)
                .iter()
                .any(|message| matches!(message, ServerMessage::LevelInfoChanged(_))),
            "the level message again"
        );
        let header = nw_messages::FacetHeader::of(nw_messages::facet::component::PLAYER, 1);
        world
            .facet_message(
                &who,
                ClientFacetMsg::AckLevelInfo {
                    header,
                    client_context: LOGIN_CLIENT_CONTEXT,
                },
                NOW,
            )
            .expect("ack");
        world
            .facet_message(
                &who,
                ClientFacetMsg::ReturnToMainWorld { header, flag: 0 },
                NOW,
            )
            .expect("leave");
        let staying = tick(&mut world, NOW + LEAVE_DELAY_MS - 1);
        assert!(staying.handoffs.is_empty(), "the last states go out first");
        let types = |output: &TickOutput| -> Vec<u64> {
            messages_for(&output.messages, &who)
                .iter()
                .map(ServerMessage::type_index)
                .collect()
        };
        assert_eq!(
            types(&staying),
            [
                nw_messages::facet::UPDATE_PHASING_TYPE_INDEX,
                nw_messages::facet::TELEPORT_WITH_LOADING_SCREEN_TYPE_INDEX,
            ],
            "the answer to the level, then the loading screen at once"
        );
        let output = tick(&mut world, NOW + level_flow::LEVEL_INFO_AGAIN_MS + 33);
        let left = messages_for(&output.messages, &who);
        let [ServerMessage::UpdatePhasing(empty)] = left.as_slice() else {
            panic!("the empty phasing at the handoff: {left:?}");
        };
        assert!(empty.entries.is_empty());
        assert_eq!(output.handoffs.len(), 1, "a request leaves from anywhere");
        let offer = &output.handoffs[0].offer;
        assert!(offer.enter_level);
        assert_eq!(
            offer.phasing_sequence, 2,
            "the level change goes on from it"
        );
    }

    /// The sailor asset of the prefab table (mob-cards note, section 1.3).
    const SAILOR_ASSET_HEX: &str = "b7e33b294bcf57b8a476abdc3a31fae7";

    /// The mobs whose cards name `asset` in the bundles of `outgoing` for `who`.
    fn cards_of(
        outgoing: &[Outgoing],
        who: &ConnectionRef,
        asset: nw_messages::AssetId,
    ) -> Vec<u64> {
        outgoing
            .iter()
            .filter(|out| out.connection == *who && out.channel == REPLICATION_CHANNEL)
            .flat_map(|out| &out.envelopes)
            .flat_map(|bytes| SlotRead::of(&bundle_in(bytes)).expect("read").updates)
            .flat_map(|update| update.members)
            .filter_map(|member| match member.state {
                MemberState::GdeMetadata(card) if card.asset == Some(asset) => card.entity_number(),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_ship_fight_shows_its_sailors_to_their_player_and_brings_two_more() {
        let prefab = format!(
            r#"[{{"vitals_id":{},"vitals_name":null,"asset":"{SAILOR_ASSET_HEX}","sub_id":2,
                "alc_component":5,"vitals_component":9,"first_members":[],"death_members":[],
                "seen":1}}]"#,
            encounter::tests::sailor_vitals()
        );
        let prefabs = nw_content::mob_prefabs::MobPrefabs::parse(&prefab).expect("table");
        let (mob_looks, _) = crate::player_content::MobLooks::of(&prefabs);
        let mut tutorial_settings = settings(MANY, MAX_PLAYERS);
        tutorial_settings.level = TUTORIAL_LEVEL;
        let mut world = SimWorld::new(
            tutorial_settings,
            WorldIds::random(0).expect("ids"),
            Arc::new(NoGround),
            slot("tut-1"),
            encounter::tests::sailor_combat(),
            Arc::new(PlayerContent {
                mob_looks,
                kill_types: encounter::tests::kill_types(),
                tutorial_quest: Some(encounter::tests::tree()),
                ..PlayerContent::default()
            }),
        );
        world.set_table(&table(&["sim-a"])).expect("table");
        let asset = world
            .content
            .mob_looks
            .look(encounter::tests::sailor_vitals(), None)
            .expect("sailor look")
            .asset;
        let sailor_cards =
            |outgoing: &[Outgoing], who: &ConnectionRef| cards_of(outgoing, who, asset);
        let (owner, other) = (connection(1), connection(2));
        for number in [1, 2] {
            let mut admit = admit_at(number, TUTORIAL_START.x, true);
            admit.position = Point::from(nw_sim_core::TUTORIAL_START);
            world.admit(&admit, NOW, SERVER_TIME).expect("admit");
        }
        let spawn_at = NOW + OWN_CARD_DELAY_MS;
        tick(&mut world, spawn_at);
        world.heard_from(&owner);
        world.heard_from(&other);
        let output = tick(&mut world, spawn_at + 33);
        assert!(
            sailor_cards(&output.messages, &owner).is_empty(),
            "the first sailor comes later, as on retail"
        );
        let card_at = NOW + encounter::FIRST_SAILOR_DELAY_MS;
        let output = tick(&mut world, card_at + 33);
        let first = sailor_cards(&output.messages, &owner);
        assert_eq!(first.len(), 1, "the owner sees its waiting sailor");
        assert!(
            sailor_cards(&output.messages, &other)
                .iter()
                .all(|sailor| !first.contains(sailor)),
            "another player does not see it"
        );
        let mut again = 0;
        for step in 2..=CARD_REFRESH_BUNDLES + 1 {
            let output = tick(&mut world, card_at + 33 * step);
            again += sailor_cards(&output.messages, &owner)
                .iter()
                .filter(|sailor| **sailor == first[0])
                .count();
        }
        assert_eq!(again, 0, "a mob with a prefab gets its card once");
        let card_at = card_at + 33 * CARD_REFRESH_BUNDLES;
        let header = nw_messages::FacetHeader::of(nw_messages::facet::component::PLAYER, 1);
        world
            .facet_message(
                &owner,
                ClientFacetMsg::QteCompleted {
                    header,
                    sequence: 0,
                },
                card_at,
            )
            .expect("lesson done");
        let sailor = world.mobs.get_mut(first[0]).expect("sailor");
        let max = sailor.health.max();
        sailor.health.take(max);
        let mut seen = Vec::new();
        for step in 2..5 {
            let output = tick(&mut world, card_at + 33 * step);
            seen.extend(sailor_cards(&output.messages, &owner));
        }
        seen.retain(|sailor| *sailor != first[0]);
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 2, "two more sailors come to their owner");
    }

    #[test]
    fn the_tutorial_exit_moves_a_spawned_player_into_the_open_world() {
        let mut tutorial = tutorial_world();
        assert!(
            tutorial
                .set_table(&table(&["sim-a", "sim-b"]))
                .expect("table")
                .is_empty(),
            "a tutorial sim keeps its players whatever the open-world table says"
        );
        let mut at_exit = admit_at(1, TUTORIAL_EXIT.x, true);
        at_exit.position = Point::from(TUTORIAL_EXIT);
        tutorial.admit(&at_exit, NOW, SERVER_TIME).expect("admit");
        let card_at = NOW + OWN_CARD_DELAY_MS;
        assert!(
            tick(&mut tutorial, card_at).handoffs.is_empty(),
            "a player whose client has not spawned stays"
        );
        tutorial.heard_from(&connection(1));
        let handoffs = tick(&mut tutorial, card_at + 33).handoffs;
        assert_eq!(handoffs.len(), 1);
        let start = &handoffs[0];
        assert_eq!(start.target, slot("sim-b"), "the owner of the beach start");
        let offer = &start.offer;
        assert!(offer.enter_level);
        assert_eq!(offer.position, Point::from(OPEN_WORLD_START));
        assert_eq!(offer.client_context, LOGIN_CLIENT_CONTEXT);
        assert!(offer.slots.is_empty());

        let mut open = world_of("sim-b", settings(MANY, MAX_PLAYERS));
        open.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        let change = open
            .take(offer, card_at, SERVER_TIME)
            .expect("take")
            .expect("level change messages");
        let level = level_info_in(&change);
        assert_eq!(level.level, OPEN_WORLD_LEVEL);
        assert_eq!(level.client_context, LOGIN_CLIENT_CONTEXT + 1);
        let messages = tick(&mut open, card_at + OWN_CARD_DELAY_MS).messages;
        let bundle = bundle_of(&messages, &connection(1)).expect("the new spawn card");
        assert_eq!(
            bundle.byte_3,
            LOGIN_CLIENT_CONTEXT + 1,
            "bundles carry the new context"
        );
        assert!(
            SlotRead::of(&bundle)
                .expect("read")
                .updates
                .iter()
                .any(|update| update.is_local_player() && update.card.is_some()),
            "the player spawns again in the new level"
        );
    }

    #[test]
    fn a_client_message_after_the_card_ends_the_hold() {
        let mut world = world_with_spawns(Vec::new());
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        world
            .admit(&admit_at(1, CELL_M / 2.0, true), NOW, SERVER_TIME)
            .expect("admit");
        let own_updates = |world: &mut SimWorld, now: u64| {
            let messages = tick(world, now).messages;
            bundle_of(&messages, &connection(1))
                .map(|bundle| SlotRead::of(&bundle).expect("read").updates)
                .unwrap_or_default()
                .iter()
                .filter(|update| update.is_local_player())
                .count()
        };
        world.heard_from(&connection(1));
        let card_at = NOW + OWN_CARD_DELAY_MS;
        assert_eq!(own_updates(&mut world, card_at), 1, "the card");
        assert_eq!(
            own_updates(&mut world, card_at + 33),
            0,
            "a message before the card does not end the hold"
        );
        world.heard_from(&connection(1));
        for step in 2..=4 {
            assert_eq!(
                own_updates(&mut world, card_at + 33 * step),
                1,
                "the position every tick once the client spawned"
            );
        }
    }

    #[test]
    fn the_own_card_names_the_login_character() {
        let mut world = world_with_spawns(Vec::new());
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        let admit = admit_at(1, CELL_M / 2.0, true);
        world.admit(&admit, NOW, SERVER_TIME).expect("admit");
        let messages = tick(&mut world, NOW + OWN_CARD_DELAY_MS).messages;
        let bundle = bundle_of(&messages, &connection(1)).expect("bundle");
        let own = SlotRead::of(&bundle)
            .expect("read")
            .updates
            .into_iter()
            .find(SlotUpdate::is_local_player)
            .expect("own group");
        let character_id = own.members.iter().find_map(|member| match &member.state {
            MemberState::PlayerComponent(state) => state.character_id,
            _ => None,
        });
        assert_eq!(
            character_id,
            Some(*admit.character.as_bytes()),
            "the client sees its local player only when characterId is its login character"
        );
    }

    #[test]
    fn the_own_card_carries_the_saved_look_and_the_health() {
        let mut world = world_with_spawns(Vec::new());
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        let mut admit = admit_at(1, CELL_M / 2.0, true);
        admit.look = Some(SAVED_LOOK);
        world.admit(&admit, NOW, SERVER_TIME).expect("admit");
        let max = world.combat.data.player_max_health();
        let messages = tick(&mut world, NOW + OWN_CARD_DELAY_MS).messages;
        let bundle = bundle_of(&messages, &connection(1)).expect("bundle");
        let own = SlotRead::of(&bundle)
            .expect("read")
            .updates
            .into_iter()
            .find(SlotUpdate::is_local_player)
            .expect("own group");
        let state_of = |class: u64| {
            own.members
                .iter()
                .find(|member| member.state.type_index() == class)
                .map(|member| member.state.clone())
        };
        assert_eq!(
            state_of(PLAYER_APPEARANCE_TYPE_INDEX),
            Some(MemberState::Appearance(PlayerAppearanceState::of(
                PlayerLook::of_choices(SAVED_LOOK)
            ))),
            "the saved look, not the default mesh"
        );
        assert_eq!(
            state_of(VITALS_TYPE_INDEX),
            Some(MemberState::Vitals(VitalsState::player(max, max))),
            "full health of the player vitals row, so the bar is not 00/00"
        );
    }

    #[test]
    fn the_own_card_wears_the_starter_kit_and_a_handoff_keeps_it() {
        let mut combat = test_combat();
        combat.spawns = Vec::new().into();
        let mut world = SimWorld::new(
            settings(MANY, MAX_PLAYERS),
            WorldIds::random(0).expect("ids"),
            Arc::new(NoGround),
            slot("sim-a"),
            combat,
            Arc::new(PlayerContent {
                kits: gear::tests::kits(),
                ..PlayerContent::default()
            }),
        );
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        let mut admit = admit_at(1, CELL_M / 2.0, true);
        admit.backstory = Some(gear::tests::BACKSTORY.to_owned());
        world.admit(&admit, NOW, SERVER_TIME).expect("admit");
        let messages = tick(&mut world, NOW + OWN_CARD_DELAY_MS).messages;
        let bundle = bundle_of(&messages, &connection(1)).expect("bundle");
        let own = SlotRead::of(&bundle)
            .expect("read")
            .updates
            .into_iter()
            .find(SlotUpdate::is_local_player)
            .expect("own group");
        let expected = Gear::starter(
            &gear::tests::kits(),
            admit.character,
            Some(gear::tests::BACKSTORY),
        )
        .expect("gear")
        .states();
        let paperdoll = own
            .members
            .iter()
            .find(|member| member.state.type_index() == PAPERDOLL_TYPE_INDEX)
            .map(|member| member.state.clone());
        assert_eq!(
            paperdoll,
            Some(MemberState::Paperdoll(Box::new(expected.paperdoll)))
        );
        let player = world
            .world
            .query::<&Player>()
            .iter(&world.world)
            .next()
            .expect("player");
        assert_eq!(
            player.backstory.as_deref(),
            Some(gear::tests::BACKSTORY),
            "a handoff offer carries it on"
        );
    }

    #[test]
    fn a_taken_player_keeps_its_look() {
        let (mut old, _, _) = border_world();
        tick(&mut old, NOW);
        let mut offer = tick_offer(&mut old);
        offer.look = Some(SAVED_LOOK);
        let mut new = world_of("sim-b", settings(MANY, MAX_PLAYERS));
        new.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        new.take(&offer, NOW, SERVER_TIME).expect("take");
        let looks: Vec<Option<PlayerLook>> = new
            .world
            .query::<&Player>()
            .iter(&new.world)
            .map(|player| player.look)
            .collect();
        assert_eq!(looks, vec![Some(PlayerLook::of_choices(SAVED_LOOK))]);
    }

    #[test]
    fn without_the_card_knob_the_own_player_is_its_position_every_tick() {
        let mut world = world_of(
            "sim-a",
            WorldSettings {
                local_player_card: false,
                ..settings(MANY, MAX_PLAYERS)
            },
        );
        world
            .admit(&admit_at(1, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
        for step in 0..2 {
            let messages = tick(&mut world, NOW + 33 * step).messages;
            let bundle = bundle_of(&messages, &connection(1)).expect("bundle");
            let own: Vec<bool> = SlotRead::of(&bundle)
                .expect("read")
                .updates
                .into_iter()
                .filter(SlotUpdate::is_local_player)
                .map(|update| update.card.is_some())
                .collect();
            assert_eq!(own, vec![false], "no own card, the position every tick");
        }
    }

    #[test]
    fn a_lost_card_comes_again_within_a_second() {
        let x = CELL_M / 2.0;
        let mut world = world_with_spawns(vec![test_spawn(x)]);
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, x + 5.0);
        let mob = crate::mobs::mob_entity(0);
        tick(&mut world, NOW);
        let cards = (1..=CARD_REFRESH_BUNDLES)
            .filter(|step| {
                next_classes_of(&mut world, NOW + 33 * step, &connection(1), mob)
                    .contains(&GDE_METADATA_TYPE_INDEX)
            })
            .count();
        assert_eq!(cards, 1, "once more in the next second of bundles");
    }

    #[test]
    fn a_dead_mob_leaves_the_bundles_and_returns_with_a_new_card() {
        const LETHAL: u32 = 100_000;
        const MOB_RESPAWN_MS: u64 = 360_000;
        let x = CELL_M / 2.0;
        let mut world = world_with_spawns(vec![test_spawn(x)]);
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, x + 50.0);
        let mob = crate::mobs::mob_entity(0);
        tick(&mut world, NOW);
        world.apply_hit(mob, 1, LETHAL, 0, NOW);
        assert!(next_classes_of(&mut world, NOW + 33, &connection(1), mob).is_empty());
        let back = tick(&mut world, NOW + MOB_RESPAWN_MS).messages;
        assert_eq!(classes_of(&world, &back, &connection(1), mob), CARD_CLASSES);
    }

    #[test]
    fn a_mob_that_returns_to_view_gets_a_new_spawn_card() {
        let x = CELL_M / 2.0;
        let mut combat = test_combat();
        combat.spawns = vec![test_spawn(x)].into();
        let mut world = SimWorld::new(
            settings(1, MAX_PLAYERS),
            WorldIds::random(0).expect("ids"),
            Arc::new(NoGround),
            slot("sim-a"),
            combat,
            Arc::default(),
        );
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, x + 5.0);
        join(&mut world, 2, x + 4.0);
        let mob = crate::mobs::mob_entity(0);
        assert!(
            next_classes_of(&mut world, NOW, &connection(1), mob).is_empty(),
            "the nearer player takes the one place"
        );
        world.leave(&connection(2));
        assert_eq!(
            next_classes_of(&mut world, NOW + 33, &connection(1), mob),
            CARD_CLASSES
        );
        join(&mut world, 2, x + 4.0);
        assert!(next_classes_of(&mut world, NOW + 66, &connection(1), mob).is_empty());
        world.leave(&connection(2));
        assert_eq!(
            next_classes_of(&mut world, NOW + 99, &connection(1), mob),
            CARD_CLASSES,
            "a new card after it was out of view"
        );
    }

    #[test]
    fn a_ghost_mob_is_in_the_bundles() {
        let mut world = world_with_spawns(Vec::new());
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, BORDER_X - 5.0);
        let mob = crate::mobs::mob_entity(3);
        let ghost = Ghost {
            entity: mob,
            position: Point {
                x: BORDER_X + 5.0,
                y: 10.0,
                z: 50.0,
            },
            mob: Some(MobGhost {
                health: 900,
                max_health: 1000,
                returning: false,
                vitals_id: 1,
                category_id: 2,
                level: 3,
            }),
        };
        world.set_ghosts(&slot("sim-b").id, vec![ghost], NOW);
        assert_eq!(
            next_classes_of(&mut world, NOW, &connection(1), mob),
            CARD_CLASSES
        );
    }

    #[test]
    fn a_player_wakes_the_mobs_near_it() {
        let x = CELL_M / 2.0;
        let mut world = world_with_spawns(vec![test_spawn(x)]);
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        world.tick(NOW, SERVER_TIME).expect("tick");
        assert_eq!(world.mob_counts()[0], (MobState::Asleep, 1));
        join(&mut world, 1, x + 5.0);
        world.tick(NOW + 33, SERVER_TIME).expect("tick");
        assert_eq!(world.mob_counts()[0], (MobState::Asleep, 0));
    }

    #[test]
    fn a_move_inside_the_limits_is_seen_and_a_fast_one_is_refused() {
        let mut world = world();
        let (a, b) = (connection(1), connection(2));
        join(&mut world, 1, 0.0);
        join(&mut world, 2, 10.0);
        assert_eq!(
            world.move_to(&a, at(SPEED_MPS), NOW + ONE_SECOND_MS),
            Ok(GroundCheck::NoGround)
        );
        let outgoing = tick(&mut world, NOW + ONE_SECOND_MS).messages;
        let (_, seen_x) = positions_seen_by(&world, &outgoing, &b)[1];
        assert!((seen_x - SPEED_MPS).abs() < CLOSE_M);
        let far = at(SPEED_MPS * 10.0);
        let later = NOW + ONE_SECOND_MS + A_BIT_LATER_MS;
        assert_eq!(
            world.move_to(&a, far, later),
            Err(MoveError::Refused(MoveRefusal::TooFast))
        );
        let outgoing = tick(&mut world, later).messages;
        let (_, own_x) = positions_seen_by(&world, &outgoing, &a)[0];
        assert!((own_x - SPEED_MPS).abs() < CLOSE_M);
        assert_eq!(
            world.move_to(&connection(9), at(1.0), later),
            Err(MoveError::NotInWorld)
        );
        let departure = world.leave(&a).expect("left");
        assert!((departure.position.x - SPEED_MPS).abs() < CLOSE_M);
    }

    #[test]
    fn a_departure_carries_the_time_of_the_last_good_move() {
        let mut world = world();
        let a = connection(1);
        join(&mut world, 1, 0.0);
        assert_eq!(world.positions()[0].moved_at_ms, NOW);
        world
            .move_to(&a, at(SPEED_MPS), NOW + ONE_SECOND_MS)
            .expect("move");
        let refused = world.move_to(&a, at(SPEED_MPS * 10.0), NOW + ONE_SECOND_MS + 1);
        assert!(refused.is_err());
        let departure = world.leave(&a).expect("left");
        assert_eq!(departure.moved_at_ms, NOW + ONE_SECOND_MS);
    }

    #[test]
    fn a_server_move_goes_out_in_three_states_in_a_row() {
        let mut world = world();
        let a = connection(1);
        join(&mut world, 1, 0.0);
        let card_at = NOW + OWN_CARD_DELAY_MS;
        tick(&mut world, card_at);
        world.heard_from(&a);
        let entity = world.player_by_number(1).expect("player");
        let step = |world: &SimWorld| {
            world
                .world
                .get::<Player>(entity)
                .expect("player")
                .teleport_step
        };
        world.world.get_mut::<Player>(entity).expect("player").moved = true;
        let mut steps = Vec::new();
        for n in 1..=4 {
            tick(&mut world, card_at + 33 * n);
            steps.push(step(&world));
        }
        assert_eq!(
            steps,
            [
                Some(TeleportStep::Confirm),
                Some(TeleportStep::Hold),
                None,
                None
            ],
            "Place, then Confirm, then Hold, one per bundle"
        );
    }

    #[test]
    fn refused_moves_for_a_while_move_the_player_back() {
        let mut world = world();
        let a = connection(1);
        join(&mut world, 1, 0.0);
        let entity = world.player_by_number(1).expect("player");
        let refused_since_ms = |world: &SimWorld| {
            world
                .world
                .get::<Player>(entity)
                .expect("player")
                .refused_since_ms
        };
        let moved = |world: &SimWorld| world.world.get::<Player>(entity).expect("player").moved;
        let far = at(SPEED_MPS * 10.0);
        let first = NOW + 1;
        assert_eq!(
            world.move_to(&a, far, first),
            Err(MoveError::Refused(MoveRefusal::TooFast))
        );
        assert_eq!(refused_since_ms(&world), Some(first));
        assert!(
            !moved(&world),
            "one refused move only keeps the last good position"
        );
        let before = first + REFUSED_RESYNC_MS - 1;
        assert_eq!(
            world.move_to(&a, far, before),
            Err(MoveError::Refused(MoveRefusal::TooFast))
        );
        assert!(
            !moved(&world),
            "a short row of refused moves does not move the player"
        );
        let over = first + REFUSED_RESYNC_MS;
        assert_eq!(
            world.move_to(&a, far, over),
            Err(MoveError::Refused(MoveRefusal::TooFast))
        );
        assert!(
            moved(&world),
            "then the next own state moves the player back"
        );
        assert_eq!(refused_since_ms(&world), None, "the move-back ends the row");
        assert_eq!(
            world.move_to(&a, far, over + 1),
            Err(MoveError::Refused(MoveRefusal::TooFast))
        );
        assert_eq!(refused_since_ms(&world), Some(over + 1), "a new row starts");
        assert_eq!(
            world.move_to(&a, at(SPEED_MPS), over + REFUSED_RESYNC_MS),
            Ok(GroundCheck::NoGround)
        );
        assert_eq!(refused_since_ms(&world), None, "a good move ends the row");
    }

    #[test]
    fn an_admit_uses_the_given_entity_number() {
        let mut world = world();
        let login = world
            .admit(&admit_at(77, 0.0, true), NOW, SERVER_TIME)
            .expect("admit")
            .expect("login messages");
        assert_eq!(login.channel, MESSAGE_CHANNEL);
        assert!(login.reliable);
        assert_eq!(login.envelopes.len(), LOGIN_MESSAGE_TYPES.len());
        let outgoing = tick(&mut world, NOW + OWN_CARD_DELAY_MS).messages;
        assert_eq!(seen_by(&world, &outgoing, &connection(77)), vec![77]);
    }

    #[test]
    fn a_login_saved_in_the_air_arrives_on_the_ground() {
        const GROUND_M: f32 = 20.0;
        let mut world = SimWorld::new(
            settings(MANY, MAX_PLAYERS),
            WorldIds::random(0).expect("ids"),
            Arc::new(FlatGround(GROUND_M)),
            slot("sim-a"),
            test_combat(),
            Arc::default(),
        );
        let mut in_the_air = admit_at(1, 0.0, true);
        in_the_air.position.z = GROUND_M + MAX_ABOVE_GROUND_M + 1.0;
        world.admit(&in_the_air, NOW, SERVER_TIME).expect("admit");
        let departure = world.leave(&connection(1)).expect("left");
        assert_eq!(departure.position.z, GROUND_M);
    }

    #[test]
    fn a_resume_sends_no_login_messages() {
        let mut world = world();
        let login = world
            .admit(&admit_at(5, 0.0, false), NOW, SERVER_TIME)
            .expect("admit");
        assert_eq!(login, None);
        assert_eq!(world.players(), 1);
    }

    #[test]
    fn a_resume_of_a_player_that_is_here_keeps_it() {
        let mut world = world();
        let first = admit_at(1, 0.0, true);
        world.admit(&first, NOW, SERVER_TIME).expect("admit");
        world
            .move_to(&connection(1), at(SPEED_MPS), NOW + ONE_SECOND_MS)
            .expect("move");
        let resume = Admit {
            login: false,
            position: Point {
                x: 100.0,
                y: 10.0,
                z: 50.0,
            },
            ..first.clone()
        };
        assert_eq!(
            world.admit(&resume, NOW, SERVER_TIME).expect("resume"),
            None
        );
        assert_eq!(world.players(), 1);
        assert_eq!(world.positions()[0].position, at(SPEED_MPS));
        let other_entity = Admit {
            entity: 2,
            ..resume
        };
        assert!(world.admit(&other_entity, NOW, SERVER_TIME).is_err());
    }

    #[test]
    fn a_resumed_player_counts_its_handoffs_from_the_given_base() {
        let mut world = world();
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        let resumed = Admit {
            handoffs: RESUME_BASE,
            ..admit_at(1, BORDER_X + 50.0, false)
        };
        world.admit(&resumed, NOW, SERVER_TIME).expect("admit");
        let output = tick(&mut world, NOW);
        assert_eq!(output.handoffs[0].offer.handoffs, RESUME_BASE + 1);
    }

    #[test]
    fn a_resumed_player_goes_on_with_the_bundle_sequence() {
        let mut world = world();
        let resumed = Admit {
            bundles_sent: BUNDLES_BEFORE,
            ..admit_at(1, 0.0, false)
        };
        world.admit(&resumed, NOW, SERVER_TIME).expect("admit");
        let output = tick(&mut world, NOW);
        let bundle = bundle_of(&output.messages, &connection(1)).expect("bundle");
        assert_eq!(bundle.leading, Some(BUNDLES_BEFORE));
    }

    #[test]
    fn a_full_sim_refuses() {
        let mut world = world();
        for number in 1..=u64::from(MAX_PLAYERS) {
            join(&mut world, number, 0.0);
        }
        assert!(matches!(
            world.admit(&admit_at(9, 0.0, true), NOW, SERVER_TIME),
            Err(WorldError::Full)
        ));
    }

    #[test]
    fn players_see_themselves_first_and_others_inside_the_radius() {
        let mut world = world();
        let (a, b, c) = (connection(1), connection(2), connection(3));
        join(&mut world, 1, 0.0);
        join(&mut world, 2, 50.0);
        join(&mut world, 3, 500.0);
        let outgoing = tick(&mut world, NOW).messages;
        assert_eq!(seen_by(&world, &outgoing, &a), vec![1, 2]);
        assert_eq!(seen_by(&world, &outgoing, &b), vec![2, 1]);
        assert_eq!(seen_by(&world, &outgoing, &c), vec![3]);
        assert!(
            outgoing
                .iter()
                .all(|out| out.channel != REPLICATION_CHANNEL || !out.reliable)
        );
    }

    /// What every connection saw in one tick: its bundle members by entity number and x.
    fn seen_by_all(world: &SimWorld, outgoing: &[Outgoing]) -> BTreeMap<u64, Vec<(u64, f32)>> {
        outgoing
            .iter()
            .filter(|out| out.channel == REPLICATION_CHANNEL)
            .map(|out| {
                (
                    out.connection.connection,
                    positions_seen_by(world, outgoing, &out.connection),
                )
            })
            .collect()
    }

    #[test]
    fn the_bundle_step_on_many_threads_gives_the_same_bundles() {
        const PLAYERS: u64 = 300;
        const SPACING_M: f32 = 7.0;
        let crowd = |threads| {
            let mut world = world_of(
                "sim-a",
                WorldSettings {
                    tick_threads: threads,
                    ..settings(MANY, PLAYERS as u32)
                },
            );
            for number in 1..=PLAYERS {
                join(&mut world, number, number as f32 * SPACING_M);
            }
            let messages = tick(&mut world, NOW).messages;
            seen_by_all(&world, &messages)
        };
        let one = crowd(1);
        assert_eq!(one.len(), PLAYERS as usize);
        assert_eq!(crowd(4), one);
    }

    #[test]
    fn only_the_nearest_are_in_the_bundle() {
        let mut world = world_of("sim-a", settings(TWO, MAX_PLAYERS + 1));
        join(&mut world, 1, 0.0);
        join(&mut world, 2, 30.0);
        join(&mut world, 3, 10.0);
        join(&mut world, 4, 20.0);
        let outgoing = tick(&mut world, NOW).messages;
        assert_eq!(seen_by(&world, &outgoing, &connection(1)), vec![1, 3, 4]);
    }

    #[test]
    fn a_player_who_leaves_is_gone_from_the_next_bundles() {
        let mut world = world();
        let (a, b) = (connection(1), connection(2));
        let admit = admit_at(2, 10.0, true);
        join(&mut world, 1, 0.0);
        world.admit(&admit, NOW, SERVER_TIME).expect("b");
        let departure = world.leave(&b).expect("left");
        assert_eq!(departure.connection, b);
        assert_eq!(departure.character, admit.character);
        assert_eq!(departure.position, at(10.0));
        assert_eq!(departure.moved_at_ms, NOW);
        let outgoing = tick(&mut world, NOW).messages;
        assert_eq!(seen_by(&world, &outgoing, &a), vec![1]);
        assert_eq!(world.leave(&b), None);
    }

    #[test]
    fn a_connection_or_character_joins_once() {
        let mut world = world();
        let first = admit_at(1, 0.0, true);
        world.admit(&first, NOW, SERVER_TIME).expect("first");
        assert!(matches!(
            world.admit(&admit_at(1, 0.0, true), NOW, SERVER_TIME),
            Err(WorldError::AlreadyJoined(_))
        ));
        let same_character = Admit {
            connection: connection(2),
            entity: 2,
            ..first
        };
        assert!(matches!(
            world.admit(&same_character, NOW, SERVER_TIME),
            Err(WorldError::CharacterInWorld(_))
        ));
    }

    #[test]
    fn a_time_synch_goes_out_every_three_seconds() {
        let mut world = world();
        join(&mut world, 1, 0.0);
        let reliable =
            |output: TickOutput| output.messages.iter().filter(|out| out.reliable).count();
        assert_eq!(reliable(tick(&mut world, NOW + 1)), 0);
        assert_eq!(reliable(tick(&mut world, NOW + TIME_SYNCH_INTERVAL_MS)), 1);
        assert_eq!(
            reliable(tick(&mut world, NOW + TIME_SYNCH_INTERVAL_MS + 1)),
            0
        );
    }

    #[test]
    fn a_position_that_is_not_finite_cannot_join() {
        let mut world = world();
        assert!(matches!(
            world.admit(&admit_at(1, f32::NAN, true), NOW, SERVER_TIME),
            Err(WorldError::NotFinite)
        ));
        assert_eq!(world.players(), 0);
    }

    #[test]
    fn a_gateway_that_stops_takes_its_players_out() {
        let mut world = world();
        join(&mut world, 1, 0.0);
        join(&mut world, 2, 0.0);
        let gateway = BusId::try_from("gw-1".to_owned()).expect("id");
        let mut gone: Vec<u64> = world
            .leave_gateway(&gateway)
            .into_iter()
            .map(|departure| departure.connection.connection)
            .collect();
        gone.sort_unstable();
        assert_eq!(gone, vec![1, 2]);
        assert_eq!(world.players(), 0);
        assert_eq!(world.positions(), Vec::new());
    }

    #[test]
    fn many_visible_players_go_out_in_bundles_that_fit_one_datagram() {
        const PLAYERS: u64 = 100;
        let mut world = world_of("sim-a", settings(MANY, PLAYERS as u32));
        let viewer = connection(1);
        for number in 1..=PLAYERS {
            join(&mut world, number, 1.0);
        }
        let outgoing = tick(&mut world, NOW).messages;
        let bundles = outgoing
            .iter()
            .filter(|out| out.connection == viewer && out.channel == REPLICATION_CHANNEL)
            .count();
        assert!(bundles > 1);
        for out in outgoing
            .iter()
            .filter(|out| out.channel == REPLICATION_CHANNEL)
        {
            let payload = encode_server_messages(&[&out.envelopes[0]]).expect("payload");
            assert!(payload.len() <= MAX_CHUNK_PAYLOAD, "{}", payload.len());
        }
        // The largest bundle: the largest sequence, entity numbers and slots, every group with
        // its card, and as many stops as one bundle takes.
        let largest_number = u64::MAX >> LARGEST_NUMBER_SHIFT;
        let mut slots = Slots::restored(
            (0..MAX_BUNDLE_MEMBERS as u64)
                .map(|offset| (offset, u16::MAX - MAX_BUNDLE_MEMBERS as u16 + offset as u16))
                .collect(),
            Vec::new(),
        );
        slots.keep_only(&[]);
        let full: Vec<BundleGroup> = (0..MAX_BUNDLE_MEMBERS as u64)
            .filter_map(|offset| {
                remote_group(
                    u16::MAX - offset as u16,
                    largest_number - offset,
                    EntityKind::PLAYER,
                    at(0.0),
                    true,
                )
                .expect("encode")
            })
            .collect();
        // The largest sequence that leaves room for the next bundles of this tick.
        let sequence = largest_number - MAX_BUNDLE_MEMBERS as u64;
        let envelopes =
            members::bundles(sequence, LOGIN_CLIENT_CONTEXT, &mut slots, full).expect("bundles");
        for envelope in &envelopes {
            let payload = encode_server_messages(&[envelope]).expect("payload");
            assert!(payload.len() <= MAX_CHUNK_PAYLOAD, "{}", payload.len());
        }
    }

    /// A world of the first of two sims, with a player 50 m inside the second sim's cells and
    /// a viewer near it on this side.
    fn border_world() -> (SimWorld, ConnectionRef, ConnectionRef) {
        let mut world = world();
        assert!(
            world
                .set_table(&table(&["sim-a", "sim-b"]))
                .expect("table")
                .is_empty()
        );
        join(&mut world, 1, BORDER_X + 50.0);
        join(&mut world, 2, BORDER_X - 20.0);
        (world, connection(1), connection(2))
    }

    #[test]
    fn a_player_past_the_margin_is_handed_off_and_frozen() {
        let (mut world, mover, viewer) = border_world();
        let first = tick(&mut world, NOW);
        assert_eq!(first.handoffs.len(), 1);
        let start = &first.handoffs[0];
        assert_eq!(start.target, slot("sim-b"));
        assert_eq!(start.offer.connection, mover);
        assert_eq!(start.offer.entity, 1);
        assert_eq!(start.offer.handoffs, 1);
        assert_eq!(start.offer.position.x, BORDER_X + 50.0);
        assert!(bundle_of(&first.messages, &mover).is_none(), "frozen");
        assert_eq!(seen_by(&world, &first.messages, &viewer), vec![2, 1]);
        assert_eq!(
            world.move_to(&mover, at(BORDER_X + 51.0), NOW + ONE_SECOND_MS),
            Err(MoveError::Frozen)
        );
        assert!(tick(&mut world, NOW + 1).handoffs.is_empty());
        let near = world_with_one_near_the_border();
        assert!(near.is_empty());
    }

    /// A player 3 m past the border (inside the margin) is not handed off.
    fn world_with_one_near_the_border() -> Vec<HandoffStart> {
        let mut world = world();
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, BORDER_X + MARGIN_M - 2.0);
        tick(&mut world, NOW).handoffs
    }

    #[test]
    fn an_answered_handoff_moves_or_keeps_the_player() {
        let (mut world, mover, _) = border_world();
        tick(&mut world, NOW);
        assert_eq!(world.handoff_done(&mover, false, NOW), HandoffEnd::Kept);
        assert_eq!(
            world.move_to(&mover, at(BORDER_X + 51.0), NOW + A_BIT_LATER_MS),
            Ok(GroundCheck::NoGround)
        );
        assert!(tick(&mut world, NOW + A_BIT_LATER_MS).handoffs.is_empty());
        let again = tick(&mut world, NOW + RETRY_MS);
        assert_eq!(again.handoffs.len(), 1);
        assert_eq!(
            world.handoff_done(&mover, true, NOW + RETRY_MS),
            HandoffEnd::Moved
        );
        assert_eq!(world.players(), 1);
        assert_eq!(world.handoff_done(&mover, true, NOW), HandoffEnd::Gone);
    }

    #[test]
    fn a_taken_player_keeps_its_number_sequence_and_budget() {
        let (mut old, mover, _) = border_world();
        tick(&mut old, NOW);
        let mut offer = tick_offer(&mut old);
        assert_eq!(offer.name, player_name(1), "the name crosses");
        // The client tied a slot to the viewer before the handoff.
        offer.slots = vec![(2, slots::FIRST_REMOTE_INTEREST)];
        offer.bundles_sent = BUNDLES_BEFORE;
        let mut new = world_of("sim-b", settings(MANY, MAX_PLAYERS));
        new.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        new.take(&offer, NOW, SERVER_TIME).expect("take");
        assert_eq!(
            new.positions()[0].moved_at_ms,
            NOW - offer.move_age_ms,
            "the move time crosses as an age"
        );
        assert_eq!(new.snapshot().0[0].name, offer.name);
        let output = tick(&mut new, NOW);
        let bundle = bundle_of(&output.messages, &mover).expect("bundle");
        assert_eq!(bundle.leading, Some(BUNDLES_BEFORE));
        assert_eq!(
            bundle.index_block.map(|block| block.indexes),
            Some(vec![slots::FIRST_REMOTE_INTEREST]),
            "the taker does not see the viewer, so it stops the viewer's slot"
        );
        assert_eq!(seen_by(&new, &output.messages, &mover), vec![1]);
        let reach = BORDER_X + 50.0 + offer.budget_m;
        assert_eq!(
            new.move_to(&mover, at(reach + 1.0), NOW),
            Err(MoveError::Refused(MoveRefusal::TooFast))
        );
        assert_eq!(
            new.move_to(&mover, at(reach - 0.5), NOW),
            Ok(GroundCheck::NoGround)
        );
        assert!(matches!(
            new.take(&offer, NOW, SERVER_TIME),
            Err(WorldError::AlreadyJoined(_))
        ));
        assert!(
            !new.cancel(&mover, offer.handoffs + 1),
            "a cancel of another attempt"
        );
        assert!(new.cancel(&mover, offer.handoffs));
        assert_eq!(new.players(), 0);
        assert!(!new.cancel(&mover, offer.handoffs));
    }

    /// The offer of the frozen player of a border world.
    fn tick_offer(world: &mut SimWorld) -> HandoffOffer {
        let mover = connection(1);
        world.handoff_done(&mover, false, NOW);
        tick(world, NOW + RETRY_MS).handoffs[0].offer.clone()
    }

    #[test]
    fn ghosts_are_seen_and_expire() {
        let mut world = world();
        join(&mut world, 1, 0.0);
        let ghost = |entity, x| Ghost {
            entity,
            position: Point {
                x,
                y: 10.0,
                z: 50.0,
            },
            mob: None,
        };
        world.set_ghosts(
            &slot("sim-b").id,
            vec![
                ghost(GHOST_NUMBER, 10.0),
                ghost(1, 20.0),
                ghost(7, f32::NAN),
            ],
            NOW,
        );
        let outgoing = tick(&mut world, NOW).messages;
        assert_eq!(
            positions_seen_by(&world, &outgoing, &connection(1)),
            vec![(1, 0.0), (GHOST_NUMBER, 10.0)]
        );
        let later = tick(&mut world, NOW + GHOST_TTL_MS + 1).messages;
        assert_eq!(seen_by(&world, &later, &connection(1)), vec![1]);
    }

    #[test]
    fn players_near_a_border_become_ghosts_of_the_neighbour() {
        let mut world = world();
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, BORDER_X - 50.0);
        join(&mut world, 2, 100.0);
        let ghosts = tick(&mut world, NOW).ghosts;
        assert_eq!(ghosts.len(), 1);
        let (target, entities) = &ghosts[0];
        assert_eq!(target, &slot("sim-b").id);
        let numbers: Vec<u64> = entities.iter().map(|ghost| ghost.entity).collect();
        assert_eq!(numbers, vec![1]);
    }

    #[test]
    fn an_empty_ghost_list_follows_a_full_one() {
        let mut world = world();
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, BORDER_X - 50.0);
        assert_eq!(tick(&mut world, NOW).ghosts.len(), 1);
        world.leave(&connection(1));
        let cleared = tick(&mut world, NOW + 1).ghosts;
        assert_eq!(cleared, vec![(slot("sim-b").id, Vec::new())]);
        assert!(tick(&mut world, NOW + 2).ghosts.is_empty());
    }

    #[test]
    fn a_sim_missing_from_the_table_drops_its_players() {
        let mut world = world();
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        join(&mut world, 1, 0.0);
        join(&mut world, 2, 0.0);
        let mut restarted = table(&["sim-a", "sim-b"]);
        restarted.sims[0].generation = GENERATION + 1;
        assert_eq!(world.set_table(&restarted).expect("table").len(), 2);
        assert_eq!(world.players(), 0);
        join(&mut world, 3, BORDER_X + 50.0);
        assert!(
            tick(&mut world, NOW).handoffs.is_empty(),
            "no table, no handoff"
        );
    }

    #[test]
    fn a_table_of_another_size_is_ignored() {
        let mut world = world();
        let mut wrong = table(&["sim-a", "sim-b"]);
        wrong.owners.pop();
        assert_eq!(world.set_table(&wrong), Err(TableError::WrongSize));
        join(&mut world, 1, BORDER_X + 50.0);
        assert!(tick(&mut world, NOW).handoffs.is_empty());
    }
}
