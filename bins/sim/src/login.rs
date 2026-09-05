// SPDX-License-Identifier: AGPL-3.0-or-later
//! The login messages that `sim` sends after a join, in the captured order (M4 spec section
//! 5.2): 446, 1034, 164, 1617, 335, 4246, 4247. Messages with an open layout (1628, 3236, 1635,
//! 6310) are not sent.
//!
//! Every id field holds a value this server chooses; no byte comes from a capture. The meaning of
//! most of these ids in the retail service is Unknown. One choice is ours alone: the 16-byte
//! entry id of `ClientAddEntryMsg` holds the player's entity number (8 zero bytes, then the
//! number as a big-endian `u64`), so a test client knows which bundle member it is.

use nw_messages::{
    Block1415acc30, CalendarConnectedMsg, ClientAddEntryMsg, ConnectedBody, MessageError,
    OBSERVED_DAY_PHASE, OBSERVED_TIME_CHANGE_F32S_AT_16, OBSERVED_TIME_CHANGE_F32S_AT_60,
    OBSERVED_TIME_CHANGE_U32S_AT_28, RAW_ID_LEN, ReceivePlayerSpawnPointMsg, ServerMessage,
    SyncDayPhaseMsg, SyncTimeChangeMsg, TimeSynchMsg, TimingConfigurationConnectedMsg,
};

/// Bytes of the entity number at the end of an entry id.
const ENTITY_NUMBER_LEN: usize = 8;

/// Ids that stay the same for every player of one `sim` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldIds {
    /// First block of the timing configuration message.
    pub timing_block: Block1415acc30,
    /// First block of the calendar message.
    pub calendar_block: Block1415acc30,
    /// The 16 bytes that both classes share in their second block.
    pub shared_session_bytes: [u8; RAW_ID_LEN],
    /// The two ids of `SyncTimeChange`.
    pub time_change_ids: [u64; 2],
    /// The time of `SyncTimeChange`: the same at every login in the captures.
    pub time_change_time: u64,
    /// The two values of `SyncDayPhase`.
    pub day_phase_ids: [u64; 2],
}

impl WorldIds {
    /// New random ids.
    ///
    /// # Errors
    ///
    /// Returns the `getrandom` error when the system has no random bytes.
    pub fn random(start_time: u64) -> Result<Self, getrandom::Error> {
        let block = || -> Result<Block1415acc30, getrandom::Error> {
            Ok(Block1415acc30 {
                value: random_u32()?,
                first: random_id()?,
                second: random_id()?,
            })
        };
        Ok(Self {
            timing_block: block()?,
            calendar_block: block()?,
            shared_session_bytes: random_id()?,
            time_change_ids: [random_u64()?, random_u64()?],
            time_change_time: start_time,
            day_phase_ids: [random_u64()?, random_u64()?],
        })
    }
}

/// Ids of one player session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionIds {
    /// The `u32` both classes share in their second block.
    pub value: u32,
    /// The 16 bytes both classes share in their second block.
    pub first: [u8; RAW_ID_LEN],
}

impl SessionIds {
    /// New random ids.
    ///
    /// # Errors
    ///
    /// Returns the `getrandom` error when the system has no random bytes.
    pub fn random() -> Result<Self, getrandom::Error> {
        Ok(Self {
            value: random_u32()?,
            first: random_id()?,
        })
    }
}

/// The entry id that carries `number` (see the module note).
pub fn entry_id(number: u64) -> [u8; RAW_ID_LEN] {
    let mut id = [0; RAW_ID_LEN];
    id[RAW_ID_LEN - ENTITY_NUMBER_LEN..].copy_from_slice(&number.to_be_bytes());
    id
}

/// The entity number in an entry id that [`entry_id`] made; `None` for another id.
pub fn entity_number(entry_id: &[u8; RAW_ID_LEN]) -> Option<u64> {
    let (zeros, number) = entry_id.split_at(RAW_ID_LEN - ENTITY_NUMBER_LEN);
    let number: [u8; ENTITY_NUMBER_LEN] = number.try_into().ok()?;
    zeros
        .iter()
        .all(|byte| *byte == 0)
        .then_some(u64::from_be_bytes(number))
}

