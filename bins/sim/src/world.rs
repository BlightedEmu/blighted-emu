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
use nw_bus::{Admit, BusId, CellTable, ConnectionRef, Ghost, HandoffOffer, Point, SimSlot};
use nw_messages::{
    MessageError, ReplicatedStateBundle, ServerMessage, TimeSynchMsg, WorldPosition,
};
use nw_sim_core::{
    CellBounds, CellGrid, CellOwners, Ground, GroundCheck, Interest, MobState, Motion, MotionState,
    MoveLimits, MoveRefusal, Position, Seen, SpatialGrid,
};
use thiserror::Error;
use uuid::Uuid;

use crate::combat::CombatSettings;
use crate::login::{SessionIds, WorldIds, login_messages};
use crate::mobs::{MobAttackIntent, MobChanges, Mobs, spawn_index};

/// Carrier channel of the login messages and the time messages.
pub const MESSAGE_CHANNEL: u8 = 0;
/// Carrier channel of the replication bundles.
pub const REPLICATION_CHANNEL: u8 = 1;
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
    /// Bundles and time messages.
    pub messages: Vec<Outgoing>,
    /// Handoffs to ask for.
    pub handoffs: Vec<HandoffStart>,
    /// Ghost lists for other sims; an empty list clears the receiver's last one.
    pub ghosts: Vec<(BusId, Vec<Ghost>)>,
    /// Hits that mobs make this tick.
    pub mob_attacks: Vec<MobAttackIntent>,
    /// Time of the mob part of the tick, in seconds.
    pub mob_seconds: f64,
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
    number: u64,
    bundles_sent: u64,
    last_time_synch_ms: u64,
    interest: Interest<u64>,
    frozen: bool,
    handoffs: u64,
    no_handoff_before_ms: u64,
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
    entities: Vec<(u64, Position)>,
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
    ghosts: HashMap<BusId, ReceivedGhosts>,
    ghost_targets: BTreeSet<BusId>,
    mobs: Mobs,
    mob_changes: MobChanges,
}

/// A new player before it is in the world.
struct Arrival {
    connection: ConnectionRef,
    character: Uuid,
    number: u64,
    position: Position,
}

