// SPDX-License-Identifier: AGPL-3.0-or-later
//! Game client routes up to character select (M2 spec sections 3.1 and 3.3, JSON note
//! `docs/re/2026-09-02-login-json-shapes.md`).
//!
//! The channel document on `login.<domain>` puts the game session key into every endpoint in the
//! path form (JSON note section 7), so the client calls
//! `https://game.<domain>/s/<session>/Prod/steam` and
//! `https://game.<domain>/s/<session>/prod/game/...`. Every route checks the session. The Steam
//! tickets in the request bodies are never stored or trusted.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{Method, Uri};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use nw_db::{Character, NewCharacter};
use nw_protocol::game_api::{
    AUTH_STACK_STAGE, AUTH_STACK_TAG, CHARACTERS_SEGMENT, ChannelDocument, CharacterSummary,
    Cluster, CreateCharacterReply, CreateCharacterRequestBody, GATEWAY_STAGE, GET_LOGIN_INFO_PATH,
    HTTP_GATEWAY_TAG, LIST_WORLDS_PATH, LOGIN_GATEWAY_TAG, ListWorldsBody, ListWorldsReply,
    LoginInfoList, LoginInfoReply, NameRecord, OMNI_METADATA_MARK, OmniAccount, OmniMetadata,
    OmniPlatformAccount, OmniTokenReply, PUBLIC_STATUS_OPEN, PublicApi, STEAM_AUTH_PATH,
    SteamAuthReply, VALIDATOR_SEGMENT, ValidateCharacterRequest, WORLD_STATUS_ACTIVE, WORLDS_PATH,
    WorldModel,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::config::{EdgeConfig, WorldConfig};
use crate::error::{ApiError, parse_json};
use crate::keys::is_game_session_key;
use crate::names::check_character_name;
use crate::store::{CharacterStore, GameSessionStore};
use crate::time::{http_date, unix_now};

/// Path part in front of the game session key: `/s/<session>`.
pub const SESSION_PREFIX: &str = "/s";
/// Path of the token service under the session. Any longer path below it is also a token call,
/// because the path the client appends to the token URL is Unknown (JSON note section 4).
pub const TOKEN_PATH: &str = "/token";
/// File name ending of the channel document.
pub const CHANNEL_FILE_SUFFIX: &str = ".json";
/// The client may add dot-separated parts around the channel key (JSON note section 7).
const CHANNEL_KEY_SEPARATOR: char = '.';
/// Id of the one cluster in the channel document.
const CLUSTER_ID: &str = "blighted";
/// Game alias in the Omni settings; the client calls `<token URL>/games/<alias>/tokens`.
const OMNI_GAME_ALIAS: &str = "blighted";
/// Region text in every `publicApis` element; the client only signs requests with it.
const API_REGION: &str = "us-east-1";
/// Value of the three `/steam` credentials; the client signs requests with them and the server
/// ignores the signatures (JSON note section 7).
const AUTH_STACK_CREDENTIAL: &str = "blighted";
/// Value of the Omni access and fallback tokens; the client does not check them.
const OMNI_TOKEN: &str = "blighted";
/// Platform name in the Omni token reply.
const STEAM_PLATFORM: &str = "steam";
/// Text shown in logs in place of a game session key.
const SESSION_MASK: &str = "<session>";
const PATH_SEPARATOR: char = '/';
/// Largest game request body. The character create body is stored (M2 spec section 7).
pub const GAME_BODY_LIMIT_BYTES: usize = 64 * 1024;

/// Everything the game routes need.
#[derive(Clone)]
pub struct GameState<G, C> {
    /// Server configuration.
    pub config: Arc<EdgeConfig>,
    /// Game session keys.
    pub game_sessions: G,
    /// Characters and names.
    pub characters: C,
}

/// Path parameters of a session route.
#[derive(Debug, Deserialize)]
struct SessionPath {
    session: String,
}

/// Path parameters of a character route.
#[derive(Debug, Deserialize)]
struct WorldPath {
    session: String,
    world_id: String,
}

/// `/s/{session}<rest>` as an `axum` route.
fn session_route(rest: &str) -> String {
    format!("{SESSION_PREFIX}/{{session}}{rest}")
}

/// A gateway operation path under the session: `/s/{session}/prod<operation>`.
fn gateway_route(operation: &str) -> String {
    session_route(&format!("{GATEWAY_STAGE}{operation}"))
}

/// The game routes on `game.<domain>`.
pub fn game_router<G, C>(state: GameState<G, C>) -> Router
where
    G: GameSessionStore,
    C: CharacterStore,
{
    Router::new()
        .route(
            &session_route(&format!("{AUTH_STACK_STAGE}{STEAM_AUTH_PATH}")),
            post(steam_auth::<G, C>),
        )
        .route(&session_route(TOKEN_PATH), any(token::<G, C>))
        .route(
            &session_route(&format!("{TOKEN_PATH}/{{*rest}}")),
            any(token::<G, C>),
        )
        .route(
            &gateway_route(GET_LOGIN_INFO_PATH),
            get(login_info::<G, C>).post(login_info::<G, C>),
        )
        .route(
            &gateway_route(LIST_WORLDS_PATH),
            get(list_worlds::<G, C>).post(list_worlds::<G, C>),
        )
        .route(
            &gateway_route(&format!("{WORLDS_PATH}/{{world_id}}{CHARACTERS_SEGMENT}")),
            post(create_character::<G, C>),
        )
        .route(
            &gateway_route(&format!("{WORLDS_PATH}/{{world_id}}{VALIDATOR_SEGMENT}")),
            post(validate_character::<G, C>),
        )
        .layer(DefaultBodyLimit::max(GAME_BODY_LIMIT_BYTES))
        .fallback(unknown_route)
        .with_state(state)
}

/// The channel document route on `login.<domain>`: `GET /<key>.json`.
pub fn channel_router<G, C>(state: GameState<G, C>) -> Router
where
    G: GameSessionStore,
    C: CharacterStore,
{
    Router::new()
        .route("/{file}", get(channel::<G, C>))
        .with_state(state)
}

/// The account of a live game session; extends the session.
async fn session_account<G: GameSessionStore, C>(
    state: &GameState<G, C>,
    session: &str,
) -> Result<Uuid, ApiError> {
    if !is_game_session_key(session) {
        return Err(ApiError::NoSession);
    }
    state
        .game_sessions
        .resolve(session, state.config.auth.game_session_ttl_seconds)
        .await?
        .ok_or(ApiError::NoSession)
}

fn is_channel_key_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
}

