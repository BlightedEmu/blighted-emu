// SPDX-License-Identifier: AGPL-3.0-or-later
//! The world state of `sim` (M4 spec sections 3.1, 7.3), in a `bevy_ecs` world, with no I/O: the
//! caller passes times in and sends the [`Outgoing`] messages.
//!
//! Every tick, each player gets replication bundles on channel 1, unreliable, as in the captures:
//! the player's own position first, then the position of every player inside the interest radius,
//! by entity number, at most [`MAX_BUNDLE_MEMBERS`] members per bundle and one group per bundle. The full position goes out every tick, so a lost
//! datagram costs nothing. How the retail service removes an entity from a client is Unknown, so a
//! player who leaves the radius is simply not in the next bundles.

use std::collections::HashMap;
use std::sync::Arc;

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use nw_bus::{BusId, ConnectionRef};
use nw_messages::{
    MessageError, ReplicatedStateBundle, ServerMessage, TimeSynchMsg, WorldPosition,
};
use nw_sim_core::{
    CellBounds, Ground, GroundCheck, Interest, Motion, MoveLimits, MoveRefusal, Position,
};
use thiserror::Error;
use uuid::Uuid;

use crate::login::{SessionIds, WorldIds, login_messages};

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
    /// The movement limits.
    pub limits: MoveLimits,
    /// The area of the cell.
    pub bounds: CellBounds,
}

