// SPDX-License-Identifier: MIT OR Apache-2.0
//! The Mannequin and CAGE files that number a creature's animations (creature-animation note,
//! 2026-09-18): the controller definitions (`*_controllerdefs.xml`: the fragments and scopes),
//! the tag definition (`*tags.xml`: the bits of the tag state) and the CAGE grid (`*.grid`: the
//! actions).
//!
//! A creature's replicated animation names a fragment by its index + 1 in `<FragmentDefs>`, an
//! action by its index + 1 among the grid's actions, and its tags as a 12-byte tag state.

use std::collections::HashMap;

use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};

/// Bytes of a tag state (the client's bit assignment, `FUN_146d88770`, fills 12 bytes).
pub const TAG_STATE_LEN: usize = 12;
/// Bits in one byte of the tag state.
const BITS_PER_BYTE: u8 = 8;

const CONTROLLER_DEF: &str = "ControllerDef";
const TAGS_ELEMENT: &str = "Tags";
const FRAGMENT_DEFS: &str = "FragmentDefs";
const SCOPE_DEFS: &str = "ScopeDefs";
const FILENAME_ATTRIBUTE: &str = "filename";
const TAG_DEFINITION: &str = "TagDefinition";
const GROUP_ELEMENT: &str = "Group";
const TAG_ELEMENT: &str = "Tag";
const NAME_ATTRIBUTE: &str = "name";
const ACTION_ELEMENT: &str = "Action";
const ACTION_NAME_ATTRIBUTE: &str = "ActionName";
const ALIAS_ATTRIBUTE: &str = "Alias";

