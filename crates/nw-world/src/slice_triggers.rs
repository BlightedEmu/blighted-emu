// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `TriggerEntityComponent`s of a dynamic slice (oquests triggers note). A creature's slice
//! (`slices/characters/...dynamicslice`, an ObjectStream in the client paks) holds one such
//! component per trigger name, most of them on a child entity. When the creature's action list
//! or behavior tree runs `TriggerEntity` with a trigger name, the server calls that component on
//! the client (type 3135, `nw_messages::trigger_entity`), and the client runs the entities the
//! component lists (effects, meshes, sounds).
//!
//! The call's target needs the slice's main entity `Id` (the first entity of the slice
//! component's `Entities`), the `Id` of the entity that owns the component, and the component
//! `Id` (Observed: 65,813 of 65,813 retail calls whose slice was read). The component's fields
//! come from the client's reflection (`FUN_143076e90`: `m_entitiesToTrigger`, `m_triggerName`;
//! `FUN_145c9c7f0`: each entry's `m_entityRef`, `m_type`, `m_applyRecursively`,
//! `m_localPlayerOnly`, and the `EventType` names).

use nw_formats::objectstream::{Element, ObjectStreamError, UUID_SIZE, lowercase_crc32};

/// Type UUID of `Javelin::TriggerEntityComponent` (`{1A4BAC98-275D-487A-88D7-F0B69CFA342F}`,
/// `FUN_143079d90`), as stored in an ObjectStream.
pub const TRIGGER_ENTITY_COMPONENT_TYPE: [u8; UUID_SIZE] = [
    0x1a, 0x4b, 0xac, 0x98, 0x27, 0x5d, 0x48, 0x7a, 0x88, 0xd7, 0xf0, 0xb6, 0x9c, 0xfa, 0x34, 0x2f,
];
/// `EventType` value `Trigger`.
const EVENT_TRIGGER: u32 = 0;
/// `EventType` value `UnTrigger`.
const EVENT_UNTRIGGER: u32 = 1;
/// `EventType` value `Activate`.
const EVENT_ACTIVATE: u32 = 2;
/// `EventType` value `Deactivate`.
const EVENT_DEACTIVATE: u32 = 3;

/// A slice whose triggers cannot be read.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SliceTriggerError {
    /// A value has another size or text form than its field needs.
    #[error("slice triggers: {0}")]
    Value(#[from] ObjectStreamError),
    /// The root has no slice component with an entity list.
    #[error("slice triggers: the root holds no slice entities")]
    NoEntities,
    /// An entity or component lacks a field the reader needs.
    #[error("slice triggers: a {0} has no {1}")]
    Missing(&'static str, &'static str),
}

/// What a trigger does to one entity (`EventType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerEvent {
    /// `Trigger` (0).
    Trigger,
    /// `UnTrigger` (1).
    UnTrigger,
    /// `Activate` (2).
    Activate,
    /// `Deactivate` (3).
    Deactivate,
    /// A value without a known name (the client registers a fifth value, 4, whose name is
    /// Unknown).
    Unnamed(u32),
}

impl From<u32> for TriggerEvent {
    fn from(value: u32) -> Self {
        match value {
            EVENT_TRIGGER => Self::Trigger,
            EVENT_UNTRIGGER => Self::UnTrigger,
            EVENT_ACTIVATE => Self::Activate,
            EVENT_DEACTIVATE => Self::Deactivate,
            other => Self::Unnamed(other),
        }
    }
}

/// One entry of `m_entitiesToTrigger`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggeredEntity {
    /// The slice entity (`m_entityRef`).
    pub entity: u64,
    /// What happens to it (`m_type`).
    pub event: TriggerEvent,
    /// `m_applyRecursively`.
    pub apply_recursively: bool,
    /// `m_localPlayerOnly`.
    pub local_player_only: bool,
}

/// One `TriggerEntityComponent` of a slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceTrigger {
    /// `m_triggerName`: the name an action list or behavior tree runs it by.
    pub name: String,
    /// The slice's main (first) entity `Id`.
    pub main: u64,
    /// The `Id` of the entity that owns the component.
    pub owner: u64,
    /// The component `Id`.
    pub component: u64,
    /// `m_entitiesToTrigger`.
    pub entities: Vec<TriggeredEntity>,
}

/// The names of the fields read, as their name CRCs.
struct Names {
    id: u32,
    base_class: u32,
    components: u32,
    entities: u32,
    trigger_name: u32,
    to_trigger: u32,
    entity_ref: u32,
    entity_id: u32,
    event: u32,
    recursive: u32,
    local_only: u32,
}

