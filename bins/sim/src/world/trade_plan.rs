// SPDX-License-Identifier: AGPL-3.0-or-later
//! A trade skill as a game master or a test bot plays it: `!queststate trade:<tradeskill>` and
//! `!questplan trade:<tradeskill>` (the objective id [`TRADE_OBJECTIVE_PREFIX`] and a
//! tradeskill id, `trade:Weaponsmithing`) answer in the words of a quest's lines
//! ([`super::gm_chat`], [`super::quest_walk`]), so a bot plays the skill as it plays a quest,
//! from the raw material to the product:
//!
//! - a gathering skill (Logging, Mining, Harvesting): the gather of a node of its lowest
//!   gatherable that the player's rank may take;
//! - Skinning: the kill of a creature of the level and the skin of its corpse;
//! - Fishing: a pole worn, then a cast from a place whose cast lands in water;
//! - Musician: an instrument worn, then a song played to its end;
//! - a refining or crafting skill: the lowest recipe of the skill that the player's rank and
//!   achievements open and a station of the level makes. While the bag lacks an ingredient,
//!   the plan is that ingredient's own source: the gather of a node whose rolls give it, the
//!   recipe that makes it (its ingredients first, [`SUB_CRAFT_DEPTH`] recipes deep), or the
//!   skin of a creature whose skin gives it. With every ingredient in the bag, the plan is the
//!   craft at the nearest station that makes the recipe.
//!
//! The skill is done for a player once its experience in the tradeskill is above what it was
//! at the first `!queststate` of the objective ([`TradeRuns`]): the gather, the catch or the
//! craft gave its experience, as the game data says. The answer `done` ends the run, so the
//! next `!queststate` starts a new one. A running skill has one step line, whose progress is
//! the number of items the player carries: a gather or a craft that gave or took an item moved
//! the step on (a weapon takes several ingots, so the same craft comes again), and one that
//! changed nothing is the same step again, which a bot gives up after a few tries.
//!
//! **Inferred** (our choice, no retail rule): which gatherable and which recipe stand for a
//! skill (the lowest by required rank or recipe level, then by id).

use std::collections::HashMap;

use bevy_ecs::entity::Entity;
use nw_content::crafting::{Recipe, RecipeDetails};
use nw_content::fishing::FISHING_TRADESKILL;
use nw_content::gathering::Gatherable;
use nw_content::music::MUSIC_TRADESKILL;
use nw_formats::objectstream::lowercase_crc32;
use nw_sim_core::Position;

use super::gathering::SKINNING_TRADESKILL;
use super::gm_chat::{QUEST_DONE, QUEST_RUNNING, QUEST_STEP};
use super::quest_walk::{FlowEvent, Outcome, SOURCE_ROLLS, StepPlan};
use super::{Body, Player, SimWorld};

/// The start of the objective id of a trade skill: `trade:Smelting`.
pub(super) const TRADE_OBJECTIVE_PREFIX: &str = "trade:";
/// The task word of a trade skill's plan lines and of its step line.
const TRADE_TASK: &str = "trade";
/// The words after the progress of a trade skill's step line.
const CARRIED_WORDS: &str = "items carried";
/// How many recipes deep an ingredient's own recipe is planned (a weapon from ingots from
/// ore: 2; one more for charcoal and the like).
const SUB_CRAFT_DEPTH: u32 = 3;
/// One gather, one kill, one skin.
const ONE: u32 = 1;
/// The tradeskill word of a gatherable of any tradeskill ([`SimWorld::trade_gatherable`]).
const ANY_SKILL: &str = "";
/// In how many of the [`SOURCE_ROLLS`] rolls of a node's or a skin's table an ingredient drops
/// for that node or skin to be its source (Inferred: one gather in four; a rare drop, as a gem
/// of an ore vein or a trophy part of a skin, is no source a player crafts from).
const COMMON_HITS: u32 = SOURCE_ROLLS / 4;
/// How many kinds of creature, nearest the player first, a skin is looked for among
/// (Inferred: each kind asks the level's spawns once, and the answer comes in the sim's tick).
const SKIN_CREATURES: usize = 24;

/// The tradeskill of trade objective `objective` (`trade:<tradeskill>`); `None` for another
/// id.
pub(super) fn trade_skill(objective: &str) -> Option<&str> {
    objective
        .get(..TRADE_OBJECTIVE_PREFIX.len())
        .filter(|prefix| prefix.eq_ignore_ascii_case(TRADE_OBJECTIVE_PREFIX))
        .and_then(|_| objective.get(TRADE_OBJECTIVE_PREFIX.len()..))
        .filter(|skill| !skill.is_empty())
}

/// The trade skill runs of this sim's players: the tradeskill experience of each at the first
/// `!queststate` of its objective, by player number and lowercase tradeskill id.
#[derive(Debug, Default)]
pub(super) struct TradeRuns {
    starts: HashMap<(u64, String), u32>,
}

