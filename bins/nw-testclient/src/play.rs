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

use std::collections::HashSet;
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use nw_carrier::MESSAGE_CHANNEL;
use nw_content::poi_tags::PoiTags;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::client_input::TALK_KIND;
use nw_messages::entity_ids::{facet_of_raw_id, raw_id_of_entity};
use nw_messages::social::SocialFacet;
use nw_messages::social::chat::{ChatCall, ChatChannel};
use nw_messages::{
    Bit8, ClientFacetMsg, ClientInputMsg, ConversationClientMsg, FacetHeader, InputPosition,
    InteractionRequest,
};
use nw_reflect::LoginToken;
use nw_sim_core::{Position, yaw_toward};
use reqwest::Client;

use crate::activity::{
    Act, Activity, ActivityKind, OPEN_STORAGE_KIND, OPEN_STORAGE_NAME, Proof, STORAGE_TAG, Step,
    arena_verdict, camp_steps, door_trial_steps, gather_steps, leave_trial_steps, loot_steps,
    lore_steps, mission_turn_in_steps, quest_craft_steps, skin_steps, start_object_steps,
    trial_steps, use_steps,
};
use crate::chat::{chat_lines, is_system};
use crate::gather::GatherNode;
use crate::group::{GroupSeat, accept_call, invite_call};
use crate::hunt::{planar, send_input};
use crate::pair::{PairError, RecordWalk, SEND_INTERVAL, session_with};
use crate::record_walk::{ARRIVED_M, RecordWalker};
use crate::rep::{RepClient, RepError};
use crate::udp::RepOutcome;
use crate::view::{CombatView, InteractionAnswer, WorldView};
use crate::walk::{WalkSettings, login_token, walk};

