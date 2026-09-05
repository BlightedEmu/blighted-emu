// SPDX-License-Identifier: AGPL-3.0-or-later
//! The two-client world check (M4 spec section 9, criteria 3.1 to 3.5 of section 1.2): two
//! clients are in the world together and see each other. The first walks a route while the
//! second watches, then sends one move that is far too long, which the world must refuse. Then
//! the first logs in again at the position the world saved.

use std::net::SocketAddr;
use std::time::Duration;

use nw_messages::{
    BodyCodec, ClientInputMsg, InputPosition, RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX, WorldPosition,
};
use nw_reflect::{LoginToken, REGISTRATION_STATUS_AUTHORIZED};
use reqwest::Client;
use thiserror::Error;

use crate::rep::{RepClient, RepError};
use crate::route::Route;
use crate::udp::{RepOutcome, RunError, run_rep_with};
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
/// Check: the second login of the first client is where its first session ended.
pub const KEPT_POSITION: &str = "the next login is at the saved position";
/// Carrier channel of client movement (type 1589): channel 1, unreliable, as in the captures.
pub const INPUT_CHANNEL: u8 = 1;
/// Time between two movement messages (the captured client sends every 30 to 40 ms).
pub const SEND_INTERVAL: Duration = Duration::from_millis(33);
/// Time at the end of the route before the far move.
pub const SETTLE: Duration = Duration::from_millis(500);
/// Time the far move is sent: every message in it, so one lost datagram cannot hide the move.
pub const JUMP_WINDOW: Duration = Duration::from_millis(500);

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

/// One registration and what the client saw.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    /// What happened on the connection.
    pub outcome: RepOutcome,
    /// The world messages, read.
    pub view: WorldView,
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
        let own_track = self
            .first
            .view
            .own_number()
            .map_or(&[][..], |number| self.first.view.track(number));
        let seen_track = self
            .first
            .view
            .own_number()
            .map_or(&[][..], |number| self.second.view.track(number));
        let origin = own_track.first().copied();
        let on_route = |position: &WorldPosition| {
            origin.is_some_and(|origin| {
                self.route.distance_to(offset(origin, *position)) <= path_tolerance_m
            })
        };
        let at_end = |position: &WorldPosition| {
            origin.is_some_and(|origin| {
                let [east, north] = offset(origin, *position);
                let [end_east, end_north] = self.route.end();
                (east - end_east).hypot(north - end_north) <= path_tolerance_m
            })
        };
        let kept = match (
            self.first.view.own_position(),
            self.again.view.own_position(),
        ) {
            (Some(before), Some(after)) => distance(before, after) <= position_tolerance_m,
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
                passed: own_track.iter().all(on_route) && own_track.last().is_some_and(at_end),
            },
            Check {
                name: KEPT_POSITION,
                passed: kept,
            },
        ]
    }

    /// Whether every check passed.
    pub fn passed(&self, path_tolerance_m: f32, position_tolerance_m: f32) -> bool {
        self.checks(path_tolerance_m, position_tolerance_m)
            .iter()
            .all(|check| check.passed)
    }
}

/// East and north of `position` from `origin`, in metres.
fn offset(origin: WorldPosition, position: WorldPosition) -> [f32; 2] {
    [position.x - origin.x, position.y - origin.y]
}

/// Distance between two positions, in metres.
pub fn distance(a: WorldPosition, b: WorldPosition) -> f32 {
    let (dx, dy, dz) = (a.x - b.x, a.y - b.y, a.z() - b.z());
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Registers with `token` at `gateway`, stays `hold`, and reads the world messages.
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
    session_with(
        gateway,
        client,
        hold,
        registration_timeout,
        |_, _, _| Ok(()),
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
    let outcome = run_rep_with(gateway, client, hold, registration_timeout, on_tick).await?;
    let view = WorldView::from_messages(&outcome.messages)?;
    Ok(Session { outcome, view })
}

/// The first client during its walk: it follows its own position in the bundles and sends
/// [`walk_offset`] from where it first stood, every [`SEND_INTERVAL`].
struct Walker<'a> {
    route: &'a Route,
    jump_m: f32,
    view: WorldView,
    read: usize,
    origin: Option<(WorldPosition, Duration)>,
    last_sent: Option<Duration>,
}

/// Where the first client is `walked` after it started: on the route, then [`SETTLE`] at its
/// end, then `jump_m` east of the end for [`JUMP_WINDOW`], then at the end again.
pub fn walk_offset(route: &Route, jump_m: f32, walked: Duration) -> [f32; 2] {
    let jump_start = route.duration() + SETTLE;
    let [end_east, end_north] = route.end();
    if walked < jump_start {
        route.offset_at(walked)
    } else if walked < jump_start + JUMP_WINDOW {
        [end_east + jump_m, end_north]
    } else {
        route.end()
    }
}

impl Walker<'_> {
    fn tick(
        &mut self,
        client: &mut RepClient,
        outcome: &mut RepOutcome,
        since_registration: Duration,
    ) -> Result<(), RepError> {
        for received in &outcome.messages[self.read..] {
            // A message that does not decode fails the run later, when the session is read.
            let _ = self.view.read(received);
        }
        self.read = outcome.messages.len();
        let Some(own) = self.view.own_position() else {
            return Ok(());
        };
        let (origin, started) = *self.origin.get_or_insert((own, since_registration));
        if self
            .last_sent
            .is_some_and(|sent| since_registration.saturating_sub(sent) < SEND_INTERVAL)
        {
            return Ok(());
        }
        self.last_sent = Some(since_registration);
        let walked = since_registration.saturating_sub(started);
        let [east, north] = walk_offset(self.route, self.jump_m, walked);
        let Some(position) =
            InputPosition::from_metres(origin.x + east, origin.y + north, origin.z())
        else {
            return Ok(());
        };
        let body = ClientInputMsg::position(position).encode()?;
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
    new_client: impl Fn(LoginToken) -> Result<RepClient, RepError>,
    mut report: impl FnMut(String),
) -> Result<PairReport, PairError> {
    let first = walk(http, &settings.first, &mut report).await?;
    let second = walk(http, &settings.second, &mut report).await?;
    let mut walker = Walker {
        route: &settings.route,
        jump_m: settings.jump_m,
        view: WorldView::default(),
        read: 0,
        origin: None,
        last_sent: None,
    };
    let walking = session_with(
        first.rep_address,
        new_client(login_token(&first.token))?,
        settings.hold,
        settings.registration_timeout,
        |client, outcome, since| walker.tick(client, outcome, since),
    );
    let (first, second) = tokio::join!(
        walking,
        session(
            second.rep_address,
            new_client(login_token(&second.token))?,
            settings.hold,
            settings.registration_timeout,
        ),
    );
    let (first, second) = (first?, second?);
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
    })
}

#[cfg(test)]
mod tests {
    use nw_messages::test_support::login_samples;
    use nw_messages::{ReplicatedStateBundle, ServerMessage};
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
            let bundle = ReplicatedStateBundle::of_positions(sequence, &positions).expect("bundle");
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
    fn a_missed_registration_or_sighting_fails_its_check() {
        let mut report = pair_report(&walked(), &walked(), at(ROUTE_END_M, 0.0));
        report.second.outcome.status = None;
        assert_eq!(failed(&report), vec![REGISTERED]);
        let mut report = pair_report(&walked(), &walked(), at(ROUTE_END_M, 0.0));
        report.first = session(FIRST, &walked(), SECOND, &[]);
        assert_eq!(failed(&report), vec![SAW_EACH_OTHER]);
    }
}