impl Names {
    fn new() -> Self {
        Self {
            id: lowercase_crc32("Id"),
            base_class: lowercase_crc32("BaseClass1"),
            components: lowercase_crc32("Components"),
            entities: lowercase_crc32("Entities"),
            trigger_name: lowercase_crc32("m_triggerName"),
            to_trigger: lowercase_crc32("m_entitiesToTrigger"),
            entity_ref: lowercase_crc32("m_entityRef"),
            entity_id: lowercase_crc32("EntityId"),
            event: lowercase_crc32("m_type"),
            recursive: lowercase_crc32("m_applyRecursively"),
            local_only: lowercase_crc32("m_localPlayerOnly"),
        }
    }
}

/// Every `TriggerEntityComponent` of the dynamic slice whose ObjectStream root is `root` (an
/// `AZ::Entity` holding the slice component), in stream order.
///
/// # Errors
///
/// Returns [`SliceTriggerError`] when the root holds no slice entities, or an entity or trigger
/// component lacks an id, a name or an entry field.
pub fn slice_triggers(root: &Element) -> Result<Vec<SliceTrigger>, SliceTriggerError> {
    let names = Names::new();
    let entities = root
        .child(names.components)
        .and_then(|components| {
            components
                .children
                .iter()
                .find_map(|component| component.child(names.entities))
        })
        .ok_or(SliceTriggerError::NoEntities)?;
    let Some(main) = entities.children.first() else {
        return Ok(Vec::new());
    };
    let main = entity_id(main, &names)?;
    let mut triggers = Vec::new();
    for entity in &entities.children {
        let owner = entity_id(entity, &names)?;
        let Some(components) = entity.child(names.components) else {
            continue;
        };
        for component in components
            .children
            .iter()
            .filter(|component| component.type_id == TRIGGER_ENTITY_COMPONENT_TYPE)
        {
            triggers.push(trigger(component, main, owner, &names)?);
        }
    }
    Ok(triggers)
}

/// The `Id` of slice entity `entity` (its `Id` element's `id`).
fn entity_id(entity: &Element, names: &Names) -> Result<u64, SliceTriggerError> {
    entity
        .child(names.id)
        .and_then(|id| id.child(names.id))
        .ok_or(SliceTriggerError::Missing("slice entity", "Id"))?
        .as_u64()
        .map_err(SliceTriggerError::from)
}

/// The component `Id` of `component`: the `Id` of its innermost base class (`AZ::Component`).
fn component_id(component: &Element, names: &Names) -> Result<u64, SliceTriggerError> {
    let mut class = component;
    loop {
        if let Some(id) = class.child(names.id) {
            return id.as_u64().map_err(SliceTriggerError::from);
        }
        class = class
            .child(names.base_class)
            .ok_or(SliceTriggerError::Missing("trigger component", "Id"))?;
    }
}

/// The trigger of `component`, owned by slice entity `owner` of the slice whose main entity is
/// `main`.
fn trigger(
    component: &Element,
    main: u64,
    owner: u64,
    names: &Names,
) -> Result<SliceTrigger, SliceTriggerError> {
    let name = component
        .child(names.trigger_name)
        .ok_or(SliceTriggerError::Missing(
            "trigger component",
            "m_triggerName",
        ))?
        .as_string()?
        .to_owned();
    let entities = match component.child(names.to_trigger) {
        Some(list) => list
            .children
            .iter()
            .map(|entry| triggered_entity(entry, names))
            .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };
    Ok(SliceTrigger {
        name,
        main,
        owner,
        component: component_id(component, names)?,
        entities,
    })
}

