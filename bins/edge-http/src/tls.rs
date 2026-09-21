// SPDX-License-Identifier: AGPL-3.0-or-later
//! The HTTPS certificates. `login.<domain>` and `game.<domain>` get the `[tls]` certificate (from
//! an ACME tool). The `[aws_mirror]` hosts get this server's own self-signed certificate, which
//! our launcher trusts for the game process only (study note
//! `docs/re/2026-09-18-remote-config-layers.md`). The choice is made by the TLS server name.

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nw_login::config::{AwsMirrorConfig, EdgeConfig};
use openssl::asn1::Asn1Time;
use openssl::bn::{BigNum, MsbOption};
use openssl::error::ErrorStack;
use openssl::hash::MessageDigest;
use openssl::pkey::PKey;
use openssl::rsa::Rsa;
use openssl::x509::extension::{
    BasicConstraints, ExtendedKeyUsage, KeyUsage, SubjectAlternativeName,
};
use openssl::x509::{X509, X509NameBuilder};
use rustls::ServerConfig;
use rustls::crypto::CryptoProvider;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;

/// RSA key size of the mirror certificate.
const MIRROR_KEY_BITS: u32 = 2048;
/// Days the mirror certificate is valid. The launcher fetches it at every game start, so a new
/// one reaches players at once.
pub const MIRROR_VALID_DAYS: u32 = 825;
/// Random bits of the serial number, so a new certificate never repeats an old serial.
const SERIAL_BITS: i32 = 128;
const X509_VERSION_3: i32 = 2;
const COMMON_NAME: &str = "CN";
/// Subject common name of the mirror certificate.
const MIRROR_COMMON_NAME: &str = "Blighted AWS mirror";
/// Protocols offered in ALPN, as `axum-server` offers them.
const ALPN_PROTOCOLS: [&[u8]; 2] = [b"h2", b"http/1.1"];
/// File mode of the written private key on Unix: owner read and write only.
#[cfg(unix)]
const OWNER_ONLY_MODE: u32 = 0o600;

/// A certificate file or key could not be made, read or used.
#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    /// A file cannot be read or written.
    #[error("cannot access {}: {source}", .path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// The I/O error.
        #[source]
        source: std::io::Error,
    },
    /// A PEM file holds no usable certificate or key.
    #[error("{} holds no valid {what}", .path.display())]
    Pem {
        /// The file.
        path: PathBuf,
        /// `certificate` or `private key`.
        what: &'static str,
    },
    /// The key does not fit the certificate, or the TLS library refused them.
    #[error("TLS setup failed: {0}")]
    Rustls(#[from] rustls::Error),
    /// OpenSSL could not make the certificate.
    #[error("certificate generation failed: {0}")]
    OpenSsl(#[from] ErrorStack),
    /// No process crypto provider is installed.
    #[error("no TLS crypto provider is installed")]
    NoProvider,
}

/// Picks the mirror certificate for the `[aws_mirror]` hosts and the main one otherwise.
#[derive(Debug)]
struct ByServerName {
    mirror_hosts: HashSet<String>,
    mirror: Arc<CertifiedKey>,
    main: Arc<CertifiedKey>,
}

impl ByServerName {
    fn pick(&self, server_name: Option<&str>) -> Arc<CertifiedKey> {
        let mirror =
            server_name.is_some_and(|name| self.mirror_hosts.contains(&name.to_ascii_lowercase()));
        Arc::clone(if mirror { &self.mirror } else { &self.main })
    }
}

impl ResolvesServerCert for ByServerName {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(self.pick(client_hello.server_name()))
    }
}

