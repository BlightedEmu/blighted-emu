// SPDX-License-Identifier: AGPL-3.0-or-later
//! Musical performances (run 15 music study; retail captures `open-world/Music`,
//! `music_abort_current_song`, `20261010-113829-464` and `SQ-Everfall-1` play-for-the-town).
//!
//! A player with an instrument asks for a performance (4891). The server creates a performance
//! entity (its own spawn group: card, [`MusicalPerformanceState`] at component 2, position) and
//! answers with its id (4869), or refuses with an error (4889). The player's own state (type
//! 4878, player component 71, [`PerformancePlayerState`]) names the performance it plays in and
//! the one it watches. The performer picks a song (4867), a sheet (4879) and a reward (5022), and
//! starts (4865, with the song's length); the client loads the song and says it is ready (5091),
//! the performance plays, the client sends each note it plays (4892), and the server scores the
//! notes, shows the results and the post show (5233 ends it early) and goes back to the pre show.
//! A song can be stopped (5417). Leaving (4881) ends the performance and starts a lockout.
//!
//! Every call starts with the facet header of the player's `MusicalPerformancePlayerComponent`,
//! which sits on the player prefab's child entity `MusicalPerformance`: the target is (entity XOR
//! [`MUSIC_CHILD`]) + [`MUSIC_FACET`] (Observed on six characters).

use crate::facet::FacetHeader;
use crate::list_delta::ReplicatedList;
use crate::slots::{children_mask, read_children, read_slot_byte, write_children};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `Javelin::MusicalPerformancePlayerComponentReplicatedState`.
pub const PERFORMANCE_PLAYER_TYPE_INDEX: u64 = 4878;
/// The player component of the performance player state.
pub const PERFORMANCE_PLAYER_COMPONENT: u64 = 71;
/// Type index of `Javelin::MusicalPerformanceReplicatedState`.
pub const PERFORMANCE_TYPE_INDEX: u64 = 4896;
/// The component of the performance state in the performance entity's group.
pub const PERFORMANCE_COMPONENT: u64 = 2;
/// The target-id constant of `MusicalPerformancePlayerComponent`.
pub const MUSIC_FACET: u64 = 0xc705_15a6_9eb5_bbc6;
/// The XOR of the player prefab's main entity and its child `MusicalPerformance`.
pub const MUSIC_CHILD: u64 = 0xdaee_4ef8_0a02_4236;
/// `RequestCreatePerformance`.
pub const CREATE_PERFORMANCE_TYPE_INDEX: u64 = 4891;
/// `RequestLeavePerformance`.
pub const LEAVE_PERFORMANCE_TYPE_INDEX: u64 = 4881;
/// `RequestSetCurrentSong`.
pub const SET_SONG_TYPE_INDEX: u64 = 4867;
/// `RequestSetCurrentSongSheet`.
pub const SET_SHEET_TYPE_INDEX: u64 = 4879;
/// `RequestStartPerformance`.
pub const START_PERFORMANCE_TYPE_INDEX: u64 = 4865;
/// `RequestSetClientReady`.
pub const CLIENT_READY_TYPE_INDEX: u64 = 5091;
/// `RequestValidateInput`.
pub const VALIDATE_INPUT_TYPE_INDEX: u64 = 4892;
/// `RequestSetCurrentReward`.
pub const SET_REWARD_TYPE_INDEX: u64 = 5022;
/// `RequestEndPostShow`.
pub const END_POST_SHOW_TYPE_INDEX: u64 = 5233;
/// `RequestAbortCurrentSong`.
pub const ABORT_SONG_TYPE_INDEX: u64 = 5417;
/// `PerformanceCreated`.
pub const PERFORMANCE_CREATED_TYPE_INDEX: u64 = 4869;
/// `FailedPerformanceCreated`.
pub const PERFORMANCE_FAILED_TYPE_INDEX: u64 = 4889;
/// An id that names no performance (Observed: `00000000ffffffff` in every empty state).
pub const NO_PERFORMANCE: u64 = 0x0000_0000_ffff_ffff;
/// The performers a performance holds: the performer and score lists have 5 entries and the
/// state has 5 seed slots.
pub const MAX_PERFORMERS: usize = 5;
const PLAYER_FIELD: &str = "musical performance player";
const PERFORMANCE_FIELD: &str = "musical performance";

/// The client enum `MusicalPerformanceErrors` (`FUN_143959180`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PerformanceError {
    /// 0.
    Unknown = 0,
    /// 1.
    None = 1,
    /// 2.
    Unsupported = 2,
    /// 3.
    NotAMember = 3,
    /// 4.
    AlreadyAMember = 4,
    /// 5.
    NotLeader = 5,
    /// 6.
    NotLocalPlayer = 6,
    /// 7.
    NoSongSelected = 7,
    /// 8.
    SheetAlreadySelected = 8,
    /// 9.
    SheetNotAvailable = 9,
    /// 10.
    SheetNotUnlocked = 10,
    /// 11.
    AlreadySelectingSheet = 11,
    /// 12.
    NotSelectingSheet = 12,
    /// 13.
    InvalidSheetId = 13,
    /// 14.
    AssetNotAvailable = 14,
    /// 15.
    InvalidAssetType = 15,
    /// 16.
    PerformanceFull = 16,
    /// 17.
    PerformanceAlreadyStarted = 17,
    /// 18.
    PerformancePreloading = 18,
    /// 19.
    PerformanceNotReady = 19,
    /// 20.
    PerformanceEnding = 20,
    /// 21.
    NoCurrentPerformance = 21,
    /// 22.
    RankTooLow = 22,
    /// 23.
    InvalidRewardId = 23,
    /// 24.
    CreateFailedInCooldown = 24,
    /// 25.
    CreateFailedRestrictedArea = 25,
    /// 26.
    SoundBankNotLoaded = 26,
    /// 27: retail's answer to a create without an instrument (Observed, `open-world/Music`).
    CreateFailedNoInstrument = 27,
    /// 29.
    PerformanceTimeout = 29,
    /// 30.
    CreateFailedTooCloseToPerformance = 30,
    /// 31.
    CreateFailedInLockout = 31,
    /// 32.
    CreateFailedTeleporting = 32,
    /// 33.
    CreateFailedInHouse = 33,
}

