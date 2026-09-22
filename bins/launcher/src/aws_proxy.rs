// SPDX-License-Identifier: GPL-3.0-or-later
//! The game process's private network view on Windows, where there is no `bwrap` sandbox
//! (study note `docs/re/2026-09-20-windows-launcher-network.md`, option A). On Linux the launcher
//! uses [`crate::mirror`] instead; this module is built only on Windows (and for tests).
//!
//! `scripts/patch-client.py --aws-proxy` points the game's three AWS clients (remote-config S3,
//! Kinesis, STS) at `127.0.0.1:47443` with `verifySSL` off. Each client then sends an HTTP
//! `CONNECT host:443` to this proxy and runs TLS inside the tunnel. The proxy:
//!
//! 1. accepts the `CONNECT` only when the host passes [`is_aws_mirror_host`], else answers `403`;
//! 2. ends the game's TLS with the launcher's own certificate (the game does not check it, because
//!    the patch cleared `verifySSL`);
//! 3. opens its own TLS to the server and pins the server's mirror certificate, the same check as
//!    [`crate::mirror::check_mirror`] (a [`RootCertStore`] holding only that certificate);
//! 4. relays the plain HTTP bytes between the two TLS legs.
//!
//! So the game's weakened certificate check reaches only loopback, and the launcher keeps the real
//! check toward the server.

use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use nw_protocol::aws_mirror::{AwsMirror, is_aws_mirror_host};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::{Builder, Runtime};
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::mirror::{MirrorError, server_addresses};

/// The loopback port the proxy listens on. `scripts/patch-client.py` (`AWS_PROXY_PORT`) writes the
/// same port into the patched client, so the two must stay equal.
pub const AWS_PROXY_PORT: u16 = 47443;
/// The HTTPS port of the server, where the mirror answers.
const MIRROR_HTTPS_PORT: u16 = 443;
/// Loopback worker threads: one accept loop plus connection relays.
const PROXY_WORKER_THREADS: usize = 2;

/// The only request method the proxy serves.
const CONNECT_METHOD: &str = "CONNECT";
/// Host and port are split on this.
const PORT_SEPARATOR: char = ':';
/// End of the request head.
const HEAD_TERMINATOR: &[u8] = b"\r\n\r\n";
/// Most bytes read while looking for [`HEAD_TERMINATOR`]; a longer head is refused.
const MAX_HEAD_BYTES: usize = 8192;
/// Answer to an accepted `CONNECT`: the tunnel is open.
const CONNECT_OK: &[u8] = b"HTTP/1.1 200 Connection Established\r\n\r\n";
/// Answer to a `CONNECT` for a host that is not an AWS mirror host.
const FORBIDDEN: &[u8] = b"HTTP/1.1 403 Forbidden\r\n\r\n";
/// Answer to anything that is not a well-formed `CONNECT`.
const BAD_REQUEST: &[u8] = b"HTTP/1.1 400 Bad Request\r\n\r\n";

/// The launcher's own certificate and key for the game-facing TLS leg. The patched client does not
/// check this certificate (its `verifySSL` is off), so it is a fixed self-signed throwaway.
const PROXY_TLS_CERT: &str = include_str!("../testdata/proxy-tls.crt");
const PROXY_TLS_KEY: &str = include_str!("../testdata/proxy-tls-key.pem");

