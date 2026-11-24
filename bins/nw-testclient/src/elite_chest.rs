// SPDX-License-Identifier: AGPL-3.0-or-later
//! The elite chest activity: the bot opens an elite chest of the open world, as a player of an
//! elite chest run does, and opens it again: the server takes the first open (its loot) and
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

/// One elite chest: its point index in the level's gatherables, its gather node, its capital's
/// entity id and its variant's display name.
#[derive(Debug, Clone, PartialEq)]
struct EliteChest {
    index: usize,
    node: GatherNode,
    capital: u64,
    display: Vec<u8>,
}

/// The elite chest the activity opens: the first point of `world` of one of the `chests` that a
/// capital of one of the crate `variants` stands on; why there is none.
fn elite_chest(
    world: &WorldFile,
    chests: &EliteChests,
    variants: &[CrateVariant],
) -> Result<EliteChest, String> {
    world
        .gatherables
        .iter()
        .enumerate()
        .filter(|(_, point)| chests.contains(point.gatherable_id))
        .find_map(|(index, point)| {
            let capital = world.capitals.iter().find(|capital| {
                capital
                    .position
                    .iter()
                    .zip(point.position)
                    .all(|(a, b)| (a - b).abs() <= CAPITAL_ON_POINT_M)
            })?;
            let variant_name = capital.variant_name.as_deref()?;
            let variant = variants
                .iter()
                .find(|variant| variant.variant_id.eq_ignore_ascii_case(variant_name))?;
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
        .ok_or_else(|| "the level has no elite chest on a capital of a known variant".to_owned())
}

/// The steps of the activity: open the elite chest, then open it again and be refused.
pub fn elite_chest_steps(world: &WorldFile, tables: &Tables) -> Result<Vec<Step>, String> {
    let chest = elite_chest(
        world,
        &EliteChests::from_tables(tables),
        &CrateVariant::from_tables(tables),
    )?;
    Ok(chest_steps(&chest))
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
        let chest = elite_chest(&world, &chests, &variants).expect("the chest");
        assert_eq!(chest.index, 1);
        assert_eq!(Some(chest.capital), capital_id(CHEST_GUID));
        let steps = chest_steps(&chest);
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
