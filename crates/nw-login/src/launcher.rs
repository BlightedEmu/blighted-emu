// SPDX-License-Identifier: AGPL-3.0-or-later
//! The launcher API on `login.<domain>` (M2 spec sections 5 and 6).

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use nw_db::Account;
use nw_protocol::launcher_api::{
    AuthMode, DiscordPoll, DiscordPollReply, DiscordStart, LauncherInfo, LauncherSession,
    PasswordCredentials, PlayReply,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::config::EdgeConfig;
use crate::discord::{DISCORD_CALLBACK_PATH, DiscordApi, authorize_url};
use crate::error::ApiError;
use crate::keys::{is_launcher_token, new_game_session_key, new_launcher_token, token_hash};
use crate::names::{check_password, normalize_username};
use crate::password::PasswordHasher;
use crate::store::{
    AccountStore, DiscordAttempt, DiscordStateStore, GameSessionStore, RateLimiter,
};
use crate::time::unix_now;

/// `GET` server name, login mode and launch arguments.
pub const INFO_PATH: &str = "/launcher/info";
/// `POST` password sign-up.
pub const REGISTER_PATH: &str = "/launcher/register";
/// `POST` password login.
pub const LOGIN_PATH: &str = "/launcher/login";
/// `POST` new game session key.
pub const PLAY_PATH: &str = "/launcher/play";
/// `POST` end the launcher session.
pub const LOGOUT_PATH: &str = "/launcher/logout";
/// `POST` start a Discord login.
pub const DISCORD_START_PATH: &str = "/launcher/discord/start";
/// `POST` ask whether a Discord login finished.
pub const DISCORD_POLL_PATH: &str = "/launcher/discord/poll";

/// Lifetime of a launcher session: 30 days.
pub const LAUNCHER_SESSION_SECONDS: i64 = 30 * 24 * 60 * 60;
/// Time to use a new game session key.
pub const GAME_SESSION_CLAIM_SECONDS: u64 = 600;
/// Lifetime of a Discord login attempt.
pub const DISCORD_LOGIN_SECONDS: u64 = 300;

const BEARER_PREFIX: &str = "Bearer ";
const LOGIN_IP_LIMIT_PREFIX: &str = "login-ip:";
const LOGIN_USER_LIMIT_PREFIX: &str = "login-user:";
const REGISTER_IP_LIMIT_PREFIX: &str = "register-ip:";
const CALLBACK_DONE: &str = "Login finished. Return to the launcher.";
const CALLBACK_EXPIRED: &str = "This login expired. Start again in the launcher.";
const CALLBACK_FAILED: &str = "Discord login failed. Start again in the launcher.";
const CALLBACK_BANNED: &str = "This account is banned.";

/// Everything the launcher routes need.
#[derive(Clone)]
pub struct LauncherState<A, G, R, D, X> {
    /// Server configuration.
    pub config: Arc<EdgeConfig>,
    /// Accounts and launcher sessions.
    pub accounts: A,
    /// Game session keys.
    pub game_sessions: G,
    /// Rate-limit counters.
    pub limits: R,
    /// Discord login attempts.
    pub discord_states: D,
    /// Discord API; `None` in password mode.
    pub discord: Option<X>,
    /// Argon2id worker pool.
    pub hasher: PasswordHasher,
}

/// The launcher routes.
pub fn launcher_router<A, G, R, D, X>(state: LauncherState<A, G, R, D, X>) -> Router
where
    A: AccountStore,
    G: GameSessionStore,
    R: RateLimiter,
    D: DiscordStateStore,
    X: DiscordApi,
{
    Router::new()
        .route(INFO_PATH, get(info::<A, G, R, D, X>))
        .route(REGISTER_PATH, post(register::<A, G, R, D, X>))
        .route(LOGIN_PATH, post(login::<A, G, R, D, X>))
        .route(PLAY_PATH, post(play::<A, G, R, D, X>))
        .route(LOGOUT_PATH, post(logout::<A, G, R, D, X>))
        .route(DISCORD_START_PATH, post(discord_start::<A, G, R, D, X>))
        .route(
            DISCORD_CALLBACK_PATH,
            get(discord_callback::<A, G, R, D, X>),
        )
        .route(DISCORD_POLL_PATH, post(discord_poll::<A, G, R, D, X>))
        .with_state(state)
}

fn parse<T: DeserializeOwned>(body: &Bytes) -> Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|_| ApiError::BadRequest)
}