/// The client enum `MusicalPerformanceState`, the performance's `performanceState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PerformanceStage {
    /// 0.
    Unknown = 0,
    /// 1: the performance was created.
    WaitingToJoin = 1,
    /// 2.
    Idle = 2,
    /// 3: song, sheet and reward are picked.
    PreShow = 3,
    /// 4: the clients load the song.
    PreLoad = 4,
    /// 5: the song plays.
    Playing = 5,
    /// 6: the scores show.
    Results = 6,
    /// 7.
    PostShow = 7,
    /// 8.
    Done = 8,
    /// 9: the performance goes.
    Ending = 9,
}

impl PerformanceStage {
    const ALL: [Self; 10] = [
        Self::Unknown,
        Self::WaitingToJoin,
        Self::Idle,
        Self::PreShow,
        Self::PreLoad,
        Self::Playing,
        Self::Results,
        Self::PostShow,
        Self::Done,
        Self::Ending,
    ];

    /// The stage of wire value `value`; `None` for another value.
    pub fn of(value: u8) -> Option<Self> {
        Self::ALL.get(usize::from(value)).copied()
    }
}

/// `MusicalPerformancePlayerComponentReplicatedState` (type 4878): each field is `None` when it
/// is not sent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PerformancePlayerState {
    /// Slot 0 child 0 `performanceId`: the performance it plays in ([`NO_PERFORMANCE`]).
    pub performance: Option<u64>,
    /// Slot 1 child 0 `lockoutDuration` (Observed 40 at a leave, 0 again 10 s later).
    pub lockout: Option<u16>,
    /// Slot 1 child 1 `performanceState`: 1 from the create to the leave, else 0.
    pub performing: Option<u8>,
    /// Slot 1 child 2 `observingPerformanceId`: the performance whose zone it is in.
    pub observing: Option<u64>,
}

/// Slots of 4878.
const PLAYER_SLOTS: u8 = 2;
/// The slot byte bits of 4878.
const PLAYER_SLOT_0: u8 = 0b01;
const PLAYER_SLOT_1: u8 = 0b10;
/// Children of slot 0 and of slot 1 of 4878.
const PLAYER_SLOT_0_CHILDREN: u8 = 1;
const PLAYER_SLOT_1_CHILDREN: u8 = 3;
const CHILD_LOCKOUT: u8 = 0;
const CHILD_PERFORMING: u8 = 1;

impl PerformancePlayerState {
    /// The first state of a character that is in no performance and has no lockout, as retail
    /// sends it on a new character's card (476 of 479 retail first groups).
    pub const NONE: Self = Self {
        performance: Some(NO_PERFORMANCE),
        lockout: None,
        performing: Some(0),
        observing: Some(NO_PERFORMANCE),
    };

    /// Reads a body.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or names a slot or child the class
    /// does not have.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let slots = read_slot_byte(reader, PLAYER_FIELD, PLAYER_SLOTS)?;
        let mut state = Self::default();
        if slots & PLAYER_SLOT_0 != 0 {
            read_children(reader, PLAYER_FIELD, PLAYER_SLOT_0_CHILDREN, |reader, _| {
                state.performance = Some(reader.u64()?);
                Ok(())
            })?;
        }
        if slots & PLAYER_SLOT_1 != 0 {
            read_children(
                reader,
                PLAYER_FIELD,
                PLAYER_SLOT_1_CHILDREN,
                |reader, child| {
                    match child {
                        CHILD_LOCKOUT => state.lockout = Some(reader.u16()?),
                        CHILD_PERFORMING => state.performing = Some(reader.u8()?),
                        _ => state.observing = Some(reader.u64()?),
                    }
                    Ok(())
                },
            )?;
        }
        Ok(state)
    }

    /// Whether it sends nothing.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Writes the body.
    pub fn write(&self, writer: &mut Writer) {
        let owner = children_mask(&[
            self.lockout.is_some(),
            self.performing.is_some(),
            self.observing.is_some(),
        ]);
        let slot_bit = |on: bool, bit: u8| if on { bit } else { 0 };
        writer.u8(slot_bit(self.performance.is_some(), PLAYER_SLOT_0)
            | slot_bit(owner != 0, PLAYER_SLOT_1));
        if let Some(performance) = self.performance {
            write_infallible(writer, 1, PLAYER_SLOT_0_CHILDREN, |writer, _| {
                writer.u64(performance);
            });
        }
        if owner != 0 {
            write_infallible(
                writer,
                owner,
                PLAYER_SLOT_1_CHILDREN,
                |writer, child| match child {
                    CHILD_LOCKOUT => {
                        self.lockout.map(|lockout| writer.u16(lockout));
                    }
                    CHILD_PERFORMING => {
                        self.performing.map(|performing| writer.u8(performing));
                    }
                    _ => {
                        self.observing.map(|observing| writer.u64(observing));
                    }
                },
            );
        }
    }
}

/// [`write_children`] with a writer that cannot fail.
fn write_infallible(
    writer: &mut Writer,
    children: u16,
    count: u8,
    mut write_child: impl FnMut(&mut Writer, u8),
) {
    write_children::<std::convert::Infallible>(writer, children, count, |writer, child| {
        write_child(writer, child);
        Ok(())
    })
    .unwrap_or_else(|never| match never {});
}

