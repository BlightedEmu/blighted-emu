// SPDX-License-Identifier: AGPL-3.0-or-later
//! One join (M5 spec section 3.1): check the account, load the position, the items, the quests
//! and the progression, read the account's admin rank, take an entity number, and ask the owner
//! of the cell to take the player.

use std::time::Duration;

use nw_bus::{
    Admit, AdmitReply, Bus, CarriedItem, ItemPlace, Join, LootLimitCount, MOB_ENTITY_BASE,
    PlayerProgression, PlayerQuests, Point, QuestRun, SimSlot, subject,
};
use nw_db::{
    AdminRank, PgPool, SavedItem, SavedPlace, SavedPosition, SavedProgression, SavedQuests,
    admin_rank, character_by_id, character_items, character_position, character_progression,
    character_quests, next_entity_number,
};
use nw_login::look::{saved_backstory, saved_look};
use nw_sim_core::{Position, TUTORIAL_FOLDER, TUTORIAL_START};
use uuid::Uuid;

use crate::sims::Assignment;

/// Longest wait for an account's admin rank; it stays far below the admit timeout.
const RANK_READ_TIMEOUT: Duration = Duration::from_millis(300);

/// The handoff count and the bundle sequence of a new player.
const NEW_PLAYER_COUNT: u64 = 0;

/// What every join needs.
#[derive(Debug, Clone)]
pub struct JoinContext {
    /// The world.
    pub world_id: Uuid,
    /// The open map level.
    pub level: String,
    /// Where characters that were never in the world appear, in turn.
    pub starts: Vec<Position>,
    /// Longest wait for the sim.
    pub admit_timeout: Duration,
    /// The world database.
    pub db: PgPool,
    /// The global database with the admin ranks; `None` gives no account the GM flag.
    pub ranks: Option<PgPool>,
    /// NATS.
    pub bus: Bus,
}

/// Why a join was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JoinRefusal {
    /// The world has no such character.
    #[error("no such character")]
    NoSuchCharacter,
    /// The character belongs to another account.
    #[error("the character belongs to another account")]
    OtherAccount,
    /// The database failed.
    #[error("database: {0}")]
    Database(String),
    /// No live sim owns the player's cell.
    #[error("no sim owns the cell")]
    NoOwner,
    /// The director has no start position.
    #[error("no start position")]
    NoStart,
    /// The sim refused the player.
    #[error("the sim refused: {0}")]
    SimRefused(String),
    /// The sim did not answer in time.
    #[error("the sim did not answer: {0}")]
    SimSilent(String),
    /// The entity number is in the mob range.
    #[error("entity number {0} is in the mob range")]
    EntityRange(u64),
}

impl JoinRefusal {
    /// Metric label.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::NoSuchCharacter => "no_such_character",
            Self::OtherAccount => "other_account",
            Self::Database(_) => "database",
            Self::NoOwner => "no_owner",
            Self::NoStart => "no_start",
            Self::SimRefused(_) => "sim_refused",
            Self::SimSilent(_) => "sim_silent",
            Self::EntityRange(_) => "entity_range",
        }
    }
}

/// `entity` as a player's entity number: refused in the mob range (M6-A spec section 6.1).
pub fn player_entity(entity: u64) -> Result<u64, JoinRefusal> {
    if entity >= MOB_ENTITY_BASE {
        return Err(JoinRefusal::EntityRange(entity));
    }
    Ok(entity)
}

/// The start position of a new character with entity number `entity`: the start positions in
/// turn (M5 load spec section 6).
pub fn start_for(starts: &[Position], entity: u64) -> Option<Position> {
    let count = u64::try_from(starts.len())
        .ok()
        .filter(|count| *count > 0)?;
    // The remainder is below the length, so it fits usize.
    starts.get((entity % count) as usize).copied()
}

/// Where a character with entity number `entity` appears: its saved spot when that spot is on
/// `level` and `on_map` holds for it, else its start ([`start_for`]). A spot that is off the map
/// would refuse every later join of the character; the sim puts the height on the ground.
pub fn join_position(
    saved: Option<&SavedPosition>,
    level: &str,
    on_map: impl Fn(Position) -> bool,
    starts: &[Position],
    entity: u64,
) -> Option<Position> {
    saved
        .filter(|saved| saved.level == level)
        .map(|saved| {
            let [x, y, z] = saved.metres();
            Position { x, y, z }
        })
        .filter(|position| on_map(*position))
        .or_else(|| start_for(starts, entity))
}