/// The channel key and the game session key of a channel file name such as `<key>.json` or
/// `SANDBOX_ID.x.<key>.CROSSPLAY.off.json` (JSON note section 7). The channel key is the file
/// name without `.json`; it may hold only ASCII letters, digits, `.`, `_` and `-`.
pub fn channel_key(file: &str) -> Option<(&str, &str)> {
    let key = file.strip_suffix(CHANNEL_FILE_SUFFIX)?;
    if !key.bytes().all(is_channel_key_byte) {
        return None;
    }
    let session = key
        .split(CHANNEL_KEY_SEPARATOR)
        .find(|part| is_game_session_key(part))?;
    Some((key, session))
}

/// The channel document for one game session: every endpoint is `game.<domain>/s/<session>`.
/// The Omni settings key is `OMNI.<channel key>`, because the client looks the settings up by the
/// key it asked for (follow-up note section 2).
pub fn channel_document(config: &EdgeConfig, channel_key: &str, session: &str) -> ChannelDocument {
    let endpoint = format!("{}{SESSION_PREFIX}/{session}", config.game_host());
    let token_url = format!("https://{endpoint}{TOKEN_PATH}");
    let api = |tag: &str| PublicApi {
        client_tag: tag.to_owned(),
        api_endpoint: endpoint.clone(),
        aws_region: API_REGION.to_owned(),
    };
    ChannelDocument {
        channel_name: config.server_name.clone(),
        platform_metadata: [(
            format!("{OMNI_METADATA_MARK}{channel_key}"),
            OmniMetadata {
                omni_game_alias: OMNI_GAME_ALIAS.to_owned(),
                omni_token_url: token_url.clone(),
                nw_token_url: token_url,
            },
        )]
        .into(),
        clusters: [(
            CLUSTER_ID.to_owned(),
            Cluster {
                localized_name: config.server_name.clone(),
                public_apis: vec![
                    api(AUTH_STACK_TAG),
                    api(LOGIN_GATEWAY_TAG),
                    api(HTTP_GATEWAY_TAG),
                ],
            },
        )]
        .into(),
    }
}

