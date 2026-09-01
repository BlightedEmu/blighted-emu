// SPDX-License-Identifier: AGPL-3.0-or-later
//! Server file for `edge-http` and `rep-gateway`.
//!
//! One file holds both listen addresses and the `RepAddress` string `edge-http` advertises.
//! Those values must name the same gateway. The cipher and the certificate subject are recorded
//! so an operator can read them. Load fails when they do not match the embedded client certificate.
//! This file does not contain a PEM.

use std::net::SocketAddr;
use std::path::Path;

use nw_common::config::{ConfigError, load_config};
use serde::Deserialize;

/// Environment prefix for overrides of the edge file.
/// Nested keys use `__`, for example `NW_EDGE_TRUST__CIPHER`.
pub const EDGE_ENV_PREFIX: &str = "NW_EDGE_";

/// Repo-relative path of the example edge file.
pub const EXAMPLE_EDGE_CONFIG_PATH: &str = "config/edge.toml";

/// HTTPS port on the architecture diagram for `edge-http` (master spec section 3).
pub const EDGE_HTTPS_PORT: u16 = 443;

/// UDP port in the client base config key `rep-default-port`.
/// Source: the endpoint note, `@assets@/Client.json`.
pub const REP_DEFAULT_PORT: u16 = 33435;

/// Cipher list the game-server path sets for `gridmate-udp` and `gridmate-udp-bsd`.
pub const GAME_SERVER_CIPHER: &str = "ECDHE-RSA-AES256-GCM-SHA384";

/// `C` of the embedded certificate subject.
pub const CERT_COUNTRY: &str = "US";

/// `ST` of the embedded certificate subject.
pub const CERT_STATE: &str = "California";

/// `L` of the embedded certificate subject.
pub const CERT_LOCALITY: &str = "SNA11";

/// `O` of the embedded certificate subject.
pub const CERT_ORGANIZATION: &str = "Amazon";

/// `OU` of the embedded certificate subject.
pub const CERT_ORGANIZATIONAL_UNIT: &str = "Amazon Game Studios";

/// `CN` of the embedded certificate subject.
pub const CERT_COMMON_NAME: &str = "New World";

/// `notBefore` of the embedded certificate.
pub const CERT_NOT_BEFORE_GMT: &str = "2025-08-04 23:15:36 GMT";

/// `notAfter` of the embedded certificate.
pub const CERT_NOT_AFTER_GMT: &str = "2027-01-06 23:15:36 GMT";

/// RSA key size of the embedded certificate. Source: the endpoint note.
pub const CERT_RSA_BITS: u16 = 2048;

const RDN_COUNTRY: &str = "C";
const RDN_STATE: &str = "ST";
const RDN_LOCALITY: &str = "L";
const RDN_ORGANIZATION: &str = "O";
const RDN_ORGANIZATIONAL_UNIT: &str = "OU";
const RDN_COMMON_NAME: &str = "CN";

const TRUST_KEY_CIPHER: &str = "cipher";
const TRUST_KEY_COUNTRY: &str = "country";
const TRUST_KEY_STATE: &str = "state";
const TRUST_KEY_LOCALITY: &str = "locality";
const TRUST_KEY_ORGANIZATION: &str = "organization";
const TRUST_KEY_ORGANIZATIONAL_UNIT: &str = "organizational_unit";
const TRUST_KEY_COMMON_NAME: &str = "common_name";

/// Subject of the embedded game-server certificate.
/// The certificate is self-signed, so the issuer is this same subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CertificateSubject {
    /// `C`.
    pub country: &'static str,
    /// `ST`.
    pub state: &'static str,
    /// `L`.
    pub locality: &'static str,
    /// `O`.
    pub organization: &'static str,
    /// `OU`.
    pub organizational_unit: &'static str,
    /// `CN`.
    pub common_name: &'static str,
}

impl CertificateSubject {
    /// The subject recorded from `NewWorld.exe`.
    pub const fn embedded() -> Self {
        Self {
            country: CERT_COUNTRY,
            state: CERT_STATE,
            locality: CERT_LOCALITY,
            organization: CERT_ORGANIZATION,
            organizational_unit: CERT_ORGANIZATIONAL_UNIT,
            common_name: CERT_COMMON_NAME,
        }
    }

