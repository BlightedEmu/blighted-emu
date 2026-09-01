// SPDX-License-Identifier: AGPL-3.0-or-later
//! Login-queue JSON from protocol note section 1.3.
//!
//! The HTTP body is one object. Its key is `LoginQueueResponse`.
//! Every field below that key is optional. Serialization omits a key whose value is absent.
//! `RepAddress` and `Signature` are strings on `Token`. They are not keys of `LoginQueueResponse`.

use serde::{Deserialize, Serialize};

/// HTTP body that carries a [`LoginQueueResponse`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LoginQueueReply {
    /// Queue object. The client looks this key up by name.
    pub login_queue_response: LoginQueueResponse,
}

/// Value of the `LoginQueueResponse` key.
///
/// A missing JSON key deserializes as [`None`]. [`None`] is omitted on the way back out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct LoginQueueResponse {
    /// Whether the client may transfer to another queue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_queue_transfer: Option<bool>,
    /// Estimated wait, as a JSON integer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_time: Option<i64>,
    /// Place in the queue, as a JSON integer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<i64>,
    /// Name of this queue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_name: Option<String>,
    /// World id the client is told to transfer to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_transfer_world_id: Option<String>,
    /// How long the client waits before it asks again, as a JSON integer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_interval: Option<i64>,
    /// Nested token. Present only when the `Token` key is present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<LoginQueueToken>,
}

