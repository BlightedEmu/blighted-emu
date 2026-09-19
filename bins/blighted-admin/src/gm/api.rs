// SPDX-License-Identifier: AGPL-3.0-or-later
//! The GM routes (admin studio spec section 8.2): each checks the rank on the server, sends the
//! command to the owning sim, and writes an audit row.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::{Extension, Router};
use nw_bus::{AdminAction, AdminCommand, AdminReply, Point};
use nw_db::AdminRank;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::access::session::Actor;
use crate::gm::SendError;
use crate::live::lock;
use crate::server::AppState;

/// `POST`: moves a player.
pub const TELEPORT_ROUTE: &str = "/api/gm/teleport";
/// `GET`: the audit log.
pub const AUDIT_ROUTE: &str = "/api/gm/audit";
/// Lowest rank that may move another player.
pub const TELEPORT_RANK: AdminRank = AdminRank::GameMaster;
/// Audit rows a page gets by default, and at most.
const AUDIT_DEFAULT_ROWS: i64 = 100;
const AUDIT_MAX_ROWS: i64 = 1000;

/// The GM routes.
pub fn gm_routes() -> Router<AppState> {
    Router::new()
        .route(TELEPORT_ROUTE, post(teleport))
        .route(AUDIT_ROUTE, get(audit))
}

/// What the page sends to move a player.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TeleportRequest {
    /// The player's entity number.
    pub entity: u64,
    /// East, metres.
    pub x: f32,
    /// North, metres.
    pub y: f32,
    /// Up, metres.
    pub z: f32,
}

#[derive(Debug, Deserialize)]
struct AuditQuery {
    limit: Option<i64>,
}

async fn teleport(
    State(state): State<AppState>,
    Extension(actor): Extension<Actor>,
    Json(request): Json<TeleportRequest>,
) -> Response {
    let target = format!("player {}", request.entity);
    let detail = serde_json::to_value(request).unwrap_or_default();
    let record = |result: String| {
        let access = state.access.clone();
        let actor = actor.clone();
        let target = target.clone();
        let detail = detail.clone();
        async move {
            access
                .record(&actor, "teleport", &target, detail, &result)
                .await
        }
    };
    let (Some(commands), Some(live)) = (&state.commands, &state.live) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if actor.rank < TELEPORT_RANK {
        record("refused: rank".to_owned()).await;
        return (StatusCode::FORBIDDEN, "this needs the game master rank").into_response();
    }
    let Some(sim) = lock(live.state()).sim_of_player(request.entity) else {
        record("not_found".to_owned()).await;
        return (StatusCode::NOT_FOUND, "no sim shows this player").into_response();
    };
    let command = AdminCommand {
        request: Uuid::new_v4(),
        actor: actor.account,
        action: AdminAction::TeleportPlayer {
            entity: request.entity,
            to: Point {
                x: request.x,
                y: request.y,
                z: request.z,
            },
        },
    };
    let (status, result) = match commands.send(&sim, &command).await {
        Ok(AdminReply::Done) => (StatusCode::OK, "done".to_owned()),
        Ok(AdminReply::NotMine) => (
            StatusCode::CONFLICT,
            "not_mine: the player moved to another sim; try again".to_owned(),
        ),
        Ok(AdminReply::Failed { reason }) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("failed: {reason}"),
        ),
        Err(SendError::Timeout) => (StatusCode::GATEWAY_TIMEOUT, "timeout".to_owned()),
        Err(SendError::Bus(error)) => (StatusCode::BAD_GATEWAY, format!("bus: {error}")),
    };
    record(result.clone()).await;
    (status, result).into_response()
}

