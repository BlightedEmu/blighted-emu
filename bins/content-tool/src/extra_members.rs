// SPDX-License-Identifier: AGPL-3.0-or-later
//! Readers for replicated state classes that `nw-messages` has no reader for: the states of the
//! tutorial's static entities, of its encounters and spell slices, and of its boss (FTUE static
//! entity note, sections 2 and 3; FTUE level entity note).
//!
//! A body is one slot byte (`0x01`: slot 0), the child mask and the sent children in order. The
//! mask is one byte (bit `i` = child `i` is sent; at most 8 children) for most classes here; a
//! class with more children has grouped masks as in `nw-messages`: seven children per mask
//! byte, whose high bit says that another mask byte follows the children of this one. Each
//! layout below is fitted on the retail bodies of the tutorial captures (Observed); a field that
//! covers more than one child is a run of children that the captures only show sent together,
//! whose split is Unknown. A body that sets part of such a run, or a child whose layout is
//! Unknown, is refused.

use nw_messages::{PositionInTheWorldReplicatedState, Reader};
use nw_reflect::{TypeId, decode_type_id};

/// `MB::DetectionVolumeEventReplicatedState`.
pub const DETECTION_VOLUME_EVENT_TYPE_INDEX: u64 = 366;
pub use nw_messages::BOSS_PHASE_TYPE_INDEX;
/// `MB::TimelineComponentReplicatedState`.
pub const TIMELINE_TYPE_INDEX: u64 = 1647;
/// `MB::LootDropReplicatedState`.
pub const LOOT_DROP_TYPE_INDEX: u64 = 2027;
/// `MB::EncounterComponentReplicatedState`.
pub const ENCOUNTER_TYPE_INDEX: u64 = 2133;
/// `MB::SpellComponentReplicatedState`.
pub const SPELL_TYPE_INDEX: u64 = 2912;
/// `MB::InteractReplicatedState`.
pub const INTERACT_TYPE_INDEX: u64 = 2930;
/// `MB::BeamAttackComponentReplicatedState`.
pub const BEAM_ATTACK_TYPE_INDEX: u64 = 2947;
/// `MB::FtueIslandComponentReplicatedState`.
pub const FTUE_ISLAND_TYPE_INDEX: u64 = 3194;
/// `MB::OwnershipComponentReplicatedState`.
pub const OWNERSHIP_TYPE_INDEX: u64 = 3217;
/// `MB::SlayerScriptReplicatedState`.
pub const SLAYER_SCRIPT_TYPE_INDEX: u64 = 3362;
/// `MB::EncounterManagerComponentReplicatedState`.
pub const ENCOUNTER_MANAGER_TYPE_INDEX: u64 = 6786;

/// The slot byte of a body that sends slot 0 only.
const SLOT_0: u8 = 0x01;
/// Children one mask byte can name.
const MASK_CHILDREN: u32 = u8::BITS;
/// Children one grouped mask byte can name.
const GROUP_CHILDREN: usize = 7;
/// The child bits of a grouped mask byte.
const GROUP_CHILD_BITS: u8 = 0x7f;
/// The bit of a grouped mask byte that says another mask byte follows.
const GROUP_CONTINUE: u8 = 0x80;
/// A replicated list sent whole: its leading count is 0.
const LIST_FULL_FORM: u64 = 0;
/// Entries of a list delta that one entry mask byte covers.
const LIST_DELTA_GROUP: u64 = 8;
/// A version flag of 1 is followed by a version number.
const VERSION_PRESENT: u8 = 1;
/// Bytes of a `GdeRef`.
const GDE_REF: usize = 16;
/// Bytes of a facet entity id or a `u64` time.
const U64: usize = 8;
/// Bytes of a `u32` or an `f32`.
const U32: usize = 4;
/// Bytes of a `u8` or a `bool`.
const U8: usize = 1;
/// Bytes of the Ownership child after the guild name: a `u16` symbol, an RGBA `f32` colour, a
/// `u16` symbol and an RGBA `f32` colour (Inferred from a guild crest body).
const CREST: usize = 36;
/// Bytes of the last three Ownership children.
const OWNERSHIP_TAIL: usize = 16;
/// Bytes of the FtueIsland children after the player's `GdeRef` and facet.
const FTUE_ISLAND_TAIL: usize = 4;
/// Bytes of one BeamAttack list element (one body, all zero).
const BEAM_ELEMENT: usize = 74;
/// Bytes of one Encounter history entry: two `u32`s, a rising sequence number and a state
/// (Inferred from the values: 0, 1, 2, 3 on one encounter wave; 1 and 2 repeat on waves that
/// run again).
const ENCOUNTER_ENTRY: usize = 8;
/// Bytes of a position: `f32` east, `f32` north and a `u16` height.
const POSITION: usize = 10;
/// Bytes of the ids of an entity: its `GdeRef` and its facet entity.
const ENTITY_IDS: usize = 24;
/// Most bytes of a rotation: its mask byte and three `u16` components.
const ROTATION_MAX: usize = 7;
/// The slot byte and child mask of a `PositionInTheWorld` body that sends only its rotation.
const ROTATION_ONLY: [u8; 2] = [SLOT_0, 0x02];

