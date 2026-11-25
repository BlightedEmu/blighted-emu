// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::EntitlementComponentReplicatedState` (type 3133, player component 19): what the player's
//! account holds, as the game client keeps it. The client reads it for every check of an owned
//! entitlement and for the store currency's balance.
//!
//! Slot 0 has three children, each whole in every retail body (929 changes of 804 ledgers, all
//! full lists):
//!
//! - child 0 `entitlements`: a bitset in the full list form, bit `EntitlementIndex` of
//!   `javelindata_entitlements`, the low bit of a byte first. Observed
//!   (`mount-menu-buying-all-store-horses`): each store purchase sent the whole set again with
//!   one more bit, 0.3 s after the call: bit 2364 with `Ent_Mount_MTX_9_horse_Valkyrie`
//!   (`EntitlementIndex` 2364), bit 2673 with `entMount_MTX_bear_Panda` (2673). Bit 1484
//!   (`Ent_Xpack_RiseOfTheAngryEarth_Owner`) is set in every body of that capture. The set ends
//!   at its last byte that holds a bit (436 bytes with the highest bit 3482); an empty set is one
//!   byte of 0.
//! - child 1 `balances`: a full list of (CRC-32 of the lowercase `UniqueTagID`, amount) pairs:
//!   the store currency (`04f228b2` "Marks of Fortune", 13000) and each consumable
//!   (`Transmog_Token` 0, the boost tokens with their counts).
//! - child 2 `entitlementsReceived`: one boolean byte, 1 once the lists are the account's.
//!
//! The two lists of one body share one version, and each new body has a higher one. Before the
//! account's lists arrive the retail body is the empty form [`RETAIL_EMPTY_BODY`] (570 bodies of
//! 124 ledgers).

use crate::items::FullList;
use crate::slots::{SLOT_0, read_slot_0, write_known_children};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `MB::EntitlementComponentReplicatedState`.
pub const ENTITLEMENT_TYPE_INDEX: u64 = 3133;
/// The player component of the entitlements.
pub const ENTITLEMENT_COMPONENT: u64 = 19;

/// Children of slot 0.
const ENTITLEMENT_CHILDREN: u8 = 3;
const CHILD_ENTITLEMENTS: u8 = 0;
const CHILD_BALANCES: u8 = 1;
const CHILD_RECEIVED: u8 = 2;
/// Bits in one byte of the set.
const BITS_PER_BYTE: u32 = 8;
const FIELD: &str = "entitlements";

/// How much of one entitlement the account holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntitlementBalance {
    /// The CRC-32 of the entitlement's lowercase `UniqueTagID`.
    pub tag: u32,
    /// The amount.
    pub amount: u32,
}

/// The entitlement state; a child that is `None` is not sent, and no child sends no slot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntitlementState {
    /// The bitset of the owned entitlements.
    pub entitlements: Option<FullList<u8>>,
    /// The balances.
    pub balances: Option<FullList<EntitlementBalance>>,
    /// Whether the lists are the account's.
    pub received: Option<bool>,
}

impl EntitlementState {
    /// The account's lists at list version `version`: the bits `owned` (each an
    /// `EntitlementIndex`) and `balances`.
    pub fn of_account(
        version: u64,
        owned: impl IntoIterator<Item = u32>,
        balances: Vec<EntitlementBalance>,
    ) -> Self {
        Self {
            entitlements: Some(FullList {
                version: Some(version),
                elements: bitset(owned),
            }),
            balances: Some(FullList {
                version: Some(version),
                elements: balances,
            }),
            received: Some(true),
        }
    }

    /// Whether the set holds the bit `index`.
    pub fn owns(&self, index: u32) -> bool {
        self.entitlements.as_ref().is_some_and(|set| {
            set.elements
                .get((index / BITS_PER_BYTE) as usize)
                .is_some_and(|byte| byte >> (index % BITS_PER_BYTE) & 1 == 1)
        })
    }

    /// The amount held of the entitlement whose CRC-32 is `tag`.
    pub fn balance_of(&self, tag: u32) -> Option<u32> {
        self.balances
            .as_ref()?
            .elements
            .iter()
            .find(|balance| balance.tag == tag)
            .map(|balance| balance.amount)
    }

    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets another slot or child, or holds a
    /// list in its delta form.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, FIELD, ENTITLEMENT_CHILDREN, |reader, child| {
            match child {
                CHILD_ENTITLEMENTS => {
                    state.entitlements = Some(FullList::read(reader, |reader| reader.u8())?);
                }
                CHILD_BALANCES => {
                    state.balances = Some(FullList::read(reader, |reader| {
                        Ok(EntitlementBalance {
                            tag: reader.u32()?,
                            amount: reader.u32()?,
                        })
                    })?);
                }
                _ => state.received = Some(reader.boolean()?),
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] from the list writer.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let children = u16::from(self.entitlements.is_some()) << CHILD_ENTITLEMENTS
            | u16::from(self.balances.is_some()) << CHILD_BALANCES
            | u16::from(self.received.is_some()) << CHILD_RECEIVED;
        if children == 0 {
            writer.u8(0);
            return Ok(());
        }
        writer.u8(SLOT_0);
        let mut result = Ok(());
        write_known_children(writer, children, ENTITLEMENT_CHILDREN, |writer, child| {
            let written = match (child, &self.entitlements, &self.balances, self.received) {
                (CHILD_ENTITLEMENTS, Some(set), _, _) => set.write(writer, |writer, byte| {
                    writer.u8(*byte);
                    Ok(())
                }),
                (CHILD_BALANCES, _, Some(balances), _) => {
                    balances.write(writer, |writer, balance| {
                        writer.u32(balance.tag).u32(balance.amount);
                        Ok(())
                    })
                }
                (_, _, _, Some(received)) => {
                    writer.boolean(received);
                    Ok(())
                }
                _ => Ok(()),
            };
            if result.is_ok() {
                result = written;
            }
        });
        result
    }
}