/// Why a move did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MoveError {
    /// The connection has no player in the world.
    #[error("no player on this connection")]
    NotInWorld,
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
    /// No random bytes for the session ids.
    #[error("no random bytes: {0}")]
    Random(#[from] getrandom::Error),
    /// A message cannot be encoded.
    #[error(transparent)]
    Message(#[from] MessageError),
}

#[derive(Component)]
struct Player {
    connection: ConnectionRef,
    character: Uuid,
    number: u64,
    bundles_sent: u64,
    last_time_synch_ms: u64,
    interest: Interest<u64>,
}

#[derive(Component, Clone, Copy)]
struct Body {
    motion: Motion,
}

/// The players of one cell.
pub struct SimWorld {
    world: World,
    by_connection: HashMap<ConnectionRef, Entity>,
    next_number: u64,
    settings: WorldSettings,
    ids: WorldIds,
    ground: Arc<dyn Ground + Send + Sync>,
}

impl SimWorld {
    /// An empty world on the ground of `ground`.
    pub fn new(
        settings: WorldSettings,
        ids: WorldIds,
        ground: Arc<dyn Ground + Send + Sync>,
    ) -> Self {
        Self {
            world: World::new(),
            by_connection: HashMap::new(),
            next_number: 1,
            settings,
            ids,
            ground,
        }
    }

    /// Players in the world.
    pub fn players(&self) -> usize {
        self.by_connection.len()
    }

    /// Adds a player at `position` and returns its login messages.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError`] when the connection or the character is already in the world.
    pub fn join(
        &mut self,
        connection: ConnectionRef,
        character: Uuid,
        position: Position,
        now_ms: u64,
        server_time: u64,
    ) -> Result<Outgoing, WorldError> {
        if self.by_connection.contains_key(&connection) {
            return Err(WorldError::AlreadyJoined(connection));
        }
        if self.player_of(character).is_some() {
            return Err(WorldError::CharacterInWorld(character));
        }
        if WorldPosition::from_metres(position.x, position.y, position.z).is_none() {
            return Err(WorldError::NotFinite);
        }
        let number = self.next_number;
        let envelopes = login_messages(&self.ids, SessionIds::random()?, number, server_time)?;
        self.next_number += 1;
        let entity = self
            .world
            .spawn((
                Player {
                    connection: connection.clone(),
                    character,
                    number,
                    bundles_sent: 0,
                    last_time_synch_ms: now_ms,
                    interest: Interest::new(self.settings.interest_radius_m, usize::MAX),
                },
                Body {
                    motion: Motion::new(position, now_ms, &self.settings.limits),
                },
            ))
            .id();
        self.by_connection.insert(connection.clone(), entity);
        Ok(Outgoing {
            connection,
            channel: MESSAGE_CHANNEL,
            reliable: true,
            envelopes,
        })
    }

    /// Moves the player of `connection` to `next`, received at `now_ms`, when the movement checks
    /// (M4 spec section 7.2 with sections 15 and 16) allow it.
    ///
    /// # Errors
    ///
    /// Returns [`MoveError`] when there is no such player or a check refuses the move; then the
    /// player keeps its last good position, which its next bundle shows.
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
        connections
            .iter()
            .filter_map(|connection| self.leave(connection))
            .collect()
    }

    /// Every player's character and position, for the periodic save.
    pub fn positions(&mut self) -> Vec<Departure> {
        let mut query = self.world.query::<(&Player, &Body)>();
        query
            .iter(&self.world)
            .map(|(player, body)| Departure {
                connection: player.connection.clone(),
                character: player.character,
                position: body.motion.position(),
            })
            .collect()
    }

    /// One tick: a bundle for every player, and a `TimeSynchMsg` when one is due.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a message cannot be encoded.
    pub fn tick(&mut self, now_ms: u64, server_time: u64) -> Result<Vec<Outgoing>, MessageError> {
        let mut bodies = self.world.query::<(&Player, &Body)>();
        let everyone: Vec<(u64, Position)> = bodies
            .iter(&self.world)
            .map(|(player, body)| (player.number, body.motion.position()))
            .collect();
        let positions: HashMap<u64, Position> = everyone.iter().copied().collect();
        let mut players = self.world.query::<(&mut Player, &Body)>();
        let mut outgoing = Vec::new();
        for (mut player, body) in players.iter_mut(&mut self.world) {
            let own = player.number;
            let others = everyone
                .iter()
                .copied()
                .filter(|(number, _)| *number != own);
            let own_position = body.motion.position();
            let seen = player.interest.update(own_position, others);
            let mut numbers: Vec<u64> = seen.entered.into_iter().chain(seen.stayed).collect();
            numbers.sort_unstable();
            let mut visible: Vec<(u64, Position)> = vec![(own, own_position)];
            visible.extend(
                numbers
                    .into_iter()
                    .filter_map(|number| Some((number, *positions.get(&number)?))),
            );
            for members in visible.chunks(MAX_BUNDLE_MEMBERS) {
                let bundle = bundle(player.bundles_sent, members)?;
                player.bundles_sent += 1;
                outgoing.push(Outgoing {
                    connection: player.connection.clone(),
                    channel: REPLICATION_CHANNEL,
                    reliable: false,
                    envelopes: vec![bundle],
                });
            }
            if now_ms.saturating_sub(player.last_time_synch_ms) >= TIME_SYNCH_INTERVAL_MS {
                player.last_time_synch_ms = now_ms;
                outgoing.push(Outgoing {
                    connection: player.connection.clone(),
                    channel: MESSAGE_CHANNEL,
                    reliable: true,
                    envelopes: vec![
                        ServerMessage::TimeSynch(TimeSynchMsg { time: server_time }).envelope()?,
                    ],
                });
            }
        }
        Ok(outgoing)
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
        Some(Departure {
            connection: player.connection.clone(),
            character: player.character,
            position: body.motion.position(),
        })
    }
}

