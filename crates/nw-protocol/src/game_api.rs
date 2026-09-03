// SPDX-License-Identifier: AGPL-3.0-or-later
//! JSON that the game client reads and writes before character select (JSON note
//! `docs/re/2026-09-02-login-json-shapes.md`).
//!
//! The client reads every field through a presence check, so the server leaves out what it does
//! not know. Serialization omits a key whose value is absent. Gateway models use PascalCase keys;
//! the channel document uses camelCase keys.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Stage path that the client appends to the auth stack endpoint (`https://%s/Prod`).
pub const AUTH_STACK_STAGE: &str = "/Prod";
/// Path that the client appends to the auth stack URL for the Steam ticket exchange.
pub const STEAM_AUTH_PATH: &str = "/steam";
/// Stage path that the client puts between the gateway endpoint and an operation path.
pub const GATEWAY_STAGE: &str = "/prod";
/// `GetLoginInfo` operation path.
pub const GET_LOGIN_INFO_PATH: &str = "/game/getlogininfo";
/// `ListWorlds` operation path.
pub const LIST_WORLDS_PATH: &str = "/game/worlds/list";
/// First part of the character operation paths `/game/worlds/<world id>/characters...`.
pub const WORLDS_PATH: &str = "/game/worlds";
/// `CreateCharacter` path after the world id.
pub const CHARACTERS_SEGMENT: &str = "/characters";
/// `ValidateCharacter` path after the world id.
pub const VALIDATOR_SEGMENT: &str = "/characters/validator";
/// `QueueLogin` operation path; `RefreshQueuedLogin` appends `/<ticket id>`.
pub const QUEUE_LOGIN_PATH: &str = "/game/login/queue/v2";

/// `clientTag` of the auth stack API.
pub const AUTH_STACK_TAG: &str = "authStack";
/// `clientTag` of the login gateway API.
pub const LOGIN_GATEWAY_TAG: &str = "loginGateway";
/// `clientTag` of the HTTP gateway API.
pub const HTTP_GATEWAY_TAG: &str = "JavelinGatewayServiceV2";
/// `WorldStatus` of a world that players may pick (follow-up note section 8).
pub const WORLD_STATUS_ACTIVE: &str = "ACTIVE";
/// `PublicStatusCode` of an open world; the client accepts only codes inside the bit mask
/// `0x1748`, and 0 passes (follow-up note section 8).
pub const PUBLIC_STATUS_OPEN: i64 = 0;

/// The channel document `<key>.json` (JSON note section 2). Clusters are top-level keys. It has
/// no `platformMetadata`: the client reads Omni settings only on the auth backend that skips
/// `/steam` (unknowns note, second pass section 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelDocument {
    /// Channel name, logged by the client.
    pub channel_name: String,
    /// Clusters by cluster id.
    #[serde(flatten)]
    pub clusters: BTreeMap<String, Cluster>,
}

/// One cluster of the channel document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cluster {
    /// Name shown by the client.
    pub localized_name: String,
    /// API endpoints. The client drops a cluster without them.
    pub public_apis: Vec<PublicApi>,
}

/// One `publicApis` element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicApi {
    /// Which API this is, for example [`AUTH_STACK_TAG`].
    pub client_tag: String,
    /// Host and optional path, without a scheme; the client puts `https://` in front.
    pub api_endpoint: String,
    /// Region text.
    pub aws_region: String,
}

/// Reply of the auth stack `/steam` call (JSON note section 3). All four values are strings; the
/// client signs later gateway calls with them, and the server ignores that signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SteamAuthReply {
    /// Access key id.
    pub access_key_id: String,
    /// Secret key.
    pub secret_key: String,
    /// Session token.
    pub session_token: String,
    /// Expiry as an RFC 822 date in GMT.
    pub expiration: String,
}

/// Reply of `GetLoginInfo` (JSON note section 5.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LoginInfoReply {
    /// The object the client looks up by name.
    pub login_info_list: LoginInfoList,
}

/// `LoginInfoList`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LoginInfoList {
    /// Characters of the account in every world.
    pub characters: Vec<CharacterSummary>,
    /// Characters the account may have on this server. Without it the select screen shows `0/0`
    /// and offers no slot to create a character (exit test, 2026-09-02).
    pub max_channel_characters: i64,
    /// Names of those characters (`includeNames`).
    pub name_reservations: Vec<NameRecord>,
    /// Worlds of the server.
    pub worlds: Vec<WorldModel>,
}

/// One character (JSON note section 5.1.1). The display name is in [`NameRecord`]. Dates are
/// RFC 822 text in GMT, which the client tries first (follow-up note section 6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CharacterSummary {
    /// Character id.
    pub character_id: String,
    /// Creation date, RFC 822 in GMT.
    pub created_date: String,
    /// Name as the player typed it; the select screen shows it (unknowns note, third pass
    /// section 2).
    pub name: String,
    /// Lower-case name.
    pub normalized_name: String,
    /// Account id.
    pub persona_id: String,
    /// World id.
    pub world_id: String,
}

