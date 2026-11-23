// SPDX-License-Identifier: AGPL-3.0-or-later
//! `nw-testclient play`: one bot of a game master account plays quests on the live servers
//! through the calls a game client sends, and reports each quest: completed, broken (the step
//! and why), or not played (a step whose flow the bot does not play yet).
//!
//! For each quest the bot asks the sim where the quest stands (`!queststate`) and what its
//! waiting steps need (`!questplan`, `sim::world::quest_walk`). It takes the quest with the
//! client's accept (3772) and turns it in with the client's turn-in (794); it talks to NPCs with
//! a talk interaction in its input, finishes conversation topics (5397), tells the server the
//! territories it enters and leaves (2297, 6416) and sends the cinematic, cutscene, screen and
//! faction calls a client sends. It kills a step's mobs with attack inputs: the placed mobs at
//! the step's spawn when only those count (by their spawn group), else mobs of the step's type
//! that a game master spawns there. It gathers the nodes of a quest object or a gatherable that
//! gives a step's item, with a client's gather interaction. Game master lines move it (`!tp`),
//! keep it alive (`!god on`), and give it the items no gather gives (`!give`), with the source
//! the sim names.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use nw_carrier::MESSAGE_CHANNEL;
use nw_content::poi_tags::PoiTags;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::client_input::{TALK_KIND, TRIGGER_KIND};
use nw_messages::entity_ids::{facet_of_raw_id, raw_id_of_entity};
use nw_messages::fishing::{FISHING_FACET, FishingClientMsg};
use nw_messages::item_calls::ItemCallMsg;
use nw_messages::social::SocialFacet;
use nw_messages::social::chat::{ChatCall, ChatChannel};
use nw_messages::{
    AfkWarningMsg, Bit8, ClientFacetMsg, ClientInputMsg, ConversationClientMsg, FacetHeader,
    InputPosition, InteractionRequest, ServerMessage,
};
use nw_reflect::LoginToken;
use nw_sim_core::{Position, yaw_toward};
use reqwest::Client;

use crate::activity::{
    Act, Activity, ActivityKind, BotSight, CLEAR_RADIUS_M, CourseRoutes, MatchEnd, MatchGoal,
    OPEN_STORAGE_KIND, OPEN_STORAGE_NAME, PVP_KILLER_FACTION, Proof, STORAGE_TAG, Step,
    accept_calls, arena_verdict, camp_steps, course_steps, door_trial_steps, gather_steps,
    leave_trial_steps, loot_steps, lore_steps, mission_turn_in_steps, mount_steps, pop_to_accept,
    pvp_flag_shown, pvp_flag_steps, quest_craft_steps, skin_steps, song_steps, start_object_steps,
    trial_steps, use_steps,
};
use crate::chat::{chat_lines, is_system};
use crate::fishing::{Angler, FishAct, FishEnd};
use crate::gather::GatherNode;
use crate::group::{GroupSeat, accept_call, invite_call};
use crate::hunt::{MAX_SWINGS_WITHOUT_HIT, planar, send_input};
use crate::pair::{PairError, RecordWalk, SEND_INTERVAL, session_with};
use crate::record_walk::{ARRIVED_M, RecordWalker};
use crate::rep::{Received, RepClient, RepError};
use crate::udp::RepOutcome;
use crate::view::{CombatView, InteractionAnswer, WorldView};
use crate::walk::{WalkSettings, login_token, walk};
use nw_messages::game_mode::MATCHMAKING_LOGIN_REQUESTED_TYPE_INDEX;
use nw_messages::social::groups::SIEGE_WARFARE_ENDED_TYPE_INDEX;

/// The game master lines after the tutorial's exit, before the first quest: the bot does not
/// die, lives again when a mob killed it before (a dead player wakes no mob), is at the top
/// level (the sim holds it to the game data's highest), and wears the best item of each kind at
/// its top gear score, so its level and gear never limit a kill (a starter sword hit a level 34
/// bear of 8,776 health for 42). It carries the tier 5 gathering tools, as a player at the top
/// level does, which `!gearup` puts into their tool slots from the bag (a quest gather of a
/// player with no pick is refused: `99E_MSQ_02_MyrddinCrystalsA`, `NoTool`).
///
/// [`SIM_LINES`] come first; the others ([`SAVED_SETUP_LINES`]) change what the server saves
/// with the character.
const SIM_LINES: [&str; 2] = ["!god on", "!heal"];
/// The setup lines whose effect the server saves with the character: they hold in every sim.
const SAVED_SETUP_LINES: [&str; 7] = [
    "!level 65",
    "!give 2hPickT5 1",
    "!give 2hAxeT5 1",
    "!give 1hSickleT5 1",
    "!give 1hSkinningKnifeT5 1",
    "!give 2hFishingPoleT5 1",
    "!gearup",
];
/// The game master lines the bot sends at its setup: [`SIM_LINES`], [`SAVED_SETUP_LINES`],
/// then a start of each quest of `gm_start` ([`PlaySettings::gm_start`]).
fn setup_lines(gm_start: &[String]) -> Vec<String> {
    SIM_LINES
        .iter()
        .chain(&SAVED_SETUP_LINES)
        .map(|line| (*line).to_owned())
        .chain(gm_start.iter().map(|quest| format!("{GM_QUEST} {quest}")))
        .collect()
}

/// Whether `received` is the server's AFK warning (2139 with its flag set), not the message that
/// takes the warning back.
fn is_afk_warning(received: &Received) -> bool {
    matches!(
        ServerMessage::from_envelope(&received.envelope),
        Some(Ok(ServerMessage::AfkWarning(AfkWarningMsg {
            warned: true,
            ..
        })))
    )
}

/// The phase after a gather that ended well: [`Phase::SimLines`] when the gather moved the bot
/// into another sim (`changed_sim`), as that sim knows nothing of the god mode of the last one.
fn phase_after_sim_change(changed_sim: bool) -> Option<Phase> {
    changed_sim.then_some(Phase::SimLines(0))
}

/// The game master line that starts a quest (`!quest <objective id>`).
const GM_QUEST: &str = "!quest";
/// The byte of the client's leave of the tutorial (0 in the capture).
const LEAVE_FLAG: u8 = 0;
/// Time a new character takes from the tutorial's leave into the open world (the leave, the
/// level load and the own card there).
const LEVEL_CHANGE_WAIT: Duration = Duration::from_secs(25);
/// Time from the own entity's first state to the first line: the client is in the world by then.
const FIRST_LINE_DELAY: Duration = Duration::from_secs(2);
/// Longest wait for the first line of an answer to a game master line.
const ANSWER_WAIT: Duration = Duration::from_secs(10);
/// Time after the first line of an answer in which its other lines come.
const ANSWER_SETTLE: Duration = Duration::from_millis(400);
/// Time after an action before the bot asks again where the quest stands.
const ACTION_PAUSE: Duration = Duration::from_millis(1500);
/// Time a step that waits (a timer, a patrol stay) gets before the bot asks again.
const WAIT_PAUSE: Duration = Duration::from_secs(5);
/// How far from the place of an `await` step the bot may stand and still wait there, metres;
/// farther, it goes there first.
const AWAIT_REACH_M: f32 = 3.0;
/// Longest fight for one kill step.
const FIGHT_TIME: Duration = Duration::from_secs(45);
/// How much longer a fight whose hits still land goes on ([`Fight::goes_on`]).
const FIGHT_EXTENSION: Duration = Duration::from_secs(15);
/// How far the bot steps aside when the server warns it that it stands still (2139): more than
/// the `ResetMinMove` of the retail `Expedition` AFK row, 1.0 m (live 2026-11-22 14:57:47: the
/// bot fought the Cutlass Keys admiral in one place for 5 minutes, and the AFK rule removed it).
const AFK_STEP_M: f32 = 2.0;
/// A mob of the kill this far from the bot is one it does not reach on foot: the game master bot
/// teleports to it (Inferred: the bot walks [`SPEED_MPS`]; a fleeing elk kept 10-12 m away).
const CHASE_TELEPORT_M: f32 = 6.0;
/// The shortest time between two teleports to a fleeing mob.
const CHASE_INTERVAL: Duration = Duration::from_secs(1);
/// Joins a plan line to its step's progress in the bot's same-plan count.
const PROGRESS_MARK: &str = " at progress ";
/// Separates the steps in a quest's step progress ([`quest_progress`]).
const STEP_PROGRESS_SEPARATOR: &str = "; ";
/// Time between two swings.
const SWING_INTERVAL: Duration = Duration::from_millis(600);
/// The bot swings at a mob this near, metres.
const REACH_M: f32 = 2.5;
/// Walking speed toward a mob, metres per second.
const SPEED_MPS: f32 = 4.0;
/// Time between two moves toward a mob.
const MOVE_INTERVAL: Duration = Duration::from_millis(100);
/// The level of the mobs the bot spawns for a kill step.
const SPAWN_LEVEL: u32 = 1;
/// Most rounds of one quest: a quest still running after them stops as broken.
const MAX_ROUNDS: u32 = 80;
/// The same plan line this many times in a row stops the quest as broken.
const MAX_SAME_PLAN: u32 = 3;
/// A step whose drop comes at a chance gets the tries that give the drop at least this often
/// ([`max_tries`]).
const DROP_CONFIDENCE: f64 = 0.95;
/// The drop chance of a sure drop, in percent.
const SURE_PERCENT: u32 = 100;
/// The word before a quest drop's chance in a step's state line (`percent: 40`).
const DROP_PERCENT_WORD: &str = "percent: ";
/// Most fights of one kill step while its hits land: a placed mob of a higher level takes
/// some 200 hits of a starter weapon (live 2026-11-06: 8776 health, 42 a hit).
const MAX_KILL_FIGHTS: u32 = 10;
/// The input records of one attack: the press and the release.
const ATTACK_RECORDS: u16 = 2;
/// The counter of the bot's first input record.
const FIRST_INPUT_COUNTER: u16 = 1;
/// A bot looks level.
const LEVEL_PITCH: f32 = 0.0;
/// The facet component of the bot's calls (Inferred: the sim reads the body only).
pub(crate) const CALL_COMPONENT: u64 = 0;
/// The first number of a salvage or open call (1733): 1 in all 173 retail calls (salvage note).
pub(crate) const SALVAGE_FIRST: u32 = 1;
/// The number of unknown meaning of a salvage call: 0 in all 173 retail calls.
pub(crate) const SALVAGE_UNKNOWN: u32 = 0;
/// How many of a stack one open takes (Inferred: 1 in all 173 retail calls).
pub(crate) const SALVAGE_ONE: u32 = 1;

/// Says `line` in area chat as the own player `own`: a game master line (`!tp`) is one.
///
/// # Errors
///
/// Returns [`RepError`] when the call cannot be encoded or sent.
pub(crate) fn say_in_area(client: &mut RepClient, own: u64, line: &str) -> Result<(), RepError> {
    let call = ChatCall::Send {
        channel: ChatChannel::Area.wire(),
        body: line.to_owned(),
        recipient_name: String::new(),
        links: Vec::new(),
    };
    let body = call.encode(SocialFacet::Chat.header(own))?;
    client.send_message(MESSAGE_CHANNEL, true, call.type_index(), body)
}

/// Sends the first time sync request of the own player `own`, as the client does when it is in
/// the game ([`crate::view::OwnView::time_sync_due`]).
///
/// # Errors
///
/// Returns [`RepError`] when the call cannot be encoded or sent.
pub(crate) fn send_time_sync(client: &mut RepClient, own: u64) -> Result<(), RepError> {
    Player::call(
        client,
        &ClientFacetMsg::TimeSyncRequest {
            header: FacetHeader::of(CALL_COMPONENT, own),
        },
    )
}
/// Time between two acts of an activity step: a teleport lands before the next act.
const ACT_GAP: Duration = Duration::from_millis(1500);
/// How near its teleport's place the server's own state shows the bot once it landed, metres.
const TELEPORT_SETTLED_M: f32 = 3.0;
/// How long a player the bot's swings did not hit in a match stays out of its fight: a
/// teammate (PvP is between the two teams only), or a foe that was dead then.
const FRIEND_MEMORY: Duration = Duration::from_secs(30);
/// A match player farther than this is no foe of the bot: an entity of the level the bot left
/// keeps its last place in the view.
const MATCH_SIGHT_M: f32 = 200.0;
/// The team that plays the objectives of an Outpost Rush or capture the flag match
/// ([`MatchGoal`]); the other team stays at its spawn.
const OBJECTIVE_TEAM: u8 = 0;
/// The team whose flag the objective team takes.
const FLAG_TEAM: usize = 1;
/// How long the bot stays in an outpost: the 60 s capture of one player, and a margin
/// (Inferred: the capture rule of the sim's `outpost_rush` module text).
const OUTPOST_STAY: Duration = Duration::from_secs(70);
/// How long the bot stays at a flag base: a few takes at the other team's, the capture at its
/// own.
const FLAG_STAY: Duration = Duration::from_secs(6);
/// How long an attacker stays on a war capture point: an outer point rises by 1 to 2 a second
/// to 99 (the sim's `war_fight` module text), and a margin.
const SIEGE_POINT_STAY: Duration = Duration::from_secs(110);
/// How far from the claim position an invasion's defender goes back onto it (a teleport), and
/// the time between two such teleports.
const CLAIM_STAND_M: f32 = 3.0;
const CLAIM_RETURN_INTERVAL: Duration = Duration::from_secs(5);
/// The time between two takes of a flag (a take in the pregame or within the flag's interact
/// timer is not taken).
const TAKE_INTERVAL: Duration = Duration::from_secs(2);
/// A teleport this far from where the bot came into an area leaves that area, metres
/// (Inferred: the bot knows no area's shape; the steps of one place lie within some 200 m, as
/// the go-to and kill plans of `withered_mines_30112` do).
const AREA_LEFT_M: f32 = 300.0;
/// The status of a taken interaction (3743).
const ACCEPTED: u8 = 1;
/// The name mark before an NPC id in a talk (`@12_JeongIseul`).
const NPC_MARK: &str = "@";

/// The words of the sim's `!queststate` and `!questplan` lines (`sim::world::gm_chat`,
/// `sim::world::quest_walk`).
const STATE_RUNNING: &str = "running ";
/// How a `!state` line starts.
const STATE_LINE: &str = "state level ";
const STATE_FINISHED: &str = "finished ";
const STATE_DONE: &str = "done ";
const STATE_NOT_TAKEN: &str = "not taken ";
/// The start of a step line of a running quest's `!queststate` answer, and the words before
/// the `POITag` of the place where the step counts.
const STATE_STEP: &str = "step ";
const STATE_PLACE: &str = "place: Some(\"";
const PLAN_LINE: &str = "plan ";
const PLAN_NOT_RUNNING: &str = "not running ";
const AT: &str = "at";
/// The word before the entrance capital of a trial line that a player enters at its door.
const TRIAL_DOOR: &str = "door";
/// The word after a gather line whose node is gathered again (a turning cyclic stone).
const GATHER_AGAIN: &str = "again";
const AFTER: &str = "after";
const TERRITORY: &str = "territory";
const FROM: &str = "from";
const POINTS: &str = "points";
/// The word of a kill plan whose mobs count only as placed: a spawned mob is in no spawn group.
const PLACED: &str = "placed";
/// The word before the radius around a kill plan's place that the bot fights within: the
/// volume of the encounter that spawns the mobs, which resets when no player is in it.
const WITHIN: &str = "within";
/// The word before each place of a kill plan where its encounter spawns a pickup taken by
/// touch (the talisman piles of `QuestLiang`): the bot walks onto them between its hits.
const TOUCH: &str = "touch";
/// The bot's swings at the fight's mob after each touch place: a taken talisman holds for the
/// next hit (`AI_AbominableLiang_ProtectiveTalisman`, 10 s); a second swing covers one that
/// missed.
const SWINGS_PER_TOUCH: u32 = 2;
/// The bot is on a touch place this near it, m (the server takes a pile within 1 m).
const TOUCH_REACH_M: f32 = 0.5;
/// The word of a plan that strikes puzzle objects (`strike <gatherable> at x y z ...`).
const STRIKE: &str = "strike";
/// Weapon swings at each puzzle object of a strike plan: the retail gong took one hit; a second
/// covers a swing that went out before the bot's own place reached the server.
const SWINGS_PER_STRIKE: u32 = 2;
/// The word of an item plan before the gatherable whose nodes give the item.
const NODE: &str = "node";
/// The word of an item plan whose step counts gathers only.
const GATHERED: &str = "gathered";
/// The source of an item plan whose item drops from the loot containers of the step's place
/// (`sim::world::quest_walk`, a task's `ChestDropProbability`).
const CONTAINER_DROP: &str = "a container drop";
/// The name of a loot container node in the bot's report lines.
const CONTAINER_NAME: &str = "loot container";
/// The bot opens the loot containers this near it for a container drop, metres (Inferred: the
/// go-to step put it in the place; `devilspit_30134` of Devil's Honey).
const CONTAINER_SEARCH_M: f32 = 150.0;
/// The game events of a mission's turn-in task in a plan the sim does not play
/// (`sim::world::missions` `TURN_IN_TAGS`): the bot turns the mission in at its board.
const MISSION_TURN_IN_EVENTS: [&str; 2] = [
    "Interacted(\"ReturnToGiver\")",
    "Interacted(\"ReturnToAnyGiver\")",
];
/// The words of a camp plan before its skin, its slice asset and each of its costs.
const CAMP_SKIN: &str = "skin";
const CAMP_ASSET: &str = "asset";
const CAMP_COST: &str = "cost";
/// The words of a skin plan before its kill: `skin`, the gatherable and the count.
const SKIN_HEAD_WORDS: usize = 3;
/// The words of an equip plan before its bag slot, its stack's instance and its paperdoll slot.
const EQUIP_BAG: &str = "bag";
const EQUIP_INSTANCE: &str = "instance";
const EQUIP_SLOT: &str = "slot";
/// The words of a craft plan line before its station and each ingredient.
const CRAFT_STATION: &str = "station";
const CRAFT_WITH: &str = "with";
/// The word of a craft plan line before each ingredient the bag lacks.
const CRAFT_GIVE: &str = "give";
/// The words before a song line's sheet and length.
const SONG_SHEET: &str = "sheet";
const SONG_SECONDS: &str = "seconds";
/// The most fish the bot catches for one catch step.
const MAX_CATCHES: u32 = 10;
/// The word before a fishing line's view yaw.
const FISH_YAW: &str = "yaw";
/// The flow of a quick course event in a plan line, and its course and section fields.
const COURSE_FLOW: &str = "Course {";
const COURSE_FIELD: &str = "course: \"";
const SECTION_FIELD: &str = "section: ";
const STOP_BROKEN: &str = "Broken:";
/// The marks before the quest that gives an achievement in an `achievements` plan line.
const QUEST_SOURCE_MARKS: [&str; 2] = ["a step of quest ", "the end of quest "];
/// The mark of a `not taken` line whose quest no NPC offers yet.
const NOT_OFFERED_MARK: &str = ": its ";

/// The bot's account, its quests and its timers.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaySettings {
    /// The bot; its account must have the game master rank.
    pub account: WalkSettings,
    /// The quests to play, in order (a chain in its order).
    pub quests: Vec<String>,
    /// The quests a game master line starts at the setup (`!quest`), past their requirements:
    /// the first quests of a test whose earlier chain a player already played live, so the run
    /// tests the quests after it.
    pub gm_start: Vec<String>,
    /// The character of the foe bot of a PvP quest step (a bot that plays
    /// [`ActivityKind::PvpFoe`]): the bot kills it for the step's player kills.
    pub pvp_foe: Option<String>,
    /// Longest time in the world.
    pub hold: Duration,
    /// Longest wait for a registration response.
    pub registration_timeout: Duration,
}

/// How the play of one quest ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// It ran from the accept to the turn-in.
    Completed,
    /// A step did not end on the server: the step and why.
    Broken(String),
    /// A step needs a flow the bot does not play: the step and the flow.
    NotPlayed(String),
}

/// The play of one quest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestPlay {
    /// The quest id.
    pub quest: String,
    /// How it ended.
    pub verdict: Verdict,
    /// What the bot did, in order.
    pub actions: Vec<String>,
}

/// What the play run saw.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayReport {
    /// What happened on the connection.
    pub outcome: RepOutcome,
    /// Each quest the bot played, in order.
    pub quests: Vec<QuestPlay>,
    /// Each activity the bot played, in order (named as the activity).
    pub activities: Vec<QuestPlay>,
}

impl PlayReport {
    /// The report as Markdown: a count of each verdict, then each quest with its stop and what
    /// the bot did.
    pub fn markdown(&self) -> String {
        let count = |wanted: fn(&Verdict) -> bool| {
            self.quests
                .iter()
                .filter(|quest| wanted(&quest.verdict))
                .count()
        };
        let mut text = String::new();
        let _ = writeln!(text, "# Quest play of the test bot\n");
        let _ = writeln!(
            text,
            "One bot of a game master account played each quest on the live servers through the \
             client's calls: the accept, talks, topics, territory entries, cinematics, gathers, \
             attacks and the turn-in. It fought the placed mobs when only those count. Game \
             master lines moved it, kept it alive, spawned a step's other mobs where the step's \
             spawn is and gave it the items no gather gives.\n"
        );
        let _ = writeln!(
            text,
            "| Verdict | Quests |\n|---|---|\n| Completed | {} |\n| Broken | {} |\n| Not played | {} |\n| Total | {} |\n",
            count(|verdict| *verdict == Verdict::Completed),
            count(|verdict| matches!(verdict, Verdict::Broken(_))),
            count(|verdict| matches!(verdict, Verdict::NotPlayed(_))),
            self.quests.len()
        );
        let _ = writeln!(
            text,
            "| Quest | Verdict | Stop | What the bot did |\n|---|---|---|---|"
        );
        for quest in &self.quests {
            let (verdict, stop) = match &quest.verdict {
                Verdict::Completed => ("Completed", ""),
                Verdict::Broken(stop) => ("Broken", stop.as_str()),
                Verdict::NotPlayed(stop) => ("Not played", stop.as_str()),
            };
            let _ = writeln!(
                text,
                "| {} | {verdict} | {} | {} |",
                cell(&quest.quest),
                cell(stop),
                cell(&quest.actions.join("; "))
            );
        }
        if !self.activities.is_empty() {
            let _ = writeln!(
                text,
                "\n## Activities\n\nEach activity's steps sent the client's calls and waited for the server's proof: an answer, a message of a type, or the answer to an interaction.\n\n| Activity | Verdict | Stop | Steps proved |\n|---|---|---|---|"
            );
            for activity in &self.activities {
                let (verdict, stop) = match &activity.verdict {
                    Verdict::Completed => ("Completed", ""),
                    Verdict::Broken(stop) => ("Broken", stop.as_str()),
                    Verdict::NotPlayed(stop) => ("Not played", stop.as_str()),
                };
                let _ = writeln!(
                    text,
                    "| {} | {verdict} | {} | {} |",
                    cell(&activity.quest),
                    cell(stop),
                    cell(&activity.actions.join("; "))
                );
            }
        }
        text
    }
}

/// A Markdown table cell of `text`.
fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

/// One step of a `!questplan` line: what the bot does next.
#[derive(Debug, Clone, PartialEq)]
enum PlanStep {
    Wait,
    /// Wait at `at`, the centre of the open world encounter that spawns the start object of
    /// gatherable `gatherable` once its earlier stages ran (`await <gatherable> at x y z`): the
    /// bot goes there and stays until the plan names the object's gather. It is a step to play
    /// now, not a wait that comes after the others, and no try of a step that does not end.
    Await {
        gatherable: String,
        at: Position,
    },
    Broken(String),
    NotPlayed(String),
    Client(Vec<ClientFacetMsg>),
    Talk {
        npc: String,
        at: Option<Position>,
    },
    Topic {
        topic: u32,
        talk: Option<(String, Position)>,
    },
    Go {
        at: Position,
        territory: Option<u16>,
    },
    Leave(Vec<u16>),
    Kill {
        vitals: String,
        count: u32,
        at: Position,
        territory: Option<u16>,
        placed: bool,
        /// The radius around `at` that the bot fights within (`within <m>`).
        within: Option<f32>,
        /// The places of the fight's pickups taken by touch (`touch x y z`, each): the bot
        /// walks onto one before every [`SWINGS_PER_TOUCH`] swings.
        touch: Vec<Position>,
    },
    /// Get `count` of `item` from `source`; `gathered`: the step counts gathers only; `node`:
    /// the gatherable whose nodes give it.
    Item {
        item: String,
        count: u32,
        source: String,
        gathered: bool,
        node: Option<String>,
    },
    Progression {
        id: String,
        target: u32,
        points: bool,
    },
    /// Gather the nearest node of the turning cyclic stone `gatherable` again, `count` times in
    /// all (the plan line `gather <gatherable> <count> again`): each gather turns it on.
    Turn {
        gatherable: String,
        count: u32,
    },
    /// Strike each of these places (puzzle gongs) with weapon swings, standing at it.
    Strike(Vec<Position>),
    /// Gather the encounter start object of slice asset `asset` at `at` as gatherable
    /// `gatherable`, by the card the bot sees there (`start <gatherable> <asset> at x y z`).
    Start {
        gatherable: String,
        asset: [u8; nw_content::town_props::ASSET_ID_LEN],
        at: Position,
    },
    /// The step waits for achievements; `sources` are the quests that give them.
    Achievements {
        needs: String,
        sources: Vec<String>,
    },
    /// Use the level object of facet entity `object` at `at` (`count` times), which carries the
    /// interact tag `tag`.
    Use {
        tag: String,
        object: u64,
        count: u32,
        at: Position,
    },
    /// Equip `item` from bag slot `bag_slot` (instance `instance`) into paperdoll slot `slot`.
    Equip {
        item: String,
        bag_slot: u32,
        instance: [u8; nw_messages::wire::RAW_ID_LEN],
        slot: u32,
    },
    /// Read the lore note of title `title` at `at`.
    Lore {
        title: String,
        at: Position,
    },
    /// Enter an instance of game mode `mode` (`crc`, the CRC-32 of its lowercase id): from
    /// afar with no `door`, else at that entrance capital (at its place, when the plan names
    /// it).
    Trial {
        mode: String,
        crc: u32,
        door: Option<(u64, Option<Position>)>,
    },
    /// Place a camp at `at` (skin `skin`, slice asset `asset`) and build it with `costs`.
    Camp {
        at: Position,
        skin: u32,
        asset: [u8; nw_content::town_props::ASSET_ID_LEN],
        costs: Vec<(String, u32)>,
    },
    /// Skin `count` corpses of gatherable `gatherable`, each after `kill` (a [`PlanStep::Kill`]
    /// of one).
    Skin {
        gatherable: String,
        count: u32,
        kill: Box<PlanStep>,
    },
    /// Open the loot bags of the kills of `kill` (a kill step): the step's item is a quest drop
    /// in a kill's bag.
    Loot {
        kill: Box<PlanStep>,
    },
    /// Play song `song` from sheet `sheet` (`duration_s` seconds) at `at`, with an instrument
    /// of the sheet worn.
    Song {
        song: String,
        sheet: String,
        duration_s: f32,
        at: Position,
    },
    /// Use the item `item` of bag slot `bag_slot` (2917).
    UseItem {
        item: String,
        bag_slot: u32,
    },
    /// Kill `count` players that are PvP foes: the foe bot of [`PlaySettings::pvp_foe`].
    Pvp {
        count: u32,
    },
    /// Fish from `at`, looking along `yaw` (radians), until a catch, after the bot told the
    /// server it came into territory `territory` (the step's fish bites only there).
    Fish {
        at: Position,
        yaw: f32,
        territory: Option<u16>,
    },
    /// Ride quick course `course` (its plan line names the section `section` to pass; a whole
    /// course's step names section 0).
    Course {
        course: String,
        section: u32,
    },
    /// Craft `recipe` once at the station of raw id `station` at `at` from the bag's
    /// `ingredients` (item, quantity), after the game master gives the `missing` ones (a player
    /// gathers or buys them).
    Craft {
        recipe: String,
        at: Position,
        station: [u8; nw_messages::wire::RAW_ID_LEN],
        ingredients: Vec<(String, u32)>,
        missing: Vec<(String, u32)>,
    },
}

