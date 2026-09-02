// SPDX-License-Identifier: AGPL-3.0-or-later
//! Login queue state.
//!
//! Position lives in this state. Valkey is the shared copy.
//! Admission uses the configured capacity. `world-director` is a later program.
//! On admission the server writes a one-time entry ticket with a 60 second lifetime.

use std::collections::VecDeque;

use nw_protocol::{LoginQueueRequest, LoginQueueResponse, LoginQueueToken};
use serde::{Deserialize, Serialize};

use crate::request::{MAX_CAPABILITY_COUNT, MAX_CAPABILITY_LEN, MAX_ID_LEN, MAX_STEAM_TICKET_LEN};
use crate::signature::{SignatureError, entry_signature};

/// Lifetime of a one-time entry ticket. Source: architecture spec section 4.2.
pub(crate) const ENTRY_TICKET_TTL_SECONDS: i64 = 60;

/// First waiting player. The client's zero point for this number is not in the note.
const FIRST_QUEUE_POSITION: i64 = 1;

/// This server does not offer a queue transfer.
const ALLOW_QUEUE_TRANSFER: bool = false;

/// How many times a Valkey update retries after a lost race.
pub(crate) const VALKEY_UPDATE_ATTEMPTS: u32 = 8;

/// Queue limits and the gateway address returned after admission.
#[derive(Debug, Clone)]
pub(crate) struct QueueSettings {
    /// World this process serves.
    pub world_id: String,
    /// `QueueName` text.
    pub queue_name: String,
    /// How many admitted entry tickets may be live at once.
    /// Zero means every player waits.
    pub capacity: u32,
    /// `RefreshInterval` in seconds.
    pub refresh_interval_seconds: i64,
    /// Multiplied by the position to make `EstimatedTime`.
    pub estimated_seconds_per_position: i64,
    /// `Token.RepAddress`. One string. This server does not split it.
    pub rep_address: String,
    /// Key for `Token.Signature`.
    pub signature_key: Vec<u8>,
}

/// A join request that passed field checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PreparedJoin {
    /// Character id, when the client sent one.
    pub character_id: Option<String>,
    /// Capability strings from the request. They are not copied onto `Token`.
    pub client_capabilities: Option<Vec<String>>,
    /// Trial flag, when the client sent one.
    pub is_trial_owner: Option<bool>,
    /// Steam application id, when the client sent one.
    pub steam_app_id: Option<i64>,
    /// Steam auth ticket, when the client sent one.
    pub steam_auth_ticket: Option<String>,
    /// World id. This is the server world when the client omitted it.
    pub world_id: String,
}

