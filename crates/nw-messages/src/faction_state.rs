// SPDX-License-Identifier: AGPL-3.0-or-later
//! Changes of the faction component's state, `MB::FactionComponentReplicatedState` (3152, own
//! component 49): slot 1 children 0 `pvpFlagPending`, 1 `notifyPending`, 2
//! `pvpFlagPendingEndTime` (u64 unix ns), 3 `lastFactionChangeTimepoint` (u64 unix ns) and 4
//! `factionChangeCount` (u16), 5 `ffaPendingEndTime` (u64 unix ns) and 6
//! `ffaAntiGroupingIsCursing`; slot 2 children 0 `faction`, 1 `pvpFlag`, 2 `hasSanctuary`, 3
//! `ffaFlag` and 4 `ffaAntiGroupingCurseStacks` (member layout table; one byte each for the
//! flags). The two anti-grouping children are not written: their rule is Unknown.
//!
//! **Observed** (the free-for-all zone, `FFA_ExitFFAAndCleanse`, `FFA_KillPlayerAndLoot`,
//! `FFA_LightningStrikeDeath`): 0.17 s after the client enters a free-for-all area (2297) the
//! own state gets `ffaPendingEndTime` (`02 20 <time>`, 30 s ahead); at that time the own state
//! gets `06 22 01 <0> 0a 01 01` (`notifyPending` 1, the end time 0, `pvpFlag` 1, `ffaFlag` 1);
//! a player that leaves before it gets the end time 0 alone (`02 20 <0>`). 0.1 s after the
//! client leaves the area (6416) the end time is set again, 5 s ahead, and at that time the
//! own state gets `06 20 <0> 0a 00 00`. Other players get the public slot: the card of a
//! player in the zone held `04 0f 02 01 00 01`, and a change `04 0a 01 01` or `04 0a 00 00`.
//!
//! **Observed**: the first join sets the faction alone (`Hive_join_yellow_faction_quest`, the
//! bundle after the 3161: `04 01 03`); a change from one faction to another sets the time and the
//! count with it (`Change-Faction`: `06 18 <time> 00 01 01 01`, the time 10 ms after the 3161).
//! A character with a faction logs in with its faction in the spawn body (`general-other-01`:
//! `06 41 00 00 0b 02 00 00`, faction 2). The changes go as recorded bodies
//! ([`MemberState::Recorded`]): the bundle walk passes the class by its layout.

use crate::payload::{BundleMember, MemberState};
use crate::slots::{children_mask, write_children};
use crate::wire::{MessageError, Writer};

/// `MB::FactionComponentReplicatedState`.
pub const FACTION_TYPE_INDEX: u64 = 3152;
/// The own component of the state.
pub const FACTION_COMPONENT: u64 = 49;
/// Slot byte bit of slot 1 (the owner's).
const OWNER_SLOT: u8 = 0x02;
/// Slot byte bit of slot 2 (the public one).
const PUBLIC_SLOT: u8 = 0x04;
/// Children of slot 1 and slot 2.
const OWNER_CHILDREN: u8 = 7;
const PUBLIC_CHILDREN: u8 = 5;
/// `pvpFlagPending`, `notifyPending`, `pvpFlagPendingEndTime` (u64 unix ns),
/// `lastFactionChangeTimepoint` (u64 unix ns) and `factionChangeCount` (u16) (slot 1).
const PVP_PENDING_CHILD: u8 = 0;
const NOTIFY_PENDING_CHILD: u8 = 1;
const PVP_PENDING_END_CHILD: u8 = 2;
const CHANGED_AT_CHILD: u8 = 3;
const CHANGES_CHILD: u8 = 4;
/// `ffaPendingEndTime` (u64 unix ns; slot 1).
const FFA_PENDING_END_CHILD: u8 = 5;
/// `faction`, `pvpFlag`, `hasSanctuary` and `ffaFlag` (slot 2).
const FACTION_CHILD: u8 = 0;
const PVP_FLAG_CHILD: u8 = 1;
const SANCTUARY_CHILD: u8 = 2;
const FFA_FLAG_CHILD: u8 = 3;
/// The faction value of a player with no faction.
const NO_FACTION: u8 = 0;