/// The slot bytes of 4896: one per group of 8 slots, each before its group's slots.
const SLOTS_PER_BYTE: usize = 8;
/// The slots of 4896.
const PERFORMANCE_SLOTS: usize = 9;
/// Children of slots 0, 1 and 2.
const SLOT_0_CHILDREN: u8 = 8;
const SLOT_1_CHILDREN: u8 = 5;
const SLOT_2_CHILDREN: u8 = 9;
/// The first seed slot (`performerSeed[0]`), and its children: the seed and the zone's player
/// count; the other seed slots have the seed only.
const FIRST_SEED_SLOT: usize = 4;
const FIRST_SEED_SLOT_CHILDREN: u8 = 2;
const SEED_SLOT_CHILDREN: u8 = 1;
/// Slot 3 has no children.
const EMPTY_SLOT: usize = 3;
/// Slot 2 children that are never sent (`songBookOverrides`, `rewardOverrides`): their layout is
/// Unknown.
const SONG_BOOK_OVERRIDES: u8 = 3;
const REWARD_OVERRIDES: u8 = 6;

/// `MusicalPerformanceReplicatedState` (type 4896): each field is `None` when it is not sent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MusicalPerformanceState {
    /// Slot 0 child 0 `performers`: 5 entity ids ([`NO_PERFORMANCE`] for a free place).
    pub performers: Option<ReplicatedList<u64>>,
    /// Slot 0 child 1 `radius`, m.
    pub radius: Option<f32>,
    /// Slot 0 child 2 `zoneType`.
    pub zone_type: Option<u32>,
    /// Slot 0 child 3 `selectedSong`: the CRC-32 of the lowercase `SongId` (0 for none).
    pub selected_song: Option<u32>,
    /// Slot 0 child 4 `packedPerformerSheetIndices`: the sheet column (1-based) of each
    /// performer.
    pub sheet_indices: Option<u16>,
    /// Slot 0 child 5 `startTime`: server ns since 2000-01-01 when the song started.
    pub start_time_ns: Option<u64>,
    /// Slot 0 child 6 `performanceState` ([`PerformanceStage`]).
    pub stage: Option<u8>,
    /// Slot 0 child 7 `flags` (Observed 1 always).
    pub flags: Option<u8>,
    /// Slot 1 child 0 `leaderIndex`.
    pub leader_index: Option<u8>,
    /// Slot 1 child 1 `errorSync`: a change counter (low 24 bits) and a mistake flag (top byte).
    pub error_sync: Option<u32>,
    /// Slot 1 child 2 `selectedRewardId`: the CRC-32 of the lowercase reward id.
    pub selected_reward: Option<u32>,
    /// Slot 1 child 3 `performerScore`: per performer, the maximum points (high 16 bits) and the
    /// points earned (low 16 bits).
    pub performer_scores: Option<ReplicatedList<u32>>,
    /// Slot 1 child 4 `performanceScore`: earned / maximum.
    pub performance_score: Option<f32>,
    /// Slot 2 child 0 `performancePages`: the achievement indexes of the unlocked song pages.
    pub pages: Option<ReplicatedList<u16>>,
    /// Slot 2 child 1 `songBookFilterType`.
    pub song_book_filter: Option<u8>,
    /// Slot 2 child 2 `songBookFilterTypeId`.
    pub song_book_filter_id: Option<u32>,
    /// Slot 2 child 4 `rewardFilterType`.
    pub reward_filter: Option<u8>,
    /// Slot 2 child 5 `rewardFilterTypeId`.
    pub reward_filter_id: Option<u32>,
    /// Slot 2 child 7 `rank`: the creator's Musician rank, zero-based.
    pub rank: Option<u16>,
    /// Slot 2 child 8 `endReason`.
    pub end_reason: Option<u8>,
    /// Slots 4 to 8 child 0 `performerSeed`, one per performer place.
    pub seeds: [Option<u64>; MAX_PERFORMERS],
    /// Slot 4 child 1 `performanceZonePlayerCount`: the players in the zone.
    pub zone_players: Option<u16>,
}