/// The proxy could not be started.
#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    /// The server's mirror could not be used (bad certificate, no address).
    #[error(transparent)]
    Mirror(#[from] MirrorError),
    /// The launcher's own certificate or key, or the server's mirror certificate, is unusable.
    #[error("cannot build the proxy TLS configuration: {0}")]
    Tls(String),
    /// The loopback port could not be opened.
    #[error("cannot open the proxy on 127.0.0.1:{AWS_PROXY_PORT}: {0}")]
    Bind(#[source] io::Error),
    /// The async runtime could not be built.
    #[error("cannot start the proxy runtime: {0}")]
    Runtime(#[source] io::Error),
}

/// The TLS set-up and server address shared by every tunnel.
struct Shared {
    /// Ends the game's TLS with the launcher's own certificate.
    acceptor: TlsAcceptor,
    /// Opens TLS to the server with the mirror certificate pinned.
    connector: TlsConnector,
    /// The server's address, where the mirror answers.
    server: SocketAddr,
}

/// A running loopback proxy. Dropping it stops the proxy and closes the port.
pub struct AwsProxy {
    /// The runtime that drives the accept loop and the tunnels. [`Drop`] shuts it down in the
    /// background, so dropping the proxy never blocks the caller.
    runtime: Option<Runtime>,
    /// The address the proxy listens on (always loopback).
    local_addr: SocketAddr,
}

impl Drop for AwsProxy {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

impl AwsProxy {
    /// Checks `mirror`, looks up `server_host`, binds [`AWS_PROXY_PORT`] on loopback, and serves
    /// tunnels to the server in the background.
    pub fn start(mirror: &AwsMirror, server_host: &str) -> Result<Self, ProxyError> {
        let server = first_server_address(server_host)?;
        Self::serve(mirror, server, (Ipv4Addr::LOCALHOST, AWS_PROXY_PORT).into())
    }

    /// The port the proxy listens on.
    pub fn port(&self) -> u16 {
        self.local_addr.port()
    }

    /// Binds `listen` and serves tunnels to `server` using `mirror` for pinning. `start` calls this
    /// with the loopback [`AWS_PROXY_PORT`]; tests call it with an ephemeral port.
    fn serve(mirror: &AwsMirror, server: SocketAddr, listen: SocketAddr) -> Result<Self, ProxyError> {
        crate::install_crypto_provider();
        let shared = Arc::new(Shared {
            acceptor: TlsAcceptor::from(Arc::new(game_server_config()?)),
            connector: TlsConnector::from(Arc::new(server_client_config(mirror)?)),
            server,
        });
        let runtime = Builder::new_multi_thread()
            .worker_threads(PROXY_WORKER_THREADS)
            .enable_io()
            .build()
            .map_err(ProxyError::Runtime)?;
        let listener = runtime
            .block_on(async { TcpListener::bind(listen).await })
            .map_err(ProxyError::Bind)?;
        let local_addr = listener.local_addr().map_err(ProxyError::Bind)?;
        runtime.spawn(accept_loop(listener, shared));
        Ok(Self {
            runtime: Some(runtime),
            local_addr,
        })
    }
}

/// The server's first address at [`MIRROR_HTTPS_PORT`].
fn first_server_address(server_host: &str) -> Result<SocketAddr, ProxyError> {
    let address = *server_addresses(server_host)?
        .first()
        .ok_or_else(|| MirrorError::Lookup {
            host: server_host.to_owned(),
            source: io::Error::new(io::ErrorKind::NotFound, "no address"),
        })?;
    Ok(SocketAddr::new(address, MIRROR_HTTPS_PORT))
}

/// The game-facing TLS server configuration from the launcher's own certificate and key.
fn game_server_config() -> Result<ServerConfig, ProxyError> {
    let certificates = CertificateDer::pem_slice_iter(PROXY_TLS_CERT.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| ProxyError::Tls(error.to_string()))?;
    let key =
        PrivateKeyDer::from_pem_slice(PROXY_TLS_KEY.as_bytes()).map_err(|error| ProxyError::Tls(error.to_string()))?;
    ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certificates, key)
        .map_err(|error| ProxyError::Tls(error.to_string()))
}

/// The server-facing TLS client configuration that pins the mirror certificate: it is the only
/// root, so the server must show exactly it (the same rule as [`crate::mirror::check_mirror`]).
fn server_client_config(mirror: &AwsMirror) -> Result<ClientConfig, ProxyError> {
    let mut roots = RootCertStore::empty();
    for certificate in CertificateDer::pem_slice_iter(mirror.certificate_pem.as_bytes()) {
        roots
            .add(certificate.map_err(|error| ProxyError::Tls(error.to_string()))?)
            .map_err(|error| ProxyError::Tls(error.to_string()))?;
    }
    Ok(ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth())
}

/// Accepts loopback connections and serves each in its own task.
async fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    while let Ok((stream, _)) = listener.accept().await {
        let shared = Arc::clone(&shared);
        tokio::spawn(async move {
            // A single failed tunnel must not stop the proxy; the game retries.
            let _ = serve_tunnel(stream, shared).await;
        });
    }
}

/// Reads the `CONNECT`, answers it, and relays the tunnel to the server.
async fn serve_tunnel(mut stream: TcpStream, shared: Arc<Shared>) -> io::Result<()> {
    let head = read_head(&mut stream).await?;
    let host = match requested_host(&head) {
        Ok(host) => host,
        Err(answer) => {
            stream.write_all(answer).await?;
            return Ok(());
        }
    };
    stream.write_all(CONNECT_OK).await?;

    let mut game = shared.acceptor.accept(stream).await?;
    let upstream = TcpStream::connect(shared.server).await?;
    let name = ServerName::try_from(host)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let mut server = shared.connector.connect(name, upstream).await?;
    copy_bidirectional(&mut game, &mut server).await?;
    Ok(())
}

/// Reads bytes until [`HEAD_TERMINATOR`], up to [`MAX_HEAD_BYTES`].
async fn read_head(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while !head.ends_with(HEAD_TERMINATOR) {
        if head.len() >= MAX_HEAD_BYTES || stream.read(&mut byte).await? == 0 {
            break;
        }
        head.push(byte[0]);
    }
    Ok(head)
}

/// The lower-case host of a well-formed `CONNECT` request for an AWS mirror host, or the answer to
/// send back: [`FORBIDDEN`] for another host, [`BAD_REQUEST`] for anything else.
fn requested_host(head: &[u8]) -> Result<String, &'static [u8]> {
    let text = std::str::from_utf8(head).map_err(|_| BAD_REQUEST)?;
    let request_line = text.lines().next().ok_or(BAD_REQUEST)?;
    let mut parts = request_line.split_whitespace();
    let (method, authority) = (parts.next().ok_or(BAD_REQUEST)?, parts.next().ok_or(BAD_REQUEST)?);
    if method != CONNECT_METHOD {
        return Err(BAD_REQUEST);
    }
    let host = authority
        .rsplit_once(PORT_SEPARATOR)
        .map_or(authority, |(host, _)| host)
        .to_ascii_lowercase();
    if host.is_empty() {
        return Err(BAD_REQUEST);
    }
    if is_aws_mirror_host(&host) {
        Ok(host)
    } else {
        Err(FORBIDDEN)
    }
}