/// The bytes of the set of `bits`, up to its last byte that holds one; one byte of 0 for no bit.
fn bitset(bits: impl IntoIterator<Item = u32>) -> Vec<u8> {
    let mut bytes = vec![0_u8];
    for bit in bits {
        let byte = (bit / BITS_PER_BYTE) as usize;
        if bytes.len() <= byte {
            bytes.resize(byte + 1, 0);
        }
        bytes[byte] |= 1 << (bit % BITS_PER_BYTE);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retail's body before the account's lists arrive (570 bodies of 124 ledgers): version 6,
    /// a set of one byte of 0, no balance, `entitlementsReceived` 0.
    const RETAIL_EMPTY_BODY: [u8; 12] = [
        0x01, 0x07, 0x00, 0x01, 0x06, 0x01, 0x00, 0x00, 0x01, 0x06, 0x00, 0x00,
    ];
    /// The start of the retail body after the second purchase of
    /// `mount-menu-buying-all-store-horses` (1793795556873): slot 0, children 0 and 1, the full
    /// form, the version 11341 and the 436 bytes of the set.
    const RETAIL_SET_HEAD: [u8; 8] = [0x01, 0x03, 0x00, 0x01, 0x8d, 0xb1, 0xb4, 0x06];
    const RETAIL_VERSION: u64 = 11_341;
    /// The highest bit of that retail set, and so its length.
    const RETAIL_LAST_BIT: u32 = 3_482;
    const RETAIL_SET_BYTES: usize = 436;
    /// `EntitlementIndex` of `Ent_Xpack_RiseOfTheAngryEarth_Owner` and of the mount of that
    /// purchase, `Ent_Mount_MTX_9_horse_Valkyrie`.
    const EXPANSION: u32 = 1_484;
    const VALKYRIE_HORSE: u32 = 2_364;
    /// The CRC-32 of `marks of fortune`, and the retail balance.
    const MARKS: u32 = 0x04f2_28b2;
    const RETAIL_MARKS: u32 = 13_000;

    fn encode(state: &EntitlementState) -> Vec<u8> {
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        writer.finish()
    }

    #[test]
    fn the_retail_empty_body_reads_and_writes_back() {
        let state = EntitlementState::read(&mut Reader::new(&RETAIL_EMPTY_BODY)).expect("read");
        assert_eq!(state.received, Some(false));
        assert_eq!(
            state.entitlements.as_ref().map(|set| set.elements.clone()),
            Some(vec![0])
        );
        assert_eq!(encode(&state), RETAIL_EMPTY_BODY);
    }

    #[test]
    fn an_account_state_starts_as_the_retail_body_and_holds_its_bits_and_balances() {
        let state = EntitlementState {
            received: None,
            ..EntitlementState::of_account(
                RETAIL_VERSION,
                [EXPANSION, VALKYRIE_HORSE, RETAIL_LAST_BIT],
                vec![EntitlementBalance {
                    tag: MARKS,
                    amount: RETAIL_MARKS,
                }],
            )
        };
        let body = encode(&state);
        assert_eq!(body[..RETAIL_SET_HEAD.len()], RETAIL_SET_HEAD);
        // Retail byte 303 of the body took bit 4 with the Valkyrie horse: index 2364.
        const VALKYRIE_BYTE: usize = 303;
        assert_eq!(body[VALKYRIE_BYTE], 0x10);
        // The balances follow the set: the full form, the same version, one pair.
        let balances = RETAIL_SET_HEAD.len() + RETAIL_SET_BYTES;
        assert_eq!(
            body[balances..],
            [
                0x00, 0x01, 0x8d, 0xb1, 0x01, 0x04, 0xf2, 0x28, 0xb2, 0x00, 0x00, 0x32, 0xc8
            ]
        );
        let read = EntitlementState::read(&mut Reader::new(&body)).expect("read");
        assert_eq!(read, state);
        assert!(read.owns(EXPANSION) && read.owns(VALKYRIE_HORSE));
        assert!(!read.owns(EXPANSION + 1));
        assert_eq!(read.balance_of(MARKS), Some(RETAIL_MARKS));
    }

    #[test]
    fn no_child_sends_no_slot() {
        assert_eq!(encode(&EntitlementState::default()), [0]);
        let state = EntitlementState::read(&mut Reader::new(&[0])).expect("read");
        assert_eq!(state, EntitlementState::default());
    }
}
