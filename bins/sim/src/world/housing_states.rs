// SPDX-License-Identifier: AGPL-3.0-or-later
//! The replicated housing states (housing states study 2026-11-11,
//! `local-data/findings/2026-11-11-housing-states.md`): the house data entity of each house with
//! its furniture (3663), the houses of a plot (3055), and what the housing state (2768) says of
//! each owned house: its trophies, its decoration score and whether its buffs are on.
//!
//! Retail, Observed:
//! - A house data entity is a spawn group of `slices/housing/housedatamaster.dynamicslice`: the
//!   card with a dynamic `GdeRef`, the furniture list (component 1) and the plot's position and
//!   rotation (component 2; the same bytes as the plot's own position in `enter_into_a_disabled
//!   _house`). The housing state names it in each owned house and, inside it, as the phased house
//!   data entity. Each player got the entities of its own houses near them (`Hive house plot`: two
//!   houses, two entities) and the top house of each plot it passed.
//! - A furniture change (3277, 3723, 1435) changes the list ([`nw_messages::house_data_state`])
//!   and the owned house's decoration score in the housing state; a use of a piece (3730) sets
//!   its use flag and its end (695) clears it.
//! - A house whose taxes are overdue has its buffs off, no trophies and decoration score 0 in the
//!   housing state (`login_into_an_expired_upkeep_house`: all three overdue houses); the house
//!   info (3692) keeps its score (`enter_into_a_disabled_house`: 89).
//! - The trophies are the CRC-32 of the lowercase `HouseItemID` of the placed items that have a
//!   `HousingStatusEffect` (every trophy of the 800 retail housing states is one).
//! - A plot's houses (3055) came on the plot entity after the plot info request (3717).
//!
//! The trophies' buffs (status effect bonuses study 2026-11-11): the player holds the
//! `HousingStatusEffect` of each trophy of each of its houses whose taxes are paid, one more
//! stack (here: potency) for each copy, at most the effect's `StackMax` (`HousingRollBonus*` 3,
//! `HousingMinGearBonus_AllCraftingT3` 5), wherever it is (the effects have
//! `PersistEffectInCrossWorldInstances`). Inferred: the housing state's `buffItemsActive` is what
//! turns them on and off (Observed: false with no trophies on the three overdue houses of
//! `login_into_an_expired_upkeep_house`); when retail adds them is Unknown (the housing ledgers
//! are of an older build whose status effect keys do not read, and no other ledger holds a
//! `Housing*` effect).
//!
//! Here, Inferred: a house data entity stands while its owner is in this sim (as a camp does), so
//! a visitor ("house guest") can enter only the house of an owner who is in the same sim, and a
//! plot lists the houses of the players of this sim; its order of trophies is the item order
//! (retail's is Unknown); a house data entity of a plot without a retail position has the home's
//! position and no rotation.

use bevy_ecs::entity::Entity;
use nw_bus::{HousingItem, OwnedHome, Point};
use nw_content::status_effects::StatusEffectRules;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::house_data_state::{HouseDataState, HousingItemEntry};
use nw_messages::housing_plot_state::{
    HOUSING_PLOT_COMPONENT, HousingPlotState, MOST_PLOT_HOUSES, PlotHouse,
};
use nw_messages::items::{FIRST_VERSION, FullList};
use nw_messages::position::{PositionInTheWorldReplicatedState, WorldPosition};
use nw_messages::{
    AssetId, BundleMember, DYNAMIC_SLICE_SUB_ID, GdeMetadataReplicatedState, ListChange,
    MemberState, NS_PER_MS, RAW_ID_LEN, ReplicatedList, as_dynamic_raw_id,
};
use nw_sim_core::Position;
use uuid::Uuid;

use crate::travel_rules::{HousingRules, TravelRules};

use super::crafting::{CARD_COMPONENT, Station};
use super::gates::merge_members;
use super::travel::id_of_raw;
use super::{Player, SimWorld, unix_ns};