fn require_mode<A, G, R, D, X>(
    state: &LauncherState<A, G, R, D, X>,
    mode: AuthMode,
) -> Result<(), ApiError> {
    if state.config.auth.mode == mode {
        Ok(())
    } else {
        Err(ApiError::WrongMode)
    }
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix(BEARER_PREFIX))
        .filter(|token| is_launcher_token(token))
        .ok_or(ApiError::NoSession)
}

/// Starts a launcher session for an account that may log in.
async fn new_launcher_session<A: AccountStore>(
    accounts: &A,
    account: &Account,
) -> Result<LauncherSession, ApiError> {
    if account.banned {
        return Err(ApiError::Banned);
    }
    let token = new_launcher_token().map_err(|_| ApiError::Unavailable)?;
    let expires_at = unix_now() + LAUNCHER_SESSION_SECONDS;
    accounts
        .create_launcher_session(token_hash(&token), account.id, expires_at)
        .await?;
    Ok(LauncherSession {
        launcher_token: token,
        expires_at,
    })
}

async fn info<A, G, R, D, X>(
    State(state): State<LauncherState<A, G, R, D, X>>,
) -> Json<LauncherInfo> {
    let config = &state.config;
    Json(LauncherInfo {
        server_name: config.server_name.clone(),
        auth_mode: config.auth.mode,
        allow_registration: config.auth.allow_registration,
        launch_arguments: config.launch_arguments(),
    })
}

async fn register<A, G, R, D, X>(
    State(state): State<LauncherState<A, G, R, D, X>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Bytes,
) -> Result<Json<LauncherSession>, ApiError>
where
    A: AccountStore,
    R: RateLimiter,
{
    require_mode(&state, AuthMode::Password)?;
    if !state.config.auth.allow_registration {
        return Err(ApiError::RegistrationClosed);
    }
    let auth = &state.config.auth;
    let attempts = state
        .limits
        .increment(
            &format!("{REGISTER_IP_LIMIT_PREFIX}{}", peer.ip()),
            auth.login_fail_window_seconds,
        )
        .await?;
    if attempts > u64::from(auth.login_fail_limit) {
        return Err(ApiError::RateLimited);
    }
    let credentials: PasswordCredentials = parse(&body)?;
    let username = normalize_username(&credentials.username)?;
    check_password(&credentials.password)?;
    let hash = state
        .hasher
        .hash(credentials.password)
        .await
        .map_err(|_| ApiError::Unavailable)?;
    let account = state
        .accounts
        .create_password_account(&username, &hash)
        .await?;
    Ok(Json(new_launcher_session(&state.accounts, &account).await?))
}

async fn login<A, G, R, D, X>(
    State(state): State<LauncherState<A, G, R, D, X>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Bytes,
) -> Result<Json<LauncherSession>, ApiError>
where
    A: AccountStore,
    R: RateLimiter,
{
    require_mode(&state, AuthMode::Password)?;
    let credentials: PasswordCredentials = parse(&body)?;
    let auth = &state.config.auth;
    let username = normalize_username(&credentials.username).ok();
    let ip_key = format!("{LOGIN_IP_LIMIT_PREFIX}{}", peer.ip());
    let user_key = username
        .as_ref()
        .map(|name| format!("{LOGIN_USER_LIMIT_PREFIX}{name}"));
    let limit = u64::from(auth.login_fail_limit);
    let mut failures = state.limits.count(&ip_key).await?;
    if let Some(key) = &user_key {
        failures = failures.max(state.limits.count(key).await?);
    }
    if failures >= limit {
        return Err(ApiError::RateLimited);
    }
    let stored = match &username {
        Some(name) => state.accounts.password_login(name).await?,
        None => None,
    };
    let (account, hash) = match stored {
        Some((account, hash)) => (Some(account), Some(hash)),
        None => (None, None),
    };
    let matches = state
        .hasher
        .verify(credentials.password, hash)
        .await
        .map_err(|_| ApiError::Unavailable)?;
    match account {
        Some(account) if matches => {
            Ok(Json(new_launcher_session(&state.accounts, &account).await?))
        }
        _ => {
            let window = auth.login_fail_window_seconds;
            state.limits.increment(&ip_key, window).await?;
            if let Some(key) = &user_key {
                state.limits.increment(key, window).await?;
            }
            Err(ApiError::InvalidCredentials)
        }
    }
}