    /// Subject text in the order recorded from the certificate.
    pub fn distinguished_name(self) -> String {
        format!(
            "{RDN_COUNTRY}={}, {RDN_STATE}={}, {RDN_LOCALITY}={}, {RDN_ORGANIZATION}={}, {RDN_ORGANIZATIONAL_UNIT}={}, {RDN_COMMON_NAME}={}",
            self.country,
            self.state,
            self.locality,
            self.organization,
            self.organizational_unit,
            self.common_name,
        )
    }
}

/// Listen addresses, the advertised `RepAddress`, and the recorded game-server trust facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeConfig {
    /// Socket `edge-http` binds for HTTPS.
    pub https_listen: SocketAddr,
    /// Socket `rep-gateway` binds for UDP.
    pub udp_listen: SocketAddr,
    /// String `edge-http` places in login-queue `Token.RepAddress`.
    /// The client prints this string as one value. This struct does not split it.
    pub rep_address: String,
    /// Cipher the client sets on the game-server path.
    pub game_server_cipher: &'static str,
    /// Subject the client trusts.
    pub certificate_subject: CertificateSubject,
    /// `notBefore` of that certificate.
    pub certificate_not_before_gmt: &'static str,
    /// `notAfter` of that certificate.
    pub certificate_not_after_gmt: &'static str,
    /// RSA modulus size in bits.
    pub certificate_rsa_bits: u16,
}

/// Failure while loading the edge file.
#[derive(Debug, thiserror::Error)]
pub enum EdgeConfigError {
    /// The file is missing or its values do not match the struct.
    #[error(transparent)]
    Load(#[from] ConfigError),
    /// `rep_address` is empty or only whitespace.
    #[error("rep_address is empty")]
    EmptyRepAddress,
    /// `trust.cipher` is not the client cipher.
    #[error("trust.{TRUST_KEY_CIPHER} must be {GAME_SERVER_CIPHER}, found {found}")]
    CipherMismatch {
        /// Cipher text from the file or the environment.
        found: String,
    },
    /// One subject field is not the embedded certificate subject.
    #[error("trust.{field} must be {expected}, found {found}")]
    SubjectMismatch {
        /// TOML key under `[trust]`.
        field: &'static str,
        /// Value from the file or the environment.
        found: String,
        /// Value the client certificate uses.
        expected: &'static str,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EdgeConfigFile {
    https_listen: SocketAddr,
    udp_listen: SocketAddr,
    rep_address: String,
    trust: TrustFile,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustFile {
    cipher: String,
    country: String,
    state: String,
    locality: String,
    organization: String,
    organizational_unit: String,
    common_name: String,
}

/// Loads the edge file, then checks the recorded trust values.
pub fn load_edge_config(file: &Path) -> Result<EdgeConfig, EdgeConfigError> {
    let parsed: EdgeConfigFile = load_config(file, EDGE_ENV_PREFIX)?;
    if parsed.rep_address.trim().is_empty() {
        return Err(EdgeConfigError::EmptyRepAddress);
    }
    validate_trust(&parsed.trust)?;
    Ok(EdgeConfig {
        https_listen: parsed.https_listen,
        udp_listen: parsed.udp_listen,
        rep_address: parsed.rep_address,
        game_server_cipher: GAME_SERVER_CIPHER,
        certificate_subject: CertificateSubject::embedded(),
        certificate_not_before_gmt: CERT_NOT_BEFORE_GMT,
        certificate_not_after_gmt: CERT_NOT_AFTER_GMT,
        certificate_rsa_bits: CERT_RSA_BITS,
    })
}

fn validate_trust(trust: &TrustFile) -> Result<(), EdgeConfigError> {
    if trust.cipher != GAME_SERVER_CIPHER {
        return Err(EdgeConfigError::CipherMismatch {
            found: trust.cipher.clone(),
        });
    }
    let subject = CertificateSubject::embedded();
    check_subject(TRUST_KEY_COUNTRY, &trust.country, subject.country)?;
    check_subject(TRUST_KEY_STATE, &trust.state, subject.state)?;
    check_subject(TRUST_KEY_LOCALITY, &trust.locality, subject.locality)?;
    check_subject(
        TRUST_KEY_ORGANIZATION,
        &trust.organization,
        subject.organization,
    )?;
    check_subject(
        TRUST_KEY_ORGANIZATIONAL_UNIT,
        &trust.organizational_unit,
        subject.organizational_unit,
    )?;
    check_subject(
        TRUST_KEY_COMMON_NAME,
        &trust.common_name,
        subject.common_name,
    )?;
    Ok(())
}

fn check_subject(
    field: &'static str,
    found: &str,
    expected: &'static str,
) -> Result<(), EdgeConfigError> {
    if found == expected {
        Ok(())
    } else {
        Err(EdgeConfigError::SubjectMismatch {
            field,
            found: found.to_owned(),
            expected,
        })
    }
}

#[cfg(test)]
#[expect(
    clippy::result_large_err,
    reason = "figment::Jail requires its closures to return figment::Result"
)]
mod tests {
    use super::*;
    use figment::Jail;
    use std::fs;
    use std::net::{IpAddr, Ipv4Addr};

