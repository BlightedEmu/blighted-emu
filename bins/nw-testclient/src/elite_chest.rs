// SPDX-License-Identifier: AGPL-3.0-or-later
//! The elite chest activity: the bot opens an elite chest in each of several elite zones of the
//! open world ([`ELITE_ZONES`]), as a player of an elite chest run does, and opens each again: the server takes the first open (its loot) and
//! refuses the second until the next daily reset (`nw_content::chest_lockouts`: retail 3743
//! kind 14 status 2 while the player's tracker entry lives; the elite chest's entry ends at the
//! daily reset in 94 of 96 entries).
//!
//! The chest is a capital of the level: the client names it by the capital's entity id
//! ([`capital_id`]), which tells the server the chest's variant and loot table and its
//! instanced lock, and by its gather point.

use nw_content::Tables;
use nw_content::chest_lockouts::EliteChests;
use nw_content::gathering::CrateVariant;
use nw_world::WorldFile;
use nw_world::capitals::capital_id;

use crate::activity::{Step, capital_gather_steps, refused_capital_gather_step};
use crate::gather::GatherNode;

/// The largest difference on each axis between a gather point and the capital that stands on
/// it, m (the sim's `CAPITAL_ON_POINT_M`: a float's rounding).
const CAPITAL_ON_POINT_M: f32 = 0.01;
/// How many elite zones the activity visits: one chest in each, by the chest point's first
/// territory (Inferred: a chest run of a few zones).
const ELITE_ZONES: usize = 4;

/// One elite chest: its point index in the level's gatherables, its gather node, its capital's
/// entity id and its variant's display name.
#[derive(Debug, Clone, PartialEq)]
struct EliteChest {
    index: usize,
    node: GatherNode,
    capital: u64,
    display: Vec<u8>,
}

/// The elite chests the activity opens: the first point in each of `zones` territories of
/// `world` of one of the `chests` that a capital of one of the crate `variants` stands on; why
/// there is none.
fn elite_chests(
    world: &WorldFile,
    chests: &EliteChests,
    variants: &[CrateVariant],
    zones: usize,
) -> Result<Vec<EliteChest>, String> {
    let mut seen = Vec::new();
    let found: Vec<EliteChest> = world
        .gatherables
        .iter()
        .enumerate()
        .filter(|(_, point)| chests.contains(point.gatherable_id))
        .filter_map(|(index, point)| {
            // A chest's point often holds a `<PLOT>` capital too (497 of 576 open world chest
            // points, listed before the chest's own capital): take the one of a known variant.
            let (capital, variant) = world
                .capitals
                .iter()
                .filter(|capital| {
                    capital
                        .position
                        .iter()
                        .zip(point.position)
                        .all(|(a, b)| (a - b).abs() <= CAPITAL_ON_POINT_M)
                })
                .find_map(|capital| {
                    let variant_name = capital.variant_name.as_deref()?;
                    let variant = variants
                        .iter()
                        .find(|variant| variant.variant_id.eq_ignore_ascii_case(variant_name))?;
                    Some((capital, variant))
                })?;
            Some(EliteChest {
                index,
                node: GatherNode {
                    gatherable: point.gatherable_id,
                    at: point.position,
                },
                capital: capital_id(&capital.id)?,
                display: variant.name.as_bytes().to_vec(),
            })
        })
        .filter(|chest| {
            let zone = world.gatherables[chest.index]
                .territory_ids
                .first()
                .copied();
            let fresh = !seen.contains(&zone);
            seen.push(zone);
            fresh
        })
        .take(zones)
        .collect();
    if found.is_empty() {
        return Err("the level has no elite chest on a capital of a known variant".to_owned());
    }
    Ok(found)
}

/// The steps of the activity: in each zone, open the elite chest, then open it again and be
/// refused.
pub fn elite_chest_steps(world: &WorldFile, tables: &Tables) -> Result<Vec<Step>, String> {
    let chests = elite_chests(
        world,
        &EliteChests::from_tables(tables),
        &CrateVariant::from_tables(tables),
        ELITE_ZONES,
    )?;
    Ok(chests.iter().flat_map(chest_steps).collect())
}

/// The open of `chest`, then its refused second open.
fn chest_steps(chest: &EliteChest) -> Vec<Step> {
    let mut steps = capital_gather_steps(
        "elite chest",
        chest.node,
        chest.index,
        chest.capital,
        &chest.display,
    );
    steps.push(refused_capital_gather_step(
        "open the elite chest again: refused until the daily reset",
        chest.node,
        chest.index,
        chest.capital,
        &chest.display,
    ));
    steps
}

#[cfg(test)]
mod tests {
    use nw_content::chest_lockouts::ContainerRow;
    use nw_formats::objectstream::lowercase_crc32;
    use nw_world::records::{Capital, GatherablePoint};

    use super::*;
    use crate::activity::{Act, Proof};

    const CHEST_GUID: &str = "aabbccdd-1122-3344-5566-778899aabbcc";

