// SPDX-License-Identifier: AGPL-3.0-or-later
//! The two-client world check (M4 spec section 9, criteria 3.1 to 3.5 of section 1.2): two
//! clients are in the world together and see each other. Each client sends its first time sync
//! request after its own spawn card, as the game client does: the sim holds every other entity
//! until then. The characters keep their saved places, which can be far apart, so the first
//! client goes to the second with a game master teleport (`!tp`). Then it walks a route while
//! the second watches, then sends one move that is far too long, which the world must refuse.
//! Then the first logs in again at the position the world saved. The walk sends the route's
//! points, or, with a [`RecordWalk`], key or pad records as the game client does
//! ([`crate::record_walk`]); then the second client must see the first where its own body walked.

use std::cell::Cell;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use nw_messages::{
    BodyCodec, ClientInputMsg, InputPosition, RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX, WorldPosition,
};
use nw_reflect::{LoginToken, REGISTRATION_STATUS_AUTHORIZED};
use nw_sim_core::{Ground, Position};
use reqwest::Client;
use thiserror::Error;

use crate::play::{say_in_area, send_time_sync};
use crate::record_walk::{InputDevice, RecordWalker};
use crate::rep::{RepClient, RepError};
use crate::route::Route;
use crate::udp::{Hold, RepOutcome, RunError, run_rep_with};
use crate::view::{ViewError, WorldView};
use crate::walk::{WalkError, WalkSettings, login_token, walk};

/// Check: every session got registration status 0.
pub const REGISTERED: &str = "every session registered";
/// Check: every session got the login messages in the captured order, then bundles.
pub const IN_ORDER: &str = "login messages in the captured order";
/// Check: each client saw the other one's entity.
pub const SAW_EACH_OTHER: &str = "each client saw the other";
/// Check: the second client saw the first one only on its route, and at its end (criterion 3.3).
pub const FOLLOWED_ROUTE: &str = "the walk was seen on the route";
/// Check: the far move never showed, and the first client stayed at the end of the route
/// (criterion 3.4).
pub const SNAPPED_BACK: &str = "the far move was refused";
/// Check: the second login of the first client is where its first session ended, in the ground
/// plane: the client walks at the height it landed at, and the server keeps the saved body on its
/// ground (live 2026-11-19: 1.2 m lower at the end of the route than the walk's height).
pub const KEPT_POSITION: &str = "the next login is at the saved position";
/// Carrier channel of client movement (type 1589): channel 1, unreliable, as in the captures.
pub const INPUT_CHANNEL: u8 = 1;
/// Time between two movement messages (the captured client sends every 30 to 40 ms).
pub const SEND_INTERVAL: Duration = Duration::from_millis(33);
/// Time at the end of the route before the far move.
pub const SETTLE: Duration = Duration::from_millis(500);
/// Time the far move is sent: every message in it, so one lost datagram cannot hide the move.
pub const JUMP_WINDOW: Duration = Duration::from_millis(500);
/// Largest distance in the ground plane, in metres, from the meeting point at which the first
/// client counts as there after its teleport (the play bot's teleport check uses the same).
pub const MEETING_REACHED_M: f32 = 3.0;
/// A gap between two states of a walking player, as another client gets them, longer than this
/// is late: three sim ticks of 33 ms.
pub const LATE_STATE_GAP: Duration = Duration::from_millis(100);

/// The two accounts, the walk and the timers.
#[derive(Debug, Clone, PartialEq)]
pub struct PairSettings {
    /// The client that walks and logs in twice.
    pub first: WalkSettings,
    /// The client that watches.
    pub second: WalkSettings,
    /// The route of the first client, from where it stands.
    pub route: Route,
    /// Length of the far move after the route, east, in metres.
    pub jump_m: f32,
    /// Time in the world per session.
    pub hold: Duration,
    /// Longest wait for a registration response.
    pub registration_timeout: Duration,
}

/// A walk with input records instead of points: the device, and the level's ground and sea
/// height the bot's own body walks on.
#[derive(Clone)]
pub struct RecordWalk {
    /// The device whose input the first client sends.
    pub device: InputDevice,
    /// The level's ground.
    pub ground: Arc<dyn Ground + Send + Sync>,
    /// The level's sea height, m.
    pub sea_level_m: f32,
}

