// SPDX-License-Identifier: AGPL-3.0-or-later
//! Our own remote config sets, answered as the S3 bucket the client reads them from (study note
//! `docs/re/2026-09-18-remote-config-layers.md`).
//!
//! The client asks for `GET /applications/<application>/configuration-sets/<dimension>/<id>/
//! <version>` on `<bucket>.s3.amazonaws.com`, for two applications and four dimensions, each
//! once `versionless` and once for its build. It polls each key again with `If-None-Match`. The
//! `[remote_config] directory` holds one file per `versionless` layer, named after its key:
//! `<application>-<dimension>.json`, and `<application>-WorldId-<world id>.json` for each world
//! ([`layer_file_name`]). A key with no layer gets the S3 `NoSuchKey` reply (retail answers
//! every build key that way). This server checks no request signature.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::header::{CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use data_encoding::{BASE64, HEXLOWER, HEXUPPER};
use md5::{Digest, Md5};
use serde_json::{Map, Value};

use crate::analytics::xml_text;
use crate::config::EdgeConfig;
use crate::error::ApiError;
use crate::game::CLUSTER_ID;

/// The two applications the client reads.
pub const APPLICATIONS: [&str; 2] = ["public", "publicGameplay"];
/// The product id of the Steam client (`ProductId` layers; seen in the client log).
pub const PRODUCT_ID: &str = "STEAM_APP_ID.1063730";
/// The version part of the key that holds the values of every build.
pub const VERSIONLESS: &str = "versionless";
/// The application whose `RegionId` layer holds the stats service endpoint.
pub const LEADERBOARDS_APPLICATION: &str = "publicGameplay";
/// Path of the stats service object in that layer.
pub const LEADERBOARDS_PATH: [&str; 2] = ["omni", "leaderboards"];
/// The stats service host the client queries (leaderboards note). This server fills it with
/// `game.<domain>`; the tracked files leave it out, because the domain is the server's own.
pub const CLIENT_ENDPOINT_KEY: &str = "client-endpoint";

/// First part of every key.
const KEY_ROOT: &str = "applications";
/// Second part of every key after the application.
const CONFIGURATION_SETS: &str = "configuration-sets";
const KEY_SEPARATOR: char = '/';
/// Between the parts of a layer file name.
const FILE_NAME_SEPARATOR: &str = "-";
/// The extension of a layer file.
const LAYER_FILE_EXTENSION: &str = "json";
/// Content type of an object (retail reply).
const OBJECT_CONTENT_TYPE: &str = "binary/octet-stream";
/// Content type of an S3 error.
const ERROR_CONTENT_TYPE: &str = "application/xml";
/// S3 request id headers.
const REQUEST_ID_HEADER: &str = "x-amz-request-id";
const HOST_ID_HEADER: &str = "x-amz-id-2";
/// Random bytes of an S3 request id (16 upper-case hex digits) and host id (base64).
const REQUEST_ID_BYTES: usize = 8;
const HOST_ID_BYTES: usize = 56;
/// `If-None-Match` value that matches every object.
const ANY_ETAG: &str = "*";
const ETAG_LIST_SEPARATOR: char = ',';
const PERCENT: u8 = b'%';
const HEX_RADIX: u32 = 16;
const PERCENT_ESCAPE_LEN: usize = 3;

/// The dimensions of a configuration set, in the client's order (`ProductId` lowest).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    /// The Steam product.
    ProductId,
    /// The cluster.
    RegionId,
    /// The Cognito pool.
    CognitoId,
    /// One world.
    WorldId,
}

impl Dimension {
    /// The key part.
    pub fn name(self) -> &'static str {
        match self {
            Self::ProductId => "ProductId",
            Self::RegionId => "RegionId",
            Self::CognitoId => "CognitoId",
            Self::WorldId => "WorldId",
        }
    }
}

/// `applications/<application>/configuration-sets/<dimension>/<id>/<version>`.
pub fn object_key(application: &str, dimension: Dimension, id: &str, version: &str) -> String {
    [
        KEY_ROOT,
        application,
        CONFIGURATION_SETS,
        dimension.name(),
        id,
        version,
    ]
    .join(&KEY_SEPARATOR.to_string())
}

