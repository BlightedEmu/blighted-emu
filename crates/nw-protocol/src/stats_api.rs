// SPDX-License-Identifier: AGPL-3.0-or-later
//! The JSON of the stats service the client reads leaderboards from (study note
//! `docs/re/2026-09-18-leaderboards-and-analytics.md`; retail captures `Hive_leaderboards` and
//! `Check_Leaderboard`). The client's `StatsserviceapiClient` sends:
//!
//! - `GET /<alias>/<namespace>/leaderboards/<board>?pageNumber=1&pageSize=100` (`GetLeaderboard`)
//! - `POST /<alias>/<namespace>/leaderboards/<board>` with [`EntitiesRequest`]
//!   (`GetLeaderboardForEntities`)
//! - `POST /<alias>/<namespace>/stats` with [`StatsRequest`] (`GetStats`)
//!
//! Absent values are `null`, as in the retail replies.

use serde::{Deserialize, Serialize};

/// The game alias in every retail path (`omni.default-game-alias`).
pub const GAME_ALIAS: &str = "new-world";
/// The path segment in front of a board id.
pub const LEADERBOARDS_SEGMENT: &str = "leaderboards";
/// The last path segment of `GetStats`.
pub const STATS_SEGMENT: &str = "stats";
/// The board id prefix of boards whose lowest value ranks first (clear times).
pub const LOWEST_FIRST_PREFIX: &str = "min-";

/// The query of `GetLeaderboard`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageQuery {
    /// The page, from 1.
    pub page_number: u32,
    /// Rows per page (100 in the retail captures).
    pub page_size: u32,
}

/// The body of `GetLeaderboardForEntities`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitiesRequest {
    /// The entities whose rows the client wants.
    pub entity_ids: Vec<String>,
}

/// One leaderboard row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaderboardEntry {
    /// The value; `null` for an entity with no value.
    pub value: Option<f64>,
    /// The entity: a character id, a company id, or the ids of a group joined by `_`.
    pub entity_id: String,
    /// The match of the value (always `null` in the retail captures).
    pub match_id: Option<String>,
    /// The place, from 1; `null` for an entity with no value.
    pub rank: Option<u64>,
}

/// The reply of `GetLeaderboard` and `GetLeaderboardForEntities`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaderboardReply {
    /// The rows.
    pub leaderboard_entries: Vec<LeaderboardEntry>,
    /// The rows of the whole board.
    pub leaderboard_size: u64,
    /// `true` or `null` in the retail captures; its meaning is Unknown. This server sends `null`.
    pub is_backed_up: Option<bool>,
}

/// The body of `GetStats`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsRequest {
    /// The entities; empty for global stats.
    pub entity_ids: Vec<String>,
    /// The stat ids.
    pub stat_ids: Vec<String>,
    /// `Global`, `Character`, `Company` or `Group`.
    pub stat_scope: String,
}

/// One stat value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatEntry {
    /// The value.
    pub value: f64,
    /// The stat id.
    pub stat_id: String,
    /// The entity; `null` for a global stat.
    pub entity_id: Option<String>,
}

/// The reply of `GetStats`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsReply {
    /// The stored values.
    pub stats: Vec<StatEntry>,
    /// ISO 8601 UTC time of the newest change, for example `2026-06-30T09:53:59.123Z`.
    pub last_updated_date: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_have_the_retail_keys() {
        let reply = LeaderboardReply {
            leaderboard_entries: vec![LeaderboardEntry {
                value: None,
                entity_id: "e".to_owned(),
                match_id: None,
                rank: None,
            }],
            leaderboard_size: 0,
            is_backed_up: None,
        };
        assert_eq!(
            serde_json::to_value(&reply).expect("json"),
            serde_json::json!({
                "leaderboardEntries": [
                    {"value": null, "entityId": "e", "matchId": null, "rank": null}
                ],
                "leaderboardSize": 0,
                "isBackedUp": null
            })
        );
        let stats: StatsRequest =
            serde_json::from_str(r#"{"entityIds":[],"statIds":["a"],"statScope":"Global"}"#)
                .expect("request");
        assert_eq!(stats.stat_ids, ["a"]);
        let query: PageQuery =
            serde_json::from_str(r#"{"pageNumber":1,"pageSize":100}"#).expect("query");
        assert_eq!(query.page_size, 100);
    }
}