/// Value of the `Token` key inside [`LoginQueueResponse`].
///
/// `RepAddress` and `Signature` live here. They are strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct LoginQueueToken {
    /// Account age, as a JSON integer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_age: Option<i64>,
    /// Whether the account is locked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_is_locked: Option<bool>,
    /// Channel id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    /// Character id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub character_id: Option<String>,
    /// Client capability text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_capabilities: Option<String>,
    /// Time the token was generated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generate_time: Option<String>,
    /// Whether the account owns the full game.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_permanent_app_owner: Option<bool>,
    /// Whether the account is a trial owner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_trial_owner: Option<bool>,
    /// Time the token was issued.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue_time: Option<String>,
    /// Location group id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_group_id: Option<String>,
    /// Location id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_id: Option<String>,
    /// Persona id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona_id: Option<String>,
    /// REP address string. The client prints this as one string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rep_address: Option<String>,
    /// Signature string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// Steam application id, as a JSON integer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steam_app_id: Option<i64>,
    /// Steam user id string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steam_user_id: Option<String>,
    /// Ticket id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticket_id: Option<String>,
    /// Token version, as a JSON integer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_version: Option<i64>,
    /// World id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub world_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALLOW_QUEUE_TRANSFER: bool = false;
    const ESTIMATED_TIME: i64 = 0;
    const POSITION: i64 = 4;
    const QUEUE_NAME: &str = "";
    const RECOMMENDED_TRANSFER_WORLD_ID: &str = "world-b";
    const REFRESH_INTERVAL: i64 = 30;
    const ACCOUNT_AGE: i64 = 100;
    const ACCOUNT_IS_LOCKED: bool = true;
    const CHANNEL_ID: &str = "live";
    const CHARACTER_ID: &str = "char-1";
    const CLIENT_CAPABILITIES: &str = "caps";
    const GENERATE_TIME: &str = "2026-08-31T00:00:00Z";
    const IS_PERMANENT_APP_OWNER: bool = true;
    const IS_TRIAL_OWNER: bool = false;
    const ISSUE_TIME: &str = "2026-08-31T00:00:01Z";
    const LOCATION_GROUP_ID: &str = "group-1";
    const LOCATION_ID: &str = "loc-1";
    const PERSONA_ID: &str = "persona-1";
    const REP_ADDRESS: &str = "rep.example:4242";
    const SIGNATURE: &str = "sig-1";
    const STEAM_APP_ID: i64 = 1063730;
    const STEAM_USER_ID: &str = "76561198000000000";
    const TICKET_ID: &str = "ticket-1";
    const TOKEN_VERSION: i64 = 3;
    const WORLD_ID: &str = "world-1";

    const PARTIAL_POSITION: i64 = 7;
    const PARTIAL_REP_ADDRESS: &str = "10.1.2.3:4242";

    const RESPONSE_KEYS: [&str; 7] = [
        "AllowQueueTransfer",
        "EstimatedTime",
        "Position",
        "QueueName",
        "RecommendedTransferWorldId",
        "RefreshInterval",
        "Token",
    ];
    const TOKEN_KEYS: [&str; 19] = [
        "AccountAge",
        "AccountIsLocked",
        "ChannelId",
        "CharacterId",
        "ClientCapabilities",
        "GenerateTime",
        "IsPermanentAppOwner",
        "IsTrialOwner",
        "IssueTime",
        "LocationGroupId",
        "LocationId",
        "PersonaId",
        "RepAddress",
        "Signature",
        "SteamAppId",
        "SteamUserId",
        "TicketId",
        "TokenVersion",
        "WorldId",
    ];

    fn full_token() -> LoginQueueToken {
        LoginQueueToken {
            account_age: Some(ACCOUNT_AGE),
            account_is_locked: Some(ACCOUNT_IS_LOCKED),
            channel_id: Some(CHANNEL_ID.to_owned()),
            character_id: Some(CHARACTER_ID.to_owned()),
            client_capabilities: Some(CLIENT_CAPABILITIES.to_owned()),
            generate_time: Some(GENERATE_TIME.to_owned()),
            is_permanent_app_owner: Some(IS_PERMANENT_APP_OWNER),
            is_trial_owner: Some(IS_TRIAL_OWNER),
            issue_time: Some(ISSUE_TIME.to_owned()),
            location_group_id: Some(LOCATION_GROUP_ID.to_owned()),
            location_id: Some(LOCATION_ID.to_owned()),
            persona_id: Some(PERSONA_ID.to_owned()),
            rep_address: Some(REP_ADDRESS.to_owned()),
            signature: Some(SIGNATURE.to_owned()),
            steam_app_id: Some(STEAM_APP_ID),
            steam_user_id: Some(STEAM_USER_ID.to_owned()),
            ticket_id: Some(TICKET_ID.to_owned()),
            token_version: Some(TOKEN_VERSION),
            world_id: Some(WORLD_ID.to_owned()),
        }
    }

    fn full_reply() -> LoginQueueReply {
        LoginQueueReply {
            login_queue_response: LoginQueueResponse {
                allow_queue_transfer: Some(ALLOW_QUEUE_TRANSFER),
                estimated_time: Some(ESTIMATED_TIME),
                position: Some(POSITION),
                queue_name: Some(QUEUE_NAME.to_owned()),
                recommended_transfer_world_id: Some(RECOMMENDED_TRANSFER_WORLD_ID.to_owned()),
                refresh_interval: Some(REFRESH_INTERVAL),
                token: Some(full_token()),
            },
        }
    }

    fn partial_reply() -> LoginQueueReply {
        LoginQueueReply {
            login_queue_response: LoginQueueResponse {
                position: Some(PARTIAL_POSITION),
                token: Some(LoginQueueToken {
                    rep_address: Some(PARTIAL_REP_ADDRESS.to_owned()),
                    ..LoginQueueToken::default()
                }),
                ..LoginQueueResponse::default()
            },
        }
    }

    fn object_keys(value: &serde_json::Value) -> Vec<&str> {
        value
            .as_object()
            .expect("json object")
            .keys()
            .map(String::as_str)
            .collect()
    }

    #[test]
    fn full_object_round_trip_keeps_present_keys() {
        let reply = full_reply();
        let text = serde_json::to_string(&reply).expect("serialize full reply");
        let parsed: LoginQueueReply = serde_json::from_str(&text).expect("parse full reply");
        assert_eq!(parsed, reply);

        let value: serde_json::Value = serde_json::from_str(&text).expect("parse json");
        assert_eq!(object_keys(&value), ["LoginQueueResponse"]);
        let response = &value["LoginQueueResponse"];
        assert_eq!(object_keys(response), RESPONSE_KEYS);
        assert!(response.get("RepAddress").is_none());
        assert!(response.get("Signature").is_none());
        assert_eq!(response["AllowQueueTransfer"], ALLOW_QUEUE_TRANSFER);
        assert_eq!(response["EstimatedTime"], ESTIMATED_TIME);
        assert_eq!(response["QueueName"], QUEUE_NAME);
        let token = &response["Token"];
        assert_eq!(object_keys(token), TOKEN_KEYS);
        assert_eq!(token["RepAddress"], REP_ADDRESS);
        assert_eq!(token["Signature"], SIGNATURE);
        assert_eq!(token["AccountAge"], ACCOUNT_AGE);
        assert_eq!(token["AccountIsLocked"], ACCOUNT_IS_LOCKED);
        assert_eq!(token["IsTrialOwner"], IS_TRIAL_OWNER);
        assert_eq!(token["SteamAppId"], STEAM_APP_ID);
        assert_eq!(token["TokenVersion"], TOKEN_VERSION);
        assert_eq!(token["WorldId"], WORLD_ID);
    }

    #[test]
    fn partial_object_omits_absent_keys() {
        let reply = partial_reply();
        let text = serde_json::to_string(&reply).expect("serialize partial reply");
        let parsed: LoginQueueReply = serde_json::from_str(&text).expect("parse partial reply");
        assert_eq!(parsed, reply);
        assert!(parsed.login_queue_response.allow_queue_transfer.is_none());
        assert!(parsed.login_queue_response.estimated_time.is_none());
        assert!(parsed.login_queue_response.queue_name.is_none());
        assert!(
            parsed
                .login_queue_response
                .recommended_transfer_world_id
                .is_none()
        );
        assert!(parsed.login_queue_response.refresh_interval.is_none());
        let token = parsed
            .login_queue_response
            .token
            .as_ref()
            .expect("token present");
        assert_eq!(token.rep_address.as_deref(), Some(PARTIAL_REP_ADDRESS));
        assert!(token.signature.is_none());
        assert!(token.account_age.is_none());
        assert!(token.world_id.is_none());

        let value: serde_json::Value = serde_json::from_str(&text).expect("parse json");
        let response = &value["LoginQueueResponse"];
        assert_eq!(object_keys(response), ["Position", "Token"]);
        assert_eq!(response["Position"], PARTIAL_POSITION);
        assert_eq!(object_keys(&response["Token"]), ["RepAddress"]);
        assert_eq!(response["Token"]["RepAddress"], PARTIAL_REP_ADDRESS);
        assert!(response.get("Signature").is_none());
        assert!(response["Token"].get("Signature").is_none());

        let again = serde_json::to_string(&parsed).expect("serialize again");
        let again_value: serde_json::Value = serde_json::from_str(&again).expect("parse again");
        assert_eq!(again_value, value);
    }

    #[test]
    fn missing_outer_key_is_rejected() {
        let text = format!(r#"{{"Position":{PARTIAL_POSITION}}}"#);
        let parsed = serde_json::from_str::<LoginQueueReply>(&text);
        assert!(parsed.is_err());
    }

    #[test]
    fn string_position_is_rejected() {
        let text = r#"{"LoginQueueResponse":{"Position":"7"}}"#;
        let parsed = serde_json::from_str::<LoginQueueReply>(text);
        assert!(parsed.is_err());
    }
}
