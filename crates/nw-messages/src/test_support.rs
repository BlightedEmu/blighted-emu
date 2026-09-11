// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sample messages for tests (feature `test-util`).

use crate::bundle::{LOCAL_PLAYER_INTEREST, ReplicatedStateBundle};
use crate::client_add_entry::ClientAddEntryMsg;
use crate::connected::{
    Block1415acc30, CalendarConnectedMsg, ConnectedBody, TimingConfigurationConnectedMsg,
};
use crate::day_phase::SyncDayPhaseMsg;
use crate::gde::{AssetId, GDE_METADATA_COMPONENT, GdeMetadataReplicatedState, PLAYER_SLICE_ASSET};
use crate::level_info::LevelInfoChangedMsg;
use crate::payload::{BundleGroup, BundleMember, MemberState};
use crate::position::{PositionInTheWorldReplicatedState, WorldPosition};
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
        ServerMessage::SelfIdentification(PlayerManagerSelfIdentificationMsg::for_entity(own)),
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
            component: 10,
            u64_at_8: 11,
            u32_at_16: 2,
        }),
        ServerMessage::LevelInfoChanged(LevelInfoChangedMsg::open_world(1, 12, 1)),
    ]
}

/// The first slot of another entity in [`players_bundle`].
pub const FIRST_OTHER_SLOT: u16 = 3;
/// The component of the position member in [`entity_group`].
const POSITION_COMPONENT: u64 = 2;

/// The group of entity `number` in slot `interest`: its card of `card` (when given), then its
/// position (when given).
pub fn entity_group(
    interest: u16,
    number: u64,
    card: Option<AssetId>,
    position: Option<WorldPosition>,
) -> BundleGroup {
    let card = card.map(|asset| BundleMember {
        component: GDE_METADATA_COMPONENT,
        state: MemberState::GdeMetadata(GdeMetadataReplicatedState::for_entity(asset, number)),
    });
    let position = position.map(|position| BundleMember {
        component: POSITION_COMPONENT,
        state: MemberState::Position(PositionInTheWorldReplicatedState {
            position: Some(position),
            ..PositionInTheWorldReplicatedState::default()
        }),
    });
    BundleGroup {
        interest: u64::from(interest),
        members: card.into_iter().chain(position).collect(),
    }
}

/// A bundle of players at `positions`, each with its card: `own` in slot 1, every other one in
/// the next slot from [`FIRST_OTHER_SLOT`] up, in order.
pub fn players_bundle(
    sequence: u64,
    own: u64,
    positions: &[(u64, WorldPosition)],
) -> ReplicatedStateBundle {
    let mut next = FIRST_OTHER_SLOT;
    let groups = positions
        .iter()
        .map(|(number, position)| {
            let interest = if *number == own {
                LOCAL_PLAYER_INTEREST
            } else {
                next += 1;
                next - 1
            };
            entity_group(interest, *number, Some(PLAYER_SLICE_ASSET), Some(*position))
        })
        .collect();
    ReplicatedStateBundle::of_groups(sequence, Vec::new(), groups).expect("bundle")
}