async fn session_account<A: AccountStore>(
    accounts: &A,
    headers: &HeaderMap,
) -> Result<Account, ApiError> {
    let token = bearer_token(headers)?;
    let account = accounts
        .launcher_session_account(token_hash(token))
        .await?
        .ok_or(ApiError::NoSession)?;
    if account.banned {
        return Err(ApiError::Banned);
    }
    Ok(account)
}

async fn play<A, G, R, D, X>(
    State(state): State<LauncherState<A, G, R, D, X>>,
    headers: HeaderMap,
) -> Result<Json<PlayReply>, ApiError>
where
    A: AccountStore,
    G: GameSessionStore,
{
    let account = session_account(&state.accounts, &headers).await?;
    let key = new_game_session_key().map_err(|_| ApiError::Unavailable)?;
    state
        .game_sessions
        .create(&key, account.id, GAME_SESSION_CLAIM_SECONDS)
        .await?;
    Ok(Json(PlayReply {
        game_session_key: key,
    }))
}

async fn logout<A, G, R, D, X>(
    State(state): State<LauncherState<A, G, R, D, X>>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError>
where
    A: AccountStore,
{
    let token = bearer_token(&headers)?;
    state
        .accounts
        .delete_launcher_session(token_hash(token))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn callback_url(config: &EdgeConfig) -> String {
    format!("https://{}{DISCORD_CALLBACK_PATH}", config.login_host())
}

async fn discord_start<A, G, R, D, X>(
    State(state): State<LauncherState<A, G, R, D, X>>,
) -> Result<Json<DiscordStart>, ApiError>
where
    D: DiscordStateStore,
{
    require_mode(&state, AuthMode::Discord)?;
    let login_state = new_launcher_token().map_err(|_| ApiError::Unavailable)?;
    let verifier = new_launcher_token().map_err(|_| ApiError::Unavailable)?;
    let url = authorize_url(
        &state.config.discord.client_id,
        &callback_url(&state.config),
        &login_state,
        &verifier,
    )
    .map_err(|_| ApiError::Unavailable)?;
    state
        .discord_states
        .begin(&login_state, &verifier, DISCORD_LOGIN_SECONDS)
        .await?;
    Ok(Json(DiscordStart {
        authorize_url: url,
        state: login_state,
    }))
}

/// Query of the Discord redirect.
#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

async fn discord_callback<A, G, R, D, X>(
    State(state): State<LauncherState<A, G, R, D, X>>,
    Query(query): Query<CallbackQuery>,
) -> impl IntoResponse
where
    A: AccountStore,
    D: DiscordStateStore,
    X: DiscordApi,
{
    match finish_discord_login(&state, query).await {
        Ok(()) => (StatusCode::OK, CALLBACK_DONE),
        Err(ApiError::Banned) => (StatusCode::FORBIDDEN, CALLBACK_BANNED),
        Err(ApiError::UnknownLogin | ApiError::WrongMode) => {
            (StatusCode::NOT_FOUND, CALLBACK_EXPIRED)
        }
        Err(error) => (error.status(), CALLBACK_FAILED),
    }
}

async fn finish_discord_login<A, G, R, D, X>(
    state: &LauncherState<A, G, R, D, X>,
    query: CallbackQuery,
) -> Result<(), ApiError>
where
    A: AccountStore,
    D: DiscordStateStore,
    X: DiscordApi,
{
    require_mode(state, AuthMode::Discord)?;
    let discord = state.discord.as_ref().ok_or(ApiError::WrongMode)?;
    let (Some(code), Some(login_state)) = (query.code, query.state) else {
        return Err(ApiError::Discord);
    };
    let verifier = state
        .discord_states
        .verifier(&login_state)
        .await?
        .ok_or(ApiError::UnknownLogin)?;
    let user = discord
        .user_for_code(&code, &verifier, &callback_url(&state.config))
        .await
        .map_err(|error| {
            tracing::warn!(error = %error, "Discord login failed");
            ApiError::Discord
        })?;
    let account = state
        .accounts
        .discord_account(&user.id, &user.display_name)
        .await?;
    let session = new_launcher_session(&state.accounts, &account).await?;
    state
        .discord_states
        .complete(&login_state, &session, DISCORD_LOGIN_SECONDS)
        .await?;
    Ok(())
}

async fn discord_poll<A, G, R, D, X>(
    State(state): State<LauncherState<A, G, R, D, X>>,
    body: Bytes,
) -> Result<Json<DiscordPollReply>, ApiError>
where
    D: DiscordStateStore,
{
    require_mode(&state, AuthMode::Discord)?;
    let poll: DiscordPoll = parse(&body)?;
    match state.discord_states.poll(&poll.state).await? {
        DiscordAttempt::Unknown => Err(ApiError::UnknownLogin),
        DiscordAttempt::Pending => Ok(Json(DiscordPollReply::Pending)),
        DiscordAttempt::Ready(session) => Ok(Json(DiscordPollReply::Ready(session))),
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::extract::connect_info::MockConnectInfo;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use nw_protocol::launcher_api::ErrorBody;
    use tower::ServiceExt;

    use super::*;
    use crate::config::tests::valid;
    use crate::discord::tests::{FAKE_USER_ID, FakeDiscord, GOOD_CODE};
    use crate::memory::{
        MemoryAccounts, MemoryDiscordStates, MemoryGameSessions, MemoryRateLimiter,
    };

    const PEER: ([u8; 4], u16) = ([192, 0, 2, 7], 50_000);
    const USERNAME: &str = "Ada";
    const PASSWORD: &str = "password-ok";
    const ONE_HASH_AT_A_TIME: usize = 1;

    type TestState = LauncherState<
        MemoryAccounts,
        MemoryGameSessions,
        MemoryRateLimiter,
        MemoryDiscordStates,
        FakeDiscord,
    >;

    fn state(mode: AuthMode) -> TestState {
        let mut config = valid();
        config.auth.mode = mode;
        config.discord.client_id = "123".to_owned();
        LauncherState {
            config: Arc::new(config),
            accounts: MemoryAccounts::default(),
            game_sessions: MemoryGameSessions::default(),
            limits: MemoryRateLimiter::default(),
            discord_states: MemoryDiscordStates::default(),
            discord: (mode == AuthMode::Discord).then_some(FakeDiscord),
            hasher: PasswordHasher::new(ONE_HASH_AT_A_TIME).expect("hasher"),
        }
    }

    fn app(state: TestState) -> Router {
        launcher_router(state).layer(MockConnectInfo(SocketAddr::from(PEER)))
    }

    async fn send(
        app: &Router,
        method: &str,
        uri: &str,
        bearer: Option<&str>,
        body: &str,
    ) -> (StatusCode, Bytes) {
        let mut request = Request::builder().method(method).uri(uri);
        if let Some(token) = bearer {
            request = request.header(header::AUTHORIZATION, format!("{BEARER_PREFIX}{token}"));
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::from(body.to_owned())).expect("request"))
            .await
            .expect("response");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        (status, bytes)
    }

    fn error_code(body: &Bytes) -> String {
        serde_json::from_slice::<ErrorBody>(body)
            .expect("error body")
            .code
    }

    fn credentials(username: &str, password: &str) -> String {
        serde_json::to_string(&PasswordCredentials {
            username: username.to_owned(),
            password: password.to_owned(),
        })
        .expect("json")
    }

    async fn register_ok(app: &Router) -> LauncherSession {
        let (status, body) = send(
            app,
            "POST",
            REGISTER_PATH,
            None,
            &credentials(USERNAME, PASSWORD),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        serde_json::from_slice(&body).expect("session")
    }

    #[tokio::test]
    async fn info_describes_the_server() {
        let (status, body) =
            send(&app(state(AuthMode::Password)), "GET", INFO_PATH, None, "").await;
        assert_eq!(status, StatusCode::OK);
        let info: LauncherInfo = serde_json::from_slice(&body).expect("info");
        assert_eq!(info.server_name, "Test server");
        assert_eq!(info.auth_mode, AuthMode::Password);
        assert!(info.allow_registration);
        assert_eq!(info.launch_arguments, valid().launch_arguments());
    }

    #[tokio::test]
    async fn register_login_play_and_logout() {
        let app = app(state(AuthMode::Password));
        let registered = register_ok(&app).await;
        assert!(is_launcher_token(&registered.launcher_token));
        let (status, body) = send(
            &app,
            "POST",
            LOGIN_PATH,
            None,
            &credentials("ADA", PASSWORD),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let session: LauncherSession = serde_json::from_slice(&body).expect("session");
        let token = session.launcher_token.as_str();
        let (status, body) = send(&app, "POST", PLAY_PATH, Some(token), "").await;
        assert_eq!(status, StatusCode::OK);
        let play: PlayReply = serde_json::from_slice(&body).expect("play");
        assert!(crate::keys::is_game_session_key(&play.game_session_key));
        let (status, _) = send(&app, "POST", LOGOUT_PATH, Some(token), "").await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, body) = send(&app, "POST", PLAY_PATH, Some(token), "").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(error_code(&body), "no_session");
    }

    #[tokio::test]
    async fn bad_sign_ups_are_refused() {
        let app = app(state(AuthMode::Password));
        register_ok(&app).await;
        let cases = [
            (
                credentials(USERNAME, PASSWORD),
                StatusCode::CONFLICT,
                "username_taken",
            ),
            (
                credentials("a b", PASSWORD),
                StatusCode::BAD_REQUEST,
                "invalid_username",
            ),
            (
                credentials("bob", "short"),
                StatusCode::BAD_REQUEST,
                "invalid_password",
            ),
            (
                "not json".to_owned(),
                StatusCode::BAD_REQUEST,
                "bad_request",
            ),
        ];
        for (body, status, code) in cases {
            let (found, reply) = send(&app, "POST", REGISTER_PATH, None, &body).await;
            assert_eq!((found, error_code(&reply).as_str()), (status, code));
        }
    }

    #[tokio::test]
    async fn closed_registration_and_wrong_mode_are_refused() {
        let mut closed = state(AuthMode::Password);
        let mut config = (*closed.config).clone();
        config.auth.allow_registration = false;
        closed.config = Arc::new(config);
        let (status, body) = send(
            &app(closed),
            "POST",
            REGISTER_PATH,
            None,
            &credentials(USERNAME, PASSWORD),
        )
        .await;
        assert_eq!(
            (status, error_code(&body).as_str()),
            (StatusCode::FORBIDDEN, "registration_closed")
        );
        let discord = app(state(AuthMode::Discord));
        let (status, body) = send(
            &discord,
            "POST",
            LOGIN_PATH,
            None,
            &credentials(USERNAME, PASSWORD),
        )
        .await;
        assert_eq!(
            (status, error_code(&body).as_str()),
            (StatusCode::NOT_FOUND, "wrong_mode")
        );
        let password = app(state(AuthMode::Password));
        let (status, _) = send(&password, "POST", DISCORD_START_PATH, None, "").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn failed_logins_are_rate_limited() {
        let app = app(state(AuthMode::Password));
        register_ok(&app).await;
        let limit = valid().auth.login_fail_limit;
        for _ in 0..limit {
            let (status, body) = send(
                &app,
                "POST",
                LOGIN_PATH,
                None,
                &credentials(USERNAME, "wrong-password"),
            )
            .await;
            assert_eq!(
                (status, error_code(&body).as_str()),
                (StatusCode::UNAUTHORIZED, "invalid_credentials")
            );
        }
        let (status, body) = send(
            &app,
            "POST",
            LOGIN_PATH,
            None,
            &credentials(USERNAME, PASSWORD),
        )
        .await;
        assert_eq!(
            (status, error_code(&body).as_str()),
            (StatusCode::TOO_MANY_REQUESTS, "rate_limited")
        );
    }

    #[tokio::test]
    async fn unknown_user_and_banned_account_cannot_log_in() {
        let state = state(AuthMode::Password);
        let accounts = state.accounts.clone();
        let app = app(state);
        let (status, _) = send(
            &app,
            "POST",
            LOGIN_PATH,
            None,
            &credentials("nobody", PASSWORD),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let session = register_ok(&app).await;
        let (account, _) = accounts
            .password_login("ada")
            .await
            .expect("store")
            .expect("account");
        accounts.set_banned(account.id, true);
        let (status, body) = send(
            &app,
            "POST",
            LOGIN_PATH,
            None,
            &credentials(USERNAME, PASSWORD),
        )
        .await;
        assert_eq!(
            (status, error_code(&body).as_str()),
            (StatusCode::FORBIDDEN, "banned")
        );
        let (status, _) = send(&app, "POST", PLAY_PATH, Some(&session.launcher_token), "").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn play_needs_a_well_formed_token() {
        let app = app(state(AuthMode::Password));
        let (status, _) = send(&app, "POST", PLAY_PATH, None, "").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = send(&app, "POST", PLAY_PATH, Some("not-a-token"), "").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn discord_login_finishes_through_the_callback() {
        let app = app(state(AuthMode::Discord));
        let (status, body) = send(&app, "POST", DISCORD_START_PATH, None, "").await;
        assert_eq!(status, StatusCode::OK);
        let start: DiscordStart = serde_json::from_slice(&body).expect("start");
        assert!(
            start
                .authorize_url
                .contains(&format!("state={}", start.state))
        );
        let poll = serde_json::to_string(&DiscordPoll {
            state: start.state.clone(),
        })
        .expect("poll");
        let (_, body) = send(&app, "POST", DISCORD_POLL_PATH, None, &poll).await;
        assert_eq!(
            serde_json::from_slice::<DiscordPollReply>(&body).expect("pending"),
            DiscordPollReply::Pending
        );
        let callback = format!(
            "{DISCORD_CALLBACK_PATH}?code={GOOD_CODE}&state={}",
            start.state
        );
        let (status, body) = send(&app, "GET", &callback, None, "").await;
        assert_eq!(
            (status, &body[..]),
            (StatusCode::OK, CALLBACK_DONE.as_bytes())
        );
        let (status, body) = send(&app, "POST", DISCORD_POLL_PATH, None, &poll).await;
        assert_eq!(status, StatusCode::OK);
        let DiscordPollReply::Ready(session) = serde_json::from_slice(&body).expect("ready") else {
            panic!("expected a finished login");
        };
        let (status, _) = send(&app, "POST", PLAY_PATH, Some(&session.launcher_token), "").await;
        assert_eq!(status, StatusCode::OK);
        let (status, body) = send(&app, "POST", DISCORD_POLL_PATH, None, &poll).await;
        assert_eq!(
            (status, error_code(&body).as_str()),
            (StatusCode::NOT_FOUND, "unknown_login")
        );
    }

    #[tokio::test]
    async fn discord_callback_refuses_bad_codes_and_unknown_states() {
        let state = state(AuthMode::Discord);
        let accounts = state.accounts.clone();
        let app = app(state);
        let (_, body) = send(&app, "POST", DISCORD_START_PATH, None, "").await;
        let start: DiscordStart = serde_json::from_slice(&body).expect("start");
        let bad = format!("{DISCORD_CALLBACK_PATH}?code=bad&state={}", start.state);
        let (status, _) = send(&app, "GET", &bad, None, "").await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        let unknown = format!("{DISCORD_CALLBACK_PATH}?code={GOOD_CODE}&state=unknown");
        let (status, body) = send(&app, "GET", &unknown, None, "").await;
        assert_eq!(
            (status, &body[..]),
            (StatusCode::NOT_FOUND, CALLBACK_EXPIRED.as_bytes())
        );
        let account = accounts
            .discord_account(FAKE_USER_ID, "Nelly")
            .await
            .expect("account");
        accounts.set_banned(account.id, true);
        let banned = format!(
            "{DISCORD_CALLBACK_PATH}?code={GOOD_CODE}&state={}",
            start.state
        );
        let (status, _) = send(&app, "GET", &banned, None, "").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}