/// One registration and what the client saw.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    /// What happened on the connection.
    pub outcome: RepOutcome,
    /// The world messages, read.
    pub view: WorldView,
}

/// Where and when the first client started its walk.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WalkStart {
    /// Its own position then: the start of the route.
    pub origin: WorldPosition,
    /// The index of that position in its own track; the positions before it are the way to the
    /// meeting point.
    pub track_index: usize,
    /// When the walk started.
    pub at: Instant,
}

/// The three sessions of a pair run, and the route the first client walked.
#[derive(Debug, Clone, PartialEq)]
pub struct PairReport {
    /// The first client, at the same time as the second.
    pub first: Session,
    /// The second client.
    pub second: Session,
    /// The first client again, after its first session ended.
    pub again: Session,
    /// The route of the first client.
    pub route: Route,
    /// The start of the walk; `None` when the first client never reached the meeting point.
    pub walk: Option<WalkStart>,
    /// When the second client got each new state of the first one, in order.
    pub walker_states: Vec<Instant>,
    /// The first client's own body after each record of a [`RecordWalk`], from the start of the
    /// walk; empty for a walk of points.
    pub walked: Vec<WorldPosition>,
}

/// One named check and its result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Check {
    /// What it checks.
    pub name: &'static str,
    /// Whether it passed.
    pub passed: bool,
}

