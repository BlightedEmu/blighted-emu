// SPDX-License-Identifier: AGPL-3.0-or-later
//! Login queue HTTP routes.
//!
//! Observed paths:
//! `/game/login/queue/v2`
//! `/game/login/queue/v2/jwt`
//! `/game/login/queue/v2/jwt/omni`
//! `/game/login/queue/v2/{ticket}`
//! `/game/login/queue/v2/{ticket}/jwt/omni`
//!
//! The refresh path without `jwt/omni` is not listed for the ticket operation,
//! so this server does not add `/game/login/queue/v2/{ticket}/jwt`.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Router, extract::DefaultBodyLimit};
use metrics_exporter_prometheus::PrometheusHandle;
use nw_protocol::LoginQueueReply;

use crate::characters::{CharacterLookup, LookupError};
use crate::queue::{LoginQueue, QueueError, QueueSettings, prepare_join};
use crate::request::{MAX_ID_LEN, parse_request};

/// Join path.
pub(crate) const PATH_QUEUE: &str = "/game/login/queue/v2";

/// Join path with the `jwt` suffix listed beside the Omni path.
pub(crate) const PATH_QUEUE_JWT: &str = "/game/login/queue/v2/jwt";

/// Omni join path.
pub(crate) const PATH_QUEUE_OMNI: &str = "/game/login/queue/v2/jwt/omni";

/// Refresh path. `{ticket}` is the ticket id.
pub(crate) const PATH_REFRESH: &str = "/game/login/queue/v2/{ticket}";

/// Omni refresh path.
pub(crate) const PATH_REFRESH_OMNI: &str = "/game/login/queue/v2/{ticket}/jwt/omni";

/// Prometheus scrape path.
pub(crate) const PATH_METRICS: &str = "/metrics";

const MAX_JSON_BODY_BYTES: usize = 64 * 1024;
const METRIC_JOIN: &str = "nw_login_queue_join_total";
const METRIC_REFRESH: &str = "nw_login_queue_refresh_total";
const ERROR_INVALID_JSON: &str = "invalid_json";
const ERROR_INVALID_REQUEST: &str = "invalid_request";
const ERROR_WORLD_MISMATCH: &str = "world_mismatch";
const ERROR_UNKNOWN_CHARACTER: &str = "unknown_character";
const ERROR_UNKNOWN_TICKET: &str = "unknown_ticket";
const ERROR_UNAVAILABLE: &str = "unavailable";
const METRICS_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// Shared state for the login routes.
#[derive(Clone)]
pub(crate) struct AppState<Q, C> {
    /// Queue store.
    pub queue: Q,
    /// Character records.
    pub characters: C,
    /// Queue limits and `RepAddress`.
    pub settings: Arc<QueueSettings>,
}

/// Login routes. `metrics` adds the Prometheus scrape route when present.
pub(crate) fn router<Q, C>(state: AppState<Q, C>, metrics: Option<PrometheusHandle>) -> Router
where
    Q: LoginQueue,
    C: CharacterLookup,
{
    let mut routes = Router::new()
        .route(PATH_QUEUE, post(join))
        .route(PATH_QUEUE_JWT, post(join))
        .route(PATH_QUEUE_OMNI, post(join))
        .route(PATH_REFRESH_OMNI, post(refresh))
        .route(PATH_REFRESH, post(refresh));
    if let Some(handle) = metrics {
        routes = routes.route(
            PATH_METRICS,
            get(move || {
                let handle = handle.clone();
                async move {
                    (
                        [(
                            header::CONTENT_TYPE,
                            HeaderValue::from_static(METRICS_CONTENT_TYPE),
                        )],
                        handle.render(),
                    )
                }
            }),
        );
    }
    routes
        .layer(DefaultBodyLimit::max(MAX_JSON_BODY_BYTES))
        .with_state(state)
}

async fn join<Q, C>(
    State(state): State<AppState<Q, C>>,
    body: Bytes,
) -> Result<Json<LoginQueueReply>, ApiError>
where
    Q: LoginQueue,
    C: CharacterLookup,
{
    metrics::counter!(METRIC_JOIN).increment(1);
    let request = parse_request(&body).map_err(|_| ApiError::InvalidJson)?;
    let prepared = prepare_join(request, &state.settings).map_err(map_queue)?;
    if let Some(character_id) = &prepared.character_id {
        let exists = state
            .characters
            .exists(&prepared.world_id, character_id)
            .await
            .map_err(map_lookup)?;
        if !exists {
            return Err(ApiError::UnknownCharacter);
        }
    }
    let response = state
        .queue
        .join(prepared, unix_now())
        .await
        .map_err(map_queue)?;
    Ok(Json(LoginQueueReply {
        login_queue_response: response,
    }))
}

async fn refresh<Q, C>(
    State(state): State<AppState<Q, C>>,
    Path(ticket_id): Path<String>,
    _body: Bytes,
) -> Result<Json<LoginQueueReply>, ApiError>
where
    Q: LoginQueue,
    C: CharacterLookup,
{
    metrics::counter!(METRIC_REFRESH).increment(1);
    if !valid_ticket_id(&ticket_id) {
        return Err(ApiError::InvalidRequest);
    }
    let response = state
        .queue
        .refresh(&ticket_id, unix_now())
        .await
        .map_err(map_queue)?;
    Ok(Json(LoginQueueReply {
        login_queue_response: response,
    }))
}

fn unix_now() -> i64 {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    i64::try_from(seconds).unwrap_or(i64::MAX)
}

fn valid_ticket_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID_LEN
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn map_queue(error: QueueError) -> ApiError {
    match error {
        QueueError::WorldMismatch => ApiError::WorldMismatch,
        QueueError::InvalidRequest => ApiError::InvalidRequest,
        QueueError::UnknownTicket => ApiError::UnknownTicket,
        QueueError::Signature(_) | QueueError::Unavailable(_) => {
            tracing::error!(%error, "login queue failed");
            ApiError::Unavailable
        }
    }
}

