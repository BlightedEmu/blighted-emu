// SPDX-License-Identifier: AGPL-3.0-or-later
//! The path of a melee attack's hit shapes over their damage windows (build 25810808,
//! 2026-09-18), for `player-attacks`.
//!
//! - **Inferred** (code and retail fit): a `CAGE-Damage` clip of a weapon's attach database
//!   (`PlayerMaleAnims_*Attach.adb`, the weapon's Mannequin context) casts its shape on the weapon:
//!   its `JointName` is a joint of the weapon's skeleton (`master_root`, `Head_0_jnt`, ...; the
//!   clip finds the joint by name on its own character, `FUN_14460cfc0`), and a name the skeleton
//!   does not have, or none, is the weapon's root. The weapon's root sits on the player's
//!   attachment of the weapon's equip type (`equiptypesdatabase.equipdb`: `Greatsword` and
//!   `RSword_LeftHolster` on `righthandattach`, the identity on `right_hand_attach` in
//!   `player_male.cdf`). Over the retail capture of 2026-09-18 (84 melee hits) this frame puts the
//!   greatsword's box on the blade (its tip `vfx_Tip_01` is 1.2 m up the root's z) through the
//!   horizontal swing of `grtswrd_prim_c1`, and a root on the player instead could not reach the
//!   retail targets 2.5 m away. A clip of the player's own database is cast on the player's joint,
//!   or its root.
//! - **Observed** (`FUN_144612ff0`, `FUN_1446050d0`, `FUN_144837220`): the cast moves the shape by
//!   `Offset{X,Y,Z}` in the joint's frame and turns it by `{x,y,z}RotationOffset` degrees
//!   (`FUN_1463347d0`); `UseEndAsCenter` moves it half its length further along its z. The server
//!   code applies the offset only on a joint it found; the retail fit needs it on the weapon root
//!   too (the sword's three hits on targets 1.6 to 2.1 m to the side, 95141 to 97221 ms), so it is
//!   applied on every shape (**Inferred**).
//! - The body: the attack's animation (the player fragment's first shown animation clip) at the
//!   point `StartTime + (t - clip start) x speed` of its clip, each joint's track relative to its
//!   parent ([`Skeleton::pose`]), taken relative to the skeleton's root (`Xform`, which carries the
//!   clip's root motion: the server's body moves with the client's input, so the shape follows the
//!   root rather than the attack's start). A weapon animation of the attach fragment moves the
//!   weapon's joints the same way. Blends between fragments are left out (**Inferred**: the
//!   damage windows start after the blend-ins of the basic attacks, 0.2 to 0.24 s).
//! - Samples: one pose per frame of the clip at its speed, from the window's start to its end.

use std::collections::{BTreeSet, HashMap};

use nw_content::player_attacks::{MeleeShape, ShapeKind};
use nw_formats::caf::Caf;
use nw_formats::character::CharacterDefinition;
use nw_formats::mannequin::{AnimClip, Fragment, ProcClip};
use nw_formats::objectstream::{Element, lowercase_crc32};
use nw_formats::rigid::{Rigid, quat_from_euler_deg};
use nw_formats::skeleton::Skeleton;

/// The player's skeleton.
pub const PLAYER_SKELETON: &str = "objects/characters/player/male/player_male.chr";
/// The player's character definition.
pub const PLAYER_DEFINITION: &str = "objects/characters/player/male/player_male.cdf";
/// The equip types database.
pub const EQUIP_TYPES: &str = "sharedassets/genericassets/items/equiptypesdatabase.equipdb";
/// The folders of the clips: the player's animations (`player_shared.chrparams`) and the props'.
pub const CLIP_FOLDERS: [&str; 2] = [
    "animations/gameplay/character/player/",
    "animations/gameplay/prop/",
];
/// The suffix of a clip file.
pub const CLIP_SUFFIX: &str = ".caf";
/// The suffix of a character definition.
pub const DEFINITION_SUFFIX: &str = ".cdf";

/// The equip types database's fields: ObjectStream names are the lower-case CRC-32 of the field's
/// display name (Observed: the names in the client's strings give every CRC of the file).
const EQUIP_TYPES_FIELD: &str = "Equip Types";
const ELEMENT_FIELD: &str = "Element";
const NAME_FIELD: &str = "Name";
const ATTACHMENT_FIELD: &str = "Attachment";

