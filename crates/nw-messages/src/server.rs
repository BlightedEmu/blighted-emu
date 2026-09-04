// SPDX-License-Identifier: AGPL-3.0-or-later
//! Every server message of M4 with a known layout (M4 spec section 5.2), by type index.

use crate::bundle::ReplicatedStateBundle;
use crate::client_add_entry::ClientAddEntryMsg;
use crate::connected::{CalendarConnectedMsg, TimingConfigurationConnectedMsg};
use crate::day_phase::SyncDayPhaseMsg;
use crate::spawn_point::ReceivePlayerSpawnPointMsg;
use crate::time_change::SyncTimeChangeMsg;
use crate::time_synch::TimeSynchMsg;
use crate::wire::{BodyCodec, MessageError};

/// Type indexes of [`ServerMessage`], in the order of the captured login.
pub const SERVER_MESSAGE_TYPES: [u64; 8] = [
    TimingConfigurationConnectedMsg::TYPE_INDEX,
    CalendarConnectedMsg::TYPE_INDEX,
    ClientAddEntryMsg::TYPE_INDEX,
    ReceivePlayerSpawnPointMsg::TYPE_INDEX,
    TimeSynchMsg::TYPE_INDEX,
    SyncTimeChangeMsg::TYPE_INDEX,
    SyncDayPhaseMsg::TYPE_INDEX,
    ReplicatedStateBundle::TYPE_INDEX,
];

/// One server message of M4.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerMessage {
    /// 446.
    TimingConfigurationConnected(TimingConfigurationConnectedMsg),
    /// 1034.
    CalendarConnected(CalendarConnectedMsg),
    /// 164.
    ClientAddEntry(ClientAddEntryMsg),
    /// 1617.
    PlayerSpawnPoint(ReceivePlayerSpawnPointMsg),
    /// 335.
    TimeSynch(TimeSynchMsg),
    /// 4246.
    TimeChange(SyncTimeChangeMsg),
    /// 4247.
    DayPhase(SyncDayPhaseMsg),
    /// 8.
    Bundle(ReplicatedStateBundle),
}

impl ServerMessage {
    /// Decodes `body` when `type_index` is one of [`SERVER_MESSAGE_TYPES`]; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let message = match type_index {
            TimingConfigurationConnectedMsg::TYPE_INDEX => {
                TimingConfigurationConnectedMsg::decode(body)
                    .map(Self::TimingConfigurationConnected)
            }
            CalendarConnectedMsg::TYPE_INDEX => {
                CalendarConnectedMsg::decode(body).map(Self::CalendarConnected)
            }
            ClientAddEntryMsg::TYPE_INDEX => {
                ClientAddEntryMsg::decode(body).map(Self::ClientAddEntry)
            }
            ReceivePlayerSpawnPointMsg::TYPE_INDEX => {
                ReceivePlayerSpawnPointMsg::decode(body).map(Self::PlayerSpawnPoint)
            }
            TimeSynchMsg::TYPE_INDEX => TimeSynchMsg::decode(body).map(Self::TimeSynch),
            SyncTimeChangeMsg::TYPE_INDEX => SyncTimeChangeMsg::decode(body).map(Self::TimeChange),
            SyncDayPhaseMsg::TYPE_INDEX => SyncDayPhaseMsg::decode(body).map(Self::DayPhase),
            ReplicatedStateBundle::TYPE_INDEX => {
                ReplicatedStateBundle::decode(body).map(Self::Bundle)
            }
            _ => return None,
        };
        Some(message)
    }

    /// The type index of this message.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::TimingConfigurationConnected(_) => TimingConfigurationConnectedMsg::TYPE_INDEX,
            Self::CalendarConnected(_) => CalendarConnectedMsg::TYPE_INDEX,
            Self::ClientAddEntry(_) => ClientAddEntryMsg::TYPE_INDEX,
            Self::PlayerSpawnPoint(_) => ReceivePlayerSpawnPointMsg::TYPE_INDEX,
            Self::TimeSynch(_) => TimeSynchMsg::TYPE_INDEX,
            Self::TimeChange(_) => SyncTimeChangeMsg::TYPE_INDEX,
            Self::DayPhase(_) => SyncDayPhaseMsg::TYPE_INDEX,
            Self::Bundle(_) => ReplicatedStateBundle::TYPE_INDEX,
        }
    }

    /// The body bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a value does not fit its field.
    pub fn encode(&self) -> Result<Vec<u8>, MessageError> {
        match self {
            Self::TimingConfigurationConnected(message) => message.encode(),
            Self::CalendarConnected(message) => message.encode(),
            Self::ClientAddEntry(message) => message.encode(),
            Self::PlayerSpawnPoint(message) => message.encode(),
            Self::TimeSynch(message) => message.encode(),
            Self::TimeChange(message) => message.encode(),
            Self::DayPhase(message) => message.encode(),
            Self::Bundle(message) => message.encode(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connected::{Block1415acc30, ConnectedBody};
    use crate::wire::RAW_ID_LEN;

    const OTHER_TYPE: u64 = 349;

    fn samples() -> Vec<ServerMessage> {
        let block = Block1415acc30 {
            value: 1,
            first: [2; RAW_ID_LEN],
            second: [3; RAW_ID_LEN],
        };
        let connected = ConnectedBody {
            block_at_0: block,
            block_at_36: block,
        };
        vec![
            ServerMessage::TimingConfigurationConnected(TimingConfigurationConnectedMsg(connected)),
            ServerMessage::CalendarConnected(CalendarConnectedMsg(connected)),
            ServerMessage::ClientAddEntry(ClientAddEntryMsg {
                entry_id: [4; RAW_ID_LEN],
            }),
            ServerMessage::PlayerSpawnPoint(ReceivePlayerSpawnPointMsg),
            ServerMessage::TimeSynch(TimeSynchMsg { time: 5 }),
            ServerMessage::TimeChange(SyncTimeChangeMsg {
                ids: [6, 7],
                f32s_at_16: [1.0; 3],
                u32s_at_28: [8; 6],
                time: 9,
                f32s_at_60: [0.5; 4],
            }),
            ServerMessage::DayPhase(SyncDayPhaseMsg {
                u64_at_0: 10,
                u64_at_8: 11,
                u32_at_16: 2,
            }),
            ServerMessage::Bundle(ReplicatedStateBundle {
                leading: None,
                byte_3: 0,
                byte_4: 1,
                flag_5: false,
                index_block: None,
                payload: vec![12; 3],
            }),
        ]
    }

    #[test]
    fn every_type_decodes_by_its_index_in_login_order() {
        let samples = samples();
        let order: Vec<u64> = samples.iter().map(ServerMessage::type_index).collect();
        assert_eq!(order, SERVER_MESSAGE_TYPES);
        for message in samples {
            let body = message.encode().expect("encode");
            assert_eq!(
                ServerMessage::decode(message.type_index(), &body),
                Some(Ok(message))
            );
        }
    }

    #[test]
    fn other_types_are_not_decoded() {
        assert_eq!(ServerMessage::decode(OTHER_TYPE, &[]), None);
    }
}