#[cfg(test)]
mod tests {
    // Every type the tests need is already imported by the parent module.
    use super::*;

    /// A server mirror certificate with its key (stands in for the server's self-signed mirror
    /// certificate), valid for the three mirror hosts.
    const MIRROR_CERT: &str = include_str!("../testdata/proxy-mirror.crt");
    const MIRROR_KEY: &str = include_str!("../testdata/proxy-mirror-key.pem");
    const MIRROR_HOST: &str = "ags-javelin-remote-config.s3.amazonaws.com";
    const OK_BODY: &[u8] = b"layers";

    fn mirror(pem: &str) -> AwsMirror {
        AwsMirror {
            hosts: vec![MIRROR_HOST.to_owned()],
            certificate_pem: pem.to_owned(),
        }
    }

    fn connect_request(authority: &str) -> Vec<u8> {
        format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").into_bytes()
    }

    /// Starts a proxy that relays to `server`. `serve` builds its own runtime, so it runs on a
    /// blocking thread, not on the test's runtime.
    async fn start_proxy(server: SocketAddr) -> AwsProxy {
        tokio::task::spawn_blocking(move || {
            AwsProxy::serve(&mirror(MIRROR_CERT), server, (Ipv4Addr::LOCALHOST, 0).into())
                .expect("proxy")
        })
        .await
        .expect("join")
    }

    #[test]
    fn a_connect_for_a_mirror_host_is_accepted() {
        assert_eq!(
            requested_host(&connect_request("kinesis.us-east-1.amazonaws.com:443")),
            Ok("kinesis.us-east-1.amazonaws.com".to_owned())
        );
        // The host is lower-cased before the check.
        assert_eq!(
            requested_host(&connect_request("STS.us-east-1.amazonaws.com:443")),
            Ok("sts.us-east-1.amazonaws.com".to_owned())
        );
    }

    #[test]
    fn a_connect_for_another_host_is_forbidden() {
        for authority in ["steamcommunity.com:443", "s3.amazonaws.com:443", "evil.net:443"] {
            assert_eq!(requested_host(&connect_request(authority)), Err(FORBIDDEN));
        }
    }