    const CONFIG_FILE: &str = "edge.toml";
    const OVERRIDE_HTTPS_PORT: u16 = 8443;
    const OBSERVED_SUBJECT: &str =
        "C=US, ST=California, L=SNA11, O=Amazon, OU=Amazon Game Studios, CN=New World";

    fn unspecified(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port)
    }

    fn example_rep_address() -> String {
        format!("{}:{REP_DEFAULT_PORT}", Ipv4Addr::LOCALHOST)
    }

    fn valid_toml() -> String {
        format!(
            "\
https_listen = \"{unspecified_https}\"
udp_listen = \"{unspecified_udp}\"
rep_address = \"{rep_address}\"

[trust]
{TRUST_KEY_CIPHER} = \"{GAME_SERVER_CIPHER}\"
{TRUST_KEY_COUNTRY} = \"{CERT_COUNTRY}\"
{TRUST_KEY_STATE} = \"{CERT_STATE}\"
{TRUST_KEY_LOCALITY} = \"{CERT_LOCALITY}\"
{TRUST_KEY_ORGANIZATION} = \"{CERT_ORGANIZATION}\"
{TRUST_KEY_ORGANIZATIONAL_UNIT} = \"{CERT_ORGANIZATIONAL_UNIT}\"
{TRUST_KEY_COMMON_NAME} = \"{CERT_COMMON_NAME}\"
",
            unspecified_https = unspecified(EDGE_HTTPS_PORT),
            unspecified_udp = unspecified(REP_DEFAULT_PORT),
            rep_address = example_rep_address(),
        )
    }

    fn load_text(text: &str) -> Result<EdgeConfig, EdgeConfigError> {
        let mut loaded = None;
        let mut error = None;
        Jail::expect_with(|jail| {
            jail.create_file(CONFIG_FILE, text)?;
            match load_edge_config(Path::new(CONFIG_FILE)) {
                Ok(config) => loaded = Some(config),
                Err(load_error) => error = Some(load_error),
            }
            Ok(())
        });
        match (loaded, error) {
            (Some(config), None) => Ok(config),
            (None, Some(load_error)) => Err(load_error),
            _ => panic!("load produced neither a config nor an error"),
        }
    }

    #[test]
    fn trust_constants_match_the_embedded_certificate() {
        assert_eq!(EDGE_HTTPS_PORT, 443);
        assert_eq!(REP_DEFAULT_PORT, 33435);
        assert_eq!(GAME_SERVER_CIPHER, "ECDHE-RSA-AES256-GCM-SHA384");
        assert_eq!(CERT_NOT_BEFORE_GMT, "2025-08-04 23:15:36 GMT");
        assert_eq!(CERT_NOT_AFTER_GMT, "2027-01-06 23:15:36 GMT");
        assert_eq!(CERT_RSA_BITS, 2048);
        assert_eq!(
            CertificateSubject::embedded().distinguished_name(),
            OBSERVED_SUBJECT
        );
    }

    #[test]
    fn loads_listen_addresses_rep_address_and_recorded_trust() {
        let config = load_text(&valid_toml()).expect("valid edge file");
        assert_eq!(config.https_listen, unspecified(EDGE_HTTPS_PORT));
        assert_eq!(config.udp_listen, unspecified(REP_DEFAULT_PORT));
        assert_eq!(config.rep_address, example_rep_address());
        assert_eq!(config.game_server_cipher, GAME_SERVER_CIPHER);
        assert_eq!(config.certificate_subject, CertificateSubject::embedded());
        assert_eq!(
            config.certificate_subject.distinguished_name(),
            OBSERVED_SUBJECT
        );
        assert_eq!(config.certificate_not_before_gmt, CERT_NOT_BEFORE_GMT);
        assert_eq!(config.certificate_not_after_gmt, CERT_NOT_AFTER_GMT);
        assert_eq!(config.certificate_rsa_bits, CERT_RSA_BITS);
    }