impl PlanStep {
    /// The order in which the bot picks among the waiting steps of a quest, lowest first: a step
    /// it can play now (a player does what is in front of it), then one that waits for
    /// achievements (that step ends by itself once they are set; when it alone is left, the bot
    /// plays the quests that give them), then one it does not play yet, then one that cannot
    /// end, then a wait. Retail `12_getoffbeach` waits for either intro achievement or for the
    /// intro cutscene's end; the achievements come only from the other beach's quest, so a new
    /// player ends the cutscene. A step that cannot end is often a branch that never should
    /// (`06A_MSQ_18_ReturnSiriusJewel_impossible_task` beside its puzzle steps), so the step
    /// the bot does not play is the stop it names. A use of an object the bot has no
    /// interaction kind for is a step it does not play: retail `06a_ts_02_forgesword` ended the
    /// partial container of `06A_TS_02_ForgeSword_useSmelter` (tag `SettlementSmelting`) by its
    /// sibling `haveItems_ingotT2` (24 ingots at 65.1 s), and the use task never counted.
    fn pick_order(&self) -> u8 {
        match self {
            Self::Achievements { .. } => 1,
            Self::NotPlayed(_) => 2,
            Self::Use { tag, .. } if !tag.eq_ignore_ascii_case(STORAGE_TAG) => 2,
            Self::Broken(_) => 3,
            Self::Wait => 4,
            _ => 0,
        }
    }
}

/// The waiting step the bot plays next among `steps` ([`PlanStep::pick_order`]; the first of
/// equal ones, in the plan's order).
fn picked_step(steps: &[(String, PlanStep)]) -> Option<&(String, PlanStep)> {
    steps
        .iter()
        .enumerate()
        .min_by_key(|(index, (_, step))| (step.pick_order(), *index))
        .map(|(_, picked)| picked)
}

/// `steps` without the achievements steps that only quest `quest` itself gives, when another
/// step is left: such a step is a check beside the quest's own path (retail
/// `09A_M15_SoulTrial_Complete_Check1` waits for `09A_M15_ArtemisTrialComplete`, which a later
/// step of `09A_M15_FightArtemis` unlocks), and the step beside it is the one to play (live
/// 2026-11-23 04:47:39: the bot stopped at the check while `09A_M15_DummyAchievement` waited
/// for `09A_M14` from `09A_M14_GetInfusedSeed`).
fn without_own_achievements(
    steps: Vec<(String, PlanStep)>,
    quest: &str,
) -> Vec<(String, PlanStep)> {
    let own = |step: &PlanStep| {
        matches!(step, PlanStep::Achievements { sources, .. }
            if !sources.is_empty() && sources.iter().all(|source| source.eq_ignore_ascii_case(quest)))
    };
    if steps.iter().all(|(_, step)| own(step)) {
        return steps;
    }
    steps.into_iter().filter(|(_, step)| !own(step)).collect()
}

/// The place in words `x y z`.
fn place_of(words: &[&str]) -> Option<Position> {
    match words {
        [x, y, z, ..] => Some(Position {
            x: x.parse().ok()?,
            y: y.parse().ok()?,
            z: z.parse().ok()?,
        }),
        _ => None,
    }
}

/// The word after the word `word` in `words`.
fn word_after<'a>(words: &[&'a str], word: &str) -> Option<&'a str> {
    let index = words.iter().position(|known| *known == word)?;
    words.get(index + 1).copied()
}

/// The territory after the word [`TERRITORY`] in `words`.
fn territory_of(words: &[&str]) -> Option<u16> {
    word_after(words, TERRITORY)?.parse().ok()
}

/// The `POITag` of the place in which task `task` counts, from the `!queststate` lines `state`
/// (`step <task> <count> GatherItems { .. place: Some("drowned_shipwreck_3_30129") .. }`).
/// `None` when the step names no place.
fn step_place<'a>(state: &'a [String], task: &str) -> Option<&'a str> {
    let step = state
        .iter()
        .find(|line| line.starts_with(&format!("{STATE_STEP}{task} ")))?;
    let (_, after) = step.split_once(STATE_PLACE)?;
    let (tag, _) = after.split_once('"')?;
    Some(tag)
}

/// The waiting steps of the quest state lines `state` with their progress (`<task> <progress>`
/// of each `step <task> <progress> ...` line), joined: what changes when any step counts.
fn quest_progress(state: &[String]) -> String {
    state
        .iter()
        .filter_map(|line| {
            let step = line.strip_prefix(STATE_STEP)?;
            let mut words = step.split_whitespace();
            Some(format!("{} {}", words.next()?, words.next()?))
        })
        .collect::<Vec<_>>()
        .join(STEP_PROGRESS_SEPARATOR)
}

/// The chance in percent of the quest drop of step `task` in the quest state lines `state`
/// (`... drop: Some(QuestDrop { ..., percent: 40, ... })`); `None` without one.
fn step_drop_percent(state: &[String], task: &str) -> Option<u32> {
    let step = state
        .iter()
        .find(|line| line.starts_with(&format!("{STATE_STEP}{task} ")))?;
    let (_, after) = step.split_once(DROP_PERCENT_WORD)?;
    after
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

/// How often the bot does one plan before the quest stops as broken: [`MAX_SAME_PLAN`] tries,
/// and for a drop at `percent` below 100 the tries that give it at [`DROP_CONFIDENCE`] (live
/// 2026-11-22 16:04-16:19: the `1127_PrimeCheek` of `1127_0102`, 40 %, came from none of six
/// pig skins in three runs of three tries).
fn max_tries(percent: Option<u32>) -> u32 {
    let Some(percent) = percent.filter(|percent| (1..SURE_PERCENT).contains(percent)) else {
        return MAX_SAME_PLAN;
    };
    let miss = 1.0 - f64::from(percent) / f64::from(SURE_PERCENT);
    // A few tens of tries at most: the cast cannot overflow.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let tries = ((1.0 - DROP_CONFIDENCE).ln() / miss.ln()).ceil() as u32;
    MAX_SAME_PLAN.max(tries.saturating_add(1))
}

/// Plan line `line` at the quest's step progress `progress` ([`quest_progress`]): the same line
/// at another progress is another plan.
fn plan_at_progress(line: &str, progress: &str) -> String {
    format!("{line}{PROGRESS_MARK}{progress}")
}

/// The area a client reports for a place of `POITag` `tag`: the first territory of
/// `TerritoryDefinitions` that carries it (`drowned_shipwreck_3_30129` is territory 30129, and
/// `shattered_obelisk` has no number in its tag).
fn area_of_place(poi_tags: &PoiTags, tag: &str) -> Option<u16> {
    poi_tags
        .territories_with(tag)
        .into_iter()
        .find_map(|territory| u16::try_from(territory).ok())
}

/// A raw id written as lowercase hexadecimal (a plan line's item instance or station).
fn raw_id_of(word: &str) -> Option<[u8; nw_messages::wire::RAW_ID_LEN]> {
    data_encoding::HEXLOWER
        .decode(word.as_bytes())
        .ok()?
        .try_into()
        .ok()
}

/// The place after the word [`AT`] in `words`.
fn place_after_at(words: &[&str]) -> Option<Position> {
    let index = words.iter().position(|word| *word == AT)?;
    place_of(&words[index + 1..])
}

/// A hexadecimal id word.
fn hex_of(word: &str) -> Option<u32> {
    u32::from_str_radix(word, 16).ok()
}

/// The client calls of client step words `words` (`cinematic-start 1a2b3c4d`), with facet header
/// `header`.
fn client_calls(words: &[&str], header: FacetHeader) -> Option<Vec<ClientFacetMsg>> {
    let id = || words.get(1).and_then(|word| hex_of(word));
    Some(match *words.first()? {
        "cinematic-start" => vec![ClientFacetMsg::CinematicStarted {
            header,
            cinematic: id()?,
        }],
        "cinematic-end" => vec![ClientFacetMsg::CinematicEnded {
            header,
            cinematic: id()?,
        }],
        "cutscene" => vec![
            ClientFacetMsg::RequestCutscene {
                header,
                cutscene: id()?,
            },
            ClientFacetMsg::CutsceneEnded { header },
        ],
        "screen" => vec![ClientFacetMsg::UiScreenOpened {
            header,
            screen: id()?,
        }],
        "faction" => vec![ClientFacetMsg::SetFaction {
            header,
            faction: words.get(1)?.parse().ok()?,
        }],
        _ => return None,
    })
}

/// The quests that `text` names as the givers of achievements (`a step of quest X`, `the end of
/// quest Y`), in order.
fn quest_sources(text: &str) -> Vec<String> {
    QUEST_SOURCE_MARKS
        .iter()
        .flat_map(|mark| text.split(mark).skip(1))
        .filter_map(|rest| rest.split([')', ',', ' ']).next())
        .filter(|quest| !quest.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The step of plan line `line` (`plan <task> <what>`), with the bot's facet header `header`:
/// `None` for a line that is no plan line or does not read.
fn plan_step(line: &str, header: FacetHeader) -> Option<(String, PlanStep)> {
    let rest = line.strip_prefix(PLAN_LINE)?;
    let (task, what) = rest.split_once(' ')?;
    Some((task.to_owned(), what_step(what, header)?))
}

/// The step of `what`, the words of a plan line after its task, with the bot's facet header
/// `header`: `None` for words that do not read.
fn what_step(what: &str, header: FacetHeader) -> Option<PlanStep> {
    let words: Vec<&str> = what.split_whitespace().collect();
    let step = match *words.first()? {
        "wait" => PlanStep::Wait,
        "await" => PlanStep::Await {
            gatherable: (*words.get(1)?).to_owned(),
            at: place_after_at(&words)?,
        },
        "stop" => {
            let reason = what.trim_start_matches("stop").trim().to_owned();
            if reason.starts_with(STOP_BROKEN) {
                PlanStep::Broken(reason)
            } else {
                PlanStep::NotPlayed(reason)
            }
        }
        "client" => PlanStep::Client(client_calls(&words[1..], header)?),
        "talk" => PlanStep::Talk {
            npc: (*words.get(1)?).to_owned(),
            at: place_after_at(&words),
        },
        "topic" => PlanStep::Topic {
            topic: hex_of(words.get(1)?)?,
            talk: words
                .iter()
                .position(|word| *word == AFTER)
                .and_then(|index| {
                    let npc = (*words.get(index + 2)?).to_owned();
                    Some((npc, place_after_at(&words[index..])?))
                }),
        },
        STRIKE => PlanStep::Strike(
            words
                .iter()
                .enumerate()
                .filter(|(_, word)| **word == AT)
                .filter_map(|(index, _)| place_of(&words[index + 1..]))
                .collect(),
        ),
        "go" => PlanStep::Go {
            at: place_of(&words[1..])?,
            territory: territory_of(&words),
        },
        "leave" => PlanStep::Leave(
            words[1..]
                .iter()
                .filter_map(|word| word.parse().ok())
                .collect(),
        ),
        "kill" => PlanStep::Kill {
            vitals: (*words.get(1)?).to_owned(),
            count: words.get(2)?.parse().ok()?,
            at: place_after_at(&words)?,
            territory: territory_of(&words),
            placed: words.contains(&PLACED),
            within: words
                .iter()
                .position(|word| *word == WITHIN)
                .and_then(|index| words.get(index + 1)?.parse().ok()),
            touch: words
                .iter()
                .enumerate()
                .filter(|(_, word)| **word == TOUCH)
                .filter_map(|(index, _)| place_of(&words[index + 1..]))
                .collect(),
        },
        "item" => {
            let (said, source) = what.split_once(&format!(" {FROM} ")).unwrap_or((what, ""));
            let said: Vec<&str> = said.split_whitespace().collect();
            PlanStep::Item {
                item: (*words.get(1)?).to_owned(),
                count: words.get(2)?.parse().ok()?,
                source: source.to_owned(),
                gathered: said.contains(&GATHERED),
                node: said
                    .iter()
                    .position(|word| *word == NODE)
                    .and_then(|index| said.get(index + 1))
                    .map(|node| (*node).to_owned()),
            }
        }
        "progression" => PlanStep::Progression {
            id: (*words.get(1)?).to_owned(),
            target: words.get(2)?.parse().ok()?,
            points: words.contains(&POINTS),
        },
        "achievements" => {
            let needs = what.trim_start_matches("achievements").trim().to_owned();
            PlanStep::Achievements {
                sources: quest_sources(&needs),
                needs,
            }
        }
        "equip" => PlanStep::Equip {
            item: (*words.get(1)?).to_owned(),
            bag_slot: word_after(&words, EQUIP_BAG)?.parse().ok()?,
            instance: raw_id_of(word_after(&words, EQUIP_INSTANCE)?)?,
            slot: word_after(&words, EQUIP_SLOT)?.parse().ok()?,
        },
        "craft" => {
            // `craft <recipe> at <x> <y> <z> station <id> [with <item> <quantity>]...
            // [give <item> <quantity>]...`.
            let pairs = |mark: &str| {
                words
                    .iter()
                    .enumerate()
                    .filter(|(_, word)| **word == mark)
                    .map(|(index, _)| {
                        Some((
                            (*words.get(index + 1)?).to_owned(),
                            words.get(index + 2)?.parse().ok()?,
                        ))
                    })
                    .collect::<Option<Vec<(String, u32)>>>()
            };
            PlanStep::Craft {
                recipe: (*words.get(1)?).to_owned(),
                at: place_after_at(&words)?,
                station: raw_id_of(word_after(&words, CRAFT_STATION)?)?,
                ingredients: pairs(CRAFT_WITH)?,
                missing: pairs(CRAFT_GIVE)?,
            }
        }
        "use" => PlanStep::Use {
            tag: (*words.get(1)?).to_owned(),
            object: u64::from_str_radix(words.get(2)?, 16).ok()?,
            count: words.get(3)?.parse().ok()?,
            at: place_after_at(&words)?,
        },
        // `song <song> sheet <sheet> seconds <length> at <x> <y> <z>`.
        "song" => PlanStep::Song {
            song: (*words.get(1)?).to_owned(),
            sheet: word_after(&words, SONG_SHEET)?.to_owned(),
            duration_s: word_after(&words, SONG_SECONDS)?.parse().ok()?,
            at: place_after_at(&words)?,
        },
        // `useitem <item> bag <slot>`.
        "useitem" => PlanStep::UseItem {
            item: (*words.get(1)?).to_owned(),
            bag_slot: word_after(&words, EQUIP_BAG)?.parse().ok()?,
        },
        "pvp" => PlanStep::Pvp {
            count: words.get(1)?.parse().ok()?,
        },
        // `fish at <x> <y> <z> yaw <radians> [territory <id>]`.
        "fish" => PlanStep::Fish {
            at: place_after_at(&words)?,
            yaw: word_after(&words, FISH_YAW)?.parse().ok()?,
            territory: territory_of(&words),
        },
        "lore" => PlanStep::Lore {
            title: (*words.get(1)?).to_owned(),
            at: place_after_at(&words)?,
        },
        // `trial <mode> <crc> [door <capital> [at <x> <y> <z>]]`.
        "trial" => PlanStep::Trial {
            mode: (*words.get(1)?).to_owned(),
            crc: hex_of(words.get(2)?)?,
            door: match word_after(&words, TRIAL_DOOR) {
                Some(capital) => Some((
                    u64::from_str_radix(capital, 16).ok()?,
                    place_after_at(&words),
                )),
                None => None,
            },
        },
        // `camp at x y z skin <hex> asset <hex> [cost <item> <count>]...`.
        "camp" => {
            let after = |word: &str| {
                words
                    .iter()
                    .position(|known| *known == word)
                    .and_then(|index| words.get(index + 1).copied())
            };
            let costs = words
                .iter()
                .enumerate()
                .filter(|(_, word)| **word == CAMP_COST)
                .map(|(index, _)| {
                    Some((
                        (*words.get(index + 1)?).to_owned(),
                        words.get(index + 2)?.parse().ok()?,
                    ))
                })
                .collect::<Option<Vec<(String, u32)>>>()?;
            PlanStep::Camp {
                at: place_after_at(&words)?,
                skin: u32::from_str_radix(after(CAMP_SKIN)?, 16).ok()?,
                asset: data_encoding::HEXLOWER
                    .decode(after(CAMP_ASSET)?.as_bytes())
                    .ok()?
                    .try_into()
                    .ok()?,
                costs,
            }
        }
        // `gather <gatherable> <count>`: the gather of a node fires the step's game event, so the
        // bot gathers nodes of the gatherable as for an item of its nodes.
        "start" => PlanStep::Start {
            gatherable: (*words.get(1)?).to_owned(),
            asset: data_encoding::HEXLOWER
                .decode(words.get(2)?.as_bytes())
                .ok()?
                .try_into()
                .ok()?,
            at: place_after_at(&words)?,
        },
        "gather" if words.contains(&GATHER_AGAIN) => PlanStep::Turn {
            gatherable: (*words.get(1)?).to_owned(),
            count: words.get(2)?.parse().ok()?,
        },
        "gather" => {
            let gatherable = (*words.get(1)?).to_owned();
            PlanStep::Item {
                item: gatherable.clone(),
                count: words.get(2)?.parse().ok()?,
                source: format!("the gather of {gatherable}"),
                gathered: true,
                node: Some(gatherable),
            }
        }
        "skin" => {
            // `skin <gatherable> <count> <kill words>`.
            let kill_words = what.splitn(SKIN_HEAD_WORDS + 1, ' ').nth(SKIN_HEAD_WORDS)?;
            let kill = what_step(kill_words, header)?;
            if !matches!(kill, PlanStep::Kill { .. }) {
                return None;
            }
            PlanStep::Skin {
                gatherable: (*words.get(1)?).to_owned(),
                count: words.get(2)?.parse().ok()?,
                kill: Box::new(kill),
            }
        }
        "loot" => {
            // `loot <kill words>`.
            let kill = what_step(what.split_once(' ')?.1, header)?;
            if !matches!(kill, PlanStep::Kill { .. }) {
                return None;
            }
            PlanStep::Loot {
                kill: Box::new(kill),
            }
        }
        "event" => {
            let flow = what.trim_start_matches("event").trim();
            course_of(flow).map_or_else(
                || PlanStep::NotPlayed(format!("needs a flow the bot does not play yet: {flow}")),
                |(course, section)| PlanStep::Course { course, section },
            )
        }
        _ => return None,
    };
    Some(step)
}

/// The course and section of the flow words of a course event (the sim's
/// `2 Course { course: "GC_Food_Race_1", section: 0, sections: 2 }`, after the event's count);
/// `None` for another flow.
fn course_of(flow: &str) -> Option<(String, u32)> {
    let (_, fields) = flow.split_once(COURSE_FLOW)?;
    let (_, after) = fields.split_once(COURSE_FIELD)?;
    let (course, rest) = after.split_once('"')?;
    let (_, section) = rest.split_once(SECTION_FIELD)?;
    let section = section.split([',', ' ', '}']).next()?.parse().ok()?;
    Some((course.to_owned(), section))
}

/// What the bot waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    State,
    Plan,
}

/// Where the bot is in its run.
#[derive(Debug, Clone, PartialEq)]
enum Phase {
    /// It leaves the tutorial with the client's `ReturnToMainWorld`, which it sends after the tutorial's last video: a new
    /// character starts in the tutorial; the open world takes no such call.
    LeaveTutorial,
    /// It waits until the time for the open world.
    Leaving { until: Duration },
    /// It sends the setup line of this index.
    Setup(usize),
    /// It sends the line of [`SIM_LINES`] of this index in the sim a gather moved it into,
    /// then asks where the quest stands.
    SimLines(usize),
    /// It asked; the answer's lines start after this many system lines.
    Asked {
        ask: Ask,
        sent: Duration,
        lines_before: usize,
        first_line: Option<Duration>,
    },
    /// It waits until the time, then asks.
    Pause { until: Duration, then: Ask },
    /// It fights the nearest mob until the time.
    Fight { until: Duration },
    /// It gathers a node for a step's item ([`Player::gather`]).
    Gather,
    /// It strikes the puzzle objects of a strike plan ([`Player::strike`]).
    Strike,
    /// It fishes for a catch step ([`Player::fishing`]).
    Fish,
    /// It plays the activities.
    Activities,
    /// Every quest and activity was played.
    Done,
}

/// Steps the bot plays one act at a time: an activity, or the gather of a quest step's node.
struct RunningSteps {
    steps: Vec<Step>,
    /// The step it is at, and the step's next act.
    step: usize,
    act: usize,
    /// When the last act went out.
    last_act: Option<Duration>,
    /// What came before the step's first act.
    marks: StepMarks,
    /// The id of the step's interaction.
    interaction: Option<u64>,
    /// The steps proved so far.
    proved: Vec<String>,
}

impl RunningSteps {
    const fn of(steps: Vec<Step>) -> Self {
        Self {
            steps,
            step: 0,
            act: 0,
            last_act: None,
            marks: StepMarks::NONE,
            interaction: None,
            proved: Vec::new(),
        }
    }

    /// The step is proved: the next one starts at its first act, with new marks.
    const fn next_step(&mut self) {
        self.step += 1;
        self.act = 0;
        self.marks.set = false;
    }
}

/// The counts of what came before a step's first act: system lines, messages, interaction
/// answers, heard lines. They are set once a step, and an act that waits over many ticks keeps
/// them (live 2026-11-23 04:16:16: a war bot's match play set them again each tick and never saw
/// the war's 245).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StepMarks {
    lines: usize,
    messages: usize,
    answers: usize,
    heard: usize,
    /// Whether they were set for the step the bot is at.
    set: bool,
}

impl StepMarks {
    /// No marks yet.
    const NONE: Self = Self {
        lines: 0,
        messages: 0,
        answers: 0,
        heard: 0,
        set: false,
    };

    /// Notes the counts, unless they were set for this step.
    const fn mark(&mut self, lines: usize, messages: usize, answers: usize, heard: usize) {
        if !self.set {
            *self = Self {
                lines,
                messages,
                answers,
                heard,
                set: true,
            };
        }
    }
}

/// The node of `nodes` of gatherable `gatherable` nearest to `from` (planar) that is not in
/// `tried`, by its index.
fn nearest_untried(
    nodes: &[GatherNode],
    tried: &HashSet<usize>,
    gatherable: u32,
    from: [f32; 3],
) -> Option<usize> {
    nodes
        .iter()
        .enumerate()
        .filter(|(index, node)| node.gatherable == gatherable && !tried.contains(index))
        .min_by(|(_, a), (_, b)| {
            planar(from, [a.at[0], a.at[1]]).total_cmp(&planar(from, [b.at[0], b.at[1]]))
        })
        .map(|(index, _)| index)
}

/// Whether `step` gathers nodes of a gatherable of which `nodes` holds no node outside `tried`.
fn gathered_out(nodes: &[GatherNode], tried: &HashSet<usize>, step: &PlanStep) -> bool {
    let PlanStep::Item {
        node: Some(node), ..
    } = step
    else {
        return false;
    };
    let gatherable = lowercase_crc32(node);
    !nodes
        .iter()
        .enumerate()
        .any(|(index, known)| known.gatherable == gatherable && !tried.contains(&index))
}

/// The bot during its hold.
struct Player {
    world: WorldView,
    combat: CombatView,
    read: usize,
    /// The server warned the bot that it stands still (2139) since it last stepped aside.
    afk_warned: bool,
    own_since: Option<Duration>,
    lines: Vec<String>,
    /// The bodies of the player chat lines the bot heard (lines with a sender), in order.
    heard: Vec<String>,
    phase: Phase,
    /// The quests still to play, in order.
    queue: std::collections::VecDeque<String>,
    /// The quest the bot plays now; `None` when every quest was played.
    current: Option<String>,
    /// The quests no NPC offered yet, with why: each is tried again after a quest that
    /// completed.
    deferred: Vec<(String, String)>,
    /// Whether a quest completed since the last quest was deferred.
    completed_since_deferral: bool,
    /// The quests played or being played, lowercase.
    played: std::collections::HashSet<String>,
    accepted: bool,
    turned_in: bool,
    /// Whether the bot turned in the current mission at its board.
    mission_turn_in_sent: bool,
    /// The tasks of the current quest the sim planned (the CRC-32 of each lowercase id): they
    /// tell the mission's instance at its turn-in ([`WorldView::instances_listing`]).
    quest_tasks: Vec<u32>,
    rounds: u32,
    last_plan: Option<String>,
    same_plan: u32,
    actions: Vec<String>,
    plays: Vec<QuestPlay>,
    last_swing: Option<Duration>,
    last_move: Option<Duration>,
    /// When the bot last teleported to a fleeing mob of its kill.
    last_chase: Option<Duration>,
    input_counter: u16,
    interaction: u64,
    /// The activities still to play, and the one played now.
    activities: std::collections::VecDeque<Activity>,
    /// The activities come before the quests ([`PlayData::activities_first`]).
    activities_first: bool,
    running: Option<(ActivityKind, RunningSteps)>,
    /// Every gather node of the level the bot is in, in world file order.
    nodes: Arc<[GatherNode]>,
    /// The gather nodes of the open world level ([`PlayData::nodes`]).
    world_nodes: Arc<[GatherNode]>,
    /// The gather nodes of each trial level ([`PlayData::trial_nodes`]).
    trial_nodes: Arc<HashMap<String, Arc<[GatherNode]>>>,
    /// The gatherables of the nodes that are loot containers (`IsLootContainer`).
    containers: HashSet<u32>,
    /// The `POITag`s of the territories, which name the area of a step's place.
    poi_tags: PoiTags,
    /// The game master lines of the setup ([`setup_lines`]).
    setup_lines: Vec<String>,
    /// The nodes the bot gathered or tried to gather.
    tried_nodes: HashSet<usize>,
    /// The corpses the bot skinned or tried to skin.
    skinned: HashSet<u64>,
    /// Where the bags the bot has not opened start in the view's list of loot bags
    /// ([`CombatView::bags`]): the ones before were opened or tried.
    loot_from: usize,
    /// The gather of a quest step's node the bot plays now, with what it is for.
    gather: Option<(String, RunningSteps)>,
    /// The game mode of the instance the bot entered for a quest step; `None` in the open
    /// world.
    in_trial: Option<String>,
    /// The game mode of the instance the running gather enters; the bot is in it once that
    /// gather ends well.
    entering_trial: Option<String>,
    /// Whether the running gather moves the bot into another sim (into or out of an
    /// instance): god mode stays on the sim it was turned on in (sim `world/admin.rs`
    /// `GodMode`), so the bot sends [`SIM_LINES`] again once the gather ends well (live
    /// 2026-11-22 13:52:24: the leader of a group, god mode on in the open world, died in the
    /// DungeonShatteredObelisk instance at a 947 hit of its boss, which then reset).
    changing_sim: bool,
    activity_plays: Vec<QuestPlay>,
    /// Every interaction answer so far.
    answers: Vec<InteractionAnswer>,
    /// The place of the last teleport, or of the bot's last own move.
    teleported_to: Option<Position>,
    /// The areas the bot told the server it came into (2297), each with where it was then; a
    /// teleport far from that place leaves it (6416), as a client does when it travels.
    entered_areas: Vec<(u16, Position)>,
    /// The lines of the last `!queststate` answer of a running quest: its steps and their places.
    state_lines: Vec<String>,
    /// The fight the bot fights now.
    fight: Option<Fight>,
    /// The strike plan it plays: its task, places, the place it is at and the swings there.
    strike: Option<Strikes>,
    /// The fights for the current kill step.
    fought: Option<Fought>,
    /// The mobs the running fight swung at.
    swung_at: Vec<u64>,
    /// The bot's own body on its walk ([`Act::Walk`]).
    walker: Option<RecordWalker>,
    /// The quick courses' rules and node paths.
    courses: Arc<CourseRoutes>,
    /// The client item index of the mount quest's reward box, or why the catalog has none.
    mount_reward_box: Result<u32, String>,
    /// Whether the bot rides its mount (its summon steps for a course ended well).
    riding: bool,
    /// Whether the running gather summons the mount: the bot rides once it ends well.
    mounting: bool,
    /// The fishing of a catch step: what it is for, the angler and the view's yaw.
    fishing: Option<Fishing>,
    /// The fish caught for the current catch step, and whether its last run caught one.
    catches: u32,
    caught_last: bool,
    /// The character of the foe bot of a PvP step ([`PlaySettings::pvp_foe`]).
    pvp_foe: Option<String>,
    /// Whether the bot's PvP flag is on (its flag steps ended well).
    pvp_armed: bool,
    /// Whether the running gather puts the PvP flag on: it is on once it ends well.
    arming_pvp: bool,
    /// The bot hunts the mobs of its fight while its activity's steps wait ([`Act::Hunt`]).
    hunting: bool,
    /// The match the bot plays ([`Act::PlayMatch`]).
    match_play: Option<MatchPlay>,
    /// The pops the bot accepts ([`Act::AcceptPops`]).
    accepting: Option<Accepting>,
}

/// The accepts of a bot's pops ([`Act::AcceptPops`]): when the act began and the match id it
/// accepted last.
struct Accepting {
    started: Duration,
    accepted: Option<String>,
}

/// A bot's play in a match of a queued game mode ([`Act::PlayMatch`]).
struct MatchPlay {
    /// When the act began.
    started: Duration,
    /// The players (and the mobs of an invasion's defence) the bot's swings did not hit, each
    /// with when it found that.
    friends: HashMap<u64, Duration>,
    /// The foe the bot fights.
    target: Option<MatchTarget>,
    /// The objective places of the bot's map and team in turn ([`objective_places`]); empty for
    /// a bot that stays; `None` until the bot knows its place.
    places: Option<Vec<Position>>,
    /// The index of the place the bot goes to (it goes round), and when it came there.
    next_place: usize,
    at_place_since: Option<Duration>,
    /// When the bot last sent a flag take.
    last_take: Option<Duration>,
}

impl MatchPlay {
    fn new(started: Duration) -> Self {
        Self {
            started,
            friends: HashMap::new(),
            target: None,
            places: None,
            next_place: 0,
            at_place_since: None,
            last_take: None,
        }
    }
}

/// The foe a bot fights in a match: its swings at it since its last hit, and the view's hits
/// dealt then.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MatchTarget {
    number: u64,
    swings: u32,
    hits: u64,
}

