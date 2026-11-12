// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::PlayerAppearanceComponentReplicatedState` (type 1195, player component 12) on the bundle
//! path (player-appearance note). The class has slot 0 only. Its constructor registers 14
//! children: twelve one-byte look choices (gender, race, skin tone, hairstyle, facial hair, hair
//! colour, facial hair colour, eye colour, face mark, scar, tattoo, tattoo colour), then
//! `iconData` (`PlayerIconData`: 7 bytes and a big-endian `u16`) and `appearanceChangeFlag` (one
//! byte). Each look byte is a position in a list of the client's character-creation data. Every
//! retail first body of the local player sets all 14 children (25 bytes).
//!
//! A look change (the barber, [`AppearanceChangeRequest`]) sends only what changed (Observed,
//! `buy_barbertoken_and_change_style`, ten calls that change one part at a time, and
//! `Hive_Transmog`): the changed look children, `iconData` when one of its seven parts changed,
//! and `appearanceChangeFlag` always, which goes 1, 0, 1, ... from the 0 of the first body.

use crate::facet::FacetHeader;
use crate::slots::{read_slot_0, write_known};
use crate::wire::{BodyCodec, MessageError, Reader, Writer};

/// Type index of `MB::PlayerAppearanceComponentReplicatedState`.
pub const PLAYER_APPEARANCE_TYPE_INDEX: u64 = 1195;
/// The player component of the appearance state (player component table).
pub const PLAYER_APPEARANCE_COMPONENT: u64 = 12;
/// `PlayerAppearanceComponentServerFacet_PurchaseAppearanceChange`.
pub const PURCHASE_APPEARANCE_CHANGE_TYPE_INDEX: u64 = 6420;
/// `PlayerAppearanceComponentClientFacet_OnPurchaseSuccess`.
pub const APPEARANCE_PURCHASED_TYPE_INDEX: u64 = 6855;
/// Children registered on slot 0.
const APPEARANCE_CHILDREN: u8 = 14;
/// The look children: 0 to 11, one byte each.
pub const LOOK_CHOICES: usize = 12;
/// Child 12: `iconData`.
const CHILD_ICON: u8 = 12;
/// Child 13: `appearanceChangeFlag`.
const CHILD_CHANGE_FLAG: u8 = 13;
/// `appearanceChangeFlag` of a look that did not change: 0 on 6,718 of 6,726 full retail bodies.
pub const LOOK_UNCHANGED: u8 = 0;
/// `appearanceChangeFlag` after an odd number of changes (Observed: the flag of the ten retail
/// barber changes went 1, 0, 1, ..., 0).
pub const LOOK_CHANGED: u8 = 1;
/// `m_backgroundColorIndex` of the icon: 0 on 5,291 of 6,749 retail bodies; its table is Unknown.
pub const DEFAULT_ICON_BACKGROUND: u16 = 0;
const FIELD: &str = "player appearance";
/// `playerGender` of a female character.
const FEMALE: u8 = 1;
/// The look child of each byte of a 6420 call, in call order (Observed,
/// `buy_barbertoken_and_change_style` 1781370830187 to 1781370874600: each call changed one
/// byte and the own change that followed set one child: byte 0 race, 1 skin tone, 2 gender, 3
/// hairstyle, 4 facial hair, 7 eye colour, 8 face mark, 9 scar, 10 tattoo; bytes 5, 6 and 11
/// never changed and equal the first look's hair colour 26, facial hair colour 27 and tattoo
/// colour 4, and `Hive_Transmog` 1782822005400 changed byte 6 with child 6).
const PURCHASE_ORDER: [usize; LOOK_CHOICES] = [1, 2, 0, 3, 4, 5, 6, 7, 8, 9, 10, 11];