/// A change of some children of the faction state; `None` children stay as they are.
///
/// **Observed** (`Hive_flag_pvp_on_and_off`, every 3152 of the own component): a PvP toggle on
/// sets `pvpFlagPending` and `notifyPending` (`02 03 01 01`), a toggle off clears the pending
/// flag (`02 03 00 01`); leaving a sanctuary with the flag pending sets
/// `pvpFlagPendingEndTime` and clears `hasSanctuary` (`06 04 <time> 04 00`); at the end time
/// the three pending children clear and `pvpFlag` is set (`06 07 00 00 <0> 02 01`); entering a
/// sanctuary with the flag on clears `pvpFlag` and makes it pending again (`06 01 01 06 00 01`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FactionStateChange {
    /// `pvpFlagPending`: the flag waits (in a sanctuary, or for its countdown).
    pub pvp_flag_pending: Option<bool>,
    /// `notifyPending`.
    pub notify_pending: Option<bool>,
    /// `pvpFlagPendingEndTime`: unix ns when the flag goes on; 0 for none.
    pub pvp_flag_pending_end_ns: Option<u64>,
    /// `lastFactionChangeTimepoint`: unix ns.
    pub last_faction_change_ns: Option<u64>,
    /// `factionChangeCount`.
    pub faction_change_count: Option<u16>,
    /// `faction`.
    pub faction: Option<u8>,
    /// `pvpFlag`: the player is flagged for PvP.
    pub pvp_flag: Option<bool>,
    /// `hasSanctuary`: the player is in a sanctuary.
    pub has_sanctuary: Option<bool>,
    /// `ffaPendingEndTime`: unix ns when the free-for-all flag goes on or off; 0 for none.
    pub ffa_pending_end_ns: Option<u64>,
    /// `ffaFlag`: the player is flagged for free-for-all PvP.
    pub ffa_flag: Option<bool>,
}

impl FactionStateChange {
    /// The change's body bytes.
    ///
    /// # Errors
    ///
    /// Never fails today; the child writer's form keeps the signature of the other states.
    pub fn body(&self) -> Result<Vec<u8>, MessageError> {
        let owner = [
            self.pvp_flag_pending.is_some(),
            self.notify_pending.is_some(),
            self.pvp_flag_pending_end_ns.is_some(),
            self.last_faction_change_ns.is_some(),
            self.faction_change_count.is_some(),
            self.ffa_pending_end_ns.is_some(),
        ];
        let public = [
            self.faction.is_some(),
            self.pvp_flag.is_some(),
            self.has_sanctuary.is_some(),
            self.ffa_flag.is_some(),
        ];
        let mut slots = 0;
        if owner.contains(&true) {
            slots |= OWNER_SLOT;
        }
        if public.contains(&true) {
            slots |= PUBLIC_SLOT;
        }
        let mut writer = Writer::new();
        writer.u8(slots);
        let flag = |value: Option<bool>| u8::from(value.unwrap_or_default());
        if owner.contains(&true) {
            write_children(
                &mut writer,
                children_mask(&owner),
                OWNER_CHILDREN,
                |writer, child| {
                    match child {
                        PVP_PENDING_CHILD => writer.u8(flag(self.pvp_flag_pending)),
                        NOTIFY_PENDING_CHILD => writer.u8(flag(self.notify_pending)),
                        PVP_PENDING_END_CHILD => {
                            writer.u64(self.pvp_flag_pending_end_ns.unwrap_or_default())
                        }
                        CHANGED_AT_CHILD => {
                            writer.u64(self.last_faction_change_ns.unwrap_or_default())
                        }
                        CHANGES_CHILD => writer.u16(self.faction_change_count.unwrap_or_default()),
                        FFA_PENDING_END_CHILD => {
                            writer.u64(self.ffa_pending_end_ns.unwrap_or_default())
                        }
                        // The mask holds no other child.
                        _ => writer,
                    };
                    Ok::<(), MessageError>(())
                },
            )?;
        }
        if public.contains(&true) {
            write_children(
                &mut writer,
                children_mask(&public),
                PUBLIC_CHILDREN,
                |writer, child| {
                    match child {
                        FACTION_CHILD => writer.u8(self.faction.unwrap_or_default()),
                        PVP_FLAG_CHILD => writer.u8(flag(self.pvp_flag)),
                        SANCTUARY_CHILD => writer.u8(flag(self.has_sanctuary)),
                        FFA_FLAG_CHILD => writer.u8(flag(self.ffa_flag)),
                        // The mask holds no other child.
                        _ => writer,
                    };
                    Ok::<(), MessageError>(())
                },
            )?;
        }
        Ok(writer.finish())
    }

