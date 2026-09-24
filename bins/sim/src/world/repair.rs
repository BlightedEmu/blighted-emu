// SPDX-License-Identifier: AGPL-3.0-or-later
//! Item durability, repairs and gear score upgrades of the players (run-15 items2 study).
//!
//! - **Wear.** With `javelin.enable-item-durability-loss` on (off by default: no retail item
//!   record of 730 ledgers lost durability), a death takes `DurabilityDmgOnDeath` of each worn
//!   item's full durability (0.4 of it for a death by a player), a completed gather takes
//!   "Base Gather Durability Cost" from the tool, and a hit that lands takes
//!   [`HIT_DURABILITY_COST`] from the weapon in the hand (best guess).
//! - **Repair** (3003 one item, 3007 every worn item, 2079 every item of a UI class): the
//!   items go to full durability for the coins and `Repair_T1` points of
//!   [`nw_content::item_repair::repair_cost`] (best guess), or one repair kit of the item's
//!   tier (3003 with its flag). A player short of either repairs nothing.
//! - **Upgrade** (3008): an item at gear score `from` goes to `to` for the
//!   `GearScoreUpgradeDefinition` currency of the levels between (retail `UmbralCurrency`,
//!   spent from its categorical progression points, 9 of 9 retail upgrades).
//!
//! No call has an answer message (retail): the record, the coins and the points go out in the
//! next bundle. A refusal changes nothing.

use std::sync::Arc;

use bevy_ecs::entity::Entity;
use nw_bus::ConnectionRef;
use nw_content::gathering::Gatherable;
use nw_content::item_repair::{
    GATHER_DURABILITY_COST, HIT_DURABILITY_COST, REPAIR_PARTS_PROGRESSION, RepairCost,
    repair_cost, repair_kit,
};
use nw_messages::ItemRepairMsg;
use nw_messages::item_calls::ItemRef;

use super::gear::RepairPick;
use super::{Player, SimWorld};

impl SimWorld {
    /// Answers repair or upgrade call `call` of the player of `connection`.
    pub fn repair_call(&mut self, connection: &ConnectionRef, call: ItemRepairMsg) {
        let Some(&entity) = self.by_connection.get(connection) else {
            return;
        };
        match call {
            ItemRepairMsg::InitiateRepair { item, use_kit, .. } => {
                let pick = RepairPick::One {
                    location: item.location,
                    slot: item.slot,
                    instance: item.instance,
                };
                self.repair(entity, pick, None, use_kit);
            }
            ItemRepairMsg::RepairAllEquipment { .. } => {
                self.repair(entity, RepairPick::Worn, None, false);
            }
            ItemRepairMsg::RepairAllItemClass { item_class, .. } => {
                self.repair(entity, RepairPick::ItemClass, Some(item_class), false);
            }
            ItemRepairMsg::UpgradeGearScore { item, from, to, .. } => {
                self.upgrade(entity, item, u32::from(from), u32::from(to));
            }
        }
    }

    /// Repairs the items `pick` names (of UI class `item_class` for [`RepairPick::ItemClass`])
    /// for player `entity`: with a repair kit of the item's tier when `use_kit`, else for the
    /// summed coins and repair parts.
    fn repair(
        &mut self,
        entity: Entity,
        pick: RepairPick,
        item_class: Option<u64>,
        use_kit: bool,
    ) {
        let content = Arc::clone(&self.content);
        let rolls = &content.item_rolls;
        let in_class = |item_id: &str| {
            item_class.is_some_and(|class| content.is_of_ui_class(item_id, class))
        };
        let repairable = |item_id: &str| rolls.item(item_id).is_some_and(|row| row.repairable);
        let parts_progression = content.progression.categorical(REPAIR_PARTS_PROGRESSION);
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let player = &mut *player;
        let Some(gear) = player.gear.as_mut() else {
            return;
        };
        let damaged: Vec<_> = gear
            .damaged(pick, in_class)
            .into_iter()
            .filter(|item| repairable(&item.item_id))
            .collect();
        if damaged.is_empty() {
            return;
        }
        if use_kit {
            let Some(tier) = rolls.item(&damaged[0].item_id).map(|row| row.tier) else {
                return;
            };
            if !gear.take(&repair_kit(tier), 1) {
                tracing::debug!(tier, "repair refused: no repair kit of the tier");
                return;
            }
            gear.repair(pick, in_class, repairable);
            return;
        }
        let cost = damaged
            .iter()
            .filter_map(|item| Some(repair_cost(rolls.item(&item.item_id)?, item.durability)))
            .fold(RepairCost::default(), |total, cost| RepairCost {
                coins: total.coins.saturating_add(cost.coins),
                parts: total.parts.saturating_add(cost.parts),
            });
        let parts_held = parts_progression.map_or(0, |parts| player.masteries.experience(parts));
        if player.coins < cost.coins || parts_held < cost.parts {
            tracing::debug!(?cost, coins = player.coins, parts_held, "repair refused: short");
            return;
        }
        if gear.repair(pick, in_class, repairable) == 0 {
            return;
        }
        if cost.coins > 0 {
            player.coins -= cost.coins;
            player.coins_changed = true;
        }
        if let Some(parts) = parts_progression.filter(|_| cost.parts > 0) {
            player.masteries.set(parts, parts_held - cost.parts);
        }
    }