impl MusicalPerformanceState {
    /// Reads a body.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, a list does not read or it sets a
    /// slot or child whose layout is not known (slot 3, the overrides).
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        let mut slots: u16 = 0;
        for slot in 0..PERFORMANCE_SLOTS {
            if slot % SLOTS_PER_BYTE == 0 {
                let left =
                    u8::try_from((PERFORMANCE_SLOTS - slot).min(SLOTS_PER_BYTE)).unwrap_or(u8::MAX);
                slots |= u16::from(read_slot_byte(reader, PERFORMANCE_FIELD, left)?) << slot;
            }
            if slots & 1 << slot == 0 {
                continue;
            }
            state.read_slot(reader, slot)?;
        }
        Ok(state)
    }

    fn read_slot(&mut self, reader: &mut Reader<'_>, slot: usize) -> Result<(), MessageError> {
        match slot {
            0 => read_children(
                reader,
                PERFORMANCE_FIELD,
                SLOT_0_CHILDREN,
                |reader, child| {
                    match child {
                        0 => {
                            self.performers =
                                Some(ReplicatedList::read(reader, PERFORMANCE_FIELD, |reader| {
                                    reader.u64()
                                })?);
                        }
                        1 => self.radius = Some(reader.f32()?),
                        2 => self.zone_type = Some(reader.u32()?),
                        3 => self.selected_song = Some(reader.u32()?),
                        4 => self.sheet_indices = Some(reader.u16()?),
                        5 => self.start_time_ns = Some(reader.u64()?),
                        6 => self.stage = Some(reader.u8()?),
                        _ => self.flags = Some(reader.u8()?),
                    }
                    Ok(())
                },
            )
            .map(drop),
            1 => read_children(
                reader,
                PERFORMANCE_FIELD,
                SLOT_1_CHILDREN,
                |reader, child| {
                    match child {
                        0 => self.leader_index = Some(reader.u8()?),
                        1 => self.error_sync = Some(reader.u32()?),
                        2 => self.selected_reward = Some(reader.u32()?),
                        3 => {
                            self.performer_scores =
                                Some(ReplicatedList::read(reader, PERFORMANCE_FIELD, |reader| {
                                    reader.u32()
                                })?);
                        }
                        _ => self.performance_score = Some(reader.f32()?),
                    }
                    Ok(())
                },
            )
            .map(drop),
            2 => read_children(
                reader,
                PERFORMANCE_FIELD,
                SLOT_2_CHILDREN,
                |reader, child| {
                    match child {
                        0 => {
                            self.pages =
                                Some(ReplicatedList::read(reader, PERFORMANCE_FIELD, |reader| {
                                    reader.u16()
                                })?);
                        }
                        1 => self.song_book_filter = Some(reader.u8()?),
                        2 => self.song_book_filter_id = Some(reader.u32()?),
                        4 => self.reward_filter = Some(reader.u8()?),
                        5 => self.reward_filter_id = Some(reader.u32()?),
                        7 => self.rank = Some(reader.u16()?),
                        8 => self.end_reason = Some(reader.u8()?),
                        unsized_child => {
                            return Err(MessageError::Unsized {
                                field: PERFORMANCE_FIELD,
                                byte: unsized_child,
                            });
                        }
                    }
                    Ok(())
                },
            )
            .map(drop),
            EMPTY_SLOT => Err(MessageError::Unsized {
                field: PERFORMANCE_FIELD,
                byte: 1 << EMPTY_SLOT,
            }),
            seed_slot => {
                let place = seed_slot - FIRST_SEED_SLOT;
                let count = if seed_slot == FIRST_SEED_SLOT {
                    FIRST_SEED_SLOT_CHILDREN
                } else {
                    SEED_SLOT_CHILDREN
                };
                read_children(reader, PERFORMANCE_FIELD, count, |reader, child| {
                    if child == 0 {
                        self.seeds[place] = Some(reader.u64()?);
                    } else {
                        self.zone_players = Some(reader.u16()?);
                    }
                    Ok(())
                })
                .map(drop)
            }
        }
    }

    /// The child bitmap of each slot.
    fn slot_children(&self) -> [u16; PERFORMANCE_SLOTS] {
        let mut children = [0; PERFORMANCE_SLOTS];
        children[0] = children_mask(&[
            self.performers.is_some(),
            self.radius.is_some(),
            self.zone_type.is_some(),
            self.selected_song.is_some(),
            self.sheet_indices.is_some(),
            self.start_time_ns.is_some(),
            self.stage.is_some(),
            self.flags.is_some(),
        ]);
        children[1] = children_mask(&[
            self.leader_index.is_some(),
            self.error_sync.is_some(),
            self.selected_reward.is_some(),
            self.performer_scores.is_some(),
            self.performance_score.is_some(),
        ]);
        children[2] = children_mask(&[
            self.pages.is_some(),
            self.song_book_filter.is_some(),
            self.song_book_filter_id.is_some(),
            false,
            self.reward_filter.is_some(),
            self.reward_filter_id.is_some(),
            false,
            self.rank.is_some(),
            self.end_reason.is_some(),
        ]);
        for (place, seed) in self.seeds.iter().enumerate() {
            let zone = place == 0 && self.zone_players.is_some();
            children[FIRST_SEED_SLOT + place] = children_mask(&[seed.is_some(), zone]);
        }
        children
    }

    /// Whether it sends nothing.
    pub fn is_empty(&self) -> bool {
        self.slot_children().iter().all(|children| *children == 0)
    }

    /// Writes the body: each slot byte, then its group's slots.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a list does not encode.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let children = self.slot_children();
        for slot in 0..PERFORMANCE_SLOTS {
            if slot % SLOTS_PER_BYTE == 0 {
                let byte = children[slot..]
                    .iter()
                    .take(SLOTS_PER_BYTE)
                    .enumerate()
                    .filter(|(_, children)| **children != 0)
                    .fold(0_u8, |byte, (bit, _)| byte | 1 << bit);
                writer.u8(byte);
            }
            if children[slot] != 0 {
                self.write_slot(writer, slot, children[slot])?;
            }
        }
        Ok(())
    }

    fn write_slot(
        &self,
        writer: &mut Writer,
        slot: usize,
        children: u16,
    ) -> Result<(), MessageError> {
        match slot {
            0 => write_children(writer, children, SLOT_0_CHILDREN, |writer, child| {
                match child {
                    0 => write_list(writer, self.performers.as_ref(), |writer, id| {
                        writer.u64(*id);
                    })?,
                    1 => put(writer, self.radius, Writer::f32),
                    2 => put(writer, self.zone_type, Writer::u32),
                    3 => put(writer, self.selected_song, Writer::u32),
                    4 => put(writer, self.sheet_indices, Writer::u16),
                    5 => put(writer, self.start_time_ns, Writer::u64),
                    6 => put(writer, self.stage, Writer::u8),
                    _ => put(writer, self.flags, Writer::u8),
                }
                Ok(())
            }),
            1 => write_children(writer, children, SLOT_1_CHILDREN, |writer, child| {
                match child {
                    0 => put(writer, self.leader_index, Writer::u8),
                    1 => put(writer, self.error_sync, Writer::u32),
                    2 => put(writer, self.selected_reward, Writer::u32),
                    3 => write_list(writer, self.performer_scores.as_ref(), |writer, score| {
                        writer.u32(*score);
                    })?,
                    _ => put(writer, self.performance_score, Writer::f32),
                }
                Ok(())
            }),
            2 => write_children(writer, children, SLOT_2_CHILDREN, |writer, child| {
                match child {
                    0 => write_list(writer, self.pages.as_ref(), |writer, page| {
                        writer.u16(*page);
                    })?,
                    1 => put(writer, self.song_book_filter, Writer::u8),
                    2 => put(writer, self.song_book_filter_id, Writer::u32),
                    4 => put(writer, self.reward_filter, Writer::u8),
                    5 => put(writer, self.reward_filter_id, Writer::u32),
                    7 => put(writer, self.rank, Writer::u16),
                    SONG_BOOK_OVERRIDES | REWARD_OVERRIDES => {}
                    _ => put(writer, self.end_reason, Writer::u8),
                }
                Ok(())
            }),
            // No child of slot 3 exists: its bitmap is always empty.
            EMPTY_SLOT => Ok(()),
            seed_slot => {
                let place = seed_slot - FIRST_SEED_SLOT;
                let count = if seed_slot == FIRST_SEED_SLOT {
                    FIRST_SEED_SLOT_CHILDREN
                } else {
                    SEED_SLOT_CHILDREN
                };
                write_children(writer, children, count, |writer, child| {
                    if child == 0 {
                        put(writer, self.seeds[place], Writer::u64);
                    } else {
                        put(writer, self.zone_players, Writer::u16);
                    }
                    Ok(())
                })
            }
        }
    }
}

