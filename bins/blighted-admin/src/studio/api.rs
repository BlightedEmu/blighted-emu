// SPDX-License-Identifier: AGPL-3.0-or-later
//! The studio routes: the design files, the full check, and Apply.

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use nw_design::{DesignError, DesignFile, DesignMode, check_schema, load_dir};
use serde::Serialize;

use crate::server::AppState;
use crate::studio::Studio;

/// The design files with their schema problems.
pub const FILES_ROUTE: &str = "/api/studio/files";
/// The full check; writes nothing.
pub const CHECK_ROUTE: &str = "/api/studio/check";
/// `GET`: the Apply state; `POST`: starts Apply.
pub const APPLY_ROUTE: &str = "/api/studio/apply";

/// The studio routes; every `POST` needs the admin header (the guard).
pub fn studio_routes() -> Router<AppState> {
    Router::new()
        .route(FILES_ROUTE, get(files))
        .route(CHECK_ROUTE, post(check))
        .route(APPLY_ROUTE, get(apply_status).post(start_apply))
}

/// What the files route returns.
#[derive(Debug, Serialize)]
struct FilesView {
    files: Vec<DesignFile>,
    errors: Vec<DesignError>,
}

fn studio_of(state: &AppState) -> Option<&Studio> {
    state.studio.as_ref()
}