/// The twelve look choices of a character, in child order. Each is a position in its
/// character-creation list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerLook {
    /// `playerGender`: 0 male, 1 female.
    pub gender: u8,
    /// `playerRace`: the face.
    pub race: u8,
    /// `playerSkinTone`.
    pub skin_tone: u8,
    /// `playerHairstyle`.
    pub hairstyle: u8,
    /// `playerFacialHair`.
    pub facial_hair: u8,
    /// `playerHairColor`.
    pub hair_color: u8,
    /// `playerFacialHairColor`.
    pub facial_hair_color: u8,
    /// `playerEyeColor`.
    pub eye_color: u8,
    /// `playerFaceMark`.
    pub face_mark: u8,
    /// `playerScar`.
    pub scar: u8,
    /// `playerTattoo`.
    pub tattoo: u8,
    /// `playerTattooColor`.
    pub tattoo_color: u8,
}

impl PlayerLook {
    /// Whether the character is female.
    pub const fn is_female(&self) -> bool {
        self.gender == FEMALE
    }

    /// The choices in child order.
    pub const fn choices(&self) -> [u8; LOOK_CHOICES] {
        [
            self.gender,
            self.race,
            self.skin_tone,
            self.hairstyle,
            self.facial_hair,
            self.hair_color,
            self.facial_hair_color,
            self.eye_color,
            self.face_mark,
            self.scar,
            self.tattoo,
            self.tattoo_color,
        ]
    }

    /// The look of `choices`, in child order.
    pub const fn of_choices(choices: [u8; LOOK_CHOICES]) -> Self {
        let [
            gender,
            race,
            skin_tone,
            hairstyle,
            facial_hair,
            hair_color,
            facial_hair_color,
            eye_color,
            face_mark,
            scar,
            tattoo,
            tattoo_color,
        ] = choices;
        Self {
            gender,
            race,
            skin_tone,
            hairstyle,
            facial_hair,
            hair_color,
            facial_hair_color,
            eye_color,
            face_mark,
            scar,
            tattoo,
            tattoo_color,
        }
    }
}

impl PlayerLook {
    /// The look of the 12 bytes of a 6420 call ([`PURCHASE_ORDER`]).
    fn read_purchase(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let bytes: [u8; LOOK_CHOICES] = reader.array()?;
        let mut choices = [0; LOOK_CHOICES];
        for (byte, child) in bytes.into_iter().zip(PURCHASE_ORDER) {
            choices[child] = byte;
        }
        Ok(Self::of_choices(choices))
    }

    /// The 12 bytes of a 6420 call ([`PURCHASE_ORDER`]).
    fn write_purchase(&self, writer: &mut Writer) {
        let choices = self.choices();
        for child in PURCHASE_ORDER {
            writer.u8(choices[child]);
        }
    }
}

/// `PlayerIconData`, the look of the character icon: seven bytes in this order (hair colour
/// before facial hair, unlike the look children) and a big-endian `u16` background colour.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerIconData {
    /// `m_genderIndex`.
    pub gender: u8,
    /// `m_raceIndex`.
    pub race: u8,
    /// `m_skintoneIndex`.
    pub skin_tone: u8,
    /// `m_hairIndex`.
    pub hair: u8,
    /// `m_hairColorIndex`.
    pub hair_color: u8,
    /// `m_facialHairIndex`.
    pub facial_hair: u8,
    /// `m_facialHairColorIndex`.
    pub facial_hair_color: u8,
    /// `m_backgroundColorIndex`.
    pub background_color: u16,
}

impl PlayerIconData {
    /// The icon of `look` on the default background. On the retail bodies the seven icon bytes
    /// equal the look's (female races excepted, for a reason that is Unknown).
    pub const fn of(look: &PlayerLook) -> Self {
        Self {
            gender: look.gender,
            race: look.race,
            skin_tone: look.skin_tone,
            hair: look.hairstyle,
            hair_color: look.hair_color,
            facial_hair: look.facial_hair,
            facial_hair_color: look.facial_hair_color,
            background_color: DEFAULT_ICON_BACKGROUND,
        }
    }

    pub(crate) fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let [
            gender,
            race,
            skin_tone,
            hair,
            hair_color,
            facial_hair,
            facial_hair_color,
        ] = reader.array()?;
        Ok(Self {
            gender,
            race,
            skin_tone,
            hair,
            hair_color,
            facial_hair,
            facial_hair_color,
            background_color: reader.u16()?,
        })
    }

    pub(crate) fn write(&self, writer: &mut Writer) {
        writer
            .bytes(&[
                self.gender,
                self.race,
                self.skin_tone,
                self.hair,
                self.hair_color,
                self.facial_hair,
                self.facial_hair_color,
            ])
            .u16(self.background_color);
    }
}