/// The layout of one or more children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    /// `children` children, sent together, of `len` bytes in all.
    Fixed { children: u32, len: usize },
    /// A string: a prefix-coded length and the bytes.
    Text,
    /// An owner's character: a string id, and its name string when the id is not empty.
    OwnerName,
    /// A replicated list of fixed-size elements; `None` when the element size is Unknown (only
    /// empty lists can be read).
    List { element: Option<usize> },
    /// A plain vector: a prefix-coded count and that many elements of `element` bytes.
    Vector { element: usize },
    /// A rotation in the form of the `PositionInTheWorld` rotation child.
    Rotation,
    /// A child whose layout is Unknown.
    Unread,
}

const fn one(len: usize) -> Field {
    Field::Fixed { children: 1, len }
}

/// How a body names its sent children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Masks {
    /// One mask byte, one bit per child.
    Single,
    /// Grouped mask bytes (module comment); every field covers one child.
    Grouped,
}

/// One class and its children, in order.
struct Layout {
    class: u64,
    masks: Masks,
    fields: &'static [Field],
}

const LAYOUTS: [Layout; 12] = [
    Layout {
        class: DETECTION_VOLUME_EVENT_TYPE_INDEX,
        masks: Masks::Single,
        fields: &[one(U64), one(U64), one(U64), one(U64)],
    },
    Layout {
        class: BOSS_PHASE_TYPE_INDEX,
        masks: Masks::Single,
        fields: &[one(U32), one(U8)],
    },
    Layout {
        class: TIMELINE_TYPE_INDEX,
        masks: Masks::Single,
        fields: &[
            Field::List { element: Some(U8) },
            Field::List { element: Some(U32) },
        ],
    },
    Layout {
        class: LOOT_DROP_TYPE_INDEX,
        masks: Masks::Single,
        fields: &[one(U32)],
    },
    Layout {
        class: INTERACT_TYPE_INDEX,
        masks: Masks::Single,
        fields: &[one(U8), one(U32)],
    },
    Layout {
        class: BEAM_ATTACK_TYPE_INDEX,
        masks: Masks::Single,
        fields: &[Field::List {
            element: Some(BEAM_ELEMENT),
        }],
    },
    Layout {
        class: FTUE_ISLAND_TYPE_INDEX,
        masks: Masks::Single,
        fields: &[
            one(GDE_REF),
            one(U64),
            Field::Fixed {
                children: 2,
                len: FTUE_ISLAND_TAIL,
            },
        ],
    },
    Layout {
        class: OWNERSHIP_TYPE_INDEX,
        masks: Masks::Single,
        fields: &[
            one(GDE_REF),
            Field::Text,
            one(CREST),
            one(U8),
            Field::OwnerName,
            Field::Fixed {
                children: 3,
                len: OWNERSHIP_TAIL,
            },
        ],
    },
    Layout {
        class: SLAYER_SCRIPT_TYPE_INDEX,
        masks: Masks::Single,
        fields: &[Field::Unread, Field::Unread, Field::List { element: None }],
    },
    Layout {
        class: ENCOUNTER_TYPE_INDEX,
        masks: Masks::Single,
        fields: &[Field::Vector {
            element: ENCOUNTER_ENTRY,
        }],
    },
    Layout {
        class: ENCOUNTER_MANAGER_TYPE_INDEX,
        masks: Masks::Single,
        fields: &[one(U32)],
    },
    // Children (names from the Azoth catalog field list): spell data id, start position, start
    // rotation, child position, child rotation, previous attachment, attachment, caster, spawn
    // time, spawn count, area scaling. The children never seen sent are Unread.
    Layout {
        class: SPELL_TYPE_INDEX,
        masks: Masks::Grouped,
        fields: &[
            one(U32),
            one(POSITION),
            Field::Rotation,
            Field::Unread,
            Field::Unread,
            Field::Unread,
            one(ENTITY_IDS),
            one(ENTITY_IDS),
            Field::Unread,
            Field::Unread,
            one(U32),
        ],
    },
];