/// The slice asset of a house data entity (`slices/housing/housedatamaster.dynamicslice`;
/// Observed on all 330 retail house data cards).
const HOUSE_DATA_ASSET: [u8; RAW_ID_LEN] = [
    0x90, 0x1e, 0x86, 0xd0, 0x93, 0xa2, 0x5f, 0x4b, 0xbf, 0xe7, 0x6f, 0x0b, 0x3c, 0xef, 0x35, 0x4b,
];
/// The components of a house data entity's furniture list and position (Observed: 330 and 283
/// retail members).
const HOUSE_DATA_COMPONENT: u64 = 1;
const HOUSE_POSITION_COMPONENT: u64 = 2;
/// Names the house data ids this server makes.
const HOUSE_DATA_NAMESPACE: u64 = 0x686f_7573_6564_6174;
/// No house data id yet (homes bought before this server made them).
const NO_HOUSE_DATA: [u8; RAW_ID_LEN] = [0; RAW_ID_LEN];
/// A taxes due time of 0: none known, the house counts as paid (Inferred).
const NO_DUE_TIME: u64 = 0;

/// The house data entity of one of a player's houses.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HouseEntity {
    /// The house's plot.
    plot: [u8; RAW_ID_LEN],
    /// Its slot key ([`super::crafting::Stations`]).
    number: u64,
    /// The furniture list its viewers have.
    sent: Vec<HousingItemEntry>,
    /// The list's version.
    version: u64,
    /// Whether its taxes were paid at the last look.
    active: bool,
}

/// A visit of a house of another player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Visit {
    /// The owner's character id, hyphenated text.
    pub(super) owner: Vec<u8>,
    /// The plot's facet entity.
    pub(super) plot_facet: u64,
    /// The house data entity's facet entity.
    pub(super) house_data_facet: u64,
}

/// A player's housing states: the house data entities of its houses, the house of another
/// player it visits and the piece of furniture it uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct HouseViews {
    entities: Vec<HouseEntity>,
    visit: Option<Visit>,
    in_use: Option<u16>,
}

impl HouseViews {
    /// The house of another player it visits.
    pub(super) const fn visit(&self) -> Option<&Visit> {
        self.visit.as_ref()
    }
}

/// What the housing state says of one owned house.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct HouseDecoration {
    /// Whether its taxes are paid: its buffs are on.
    pub(super) active: bool,
    /// Its trophies.
    pub(super) trophies: Vec<u32>,
    /// Its decoration score.
    pub(super) score: f32,
}

/// Whether the taxes of a house due at `due_ms` are paid at `unix_ms`.
const fn taxes_paid(due_ms: u64, unix_ms: u64) -> bool {
    due_ms == NO_DUE_TIME || due_ms > unix_ms
}

/// The `RankingPoints` of the furniture `items`: each item counts at most its
/// `RankingPointsDuplicateLimit` times (Inferred: retail added an item's points for its first
/// copy, `20261010-125035-658` 1791629657892 +0.12; the rule for further copies is Unknown).
pub(super) fn decoration_points(items: &[HousingItem], rules: &TravelRules) -> f64 {
    let mut counted: Vec<(u32, u32)> = Vec::new();
    items
        .iter()
        .filter_map(|placed| Some((placed.item, rules.furniture(placed.item)?)))
        .filter(|(item, furniture)| {
            let count = match counted.iter_mut().find(|(seen, _)| seen == item) {
                Some((_, count)) => {
                    *count += 1;
                    *count
                }
                None => {
                    counted.push((*item, 1));
                    1
                }
            };
            furniture.duplicate_limit.is_none_or(|limit| count <= limit)
        })
        .map(|(_, furniture)| furniture.ranking_points)
        .sum()
}

/// The decoration of `home` at `unix_ms`: an overdue house has its buffs off, no trophies and
/// score 0.
pub(super) fn house_decoration(
    home: &OwnedHome,
    rules: &TravelRules,
    unix_ms: u64,
) -> HouseDecoration {
    if !taxes_paid(home.record.taxes_due_ms, unix_ms) {
        return HouseDecoration {
            active: false,
            trophies: Vec::new(),
            score: 0.0,
        };
    }
    let trophies = home
        .items
        .iter()
        .filter_map(|placed| rules.furniture(placed.item))
        .filter(|furniture| furniture.trophy_effect.is_some())
        .map(|furniture| lowercase_crc32(&furniture.item_id))
        .collect();
    HouseDecoration {
        active: true,
        trophies,
        // The score is a few points: it fits f32.
        score: decoration_points(&home.items, rules) as f32,
    }
}

