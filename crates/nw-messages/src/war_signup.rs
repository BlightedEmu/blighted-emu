// SPDX-License-Identifier: AGPL-3.0-or-later
//! War sign-up and roster calls of the player's `TerritoryInteractorComponent`, with their
//! answers (wars and sieges study 2026-11-12: every call and answer of the capture index;
//! ledgers `army-management`, `banned_from_warleader_to_join_war`, `aasdf`,
//! `sign_up_for_invasion`, `RS_offensive_war`, `joining_defending_war_2`).
//!
//! Every body starts with a facet header. After the header:
//!
//! - 2762 sign-up: the side `u32` ([`SIDE_ATTACKER`] or [`SIDE_DEFENDER`]; an invasion's
//!   sign-up is always the defender), then the territory `u16`.
//! - 2767 list and 2771 count: the side `u32`. 2772 roster: the header alone.
//! - 2784 leave and 2791 remote board use: the territory `u16`.
//! - 2779 add to roster: a [`WarPlayer`], then the group `u32`.
//! - 2781 remove from roster: a [`WarPlayer`], its group and slot `u32`.
//! - 2782 set permissions: a [`WarPlayer`], its group and slot `u32`, the permission `u8`.
//! - 1871 swap: two (player, group, slot).
//! - 1676 move: a [`WarPlayer`], its group and slot, the target group (Inferred order: the
//!   three retail calls were refused, so no roster change shows which `u32` is which).
//! - 1892 kick: a [`WarPlayer`].
//!
//! Answers:
//!
//! - 2169 to a sign-up, 2733 to a roster call: [`CallResult`] (Observed: `01 00000000` for a
//!   done call; `00 0000000c` for a player the war leader banned, `00 0000000e` for a refused
//!   swap or move).
//! - 2230 to a leave: the territory `u16`, then a [`CallResult`].
//! - 2726 to a list: the side `u32`, a second `u32` (Observed: the side again in 41 of 44
//!   bodies, 4 in the rest; meaning Unknown), the entry count, then per signed-up player a
//!   [`WarPlayer`], its company id and its gear score `u16`.
//! - 2729 to a count: the side `u32` and the count `u32`.

use crate::facet::FacetHeader;
use crate::war_states::{read_war_text, write_war_text};
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// Type index of `TerritoryInteractorComponentServerFacet_RequestSignupList`.
pub const REQUEST_SIGNUP_LIST_TYPE_INDEX: u64 = 2767;
/// Type index of `TerritoryInteractorComponentClientFacet_OnSignupListReceived`.
pub const SIGNUP_LIST_RECEIVED_TYPE_INDEX: u64 = 2726;
/// Type index of `TerritoryInteractorComponentServerFacet_RequestRoster`.
pub const REQUEST_ROSTER_TYPE_INDEX: u64 = 2772;
/// Type index of `TerritoryInteractorComponentServerFacet_RequestRemoteInteract`.
pub const REQUEST_REMOTE_INTERACT_TYPE_INDEX: u64 = 2791;
/// Type index of `TerritoryInteractorComponentServerFacet_RequestSignupCount`.
pub const REQUEST_SIGNUP_COUNT_TYPE_INDEX: u64 = 2771;
/// Type index of `TerritoryInteractorComponentClientFacet_OnSignupCountReceived`.
pub const SIGNUP_COUNT_RECEIVED_TYPE_INDEX: u64 = 2729;
/// Type index of `TerritoryInteractorComponentServerFacet_RequestSignup`.
pub const REQUEST_SIGNUP_TYPE_INDEX: u64 = 2762;
/// Type index of `TerritoryInteractorComponentClientFacet_OnSignupResponseReceived`.
pub const SIGNUP_RESPONSE_RECEIVED_TYPE_INDEX: u64 = 2169;
/// Type index of `TerritoryInteractorComponentServerFacet_RequestLeave`.
pub const REQUEST_LEAVE_TYPE_INDEX: u64 = 2784;
/// Type index of `TerritoryInteractorComponentClientFacet_OnLeaveResponseReceived`.
pub const LEAVE_RESPONSE_RECEIVED_TYPE_INDEX: u64 = 2230;
/// Type index of `TerritoryInteractorComponentServerFacet_RequestAddPlayerToRoster`.
pub const REQUEST_ADD_TO_ROSTER_TYPE_INDEX: u64 = 2779;
/// Type index of `TerritoryInteractorComponentServerFacet_RequestRemovePlayerFromRoster`.
pub const REQUEST_REMOVE_FROM_ROSTER_TYPE_INDEX: u64 = 2781;
/// Type index of `TerritoryInteractorComponentServerFacet_RequestSetPlayerPermissions`.
pub const REQUEST_SET_PERMISSIONS_TYPE_INDEX: u64 = 2782;
/// Type index of `TerritoryInteractorComponentServerFacet_RequestSwapPlayers`.
pub const REQUEST_SWAP_PLAYERS_TYPE_INDEX: u64 = 1871;
/// Type index of `TerritoryInteractorComponentServerFacet_RequestMovePlayer`.
pub const REQUEST_MOVE_PLAYER_TYPE_INDEX: u64 = 1676;
/// Type index of `TerritoryInteractorComponentServerFacet_RequestKickPlayer`.
pub const REQUEST_KICK_PLAYER_TYPE_INDEX: u64 = 1892;
/// Type index of `TerritoryInteractorComponentClientFacet_OnSetRosterResponseReceived`.
pub const SET_ROSTER_RESPONSE_RECEIVED_TYPE_INDEX: u64 = 2733;

