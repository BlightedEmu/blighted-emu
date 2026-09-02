// SPDX-License-Identifier: GPL-3.0-or-later
//! Client of the server's launcher API (M2 spec section 5). Calls block; the window runs them
//! on a worker thread.

use std::time::Duration;

use nw_protocol::launcher_api::{
    DISCORD_POLL_PATH, DISCORD_START_PATH, DiscordPoll, DiscordPollReply, DiscordStart, ErrorBody,
    INFO_PATH, LOGIN_PATH, LOGOUT_PATH, LauncherInfo, LauncherSession, PLAY_PATH,
    PasswordCredentials, PlayReply, REGISTER_PATH,
};
use reqwest::blocking::{Client, RequestBuilder};
use serde::de::DeserializeOwned;

/// Time limit of one call.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(15);

/// A failed call.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// The server could not be reached.
    #[error("server unreachable: {0}")]
    Unreachable(#[source] reqwest::Error),
    /// The server refused the call.
    #[error("{message}")]
    Server {
        /// HTTP status.
        status: u16,
        /// Error code from the body.
        code: String,
        /// Error text from the body.
        message: String,
    },
    /// The reply is not the expected JSON.
    #[error("server reply is not valid: {0}")]
    BadReply(#[source] reqwest::Error),
}

/// The launcher API of one server.
#[derive(Debug, Clone)]
pub struct LauncherApi {
    base: String,
    client: Client,
}

impl LauncherApi {
    /// A client for the API at `base` (for example `https://login.example.org`). Needs the
    /// process crypto provider ([`crate::install_crypto_provider`]).
    pub fn new(base: String) -> Result<Self, ApiError> {
        let client = Client::builder()
            .timeout(CALL_TIMEOUT)
            .build()
            .map_err(ApiError::Unreachable)?;
        Ok(Self { base, client })
    }

    /// Server name, login mode and launch arguments.
    pub fn info(&self) -> Result<LauncherInfo, ApiError> {
        self.call(self.client.get(self.url(INFO_PATH)))
    }

    /// Makes a password account and logs in.
    pub fn register(&self, credentials: &PasswordCredentials) -> Result<LauncherSession, ApiError> {
        self.call(self.client.post(self.url(REGISTER_PATH)).json(credentials))
    }

    /// Logs in with a password.
    pub fn login(&self, credentials: &PasswordCredentials) -> Result<LauncherSession, ApiError> {
        self.call(self.client.post(self.url(LOGIN_PATH)).json(credentials))
    }

    /// A game session key for one client start.
    pub fn play(&self, launcher_token: &str) -> Result<PlayReply, ApiError> {
        self.call(
            self.client
                .post(self.url(PLAY_PATH))
                .bearer_auth(launcher_token),
        )
    }

    /// Ends the launcher session.
    pub fn logout(&self, launcher_token: &str) -> Result<(), ApiError> {
        let response = self
            .client
            .post(self.url(LOGOUT_PATH))
            .bearer_auth(launcher_token)
            .send()
            .map_err(ApiError::Unreachable)?;
        check_status(response).map(drop)
    }

    /// Starts a Discord login.
    pub fn discord_start(&self) -> Result<DiscordStart, ApiError> {
        self.call(self.client.post(self.url(DISCORD_START_PATH)))
    }

    /// Asks whether a Discord login finished.
    pub fn discord_poll(&self, state: &str) -> Result<DiscordPollReply, ApiError> {
        let body = DiscordPoll {
            state: state.to_owned(),
        };
        self.call(self.client.post(self.url(DISCORD_POLL_PATH)).json(&body))
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn call<T: DeserializeOwned>(&self, request: RequestBuilder) -> Result<T, ApiError> {
        let response = request.send().map_err(ApiError::Unreachable)?;
        check_status(response)?.json().map_err(ApiError::BadReply)
    }
}

fn check_status(
    response: reqwest::blocking::Response,
) -> Result<reqwest::blocking::Response, ApiError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body: ErrorBody = response.json().map_err(ApiError::BadReply)?;
    Err(ApiError::Server {
        status: status.as_u16(),
        code: body.code,
        message: body.message,
    })
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use nw_protocol::launcher_api::AuthMode;

    use super::*;

    const TOKEN: &str = "launcher-token";
    const KEY: &str = "game-session-key";
    const EXPIRES_AT: i64 = 1_760_400_000;

    fn session() -> LauncherSession {
        LauncherSession {
            launcher_token: TOKEN.to_owned(),
            expires_at: EXPIRES_AT,
        }
    }

    /// Serves a fake launcher API on a free port in its own thread and returns its base URL.
    fn fake_server() -> String {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().expect("runtime");
            runtime.block_on(async move {
                let app = Router::new()
                    .route(
                        INFO_PATH,
                        get(|| async {
                            Json(LauncherInfo {
                                server_name: "Fake".to_owned(),
                                auth_mode: AuthMode::Password,
                                allow_registration: true,
                                launch_arguments: Vec::new(),
                            })
                        }),
                    )
                    .route(LOGIN_PATH, post(|| async { Json(session()) }))
                    .route(
                        PLAY_PATH,
                        post(|headers: HeaderMap| async move {
                            let bearer = format!("Bearer {TOKEN}");
                            if headers
                                .get("authorization")
                                .and_then(|value| value.to_str().ok())
                                == Some(bearer.as_str())
                            {
                                Ok(Json(PlayReply {
                                    game_session_key: KEY.to_owned(),
                                }))
                            } else {
                                Err((
                                    StatusCode::UNAUTHORIZED,
                                    Json(ErrorBody {
                                        code: "no_session".to_owned(),
                                        message: "log in again".to_owned(),
                                    }),
                                ))
                            }
                        }),
                    );
                let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
                    .await
                    .expect("bind");
                sender
                    .send(listener.local_addr().expect("address"))
                    .expect("send");
                axum::serve(listener, app).await.expect("serve");
            });
        });
        let address = receiver.recv().expect("address");
        format!("http://{address}")
    }

    #[test]
    fn calls_parse_replies_and_errors() {
        crate::install_crypto_provider();
        let api = LauncherApi::new(fake_server()).expect("client");
        assert_eq!(api.info().expect("info").server_name, "Fake");
        let credentials = PasswordCredentials {
            username: "ada".to_owned(),
            password: "password-ok".to_owned(),
        };
        assert_eq!(api.login(&credentials).expect("login"), session());
        assert_eq!(api.play(TOKEN).expect("play").game_session_key, KEY);
        match api.play("other") {
            Err(ApiError::Server { status, code, .. }) => {
                assert_eq!((status, code.as_str()), (401, "no_session"));
            }
            other => panic!("expected a server error, got {other:?}"),
        }
    }

    #[test]
    fn unreachable_server_is_reported() {
        crate::install_crypto_provider();
        let api = LauncherApi::new("http://127.0.0.1:9".to_owned()).expect("client");
        assert!(matches!(api.info(), Err(ApiError::Unreachable(_))));
    }
}