impl TradeRuns {
    /// Whether the run of `skill` by player `number` is done at experience `experience`: above
    /// the experience at its start. A first call starts the run; a done run is forgotten.
    fn done(&mut self, number: u64, skill: &str, experience: u32) -> bool {
        let key = (number, skill.to_lowercase());
        let start = *self.starts.entry(key.clone()).or_insert(experience);
        let done = experience > start;
        if done {
            self.starts.remove(&key);
        }
        done
    }
}

fn stop(outcome: Outcome, reason: String) -> StepPlan {
    StepPlan::Stop(outcome, reason)
}

/// One gather of a node of `gatherable` for `item`.
fn gather_of(gatherable: &Gatherable, item: &str) -> StepPlan {
    StepPlan::Item {
        item: item.to_owned(),
        count: ONE,
        gathered: true,
        source: format!("gathered from {}", gatherable.id),
        node: Some(gatherable.id.clone()),
    }
}

impl SimWorld {
    /// The experience of player `number` in tradeskill `skill`; 0 for a player or a
    /// tradeskill this sim does not know.
    fn trade_experience(&mut self, number: u64, skill: &str) -> u32 {
        let entity = self.player_by_number(number);
        let mastery = self.content.progression.tradeskill(skill);
        let player = entity.and_then(|entity| self.world.get::<Player>(entity));
        match (mastery, player) {
            (Some(mastery), Some(player)) => player.masteries.experience(mastery),
            _ => 0,
        }
    }

    /// The `!queststate` lines of trade objective `objective` of tradeskill `skill` for player
    /// `number` (module text).
    pub(super) fn trade_state_lines(
        &mut self,
        number: u64,
        objective: &str,
        skill: &str,
    ) -> Vec<String> {
        let experience = self.trade_experience(number, skill);
        if self.trade_runs.done(number, skill, experience) {
            tracing::info!(player = number, skill, experience, "trade skill run done");
            return vec![format!("{QUEST_DONE} {objective}")];
        }
        let carried = self.carried_total(number);
        vec![
            format!("{QUEST_RUNNING} {objective}"),
            format!("{QUEST_STEP} {TRADE_TASK} {carried} {CARRIED_WORDS}"),
        ]
    }

    /// How many items player `number` carries, worn and in the bag: the progress of a trade
    /// skill's step (module text).
    fn carried_total(&mut self, number: u64) -> u32 {
        self.player_by_number(number)
            .and_then(|entity| self.world.get::<Player>(entity))
            .and_then(|player| player.gear.as_ref())
            .map_or(0, |gear| {
                gear.carried()
                    .iter()
                    .fold(0_u32, |total, item| total.saturating_add(item.quantity))
            })
    }

    /// The `!questplan` line of tradeskill `skill` for player `number`: what the player does
    /// next (module text).
    pub(crate) fn trade_plan_lines(&mut self, number: u64, skill: &str) -> Vec<String> {
        let plan = match self.player_by_number(number) {
            Some(entity) => self.trade_plan(entity, skill),
            None => stop(Outcome::Broken, format!("no player {number}")),
        };
        vec![self.plan_line(TRADE_TASK, &plan)]
    }

    fn trade_plan(&self, entity: Entity, skill: &str) -> StepPlan {
        let Some(at) = self
            .world
            .get::<Body>(entity)
            .map(|body| body.motion.position())
        else {
            return stop(Outcome::Broken, "the player has no body".to_owned());
        };
        if self.content.progression.tradeskill(skill).is_none() {
            return stop(Outcome::Broken, format!("no tradeskill {skill}"));
        }
        if skill.eq_ignore_ascii_case(FISHING_TRADESKILL) {
            return self.trade_fish(entity, at);
        }
        if skill.eq_ignore_ascii_case(SKINNING_TRADESKILL) {
            return self.trade_skin(entity, at, None);
        }
        if skill.eq_ignore_ascii_case(MUSIC_TRADESKILL) {
            return self.trade_song(entity, at);
        }
        if let Some(gatherable) = self.trade_gatherable(entity, skill, None) {
            return gather_of(gatherable, &gatherable.id);
        }
        match self.trade_recipe(entity, at, |_, details| {
            details
                .tradeskill
                .as_deref()
                .is_some_and(|known| known.eq_ignore_ascii_case(skill))
                && self.raises(details.game_event.as_deref(), skill)
        }) {
            Some(recipe) => self.trade_craft(entity, at, recipe, SUB_CRAFT_DEPTH),
            None => stop(
                Outcome::NeedsLook,
                format!(
                    "no gatherable and no recipe of {skill} that the player's rank and achievements open, the level has a node or a station for, and whose ingredients have a source"
                ),
            ),
        }
    }