async fn channel<G, C>(
    State(state): State<GameState<G, C>>,
    Path(file): Path<String>,
) -> Result<Json<ChannelDocument>, ApiError>
where
    G: GameSessionStore,
{
    if !file.ends_with(CHANNEL_FILE_SUFFIX) {
        return Err(ApiError::UnknownRoute);
    }
    let (key, session) = channel_key(&file).ok_or(ApiError::NoSession)?;
    session_account(&state, session).await?;
    Ok(Json(channel_document(&state.config, key, session)))
}

fn session_seconds(config: &EdgeConfig) -> i64 {
    i64::try_from(config.auth.game_session_ttl_seconds).unwrap_or(i64::MAX)
}

async fn steam_auth<G, C>(
    State(state): State<GameState<G, C>>,
    Path(path): Path<SessionPath>,
) -> Result<Json<SteamAuthReply>, ApiError>
where
    G: GameSessionStore,
{
    session_account(&state, &path.session).await?;
    let expires_at = unix_now().saturating_add(session_seconds(&state.config));
    Ok(Json(SteamAuthReply {
        access_key_id: AUTH_STACK_CREDENTIAL.to_owned(),
        secret_key: AUTH_STACK_CREDENTIAL.to_owned(),
        session_token: AUTH_STACK_CREDENTIAL.to_owned(),
        expiration: http_date(expires_at),
    }))
}

async fn token<G, C>(
    State(state): State<GameState<G, C>>,
    Path(path): Path<SessionPath>,
) -> Result<Json<OmniTokenReply>, ApiError>
where
    G: GameSessionStore,
{
    session_account(&state, &path.session).await?;
    Ok(Json(OmniTokenReply {
        access_token: OMNI_TOKEN.to_owned(),
        fallback_token: OMNI_TOKEN.to_owned(),
        platform_account: OmniPlatformAccount {
            platform: STEAM_PLATFORM.to_owned(),
        },
        account: OmniAccount {
            is_new_account: false,
        },
        expires_in: session_seconds(&state.config),
    }))
}

fn to_json_integer(value: u32) -> i64 {
    i64::from(value)
}

fn world_model(world: &WorldConfig) -> WorldModel {
    WorldModel {
        is_full: false,
        max_account_characters: to_json_integer(world.max_characters_per_account),
        max_connection_count: to_json_integer(world.capacity),
        public_name: world.name.clone(),
        public_status_code: PUBLIC_STATUS_OPEN,
        world_id: world.id.to_string(),
        world_name: world.name.clone(),
        world_status: WORLD_STATUS_ACTIVE.to_owned(),
    }
}

fn world_models(config: &EdgeConfig) -> Vec<WorldModel> {
    config.worlds.iter().map(world_model).collect()
}

/// The character summary the client shows (JSON note section 5.1.1).
pub(crate) fn character_summary(world_id: Uuid, character: &Character) -> CharacterSummary {
    CharacterSummary {
        character_id: character.id.to_string(),
        created_date: http_date(character.created_at_unix),
        normalized_name: character.name.to_lowercase(),
        persona_id: character.account_id.to_string(),
        world_id: world_id.to_string(),
    }
}

fn name_record(character: &Character) -> NameRecord {
    NameRecord {
        created_date: http_date(character.created_at_unix),
        display_name: character.name.clone(),
        normalized_name: character.name.to_lowercase(),
        owning_resource_id: character.id.to_string(),
        persona_id: character.account_id.to_string(),
    }
}

async fn login_info<G, C>(
    State(state): State<GameState<G, C>>,
    Path(path): Path<SessionPath>,
) -> Result<Json<LoginInfoReply>, ApiError>
where
    G: GameSessionStore,
    C: CharacterStore,
{
    let account = session_account(&state, &path.session).await?;
    let mut characters = Vec::new();
    let mut name_reservations = Vec::new();
    for world in &state.config.worlds {
        for character in state.characters.characters(world.id, account).await? {
            name_reservations.push(name_record(&character));
            characters.push(character_summary(world.id, &character));
        }
    }
    Ok(Json(LoginInfoReply {
        login_info_list: LoginInfoList {
            characters,
            name_reservations,
            worlds: world_models(&state.config),
        },
    }))
}

