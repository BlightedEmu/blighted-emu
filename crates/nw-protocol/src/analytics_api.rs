// SPDX-License-Identifier: AGPL-3.0-or-later
//! The JSON of the analytics stream the client writes (study note
//! `docs/re/2026-09-18-leaderboards-and-analytics.md`): Kinesis `PutRecords`, as in the retail
//! `Hive_leaderboards` capture. Each record's `Data` is base64 of one JSON event with keys such
//! as `type`, `attributes` and `metrics`.

use serde::{Deserialize, Serialize};

/// The header that names the Kinesis operation.
pub const TARGET_HEADER: &str = "x-amz-target";
/// The one Kinesis operation the client's analytics uses.
pub const PUT_RECORDS_TARGET: &str = "Kinesis_20131202.PutRecords";
/// The content type of Kinesis requests and replies.
pub const KINESIS_CONTENT_TYPE: &str = "application/x-amz-json-1.1";
/// The most records one `PutRecords` takes (Kinesis service limit).
pub const MAX_RECORDS_PER_PUT: usize = 500;

/// The body of `PutRecords`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PutRecordsRequest {
    /// The records.
    pub records: Vec<PutRecordsEntry>,
    /// The stream, `regional-client-javelin-stream` in retail.
    pub stream_name: String,
}

/// One record of `PutRecords`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PutRecordsEntry {
    /// The record, base64.
    pub data: String,
    /// The partition key (a UUID in retail).
    pub partition_key: String,
}

/// The reply of `PutRecords`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PutRecordsReply {
    /// Records not stored.
    pub failed_record_count: u32,
    /// One result per record, in order.
    pub records: Vec<PutRecordsResult>,
}

/// The result of one stored record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PutRecordsResult {
    /// The record's place in the stream.
    pub sequence_number: String,
    /// The shard that holds it.
    pub shard_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_records_keys_are_pascal_case() {
        let request: PutRecordsRequest = serde_json::from_str(
            r#"{"Records":[{"Data":"e30=","PartitionKey":"k"}],"StreamName":"s"}"#,
        )
        .expect("request");
        assert_eq!(request.records[0].data, "e30=");
        let reply = PutRecordsReply {
            failed_record_count: 0,
            records: vec![PutRecordsResult {
                sequence_number: "1".to_owned(),
                shard_id: "shardId-000000000000".to_owned(),
            }],
        };
        assert_eq!(
            serde_json::to_value(&reply).expect("json"),
            serde_json::json!({
                "FailedRecordCount": 0,
                "Records": [{"SequenceNumber": "1", "ShardId": "shardId-000000000000"}]
            })
        );
    }
}
