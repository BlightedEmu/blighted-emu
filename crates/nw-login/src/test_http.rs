// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sends requests to a router in tests.

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use nw_protocol::launcher_api::ErrorBody;
use tower::ServiceExt;

/// Sends `request` and returns the status and the whole body.
pub(crate) async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Bytes) {
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, bytes)
}

/// A request with a text body.
pub(crate) fn request(method: &str, uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::from(body.to_owned()))
        .expect("request")
}

/// The `code` of an error body.
pub(crate) fn error_code(body: &Bytes) -> String {
    serde_json::from_slice::<ErrorBody>(body)
        .expect("error body")
        .code
}
