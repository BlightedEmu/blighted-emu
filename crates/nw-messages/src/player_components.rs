// SPDX-License-Identifier: AGPL-3.0-or-later
//! The replicated components of the player prefab (world-entry note, "Spawn condition traced").
//! A member's component index picks the component of the portrayed entity that takes the state
//! (`FUN_14167beb0`). When the player prefab is portrayed with no state for its other components,
//! the real client crashes while it applies the cached states. So the player's group carries one
//! state for every replicated component: the spawn card, the position, a connected type-3935
//! `PlayerComponentReplicatedState`, and an empty state ("no slot set") for each other component
//! below.
//!
//! The table is the client's own fragment log of a captured session (`NH-MSQ-10`, 2026-09-16):
//! every member of slot 1 it decoded, as (component index, class type index). All of these
//! classes use the generic slot reader `FUN_1417b4110`, whose first byte is the slot bitmap; on
//! every captured body only bits 0 to 2 are set, so one slot byte of 0 is a whole empty body.
//! Component 0 (the spawn card) and 16 (`ALCReplicatedState`) are not in the table.

use crate::components::{
    COMBAT_STATUS_TYPE_INDEX, CombatStatusState, STAMINA_TYPE_INDEX, StaminaState,
};
use crate::payload::{BundleMember, MemberState};
use crate::slots::{children_mask, read_children, read_slot_byte, write_children};
use crate::vitals::{VITALS_TYPE_INDEX, VitalsState};
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// Type index of `MB::PlayerComponentReplicatedState`.
pub const PLAYER_COMPONENT_TYPE_INDEX: u64 = 3935;
/// The empty slot byte of a generic-reader body.
const NO_SLOT: u8 = 0;
/// Slots of type 3935: base slot 0 plus two add-slot calls (`FUN_146713160`).
const PLAYER_COMPONENT_SLOTS: u8 = 3;
/// Slot-byte bit for slot 2 (packet `group1`: `characterId`, `playerConnected`, …).
const SLOT_2_BIT: u8 = 0b100;
/// Children registered on slot 2, in constructor order.
const SLOT_2_CHILDREN: u8 = 9;
/// Slot 2 child 0: `characterId` (presence `FUN_14087ac50`).
const CHILD_CHARACTER_ID: u16 = 1 << 0;
/// Slot 2 child 3: `playerConnected` (one boolean byte, `FUN_1417b4c20`).
const CHILD_PLAYER_CONNECTED: u16 = 1 << 3;
/// Presence flag bit 0: take the fixed 16-byte path (`FUN_14087ac50`).
const PRESENCE_FLAG_FIXED: u8 = 0x01;
const FIELD: &str = "player component";

/// (component index, class type index) of each replicated component of the player prefab, other
/// than the spawn card and the position.
pub const PLAYER_EMPTY_COMPONENTS: [(u64, u64); 56] = [
    (1, 6951),
    (2, 5437),
    (4, 2406),
    (5, 3290),
    (6, 2768),
    (9, 3935),
    (10, 3829),
    (12, 1195),
    (13, 603),
    (14, 1528),
    (17, 3563),
    (18, 5338),
    (19, 3133),
    (20, 3652),
    (22, 3857),
    (24, 3312),
    (25, 1292),
    (26, 3752),
    (27, 129),
    (28, COMBAT_STATUS_TYPE_INDEX),
    (29, 6234),
    (31, 1927),
    (32, 2895),
    (33, 982),
    (34, 4913),
    (35, 3765),
    (36, 81),
    (39, 5620),
    (41, VITALS_TYPE_INDEX),
    (42, STAMINA_TYPE_INDEX),
    (43, 1525),
    (44, 4236),
    (45, 2932),
    (46, 185),
    (47, 1652),
    (48, 5257),
    (49, 3152),
    (50, 4321),
    (51, 1994),
    (55, 4176),
    (56, 3147),
    (57, 1566),
    (60, 3786),
    (61, 1755),
    (62, 3183),
    (65, 911),
    (66, 3210),
    (67, 3681),
    (68, 497),
    (70, 899),
    (71, 4878),
    (72, 5485),
    (73, 5606),
    (74, 5691),
    (76, 1755),
    (77, 2938),
];