fn read(path: &Path) -> Result<Vec<u8>, TlsError> {
    std::fs::read(path).map_err(|source| TlsError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn certified_key(
    certificate_path: &Path,
    key_path: &Path,
    provider: &CryptoProvider,
) -> Result<Arc<CertifiedKey>, TlsError> {
    let chain = CertificateDer::pem_slice_iter(&read(certificate_path)?)
        .collect::<Result<Vec<_>, _>>()
        .ok()
        .filter(|chain| !chain.is_empty())
        .ok_or_else(|| TlsError::Pem {
            path: certificate_path.to_path_buf(),
            what: "certificate",
        })?;
    let key = PrivateKeyDer::from_pem_slice(&read(key_path)?).map_err(|_| TlsError::Pem {
        path: key_path.to_path_buf(),
        what: "private key",
    })?;
    Ok(Arc::new(CertifiedKey::from_der(chain, key, provider)?))
}

/// The TLS settings: the `[tls]` certificate, and the mirror certificate for the
/// `[aws_mirror]` hosts. Needs the process crypto provider.
pub fn server_config(config: &EdgeConfig) -> Result<ServerConfig, TlsError> {
    let provider = CryptoProvider::get_default().ok_or(TlsError::NoProvider)?;
    let resolver = ByServerName {
        mirror_hosts: config.aws_mirror.hosts.iter().cloned().collect(),
        mirror: certified_key(
            &config.aws_mirror.certificate_pem_path,
            &config.aws_mirror.private_key_pem_path,
            provider,
        )?,
        main: certified_key(
            &config.tls.certificate_pem_path,
            &config.tls.private_key_pem_path,
            provider,
        )?,
    };
    let mut server = ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(resolver));
    server.alpn_protocols = ALPN_PROTOCOLS.iter().map(|name| name.to_vec()).collect();
    Ok(server)
}

/// The mirror certificate as PEM text, for the launcher info.
pub fn mirror_certificate_pem(mirror: &AwsMirrorConfig) -> Result<String, TlsError> {
    let path = &mirror.certificate_pem_path;
    String::from_utf8(read(path)?).map_err(|_| TlsError::Pem {
        path: path.clone(),
        what: "certificate",
    })
}

/// A self-signed RSA certificate for exactly `hosts`: not a CA, server use only, valid
/// [`MIRROR_VALID_DAYS`] days. Returns the certificate and PKCS#8 key, as PEM.
fn mirror_certificate(hosts: &[String]) -> Result<(Vec<u8>, Vec<u8>), ErrorStack> {
    let key = PKey::from_rsa(Rsa::generate(MIRROR_KEY_BITS)?)?;
    let mut name = X509NameBuilder::new()?;
    name.append_entry_by_text(COMMON_NAME, MIRROR_COMMON_NAME)?;
    let name = name.build();
    let mut builder = X509::builder()?;
    builder.set_version(X509_VERSION_3)?;
    let mut serial = BigNum::new()?;
    serial.rand(SERIAL_BITS, MsbOption::MAYBE_ZERO, false)?;
    let serial = serial.to_asn1_integer()?;
    builder.set_serial_number(&serial)?;
    builder.set_subject_name(&name)?;
    builder.set_issuer_name(&name)?;
    builder.set_pubkey(&key)?;
    let not_before = Asn1Time::days_from_now(0)?;
    let not_after = Asn1Time::days_from_now(MIRROR_VALID_DAYS)?;
    builder.set_not_before(&not_before)?;
    builder.set_not_after(&not_after)?;
    builder.append_extension(BasicConstraints::new().critical().build()?)?;
    builder.append_extension(
        KeyUsage::new()
            .critical()
            .digital_signature()
            .key_encipherment()
            .build()?,
    )?;
    builder.append_extension(ExtendedKeyUsage::new().server_auth().build()?)?;
    let mut names = SubjectAlternativeName::new();
    for host in hosts {
        names.dns(host);
    }
    let names = names.build(&builder.x509v3_context(None, None))?;
    builder.append_extension(names)?;
    builder.sign(&key, MessageDigest::sha256())?;
    Ok((builder.build().to_pem()?, key.private_key_to_pem_pkcs8()?))
}

/// Writes `bytes` to a new file; an existing file is an error, so no certificate is replaced.
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), TlsError> {
    let io_error = |source| TlsError::Io {
        path: path.to_path_buf(),
        source,
    };
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(OWNER_ONLY_MODE);
    }
    options
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(io_error)
}

