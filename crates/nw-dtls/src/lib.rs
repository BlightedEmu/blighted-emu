// SPDX-License-Identifier: AGPL-3.0-or-later
//! DTLS 1.2 for the REP connection, with OpenSSL and no sockets (M3 spec section 3.3).
//!
//! The cipher is `ECDHE-RSA-AES256-GCM-SHA384`, the one the client allows. The server answers a
//! first ClientHello with a cookie (HMAC-SHA256 of the peer address with a secret made at start),
//! so a spoofed source address costs it one small reply. The server does not ask for a client
//! certificate: the login ticket is the identity. The client trusts only the certificate it is
//! given.
//!
//! A [`DtlsSession`] takes received datagrams and returns decrypted application data; the
//! datagrams it wants to send wait in [`DtlsSession::take_transmit`]. During the handshake the
//! caller calls [`DtlsSession::poll_timer`] often (for example every 100 ms): OpenSSL then sends a
//! lost flight again when its timer has run out.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::SocketAddr;

use openssl::asn1::Asn1Time;
use openssl::bn::BigNum;
use openssl::error::ErrorStack;
use openssl::ex_data::Index;
use openssl::hash::MessageDigest;
use openssl::memcmp;
use openssl::pkey::{PKey, Private};
use openssl::rsa::Rsa;
use openssl::sign::Signer;
use openssl::ssl::{
    ErrorCode, HandshakeError, MidHandshakeSslStream, Ssl, SslContext, SslContextBuilder,
    SslMethod, SslOptions, SslRef, SslStream, SslVerifyMode, SslVersion,
};
use openssl::x509::store::X509StoreBuilder;
use openssl::x509::{X509, X509NameBuilder};
use thiserror::Error;

/// The only cipher the client offers for `gridmate-udp` (protocol note 1.4).
pub const CIPHER_LIST: &str = "ECDHE-RSA-AES256-GCM-SHA384";
/// Bytes of a server cookie.
pub const COOKIE_LEN: usize = 16;
/// Bytes of the cookie secret.
pub const COOKIE_SECRET_LEN: usize = 32;
/// Largest datagram this session reads.
pub const MAX_DATAGRAM_LEN: usize = 16 * 1024;
/// Datagrams one session keeps waiting to be read or sent.
pub const MAX_QUEUED_DATAGRAMS: usize = 256;
/// RSA key size of a generated identity (the client's own certificate uses RSA 2048).
pub const IDENTITY_KEY_BITS: u32 = 2048;
/// X.509 version 3 (the field holds the version minus 1).
const X509_VERSION_3: i32 = 2;
const SERIAL_NUMBER: u32 = 1;
const COMMON_NAME: &str = "CN";

/// A certificate and its private key, as PEM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// Certificate PEM.
    pub certificate_pem: Vec<u8>,
    /// PKCS#8 private key PEM.
    pub private_key_pem: Vec<u8>,
}

/// Makes a self-signed certificate with an RSA key for `common_name`, valid `valid_days` days
/// from now. The gateway uses it as its own identity; clients trust exactly this certificate.
///
/// # Errors
///
/// Returns [`DtlsError::Setup`] when OpenSSL cannot make the key or the certificate.
pub fn self_signed_identity(common_name: &str, valid_days: u32) -> Result<Identity, DtlsError> {
    let key = PKey::from_rsa(Rsa::generate(IDENTITY_KEY_BITS)?)?;
    let mut subject = X509NameBuilder::new()?;
    subject.append_entry_by_text(COMMON_NAME, common_name)?;
    let subject = subject.build();
    let mut builder = X509::builder()?;
    builder.set_version(X509_VERSION_3)?;
    let serial = BigNum::from_u32(SERIAL_NUMBER)?.to_asn1_integer()?;
    builder.set_serial_number(&serial)?;
    builder.set_subject_name(&subject)?;
    builder.set_issuer_name(&subject)?;
    builder.set_pubkey(&key)?;
    let not_before = Asn1Time::days_from_now(0)?;
    let not_after = Asn1Time::days_from_now(valid_days)?;
    builder.set_not_before(&not_before)?;
    builder.set_not_after(&not_after)?;
    builder.sign(&key, MessageDigest::sha256())?;
    Ok(Identity {
        certificate_pem: builder.build().to_pem()?,
        private_key_pem: key.private_key_to_pem_pkcs8()?,
    })
}

