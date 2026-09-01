// SPDX-License-Identifier: AGPL-3.0-or-later
//! Readers from a `region.metadata` or capital `.metadata` ObjectStream to typed records
//! (M1b spec section 3.3). Sections and fields are matched by name CRC. Unknown sections and
//! fields are skipped and counted.

use std::collections::BTreeMap;

use nw_formats::objectstream::{Element, ObjectStreamError};

use crate::records::{
    EncounterPoint, GatherablePoint, Landmark, LorePoint, NpcPoint, Position, VitalsSpawn,
};

/// Section `npcData`.
pub const SECTION_NPCS: u32 = 0x1fe2_c4e5;
/// Vitals spawn section (name Unknown; found by its `vitalsId` field).
pub const SECTION_VITALS: u32 = 0x390b_e215;
/// Gatherable section (name Unknown; found by its `gatherableId` field).
pub const SECTION_GATHERABLES: u32 = 0x5f96_b5ce;
/// Landmark section (name Unknown; found by its `landmarkType` field).
pub const SECTION_LANDMARKS: u32 = 0x6e2c_bf8b;
/// Encounter section (name Unknown; found by its `encounterId` field).
pub const SECTION_ENCOUNTERS: u32 = 0xcf91_f6ef;
/// Section `loreData`.
pub const SECTION_LORE: u32 = 0x0417_2e19;

/// Field `territoryIds`: a list of `u16` elements.
pub const FIELD_TERRITORY_IDS: u32 = 0x656c_287c;
/// Field `worldPosition`: a `Vector3`.
pub const FIELD_WORLD_POSITION: u32 = 0xdd21_2bdf;
/// Field `npcId`: a Crc32 wrapper.
pub const FIELD_NPC_ID: u32 = 0x3f6e_9e7b;
/// Field `factionType`: a `u8`.
pub const FIELD_FACTION_TYPE: u32 = 0xcc23_5572;
/// Field `vitalsId`: a Crc32 wrapper.
pub const FIELD_VITALS_ID: u32 = 0xfc75_b47f;
/// Field `vitalsCategoryId`: a Crc32 wrapper.
pub const FIELD_VITALS_CATEGORY_ID: u32 = 0xd407_ae7f;
/// Field `vitalsLevel`: a `u32`.
pub const FIELD_VITALS_LEVEL: u32 = 0x7c84_56ea;
/// Field `isEncounter`: a bool.
pub const FIELD_IS_ENCOUNTER: u32 = 0xd2bb_1aae;
/// Field `gatherableId`: a Crc32 wrapper.
pub const FIELD_GATHERABLE_ID: u32 = 0xacf6_af9e;
/// Field `landmarkType`: a `u8`.
pub const FIELD_LANDMARK_TYPE: u32 = 0x6acb_daed;
/// Field `landmarkData`: a string.
pub const FIELD_LANDMARK_DATA: u32 = 0x4be6_7ea7;
/// Field `Radius`: an `f32`.
pub const FIELD_RADIUS: u32 = 0x3b7c_6e5a;
/// Field `encounterId`: a string.
pub const FIELD_ENCOUNTER_ID: u32 = 0xcc9e_a92a;
/// Field `loreId`: a Crc32 wrapper.
pub const FIELD_LORE_ID: u32 = 0xa45c_9a6d;
/// Field `Value`: the `u32` inside a Crc32 wrapper.
pub const FIELD_CRC_VALUE: u32 = 0x1d77_5834;

const NPC_FIELDS: &[u32] = &[
    FIELD_TERRITORY_IDS,
    FIELD_NPC_ID,
    FIELD_WORLD_POSITION,
    FIELD_FACTION_TYPE,
];
const VITALS_FIELDS: &[u32] = &[
    FIELD_TERRITORY_IDS,
    FIELD_VITALS_ID,
    FIELD_VITALS_CATEGORY_ID,
    FIELD_VITALS_LEVEL,
    FIELD_WORLD_POSITION,
    FIELD_IS_ENCOUNTER,
];
const GATHERABLE_FIELDS: &[u32] = &[
    FIELD_TERRITORY_IDS,
    FIELD_GATHERABLE_ID,
    FIELD_WORLD_POSITION,
    FIELD_IS_ENCOUNTER,
];
const LANDMARK_FIELDS: &[u32] = &[
    FIELD_TERRITORY_IDS,
    FIELD_LANDMARK_TYPE,
    FIELD_LANDMARK_DATA,
    FIELD_WORLD_POSITION,
    FIELD_RADIUS,
];
const ENCOUNTER_FIELDS: &[u32] = &[
    FIELD_TERRITORY_IDS,
    FIELD_ENCOUNTER_ID,
    FIELD_WORLD_POSITION,
];
const LORE_FIELDS: &[u32] = &[FIELD_TERRITORY_IDS, FIELD_LORE_ID, FIELD_WORLD_POSITION];
/// Section key for a root child without a name.
const UNNAMED_SECTION: u32 = 0;