/// Whether a character with the saved spot `saved` starts in the tutorial: it was never in the
/// world, or it left during the tutorial (which then starts again).
pub fn starts_in_tutorial(saved: Option<&SavedPosition>) -> bool {
    saved.is_none_or(|saved| saved.level == TUTORIAL_FOLDER)
}

/// Routes `join`: a new player to the tutorial sim `tutorial` (when one is live) at the tutorial
/// start, every other player to the owner of its cell in `assignment`. Returns the sim, the
/// entity number (a new one, or the resumed one) and whether the account is a game master.
///
/// # Errors
///
/// Returns the [`JoinRefusal`] of the first step that fails.
pub async fn route_join(
    context: &JoinContext,
    assignment: &Assignment,
    tutorial: Option<&SimSlot>,
    join: &Join,
) -> Result<(SimSlot, u64, bool), JoinRefusal> {
    let database = |error: nw_db::DbError| JoinRefusal::Database(error.to_string());
    let character = character_by_id(&context.db, join.character)
        .await
        .map_err(database)?
        .ok_or(JoinRefusal::NoSuchCharacter)?;
    if character.account_id != join.account {
        return Err(JoinRefusal::OtherAccount);
    }
    let saved = character_position(&context.db, join.character)
        .await
        .map_err(database)?;
    let (entity, handoffs, bundles_sent) = match join.resume {
        Some(resume) => (
            player_entity(resume.entity)?,
            resume.handoffs,
            resume.bundles_sent,
        ),
        None => (
            player_entity(next_entity_number(&context.db).await.map_err(database)?)?,
            NEW_PLAYER_COUNT,
            NEW_PLAYER_COUNT,
        ),
    };
    let tutorial = tutorial.filter(|_| starts_in_tutorial(saved.as_ref()));
    // The tutorial always starts from the starter kit, no quest and no progression (admin
    // studio S5, S7; the tutorial sim saves none of them).
    let (items, quests, progression) = match tutorial {
        Some(_) => (None, PlayerQuests::default(), PlayerProgression::default()),
        None => (
            character_items(&context.db, join.character)
                .await
                .map_err(database)?
                .map(carried_items),
            character_quests(&context.db, join.character)
                .await
                .map_err(database)?
                .map(player_quests)
                .unwrap_or_default(),
            character_progression(&context.db, join.character)
                .await
                .map_err(database)?
                .map(player_progression)
                .unwrap_or_default(),
        ),
    };
    let game_master = is_game_master(context.ranks.as_ref(), join.account).await;
    let (sim, position) = match tutorial {
        Some(tutorial) => (tutorial.clone(), TUTORIAL_START),
        None => {
            let position = join_position(
                saved.as_ref(),
                &context.level,
                |position| assignment.covers(position),
                &context.starts,
                entity,
            )
            .ok_or(JoinRefusal::NoStart)?;
            let sim = assignment
                .owner_at(position)
                .ok_or(JoinRefusal::NoOwner)?
                .clone();
            (sim, position)
        }
    };
    let admit = Admit {
        connection: join.connection.clone(),
        account: join.account,
        character: join.character,
        name: character.name,
        entity,
        position: Point::from(position),
        login: join.resume.is_none(),
        handoffs,
        bundles_sent,
        look: saved_look(&character.appearance).map(|look| look.choices()),
        backstory: saved_backstory(&character.appearance),
        items,
        quests,
        progression,
        game_master,
    };
    let reply = context
        .bus
        .request::<Admit, AdmitReply>(
            subject::sim_admit(context.world_id, &sim.id),
            &admit,
            context.admit_timeout,
        )
        .await
        .map_err(|error| JoinRefusal::SimSilent(error.to_string()))?;
    match reply {
        AdmitReply::Admitted => Ok((sim, entity, game_master)),
        AdmitReply::Refused { reason } => Err(JoinRefusal::SimRefused(reason)),
    }
}

