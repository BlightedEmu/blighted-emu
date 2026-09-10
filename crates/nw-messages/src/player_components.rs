// SPDX-License-Identifier: AGPL-3.0-or-later
//! The replicated components of the player prefab (world-entry note, "Spawn condition traced").
//! A member's component index picks the component of the portrayed entity that takes the state
//! (`FUN_14167beb0`). When the player prefab is portrayed with no state for its other components,
//! the real client crashes while it applies the cached states. So the player's group carries one
//! state for every replicated component: the spawn card, the position, and an empty state ("no
//! slot set") for each component below.
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
use crate::vitals::{VITALS_TYPE_INDEX, VitalsState};
use crate::wire::{MessageError, Reader, Writer};

/// The empty slot byte of a generic-reader body.
const NO_SLOT: u8 = 0;

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

/// The empty state of player component `class`: the default of a class with its own reader here
/// (it writes the same single slot byte of 0), else an [`EmptyState`].
pub fn empty_state(class: u64) -> MemberState {
    match class {
        VITALS_TYPE_INDEX => MemberState::Vitals(VitalsState::default()),
        STAMINA_TYPE_INDEX => MemberState::Stamina(StaminaState::default()),
        COMBAT_STATUS_TYPE_INDEX => MemberState::CombatStatus(CombatStatusState::default()),
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
        VITALS_TYPE_INDEX | STAMINA_TYPE_INDEX | COMBAT_STATUS_TYPE_INDEX
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
        let mut reader = Reader::new(&[1]);
        assert!(matches!(
            EmptyState::read(class, &mut reader),
            Err(MessageError::Unsized { .. })
        ));
    }
}
