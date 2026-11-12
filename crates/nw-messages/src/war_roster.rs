// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::TerritoryInteractorReplicatedState` (1880): a player's war sign-ups and the roster of
//! its war (wars and sieges study 2026-11-12: 2,703 bodies in 46 ledgers, every one read here
//! and written back to the same bytes).
//!
//! Layout from the client's member table (63 children): child 0 the player's sign-ups by
//! territory ([`SignupData`]); children 1 to 60 the roster, per group (10) five players
//! ([`RosterPlayer`]) and then the group's data ([`RosterGroup`]); child 61
//! `replicatedBackfillEnabledState`; child 62 `kickedRaidMemberCooldown`.
//!
//! Observed (`army-management` 1782934970702): the first roster body sends all 60 children; an
//! empty slot is a player with two empty texts and a zero company; each group's data holds its
//! index in its last byte. A later body sends the changed slots alone.

use crate::keyed_list::KeyedList;
use crate::slots::{SLOT_0, read_children_64, read_slot_byte, write_children_64};
use crate::war_signup::WarPlayer;
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// Type index of `MB::TerritoryInteractorReplicatedState`.
pub const TERRITORY_INTERACTOR_TYPE_INDEX: u64 = 1880;
/// Groups of a war roster (Observed: 10, as the war raids of the war data).
pub const ROSTER_GROUPS: u8 = 10;
/// Players of a roster group (Observed: 5).
pub const GROUP_SLOTS: u8 = 5;
/// A roster player's permission: a member (Observed: most players).
pub const PERMISSION_MEMBER: u8 = 0;
/// A roster player's permission: an officer who may edit the roster (Inferred: 1 on a few
/// players of the war leader's company).
pub const PERMISSION_OFFICER: u8 = 1;
/// A roster player's permission: the war leader (Observed: 2 on the first player).
pub const PERMISSION_LEADER: u8 = 2;

const FIELD: &str = "TerritoryInteractorReplicatedState";
const SLOTS: u8 = 1;
const CHILDREN: u8 = 63;
const SIGNUPS_CHILD: u8 = 0;
const FIRST_ROSTER_CHILD: u8 = 1;
/// Children of one group: its players and its data.
const GROUP_CHILDREN: u8 = GROUP_SLOTS + 1;
const BACKFILL_CHILD: u8 = 61;
const KICK_COOLDOWN_CHILD: u8 = 62;

/// One sign-up of the player (child 0, by territory id).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignupData {
    /// The company of the war's side (Observed: the war company of the player's side).
    pub company: [u8; RAW_ID_LEN],
    /// The side (`SIDE_ATTACKER` or `SIDE_DEFENDER`).
    pub side: u32,
    /// Whether the player is on the roster (Observed: 0 after the sign-up, 1 later; Inferred).
    pub on_roster: u8,
    /// A byte (Observed: 0; meaning Unknown).
    pub flag: u8,
}

/// A player of the roster (an empty slot is [`RosterPlayer::default`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RosterPlayer {
    /// The player.
    pub player: WarPlayer,
    /// Its company (zero for a player in none or an empty slot).
    pub company: [u8; RAW_ID_LEN],
    /// Its gear score (Observed: 9 to 800; Inferred: the level for a low player).
    pub gear_score: u16,
    /// Its permission ([`PERMISSION_MEMBER`], [`PERMISSION_OFFICER`], [`PERMISSION_LEADER`]).
    pub permission: u8,
    /// A byte (Observed: 0; meaning Unknown).
    pub flag: u8,
}

impl RosterPlayer {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            player: WarPlayer::read(reader)?,
            company: reader.raw_id()?,
            gear_score: reader.u16()?,
            permission: reader.u8()?,
            flag: reader.u8()?,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        self.player.write(writer)?;
        writer
            .bytes(&self.company)
            .u16(self.gear_score)
            .u8(self.permission)
            .u8(self.flag);
        Ok(())
    }
}

/// The data of a roster group (Observed: three zero bytes and the group's index).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RosterGroup {
    /// The first three bytes (meaning Unknown).
    pub flags: [u8; 3],
    /// The group's index (0 to 9).
    pub index: u8,
}

/// The child of the player in `slot` of `group`.
pub const fn player_child(group: u8, slot: u8) -> u8 {
    FIRST_ROSTER_CHILD + group * GROUP_CHILDREN + slot
}

/// The child of the data of `group`.
pub const fn group_child(group: u8) -> u8 {
    FIRST_ROSTER_CHILD + group * GROUP_CHILDREN + GROUP_SLOTS
}

/// The territory interactor state; a `None` child is not sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TerritoryInteractorState {
    /// Child 0, `replicatedPlayerSignupDataMap`.
    pub signups: Option<KeyedList<u16, SignupData>>,
    /// The roster players sent, by (group, slot).
    pub players: Vec<((u8, u8), RosterPlayer)>,
    /// The roster groups' data sent, by group.
    pub groups: Vec<(u8, RosterGroup)>,
    /// Child 61, `replicatedBackfillEnabledState`.
    pub backfill: Option<u8>,
    /// Child 62, `kickedRaidMemberCooldown` (Inferred: a time, Unix ns).
    pub kick_cooldown: Option<u64>,
}