    /// A cast from a place near `at` whose cast lands in water, with a pole worn; before it,
    /// the equip of the bag's pole; with no pole, the craft of the lowest pole recipe (its
    /// ingredients first), as a player makes its first pole.
    fn trade_fish(&self, entity: Entity, at: Position) -> StepPlan {
        if self.wears_pole(entity) {
            return match self.fishing_place(entity, at, None) {
                Some((place, yaw)) => StepPlan::Fish {
                    at: place,
                    yaw,
                    territory: None,
                    event: Box::new(StepPlan::Wait),
                },
                None => stop(
                    Outcome::NeedsLook,
                    format!(
                        "no place within {:.0} m whose cast lands in water",
                        super::fishing::FISHING_PLACE_SEARCH_M
                    ),
                ),
            };
        }
        if let Some(equip) = self.bag_pole_equip(entity) {
            return equip;
        }
        let makes_pole =
            |recipe: &Recipe, _: &RecipeDetails| self.content.fishing.pole(&recipe.item).is_some();
        match self.trade_recipe(entity, at, makes_pole) {
            Some(recipe) => self.trade_craft(entity, at, recipe, SUB_CRAFT_DEPTH),
            None => stop(
                Outcome::NeedsLook,
                "the bag holds no fishing pole, and no pole recipe is open to the player"
                    .to_owned(),
            ),
        }
    }

    /// A song at `at`: the first song by id whose completion event gives Musician experience,
    /// on a sheet of the instrument player `entity` wears; before it, the equip of a bag
    /// instrument or the instrument's item step ([`SimWorld::song_for_event`]).
    fn trade_song(&self, entity: Entity, at: Position) -> StepPlan {
        let song = self
            .content
            .music
            .songs()
            .filter(|song| self.raises(Some(&song.completion_event), MUSIC_TRADESKILL))
            .min_by(|left, right| left.id.cmp(&right.id));
        match song {
            Some(song) => self.song_for_event(
                Some(entity),
                StepPlan::Event(
                    FlowEvent::Song {
                        song: song.id.clone(),
                        at,
                    },
                    ONE,
                ),
            ),
            None => stop(
                Outcome::NeedsLook,
                "no song's completion event gives Musician experience".to_owned(),
            ),
        }
    }

    /// Whether game event `event` gives experience in tradeskill `skill` (its
    /// `CategoricalProgressionId`): the event of a gather or a craft that raises the skill.
    fn raises(&self, event: Option<&str>, skill: &str) -> bool {
        event
            .and_then(|event| self.content.event_grants.get(event))
            .and_then(|grant| grant.progression.as_ref())
            .is_some_and(|(id, reward)| id.eq_ignore_ascii_case(skill) && *reward > 0.0)
    }

    /// Whether `gatherable` is a node of its trade on this level: its gather event gives its
    /// own tradeskill experience ([`Self::raises`]; a quest node's event gives none), it has a
    /// loot table and a node, and it is no container.
    fn trade_node(&self, gatherable: &Gatherable) -> bool {
        gatherable
            .tradeskill
            .as_deref()
            .is_some_and(|own| self.raises(gatherable.game_event.as_deref(), own))
            && gatherable.loot_table.is_some()
            && !gatherable.loot_container
            && self.node_count(gatherable) > 0
    }

    /// How many nodes of `gatherable` the level has.
    fn node_count(&self, gatherable: &Gatherable) -> u32 {
        self.content
            .gather_points
            .count(lowercase_crc32(&gatherable.id))
    }

    /// The rank `gatherable` asks of its tradeskill.
    fn node_rank(&self, gatherable: &Gatherable) -> u32 {
        self.content
            .gather_rules
            .of(lowercase_crc32(&gatherable.id))
            .required_rank
    }

    /// Whether the rank of player `entity` may take `gatherable` with no item of its own.
    fn may_gather(&self, entity: Entity, gatherable: &Gatherable) -> bool {
        let rule = self
            .content
            .gather_rules
            .of(lowercase_crc32(&gatherable.id));
        let rank = gatherable
            .tradeskill
            .as_deref()
            .map_or(0, |skill| self.tradeskill_perks(entity, skill).0);
        rule.required_item.is_none() && rule.required_rank <= u32::from(rank)
    }

