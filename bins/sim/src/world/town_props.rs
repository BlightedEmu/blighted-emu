// SPDX-License-Identifier: AGPL-3.0-or-later
//! The spawn groups of the settlement and fort props ([`nw_content::town_props`]): each prop
//! is an entity with a card of its slice and the states retail sent with it, at the place its
//! place state names. A prop comes into a view as a crafting station does; it has no station
//! type, so no recipe names it.

use nw_content::town_props::{TownProp, TownProps};
use nw_messages::{
    AssetId, BundleMember, DYNAMIC_SLICE_SUB_ID, GdeMetadataReplicatedState, MemberState,
};
use nw_sim_core::Position;

use super::crafting::{CARD_COMPONENT, Station, dynamic_ref};

/// Names the ids this server makes for props.
const PROP_NAMESPACE: u64 = 0x746f_776e_7072_6f70;

/// The props of `props` as stations. A prop whose states do not read or name no place is left
/// out: without a place it is in no view.
pub(super) fn stations(props: &TownProps) -> Vec<Station> {
    props
        .iter()
        .enumerate()
        .filter_map(|(index, prop)| station(index, prop))
        .collect()
}

fn station(index: usize, prop: &TownProp) -> Option<Station> {
    let states = prop
        .members
        .iter()
        .map(|bytes| BundleMember::read_from(bytes).map(|(member, _)| member))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let position = states.iter().find_map(|member| match &member.state {
        MemberState::Position(place) => place.position.map(|at| Position {
            x: at.x,
            y: at.y,
            z: at.z(),
        }),
        _ => None,
    })?;
    let gde_ref = dynamic_ref(PROP_NAMESPACE, index);
    let card = BundleMember {
        component: CARD_COMPONENT,
        state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
            asset: Some(AssetId {
                id: prop.asset,
                sub_id: DYNAMIC_SLICE_SUB_ID,
            }),
            gde_ref: Some(gde_ref),
        }),
    };
    Some(Station {
        gde_ref,
        position,
        station_type: String::new(),
        name: Vec::new(),
        members: std::iter::once(card).chain(states).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The carpentry building of a retail settlement and a prop without a place state.
    const TABLE: &str = r#"{"props": [
        {"asset": "007cf140474555d9aa1af9a817fc5443",
         "members": ["010d01034599065345d8bbc91f74037fff"]},
        {"asset": "007cf140474555d9aa1af9a817fc5443", "members": []}
    ]}"#;

    #[test]
    fn a_prop_is_a_card_of_its_slice_and_its_retail_states_at_its_place() {
        let props = TownProps::parse(TABLE).expect("table");
        let stations = stations(&props);
        assert_eq!(stations.len(), 1, "the prop without a place is left out");
        let prop = &stations[0];
        assert!((prop.position.x - 4896.79).abs() < 0.01);
        assert!((prop.position.y - 6935.47).abs() < 0.01);
        let hex: Vec<String> = prop
            .members
            .iter()
            .map(|member| hex::encode(member.encode().expect("encode")))
            .collect();
        assert!(hex[0].starts_with("000a0103007cf140474555d9aa1af9a817fc544300000002"));
        assert_eq!(hex[1], "010d01034599065345d8bbc91f74037fff");
        assert_eq!(nw_messages::as_dynamic_raw_id(prop.gde_ref), prop.gde_ref);
    }
}