/// Failure inside the queue.
#[derive(Debug, thiserror::Error)]
pub(crate) enum QueueError {
    /// The request named a different world.
    #[error("world id does not match this server")]
    WorldMismatch,
    /// A request field is empty or too long.
    #[error("login queue request is not valid")]
    InvalidRequest,
    /// The ticket id is not in the queue.
    #[error("login ticket was not found")]
    UnknownTicket,
    /// The signature key was rejected.
    #[error("could not sign the entry ticket")]
    Signature(#[from] SignatureError),
    /// Valkey or the in-memory lock did not complete the update.
    #[error("login queue store is unavailable")]
    Unavailable(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Result of one queue change, including tickets the store must write or delete.
#[derive(Debug)]
pub(crate) struct Transition {
    /// Reply, or [`QueueError::UnknownTicket`] after the state was updated.
    pub outcome: Result<LoginQueueResponse, QueueError>,
    /// Entry keys to delete.
    pub expired_ticket_ids: Vec<String>,
    /// Entry keys to write.
    pub new_entries: Vec<EntryTicket>,
}

/// One-time entry ticket stored beside the queue state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EntryTicket {
    /// Ticket id.
    pub ticket_id: String,
    /// JSON record for the gateway.
    pub payload: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct QueueEntry {
    ticket_id: String,
    join: PreparedJoin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AdmittedEntry {
    entry: QueueEntry,
    expires_at_unix: i64,
    signature: String,
}

/// Waiting players and live entry tickets for one world.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WorldQueue {
    waiting: VecDeque<QueueEntry>,
    admitted: Vec<AdmittedEntry>,
}

#[derive(Debug, Serialize)]
struct EntryRecord<'a> {
    ticket_id: &'a str,
    world_id: &'a str,
    character_id: Option<&'a str>,
    rep_address: &'a str,
    signature: &'a str,
    expires_at_unix: i64,
}

/// Checks the request against this server's world and length limits.
pub(crate) fn prepare_join(
    request: LoginQueueRequest,
    settings: &QueueSettings,
) -> Result<PreparedJoin, QueueError> {
    let world_id = match request.world_id {
        None => settings.world_id.clone(),
        Some(world) if world == settings.world_id => world,
        Some(world) if world.is_empty() || world.len() > MAX_ID_LEN => {
            return Err(QueueError::InvalidRequest);
        }
        Some(_) => return Err(QueueError::WorldMismatch),
    };
    let character_id = optional_id(request.character_id)?;
    let steam_auth_ticket = match request.steam_auth_ticket {
        None => None,
        Some(ticket) if ticket.is_empty() => None,
        Some(ticket) if ticket.len() > MAX_STEAM_TICKET_LEN => {
            return Err(QueueError::InvalidRequest);
        }
        Some(ticket) => Some(ticket),
    };
    let client_capabilities = check_capabilities(request.client_capabilities)?;
    Ok(PreparedJoin {
        character_id,
        client_capabilities,
        is_trial_owner: request.is_trial_owner,
        steam_app_id: request.steam_app_id,
        steam_auth_ticket,
        world_id,
    })
}

/// Adds a player, then admits from the front while capacity remains.
pub(crate) fn apply_join(
    state: &mut WorldQueue,
    prepared: PreparedJoin,
    new_ticket_id: &str,
    now_unix: i64,
    settings: &QueueSettings,
) -> Result<Transition, QueueError> {
    let expired_ticket_ids = expire(state, now_unix);
    if let Some(ticket_id) = find_steam(state, prepared.steam_auth_ticket.as_deref()) {
        let new_entries = admit(state, now_unix, settings)?;
        let outcome = response_for(state, &ticket_id, settings);
        return Ok(Transition {
            outcome,
            expired_ticket_ids,
            new_entries,
        });
    }
    let ticket_id = new_ticket_id.to_owned();
    state.waiting.push_back(QueueEntry {
        ticket_id: ticket_id.clone(),
        join: prepared,
    });
    let new_entries = admit(state, now_unix, settings)?;
    let outcome = response_for(state, &ticket_id, settings);
    Ok(Transition {
        outcome,
        expired_ticket_ids,
        new_entries,
    })
}

/// Refreshes one ticket and admits from the front while capacity remains.
pub(crate) fn apply_refresh(
    state: &mut WorldQueue,
    ticket_id: &str,
    now_unix: i64,
    settings: &QueueSettings,
) -> Result<Transition, QueueError> {
    let expired_ticket_ids = expire(state, now_unix);
    let new_entries = admit(state, now_unix, settings)?;
    let outcome = response_for(state, ticket_id, settings);
    Ok(Transition {
        outcome,
        expired_ticket_ids,
        new_entries,
    })
}

fn optional_id(value: Option<String>) -> Result<Option<String>, QueueError> {
    match value {
        None => Ok(None),
        Some(id) if id.is_empty() || id.len() > MAX_ID_LEN => Err(QueueError::InvalidRequest),
        Some(id) => Ok(Some(id)),
    }
}

fn check_capabilities(value: Option<Vec<String>>) -> Result<Option<Vec<String>>, QueueError> {
    let Some(items) = value else {
        return Ok(None);
    };
    if items.len() > MAX_CAPABILITY_COUNT {
        return Err(QueueError::InvalidRequest);
    }
    if items
        .iter()
        .any(|item| item.is_empty() || item.len() > MAX_CAPABILITY_LEN)
    {
        return Err(QueueError::InvalidRequest);
    }
    Ok(Some(items))
}

fn expire(state: &mut WorldQueue, now_unix: i64) -> Vec<String> {
    let mut removed = Vec::new();
    state.admitted.retain(|entry| {
        if entry.expires_at_unix <= now_unix {
            removed.push(entry.entry.ticket_id.clone());
            false
        } else {
            true
        }
    });
    removed
}

fn admit(
    state: &mut WorldQueue,
    now_unix: i64,
    settings: &QueueSettings,
) -> Result<Vec<EntryTicket>, QueueError> {
    let mut created = Vec::new();
    while has_room(state.admitted.len(), settings.capacity) {
        let Some(entry) = state.waiting.front().cloned() else {
            break;
        };
        let expires_at_unix = now_unix.saturating_add(ENTRY_TICKET_TTL_SECONDS);
        let signature = entry_signature(
            &settings.signature_key,
            &entry.ticket_id,
            &entry.join.world_id,
            entry.join.character_id.as_deref(),
            expires_at_unix,
        )?;
        let payload = serde_json::to_string(&EntryRecord {
            ticket_id: &entry.ticket_id,
            world_id: &entry.join.world_id,
            character_id: entry.join.character_id.as_deref(),
            rep_address: &settings.rep_address,
            signature: &signature,
            expires_at_unix,
        })
        .map_err(|error| QueueError::Unavailable(Box::new(error)))?;
        state.waiting.pop_front();
        created.push(EntryTicket {
            ticket_id: entry.ticket_id.clone(),
            payload,
        });
        state.admitted.push(AdmittedEntry {
            entry,
            expires_at_unix,
            signature,
        });
    }
    Ok(created)
}

fn has_room(admitted: usize, capacity: u32) -> bool {
    u32::try_from(admitted)
        .map(|count| count < capacity)
        .unwrap_or(false)
}

fn find_steam(state: &WorldQueue, steam: Option<&str>) -> Option<String> {
    let steam = steam?;
    state
        .waiting
        .iter()
        .find(|entry| entry.join.steam_auth_ticket.as_deref() == Some(steam))
        .map(|entry| entry.ticket_id.clone())
        .or_else(|| {
            state
                .admitted
                .iter()
                .find(|entry| entry.entry.join.steam_auth_ticket.as_deref() == Some(steam))
                .map(|entry| entry.entry.ticket_id.clone())
        })
}

fn response_for(
    state: &WorldQueue,
    ticket_id: &str,
    settings: &QueueSettings,
) -> Result<LoginQueueResponse, QueueError> {
    if let Some(admitted) = state
        .admitted
        .iter()
        .find(|entry| entry.entry.ticket_id == ticket_id)
    {
        return Ok(admitted_response(admitted, settings));
    }
    if let Some((index, entry)) = state
        .waiting
        .iter()
        .enumerate()
        .find(|(_, entry)| entry.ticket_id == ticket_id)
    {
        let position = i64::try_from(index)
            .unwrap_or(i64::MAX)
            .saturating_add(FIRST_QUEUE_POSITION);
        return Ok(waiting_response(entry, position, settings));
    }
    Err(QueueError::UnknownTicket)
}

fn waiting_response(
    entry: &QueueEntry,
    position: i64,
    settings: &QueueSettings,
) -> LoginQueueResponse {
    LoginQueueResponse {
        allow_queue_transfer: Some(ALLOW_QUEUE_TRANSFER),
        estimated_time: Some(position.saturating_mul(settings.estimated_seconds_per_position)),
        position: Some(position),
        queue_name: Some(settings.queue_name.clone()),
        recommended_transfer_world_id: None,
        refresh_interval: Some(settings.refresh_interval_seconds),
        ticket_id: Some(entry.ticket_id.clone()),
        token: Some(token_for(&entry.join, &entry.ticket_id, None, None)),
    }
}

fn admitted_response(admitted: &AdmittedEntry, settings: &QueueSettings) -> LoginQueueResponse {
    LoginQueueResponse {
        allow_queue_transfer: Some(ALLOW_QUEUE_TRANSFER),
        estimated_time: None,
        position: None,
        queue_name: Some(settings.queue_name.clone()),
        recommended_transfer_world_id: None,
        refresh_interval: Some(settings.refresh_interval_seconds),
        ticket_id: Some(admitted.entry.ticket_id.clone()),
        token: Some(token_for(
            &admitted.entry.join,
            &admitted.entry.ticket_id,
            Some(settings.rep_address.clone()),
            Some(admitted.signature.clone()),
        )),
    }
}

/// Shared queue operations.
pub(crate) trait LoginQueue: Clone + Send + Sync + 'static {
    /// Adds a player.
    fn join(
        &self,
        prepared: PreparedJoin,
        now_unix: i64,
    ) -> impl std::future::Future<Output = Result<LoginQueueResponse, QueueError>> + Send;

    /// Reads one ticket again.
    fn refresh(
        &self,
        ticket_id: &str,
        now_unix: i64,
    ) -> impl std::future::Future<Output = Result<LoginQueueResponse, QueueError>> + Send;
}

fn token_for(
    join: &PreparedJoin,
    ticket_id: &str,
    rep_address: Option<String>,
    signature: Option<String>,
) -> LoginQueueToken {
    LoginQueueToken {
        character_id: join.character_id.clone(),
        is_trial_owner: join.is_trial_owner,
        rep_address,
        signature,
        steam_app_id: join.steam_app_id,
        ticket_id: Some(ticket_id.to_owned()),
        world_id: Some(join.world_id.clone()),
        ..LoginQueueToken::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORLD: &str = "world-1";
    const QUEUE_NAME: &str = "world-1";
    const REP_ADDRESS: &str = "127.0.0.1:33435";
    const NOW: i64 = 1_700_000_000;
    const REFRESH: i64 = 5;
    const SECONDS_PER_POSITION: i64 = 5;
    const CHARACTER: &str = "char-1";
    const STEAM: &str = "steam-ticket";

    fn settings(capacity: u32) -> QueueSettings {
        QueueSettings {
            world_id: WORLD.to_owned(),
            queue_name: QUEUE_NAME.to_owned(),
            capacity,
            refresh_interval_seconds: REFRESH,
            estimated_seconds_per_position: SECONDS_PER_POSITION,
            rep_address: REP_ADDRESS.to_owned(),
            signature_key: b"change-me-signature-key".to_vec(),
        }
    }

    fn join_request(character: bool, steam: bool) -> LoginQueueRequest {
        LoginQueueRequest {
            character_id: character.then(|| CHARACTER.to_owned()),
            client_capabilities: Some(vec!["move".to_owned()]),
            is_trial_owner: Some(false),
            steam_app_id: Some(1063730),
            steam_auth_ticket: steam.then(|| STEAM.to_owned()),
            world_id: Some(WORLD.to_owned()),
        }
    }

    #[test]
    fn capacity_admits_the_first_player_and_holds_the_second() {
        let settings = settings(1);
        let mut state = WorldQueue::default();
        let first = apply_join(
            &mut state,
            prepare_join(join_request(true, true), &settings).expect("prepare"),
            "ticket-a",
            NOW,
            &settings,
        )
        .expect("join");
        let reply = first.outcome.expect("admitted");
        let token = reply.token.expect("token");
        assert!(reply.position.is_none());
        assert_eq!(token.rep_address.as_deref(), Some(REP_ADDRESS));
        assert!(
            token
                .signature
                .as_deref()
                .is_some_and(|value| !value.is_empty())
        );
        assert!(token.client_capabilities.is_none());
        assert_eq!(token.character_id.as_deref(), Some(CHARACTER));
        assert_eq!(first.new_entries.len(), 1);

        let second = apply_join(
            &mut state,
            prepare_join(join_request(false, false), &settings).expect("prepare"),
            "ticket-b",
            NOW,
            &settings,
        )
        .expect("join");
        let waiting = second.outcome.expect("waiting");
        assert_eq!(waiting.position, Some(FIRST_QUEUE_POSITION));
        assert_eq!(
            waiting.estimated_time,
            Some(FIRST_QUEUE_POSITION * SECONDS_PER_POSITION)
        );
        let waiting_token = waiting.token.expect("token");
        assert!(waiting_token.rep_address.is_none());
        assert!(waiting_token.signature.is_none());
        assert_eq!(waiting_token.ticket_id.as_deref(), Some("ticket-b"));
    }

    #[test]
    fn same_steam_ticket_keeps_one_place() {
        let settings = settings(0);
        let mut state = WorldQueue::default();
        let prepared = prepare_join(join_request(false, true), &settings).expect("prepare");
        apply_join(&mut state, prepared.clone(), "ticket-a", NOW, &settings).expect("join");
        let again = apply_join(&mut state, prepared, "ticket-b", NOW, &settings).expect("join");
        let token = again.outcome.expect("reply").token.expect("token");
        assert_eq!(token.ticket_id.as_deref(), Some("ticket-a"));
        assert_eq!(state.waiting.len(), 1);
    }

    #[test]
    fn refresh_after_expiry_admits_the_next_player() {
        let settings = settings(1);
        let mut state = WorldQueue::default();
        apply_join(
            &mut state,
            prepare_join(join_request(false, false), &settings).expect("prepare"),
            "ticket-a",
            NOW,
            &settings,
        )
        .expect("join");
        apply_join(
            &mut state,
            prepare_join(LoginQueueRequest::default(), &settings).expect("prepare"),
            "ticket-b",
            NOW,
            &settings,
        )
        .expect("join");
        let refreshed = apply_refresh(
            &mut state,
            "ticket-b",
            NOW + ENTRY_TICKET_TTL_SECONDS,
            &settings,
        )
        .expect("refresh");
        let token = refreshed.outcome.expect("admitted").token.expect("token");
        assert_eq!(token.ticket_id.as_deref(), Some("ticket-b"));
        assert_eq!(token.rep_address.as_deref(), Some(REP_ADDRESS));
        assert_eq!(refreshed.expired_ticket_ids, vec!["ticket-a".to_owned()]);
    }

    #[test]
    fn unknown_ticket_is_an_error_and_other_world_is_rejected() {
        let settings = settings(1);
        let mut state = WorldQueue::default();
        let refreshed = apply_refresh(&mut state, "missing", NOW, &settings).expect("refresh");
        assert!(matches!(refreshed.outcome, Err(QueueError::UnknownTicket)));
        let mut request = join_request(false, false);
        request.world_id = Some("other-world".to_owned());
        let error = prepare_join(request, &settings).expect_err("world");
        assert!(matches!(error, QueueError::WorldMismatch));
    }
}
