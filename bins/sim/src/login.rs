// SPDX-License-Identifier: AGPL-3.0-or-later
//! The login messages that `sim` sends after a join, in the captured order (M4 spec section
//! 5.2): 446, 1034, 1628, 164, 1617, 335, 4246, 4247. Message 1628
//! (`PlayerManagerSelfIdentificationMsg`) is the one that moves the real client out of
//! `WaitingForActorGameConnection`; this server sends its smallest accepted body (see
//! [`PlayerManagerSelfIdentificationMsg`]). Messages 3236, 1635 and 6310 (still open layout) are
//! not sent.
//!
//! Every id field holds a value this server chooses; no byte comes from a capture. The meaning of
//! most of these ids in the retail service is Unknown. One choice is ours alone: the entry id of
//! `ClientAddEntryMsg` holds the player's entity number ([`ClientAddEntryMsg::for_entity`]), so a
//! test client knows which bundle member it is.

use nw_messages::{
    Block1415acc30, CalendarConnectedMsg, ClientAddEntryMsg, ConnectedBody, LevelInfoChangedMsg,
    MessageError, OBSERVED_DAY_PHASE, OBSERVED_TIME_CHANGE_F32S_AT_16,
    OBSERVED_TIME_CHANGE_F32S_AT_60, OBSERVED_TIME_CHANGE_U32S_AT_28,
    PlayerManagerSelfIdentificationMsg, RAW_ID_LEN, ReceivePlayerSpawnPointMsg, ServerMessage,
    SyncDayPhaseMsg, SyncTimeChangeMsg, TimeSynchMsg, TimingConfigurationConnectedMsg,
};

/// The `clientCtx` id of `LevelInfoChangedMsg`. The client stores it and expects every state
/// bundle that follows to carry the same number (bootstrap bodies note, type 1635).
const SESSION_CLIENT_CONTEXT: u8 = 1;
/// The phase id of the single open-world phasing entry. It must not be 0 (the client drops a
/// 0-phase entry); one phase is used for every player of this server.
const SESSION_PHASE_ID: u64 = 1;

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
        ServerMessage::SelfIdentification(PlayerManagerSelfIdentificationMsg),
        ServerMessage::ClientAddEntry(ClientAddEntryMsg::for_entity(entity)),
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
            component: world.day_phase_ids[0],
            u64_at_8: world.day_phase_ids[1],
            u32_at_16: OBSERVED_DAY_PHASE,
        }),
        ServerMessage::LevelInfoChanged(LevelInfoChangedMsg::open_world(
            SESSION_CLIENT_CONTEXT,
            server_time,
            SESSION_PHASE_ID,
        )),
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
    use nw_messages::{BodyCodec, LOGIN_MESSAGE_TYPES};
    use nw_reflect::Envelope;

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
        assert_eq!(types, LOGIN_MESSAGE_TYPES);
        let entry_slot = LOGIN_MESSAGE_TYPES
            .iter()
            .position(|&type_index| type_index == ClientAddEntryMsg::TYPE_INDEX)
            .expect("the entry message is in the login order");
        let entry = Envelope::decode(&envelopes[entry_slot]).expect("entry");
        let Some(Ok(ServerMessage::ClientAddEntry(message))) = ServerMessage::from_envelope(&entry)
        else {
            panic!("expected the entry message");
        };
        assert_eq!(message.entity_number(), Some(ENTITY));
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
}
