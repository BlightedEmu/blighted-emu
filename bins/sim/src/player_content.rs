// SPDX-License-Identifier: AGPL-3.0-or-later
//! The content a sim needs for its players besides combat: the starter kit of each archetype
//! pick, the bit of each achievement, the look of each mob prefab, and the quests.

use nw_content::crafting::{Recipes, StructureCosts};
use nw_content::creature_animations::CreatureAnimations;
use nw_content::gameplay_config::GameplayConfig;
use nw_content::gathering::{CrateLoot, CrateVariant, Gatherables};
use nw_content::leaderboards::LeaderboardStats;
use nw_content::loot::{LootTables, MobLoot, PlayerLootLevels};
use nw_content::loot_limits::{LootLimitRules, LootLimits, ResetClock};
use nw_content::poi_tags::PoiTags;
use nw_content::progression::Progression;
use nw_content::rewards::{KillExperience, Rewards};
use nw_formats::objectstream::lowercase_crc32;
use nw_sim_core::Position;
use nw_world::records::{GatherablePoint, WorldFile};
use nw_world::stations::CraftingStation;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use nw_content::Tables;
use nw_content::achievements::AchievementIndexes;
use nw_content::ftue_entities::FtueLevel;
use nw_content::mob_prefabs::{ASSET_ID_LEN, MobPrefabs, PrefabKey, PrefabMember};
use nw_content::starter::StarterKits;
use nw_messages::{AlcState, AssetId, BundleMember, MemberState, VitalsState};

use crate::objectives::{
    KillTypes, ObjectiveTree, TaskBook, TaskRow, counted_game_events, task_rows,
};

/// The tutorial quest's objective id (`Objectives`).
pub const TUTORIAL_OBJECTIVE: &str = "01_SoleSurvivor2";
/// The first open-world quest, which a player gets when it leaves the tutorial (retail: in the
/// first open-world state, xp-potion-exit note, section 3.3).
pub const OPEN_WORLD_OBJECTIVE: &str = "12_GetOffBeach";
/// Its root task.
pub const OPEN_WORLD_QUEST_ROOT: &str = "12_getoffbeach_container";
/// The root task of the tutorial quest `01_SoleSurvivor2` (live-tutorial note, appendix A).
pub const TUTORIAL_QUEST_ROOT: &str = "01_SoleSurvivor2_Container";

/// One objective a player can run (admin studio spec section 6.3).
#[derive(Debug, Clone, PartialEq)]
pub struct QuestDef {
    /// `ObjectiveID`.
    pub id: String,
    /// Its task tree.
    pub tree: Arc<ObjectiveTree>,
    /// `ItemRewardName` and `ItemRewardQty`, given at completion.
    pub reward: Option<(String, u32)>,
    /// `Repeatable`: it can run again after it is done.
    pub repeatable: bool,
    /// `RequiredAchievementID`: the achievement expression an NPC offers it on.
    pub requirement: Option<String>,
    /// `AchievementId`: set at completion; the next quest of the chain requires it.
    pub achievement: Option<String>,
    /// `NPCDestinationId`: the NPC the player turns it in at; without one it is done when its
    /// tree is.
    pub turn_in: Option<String>,
    /// `SuccessGameEventID`: the game event whose reward the completion gives.
    pub success_event: Option<String>,
}

/// The `Objectives` cells a quest needs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObjectiveRow {
    /// `ObjectiveID`.
    pub id: String,
    /// `Task`: the root task.
    pub root: String,
    /// `ItemRewardName`.
    pub reward_item: Option<String>,
    /// `ItemRewardQty`.
    pub reward_quantity: Option<i64>,
    /// `Repeatable`.
    pub repeatable: bool,
    /// `RequiredAchievementID`.
    pub requirement: Option<String>,
    /// `AchievementId`.
    pub achievement: Option<String>,
    /// `NPCDestinationId`.
    pub turn_in: Option<String>,
    /// `SuccessGameEventID`.
    pub success_event: Option<String>,
}

