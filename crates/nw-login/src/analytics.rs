// SPDX-License-Identifier: AGPL-3.0-or-later
//! Our analytics stream (study note `docs/re/2026-09-18-leaderboards-and-analytics.md`).
//!
//! The client has no endpoint key for its analytics: it puts its events on the Kinesis stream at
//! `kinesis.<analytics.region-name>.amazonaws.com` with credentials from an STS `AssumeRole` at
//! `sts.<region>.amazonaws.com`. When those names lead to `edge-http`, these routes answer both:
//! STS hands out credentials that only this server reads (it checks no signature), and Kinesis
//! `PutRecords` stores the events in the global database.

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Form, Router};
use data_encoding::BASE64;
use nw_common::time::{iso8601_seconds, unix_seconds};
use nw_db::AnalyticsRecord;
use nw_protocol::analytics_api::{
    KINESIS_CONTENT_TYPE, MAX_RECORDS_PER_PUT, PUT_RECORDS_TARGET, PutRecordsReply,
    PutRecordsRequest, PutRecordsResult, TARGET_HEADER,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::error::{ApiError, parse_json};
use crate::game::AUTH_STACK_CREDENTIAL;
use crate::store::AnalyticsStore;

/// The Kinesis and STS host names (shared with the launcher, which maps them to this server).
pub use nw_protocol::aws_mirror::{is_kinesis_host, is_sts_host};

/// Largest `PutRecords` body (the Kinesis limit of one request).
pub const KINESIS_BODY_LIMIT_BYTES: usize = 5 * 1024 * 1024;
/// Largest STS body: one form.
const STS_BODY_LIMIT_BYTES: usize = 16 * 1024;
/// Prometheus counter of stored analytics records.
pub const RECORDS_METRIC: &str = "edge_http_analytics_records_total";
/// The shard every record goes to: this stream has one.
const SHARD_ID: &str = "shardId-000000000000";
/// The one STS action the client sends.
const ASSUME_ROLE_ACTION: &str = "AssumeRole";
/// Credential lifetime bounds and default of `AssumeRole` (the STS limits).
const MIN_DURATION_SECONDS: u64 = 900;
const MAX_DURATION_SECONDS: u64 = 43_200;
const DEFAULT_DURATION_SECONDS: u64 = 3600;
const XML_CONTENT_TYPE: &str = "text/xml";

/// What the Kinesis route needs.
#[derive(Clone)]
pub struct AnalyticsState<A> {
    /// The stored records.
    pub analytics: A,
}

/// The Kinesis route: `POST /` with `X-Amz-Target: Kinesis_20131202.PutRecords`.
pub fn kinesis_router<A: AnalyticsStore>(state: AnalyticsState<A>) -> Router {
    Router::new()
        .route("/", post(put_records::<A>))
        .layer(DefaultBodyLimit::max(KINESIS_BODY_LIMIT_BYTES))
        .with_state(state)
}

/// The STS route: `POST /` with an `AssumeRole` form.
pub fn sts_router() -> Router {
    Router::new()
        .route("/", post(assume_role))
        .layer(DefaultBodyLimit::max(STS_BODY_LIMIT_BYTES))
}

async fn put_records<A: AnalyticsStore>(
    State(state): State<AnalyticsState<A>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let target = headers
        .get(TARGET_HEADER)
        .and_then(|value| value.to_str().ok());
    if target != Some(PUT_RECORDS_TARGET) {
        return Err(ApiError::UnknownRoute);
    }
    let request: PutRecordsRequest = parse_json(&body)?;
    if request.records.is_empty() || request.records.len() > MAX_RECORDS_PER_PUT {
        return Err(ApiError::BadRequest);
    }
    let records = request
        .records
        .into_iter()
        .map(|entry| {
            BASE64
                .decode(entry.data.as_bytes())
                .map(|data| AnalyticsRecord {
                    partition_key: entry.partition_key,
                    data,
                })
                .map_err(|_| ApiError::BadRequest)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let ids = state
        .analytics
        .put_records(&request.stream_name, &records)
        .await?;
    metrics::counter!(RECORDS_METRIC).increment(u64::try_from(ids.len()).unwrap_or(u64::MAX));
    let reply = PutRecordsReply {
        failed_record_count: 0,
        records: ids
            .into_iter()
            .map(|id| PutRecordsResult {
                sequence_number: id.to_string(),
                shard_id: SHARD_ID.to_owned(),
            })
            .collect(),
    };
    let json = serde_json::to_vec(&reply).map_err(|_| ApiError::Unavailable)?;
    Ok(([(CONTENT_TYPE, KINESIS_CONTENT_TYPE)], json).into_response())
}

/// The form of an STS request; the other fields are ignored.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct StsForm {
    action: String,
    role_arn: Option<String>,
    role_session_name: Option<String>,
    duration_seconds: Option<u64>,
}

/// `text` with the five XML special characters escaped.
pub(crate) fn xml_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

/// `AssumeRole`: credentials for the role the client names, valid for the time it asks (within
/// the STS bounds). The keys are the same as those of the `/steam` reply; this server checks no
/// signature.
async fn assume_role(Form(form): Form<StsForm>) -> Result<Response, ApiError> {
    if form.action != ASSUME_ROLE_ACTION {
        return Err(ApiError::BadRequest);
    }
    let duration = form
        .duration_seconds
        .unwrap_or(DEFAULT_DURATION_SECONDS)
        .clamp(MIN_DURATION_SECONDS, MAX_DURATION_SECONDS);
    let now = u64::try_from(unix_seconds()).unwrap_or(0);
    let expiration = iso8601_seconds(now.saturating_add(duration));
    let role = xml_text(form.role_arn.as_deref().unwrap_or_default());
    let session = xml_text(form.role_session_name.as_deref().unwrap_or_default());
    let body = format!(
        "<AssumeRoleResponse xmlns=\"https://sts.amazonaws.com/doc/2011-06-15/\">\
         <AssumeRoleResult>\
         <AssumedRoleUser><AssumedRoleId>{AUTH_STACK_CREDENTIAL}:{session}</AssumedRoleId>\
         <Arn>{role}</Arn></AssumedRoleUser>\
         <Credentials><AccessKeyId>{AUTH_STACK_CREDENTIAL}</AccessKeyId>\
         <SecretAccessKey>{AUTH_STACK_CREDENTIAL}</SecretAccessKey>\
         <SessionToken>{AUTH_STACK_CREDENTIAL}</SessionToken>\
         <Expiration>{expiration}</Expiration></Credentials>\
         </AssumeRoleResult>\
         <ResponseMetadata><RequestId>{}</RequestId></ResponseMetadata>\
         </AssumeRoleResponse>",
        Uuid::new_v4()
    );
    Ok((StatusCode::OK, [(CONTENT_TYPE, XML_CONTENT_TYPE)], body).into_response())
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;

    use super::*;
    use crate::memory::MemoryAnalytics;
    use crate::test_http::{self, error_code};

    const STREAM: &str = "regional-client-javelin-stream";

    fn kinesis(target: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/")
            .header(TARGET_HEADER, target)
            .header(CONTENT_TYPE, KINESIS_CONTENT_TYPE)
            .body(Body::from(body.to_owned()))
            .expect("request")
    }

    fn sts(body: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body.to_owned()))
            .expect("request")
    }

    #[tokio::test]
    async fn put_records_stores_each_decoded_record() {
        let analytics = MemoryAnalytics::default();
        let app = kinesis_router(AnalyticsState {
            analytics: analytics.clone(),
        });
        let event = br#"{"type":"leaderboard_view"}"#;
        let body = serde_json::json!({
            "Records": [
                {"Data": BASE64.encode(event), "PartitionKey": "k1"},
                {"Data": BASE64.encode(b"x"), "PartitionKey": "k2"}
            ],
            "StreamName": STREAM
        })
        .to_string();
        let (status, bytes) = test_http::send(&app, kinesis(PUT_RECORDS_TARGET, &body)).await;
        assert_eq!(status, StatusCode::OK);
        let reply: PutRecordsReply = serde_json::from_slice(&bytes).expect("reply");
        assert_eq!(reply.failed_record_count, 0);
        assert_eq!(
            reply
                .records
                .iter()
                .map(|record| record.sequence_number.as_str())
                .collect::<Vec<_>>(),
            ["0", "1"]
        );
        assert_eq!(
            analytics.records(),
            [
                (
                    STREAM.to_owned(),
                    AnalyticsRecord {
                        partition_key: "k1".to_owned(),
                        data: event.to_vec()
                    }
                ),
                (
                    STREAM.to_owned(),
                    AnalyticsRecord {
                        partition_key: "k2".to_owned(),
                        data: b"x".to_vec()
                    }
                ),
            ]
        );
    }

    #[tokio::test]
    async fn other_kinesis_requests_are_refused() {
        let analytics = MemoryAnalytics::default();
        let app = kinesis_router(AnalyticsState {
            analytics: analytics.clone(),
        });
        let valid = r#"{"Records":[{"Data":"e30=","PartitionKey":"k"}],"StreamName":"s"}"#;
        let cases = [
            ("Kinesis_20131202.DeleteStream", valid, "unknown_route"),
            (
                PUT_RECORDS_TARGET,
                r#"{"Records":[],"StreamName":"s"}"#,
                "bad_request",
            ),
            (
                PUT_RECORDS_TARGET,
                r#"{"Records":[{"Data":"not base64!","PartitionKey":"k"}],"StreamName":"s"}"#,
                "bad_request",
            ),
        ];
        for (target, body, code) in cases {
            let (status, bytes) = test_http::send(&app, kinesis(target, body)).await;
            assert!(status.is_client_error(), "{target}");
            assert_eq!(error_code(&bytes), code, "{body}");
        }
        assert!(analytics.records().is_empty());
    }

    #[tokio::test]
    async fn assume_role_hands_out_credentials() {
        let app = sts_router();
        let (status, bytes) = test_http::send(
            &app,
            sts("Action=AssumeRole&RoleArn=arn%3Aaws%3Aiam%3A%3A1%3Arole%2Fr%3Cx&RoleSessionName=s&DurationSeconds=900&Version=2011-06-15"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let text = String::from_utf8(bytes.to_vec()).expect("text");
        assert!(
            text.contains("<AccessKeyId>blighted</AccessKeyId>"),
            "{text}"
        );
        assert!(
            text.contains("<Arn>arn:aws:iam::1:role/r&lt;x</Arn>"),
            "{text}"
        );
        assert!(text.contains("<Expiration>"), "{text}");
        let (status, _) = test_http::send(&app, sts("Action=GetCallerIdentity")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