/// The file of the `versionless` layer `application` / `dimension` / `id`: one file per layer in
/// one folder; only a world's file names its id, the other ids come from the configuration.
pub fn layer_file_name(application: &str, dimension: Dimension, id: &str) -> String {
    let stem = match dimension {
        Dimension::WorldId => [application, dimension.name(), id].join(FILE_NAME_SEPARATOR),
        Dimension::ProductId | Dimension::RegionId | Dimension::CognitoId => {
            [application, dimension.name()].join(FILE_NAME_SEPARATOR)
        }
    };
    format!("{stem}.{LAYER_FILE_EXTENSION}")
}

/// The layers our client asks for with this configuration: dimension and id.
pub fn requested_layers(config: &EdgeConfig) -> Vec<(Dimension, String)> {
    let mut layers = vec![
        (Dimension::ProductId, PRODUCT_ID.to_owned()),
        (Dimension::RegionId, CLUSTER_ID.to_owned()),
        (
            Dimension::CognitoId,
            config.remote_config.cognito_pool_id.to_string(),
        ),
    ];
    layers.extend(
        config
            .worlds
            .iter()
            .map(|world| (Dimension::WorldId, world.id.to_string())),
    );
    layers
}

/// The config sets could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum RemoteConfigError {
    /// A file or folder cannot be read.
    #[error("cannot read {}: {source}", .path.display())]
    Io {
        /// The file or folder.
        path: PathBuf,
        /// The I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The folder holds a file that is no layer of this configuration.
    #[error("remote config file {} is no layer this server serves", .0.display())]
    Unexpected(PathBuf),
    /// A file is not a JSON object.
    #[error("remote config {key} is not a JSON object")]
    NotObject {
        /// The key.
        key: String,
    },
    /// A layer the client asks for has no file.
    #[error("remote config {key} is missing: no file {}", .path.display())]
    Missing {
        /// The key.
        key: String,
        /// The file it needs.
        path: PathBuf,
    },
    /// The leaderboards layer has no `omni.leaderboards` object, or sets the key the server fills.
    #[error("remote config {key}: {reason}")]
    Leaderboards {
        /// The key.
        key: String,
        /// What is wrong.
        reason: String,
    },
}

/// One object: its body and its ETag.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteConfigObject {
    body: Vec<u8>,
    /// `"<MD5 hex>"`, as S3 gives for an object stored in one part.
    etag: String,
}

impl RemoteConfigObject {
    fn new(body: Vec<u8>) -> Self {
        let etag = format!("\"{}\"", HEXLOWER.encode(&Md5::digest(&body)));
        Self { body, etag }
    }
}

/// The loaded config sets, by key.
#[derive(Debug, Clone)]
pub struct RemoteConfigSets {
    objects: Arc<HashMap<String, RemoteConfigObject>>,
}

impl RemoteConfigSets {
    /// Loads the `versionless` layer of every application and layer of [`requested_layers`]
    /// from its file in `[remote_config] directory` ([`layer_file_name`]). Each must be a JSON
    /// object, and the folder may hold no other file. The stats service endpoint of the
    /// [`LEADERBOARDS_APPLICATION`] `RegionId` layer is set to `game.<domain>`; other layers are
    /// served byte for byte.
    pub fn load(config: &EdgeConfig) -> Result<Self, RemoteConfigError> {
        let root = &config.remote_config.directory;
        let leaderboards_key = object_key(
            LEADERBOARDS_APPLICATION,
            Dimension::RegionId,
            CLUSTER_ID,
            VERSIONLESS,
        );
        let mut objects = HashMap::new();
        let mut layer_files = HashSet::new();
        for application in APPLICATIONS {
            for (dimension, id) in requested_layers(config) {
                let key = object_key(application, dimension, &id, VERSIONLESS);
                let path = root.join(layer_file_name(application, dimension, &id));
                let bytes = std::fs::read(&path).map_err(|source| {
                    if source.kind() == std::io::ErrorKind::NotFound {
                        RemoteConfigError::Missing {
                            key: key.clone(),
                            path: path.clone(),
                        }
                    } else {
                        RemoteConfigError::Io {
                            path: path.clone(),
                            source,
                        }
                    }
                })?;
                let Ok(Value::Object(object)) = serde_json::from_slice::<Value>(&bytes) else {
                    return Err(RemoteConfigError::NotObject { key });
                };
                let body = if key == leaderboards_key {
                    with_client_endpoint(object, &config.game_host(), &key)?
                } else {
                    bytes
                };
                objects.insert(key, RemoteConfigObject::new(body));
                layer_files.insert(path);
            }
        }
        let io_error = |source| RemoteConfigError::Io {
            path: root.clone(),
            source,
        };
        for entry in std::fs::read_dir(root).map_err(io_error)? {
            let path = entry.map_err(io_error)?.path();
            if !layer_files.contains(&path) {
                return Err(RemoteConfigError::Unexpected(path));
            }
        }
        Ok(Self {
            objects: Arc::new(objects),
        })
    }