/// One `m_entitiesToTrigger` entry.
fn triggered_entity(entry: &Element, names: &Names) -> Result<TriggeredEntity, SliceTriggerError> {
    const ENTRY: &str = "trigger entry";
    let field = move |name_crc: u32, label: &'static str| {
        entry
            .child(name_crc)
            .ok_or(SliceTriggerError::Missing(ENTRY, label))
    };
    let entity = field(names.entity_ref, "m_entityRef")?
        .child(names.entity_id)
        .and_then(|id| id.child(names.id))
        .ok_or(SliceTriggerError::Missing(ENTRY, "EntityId"))?
        .as_u64()?;
    Ok(TriggeredEntity {
        entity,
        event: TriggerEvent::from(field(names.event, "m_type")?.as_u32()?),
        apply_recursively: field(names.recursive, "m_applyRecursively")?.as_bool()?,
        local_player_only: field(names.local_only, "m_localPlayerOnly")?.as_bool()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ids of `lost_thorpe_relic_spearbearerexists.dynamicslice` (game paks): the main entity,
    /// the child `TriggerEntities`, its `DeactivateVisibility` component and the first entity
    /// that component deactivates (`SpearMeshIntro`).
    const MAIN: u64 = 0xfb9b_4cbf_ddda_1913;
    const OWNER: u64 = 0xfb9b_4d4b_c015_fd41;
    const COMPONENT: u64 = 0x0db2_7d1c_ed36_c354;
    const SPEAR_MESH: u64 = 0xfb9b_4ce9_8b92_340e;
    const NAME: &str = "DeactivateVisibility";
    /// Another component type (any type but the trigger component).
    const OTHER_TYPE: [u8; UUID_SIZE] = [7; UUID_SIZE];
    const ENTITY_NAME: &str = "IoN_Lost_Thorpe_Relic_SpearBearerExists";

    fn named(name: &str, children: Vec<Element>) -> Element {
        Element::with_children(lowercase_crc32(name), children)
    }

    fn value(name: &str, bytes: &[u8]) -> Element {
        Element::with_value(lowercase_crc32(name), bytes.to_vec())
    }

    fn id_holder(name: &str, id: u64) -> Element {
        named(name, vec![value("id", &id.to_be_bytes())])
    }

    fn entity(id: u64, components: Vec<Element>) -> Element {
        named(
            "element",
            vec![
                id_holder("Id", id),
                value("Name", ENTITY_NAME.as_bytes()),
                named("Components", components),
            ],
        )
    }

    fn entry(target: u64, event: u32, local_only: bool) -> Element {
        named(
            "element",
            vec![
                named("m_entityRef", vec![id_holder("EntityId", target)]),
                value("m_type", &event.to_be_bytes()),
                value("m_applyRecursively", &[1]),
                value("m_localPlayerOnly", &[u8::from(local_only)]),
            ],
        )
    }

    fn trigger_component(id: u64, name: &str, entries: Vec<Element>) -> Element {
        let mut component = named(
            "element",
            vec![
                named(
                    "BaseClass1",
                    vec![
                        named("BaseClass1", vec![value("Id", &id.to_be_bytes())]),
                        named("m_clientFacetPtr", Vec::new()),
                    ],
                ),
                named("m_entitiesToTrigger", entries),
                value("m_triggerName", name.as_bytes()),
            ],
        );
        component.type_id = TRIGGER_ENTITY_COMPONENT_TYPE;
        component
    }

    fn other_component() -> Element {
        let mut component = named("element", vec![id_holder("BaseClass1", 1)]);
        component.type_id = OTHER_TYPE;
        component
    }

    fn slice(entities: Vec<Element>) -> Element {
        named(
            "root",
            vec![
                id_holder("Id", 1),
                named(
                    "Components",
                    vec![named("element", vec![named("Entities", entities)])],
                ),
            ],
        )
    }

    #[test]
    fn a_child_trigger_is_read_with_the_main_entity() {
        let root = slice(vec![
            entity(MAIN, vec![other_component()]),
            entity(
                OWNER,
                vec![
                    other_component(),
                    trigger_component(
                        COMPONENT,
                        NAME,
                        vec![entry(SPEAR_MESH, EVENT_DEACTIVATE, false)],
                    ),
                ],
            ),
        ]);
        let triggers = slice_triggers(&root).expect("triggers");
        assert_eq!(
            triggers,
            [SliceTrigger {
                name: NAME.to_owned(),
                main: MAIN,
                owner: OWNER,
                component: COMPONENT,
                entities: vec![TriggeredEntity {
                    entity: SPEAR_MESH,
                    event: TriggerEvent::Deactivate,
                    apply_recursively: true,
                    local_player_only: false,
                }],
            }]
        );
    }

    #[test]
    fn a_trigger_on_the_main_entity_has_itself_as_owner() {
        let root = slice(vec![entity(
            MAIN,
            vec![trigger_component(COMPONENT, NAME, Vec::new())],
        )]);
        let triggers = slice_triggers(&root).expect("triggers");
        assert_eq!(triggers.len(), 1);
        assert_eq!(triggers[0].main, triggers[0].owner);
        assert!(triggers[0].entities.is_empty());
    }

    #[test]
    fn event_types_keep_their_names_and_other_values() {
        assert_eq!(TriggerEvent::from(EVENT_TRIGGER), TriggerEvent::Trigger);
        assert_eq!(TriggerEvent::from(EVENT_UNTRIGGER), TriggerEvent::UnTrigger);
        assert_eq!(TriggerEvent::from(EVENT_ACTIVATE), TriggerEvent::Activate);
        assert_eq!(TriggerEvent::from(4), TriggerEvent::Unnamed(4));
    }

    #[test]
    fn a_slice_without_triggers_or_entities() {
        let empty = slice(vec![entity(MAIN, vec![other_component()])]);
        assert_eq!(slice_triggers(&empty), Ok(Vec::new()));
        assert_eq!(
            slice_triggers(&named("root", Vec::new())),
            Err(SliceTriggerError::NoEntities)
        );
    }

    #[test]
    fn a_trigger_without_a_name_is_an_error() {
        let mut component = trigger_component(COMPONENT, NAME, Vec::new());
        component
            .children
            .retain(|child| child.name_crc != Some(lowercase_crc32("m_triggerName")));
        let root = slice(vec![entity(MAIN, vec![component])]);
        assert_eq!(
            slice_triggers(&root),
            Err(SliceTriggerError::Missing(
                "trigger component",
                "m_triggerName"
            ))
        );
    }
}
