// SPDX-License-Identifier: AGPL-3.0-or-later
//! Refuses a `POST` that another web site could send (admin tools spec section 7): it needs the
//! header `X-NW-Admin: 1`, which a cross-site request cannot set without a CORS pre-flight that
//! this server does not answer, and an `Origin`, when sent, of this server.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Header every changing request needs.
pub const ADMIN_HEADER: &str = "x-nw-admin";
/// Its value.
pub const ADMIN_HEADER_VALUE: &str = "1";
const LOCALHOST: &str = "localhost";

/// The origins of this server: the listen address and, on a loopback address, `localhost`.
#[derive(Debug, Clone)]
pub struct AdminOrigin(Arc<[String]>);

impl AdminOrigin {
    /// The origins of a server on `listen`.
    pub fn of(listen: SocketAddr) -> Self {
        let mut origins = vec![format!("http://{listen}")];
        if listen.ip().is_loopback() {
            origins.push(format!("http://{LOCALHOST}:{}", listen.port()));
        }
        Self(origins.into())
    }

    fn allows(&self, origin: &[u8]) -> bool {
        self.0.iter().any(|own| own.as_bytes() == origin)
    }
}

/// Lets `GET` and `HEAD` through; any other method needs [`ADMIN_HEADER`] and, when sent, an
/// `Origin` of this server. Others get 403.
pub async fn admin_guard(
    State(origin): State<AdminOrigin>,
    request: Request,
    next: Next,
) -> Response {
    if request.method() != Method::GET && request.method() != Method::HEAD {
        let headers = request.headers();
        let marked = headers
            .get(ADMIN_HEADER)
            .is_some_and(|value| value == ADMIN_HEADER_VALUE);
        let same_origin = headers
            .get(header::ORIGIN)
            .is_none_or(|value| origin.allows(value.as_bytes()));
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
        Request::builder().method("POST").uri("/test")
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
    async fn a_get_needs_no_header() {
        let app = Router::new()
            .route("/read", axum::routing::get(|| async { "data" }))
            .layer(axum::middleware::from_fn_with_state(
                AdminOrigin::of(LISTEN.parse().expect("address")),
                admin_guard,
            ));
        let request = Request::builder()
            .uri("/read")
            .body(Body::empty())
            .expect("request");
        assert_eq!(
            app.oneshot(request).await.expect("response").status(),
            StatusCode::OK
        );
    }
}
