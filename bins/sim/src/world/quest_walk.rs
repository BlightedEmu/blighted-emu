// SPDX-License-Identifier: AGPL-3.0-or-later
//! The quest walk's character: a test player that plays one quest in a world of its own
//! (`sim quest-walk`). It takes the quest, plays its steps and turns it in through the calls a
//! game client makes (the talk with the NPC, the accept, the territory entries, the finished
//! topics, the cinematics and screens, the faction pick, the turn-in). A step whose client flow
//! it does not drive (a kill, a gather, a craft, a catch) is played as the game event the server
//! makes of that flow, after a check that the level holds what the step needs: a mob of the
//! type, a gatherable or another source of the item, a recipe, a place. A step it cannot check
//! or play stops the walk of its quest with the reason.

use std::collections::HashMap;

use nw_bus::{
    Admit, BusId, ConnectionRef, PlayerInventory, PlayerProgression, PlayerQuests, Point,
};
use nw_content::factions::{COVENANT, MARAUDERS, SYNDICATE, faction_requirement_met};
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::client_input::TALK_KIND;
use nw_messages::{ClientFacetMsg, ConversationClientMsg, FacetHeader, InteractionRequest};
use nw_sim_core::Position;
use nw_world::dungeon_scripts::{OPEN_WORLD_LEVEL, TargetSlice};
use nw_world::records::VitalsSpawn;
use uuid::Uuid;

use super::{OWN_CARD_DELAY_MS, Placement, Player, SimWorld};
use crate::objectives::{Event, FishCaught, FishingStep, QuestDrop, TaskKind, kills_count};
use crate::player_content::requirement_met;

/// The time of one world tick.
const TICK_MS: u64 = 33;
/// The ticks after each round of steps.
const TICKS_PER_ROUND: u64 = 3;
/// The time a round waits when its steps changed nothing: a timer or a patrol ends meanwhile.
const WAIT_MS: u64 = 60_000;
/// The most rounds of one quest: a quest that still runs after them has a step that never
/// ends.
const MAX_ROUNDS: u32 = 400;
/// The rounds in a row that may change nothing before the walk stops the quest.
const IDLE_ROUNDS: u32 = 2;
/// How far the walk goes from an area it must leave.
const LEAVE_DISTANCE_M: f32 = 500.0;
/// The name mark before an NPC id in a talk (`@12_JeongIseul`).
const NPC_MARK: &str = "@";
/// The length and weight of a fish the walk catches above a catch task's least values.
const CATCH_MARGIN: f32 = 1.0;
/// The item of a catch task that names no fish (Inferred: a task with no item takes any fish).
const ANY_FISH: &str = "AnyFish";
/// The factions a player may join, in the order the walk tries them.
const JOINABLE: [u8; 3] = [SYNDICATE, MARAUDERS, COVENANT];
/// The marks of an achievement expression ([`requirement_met`]).
const EXPRESSION_MARKS: [&str; 4] = ["&&", "||", "(", ")"];
/// The negation mark of an achievement expression.
const NOT_MARK: char = '!';
/// The enemy type of a kill step that counts player kills.
const PLAYER_ENEMY: &str = "Player";
/// How near a patrol's place the player stands while its stay counts.
const PATROL_REACH_M: f32 = 1.0;
/// The words of a client step in a `!questplan` line.
const CINEMATIC_START: &str = "cinematic-start";
const CINEMATIC_END: &str = "cinematic-end";
const CUTSCENE: &str = "cutscene";
const SCREEN: &str = "screen";
const FACTION: &str = "faction";
/// The words of a `!questplan` line: `plan <task> <what>`.
const PLAN_LINE: &str = "plan";
const PLAN_WAIT: &str = "wait";
/// The plan word of [`StepPlan::AwaitStart`].
const PLAN_AWAIT: &str = "await";
const PLAN_STOP: &str = "stop";
const PLAN_CLIENT: &str = "client";
const PLAN_TALK: &str = "talk";
const PLAN_TOPIC: &str = "topic";
const PLAN_GO: &str = "go";
const PLAN_LEAVE: &str = "leave";
const PLAN_KILL: &str = "kill";
const PLAN_ITEM: &str = "item";
/// The word of a kill plan whose mobs count only as placed (by their spawn category).
const PLAN_PLACED: &str = "placed";
/// The word before each place where a kill plan's encounter spawns a pickup taken by touch (the
/// talisman piles of `QuestLiang`, [`super::boss_scripts::touch_pickup`]): the player walks onto
/// them during the fight.
const PLAN_TOUCH: &str = "touch";
/// The word before the radius around a kill plan's place that the player fights within: the
/// volume of the encounter that spawns the mobs ([`super::dungeon_script::volume_radius_m`]).
const PLAN_WITHIN: &str = "within";
/// The word before the gatherable whose nodes give an item plan's item.
const PLAN_NODE: &str = "node";
const PLAN_ACHIEVEMENTS: &str = "achievements";
const PLAN_PROGRESSION: &str = "progression";
const PLAN_EVENT: &str = "event";
const PLAN_USE: &str = "use";
const PLAN_EQUIP: &str = "equip";
const PLAN_SKIN: &str = "skin";
const PLAN_GATHER: &str = "gather";
/// The word after a gather whose node is gathered again (a turning cyclic stone).
const PLAN_AGAIN: &str = "again";
const PLAN_START: &str = "start";
/// The word of a plan that strikes puzzle objects: `strike <gatherable> at x y z ...`.
const PLAN_STRIKE: &str = "strike";
/// How near an encounter's place its start object and the start object's gather point stand,
/// metres (Observed: the gather point `04A_Side02_02_EncounterStart_01` at 8732.261 4251.937,
/// the encounter `Enc_Quest_04A_Side02_ObeliskDefenders_01` at the same place).
const START_OBJECT_REACH_M: f32 = 2.0;
/// The file name start of an open world encounter's start object slice
/// (`io_worldevent_09a_s3_freshkill.dynamicslice`).
const START_OBJECT_PREFIX: &str = "io_worldevent_";
/// The file name end of a slice.
const SLICE_SUFFIX: &str = ".dynamicslice";
const PLAN_CAMP: &str = "camp";
const PLAN_LORE: &str = "lore";
const PLAN_TRIAL: &str = "trial";
/// The word before the entrance capital of a trial that a player enters at its door.
const PLAN_TRIAL_DOOR: &str = "door";
/// The infix of a locked entrance slice (`4__Trial_Entrance_QuestMedea_Locked`): a player on
/// the quest uses the other door of the same game mode.
const LOCKED_ENTRANCE: &str = "_locked";
const PLAN_LOOT: &str = "loot";
const PLAN_CRAFT: &str = "craft";
/// The word of a fishing line: `fish at <x> <y> <z> yaw <radians> [territory <id>]`.
const PLAN_FISH: &str = "fish";
/// The `ItemClass` part of an instrument a player plays (`EquippableTool+InstrumentFlute`); its
/// skins (`Skin_Flute_DarkFae`) have the instrument class without it.
const INSTRUMENT_TOOL_CLASS: &str = "EquippableTool";
/// The words of a song line: `song <song> sheet <sheet> seconds <length> at <x> <y> <z>`.
const PLAN_SONG: &str = "song";
const PLAN_SONG_SHEET: &str = "sheet";
const PLAN_SONG_SECONDS: &str = "seconds";
/// The words of an item use line: `useitem <item> bag <slot>`.
const PLAN_USE_ITEM: &str = "useitem";
const PLAN_USE_ITEM_BAG: &str = "bag";
/// The word of a line of player kills: `pvp <count>`.
const PLAN_PVP: &str = "pvp";
const PLAN_FISH_YAW: &str = "yaw";
/// How many rolls of a gatherable's loot table measure how often it gives a step's item.
pub(super) const SOURCE_ROLLS: u32 = 64;
/// The fixed random sequence of those rolls (a 64-bit linear congruential generator, Knuth's
/// MMIX constants), so a plan names the same source every time.
const SOURCE_ROLL_SEED: u64 = 0x2545_f491_4f6c_dd1d;
const SOURCE_ROLL_MULTIPLIER: u64 = 6_364_136_223_846_793_005;
const SOURCE_ROLL_INCREMENT: u64 = 1_442_695_040_888_963_407;
/// The high bits of the generator's state are its best ones.
const SOURCE_ROLL_SHIFT: u32 = 33;
/// The word before each ingredient of a craft plan.
const PLAN_CRAFT_WITH: &str = "with";
/// The word before each ingredient of a craft plan that the bag lacks.
const PLAN_CRAFT_GIVE: &str = "give";
const PLAN_NOT_RUNNING: &str = "not running";

/// `bytes` as lowercase hex, as a plan line writes an id.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// What the level holds that a quest's steps need: the places of its NPCs, territories and
/// mobs, who offers each quest, who holds each topic and what gives each achievement. The walk
/// checks the steps against it, and `!questplan` names its places (built at the sim's start by
/// [`crate::quest_walk::quest_guide`]).
#[derive(Debug, Clone, PartialEq)]
pub struct QuestGuide {
    /// Each placed NPC of the level, by the CRC-32 of its lowercase id.
    pub npcs: HashMap<u32, Position>,
    /// A place in each territory: the first record of the level that lies in it.
    pub territory_spots: HashMap<u16, Position>,
    /// The NPCs whose conversation states offer each quest, by lowercase quest id.
    pub givers: HashMap<String, Vec<String>>,
    /// The NPCs that hold each conversation topic, by the CRC-32 of the lowercase topic id.
    pub topic_npcs: HashMap<u32, Vec<String>>,
    /// What gives each achievement (a quest's completion or a step's unlock), by lowercase id.
    pub achievement_sources: HashMap<String, String>,
    /// The success game events of the quests, lowercase: a quest's end fires its event.
    pub success_events: std::collections::HashSet<String>,
    /// Every vitals spawn of the level, also the encounter spawns the open map does not run.
    pub all_spawns: Vec<VitalsSpawn>,
    /// Where a character stands when no NPC gives a place.
    pub start: Position,
}

impl Default for QuestGuide {
    fn default() -> Self {
        Self {
            npcs: HashMap::new(),
            territory_spots: HashMap::new(),
            givers: HashMap::new(),
            topic_npcs: HashMap::new(),
            achievement_sources: HashMap::new(),
            success_events: std::collections::HashSet::new(),
            all_spawns: Vec::new(),
            start: Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        }
    }
}

impl QuestGuide {
    /// The place of NPC `npc` (any case), when the level has it.
    fn npc_at(&self, npc: &str) -> Option<Position> {
        self.npcs.get(&lowercase_crc32(npc)).copied()
    }
}

/// How the walk of one quest ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Outcome {
    /// It ran from the accept to its end.
    Completed,
    /// A step cannot end on this server: the stop names it and why.
    Broken,
    /// A step needs something the walk cannot tell is there: the stop names it.
    NeedsLook,
    /// A step needs a flow the walk does not play (an expedition, a house).
    NotWalked,
    /// The quest did not start.
    NotStarted,
}

/// The walk of one quest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestWalk {
    /// The quest id.
    pub quest: String,
    /// How it ended.
    pub outcome: Outcome,
    /// The step it stopped at and why; `None` for a completed quest.
    pub stop: Option<String>,
    /// What the walk did that a reader should know: the steps played as events, the NPCs it
    /// did not find, the requirements it gave.
    pub notes: Vec<String>,
}

/// What one step's play did.
enum Play {
    /// The walk did what the step asks; with a note of how.
    Done(Option<String>),
    /// The step waits for time (a timer, a patrol stay) or for another quest.
    Waits,
    /// The step cannot end on this server.
    Broken(String),
    /// The walk cannot tell whether the level has what the step needs.
    NeedsLook(String),
    /// The step needs a flow the walk does not play.
    NotWalked(String),
}

/// Where one task of a quest stands: its id's CRC-32, its progress and its status.
type TaskMark = (u32, u32, u8);

/// A waiting step of one of the character's quests.
pub(crate) struct WaitingStep {
    quest: String,
    task: String,
    kind: TaskKind,
    progress: u32,
    /// Its row's `GameModeID`: the instance game mode the step is played in
    /// ([`crate::objectives::ObjectiveRun::task_game_mode`]).
    game_mode: Option<String>,
    /// Whether a task of its quest, waiting or not, drops the step's item from containers
    /// ([`crate::objectives::ObjectiveRun::drops_from_containers`]).
    container_item: bool,
}

/// The connection of walk character `number` on gateway `gateway`.
fn walk_connection(gateway: &BusId, number: u64) -> ConnectionRef {
    ConnectionRef {
        gateway: gateway.clone(),
        connection: number,
    }
}

/// The facet header of walk character `number`'s messages.
fn header(number: u64) -> FacetHeader {
    FacetHeader::of(0, number)
}

/// The achievement names of expression `expression`, each with whether it is negated.
pub(super) fn expression_names(expression: &str) -> Vec<(String, bool)> {
    let mut spaced = expression.to_owned();
    for mark in EXPRESSION_MARKS {
        spaced = spaced.replace(mark, " ");
    }
    spaced
        .split_whitespace()
        .map(|word| match word.strip_prefix(NOT_MARK) {
            Some(name) => (name.to_owned(), true),
            None => (word.to_owned(), false),
        })
        .filter(|(name, _)| !name.is_empty())
        .collect()
}

impl SimWorld {
    /// Walks quest `quest` with a new character `number` from `now_ms` on (advanced to the
    /// walk's end), and takes the character out again.
    pub(crate) fn walk_quest(
        &mut self,
        level: &QuestGuide,
        gateway: &BusId,
        number: u64,
        quest: &str,
        now_ms: &mut u64,
    ) -> QuestWalk {
        let connection = walk_connection(gateway, number);
        let mut walk = QuestWalk {
            quest: quest.to_owned(),
            outcome: Outcome::Completed,
            stop: None,
            notes: Vec::new(),
        };
        self.walk_quest_with(level, number, &connection, &mut walk, now_ms);
        self.leave(&connection);
        self.quests.remove(&number);
        walk
    }

    fn walk_quest_with(
        &mut self,
        level: &QuestGuide,
        number: u64,
        connection: &ConnectionRef,
        walk: &mut QuestWalk,
        now_ms: &mut u64,
    ) {
        let Some(def) = self.content.quests.quest(&walk.quest) else {
            walk.outcome = Outcome::NotStarted;
            walk.stop = Some("no such quest, or its root task is not a task row".to_owned());
            return;
        };
        let id = def.id.clone();
        let lower = id.to_lowercase();
        let givers = level.givers.get(&lower).cloned().unwrap_or_default();
        let giver = givers
            .iter()
            .find_map(|npc| level.npc_at(npc).map(|at| (npc.clone(), at)));
        match (&giver, givers.is_empty(), def.mission.is_some()) {
            (_, _, true) => walk
                .notes
                .push("a town board mission: taken without a board".to_owned()),
            (None, true, false) => walk
                .notes
                .push("no NPC offers it (a chain step or a scripted start)".to_owned()),
            (None, false, false) => walk.notes.push(format!(
                "no NPC that offers it is in the open world: {}",
                givers.join(", ")
            )),
            (Some(_), _, false) => {}
        }
        let start = giver.as_ref().map_or(level.start, |(_, at)| *at);
        let backstory = self.walk_backstory(def.requirement.as_deref());
        if let Err(reason) = self.walk_admit(number, connection, start, backstory, now_ms) {
            walk.outcome = Outcome::NotStarted;
            walk.stop = Some(reason);
            return;
        }
        self.walk_meet_requirement(level, number, def.requirement.as_deref(), walk, *now_ms);
        if let Some(faction) = def.required_faction.as_deref() {
            self.walk_join_faction(number, connection, faction, walk, *now_ms);
        }
        if let Some((npc, _)) = &giver {
            self.walk_talk(level, number, connection, npc, *now_ms);
        }
        let objective = lowercase_crc32(&lower);
        let accept = ClientFacetMsg::AcceptObjective {
            header: header(number),
            objective,
        };
        if def.mission.is_none() {
            let _ = self.facet_message(connection, accept, *now_ms);
        }
        if !self.walk_runs(number, &id) {
            match self.accept_quest(number, &id, None, *now_ms) {
                Ok(()) if def.mission.is_none() => walk
                    .notes
                    .push("the accept message did not start it; a direct start did".to_owned()),
                Ok(()) => {}
                Err(refusal) => {
                    walk.outcome = Outcome::NotStarted;
                    walk.stop = Some(refusal.to_string());
                    return;
                }
            }
        }
        let mut idle = 0;
        let mut turned_in = false;
        for _ in 0..MAX_ROUNDS {
            self.walk_ticks(now_ms);
            if !self.walk_runs(number, &id) {
                let done = self
                    .player_by_number(number)
                    .and_then(|entity| self.world.get::<Player>(entity))
                    .is_some_and(|player| {
                        player
                            .done_quests
                            .iter()
                            .any(|done| done.eq_ignore_ascii_case(&id))
                    });
                if !done {
                    walk.outcome = Outcome::Broken;
                    walk.stop = Some("the quest left the journal before its end".to_owned());
                }
                return;
            }
            if self.walk_finished(number, &id) {
                if turned_in {
                    walk.outcome = Outcome::Broken;
                    walk.stop =
                        Some("its tasks are done, but the turn-in does not end it".to_owned());
                    return;
                }
                if let Some(npc) = def.turn_in.as_deref() {
                    if level.npc_at(npc).is_none() {
                        walk.notes
                            .push(format!("its turn-in NPC {npc} is not in the open world"));
                    }
                    self.walk_talk(level, number, connection, npc, *now_ms);
                }
                let complete = ClientFacetMsg::CompleteObjective {
                    header: header(number),
                    objective,
                };
                let _ = self.facet_message(connection, complete, *now_ms);
                turned_in = true;
                continue;
            }
            let before = self.walk_signature(number);
            let steps = self.walk_waiting(number);
            let mut stops = Vec::new();
            let mut waits = false;
            let at = self
                .player_by_number(number)
                .and_then(|entity| self.world.get::<super::Body>(entity))
                .map_or(level.start, |body| body.motion.position());
            for step in &steps {
                let plan = self.walk_plan(level, step, at);
                match self.walk_act(level, number, connection, plan, *now_ms) {
                    Play::Done(note) => {
                        if let Some(note) = note {
                            let note = format!("{}: {note}", step.task);
                            if !walk.notes.contains(&note) {
                                walk.notes.push(note);
                            }
                        }
                    }
                    Play::Waits => waits = true,
                    Play::Broken(reason) => stops.push((Outcome::Broken, step, reason)),
                    Play::NeedsLook(reason) => stops.push((Outcome::NeedsLook, step, reason)),
                    Play::NotWalked(reason) => stops.push((Outcome::NotWalked, step, reason)),
                }
            }
            self.walk_ticks(now_ms);
            if self.walk_signature(number) != before {
                idle = 0;
                continue;
            }
            idle += 1;
            if waits || idle < IDLE_ROUNDS {
                *now_ms += WAIT_MS;
                continue;
            }
            stops.sort_by_key(|(outcome, _, _)| *outcome);
            let (outcome, stop) = match stops.first() {
                Some((outcome, step, reason)) => (*outcome, Self::stop_text(step, reason)),
                None => (
                    Outcome::Broken,
                    format!(
                        "no step progresses after the walk did what each asks: {}",
                        steps
                            .iter()
                            .map(|step| format!("{} ({})", step.task, step.kind.name()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ),
            };
            walk.outcome = outcome;
            walk.stop = Some(stop);
            return;
        }
        walk.outcome = Outcome::Broken;
        walk.stop = Some(format!("still running after {MAX_ROUNDS} rounds"));
    }

    /// The stop text of `step` for `reason`.
    fn stop_text(step: &WaitingStep, reason: &str) -> String {
        format!(
            "{} / {} ({}): {reason}",
            step.quest,
            step.task,
            step.kind.name()
        )
    }

    /// Admits walk character `number` at `at` as a new character and waits until its own card
    /// went out and its first time sync came.
    fn walk_admit(
        &mut self,
        number: u64,
        connection: &ConnectionRef,
        at: Position,
        backstory: Option<String>,
        now_ms: &mut u64,
    ) -> Result<(), String> {
        let admit = Admit {
            connection: connection.clone(),
            account: Uuid::new_v4(),
            character: Uuid::new_v4(),
            name: format!("Walker{number}"),
            entity: number,
            position: Point {
                x: at.x,
                y: at.y,
                z: at.z,
            },
            login: true,
            handoffs: 0,
            bundles_sent: 0,
            look: None,
            backstory,
            items: None,
            inventory: PlayerInventory::default(),
            quests: PlayerQuests::default(),
            progression: PlayerProgression::default(),
            game_master: false,
            account_in_probation: false,
            entitlements: Vec::new(),
            balances: nw_bus::AccountBalances::default(),
        };
        self.admit(&admit, *now_ms, *now_ms)
            .map_err(|error| format!("the character was not admitted: {error}"))?;
        *now_ms += OWN_CARD_DELAY_MS;
        self.walk_ticks(now_ms);
        self.heard_from(connection);
        let top = self.content.progression.max_level();
        self.set_level(number, top);
        Ok(())
    }

    /// The archetype a character of a quest with requirement `requirement` picks: the one whose
    /// unlock the requirement names, else the first one, so that it wears a starter kit.
    fn walk_backstory(&self, requirement: Option<&str>) -> Option<String> {
        let unlocks = &self.content.archetype_unlocks;
        let named = requirement.and_then(|requirement| {
            expression_names(requirement)
                .into_iter()
                .filter(|(_, negated)| !negated)
                .find_map(|(name, _)| {
                    unlocks
                        .iter()
                        .find(|(_, unlock)| unlock.eq_ignore_ascii_case(&name))
                })
        });
        named
            .or_else(|| unlocks.first())
            .map(|(backstory, _)| backstory.clone())
    }

    /// What gives achievement `name` on this level: a quest's end or step, a lore note of the
    /// level, or an archetype pick.
    pub(super) fn walk_achievement_source(&self, level: &QuestGuide, name: &str) -> Option<String> {
        if let Some(source) = level.achievement_sources.get(&name.to_lowercase()) {
            return Some(source.clone());
        }
        if let Some(note) = self.content.lore_notes.unlocking(name) {
            return self
                .content
                .lore_points
                .first(note.id)
                .map(|_| format!("the lore note {} of the level", note.title));
        }
        self.content
            .archetype_unlocks
            .iter()
            .find(|(_, unlock)| unlock.eq_ignore_ascii_case(name))
            .map(|(backstory, _)| format!("the archetype pick {backstory}"))
    }

    /// Runs the world for [`TICKS_PER_ROUND`] ticks from `now_ms` on.
    fn walk_ticks(&mut self, now_ms: &mut u64) {
        for _ in 0..TICKS_PER_ROUND {
            *now_ms += TICK_MS;
            let _ = self.tick(*now_ms, *now_ms);
        }
    }

    /// Gives walk character `number` every achievement that quest requirement `requirement`
    /// names without a negation, as the quests and steps before it would.
    fn walk_meet_requirement(
        &mut self,
        level: &QuestGuide,
        number: u64,
        requirement: Option<&str>,
        walk: &mut QuestWalk,
        now_ms: u64,
    ) {
        let Some(requirement) = requirement else {
            return;
        };
        for (name, negated) in expression_names(requirement) {
            if negated {
                continue;
            }
            match self.walk_achievement_source(level, &name) {
                Some(source) => walk.notes.push(format!(
                    "needs achievement {name}: given, as {source} gives it"
                )),
                None => walk.notes.push(format!(
                    "needs achievement {name}: given, but no quest or step gives it"
                )),
            }
            self.earn_achievement(number, &name, now_ms);
        }
        let met = self
            .player_by_number(number)
            .and_then(|entity| self.world.get::<Player>(entity))
            .is_some_and(|player| {
                requirement_met(requirement, |id| {
                    player.flow.has_achievement(&self.achievement_rules, id)
                })
            });
        if !met {
            walk.notes
                .push(format!("its requirement {requirement} is still not met"));
        }
    }

    /// Joins walk character `number` to the first faction that `RequiredFaction` cell
    /// `required` takes, with the client's pick.
    fn walk_join_faction(
        &mut self,
        number: u64,
        connection: &ConnectionRef,
        required: &str,
        walk: &mut QuestWalk,
        now_ms: u64,
    ) {
        let Some(faction) = JOINABLE
            .into_iter()
            .find(|faction| faction_requirement_met(Some(required), *faction))
        else {
            walk.notes
                .push(format!("no faction a player may join meets {required}"));
            return;
        };
        let pick = ClientFacetMsg::SetFaction {
            header: header(number),
            faction,
        };
        let _ = self.facet_message(connection, pick, now_ms);
    }

    /// Walk character `number` goes to NPC `npc` when the level has it and talks to it.
    fn walk_talk(
        &mut self,
        level: &QuestGuide,
        number: u64,
        connection: &ConnectionRef,
        npc: &str,
        now_ms: u64,
    ) {
        if let Some(at) = level.npc_at(npc) {
            self.walk_go(number, at, now_ms);
        }
        let request = InteractionRequest {
            target: [0; nw_messages::wire::RAW_ID_LEN],
            target_entity: 0,
            interaction: now_ms,
            kind: TALK_KIND,
            gatherable: None,
        };
        let name = format!("{NPC_MARK}{npc}");
        self.interact(connection, request, name.as_bytes(), now_ms);
    }

    /// Moves walk character `number` to `at`.
    fn walk_go(&mut self, number: u64, at: Position, now_ms: u64) {
        if let Some(entity) = self.player_by_number(number) {
            self.place_player(entity, at, None, Placement::Drop, now_ms);
        }
    }

    /// Whether walk character `number` runs quest `id`.
    fn walk_runs(&self, number: u64, id: &str) -> bool {
        self.quests_of_player(number)
            .iter()
            .any(|quest| quest.def.id.eq_ignore_ascii_case(id))
    }

    /// Whether walk character `number`'s quest `id` has all its tasks done.
    fn walk_finished(&self, number: u64, id: &str) -> bool {
        self.quests_of_player(number)
            .iter()
            .any(|quest| quest.def.id.eq_ignore_ascii_case(id) && quest.run.finished())
    }

    /// What walk character `number`'s quests stand at: the progress of each.
    fn walk_signature(&self, number: u64) -> Vec<(String, Vec<TaskMark>)> {
        self.quests_of_player(number)
            .iter()
            .map(|quest| {
                (
                    quest.def.id.clone(),
                    quest
                        .run
                        .progress()
                        .iter()
                        .map(|task| (task.task, task.progress, task.status as u8))
                        .collect(),
                )
            })
            .collect()
    }

    /// The waiting steps of walk character `number`'s quests.
    fn walk_waiting(&self, number: u64) -> Vec<WaitingStep> {
        self.quests_of_player(number)
            .iter()
            .flat_map(|quest| {
                quest
                    .run
                    .waiting_steps()
                    .into_iter()
                    .map(|(task, kind, progress)| WaitingStep {
                        quest: quest.def.id.clone(),
                        task: task.to_owned(),
                        container_item: match kind {
                            TaskKind::HaveItems { item, .. }
                            | TaskKind::GatherItems { item, .. } => {
                                quest.run.drops_from_containers(item)
                            }
                            _ => false,
                        },
                        kind: kind.clone(),
                        progress,
                        game_mode: quest.run.task_game_mode(task).map(str::to_owned),
                    })
            })
            .collect()
    }
}

/// The place of record position `at`.
fn position_of(at: [f32; 3]) -> Position {
    Position {
        x: at[0],
        y: at[1],
        z: at[2],
    }
}

/// A step a client plays with its own calls: a cinematic, a cutscene, a screen, a faction pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClientStep {
    /// The cinematic video of the id starts.
    CinematicStart(u32),
    /// The cinematic video of the id ends.
    CinematicEnd(u32),
    /// The in-game cutscene of the id plays to its end.
    Cutscene(u32),
    /// The UI screen of the id opens.
    Screen(u32),
    /// The player picks the faction.
    Faction(u8),
}

impl ClientStep {
    /// The calls the client sends for the step, with facet header `header`: a cutscene is asked
    /// for, then ends.
    fn calls(self, header: FacetHeader) -> Vec<ClientFacetMsg> {
        match self {
            Self::CinematicStart(cinematic) => {
                vec![ClientFacetMsg::CinematicStarted { header, cinematic }]
            }
            Self::CinematicEnd(cinematic) => {
                vec![ClientFacetMsg::CinematicEnded { header, cinematic }]
            }
            Self::Cutscene(cutscene) => vec![
                ClientFacetMsg::RequestCutscene { header, cutscene },
                ClientFacetMsg::CutsceneEnded { header },
            ],
            Self::Screen(screen) => vec![ClientFacetMsg::UiScreenOpened { header, screen }],
            Self::Faction(faction) => vec![ClientFacetMsg::SetFaction { header, faction }],
        }
    }

    /// The step's words in a `!questplan` line: its name, then its id in hexadecimal (a
    /// faction's number in decimal).
    fn words(self) -> String {
        match self {
            Self::CinematicStart(id) => format!("{CINEMATIC_START} {id:08x}"),
            Self::CinematicEnd(id) => format!("{CINEMATIC_END} {id:08x}"),
            Self::Cutscene(id) => format!("{CUTSCENE} {id:08x}"),
            Self::Screen(id) => format!("{SCREEN} {id:08x}"),
            Self::Faction(faction) => format!("{FACTION} {faction}"),
        }
    }
}

/// A flow the walk plays as the game event the server makes of it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FlowEvent {
    /// A container was looted.
    ContainerLooted,
    /// The recipe was crafted.
    Crafted(String),
    /// An item of family `family` was crafted; `item` is the item the step names
    /// (`TaskCraftWeaponFamily` `ItemName`, `1hSwordT2` of `06A_TS_02_ForgeSword_craftRecipe`).
    CraftedFamily {
        family: String,
        item: Option<String>,
    },
    /// The item was equipped.
    Equipped(String),
    /// The game event fired.
    GameEvent(String),
    /// An object of the interact tag was used.
    Interacted(String),
    /// The cyclic gatherable was gathered in the state.
    Cyclic { cyclic: String, state: u32 },
    /// The item was used.
    Used(String),
    /// A step of fishing.
    Fishing(FishingStep),
    /// A fish was caught.
    Caught {
        item: String,
        length: f32,
        weight: f32,
        hotspot: bool,
    },
    /// The item was salvaged.
    Salvaged(String),
    /// The song was performed at the place.
    Song { song: String, at: Position },
    /// The section of the quick course of so many sections was passed.
    Course {
        course: String,
        section: u32,
        sections: u32,
    },
    /// The player died.
    Died,
}

/// What a waiting step needs a player to do next. The plan checks the level (a mob of the type,
/// a source of the item, a recipe, a place, an NPC) and changes nothing. The walk carries it out
/// in its own world ([`SimWorld::walk_act`]); `!questplan` names it to a game master or a test
/// bot ([`SimWorld::plan_line`]).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StepPlan {
    /// Wait: for a timer, a patrol stay or another quest.
    Wait,
    /// Wait at `at`, in the volume of the open world encounter that spawns there the start
    /// object of gatherable `gatherable` once its earlier stages ran (a player who comes stands
    /// there until it is up).
    AwaitStart { gatherable: String, at: Position },
    /// Strike each of these puzzle objects (gatherable, place) with a weapon hit, standing at it
    /// (`super::puzzles`, the gongs of `PuzzleMSQGong`).
    Strike(Vec<(String, Position)>),
    /// Nothing: a pin task (a game event task with no event, `neverbetrue`) never ends; its
    /// container ends through its other steps (a `PartialTaskContainer` through any one), so
    /// it alone says nothing of the quest (`12A_MSQ_24_DefeatGawain_Enter_PinTask` beside the
    /// `QuestGawain` run). When nothing else moves, the round's "no step progresses" stop
    /// names it.
    Pin,
    /// The step cannot be played: how the walk ends, and why.
    Stop(Outcome, String),
    /// Send the client calls of this step.
    Client(ClientStep),
    /// Talk to the NPC, at its place when the level has it.
    Talk { npc: String, at: Option<Position> },
    /// Finish the conversation topic, after a talk with the NPC that holds it, when one does.
    Topic {
        topic: u32,
        npc: Option<(String, Position)>,
    },
    /// Go to the place, entering the territory when one is named.
    Go {
        at: Position,
        territory: Option<u16>,
    },
    /// Leave these territories.
    Leave(Vec<u16>),
    /// Kill so many mobs of the spawn, in the territory when one is named; `spawned`: a mob
    /// made from the spawn's vitals alone counts (else only the placed mobs count, by their
    /// spawn category). The spawn is one the sim runs, or the level's record of a mob that
    /// the open world encounter `encounter` (its slice) spawns in a stage.
    Kill {
        spawn: VitalsSpawn,
        count: u32,
        territory: Option<u16>,
        spawned: bool,
        encounter: Option<String>,
    },
    /// Get so many of the item from the source; `gathered`: the step counts gathers; `node`:
    /// the gatherable whose nodes give it, when a gather gives it.
    Item {
        item: String,
        count: u32,
        gathered: bool,
        source: String,
        node: Option<String>,
    },
    /// Earn the achievements, each with what gives it.
    Achievements(Vec<(String, String)>),
    /// Set the progression to the target, a rank or points.
    Progression {
        id: String,
        target: u32,
        points: bool,
    },
    /// Play the flow this many times.
    Event(FlowEvent, u32),
    /// Use the level object `object` (its facet entity key) at `at` this many times: open it,
    /// then end the interaction, as a client does (the tag counts at the end of the
    /// interaction, [`SimWorld::interaction_ended`]); the nearest object with the step's tag.
    Use {
        tag: String,
        count: u32,
        object: u64,
        at: Position,
    },
    /// Equip item `item` from bag slot `bag_slot` (its stack's instance `instance`) into
    /// paperdoll slot `slot`, as the client's equip call names them ([`super::gear::Gear::equip_place`]); a
    /// `!questplan` answer gives it for an equip step whose item is in the player's bag.
    Equip {
        item: String,
        bag_slot: u32,
        instance: [u8; nw_messages::wire::RAW_ID_LEN],
        slot: u32,
    },
    /// Skin `count` corpses of gatherable `gatherable` (its `Skinning` row, named as the step's
    /// drop enemy) after `kill`, the kill of one: a gather step whose item is that enemy's quest
    /// drop gets it from the skin, not the kill ([`SimWorld::quest_drops`]). A `!questplan`
    /// answer gives it in place of `item`, the plan the walk plays.
    Skin {
        gatherable: String,
        count: u32,
        kill: Box<StepPlan>,
        item: Box<StepPlan>,
    },
    /// Open the loot bags of `kill`'s kills: an item step whose item is the quest drop of an
    /// enemy gets it from the loot bag of a kill of one
    /// ([`crate::objectives::ObjectiveRun::kill_drops`]; `12A_MSQ_15_DefeatNavigator_CollectGun`,
    /// the three seals of `12A_MSQ_20_CollectCrown`). A `!questplan` answer gives it in place of
    /// `item`, the plan the walk plays.
    Loot {
        kill: Box<StepPlan>,
        item: Box<StepPlan>,
    },
    /// The fight at the encounter whose success fires the step's game event: `kill`, a fight at
    /// the encounter's place with no spawn by a game master (`04A_Side02_02_01a_Task_Encounter_01`
    /// waits for `04A_Side02_02_Encounter_01_Success` of `Enc_Quest_04A_Side02_ObeliskDefenders_01`).
    /// A `!questplan` answer gives it in place of `event`, the plan the walk plays.
    Encounter {
        kill: Box<StepPlan>,
        event: Box<StepPlan>,
    },
    /// The gather of the start object that the encounter whose success fires the step's game
    /// event spawned and that stands: gatherable `gatherable` (its gather point at the
    /// encounter's place), its card of slice asset `asset`, at `at` (the stage `Wave_01:
    /// Interact with IO` of `Enc_Quest_04A_Side02_ObeliskDefenders_01` waits for the gather of
    /// `io_04a_side02_obeliskdefenders_01_encstart`). A `!questplan` answer gives it in place
    /// of `event`, the plan the walk plays.
    Start {
        gatherable: String,
        asset: [u8; 16],
        at: Position,
        event: Box<StepPlan>,
    },
    /// Craft recipe `recipe` once at the station of `GdeRef` `station` at `at`, from the bag's
    /// `ingredients` (item, quantity), of which the bag lacks `missing` (a player gathers or buys
    /// them first: the timber and leather of `12A_Side_16_ToGoodUse`'s sword); the retail cook
    /// of `12_boarfeast_05_01` opens the level
    /// campfire `@ui_watcherscampfire` (3743), crafts with the pork it carries (2997), and closes
    /// the screen (2999) and the interaction (2473) after the progress (2993). A `!questplan`
    /// answer gives it in place of `event`, the plan the walk plays.
    Craft {
        recipe: String,
        station: [u8; nw_messages::wire::RAW_ID_LEN],
        at: Position,
        ingredients: Vec<(String, u32)>,
        missing: Vec<(String, u32)>,
        event: Box<StepPlan>,
    },
    /// Gather `count` nodes of gatherable `gatherable`, whose gather fires the step's game event
    /// (the rescue gathers `MB_MSQ2_RescueMiner_Rescue01` of `12A_MSQ_07_FreeMiners`). A
    /// `!questplan` answer gives it in place of `event`, the plan the walk plays. With
    /// `again`, the same node is gathered again: a turning cyclic stone (retail
    /// `06A_MSQ_18_ReturnSiriusJewel`: the client gathered `Cyclic_06A_MSQ_puzzle2_stone1` twice
    /// for its state 1, `..._stone2` once for its state 0).
    Gather {
        gatherable: String,
        count: u32,
        again: bool,
        event: Box<StepPlan>,
    },
    /// Place a camp at `at` and build it (its `BindCamp` event, the camp tutorial of
    /// `12A_MSQ_12_CampTutorial`): the placement's skin, the camp's slice asset and its costs.
    /// A `!questplan` answer gives it in place of `event`, the plan the walk plays.
    Camp {
        at: Position,
        camp: super::travel::CampPlan,
        event: Box<StepPlan>,
    },
    /// Read the lore note of title `title` at its lore point `at`, whose read sets the step's
    /// `Lore_unlock_` achievement ([`SimWorld::read_lore`]; `12A_MSQ_17_LochnirsNotes`). A
    /// `!questplan` answer gives it in place of `achievements`, the plan the walk plays.
    Lore {
        title: String,
        at: Position,
        achievements: Box<StepPlan>,
    },
    /// Perform song `song` from sheet `sheet` (`duration_s` seconds) at `at`, with an instrument
    /// of the sheet worn (retail `04a_side05_02_playfortown`: the flute in slot 39, then the
    /// performance). A `!questplan` answer gives it in place of `event`, the plan the walk plays.
    Song {
        song: String,
        sheet: String,
        duration_s: f64,
        at: Position,
        event: Box<StepPlan>,
    },
    /// Use the item of bag slot `bag_slot`, `item` (`TaskUseItems`: `09A_S0_UsePotion` drinks
    /// `09A_S0_SporeElixir`). A `!questplan` answer gives it in place of `event`, the plan the
    /// walk plays.
    UseItem {
        item: String,
        bag_slot: u32,
        event: Box<StepPlan>,
    },
    /// Kill `count` players, PvP foes of the player (game event `PlayerKill`, fired at a
    /// player's kill of a player: `C10A_Q6_KillPlayers`). A `!questplan` answer gives it in
    /// place of `event`, the plan the walk plays.
    Pvp { count: u32, event: Box<StepPlan> },
    /// Fish from `at`, looking along `yaw` (radians), until a catch: the place on dry ground
    /// from which a cast lands in water ([`SimWorld::fishing_place`]), in territory `territory`
    /// when the step's fish bites only there. A `!questplan` answer gives it in place of
    /// `event`, the plan the walk plays.
    Fish {
        at: Position,
        yaw: f32,
        territory: Option<u32>,
        event: Box<StepPlan>,
    },
    /// Enter an instance of game mode `mode` (`crc`, the CRC-32 of its lowercase id). With no
    /// `door`, from afar: 5039, 5037, then 946 ([`SimWorld::arena_call`]; a game mode of
    /// `remote_entry`, as the `QuestGawain` trial of `12A_MSQ_24_DefeatGawain`). With a `door`,
    /// at that entrance capital (and its place, when the level's entrance entities hold it): the
    /// dungeon menu (interaction 71), the entry (72), then 946 (a mode that a player may not
    /// enter from afar, as the `QuestMedea` trial of `06A_MSQ_21_DefeatBoss`). The step counts
    /// at the arrival in the instance. A `!questplan` answer gives it in place of `stop`, the
    /// plan the walk plays.
    Trial {
        mode: String,
        crc: u32,
        door: Option<(u64, Option<Position>)>,
        stop: Box<StepPlan>,
    },
}