    /// The body of an object, if it exists.
    pub fn body(&self, key: &str) -> Option<&[u8]> {
        self.objects.get(key).map(|object| object.body.as_slice())
    }

    /// Every key, sorted.
    pub fn keys(&self) -> Vec<&str> {
        let mut keys: Vec<&str> = self.objects.keys().map(String::as_str).collect();
        keys.sort_unstable();
        keys
    }
}

/// `object` with [`CLIENT_ENDPOINT_KEY`] set to `game_host`, as JSON text.
fn with_client_endpoint(
    mut object: Map<String, Value>,
    game_host: &str,
    key: &str,
) -> Result<Vec<u8>, RemoteConfigError> {
    let fail = |reason: String| RemoteConfigError::Leaderboards {
        key: key.to_owned(),
        reason,
    };
    let [omni, leaderboards] = LEADERBOARDS_PATH;
    let section = object
        .get_mut(omni)
        .and_then(|value| value.get_mut(leaderboards))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| fail(format!("has no {omni}.{leaderboards} object")))?;
    if section.contains_key(CLIENT_ENDPOINT_KEY) {
        return Err(fail(format!(
            "sets {CLIENT_ENDPOINT_KEY}, which the server fills from its domain"
        )));
    }
    section.insert(
        CLIENT_ENDPOINT_KEY.to_owned(),
        Value::String(game_host.to_owned()),
    );
    serde_json::to_vec_pretty(&Value::Object(object)).map_err(|error| fail(error.to_string()))
}

/// The S3 route: `GET /<key>` (and `HEAD`).
pub fn remote_config_router(sets: RemoteConfigSets) -> Router {
    Router::new()
        .route("/{*key}", get(get_object))
        .with_state(sets)
}

async fn get_object(
    State(sets): State<RemoteConfigSets>,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let Some(ids) = RequestIds::new() else {
        return ApiError::Unavailable.into_response();
    };
    let raw = uri.path().trim_start_matches(KEY_SEPARATOR);
    let key = percent_decode(raw).unwrap_or_else(|| raw.to_owned());
    let mut response = match sets.objects.get(&key) {
        Some(object) => object_reply(object, &headers),
        None => no_such_key(&key, &ids),
    };
    ids.add_headers(&mut response);
    response
}

/// `200` with the body, or `304` when `If-None-Match` names its ETag; both carry the ETag.
fn object_reply(object: &RemoteConfigObject, headers: &HeaderMap) -> Response {
    let mut response = if etag_matches(headers, &object.etag) {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        (
            [(CONTENT_TYPE, HeaderValue::from_static(OBJECT_CONTENT_TYPE))],
            Body::from(object.body.clone()),
        )
            .into_response()
    };
    if let Ok(etag) = HeaderValue::from_str(&object.etag) {
        response.headers_mut().insert(ETAG, etag);
    }
    response
}

/// Whether `If-None-Match` names `etag` (or `*`).
fn etag_matches(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get_all(IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(ETAG_LIST_SEPARATOR))
        .map(str::trim)
        .any(|candidate| candidate == etag || candidate == ANY_ETAG)
}

/// The S3 `NoSuchKey` reply.
fn no_such_key(key: &str, ids: &RequestIds) -> Response {
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Error><Code>NoSuchKey</Code>\
         <Message>The specified key does not exist.</Message><Key>{}</Key>\
         <RequestId>{}</RequestId><HostId>{}</HostId></Error>",
        xml_text(key),
        ids.request_id,
        ids.host_id
    );
    (
        StatusCode::NOT_FOUND,
        [(CONTENT_TYPE, HeaderValue::from_static(ERROR_CONTENT_TYPE))],
        body,
    )
        .into_response()
}

/// Fresh S3 request ids: 16 upper-case hex digits and a base64 host id.
struct RequestIds {
    request_id: String,
    host_id: String,
}