/// Failure of DTLS setup or of one session.
#[derive(Debug, Error)]
pub enum DtlsError {
    /// OpenSSL refused a setting, a certificate, or a key.
    #[error("DTLS setup failed: {0}")]
    Setup(#[from] ErrorStack),
    /// The handshake failed (for example an untrusted certificate or a bad record).
    #[error("DTLS handshake failed: {0}")]
    Handshake(String),
    /// A record after the handshake could not be read or written.
    #[error("DTLS record failed: {0}")]
    Record(String),
    /// Application data was sent before the handshake finished.
    #[error("DTLS session is not established")]
    NotEstablished,
    /// Too many datagrams wait in the session.
    #[error("too many datagrams wait in the DTLS session")]
    Backlog,
    /// The session failed earlier and takes no more data.
    #[error("DTLS session has failed")]
    Failed,
}

/// The server side settings: certificate, key, and cookie secret.
pub struct DtlsServerConfig {
    context: SslContext,
    peer_index: Index<Ssl, SocketAddr>,
}

impl DtlsServerConfig {
    /// Server settings from a PEM certificate and a PEM private key.
    ///
    /// # Errors
    ///
    /// Returns [`DtlsError::Setup`] when the certificate or key does not load or does not match.
    pub fn new(
        certificate_pem: &[u8],
        private_key_pem: &[u8],
        cookie_secret: [u8; COOKIE_SECRET_LEN],
    ) -> Result<Self, DtlsError> {
        let certificate = X509::from_pem(certificate_pem)?;
        let key = PKey::private_key_from_pem(private_key_pem)?;
        let peer_index = Ssl::new_ex_index::<SocketAddr>()?;
        let mut builder = dtls_builder()?;
        builder.set_certificate(&certificate)?;
        builder.set_private_key(&key)?;
        builder.check_private_key()?;
        builder.set_options(SslOptions::COOKIE_EXCHANGE);
        let secret = PKey::hmac(&cookie_secret)?;
        let generate_secret = secret.clone();
        builder.set_cookie_generate_cb(move |ssl, out| {
            let cookie = cookie_for(ssl, peer_index, &generate_secret)?;
            out[..COOKIE_LEN].copy_from_slice(&cookie);
            Ok(COOKIE_LEN)
        });
        builder.set_cookie_verify_cb(move |ssl, cookie| {
            cookie_for(ssl, peer_index, &secret)
                .is_ok_and(|expected| cookie.len() == COOKIE_LEN && memcmp::eq(&expected, cookie))
        });
        Ok(Self {
            context: builder.build(),
            peer_index,
        })
    }
}

/// The client side settings: the one certificate it trusts.
pub struct DtlsClientConfig {
    context: SslContext,
}

impl DtlsClientConfig {
    /// Client settings that trust only `trusted_certificate_pem`.
    ///
    /// # Errors
    ///
    /// Returns [`DtlsError::Setup`] when the certificate does not load.
    pub fn new(trusted_certificate_pem: &[u8]) -> Result<Self, DtlsError> {
        let certificate = X509::from_pem(trusted_certificate_pem)?;
        let mut store = X509StoreBuilder::new()?;
        store.add_cert(certificate)?;
        let mut builder = dtls_builder()?;
        builder.set_cert_store(store.build());
        builder.set_verify(SslVerifyMode::PEER);
        Ok(Self {
            context: builder.build(),
        })
    }
}

fn dtls_builder() -> Result<SslContextBuilder, ErrorStack> {
    let mut builder = SslContext::builder(SslMethod::dtls())?;
    builder.set_min_proto_version(Some(SslVersion::DTLS1_2))?;
    builder.set_max_proto_version(Some(SslVersion::DTLS1_2))?;
    builder.set_cipher_list(CIPHER_LIST)?;
    builder.set_options(SslOptions::NO_QUERY_MTU);
    Ok(builder)
}

fn cookie_for(
    ssl: &SslRef,
    peer_index: Index<Ssl, SocketAddr>,
    secret: &PKey<Private>,
) -> Result<[u8; COOKIE_LEN], ErrorStack> {
    let peer = ssl
        .ex_data(peer_index)
        .map(ToString::to_string)
        .unwrap_or_default();
    let mut signer = Signer::new(MessageDigest::sha256(), secret)?;
    let digest = signer.sign_oneshot_to_vec(peer.as_bytes())?;
    let mut cookie = [0_u8; COOKIE_LEN];
    cookie.copy_from_slice(&digest[..COOKIE_LEN]);
    Ok(cookie)
}

/// In-memory datagram queues between OpenSSL and the caller.
#[derive(Debug, Default)]
struct Pipe {
    inbox: VecDeque<Vec<u8>>,
    outbox: VecDeque<Vec<u8>>,
}

impl Read for Pipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let Some(datagram) = self.inbox.pop_front() else {
            return Err(io::ErrorKind::WouldBlock.into());
        };
        let len = datagram.len().min(buf.len());
        buf[..len].copy_from_slice(&datagram[..len]);
        Ok(len)
    }
}

