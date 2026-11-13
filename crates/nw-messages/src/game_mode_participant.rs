// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Javelin::GameModeParticipantReplicatedState` (class 3312, own player component 24): the
//! game modes a player takes part in and its matchmaking queue. Layout from the client's member
//! table: slot 0 holds `activeGameModes`, `flags`, `queuingForGameModes`,
//! `queueEligibleTimesForGameModes`, `gameModeMutationContext` and `lastTeamIndex`; slot 1 the
//! matchmaking service children (`matchmakingServiceActivity`, `..Status`, `..MatchId`,
//! `..DesiredPlayers`, `matchmakingServieAcceptedPlayers`, `isReplicatingGroupActivity`); slot 2
//! `groupActivityEligibility`.
//!
//! Observed in the retail duels (`duel_capture`) and arenas (`3v3`, `20261010-125035-658`,
//! `3v3_arena_group_2`, `Hive_3v3_group`; arena flow study 2026-11-11):
//!
//! - when the player joins a mode: `activeGameModes` in its full form with the one mode (the
//!   key: the game mode entity's raw id, its facet entity and a second id; the value: the
//!   mode's id, its map id, the join time and, in an arena, the instance phase id), `flags` and
//!   `lastTeamIndex` (`01 23 00 01 <version> 01 <key> <value> <flags> <team>`);
//! - while the mode runs: `flags` alone (`01 02 <flags>`);
//! - when the player leaves it: the empty mode list, zero flags, an empty mutation context
//!   and the team index 255 (`01 33 00 01 <version> 00 <13 + 6 zero bytes> ff`);
//! - when the player joins an expedition: the mutation context too, empty in a normal run
//!   (Starstone 7.141 s) and, in a mutated run, the CRC-32 of the lowercase elemental
//!   category, promotion and curse ids and the difficulty (`EXP-Empyrian-Forge-M1` 476.658 s:
//!   `79366364 72e544c1 08218e8e 01`, `voi`, `indomitable`, `censored`, 1;
//!   `M2-TEMPEST-HEART` 443.560 s: `fir`, `savage`, `fiendish`, 2);
//! - a queue join: `queuingForGameModes` with one entry and `matchmakingServiceActivity` 2
//!   (`03 04 <list> 01 02`); a pop: the entry's match size, status 10, the match id and the
//!   desired players; each accept: the accepted count (`02 10 <count>`);
//! - a group eligibility subscription (6211): `isReplicatingGroupActivity` with, on a
//!   subscribe, `groupActivityEligibility` (`06 20 01 01 <16 bytes>`).
//!
//! The meaning of the bits of `flags` and of `groupActivityEligibility` is Unknown: the sim
//! sends the values retail sent in each phase.

use crate::items::FullList;
use crate::slots::{children_mask, write_known_children};
use crate::wire::{MessageError, RAW_ID_LEN, Writer};

/// Type index of `Javelin::GameModeParticipantReplicatedState`.
pub const GAME_MODE_PARTICIPANT_TYPE_INDEX: u64 = 3312;
/// The own player component of the state.
pub const GAME_MODE_PARTICIPANT_COMPONENT: u64 = 24;
/// Bytes of `flags`.
pub const PARTICIPANT_FLAGS_LEN: usize = 6;
/// Bytes of `groupActivityEligibility`.
pub const ELIGIBILITY_LEN: usize = 16;
/// The `lastTeamIndex` of a player in no team (Observed when the player left its duel).
pub const NO_TEAM: u8 = 0xff;
/// The children of slot 0.
const SLOT_0_CHILDREN: u8 = 6;
/// The children of slot 1.
const SLOT_1_CHILDREN: u8 = 6;
/// The children of slot 2.
const SLOT_2_CHILDREN: u8 = 1;
/// The slot bits of the slot byte.
const SLOT_0: u8 = 0x01;
const SLOT_1: u8 = 0x02;
const SLOT_2: u8 = 0x04;
/// The slot byte of a state that sets no child.
const NO_SLOT: u8 = 0;
/// The child bit of `groupActivityEligibility` in slot 2.
const ELIGIBILITY_CHILD: u16 = 0x01;