/// Whether `account` has the game master rank (or a higher one) in `ranks`. A failed or slow
/// read (above [`RANK_READ_TIMEOUT`]) gives no GM rights, so a database fault never refuses or
/// holds up a join.
async fn is_game_master(ranks: Option<&PgPool>, account: Uuid) -> bool {
    let Some(ranks) = ranks else {
        return false;
    };
    match tokio::time::timeout(RANK_READ_TIMEOUT, admin_rank(ranks, account)).await {
        Ok(Ok(rank)) => rank.is_some_and(|rank| rank >= AdminRank::GameMaster),
        Ok(Err(error)) => {
            tracing::warn!(%error, %account, "admin rank read failed; no GM rights");
            false
        }
        Err(_) => {
            tracing::warn!(%account, "admin rank read too slow; no GM rights");
            false
        }
    }
}

/// The saved quests as a sim runs them; an active run that no longer reads is dropped.
fn player_quests(saved: SavedQuests) -> PlayerQuests {
    let active =
        saved.active.and_then(
            |(objective, run)| match serde_json::from_value::<QuestRun>(run) {
                Ok(run) => Some((objective, run)),
                Err(error) => {
                    tracing::warn!(%error, objective, "saved quest run does not read");
                    None
                }
            },
        );
    PlayerQuests {
        active,
        done: saved.done,
        ..PlayerQuests::default()
    }
}

/// The saved progression as a sim keeps it.
fn player_progression(saved: SavedProgression) -> PlayerProgression {
    PlayerProgression {
        experience: saved.experience,
        coins: saved.coins,
        achievements: saved.achievements,
        loot_limits: saved
            .loot_limits
            .into_iter()
            .map(|limit| LootLimitCount {
                limit_id: limit.limit_id,
                count: limit.count,
                counted_ns: limit.counted_ns,
                ends_ns: limit.ends_ns,
            })
            .collect(),
        masteries: saved.masteries,
        attributes: saved.attributes,
        weapon_points: saved.weapon_points,
    }
}