fn map_lookup(error: LookupError) -> ApiError {
    tracing::error!(%error, "character lookup failed");
    ApiError::Unavailable
}

enum ApiError {
    InvalidJson,
    InvalidRequest,
    WorldMismatch,
    UnknownCharacter,
    UnknownTicket,
    Unavailable,
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            Self::InvalidJson | Self::InvalidRequest | Self::WorldMismatch => {
                StatusCode::BAD_REQUEST
            }
            Self::UnknownCharacter | Self::UnknownTicket => StatusCode::NOT_FOUND,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::InvalidJson => ERROR_INVALID_JSON,
            Self::InvalidRequest => ERROR_INVALID_REQUEST,
            Self::WorldMismatch => ERROR_WORLD_MISMATCH,
            Self::UnknownCharacter => ERROR_UNKNOWN_CHARACTER,
            Self::UnknownTicket => ERROR_UNKNOWN_TICKET,
            Self::Unavailable => ERROR_UNAVAILABLE,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = Json(ErrorBody { error: self.code() });
        (status, body).into_response()
    }
}

#[derive(serde::Serialize)]
struct ErrorBody {
    error: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use crate::memory::MemoryQueue;

    const WORLD: &str = "world-1";
    const CHARACTER: &str = "char-1";
    const REP_ADDRESS: &str = "127.0.0.1:33435";
    const CAPACITY: u32 = 10;

    #[derive(Clone)]
    struct FixedCharacters {
        ids: HashSet<(String, String)>,
    }

    impl CharacterLookup for FixedCharacters {
        async fn exists(&self, world_id: &str, character_id: &str) -> Result<bool, LookupError> {
            Ok(self
                .ids
                .contains(&(world_id.to_owned(), character_id.to_owned())))
        }
    }

    fn settings() -> Arc<QueueSettings> {
        Arc::new(QueueSettings {
            world_id: WORLD.to_owned(),
            queue_name: WORLD.to_owned(),
            capacity: CAPACITY,
            refresh_interval_seconds: 5,
            estimated_seconds_per_position: 5,
            rep_address: REP_ADDRESS.to_owned(),
            signature_key: b"change-me-signature-key".to_vec(),
        })
    }

    fn test_router() -> Router {
        let settings = settings();
        let queue = MemoryQueue::new(Arc::clone(&settings));
        let characters = FixedCharacters {
            ids: HashSet::from([(WORLD.to_owned(), CHARACTER.to_owned())]),
        };
        router(
            AppState {
                queue,
                characters,
                settings,
            },
            None,
        )
    }

    async fn post_json(app: Router, path: &str, body: &str) -> (StatusCode, serde_json::Value) {
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_owned()))
                    .expect("request"),
            )
            .await
            .expect("response");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, value)
    }

    #[tokio::test]
    async fn omni_join_puts_rep_address_on_the_token_and_omits_absent_keys() {
        let body = format!(
            r#"{{"LoginQueueRequest":{{"CharacterId":"{CHARACTER}","ClientCapabilities":["move"],"WorldId":"{WORLD}"}}}}"#
        );
        let (status, value) = post_json(test_router(), PATH_QUEUE_OMNI, &body).await;
        assert_eq!(status, StatusCode::OK);
        let response = &value["LoginQueueResponse"];
        assert!(response.get("RepAddress").is_none());
        assert!(response.get("Signature").is_none());
        assert!(response.get("RecommendedTransferWorldId").is_none());
        let token = &response["Token"];
        assert_eq!(token["RepAddress"], REP_ADDRESS);
        assert!(
            token["Signature"]
                .as_str()
                .is_some_and(|text| !text.is_empty())
        );
        assert!(token.get("ClientCapabilities").is_none());
        assert!(token.get("AccountAge").is_none());
        assert!(token.get("SteamUserId").is_none());
        assert_eq!(token["CharacterId"], CHARACTER);
        assert_eq!(token["WorldId"], WORLD);
    }

    #[tokio::test]
    async fn bare_and_jwt_paths_join_and_refresh_uses_the_ticket() {
        let app = test_router();
        let (status, value) =
            post_json(app.clone(), PATH_QUEUE, r#"{"LoginQueueRequest":{}}"#).await;
        assert_eq!(status, StatusCode::OK);
        let ticket = value["LoginQueueResponse"]["Token"]["TicketId"]
            .as_str()
            .expect("ticket")
            .to_owned();
        let (jwt_status, _) = post_json(app.clone(), PATH_QUEUE_JWT, "").await;
        assert_eq!(jwt_status, StatusCode::OK);
        let refresh = format!("/game/login/queue/v2/{ticket}/jwt/omni");
        let (refresh_status, refreshed) = post_json(app, &refresh, "").await;
        assert_eq!(refresh_status, StatusCode::OK);
        assert_eq!(refreshed["LoginQueueResponse"]["Token"]["TicketId"], ticket);
    }

    #[tokio::test]
    async fn unknown_character_is_not_found() {
        let body = r#"{"LoginQueueRequest":{"CharacterId":"missing","WorldId":"world-1"}}"#;
        let (status, value) = post_json(test_router(), PATH_QUEUE, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(value["error"], ERROR_UNKNOWN_CHARACTER);
    }

    #[tokio::test]
    async fn bad_json_is_rejected_and_get_is_not_allowed() {
        let (status, value) = post_json(test_router(), PATH_QUEUE, "not-json").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(value["error"], ERROR_INVALID_JSON);
        let response = test_router()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(PATH_QUEUE)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }
}
