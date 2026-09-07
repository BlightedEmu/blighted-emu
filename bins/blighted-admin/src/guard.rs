// SPDX-License-Identifier: AGPL-3.0-or-later
//! Refuses requests that another web site could send (admin tools spec section 7). Every request
//! needs a `Host` of this server: a page whose name was rebound to 127.0.0.1 (DNS rebinding)
//! sends its own name and cannot read the capture data. A request other than `GET` or `HEAD`
//! also needs the header `X-Blighted-Admin: 1`, which a cross-site request cannot set without a CORS
//! pre-flight that this server does not answer, and an `Origin`, when sent, of this server.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Header every changing request needs.
pub const ADMIN_HEADER: &str = "x-blighted-admin";
/// Its value.
pub const ADMIN_HEADER_VALUE: &str = "1";
const LOCALHOST: &str = "localhost";

const HTTP_SCHEME: &str = "http://";

/// The hosts of this server: the listen address and, on a loopback address, `localhost`.
#[derive(Debug, Clone)]
pub struct AdminOrigin(Arc<[String]>);

impl AdminOrigin {
    /// The hosts of a server on `listen`.
    pub fn of(listen: SocketAddr) -> Self {
        let mut hosts = vec![listen.to_string()];
        if listen.ip().is_loopback() {
            hosts.push(format!("{LOCALHOST}:{}", listen.port()));
        }
        Self(hosts.into())
    }

    fn allows_host(&self, host: &[u8]) -> bool {
        self.0.iter().any(|own| own.as_bytes() == host)
    }

    fn allows_origin(&self, origin: &[u8]) -> bool {
        origin
            .strip_prefix(HTTP_SCHEME.as_bytes())
            .is_some_and(|host| self.allows_host(host))
    }
}

/// Needs a `Host` of this server; a method other than `GET` and `HEAD` also needs
/// [`ADMIN_HEADER`] and, when sent, an `Origin` of this server. Others get 403.
pub async fn admin_guard(
    State(origin): State<AdminOrigin>,
    request: Request,
    next: Next,
) -> Response {
    let own_host = request
        .headers()
        .get(header::HOST)
        .is_some_and(|host| origin.allows_host(host.as_bytes()));
    if !own_host {
        return StatusCode::FORBIDDEN.into_response();
    }
    if request.method() != Method::GET && request.method() != Method::HEAD {
        let headers = request.headers();
        let marked = headers
            .get(ADMIN_HEADER)
            .is_some_and(|value| value == ADMIN_HEADER_VALUE);
        let same_origin = headers
            .get(header::ORIGIN)
            .is_none_or(|value| origin.allows_origin(value.as_bytes()));
        if !(marked && same_origin) {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::post;
    use tower::ServiceExt;

    use super::*;

    const LISTEN: &str = "127.0.0.1:9180";

    fn app() -> Router {
        Router::new()
            .route("/test", post(|| async { "done" }))
            .layer(axum::middleware::from_fn_with_state(
                AdminOrigin::of(LISTEN.parse().expect("address")),
                admin_guard,
            ))
    }

    async fn status(request: Request<Body>) -> StatusCode {
        app().oneshot(request).await.expect("response").status()
    }

    fn post_to_test() -> axum::http::request::Builder {
        Request::builder()
            .method("POST")
            .uri("/test")
            .header("host", LISTEN)
    }

    #[tokio::test]
    async fn a_post_with_the_admin_header_passes() {
        let request = post_to_test()
            .header(ADMIN_HEADER, ADMIN_HEADER_VALUE)
            .header("origin", "http://127.0.0.1:9180")
            .body(Body::empty())
            .expect("request");
        assert_eq!(status(request).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn a_post_without_the_admin_header_is_refused() {
        let request = post_to_test().body(Body::empty()).expect("request");
        assert_eq!(status(request).await, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_post_from_another_origin_is_refused() {
        let request = post_to_test()
            .header(ADMIN_HEADER, ADMIN_HEADER_VALUE)
            .header("origin", "https://example.com")
            .body(Body::empty())
            .expect("request");
        assert_eq!(status(request).await, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_request_for_another_host_is_refused() {
        for host in [Some("evil.example:9180"), None] {
            let mut request = Request::builder().uri("/read");
            if let Some(host) = host {
                request = request.header("host", host);
            }
            let app = Router::new()
                .route("/read", axum::routing::get(|| async { "data" }))
                .layer(axum::middleware::from_fn_with_state(
                    AdminOrigin::of(LISTEN.parse().expect("address")),
                    admin_guard,
                ));
            let response = app
                .oneshot(request.body(Body::empty()).expect("request"))
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{host:?}");
        }
    }

    #[tokio::test]
    async fn a_get_needs_no_header() {
        let app = Router::new()
            .route("/read", axum::routing::get(|| async { "data" }))
            .layer(axum::middleware::from_fn_with_state(
                AdminOrigin::of(LISTEN.parse().expect("address")),
                admin_guard,
            ));
        let request = Request::builder()
            .uri("/read")
            .header("host", "localhost:9180")
            .body(Body::empty())
            .expect("request");
        assert_eq!(
            app.oneshot(request).await.expect("response").status(),
            StatusCode::OK
        );
    }
}