/// `CAGE-Damage` parameters.
const SHAPE_TYPE: &str = "MeleeAttackShapeCastType";
const SHAPE_SPHERE: &str = "MeleeAttackShapeCastTypeSphere";
const SHAPE_CAPSULE: &str = "MeleeAttackShapeCastTypeCapsule";
const SHAPE_BOX: &str = "MeleeAttackShapeCastTypeBox";
const SHAPE_CYLINDER: &str = "MeleeAttackShapeCastTypeCylinder";
const SHAPE_NONE: &str = "MeleeAttackShapeCastTypeNone";
const RADIUS: &str = "MeleeAttackShapeRadius";
const HALF_HEIGHT: &str = "MeleeAttackCapsuleHalfHeight";
const BOX_DIMENSIONS: [&str; 3] = [
    "MeleeAttackBoxDimensionsX",
    "MeleeAttackBoxDimensionsY",
    "MeleeAttackBoxDimensionsZ",
];
const OFFSETS: [&str; 3] = ["OffsetX", "OffsetY", "OffsetZ"];
const ROTATIONS: [&str; 3] = ["xRotationOffset", "yRotationOffset", "zRotationOffset"];
const JOINT_NAME: &str = "JointName";
const END_AS_CENTER: &str = "UseEndAsCenter";
const TRUE_TEXT: &str = "true";
/// Half, for half extents and half lengths.
const HALF: f32 = 0.5;
/// Milliseconds per second.
pub const MS_PER_SECOND: f64 = 1_000.0;

/// Where a damage clip plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipContext {
    /// In the weapon's attach database: cast on the weapon.
    Weapon,
    /// In the player's database: cast on the player.
    Character,
}

/// The skeletons, attachments and clips the shapes need.
#[derive(Debug, Clone, Default)]
pub struct Bodies {
    /// The player's skeleton.
    pub player: Skeleton,
    /// The player's character definition.
    pub player_definition: CharacterDefinition,
    /// The weapons' character definitions, by lower-case path.
    pub weapon_definitions: HashMap<String, CharacterDefinition>,
    /// Skeletons by lower-case path.
    pub skeletons: HashMap<String, Skeleton>,
    /// Clips by lower-case animation name; `None` for a name two files share.
    pub clips: HashMap<String, Option<Caf>>,
    /// The main-hand attachment of each equip type, by lower-case equip type.
    pub equip_attachments: HashMap<String, String>,
}

/// The weapon a stance's attacks are cast on: its attachment and its skeletons.
#[derive(Debug, Clone)]
pub struct WeaponBody<'a> {
    /// The player's attachment the weapon sits on.
    pub attachment: &'a str,
    /// The skeletons of the stance's weapons.
    pub skeletons: Vec<&'a Skeleton>,
}