/// The records of one or more metadata files.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MetadataRecords {
    /// NPC points.
    pub npcs: Vec<NpcPoint>,
    /// Creature spawn points.
    pub vitals: Vec<VitalsSpawn>,
    /// Gatherable points.
    pub gatherables: Vec<GatherablePoint>,
    /// Lore points.
    pub lore: Vec<LorePoint>,
    /// Landmarks.
    pub landmarks: Vec<Landmark>,
    /// Encounter points.
    pub encounters: Vec<EncounterPoint>,
}

/// What the readers skipped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkipCounts {
    /// Records in sections without a reader, by section name CRC.
    pub sections: BTreeMap<u32, u64>,
    /// Unknown fields, by (section name CRC, field name CRC).
    pub fields: BTreeMap<(u32, u32), u64>,
}

impl SkipCounts {
    /// Adds the counts of `other`.
    pub fn merge(&mut self, other: &SkipCounts) {
        for (section, count) in &other.sections {
            *self.sections.entry(*section).or_default() += count;
        }
        for (key, count) in &other.fields {
            *self.fields.entry(*key).or_default() += count;
        }
    }
}

/// Error from a metadata reader.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MetadataError {
    /// A required field is missing.
    #[error("section {section:08x}: field {field:08x} is missing")]
    MissingField {
        /// Section name CRC.
        section: u32,
        /// Field name CRC.
        field: u32,
    },
    /// A field value does not fit its type.
    #[error("section {section:08x}: field {field:08x}: {source}")]
    Value {
        /// Section name CRC.
        section: u32,
        /// Field name CRC.
        field: u32,
        /// The value error.
        #[source]
        source: ObjectStreamError,
    },
}

/// Reads every known section of a metadata root into `records` and counts what it skips.
pub fn read_metadata(
    root: &Element,
    records: &mut MetadataRecords,
    skipped: &mut SkipCounts,
) -> Result<(), MetadataError> {
    for section in &root.children {
        match section.name_crc.unwrap_or(UNNAMED_SECTION) {
            SECTION_NPCS => {
                read_section(section, NPC_FIELDS, skipped, read_npc, &mut records.npcs)?
            }
            SECTION_VITALS => read_section(
                section,
                VITALS_FIELDS,
                skipped,
                read_vitals,
                &mut records.vitals,
            )?,
            SECTION_GATHERABLES => read_section(
                section,
                GATHERABLE_FIELDS,
                skipped,
                read_gatherable,
                &mut records.gatherables,
            )?,
            SECTION_LANDMARKS => read_section(
                section,
                LANDMARK_FIELDS,
                skipped,
                read_landmark,
                &mut records.landmarks,
            )?,
            SECTION_ENCOUNTERS => read_section(
                section,
                ENCOUNTER_FIELDS,
                skipped,
                read_encounter,
                &mut records.encounters,
            )?,
            SECTION_LORE => {
                read_section(section, LORE_FIELDS, skipped, read_lore, &mut records.lore)?
            }
            other => {
                *skipped.sections.entry(other).or_default() += section.children.len() as u64;
            }
        }
    }
    Ok(())
}