/// Every objective by lowercase id and by the CRC-32 of that id, with the task rows to build
/// their trees.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct QuestBook {
    tasks: TaskBook,
    objectives: HashMap<String, ObjectiveRow>,
    /// The lowercase id of each objective by its CRC-32 (the client names quests so: 3772, 794).
    by_crc: HashMap<u32, String>,
}

/// A cell that is empty or missing as `None`.
fn cell(value: Option<&String>) -> Option<String> {
    value.filter(|text| !text.is_empty()).cloned()
}

impl QuestBook {
    /// The objectives of `tables` over their task rows `rows` ([`task_rows`]).
    pub fn from_tables(tables: &Tables, rows: Vec<TaskRow>) -> Self {
        Self::build(
            tables
                .objectives
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| ObjectiveRow {
                    id: row.objective_id.clone(),
                    root: row.task.clone(),
                    reward_item: row.item_reward_name.clone(),
                    reward_quantity: row.item_reward_qty,
                    repeatable: row.repeatable.unwrap_or_default(),
                    requirement: cell(row.required_achievement_id.as_ref()),
                    achievement: cell(row.achievement_id.as_ref()),
                    turn_in: cell(row.npc_destination_id.as_ref()),
                    success_event: cell(row.success_game_event_id.as_ref()),
                }),
            TaskBook::new(rows),
        )
    }

    /// The book of `objectives` over `tasks`; the first row of an id wins.
    pub fn build(objectives: impl IntoIterator<Item = ObjectiveRow>, tasks: TaskBook) -> Self {
        let mut by_id = HashMap::new();
        for row in objectives {
            by_id.entry(row.id.to_lowercase()).or_insert(row);
        }
        let by_crc = by_id
            .keys()
            .map(|id| (lowercase_crc32(id), id.clone()))
            .collect();
        Self {
            tasks,
            objectives: by_id,
            by_crc,
        }
    }

    /// The quest whose lowercase id has the CRC-32 `crc`, as [`QuestBook::quest`].
    pub fn quest_by_crc(&self, crc: u32) -> Option<QuestDef> {
        self.quest(self.by_crc.get(&crc)?)
    }

    /// The quest of objective `id` (any case); `None` when there is no such objective or its
    /// root task is not a task row.
    pub fn quest(&self, id: &str) -> Option<QuestDef> {
        let row = self.objectives.get(&id.to_lowercase())?;
        let tree = self.tasks.tree(&row.root)?;
        let reward = row
            .reward_item
            .as_ref()
            .filter(|item| !item.is_empty())
            .map(|item| {
                let quantity = row
                    .reward_quantity
                    .and_then(|quantity| u32::try_from(quantity).ok())
                    .filter(|quantity| *quantity > 0)
                    .unwrap_or(1);
                (item.clone(), quantity)
            });
        Some(QuestDef {
            id: row.id.clone(),
            tree: Arc::new(tree),
            reward,
            repeatable: row.repeatable,
            requirement: row.requirement.clone(),
            achievement: row.achievement.clone(),
            turn_in: row.turn_in.clone(),
            success_event: row.success_event.clone(),
        })
    }
}

/// One token of an achievement expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token<'a> {
    Name(&'a str),
    Not,
    And,
    Or,
    Open,
    Close,
}

/// The tokens of `expression`; `None` when it holds another character.
fn tokens(expression: &str) -> Option<Vec<Token<'_>>> {
    let mut tokens = Vec::new();
    let mut rest = expression.trim_start();
    while let Some(first) = rest.chars().next() {
        let (token, length) = match first {
            '!' => (Token::Not, 1),
            '(' => (Token::Open, 1),
            ')' => (Token::Close, 1),
            '&' if rest.starts_with("&&") => (Token::And, 2),
            '|' if rest.starts_with("||") => (Token::Or, 2),
            name if name.is_ascii_alphanumeric() || name == '_' => {
                let length = rest
                    .find(|next: char| !(next.is_ascii_alphanumeric() || next == '_'))
                    .unwrap_or(rest.len());
                (Token::Name(&rest[..length]), length)
            }
            _ => return None,
        };
        tokens.push(token);
        rest = rest[length..].trim_start();
    }
    Some(tokens)
}

