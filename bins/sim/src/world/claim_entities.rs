// SPDX-License-Identifier: AGPL-3.0-or-later
//! The land claim entity of each territory ([`nw_messages::LandClaimState`], class 62): the
//! level's claim plot capital at the territory's claim position, shown to every client in view
//! of it, with the company that owns the claim.
//!
//! Observed (every class-62 body of the capture index, 81 bodies in 22 ledgers; claims 2, 4, 6,
//! 10, 13 and 14): retail sends the entity's group whole each time it comes into a client's
//! view (`fort_capping_pvp_flagged` 1.932, 152.311, 303.249 and 456.005 s, each with a new
//! interest id), and never one of its states alone. The group: a card with the capital's
//! `GdeRef` only (a level entity), the ownership of the claim's company (its id, name and crest,
//! no owner player, permission rule [`OWNED_PERMISSION_RULE`], no structure name, raid 0, the
//! company's faction, no PvP or FFA flag, no option), the buildable controller built
//! ([`BUILT_STATE`], no foundation events sent, the committed resources of the slice's
//! blueprint: one `ClaimCharterT1`), the placement obstruction, the land claim state with the
//! territory id and the interact state off. The component numbers are the slice's
//! ([`nw_world::claim_plots`]).
//!
//! Inferred: a claim without an owner shows no company and the slice's own permission rule
//! (every retail claim had an owner). A change of owner reaches the clients that see the entity
//! as a change of its ownership alone, as a camp's change does ([`super::crafting::Stations`]);
//! no capture shows an owner change while the entity is in view. The placement obstruction is 0:
//! it was 0 in 19 of the 22 ledgers; the values 113 (`fort_capping_pvp_flagged`), 44
//! (`Playthrough_Part_03`) and 21 (`raid_group_influence_race`) have an **Unknown** meaning.

use nw_content::crafting::StructureCosts;
use nw_messages::buildables::{GuildCrest, OwnerIdentification};
use nw_messages::items::FIRST_VERSION;
use nw_messages::keyed_list::KeyedList;
use nw_messages::{
    BuildableControllerState, BundleMember, CLAIM_KEYS, ClaimOwner, GdeMetadataReplicatedState,
    InteractState, LandClaimState, MemberState, OwnershipState, PlacementObstructionState,
};
use nw_sim_core::Position;
use nw_world::claim_plots::PlacedClaimPlot;

use super::SimWorld;
use super::crafting::{
    CARD_COMPONENT, NO_FACTION, NO_GUILD_ID, NO_RAID, Station, Stations, committed, gde_ref_of,
};
use super::land_claims::{CLAIM_POSITIONS, Territories};

/// How far in the ground plane a claim plot capital may lie from its claim's position
/// (Observed: 0.003 to 0.004 m for the plots of claims 2 and 10).
const CLAIM_PLOT_REACH_M: f32 = 1.0;
/// The buildable state of every retail land claim entity: built.
const BUILT_STATE: u8 = 5;
/// The permission rule of an owned claim's entity (Observed in all 22 ledgers).
const OWNED_PERMISSION_RULE: u8 = 3;
/// The placement obstruction the entity shows (module doc).
const NO_OBSTRUCTION: u8 = 0;
/// The interact state of every retail land claim entity: off, no interactor count sent.
const INTERACT_OFF: InteractState = InteractState {
    enabled: Some(false),
    has_interactors: None,
};
/// The permission options of every retail land claim entity.
const NO_PERMISSION_OPTIONS: u8 = 0;

/// One territory's land claim entity in this sim's stations.
#[derive(Debug, Clone, PartialEq)]
struct ClaimEntity {
    /// The territory id.
    territory: u16,
    /// Its slot key in the stations.
    number: u64,
    /// The component of its ownership.
    ownership_component: u64,
    /// The slice's permission rule, shown while no company owns the claim.
    unowned_rule: u8,
    /// The owner it shows.
    shown: Option<ClaimOwner>,
}

/// The land claim entities of this sim.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct ClaimEntities {
    entities: Vec<ClaimEntity>,
}

/// The territory of the claim whose position is within [`CLAIM_PLOT_REACH_M`] of `position`.
fn territory_at(position: Position) -> Option<u16> {
    CLAIM_POSITIONS
        .iter()
        .zip(CLAIM_KEYS)
        .find(|([x, y, z], _)| {
            position.horizontal_distance(Position {
                x: *x,
                y: *y,
                z: *z,
            }) <= CLAIM_PLOT_REACH_M
        })
        .map(|(_, territory)| territory)
}

