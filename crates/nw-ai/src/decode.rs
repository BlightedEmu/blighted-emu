// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reads `.behaviortree` ObjectStreams (behavior-tree study, sections 1.1 to 1.3) into the shared
//! node arena of [`crate::data::AiData`].
//!
//! Layout (Observed): the root element is a `Tree` (`Name`, `Root`). A node is an
//! `intrusive_ptr<Node>` that holds one element, the node class; the class chains its bases
//! through `BaseClass1` (`Node` has `Id`, `Name`, `BaseUtility`, `Interruptable`; a `Decorator`
//! adds `Child`, a `Composite` `Children`). A `TreeReference` holds a full copy of the referenced
//! `Tree` in `SerializedTree`. A field's name CRC is the lower-case CRC32 of its name.
//!
//! Identical sub-trees are stored once: a node is added only when no equal node (same class,
//! fields and children, node `Id` left out) exists yet. The node `Id`s are not kept: copies of
//! one sub-tree differ only there (study section 1.2).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use nw_formats::objectstream::{Element, ObjectStreamError, lowercase_crc32, parse_object_stream};
use nw_formats::pak::{Decompressor, PakArchive, PakError};

use crate::classes::{Class, class_of};
use crate::data::TreeAsset;
use crate::node::{
    ActionTarget, AssetRef, CageParams, Children, CooldownOverride, CooldownSpec,
    EntityQueryParams, FactOp, Gates, Motion, NavRayCast, NavigationParams, Node, NodeId, NodeKind,
    PositionType, ProjectileParams, SelectPositionParams, SelectionMethod, StrafeParams,
    TagOperation, TargetLimits, TrackerParams,
};
use crate::value::{CompareOp, Condition, ConditionGroup, KeyId, NameId, Param, Value};

/// Name of the base class element of a class.
const BASE_CLASS: &str = "BaseClass1";
/// `AZ::Data::Asset` value: GUID (16), sub id and padding (16), asset type (16), hint length
/// (`u64`), hint (Observed, study section 1.2).
const ASSET_GUID_LEN: usize = 16;
/// Where the hint length starts in an asset reference value.
pub const ASSET_HINT_LEN_AT: usize = 48;
const ASSET_HINT_AT: usize = ASSET_HINT_LEN_AT + size_of::<u64>();
/// `GroupLogicalOperator` 1 is Or (Inferred, study section 1.4).
const GROUP_OR: i64 = 1;
/// `SelectionMethod` values (Observed in `FUN_1426355b0`).
const SELECTION_TOP: i64 = 0;
const SELECTION_WEIGHTED: i64 = 1;

/// A tree that cannot be read.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DecodeError {
    /// An element has a type UUID none of the 77 classes has.
    #[error("unknown class {0:02x?}")]
    UnknownClass([u8; 16]),
    /// An element has another class than its place needs.
    #[error("{field}: class {found:?} where {expected} was expected")]
    WrongClass {
        /// The field.
        field: &'static str,
        /// The class found.
        found: Class,
        /// What the place needs.
        expected: &'static str,
    },
    /// A needed field is missing.
    #[error("{class:?} has no field {field}")]
    MissingField {
        /// The class.
        class: Class,
        /// The field.
        field: &'static str,
    },
    /// A field the reader does not know.
    #[error("{class:?} has an unread field with name CRC {crc:08x}")]
    UnreadField {
        /// The class.
        class: Class,
        /// Its name CRC.
        crc: u32,
    },
    /// A value has another size than its type.
    #[error("{field}: {class:?} value has {size} bytes")]
    BadValue {
        /// The field.
        field: &'static str,
        /// Its class.
        class: Class,
        /// Its size.
        size: usize,
    },
    /// A stored enum number is out of range.
    #[error("{field}: {value} is not a known value")]
    BadEnum {
        /// The field.
        field: &'static str,
        /// The number.
        value: i64,
    },
    /// A `BlackboardKey` is not the lower-case CRC32 of its `BlackboardKeyName`.
    #[error("blackboard key {name:?} has CRC {stored:#x}, expected {expected:#x}")]
    KeyCrc {
        /// The key name.
        name: String,
        /// The stored CRC.
        stored: u64,
        /// The CRC of the name.
        expected: u64,
    },
    /// A count does not fit the arena.
    #[error("the node arena is full")]
    Full,
}

/// File extension of a behavior tree in Server.pak.
pub const TREE_EXTENSION: &str = ".behaviortree";

