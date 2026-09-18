// SPDX-License-Identifier: AGPL-3.0-or-later
//! The login messages that `sim` sends after a join, in the captured order (M4 spec section
//! 5.2): 446, 1034, 1628, 164, 1617, 335, 4246, 4247. Message 1628
//! (`PlayerManagerSelfIdentificationMsg`) is the one that moves the real client out of
//! `WaitingForActorGameConnection`; this server sends its smallest accepted body (see
//! [`PlayerManagerSelfIdentificationMsg`]). Message 3236 (still open layout) is not sent. The
//! `LevelInfoChangedMsg` (1635) names the sim's level: the tutorial or the open world
//! (new-player-start note); [`level_change_messages`] moves a player into another level. The
//! phasing update (6310) follows the client's acknowledgement of the level (`world::level_flow`).
//!
//! Most id fields hold a value this server chooses; the meaning of most of these ids in the
//! retail service is Unknown. One choice is ours alone: the entry id of `ClientAddEntryMsg` and
//! the first presence of `PlayerManagerSelfIdentificationMsg` hold the player's entity number, the
//! same 16 bytes as the spawn-card GdeRef (local-player-check note). The time messages are facet
//! calls of the player's time component: their ids are its [`FacetHeader`] (tutorial-intro note,
//! section 0), else the client drops them.

use nw_messages::{
    Block1415acc30, CalendarConnectedMsg, ClientAddEntryMsg, ConnectedBody, FacetHeader,
    LOGIN_CLIENT_CONTEXT, LevelInfoChangedMsg, LevelSpec, MessageError, OBSERVED_DAY_PHASE,
    OBSERVED_TIME_CHANGE_F32S_AT_16, OBSERVED_TIME_CHANGE_F32S_AT_60,
    OBSERVED_TIME_CHANGE_U32S_AT_28, PlayerManagerSelfIdentificationMsg, RAW_ID_LEN,
    ReceivePlayerSpawnPointMsg, ServerMessage, SyncDayPhaseMsg, SyncTimeChangeMsg, TUTORIAL_LEVEL,
    TimeSynchMsg, TimingConfigurationConnectedMsg,
};

/// The phase id of the single open-world phasing entry. It must not be 0 (the client drops a
/// 0-phase entry); one phase is used for every player of this server.
const OPEN_WORLD_PHASE_ID: u64 = 1;

/// The phase id of a player entering `level`: the shared open-world phase, or a new random
/// private instance for the tutorial (each tutorial player is alone, as in retail).
///
/// # Errors
///
/// Returns the `getrandom` error when the system has no random bytes.
pub fn phase_id(level: LevelSpec) -> Result<u64, getrandom::Error> {
    if level == TUTORIAL_LEVEL {
        // A phase id must not be 0.
        Ok(random_u64()?.max(1))
    } else {
        Ok(OPEN_WORLD_PHASE_ID)
    }
}

/// Ids that stay the same for every player of one `sim` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldIds {
    /// First block of the timing configuration message.
    pub timing_block: Block1415acc30,
    /// First block of the calendar message.
    pub calendar_block: Block1415acc30,
    /// The 16 bytes that both classes share in their second block.
    pub shared_session_bytes: [u8; RAW_ID_LEN],
    /// The time of `SyncTimeChange`: the same at every login in the captures.
    pub time_change_time: u64,
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
            time_change_time: start_time,
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

/// The `LevelInfoChangedMsg` of a login into `level`, in phase `phase_id`.
pub fn login_level_info(level: LevelSpec, server_time: u64, phase_id: u64) -> LevelInfoChangedMsg {
    LevelInfoChangedMsg::of_level(level, LOGIN_CLIENT_CONTEXT, server_time, phase_id)
}

/// The login messages for one player, as envelopes, in the captured order, ending with
/// `level_info`.
///
/// # Errors
///
/// Returns [`MessageError`] when a message cannot be encoded.
pub fn login_messages(
    world: &WorldIds,
    session: SessionIds,
    entity: u64,
    server_time: u64,
    level_info: &LevelInfoChangedMsg,
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
        ServerMessage::SelfIdentification(PlayerManagerSelfIdentificationMsg::for_entity(entity)),
        ServerMessage::ClientAddEntry(ClientAddEntryMsg::for_entity(entity)),
        ServerMessage::PlayerSpawnPoint(ReceivePlayerSpawnPointMsg),
        ServerMessage::TimeSynch(TimeSynchMsg::at_server_ms(server_time)),
        ServerMessage::LevelInfoChanged(level_info.clone()),
    ];
    messages.iter().map(ServerMessage::envelope).collect()
}

/// The answer to the client's time request (4248) with header `time`: the time change, then the
/// day phase. Sent with the login, they reached the client before its player existed and it
/// dropped them, so its clock stayed unset (live 2026-09-16: the potion cooldown showed the date).
pub fn time_answers(world: &WorldIds, time: FacetHeader) -> [ServerMessage; 2] {
    [
        ServerMessage::TimeChange(SyncTimeChangeMsg {
            ids: [time.target, time.entity],
            f32s_at_16: OBSERVED_TIME_CHANGE_F32S_AT_16,
            u32s_at_28: OBSERVED_TIME_CHANGE_U32S_AT_28,
            time: world.time_change_time,
            f32s_at_60: OBSERVED_TIME_CHANGE_F32S_AT_60,
        }),
        ServerMessage::DayPhase(SyncDayPhaseMsg {
            component: time.target,
            u64_at_8: time.entity,
            u32_at_16: OBSERVED_DAY_PHASE,
        }),
    ]
}