impl Write for Pipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.outbox.len() >= MAX_QUEUED_DATAGRAMS {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        self.outbox.push_back(buf.to_vec());
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

enum State {
    Handshaking(MidHandshakeSslStream<Pipe>),
    Established(SslStream<Pipe>),
    Failed,
}

/// One DTLS session, server or client.
pub struct DtlsSession {
    state: State,
}

impl DtlsSession {
    /// A server session for a peer at `peer`; it waits for the ClientHello.
    ///
    /// # Errors
    ///
    /// Returns [`DtlsError`] when OpenSSL cannot start the session.
    pub fn accept(
        config: &DtlsServerConfig,
        peer: SocketAddr,
        mtu: u32,
    ) -> Result<Self, DtlsError> {
        let mut ssl = Ssl::new(&config.context)?;
        ssl.set_mtu(mtu)?;
        ssl.set_ex_data(config.peer_index, peer);
        Self::start(ssl.accept(Pipe::default()))
    }

    /// A client session; its ClientHello waits in [`DtlsSession::take_transmit`].
    ///
    /// # Errors
    ///
    /// Returns [`DtlsError`] when OpenSSL cannot start the session.
    pub fn connect(config: &DtlsClientConfig, mtu: u32) -> Result<Self, DtlsError> {
        let mut ssl = Ssl::new(&config.context)?;
        ssl.set_mtu(mtu)?;
        Self::start(ssl.connect(Pipe::default()))
    }

    fn start(result: Result<SslStream<Pipe>, HandshakeError<Pipe>>) -> Result<Self, DtlsError> {
        let state = match result {
            Ok(stream) => State::Established(stream),
            Err(HandshakeError::WouldBlock(mid)) => State::Handshaking(mid),
            Err(error) => return Err(DtlsError::Handshake(error.to_string())),
        };
        Ok(Self { state })
    }

    /// Whether the handshake is done.
    pub fn is_established(&self) -> bool {
        matches!(self.state, State::Established(_))
    }

    /// Takes one received datagram and returns the application data it holds.
    ///
    /// # Errors
    ///
    /// Returns [`DtlsError`] when the handshake fails, a record is bad, too many datagrams wait,
    /// or the session failed before. The caller drops the session.
    pub fn receive(&mut self, datagram: &[u8]) -> Result<Vec<Vec<u8>>, DtlsError> {
        let inbox = match &mut self.state {
            State::Handshaking(mid) => &mut mid.get_mut().inbox,
            State::Established(stream) => &mut stream.get_mut().inbox,
            State::Failed => return Err(DtlsError::Failed),
        };
        if inbox.len() >= MAX_QUEUED_DATAGRAMS {
            return Err(DtlsError::Backlog);
        }
        inbox.push_back(datagram.to_vec());
        self.poll_timer()?;
        self.read_all()
    }

    /// Moves the handshake on; OpenSSL sends a lost flight again when its timer has run out.
    ///
    /// # Errors
    ///
    /// Returns [`DtlsError::Handshake`] when the handshake fails.
    pub fn poll_timer(&mut self) -> Result<(), DtlsError> {
        let State::Handshaking(_) = self.state else {
            return Ok(());
        };
        let State::Handshaking(mid) = std::mem::replace(&mut self.state, State::Failed) else {
            return Ok(());
        };
        self.state = match mid.handshake() {
            Ok(stream) => State::Established(stream),
            Err(HandshakeError::WouldBlock(mid)) => State::Handshaking(mid),
            Err(error) => return Err(DtlsError::Handshake(error.to_string())),
        };
        Ok(())
    }

    fn read_all(&mut self) -> Result<Vec<Vec<u8>>, DtlsError> {
        let State::Established(stream) = &mut self.state else {
            return Ok(Vec::new());
        };
        let mut received = Vec::new();
        let mut buffer = vec![0_u8; MAX_DATAGRAM_LEN];
        loop {
            match stream.ssl_read(&mut buffer) {
                Ok(len) => received.push(buffer[..len].to_vec()),
                Err(error) if error.code() == ErrorCode::WANT_READ => return Ok(received),
                Err(error) => {
                    self.state = State::Failed;
                    return Err(DtlsError::Record(error.to_string()));
                }
            }
        }
    }