/// The attacker side of a war (Observed: 2762 side 1 on the attackers' board).
pub const SIDE_ATTACKER: u32 = 1;
/// The defender side of a war or an invasion (Observed: every invasion sign-up is side 2).
pub const SIDE_DEFENDER: u32 = 2;
/// The kind byte of every retail [`WarPlayer`] (Observed: 2; meaning Unknown).
pub const WAR_PLAYER_KIND: u8 = 2;
/// The result byte of a done call (Observed).
pub const RESULT_DONE: u8 = 1;
/// The result byte of a refused call (Observed).
pub const RESULT_REFUSED: u8 = 0;
/// The reason of a done call (Observed).
pub const REASON_NONE: u32 = 0;
/// The reason of a sign-up of a player the war leader banned (Observed:
/// `banned_from_warleader_to_join_war` 1781978506963).
pub const REASON_BANNED: u32 = 12;
/// The reason of a refused roster swap or move (Observed: `joining_defending_war_2`
/// 1781898776428; the client's text for it is Unknown).
pub const REASON_ROSTER_REFUSED: u32 = 14;

/// The most sign-ups of one 2726 (an entry is at least 21 bytes).
const MAX_SIGNUPS: u64 = 0x1_0000;

/// A player of a sign-up list or a roster: its character id as text (a UUID), its name and a
/// byte (Observed: [`WAR_PLAYER_KIND`]). An empty roster slot has two empty texts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WarPlayer {
    /// The character id as text.
    pub character: String,
    /// The character name.
    pub name: String,
    /// The byte after the name.
    pub kind: u8,
}

impl WarPlayer {
    pub(crate) fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            character: read_war_text(reader)?,
            name: read_war_text(reader)?,
            kind: reader.u8()?,
        })
    }

    pub(crate) fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        write_war_text(writer, &self.character)?;
        write_war_text(writer, &self.name)?;
        writer.u8(self.kind);
        Ok(())
    }
}

/// A roster place: a group (0 to 9) and a slot in it (0 to 4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RosterPlace {
    /// The group.
    pub group: u32,
    /// The slot.
    pub slot: u32,
}

impl RosterPlace {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            group: reader.u32()?,
            slot: reader.u32()?,
        })
    }

    fn write(self, writer: &mut Writer) {
        writer.u32(self.group).u32(self.slot);
    }
}

/// One signed-up player of a 2726.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignupEntry {
    /// The player.
    pub player: WarPlayer,
    /// Its company id (zero for a player in none).
    pub company: [u8; RAW_ID_LEN],
    /// Its gear score (Observed: 781 to 800).
    pub gear_score: u16,
}

/// The result of a sign-up or roster call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallResult {
    /// [`RESULT_DONE`] or [`RESULT_REFUSED`].
    pub result: u8,
    /// The reason ([`REASON_NONE`] for a done call).
    pub reason: u32,
}