async fn list_worlds<G, C>(
    State(state): State<GameState<G, C>>,
    Path(path): Path<SessionPath>,
) -> Result<Json<ListWorldsReply>, ApiError>
where
    G: GameSessionStore,
{
    session_account(&state, &path.session).await?;
    Ok(Json(ListWorldsReply {
        list_worlds_response_body: ListWorldsBody {
            worlds: world_models(&state.config),
        },
    }))
}

/// The configured world of a path's world id.
fn configured_world<'a>(
    config: &'a EdgeConfig,
    world_id: &str,
) -> Result<&'a WorldConfig, ApiError> {
    Uuid::parse_str(world_id)
        .ok()
        .and_then(|id| config.world(id))
        .ok_or(ApiError::UnknownWorld)
}

async fn create_character<G, C>(
    State(state): State<GameState<G, C>>,
    Path(path): Path<WorldPath>,
    body: Bytes,
) -> Result<Json<CreateCharacterReply>, ApiError>
where
    G: GameSessionStore,
    C: CharacterStore,
{
    let account = session_account(&state, &path.session).await?;
    let world = configured_world(&state.config, &path.world_id)?;
    let body: CreateCharacterRequestBody = parse_json(&body)?;
    let mut request = body.create_character_request.ok_or(ApiError::BadRequest)?;
    let name = request.name.clone().ok_or(ApiError::BadRequest)?;
    check_character_name(&name)?;
    let owned = state.characters.characters(world.id, account).await?.len();
    let limit = usize::try_from(world.max_characters_per_account).unwrap_or(usize::MAX);
    if owned >= limit {
        return Err(ApiError::CharacterLimit);
    }
    // The stored request keeps everything but the Steam ticket.
    request.steam_auth_ticket = None;
    let appearance = serde_json::to_value(&request).map_err(|_| ApiError::BadRequest)?;
    let new = NewCharacter {
        id: Uuid::new_v4(),
        account_id: account,
        name,
        appearance,
    };
    let character = state.characters.create(world.id, &new).await?;
    Ok(Json(CreateCharacterReply {
        character: character_summary(world.id, &character),
    }))
}

/// Checks a name. The reply body model is Unknown (JSON note section 5.4), so a free name gets an
/// empty JSON object and a refused name gets an error status.
async fn validate_character<G, C>(
    State(state): State<GameState<G, C>>,
    Path(path): Path<WorldPath>,
    body: Bytes,
) -> Result<Json<serde_json::Map<String, serde_json::Value>>, ApiError>
where
    G: GameSessionStore,
    C: CharacterStore,
{
    session_account(&state, &path.session).await?;
    configured_world(&state.config, &path.world_id)?;
    let request: ValidateCharacterRequest = parse_json(&body)?;
    let name = request
        .validate_character_body
        .and_then(|body| body.name)
        .ok_or(ApiError::BadRequest)?;
    check_character_name(&name)?;
    if state.characters.name_taken(&name).await? {
        return Err(ApiError::CharacterNameTaken);
    }
    Ok(Json(serde_json::Map::new()))
}