    /// The gatherable that stands for tradeskill `skill` ([`ANY_SKILL`]: any tradeskill but
    /// Skinning), a node of its trade ([`Self::trade_node`]) that player `entity` may take
    /// ([`Self::may_gather`]): the lowest by required rank, then the one with the most nodes
    /// on the level (the trade's common node), then by id. With `giving`, only one whose rolls
    /// give that item in [`COMMON_HITS`] rolls or more, the one that gives it most often
    /// before the others.
    fn trade_gatherable(
        &self,
        entity: Entity,
        skill: &str,
        giving: Option<&str>,
    ) -> Option<&Gatherable> {
        let content = &self.content;
        let hits = |gatherable: &Gatherable| match (giving, gatherable.loot_table.as_deref()) {
            (Some(item), Some(table)) if content.loot.table_drops(table, item) => {
                self.roll_hits(table, item)
            }
            (Some(_), _) => 0,
            (None, _) => ONE,
        };
        content
            .gatherables
            .iter()
            .filter(|gatherable| {
                gatherable.tradeskill.as_deref().is_some_and(|known| {
                    known != SKINNING_TRADESKILL
                        && (skill == ANY_SKILL || known.eq_ignore_ascii_case(skill))
                }) && self.trade_node(gatherable)
                    && self.may_gather(entity, gatherable)
            })
            .map(|gatherable| (hits(gatherable), gatherable))
            .filter(|(hits, _)| *hits >= giving.map_or(ONE, |_| COMMON_HITS))
            .min_by(|(left_hits, left), (right_hits, right)| {
                right_hits
                    .cmp(left_hits)
                    .then_with(|| self.node_rank(left).cmp(&self.node_rank(right)))
                    .then_with(|| self.node_count(right).cmp(&self.node_count(left)))
                    .then_with(|| left.id.cmp(&right.id))
            })
            .map(|(_, gatherable)| gatherable)
    }

    /// The skins of the game data that player `entity` may take: with `giving`, those whose
    /// rolls give that item in [`COMMON_HITS`] rolls or more; with none, those whose event
    /// gives Skinning experience ([`Self::raises`]).
    fn trade_skins(&self, entity: Entity, giving: Option<&str>) -> Vec<&Gatherable> {
        let content = &self.content;
        content
            .gatherables
            .iter()
            .filter(|gatherable| {
                gatherable.tradeskill.as_deref() == Some(SKINNING_TRADESKILL)
                    && self.may_gather(entity, gatherable)
                    && match (giving, gatherable.loot_table.as_deref()) {
                        (Some(item), Some(table)) => self.commonly_drops(table, item),
                        (Some(_), None) => false,
                        (None, _) => {
                            self.raises(gatherable.game_event.as_deref(), SKINNING_TRADESKILL)
                        }
                    }
            })
            .collect()
    }

    /// The kill of a creature of the level and the skin of its corpse: the creature nearest
    /// `at`, of the [`SKIN_CREATURES`] nearest kinds, whose corpse has one of the skins of
    /// [`Self::trade_skins`].
    fn trade_skin(&self, entity: Entity, at: Position, giving: Option<&str>) -> StepPlan {
        let content = &self.content;
        let none = || {
            stop(
                Outcome::NeedsLook,
                format!(
                    "none of the {SKIN_CREATURES} kinds of creature nearest the player has a skin its rank may take{}",
                    giving
                        .map(|item| format!(" that gives {item}"))
                        .unwrap_or_default()
                ),
            )
        };
        let skins = self.trade_skins(entity, giving);
        if skins.is_empty() {
            return none();
        }
        let mut nearest: HashMap<&str, f32> = HashMap::new();
        for spawn in self.combat.spawns.iter() {
            if let Some((enemy, _)) = content.kill_types.of(spawn.vitals_id) {
                let [x, y, z] = spawn.position;
                let distance = Position { x, y, z }.horizontal_distance(at);
                let known = nearest.entry(enemy).or_insert(distance);
                *known = known.min(distance);
            }
        }
        let mut enemies: Vec<(&str, f32)> = nearest.into_iter().collect();
        enemies.sort_by(|(left, left_m), (right, right_m)| {
            left_m.total_cmp(right_m).then_with(|| left.cmp(right))
        });
        let skinned = enemies
            .into_iter()
            .take(SKIN_CREATURES)
            .find_map(|(enemy, _)| {
                let id = self.skinning_gatherable(enemy)?;
                let gatherable = skins
                    .iter()
                    .find(|skin| skin.id.eq_ignore_ascii_case(&id))?;
                let kill = self.walk_kill(&content.quest_guide, enemy, ONE, None);
                matches!(kill, StepPlan::Kill { .. }).then_some((*gatherable, kill))
            });
        match skinned {
            Some((gatherable, kill)) => StepPlan::Skin {
                gatherable: gatherable.id.clone(),
                count: ONE,
                kill: Box::new(kill),
                item: Box::new(gather_of(gatherable, giving.unwrap_or(&gatherable.id))),
            },
            None => none(),
        }
    }

    /// Whether the rolls of loot table `table` give `item` in [`COMMON_HITS`] rolls or more.
    fn commonly_drops(&self, table: &str, item: &str) -> bool {
        self.content.loot.table_drops(table, item) && self.roll_hits(table, item) >= COMMON_HITS
    }

