// SPDX-License-Identifier: AGPL-3.0-or-later
//! The login ticket check and the registration response (M3 spec section 7; carrier captures note
//! section 7).
//!
//! The ticket is taken one time from the queue store before any other check, so a ticket works at
//! most once, also when a later check fails.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nw_login::queue::{EntryRecord, QueueStore};
use nw_login::secrets::SigningKey;
use nw_login::signature::verify_entry;
use nw_reflect::{LoginToken, RegistrationResponseMsg};
use uuid::Uuid;

/// Response status of an accepted registration.
pub const STATUS_AUTHORIZED: u32 = 0;
/// Response status of a refused registration; the gateway then disconnects.
pub const STATUS_REFUSED: u32 = 1;
/// The four bytes of all 43 captured responses (meanings Unknown).
pub const OBSERVED_RESPONSE_FLAGS: [u8; 4] = [1, 0, 0, 1];
/// Random bytes of a session token; it is sent as 32 upper-case hexadecimal characters.
pub const SESSION_TOKEN_BYTES: usize = 16;
/// Start of the response clock: 2000-01-01 00:00:00 UTC.
pub const CLOCK_EPOCH: Duration = Duration::from_secs(946_684_800);
/// Characters of a ticket id kept in logs.
pub const LOGGED_TICKET_CHARS: usize = 8;

/// The ticket fields of a V3 request, as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketClaims {
    /// `TicketId`.
    pub ticket_id: String,
    /// `Signature`.
    pub signature: String,
    /// `CharacterId`.
    pub character_id: String,
}

impl TicketClaims {
    /// The ticket fields of a login token. Bytes that are not UTF-8 make the text fail the checks.
    pub fn from_token(token: &LoginToken) -> Self {
        Self {
            ticket_id: String::from_utf8_lossy(&token.ticket_id).into_owned(),
            signature: String::from_utf8_lossy(&token.signature).into_owned(),
            character_id: String::from_utf8_lossy(&token.character_id).into_owned(),
        }
    }

    /// The ticket id with all but the first characters hidden, for logs.
    pub fn masked_ticket(&self) -> String {
        let shown: String = self.ticket_id.chars().take(LOGGED_TICKET_CHARS).collect();
        format!("{shown}…")
    }
}

/// Why a registration was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// No entry record: unknown, expired in the store, or already used.
    UnknownTicket,
    /// The signature does not match the record.
    BadSignature,
    /// The record is for another world.
    OtherWorld,
    /// The record is for another character.
    OtherCharacter,
    /// The record has expired.
    Expired,
    /// The queue store could not be reached.
    StoreUnavailable,
}

impl Refusal {
    /// A short name for logs and metrics.
    pub const fn reason(self) -> &'static str {
        match self {
            Self::UnknownTicket => "unknown_ticket",
            Self::BadSignature => "bad_signature",
            Self::OtherWorld => "other_world",
            Self::OtherCharacter => "other_character",
            Self::Expired => "expired",
            Self::StoreUnavailable => "store_unavailable",
        }
    }
}

/// Takes the ticket from the store and checks it, in order: a record exists, the signature
/// matches (constant time), the world is `world_id`, the character matches, and the record has
/// not expired at `now_unix_seconds`.
///
/// # Errors
///
/// Returns the first [`Refusal`] that applies.
pub async fn check_ticket<Q: QueueStore>(
    store: &Q,
    key: &SigningKey,
    world_id: Uuid,
    claims: &TicketClaims,
    now_unix_seconds: i64,
) -> Result<EntryRecord, Refusal> {
    let record = store
        .take_entry(&claims.ticket_id)
        .await
        .map_err(|_| Refusal::StoreUnavailable)?
        .ok_or(Refusal::UnknownTicket)?;
    if !verify_entry(key, &record.claims(), &claims.signature) {
        return Err(Refusal::BadSignature);
    }
    if record.world_id != world_id {
        return Err(Refusal::OtherWorld);
    }
    if record.character_id.to_string() != claims.character_id {
        return Err(Refusal::OtherCharacter);
    }
    if record.expires_at < now_unix_seconds {
        return Err(Refusal::Expired);
    }
    Ok(record)
}