/// A failed pair run.
#[derive(Debug, Error)]
pub enum PairError {
    /// An HTTPS step failed.
    #[error(transparent)]
    Walk(#[from] WalkError),
    /// The UDP run failed.
    #[error(transparent)]
    Run(#[from] RunError),
    /// The client could not start.
    #[error(transparent)]
    Rep(#[from] RepError),
    /// A world message did not decode.
    #[error(transparent)]
    View(#[from] ViewError),
}

impl PairReport {
    /// Every check, in order. A position is on the route within `path_tolerance_m`; the next
    /// login is within `position_tolerance_m` of the saved position.
    pub fn checks(&self, path_tolerance_m: f32, position_tolerance_m: f32) -> Vec<Check> {
        let sessions = [&self.first, &self.second, &self.again];
        let saw = |viewer: &Session, seen: &Session| {
            seen.view
                .own_number()
                .is_some_and(|number| viewer.view.others().contains(&number))
        };
        let own_track =
            self.first
                .view
                .own_number()
                .zip(self.walk)
                .map_or(&[][..], |(number, walk)| {
                    self.first
                        .view
                        .track(number)
                        .get(walk.track_index..)
                        .unwrap_or_default()
                });
        let seen_track = self
            .first
            .view
            .own_number()
            .map_or(&[][..], |number| self.second.view.track(number));
        let origin = self.walk.map(|walk| walk.origin);
        // A walk of records went where the client's own body walked, not along the route's
        // points at their times.
        let walked_route = origin.and_then(|origin| self.walked_route(origin));
        let route = if self.walked.is_empty() {
            Some(&self.route)
        } else {
            walked_route.as_ref()
        };
        let on_route = |position: &WorldPosition| {
            origin.zip(route).is_some_and(|(origin, route)| {
                route.distance_to(offset(origin, *position)) <= path_tolerance_m
            })
        };
        let at_end = |position: &WorldPosition| {
            origin.zip(route).is_some_and(|(origin, route)| {
                let [east, north] = offset(origin, *position);
                let [end_east, end_north] = route.end();
                (east - end_east).hypot(north - end_north) <= path_tolerance_m
            })
        };
        let kept = match (
            self.first.view.own_position(),
            self.again.view.own_position(),
        ) {
            (Some(before), Some(after)) => {
                horizontal_distance(before, after) <= position_tolerance_m
            }
            _ => false,
        };
        vec![
            Check {
                name: REGISTERED,
                passed: sessions
                    .iter()
                    .all(|session| session.outcome.status == Some(REGISTRATION_STATUS_AUTHORIZED)),
            },
            Check {
                name: IN_ORDER,
                passed: sessions
                    .iter()
                    .all(|session| session.view.in_captured_order()),
            },
            Check {
                name: SAW_EACH_OTHER,
                passed: saw(&self.first, &self.second) && saw(&self.second, &self.first),
            },
            Check {
                name: FOLLOWED_ROUTE,
                passed: !seen_track.is_empty()
                    && seen_track.iter().all(on_route)
                    && seen_track.iter().any(at_end),
            },
            Check {
                name: SNAPPED_BACK,
                passed: !own_track.is_empty()
                    && own_track.iter().all(on_route)
                    && own_track.last().is_some_and(at_end),
            },
            Check {
                name: KEPT_POSITION,
                passed: kept,
            },
        ]
    }

    /// The path of the first client's own body in a walk of records, from `origin`; `None` for a
    /// walk of points or one too short for a path.
    fn walked_route(&self, origin: WorldPosition) -> Option<Route> {
        let points = self
            .walked
            .iter()
            .map(|position| offset(origin, *position))
            .collect();
        Route::new(points, self.route.speed_mps()).ok()
    }

    /// Whether every check passed.
    pub fn passed(&self, path_tolerance_m: f32, position_tolerance_m: f32) -> bool {
        self.checks(path_tolerance_m, position_tolerance_m)
            .iter()
            .all(|check| check.passed)
    }

    /// The times between two states of the first client that the second got while the route
    /// was walked, in order: how often a watcher gets a walking player's state.
    pub fn walk_state_gaps(&self) -> Vec<Duration> {
        let Some(walk) = self.walk else {
            return Vec::new();
        };
        let end = walk.at + self.route.duration();
        let during: Vec<Instant> = self
            .walker_states
            .iter()
            .copied()
            .filter(|at| (walk.at..=end).contains(at))
            .collect();
        during
            .windows(2)
            .map(|pair| pair[1].duration_since(pair[0]))
            .collect()
    }
}

/// East and north of `position` from `origin`, in metres.
fn offset(origin: WorldPosition, position: WorldPosition) -> [f32; 2] {
    [position.x - origin.x, position.y - origin.y]
}

/// Distance between two positions in the ground plane, in metres.
fn horizontal_distance(a: WorldPosition, b: WorldPosition) -> f32 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// Registers with `token` at `gateway`, stays `hold` in the game (it sends the first time sync
/// request, [`InGame`]), and reads the world messages.
///
/// # Errors
///
/// Returns [`PairError`] when the run fails or a world message does not decode.
pub async fn session(
    gateway: SocketAddr,
    client: RepClient,
    hold: Duration,
    registration_timeout: Duration,
) -> Result<Session, PairError> {
    let mut in_game = InGame::default();
    session_with(
        gateway,
        client,
        hold,
        registration_timeout,
        |client, outcome, _| in_game.tick(client, outcome),
    )
    .await
}

/// [`session`] with the per-tick hook of [`run_rep_with`].
///
/// # Errors
///
/// Returns [`PairError`] when the run fails or a world message does not decode.
pub async fn session_with(
    gateway: SocketAddr,
    client: RepClient,
    hold: Duration,
    registration_timeout: Duration,
    on_tick: impl FnMut(&mut RepClient, &mut RepOutcome, Duration) -> Result<(), RepError>,
) -> Result<Session, PairError> {
    let outcome = run_rep_with(
        gateway,
        client,
        Hold::For(hold),
        registration_timeout,
        on_tick,
    )
    .await?;
    let view = WorldView::from_messages(&outcome.messages)?;
    Ok(Session { outcome, view })
}

/// What a client of the pair has read so far in its session. It sends the first time sync
/// request when it is due ([`WorldView::time_sync_due`]), as the game client does once it is in
/// the game.
#[derive(Default)]
struct InGame {
    view: WorldView,
    read: usize,
    /// Whether it sent the time sync request.
    in_game: bool,
}

impl InGame {
    fn tick(&mut self, client: &mut RepClient, outcome: &RepOutcome) -> Result<(), RepError> {
        for received in &outcome.messages[self.read..] {
            // A message that does not decode fails the run later, when the session is read.
            let _ = self.view.read(received);
        }
        self.read = outcome.messages.len();
        if let Some(own) = self.view.time_sync_due() {
            send_time_sync(client, own)?;
            self.in_game = true;
        }
        Ok(())
    }
}

/// What the two sessions of a pair run tell each other while they run.
#[derive(Default)]
struct Meeting {
    /// Where the second client stands once it is in the game.
    place: Cell<Option<WorldPosition>>,
    /// The entity number of the first client.
    walker: Cell<Option<u64>>,
}

/// The first client during its walk: once in the game it goes to the second client with a
/// teleport, then it follows its own position in the bundles and sends [`walk_offset`] from
/// where it landed, every [`SEND_INTERVAL`]: as the point itself, or, in a [`RecordWalk`], as
/// the goal of its own body's next record. In a [`RecordWalk`] the far move is records whose
/// camera is `jump_m` east of the body (a client that puts itself far away), and the body stays
/// at the end of the route.
struct Walker<'a> {
    route: &'a Route,
    jump_m: f32,
    meeting: &'a Meeting,
    in_game: InGame,
    teleported: bool,
    /// The start of the walk, and its time since the registration.
    start: Option<(WalkStart, Duration)>,
    last_sent: Option<Duration>,
    records: Option<&'a RecordWalk>,
    /// Its own body in a [`RecordWalk`], from the start of the walk.
    body: Option<RecordWalker>,
}

/// The second client: once in the game it gives its place as the meeting point, and it notes
/// when each new state of the first client comes.
struct Watcher<'a> {
    meeting: &'a Meeting,
    in_game: InGame,
    walker_states: Vec<Instant>,
}

impl Watcher<'_> {
    fn tick(&mut self, client: &mut RepClient, outcome: &RepOutcome) -> Result<(), RepError> {
        self.in_game.tick(client, outcome)?;
        let view = &self.in_game.view;
        if self.in_game.in_game && self.meeting.place.get().is_none() {
            self.meeting.place.set(view.own_position());
        }
        if let Some(walker) = self.meeting.walker.get() {
            let now = Instant::now();
            let seen = view.track(walker).len();
            let new = seen.saturating_sub(self.walker_states.len());
            self.walker_states.extend(std::iter::repeat_n(now, new));
        }
        Ok(())
    }
}

/// Where the first client is `walked` after it started: on the route, then [`SETTLE`] at its
/// end, then `jump_m` east of the end for [`JUMP_WINDOW`], then at the end again.
pub fn walk_offset(route: &Route, jump_m: f32, walked: Duration) -> [f32; 2] {
    let [end_east, end_north] = route.end();
    if walked < route.duration() + SETTLE {
        route.offset_at(walked)
    } else if in_jump(route, walked) {
        [end_east + jump_m, end_north]
    } else {
        route.end()
    }
}

/// Whether the first client sends the far move `walked` after it started.
fn in_jump(route: &Route, walked: Duration) -> bool {
    let jump_start = route.duration() + SETTLE;
    (jump_start..jump_start + JUMP_WINDOW).contains(&walked)
}

/// `input` with the camera of its newest record `east_m` farther east.
fn camera_moved_east(mut input: ClientInputMsg, east_m: f32) -> ClientInputMsg {
    if let Some(camera) = input
        .records
        .first_mut()
        .and_then(|record| record.bit_7.as_mut())
    {
        camera.x += east_m;
    }
    input
}

/// `at` as the place of a walking body.
fn body_place(at: WorldPosition) -> Position {
    Position {
        x: at.x,
        y: at.y,
        z: at.z(),
    }
}

impl Walker<'_> {
    fn tick(
        &mut self,
        client: &mut RepClient,
        outcome: &mut RepOutcome,
        since_registration: Duration,
    ) -> Result<(), RepError> {
        self.in_game.tick(client, outcome)?;
        let view = &self.in_game.view;
        let (Some(number), Some(own)) = (view.own_number(), view.own_position()) else {
            return Ok(());
        };
        self.meeting.walker.set(Some(number));
        let (start, started) = match self.start {
            Some(start) => start,
            None => {
                let Some(place) = self.meeting.place.get().filter(|_| self.in_game.in_game) else {
                    return Ok(());
                };
                if horizontal_distance(own, place) > MEETING_REACHED_M {
                    if !self.teleported {
                        self.teleported = true;
                        let line = format!("!tp {} {} {}", place.x, place.y, place.z());
                        say_in_area(client, number, &line)?;
                    }
                    return Ok(());
                }
                let start = WalkStart {
                    origin: own,
                    track_index: view.track(number).len().saturating_sub(1),
                    at: Instant::now(),
                };
                self.body = self
                    .records
                    .map(|records| RecordWalker::new(body_place(own), records.device));
                *self.start.insert((start, since_registration))
            }
        };
        let origin = start.origin;
        if self
            .last_sent
            .is_some_and(|sent| since_registration.saturating_sub(sent) < SEND_INTERVAL)
        {
            return Ok(());
        }
        self.last_sent = Some(since_registration);
        let walked = since_registration.saturating_sub(started);
        let [east, north] = walk_offset(self.route, self.jump_m, walked);
        let goal = [origin.x + east, origin.y + north];
        let input = match (self.records, self.body.as_mut()) {
            (Some(records), Some(body)) => {
                body.follow_server(body_place(own));
                let jumping = in_jump(self.route, walked);
                let [end_east, end_north] = self.route.end();
                let goal = if jumping {
                    [origin.x + end_east, origin.y + end_north]
                } else {
                    goal
                };
                let input = body.frame(
                    goal,
                    SEND_INTERVAL.as_secs_f32(),
                    &*records.ground,
                    records.sea_level_m,
                );
                if jumping {
                    input.map(|input| camera_moved_east(input, self.jump_m))
                } else {
                    input
                }
            }
            _ => InputPosition::from_metres(goal[0], goal[1], origin.z())
                .map(ClientInputMsg::position),
        };
        let Some(input) = input else {
            return Ok(());
        };
        let body = input.encode()?;
        client.send_message(
            INPUT_CHANNEL,
            false,
            RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX,
            body,
        )
    }
}

