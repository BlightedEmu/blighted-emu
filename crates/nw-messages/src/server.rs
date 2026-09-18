// SPDX-License-Identifier: AGPL-3.0-or-later
//! Every server message of M4 with a known layout (M4 spec section 5.2), by type index.

use crate::bundle::ReplicatedStateBundle;
use crate::client_add_entry::ClientAddEntryMsg;
use crate::combat::{DamageDealtMsg, DeathRecapMsg, MeleeHitMsg, OnDamageMsg};
use crate::connected::{CalendarConnectedMsg, TimingConfigurationConnectedMsg};
use crate::day_phase::SyncDayPhaseMsg;
use crate::facet::{
    AdjustFxScriptsMsg, CutsceneEndFinalizedMsg, CutsceneGrantedMsg, DisableCollisionMsg,
    GatheringInterruptedMsg, InteractionEndedMsg, InteractionValidatedMsg, PlayerSeenByAiMsg,
    SelectedAsTargetMsg, TeleportWithLoadingScreenMsg, UnselectedAsTargetMsg, UpdatePhasingMsg,
};
use crate::level_info::LevelInfoChangedMsg;
use crate::self_ident::PlayerManagerSelfIdentificationMsg;
use crate::spawn_point::ReceivePlayerSpawnPointMsg;
use crate::time_change::SyncTimeChangeMsg;
use crate::time_synch::TimeSynchMsg;
use nw_reflect::{Envelope, TypeId};

use crate::wire::{BodyCodec, MessageError};

/// Type indexes of [`ServerMessage`], in the order of the captured login.
pub const SERVER_MESSAGE_TYPES: [u64; 10] = [
    TimingConfigurationConnectedMsg::TYPE_INDEX,
    CalendarConnectedMsg::TYPE_INDEX,
    PlayerManagerSelfIdentificationMsg::TYPE_INDEX,
    ClientAddEntryMsg::TYPE_INDEX,
    ReceivePlayerSpawnPointMsg::TYPE_INDEX,
    TimeSynchMsg::TYPE_INDEX,
    SyncTimeChangeMsg::TYPE_INDEX,
    SyncDayPhaseMsg::TYPE_INDEX,
    LevelInfoChangedMsg::TYPE_INDEX,
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
    /// 1628.
    SelfIdentification(PlayerManagerSelfIdentificationMsg),
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
    /// 1635.
    LevelInfoChanged(LevelInfoChangedMsg),
    /// 8.
    Bundle(ReplicatedStateBundle),
    /// 2071.
    DamageDealt(DamageDealtMsg),
    /// 3601.
    OnDamage(OnDamageMsg),
    /// 2218.
    MeleeHit(MeleeHitMsg),
    /// 608.
    AdjustFxScripts(AdjustFxScriptsMsg),
    /// 4299.
    DeathRecap(DeathRecapMsg),
    /// 6310.
    UpdatePhasing(UpdatePhasingMsg),
    /// 6707.
    CutsceneGranted(CutsceneGrantedMsg),
    /// 6709.
    CutsceneEndFinalized(CutsceneEndFinalizedMsg),
    /// 3951.
    TeleportWithLoadingScreen(TeleportWithLoadingScreenMsg),
    /// 3743.
    InteractionValidated(InteractionValidatedMsg),
    /// 3746.
    InteractionEnded(InteractionEndedMsg),
    /// 2724.
    GatheringInterrupted(GatheringInterruptedMsg),
    /// 955.
    PlayerSeenByAi(PlayerSeenByAiMsg),
    /// 2415.
    SelectedAsTarget(SelectedAsTargetMsg),
    /// 3916.
    UnselectedAsTarget(UnselectedAsTargetMsg),
    /// 1793.
    DisableCollision(DisableCollisionMsg),
}

