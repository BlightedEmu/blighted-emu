// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Token.Signature` of an entry ticket (M2 spec section 8): hex HMAC-SHA256 over
//! length-prefixed fields. `rep-gateway` checks it in M3.

use hmac::Mac;
use sha2::Sha256;
use uuid::Uuid;

use crate::secrets::SigningKey;

/// The fields an entry ticket signature covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryClaims {
    /// Entry ticket id.
    pub ticket_id: String,
    /// Account that may use the ticket.
    pub account_id: Uuid,
    /// World of the ticket.
    pub world_id: Uuid,
    /// Character that enters the world.
    pub character_id: Uuid,
    /// Expiry, Unix seconds.
    pub expires_at: i64,
}

impl EntryClaims {
    /// The signed bytes: each field as a `u32` big-endian length, then its UTF-8 text.
    pub fn payload(&self) -> Vec<u8> {
        let fields = [
            self.ticket_id.clone(),
            self.account_id.to_string(),
            self.world_id.to_string(),
            self.character_id.to_string(),
            self.expires_at.to_string(),
        ];
        let mut payload = Vec::new();
        for field in fields {
            // Every field is far shorter than 4 GiB.
            payload.extend_from_slice(&(field.len() as u32).to_be_bytes());
            payload.extend_from_slice(field.as_bytes());
        }
        payload
    }
}

/// Hex HMAC-SHA256 of the claims.
pub fn sign_entry(key: &SigningKey, claims: &EntryClaims) -> String {
    hex::encode(mac(key, claims).finalize().into_bytes())
}

/// True when `signature` is the hex HMAC-SHA256 of the claims (compared in constant time).
pub fn verify_entry(key: &SigningKey, claims: &EntryClaims, signature: &str) -> bool {
    hex::decode(signature).is_ok_and(|bytes| mac(key, claims).verify_slice(&bytes).is_ok())
}

fn mac(key: &SigningKey, claims: &EntryClaims) -> hmac::Hmac<Sha256> {
    let mut mac = key.mac();
    mac.update(&claims.payload());
    mac
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_HEX: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    const SIGNATURE_HEX_LEN: usize = 64;
    const EXPIRES_AT: i64 = 1_760_400_000;

    fn claims() -> EntryClaims {
        EntryClaims {
            ticket_id: "ticket-1".to_owned(),
            account_id: Uuid::from_u128(1),
            world_id: Uuid::from_u128(2),
            character_id: Uuid::from_u128(3),
            expires_at: EXPIRES_AT,
        }
    }

    fn key() -> SigningKey {
        SigningKey::from_hex(KEY_HEX).expect("key")
    }

    #[test]
    fn payload_is_length_prefixed() {
        let payload = claims().payload();
        assert_eq!(&payload[..4], &8_u32.to_be_bytes());
        assert_eq!(&payload[4..12], b"ticket-1");
    }

    #[test]
    fn signature_verifies_and_binds_every_field() {
        let signature = sign_entry(&key(), &claims());
        assert_eq!(signature.len(), SIGNATURE_HEX_LEN);
        assert!(verify_entry(&key(), &claims(), &signature));
        let mut other_account = claims();
        other_account.account_id = Uuid::from_u128(9);
        assert!(!verify_entry(&key(), &other_account, &signature));
        let mut other_expiry = claims();
        other_expiry.expires_at += 1;
        assert!(!verify_entry(&key(), &other_expiry, &signature));
        assert!(!verify_entry(&key(), &claims(), "not hex"));
    }
}