/// Runs the pair: both clients in the world together while the first walks, then the first one
/// again. `new_client` makes a client for a token; `report` gets one line per HTTPS step.
///
/// # Errors
///
/// Returns [`PairError`] for the first step that fails.
pub async fn run_pair(
    http: &Client,
    settings: &PairSettings,
    records: Option<&RecordWalk>,
    new_client: impl Fn(LoginToken) -> Result<RepClient, RepError>,
    mut report: impl FnMut(String),
) -> Result<PairReport, PairError> {
    let first = walk(http, &settings.first, &mut report).await?;
    let second = walk(http, &settings.second, &mut report).await?;
    let meeting = Meeting::default();
    let mut walker = Walker {
        route: &settings.route,
        jump_m: settings.jump_m,
        meeting: &meeting,
        in_game: InGame::default(),
        teleported: false,
        start: None,
        last_sent: None,
        records,
        body: None,
    };
    let mut watcher = Watcher {
        meeting: &meeting,
        in_game: InGame::default(),
        walker_states: Vec::new(),
    };
    let walking = session_with(
        first.rep_address,
        new_client(login_token(&first.token))?,
        settings.hold,
        settings.registration_timeout,
        |client, outcome, since| walker.tick(client, outcome, since),
    );
    let watching = session_with(
        second.rep_address,
        new_client(login_token(&second.token))?,
        settings.hold,
        settings.registration_timeout,
        |client, outcome, _| watcher.tick(client, outcome),
    );
    let (first, second) = tokio::join!(walking, watching);
    let (first, second) = (first?, second?);
    let walk_start = walker.start.map(|(start, _)| start);
    let walked = walker.body.map_or_else(Vec::new, |body| {
        body.track()
            .iter()
            .filter_map(|at| WorldPosition::from_metres(at.x, at.y, at.z))
            .collect()
    });
    let again = walk(http, &settings.first, &mut report).await?;
    let again = session(
        again.rep_address,
        new_client(login_token(&again.token))?,
        settings.hold,
        settings.registration_timeout,
    )
    .await?;
    Ok(PairReport {
        first,
        second,
        again,
        route: settings.route.clone(),
        walk: walk_start,
        walker_states: watcher.walker_states,
        walked,
    })
}