/// Where a player gets an item: the source in words, and the gatherable whose nodes give it
/// when a gather gives it.
struct ItemSource {
    said: String,
    node: Option<String>,
}

impl ItemSource {
    const fn said(said: String) -> Self {
        Self { said, node: None }
    }

    fn gathered(said: String, gatherable: &str) -> Self {
        Self {
            said,
            node: Some(gatherable.to_owned()),
        }
    }
}

/// `plan`, with a step of player kills (game event `PlayerKill`) as kills of PvP foes
/// ([`StepPlan::Pvp`]); any other plan as it is.
fn pvp_for_event(plan: StepPlan) -> StepPlan {
    match plan {
        StepPlan::Event(FlowEvent::GameEvent(ref fired), count)
            if fired.eq_ignore_ascii_case(super::attack::PLAYER_KILL_EVENT) =>
        {
            StepPlan::Pvp {
                count,
                event: Box::new(plan),
            }
        }
        other => other,
    }
}

impl SimWorld {
    /// What waiting step `step` of a quest needs a player to do next, on the level of `level`.
    /// `at` is where the player stands.
    pub(crate) fn walk_plan(
        &self,
        level: &QuestGuide,
        step: &WaitingStep,
        at: Position,
    ) -> StepPlan {
        let content = &self.content;
        let remaining = |count: u32| count.saturating_sub(step.progress).max(1);
        let event = |flow, count| StepPlan::Event(flow, remaining(count));
        let broken = |reason: String| StepPlan::Stop(Outcome::Broken, reason);
        match &step.kind {
            TaskKind::Consecutive
            | TaskKind::Partial
            | TaskKind::Simple
            | TaskKind::Timer { .. }
            | TaskKind::FailTimer => StepPlan::Wait,
            TaskKind::UnlockAchievement(_)
            | TaskKind::LockAchievement(_)
            | TaskKind::TemporaryUnlock(_)
            | TaskKind::RemoveItem { .. }
            | TaskKind::GiveItem { .. }
            | TaskKind::TriggerGameEvent(_)
            | TaskKind::Empty => broken("a step that ends at its start still waits".to_owned()),
            TaskKind::HasAchievement(expression)
                if let Some(object) = super::puzzles::object_giving(expression) =>
            {
                // A puzzle object's use gives it: the gather of that object (the altars).
                let gatherable = object.gatherable.to_owned();
                let fired = content
                    .gatherables
                    .get(lowercase_crc32(&gatherable))
                    .and_then(|known| known.game_event.clone())
                    .unwrap_or_else(|| gatherable.clone());
                StepPlan::Gather {
                    gatherable,
                    count: 1,
                    again: false,
                    event: Box::new(event(FlowEvent::GameEvent(fired), 1)),
                }
            }
            TaskKind::SlayerScript(script) => match super::puzzles::puzzle_of(script) {
                Some(puzzle) if puzzle.used == super::puzzles::PuzzleUse::Strike => {
                    let targets: Vec<(String, Position)> = puzzle
                        .objects
                        .iter()
                        .filter_map(|object| {
                            let at = content
                                .gather_points
                                .first(lowercase_crc32(object.gatherable))?;
                            Some((object.gatherable.to_owned(), at))
                        })
                        .collect();
                    if targets.len() == puzzle.objects.len() {
                        StepPlan::Strike(targets)
                    } else {
                        broken(format!(
                            "a gong of puzzle {script} has no place on the level"
                        ))
                    }
                }
                // A gather puzzle ends by its achievement steps' gathers (the altars).
                Some(_) => StepPlan::Wait,
                None => broken(format!(
                    "the server has no puzzle of slayer script {script}"
                )),
            },
            TaskKind::HasAchievement(expression) => {
                let mut given = Vec::new();
                for (name, negated) in expression_names(expression) {
                    if negated {
                        continue;
                    }
                    match self.walk_achievement_source(level, &name) {
                        Some(source) => given.push((name, source)),
                        None => {
                            return StepPlan::Stop(
                                Outcome::NeedsLook,
                                format!("waits for achievement {name}; no quest or step gives it"),
                            );
                        }
                    }
                }
                StepPlan::Achievements(given)
            }
            TaskKind::CinematicStart(cinematic) => {
                StepPlan::Client(ClientStep::CinematicStart(*cinematic))
            }
            TaskKind::CinematicComplete(cinematic) => {
                StepPlan::Client(ClientStep::CinematicEnd(*cinematic))
            }
            TaskKind::CutsceneComplete(cutscene) => {
                StepPlan::Client(ClientStep::Cutscene(*cutscene))
            }
            TaskKind::OpenUiScreen(screen) => StepPlan::Client(ClientStep::Screen(*screen)),
            TaskKind::ChooseFaction => StepPlan::Client(ClientStep::Faction(SYNDICATE)),
            TaskKind::TriggerArea { area, leave, place } => match (area, place) {
                (Some(area), _) => {
                    let spot = area.spot();
                    StepPlan::Go {
                        at: if *leave {
                            Position {
                                x: spot.x + LEAVE_DISTANCE_M,
                                ..spot
                            }
                        } else {
                            spot
                        },
                        territory: None,
                    }
                }
                (None, Some(place)) => self.walk_to_poi(level, std::slice::from_ref(place)),
                (None, None) => broken(
                    "no place: its trigger volume, its location and its POI are all unknown"
                        .to_owned(),
                ),
            },
            TaskKind::KillContribution {
                enemy,
                count,
                place,
                ..
            } if enemy.eq_ignore_ascii_case(PLAYER_ENEMY) => StepPlan::Stop(
                Outcome::NotWalked,
                format!("needs {} kills of other players (PvP)", remaining(*count)),
            ),
            TaskKind::KillContribution {
                enemy,
                count,
                place,
                ..
            } => self.walk_kill(level, enemy, remaining(*count), place.as_deref()),
            TaskKind::HaveItem(Some(item)) => {
                self.walk_get_item(level, item, 1, None, false, false)
            }
            TaskKind::HaveItem(None) => event(FlowEvent::ContainerLooted, 1),
            TaskKind::HaveItems {
                item,
                count,
                drop,
                chest,
                ..
            } => self.walk_get_item(
                level,
                item,
                remaining(*count),
                drop.as_ref(),
                chest.is_some() || step.container_item,
                false,
            ),
            TaskKind::GatherItems {
                item,
                count,
                drop,
                chest,
                ..
            } => self.walk_get_item(
                level,
                item,
                remaining(*count),
                drop.as_ref(),
                chest.is_some() || step.container_item,
                true,
            ),
            TaskKind::RequireGather { item, count } => {
                self.walk_get_item(level, item, remaining(*count), None, false, true)
            }
            TaskKind::CraftRecipe(recipe) => {
                if content.recipes.get(lowercase_crc32(recipe)).is_none() {
                    return broken(format!("no recipe {recipe}"));
                }
                event(FlowEvent::Crafted(recipe.clone()), 1)
            }
            TaskKind::CraftAnyRecipe { recipes, count } => {
                match recipes
                    .iter()
                    .find(|recipe| content.recipes.get(lowercase_crc32(recipe)).is_some())
                {
                    Some(recipe) => event(FlowEvent::Crafted(recipe.clone()), *count),
                    None => broken(format!(
                        "none of its recipes exists: {}",
                        recipes.join(", ")
                    )),
                }
            }
            TaskKind::Never => StepPlan::Pin,
            TaskKind::EquipItem(item) => event(FlowEvent::Equipped(item.clone()), 1),
            TaskKind::ConversationTopic(topic) => {
                let npcs = level.topic_npcs.get(topic).cloned().unwrap_or_default();
                let placed = npcs
                    .iter()
                    .find_map(|npc| level.npc_at(npc).map(|at| (npc.clone(), at)));
                if placed.is_none() && !npcs.is_empty() {
                    return broken(format!(
                        "the NPC of its topic is not in the open world: {}",
                        npcs.join(", ")
                    ));
                }
                StepPlan::Topic {
                    topic: *topic,
                    npc: placed,
                }
            }
            TaskKind::GoToPoi(tags) => self.walk_to_poi(level, tags),
            TaskKind::ExitPoi(tag) => StepPlan::Leave(
                content
                    .poi_tags
                    .territories_with(tag)
                    .into_iter()
                    .filter_map(|territory| u16::try_from(territory).ok())
                    .collect(),
            ),
            TaskKind::GameEvent {
                event: fired,
                count,
            } => {
                let object = (0..content.quest_objects.len())
                    .filter_map(|index| content.quest_objects.get(index))
                    .any(|object| object.event.eq_ignore_ascii_case(fired));
                let gatherable = content.gatherables.iter().any(|gatherable| {
                    gatherable
                        .game_event
                        .as_deref()
                        .is_some_and(|known| known.eq_ignore_ascii_case(fired))
                        && content
                            .gather_points
                            .first(lowercase_crc32(&gatherable.id))
                            .is_some()
                });
                let fired_by_server = content.recipes.fires_event(fired)
                    || fired.eq_ignore_ascii_case(super::travel::BIND_CAMP_EVENT)
                    || fired.eq_ignore_ascii_case(super::attack::PLAYER_KILL_EVENT)
                    || level.success_events.contains(&fired.to_lowercase())
                    || self.encounter_firing(fired).is_some();
                if object || gatherable || fired_by_server {
                    event(FlowEvent::GameEvent(fired.clone()), *count)
                } else {
                    StepPlan::Stop(
                        Outcome::NeedsLook,
                        format!(
                            "no quest object, gatherable, recipe or quest end of the level fires game event {fired}"
                        ),
                    )
                }
            }
            TaskKind::Interact { tag, count } => {
                let board_turn_in = super::missions::TURN_IN_TAGS
                    .iter()
                    .any(|turn_in| turn_in.eq_ignore_ascii_case(tag));
                let nearest = (!board_turn_in)
                    .then(|| content.interact_tags.nearest_with(tag, [at.x, at.y, at.z]))
                    .flatten();
                // An object whose variant is a placed gatherable is gathered for its interact
                // (retail `1124_0101_enpleinaire` 146.1 s: kind 14 of `1124_VistaPoint1`).
                let gathered = (!board_turn_in)
                    .then(|| {
                        content
                            .interact_tags
                            .gatherables_with(tag)
                            .into_iter()
                            .find_map(|crc| {
                                content.gather_points.first(crc)?;
                                content.gatherables.get(crc).map(|known| known.id.clone())
                            })
                    })
                    .flatten();
                if let Some(gatherable) = gathered {
                    StepPlan::Gather {
                        gatherable,
                        count: remaining(*count),
                        again: false,
                        event: Box::new(event(FlowEvent::Interacted(tag.clone()), *count)),
                    }
                } else if let Some((object, place)) = nearest {
                    StepPlan::Use {
                        tag: tag.clone(),
                        count: remaining(*count),
                        object,
                        at: position_of(place),
                    }
                } else if content.interact_tags.knows(tag) || board_turn_in {
                    event(FlowEvent::Interacted(tag.clone()), *count)
                } else {
                    broken(format!("no object of the level has interact tag {tag}"))
                }
            }
            TaskKind::CyclicState {
                cyclic,
                state,
                count,
            } => {
                // A cyclic is known by a gatherable of that id (the turning stones of
                // `06A_MSQ_18`) or by its own `VariantID` that names a gatherable per state:
                // `09A_M14_PlaceStaff` asks for `Cyclic_09A_M14_NatureRitual`, whose three
                // states show `09A_M14_NatureRitual_0` to `_2` (retail
                // `09a_m14_getinfusedseed` gathered `_0` at 473.221 s and `_1` at 480.955 s).
                if content
                    .cyclic_gatherables
                    .of(lowercase_crc32(cyclic))
                    .is_empty()
                    && content.cyclic_gatherables.gatherables_of(cyclic).is_empty()
                {
                    broken(format!("no cyclic gatherable {cyclic}"))
                } else {
                    event(
                        FlowEvent::Cyclic {
                            cyclic: cyclic.clone(),
                            state: *state,
                        },
                        *count,
                    )
                }
            }
            TaskKind::UseItems { item, count } => event(FlowEvent::Used(item.clone()), *count),
            TaskKind::Objective(_) => StepPlan::Wait,
            TaskKind::GoToTerritory { territory, poi } => {
                let Ok(territory) = u16::try_from(*territory) else {
                    return broken(format!("territory {territory} is no territory id"));
                };
                match (poi, level.territory_spots.get(&territory)) {
                    (Some(poi), _) => self.walk_to_poi(level, std::slice::from_ref(poi)),
                    (None, Some(spot)) => StepPlan::Go {
                        at: *spot,
                        territory: Some(territory),
                    },
                    (None, None) => broken(format!(
                        "no place of the level lies in territory {territory}"
                    )),
                }
            }
            TaskKind::ConversationState(npc) => match level.npc_at(npc) {
                Some(at) => StepPlan::Talk {
                    npc: npc.clone(),
                    at: Some(at),
                },
                None => broken(format!("NPC {npc} is not in the open world")),
            },
            TaskKind::HaveLevel {
                progression,
                target,
                points,
            } => match progression {
                None => broken(format!(
                    "waits for character level {target}; the walk character is at the top level"
                )),
                Some(id) if content.progression.categorical(id).is_none() => {
                    broken(format!("no progression {id}"))
                }
                Some(id) => StepPlan::Progression {
                    id: id.clone(),
                    target: *target,
                    points: *points,
                },
            },
            TaskKind::Patrol { place, .. } => {
                match self.walk_to_poi(level, std::slice::from_ref(place)) {
                    // The stay counts while the player is in the place.
                    StepPlan::Go { at: spot, .. }
                        if spot.horizontal_distance(at) <= PATROL_REACH_M =>
                    {
                        StepPlan::Wait
                    }
                    other => other,
                }
            }
            TaskKind::GameMode(mode) | TaskKind::CompleteGameMode(mode) => {
                if content.instances.by_id(mode).is_none() {
                    broken(format!("no game mode {mode}"))
                } else {
                    StepPlan::Stop(
                        Outcome::NotWalked,
                        format!(
                            "needs a run of game mode {mode} (an expedition, a raid or an arena)"
                        ),
                    )
                }
            }
            TaskKind::Fishing {
                step: fishing,
                count,
            } => event(FlowEvent::Fishing(*fishing), *count),
            TaskKind::CatchFish { rule, count } => event(
                FlowEvent::Caught {
                    item: rule.item.clone().unwrap_or_else(|| ANY_FISH.to_owned()),
                    length: rule.min_length.unwrap_or_default() + CATCH_MARGIN,
                    weight: rule.min_weight.unwrap_or_default() + CATCH_MARGIN,
                    hotspot: rule.hotspot,
                },
                *count,
            ),
            TaskKind::Salvage { item, count } => event(FlowEvent::Salvaged(item.clone()), *count),
            TaskKind::CraftFamily {
                family,
                item,
                count,
            } => event(
                FlowEvent::CraftedFamily {
                    family: family.clone(),
                    item: item.clone(),
                },
                *count,
            ),
            TaskKind::HaveFamily { family, .. } => StepPlan::Stop(
                Outcome::NotWalked,
                format!("holds items of family {family}: the walk does not pick such items"),
            ),
            TaskKind::PerformSong { song, near, count } => {
                if content.music.song(lowercase_crc32(song)).is_none() {
                    return broken(format!("no song {song}"));
                }
                event(
                    FlowEvent::Song {
                        song: song.clone(),
                        at: near.map_or(at, |area| area.spot()),
                    },
                    *count,
                )
            }
            TaskKind::QuickCourse(course) => match content.quick_course_paths.get(course) {
                Some(path) => {
                    let sections = u32::try_from(path.nodes.len()).unwrap_or(u32::MAX);
                    StepPlan::Event(
                        FlowEvent::Course {
                            course: course.clone(),
                            section: 0,
                            sections,
                        },
                        sections,
                    )
                }
                None => broken(format!("no path of quick course {course}")),
            },
            TaskKind::QuickCourseSection { course, section } => {
                match content.quick_course_paths.get(course) {
                    Some(path) => StepPlan::Event(
                        FlowEvent::Course {
                            course: course.clone(),
                            section: *section,
                            sections: u32::try_from(path.nodes.len()).unwrap_or(u32::MAX),
                        },
                        1,
                    ),
                    None => broken(format!("no path of quick course {course}")),
                }
            }
            TaskKind::OwnHouses(houses) => StepPlan::Stop(
                Outcome::NotWalked,
                format!("owns {houses} houses: the walk does not buy houses"),
            ),
            TaskKind::Die { count } => event(FlowEvent::Died, *count),
            TaskKind::NotRun(kind) => broken(format!("the server does not run step kind {kind}")),
        }
    }