/// `path` with every game session key replaced by [`SESSION_MASK`].
pub fn masked_path(path: &str) -> String {
    path.split(PATH_SEPARATOR)
        .map(|part| {
            if is_game_session_key(part) {
                SESSION_MASK
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(&PATH_SEPARATOR.to_string())
}

/// Logs a request no route serves, so that the exit test shows what the client asked for.
pub async fn unknown_route(method: Method, uri: Uri) -> ApiError {
    tracing::warn!(%method, path = %masked_path(uri.path()), "request for an unknown route");
    ApiError::UnknownRoute
}

#[cfg(test)]
mod tests {
    use axum::body::Bytes;
    use axum::http::StatusCode;
    use nw_db::NewCharacter;

    use super::*;
    use crate::config::tests::valid;
    use crate::keys::new_game_session_key;
    use crate::memory::{MemoryCharacters, MemoryGameSessions};
    use crate::test_http::{self, error_code};

    const CLAIM_SECONDS: u64 = 600;
    const GMT_SUFFIX: &str = " GMT";
    const NAME: &str = "Ada";
    const SECOND_NAME: &str = "Bea";
    const THIRD_NAME: &str = "Cid";
    const STEAM_TICKET: &str = "ticket";
    /// `max_characters_per_account` of the test configuration.
    const TEST_CHARACTER_LIMIT: u32 = 2;
    const OTHER_KEY: &str = "abcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrst";

    struct Fixture {
        state: GameState<MemoryGameSessions, MemoryCharacters>,
        session: String,
        account: Uuid,
    }

    async fn fixture() -> Fixture {
        let config = valid();
        let world_ids: Vec<Uuid> = config.worlds.iter().map(|world| world.id).collect();
        let state = GameState {
            config: Arc::new(config),
            game_sessions: MemoryGameSessions::default(),
            characters: MemoryCharacters::new(&world_ids),
        };
        let session = new_game_session_key().expect("key");
        let account = Uuid::new_v4();
        state
            .game_sessions
            .create(&session, account, CLAIM_SECONDS)
            .await
            .expect("session");
        Fixture {
            state,
            session,
            account,
        }
    }

    fn game_path(session: &str, rest: &str) -> String {
        format!("{SESSION_PREFIX}/{session}{rest}")
    }

    async fn call(app: &Router, method: &str, uri: &str) -> (StatusCode, Bytes) {
        test_http::send(app, test_http::request(method, uri, "")).await
    }

    async fn post(app: &Router, uri: &str, body: &str) -> (StatusCode, Bytes) {
        test_http::send(app, test_http::request("POST", uri, body)).await
    }

    fn character_path(session: &str, world: &str, segment: &str) -> String {
        game_path(
            session,
            &format!("{GATEWAY_STAGE}{WORLDS_PATH}/{world}{segment}"),
        )
    }

    fn create_body(name: &str) -> String {
        serde_json::json!({"CreateCharacterRequest": {
            "Name": name,
            "CharacterCreationParams": "{}",
            "SteamAuthTicket": STEAM_TICKET,
        }})
        .to_string()
    }

    fn validate_body(name: &str) -> String {
        serde_json::json!({"ValidateCharacterBody": {"Name": name}}).to_string()
    }

    #[test]
    fn channel_key_is_found_in_the_file_name() {
        let plain = format!("{OTHER_KEY}.json");
        assert_eq!(channel_key(&plain), Some((OTHER_KEY, OTHER_KEY)));
        let decorated = format!("SANDBOX_ID.test.{OTHER_KEY}.CROSSPLAY.off");
        assert_eq!(
            channel_key(&format!("{decorated}.json")),
            Some((decorated.as_str(), OTHER_KEY))
        );
        assert_eq!(channel_key(OTHER_KEY), None);
        assert_eq!(channel_key("live.json"), None);
        assert_eq!(channel_key(&format!("{OTHER_KEY}\"x.json")), None);
    }

    #[test]
    fn channel_document_points_every_api_at_the_session() {
        let config = valid();
        let decorated = format!("{OTHER_KEY}.CROSSPLAY.off");
        let document = channel_document(&config, &decorated, OTHER_KEY);
        let endpoint = format!("game.example.org/s/{OTHER_KEY}");
        let cluster = &document.clusters[CLUSTER_ID];
        let tags: Vec<&str> = cluster
            .public_apis
            .iter()
            .map(|api| api.client_tag.as_str())
            .collect();
        assert_eq!(tags, [AUTH_STACK_TAG, LOGIN_GATEWAY_TAG, HTTP_GATEWAY_TAG]);
        assert!(
            cluster
                .public_apis
                .iter()
                .all(|api| api.api_endpoint == endpoint)
        );
        let (key, omni) = document
            .platform_metadata
            .iter()
            .next()
            .expect("omni settings");
        assert_eq!(key, &format!("{OMNI_METADATA_MARK}{decorated}"));
        assert_eq!(omni.omni_game_alias, OMNI_GAME_ALIAS);
        assert_eq!(
            omni.omni_token_url,
            format!("https://{endpoint}{TOKEN_PATH}")
        );
    }

    #[test]
    fn session_keys_are_masked_in_logs() {
        assert_eq!(
            masked_path(&format!("/s/{OTHER_KEY}/prod/game/x")),
            "/s/<session>/prod/game/x"
        );
    }

    #[tokio::test]
    async fn channel_route_needs_a_live_session() {
        let fixture = fixture().await;
        let app = channel_router(fixture.state.clone());
        let (status, body) = call(&app, "GET", &format!("/{}.json", fixture.session)).await;
        assert_eq!(status, StatusCode::OK);
        let document: ChannelDocument = serde_json::from_slice(&body).expect("document");
        assert_eq!(document.channel_name, fixture.state.config.server_name);
        let (status, body) = call(&app, "GET", &format!("/{OTHER_KEY}.json")).await;
        assert_eq!(
            (status, error_code(&body).as_str()),
            (StatusCode::UNAUTHORIZED, "no_session")
        );
        let (status, _) = call(&app, "GET", &format!("/{}", fixture.session)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn steam_and_token_calls_need_a_session() {
        let fixture = fixture().await;
        let app = game_router(fixture.state.clone());
        let steam = format!("{AUTH_STACK_STAGE}{STEAM_AUTH_PATH}");
        let (status, body) = call(&app, "POST", &game_path(&fixture.session, &steam)).await;
        assert_eq!(status, StatusCode::OK);
        let reply: SteamAuthReply = serde_json::from_slice(&body).expect("steam");
        assert!(reply.expiration.ends_with(GMT_SUFFIX));
        for rest in [TOKEN_PATH, "/token/v1/tokens"] {
            let (status, body) = call(&app, "POST", &game_path(&fixture.session, rest)).await;
            assert_eq!(status, StatusCode::OK, "{rest}");
            let reply: OmniTokenReply = serde_json::from_slice(&body).expect("token");
            assert_eq!(reply.platform_account.platform, STEAM_PLATFORM);
        }
        let (status, _) = call(&app, "POST", &game_path(OTHER_KEY, &steam)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = call(&app, "POST", &game_path("short", TOKEN_PATH)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn login_info_lists_the_account_characters_and_worlds() {
        let fixture = fixture().await;
        let world_id = fixture.state.config.worlds[0].id;
        let created = fixture
            .state
            .characters
            .create(
                world_id,
                &NewCharacter {
                    id: Uuid::new_v4(),
                    account_id: fixture.account,
                    name: NAME.to_owned(),
                    appearance: serde_json::json!({}),
                },
            )
            .await
            .expect("create");
        let app = game_router(fixture.state.clone());
        let path = game_path(
            &fixture.session,
            &format!("{GATEWAY_STAGE}{GET_LOGIN_INFO_PATH}"),
        );
        for method in ["GET", "POST"] {
            let (status, body) = call(&app, method, &path).await;
            assert_eq!(status, StatusCode::OK, "{method}");
            let reply: LoginInfoReply = serde_json::from_slice(&body).expect("login info");
            let list = reply.login_info_list;
            assert_eq!(list.characters, vec![character_summary(world_id, &created)]);
            assert_eq!(list.name_reservations[0].display_name, NAME);
            assert_eq!(list.worlds, world_models(&fixture.state.config));
        }
        let (status, _) = call(
            &app,
            "POST",
            &game_path(OTHER_KEY, &format!("{GATEWAY_STAGE}{GET_LOGIN_INFO_PATH}")),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn world_list_shows_selectable_worlds() {
        let fixture = fixture().await;
        let app = game_router(fixture.state.clone());
        let path = game_path(
            &fixture.session,
            &format!("{GATEWAY_STAGE}{LIST_WORLDS_PATH}"),
        );
        let (status, body) = call(&app, "POST", &path).await;
        assert_eq!(status, StatusCode::OK);
        let reply: ListWorldsReply = serde_json::from_slice(&body).expect("worlds");
        let world = &reply.list_worlds_response_body.worlds[0];
        let configured = &fixture.state.config.worlds[0];
        assert_eq!(world.world_id, configured.id.to_string());
        assert_eq!(
            world.max_account_characters,
            i64::from(configured.max_characters_per_account)
        );
        assert_eq!(world.world_status, WORLD_STATUS_ACTIVE);
        assert_eq!(world.public_status_code, PUBLIC_STATUS_OPEN);
    }

    #[tokio::test]
    async fn unknown_game_routes_get_404() {
        let fixture = fixture().await;
        let app = game_router(fixture.state.clone());
        let (status, body) = call(&app, "GET", &game_path(&fixture.session, "/prod/other")).await;
        assert_eq!(
            (status, error_code(&body).as_str()),
            (StatusCode::NOT_FOUND, "unknown_route")
        );
    }

    #[tokio::test]
    async fn characters_are_created_once_per_name() {
        let fixture = fixture().await;
        let world = fixture.state.config.worlds[0].id;
        let app = game_router(fixture.state.clone());
        let path = character_path(&fixture.session, &world.to_string(), CHARACTERS_SEGMENT);
        let (status, body) = post(&app, &path, &create_body(NAME)).await;
        assert_eq!(status, StatusCode::OK);
        let reply: CreateCharacterReply = serde_json::from_slice(&body).expect("reply");
        assert_eq!(reply.character.normalized_name, NAME.to_lowercase());
        assert_eq!(reply.character.world_id, world.to_string());
        let stored = fixture
            .state
            .characters
            .characters(world, fixture.account)
            .await
            .expect("stored");
        assert_eq!(stored[0].appearance["Name"], NAME);
        assert!(stored[0].appearance.get("SteamAuthTicket").is_none());
        let (status, body) = post(&app, &path, &create_body(&NAME.to_uppercase())).await;
        assert_eq!(
            (status, error_code(&body).as_str()),
            (StatusCode::CONFLICT, "character_name_taken")
        );
        let (status, body) = post(&app, &path, &create_body("A1")).await;
        assert_eq!(
            (status, error_code(&body).as_str()),
            (StatusCode::BAD_REQUEST, "invalid_character_name")
        );
        let (status, body) = post(&app, &path, "{}").await;
        assert_eq!(
            (status, error_code(&body).as_str()),
            (StatusCode::BAD_REQUEST, "bad_request")
        );
    }

    #[tokio::test]
    async fn the_character_limit_is_kept() {
        let fixture = fixture().await;
        let world = &fixture.state.config.worlds[0];
        assert_eq!(world.max_characters_per_account, TEST_CHARACTER_LIMIT);
        let app = game_router(fixture.state.clone());
        let path = character_path(&fixture.session, &world.id.to_string(), CHARACTERS_SEGMENT);
        for name in [NAME, SECOND_NAME] {
            let (status, _) = post(&app, &path, &create_body(name)).await;
            assert_eq!(status, StatusCode::OK, "{name}");
        }
        let (status, body) = post(&app, &path, &create_body(THIRD_NAME)).await;
        assert_eq!(
            (status, error_code(&body).as_str()),
            (StatusCode::CONFLICT, "character_limit")
        );
    }

    #[tokio::test]
    async fn character_routes_check_session_world_and_size() {
        let fixture = fixture().await;
        let world = fixture.state.config.worlds[0].id.to_string();
        let app = game_router(fixture.state.clone());
        for segment in [CHARACTERS_SEGMENT, VALIDATOR_SEGMENT] {
            let path = character_path(OTHER_KEY, &world, segment);
            let (status, _) = post(&app, &path, &create_body(NAME)).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{segment}");
            for unknown in [Uuid::new_v4().to_string(), "not-a-uuid".to_owned()] {
                let path = character_path(&fixture.session, &unknown, segment);
                let (status, body) = post(&app, &path, &validate_body(NAME)).await;
                assert_eq!(
                    (status, error_code(&body).as_str()),
                    (StatusCode::NOT_FOUND, "unknown_world"),
                    "{segment} {unknown}"
                );
            }
        }
        let path = character_path(&fixture.session, &world, CHARACTERS_SEGMENT);
        let huge = "x".repeat(GAME_BODY_LIMIT_BYTES + 1);
        let (status, _) = post(&app, &path, &huge).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn validator_reports_free_taken_and_bad_names() {
        let fixture = fixture().await;
        let world = fixture.state.config.worlds[0].id.to_string();
        let app = game_router(fixture.state.clone());
        let validator = character_path(&fixture.session, &world, VALIDATOR_SEGMENT);
        let (status, body) = post(&app, &validator, &validate_body(NAME)).await;
        assert_eq!((status, &body[..]), (StatusCode::OK, &b"{}"[..]));
        let create = character_path(&fixture.session, &world, CHARACTERS_SEGMENT);
        post(&app, &create, &create_body(NAME)).await;
        let (status, body) = post(&app, &validator, &validate_body(NAME)).await;
        assert_eq!(
            (status, error_code(&body).as_str()),
            (StatusCode::CONFLICT, "character_name_taken")
        );
        let (status, _) = post(&app, &validator, &validate_body("no")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = post(&app, &validator, "{}").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
