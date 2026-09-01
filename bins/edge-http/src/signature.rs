// SPDX-License-Identifier: AGPL-3.0-or-later
//! HMAC-SHA256 signature for a one-time entry ticket.
//!
//! The client protocol note does not record the signature algorithm.
//! This string is non-empty, as the architecture spec requires.
//! It is a signature for this preservation server. It is not a Steam ticket
//! and it is not a signature for an official server.

use hmac::{KeyInit, Mac};
use sha2::Sha256;

/// Separator between the signed fields.
const FIELD_SEPARATOR: char = '\n';

/// Hex length of a SHA-256 HMAC.
const SIGNATURE_HEX_LEN: usize = 64;

/// Builds the signature placed in `Token.Signature`.
pub(crate) fn entry_signature(
    key: &[u8],
    ticket_id: &str,
    world_id: &str,
    character_id: Option<&str>,
    expires_at_unix: i64,
) -> Result<String, SignatureError> {
    let character_id = character_id.unwrap_or("");
    let payload = format!(
        "{ticket_id}{FIELD_SEPARATOR}{world_id}{FIELD_SEPARATOR}{character_id}{FIELD_SEPARATOR}{expires_at_unix}"
    );
    let mut mac = hmac::Hmac::<Sha256>::new_from_slice(key).map_err(|_| SignatureError)?;
    mac.update(payload.as_bytes());
    let encoded = hex::encode(mac.finalize().into_bytes());
    debug_assert_eq!(encoded.len(), SIGNATURE_HEX_LEN);
    Ok(encoded)
}

/// The signature key was rejected by HMAC.
#[derive(Debug, thiserror::Error)]
#[error("signature key was rejected")]
pub(crate) struct SignatureError;

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &[u8] = b"change-me-signature-key";
    const TICKET: &str = "ticket-1";
    const WORLD: &str = "world-1";
    const CHARACTER: &str = "char-1";
    const EXPIRES: i64 = 1_700_000_000;

    #[test]
    fn signature_is_64_hex_characters_and_changes_with_the_ticket() {
        let signature =
            entry_signature(KEY, TICKET, WORLD, Some(CHARACTER), EXPIRES).expect("sign");
        assert_eq!(signature.len(), SIGNATURE_HEX_LEN);
        assert!(
            signature
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        );
        let again = entry_signature(KEY, TICKET, WORLD, Some(CHARACTER), EXPIRES).expect("sign");
        assert_eq!(signature, again);
        let other =
            entry_signature(KEY, "ticket-2", WORLD, Some(CHARACTER), EXPIRES).expect("sign");
        assert_ne!(signature, other);
    }
}
