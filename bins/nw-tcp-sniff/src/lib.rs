// SPDX-License-Identifier: AGPL-3.0-or-later
//! A diagnostic TCP listener.
//!
//! It accepts TCP connections and records every byte a peer sends. Each connection writes a raw
//! `.bin` dump file and logs a short hex preview with a running byte total. It never sends a byte
//! back, so it only shows what the client sends first. Its purpose is to learn the TCP transport
//! framing of the real client, which no capture records because the live servers used
//! `gridmate-udp`.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use nw_common::logging::{LoggingError, init_logging};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Bytes read from the socket in one step.
const READ_CHUNK_BYTES: usize = 4096;
/// Bytes of each hex preview logged per chunk.
const HEX_PREVIEW_BYTES: usize = 64;

/// Settings for the sniff listener.
#[derive(Debug, Clone)]
pub struct SniffConfig {
    /// Address to listen on.
    pub listen: SocketAddr,
    /// Directory for the per-connection dump files.
    pub out_dir: PathBuf,
    /// `tracing` filter, for example `info`.
    pub log_filter: String,
}

/// Failure of the sniff listener.
#[derive(Debug, thiserror::Error)]
pub enum SniffError {
    /// Logging setup failed.
    #[error(transparent)]
    Logging(#[from] LoggingError),
    /// A socket, a file, or the runtime failed.
    #[error("sniff I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

/// Runs the sniff listener until the process is stopped.
///
/// # Errors
///
/// Returns [`SniffError`] when logging setup, the runtime, or the listening socket fails.
pub fn serve(config: SniffConfig) -> Result<(), SniffError> {
    init_logging(&config.log_filter)?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(serve_async(config))
}

/// Binds the listener and records each accepted connection until Ctrl-C.
async fn serve_async(config: SniffConfig) -> Result<(), SniffError> {
    tokio::fs::create_dir_all(&config.out_dir).await?;
    let listener = TcpListener::bind(config.listen).await?;
    tracing::info!(address = %config.listen, out_dir = %config.out_dir.display(), "sniffing TCP");
    let mut next_id: u64 = 0;
    loop {
        let accept = tokio::select! {
            result = listener.accept() => result,
            _ = tokio::signal::ctrl_c() => return Ok(()),
        };
        let (stream, peer) = accept?;
        let id = next_id;
        next_id += 1;
        let path = config.out_dir.join(dump_file_name(id, peer));
        tokio::spawn(async move {
            tracing::info!(id, %peer, path = %path.display(), "connection opened");
            match record_stream(stream, &path).await {
                Ok(total) => tracing::info!(id, %peer, total, "connection closed"),
                Err(error) => tracing::warn!(id, %peer, %error, "connection failed"),
            }
        });
    }
}

/// Reads every byte from `reader`, appends it to `dump_path`, and logs a hex preview of each
/// chunk. Returns the total number of bytes read.
///
/// # Errors
///
/// Returns [`SniffError`] when the dump file cannot be written or the socket read fails.
async fn record_stream<R: AsyncRead + Unpin>(
    mut reader: R,
    dump_path: &Path,
) -> Result<u64, SniffError> {
    let mut file = tokio::fs::File::create(dump_path).await?;
    let mut buffer = [0_u8; READ_CHUNK_BYTES];
    let mut total: u64 = 0;
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            file.flush().await?;
            return Ok(total);
        }
        let chunk = &buffer[..read];
        file.write_all(chunk).await?;
        total += read as u64;
        tracing::info!(
            bytes = read,
            total,
            preview = %hex_preview(chunk, HEX_PREVIEW_BYTES),
            "received"
        );
    }
}

/// The dump file name for a connection: its id and its peer, with no characters that a path
/// rejects.
fn dump_file_name(id: u64, peer: SocketAddr) -> String {
    let peer = peer.to_string().replace([':', '.'], "-");
    format!("conn-{id:04}-{peer}.bin")
}

/// A hex preview of up to `max` bytes, with a trailing marker when more bytes follow.
fn hex_preview(bytes: &[u8], max: usize) -> String {
    let shown = bytes.len().min(max);
    let mut text = hex::encode(&bytes[..shown]);
    if bytes.len() > max {
        text.push('…');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = &[0x80, 0x00, 0x05, 0x68, 0x65, 0x6c, 0x6c, 0x6f];

    #[test]
    fn hex_preview_shows_all_bytes_when_short() {
        assert_eq!(hex_preview(SAMPLE, HEX_PREVIEW_BYTES), "80000568656c6c6f");
    }

    #[test]
    fn hex_preview_truncates_and_marks_more() {
        assert_eq!(hex_preview(SAMPLE, 3), "800005…");
    }

    #[test]
    fn dump_file_name_has_no_path_separators() {
        let peer: SocketAddr = "127.0.0.1:33435".parse().expect("address");
        assert_eq!(dump_file_name(7, peer), "conn-0007-127-0-0-1-33435.bin");
    }

    #[tokio::test]
    async fn record_stream_writes_every_byte_to_the_dump() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("dump.bin");
        let (mut writer, reader) = tokio::io::duplex(16);
        let write = tokio::spawn(async move {
            writer.write_all(SAMPLE).await.expect("write");
            // Dropping the writer ends the stream so the reader sees EOF.
        });
        let total = record_stream(reader, &path).await.expect("record");
        write.await.expect("writer task");
        assert_eq!(total, SAMPLE.len() as u64);
        assert_eq!(std::fs::read(&path).expect("read dump"), SAMPLE);
    }
}