/// The objective places of a bot of team `team` that stands at `own` in a match for `goal`, in
/// the order it goes round them: the outposts A, B and C, or the other team's flag base then its
/// own base, of the map whose places lie nearest; none for the team that stays or the fight. A
/// war's capture points are the goal's own, for every team.
fn objective_places(goal: &MatchGoal, team: Option<u8>, own: Position) -> Vec<Position> {
    let at = |[x, y, z]: [f32; 3]| Position { x, y, z };
    if let MatchGoal::Siege(points) = goal {
        return points.iter().copied().map(at).collect();
    }
    if team != Some(OBJECTIVE_TEAM) {
        return Vec::new();
    }
    let maps: Vec<Vec<[f32; 3]>> = match goal {
        MatchGoal::Fight | MatchGoal::Siege(_) | MatchGoal::Defend(_) | MatchGoal::Clear { .. } => {
            return Vec::new();
        }
        MatchGoal::Outposts => sim::world::outpost_places()
            .map(|outposts| outposts.to_vec())
            .collect(),
        MatchGoal::Flags => sim::world::flag_bases()
            .map(|bases| vec![bases[FLAG_TEAM], bases[usize::from(OBJECTIVE_TEAM)]])
            .collect(),
    };
    let distance = |[x, y, z]: [f32; 3]| {
        let (dx, dy, dz) = (x - own.x, y - own.y, z - own.z);
        (dx * dx + dy * dy + dz * dz).sqrt()
    };
    let nearest = |places: &Vec<[f32; 3]>| {
        places
            .iter()
            .map(|place| distance(*place))
            .fold(f32::INFINITY, f32::min)
    };
    maps.into_iter()
        .min_by(|a, b| nearest(a).total_cmp(&nearest(b)))
        .unwrap_or_default()
        .into_iter()
        .map(at)
        .collect()
}

/// The fishing of a catch step.
struct Fishing {
    what: String,
    angler: Angler,
    yaw: f32,
}

/// What the bot did in a fight for a kill step.
struct Fight {
    task: String,
    /// The fight is with the nearest other player, a PvP foe, not with mobs.
    foe: bool,
    /// The place and radius the bot fights within: the mobs farther out are not its fight.
    area: Option<(Position, f32)>,
    /// The vitals id (CRC-32 of the lowercase id) of the kill step's mob: the bot fights the
    /// nearest such mob first, then the nearest other one. Live 2026-11-22 13:58-14:03: the bot
    /// fought the `Wolf_White_49` that came at it beside the fleeing `Elk_Strange_48` of
    /// `0332_0102_fabulousfurs` and skinned no pelt in five fights.
    vitals: Option<u32>,
    /// Whether it saw a mob at any time of the fight.
    saw_mobs: bool,
    swings: u32,
    /// The view's hits dealt and mob deaths when the fight started.
    hits_before: u64,
    mob_deaths_before: u64,
    /// The view's hits dealt when the fight last went on past its time, and how often it did.
    hits_at_check: u64,
    extensions: u32,
    /// The view's bundles and mob cards read when the fight started: a fight that saw no mob
    /// tells by them whether the server sent the bot its surroundings.
    bundles_before: u64,
    mob_cards_before: u64,
    /// The places of the fight's pickups taken by touch ([`PlanStep::Kill`]), the next one to
    /// walk onto, and the swings since the last.
    touch: Vec<Position>,
    next_touch: usize,
    swings_since_touch: u32,
}

/// The fights for one kill step so far: the hits the bot landed and the mobs that died in view.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Fought {
    task: String,
    hits: u64,
    mob_deaths: u64,
    fights: u32,
    /// The hits of the last fight.
    last_hits: u64,
    /// The mobs the fights swung at since the last fight that landed a hit: the next fight takes
    /// another mob first. Live 2026-11-23 04:48-04:50Z: the Ennead Goliaths have no mob prefab, so
    /// their cards carry no vitals (no row, no invulnerability), and three fights swung at the
    /// nearest one, Horus, who takes no hits while the Brute stands.
    spared: Vec<u64>,
}

impl Fought {
    /// Whether the bot fights on for kill step `task` though its plan did not change: its last
    /// fight landed hits (the mob loses health), within [`MAX_KILL_FIGHTS`].
    fn fights_on(fought: Option<&Self>, task: &str) -> bool {
        fought.is_some_and(|fought| {
            fought.task == task && fought.last_hits > 0 && fought.fights < MAX_KILL_FIGHTS
        })
    }

    /// The verdict of kill step `task` that did not end after its fights: a step whose fights
    /// killed no mob says nothing of its quest (the bot's hits did not kill), else the kills did
    /// not count and the quest is broken.
    fn verdict(fought: Option<&Self>, task: &str, stop: String) -> Verdict {
        match fought.filter(|fought| fought.task == task) {
            Some(fought) if fought.mob_deaths == 0 => Verdict::NotPlayed(format!(
                "{task}: the bot landed {} hits and no mob died ({stop})",
                fought.hits
            )),
            Some(fought) => Verdict::Broken(format!(
                "{stop}; {} mobs died near the bot",
                fought.mob_deaths
            )),
            None => Verdict::Broken(stop),
        }
    }
}

/// How far above a place the bot's teleport asks to be put: the server drops the body from the
/// asked height to the floor under it ([`Act::Teleport`] places), and a spawn point may lie a
/// little under its floor. Live 2026-11-23 04:48:22.657Z: `!tp 1297.5 1442.9 929.3` (the Ennead
/// Horus Goliath's spawn point, 0.3 m under the arena floor at 929.59) dropped the bot onto the
/// floor 61 m below, z 868.02. Inferred: one metre clears such a point and stays under a ceiling.
const TELEPORT_LIFT_M: f32 = 1.0;

/// The game master line that teleports the bot to `at`, asked [`TELEPORT_LIFT_M`] above it.
fn teleport_line(at: Position) -> String {
    format!("!tp {:.1} {:.1} {:.1}", at.x, at.y, at.z + TELEPORT_LIFT_M)
}

/// The mobs a kill step's next fight takes last ([`Fought::spared`]): those of `spared` and the
/// mobs `swung_at` of a fight that landed no hit, none after a fight whose `hits` landed.
fn spared_after(mut spared: Vec<u64>, swung_at: Vec<u64>, hits: u64) -> Vec<u64> {
    if hits > 0 {
        return Vec::new();
    }
    for mob in swung_at {
        if !spared.contains(&mob) {
            spared.push(mob);
        }
    }
    spared
}

/// The mob (entity, place) a kill step's fight swings at, from `mobs` in view: the nearest one in
/// `area` whose vitals id (`vitals_of`) is the step's `vitals`, else the nearest one there; a mob
/// that takes no hits now (`takes_no_hits`: its vitals show invulnerability) only when every mob
/// there takes none. Live 2026-11-23 03:03:53Z: the Ennead Horus Goliath takes no hits while
/// the Brute stands (retail rule, `EXP-The-Ennead` 1565.5-1698.8 s), and the bots swung at it.
fn fight_target(
    mobs: &[(u64, nw_messages::WorldPosition)],
    vitals_of: impl Fn(u64) -> Option<u32>,
    takes_no_hits: impl Fn(u64) -> bool,
    area: Option<(Position, f32)>,
    vitals: Option<u32>,
    from: [f32; 3],
) -> Option<(u64, nw_messages::WorldPosition)> {
    let nearest = |a: &(u64, nw_messages::WorldPosition), b: &(u64, nw_messages::WorldPosition)| {
        planar(from, [a.1.x, a.1.y]).total_cmp(&planar(from, [b.1.x, b.1.y]))
    };
    let in_area_mobs: Vec<(u64, nw_messages::WorldPosition)> = mobs
        .iter()
        .filter(|(_, mob)| in_area(area, [mob.x, mob.y]))
        .copied()
        .collect();
    let hittable: Vec<(u64, nw_messages::WorldPosition)> = in_area_mobs
        .iter()
        .filter(|(entity, _)| !takes_no_hits(*entity))
        .copied()
        .collect();
    let in_fight = if hittable.is_empty() {
        in_area_mobs
    } else {
        hittable
    };
    in_fight
        .iter()
        .filter(|(entity, _)| vitals.is_some() && vitals_of(*entity) == vitals)
        .copied()
        .min_by(nearest)
        .or_else(|| in_fight.iter().copied().min_by(nearest))
}

/// Whether a bot at `own` stays where it is for a kill step's fight in `area`: it is in that
/// area already (a fight without one starts at the step's place).
fn stays_in_fight_area(area: Option<(Position, f32)>, own: Option<Position>) -> bool {
    area.is_some() && own.is_some_and(|own_at| in_area(area, [own_at.x, own_at.y]))
}

/// Whether a mob at `at` is in the fight's `area` (place and radius): within the radius less
/// the bot's reach, so the bot that walks up to it stays in the area; every mob without one.
fn in_area(area: Option<(Position, f32)>, at: [f32; 2]) -> bool {
    area.is_none_or(|(place, radius_m)| {
        planar([place.x, place.y, place.z], at) <= (radius_m - REACH_M).max(0.0)
    })
}

/// The strike plan the bot plays ([`PlanStep::Strike`]).
struct Strikes {
    task: String,
    places: Vec<Position>,
    /// The place it strikes now, by index.
    next: usize,
    /// Its swings at that place.
    swings: u32,
}

impl Fight {
    /// The touch place the bot walks onto next, when the fight has touch places and the bot
    /// swung [`SWINGS_PER_TOUCH`] times since the last.
    fn touch_due(&self) -> Option<Position> {
        (self.swings_since_touch >= SWINGS_PER_TOUCH)
            .then(|| self.touch.get(self.next_touch).copied())
            .flatten()
    }

    /// The bot stands on the touch place [`Self::touch_due`] gave: its swings come next, then the
    /// next place.
    fn touched(&mut self) {
        if !self.touch.is_empty() {
            self.next_touch = (self.next_touch + 1) % self.touch.len();
        }
        self.swings_since_touch = 0;
    }

    /// Whether the fight goes on past its time, given the view's `hits_dealt` and `mob_deaths`
    /// now: a mob fight whose hits landed since the last check and in which no mob died yet
    /// goes on (the run's hold bounds it). Live 2026-11-22 14:34-14:41: the `Barbary_Lion_61`
    /// of `16_Side_20` (28,000 health) had 1,584 to 2,792 left at each fight's end and healed
    /// back before the next. Live 15:31-15:37: the Cutlass Keys admiral
    /// `Undead_Admiral_Brute_DG_Cutlass_00` has 441,551 health, about 1,030 hits of the bot,
    /// some 13 minutes of one fight; a fight that ended after four extensions (105 s) left it
    /// at 313,177 after three fights.
    fn goes_on(&mut self, hits_dealt: u64, mob_deaths: u64) -> bool {
        let goes_on =
            !self.foe && hits_dealt > self.hits_at_check && mob_deaths == self.mob_deaths_before;
        if goes_on {
            self.hits_at_check = hits_dealt;
            self.extensions += 1;
        }
        goes_on
    }

    /// The verdict of a fight that says nothing of its quest: the bot saw no mob to fight, with
    /// where it stood (`at`) and what `view` read in the fight.
    fn not_played(&self, view: &CombatView, at: Option<Position>) -> Option<Verdict> {
        (!self.saw_mobs).then(|| {
            Verdict::NotPlayed(format!(
                "{}: the bot saw no mob there in {} s (it stood at {}; in the fight it read {} world bundles and {} mob cards)",
                self.task,
                FIGHT_TIME.as_secs(),
                at.map_or_else(
                    || "an unknown place".to_owned(),
                    |at| format!("{:.1} {:.1} {:.1}", at.x, at.y, at.z)
                ),
                view.bundles().saturating_sub(self.bundles_before),
                view.mob_cards().saturating_sub(self.mob_cards_before)
            ))
        })
    }
}

impl Player {
    fn tick(
        &mut self,
        client: &mut RepClient,
        outcome: &RepOutcome,
        since: Duration,
    ) -> Result<(), RepError> {
        for received in &outcome.messages[self.read..] {
            // A message that does not decode fails nothing here: the plays read the answers.
            let _ = self.world.read(received);
            let _ = self.combat.read(received);
            self.afk_warned |= is_afk_warning(received);
        }
        if let Some(own) = self.combat.time_sync_due() {
            send_time_sync(client, own)?;
        }
        let (system, heard): (Vec<_>, Vec<_>) = chat_lines(&outcome.messages[self.read..])
            .into_iter()
            .partition(is_system);
        self.lines.extend(system.into_iter().map(|line| line.body));
        self.heard.extend(heard.into_iter().map(|line| line.body));
        self.read = outcome.messages.len();
        self.answers.extend(self.combat.take_interactions());
        let Some(own) = self.world.own_number() else {
            return Ok(());
        };
        let first = *self.own_since.get_or_insert(since);
        if since.saturating_sub(first) < FIRST_LINE_DELAY {
            return Ok(());
        }
        if std::mem::take(&mut self.afk_warned) {
            self.step_aside(client)?;
        }
        let header = FacetHeader::of(CALL_COMPONENT, own);
        match self.phase.clone() {
            Phase::Done => Ok(()),
            Phase::Activities => self.activity_tick(client, outcome, own, since),
            Phase::LeaveTutorial => {
                // The sim takes no leave before the first time sync, sent after the own card.
                Self::call(
                    client,
                    &ClientFacetMsg::ReturnToMainWorld {
                        header,
                        flag: LEAVE_FLAG,
                    },
                )?;
                self.phase = Phase::Leaving {
                    until: since + LEVEL_CHANGE_WAIT,
                };
                Ok(())
            }
            Phase::Leaving { until } if since >= until => {
                self.phase = Phase::Setup(0);
                Ok(())
            }
            Phase::Leaving { .. } => Ok(()),
            Phase::Setup(index) => match self.setup_lines.get(index).cloned() {
                Some(line) => {
                    self.say(client, own, &line)?;
                    self.phase = Phase::Setup(index + 1);
                    Ok(())
                }
                None if self.activities_first => {
                    self.phase = Phase::Activities;
                    Ok(())
                }
                None => self.ask(client, own, Ask::State, since),
            },
            Phase::SimLines(index) => match SIM_LINES.get(index) {
                Some(line) => {
                    self.say(client, own, line)?;
                    self.phase = Phase::SimLines(index + 1);
                    Ok(())
                }
                None => self.ask(client, own, Ask::State, since),
            },
            Phase::Pause { until, then } if since >= until => self.ask(client, own, then, since),
            Phase::Pause { .. } => Ok(()),
            Phase::Fight { until } if since >= until => {
                let (hits, deaths) = (self.combat.hits_dealt(), self.combat.mob_deaths());
                if self
                    .fight
                    .as_mut()
                    .is_some_and(|fight| fight.goes_on(hits, deaths))
                {
                    self.phase = Phase::Fight {
                        until: since + FIGHT_EXTENSION,
                    };
                    return Ok(());
                }
                self.fight_ended(client, own, since)
            }
            Phase::Fight { .. } => self.fight(client, own, since),
            Phase::Strike => self.strike_tick(client, own, since),
            Phase::Gather => self.gather_tick(client, outcome, own, since),
            Phase::Fish => self.fish_tick(client, own, since),
            Phase::Asked {
                ask,
                sent,
                lines_before,
                first_line,
            } => {
                let arrived = self.lines.len() > lines_before;
                match (arrived, first_line) {
                    (false, _) if since.saturating_sub(sent) > ANSWER_WAIT => {
                        self.end_quest(Verdict::Broken(format!(
                            "no answer from the server to {}",
                            self.ask_line(ask)
                        )));
                        self.next_quest(client, own, since)
                    }
                    (false, _) => Ok(()),
                    (true, None) => {
                        self.phase = Phase::Asked {
                            ask,
                            sent,
                            lines_before,
                            first_line: Some(since),
                        };
                        Ok(())
                    }
                    (true, Some(first)) if since.saturating_sub(first) >= ANSWER_SETTLE => {
                        let answer = self.lines[lines_before..].to_vec();
                        match ask {
                            Ask::State => self.on_state(client, own, header, &answer, since),
                            Ask::Plan => self.on_plan(client, own, header, &answer, since),
                        }
                    }
                    (true, Some(_)) => Ok(()),
                }
            }
        }
    }

    /// The quest the bot plays now.
    fn current(&self) -> &str {
        self.current.as_deref().unwrap_or_default()
    }

    /// The game master line of `ask` for the current quest.
    fn ask_line(&self, ask: Ask) -> String {
        match ask {
            Ask::State => format!("!queststate {}", self.current()),
            Ask::Plan => format!("!questplan {}", self.current()),
        }
    }

    fn ask(
        &mut self,
        client: &mut RepClient,
        own: u64,
        ask: Ask,
        since: Duration,
    ) -> Result<(), RepError> {
        if self.current.is_none() {
            self.phase = if self.activities.is_empty() {
                Phase::Done
            } else {
                Phase::Activities
            };
            return Ok(());
        }
        let line = self.ask_line(ask);
        self.say(client, own, &line)?;
        self.phase = Phase::Asked {
            ask,
            sent: since,
            lines_before: self.lines.len(),
            first_line: None,
        };
        Ok(())
    }

    /// Says `line` in area chat.
    fn say(&mut self, client: &mut RepClient, own: u64, line: &str) -> Result<(), RepError> {
        say_in_area(client, own, line)
    }

    /// Sends client call `call`.
    fn call(client: &mut RepClient, call: &ClientFacetMsg) -> Result<(), RepError> {
        client.send_message(MESSAGE_CHANNEL, true, call.type_index(), call.encode()?)
    }

    fn pause(&mut self, since: Duration, wait: Duration, then: Ask) {
        self.phase = Phase::Pause {
            until: since + wait,
            then,
        };
    }

    fn on_state(
        &mut self,
        client: &mut RepClient,
        own: u64,
        header: FacetHeader,
        answer: &[String],
        since: Duration,
    ) -> Result<(), RepError> {
        let objective = lowercase_crc32(&self.current().to_lowercase());
        // Answers to earlier lines (a setup line's `Done.`) may come in the same batch.
        let line = answer
            .iter()
            .map(String::as_str)
            .find(|line| {
                [STATE_RUNNING, STATE_FINISHED, STATE_DONE, STATE_NOT_TAKEN]
                    .iter()
                    .any(|start| line.starts_with(start))
            })
            .unwrap_or_default();
        self.rounds += 1;
        if self.rounds > MAX_ROUNDS {
            self.end_quest(Verdict::Broken(format!(
                "still running after {MAX_ROUNDS} rounds"
            )));
            return self.next_quest(client, own, since);
        }
        if line.starts_with(STATE_DONE) {
            // A quest taken in an earlier run that this run turned in ended in this run too, and
            // so did a quest started by a game master line whose steps the bot played.
            self.end_quest(
                if self.accepted || self.turned_in || !self.actions.is_empty() {
                    Verdict::Completed
                } else {
                    Verdict::NotPlayed("the character had done it before this run".to_owned())
                },
            );
            return self.next_quest(client, own, since);
        }
        let first: Vec<String> = quest_sources(line)
            .into_iter()
            .filter(|quest| !self.played.contains(&quest.to_lowercase()))
            .collect();
        if line.starts_with(STATE_NOT_TAKEN) && !first.is_empty() && !self.accepted {
            // The quests that give the missing achievements go first; this one comes after.
            self.actions.clear();
            let current = self.current().to_owned();
            self.queue.push_front(current);
            for quest in first.into_iter().rev() {
                self.queue.push_front(quest);
            }
            self.current = None;
            return self.advance(client, own, since);
        }
        if line.starts_with(STATE_NOT_TAKEN) && line.contains(NOT_OFFERED_MARK) && !self.accepted {
            self.deferred
                .push((self.current().to_owned(), line.to_owned()));
            self.completed_since_deferral = false;
            self.actions.clear();
            return self.advance(client, own, since);
        }
        if line.starts_with(STATE_NOT_TAKEN) {
            if self.accepted {
                self.end_quest(Verdict::Broken(format!(
                    "the accept did not start it ({line})"
                )));
                return self.next_quest(client, own, since);
            }
            Self::call(
                client,
                &ClientFacetMsg::AcceptObjective { header, objective },
            )?;
            self.accepted = true;
            self.actions.push("accepted".to_owned());
            self.pause(since, ACTION_PAUSE, Ask::State);
            return Ok(());
        }
        if line.starts_with(STATE_FINISHED) {
            if self.turned_in {
                self.end_quest(Verdict::Broken(
                    "its tasks are done, but the turn-in does not end it".to_owned(),
                ));
                return self.next_quest(client, own, since);
            }
            Self::call(
                client,
                &ClientFacetMsg::CompleteObjective { header, objective },
            )?;
            self.turned_in = true;
            self.actions.push("turned in".to_owned());
            self.pause(since, ACTION_PAUSE, Ask::State);
            return Ok(());
        }
        if line.starts_with(STATE_RUNNING) {
            self.state_lines = answer.to_vec();
            return self.ask(client, own, Ask::Plan, since);
        }
        self.end_quest(Verdict::Broken(format!(
            "an answer the bot does not read: {line}"
        )));
        self.next_quest(client, own, since)
    }

