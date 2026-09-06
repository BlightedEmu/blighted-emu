// SPDX-License-Identifier: AGPL-3.0-or-later
//! Every server message of M4 with a known layout (M4 spec section 5.2), by type index.

use crate::bundle::ReplicatedStateBundle;
use crate::client_add_entry::ClientAddEntryMsg;
use crate::combat::{DamageDealtMsg, DeathRecapMsg, OnDamageMsg};
use crate::connected::{CalendarConnectedMsg, TimingConfigurationConnectedMsg};
use crate::day_phase::SyncDayPhaseMsg;
use crate::spawn_point::ReceivePlayerSpawnPointMsg;
use crate::time_change::SyncTimeChangeMsg;
use crate::time_synch::TimeSynchMsg;
use nw_reflect::{Envelope, TypeId};

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

/// Type indexes of the login messages, in the captured order: every type of
/// [`SERVER_MESSAGE_TYPES`] but the bundle, which comes after them.
pub const LOGIN_MESSAGE_TYPES: &[u64] = SERVER_MESSAGE_TYPES
    .split_at(SERVER_MESSAGE_TYPES.len() - 1)
    .0;

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
    /// 2071.
    DamageDealt(DamageDealtMsg),
    /// 3601.
    OnDamage(OnDamageMsg),
    /// 4299.
    DeathRecap(DeathRecapMsg),
}

impl ServerMessage {
    /// Decodes `body` when `type_index` is one of [`SERVER_MESSAGE_TYPES`] or a combat message
    /// of M6-B (2071, 3601, 4299); `None` otherwise.
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
            DamageDealtMsg::TYPE_INDEX => DamageDealtMsg::decode(body).map(Self::DamageDealt),
            OnDamageMsg::TYPE_INDEX => OnDamageMsg::decode(body).map(Self::OnDamage),
            DeathRecapMsg::TYPE_INDEX => DeathRecapMsg::decode(body).map(Self::DeathRecap),
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
            Self::DamageDealt(_) => DamageDealtMsg::TYPE_INDEX,
            Self::OnDamage(_) => OnDamageMsg::TYPE_INDEX,
            Self::DeathRecap(_) => DeathRecapMsg::TYPE_INDEX,
        }
    }

    /// The whole envelope: header, type index and body.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a value does not fit its field.
    pub fn envelope(&self) -> Result<Vec<u8>, MessageError> {
        Ok(Envelope::message(TypeId::FileSlot(self.type_index()), self.encode()?).encode()?)
    }

    /// The message in an envelope; `None` for an envelope of another type.
    pub fn from_envelope(envelope: &Envelope) -> Option<Result<Self, MessageError>> {
        Self::decode(envelope.type_index()?, &envelope.body)
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
            Self::DamageDealt(message) => message.encode(),
            Self::OnDamage(message) => message.encode(),
            Self::DeathRecap(message) => message.encode(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::login_samples;

    const ENTITY: u64 = 4;

    const OTHER_TYPE: u64 = 349;

    fn samples() -> Vec<ServerMessage> {
        let mut samples = login_samples(ENTITY);
        samples.push(ServerMessage::Bundle(ReplicatedStateBundle {
            leading: None,
            byte_3: 0,
            byte_4: 1,
            flag_5: false,
            index_block: None,
            payload: vec![12; 3],
        }));
        samples
    }

    #[test]
    fn every_type_decodes_by_its_index_in_login_order() {
        let samples = samples();
        let order: Vec<u64> = samples.iter().map(ServerMessage::type_index).collect();
        assert_eq!(order, SERVER_MESSAGE_TYPES);
        assert_eq!(
            LOGIN_MESSAGE_TYPES.last(),
            Some(&SyncDayPhaseMsg::TYPE_INDEX)
        );
        for message in samples {
            let body = message.encode().expect("encode");
            assert_eq!(
                ServerMessage::decode(message.type_index(), &body),
                Some(Ok(message))
            );
        }
    }

    #[test]
    fn combat_messages_decode_by_their_index() {
        let samples = [
            ServerMessage::DamageDealt(DamageDealtMsg::hit(ENTITY, ENTITY + 1, 5.0)),
            ServerMessage::OnDamage(OnDamageMsg::hit(ENTITY, ENTITY + 1, 5.0)),
            ServerMessage::DeathRecap(DeathRecapMsg::of(ENTITY, ENTITY + 1)),
        ];
        for message in samples {
            let body = message.encode().expect("encode");
            assert_eq!(
                ServerMessage::decode(message.type_index(), &body),
                Some(Ok(message))
            );
        }
    }

    #[test]
    fn a_message_round_trips_through_its_envelope() {
        for message in samples() {
            let bytes = message.envelope().expect("envelope");
            let envelope = Envelope::decode(&bytes).expect("decode");
            assert_eq!(ServerMessage::from_envelope(&envelope), Some(Ok(message)));
        }
    }

    #[test]
    fn other_types_are_not_decoded() {
        assert_eq!(ServerMessage::decode(OTHER_TYPE, &[]), None);
    }
}