impl SimWorld {
    /// An empty world of the sim `me` on the ground of `ground`, with the mobs of `combat`.
    pub fn new(
        settings: WorldSettings,
        ids: WorldIds,
        ground: Arc<dyn Ground + Send + Sync>,
        me: SimSlot,
        combat: CombatSettings,
    ) -> Self {
        Self {
            mobs: Mobs::new(Arc::clone(&combat.spawns), &combat),
            mob_changes: MobChanges::default(),
            world: World::new(),
            by_connection: HashMap::new(),
            settings,
            ids,
            ground,
            me,
            cells: None,
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
        let arrival = Arrival {
            connection: admit.connection.clone(),
            character: admit.character,
            number: admit.entity,
            position: Position::from(admit.position),
        };
        self.check(&arrival)?;
        let login = if admit.login {
            let envelopes =
                login_messages(&self.ids, SessionIds::random()?, admit.entity, server_time)?;
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
        self.spawn(
            arrival,
            motion,
            admit.bundles_sent,
            now_ms,
            admit.handoffs,
            now_ms,
        );
        Ok(login)
    }

    /// Takes the player of `offer` from another sim, with the same entity number, bundle
    /// sequence and movement budget.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError`] as [`SimWorld::admit`] does.
    pub fn take(&mut self, offer: &HandoffOffer, now_ms: u64) -> Result<(), WorldError> {
        let arrival = Arrival {
            connection: offer.connection.clone(),
            character: offer.character,
            number: offer.entity,
            position: Position::from(offer.position),
        };
        self.check(&arrival)?;
        let state = MotionState {
            position: arrival.position,
            move_age_ms: offer.move_age_ms,
            budget_m: offer.budget_m,
        };
        let motion = Motion::import(state, now_ms, &self.settings.limits);
        let retry_at = now_ms.saturating_add(self.settings.handoff_retry_ms);
        self.spawn(
            arrival,
            motion,
            offer.bundles_sent,
            now_ms.saturating_sub(offer.time_synch_age_ms),
            offer.handoffs,
            retry_at,
        );
        Ok(())
    }

    /// Moves the player of `connection` to `next`, received at `now_ms`, when the movement checks
    /// (M4 spec section 7.2 with sections 15 and 16) allow it.
    ///
    /// # Errors
    ///
    /// Returns [`MoveError`] when there is no such player, it is frozen, or a check refuses the
    /// move; then the player keeps its last good position, which its next bundle shows.
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
        let mut body = self
            .world
            .get_mut::<Body>(entity)
            .ok_or(MoveError::NotInWorld)?;
        body.motion
            .try_move(
                &self.settings.limits,
                &*self.ground,
                &self.settings.bounds,
                next,
                now_ms,
            )
            .map_err(MoveError::Refused)
    }

    /// Removes a player.
    pub fn leave(&mut self, connection: &ConnectionRef) -> Option<Departure> {
        let entity = self.by_connection.remove(connection)?;
        let departure = self.departure(entity);
        self.world.despawn(entity);
        departure
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
            self.by_connection.remove(connection);
            self.world.despawn(entity);
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
            self.by_connection.remove(connection);
            self.world.despawn(entity);
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
            .map(|ghost| (ghost.entity, Position::from(ghost.position)))
            .collect();
        self.ghosts.insert(
            from.clone(),
            ReceivedGhosts {
                received_ms: now_ms,
                entities,
            },
        );
    }

    /// What the cell tables did to the mobs since the last call.
    pub fn take_mob_changes(&mut self) -> MobChanges {
        std::mem::take(&mut self.mob_changes)
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
        let mob_attacks = self.tick_mobs(now_ms);
        let mob_seconds = mobs_started.elapsed().as_secs_f64();
        let messages = self.bundles(now_ms, server_time)?;
        Ok(TickOutput {
            messages,
            handoffs,
            ghosts,
            mob_attacks,
            mob_seconds,
        })
    }

    /// Runs the mobs with the players of this sim and the player ghosts of others.
    fn tick_mobs(&mut self, now_ms: u64) -> Vec<MobAttackIntent> {
        let mut seen: Vec<Seen> = Vec::new();
        let mut query = self.world.query::<(&Player, &Body)>();
        for (player, body) in query.iter(&self.world) {
            seen.push(Seen {
                entity: player.number,
                position: body.motion.position(),
            });
        }
        let ghosts = self.ghosts.values().flat_map(|ghosts| &ghosts.entities);
        for (entity, position) in ghosts {
            if spawn_index(*entity).is_none() {
                seen.push(Seen {
                    entity: *entity,
                    position: *position,
                });
            }
        }
        self.mobs.tick(&seen, now_ms, &*self.ground)
    }

    fn start_handoffs(&mut self, now_ms: u64) -> Vec<HandoffStart> {
        let Some(cells) = &self.cells else {
            return Vec::new();
        };
        let margin = self.settings.handoff_margin_m;
        let mut query = self.world.query::<(&mut Player, &Body)>();
        let mut starts = Vec::new();
        for (mut player, body) in query.iter_mut(&mut self.world) {
            if player.frozen || now_ms < player.no_handoff_before_ms {
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
            let state = body.motion.export(now_ms);
            starts.push(HandoffStart {
                target: target.clone(),
                offer: HandoffOffer {
                    connection: player.connection.clone(),
                    character: player.character,
                    entity: player.number,
                    position: Point::from(state.position),
                    move_age_ms: state.move_age_ms,
                    budget_m: state.budget_m,
                    bundles_sent: player.bundles_sent,
                    time_synch_age_ms: now_ms.saturating_sub(player.last_time_synch_ms),
                    handoffs: player.handoffs.saturating_add(1),
                },
            });
        }
        starts
    }

    fn ghosts_out(&mut self) -> Vec<(BusId, Vec<Ghost>)> {
        let mut lists: BTreeMap<BusId, Vec<Ghost>> = BTreeMap::new();
        if let Some(cells) = &self.cells {
            let radius = self.settings.interest_radius_m;
            let mut query = self.world.query::<(&Player, &Body)>();
            for (player, body) in query.iter(&self.world) {
                let position = body.motion.position();
                for owner in cells.owners.others_within(cells.me, position, radius) {
                    if let Some(sim) = cells.sims.get(usize::from(owner)) {
                        lists.entry(sim.id.clone()).or_default().push(Ghost {
                            entity: player.number,
                            position: Point::from(position),
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
    fn bundles(&mut self, now_ms: u64, server_time: u64) -> Result<Vec<Outgoing>, MessageError> {
        let mut bodies = self.world.query::<(&Player, &Body)>();
        let mut everyone: Vec<(u64, Position)> = bodies
            .iter(&self.world)
            .map(|(player, body)| (player.number, body.motion.position()))
            .collect();
        let own: HashSet<u64> = everyone.iter().map(|(number, _)| *number).collect();
        everyone.extend(
            self.ghosts
                .values()
                .flat_map(|ghosts| ghosts.entities.iter().copied())
                .filter(|(number, _)| !own.contains(number)),
        );
        // The radius is checked at start to be a finite number above 0.
        let Some(mut grid) = SpatialGrid::new(self.settings.interest_radius_m) else {
            return Ok(Vec::new());
        };
        for (number, position) in &everyone {
            grid.insert(*number, *position);
        }
        let near = Neighbourhood {
            grid: &grid,
            positions: everyone.iter().copied().collect(),
            now_ms,
            server_time,
        };
        let mut query = self.world.query::<(&mut Player, &Body)>();
        let mut players: Vec<(Mut<Player>, &Body)> = query.iter_mut(&mut self.world).collect();
        let threads = self
            .settings
            .tick_threads
            .min(players.len() / MIN_PLAYERS_PER_THREAD)
            .max(1);
        if threads == 1 {
            return near.bundles_of(&mut players);
        }
        let chunk = players.len().div_ceil(threads);
        let parts: Vec<Result<Vec<Outgoing>, MessageError>> = std::thread::scope(|scope| {
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
        let mut outgoing = Vec::new();
        for part in parts {
            outgoing.extend(part?);
        }
        Ok(outgoing)
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
        bundles_sent: u64,
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
                    number: arrival.number,
                    bundles_sent,
                    last_time_synch_ms,
                    interest: Interest::new(
                        self.settings.interest_radius_m,
                        self.settings.max_visible_players,
                    ),
                    frozen: false,
                    handoffs,
                    no_handoff_before_ms,
                },
                Body { motion },
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

fn departure_of(player: &Player, body: &Body) -> Departure {
    Departure {
        connection: player.connection.clone(),
        character: player.character,
        position: body.motion.position(),
        moved_at_ms: body.motion.moved_ms(),
    }
}

/// What the bundle step of one tick reads: everyone's position, in a grid.
struct Neighbourhood<'a> {
    grid: &'a SpatialGrid<u64>,
    positions: HashMap<u64, Position>,
    now_ms: u64,
    server_time: u64,
}

impl Neighbourhood<'_> {
    /// The bundles and time messages of `players`, in their order.
    fn bundles_of(
        &self,
        players: &mut [(Mut<Player>, &Body)],
    ) -> Result<Vec<Outgoing>, MessageError> {
        let mut outgoing = Vec::new();
        for (player, body) in players.iter_mut() {
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
            let mut numbers: Vec<u64> = seen.entered.into_iter().chain(seen.stayed).collect();
            numbers.sort_unstable();
            let mut visible: Vec<(u64, Position)> = vec![(own, own_position)];
            visible.extend(
                numbers
                    .into_iter()
                    .filter_map(|number| Some((number, *self.positions.get(&number)?))),
            );
            for members in visible.chunks(MAX_BUNDLE_MEMBERS) {
                let bundle = bundle(player.bundles_sent, members)?;
                player.bundles_sent = player.bundles_sent.saturating_add(1);
                outgoing.push(Outgoing {
                    connection: player.connection.clone(),
                    channel: REPLICATION_CHANNEL,
                    reliable: false,
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
                        ServerMessage::TimeSynch(TimeSynchMsg {
                            time: self.server_time,
                        })
                        .envelope()?,
                    ],
                });
            }
        }
        Ok(outgoing)
    }
}

/// One bundle envelope ([`ReplicatedStateBundle::of_positions`]). Every position in the world is
/// finite (`admit`, `take` and `set_ghosts` refuse others), so no member is left out.
fn bundle(sequence: u64, visible: &[(u64, Position)]) -> Result<Vec<u8>, MessageError> {
    let positions: Vec<(u64, WorldPosition)> = visible
        .iter()
        .filter_map(|(number, position)| {
            Some((
                *number,
                WorldPosition::from_metres(position.x, position.y, position.z)?,
            ))
        })
        .collect();
    ServerMessage::Bundle(ReplicatedStateBundle::of_positions(sequence, &positions)?).envelope()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use nw_bus::CellTable;
    use nw_carrier::sender::MAX_CHUNK_PAYLOAD;
    use nw_messages::{BundlePayload, LOGIN_MESSAGE_TYPES, MemberState};
    use nw_reflect::{Envelope, encode_server_messages};
    use nw_sim_core::test_support::NoGround;
    use nw_sim_core::{CellGrid, CellOwners, GroundCheck, MoveLimits, MoveRefusal};

    use super::*;
    use crate::combat::tests::{TEST_LEVEL, TEST_MOB, test_combat};
    use crate::mobs::VitalsSpawn;

    /// The largest entity number that fits the 5-byte prefix code (35 bits).
    const LARGEST_NUMBER_SHIFT: u32 = 29;

    const RADIUS: f32 = 100.0;
    const NOW: u64 = 10_000;
    const SERVER_TIME: u64 = 7;
    /// 5 m/s with no margin and a burst of 1 s: at most 5 m in one move.
    const SPEED_MPS: f32 = 5.0;
    const BURST_SECONDS: f32 = 1.0;
    const GROUND_TOLERANCE_M: f32 = 2.0;
    const MAX_ABOVE_GROUND_M: f32 = 150.0;
    const LEVEL_M: f32 = 14_336.0;
    const CELL_M: f32 = 2048.0;
    /// The first column of the second sim of two (columns 3 to 6).
    const BORDER_X: f32 = 3.0 * CELL_M;
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

    fn slot(name: &str) -> SimSlot {
        SimSlot {
            id: BusId::try_from(name.to_owned()).expect("id"),
            generation: GENERATION,
        }
    }

    fn settings(max_visible_players: usize, max_players: u32) -> WorldSettings {
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
        }
    }

    fn world_of(name: &str, settings: WorldSettings) -> SimWorld {
        SimWorld::new(
            settings,
            WorldIds::random(0).expect("ids"),
            Arc::new(NoGround),
            slot(name),
            test_combat(),
        )
    }

    fn world() -> SimWorld {
        world_of("sim-a", settings(MANY, MAX_PLAYERS))
    }

    /// A spawn of the test mob type at (`x`, 10).
    fn test_spawn(x: f32) -> VitalsSpawn {
        VitalsSpawn {
            vitals_id: nw_formats::objectstream::lowercase_crc32(TEST_MOB),
            level: TEST_LEVEL,
            position: [x, 10.0, 50.0],
            ..VitalsSpawn::default()
        }
    }

    /// A world of `sim-a` with the test mob spawns `spawns`.
    fn world_with_spawns(spawns: Vec<VitalsSpawn>) -> SimWorld {
        let mut combat = test_combat();
        combat.spawns = spawns.into();
        SimWorld::new(
            settings(MANY, MAX_PLAYERS),
            WorldIds::random(0).expect("ids"),
            Arc::new(NoGround),
            slot("sim-a"),
            combat,
        )
    }

    fn grid() -> CellGrid {
        CellGrid::new(LEVEL_M, LEVEL_M, CELL_M).expect("grid")
    }

    /// The table of `names` in column stripes.
    fn table(names: &[&str]) -> CellTable {
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

    fn connection(number: u64) -> ConnectionRef {
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

    fn admit_at(number: u64, x: f32, login: bool) -> Admit {
        Admit {
            connection: connection(number),
            account: Uuid::new_v4(),
            character: Uuid::new_v4(),
            entity: number,
            position: Point {
                x,
                y: 10.0,
                z: 50.0,
            },
            login,
            handoffs: 0,
            bundles_sent: 0,
        }
    }

    fn join(world: &mut SimWorld, number: u64, x: f32) {
        world
            .admit(&admit_at(number, x, true), NOW, SERVER_TIME)
            .expect("admit");
    }

    /// The entity numbers in the bundle of one connection.
    fn seen_by(outgoing: &[Outgoing], who: &ConnectionRef) -> Vec<u64> {
        positions_seen_by(outgoing, who)
            .into_iter()
            .map(|(number, _)| number)
            .collect()
    }

    fn bundle_of(outgoing: &[Outgoing], who: &ConnectionRef) -> Option<ReplicatedStateBundle> {
        let message = outgoing
            .iter()
            .find(|out| out.connection == *who && out.channel == REPLICATION_CHANNEL)?;
        let envelope = Envelope::decode(&message.envelopes[0]).expect("envelope");
        let Some(Ok(ServerMessage::Bundle(bundle))) = ServerMessage::from_envelope(&envelope)
        else {
            panic!("expected a bundle");
        };
        Some(bundle)
    }

    /// The entity numbers and x positions in the bundle of one connection.
    fn positions_seen_by(outgoing: &[Outgoing], who: &ConnectionRef) -> Vec<(u64, f32)> {
        let bundle = bundle_of(outgoing, who).expect("bundle");
        BundlePayload::decode(&bundle.payload)
            .expect("payload")
            .groups
            .iter()
            .flat_map(|group| {
                group.members.iter().map(|member| {
                    let MemberState::Position(state) = &member.state;
                    (member.u64_at_0, state.position.expect("position").x)
                })
            })
            .collect()
    }

    fn tick(world: &mut SimWorld, now: u64) -> TickOutput {
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
        let (_, seen_x) = positions_seen_by(&outgoing, &b)[1];
        assert!((seen_x - SPEED_MPS).abs() < CLOSE_M);
        let far = at(SPEED_MPS * 10.0);
        let later = NOW + ONE_SECOND_MS + A_BIT_LATER_MS;
        assert_eq!(
            world.move_to(&a, far, later),
            Err(MoveError::Refused(MoveRefusal::TooFast))
        );
        let outgoing = tick(&mut world, later).messages;
        let (_, own_x) = positions_seen_by(&outgoing, &a)[0];
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
    fn an_admit_uses_the_given_entity_number() {
        let mut world = world();
        let login = world
            .admit(&admit_at(77, 0.0, true), NOW, SERVER_TIME)
            .expect("admit")
            .expect("login messages");
        assert_eq!(login.channel, MESSAGE_CHANNEL);
        assert!(login.reliable);
        assert_eq!(login.envelopes.len(), LOGIN_MESSAGE_TYPES.len());
        let outgoing = tick(&mut world, NOW).messages;
        assert_eq!(seen_by(&outgoing, &connection(77)), vec![77]);
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
        assert_eq!(seen_by(&outgoing, &a), vec![1, 2]);
        assert_eq!(seen_by(&outgoing, &b), vec![2, 1]);
        assert_eq!(seen_by(&outgoing, &c), vec![3]);
        assert!(
            outgoing
                .iter()
                .all(|out| out.channel != REPLICATION_CHANNEL || !out.reliable)
        );
    }

    /// What every connection saw in one tick: its bundle members by entity number and x.
    fn seen_by_all(outgoing: &[Outgoing]) -> BTreeMap<u64, Vec<(u64, f32)>> {
        outgoing
            .iter()
            .filter(|out| out.channel == REPLICATION_CHANNEL)
            .map(|out| {
                (
                    out.connection.connection,
                    positions_seen_by(outgoing, &out.connection),
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
            seen_by_all(&tick(&mut world, NOW).messages)
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
        assert_eq!(seen_by(&outgoing, &connection(1)), vec![1, 3, 4]);
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
        assert_eq!(seen_by(&outgoing, &a), vec![1]);
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
        assert_eq!(bundles, (PLAYERS as usize).div_ceil(MAX_BUNDLE_MEMBERS));
        let largest_number = u64::MAX >> LARGEST_NUMBER_SHIFT;
        let full: Vec<(u64, Position)> = (0..MAX_BUNDLE_MEMBERS as u64)
            .map(|offset| (largest_number - offset, at(0.0)))
            .collect();
        let envelope = bundle(u64::MAX >> LARGEST_NUMBER_SHIFT, &full).expect("bundle");
        let payload = encode_server_messages(&[&envelope]).expect("payload");
        assert!(payload.len() <= MAX_CHUNK_PAYLOAD, "{}", payload.len());
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
        assert_eq!(seen_by(&first.messages, &viewer), vec![2, 1]);
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
        offer.bundles_sent = BUNDLES_BEFORE;
        let mut new = world_of("sim-b", settings(MANY, MAX_PLAYERS));
        new.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        new.take(&offer, NOW).expect("take");
        assert_eq!(
            new.positions()[0].moved_at_ms,
            NOW - offer.move_age_ms,
            "the move time crosses as an age"
        );
        let output = tick(&mut new, NOW);
        let bundle = bundle_of(&output.messages, &mover).expect("bundle");
        assert_eq!(bundle.leading, Some(BUNDLES_BEFORE));
        assert_eq!(seen_by(&output.messages, &mover), vec![1]);
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
            new.take(&offer, NOW),
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
            positions_seen_by(&outgoing, &connection(1)),
            vec![(1, 0.0), (GHOST_NUMBER, 10.0)]
        );
        let later = tick(&mut world, NOW + GHOST_TTL_MS + 1).messages;
        assert_eq!(seen_by(&later, &connection(1)), vec![1]);
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