/// Reads an expression of `tokens` from `at`: terms joined by `||`.
fn any_of(tokens: &[Token<'_>], at: &mut usize, has: &dyn Fn(&str) -> bool) -> Option<bool> {
    let mut value = all_of(tokens, at, has)?;
    while tokens.get(*at) == Some(&Token::Or) {
        *at += 1;
        // Every term is read, so a bad one later fails the whole expression.
        value |= all_of(tokens, at, has)?;
    }
    Some(value)
}

/// Reads a term: factors joined by `&&`.
fn all_of(tokens: &[Token<'_>], at: &mut usize, has: &dyn Fn(&str) -> bool) -> Option<bool> {
    let mut value = factor(tokens, at, has)?;
    while tokens.get(*at) == Some(&Token::And) {
        *at += 1;
        value &= factor(tokens, at, has)?;
    }
    Some(value)
}

/// Reads a factor: a name, a `!` factor or a bracketed expression.
fn factor(tokens: &[Token<'_>], at: &mut usize, has: &dyn Fn(&str) -> bool) -> Option<bool> {
    let token = *tokens.get(*at)?;
    *at += 1;
    match token {
        Token::Name(name) => Some(has(name)),
        Token::Not => factor(tokens, at, has).map(|value| !value),
        Token::Open => {
            let value = any_of(tokens, at, has)?;
            (tokens.get(*at) == Some(&Token::Close)).then(|| {
                *at += 1;
                value
            })
        }
        Token::And | Token::Or | Token::Close => None,
    }
}

/// Whether the achievement expression `expression` (`RequiredAchievementID`: names joined by
/// `&&` and `||`, `!` before a name or a bracket) holds for a player who has the achievements
/// that `has` names. An expression this cannot read does not hold.
pub fn requirement_met(expression: &str, has: impl Fn(&str) -> bool) -> bool {
    let Some(tokens) = tokens(expression) else {
        return false;
    };
    if tokens.is_empty() {
        return true;
    }
    let mut at = 0;
    any_of(&tokens, &mut at, &has).is_some_and(|value| value && at == tokens.len())
}

/// What a client needs to show one mob prefab (mob-cards note, section 2).
#[derive(Debug, Clone, PartialEq)]
pub struct MobLook {
    /// The creature slice asset of its spawn card.
    pub asset: AssetId,
    /// The component of its `ALCReplicatedState`.
    pub alc_component: u64,
    /// The component of its vitals state.
    pub vitals_component: u64,
    /// Its retail first `ALCReplicatedState`, moved to each spawn point; `None` when the table
    /// has none.
    pub alc: Option<AlcState>,
    /// Its retail first vitals state, with each mob's health written in; `None` when the table
    /// has none.
    pub vitals: Option<VitalsState>,
    /// Its other states when it comes into view.
    pub first: Vec<BundleMember>,
    /// Its other states when it dies.
    pub death: Vec<BundleMember>,
}

/// The look of every mob prefab, by vitals id and slice asset.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MobLooks {
    by_key: HashMap<PrefabKey, MobLook>,
    /// The asset of each vitals id's most seen prefab.
    default_asset: HashMap<u32, [u8; ASSET_ID_LEN]>,
}

impl MobLooks {
    /// The looks of `prefabs`. A state whose body does not decode is left out of its prefab;
    /// the second value counts them.
    pub fn of(prefabs: &MobPrefabs) -> (Self, usize) {
        let mut skipped = 0;
        let mut members = |rows: &[PrefabMember]| -> Vec<BundleMember> {
            rows.iter()
                .filter_map(|row| match MemberState::decode(row.class, &row.body) {
                    Ok(state) => Some(BundleMember {
                        component: row.component,
                        state,
                    }),
                    Err(_) => {
                        skipped += 1;
                        None
                    }
                })
                .collect()
        };
        let by_key = prefabs
            .iter()
            .map(|(key, prefab)| {
                // The ALC and vitals states of the first group are taken apart: each mob gets
                // them with its own place and health.
                let mut alc = None;
                let mut vitals = None;
                let mut first = Vec::new();
                for member in members(&prefab.first_members) {
                    match member.state {
                        MemberState::Alc(state) => alc = Some(state),
                        MemberState::Vitals(state) => vitals = Some(state),
                        state => first.push(BundleMember {
                            component: member.component,
                            state,
                        }),
                    }
                }
                let look = MobLook {
                    asset: AssetId {
                        id: prefab.asset,
                        sub_id: prefab.sub_id,
                    },
                    alc_component: prefab.alc_component,
                    vitals_component: prefab.vitals_component,
                    alc,
                    vitals,
                    first,
                    death: members(&prefab.death_members),
                };
                (key, look)
            })
            .collect();
        let looks = Self {
            by_key,
            default_asset: prefabs.default_assets().collect(),
        };
        (looks, skipped)
    }

    /// The look of vitals id `vitals_id` with slice asset `asset`; without an asset, or when the
    /// table has no prefab of it, the vitals id's most seen look.
    pub fn look(&self, vitals_id: u32, asset: Option<[u8; ASSET_ID_LEN]>) -> Option<&MobLook> {
        asset
            .and_then(|asset| self.by_key.get(&(vitals_id, asset)))
            .or_else(|| {
                let asset = self.default_asset.get(&vitals_id)?;
                self.by_key.get(&(vitals_id, *asset))
            })
    }

    /// Prefabs with a look.
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    /// Whether there is no look.
    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }
}

/// The player content of one content bundle and mob prefab table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayerContent {
    /// The login kit of each archetype pick.
    pub kits: StarterKits,
    /// The bit of each achievement.
    pub achievements: AchievementIndexes,
    /// The look of each mob prefab.
    pub mob_looks: MobLooks,
    /// The animation names of each creature slice.
    pub creature_animations: CreatureAnimations,
    /// The achievement each archetype pick unlocks: (backstory id, achievement id), from
    /// `BackstoryDefinition.AchievementUnlockOverride`.
    pub archetype_unlocks: Vec<(String, String)>,
    /// The vitals id and categories of every mob type, for quest kill tasks.
    pub kill_types: KillTypes,
    /// The task tree of the tutorial quest; `None` when the content lacks it.
    pub tutorial_quest: Option<Arc<ObjectiveTree>>,
    /// Every quest; the first open-world quest is one of them.
    pub quests: QuestBook,
    /// The tutorial level's entities: its static entities, its asset entities and the retail
    /// causes of their groups.
    pub tutorial_level: FtueLevel,
    /// The gatherables.
    pub gatherables: Gatherables,
    /// The crafting recipes.
    pub recipes: Recipes,
    /// The reward of each game event, for quest completions.
    pub rewards: Rewards,
    /// Every loot table and bucket, for gathering and mob kills.
    pub loot: LootTables,
    /// What each mob type drops when killed.
    pub mob_loot: MobLoot,
    /// The loot table of each loot crate of the level.
    pub crates: CrateLoot,
    /// The level's crafting stations.
    pub stations: Vec<CraftingStation>,
    /// The resources each structure blueprint costs, for the stations' committed resources.
    pub structure_costs: StructureCosts,
    /// The loot value of a player's level by its experience.
    pub player_loot_levels: PlayerLootLevels,
    /// The point-of-interest tags of each territory, for go-to and exit tasks.
    pub poi_tags: PoiTags,
    /// The level's quest objects that the player breaks with its weapons.
    pub quest_objects: QuestObjects,
    /// The leaderboards each stat feeds.
    pub leaderboards: LeaderboardStats,
    /// The gameplay values of the remote config layers (`crate::gameplay`).
    pub gameplay: GameplayConfig,
    /// The experience of a kill by the mob's level, before the multipliers.
    pub kill_experience: KillExperience,
    /// The lowercase ids of the items that `javelin.disabled-item-classes` disables.
    pub disabled_items: HashSet<String>,
    /// The loot limits and their resets; `None` applies no limit.
    pub loot_limits: Option<LootLimitRules>,
    /// The level table and the weapon masteries.
    pub progression: Progression,
}