    /// Whether a player gets `item` by a gather, a craft (`depth` recipes deep) or a skin
    /// that commonly gives it ([`Self::commonly_drops`]); no creature is looked for.
    fn has_source(&self, entity: Entity, item: &str, depth: u32) -> bool {
        let content = &self.content;
        let dropped_by = |gatherable: &Gatherable| {
            gatherable
                .loot_table
                .as_deref()
                .is_some_and(|table| self.commonly_drops(table, item))
        };
        content.gatherables.iter().any(|gatherable| {
            gatherable.tradeskill.as_deref() != Some(SKINNING_TRADESKILL)
                && self.trade_node(gatherable)
                && self.may_gather(entity, gatherable)
                && dropped_by(gatherable)
        }) || (depth > 0
            && content
                .recipes
                .making(item)
                .is_some_and(|recipe| self.craftable(entity, recipe, depth - 1)))
            || !self.trade_skins(entity, Some(item)).is_empty()
    }

    /// Whether player `entity` may craft `recipe` and each ingredient has a source
    /// ([`Self::has_source`]).
    fn craftable(&self, entity: Entity, recipe: &Recipe, depth: u32) -> bool {
        self.may_craft(entity, recipe)
            && recipe
                .ingredients
                .iter()
                .all(|ingredient| self.sourced_item(entity, ingredient, depth).is_some())
    }

    /// The first item that `ingredient` takes and that has a source ([`Self::has_source`]): of
    /// a category, a player crafts with the item it can get (a light ration takes any raw
    /// food: pork from a boar, not the egg a bird's skin seldom gives).
    fn sourced_item(
        &self,
        entity: Entity,
        ingredient: &nw_content::crafting::Ingredient,
        depth: u32,
    ) -> Option<String> {
        self.content
            .recipes
            .items_for(ingredient)
            .into_iter()
            .find(|item| self.has_source(entity, item, depth))
            .map(|item| {
                self.content
                    .kits
                    .item(&item)
                    .map_or(item, |known| known.item_id.clone())
            })
    }

    /// The lowest recipe by recipe level, then by id, among those that `wanted` takes, with
    /// no cooldown, that a station of the level makes and player `entity` may craft from
    /// sources it has ([`Self::craftable`]).
    fn trade_recipe(
        &self,
        entity: Entity,
        at: Position,
        wanted: impl Fn(&Recipe, &RecipeDetails) -> bool,
    ) -> Option<&Recipe> {
        let recipes = &self.content.recipes;
        let mut candidates: Vec<(u32, &Recipe)> = recipes
            .iter()
            .filter_map(|recipe| {
                let details = recipes.details(lowercase_crc32(&recipe.id));
                (details.cooldown.is_none()
                    && wanted(recipe, &details)
                    && self
                        .stations
                        .nearest(at, |station| recipe.made_at(station))
                        .is_some())
                .then_some((details.recipe_level, recipe))
            })
            .collect();
        candidates.sort_by(|(left_level, left), (right_level, right)| {
            left_level
                .cmp(right_level)
                .then_with(|| left.id.cmp(&right.id))
        });
        candidates
            .into_iter()
            .map(|(_, recipe)| recipe)
            .find(|recipe| self.craftable(entity, recipe, SUB_CRAFT_DEPTH))
    }

    /// Whether the tradeskill rank and the achievements of player `entity` open `recipe`.
    fn may_craft(&self, entity: Entity, recipe: &Recipe) -> bool {
        let details = self.content.recipes.details(lowercase_crc32(&recipe.id));
        let level_reached = details.tradeskill.as_deref().is_none_or(|tradeskill| {
            self.tradeskill_perks(entity, tradeskill).1.recipe_level >= details.recipe_level
        });
        level_reached
            && self.world.get::<Player>(entity).is_some_and(|player| {
                details.open_to(|id| player.flow.has_achievement(&self.achievement_rules, id))
            })
    }

    /// The craft of `recipe` at the nearest station to `at` that makes it, from the bag of
    /// player `entity`; while the bag lacks an ingredient, that ingredient's source
    /// ([`Self::trade_input`]), its own recipe at most `depth` recipes deep.
    fn trade_craft(&self, entity: Entity, at: Position, recipe: &Recipe, depth: u32) -> StepPlan {
        let Some(station) = self.stations.nearest(at, |station| recipe.made_at(station)) else {
            return stop(
                Outcome::NeedsLook,
                format!("no station of the level makes {}", recipe.id),
            );
        };
        let gear = self
            .world
            .get::<Player>(entity)
            .and_then(|player| player.gear.as_ref());
        let mut ingredients = Vec::new();
        for ingredient in &recipe.ingredients {
            let Some((item, held)) = self.ingredient_item(gear, ingredient) else {
                return stop(
                    Outcome::Broken,
                    format!("no item fits an ingredient of {}", recipe.id),
                );
            };
            if held < ingredient.quantity {
                // The carried item when the bag holds some of it, else one with a source.
                let wanted = Some(item)
                    .filter(|_| held > 0)
                    .or_else(|| self.sourced_item(entity, ingredient, depth));
                return match wanted {
                    Some(item) => self.trade_input(entity, at, &item, depth),
                    None => stop(
                        Outcome::NeedsLook,
                        format!("no item of an ingredient of {} has a source", recipe.id),
                    ),
                };
            }
            ingredients.push((item, ingredient.quantity));
        }
        StepPlan::Craft {
            recipe: recipe.id.clone(),
            station: station.gde_ref,
            at: station.position,
            ingredients,
            missing: Vec::new(),
            event: Box::new(StepPlan::Wait),
        }
    }