    fn on_plan(
        &mut self,
        client: &mut RepClient,
        own: u64,
        header: FacetHeader,
        answer: &[String],
        since: Duration,
    ) -> Result<(), RepError> {
        if answer.iter().any(|line| line.starts_with(PLAN_NOT_RUNNING)) {
            return self.ask(client, own, Ask::State, since);
        }
        let steps = without_own_achievements(
            answer
                .iter()
                .filter_map(|line| plan_step(line, header))
                .collect(),
            self.current(),
        );
        // A gather step whose every node the bot tried waits for another step's gather (retail
        // `99B_MSQ_01_d_02_04_02_01_Gather_Candles`: one candle from each of the nodes that
        // the sibling steps name), so the bot plays the others first.
        let fresh: Vec<(String, PlanStep)> = steps
            .iter()
            .filter(|(_, step)| !gathered_out(&self.nodes, &self.tried_nodes, step))
            .cloned()
            .collect();
        let Some((task, step)) =
            picked_step(if fresh.is_empty() { &steps } else { &fresh }).cloned()
        else {
            self.end_quest(Verdict::Broken(format!(
                "a plan the bot does not read: {}",
                answer.join(" / ")
            )));
            return self.next_quest(client, own, since);
        };
        let task_crc = lowercase_crc32(&task.to_lowercase());
        if !self.quest_tasks.contains(&task_crc) {
            self.quest_tasks.push(task_crc);
        }
        // A skin step without a corpse to skin kills one first, as a player does, and so does a
        // loot step without a bag of a kill to open.
        let step = match step {
            PlanStep::Skin { kill, .. } if self.fresh_corpse(&kill).is_none() => *kill,
            PlanStep::Loot { kill } if self.combat.bags().len() <= self.loot_from => {
                self.loot_from = self.combat.bags().len();
                *kill
            }
            other => other,
        };
        let line = answer
            .iter()
            .find(|line| line.starts_with(&format!("{PLAN_LINE}{task} ")))
            .cloned()
            .unwrap_or_default();
        let fights_on = (matches!(step, PlanStep::Kill { .. })
            && Fought::fights_on(self.fought.as_ref(), &task))
            || (matches!(step, PlanStep::Fish { .. }) && self.fishes_on());
        // A plan after a step of the quest counted is no plan again: live 2026-11-22
        // 13:52-14:01, the bot gave up `0713_0101`'s eight charcoal after three kills that each
        // gave one; 16:05-16:12, the loin and the steak of `1127_0102` dropped from the pig
        // skins of the cheek step's plan.
        let plan = plan_at_progress(&line, &quest_progress(&self.state_lines));
        let waits = matches!(step, PlanStep::Wait | PlanStep::Await { .. });
        if self.last_plan.as_deref() == Some(plan.as_str()) && !waits && !fights_on {
            self.same_plan += 1;
        } else {
            if self.last_plan.as_deref() != Some(plan.as_str()) {
                // The step moved on (a kill counted): its fights start over, and so do its
                // catches.
                self.fought = None;
                self.catches = 0;
            }
            self.same_plan = 1;
            self.last_plan = Some(plan);
        }
        let max_same_plan = max_tries(step_drop_percent(&self.state_lines, &task));
        if self.same_plan > max_same_plan {
            let stop = format!(
                "{task}: the step did not end after the bot did it {max_same_plan} times ({line})"
            );
            let verdict = if matches!(step, PlanStep::Kill { .. }) {
                Fought::verdict(self.fought.as_ref(), &task, stop)
            } else {
                Verdict::Broken(stop)
            };
            self.end_quest(verdict);
            return self.next_quest(client, own, since);
        }
        if leaves_trial_first(&step, self.in_trial.as_deref())
            && let Some(mode) = self.in_trial.take()
        {
            self.nodes = level_nodes(&self.world_nodes, &self.trial_nodes, None);
            self.tried_nodes.clear();
            self.gather = Some((
                format!("{task}: left the instance of {mode}"),
                RunningSteps::of(leave_trial_steps()),
            ));
            self.changing_sim = true;
            self.phase = Phase::Gather;
            return Ok(());
        }
        // Any other step may take the bot off its mount (an interaction or a fight does): the
        // next course step summons it again (live 2026-11-22 13:37:07: the gather of
        // `09A_S11_BrazierA` came between two courses, and the second course never started).
        if !matches!(step, PlanStep::Course { .. }) {
            self.riding = false;
        }
        match step {
            PlanStep::Wait => self.pause(since, WAIT_PAUSE, Ask::State),
            PlanStep::Await { gatherable, at } => {
                let far = self.own_place().is_none_or(|own_at| {
                    planar([own_at.x, own_at.y, own_at.z], [at.x, at.y]) > AWAIT_REACH_M
                });
                if far {
                    self.teleport(client, own, at)?;
                    self.actions.push(format!(
                        "{task}: went to {:.0} {:.0} and waited for the start object {gatherable}",
                        at.x, at.y
                    ));
                }
                self.pause(since, ACTION_PAUSE, Ask::State);
            }
            PlanStep::Broken(reason) => {
                self.end_quest(Verdict::Broken(format!("{task}: {reason}")));
                return self.next_quest(client, own, since);
            }
            PlanStep::NotPlayed(reason)
                if !self.mission_turn_in_sent
                    && MISSION_TURN_IN_EVENTS
                        .iter()
                        .any(|event| reason.contains(event)) =>
            {
                let instances = self.world.instances_listing(&self.quest_tasks);
                let [instance] = instances[..] else {
                    self.end_quest(Verdict::NotPlayed(format!(
                        "{task}: {reason}; {} objectives of the own task list list the tasks the bot played, not one",
                        instances.len()
                    )));
                    return self.next_quest(client, own, since);
                };
                // A mission ends at its giver: the bot turns it in at the town board.
                self.mission_turn_in_sent = true;
                self.gather = Some((
                    format!("{task}: turned in the mission at the town board"),
                    RunningSteps::of(mission_turn_in_steps(instance)),
                ));
                self.phase = Phase::Gather;
            }
            PlanStep::NotPlayed(reason) => {
                self.end_quest(Verdict::NotPlayed(format!("{task}: {reason}")));
                return self.next_quest(client, own, since);
            }
            PlanStep::Client(calls) => {
                for call in &calls {
                    Self::call(client, call)?;
                }
                self.actions.push(format!("{task}: client calls"));
                self.pause(since, ACTION_PAUSE, Ask::State);
            }
            PlanStep::Talk { npc, at } => {
                if let Some(at) = at {
                    self.teleport(client, own, at)?;
                }
                self.talk(client, own, &npc)?;
                self.actions.push(format!("{task}: talked to {npc}"));
                self.pause(since, ACTION_PAUSE, Ask::State);
            }
            PlanStep::Topic { topic, talk } => {
                if let Some((npc, at)) = &talk {
                    self.teleport(client, own, *at)?;
                    self.talk(client, own, npc)?;
                }
                let finished = ConversationClientMsg::FinishedTopics {
                    header,
                    topics: vec![topic],
                };
                client.send_message(
                    MESSAGE_CHANNEL,
                    true,
                    finished.type_index(),
                    finished.encode(),
                )?;
                self.actions
                    .push(format!("{task}: finished topic {topic:08x}"));
                self.pause(since, ACTION_PAUSE, Ask::State);
            }
            PlanStep::Strike(places) => {
                let Some(&first) = places.first() else {
                    self.end_quest(Verdict::Broken(format!(
                        "{task}: a strike plan of no place"
                    )));
                    return self.next_quest(client, own, since);
                };
                self.teleport(client, own, first)?;
                self.strike = Some(Strikes {
                    task,
                    places,
                    next: 0,
                    swings: 0,
                });
                self.phase = Phase::Strike;
            }
            PlanStep::Go { at, territory } => {
                self.teleport(client, own, at)?;
                if let Some(territory) = territory {
                    self.enter(client, header, territory, at)?;
                }
                self.actions
                    .push(format!("{task}: went to {:.0} {:.0}", at.x, at.y));
                self.pause(since, ACTION_PAUSE, Ask::State);
            }
            PlanStep::Leave(territories) => {
                for territory in territories {
                    Self::call(client, &exited(header, territory))?;
                    self.entered_areas.retain(|(known, _)| *known != territory);
                }
                self.actions.push(format!("{task}: left its place"));
                self.pause(since, ACTION_PAUSE, Ask::State);
            }
            PlanStep::Kill {
                vitals,
                count,
                at,
                territory,
                placed,
                within,
                touch,
            } => {
                // A bot already in the encounter's fight area fights on where it stands: live
                // 2026-11-22 22:07:43, a new fight round put it back on the place of
                // `Enc_WorldEvent_09A_S17_WagonAttack` (z 219.4, on the wagon), each of its moves
                // to the ground was refused (`off_ground`) until 22:10, and the Hercyne it had
                // hit to 16,690 of 31,098 was never hit again.
                let area = within.map(|radius_m| (at, radius_m));
                if !stays_in_fight_area(area, self.own_place()) {
                    self.teleport(client, own, at)?;
                }
                if let Some(territory) = territory {
                    self.enter(client, header, territory, at)?;
                }
                if placed {
                    self.actions.push(format!(
                        "{task}: fought the placed {vitals} for {count} kills"
                    ));
                } else {
                    self.say(
                        client,
                        own,
                        &format!("!spawn {vitals} {SPAWN_LEVEL} {count}"),
                    )?;
                    self.actions
                        .push(format!("{task}: fought {count} spawned {vitals}"));
                }
                self.fight = Some(Fight {
                    task,
                    foe: false,
                    area,
                    vitals: Some(lowercase_crc32(&vitals)),
                    saw_mobs: false,
                    swings: 0,
                    hits_before: self.combat.hits_dealt(),
                    mob_deaths_before: self.combat.mob_deaths(),
                    hits_at_check: self.combat.hits_dealt(),
                    extensions: 0,
                    bundles_before: self.combat.bundles(),
                    mob_cards_before: self.combat.mob_cards(),
                    touch,
                    next_touch: 0,
                    swings_since_touch: SWINGS_PER_TOUCH,
                });
                self.phase = Phase::Fight {
                    until: since + FIGHT_TIME,
                };
            }
            PlanStep::Start {
                gatherable,
                asset,
                at,
            } => {
                self.teleport(client, own, at)?;
                // The object's card comes after the bot is near: the next plan finds it.
                let Some(object) = self.combat.card_of(&[asset]) else {
                    self.actions
                        .push(format!("{task}: went to the start object {gatherable}"));
                    self.pause(since, ACTION_PAUSE, Ask::State);
                    return Ok(());
                };
                self.gather = Some((
                    format!("{task}: gathered the start object {gatherable}"),
                    RunningSteps::of(start_object_steps(object, lowercase_crc32(&gatherable))),
                ));
                self.phase = Phase::Gather;
            }
            PlanStep::Turn { gatherable, count } => {
                let own_at = self.own_place();
                let crc = lowercase_crc32(&gatherable);
                let Some(index) = own_at.and_then(|at| {
                    nearest_untried(&self.nodes, &HashSet::new(), crc, [at.x, at.y, at.z])
                }) else {
                    self.end_quest(Verdict::Broken(format!(
                        "{task}: the level has no {gatherable} node to turn"
                    )));
                    return self.next_quest(client, own, since);
                };
                let steps = gather_steps(&gatherable, self.nodes[index], index);
                self.gather = Some((
                    format!("{task}: turned {gatherable} node {index} ({count} to go)"),
                    RunningSteps::of(steps),
                ));
                self.phase = Phase::Gather;
            }
            PlanStep::Item {
                node: Some(node), ..
            } => {
                let Some(index) = self.untried_node(lowercase_crc32(&node)) else {
                    self.end_quest(Verdict::Broken(format!(
                        "{task}: the bot gathered each {node} node of the level and the step did not end"
                    )));
                    return self.next_quest(client, own, since);
                };
                self.tried_nodes.insert(index);
                // A drop that counts in one place only (a `POITag`) needs the bot in that area,
                // as a client that walked in has told the server.
                if let Some(territory) = self.step_area(&task) {
                    let [x, y, z] = self.nodes[index].at;
                    let at = Position { x, y, z };
                    self.teleport(client, own, at)?;
                    self.enter(client, header, territory, at)?;
                }
                let steps = gather_steps(&node, self.nodes[index], index);
                self.gather = Some((
                    format!("{task}: gathered {node} node {index}"),
                    RunningSteps::of(steps),
                ));
                self.phase = Phase::Gather;
            }
            PlanStep::Item {
                item,
                source,
                gathered: true,
                node: None,
                ..
            } => {
                let container = (source == CONTAINER_DROP)
                    .then(|| self.untried_container())
                    .flatten();
                if let Some(index) = container {
                    // The item drops from the loot containers of the step's place: the bot
                    // opens the nearest one, in that area, as a player who walked in does.
                    self.tried_nodes.insert(index);
                    let node = self.nodes[index];
                    let [x, y, z] = node.at;
                    let at = Position { x, y, z };
                    self.teleport(client, own, at)?;
                    if let Some(territory) = self.step_area(&task) {
                        self.enter(client, header, territory, at)?;
                    }
                    self.gather = Some((
                        format!("{task}: opened loot container node {index}"),
                        RunningSteps::of(gather_steps(CONTAINER_NAME, node, index)),
                    ));
                    self.phase = Phase::Gather;
                    return Ok(());
                }
                // A game master's give is no gather: the step would never count it.
                self.end_quest(Verdict::NotPlayed(format!(
                    "{task}: the step counts gathers of {item} ({source}), and the bot has no gather for it"
                )));
                return self.next_quest(client, own, since);
            }
            PlanStep::Item {
                item,
                count,
                source,
                node: None,
                ..
            } => {
                self.say(client, own, &format!("!give {item} {count}"))?;
                self.actions.push(format!(
                    "{task}: given {count} {item} by a game master ({source})"
                ));
                self.pause(since, ACTION_PAUSE, Ask::State);
            }
            PlanStep::Use {
                tag,
                object,
                count,
                at,
            } => {
                if !tag.eq_ignore_ascii_case(STORAGE_TAG) {
                    self.end_quest(Verdict::NotPlayed(format!(
                        "{task}: the interaction kind of an object with interact tag {tag} is Unknown"
                    )));
                    return self.next_quest(client, own, since);
                }
                let steps = use_steps(
                    object,
                    [at.x, at.y, at.z],
                    OPEN_STORAGE_KIND,
                    OPEN_STORAGE_NAME,
                );
                self.gather = Some((
                    format!("{task}: opened and closed storage {object:016x} ({count} left)"),
                    RunningSteps::of(steps),
                ));
                self.phase = Phase::Gather;
            }
            PlanStep::Song {
                song,
                sheet,
                duration_s,
                at,
            } => {
                self.gather = Some((
                    format!("{task}: played song {song} from sheet {sheet}"),
                    RunningSteps::of(song_steps(
                        lowercase_crc32(&song.to_lowercase()),
                        lowercase_crc32(&sheet.to_lowercase()),
                        duration_s,
                        [at.x, at.y, at.z],
                    )),
                ));
                self.phase = Phase::Gather;
            }
            PlanStep::UseItem { item, bag_slot } => {
                let call = ItemCallMsg::UseItem {
                    header: FacetHeader::of(CALL_COMPONENT, own),
                    bag_slot,
                };
                client.send_message(MESSAGE_CHANNEL, true, call.type_index(), call.encode()?)?;
                self.actions
                    .push(format!("{task}: used {item} from bag slot {bag_slot}"));
                self.pause(since, ACTION_PAUSE, Ask::State);
            }
            PlanStep::Pvp { count } => {
                let Some(foe) = self.pvp_foe.clone() else {
                    self.end_quest(Verdict::NotPlayed(format!(
                        "{task}: {count} player kills need a foe bot (--pvp-foe <character>)"
                    )));
                    return self.next_quest(client, own, since);
                };
                if !self.pvp_armed {
                    self.arming_pvp = true;
                    self.gather = Some((
                        format!("{task}: put the PvP flag on"),
                        RunningSteps::of(pvp_flag_steps(PVP_KILLER_FACTION)),
                    ));
                    self.phase = Phase::Gather;
                    return Ok(());
                }
                // The foe stands where it is: a game master's teleport to it.
                self.say(client, own, &format!("!tp {foe}"))?;
                self.actions
                    .push(format!("{task}: fought the foe {foe} for {count} kills"));
                self.fight = Some(Fight {
                    task,
                    foe: true,
                    area: None,
                    vitals: None,
                    saw_mobs: false,
                    swings: 0,
                    hits_before: self.combat.hits_dealt(),
                    mob_deaths_before: self.combat.mob_deaths(),
                    hits_at_check: self.combat.hits_dealt(),
                    extensions: 0,
                    bundles_before: self.combat.bundles(),
                    mob_cards_before: self.combat.mob_cards(),
                    touch: Vec::new(),
                    next_touch: 0,
                    swings_since_touch: SWINGS_PER_TOUCH,
                });
                self.phase = Phase::Fight {
                    until: since + FIGHT_TIME,
                };
            }
            PlanStep::Fish { at, yaw, territory } => {
                self.teleport(client, own, at)?;
                if let Some(territory) = territory {
                    self.enter(client, header, territory, at)?;
                }
                self.fishing = Some(Fishing {
                    what: format!(
                        "{task}: fished at {:.0} {:.0} {:.0} (yaw {yaw:.2})",
                        at.x, at.y, at.z
                    ),
                    angler: Angler::new(since, self.combat.fishing_steps().len()),
                    yaw,
                });
                self.phase = Phase::Fish;
            }
            PlanStep::Course { course, section } => {
                if !self.riding {
                    let reward_box = match &self.mount_reward_box {
                        Ok(reward_box) => *reward_box,
                        Err(why) => {
                            self.end_quest(Verdict::NotPlayed(format!(
                                "{task}: course {course} needs a mount: {why}"
                            )));
                            return self.next_quest(client, own, since);
                        }
                    };
                    // A course starts once its rider is on a mount (module doc of the sim's
                    // `world/quick_course.rs`; retail `0334_0102_towarmthesoulwardens` summoned
                    // the horse 5.4 s after the accept, before the course).
                    self.mounting = true;
                    self.gather = Some((
                        format!("{task}: summoned the mount for course {course}"),
                        RunningSteps::of(mount_steps(reward_box)),
                    ));
                    self.phase = Phase::Gather;
                    return Ok(());
                }
                let Some(nodes) = self.courses.nodes(&course) else {
                    self.end_quest(Verdict::Broken(format!(
                        "{task}: no node path of quick course {course}"
                    )));
                    return self.next_quest(client, own, since);
                };
                // A section step counts once its section is passed; the ride starts at the
                // course's start, as a course always does.
                let whole = section == 0;
                self.gather = Some((
                    format!(
                        "{task}: rode course {course} ({} nodes, section {section})",
                        nodes.len()
                    ),
                    RunningSteps::of(course_steps(&nodes, whole)),
                ));
                self.phase = Phase::Gather;
            }
            PlanStep::Lore { title, at } => {
                self.gather = Some((
                    format!("{task}: read the lore note {title}"),
                    RunningSteps::of(lore_steps(&title, [at.x, at.y, at.z])),
                ));
                self.phase = Phase::Gather;
            }
            PlanStep::Trial { mode, crc, door } => {
                let steps = match door {
                    Some((capital, at)) => {
                        door_trial_steps(&mode, crc, capital, at.map(|at| [at.x, at.y, at.z]))
                    }
                    None => trial_steps(&mode, crc),
                };
                self.gather = Some((
                    format!("{task}: entered the instance of {mode}"),
                    RunningSteps::of(steps),
                ));
                self.entering_trial = Some(mode);
                self.changing_sim = true;
                self.phase = Phase::Gather;
            }
            PlanStep::Camp {
                at,
                skin,
                asset,
                costs,
            } => {
                self.gather = Some((
                    format!("{task}: placed and built a camp"),
                    RunningSteps::of(camp_steps([at.x, at.y, at.z], skin, asset, &costs)),
                ));
                self.phase = Phase::Gather;
            }
            PlanStep::Skin {
                gatherable,
                count,
                kill,
            } => {
                let Some((corpse, died_at)) = self.fresh_corpse(&kill) else {
                    return self.ask(client, own, Ask::State, since);
                };
                self.skinned.insert(corpse);
                let steps = skin_steps(
                    raw_id_of_entity(corpse),
                    lowercase_crc32(&gatherable),
                    [died_at.x, died_at.y, died_at.z()],
                );
                self.gather = Some((
                    format!("{task}: skinned the {gatherable} corpse ({count} left)"),
                    RunningSteps::of(steps),
                ));
                self.phase = Phase::Gather;
            }
            PlanStep::Craft {
                recipe,
                at,
                station,
                ingredients,
                missing,
            } => {
                for (item, count) in &missing {
                    self.say(client, own, &format!("!give {item} {count}"))?;
                    self.actions.push(format!(
                        "{task}: given {count} {item} for the craft by a game master"
                    ));
                }
                self.gather = Some((
                    format!("{task}: crafted {recipe} at its station"),
                    RunningSteps::of(quest_craft_steps(
                        &recipe,
                        station,
                        [at.x, at.y, at.z],
                        ingredients,
                    )),
                ));
                self.phase = Phase::Gather;
            }
            PlanStep::Loot { .. } => {
                let bags: Vec<[u8; nw_messages::wire::RAW_ID_LEN]> = self.combat.bags()
                    [self.loot_from..]
                    .iter()
                    .rev()
                    .copied()
                    .collect();
                self.loot_from = self.combat.bags().len();
                self.gather = Some((
                    format!("{task}: opened {} loot bags of its kills", bags.len()),
                    RunningSteps::of(loot_steps(&bags)),
                ));
                self.phase = Phase::Gather;
            }
            PlanStep::Equip {
                item,
                bag_slot,
                instance,
                slot,
            } => {
                Self::call(
                    client,
                    &ClientFacetMsg::EquipItem {
                        header,
                        bag_slot,
                        equip_slot: slot,
                        quantity: 1,
                        instance,
                        flag: 0,
                    },
                )?;
                self.actions.push(format!(
                    "{task}: equipped {item} from bag slot {bag_slot} into slot {slot}"
                ));
                self.pause(since, ACTION_PAUSE, Ask::State);
            }
            PlanStep::Achievements { needs, sources } => {
                let first: Vec<String> = sources
                    .into_iter()
                    .filter(|quest| !self.played.contains(&quest.to_lowercase()))
                    .collect();
                if first.is_empty() {
                    self.end_quest(Verdict::NotPlayed(format!(
                        "{task}: needs achievements no quest the bot can play gives: {needs}"
                    )));
                    return self.next_quest(client, own, since);
                }
                // The quests that give the achievements go first; this one waits in the journal.
                self.actions.push(format!(
                    "{task}: needs {needs}; plays {} first",
                    first.join(", ")
                ));
                let current = self.current().to_owned();
                self.queue.push_front(current);
                for quest in first.into_iter().rev() {
                    self.queue.push_front(quest);
                }
                self.current = None;
                return self.advance(client, own, since);
            }
            PlanStep::Progression { id, target, points } => {
                let line = if points {
                    format!("!masteryxp {id} {target}")
                } else {
                    format!("!mastery {id} {target}")
                };
                self.say(client, own, &line)?;
                self.actions.push(format!("{task}: {line}"));
                self.pause(since, ACTION_PAUSE, Ask::State);
            }
        }
        Ok(())
    }

    /// Plays the activities: each step's acts one at a time, [`ACT_GAP`] apart, then its proof.
    fn activity_tick(
        &mut self,
        client: &mut RepClient,
        outcome: &RepOutcome,
        own: u64,
        since: Duration,
    ) -> Result<(), RepError> {
        if self.running.is_none() {
            let Some(activity) = self.activities.pop_front() else {
                // The quests after activities that came first (none left: the play is done).
                return self.ask(client, own, Ask::State, since);
            };
            match activity.steps {
                Ok(steps) => self.running = Some((activity.kind, RunningSteps::of(steps))),
                Err(why) => {
                    self.activity_plays.push(QuestPlay {
                        quest: format!("{:?}", activity.kind),
                        verdict: Verdict::NotPlayed(why),
                        actions: Vec::new(),
                    });
                    return Ok(());
                }
            }
        }
        let Some((kind, mut running)) = self.running.take() else {
            return Ok(());
        };
        if self.hunting {
            self.fight(client, own, since)?;
        }
        match self.steps_tick(client, outcome, own, since, &mut running)? {
            Some(verdict) => self.activity_plays.push(QuestPlay {
                quest: format!("{kind:?}"),
                verdict,
                actions: running.proved,
            }),
            None => self.running = Some((kind, running)),
        }
        Ok(())
    }