/// A quest object of the level: a gatherable point whose gather is a game event that a quest's
/// `TaskGameEvent` counts (Grace's corruption nodules, `MSQ_CorruptedNodule_1_interact`). Retail
/// 2026-09-18 broke them with weapon hits, with no interaction from the client.
#[derive(Debug, Clone, PartialEq)]
pub struct QuestObject {
    /// The game event of its gatherable.
    pub event: String,
    /// Where it stands (the level's gatherable point).
    pub position: Position,
    /// How long it stays broken: its gatherable's `MinRespawnRate`.
    pub respawn_ms: u64,
}

/// The quest objects of a level.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct QuestObjects {
    objects: Vec<QuestObject>,
}

impl QuestObjects {
    /// The quest objects among the level's gatherable `points`: those of a gatherable in
    /// `gatherables` whose game event is one of `counted` (lowercase ids).
    pub fn build(
        points: &[GatherablePoint],
        gatherables: &Gatherables,
        counted: &HashSet<String>,
    ) -> Self {
        Self {
            objects: points
                .iter()
                .filter_map(|point| {
                    let gatherable = gatherables.get(point.gatherable_id)?;
                    let event = gatherable.game_event.clone()?;
                    counted.contains(&event.to_lowercase()).then(|| {
                        let [x, y, z] = point.position;
                        QuestObject {
                            event,
                            position: Position { x, y, z },
                            respawn_ms: gatherable.respawn_ms.0,
                        }
                    })
                })
                .collect(),
        }
    }