/// An empty state of a generic-reader class without a reader of its own here: one slot byte of
/// 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyState {
    /// The class type index.
    pub class: u64,
}

impl EmptyState {
    /// Whether `class` is a player component class that this state stands for.
    pub fn covers(class: u64) -> bool {
        !has_own_reader(class)
            && PLAYER_EMPTY_COMPONENTS
                .iter()
                .any(|(_, player_class)| *player_class == class)
    }

    /// Reads an empty body of `class`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Unsized`] for a body that sets a slot: this server does not know
    /// the children of these classes, so a bundle walk stops there.
    pub(crate) fn read(class: u64, reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let byte = reader.u8()?;
        if byte != NO_SLOT {
            return Err(MessageError::Unsized {
                field: "player component",
                byte,
            });
        }
        Ok(Self { class })
    }

    /// Writes the empty body.
    pub(crate) fn write(writer: &mut Writer) {
        writer.u8(NO_SLOT);
    }
}

/// `MB::PlayerComponentReplicatedState` (type 3935, component 9). Slot 2 holds
/// `characterId` and `playerConnected` (local-player-check note). Other slot-2 children and
/// slots 0 and 1 are not encoded here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerComponentState {
    /// Slot 2 child 0: the 16-byte character id, when sent.
    pub character_id: Option<[u8; RAW_ID_LEN]>,
    /// Slot 2 child 3: whether this player is connected, when sent.
    pub player_connected: Option<bool>,
}

impl PlayerComponentState {
    /// The first body of a local player: `characterId` is `id`, `playerConnected` is true.
    pub const fn connected(id: [u8; RAW_ID_LEN]) -> Self {
        Self {
            character_id: Some(id),
            player_connected: Some(true),
        }
    }

    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets slot 0 or 1, or sets a slot-2
    /// child this reader does not size.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let slots = read_slot_byte(reader, FIELD, PLAYER_COMPONENT_SLOTS)?;
        if slots & !SLOT_2_BIT != 0 {
            return Err(MessageError::Unsized {
                field: FIELD,
                byte: slots,
            });
        }
        if slots & SLOT_2_BIT == 0 {
            return Ok(Self::default());
        }
        let children = read_children(reader, FIELD, SLOT_2_CHILDREN)?;
        let known = CHILD_CHARACTER_ID | CHILD_PLAYER_CONNECTED;
        if children & !known != 0 {
            return Err(MessageError::Unsized {
                field: FIELD,
                byte: children as u8,
            });
        }
        let character_id = (children & CHILD_CHARACTER_ID != 0)
            .then(|| read_presence_id(reader))
            .transpose()?;
        let player_connected = (children & CHILD_PLAYER_CONNECTED != 0)
            .then(|| reader.boolean())
            .transpose()?;
        Ok(Self {
            character_id,
            player_connected,
        })
    }

    /// Writes the state for the bundle path. A slot without a known child is not sent.
    pub fn write(&self, writer: &mut Writer) {
        let character_id = self.character_id;
        let player_connected = self.player_connected;
        let children = children_mask(&[
            character_id.is_some(),
            false,
            false,
            player_connected.is_some(),
        ]);
        if children == 0 {
            writer.u8(NO_SLOT);
            return;
        }
        writer.u8(SLOT_2_BIT);
        write_children(writer, children, SLOT_2_CHILDREN);
        if let Some(id) = character_id {
            writer.u8(PRESENCE_FLAG_FIXED).bytes(&id);
        }
        if let Some(connected) = player_connected {
            writer.boolean(connected);
        }
    }
}

/// Reads one presence id (`FUN_14087ac50`): a flag byte whose bit 0 must be set, then 16 bytes.
fn read_presence_id(reader: &mut Reader<'_>) -> Result<[u8; RAW_ID_LEN], MessageError> {
    let flag = reader.u8()?;
    if flag & PRESENCE_FLAG_FIXED == 0 {
        return Err(MessageError::BadTag {
            field: "PlayerComponentReplicatedState characterId flag",
            tag: flag,
        });
    }
    reader.raw_id()
}