/// The game master lines after the tutorial's exit, before the first quest: the bot does not
/// die, lives again when a mob killed it before (a dead player wakes no mob), is at the top
/// level (the sim holds it to the game data's highest), and wears the best item of each kind at
/// its top gear score, so its level and gear never limit a kill (a starter sword hit a level 34
/// bear of 8,776 health for 42). It carries the tier 5 gathering tools, as a player at the top
/// level does, which `!gearup` puts into their tool slots from the bag (a quest gather of a
/// player with no pick is refused: `99E_MSQ_02_MyrddinCrystalsA`, `NoTool`).
const SETUP_LINES: [&str; 9] = [
    "!god on",
    "!heal",
    "!level 65",
    "!give 2hPickT5 1",
    "!give 2hAxeT5 1",
    "!give 1hSickleT5 1",
    "!give 1hSkinningKnifeT5 1",
    "!give 2hFishingPoleT5 1",
    "!gearup",
];
/// The game master lines the bot sends at its setup: [`SETUP_LINES`], then a start of each
/// quest of `gm_start` ([`PlaySettings::gm_start`]).
fn setup_lines(gm_start: &[String]) -> Vec<String> {
    SETUP_LINES
        .iter()
        .map(|line| (*line).to_owned())
        .chain(gm_start.iter().map(|quest| format!("{GM_QUEST} {quest}")))
        .collect()
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
/// Longest fight for one kill step.
const FIGHT_TIME: Duration = Duration::from_secs(45);
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
const SALVAGE_FIRST: u32 = 1;
/// The number of unknown meaning of a salvage call: 0 in all 173 retail calls.
const SALVAGE_UNKNOWN: u32 = 0;
/// How many of a stack one open takes (Inferred: 1 in all 173 retail calls).
const SALVAGE_ONE: u32 = 1;

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
    /// the bot does not play is the stop it names.
    const fn pick_order(&self) -> u8 {
        match self {
            Self::Achievements { .. } => 1,
            Self::NotPlayed(_) => 2,
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
        "event" => PlanStep::NotPlayed(format!(
            "needs a flow the bot does not play yet: {}",
            what.trim_start_matches("event").trim()
        )),
        _ => return None,
    };
    Some(step)
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
    /// What came before the step's first act: system lines, messages, interaction answers.
    lines_before: usize,
    messages_before: usize,
    answers_before: usize,
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
            lines_before: 0,
            messages_before: 0,
            answers_before: 0,
            interaction: None,
            proved: Vec::new(),
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
    own_since: Option<Duration>,
    lines: Vec<String>,
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
    input_counter: u16,
    interaction: u64,
    /// The activities still to play, and the one played now.
    activities: std::collections::VecDeque<Activity>,
    /// The activities come before the quests ([`PlayData::activities_first`]).
    activities_first: bool,
    running: Option<(ActivityKind, RunningSteps)>,
    /// Every gather node of the level, in world file order.
    nodes: Arc<[GatherNode]>,
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
    /// The fights for the current kill step.
    fought: Option<Fought>,
    /// The bot's own body on its walk ([`Act::Walk`]).
    walker: Option<RecordWalker>,
}

/// What the bot did in a fight for a kill step.
struct Fight {
    task: String,
    /// Whether it saw a mob at any time of the fight.
    saw_mobs: bool,
    swings: u32,
    /// The view's hits dealt and mob deaths when the fight started.
    hits_before: u64,
    mob_deaths_before: u64,
    /// The view's bundles and mob cards read when the fight started: a fight that saw no mob
    /// tells by them whether the server sent the bot its surroundings.
    bundles_before: u64,
    mob_cards_before: u64,
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

impl Fight {
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
        }
        if let Some(own) = self.combat.time_sync_due() {
            send_time_sync(client, own)?;
        }
        self.lines.extend(
            chat_lines(&outcome.messages[self.read..])
                .into_iter()
                .filter(is_system)
                .map(|line| line.body),
        );
        self.read = outcome.messages.len();
        self.answers.extend(self.combat.take_interactions());
        let Some(own) = self.world.own_number() else {
            return Ok(());
        };
        let first = *self.own_since.get_or_insert(since);
        if since.saturating_sub(first) < FIRST_LINE_DELAY {
            return Ok(());
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
            Phase::Pause { until, then } if since >= until => self.ask(client, own, then, since),
            Phase::Pause { .. } => Ok(()),
            Phase::Fight { until } if since >= until => self.fight_ended(client, own, since),
            Phase::Fight { .. } => self.fight(client, since),
            Phase::Gather => self.gather_tick(client, outcome, own, since),
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
        let steps: Vec<(String, PlanStep)> = answer
            .iter()
            .filter_map(|line| plan_step(line, header))
            .collect();
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
            PlanStep::Skin { kill, .. } if self.fresh_corpse().is_none() => *kill,
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
        let fights_on =
            matches!(step, PlanStep::Kill { .. }) && Fought::fights_on(self.fought.as_ref(), &task);
        if self.last_plan.as_deref() == Some(line.as_str()) && step != PlanStep::Wait && !fights_on
        {
            self.same_plan += 1;
        } else {
            if self.last_plan.as_deref() != Some(line.as_str()) {
                // The step moved on (a kill counted): its fights start over.
                self.fought = None;
            }
            self.same_plan = 1;
            self.last_plan = Some(line.clone());
        }
        if self.same_plan > MAX_SAME_PLAN {
            let stop = format!(
                "{task}: the step did not end after the bot did it {MAX_SAME_PLAN} times ({line})"
            );
            let verdict = if matches!(step, PlanStep::Kill { .. }) {
                Fought::verdict(self.fought.as_ref(), &task, stop)
            } else {
                Verdict::Broken(stop)
            };
            self.end_quest(verdict);
            return self.next_quest(client, own, since);
        }
        // A step the instance's sim cannot place lies outside the instance: the bot leaves it
        // first, as a player walks out after the trial's own steps.
        if matches!(step, PlanStep::Broken(_) | PlanStep::NotPlayed(_))
            && let Some(mode) = self.in_trial.take()
        {
            self.gather = Some((
                format!("{task}: left the instance of {mode}"),
                RunningSteps::of(leave_trial_steps()),
            ));
            self.phase = Phase::Gather;
            return Ok(());
        }
        match step {
            PlanStep::Wait => self.pause(since, WAIT_PAUSE, Ask::State),
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
            } => {
                self.teleport(client, own, at)?;
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
                    saw_mobs: false,
                    swings: 0,
                    hits_before: self.combat.hits_dealt(),
                    mob_deaths_before: self.combat.mob_deaths(),
                    bundles_before: self.combat.bundles(),
                    mob_cards_before: self.combat.mob_cards(),
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
                gatherable, count, ..
            } => {
                let Some((corpse, died_at)) = self.fresh_corpse() else {
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
                    self.in_trial = Some(mode);
                }
                self.actions.push(what);
            }
            Some(Verdict::Broken(why) | Verdict::NotPlayed(why)) => {
                self.entering_trial = None;
                self.actions.push(format!("{what}: {why}"));
            }
            None => {
                self.gather = Some((what, running));
                return Ok(());
            }
        }
        self.ask(client, own, Ask::State, since)
    }

    /// The last mob that died in view, while its corpse is shown and the bot has not skinned it.
    fn fresh_corpse(&self) -> Option<(u64, nw_messages::WorldPosition)> {
        self.combat
            .last_death()
            .filter(|(corpse, _)| !self.skinned.contains(corpse))
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
            if running.act == 0 {
                running.lines_before = self.lines.len();
                running.messages_before = outcome.messages.len();
                running.answers_before = self.answers.len();
            }
            match act {
                Act::Gm(line) => self.say(client, own, line)?,
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
        let new_answers = &self.answers[running.answers_before..];
        let proof = match &step.proof {
            Proof::Sent => Some(Ok(())),
            Proof::Line(takes, _) => self.lines[running.lines_before..]
                .iter()
                .any(|line| takes(line))
                .then_some(Ok(())),
            Proof::Message(type_index) => outcome.messages[running.messages_before..]
                .iter()
                .any(|received| received.envelope.type_index() == Some(*type_index))
                .then_some(Ok(())),
            Proof::ArenaYes(type_index) => outcome.messages[running.messages_before..]
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
        };
        match proof {
            Some(Ok(())) => {
                running.proved.push(step.what.clone());
                running.step += 1;
                running.act = 0;
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
                };
                let answered = self.lines[running.lines_before..].join(" / ");
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
        self.say(
            client,
            own,
            &format!("!tp {:.1} {:.1} {:.1}", at.x, at.y, at.z),
        )
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
        self.fought = Some(match self.fought.take() {
            Some(fought) if fought.task == fight.task => Fought {
                hits: fought.hits + hits,
                mob_deaths: fought.mob_deaths + mob_deaths,
                fights: fought.fights + 1,
                last_hits: hits,
                ..fought
            },
            _ => Fought {
                task: fight.task,
                hits,
                mob_deaths,
                fights: 1,
                last_hits: hits,
            },
        });
        self.ask(client, own, Ask::State, since)
    }

    /// Walks to the nearest mob and swings at it.
    fn fight(&mut self, client: &mut RepClient, since: Duration) -> Result<(), RepError> {
        let Some(own) = self.own_place() else {
            return Ok(());
        };
        let from = [own.x, own.y, own.z];
        let Some((_, mob)) = self
            .combat
            .mobs()
            .min_by(|(_, a), (_, b)| planar(from, [a.x, a.y]).total_cmp(&planar(from, [b.x, b.y])))
        else {
            return Ok(());
        };
        if let Some(fight) = &mut self.fight {
            fight.saw_mobs = true;
        }
        let distance = planar(from, [mob.x, mob.y]);
        if distance > REACH_M {
            if self
                .last_move
                .is_some_and(|last| since.saturating_sub(last) < MOVE_INTERVAL)
            {
                return Ok(());
            }
            self.last_move = Some(since);
            let length = (SPEED_MPS * MOVE_INTERVAL.as_secs_f32()).min(distance - REACH_M / 2.0);
            let x = own.x + (mob.x - own.x) / distance * length;
            let y = own.y + (mob.y - own.y) / distance * length;
            let Some(position) = InputPosition::from_metres(x, y, mob.z()) else {
                return Ok(());
            };
            // The server sends no own place for the bot's own moves: the bot stands where it
            // sent itself.
            self.teleported_to = Some(Position { x, y, z: mob.z() });
            return send_input(client, &ClientInputMsg::position(position));
        }
        if self
            .last_swing
            .is_some_and(|last| since.saturating_sub(last) < SWING_INTERVAL)
        {
            return Ok(());
        }
        self.last_swing = Some(since);
        let Some(at) = InputPosition::from_metres(own.x, own.y, own.z) else {
            return Ok(());
        };
        if let Some(fight) = &mut self.fight {
            fight.swings += 1;
        }
        let yaw = yaw_toward(
            own,
            Position {
                x: mob.x,
                y: mob.y,
                z: mob.z(),
            },
        );
        let counter = self.input_counter;
        self.input_counter = counter.wrapping_add(ATTACK_RECORDS);
        send_input(
            client,
            &ClientInputMsg::attack(at, counter).looking([yaw, LEVEL_PITCH]),
        )
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

    /// Goes on with the next quest to play: the queue's next, else the deferred ones again when
    /// a quest completed since they were deferred. Each deferred quest that no NPC ever offered
    /// ends as not played, with its reason.
    fn advance(
        &mut self,
        client: &mut RepClient,
        own: u64,
        since: Duration,
    ) -> Result<(), RepError> {
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
        self.ask(client, own, Ask::State, since)
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
    /// The level's gather nodes.
    pub nodes: Arc<[GatherNode]>,
    /// The gatherables (the CRC-32 of each lowercase id) of the nodes that are loot containers.
    pub containers: HashSet<u32>,
    /// The territories' `POITag`s.
    pub poi_tags: PoiTags,
    /// The bot's seat in a group of bots ([`crate::group`]); `None` for a bot alone.
    pub group: Option<GroupSeat>,
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
    data: PlayData,
    new_client: impl Fn(LoginToken) -> Result<RepClient, RepError>,
    mut report: impl FnMut(String),
) -> Result<PlayReport, PairError> {
    let PlayData {
        activities,
        activities_first,
        nodes,
        containers,
        poi_tags,
        group,
    } = data;
    let admission = walk(http, &settings.account, &mut report).await?;
    if let Some(seat) = &group {
        seat.set_character(admission.token.character_id.clone());
    }
    let mut player = Player {
        world: WorldView::default(),
        combat: CombatView::default(),
        read: 0,
        own_since: None,
        lines: Vec::new(),
        phase: Phase::LeaveTutorial,
        setup_lines: setup_lines(&settings.gm_start),
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
        input_counter: FIRST_INPUT_COUNTER,
        interaction: 0,
        activities: activities.into(),
        activities_first,
        running: None,
        nodes,
        containers,
        poi_tags,
        tried_nodes: HashSet::new(),
        skinned: HashSet::new(),
        loot_from: 0,
        gather: None,
        in_trial: None,
        entering_trial: None,
        activity_plays: Vec::new(),
        answers: Vec::new(),
        teleported_to: None,
        entered_areas: Vec::new(),
        state_lines: Vec::new(),
        fight: None,
        fought: None,
        walker: None,
    };
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

#[cfg(test)]
mod tests {
    use super::*;

    const OWN: u64 = 7;

    fn header() -> FacetHeader {
        FacetHeader::of(CALL_COMPONENT, OWN)
    }

    #[test]
    fn the_setup_starts_each_gm_started_quest_after_the_bot_s_own_lines() {
        let lines = setup_lines(&["12A_MSQ_09_ReturnRegent".to_owned()]);
        assert_eq!(&lines[..SETUP_LINES.len()], SETUP_LINES);
        assert_eq!(
            lines[SETUP_LINES.len()..],
            ["!quest 12A_MSQ_09_ReturnRegent"]
        );
        assert_eq!(setup_lines(&[]).len(), SETUP_LINES.len());
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
                    placed: false
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
                    placed: true
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
                placed: false
            })
        );
        assert!(
            matches!(
                read("plan w_kill kill Test_Boar 3 at 1064.0 10.0 50.0 placed"),
                Some(PlanStep::Kill { placed: true, .. })
            ),
            "only the placed mobs count"
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
    fn the_bot_fights_on_while_its_hits_land() {
        let fought = |fights, last_hits| Fought {
            task: "w_kill".to_owned(),
            hits: 56,
            mob_deaths: 0,
            fights,
            last_hits,
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

    #[test]
    fn a_fight_without_a_mob_in_view_plays_nothing_of_its_quest() {
        let mut fight = Fight {
            task: "w_kill".to_owned(),
            saw_mobs: false,
            swings: 0,
            hits_before: 0,
            mob_deaths_before: 0,
            bundles_before: 0,
            mob_cards_before: 0,
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