    /// The nearest object within `reach_m` of `at` that `wanted` takes, with its index.
    pub fn nearest(
        &self,
        at: Position,
        reach_m: f32,
        wanted: impl Fn(usize, &QuestObject) -> bool,
    ) -> Option<(usize, &QuestObject)> {
        self.objects
            .iter()
            .enumerate()
            .map(|(index, object)| (index, object, object.position.horizontal_distance(at)))
            .filter(|(index, object, distance)| *distance <= reach_m && wanted(*index, object))
            .min_by(|(_, _, a), (_, _, b)| a.total_cmp(b))
            .map(|(index, object, _)| (index, object))
    }

    /// Objects in the level.
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Whether the level has none.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}

impl PlayerContent {
    /// The player content of `tables` (with the datasheet overrides of `gameplay` applied)
    /// with the mob looks `mob_looks`, the creature animations `creature_animations`, the
    /// tutorial level's entities `tutorial_level`, the gatherable points and capitals of the
    /// sim's level `level`, the gameplay values `gameplay` and the loot limit resets
    /// `loot_reset`.
    pub fn new(
        tables: &Tables,
        mob_looks: MobLooks,
        creature_animations: CreatureAnimations,
        tutorial_level: FtueLevel,
        level: &WorldFile,
        gameplay: GameplayConfig,
        loot_reset: ResetClock,
    ) -> Self {
        let level_gatherables = &level.gatherables;
        let rows = task_rows(tables);
        let gatherables = Gatherables::from_tables(tables);
        let quest_objects =
            QuestObjects::build(level_gatherables, &gatherables, &counted_game_events(&rows));
        Self {
            kits: StarterKits::from_tables(tables),
            achievements: AchievementIndexes::from_tables(tables),
            mob_looks,
            creature_animations,
            archetype_unlocks: tables
                .backstory_definition
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    let unlock = row.achievement_unlock_override.clone()?;
                    Some((row.backstory_id.clone(), unlock))
                })
                .collect(),
            kill_types: KillTypes::from_tables(tables),
            tutorial_quest: ObjectiveTree::build(&rows, TUTORIAL_QUEST_ROOT).map(Arc::new),
            quests: QuestBook::from_tables(tables, rows),
            tutorial_level,
            gatherables,
            recipes: Recipes::from_tables(tables),
            rewards: Rewards::from_tables(tables),
            loot: LootTables::from_tables(tables),
            mob_loot: MobLoot::from_tables(tables),
            crates: CrateLoot::build(
                CrateVariant::from_tables(tables),
                level.capitals.iter().filter_map(|capital| {
                    Some((capital.id.as_str(), capital.variant_name.as_deref()?))
                }),
                level
                    .spawned
                    .iter()
                    .map(|slice| (slice.id, slice.variant.as_str())),
            ),
            stations: level.stations.clone(),
            structure_costs: StructureCosts::from_tables(tables),
            player_loot_levels: PlayerLootLevels::from_tables(tables),
            poi_tags: PoiTags::from_tables(tables),
            quest_objects,
            leaderboards: LeaderboardStats::from_tables(tables),
            kill_experience: KillExperience::from_tables(tables),
            disabled_items: disabled_items(
                tables
                    .master_item_definitions
                    .iter()
                    .flat_map(|table| &table.rows)
                    .map(|row| (row.item_id.as_str(), row.item_class.as_deref())),
                &gameplay,
            ),
            loot_limits: Some(LootLimitRules {
                limits: LootLimits::from_tables(tables),
                clock: loot_reset,
            }),
            gameplay,
            progression: Progression::of_tables(tables),
        }
    }

    /// Whether item `item_id` (any case) is disabled.
    pub fn is_item_disabled(&self, item_id: &str) -> bool {
        self.disabled_items.contains(&item_id.to_lowercase())
    }
}