/// One name record (JSON note section 5.1.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NameRecord {
    /// Creation date, RFC 822 in GMT.
    pub created_date: String,
    /// Name as the player typed it.
    pub display_name: String,
    /// Lower-case name.
    pub normalized_name: String,
    /// Character id that owns the name.
    pub owning_resource_id: String,
    /// Account id.
    pub persona_id: String,
}

/// One world (JSON note section 5.2). A player can pick the world only when `WorldStatus` is
/// [`WORLD_STATUS_ACTIVE`] and `PublicStatusCode` passes (follow-up note section 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct WorldModel {
    /// Whether the world takes no more players.
    pub is_full: bool,
    /// Characters one account may have in this world.
    pub max_account_characters: i64,
    /// Players allowed at one time.
    pub max_connection_count: i64,
    /// Name shown in the list.
    pub public_name: String,
    /// Status code; see [`PUBLIC_STATUS_OPEN`].
    pub public_status_code: i64,
    /// World id.
    pub world_id: String,
    /// World name.
    pub world_name: String,
    /// Status text; see [`WORLD_STATUS_ACTIVE`].
    pub world_status: String,
}

/// Reply of `ListWorlds` (JSON note section 5.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ListWorldsReply {
    /// The object the client looks up by name.
    pub list_worlds_response_body: ListWorldsBody,
}

/// `ListWorldsResponseBody`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ListWorldsBody {
    /// Worlds of the server.
    pub worlds: Vec<WorldModel>,
}

/// Request of `ValidateCharacter` (JSON note section 5.4). The world id is in the path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct ValidateCharacterRequest {
    /// The body object; absent when the client did not write it.
    pub validate_character_body: Option<ValidateCharacterBody>,
}

/// `ValidateCharacterBody`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct ValidateCharacterBody {
    /// Name to check.
    pub name: Option<String>,
}

/// Request of `CreateCharacter` (JSON note section 5.6). The world id is in the path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct CreateCharacterRequestBody {
    /// The request object; absent when the client did not write it.
    pub create_character_request: Option<CreateCharacterRequest>,
}

/// `CreateCharacterRequest`. Every field is optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct CreateCharacterRequest {
    /// Creation settings as one JSON string (content Unknown).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub character_creation_params: Option<String>,
    /// Capability strings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_capabilities: Option<Vec<String>>,
    /// Character name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Whether to skip the first-time experience.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_ftue: Option<bool>,
    /// Steam application id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steam_app_id: Option<i64>,
    /// Steam auth ticket. The server never trusts or stores it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steam_auth_ticket: Option<String>,
}

