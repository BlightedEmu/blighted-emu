// SPDX-License-Identifier: AGPL-3.0-or-later
//! The interact tags of a level object's slice (finding "task interact"): the
//! `m_interactTagString` of each interact component. A `TaskInteract` objective task counts an
//! interact whose tag matches its `InteractTag` (client text: "When a player performs interact
//! that has an InteractTag that matches this string, we will increment progress"). Observed: the
//! Monarch's Bluffs settlement slice holds `SettlementStorage`, the town alchemy station slice
//! `SettlementAlchemy`.

use nw_formats::objectstream::{Element, lowercase_crc32};
use serde::{Deserialize, Serialize};

/// The field of an interact component that holds its tag.
const INTERACT_TAG_FIELD: &str = "m_interactTagString";

/// A level object and the interact tags of its slice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InteractTagged {
    /// The object.
    pub owner: TagOwner,
    /// Its tags, each once, in slice order.
    pub tags: Vec<String>,
}

/// A level object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TagOwner {
    /// A capital, by its GUID text.
    Capital(String),
    /// A spawned slice, by its GDE id.
    Spawned(u64),
}

/// The interact tags of the slice `root`, each once, in slice order, without the white space and
/// NUL bytes around them; empty ones are left out. Observed: one slice stores
/// `IO_MB_MSQ2_Anchor\r\n`, which its quest task names `IO_MB_MSQ2_Anchor` (Inferred: retail
/// ignores the line end, or the task could not be done).
pub fn interact_tags_of(root: &Element) -> Vec<String> {
    let field = lowercase_crc32(INTERACT_TAG_FIELD);
    let mut tags = Vec::new();
    collect(root, field, &mut tags);
    tags
}

fn collect(element: &Element, field: u32, tags: &mut Vec<String>) {
    if element.name_crc == Some(field) {
        let text = String::from_utf8_lossy(&element.value);
        let tag = text.trim_matches(|c: char| c.is_whitespace() || c == '\0');
        if !tag.is_empty() && !tags.iter().any(|known| known == tag) {
            tags.push(tag.to_owned());
        }
    }
    for child in &element.children {
        collect(child, field, tags);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tagged(value: &str) -> Element {
        Element::with_value(
            lowercase_crc32(INTERACT_TAG_FIELD),
            value.as_bytes().to_vec(),
        )
    }

    #[test]
    fn every_interact_component_gives_its_tag_once() {
        let component = |children| Element {
            children,
            ..Element::default()
        };
        let root = component(vec![
            component(vec![tagged("SettlementStorage")]),
            component(vec![tagged("SettlementStorage"), tagged(""), tagged(" \0")]),
            component(vec![tagged("IO_MB_MSQ2_Anchor\r\n")]),
            component(vec![tagged("SettlementAlchemy")]),
            component(vec![Element::with_value(
                lowercase_crc32("m_name"),
                b"Storage".to_vec(),
            )]),
        ]);
        assert_eq!(
            interact_tags_of(&root),
            [
                "SettlementStorage",
                "IO_MB_MSQ2_Anchor",
                "SettlementAlchemy"
            ]
        );
    }
}