    /// Encrypts one application datagram.
    ///
    /// # Errors
    ///
    /// Returns [`DtlsError::NotEstablished`] before the handshake is done, and
    /// [`DtlsError::Record`] when OpenSSL cannot write it.
    pub fn send(&mut self, plaintext: &[u8]) -> Result<(), DtlsError> {
        let State::Established(stream) = &mut self.state else {
            return Err(DtlsError::NotEstablished);
        };
        stream
            .ssl_write(plaintext)
            .map(|_| ())
            .map_err(|error| DtlsError::Record(error.to_string()))
    }

    /// Datagrams to send to the peer now.
    pub fn take_transmit(&mut self) -> Vec<Vec<u8>> {
        let outbox = match &mut self.state {
            State::Handshaking(mid) => &mut mid.get_mut().outbox,
            State::Established(stream) => &mut stream.get_mut().outbox,
            State::Failed => return Vec::new(),
        };
        outbox.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;
    use std::time::{Duration, Instant};

    use proptest::prelude::*;

    use super::*;

    const MTU: u32 = 1200;
    const VALID_DAYS: u32 = 30;
    const SECRET: [u8; COOKIE_SECRET_LEN] = [7; COOKIE_SECRET_LEN];
    const MESSAGE: &[u8] = b"carrier datagram";
    const PEER: &str = "192.0.2.10:40000";
    const OTHER_PEER: &str = "192.0.2.11:40000";
    /// Longer than the DTLS start timer (1 s).
    const RESEND_WAIT: Duration = Duration::from_millis(1100);
    const STEP: Duration = Duration::from_millis(20);
    const MAX_ROUNDS: usize = 200;
    const TLS_HANDSHAKE_RECORD: u8 = 22;
    /// Byte of the handshake type in a DTLS handshake record: 13 header bytes, then the type.
    const HANDSHAKE_TYPE_OFFSET: usize = 13;
    const HELLO_VERIFY_REQUEST: u8 = 3;
    const SERVER_HELLO: u8 = 2;
    /// A resend after the timer, not at once.
    const MIN_RESEND_DELAY: Duration = Duration::from_millis(500);

    /// One server identity for every test (RSA key generation is slow).
    static REP: LazyLock<(Vec<u8>, Vec<u8>)> = LazyLock::new(|| identity("rep"));
    /// A second identity that the server does not have.
    static OTHER: LazyLock<(Vec<u8>, Vec<u8>)> = LazyLock::new(|| identity("other"));

    /// A self-signed certificate and its key, as PEM.
    fn identity(name: &str) -> (Vec<u8>, Vec<u8>) {
        let identity = self_signed_identity(name, VALID_DAYS).expect("identity");
        (identity.certificate_pem, identity.private_key_pem)
    }

    fn pair(trusted: &[u8], certificate: &[u8], key: &[u8]) -> (DtlsSession, DtlsSession) {
        let server_config = DtlsServerConfig::new(certificate, key, SECRET).expect("server config");
        let client_config = DtlsClientConfig::new(trusted).expect("client config");
        let server =
            DtlsSession::accept(&server_config, PEER.parse().expect("peer"), MTU).expect("accept");
        let client = DtlsSession::connect(&client_config, MTU).expect("connect");
        (client, server)
    }

    /// Moves datagrams both ways until both sides are established or a side fails.
    fn handshake(client: &mut DtlsSession, server: &mut DtlsSession) -> Result<(), DtlsError> {
        for _ in 0..MAX_ROUNDS {
            for datagram in client.take_transmit() {
                server.receive(&datagram)?;
            }
            for datagram in server.take_transmit() {
                client.receive(&datagram)?;
            }
            if client.is_established() && server.is_established() {
                return Ok(());
            }
            client.poll_timer()?;
            server.poll_timer()?;
        }
        Err(DtlsError::Handshake("no progress".to_owned()))
    }

    #[test]
    fn a_trusted_server_completes_the_handshake_and_carries_data() {
        let (certificate, key) = &*REP;
        let (mut client, mut server) = pair(certificate, certificate, key);
        handshake(&mut client, &mut server).expect("handshake");
        client.send(MESSAGE).expect("send");
        let mut received = Vec::new();
        for datagram in client.take_transmit() {
            received.extend(server.receive(&datagram).expect("receive"));
        }
        assert_eq!(received, vec![MESSAGE.to_vec()]);
        server.send(MESSAGE).expect("reply");
        let mut replies = Vec::new();
        for datagram in server.take_transmit() {
            replies.extend(client.receive(&datagram).expect("client receive"));
        }
        assert_eq!(replies, vec![MESSAGE.to_vec()]);
    }

    #[test]
    fn the_first_client_hello_gets_a_cookie() {
        let (certificate, key) = &*REP;
        let (mut client, mut server) = pair(certificate, certificate, key);
        for datagram in client.take_transmit() {
            server.receive(&datagram).expect("hello");
        }
        let reply = server.take_transmit();
        assert_eq!(reply.len(), 1);
        assert_eq!(reply[0][0], TLS_HANDSHAKE_RECORD);
        assert_eq!(reply[0][HANDSHAKE_TYPE_OFFSET], HELLO_VERIFY_REQUEST);
    }

    #[test]
    fn a_cookie_made_for_another_address_is_refused() {
        let (certificate, key) = &*REP;
        let server_config = DtlsServerConfig::new(certificate, key, SECRET).expect("server config");
        let client_config = DtlsClientConfig::new(certificate).expect("client config");
        let mut client = DtlsSession::connect(&client_config, MTU).expect("connect");
        let mut first =
            DtlsSession::accept(&server_config, PEER.parse().expect("peer"), MTU).expect("accept");
        for datagram in client.take_transmit() {
            first.receive(&datagram).expect("hello");
        }
        for datagram in first.take_transmit() {
            client.receive(&datagram).expect("cookie");
        }
        let mut other = DtlsSession::accept(&server_config, OTHER_PEER.parse().expect("peer"), MTU)
            .expect("other");
        for datagram in client.take_transmit() {
            other.receive(&datagram).expect("hello with cookie");
        }
        let reply = other.take_transmit();
        assert!(!other.is_established());
        assert!(
            reply
                .iter()
                .all(|datagram| datagram.get(HANDSHAKE_TYPE_OFFSET) != Some(&SERVER_HELLO))
        );
    }

    #[test]
    fn a_client_that_trusts_another_certificate_fails() {
        let (certificate, key) = &*REP;
        let (other, _) = &*OTHER;
        let (mut client, mut server) = pair(other, certificate, key);
        assert!(matches!(
            handshake(&mut client, &mut server),
            Err(DtlsError::Handshake(_))
        ));
    }

    #[test]
    fn a_lost_flight_is_sent_again_after_the_timer() {
        let (certificate, key) = &*REP;
        let (mut client, mut server) = pair(certificate, certificate, key);
        for datagram in client.take_transmit() {
            server.receive(&datagram).expect("hello");
        }
        for datagram in server.take_transmit() {
            client.receive(&datagram).expect("cookie");
        }
        for datagram in client.take_transmit() {
            server.receive(&datagram).expect("hello with cookie");
        }
        let lost = server.take_transmit();
        assert!(!lost.is_empty());
        let start = Instant::now();
        let mut resent = Vec::new();
        while resent.is_empty() && start.elapsed() < 2 * RESEND_WAIT {
            std::thread::sleep(STEP);
            client.poll_timer().expect("client timer");
            server.poll_timer().expect("server timer");
            resent = server.take_transmit();
        }
        assert!(!resent.is_empty(), "the server sent nothing again");
        assert!(start.elapsed() >= MIN_RESEND_DELAY);
        for datagram in resent {
            client.receive(&datagram).expect("resent flight");
        }
        handshake(&mut client, &mut server).expect("handshake after loss");
    }

    #[test]
    fn data_before_the_handshake_is_refused() {
        let (certificate, key) = &*REP;
        let (mut client, _) = pair(certificate, certificate, key);
        assert!(matches!(
            client.send(MESSAGE),
            Err(DtlsError::NotEstablished)
        ));
    }

    #[test]
    fn a_bad_certificate_or_key_is_a_setup_error() {
        let (certificate, key) = &*REP;
        let (_, other_key) = &*OTHER;
        assert!(matches!(
            DtlsServerConfig::new(b"not pem", key, SECRET),
            Err(DtlsError::Setup(_))
        ));
        assert!(matches!(
            DtlsServerConfig::new(certificate, other_key, SECRET),
            Err(DtlsError::Setup(_))
        ));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        #[test]
        fn random_datagrams_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
            let (certificate, key) = &*REP;
            let server_config = DtlsServerConfig::new(certificate, key, SECRET).expect("config");
            let mut server = DtlsSession::accept(&server_config, PEER.parse().expect("peer"), MTU).expect("accept");
            let _ = server.receive(&bytes);
        }
    }
}