/// The ownership a claim entity shows for `owner`, or with no owner under `unowned_rule`.
fn claim_ownership(owner: Option<&ClaimOwner>, unowned_rule: u8) -> OwnershipState {
    OwnershipState {
        guild_id: Some(owner.map_or(NO_GUILD_ID, |owner| owner.guild_id)),
        guild_owner: Some(owner.map_or_else(Vec::new, |owner| owner.guild_name.clone())),
        guild_crest: Some(owner.map_or(GuildCrest::NONE, |owner| owner.crest)),
        owner: Some(OwnerIdentification::none()),
        permission_rule: Some(owner.map_or(unowned_rule, |_| OWNED_PERMISSION_RULE)),
        structure_name: Some(Vec::new()),
        raid_id: Some(NO_RAID),
        faction_type: Some(owner.map_or(NO_FACTION, |owner| owner.faction)),
        owner_pvp: Some(false),
        owner_ffa: Some(false),
        permission_options: Some(NO_PERMISSION_OPTIONS),
    }
}

/// The spawn members of the entity of `plot` for `territory`, whose blueprint costs `costs`
/// lists, with no owner, by component number.
fn claim_members(
    plot: &PlacedClaimPlot,
    territory: u16,
    costs: &StructureCosts,
) -> Vec<BundleMember> {
    let slice = &plot.slice;
    let member = |component: u8, state| BundleMember {
        component: u64::from(component),
        state,
    };
    let mut members = vec![
        BundleMember {
            component: CARD_COMPONENT,
            state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                asset: None,
                gde_ref: Some(gde_ref_of(plot.id)),
            }),
        },
        member(
            slice.ownership.0,
            MemberState::Ownership(claim_ownership(None, slice.ownership.1)),
        ),
        member(
            slice.buildable,
            MemberState::BuildableController(BuildableControllerState {
                current_state: Some(BUILT_STATE),
                committed_resources: Some(KeyedList::full(
                    FIRST_VERSION,
                    committed(costs.of(&slice.structure)),
                )),
                ..BuildableControllerState::default()
            }),
        ),
        member(
            slice.obstruction,
            MemberState::PlacementObstruction(PlacementObstructionState {
                has_completion_obstruction: Some(NO_OBSTRUCTION),
            }),
        ),
        member(
            slice.land_claim,
            MemberState::LandClaim(LandClaimState {
                claim: Some(territory),
            }),
        ),
        member(slice.interact, MemberState::Interact(INTERACT_OFF)),
    ];
    members.sort_by_key(|member| member.component);
    members
}

impl ClaimEntities {
    /// Adds the land claim entity of each claim whose plot is among `plots` to `stations`; a
    /// plot at no claim's position is left out.
    pub(super) fn place(
        stations: &mut Stations,
        plots: &[PlacedClaimPlot],
        costs: &StructureCosts,
    ) -> Self {
        let mut entities = Vec::new();
        for plot in plots {
            let [x, y, z] = plot.position;
            let position = Position { x, y, z };
            let Some(territory) = territory_at(position) else {
                continue;
            };
            let number = stations.add_level_entity(Station {
                gde_ref: gde_ref_of(plot.id),
                position,
                station_type: String::new(),
                name: Vec::new(),
                members: claim_members(plot, territory, costs),
            });
            entities.push(ClaimEntity {
                territory,
                number,
                ownership_component: u64::from(plot.slice.ownership.0),
                unowned_rule: plot.slice.ownership.1,
                shown: None,
            });
        }
        Self { entities }
    }

    /// Shows each entity the owner of its claim in `territories`: an entity whose owner changed
    /// gets its new ownership, in its spawn members and as a change to the clients that see it.
    pub(super) fn show_owners(&mut self, stations: &mut Stations, territories: &Territories) {
        for entity in &mut self.entities {
            let owner = territories.owner(entity.territory);
            if entity.shown.as_ref() == owner {
                continue;
            }
            entity.shown = owner.cloned();
            let ownership = BundleMember {
                component: entity.ownership_component,
                state: MemberState::Ownership(claim_ownership(owner, entity.unowned_rule)),
            };
            let merged = ownership.clone();
            stations.change_camp(
                entity.number,
                |members| {
                    members
                        .iter_mut()
                        .filter(|member| member.component == merged.component)
                        .for_each(|member| *member = merged.clone());
                },
                vec![ownership],
            );
        }
    }

    /// The territories that have a land claim entity here.
    #[cfg(test)]
    fn territories(&self) -> Vec<u16> {
        self.entities
            .iter()
            .map(|entity| entity.territory)
            .collect()
    }
}