/// The trophy effects of the houses `homes` at `unix_ms` (effect, copies), by effect: the
/// `HousingStatusEffect` of each trophy of each house whose taxes are paid, one for each copy, at
/// most the effect's `StackMax` in `effects`; a trophy whose effect `effects` does not have is
/// left out.
pub(super) fn trophy_effects(
    homes: &[OwnedHome],
    rules: &TravelRules,
    effects: &StatusEffectRules,
    unix_ms: u64,
) -> Vec<(u32, f64)> {
    let mut copies: Vec<(u32, u8)> = Vec::new();
    let trophies = homes
        .iter()
        .filter(|home| taxes_paid(home.record.taxes_due_ms, unix_ms))
        .flat_map(|home| &home.items)
        .filter_map(|placed| rules.furniture(placed.item)?.trophy_effect.as_deref())
        .filter_map(|id| effects.by_id(id));
    for rule in trophies {
        match copies.iter_mut().find(|(crc, _)| *crc == rule.crc) {
            Some((_, count)) => *count = count.saturating_add(1).min(rule.stack_max),
            None => copies.push((rule.crc, 1)),
        }
    }
    copies.sort_unstable_by_key(|(crc, _)| *crc);
    copies
        .into_iter()
        .map(|(crc, count)| (crc, f64::from(count)))
        .collect()
}

/// The furniture list of `items`, at least `len` long: each item at its index, an index without
/// an item empty; the item at `in_use` is in use.
fn house_entries(items: &[HousingItem], len: usize, in_use: Option<u16>) -> Vec<HousingItemEntry> {
    let top = items
        .iter()
        .map(|placed| usize::from(placed.index) + 1)
        .max()
        .unwrap_or_default();
    let mut entries = vec![HousingItemEntry::EMPTY; top.max(len)];
    for placed in items {
        let Point { x, y, z } = placed.position.point();
        let mut entry =
            HousingItemEntry::placed([x, y, z], placed.rotation.map(f32::from_bits), placed.item);
        entry.in_use = in_use == Some(placed.index);
        entries[usize::from(placed.index)] = entry;
    }
    entries
}

/// The changes from `sent` to `now` at `version`: a changed index, and a new index first empty
/// and then with its item (Observed: see the module doc); the first change carries the
/// version, the others 0 (Observed: every retail delta).
fn item_changes(
    sent: &[HousingItemEntry],
    now: &[HousingItemEntry],
    version: u64,
) -> Vec<ListChange<HousingItemEntry>> {
    let mut changes = Vec::new();
    for (index, entry) in (0_u32..).zip(now) {
        match usize::try_from(index).ok().and_then(|at| sent.get(at)) {
            Some(before) if before == entry => {}
            Some(_) => changes.push((index, *entry)),
            None => {
                changes.push((index, HousingItemEntry::EMPTY));
                changes.push((index, *entry));
            }
        }
    }
    changes
        .into_iter()
        .enumerate()
        .map(|(position, (index, value))| ListChange {
            index,
            version: if position == 0 { version } else { 0 },
            value,
        })
        .collect()
}

/// The house data member of the list `items`.
fn house_data_member(items: ReplicatedList<HousingItemEntry>) -> BundleMember {
    BundleMember {
        component: HOUSE_DATA_COMPONENT,
        state: MemberState::HouseData(HouseDataState { items: Some(items) }),
    }
}

/// The whole list `entries` at `version`.
fn full_list(entries: &[HousingItemEntry], version: u64) -> ReplicatedList<HousingItemEntry> {
    ReplicatedList::Full(FullList {
        version: Some(version),
        elements: entries.to_vec(),
    })
}