    /// Raises the gear score of the item `item` names from `from` to `to` for player `entity`,
    /// paying the upgrade levels' currency from its categorical progression points.
    fn upgrade(&mut self, entity: Entity, item: ItemRef, from: u32, to: u32) {
        let content = Arc::clone(&self.content);
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let player = &mut *player;
        let Some(gear) = player.gear.as_mut() else {
            return;
        };
        if let Err(refusal) = gear.upgrade_target(item.location, item.slot, item.instance, from) {
            tracing::debug!(?refusal, from, to, "upgrade refused");
            return;
        }
        let (currency_id, cost) = match content.gear_score_upgrades.cost(from, to) {
            Ok(cost) => cost,
            Err(refusal) => {
                tracing::debug!(?refusal, from, to, "upgrade refused");
                return;
            }
        };
        let Some(currency) = content.progression.categorical(&currency_id) else {
            tracing::debug!(currency = %currency_id, "upgrade refused: unknown currency");
            return;
        };
        let balance = player.masteries.experience(currency);
        let Some(left) = u64::from(balance)
            .checked_sub(cost)
            .and_then(|left| u32::try_from(left).ok())
        else {
            tracing::debug!(balance, cost, "upgrade refused: short of currency");
            return;
        };
        if gear
            .set_gear_score(item.location, item.slot, item.instance, to)
            .is_ok()
        {
            player.masteries.set(currency, left);
        }
    }

    /// Player `entity` died (by a player when `pvp`): its worn items wear when durability loss
    /// is on.
    pub(super) fn wear_at_death(&mut self, entity: Entity, pvp: bool) {
        if !self.content.gameplay.durability_loss() {
            return;
        }
        let content = Arc::clone(&self.content);
        if let Some(mut player) = self.world.get_mut::<Player>(entity)
            && let Some(gear) = player.gear.as_mut()
        {
            gear.wear_at_death(&content.item_rolls, pvp);
        }
    }

    /// Player `entity` completed a gather of `gatherable`: the tool in its tradeskill's slot
    /// wears by the gather cost when durability loss is on.
    pub(super) fn wear_tool(&mut self, entity: Entity, gatherable: &Gatherable) {
        let Some(slot_id) = super::gathering::tool_slot(gatherable) else {
            return;
        };
        self.wear_slot(entity, slot_id, GATHER_DURABILITY_COST);
    }

    /// A hit of player `attacker` (a player number) landed: the weapon in its hand wears when
    /// durability loss is on (best guess: every landed hit, [`HIT_DURABILITY_COST`]).
    pub(super) fn wear_weapon(&mut self, attacker: u64) {
        if !self.content.gameplay.durability_loss() {
            return;
        }
        let Some(entity) = self.player_by_number(attacker) else {
            return;
        };
        let held = self
            .world
            .get::<Player>(entity)
            .and_then(|player| player.gear.as_ref()?.held_slot());
        if let Some(slot_id) = held {
            self.wear_slot(entity, slot_id, HIT_DURABILITY_COST);
        }
    }