impl SimWorld {
    /// Shows the land claim entities the owners this sim read last.
    pub(super) fn show_claim_owners(&mut self) {
        self.claim_entities
            .show_owners(&mut self.stations, &self.territories);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nw_content::town_props::TownProps;
    use nw_world::claim_plots::ClaimPlotSlice;

    use crate::world::slots::Slot;

    /// Claim 10's plot as retail and the pak show it (`fort_capping_pvp_flagged` 1.932 s: key
    /// `0x5136d0d76253457b`, components 1, 2, 5, 7 and 8; slice rule 1, blueprint `ClaimT1_30m`).
    fn claim_10_plot() -> PlacedClaimPlot {
        PlacedClaimPlot {
            id: 0x5136_d0d7_6253_457b,
            position: [6851.21, 1330.68, 62.92],
            slice: ClaimPlotSlice {
                slice: "slices/darkness/zone_capitalplot_windswardshores.dynamicslice".into(),
                ownership: (1, 1),
                buildable: 2,
                structure: "ClaimT1_30m".into(),
                obstruction: 5,
                land_claim: 7,
                interact: 8,
            },
        }
    }

    const COMPANY_NAME: &str = "Made Up Company";
    const COMPANY_FACTION: u8 = 2;

    fn owner() -> nw_db::TerritoryOwner {
        nw_db::TerritoryOwner {
            territory: 10,
            company: uuid::Uuid::from_u128(0x1111),
            name: COMPANY_NAME.to_owned(),
            faction: COMPANY_FACTION,
            crest: [0; nw_db::CREST_LEN],
            since_ms: 1,
            upkeep_due_ms: 2,
        }
    }

    fn placed() -> (Stations, ClaimEntities) {
        let mut stations = Stations::build(&[], &StructureCosts::default(), &TownProps::default());
        let far = PlacedClaimPlot {
            position: [0.0, 0.0, 0.0],
            ..claim_10_plot()
        };
        let entities = ClaimEntities::place(
            &mut stations,
            &[claim_10_plot(), far],
            &StructureCosts::default(),
        );
        (stations, entities)
    }

    fn ownership_of(members: &[BundleMember]) -> OwnershipState {
        members
            .iter()
            .find_map(|member| match &member.state {
                MemberState::Ownership(state) => Some(state.clone()),
                _ => None,
            })
            .expect("an ownership")
    }

    #[test]
    fn the_claim_entity_spawns_with_the_retail_group_of_its_plot() {
        let (stations, entities) = placed();
        assert_eq!(
            entities.territories(),
            [10],
            "the plot at no claim is left out"
        );
        let number = entities.entities[0].number;
        let members = stations.members(number).expect("a station");
        let components: Vec<u64> = members.iter().map(|member| member.component).collect();
        assert_eq!(components, [CARD_COMPONENT, 1, 2, 5, 7, 8]);
        assert_eq!(
            members[0].state,
            MemberState::GdeMetadata(GdeMetadataReplicatedState {
                asset: None,
                gde_ref: Some([
                    0x7b, 0x45, 0x53, 0x62, 0xd7, 0xd0, 0x36, 0x51, 0, 0, 0, 0, 0, 0, 0, 0
                ]),
            }),
            "the retail card's GdeRef"
        );
        assert_eq!(
            members[4].state,
            MemberState::LandClaim(LandClaimState { claim: Some(10) })
        );
        assert_eq!(members[5].state, MemberState::Interact(INTERACT_OFF));
        let MemberState::BuildableController(built) = &members[2].state else {
            panic!("buildable");
        };
        assert_eq!(built.current_state, Some(BUILT_STATE));
        assert_eq!(built.check_foundation_events, None);
        let unowned = ownership_of(members);
        assert_eq!(unowned.guild_id, Some(NO_GUILD_ID));
        assert_eq!(unowned.permission_rule, Some(1), "the slice's rule");
        let near = Position {
            x: 6860.0,
            y: 1330.0,
            z: 63.0,
        };
        assert!(stations.numbers_near(near, 50.0).any(|seen| seen == number));
    }

    #[test]
    fn an_owner_change_goes_to_the_viewers_and_into_the_spawn_members() {
        let (mut stations, mut entities) = placed();
        let number = entities.entities[0].number;
        let mut territories = Territories::default();
        entities.show_owners(&mut stations, &territories);
        let unchanged = Slot {
            interest: 9,
            new: false,
        };
        assert_eq!(
            stations.group(number, unchanged),
            None,
            "no owner: no change"
        );
        territories.read_again_for_test(vec![owner()]);
        entities.show_owners(&mut stations, &territories);
        let change = stations.group(number, unchanged).expect("the change");
        assert_eq!(change.members.len(), 1, "the ownership alone");
        let owned = ownership_of(&change.members);
        assert_eq!(owned.guild_id, Some(*owner().company.as_bytes()));
        assert_eq!(owned.guild_owner, Some(COMPANY_NAME.as_bytes().to_vec()));
        assert_eq!(owned.permission_rule, Some(OWNED_PERMISSION_RULE));
        assert_eq!(owned.faction_type, Some(COMPANY_FACTION));
        assert_eq!(owned.owner, Some(OwnerIdentification::none()));
        assert_eq!(
            ownership_of(stations.members(number).expect("a station")),
            owned,
            "a later viewer gets the owner in the spawn group"
        );
    }
}