/// The house data id of the house of `character` on `plot`: the same on every start, in the
/// dynamic form.
fn house_data_id(character: Uuid, plot: &[u8; RAW_ID_LEN]) -> [u8; RAW_ID_LEN] {
    let namespace = Uuid::from_u64_pair(HOUSE_DATA_NAMESPACE, 0);
    let mut name = character.as_bytes().to_vec();
    name.extend_from_slice(plot);
    as_dynamic_raw_id(*Uuid::new_v5(&namespace, &name).as_bytes())
}

/// Gives each house of `player` without a house data id its id.
fn mint_house_data(player: &mut Player) {
    let character = player.character;
    for home in player.travel.homes_mut() {
        if home.record.house_data == NO_HOUSE_DATA {
            home.record.house_data = house_data_id(character, &home.plot);
            home.record.house_data_facet = id_of_raw(&home.record.house_data);
        }
    }
}

/// The character id of `character` as the housing states name it.
fn owner_text(character: Uuid) -> Vec<u8> {
    character.hyphenated().to_string().into_bytes()
}

impl SimWorld {
    /// The house data entities of every player's houses: new houses get theirs, changed
    /// furniture goes out, abandoned houses lose theirs; a house whose taxes fall overdue sends
    /// the housing state again.
    pub(super) fn tick_housing_states(&mut self) {
        if self.is_tutorial() {
            return;
        }
        let unix_ms = unix_ns(self.server_ms) / NS_PER_MS;
        let server_ms = self.server_ms;
        let content = std::sync::Arc::clone(&self.content);
        let (travel_rules, effects) = (&content.travel, &content.status_effects);
        let stations = &mut self.stations;
        let mut query = self.world.query::<&mut Player>();
        for mut player in query.iter_mut(&mut self.world) {
            if !player.own_card_sent {
                continue;
            }
            mint_house_data(&mut player);
            let inside = player.travel.inside();
            let travel = &mut player.travel;
            let mut views = std::mem::take(&mut travel.houses);
            let homes = travel.homes();
            views.entities.retain(|house| {
                let kept = homes.iter().any(|home| home.plot == house.plot);
                if !kept {
                    stations.remove_camp(house.number);
                }
                kept
            });
            let mut overdue_changed = false;
            for home in homes {
                let in_use = views.in_use.filter(|_| inside == Some(home.plot));
                let active = taxes_paid(home.record.taxes_due_ms, unix_ms);
                match views
                    .entities
                    .iter_mut()
                    .find(|house| house.plot == home.plot)
                {
                    Some(house) => {
                        let now = house_entries(&home.items, house.sent.len(), in_use);
                        if now != house.sent {
                            house.version = house.version.saturating_add(1);
                            let changes = item_changes(&house.sent, &now, house.version);
                            let whole = house_data_member(full_list(&now, house.version));
                            stations.change_camp(
                                house.number,
                                |members| merge_members(members, &[whole]),
                                vec![house_data_member(ReplicatedList::Changes(changes))],
                            );
                            house.sent = now;
                        }
                        overdue_changed |= house.active != active;
                        house.active = active;
                    }
                    None => {
                        let sent = house_entries(&home.items, 0, in_use);
                        let place = plot_place(stations, home);
                        let gde_ref = home.record.house_data;
                        let members = vec![
                            BundleMember {
                                component: CARD_COMPONENT,
                                state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                                    asset: Some(AssetId {
                                        id: HOUSE_DATA_ASSET,
                                        sub_id: DYNAMIC_SLICE_SUB_ID,
                                    }),
                                    gde_ref: Some(gde_ref),
                                }),
                            },
                            house_data_member(full_list(&sent, FIRST_VERSION)),
                            BundleMember {
                                component: HOUSE_POSITION_COMPONENT,
                                state: MemberState::Position(place),
                            },
                        ];
                        let position = place.position.map_or_else(
                            || {
                                let Point { x, y, z } = home.position.point();
                                Position { x, y, z }
                            },
                            |at| Position {
                                x: at.x,
                                y: at.y,
                                z: at.z(),
                            },
                        );
                        let number = stations.place_camp(|_| Station {
                            gde_ref,
                            position,
                            station_type: String::new(),
                            name: Vec::new(),
                            members,
                        });
                        views.entities.push(HouseEntity {
                            plot: home.plot,
                            number,
                            sent,
                            version: FIRST_VERSION,
                            active,
                        });
                    }
                }
            }
            travel.houses = views;
            if overdue_changed {
                travel.mark_housing_due();
            }
            let trophies = trophy_effects(player.travel.homes(), travel_rules, effects, unix_ms);
            let number = player.number;
            player
                .status_effects
                .set_trophy_effects(effects, trophies, number, server_ms);
        }
    }

    /// Takes the house data entities of player `entity` out of the world.
    pub(super) fn despawn_houses(&mut self, entity: Entity) {
        let Some(houses) = self
            .world
            .get_mut::<Player>(entity)
            .map(|mut player| std::mem::take(&mut player.travel.houses.entities))
        else {
            return;
        };
        for house in houses {
            self.stations.remove_camp(house.number);
        }
    }

    /// Player `entity` enters the house whose house data entity is `facet` (544): its own, or
    /// the house of another player of this sim, which it then visits. A house no player here
    /// owns is no visit.
    pub(super) fn note_house_visit(&mut self, entity: Entity, facet: u64) {
        let own = self.world.get::<Player>(entity).is_some_and(|player| {
            player
                .travel
                .homes()
                .iter()
                .any(|home| home.record.house_data_facet == facet)
        });
        let visit = if own {
            None
        } else {
            let mut query = self.world.query::<&Player>();
            query.iter(&self.world).find_map(|owner| {
                let home = owner
                    .travel
                    .homes()
                    .iter()
                    .find(|home| home.record.house_data_facet == facet)?;
                Some(Visit {
                    owner: owner_text(owner.character),
                    plot_facet: id_of_raw(&home.plot),
                    house_data_facet: facet,
                })
            })
        };
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.travel.houses.visit = visit;
        }
    }

    /// Player `entity` uses the piece of furniture `index` of the house it is in (3730), or
    /// stops (695, `None`).
    pub(super) fn use_housing_item(&mut self, entity: Entity, index: Option<u16>) {
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.travel.houses.in_use = index;
        }
    }

    /// The houses of the plot of player `entity`'s last housing interaction go to the plot
    /// entity (3055): the houses of this sim's players on it, best first, at most
    /// [`MOST_PLOT_HOUSES`]. A plot that is no spawned prop has no entity to carry them.
    pub(super) fn show_plot_houses(&mut self, entity: Entity) {
        let Some(plot) = self.world.get_mut::<Player>(entity).and_then(|mut player| {
            mint_house_data(&mut player);
            player.travel.plot()
        }) else {
            return;
        };
        let Some(number) = self.stations.number_of(&plot) else {
            return;
        };
        let rules = &self.content.travel;
        let mut query = self.world.query::<&Player>();
        let mut houses: Vec<PlotHouse> = query
            .iter(&self.world)
            .flat_map(|owner| {
                owner
                    .travel
                    .homes()
                    .iter()
                    .filter(move |home| home.plot == plot)
                    .map(move |home| PlotHouse {
                        owner: owner_text(owner.character),
                        score: HousingRules::house_score(decoration_points(&home.items, rules)),
                        house_data: home.record.house_data,
                        house_data_facet: home.record.house_data_facet,
                    })
            })
            .collect();
        houses.sort_by_key(|house| std::cmp::Reverse(house.score));
        houses.truncate(MOST_PLOT_HOUSES);
        let member = BundleMember {
            component: HOUSING_PLOT_COMPONENT,
            state: MemberState::HousingPlot(HousingPlotState {
                houses: Some(houses),
            }),
        };
        self.stations.change_camp(
            number,
            |members| merge_members(members, std::slice::from_ref(&member)),
            vec![member.clone()],
        );
    }
}