#[cfg(test)]
mod tests {
    use nw_messages::ServerMessage;
    use nw_messages::test_support::{login_samples, players_bundle};
    use nw_reflect::{Envelope, TypeId};

    use super::*;
    use crate::rep::Received;

    const FIRST: u64 = 1;
    const SECOND: u64 = 2;
    const LOGIN_CHANNEL: u8 = 0;
    const BUNDLE_CHANNEL: u8 = 1;
    const PATH_TOLERANCE_M: f32 = 0.5;
    const POSITION_TOLERANCE_M: f32 = 0.1;
    const ORIGIN: f32 = 100.0;
    const ROUTE_END_M: f32 = 3.0;
    const HALF_WAY_M: f32 = 1.5;
    /// Beside the route, farther than the path tolerance.
    const OFF_ROUTE_M: f32 = 2.0;
    const JUMP_M: f32 = 50.0;
    const SPEED_MPS: f32 = 1.5;

    fn route() -> Route {
        Route::new(vec![[0.0, 0.0], [ROUTE_END_M, 0.0]], SPEED_MPS).expect("route")
    }

    /// A point `along` metres down the route and `beside` metres off it.
    fn at(along: f32, beside: f32) -> WorldPosition {
        WorldPosition::from_metres(ORIGIN + along, ORIGIN + beside, ORIGIN).expect("finite")
    }