/// The lowercase ids of the master `items` (`ItemID`, `ItemClass`) whose class list holds a
/// class that `gameplay` disables (client check `FUN_14443ea80`, build 25810808: an item is
/// disabled when one of its classes is in `javelin.disabled-item-classes`).
fn disabled_items<'a>(
    items: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
    gameplay: &GameplayConfig,
) -> HashSet<String> {
    items
        .into_iter()
        .filter(|(_, class)| class.is_some_and(|class| gameplay.is_class_disabled(class)))
        .map(|(item_id, _)| item_id.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_items_of_a_disabled_class_are_disabled() {
        let gameplay = GameplayConfig {
            disabled_item_classes: vec!["Dagger".to_owned()],
            ..GameplayConfig::default()
        };
        let content = PlayerContent {
            disabled_items: disabled_items(
                [
                    (
                        "1hDaggerDropT2",
                        Some("Weapon+EquippableMainHand+Dagger+Melee"),
                    ),
                    (
                        "1hSwordT2_FTUE",
                        Some("Weapon+EquippableMainHand+Sword+Melee"),
                    ),
                    ("NoClass", None),
                ],
                &gameplay,
            ),
            ..PlayerContent::default()
        };
        assert!(content.is_item_disabled("1hdaggerdropt2"));
        assert!(!content.is_item_disabled("1hSwordT2_FTUE"));
        assert!(!content.is_item_disabled("NoClass"));
    }

    const SAILOR: u32 = 2_288_616_137;

    #[test]
    fn a_requirement_reads_the_retail_expressions() {
        // Retail RequiredAchievementID cells (beach-quests note).
        const GET_OFF_BEACH: &str = "!S_introtosettlement && !Watched_IGC_Aelstrom_WW";
        const BOAR_FEAST: &str = "S_skinningknife_mb && !Archetype_Mercenary && \
            !Archetype_Warmonger && !Archetype_Tracker && !Archetype_Musketeer && \
            !Archetype_Occultist && !Archetype_Theurgist && !Archetype_Wanderer";
        const BOAR_FEAST_MYSTIC: &str = "S_skinningknife_mb && Archetype_Theurgist";
        let mystic = |name: &str| ["S_skinningknife_mb", "Archetype_Theurgist"].contains(&name);
        let nobody = |_: &str| false;
        assert!(requirement_met(GET_OFF_BEACH, nobody));
        assert!(!requirement_met(GET_OFF_BEACH, |name| name == "S_introtosettlement"));
        assert!(requirement_met(BOAR_FEAST_MYSTIC, mystic));
        assert!(
            !requirement_met(BOAR_FEAST, mystic),
            "the mystic gets its own variant"
        );
        assert!(requirement_met(BOAR_FEAST, |name| name == "S_skinningknife_mb"));
        assert!(requirement_met("(A || B) && !C", |name| name == "B"));
        assert!(!requirement_met("(A || B) && !C", |name| name != "A"));
        assert!(requirement_met("", nobody), "no requirement");
        assert!(!requirement_met("A &&", |_| true), "a cut expression");
        assert!(!requirement_met("(A", |_| true));
        assert!(!requirement_met("A = B", |_| true));
    }

    #[test]
    fn a_quest_is_found_by_the_crc_of_its_id() {
        // Retail 3772 of 12_skinningknife.
        const SKINNING_KNIFE: u32 = 0xf523_32f7;
        let book = QuestBook::build(
            [ObjectiveRow {
                id: "12_skinningknife".to_owned(),
                root: "root".to_owned(),
                turn_in: Some("01_UrdaMB_01".to_owned()),
                ..ObjectiveRow::default()
            }],
            TaskBook::new(vec![crate::objectives::TaskRow {
                task_id: "root".to_owned(),
                kind: "ConsecutiveTaskContainer".to_owned(),
                ..crate::objectives::TaskRow::default()
            }]),
        );
        let quest = book.quest_by_crc(SKINNING_KNIFE).expect("quest");
        assert_eq!(quest.turn_in.as_deref(), Some("01_UrdaMB_01"));
        assert!(book.quest_by_crc(0).is_none());
    }

    #[test]
    fn prefab_bodies_become_states_and_bad_ones_are_left_out() {
        let text = format!(
            r#"[{{"vitals_id":{SAILOR},"vitals_name":null,
                "asset":"b7e33b294bcf57b8a476abdc3a31fae7","sub_id":2,
                "alc_component":5,"vitals_component":9,
                "first_members":[{{"component":6,"class":12,"body":"010104"}},
                                 {{"component":3,"class":1528,"body":"01"}}],
                "death_members":[{{"component":20,"class":3139,"body":"010201"}}],
                "seen":1}}]"#
        );
        let prefabs = MobPrefabs::parse(&text).expect("table");
        let (looks, skipped) = MobLooks::of(&prefabs);
        assert_eq!(skipped, 1, "a cut damage receiver body");
        let sailor = looks.look(SAILOR, None).expect("sailor");
        assert_eq!(sailor.asset.sub_id, 2);
        assert_eq!(sailor.first.len(), 1);
        assert_eq!(sailor.first[0].component, 6);
        assert_eq!(sailor.death.len(), 1);
    }

    #[test]
    fn a_mob_with_its_own_asset_gets_its_own_look() {
        // Retail: the door legionnaire (179393) and the signifier (202592) share their vitals id.
        const LEGIONNAIRE: u32 = 3_705_015_359;
        const DOOR: [u8; ASSET_ID_LEN] =
            0xc05f_c957_8487_5559_afe8_74a7_8f95_fb64_u128.to_be_bytes();
        const SIGNIFIER: [u8; ASSET_ID_LEN] =
            0x87a3_35e7_6b86_50d5_a465_fe08_10bd_a5df_u128.to_be_bytes();
        let row = |asset: [u8; ASSET_ID_LEN], sub_id: u32, seen: u32| {
            let asset: String = asset.iter().map(|byte| format!("{byte:02x}")).collect();
            format!(
                r#"{{"vitals_id":{LEGIONNAIRE},"vitals_name":null,"asset":"{asset}",
                    "sub_id":{sub_id},"alc_component":5,"vitals_component":9,
                    "first_members":[],"death_members":[],"seen":{seen}}}"#
            )
        };
        let text = format!("[{},{}]", row(DOOR, 1, 2), row(SIGNIFIER, 2, 3));
        let (looks, _) = MobLooks::of(&MobPrefabs::parse(&text).expect("table"));
        let sub_id = |asset| looks.look(LEGIONNAIRE, asset).map(|look| look.asset.sub_id);
        assert_eq!(sub_id(Some(DOOR)), Some(1));
        assert_eq!(sub_id(Some(SIGNIFIER)), Some(2));
        assert_eq!(sub_id(None), Some(2), "the most seen look without an asset");
        assert_eq!(
            sub_id(Some([0; ASSET_ID_LEN])),
            Some(2),
            "and for an asset not in the table"
        );
    }
}