impl CallResult {
    /// A done call.
    pub const DONE: Self = Self {
        result: RESULT_DONE,
        reason: REASON_NONE,
    };

    /// A refused call for `reason`.
    pub const fn refused(reason: u32) -> Self {
        Self {
            result: RESULT_REFUSED,
            reason,
        }
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            result: reader.u8()?,
            reason: reader.u32()?,
        })
    }

    fn write(self, writer: &mut Writer) {
        writer.u8(self.result).u32(self.reason);
    }
}

/// A war sign-up or roster call of the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarSignupClientMsg {
    /// 2767: send the sign-up list of this side.
    RequestSignupList {
        /// The header.
        header: FacetHeader,
        /// The side.
        side: u32,
    },
    /// 2772: send the current war roster.
    RequestRoster {
        /// The header.
        header: FacetHeader,
    },
    /// 2791: use a territory board from far away.
    RequestRemoteInteract {
        /// The header.
        header: FacetHeader,
        /// The territory.
        territory: u16,
    },
    /// 2771: send how many signed up for this side.
    RequestSignupCount {
        /// The header.
        header: FacetHeader,
        /// The side.
        side: u32,
    },
    /// 2762: sign the player up for this side of this territory's war.
    RequestSignup {
        /// The header.
        header: FacetHeader,
        /// The side.
        side: u32,
        /// The territory.
        territory: u16,
    },
    /// 2784: take the player off this territory's war sign-up.
    RequestLeave {
        /// The header.
        header: FacetHeader,
        /// The territory.
        territory: u16,
    },
    /// 2779: put a signed-up player into a group of the roster.
    AddToRoster {
        /// The header.
        header: FacetHeader,
        /// The player.
        player: WarPlayer,
        /// The group.
        group: u32,
    },
    /// 2781: take a player off the roster.
    RemoveFromRoster {
        /// The header.
        header: FacetHeader,
        /// The player.
        player: WarPlayer,
        /// Its place.
        place: RosterPlace,
    },
    /// 2782: set a roster player's permission.
    SetPermissions {
        /// The header.
        header: FacetHeader,
        /// The player.
        player: WarPlayer,
        /// Its place.
        place: RosterPlace,
        /// The permission (Observed: 0, 1 and 2).
        permission: u8,
    },
    /// 1871: swap two roster players.
    SwapPlayers {
        /// The header.
        header: FacetHeader,
        /// The first player and its place.
        first: (WarPlayer, RosterPlace),
        /// The second player and its place.
        second: (WarPlayer, RosterPlace),
    },
    /// 1676: move a roster player into another group.
    MovePlayer {
        /// The header.
        header: FacetHeader,
        /// The player.
        player: WarPlayer,
        /// Its place.
        place: RosterPlace,
        /// The target group.
        group: u32,
    },
    /// 1892: kick a player out of the war.
    KickPlayer {
        /// The header.
        header: FacetHeader,
        /// The player.
        player: WarPlayer,
    },
}