/// Writes `value` with `write` when it is present.
fn put<T>(writer: &mut Writer, value: Option<T>, write: fn(&mut Writer, T) -> &mut Writer) {
    if let Some(value) = value {
        write(writer, value);
    }
}

/// Writes `list` when it is present.
fn write_list<T>(
    writer: &mut Writer,
    list: Option<&ReplicatedList<T>>,
    mut write_element: impl FnMut(&mut Writer, &T),
) -> Result<(), MessageError> {
    list.map_or(Ok(()), |list| {
        list.write(writer, |writer, element| {
            write_element(writer, element);
            Ok(())
        })
    })
}

/// A music call of the client.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MusicClientMsg {
    /// 4891: create a performance.
    CreatePerformance {
        /// The header.
        header: FacetHeader,
    },
    /// 4881: leave the performance (retail sends it twice, and after a refused create).
    LeavePerformance {
        /// The header.
        header: FacetHeader,
    },
    /// 4867: pick a song (0 for none).
    SetSong {
        /// The header.
        header: FacetHeader,
        /// The CRC-32 of the lowercase `SongId`.
        song: u32,
    },
    /// 4879: pick a sheet of the song.
    SetSheet {
        /// The header.
        header: FacetHeader,
        /// The CRC-32 of the lowercase `SongSheetId`.
        sheet: u32,
    },
    /// 4865: start the song.
    StartPerformance {
        /// The header.
        header: FacetHeader,
        /// The song's length, s (the song book's `Duration`).
        duration_s: f32,
    },
    /// 5091: the client loaded the song.
    ClientReady {
        /// The header.
        header: FacetHeader,
        /// Ready (retail 1).
        ready: bool,
    },
    /// 4892: a note the player played.
    ValidateInput {
        /// The header.
        header: FacetHeader,
        /// The chart note index, from 0.
        note: u32,
        /// The key pressed (1 to 7).
        key: u8,
        /// When, s since the song started.
        time_s: f32,
    },
    /// 5022: pick the reward of the performance.
    SetReward {
        /// The header.
        header: FacetHeader,
        /// The CRC-32 of the lowercase reward id.
        reward: u32,
    },
    /// 5233: close the post show.
    EndPostShow {
        /// The header.
        header: FacetHeader,
    },
    /// 5417: stop the song.
    AbortSong {
        /// The header.
        header: FacetHeader,
    },
}