/// A Mannequin or CAGE file that cannot be read.
#[derive(Debug, thiserror::Error)]
pub enum MannequinError {
    /// The XML is not well formed.
    #[error("mannequin XML: {0}")]
    Xml(#[from] quick_xml::Error),
    /// An attribute cannot be read.
    #[error("mannequin XML attribute: {0}")]
    Attribute(#[from] quick_xml::events::attributes::AttrError),
    /// The root or a needed element is missing.
    #[error("mannequin XML has no <{0}>")]
    Missing(&'static str),
    /// A needed attribute is missing.
    #[error("mannequin XML <{element}> has no {attribute}")]
    MissingAttribute {
        /// The element.
        element: &'static str,
        /// The attribute.
        attribute: &'static str,
    },
    /// A tag definition needs more bits than a tag state holds.
    #[error("tag definition needs more than {TAG_STATE_LEN} bytes")]
    TooManyTags,
}

/// One element of a small XML file: its name, attributes and child elements in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Node {
    name: String,
    attributes: Vec<(String, String)>,
    children: Vec<Node>,
}

impl Node {
    fn attribute(&self, key: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    fn child(&self, name: &'static str) -> Result<&Self, MannequinError> {
        self.children
            .iter()
            .find(|child| child.name == name)
            .ok_or(MannequinError::Missing(name))
    }

    /// Every element below it with name `name`, in document order.
    fn descendants<'a>(&'a self, name: &'a str, found: &mut Vec<&'a Self>) {
        for child in &self.children {
            if child.name == name {
                found.push(child);
            }
            child.descendants(name, found);
        }
    }
}

fn node_of(start: &BytesStart<'_>) -> Result<Node, MannequinError> {
    let attributes = start
        .attributes()
        .map(|attribute| {
            let attribute = attribute?;
            let key = attribute.key.as_ref().to_owned();
            let value = attribute.normalized_value(XmlVersion::Implicit1_0)?;
            Ok((key, value.into_owned()))
        })
        .collect::<Result<Vec<_>, MannequinError>>()?;
    Ok(Node {
        name: start.name().as_ref().to_owned(),
        attributes,
        children: Vec::new(),
    })
}

/// The root element of `text`, with its element tree (text and comments are left out).
fn parse_tree(text: &str) -> Result<Node, MannequinError> {
    let mut reader = quick_xml::Reader::from_str(text);
    let mut stack: Vec<Node> = vec![Node::default()];
    loop {
        match reader.read_event()? {
            Event::Start(start) => stack.push(node_of(&start)?),
            Event::Empty(start) => {
                let node = node_of(&start)?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                }
            }
            Event::End(_) => {
                let Some(node) = stack.pop() else {
                    break;
                };
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    stack
        .pop()
        .and_then(|document| document.children.into_iter().next())
        .ok_or(MannequinError::Missing("root"))
}

/// A creature's Mannequin controller definitions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ControllerDef {
    /// The tag definition file (`<Tags filename>`), as written (any case).
    pub tags_file: String,
    /// The fragment names, in `<FragmentDefs>` order: fragment `n` is at `n - 1`.
    pub fragments: Vec<String>,
    /// The scope names, in `<ScopeDefs>` order: a scope's bit in `scopeData` is its index.
    pub scopes: Vec<String>,
}

impl ControllerDef {
    /// Reads `*_controllerdefs.xml`.
    ///
    /// # Errors
    ///
    /// Returns [`MannequinError`] when the XML or a needed element is missing.
    pub fn parse(text: &str) -> Result<Self, MannequinError> {
        let root = parse_tree(text)?;
        if root.name != CONTROLLER_DEF {
            return Err(MannequinError::Missing(CONTROLLER_DEF));
        }
        let tags_file = root
            .child(TAGS_ELEMENT)?
            .attribute(FILENAME_ATTRIBUTE)
            .ok_or(MannequinError::MissingAttribute {
                element: TAGS_ELEMENT,
                attribute: FILENAME_ATTRIBUTE,
            })?
            .to_owned();
        let names = |element| -> Result<Vec<String>, MannequinError> {
            Ok(root
                .child(element)?
                .children
                .iter()
                .map(|child| child.name.clone())
                .collect())
        };
        Ok(Self {
            tags_file,
            fragments: names(FRAGMENT_DEFS)?,
            scopes: names(SCOPE_DEFS)?,
        })
    }
}

/// Where one tag's value goes in the tag state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TagBits {
    /// The byte of the tag state.
    pub byte: u8,
    /// The first bit of its group (or of the tag) in that byte.
    pub shift: u8,
    /// The tag's value in its group: its index + 1 (1 for a tag without a group).
    pub value: u8,
}

/// A Mannequin tag definition: the bits of each tag.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagDefinition {
    tags: HashMap<String, TagBits>,
}

impl TagDefinition {
    /// Reads `*tags.xml` and gives each tag its bits as the client does (`FUN_146d88770`, after
    /// the load in `FUN_146e4a9a0`): byte by byte, first every group not yet placed that still
    /// fits in the byte, in file order (a group of `n` tags takes the bits of the number `n`,
    /// its tags the values 1 to `n`), then single tags in the bits left, in file order.
    ///
    /// # Errors
    ///
    /// Returns [`MannequinError`] when the XML or `<Tags>` is missing, or the tags need more
    /// than [`TAG_STATE_LEN`] bytes.
    pub fn parse(text: &str) -> Result<Self, MannequinError> {
        let root = parse_tree(text)?;
        if root.name != TAG_DEFINITION {
            return Err(MannequinError::Missing(TAG_DEFINITION));
        }
        let entries = &root.child(TAGS_ELEMENT)?.children;
        let name_of = |node: &Node| node.attribute(NAME_ATTRIBUTE).map(str::to_owned);
        let groups: Vec<Vec<String>> = entries
            .iter()
            .filter(|node| node.name == GROUP_ELEMENT)
            .map(|group| group.children.iter().filter_map(name_of).collect())
            .collect();
        let singles: Vec<String> = entries
            .iter()
            .filter(|node| node.name == TAG_ELEMENT)
            .filter_map(name_of)
            .collect();
        let mut tags = HashMap::new();
        let mut groups_left: Vec<&Vec<String>> = groups.iter().collect();
        let mut singles_left = singles.iter();
        let mut byte: u8 = 0;
        while !groups_left.is_empty() || singles_left.len() > 0 {
            if usize::from(byte) >= TAG_STATE_LEN {
                return Err(MannequinError::TooManyTags);
            }
            let mut used: u8 = 0;
            groups_left.retain(|group| {
                // A group of `n` tags holds the values 1 to `n`: the bits of `n`.
                let width =
                    u8::try_from(usize::BITS - group.len().leading_zeros()).unwrap_or(u8::MAX);
                if used + width > BITS_PER_BYTE {
                    return true;
                }
                for (index, name) in group.iter().enumerate() {
                    let value = u8::try_from(index + 1).unwrap_or(u8::MAX);
                    tags.insert(
                        name.clone(),
                        TagBits {
                            byte,
                            shift: used,
                            value,
                        },
                    );
                }
                used += width;
                false
            });
            while used < BITS_PER_BYTE {
                let Some(name) = singles_left.next() else {
                    break;
                };
                tags.insert(
                    name.clone(),
                    TagBits {
                        byte,
                        shift: used,
                        value: 1,
                    },
                );
                used += 1;
            }
            byte += 1;
        }
        Ok(Self { tags })
    }