impl ServerMessage {
    /// Decodes `body` when `type_index` is one of [`SERVER_MESSAGE_TYPES`], a combat message of
    /// M6-B (2071, 3601, 4299, 2218) or a tutorial facet call (6310, 6707, 6709, 3951, 3743, 3746,
    /// 2724, 955, 2415, 3916, 1793); `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let message = match type_index {
            TimingConfigurationConnectedMsg::TYPE_INDEX => {
                TimingConfigurationConnectedMsg::decode(body)
                    .map(Self::TimingConfigurationConnected)
            }
            CalendarConnectedMsg::TYPE_INDEX => {
                CalendarConnectedMsg::decode(body).map(Self::CalendarConnected)
            }
            PlayerManagerSelfIdentificationMsg::TYPE_INDEX => {
                PlayerManagerSelfIdentificationMsg::decode(body).map(Self::SelfIdentification)
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
            LevelInfoChangedMsg::TYPE_INDEX => {
                LevelInfoChangedMsg::decode(body).map(Self::LevelInfoChanged)
            }
            ReplicatedStateBundle::TYPE_INDEX => {
                ReplicatedStateBundle::decode(body).map(Self::Bundle)
            }
            DamageDealtMsg::TYPE_INDEX => DamageDealtMsg::decode(body).map(Self::DamageDealt),
            OnDamageMsg::TYPE_INDEX => OnDamageMsg::decode(body).map(Self::OnDamage),
            MeleeHitMsg::TYPE_INDEX => MeleeHitMsg::decode(body).map(Self::MeleeHit),
            AdjustFxScriptsMsg::TYPE_INDEX => {
                AdjustFxScriptsMsg::decode(body).map(Self::AdjustFxScripts)
            }
            DeathRecapMsg::TYPE_INDEX => DeathRecapMsg::decode(body).map(Self::DeathRecap),
            UpdatePhasingMsg::TYPE_INDEX => UpdatePhasingMsg::decode(body).map(Self::UpdatePhasing),
            CutsceneGrantedMsg::TYPE_INDEX => {
                CutsceneGrantedMsg::decode(body).map(Self::CutsceneGranted)
            }
            CutsceneEndFinalizedMsg::TYPE_INDEX => {
                CutsceneEndFinalizedMsg::decode(body).map(Self::CutsceneEndFinalized)
            }
            TeleportWithLoadingScreenMsg::TYPE_INDEX => {
                TeleportWithLoadingScreenMsg::decode(body).map(Self::TeleportWithLoadingScreen)
            }
            InteractionValidatedMsg::TYPE_INDEX => {
                InteractionValidatedMsg::decode(body).map(Self::InteractionValidated)
            }
            InteractionEndedMsg::TYPE_INDEX => {
                InteractionEndedMsg::decode(body).map(Self::InteractionEnded)
            }
            GatheringInterruptedMsg::TYPE_INDEX => {
                GatheringInterruptedMsg::decode(body).map(Self::GatheringInterrupted)
            }
            PlayerSeenByAiMsg::TYPE_INDEX => {
                PlayerSeenByAiMsg::decode(body).map(Self::PlayerSeenByAi)
            }
            SelectedAsTargetMsg::TYPE_INDEX => {
                SelectedAsTargetMsg::decode(body).map(Self::SelectedAsTarget)
            }
            UnselectedAsTargetMsg::TYPE_INDEX => {
                UnselectedAsTargetMsg::decode(body).map(Self::UnselectedAsTarget)
            }
            DisableCollisionMsg::TYPE_INDEX => {
                DisableCollisionMsg::decode(body).map(Self::DisableCollision)
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
            Self::SelfIdentification(_) => PlayerManagerSelfIdentificationMsg::TYPE_INDEX,
            Self::ClientAddEntry(_) => ClientAddEntryMsg::TYPE_INDEX,
            Self::PlayerSpawnPoint(_) => ReceivePlayerSpawnPointMsg::TYPE_INDEX,
            Self::TimeSynch(_) => TimeSynchMsg::TYPE_INDEX,
            Self::TimeChange(_) => SyncTimeChangeMsg::TYPE_INDEX,
            Self::DayPhase(_) => SyncDayPhaseMsg::TYPE_INDEX,
            Self::LevelInfoChanged(_) => LevelInfoChangedMsg::TYPE_INDEX,
            Self::Bundle(_) => ReplicatedStateBundle::TYPE_INDEX,
            Self::DamageDealt(_) => DamageDealtMsg::TYPE_INDEX,
            Self::OnDamage(_) => OnDamageMsg::TYPE_INDEX,
            Self::MeleeHit(_) => MeleeHitMsg::TYPE_INDEX,
            Self::AdjustFxScripts(_) => AdjustFxScriptsMsg::TYPE_INDEX,
            Self::DeathRecap(_) => DeathRecapMsg::TYPE_INDEX,
            Self::UpdatePhasing(_) => UpdatePhasingMsg::TYPE_INDEX,
            Self::CutsceneGranted(_) => CutsceneGrantedMsg::TYPE_INDEX,
            Self::CutsceneEndFinalized(_) => CutsceneEndFinalizedMsg::TYPE_INDEX,
            Self::TeleportWithLoadingScreen(_) => TeleportWithLoadingScreenMsg::TYPE_INDEX,
            Self::InteractionValidated(_) => InteractionValidatedMsg::TYPE_INDEX,
            Self::InteractionEnded(_) => InteractionEndedMsg::TYPE_INDEX,
            Self::GatheringInterrupted(_) => GatheringInterruptedMsg::TYPE_INDEX,
            Self::PlayerSeenByAi(_) => PlayerSeenByAiMsg::TYPE_INDEX,
            Self::SelectedAsTarget(_) => SelectedAsTargetMsg::TYPE_INDEX,
            Self::UnselectedAsTarget(_) => UnselectedAsTargetMsg::TYPE_INDEX,
            Self::DisableCollision(_) => DisableCollisionMsg::TYPE_INDEX,
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
            Self::SelfIdentification(message) => message.encode(),
            Self::ClientAddEntry(message) => message.encode(),
            Self::PlayerSpawnPoint(message) => message.encode(),
            Self::TimeSynch(message) => message.encode(),
            Self::TimeChange(message) => message.encode(),
            Self::DayPhase(message) => message.encode(),
            Self::LevelInfoChanged(message) => message.encode(),
            Self::Bundle(message) => message.encode(),
            Self::DamageDealt(message) => message.encode(),
            Self::OnDamage(message) => message.encode(),
            Self::MeleeHit(message) => message.encode(),
            Self::AdjustFxScripts(message) => message.encode(),
            Self::DeathRecap(message) => message.encode(),
            Self::UpdatePhasing(message) => message.encode(),
            Self::CutsceneGranted(message) => message.encode(),
            Self::CutsceneEndFinalized(message) => message.encode(),
            Self::TeleportWithLoadingScreen(message) => message.encode(),
            Self::InteractionValidated(message) => message.encode(),
            Self::InteractionEnded(message) => message.encode(),
            Self::GatheringInterrupted(message) => message.encode(),
            Self::PlayerSeenByAi(message) => message.encode(),
            Self::SelectedAsTarget(message) => message.encode(),
            Self::UnselectedAsTarget(message) => message.encode(),
            Self::DisableCollision(message) => message.encode(),
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
            Some(&LevelInfoChangedMsg::TYPE_INDEX)
        );
        for message in samples {
            let body = message.encode().expect("encode");
            assert_eq!(
                ServerMessage::decode(message.type_index(), &body),
                Some(Ok(message))
            );
        }
    }