/// One game mode a player takes part in (`activeGameModes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveGameMode {
    /// The raw id of the game mode entity.
    pub raw_id: [u8; RAW_ID_LEN],
    /// The game mode entity's facet entity.
    pub facet: u64,
    /// A second id of the game mode entity (name Unknown).
    pub second: u64,
    /// CRC-32 of the lowercase `GameModeId`.
    pub mode: u32,
    /// The mode's map id: CRC-32 of the lowercase `GameModeMapId` (Observed: the mode's id in a
    /// duel and on `NW_Arena01`, `8b2f3dab` on `NW_Arena_3v3_04`).
    pub map: u32,
    /// When the player joined: ns of server time since 2000-01-01.
    pub joined_ns: u64,
    /// The instance phase id of an arena (the 1635's); 0 in a duel (Observed).
    pub phase: u64,
}

/// One queue of a player (`queuingForGameModes`): Observed in every retail arena queue entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueEntry {
    /// CRC-32 of the lowercase `GameModeId`.
    pub mode: u32,
    /// When the player joined the queue: ns of server time since 2000-01-01.
    pub joined_ns: u64,
    /// A value of Unknown meaning (0 in every retail entry).
    pub unknown: u32,
    /// The players of the match it was matched into; 0 before a pop (Observed: 2 and 6).
    pub match_size: u32,
    /// A byte of Unknown meaning (1 in every retail entry).
    pub flag: u8,
}

/// `gameModeMutationContext`: the mutation of the player's game mode (module text).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MutationContextWire {
    /// CRC-32 of the lowercase elemental category (`fir`).
    pub elemental: u32,
    /// CRC-32 of the lowercase `PromotionMutationID`.
    pub promotion: u32,
    /// CRC-32 of the lowercase `CurseMutationID`.
    pub curse: u32,
    /// The mutation difficulty (1 to 3); 0 in the empty context.
    pub difficulty: u8,
}

impl MutationContextWire {
    /// The empty context: no mutation.
    pub const NONE: Self = Self {
        elemental: 0,
        promotion: 0,
        curse: 0,
        difficulty: 0,
    };
}

/// The matchmaking service children of slot 1. A `None` child is not sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MatchmakingService {
    /// `matchmakingServiceActivity` (Observed: 2 queued for an arena, 0 for Outpost Rush, 0x16
    /// with no queue).
    pub activity: Option<u8>,
    /// `matchmakingServiceStatus` (Observed: 10 at a pop, 1 two seconds later, 5 at the move).
    pub status: Option<u32>,
    /// `matchmakingServiceMatchId`: the match id as hyphenated UUID text; empty with no match.
    pub match_id: Option<String>,
    /// `matchmakingServiceDesiredPlayers`.
    pub desired: Option<u8>,
    /// `matchmakingServieAcceptedPlayers` (the client's spelling).
    pub accepted: Option<u8>,
    /// `isReplicatingGroupActivity`.
    pub replicating_group: Option<bool>,
}

impl MatchmakingService {
    fn mask(&self) -> u16 {
        children_mask(&[
            self.activity.is_some(),
            self.status.is_some(),
            self.match_id.is_some(),
            self.desired.is_some(),
            self.accepted.is_some(),
            self.replicating_group.is_some(),
        ])
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let mask = self.mask();
        let mut result = Ok(());
        write_known_children(writer, mask, SLOT_1_CHILDREN, |writer, child| match child {
            0 => {
                if let Some(activity) = self.activity {
                    writer.u8(activity);
                }
            }
            1 => {
                if let Some(status) = self.status {
                    writer.u32(status);
                }
            }
            2 => {
                if let Some(text) = &self.match_id {
                    result = writer.prefixed(text.len() as u64).map(|writer| {
                        writer.bytes(text.as_bytes());
                    });
                }
            }
            3 => {
                if let Some(desired) = self.desired {
                    writer.u8(desired);
                }
            }
            4 => {
                if let Some(accepted) = self.accepted {
                    writer.u8(accepted);
                }
            }
            _ => {
                if let Some(replicating) = self.replicating_group {
                    writer.u8(u8::from(replicating));
                }
            }
        });
        result
    }
}