impl WarSignupClientMsg {
    /// Decodes `body` when `type_index` is a war sign-up or roster call; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let read = |read_rest: fn(FacetHeader, &mut Reader<'_>) -> Result<Self, MessageError>| {
            let mut reader = Reader::new(body);
            let message = read_rest(FacetHeader::read(&mut reader)?, &mut reader)?;
            reader.finish()?;
            Ok(message)
        };
        Some(match type_index {
            REQUEST_SIGNUP_LIST_TYPE_INDEX => read(|header, reader| {
                Ok(Self::RequestSignupList {
                    header,
                    side: reader.u32()?,
                })
            }),
            REQUEST_ROSTER_TYPE_INDEX => read(|header, _| Ok(Self::RequestRoster { header })),
            REQUEST_REMOTE_INTERACT_TYPE_INDEX => read(|header, reader| {
                Ok(Self::RequestRemoteInteract {
                    header,
                    territory: reader.u16()?,
                })
            }),
            REQUEST_SIGNUP_COUNT_TYPE_INDEX => read(|header, reader| {
                Ok(Self::RequestSignupCount {
                    header,
                    side: reader.u32()?,
                })
            }),
            REQUEST_SIGNUP_TYPE_INDEX => read(|header, reader| {
                Ok(Self::RequestSignup {
                    header,
                    side: reader.u32()?,
                    territory: reader.u16()?,
                })
            }),
            REQUEST_LEAVE_TYPE_INDEX => read(|header, reader| {
                Ok(Self::RequestLeave {
                    header,
                    territory: reader.u16()?,
                })
            }),
            REQUEST_ADD_TO_ROSTER_TYPE_INDEX => read(|header, reader| {
                Ok(Self::AddToRoster {
                    header,
                    player: WarPlayer::read(reader)?,
                    group: reader.u32()?,
                })
            }),
            REQUEST_REMOVE_FROM_ROSTER_TYPE_INDEX => read(|header, reader| {
                Ok(Self::RemoveFromRoster {
                    header,
                    player: WarPlayer::read(reader)?,
                    place: RosterPlace::read(reader)?,
                })
            }),
            REQUEST_SET_PERMISSIONS_TYPE_INDEX => read(|header, reader| {
                Ok(Self::SetPermissions {
                    header,
                    player: WarPlayer::read(reader)?,
                    place: RosterPlace::read(reader)?,
                    permission: reader.u8()?,
                })
            }),
            REQUEST_SWAP_PLAYERS_TYPE_INDEX => read(|header, reader| {
                Ok(Self::SwapPlayers {
                    header,
                    first: (WarPlayer::read(reader)?, RosterPlace::read(reader)?),
                    second: (WarPlayer::read(reader)?, RosterPlace::read(reader)?),
                })
            }),
            REQUEST_MOVE_PLAYER_TYPE_INDEX => read(|header, reader| {
                Ok(Self::MovePlayer {
                    header,
                    player: WarPlayer::read(reader)?,
                    place: RosterPlace::read(reader)?,
                    group: reader.u32()?,
                })
            }),
            REQUEST_KICK_PLAYER_TYPE_INDEX => read(|header, reader| {
                Ok(Self::KickPlayer {
                    header,
                    player: WarPlayer::read(reader)?,
                })
            }),
            _ => return None,
        })
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::RequestSignupList { .. } => REQUEST_SIGNUP_LIST_TYPE_INDEX,
            Self::RequestRoster { .. } => REQUEST_ROSTER_TYPE_INDEX,
            Self::RequestRemoteInteract { .. } => REQUEST_REMOTE_INTERACT_TYPE_INDEX,
            Self::RequestSignupCount { .. } => REQUEST_SIGNUP_COUNT_TYPE_INDEX,
            Self::RequestSignup { .. } => REQUEST_SIGNUP_TYPE_INDEX,
            Self::RequestLeave { .. } => REQUEST_LEAVE_TYPE_INDEX,
            Self::AddToRoster { .. } => REQUEST_ADD_TO_ROSTER_TYPE_INDEX,
            Self::RemoveFromRoster { .. } => REQUEST_REMOVE_FROM_ROSTER_TYPE_INDEX,
            Self::SetPermissions { .. } => REQUEST_SET_PERMISSIONS_TYPE_INDEX,
            Self::SwapPlayers { .. } => REQUEST_SWAP_PLAYERS_TYPE_INDEX,
            Self::MovePlayer { .. } => REQUEST_MOVE_PLAYER_TYPE_INDEX,
            Self::KickPlayer { .. } => REQUEST_KICK_PLAYER_TYPE_INDEX,
        }
    }

    /// The header of the call.
    pub const fn header(&self) -> FacetHeader {
        match self {
            Self::RequestSignupList { header, .. }
            | Self::RequestRoster { header }
            | Self::RequestRemoteInteract { header, .. }
            | Self::RequestSignupCount { header, .. }
            | Self::RequestSignup { header, .. }
            | Self::RequestLeave { header, .. }
            | Self::AddToRoster { header, .. }
            | Self::RemoveFromRoster { header, .. }
            | Self::SetPermissions { header, .. }
            | Self::SwapPlayers { header, .. }
            | Self::MovePlayer { header, .. }
            | Self::KickPlayer { header, .. } => *header,
        }
    }

    /// The body bytes, as the client sends them.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a text is too long.
    pub fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header().write(&mut writer);
        match self {
            Self::RequestSignupList { side, .. } | Self::RequestSignupCount { side, .. } => {
                writer.u32(*side);
            }
            Self::RequestRoster { .. } => {}
            Self::RequestRemoteInteract { territory, .. }
            | Self::RequestLeave { territory, .. } => {
                writer.u16(*territory);
            }
            Self::RequestSignup {
                side, territory, ..
            } => {
                writer.u32(*side).u16(*territory);
            }
            Self::AddToRoster { player, group, .. } => {
                player.write(&mut writer)?;
                writer.u32(*group);
            }
            Self::RemoveFromRoster { player, place, .. } => {
                player.write(&mut writer)?;
                place.write(&mut writer);
            }
            Self::SetPermissions {
                player,
                place,
                permission,
                ..
            } => {
                player.write(&mut writer)?;
                place.write(&mut writer);
                writer.u8(*permission);
            }
            Self::SwapPlayers { first, second, .. } => {
                for (player, place) in [first, second] {
                    player.write(&mut writer)?;
                    place.write(&mut writer);
                }
            }
            Self::MovePlayer {
                player,
                place,
                group,
                ..
            } => {
                player.write(&mut writer)?;
                place.write(&mut writer);
                writer.u32(*group);
            }
            Self::KickPlayer { player, .. } => player.write(&mut writer)?,
        }
        Ok(writer.finish())
    }
}