/// A Server.pak that cannot be read.
#[derive(Debug, thiserror::Error)]
pub enum ServerPakError {
    /// The pak cannot be read.
    #[error(transparent)]
    Pak(#[from] PakError),
    /// An entry is not an ObjectStream.
    #[error("{path}: {source}")]
    Stream {
        /// The entry.
        path: String,
        /// Why.
        source: ObjectStreamError,
    },
    /// An entry is not a tree.
    #[error("{path}: {source}")]
    Decode {
        /// The entry.
        path: String,
        /// Why.
        source: DecodeError,
    },
    /// The pak holds no tree.
    #[error("{0} holds no behavior tree")]
    Empty(PathBuf),
}

/// The behavior tree entries of the pak at `path` (lower-case path and bytes), sorted by path.
///
/// # Errors
///
/// Returns [`ServerPakError`] when the pak cannot be read or holds no tree.
pub fn server_pak_entries(
    path: &Path,
    decompressor: &dyn Decompressor,
) -> Result<Vec<(String, Vec<u8>)>, ServerPakError> {
    let mut pak = PakArchive::open(path)?;
    let mut entries = Vec::new();
    for entry in pak.entries()? {
        let lower = entry.path.to_lowercase();
        if lower.ends_with(TREE_EXTENSION) {
            entries.push((lower, pak.read(&entry, decompressor)?));
        }
    }
    if entries.is_empty() {
        return Err(ServerPakError::Empty(path.to_path_buf()));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(entries)
}

/// Decodes tree entries (path, ObjectStream bytes) into `decoder`, in order.
///
/// # Errors
///
/// Returns [`ServerPakError`] for the first entry that does not decode.
pub fn decode_entries(
    decoder: &mut TreeDecoder,
    entries: &[(String, Vec<u8>)],
) -> Result<(), ServerPakError> {
    for (path, bytes) in entries {
        let root = parse_object_stream(bytes).map_err(|source| ServerPakError::Stream {
            path: path.clone(),
            source,
        })?;
        decoder
            .add_tree(path, &root)
            .map_err(|source| ServerPakError::Decode {
                path: path.clone(),
                source,
            })?;
    }
    Ok(())
}

/// The fields of one class element, its bases included, by name CRC.
struct Fields<'a> {
    class: Class,
    items: Vec<(u32, &'a Element, bool)>,
}

impl<'a> Fields<'a> {
    fn of(class: Class, element: &'a Element) -> Self {
        let mut items = Vec::new();
        collect_fields(element, &mut items);
        Self { class, items }
    }

    fn take(&mut self, name: &'static str) -> Option<&'a Element> {
        let crc = lowercase_crc32(name);
        self.items
            .iter_mut()
            .find(|(known, _, used)| *known == crc && !*used)
            .map(|(_, element, used)| {
                *used = true;
                *element
            })
    }

    fn need(&mut self, name: &'static str) -> Result<&'a Element, DecodeError> {
        let class = self.class;
        self.take(name)
            .ok_or(DecodeError::MissingField { class, field: name })
    }

    /// Fails on the first field nothing read.
    fn finish(self) -> Result<(), DecodeError> {
        match self.items.iter().find(|(_, _, used)| !used) {
            Some((crc, _, _)) => Err(DecodeError::UnreadField {
                class: self.class,
                crc: *crc,
            }),
            None => Ok(()),
        }
    }
}

fn collect_fields<'a>(element: &'a Element, items: &mut Vec<(u32, &'a Element, bool)>) {
    let base = lowercase_crc32(BASE_CLASS);
    for child in &element.children {
        match child.name_crc {
            Some(crc) if crc == base => collect_fields(child, items),
            Some(crc) => items.push((crc, child, false)),
            None => {}
        }
    }
}

fn class(element: &Element) -> Result<Class, DecodeError> {
    class_of(&element.type_id).ok_or(DecodeError::UnknownClass(element.type_id))
}

fn expect(field: &'static str, element: &Element, expected: Class) -> Result<(), DecodeError> {
    let found = class(element)?;
    if found == expected {
        Ok(())
    } else {
        Err(DecodeError::WrongClass {
            field,
            found,
            expected: "another class",
        })
    }
}

fn fixed<const N: usize>(field: &'static str, element: &Element) -> Result<[u8; N], DecodeError> {
    element
        .value
        .as_slice()
        .try_into()
        .map_err(|_| DecodeError::BadValue {
            field,
            class: class_of(&element.type_id).unwrap_or(Class::Node),
            size: element.value.len(),
        })
}

/// A primitive number, bool or string as a value.
enum Primitive {
    Bool(bool),
    Int(i64),
    Float(f32),
    Text(String),
}

fn primitive(field: &'static str, element: &Element) -> Result<Primitive, DecodeError> {
    Ok(match class(element)? {
        Class::Bool => Primitive::Bool(fixed::<1>(field, element)?[0] != 0),
        Class::Float => Primitive::Float(f32::from_be_bytes(fixed(field, element)?)),
        Class::Int => Primitive::Int(i64::from(i32::from_be_bytes(fixed(field, element)?))),
        Class::UnsignedInt => Primitive::Int(i64::from(u32::from_be_bytes(fixed(field, element)?))),
        Class::UnsignedChar => Primitive::Int(i64::from(fixed::<1>(field, element)?[0])),
        Class::UnsignedShort => {
            Primitive::Int(i64::from(u16::from_be_bytes(fixed(field, element)?)))
        }
        Class::U64 => {
            let value = u64::from_be_bytes(fixed(field, element)?);
            Primitive::Int(i64::try_from(value).map_err(|_| DecodeError::BadEnum {
                field,
                value: i64::MAX,
            })?)
        }
        Class::String => Primitive::Text(String::from_utf8_lossy(&element.value).into_owned()),
        found => {
            return Err(DecodeError::WrongClass {
                field,
                found,
                expected: "a primitive",
            });
        }
    })
}

/// Builds the shared arena from the trees given to [`TreeDecoder::add_tree`].
#[derive(Debug, Default)]
pub struct TreeDecoder {
    names: Vec<String>,
    name_index: HashMap<String, NameId>,
    keys: Vec<NameId>,
    key_index: HashMap<String, KeyId>,
    nodes: Vec<Node>,
    /// Nodes by their postcard bytes (a node holds floats, so it has no `Hash`).
    node_index: HashMap<Vec<u8>, NodeId>,
    child_ids: Vec<NodeId>,
    child_index: HashMap<Vec<NodeId>, u32>,
    trees: Vec<TreeAsset>,
}

/// What [`TreeDecoder::finish`] returns: the parts of [`crate::data::AiData`] the trees fill.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DecodedTrees {
    /// Every string.
    pub names: Vec<String>,
    /// The blackboard keys, by name id.
    pub keys: Vec<NameId>,
    /// The node arena.
    pub nodes: Vec<Node>,
    /// The child lists.
    pub child_ids: Vec<NodeId>,
    /// The trees, in the order they were added.
    pub trees: Vec<TreeAsset>,
}

/// A decoded node and its size with every sub-tree copy counted.
struct Decoded {
    id: Option<NodeId>,
    expanded: u32,
}

impl TreeDecoder {
    /// A decoder that already has `names` (their ids stay the same).
    pub fn with_names(names: Vec<String>) -> Self {
        let name_index = names
            .iter()
            .enumerate()
            .filter_map(|(index, name)| Some((name.clone(), NameId::try_from(index).ok()?)))
            .collect();
        Self {
            names,
            name_index,
            ..Self::default()
        }
    }

    /// The string `id`.
    pub fn name(&self, id: NameId) -> &str {
        self.names.get(id as usize).map_or("", String::as_str)
    }

    /// The id of `text`, added when new.
    pub fn intern(&mut self, text: &str) -> NameId {
        if let Some(id) = self.name_index.get(text) {
            return *id;
        }
        let id = NameId::try_from(self.names.len()).unwrap_or(NameId::MAX);
        self.names.push(text.to_owned());
        self.name_index.insert(text.to_owned(), id);
        id
    }

    /// The key named `text` (keys are matched without case: the client keys a blackboard
    /// entry by the lower-case CRC32 of its name, study section 1.2), added when new.
    pub fn key(&mut self, text: &str) -> KeyId {
        let lower = text.to_lowercase();
        if let Some(id) = self.key_index.get(&lower) {
            return *id;
        }
        let name = self.intern(text);
        let id = KeyId::try_from(self.keys.len()).unwrap_or(KeyId::MAX);
        self.keys.push(name);
        self.key_index.insert(lower, id);
        id
    }

    /// Reads one `.behaviortree` stream root (a `Tree`) found at `path` and returns its index.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] when an element is not what the layout needs.
    pub fn add_tree(&mut self, path: &str, root: &Element) -> Result<u32, DecodeError> {
        let decoded = self.tree(root)?;
        let path = self.intern(&path.to_lowercase());
        let index = u32::try_from(self.trees.len()).map_err(|_| DecodeError::Full)?;
        self.trees.push(TreeAsset {
            path,
            root: decoded.1.id,
            expanded_nodes: decoded.1.expanded,
        });
        Ok(index)
    }

    /// The literal of an element outside a tree (a slice field): a primitive, or a
    /// `BehaviorParameter` or `AZStd::any` that holds one.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] when the element holds no literal.
    pub fn value_of(
        &mut self,
        field: &'static str,
        element: &Element,
    ) -> Result<Value, DecodeError> {
        let element = match class(element)? {
            Class::Any => element.children.first().ok_or(DecodeError::MissingField {
                class: Class::Any,
                field,
            })?,
            _ => element,
        };
        match self.param_of(field, element)? {
            Param {
                key: None,
                value: Some(value),
            } => Ok(value),
            _ => Err(DecodeError::WrongClass {
                field,
                found: class(element)?,
                expected: "a literal",
            }),
        }
    }

    /// An `AZ::Data::Asset` outside a tree; `None` for an empty reference.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] when the element is not an asset reference.
    pub fn asset_of(
        &mut self,
        field: &'static str,
        element: &Element,
    ) -> Result<Option<AssetRef>, DecodeError> {
        self.asset(field, element)
    }

    /// A `Parameters` (`SelectPositionParams`) outside a tree (a slice navigation config).
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] when the element is not one.
    pub fn select_position_of(
        &mut self,
        element: &Element,
    ) -> Result<SelectPositionParams, DecodeError> {
        self.select_position(element)
    }

    /// The decoded trees and the arena.
    pub fn finish(self) -> DecodedTrees {
        DecodedTrees {
            names: self.names,
            keys: self.keys,
            nodes: self.nodes,
            child_ids: self.child_ids,
            trees: self.trees,
        }
    }

    /// A `Tree` element: its name and root.
    fn tree(&mut self, element: &Element) -> Result<(String, Decoded), DecodeError> {
        expect("Tree", element, Class::Tree)?;
        let mut fields = Fields::of(Class::Tree, element);
        let name = self.string("Name", fields.need("Name")?)?;
        let root = self.node_ptr(fields.need("Root")?)?;
        fields.take("Id");
        fields.finish()?;
        Ok((name, root))
    }

    fn string(&mut self, field: &'static str, element: &Element) -> Result<String, DecodeError> {
        match primitive(field, element)? {
            Primitive::Text(text) => Ok(text),
            _ => Err(DecodeError::WrongClass {
                field,
                found: class(element)?,
                expected: "a string",
            }),
        }
    }

    /// A text field: a string, or a `BehaviorParameter` that holds a text literal and no key.
    fn text(&mut self, fields: &mut Fields<'_>, name: &'static str) -> Result<NameId, DecodeError> {
        const EXPECTED: &str = "a text";
        match self.fixed_value(fields, name, EXPECTED)? {
            Value::Text(text) => Ok(text),
            _ => Err(DecodeError::BadEnum {
                field: name,
                value: i64::MIN,
            }),
        }
    }

    /// A text parameter: a text literal, a key, or a key with a text fall-back.
    fn text_param(
        &mut self,
        fields: &mut Fields<'_>,
        name: &'static str,
    ) -> Result<Param, DecodeError> {
        let element = fields.need(name)?;
        let param = self.param_of(name, element)?;
        match param.value {
            None | Some(Value::Text(_)) => Ok(param),
            Some(_) => Err(DecodeError::WrongClass {
                field: name,
                found: class(element)?,
                expected: "a text",
            }),
        }
    }

    /// The literal of a field that is a primitive or a `BehaviorParameter` without a key (many
    /// fields are stored either way).
    fn fixed_value(
        &mut self,
        fields: &mut Fields<'_>,
        name: &'static str,
        expected: &'static str,
    ) -> Result<Value, DecodeError> {
        let element = fields.need(name)?;
        match self.param_of(name, element)? {
            Param {
                key: None,
                value: Some(value),
            } => Ok(value),
            _ => Err(DecodeError::WrongClass {
                field: name,
                found: class(element)?,
                expected,
            }),
        }
    }

    fn int(&mut self, fields: &mut Fields<'_>, name: &'static str) -> Result<i64, DecodeError> {
        const EXPECTED: &str = "an integer";
        match self.fixed_value(fields, name, EXPECTED)? {
            Value::Int(value) => Ok(value),
            _ => Err(DecodeError::BadEnum {
                field: name,
                value: i64::MIN,
            }),
        }
    }

    fn boolean(
        &mut self,
        fields: &mut Fields<'_>,
        name: &'static str,
    ) -> Result<bool, DecodeError> {
        const EXPECTED: &str = "a bool";
        match self.fixed_value(fields, name, EXPECTED)? {
            Value::Bool(value) => Ok(value),
            _ => Err(DecodeError::BadEnum {
                field: name,
                value: i64::MIN,
            }),
        }
    }

    fn float(&mut self, fields: &mut Fields<'_>, name: &'static str) -> Result<f32, DecodeError> {
        const EXPECTED: &str = "a float";
        match self.fixed_value(fields, name, EXPECTED)? {
            Value::Float(value) => Ok(value),
            _ => Err(DecodeError::BadEnum {
                field: name,
                value: i64::MIN,
            }),
        }
    }

    /// A literal of a primitive element.
    fn literal(&mut self, field: &'static str, element: &Element) -> Result<Value, DecodeError> {
        Ok(match primitive(field, element)? {
            Primitive::Bool(value) => Value::Bool(value),
            Primitive::Int(value) => Value::Int(value),
            Primitive::Float(value) => Value::Float(value),
            Primitive::Text(text) => Value::Text(self.intern(&text)),
        })
    }

    /// A field that is a `BehaviorParameter` or a primitive literal; a missing field is unset.
    fn param(&mut self, fields: &mut Fields<'_>, name: &'static str) -> Result<Param, DecodeError> {
        match fields.take(name) {
            Some(element) => self.param_of(name, element),
            None => Ok(Param::UNSET),
        }
    }

    fn param_of(&mut self, field: &'static str, element: &Element) -> Result<Param, DecodeError> {
        if class(element)? != Class::BehaviorParameter {
            return Ok(Param::literal(self.literal(field, element)?));
        }
        let mut fields = Fields::of(Class::BehaviorParameter, element);
        let key_name = self.string("BlackboardKeyName", fields.need("BlackboardKeyName")?)?;
        let stored = match fields.take("BlackboardKey") {
            Some(crc) => Some(u64::from_be_bytes(fixed("BlackboardKey", crc)?)),
            None => None,
        };
        let storage = fields.need("Storage")?;
        expect("Storage", storage, Class::Any)?;
        let value = match storage.children.first() {
            Some(data) => Some(self.literal(field, data)?),
            None => None,
        };
        fields.take("IsDefaultValue");
        fields.finish()?;
        // A stored key of 0 names no key (Observed: a slice parameter of
        // `damned_villager_cleaver_15` holds eight NUL bytes as its name and key 0).
        let key = if key_name.is_empty() || stored == Some(0) {
            None
        } else {
            let expected = u64::from(lowercase_crc32(&key_name));
            if let Some(stored) = stored.filter(|stored| *stored != expected) {
                return Err(DecodeError::KeyCrc {
                    name: key_name,
                    stored,
                    expected,
                });
            }
            Some(self.key(&key_name))
        };
        Ok(Param { key, value })
    }

    /// A string field naming a blackboard key; empty means none.
    fn key_field(
        &mut self,
        fields: &mut Fields<'_>,
        name: &'static str,
    ) -> Result<Option<KeyId>, DecodeError> {
        let text = self.string(name, fields.need(name)?)?;
        Ok((!text.is_empty()).then(|| self.key(&text)))
    }

    fn asset(
        &mut self,
        field: &'static str,
        element: &Element,
    ) -> Result<Option<AssetRef>, DecodeError> {
        expect(field, element, Class::Asset)?;
        let value = &element.value;
        let bad = || DecodeError::BadValue {
            field,
            class: Class::Asset,
            size: value.len(),
        };
        let guid: [u8; ASSET_GUID_LEN] = value
            .get(..ASSET_GUID_LEN)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(bad)?;
        let hint_len = value
            .get(ASSET_HINT_LEN_AT..ASSET_HINT_AT)
            .and_then(|bytes| <[u8; 8]>::try_from(bytes).ok())
            .map(u64::from_be_bytes)
            .and_then(|len| usize::try_from(len).ok())
            .ok_or_else(bad)?;
        let hint = value
            .get(ASSET_HINT_AT..ASSET_HINT_AT + hint_len)
            .ok_or_else(bad)?;
        let hint = String::from_utf8_lossy(hint).to_lowercase();
        if guid == [0; ASSET_GUID_LEN] && hint.is_empty() {
            return Ok(None);
        }
        Ok(Some(AssetRef {
            guid,
            hint: (!hint.is_empty()).then(|| self.intern(&hint)),
        }))
    }

    fn position(
        &mut self,
        fields: &mut Fields<'_>,
        name: &'static str,
    ) -> Result<PositionType, DecodeError> {
        let element = fields.need(name)?;
        expect(name, element, Class::PositionType)?;
        let mut inner = Fields::of(Class::PositionType, element);
        let value = self.int(&mut inner, "PositionType")?;
        inner.finish()?;
        PositionType::from_stored(value).ok_or(DecodeError::BadEnum { field: name, value })
    }

    fn conditions(
        &mut self,
        fields: &mut Fields<'_>,
        name: &'static str,
    ) -> Result<ConditionGroup, DecodeError> {
        let element = fields.need(name)?;
        expect(name, element, Class::ConditionGroup)?;
        let mut group = Fields::of(Class::ConditionGroup, element);
        let list = group.need("Conditions")?;
        let logic = self.int(&mut group, "GroupLogicalOperator")?;
        group.finish()?;
        let mut conditions = Vec::new();
        for entry in &list.children {
            expect("Conditions", entry, Class::Condition)?;
            let mut condition = Fields::of(Class::Condition, entry);
            let left = self.param(&mut condition, "LeftOperand")?;
            let op_number = self.int(&mut condition, "Operator")?;
            let op = CompareOp::from_stored(op_number).ok_or(DecodeError::BadEnum {
                field: "Operator",
                value: op_number,
            })?;
            let right = self.param(&mut condition, "RightOperand")?;
            let auto_plan = self.boolean(&mut condition, "AutoPlan")?;
            let allow_dynamic_scaling = self.boolean(&mut condition, "AllowDynamicScaling")?;
            condition.finish()?;
            conditions.push(Condition {
                left,
                op,
                right,
                auto_plan,
                allow_dynamic_scaling,
            });
        }
        Ok(ConditionGroup {
            any: logic == GROUP_OR,
            conditions,
        })
    }

    fn gates(&mut self, fields: &mut Fields<'_>) -> Result<Gates, DecodeError> {
        Ok(Gates {
            entry: self.conditions(fields, "EntryConditions")?,
            continuation: self.conditions(fields, "ContinuationConditions")?,
            interrupt: self.conditions(fields, "InterruptConditions")?,
        })
    }

    fn limits(&mut self, element: &Element) -> Result<TargetLimits, DecodeError> {
        expect("LimitsOverride", element, Class::TargetSelectionLimits)?;
        let mut fields = Fields::of(Class::TargetSelectionLimits, element);
        let limits = TargetLimits {
            min_targets: self.param(&mut fields, "m_minTargets")?,
            max_targets: self.param(&mut fields, "m_maxTargets")?,
            top_exclude: self.param(&mut fields, "m_topTargetsToExclude")?,
            range_behavior: self.param(&mut fields, "m_targetSelectionLimitsRangeBehavior")?,
        };
        fields.finish()?;
        Ok(limits)
    }

    fn action_target(&mut self, element: &Element) -> Result<ActionTarget, DecodeError> {
        expect("ActionTarget", element, Class::ActionTargetHelper)?;
        let mut fields = Fields::of(Class::ActionTargetHelper, element);
        let profile = self.asset("TargetingProfile", fields.need("TargetingProfile")?)?;
        let tag_filter = self.text(&mut fields, "TargetTagFilter")?;
        let limits = self.limits(fields.need("LimitsOverride")?)?;
        fields.finish()?;
        Ok(ActionTarget {
            profile,
            tag_filter,
            limits,
        })
    }

    fn nav_ray_cast(&mut self, element: &Element) -> Result<NavRayCast, DecodeError> {
        expect("NavRayCast", element, Class::NavRayCastHelper)?;
        let mut fields = Fields::of(Class::NavRayCastHelper, element);
        let ray = NavRayCast {
            direction: self.param(&mut fields, "Direction")?,
            distance: self.param(&mut fields, "Distance")?,
            distance_from_wall: self.param(&mut fields, "DistanceFromWall")?,
            check_at_execution: self.boolean(&mut fields, "CheckAtExecution")?,
            allow_dynamic_scaling: self.boolean(&mut fields, "AllowDynamicScaling")?,
        };
        fields.finish()?;
        Ok(ray)
    }

    fn select_position(&mut self, element: &Element) -> Result<SelectPositionParams, DecodeError> {
        expect("SelectPositionParams", element, Class::Parameters)?;
        let mut fields = Fields::of(Class::Parameters, element);
        let params = SelectPositionParams {
            origin: self.position(&mut fields, "Origin")?,
            anchor: self.position(&mut fields, "Anchor")?,
            direction: self.param(&mut fields, "Direction")?,
            mode: self.param(&mut fields, "PositionSelectMode")?,
            check_path_to_origin: self.boolean(&mut fields, "CheckPathToOrigin")?,
            check_path_to_anchor: self.boolean(&mut fields, "CheckPathToAnchor")?,
            start_angle: self.param(&mut fields, "StartAngle")?,
            stop_angle: self.param(&mut fields, "StopAngle")?,
            favor_middle_angle: self.boolean(&mut fields, "FavorMiddleAngle")?,
            range_min: self.param(&mut fields, "RangeMin")?,
            range_max: self.param(&mut fields, "RangeMax")?,
            distance_from_wall: self.param(&mut fields, "DistanceFromWall")?,
            query_radius: self.param(&mut fields, "QueryRadius")?,
            max_attempts: self.param(&mut fields, "MaxAttempts")?,
            start_angle_param: self.param(&mut fields, "StartAngleParam")?,
            stop_angle_param: self.param(&mut fields, "StopAngleParam")?,
            range_min_param: self.param(&mut fields, "RangeMinParam")?,
            range_max_param: self.param(&mut fields, "RangeMaxParam")?,
            distance_from_wall_param: self.param(&mut fields, "DistanceFromWallParam")?,
            query_radius_param: self.param(&mut fields, "QueryRadiusParam")?,
            max_attempts_param: self.param(&mut fields, "MaxAttemptsParam")?,
            group_detector: self.param(&mut fields, "GroupDetector")?,
            group_range: self.param(&mut fields, "GroupRange")?,
        };
        fields.finish()?;
        Ok(params)
    }

    fn cooldown(&mut self, fields: &mut Fields<'_>) -> Result<CooldownSpec, DecodeError> {
        let name = self.text(fields, "CooldownName")?;
        let min = self.param(fields, "TimeMin")?;
        let max = self.param(fields, "TimeMax")?;
        let stored = self.int(fields, "Override")?;
        let override_rule = CooldownOverride::from_stored(stored).ok_or(DecodeError::BadEnum {
            field: "Override",
            value: stored,
        })?;
        Ok(CooldownSpec {
            name,
            min,
            max,
            override_rule,
        })
    }

    fn cage(&mut self, fields: &mut Fields<'_>) -> Result<CageParams, DecodeError> {
        Ok(CageParams {
            task: self.text_param(fields, "ActionName")?,
            wait_for_completion: self.boolean(fields, "WaitForCAGECompletion")?,
            fail_timeout: self.param(fields, "FailTimeoutSeconds")?,
            nav_ray_cast: self.nav_ray_cast(fields.need("NavRayCast")?)?,
            align_position: self.param(fields, "AlignToObjectPosition")?,
            align_orientation: self.param(fields, "AlignToObjectOrientation")?,
            can_kill_deaths_door: self.boolean(fields, "CanKillDeathsDoor")?,
            target: self.action_target(fields.need("ActionTarget")?)?,
            target_evaluation_ms: self.param(fields, "ActionTargetEvaluationFrequencyMS")?,
        })
    }

    /// The motion fields that navigation and strafe actions share.
    fn motion(&mut self, fields: &mut Fields<'_>) -> Result<Motion, DecodeError> {
        let nav_config = self.optional_text(fields, "NavConfigType")?;
        let max_speed = self.param(fields, "MaxSpeed")?;
        let min_speed = self.param(fields, "MinSpeed")?;
        let linear_acceleration = self.param(fields, "LinearAcceleration")?;
        let linear_deceleration = self.param(fields, "LinearDeceleration")?;
        let arrival_threshold = self.param(fields, "ArrivalThreshold")?;
        let target_repath_distance = self.param(fields, "TargetRepathDistance")?;
        let allow_dynamic_scaling = self.boolean(fields, "AllowDynamicScaling")?;
        let allowed_stuck_resolutions = self.param(fields, "AllowedStuckResolutions")?;
        let give_up_behavior = self.param(fields, "GiveUpBehavior")?;
        let has_select = self.boolean(fields, "HasSelectPositionParams")?;
        let params = self.select_position(fields.need("SelectPositionParams")?)?;
        Ok(Motion {
            nav_config,
            max_speed,
            min_speed,
            linear_acceleration,
            linear_deceleration,
            arrival_threshold,
            target_repath_distance,
            allow_dynamic_scaling,
            allowed_stuck_resolutions,
            give_up_behavior,
            select_position: has_select.then_some(params),
        })
    }

    /// A text parameter that may hold nothing: `None` for no value or an empty text.
    fn optional_text(
        &mut self,
        fields: &mut Fields<'_>,
        name: &'static str,
    ) -> Result<Option<NameId>, DecodeError> {
        let element = fields.need(name)?;
        match self.param_of(name, element)? {
            Param {
                key: None,
                value: Some(Value::Text(text)),
            } => Ok((!self.names[text as usize].is_empty()).then_some(text)),
            Param {
                key: None,
                value: None,
            } => Ok(None),
            _ => Err(DecodeError::WrongClass {
                field: name,
                found: class(element)?,
                expected: "a text or nothing",
            }),
        }
    }

    fn navigation(&mut self, fields: &mut Fields<'_>) -> Result<NavigationParams, DecodeError> {
        let cage = self.cage(fields)?;
        let motion = self.motion(fields)?;
        let nav_key_text = self.string("NavKey", fields.need("NavKey")?)?;
        self.check_crc(fields, "NavKeyCrc", &nav_key_text)?;
        let nav_key = (!nav_key_text.is_empty()).then(|| self.key(&nav_key_text));
        Ok(NavigationParams {
            cage,
            motion,
            nav_key,
            turn_rate: self.param(fields, "TurnRate")?,
            angular_acceleration: self.param(fields, "AngularAcceleration")?,
            angular_deceleration: self.param(fields, "AngularDeceleration")?,
            facing_mode: self.param(fields, "FacingMode")?,
            face_movement_smooth_time: self.param(fields, "FaceMovementSmoothTime")?,
            agent_radius: self.param(fields, "AgentRadius")?,
            teleport: self.boolean(fields, "Teleport")?,
            allow_partial_paths: self.boolean(fields, "AllowPartialPaths")?,
            calculate_path_on_exec: self.boolean(fields, "CalculatePathOnExec")?,
            start_action_override: self.optional_text(fields, "StartActionOverride")?,
            turn_action_override: self.optional_text(fields, "TurnActionOverride")?,
            observe_heat: self.boolean(fields, "ObserveHeat")?,
            apply_heat: self.boolean(fields, "ApplyHeat")?,
            max_corridor_width: self.param(fields, "MaxCorridorWidth")?,
            nearby_nav_link_removal_threshold: self
                .param(fields, "NearbyNavLinkRemovalThreshold")?,
            waypoint_look_ahead_radius: self.param(fields, "WaypointLookAheadRadius")?,
            max_steering_force: self.param(fields, "MaxSteeringForce")?,
            max_avoidance_force: self.param(fields, "MaxAvoidanceForce")?,
            max_separation_force: self.param(fields, "MaxSeparationForce")?,
            steering_ramp_down_range_min: self.param(fields, "SteeringRampDownRangeMin")?,
            steering_ramp_down_range_max: self.param(fields, "SteeringRampDownRangeMax")?,
            enable_avoidance: self.boolean(fields, "EnableAvoidance")?,
            avoidance_look_ahead_horizon: self.param(fields, "AvoidanceLookAheadHorizon")?,
            avoidance_force: self.param(fields, "AvoidanceForce")?,
            enable_separation: self.boolean(fields, "EnableSeparation")?,
            separation_circle_force: self.param(fields, "SeparationCircleForce")?,
            separation_circle_radius: self.param(fields, "SeparationCircleRadius")?,
            separation_cone_force: self.param(fields, "SeparationConeForce")?,
            separation_cone_range: self.param(fields, "SeparationConeRange")?,
            separation_cone_angle: self.param(fields, "SeparationConeAngle")?,
        })
    }

    fn strafe(&mut self, fields: &mut Fields<'_>) -> Result<StrafeParams, DecodeError> {
        let cage = self.cage(fields)?;
        let motion = self.motion(fields)?;
        let config = motion
            .nav_config
            .map_or_else(String::new, |config| self.names[config as usize].clone());
        self.check_crc(fields, "NavConfigTypeCrc", &config)?;
        Ok(StrafeParams {
            cage,
            motion,
            ideal_distance_min: self.param(fields, "IdealDistanceMin")?,
            ideal_distance_max: self.param(fields, "IdealDistanceMax")?,
            approach_distance_ratio: self.param(fields, "ApproachDistanceRatio")?,
            retreat_distance_ratio: self.param(fields, "RetreatDistanceRatio")?,
            lateral_frequency_min: self.param(fields, "LateralMovementFrequencyMin")?,
            lateral_frequency_max: self.param(fields, "LateralMovementFrequencyMax")?,
            lateral_angle_min: self.param(fields, "LateralMovementAngleMin")?,
            lateral_angle_max: self.param(fields, "LateralMovementAngleMax")?,
            direct_frequency_min: self.param(fields, "DirectMovementFrequencyMin")?,
            direct_frequency_max: self.param(fields, "DirectMovementFrequencyMax")?,
            allow_standing_still: self.param(fields, "AllowStandingStill")?,
            stand_still_frequency_min: self.param(fields, "StandStillFrequencyMin")?,
            stand_still_frequency_max: self.param(fields, "StandStillFrequencyMax")?,
            stand_still_duration_min: self.param(fields, "StandStillDurationMin")?,
            stand_still_duration_max: self.param(fields, "StandStillDurationMax")?,
            reposition_keep_away_distance: self.param(fields, "RepositionKeepAwayDistance")?,
            facing_convergence_rate: self.param(fields, "FacingConvergenceRate")?,
            direction_change_rate: self.param(fields, "DirectionChangeRateDegs")?,
        })
    }

    /// Checks that the `AZ::Crc32` field `name` holds the lower-case CRC32 of `text` (0 for an
    /// empty text), as `NavKeyCrc` beside `NavKey` (Observed: `Target_Pos` 1338623065).
    fn check_crc(
        &mut self,
        fields: &mut Fields<'_>,
        name: &'static str,
        text: &str,
    ) -> Result<(), DecodeError> {
        let element = fields.need(name)?;
        expect(name, element, Class::Crc32)?;
        let mut inner = Fields::of(Class::Crc32, element);
        let stored = self.int(&mut inner, "Value")?;
        inner.finish()?;
        let expected = if text.is_empty() {
            0
        } else {
            i64::from(lowercase_crc32(text))
        };
        if stored == expected {
            Ok(())
        } else {
            Err(DecodeError::KeyCrc {
                name: text.to_owned(),
                stored: u64::try_from(stored).unwrap_or(u64::MAX),
                expected: u64::try_from(expected).unwrap_or(u64::MAX),
            })
        }
    }

    fn tag_operation(&mut self, element: &Element) -> Result<TagOperation, DecodeError> {
        expect("TagOperation", element, Class::TagOperation)?;
        let mut fields = Fields::of(Class::TagOperation, element);
        let operation = TagOperation {
            tag: self.text(&mut fields, "Tag")?,
            add: self.boolean(&mut fields, "IsOperation")?,
            strict: self.boolean(&mut fields, "PerformStrictOperation")?,
        };
        fields.finish()?;
        Ok(operation)
    }

    /// An `intrusive_ptr<Node>`: `None` when it holds nothing.
    fn node_ptr(&mut self, element: &Element) -> Result<Decoded, DecodeError> {
        expect("node pointer", element, Class::NodePtr)?;
        match element.children.first() {
            Some(node) => self.node(node),
            None => Ok(Decoded {
                id: None,
                expanded: 0,
            }),
        }
    }

    fn child(
        &mut self,
        fields: &mut Fields<'_>,
        expanded: &mut u32,
    ) -> Result<Option<NodeId>, DecodeError> {
        let decoded = self.node_ptr(fields.need("Child")?)?;
        *expanded += decoded.expanded;
        Ok(decoded.id)
    }

    fn children(
        &mut self,
        fields: &mut Fields<'_>,
        expanded: &mut u32,
    ) -> Result<Children, DecodeError> {
        let list = fields.need("Children")?;
        expect("Children", list, Class::NodeVector)?;
        let mut ids = Vec::with_capacity(list.children.len());
        for entry in &list.children {
            let decoded = self.node_ptr(entry)?;
            *expanded += decoded.expanded;
            if let Some(id) = decoded.id {
                ids.push(id);
            }
        }
        let count = u32::try_from(ids.len()).map_err(|_| DecodeError::Full)?;
        let first = match self.child_index.get(&ids) {
            Some(first) => *first,
            None => {
                let first = u32::try_from(self.child_ids.len()).map_err(|_| DecodeError::Full)?;
                self.child_ids.extend_from_slice(&ids);
                self.child_index.insert(ids, first);
                first
            }
        };
        Ok(Children { first, count })
    }

    /// A node class element.
    fn node(&mut self, element: &Element) -> Result<Decoded, DecodeError> {
        let class = class(element)?;
        let mut fields = Fields::of(class, element);
        fields.take("Id");
        let name = self.text(&mut fields, "Name")?;
        let utility = self.float(&mut fields, "BaseUtility")?;
        let interruptable = self.boolean(&mut fields, "Interruptable")?;
        let mut expanded = 1;
        let kind = self.kind(class, &mut fields, &mut expanded)?;
        fields.finish()?;
        let node = Node {
            name,
            utility,
            interruptable,
            kind,
        };
        let bytes = postcard::to_allocvec(&node).map_err(|_| DecodeError::Full)?;
        let id = match self.node_index.get(&bytes) {
            Some(id) => *id,
            None => {
                let id = NodeId::try_from(self.nodes.len()).map_err(|_| DecodeError::Full)?;
                self.nodes.push(node);
                self.node_index.insert(bytes, id);
                id
            }
        };
        Ok(Decoded {
            id: Some(id),
            expanded,
        })
    }

    fn kind(
        &mut self,
        class: Class,
        fields: &mut Fields<'_>,
        expanded: &mut u32,
    ) -> Result<NodeKind, DecodeError> {
        Ok(match class {
            Class::Sequencer => NodeKind::Sequencer(self.children(fields, expanded)?),
            Class::PermissiveSequencer => {
                NodeKind::PermissiveSequencer(self.children(fields, expanded)?)
            }
            Class::Selector => NodeKind::Selector(self.children(fields, expanded)?),
            Class::UtilitySelector => {
                let children = self.children(fields, expanded)?;
                let method = match self.int(fields, "SelectionMethod")? {
                    SELECTION_TOP => SelectionMethod::Top,
                    SELECTION_WEIGHTED => SelectionMethod::WeightedRandom,
                    value => {
                        return Err(DecodeError::BadEnum {
                            field: "SelectionMethod",
                            value,
                        });
                    }
                };
                let top_n =
                    u64::from_be_bytes(fixed("WeightedRandomN", fields.need("WeightedRandomN")?)?);
                NodeKind::UtilitySelector {
                    children,
                    method,
                    top_n,
                    preserve_if_no_change: self
                        .boolean(fields, "PreserveExecutingPlanIfNoChange")?,
                    always_preserve: self.boolean(fields, "AlwaysPreserveExecutingPlan")?,
                }
            }
            Class::Parallel => NodeKind::Parallel {
                children: self.children(fields, expanded)?,
                completion_mode: self.param(fields, "CompletionMode")?,
            },
            Class::ForEachTargetDecorator => NodeKind::ForEachTarget {
                children: self.children(fields, expanded)?,
                execution: self.param(fields, "ExecutionIterationBehavior")?,
                evaluation: self.param(fields, "EvaluationIterationBehavior")?,
                max_targets: self.param(fields, "MaxNumberOfTargets")?,
            },
            Class::TreeReference => {
                let tree = fields.need("SerializedTree")?;
                expect("SerializedTree", tree, Class::TreePtr)?;
                let Some(inner) = tree.children.first() else {
                    return Err(DecodeError::MissingField {
                        class,
                        field: "SerializedTree",
                    });
                };
                let (path, root) = self.tree(inner)?;
                *expanded += root.expanded;
                NodeKind::SubTree {
                    path: self.intern(&path.to_lowercase()),
                    root: root.id,
                }
            }
            Class::BlackboardCondition => NodeKind::BlackboardCondition {
                child: self.child(fields, expanded)?,
                gates: Box::new(self.gates(fields)?),
            },
            Class::DistanceCondition => NodeKind::DistanceCondition {
                child: self.child(fields, expanded)?,
                gates: Box::new(self.gates(fields)?),
                vertical_offset: self.param(fields, "VerticalOffset")?,
                vertical_threshold: self.param(fields, "VerticalThreshold")?,
                allow_dynamic_scaling: self.boolean(fields, "AllowDynamicScaling")?,
            },
            Class::CooldownDecorator => NodeKind::Cooldown {
                child: self.child(fields, expanded)?,
                cooldown: self.cooldown(fields)?,
                omit_on_failure: self.boolean(fields, "OmitOnFailure")?,
            },
            Class::IsCooldownActiveCondition => NodeKind::IsCooldownActive {
                child: self.child(fields, expanded)?,
                name: self.text(fields, "CooldownName")?,
                invert: self.boolean(fields, "InvertCheck")?,
            },
            Class::MannequinTagCondition => NodeKind::MannequinTag {
                child: self.child(fields, expanded)?,
                tag: self.text(fields, "Tag")?,
                invert: self.boolean(fields, "InvertCheck")?,
            },
            Class::SetMannequinTag => NodeKind::SetMannequinTag {
                child: self.child(fields, expanded)?,
                tag: self.text(fields, "Tag")?,
            },
            Class::ActionTargetTrackerDecorator => {
                let child = self.child(fields, expanded)?;
                let target = self.action_target(fields.need("ActionTarget")?)?;
                let profile = self.asset("TargetingProfile", fields.need("TargetingProfile")?)?;
                NodeKind::ActionTargetTracker {
                    child,
                    params: Box::new(TrackerParams {
                        target,
                        evaluation_ms: self.param(fields, "ActionTargetEvaluationFrequencyMS")?,
                        profile,
                        tag_filter: self.text(fields, "TargetTagFilter")?,
                        min_targets: self.param(fields, "MinTargets")?,
                        max_targets: self.param(fields, "MaxTargets")?,
                        top_exclude: self.param(fields, "TopExclude")?,
                        all_valid_targets: self.boolean(fields, "TargetAllValidTargets")?,
                        range_behavior: self.param(fields, "TargetSelectionLimitsRangeBehavior")?,
                    }),
                }
            }
            Class::Timer => NodeKind::Timer {
                child: self.child(fields, expanded)?,
                min: self.param(fields, "TimeMin")?,
                max: self.param(fields, "TimeMax")?,
                succeed_on_timeout: self.boolean(fields, "SucceedOnTimeOut")?,
            },
            Class::FrequencyLimiter => NodeKind::FrequencyLimiter {
                child: self.child(fields, expanded)?,
                min: self.param(fields, "IntervalMin")?,
                max: self.param(fields, "IntervalMax")?,
                stamp_on_success: self.boolean(fields, "StampOnSuccess")?,
                stamp_on_failure: self.boolean(fields, "StampOnFailure")?,
            },
            Class::VariableExistsCondition => {
                let child = self.child(fields, expanded)?;
                let name = self.string("VariableName", fields.need("VariableName")?)?;
                NodeKind::VariableExists {
                    child,
                    key: self.key(&name),
                    invert: self.boolean(fields, "InvertCheck")?,
                }
            }
            Class::CountLimiter => NodeKind::CountLimiter {
                child: self.child(fields, expanded)?,
                max: self.param(fields, "MaxCount")?,
                count_on_success: self.boolean(fields, "CountOnSuccess")?,
                count_on_failure: self.boolean(fields, "CountOnFailure")?,
            },
            Class::LockTargetSelection => NodeKind::LockTargetSelection {
                child: self.child(fields, expanded)?,
            },
            Class::RandomChanceCondition => NodeKind::RandomChance {
                child: self.child(fields, expanded)?,
                rate: self.param(fields, "SuccessRate")?,
            },
            Class::StatusEffectDecorator => NodeKind::StatusEffect {
                child: self.child(fields, expanded)?,
                effect: self.text(fields, "StatusEffectId")?,
                remove_on_end: self.boolean(fields, "RemoveOnEnd")?,
            },
            Class::EncounterEventDecorator => NodeKind::EncounterEvent {
                child: self.child(fields, expanded)?,
                event: self.text(fields, "EventId")?,
                max_count: self.param(fields, "MaxCount")?,
                on_success: self.boolean(fields, "SendOnSuccess")?,
                on_failure: self.boolean(fields, "SendOnFailure")?,
                on_cancelled: self.boolean(fields, "SendOnCancelled")?,
            },
            Class::AlwaysSucceed => NodeKind::AlwaysSucceed {
                child: self.child(fields, expanded)?,
                during_evaluation: self.boolean(fields, "ApplyDuringEvaluation")?,
                during_execution: self.boolean(fields, "ApplyDuringExecution")?,
            },
            Class::VariableMonitor => {
                let child = self.child(fields, expanded)?;
                let name = self.string("VariableName", fields.need("VariableName")?)?;
                NodeKind::VariableMonitor {
                    child,
                    key: self.key(&name),
                }
            }
            Class::VisibilityRaycast => NodeKind::VisibilityRaycast {
                child: self.child(fields, expanded)?,
                start: self.position(fields, "StartPosition")?,
                end: self.position(fields, "EndPosition")?,
                offset: self.param(fields, "OffsetLength")?,
            },
            Class::EntityQueryCondition => {
                let child = self.child(fields, expanded)?;
                let gates = self.gates(fields)?;
                NodeKind::EntityQuery {
                    child,
                    params: Box::new(EntityQueryParams {
                        gates,
                        center: self.param(fields, "Center")?,
                        radius: self.param(fields, "Radius")?,
                        tag_filter: self.text(fields, "TagFilter")?,
                        matching_vitals_state: self.param(fields, "MatchingVitalsState")?,
                        not_matching_vitals_state: self.param(fields, "NotMatchingVitalsState")?,
                        in_threat_filter: self.param(fields, "InThreatFilter")?,
                        in_threat_filter_string: self.text(fields, "InThreatFilterString")?,
                    }),
                }
            }
            Class::Repeater => NodeKind::Repeater {
                child: self.child(fields, expanded)?,
                count_min: self.param(fields, "RepeatCountMin")?,
                count_max: self.param(fields, "RepeatCountMax")?,
                seconds_min: self.param(fields, "RepeatSecondsMin")?,
                seconds_max: self.param(fields, "RepeatSecondsMax")?,
            },
            Class::ManualTargeting => {
                let child = self.child(fields, expanded)?;
                let name = self.string("TargetVariable", fields.need("TargetVariable")?)?;
                NodeKind::ManualTargeting {
                    child,
                    key: self.key(&name),
                    assume_visible: self.boolean(fields, "AssumeTargetVisible")?,
                    height_offset: self.param(fields, "TargetHeightOffset")?,
                }
            }
            Class::CageAction => NodeKind::Cage(Box::new(self.cage(fields)?)),
            Class::NavigationAction => NodeKind::Navigation(Box::new(self.navigation(fields)?)),
            Class::StrafeAction => NodeKind::Strafe(Box::new(self.strafe(fields)?)),
            Class::SetCooldownAction => NodeKind::SetCooldown(self.cooldown(fields)?),
            Class::SetBlackboardFactAction => {
                let name = self.string("FactName", fields.need("FactName")?)?;
                let key = self.key(&name);
                let stored = self.int(fields, "Operator")?;
                NodeKind::SetFact {
                    key,
                    op: FactOp::from_stored(stored).ok_or(DecodeError::BadEnum {
                        field: "Operator",
                        value: stored,
                    })?,
                    value: self.param(fields, "RightOperand")?,
                }
            }
            Class::SetBlackboardFactOnRemoteEntityAction => NodeKind::SetRemoteFact {
                fact: self.text(fields, "FactName")?,
                value: self.param(fields, "Value")?,
                overwrite: self.boolean(fields, "OverwriteTargetFact")?,
                target: self.action_target(fields.need("ActionTarget")?)?,
            },
            Class::TriggerEntity => NodeKind::TriggerEntity {
                trigger: self.text(fields, "TriggerName")?,
            },
            Class::TriggerRemoteEntity => NodeKind::TriggerRemoteEntity {
                trigger: self.text(fields, "TriggerName")?,
                all_targets: self.boolean(fields, "UseAllAvailableTargets")?,
                target: self.action_target(fields.need("ActionTarget")?)?,
            },
            Class::SetBeamAttackActive => NodeKind::SetBeam {
                beam: self.text(fields, "BeamName")?,
                active: self.boolean(fields, "Active")?,
                aim_mode: self.param(fields, "AimMode")?,
                lock_target: self.boolean(fields, "LockTarget")?,
            },
            Class::ExecuteSpawn => {
                let slice = self.asset("Slice", fields.need("Slice")?)?.ok_or(
                    DecodeError::MissingField {
                        class,
                        field: "Slice",
                    },
                )?;
                NodeKind::ExecuteSpawn {
                    slice,
                    alias: self.asset("SliceAlias", fields.need("SliceAlias")?)?,
                    count: self.param(fields, "Count")?,
                    position_key: self.key_field(fields, "TargetPosBlackboardKey")?,
                    use_owner_rotation: self.boolean(fields, "UseOwnerRotation")?,
                }
            }
            Class::SelectPositionAction => NodeKind::SelectPosition {
                params: Box::new(self.select_position(fields.need("Params")?)?),
                storage: self.key_field(fields, "StorageFact")?,
                allow_dynamic_scaling: self.boolean(fields, "AllowDynamicScaling")?,
            },
            Class::FakeAction => NodeKind::Fake {
                indefinitely: self.boolean(fields, "ExecuteIndefinitely")?,
            },
            Class::CompleteOrder => NodeKind::CompleteOrder,
            Class::AssignOrder => {
                let order = fields.need("Order")?;
                expect("Order", order, Class::Crc32)?;
                let mut inner = Fields::of(Class::Crc32, order);
                let value = self.int(&mut inner, "Value")?;
                inner.finish()?;
                NodeKind::AssignOrder {
                    order: u32::try_from(value).map_err(|_| DecodeError::BadEnum {
                        field: "Order",
                        value,
                    })?,
                }
            }
            Class::CountEntitiesWithTag => {
                let tag = self.text(fields, "TagName")?;
                let name = self.string("BlackboardKey", fields.need("BlackboardKey")?)?;
                NodeKind::CountEntitiesWithTag {
                    tag,
                    key: self.key(&name),
                }
            }
            Class::Delay => NodeKind::Delay {
                min: self.param(fields, "TimeMin")?,
                max: self.param(fields, "TimeMax")?,
            },
            Class::SelectWaypointAction => NodeKind::SelectWaypoint,
            Class::SpawnProjectile => NodeKind::SpawnProjectile(Box::new(ProjectileParams {
                target: self.action_target(fields.need("ActionTarget")?)?,
                target_pos: self.param(fields, "TargetPos")?,
                use_all_available_targets: self.boolean(fields, "UseAllAvailableTargets")?,
                min_targets_per_shot: self.param(fields, "MinTargetsPerShot")?,
                max_targets_per_shot: self.param(fields, "MaxTargetsPerShot")?,
                min_seconds_between_shots: self.param(fields, "MinSecondsBetweenShots")?,
                max_seconds_between_shots: self.param(fields, "MaxSecondsBetweenShots")?,
            })),
            Class::UpdateNwTags => {
                let main = self.tag_operation(fields.need("MainTagOperation")?)?;
                let list = fields.need("TagOperations")?;
                expect("TagOperations", list, Class::TagOperationVector)?;
                let operations = list
                    .children
                    .iter()
                    .map(|entry| self.tag_operation(entry))
                    .collect::<Result<_, _>>()?;
                NodeKind::UpdateNwTags { main, operations }
            }
            other => {
                return Err(DecodeError::WrongClass {
                    field: "node",
                    found: other,
                    expected: "a node class",
                });
            }
        })
    }
}

#[cfg(all(test, feature = "local-data"))]
mod server_pak_tests {
    use std::collections::HashSet;

    use nw_formats::objectstream::write_object_stream;
    use nw_oodle::KrakenDecompressor;

    use super::*;

    /// Behavior trees in Server.pak (study section 1.1: 1,075 entries, all trees).
    const SERVER_PAK_TREES: usize = 1_075;
    /// The beach boar's tree and its node count with every sub-tree copy (study section 2.4).
    const BOAR_TREE: &str = "sharedassets/ai/behaviortrees/predator/boar/boar_base.behaviortree";
    const BOAR_EXPANDED_NODES: u32 = 802;
    /// Distinct sub-tree paths without case (study section 1.2: 789 paths; two of them,
    /// `damned/damned_POI_Alert` and `damned_poi_alert`, differ only in case).
    const SUBTREE_PATHS: usize = 788;

    fn server_pak() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local-data/server-pak/Server.pak")
    }

    #[test]
    fn every_server_pak_tree_round_trips_and_decodes() {
        let entries = server_pak_entries(&server_pak(), &KrakenDecompressor).expect("pak");
        assert_eq!(entries.len(), SERVER_PAK_TREES);
        for (path, bytes) in &entries {
            let root = parse_object_stream(bytes).expect(path);
            let written = write_object_stream(&root).expect(path);
            assert_eq!(parse_object_stream(&written).expect(path), root, "{path}");
        }
        let mut decoder = TreeDecoder::default();
        decode_entries(&mut decoder, &entries).expect("trees");
        let trees = decoder.finish();
        assert_eq!(trees.trees.len(), SERVER_PAK_TREES);
        let boar = trees
            .trees
            .iter()
            .find(|tree| trees.names[tree.path as usize] == BOAR_TREE)
            .expect("boar");
        assert_eq!(boar.expanded_nodes, BOAR_EXPANDED_NODES);
        let paths: HashSet<NameId> = trees
            .nodes
            .iter()
            .filter_map(|node| match node.kind {
                NodeKind::SubTree { path, .. } => Some(path),
                _ => None,
            })
            .collect();
        assert_eq!(paths.len(), SUBTREE_PATHS);
        let expanded: u64 = trees
            .trees
            .iter()
            .map(|tree| u64::from(tree.expanded_nodes))
            .sum();
        assert!(
            (trees.nodes.len() as u64) * 4 < expanded,
            "sharing: {} of {expanded}",
            trees.nodes.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classes::CLASSES;

    /// Name of the elements of a vector.
    const ELEMENT: &str = "element";

    fn uuid_of(class: Class) -> [u8; 16] {
        CLASSES
            .iter()
            .find(|(known, _)| *known == class)
            .map(|(_, uuid)| *uuid)
            .expect("class")
    }

    fn field(name: &str, class: Class, value: Vec<u8>, children: Vec<Element>) -> Element {
        Element {
            name_crc: Some(lowercase_crc32(name)),
            version: None,
            type_id: uuid_of(class),
            value,
            children,
        }
    }

    fn text(name: &str, value: &str) -> Element {
        field(name, Class::String, value.as_bytes().to_vec(), Vec::new())
    }

    fn float(name: &str, value: f32) -> Element {
        field(name, Class::Float, value.to_be_bytes().to_vec(), Vec::new())
    }

    fn flag(name: &str, value: bool) -> Element {
        field(name, Class::Bool, vec![u8::from(value)], Vec::new())
    }

    fn int(name: &str, value: i32) -> Element {
        field(name, Class::Int, value.to_be_bytes().to_vec(), Vec::new())
    }

    fn base(name: &str, extra: Vec<Element>) -> Element {
        let mut node = vec![
            field("Id", Class::Uuid, vec![1; 16], Vec::new()),
            text("Name", name),
            float("BaseUtility", 1.0),
            flag("Interruptable", true),
        ];
        node.extend(extra);
        field(BASE_CLASS, Class::Node, Vec::new(), node)
    }

    fn ptr(name: &str, node: Option<Element>) -> Element {
        field(name, Class::NodePtr, Vec::new(), node.into_iter().collect())
    }

    fn key_param(name: &str, key: &str, fallback: Option<f32>) -> Element {
        let storage = field(
            "Storage",
            Class::Any,
            Vec::new(),
            fallback
                .map(|value| float("m_data", value))
                .into_iter()
                .collect(),
        );
        field(
            name,
            Class::BehaviorParameter,
            Vec::new(),
            vec![
                text("BlackboardKeyName", key),
                field(
                    "BlackboardKey",
                    Class::U64,
                    u64::from(lowercase_crc32(key)).to_be_bytes().to_vec(),
                    Vec::new(),
                ),
                storage,
                flag("IsDefaultValue", false),
            ],
        )
    }

    fn set_cooldown(name: &str) -> Element {
        Element {
            name_crc: Some(lowercase_crc32(ELEMENT)),
            type_id: uuid_of(Class::SetCooldownAction),
            children: vec![
                base(name, Vec::new()),
                text("CooldownName", "AttackCooldown"),
                float("TimeMin", 2.0),
                float("TimeMax", 3.0),
                int("Override", 2),
            ],
            ..Element::default()
        }
    }

    fn condition_group(conditions: Vec<Element>) -> Vec<Element> {
        vec![
            field("Conditions", Class::ConditionVector, Vec::new(), conditions),
            int("GroupLogicalOperator", 0),
        ]
    }

    fn far_condition() -> Element {
        field(
            ELEMENT,
            Class::Condition,
            Vec::new(),
            vec![
                key_param("LeftOperand", "Target_Dist", None),
                int("Operator", 4),
                float("RightOperand", 7.0),
                flag("AutoPlan", false),
                flag("AllowDynamicScaling", true),
            ],
        )
    }

    /// A tree: Selector [ BlackboardCondition(Target_Dist > 7) -> SetCooldown, SetCooldown ].
    fn tree() -> Element {
        let gate = Element {
            name_crc: Some(lowercase_crc32(ELEMENT)),
            type_id: uuid_of(Class::BlackboardCondition),
            children: vec![
                field(
                    BASE_CLASS,
                    Class::Decorator,
                    Vec::new(),
                    vec![
                        base("Far", Vec::new()),
                        ptr("Child", Some(set_cooldown("Attack"))),
                    ],
                ),
                field(
                    "EntryConditions",
                    Class::ConditionGroup,
                    Vec::new(),
                    condition_group(vec![far_condition()]),
                ),
                field(
                    "ContinuationConditions",
                    Class::ConditionGroup,
                    Vec::new(),
                    condition_group(Vec::new()),
                ),
                field(
                    "InterruptConditions",
                    Class::ConditionGroup,
                    Vec::new(),
                    condition_group(Vec::new()),
                ),
            ],
            ..Element::default()
        };
        let children = field(
            "Children",
            Class::NodeVector,
            Vec::new(),
            vec![
                ptr(ELEMENT, Some(gate)),
                ptr(ELEMENT, Some(set_cooldown("Attack"))),
            ],
        );
        let selector = Element {
            name_crc: Some(lowercase_crc32(ELEMENT)),
            type_id: uuid_of(Class::Selector),
            children: vec![field(
                BASE_CLASS,
                Class::Composite,
                Vec::new(),
                vec![base("Root", Vec::new()), children],
            )],
            ..Element::default()
        };
        Element {
            type_id: uuid_of(Class::Tree),
            children: vec![text("Name", ""), ptr("Root", Some(selector))],
            ..Element::default()
        }
    }

    #[test]
    fn a_small_tree_decodes_with_shared_nodes() {
        let mut decoder = TreeDecoder::default();
        let index = decoder.add_tree("A/B.behaviortree", &tree()).expect("tree");
        let trees = decoder.finish();
        assert_eq!(index, 0);
        assert_eq!(
            trees.trees[0].expanded_nodes, 4,
            "selector, gate, two cooldown copies"
        );
        assert_eq!(
            trees.nodes.len(),
            3,
            "the two equal cooldown leaves are one node"
        );
        assert_eq!(
            trees.names[trees.trees[0].path as usize],
            "a/b.behaviortree"
        );
        let root = &trees.nodes[trees.trees[0].root.expect("root") as usize];
        let NodeKind::Selector(children) = root.kind else {
            panic!("selector");
        };
        let ids = &trees.child_ids[children.first as usize..][..children.count as usize];
        let NodeKind::BlackboardCondition { child, gates } = &trees.nodes[ids[0] as usize].kind
        else {
            panic!("gate");
        };
        assert_eq!(*child, Some(ids[1]));
        let condition = gates.entry.conditions[0];
        assert_eq!(condition.op, CompareOp::GreaterThan);
        assert_eq!(condition.right.value, Some(Value::Float(7.0)));
        let key = condition.left.key.expect("key");
        assert_eq!(
            trees.names[trees.keys[usize::from(key)] as usize],
            "Target_Dist"
        );
        let NodeKind::SetCooldown(cooldown) = trees.nodes[ids[1] as usize].kind else {
            panic!("cooldown");
        };
        assert_eq!(cooldown.override_rule, CooldownOverride::KeepLongest);
    }

    #[test]
    fn a_key_crc_that_is_not_the_name_crc_is_refused() {
        let mut decoder = TreeDecoder::default();
        let mut bad = key_param("LeftOperand", "Target_Dist", Some(1.0));
        bad.children[1].value = 7u64.to_be_bytes().to_vec();
        assert!(matches!(
            decoder.param_of("LeftOperand", &bad),
            Err(DecodeError::KeyCrc { .. })
        ));
        let good = key_param("LeftOperand", "TARGET_dist", Some(1.0));
        let param = decoder.param_of("LeftOperand", &good).expect("param");
        assert_eq!(param.value, Some(Value::Float(1.0)));
        assert_eq!(
            decoder.key("Target_Dist"),
            param.key.expect("key"),
            "no case"
        );
    }

    #[test]
    fn an_unknown_field_or_class_is_refused() {
        let mut decoder = TreeDecoder::default();
        let mut extra = set_cooldown("x");
        extra.children.push(flag("SomethingNew", true));
        assert!(matches!(
            decoder.node(&extra),
            Err(DecodeError::UnreadField { .. })
        ));
        let mut unknown = set_cooldown("x");
        unknown.type_id = [9; 16];
        assert_eq!(
            decoder.node(&unknown).err(),
            Some(DecodeError::UnknownClass([9; 16]))
        );
    }
}