    #[test]
    fn a_non_connect_or_malformed_request_is_a_bad_request() {
        assert_eq!(
            requested_host(b"GET kinesis.us-east-1.amazonaws.com:443 HTTP/1.1\r\n\r\n"),
            Err(BAD_REQUEST)
        );
        assert_eq!(requested_host(b"CONNECT\r\n\r\n"), Err(BAD_REQUEST));
        assert_eq!(requested_host(b"\r\n\r\n"), Err(BAD_REQUEST));
    }

    /// A fake server that shows `cert_pem`/`key_pem` on one TLS connection and echoes a reply.
    async fn fake_server(cert_pem: &str, key_pem: &str) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let certificates = CertificateDer::pem_slice_iter(cert_pem.as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .expect("cert");
        let key = PrivateKeyDer::from_pem_slice(key_pem.as_bytes()).expect("key");
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certificates, key)
            .expect("server config");
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bind");
        let address = listener.local_addr().expect("addr");
        let handle = tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut tls) = acceptor.accept(stream).await
            {
                let mut buffer = [0_u8; 64];
                let read = tls.read(&mut buffer).await.unwrap_or(0);
                let _ = tls.write_all(&buffer[..read]).await;
            }
        });
        (address, handle)
    }

    /// A client that trusts `root_pem`, for the game-facing leg (which trusts the proxy's cert).
    fn client_trusting(root_pem: &str) -> TlsConnector {
        let mut roots = RootCertStore::empty();
        for certificate in CertificateDer::pem_slice_iter(root_pem.as_bytes()) {
            roots.add(certificate.expect("cert")).expect("add");
        }
        TlsConnector::from(Arc::new(
            ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        ))
    }

    #[tokio::test]
    async fn the_proxy_pins_the_mirror_certificate() {
        crate::install_crypto_provider();
        let connector = TlsConnector::from(Arc::new(
            server_client_config(&mirror(MIRROR_CERT)).expect("client config"),
        ));
        let name = ServerName::try_from(MIRROR_HOST).expect("name");

        // The right certificate is accepted.
        let (good, _good) = fake_server(MIRROR_CERT, MIRROR_KEY).await;
        let stream = TcpStream::connect(good).await.expect("connect");
        assert!(connector.connect(name.clone(), stream).await.is_ok());

        // The launcher's own certificate (not the pinned one) is refused.
        let (wrong, _wrong) = fake_server(PROXY_TLS_CERT, PROXY_TLS_KEY).await;
        let stream = TcpStream::connect(wrong).await.expect("connect");
        assert!(connector.connect(name, stream).await.is_err());
    }

    #[tokio::test]
    async fn an_accepted_tunnel_reaches_the_server_and_relays_bytes() {
        crate::install_crypto_provider();
        let (server, _server) = fake_server(MIRROR_CERT, MIRROR_KEY).await;
        let proxy = start_proxy(server).await;

        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port()))
            .await
            .expect("connect");
        stream
            .write_all(&connect_request(&format!("{MIRROR_HOST}:443")))
            .await
            .expect("send connect");
        let mut answer = [0_u8; CONNECT_OK.len()];
        stream.read_exact(&mut answer).await.expect("read answer");
        assert_eq!(&answer, CONNECT_OK);

        // Inside the tunnel the game does TLS to the proxy (trusting the proxy's own cert here).
        let name = ServerName::try_from("localhost").expect("name");
        let mut tls = client_trusting(PROXY_TLS_CERT)
            .connect(name, stream)
            .await
            .expect("tunnel tls");
        tls.write_all(OK_BODY).await.expect("send body");
        let mut echoed = vec![0_u8; OK_BODY.len()];
        tls.read_exact(&mut echoed).await.expect("read echo");
        assert_eq!(echoed, OK_BODY);
    }

    #[tokio::test]
    async fn a_forbidden_host_gets_403_and_no_tunnel() {
        crate::install_crypto_provider();
        let (server, _server) = fake_server(MIRROR_CERT, MIRROR_KEY).await;
        let proxy = start_proxy(server).await;

        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port()))
            .await
            .expect("connect");
        stream
            .write_all(&connect_request("steamcommunity.com:443"))
            .await
            .expect("send connect");
        let mut answer = Vec::new();
        stream.read_to_end(&mut answer).await.expect("read answer");
        assert_eq!(answer, FORBIDDEN);
    }
}
