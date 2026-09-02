// SPDX-License-Identifier: AGPL-3.0-or-later
//! Request limits and the request counter (M2 spec section 10).

use std::time::Duration;

use axum::Router;
use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use nw_login::error::ApiError;
use tower::limit::GlobalConcurrencyLimitLayer;

/// Prometheus counter of answered requests, labelled by status code.
pub(crate) const REQUESTS_METRIC: &str = "edge_http_requests_total";
const STATUS_LABEL: &str = "status";

/// Adds the limit on requests in progress, the time limit, and the request counter. The time limit
/// sits outside the limit on requests in progress, so the wait for a free slot counts against it;
/// the counter sits outside both, so it also counts requests that ran out of time.
pub(crate) fn with_limits(router: Router, time_limit: Duration, max_in_flight: usize) -> Router {
    router
        .layer(GlobalConcurrencyLimitLayer::new(max_in_flight))
        .layer(middleware::from_fn_with_state(time_limit, limit_time))
        .layer(middleware::from_fn(count_requests))
}

async fn limit_time(State(limit): State<Duration>, request: Request, next: Next) -> Response {
    match tokio::time::timeout(limit, next.run(request)).await {
        Ok(response) => response,
        Err(_) => ApiError::Unavailable.into_response(),
    }
}

async fn count_requests(request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    metrics::counter!(REQUESTS_METRIC, STATUS_LABEL => response.status().as_u16().to_string())
        .increment(1);
    response
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::StatusCode;
    use axum::routing::get;
    use metrics_exporter_prometheus::PrometheusBuilder;
    use tower::ServiceExt;

    use super::*;

    const SHORT_LIMIT: Duration = Duration::from_millis(20);
    const SLOW_HANDLER: Duration = Duration::from_millis(500);
    const IN_FLIGHT: usize = 4;
    const ONE_SLOT: usize = 1;
    const WAITING_REQUESTS: usize = 5;
    const WAIT_LIMIT: Duration = Duration::from_millis(100);

    fn app_with(time_limit: Duration, slots: usize) -> Router {
        let router = Router::new().route("/fast", get(|| async { "ok" })).route(
            "/slow",
            get(|| async {
                tokio::time::sleep(SLOW_HANDLER).await;
                "late"
            }),
        );
        with_limits(router, time_limit, slots)
    }

    fn request(path: &str) -> Request {
        Request::builder()
            .uri(path)
            .body(Body::empty())
            .expect("request")
    }

    async fn status(path: &str) -> StatusCode {
        app_with(SHORT_LIMIT, IN_FLIGHT)
            .oneshot(request(path))
            .await
            .expect("response")
            .status()
    }

    #[tokio::test(start_paused = true)]
    async fn waiting_for_a_slot_counts_against_the_time_limit() {
        let app = app_with(WAIT_LIMIT, ONE_SLOT);
        let started = tokio::time::Instant::now();
        let requests: Vec<_> = (0..WAITING_REQUESTS)
            .map(|_| {
                let app = app.clone();
                tokio::spawn(async move {
                    app.oneshot(request("/slow"))
                        .await
                        .expect("response")
                        .status()
                })
            })
            .collect();
        for answer in requests {
            assert_eq!(answer.await.expect("task"), StatusCode::SERVICE_UNAVAILABLE);
        }
        assert!(
            started.elapsed() < WAIT_LIMIT * 2,
            "waited {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn slow_requests_are_cut_off() {
        assert_eq!(status("/fast").await, StatusCode::OK);
        assert_eq!(status("/slow").await, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn requests_are_counted_by_status() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        let _guard = metrics::set_default_local_recorder(&recorder);
        assert_eq!(status("/fast").await, StatusCode::OK);
        assert_eq!(status("/missing").await, StatusCode::NOT_FOUND);
        let text = handle.render();
        assert!(
            text.contains(&format!("{REQUESTS_METRIC}{{{STATUS_LABEL}=\"200\"}} 1")),
            "{text}"
        );
        assert!(
            text.contains(&format!("{REQUESTS_METRIC}{{{STATUS_LABEL}=\"404\"}} 1")),
            "{text}"
        );
    }
}