impl RequestIds {
    /// New random ids, or none when the system random source fails.
    fn new() -> Option<Self> {
        let mut request_id = [0_u8; REQUEST_ID_BYTES];
        let mut host_id = [0_u8; HOST_ID_BYTES];
        getrandom::fill(&mut request_id).ok()?;
        getrandom::fill(&mut host_id).ok()?;
        Some(Self {
            request_id: HEXUPPER.encode(&request_id),
            host_id: BASE64.encode(&host_id),
        })
    }

    fn add_headers(&self, response: &mut Response) {
        for (name, value) in [
            (REQUEST_ID_HEADER, &self.request_id),
            (HOST_ID_HEADER, &self.host_id),
        ] {
            if let Ok(value) = HeaderValue::from_str(value) {
                response.headers_mut().insert(name, value);
            }
        }
    }
}

/// `%XX` escapes decoded, or none when an escape or the result is not valid.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == PERCENT {
            let escape = text.get(index + 1..index + PERCENT_ESCAPE_LEN)?;
            out.push(u8::from_str_radix(escape, HEX_RADIX).ok()?);
            index += PERCENT_ESCAPE_LEN;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
pub(crate) mod tests {
    use axum::body::Bytes;
    use axum::http::Request;
    use std::collections::BTreeSet;

    use super::*;
    use crate::config::tests::valid;
    use crate::test_http;