/// The appearance state; each part when sent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerAppearanceState {
    /// The look choices that are sent, in child order: all twelve in a full body, the changed
    /// ones in a change.
    pub choices: [Option<u8>; LOOK_CHOICES],
    /// `iconData`.
    pub icon: Option<PlayerIconData>,
    /// `appearanceChangeFlag`.
    pub change_flag: Option<u8>,
}

impl PlayerAppearanceState {
    /// The full state of `look`, as a retail first body: the look, its icon, and no change.
    pub const fn of(look: PlayerLook) -> Self {
        let all = look.choices();
        let mut choices = [None; LOOK_CHOICES];
        let mut child = 0;
        while child < LOOK_CHOICES {
            choices[child] = Some(all[child]);
            child += 1;
        }
        Self {
            choices,
            icon: Some(PlayerIconData::of(&look)),
            change_flag: Some(LOOK_UNCHANGED),
        }
    }

    /// The change from look `before` to look `after` with the change flag `flag`: the look
    /// children that differ, the icon when one of its parts differs, and the flag.
    pub fn change(before: &PlayerLook, after: &PlayerLook, flag: u8) -> Self {
        let (old, new) = (before.choices(), after.choices());
        let icon = PlayerIconData::of(after);
        Self {
            choices: std::array::from_fn(|child| (old[child] != new[child]).then_some(new[child])),
            icon: (PlayerIconData::of(before) != icon).then_some(icon),
            change_flag: Some(flag),
        }
    }

    /// The whole look, when the state sends all twelve choices.
    pub fn look(&self) -> Option<PlayerLook> {
        let mut choices = [0; LOOK_CHOICES];
        for (choice, sent) in choices.iter_mut().zip(self.choices) {
            *choice = sent?;
        }
        Some(PlayerLook::of_choices(choices))
    }

    /// Reads the state from the bundle path: a full body or a change.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, FIELD, APPEARANCE_CHILDREN, |reader, child| {
            match child {
                CHILD_ICON => state.icon = Some(PlayerIconData::read(reader)?),
                CHILD_CHANGE_FLAG => state.change_flag = Some(reader.u8()?),
                _ => state.choices[usize::from(child)] = Some(reader.u8()?),
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        let look_bits = self
            .choices
            .iter()
            .enumerate()
            .filter(|(_, choice)| choice.is_some())
            .fold(0_u16, |bits, (child, _)| bits | 1 << child);
        let children = look_bits
            | u16::from(self.icon.is_some()) << CHILD_ICON
            | u16::from(self.change_flag.is_some()) << CHILD_CHANGE_FLAG;
        write_known(writer, children, APPEARANCE_CHILDREN, |writer, child| {
            match child {
                CHILD_ICON => self.icon.map(|icon| icon.write(writer)),
                CHILD_CHANGE_FLAG => self.change_flag.map(|flag| {
                    writer.u8(flag);
                }),
                _ => self.choices[usize::from(child)].map(|choice| {
                    writer.u8(choice);
                }),
            };
        });
    }
}

/// The change flag after one more change of a look whose flag is `flag` (Observed: it goes
/// 1, 0, 1, ...).
pub const fn next_change_flag(flag: u8) -> u8 {
    if flag == LOOK_UNCHANGED {
        LOOK_CHANGED
    } else {
        LOOK_UNCHANGED
    }
}

/// A barber call (6420, `PurchaseAppearanceChange`): the whole new look, in the call's own byte
/// order ([`PURCHASE_ORDER`]), and no other field (Observed: 16 header bytes and 12 look bytes
/// in all 11 retail calls).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppearanceChangeRequest {
    /// The header (the player's `PlayerAppearanceComponent`).
    pub header: FacetHeader,
    /// The look the player buys.
    pub look: PlayerLook,
}