/// The saved items as the list a sim builds gear from.
fn carried_items(saved: Vec<SavedItem>) -> Vec<CarriedItem> {
    saved
        .into_iter()
        .map(|item| CarriedItem {
            item_id: item.item_id,
            quantity: item.quantity,
            place: match item.place {
                SavedPlace::Worn(slot) => ItemPlace::Worn(slot),
                SavedPlace::Bag(slot) => ItemPlace::Bag(slot),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_quests_read_their_run_and_drop_a_bad_one() {
        let run = QuestRun {
            tasks: vec![nw_bus::QuestTask::Done],
            listed: vec![0],
        };
        let good = player_quests(SavedQuests {
            active: Some((
                "12_GetOffBeach".to_owned(),
                serde_json::to_value(&run).expect("json"),
            )),
            done: vec!["01_SoleSurvivor2".to_owned()],
        });
        assert_eq!(good.active, Some(("12_GetOffBeach".to_owned(), run)));
        assert_eq!(good.done, ["01_SoleSurvivor2"]);
        let bad = player_quests(SavedQuests {
            active: Some(("Q".to_owned(), serde_json::json!({"tasks": "no"}))),
            done: Vec::new(),
        });
        assert_eq!(bad.active, None);
    }

    #[test]
    fn saved_progression_keeps_every_value() {
        let saved = SavedProgression {
            experience: 1_250,
            coins: 375,
            achievements: vec![14_809, 15_317],
            loot_limits: vec![nw_db::SavedLootLimit {
                limit_id: "2hwarhammer_ftue_corruptedsailer_v2".to_owned(),
                count: 1,
                counted_ns: 1_793_091_600_894_621_000,
                ends_ns: 1_793_091_600_894_621_000,
            }],
            masteries: std::collections::BTreeMap::from([("BowAbilityTable".to_owned(), 30)]),
            attributes: vec![(2, 15)],
            weapon_points: vec![("Ability_Greatsword_Combo".to_owned(), 1)],
        };
        assert_eq!(
            player_progression(saved),
            PlayerProgression {
                experience: 1_250,
                coins: 375,
                achievements: vec![14_809, 15_317],
                loot_limits: vec![LootLimitCount {
                    limit_id: "2hwarhammer_ftue_corruptedsailer_v2".to_owned(),
                    count: 1,
                    counted_ns: 1_793_091_600_894_621_000,
                    ends_ns: 1_793_091_600_894_621_000,
                }],
                masteries: std::collections::BTreeMap::from([("BowAbilityTable".to_owned(), 30)]),
                attributes: vec![(2, 15)],
                weapon_points: vec![("Ability_Greatsword_Combo".to_owned(), 1)],
            }
        );
    }

    #[test]
    fn saved_items_keep_their_places() {
        let carried = carried_items(vec![
            SavedItem {
                item_id: "1hSwordT2_FTUE".to_owned(),
                quantity: 1,
                place: SavedPlace::Worn(3),
            },
            SavedItem {
                item_id: "PotionHealthT1".to_owned(),
                quantity: 5,
                place: SavedPlace::Bag(7),
            },
        ]);
        assert_eq!(
            carried
                .iter()
                .map(|item| (item.item_id.as_str(), item.quantity, item.place))
                .collect::<Vec<_>>(),
            [
                ("1hSwordT2_FTUE", 1, ItemPlace::Worn(3)),
                ("PotionHealthT1", 5, ItemPlace::Bag(7)),
            ]
        );
    }

    #[test]
    fn a_number_in_the_mob_range_is_refused() {
        assert_eq!(player_entity(MOB_ENTITY_BASE - 1), Ok(MOB_ENTITY_BASE - 1));
        assert_eq!(
            player_entity(MOB_ENTITY_BASE),
            Err(JoinRefusal::EntityRange(MOB_ENTITY_BASE))
        );
    }

    const STARTS: [Position; 3] = [
        Position {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        },
        Position {
            x: 2.0,
            y: 0.0,
            z: 0.0,
        },
        Position {
            x: 3.0,
            y: 0.0,
            z: 0.0,
        },
    ];

    #[test]
    fn new_characters_and_tutorial_leavers_start_in_the_tutorial() {
        assert!(starts_in_tutorial(None));
        assert!(starts_in_tutorial(Some(&saved_at(TUTORIAL_FOLDER, 1.0))));
        assert!(!starts_in_tutorial(Some(&saved_at(LEVEL, 1.0))));
    }

    #[test]
    fn new_characters_take_the_start_points_in_turn() {
        let xs: Vec<f32> = (1..=6)
            .map(|entity| start_for(&STARTS, entity).map(|start| start.x))
            .collect::<Option<_>>()
            .expect("starts");
        assert_eq!(xs, vec![2.0, 3.0, 1.0, 2.0, 3.0, 1.0]);
        assert_eq!(start_for(&[], 1), None);
    }

    const LEVEL: &str = "newworld_vitaeeterna";
    const MAP_EDGE_M: f32 = 100.0;

    fn saved_at(level: &str, x: f64) -> SavedPosition {
        SavedPosition {
            level: level.to_owned(),
            x,
            y: 10.0,
            z: 50.0,
            yaw: 0.0,
            saved_at_ms: 0,
        }
    }

    fn on_map(position: Position) -> bool {
        (0.0..=MAP_EDGE_M).contains(&position.x)
    }

    #[test]
    fn a_saved_spot_on_the_map_is_kept_and_one_off_it_gets_a_start() {
        let entity = 1;
        let saved = saved_at(LEVEL, 42.0);
        let kept = join_position(Some(&saved), LEVEL, on_map, &STARTS, entity);
        assert_eq!(
            kept,
            Some(Position {
                x: 42.0,
                y: 10.0,
                z: 50.0
            })
        );
        let start = start_for(&STARTS, entity);
        let off_map = saved_at(LEVEL, f64::from(MAP_EDGE_M) + 1.0);
        assert_eq!(
            join_position(Some(&off_map), LEVEL, on_map, &STARTS, entity),
            start
        );
        let other_level = saved_at("nw_dungeon_edengrove_00", 42.0);
        assert_eq!(
            join_position(Some(&other_level), LEVEL, on_map, &STARTS, entity),
            start
        );
        assert_eq!(join_position(None, LEVEL, on_map, &STARTS, entity), start);
        assert_eq!(
            join_position(Some(&off_map), LEVEL, on_map, &[], entity),
            None
        );
    }
}