/// Reads a member header: the component integer and the class, when the class is a file slot.
pub fn read_member_header(reader: &mut Reader<'_>) -> Option<(u64, u64)> {
    let component = reader.prefixed().ok()?;
    let (class, class_len) = decode_type_id(reader.rest()).ok()?;
    let TypeId::FileSlot(class) = class else {
        return None;
    };
    reader.bytes(class_len).ok()?;
    Some((component, class))
}

/// Whether `class` has a reader here.
pub fn has_reader(class: u64) -> bool {
    LAYOUTS.iter().any(|layout| layout.class == class)
}

/// Reads one body of `class` from `reader`; `None` when the class has no reader here or the
/// bytes do not fit its layout.
pub fn read_body(class: u64, reader: &mut Reader<'_>) -> Option<()> {
    let layout = LAYOUTS.iter().find(|layout| layout.class == class)?;
    if reader.u8().ok()? != SLOT_0 {
        return None;
    }
    match layout.masks {
        Masks::Single => read_single(layout.fields, reader),
        Masks::Grouped => read_grouped(layout.fields, reader),
    }
}

/// Reads the children after one mask byte.
fn read_single(fields: &[Field], reader: &mut Reader<'_>) -> Option<()> {
    let mask = u32::from(reader.u8().ok()?);
    let mut child = 0;
    for field in fields {
        let children = match field {
            Field::Fixed { children, .. } => *children,
            _ => 1,
        };
        let run = ((1 << children) - 1) << child;
        child += children;
        match mask & run {
            0 => continue,
            sent if sent != run => return None,
            _ => read_field(*field, reader)?,
        }
    }
    (child >= MASK_CHILDREN || mask >> child == 0).then_some(())
}

/// Reads the children after grouped mask bytes.
fn read_grouped(fields: &[Field], reader: &mut Reader<'_>) -> Option<()> {
    let mut first = 0;
    loop {
        let byte = reader.u8().ok()?;
        let bits = byte & GROUP_CHILD_BITS;
        let last = byte & GROUP_CONTINUE == 0;
        if bits == 0 && last {
            return None;
        }
        for child in (0..GROUP_CHILDREN).filter(|child| bits >> child & 1 != 0) {
            match fields.get(first + child)? {
                Field::Fixed { children, .. } if *children != 1 => return None,
                field => read_field(*field, reader)?,
            }
        }
        if last {
            return Some(());
        }
        first += GROUP_CHILDREN;
        if first >= fields.len() {
            return None;
        }
    }
}

fn read_field(field: Field, reader: &mut Reader<'_>) -> Option<()> {
    match field {
        Field::Fixed { len, .. } => reader.bytes(len).ok().map(|_| ()),
        Field::Text => read_text(reader).map(|_| ()),
        Field::OwnerName => {
            if read_text(reader)? > 0 {
                read_text(reader)?;
            }
            Some(())
        }
        Field::List { element } => read_list(element, reader),
        Field::Vector { element } => {
            let count = usize::try_from(reader.prefixed().ok()?).ok()?;
            reader.bytes(count.checked_mul(element)?).ok().map(|_| ())
        }
        Field::Rotation => read_rotation(reader),
        Field::Unread => None,
    }
}