    /// The own member of the change.
    ///
    /// # Errors
    ///
    /// As [`Self::body`].
    pub fn member(&self) -> Result<BundleMember, MessageError> {
        Ok(BundleMember {
            component: FACTION_COMPONENT,
            state: MemberState::Recorded {
                class: FACTION_TYPE_INDEX,
                body: self.body()?,
            },
        })
    }
}

/// The public slot of a player's faction state: what other players get of it.
///
/// **Observed**: the card of another player holds `pvpFlag` and `ffaFlag` always and the
/// faction of a player that has one (`04 0a 00 00` for no faction, the public part of both
/// retail new characters; `04 0b <faction> 00 00` in `general-other-01`). Inferred:
/// `hasSanctuary` goes with the card only when it is set (one retail card of a player in the
/// free-for-all zone held it as 0, `04 0f 02 01 00 01`; the card of a new character does not).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PublicFaction {
    /// `faction`.
    pub faction: u8,
    /// `pvpFlag`.
    pub pvp_flag: bool,
    /// `hasSanctuary`.
    pub sanctuary: bool,
    /// `ffaFlag`.
    pub ffa_flag: bool,
}

impl PublicFaction {
    /// The state of the card of the player.
    pub fn card(&self) -> FactionStateChange {
        FactionStateChange {
            faction: (self.faction != NO_FACTION).then_some(self.faction),
            pvp_flag: Some(self.pvp_flag),
            has_sanctuary: self.sanctuary.then_some(true),
            ffa_flag: Some(self.ffa_flag),
            ..FactionStateChange::default()
        }
    }

    /// The change from `seen`, what the viewers hold: the children that differ. `None` when
    /// none does.
    pub fn change_from(&self, seen: &Self) -> Option<FactionStateChange> {
        let change = FactionStateChange {
            faction: (self.faction != seen.faction).then_some(self.faction),
            pvp_flag: (self.pvp_flag != seen.pvp_flag).then_some(self.pvp_flag),
            has_sanctuary: (self.sanctuary != seen.sanctuary).then_some(self.sanctuary),
            ffa_flag: (self.ffa_flag != seen.ffa_flag).then_some(self.ffa_flag),
            ..FactionStateChange::default()
        };
        (change != FactionStateChange::default()).then_some(change)
    }
}

/// One change of the faction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FactionChange {
    /// The faction now.
    pub faction: u8,
    /// For a change from one faction to another: the change's unix ns and the changes so far.
    pub switched: Option<(u64, u16)>,
}

impl FactionChange {
    fn state(&self) -> FactionStateChange {
        FactionStateChange {
            faction: Some(self.faction),
            last_faction_change_ns: self.switched.map(|(changed_ns, _)| changed_ns),
            faction_change_count: self.switched.map(|(_, changes)| changes),
            ..FactionStateChange::default()
        }
    }

    /// The change's body bytes.
    ///
    /// # Errors
    ///
    /// As [`FactionStateChange::body`].
    pub fn body(&self) -> Result<Vec<u8>, MessageError> {
        self.state().body()
    }