/// The login messages for one player, as envelopes, in the captured order.
///
/// # Errors
///
/// Returns [`MessageError`] when a message cannot be encoded.
pub fn login_messages(
    world: &WorldIds,
    session: SessionIds,
    entity: u64,
    server_time: u64,
) -> Result<Vec<Vec<u8>>, MessageError> {
    let session_block = Block1415acc30 {
        value: session.value,
        first: session.first,
        second: world.shared_session_bytes,
    };
    let messages = [
        ServerMessage::TimingConfigurationConnected(TimingConfigurationConnectedMsg(
            ConnectedBody {
                block_at_0: world.timing_block,
                block_at_36: session_block,
            },
        )),
        ServerMessage::CalendarConnected(CalendarConnectedMsg(ConnectedBody {
            block_at_0: world.calendar_block,
            block_at_36: session_block,
        })),
        ServerMessage::ClientAddEntry(ClientAddEntryMsg {
            entry_id: entry_id(entity),
        }),
        ServerMessage::PlayerSpawnPoint(ReceivePlayerSpawnPointMsg),
        ServerMessage::TimeSynch(TimeSynchMsg { time: server_time }),
        ServerMessage::TimeChange(SyncTimeChangeMsg {
            ids: world.time_change_ids,
            f32s_at_16: OBSERVED_TIME_CHANGE_F32S_AT_16,
            u32s_at_28: OBSERVED_TIME_CHANGE_U32S_AT_28,
            time: world.time_change_time,
            f32s_at_60: OBSERVED_TIME_CHANGE_F32S_AT_60,
        }),
        ServerMessage::DayPhase(SyncDayPhaseMsg {
            u64_at_0: world.day_phase_ids[0],
            u64_at_8: world.day_phase_ids[1],
            u32_at_16: OBSERVED_DAY_PHASE,
        }),
    ];
    messages.iter().map(ServerMessage::envelope).collect()
}

fn random_id() -> Result<[u8; RAW_ID_LEN], getrandom::Error> {
    let mut id = [0; RAW_ID_LEN];
    getrandom::fill(&mut id)?;
    Ok(id)
}

fn random_u32() -> Result<u32, getrandom::Error> {
    getrandom::u32()
}

fn random_u64() -> Result<u64, getrandom::Error> {
    getrandom::u64()
}

#[cfg(test)]
mod tests {
    use nw_messages::{
        CALENDAR_CONNECTED_MSG_TYPE_INDEX, CLIENT_ADD_ENTRY_MSG_TYPE_INDEX,
        RECEIVE_PLAYER_SPAWN_POINT_MSG_TYPE_INDEX, SYNC_DAY_PHASE_MSG_TYPE_INDEX,
        SYNC_TIME_CHANGE_MSG_TYPE_INDEX, TIMING_CONFIGURATION_CONNECTED_MSG_TYPE_INDEX,
    };
    use nw_reflect::{Envelope, TIME_SYNCH_MSG_TYPE_INDEX};

    use super::*;

    const ENTITY: u64 = 0x0102_0304_0506_0708;
    const SERVER_TIME: u64 = 99;
    const START_TIME: u64 = 5;

    #[test]
    fn the_login_messages_come_in_the_captured_order() {
        let world = WorldIds::random(START_TIME).expect("world");
        let session = SessionIds::random().expect("session");
        let envelopes = login_messages(&world, session, ENTITY, SERVER_TIME).expect("messages");
        let types: Vec<u64> = envelopes
            .iter()
            .map(|bytes| {
                Envelope::decode(bytes)
                    .expect("envelope")
                    .type_index()
                    .expect("slot")
            })
            .collect();
        assert_eq!(
            types,
            [
                TIMING_CONFIGURATION_CONNECTED_MSG_TYPE_INDEX,
                CALENDAR_CONNECTED_MSG_TYPE_INDEX,
                CLIENT_ADD_ENTRY_MSG_TYPE_INDEX,
                RECEIVE_PLAYER_SPAWN_POINT_MSG_TYPE_INDEX,
                TIME_SYNCH_MSG_TYPE_INDEX,
                SYNC_TIME_CHANGE_MSG_TYPE_INDEX,
                SYNC_DAY_PHASE_MSG_TYPE_INDEX,
            ]
        );
        let entry = Envelope::decode(&envelopes[2]).expect("entry");
        let Some(Ok(ServerMessage::ClientAddEntry(message))) = ServerMessage::from_envelope(&entry)
        else {
            panic!("expected the entry message");
        };
        assert_eq!(entity_number(&message.entry_id), Some(ENTITY));
    }

    #[test]
    fn both_connected_messages_share_the_session_block() {
        let world = WorldIds::random(START_TIME).expect("world");
        let session = SessionIds::random().expect("session");
        let envelopes = login_messages(&world, session, ENTITY, SERVER_TIME).expect("messages");
        let second_block = |bytes: &[u8]| match ServerMessage::from_envelope(
            &Envelope::decode(bytes).expect("envelope"),
        ) {
            Some(Ok(ServerMessage::TimingConfigurationConnected(message))) => message.0.block_at_36,
            Some(Ok(ServerMessage::CalendarConnected(message))) => message.0.block_at_36,
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(second_block(&envelopes[0]), second_block(&envelopes[1]));
    }

    #[test]
    fn an_id_that_is_not_an_entity_number_is_refused() {
        assert_eq!(entity_number(&entry_id(ENTITY)), Some(ENTITY));
        assert_eq!(entity_number(&[1; RAW_ID_LEN]), None);
    }
}