    /// A point of interest of one of `tags`: a quest volume of the tag, else a place of a
    /// territory that carries it.
    fn walk_to_poi(&self, level: &QuestGuide, tags: &[String]) -> StepPlan {
        let content = &self.content;
        if let Some(volume) = content
            .quest_volumes
            .iter()
            .find(|volume| tags.iter().any(|tag| volume.tag.eq_ignore_ascii_case(tag)))
        {
            let [x, y, z] = nw_world::collision::transform_point(&volume.place, [0.0; 3]);
            return StepPlan::Go {
                at: Position { x, y, z },
                territory: None,
            };
        }
        // A trigger area of the running instance's script of that name: retail
        // `0726_0101_d_01a_AreaTrigger` (tag `AreaEntryGate`) ended when the player walked into
        // the gate area of `DungeonShatterMtn00` (`07-Tempests_Heart` 163.96 s at 1458.8 952.0).
        if let Some(area) = self
            .dungeon_run
            .as_ref()
            .and_then(|run| tags.iter().find_map(|tag| run.script().trigger_area(tag)))
        {
            let [x, y, z] = nw_world::collision::transform_point(&area.place, [0.0; 3]);
            return StepPlan::Go {
                at: Position { x, y, z },
                territory: None,
            };
        }
        tags.iter()
            .find_map(|tag| {
                content
                    .poi_tags
                    .territories_with(tag)
                    .into_iter()
                    .filter_map(|territory| u16::try_from(territory).ok())
                    .find_map(|territory| {
                        level
                            .territory_spots
                            .get(&territory)
                            .map(|spot| StepPlan::Go {
                                at: *spot,
                                territory: Some(territory),
                            })
                    })
            })
            .unwrap_or_else(|| {
                StepPlan::Stop(
                    Outcome::Broken,
                    format!(
                        "no place of the level carries POI tag {}",
                        tags.join(" or ")
                    ),
                )
            })
    }

    /// The vitals id and categories of the spawn that counts as `enemy`, with its spawn
    /// categories, when the kill types know its vitals.
    fn walk_counts(&self, spawn: &VitalsSpawn, enemy: &str) -> bool {
        let content = &self.content;
        content
            .kill_types
            .of(spawn.vitals_id)
            .is_some_and(|(vitals_id, categories)| {
                let spawn_categories = content.kill_types.spawn_categories(Some(spawn.category_id));
                kills_count(enemy, vitals_id, categories, &spawn_categories)
            })
    }

    /// The slice of the open world encounter that spawns the mob of the level's encounter
    /// spawn `spawn` in one of its stages, when this sim runs the open world's encounters and
    /// that encounter starts by itself when a player comes near. The sim leaves such a spawn
    /// out of its own spawns ([`crate::mobs::in_scripted_group`]): the encounter's stage
    /// spawns its mob (`12A_MSQ_22_DestroyShrine_Defeat_Captain`: `Wave_02 - Objective: Kill
    /// Captain Boss` of `enc_worldevent_12mb06_captain_boss_waveevent`).
    fn spawning_encounter(&self, spawn: &VitalsSpawn) -> Option<String> {
        let script = self
            .settings
            .world_events
            .then(|| self.content.dungeon_scripts.game_mode(OPEN_WORLD_LEVEL))
            .flatten()?;
        script
            .encounters
            .iter()
            .filter(|encounter| {
                super::dungeon_script::starts_when_enabled(encounter)
                    && crate::mobs::in_scripted_group(spawn, &[encounter.position])
            })
            .find(|encounter| {
                encounter
                    .stages
                    .iter()
                    .flat_map(|stage| &stage.spawns)
                    .filter_map(|stage_spawn| encounter.targets.get(stage_spawn.target))
                    .flat_map(|target| &target.slices)
                    .flat_map(TargetSlice::mobs)
                    .any(|mob| lowercase_crc32(&mob.vitals_id) == spawn.vitals_id)
            })
            .map(|encounter| encounter.slice.clone())
    }

    /// The encounter whose success fires game event `fired` (its `m_universalSuccessGameEventId`,
    /// [`super::dungeon_script::DungeonRun::success_event_of`]): one of the instance this sim
    /// runs, else one of the open world when this sim runs its encounters.
    pub(super) fn encounter_firing(
        &self,
        fired: &str,
    ) -> Option<&nw_world::dungeon_scripts::Encounter> {
        self.encounter_firing_at(fired)
            .map(|(_, encounter)| encounter)
    }

    /// The encounter whose success fires game event `fired`, with its index in the script
    /// ([`Self::encounter_firing`]). Of the open world's spawn groups only the choice runs (a
    /// group of breaches runs one at a time): an encounter that is no choice spawns nothing when
    /// a player comes (live 2026-11-24 04:22:32Z: `FM_corruption_breach` named the first breach
    /// of the list, the bot saw no mob there in 45 s).
    fn encounter_firing_at(
        &self,
        fired: &str,
    ) -> Option<(usize, &nw_world::dungeon_scripts::Encounter)> {
        let may_run = |index: usize| {
            self.dungeon_run.is_some()
                || self
                    .world_events
                    .as_ref()
                    .is_none_or(|run| run.chosen(index))
        };
        self.encounter_script()?
            .encounters
            .iter()
            .enumerate()
            .find(|(index, encounter)| {
                may_run(*index)
                    && encounter
                        .rewards
                        .as_ref()
                        .is_some_and(|rewards| rewards.global_event.eq_ignore_ascii_case(fired))
            })
    }

    /// The living mob of open world encounter `index` that a fight there takes next: the one
    /// that takes hits and has the most health, with its vitals id (CRC) and place; `None`
    /// while none lives (its waves have not come, or the encounter is an instance's).
    pub(super) fn encounter_mob_now(&self, index: usize) -> Option<(u32, [f32; 3])> {
        let run = self.world_events.as_ref()?;
        run.living_mobs_of(index)
            .into_iter()
            .filter(|(entity, _)| self.mobs.invulnerable(*entity).is_none())
            .filter_map(|(entity, _)| {
                let vitals = self.mobs.vitals_of(entity)?;
                let mob = self.mobs.get(entity)?;
                Some((
                    vitals,
                    [mob.position.x, mob.position.y, mob.position.z],
                    mob.health.current(),
                ))
            })
            .max_by_key(|(_, _, health)| *health)
            .map(|(vitals, at, _)| (vitals, at))
    }

    /// The open world encounter of slice `slice`, when this sim runs the open world's
    /// encounters and no instance: only an open world encounter resets when its players leave
    /// its volume ([`super::dungeon_script::volume_radius_m`]).
    fn open_world_encounter_of_slice(
        &self,
        slice: &str,
    ) -> Option<&nw_world::dungeon_scripts::Encounter> {
        if self.dungeon_run.is_some() {
            return None;
        }
        self.encounter_script()?
            .encounters
            .iter()
            .find(|encounter| encounter.slice == slice)
    }

    /// The places where the encounter of slice `slice` spawns pickups taken by touch and its
    /// area triggers ([`super::dungeon_script::is_area_trigger`]), each once, in stage order:
    /// the fight walks onto them. Live 2026-11-24 07:52-08:07Z: the bot stood at Medusa's spawn,
    /// 31 m from the `Wave 0 - Setup` trigger at 8644.3 1406.0, which never fired, so Medusa
    /// never came.
    fn touch_points(&self, slice: &str) -> Vec<Position> {
        let Some(encounter) = self
            .encounter_script()
            .and_then(|script| script.encounters.iter().find(|known| known.slice == slice))
        else {
            return Vec::new();
        };
        let mut places: Vec<[f32; 3]> = Vec::new();
        for spawn in encounter.stages.iter().flat_map(|stage| &stage.spawns) {
            let touched = encounter.targets.get(spawn.target).is_some_and(|target| {
                target.slices.iter().any(|known| {
                    super::boss_scripts::touch_pickup(&known.path)
                        || super::dungeon_script::is_area_trigger(&known.path)
                })
            });
            if touched {
                for place in &spawn.locations {
                    if !places.contains(place) {
                        places.push(*place);
                    }
                }
            }
        }
        places.into_iter().map(position_of).collect()
    }

    /// The level script whose encounters this sim runs: the instance's, else the open world's
    /// when this sim runs its encounters.
    fn encounter_script(&self) -> Option<&nw_world::dungeon_scripts::LevelScript> {
        match &self.dungeon_run {
            Some(run) => Some(run.script()),
            None => self
                .settings
                .world_events
                .then(|| self.content.dungeon_scripts.game_mode(OPEN_WORLD_LEVEL))
                .flatten(),
        }
    }

    /// `plan`, with a game event step that an encounter's success fires as the fight at that
    /// encounter's place ([`StepPlan::Encounter`]: a kill of its first mob, placed, so no game
    /// master spawns one; the encounter spawns its waves when the player comes into its volume);
    /// any other plan as it is.
    fn encounter_for_event(&self, plan: StepPlan) -> StepPlan {
        let StepPlan::Event(FlowEvent::GameEvent(fired), _) = &plan else {
            return plan;
        };
        let Some((index, encounter)) = self.encounter_firing_at(fired) else {
            return plan;
        };
        let place = position_of(encounter.position);
        if let Some((slice, asset)) = self
            .encounter_objects
            .standing_near(place, START_OBJECT_REACH_M)
            && let Some(gatherable) = self.start_object_gatherable(slice, place)
        {
            return StepPlan::Start {
                gatherable: gatherable.id.clone(),
                asset,
                at: place,
                event: Box::new(plan),
            };
        }
        // While a wave of it lives, the fight takes that wave's mob where it stands (live
        // 2026-11-24 05:24-05:39Z: a major breach's later waves are impalers, heavies and an
        // ogre, and a plan of its first target's acolyte held the bot at the acolytes).
        let (vitals_id, at) = self.encounter_mob_now(index).unwrap_or_else(|| {
            let vitals = encounter
                .targets
                .iter()
                .flat_map(|target| &target.slices)
                .find_map(|slice| slice.mob.as_ref())
                .map_or_else(|| encounter.name.clone(), |mob| mob.vitals_id.clone());
            (lowercase_crc32(&vitals), encounter.position)
        });
        let kill = StepPlan::Kill {
            spawn: VitalsSpawn {
                vitals_id,
                position: at,
                is_encounter: true,
                ..VitalsSpawn::default()
            },
            count: 1,
            territory: None,
            spawned: false,
            encounter: Some(encounter.slice.clone()),
        };
        StepPlan::Encounter {
            kill: Box::new(kill),
            event: Box::new(plan),
        }
    }

    /// The gatherable of the start object of slice `slice` that stands at `place`: the one its
    /// slice is named after (`io_worldevent_09a_s17_protectranger` is `09A_S17_ProtectRanger`;
    /// Inferred from the names, as the slice's gatherable controller is not in the content),
    /// else a quest gatherable whose point is there, not a tradeskill node at the same place
    /// (`1434_0103`: `Wolf_Elemental_Wood` stands at the pyre and asks a rank). Live 2026-11-22
    /// 21:31:38: the first quest gatherable at the wagon was `09A_S17_WagonSupplies`, a later
    /// step's object, and the sim refused that gather (`NotOnQuest`); A's run had named
    /// `09A_S17_ProtectRanger` (the order of a hash table).
    fn start_object_gatherable(
        &self,
        slice: &str,
        place: Position,
    ) -> Option<&nw_content::gathering::Gatherable> {
        let named = slice
            .rsplit('/')
            .next()
            .and_then(|file| file.strip_suffix(SLICE_SUFFIX))
            .and_then(|stem| stem.strip_prefix(START_OBJECT_PREFIX))
            .and_then(|id| self.content.gatherables.get(lowercase_crc32(id)));
        named.or_else(|| {
            self.content
                .gatherables
                .iter()
                .filter(|gatherable| {
                    gatherable.quest_interact()
                        && !self
                            .content
                            .gather_points
                            .within(lowercase_crc32(&gatherable.id), place, START_OBJECT_REACH_M)
                            .is_empty()
                })
                .min_by(|a, b| a.id.cmp(&b.id))
        })
    }

    /// The spawn of a mob that counts as `enemy` in an encounter of the instance this sim runs,
    /// with its encounter's slice: the place where that encounter's stage spawns it, or the
    /// encounter's own place (the `QuestGawain` trial spawns `Lost_Monarch` in its
    /// `EncounterBoss`, not where the open map's spawn of that mob stands). `None` outside an
    /// instance.
    fn instance_encounter_spawn(&self, enemy: &str) -> Option<(VitalsSpawn, String)> {
        self.encounter_spawn_in(self.dungeon_run.as_ref()?.script(), enemy)
    }

    /// The game mode whose instance spawns a mob that counts as `enemy` in an encounter of its
    /// level script ([`Self::encounter_spawn_in`]), not the open world's.
    fn game_mode_spawning(&self, enemy: &str) -> Option<&str> {
        self.content
            .dungeon_scripts
            .game_modes
            .iter()
            .filter(|(mode, _)| *mode != OPEN_WORLD_LEVEL)
            .find(|(_, script)| self.encounter_spawn_in(script, enemy).is_some())
            .map(|(mode, _)| mode.as_str())
    }

    /// The spawn of a mob that counts as `enemy` in an encounter of `script`, with its
    /// encounter's slice ([`Self::instance_encounter_spawn`]).
    fn encounter_spawn_in(
        &self,
        script: &nw_world::dungeon_scripts::LevelScript,
        enemy: &str,
    ) -> Option<(VitalsSpawn, String)> {
        script.encounters.iter().find_map(|encounter| {
            encounter
                .stages
                .iter()
                .flat_map(|stage| &stage.spawns)
                .find_map(|stage_spawn| {
                    let mob = encounter
                        .targets
                        .get(stage_spawn.target)?
                        .slices
                        .iter()
                        .flat_map(TargetSlice::mobs)
                        .find(|mob| {
                            self.content
                                .kill_types
                                .of(lowercase_crc32(&mob.vitals_id))
                                .is_some_and(|(vitals_id, categories)| {
                                    kills_count(enemy, vitals_id, categories, &[])
                                })
                        })?;
                    Some((
                        VitalsSpawn {
                            vitals_id: lowercase_crc32(&mob.vitals_id),
                            category_id: lowercase_crc32(&mob.category_id),
                            level: mob.level,
                            position: stage_spawn
                                .locations
                                .first()
                                .copied()
                                .unwrap_or(encounter.position),
                            is_encounter: true,
                            territory_ids: Vec::new(),
                        },
                        encounter.slice.clone(),
                    ))
                })
        })
    }

    /// `count` kills of mobs that count as `enemy` (in POI `place` when it names one): a mob
    /// of an encounter of the instance this sim runs ([`Self::instance_encounter_spawn`]), else
    /// a spawn of such a mob that the open map runs, else one that an open world encounter it
    /// runs spawns ([`Self::spawning_encounter`]).
    pub(super) fn walk_kill(
        &self,
        level: &QuestGuide,
        enemy: &str,
        count: u32,
        place: Option<&str>,
    ) -> StepPlan {
        let territories: Option<Vec<u32>> =
            place.map(|place| self.content.poi_tags.territories_with(place));
        let in_place = |spawn: &VitalsSpawn| {
            territories.as_ref().is_none_or(|territories| {
                spawn
                    .territory_ids
                    .iter()
                    .any(|territory| territories.contains(&u32::from(*territory)))
            })
        };
        let counts = |spawn: &VitalsSpawn| self.walk_counts(spawn, enemy) && in_place(spawn);
        // The instance's encounter spawns its mob when the player comes: the player fights
        // that mob where it stands.
        if let Some((spawn, encounter)) = self.instance_encounter_spawn(enemy) {
            return StepPlan::Kill {
                spawn,
                count,
                territory: None,
                spawned: false,
                encounter: Some(encounter),
            };
        }
        let found = self
            .combat
            .spawns
            .iter()
            .find(|spawn| counts(spawn))
            .map(|spawn| (spawn.clone(), None))
            .or_else(|| {
                level
                    .all_spawns
                    .iter()
                    .filter(|spawn| counts(spawn))
                    .find_map(|spawn| {
                        self.spawning_encounter(spawn)
                            .map(|encounter| (spawn.clone(), Some(encounter)))
                    })
            });
        match found {
            Some((spawn, encounter)) => {
                let territory = territories.as_ref().and_then(|territories| {
                    spawn
                        .territory_ids
                        .iter()
                        .find(|territory| territories.contains(&u32::from(**territory)))
                        .copied()
                });
                // A mob that only an encounter spawns is fought where the encounter spawns it:
                // a game master's mob of its vitals is no real spawn.
                let spawned = encounter.is_none()
                    && self.content.kill_types.of(spawn.vitals_id).is_some_and(
                        |(vitals_id, categories)| kills_count(enemy, vitals_id, categories, &[]),
                    );
                StepPlan::Kill {
                    spawn,
                    count,
                    territory,
                    spawned,
                    encounter,
                }
            }
            None => {
                let where_ = place
                    .map(|place| format!(" in POI {place}"))
                    .unwrap_or_default();
                StepPlan::Stop(
                    Outcome::Broken,
                    if level.all_spawns.iter().any(counts) {
                        format!(
                            "mobs that count as {enemy}{where_} spawn only in encounters the open map does not run"
                        )
                    } else {
                        format!("no mob that counts as {enemy} spawns in the open world{where_}")
                    },
                )
            }
        }
    }

    /// `count` of item `item` from a source the level has; `drop` is the step's own drop,
    /// `chest` whether containers drop it; `gathered`: the step counts gathers.
    fn walk_get_item(
        &self,
        level: &QuestGuide,
        item: &str,
        count: u32,
        drop: Option<&QuestDrop>,
        chest: bool,
        gathered: bool,
    ) -> StepPlan {
        match self.walk_item_source(level, item, drop, chest) {
            Some(ItemSource { said, node }) => StepPlan::Item {
                item: item.to_owned(),
                count,
                gathered,
                source: said,
                node,
            },
            None => StepPlan::Stop(
                Outcome::Broken,
                format!(
                    "no source of {item}: no drop, gatherable, mob loot, recipe, shop or loot table of the level gives it"
                ),
            ),
        }
    }

    /// In how many of [`SOURCE_ROLLS`] rolls of loot table `table` (no mob, no levels, a fixed
    /// random sequence) item `item` drops: a table can hold an item that its rolls almost never
    /// give (live 2026-11-22 07:05:31: three `DyePlantViolet` gathers for `WoodT1` gave none,
    /// and a `Goat` gave 13 `RawhideT1` at once where a `Bison` gave meat only).
    pub(super) fn roll_hits(&self, table: &str, item: &str) -> u32 {
        let mut seed = SOURCE_ROLL_SEED;
        let mut below = |bound: u32| {
            seed = seed
                .wrapping_mul(SOURCE_ROLL_MULTIPLIER)
                .wrapping_add(SOURCE_ROLL_INCREMENT);
            u32::try_from((seed >> SOURCE_ROLL_SHIFT) % u64::from(bound.max(1))).unwrap_or(0)
        };
        let context = nw_content::loot::LootContext {
            enemy: None,
            content: None,
            player: None,
            poi: None,
            tags: &[],
            reached_limits: &[],
        };
        let mut hits = 0;
        for _ in 0..SOURCE_ROLLS {
            let roll = self.content.loot.roll(table, context, &mut below);
            if roll
                .items
                .iter()
                .any(|(dropped, _)| dropped.eq_ignore_ascii_case(item))
            {
                hits += 1;
            }
        }
        hits
    }

    /// Where a player gets item `item` on this level; `drop` is the step's own drop of an enemy
    /// or a quest object, `chest` whether the containers of its place drop it.
    fn walk_item_source(
        &self,
        level: &QuestGuide,
        item: &str,
        drop: Option<&QuestDrop>,
        chest: bool,
    ) -> Option<ItemSource> {
        let content = &self.content;
        let sources = drop.map_or(&[][..], |drop| drop.sources.as_slice());
        // A mob of the instance's encounters drops it too (live 2026-11-22 14:02:10: the
        // Starstone boss `AncientGuardian_Heavy_Boss_Named_DG_Everfall_00` of `EncounterBoss`
        // died, and its quest drop `0411_0414_StartstoneDiadem` had no source in the plan),
        // and a mob that only an open world encounter spawns (`09A_S3_HercyneCerebrum` drops
        // from `FL_Hercyne_Reindeer_WaveEvent`, which `Enc_09A_S3_WoundedHercyne` spawns in
        // `Wave_02`; retail `09a_s3_hunthercyne` killed it at 152.1 s).
        if let Some(enemy) = sources.iter().find(|enemy| {
            self.combat
                .spawns
                .iter()
                .any(|spawn| self.walk_counts(spawn, enemy))
                || self.instance_encounter_spawn(enemy).is_some()
                || level.all_spawns.iter().any(|spawn| {
                    self.walk_counts(spawn, enemy) && self.spawning_encounter(spawn).is_some()
                })
        }) {
            return Some(ItemSource::said(format!("the drop of {enemy}")));
        }
        if let Some(object) = sources.iter().find(|object| {
            content
                .gather_points
                .first(lowercase_crc32(object))
                .is_some()
        }) {
            return Some(ItemSource::gathered(
                format!("the drop of quest object {object}"),
                object,
            ));
        }
        // A drop that names an interact tag drops at the gather of an object of that tag
        // (retail `04a_side04_06_getflowers`: five gathers of `04A_Side04_01_Flowers`, tag
        // `IO_04A_Side04_01_Flowers`, raised its item step by one each).
        if let Some(gatherable) = sources.iter().find_map(|tag| {
            content
                .interact_tags
                .gatherables_with(tag)
                .into_iter()
                .find_map(|crc| {
                    content.gather_points.first(crc)?;
                    content.gatherables.get(crc).map(|known| known.id.clone())
                })
        }) {
            return Some(ItemSource::gathered(
                format!("the gather of quest object {gatherable}"),
                &gatherable,
            ));
        }
        if chest {
            return Some(ItemSource::said("a container drop".to_owned()));
        }
        if let Some(gatherable) = content
            .gatherables
            .iter()
            .filter(|gatherable| {
                gatherable
                    .loot_table
                    .as_deref()
                    .is_some_and(|table| content.loot.table_drops(table, item))
                    && content
                        .gather_points
                        .first(lowercase_crc32(&gatherable.id))
                        .is_some()
            })
            .max_by(|a, b| {
                // A gatherable whose rolls give the item and that asks no rank or item first
                // (live 2026-11-22 08:03:01: `AppleTreeSmall` refused the bot's gathers for its
                // logging rank), then the one whose rolls give the item most often.
                let rank = |gatherable: &nw_content::gathering::Gatherable| {
                    let free = content.gather_rules.of(lowercase_crc32(&gatherable.id))
                        == nw_content::gathering::GatherRule::default();
                    let hits = gatherable
                        .loot_table
                        .as_deref()
                        .map_or(0, |table| self.roll_hits(table, item));
                    (free && hits > 0, hits)
                };
                rank(a).cmp(&rank(b)).then_with(|| b.id.cmp(&a.id))
            })
        {
            return Some(ItemSource::gathered(
                format!("gathered from {}", gatherable.id),
                &gatherable.id,
            ));
        }
        if let Some(vitals_id) = self.combat.spawns.iter().find_map(|spawn| {
            let drop = content.mob_loot.of(spawn.vitals_id)?;
            content
                .loot
                .table_drops(&drop.table, item)
                .then(|| content.kill_types.of(spawn.vitals_id).map(|(id, _)| id))
                .flatten()
        }) {
            return Some(ItemSource::said(format!("the loot of {vitals_id}")));
        }
        if let Some(recipe) = content.recipes.making(item) {
            return Some(ItemSource::said(format!("crafted with {}", recipe.id)));
        }
        if content.shops.sells(item) {
            return Some(ItemSource::said("bought at a shop".to_owned()));
        }
        content
            .loot
            .drops_item(item)
            .then(|| ItemSource::said("a loot table holds it".to_owned()))
    }

