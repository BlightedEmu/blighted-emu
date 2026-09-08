// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sample messages for tests (feature `test-util`).

use crate::client_add_entry::ClientAddEntryMsg;
use crate::connected::{
    Block1415acc30, CalendarConnectedMsg, ConnectedBody, TimingConfigurationConnectedMsg,
};
use crate::day_phase::SyncDayPhaseMsg;
use crate::self_ident::PlayerManagerSelfIdentificationMsg;
use crate::server::ServerMessage;
use crate::spawn_point::ReceivePlayerSpawnPointMsg;
use crate::time_change::SyncTimeChangeMsg;
use crate::time_synch::TimeSynchMsg;
use crate::wire::RAW_ID_LEN;

/// One message of each login type, in the captured order; the entry names entity `own`.
pub fn login_samples(own: u64) -> Vec<ServerMessage> {
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
        ServerMessage::SelfIdentification(PlayerManagerSelfIdentificationMsg),
        ServerMessage::ClientAddEntry(ClientAddEntryMsg::for_entity(own)),
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
    ]
}