    #[test]
    fn example_file_records_trust_and_has_no_pem() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(EXAMPLE_EDGE_CONFIG_PATH);
        let text = fs::read_to_string(&path).expect("example edge file");
        assert!(
            !text.contains("BEGIN CERTIFICATE"),
            "the example file must not contain a PEM"
        );
        assert!(
            !text.contains("PRIVATE KEY"),
            "the example file must not contain a private key"
        );
        let config = load_text(&text).expect("example edge file parses");
        assert_eq!(config.https_listen, unspecified(EDGE_HTTPS_PORT));
        assert_eq!(config.udp_listen, unspecified(REP_DEFAULT_PORT));
        assert_eq!(config.rep_address, example_rep_address());
        assert_eq!(config.game_server_cipher, GAME_SERVER_CIPHER);
        assert_eq!(
            config.certificate_subject.distinguished_name(),
            OBSERVED_SUBJECT
        );
    }

    #[test]
    fn environment_overrides_the_advertised_rep_address() {
        let override_address = format!("203.0.113.10:{REP_DEFAULT_PORT}");
        Jail::expect_with(|jail| {
            jail.create_file(CONFIG_FILE, &valid_toml())?;
            jail.set_env("NW_EDGE_REP_ADDRESS", format!("\"{override_address}\""));
            let config = load_edge_config(Path::new(CONFIG_FILE)).expect("env override");
            assert_eq!(config.rep_address, override_address);
            assert_eq!(config.https_listen.port(), EDGE_HTTPS_PORT);
            Ok(())
        });
    }

    #[test]
    fn environment_can_move_the_https_listen_port() {
        Jail::expect_with(|jail| {
            jail.create_file(CONFIG_FILE, &valid_toml())?;
            jail.set_env(
                "NW_EDGE_HTTPS_LISTEN",
                format!("\"{}:{OVERRIDE_HTTPS_PORT}\"", Ipv4Addr::LOCALHOST),
            );
            let config = load_edge_config(Path::new(CONFIG_FILE)).expect("listen override");
            assert_eq!(
                config.https_listen,
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), OVERRIDE_HTTPS_PORT)
            );
            Ok(())
        });
    }

    #[test]
    fn empty_rep_address_is_an_error() {
        let text = valid_toml().replace(
            &format!("rep_address = \"{}\"", example_rep_address()),
            "rep_address = \"   \"",
        );
        let error = load_text(&text).expect_err("blank RepAddress");
        assert!(matches!(error, EdgeConfigError::EmptyRepAddress));
    }

    #[test]
    fn wrong_cipher_is_an_error() {
        let text = valid_toml().replace(GAME_SERVER_CIPHER, "AES128-SHA");
        let error = load_text(&text).expect_err("cipher");
        assert!(
            matches!(error, EdgeConfigError::CipherMismatch { found } if found == "AES128-SHA")
        );
    }

    #[test]
    fn wrong_certificate_subject_is_an_error() {
        let text = valid_toml().replace(CERT_COMMON_NAME, "Other Name");
        let error = load_text(&text).expect_err("subject");
        assert!(matches!(
            error,
            EdgeConfigError::SubjectMismatch { field, found, expected }
                if field == TRUST_KEY_COMMON_NAME
                    && found == "Other Name"
                    && expected == CERT_COMMON_NAME
        ));
    }

    #[test]
    fn unknown_key_is_an_error() {
        let text = format!("extra = true\n{}", valid_toml());
        let error = load_text(&text).expect_err("unknown key");
        assert!(matches!(
            error,
            EdgeConfigError::Load(ConfigError::Invalid(_))
        ));
    }

    #[test]
    fn bad_listen_address_is_an_error() {
        let text =
            valid_toml().replace(&unspecified(EDGE_HTTPS_PORT).to_string(), "not-an-address");
        let error = load_text(&text).expect_err("listen address");
        assert!(matches!(
            error,
            EdgeConfigError::Load(ConfigError::Invalid(_))
        ));
    }

    #[test]
    fn missing_file_is_an_error() {
        Jail::expect_with(|_jail| {
            let error = load_edge_config(Path::new(CONFIG_FILE)).expect_err("missing");
            assert!(
                matches!(error, EdgeConfigError::Load(ConfigError::FileNotFound(path)) if path == Path::new(CONFIG_FILE))
            );
            Ok(())
        });
    }
}