    /// The tracked config sets.
    fn committed_directory() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/remote-config")
    }

    /// The tracked config sets, loaded for `config`.
    pub(crate) fn committed_sets(config: &EdgeConfig) -> RemoteConfigSets {
        let mut config = config.clone();
        config.remote_config.directory = committed_directory();
        RemoteConfigSets::load(&config).expect("tracked config sets")
    }

    fn sets() -> RemoteConfigSets {
        committed_sets(&valid())
    }

    const COGNITO_ID: &str = "b04fbfcd-942c-472d-88c0-c762c4076b2f";
    const WORLD_ID: &str = "8d0f4c2e-0000-4000-8000-000000000001";
    const BUILD_VERSION: &str = "6033;6024067";
    const ESCAPED_BUILD_VERSION: &str = "6033%3B6024067";

    /// The key paths (nested object keys joined with `/`) our layers serve: the retail key names,
    /// less the four Amazon-only stats keys (`server-endpoint`, `ingestion-endpoint`,
    /// `stats-assume-role`, `ingestion-assume-role`), plus the switches the leaderboards study
    /// asks for (`analytics/enable` and seven `javelin` keys of the publicGameplay `RegionId`
    /// layer; leaderboards note section 1).
    const SERVED_KEY_PATHS: [(&str, Dimension, &[&str]); 8] = [
        (
            "public",
            Dimension::ProductId,
            &[
                "javelin/landing-screen-character-transfer",
                "javelin/landing-screen-character-transfer-direct",
                "javelin/red-button-disable-traffic-mitigation",
            ],
        ),
        (
            "public",
            Dimension::RegionId,
            &[
                "analytics/enable",
                "analytics/event-types-to-disable",
                "analytics/kinesis-put-role",
                "analytics/kinesis-region-name",
                "analytics/kinesis-stream-name",
                "analytics/overflow-kinesis-put-role",
                "analytics/overflow-kinesis-stream-name",
                "analytics/region-name",
            ],
        ),
        (
            "public",
            Dimension::CognitoId,
            &["javelin/dynamic-datatable-config"],
        ),
        ("public", Dimension::WorldId, &[]),
        (
            "publicGameplay",
            Dimension::ProductId,
            &[
                "javelin/xp-multipliers/character",
                "javelin/xp-multipliers/progression",
            ],
        ),
        (
            "publicGameplay",
            Dimension::RegionId,
            &[
                "javelin-server/enable-twitch-system",
                "javelin/enable-leaderboard-rotations",
                "javelin/enable-leaderboards",
                "javelin/enable-leaderboards-stat-queries",
                "javelin/enable-leaderboards-ui",
                "javelin/leaderboard-rotation-startDate",
                "javelin/leaderboard-season-rotation",
                "javelin/query-leaderboards-service-from-client",
                "javelin/use-omni-penalties",
                "javelin/use-persona-penalties",
                "javelin/use-season-end-time-config",
                "javelin/weekly-leaderboard-reset-hour",
                "omni/leaderboards/client-endpoint",
                "omni/leaderboards/region",
            ],
        ),
        (
            "publicGameplay",
            Dimension::CognitoId,
            &[
                "javelin/disabled-item-classes",
                "javelin/dynamic-datatable-config",
                "javelin/game-mode-matchmaking-service-enabled-modes",
                "javelin/gamemode-activity-type-penalty-overrides",
                "javelin/gamemode-dungeon-mutated-max-runs",
                "javelin/gamemode-group-trial-max-refresh-seconds",
                "javelin/gamemode-mutations/forceRefresh",
                "javelin/gamemode-mutations/seedModifier",
                "javelin/gamemode-scheduler/cadenceInSec",
                "javelin/gamemode-scheduler/maxToSchedule",
                "javelin/gear-set-storage-max-num-paid-loadouts",
                "javelin/housing/house-level-reqs",
                "javelin/leaderboard-season-end-datetime",
                "javelin/red-button-catch-all-achievement-correction",
                "javelin/red-button-disable-spawn-slice-list",
                "javelin/red-button-npa-override",
                "javelin/schedule-overrides",
                "javelin/seasons-rewards.season-end-datetime",
                "javelin/social/enable-company-only-war-slots",
                "javelin/storage-search.crafting-enabled",
                "javelin/storage-search.enabled",
                "javelin/storage-search.housing-enabled",
                "javelin/territory-entry-requirement-enabled",
                "javelin/transmog-own-all-appearances",
                "javelin/wallet-cap-player",
                "javelin/xp-multipliers/character",
                "javelin/xp-multipliers/progression",
            ],
        ),
        (
            "publicGameplay",
            Dimension::WorldId,
            &["services/matchmaking/disable-instance-polling"],
        ),
    ];

    fn layer_id(dimension: Dimension) -> &'static str {
        match dimension {
            Dimension::ProductId => PRODUCT_ID,
            Dimension::RegionId => CLUSTER_ID,
            Dimension::CognitoId => COGNITO_ID,
            Dimension::WorldId => WORLD_ID,
        }
    }

    fn key_paths(value: &Value, prefix: &str, out: &mut BTreeSet<String>) {
        if let Value::Object(object) = value {
            for (name, child) in object {
                let path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}{KEY_SEPARATOR}{name}")
                };
                match child {
                    Value::Object(inner) if !inner.is_empty() => key_paths(child, &path, out),
                    _ => {
                        out.insert(path);
                    }
                }
            }
        }
    }

    fn served_json(sets: &RemoteConfigSets, key: &str) -> Value {
        serde_json::from_slice(sets.body(key).expect(key)).expect("json")
    }

    #[test]
    fn tracked_sets_have_the_retail_layers_and_the_served_key_names() {
        let sets = sets();
        let mut expected_keys = Vec::new();
        for (application, dimension, paths) in SERVED_KEY_PATHS {
            let key = object_key(application, dimension, layer_id(dimension), VERSIONLESS);
            let mut found = BTreeSet::new();
            key_paths(&served_json(&sets, &key), "", &mut found);
            let wanted: BTreeSet<String> = paths.iter().map(|path| (*path).to_owned()).collect();
            assert_eq!(found, wanted, "{key}");
            expected_keys.push(key);
        }
        expected_keys.sort_unstable();
        assert_eq!(sets.keys(), expected_keys);
    }

    #[test]
    fn tracked_sets_name_no_amazon_host_or_account() {
        let sets = sets();
        for key in sets.keys() {
            let text = String::from_utf8_lossy(sets.body(key).expect("body")).to_lowercase();
            for forbidden in [
                "amazon",
                "cloudfront",
                "iad-prod",
                "040969890792",
                "295811826248",
            ] {
                assert!(!text.contains(forbidden), "{key} names {forbidden}");
            }
        }
    }

    #[test]
    fn the_stats_endpoint_is_the_servers_game_host() {
        let sets = sets();
        let key = object_key(
            LEADERBOARDS_APPLICATION,
            Dimension::RegionId,
            CLUSTER_ID,
            VERSIONLESS,
        );
        let leaderboards = &served_json(&sets, &key)["omni"]["leaderboards"];
        assert_eq!(leaderboards[CLIENT_ENDPOINT_KEY], "game.example.org");
        assert_eq!(leaderboards["region"], "us-east-1");
    }

    #[test]
    fn other_files_are_served_byte_for_byte() {
        let sets = sets();
        let key = object_key("public", Dimension::ProductId, PRODUCT_ID, VERSIONLESS);
        let file = std::fs::read(committed_directory().join(layer_file_name(
            "public",
            Dimension::ProductId,
            PRODUCT_ID,
        )))
        .expect("file");
        assert_eq!(sets.body(&key), Some(file.as_slice()));
    }

    /// A copy of the tracked sets in a temporary folder, and a config that reads it.
    fn copied_sets() -> (tempfile::TempDir, EdgeConfig) {
        let dir = tempfile::tempdir().expect("temp dir");
        for entry in std::fs::read_dir(committed_directory()).expect("folder") {
            let file = entry.expect("entry").path();
            std::fs::copy(&file, dir.path().join(file.file_name().expect("name"))).expect("copy");
        }
        let mut config = valid();
        config.remote_config.directory = dir.path().to_path_buf();
        (dir, config)
    }

    #[test]
    fn a_missing_layer_is_an_error() {
        let (dir, mut config) = copied_sets();
        let missing = object_key("public", Dimension::WorldId, WORLD_ID, VERSIONLESS);
        std::fs::remove_file(dir.path().join(layer_file_name(
            "public",
            Dimension::WorldId,
            WORLD_ID,
        )))
        .expect("remove");
        assert!(matches!(
            RemoteConfigSets::load(&config),
            Err(RemoteConfigError::Missing { key, .. }) if key == missing
        ));
        config.remote_config.directory = dir.path().join("absent");
        assert!(matches!(
            RemoteConfigSets::load(&config),
            Err(RemoteConfigError::Missing { .. })
        ));
    }

    #[test]
    fn a_file_must_be_a_json_object() {
        let (dir, config) = copied_sets();
        let key = object_key("public", Dimension::ProductId, PRODUCT_ID, VERSIONLESS);
        std::fs::write(
            dir.path()
                .join(layer_file_name("public", Dimension::ProductId, PRODUCT_ID)),
            "[1]",
        )
        .expect("write");
        assert!(matches!(
            RemoteConfigSets::load(&config),
            Err(RemoteConfigError::NotObject { key: found }) if found == key
        ));
    }

    #[test]
    fn the_folder_holds_only_layer_files() {
        let (dir, config) = copied_sets();
        let stray = dir.path().join("publicGameplay-RegionId-old.json");
        std::fs::write(&stray, "{}").expect("write");
        assert!(matches!(
            RemoteConfigSets::load(&config),
            Err(RemoteConfigError::Unexpected(path)) if path == stray
        ));
    }

    #[test]
    fn the_files_must_not_set_the_client_endpoint() {
        let (dir, config) = copied_sets();
        let key = object_key(
            LEADERBOARDS_APPLICATION,
            Dimension::RegionId,
            CLUSTER_ID,
            VERSIONLESS,
        );
        let path = dir.path().join(layer_file_name(
            LEADERBOARDS_APPLICATION,
            Dimension::RegionId,
            CLUSTER_ID,
        ));
        let mut value: Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
        value["omni"]["leaderboards"][CLIENT_ENDPOINT_KEY] = Value::from("elsewhere.example.net");
        std::fs::write(&path, serde_json::to_vec(&value).expect("json")).expect("write");
        assert!(matches!(
            RemoteConfigSets::load(&config),
            Err(RemoteConfigError::Leaderboards { key: found, .. }) if found == key
        ));
        value["omni"] = Value::from("no object");
        std::fs::write(&path, serde_json::to_vec(&value).expect("json")).expect("write");
        assert!(matches!(
            RemoteConfigSets::load(&config),
            Err(RemoteConfigError::Leaderboards { key: found, .. }) if found == key
        ));
    }

    async fn get(path: &str, if_none_match: Option<&str>) -> (StatusCode, HeaderMap, Bytes) {
        let mut request = Request::builder().method("GET").uri(path);
        if let Some(etag) = if_none_match {
            request = request.header(IF_NONE_MATCH, etag);
        }
        let request = request.body(Body::empty()).expect("request");
        let response = tower::ServiceExt::oneshot(remote_config_router(sets()), request)
            .await
            .expect("response");
        let status = response.status();
        let headers = response.headers().clone();
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .expect("body")
            .to_bytes();
        (status, headers, body)
    }

    fn header<'a>(headers: &'a HeaderMap, name: &str) -> &'a str {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn an_object_has_its_body_type_and_etag() {
        let key = object_key("public", Dimension::ProductId, PRODUCT_ID, VERSIONLESS);
        let (status, headers, body) = get(&format!("/{key}"), None).await;
        assert_eq!(status, StatusCode::OK);
        let file = std::fs::read(committed_directory().join(layer_file_name(
            "public",
            Dimension::ProductId,
            PRODUCT_ID,
        )))
        .expect("file");
        assert_eq!(body.as_ref(), file.as_slice());
        assert_eq!(header(&headers, "content-type"), OBJECT_CONTENT_TYPE);
        let etag = format!("\"{}\"", HEXLOWER.encode(&Md5::digest(&file)));
        assert_eq!(header(&headers, "etag"), etag);
        assert_eq!(
            header(&headers, REQUEST_ID_HEADER).len(),
            REQUEST_ID_BYTES * 2
        );
        assert!(!header(&headers, HOST_ID_HEADER).is_empty());
    }

    #[tokio::test]
    async fn a_known_etag_gets_not_modified() {
        let key = object_key("publicGameplay", Dimension::WorldId, WORLD_ID, VERSIONLESS);
        let path = format!("/{key}");
        let (_, headers, _) = get(&path, None).await;
        let etag = header(&headers, "etag").to_owned();
        let (status, headers, body) = get(&path, Some(&etag)).await;
        assert_eq!(status, StatusCode::NOT_MODIFIED);
        assert!(body.is_empty());
        assert_eq!(header(&headers, "etag"), etag);
        let (status, _, _) = get(&path, Some(&format!("\"other\", {etag}"))).await;
        assert_eq!(status, StatusCode::NOT_MODIFIED);
        let (status, _, _) = get(&path, Some(ANY_ETAG)).await;
        assert_eq!(status, StatusCode::NOT_MODIFIED);
        let (status, _, body) = get(&path, Some("\"0123\"")).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.is_empty());
    }

    #[tokio::test]
    async fn a_build_key_gets_no_such_key() {
        for (dimension, id) in [
            (Dimension::ProductId, PRODUCT_ID),
            (Dimension::RegionId, CLUSTER_ID),
            (Dimension::CognitoId, COGNITO_ID),
            (Dimension::WorldId, WORLD_ID),
        ] {
            for application in APPLICATIONS {
                let escaped = object_key(application, dimension, id, ESCAPED_BUILD_VERSION);
                let (status, headers, body) = get(&format!("/{escaped}"), None).await;
                assert_eq!(status, StatusCode::NOT_FOUND, "{escaped}");
                assert_eq!(header(&headers, "content-type"), ERROR_CONTENT_TYPE);
                let text = String::from_utf8(body.to_vec()).expect("text");
                let key = object_key(application, dimension, id, BUILD_VERSION);
                assert!(
                    text.starts_with(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Error><Code>NoSuchKey</Code>"
                    ),
                    "{text}"
                );
                assert!(text.contains(&format!("<Key>{key}</Key>")), "{text}");
                assert!(text.contains(&format!(
                    "<RequestId>{}</RequestId>",
                    header(&headers, REQUEST_ID_HEADER)
                )));
                assert!(text.contains(&format!(
                    "<HostId>{}</HostId>",
                    header(&headers, HOST_ID_HEADER)
                )));
            }
        }
    }

    #[tokio::test]
    async fn other_methods_are_refused() {
        let key = object_key("public", Dimension::ProductId, PRODUCT_ID, VERSIONLESS);
        let request = test_http::request("PUT", &format!("/{key}"), "{}");
        let (status, _) = test_http::send(&remote_config_router(sets()), request).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    }

    #[test]
    fn percent_escapes_are_decoded() {
        assert_eq!(percent_decode("a%3Bb%3ab").as_deref(), Some("a;b:b"));
        assert_eq!(percent_decode("plain").as_deref(), Some("plain"));
        for bad in ["%", "%3", "%zz", "%FF"] {
            assert_eq!(percent_decode(bad), None, "{bad}");
        }
    }

    #[test]
    fn requested_layers_follow_the_configuration() {
        let layers = requested_layers(&valid());
        assert_eq!(
            layers,
            [
                (Dimension::ProductId, PRODUCT_ID.to_owned()),
                (Dimension::RegionId, CLUSTER_ID.to_owned()),
                (Dimension::CognitoId, COGNITO_ID.to_owned()),
                (Dimension::WorldId, WORLD_ID.to_owned()),
            ]
        );
    }
}