/// The state. A `None` child is not sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameModeParticipantState {
    /// Child 0, `activeGameModes`, in its full form.
    pub active: Option<FullList<ActiveGameMode>>,
    /// Child 1, `flags`.
    pub flags: Option<[u8; PARTICIPANT_FLAGS_LEN]>,
    /// Child 2, `queuingForGameModes`, in its full form.
    pub queuing: Option<FullList<QueueEntry>>,
    /// Child 3, `queueEligibleTimesForGameModes`, in its full form: each mode's CRC and the
    /// Unix ns from which it may queue again (Observed: a leave of a running arena match).
    pub eligible: Option<FullList<(u32, u64)>>,
    /// Child 4, `gameModeMutationContext`: [`MutationContextWire::NONE`] when the player leaves
    /// its mode or joins a normal expedition, the run's mutation in a mutated one.
    pub mutation_context: Option<MutationContextWire>,
    /// Child 5, `lastTeamIndex`.
    pub last_team: Option<u8>,
    /// Slot 1, the matchmaking service.
    pub service: MatchmakingService,
    /// Slot 2, `groupActivityEligibility`.
    pub eligibility: Option<[u8; ELIGIBILITY_LEN]>,
}

impl GameModeParticipantState {
    fn slot_0_mask(&self) -> u16 {
        children_mask(&[
            self.active.is_some(),
            self.flags.is_some(),
            self.queuing.is_some(),
            self.eligible.is_some(),
            self.mutation_context.is_some(),
            self.last_team.is_some(),
        ])
    }