/// Makes the mirror certificate for the `[aws_mirror]` hosts and writes it and its key to the
/// configured files. Existing files are never replaced: move them away to make a new one.
pub fn write_mirror_certificate(mirror: &AwsMirrorConfig) -> Result<(), TlsError> {
    let (certificate, key) = mirror_certificate(&mirror.hosts)?;
    write_new(&mirror.private_key_pem_path, &key)?;
    write_new(&mirror.certificate_pem_path, &certificate)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOSTS: [&str; 2] = [
        "ags-javelin-remote-config.s3.amazonaws.com",
        "sts.us-east-1.amazonaws.com",
    ];

    fn hosts() -> Vec<String> {
        HOSTS.iter().map(|host| (*host).to_owned()).collect()
    }

    fn mirror_in(dir: &Path) -> AwsMirrorConfig {
        AwsMirrorConfig {
            hosts: hosts(),
            certificate_pem_path: dir.join("mirror.pem"),
            private_key_pem_path: dir.join("mirror-key.pem"),
        }
    }

    #[test]
    fn the_certificate_names_the_hosts_and_is_no_ca() {
        let (certificate, _) = mirror_certificate(&hosts()).expect("certificate");
        let certificate = X509::from_pem(&certificate).expect("parse");
        let names: Vec<String> = certificate
            .subject_alt_names()
            .expect("names")
            .iter()
            .filter_map(|name| name.dnsname().map(str::to_owned))
            .collect();
        assert_eq!(names, hosts());
        let text = String::from_utf8(certificate.to_text().expect("text")).expect("utf-8");
        assert!(text.contains("CA:FALSE"), "{text}");
        assert!(text.contains("TLS Web Server Authentication"), "{text}");
        assert!(
            certificate
                .verify(&certificate.public_key().expect("key"))
                .expect("verify")
        );
    }

    #[test]
    fn writing_never_replaces_a_certificate() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mirror = mirror_in(dir.path());
        write_mirror_certificate(&mirror).expect("first");
        let first = std::fs::read(&mirror.certificate_pem_path).expect("read");
        assert!(matches!(
            write_mirror_certificate(&mirror),
            Err(TlsError::Io { .. })
        ));
        assert_eq!(
            std::fs::read(&mirror.certificate_pem_path).expect("read"),
            first
        );
        assert!(
            mirror_certificate_pem(&mirror)
                .expect("pem")
                .starts_with("-----BEGIN CERTIFICATE-----")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&mirror.private_key_pem_path)
                .expect("key")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, OWNER_ONLY_MODE);
        }
    }

    #[test]
    fn the_server_name_picks_the_certificate() {
        nw_login::install_crypto_provider();
        let provider = CryptoProvider::get_default().expect("provider");
        let mirror_dir = tempfile::tempdir().expect("temp dir");
        let main_dir = tempfile::tempdir().expect("temp dir");
        let mirror = mirror_in(mirror_dir.path());
        let main = AwsMirrorConfig {
            hosts: vec!["login.example.org".to_owned()],
            ..mirror_in(main_dir.path())
        };
        write_mirror_certificate(&mirror).expect("mirror");
        write_mirror_certificate(&main).expect("main");
        let load = |config: &AwsMirrorConfig| {
            certified_key(
                &config.certificate_pem_path,
                &config.private_key_pem_path,
                provider,
            )
            .expect("load")
        };
        let resolver = ByServerName {
            mirror_hosts: mirror.hosts.iter().cloned().collect(),
            mirror: load(&mirror),
            main: load(&main),
        };
        let mirror_cert = &load(&mirror).cert[0];
        let main_cert = &load(&main).cert[0];
        assert_eq!(&resolver.pick(Some(HOSTS[0])).cert[0], mirror_cert);
        assert_eq!(
            &resolver.pick(Some("STS.us-east-1.amazonaws.com")).cert[0],
            mirror_cert
        );
        assert_eq!(&resolver.pick(Some("login.example.org")).cert[0], main_cert);
        assert_eq!(&resolver.pick(None).cert[0], main_cert);
    }

    /// Serves `ok` over HTTPS with [`server_config`] for a config whose `[tls]` and
    /// `[aws_mirror]` files are in `dir`, and returns the address.
    async fn serve_ok(dir: &Path) -> std::net::SocketAddr {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/edge.toml");
        let mut config = nw_login::config::load_edge_config(&path).expect("config");
        config.aws_mirror = mirror_in(dir);
        let main_dir = dir.join("main");
        std::fs::create_dir(&main_dir).expect("folder");
        let main = AwsMirrorConfig {
            hosts: vec![config.login_host()],
            ..mirror_in(&main_dir)
        };
        write_mirror_certificate(&config.aws_mirror).expect("mirror");
        write_mirror_certificate(&main).expect("main");
        config.tls.certificate_pem_path = main.certificate_pem_path;
        config.tls.private_key_pem_path = main.private_key_pem_path;
        let tls = axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(
            server_config(&config).expect("server config"),
        ));
        let app = axum::Router::new().route("/", axum::routing::get(|| async { "ok" }));
        let handle = axum_server::Handle::new();
        let listening = handle.clone();
        tokio::spawn(
            axum_server::bind_rustls(LOOPBACK_ANY_PORT.parse().expect("address"), tls)
                .handle(handle)
                .serve(app.into_make_service()),
        );
        listening.listening().await.expect("listening")
    }

    const LOOPBACK_ANY_PORT: &str = "127.0.0.1:0";

    async fn fetch(trusted: &Path, host: &str, address: std::net::SocketAddr) -> bool {
        let pem = std::fs::read(trusted).expect("certificate");
        let client = reqwest::Client::builder()
            .tls_certs_only([reqwest::Certificate::from_pem(&pem).expect("pem")])
            .resolve(host, address)
            .build()
            .expect("client");
        let url = format!("https://{host}:{}/", address.port());
        match client.get(url).send().await {
            Ok(response) => response.text().await.expect("text") == "ok",
            Err(_) => false,
        }
    }

    #[tokio::test]
    async fn a_client_that_trusts_only_the_mirror_certificate_reaches_the_mirror_hosts() {
        nw_login::install_crypto_provider();
        let dir = tempfile::tempdir().expect("temp dir");
        let address = serve_ok(dir.path()).await;
        let mirror = mirror_in(dir.path()).certificate_pem_path;
        let main = mirror_in(&dir.path().join("main")).certificate_pem_path;
        for host in HOSTS {
            assert!(fetch(&mirror, host, address).await, "{host}");
            assert!(!fetch(&main, host, address).await, "{host}");
        }
        assert!(fetch(&main, "login.example.org", address).await);
        assert!(!fetch(&mirror, "login.example.org", address).await);
    }

    #[test]
    fn a_wrong_key_or_file_is_refused() {
        nw_login::install_crypto_provider();
        let provider = CryptoProvider::get_default().expect("provider");
        let dir = tempfile::tempdir().expect("temp dir");
        let mirror = mirror_in(dir.path());
        assert!(matches!(
            certified_key(
                &mirror.certificate_pem_path,
                &mirror.private_key_pem_path,
                provider
            ),
            Err(TlsError::Io { .. })
        ));
        write_mirror_certificate(&mirror).expect("write");
        assert!(matches!(
            certified_key(
                &mirror.private_key_pem_path,
                &mirror.private_key_pem_path,
                provider
            ),
            Err(TlsError::Pem { .. })
        ));
        let other_dir = tempfile::tempdir().expect("temp dir");
        let other = mirror_in(other_dir.path());
        write_mirror_certificate(&other).expect("write");
        assert!(matches!(
            certified_key(
                &mirror.certificate_pem_path,
                &other.private_key_pem_path,
                provider
            ),
            Err(TlsError::Rustls(_))
        ));
    }
}
