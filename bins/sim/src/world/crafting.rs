// SPDX-License-Identifier: AGPL-3.0-or-later
//! Crafting at a station (beach-quests note, section 4). The player opens a station (interaction
//! kind 20; the server answers with the station's name and the player's interactor target), sends
//! the recipe and the ingredients it picked (2997), and 1,057 ms later the server answers with
//! the item crafted and an idle update (2993 twice). The close of the window (2999) gets one more
//! idle update.

use bevy_ecs::entity::Entity;
use nw_content::crafting::Recipe;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{CraftIngredient, CraftProgressMsg, InteractionRequest, ServerMessage};

use super::statics::interactor_target;
use super::{Player, SimWorld, gather_answer, send_now};
use crate::objectives::Event;

/// A crafting station of the level: the client names it by its entity id.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Station {
    /// Its entity id in the client's level data.
    target: [u8; RAW_ID_LEN],
    /// Its station type, as recipes list them (`StationType1` to `StationType4`).
    station_type: &'static str,
    /// The name its interaction's answer carries.
    name: &'static [u8],
}

/// The stations this server knows: Urda's Campfire on the start beach (Observed: its id and name;
/// Inferred: a `camp1` station, as both beach recipes list it).
static STATIONS: [Station; 1] = [Station {
    target: 0xff21_e713_8c5a_804d_0000_0000_0000_0000_u128.to_be_bytes(),
    station_type: "camp1",
    name: b"@ui_watcherscampfire",
}];
/// The status of an accepted and of a refused station interaction.
const STATION_ACCEPTED: u8 = 1;
const STATION_REFUSED: u8 = 2;
/// How long a craft takes (retail: 2993 1,057 ms after the 2997, knife and ration alike).
const CRAFT_MS: u64 = 1_057;
/// The crafts one request makes when it asks for none.
const ONE_CRAFT: u32 = 1;

/// A craft that runs: its items are taken, its output comes at `due_ms`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PendingCraft {
    number: u64,
    recipe: String,
    item: String,
    quantity: u32,
    due_ms: u64,
}

impl SimWorld {
    /// Player `number` (`entity`) uses the crafting station of `request`: a known station is
    /// accepted and becomes the player's interactor target; another is refused.
    pub(super) fn open_station(
        &mut self,
        entity: Entity,
        number: u64,
        request: InteractionRequest,
    ) {
        let station = STATIONS
            .iter()
            .find(|station| station.target == request.target);
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let Some(station) = station else {
            tracing::debug!(
                player = number,
                ?request,
                "use of an unknown crafting station"
            );
            let refusal = gather_answer(number, request, STATION_REFUSED, &[]);
            send_now(&mut self.combat_messages, &player.connection, &[refusal]);
            return;
        };
        let answer = gather_answer(number, request, STATION_ACCEPTED, station.name);
        send_now(&mut self.combat_messages, &player.connection, &[answer]);
        player
            .extra_members
            .push(interactor_target(request.target_entity));
        player.station = Some(station);
    }

    /// Player `number` (`entity`) crafts `crafts` of the recipe whose lowercase id has the CRC-32
    /// `recipe` at `now_ms`, from the `picked` ingredients: at a station of the recipe's type,
    /// with every ingredient in the bag. The ingredients are taken at once; the output comes
    /// [`CRAFT_MS`] later ([`SimWorld::crafts_due`]). A refused craft gets an idle update.
    pub(super) fn craft(
        &mut self,
        entity: Entity,
        number: u64,
        recipe: u32,
        crafts: u32,
        picked: &[CraftIngredient],
        now_ms: u64,
    ) {
        let crafts = crafts.max(ONE_CRAFT);
        let Some(recipe) = self.content.recipes.get(recipe).cloned() else {
            tracing::debug!(player = number, recipe, "craft of an unknown recipe");
            self.send_idle(entity, number);
            return;
        };
        let taken = self.take_ingredients(entity, &recipe, crafts, picked);
        if !taken {
            self.send_idle(entity, number);
            return;
        }
        tracing::debug!(player = number, recipe = %recipe.id, crafts, "craft");
        self.crafts.push(PendingCraft {
            number,
            recipe: recipe.id.clone(),
            item: recipe.item,
            quantity: recipe.quantity.saturating_mul(crafts),
            due_ms: now_ms.saturating_add(CRAFT_MS),
        });
    }

    /// Takes the ingredients of `crafts` of `recipe` out of the bag of player `entity`: for each
    /// ingredient, the item the player picked that fits it, else the first carried one that
    /// fits. Whether the player stands at a station of the recipe and the bag held them all
    /// (nothing is taken when not).
    fn take_ingredients(
        &mut self,
        entity: Entity,
        recipe: &Recipe,
        crafts: u32,
        picked: &[CraftIngredient],
    ) -> bool {
        let recipes = &self.content.recipes;
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return false;
        };
        let number = player.number;
        if !player
            .station
            .is_some_and(|station| recipe.made_at(station.station_type))
        {
            tracing::debug!(player = number, recipe = %recipe.id, "craft away from its station");
            return false;
        }
        let Some(gear) = player.gear.as_mut() else {
            return false;
        };
        let carried: Vec<String> = gear
            .carried()
            .into_iter()
            .map(|item| item.item_id)
            .collect();
        let mut takes = Vec::new();
        for ingredient in &recipe.ingredients {
            let needed = ingredient.quantity.saturating_mul(crafts);
            let fits = |item: &&String| recipes.fits(ingredient, item);
            let chosen = carried
                .iter()
                .filter(fits)
                .find(|item| picked.iter().any(|pick| pick.item == lowercase_crc32(item)))
                .or_else(|| carried.iter().find(fits));
            match chosen {
                Some(item) if gear.bag_count(item) >= needed => takes.push((item.clone(), needed)),
                _ => {
                    tracing::debug!(player = number, recipe = %recipe.id, ?ingredient, "craft without an ingredient");
                    return false;
                }
            }
        }
        for (item, quantity) in &takes {
            gear.take(item, *quantity);
        }
        true
    }

    /// The crafts whose time came at `now_ms` end: the output goes into the bag, the client gets
    /// the craft and an idle update, and the player's quest takes the craft.
    pub(super) fn crafts_due(&mut self, now_ms: u64) {
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.crafts)
            .into_iter()
            .partition(|craft| now_ms >= craft.due_ms);
        self.crafts = later;
        for craft in due {
            let Some(entity) = self.player_by_number(craft.number) else {
                continue;
            };
            self.give_loot(craft.number, vec![(craft.item, craft.quantity)], now_ms);
            if let Some(player) = self.world.get::<Player>(entity) {
                let messages = [
                    ServerMessage::CraftProgress(CraftProgressMsg::crafted(
                        craft.number,
                        &craft.recipe,
                        craft.quantity,
                    )),
                    ServerMessage::CraftProgress(CraftProgressMsg::idle(craft.number)),
                ];
                send_now(&mut self.combat_messages, &player.connection, &messages);
            }
            self.quest_event(craft.number, Event::Crafted(&craft.recipe), now_ms);
        }
    }

    /// Player `number` (`entity`) closes the crafting window: it leaves the station and gets an
    /// idle update.
    pub(super) fn close_station(&mut self, entity: Entity, number: u64) {
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.station = None;
        }
        self.send_idle(entity, number);
    }

    fn send_idle(&mut self, entity: Entity, number: u64) {
        if let Some(player) = self.world.get::<Player>(entity) {
            let idle = ServerMessage::CraftProgress(CraftProgressMsg::idle(number));
            send_now(&mut self.combat_messages, &player.connection, &[idle]);
        }
    }
}