    /// Where player `entity` gets ingredient `item`: the gather of a node whose rolls give it,
    /// else the recipe that makes it (with `depth` left), else the skin that gives it.
    fn trade_input(&self, entity: Entity, at: Position, item: &str, depth: u32) -> StepPlan {
        if let Some(gatherable) = self.trade_gatherable(entity, ANY_SKILL, Some(item)) {
            return gather_of(gatherable, item);
        }
        let made = self
            .content
            .recipes
            .making(item)
            .filter(|recipe| depth > 0 && self.may_craft(entity, recipe));
        match made {
            Some(recipe) => self.trade_craft(entity, at, recipe, depth - 1),
            None => self.trade_skin(entity, at, Some(item)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nw_content::crafting::{Ingredient, IngredientKind, RecipeDetails, Recipes};
    use nw_content::gathering::Gatherables;
    use nw_content::loot::{LootRow, LootSource, LootTables};
    use nw_content::progression::{Progression, RankPerks, TradeskillRankRow, TradeskillSource};
    use nw_content::rewards::{EventGrant, EventGrants};
    use nw_messages::wire::RAW_ID_LEN;

    use super::super::crafting::Station;
    use super::super::gear::tests::{BACKSTORY, kits_with};
    use super::super::tests::{CELL_M, NOW, admit_at, world};
    use super::*;
    use crate::placed_points::PlacedPoints;

    const PLAYER: u64 = 1;
    const MINING: &str = "Mining";
    const SMELTING: &str = "Smelting";
    const WEAPONSMITHING: &str = "Weaponsmithing";
    const ORE: &str = "OreT1";
    const INGOT: &str = "IngotT1";
    const SWORD: &str = "SwordT1";
    const VEIN: &str = "Test_Vein";
    const QUEST_VEIN: &str = "A_Quest_Vein";
    const QUEST_EVENT: &str = "Quest_Vein_interact";
    const MINE_EVENT: &str = "Gather_Mining";
    const SMELT_EVENT: &str = "Craft_Smelting";
    const FORGE_EVENT: &str = "Craft_Weaponsmithing";
    const EVENT_EXPERIENCE: f64 = 4.0;
    const SMELT: &str = "Smelt_Ingot";
    const FORGE: &str = "Forge_Sword";
    const MASTER_FORGE: &str = "A_Master_Sword";
    const ORE_PER_INGOT: u32 = 2;
    const MASTER_LEVEL: u32 = 5;
    const SMELTER: &str = "smelting1";
    const FORGE_STATION: &str = "blacksmith1";
    const SMELTER_ID: u8 = 1;
    const FORGE_ID: u8 = 2;
    const TOP_RANK: i64 = 200;
    const RANKS: i64 = 4;
    const RANK_CAP: f64 = 100.0;
    const MAX_ROLL: i64 = 100;
    const ALWAYS: &str = "0";

    fn at(x: f32) -> Position {
        Position {
            x,
            y: 10.0,
            z: 50.0,
        }
    }

    fn recipe(id: &str, item: &str, input: &str, quantity: u32, station: &str) -> Recipe {
        Recipe {
            id: id.to_owned(),
            item: item.to_owned(),
            quantity: 1,
            ingredients: vec![Ingredient {
                kind: IngredientKind::Item(input.to_owned()),
                quantity,
            }],
            stations: vec![station.to_owned()],
        }
    }

    fn details(tradeskill: &str, recipe_level: u32, event: &str) -> RecipeDetails {
        RecipeDetails {
            tradeskill: Some(tradeskill.to_owned()),
            recipe_level,
            game_event: Some(event.to_owned()),
            ..RecipeDetails::default()
        }
    }

    fn vein(id: &str, game_event: Option<&str>) -> Gatherable {
        Gatherable {
            id: id.to_owned(),
            gather_ms: 0,
            respawn_ms: (0, 0),
            loot_table: Some(VEIN.to_owned()),
            loot_container: false,
            tradeskill: Some(MINING.to_owned()),
            tool_required: false,
            by_hand: false,
            game_event: game_event.map(str::to_owned),
            animation: None,
        }
    }

    /// A world with a vein that gives ore and Mining experience, a quest vein whose event gives
    /// none, a smelter that makes an ingot of two ore and a forge that makes a sword of one
    /// ingot, and player [`PLAYER`] with an empty bag.
    fn trade_world() -> SimWorld {
        let row = |id: &str, cell: &str, and_or: Option<&str>, max_roll: Option<i64>| LootRow {
            id: id.to_owned(),
            and_or: and_or.map(str::to_owned),
            max_roll,
            cells: [(1, cell.to_owned())].into(),
            ..LootRow::default()
        };
        let mut world = world();
        let mut content = (*world.content).clone();
        content.kits = kits_with(&[(ORE, "Resource"), (INGOT, "Resource")]);
        content.progression =
            Progression::default().with_tradeskills([MINING, SMELTING, WEAPONSMITHING].map(|id| {
                TradeskillSource {
                    id: id.to_owned(),
                    max_levels: vec![TOP_RANK],
                    ranks: (0..RANKS)
                        .map(|rank| TradeskillRankRow {
                            rank,
                            cap: RANK_CAP,
                            perks: RankPerks::default(),
                        })
                        .collect(),
                }
            }));
        content.loot = LootTables::build(LootSource {
            rows: vec![
                row(VEIN, ORE, Some("AND"), None),
                row(&format!("{VEIN}_Qty"), "1", None, None),
                row(&format!("{VEIN}_Probs"), ALWAYS, None, Some(MAX_ROLL)),
            ],
            bucket_items: Vec::new(),
        });
        content.gatherables = Gatherables::build([
            vein(VEIN, Some(MINE_EVENT)),
            vein(QUEST_VEIN, Some(QUEST_EVENT)),
        ]);
        content.event_grants = EventGrants::build(
            [
                (MINE_EVENT, MINING),
                (SMELT_EVENT, SMELTING),
                (FORGE_EVENT, WEAPONSMITHING),
            ]
            .map(|(event, skill)| {
                (
                    event.to_owned(),
                    EventGrant {
                        progression: Some((skill.to_owned(), EVENT_EXPERIENCE)),
                        ..EventGrant::default()
                    },
                )
            }),
        );
        content.gather_points = PlacedPoints::of([
            (lowercase_crc32(VEIN), at(CELL_M / 2.0)),
            (lowercase_crc32(QUEST_VEIN), at(CELL_M / 2.0)),
        ]);
        content.recipes = Recipes::build(
            [
                recipe(SMELT, INGOT, ORE, ORE_PER_INGOT, SMELTER),
                recipe(FORGE, SWORD, INGOT, 1, FORGE_STATION),
                recipe(MASTER_FORGE, SWORD, INGOT, 1, FORGE_STATION),
            ],
            Vec::new(),
        )
        .with_details([
            (SMELT.to_owned(), details(SMELTING, 0, SMELT_EVENT)),
            (FORGE.to_owned(), details(WEAPONSMITHING, 0, FORGE_EVENT)),
            (
                MASTER_FORGE.to_owned(),
                details(WEAPONSMITHING, MASTER_LEVEL, FORGE_EVENT),
            ),
        ]);
        world.content = Arc::new(content);
        let mut admit = admit_at(PLAYER, CELL_M / 2.0, true);
        admit.backstory = Some(BACKSTORY.to_owned());
        world.admit(&admit, NOW, NOW).expect("admit");
        for (id, station_type) in [(SMELTER_ID, SMELTER), (FORGE_ID, FORGE_STATION)] {
            world.stations.add_level_entity(Station {
                gde_ref: [id; RAW_ID_LEN],
                position: at(CELL_M / 2.0 + f32::from(id)),
                station_type: station_type.to_owned(),
                name: Vec::new(),
                members: Vec::new(),
            });
        }
        world
    }

    fn give(world: &mut SimWorld, item: &str, quantity: u32) {
        let entity = world.player_by_number(PLAYER).expect("player");
        let known = world.content.kits.item(item).cloned().expect("item");
        world
            .world
            .get_mut::<Player>(entity)
            .and_then(|mut player| player.gear.as_mut().map(|gear| gear.add(&known, quantity)))
            .expect("gear");
    }

    fn plan(world: &mut SimWorld, skill: &str) -> String {
        world.trade_plan_lines(PLAYER, skill).remove(0)
    }

    #[test]
    fn a_trade_objective_names_its_tradeskill_and_a_quest_id_names_none() {
        assert_eq!(trade_skill("trade:Smelting"), Some(SMELTING));
        assert_eq!(trade_skill("TRADE:Smelting"), Some(SMELTING));
        assert_eq!(trade_skill("trade:"), None);
        assert_eq!(trade_skill("12A_MSQ_09"), None);
    }

    #[test]
    fn a_crafting_skill_is_played_from_the_node_to_the_product() {
        let mut world = trade_world();
        // An empty bag: the sword needs an ingot, the ingot needs ore, the vein gives ore.
        let gather = plan(&mut world, WEAPONSMITHING);
        assert!(
            gather.contains(&format!("item {ORE} 1 gathered node {VEIN}")),
            "{gather}"
        );
        // With the ore: the ingot at the smelter.
        give(&mut world, ORE, ORE_PER_INGOT);
        let smelt = plan(&mut world, WEAPONSMITHING);
        assert!(smelt.contains(&format!("craft {SMELT} ")), "{smelt}");
        assert!(
            smelt.contains(&format!("with {ORE} {ORE_PER_INGOT}")) && !smelt.contains(" give "),
            "{smelt}"
        );
        // With the ingot: the lowest sword recipe at the forge, not the one above the rank.
        give(&mut world, INGOT, 1);
        let forge = plan(&mut world, WEAPONSMITHING);
        assert!(forge.contains(&format!("craft {FORGE} ")), "{forge}");
        assert!(
            forge.contains(&format!("{FORGE_ID:02x}").repeat(RAW_ID_LEN)),
            "{forge}"
        );
    }

    #[test]
    fn a_gathering_skill_is_the_gather_of_its_node_and_never_of_a_quest_node() {
        let mut world = trade_world();
        let mine = plan(&mut world, MINING);
        assert!(
            mine.contains(&format!("gathered node {VEIN} ")) && !mine.contains(QUEST_VEIN),
            "{mine}"
        );
    }

    #[test]
    fn a_skill_the_data_does_not_hold_and_a_skill_with_nothing_to_do_are_stops() {
        let mut world = trade_world();
        let unknown = plan(&mut world, "Juggling");
        assert!(unknown.contains("no tradeskill Juggling"), "{unknown}");
        let mut content = (*world.content).clone();
        content.recipes = Recipes::default();
        world.content = Arc::new(content);
        let nothing = plan(&mut world, SMELTING);
        assert!(nothing.contains("no gatherable and no recipe"), "{nothing}");
    }

    #[test]
    fn a_skill_is_done_when_its_experience_rose_and_the_next_run_starts_again() {
        const OBJECTIVE: &str = "trade:Smelting";
        const GAINED: u32 = 5;
        let mut world = trade_world();
        let state = |world: &mut SimWorld| world.trade_state_lines(PLAYER, OBJECTIVE, SMELTING);
        let running = state(&mut world);
        assert_eq!(running[0], format!("{QUEST_RUNNING} {OBJECTIVE}"));
        assert!(running[1].starts_with("step trade "), "{running:?}");
        assert_eq!(
            state(&mut world),
            running,
            "no experience yet, the same step"
        );
        give(&mut world, ORE, ORE_PER_INGOT);
        let moved_on = state(&mut world);
        assert_eq!(moved_on[0], running[0]);
        assert_ne!(moved_on[1], running[1], "an item more: the step moved on");
        let running = moved_on;
        let entity = world.player_by_number(PLAYER).expect("player");
        let content = Arc::clone(&world.content);
        let mastery = content.progression.tradeskill(SMELTING).expect("skill");
        world
            .world
            .get_mut::<Player>(entity)
            .expect("player")
            .masteries
            .set(mastery, GAINED);
        assert_eq!(state(&mut world), vec![format!("{QUEST_DONE} {OBJECTIVE}")]);
        assert_eq!(state(&mut world), running, "a new run");
    }
}

#[cfg(all(test, feature = "local-data"))]
mod retail_tests {
    use super::super::tests::{NOW, admit_at};
    use crate::quest_walk::tests::retail_world;

    const PLAYER: u64 = 1;
    /// A place in the Monarch's Bluffs settlement (a bot's camp of 2026-11-23).
    const TOWN: [f32; 3] = [7420.6, 3623.2, 104.2];
    /// The trade skills of the game data: gathering, refining and crafting.
    const SKILLS: [&str; 18] = [
        "Logging",
        "Mining",
        "Harvesting",
        "Skinning",
        "Fishing",
        "Smelting",
        "Woodworking",
        "Leatherworking",
        "Weaving",
        "Stonecutting",
        "Weaponsmithing",
        "Armoring",
        "Engineering",
        "Jewelcrafting",
        "Arcana",
        "Cooking",
        "Furnishing",
        "Musician",
    ];

    /// A new character in Monarch's Bluffs gets a first thing to do for every trade skill of
    /// the game data: none is a stop.
    #[test]
    fn every_trade_skill_of_the_game_data_has_a_first_step_for_a_new_character() {
        let (_, mut world) = retail_world("trade-plan");
        let mut admit = admit_at(PLAYER, TOWN[0], true);
        admit.position.y = TOWN[1];
        admit.position.z = TOWN[2];
        admit.backstory = Some("Archetype_Soldier".to_owned());
        world.admit(&admit, NOW, NOW).expect("admit");
        // The settlements' stations stand after the first tick of them.
        world.tick_town_stations();
        let plans: Vec<(&str, String)> = SKILLS
            .iter()
            .map(|skill| (*skill, world.trade_plan_lines(PLAYER, skill).remove(0)))
            .collect();
        let stops: Vec<&(&str, String)> = plans
            .iter()
            .filter(|(_, plan)| plan.contains(" stop "))
            .collect();
        assert!(stops.is_empty(), "{stops:#?}\nall: {plans:#?}");
    }
}