    /// Plays the gather of a quest step's node; once it ended, notes how and asks where the
    /// quest stands.
    fn gather_tick(
        &mut self,
        client: &mut RepClient,
        outcome: &RepOutcome,
        own: u64,
        since: Duration,
    ) -> Result<(), RepError> {
        let Some((what, mut running)) = self.gather.take() else {
            return self.ask(client, own, Ask::State, since);
        };
        match self.steps_tick(client, outcome, own, since, &mut running)? {
            Some(Verdict::Completed) => {
                if let Some(mode) = self.entering_trial.take() {
                    self.nodes = level_nodes(&self.world_nodes, &self.trial_nodes, Some(&mode));
                    self.tried_nodes.clear();
                    self.in_trial = Some(mode);
                }
                if std::mem::take(&mut self.mounting) {
                    self.riding = true;
                }
                if std::mem::take(&mut self.arming_pvp) {
                    self.pvp_armed = true;
                }
                self.actions.push(what);
                if let Some(phase) = phase_after_sim_change(std::mem::take(&mut self.changing_sim))
                {
                    self.phase = phase;
                    return Ok(());
                }
            }
            Some(Verdict::Broken(why) | Verdict::NotPlayed(why)) => {
                self.entering_trial = None;
                self.changing_sim = false;
                self.mounting = false;
                self.arming_pvp = false;
                self.actions.push(format!("{what}: {why}"));
            }
            None => {
                self.gather = Some((what, running));
                return Ok(());
            }
        }
        self.ask(client, own, Ask::State, since)
    }

    /// Plays one tick of the fishing of a catch step ([`Angler`]): its keys go out in the bot's
    /// input, looking along the fishing yaw, and its acknowledgements as 5339; once it caught a
    /// fish or gave up, notes how and asks where the quest stands.
    fn fish_tick(
        &mut self,
        client: &mut RepClient,
        own: u64,
        since: Duration,
    ) -> Result<(), RepError> {
        let Some(mut fishing) = self.fishing.take() else {
            return self.ask(client, own, Ask::State, since);
        };
        match fishing.angler.tick(self.combat.fishing_steps(), since) {
            Ok(acts) => {
                for act in acts {
                    self.fish_act(client, own, &mut fishing, act)?;
                }
                self.fishing = Some(fishing);
                Ok(())
            }
            Err(end) => {
                self.caught_last = end == FishEnd::Caught;
                self.catches += u32::from(self.caught_last);
                self.actions.push(match end {
                    FishEnd::Caught => format!("{}: caught a fish", fishing.what),
                    FishEnd::GaveUp(why) => format!("{}: {why}", fishing.what),
                });
                self.ask(client, own, Ask::State, since)
            }
        }
    }

    /// Whether the bot fishes on for a catch step whose plan did not change: its last run caught
    /// a fish that was not the step's (a quest fish bites at its chance, for sure after its
    /// `GuranteedDropAttempts`: retail `16_side_11_feedifrit` caught it at the sixth catch),
    /// within [`MAX_CATCHES`].
    const fn fishes_on(&self) -> bool {
        self.caught_last && self.catches < MAX_CATCHES
    }

    /// Sends fishing act `act` of `fishing`.
    fn fish_act(
        &mut self,
        client: &mut RepClient,
        own: u64,
        fishing: &mut Fishing,
        act: FishAct,
    ) -> Result<(), RepError> {
        let counter = self.input_counter;
        let input = |position| match act {
            FishAct::FishingKey => Some((ClientInputMsg::fishing_key(position, counter), 2)),
            FishAct::Hold { pressed } => Some((
                ClientInputMsg::attack_holding(position, counter, pressed),
                1,
            )),
            FishAct::LetGo => Some((ClientInputMsg::attack_letting_go(position, counter), 1)),
            FishAct::Turn(_) | FishAct::Acknowledge(_) => None,
        };
        match act {
            FishAct::Turn(radians) => fishing.yaw += radians,
            FishAct::Acknowledge(next_sequence) => {
                let call = FishingClientMsg::StateReceived {
                    header: FacetHeader::of(FISHING_FACET, own),
                    next_sequence,
                };
                client.send_message(MESSAGE_CHANNEL, true, call.type_index(), call.encode())?;
            }
            FishAct::FishingKey | FishAct::Hold { .. } | FishAct::LetGo => {
                if let Some((message, records)) = self.own_input_position().and_then(input) {
                    self.input_counter = counter.wrapping_add(records);
                    send_input(client, &message.looking([fishing.yaw, LEVEL_PITCH]))?;
                }
            }
        }
        Ok(())
    }

    /// The newest mob of skin step kill `kill` (a [`PlanStep::Kill`]) that died in view, while
    /// its corpse is shown and the bot has not skinned it; `None` for another step.
    fn fresh_corpse(&self, kill: &PlanStep) -> Option<(u64, nw_messages::WorldPosition)> {
        let PlanStep::Kill { vitals, .. } = kill else {
            return None;
        };
        self.combat
            .corpses_of(lowercase_crc32(vitals))
            .find(|(corpse, _)| !self.skinned.contains(corpse))
    }

    /// The nearest loot container node within [`CONTAINER_SEARCH_M`] of the bot that it has not
    /// tried yet: the bot stands in the step's place after its go-to step.
    fn untried_container(&self) -> Option<usize> {
        let own = self.own_place()?;
        self.nodes
            .iter()
            .enumerate()
            .filter(|(index, node)| {
                self.containers.contains(&node.gatherable) && !self.tried_nodes.contains(index)
            })
            .map(|(index, node)| {
                (
                    index,
                    planar([own.x, own.y, own.z], [node.at[0], node.at[1]]),
                )
            })
            .filter(|(_, distance)| *distance <= CONTAINER_SEARCH_M)
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(index, _)| index)
    }

    /// The area of the place in which task `task` of the running quest counts
    /// ([`step_place`], [`area_of_place`]).
    fn step_area(&self, task: &str) -> Option<u16> {
        area_of_place(&self.poi_tags, step_place(&self.state_lines, task)?)
    }

    /// The nearest node of gatherable `gatherable` to the bot that it has not tried yet.
    fn untried_node(&self, gatherable: u32) -> Option<usize> {
        let own = self.own_place()?;
        nearest_untried(
            &self.nodes,
            &self.tried_nodes,
            gatherable,
            [own.x, own.y, own.z],
        )
    }

    /// Sends interaction `request` with the name `name` in the bot's input, with a fresh
    /// interaction id (and the bot's own entity as its target entity when it names none);
    /// returns the id, whose answer the step waits for.
    fn interact(
        &mut self,
        client: &mut RepClient,
        own: u64,
        request: InteractionRequest,
        name: &[u8],
    ) -> Result<u64, RepError> {
        self.interaction += 1;
        let request = InteractionRequest {
            interaction: self.interaction,
            target_entity: if request.target_entity == 0 {
                own
            } else {
                request.target_entity
            },
            ..request
        };
        if let Some(position) = self.own_input_position() {
            let body = Bit8::of_interaction(request, name)?;
            send_input(
                client,
                &ClientInputMsg::position(position).interacting(body),
            )?;
        }
        Ok(self.interaction)
    }

    /// Plays one tick of `running`; its verdict once it ended.
    fn steps_tick(
        &mut self,
        client: &mut RepClient,
        outcome: &RepOutcome,
        own: u64,
        since: Duration,
        running: &mut RunningSteps,
    ) -> Result<Option<Verdict>, RepError> {
        let Some(step) = running.steps.get(running.step) else {
            return Ok(Some(Verdict::Completed));
        };
        if running
            .last_act
            .is_some_and(|last| since.saturating_sub(last) < ACT_GAP)
        {
            return Ok(None);
        }
        if let Some(act) = step.acts.get(running.act) {
            running.marks.mark(
                self.lines.len(),
                outcome.messages.len(),
                self.answers.len(),
                self.heard.len(),
            );
            match act {
                Act::Gm(line) => self.say(client, own, line)?,
                Act::PvpFlagOn => {
                    let shown = self
                        .lines
                        .iter()
                        .rev()
                        .find(|line| line.starts_with(STATE_LINE))
                        .is_some_and(|line| pvp_flag_shown(line));
                    if !shown {
                        Self::call(
                            client,
                            &ClientFacetMsg::TogglePvpFlag {
                                header: FacetHeader::of(CALL_COMPONENT, own),
                            },
                        )?;
                    }
                }
                Act::SummonKey => {
                    if let Some(position) = self.own_input_position() {
                        let counter = self.input_counter;
                        self.input_counter = counter.wrapping_add(ATTACK_RECORDS);
                        send_input(client, &ClientInputMsg::summon_mount(position, counter))?;
                    }
                }
                Act::OpenBagItem(item_index) => {
                    if let Some((bag_slot, instance)) = self.combat.bag_slot_of(*item_index) {
                        Self::call(
                            client,
                            &ClientFacetMsg::InitiateSalvage {
                                header: FacetHeader::of(CALL_COMPONENT, own),
                                first: SALVAGE_FIRST,
                                bag_slot,
                                instance,
                                unknown: SALVAGE_UNKNOWN,
                                quantity: SALVAGE_ONE,
                            },
                        )?;
                    }
                }
                Act::Pause(wait) => {
                    running.act += 1;
                    running.last_act = Some((since + *wait).saturating_sub(ACT_GAP));
                    return Ok(None);
                }
                Act::Teleport([x, y, z]) => self.teleport(
                    client,
                    own,
                    Position {
                        x: *x,
                        y: *y,
                        z: *z,
                    },
                )?,
                Act::Call(make) => {
                    let (type_index, body) = make(own)?;
                    client.send_message(MESSAGE_CHANNEL, true, type_index, body)?;
                }
                Act::Interact { request, name } => {
                    running.interaction = Some(self.interact(client, own, *request, name)?);
                }
                Act::InteractCard { assets, kind } => {
                    // The card may come a few bundles after the teleport: wait for it.
                    let Some(target) = self.combat.card_of(assets) else {
                        return Ok(None);
                    };
                    let request = InteractionRequest {
                        target,
                        target_entity: facet_of_raw_id(&target),
                        interaction: 0,
                        kind: *kind,
                        gatherable: None,
                    };
                    running.interaction = Some(self.interact(client, own, request, &[])?);
                }
                Act::InviteGroup(seat) => {
                    let Some(members) = seat.members_ready() else {
                        return Ok(None);
                    };
                    for character in members {
                        let (type_index, body) = invite_call(character, own)?;
                        client.send_message(MESSAGE_CHANNEL, true, type_index, body)?;
                    }
                }
                // A bot already in a group (of an earlier run: the social service keeps it) has
                // no invite to answer.
                Act::AcceptGroupInvite(seat) if self.world.own_group().is_none() => {
                    seat.set_ready();
                    let Some(invite) = self.world.inbound_group_invite() else {
                        return Ok(None);
                    };
                    let (type_index, body) = accept_call(invite, own)?;
                    client.send_message(MESSAGE_CHANNEL, true, type_index, body)?;
                }
                Act::AcceptGroupInvite(seat) => seat.set_ready(),
                Act::LeaderInside(seat) => seat.set_leader_inside(),
                Act::Walk {
                    seat,
                    east_m,
                    north_m,
                    records,
                } => {
                    if !self.walk(client, seat, [*east_m, *north_m], records, since)? {
                        return Ok(None);
                    }
                }
                Act::AcceptPops { longest } => {
                    let accepting = self.accepting.get_or_insert(Accepting {
                        started: since,
                        accepted: None,
                    });
                    let moved = outcome.messages[running.marks.messages..]
                        .iter()
                        .any(|received| {
                            received.envelope.type_index()
                                == Some(MATCHMAKING_LOGIN_REQUESTED_TYPE_INDEX)
                        });
                    if !moved {
                        if since.saturating_sub(accepting.started) > *longest {
                            self.accepting = None;
                            return Ok(Some(Verdict::Broken(format!(
                                "{}: no move (5449) within {} s",
                                step.what,
                                longest.as_secs()
                            ))));
                        }
                        if let Some(match_id) =
                            pop_to_accept(self.world.matchmaking(), accepting.accepted.as_deref())
                        {
                            for (type_index, body) in accept_calls(own, &match_id)? {
                                client.send_message(MESSAGE_CHANNEL, true, type_index, body)?;
                            }
                            accepting.accepted = Some(match_id);
                        }
                        return Ok(None);
                    }
                    self.accepting = None;
                }
                Act::PlayMatch { end, goal, longest } => {
                    if self.match_play.is_none() {
                        // The bot stands where the server put it in the match: its last teleport
                        // was in the level it left.
                        self.teleported_to = None;
                        self.match_play = Some(MatchPlay::new(since));
                    }
                    let started = self.match_play.as_ref().map_or(since, |play| play.started);
                    if since.saturating_sub(started) > *longest {
                        self.match_play = None;
                        return Ok(Some(Verdict::Broken(format!(
                            "{}: the match did not end within {} s",
                            step.what,
                            longest.as_secs()
                        ))));
                    }
                    let ended = match end {
                        MatchEnd::LeftMode(mode) => !self.world.matchmaking().active.contains(mode),
                        MatchEnd::WarEnded => outcome.messages[running.marks.messages..]
                            .iter()
                            .any(|received| {
                                received.envelope.type_index()
                                    == Some(SIEGE_WARFARE_ENDED_TYPE_INDEX)
                            }),
                    };
                    if !ended {
                        self.match_tick(client, own, goal, since)?;
                        return Ok(None);
                    }
                    self.match_play = None;
                }
                Act::InteractDynamic(request) => {
                    let Some(request) = request(&self.sight(outcome, own, running)) else {
                        return Ok(None);
                    };
                    running.interaction = Some(self.interact(client, own, request, &[])?);
                }
                Act::Hunt(Some(radius_m)) => {
                    let Some(center) = self.own_place() else {
                        return Ok(None);
                    };
                    self.fight = Some(Fight {
                        task: "hunt".to_owned(),
                        foe: false,
                        area: Some((center, *radius_m)),
                        vitals: None,
                        saw_mobs: false,
                        swings: 0,
                        hits_before: self.combat.hits_dealt(),
                        mob_deaths_before: self.combat.mob_deaths(),
                        hits_at_check: self.combat.hits_dealt(),
                        extensions: 0,
                        bundles_before: self.combat.bundles(),
                        mob_cards_before: self.combat.mob_cards(),
                        touch: Vec::new(),
                        next_touch: 0,
                        swings_since_touch: SWINGS_PER_TOUCH,
                    });
                    self.hunting = true;
                }
                Act::Hunt(None) => {
                    self.hunting = false;
                    self.fight = None;
                }
                Act::Dynamic(calls) => {
                    let Some(calls) = calls(&self.sight(outcome, own, running)) else {
                        return Ok(None);
                    };
                    for (type_index, body) in calls? {
                        client.send_message(MESSAGE_CHANNEL, true, type_index, body)?;
                    }
                }
                Act::EndInteraction { target_entity } => {
                    if let Some(interaction) = running.interaction {
                        Self::call(
                            client,
                            &ClientFacetMsg::CancelInteraction {
                                header: FacetHeader::of(CALL_COMPONENT, own),
                                target: [0; nw_messages::wire::RAW_ID_LEN],
                                target_entity: *target_entity,
                                interaction,
                            },
                        )?;
                    }
                }
            }
            running.act += 1;
            running.last_act = Some(since);
            return Ok(None);
        }
        let started = running.last_act.unwrap_or(since);
        let new_answers = &self.answers[running.marks.answers..];
        let proof = match &step.proof {
            Proof::Sent => Some(Ok(())),
            Proof::Line(takes, _) => self.lines[running.marks.lines..]
                .iter()
                .any(|line| takes(line))
                .then_some(Ok(())),
            Proof::Message(type_index) => outcome.messages[running.marks.messages..]
                .iter()
                .any(|received| received.envelope.type_index() == Some(*type_index))
                .then_some(Ok(())),
            Proof::ArenaYes(type_index) => outcome.messages[running.marks.messages..]
                .iter()
                .find(|received| received.envelope.type_index() == Some(*type_index))
                .map(|received| {
                    arena_verdict(*type_index, &received.envelope.body)
                        .map_err(|why| format!("{}: {why}", step.what))
                }),
            Proof::Accepted => new_answers.iter().find_map(|answer| match answer {
                InteractionAnswer::Validated {
                    interaction,
                    status,
                } if Some(*interaction) == running.interaction => Some(if *status == ACCEPTED {
                    Ok(())
                } else {
                    Err(format!("{}: refused (status {status})", step.what))
                }),
                _ => None,
            }),
            Proof::Ended => new_answers
                .contains(&InteractionAnswer::Ended)
                .then_some(Ok(())),
            Proof::Answered => new_answers
                .iter()
                .any(|answer| {
                    matches!(answer, InteractionAnswer::Validated { interaction, .. }
                        if Some(*interaction) == running.interaction)
                })
                .then_some(Ok(())),
            Proof::GroupFull(seat) => seat.others_joined().then_some(Ok(())),
            Proof::InGroup(seat) => self.world.own_group().map(|_| {
                seat.set_joined();
                Ok(())
            }),
            Proof::LeaderInside(seat) => seat.leader_inside().then_some(Ok(())),
            Proof::LeaderDone(seat) => seat.leader_done().then_some(Ok(())),
            Proof::Paused => (since.saturating_sub(started) >= step.wait).then_some(Ok(())),
            Proof::Check(check, _) => check(&self.sight(outcome, own, running))
                .map(|checked| checked.map_err(|why| format!("{}: {why}", step.what))),
        };
        match proof {
            Some(Ok(())) => {
                running.proved.push(step.what.clone());
                running.next_step();
                Ok(None)
            }
            Some(Err(why)) => Ok(Some(Verdict::Broken(why))),
            None if since.saturating_sub(started) > step.wait => {
                let wanted = match &step.proof {
                    Proof::Line(_, described) => format!("an answer line: {described}"),
                    Proof::Message(type_index) => format!("a {type_index} message"),
                    Proof::ArenaYes(type_index) => format!("a {type_index} answer that says yes"),
                    Proof::Accepted => "the interaction's answer".to_owned(),
                    Proof::Ended => "the interaction's end".to_owned(),
                    Proof::Answered => "an answer to the interaction".to_owned(),
                    Proof::Sent => String::new(),
                    Proof::GroupFull(_) => "every other bot in the group".to_owned(),
                    Proof::InGroup(_) => "a group in the own groups state".to_owned(),
                    Proof::LeaderInside(_) => "the leader in the instance".to_owned(),
                    Proof::LeaderDone(_) => "the end of the leader's play".to_owned(),
                    Proof::Paused => "the end of the pause".to_owned(),
                    Proof::Check(_, described) => described.clone(),
                };
                let answered = self.lines[running.marks.lines..].join(" / ");
                Ok(Some(Verdict::Broken(format!(
                    "{}: no proof within {} s (waited for {wanted}; the server answered: {})",
                    step.what,
                    step.wait.as_secs(),
                    if answered.is_empty() {
                        "nothing"
                    } else {
                        &answered
                    }
                ))))
            }
            None => Ok(None),
        }
    }

    /// What the bot saw since the first act of the running step ([`BotSight`]).
    fn sight<'a>(
        &'a self,
        outcome: &'a RepOutcome,
        own: u64,
        running: &RunningSteps,
    ) -> BotSight<'a> {
        BotSight {
            own,
            bag: self.combat.bag(),
            lines: &self.lines[running.marks.lines..],
            heard: &self.heard[running.marks.heard..],
            messages: &outcome.messages[running.marks.messages..],
            dead: self.combat.own_dead(),
            in_group: self.world.own_group().is_some(),
            matchmaking: self.world.matchmaking(),
            group_invite: self.world.inbound_group_invite(),
        }
    }

    /// One frame of the walk to `offset` (east, north) from the first server place of `seat`
    /// with input records of `records` ([`RecordWalker`]), one every [`SEND_INTERVAL`]; whether
    /// the bot's own body is there. The server sends no own place for the bot's own moves: the
    /// bot stands where its own body walked.
    fn walk(
        &mut self,
        client: &mut RepClient,
        seat: &GroupSeat,
        offset: [f32; 2],
        records: &RecordWalk,
        since: Duration,
    ) -> Result<bool, RepError> {
        let Some([x, y, z]) = seat.first_place() else {
            return Ok(true);
        };
        let walker = self
            .walker
            .get_or_insert_with(|| RecordWalker::new(Position { x, y, z }, records.device));
        let goal = [x + offset[0], y + offset[1]];
        let at = walker.position();
        if planar([at.x, at.y, at.z], goal) <= ARRIVED_M {
            return Ok(true);
        }
        if self
            .last_move
            .is_some_and(|last| since.saturating_sub(last) < SEND_INTERVAL)
        {
            return Ok(false);
        }
        self.last_move = Some(since);
        let input = walker.frame(
            goal,
            SEND_INTERVAL.as_secs_f32(),
            &*records.ground,
            records.sea_level_m,
        );
        self.teleported_to = Some(walker.position());
        match input {
            Some(input) => {
                send_input(client, &input)?;
                Ok(false)
            }
            None => Ok(true),
        }
    }

    /// Moves the bot to `at` with a game master line. It first leaves each area it came into
    /// farther than [`AREA_LEFT_M`] from `at`: a client tells the server the areas it leaves.
    fn teleport(&mut self, client: &mut RepClient, own: u64, at: Position) -> Result<(), RepError> {
        let header = FacetHeader::of(CALL_COMPONENT, own);
        let (left, kept) = self
            .entered_areas
            .drain(..)
            .partition(|(_, entered_at)| entered_at.horizontal_distance(at) > AREA_LEFT_M);
        self.entered_areas = kept;
        for (territory, _) in left {
            Self::call(client, &exited(header, territory))?;
        }
        self.teleported_to = Some(at);
        self.say(client, own, &teleport_line(at))
    }

    /// Tells the server the bot came into area `territory` (2297) at `at`, the place of its
    /// last teleport.
    fn enter(
        &mut self,
        client: &mut RepClient,
        header: FacetHeader,
        territory: u16,
        at: Position,
    ) -> Result<(), RepError> {
        Self::call(client, &entered(header, territory))?;
        self.entered_areas.retain(|(known, _)| *known != territory);
        self.entered_areas.push((territory, at));
        Ok(())
    }

    /// Where the bot stands: the place of its last teleport until the server's own state shows
    /// it there (an input at the old place would move it back), else the place it last saw.
    fn own_place(&self) -> Option<Position> {
        let seen = self.world.own_position().map(|own| Position {
            x: own.x,
            y: own.y,
            z: own.z(),
        });
        match (self.teleported_to, seen) {
            (Some(target), Some(seen))
                if target.horizontal_distance(seen) <= TELEPORT_SETTLED_M =>
            {
                Some(seen)
            }
            (Some(target), _) => Some(target),
            (None, seen) => seen,
        }
    }

    /// Talks to NPC `npc` with a talk interaction in the bot's input.
    fn talk(&mut self, client: &mut RepClient, own: u64, npc: &str) -> Result<(), RepError> {
        self.interaction += 1;
        let request = InteractionRequest {
            target: [0; nw_messages::wire::RAW_ID_LEN],
            target_entity: own,
            interaction: self.interaction,
            kind: TALK_KIND,
            gatherable: None,
        };
        let name = format!("{NPC_MARK}{npc}");
        let Some(position) = self.own_input_position() else {
            return Ok(());
        };
        let body = Bit8::of_interaction(request, name.as_bytes())?;
        send_input(
            client,
            &ClientInputMsg::position(position).interacting(body),
        )
    }

    /// Steps [`AFK_STEP_M`] east of where the bot stands, as a player does at the AFK warning;
    /// a fight walks it back to its mob.
    fn step_aside(&mut self, client: &mut RepClient) -> Result<(), RepError> {
        let Some(own) = self.own_place() else {
            return Ok(());
        };
        let to = Position {
            x: own.x + AFK_STEP_M,
            ..own
        };
        let Some(position) = InputPosition::from_metres(to.x, to.y, to.z) else {
            return Ok(());
        };
        self.teleported_to = Some(to);
        send_input(client, &ClientInputMsg::position(position))
    }

    /// The bot's place as an input position.
    fn own_input_position(&self) -> Option<InputPosition> {
        let own = self.own_place()?;
        InputPosition::from_metres(own.x, own.y, own.z)
    }

    /// Notes what the fight did and asks where the quest stands; a fight in which the bot saw
    /// no mob says nothing of the quest, so the quest is not played.
    fn fight_ended(
        &mut self,
        client: &mut RepClient,
        own: u64,
        since: Duration,
    ) -> Result<(), RepError> {
        let Some(fight) = self.fight.take() else {
            return self.ask(client, own, Ask::State, since);
        };
        if let Some(verdict) = fight.not_played(&self.combat, self.own_place()) {
            self.end_quest(verdict);
            return self.next_quest(client, own, since);
        }
        let hits = self.combat.hits_dealt() - fight.hits_before;
        let mob_deaths = self.combat.mob_deaths() - fight.mob_deaths_before;
        self.actions.push(format!(
            "{}: swung {} times, landed {hits} hits, saw {mob_deaths} mobs die",
            fight.task, fight.swings
        ));
        let swung_at = std::mem::take(&mut self.swung_at);
        self.fought = Some(match self.fought.take() {
            Some(fought) if fought.task == fight.task => Fought {
                hits: fought.hits + hits,
                mob_deaths: fought.mob_deaths + mob_deaths,
                fights: fought.fights + 1,
                last_hits: hits,
                spared: spared_after(fought.spared, swung_at, hits),
                ..fought
            },
            _ => Fought {
                task: fight.task,
                hits,
                mob_deaths,
                fights: 1,
                last_hits: hits,
                spared: spared_after(Vec::new(), swung_at, hits),
            },
        });
        self.ask(client, own, Ask::State, since)
    }

    /// Walks to the nearest mob and swings at it.
    /// Swings at the place of the strike plan it is at, [`SWINGS_PER_STRIKE`] times, then goes
    /// to the next; after the last it asks where the quest stands.
    fn strike_tick(
        &mut self,
        client: &mut RepClient,
        own: u64,
        since: Duration,
    ) -> Result<(), RepError> {
        if self
            .last_swing
            .is_some_and(|last| since.saturating_sub(last) < SWING_INTERVAL)
        {
            return Ok(());
        }
        let Some(strike) = self.strike.as_mut() else {
            return self.ask(client, own, Ask::State, since);
        };
        if strike.swings >= SWINGS_PER_STRIKE {
            strike.next += 1;
            strike.swings = 0;
            let Some(&place) = strike.places.get(strike.next) else {
                let done = format!("{}: struck {} places", strike.task, strike.places.len());
                self.strike = None;
                self.actions.push(done);
                return self.ask(client, own, Ask::State, since);
            };
            self.last_swing = Some(since);
            return self.teleport(client, own, place);
        }
        let place = strike.places[strike.next];
        strike.swings += 1;
        self.last_swing = Some(since);
        let at = self.own_place().unwrap_or(place);
        let Some(input_at) = InputPosition::from_metres(at.x, at.y, at.z) else {
            return Ok(());
        };
        let yaw = yaw_toward(at, place);
        let counter = self.input_counter;
        self.input_counter = counter.wrapping_add(ATTACK_RECORDS);
        send_input(
            client,
            &ClientInputMsg::attack(input_at, counter).looking([yaw, LEVEL_PITCH]),
        )
    }

    fn fight(
        &mut self,
        client: &mut RepClient,
        own_entity: u64,
        since: Duration,
    ) -> Result<(), RepError> {
        let Some(own) = self.own_place() else {
            return Ok(());
        };
        let from = [own.x, own.y, own.z];
        if let Some(place) = self.fight.as_ref().and_then(Fight::touch_due) {
            let distance = planar(from, [place.x, place.y]);
            if distance <= TOUCH_REACH_M {
                if let Some(fight) = &mut self.fight {
                    fight.touched();
                }
                return Ok(());
            }
            return self.walk_toward(client, own, place, distance, 0.0, since);
        }
        let area = self.fight.as_ref().and_then(|fight| fight.area);
        let vitals = self.fight.as_ref().and_then(|fight| fight.vitals);
        let nearest = |a: &nw_messages::WorldPosition, b: &nw_messages::WorldPosition| {
            planar(from, [a.x, a.y]).total_cmp(&planar(from, [b.x, b.y]))
        };
        // A PvP fight is with the nearest other player (the foe bot it went to), a kill step's
        // with the nearest mob of its vitals id in its area, else the nearest other mob there.
        let target = if self.fight.as_ref().is_some_and(|fight| fight.foe) {
            self.world
                .others()
                .into_iter()
                .filter_map(|number| self.world.position(number))
                .min_by(nearest)
        } else {
            let mobs: Vec<_> = self.combat.mobs().collect();
            let spared: &[u64] = match (&self.fought, &self.fight) {
                (Some(fought), Some(fight)) if fought.task == fight.task => &fought.spared,
                _ => &[],
            };
            let target = fight_target(
                &mobs,
                |entity| self.combat.vitals_of(entity),
                |entity| self.combat.takes_no_hits(entity) || spared.contains(&entity),
                area,
                vitals,
                from,
            );
            if let Some((entity, _)) = target
                && !self.swung_at.contains(&entity)
            {
                self.swung_at.push(entity);
            }
            // A mob of the kill that keeps away (the fleeing `Elk_Strange_48` of
            // `0332_0102_fabulousfurs` stayed 10-12 m from the bot, sim 14:34:15-14:35:01
            // `FleeFromTarget`, `FleeAnyTowardHome`): the game master bot goes to it.
            if let Some((entity, mob)) = target
                && vitals.is_some()
                && self.combat.vitals_of(entity) == vitals
                && planar(from, [mob.x, mob.y]) > CHASE_TELEPORT_M
                && self
                    .last_chase
                    .is_none_or(|last| since.saturating_sub(last) >= CHASE_INTERVAL)
            {
                self.last_chase = Some(since);
                return self.teleport(
                    client,
                    own_entity,
                    Position {
                        x: mob.x,
                        y: mob.y,
                        z: mob.z(),
                    },
                );
            }
            target.map(|(_, mob)| mob)
        };
        let Some(mob) = target else {
            return Ok(());
        };
        if let Some(fight) = &mut self.fight {
            fight.saw_mobs = true;
        }
        if self.close_and_swing(client, own, mob, since)?
            && let Some(fight) = &mut self.fight
        {
            fight.swings += 1;
            fight.swings_since_touch += 1;
        }
        Ok(())
    }

    /// Walks from `own` toward `target` until it is in reach, then swings at it once each
    /// [`SWING_INTERVAL`]; whether it swung.
    fn close_and_swing(
        &mut self,
        client: &mut RepClient,
        own: Position,
        target: nw_messages::WorldPosition,
        since: Duration,
    ) -> Result<bool, RepError> {
        let place = Position {
            x: target.x,
            y: target.y,
            z: target.z(),
        };
        let distance = planar([own.x, own.y, own.z], [target.x, target.y]);
        if distance > REACH_M {
            self.walk_toward(client, own, place, distance, REACH_M / 2.0, since)?;
            return Ok(false);
        }
        if self
            .last_swing
            .is_some_and(|last| since.saturating_sub(last) < SWING_INTERVAL)
        {
            return Ok(false);
        }
        self.last_swing = Some(since);
        let Some(at) = InputPosition::from_metres(own.x, own.y, own.z) else {
            return Ok(false);
        };
        let yaw = yaw_toward(own, place);
        let counter = self.input_counter;
        self.input_counter = counter.wrapping_add(ATTACK_RECORDS);
        send_input(
            client,
            &ClientInputMsg::attack(at, counter).looking([yaw, LEVEL_PITCH]),
        )?;
        Ok(true)
    }

    /// One tick of the bot's match play for `goal` ([`Act::PlayMatch`]); a dead bot waits.
    fn match_tick(
        &mut self,
        client: &mut RepClient,
        own_entity: u64,
        goal: &MatchGoal,
        since: Duration,
    ) -> Result<(), RepError> {
        let Some(own) = self.own_place() else {
            return Ok(());
        };
        if self.combat.own_dead() {
            return Ok(());
        }
        match goal {
            MatchGoal::Fight => self.fight_foe(client, own, since),
            MatchGoal::Defend(claim) => self.defend_claim(
                client,
                own_entity,
                own,
                *claim,
                (sim::world::WAR_CAPTURE_RADIUS_M, None),
                since,
            ),
            MatchGoal::Clear { claim, vitals } => self.defend_claim(
                client,
                own_entity,
                own,
                *claim,
                (CLEAR_RADIUS_M, Some(vitals.as_slice())),
                since,
            ),
            MatchGoal::Outposts | MatchGoal::Flags | MatchGoal::Siege(_) => {
                self.play_objective(client, own_entity, own, goal, since)
            }
        }
    }

    /// Holds the claim position `claim` of an invasion: fights the nearest mob within
    /// `radius_m` of it that takes hits (and whose vitals id is one of `vitals`, when given) (a teleport to one farther than [`CHASE_TELEPORT_M`], at
    /// most one each [`CHASE_INTERVAL`]; a mob that [`MAX_SWINGS_WITHOUT_HIT`] swings did not hit
    /// is left out for [`FRIEND_MEMORY`], as a match's friend is: live 2026-11-23 05:54-06:05Z, a
    /// defender swung at one fort mob for ten minutes with no hit and the wave never ended),
    /// else goes back onto it (a teleport, at most one each [`CLAIM_RETURN_INTERVAL`]) when it
    /// stands farther than [`CLAIM_STAND_M`].
    fn defend_claim(
        &mut self,
        client: &mut RepClient,
        own_entity: u64,
        own: Position,
        claim: [f32; 3],
        (radius_m, vitals): (f32, Option<&[u32]>),
        since: Duration,
    ) -> Result<(), RepError> {
        let [x, y, z] = claim;
        let claim = Position { x, y, z };
        let hits = self.combat.hits_dealt();
        let Some(play) = self.match_play.as_mut() else {
            return Ok(());
        };
        play.friends
            .retain(|_, found| since.saturating_sub(*found) < FRIEND_MEMORY);
        let mob = self
            .combat
            .mobs()
            .filter(|(entity, at)| {
                planar([claim.x, claim.y, claim.z], [at.x, at.y]) <= radius_m
                    && vitals.is_none_or(|wanted| {
                        self.combat
                            .vitals_of(*entity)
                            .is_some_and(|id| wanted.contains(&id))
                    })
                    && !self.combat.takes_no_hits(*entity)
                    && !play.friends.contains_key(entity)
            })
            .min_by(|(_, a), (_, b)| {
                planar([own.x, own.y, own.z], [a.x, a.y])
                    .total_cmp(&planar([own.x, own.y, own.z], [b.x, b.y]))
            });
        if let Some((number, mob)) = mob {
            let target = play.target.get_or_insert(MatchTarget {
                number,
                swings: 0,
                hits,
            });
            if target.number != number || hits > target.hits {
                *target = MatchTarget {
                    number,
                    swings: 0,
                    hits,
                };
            } else if target.swings >= MAX_SWINGS_WITHOUT_HIT {
                play.friends.insert(number, since);
                play.target = None;
                return Ok(());
            }
            if planar([own.x, own.y, own.z], [mob.x, mob.y]) > CHASE_TELEPORT_M
                && self
                    .last_chase
                    .is_none_or(|last| since.saturating_sub(last) >= CHASE_INTERVAL)
            {
                self.last_chase = Some(since);
                return self.teleport(
                    client,
                    own_entity,
                    Position {
                        x: mob.x,
                        y: mob.y,
                        z: mob.z(),
                    },
                );
            }
            if self.close_and_swing(client, own, mob, since)?
                && let Some(target) = self
                    .match_play
                    .as_mut()
                    .and_then(|play| play.target.as_mut())
            {
                target.swings += 1;
            }
            return Ok(());
        }
        play.target = None;
        let away = own.horizontal_distance(claim) > CLAIM_STAND_M;
        let due = play
            .at_place_since
            .is_none_or(|last| since.saturating_sub(last) >= CLAIM_RETURN_INTERVAL);
        if away && due {
            play.at_place_since = Some(since);
            return self.teleport(client, own_entity, claim);
        }
        Ok(())
    }

    /// Fights the nearest match player in sight that is no mob and not known as a friend: after
    /// [`MAX_SWINGS_WITHOUT_HIT`] swings with no hit dealt it is a friend for [`FRIEND_MEMORY`].
    fn fight_foe(
        &mut self,
        client: &mut RepClient,
        own: Position,
        since: Duration,
    ) -> Result<(), RepError> {
        let hits = self.combat.hits_dealt();
        let mobs: HashSet<u64> = self.combat.mobs().map(|(number, _)| number).collect();
        let Some(play) = self.match_play.as_mut() else {
            return Ok(());
        };
        play.friends
            .retain(|_, found| since.saturating_sub(*found) < FRIEND_MEMORY);
        let from = [own.x, own.y, own.z];
        let foe = self
            .world
            .others()
            .into_iter()
            .filter(|number| !mobs.contains(number) && !play.friends.contains_key(number))
            .filter_map(|number| Some((number, self.world.position(number)?)))
            .filter(|(_, at)| planar(from, [at.x, at.y]) <= MATCH_SIGHT_M)
            .min_by(|(_, a), (_, b)| planar(from, [a.x, a.y]).total_cmp(&planar(from, [b.x, b.y])));
        let Some((number, at)) = foe else {
            play.target = None;
            return Ok(());
        };
        let target = play.target.get_or_insert(MatchTarget {
            number,
            swings: 0,
            hits,
        });
        if target.number != number || hits > target.hits {
            *target = MatchTarget {
                number,
                swings: 0,
                hits,
            };
        } else if target.swings >= MAX_SWINGS_WITHOUT_HIT {
            play.friends.insert(number, since);
            play.target = None;
            return Ok(());
        }
        if self.close_and_swing(client, own, at, since)?
            && let Some(target) = self
                .match_play
                .as_mut()
                .and_then(|play| play.target.as_mut())
        {
            target.swings += 1;
        }
        Ok(())
    }

    /// Goes round the bot's objective places ([`objective_places`]) with a teleport onto each
    /// and stays there; at the other team's flag base it takes the flag once each
    /// [`TAKE_INTERVAL`].
    fn play_objective(
        &mut self,
        client: &mut RepClient,
        own_entity: u64,
        own: Position,
        goal: &MatchGoal,
        since: Duration,
    ) -> Result<(), RepError> {
        let team = self.world.matchmaking().team;
        let flag = self
            .combat
            .card_of(&[sim::world::FLAG_PICKUP_ASSETS[FLAG_TEAM]]);
        let Some(play) = self.match_play.as_mut() else {
            return Ok(());
        };
        let places = play
            .places
            .get_or_insert_with(|| objective_places(goal, team, own));
        let Some(&place) = places.get(play.next_place % places.len().max(1)) else {
            return Ok(());
        };
        let Some(arrived) = play.at_place_since else {
            play.at_place_since = Some(since);
            return self.teleport(client, own_entity, place);
        };
        let at_flag = *goal == MatchGoal::Flags && play.next_place % 2 == 0;
        let take = at_flag
            && play
                .last_take
                .is_none_or(|last| since.saturating_sub(last) >= TAKE_INTERVAL);
        let stay = match goal {
            MatchGoal::Outposts => OUTPOST_STAY,
            MatchGoal::Siege(_) => SIEGE_POINT_STAY,
            _ => FLAG_STAY,
        };
        if since.saturating_sub(arrived) >= stay {
            play.next_place += 1;
            play.at_place_since = None;
        }
        if take && let Some(target) = flag {
            play.last_take = Some(since);
            let request = InteractionRequest {
                target,
                target_entity: facet_of_raw_id(&target),
                interaction: 0,
                kind: TRIGGER_KIND,
                gatherable: None,
            };
            self.interact(client, own_entity, request, &[])?;
        }
        Ok(())
    }

    /// One walking step from `own` toward `place`, `distance` away, to `stop_m` short of it,
    /// at most one each [`MOVE_INTERVAL`].
    fn walk_toward(
        &mut self,
        client: &mut RepClient,
        own: Position,
        place: Position,
        distance: f32,
        stop_m: f32,
        since: Duration,
    ) -> Result<(), RepError> {
        if self
            .last_move
            .is_some_and(|last| since.saturating_sub(last) < MOVE_INTERVAL)
        {
            return Ok(());
        }
        self.last_move = Some(since);
        let length = (SPEED_MPS * MOVE_INTERVAL.as_secs_f32()).min(distance - stop_m);
        let x = own.x + (place.x - own.x) / distance * length;
        let y = own.y + (place.y - own.y) / distance * length;
        let Some(position) = InputPosition::from_metres(x, y, place.z) else {
            return Ok(());
        };
        // The server sends no own place for the bot's own moves: the bot stands where it sent
        // itself.
        self.teleported_to = Some(Position { x, y, z: place.z });
        send_input(client, &ClientInputMsg::position(position))
    }

    fn end_quest(&mut self, verdict: Verdict) {
        if verdict == Verdict::Completed {
            self.completed_since_deferral = true;
        }
        self.plays.push(QuestPlay {
            quest: self.current().to_owned(),
            verdict,
            actions: std::mem::take(&mut self.actions),
        });
    }

    fn next_quest(
        &mut self,
        client: &mut RepClient,
        own: u64,
        since: Duration,
    ) -> Result<(), RepError> {
        self.current = None;
        self.advance(client, own, since)
    }

    /// Goes on with the next quest to play ([`Self::take_next_quest`]) and asks where it stands.
    fn advance(
        &mut self,
        client: &mut RepClient,
        own: u64,
        since: Duration,
    ) -> Result<(), RepError> {
        self.take_next_quest();
        self.ask(client, own, Ask::State, since)
    }

    /// Takes the next quest to play: the queue's next, else the deferred ones again when a
    /// quest completed since they were deferred. Each deferred quest that no NPC ever offered
    /// ends as not played, with its reason. What the bot did for the last quest is forgotten,
    /// also the nodes it tried (live 2026-11-22 19:02:40: `C10A_Q3R_Revelation` stopped at its
    /// three pylons without one gather, as the run's `C10A_Q3_Revelation` had tried them).
    fn take_next_quest(&mut self) {
        if self.queue.is_empty() && !self.deferred.is_empty() {
            if self.completed_since_deferral {
                self.completed_since_deferral = false;
                self.queue
                    .extend(self.deferred.drain(..).map(|(quest, _)| quest));
            } else {
                for (quest, why) in std::mem::take(&mut self.deferred) {
                    self.plays.push(QuestPlay {
                        quest,
                        verdict: Verdict::NotPlayed(format!("no NPC offers it: {why}")),
                        actions: Vec::new(),
                    });
                }
            }
        }
        self.current = self.queue.pop_front();
        if let Some(quest) = &self.current {
            self.played.insert(quest.to_lowercase());
        }
        self.accepted = false;
        self.turned_in = false;
        self.mission_turn_in_sent = false;
        self.quest_tasks.clear();
        self.fought = None;
        self.rounds = 0;
        self.last_plan = None;
        self.same_plan = 0;
        self.tried_nodes.clear();
    }
}