    fn walked() -> Vec<WorldPosition> {
        vec![at(0.0, 0.0), at(HALF_WAY_M, 0.0), at(ROUTE_END_M, 0.0)]
    }

    fn received(channel: u8, message: &ServerMessage) -> Received {
        Received {
            channel,
            envelope: Envelope::message(
                TypeId::FileSlot(message.type_index()),
                message.encode().expect("encode"),
            ),
        }
    }

    /// A registered session of entity `own` along `own_track`, which saw `other` along
    /// `other_track` (one bundle per step).
    fn session(
        own: u64,
        own_track: &[WorldPosition],
        other: u64,
        other_track: &[WorldPosition],
    ) -> Session {
        let mut messages: Vec<Received> = login_samples(own)
            .iter()
            .map(|message| received(LOGIN_CHANNEL, message))
            .collect();
        for step in 0..own_track.len().max(other_track.len()) {
            let mut positions = Vec::new();
            if let Some(position) = own_track.get(step).or(own_track.last()) {
                positions.push((own, *position));
            }
            if let Some(position) = other_track.get(step) {
                positions.push((other, *position));
            }
            let sequence = u64::try_from(step).expect("small");
            let bundle = players_bundle(sequence, own, &positions);
            messages.push(received(BUNDLE_CHANNEL, &ServerMessage::Bundle(bundle)));
        }
        let view = WorldView::from_messages(&messages).expect("view");
        Session {
            outcome: RepOutcome {
                status: Some(REGISTRATION_STATUS_AUTHORIZED),
                messages,
                ..RepOutcome::default()
            },
            view,
        }
    }

    /// A walk that started at the first position of `first_track`, `track_index` positions
    /// into it.
    fn walk_start(first_track: &[WorldPosition], track_index: usize) -> Option<WalkStart> {
        first_track.get(track_index).map(|origin| WalkStart {
            origin: *origin,
            track_index,
            at: Instant::now(),
        })
    }

    fn pair_report(
        first_track: &[WorldPosition],
        second_sees_first: &[WorldPosition],
        again_at: WorldPosition,
    ) -> PairReport {
        PairReport {
            first: session(FIRST, first_track, SECOND, &[at(0.0, 0.0)]),
            second: session(SECOND, &[at(0.0, 0.0)], FIRST, second_sees_first),
            again: session(FIRST, &[again_at], SECOND, &[]),
            route: route(),
            walk: walk_start(first_track, 0),
            walker_states: Vec::new(),
            walked: Vec::new(),
        }
    }

