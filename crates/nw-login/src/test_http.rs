// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sends requests to a router in tests, and captures log lines.

use std::sync::{Arc, Mutex, PoisonError};

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

/// Log lines written on this thread while the capture lives.
pub(crate) struct LogCapture {
    lines: Arc<Mutex<Vec<u8>>>,
    _guard: tracing::subscriber::DefaultGuard,
}

struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for CaptureWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl LogCapture {
    /// Starts capturing the log lines of this thread.
    pub(crate) fn start() -> Self {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let target = Arc::clone(&lines);
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || CaptureWriter(Arc::clone(&target)))
            .finish();
        Self {
            lines,
            _guard: tracing::subscriber::set_default(subscriber),
        }
    }

    /// Everything logged so far.
    pub(crate) fn text(&self) -> String {
        let lines = self.lines.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8_lossy(&lines).into_owned()
    }
}

/// The `code` of an error body.
pub(crate) fn error_code(body: &Bytes) -> String {
    serde_json::from_slice::<ErrorBody>(body)
        .expect("error body")
        .code
}