/// The client's entry into territory `territory` (2297).
pub(crate) fn entered(header: FacetHeader, territory: u16) -> ClientFacetMsg {
    ClientFacetMsg::TerritoryEntered {
        header,
        area: [0; nw_messages::wire::RAW_ID_LEN],
        area_entity: 0,
        extra: 0,
        territory,
        unknown: 0,
    }
}

/// The client's exit from territory `territory` (6416).
fn exited(header: FacetHeader, territory: u16) -> ClientFacetMsg {
    ClientFacetMsg::TerritoryExited {
        header,
        area: [0; nw_messages::wire::RAW_ID_LEN],
        area_entity: 0,
        extra: 0,
        territory,
    }
}

/// What the play bot plays with besides its quests: the activities after them and the level's
/// data.
pub struct PlayData {
    /// The activities to play after the quests (before them with `activities_first`), in order.
    pub activities: Vec<Activity>,
    /// The activities come before the quests: a group leader forms its group first, so the
    /// quests' expedition opens for the group ([`crate::group::quest_group_steps`]).
    pub activities_first: bool,
    /// The bot sends its game master setup lines ([`setup_lines`]) after it left the tutorial;
    /// without them it plays as it logged in (a save check reads what the save gave back).
    pub game_master_setup: bool,
    /// The open world level's gather nodes.
    pub nodes: Arc<[GatherNode]>,
    /// The gather nodes of each instance game mode on a level of its own, by the lowercase
    /// game mode id: the bot gathers them while it is in that trial.
    pub trial_nodes: Arc<HashMap<String, Arc<[GatherNode]>>>,
    /// The gatherables (the CRC-32 of each lowercase id) of the nodes that are loot containers.
    pub containers: HashSet<u32>,
    /// The territories' `POITag`s.
    pub poi_tags: PoiTags,
    /// The bot's seat in a group of bots ([`crate::group`]); `None` for a bot alone.
    pub group: Option<GroupSeat>,
    /// The quick courses' rules and node paths.
    pub courses: Arc<CourseRoutes>,
    /// The client item index of the mount quest's reward box, or why the catalog has none.
    pub mount_reward_box: Result<u32, String>,
}

impl Player {
    /// A bot at its start, in the tutorial with its first quest to play, that plays with
    /// `data`'s activities and level data (its group seat stays with the caller).
    fn new(settings: &PlaySettings, data: PlayData) -> Self {
        let PlayData {
            activities,
            activities_first,
            game_master_setup,
            nodes,
            trial_nodes,
            containers,
            poi_tags,
            group: _,
            courses,
            mount_reward_box,
        } = data;
        Self {
            world: WorldView::default(),
            combat: CombatView::default(),
            read: 0,
            afk_warned: false,
            own_since: None,
            lines: Vec::new(),
            heard: Vec::new(),
            phase: Phase::LeaveTutorial,
            setup_lines: if game_master_setup {
                setup_lines(&settings.gm_start)
            } else {
                Vec::new()
            },
            queue: settings.quests.iter().skip(1).cloned().collect(),
            current: settings.quests.first().cloned(),
            deferred: Vec::new(),
            completed_since_deferral: false,
            played: settings
                .quests
                .first()
                .map(|quest| quest.to_lowercase())
                .into_iter()
                .collect(),
            accepted: false,
            turned_in: false,
            mission_turn_in_sent: false,
            quest_tasks: Vec::new(),
            rounds: 0,
            last_plan: None,
            same_plan: 0,
            actions: Vec::new(),
            plays: Vec::new(),
            last_swing: None,
            last_move: None,
            last_chase: None,
            input_counter: FIRST_INPUT_COUNTER,
            interaction: 0,
            activities: activities.into(),
            activities_first,
            running: None,
            nodes: Arc::clone(&nodes),
            world_nodes: nodes,
            trial_nodes,
            containers,
            poi_tags,
            tried_nodes: HashSet::new(),
            skinned: HashSet::new(),
            loot_from: 0,
            gather: None,
            in_trial: None,
            entering_trial: None,
            changing_sim: false,
            activity_plays: Vec::new(),
            answers: Vec::new(),
            teleported_to: None,
            entered_areas: Vec::new(),
            state_lines: Vec::new(),
            fight: None,
            strike: None,
            fought: None,
            swung_at: Vec::new(),
            walker: None,
            courses,
            mount_reward_box,
            riding: false,
            mounting: false,
            fishing: None,
            catches: 0,
            caught_last: false,
            pvp_foe: settings.pvp_foe.clone(),
            pvp_armed: false,
            arming_pvp: false,
            hunting: false,
            match_play: None,
            accepting: None,
        }
    }
}

/// Runs the play with `data`; `new_client` makes a client for a token; `report` gets one line
/// per HTTPS step.
///
/// # Errors
///
/// Returns [`PairError`] for the first step that fails.
pub async fn run_play(
    http: &Client,
    settings: &PlaySettings,
    mut data: PlayData,
    new_client: impl Fn(LoginToken) -> Result<RepClient, RepError>,
    mut report: impl FnMut(String),
) -> Result<PlayReport, PairError> {
    let group = data.group.take();
    let admission = walk(http, &settings.account, &mut report).await?;
    if let Some(seat) = &group {
        seat.set_character(admission.token.character_id.clone());
    }
    let mut player = Player::new(settings, data);
    let session = session_with(
        admission.rep_address,
        new_client(login_token(&admission.token))?,
        settings.hold,
        settings.registration_timeout,
        |client, outcome, since| {
            if player.phase == Phase::Done {
                return client.close();
            }
            player.tick(client, outcome, since)
        },
    )
    .await?;
    // A quest still being played when the hold ended stops as broken.
    if player.current.is_some() && player.phase != Phase::Done {
        player.end_quest(Verdict::Broken("the hold ended first".to_owned()));
    }
    if let Some((kind, running)) = player.running.take() {
        player.activity_plays.push(QuestPlay {
            quest: format!("{kind:?}"),
            verdict: Verdict::Broken("the hold ended first".to_owned()),
            actions: running.proved,
        });
    }
    Ok(PlayReport {
        outcome: session.outcome,
        quests: player.plays,
        activities: player.activity_plays,
    })
}

/// The gather nodes of the level the bot is in: those of trial `in_trial`'s own level, else
/// (the open world, or a trial played on it) the open world's `world` nodes.
fn level_nodes(
    world: &Arc<[GatherNode]>,
    trials: &HashMap<String, Arc<[GatherNode]>>,
    in_trial: Option<&str>,
) -> Arc<[GatherNode]> {
    in_trial
        .and_then(|mode| trials.get(&mode.to_lowercase()))
        .map_or_else(|| Arc::clone(world), Arc::clone)
}