/// Where the house data entity of `home` stands: the position and rotation of its plot's
/// spawned prop (Observed: the same as the plot's), else the home's position.
fn plot_place(
    stations: &super::crafting::Stations,
    home: &OwnedHome,
) -> PositionInTheWorldReplicatedState {
    let prop = stations
        .number_of(&home.plot)
        .and_then(|number| stations.members(number))
        .and_then(|members| {
            members.iter().find_map(|member| match &member.state {
                MemberState::Position(place) => Some(*place),
                _ => None,
            })
        });
    prop.unwrap_or_else(|| {
        let Point { x, y, z } = home.position.point();
        PositionInTheWorldReplicatedState {
            position: WorldPosition::from_metres(x, y, z),
            rotation: None,
            scale: None,
        }
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use nw_bus::{HomeRecord, StoredPoint};
    use nw_messages::house_data_state::PackedRotation;

    use super::*;
    use crate::travel_rules::Furniture;

    /// Made-up client item indexes: a chair, a trophy and an item no row names.
    const CHAIR: u32 = 46_001;
    const TROPHY: u32 = 46_002;
    const UNKNOWN: u32 = 46_003;
    const CHAIR_ID: &str = "House_HousingItem_Test_Chair";
    const TROPHY_ID: &str = "House_HousingItem_Buff_Test_Trophy";
    /// The trophy's effect: build 25810808 `HousingRollBonusMiningT3` (three stacks at most).
    const TROPHY_EFFECT: &str = "HousingRollBonusMiningT3";
    const TROPHY_STACK_MAX: i64 = 3;
    const CHAIR_POINTS: f64 = 0.11;
    const TROPHY_POINTS: f64 = 0.16;
    const NOW_MS: u64 = 1_800_000_000_000;
    const DAY_MS: u64 = 86_400_000;

    fn rules() -> TravelRules {
        TravelRules::default().with_housing(
            Vec::new(),
            &[],
            HashMap::from([
                (
                    CHAIR,
                    Furniture {
                        item_id: CHAIR_ID.to_owned(),
                        ranking_points: CHAIR_POINTS,
                        duplicate_limit: Some(2),
                        trophy_effect: None,
                    },
                ),
                (
                    TROPHY,
                    Furniture {
                        item_id: TROPHY_ID.to_owned(),
                        ranking_points: TROPHY_POINTS,
                        duplicate_limit: Some(1),
                        trophy_effect: Some(TROPHY_EFFECT.to_owned()),
                    },
                ),
            ]),
        )
    }

    fn placed(index: u16, item: u32) -> HousingItem {
        HousingItem {
            index,
            item,
            position: StoredPoint::of(Point {
                x: 1.0,
                y: -0.5,
                z: 3.75,
            }),
            rotation: [0.0, 0.0, 0.0, 1.0].map(f32::to_bits),
        }
    }

    fn home(items: Vec<HousingItem>, taxes_due_ms: u64) -> OwnedHome {
        OwnedHome {
            plot: [0x11; RAW_ID_LEN],
            position: StoredPoint::of(Point {
                x: 100.0,
                y: 200.0,
                z: 30.0,
            }),
            purchased_ms: NOW_MS - DAY_MS,
            items,
            record: HomeRecord {
                taxes_due_ms,
                ..HomeRecord::default()
            },
        }
    }

    #[test]
    fn copies_above_the_duplicate_limit_add_no_points() {
        let items = [
            placed(0, CHAIR),
            placed(1, CHAIR),
            placed(2, CHAIR),
            placed(3, TROPHY),
            placed(4, UNKNOWN),
        ];
        let points = decoration_points(&items, &rules());
        assert!(
            (points - (2.0 * CHAIR_POINTS + TROPHY_POINTS)).abs() < 1e-9,
            "{points}"
        );
    }

    #[test]
    fn the_trophies_of_paid_houses_hold_their_effects_up_to_the_stack_limit() {
        use nw_content::status_effects::{EffectSourceRow, StatusEffectSource};
        let effects = StatusEffectRules::build(StatusEffectSource {
            effects: vec![EffectSourceRow {
                status_id: TROPHY_EFFECT.to_owned(),
                stack_max: Some(TROPHY_STACK_MAX),
                ..EffectSourceRow::default()
            }],
            ..StatusEffectSource::default()
        });
        let trophy = lowercase_crc32(TROPHY_EFFECT);
        let paid = |items| home(items, NOW_MS + DAY_MS);
        let homes = [
            paid(vec![placed(0, CHAIR), placed(1, TROPHY)]),
            paid(vec![placed(0, TROPHY), placed(1, TROPHY)]),
            paid(vec![placed(0, TROPHY), placed(1, UNKNOWN)]),
        ];
        assert_eq!(
            trophy_effects(&homes, &rules(), &effects, NOW_MS),
            [(trophy, 3.0)],
            "four copies, three stacks at most"
        );
        let overdue = home(vec![placed(0, TROPHY)], NOW_MS - DAY_MS);
        assert!(trophy_effects(&[overdue], &rules(), &effects, NOW_MS).is_empty());
        assert!(
            trophy_effects(&homes, &rules(), &StatusEffectRules::default(), NOW_MS).is_empty(),
            "an effect the data does not have"
        );
    }

    #[test]
    fn a_paid_house_names_its_trophies_and_score() {
        let paid = home(vec![placed(0, CHAIR), placed(1, TROPHY)], NOW_MS + DAY_MS);
        let decoration = house_decoration(&paid, &rules(), NOW_MS);
        assert!(decoration.active);
        assert_eq!(decoration.trophies, vec![lowercase_crc32(TROPHY_ID)]);
        assert!((f64::from(decoration.score) - (CHAIR_POINTS + TROPHY_POINTS)).abs() < 1e-6);
    }

    /// Retail `login_into_an_expired_upkeep_house`: an overdue house has its buffs off, no
    /// trophies and score 0.
    #[test]
    fn an_overdue_house_has_no_buffs_trophies_or_score() {
        let overdue = home(vec![placed(0, TROPHY)], NOW_MS - 1);
        assert_eq!(
            house_decoration(&overdue, &rules(), NOW_MS),
            HouseDecoration {
                active: false,
                trophies: Vec::new(),
                score: 0.0,
            }
        );
        let unknown = home(vec![placed(0, TROPHY)], NO_DUE_TIME);
        assert!(house_decoration(&unknown, &rules(), NOW_MS).active);
    }

    #[test]
    fn the_list_keeps_empty_indexes_and_the_use_flag() {
        let entries = house_entries(&[placed(2, CHAIR)], 0, Some(2));
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0], HousingItemEntry::EMPTY);
        assert_eq!(entries[2].position_cm, [100, -50, 375]);
        assert_eq!(
            entries[2].rotation,
            PackedRotation {
                flag: 0x6e,
                parts: [0; 3],
            },
            "the identity: w largest, x, y and z zero"
        );
        assert!(entries[2].in_use);
        let picked_up = house_entries(&[], entries.len(), None);
        assert_eq!(picked_up, vec![HousingItemEntry::EMPTY; 3]);
    }

    /// Retail `20261010-125035-658`: a new index goes empty first, then with its item; a changed
    /// index goes once; only the first change has the version.
    #[test]
    fn a_new_index_goes_empty_then_with_its_item() {
        let before = house_entries(&[placed(0, CHAIR)], 0, None);
        let now = house_entries(&[placed(0, CHAIR), placed(1, TROPHY)], 0, Some(0));
        let changes = item_changes(&before, &now, 7);
        let summary: Vec<(u32, u64, bool)> = changes
            .iter()
            .map(|change| {
                (
                    change.index,
                    change.version,
                    change.value == HousingItemEntry::EMPTY,
                )
            })
            .collect();
        assert_eq!(summary, vec![(0, 7, false), (1, 0, true), (1, 0, false)]);
        assert!(item_changes(&now, &now, 8).is_empty());
    }

    #[test]
    fn a_house_data_id_is_dynamic_and_stable() {
        let character = Uuid::from_u128(0x1234_5678_9abc_4def_8123_4567_89ab_cdef);
        let plot = [0x22; RAW_ID_LEN];
        let id = house_data_id(character, &plot);
        assert_eq!(id, house_data_id(character, &plot));
        assert_ne!(id, house_data_id(character, &[0x33; RAW_ID_LEN]));
        assert_eq!(id, as_dynamic_raw_id(id));
    }
}