/// The main-hand attachment of each equip type of the equip types database `root`.
pub fn equip_attachments(root: &Element) -> HashMap<String, String> {
    let crc = lowercase_crc32;
    let string = |element: &Element, field: &str| {
        element
            .child(crc(field))
            .and_then(|value| value.as_string().ok())
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    root.child(crc(EQUIP_TYPES_FIELD))
        .map(|types| {
            types
                .children_named(crc(ELEMENT_FIELD))
                .filter_map(|element| {
                    Some((
                        string(element, NAME_FIELD)?.to_lowercase(),
                        string(element, ATTACHMENT_FIELD)?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

impl Bodies {
    /// The clip of animation `name`.
    ///
    /// # Errors
    ///
    /// Returns why when no file or two files have the name.
    pub fn clip(&self, name: &str) -> Result<&Caf, String> {
        match self.clips.get(&name.to_lowercase()) {
            Some(Some(clip)) => Ok(clip),
            Some(None) => Err(format!("animation {name}: two clip files")),
            None => Err(format!("animation {name}: no clip file")),
        }
    }

    /// The skeletons of the weapon definitions `definitions` (lower-case paths).
    ///
    /// # Errors
    ///
    /// Returns why when a definition or its skeleton is missing.
    pub fn weapon_skeletons<'a>(
        &'a self,
        definitions: &BTreeSet<String>,
    ) -> Result<Vec<&'a Skeleton>, String> {
        definitions
            .iter()
            .map(|path| {
                let definition = self
                    .weapon_definitions
                    .get(path)
                    .ok_or_else(|| format!("weapon definition {path}: not read"))?;
                let model = normalized(&definition.model);
                self.skeletons
                    .get(&model)
                    .ok_or_else(|| format!("weapon skeleton {model}: not read"))
            })
            .collect()
    }

    /// The fragment's length: its longest animation layer, the last clip of each played to its
    /// animation's end (a loop to its next clip only); `None` when a clip's file is missing.
    pub fn fragment_length_s(&self, fragment: &Fragment) -> Option<f64> {
        let mut length: f64 = 0.0;
        for clip in fragment.anim_layers.iter().flatten() {
            let end = match clip.end_s {
                Some(end) => end,
                None if clip.looped => clip.start_s,
                None => {
                    let duration = f64::from(self.clip(&clip.name).ok()?.duration_s());
                    clip.start_s + (duration - clip.anim_start_s).max(0.0) / clip.speed
                }
            };
            length = length.max(end);
        }
        Some(length)
    }
}

/// A path as the paks name it: lower case, forward slashes.
pub fn normalized(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

fn number(clip: &ProcClip, key: &str) -> Result<f32, String> {
    clip.param(key)
        .and_then(|text| text.trim().parse().ok())
        .ok_or_else(|| format!("damage clip without {key}"))
}

fn numbers(clip: &ProcClip, keys: [&str; 3]) -> Result<[f32; 3], String> {
    Ok([
        number(clip, keys[0])?,
        number(clip, keys[1])?,
        number(clip, keys[2])?,
    ])
}

/// The shape of damage clip `clip`; `None` for `MeleeAttackShapeCastTypeNone`.
///
/// # Errors
///
/// Returns why when its type or a size is missing.
pub fn shape_of(clip: &ProcClip) -> Result<Option<ShapeKind>, String> {
    let shape = match clip.param(SHAPE_TYPE).unwrap_or_default() {
        SHAPE_SPHERE => ShapeKind::Sphere {
            radius_m: number(clip, RADIUS)?,
        },
        SHAPE_CAPSULE => ShapeKind::Capsule {
            radius_m: number(clip, RADIUS)?,
            half_height_m: number(clip, HALF_HEIGHT)?,
        },
        SHAPE_BOX => ShapeKind::Box {
            half_extents_m: numbers(clip, BOX_DIMENSIONS)?.map(|size| size * HALF),
        },
        SHAPE_CYLINDER => ShapeKind::Cylinder {
            radius_m: number(clip, RADIUS)?,
            half_height_m: number(clip, HALF_HEIGHT)?,
        },
        SHAPE_NONE => return Ok(None),
        other => return Err(format!("damage clip of shape type {other:?}")),
    };
    Ok(Some(shape))
}

/// Half the length of `shape` along its z.
fn half_length(shape: ShapeKind) -> f32 {
    match shape {
        ShapeKind::Sphere { .. } => 0.0,
        ShapeKind::Capsule { half_height_m, .. } | ShapeKind::Cylinder { half_height_m, .. } => {
            half_height_m
        }
        ShapeKind::Box { half_extents_m } => half_extents_m[2],
    }
}

/// The frame of `clip` (an animation clip of a fragment) `fragment_s` after the fragment starts.
fn frame_of(clip: &AnimClip, caf: &Caf, fragment_s: f64) -> f32 {
    let duration = f64::from(caf.duration_s());
    let mut at = clip.anim_start_s + (fragment_s - clip.start_s).max(0.0) * clip.speed;
    if clip.looped && duration > 0.0 {
        at = at.rem_euclid(duration);
    }
    // A clip time of a few seconds fits an f32.
    #[allow(clippy::cast_possible_truncation)]
    let at = at.clamp(0.0, duration) as f32;
    caf.frame_at(at)
}

/// The joint `name` of the weapon skeletons `skeletons`: its index in the first; `None` when no
/// skeleton has it (the weapon's root).
///
/// # Errors
///
/// Returns why when only some skeletons have it or their bind poses differ.
fn weapon_joint(skeletons: &[&Skeleton], name: &str) -> Result<Option<usize>, String> {
    if name.is_empty() {
        return Ok(None);
    }
    let found: Vec<Option<Rigid>> = skeletons
        .iter()
        .map(|skeleton| {
            skeleton
                .joint(name)
                .map(|joint| skeleton.joints[joint].bind_model)
        })
        .collect();
    if found.iter().all(Option::is_none) {
        return Ok(None);
    }
    let first = found
        .first()
        .copied()
        .flatten()
        .ok_or_else(|| format!("joint {name}: not in every weapon skeleton"))?;
    if found.iter().any(|pose| *pose != Some(first)) {
        return Err(format!(
            "joint {name}: not the same in every weapon skeleton"
        ));
    }
    Ok(skeletons.first().and_then(|skeleton| skeleton.joint(name)))
}

/// A root path: per step, the fragment time, s, and the root's ground-plane offset, m.
pub type RootPath = Vec<(f64, [f32; 2])>;

/// The root path of an attack whose body plays `body_clip` over a fragment of `length_s`: the
/// seconds between two steps (one frame of the clip at its speed) and, per step from the
/// fragment's start to its end, the fragment time and the ground-plane offset of the skeleton's
/// root (`Xform`, which carries the clip's root motion) from where it stood at the start, in the
/// character's space (x right, y forward).
///
/// # Errors
///
/// Returns why when the clip file is missing or has no frame time.
pub fn root_path(
    bodies: &Bodies,
    body_clip: &AnimClip,
    length_s: f64,
) -> Result<(f64, RootPath), String> {
    let body = bodies.clip(&body_clip.name)?;
    let step_s = f64::from(body.frame_seconds()) / body_clip.speed;
    if step_s <= 0.0 {
        return Err(format!("animation {}: no frame time", body_clip.name));
    }
    let root_at = |time_s: f64| {
        bodies
            .player
            .pose(body, frame_of(body_clip, body, time_s))
            .first()
            .copied()
            .unwrap_or_default()
            .translation
    };
    let start = root_at(0.0);
    let mut path = Vec::new();
    let mut time_s = 0.0;
    loop {
        let at = root_at(time_s);
        path.push((time_s, [at[0] - start[0], at[1] - start[1]]));
        if time_s >= length_s {
            break;
        }
        time_s += step_s;
    }
    Ok((step_s, path))
}

/// Seconds as whole milliseconds.
pub fn ms(seconds: f64) -> u64 {
    // A clip time is a few seconds.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a clip time of a few seconds"
    )]
    let ms = (seconds * MS_PER_SECOND).round().max(0.0) as u64;
    ms
}

/// The shape path of damage clip `clip` playing in `context` of an attack whose body plays
/// `body_clip` and whose weapon plays `weapon_clip`, on `weapon`; `None` for a clip without a
/// shape.
///
/// # Errors
///
/// Returns why when a clip file, the attachment, a joint or a parameter is missing.
pub fn melee_shape(
    bodies: &Bodies,
    weapon: Option<&WeaponBody<'_>>,
    body_clip: &AnimClip,
    weapon_clip: Option<&AnimClip>,
    clip: &ProcClip,
    context: ClipContext,
) -> Result<Option<MeleeShape>, String> {
    let Some(shape) = shape_of(clip)? else {
        return Ok(None);
    };
    let end_s = clip
        .end_s
        .ok_or_else(|| "damage clip lasts to the fragment's end".to_owned())?;
    let body = bodies.clip(&body_clip.name)?;
    let joint_name = clip.param(JOINT_NAME).unwrap_or_default();
    let local = Rigid::translation(numbers(clip, OFFSETS)?)
        .then_local(&Rigid::rotation(quat_from_euler_deg(numbers(
            clip, ROTATIONS,
        )?)))
        .then_local(&Rigid::translation(
            if clip.param(END_AS_CENTER) == Some(TRUE_TEXT) {
                [0.0, 0.0, half_length(shape)]
            } else {
                [0.0; 3]
            },
        ));
    // A joint of the weapon's skeleton, and the weapon's animation when it plays one.
    struct WeaponJoint<'a> {
        skeleton: &'a Skeleton,
        joint: usize,
        animation: Option<(&'a AnimClip, &'a Caf)>,
    }
    // What the shape hangs on, relative to the player's root, at fragment time `t`.
    enum Hang<'a> {
        Weapon {
            bone: usize,
            attachment: Rigid,
            joint: Option<WeaponJoint<'a>>,
        },
        Character(Option<usize>),
    }
    let hang = match context {
        ClipContext::Weapon => {
            let weapon =
                weapon.ok_or_else(|| "weapon clip of a stance without a weapon".to_owned())?;
            let attachment = bodies
                .player_definition
                .bone_attachment(weapon.attachment)
                .ok_or_else(|| format!("attachment {}: not in the player", weapon.attachment))?;
            let bone_name = attachment.bone.as_deref().unwrap_or_default();
            let bone = bodies
                .player
                .joint(bone_name)
                .ok_or_else(|| format!("attachment bone {bone_name}: not in the player"))?;
            let joint = match weapon_joint(&weapon.skeletons, joint_name)? {
                Some(joint) => Some(WeaponJoint {
                    skeleton: weapon
                        .skeletons
                        .first()
                        .copied()
                        .ok_or_else(|| "no weapon skeleton".to_owned())?,
                    joint,
                    animation: weapon_clip
                        .map(|clip| Ok::<_, String>((clip, bodies.clip(&clip.name)?)))
                        .transpose()?,
                }),
                None => None,
            };
            Hang::Weapon {
                bone,
                attachment: attachment.relative,
                joint,
            }
        }
        ClipContext::Character => Hang::Character(
            bodies
                .player
                .joint(joint_name)
                .filter(|_| !joint_name.is_empty()),
        ),
    };
    let step_s = f64::from(body.frame_seconds()) / body_clip.speed;
    if step_s <= 0.0 {
        return Err(format!("animation {}: no frame time", body_clip.name));
    }
    let mut poses = Vec::new();
    let mut time_s = clip.start_s;
    loop {
        let pose = bodies.player.pose(body, frame_of(body_clip, body, time_s));
        let root = pose.first().copied().unwrap_or_default().inverse();
        let hung = match &hang {
            Hang::Weapon {
                bone,
                attachment,
                joint,
            } => {
                let on = pose[*bone].then_local(attachment);
                match joint {
                    Some(WeaponJoint {
                        skeleton,
                        joint,
                        animation: Some((clip, caf)),
                    }) => on.then_local(&skeleton.pose(caf, frame_of(clip, caf, time_s))[*joint]),
                    Some(WeaponJoint {
                        skeleton,
                        joint,
                        animation: None,
                    }) => on.then_local(&skeleton.joints[*joint].bind_model),
                    None => on,
                }
            }
            Hang::Character(Some(joint)) => pose[*joint],
            Hang::Character(None) => pose.first().copied().unwrap_or_default(),
        };
        poses.push(root.then_local(&hung).then_local(&local));
        if time_s >= end_s {
            break;
        }
        time_s += step_s;
    }
    // A frame time of a clip at its speed is a few tens of milliseconds.
    #[allow(clippy::cast_possible_truncation)]
    let step_ms = (step_s * MS_PER_SECOND) as f32;
    Ok(Some(MeleeShape {
        window_ms: [ms(clip.start_s), ms(end_s)],
        shape,
        step_ms,
        poses,
    }))
}

/// Bodies for tests: a player and a blade.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use nw_formats::caf::Track;
    use nw_formats::rigid::{IDENTITY_QUAT, quat_about_z};
    use nw_formats::skeleton::Joint;

    /// The equip type of the test sword (`RSword_LeftHolster`, lower case).
    pub(crate) const SWORD_EQUIP_TYPE: &str = "rsword_leftholster";
    /// The test sword's character definition and skeleton.
    pub(crate) const SWORD_DEFINITION: &str = "objects/weapons/sword.cdf";
    pub(crate) const SWORD_SKELETON: &str = "objects/weapons/sword_skel.chr";
    /// The test clip: 30 frames of 1/30 s.
    pub(crate) const SWING: &str = "swing";

    fn joint(name: &str, parent: Option<usize>, local: Rigid) -> Joint {
        Joint {
            name: name.to_owned(),
            controller_id: lowercase_crc32(name),
            parent,
            bind_model: local,
            bind_local: local,
        }
    }

    /// A player of a root and a hand 1 m up and 0.5 m to the right, whose clip `swing` turns the
    /// root a quarter turn (to the west) and moves it 1 m forward over 30 frames; a blade of one
    /// joint, `master_root`, on `righthandattach`.
    pub(crate) fn bodies() -> Bodies {
        let player = Skeleton {
            joints: vec![
                joint("Xform", None, Rigid::IDENTITY),
                joint(
                    "right_hand_attach",
                    Some(0),
                    Rigid::translation([0.5, 0.0, 1.0]),
                ),
            ],
        };
        let mut clip = Caf {
            ticks_per_frame: 1,
            seconds_per_tick: 1.0 / 30.0,
            start_frame: 0,
            end_frame: 30,
            ..Caf::default()
        };
        clip.tracks.insert(
            lowercase_crc32("Xform"),
            Track {
                rotations: vec![
                    (0.0, IDENTITY_QUAT),
                    (30.0, quat_about_z(std::f32::consts::FRAC_PI_2)),
                ],
                positions: vec![(0.0, [0.0; 3]), (30.0, [0.0, 1.0, 0.0])],
            },
        );
        let blade = Skeleton {
            joints: vec![joint("master_root", None, Rigid::IDENTITY)],
        };
        let player_definition = CharacterDefinition::parse(
            r#"<CharacterDefinition><Model File="p.chr"/><AttachmentList>
            <Attachment Type="CA_BONE" AName="righthandattach" RelRotation="1,0,0,0" RelPosition="0,0,0" BoneName="right_hand_attach"/>
            </AttachmentList></CharacterDefinition>"#,
        )
        .expect("cdf");
        let sword = CharacterDefinition {
            model: SWORD_SKELETON.to_owned(),
            attachments: Vec::new(),
        };
        Bodies {
            player,
            player_definition,
            weapon_definitions: HashMap::from([(SWORD_DEFINITION.to_owned(), sword)]),
            skeletons: HashMap::from([(SWORD_SKELETON.to_owned(), blade)]),
            clips: HashMap::from([(SWING.to_owned(), Some(clip))]),
            equip_attachments: HashMap::from([(
                SWORD_EQUIP_TYPE.to_owned(),
                "righthandattach".to_owned(),
            )]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{SWORD_SKELETON, bodies};
    use super::*;
    use nw_formats::objectstream::write_object_stream;
    use nw_formats::rigid::{length, rotate, sub};

    fn damage(params: &[(&str, &str)], start_s: f64, end_s: f64) -> ProcClip {
        ProcClip {
            kind: "CAGE-Damage".to_owned(),
            start_s,
            end_s: Some(end_s),
            params: params
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            layer: 0,
        }
    }

    fn swing() -> AnimClip {
        AnimClip {
            name: "swing".to_owned(),
            start_s: 0.0,
            anim_start_s: 0.0,
            blend_s: 0.0,
            speed: 1.0,
            looped: false,
            weight: 1.0,
            end_s: None,
        }
    }

    const CAPSULE: [(&str, &str); 10] = [
        (
            "MeleeAttackShapeCastType",
            "MeleeAttackShapeCastTypeCapsule",
        ),
        ("MeleeAttackShapeRadius", "0.3"),
        ("MeleeAttackCapsuleHalfHeight", "0.4"),
        ("OffsetX", "0"),
        ("OffsetY", "0"),
        ("OffsetZ", "0.5"),
        ("xRotationOffset", "0"),
        ("yRotationOffset", "0"),
        ("zRotationOffset", "90"),
        ("JointName", "master_root"),
    ];

    #[test]
    fn a_weapon_shape_hangs_on_the_hand_relative_to_the_moving_root() {
        let bodies = bodies();
        let weapon = WeaponBody {
            attachment: "righthandattach",
            skeletons: vec![&bodies.skeletons[SWORD_SKELETON]],
        };
        let clip = damage(&CAPSULE, 0.0, 1.0);
        let shape = melee_shape(
            &bodies,
            Some(&weapon),
            &swing(),
            None,
            &clip,
            ClipContext::Weapon,
        )
        .expect("shape")
        .expect("a capsule");
        assert_eq!(shape.window_ms, [0, 1_000]);
        assert_eq!(shape.poses.len(), 31, "a pose per frame, both ends");
        assert!((shape.step_ms - 1_000.0 / 30.0).abs() < 1e-3);
        // Relative to the root the hand never moves: root motion is the body's.
        for pose in &shape.poses {
            assert!(
                length(sub(pose.translation, [0.5, 0.0, 1.5])) < 1e-4,
                "{pose:?}"
            );
            assert!(length(sub(rotate(pose.rotation, [1.0, 0.0, 0.0]), [0.0, 1.0, 0.0])) < 1e-4);
        }
        assert_eq!(
            shape.shape,
            ShapeKind::Capsule {
                radius_m: 0.3,
                half_height_m: 0.4
            }
        );
    }

    #[test]
    fn the_end_as_centre_moves_the_shape_half_its_length_and_none_has_no_shape() {
        let bodies = bodies();
        let mut params = CAPSULE.to_vec();
        params.push(("UseEndAsCenter", "true"));
        params[8] = ("zRotationOffset", "0");
        let clip = damage(&params, 0.0, 0.1);
        let shape = melee_shape(&bodies, None, &swing(), None, &clip, ClipContext::Character)
            .expect("shape")
            .expect("a capsule");
        // On the player's root (the player has no `master_root`): 0.5 offset + 0.4 half height.
        assert!(length(sub(shape.poses[0].translation, [0.0, 0.0, 0.9])) < 1e-4);
        let mut none = CAPSULE.to_vec();
        none[0] = ("MeleeAttackShapeCastType", "MeleeAttackShapeCastTypeNone");
        assert_eq!(
            melee_shape(
                &bodies,
                None,
                &swing(),
                None,
                &damage(&none, 0.0, 0.1),
                ClipContext::Character
            ),
            Ok(None)
        );
        assert!(
            melee_shape(&bodies, None, &swing(), None, &clip, ClipContext::Weapon).is_err(),
            "a weapon clip needs a weapon"
        );
    }

    #[test]
    fn a_box_has_half_its_dimensions_and_a_fragment_its_length() {
        let clip = damage(
            &[
                ("MeleeAttackShapeCastType", "MeleeAttackShapeCastTypeBox"),
                ("MeleeAttackBoxDimensionsX", "1.2"),
                ("MeleeAttackBoxDimensionsY", "0.5"),
                ("MeleeAttackBoxDimensionsZ", "2"),
            ],
            0.0,
            0.1,
        );
        assert_eq!(
            shape_of(&clip),
            Ok(Some(ShapeKind::Box {
                half_extents_m: [0.6, 0.25, 1.0]
            }))
        );
        let bodies = bodies();
        let fragment = Fragment {
            anim_layers: vec![vec![AnimClip {
                anim_start_s: 0.25,
                speed: 1.25,
                ..swing()
            }]],
            ..Fragment::default()
        };
        // (1 s - 0.25 s) / 1.25.
        let length = bodies.fragment_length_s(&fragment).expect("length");
        assert!((length - 0.6).abs() < 1e-6);
        assert_eq!(
            bodies.clip("Missing").err().as_deref(),
            Some("animation Missing: no clip file")
        );
    }

    #[test]
    fn the_equip_types_name_their_attachment() {
        let crc = lowercase_crc32;
        let text =
            |name: &str, value: &str| Element::with_value(crc(name), value.as_bytes().to_vec());
        let element = |name: &str, attachment: &str| {
            Element::with_children(
                crc(ELEMENT_FIELD),
                vec![text(NAME_FIELD, name), text(ATTACHMENT_FIELD, attachment)],
            )
        };
        let root = Element::with_children(
            0,
            vec![Element::with_children(
                crc(EQUIP_TYPES_FIELD),
                vec![
                    element("Greatsword", "righthandattach"),
                    element("Shield", ""),
                ],
            )],
        );
        let bytes = write_object_stream(&root).expect("stream");
        let found = equip_attachments(
            &nw_formats::objectstream::parse_object_stream(&bytes).expect("root"),
        );
        assert_eq!(
            found.get("greatsword").map(String::as_str),
            Some("righthandattach")
        );
        assert_eq!(found.get("shield"), None, "no attachment");
        assert_eq!(
            normalized("Objects\\Weapons\\A.cdf"),
            "objects/weapons/a.cdf"
        );
        assert_eq!(ms(0.4333), 433);
    }
}