/// Whether the bot leaves the trial it is in (`in_trial`) before it plays `step`: a step the
/// instance's sim cannot place lies outside the instance, as a player walks out after the
/// trial's own steps; and a trial of another game mode is entered from the open world, as the
/// sim answers no activation from inside an instance sim (live 2026-11-22 13:54 UTC: a bot
/// still in the `QuestCaerDun` instance asked three times for `QuestNihilo` and got no 6776).
fn leaves_trial_first(step: &PlanStep, in_trial: Option<&str>) -> bool {
    let Some(inside) = in_trial else {
        return false;
    };
    match step {
        PlanStep::Broken(_) | PlanStep::NotPlayed(_) => true,
        PlanStep::Trial { mode, .. } => !mode.eq_ignore_ascii_case(inside),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN: u64 = 7;

    #[test]
    fn a_step_keeps_its_first_marks_while_an_act_waits_and_the_next_step_marks_again() {
        let mut running = RunningSteps::of(Vec::new());
        running.marks.mark(1, 10, 2, 3);
        // A waiting act (a war bot's match play) ticks again with more messages.
        running.marks.mark(5, 50, 6, 7);
        assert_eq!(
            (
                running.marks.lines,
                running.marks.messages,
                running.marks.answers,
                running.marks.heard
            ),
            (1, 10, 2, 3),
            "the messages since the step began stay in view"
        );
        running.next_step();
        running.marks.mark(5, 50, 6, 7);
        assert_eq!(
            (running.step, running.act, running.marks.messages),
            (1, 0, 50)
        );
    }

    fn header() -> FacetHeader {
        FacetHeader::of(CALL_COMPONENT, OWN)
    }

    /// Live 2026-11-22 21:31:52-21:32:16: a plain wait for `1602_0401`'s banner came after the
    /// sibling fight step; an `await` is a step to play now (not a wait that comes last), so
    /// the plan's order puts it before the later fight.
    #[test]
    fn an_await_step_is_read_goes_first_and_counts_no_try() {
        let header = header();
        let steps: Vec<(String, PlanStep)> = [
            "plan d_03_01 await 1656_0401_Banner at 5453.8 7427.5 257.0",
            "plan d_03_02 kill Legion_Sagittarii_Named_01 1 at 5453.8 7427.5 257.0 placed within 30.0",
        ]
        .iter()
        .filter_map(|line| plan_step(line, header))
        .collect();
        assert_eq!(
            steps[0].1,
            PlanStep::Await {
                gatherable: "1656_0401_Banner".to_owned(),
                at: Position {
                    x: 5453.8,
                    y: 7427.5,
                    z: 257.0
                }
            }
        );
        assert_eq!(
            picked_step(&steps).map(|(task, _)| task.as_str()),
            Some("d_03_01")
        );
    }

    #[test]
    fn a_bot_in_the_fight_area_stays_and_one_outside_goes_to_its_place() {
        const RADIUS_M: f32 = 30.0;
        let centre = Position {
            x: 8541.4,
            y: 1277.0,
            z: 219.4,
        };
        let near = Position {
            x: centre.x + 7.0,
            ..centre
        };
        let far = Position {
            x: centre.x + RADIUS_M,
            ..centre
        };
        let area = Some((centre, RADIUS_M));
        assert!(stays_in_fight_area(area, Some(near)));
        assert!(
            !stays_in_fight_area(area, Some(far)),
            "outside the area less the reach"
        );
        assert!(
            !stays_in_fight_area(None, Some(near)),
            "a fight with no area"
        );
        assert!(
            !stays_in_fight_area(area, None),
            "a bot whose place is not known"
        );
    }

    #[test]
    fn the_next_quest_tries_every_node_again() {
        // Live 2026-11-22 19:00-19:02: one run played `C10A_Q3_Revelation`, then
        // `C10A_Q3R_Revelation`, whose pylon step names the same three nodes.
        const QUESTS: [&str; 2] = ["C10A_Q3_Revelation", "C10A_Q3R_Revelation"];
        const TRIED_NODE: usize = 3;
        let settings = PlaySettings {
            account: WalkSettings {
                login_url: String::new(),
                game_url: String::new(),
                account: String::new(),
                password: String::new(),
                character_name: String::new(),
                creation_params: String::new(),
            },
            quests: QUESTS.map(str::to_owned).to_vec(),
            gm_start: Vec::new(),
            pvp_foe: None,
            hold: Duration::ZERO,
            registration_timeout: Duration::ZERO,
        };
        let data = PlayData {
            activities: Vec::new(),
            activities_first: false,
            game_master_setup: true,
            nodes: Arc::from(Vec::new()),
            trial_nodes: Arc::new(HashMap::new()),
            containers: HashSet::new(),
            poi_tags: PoiTags::default(),
            group: None,
            courses: Arc::new(CourseRoutes::default()),
            mount_reward_box: Err(String::new()),
        };
        let mut player = Player::new(&settings, data);
        player.tried_nodes.insert(TRIED_NODE);
        player.same_plan = 1;
        player.take_next_quest();
        assert_eq!(player.current.as_deref(), Some(QUESTS[1]));
        assert!(
            player.tried_nodes.is_empty(),
            "the node is untried for the next quest"
        );
        assert_eq!(player.same_plan, 0);
    }

    #[test]
    fn the_objective_team_goes_round_the_places_of_its_map_and_the_other_stays() {
        let at = |[x, y, z]: [f32; 3]| Position { x, y, z };
        let vertical = sim::world::outpost_places()
            .nth(1)
            .expect("the Vertical map");
        let near_vertical = at([vertical[1][0] + 5.0, vertical[1][1], vertical[1][2]]);
        assert_eq!(
            objective_places(&MatchGoal::Outposts, Some(OBJECTIVE_TEAM), near_vertical),
            vertical.map(at).to_vec(),
            "the outposts A, B and C of the map the bot stands in"
        );
        assert!(
            objective_places(&MatchGoal::Outposts, Some(1), near_vertical).is_empty(),
            "the other team stays"
        );
        assert!(objective_places(&MatchGoal::Outposts, None, near_vertical).is_empty());
        let long = sim::world::flag_bases().next().expect("the Long map");
        assert_eq!(
            objective_places(&MatchGoal::Flags, Some(OBJECTIVE_TEAM), at(long[0])),
            vec![at(long[FLAG_TEAM]), at(long[0])],
            "the other team's flag base, then the own base"
        );
        assert!(objective_places(&MatchGoal::Fight, Some(OBJECTIVE_TEAM), at(long[0])).is_empty());
        assert_eq!(
            objective_places(
                &MatchGoal::Siege(vec![long[1], long[0]]),
                Some(1),
                at(long[0])
            ),
            vec![at(long[1]), at(long[0])],
            "a war's capture points for every side the goal names them"
        );
        assert!(
            objective_places(
                &MatchGoal::Defend(long[0]),
                Some(OBJECTIVE_TEAM),
                at(long[0])
            )
            .is_empty()
        );
    }

    #[test]
    fn a_move_into_another_sim_turns_god_mode_on_again_there() {
        // Live 2026-11-22 13:52:24: god mode stayed in the open world sim, and the leader died
        // in the DungeonShatteredObelisk instance.
        assert_eq!(phase_after_sim_change(true), Some(Phase::SimLines(0)));
        assert_eq!(phase_after_sim_change(false), None);
        assert_eq!(SIM_LINES, ["!god on", "!heal"]);
    }

    #[test]
    fn the_setup_starts_each_gm_started_quest_after_the_bot_s_own_lines() {
        let lines = setup_lines(&["12A_MSQ_09_ReturnRegent".to_owned()]);
        let own_lines = SIM_LINES.len() + SAVED_SETUP_LINES.len();
        assert_eq!(&lines[..SIM_LINES.len()], SIM_LINES);
        assert_eq!(&lines[SIM_LINES.len()..own_lines], SAVED_SETUP_LINES);
        assert_eq!(lines[own_lines..], ["!quest 12A_MSQ_09_ReturnRegent"]);
        assert_eq!(setup_lines(&[]).len(), own_lines);
    }

    #[test]
    fn a_drop_at_a_chance_gets_more_tries() {
        // Live 2026-11-22 `!queststate 1127_0102_MakinBacon`.
        const CHEEK: &str = "1127_0102_GatherMeat1";
        let state = vec![format!(
            "step {CHEEK} 0 GatherItems {{ item: \"1127_PrimeCheek\", count: 1, drop: \
             Some(QuestDrop {{ sources: [\"Pig_Domestic_11MD07\"], percent: 40, place: None }}), \
             chest: None, returned: true }}"
        )];
        assert_eq!(step_drop_percent(&state, CHEEK), Some(40));
        assert_eq!(step_drop_percent(&state, "1127_0102_GatherMeat2"), None);
        // 0.6^6 = 0.047: six tries give the cheek at 95 %, and the kill before the first skin.
        assert_eq!(max_tries(Some(40)), 7);
        assert_eq!(max_tries(Some(100)), MAX_SAME_PLAN);
        assert_eq!(max_tries(Some(80)), MAX_SAME_PLAN);
        assert_eq!(max_tries(None), MAX_SAME_PLAN);
    }

    #[test]
    fn a_plan_after_a_step_of_its_quest_counted_is_another_plan() {
        // Live 2026-11-22 `!queststate 0713_0101_explorerift`: the charcoal step at 2, then 3.
        const CHARCOAL: &str = "0713_0101_d_04_02_TaskHaveAndReturnItems_8_demoncharcoal";
        const LINE: &str = "plan 0713_0101_d_04_02_TaskHaveAndReturnItems_8_demoncharcoal loot kill \
                            Risen_Corruption_Entity_61 1 at 10059.4 9292.2 530.3 placed";
        const EMBERS: &str = "0713_0101_d_04_03_TaskHaveAndReturnItems_8_hellfireembers";
        let state = |charcoal: u32, embers: u32| {
            vec![
                "running 0713_0101_explorerift".to_owned(),
                format!(
                    "step {CHARCOAL} {charcoal} HaveItems {{ item: \"0713_0101_DamonCharcoal\" }}"
                ),
                format!(
                    "step {EMBERS} {embers} HaveItems {{ item: \"0713_0101_HellfireEmbers\" }}"
                ),
            ]
        };
        assert_eq!(
            quest_progress(&state(2, 0)),
            format!("{CHARCOAL} 2; {EMBERS} 0")
        );
        let plan =
            |charcoal, embers| plan_at_progress(LINE, &quest_progress(&state(charcoal, embers)));
        assert_eq!(plan(2, 0), plan(2, 0), "the same plan");
        assert_ne!(plan(2, 0), plan(3, 0), "a kill counted");
        assert_ne!(plan(2, 0), plan(2, 1), "another step counted");
    }

    #[test]
    fn a_step_that_counts_in_one_place_names_the_area_of_its_poi_tag() {
        // Live 2026-11-21 `!queststate 12_pillageship_soldier`: the treasure counts only in
        // `drowned_shipwreck_3_30129`; `04A_Side03_02_AncientScrolls` only in `shattered_obelisk`.
        const TREASURE: &str = "12_pillageship_04_01_GatherReturn_Treasure_A_shipwreck_3";
        const SHIPWRECK: &str = "drowned_shipwreck_3_30129";
        const SHIPWRECK_AREA: u32 = 30129;
        const OBELISK: &str = "shattered_obelisk";
        // A made-up area: the test names the tag's first territory.
        const OBELISK_AREA: u32 = 40001;
        let state = [
            "running 12_pillageship_soldier".to_owned(),
            format!(
                "step {TREASURE} 0 GatherItems {{ item: \"0601_0104_Treasure_A\", count: 1, drop: \
                 Some(QuestDrop {{ sources: [\"MSQ_Treasure_A\"], percent: 100, place: \
                 Some(\"{SHIPWRECK}\") }}), chest: None, returned: true }}"
            ),
            "step anywhere 0 GatherItems { item: \"x\", count: 1, drop: None, chest: None }"
                .to_owned(),
        ];
        assert_eq!(step_place(&state, TREASURE), Some(SHIPWRECK));
        assert_eq!(step_place(&state, "anywhere"), None, "no place");
        assert_eq!(step_place(&state, "12_pillageship"), None, "another task");
        let tags = PoiTags::build([
            (SHIPWRECK_AREA, SHIPWRECK.to_owned()),
            (OBELISK_AREA, format!("{OBELISK}, ruins")),
        ]);
        assert_eq!(area_of_place(&tags, SHIPWRECK), Some(30129));
        assert_eq!(area_of_place(&tags, OBELISK), Some(40001));
        assert_eq!(area_of_place(&tags, "nowhere"), None);
    }

    #[test]
    fn a_lore_line_reads_its_title_and_place() {
        assert_eq!(
            plan_step(
                "plan n lore 12_MonarchsBluffs_Title_20_01 at 8120.5 3050.0 112.0",
                header()
            )
            .map(|(_, step)| step),
            Some(PlanStep::Lore {
                title: "12_MonarchsBluffs_Title_20_01".to_owned(),
                at: Position {
                    x: 8120.5,
                    y: 3050.0,
                    z: 112.0
                }
            })
        );
        let steps = lore_steps("12_MonarchsBluffs_Title_20_01", [0.0, 0.0, 0.0]);
        assert!(matches!(
            steps[0].acts.last(),
            Some(Act::Interact { name, .. }) if name == b"@12_MonarchsBluffs_Title_20_01"
        ));
    }

    #[test]
    fn a_bot_in_a_trial_gathers_the_nodes_of_the_trial_level() {
        const STAFF_PLACE: u32 = 0x99e0_0001;
        const OPEN_NODE: u32 = 0x0000_0002;
        let node = |gatherable: u32| GatherNode {
            gatherable,
            at: [0.0; 3],
        };
        let world: Arc<[GatherNode]> = Arc::from([node(OPEN_NODE)]);
        let trials = HashMap::from([(
            "questmotherwell".to_owned(),
            Arc::from([node(STAFF_PLACE)]) as Arc<[GatherNode]>,
        )]);
        let gatherables = |nodes: Arc<[GatherNode]>| -> Vec<u32> {
            nodes.iter().map(|node| node.gatherable).collect()
        };
        assert_eq!(
            gatherables(level_nodes(&world, &trials, Some("QuestMotherwell"))),
            [STAFF_PLACE]
        );
        assert_eq!(
            gatherables(level_nodes(&world, &trials, Some("QuestCaerDun"))),
            [OPEN_NODE],
            "a trial on the open world level gathers the open world's nodes"
        );
        assert_eq!(gatherables(level_nodes(&world, &trials, None)), [OPEN_NODE]);
    }

    #[test]
    fn a_bot_in_a_trial_leaves_it_before_another_trial_or_a_step_outside_it() {
        const INSIDE: &str = "QuestCaerDun";
        const OTHER: &str = "QuestNihilo";
        const CRC: u32 = 0x634a_d894;
        let trial = |mode: &str| PlanStep::Trial {
            mode: mode.to_owned(),
            crc: CRC,
            door: None,
        };
        assert!(leaves_trial_first(&trial(OTHER), Some(INSIDE)));
        assert!(!leaves_trial_first(
            &trial(&INSIDE.to_lowercase()),
            Some(INSIDE)
        ));
        assert!(leaves_trial_first(
            &PlanStep::Broken(String::new()),
            Some(INSIDE)
        ));
        assert!(!leaves_trial_first(&PlanStep::Wait, Some(INSIDE)));
        assert!(
            !leaves_trial_first(&trial(OTHER), None),
            "in the open world it enters"
        );
    }

    #[test]
    fn a_trial_line_reads_its_game_mode_and_enters_then_leaves_by_the_arena_calls() {
        const MODE: &str = "QuestGawain";
        const CRC: u32 = 0x782b_a83f;
        assert_eq!(
            plan_step(&format!("plan t trial {MODE} {CRC:08x}"), header()).map(|(_, step)| step),
            Some(PlanStep::Trial {
                mode: MODE.to_owned(),
                crc: CRC,
                door: None,
            })
        );
        assert_eq!(
            plan_step(&format!("plan t trial {MODE}"), header()),
            None,
            "a trial line without its game mode's CRC does not read"
        );
        let proofs: Vec<u64> = trial_steps(MODE, CRC)
            .into_iter()
            .filter_map(|step| match step.proof {
                Proof::Message(type_index) | Proof::ArenaYes(type_index) => Some(type_index),
                _ => None,
            })
            .collect();
        assert_eq!(
            proofs,
            [
                nw_messages::arena::REMOTE_INTERACT_RESULT_TYPE_INDEX,
                nw_messages::arena::DUNGEON_ACTIVATE_RESULT_TYPE_INDEX,
                nw_messages::LEVEL_INFO_CHANGED_MSG_TYPE_INDEX,
            ]
        );
        let leave = leave_trial_steps();
        assert!(matches!(
            leave[0].proof,
            Proof::Message(nw_messages::LEVEL_INFO_CHANGED_MSG_TYPE_INDEX)
        ));
        let Some(Act::Call(call)) = leave[0].acts.first() else {
            panic!("a call");
        };
        let (type_index, _) = call(1).expect("encodes");
        assert_eq!(
            type_index,
            nw_messages::arena::ArenaClientMsg::ForfeitArena {
                header: header(),
                flag: 0
            }
            .type_index()
        );
    }

    #[test]
    fn a_door_trial_line_reads_its_door_and_enters_at_it_by_the_dungeon_interactions() {
        const MODE: &str = "QuestMedea";
        const CRC: u32 = 0x1234_5678;
        const DOOR: u64 = 0x0102_0304_0506_0708;
        assert_eq!(
            plan_step(
                &format!("plan t trial {MODE} {CRC:08x} door {DOOR:016x} at 8504.0 2663.0 56.6"),
                header()
            )
            .map(|(_, step)| step),
            Some(PlanStep::Trial {
                mode: MODE.to_owned(),
                crc: CRC,
                door: Some((
                    DOOR,
                    Some(Position {
                        x: 8504.0,
                        y: 2663.0,
                        z: 56.6
                    })
                )),
            })
        );
        assert_eq!(
            plan_step(
                &format!("plan t trial {MODE} {CRC:08x} door {DOOR:016x}"),
                header()
            )
            .map(|(_, step)| step),
            Some(PlanStep::Trial {
                mode: MODE.to_owned(),
                crc: CRC,
                door: Some((DOOR, None)),
            }),
            "a door with no place: the bot enters where it stands"
        );
        let steps = door_trial_steps(MODE, CRC, DOOR, Some([8504.0, 2663.0, 56.6]));
        let kinds: Vec<u8> = steps
            .iter()
            .flat_map(|step| &step.acts)
            .filter_map(|act| match act {
                Act::Interact { request, .. } if request.target_entity == DOOR => {
                    Some(request.kind)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            kinds,
            [
                nw_messages::client_input::OPEN_DUNGEON_MENU_KIND,
                nw_messages::client_input::ENTER_DUNGEON_KIND
            ]
        );
        assert!(matches!(steps[0].acts.first(), Some(Act::Teleport(_))));
        let proofs: Vec<u64> = steps
            .into_iter()
            .filter_map(|step| match step.proof {
                Proof::Message(type_index) | Proof::ArenaYes(type_index) => Some(type_index),
                _ => None,
            })
            .collect();
        assert_eq!(
            proofs,
            [
                nw_messages::arena::REMOTE_INTERACT_RESULT_TYPE_INDEX,
                nw_messages::arena::DUNGEON_ACTIVATE_RESULT_TYPE_INDEX,
                nw_messages::LEVEL_INFO_CHANGED_MSG_TYPE_INDEX,
            ]
        );
    }

    #[test]
    fn a_camp_line_reads_its_place_skin_asset_and_costs() {
        const ASSET: &str = "3c4895410000000000000000019d4900";
        let step = plan_step(
            &format!(
                "plan c camp at 7598.0 4149.0 70.5 skin 00000000 asset {ASSET} cost WoodT1 5 cost FlintT1 1"
            ),
            header(),
        )
        .map(|(_, step)| step);
        assert_eq!(
            step,
            Some(PlanStep::Camp {
                at: Position {
                    x: 7598.0,
                    y: 4149.0,
                    z: 70.5
                },
                skin: 0,
                asset: data_encoding::HEXLOWER
                    .decode(ASSET.as_bytes())
                    .expect("hex")
                    .try_into()
                    .expect("16 bytes"),
                costs: vec![("WoodT1".to_owned(), 5), ("FlintT1".to_owned(), 1)]
            })
        );
    }

    #[test]
    fn a_gather_line_gathers_the_nodes_of_its_gatherable() {
        assert!(matches!(
            plan_step("plan r gather MB_MSQ2_RescueMiner_Rescue01 1", header()),
            Some((_, PlanStep::Item { node: Some(node), gathered: true, count: 1, .. }))
                if node == "MB_MSQ2_RescueMiner_Rescue01"
        ));
    }

    #[test]
    fn an_achievements_check_of_the_quest_itself_gives_way_to_another_step() {
        const QUEST: &str = "09A_M15_FightArtemis";
        let read = |line: &str| plan_step(line, header()).expect("a step");
        let steps = vec![
            read(
                "plan c achievements 09A_M15_ArtemisTrialComplete (from a step of quest 09A_M15_FightArtemis)",
            ),
            read("plan f wait"),
            read("plan d achievements 09A_M14 (from the end of quest 09A_M14_GetInfusedSeed)"),
        ];
        let left = without_own_achievements(steps.clone(), QUEST);
        assert_eq!(
            left.iter()
                .map(|(task, _)| task.as_str())
                .collect::<Vec<_>>(),
            ["f", "d"]
        );
        assert_eq!(picked_step(&left).map(|(task, _)| task.as_str()), Some("d"));
        let alone = vec![steps[0].clone()];
        assert_eq!(
            without_own_achievements(alone.clone(), QUEST),
            alone,
            "nothing else to play"
        );
    }

    #[test]
    fn a_skin_line_reads_its_gatherable_and_its_kill() {
        let step = plan_step(
            "plan s skin Boar 1 kill CK_Boar_10WS10 1 at 6088.6 834.6 129.3",
            header(),
        )
        .map(|(_, step)| step);
        assert_eq!(
            step,
            Some(PlanStep::Skin {
                gatherable: "Boar".to_owned(),
                count: 1,
                kill: Box::new(PlanStep::Kill {
                    vitals: "CK_Boar_10WS10".to_owned(),
                    count: 1,
                    at: Position {
                        x: 6088.6,
                        y: 834.6,
                        z: 129.3
                    },
                    territory: None,
                    placed: false,
                    within: None,
                    touch: Vec::new()
                })
            })
        );
        assert_eq!(
            plan_step("plan s skin Boar 1 go 1.0 2.0 3.0", header()),
            None,
            "a skin comes after a kill"
        );
    }

    /// `12A_MSQ_15_DefeatNavigator_CollectGun`: the blunderbuss is in the loot bag of the named
    /// navigator.
    #[test]
    fn a_loot_line_reads_its_kill_and_opens_each_bag_with_the_loot_interaction() {
        let step = plan_step(
            "plan g loot kill Undead_Navigator_Named_12MB01d 1 at 7822.0 4209.0 30.0 placed",
            header(),
        )
        .map(|(_, step)| step);
        assert_eq!(
            step,
            Some(PlanStep::Loot {
                kill: Box::new(PlanStep::Kill {
                    vitals: "Undead_Navigator_Named_12MB01d".to_owned(),
                    count: 1,
                    at: Position {
                        x: 7822.0,
                        y: 4209.0,
                        z: 30.0
                    },
                    territory: None,
                    placed: true,
                    within: None,
                    touch: Vec::new()
                })
            })
        );
        assert_eq!(
            plan_step("plan g loot go 1.0 2.0 3.0", header()),
            None,
            "a loot comes after a kill"
        );
        const NEWER: u8 = 2;
        let bags = [
            [1; nw_messages::wire::RAW_ID_LEN],
            [NEWER; nw_messages::wire::RAW_ID_LEN],
        ];
        let steps = loot_steps(&[bags[1], bags[0]]);
        assert_eq!(steps.len(), bags.len());
        let Some(Act::Interact { request, .. }) = steps[0].acts.first() else {
            panic!("an interaction");
        };
        assert_eq!(request.kind, nw_messages::client_input::LOOT_KIND);
        assert_eq!(request.target, bags[1]);
        assert_eq!(request.target_entity, facet_of_raw_id(&bags[1]));
        assert!(matches!(steps[0].proof, Proof::Answered));
    }

    /// The cook of `12_boarfeast_05_01` at the watchers' campfire with the skinned pork.
    #[test]
    fn a_craft_line_reads_its_station_and_ingredients_and_opens_crafts_and_closes() {
        const STATION: &str = "ff21e7138c5a804d0000000000000000";
        let step = plan_step(
            &format!(
                "plan c craft Procedural_HealthRecoveryT1 at 7000.0 3000.0 40.0 station {STATION} with PorkT1 1 with TimberT1 2 give TimberT1 2"
            ),
            header(),
        )
        .map(|(_, step)| step);
        let station = raw_id_of(STATION).expect("hex");
        assert_eq!(
            step,
            Some(PlanStep::Craft {
                recipe: "Procedural_HealthRecoveryT1".to_owned(),
                at: Position {
                    x: 7000.0,
                    y: 3000.0,
                    z: 40.0
                },
                station,
                ingredients: vec![("PorkT1".to_owned(), 1), ("TimberT1".to_owned(), 2)],
                missing: vec![("TimberT1".to_owned(), 2)],
            })
        );
        assert_eq!(
            plan_step(
                "plan c craft Procedural_HealthRecoveryT1 at 7000.0 3000.0 40.0",
                header()
            ),
            None,
            "a craft names its station"
        );
        let steps = quest_craft_steps(
            "Procedural_HealthRecoveryT1",
            station,
            [7000.0, 3000.0, 40.0],
            vec![("PorkT1".to_owned(), 1)],
        );
        let proofs: Vec<bool> = steps
            .iter()
            .map(|step| matches!(step.proof, Proof::Accepted))
            .collect();
        assert_eq!(proofs, [true, false, false], "the station opens first");
        assert!(matches!(
            steps[1].proof,
            Proof::Message(nw_messages::facet::CRAFTING_PROGRESS_TYPE_INDEX)
        ));
        assert!(matches!(
            steps[2].acts.last(),
            Some(Act::EndInteraction { target_entity }) if *target_entity == facet_of_raw_id(&station)
        ));
    }

    #[test]
    fn an_equip_line_reads_the_client_s_equip_call() {
        const INSTANCE: &str = "4b6024e9e030411699fc899e15fb70ce";
        let step = plan_step(
            &format!("plan e equip 1hSkinningKnifeT1 bag 3 instance {INSTANCE} slot 35"),
            header(),
        )
        .map(|(_, step)| step);
        assert_eq!(
            step,
            Some(PlanStep::Equip {
                item: "1hSkinningKnifeT1".to_owned(),
                bag_slot: 3,
                instance: data_encoding::HEXLOWER
                    .decode(INSTANCE.as_bytes())
                    .expect("hex")
                    .try_into()
                    .expect("16 bytes"),
                slot: 35
            })
        );
        assert_eq!(
            plan_step(
                "plan e equip 1hSkinningKnifeT1 bag 3 instance 00 slot 35",
                header()
            ),
            None,
            "an instance of another length does not read"
        );
    }

    #[test]
    fn a_use_line_reads_its_object_and_place() {
        assert_eq!(
            plan_step(
                "plan v use SettlementStorage e0f2606175430000 1 at 9024.5 4102.0 101.3",
                header()
            ),
            Some((
                "v".to_owned(),
                PlanStep::Use {
                    tag: "SettlementStorage".to_owned(),
                    object: 0xe0f2_6061_7543_0000,
                    count: 1,
                    at: Position {
                        x: 9024.5,
                        y: 4102.0,
                        z: 101.3
                    },
                }
            ))
        );
    }

    #[test]
    fn a_course_event_line_reads_its_course_and_section() {
        // The live `!questplan` answers of 2026-11-22 (`0334_0102`, `09A_S11`).
        let read = |line: &str| plan_step(line, header()).map(|(_, step)| step);
        assert_eq!(
            read(
                "plan 0334_0102_d_02_GC_Food_Race_1 event 2 Course { course: \"GC_Food_Race_1\", section: 0, sections: 2 }"
            ),
            Some(PlanStep::Course {
                course: "GC_Food_Race_1".to_owned(),
                section: 0,
            })
        );
        assert_eq!(
            read("plan s event 1 Course { course: \"FL_09A_S11_1\", section: 4, sections: 13 }"),
            Some(PlanStep::Course {
                course: "FL_09A_S11_1".to_owned(),
                section: 4,
            })
        );
        assert!(matches!(
            read(
                "plan f event 1 Caught { item: \"16_Side_11_TastyFish\", length: 1.0, weight: 1.0, hotspot: false }"
            ),
            Some(PlanStep::NotPlayed(_))
        ));
    }

    #[test]
    fn a_song_line_reads_its_song_sheet_length_and_place() {
        assert_eq!(
            plan_step(
                "plan 04A_Side05_02_00E_PracticeSong1 song GiftEasy sheet Gift_AzothFlute_Part1_Easy seconds 60.888 at 8904.8 4223.9 138.7",
                header()
            ),
            Some((
                "04A_Side05_02_00E_PracticeSong1".to_owned(),
                PlanStep::Song {
                    song: "GiftEasy".to_owned(),
                    sheet: "Gift_AzothFlute_Part1_Easy".to_owned(),
                    duration_s: 60.888,
                    at: Position {
                        x: 8904.8,
                        y: 4223.9,
                        z: 138.7
                    },
                }
            ))
        );
    }

    #[test]
    fn a_use_item_line_reads_its_item_and_bag_slot() {
        assert_eq!(
            plan_step(
                "plan 09A_S0_UsePotion useitem 09A_S0_SporeElixir bag 12",
                header()
            ),
            Some((
                "09A_S0_UsePotion".to_owned(),
                PlanStep::UseItem {
                    item: "09A_S0_SporeElixir".to_owned(),
                    bag_slot: 12,
                }
            ))
        );
    }

    #[test]
    fn a_pvp_line_reads_its_kills() {
        assert_eq!(
            plan_step("plan C10A_Q6_KillPlayers pvp 3", header()),
            Some(("C10A_Q6_KillPlayers".to_owned(), PlanStep::Pvp { count: 3 }))
        );
        assert_eq!(plan_step("plan C10A_Q6_KillPlayers pvp", header()), None);
    }

    #[test]
    fn a_fish_line_reads_its_place_yaw_and_territory() {
        assert_eq!(
            plan_step(
                "plan 16_Side_11_d_01_catch_fish fish at 6473.6 9155.9 165.6 yaw 2.356 territory 16",
                header()
            ),
            Some((
                "16_Side_11_d_01_catch_fish".to_owned(),
                PlanStep::Fish {
                    at: Position {
                        x: 6473.6,
                        y: 9155.9,
                        z: 165.6
                    },
                    yaw: 2.356,
                    territory: Some(16),
                }
            ))
        );
    }

    #[test]
    fn a_use_the_bot_has_no_kind_for_comes_after_a_sibling_it_can_play() {
        // Retail `06a_ts_02_forgesword`: the partial container of the smelter use ended by its
        // ingot sibling; the use never counted.
        let plan = [
            "plan 06A_TS_02_ForgeSword_useSmelter use SettlementSmelting 00ab12cd34ef5678 1 at 9328.0 2692.0 82.6",
            "plan 06A_TS_02_ForgeSword_haveItems_ingotT2 item IngotT2 9 from a recipe",
            "plan v use SettlementStorage e0f2606175430000 1 at 9024.5 4102.0 101.3",
        ];
        let steps: Vec<(String, PlanStep)> = plan
            .iter()
            .filter_map(|line| plan_step(line, header()))
            .collect();
        assert_eq!(steps.len(), plan.len());
        assert_eq!(
            picked_step(&steps[..2]).map(|(task, _)| task.as_str()),
            Some("06A_TS_02_ForgeSword_haveItems_ingotT2")
        );
        assert_eq!(
            picked_step(&[steps[0].clone(), steps[2].clone()]).map(|(task, _)| task.as_str()),
            Some("v"),
            "a storage use is a step the bot plays"
        );
    }

    #[test]
    fn a_start_line_reads_its_gatherable_asset_and_place() {
        assert_eq!(
            plan_step(
                "plan e start 04A_Side02_02_EncounterStart_01 a28f5f64c3e15e9f9cbe6588b06b71c5 at 8732.3 4251.9 181.2",
                header()
            )
            .map(|(_, step)| step),
            Some(PlanStep::Start {
                gatherable: "04A_Side02_02_EncounterStart_01".to_owned(),
                asset: [
                    0xa2, 0x8f, 0x5f, 0x64, 0xc3, 0xe1, 0x5e, 0x9f, 0x9c, 0xbe, 0x65, 0x88, 0xb0,
                    0x6b, 0x71, 0xc5
                ],
                at: Position {
                    x: 8732.3,
                    y: 4251.9,
                    z: 181.2
                },
            })
        );
        let steps = start_object_steps([7; nw_messages::wire::RAW_ID_LEN], 0x1101_7351);
        assert!(matches!(
            steps[0].acts.last(),
            Some(Act::Interact { request, .. })
                if request.gatherable == Some(0x1101_7351) && request.target == [7; nw_messages::wire::RAW_ID_LEN]
        ));
    }

    #[test]
    fn a_gather_line_with_again_turns_the_same_stone() {
        assert_eq!(
            plan_step(
                "plan s gather Cyclic_06A_MSQ_puzzle2_stone1 2 again",
                header()
            )
            .map(|(_, step)| step),
            Some(PlanStep::Turn {
                gatherable: "Cyclic_06A_MSQ_puzzle2_stone1".to_owned(),
                count: 2,
            })
        );
        assert!(matches!(
            plan_step("plan s gather Walk_Miner 1", header()).map(|(_, step)| step),
            Some(PlanStep::Item { .. })
        ));
    }

    #[test]
    fn a_gather_step_whose_nodes_the_bot_tried_waits_for_the_other_steps() {
        // The live plan of `99B_MSQ_01_searchwalsham` (2026-11-22) after the gather of candle A:
        // the item step still names candle A, the bot's only A node is tried.
        const CANDLE_A: &str = "99B_MSQ_01_BlessedCandleA";
        let plan = [
            "plan 99B_MSQ_01_d_02_04_02_01_Gather_Candles item 99B_MSQ_01_BlessedCandle 2 gathered node 99B_MSQ_01_BlessedCandleA from the drop of quest object 99B_MSQ_01_BlessedCandleA",
            "plan 99B_MSQ_01_d_02_04_02_03_Interact_BlessedCandleB gather 99B_MSQ_01_BlessedCandleB 1",
        ];
        let steps: Vec<(String, PlanStep)> = plan
            .iter()
            .filter_map(|line| plan_step(line, header()))
            .collect();
        let node = |gatherable: &str| GatherNode {
            gatherable: lowercase_crc32(gatherable),
            at: [0.0, 0.0, 0.0],
        };
        let nodes = [node(CANDLE_A), node("99B_MSQ_01_BlessedCandleB")];
        let mut tried = HashSet::new();
        assert!(
            !gathered_out(&nodes, &tried, &steps[0].1),
            "the A node is not tried yet"
        );
        tried.insert(0);
        assert!(gathered_out(&nodes, &tried, &steps[0].1));
        assert!(
            !gathered_out(&nodes, &tried, &steps[1].1),
            "the B node is still to gather"
        );
        assert_eq!(
            picked_step(&[steps[1].clone()]).map(|(task, _)| task.as_str()),
            Some("99B_MSQ_01_d_02_04_02_03_Interact_BlessedCandleB")
        );
    }

    #[test]
    fn the_bot_plays_a_step_it_can_play_before_one_that_waits_for_achievements() {
        // The live `!questplan` answer of `12_getoffbeach` for a new character (2026-11-17).
        let plan = [
            "plan 12_getoffbeach_01_Watched_OtherBeachIntro achievements Watched_IGC_Aelstrom_WW (from a step of quest 06_getoffbeach)",
            "plan 12_getoffbeach_01_Watched_IntroWatched achievements Watched_IGC_Aelstrom_MB (from a step of quest 12_getoffbeach)",
            "plan 12_getoffbeach_01_Aelstrom_Beach_cinematic_timer client cutscene af4c01c8",
        ];
        let steps: Vec<(String, PlanStep)> = plan
            .iter()
            .filter_map(|line| plan_step(line, header()))
            .collect();
        let (task, step) = picked_step(&steps).expect("a step");
        assert_eq!(task, "12_getoffbeach_01_Aelstrom_Beach_cinematic_timer");
        assert!(matches!(step, PlanStep::Client(_)));
        let waits: Vec<(String, PlanStep)> = [
            ("wait".to_owned(), PlanStep::Wait),
            (
                "cannot".to_owned(),
                PlanStep::NotPlayed("a flow".to_owned()),
            ),
            (
                "needs".to_owned(),
                PlanStep::Achievements {
                    needs: String::new(),
                    sources: Vec::new(),
                },
            ),
        ]
        .into();
        assert_eq!(
            picked_step(&waits).map(|(task, _)| task.as_str()),
            Some("needs")
        );
        assert_eq!(
            picked_step(&waits[..2]).map(|(task, _)| task.as_str()),
            Some("cannot"),
            "a step the bot cannot play before a wait"
        );
        assert_eq!(
            picked_step(&waits[..1]).map(|(task, _)| task.as_str()),
            Some("wait")
        );
    }

    #[test]
    fn a_step_the_bot_does_not_play_is_the_stop_before_a_branch_that_cannot_end() {
        // The live `!questplan` answer of `06A_MSQ_18_ReturnSiriusJewel` (2026-11-21): its
        // impossible task stands beside the puzzle steps that end the quest.
        let plan = [
            "plan 06A_MSQ_18_ReturnSiriusJewel_impossible_task stop Broken: no place: its trigger volume, its location and its POI are all unknown",
            "plan 06A_MSQ_18_ReturnSiriusJewel_interact_puzzle2_stone1 event 1 Cyclic { cyclic: \"Cyclic_06A_MSQ_puzzle2_stone1\", state: 1 }",
        ];
        let steps: Vec<(String, PlanStep)> = plan
            .iter()
            .filter_map(|line| plan_step(line, header()))
            .collect();
        let (task, step) = picked_step(&steps).expect("a step");
        assert_eq!(task, "06A_MSQ_18_ReturnSiriusJewel_interact_puzzle2_stone1");
        assert!(matches!(step, PlanStep::NotPlayed(_)));
    }

    #[test]
    fn plan_lines_read_into_steps() {
        let read = |line: &str| plan_step(line, header()).map(|(_, step)| step);
        assert_eq!(
            read("plan g_talk talk Walk_Npc at 1029.0 10.0 50.0"),
            Some(PlanStep::Talk {
                npc: "Walk_Npc".to_owned(),
                at: Some(Position {
                    x: 1029.0,
                    y: 10.0,
                    z: 50.0
                })
            })
        );
        assert_eq!(
            read("plan g_kill kill Test_Boar 2 at 1054.0 10.0 50.0 territory 42"),
            Some(PlanStep::Kill {
                vitals: "Test_Boar".to_owned(),
                count: 2,
                at: Position {
                    x: 1054.0,
                    y: 10.0,
                    z: 50.0
                },
                territory: Some(42),
                placed: false,
                within: None,
                touch: Vec::new()
            })
        );
        assert!(
            matches!(
                read("plan w_kill kill Test_Boar 3 at 1064.0 10.0 50.0 placed"),
                Some(PlanStep::Kill { placed: true, .. })
            ),
            "only the placed mobs count"
        );
        assert!(
            matches!(
                read("plan w_kill kill Test_Boar 1 at 1064.0 10.0 50.0 placed within 30.0"),
                Some(PlanStep::Kill {
                    placed: true,
                    within: Some(30.0),
                    ..
                })
            ),
            "the bot fights in the encounter's volume"
        );
        assert_eq!(
            read(
                "plan t kill Abominable_Liang_MSQ 1 at 1045.1 943.4 130.0 placed \
                 touch 1041.1 938.4 130.0 touch 1034.1 938.4 130.0"
            )
            .and_then(|step| match step {
                PlanStep::Kill { touch, .. } => Some(touch),
                _ => None,
            }),
            Some(vec![
                Position {
                    x: 1041.1,
                    y: 938.4,
                    z: 130.0
                },
                Position {
                    x: 1034.1,
                    y: 938.4,
                    z: 130.0
                }
            ]),
            "the bot walks onto the talisman piles' places"
        );
        assert_eq!(
            read("plan p strike G1 at 1.0 2.0 3.0 G2 at 4.0 5.0 6.0"),
            Some(PlanStep::Strike(vec![
                Position {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0
                },
                Position {
                    x: 4.0,
                    y: 5.0,
                    z: 6.0
                },
            ])),
            "a strike plan names each gong's place"
        );
        assert_eq!(
            read("plan t topic 0308314e after talk Grace at 1.0 2.0 3.0"),
            Some(PlanStep::Topic {
                topic: 0x0308_314e,
                talk: Some((
                    "Grace".to_owned(),
                    Position {
                        x: 1.0,
                        y: 2.0,
                        z: 3.0
                    }
                ))
            })
        );
        assert_eq!(
            read("plan t item 12A_Key 1 from the drop of quest object Box"),
            Some(PlanStep::Item {
                item: "12A_Key".to_owned(),
                count: 1,
                source: "the drop of quest object Box".to_owned(),
                gathered: false,
                node: None
            })
        );
        assert_eq!(
            read("plan t item Orb 2 gathered node Walk_Orb from the drop of quest object Walk_Orb"),
            Some(PlanStep::Item {
                item: "Orb".to_owned(),
                count: 2,
                source: "the drop of quest object Walk_Orb".to_owned(),
                gathered: true,
                node: Some("Walk_Orb".to_owned())
            }),
            "the bot gathers the nodes of Walk_Orb"
        );
        assert_eq!(
            read("plan t client cutscene 1a2b3c4d"),
            Some(PlanStep::Client(vec![
                ClientFacetMsg::RequestCutscene {
                    header: header(),
                    cutscene: 0x1a2b_3c4d
                },
                ClientFacetMsg::CutsceneEnded { header: header() }
            ]))
        );
        assert_eq!(
            read("plan t stop Broken: no mob that counts as Dragon spawns in the open world"),
            Some(PlanStep::Broken(
                "Broken: no mob that counts as Dragon spawns in the open world".to_owned()
            ))
        );
        assert!(matches!(
            read("plan t stop NotWalked: needs a run of game mode X"),
            Some(PlanStep::NotPlayed(_))
        ));
        assert_eq!(read("plan t wait"), Some(PlanStep::Wait));
        assert_eq!(
            read(
                "plan t achievements A (from a step of quest 06_beach), B (from the end of quest 12_x)"
            ),
            Some(PlanStep::Achievements {
                needs: "A (from a step of quest 06_beach), B (from the end of quest 12_x)"
                    .to_owned(),
                sources: vec!["06_beach".to_owned(), "12_x".to_owned()]
            })
        );
        assert_eq!(read("running q"), None);
    }

    #[test]
    fn a_kill_step_that_killed_nothing_is_not_played_and_one_whose_kills_did_not_count_is_broken() {
        let fought = |mob_deaths| Fought {
            task: "w_kill".to_owned(),
            hits: 170,
            mob_deaths,
            fights: 3,
            last_hits: 0,
            spared: Vec::new(),
        };
        assert_eq!(
            Fought::verdict(Some(&fought(0)), "w_kill", "stop".to_owned()),
            Verdict::NotPlayed("w_kill: the bot landed 170 hits and no mob died (stop)".to_owned())
        );
        assert_eq!(
            Fought::verdict(Some(&fought(4)), "w_kill", "stop".to_owned()),
            Verdict::Broken("stop; 4 mobs died near the bot".to_owned())
        );
        assert_eq!(
            Fought::verdict(Some(&fought(0)), "other", "stop".to_owned()),
            Verdict::Broken("stop".to_owned()),
            "the fights of another step"
        );
    }

    #[test]
    fn a_teleport_asks_for_a_place_a_metre_above_its_point() {
        let horus = Position {
            x: 1297.5,
            y: 1442.9,
            z: 929.3,
        };
        assert_eq!(teleport_line(horus), "!tp 1297.5 1442.9 930.3");
    }

    #[test]
    fn a_fight_that_landed_no_hit_spares_its_mobs_until_hits_land() {
        const HORUS: u64 = 1;
        const BRUTE: u64 = 2;
        let spared = spared_after(Vec::new(), vec![HORUS], 0);
        assert_eq!(spared, [HORUS], "no hit landed on Horus");
        let spared = spared_after(spared, vec![HORUS], 0);
        assert_eq!(spared, [HORUS], "once");
        assert!(
            spared_after(spared, vec![BRUTE], 40).is_empty(),
            "hits landed"
        );
    }

    #[test]
    fn the_bot_fights_on_while_its_hits_land() {
        let fought = |fights, last_hits| Fought {
            task: "w_kill".to_owned(),
            hits: 56,
            mob_deaths: 0,
            fights,
            last_hits,
            spared: Vec::new(),
        };
        assert!(Fought::fights_on(Some(&fought(1, 56)), "w_kill"));
        assert!(
            !Fought::fights_on(Some(&fought(1, 0)), "w_kill"),
            "no hit landed"
        );
        assert!(
            !Fought::fights_on(Some(&fought(MAX_KILL_FIGHTS, 56)), "w_kill"),
            "the most fights"
        );
        assert!(!Fought::fights_on(Some(&fought(1, 56)), "other"));
        assert!(!Fought::fights_on(None, "w_kill"));
    }

    /// `WorldEvent_Enc_99B_MSQ_YonasRetreat` (a sphere of 30 m, a grace of 1 s) reset when the
    /// bot ran 50 m north after the fort's placed mobs: the bot fights only in the volume.
    #[test]
    fn a_fight_in_an_area_takes_only_the_mobs_in_reach_inside_it() {
        const RADIUS_M: f32 = 30.0;
        let place = Position {
            x: 9452.7,
            y: 6765.8,
            z: 254.1,
        };
        let area = Some((place, RADIUS_M));
        assert!(in_area(area, [place.x, place.y + 20.0]), "a wave mob");
        assert!(!in_area(area, [place.x, place.y + 52.0]), "a fort mob");
        assert!(
            !in_area(area, [place.x, place.y + RADIUS_M - 1.0]),
            "the bot at its reach would stand out of the area"
        );
        assert!(in_area(None, [place.x, place.y + 52.0]), "no area");
    }

    #[test]
    fn a_kill_fights_the_nearest_mob_of_its_vitals_id_first() {
        // Live 2026-11-22 13:58: wolves came at the bot beside the fleeing elk of the step.
        const WOLF: u64 = 1;
        const ELK: u64 = 2;
        const WOLF_VITALS: u32 = 10;
        const ELK_VITALS: u32 = 20;
        let at = |x| nw_messages::WorldPosition::from_metres(x, 0.0, 0.0).expect("place");
        let mobs = [(WOLF, at(2.0)), (ELK, at(9.0))];
        let vitals_of = |entity| match entity {
            WOLF => Some(WOLF_VITALS),
            ELK => Some(ELK_VITALS),
            _ => None,
        };
        let from = [0.0, 0.0, 0.0];
        assert_eq!(
            fight_target(&mobs, vitals_of, |_| false, None, Some(ELK_VITALS), from),
            Some((ELK, at(9.0))),
            "the elk, though the wolf is nearer"
        );
        assert_eq!(
            fight_target(&mobs, vitals_of, |_| false, None, None, from),
            Some((WOLF, at(2.0))),
            "no vitals id: the nearest mob"
        );
        assert_eq!(
            fight_target(
                &[(WOLF, at(2.0))],
                vitals_of,
                |_| false,
                None,
                Some(ELK_VITALS),
                from
            ),
            Some((WOLF, at(2.0))),
            "no elk in view: the nearest other mob"
        );
        let elk_takes_no_hits = |entity| entity == ELK;
        assert_eq!(
            fight_target(
                &mobs,
                vitals_of,
                elk_takes_no_hits,
                None,
                Some(ELK_VITALS),
                from
            ),
            Some((WOLF, at(2.0))),
            "the elk takes no hits now: the wolf"
        );
        assert_eq!(
            fight_target(&mobs, vitals_of, |_| true, None, Some(ELK_VITALS), from),
            Some((ELK, at(9.0))),
            "none takes hits: the step's mob"
        );
    }

    #[test]
    fn a_fight_whose_hits_land_and_whose_mob_lives_goes_on() {
        // More rounds than the four extensions of the first fight rule (105 s): the Cutlass
        // Keys admiral takes some 13 minutes of the bot's hits.
        const FIGHT_ROUNDS: u32 = 52;
        let fight = || Fight {
            task: "16_Side_20_d_02_kill_lion".to_owned(),
            foe: false,
            area: None,
            vitals: None,
            saw_mobs: true,
            swings: 0,
            hits_before: 0,
            mob_deaths_before: 0,
            hits_at_check: 0,
            extensions: 0,
            bundles_before: 0,
            mob_cards_before: 0,
            touch: Vec::new(),
            next_touch: 0,
            swings_since_touch: SWINGS_PER_TOUCH,
        };
        let mut lion = fight();
        for round in 1..=FIGHT_ROUNDS {
            assert!(lion.goes_on(u64::from(round) * 10, 0), "round {round}");
        }
        assert!(
            !lion.goes_on(u64::from(FIGHT_ROUNDS) * 10, 0),
            "no hit landed since the last check"
        );
        let mut idle = fight();
        assert!(!idle.goes_on(0, 0), "no hit landed");
        let mut won = fight();
        assert!(
            !won.goes_on(10, 1),
            "a mob died: the loot and the skin come next"
        );
    }

    #[test]
    fn the_afk_warning_is_known_from_the_message_that_takes_it_back() {
        // Retail 2139: flag 1 and 120.0 seconds left in an expedition, then flag 0 and 0.0.
        const OWN: u64 = 7;
        const SECONDS_LEFT: f32 = 120.0;
        let received = |message: AfkWarningMsg| {
            let message = ServerMessage::AfkWarning(message);
            Received {
                channel: 0,
                envelope: nw_reflect::Envelope::message(
                    nw_reflect::TypeId::FileSlot(message.type_index()),
                    message.encode().expect("encode"),
                ),
            }
        };
        assert!(is_afk_warning(&received(AfkWarningMsg::warning(
            OWN,
            SECONDS_LEFT
        ))));
        assert!(!is_afk_warning(&received(AfkWarningMsg::cleared(OWN))));
    }

    #[test]
    fn a_fight_with_touch_places_walks_onto_each_in_turn_between_its_swings() {
        let place = |x: f32| Position {
            x,
            y: 938.4,
            z: 130.0,
        };
        let mut fight = Fight {
            task: "t_kill".to_owned(),
            foe: false,
            area: None,
            vitals: None,
            saw_mobs: false,
            swings: 0,
            hits_before: 0,
            mob_deaths_before: 0,
            hits_at_check: 0,
            extensions: 0,
            bundles_before: 0,
            mob_cards_before: 0,
            touch: vec![place(1041.1), place(1034.1)],
            next_touch: 0,
            swings_since_touch: SWINGS_PER_TOUCH,
        };
        assert_eq!(fight.touch_due(), Some(place(1041.1)), "a pile first");
        fight.touched();
        assert_eq!(fight.touch_due(), None, "then its swings");
        fight.swings_since_touch = SWINGS_PER_TOUCH;
        assert_eq!(fight.touch_due(), Some(place(1034.1)));
        fight.touched();
        fight.swings_since_touch = SWINGS_PER_TOUCH;
        assert_eq!(fight.touch_due(), Some(place(1041.1)), "round again");
        fight.touch.clear();
        assert_eq!(fight.touch_due(), None, "a fight without touch places");
    }

    #[test]
    fn a_fight_without_a_mob_in_view_plays_nothing_of_its_quest() {
        let mut fight = Fight {
            task: "w_kill".to_owned(),
            foe: false,
            area: None,
            vitals: None,
            saw_mobs: false,
            swings: 0,
            hits_before: 0,
            mob_deaths_before: 0,
            hits_at_check: 0,
            extensions: 0,
            bundles_before: 0,
            mob_cards_before: 0,
            touch: Vec::new(),
            next_touch: 0,
            swings_since_touch: SWINGS_PER_TOUCH,
        };
        let view = CombatView::default();
        let at = Some(Position {
            x: 6088.6,
            y: 834.6,
            z: 129.3,
        });
        assert_eq!(
            fight.not_played(&view, at),
            Some(Verdict::NotPlayed(
                "w_kill: the bot saw no mob there in 45 s (it stood at 6088.6 834.6 129.3; in the fight it read 0 world bundles and 0 mob cards)".to_owned()
            ))
        );
        fight.saw_mobs = true;
        assert_eq!(
            fight.not_played(&view, at),
            None,
            "the quest's state decides"
        );
    }

    #[test]
    fn the_bot_gathers_the_nearest_node_of_the_kind_it_did_not_try() {
        const ORB: u32 = 0x0b0b;
        const STONE: u32 = 0x5705;
        let nodes = [
            GatherNode {
                gatherable: ORB,
                at: [100.0, 0.0, 0.0],
            },
            GatherNode {
                gatherable: STONE,
                at: [1.0, 0.0, 0.0],
            },
            GatherNode {
                gatherable: ORB,
                at: [10.0, 0.0, 0.0],
            },
        ];
        let mut tried = HashSet::new();
        let from = [0.0, 0.0, 0.0];
        assert_eq!(nearest_untried(&nodes, &tried, ORB, from), Some(2));
        tried.insert(2);
        assert_eq!(nearest_untried(&nodes, &tried, ORB, from), Some(0));
        tried.insert(0);
        assert_eq!(
            nearest_untried(&nodes, &tried, ORB, from),
            None,
            "each tried"
        );
    }

    #[test]
    fn the_report_counts_each_verdict_and_lists_each_quest() {
        let report = PlayReport {
            outcome: RepOutcome::default(),
            activities: Vec::new(),
            quests: vec![
                QuestPlay {
                    quest: "A".to_owned(),
                    verdict: Verdict::Completed,
                    actions: vec!["accepted".to_owned(), "turned in".to_owned()],
                },
                QuestPlay {
                    quest: "B".to_owned(),
                    verdict: Verdict::Broken("t: no | place".to_owned()),
                    actions: Vec::new(),
                },
            ],
        };
        let text = report.markdown();
        assert!(text.contains("| Completed | 1 |"));
        assert!(text.contains("| Broken | 1 |"));
        assert!(text.contains("| A | Completed |  | accepted; turned in |"));
        assert!(text.contains("| B | Broken | t: no \\| place |  |"));
    }
}