    /// Carries out `plan` for waiting step `step` of walk character `number` in this world at
    /// `now_ms`.
    fn walk_act(
        &mut self,
        level: &QuestGuide,
        number: u64,
        connection: &ConnectionRef,
        plan: StepPlan,
        now_ms: u64,
    ) -> Play {
        match plan {
            StepPlan::Wait | StepPlan::AwaitStart { .. } => Play::Waits,
            StepPlan::Strike(targets) => {
                for (gatherable, _) in targets {
                    self.puzzle_struck(number, &gatherable, now_ms);
                }
                Play::Done(None)
            }
            StepPlan::Pin => Play::Done(None),
            StepPlan::Stop(Outcome::NeedsLook, reason) => Play::NeedsLook(reason),
            StepPlan::Stop(Outcome::NotWalked, reason) => Play::NotWalked(reason),
            StepPlan::Stop(_, reason) => Play::Broken(reason),
            StepPlan::Client(step) => {
                for call in step.calls(header(number)) {
                    let _ = self.facet_message(connection, call, now_ms);
                }
                Play::Done(None)
            }
            StepPlan::Talk { npc, .. } => {
                self.walk_talk(level, number, connection, &npc, now_ms);
                Play::Done(None)
            }
            StepPlan::Topic { topic, npc } => {
                if let Some((npc, _)) = &npc {
                    self.walk_talk(level, number, connection, npc, now_ms);
                }
                let finished = ConversationClientMsg::FinishedTopics {
                    header: header(number),
                    topics: vec![topic],
                };
                self.conversation_call(connection, finished, now_ms);
                Play::Done(npc.is_none().then(|| "no NPC holds its topic".to_owned()))
            }
            StepPlan::Go { at, territory } => {
                self.walk_go(number, at, now_ms);
                if let Some(territory) = territory {
                    self.walk_territory(number, connection, territory, true, now_ms);
                }
                Play::Done(None)
            }
            StepPlan::Leave(territories) => {
                for territory in territories {
                    self.walk_territory(number, connection, territory, false, now_ms);
                }
                Play::Done(None)
            }
            StepPlan::Kill {
                spawn,
                count,
                territory,
                encounter,
                ..
            } => {
                let content = std::sync::Arc::clone(&self.content);
                self.walk_go(number, position_of(spawn.position), now_ms);
                if let Some(territory) = territory {
                    self.walk_territory(number, connection, territory, true, now_ms);
                    // The territory list feeds the places at the next tick.
                    let _ = self.tick(now_ms, now_ms);
                }
                let Some((vitals_id, categories)) = content.kill_types.of(spawn.vitals_id) else {
                    return Play::Waits;
                };
                let spawn_categories = content.kill_types.spawn_categories(Some(spawn.category_id));
                for _ in 0..count {
                    self.quest_event(
                        number,
                        Event::Killed {
                            vitals_id,
                            categories,
                            spawn_categories: &spawn_categories,
                        },
                        now_ms,
                    );
                }
                Play::Done(Some(match encounter {
                    Some(encounter) => {
                        format!(
                            "killed {vitals_id} at its spawn in encounter {encounter}, by event"
                        )
                    }
                    None => format!("killed {vitals_id} at its spawn, by event"),
                }))
            }
            StepPlan::Item {
                item,
                count,
                gathered,
                source,
                ..
            } => {
                self.give_loot(number, vec![(item.clone(), count)], now_ms);
                if gathered {
                    self.quest_event(
                        number,
                        Event::Gathered {
                            item: &item,
                            quantity: count,
                        },
                        now_ms,
                    );
                }
                Play::Done(Some(format!("got {item} ({source})")))
            }
            StepPlan::Achievements(given) => {
                for (name, _) in &given {
                    self.earn_achievement(number, name, now_ms);
                }
                Play::Done(Some(format!(
                    "achievements given: {}",
                    given
                        .iter()
                        .map(|(name, source)| format!("{name} (from {source})"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )))
            }
            StepPlan::Progression { id, target, points } => {
                if points {
                    self.give_mastery_experience(number, &id, target);
                } else {
                    self.set_mastery_rank(
                        number,
                        Some(&id),
                        u16::try_from(target).unwrap_or(u16::MAX),
                    );
                }
                Play::Done(Some(format!("progression {id} set to {target}")))
            }
            StepPlan::Event(flow, count) => {
                let note = self.walk_flow(number, &flow, count, now_ms);
                Play::Done(Some(format!("{note}, by event")))
            }
            StepPlan::Use {
                tag, count, object, ..
            } => {
                let note = self.walk_flow(number, &FlowEvent::Interacted(tag), count, now_ms);
                Play::Done(Some(format!("{note}, by event (object {object:016x})")))
            }
            StepPlan::Equip { item, slot, .. } => {
                let note = self.walk_flow(number, &FlowEvent::Equipped(item), 1, now_ms);
                Play::Done(Some(format!("{note}, by event (slot {slot})")))
            }
            StepPlan::Encounter { event, .. } | StepPlan::Start { event, .. } => {
                self.walk_act(level, number, connection, *event, now_ms)
            }
            StepPlan::Skin { item, .. } | StepPlan::Loot { item, .. } => {
                self.walk_act(level, number, connection, *item, now_ms)
            }
            StepPlan::Gather { event, .. }
            | StepPlan::Camp { event, .. }
            | StepPlan::Craft { event, .. }
            | StepPlan::Fish { event, .. }
            | StepPlan::Pvp { event, .. }
            | StepPlan::Song { event, .. }
            | StepPlan::UseItem { event, .. } => {
                self.walk_act(level, number, connection, *event, now_ms)
            }
            StepPlan::Lore { achievements, .. } => {
                self.walk_act(level, number, connection, *achievements, now_ms)
            }
            StepPlan::Trial { stop, .. } => self.walk_act(level, number, connection, *stop, now_ms),
        }
    }

    /// Plays `flow` `count` times for walk character `number` as the game event the server
    /// makes of it, and names what it played.
    fn walk_flow(&mut self, number: u64, flow: &FlowEvent, count: u32, now_ms: u64) -> String {
        if let FlowEvent::Equipped(item) | FlowEvent::Used(item) = flow {
            self.give_loot(number, vec![(item.clone(), count)], now_ms);
        }
        if let FlowEvent::Song { at, .. } = flow {
            self.walk_go(number, *at, now_ms);
        }
        let families = match flow {
            FlowEvent::CraftedFamily { family, .. } => vec![family.clone()],
            _ => Vec::new(),
        };
        for index in 0..count {
            let event = match flow {
                FlowEvent::ContainerLooted => Event::ContainerLooted { items: &[] },
                FlowEvent::Crafted(recipe) => Event::Crafted(recipe),
                FlowEvent::CraftedFamily { .. } => Event::CraftedFamilies(&families),
                FlowEvent::Equipped(item) => Event::Equipped(item),
                FlowEvent::GameEvent(fired) => Event::GameEvent(fired),
                FlowEvent::Interacted(tag) => Event::Interacted(tag),
                FlowEvent::Cyclic { cyclic, state } => Event::CyclicGathered {
                    cyclic,
                    state: *state,
                },
                FlowEvent::Used(item) => Event::ItemUsed(item),
                FlowEvent::Fishing(step) => Event::Fishing(*step),
                FlowEvent::Caught {
                    item,
                    length,
                    weight,
                    hotspot,
                } => Event::FishCaught(FishCaught {
                    item,
                    length: *length,
                    weight: *weight,
                    hotspot: *hotspot,
                }),
                FlowEvent::Salvaged(item) => Event::Salvaged { item, quantity: 1 },
                FlowEvent::Song { song, at } => Event::SongPerformed { song, at: *at },
                FlowEvent::Course {
                    course,
                    section,
                    sections,
                } => Event::QuickCourseSection {
                    course,
                    section: section + index,
                    sections: *sections,
                },
                FlowEvent::Died => Event::Died,
            };
            self.quest_event(number, event, now_ms);
        }
        format!("{flow:?}")
    }

    /// The `!questplan` answer of player `number` for its running quest `objective`: one line
    /// per waiting step, with what the player does next ([`Self::plan_line`]); `not running`
    /// when the player does not run it.
    pub(crate) fn quest_plan_lines(&mut self, number: u64, objective: &str) -> Vec<String> {
        if let Some(mode) = super::run_plan::run_mode(objective) {
            return self.run_plan_lines(number, mode);
        }
        if let Some(skill) = super::trade_plan::trade_skill(objective) {
            return self.trade_plan_lines(number, skill);
        }
        let entity = self.player_by_number(number);
        let at = entity
            .and_then(|entity| self.world.get::<super::Body>(entity))
            .map(|body| body.motion.position());
        let content = std::sync::Arc::clone(&self.content);
        let steps: Vec<WaitingStep> = self
            .walk_waiting(number)
            .into_iter()
            .filter(|step| step.quest.eq_ignore_ascii_case(objective))
            .collect();
        match at {
            Some(at) if self.walk_runs(number, objective) => steps
                .iter()
                .map(|step| {
                    let plan = self.walk_plan(&content.quest_guide, step, at);
                    let plan = self.equip_from_bag(entity, plan);
                    let plan = self.use_from_bag(entity, plan);
                    let plan = self.skin_from_drop(&content.quest_guide, step, plan);
                    let plan = self.loot_from_drop(&content.quest_guide, step, plan);
                    let plan = self.gather_for_event(plan);
                    let plan = self.gather_for_cyclic(entity, plan);
                    let plan = self.encounter_for_event(plan);
                    let plan = self.camp_for_event(entity, at, plan);
                    let plan = self.craft_for_event(entity, at, plan);
                    let plan = self.fish_for_catch(&content.quest_guide, entity, step, at, plan);
                    let plan = pvp_for_event(plan);
                    let plan = self.song_for_event(entity, plan);
                    let plan = self.lore_for_achievement(plan);
                    let plan = self.trial_for_game_mode(step, plan);
                    let plan = self.go_outside_instance(plan);
                    self.plan_line(&step.task, &plan)
                })
                .collect(),
            _ => vec![format!("{PLAN_NOT_RUNNING} {objective}")],
        }
    }

    /// `plan`, with a use step whose item is in the bag of player `entity` as the use of its bag
    /// slot ([`StepPlan::UseItem`], the client's 2917); any other plan as it is.
    fn use_from_bag(&self, entity: Option<bevy_ecs::entity::Entity>, plan: StepPlan) -> StepPlan {
        let StepPlan::Event(FlowEvent::Used(item), _) = &plan else {
            return plan;
        };
        let bag_slot = entity
            .and_then(|entity| self.world.get::<super::Player>(entity))
            .and_then(|player| player.gear.as_ref()?.bag_slot_of(item));
        match bag_slot {
            Some(bag_slot) => StepPlan::UseItem {
                item: item.clone(),
                bag_slot,
                event: Box::new(plan),
            },
            None => plan,
        }
    }

    /// `plan`, with an equip step whose item is in the bag of player `entity` as the equip call
    /// that puts it on ([`StepPlan::Equip`]); any other plan as it is. The walk plays the equip
    /// as its event; a bot sends the call.
    fn equip_from_bag(&self, entity: Option<bevy_ecs::entity::Entity>, plan: StepPlan) -> StepPlan {
        let StepPlan::Event(FlowEvent::Equipped(item), count) = plan else {
            return plan;
        };
        let classes = self
            .content
            .item_rolls
            .item(&item)
            .map_or(&[][..], |row| row.classes.as_slice());
        let place = entity
            .and_then(|entity| self.world.get::<super::Player>(entity))
            .and_then(|player| player.gear.as_ref()?.equip_place(&item, classes));
        match place {
            Some((bag_slot, instance, slot)) => StepPlan::Equip {
                item,
                bag_slot,
                instance,
                slot,
            },
            None => StepPlan::Event(FlowEvent::Equipped(item), count),
        }
    }

    /// `plan`, with a gather step whose item is the quest drop of an enemy that has a skinning
    /// gatherable of its name as that skin after a kill of one ([`StepPlan::Skin`]); any other
    /// plan as it is.
    fn skin_from_drop(&self, level: &QuestGuide, step: &WaitingStep, plan: StepPlan) -> StepPlan {
        let (
            StepPlan::Item {
                count,
                gathered: true,
                node: None,
                ..
            },
            TaskKind::GatherItems {
                drop: Some(drop), ..
            },
        ) = (&plan, &step.kind)
        else {
            return plan;
        };
        let count = *count;
        let skinned = drop.sources.iter().find_map(|enemy| {
            let gatherable = self.skinning_gatherable(enemy)?;
            let kill = self.walk_kill(level, enemy, 1, drop.place.as_deref());
            matches!(kill, StepPlan::Kill { .. }).then_some((gatherable, kill))
        });
        match skinned {
            Some((gatherable, kill)) => StepPlan::Skin {
                gatherable,
                count,
                kill: Box::new(kill),
                item: Box::new(plan),
            },
            None => plan,
        }
    }

    /// The skinning gatherable of a corpse of enemy `enemy`: the one of that name, else the one
    /// the creature slice of a level spawn that counts as it names, else the first one a kill
    /// category of such a spawn names (live 2026-11-22 09:00:
    /// `16_Side_20_HelpBastet` drops its heart from `Barbary_Lion_16BS12`, whose categories
    /// hold `Primal_Lion`, a skinning gatherable; no gatherable has the vitals id's name).
    pub(super) fn skinning_gatherable(&self, enemy: &str) -> Option<String> {
        let skinning = |name: &str| {
            self.content
                .gatherables
                .get(lowercase_crc32(name))
                .filter(|gatherable| {
                    gatherable.tradeskill.as_deref() == Some(super::gathering::SKINNING_TRADESKILL)
                })
                .map(|gatherable| gatherable.id.clone())
        };
        let counting = || {
            self.combat
                .spawns
                .iter()
                .filter(|spawn| self.walk_counts(spawn, enemy))
        };
        skinning(enemy)
            .or_else(|| {
                // The gatherable its creature slice names: retail `1127_0102_makinbacon`
                // skinned `PigDomestic` (112.0 to 127.2 s) from the `Pig_Domestic_49` that
                // `slices/characters/pig_domestic.dynamicslice` is; no category names it.
                counting().find_map(|spawn| {
                    let look = self.content.mob_looks.look(spawn.vitals_id, None)?;
                    let creature = self.content.creature_animations.get(&look.asset.id)?;
                    skinning(creature.skin_gatherable()?)
                })
            })
            .or_else(|| {
                counting().find_map(|spawn| {
                    let (_, categories) = self.content.kill_types.of(spawn.vitals_id)?;
                    categories.iter().find_map(|category| skinning(category))
                })
            })
    }

    /// `plan`, with an item step whose item is the quest drop of an enemy that the level spawns
    /// as the loot of a kill of one ([`StepPlan::Loot`]); any other plan as it is.
    fn loot_from_drop(&self, level: &QuestGuide, step: &WaitingStep, plan: StepPlan) -> StepPlan {
        let (
            StepPlan::Item {
                gathered: false,
                node: None,
                ..
            },
            TaskKind::HaveItems {
                drop: Some(drop), ..
            },
        ) = (&plan, &step.kind)
        else {
            return plan;
        };
        let kill = drop.sources.iter().find_map(|enemy| {
            let kill = self.walk_kill(level, enemy, 1, drop.place.as_deref());
            matches!(kill, StepPlan::Kill { .. }).then_some(kill)
        });
        match kill {
            Some(kill) => StepPlan::Loot {
                kill: Box::new(kill),
                item: Box::new(plan),
            },
            None => plan,
        }
    }

    /// `plan`, with a game mode step, or a step of any type played in a game mode's instance
    /// (its row's `GameModeID`, [`WaitingStep::game_mode`]: a kill, a game event, a trigger
    /// area), of a mode that is never a matched kind as its entry ([`StepPlan::Trial`]), outside
    /// an instance of that mode: from afar for a `remote_entry` mode, else at an entrance door
    /// of the level that opens it ([`Self::trial_door`]); any other plan as it is. Retail
    /// `99B_MSQ_10_d_03_01_02_02_02_Kill_1_Umberto` (game mode `QuestCaerDun`) has no game mode
    /// step: the player entered from afar (capture `99b_msq_10_cleansefort`: one 5037, then the
    /// 946). An item step whose item no source of the level gives, and that drops from a mob
    /// that only an instance's encounter spawns, enters that instance too (live 2026-11-22
    /// 14:4x: `0411_0414_d_03_task_Diadem` drops from the Starstone boss of
    /// `DungeonShatteredObelisk`'s `EncounterBoss`; outside it, its plan was "no source").
    fn trial_for_game_mode(&self, step: &WaitingStep, plan: StepPlan) -> StepPlan {
        let mode = match (&step.kind, &step.game_mode) {
            (TaskKind::GameMode(mode), _) | (_, Some(mode)) => mode.as_str(),
            (
                TaskKind::HaveItems {
                    drop: Some(drop), ..
                },
                None,
            ) if self.instance_run.is_none() && matches!(plan, StepPlan::Stop(..)) => {
                match drop
                    .sources
                    .iter()
                    .find_map(|enemy| self.game_mode_spawning(enemy))
                {
                    Some(mode) => mode,
                    None => return plan,
                }
            }
            _ => return plan,
        };
        self.trial_into(mode, plan)
    }

    /// `plan`, outside an instance of game mode `mode` (never a matched kind), as the entry of
    /// that mode ([`StepPlan::Trial`], [`Self::trial_for_game_mode`]); inside one, or for a
    /// matched or unknown mode, `plan` as it is.
    pub(super) fn trial_into(&self, mode: &str, plan: StepPlan) -> StepPlan {
        let inside = self
            .instance_run
            .as_ref()
            .is_some_and(|run| run.game_mode().eq_ignore_ascii_case(mode));
        let Some(found) = self.content.instances.by_id(mode) else {
            return plan;
        };
        if found.kind.is_matched() || inside {
            return plan;
        }
        let door = if found.remote_entry {
            None
        } else {
            match self.trial_door(&found.id) {
                Some(door) => Some(door),
                None => return plan,
            }
        };
        StepPlan::Trial {
            mode: found.id.clone(),
            crc: found.crc,
            door,
            stop: Box::new(plan),
        }
    }

    /// `plan`, with a go step outside the frame (`WorldBounds`) of the running instance's map
    /// as a stop: the place lies outside the instance, so a player leaves it first; any other
    /// plan as it is. Live 2026-11-22 14:07:51 UTC: after the Corrupted_Leviathan of the
    /// `QuestNihilo` trial died, `99D_MSQ_07_PortalFight_triggerArea_exitCorruptionTrigger` asked
    /// for 8711 7733 of the open world, which the trial's map (777 534, 640 by 640) does not
    /// hold.
    fn go_outside_instance(&self, plan: StepPlan) -> StepPlan {
        let StepPlan::Go { at, .. } = &plan else {
            return plan;
        };
        let Some(run) = self.instance_run.as_ref() else {
            return plan;
        };
        let map = self
            .content
            .instances
            .by_id(run.game_mode())
            .and_then(|mode| mode.map_of(self.settings.level.folder));
        match map {
            Some(map) if !map.frames(at.x, at.y) => StepPlan::Stop(
                Outcome::NotWalked,
                format!(
                    "{:.1} {:.1} lies outside the instance of {}",
                    at.x,
                    at.y,
                    run.game_mode()
                ),
            ),
            _ => plan,
        }
    }

    /// The entrance capital of the level that opens game mode `mode` ([`EntranceDoor::mode`]),
    /// with its place when the level's entrance entities hold it: an unlocked door before a
    /// locked one, then the lowest capital id.
    ///
    /// [`EntranceDoor::mode`]: nw_content::dungeon_entrances::EntranceDoor::mode
    fn trial_door(&self, mode: &str) -> Option<(u64, Option<Position>)> {
        let content = &self.content;
        let capital = content
            .entrances
            .iter()
            .filter(|(_, door)| {
                door.mode(content.instances.modes())
                    .is_some_and(|opened| opened.id.eq_ignore_ascii_case(mode))
            })
            .min_by_key(|(capital, door)| {
                (
                    door.slice.to_lowercase().contains(LOCKED_ENTRANCE),
                    **capital,
                )
            })
            .map(|(capital, _)| *capital)?;
        let place = content
            .entrance_entities
            .iter()
            .find(|entity| entity.capital == capital)
            .map(|entity| entity.position);
        Some((capital, place))
    }

    /// `plan`, with a step that waits for a lore note's achievement as the read of that note at
    /// its lore point ([`StepPlan::Lore`]); any other plan as it is.
    fn lore_for_achievement(&self, plan: StepPlan) -> StepPlan {
        let StepPlan::Achievements(given) = &plan else {
            return plan;
        };
        let note = given.iter().find_map(|(name, _)| {
            let note = self.content.lore_notes.unlocking(name)?;
            Some((note.title.clone(), self.content.lore_points.first(note.id)?))
        });
        match note {
            Some((title, at)) => StepPlan::Lore {
                title,
                at,
                achievements: Box::new(plan),
            },
            None => plan,
        }
    }

    /// `plan`, with the camp step (`BindCamp`) as the camp player `entity` places at `at` and
    /// builds ([`StepPlan::Camp`]), or as the reason the server would refuse that camp; any
    /// other plan as it is.
    fn camp_for_event(
        &self,
        entity: Option<bevy_ecs::entity::Entity>,
        at: Position,
        plan: StepPlan,
    ) -> StepPlan {
        let StepPlan::Event(FlowEvent::GameEvent(fired), _) = &plan else {
            return plan;
        };
        if !fired.eq_ignore_ascii_case(super::travel::BIND_CAMP_EVENT) {
            return plan;
        }
        let Some(entity) = entity else {
            return plan;
        };
        match self.camp_for_plan(entity, at) {
            Ok(camp) => StepPlan::Camp {
                at,
                camp,
                event: Box::new(plan),
            },
            Err(refusal) => StepPlan::Stop(Outcome::NeedsLook, format!("a camp here: {refusal}")),
        }
    }

    /// `plan`, with a craft step of one recipe as that craft at the station nearest to `at` that
    /// makes it, from the bag of player `entity` ([`StepPlan::Craft`]); any other plan as it is.
    fn craft_for_event(
        &self,
        entity: Option<bevy_ecs::entity::Entity>,
        at: Position,
        plan: StepPlan,
    ) -> StepPlan {
        let recipes = &self.content.recipes;
        let recipe = match &plan {
            StepPlan::Event(FlowEvent::Crafted(recipe_id), _) => {
                recipes.get(lowercase_crc32(recipe_id))
            }
            StepPlan::Event(FlowEvent::CraftedFamily { family, item }, _) => {
                self.family_recipe(at, family, item.as_deref())
            }
            _ => return plan,
        };
        let Some(recipe) = recipe else {
            return plan;
        };
        let Some(station) = self
            .stations
            .nearest(at, |station_type| recipe.made_at(station_type))
        else {
            return plan;
        };
        let gear = entity
            .and_then(|entity| self.world.get::<super::Player>(entity))
            .and_then(|player| player.gear.as_ref());
        let mut ingredients = Vec::new();
        let mut missing = Vec::new();
        for ingredient in &recipe.ingredients {
            let Some((item, held)) = self.ingredient_item(gear, ingredient) else {
                continue;
            };
            if held < ingredient.quantity {
                missing.push((item.clone(), ingredient.quantity - held));
            }
            ingredients.push((item, ingredient.quantity));
        }
        StepPlan::Craft {
            recipe: recipe.id.clone(),
            station: station.gde_ref,
            at: station.position,
            ingredients,
            missing,
            event: Box::new(plan),
        }
    }

    /// The recipe a craft of a family step makes near `at`: one whose item is of family
    /// `family` and that a station of the level makes; the one that makes the step's own item
    /// `item` first (retail `06a_ts_02_forgesword` 79.0 s: `Procedural_LongswordT2` for
    /// `SwordAbilityTable`, item `1hSwordT2`), else the first by id.
    fn family_recipe(
        &self,
        at: Position,
        family: &str,
        item: Option<&str>,
    ) -> Option<&nw_content::crafting::Recipe> {
        let made_here = |recipe: &&nw_content::crafting::Recipe| {
            super::item_of_family(&self.content, &recipe.item, family)
                && self
                    .stations
                    .nearest(at, |station_type| recipe.made_at(station_type))
                    .is_some()
        };
        let mut candidates: Vec<&nw_content::crafting::Recipe> =
            self.content.recipes.iter().filter(made_here).collect();
        candidates.sort_by_key(|recipe| {
            (
                item.is_none_or(|item| !recipe.item.eq_ignore_ascii_case(item)),
                recipe.id.to_lowercase(),
            )
        });
        candidates.first().copied()
    }

    /// `plan`, with a catch step as fishing from a place of the level whose cast lands in water
    /// ([`StepPlan::Fish`], [`SimWorld::fishing_place`]) near `at`, else near the quest's turn-in
    /// NPC, else near the spot of `level` in the fish's territory, after an equip of the bag's
    /// pole when player `entity` wears none ([`StepPlan::Equip`]); any other plan as it is.
    fn fish_for_catch(
        &self,
        level: &QuestGuide,
        entity: Option<bevy_ecs::entity::Entity>,
        step: &WaitingStep,
        at: Position,
        plan: StepPlan,
    ) -> StepPlan {
        let StepPlan::Event(FlowEvent::Caught { .. }, _) = &plan else {
            return plan;
        };
        let TaskKind::CatchFish { rule, .. } = &step.kind else {
            return plan;
        };
        let Some(entity) = entity else {
            return plan;
        };
        if !self.wears_pole(entity) {
            return match self.bag_pole_equip(entity) {
                Some(equip) => equip,
                None => StepPlan::Stop(
                    Outcome::NeedsLook,
                    "the bag holds no fishing pole to wear".to_owned(),
                ),
            };
        }
        // Near the player first, then near the quest's turn-in NPC and in the fish's territory:
        // a player fishes where the quest sends it (retail `16_side_11_feedifrit` fished 60 m
        // from where it took the quest).
        let turn_in = self
            .content
            .quests
            .quest(&step.quest)
            .and_then(|quest| quest.turn_in)
            .and_then(|npc| level.npc_at(&npc));
        let territory_spot = rule
            .territory()
            .and_then(|territory| u16::try_from(territory).ok())
            .and_then(|territory| level.territory_spots.get(&territory).copied());
        let place = [Some(at), turn_in, territory_spot]
            .into_iter()
            .flatten()
            .find_map(|from| self.fishing_place(entity, from, rule.water()));
        match place {
            Some((place, yaw)) => StepPlan::Fish {
                at: place,
                yaw,
                territory: rule.territory(),
                event: Box::new(plan),
            },
            None => StepPlan::Stop(
                Outcome::NeedsLook,
                format!(
                    "no place within {:.0} m whose cast lands in water",
                    super::fishing::FISHING_PLACE_SEARCH_M
                ),
            ),
        }
    }

    /// `plan`, with a song step as the performance of a sheet of the song whose instrument
    /// player `entity` wears ([`StepPlan::Song`]); before it, the equip of a bag instrument of
    /// the song's first sheet ([`StepPlan::Equip`]), or the item step of the first such
    /// instrument when the bag holds none (a game master gives it to a bot); any other plan as it
    /// is.
    pub(super) fn song_for_event(
        &self,
        entity: Option<bevy_ecs::entity::Entity>,
        plan: StepPlan,
    ) -> StepPlan {
        let StepPlan::Event(FlowEvent::Song { song, at }, _) = &plan else {
            return plan;
        };
        let Some(entity) = entity else {
            return plan;
        };
        let music = &self.content.music;
        let Some(row) = music.song(lowercase_crc32(&song.to_lowercase())) else {
            return plan;
        };
        let sheets: Vec<&nw_content::music::Sheet> = row
            .sheets
            .iter()
            .filter_map(|sheet| music.sheet(lowercase_crc32(&sheet.to_lowercase())))
            .collect();
        let worn = self.worn_instrument_slots(entity);
        if let Some(sheet) = sheets.iter().find(|sheet| {
            music
                .instrument_slot(&sheet.instrument)
                .is_some_and(|slot| worn.contains(&slot))
        }) {
            return StepPlan::Song {
                song: row.id.clone(),
                sheet: sheet.id.clone(),
                duration_s: row.duration_s,
                at: *at,
                event: Box::new(plan),
            };
        }
        let Some(class) = sheets
            .first()
            .and_then(|sheet| nw_content::music::instrument_class_of(&sheet.instrument))
        else {
            return StepPlan::Stop(
                Outcome::Broken,
                format!("song {song} has no sheet of a known instrument"),
            );
        };
        let rolls = &self.content.item_rolls;
        let bag_instrument = self
            .world
            .get::<Player>(entity)
            .and_then(|player| player.gear.as_ref())
            .and_then(|gear| {
                gear.carried().into_iter().find_map(|stack| {
                    let classes = rolls.item(&stack.item_id)?.classes.as_slice();
                    classes
                        .iter()
                        .any(|known| known.eq_ignore_ascii_case(class))
                        .then(|| gear.equip_place(&stack.item_id, classes))
                        .flatten()
                        .map(|(bag_slot, instance, slot)| StepPlan::Equip {
                            item: stack.item_id.clone(),
                            bag_slot,
                            instance,
                            slot,
                        })
                })
            });
        if let Some(equip) = bag_instrument {
            return equip;
        }
        match rolls.first_of_classes(&[INSTRUMENT_TOOL_CLASS, class]) {
            Some(instrument) => StepPlan::Item {
                item: instrument.item_id.clone(),
                count: 1,
                gathered: false,
                source: format!("an instrument of class {class} for song {song}"),
                node: None,
            },
            None => StepPlan::Stop(
                Outcome::Broken,
                format!("no item of instrument class {class} for song {song}"),
            ),
        }
    }

    /// The equip of the first fishing pole in the bag of player `entity` ([`StepPlan::Equip`]
    /// into its tool slot); `None` when the bag holds none.
    pub(super) fn bag_pole_equip(&self, entity: bevy_ecs::entity::Entity) -> Option<StepPlan> {
        let gear = self.world.get::<Player>(entity)?.gear.as_ref()?;
        gear.carried().into_iter().find_map(|stack| {
            self.content.fishing.pole(&stack.item_id)?;
            let classes = self
                .content
                .item_rolls
                .item(&stack.item_id)
                .map_or(&[][..], |row| row.classes.as_slice());
            let (bag_slot, instance, slot) = gear.equip_place(&stack.item_id, classes)?;
            Some(StepPlan::Equip {
                item: stack.item_id.clone(),
                bag_slot,
                instance,
                slot,
            })
        })
    }

    /// The item a craft takes for `ingredient` and how many of it the bag `gear` holds: the
    /// carried item that fits it with the most in the bag, else the ingredient's own item or the
    /// lowest item of its category ([`nw_content::crafting::Recipes::item_for`]), by its id in
    /// the item table's case; `None` for a category that no item is in.
    pub(super) fn ingredient_item(
        &self,
        gear: Option<&super::gear::Gear>,
        ingredient: &nw_content::crafting::Ingredient,
    ) -> Option<(String, u32)> {
        let recipes = &self.content.recipes;
        let carried = gear.and_then(|gear| {
            gear.carried()
                .into_iter()
                .filter(|item| recipes.fits(ingredient, &item.item_id))
                .map(|item| (gear.bag_count(&item.item_id), item.item_id))
                .max()
        });
        if let Some((held, item)) = carried {
            return Some((item, held));
        }
        let item = recipes.item_for(ingredient)?;
        let item = self
            .content
            .kits
            .item(&item)
            .map_or(item, |known| known.item_id.clone());
        Some((item, 0))
    }

    /// `plan`, with a game event step that the gather of a placed gatherable fires as gathers of
    /// it ([`StepPlan::Gather`]), or as the gather of the start object an open world encounter
    /// spawns there, which the player awaits in the encounter's volume while it is not up
    /// ([`Self::wait_for_start_object`]). Any other plan as it is.
    fn gather_for_event(&self, plan: StepPlan) -> StepPlan {
        let StepPlan::Event(FlowEvent::GameEvent(fired), count) = &plan else {
            return plan;
        };
        let gatherable = self.content.gatherables.iter().find(|gatherable| {
            gatherable
                .game_event
                .as_deref()
                .is_some_and(|known| known.eq_ignore_ascii_case(fired))
                && self
                    .content
                    .gather_points
                    .first(lowercase_crc32(&gatherable.id))
                    .is_some()
        });
        let Some(gatherable) = gatherable else {
            return plan;
        };
        // An encounter's start object that stands on the gatherable's point is that gather
        // (retail `09a_s16_destroyabomination` 205.5 s: the gather of `09A_S16_ThrowSwitch`
        // counted one gatherer, a shared start object), not the level node under it.
        let place = self
            .content
            .gather_points
            .first(lowercase_crc32(&gatherable.id));
        let start = place.and_then(|place| {
            let asset = self
                .encounter_objects
                .standing_asset_near(place, START_OBJECT_REACH_M)?;
            Some((place, asset))
        });
        if start.is_none()
            && let Some(wait) =
                place.and_then(|place| self.wait_for_start_object(&gatherable.id, place))
        {
            return wait;
        }
        match start {
            Some((at, asset)) => StepPlan::Start {
                gatherable: gatherable.id.clone(),
                asset,
                at,
                event: Box::new(plan),
            },
            None => StepPlan::Gather {
                gatherable: gatherable.id.clone(),
                count: *count,
                again: false,
                event: Box::new(plan),
            },
        }
    }

    /// While no start object stands at gather point `place` of a game event step's gatherable
    /// `gatherable`, the wait at the centre of the open world encounter this sim runs that
    /// spawns its start object there ([`StepPlan::AwaitStart`]): the player's coming starts it,
    /// and its stage spawns the object. The wait goes before any sibling step (live
    /// 2026-11-22 21:31:52-21:32:16: with a plain wait the bot fought `1602_0401`'s sibling
    /// step, broke the level banner, and the encounter's banner was never gathered). Retail `09a_s3_hunthercyne` 126.3 s gathered the fresh kill
    /// that `Enc_09A_S3_WoundedHercyne`'s `Wave_01` spawns (a dynamic entity, not the level
    /// node), and its `Wave_02` reindeer died at 152.1 s; live 2026-11-22 20:38:40 the bot
    /// gathered the level node `09A_S3_FreshKill` 4 s after it came into the volume, before the
    /// 3 s setup stage and the object, and no wave ran.
    fn wait_for_start_object(&self, gatherable: &str, place: Position) -> Option<StepPlan> {
        if self.dungeon_run.is_some() {
            return None;
        }
        let encounter =
            self.encounter_script()?
                .encounters
                .iter()
                .find(|encounter| {
                    super::dungeon_script::starts_when_enabled(encounter)
                        && encounter
                            .stages
                            .iter()
                            .flat_map(|stage| &stage.spawns)
                            .any(|spawn| {
                                let spawns_object =
                                    encounter.targets.get(spawn.target).is_some_and(|target| {
                                        target.slices.iter().any(|slice| {
                                            self.encounter_objects.is_kind(&slice.path)
                                        })
                                    });
                                let spawned_at = if spawn.locations.is_empty() {
                                    std::slice::from_ref(&encounter.position)
                                } else {
                                    spawn.locations.as_slice()
                                };
                                spawns_object
                                    && spawned_at.iter().any(|location| {
                                        position_of(*location).horizontal_distance(place)
                                            <= START_OBJECT_REACH_M
                                    })
                            })
                })?;
        Some(StepPlan::AwaitStart {
            gatherable: gatherable.to_owned(),
            at: position_of(encounter.position),
        })
    }

    /// `plan`, with a cyclic state step as gathers of a placed gatherable that shows the cyclic
    /// ([`StepPlan::Gather`]): one that shows the wanted state, gathered once; else a turning
    /// one (it shows every state), gathered until the turns of player `entity` reach the state
    /// ([`SimWorld::cyclic_gathers`]: a gather counts the state the stone showed); any other
    /// plan as it is.
    fn gather_for_cyclic(
        &self,
        entity: Option<bevy_ecs::entity::Entity>,
        plan: StepPlan,
    ) -> StepPlan {
        let StepPlan::Event(FlowEvent::Cyclic { cyclic, state }, _) = &plan else {
            return plan;
        };
        let content = &self.content;
        let shown = |gatherable: &nw_content::gathering::Gatherable| {
            content
                .cyclic_gatherables
                .of(lowercase_crc32(&gatherable.id))
                .iter()
                .find(|shown| shown.cyclic.eq_ignore_ascii_case(cyclic))
                .filter(|shown| shown.state.is_none_or(|known| known == *state))
                .cloned()
        };
        let Some((gatherable, shown)) = content.gatherables.iter().find_map(|gatherable| {
            content
                .gather_points
                .first(lowercase_crc32(&gatherable.id))
                .and(shown(gatherable).map(|shown| (gatherable.id.clone(), shown)))
        }) else {
            return plan;
        };
        let (count, again) = match shown.state {
            Some(_) => (1, false),
            None => {
                let turns = entity
                    .and_then(|entity| self.world.get::<Player>(entity))
                    .and_then(|player| player.cyclic_turns.get(&cyclic.to_lowercase()).copied())
                    .unwrap_or_default();
                let states = shown.states.max(1);
                let shows = turns % states;
                ((state % states + states - shows) % states + 1, true)
            }
        };
        StepPlan::Gather {
            gatherable,
            count,
            again,
            event: Box::new(plan),
        }
    }

    /// The `!questplan` line of `plan` for task `task`: what a player does next, with the
    /// places it goes to.
    pub(crate) fn plan_line(&self, task: &str, plan: &StepPlan) -> String {
        format!("{PLAN_LINE} {task} {}", self.plan_what(plan))
    }

    /// What `plan` asks a player to do, in the words of a `!questplan` line after its task.
    fn plan_what(&self, plan: &StepPlan) -> String {
        let at = |at: &Position| format!("{:.1} {:.1} {:.1}", at.x, at.y, at.z);
        let territory = |territory: &Option<u16>| {
            territory
                .map(|territory| format!(" territory {territory}"))
                .unwrap_or_default()
        };
        match plan {
            StepPlan::Wait | StepPlan::Pin => PLAN_WAIT.to_owned(),
            StepPlan::AwaitStart {
                gatherable,
                at: place,
            } => {
                format!("{PLAN_AWAIT} {gatherable} at {}", at(place))
            }
            StepPlan::Stop(outcome, reason) => format!("{PLAN_STOP} {outcome:?}: {reason}"),
            StepPlan::Client(step) => format!("{PLAN_CLIENT} {}", step.words()),
            StepPlan::Talk { npc, at: place } => format!(
                "{PLAN_TALK} {npc}{}",
                place
                    .map(|place| format!(" at {}", at(&place)))
                    .unwrap_or_default()
            ),
            StepPlan::Topic { topic, npc } => format!(
                "{PLAN_TOPIC} {topic:08x}{}",
                npc.as_ref()
                    .map(|(npc, place)| format!(" after {PLAN_TALK} {npc} at {}", at(place)))
                    .unwrap_or_default()
            ),
            StepPlan::Go {
                at: place,
                territory: entered,
            } => format!("{PLAN_GO} {}{}", at(place), territory(entered)),
            StepPlan::Leave(territories) => format!(
                "{PLAN_LEAVE} {}",
                territories
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            StepPlan::Kill {
                spawn,
                count,
                territory: entered,
                spawned,
                encounter,
            } => {
                let vitals = self.content.kill_types.of(spawn.vitals_id).map_or_else(
                    || format!("{:08x}", spawn.vitals_id),
                    |(id, _)| id.to_owned(),
                );
                let within = encounter
                    .as_deref()
                    .and_then(|slice| self.open_world_encounter_of_slice(slice))
                    .map(|encounter| {
                        format!(
                            " {PLAN_WITHIN} {:.1}",
                            super::dungeon_script::volume_radius_m(encounter)
                        )
                    })
                    .unwrap_or_default();
                let touches: String = encounter
                    .as_deref()
                    .map(|slice| self.touch_points(slice))
                    .unwrap_or_default()
                    .iter()
                    .map(|place| format!(" {PLAN_TOUCH} {}", at(place)))
                    .collect();
                format!(
                    "{PLAN_KILL} {vitals} {count} at {}{}{}{within}{touches}",
                    at(&position_of(spawn.position)),
                    territory(entered),
                    if *spawned {
                        String::new()
                    } else {
                        format!(" {PLAN_PLACED}")
                    }
                )
            }
            StepPlan::Item {
                item,
                count,
                gathered,
                source,
                node,
            } => format!(
                "{PLAN_ITEM} {item} {count}{}{} from {source}",
                if *gathered { " gathered" } else { "" },
                node.as_ref()
                    .map(|node| format!(" {PLAN_NODE} {node}"))
                    .unwrap_or_default()
            ),
            StepPlan::Achievements(given) => format!(
                "{PLAN_ACHIEVEMENTS} {}",
                given
                    .iter()
                    .map(|(name, source)| format!("{name} (from {source})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            StepPlan::Progression { id, target, points } => format!(
                "{PLAN_PROGRESSION} {id} {target}{}",
                if *points { " points" } else { "" }
            ),
            StepPlan::Event(flow, count) => format!("{PLAN_EVENT} {count} {flow:?}"),
            StepPlan::Use {
                tag,
                count,
                object,
                at: place,
            } => format!("{PLAN_USE} {tag} {object:016x} {count} at {}", at(place)),
            StepPlan::Equip {
                item,
                bag_slot,
                instance,
                slot,
            } => format!(
                "{PLAN_EQUIP} {item} bag {bag_slot} instance {} slot {slot}",
                hex(instance)
            ),
            StepPlan::Skin {
                gatherable,
                count,
                kill,
                ..
            } => format!("{PLAN_SKIN} {gatherable} {count} {}", self.plan_what(kill)),
            StepPlan::Strike(targets) => format!(
                "{PLAN_STRIKE} {}",
                targets
                    .iter()
                    .map(|(gatherable, place)| format!("{gatherable} at {}", at(place)))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            StepPlan::Gather {
                gatherable,
                count,
                again,
                ..
            } => format!(
                "{PLAN_GATHER} {gatherable} {count}{}",
                if *again {
                    format!(" {PLAN_AGAIN}")
                } else {
                    String::new()
                }
            ),
            StepPlan::Lore {
                title, at: place, ..
            } => format!("{PLAN_LORE} {title} at {}", at(place)),
            StepPlan::Trial {
                mode, crc, door, ..
            } => format!(
                "{PLAN_TRIAL} {mode} {crc:08x}{}",
                (*door).map_or_else(String::new, |(capital, place)| format!(
                    " {PLAN_TRIAL_DOOR} {capital:016x}{}",
                    place.map_or_else(String::new, |place| format!(" at {}", at(&place)))
                ))
            ),
            StepPlan::Loot { kill, .. } => format!("{PLAN_LOOT} {}", self.plan_what(kill)),
            StepPlan::Encounter { kill, .. } => self.plan_what(kill),
            StepPlan::Start {
                gatherable,
                asset,
                at: place,
                ..
            } => format!("{PLAN_START} {gatherable} {} at {}", hex(asset), at(place)),
            StepPlan::Craft {
                recipe,
                station,
                at: place,
                ingredients,
                missing,
                ..
            } => {
                let words = |word: &str, items: &[(String, u32)]| {
                    items
                        .iter()
                        .map(|(item, quantity)| format!(" {word} {item} {quantity}"))
                        .collect::<String>()
                };
                format!(
                    "{PLAN_CRAFT} {recipe} at {} station {}{}{}",
                    at(place),
                    hex(station),
                    words(PLAN_CRAFT_WITH, ingredients),
                    words(PLAN_CRAFT_GIVE, missing)
                )
            }
            StepPlan::Pvp { count, .. } => format!("{PLAN_PVP} {count}"),
            StepPlan::UseItem { item, bag_slot, .. } => {
                format!("{PLAN_USE_ITEM} {item} {PLAN_USE_ITEM_BAG} {bag_slot}")
            }
            StepPlan::Song {
                song,
                sheet,
                duration_s,
                at: place,
                ..
            } => format!(
                "{PLAN_SONG} {song} {PLAN_SONG_SHEET} {sheet} {PLAN_SONG_SECONDS} {duration_s:.3} at {}",
                at(place)
            ),
            StepPlan::Fish {
                at: place,
                yaw,
                territory: zone,
                ..
            } => format!(
                "{PLAN_FISH} at {} {PLAN_FISH_YAW} {yaw:.3}{}",
                at(place),
                zone.map(|zone| format!(" territory {zone}"))
                    .unwrap_or_default()
            ),
            StepPlan::Camp {
                at: place, camp, ..
            } => format!(
                "{PLAN_CAMP} at {} skin {:08x} asset {}{}",
                at(place),
                camp.skin,
                camp.asset
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
                camp.costs
                    .iter()
                    .map(|(item, quantity)| format!(" cost {item} {quantity}"))
                    .collect::<String>()
            ),
        }
    }

    /// Walk character `number`'s client tells it entered (or left) territory `territory`.
    fn walk_territory(
        &mut self,
        number: u64,
        connection: &ConnectionRef,
        territory: u16,
        entered: bool,
        now_ms: u64,
    ) {
        let area = [0; nw_messages::wire::RAW_ID_LEN];
        let message = if entered {
            ClientFacetMsg::TerritoryEntered {
                header: header(number),
                area,
                area_entity: 0,
                extra: 0,
                territory,
                unknown: 0,
            }
        } else {
            ClientFacetMsg::TerritoryExited {
                header: header(number),
                area,
                area_entity: 0,
                extra: 0,
                territory,
            }
        };
        let _ = self.facet_message(connection, message, now_ms);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nw_content::poi_tags::PoiTags;
    use nw_sim_core::test_support::NoGround;

    use super::super::tests::{CELL_M, MANY, MAX_PLAYERS, NOW, settings, slot, table, test_spawn};
    use super::*;
    use crate::combat::test_support::{TEST_LEVEL, TEST_MOB};
    use crate::combat::tests::test_combat;
    use crate::login::WorldIds;
    use crate::objectives::{KillTypes, TaskBook, TaskRow};
    use crate::placed_points::PlacedPoints;
    use crate::player_content::{LoreNote, LoreNotes, ObjectiveRow, PlayerContent, QuestBook};

    const NPC: &str = "Walk_Npc";
    const CAMP: &str = "walk_camp";
    const CAMP_TERRITORY: u16 = 4_242;
    const GOOD: &str = "walk_good";
    const NO_MOB: &str = "walk_dragon";
    const NO_NPC: &str = "walk_gone";
    const NOT_RUN: &str = "walk_emote";
    const BOARD: &str = "walk_board";
    const PVP: &str = "walk_pvp";
    const ORB: &str = "walk_orb";
    const ORB_OBJECT: &str = "Walk_Orb";
    const DEN: &str = "walk_den";
    /// A quest whose partial container holds a pin task and a kill, and one of a pin alone.
    const PIN_BESIDE: &str = "walk_pin_beside";
    const PIN_ALONE: &str = "walk_pin_alone";
    const DEN_GROUP: &str = "Walk_DenBoars";
    /// A quest whose gather step's item is the boar's quest drop (retail `12_boarfeast` pork):
    /// a player skins a boar for it, the `Boar` skinning gatherable.
    const SKIN: &str = "walk_skin";
    /// A skin quest whose drop names the mob's vitals id, not a gatherable.
    const SKIN_MOB: &str = "walk_skin_mob";
    const BOAR_GATHERABLE: &str = "Boar";
    /// A quest whose game event step fires at the gather of a placed gatherable (retail
    /// `12A_MSQ_07_FreeMiners`: the rescue is a gather by hand of `MB_MSQ2_RescueMiner_Rescue01`).
    const RESCUE: &str = "walk_rescue";
    const RESCUE_EVENT: &str = "Walk_Rescue_interact";
    const RESCUE_GATHERABLE: &str = "Walk_Miner";
    /// A quest whose step waits for a built camp (retail `12A_MSQ_12_CampTutorial`).
    const CAMP_QUEST: &str = "walk_camp";
    /// A quest whose step waits for a lore note's achievement (retail
    /// `12A_MSQ_17_LochnirsNotes`: `Lore_unlock_12_MonarchsBluffs_Title_20_01`).
    const LORE_QUEST: &str = "walk_lore";
    /// A quest whose step waits for the entry into a solo trial (retail
    /// `12A_MSQ_24_DefeatGawain_Enter_GawainTrial`: `QuestGawain`, entered from afar).
    const TRIAL_QUEST: &str = "walk_trial";
    const LOOT_QUEST: &str = "walk_loot";
    /// A lore note whose row names its achievement, which is no `Lore_unlock_` and its title
    /// (retail: the plaque `@99A_MSQ_SoulwardenIntro_Title_01_01` of
    /// `99A_MSQ_01_LearnSoulwardens` unlocks `Lore_unlock_99A_MSQ_SoulwardenIntro_01_01`).
    const LORE_ID: &str = "Walk_Note";
    const LORE_TITLE: &str = "@Walk_Note_Title";
    const LORE_ACHIEVEMENT: &str = "Lore_unlock_Walk_Note";

    fn gateway() -> BusId {
        BusId::try_from("gw-walk".to_owned()).expect("id")
    }

    fn at(x: f32) -> Position {
        Position {
            x,
            y: 10.0,
            z: 50.0,
        }
    }

    fn task(id: &str, kind: &str, children: &[&str]) -> TaskRow {
        TaskRow {
            task_id: id.to_owned(),
            kind: kind.to_owned(),
            sub_tasks: children.iter().map(|child| (*child).to_owned()).collect(),
            ..TaskRow::default()
        }
    }

    /// A world with a quest that a player can end (talk, two boar kills, a camp, a turn-in),
    /// one whose mob spawns nowhere, one whose NPC is not on the level, one with a step kind
    /// the server does not run, one whose item drops from a quest object and one whose boars
    /// count only by their spawn group.
    fn walk_world() -> (SimWorld, QuestGuide) {
        let objective = |id: &str, root: &str, turn_in: Option<&str>| ObjectiveRow {
            id: id.to_owned(),
            root: root.to_owned(),
            turn_in: turn_in.map(str::to_owned),
            ..ObjectiveRow::default()
        };
        let quests = QuestBook::build(
            [
                objective(GOOD, "g_root", Some(NPC)),
                objective(NO_MOB, "d_root", None),
                objective(NO_NPC, "n_root", None),
                objective(NOT_RUN, "e_root", None),
                objective(BOARD, "b_root", None),
                objective(PVP, "p_root", None),
                objective(ORB, "o_root", None),
                objective(DEN, "w_root", None),
                objective(PIN_BESIDE, "q_root", None),
                objective(PIN_ALONE, "a_root", None),
                objective(SKIN, "s_root", None),
                objective(SKIN_MOB, "m_root", None),
                objective(RESCUE, "r_root", None),
                objective(CAMP_QUEST, "c_root", None),
                objective(LORE_QUEST, "l_root", None),
                objective(TRIAL_QUEST, "t_root", None),
                objective(LOOT_QUEST, "k_root", None),
            ],
            TaskBook::new(vec![
                task(
                    "g_root",
                    "ConsecutiveTaskContainer",
                    &["g_talk", "g_kill", "g_camp"],
                ),
                TaskRow {
                    item_name: Some(NPC.to_owned()),
                    ..task("g_talk", "TaskConversationState", &[])
                },
                TaskRow {
                    kill_enemy_type: Some("Boar".to_owned()),
                    target_qty: Some(2),
                    ..task("g_kill", "TaskKillContribution", &[])
                },
                TaskRow {
                    poi_tag: Some(CAMP.to_owned()),
                    ..task("g_camp", "TaskGoToPOITag", &[])
                },
                task("d_root", "ConsecutiveTaskContainer", &["d_kill"]),
                TaskRow {
                    kill_enemy_type: Some("Dragon".to_owned()),
                    target_qty: Some(1),
                    ..task("d_kill", "TaskKillContribution", &[])
                },
                task("n_root", "ConsecutiveTaskContainer", &["n_talk"]),
                TaskRow {
                    item_name: Some("Gone_Npc".to_owned()),
                    ..task("n_talk", "TaskConversationState", &[])
                },
                task("e_root", "ConsecutiveTaskContainer", &["e_emote"]),
                task("e_emote", "TaskPerformEmote", &[]),
                task("b_root", "ConsecutiveTaskContainer", &["b_return"]),
                TaskRow {
                    interact_tag: Some("ReturnToGiver".to_owned()),
                    ..task("b_return", "TaskInteract", &[])
                },
                task("p_root", "ConsecutiveTaskContainer", &["p_kill"]),
                TaskRow {
                    kill_enemy_type: Some("Player".to_owned()),
                    target_qty: Some(3),
                    ..task("p_kill", "TaskKillContribution", &[])
                },
                task("o_root", "ConsecutiveTaskContainer", &["o_get"]),
                TaskRow {
                    item_name: Some("Walk_OrbItem".to_owned()),
                    item_drop_vc: Some(ORB_OBJECT.to_owned()),
                    ..task("o_get", "TaskGatherAndReturnItems", &[])
                },
                // Retail `12A_MSQ_24_DefeatGawain_Enter_Partial`: a pin beside the step that
                // ends the container.
                task("q_root", "PartialTaskContainer", &["q_pin", "q_kill"]),
                task("q_pin", "TaskGameEvent", &[]),
                TaskRow {
                    kill_enemy_type: Some("Boar".to_owned()),
                    target_qty: Some(1),
                    ..task("q_kill", "TaskKillContribution", &[])
                },
                task("a_root", "ConsecutiveTaskContainer", &["a_pin"]),
                task("a_pin", "TaskGameEvent", &[]),
                task("l_root", "ConsecutiveTaskContainer", &["l_read"]),
                TaskRow {
                    item_name: Some(LORE_ACHIEVEMENT.to_owned()),
                    ..task("l_read", "TaskAchievement", &[])
                },
                task("t_root", "ConsecutiveTaskContainer", &["t_enter"]),
                TaskRow {
                    game_mode_id: Some(super::super::instances::tests::trial().id),
                    ..task("t_enter", "TaskGameMode", &[])
                },
                task("c_root", "ConsecutiveTaskContainer", &["c_bind"]),
                TaskRow {
                    kill_enemy_type: Some(super::super::travel::BIND_CAMP_EVENT.to_owned()),
                    ..task("c_bind", "TaskGameEvent", &[])
                },
                task("r_root", "ConsecutiveTaskContainer", &["r_free"]),
                TaskRow {
                    kill_enemy_type: Some(RESCUE_EVENT.to_owned()),
                    ..task("r_free", "TaskGameEvent", &[])
                },
                task("k_root", "ConsecutiveTaskContainer", &["k_get"]),
                TaskRow {
                    item_name: Some("Walk_Gun".to_owned()),
                    item_drop_vc: Some("Boar".to_owned()),
                    ..task("k_get", "TaskHaveAndReturnItems", &[])
                },
                task("s_root", "ConsecutiveTaskContainer", &["s_get"]),
                TaskRow {
                    item_name: Some("Walk_Pork".to_owned()),
                    item_drop_vc: Some(BOAR_GATHERABLE.to_owned()),
                    ..task("s_get", "TaskGatherItems", &[])
                },
                task("m_root", "ConsecutiveTaskContainer", &["m_get"]),
                TaskRow {
                    item_name: Some("Walk_Pork".to_owned()),
                    item_drop_vc: Some(TEST_MOB.to_owned()),
                    ..task("m_get", "TaskGatherItems", &[])
                },
                task("w_root", "ConsecutiveTaskContainer", &["w_kill"]),
                TaskRow {
                    kill_enemy_type: Some(DEN_GROUP.to_owned()),
                    target_qty: Some(3),
                    ..task("w_kill", "TaskKillContribution", &[])
                },
            ]),
        );
        let mut combat = test_combat();
        combat.spawns = vec![
            test_spawn(CELL_M / 2.0 + 30.0),
            VitalsSpawn {
                category_id: lowercase_crc32(DEN_GROUP),
                ..test_spawn(CELL_M / 2.0 + 40.0)
            },
        ]
        .into();
        let level = QuestGuide {
            npcs: HashMap::from([(lowercase_crc32(NPC), at(CELL_M / 2.0 + 5.0))]),
            territory_spots: HashMap::from([(CAMP_TERRITORY, at(CELL_M / 2.0 + 60.0))]),
            givers: HashMap::from([(GOOD.to_owned(), vec![NPC.to_owned()])]),
            topic_npcs: HashMap::new(),
            achievement_sources: HashMap::new(),
            success_events: std::collections::HashSet::new(),
            all_spawns: Vec::new(),
            start: at(CELL_M / 2.0),
        };
        let world = SimWorld::new(
            settings(MANY, MAX_PLAYERS),
            WorldIds::random(0).expect("ids"),
            Arc::new(NoGround),
            slot("sim-a"),
            combat,
            Arc::new(PlayerContent {
                quests,
                kill_types: KillTypes::build([(TEST_MOB.to_owned(), "Boar,Hostile".to_owned())]),
                poi_tags: PoiTags::build([(u32::from(CAMP_TERRITORY), CAMP.to_owned())]),
                gather_points: PlacedPoints::of([
                    (lowercase_crc32(ORB_OBJECT), at(CELL_M / 2.0 + 20.0)),
                    (lowercase_crc32(RESCUE_GATHERABLE), at(CELL_M / 2.0 + 25.0)),
                ]),
                quest_guide: level.clone(),
                instances: nw_content::instances::InstanceCatalog::build([
                    super::super::instances::tests::trial(),
                ]),
                lore_notes: LoreNotes::build([LoreNote::of(
                    LORE_ID,
                    LORE_TITLE,
                    Some(LORE_ACHIEVEMENT),
                )]),
                lore_points: PlacedPoints::of([(
                    lowercase_crc32(LORE_ID),
                    at(CELL_M / 2.0 + 35.0),
                )]),
                gatherables: nw_content::gathering::Gatherables::build([
                    nw_content::gathering::Gatherable {
                        id: BOAR_GATHERABLE.to_owned(),
                        gather_ms: 0,
                        respawn_ms: (0, 0),
                        loot_table: None,
                        loot_container: false,
                        tradeskill: Some(super::super::gathering::SKINNING_TRADESKILL.to_owned()),
                        tool_required: false,
                        by_hand: false,
                        game_event: None,
                        animation: None,
                    },
                    nw_content::gathering::Gatherable {
                        id: RESCUE_GATHERABLE.to_owned(),
                        gather_ms: 0,
                        respawn_ms: (0, 0),
                        loot_table: None,
                        loot_container: false,
                        tradeskill: None,
                        tool_required: false,
                        by_hand: true,
                        game_event: Some(RESCUE_EVENT.to_owned()),
                        animation: None,
                    },
                ]),
                ..PlayerContent::default()
            }),
        );
        (world, level)
    }

    #[test]
    fn the_walk_ends_a_quest_a_player_can_end_and_names_the_step_of_each_other_one() {
        let (mut world, level) = walk_world();
        let mut now = NOW;
        let good = world.walk_quest(&level, &gateway(), 1, GOOD, &mut now);
        assert_eq!(good.outcome, Outcome::Completed, "{good:?}");
        assert!(
            good.notes
                .iter()
                .any(|note| note.contains("killed Test_Boar")),
            "the kills went in at the boar's spawn: {good:?}"
        );
        let no_mob = world.walk_quest(&level, &gateway(), 2, NO_MOB, &mut now);
        assert_eq!(no_mob.outcome, Outcome::Broken);
        assert!(
            no_mob.stop.as_deref().is_some_and(
                |stop| stop.contains("d_kill") && stop.contains("no mob that counts as Dragon")
            ),
            "{no_mob:?}"
        );
        let no_npc = world.walk_quest(&level, &gateway(), 3, NO_NPC, &mut now);
        assert_eq!(no_npc.outcome, Outcome::Broken);
        assert!(
            no_npc
                .stop
                .as_deref()
                .is_some_and(|stop| stop.contains("NPC Gone_Npc is not in the open world")),
            "{no_npc:?}"
        );
        assert!(
            no_npc
                .notes
                .iter()
                .any(|note| note.contains("no NPC offers it")),
            "{no_npc:?}"
        );
        let not_run = world.walk_quest(&level, &gateway(), 4, NOT_RUN, &mut now);
        assert_eq!(not_run.outcome, Outcome::Broken);
        assert!(
            not_run
                .stop
                .as_deref()
                .is_some_and(|stop| stop.contains("does not run step kind TaskPerformEmote")),
            "{not_run:?}"
        );
        let board = world.walk_quest(&level, &gateway(), 6, BOARD, &mut now);
        assert_eq!(
            board.outcome,
            Outcome::Completed,
            "the board turn-in: {board:?}"
        );
        let pvp = world.walk_quest(&level, &gateway(), 7, PVP, &mut now);
        assert_eq!(pvp.outcome, Outcome::NotWalked, "{pvp:?}");
        let beside = world.walk_quest(&level, &gateway(), 8, PIN_BESIDE, &mut now);
        assert_eq!(
            beside.outcome,
            Outcome::Completed,
            "a pin says nothing while its sibling ends the container: {beside:?}"
        );
        let alone = world.walk_quest(&level, &gateway(), 9, PIN_ALONE, &mut now);
        assert_eq!(alone.outcome, Outcome::Broken);
        assert!(
            alone
                .stop
                .as_deref()
                .is_some_and(|stop| stop.contains("no step progresses") && stop.contains("a_pin")),
            "{alone:?}"
        );
        let unknown = world.walk_quest(&level, &gateway(), 5, "no_such_quest", &mut now);
        assert_eq!(unknown.outcome, Outcome::NotStarted);
        assert_eq!(world.players(), 0, "each walk character left");
    }

    #[test]
    fn the_plan_of_each_step_names_what_a_player_does_and_where() {
        let (mut world, _) = walk_world();
        world
            .admit(
                &super::super::tests::admit_at(1, CELL_M / 2.0, true),
                NOW,
                NOW,
            )
            .expect("admit");
        assert_eq!(world.quest_plan_lines(1, GOOD), ["not running walk_good"]);
        world.start_quest(1, GOOD, NOW).expect("start");
        assert_eq!(
            world.quest_plan_lines(1, GOOD),
            ["plan g_talk talk Walk_Npc at 1029.0 10.0 50.0"]
        );
        world.quest_event(1, Event::Talked(NPC), NOW);
        assert_eq!(
            world.quest_plan_lines(1, GOOD),
            ["plan g_kill kill Test_Boar 2 at 1054.0 10.0 50.0"]
        );
        world.start_quest(1, ORB, NOW).expect("start");
        assert_eq!(
            world.quest_plan_lines(1, ORB),
            [
                "plan o_get item Walk_OrbItem 1 gathered node Walk_Orb from the drop of quest object Walk_Orb"
            ],
            "the bot gathers the quest object's nodes"
        );
        world.start_quest(1, DEN, NOW).expect("start");
        assert_eq!(
            world.quest_plan_lines(1, DEN),
            ["plan w_kill kill Test_Boar 3 at 1064.0 10.0 50.0 placed"],
            "a boar made from its vitals is in no spawn group: only the placed ones count"
        );
        world.start_quest(1, SKIN, NOW).expect("start");
        assert_eq!(
            world.quest_plan_lines(1, SKIN),
            ["plan s_get skin Boar 1 kill Test_Boar 1 at 1054.0 10.0 50.0"],
            "a player kills a boar and skins it for the quest's pork"
        );
        world.start_quest(1, SKIN_MOB, NOW).expect("start");
        assert_eq!(
            world.quest_plan_lines(1, SKIN_MOB),
            ["plan m_get skin Boar 1 kill Test_Boar 1 at 1054.0 10.0 50.0"],
            "the drop names the mob: its corpse is skinned as its kill category's gatherable"
        );
        world.start_quest(1, LOOT_QUEST, NOW).expect("start");
        assert_eq!(
            world.quest_plan_lines(1, LOOT_QUEST),
            ["plan k_get loot kill Test_Boar 1 at 1054.0 10.0 50.0"],
            "a player kills a boar and opens its loot bag for the quest's gun"
        );
        world.start_quest(1, RESCUE, NOW).expect("start");
        assert_eq!(
            world.quest_plan_lines(1, RESCUE),
            ["plan r_free gather Walk_Miner 1"],
            "a player gathers the node whose gather fires the step's event"
        );
        world.start_quest(1, CAMP_QUEST, NOW).expect("start");
        assert_eq!(
            world.quest_plan_lines(1, CAMP_QUEST),
            [
                "plan c_bind stop NeedsLook: a camp here: Camp refused: no Camping rank (camp tier 0; tier 1 opens at level 10)."
            ],
            "the plan names why the server would refuse the camp a player places here"
        );
        world.start_quest(1, LORE_QUEST, NOW).expect("start");
        assert_eq!(
            world.quest_plan_lines(1, LORE_QUEST),
            ["plan l_read lore Walk_Note_Title at 1059.0 10.0 50.0"],
            "a player reads the note that unlocks the step's achievement, at its lore point"
        );
        world.start_quest(1, TRIAL_QUEST, NOW).expect("start");
        let trial = super::super::instances::tests::trial();
        assert_eq!(
            world.quest_plan_lines(1, TRIAL_QUEST),
            [format!("plan t_enter trial {} {:08x}", trial.id, trial.crc)],
            "a player enters the trial from afar"
        );
        assert_eq!(
            ClientStep::Cutscene(0x1a2b_3c4d).words(),
            "cutscene 1a2b3c4d"
        );
        assert_eq!(ClientStep::Faction(SYNDICATE).words(), "faction 1");
    }

    #[test]
    fn a_cyclic_state_step_is_gathers_of_its_stone_until_the_turns_reach_the_state() {
        // Retail `06A_MSQ_18_ReturnSiriusJewel`: stone 1 (state 1 of 4) gathered twice, stone 2
        // (state 0) once; one gatherable shows every state of a stone.
        const QUEST: &str = "walk_cyclic";
        const STONE: &str = "Walk_Stone";
        const STATES: usize = 4;
        let (mut world, _) = walk_world();
        world
            .admit(
                &super::super::tests::admit_at(1, CELL_M / 2.0, true),
                NOW,
                NOW,
            )
            .expect("admit");
        let mut content = (*world.content).clone();
        content.quests = QuestBook::build(
            [ObjectiveRow {
                id: QUEST.to_owned(),
                root: "y_root".to_owned(),
                ..ObjectiveRow::default()
            }],
            TaskBook::new(vec![
                task("y_root", "ConsecutiveTaskContainer", &["y_turn"]),
                TaskRow {
                    item_name: Some(STONE.to_owned()),
                    item_drop_probability: Some(1),
                    ..task("y_turn", "TaskGatherCyclicState", &[])
                },
            ]),
        );
        content.gatherables =
            nw_content::gathering::Gatherables::build([nw_content::gathering::Gatherable {
                id: STONE.to_owned(),
                gather_ms: 0,
                respawn_ms: (0, 0),
                loot_table: None,
                loot_container: false,
                tradeskill: None,
                tool_required: false,
                by_hand: true,
                game_event: None,
                animation: None,
            }]);
        content.gather_points =
            PlacedPoints::of([(lowercase_crc32(STONE), at(CELL_M / 2.0 + 20.0))]);
        content.cyclic_gatherables = nw_content::gathering::CyclicGatherables::build([(
            STONE.to_owned(),
            vec![Some(STONE.to_owned()); STATES],
        )]);
        world.content = Arc::new(content);
        world.start_quest(1, QUEST, NOW).expect("start");
        assert_eq!(
            world.quest_plan_lines(1, QUEST),
            [format!("plan y_turn gather {STONE} 2 again")],
            "state 1 from a stone in state 0: two gathers of the same stone"
        );
        let entity = world.player_by_number(1).expect("player");
        world
            .world
            .get_mut::<Player>(entity)
            .expect("player")
            .cyclic_turns
            .insert(STONE.to_lowercase(), 1);
        assert_eq!(
            world.quest_plan_lines(1, QUEST),
            [format!("plan y_turn gather {STONE} 1 again")],
            "one turn done: one gather left"
        );
    }

    #[test]
    fn a_cyclic_step_named_by_the_cyclic_s_variant_is_the_gather_of_its_state_s_gatherable() {
        // Retail `09a_m14_getinfusedseed`: `09A_M14_PlaceStaff` (state 0 of
        // `Cyclic_09A_M14_NatureRitual`) ended with the gather of `09A_M14_NatureRitual_0`.
        const QUEST: &str = "walk_ritual";
        const CYCLIC: &str = "Cyclic_Walk_Ritual";
        const STATES: [&str; 3] = ["Walk_Ritual_0", "Walk_Ritual_1", "Walk_Ritual_2"];
        let (mut world, _) = walk_world();
        world
            .admit(
                &super::super::tests::admit_at(1, CELL_M / 2.0, true),
                NOW,
                NOW,
            )
            .expect("admit");
        let mut content = (*world.content).clone();
        content.quests = QuestBook::build(
            [ObjectiveRow {
                id: QUEST.to_owned(),
                root: "r_root".to_owned(),
                ..ObjectiveRow::default()
            }],
            TaskBook::new(vec![
                task("r_root", "ConsecutiveTaskContainer", &["r_place"]),
                TaskRow {
                    item_name: Some(CYCLIC.to_owned()),
                    item_drop_probability: Some(0),
                    ..task("r_place", "TaskGatherCyclicState", &[])
                },
            ]),
        );
        content.gatherables = nw_content::gathering::Gatherables::build(STATES.map(|id| {
            nw_content::gathering::Gatherable {
                id: id.to_owned(),
                gather_ms: 0,
                respawn_ms: (0, 0),
                loot_table: None,
                loot_container: false,
                tradeskill: None,
                tool_required: false,
                by_hand: true,
                game_event: None,
                animation: None,
            }
        }));
        content.gather_points =
            PlacedPoints::of(STATES.map(|id| (lowercase_crc32(id), at(CELL_M / 2.0 + 20.0))));
        content.cyclic_gatherables = nw_content::gathering::CyclicGatherables::build([(
            CYCLIC.to_owned(),
            STATES.map(|id| Some(id.to_owned())).to_vec(),
        )]);
        world.content = Arc::new(content);
        world.start_quest(1, QUEST, NOW).expect("start");
        assert_eq!(
            world.quest_plan_lines(1, QUEST),
            [format!("plan r_place gather {} 1", STATES[0])],
            "the gatherable that the cyclic shows in state 0, once"
        );
    }

    #[test]
    fn a_go_step_outside_the_instance_map_is_a_stop_so_the_player_leaves_first() {
        // Live 2026-11-22: `99D_MSQ_07_PortalFight_triggerArea_exitCorruptionTrigger` (8711 7733
        // of the open world) came next inside the `QuestNihilo` trial.
        const PHASE: u64 = 1;
        let mode = super::super::instances::tests::trial();
        let map = &mode.map;
        let mut settings = settings(MANY, MAX_PLAYERS);
        settings.instance_role = true;
        settings.level = nw_messages::LevelSpec::instance(
            &map.folder,
            &map.map_name,
            &map.coatlicue_path,
            map.frame,
        );
        let content = crate::player_content::PlayerContent {
            instances: nw_content::instances::InstanceCatalog::build([mode.clone()]),
            ..crate::player_content::PlayerContent::default()
        };
        let mut inside = SimWorld::new(
            settings,
            crate::login::WorldIds::random(0).expect("ids"),
            Arc::new(NoGround),
            slot("inst-1"),
            test_combat(),
            Arc::new(content),
        );
        let go = |x: f32, y: f32| StepPlan::Go {
            at: Position { x, y, z: 0.0 },
            territory: None,
        };
        let [left, bottom, width, height] = map.frame;
        let (in_x, in_y) = (left + width / 2.0, bottom + height / 2.0);
        assert_eq!(
            inside.go_outside_instance(go(8711.0, 7733.0)),
            go(8711.0, 7733.0),
            "with no instance running a go step stays"
        );
        assert_eq!(
            inside.open_instance(Uuid::from_u128(1), &mode.id, PHASE, Vec::new(), None, NOW),
            nw_bus::InstanceOpened::Opened
        );
        assert!(matches!(
            inside.go_outside_instance(go(8711.0, 7733.0)),
            StepPlan::Stop(Outcome::NotWalked, _)
        ));
        assert_eq!(
            inside.go_outside_instance(go(in_x, in_y)),
            go(in_x, in_y),
            "a place in the map's frame is a go step"
        );
    }

    #[test]
    fn a_kill_that_counts_only_in_a_trial_enters_the_trial_first() {
        // Retail `99B_MSQ_10_d_03_01_02_02_02_Kill_1_Umberto`: a kill of game mode
        // `QuestCaerDun`, with no game mode step before it; the player entered from afar.
        const QUEST: &str = "walk_trial_kill";
        const ENEMY: &str = "Walk_Umberto";
        let (mut world, _) = walk_world();
        world
            .admit(
                &super::super::tests::admit_at(1, CELL_M / 2.0, true),
                NOW,
                NOW,
            )
            .expect("admit");
        let trial = super::super::instances::tests::trial();
        let mut content = (*world.content).clone();
        content.quests = QuestBook::build(
            [ObjectiveRow {
                id: QUEST.to_owned(),
                root: "u_root".to_owned(),
                ..ObjectiveRow::default()
            }],
            TaskBook::new(vec![
                task("u_root", "ConsecutiveTaskContainer", &["u_kill"]),
                TaskRow {
                    kill_enemy_type: Some(ENEMY.to_owned()),
                    target_qty: Some(1),
                    game_mode_id: Some(trial.id.clone()),
                    ..task("u_kill", "TaskKillContribution", &[])
                },
            ]),
        );
        world.content = Arc::new(content);
        world.start_quest(1, QUEST, NOW).expect("start");
        let lines = world.quest_plan_lines(1, QUEST);
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].starts_with(&format!("plan u_kill trial {} {:08x}", trial.id, trial.crc)),
            "outside the trial the kill's plan is the trial's entry: {lines:?}"
        );
    }

    #[test]
    fn an_item_that_drops_only_from_an_instance_s_encounter_mob_enters_that_instance_first() {
        // Live 2026-11-22 14:4x: `0411_0414_d_03_task_Diadem` drops from the boss that only
        // `DungeonShatteredObelisk`'s `EncounterBoss` spawns; outside it, its plan was "no
        // source".
        use nw_world::dungeon_scripts::{
            Encounter, LevelScript, Spawn, Stage, Target, TargetMob, TargetSlice,
        };
        let (mut world, _) = walk_world();
        world
            .admit(
                &super::super::tests::admit_at(1, CELL_M / 2.0, true),
                NOW,
                NOW,
            )
            .expect("admit");
        world.combat.spawns = Vec::new().into();
        let trial = super::super::instances::tests::trial();
        let mut content = (*world.content).clone();
        content.dungeon_scripts.game_modes.insert(
            trial.id.clone(),
            LevelScript {
                encounters: vec![Encounter {
                    stages: vec![Stage {
                        spawns: vec![Spawn {
                            target: 0,
                            count: 1,
                            ..Spawn::default()
                        }],
                        ..Stage::default()
                    }],
                    targets: vec![Target {
                        slices: vec![TargetSlice {
                            mob: Some(TargetMob {
                                vitals_id: TEST_MOB.to_owned(),
                                ..TargetMob::default()
                            }),
                            ..TargetSlice::default()
                        }],
                        ..Target::default()
                    }],
                    ..Encounter::default()
                }],
                ..LevelScript::default()
            },
        );
        world.content = Arc::new(content);
        world.start_quest(1, LOOT_QUEST, NOW).expect("start");
        let lines = world.quest_plan_lines(1, LOOT_QUEST);
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].starts_with(&format!("plan k_get trial {} {:08x}", trial.id, trial.crc)),
            "outside the instance the drop's plan is the instance's entry: {lines:?}"
        );
    }

    #[test]
    fn a_step_of_any_type_with_a_game_mode_enters_its_trial_first() {
        // Retail `99F_MSQ_ER_FightLiang_KillLiang`: game event
        // `99F_MSQ_08_Liang_Soul_Separation_interact` of game mode `QuestLiang`, after a trigger
        // area ended the partial container of the game mode step (capture
        // `99f_msq_er_fightliang`: one 5037, then the 946); `09A_M15_TriggerArea_TrialEnter`: a
        // trigger area of game mode `QuestAdiana` (capture `09a_m15_fightartemis`: one 5037).
        const QUEST: &str = "walk_trial_event";
        const EVENT: &str = "Walk_Soul_Separation_interact";
        const AREA_QUEST: &str = "walk_trial_area";
        const AREA_PLACE: &str = "1100.0 10.0 50.0";
        let (mut world, _) = walk_world();
        world
            .admit(
                &super::super::tests::admit_at(1, CELL_M / 2.0, true),
                NOW,
                NOW,
            )
            .expect("admit");
        let trial = super::super::instances::tests::trial();
        let mut content = (*world.content).clone();
        content.quests = QuestBook::build(
            [
                ObjectiveRow {
                    id: QUEST.to_owned(),
                    root: "v_root".to_owned(),
                    ..ObjectiveRow::default()
                },
                ObjectiveRow {
                    id: AREA_QUEST.to_owned(),
                    root: "x_root".to_owned(),
                    ..ObjectiveRow::default()
                },
            ],
            TaskBook::new(vec![
                task("v_root", "ConsecutiveTaskContainer", &["v_event"]),
                TaskRow {
                    kill_enemy_type: Some(EVENT.to_owned()),
                    game_mode_id: Some(trial.id.clone()),
                    ..task("v_event", "TaskGameEvent", &[])
                },
                task("x_root", "ConsecutiveTaskContainer", &["x_area"]),
                TaskRow {
                    override_location: Some(AREA_PLACE.to_owned()),
                    game_mode_id: Some(trial.id.clone()),
                    ..task("x_area", "TaskTriggerArea", &[])
                },
            ]),
        );
        world.content = Arc::new(content);
        world.start_quest(1, QUEST, NOW).expect("start");
        let lines = world.quest_plan_lines(1, QUEST);
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].starts_with(&format!(
                "plan v_event trial {} {:08x}",
                trial.id, trial.crc
            )),
            "outside the trial the event's plan is the trial's entry: {lines:?}"
        );
        world.start_quest(1, AREA_QUEST, NOW).expect("start");
        let lines = world.quest_plan_lines(1, AREA_QUEST);
        assert!(
            lines[0].starts_with(&format!("plan x_area trial {} {:08x}", trial.id, trial.crc)),
            "outside the trial the area's plan is the trial's entry: {lines:?}"
        );
    }

    #[test]
    fn a_trial_that_no_player_enters_from_afar_is_entered_at_its_unlocked_door() {
        // Retail `06A_MSQ_21_DefeatBoss_enterSoulTrial`: `QuestMedea` is no remote entry; its
        // two capitals `4_1_Trial_Entrance_QuestMedea` and `4__Trial_Entrance_QuestMedea_Locked`
        // stand at the trial.
        const LOCKED_DOOR: u64 = 0x10;
        const DOOR: u64 = 0x20;
        let (mut world, _) = walk_world();
        world
            .admit(
                &super::super::tests::admit_at(1, CELL_M / 2.0, true),
                NOW,
                NOW,
            )
            .expect("admit");
        let trial = super::super::instances::tests::trial();
        let mut content = (*world.content).clone();
        content.instances =
            nw_content::instances::InstanceCatalog::build([nw_content::instances::InstanceMode {
                remote_entry: false,
                ..trial.clone()
            }]);
        content.entrances = [
            (
                LOCKED_DOOR,
                format!("Dungeon/Trial/4__Trial_Entrance_{}_Locked", trial.id),
            ),
            (
                DOOR,
                format!("Dungeon/Trial/4_1_Trial_Entrance_{}", trial.id),
            ),
        ]
        .into_iter()
        .map(|(capital, slice)| {
            let door = nw_content::dungeon_entrances::EntranceDoor {
                slice,
                area_mode: None,
            };
            (capital, door)
        })
        .collect();
        world.content = Arc::new(content.clone());
        world.start_quest(1, TRIAL_QUEST, NOW).expect("start");
        assert_eq!(
            world.quest_plan_lines(1, TRIAL_QUEST),
            [format!(
                "plan t_enter trial {} {:08x} door {DOOR:016x}",
                trial.id, trial.crc
            )],
            "a player enters the trial at its unlocked door"
        );
        content.entrance_entities = vec![crate::player_content::EntranceEntity {
            capital: DOOR,
            position: at(CELL_M / 2.0 + 40.0),
            entrance_component: 0,
            members: Vec::new(),
        }];
        world.content = Arc::new(content.clone());
        assert_eq!(
            world.quest_plan_lines(1, TRIAL_QUEST),
            [format!(
                "plan t_enter trial {} {:08x} door {DOOR:016x} at 1064.0 10.0 50.0",
                trial.id, trial.crc
            )],
            "the door's place when the level's entrance entities hold it"
        );
        content.entrances.clear();
        world.content = Arc::new(content);
        assert!(
            world.quest_plan_lines(1, TRIAL_QUEST)[0].contains("stop"),
            "with no door the plan stays the step's stop"
        );
    }

    #[test]
    fn a_placed_mob_of_the_spawn_group_counts_and_a_spawned_one_does_not() {
        const LETHAL: u32 = 100_000;
        let (mut world, _) = walk_world();
        world.set_table(&table(&["sim-a", "sim-b"])).expect("table");
        world
            .admit(
                &super::super::tests::admit_at(1, CELL_M / 2.0 + 40.0, true),
                NOW,
                NOW,
            )
            .expect("admit");
        world.start_quest(1, DEN, NOW).expect("start");
        let at = nw_bus::Point {
            x: CELL_M / 2.0 + 40.0,
            y: 10.0,
            z: 50.0,
        };
        let spawned = world
            .spawn_gm(TEST_MOB, TEST_LEVEL, at, 1, None)
            .expect("spawn")[0];
        world.apply_hit(spawned, 1, LETHAL, 0, NOW);
        assert_eq!(
            world.quest_plan_lines(1, DEN),
            ["plan w_kill kill Test_Boar 3 at 1064.0 10.0 50.0 placed"],
            "a GM mob is in no spawn group"
        );
        world.apply_hit(crate::mobs::mob_entity(1), 1, LETHAL, 0, NOW);
        assert_eq!(
            world.quest_plan_lines(1, DEN),
            ["plan w_kill kill Test_Boar 2 at 1064.0 10.0 50.0 placed"],
            "the placed boar of the group counts"
        );
    }

    /// A world that runs the open world's encounters when `world_events`, with one encounter of
    /// slice [`ENCOUNTER_SLICE`] at `encounter_at` whose only stage spawns the test boar;
    /// `starts_by_itself` makes it start when a player comes near.
    fn encounter_world(world_events: bool, starts_by_itself: bool, encounter_at: f32) -> SimWorld {
        use nw_world::dungeon_scripts::{
            DungeonScripts, Encounter, LevelScript, Spawn, Stage, Target, TargetMob, TargetSlice,
        };
        use nw_world::level_script::{SpawnerKind, SpawnerRules};

        let boar = TargetSlice {
            mob: Some(TargetMob {
                vitals_id: TEST_MOB.to_owned(),
                ..TargetMob::default()
            }),
            ..TargetSlice::default()
        };
        let encounter = Encounter {
            slice: ENCOUNTER_SLICE.to_owned(),
            position: [encounter_at, 10.0, 50.0],
            auto_activate: false,
            spawner: Some(SpawnerRules {
                kind: SpawnerKind::Area,
                live_count: 1,
                respawn_s: [0.0, 0.0],
                spawn_on_enable: starts_by_itself,
                respawn_after_death: false,
                maintain_population: false,
            }),
            stages: vec![Stage {
                spawns: vec![Spawn {
                    target: 0,
                    count: 1,
                    ..Spawn::default()
                }],
                ..Stage::default()
            }],
            targets: vec![Target {
                name: "walk_boar_spawner".to_owned(),
                slices: vec![boar],
            }],
            ..Encounter::default()
        };
        let scripts = DungeonScripts {
            game_modes: std::collections::BTreeMap::from([(
                OPEN_WORLD_LEVEL.to_owned(),
                LevelScript {
                    encounters: vec![encounter],
                    ..LevelScript::default()
                },
            )]),
        };
        SimWorld::new(
            super::super::WorldSettings {
                world_events,
                ..settings(MANY, MAX_PLAYERS)
            },
            WorldIds::random(0).expect("ids"),
            Arc::new(NoGround),
            slot("sim-a"),
            test_combat(),
            Arc::new(PlayerContent {
                dungeon_scripts: scripts,
                ..PlayerContent::default()
            }),
        )
    }

    /// The slice of [`encounter_world`]'s encounter.
    const ENCOUNTER_SLICE: &str = "slices/encounters/walk_boar_encounter.dynamicslice";
    /// Where the encounter spawn of the boar stands (x).
    const SPAWN_X: f32 = CELL_M / 2.0 + 30.0;
    /// Far beyond any spawn group of the boar.
    const FAR_M: f32 = 1_000.0;

    /// The level's encounter spawn of the test boar at [`SPAWN_X`].
    fn encounter_spawn() -> VitalsSpawn {
        VitalsSpawn {
            is_encounter: true,
            ..test_spawn(SPAWN_X)
        }
    }

    /// Retail `12A_MSQ_24_DefeatGawain_Defeat_Gawain` in the `QuestGawain` trial: the trial's
    /// `EncounterBoss` spawns `Lost_Monarch` at its own place, not where the open map's spawn
    /// of that mob stands.
    #[test]
    fn a_kill_step_in_an_instance_is_the_fight_of_its_encounter_mob_where_it_spawns() {
        use nw_world::dungeon_scripts::{
            Encounter, LevelScript, Spawn, Stage, Target, TargetMob, TargetSlice,
        };
        const BOSS_AT: [f32; 3] = [6854.0, 2930.0, 77.0];
        const BOSS_SLICE: &str = "slices/encounters/walk_boss.dynamicslice";
        const BOSS_LEVEL: u32 = 17;
        let (mut world, _) = walk_world();
        world
            .admit(
                &super::super::tests::admit_at(1, CELL_M / 2.0, true),
                NOW,
                NOW,
            )
            .expect("admit");
        world.start_quest(1, NO_MOB, NOW).expect("start");
        let open_world = world.quest_plan_lines(1, NO_MOB);
        world.dungeon_run = Some(super::super::dungeon_script::DungeonRun::new(
            uuid::Uuid::from_u128(1),
            LevelScript {
                encounters: vec![Encounter {
                    slice: BOSS_SLICE.to_owned(),
                    stages: vec![Stage {
                        spawns: vec![Spawn {
                            target: 0,
                            count: 1,
                            locations: vec![BOSS_AT],
                            ..Spawn::default()
                        }],
                        ..Stage::default()
                    }],
                    targets: vec![Target {
                        slices: vec![TargetSlice {
                            mob: Some(TargetMob {
                                vitals_id: TEST_MOB.to_owned(),
                                level: BOSS_LEVEL,
                                ..TargetMob::default()
                            }),
                            ..TargetSlice::default()
                        }],
                        ..Target::default()
                    }],
                    ..Encounter::default()
                }],
                ..LevelScript::default()
            },
        ));
        world.start_quest(1, GOOD, NOW).expect("start");
        world.quest_event(1, Event::Talked(NPC), NOW);
        assert_eq!(
            world.quest_plan_lines(1, GOOD),
            [format!(
                "plan g_kill kill {TEST_MOB} 2 at 6854.0 2930.0 77.0 placed"
            )],
            "the player fights the boss its encounter spawns"
        );
        assert_eq!(
            world.quest_plan_lines(1, NO_MOB),
            open_world,
            "a mob that no encounter of the instance spawns is planned as before"
        );
    }

    #[test]
    fn a_quest_drop_of_an_instance_encounter_mob_is_the_loot_of_its_kill() {
        // Live 2026-11-22 14:02:10: `0411_0414_d_03_task_Diadem` drops
        // `0411_0414_StartstoneDiadem` from the Starstone boss, whose only spawn is the
        // instance's `EncounterBoss`; its plan was "no source".
        use nw_world::dungeon_scripts::{
            Encounter, LevelScript, Spawn, Stage, Target, TargetMob, TargetSlice,
        };
        const BOSS_AT: [f32; 3] = [378.5, 575.7, 335.0];
        let (mut world, _) = walk_world();
        world
            .admit(
                &super::super::tests::admit_at(1, CELL_M / 2.0, true),
                NOW,
                NOW,
            )
            .expect("admit");
        world.combat.spawns = Vec::new().into();
        world.start_quest(1, LOOT_QUEST, NOW).expect("start");
        assert!(
            world.quest_plan_lines(1, LOOT_QUEST)[0].contains("no source of Walk_Gun"),
            "no mob of the level drops it"
        );
        world.dungeon_run = Some(super::super::dungeon_script::DungeonRun::new(
            uuid::Uuid::from_u128(1),
            LevelScript {
                encounters: vec![Encounter {
                    stages: vec![Stage {
                        spawns: vec![Spawn {
                            target: 0,
                            count: 1,
                            locations: vec![BOSS_AT],
                            ..Spawn::default()
                        }],
                        ..Stage::default()
                    }],
                    targets: vec![Target {
                        slices: vec![TargetSlice {
                            mob: Some(TargetMob {
                                vitals_id: TEST_MOB.to_owned(),
                                ..TargetMob::default()
                            }),
                            ..TargetSlice::default()
                        }],
                        ..Target::default()
                    }],
                    ..Encounter::default()
                }],
                ..LevelScript::default()
            },
        ));
        assert_eq!(
            world.quest_plan_lines(1, LOOT_QUEST),
            ["plan k_get loot kill Test_Boar 1 at 378.5 575.7 335.0 placed"],
            "the player kills the encounter's mob and opens its loot bag"
        );
    }

    /// Retail `09a_s3_hunthercyne`: `09A_S3_HercyneCerebrum_Check` drops
    /// `09A_S3_HercyneCerebrum` from `FL_Hercyne_Reindeer_WaveEvent`, whose only spawn is the
    /// one that `Enc_09A_S3_WoundedHercyne` spawns in `Wave_02` (killed at 152.1 s); the plan
    /// was "no source". The kill at the encounter, then its loot bag, gives the item.
    #[test]
    fn a_quest_drop_of_an_open_world_encounter_mob_is_the_loot_of_its_kill() {
        const ITEM: &str = "Walk_Cerebrum";
        let world_of = |world_events: bool| {
            let mut world = encounter_world(world_events, true, SPAWN_X);
            world.combat.spawns = Vec::new().into();
            let mut content = (*world.content).clone();
            content.kill_types = KillTypes::build([(TEST_MOB.to_owned(), "Hostile".to_owned())]);
            world.content = Arc::new(content);
            world
        };
        let world = world_of(true);
        let level = QuestGuide {
            all_spawns: vec![encounter_spawn()],
            ..QuestGuide::default()
        };
        let step = WaitingStep {
            quest: "walk_cerebrum".to_owned(),
            task: "walk_cerebrum_check".to_owned(),
            kind: TaskKind::HaveItems {
                item: ITEM.to_owned(),
                count: 1,
                returned: true,
                drop: Some(crate::objectives::QuestDrop {
                    sources: vec![TEST_MOB.to_owned()],
                    percent: 100,
                    place: None,
                }),
                chest: None,
            },
            progress: 0,
            container_item: false,
            game_mode: None,
        };
        let plan = world.walk_plan(&level, &step, at(CELL_M / 2.0));
        assert_eq!(
            plan,
            StepPlan::Item {
                item: ITEM.to_owned(),
                count: 1,
                gathered: false,
                source: format!("the drop of {TEST_MOB}"),
                node: None,
            }
        );
        let loot = world.loot_from_drop(&level, &step, plan);
        assert_eq!(
            world.plan_what(&loot),
            format!("loot kill {TEST_MOB} 1 at {SPAWN_X:.1} 10.0 50.0 placed within 30.0"),
            "the player kills the encounter's own mob in its volume (no game master's mob) and \
             opens its loot bag"
        );
        assert!(
            matches!(
                world_of(false).walk_plan(&level, &step, at(CELL_M / 2.0)),
                StepPlan::Stop(Outcome::Broken, _)
            ),
            "a sim that does not run the open world's encounters has no source"
        );
    }

    /// Retail `07-Tempests_Heart`: `0726_0101_d_01a_AreaTrigger` (tag `AreaEntryGate`) ended at
    /// 163.96 s when the player came into the gate area of the expedition's script.
    #[test]
    fn a_trigger_area_step_named_by_an_area_of_the_instance_script_is_a_go_there() {
        use nw_world::collision::Geometry;
        use nw_world::dungeon_scripts::{Detector, LevelScript, TriggerArea};
        const QUEST: &str = "walk_gate";
        const AREA: &str = "AreaWalkGate";
        let (mut world, _) = walk_world();
        world
            .admit(
                &super::super::tests::admit_at(1, CELL_M / 2.0, true),
                NOW,
                NOW,
            )
            .expect("admit");
        let mut content = (*world.content).clone();
        content.quests = QuestBook::build(
            [ObjectiveRow {
                id: QUEST.to_owned(),
                root: "g_root".to_owned(),
                ..ObjectiveRow::default()
            }],
            TaskBook::new(vec![
                task("g_root", "ConsecutiveTaskContainer", &["g_gate"]),
                TaskRow {
                    target_tag: Some(AREA.to_owned()),
                    ..task("g_gate", "TaskTriggerArea", &[])
                },
            ]),
        );
        world.content = Arc::new(content);
        world.start_quest(1, QUEST, NOW).expect("start");
        assert!(
            world.quest_plan_lines(1, QUEST)[0].contains("Broken"),
            "no instance runs: no area of that name"
        );
        world.dungeon_run = Some(super::super::dungeon_script::DungeonRun::new(
            uuid::Uuid::from_u128(1),
            LevelScript {
                trigger_areas: vec![TriggerArea {
                    name: AREA.to_owned(),
                    volume: Detector {
                        shape: Geometry::Box {
                            half_extents: [5.0, 5.0, 5.0],
                        },
                        place: [
                            [1.0, 0.0, 0.0, 1456.2],
                            [0.0, 1.0, 0.0, 960.3],
                            [0.0, 0.0, 1.0, 184.5],
                        ],
                    },
                }],
                ..LevelScript::default()
            },
        ));
        assert_eq!(
            world.quest_plan_lines(1, QUEST),
            ["plan g_gate go 1456.2 960.3 184.5"]
        );
        world.tick_quests(NOW);
        assert!(
            world
                .quests_of_player(1)
                .iter()
                .any(|quest| quest.def.id == QUEST),
            "a player away from the area is not there"
        );
        let entity = world.player_by_number(1).expect("player");
        let gate = Position {
            x: 1456.2,
            y: 960.3,
            z: 184.5,
        };
        world.place_player(entity, gate, None, super::super::Placement::Stand, NOW);
        world.tick_quests(NOW);
        assert!(
            world
                .quests_of_player(1)
                .iter()
                .all(|quest| quest.def.id != QUEST),
            "a player in the area counts as there"
        );
    }

    /// `QuestLiang`: the encounter of the step's event spawns talisman piles, which a player
    /// takes by walking onto them; the kill plan names their places once each, and the place
    /// of the area trigger that starts the fight (`QuestMedusa`'s `Wave 0 - Setup`).
    #[test]
    fn an_encounter_kill_names_the_places_of_its_touch_pickups() {
        use nw_world::dungeon_scripts::{
            Encounter, LevelScript, Spawn, Stage, Target, TargetMob, TargetSlice,
        };
        const QUEST: &str = "walk_talisman";
        const SUCCESS: &str = "Walk_Talisman_Success";
        const SLICE: &str = "slices/walk/enc_walk_talisman.dynamicslice";
        let (mut world, _) = walk_world();
        world
            .admit(
                &super::super::tests::admit_at(1, CELL_M / 2.0, true),
                NOW,
                NOW,
            )
            .expect("admit");
        let mut content = (*world.content).clone();
        content.quests = QuestBook::build(
            [ObjectiveRow {
                id: QUEST.to_owned(),
                root: "t_root".to_owned(),
                ..ObjectiveRow::default()
            }],
            TaskBook::new(vec![
                task("t_root", "ConsecutiveTaskContainer", &["t_wait"]),
                TaskRow {
                    kill_enemy_type: Some(SUCCESS.to_owned()),
                    ..task("t_wait", "TaskGameEvent", &[])
                },
            ]),
        );
        world.content = Arc::new(content);
        world.start_quest(1, QUEST, NOW).expect("start");
        let pile = |locations: Vec<[f32; 3]>| Spawn {
            target: 1,
            delay_s: 12,
            count: 3,
            locations,
            turns_deg: Vec::new(),
        };
        world.dungeon_run = Some(super::super::dungeon_script::DungeonRun::new(
            uuid::Uuid::from_u128(1),
            LevelScript {
                encounters: vec![Encounter {
                    name: "EncounterBoss".to_owned(),
                    slice: SLICE.to_owned(),
                    position: [1045.1, 943.4, 130.0],
                    targets: vec![
                        Target {
                            slices: vec![TargetSlice {
                                mob: Some(TargetMob {
                                    vitals_id: TEST_MOB.to_owned(),
                                    ..TargetMob::default()
                                }),
                                ..TargetSlice::default()
                            }],
                            ..Target::default()
                        },
                        Target {
                            slices: vec![TargetSlice {
                                path: super::super::boss_scripts::TALISMAN_PILE_SLICE.to_owned(),
                                ..TargetSlice::default()
                            }],
                            ..Target::default()
                        },
                        Target {
                            slices: vec![TargetSlice {
                                path: "slices/encounters/ai_encounter_area_trigger.dynamicslice"
                                    .to_owned(),
                                ..TargetSlice::default()
                            }],
                            ..Target::default()
                        },
                    ],
                    stages: vec![
                        Stage {
                            spawns: vec![
                                pile(vec![[1041.1, 938.4, 130.0], [1034.1, 938.4, 130.0]]),
                                Spawn {
                                    target: 2,
                                    delay_s: 0,
                                    count: 1,
                                    locations: vec![[1050.0, 950.0, 130.0]],
                                    turns_deg: Vec::new(),
                                },
                            ],
                            ..Stage::default()
                        },
                        Stage {
                            spawns: vec![pile(vec![[1034.1, 938.4, 130.0]])],
                            ..Stage::default()
                        },
                    ],
                    rewards: Some(nw_world::level_script::DifficultyRewards {
                        global_event: SUCCESS.to_owned(),
                        ..nw_world::level_script::DifficultyRewards::default()
                    }),
                    ..Encounter::default()
                }],
                ..LevelScript::default()
            },
        ));
        assert_eq!(
            world.quest_plan_lines(1, QUEST),
            [format!(
                "plan t_wait kill {TEST_MOB} 1 at 1045.1 943.4 130.0 placed \
                 touch 1041.1 938.4 130.0 touch 1034.1 938.4 130.0 touch 1050.0 950.0 130.0"
            )]
        );
    }

    #[test]
    fn a_step_that_waits_for_an_encounter_s_success_is_the_fight_at_that_encounter() {
        // Retail `04A_Side02_02_01a_Task_Encounter_01` waits for
        // `04A_Side02_02_Encounter_01_Success`, the success event of the encounter slice
        // `enc_quest_04a_side02_obeliskdefenders_01` at 8732.3 4251.9 181.2.
        use nw_world::dungeon_scripts::{Encounter, LevelScript, Target, TargetMob, TargetSlice};
        const QUEST: &str = "walk_defend";
        const SUCCESS: &str = "Walk_Defend_Success";
        let (mut world, _) = walk_world();
        world
            .admit(
                &super::super::tests::admit_at(1, CELL_M / 2.0, true),
                NOW,
                NOW,
            )
            .expect("admit");
        let mut content = (*world.content).clone();
        content.quests = QuestBook::build(
            [ObjectiveRow {
                id: QUEST.to_owned(),
                root: "z_root".to_owned(),
                ..ObjectiveRow::default()
            }],
            TaskBook::new(vec![
                task("z_root", "ConsecutiveTaskContainer", &["z_wait"]),
                TaskRow {
                    kill_enemy_type: Some(SUCCESS.to_owned()),
                    ..task("z_wait", "TaskGameEvent", &[])
                },
            ]),
        );
        world.content = Arc::new(content);
        world.start_quest(1, QUEST, NOW).expect("start");
        assert!(
            world.quest_plan_lines(1, QUEST)[0].contains("NeedsLook"),
            "no source fires the event: {:?}",
            world.quest_plan_lines(1, QUEST)
        );
        world.dungeon_run = Some(super::super::dungeon_script::DungeonRun::new(
            uuid::Uuid::from_u128(1),
            LevelScript {
                encounters: vec![Encounter {
                    name: "Walk_Defend".to_owned(),
                    position: [8732.3, 4251.9, 181.2],
                    targets: vec![Target {
                        slices: vec![TargetSlice {
                            mob: Some(TargetMob {
                                vitals_id: TEST_MOB.to_owned(),
                                ..TargetMob::default()
                            }),
                            ..TargetSlice::default()
                        }],
                        ..Target::default()
                    }],
                    rewards: Some(nw_world::level_script::DifficultyRewards {
                        global_event: SUCCESS.to_owned(),
                        ..nw_world::level_script::DifficultyRewards::default()
                    }),
                    ..Encounter::default()
                }],
                ..LevelScript::default()
            },
        ));
        assert_eq!(
            world.quest_plan_lines(1, QUEST),
            [format!(
                "plan z_wait kill {TEST_MOB} 1 at 8732.3 4251.9 181.2 placed"
            )],
            "the player fights at the encounter, whose success fires the step's event"
        );
        // Its start object stands: the player gathers it first, by the gather point there.
        const START: &str = "Walk_Defend_Start";
        const START_SLICE: &str = "slices/walk/io_walk_defend_encstart.dynamicslice";
        world.encounter_objects = super::super::encounter_roots::EncounterObjects::build(
            &nw_content::town_props::TownProps::parse(&format!(
                r#"{{"props": [{{"asset": "a28f5f64c3e15e9f9cbe6588b06b71c5", "slice": "{START_SLICE}", "members": []}}]}}"#
            ))
            .expect("table"),
        );
        world
            .encounter_objects
            .spawn(START_SLICE, [8732.3, 4251.9, 181.2], 0.0)
            .expect("spawned");
        // A logging node stands at the same place (`1434_0103`: `Wolf_Elemental_Wood` at the
        // pyre): the plan takes the start object's quest gatherable.
        const LOGGING_NODE: &str = "Walk_Defend_Tree";
        let gatherable = |id: &str, tradeskill: Option<&str>| nw_content::gathering::Gatherable {
            id: id.to_owned(),
            gather_ms: 0,
            respawn_ms: (0, 0),
            loot_table: None,
            loot_container: false,
            tradeskill: tradeskill.map(str::to_owned),
            tool_required: false,
            by_hand: true,
            game_event: Some(format!("{id}_Interact")),
            animation: None,
        };
        let start_at = Position {
            x: 8732.3,
            y: 4251.9,
            z: 181.2,
        };
        let mut content = (*world.content).clone();
        content.gatherables = nw_content::gathering::Gatherables::build([
            gatherable(LOGGING_NODE, Some("Logging")),
            gatherable(START, None),
        ]);
        content.gather_points = PlacedPoints::of([
            (lowercase_crc32(LOGGING_NODE), start_at),
            (lowercase_crc32(START), start_at),
        ]);
        world.content = Arc::new(content);
        assert_eq!(
            world.quest_plan_lines(1, QUEST),
            [format!(
                "plan z_wait start {START} a28f5f64c3e15e9f9cbe6588b06b71c5 at 8732.3 4251.9 181.2"
            )]
        );
    }

    /// Retail `09A_S16_ThrowSwitch`: the step's game event comes from the gather of the switch;
    /// while the encounter's start object stands on the switch's point the plan gathers that
    /// object, else the level node.
    #[test]
    fn a_gather_step_takes_the_start_object_that_stands_on_its_node() {
        const QUEST: &str = "walk_switch";
        const SWITCH: &str = "Walk_ThrowSwitch";
        const SLICE: &str = "slices/walk/io_walk_throwswitch.dynamicslice";
        const ASSET: &str = "a28f5f64c3e15e9f9cbe6588b06b71c5";
        let at = Position {
            x: 8347.2,
            y: 1362.4,
            z: 279.9,
        };
        let (mut world, _) = walk_world();
        world
            .admit(
                &super::super::tests::admit_at(1, CELL_M / 2.0, true),
                NOW,
                NOW,
            )
            .expect("admit");
        let mut content = (*world.content).clone();
        content.quests = QuestBook::build(
            [ObjectiveRow {
                id: QUEST.to_owned(),
                root: "y_root".to_owned(),
                ..ObjectiveRow::default()
            }],
            TaskBook::new(vec![
                task("y_root", "ConsecutiveTaskContainer", &["y_switch"]),
                TaskRow {
                    kill_enemy_type: Some(format!("{SWITCH}_Interact")),
                    ..task("y_switch", "TaskGameEvent", &[])
                },
            ]),
        );
        content.gatherables =
            nw_content::gathering::Gatherables::build([nw_content::gathering::Gatherable {
                id: SWITCH.to_owned(),
                gather_ms: 0,
                respawn_ms: (0, 0),
                loot_table: None,
                loot_container: true,
                tradeskill: None,
                tool_required: false,
                by_hand: true,
                game_event: Some(format!("{SWITCH}_Interact")),
                animation: None,
            }]);
        content.gather_points = PlacedPoints::of([(lowercase_crc32(SWITCH), at)]);
        world.content = Arc::new(content);
        world.start_quest(1, QUEST, NOW).expect("start");
        assert_eq!(
            world.quest_plan_lines(1, QUEST),
            [format!("plan y_switch gather {SWITCH} 1")]
        );
        world.encounter_objects = super::super::encounter_roots::EncounterObjects::build(
            &nw_content::town_props::TownProps::parse(&format!(
                r#"{{"props": [{{"asset": "{ASSET}", "slice": "{SLICE}", "members": []}}]}}"#
            ))
            .expect("table"),
        );
        world
            .encounter_objects
            .spawn(SLICE, [at.x, at.y, at.z], 0.0)
            .expect("spawned");
        assert_eq!(
            world.quest_plan_lines(1, QUEST),
            [format!(
                "plan y_switch start {SWITCH} {ASSET} at 8347.2 1362.4 279.9"
            )]
        );
    }

    /// Retail `09a_s3_hunthercyne` 126.3 s: the fresh kill gathered is the object that
    /// `Enc_09A_S3_WoundedHercyne`'s `Wave_01` spawns after its 3 s setup. Before the object
    /// stands, the player awaits it at the encounter's centre; the level node under it is no
    /// gather (live 2026-11-22 20:38:40 it ran no wave).
    #[test]
    fn a_start_object_an_encounter_spawns_later_is_waited_for_in_its_volume() {
        use nw_world::dungeon_scripts::{
            DungeonScripts, Encounter, LevelScript, Spawn, Stage, Target, TargetSlice,
        };
        const QUEST: &str = "walk_freshkill";
        const KILL: &str = "Walk_FreshKill";
        const SLICE: &str = "slices/walk/io_walk_freshkill.dynamicslice";
        const ASSET: &str = "a28f5f64c3e15e9f9cbe6588b06b71c6";
        const VOLUME_M: f32 = 30.0;
        let place = Position {
            x: 8449.2,
            y: 1572.2,
            z: 169.6,
        };
        let (mut world, _) = walk_world();
        world.settings.world_events = true;
        world
            .admit(
                &super::super::tests::admit_at(1, CELL_M / 2.0, true),
                NOW,
                NOW,
            )
            .expect("admit");
        let mut content = (*world.content).clone();
        content.quests = QuestBook::build(
            [ObjectiveRow {
                id: QUEST.to_owned(),
                root: "f_root".to_owned(),
                ..ObjectiveRow::default()
            }],
            TaskBook::new(vec![
                task("f_root", "ConsecutiveTaskContainer", &["f_kill"]),
                TaskRow {
                    kill_enemy_type: Some(format!("{KILL}_Interact")),
                    ..task("f_kill", "TaskGameEvent", &[])
                },
            ]),
        );
        content.gatherables =
            nw_content::gathering::Gatherables::build([nw_content::gathering::Gatherable {
                id: KILL.to_owned(),
                gather_ms: 0,
                respawn_ms: (0, 0),
                loot_table: None,
                loot_container: true,
                tradeskill: None,
                tool_required: false,
                by_hand: true,
                game_event: Some(format!("{KILL}_Interact")),
                animation: None,
            }]);
        content.gather_points = PlacedPoints::of([(lowercase_crc32(KILL), place)]);
        let setup = Stage::default();
        let interact = Stage {
            spawns: vec![Spawn {
                target: 0,
                count: 1,
                locations: vec![[place.x, place.y, place.z]],
                ..Spawn::default()
            }],
            ..Stage::default()
        };
        content.dungeon_scripts = DungeonScripts {
            game_modes: std::collections::BTreeMap::from([(
                OPEN_WORLD_LEVEL.to_owned(),
                LevelScript {
                    encounters: vec![Encounter {
                        position: [place.x, place.y, place.z],
                        auto_activate: true,
                        detection_radius_m: Some(VOLUME_M),
                        top_stages: vec![0, 1],
                        stages: vec![setup, interact],
                        targets: vec![Target {
                            slices: vec![TargetSlice {
                                path: SLICE.to_owned(),
                                ..TargetSlice::default()
                            }],
                            ..Target::default()
                        }],
                        ..Encounter::default()
                    }],
                    ..LevelScript::default()
                },
            )]),
        };
        world.content = Arc::new(content);
        world.encounter_objects = super::super::encounter_roots::EncounterObjects::build(
            &nw_content::town_props::TownProps::parse(&format!(
                r#"{{"props": [{{"asset": "{ASSET}", "slice": "{SLICE}", "members": []}}]}}"#
            ))
            .expect("table"),
        );
        world.start_quest(1, QUEST, NOW).expect("start");
        assert_eq!(
            world.quest_plan_lines(1, QUEST),
            [format!("plan f_kill await {KILL} at 8449.2 1572.2 169.6")],
            "the player awaits the object in the volume"
        );
        world
            .encounter_objects
            .spawn(SLICE, [place.x, place.y, place.z], 0.0)
            .expect("spawned");
        assert_eq!(
            world.quest_plan_lines(1, QUEST),
            [format!(
                "plan f_kill start {KILL} {ASSET} at 8449.2 1572.2 169.6"
            )],
            "the object stands: its gather"
        );
        world.settings.world_events = false;
        assert_eq!(
            world.wait_for_start_object(KILL, place),
            None,
            "a sim that runs no open world encounter"
        );
    }

    /// Live 2026-11-22 21:31:38: the start object `io_worldevent_09a_s17_protectranger` stands
    /// where `09A_S17_WagonSupplies` (a later step's object) has its point too; its gather is
    /// that of `09A_S17_ProtectRanger`, the gatherable its slice is named after.
    #[test]
    fn a_start_object_s_gatherable_is_the_one_its_slice_is_named_after() {
        const RANGER: &str = "09A_S17_ProtectRanger";
        const WAGON: &str = "09A_S17_WagonSupplies";
        const SLICE: &str = "slices/worldevents/firstlight/interact/protectranger/\
                             io_worldevent_09a_s17_protectranger.dynamicslice";
        let place = Position {
            x: 8541.4,
            y: 1277.0,
            z: 219.4,
        };
        let (mut world, _) = walk_world();
        let mut content = (*world.content).clone();
        let quest_object = |id: &str| nw_content::gathering::Gatherable {
            id: id.to_owned(),
            gather_ms: 0,
            respawn_ms: (0, 0),
            loot_table: None,
            loot_container: true,
            tradeskill: None,
            tool_required: false,
            by_hand: true,
            game_event: Some(format!("{id}_Interact")),
            animation: None,
        };
        content.gatherables =
            nw_content::gathering::Gatherables::build([quest_object(WAGON), quest_object(RANGER)]);
        content.gather_points = PlacedPoints::of([(lowercase_crc32(WAGON), place)]);
        world.content = Arc::new(content);
        assert_eq!(
            world
                .start_object_gatherable(SLICE, place)
                .map(|gatherable| gatherable.id.as_str()),
            Some(RANGER)
        );
        assert_eq!(
            world
                .start_object_gatherable("slices/walk/other_object.dynamicslice", place)
                .map(|gatherable| gatherable.id.as_str()),
            Some(WAGON),
            "a slice of no gatherable's name: the quest gatherable at its place"
        );
    }

    /// A world whose quest `QUEST` waits, in a simple container, on the slayer script task
    /// `script` and on the achievement steps `achievements`, with the gatherable points
    /// `points`; player 1 is on it.
    fn puzzle_world(script: &str, achievements: &[&str], points: &[(&str, Position)]) -> SimWorld {
        let (mut world, _) = walk_world();
        world
            .admit(
                &super::super::tests::admit_at(1, CELL_M / 2.0, true),
                NOW,
                NOW,
            )
            .expect("admit");
        let mut children = vec!["z_puzzle".to_owned()];
        let mut rows = vec![TaskRow {
            item_name: Some(script.to_owned()),
            ..task("z_puzzle", "TaskSlayerScript", &[])
        }];
        for (index, achievement) in achievements.iter().enumerate() {
            let id = format!("z_altar_{index}");
            rows.push(TaskRow {
                item_name: Some((*achievement).to_owned()),
                ..task(&id, "TaskAchievement", &[])
            });
            children.push(id);
        }
        let children: Vec<&str> = children.iter().map(String::as_str).collect();
        rows.push(task("z_root", "SimpleTaskContainer", &children));
        let mut content = (*world.content).clone();
        content.quests = QuestBook::build(
            [ObjectiveRow {
                id: PUZZLE_QUEST.to_owned(),
                root: "z_root".to_owned(),
                ..ObjectiveRow::default()
            }],
            TaskBook::new(rows),
        );
        content.gather_points = PlacedPoints::of(
            points
                .iter()
                .map(|(gatherable, at)| (lowercase_crc32(gatherable), *at)),
        );
        world.content = Arc::new(content);
        world.start_quest(1, PUZZLE_QUEST, NOW).expect("start");
        world
    }

    const PUZZLE_QUEST: &str = "walk_puzzle";

    /// Retail `99f_msq_er_meetwiseman`: three weapon hits at the gongs (in the order 2, 1, 3)
    /// end `99F_MSQ_ER_MeetWiseman_Puzzle`. The plan strikes each gong; a struck gong is no
    /// longer open to strikes.
    #[test]
    fn the_gong_puzzle_is_planned_as_strikes_and_ends_with_the_third_gong() {
        let gong = |x: f32, y: f32| Position { x, y, z: 188.34 };
        let gongs = [
            ("99F_MSQ_02_Puzzle1", gong(8267.0, 6713.41)),
            ("99F_MSQ_02_Puzzle2", gong(8288.1, 6693.65)),
            ("99F_MSQ_02_Puzzle3", gong(8263.46, 6684.7)),
        ];
        let mut world = puzzle_world("PuzzleMSQGong", &[], &gongs);
        assert_eq!(
            world.quest_plan_lines(1, PUZZLE_QUEST),
            [
                "plan z_puzzle strike 99F_MSQ_02_Puzzle1 at 8267.0 6713.4 188.3 \
              99F_MSQ_02_Puzzle2 at 8288.1 6693.6 188.3 \
              99F_MSQ_02_Puzzle3 at 8263.5 6684.7 188.3"
                    .to_owned()
            ]
        );
        assert_eq!(world.open_strike_objects(1).len(), gongs.len());
        for (gatherable, _) in [gongs[1], gongs[0]] {
            world.puzzle_struck(1, gatherable, NOW + 1);
        }
        assert_eq!(
            world.open_strike_objects(1),
            [gongs[2].0],
            "the gong not struck"
        );
        world.solve_puzzles_due(NOW + 2);
        assert!(!world.walk_finished(1, PUZZLE_QUEST), "two gongs");
        world.puzzle_struck(1, gongs[2].0, NOW + 3);
        assert!(world.walk_finished(1, PUZZLE_QUEST));
    }

    /// Retail `99g_msq_sm_attackmentally`: each altar's gather gives its achievement, and
    /// `CompletePuzzle` ends 15.07 s after the third. The achievement steps are planned as the
    /// altars' gathers, the slayer script step waits; the altars open to the player.
    #[test]
    fn the_altar_puzzle_gives_each_achievement_and_ends_after_the_third_altar() {
        const ALTARS: [&str; 3] = [
            "99G_MSQ_SM_AttackMentally_BroochAltar",
            "99G_MSQ_SM_AttackMentally_LogAltar",
            "99G_MSQ_SM_AttackMentally_CupAltar",
        ];
        let altar_at = Position {
            x: 9395.79,
            y: 8993.15,
            z: 645.97,
        };
        let mut world = puzzle_world(
            "PuzzleMSQAttackMentally",
            &["99G_MSQ_SM_AttackMentally_Altar_A"],
            &[(ALTARS[0], altar_at)],
        );
        let lines = world.quest_plan_lines(1, PUZZLE_QUEST);
        assert!(
            lines.contains(&"plan z_puzzle wait".to_owned()),
            "{lines:?}"
        );
        assert!(
            lines.contains(&format!("plan z_altar_0 gather {} 1", ALTARS[0])),
            "{lines:?}"
        );
        assert!(world.puzzle_opens(1, lowercase_crc32(ALTARS[1])));
        assert!(!world.puzzle_opens(1, lowercase_crc32("99F_MSQ_02_Puzzle1")));
        world.puzzle_gathered(1, ALTARS[0], NOW);
        assert!(
            !world
                .quest_plan_lines(1, PUZZLE_QUEST)
                .iter()
                .any(|line| line.starts_with("plan z_altar_0")),
            "the altar's achievement step ended"
        );
        world.puzzle_gathered(1, ALTARS[2], NOW + 1);
        let third = NOW + 2;
        world.puzzle_gathered(1, ALTARS[1], third);
        world.solve_puzzles_due(third + super::super::puzzles::PUZZLES[1].solved_after_ms - 1);
        assert!(!world.walk_finished(1, PUZZLE_QUEST), "the 15 s run");
        world.solve_puzzles_due(third + super::super::puzzles::PUZZLES[1].solved_after_ms);
        assert!(world.walk_finished(1, PUZZLE_QUEST));
    }

    /// The fight at an open world encounter stays in its volume (`YonasRetreat` reset when the
    /// bot ran 50 m out of its 30 m sphere): the kill plan names the volume's radius.
    #[test]
    fn a_kill_at_an_open_world_encounter_names_the_volume_radius() {
        let world = encounter_world(true, true, SPAWN_X);
        let kill = |encounter: Option<&str>| StepPlan::Kill {
            spawn: encounter_spawn(),
            count: 1,
            territory: None,
            spawned: false,
            encounter: encounter.map(str::to_owned),
        };
        let in_volume = world.plan_what(&kill(Some(ENCOUNTER_SLICE)));
        assert!(
            in_volume.ends_with(" placed within 30.0"),
            "the encounter's volume radius: {in_volume}"
        );
        let plain = world.plan_what(&kill(None));
        assert!(plain.ends_with(" placed"), "a mob of no encounter: {plain}");
    }

    #[test]
    fn an_encounter_that_starts_by_itself_near_the_spawn_spawns_its_mob() {
        let world = encounter_world(true, true, SPAWN_X);
        assert_eq!(
            world.spawning_encounter(&encounter_spawn()),
            Some(ENCOUNTER_SLICE.to_owned())
        );
        assert_eq!(
            world.spawning_encounter(&test_spawn(SPAWN_X)),
            None,
            "a placed spawn is in no encounter's group"
        );
        let other_mob = VitalsSpawn {
            vitals_id: lowercase_crc32("Walk_Wolf"),
            ..encounter_spawn()
        };
        assert_eq!(
            world.spawning_encounter(&other_mob),
            None,
            "no stage spawns that mob"
        );
    }

    #[test]
    fn no_encounter_spawns_the_mob_when_it_is_not_run_far_or_waits_for_a_start() {
        assert_eq!(
            encounter_world(false, true, SPAWN_X).spawning_encounter(&encounter_spawn()),
            None,
            "this sim does not run the open world's encounters"
        );
        assert_eq!(
            encounter_world(true, true, SPAWN_X + FAR_M).spawning_encounter(&encounter_spawn()),
            None,
            "the spawn is outside the encounter's group"
        );
        assert_eq!(
            encounter_world(true, false, SPAWN_X).spawning_encounter(&encounter_spawn()),
            None,
            "the encounter waits for a script to start it"
        );
    }

    /// Retail `1124_0101_enpleinaire` 146.1 s: the client gathered (kind 14) the object of
    /// `IO_1124_VistaPoint1`, gatherable `1124_VistaPoint1`; the plan names that gather.
    #[test]
    fn an_interact_of_a_tagged_gatherable_is_its_gather() {
        const TAG: &str = "IO_1124_VistaPoint1";
        const VISTA: &str = "1124_VistaPoint1";
        const TASK: &str = "1124_0101_02_TravelTask";
        let (mut world, level) = walk_world();
        let mut content = (*world.content).clone();
        content.interact_tags = nw_content::interact_tags::InteractTags::default()
            .with_gatherables([(
                lowercase_crc32(VISTA),
                nw_content::interact_tags::VariantTag {
                    tag: TAG.to_owned(),
                    only_on_quest: true,
                },
            )]);
        content.gatherables =
            nw_content::gathering::Gatherables::build([nw_content::gathering::Gatherable {
                id: VISTA.to_owned(),
                gather_ms: 0,
                respawn_ms: (0, 0),
                loot_table: None,
                loot_container: true,
                tradeskill: None,
                tool_required: false,
                by_hand: true,
                game_event: None,
                animation: None,
            }]);
        content.gather_points = PlacedPoints::of([(lowercase_crc32(VISTA), at(CELL_M / 2.0))]);
        world.content = Arc::new(content);
        let step = WaitingStep {
            quest: "1124_0101_EnPleinAire".to_owned(),
            task: TASK.to_owned(),
            kind: TaskKind::Interact {
                tag: TAG.to_owned(),
                count: 1,
            },
            progress: 0,
            container_item: false,
            game_mode: None,
        };
        let plan = world.walk_plan(&level, &step, at(CELL_M / 2.0));
        assert_eq!(
            plan,
            StepPlan::Gather {
                gatherable: VISTA.to_owned(),
                count: 1,
                again: false,
                event: Box::new(StepPlan::Event(FlowEvent::Interacted(TAG.to_owned()), 1)),
            }
        );
    }

    /// Retail `1127_0102_makinbacon`: the pig corpses were the gatherable `PigDomestic` (112.0
    /// to 127.2 s), which `slices/characters/pig_domestic.dynamicslice` names and no kill
    /// category of the pig names.
    #[test]
    fn a_skin_step_takes_the_gatherable_the_creature_slice_names() {
        const PIG_GATHERABLE: &str = "PigDomestic";
        const PIG_ASSET: [u8; nw_content::mob_prefabs::ASSET_ID_LEN] =
            [9; nw_content::mob_prefabs::ASSET_ID_LEN];
        let (mut world, _) = walk_world();
        let mut content = (*world.content).clone();
        content.kill_types = KillTypes::build([(TEST_MOB.to_owned(), "Hostile".to_owned())]);
        content.gatherables =
            nw_content::gathering::Gatherables::build([nw_content::gathering::Gatherable {
                id: PIG_GATHERABLE.to_owned(),
                gather_ms: 0,
                respawn_ms: (0, 0),
                loot_table: None,
                loot_container: false,
                tradeskill: Some(super::super::gathering::SKINNING_TRADESKILL.to_owned()),
                tool_required: false,
                by_hand: false,
                game_event: None,
                animation: None,
            }]);
        let prefab = format!(
            r#"[{{"vitals_id":{},"vitals_name":null,"asset":"{}","sub_id":2,
                "alc_component":5,"vitals_component":9,"first_members":[],"death_members":[],
                "seen":1}}]"#,
            lowercase_crc32(TEST_MOB),
            hex::encode(PIG_ASSET)
        );
        let prefabs = nw_content::mob_prefabs::MobPrefabs::parse(&prefab).expect("table");
        content.mob_looks = crate::player_content::MobLooks::of(&prefabs).0;
        content.creature_animations =
            nw_content::creature_animations::CreatureAnimations::of_rows(vec![
                nw_content::creature_animations::CreatureRow {
                    asset: hex::encode(PIG_ASSET),
                    slice: "slices/characters/pig_domestic.dynamicslice".to_owned(),
                    fragments: Vec::new(),
                    scopes: Vec::new(),
                    actions: Vec::new(),
                    tags: std::collections::BTreeMap::new(),
                    default_tags: Vec::new(),
                    fragment_ticks: std::collections::BTreeMap::new(),
                    reaction_paces: std::collections::BTreeMap::new(),
                    collider_radius_mm: None,
                    skinnable: true,
                    skin_gatherable: Some(PIG_GATHERABLE.to_owned()),
                    spawned_vitals: Vec::new(),
                    weapon_animation_database: None,
                },
            ])
            .expect("creatures");
        world.content = Arc::new(content);
        world
            .admit(
                &super::super::tests::admit_at(1, CELL_M / 2.0, true),
                NOW,
                NOW,
            )
            .expect("admit");
        world.start_quest(1, SKIN_MOB, NOW).expect("start");
        assert_eq!(
            world.quest_plan_lines(1, SKIN_MOB),
            [format!(
                "plan m_get skin {PIG_GATHERABLE} 1 kill {TEST_MOB} 1 at 1054.0 10.0 50.0"
            )]
        );
    }

    /// Client rows of `1605_0501_miraclesmadness`: `HaveAndReturn_3_CharmionsJournal` drops
    /// nothing itself; its sibling journal tasks drop the journals from containers.
    #[test]
    fn an_item_that_a_task_of_the_quest_drops_from_containers_is_a_container_drop() {
        const JOURNAL: &str = "1651_0101_CharmionsJournal";
        const COUNT: u32 = 3;
        let (world, level) = walk_world();
        let step = |container_item| WaitingStep {
            quest: "1605_0501_miraclesmadness".to_owned(),
            task: "1605_0501_d_02_02_01_HaveAndReturn_3_CharmionsJournal".to_owned(),
            kind: TaskKind::HaveItems {
                item: JOURNAL.to_owned(),
                count: COUNT,
                returned: true,
                drop: None,
                chest: None,
            },
            progress: 0,
            container_item,
            game_mode: None,
        };
        assert_eq!(
            world.walk_plan(&level, &step(true), at(CELL_M / 2.0)),
            StepPlan::Item {
                item: JOURNAL.to_owned(),
                count: COUNT,
                gathered: false,
                source: "a container drop".to_owned(),
                node: None,
            }
        );
        assert!(matches!(
            world.walk_plan(&level, &step(false), at(CELL_M / 2.0)),
            StepPlan::Stop(Outcome::Broken, _)
        ));
    }

    /// Retail `04a_side04_06_getflowers`: the flowers step names its interact tag, no
    /// `ItemDropVC`; five gathers of the tagged flowers gave its five flowers.
    #[test]
    fn an_item_that_drops_from_a_tag_is_gathered_from_the_placed_gatherable_of_the_tag() {
        const TAG: &str = "IO_04A_Side04_01_Flowers";
        const FLOWERS: &str = "04A_Side04_01_Flowers";
        const ITEM: &str = "04aSide04_06_Flowers";
        const COUNT: u32 = 5;
        let (mut world, level) = walk_world();
        let mut content = (*world.content).clone();
        content.interact_tags = nw_content::interact_tags::InteractTags::default()
            .with_gatherables([(
                lowercase_crc32(FLOWERS),
                nw_content::interact_tags::VariantTag {
                    tag: TAG.to_owned(),
                    only_on_quest: false,
                },
            )]);
        content.gatherables =
            nw_content::gathering::Gatherables::build([nw_content::gathering::Gatherable {
                id: FLOWERS.to_owned(),
                gather_ms: 0,
                respawn_ms: (0, 0),
                loot_table: None,
                loot_container: true,
                tradeskill: None,
                tool_required: false,
                by_hand: true,
                game_event: None,
                animation: None,
            }]);
        content.gather_points = PlacedPoints::of([(lowercase_crc32(FLOWERS), at(CELL_M / 2.0))]);
        world.content = Arc::new(content);
        let step = WaitingStep {
            quest: "04A_Side04_06_GetFlowers".to_owned(),
            task: "04A_Side04_06_02_Task_RetrieveFlowersFromIO".to_owned(),
            kind: TaskKind::HaveItems {
                item: ITEM.to_owned(),
                count: COUNT,
                returned: true,
                drop: Some(crate::objectives::QuestDrop {
                    sources: vec![TAG.to_owned()],
                    percent: 100,
                    place: None,
                }),
                chest: None,
            },
            progress: 0,
            container_item: false,
            game_mode: None,
        };
        assert_eq!(
            world.walk_plan(&level, &step, at(CELL_M / 2.0)),
            StepPlan::Item {
                item: ITEM.to_owned(),
                count: COUNT,
                gathered: false,
                source: format!("the gather of quest object {FLOWERS}"),
                node: Some(FLOWERS.to_owned()),
            }
        );
    }

    /// Retail `04a_side05_02_playfortown`: `04A_Side05_02_00E_PracticeSong1` (`GiftEasy`, its
    /// one sheet on the flute) was played after the flute went into slot 39.
    #[test]
    fn a_song_step_plays_a_sheet_of_a_worn_instrument_after_its_give_and_equip() {
        use nw_content::generated::{MusicalInstrumentSlot, SongBookData, SongBookSheets, Tables};
        use nw_content::item_rolls::{ItemRollRow, ItemRolls};
        use nw_content::music::MusicRules;
        use nw_content::perks::PerkTable;
        use nw_content::table::Table;
        const SONG: &str = "GiftEasy";
        const SHEET: &str = "Gift_AzothFlute_Part1_Easy";
        const FLUTE: &str = "2hInstrumentFluteT1";
        const FLUTE_SLOT: u32 = 39;
        fn table<T>(rows: Vec<T>) -> Table<T> {
            Table {
                source_path: String::new(),
                unique_id: String::new(),
                rows,
            }
        }
        let song: SongBookData = serde_json::from_value(serde_json::json!({
            "completion_game_event": "SongCompletedEasy", "crowd": "Gift",
            "dance_timeline": "Performance_Dance", "duration": 60.888,
            "filter_type": "DefaultPerformance", "highest_rank_achievement_id": "MP_Perfect_GiftEasy",
            "hit_zone": 0.2222222, "music_title": "@songtitle_Gift", "note_pattern": "Novice",
            "perfect_zone": 0.08333334, "performance": "Easy", "scroll_speed": 2.5,
            "slot01": SHEET, "song_id": SONG, "sound_bank": "MXMI_Gift", "tempo": 100
        }))
        .expect("song row");
        let sheet: SongBookSheets = serde_json::from_value(serde_json::json!({
            "audio_event": "AZF_GIFT_100", "error_event": "AZF_MISTAKE",
            "instrument": "instrument-flute-slot",
            "pages": "MP_Gift_AzothFlute_Part1_Page1,MP_Gift_AzothFlute_Part1_Page2",
            "sheet_id": SHEET, "timeline": "AZF_GIFT_100"
        }))
        .expect("sheet row");
        let slot: MusicalInstrumentSlot = serde_json::from_value(serde_json::json!({
            "clef_icon_path": "", "name": "@ui_instrument-flute-slot_name",
            "paperdoll_id": "instrument-flute-slot"
        }))
        .expect("slot row");
        let (mut world, level) = walk_world();
        let mut content = (*world.content).clone();
        content.music = MusicRules::from_tables(&Tables {
            song_book_data: vec![table(vec![song])],
            song_book_sheets: vec![table(vec![sheet])],
            musical_instrument_slot: vec![table(vec![slot])],
            ..Tables::default()
        });
        content.kits =
            super::super::gear::tests::kits_with(&[(FLUTE, "EquippableTool+InstrumentFlute")]);
        content.item_rolls = ItemRolls::build(
            [ItemRollRow {
                item_id: FLUTE.to_owned(),
                classes: vec!["EquippableTool".to_owned(), "InstrumentFlute".to_owned()],
                tier: 1,
                ..ItemRollRow::default()
            }],
            PerkTable::default(),
        );
        world.content = Arc::new(content);
        let mut admit = super::super::tests::admit_at(1, CELL_M / 2.0, true);
        admit.backstory = Some(super::super::gear::tests::BACKSTORY.to_owned());
        world.admit(&admit, NOW, NOW).expect("admit");
        let entity = world.player_by_number(1).expect("player");
        let place = at(CELL_M / 2.0);
        let step = WaitingStep {
            quest: "04A_Side05_02_PlayForTown".to_owned(),
            task: "04A_Side05_02_00E_PracticeSong1".to_owned(),
            kind: TaskKind::PerformSong {
                song: SONG.to_owned(),
                near: None,
                count: 1,
            },
            progress: 0,
            container_item: false,
            game_mode: None,
        };
        let plan = |world: &SimWorld| {
            let plan = world.walk_plan(&level, &step, place);
            world.song_for_event(Some(entity), plan)
        };
        assert!(
            matches!(plan(&world), StepPlan::Item { ref item, .. } if item == FLUTE),
            "no flute: its give"
        );
        let flute = world.content.kits.item(FLUTE).cloned().expect("flute");
        world
            .world
            .get_mut::<Player>(entity)
            .and_then(|mut player| player.gear.as_mut().map(|gear| gear.add(&flute, 1)))
            .expect("gear");
        assert!(
            matches!(plan(&world), StepPlan::Equip { slot, .. } if slot == FLUTE_SLOT),
            "the flute in the bag: its equip into the flute slot"
        );
        world
            .world
            .get_mut::<Player>(entity)
            .and_then(|mut player| {
                player
                    .gear
                    .as_mut()
                    .map(|gear| gear.renew(&flute, 1, FLUTE_SLOT, 0))
            })
            .expect("gear");
        assert_eq!(
            world.plan_what(&plan(&world)),
            format!(
                "song {SONG} sheet {SHEET} seconds 60.888 at {:.1} 10.0 50.0",
                CELL_M / 2.0
            )
        );
    }

    /// `09A_S0_UsePotion` (`TaskUseItems`) drinks the crafted `09A_S0_SporeElixir`: the use of
    /// its bag slot once the bag holds it.
    #[test]
    fn a_use_step_of_a_bag_item_is_the_use_of_its_bag_slot() {
        const ELIXIR: &str = "09A_S0_SporeElixir";
        let (mut world, _) = walk_world();
        let mut content = (*world.content).clone();
        content.kits = super::super::gear::tests::kits_with(&[(ELIXIR, "Consumable")]);
        world.content = Arc::new(content);
        let mut admit = super::super::tests::admit_at(1, CELL_M / 2.0, true);
        admit.backstory = Some(super::super::gear::tests::BACKSTORY.to_owned());
        world.admit(&admit, NOW, NOW).expect("admit");
        let entity = world.player_by_number(1).expect("player");
        let used = || StepPlan::Event(FlowEvent::Used(ELIXIR.to_owned()), 1);
        assert_eq!(
            world.use_from_bag(Some(entity), used()),
            used(),
            "not in the bag"
        );
        let elixir = world.content.kits.item(ELIXIR).cloned().expect("elixir");
        world
            .world
            .get_mut::<Player>(entity)
            .and_then(|mut player| player.gear.as_mut().map(|gear| gear.add(&elixir, 1)))
            .expect("gear");
        let bag_slot = world
            .world
            .get::<Player>(entity)
            .and_then(|player| player.gear.as_ref()?.bag_slot_of(ELIXIR))
            .expect("in the bag");
        assert_eq!(
            world.plan_what(&world.use_from_bag(Some(entity), used())),
            format!("useitem {ELIXIR} bag {bag_slot}")
        );
    }

    /// `C10A_Q6_KillPlayers` waits for 3 of game event `PlayerKill`: its plan is player kills.
    #[test]
    fn a_player_kill_step_is_the_kill_of_pvp_foes() {
        let (world, level) = walk_world();
        let step = WaitingStep {
            quest: "C10A_Q6_Citadel".to_owned(),
            task: "C10A_Q6_KillPlayers".to_owned(),
            kind: TaskKind::GameEvent {
                event: super::super::attack::PLAYER_KILL_EVENT.to_owned(),
                count: 3,
            },
            progress: 1,
            container_item: false,
            game_mode: None,
        };
        let plan = pvp_for_event(world.walk_plan(&level, &step, at(CELL_M / 2.0)));
        assert_eq!(world.plan_what(&plan), "pvp 2");
        assert_eq!(
            pvp_for_event(StepPlan::Event(FlowEvent::GameEvent("Other".to_owned()), 1)),
            StepPlan::Event(FlowEvent::GameEvent("Other".to_owned()), 1)
        );
    }

    /// Retail `06a_ts_02_forgesword` 79.0 s: the family step `SwordAbilityTable` (item
    /// `1hSwordT2`) was the craft of `Procedural_LongswordT2`.
    #[test]
    fn a_family_craft_step_crafts_the_recipe_of_its_item_at_a_station_that_makes_it() {
        use nw_content::crafting::{Ingredient, IngredientKind, Recipe, Recipes};
        use nw_content::item_rolls::{ItemRollRow, ItemRolls};
        use nw_content::perks::PerkTable;
        const FAMILY: &str = "SwordAbilityTable";
        const SWORD: &str = "1hSwordT2";
        const OTHER_SWORD: &str = "1hSwordT1";
        const RECIPE: &str = "Procedural_LongswordT2";
        const ANVIL: &str = "blacksmith2";
        const INGOT: &str = "IngotT2";
        const INGOTS: u32 = 9;
        let (mut world, _) = walk_world();
        let mut content = (*world.content).clone();
        let recipe = |id: &str, item: &str| Recipe {
            id: id.to_owned(),
            item: item.to_owned(),
            quantity: 1,
            ingredients: vec![Ingredient {
                kind: IngredientKind::Item(INGOT.to_owned()),
                quantity: INGOTS,
            }],
            stations: vec![ANVIL.to_owned()],
        };
        content.recipes = Recipes::build(
            [
                recipe("A_Procedural_LongswordT1", OTHER_SWORD),
                recipe(RECIPE, SWORD),
            ],
            [],
        );
        let sword_row = |item: &str| ItemRollRow {
            item_id: item.to_owned(),
            classes: vec![FAMILY.to_owned()],
            ..ItemRollRow::default()
        };
        content.item_rolls = ItemRolls::build(
            [sword_row(SWORD), sword_row(OTHER_SWORD)],
            PerkTable::default(),
        );
        world.content = Arc::new(content);
        world
            .stations
            .add_level_entity(super::super::crafting::Station {
                gde_ref: [2; nw_messages::wire::RAW_ID_LEN],
                position: at(CELL_M / 2.0),
                station_type: ANVIL.to_owned(),
                name: Vec::new(),
                members: Vec::new(),
            });
        let family = |item: Option<&str>| {
            StepPlan::Event(
                FlowEvent::CraftedFamily {
                    family: FAMILY.to_owned(),
                    item: item.map(str::to_owned),
                },
                1,
            )
        };
        let crafts = |plan: StepPlan| match world.craft_for_event(None, at(CELL_M / 2.0), plan) {
            StepPlan::Craft { recipe, .. } => Some(recipe),
            _ => None,
        };
        assert_eq!(crafts(family(Some(SWORD))).as_deref(), Some(RECIPE));
        assert_eq!(
            crafts(family(None)).as_deref(),
            Some("A_Procedural_LongswordT1"),
            "without an item, the first recipe by id"
        );
        assert_eq!(
            crafts(StepPlan::Event(
                FlowEvent::CraftedFamily {
                    family: "BowAbilityTable".to_owned(),
                    item: None,
                },
                1
            )),
            None,
            "no recipe makes an item of another family"
        );
    }

    /// Live 2026-11-22 07:05:31: the plan sent the bot to `DyePlantViolet` for `WoodT1`, a
    /// table that holds wood its rolls never gave; the plan names the gatherable whose rolls give
    /// the item, of those a player gathers without a rank or an item first.
    #[test]
    fn an_item_is_gathered_from_the_gatherable_whose_rolls_give_it() {
        use nw_content::loot::{LootRow, LootSource, LootTables};
        const WOOD: &str = "WoodT1";
        const PLANT: &str = "Test_Plant";
        const TREE: &str = "Test_Tree";
        const MAX_ROLL: i64 = 100;
        const ALWAYS: &str = "0";
        const NEVER: &str = "101";
        let row = |id: &str, cell: &str, and_or: Option<&str>, max_roll: Option<i64>| LootRow {
            id: id.to_owned(),
            and_or: and_or.map(str::to_owned),
            max_roll,
            cells: [(1, cell.to_owned())].into(),
            ..LootRow::default()
        };
        let table = |id: &str, chance: &str| {
            [
                row(id, WOOD, Some("AND"), None),
                row(&format!("{id}_Qty"), "1", None, None),
                row(&format!("{id}_Probs"), chance, None, Some(MAX_ROLL)),
            ]
        };
        let gatherable = |id: &str| nw_content::gathering::Gatherable {
            id: id.to_owned(),
            gather_ms: 0,
            respawn_ms: (0, 0),
            loot_table: Some(id.to_owned()),
            loot_container: false,
            tradeskill: None,
            tool_required: false,
            by_hand: true,
            game_event: None,
            animation: None,
        };
        let (mut world, _) = walk_world();
        let mut content = (*world.content).clone();
        content.loot = LootTables::build(LootSource {
            rows: table(PLANT, NEVER)
                .into_iter()
                .chain(table(TREE, ALWAYS))
                .collect(),
            bucket_items: Vec::new(),
        });
        content.gatherables =
            nw_content::gathering::Gatherables::build([gatherable(PLANT), gatherable(TREE)]);
        content.gather_points = PlacedPoints::of([
            (lowercase_crc32(PLANT), at(CELL_M / 2.0)),
            (lowercase_crc32(TREE), at(CELL_M / 2.0 + 1.0)),
        ]);
        world.content = Arc::new(content.clone());
        assert_eq!(world.roll_hits(PLANT, WOOD), 0);
        assert_eq!(world.roll_hits(TREE, WOOD), SOURCE_ROLLS);
        let source = world
            .walk_item_source(&QuestGuide::default(), WOOD, None, false)
            .expect("a source");
        assert_eq!(source.node.as_deref(), Some(TREE), "{}", source.said);
        // A tree that asks a logging rank comes after a plant that asks none.
        content.loot = LootTables::build(LootSource {
            rows: table(PLANT, ALWAYS)
                .into_iter()
                .chain(table(TREE, ALWAYS))
                .collect(),
            bucket_items: Vec::new(),
        });
        content.gather_rules = nw_content::gathering::GatherRules::build([(
            TREE.to_owned(),
            nw_content::gathering::GatherRule {
                required_rank: 1,
                required_item: None,
            },
        )]);
        world.content = Arc::new(content);
        let source = world
            .walk_item_source(&QuestGuide::default(), WOOD, None, false)
            .expect("a source");
        assert_eq!(source.node.as_deref(), Some(PLANT), "{}", source.said);
    }

    /// `12_boarfeast_05_01_TaskGameEvent_1_Craft_CookingT1`: retail cooked at the level campfire
    /// with the pork the player carried (`12-3_boarfeast-Campfire-Feast`, 97.0 to 106.7 s). An
    /// ingredient the bag lacks is named as one to get first (live 2026-11-22 07:02:06
    /// `12A_Side_16_ToGoodUse_CraftSword`: `craft refused ... refusal=Ingredient` when the plan
    /// named no timber or leather).
    #[test]
    fn a_craft_step_is_the_craft_at_the_nearest_station_that_makes_it_from_the_bag() {
        use nw_content::crafting::{Ingredient, IngredientKind, Recipe, Recipes};
        const RECIPE: &str = "Procedural_HealthRecoveryT1";
        const PORK: &str = "PorkT1";
        const MEAT: &str = "FoodIngredientT1";
        const SPICE: &str = "SpiceT1";
        const SPICES: &str = "Spices";
        const SPICE_QUANTITY: u32 = 2;
        const CAMPFIRE: &str = "campfire1";
        const FAR_M: f32 = 500.0;
        const NEAR_M: f32 = 20.0;
        const NEAREST_M: f32 = 5.0;
        let (mut world, _) = walk_world();
        let mut content = (*world.content).clone();
        content.kits = super::super::gear::tests::kits_with(&[(PORK, "Resource+Food")]);
        content.recipes = Recipes::build(
            [Recipe {
                id: RECIPE.to_owned(),
                item: "FoodHealthRecoveryT1".to_owned(),
                quantity: 1,
                ingredients: vec![
                    Ingredient {
                        kind: IngredientKind::Category(MEAT.to_owned()),
                        quantity: 1,
                    },
                    Ingredient {
                        kind: IngredientKind::Category(SPICES.to_owned()),
                        quantity: SPICE_QUANTITY,
                    },
                ],
                stations: vec![CAMPFIRE.to_owned()],
            }],
            [
                (PORK.to_owned(), MEAT.to_owned()),
                (SPICE.to_owned(), SPICES.to_owned()),
            ],
        );
        world.content = Arc::new(content);
        let mut admit = super::super::tests::admit_at(1, CELL_M / 2.0, true);
        admit.backstory = Some(super::super::gear::tests::BACKSTORY.to_owned());
        world.admit(&admit, NOW, NOW).expect("admit");
        let station = |dx: f32, station_type: &str, id: u8| super::super::crafting::Station {
            gde_ref: [id; nw_messages::wire::RAW_ID_LEN],
            position: at(CELL_M / 2.0 + dx),
            station_type: station_type.to_owned(),
            name: Vec::new(),
            members: Vec::new(),
        };
        const NEAR_ID: u8 = 2;
        world.stations.add_level_entity(station(FAR_M, CAMPFIRE, 1));
        world
            .stations
            .add_level_entity(station(NEAR_M, CAMPFIRE, NEAR_ID));
        world
            .stations
            .add_level_entity(station(NEAREST_M, "smelting1", 3));
        let entity = world.player_by_number(1).expect("player");
        let pork = world.content.kits.item(PORK).cloned().expect("pork");
        world
            .world
            .get_mut::<super::super::Player>(entity)
            .and_then(|mut player| player.gear.as_mut().map(|gear| gear.add(&pork, 1)))
            .expect("gear");
        let crafted = || StepPlan::Event(FlowEvent::Crafted(RECIPE.to_owned()), 1);
        let plan = world.craft_for_event(Some(entity), at(CELL_M / 2.0), crafted());
        assert_eq!(
            world.plan_what(&plan),
            format!(
                "craft {RECIPE} at {:.1} 10.0 50.0 station {} with {PORK} 1 with {} {SPICE_QUANTITY} give {} {SPICE_QUANTITY}",
                CELL_M / 2.0 + NEAR_M,
                hex(&[NEAR_ID; nw_messages::wire::RAW_ID_LEN]),
                SPICE.to_lowercase(),
                SPICE.to_lowercase()
            ),
            "the nearest campfire, not the nearer smelter, with the carried pork and the spice to get"
        );
        assert_eq!(
            world.craft_for_event(Some(entity), at(CELL_M / 2.0), StepPlan::Wait),
            StepPlan::Wait
        );
    }

    #[test]
    fn an_expression_names_each_achievement_and_its_negation() {
        assert_eq!(
            expression_names("(A && !B) || C"),
            [
                ("A".to_owned(), false),
                ("B".to_owned(), true),
                ("C".to_owned(), false)
            ]
        );
    }
}