/// One bundle envelope ([`ReplicatedStateBundle::of_positions`]). Every position in the world is
/// finite (`join` refuses others), so no member is left out.
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
    use nw_carrier::sender::MAX_CHUNK_PAYLOAD;
    use nw_messages::{BundlePayload, LOGIN_MESSAGE_TYPES, MemberState};
    use nw_reflect::{Envelope, encode_server_messages};
    use nw_sim_core::{CellBounds, GroundCheck, MoveLimits, MoveRefusal};

    use nw_sim_core::test_support::NoGround;

    use super::*;

    /// The largest entity number that fits the 5-byte prefix code (35 bits).
    const LARGEST_NUMBER_SHIFT: u32 = 29;

    const RADIUS: f32 = 100.0;
    const NOW: u64 = 10_000;
    const SERVER_TIME: u64 = 7;
    /// 5 m/s with no margin and a burst of 1 s: at most 5 m in one move.
    const SPEED_MPS: f32 = 5.0;
    const BURST_SECONDS: f32 = 1.0;
    const GROUND_TOLERANCE_M: f32 = 2.0;
    const MAX_ABOVE_GROUND_M: f32 = 64.0;
    const BOUNDS: CellBounds = CellBounds {
        min_x: 0.0,
        min_y: 0.0,
        max_x: 14_336.0,
        max_y: 14_336.0,
    };
    const ONE_SECOND_MS: u64 = 1000;
    const A_BIT_LATER_MS: u64 = 100;
    const CLOSE_M: f32 = 0.01;

    fn world() -> SimWorld {
        let limits = MoveLimits::new(
            SPEED_MPS,
            0.0,
            BURST_SECONDS,
            GROUND_TOLERANCE_M,
            MAX_ABOVE_GROUND_M,
        )
        .expect("limits");
        SimWorld::new(
            WorldSettings {
                interest_radius_m: RADIUS,
                limits,
                bounds: BOUNDS,
            },
            WorldIds::random(0).expect("ids"),
            Arc::new(NoGround),
        )
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

    /// The entity numbers in the bundle of one connection.
    fn seen_by(outgoing: &[Outgoing], who: &ConnectionRef) -> Vec<u64> {
        positions_seen_by(outgoing, who)
            .into_iter()
            .map(|(number, _)| number)
            .collect()
    }

    /// The entity numbers and x positions in the bundle of one connection.
    fn positions_seen_by(outgoing: &[Outgoing], who: &ConnectionRef) -> Vec<(u64, f32)> {
        let message = outgoing
            .iter()
            .find(|out| out.connection == *who && out.channel == REPLICATION_CHANNEL)
            .expect("bundle");
        let envelope = Envelope::decode(&message.envelopes[0]).expect("envelope");
        let Some(Ok(ServerMessage::Bundle(bundle))) = ServerMessage::from_envelope(&envelope)
        else {
            panic!("expected a bundle");
        };
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

    #[test]
    fn a_move_inside_the_limits_is_seen_and_a_fast_one_is_refused() {
        let mut world = world();
        let (a, b) = (connection(1), connection(2));
        world
            .join(a.clone(), Uuid::new_v4(), at(0.0), NOW, SERVER_TIME)
            .expect("a");
        world
            .join(b.clone(), Uuid::new_v4(), at(10.0), NOW, SERVER_TIME)
            .expect("b");
        assert_eq!(
            world.move_to(&a, at(SPEED_MPS), NOW + ONE_SECOND_MS),
            Ok(GroundCheck::NoGround)
        );
        let outgoing = world.tick(NOW + ONE_SECOND_MS, SERVER_TIME).expect("tick");
        let (_, seen_x) = positions_seen_by(&outgoing, &b)[1];
        assert!((seen_x - SPEED_MPS).abs() < CLOSE_M);
        let far = at(SPEED_MPS * 10.0);
        let later = NOW + ONE_SECOND_MS + A_BIT_LATER_MS;
        assert_eq!(
            world.move_to(&a, far, later),
            Err(MoveError::Refused(MoveRefusal::TooFast))
        );
        let outgoing = world.tick(later, SERVER_TIME).expect("tick");
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
    fn a_join_sends_the_login_messages_reliably_on_channel_0() {
        let mut world = world();
        let login = world
            .join(connection(1), Uuid::new_v4(), at(0.0), NOW, SERVER_TIME)
            .expect("join");
        assert_eq!(login.channel, MESSAGE_CHANNEL);
        assert!(login.reliable);
        assert_eq!(login.envelopes.len(), LOGIN_MESSAGE_TYPES.len());
        assert_eq!(world.players(), 1);
    }

    #[test]
    fn players_see_themselves_first_and_others_inside_the_radius() {
        let mut world = world();
        let (a, b, c) = (connection(1), connection(2), connection(3));
        world
            .join(a.clone(), Uuid::new_v4(), at(0.0), NOW, SERVER_TIME)
            .expect("a");
        world
            .join(b.clone(), Uuid::new_v4(), at(50.0), NOW, SERVER_TIME)
            .expect("b");
        world
            .join(c.clone(), Uuid::new_v4(), at(500.0), NOW, SERVER_TIME)
            .expect("c");
        let outgoing = world.tick(NOW, SERVER_TIME).expect("tick");
        assert_eq!(seen_by(&outgoing, &a), vec![1, 2]);
        assert_eq!(seen_by(&outgoing, &b), vec![2, 1]);
        assert_eq!(seen_by(&outgoing, &c), vec![3]);
        assert!(
            outgoing
                .iter()
                .all(|out| out.channel != REPLICATION_CHANNEL || !out.reliable)
        );
    }

    #[test]
    fn a_player_who_leaves_is_gone_from_the_next_bundles() {
        let mut world = world();
        let (a, b) = (connection(1), connection(2));
        let character = Uuid::new_v4();
        world
            .join(a.clone(), Uuid::new_v4(), at(0.0), NOW, SERVER_TIME)
            .expect("a");
        world
            .join(b.clone(), character, at(10.0), NOW, SERVER_TIME)
            .expect("b");
        let departure = world.leave(&b).expect("left");
        assert_eq!(departure.connection, b);
        assert_eq!(departure.character, character);
        assert_eq!(departure.position, at(10.0));
        let outgoing = world.tick(NOW, SERVER_TIME).expect("tick");
        assert_eq!(seen_by(&outgoing, &a), vec![1]);
        assert_eq!(world.leave(&b), None);
    }

    #[test]
    fn a_connection_or_character_joins_once() {
        let mut world = world();
        let character = Uuid::new_v4();
        world
            .join(connection(1), character, at(0.0), NOW, SERVER_TIME)
            .expect("first");
        assert!(matches!(
            world.join(connection(1), Uuid::new_v4(), at(0.0), NOW, SERVER_TIME),
            Err(WorldError::AlreadyJoined(_))
        ));
        assert!(matches!(
            world.join(connection(2), character, at(0.0), NOW, SERVER_TIME),
            Err(WorldError::CharacterInWorld(_))
        ));
    }

    #[test]
    fn a_time_synch_goes_out_every_three_seconds() {
        let mut world = world();
        world
            .join(connection(1), Uuid::new_v4(), at(0.0), NOW, SERVER_TIME)
            .expect("join");
        let reliable = |outgoing: &[Outgoing]| outgoing.iter().filter(|out| out.reliable).count();
        assert_eq!(
            reliable(&world.tick(NOW + 1, SERVER_TIME).expect("tick")),
            0
        );
        assert_eq!(
            reliable(
                &world
                    .tick(NOW + TIME_SYNCH_INTERVAL_MS, SERVER_TIME)
                    .expect("tick")
            ),
            1
        );
        assert_eq!(
            reliable(
                &world
                    .tick(NOW + TIME_SYNCH_INTERVAL_MS + 1, SERVER_TIME)
                    .expect("tick")
            ),
            0
        );
    }

    #[test]
    fn a_position_that_is_not_finite_cannot_join() {
        let mut world = world();
        assert!(matches!(
            world.join(
                connection(1),
                Uuid::new_v4(),
                at(f32::NAN),
                NOW,
                SERVER_TIME
            ),
            Err(WorldError::NotFinite)
        ));
        assert_eq!(world.players(), 0);
    }

    #[test]
    fn a_gateway_that_stops_takes_its_players_out() {
        let mut world = world();
        world
            .join(connection(1), Uuid::new_v4(), at(0.0), NOW, SERVER_TIME)
            .expect("one");
        world
            .join(connection(2), Uuid::new_v4(), at(0.0), NOW, SERVER_TIME)
            .expect("two");
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
        let mut world = world();
        let viewer = connection(1);
        world
            .join(viewer.clone(), Uuid::new_v4(), at(0.0), NOW, SERVER_TIME)
            .expect("viewer");
        for number in 2..=100 {
            world
                .join(
                    connection(number),
                    Uuid::new_v4(),
                    at(1.0),
                    NOW,
                    SERVER_TIME,
                )
                .expect("other");
        }
        let outgoing = world.tick(NOW, SERVER_TIME).expect("tick");
        let bundles: Vec<&Outgoing> = outgoing
            .iter()
            .filter(|out| out.connection == viewer && out.channel == REPLICATION_CHANNEL)
            .collect();
        assert_eq!(bundles.len(), 100_usize.div_ceil(MAX_BUNDLE_MEMBERS));
        let largest_number = u64::MAX >> LARGEST_NUMBER_SHIFT;
        let full: Vec<(u64, Position)> = (0..MAX_BUNDLE_MEMBERS as u64)
            .map(|offset| (largest_number - offset, at(0.0)))
            .collect();
        let envelope = bundle(u64::MAX >> LARGEST_NUMBER_SHIFT, &full).expect("bundle");
        let payload = encode_server_messages(&[&envelope]).expect("payload");
        assert!(payload.len() <= MAX_CHUNK_PAYLOAD, "{}", payload.len());
    }
}
