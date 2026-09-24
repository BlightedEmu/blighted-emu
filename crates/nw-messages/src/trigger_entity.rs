// SPDX-License-Identifier: AGPL-3.0-or-later
//! `TriggerEntityComponentClientFacet_SyncExecuteEntities` (type 3135): the server runs one
//! `TriggerEntityComponent` of a creature's slice on the client (oquests triggers note).
//!
//! The body is the 16-byte facet header and nothing else (Observed: 73,038 retail bodies of 176
//! sessions, all 16 bytes). The header names the creature (its facet entity) and the component:
//! a slice holds one `TriggerEntityComponent` per trigger name (`m_triggerName`), on a child
//! entity of the slice, and the component lists the slice entities it runs
//! (`m_entitiesToTrigger`: entity, `EventType`, apply recursively, local player only). The
//! target is `(entity XOR (main entity Id XOR owner entity Id)) + component Id`, with the slice's
//! first entity as the main entity (Observed: 65,813 of 65,813 retail calls whose creature's
//! slice was extracted).
//!
//! The server sends it when the creature's action list or behavior tree runs a `TriggerEntity`
//! activity or node with that trigger name (Inferred: the names of the calls are the
//! `TriggerName`s of the `.actionlist` and `.behaviortree` files of the slices).

use crate::facet::{FacetHeader, facet_entity, header_only_message};
use crate::wire::{BodyCodec, MessageError, Reader, Writer};

/// Type index of `TriggerEntityComponentClientFacet_SyncExecuteEntities`.
pub const SYNC_EXECUTE_ENTITIES_TYPE_INDEX: u64 = 3135;

/// Where one `TriggerEntityComponent` sits in its slice: what a call to it on an instance of the
/// slice needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerAddress {
    /// The XOR of the slice's main entity `Id` and the `Id` of the entity that owns the
    /// component (0 for a component on the main entity).
    pub child: u64,
    /// The component's `Id`.
    pub component: u64,
}

impl TriggerAddress {
    /// The address of component `component` of slice entity `owner`, in a slice whose main
    /// (first) entity is `main`.
    pub const fn in_slice(main: u64, owner: u64, component: u64) -> Self {
        Self {
            child: main ^ owner,
            component,
        }
    }

    /// The target id of the component on the instance whose facet entity is `entity`.
    pub const fn target(&self, entity: u64) -> u64 {
        (entity ^ self.child).wrapping_add(self.component)
    }
}

header_only_message!(
    /// 3135: the client runs the entities of the `TriggerEntityComponent` that the header
    /// names, on the creature that the header's entity names.
    SyncExecuteEntitiesMsg,
    SYNC_EXECUTE_ENTITIES_TYPE_INDEX
);

impl SyncExecuteEntitiesMsg {
    /// The call that runs the trigger at `address` on the creature of entity number
    /// `entity_number`.
    pub fn of(address: TriggerAddress, entity_number: u64) -> Self {
        let entity = facet_entity(entity_number);
        Self {
            header: FacetHeader {
                target: address.target(entity),
                entity,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The main entity of `lost_thorpe_relic_spearbearerexists.dynamicslice` (game paks).
    const SPEARBEARER_MAIN: u64 = 0xfb9b_4cbf_ddda_1913;
    /// Its child entity `TriggerEntities`, which owns the slice's trigger components.
    const SPEARBEARER_TRIGGERS: u64 = 0xfb9b_4d4b_c015_fd41;
    /// The `DeactivateVisibility` trigger component of that child.
    const DEACTIVATE_VISIBILITY: u64 = 0x0db2_7d1c_ed36_c354;
    /// A synthetic facet entity in the retail form.
    const ENTITY: u64 = 0x0123_4567_89ab_cdef;
    /// [`ENTITY`]'s target of [`DEACTIVATE_VISIBILITY`]: (entity XOR child) + component, worked
    /// out by hand.
    const ENTITY_TARGET: u64 = 0x0ed5_c1b0_819a_ed11;
    const ENTITY_NUMBER: u64 = 9;
    /// The retail body length: the facet header alone.
    const BODY_LEN: usize = 16;

    fn spearbearer() -> TriggerAddress {
        TriggerAddress::in_slice(SPEARBEARER_MAIN, SPEARBEARER_TRIGGERS, DEACTIVATE_VISIBILITY)
    }

    #[test]
    fn the_address_xors_the_main_and_owner_ids() {
        assert_eq!(spearbearer().child, 0x0000_01f4_1dcf_e452);
        assert_eq!(spearbearer().target(ENTITY), ENTITY_TARGET);
        let on_main = TriggerAddress::in_slice(SPEARBEARER_MAIN, SPEARBEARER_MAIN, 1);
        assert_eq!(on_main.target(ENTITY), ENTITY + 1);
    }

    #[test]
    fn the_call_names_the_creature_and_its_component() {
        let message = SyncExecuteEntitiesMsg::of(spearbearer(), ENTITY_NUMBER);
        let entity = facet_entity(ENTITY_NUMBER);
        assert_eq!(message.header.entity, entity);
        assert_eq!(message.header.target, spearbearer().target(entity));
    }

    #[test]
    fn the_body_is_the_header_alone() {
        let message = SyncExecuteEntitiesMsg {
            header: FacetHeader {
                target: ENTITY_TARGET,
                entity: ENTITY,
            },
        };
        let body = message.encode().expect("encode");
        assert_eq!(body.len(), BODY_LEN);
        assert_eq!(body[..8], ENTITY_TARGET.to_be_bytes());
        assert_eq!(body[8..], ENTITY.to_be_bytes());
        assert_eq!(SyncExecuteEntitiesMsg::decode(&body), Ok(message));
    }

    #[test]
    fn the_server_message_decodes_by_its_index() {
        let message =
            crate::ServerMessage::SyncExecuteEntities(SyncExecuteEntitiesMsg::of(spearbearer(), 1));
        assert_eq!(message.type_index(), SYNC_EXECUTE_ENTITIES_TYPE_INDEX);
        let body = message.encode().expect("encode");
        assert_eq!(
            crate::ServerMessage::decode(SYNC_EXECUTE_ENTITIES_TYPE_INDEX, &body),
            Some(Ok(message))
        );
    }

    #[test]
    fn a_body_with_more_or_fewer_bytes_does_not_decode() {
        let body = SyncExecuteEntitiesMsg::of(spearbearer(), ENTITY_NUMBER)
            .encode()
            .expect("encode");
        let mut longer = body.clone();
        longer.push(0);
        assert_eq!(
            SyncExecuteEntitiesMsg::decode(&longer),
            Err(MessageError::TrailingBytes { left: 1 })
        );
        assert!(matches!(
            SyncExecuteEntitiesMsg::decode(&body[..BODY_LEN - 1]),
            Err(MessageError::Truncated { .. })
        ));
    }
}