    /// Writes the state for the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a list or the match id does not write.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let slot_0 = self.slot_0_mask();
        let slot_1 = self.service.mask();
        let slots = [
            (SLOT_0, slot_0 != 0),
            (SLOT_1, slot_1 != 0),
            (SLOT_2, self.eligibility.is_some()),
        ]
        .into_iter()
        .filter(|(_, present)| *present)
        .fold(NO_SLOT, |byte, (bit, _)| byte | bit);
        writer.u8(slots);
        if slot_0 != 0 {
            self.write_slot_0(writer, slot_0)?;
        }
        if slot_1 != 0 {
            self.service.write(writer)?;
        }
        if let Some(eligibility) = &self.eligibility {
            write_known_children(writer, ELIGIBILITY_CHILD, SLOT_2_CHILDREN, |writer, _| {
                writer.bytes(eligibility);
            });
        }
        Ok(())
    }

    fn write_slot_0(&self, writer: &mut Writer, mask: u16) -> Result<(), MessageError> {
        let mut result = Ok(());
        write_known_children(writer, mask, SLOT_0_CHILDREN, |writer, child| {
            let written = match child {
                0 => self.active.as_ref().map_or(Ok(()), |active| {
                    active.write(writer, |writer, mode| {
                        writer
                            .bytes(&mode.raw_id)
                            .u64(mode.facet)
                            .u64(mode.second)
                            .u32(mode.mode)
                            .u32(mode.map)
                            .u64(mode.joined_ns)
                            .u64(mode.phase);
                        Ok(())
                    })
                }),
                1 => {
                    if let Some(flags) = self.flags {
                        writer.bytes(&flags);
                    }
                    Ok(())
                }
                2 => self.queuing.as_ref().map_or(Ok(()), |queuing| {
                    queuing.write(writer, |writer, entry| {
                        writer
                            .u32(entry.mode)
                            .u64(entry.joined_ns)
                            .u32(entry.unknown)
                            .u32(entry.match_size)
                            .u8(entry.flag);
                        Ok(())
                    })
                }),
                3 => self.eligible.as_ref().map_or(Ok(()), |eligible| {
                    eligible.write(writer, |writer, (mode, unix_ns)| {
                        writer.u32(*mode).u64(*unix_ns);
                        Ok(())
                    })
                }),
                4 => {
                    if let Some(context) = self.mutation_context {
                        writer
                            .u32(context.elemental)
                            .u32(context.promotion)
                            .u32(context.curse)
                            .u8(context.difficulty);
                    }
                    Ok(())
                }
                _ => {
                    if let Some(team) = self.last_team {
                        writer.u8(team);
                    }
                    Ok(())
                }
            };
            if result.is_ok() {
                result = written;
            }
        });
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CRC-32 of `duel`.
    const DUEL: u32 = 0x9bb4_a762;
    /// CRC-32 of `arena3v3`.
    const ARENA: u32 = 0x1fad_9d42;

    fn hex(state: &GameModeParticipantState) -> String {
        let mut writer = Writer::new();
        state.write(&mut writer).expect("the state writes");
        writer
            .finish()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// Retail `duel_capture`: the join at 67.299 s (a made-up game mode entity), the fight's
    /// flags at 73.333 s and the leave at 91.966 s.
    #[test]
    fn the_states_of_a_retail_duel_write_the_same_bytes() {
        let join = GameModeParticipantState {
            active: Some(FullList {
                version: Some(0x0729),
                elements: vec![ActiveGameMode {
                    raw_id: [0xa1; RAW_ID_LEN],
                    facet: 0xa2a2_a2a2_a2a2_a2a2,
                    second: 0xa3a3_a3a3_a3a3_a3a3,
                    mode: DUEL,
                    map: DUEL,
                    joined_ns: 0x0b95_7d54_6a87_b4f9,
                    phase: 0,
                }],
            }),
            flags: Some([0x00, 0x00, 0x17, 0x10, 0x00, 0x00]),
            last_team: Some(0),
            ..GameModeParticipantState::default()
        };
        let expected = [
            "01230001a91c01",
            "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a2a2a2a2a2a2a2a2a3a3a3a3a3a3a3a3",
            "9bb4a7629bb4a7620b957d546a87b4f90000000000000000",
            "00001710000000",
        ]
        .concat();
        assert_eq!(hex(&join), expected);
        let fight = GameModeParticipantState {
            flags: Some([0xef, 0x17, 0x17, 0x30, 0x00, 0x00]),
            ..GameModeParticipantState::default()
        };
        assert_eq!(hex(&fight), "0102ef1717300000");
        let leave = GameModeParticipantState {
            active: Some(FullList {
                version: Some(0x0df5),
                elements: Vec::new(),
            }),
            flags: Some([0; PARTICIPANT_FLAGS_LEN]),
            mutation_context: Some(MutationContextWire::NONE),
            last_team: Some(NO_TEAM),
            ..GameModeParticipantState::default()
        };
        assert_eq!(
            hex(&leave),
            "01330001b5370000000000000000000000000000000000000000ff"
        );
    }

    /// Retail `20261010-125035-658` (arena flow study, section 5.4; made-up ids): the queue
    /// join of 286.5 s, the pop of 292.196 s with a made-up match id, the accept count, the
    /// arena join of 300.021 s and the round flags.
    #[test]
    fn the_states_of_a_retail_arena_queue_and_match_write_the_same_bytes() {
        let queued = GameModeParticipantState {
            queuing: Some(FullList {
                version: Some(12_063),
                elements: vec![QueueEntry {
                    mode: ARENA,
                    joined_ns: 0x0bb9_d8f1_dae8_12a9,
                    unknown: 0,
                    match_size: 2,
                    flag: 1,
                }],
            }),
            service: MatchmakingService {
                status: Some(0x0a),
                match_id: Some("00000000-0000-7000-8000-000000000001".to_owned()),
                desired: Some(2),
                ..MatchmakingService::default()
            },
            ..GameModeParticipantState::default()
        };
        assert_eq!(
            hex(&queued),
            [
                "030400019fbc01",
                "1fad9d420bb9d8f1dae812a90000000000000002010e0000000a24",
                "30303030303030302d303030302d373030302d383030302d30303030303030303030303102",
            ]
            .concat()
        );
        let status = GameModeParticipantState {
            service: MatchmakingService {
                status: Some(1),
                ..MatchmakingService::default()
            },
            ..GameModeParticipantState::default()
        };
        assert_eq!(hex(&status), "020200000001");
        let accepted = GameModeParticipantState {
            service: MatchmakingService {
                accepted: Some(1),
                ..MatchmakingService::default()
            },
            ..GameModeParticipantState::default()
        };
        assert_eq!(hex(&accepted), "021001");
        let joined = GameModeParticipantState {
            active: Some(FullList {
                version: Some(12),
                elements: vec![ActiveGameMode {
                    raw_id: [0xf1; RAW_ID_LEN],
                    facet: 0xf1f1_f1f1_f1f1_f1f1,
                    second: 0xf2f2_f2f2_f2f2_f2f2,
                    mode: ARENA,
                    map: ARENA,
                    joined_ns: 0x0bb9_d8f4_468c_275c,
                    phase: 0x8b2c_bbb8_6ac5_c889,
                }],
            }),
            flags: Some([0x03, 0xc0, 0xe7, 0x08, 0x00, 0x00]),
            last_team: Some(0),
            ..GameModeParticipantState::default()
        };
        assert_eq!(
            hex(&joined),
            [
                "012300010c01",
                "f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f2f2f2f2f2f2f2f2",
                "1fad9d421fad9d420bb9d8f4468c275c8b2cbbb86ac5c88903c0e708000000",
            ]
            .concat()
        );
    }

    /// Retail `3v3` 661.769 s (the 6211 answer) and `3v3_arena_group_2` 477.366 s (a queue
    /// that ended) and 765.332 s (the leaver penalty).
    #[test]
    fn the_retail_eligibility_end_of_queue_and_penalty_write_the_same_bytes() {
        let subscribed = GameModeParticipantState {
            service: MatchmakingService {
                replicating_group: Some(true),
                ..MatchmakingService::default()
            },
            eligibility: Some([
                0xbf, 0xf9, 0x7e, 0xfe, 0xbf, 0xef, 0x7f, 0xff, 0xff, 0xe7, 0x2f, 0, 0, 0, 0, 0,
            ]),
            ..GameModeParticipantState::default()
        };
        assert_eq!(hex(&subscribed), "06200101bff97efebfef7fffffe72f0000000000");
        let ended = GameModeParticipantState {
            queuing: Some(FullList {
                version: Some(9_433),
                elements: Vec::new(),
            }),
            service: MatchmakingService {
                activity: Some(0x16),
                status: Some(0),
                match_id: Some(String::new()),
                desired: Some(0),
                accepted: Some(0),
                replicating_group: None,
            },
            ..GameModeParticipantState::default()
        };
        assert_eq!(hex(&ended), "030400019993001f1600000000000000");
        let penalty = GameModeParticipantState {
            eligible: Some(FullList {
                version: Some(5),
                elements: vec![
                    (0x91b9_9a18, 0x188a_a65f_bebd_4c48),
                    (ARENA, 0x18ba_90b7_6b06_b2d4),
                ],
            }),
            ..GameModeParticipantState::default()
        };
        assert_eq!(
            hex(&penalty),
            "01080001050291b99a18188aa65fbebd4c481fad9d4218ba90b76b06b2d4"
        );
    }

    /// Retail `M2-TEMPEST-HEART` 443.560 s (made-up game mode entity and times): a player's join
    /// of a mutated expedition carries the mutation context `fir`, `savage`, `fiendish` at
    /// difficulty 2; Starstone 7.141 s: a normal run's join carries the empty context.
    #[test]
    fn an_expedition_join_writes_its_mutation_context() {
        /// CRC-32 of `dungeonshattermtn00`.
        const TEMPEST: u32 = 0x7f1c_8a63;
        let mutated = GameModeParticipantState {
            active: Some(FullList {
                version: Some(0x0df5),
                elements: vec![ActiveGameMode {
                    raw_id: [0xb1; RAW_ID_LEN],
                    facet: 0xb2b2_b2b2_b2b2_b2b2,
                    second: 0xb3b3_b3b3_b3b3_b3b3,
                    mode: TEMPEST,
                    map: TEMPEST,
                    joined_ns: 0x0bb7_7972_37de_ed1b,
                    phase: 0x8b2c_bbb8_6aa9_a152,
                }],
            }),
            flags: Some([0x11, 0x00, 0x07, 0x00, 0x00, 0x00]),
            mutation_context: Some(MutationContextWire {
                elemental: 0xb92f_ae7e,
                promotion: 0x8803_b6d1,
                curse: 0xa78a_7b02,
                difficulty: 2,
            }),
            last_team: Some(0),
            ..GameModeParticipantState::default()
        };
        assert_eq!(
            hex(&mutated),
            [
                "01330001b53701",
                "b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b2b2b2b2b2b2b2b2b3b3b3b3b3b3b3b3",
                "7f1c8a637f1c8a630bb7797237deed1b8b2cbbb86aa9a152",
                "110007000000",
                "b92fae7e8803b6d1a78a7b0202",
                "00",
            ]
            .concat()
        );
        let normal = GameModeParticipantState {
            mutation_context: Some(MutationContextWire::NONE),
            ..GameModeParticipantState::default()
        };
        assert_eq!(hex(&normal), "011000000000000000000000000000");
    }
}