    /// The own member of the change.
    ///
    /// # Errors
    ///
    /// As [`FactionStateChange::body`].
    pub fn member(&self) -> Result<BundleMember, MessageError> {
        self.state().member()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_join_and_a_switch_are_the_retail_bodies() {
        let join = FactionChange {
            faction: 3,
            switched: None,
        };
        assert_eq!(join.body().expect("join"), [0x04, 0x01, 0x03]);
        let switch = FactionChange {
            faction: 1,
            switched: Some((0x18e1_bb69_e04f_ba64, 1)),
        };
        assert_eq!(
            switch.body().expect("switch"),
            [6, 24, 24, 225, 187, 105, 224, 79, 186, 100, 0, 1, 1, 1]
        );
        let member = join.member().expect("member");
        assert_eq!(
            member,
            FactionStateChange {
                faction: Some(3),
                ..FactionStateChange::default()
            }
            .member()
            .expect("same member")
        );
        assert_eq!(member.component, FACTION_COMPONENT);
        assert_eq!(member.state.type_index(), FACTION_TYPE_INDEX);
    }

    /// The PvP flag bodies of `Hive_flag_pvp_on_and_off` (own component 49), in ledger order.
    #[test]
    fn the_pvp_flag_changes_are_the_retail_bodies() {
        const END_NS: u64 = 0x18bd_da4c_f21c_6278;
        let change = FactionStateChange::default;
        let toggle_on = FactionStateChange {
            pvp_flag_pending: Some(true),
            notify_pending: Some(true),
            ..change()
        };
        assert_eq!(toggle_on.body().expect("on"), [0x02, 0x03, 0x01, 0x01]);
        let entered = FactionStateChange {
            has_sanctuary: Some(true),
            ..change()
        };
        assert_eq!(entered.body().expect("entered"), [0x04, 0x04, 0x01]);
        let left = FactionStateChange {
            pvp_flag_pending_end_ns: Some(END_NS),
            has_sanctuary: Some(false),
            ..change()
        };
        let mut expected = vec![0x06, 0x04];
        expected.extend(END_NS.to_be_bytes());
        expected.extend([0x04, 0x00]);
        assert_eq!(left.body().expect("left"), expected);
        let flagged = FactionStateChange {
            pvp_flag_pending: Some(false),
            notify_pending: Some(false),
            pvp_flag_pending_end_ns: Some(0),
            pvp_flag: Some(true),
            ..change()
        };
        assert_eq!(
            flagged.body().expect("flagged"),
            [0x06, 0x07, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02, 0x01]
        );
        let back_in = FactionStateChange {
            pvp_flag_pending: Some(true),
            pvp_flag: Some(false),
            has_sanctuary: Some(true),
            ..change()
        };
        assert_eq!(
            back_in.body().expect("back in"),
            [0x06, 0x01, 0x01, 0x06, 0x00, 0x01]
        );
        let toggle_off = FactionStateChange {
            pvp_flag_pending: Some(false),
            notify_pending: Some(true),
            ..change()
        };
        assert_eq!(toggle_off.body().expect("off"), [0x02, 0x03, 0x00, 0x01]);
    }

    #[test]
    fn the_free_for_all_changes_are_the_retail_bodies() {
        /// The end time of the retail entry (`FFA_ExitFFAAndCleanse`).
        const ENDS_NS: u64 = 0x18cc_17b0_ec50_433c;
        let pending = FactionStateChange {
            ffa_pending_end_ns: Some(ENDS_NS),
            ..FactionStateChange::default()
        };
        assert_eq!(
            pending.body().expect("pending"),
            [2, 32, 24, 204, 23, 176, 236, 80, 67, 60]
        );
        let on = FactionStateChange {
            notify_pending: Some(true),
            ffa_pending_end_ns: Some(0),
            pvp_flag: Some(true),
            ffa_flag: Some(true),
            ..FactionStateChange::default()
        };
        assert_eq!(
            on.body().expect("on"),
            [6, 34, 1, 0, 0, 0, 0, 0, 0, 0, 0, 10, 1, 1]
        );
        let off = FactionStateChange {
            ffa_pending_end_ns: Some(0),
            pvp_flag: Some(false),
            ffa_flag: Some(false),
            ..FactionStateChange::default()
        };
        assert_eq!(
            off.body().expect("off"),
            [6, 32, 0, 0, 0, 0, 0, 0, 0, 0, 10, 0, 0]
        );
    }

    #[test]
    fn other_players_get_the_public_slot_alone() {
        let none = PublicFaction::default();
        assert_eq!(none.card().body().expect("card"), [0x04, 0x0a, 0x00, 0x00]);
        let flagged = PublicFaction {
            faction: 2,
            pvp_flag: true,
            sanctuary: false,
            ffa_flag: true,
        };
        assert_eq!(
            flagged.card().body().expect("card"),
            [0x04, 0x0b, 0x02, 0x01, 0x01]
        );
        // The retail change of a player whose free-for-all flag goes on, and none for no change.
        assert_eq!(
            flagged
                .change_from(&PublicFaction {
                    faction: 2,
                    ..PublicFaction::default()
                })
                .expect("a change")
                .body()
                .expect("change"),
            [0x04, 0x0a, 0x01, 0x01]
        );
        assert_eq!(flagged.change_from(&flagged), None);
    }
}
