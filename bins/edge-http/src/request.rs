// SPDX-License-Identifier: AGPL-3.0-or-later
//! `LoginQueueRequest` fields written by the client.
//!
//! Each field is optional. A missing key stays absent.
//! `ClientCapabilities` is an array of strings on the request.
//! The token field of the same name is a string, and the conversion is not in the note,
//! so the reply omits that token field.

use serde::Deserialize;

/// Longest id string this server accepts, in bytes.
pub(crate) const MAX_ID_LEN: usize = 128;

/// Longest Steam auth ticket this server stores, in bytes.
pub(crate) const MAX_STEAM_TICKET_LEN: usize = 4096;

/// Most capability strings accepted on one request.
pub(crate) const MAX_CAPABILITY_COUNT: usize = 64;

/// Longest one capability string, in bytes.
pub(crate) const MAX_CAPABILITY_LEN: usize = 128;

/// Body written by `FUN_1474ecc80` when the request object is present.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub(crate) struct LoginQueueRequest {
    /// Character id.
    pub character_id: Option<String>,
    /// Capability strings.
    pub client_capabilities: Option<Vec<String>>,
    /// Whether the account is a trial owner.
    pub is_trial_owner: Option<bool>,
    /// Steam application id.
    pub steam_app_id: Option<i64>,
    /// Steam auth ticket text. This server stores it and does not send it to Steam.
    pub steam_auth_ticket: Option<String>,
    /// World id.
    pub world_id: Option<String>,
}

/// Outer object. The writer emits `LoginQueueRequest` when its presence flag is set.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub(crate) struct LoginQueueRequestBody {
    /// Present when the client wrote the request object.
    pub login_queue_request: Option<LoginQueueRequest>,
}

/// Parses a queue body. An empty body is a request with every field absent.
pub(crate) fn parse_request(bytes: &[u8]) -> Result<LoginQueueRequest, serde_json::Error> {
    if bytes.is_empty() {
        return Ok(LoginQueueRequest::default());
    }
    let body: LoginQueueRequestBody = serde_json::from_slice(bytes)?;
    Ok(body.login_queue_request.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICKET: &str = "steam-ticket";
    const CHARACTER: &str = "char-1";
    const WORLD: &str = "world-1";
    const APP_ID: i64 = 1063730;

    #[test]
    fn absent_keys_stay_absent() {
        let request = parse_request(br#"{"LoginQueueRequest":{"SteamAuthTicket":"steam-ticket"}}"#)
            .expect("parse");
        assert_eq!(request.steam_auth_ticket.as_deref(), Some(TICKET));
        assert!(request.character_id.is_none());
        assert!(request.client_capabilities.is_none());
        assert!(request.is_trial_owner.is_none());
        assert!(request.steam_app_id.is_none());
        assert!(request.world_id.is_none());
    }

    #[test]
    fn writer_fields_parse() {
        let text = format!(
            r#"{{"LoginQueueRequest":{{"CharacterId":"{CHARACTER}","ClientCapabilities":["move","chat"],"IsTrialOwner":false,"SteamAppId":{APP_ID},"SteamAuthTicket":"{TICKET}","WorldId":"{WORLD}"}}}}"#
        );
        let request = parse_request(text.as_bytes()).expect("parse");
        assert_eq!(request.character_id.as_deref(), Some(CHARACTER));
        assert_eq!(
            request.client_capabilities,
            Some(vec!["move".to_owned(), "chat".to_owned()])
        );
        assert_eq!(request.is_trial_owner, Some(false));
        assert_eq!(request.steam_app_id, Some(APP_ID));
        assert_eq!(request.steam_auth_ticket.as_deref(), Some(TICKET));
        assert_eq!(request.world_id.as_deref(), Some(WORLD));
    }

    #[test]
    fn empty_body_has_no_fields() {
        let request = parse_request(b"").expect("empty");
        assert_eq!(request, LoginQueueRequest::default());
    }
}