/// A war sign-up or roster answer of the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarSignupServerMsg {
    /// 2726: the players who signed up for a side.
    SignupListReceived {
        /// The header.
        header: FacetHeader,
        /// The side of the 2767 it answers.
        side: u32,
        /// A second value (module doc).
        extra: u32,
        /// The signed-up players.
        entries: Vec<SignupEntry>,
    },
    /// 2729: how many signed up for a side.
    SignupCountReceived {
        /// The header.
        header: FacetHeader,
        /// The side of the 2771 it answers.
        side: u32,
        /// How many signed up.
        count: u32,
    },
    /// 2169: the answer to a 2762 sign-up.
    SignupResponseReceived {
        /// The header.
        header: FacetHeader,
        /// The result.
        result: CallResult,
    },
    /// 2230: the answer to a 2784 leave.
    LeaveResponseReceived {
        /// The header.
        header: FacetHeader,
        /// The territory of the leave.
        territory: u16,
        /// The result.
        result: CallResult,
    },
    /// 2733: the answer to a roster call.
    SetRosterResponseReceived {
        /// The header.
        header: FacetHeader,
        /// The result.
        result: CallResult,
    },
}

impl WarSignupServerMsg {
    /// Whether `type_index` is a war sign-up or roster answer.
    pub const fn is_type(type_index: u64) -> bool {
        matches!(
            type_index,
            SIGNUP_LIST_RECEIVED_TYPE_INDEX
                | SIGNUP_COUNT_RECEIVED_TYPE_INDEX
                | SIGNUP_RESPONSE_RECEIVED_TYPE_INDEX
                | LEAVE_RESPONSE_RECEIVED_TYPE_INDEX
                | SET_ROSTER_RESPONSE_RECEIVED_TYPE_INDEX
        )
    }

