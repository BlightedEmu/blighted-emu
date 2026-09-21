// SPDX-License-Identifier: MIT OR Apache-2.0
//! A character definition (`.cdf`, XML; build 25810808, 2026-09-18): its skeleton (`<Model File>`)
//! and its attachments. A bone attachment (`Type="CA_BONE"`) sits on the joint `BoneName` at
//! `RelPosition` and `RelRotation` relative to it; `RelRotation` is written `w,x,y,z` (Observed:
//! the identity is `1,0,0,0` on every attachment without a turn of `player_male.cdf`).

use crate::mannequin::{MannequinError, parse_tree};
use crate::rigid::{Rigid, quat_normalize};

const ROOT_ELEMENT: &str = "CharacterDefinition";
const MODEL_ELEMENT: &str = "Model";
const FILE_ATTRIBUTE: &str = "File";
const ATTACHMENT_LIST: &str = "AttachmentList";
const NAME_ATTRIBUTE: &str = "AName";
const TYPE_ATTRIBUTE: &str = "Type";
const BONE_ATTRIBUTE: &str = "BoneName";
const POSITION_ATTRIBUTE: &str = "RelPosition";
const ROTATION_ATTRIBUTE: &str = "RelRotation";
/// The type of an attachment on a joint.
pub const BONE_ATTACHMENT: &str = "CA_BONE";
const SEPARATOR: char = ',';
const VEC3_PARTS: usize = 3;
const QUAT_PARTS: usize = 4;

/// One attachment.
#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    /// `AName`.
    pub name: String,
    /// `Type` (`CA_BONE`, `CA_SKIN`, ...).
    pub kind: String,
    /// `BoneName`, for a bone attachment.
    pub bone: Option<String>,
    /// Its place relative to the joint (`RelPosition`, `RelRotation`); the identity when they are
    /// not written.
    pub relative: Rigid,
}

/// A character definition.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CharacterDefinition {
    /// The skeleton file (`<Model File>`), as written.
    pub model: String,
    /// The attachments, in file order.
    pub attachments: Vec<Attachment>,
}

fn numbers<const N: usize>(text: Option<&str>) -> Option<[f32; N]> {
    let parts: Vec<f32> = text?
        .split(SEPARATOR)
        .map(|part| part.trim().parse().ok())
        .collect::<Option<_>>()?;
    parts.try_into().ok()
}

impl CharacterDefinition {
    /// Reads a `.cdf` file.
    ///
    /// # Errors
    ///
    /// Returns [`MannequinError`] when the XML or its `<CharacterDefinition>` or `<Model>` is
    /// missing.
    pub fn parse(text: &str) -> Result<Self, MannequinError> {
        let root = parse_tree(text)?;
        if root.name != ROOT_ELEMENT {
            return Err(MannequinError::Missing(ROOT_ELEMENT));
        }
        let model = root
            .child(MODEL_ELEMENT)?
            .attribute(FILE_ATTRIBUTE)
            .ok_or(MannequinError::MissingAttribute {
                element: MODEL_ELEMENT,
                attribute: FILE_ATTRIBUTE,
            })?
            .to_owned();
        let attachments = root
            .children
            .iter()
            .filter(|child| child.name == ATTACHMENT_LIST)
            .flat_map(|list| &list.children)
            .filter_map(|node| {
                let position = numbers::<VEC3_PARTS>(node.attribute(POSITION_ATTRIBUTE));
                let rotation = numbers::<QUAT_PARTS>(node.attribute(ROTATION_ATTRIBUTE))
                    .map(|[w, x, y, z]| quat_normalize([x, y, z, w]));
                Some(Attachment {
                    name: node.attribute(NAME_ATTRIBUTE)?.to_owned(),
                    kind: node
                        .attribute(TYPE_ATTRIBUTE)
                        .unwrap_or_default()
                        .to_owned(),
                    bone: node.attribute(BONE_ATTRIBUTE).map(str::to_owned),
                    relative: Rigid {
                        rotation: rotation.unwrap_or(Rigid::IDENTITY.rotation),
                        translation: position.unwrap_or(Rigid::IDENTITY.translation),
                    },
                })
            })
            .collect();
        Ok(Self { model, attachments })
    }

    /// The bone attachment named `name` (any case).
    pub fn bone_attachment(&self, name: &str) -> Option<&Attachment> {
        self.attachments.iter().find(|attachment| {
            attachment.kind == BONE_ATTACHMENT && attachment.name.eq_ignore_ascii_case(name)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rigid::{length, rotate, sub};

    /// A cut of `objects/characters/player/male/player_male.cdf` (build 25810808).
    const PLAYER: &str = r#"<CharacterDefinition>
 <Model File="objects/characters/player/male/player_male.chr" Material="x"/>
 <AttachmentList>
  <Attachment Type="CA_BONE" AName="righthandattach" SerialNumber="67DB5AA66C7B" RelRotation="1,0,0,0" RelPosition="0,0,0" BoneName="right_hand_attach" Flags="0"/>
  <Attachment Type="CA_BONE" AName="GreatAxeHandAttach" SerialNumber="67DB5AA66C93" RelRotation="0.03567167,0.91022986,0.39269131,0.12650189" RelPosition="-0.064320326,-0.043956585,0.10634214" BoneName="right_hand_attach" Flags="0"/>
  <Attachment Type="CA_SKIN" AName="hair" SerialNumber="67DB5AA66C7E" Flags="0"/>
 </AttachmentList>
</CharacterDefinition>"#;

    #[test]
    fn a_definition_has_its_model_and_bone_attachments() {
        let definition = CharacterDefinition::parse(PLAYER).expect("cdf");
        assert_eq!(
            definition.model,
            "objects/characters/player/male/player_male.chr"
        );
        assert_eq!(definition.attachments.len(), 3);
        let hand = definition.bone_attachment("RightHandAttach").expect("hand");
        assert_eq!(hand.bone.as_deref(), Some("right_hand_attach"));
        assert_eq!(hand.relative, Rigid::IDENTITY);
        assert_eq!(definition.bone_attachment("hair"), None, "a skin");
        let axe = definition
            .bone_attachment("GreatAxeHandAttach")
            .expect("axe");
        assert!(
            length(sub(
                axe.relative.translation,
                [-0.064_320_33, -0.043_956_585, 0.106_342_14]
            )) < 1e-6
        );
        // w first in the file: a near half turn about (0.91, 0.39, 0.13).
        let turned = rotate(
            axe.relative.rotation,
            [0.910_229_9, 0.392_691_3, 0.126_501_9],
        );
        assert!(length(sub(turned, [0.910_229_9, 0.392_691_3, 0.126_501_9])) < 1e-4);
    }

    #[test]
    fn a_file_without_a_model_is_an_error() {
        let text = "<CharacterDefinition><AttachmentList/></CharacterDefinition>";
        assert!(matches!(
            CharacterDefinition::parse(text),
            Err(MannequinError::Missing(MODEL_ELEMENT))
        ));
        assert!(matches!(
            CharacterDefinition::parse("<Other/>"),
            Err(MannequinError::Missing(ROOT_ELEMENT))
        ));
    }
}
