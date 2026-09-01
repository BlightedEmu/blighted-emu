// SPDX-License-Identifier: AGPL-3.0-or-later
//! Channel JSON fields the client reads after it fetches `<base>/<key>.json`.
//!
//! The parent key of the public-API objects is not named in the notes.
//! This parser collects every object that has `clientTag`, at any depth, except inside
//! `platformMetadata`. That walk is a parser rule. It is not a claim about the client's loop.

use serde_json::{Map, Value};

/// JSON key for a public API object's tag.
pub const FIELD_CLIENT_TAG: &str = "clientTag";

/// JSON key for the host or URL text a public API object stores.
pub const FIELD_API_ENDPOINT: &str = "apiEndpoint";

/// JSON key for the AWS region on `authStack` and `loginGateway`.
pub const FIELD_AWS_REGION: &str = "awsRegion";

/// JSON key of the object that holds the token and persona URLs.
pub const FIELD_PLATFORM_METADATA: &str = "platformMetadata";

/// JSON key for the Omni token URL. The client reads it from `platformMetadata`.
pub const FIELD_OMNI_TOKEN_URL: &str = "omniTokenUrl";

/// JSON key for the New World token URL. The same parser accepts it.
pub const FIELD_NW_TOKEN_URL: &str = "nwTokenUrl";

/// JSON key for the persona host. The same parser accepts it.
pub const FIELD_PERSONA_HOST_V2: &str = "personaHostV2";

/// `clientTag` value for the auth stack.
pub const CLIENT_TAG_AUTH_STACK: &str = "authStack";

/// `clientTag` value for the login gateway.
pub const CLIENT_TAG_LOGIN_GATEWAY: &str = "loginGateway";

/// `clientTag` value for the Javelin gateway.
pub const CLIENT_TAG_JAVELIN_GATEWAY_V2: &str = "JavelinGatewayServiceV2";

/// `clientTag` value for the CloudFront gateway tag.
/// The host field name for this tag is not in the notes.
pub const CLIENT_TAG_JAVELIN_GATEWAY_CF: &str = "JavelinGatewayService-CF";

/// `apiEndpoint` in the built-in local channel when the product id is `dev`.
/// Source: the endpoint note.
pub const DEV_LOCAL_API_ENDPOINT: &str = "127.0.0.1";

/// Scheme the client puts in front of `authStack` and `JavelinGatewayServiceV2`.
pub const HTTPS_URL_PREFIX: &str = "https://";

/// Path the client appends for `authStack` (`https://%s/Prod`).
pub const AUTH_STACK_PATH: &str = "/Prod";

/// Path the client appends for `JavelinGatewayServiceV2` (`https://%s/prod`).
pub const JAVELIN_GATEWAY_V2_PATH: &str = "/prod";

/// Tags whose `apiEndpoint` this parser can turn into the URL the client builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientTag {
    /// `authStack`. The client builds `https://%s/Prod`.
    AuthStack,
    /// `loginGateway`. The client uses the endpoint string as stored.
    LoginGateway,
    /// `JavelinGatewayServiceV2`. The client builds `https://%s/prod`.
    JavelinGatewayServiceV2,
}

impl ClientTag {
    /// The `clientTag` string the client compares.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AuthStack => CLIENT_TAG_AUTH_STACK,
            Self::LoginGateway => CLIENT_TAG_LOGIN_GATEWAY,
            Self::JavelinGatewayServiceV2 => CLIENT_TAG_JAVELIN_GATEWAY_V2,
        }
    }
}

/// One public API object that has `apiEndpoint`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicApi {
    /// Which tag selected this object.
    pub client_tag: ClientTag,
    /// `apiEndpoint` text, unchanged.
    pub api_endpoint: String,
    /// `awsRegion` for `authStack` and `loginGateway`.
    /// `JavelinGatewayServiceV2` does not read this field, so it stays empty.
    pub aws_region: Option<String>,
}

