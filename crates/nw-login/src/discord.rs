// SPDX-License-Identifier: AGPL-3.0-or-later
//! Discord OAuth2 with PKCE (M2 spec section 5.3). Only the server talks to Discord; it holds the
//! client secret. Scope: `identify`.

use std::future::Future;

use data_encoding::BASE64URL_NOPAD;
use reqwest::Url;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Discord login page.
pub const DISCORD_AUTHORIZE_URL: &str = "https://discord.com/oauth2/authorize";
/// Discord token endpoint.
pub const DISCORD_TOKEN_URL: &str = "https://discord.com/api/v10/oauth2/token";
/// Discord current-user endpoint.
pub const DISCORD_ME_URL: &str = "https://discord.com/api/v10/users/@me";
/// The only scope the server asks for.
pub const DISCORD_SCOPE: &str = "identify";
/// Callback path on `login.<domain>`.
pub const DISCORD_CALLBACK_PATH: &str = "/launcher/discord/callback";

/// The Discord user behind a login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordUser {
    /// Discord user id (snowflake text).
    pub id: String,
    /// Global display name, or the username when none is set.
    pub display_name: String,
}

/// Discord refused the code or could not be reached.
#[derive(Debug, thiserror::Error)]
#[error("Discord request failed")]
pub struct DiscordError(#[source] Box<dyn std::error::Error + Send + Sync>);

/// Exchanges a login code for the Discord user.
pub trait DiscordApi: Clone + Send + Sync + 'static {
    /// The user of an authorization `code` from the callback.
    fn user_for_code(
        &self,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> impl Future<Output = Result<DiscordUser, DiscordError>> + Send;
}

/// PKCE S256 challenge of a verifier.
pub fn pkce_challenge(verifier: &str) -> String {
    BASE64URL_NOPAD.encode(&Sha256::digest(verifier.as_bytes()))
}

/// The Discord login page for one attempt.
pub fn authorize_url(
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    verifier: &str,
) -> Result<String, DiscordError> {
    let challenge = pkce_challenge(verifier);
    Url::parse_with_params(
        DISCORD_AUTHORIZE_URL,
        [
            ("response_type", "code"),
            ("client_id", client_id),
            ("scope", DISCORD_SCOPE),
            ("redirect_uri", redirect_uri),
            ("state", state),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
        ],
    )
    .map(String::from)
    .map_err(|error| DiscordError(Box::new(error)))
}

/// Discord's HTTP API.
#[derive(Debug, Clone)]
pub struct DiscordHttp {
    client: reqwest::Client,
    client_id: String,
    client_secret: String,
}

#[derive(Deserialize)]
struct TokenReply {
    access_token: String,
}

#[derive(Deserialize)]
struct MeReply {
    id: String,
    username: String,
    global_name: Option<String>,
}

impl DiscordHttp {
    /// A client for one Discord app. Needs the process crypto provider
    /// ([`crate::install_crypto_provider`]).
    pub fn new(client_id: String, client_secret: String) -> Result<Self, DiscordError> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|error| DiscordError(Box::new(error)))?;
        Ok(Self {
            client,
            client_id,
            client_secret,
        })
    }
}

fn failed(error: reqwest::Error) -> DiscordError {
    DiscordError(Box::new(error))
}

impl DiscordApi for DiscordHttp {
    async fn user_for_code(
        &self,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<DiscordUser, DiscordError> {
        let token: TokenReply = self
            .client
            .post(DISCORD_TOKEN_URL)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("code_verifier", verifier),
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
            ])
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(failed)?
            .json()
            .await
            .map_err(failed)?;
        let me: MeReply = self
            .client
            .get(DISCORD_ME_URL)
            .bearer_auth(token.access_token)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(failed)?
            .json()
            .await
            .map_err(failed)?;
        Ok(DiscordUser {
            id: me.id,
            display_name: me.global_name.unwrap_or(me.username),
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A Discord stand-in: the code `good` belongs to one fixed user.
    #[derive(Debug, Clone)]
    pub(crate) struct FakeDiscord;

    /// The code [`FakeDiscord`] accepts.
    pub(crate) const GOOD_CODE: &str = "good";
    /// The user of [`GOOD_CODE`].
    pub(crate) const FAKE_USER_ID: &str = "80351110224678912";

    impl DiscordApi for FakeDiscord {
        async fn user_for_code(
            &self,
            code: &str,
            _verifier: &str,
            _redirect_uri: &str,
        ) -> Result<DiscordUser, DiscordError> {
            if code == GOOD_CODE {
                Ok(DiscordUser {
                    id: FAKE_USER_ID.to_owned(),
                    display_name: "Nelly".to_owned(),
                })
            } else {
                Err(DiscordError(Box::new(std::io::Error::other("bad code"))))
            }
        }
    }

    #[test]
    fn pkce_challenge_matches_rfc_7636_example() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn authorize_url_has_every_parameter() {
        let url = authorize_url(
            "123",
            "https://login.example.org/launcher/discord/callback",
            "st",
            "ver",
        )
        .expect("url");
        assert!(
            url.starts_with(
                "https://discord.com/oauth2/authorize?response_type=code&client_id=123"
            )
        );
        assert!(url.contains("scope=identify"));
        assert!(url.contains(
            "redirect_uri=https%3A%2F%2Flogin.example.org%2Flauncher%2Fdiscord%2Fcallback"
        ));
        assert!(url.contains("state=st"));
        assert!(url.contains("code_challenge_method=S256"));
    }
}
