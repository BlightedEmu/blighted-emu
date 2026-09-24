// SPDX-License-Identifier: AGPL-3.0-or-later
//! Items with rolls of their own (run-15 items2 study): a dropped gear item rolls its gear
//! score from its loot entry, the player's level and the killed mob's level
//! ([`nw_content::drop_rolls`]) and its perks from its perk buckets
//! ([`nw_content::item_rolls`]); a crafted one comes with its gear score. Each rolled item is a
//! stack of its own in the bag.

use std::collections::BTreeMap;
use std::sync::Arc;

use nw_content::item_rolls::RolledItem;
use nw_content::loot::DropGearScore;

use super::{Player, SimWorld};
use crate::objectives::Event;

/// One dropped item stack and its rolls (`None`: an item of its data values).
pub(super) type RolledDrop = ((String, u32), Option<RolledItem>);

/// A drop's items and the rolls of each entry: (item id, quantity) pairs and their rolls.
pub(super) type DropEntries = (Vec<(String, u32)>, Vec<Option<RolledItem>>);

impl SimWorld {
    /// The rolls of the `items` player `number` gets (from a kill of `mob`, when one), with the
    /// gear score `hints` of the loot roll by item id: an item with rolls of its own becomes one
    /// entry per item, each with its gear score and perks; another stays one entry without
    /// rolls.
    pub(super) fn roll_drops(
        &mut self,
        number: u64,
        mob: Option<u64>,
        items: Vec<(String, u32)>,
        hints: &BTreeMap<String, DropGearScore>,
    ) -> DropEntries {
        let content = Arc::clone(&self.content);
        let player_level = self
            .player_by_number(number)
            .and_then(|entity| self.world.get::<Player>(entity))
            .map(|player| content.progression.level_of(player.experience));
        let mob_level = mob.and_then(|mob| self.mobs.level(mob));
        let plain = DropGearScore::default();
        let dice = &mut self.dice;
        let mut below = |n: u32| dice.below(n);
        let mut entries = Vec::new();
        let mut rolls = Vec::new();
        for (item_id, quantity) in items {
            let Some(row) = content.item_rolls.item(&item_id).filter(|row| row.rolls()) else {
                entries.push((item_id, quantity));
                rolls.push(None);
                continue;
            };
            let hint = hints.get(&item_id).unwrap_or(&plain);
            for _ in 0..quantity {
                let wanted =
                    content
                        .drop_rolls
                        .gear_score(hint, row, player_level, mob_level, &mut below);
                let rolled = content
                    .item_rolls
                    .roll(&item_id, wanted, &hint.perks, &mut below);
                entries.push((item_id.clone(), 1));
                rolls.push(Some(rolled));
            }
        }
        (entries, rolls)
    }

    /// Player `number` gets the dropped `items` at `now_ms`: the items without rolls as loot
    /// ([`SimWorld::give_loot`]), each rolled one as a stack of its own.
    pub(super) fn give_drops(&mut self, number: u64, items: Vec<RolledDrop>, now_ms: u64) {
        let mut plain = Vec::new();
        let mut rolled: BTreeMap<String, Vec<RolledItem>> = BTreeMap::new();
        for ((item_id, quantity), roll) in items {
            match roll {
                Some(roll) => rolled.entry(item_id).or_default().push(roll),
                None => plain.push((item_id, quantity)),
            }
        }
        self.give_loot(number, plain, now_ms);
        for (item_id, rolls) in rolled {
            self.give_rolled(number, &item_id, rolls, now_ms);
        }
    }

    /// Player `number` gets one item `item_id` per entry of `rolled`, each with its gear score
    /// and perks, at `now_ms`; its quest counts the items that fit its bag.
    pub(super) fn give_rolled(
        &mut self,
        number: u64,
        item_id: &str,
        rolled: Vec<RolledItem>,
        now_ms: u64,
    ) {
        let Some(entity) = self.player_by_number(number) else {
            return;
        };
        let content = Arc::clone(&self.content);
        let Some(item) = content.kits.item(item_id) else {
            tracing::debug!(item = %item_id, "rolled item without a master row");
            return;
        };
        let mut gained = 0_u32;
        if let Some(mut player) = self.world.get_mut::<Player>(entity)
            && let Some(gear) = player.gear.as_mut()
        {
            for roll in &rolled {
                if gear.add_rolled(&content.kits, item, 1, roll) {
                    gained += 1;
                } else {
                    tracing::info!(player = number, item = %item_id, "rolled item lost: a full bag");
                }
            }
        }
        if gained > 0 {
            tracing::debug!(player = number, item = %item_id, gained, "rolled items gained");
            self.quest_event(
                number,
                Event::ItemGained {
                    item: item_id,
                    quantity: gained,
                },
                now_ms,
            );
        }
    }
}