impl PublicApi {
    /// The URL string the client builds for this tag.
    ///
    /// `loginGateway` returns `api_endpoint` unchanged.
    pub fn client_url(&self) -> String {
        match self.client_tag {
            ClientTag::AuthStack => auth_stack_url(&self.api_endpoint),
            ClientTag::LoginGateway => login_gateway_url(&self.api_endpoint),
            ClientTag::JavelinGatewayServiceV2 => javelin_gateway_v2_url(&self.api_endpoint),
        }
    }
}

/// URL the client builds for `authStack`.
pub fn auth_stack_url(api_endpoint: &str) -> String {
    format!("{HTTPS_URL_PREFIX}{api_endpoint}{AUTH_STACK_PATH}")
}

/// URL the client uses for `loginGateway`: the stored endpoint text.
pub fn login_gateway_url(api_endpoint: &str) -> String {
    api_endpoint.to_owned()
}

/// URL the client builds for `JavelinGatewayServiceV2`.
pub fn javelin_gateway_v2_url(api_endpoint: &str) -> String {
    format!("{HTTPS_URL_PREFIX}{api_endpoint}{JAVELIN_GATEWAY_V2_PATH}")
}

/// Channel document fields this server must be able to serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelConfig {
    /// `platformMetadata.omniTokenUrl`.
    pub omni_token_url: String,
    /// `platformMetadata.nwTokenUrl` when the key is present.
    pub nw_token_url: Option<String>,
    /// `platformMetadata.personaHostV2` when the key is present.
    pub persona_host_v2: Option<String>,
    /// Public API objects that have `apiEndpoint`, in document order.
    pub public_apis: Vec<PublicApi>,
    /// Objects whose `clientTag` is `JavelinGatewayService-CF`.
    /// The host field name is not in the notes, so this count has no host.
    pub javelin_gateway_cf_count: usize,
}

/// Failure while reading a channel document.
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    /// The text is not JSON.
    #[error("channel document is not valid JSON")]
    InvalidJson(#[source] serde_json::Error),
    /// The root value is not an object.
    #[error("channel document must be a JSON object")]
    NotAnObject,
    /// `platformMetadata` is absent.
    #[error("channel document is missing {FIELD_PLATFORM_METADATA}")]
    MissingPlatformMetadata,
    /// `platformMetadata` is present and is not an object.
    #[error("{FIELD_PLATFORM_METADATA} must be a JSON object")]
    PlatformMetadataNotObject,
    /// A required key is absent.
    #[error("channel document is missing {field}")]
    MissingField {
        /// JSON key that was absent.
        field: &'static str,
    },
    /// A key is present but the value is not a string.
    #[error("{field} must be a string")]
    FieldNotString {
        /// JSON key whose value has the wrong type.
        field: &'static str,
    },
    /// A string key is present and empty.
    #[error("{field} is empty")]
    EmptyField {
        /// JSON key whose string is empty.
        field: &'static str,
    },
    /// `clientTag` is not one of the four tags the client selects.
    #[error("unknown clientTag `{tag}`")]
    UnknownClientTag {
        /// The tag text from the document.
        tag: String,
    },
}

/// Reads the channel fields the client selects.
pub fn parse_channel_config(text: &str) -> Result<ChannelConfig, ChannelError> {
    let value: Value = serde_json::from_str(text).map_err(ChannelError::InvalidJson)?;
    let Value::Object(root) = value else {
        return Err(ChannelError::NotAnObject);
    };
    let Some(metadata_value) = root.get(FIELD_PLATFORM_METADATA) else {
        return Err(ChannelError::MissingPlatformMetadata);
    };
    let Value::Object(metadata) = metadata_value else {
        return Err(ChannelError::PlatformMetadataNotObject);
    };
    let omni_token_url = require_nonempty_string(metadata, FIELD_OMNI_TOKEN_URL)?;
    let nw_token_url = optional_nonempty_string(metadata, FIELD_NW_TOKEN_URL)?;
    let persona_host_v2 = optional_nonempty_string(metadata, FIELD_PERSONA_HOST_V2)?;

    let mut public_apis = Vec::new();
    let mut javelin_gateway_cf_count = 0;
    walk_map(&root, &mut public_apis, &mut javelin_gateway_cf_count)?;

    Ok(ChannelConfig {
        omni_token_url,
        nw_token_url,
        persona_host_v2,
        public_apis,
        javelin_gateway_cf_count,
    })
}

