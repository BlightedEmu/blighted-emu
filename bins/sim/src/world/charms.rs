// SPDX-License-Identifier: AGPL-3.0-or-later
//! Gems and charms put into items (6983 and 7038, run-15 items2 study) and the notification
//! counts the answer carries.
//!
//! Retail MSQ15 (9 of 9 requests): about 40 ms after a 6983 the server sent one empty storage
//! search (7048: the gem stack changed; the world sends it with the item change), then 3234
//! `CharmSlotRequestsSucceeded` with the perk's `DisplayName` (`perkname`) and the item's `Name`
//! (`itemname`), then 3236 with that notification's count one higher; the item's records carry
//! the new perk. No coins and no other currency moved. The client sends 7038 about 170 ms after
//! each 6983 and when the charm window closes; retail answered it with nothing, and this server
//! keeps no pending request for it to drop. A refusal has its own notification,
//! `CharmSlotRequestsFailed` (best guess: sent alone, as 3233, since no capture holds one).

use std::sync::Arc;

use bevy_ecs::entity::Entity;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::item_calls::LOCATION_BAG;
use nw_messages::{
    ItemRef, Replacement, SendNotificationMsg, SendNotificationWithReplacementsMsg, ServerMessage,
};

use super::gear::{CharmRefusal, CharmTarget};
use super::{Player, SimWorld, send_now};

/// The notification of a slotted gem or charm.
const CHARM_SLOTTED: &str = "CharmSlotRequestsSucceeded";
/// The notification of a refused one.
const CHARM_REFUSED: &str = "CharmSlotRequestsFailed";
/// The placeholder of the perk's name.
const PERK_NAME_KEY: &str = "perkname";
/// The placeholder of the item's name.
const ITEM_NAME_KEY: &str = "itemname";

impl SimWorld {
    /// Player `number` (`entity`) puts the gem or charm `charm` into perk slot `perk_index` of
    /// the item `target` (6983); the answer is the success or the refusal notification and the
    /// counts.
    pub(super) fn slot_charm(
        &mut self,
        entity: Entity,
        number: u64,
        target: ItemRef,
        charm: ItemRef,
        perk_index: u32,
    ) {
        let content = Arc::clone(&self.content);
        let dice = &mut self.dice;
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let player = &mut *player;
        let connection = player.connection.clone();
        let result = if charm.location != LOCATION_BAG {
            Err(CharmRefusal::NoCharm)
        } else {
            player
                .gear
                .as_mut()
                .ok_or(CharmRefusal::NoSuchItem)
                .and_then(|gear| {
                    gear.slot_charm(
                        &content.kits,
                        &content.item_rolls,
                        CharmTarget {
                            location: target.location,
                            slot: target.slot,
                            instance: target.instance,
                        },
                        charm.slot,
                        charm.instance,
                        perk_index,
                        &mut |below| dice.below(below),
                    )
                })
        };
        let (notification, shown) = match result {
            Ok(slotted) => {
                let perk_name = content
                    .item_rolls
                    .perks
                    .perk(&slotted.perk_id)
                    .and_then(|perk| perk.display_name.clone())
                    .unwrap_or(slotted.perk_id);
                let item_name = content
                    .item_names
                    .of(&slotted.item_id)
                    .map_or(slotted.item_id.clone(), str::to_owned);
                let notification = lowercase_crc32(CHARM_SLOTTED);
                let replacements = vec![
                    Replacement {
                        key: lowercase_crc32(PERK_NAME_KEY),
                        text: perk_name.into_bytes(),
                    },
                    Replacement {
                        key: lowercase_crc32(ITEM_NAME_KEY),
                        text: item_name.into_bytes(),
                    },
                ];
                (
                    notification,
                    ServerMessage::SendNotificationWithReplacements(
                        SendNotificationWithReplacementsMsg::to(number, notification, replacements),
                    ),
                )
            }
            Err(refusal) => {
                tracing::debug!(player = number, ?refusal, "gem or charm refused");
                let notification = lowercase_crc32(CHARM_REFUSED);
                (
                    notification,
                    ServerMessage::SendNotification(SendNotificationMsg::to(number, notification)),
                )
            }
        };
        let Some(sync) = self
            .world
            .get_mut::<Player>(entity)
            .map(|mut player| player.notification_sync(notification))
        else {
            return;
        };
        send_now(&mut self.combat_messages, &connection, &[shown, sync]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_placeholders_are_the_retail_keys() {
        /// The retail keys of `perkname` and `itemname` (notifications note).
        const RETAIL_PERK_NAME: u32 = 0xc793_6637;
        const RETAIL_ITEM_NAME: u32 = 0xf45d_1cce;
        assert_eq!(lowercase_crc32(PERK_NAME_KEY), RETAIL_PERK_NAME);
        assert_eq!(lowercase_crc32(ITEM_NAME_KEY), RETAIL_ITEM_NAME);
    }
}