    /// The item worn in `slot_id` of player `entity` loses `amount` when durability loss is on.
    fn wear_slot(&mut self, entity: Entity, slot_id: u32, amount: u32) {
        if !self.content.gameplay.durability_loss() {
            return;
        }
        if let Some(mut player) = self.world.get_mut::<Player>(entity)
            && let Some(gear) = player.gear.as_mut()
        {
            gear.wear_slot(slot_id, amount);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nw_content::gameplay_config::GameplayConfig;
    use nw_content::item_repair::GearScoreUpgrades;
    use nw_content::item_rolls::{ItemRollRow, ItemRolls};
    use nw_content::perks::PerkTable;
    use nw_content::progression::{Progression, ProgressionSource};
    use nw_messages::item_calls::LOCATION_BAG;
    use nw_messages::wire::RAW_ID_LEN;
    use nw_messages::{FacetHeader, ItemRef, ItemRepairMsg};

    use super::*;
    use crate::player_content::PlayerContent;
    use crate::world::gear::Gear;
    use crate::world::gear::tests::{BACKSTORY, CHARACTER, FLINT, SWORD, kits};
    use crate::world::tests::{connection, join, world_with_spawns};

    const UMBRAL: &str = "UmbralCurrency";
    const UMBRAL_CAP: i64 = 10_000;
    const PARTS_CAP: i64 = 2_000;
    const SWORD_DURABILITY: u32 = 375;
    const SWORD_TIER: u32 = 5;
    const FIRST_UPGRADE_LEVEL: u32 = 700;
    const LEVELS: u32 = 100;
    const LEVEL_COST: u64 = 5;
    const PARTS: u32 = 5;

    fn header() -> FacetHeader {
        FacetHeader {
            target: 0,
            entity: 0,
        }
    }

    fn content() -> PlayerContent {
        let currency = |id: &str, cap| ProgressionSource {
            id: id.to_owned(),
            max_level: 0,
            auto_rank_up: false,
            ranks: vec![(0, cap)],
        };
        PlayerContent {
            kits: kits(),
            item_rolls: ItemRolls::build(
                [ItemRollRow {
                    item_id: SWORD.to_owned(),
                    tier: SWORD_TIER,
                    durability: SWORD_DURABILITY,
                    durability_loss_on_death: 0.12,
                    repairable: true,
                    repair_dust_modifier: Some(0.25),
                    ..ItemRollRow::default()
                }],
                PerkTable::default(),
            ),
            gear_score_upgrades: GearScoreUpgrades::build(
                (FIRST_UPGRADE_LEVEL..FIRST_UPGRADE_LEVEL + LEVELS)
                    .map(|level| (level, UMBRAL.to_owned(), LEVEL_COST)),
            ),
            progression: Progression::default().with_progressions([
                currency(UMBRAL, UMBRAL_CAP),
                currency(REPAIR_PARTS_PROGRESSION, PARTS_CAP),
            ]),
            gameplay: GameplayConfig {
                item_durability_loss: Some(true),
                ..GameplayConfig::default()
            },
            ..PlayerContent::default()
        }
    }

    /// A world whose player 1 wears the test kit, carries flint, and holds `currency` of
    /// `currency_id`.
    fn world(currency_id: &str, currency: u32) -> (SimWorld, Entity) {
        let mut world = world_with_spawns(Vec::new());
        world.content = Arc::new(content());
        join(&mut world, 1, 0.0);
        let kits = kits();
        let mut gear = Gear::starter(&kits, CHARACTER, Some(BACKSTORY)).expect("gear");
        assert!(gear.add(kits.item(FLINT).expect("flint"), 1));
        let entity = world.by_connection[&connection(1)];
        let content = Arc::clone(&world.content);
        let mastery = content.progression.categorical(currency_id).expect("currency");
        let mut player = world.world.get_mut::<Player>(entity).expect("player");
        player.gear = Some(gear);
        player.masteries.set(mastery, currency);
        drop(player);
        (world, entity)
    }

    fn player(world: &SimWorld, entity: Entity) -> &Player {
        world.world.get::<Player>(entity).expect("player")
    }

    fn balance(world: &SimWorld, entity: Entity, id: &str) -> u32 {
        let mastery = world.content.progression.categorical(id).expect("currency");
        player(world, entity).masteries.experience(mastery)
    }

    fn sword_durability(world: &SimWorld, entity: Entity) -> Option<u32> {
        player(world, entity)
            .gear
            .as_ref()?
            .carried()
            .into_iter()
            .find(|item| item.item_id == SWORD)?
            .state
            .durability
    }

    /// The flint's bag slot, instance id and gear score.
    fn flint(world: &SimWorld, entity: Entity) -> (u32, [u8; RAW_ID_LEN], u32) {
        let carried = player(world, entity).gear.as_ref().expect("gear").carried();
        let flint = carried
            .into_iter()
            .find(|item| item.item_id == FLINT)
            .expect("flint");
        let nw_bus::ItemPlace::Bag(slot) = flint.place else {
            panic!("in the bag");
        };
        (
            u32::from(slot),
            flint.state.instance.expect("instance"),
            flint.state.gear_score.expect("gear score"),
        )
    }

    fn upgrade(world: &mut SimWorld, slot: u32, instance: [u8; RAW_ID_LEN], from: u16, to: u16) {
        world.repair_call(
            &connection(1),
            ItemRepairMsg::UpgradeGearScore {
                header: header(),
                item: ItemRef {
                    location: LOCATION_BAG,
                    slot,
                    instance,
                    extra: 0,
                },
                from,
                to,
            },
        );
    }

    #[test]
    fn an_upgrade_spends_the_level_costs_and_a_short_one_changes_nothing() {
        const START: u16 = 700;
        let (mut world, entity) = world(UMBRAL, 12);
        let (slot, instance, _) = flint(&world, entity);
        world
            .world
            .get_mut::<Player>(entity)
            .and_then(|mut player| {
                player
                    .gear
                    .as_mut()?
                    .set_gear_score(LOCATION_BAG, slot, instance, u32::from(START))
                    .ok()
            })
            .expect("set");
        // A call that starts from another gear score is refused.
        upgrade(&mut world, slot, instance, START + 1, START + 2);
        assert_eq!(flint(&world, entity).2, u32::from(START));
        upgrade(&mut world, slot, instance, START, START + 2);
        assert_eq!(flint(&world, entity).2, u32::from(START) + 2);
        assert_eq!(balance(&world, entity, UMBRAL), 2, "two levels of 5");
        upgrade(&mut world, slot, instance, START + 2, START + 4);
        assert_eq!(flint(&world, entity).2, u32::from(START) + 2, "short of currency");
        assert_eq!(balance(&world, entity, UMBRAL), 2);
    }

    #[test]
    fn a_death_wears_the_sword_and_a_repair_costs_coins_and_parts() {
        let (mut world, entity) = world(REPAIR_PARTS_PROGRESSION, PARTS);
        world.wear_at_death(entity, false);
        // ceil(375 × 0.12) = 45 lost.
        assert_eq!(sword_durability(&world, entity), Some(SWORD_DURABILITY - 45));
        let repair_all = ItemRepairMsg::RepairAllEquipment { header: header() };
        world.repair_call(&connection(1), repair_all);
        assert_eq!(
            sword_durability(&world, entity),
            Some(SWORD_DURABILITY - 45),
            "no coins: ceil(45 × 0.01 × 3) = 2 are needed"
        );
        world.world.get_mut::<Player>(entity).expect("player").coins = 10;
        world.repair_call(&connection(1), repair_all);
        assert_eq!(sword_durability(&world, entity), Some(SWORD_DURABILITY));
        assert_eq!(player(&world, entity).coins, 8);
        // ceil(45 × 0.1 × 0.25 ÷ 3.75) = 1 part.
        assert_eq!(balance(&world, entity, REPAIR_PARTS_PROGRESSION), PARTS - 1);
    }

    #[test]
    fn wear_waits_for_the_durability_switch() {
        let (mut world, entity) = world(REPAIR_PARTS_PROGRESSION, PARTS);
        Arc::make_mut(&mut world.content).gameplay = GameplayConfig::default();
        world.wear_at_death(entity, false);
        assert_eq!(sword_durability(&world, entity), Some(SWORD_DURABILITY));
    }
}