/// Reads a rotation with the `nw-messages` reader of the `PositionInTheWorld` rotation child.
fn read_rotation(reader: &mut Reader<'_>) -> Option<()> {
    let rest = reader.rest();
    let mut body = ROTATION_ONLY.to_vec();
    body.extend_from_slice(&rest[..rest.len().min(ROTATION_MAX)]);
    let mut inner = Reader::new(&body);
    PositionInTheWorldReplicatedState::read(&mut inner).ok()?;
    let used = body.len() - inner.remaining() - ROTATION_ONLY.len();
    reader.bytes(used).ok().map(|_| ())
}

/// Reads a string; returns its length.
fn read_text(reader: &mut Reader<'_>) -> Option<usize> {
    let len = usize::try_from(reader.prefixed().ok()?).ok()?;
    reader.bytes(len).ok()?;
    Some(len)
}

/// Reads a version: a flag byte, and a wide number when the flag is 1.
fn read_version(reader: &mut Reader<'_>) -> Option<()> {
    if reader.u8().ok()? == VERSION_PRESENT {
        reader.wide_prefixed().ok()?;
    }
    Some(())
}

/// Reads a replicated list (paperdoll note, section 2): a leading count of 0, a version, a
/// count and the elements; or, for a delta, groups of up to 8 entries, each group an entry mask
/// byte and per entry a wide key, a version and, when its mask bit is set, the element.
fn read_list(element: Option<usize>, reader: &mut Reader<'_>) -> Option<()> {
    let read_elements = |count: u64, reader: &mut Reader<'_>| match (count, element) {
        (0, _) => Some(()),
        (_, None) => None,
        (count, Some(len)) => {
            let total = usize::try_from(count).ok()?.checked_mul(len)?;
            reader.bytes(total).ok().map(|_| ())
        }
    };
    let leading = reader.prefixed().ok()?;
    if leading == LIST_FULL_FORM {
        read_version(reader)?;
        let count = reader.prefixed().ok()?;
        return read_elements(count, reader);
    }
    let mut left = leading;
    while left > 0 {
        let mask = reader.u8().ok()?;
        let entries = left.min(LIST_DELTA_GROUP);
        for entry in 0..entries {
            reader.wide_prefixed().ok()?;
            read_version(reader)?;
            read_elements(u64::from(mask >> entry & 1), reader)?;
        }
        left -= entries;
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reads(class: u64, body: &str) -> bool {
        let bytes = hex::decode(body).expect("hex");
        let mut reader = Reader::new(&bytes);
        read_body(class, &mut reader).is_some() && reader.is_empty()
    }

    /// The default Ownership body: no owner, white crest colours.
    const OWNERSHIP_DEFAULT: &str = concat!(
        "01ff",
        "00000000000000000000000000000000",
        "00",
        "0000",
        "3f8000003f8000003f8000003f800000",
        "0000",
        "3f8000003f8000003f8000003f800000",
        "00",
        "00",
        "02010000000000000000000f00000000"
    );

    #[test]
    fn retail_like_bodies_read_to_their_end() {
        assert!(reads(OWNERSHIP_TYPE_INDEX, OWNERSHIP_DEFAULT));
        assert!(reads(
            FTUE_ISLAND_TYPE_INDEX,
            "010f1111111111111111111111111111111122222222222222220000801f"
        ));
        assert!(reads(
            DETECTION_VOLUME_EVENT_TYPE_INDEX,
            "010a00000000000000010000000000000002"
        ));
        assert!(reads(TIMELINE_TYPE_INDEX, "010300010201000001020100000000"));
        assert!(reads(TIMELINE_TYPE_INDEX, "01020001040142c80000"));
        assert!(reads(INTERACT_TYPE_INDEX, "01020000000a"));
        assert!(reads(LOOT_DROP_TYPE_INDEX, "010100000001"));
        assert!(reads(SLAYER_SCRIPT_TYPE_INDEX, "010400010300"));
        assert!(reads(BOSS_PHASE_TYPE_INDEX, "01030000000001"));
    }

    #[test]
    fn an_owner_name_follows_a_non_empty_owner_id() {
        let named = concat!(
            "01ff",
            "00000000000000000000000000000000",
            "00",
            "0000",
            "3f8000003f8000003f8000003f800000",
            "0000",
            "3f8000003f8000003f8000003f800000",
            "00",
            "03616263",
            "03787978",
            "02010000000000000000000f00000000"
        );
        assert!(reads(OWNERSHIP_TYPE_INDEX, named));
    }

    #[test]
    fn a_list_delta_reads_its_entries() {
        // Two entries: entry 0 set (key 1, no version, element 0x2a), entry 1 removed (key 2).
        assert!(reads(
            TIMELINE_TYPE_INDEX,
            "01010201 01002a 0200".replace(' ', "").as_str()
        ));
        assert!(!reads(TIMELINE_TYPE_INDEX, "0101020101002a"));
    }

    #[test]
    fn unknown_layouts_are_refused() {
        // Part of the FtueIsland tail run.
        assert!(!reads(
            FTUE_ISLAND_TYPE_INDEX,
            "01041111111111111111111111111111111122222222222222220000"
        ));
        // A SlayerScript child whose layout is Unknown.
        assert!(!reads(SLAYER_SCRIPT_TYPE_INDEX, "010100"));
        // A child past the known ones.
        assert!(!reads(LOOT_DROP_TYPE_INDEX, "010300000001"));
        // A list with elements of Unknown size.
        assert!(!reads(SLAYER_SCRIPT_TYPE_INDEX, "0104000103012a"));
        // Another slot.
        assert!(!reads(INTERACT_TYPE_INDEX, "02020000000a"));
        assert!(!has_reader(1));
        assert!(has_reader(OWNERSHIP_TYPE_INDEX));
    }

    /// A force-grab spell body with a short rotation and no attachment (ids replaced).
    const SPELL_SHORT_ROTATION: &str = concat!(
        "01c7",
        "79fc590a",
        "43f06d74436319250f16",
        "039fbd",
        "00000000000000000000000000000000",
        "00000000ffffffff",
        "09",
        "11111111111111111111111111111111",
        "2222222222222222",
        "3f800000"
    );
    /// A force-grab spell body with a full rotation and an attachment (ids replaced).
    const SPELL_FULL_ROTATION: &str = concat!(
        "01c7",
        "f49e8e89",
        "43f04a95436359330eee",
        "407ffd7ff9f89d",
        "33333333333333333333333333333333",
        "4444444444444444",
        "09",
        "11111111111111111111111111111111",
        "2222222222222222",
        "3f800000"
    );

    #[test]
    fn encounter_and_spell_bodies_read_to_their_end() {
        // History entries (0, 0) to (3, 3), and the entries of a wave that runs again.
        assert!(reads(
            ENCOUNTER_TYPE_INDEX,
            "0101040000000000000000000000010000000100000002000000020000000300000003"
        ));
        assert!(reads(
            ENCOUNTER_TYPE_INDEX,
            "0101020000000a000000010000000b00000002"
        ));
        assert!(reads(ENCOUNTER_MANAGER_TYPE_INDEX, "010100000002"));
        assert!(reads(SPELL_TYPE_INDEX, SPELL_SHORT_ROTATION));
        assert!(reads(SPELL_TYPE_INDEX, SPELL_FULL_ROTATION));
        // The data id, then the area scaling in the second mask byte; the area scaling alone.
        assert!(reads(SPELL_TYPE_INDEX, "0181000000070800000000"));
        assert!(reads(SPELL_TYPE_INDEX, "0180083f800000"));
    }

    #[test]
    fn spell_children_of_unknown_layout_are_refused() {
        // Child 3 (the child position) was never seen sent.
        assert!(!reads(SPELL_TYPE_INDEX, "010800000000000000000000"));
        // Child 11 is past the known children.
        assert!(!reads(SPELL_TYPE_INDEX, "01810000000710"));
        // A last mask byte without children.
        assert!(!reads(SPELL_TYPE_INDEX, "018000"));
        // An encounter history cut short.
        assert!(!reads(
            ENCOUNTER_TYPE_INDEX,
            "010102000000000000000000000001"
        ));
    }
}