/// The response body: `status`, the server clock in nanoseconds since 2000-01-01, a new session
/// token for an accepted registration (empty for a refused one), the server version text, and the
/// observed four bytes.
///
/// # Errors
///
/// Returns [`getrandom::Error`] when the system cannot give random bytes.
pub fn response(
    status: u32,
    now: SystemTime,
    server_version: &str,
) -> Result<RegistrationResponseMsg, getrandom::Error> {
    let session_token = if status == STATUS_AUTHORIZED {
        let mut bytes = [0_u8; SESSION_TOKEN_BYTES];
        getrandom::fill(&mut bytes)?;
        hex::encode_upper(bytes).into_bytes()
    } else {
        Vec::new()
    };
    Ok(RegistrationResponseMsg {
        uint32_at_0x8: status,
        uint64: clock_nanos(now),
        bytes_at_0x18: session_token,
        bytes_at_0x38: server_version.as_bytes().to_vec(),
        bytes_at_0x58: OBSERVED_RESPONSE_FLAGS,
    })
}

/// Nanoseconds from 2000-01-01 00:00:00 UTC to `now`; 0 before that day.
fn clock_nanos(now: SystemTime) -> u64 {
    now.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|since_unix| since_unix.checked_sub(CLOCK_EPOCH))
        .map_or(0, |since_epoch| {
            u64::try_from(since_epoch.as_nanos()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use nw_login::queue::{QueueAction, QueueSettings, QueueStatus, QueueTime};
    use nw_login::signature::sign_entry;
    use nw_login::store::StoreError;

    use super::*;

    const KEY_HEX: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    const OTHER_KEY_HEX: &str = "ff0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    const NOW: i64 = 1_800_000_000;
    const LIFETIME: i64 = 60;
    const TICKET: &str = "8d0f4c2e000040008000000000000001.ticket";
    const VERSION: &str = "[BLIGHTED].Javelin.1.365.6031.6004151";
    /// 2026-01-01 00:00:00 UTC in Unix seconds.
    const NEW_YEAR_2026: u64 = 1_767_225_600;
    /// The same time as nanoseconds since 2000-01-01.
    const NEW_YEAR_2026_CLOCK: u64 = 820_540_800_000_000_000;

    /// A queue store that holds entry records only.
    #[derive(Clone, Default)]
    struct Records {
        records: Arc<Mutex<HashMap<String, EntryRecord>>>,
        down: bool,
    }

    impl QueueStore for Records {
        async fn update(
            &self,
            _: &QueueSettings,
            _: Uuid,
            _: QueueAction,
            _: QueueTime,
        ) -> Result<Option<QueueStatus>, StoreError> {
            Ok(None)
        }

        async fn take_entry(&self, ticket_id: &str) -> Result<Option<EntryRecord>, StoreError> {
            if self.down {
                return Err(StoreError::unavailable(std::io::Error::other("down")));
            }
            Ok(self.records.lock().expect("lock").remove(ticket_id))
        }
    }

    fn key() -> SigningKey {
        SigningKey::from_hex(KEY_HEX).expect("key")
    }

    fn record(world_id: Uuid, character_id: Uuid) -> EntryRecord {
        EntryRecord {
            ticket_id: TICKET.to_owned(),
            account_id: Uuid::new_v4(),
            world_id,
            character_id,
            expires_at: NOW + LIFETIME,
        }
    }

    /// A store with `record` and the claims a client sends for it.
    fn setup(record: &EntryRecord, signed_with: &SigningKey) -> (Records, TicketClaims) {
        let store = Records::default();
        store
            .records
            .lock()
            .expect("lock")
            .insert(record.ticket_id.clone(), record.clone());
        let claims = TicketClaims {
            ticket_id: record.ticket_id.clone(),
            signature: sign_entry(signed_with, &record.claims()),
            character_id: record.character_id.to_string(),
        };
        (store, claims)
    }

    #[tokio::test]
    async fn a_good_ticket_is_accepted_once() {
        let world = Uuid::new_v4();
        let entry = record(world, Uuid::new_v4());
        let (store, claims) = setup(&entry, &key());
        assert_eq!(
            check_ticket(&store, &key(), world, &claims, NOW).await,
            Ok(entry)
        );
        assert_eq!(
            check_ticket(&store, &key(), world, &claims, NOW).await,
            Err(Refusal::UnknownTicket)
        );
    }

    #[tokio::test]
    async fn each_check_refuses_its_case_and_the_ticket_is_gone() {
        let world = Uuid::new_v4();
        let other_key = SigningKey::from_hex(OTHER_KEY_HEX).expect("other key");
        let cases = [
            (
                record(world, Uuid::new_v4()),
                other_key,
                world,
                None,
                NOW,
                Refusal::BadSignature,
            ),
            (
                record(Uuid::new_v4(), Uuid::new_v4()),
                key(),
                world,
                None,
                NOW,
                Refusal::OtherWorld,
            ),
            (
                record(world, Uuid::new_v4()),
                key(),
                world,
                Some(Uuid::new_v4()),
                NOW,
                Refusal::OtherCharacter,
            ),
            (
                record(world, Uuid::new_v4()),
                key(),
                world,
                None,
                NOW + LIFETIME + 1,
                Refusal::Expired,
            ),
        ];
        for (entry, signed_with, gateway_world, other_character, now, refusal) in cases {
            let (store, mut claims) = setup(&entry, &signed_with);
            if let Some(character) = other_character {
                claims.character_id = character.to_string();
            }
            assert_eq!(
                check_ticket(&store, &key(), gateway_world, &claims, now).await,
                Err(refusal)
            );
            assert_eq!(
                check_ticket(&store, &key(), gateway_world, &claims, now).await,
                Err(Refusal::UnknownTicket)
            );
        }
    }

    #[tokio::test]
    async fn a_store_failure_refuses() {
        let world = Uuid::new_v4();
        let (mut store, claims) = setup(&record(world, Uuid::new_v4()), &key());
        store.down = true;
        assert_eq!(
            check_ticket(&store, &key(), world, &claims, NOW).await,
            Err(Refusal::StoreUnavailable)
        );
    }

    #[test]
    fn an_accepted_response_has_the_captured_shape() {
        let now = UNIX_EPOCH + Duration::from_secs(NEW_YEAR_2026);
        let accepted = response(STATUS_AUTHORIZED, now, VERSION).expect("response");
        assert_eq!(accepted.uint32_at_0x8, STATUS_AUTHORIZED);
        assert_eq!(accepted.uint64, NEW_YEAR_2026_CLOCK);
        assert_eq!(accepted.bytes_at_0x18.len(), 2 * SESSION_TOKEN_BYTES);
        assert!(
            accepted
                .bytes_at_0x18
                .iter()
                .all(|byte| byte.is_ascii_digit() || byte.is_ascii_uppercase())
        );
        assert_eq!(accepted.bytes_at_0x38, VERSION.as_bytes());
        assert_eq!(accepted.bytes_at_0x58, OBSERVED_RESPONSE_FLAGS);
        let refused = response(STATUS_REFUSED, now, VERSION).expect("refused");
        assert!(refused.bytes_at_0x18.is_empty());
        assert_eq!(clock_nanos(UNIX_EPOCH), 0);
    }

    #[test]
    fn claims_come_from_the_token_and_logs_hide_the_ticket() {
        let token = LoginToken {
            ticket_id: TICKET.as_bytes().to_vec(),
            signature: b"abc".to_vec(),
            character_id: b"character".to_vec(),
            ..LoginToken::default()
        };
        let claims = TicketClaims::from_token(&token);
        assert_eq!(claims.ticket_id, TICKET);
        assert_eq!(claims.signature, "abc");
        assert_eq!(claims.masked_ticket(), "8d0f4c2e…");
    }
}
