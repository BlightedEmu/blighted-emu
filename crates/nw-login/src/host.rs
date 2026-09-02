// SPDX-License-Identifier: AGPL-3.0-or-later
//! Routing by host name (M2 spec section 14, parent spec 4.2). `login.<domain>` serves the
//! launcher API and the channel document; `game.<domain>` serves the game routes. A request for
//! any other host gets `421`.

use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::header::HOST;
use axum::response::{IntoResponse, Response};
use tower::ServiceExt;

use crate::discord::DiscordApi;
use crate::error::ApiError;
use crate::game::{GameState, channel_router, game_router, unknown_route};
use crate::launcher::{LauncherState, launcher_router};
use crate::queue::QueueStore;
use crate::store::{
    AccountStore, CharacterStore, DiscordStateStore, GameSessionStore, RateLimiter,
};

const PORT_SEPARATOR: char = ':';
/// A fully qualified host name may end in a dot.
const ROOT_LABEL: char = '.';

#[derive(Clone)]
struct HostRoutes {
    login_host: Arc<str>,
    game_host: Arc<str>,
    login: Router,
    game: Router,
}

/// The host a request names: the URI authority (HTTP/2) or the `Host` header (HTTP/1.1),
/// without port and final dot.
fn request_host(request: &Request) -> Option<&str> {
    let host = match request.uri().authority() {
        Some(authority) => authority.host(),
        None => {
            let header = request.headers().get(HOST)?.to_str().ok()?;
            header
                .rsplit_once(PORT_SEPARATOR)
                .filter(|(_, port)| port.bytes().all(|byte| byte.is_ascii_digit()))
                .map_or(header, |(host, _)| host)
        }
    };
    Some(host.trim_end_matches(ROOT_LABEL))
}

async fn by_host(State(routes): State<HostRoutes>, request: Request) -> Response {
    let target = match request_host(&request) {
        Some(host) if host.eq_ignore_ascii_case(&routes.login_host) => routes.login,
        Some(host) if host.eq_ignore_ascii_case(&routes.game_host) => routes.game,
        _ => return ApiError::UnknownHost.into_response(),
    };
    match target.oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    }
}

/// Every route of `edge-http`, chosen by host name.
pub fn edge_router<A, G, R, D, X, C, Q>(
    launcher: LauncherState<A, G, R, D, X>,
    game: GameState<G, C, R, Q>,
) -> Router
where
    A: AccountStore,
    G: GameSessionStore,
    R: RateLimiter,
    D: DiscordStateStore,
    X: DiscordApi,
    C: CharacterStore,
    Q: QueueStore,
{
    let routes = HostRoutes {
        login_host: game.config.login_host().into(),
        game_host: game.config.game_host().into(),
        login: launcher_router(launcher)
            .merge(channel_router(game.clone()))
            .fallback(unknown_route)
            .method_not_allowed_fallback(unknown_route),
        game: game_router(game),
    };
    Router::new().fallback(by_host).with_state(routes)
}

#[cfg(test)]
mod tests {
    use axum::body::{Body, Bytes};
    use axum::http::{Request as HttpRequest, StatusCode};
    use nw_protocol::game_api::{AUTH_STACK_STAGE, STEAM_AUTH_PATH};
    use nw_protocol::launcher_api::{AuthMode, INFO_PATH, LOGIN_PATH};

    use super::*;
    use crate::game::SESSION_PREFIX;
    use crate::game::tests::fixture;
    use crate::launcher::tests::state;
    use crate::test_http::{self, error_code};

    const LOGIN_HOST: &str = "login.example.org";
    const GAME_HOST: &str = "game.example.org";

    async fn app_and_session() -> (Router, String) {
        let game = fixture().await;
        let mut launcher = state(AuthMode::Password);
        launcher.game_sessions = game.state.game_sessions.clone();
        (edge_router(launcher, game.state), game.session)
    }

    async fn get(app: &Router, host: &str, path: &str) -> (StatusCode, Bytes) {
        let request = HttpRequest::builder()
            .method("GET")
            .uri(path)
            .header(HOST, host)
            .body(Body::empty())
            .expect("request");
        test_http::send(app, request).await
    }

    #[tokio::test]
    async fn login_host_serves_launcher_and_channel() {
        let (app, session) = app_and_session().await;
        let (status, _) = get(&app, LOGIN_HOST, INFO_PATH).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = get(&app, &format!("{LOGIN_HOST}:443"), INFO_PATH).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = get(&app, "LOGIN.example.org.", &format!("/{session}.json")).await;
        assert_eq!(status, StatusCode::OK);
        for path in ["/favicon.ico", LOGIN_PATH] {
            let (status, body) = get(&app, LOGIN_HOST, path).await;
            assert_eq!(
                (status, error_code(&body).as_str()),
                (StatusCode::NOT_FOUND, "unknown_route"),
                "{path}"
            );
        }
    }

    #[tokio::test]
    async fn game_host_serves_only_game_routes() {
        let (app, session) = app_and_session().await;
        let steam = format!("{SESSION_PREFIX}/{session}{AUTH_STACK_STAGE}{STEAM_AUTH_PATH}");
        let request = HttpRequest::builder()
            .method("POST")
            .uri(format!("https://{GAME_HOST}{steam}"))
            .body(Body::empty())
            .expect("request");
        let (status, _) = test_http::send(&app, request).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = get(&app, GAME_HOST, INFO_PATH).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = get(&app, GAME_HOST, &format!("/{session}.json")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn other_hosts_are_refused() {
        let (app, _) = app_and_session().await;
        for host in ["example.org", "login.example.org.evil.net", "10.0.0.1"] {
            let (status, body) = get(&app, host, INFO_PATH).await;
            assert_eq!(
                (status, error_code(&body).as_str()),
                (StatusCode::MISDIRECTED_REQUEST, "unknown_host"),
                "{host}"
            );
        }
        let request = HttpRequest::builder()
            .uri(INFO_PATH)
            .body(Body::empty())
            .expect("request");
        let (status, _) = test_http::send(&app, request).await;
        assert_eq!(status, StatusCode::MISDIRECTED_REQUEST);
    }
}