/// The messages that move a player's client into another level (retail end of the tutorial, in
/// the captured order without the facet calls): the entry and spawn-point messages again, then
/// `level_info`, whose client context every later bundle must carry.
///
/// # Errors
///
/// Returns [`MessageError`] when a message cannot be encoded.
pub fn level_change_messages(
    entity: u64,
    level_info: &LevelInfoChangedMsg,
) -> Result<Vec<Vec<u8>>, MessageError> {
    let messages = [
        ServerMessage::ClientAddEntry(ClientAddEntryMsg::for_entity(entity)),
        ServerMessage::PlayerSpawnPoint(ReceivePlayerSpawnPointMsg),
        ServerMessage::LevelInfoChanged(level_info.clone()),
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
    use nw_messages::facet::component;
    use nw_messages::{BodyCodec, LOGIN_MESSAGE_TYPES, OPEN_WORLD_LEVEL};
    use nw_reflect::Envelope;

    use super::*;

    const ENTITY: u64 = 0x0102_0304_0506_0708;
    const SERVER_TIME: u64 = 99;
    const START_TIME: u64 = 5;

    #[test]
    fn the_login_messages_come_in_the_captured_order() {
        let world = WorldIds::random(START_TIME).expect("world");
        let session = SessionIds::random().expect("session");
        let level_info = login_level_info(OPEN_WORLD_LEVEL, SERVER_TIME, 1);
        let envelopes =
            login_messages(&world, session, ENTITY, SERVER_TIME, &level_info).expect("messages");
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
        let ident_slot = LOGIN_MESSAGE_TYPES
            .iter()
            .position(|&type_index| type_index == PlayerManagerSelfIdentificationMsg::TYPE_INDEX)
            .expect("the identification message is in the login order");
        let ident = Envelope::decode(&envelopes[ident_slot]).expect("ident");
        let Some(Ok(ServerMessage::SelfIdentification(ident))) =
            ServerMessage::from_envelope(&ident)
        else {
            panic!("expected the identification message");
        };
        assert_eq!(ident.character_id, message.entry_id);
    }

    #[test]
    fn the_time_answers_call_the_asking_time_component_in_order() {
        let world = WorldIds::random(START_TIME).expect("world");
        let time = FacetHeader::of(component::TIME, ENTITY);
        let [
            ServerMessage::TimeChange(change),
            ServerMessage::DayPhase(phase),
        ] = time_answers(&world, time)
        else {
            panic!("the time change, then the day phase");
        };
        assert_eq!(change.ids, [time.target, time.entity]);
        assert_eq!(change.time, world.time_change_time);
        assert_eq!(
            (phase.component, phase.u64_at_8),
            (time.target, time.entity)
        );
    }

    #[test]
    fn both_connected_messages_share_the_session_block() {
        let world = WorldIds::random(START_TIME).expect("world");
        let session = SessionIds::random().expect("session");
        let level_info = login_level_info(OPEN_WORLD_LEVEL, SERVER_TIME, 1);
        let envelopes =
            login_messages(&world, session, ENTITY, SERVER_TIME, &level_info).expect("messages");
        let second_block = |bytes: &[u8]| match ServerMessage::from_envelope(
            &Envelope::decode(bytes).expect("envelope"),
        ) {
            Some(Ok(ServerMessage::TimingConfigurationConnected(message))) => message.0.block_at_36,
            Some(Ok(ServerMessage::CalendarConnected(message))) => message.0.block_at_36,
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(second_block(&envelopes[0]), second_block(&envelopes[1]));
    }

    fn level_info_of(envelopes: &[Vec<u8>]) -> LevelInfoChangedMsg {
        envelopes
            .iter()
            .find_map(|bytes| {
                match ServerMessage::from_envelope(&Envelope::decode(bytes).expect("envelope")) {
                    Some(Ok(ServerMessage::LevelInfoChanged(message))) => Some(message),
                    _ => None,
                }
            })
            .expect("a level message")
    }

    #[test]
    fn a_tutorial_login_names_the_tutorial_in_a_private_phase() {
        let world = WorldIds::random(START_TIME).expect("world");
        let session = SessionIds::random().expect("session");
        let phase = phase_id(TUTORIAL_LEVEL).expect("phase");
        assert_ne!(phase, 0);
        assert_eq!(
            phase_id(OPEN_WORLD_LEVEL).expect("phase"),
            OPEN_WORLD_PHASE_ID
        );
        let level_info = login_level_info(TUTORIAL_LEVEL, SERVER_TIME, phase);
        let envelopes =
            login_messages(&world, session, ENTITY, SERVER_TIME, &level_info).expect("messages");
        let level = level_info_of(&envelopes);
        assert_eq!(level.level, TUTORIAL_LEVEL);
        assert_eq!(level.client_context, LOGIN_CLIENT_CONTEXT);
        assert_eq!(level.phasing[0].phase_id, phase);
    }

    #[test]
    fn a_level_change_names_the_new_level_with_the_next_context() {
        const NEXT_CONTEXT: u8 = LOGIN_CLIENT_CONTEXT + 1;
        let level_info =
            LevelInfoChangedMsg::of_level(OPEN_WORLD_LEVEL, NEXT_CONTEXT, SERVER_TIME, 1);
        let envelopes = level_change_messages(ENTITY, &level_info).expect("messages");
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
            vec![
                ClientAddEntryMsg::TYPE_INDEX,
                ReceivePlayerSpawnPointMsg::TYPE_INDEX,
                LevelInfoChangedMsg::TYPE_INDEX
            ]
        );
        let level = level_info_of(&envelopes);
        assert_eq!(
            (level.level, level.client_context),
            (OPEN_WORLD_LEVEL, NEXT_CONTEXT)
        );
    }
}