/// Reply of `CreateCharacter`: the new character.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateCharacterReply {
    /// The object the client looks up by name.
    pub character: CharacterSummary,
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORLD_ID: &str = "8d0f4c2e-0000-4000-8000-000000000001";
    const CHARACTER_ID: &str = "0b5e0a3c-0000-4000-8000-000000000002";
    const ACCOUNT_ID: &str = "0b5e0a3c-0000-4000-8000-000000000003";
    const DATE: &str = "Tue, 14 Oct 2025 00:00:00 GMT";
    const ENDPOINT: &str = "game.example.org/s/session";
    const CLUSTER_ID: &str = "blighted";
    const MAX_CHARACTERS: i64 = 2;
    const CAPACITY: i64 = 100;
    const STEAM_APP_ID: i64 = 1_063_730;

    fn keys(value: &serde_json::Value) -> Vec<&str> {
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("json object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        keys
    }

    fn json<T: Serialize>(value: &T) -> serde_json::Value {
        serde_json::to_value(value).expect("serialize")
    }

    fn summary() -> CharacterSummary {
        CharacterSummary {
            character_id: CHARACTER_ID.to_owned(),
            created_date: DATE.to_owned(),
            name: "Ada".to_owned(),
            normalized_name: "ada".to_owned(),
            persona_id: ACCOUNT_ID.to_owned(),
            world_id: WORLD_ID.to_owned(),
        }
    }

    fn world() -> WorldModel {
        WorldModel {
            is_full: false,
            max_account_characters: MAX_CHARACTERS,
            max_connection_count: CAPACITY,
            public_name: "Aeternum".to_owned(),
            public_status_code: PUBLIC_STATUS_OPEN,
            world_id: WORLD_ID.to_owned(),
            world_name: "Aeternum".to_owned(),
            world_status: WORLD_STATUS_ACTIVE.to_owned(),
        }
    }

    #[test]
    fn channel_document_puts_clusters_at_the_top() {
        let api = |tag: &str| PublicApi {
            client_tag: tag.to_owned(),
            api_endpoint: ENDPOINT.to_owned(),
            aws_region: "us-east".to_owned(),
        };
        let document = ChannelDocument {
            channel_name: "Test".to_owned(),
            clusters: BTreeMap::from([(
                CLUSTER_ID.to_owned(),
                Cluster {
                    localized_name: "Test".to_owned(),
                    public_apis: vec![api(AUTH_STACK_TAG), api(LOGIN_GATEWAY_TAG)],
                },
            )]),
        };
        let value = json(&document);
        assert_eq!(keys(&value), [CLUSTER_ID, "channelName"]);
        assert_eq!(keys(&value[CLUSTER_ID]), ["localizedName", "publicApis"]);
        assert_eq!(
            keys(&value[CLUSTER_ID]["publicApis"][0]),
            ["apiEndpoint", "awsRegion", "clientTag"]
        );
        assert_eq!(
            value[CLUSTER_ID]["publicApis"][1]["clientTag"],
            LOGIN_GATEWAY_TAG
        );
        assert_eq!(
            serde_json::from_value::<ChannelDocument>(value).expect("parse"),
            document
        );
    }

    #[test]
    fn steam_reply_has_the_client_keys() {
        let steam = SteamAuthReply {
            access_key_id: "a".to_owned(),
            secret_key: "b".to_owned(),
            session_token: "c".to_owned(),
            expiration: DATE.to_owned(),
        };
        assert_eq!(
            keys(&json(&steam)),
            ["AccessKeyId", "Expiration", "SecretKey", "SessionToken"]
        );
    }

    #[test]
    fn login_info_has_the_client_keys() {
        let reply = LoginInfoReply {
            login_info_list: LoginInfoList {
                characters: vec![summary()],
                max_channel_characters: MAX_CHARACTERS,
                name_reservations: vec![NameRecord {
                    created_date: DATE.to_owned(),
                    display_name: "Ada".to_owned(),
                    normalized_name: "ada".to_owned(),
                    owning_resource_id: CHARACTER_ID.to_owned(),
                    persona_id: ACCOUNT_ID.to_owned(),
                }],
                worlds: vec![world()],
            },
        };
        let value = json(&reply);
        assert_eq!(keys(&value), ["LoginInfoList"]);
        let list = &value["LoginInfoList"];
        assert_eq!(
            keys(list),
            [
                "Characters",
                "MaxChannelCharacters",
                "NameReservations",
                "Worlds"
            ]
        );
        assert_eq!(
            keys(&list["Characters"][0]),
            [
                "CharacterId",
                "CreatedDate",
                "Name",
                "NormalizedName",
                "PersonaId",
                "WorldId"
            ]
        );
        assert_eq!(
            keys(&list["NameReservations"][0]),
            [
                "CreatedDate",
                "DisplayName",
                "NormalizedName",
                "OwningResourceId",
                "PersonaId"
            ]
        );
        assert_eq!(
            keys(&list["Worlds"][0]),
            [
                "IsFull",
                "MaxAccountCharacters",
                "MaxConnectionCount",
                "PublicName",
                "PublicStatusCode",
                "WorldId",
                "WorldName",
                "WorldStatus"
            ]
        );
        assert_eq!(list["Worlds"][0]["WorldStatus"], WORLD_STATUS_ACTIVE);
    }

    #[test]
    fn world_list_and_create_reply_use_their_envelopes() {
        let worlds = ListWorldsReply {
            list_worlds_response_body: ListWorldsBody {
                worlds: vec![world()],
            },
        };
        let value = json(&worlds);
        assert_eq!(keys(&value), ["ListWorldsResponseBody"]);
        assert_eq!(keys(&value["ListWorldsResponseBody"]), ["Worlds"]);
        let created = json(&CreateCharacterReply {
            character: summary(),
        });
        assert_eq!(keys(&created), ["Character"]);
    }

    #[test]
    fn character_requests_parse_with_missing_fields() {
        let create: CreateCharacterRequestBody = serde_json::from_str(
            r#"{"CreateCharacterRequest":{"Name":"Ada","CharacterCreationParams":"{}","SteamAuthTicket":"t","SteamAppId":1063730}}"#,
        )
        .expect("create");
        let request = create.create_character_request.expect("request");
        assert_eq!(request.name.as_deref(), Some("Ada"));
        assert_eq!(request.steam_app_id, Some(STEAM_APP_ID));
        assert_eq!(request.skip_ftue, None);
        let empty: CreateCharacterRequestBody = serde_json::from_str("{}").expect("empty");
        assert_eq!(empty.create_character_request, None);
        let validate: ValidateCharacterRequest =
            serde_json::from_str(r#"{"ValidateCharacterBody":{"Name":"Ada"}}"#).expect("validate");
        assert_eq!(
            validate
                .validate_character_body
                .and_then(|body| body.name)
                .as_deref(),
            Some("Ada")
        );
    }
}