impl AppearanceChangeRequest {
    /// Reads the call after its header.
    pub(crate) fn read_after(
        header: FacetHeader,
        reader: &mut Reader<'_>,
    ) -> Result<Self, MessageError> {
        Ok(Self {
            header,
            look: PlayerLook::read_purchase(reader)?,
        })
    }

    /// Writes the call.
    pub(crate) fn write(&self, writer: &mut Writer) {
        self.header.write(writer);
        self.look.write_purchase(writer);
    }
}

/// 6855 `OnPurchaseSuccess`: the barber change is done. The header only: the barber call's own
/// (Observed: all 11 retail answers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppearancePurchasedMsg {
    /// The header of the barber call.
    pub header: FacetHeader,
}

impl BodyCodec for AppearancePurchasedMsg {
    const TYPE_INDEX: u64 = APPEARANCE_PURCHASED_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        reader.finish()?;
        Ok(Self { header })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The look of the live test character: male, face05, skin01, hairstyle65m, facialhair44m,
    /// haircolor5 twice, eyecolor12, no face mark, scar6, tattoo26, tattoocolor15.
    const LOOK: [u8; LOOK_CHOICES] = [0, 5, 0, 65, 44, 4, 4, 11, 0, 6, 26, 14];
    /// Child byte of children 0 to 6 with more to come, then of children 7 to 13.
    const FIRST_GROUP: u8 = 0xff;
    const SECOND_GROUP: u8 = 0x7f;
    /// A retail full body: 25 bytes.
    const FULL_BODY_LEN: usize = 25;

    fn encoded(state: &PlayerAppearanceState) -> Vec<u8> {
        let mut writer = Writer::new();
        state.write(&mut writer);
        writer.finish()
    }

    #[test]
    fn a_full_look_is_the_25_byte_retail_form() {
        let look = PlayerLook::of_choices(LOOK);
        let bytes = encoded(&PlayerAppearanceState::of(look));
        let mut expected = vec![0x01, FIRST_GROUP];
        expected.extend_from_slice(&LOOK[..7]);
        expected.push(SECOND_GROUP);
        expected.extend_from_slice(&LOOK[7..]);
        // iconData: gender, race, skin, hair, hair colour, facial hair, facial hair colour, then
        // the background colour as a big-endian u16.
        expected.extend_from_slice(&[0, 5, 0, 65, 4, 44, 4, 0, 0]);
        expected.push(LOOK_UNCHANGED);
        assert_eq!(bytes, expected);
        assert_eq!(bytes.len(), FULL_BODY_LEN);
        let mut reader = Reader::new(&bytes);
        let read = PlayerAppearanceState::read(&mut reader).expect("read");
        reader.finish().expect("to the end");
        assert_eq!(read, PlayerAppearanceState::of(look));
        assert_eq!(read.look().map(|look| look.choices()), Some(LOOK));
    }

    #[test]
    fn gender_1_is_female() {
        let female = PlayerLook {
            gender: FEMALE,
            ..PlayerLook::default()
        };
        assert!(female.is_female());
        assert!(!PlayerLook::default().is_female(), "gender 0 is male");
    }

    #[test]
    fn no_look_is_one_zero_byte() {
        let bytes = encoded(&PlayerAppearanceState::default());
        assert_eq!(bytes, vec![0]);
        let read = PlayerAppearanceState::read(&mut Reader::new(&bytes)).expect("read");
        assert_eq!(read, PlayerAppearanceState::default());
    }

    #[test]
    fn the_icon_and_flag_alone_read_back() {
        let state = PlayerAppearanceState {
            choices: [None; LOOK_CHOICES],
            icon: Some(PlayerIconData::of(&PlayerLook::of_choices(LOOK))),
            change_flag: Some(1),
        };
        let bytes = encoded(&state);
        let mut reader = Reader::new(&bytes);
        assert_eq!(
            PlayerAppearanceState::read(&mut reader).expect("read"),
            state
        );
        reader.finish().expect("to the end");
    }