/// The empty state of player component `class`: the default of a class with its own reader here
/// (it writes the same single slot byte of 0), else an [`EmptyState`].
pub fn empty_state(class: u64) -> MemberState {
    match class {
        VITALS_TYPE_INDEX => MemberState::Vitals(VitalsState::default()),
        STAMINA_TYPE_INDEX => MemberState::Stamina(StaminaState::default()),
        COMBAT_STATUS_TYPE_INDEX => MemberState::CombatStatus(CombatStatusState::default()),
        PLAYER_COMPONENT_TYPE_INDEX => {
            MemberState::PlayerComponent(PlayerComponentState::default())
        }
        _ => MemberState::Empty(EmptyState { class }),
    }
}

/// The members of the empty states of every player component in [`PLAYER_EMPTY_COMPONENTS`].
pub fn player_empty_members() -> impl Iterator<Item = BundleMember> {
    PLAYER_EMPTY_COMPONENTS
        .iter()
        .map(|(component, class)| BundleMember {
            component: *component,
            state: empty_state(*class),
        })
}

fn has_own_reader(class: u64) -> bool {
    matches!(
        class,
        VITALS_TYPE_INDEX
            | STAMINA_TYPE_INDEX
            | COMBAT_STATUS_TYPE_INDEX
            | PLAYER_COMPONENT_TYPE_INDEX
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::{BundleGroup, BundlePayload};

    const LOCAL_SLOT: u64 = 1;
    /// A member: a 1-byte component index, a class id of at most 2 bytes, a 1-byte body.
    const MAX_EMPTY_MEMBER_BYTES: usize = 4;

    #[test]
    fn every_empty_player_component_is_one_zero_byte_and_reads_back() {
        let members: Vec<BundleMember> = player_empty_members().collect();
        for member in &members {
            let bytes = member.encode().expect("member");
            assert_eq!(bytes.last(), Some(&NO_SLOT), "{member:?}");
            assert!(bytes.len() <= MAX_EMPTY_MEMBER_BYTES, "{member:?}");
        }
        let payload = BundlePayload {
            groups: vec![BundleGroup {
                interest: LOCAL_SLOT,
                members: members.clone(),
            }],
        };
        let read = BundlePayload::decode(&payload.encode().expect("payload")).expect("read");
        assert_eq!(read.groups[0].members, members);
    }

    #[test]
    fn a_set_slot_of_an_empty_class_stops_the_walk() {
        let class = PLAYER_EMPTY_COMPONENTS[0].1;
        assert!(EmptyState::covers(class));
        assert!(
            !EmptyState::covers(VITALS_TYPE_INDEX),
            "vitals has its own reader"
        );
        assert!(
            !EmptyState::covers(PLAYER_COMPONENT_TYPE_INDEX),
            "player component has its own reader"
        );
        let mut reader = Reader::new(&[1]);
        assert!(matches!(
            EmptyState::read(class, &mut reader),
            Err(MessageError::Unsized { .. })
        ));
    }

    #[test]
    fn a_connected_player_component_round_trips() {
        const ID: [u8; RAW_ID_LEN] = [9; RAW_ID_LEN];
        let state = PlayerComponentState::connected(ID);
        let mut writer = Writer::new();
        state.write(&mut writer);
        let bytes = writer.finish();
        assert_eq!(bytes.first(), Some(&SLOT_2_BIT));
        let mut reader = Reader::new(&bytes);
        let read = PlayerComponentState::read(&mut reader).expect("read");
        reader.finish().expect("to the end");
        assert_eq!(read, state);
        assert_eq!(read.player_connected, Some(true));
        assert_eq!(read.character_id, Some(ID));
    }

    #[test]
    fn slot_1_of_a_player_component_stops_the_walk() {
        let mut reader = Reader::new(&[0b010]);
        assert!(matches!(
            PlayerComponentState::read(&mut reader),
            Err(MessageError::Unsized { .. })
        ));
    }
}