fn walk_map(
    map: &Map<String, Value>,
    public_apis: &mut Vec<PublicApi>,
    javelin_gateway_cf_count: &mut usize,
) -> Result<(), ChannelError> {
    if map.contains_key(FIELD_CLIENT_TAG) {
        return take_public_api(map, public_apis, javelin_gateway_cf_count);
    }
    for (key, child) in map {
        if key == FIELD_PLATFORM_METADATA {
            continue;
        }
        walk_value(child, public_apis, javelin_gateway_cf_count)?;
    }
    Ok(())
}

fn walk_value(
    value: &Value,
    public_apis: &mut Vec<PublicApi>,
    javelin_gateway_cf_count: &mut usize,
) -> Result<(), ChannelError> {
    match value {
        Value::Object(map) => walk_map(map, public_apis, javelin_gateway_cf_count),
        Value::Array(items) => {
            for item in items {
                walk_value(item, public_apis, javelin_gateway_cf_count)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn take_public_api(
    map: &Map<String, Value>,
    public_apis: &mut Vec<PublicApi>,
    javelin_gateway_cf_count: &mut usize,
) -> Result<(), ChannelError> {
    let tag_text = require_nonempty_string(map, FIELD_CLIENT_TAG)?;
    match tag_text.as_str() {
        CLIENT_TAG_JAVELIN_GATEWAY_CF => {
            *javelin_gateway_cf_count += 1;
            Ok(())
        }
        CLIENT_TAG_AUTH_STACK => {
            public_apis.push(read_endpoint(map, ClientTag::AuthStack)?);
            Ok(())
        }
        CLIENT_TAG_LOGIN_GATEWAY => {
            public_apis.push(read_endpoint(map, ClientTag::LoginGateway)?);
            Ok(())
        }
        CLIENT_TAG_JAVELIN_GATEWAY_V2 => {
            public_apis.push(read_endpoint(map, ClientTag::JavelinGatewayServiceV2)?);
            Ok(())
        }
        _ => Err(ChannelError::UnknownClientTag { tag: tag_text }),
    }
}

fn read_endpoint(
    map: &Map<String, Value>,
    client_tag: ClientTag,
) -> Result<PublicApi, ChannelError> {
    let api_endpoint = require_nonempty_string(map, FIELD_API_ENDPOINT)?;
    let aws_region = match client_tag {
        ClientTag::AuthStack | ClientTag::LoginGateway => {
            Some(require_nonempty_string(map, FIELD_AWS_REGION)?)
        }
        ClientTag::JavelinGatewayServiceV2 => None,
    };
    Ok(PublicApi {
        client_tag,
        api_endpoint,
        aws_region,
    })
}

fn require_nonempty_string(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<String, ChannelError> {
    match object.get(field) {
        None => Err(ChannelError::MissingField { field }),
        Some(Value::String(text)) if text.is_empty() => Err(ChannelError::EmptyField { field }),
        Some(Value::String(text)) => Ok(text.clone()),
        Some(_) => Err(ChannelError::FieldNotString { field }),
    }
}

fn optional_nonempty_string(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<String>, ChannelError> {
    match object.get(field) {
        None => Ok(None),
        Some(_) => require_nonempty_string(object, field).map(Some),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TEST_OMNI_TOKEN_URL: &str = "https://tokens.example/games/new-world/tokens";
    const TEST_NW_TOKEN_URL: &str = "https://nw-tokens.example/token";
    const TEST_PERSONA_HOST: &str = "https://persona.example";
    const TEST_AWS_REGION: &str = "us-east-1";
    const TEST_API_HOST: &str = "edge.example";

    fn document(value: Value) -> String {
        serde_json::to_string(&value).expect("test document is JSON")
    }

    #[test]
    fn field_names_match_the_client_keys() {
        assert_eq!(FIELD_API_ENDPOINT, "apiEndpoint");
        assert_eq!(FIELD_AWS_REGION, "awsRegion");
        assert_eq!(FIELD_CLIENT_TAG, "clientTag");
        assert_eq!(FIELD_OMNI_TOKEN_URL, "omniTokenUrl");
        assert_eq!(FIELD_NW_TOKEN_URL, "nwTokenUrl");
        assert_eq!(FIELD_PERSONA_HOST_V2, "personaHostV2");
        assert_eq!(FIELD_PLATFORM_METADATA, "platformMetadata");
        assert_eq!(CLIENT_TAG_AUTH_STACK, "authStack");
        assert_eq!(CLIENT_TAG_LOGIN_GATEWAY, "loginGateway");
        assert_eq!(CLIENT_TAG_JAVELIN_GATEWAY_V2, "JavelinGatewayServiceV2");
        assert_eq!(CLIENT_TAG_JAVELIN_GATEWAY_CF, "JavelinGatewayService-CF");
        assert_eq!(DEV_LOCAL_API_ENDPOINT, "127.0.0.1");
        assert_eq!(AUTH_STACK_PATH, "/Prod");
        assert_eq!(JAVELIN_GATEWAY_V2_PATH, "/prod");
    }

    #[test]
    fn parses_api_endpoint_region_tag_and_omni_token_url() {
        let text = document(json!({
            FIELD_PLATFORM_METADATA: {
                FIELD_OMNI_TOKEN_URL: TEST_OMNI_TOKEN_URL,
                FIELD_NW_TOKEN_URL: TEST_NW_TOKEN_URL,
                FIELD_PERSONA_HOST_V2: TEST_PERSONA_HOST,
            },
            "groups": [
                {
                    FIELD_CLIENT_TAG: CLIENT_TAG_AUTH_STACK,
                    FIELD_API_ENDPOINT: TEST_API_HOST,
                    FIELD_AWS_REGION: TEST_AWS_REGION,
                    "extra": true,
                },
                {
                    FIELD_CLIENT_TAG: CLIENT_TAG_JAVELIN_GATEWAY_CF,
                },
                {
                    FIELD_CLIENT_TAG: CLIENT_TAG_JAVELIN_GATEWAY_V2,
                    FIELD_API_ENDPOINT: TEST_API_HOST,
                    FIELD_AWS_REGION: "ignored-for-this-tag",
                },
                {
                    FIELD_CLIENT_TAG: CLIENT_TAG_LOGIN_GATEWAY,
                    FIELD_API_ENDPOINT: DEV_LOCAL_API_ENDPOINT,
                    FIELD_AWS_REGION: TEST_AWS_REGION,
                }
            ]
        }));

        let config = parse_channel_config(&text).expect("channel document");
        assert_eq!(config.omni_token_url, TEST_OMNI_TOKEN_URL);
        assert_eq!(config.nw_token_url.as_deref(), Some(TEST_NW_TOKEN_URL));
        assert_eq!(config.persona_host_v2.as_deref(), Some(TEST_PERSONA_HOST));
        assert_eq!(config.javelin_gateway_cf_count, 1);
        assert_eq!(config.public_apis.len(), 3);

        let auth = &config.public_apis[0];
        assert_eq!(auth.client_tag, ClientTag::AuthStack);
        assert_eq!(auth.client_tag.as_str(), CLIENT_TAG_AUTH_STACK);
        assert_eq!(auth.api_endpoint, TEST_API_HOST);
        assert_eq!(auth.aws_region.as_deref(), Some(TEST_AWS_REGION));
        assert_eq!(auth.client_url(), auth_stack_url(TEST_API_HOST));
        assert_eq!(auth.client_url(), "https://edge.example/Prod");

        let gateway_v2 = &config.public_apis[1];
        assert_eq!(gateway_v2.client_tag, ClientTag::JavelinGatewayServiceV2);
        assert_eq!(gateway_v2.api_endpoint, TEST_API_HOST);
        assert_eq!(gateway_v2.aws_region, None);
        assert_eq!(
            gateway_v2.client_url(),
            javelin_gateway_v2_url(TEST_API_HOST)
        );
        assert_eq!(gateway_v2.client_url(), "https://edge.example/prod");

        let login = &config.public_apis[2];
        assert_eq!(login.client_tag, ClientTag::LoginGateway);
        assert_eq!(login.api_endpoint, DEV_LOCAL_API_ENDPOINT);
        assert_eq!(login.aws_region.as_deref(), Some(TEST_AWS_REGION));
        assert_eq!(
            login.client_url(),
            login_gateway_url(DEV_LOCAL_API_ENDPOINT)
        );
        assert_eq!(login.client_url(), DEV_LOCAL_API_ENDPOINT);
    }

    #[test]
    fn auth_stack_dev_endpoint_uses_prod_path() {
        let url = auth_stack_url(DEV_LOCAL_API_ENDPOINT);
        assert_eq!(url, "https://127.0.0.1/Prod");
    }

    #[test]
    fn login_gateway_on_the_root_object_keeps_the_stored_endpoint() {
        let text = document(json!({
            FIELD_PLATFORM_METADATA: {
                FIELD_OMNI_TOKEN_URL: TEST_OMNI_TOKEN_URL,
            },
            FIELD_CLIENT_TAG: CLIENT_TAG_LOGIN_GATEWAY,
            FIELD_API_ENDPOINT: "https://login.example/prod",
            FIELD_AWS_REGION: TEST_AWS_REGION,
        }));
        let config = parse_channel_config(&text).expect("root login gateway");
        assert_eq!(config.public_apis.len(), 1);
        assert_eq!(
            config.public_apis[0].client_url(),
            "https://login.example/prod"
        );
    }

    #[test]
    fn client_tag_inside_platform_metadata_is_not_a_public_api() {
        let text = document(json!({
            FIELD_PLATFORM_METADATA: {
                FIELD_OMNI_TOKEN_URL: TEST_OMNI_TOKEN_URL,
                FIELD_CLIENT_TAG: CLIENT_TAG_AUTH_STACK,
                FIELD_API_ENDPOINT: "should-not-count",
                FIELD_AWS_REGION: TEST_AWS_REGION,
            }
        }));
        let config = parse_channel_config(&text).expect("metadata only");
        assert!(config.public_apis.is_empty());
        assert_eq!(config.javelin_gateway_cf_count, 0);
        assert_eq!(config.omni_token_url, TEST_OMNI_TOKEN_URL);
        assert_eq!(config.nw_token_url, None);
        assert_eq!(config.persona_host_v2, None);
    }

    #[test]
    fn omni_token_url_outside_platform_metadata_does_not_count() {
        let text = document(json!({
            FIELD_OMNI_TOKEN_URL: TEST_OMNI_TOKEN_URL,
        }));
        let error = parse_channel_config(&text).expect_err("omni URL is in the wrong object");
        assert!(matches!(error, ChannelError::MissingPlatformMetadata));
    }

    #[test]
    fn missing_api_endpoint_is_an_error() {
        let text = document(json!({
            FIELD_PLATFORM_METADATA: { FIELD_OMNI_TOKEN_URL: TEST_OMNI_TOKEN_URL },
            "groups": [{
                FIELD_CLIENT_TAG: CLIENT_TAG_AUTH_STACK,
                FIELD_AWS_REGION: TEST_AWS_REGION,
            }]
        }));
        let error = parse_channel_config(&text).expect_err("apiEndpoint is required");
        assert!(matches!(
            error,
            ChannelError::MissingField {
                field: FIELD_API_ENDPOINT
            }
        ));
    }

    #[test]
    fn auth_stack_missing_aws_region_is_an_error() {
        let text = document(json!({
            FIELD_PLATFORM_METADATA: { FIELD_OMNI_TOKEN_URL: TEST_OMNI_TOKEN_URL },
            "groups": [{
                FIELD_CLIENT_TAG: CLIENT_TAG_AUTH_STACK,
                FIELD_API_ENDPOINT: TEST_API_HOST,
            }]
        }));
        let error = parse_channel_config(&text).expect_err("awsRegion is required");
        assert!(matches!(
            error,
            ChannelError::MissingField {
                field: FIELD_AWS_REGION
            }
        ));
    }

    #[test]
    fn empty_omni_token_url_is_an_error() {
        let text = document(json!({
            FIELD_PLATFORM_METADATA: { FIELD_OMNI_TOKEN_URL: "" },
        }));
        let error = parse_channel_config(&text).expect_err("empty omni URL");
        assert!(matches!(
            error,
            ChannelError::EmptyField {
                field: FIELD_OMNI_TOKEN_URL
            }
        ));
    }

    #[test]
    fn aws_region_must_be_a_string() {
        let text = document(json!({
            FIELD_PLATFORM_METADATA: { FIELD_OMNI_TOKEN_URL: TEST_OMNI_TOKEN_URL },
            "groups": [{
                FIELD_CLIENT_TAG: CLIENT_TAG_LOGIN_GATEWAY,
                FIELD_API_ENDPOINT: DEV_LOCAL_API_ENDPOINT,
                FIELD_AWS_REGION: 1,
            }]
        }));
        let error = parse_channel_config(&text).expect_err("region type");
        assert!(matches!(
            error,
            ChannelError::FieldNotString {
                field: FIELD_AWS_REGION
            }
        ));
    }

    #[test]
    fn unknown_client_tag_is_an_error() {
        let text = document(json!({
            FIELD_PLATFORM_METADATA: { FIELD_OMNI_TOKEN_URL: TEST_OMNI_TOKEN_URL },
            "groups": [{
                FIELD_CLIENT_TAG: "not-a-tag",
                FIELD_API_ENDPOINT: TEST_API_HOST,
            }]
        }));
        let error = parse_channel_config(&text).expect_err("unknown tag");
        assert!(matches!(error, ChannelError::UnknownClientTag { tag } if tag == "not-a-tag"));
    }

    #[test]
    fn snake_case_api_endpoint_key_is_not_accepted() {
        let text = document(json!({
            FIELD_PLATFORM_METADATA: { FIELD_OMNI_TOKEN_URL: TEST_OMNI_TOKEN_URL },
            "groups": [{
                FIELD_CLIENT_TAG: CLIENT_TAG_JAVELIN_GATEWAY_V2,
                "api_endpoint": TEST_API_HOST,
            }]
        }));
        let error = parse_channel_config(&text).expect_err("wrong key");
        assert!(matches!(
            error,
            ChannelError::MissingField {
                field: FIELD_API_ENDPOINT
            }
        ));
    }

    #[test]
    fn root_must_be_an_object() {
        let error = parse_channel_config("[]").expect_err("array root");
        assert!(matches!(error, ChannelError::NotAnObject));
    }

    #[test]
    fn invalid_json_is_an_error() {
        let error = parse_channel_config("{").expect_err("truncated JSON");
        assert!(matches!(error, ChannelError::InvalidJson(_)));
    }

    #[test]
    fn platform_metadata_must_be_an_object() {
        let text = document(json!({ FIELD_PLATFORM_METADATA: [TEST_OMNI_TOKEN_URL] }));
        let error = parse_channel_config(&text).expect_err("metadata type");
        assert!(matches!(error, ChannelError::PlatformMetadataNotObject));
    }
}
