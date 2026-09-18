// SPDX-License-Identifier: AGPL-3.0-or-later
//! The HTTP routes of `blighted-admin` (admin tools spec sections 3 and 7).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use serde::Deserialize;

use crate::assets::asset;
use crate::captures::Captures;
use crate::captures::api::capture_routes;
use crate::captures::catalog::search;
use crate::guard::{AdminOrigin, admin_guard};
use crate::live::Live;
use crate::live::bots::bot_routes;
use crate::live::stream::live_routes;
use crate::relief::relief_tile;
use crate::studio::Studio;
use crate::studio::api::studio_routes;
use crate::world::{SpawnKind, SpawnQuery, WorldLayer};
use nw_world::map_tiles::tile_file;

/// Content type of map tiles.
pub const PNG: &str = "image/png";
const INDEX: &str = "index.html";
const LIVE_PAGE: &str = "live.html";
/// The live page.
pub const LIVE_PAGE_ROUTE: &str = "/live";

/// What every route shares.
#[derive(Debug, Clone)]
pub struct AppState {
    world: Arc<WorldLayer>,
    map_dir: Arc<PathBuf>,
    origin: AdminOrigin,
    pub(crate) captures: Arc<Captures>,
    pub(crate) live: Option<Live>,
    pub(crate) studio: Option<Studio>,
}

impl AppState {
    /// The state of a server on `listen` with `world`, the map tiles in `map_dir`, the capture
    /// sessions, and the live panel when configured.
    pub fn new(
        world: WorldLayer,
        map_dir: PathBuf,
        listen: SocketAddr,
        captures: Captures,
        live: Option<Live>,
    ) -> Self {
        Self {
            world: Arc::new(world),
            map_dir: Arc::new(map_dir),
            origin: AdminOrigin::of(listen),
            captures: Arc::new(captures),
            live,
            studio: None,
        }
    }

    /// The same state with the studio routes served by `studio`.
    pub fn with_studio(self, studio: Studio) -> Self {
        Self {
            studio: Some(studio),
            ..self
        }
    }
}

/// Every route, behind [`admin_guard`].
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(|| web_file(UrlPath(INDEX.to_owned()))))
        .route(
            LIVE_PAGE_ROUTE,
            get(|| web_file(UrlPath(LIVE_PAGE.to_owned()))),
        )
        .route("/web/{file}", get(web_file))
        .route("/api/world/info", get(world_info))
        .route("/api/world/tiles/{level}/{row}/{column}", get(map_tile))
        .route("/api/world/relief/{level}/{row}/{column}", get(relief))
        .route("/api/world/spawns", get(spawns))
        .route("/api/captures/sessions", get(sessions))
        .route("/api/captures/types/{idx}", get(type_label))
        .merge(capture_routes())
        .merge(live_routes())
        .merge(bot_routes())
        .merge(studio_routes())
        .layer(axum::middleware::from_fn_with_state(
            state.origin.clone(),
            admin_guard,
        ))
        .with_state(state)
}