fn read_section<T>(
    section: &Element,
    known: &[u32],
    skipped: &mut SkipCounts,
    read: fn(&Fields<'_>) -> Result<T, MetadataError>,
    out: &mut Vec<T>,
) -> Result<(), MetadataError> {
    let section_crc = section.name_crc.unwrap_or(UNNAMED_SECTION);
    for record in &section.children {
        let fields = Fields::new(section_crc, record, known, skipped);
        out.push(read(&fields)?);
    }
    Ok(())
}

fn read_npc(fields: &Fields<'_>) -> Result<NpcPoint, MetadataError> {
    Ok(NpcPoint {
        npc_id: fields.crc(FIELD_NPC_ID)?,
        position: fields.position()?,
        faction_type: fields.value(FIELD_FACTION_TYPE, Element::as_u8)?,
        territory_ids: fields.territory_ids()?,
    })
}

fn read_vitals(fields: &Fields<'_>) -> Result<VitalsSpawn, MetadataError> {
    Ok(VitalsSpawn {
        vitals_id: fields.crc(FIELD_VITALS_ID)?,
        category_id: fields.crc(FIELD_VITALS_CATEGORY_ID)?,
        level: fields.value(FIELD_VITALS_LEVEL, Element::as_u32)?,
        position: fields.position()?,
        is_encounter: fields.value(FIELD_IS_ENCOUNTER, Element::as_bool)?,
        territory_ids: fields.territory_ids()?,
    })
}

fn read_gatherable(fields: &Fields<'_>) -> Result<GatherablePoint, MetadataError> {
    Ok(GatherablePoint {
        gatherable_id: fields.crc(FIELD_GATHERABLE_ID)?,
        position: fields.position()?,
        is_encounter: fields.value(FIELD_IS_ENCOUNTER, Element::as_bool)?,
        territory_ids: fields.territory_ids()?,
    })
}

fn read_landmark(fields: &Fields<'_>) -> Result<Landmark, MetadataError> {
    Ok(Landmark {
        landmark_type: fields.value(FIELD_LANDMARK_TYPE, Element::as_u8)?,
        landmark_data: fields.text(FIELD_LANDMARK_DATA)?,
        position: fields.position()?,
        radius: fields.value(FIELD_RADIUS, Element::as_f32)?,
        territory_ids: fields.territory_ids()?,
    })
}

fn read_encounter(fields: &Fields<'_>) -> Result<EncounterPoint, MetadataError> {
    Ok(EncounterPoint {
        encounter_id: fields.text(FIELD_ENCOUNTER_ID)?,
        position: fields.position()?,
        territory_ids: fields.territory_ids()?,
    })
}

fn read_lore(fields: &Fields<'_>) -> Result<LorePoint, MetadataError> {
    Ok(LorePoint {
        lore_id: fields.crc(FIELD_LORE_ID)?,
        position: fields.position()?,
        territory_ids: fields.territory_ids()?,
    })
}

/// The known fields of one record.
struct Fields<'a> {
    section: u32,
    known: Vec<(u32, &'a Element)>,
}

impl<'a> Fields<'a> {
    /// Collects the fields of `record` whose CRC is in `known`; counts the others.
    fn new(section: u32, record: &'a Element, known: &[u32], skipped: &mut SkipCounts) -> Self {
        let mut fields = Vec::new();
        for field in &record.children {
            match field.name_crc {
                Some(crc) if known.contains(&crc) => fields.push((crc, field)),
                other => {
                    let key = (section, other.unwrap_or(UNNAMED_SECTION));
                    *skipped.fields.entry(key).or_default() += 1;
                }
            }
        }
        Self {
            section,
            known: fields,
        }
    }

    fn get(&self, field: u32) -> Result<&'a Element, MetadataError> {
        self.known
            .iter()
            .find(|(crc, _)| *crc == field)
            .map(|(_, element)| *element)
            .ok_or(MetadataError::MissingField {
                section: self.section,
                field,
            })
    }

    fn value<T>(
        &self,
        field: u32,
        read: fn(&Element) -> Result<T, ObjectStreamError>,
    ) -> Result<T, MetadataError> {
        read(self.get(field)?).map_err(|source| MetadataError::Value {
            section: self.section,
            field,
            source,
        })
    }

    fn text(&self, field: u32) -> Result<String, MetadataError> {
        self.value(field, |element| element.as_string().map(str::to_owned))
    }

    fn position(&self) -> Result<Position, MetadataError> {
        self.value(FIELD_WORLD_POSITION, Element::as_vec3)
    }

    /// The `u32` inside a Crc32 wrapper field.
    fn crc(&self, field: u32) -> Result<u32, MetadataError> {
        let value = self
            .get(field)?
            .child(FIELD_CRC_VALUE)
            .ok_or(MetadataError::MissingField {
                section: self.section,
                field: FIELD_CRC_VALUE,
            })?;
        value.as_u32().map_err(|source| MetadataError::Value {
            section: self.section,
            field,
            source,
        })
    }

    fn territory_ids(&self) -> Result<Vec<u16>, MetadataError> {
        self.value(FIELD_TERRITORY_IDS, |list| {
            list.children.iter().map(Element::as_u16).collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use nw_formats::objectstream::lowercase_crc32;

    use super::*;

    const FIELD_ELEMENT: u32 = 0x4140_5e39;
    const UNKNOWN_FIELD: u32 = 0xb9a5_aca4;
    const UNKNOWN_SECTION: u32 = 0xc496_c848;
    const POSITION: Position = [8537.24, 8199.65, 584.83];
    const TERRITORY: u16 = 0x10;
    const VITALS_ID: u32 = 0xae65_fdc7;
    const CATEGORY_ID: u32 = 0x1d95_b3e9;
    const LEVEL: u32 = 46;

    fn vec3(position: Position) -> Element {
        Element::with_value(
            FIELD_WORLD_POSITION,
            position
                .iter()
                .flat_map(|value| value.to_be_bytes())
                .collect(),
        )
    }

    fn crc(field: u32, value: u32) -> Element {
        Element::with_children(
            field,
            vec![Element::with_value(
                FIELD_CRC_VALUE,
                value.to_be_bytes().to_vec(),
            )],
        )
    }

    fn territories() -> Element {
        Element::with_children(
            FIELD_TERRITORY_IDS,
            vec![Element::with_value(
                FIELD_ELEMENT,
                TERRITORY.to_be_bytes().to_vec(),
            )],
        )
    }

    fn record(fields: Vec<Element>) -> Element {
        Element::with_children(FIELD_ELEMENT, fields)
    }

    fn root(sections: Vec<Element>) -> Element {
        Element {
            children: sections,
            ..Element::default()
        }
    }

    fn vitals_record() -> Element {
        record(vec![
            territories(),
            crc(FIELD_VITALS_ID, VITALS_ID),
            crc(FIELD_VITALS_CATEGORY_ID, CATEGORY_ID),
            Element::with_value(FIELD_VITALS_LEVEL, LEVEL.to_be_bytes().to_vec()),
            vec3(POSITION),
            Element::with_value(UNKNOWN_FIELD, vec![1]),
            Element::with_value(FIELD_IS_ENCOUNTER, vec![0]),
        ])
    }

    fn read(root: &Element) -> Result<(MetadataRecords, SkipCounts), MetadataError> {
        let mut records = MetadataRecords::default();
        let mut skipped = SkipCounts::default();
        read_metadata(root, &mut records, &mut skipped)?;
        Ok((records, skipped))
    }

    #[test]
    fn field_constants_are_crcs_of_their_names() {
        let names = [
            (SECTION_NPCS, "npcData"),
            (SECTION_LORE, "loreData"),
            (FIELD_TERRITORY_IDS, "territoryIds"),
            (FIELD_WORLD_POSITION, "worldPosition"),
            (FIELD_NPC_ID, "npcId"),
            (FIELD_FACTION_TYPE, "factionType"),
            (FIELD_VITALS_ID, "vitalsId"),
            (FIELD_VITALS_CATEGORY_ID, "vitalsCategoryId"),
            (FIELD_VITALS_LEVEL, "vitalsLevel"),
            (FIELD_IS_ENCOUNTER, "isEncounter"),
            (FIELD_GATHERABLE_ID, "gatherableId"),
            (FIELD_LANDMARK_TYPE, "landmarkType"),
            (FIELD_LANDMARK_DATA, "landmarkData"),
            (FIELD_RADIUS, "Radius"),
            (FIELD_ENCOUNTER_ID, "encounterId"),
            (FIELD_LORE_ID, "loreId"),
            (FIELD_CRC_VALUE, "Value"),
        ];
        for (constant, name) in names {
            assert_eq!(constant, lowercase_crc32(name), "{name}");
        }
    }

    #[test]
    fn reads_a_vitals_record_and_counts_the_unknown_field() {
        let (records, skipped) = read(&root(vec![Element::with_children(
            SECTION_VITALS,
            vec![vitals_record()],
        )]))
        .unwrap();
        assert_eq!(
            records.vitals,
            [VitalsSpawn {
                vitals_id: VITALS_ID,
                category_id: CATEGORY_ID,
                level: LEVEL,
                position: POSITION,
                is_encounter: false,
                territory_ids: vec![TERRITORY],
            }]
        );
        assert_eq!(skipped.fields[&(SECTION_VITALS, UNKNOWN_FIELD)], 1);
    }

    #[test]
    fn reads_every_other_record_kind() {
        let sections = vec![
            Element::with_children(
                SECTION_NPCS,
                vec![record(vec![
                    territories(),
                    crc(FIELD_NPC_ID, VITALS_ID),
                    vec3(POSITION),
                    Element::with_value(FIELD_FACTION_TYPE, vec![3]),
                ])],
            ),
            Element::with_children(
                SECTION_GATHERABLES,
                vec![record(vec![
                    territories(),
                    crc(FIELD_GATHERABLE_ID, VITALS_ID),
                    vec3(POSITION),
                    Element::with_value(FIELD_IS_ENCOUNTER, vec![1]),
                ])],
            ),
            Element::with_children(
                SECTION_LANDMARKS,
                vec![record(vec![
                    territories(),
                    Element::with_value(FIELD_LANDMARK_TYPE, vec![14]),
                    Element::with_value(FIELD_LANDMARK_DATA, Vec::new()),
                    vec3(POSITION),
                    Element::with_value(FIELD_RADIUS, 2.5_f32.to_be_bytes().to_vec()),
                ])],
            ),
            Element::with_children(
                SECTION_ENCOUNTERS,
                vec![record(vec![
                    territories(),
                    Element::with_value(FIELD_ENCOUNTER_ID, b"WorldEvent_Enc".to_vec()),
                    vec3(POSITION),
                ])],
            ),
            Element::with_children(
                SECTION_LORE,
                vec![record(vec![
                    territories(),
                    crc(FIELD_LORE_ID, VITALS_ID),
                    vec3(POSITION),
                ])],
            ),
        ];
        let (records, skipped) = read(&root(sections)).unwrap();
        assert_eq!(records.npcs[0].faction_type, 3);
        assert_eq!(records.npcs[0].npc_id, VITALS_ID);
        assert!(records.gatherables[0].is_encounter);
        assert_eq!(records.landmarks[0].landmark_type, 14);
        assert_eq!(records.landmarks[0].landmark_data, "");
        assert_eq!(records.landmarks[0].radius, 2.5);
        assert_eq!(records.encounters[0].encounter_id, "WorldEvent_Enc");
        assert_eq!(records.lore[0].lore_id, VITALS_ID);
        assert_eq!(records.lore[0].position, POSITION);
        assert_eq!(skipped, SkipCounts::default());
    }

    #[test]
    fn unknown_section_is_counted() {
        let section = Element::with_children(UNKNOWN_SECTION, vec![record(Vec::new()); 2]);
        let (records, skipped) = read(&root(vec![section])).unwrap();
        assert_eq!(records, MetadataRecords::default());
        assert_eq!(skipped.sections[&UNKNOWN_SECTION], 2);
    }

    #[test]
    fn missing_field_is_an_error() {
        let mut record = vitals_record();
        record
            .children
            .retain(|field| field.name_crc != Some(FIELD_VITALS_LEVEL));
        let section = Element::with_children(SECTION_VITALS, vec![record]);
        assert_eq!(
            read(&root(vec![section])),
            Err(MetadataError::MissingField {
                section: SECTION_VITALS,
                field: FIELD_VITALS_LEVEL
            })
        );
    }

    #[test]
    fn wrong_value_size_is_an_error() {
        let mut record = vitals_record();
        for field in &mut record.children {
            if field.name_crc == Some(FIELD_WORLD_POSITION) {
                field.value.pop();
            }
        }
        let section = Element::with_children(SECTION_VITALS, vec![record]);
        assert!(matches!(
            read(&root(vec![section])),
            Err(MetadataError::Value {
                section: SECTION_VITALS,
                field: FIELD_WORLD_POSITION,
                ..
            })
        ));
    }

    #[test]
    fn crc_wrapper_without_value_is_an_error() {
        let mut record = vitals_record();
        for field in &mut record.children {
            if field.name_crc == Some(FIELD_VITALS_ID) {
                field.children.clear();
            }
        }
        let section = Element::with_children(SECTION_VITALS, vec![record]);
        assert_eq!(
            read(&root(vec![section])),
            Err(MetadataError::MissingField {
                section: SECTION_VITALS,
                field: FIELD_CRC_VALUE
            })
        );
    }

    #[test]
    fn merge_adds_counts() {
        let mut total = SkipCounts::default();
        let mut part = SkipCounts::default();
        part.sections.insert(UNKNOWN_SECTION, 2);
        part.fields.insert((SECTION_VITALS, UNKNOWN_FIELD), 3);
        total.merge(&part);
        total.merge(&part);
        assert_eq!(total.sections[&UNKNOWN_SECTION], 4);
        assert_eq!(total.fields[&(SECTION_VITALS, UNKNOWN_FIELD)], 6);
    }
}