    /// The type index of the answer.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::SignupListReceived { .. } => SIGNUP_LIST_RECEIVED_TYPE_INDEX,
            Self::SignupCountReceived { .. } => SIGNUP_COUNT_RECEIVED_TYPE_INDEX,
            Self::SignupResponseReceived { .. } => SIGNUP_RESPONSE_RECEIVED_TYPE_INDEX,
            Self::LeaveResponseReceived { .. } => LEAVE_RESPONSE_RECEIVED_TYPE_INDEX,
            Self::SetRosterResponseReceived { .. } => SET_ROSTER_RESPONSE_RECEIVED_TYPE_INDEX,
        }
    }

    /// The header of the answer.
    pub const fn header(&self) -> FacetHeader {
        match self {
            Self::SignupListReceived { header, .. }
            | Self::SignupCountReceived { header, .. }
            | Self::SignupResponseReceived { header, .. }
            | Self::LeaveResponseReceived { header, .. }
            | Self::SetRosterResponseReceived { header, .. } => *header,
        }
    }

    /// Decodes the body of the answer of `type_index`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not match its layout or `type_index` is not
    /// a war sign-up or roster answer.
    pub fn decode(type_index: u64, body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let message = match type_index {
            SIGNUP_LIST_RECEIVED_TYPE_INDEX => {
                let side = reader.u32()?;
                let extra = reader.u32()?;
                let count = reader.prefixed()?;
                if count > MAX_SIGNUPS {
                    return Err(MessageError::CountTooLarge {
                        count,
                        limit: MAX_SIGNUPS,
                    });
                }
                let entries = (0..count)
                    .map(|_| {
                        Ok(SignupEntry {
                            player: WarPlayer::read(&mut reader)?,
                            company: reader.raw_id()?,
                            gear_score: reader.u16()?,
                        })
                    })
                    .collect::<Result<_, MessageError>>()?;
                Self::SignupListReceived {
                    header,
                    side,
                    extra,
                    entries,
                }
            }
            SIGNUP_COUNT_RECEIVED_TYPE_INDEX => Self::SignupCountReceived {
                header,
                side: reader.u32()?,
                count: reader.u32()?,
            },
            SIGNUP_RESPONSE_RECEIVED_TYPE_INDEX => Self::SignupResponseReceived {
                header,
                result: CallResult::read(&mut reader)?,
            },
            LEAVE_RESPONSE_RECEIVED_TYPE_INDEX => Self::LeaveResponseReceived {
                header,
                territory: reader.u16()?,
                result: CallResult::read(&mut reader)?,
            },
            SET_ROSTER_RESPONSE_RECEIVED_TYPE_INDEX => Self::SetRosterResponseReceived {
                header,
                result: CallResult::read(&mut reader)?,
            },
            _ => {
                return Err(MessageError::BadTag {
                    field: "war sign-up",
                    tag: u8::try_from(type_index).unwrap_or(u8::MAX),
                });
            }
        };
        reader.finish()?;
        Ok(message)
    }

    /// The body bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a text is too long or the list is too large.
    pub fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header().write(&mut writer);
        match self {
            Self::SignupListReceived {
                side,
                extra,
                entries,
                ..
            } => {
                writer
                    .u32(*side)
                    .u32(*extra)
                    .prefixed(entries.len() as u64)?;
                for entry in entries {
                    entry.player.write(&mut writer)?;
                    writer.bytes(&entry.company).u16(entry.gear_score);
                }
            }
            Self::SignupCountReceived { side, count, .. } => {
                writer.u32(*side).u32(*count);
            }
            Self::SignupResponseReceived { result, .. }
            | Self::SetRosterResponseReceived { result, .. } => result.write(&mut writer),
            Self::LeaveResponseReceived {
                territory, result, ..
            } => {
                writer.u16(*territory);
                result.write(&mut writer);
            }
        }
        Ok(writer.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::tests::hex;

    /// A made-up territory interactor header.
    const HEADER: &str = "0102030405060708090a0b0c0d0e0f10";
    /// A made-up player in the retail form: a UUID text, the name "Testa", kind 2.
    const PLAYER: &str =
        "2431313131313131312d323232322d333333332d343434342d35353535353535353535353505546573746102";

    fn client(type_index: u64, args: &str) -> WarSignupClientMsg {
        WarSignupClientMsg::decode(type_index, &hex(&format!("{HEADER}{args}")))
            .expect("a sign-up call")
            .expect("reads")
    }

    /// The retail forms of every call (`army-management`, `joining_defending_war_2`,
    /// `banned_from_warleader_to_join_war`), with a made-up header and player.
    #[test]
    fn the_client_calls_read_and_write_back() {
        let player = PLAYER;
        for (type_index, args) in [
            (REQUEST_SIGNUP_LIST_TYPE_INDEX, "00000001".to_owned()),
            (REQUEST_ROSTER_TYPE_INDEX, String::new()),
            (REQUEST_REMOTE_INTERACT_TYPE_INDEX, "000b".to_owned()),
            (REQUEST_SIGNUP_COUNT_TYPE_INDEX, "00000002".to_owned()),
            (REQUEST_SIGNUP_TYPE_INDEX, "00000001000b".to_owned()),
            (REQUEST_LEAVE_TYPE_INDEX, "000d".to_owned()),
            (
                REQUEST_ADD_TO_ROSTER_TYPE_INDEX,
                format!("{player}00000008"),
            ),
            (
                REQUEST_REMOVE_FROM_ROSTER_TYPE_INDEX,
                format!("{player}0000000800000003"),
            ),
            (
                REQUEST_SET_PERMISSIONS_TYPE_INDEX,
                format!("{player}000000080000000302"),
            ),
            (
                REQUEST_SWAP_PLAYERS_TYPE_INDEX,
                format!("{player}0000000100000000{player}0000000100000000"),
            ),
            (
                REQUEST_MOVE_PLAYER_TYPE_INDEX,
                format!("{player}000000030000000100000004"),
            ),
            (REQUEST_KICK_PLAYER_TYPE_INDEX, player.to_owned()),
        ] {
            let call = client(type_index, &args);
            assert_eq!(call.type_index(), type_index);
            assert_eq!(
                call.encode().expect("writes"),
                hex(&format!("{HEADER}{args}"))
            );
        }
        let WarSignupClientMsg::SetPermissions {
            player,
            place,
            permission,
            ..
        } = client(
            REQUEST_SET_PERMISSIONS_TYPE_INDEX,
            &format!("{PLAYER}000000080000000302"),
        )
        else {
            panic!("a permission call");
        };
        assert_eq!(player.name, "Testa");
        assert_eq!(player.kind, WAR_PLAYER_KIND);
        assert_eq!(place, RosterPlace { group: 8, slot: 3 });
        assert_eq!(permission, 2);
    }

    /// The retail answer forms: a two-entry list (gear scores 800 and 781), a count, a done and
    /// a banned sign-up, a leave and a refused roster call.
    #[test]
    fn the_answers_read_and_write_back() {
        let player = PLAYER;
        let company = "a1a2a3a4a5a6a7a8a9aaabacadaeafb0";
        let list = format!("{HEADER}0000000100000001 02{player}{company}0320{player}{company}030d")
            .replace(' ', "");
        let answer = WarSignupServerMsg::decode(SIGNUP_LIST_RECEIVED_TYPE_INDEX, &hex(&list))
            .expect("reads");
        let WarSignupServerMsg::SignupListReceived { side, entries, .. } = &answer else {
            panic!("a list");
        };
        assert_eq!(*side, SIDE_ATTACKER);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].gear_score, 800);
        assert_eq!(entries[1].gear_score, 781);
        assert_eq!(answer.encode().expect("writes"), hex(&list));
        for (type_index, args) in [
            (SIGNUP_COUNT_RECEIVED_TYPE_INDEX, "0000000100000003"),
            (SIGNUP_RESPONSE_RECEIVED_TYPE_INDEX, "0100000000"),
            (SIGNUP_RESPONSE_RECEIVED_TYPE_INDEX, "000000000c"),
            (LEAVE_RESPONSE_RECEIVED_TYPE_INDEX, "00040100000000"),
            (SET_ROSTER_RESPONSE_RECEIVED_TYPE_INDEX, "000000000e"),
        ] {
            let body = format!("{HEADER}{args}");
            let answer = WarSignupServerMsg::decode(type_index, &hex(&body)).expect("reads");
            assert_eq!(answer.type_index(), type_index);
            assert!(WarSignupServerMsg::is_type(type_index));
            assert_eq!(answer.encode().expect("writes"), hex(&body));
        }
        let banned = WarSignupServerMsg::decode(
            SIGNUP_RESPONSE_RECEIVED_TYPE_INDEX,
            &hex(&format!("{HEADER}000000000c")),
        );
        assert_eq!(
            banned,
            Ok(WarSignupServerMsg::SignupResponseReceived {
                header: FacetHeader::read(&mut Reader::new(&hex(HEADER))).expect("header"),
                result: CallResult::refused(REASON_BANNED),
            })
        );
    }

    #[test]
    fn other_types_are_refused() {
        assert!(WarSignupClientMsg::decode(1, &[]).is_none());
        assert!(matches!(
            WarSignupServerMsg::decode(1, &hex(HEADER)),
            Err(MessageError::BadTag { .. })
        ));
    }
}