    /// Three chests, two in one elite zone: the run opens one chest of each zone.
    #[test]
    fn the_run_opens_one_chest_in_each_elite_zone() {
        const CHEST: &str = "LootLargeT3ElitePOI";
        const VARIANT: &str = "Elite_T3_L";
        const ZONES: [u16; 3] = [7, 7, 9];
        let chests = EliteChests::build([ContainerRow {
            id: CHEST,
            container: true,
            elite: true,
        }]);
        let variants = vec![CrateVariant {
            variant_id: VARIANT.to_owned(),
            gatherable: CHEST.to_owned(),
            name: "@LootContainer_EliteLarge_DisplayName".to_owned(),
            table: "ChestLootElite".to_owned(),
        }];
        let places: Vec<[f32; 3]> = (0..ZONES.len())
            .map(|index| [100.0 * (index as f32 + 1.0), 0.0, 0.0])
            .collect();
        let world = WorldFile {
            gatherables: ZONES
                .iter()
                .zip(&places)
                .map(|(zone, at)| GatherablePoint {
                    gatherable_id: lowercase_crc32(CHEST),
                    position: *at,
                    territory_ids: vec![*zone],
                    ..GatherablePoint::default()
                })
                .collect(),
            capitals: places
                .iter()
                .map(|at| Capital {
                    id: CHEST_GUID.to_owned(),
                    position: *at,
                    variant_name: Some(VARIANT.to_owned()),
                    ..Capital::default()
                })
                .collect(),
            ..WorldFile::default()
        };
        let found = elite_chests(&world, &chests, &variants, ELITE_ZONES).expect("chests");
        assert_eq!(
            found.iter().map(|chest| chest.index).collect::<Vec<_>>(),
            [0, 2]
        );
    }

    /// A plot capital without a variant stands on the chest's point before the chest's own
    /// capital: the run takes the chest's.
    #[test]
    fn a_plot_capital_on_the_chest_s_point_is_not_the_chest() {
        const CHEST: &str = "LootLargeT3ElitePOI";
        const VARIANT: &str = "ChestEliteT5";
        const PLOT: &str = "<PLOT>";
        const PLOT_GUID: &str = "11111111-2222-3333-4444-555555555555";
        const AT: [f32; 3] = [100.0, 200.0, 30.0];
        let chests = EliteChests::build([ContainerRow {
            id: CHEST,
            container: true,
            elite: true,
        }]);
        let variants = vec![CrateVariant {
            variant_id: VARIANT.to_owned(),
            gatherable: CHEST.to_owned(),
            name: "@LootContainer_EliteLarge_DisplayName".to_owned(),
            table: "ChestLootElite".to_owned(),
        }];
        let world = WorldFile {
            gatherables: vec![GatherablePoint {
                gatherable_id: lowercase_crc32(CHEST),
                position: AT,
                ..GatherablePoint::default()
            }],
            capitals: vec![
                Capital {
                    id: PLOT_GUID.to_owned(),
                    position: AT,
                    slice_name: PLOT.to_owned(),
                    ..Capital::default()
                },
                Capital {
                    id: CHEST_GUID.to_owned(),
                    position: AT,
                    variant_name: Some(VARIANT.to_owned()),
                    ..Capital::default()
                },
            ],
            ..WorldFile::default()
        };
        let found = elite_chests(&world, &chests, &variants, ELITE_ZONES).expect("chests");
        assert_eq!(
            found.iter().map(|chest| chest.capital).collect::<Vec<_>>(),
            [capital_id(CHEST_GUID).expect("guid")]
        );
    }

    #[test]
    fn the_bot_opens_the_elite_chest_by_its_capital_then_is_refused() {
        let chest = "LootLargeT3ElitePOI";
        let chests = EliteChests::build([ContainerRow {
            id: chest,
            container: true,
            elite: true,
        }]);
        let at = [10.0, 20.0, 30.0];
        const VARIANT: &str = "Elite_T3_L";
        const DISPLAY: &str = "@LootContainer_EliteLarge_DisplayName";
        let variants = vec![CrateVariant {
            variant_id: VARIANT.to_owned(),
            gatherable: chest.to_owned(),
            name: DISPLAY.to_owned(),
            table: "ChestLootElite".to_owned(),
        }];
        let world = WorldFile {
            gatherables: vec![
                GatherablePoint {
                    gatherable_id: lowercase_crc32("SingleStone"),
                    position: [0.0; 3],
                    ..GatherablePoint::default()
                },
                GatherablePoint {
                    gatherable_id: lowercase_crc32(chest),
                    position: at,
                    ..GatherablePoint::default()
                },
            ],
            capitals: vec![Capital {
                id: CHEST_GUID.to_owned(),
                position: at,
                variant_name: Some(VARIANT.to_owned()),
                ..Capital::default()
            }],
            ..WorldFile::default()
        };
        let found = elite_chests(&world, &chests, &variants, ELITE_ZONES).expect("the chest");
        assert_eq!(found.len(), 1, "one zone");
        let chest = &found[0];
        assert_eq!(chest.index, 1);
        assert_eq!(Some(chest.capital), capital_id(CHEST_GUID));
        let steps = chest_steps(chest);
        let Some(Act::Interact { request, name }) = steps[0].acts.last() else {
            panic!("the first step gathers");
        };
        assert_eq!(request.target_entity, chest.capital, "named by its capital");
        assert_eq!(
            name.as_slice(),
            DISPLAY.as_bytes(),
            "and its variant's name"
        );
        assert!(matches!(
            steps.last().map(|step| &step.proof),
            Some(Proof::Refused)
        ));
    }
}