impl MusicClientMsg {
    /// Decodes `body` when `type_index` is a music call; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let read = |read_rest: fn(FacetHeader, &mut Reader<'_>) -> Result<Self, MessageError>| {
            let mut reader = Reader::new(body);
            let message = read_rest(FacetHeader::read(&mut reader)?, &mut reader)?;
            reader.finish()?;
            Ok(message)
        };
        Some(match type_index {
            CREATE_PERFORMANCE_TYPE_INDEX => {
                read(|header, _| Ok(Self::CreatePerformance { header }))
            }
            LEAVE_PERFORMANCE_TYPE_INDEX => read(|header, _| Ok(Self::LeavePerformance { header })),
            SET_SONG_TYPE_INDEX => read(|header, reader| {
                Ok(Self::SetSong {
                    header,
                    song: reader.u32()?,
                })
            }),
            SET_SHEET_TYPE_INDEX => read(|header, reader| {
                Ok(Self::SetSheet {
                    header,
                    sheet: reader.u32()?,
                })
            }),
            START_PERFORMANCE_TYPE_INDEX => read(|header, reader| {
                Ok(Self::StartPerformance {
                    header,
                    duration_s: reader.f32()?,
                })
            }),
            CLIENT_READY_TYPE_INDEX => read(|header, reader| {
                Ok(Self::ClientReady {
                    header,
                    ready: reader.boolean()?,
                })
            }),
            VALIDATE_INPUT_TYPE_INDEX => read(|header, reader| {
                Ok(Self::ValidateInput {
                    header,
                    note: reader.u32()?,
                    key: reader.u8()?,
                    time_s: reader.f32()?,
                })
            }),
            SET_REWARD_TYPE_INDEX => read(|header, reader| {
                Ok(Self::SetReward {
                    header,
                    reward: reader.u32()?,
                })
            }),
            END_POST_SHOW_TYPE_INDEX => read(|header, _| Ok(Self::EndPostShow { header })),
            ABORT_SONG_TYPE_INDEX => read(|header, _| Ok(Self::AbortSong { header })),
            _ => return None,
        })
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::CreatePerformance { .. } => CREATE_PERFORMANCE_TYPE_INDEX,
            Self::LeavePerformance { .. } => LEAVE_PERFORMANCE_TYPE_INDEX,
            Self::SetSong { .. } => SET_SONG_TYPE_INDEX,
            Self::SetSheet { .. } => SET_SHEET_TYPE_INDEX,
            Self::StartPerformance { .. } => START_PERFORMANCE_TYPE_INDEX,
            Self::ClientReady { .. } => CLIENT_READY_TYPE_INDEX,
            Self::ValidateInput { .. } => VALIDATE_INPUT_TYPE_INDEX,
            Self::SetReward { .. } => SET_REWARD_TYPE_INDEX,
            Self::EndPostShow { .. } => END_POST_SHOW_TYPE_INDEX,
            Self::AbortSong { .. } => ABORT_SONG_TYPE_INDEX,
        }
    }

    /// The header.
    pub const fn header(&self) -> FacetHeader {
        match self {
            Self::CreatePerformance { header }
            | Self::LeavePerformance { header }
            | Self::SetSong { header, .. }
            | Self::SetSheet { header, .. }
            | Self::StartPerformance { header, .. }
            | Self::ClientReady { header, .. }
            | Self::ValidateInput { header, .. }
            | Self::SetReward { header, .. }
            | Self::EndPostShow { header }
            | Self::AbortSong { header } => *header,
        }
    }

    /// The body bytes, as the client sends them.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        self.header().write(&mut writer);
        match self {
            Self::CreatePerformance { .. }
            | Self::LeavePerformance { .. }
            | Self::EndPostShow { .. }
            | Self::AbortSong { .. } => {}
            Self::SetSong { song: id, .. }
            | Self::SetSheet { sheet: id, .. }
            | Self::SetReward { reward: id, .. } => {
                writer.u32(*id);
            }
            Self::StartPerformance { duration_s, .. } => {
                writer.f32(*duration_s);
            }
            Self::ClientReady { ready, .. } => {
                writer.boolean(*ready);
            }
            Self::ValidateInput {
                note, key, time_s, ..
            } => {
                writer.u32(*note).u8(*key).f32(*time_s);
            }
        }
        writer.finish()
    }
}

/// A music call of the server.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MusicServerMsg {
    /// 4869: the performance was created.
    PerformanceCreated {
        /// The header.
        header: FacetHeader,
        /// The performance's id: the first 8 bytes of its entity id, reversed.
        performance: u64,
        /// Created (Observed 1 every time).
        created: bool,
    },
    /// 4889: the create was refused.
    PerformanceFailed {
        /// The header.
        header: FacetHeader,
        /// Why ([`PerformanceError`]).
        error: u8,
        /// The performance zone's radius, m (the 4896 `radius`).
        radius: f32,
        /// The performance zone's type (the 4896 `zoneType`).
        zone_type: u32,
    },
}