    /// crc32 of `potionheal`.
    const POTION_HEAL_SCRIPT: u32 = 0xf7e1_e0d6;

    #[test]
    fn the_retail_potion_script_message_has_the_retail_bytes() {
        // Retail 608 (xp-potion-exit note, section 2.2): `d934888ec32a8b14 bfa96f3ea2e36a5a
        // f7e1e0d6 01`; the facet entity is the reversed player number.
        const PLAYER: u64 = 0x5a6a_e3a2_3e6f_a9bf;
        let body = AdjustFxScriptsMsg::on(PLAYER, POTION_HEAL_SCRIPT)
            .encode()
            .expect("encode");
        assert_eq!(
            body,
            [
                0xd9, 0x34, 0x88, 0x8e, 0xc3, 0x2a, 0x8b, 0x14, 0xbf, 0xa9, 0x6f, 0x3e, 0xa2, 0xe3,
                0x6a, 0x5a, 0xf7, 0xe1, 0xe0, 0xd6, 0x01
            ]
        );
    }

    #[test]
    fn combat_messages_decode_by_their_index() {
        let samples = [
            ServerMessage::DamageDealt(DamageDealtMsg::hit(ENTITY, ENTITY + 1, 5.0, false)),
            ServerMessage::OnDamage(crate::test_support::melee_sample(ENTITY, ENTITY + 1, 5.0)),
            ServerMessage::DeathRecap(DeathRecapMsg::of(ENTITY)),
            ServerMessage::MeleeHit(MeleeHitMsg::on(ENTITY, true)),
            ServerMessage::AdjustFxScripts(AdjustFxScriptsMsg::on(ENTITY, POTION_HEAL_SCRIPT)),
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