    /// Every tag with its bits.
    pub fn iter(&self) -> impl Iterator<Item = (&String, TagBits)> {
        self.tags.iter().map(|(name, bits)| (name, *bits))
    }

    /// Whether the definition has tag `name`.
    pub fn has(&self, name: &str) -> bool {
        self.tags.contains_key(name)
    }

    /// The tag state that sets `names`; a name the definition does not have sets nothing.
    pub fn state<'a>(&self, names: impl IntoIterator<Item = &'a str>) -> [u8; TAG_STATE_LEN] {
        let mut state = [0; TAG_STATE_LEN];
        for bits in names.into_iter().filter_map(|name| self.tags.get(name)) {
            state[usize::from(bits.byte)] |= bits.value << bits.shift;
        }
        state
    }
}

impl FromIterator<(String, TagBits)> for TagDefinition {
    /// The definition of tags whose bits were given before (a stored table).
    fn from_iter<T: IntoIterator<Item = (String, TagBits)>>(tags: T) -> Self {
        Self {
            tags: tags.into_iter().collect(),
        }
    }
}

/// A creature's CAGE grid: its actions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CageGrid {
    /// The action names, in the client's order: action `n` is at `n - 1`.
    pub actions: Vec<String>,
}

impl CageGrid {
    /// Reads `*.grid`: the `<Action>`s of its action lists in file order. An action without an
    /// `Alias` gets no number (retail: the swarmer's `Equip` has none, and its later actions are
    /// one lower than their place; creature-animation note).
    ///
    /// # Errors
    ///
    /// Returns [`MannequinError`] when the XML is not well formed.
    pub fn parse(text: &str) -> Result<Self, MannequinError> {
        let root = parse_tree(text)?;
        let mut actions = Vec::new();
        root.descendants(ACTION_ELEMENT, &mut actions);
        Ok(Self {
            actions: actions
                .into_iter()
                .filter(|action| action.attribute(ALIAS_ATTRIBUTE).is_some())
                .filter_map(|action| action.attribute(ACTION_NAME_ATTRIBUTE))
                .map(str::to_owned)
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The boar's tag definition (`boar_tags.xml`, build 25810808).
    const BOAR_TAGS: &str = r#"<TagDefinition version="2">
 <Tags>
  <Group name="Variant"><Tag name="Pig"/><Tag name="Boar"/><Tag name="Strange"/><Tag name="Armadillo"/></Group>
  <Group name="WeaponType"><Tag name="CurledBall"/><Tag name="DummyWeapon"/></Group>
  <Tag name="Alerted"/><Tag name="DisableLookPose"/><Tag name="ChainAttack"/><Tag name="BreakoutWindow"/>
  <Tag name="Stunned"/><Tag name="Reaction"/><Tag name="Reset"/><Tag name="End_Loop"/>
  <Tag name="ReactionLimiter_Active"/>
 </Tags>
</TagDefinition>"#;

    /// The risen's tag definition: groups that do not fit move to the next byte, and single tags
    /// fill the bits left (`risen_tags.xml`).
    const RISEN_TAGS: &str = r#"<TagDefinition version="2"><Tags>
  <Group name="Arms"><Tag name="Left_Arm"/><Tag name="Right_Arm"/><Tag name="Both_Arms"/></Group>
  <Group name="Family"><Tag name="Withered"/><Tag name="CorruptionConstruct"/><Tag name="BlightFiend"/><Tag name="Undead"/><Tag name="Invasion"/><Tag name="Inquisitor"/><Tag name="AngryEarth"/><Tag name="Lost"/></Group>
  <Group name="Variant"><Tag name="Base"/><Tag name="Regurgitator"/><Tag name="Impaler"/><Tag name="Bomber"/><Tag name="SwampFiend"/><Tag name="Shivers"/><Tag name="Flamekeeper"/><Tag name="Feculent"/><Tag name="Blackpowder"/><Tag name="SulfurEntity"/><Tag name="FeralGhoul"/></Group>
  <Group name="DifficultyTier"><Tag name="DT1"/><Tag name="DT3"/><Tag name="DT5"/><Tag name="Unstable"/></Group>
  <Group name="RunVariant"><Tag name="Run_v2"/><Tag name="Run_v3"/><Tag name="Run_v4"/><Tag name="Run_v5"/></Group>
  <Tag name="LookPose"/><Tag name="End_Loop"/><Tag name="Alerted"/><Tag name="Reaction"/>
</Tags></TagDefinition>"#;

    fn state_hex(definition: &TagDefinition, names: &[&str]) -> Vec<u8> {
        definition.state(names.iter().copied()).to_vec()
    }

    #[test]
    fn the_boar_tags_are_the_retail_bytes() {
        let tags = TagDefinition::parse(BOAR_TAGS).expect("tags");
        // Retail calm boar `02 00..`, alerted `22 00..`, POI end `02 10`.
        assert_eq!(state_hex(&tags, &["Boar"])[..2], [0x02, 0x00]);
        assert_eq!(state_hex(&tags, &["Boar", "Alerted"])[..2], [0x22, 0x00]);
        assert_eq!(state_hex(&tags, &["Boar", "End_Loop"])[..2], [0x02, 0x10]);
        assert_eq!(
            state_hex(&tags, &["Boar", "NotATag"])[..1],
            [0x02],
            "unknown: nothing"
        );
    }

    #[test]
    fn a_group_that_does_not_fit_moves_to_the_next_byte() {
        let tags = TagDefinition::parse(RISEN_TAGS).expect("tags");
        // Retail risen calm `0b 31`, alerted `0b b1`.
        let calm = ["Both_Arms", "CorruptionConstruct", "Base", "DT5"];
        assert_eq!(state_hex(&tags, &calm)[..2], [0x0b, 0x31]);
        let alert: Vec<&str> = calm.iter().copied().chain(["Alerted"]).collect();
        assert_eq!(state_hex(&tags, &alert)[..2], [0x0b, 0xb1]);
    }

    #[test]
    fn the_controller_numbers_fragments_and_scopes_in_file_order() {
        let text = r#"<ControllerDef>
 <Tags filename="Animations/Mannequin/ADB/NPC_Monster/Boar_Family/Boar_Tags.xml"/>
 <FragmentDefs><Idle scopes="FullBody"><Override tags="A" scopes="FullBody"/></Idle><Move scopes="FullBody"/></FragmentDefs>
 <ScopeDefs><FullBody layer="0"/><Additive layer="3"/><LookPose layer="7"/></ScopeDefs>
</ControllerDef>"#;
        let def = ControllerDef::parse(text).expect("defs");
        assert_eq!(def.fragments, ["Idle", "Move"]);
        assert_eq!(def.scopes, ["FullBody", "Additive", "LookPose"]);
        assert_eq!(
            def.tags_file,
            "Animations/Mannequin/ADB/NPC_Monster/Boar_Family/Boar_Tags.xml"
        );
    }

    #[test]
    fn an_action_without_an_alias_has_no_number() {
        let text = r#"<Grid><ActionLists>
  <ActionList Path="a"><Actions><Action ActionName="LookPose" Alias="LookPose"/><Action ActionName="Equip" IsIgnoredByGrid="true"/></Actions></ActionList>
  <ActionList Path="b"><Actions><Action ActionName="Death" Alias="Death"/></Actions></ActionList>
</ActionLists></Grid>"#;
        assert_eq!(
            CageGrid::parse(text).expect("grid").actions,
            ["LookPose", "Death"]
        );
    }
}