async fn audit(
    State(state): State<AppState>,
    Extension(actor): Extension<Actor>,
    Query(query): Query<AuditQuery>,
) -> Response {
    let Some(store) = state.access.store() else {
        return Json(Vec::<nw_db::AuditRow>::new()).into_response();
    };
    let limit = query
        .limit
        .unwrap_or(AUDIT_DEFAULT_ROWS)
        .clamp(1, AUDIT_MAX_ROWS);
    let own = (actor.rank < AdminRank::HeadAdmin)
        .then_some(actor.account)
        .flatten();
    match store.audit_rows(own, limit).await {
        Ok(rows) => Json(rows).into_response(),
        Err(error) => {
            tracing::warn!(%error, "audit rows");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::body::Body;
    use axum::http::{Method, Request, header};
    use nw_bus::{BusId, SimSnapshot, SnapPlayer};
    use tower::ServiceExt;

    use super::*;
    use crate::access::session::Actor;
    use crate::access::store::Store;
    use crate::access::store::memory::{MemoryStore, lock as lock_store};
    use crate::access::{Access, AccessMode};
    use crate::captures::Captures;
    use crate::gm::fake::{FakeSims, commands};
    use crate::guard::{ADMIN_HEADER, ADMIN_HEADER_VALUE, AdminOrigin};
    use crate::live::bots::idle_bots;
    use crate::live::{Live, shared};
    use crate::server::router;
    use crate::world::tests::test_layer;

    const CAP: usize = 5_000;
    const LISTEN: &str = "127.0.0.1:9180";
    const PLAYER: u64 = 41;
    const SIM: &str = "sim-a";

    struct Fixture {
        sims: Arc<Mutex<FakeSims>>,
        store: Arc<Mutex<MemoryStore>>,
        state: AppState,
    }

    fn fixture() -> Fixture {
        let live_state = shared();
        lock(&live_state).snapshot_part(SimSnapshot {
            sim: BusId::try_from(SIM.to_owned()).expect("id"),
            time_ms: 1,
            part: 0,
            parts: 1,
            players: vec![SnapPlayer {
                entity: PLAYER,
                name: "Bot".to_owned(),
                x: 1.0,
                y: 2.0,
                z: 3.0,
                dead: false,
                frozen: false,
            }],
            awake_mobs: Vec::new(),
            mob_counts: None,
        });
        let live = Live::new(
            live_state.clone(),
            std::time::Duration::from_millis(1),
            idle_bots(live_state),
            tokio::sync::watch::channel(false).1,
        );
        let sims = Arc::new(Mutex::new(FakeSims::default()));
        let store = Arc::new(Mutex::new(MemoryStore::default()));
        let listen = LISTEN.parse().expect("address");
        let state = AppState::new(
            test_layer(CAP),
            std::env::temp_dir(),
            listen,
            Captures::default(),
            Some(live),
        )
        .with_access(
            Access::new(AccessMode::Local, Some(Store::Memory(store.clone())), None),
            AdminOrigin::of(listen),
        )
        .with_commands(commands(&sims));
        Fixture { sims, store, state }
    }

    fn teleport_request(entity: u64) -> Request<Body> {
        Request::builder()
            .method(Method::POST)
            .uri(TELEPORT_ROUTE)
            .header(header::HOST, LISTEN)
            .header(ADMIN_HEADER, ADMIN_HEADER_VALUE)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({ "entity": entity, "x": 10.0, "y": 20.0, "z": 30.0 })
                    .to_string(),
            ))
            .expect("request")
    }

    async fn status(fixture: &Fixture, request: Request<Body>) -> StatusCode {
        router(fixture.state.clone())
            .oneshot(request)
            .await
            .expect("response")
            .status()
    }

    fn results(fixture: &Fixture) -> Vec<String> {
        lock_store(&fixture.store)
            .audit
            .iter()
            .map(|entry| entry.result.clone())
            .collect()
    }

    #[tokio::test]
    async fn a_teleport_goes_to_the_owning_sim_and_is_audited() {
        let fixture = fixture();
        fixture
            .sims
            .lock()
            .expect("sims")
            .answers
            .push_back(Some(AdminReply::Done));
        assert_eq!(
            status(&fixture, teleport_request(PLAYER)).await,
            StatusCode::OK
        );
        let sent = &fixture.sims.lock().expect("sims").sent;
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0.to_string(), SIM);
        assert!(matches!(
            sent[0].1.action,
            AdminAction::TeleportPlayer { entity: PLAYER, .. }
        ));
        assert_eq!(results(&fixture), ["done"]);
    }

    #[tokio::test]
    async fn not_mine_and_timeout_are_errors_with_audit_rows() {
        let fixture = fixture();
        {
            let mut sims = fixture.sims.lock().expect("sims");
            sims.answers.push_back(Some(AdminReply::NotMine));
            sims.answers.push_back(None);
        }
        assert_eq!(
            status(&fixture, teleport_request(PLAYER)).await,
            StatusCode::CONFLICT
        );
        assert_eq!(
            status(&fixture, teleport_request(PLAYER)).await,
            StatusCode::GATEWAY_TIMEOUT
        );
        let results = results(&fixture);
        assert!(results[0].starts_with("not_mine"));
        assert_eq!(results[1], "timeout");
    }

    #[tokio::test]
    async fn a_low_rank_is_refused_without_a_bus_request() {
        let fixture = fixture();
        let csr = Actor {
            account: Some(Uuid::from_u128(9)),
            username: "csr".to_owned(),
            rank: AdminRank::Csr,
        };
        let response = gm_routes()
            .layer(Extension(csr))
            .with_state(fixture.state.clone())
            .oneshot(teleport_request(PLAYER))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(fixture.sims.lock().expect("sims").sent.is_empty());
        assert_eq!(results(&fixture), ["refused: rank"]);
    }

    #[tokio::test]
    async fn an_unknown_player_is_not_found() {
        let fixture = fixture();
        assert_eq!(
            status(&fixture, teleport_request(PLAYER + 1)).await,
            StatusCode::NOT_FOUND
        );
        assert!(fixture.sims.lock().expect("sims").sent.is_empty());
    }
}