    /// The first own look of `buy_barbertoken_and_change_style`: male, race 30, hair colour 26,
    /// facial hair colour 27, tattoo colour 4, the rest 0.
    const FIRST_LOOK: [u8; LOOK_CHOICES] = [0, 30, 0, 0, 0, 26, 27, 0, 0, 0, 0, 4];

    fn read_back(bytes: &[u8]) -> PlayerAppearanceState {
        let mut reader = Reader::new(bytes);
        let state = PlayerAppearanceState::read(&mut reader).expect("read");
        reader.finish().expect("to the end");
        state
    }

    #[test]
    fn the_retail_barber_changes_send_what_changed_and_the_flag() {
        let first = PlayerLook::of_choices(FIRST_LOOK);
        // 1781370830187: female and race 28; the icon changes too; the first flag 1.
        let female = PlayerLook {
            gender: 1,
            race: 28,
            ..first
        };
        let change = PlayerAppearanceState::change(&first, &female, LOOK_CHANGED);
        let retail = [1, 131, 1, 28, 96, 1, 28, 0, 0, 26, 0, 27, 0, 0, 1];
        assert_eq!(encoded(&change), retail);
        assert_eq!(read_back(&retail), change);
        assert_eq!(change.look(), None, "a change is no whole look");
        // 1781370853899 to 1781370859351: eye colour 0 to 1 is no part of the icon.
        let before = PlayerLook::of_choices([1, 20, 1, 1, 1, 26, 27, 0, 0, 0, 0, 4]);
        let eyes = PlayerLook {
            eye_color: 1,
            ..before
        };
        let retail = [1, 128, 65, 1, 0];
        let change = PlayerAppearanceState::change(&before, &eyes, LOOK_UNCHANGED);
        assert_eq!(encoded(&change), retail);
        assert_eq!(read_back(&retail), change);
    }

    #[test]
    fn the_retail_hive_change_sends_facial_hair_colour_tattoo_and_icon() {
        // `Hive_Transmog` 1782822005400: the call's look; the own change set facial hair colour
        // 19 and tattoo 0. The look before is made up in those two parts.
        let after = PlayerLook::of_choices([0, 23, 3, 6, 1, 16, 19, 17, 3, 0, 0, 14]);
        let before = PlayerLook {
            facial_hair_color: 5,
            tattoo: 9,
            ..after
        };
        let retail = [1, 192, 19, 104, 0, 0, 23, 3, 6, 16, 1, 19, 0, 0, 1];
        let change = PlayerAppearanceState::change(&before, &after, LOOK_CHANGED);
        assert_eq!(encoded(&change), retail);
        assert_eq!(read_back(&retail), change);
    }

    #[test]
    fn the_change_flag_goes_one_zero_one() {
        assert_eq!(next_change_flag(LOOK_UNCHANGED), LOOK_CHANGED);
        assert_eq!(next_change_flag(LOOK_CHANGED), LOOK_UNCHANGED);
    }

    /// A made-up header and the 12 look bytes of the first retail barber call
    /// (`buy_barbertoken_and_change_style` 1781370830187).
    const BARBER_CALL: &str = "0102030405060708a1a2a3a4a5a6a7a81c000100001a1b0000000004";

    #[test]
    fn a_barber_call_reads_the_look_in_its_own_order_and_writes_back() {
        let body = crate::test_support::hex_bytes(BARBER_CALL);
        let mut reader = Reader::new(&body);
        let header = FacetHeader::read(&mut reader).expect("header");
        let call = AppearanceChangeRequest::read_after(header, &mut reader).expect("call");
        reader.finish().expect("to the end");
        assert_eq!(
            call.look,
            PlayerLook {
                gender: 1,
                race: 28,
                ..PlayerLook::of_choices(FIRST_LOOK)
            }
        );
        let mut writer = Writer::new();
        call.write(&mut writer);
        assert_eq!(writer.finish(), body);
        let answer = AppearancePurchasedMsg { header };
        let bytes = answer.encode().expect("encode");
        assert_eq!(bytes, body[..body.len() - LOOK_CHOICES]);
        assert_eq!(
            AppearancePurchasedMsg::decode(&bytes).expect("decode"),
            answer
        );
    }
}