impl MusicServerMsg {
    /// Whether `type_index` is a music call of the server.
    pub const fn is_type(type_index: u64) -> bool {
        matches!(
            type_index,
            PERFORMANCE_CREATED_TYPE_INDEX | PERFORMANCE_FAILED_TYPE_INDEX
        )
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::PerformanceCreated { .. } => PERFORMANCE_CREATED_TYPE_INDEX,
            Self::PerformanceFailed { .. } => PERFORMANCE_FAILED_TYPE_INDEX,
        }
    }

    /// Decodes the body of the call of `type_index`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not match its layout or `type_index` is not a
    /// music call ([`MessageError::BadTag`]).
    pub fn decode(type_index: u64, body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let message = match type_index {
            PERFORMANCE_CREATED_TYPE_INDEX => Self::PerformanceCreated {
                header,
                performance: reader.u64()?,
                created: reader.boolean()?,
            },
            PERFORMANCE_FAILED_TYPE_INDEX => Self::PerformanceFailed {
                header,
                error: reader.u8()?,
                radius: reader.f32()?,
                zone_type: reader.u32()?,
            },
            _ => {
                return Err(MessageError::BadTag {
                    field: PLAYER_FIELD,
                    tag: u8::try_from(type_index).unwrap_or(u8::MAX),
                });
            }
        };
        reader.finish()?;
        Ok(message)
    }

    /// The body bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Self::PerformanceCreated {
                header,
                performance,
                created,
            } => {
                header.write(&mut writer);
                writer.u64(*performance).boolean(*created);
            }
            Self::PerformanceFailed {
                header,
                error,
                radius,
                zone_type,
            } => {
                header.write(&mut writer);
                writer.u8(*error).f32(*radius).u32(*zone_type);
            }
        }
        writer.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::FullList;
    use crate::list_delta::ListChange;

    /// A header (synthetic ids).
    const HEADER: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    /// A performance id and a performer entity (synthetic).
    const PERFORMANCE: u64 = 0x1122_3344_5566_7788;
    const PERFORMER: u64 = 0x0102_0304_0506_0708;
    /// Retail 4878 bodies of `open-world/Music`: the new character's, the create (20533 ms), the
    /// leave (101381 ms) and the lockout's end (111447 ms).
    const RETAIL_NEW: [u8; 20] = [
        0x03, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0x06, 0x00, 0x00, 0x00, 0x00,
        0x00, 0xff, 0xff, 0xff, 0xff,
    ];
    const RETAIL_CREATE: [u8; 3] = [0x02, 0x02, 0x01];
    const RETAIL_LEAVE: [u8; 14] = [
        0x03, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0x03, 0x00, 0x28, 0x00,
    ];
    const RETAIL_LOCKOUT_END: [u8; 4] = [0x02, 0x01, 0x00, 0x00];
    const RETAIL_LOCKOUT: u16 = 40;
    /// Retail 4896 changes of `open-world/Music`: the results (91415 ms: state 6, `errorSync` 7,
    /// performer 0 scored 624 of 630 at list version 12, score 0.990476) and the end (101414 ms:
    /// performer 0 gone at version 16, no sheet, state 9).
    const RETAIL_RESULTS: [u8; 21] = [
        0x03, 0x40, 0x06, 0x1a, 0x00, 0x00, 0x00, 0x07, 0x01, 0x01, 0x00, 0x01, 0x0c, 0x02, 0x76,
        0x02, 0x70, 0x3f, 0x7d, 0x8f, 0xd9,
    ];
    const RETAIL_ENDING: [u8; 18] = [
        0x01, 0x51, 0x01, 0x01, 0x00, 0x01, 0x10, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
        0x00, 0x00, 0x09,
    ];
    const SECOND_SLOT_BYTE: u8 = 0x00;
    const SCORE_VERSION: u64 = 12;
    const ENDING_VERSION: u64 = 16;
    const SCORE: u32 = 0x0276_0270;
    /// The 4889 of a create without an instrument (`open-world/Music`, 10582 ms).
    const RETAIL_FAILED: [u8; 9] = [0x1b, 0x41, 0xa0, 0x00, 0x00, 0x8e, 0x55, 0x58, 0xd9];
    const RETAIL_RADIUS: f32 = 20.0;
    const RETAIL_ZONE_TYPE: u32 = 0x8e55_58d9;
    /// Retail 4892 arguments (39765 ms): note 0, key 6, 5.94 s.
    const RETAIL_INPUT: [u8; 9] = [0x00, 0x00, 0x00, 0x00, 0x06, 0x40, 0xbe, 0x03, 0xff];
    /// The CRC-32 of `poundhammereasy` (retail 4867).
    const POUND_HAMMER: u32 = 0x16d9_564e;

    fn body(arguments: &[u8]) -> Vec<u8> {
        [&HEADER[..], arguments].concat()
    }

    fn header() -> FacetHeader {
        FacetHeader::read(&mut Reader::new(&HEADER)).expect("header")
    }

    fn player(bytes: &[u8]) -> PerformancePlayerState {
        let mut reader = Reader::new(bytes);
        let state = PerformancePlayerState::read(&mut reader).expect("read");
        reader.finish().expect("whole");
        state
    }

    fn player_bytes(state: &PerformancePlayerState) -> Vec<u8> {
        let mut writer = Writer::new();
        state.write(&mut writer);
        writer.finish()
    }

    fn performance(bytes: &[u8]) -> MusicalPerformanceState {
        let mut reader = Reader::new(bytes);
        let state = MusicalPerformanceState::read(&mut reader).expect("read");
        reader.finish().expect("whole");
        state
    }

    fn performance_bytes(state: &MusicalPerformanceState) -> Vec<u8> {
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        writer.finish()
    }

    #[test]
    fn the_retail_player_states_read_and_write_back() {
        assert_eq!(player(&RETAIL_NEW), PerformancePlayerState::NONE);
        assert_eq!(player_bytes(&player(&RETAIL_NEW)), RETAIL_NEW);
        let created = PerformancePlayerState {
            performing: Some(1),
            ..PerformancePlayerState::default()
        };
        assert_eq!(player(&RETAIL_CREATE), created);
        assert_eq!(player_bytes(&created), RETAIL_CREATE);
        let left = PerformancePlayerState {
            performance: Some(NO_PERFORMANCE),
            lockout: Some(RETAIL_LOCKOUT),
            performing: Some(0),
            observing: None,
        };
        assert_eq!(player(&RETAIL_LEAVE), left);
        assert_eq!(player_bytes(&left), RETAIL_LEAVE);
        let ended = PerformancePlayerState {
            lockout: Some(0),
            ..PerformancePlayerState::default()
        };
        assert_eq!(player(&RETAIL_LOCKOUT_END), ended);
        assert_eq!(player_bytes(&ended), RETAIL_LOCKOUT_END);
        assert_eq!(player_bytes(&PerformancePlayerState::default()), [0x00]);
        assert!(PerformancePlayerState::default().is_empty());
        assert!(PerformancePlayerState::read(&mut Reader::new(&[0x04])).is_err());
    }

    #[test]
    fn the_retail_performance_changes_read_and_write_back() {
        let results_bytes = [&RETAIL_RESULTS[..], &[SECOND_SLOT_BYTE]].concat();
        let results = MusicalPerformanceState {
            stage: Some(PerformanceStage::Results as u8),
            error_sync: Some(7),
            performer_scores: Some(ReplicatedList::Changes(vec![ListChange {
                index: 0,
                version: SCORE_VERSION,
                value: SCORE,
            }])),
            performance_score: Some(f32::from_bits(0x3f7d_8fd9)),
            ..MusicalPerformanceState::default()
        };
        assert_eq!(performance(&results_bytes), results);
        assert_eq!(performance_bytes(&results), results_bytes);
        let ending_bytes = [&RETAIL_ENDING[..], &[SECOND_SLOT_BYTE]].concat();
        let ending = MusicalPerformanceState {
            performers: Some(ReplicatedList::Changes(vec![ListChange {
                index: 0,
                version: ENDING_VERSION,
                value: NO_PERFORMANCE,
            }])),
            sheet_indices: Some(0),
            stage: Some(PerformanceStage::Ending as u8),
            ..MusicalPerformanceState::default()
        };
        assert_eq!(performance(&ending_bytes), ending);
        assert_eq!(performance_bytes(&ending), ending_bytes);
    }

    #[test]
    fn a_whole_performance_state_with_every_slot_writes_back() {
        let whole = MusicalPerformanceState {
            performers: Some(ReplicatedList::Full(FullList::first(vec![
                PERFORMER,
                NO_PERFORMANCE,
                NO_PERFORMANCE,
                NO_PERFORMANCE,
                NO_PERFORMANCE,
            ]))),
            radius: Some(RETAIL_RADIUS),
            zone_type: Some(RETAIL_ZONE_TYPE),
            selected_song: Some(POUND_HAMMER),
            sheet_indices: Some(1),
            start_time_ns: Some(PERFORMANCE),
            stage: Some(PerformanceStage::WaitingToJoin as u8),
            flags: Some(1),
            leader_index: Some(0),
            error_sync: Some(1),
            selected_reward: Some(POUND_HAMMER),
            performer_scores: Some(ReplicatedList::Full(FullList::first(vec![
                0;
                MAX_PERFORMERS
            ]))),
            performance_score: Some(0.0),
            pages: Some(ReplicatedList::Full(FullList::first(vec![4514, 4661]))),
            song_book_filter: Some(2),
            song_book_filter_id: Some(3),
            reward_filter: Some(2),
            reward_filter_id: Some(4),
            rank: Some(219),
            end_reason: Some(0),
            seeds: [Some(0x6898), None, None, None, Some(7)],
            zone_players: Some(1),
        };
        let bytes = performance_bytes(&whole);
        assert_eq!(performance(&bytes), whole);
        assert_eq!(bytes[0], 0b0001_0111, "slots 0, 1, 2 and 4");
        assert_eq!(bytes.last(), Some(&7), "the last seed");
        assert!(MusicalPerformanceState::default().is_empty());
        assert_eq!(
            performance_bytes(&MusicalPerformanceState::default()),
            [0x00, 0x00]
        );
        assert!(
            MusicalPerformanceState::read(&mut Reader::new(&[0x08])).is_err(),
            "slot 3 has no children"
        );
    }

    #[test]
    fn the_retail_calls_read_and_write_back() {
        let input = MusicClientMsg::decode(VALIDATE_INPUT_TYPE_INDEX, &body(&RETAIL_INPUT))
            .expect("a music call")
            .expect("reads");
        let MusicClientMsg::ValidateInput {
            note, key, time_s, ..
        } = input
        else {
            panic!("an input");
        };
        assert_eq!((note, key), (0, 6));
        assert!((time_s - 5.938).abs() < 1e-3);
        assert_eq!(input.encode(), body(&RETAIL_INPUT));
        let song = MusicClientMsg::decode(SET_SONG_TYPE_INDEX, &body(&POUND_HAMMER.to_be_bytes()))
            .expect("a music call")
            .expect("reads");
        assert_eq!(
            song,
            MusicClientMsg::SetSong {
                header: header(),
                song: POUND_HAMMER,
            }
        );
        assert_eq!(song.type_index(), SET_SONG_TYPE_INDEX);
        for (type_index, empty) in [
            (
                CREATE_PERFORMANCE_TYPE_INDEX,
                MusicClientMsg::CreatePerformance { header: header() },
            ),
            (
                LEAVE_PERFORMANCE_TYPE_INDEX,
                MusicClientMsg::LeavePerformance { header: header() },
            ),
            (
                END_POST_SHOW_TYPE_INDEX,
                MusicClientMsg::EndPostShow { header: header() },
            ),
            (
                ABORT_SONG_TYPE_INDEX,
                MusicClientMsg::AbortSong { header: header() },
            ),
        ] {
            let decoded = MusicClientMsg::decode(type_index, &HEADER)
                .expect("a music call")
                .expect("reads");
            assert_eq!(decoded, empty);
            assert_eq!(decoded.encode(), HEADER);
        }
        let ready = MusicClientMsg::decode(CLIENT_READY_TYPE_INDEX, &body(&[1]))
            .expect("a music call")
            .expect("reads");
        assert_eq!(ready.encode(), body(&[1]));
        assert!(MusicClientMsg::decode(1, &HEADER).is_none());
        assert!(matches!(
            MusicClientMsg::decode(CLIENT_READY_TYPE_INDEX, &body(&[2])),
            Some(Err(_))
        ));
    }

    #[test]
    fn the_retail_server_calls_read_and_write_back() {
        let failed = MusicServerMsg::PerformanceFailed {
            header: header(),
            error: PerformanceError::CreateFailedNoInstrument as u8,
            radius: RETAIL_RADIUS,
            zone_type: RETAIL_ZONE_TYPE,
        };
        assert_eq!(failed.encode(), body(&RETAIL_FAILED));
        assert_eq!(
            MusicServerMsg::decode(PERFORMANCE_FAILED_TYPE_INDEX, &body(&RETAIL_FAILED)),
            Ok(failed)
        );
        let created = MusicServerMsg::PerformanceCreated {
            header: header(),
            performance: PERFORMANCE,
            created: true,
        };
        assert_eq!(
            MusicServerMsg::decode(PERFORMANCE_CREATED_TYPE_INDEX, &created.encode()),
            Ok(created)
        );
        assert_eq!(created.encode().len(), HEADER.len() + 9);
        assert!(MusicServerMsg::is_type(PERFORMANCE_CREATED_TYPE_INDEX));
        assert!(!MusicServerMsg::is_type(VALIDATE_INPUT_TYPE_INDEX));
        assert_eq!(PerformanceStage::of(9), Some(PerformanceStage::Ending));
        assert_eq!(PerformanceStage::of(10), None);
    }
}