async fn web_file(UrlPath(file): UrlPath<String>) -> Response {
    match asset(&file) {
        Some(found) => ([(header::CONTENT_TYPE, found.content_type)], found.body).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn world_info(State(state): State<AppState>) -> Response {
    Json(state.world.info()).into_response()
}

async fn map_tile(
    State(state): State<AppState>,
    UrlPath((level, row, column)): UrlPath<(u8, u32, u32)>,
) -> Response {
    let path = tile_file(&state.map_dir, level, row, column);
    match tokio::fs::read(&path).await {
        Ok(bytes) => png_response(bytes),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn relief(
    State(state): State<AppState>,
    UrlPath((level, row, column)): UrlPath<(u8, u32, u32)>,
) -> Response {
    let world = Arc::clone(&state.world);
    let drawn = tokio::task::spawn_blocking(move || {
        relief_tile(world.world(), world.tiles(), level, row, column)
    })
    .await;
    match drawn {
        Ok(Some(bytes)) => png_response(bytes),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct SpawnParams {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    kinds: String,
}

async fn spawns(State(state): State<AppState>, Query(params): Query<SpawnParams>) -> Response {
    let answer = SpawnKind::parse_list(&params.kinds).and_then(|kinds| {
        state.world.spawns(&SpawnQuery {
            min_x: params.min_x,
            min_y: params.min_y,
            max_x: params.max_x,
            max_y: params.max_y,
            kinds,
        })
    });
    match answer {
        Ok(points) => Json(points).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct SessionParams {
    #[serde(default)]
    q: String,
}

async fn sessions(State(state): State<AppState>, Query(params): Query<SessionParams>) -> Response {
    Json(search(&state.captures.sessions, &params.q)).into_response()
}

async fn type_label(State(state): State<AppState>, UrlPath(idx): UrlPath<u64>) -> Response {
    Json(state.captures.labels.label(idx)).into_response()
}

fn png_response(bytes: Vec<u8>) -> Response {
    ([(header::CONTENT_TYPE, PNG)], bytes).into_response()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;
    use crate::captures::Captures;
    use crate::captures::catalog::SessionEntry;
    use crate::world::tests::{REGION_M, test_layer};

    const CAP: usize = 5_000;
    const LISTEN: &str = "127.0.0.1:9180";

    fn app(map_dir: &Path) -> Router {
        app_with(map_dir, Captures::default())
    }

    fn app_with(map_dir: &Path, captures: Captures) -> Router {
        router(AppState::new(
            test_layer(CAP),
            map_dir.to_path_buf(),
            LISTEN.parse().expect("address"),
            captures,
            None,
        ))
    }

    async fn get(app: Router, uri: &str) -> (StatusCode, Option<String>, Vec<u8>) {
        let response = app
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header(header::HOST, LISTEN)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        (status, content_type, body.to_vec())
    }

    #[tokio::test]
    async fn the_index_and_web_files_are_served() {
        let dir = tempfile::tempdir().expect("dir");
        let (status, content_type, body) = get(app(dir.path()), "/").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type.as_deref(), Some(crate::assets::HTML));
        assert!(String::from_utf8_lossy(&body).contains("<html"));
        let (status, content_type, _) = get(app(dir.path()), "/web/style.css").await;
        assert_eq!(
            (status, content_type.as_deref()),
            (StatusCode::OK, Some(crate::assets::CSS))
        );
    }

    #[tokio::test]
    async fn the_live_page_is_served_at_live() {
        let dir = tempfile::tempdir().expect("dir");
        let (status, content_type, body) = get(app(dir.path()), LIVE_PAGE_ROUTE).await;
        assert_eq!(
            (status, content_type.as_deref()),
            (StatusCode::OK, Some(crate::assets::HTML))
        );
        assert!(String::from_utf8_lossy(&body).contains("/web/live.js"));
    }

    #[tokio::test]
    async fn an_unknown_web_file_is_404() {
        let dir = tempfile::tempdir().expect("dir");
        let (status, _, _) = get(app(dir.path()), "/web/secret.txt").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn info_gives_the_world_size() {
        let dir = tempfile::tempdir().expect("dir");
        let (status, _, body) = get(app(dir.path()), "/api/world/info").await;
        assert_eq!(status, StatusCode::OK);
        let info: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(info["world_m"], REGION_M);
    }

    #[tokio::test]
    async fn a_missing_tile_is_404_and_a_present_one_is_png() {
        let dir = tempfile::tempdir().expect("dir");
        let (status, _, _) = get(app(dir.path()), "/api/world/tiles/1/0/0").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        std::fs::create_dir_all(dir.path().join("1")).expect("dir");
        std::fs::write(dir.path().join("1").join("0_0.png"), b"png bytes").expect("write");
        let (status, content_type, body) = get(app(dir.path()), "/api/world/tiles/1/0/0").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type.as_deref(), Some(PNG));
        assert_eq!(body, b"png bytes");
    }

    #[tokio::test]
    async fn relief_tiles_are_png_and_outside_ones_404() {
        let dir = tempfile::tempdir().expect("dir");
        let (status, content_type, _) = get(app(dir.path()), "/api/world/relief/4/0/0").await;
        assert_eq!(
            (status, content_type.as_deref()),
            (StatusCode::OK, Some(PNG))
        );
        let (status, _, _) = get(app(dir.path()), "/api/world/relief/4/5/0").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn spawns_come_as_json_and_a_bad_box_is_400() {
        let dir = tempfile::tempdir().expect("dir");
        let (status, _, body) = get(
            app(dir.path()),
            "/api/world/spawns?min_x=0&min_y=0&max_x=2048&max_y=2048&kinds=vitals",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let points: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(points.as_array().map(Vec::len), Some(1));
        let (status, _, _) = get(
            app(dir.path()),
            "/api/world/spawns?min_x=9&min_y=0&max_x=1&max_y=2048&kinds=vitals",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn sessions_are_listed_and_searched() {
        let dir = tempfile::tempdir().expect("dir");
        let entry = |id: &str| SessionEntry {
            id: id.to_owned(),
            path: dir.path().join(id),
            bytes: 1,
            name: None,
            started_at: None,
            post_title: None,
        };
        let captures = Captures {
            sessions: vec![entry("dummy/ledger.bin"), entry("raid/ledger.bin")],
            ..Captures::default()
        };
        let (status, _, body) = get(
            app_with(dir.path(), captures),
            "/api/captures/sessions?q=raid",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let sessions: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(sessions[0]["id"], "raid/ledger.bin");
        assert_eq!(sessions.as_array().map(Vec::len), Some(1));
        assert!(sessions[0].get("path").is_none(), "no local path is sent");
    }

    #[tokio::test]
    async fn a_type_label_comes_as_json() {
        let dir = tempfile::tempdir().expect("dir");
        let (status, _, body) = get(app(dir.path()), "/api/captures/types/8").await;
        assert_eq!(status, StatusCode::OK);
        let label: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert!(label["name"].is_null());
    }
}