async fn files(State(state): State<AppState>) -> Response {
    let Some(studio) = studio_of(&state) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let design_dir = studio.paths.design_dir.clone();
    let loaded = tokio::task::spawn_blocking(move || {
        let (set, mut errors) = load_dir(&design_dir);
        errors.extend(check_schema(&set));
        FilesView {
            files: set.files,
            errors,
        }
    })
    .await;
    match loaded {
        Ok(view) => Json(view).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn check(State(state): State<AppState>) -> Response {
    let Some(studio) = studio_of(&state) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match studio.design(DesignMode::Check).await {
        Ok(report) => Json(report).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error).into_response(),
    }
}

async fn apply_status(State(state): State<AppState>) -> Response {
    match studio_of(&state) {
        Some(studio) => Json(studio.apply_status()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn start_apply(State(state): State<AppState>) -> Response {
    match studio_of(&state) {
        Some(studio) if studio.start_apply() => StatusCode::ACCEPTED.into_response(),
        Some(_) => (StatusCode::CONFLICT, "an Apply runs").into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::{Method, Request, header};
    use http_body_util::BodyExt;
    use nw_bus::{BusId, SimHeartbeat};
    use nw_common::time::unix_ms;
    use nw_content::ContentBundle;
    use nw_content::bundle::bundle_path;
    use nw_world::WorldFile;
    use nw_world::records::{LevelInfo, VitalsSpawn};
    use nw_world::world_file::world_file_path;
    use serde_json::Value;
    use tower::ServiceExt;

    use super::*;
    use crate::captures::Captures;
    use crate::config::StudioConfig;
    use crate::guard::{ADMIN_HEADER, ADMIN_HEADER_VALUE};
    use crate::live::{SharedState, lock, shared};
    use crate::server::{AppState, router};
    use crate::studio::Studio;
    use crate::world::tests::test_layer;

    const CAP: usize = 5_000;
    const LISTEN: &str = "127.0.0.1:9180";
    const BUILD: &str = "22469132";
    const MAP: &str = "lv";
    const SIM: &str = "sim-1";
    const POLLS: usize = 200;
    const POLL: Duration = Duration::from_millis(50);
    const SHORT_TIMEOUT_S: u64 = 1;
    const LONG_TIMEOUT_S: u64 = 30;

    struct Fixture {
        root: tempfile::TempDir,
        live: SharedState,
        studio: Studio,
    }

    impl Fixture {
        fn path(&self, relative: &str) -> PathBuf {
            self.root.path().join(relative)
        }

        fn app(&self) -> axum::Router {
            router(
                AppState::new(
                    test_layer(CAP),
                    std::env::temp_dir(),
                    LISTEN.parse().expect("address"),
                    Captures::default(),
                    None,
                )
                .with_studio(self.studio.clone()),
            )
        }

        fn spawn_file(&self, key: &str, fields: &str) {
            let path = self.path(&format!("content/spawns/{MAP}/{key}.toml"));
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
            std::fs::write(
                path,
                format!("schema = 1\nkind = \"spawn_point\"\naction = \"override\"\nid = \"{key}\"\nmap = \"{MAP}\"\n{fields}\n"),
            )
            .expect("write");
        }

        fn merged_bundle(&self) -> PathBuf {
            bundle_path(&self.path("out"), BUILD)
        }
    }

    fn world() -> WorldFile {
        let mut world = WorldFile {
            level: LevelInfo {
                name: MAP.to_owned(),
                region_size: 256,
                grid_width: 4,
                grid_height: 4,
                ..LevelInfo::default()
            },
            vitals: vec![VitalsSpawn {
                vitals_id: 1,
                level: 5,
                position: [1.0, 2.0, 3.0],
                ..VitalsSpawn::default()
            }],
            ..WorldFile::default()
        };
        world.fill_point_keys();
        world
    }

    fn key() -> String {
        world().vitals_keys[0].clone()
    }

    fn fixture(fifo: bool, restart_timeout_s: u64) -> Fixture {
        let root = tempfile::tempdir().expect("dir");
        let retail = root.path().join("retail");
        ContentBundle {
            client_build: BUILD.to_owned(),
            ..ContentBundle::default()
        }
        .save(&bundle_path(&retail, BUILD))
        .expect("bundle");
        world()
            .save(&world_file_path(&retail, BUILD, MAP), BUILD)
            .expect("world");
        let control_fifo = root.path().join("control");
        if fifo {
            let status = std::process::Command::new("mkfifo")
                .arg(&control_fifo)
                .status()
                .expect("mkfifo");
            assert!(status.success());
        }
        let config = StudioConfig {
            design_dir: root.path().join("content"),
            retail_dir: retail,
            control_fifo,
            restart_timeout_s,
        };
        let live = shared();
        let studio = Studio::new(&config, root.path().join("out"), BUILD, Some(live.clone()));
        Fixture { root, live, studio }
    }

    fn beat(started_at_ms: u64) -> SimHeartbeat {
        SimHeartbeat {
            id: BusId::try_from(SIM.to_owned()).expect("id"),
            started_at_ms,
            players: 0,
            max_players: 1,
            level: MAP.to_owned(),
        }
    }

    /// Reads the control FIFO like `live-stack.sh` (opened read-write), and calls `on_line`
    /// for each line.
    fn read_control(path: &Path, on_line: impl Fn(&str) + Send + 'static) {
        let fifo = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .expect("open fifo");
        std::thread::spawn(move || {
            for line in BufReader::new(fifo).lines() {
                on_line(&line.expect("line"));
            }
        });
    }

    fn request(method: Method, uri: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, LISTEN)
            .header(ADMIN_HEADER, ADMIN_HEADER_VALUE)
            .body(Body::empty())
            .expect("request")
    }

    async fn call(fixture: &Fixture, method: Method, uri: &str) -> (StatusCode, Value) {
        let response = fixture
            .app()
            .oneshot(request(method, uri))
            .await
            .expect("response");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// The Apply state once it is no longer running.
    async fn finished(fixture: &Fixture) -> Value {
        for _ in 0..POLLS {
            let (_, status) = call(fixture, Method::GET, APPLY_ROUTE).await;
            if status["phase"] != "running" {
                return status;
            }
            tokio::time::sleep(POLL).await;
        }
        panic!("apply still running");
    }

    #[tokio::test]
    async fn files_lists_design_files_and_errors() {
        let fixture = fixture(false, LONG_TIMEOUT_S);
        fixture.spawn_file(&key(), "level = 30");
        fixture.spawn_file("1_0.0_0.0_0.0", "level = 66");
        let (status, body) = call(&fixture, Method::GET, FILES_ROUTE).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["files"].as_array().map(Vec::len), Some(2));
        assert_eq!(body["errors"].as_array().map(Vec::len), Some(1));
        assert_eq!(body["errors"][0]["problem"]["code"], "out_of_range");
    }

    #[tokio::test]
    async fn check_reports_unknown_ids_without_writing() {
        let fixture = fixture(false, LONG_TIMEOUT_S);
        fixture.spawn_file("1_0.0_0.0_0.0", "level = 30");
        let (status, body) = call(&fixture, Method::POST, CHECK_ROUTE).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["errors"][0]["problem"]["code"], "unknown_id");
        assert_eq!(body["written"], false);
        assert!(!fixture.merged_bundle().exists());
    }

    #[tokio::test]
    async fn apply_writes_and_finishes_without_fifo() {
        let fixture = fixture(false, LONG_TIMEOUT_S);
        fixture.spawn_file(&key(), "level = 30");
        let (status, _) = call(&fixture, Method::POST, APPLY_ROUTE).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let end = finished(&fixture).await;
        assert_eq!(end["phase"], "done", "{end}");
        assert!(
            end["detail"]
                .as_str()
                .unwrap_or_default()
                .contains("control")
        );
        assert!(fixture.merged_bundle().exists());
    }

    #[tokio::test]
    async fn apply_with_errors_fails_and_writes_nothing() {
        let fixture = fixture(false, LONG_TIMEOUT_S);
        fixture.spawn_file("1_0.0_0.0_0.0", "level = 30");
        call(&fixture, Method::POST, APPLY_ROUTE).await;
        let end = finished(&fixture).await;
        assert_eq!(end["phase"], "failed");
        assert_eq!(end["step"], "check");
        assert_eq!(end["errors"][0]["problem"]["code"], "unknown_id");
        assert!(!fixture.merged_bundle().exists());
    }

    #[tokio::test]
    async fn a_second_apply_while_running_is_refused() {
        let fixture = fixture(true, SHORT_TIMEOUT_S);
        read_control(&fixture.path("control"), |_| {});
        lock(&fixture.live).heartbeat(beat(1), unix_ms());
        let (first, _) = call(&fixture, Method::POST, APPLY_ROUTE).await;
        let (second, _) = call(&fixture, Method::POST, APPLY_ROUTE).await;
        assert_eq!(
            (first, second),
            (StatusCode::ACCEPTED, StatusCode::CONFLICT)
        );
        finished(&fixture).await;
    }

    #[tokio::test]
    async fn apply_fails_when_sims_do_not_return() {
        let fixture = fixture(true, SHORT_TIMEOUT_S);
        read_control(&fixture.path("control"), |_| {});
        lock(&fixture.live).heartbeat(beat(1), unix_ms());
        call(&fixture, Method::POST, APPLY_ROUTE).await;
        let end = finished(&fixture).await;
        assert_eq!(end["phase"], "failed", "{end}");
        assert_eq!(end["step"], "wait_sims");
    }

    #[tokio::test]
    async fn apply_finishes_when_every_sim_restarts() {
        let fixture = fixture(true, LONG_TIMEOUT_S);
        let live = fixture.live.clone();
        read_control(&fixture.path("control"), move |line| {
            if line == "restart-world" {
                lock(&live).heartbeat(beat(unix_ms() + 1), unix_ms());
            }
        });
        lock(&fixture.live).heartbeat(beat(1), unix_ms());
        call(&fixture, Method::POST, APPLY_ROUTE).await;
        let end = finished(&fixture).await;
        assert_eq!(end["phase"], "done", "{end}");
        assert_eq!(end["detail"], "1 of 1 sims back");
    }

    #[tokio::test]
    async fn the_apply_route_needs_the_admin_header() {
        let fixture = fixture(false, LONG_TIMEOUT_S);
        let response = fixture
            .app()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(APPLY_ROUTE)
                    .header(header::HOST, LISTEN)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