    fn failed(report: &PairReport) -> Vec<&'static str> {
        report
            .checks(PATH_TOLERANCE_M, POSITION_TOLERANCE_M)
            .into_iter()
            .filter(|check| !check.passed)
            .map(|check| check.name)
            .collect()
    }

    #[test]
    fn every_check_passes_for_a_walk_on_the_route_and_a_refused_jump() {
        let report = pair_report(&walked(), &walked(), at(ROUTE_END_M, 0.0));
        assert!(failed(&report).is_empty(), "{:?}", failed(&report));
        assert!(report.passed(PATH_TOLERANCE_M, POSITION_TOLERANCE_M));
    }

    #[test]
    fn a_walk_of_records_is_checked_against_where_the_clients_own_body_walked() {
        // The body went beside the route's points (keys move it at their own speed), and the
        // second client saw it there.
        let body = vec![
            at(0.0, 0.0),
            at(HALF_WAY_M, OFF_ROUTE_M),
            at(ROUTE_END_M, 0.0),
        ];
        let mut report = pair_report(&body, &body, at(ROUTE_END_M, 0.0));
        assert_eq!(
            failed(&report),
            vec![FOLLOWED_ROUTE, SNAPPED_BACK],
            "against the route's points"
        );
        report.walked = body;
        assert!(failed(&report).is_empty(), "{:?}", failed(&report));
        // A place the body never walked fails, as off the route.
        let mut seen = report.walked.clone();
        seen[1] = at(HALF_WAY_M, -OFF_ROUTE_M);
        report.second = session(SECOND, &[at(0.0, 0.0)], FIRST, &seen);
        assert_eq!(failed(&report), vec![FOLLOWED_ROUTE]);
    }

    #[test]
    fn a_position_off_the_route_or_a_shown_jump_fails_its_check() {
        let mut beside = walked();
        beside[1] = at(HALF_WAY_M, OFF_ROUTE_M);
        let report = pair_report(&walked(), &beside, at(ROUTE_END_M, 0.0));
        assert_eq!(failed(&report), vec![FOLLOWED_ROUTE]);
        let short = &walked()[..2];
        let report = pair_report(&walked(), short, at(ROUTE_END_M, 0.0));
        assert_eq!(failed(&report), vec![FOLLOWED_ROUTE]);
        let mut jumped = walked();
        jumped.push(at(ROUTE_END_M + JUMP_M, 0.0));
        let report = pair_report(&jumped, &walked(), at(ROUTE_END_M + JUMP_M, 0.0));
        assert_eq!(failed(&report), vec![SNAPPED_BACK]);
        let report = pair_report(&walked(), &walked(), at(0.0, 0.0));
        assert_eq!(failed(&report), vec![KEPT_POSITION]);
    }

    #[test]
    fn the_far_move_follows_the_walk_for_a_while_then_the_end_again() {
        let route = route();
        let end = route.duration() + SETTLE;
        assert_eq!(walk_offset(&route, JUMP_M, Duration::ZERO), [0.0, 0.0]);
        assert_eq!(
            walk_offset(&route, JUMP_M, end),
            [ROUTE_END_M + JUMP_M, 0.0]
        );
        assert_eq!(
            walk_offset(&route, JUMP_M, end + JUMP_WINDOW - SEND_INTERVAL),
            [ROUTE_END_M + JUMP_M, 0.0]
        );
        assert_eq!(
            walk_offset(&route, JUMP_M, end + JUMP_WINDOW),
            [ROUTE_END_M, 0.0]
        );
    }

    #[test]
    fn the_next_login_may_stand_lower_on_the_ground() {
        const GROUND_DROP_M: f32 = 1.2;
        let end = at(ROUTE_END_M, 0.0);
        let lower =
            WorldPosition::from_metres(end.x, end.y, end.z() - GROUND_DROP_M).expect("finite");
        let report = pair_report(&walked(), &walked(), lower);
        assert!(failed(&report).is_empty(), "{:?}", failed(&report));
    }

    #[test]
    fn the_way_to_the_meeting_point_is_not_part_of_the_walk() {
        const FAR_M: f32 = 9000.0;
        let mut track = vec![at(-FAR_M, FAR_M)];
        track.extend(walked());
        let mut report = pair_report(&track, &walked(), at(ROUTE_END_M, 0.0));
        report.walk = walk_start(&track, 1);
        assert!(failed(&report).is_empty(), "{:?}", failed(&report));
        report.walk = None;
        assert_eq!(
            failed(&report),
            vec![FOLLOWED_ROUTE, SNAPPED_BACK],
            "no meeting, no walk"
        );
    }

    #[test]
    fn the_state_gaps_are_those_of_the_walk_only() {
        const GAP: Duration = Duration::from_millis(33);
        let mut report = pair_report(&walked(), &walked(), at(ROUTE_END_M, 0.0));
        let start = report.walk.expect("walk").at;
        let end = start + report.route.duration();
        report.walker_states = vec![
            start.checked_sub(GAP).expect("before"),
            start,
            start + GAP,
            start + GAP * 3,
            end + GAP,
        ];
        assert_eq!(report.walk_state_gaps(), vec![GAP, GAP * 2]);
        report.walk = None;
        assert!(report.walk_state_gaps().is_empty());
    }

    #[test]
    fn a_missed_registration_or_sighting_fails_its_check() {
        let mut report = pair_report(&walked(), &walked(), at(ROUTE_END_M, 0.0));
        report.second.outcome.status = None;
        assert_eq!(failed(&report), vec![REGISTERED]);
        let mut report = pair_report(&walked(), &walked(), at(ROUTE_END_M, 0.0));
        report.first = session(FIRST, &walked(), SECOND, &[]);
        assert_eq!(failed(&report), vec![SAW_EACH_OTHER]);
    }
}
