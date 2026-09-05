// SPDX-License-Identifier: AGPL-3.0-or-later
//! The two-client world check (M4 spec section 9, criteria 3.1, 3.2 and 3.5 of section 1.2): two
//! clients log in and stay in the world together, each must see the other, and the first logs in
//! again at the position the world saved. Movement waits for the type 1589 layout.

use std::net::SocketAddr;
use std::time::Duration;

use nw_messages::WorldPosition;
use nw_reflect::{LoginToken, REGISTRATION_STATUS_AUTHORIZED};
use reqwest::Client;
use thiserror::Error;

use crate::rep::{RepClient, RepError};
use crate::udp::{RepOutcome, RunError, run_rep};
use crate::view::{ViewError, WorldView};
use crate::walk::{WalkError, WalkSettings, login_token, walk};

/// Check: every session got registration status 0.
pub const REGISTERED: &str = "every session registered";
/// Check: every session got the login messages in the captured order, then bundles.
pub const IN_ORDER: &str = "login messages in the captured order";
/// Check: each client saw the other one's entity.
pub const SAW_EACH_OTHER: &str = "each client saw the other";
/// Check: the second login of the first client is where its first session ended.
pub const KEPT_POSITION: &str = "the next login is at the saved position";

/// The two accounts and the timers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairSettings {
    /// The client that logs in twice.
    pub first: WalkSettings,
    /// The client that stays once.
    pub second: WalkSettings,
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

/// The three sessions of a pair run.
#[derive(Debug, Clone, PartialEq)]
pub struct PairReport {
    /// The first client, at the same time as the second.
    pub first: Session,
    /// The second client.
    pub second: Session,
    /// The first client again, after its first session ended.
    pub again: Session,
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
    /// Every check, in order.
    pub fn checks(&self, tolerance_m: f32) -> Vec<Check> {
        let sessions = [&self.first, &self.second, &self.again];
        let saw = |viewer: &Session, seen: &Session| {
            seen.view
                .own_number()
                .is_some_and(|number| viewer.view.others().contains(&number))
        };
        let kept = match (
            self.first.view.own_position(),
            self.again.view.own_position(),
        ) {
            (Some(before), Some(after)) => distance(before, after) <= tolerance_m,
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
                name: KEPT_POSITION,
                passed: kept,
            },
        ]
    }

    /// Whether every check passed.
    pub fn passed(&self, tolerance_m: f32) -> bool {
        self.checks(tolerance_m).iter().all(|check| check.passed)
    }
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
    let outcome = run_rep(gateway, client, hold, registration_timeout).await?;
    let view = WorldView::from_messages(&outcome.messages)?;
    Ok(Session { outcome, view })
}

/// Runs the pair: both clients in the world together, then the first one again. `new_client`
/// makes a client for a token; `report` gets one line per HTTPS step.
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
    let (first, second) = tokio::join!(
        session(
            first.rep_address,
            new_client(login_token(&first.token))?,
            settings.hold,
            settings.registration_timeout,
        ),
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
    const TOLERANCE_M: f32 = 0.1;
    const HERE: f32 = 100.0;
    /// Farther than the tolerance from [`HERE`].
    const ELSEWHERE: f32 = 101.0;

    fn received(channel: u8, message: &ServerMessage) -> Received {
        Received {
            channel,
            envelope: Envelope::message(
                TypeId::FileSlot(message.type_index()),
                message.encode().expect("encode"),
            ),
        }
    }

    /// A registered session of entity `own` at `x` that saw `seen`.
    fn session(own: u64, x: f32, seen: &[u64]) -> Session {
        let at = WorldPosition::from_metres(x, x, x).expect("finite");
        let mut messages: Vec<Received> = login_samples(own)
            .iter()
            .map(|message| received(LOGIN_CHANNEL, message))
            .collect();
        let mut positions = vec![(own, at)];
        positions.extend(seen.iter().map(|number| (*number, at)));
        let bundle = ReplicatedStateBundle::of_positions(0, &positions).expect("bundle");
        messages.push(received(BUNDLE_CHANNEL, &ServerMessage::Bundle(bundle)));
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

    fn failed(report: &PairReport) -> Vec<&'static str> {
        report
            .checks(TOLERANCE_M)
            .into_iter()
            .filter(|check| !check.passed)
            .map(|check| check.name)
            .collect()
    }

    #[test]
    fn every_check_passes_when_both_see_each_other_and_the_position_is_kept() {
        let report = PairReport {
            first: session(FIRST, HERE, &[SECOND]),
            second: session(SECOND, HERE, &[FIRST]),
            again: session(FIRST, HERE, &[]),
        };
        assert!(failed(&report).is_empty());
        assert!(report.passed(TOLERANCE_M));
    }

    #[test]
    fn a_missed_sighting_or_a_moved_position_fails_its_check() {
        let report = PairReport {
            first: session(FIRST, HERE, &[]),
            second: session(SECOND, HERE, &[FIRST]),
            again: session(FIRST, ELSEWHERE, &[]),
        };
        assert_eq!(failed(&report), vec![SAW_EACH_OTHER, KEPT_POSITION]);
        assert!(!report.passed(TOLERANCE_M));
        let mut refused = session(SECOND, HERE, &[FIRST]);
        refused.outcome.status = None;
        let report = PairReport {
            second: refused,
            ..PairReport {
                first: session(FIRST, HERE, &[SECOND]),
                second: session(SECOND, HERE, &[FIRST]),
                again: session(FIRST, HERE, &[]),
            }
        };
        assert_eq!(failed(&report), vec![REGISTERED]);
    }
}