impl TerritoryInteractorState {
    /// Reads the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not match the layout.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        if read_slot_byte(reader, FIELD, SLOTS)? == 0 {
            return Ok(state);
        }
        read_children_64(reader, FIELD, CHILDREN, |reader, child| {
            match child {
                SIGNUPS_CHILD => {
                    state.signups = Some(KeyedList::read(reader, |reader| {
                        Ok(SignupData {
                            company: reader.raw_id()?,
                            side: reader.u32()?,
                            on_roster: reader.u8()?,
                            flag: reader.u8()?,
                        })
                    })?);
                }
                BACKFILL_CHILD => state.backfill = Some(reader.u8()?),
                KICK_COOLDOWN_CHILD => state.kick_cooldown = Some(reader.u64()?),
                roster => {
                    let at = roster - FIRST_ROSTER_CHILD;
                    let (group, slot) = (at / GROUP_CHILDREN, at % GROUP_CHILDREN);
                    if slot == GROUP_SLOTS {
                        let [first, second, third, index] = reader.array()?;
                        state.groups.push((
                            group,
                            RosterGroup {
                                flags: [first, second, third],
                                index,
                            },
                        ));
                    } else {
                        state
                            .players
                            .push(((group, slot), RosterPlayer::read(reader)?));
                    }
                }
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state, its children in child order.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a text is too long or a list does not write.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let mut children: u64 = 0;
        if self.signups.is_some() {
            children |= 1 << SIGNUPS_CHILD;
        }
        for ((group, slot), _) in &self.players {
            children |= 1 << player_child(*group, *slot);
        }
        for (group, _) in &self.groups {
            children |= 1 << group_child(*group);
        }
        if self.backfill.is_some() {
            children |= 1 << BACKFILL_CHILD;
        }
        if self.kick_cooldown.is_some() {
            children |= 1 << KICK_COOLDOWN_CHILD;
        }
        if children == 0 {
            writer.u8(0);
            return Ok(());
        }
        writer.u8(SLOT_0);
        write_children_64(writer, children, CHILDREN, |writer, child| match child {
            SIGNUPS_CHILD => self.signups.as_ref().map_or(Ok(()), |signups| {
                signups.write(writer, |writer, signup| {
                    writer
                        .bytes(&signup.company)
                        .u32(signup.side)
                        .u8(signup.on_roster)
                        .u8(signup.flag);
                    Ok(())
                })
            }),
            BACKFILL_CHILD => {
                self.backfill.iter().for_each(|value| {
                    writer.u8(*value);
                });
                Ok(())
            }
            KICK_COOLDOWN_CHILD => {
                self.kick_cooldown.iter().for_each(|value| {
                    writer.u64(*value);
                });
                Ok(())
            }
            roster => {
                let at = roster - FIRST_ROSTER_CHILD;
                let (group, slot) = (at / GROUP_CHILDREN, at % GROUP_CHILDREN);
                if slot == GROUP_SLOTS {
                    if let Some((_, data)) = self.groups.iter().find(|(at, _)| *at == group) {
                        writer.bytes(&data.flags).u8(data.index);
                    }
                    Ok(())
                } else {
                    self.players
                        .iter()
                        .find(|(at, _)| *at == (group, slot))
                        .map_or(Ok(()), |(_, player)| player.write(writer))
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::war_signup::{SIDE_ATTACKER, WAR_PLAYER_KIND};

    fn round(state: &TerritoryInteractorState) -> Vec<u8> {
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        let body = writer.finish();
        let mut reader = Reader::new(&body);
        assert_eq!(
            &TerritoryInteractorState::read(&mut reader).expect("read"),
            state
        );
        reader.finish().expect("used up");
        body
    }

    /// The retail sign-up body form (`CK_won_offensive_war` 1785437056432): slot 0, child 0,
    /// the full map of one sign-up for territory 10 (a made-up company).
    #[test]
    fn a_sign_up_body_writes_the_retail_form() {
        let company = [0xc1; RAW_ID_LEN];
        let state = TerritoryInteractorState {
            signups: Some(KeyedList::full(
                7,
                vec![(
                    10,
                    SignupData {
                        company,
                        side: SIDE_ATTACKER,
                        on_roster: 1,
                        flag: 0,
                    },
                )],
            )),
            ..TerritoryInteractorState::default()
        };
        let mut expected = vec![SLOT_0, 0x01, 0x00, 0x01, 0x07, 0x01, 0x00, 0x0a];
        expected.extend(company);
        expected.extend([0, 0, 0, 1, 1, 0]);
        assert_eq!(round(&state), expected);
    }

    /// A roster player in group 8, slot 3, the data of group 9 and the cooldown: the children
    /// past the first child byte go in their own child bytes.
    #[test]
    fn roster_children_read_and_write_back() {
        let state = TerritoryInteractorState {
            players: vec![(
                (8, 3),
                RosterPlayer {
                    player: WarPlayer {
                        character: "11111111-2222-3333-4444-555555555555".to_owned(),
                        name: "Testa".to_owned(),
                        kind: WAR_PLAYER_KIND,
                    },
                    company: [0xc1; RAW_ID_LEN],
                    gear_score: 800,
                    permission: PERMISSION_OFFICER,
                    flag: 0,
                },
            )],
            groups: vec![(
                9,
                RosterGroup {
                    flags: [0; 3],
                    index: 9,
                },
            )],
            backfill: Some(0),
            kick_cooldown: Some(1),
            ..TerritoryInteractorState::default()
        };
        assert_eq!(player_child(8, 3), 52);
        assert_eq!(group_child(9), 60);
        round(&state);
        round(&TerritoryInteractorState::default());
    }
}
