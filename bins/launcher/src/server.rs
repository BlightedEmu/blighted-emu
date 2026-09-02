// SPDX-License-Identifier: GPL-3.0-or-later
//! The server address a player types, turned into the launcher API base URL.

use nw_protocol::launcher_api::is_forbidden_host;

/// Host label of the launcher API: `login.<domain>`.
pub const LOGIN_HOST_LABEL: &str = "login";
const HTTPS_PREFIX: &str = "https://";

/// A server address that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServerAddressError {
    /// Empty, or not a plain domain name.
    #[error("enter the server domain, for example example.org")]
    Invalid,
    /// An Amazon or CloudFront domain (master spec 4.1).
    #[error("Amazon and CloudFront addresses are refused")]
    Forbidden,
}

/// The launcher API base URL for a typed address: `example.org`, `login.example.org` and
/// `https://login.example.org` all give `https://login.example.org`.
pub fn api_base_url(input: &str) -> Result<String, ServerAddressError> {
    let host = input
        .trim()
        .trim_start_matches(HTTPS_PREFIX)
        .trim_end_matches('/')
        .to_ascii_lowercase();
    let valid = !host.is_empty()
        && host.contains('.')
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-');
    if !valid {
        return Err(ServerAddressError::Invalid);
    }
    if is_forbidden_host(&host) {
        return Err(ServerAddressError::Forbidden);
    }
    let login_prefix = format!("{LOGIN_HOST_LABEL}.");
    if host.starts_with(&login_prefix) {
        Ok(format!("{HTTPS_PREFIX}{host}"))
    } else {
        Ok(format!("{HTTPS_PREFIX}{login_prefix}{host}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_forms_give_the_login_host() {
        for input in [
            "example.org",
            "login.example.org",
            "https://login.example.org/",
            " Example.ORG ",
        ] {
            assert_eq!(
                api_base_url(input).as_deref(),
                Ok("https://login.example.org"),
                "{input}"
            );
        }
    }

    #[test]
    fn bad_and_forbidden_addresses_are_refused() {
        assert_eq!(api_base_url(""), Err(ServerAddressError::Invalid));
        assert_eq!(api_base_url("localhost"), Err(ServerAddressError::Invalid));
        assert_eq!(
            api_base_url("http://example.org"),
            Err(ServerAddressError::Invalid)
        );
        assert_eq!(
            api_base_url("example.org/path"),
            Err(ServerAddressError::Invalid)
        );
        assert_eq!(
            api_base_url("d2c74t4zimux3r.cloudfront.net"),
            Err(ServerAddressError::Forbidden)
        );
        assert_eq!(
            api_base_url("login.amazongames.com"),
            Err(ServerAddressError::Forbidden)
        );
    }
}
