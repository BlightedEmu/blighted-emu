// SPDX-License-Identifier: AGPL-3.0-or-later
//! The player activities of `nw-testclient play` besides quests: each is a script of steps, and
//! each step sends client calls (or a game master line) and waits for the server's proof that
//! it worked: an answer line, a server message of a type, or the answer to an interaction. The
//! places and ids (a gather node, a crafting station, an inn, a shrine, a game mode, a town
//! mission) come from the level's world file and the content bundle, as the client knows them.
//! An activity the bot cannot play names why instead of steps.

use std::time::Duration;

use clap::ValueEnum;
use nw_content::Tables;
use nw_content::crafting::{IngredientKind, Recipes};
use nw_content::instances::{InstanceCatalog, InstanceKind};
use nw_content::poi_tags::PoiTags;
use nw_content::quick_courses::QuickCourseRules;
use nw_content::starter::StarterKits;
use nw_content::town_props::ASSET_ID_LEN;
use nw_content::town_stations::TownStations;
use nw_content::war_forts::{CLAIM_POINT_NUMBER, WarForts};
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::arena::{
    ActivateResponse, ArenaClientMsg, ArenaServerMsg, DUNGEON_ACTIVATE_RESULT_TYPE_INDEX,
    PLAYER_ARENA_COMPONENT, REMOTE_INTERACT_RESULT_TYPE_INDEX,
};
use nw_messages::client_input::{
    APPLY_ALL_RESOURCES_KIND, CRAFTING_STATION_KIND, ENTER_DUNGEON_KIND, GATHER_KIND, LOOT_KIND,
    LORE_KIND, OPEN_DUNGEON_MENU_KIND, TOWN_BOARD_KIND, TOWN_BOARD_NAME, WAR_BOARD_KIND,
    WAR_BOARD_NAME,
};
use nw_messages::conversation_client::ObjectiveInteractorClientMsg;
use nw_messages::facet::LEFT_GAME_MODE_QUEUE_TYPE_INDEX;
use nw_messages::facet::{
    CRAFTING_PROGRESS_TYPE_INDEX, INGREDIENT_ITEM, TELEPORT_WITH_LOADING_SCREEN_TYPE_INDEX,
    component as facet_component,
};
use nw_messages::game_mode::{
    ARENA_ACTIVITY, CAPTURE_THE_FLAG_ACTIVITY, MATCHMAKING_LOGIN_REQUESTED_TYPE_INDEX,
    OUTPOST_RUSH_ACTIVITY, ROLE_SELECTION_CANCELED_TYPE_INDEX,
    START_MATCHMAKING_ROLE_SELECTION_TYPE_INDEX,
};
use nw_messages::mount::{
    MOUNT_FACET, MountClientMsg, QUICK_COURSE_ENDED_TYPE_INDEX, SUMMON_PENDING_TYPE_INDEX,
};
use nw_messages::music::{
    MUSIC_CHILD, MUSIC_FACET, MusicClientMsg, PERFORMANCE_CREATED_TYPE_INDEX,
};
use nw_messages::social::SocialFacet;
use nw_messages::social::groups::{GroupCall, LEAVE_VALUE, SIEGE_WARFARE_ENDED_TYPE_INDEX};
use nw_messages::travel::{
    LandmarkRef, PLACEMENT_RESULTS_TYPE_INDEX, StructurePlacement, TravelClientMsg, component,
};
use nw_messages::war_signup::{
    SET_ROSTER_RESPONSE_RECEIVED_TYPE_INDEX, SIDE_ATTACKER, SIDE_DEFENDER,
    SIGNUP_LIST_RECEIVED_TYPE_INDEX, SIGNUP_RESPONSE_RECEIVED_TYPE_INDEX,
};
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    BundleMember, ClientFacetMsg, CraftIngredient, FacetHeader, GameModeClientMsg,
    InteractionRequest, ItemRecord, LEVEL_INFO_CHANGED_MSG_TYPE_INDEX, MemberState, MessageError,
    QueueActivity, TERRITORY_FACET_READY_TYPE_INDEX, TERRITORY_INTERACTOR_COMPONENT,
    WarSignupClientMsg,
};
use nw_world::WorldFile;
use nw_world::capitals::capital_id;
use nw_world::quick_courses::QuickCoursePaths;
/// A game master line the sim carried out.
use sim::world::GM_DONE as DONE;

use crate::gather::{GatherNode, node_id};
use crate::group::{GroupSeat, accept_call};
use crate::pair::RecordWalk;
use crate::rep::Received;
use crate::view::Matchmaking;

/// The kind of an inn's open interaction (`travel::kind::OPEN_INN` of the sim; retail opened
/// the inn with it before the check in, live-inn note section 1).
const OPEN_INN_KIND: u8 = 70;
/// The kind of an inn check in on the same interaction (`travel::kind::SET_SPAWN_POINT`).
const SET_SPAWN_POINT_KIND: u8 = 18;
/// The interact tag of a settlement storage (`TaskInteract` of the vault and market steps).
pub const STORAGE_TAG: &str = "SettlementStorage";
/// The kind of a storage open (`item_calls::OPEN_STORAGE_KIND` of the sim; retail vault capture
/// 2026-09-27).
pub const OPEN_STORAGE_KIND: u8 = 26;
/// The name a storage open carries (retail vault capture 2026-09-27).
pub const OPEN_STORAGE_NAME: &[u8] = b"@ui_global_storage";
/// The landmark type of an inn recall (`INN_RESPAWN_LANDMARK` of the sim).
const INN_RECALL_LANDMARK: u8 = 20;
/// The slice name part of an inn's innkeeper capital (`slices/characters/mission/
/// npc_1222_innkeep`): a check in names the innkeeper's capital (the retail S8 check-in named
/// the Prydwen innkeeper, `NPC_1222_Innkeep`), and an inn recall lands in front of it.
const INNKEEPER_SLICE_MARK: &str = "_innkeep";
/// The `POITag` of a fast travel shrine's territory (live-inn note section 4: all 137 rows are
/// shrines).
const FAST_TRAVEL_TAG: &str = "fasttravel";
/// The gatherable the harvest gathers by hand (Observed: the retail flint request of the
/// gather note, newchar 739603, names `SingleStone`).
const HARVESTED_GATHERABLE: &str = "SingleStone";
/// The item of the recipe the craft makes (Observed: the retail smelt of the crafting note,
/// four `OreT1` at a `smelting2` station).
const CRAFTED_ITEM: &str = "IngotT2";
/// The quest that unlocks the first horse ("My Kingdom for a Horse").
const MOUNT_QUEST: &str = "9401_BefriendHorse";
/// The reward box the mount quest's end gives: the horse is owned once it is opened (content
/// bundle 25810808: its salvage fires `Reward_MountReward_Box_1`, whose achievement owns the
/// horse; retail mount quest capture, 283.210 s: the box opened (1733), then the first summon).
const MOUNT_REWARD_BOX: &str = "MountItem_MountReward_Box_1";
/// The wait from the summon's start to its authorization (retail: the client completed the
/// summon 3,049 to 3,070 ms after the 5747).
const SUMMON_WAIT: Duration = Duration::from_millis(3500);
/// How a `!queststate` line of a done quest starts.
const QUEST_DONE: &str = "done ";
/// The mount part of a `!state` line of a rider.
const STATE_RIDING: &str = " mount riding ";
/// The progression of azoth, which a fast travel spends.
const AZOTH: &str = "Azoth_Currency";
/// Azoth the bot gets before a fast travel: more than any trip costs.
const AZOTH_GIVEN: u32 = 500;
/// The level the level activity sets, then passes with experience.
const SET_LEVEL: u32 = 5;
/// Experience given after the set level: enough for the next levels.
const EXPERIENCE_GIVEN: u32 = 20_000;
/// Coins for a craft's fee.
const CRAFT_COINS: u32 = 10_000;
/// The rank of a craft's tradeskill.
const CRAFT_RANK: u32 = 200;
/// How long a step's proof may take.
pub(crate) const STEP_WAIT: Duration = Duration::from_secs(10);
/// How long a gather or a fast travel may take: the gather time, the 10 s channel.
pub(crate) const LONG_WAIT: Duration = Duration::from_secs(25);
/// The first words of the sim's `!state` line.
const STATE_LEVEL: &str = "state level ";
/// How the first word of the PvP part of the `!state` line starts.
const STATE_PVP: &str = " pvp ";

/// An activity a player does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ActivityKind {
    /// Level up: a set level, then experience.
    Level,
    /// The PvP flag on, then off.
    Pvp,
    /// The foe of another bot's PvP quest step (`C10A_Q6_KillPlayers`): god mode off, the PvP
    /// flag on in [`PVP_FOE_FACTION`], and up again after each death for [`FOE_STAND`]; the
    /// killer bot plays the quest with `--pvp-foe` naming this bot's character.
    PvpFoe,
    /// Harvest a gather node by hand.
    Harvest,
    /// Craft a recipe at a crafting station.
    Craft,
    /// Craft a recipe at a settlement station that the server spawns: its card gives its id.
    TownCraft,
    /// Check in at an inn, then recall to it.
    Inn,
    /// Fast travel at a shrine.
    Shrine,
    /// Ask to enter an expedition from afar.
    Dungeon,
    /// Take a town board mission.
    Board,
    /// Invite a second player into a group.
    Group,
    /// Summon and ride a mount.
    Mount,
    /// Play a song on an instrument.
    Music,
    /// A game master's company of the bot's faction owns Monarch's Bluffs, and the bot opens
    /// the town's war board; the town is given back to no owner at the end.
    Company,
    /// The bot joins the Covenant and makes a company that stays: the attacker of a game
    /// master's war test (`!war <territory id> <minutes to start> Bot War Company`), as a war
    /// needs an attacker company of a faction other than the owner's.
    EnemyCompany,
    /// The bot's company ([`ActivityKind::EnemyCompany`]) declares a game master war on the
    /// territory of [`WAR_TERRITORY`], and the bot uses that territory's war board from afar,
    /// signs up as the attacker, asks for the sign-up list and the roster and fills the roster
    /// as its war leader, as a client's war board screen does; the war is cancelled at the end. Its owner must be a company of
    /// another faction.
    WarSignup,
    /// The bot's company owns the territory of [`INVASION_TERRITORY`], a game master schedules
    /// its invasion (`!invasion`), and the bot uses its war board from afar and signs up as a
    /// defender, the only side of an invasion; the invasion is cancelled and the territory given
    /// back to no owner at the end.
    InvasionSignup,
    /// A check of the server's systems (saves, economy, social, character progress) by one or
    /// two bots at the same time: `nw-testclient systems` runs it ([`crate::systems`]).
    Systems,
    /// The bot queues for the 3v3 arena alone, accepts the match the queue finds, plays it (it
    /// fights the players of the other team) and is back in the open world at its end
    /// ([`queued_match_steps`]). The match needs as many bots in the queue as the world
    /// director's `[instances.arena] match_size`.
    Arena,
    /// As [`ActivityKind::Arena`] for Outpost Rush: team 0 takes the three outposts in turn,
    /// team 1 stays at its spawn ([`MatchGoal::Outposts`]).
    OutpostRush,
    /// As [`ActivityKind::Arena`] for capture the flag: team 0 takes team 1's flag to its own
    /// base, team 1 stays at its spawn ([`MatchGoal::Flags`]).
    CaptureTheFlag,
    /// The bot closes the arena queue's role picking screen, then joins and leaves the queue
    /// ([`queue_leave_steps`]): no match and no instance.
    QueueLeave,
}

/// One call of a step, written once the bot knows its own entity number.
pub type Call = Box<dyn Fn(u64) -> Result<(u64, Vec<u8>), MessageError>>;

/// What a bot saw, for an act or a proof that reads it ([`Act::Dynamic`], [`Proof::Check`]).
pub struct BotSight<'a> {
    /// The bot's own entity number.
    pub own: u64,
    /// The bot's bag, one record per slot ([`crate::view::CombatView::bag`]).
    pub bag: &'a [ItemRecord],
    /// The system chat lines since the step's first act.
    pub lines: &'a [String],
    /// The bodies of the player chat lines since the step's first act.
    pub heard: &'a [String],
    /// The server messages since the step's first act.
    pub messages: &'a [Received],
    /// Whether the bot's own newest vitals say it is dead.
    pub dead: bool,
    /// Whether the bot's own groups state names a group.
    pub in_group: bool,
    /// The bot's own queue and game modes ([`crate::view::WorldView::matchmaking`]).
    pub matchmaking: &'a Matchmaking,
    /// The first invite in the bot's own groups state (a group's or a war raid's).
    pub group_invite: Option<[u8; RAW_ID_LEN]>,
}

/// The calls of an act that reads what the bot saw: `None` to wait for a later tick, else the
/// calls to send (type index and body each; none for an act that only notes what it saw).
pub type Dynamic = Box<dyn Fn(&BotSight<'_>) -> Option<Result<Vec<(u64, Vec<u8>)>, MessageError>>>;

/// The request of an interaction written from what the bot saw: `None` to wait for a later tick.
pub type DynamicInteraction = Box<dyn Fn(&BotSight<'_>) -> Option<InteractionRequest>>;

/// A proof that reads what the bot saw: `None` while it waits, else whether the step worked
/// (with why not).
pub type Check = Box<dyn Fn(&BotSight<'_>) -> Option<Result<(), String>>>;

/// What a step sends.
pub enum Act {
    /// A game master line.
    Gm(String),
    /// A game master's teleport to the place; the bot's next inputs stand there.
    Teleport([f32; 3]),
    /// A press of the summon mount key in the bot's input.
    SummonKey,
    /// A toggle of the PvP flag (2525) unless the newest `!state` line shows the flag on or on
    /// its way on: a toggle then would put it off (live 2026-11-22 15:07: a foe bot flagged by
    /// an earlier run toggled it off).
    PvpFlagOn,
    /// The bag item of this client item index opens, as the client opens a loot container
    /// item (1733); nothing when the bag holds none (it was opened before).
    OpenBagItem(u32),
    /// A wait before the next act (a summon's authorization).
    Pause(Duration),
    /// A client call: its type index and body for the bot's own entity number.
    Call(Call),
    /// An interaction of kind `kind` with the entity whose card, among those the bot has seen,
    /// names one of the slice assets `assets` (an entity the server spawns, whose id only its
    /// card tells); the act waits for such a card.
    InteractCard {
        /// The slice assets the entity may show.
        assets: Vec<[u8; ASSET_ID_LEN]>,
        /// The interaction kind.
        kind: u8,
    },
    /// An interaction in the bot's input, with the interaction's name.
    Interact {
        /// The interaction; the bot gives it a fresh id.
        request: InteractionRequest,
        /// The name the interaction carries (an NPC's, a board's); empty for none.
        name: Vec<u8>,
    },
    /// The client ends the step's interaction with the object of facet entity `target_entity`,
    /// as a player who closes its screen (the server counts the object's interact tags then).
    EndInteraction {
        /// The object's facet entity.
        target_entity: u64,
    },
    /// The leader of a group of bots invites every other bot of its seat's group (3492), once
    /// each of them is in the world ([`crate::group`]).
    InviteGroup(GroupSeat),
    /// A bot of a group accepts the invite its own groups state shows (3495), once one shows.
    AcceptGroupInvite(GroupSeat),
    /// The leader of a group tells the other bots it is in the instance.
    LeaderInside(GroupSeat),
    /// The bot walks to `east_m` and `north_m` metres from the first server place of its seat
    /// (where it came into the instance) with input records, as the game client walks
    /// ([`crate::record_walk`]).
    Walk {
        /// The bot's seat, whose first place is the start.
        seat: GroupSeat,
        /// Metres east of the start.
        east_m: f32,
        /// Metres north of the start.
        north_m: f32,
        /// The device and the level's ground of the bot's own body.
        records: RecordWalk,
    },
    /// Calls written from what the bot saw ([`Dynamic`]); the act waits while it gives `None`.
    Dynamic(Dynamic),
    /// The bot fights the nearest mob within this many metres of where it stands now, each tick,
    /// while the next steps wait (a kill experience check); `None` ends the hunt.
    Hunt(Option<f32>),
    /// An interaction whose request is written from what the bot saw (the revive of another
    /// bot's player, whose number that bot tells); the act waits while it gives `None`. The bot
    /// gives it a fresh id.
    InteractDynamic(DynamicInteraction),
    /// The bot accepts each new pop of its queue ([`pop_to_accept`], [`accept_calls`]) until the
    /// match moves it (5449); with none within `longest` from the act's start the step fails.
    AcceptPops {
        /// The longest wait for the move.
        longest: Duration,
    },
    /// The bot plays a match or a war for `goal` until `end`; one that lasts longer than
    /// `longest` from the act's start fails the step.
    PlayMatch {
        /// What ends it.
        end: MatchEnd,
        /// What the bot does in it.
        goal: MatchGoal,
        /// The longest match.
        longest: Duration,
    },
}

/// What proves that a step worked.
pub enum Proof {
    /// A game master answer line that the test takes, described.
    Line(Box<dyn Fn(&str) -> bool>, String),
    /// A server message of this type index.
    Message(u64),
    /// An arena answer of this type index that says yes ([`arena_verdict`]); one that says no
    /// fails the step.
    ArenaYes(u64),
    /// The server took the step's interaction (3743 status 1).
    Accepted,
    /// The interaction ended (3746).
    Ended,
    /// The server answered the step's interaction (3743), taken or refused: a later step proves
    /// what it gave.
    Answered,
    /// Nothing: the server answers the call with no message (a toggle, a territory entry, a
    /// board take); a later step proves it.
    Sent,
    /// Every other bot of the seat's group is in the group.
    GroupFull(GroupSeat),
    /// The own groups state names a group; the seat tells the others.
    InGroup(GroupSeat),
    /// The leader of the seat's group is in the instance.
    LeaderInside(GroupSeat),
    /// The group's leader ended its play ([`GroupSeat::leader_done`]).
    LeaderDone(GroupSeat),
    /// The step's wait passed: a pause.
    Paused,
    /// A check of what the bot saw ([`Check`]), described.
    Check(Check, String),
}

/// What a bot does in a match of a queued game mode ([`Act::PlayMatch`]). The two roles of
/// Outpost Rush and capture the flag are a test plan (our design): one team plays the objective
/// alone, so that the match comes to its scored end.
/// The bot has no path finder and walls and gates stand between the places, so it goes to each
/// place with a game master's teleport.
#[derive(Debug, Clone, PartialEq)]
pub enum MatchGoal {
    /// Fight the nearest player of the other team (the arena).
    Fight,
    /// Team 0 goes onto the outposts A, B and C in turn and stays in each while it takes it;
    /// team 1 stays at its spawn (Outpost Rush).
    Outposts,
    /// Team 0 goes to team 1's flag, takes it with the trigger interaction and brings it to its
    /// own base, again and again; team 1 stays at its spawn (capture the flag).
    Flags,
    /// Go round these war capture points and stay in each while it is taken; none: stay at the
    /// arrival (a war's defender).
    Siege(Vec<[f32; 3]>),
    /// Hold this claim position: stand on it and fight the mobs that come within the capture
    /// radius of it (an invasion's defender).
    Defend([f32; 3]),
    /// Clear an invasion's waves around the claim position `claim`: fight the nearest mob within
    /// [`CLEAR_RADIUS_M`] of it whose vitals id (the CRC-32 of its lowercase id) is one of
    /// `vitals`, the fort's wave mobs (a game master's teleport to one out of reach on foot),
    /// else go back onto the claim. A wave starts only once the mobs of the wave before it are
    /// dead (the sim's `invasions` module), so a defender that only holds the claim keeps the
    /// first wave; the open-world mobs and the fort's own structures near it are no wave mobs
    /// (live 2026-11-23 06:52Z and 05:54Z: a clearing bot fought both).
    Clear {
        /// The claim position.
        claim: [f32; 3],
        /// The vitals ids of the fort's wave mobs.
        vitals: Vec<u32>,
    },
}

/// How far from the claim position an invasion's clearing defender goes for a mob, metres
/// (Inferred: the waves come from their lanes outside the fort's walls; live 2026-11-23
/// 04:59-05:24Z, defenders that held the claim alone kept the invasion in its first wave).
pub const CLEAR_RADIUS_M: f32 = 250.0;

/// What ends a bot's match play ([`Act::PlayMatch`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchEnd {
    /// The own participant state shows the player out of this game mode (the CRC-32 of its
    /// lowercase id): the match ended and the player is back.
    LeftMode(u32),
    /// The war or invasion ended: its result came (245).
    WarEnded,
}

/// One step of an activity.
pub struct Step {
    /// What it does, for the report.
    pub what: String,
    /// What it sends.
    pub acts: Vec<Act>,
    /// What proves it.
    pub proof: Proof,
    /// How long the proof may take.
    pub wait: Duration,
}

/// One activity: its steps, or why the bot cannot play it.
pub struct Activity {
    /// The activity.
    pub kind: ActivityKind,
    /// Its steps; `Err` names why it cannot be played.
    pub steps: Result<Vec<Step>, String>,
}

/// What the activities read: the level and the content.
pub struct ActivityData<'a> {
    /// The level's world file.
    pub world: &'a WorldFile,
    /// The content bundle's tables.
    pub tables: &'a Tables,
    /// The settlements' station slots and slices.
    pub town_stations: &'a TownStations,
    /// The war forts: their capture points.
    pub war_forts: &'a WarForts,
}

/// A step of `what` that sends `acts` and waits `wait` for `proof`.
pub(crate) fn step(what: impl Into<String>, acts: Vec<Act>, proof: Proof, wait: Duration) -> Step {
    Step {
        what: what.into(),
        acts,
        proof,
        wait,
    }
}

/// A game master line answered with [`DONE`].
pub(crate) fn gm_done(line: String) -> Step {
    step(
        line.clone(),
        vec![Act::Gm(line)],
        Proof::Line(Box::new(|answer| answer == DONE), DONE.to_owned()),
        STEP_WAIT,
    )
}

/// A `!state` line that `wanted` takes, described as `described`.
fn state_shows(what: &str, wanted: impl Fn(&str) -> bool + 'static, described: &str) -> Step {
    step(
        what,
        vec![Act::Gm("!state".to_owned())],
        Proof::Line(
            Box::new(move |line| line.starts_with(STATE_LEVEL) && wanted(line)),
            described.to_owned(),
        ),
        STEP_WAIT,
    )
}

/// A teleport to `at`.
pub(crate) const fn teleport(at: [f32; 3]) -> Act {
    Act::Teleport(at)
}

/// A facet call `call` of component `component` for the bot's entity.
fn facet(make: impl Fn(FacetHeader) -> ClientFacetMsg + 'static, component: u64) -> Act {
    Act::Call(Box::new(move |own| {
        let call = make(FacetHeader::of(component, own));
        Ok((call.type_index(), call.encode()?))
    }))
}

/// The retail placement's numbers (all 5 retail placements, camp tutorial capture): the socket
/// `0xffffffff`, the value 50 and the flag on; their meaning is Unknown.
const CAMP_SOCKET: u32 = 0xffff_ffff;
const CAMP_VALUE: u32 = 50;
const CAMP_FLAG: bool = true;
/// The rotation of a camp the bot places: none (x, y, z 0, w 1).
const CAMP_ROTATION: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// The steps of a camp the bot places at `at` and builds, as the camp tutorial
/// (`12A_MSQ_12_CampTutorial`, retail capture `12a_msq_12_camptutorial`): the game master gives
/// its `costs` (a build takes them from the bag), the placement (2236, skin `skin`) is answered
/// by 441, and the "apply all resources" interaction on the camp's card (slice asset `asset`)
/// builds it; the build takes its build time (about 7 s, live 2026-11-17), and the server ends
/// the interaction (3746) when the camp is built. A quest check before that end finds the step
/// open, and a second placement would replace the camp and start its build over.
pub fn camp_steps(
    at: [f32; 3],
    skin: u32,
    asset: [u8; ASSET_ID_LEN],
    costs: &[(String, u32)],
) -> Vec<Step> {
    let mut steps: Vec<Step> = costs
        .iter()
        .map(|(item, quantity)| gm_done(format!("!give {item} {quantity}")))
        .collect();
    steps.push(step(
        "place a camp where the bot stands",
        vec![Act::Call(Box::new(move |own| {
            let header = FacetHeader::of(component::CAMPING, own);
            // The owner is the header's halves swapped (every retail placement).
            let mut owner = [0; RAW_ID_LEN];
            let (entity, target) = owner.split_at_mut(size_of::<u64>());
            entity.copy_from_slice(&header.entity.to_be_bytes());
            target.copy_from_slice(&header.target.to_be_bytes());
            let call = TravelClientMsg::PlaceStructure {
                header,
                placement: StructurePlacement {
                    position: at,
                    rotation: CAMP_ROTATION,
                    skin,
                    // The same 16 bytes in every retail placement; their value is Unknown.
                    structure: [0; RAW_ID_LEN],
                    owner,
                    socket: CAMP_SOCKET,
                    value: CAMP_VALUE,
                    flag: CAMP_FLAG,
                },
            };
            Ok((call.type_index(), call.encode()))
        }))],
        Proof::Message(PLACEMENT_RESULTS_TYPE_INDEX),
        STEP_WAIT,
    ));
    steps.push(step(
        "build the camp",
        vec![Act::InteractCard {
            assets: vec![asset],
            kind: APPLY_ALL_RESOURCES_KIND,
        }],
        Proof::Accepted,
        LONG_WAIT,
    ));
    steps.push(step("the build ends", Vec::new(), Proof::Ended, LONG_WAIT));
    steps
}

/// A fast travel to `landmark` (3713).
fn fast_travel(landmark: LandmarkRef) -> Act {
    Act::Call(Box::new(move |own| {
        let call = TravelClientMsg::FastTravelLandmark {
            header: FacetHeader::of(component::PLAYER_HOUSING, own),
            landmark,
        };
        Ok((call.type_index(), call.encode()))
    }))
}

/// The level of a `!state` line.
fn state_level(line: &str) -> Option<u32> {
    line.strip_prefix(STATE_LEVEL)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

impl Activity {
    /// The activity of `kind` on the level and content of `data`.
    pub fn of(kind: ActivityKind, data: &ActivityData<'_>) -> Self {
        let steps = match kind {
            ActivityKind::Level => Ok(level()),
            ActivityKind::Pvp => Ok(pvp()),
            ActivityKind::PvpFoe => Ok(pvp_foe()),
            ActivityKind::Harvest => harvest(data),
            ActivityKind::Craft => craft(data),
            ActivityKind::TownCraft => town_craft(data),
            ActivityKind::Inn => inn(data),
            ActivityKind::Shrine => shrine(data),
            ActivityKind::Dungeon => dungeon(data),
            ActivityKind::Board => board(data),
            ActivityKind::Group => Err(
                "needs the other bots of a group in the world at the same time: \
                 `nw-testclient expedition` runs them"
                    .to_owned(),
            ),
            ActivityKind::Mount => mount(data),
            ActivityKind::Music => Err(
                "needs an instrument worn: the bot cannot read the bag slot of a given instrument to equip it"
                    .to_owned(),
            ),
            ActivityKind::Company => company(data),
            ActivityKind::EnemyCompany => Ok(enemy_company()),
            ActivityKind::WarSignup => Ok(war_signup()),
            ActivityKind::InvasionSignup => Ok(invasion_signup()),
            ActivityKind::Systems => Err(
                "needs its check and its bots: `nw-testclient systems` runs it".to_owned(),
            ),
            ActivityKind::Arena => queued_match_steps(data, InstanceKind::Arena),
            ActivityKind::OutpostRush => queued_match_steps(data, InstanceKind::OutpostRush),
            ActivityKind::CaptureTheFlag => {
                queued_match_steps(data, InstanceKind::CaptureTheFlag)
            }
            ActivityKind::QueueLeave => queue_leave_steps(data),
        };
        Self { kind, steps }
    }
}

fn level() -> Vec<Step> {
    vec![
        gm_done(format!("!level {SET_LEVEL}")),
        state_shows(
            "the state shows the set level",
            |line| state_level(line) == Some(SET_LEVEL),
            &format!("level {SET_LEVEL}"),
        ),
        gm_done(format!("!xp {EXPERIENCE_GIVEN}")),
        state_shows(
            "the experience raised the level",
            |line| state_level(line).is_some_and(|level| level > SET_LEVEL),
            &format!("a level above {SET_LEVEL}"),
        ),
    ]
}

/// Whether the `!state` line `line` shows the PvP flag on or on its way on.
pub fn pvp_flag_shown(line: &str) -> bool {
    line.starts_with(STATE_LEVEL)
        && (line.contains(&format!("{STATE_PVP}on"))
            || line.contains(&format!("{STATE_PVP}pending")))
}

/// A toggle of the PvP flag (2525).
fn pvp_toggle() -> Act {
    facet(|header| ClientFacetMsg::TogglePvpFlag { header }, 0)
}

fn pvp() -> Vec<Step> {
    let toggle = pvp_toggle;
    vec![
        step(
            "toggle the PvP flag on",
            vec![toggle()],
            Proof::Sent,
            Duration::ZERO,
        ),
        state_shows(
            "the flag is on its way on, or on",
            |line| {
                line.contains(&format!("{STATE_PVP}pending"))
                    || line.contains(&format!("{STATE_PVP}on"))
            },
            "pvp pending or on",
        ),
        step(
            "toggle the PvP flag off",
            vec![toggle()],
            Proof::Sent,
            Duration::ZERO,
        ),
        state_shows(
            "the flag is off",
            |line| line.contains(&format!("{STATE_PVP}off")),
            "pvp off",
        ),
    ]
}

/// A summon and ride of the horse that the mount quest unlocks, after its reward box opens.
fn mount(data: &ActivityData<'_>) -> Result<Vec<Step>, String> {
    mount_reward_box(data.tables).map(mount_steps)
}

/// The client item index of the mount quest's reward box ([`MOUNT_REWARD_BOX`]).
///
/// # Errors
///
/// Names the box when the item catalog of `tables` does not hold it.
pub fn mount_reward_box(tables: &Tables) -> Result<u32, String> {
    StarterKits::from_tables(tables)
        .item(MOUNT_REWARD_BOX)
        .map(|item| item.item_index)
        .ok_or_else(|| format!("the item catalog has no {MOUNT_REWARD_BOX}"))
}

/// The steps of a summon and ride of the horse of the done mount quest: the reward box of client
/// item index `reward_box` opens (once: the horse is owned after it), the summon key starts the
/// summon, and the client completes it once it is authorized.
pub fn mount_steps(reward_box: u32) -> Vec<Step> {
    vec![
        step(
            format!("the mount quest {MOUNT_QUEST} is done (play it with --quests first)"),
            vec![Act::Gm(format!("!queststate {MOUNT_QUEST}"))],
            Proof::Line(
                Box::new(|line| line.starts_with(QUEST_DONE)),
                format!("done {MOUNT_QUEST}"),
            ),
            STEP_WAIT,
        ),
        step(
            format!("open the reward box {MOUNT_REWARD_BOX} when the bag holds it"),
            vec![Act::OpenBagItem(reward_box)],
            Proof::Sent,
            Duration::ZERO,
        ),
        step(
            "press the summon key",
            vec![Act::SummonKey],
            Proof::Message(SUMMON_PENDING_TYPE_INDEX),
            STEP_WAIT,
        ),
        step(
            "complete the summon once it is authorized",
            vec![
                Act::Pause(SUMMON_WAIT),
                Act::Call(Box::new(|own| {
                    let call = MountClientMsg::CompleteSummon {
                        header: FacetHeader::of(MOUNT_FACET, own),
                    };
                    Ok((call.type_index(), call.encode()))
                })),
            ],
            Proof::Sent,
            Duration::ZERO,
        ),
        state_shows(
            "the state shows the bot riding",
            |line| line.contains(STATE_RIDING),
            "mount riding",
        ),
    ]
}

/// The quick courses a rider rides: their rules and their node paths.
#[derive(Debug, Clone, Default)]
pub struct CourseRoutes {
    /// The courses' rules (a course may ride another's path).
    pub rules: QuickCourseRules,
    /// The node paths of the courses' slices.
    pub paths: QuickCoursePaths,
}

impl CourseRoutes {
    /// The node places of course `course` (any case), in section order: the path of its
    /// `PathReferenceQuickCourseId`, else its own, as the sim rides it; `None` for a course with
    /// no path.
    pub fn nodes(&self, course: &str) -> Option<Vec<[f32; 3]>> {
        let path = self
            .rules
            .course(course)
            .map_or(course, |rules| rules.path_id());
        self.paths
            .get(path)
            .map(|path| path.nodes.iter().map(|node| node.position).collect())
    }
}

/// What the bot waits after a song's length for its end: the results come 1.05 s after the song
/// (the sim's music module; retail `open-world/Music`), with a margin.
const SONG_END_WAIT: Duration = Duration::from_secs(3);

/// A music call `make` of the bot's performance, on the own player's music child (the sim's
/// `music_header`; live 2026-11-22 14:34:16: a header of the facet alone was "a music call of
/// another target").
fn music(make: impl Fn(FacetHeader) -> MusicClientMsg + 'static) -> Act {
    Act::Call(Box::new(move |own| {
        let call = make(FacetHeader::of_child(MUSIC_FACET, MUSIC_CHILD, own));
        Ok((call.type_index(), call.encode()))
    }))
}

/// The steps of a performance of song `song` from sheet `sheet` (the CRC-32 of each lowercase
/// id) of `duration_s` seconds at `at`, with an instrument of the sheet worn, as retail
/// `04a_side05_02_playfortown` played it: create (4891, answered by 4869 at 113.0 s), the song
/// (4867), the sheet (4879), the start with the song's length (4865), ready (5091), the song,
/// then the post show's end (5233, 195.2 s); the bot plays no notes (the sim counts a finished
/// song whatever its score) and leaves the performance (4881).
pub fn song_steps(song: u32, sheet: u32, duration_s: f32, at: [f32; 3]) -> Vec<Step> {
    let call = |what: &str, act: Act| step(what, vec![act], Proof::Sent, Duration::ZERO);
    vec![
        step(
            "create a performance where the bot stands",
            vec![
                teleport(at),
                music(|header| MusicClientMsg::CreatePerformance { header }),
            ],
            Proof::Message(PERFORMANCE_CREATED_TYPE_INDEX),
            STEP_WAIT,
        ),
        call(
            "pick the song",
            music(move |header| MusicClientMsg::SetSong { header, song }),
        ),
        call(
            "pick the sheet",
            music(move |header| MusicClientMsg::SetSheet { header, sheet }),
        ),
        call(
            "start the song",
            music(move |header| MusicClientMsg::StartPerformance { header, duration_s }),
        ),
        step(
            "the song plays to its end",
            vec![
                music(|header| MusicClientMsg::ClientReady {
                    header,
                    ready: true,
                }),
                Act::Pause(Duration::from_secs_f32(duration_s) + SONG_END_WAIT),
            ],
            Proof::Sent,
            Duration::ZERO,
        ),
        call(
            "end the post show",
            music(|header| MusicClientMsg::EndPostShow { header }),
        ),
        call(
            "leave the performance",
            music(|header| MusicClientMsg::LeavePerformance { header }),
        ),
    ]
}

/// How long the rider stays at a course node before it rides on: the sim finds a hit at its
/// next tick, and a course starts one tick after its rider is on a mount with the course task
/// running (`world/quick_course.rs` of the sim).
const COURSE_NODE_WAIT: Duration = Duration::from_secs(1);

/// The steps of a ride of a quick course whose nodes stand at `nodes`, in section order: the
/// rider goes to each node in turn (retail `ef_tour_0-Hermit-s-Rally`: the client sends nothing
/// for a start, a hit or a finish; the server finds the hits from the rider's place). A ride to
/// the course's end (`finishes`) waits for the server's `OnQuickCourseEnded` (6065).
pub fn course_steps(nodes: &[[f32; 3]], finishes: bool) -> Vec<Step> {
    let last = nodes.len().saturating_sub(1);
    nodes
        .iter()
        .enumerate()
        .map(|(index, &at)| {
            let what = format!(
                "ride to course node {index} at {:.1} {:.1} {:.1}",
                at[0], at[1], at[2]
            );
            let acts = vec![teleport(at), Act::Pause(COURSE_NODE_WAIT)];
            if finishes && index == last {
                step(
                    what,
                    acts,
                    Proof::Message(QUICK_COURSE_ENDED_TYPE_INDEX),
                    STEP_WAIT,
                )
            } else {
                step(what, acts, Proof::Sent, Duration::ZERO)
            }
        })
        .collect()
}

/// A node of a gatherable a player gathers by hand with no rank or item.
fn harvest(data: &ActivityData<'_>) -> Result<Vec<Step>, String> {
    let gatherable = lowercase_crc32(HARVESTED_GATHERABLE);
    let (node, point) = data
        .world
        .gatherables
        .iter()
        .enumerate()
        .find(|(_, point)| point.gatherable_id == gatherable)
        .ok_or("the level has no node of the retail flint gather")?;
    Ok(gather_steps(
        HARVESTED_GATHERABLE,
        GatherNode {
            gatherable,
            at: point.position,
        },
        node,
    ))
}

/// Goes to `node` (its index in the level's gatherables, which names it to the server as a
/// client's node id does) of gatherable `name`, gathers it and waits for the gather's end.
pub fn gather_steps(name: &str, node: GatherNode, index: usize) -> Vec<Step> {
    let (target, target_entity) = node_id(index);
    gather_of(
        format!("go to a {name} node and gather it"),
        vec![teleport(node.at)],
        target,
        target_entity,
        node.gatherable,
    )
}

/// The steps of a gather of the encounter start object of raw id `object` (its card's id) as
/// gatherable `gatherable` (the CRC-32 of its lowercase id): the encounter's stage that waits
/// for its gather goes on. The bot stands at the object.
pub fn start_object_steps(object: [u8; RAW_ID_LEN], gatherable: u32) -> Vec<Step> {
    gather_of(
        "gather the encounter's start object".to_owned(),
        Vec::new(),
        object,
        nw_messages::facet_of_raw_id(&object),
        gatherable,
    )
}

/// The steps of a skin of the corpse of raw id `corpse` as gatherable `gatherable` (the
/// CRC-32 of its lowercase id): the gather request of retail's skin (beach capture `12-3`:
/// kind 14, the corpse's raw id and facet entity, gatherable `Boar`), then its end. The bot
/// goes to the corpse's place `at` first: a mob that moved in the fight dies away from the
/// bot (live 2026-11-22 10:28:46: `gather out of reach of any such node
/// gatherable=Primal_Lion`).
pub fn skin_steps(corpse: [u8; RAW_ID_LEN], gatherable: u32, at: [f32; 3]) -> Vec<Step> {
    gather_of(
        "go to the corpse and skin it".to_owned(),
        vec![teleport(at)],
        corpse,
        nw_messages::facet_of_raw_id(&corpse),
        gatherable,
    )
}

/// The steps that open the loot bags `bags` (ids from their cards, newest first) with the loot
/// interaction (kind 28), as a player takes the loot of its kills: the server gives the items of
/// its own bags and refuses the others (an empty roll, another player's bag).
pub fn loot_steps(bags: &[[u8; RAW_ID_LEN]]) -> Vec<Step> {
    bags.iter()
        .map(|bag| {
            step(
                "open a loot bag".to_owned(),
                vec![Act::Interact {
                    request: InteractionRequest {
                        target: *bag,
                        target_entity: nw_messages::facet_of_raw_id(bag),
                        interaction: 0,
                        kind: LOOT_KIND,
                        gatherable: None,
                    },
                    name: Vec::new(),
                }],
                Proof::Answered,
                STEP_WAIT,
            )
        })
        .collect()
}

/// The step of a read of the lore note of title `title` at its lore point `at`: the lore
/// interaction (kind 47) names the note by its localized title, `@` and the title (the
/// server's `read_lore`), and the server takes it within reach of the note.
pub fn lore_steps(title: &str, at: [f32; 3]) -> Vec<Step> {
    vec![step(
        format!("go to the lore note {title} and read it"),
        vec![
            teleport(at),
            Act::Interact {
                request: InteractionRequest {
                    target: [0; RAW_ID_LEN],
                    target_entity: 0,
                    interaction: 0,
                    kind: LORE_KIND,
                    gatherable: None,
                },
                name: format!("{LOCALIZED_MARK}{title}").into_bytes(),
            },
        ],
        Proof::Accepted,
        STEP_WAIT,
    )]
}

/// Starts a localized name (a lore note's title as the client names it).
const LOCALIZED_MARK: char = '@';

/// A gather of gatherable `gatherable` at the object of raw id `target` (facet entity
/// `target_entity`) after `before`, then its end and the client's acknowledgement of that end
/// (2473, 15 to 25 ms after each 3746 on retail): the server counts the object's interact tags
/// then (the crown of `12A_MSQ_24_DefeatGawain_CollectCrownItem`).
fn gather_of(
    what: String,
    mut before: Vec<Act>,
    target: [u8; RAW_ID_LEN],
    target_entity: u64,
    gatherable: u32,
) -> Vec<Step> {
    before.push(Act::Interact {
        request: InteractionRequest {
            target,
            target_entity,
            interaction: 0,
            kind: GATHER_KIND,
            gatherable: Some(gatherable),
        },
        name: Vec::new(),
    });
    vec![
        step(what, before, Proof::Accepted, STEP_WAIT),
        step("the gather ends", Vec::new(), Proof::Ended, LONG_WAIT),
        step(
            "acknowledge the end of the gather",
            vec![Act::EndInteraction { target_entity }],
            Proof::Sent,
            STEP_WAIT,
        ),
    ]
}

/// The steps of a use of the level object of facet entity `object` at `at`: go there, open it
/// with an interaction of kind `kind` and name `name`, and close it once the server took the
/// open (an end sent before it would end no interaction).
pub fn use_steps(object: u64, at: [f32; 3], kind: u8, name: &[u8]) -> Vec<Step> {
    let request = InteractionRequest {
        target: [0; RAW_ID_LEN],
        target_entity: object,
        interaction: 0,
        kind,
        gatherable: None,
    };
    vec![
        step(
            format!("go to object {object:016x} and open it"),
            vec![
                teleport(at),
                Act::Interact {
                    request,
                    name: name.to_vec(),
                },
            ],
            Proof::Accepted,
            STEP_WAIT,
        ),
        step(
            format!("close object {object:016x}"),
            vec![Act::EndInteraction {
                target_entity: object,
            }],
            Proof::Sent,
            STEP_WAIT,
        ),
    ]
}

/// A recipe of item ingredients made at a station of the level.
fn craft(data: &ActivityData<'_>) -> Result<Vec<Step>, String> {
    let recipes = Recipes::from_tables(data.tables);
    let recipe = recipes
        .making(CRAFTED_ITEM)
        .ok_or("the content has no recipe of the retail smelt")?;
    let station = data
        .world
        .stations
        .iter()
        .find(|station| recipe.made_at(&station.components.station_type))
        .ok_or("no station of the level makes the retail smelt")?;
    let mut target = [0; RAW_ID_LEN];
    target[..size_of::<u64>()].copy_from_slice(&station.id.to_le_bytes());
    craft_steps(
        &recipes,
        recipe,
        format!(
            "go to the {} station and open it",
            station.components.station_type
        ),
        vec![
            teleport(station.position),
            Act::Interact {
                request: InteractionRequest {
                    target,
                    target_entity: station.id,
                    interaction: 0,
                    kind: CRAFTING_STATION_KIND,
                    gatherable: None,
                },
                name: Vec::new(),
            },
        ],
    )
}

/// The retail smelt at a settlement station that makes it ([`nw_content::town_stations`]): the
/// bot goes to the first slot of a smelting upgrade, waits for the card of one of its slices and
/// opens it by the card's id.
fn town_craft(data: &ActivityData<'_>) -> Result<Vec<Step>, String> {
    let recipes = Recipes::from_tables(data.tables);
    let recipe = recipes
        .making(CRAFTED_ITEM)
        .ok_or("the content has no recipe of the retail smelt")?;
    let slot = data
        .town_stations
        .slots()
        .iter()
        .find(|slot| {
            data.town_stations
                .slices_of(slot.upgrade)
                .any(|slice| recipe.made_at(&slice.station_type))
        })
        .ok_or("no settlement station makes the retail smelt")?;
    let place = BundleMember::read_from(&slot.place)
        .ok()
        .and_then(|(member, _)| match member.state {
            MemberState::Position(state) => state.position,
            _ => None,
        })
        .ok_or("the settlement station's place does not read")?;
    let assets = data
        .town_stations
        .slices_of(slot.upgrade)
        .map(|slice| slice.asset)
        .collect();
    craft_steps(
        &recipes,
        recipe,
        format!(
            "go to the settlement smelter of territory {} and open it by its card",
            slot.territory
        ),
        vec![
            teleport([place.x, place.y, place.z()]),
            Act::InteractCard {
                assets,
                kind: CRAFTING_STATION_KIND,
            },
        ],
    )
}

/// The steps of a craft of `recipe` (of `recipes`): coins, the tradeskill's rank and the
/// ingredients from the game master, then the station's opening `open` (described by `what`),
/// then the craft call, proved by the progress update.
fn craft_steps(
    recipes: &Recipes,
    recipe: &nw_content::crafting::Recipe,
    what: String,
    open: Vec<Act>,
) -> Result<Vec<Step>, String> {
    if !recipe
        .ingredients
        .iter()
        .all(|ingredient| matches!(ingredient.kind, IngredientKind::Item(_)))
    {
        return Err("the retail smelt asks for an ingredient category".to_owned());
    }
    let details = recipes.details(lowercase_crc32(&recipe.id));
    let ingredients: Vec<(String, u32)> = recipe
        .ingredients
        .iter()
        .filter_map(|ingredient| match &ingredient.kind {
            IngredientKind::Item(item) => Some((item.clone(), ingredient.quantity)),
            IngredientKind::Category(_) => None,
        })
        .collect();
    let mut steps = vec![gm_done(format!("!coins {CRAFT_COINS}"))];
    if let Some(tradeskill) = &details.tradeskill {
        steps.push(gm_done(format!("!mastery {tradeskill} {CRAFT_RANK}")));
    }
    steps.extend(
        ingredients
            .iter()
            .map(|(item, quantity)| gm_done(format!("!give {item} {quantity}"))),
    );
    steps.push(step(what, open, Proof::Accepted, STEP_WAIT));
    steps.push(step(
        format!("craft {}", recipe.id),
        vec![craft_call(&recipe.id, details.fee, ingredients)],
        Proof::Message(CRAFTING_PROGRESS_TYPE_INDEX),
        STEP_WAIT,
    ));
    Ok(steps)
}

/// The craft call (2997) of one craft of recipe `recipe` for the fee `fee` the client shows,
/// with the bag items `ingredients` (item, quantity).
fn craft_call(recipe: &str, fee: u32, ingredients: Vec<(String, u32)>) -> Act {
    let recipe = lowercase_crc32(recipe);
    facet(
        move |header| ClientFacetMsg::SetCraftingDetails {
            header,
            recipe,
            quantity: 1,
            fee,
            gear_score: 0,
            perk_items: Vec::new(),
            unknown: 0,
            ingredients: ingredients
                .iter()
                .map(|(item, quantity)| CraftIngredient {
                    kind: INGREDIENT_ITEM,
                    quantity: *quantity,
                    item: lowercase_crc32(item),
                })
                .collect(),
        },
        CRAFT_COMPONENT,
    )
}

/// The component of the craft calls: the one the craft activity's calls name, which the
/// server takes.
const CRAFT_COMPONENT: u64 = 0;
/// The fee a quest's craft shows (retail cook of `12_boarfeast`: 0).
const QUEST_CRAFT_FEE: u32 = 0;

/// The steps of a quest's craft of `recipe` at the station of `GdeRef` `station` at `at`, from
/// the bag's `ingredients`, as retail cooked for `12_boarfeast_05_01`
/// (`12-3_boarfeast-Campfire-Feast` 97.0 to 106.7 s): the station opened (3743), the craft
/// (2997) proved by its progress (2993), then the craft screen closed (2999) and the
/// interaction ended (2473).
pub fn quest_craft_steps(
    recipe: &str,
    station: [u8; RAW_ID_LEN],
    at: [f32; 3],
    ingredients: Vec<(String, u32)>,
) -> Vec<Step> {
    let target_entity = nw_messages::facet_of_raw_id(&station);
    vec![
        step(
            format!("go to the station and open it for {recipe}"),
            vec![
                teleport(at),
                Act::Interact {
                    request: InteractionRequest {
                        target: station,
                        target_entity,
                        interaction: 0,
                        kind: CRAFTING_STATION_KIND,
                        gatherable: None,
                    },
                    name: Vec::new(),
                },
            ],
            Proof::Accepted,
            STEP_WAIT,
        ),
        step(
            format!("craft {recipe}"),
            vec![craft_call(recipe, QUEST_CRAFT_FEE, ingredients)],
            Proof::Message(CRAFTING_PROGRESS_TYPE_INDEX),
            STEP_WAIT,
        ),
        step(
            "close the craft screen and the station".to_owned(),
            vec![
                facet(
                    |header| ClientFacetMsg::CancelCrafting { header },
                    CRAFT_COMPONENT,
                ),
                Act::EndInteraction { target_entity },
            ],
            Proof::Sent,
            STEP_WAIT,
        ),
    ]
}

/// A settlement inn of the level: a check in, then a recall to it.
fn inn(data: &ActivityData<'_>) -> Result<Vec<Step>, String> {
    let capital = data
        .world
        .capitals
        .iter()
        .find(|capital| {
            capital
                .slice_name
                .to_ascii_lowercase()
                .contains(INNKEEPER_SLICE_MARK)
        })
        .ok_or("no capital of the level is an innkeeper")?;
    let id = capital_id(&capital.id).ok_or("the inn's capital has no entity id")?;
    let request = |kind| InteractionRequest {
        target: [0; RAW_ID_LEN],
        target_entity: id,
        interaction: 0,
        kind,
        gatherable: None,
    };
    let position = capital.position;
    Ok(vec![
        step(
            "go to an inn and open it",
            vec![
                teleport(position),
                Act::Interact {
                    request: request(OPEN_INN_KIND),
                    name: Vec::new(),
                },
            ],
            Proof::Accepted,
            STEP_WAIT,
        ),
        step(
            "check in",
            vec![Act::Interact {
                request: request(SET_SPAWN_POINT_KIND),
                name: Vec::new(),
            }],
            Proof::Accepted,
            STEP_WAIT,
        ),
        state_shows(
            "the state names the inn",
            move |line| line.contains(&format!("inn {id:016x}")),
            &format!("inn {id:016x}"),
        ),
        step(
            "recall to the inn",
            vec![fast_travel(LandmarkRef {
                territory: 0,
                landmark_type: INN_RECALL_LANDMARK,
                position: [position[0], position[1], 0.0],
            })],
            Proof::Message(TELEPORT_WITH_LOADING_SCREEN_TYPE_INDEX),
            LONG_WAIT,
        ),
    ])
}

/// A fast travel shrine of the level.
fn shrine(data: &ActivityData<'_>) -> Result<Vec<Step>, String> {
    let tags = PoiTags::from_tables(data.tables);
    let shrines = tags.territories_with(FAST_TRAVEL_TAG);
    let (landmark, territory) = data
        .world
        .landmarks
        .iter()
        .find_map(|landmark| {
            landmark
                .territory_ids
                .iter()
                .find(|territory| shrines.contains(&u32::from(**territory)))
                .map(|territory| (landmark, *territory))
        })
        .ok_or("the level has no landmark in a fast travel territory")?;
    let (landmark_type, position) = (landmark.landmark_type, landmark.position);
    Ok(vec![
        step(
            "enter the shrine's territory (its discovery)",
            vec![facet(
                move |header| ClientFacetMsg::TerritoryEntered {
                    header,
                    area: [0; RAW_ID_LEN],
                    area_entity: 0,
                    extra: 0,
                    territory,
                    unknown: 0,
                },
                0,
            )],
            Proof::Sent,
            Duration::ZERO,
        ),
        gm_done(format!("!masteryxp {AZOTH} {AZOTH_GIVEN}")),
        step(
            format!("fast travel to the shrine of territory {territory}"),
            vec![fast_travel(LandmarkRef {
                territory,
                landmark_type,
                position: [position[0], position[1], 0.0],
            })],
            Proof::Message(TELEPORT_WITH_LOADING_SCREEN_TYPE_INDEX),
            LONG_WAIT,
        ),
    ])
}

/// An expedition that takes a remote entry.
fn dungeon(data: &ActivityData<'_>) -> Result<Vec<Step>, String> {
    let catalog = InstanceCatalog::from_tables(data.tables);
    let mode = catalog
        .modes()
        .find(|mode| mode.kind == InstanceKind::Expedition && mode.remote_entry)
        .ok_or("no expedition takes a remote entry")?;
    let game_mode = mode.crc;
    let mut steps = Vec::new();
    if let Some(level) = mode.required_level {
        steps.push(gm_done(format!("!level {level}")));
    }
    steps.push(ask_remote_entry(
        format!("ask to enter expedition {}", mode.id),
        game_mode,
    ));
    Ok(steps)
}

/// Asks whether the bot may use game mode `game_mode` (the CRC-32 of its lowercase id) from
/// afar (5039); the 5038 answer proves it.
fn ask_remote_entry(what: String, game_mode: u32) -> Step {
    step(
        what,
        vec![arena_call(move |header| {
            ArenaClientMsg::RequestRemoteInteract { header, game_mode }
        })],
        Proof::ArenaYes(REMOTE_INTERACT_RESULT_TYPE_INDEX),
        STEP_WAIT,
    )
}

/// Whether the arena answer `body` of type `type_index` says yes: a 5038 that allows the game
/// mode, a 6776 with `Success`. `Err` names the answer that says no, or why it does not read.
///
/// # Errors
///
/// Returns the refusal or the decode error, described.
pub fn arena_verdict(type_index: u64, body: &[u8]) -> Result<(), String> {
    match ArenaServerMsg::decode(type_index, body) {
        Ok(
            ArenaServerMsg::RemoteInteractResult { allowed: true, .. }
            | ArenaServerMsg::ActivateResult {
                response: ActivateResponse::Success,
                ..
            },
        ) => Ok(()),
        Ok(ArenaServerMsg::RemoteInteractResult { .. }) => {
            Err("the game mode is not allowed".to_owned())
        }
        Ok(ArenaServerMsg::ActivateResult { response, .. }) => {
            Err(format!("the activation answered {response:?}"))
        }
        Err(error) => Err(format!("the arena answer does not read: {error}")),
    }
}

/// The arena call `call` makes with the bot's arena facet header.
fn arena_call(call: impl Fn(FacetHeader) -> ArenaClientMsg + 'static) -> Act {
    Act::Call(Box::new(move |own| {
        let call = call(FacetHeader::of(PLAYER_ARENA_COMPONENT, own));
        Ok((call.type_index(), call.encode()))
    }))
}

/// The one byte of a 5037 and a 926 (0 in every capture).
const ARENA_CALL_FLAG: u8 = 0;

/// Enters an instance of game mode `game_mode` (the CRC-32 of its lowercase id) from afar, as
/// the client does (run 15 instances study): 5039, 5037 (the 6776 answer), then 946, after
/// which the handoff to the instance sim sends the instance level (1635).
pub fn trial_steps(mode: &str, game_mode: u32) -> Vec<Step> {
    vec![
        ask_remote_entry(format!("ask to enter game mode {mode}"), game_mode),
        step(
            format!("activate game mode {mode}"),
            vec![arena_call(|header| {
                ArenaClientMsg::RequestRemoteEnterDungeon {
                    header,
                    flag: ARENA_CALL_FLAG,
                }
            })],
            Proof::ArenaYes(DUNGEON_ACTIVATE_RESULT_TYPE_INDEX),
            LONG_WAIT,
        ),
        teleport_into(mode),
    ]
}

/// Enters an instance of game mode `mode` (`game_mode`, the CRC-32 of its lowercase id) at its
/// entrance capital `door`, as a player at the door does: it goes to the door's place `at` (when
/// known), opens the entrance's menu (interaction 71, the 5038 answer), enters (72, the 6776
/// answer), then 946 ([`teleport_into`]).
pub fn door_trial_steps(mode: &str, game_mode: u32, door: u64, at: Option<[f32; 3]>) -> Vec<Step> {
    let request = |kind| Act::Interact {
        request: InteractionRequest {
            target: [0; RAW_ID_LEN],
            target_entity: door,
            interaction: 0,
            kind,
            gatherable: None,
        },
        name: SOLO_TRIAL_ENTRY_NAME.to_vec(),
    };
    let mut open = Vec::from_iter(at.map(teleport));
    open.push(request(OPEN_DUNGEON_MENU_KIND));
    vec![
        step(
            format!("open the entrance {door:016x} of game mode {mode} ({game_mode:08x})"),
            open,
            Proof::ArenaYes(REMOTE_INTERACT_RESULT_TYPE_INDEX),
            LONG_WAIT,
        ),
        step(
            format!("enter game mode {mode} at its entrance"),
            vec![request(ENTER_DUNGEON_KIND)],
            Proof::ArenaYes(DUNGEON_ACTIVATE_RESULT_TYPE_INDEX),
            LONG_WAIT,
        ),
        teleport_into(mode),
    ]
}

/// The name of a solo trial entrance's interactions, its leading space included (retail
/// `06A_MSQ_21_DefeatBoss` and `99A_MSQ_15_Ritual`: 71 then 72 at the trial door).
const SOLO_TRIAL_ENTRY_NAME: &[u8] = b" @arena_solo_enter";

/// The 946 into the instance the server granted for game mode `mode`, which the handoff to the
/// instance sim answers with the instance level (1635).
fn teleport_into(mode: &str) -> Step {
    step(
        format!("teleport into game mode {mode}"),
        vec![arena_call(|header| ArenaClientMsg::TeleportIntoDungeon {
            header,
        })],
        Proof::Message(LEVEL_INFO_CHANGED_MSG_TYPE_INDEX),
        LONG_WAIT,
    )
}

/// Leaves the instance the bot is in (926), after which the handoff back to the open world
/// sends the open world level (1635).
pub fn leave_trial_steps() -> Vec<Step> {
    vec![step(
        "leave the instance".to_owned(),
        vec![arena_call(|header| ArenaClientMsg::ForfeitArena {
            header,
            flag: ARENA_CALL_FLAG,
        })],
        Proof::Message(LEVEL_INFO_CHANGED_MSG_TYPE_INDEX),
        LONG_WAIT,
    )]
}

/// The made-up company of the company activity.
const BOT_COMPANY: &str = "Bot Test Company";
/// The faction the company activity's bot joins (any faction makes a company).
pub(crate) const BOT_FACTION: &str = "syndicate";
/// Monarch's Bluffs, the town of the company activity.
const MONARCHS_BLUFFS: u16 = 12;
/// The Monarch's Bluffs town project board (retail `SQ-Monarchs-Bluffs`, as its card names it):
/// the company activity uses the war board nearest it.
const MONARCHS_BLUFFS_TOWN_BOARD: [u8; 8] = [0x17, 0x28, 0xe7, 0xba, 0x8e, 0xf6, 0x90, 0x95];

/// A game master's company owns Monarch's Bluffs and the bot opens the town's war board
/// ([`ActivityKind::Company`]).
fn company(data: &ActivityData<'_>) -> Result<Vec<Step>, String> {
    let boards = |slice| sim::player_content::sign_boards(data.world, slice);
    let town_board = boards(sim::player_content::TOWN_BOARD_SLICE)
        .into_iter()
        .find(|board| board.id == u64::from_le_bytes(MONARCHS_BLUFFS_TOWN_BOARD))
        .ok_or("the level has no Monarch's Bluffs town board")?;
    let war_board = boards(sim::player_content::WAR_BOARD_SLICE)
        .into_iter()
        .min_by(|a, b| {
            let distance = |board: &sim::player_content::SignBoard| {
                board.position.horizontal_distance(town_board.position)
            };
            distance(a).total_cmp(&distance(b))
        })
        .ok_or("the level has no war board")?;
    let mut target = [0; RAW_ID_LEN];
    target[..size_of::<u64>()].copy_from_slice(&war_board.id.to_le_bytes());
    let at = war_board.position;
    let mut steps = faction_and_company(BOT_FACTION, BOT_COMPANY);
    steps.extend([
        gm_done(format!("!owner {MONARCHS_BLUFFS} {BOT_COMPANY}")),
        step(
            "go to the Monarch's Bluffs war board and use it",
            vec![
                teleport([at.x, at.y, at.z]),
                facet(
                    |header| crate::play::entered(header, MONARCHS_BLUFFS),
                    crate::play::CALL_COMPONENT,
                ),
                Act::Interact {
                    request: InteractionRequest {
                        target,
                        target_entity: nw_messages::facet_of_raw_id(&target),
                        interaction: 0,
                        kind: WAR_BOARD_KIND,
                        gatherable: None,
                    },
                    name: WAR_BOARD_NAME.to_vec(),
                },
            ],
            Proof::Accepted,
            STEP_WAIT,
        ),
        gm_done(format!("!owner {MONARCHS_BLUFFS} clear")),
    ]);
    Ok(steps)
}

/// The made-up company of the enemy company activity.
const ENEMY_COMPANY: &str = "Bot War Company";
/// The faction of the enemy company: not the faction of the company activity, so a war of the
/// two has a side for each.
pub(crate) const ENEMY_FACTION: &str = "covenant";

/// The territory of the war sign-up activity: Reekwater, an invasion and war fort.
const WAR_TERRITORY: u16 = 5;
/// Minutes before the war sign-up activity's war starts: sign-ups close at the start.
const WAR_START_MINUTES: u32 = 10;
/// The sign-up list page a client's war board screen asks for first (2767).
const FIRST_LIST_PAGE: u32 = 1;
/// How long the war sign-up activity waits for the client's next roster and list poll (retail
/// `army-management`: the screen asks every 5 s).
const ROSTER_POLL_WAIT: Duration = Duration::from_secs(5);

/// A war sign-up call of the bot ([`TERRITORY_INTERACTOR_COMPONENT`]).
pub(crate) fn war_call(make: impl Fn(FacetHeader) -> WarSignupClientMsg + 'static) -> Act {
    Act::Call(Box::new(move |own| {
        let call = make(FacetHeader::of(TERRITORY_INTERACTOR_COMPONENT, own));
        Ok((call.type_index(), call.encode()?))
    }))
}

/// [`ActivityKind::WarSignup`]: the calls of a client's war board screen (retail
/// `army-management`: 2791, 2722, then 2772 and 2767 every 5 s; a sign-up 2762 is answered
/// with 2169 and 2722), and the war leader's Fill Roster (2777, answered with 2733).
fn war_signup() -> Vec<Step> {
    let cancel = format!("!war {WAR_TERRITORY} cancel");
    let mut steps = vec![
        gm_any(cancel.clone()),
        gm_done(format!(
            "!war {WAR_TERRITORY} {WAR_START_MINUTES} {ENEMY_COMPANY}"
        )),
    ];
    steps.extend(war_board_signup(
        WAR_TERRITORY,
        SIDE_ATTACKER,
        "sign up as the attacker (2762)",
    ));
    steps.extend([
        step(
            "ask for the roster and the first page of its side's sign-up list (2772, 2767)",
            vec![
                Act::Pause(ROSTER_POLL_WAIT),
                war_call(|header| WarSignupClientMsg::RequestRoster { header }),
                war_call(|header| WarSignupClientMsg::RequestSignupList {
                    header,
                    page: FIRST_LIST_PAGE,
                }),
            ],
            Proof::Message(SIGNUP_LIST_RECEIVED_TYPE_INDEX),
            STEP_WAIT,
        ),
        step(
            "fill the roster as the war leader (2777)",
            vec![war_call(|header| WarSignupClientMsg::FillRoster { header })],
            Proof::Message(SET_ROSTER_RESPONSE_RECEIVED_TYPE_INDEX),
            STEP_WAIT,
        ),
        gm_done(cancel),
    ]);
    steps
}

/// The territory of the invasion sign-up activity: an invasion fort of the content (Observed:
/// the `invasion` ledger's), not the war sign-up's [`WAR_TERRITORY`], whose owner that activity
/// needs.
const INVASION_TERRITORY: u16 = 16;
/// Minutes before the invasion sign-up activity's invasion starts.
const INVASION_START_MINUTES: u32 = 10;

/// [`ActivityKind::InvasionSignup`]: the bot's company owns the territory, its invasion is
/// scheduled, and the bot signs up from afar as a war board screen does (2791 answered with
/// 2722, a sign-up 2762 of the defender side with 2169).
fn invasion_signup() -> Vec<Step> {
    let cancel = format!("!war {INVASION_TERRITORY} cancel");
    let mut steps = faction_and_company(BOT_FACTION, BOT_COMPANY);
    steps.extend([
        gm_any(cancel.clone()),
        gm_done(format!("!owner {INVASION_TERRITORY} {BOT_COMPANY}")),
        gm_done(format!(
            "!invasion {INVASION_TERRITORY} {INVASION_START_MINUTES}"
        )),
    ]);
    steps.extend(war_board_signup(
        INVASION_TERRITORY,
        SIDE_DEFENDER,
        "sign up as a defender of the invasion (2762)",
    ));
    steps.extend([
        gm_done(cancel),
        gm_done(format!("!owner {INVASION_TERRITORY} clear")),
    ]);
    steps
}

/// The bot joins [`ENEMY_FACTION`] and makes [`ENEMY_COMPANY`] ([`ActivityKind::EnemyCompany`]).
fn enemy_company() -> Vec<Step> {
    faction_and_company(ENEMY_FACTION, ENEMY_COMPANY)
}

/// The bot joins `faction` (or is in it already).
pub(crate) fn faction_step(faction: &str) -> Step {
    step(
        format!("!faction {faction}"),
        vec![Act::Gm(format!("!faction {faction}"))],
        Proof::Line(
            Box::new(|line| line == DONE || line == sim::world::FACTION_UNCHANGED),
            "a faction".to_owned(),
        ),
        STEP_WAIT,
    )
}

/// The faction of the killer bot of a PvP quest step, and of its foe bot: two factions, so the
/// two flagged players are foes (the sim's `pvp_foes`: flagged players of two factions).
pub const PVP_KILLER_FACTION: &str = "syndicate";
const PVP_FOE_FACTION: &str = "marauder";
/// How long a toggled PvP flag takes to come on outside a sanctuary (the sim's
/// `PVP_FLAG_DELAY_MS`, 30 s, retail `Hive_flag_pvp_on_and_off`), with a margin.
const PVP_FLAG_ON_WAIT: Duration = Duration::from_secs(32);
/// How long the foe bot stands for its killer, and how often it gets up after a death (a game
/// master's `!heal` raises a fallen player, and refills a living one: live 2026-11-22 14:51, a
/// heal every 5 s undid the killer bot's hits of about 380 points twice a second, so the gap
/// is longer than the killer takes for a kill).
const FOE_STAND: Duration = Duration::from_secs(600);
const FOE_RISE_GAP: Duration = Duration::from_secs(40);

/// The steps that put the bot's PvP flag on in `faction`: the faction, the state, the toggle
/// (2525) when the state does not show the flag on, the flag's countdown, and the state that
/// shows it on.
pub fn pvp_flag_steps(faction: &str) -> Vec<Step> {
    vec![
        faction_step(faction),
        state_shows("the state before the toggle", |_| true, "a state line"),
        step(
            "toggle the PvP flag on unless it is on",
            vec![Act::PvpFlagOn],
            Proof::Sent,
            Duration::ZERO,
        ),
        step(
            "wait for the flag's countdown",
            vec![Act::Pause(PVP_FLAG_ON_WAIT)],
            Proof::Sent,
            Duration::ZERO,
        ),
        state_shows(
            "the flag is on",
            |line| line.contains(&format!("{STATE_PVP}on")),
            "pvp on",
        ),
    ]
}

/// The foe of a PvP quest step ([`ActivityKind::PvpFoe`]).
fn pvp_foe() -> Vec<Step> {
    let mut steps = vec![gm_done("!god off".to_owned())];
    steps.extend(pvp_flag_steps(PVP_FOE_FACTION));
    let rises = FOE_STAND.as_secs() / FOE_RISE_GAP.as_secs();
    steps.extend((0..rises).map(|_| {
        step(
            "stand up after a death",
            vec![Act::Gm("!heal".to_owned()), Act::Pause(FOE_RISE_GAP)],
            Proof::Sent,
            Duration::ZERO,
        )
    }));
    steps
}

/// The bot joins `faction` and makes the company `company` (or is in it already, or the name
/// is taken).
fn faction_and_company(faction: &str, company: &str) -> Vec<Step> {
    let made = sim::world::company_made(company);
    vec![
        faction_step(faction),
        step(
            format!("!company {company}"),
            vec![Act::Gm(format!("!company {company}"))],
            Proof::Line(
                Box::new(move |line| {
                    line == made
                        || line == sim::world::COMPANY_IN_COMPANY
                        || line == sim::world::COMPANY_NAME_TAKEN
                }),
                "the company".to_owned(),
            ),
            STEP_WAIT,
        ),
    ]
}

/// A town board mission of the content.
/// The step that opens the town board, as the board activity's first step does.
fn open_town_board() -> Step {
    step(
        "open the town board",
        vec![Act::Interact {
            request: InteractionRequest {
                target: [0; RAW_ID_LEN],
                target_entity: 0,
                interaction: 0,
                kind: TOWN_BOARD_KIND,
                gatherable: None,
            },
            name: TOWN_BOARD_NAME.to_vec(),
        }],
        Proof::Accepted,
        STEP_WAIT,
    )
}

/// The steps of a town board mission's turn-in (its `ReturnToGiver` task): the town board
/// opens, and the client turns in the mission of instance `instance` there (2461,
/// `sim::world::missions`).
pub fn mission_turn_in_steps(instance: u64) -> Vec<Step> {
    vec![
        open_town_board(),
        step(
            "turn in the mission",
            vec![Act::Call(Box::new(move |own| {
                let call = ObjectiveInteractorClientMsg::CompleteObjective {
                    header: FacetHeader::of(0, own),
                    instance,
                };
                Ok((call.type_index(), call.encode()))
            }))],
            Proof::Sent,
            Duration::ZERO,
        ),
    ]
}

fn board(data: &ActivityData<'_>) -> Result<Vec<Step>, String> {
    let row = data
        .tables
        .mission_data
        .iter()
        .flat_map(|table| &table.rows)
        .find(|row| {
            row.enabled != Some(false)
                && !row.mission_id.is_empty()
                && row
                    .territory_project_id
                    .as_deref()
                    .is_some_and(|id| !id.is_empty())
        })
        .ok_or("the content has no town board mission")?;
    let mission = row.mission_id.clone();
    let mission_crc = lowercase_crc32(&mission);
    let project = lowercase_crc32(row.territory_project_id.as_deref().unwrap_or_default());
    let shown = mission.clone();
    Ok(vec![
        open_town_board(),
        step(
            format!("take mission {mission}"),
            vec![Act::Call(Box::new(move |own| {
                let call = ObjectiveInteractorClientMsg::SelectCommunityGoal {
                    header: FacetHeader::of(0, own),
                    mission: mission_crc,
                    project,
                };
                Ok((call.type_index(), call.encode()))
            }))],
            Proof::Sent,
            Duration::ZERO,
        ),
        step(
            "the mission runs",
            vec![Act::Gm(format!("!queststate {shown}"))],
            Proof::Line(
                Box::new(|line| line.starts_with("running ")),
                "running".to_owned(),
            ),
            STEP_WAIT,
        ),
    ])
}

/// The first role pick of a queue join (Observed: 1 at once in every retail join).
const FIRST_ROLE: u8 = 1;
/// The role a bot picks: not the healer role 4 (the director's `HEALER_ROLE`), as a team takes
/// at most one healer and a queue of healers forms no match (Inferred: retail picked 3 or 4).
const DAMAGE_ROLE: u8 = 3;
/// The `extra` value and flag of a solo queue join (Observed: `01 3316 0001 00` solo).
const SOLO_JOIN_EXTRA: u16 = 1;
const SOLO_JOIN_FLAG: u8 = 0;
/// The last byte of a 5741 (Observed: 1 in retail).
const ACCEPT_FLAG: u8 = 1;
/// The longest wait for the queue to find a match: the other bots join over some minutes.
const QUEUE_WAIT: Duration = Duration::from_secs(600);
/// The longest wait from the accept to the move (5449): an arena match moves once every
/// player accepted, an Outpost Rush or capture the flag match once its instance opened.
const ACCEPT_WAIT: Duration = Duration::from_secs(180);
/// The longest wait from the 5450 to the arrival in the match (the handoff to the instance sim).
const ARRIVAL_WAIT: Duration = Duration::from_secs(90);
/// The longest wait from the end of the match play to the move back into the open world.
const RETURN_WAIT: Duration = Duration::from_secs(120);
/// The longest arena match (Inferred: a margin above its rounds and its 60 s cleanup).
const ARENA_LONGEST: Duration = Duration::from_secs(20 * SECONDS_PER_MINUTE);
/// The longest Outpost Rush match: 90 s pregame, `TimeLimit` 2,700 s and 180 s cleanup (the
/// sim's `outpost_rush` module text), and a minute more.
const OUTPOST_RUSH_LONGEST: Duration = Duration::from_secs(90 + 2_700 + 180 + SECONDS_PER_MINUTE);
/// The longest capture the flag match: 180 s pregame, 600 s play, 300 s overtime and 60 s
/// cleanup (the sim's `capture_the_flag` module text), and a minute more.
const CAPTURE_THE_FLAG_LONGEST: Duration =
    Duration::from_secs(180 + 600 + 300 + 60 + SECONDS_PER_MINUTE);
/// Seconds per minute.
const SECONDS_PER_MINUTE: u64 = 60;

/// The participant call `make` with the bot's game mode participant header.
fn participant_call(make: impl Fn(FacetHeader) -> GameModeClientMsg + 'static) -> Act {
    Act::Call(Box::new(move |own| {
        let call = make(FacetHeader::of(facet_component::GAME_MODE_PARTICIPANT, own));
        Ok((call.type_index(), call.encode()?))
    }))
}

/// A proof that the own game mode participant state shows `wanted`, described.
fn matchmaking_shows(wanted: impl Fn(&Matchmaking) -> bool + 'static, described: &str) -> Proof {
    Proof::Check(
        Box::new(move |sight| wanted(sight.matchmaking).then_some(Ok(()))),
        described.to_owned(),
    )
}

/// The accept of the match `match_id` of a pop by the bot `own`: 5741 with the match id and
/// 946, sent together as retail sends them (the sim's `queue` module text, point 5).
///
/// # Errors
///
/// Returns [`MessageError`] when the 5741 does not write.
pub fn accept_calls(own: u64, match_id: &str) -> Result<Vec<(u64, Vec<u8>)>, MessageError> {
    let accept = GameModeClientMsg::AcceptMatch {
        header: FacetHeader::of(facet_component::GAME_MODE_PARTICIPANT, own),
        match_id: match_id.to_owned(),
        flag: ACCEPT_FLAG,
    };
    let teleport = ArenaClientMsg::TeleportIntoDungeon {
        header: FacetHeader::of(PLAYER_ARENA_COMPONENT, own),
    };
    Ok(vec![
        (accept.type_index(), accept.encode()?),
        (teleport.type_index(), teleport.encode()),
    ])
}

/// The match id of a pop the bot has not accepted yet: the own participant state's newest, when
/// it is not `accepted`. A match that did not open pops again with a new id, and a client
/// accepts each pop it shows (live 2026-11-23 04:51:56-04:52:39Z: 45 pops while no capture the
/// flag sim was free; the bots that kept their first accept never moved).
pub fn pop_to_accept(matchmaking: &Matchmaking, accepted: Option<&str>) -> Option<String> {
    matchmaking
        .match_id
        .clone()
        .filter(|match_id| Some(match_id.as_str()) != accepted)
}

/// The join of the queue of game mode `id` (its 6254 `activity`) alone, answered with 6295.
fn join_queue(id: &str, activity: u16) -> Step {
    step(
        format!("join the {id} queue alone (6254)"),
        vec![participant_call(move |header| {
            GameModeClientMsg::JoinQueueForActivities {
                header,
                activities: vec![QueueActivity {
                    activity,
                    extra: SOLO_JOIN_EXTRA,
                    flag: SOLO_JOIN_FLAG,
                }],
            }
        })],
        Proof::Message(START_MATCHMAKING_ROLE_SELECTION_TYPE_INDEX),
        STEP_WAIT,
    )
}

/// The two role picks of a join (6272): the bot is in the queue after the second.
fn pick_roles() -> Step {
    step(
        "pick the roles (6272 twice)",
        [FIRST_ROLE, DAMAGE_ROLE]
            .into_iter()
            .map(|role| {
                participant_call(move |header| GameModeClientMsg::SetMatchmakingRole {
                    header,
                    role,
                })
            })
            .collect(),
        Proof::Sent,
        Duration::ZERO,
    )
}

/// [`ActivityKind::QueueLeave`]: the bot joins the 3v3 arena queue alone and closes the role
/// picking screen (6297, answered with 6296), joins again, picks its roles and leaves the queue
/// (1066, answered with 4970), as the sim's `queue` module text has it (point 7). A bot alone
/// in the queue finds no match (a match takes `[instances.arena] match_size` players), so no
/// instance opens.
fn queue_leave_steps(data: &ActivityData<'_>) -> Result<Vec<Step>, String> {
    let catalog = InstanceCatalog::from_tables(data.tables);
    let mode = catalog
        .modes()
        .find(|mode| mode.kind == InstanceKind::Arena)
        .ok_or("no arena game mode")?;
    let (id, crc) = (mode.id.clone(), mode.crc);
    Ok(vec![
        join_queue(&id, ARENA_ACTIVITY),
        step(
            "close the role picking screen (6297)",
            vec![participant_call(|header| {
                GameModeClientMsg::CancelRoleSelection { header }
            })],
            Proof::Message(ROLE_SELECTION_CANCELED_TYPE_INDEX),
            STEP_WAIT,
        ),
        join_queue(&id, ARENA_ACTIVITY),
        pick_roles(),
        step(
            format!("leave the {id} queue (1066)"),
            vec![participant_call(move |header| {
                GameModeClientMsg::LeaveQueueForGameMode {
                    header,
                    game_mode: crc,
                    group: false,
                }
            })],
            Proof::Message(LEFT_GAME_MODE_QUEUE_TYPE_INDEX),
            STEP_WAIT,
        ),
    ])
}

/// The steps of a queued PvP game mode of `kind` as the client plays it (the sim's `queue`
/// module text): 6211 (subscribe), 6254 answered with 6295, the two 6272, the pop (the match id
/// in the own participant state), 5741 with 946 answered with 5449, 5450 and the move into the
/// match, the match, and the move back.
fn queued_match_steps(data: &ActivityData<'_>, kind: InstanceKind) -> Result<Vec<Step>, String> {
    let catalog = InstanceCatalog::from_tables(data.tables);
    let mode = catalog
        .modes()
        .find(|mode| mode.kind == kind)
        .ok_or_else(|| format!("no game mode of kind {kind:?}"))?;
    let (activity, goal, longest, god) = match kind {
        InstanceKind::Arena => (ARENA_ACTIVITY, MatchGoal::Fight, ARENA_LONGEST, false),
        InstanceKind::OutpostRush => (
            OUTPOST_RUSH_ACTIVITY,
            MatchGoal::Outposts,
            OUTPOST_RUSH_LONGEST,
            true,
        ),
        InstanceKind::CaptureTheFlag => (
            CAPTURE_THE_FLAG_ACTIVITY,
            MatchGoal::Flags,
            CAPTURE_THE_FLAG_LONGEST,
            true,
        ),
        _ => return Err(format!("{kind:?} is not a queued PvP game mode")),
    };
    let crc = mode.crc;
    let id = mode.id.clone();
    let described = format!("{goal:?}");
    let mut steps = vec![
        step(
            "open the queue screen (6211)",
            vec![participant_call(|header| {
                GameModeClientMsg::ModifyGroupEligibilitySubscription {
                    header,
                    subscribed: true,
                }
            })],
            Proof::Sent,
            Duration::ZERO,
        ),
        join_queue(&id, activity),
        pick_roles(),
        step(
            "the queue finds a match",
            Vec::new(),
            matchmaking_shows(
                |matchmaking| matchmaking.match_id.is_some(),
                "a match id in the own participant state",
            ),
            QUEUE_WAIT,
        ),
        step(
            "accept the match (5741, 946)",
            vec![Act::AcceptPops {
                longest: ACCEPT_WAIT,
            }],
            Proof::Message(MATCHMAKING_LOGIN_REQUESTED_TYPE_INDEX),
            ACCEPT_WAIT,
        ),
        step(
            format!("sign in to the match (5450) and move into {id}"),
            vec![participant_call(|header| {
                GameModeClientMsg::MatchmakingLoginConfirmed { header }
            })],
            matchmaking_shows(
                move |matchmaking| matchmaking.active.contains(&crc),
                "the game mode in the own participant state",
            ),
            ARRIVAL_WAIT,
        ),
    ];
    // God mode holds on the sim it was turned on in: the match's sim starts without it.
    if god {
        steps.push(gm_done("!god on".to_owned()));
    }
    steps.push(step(
        format!("play the {id} match ({described}) to its end and come back"),
        vec![Act::PlayMatch {
            end: MatchEnd::LeftMode(crc),
            goal,
            longest,
        }],
        matchmaking_shows(
            move |matchmaking| !matchmaking.active.contains(&crc),
            "the game mode gone from the own participant state",
        ),
        RETURN_WAIT,
    ));
    Ok(steps)
}

/// The territory of the war fight activities: a war fort whose capture points the captures
/// show (Observed: `everfall_war`, `everfall_war_2`; the fort table's territory 4), so its war
/// gets an instance.
pub(crate) const WAR_FIGHT_TERRITORY: u16 = 4;
/// Minutes from the declaration to the war's start: the sign-ups, the roster and the move into
/// the instance (which opens at once, 15 minutes before a start) come first.
pub(crate) const WAR_FIGHT_START_MINUTES: u32 = 6;
/// The territory of the invasion fight: the invasion fort of the `invasion` ledger.
pub const INVASION_FIGHT_TERRITORY: u16 = INVASION_TERRITORY;
/// Minutes from the invasion's schedule to its start (the sign-ups and the move come first).
pub(crate) const INVASION_FIGHT_START_MINUTES: u32 = 4;
/// How long a bot waits after its sign-up before it fills the roster: the other bots of its side
/// sign up meanwhile, and only the war leader's fill counts (the others are refused).
pub(crate) const FILL_WAIT: Duration = Duration::from_secs(60);
/// The longest wait for the raid invite and the move into the instance.
const INVITE_WAIT: Duration = Duration::from_secs(10 * SECONDS_PER_MINUTE);
/// The longest war: its start and its 30 minutes, and a margin.
pub(crate) const WAR_LONGEST: Duration = Duration::from_secs(45 * SECONDS_PER_MINUTE);
/// The longest invasion: its start and its 1,500 s, and a margin.
pub(crate) const INVASION_LONGEST: Duration = Duration::from_secs(40 * SECONDS_PER_MINUTE);

/// A step whose game master line `line` takes any answer: a repeat by another bot of the side
/// (a war or invasion that is there already) answers otherwise, and the step goes on.
pub(crate) fn gm_any(line: String) -> Step {
    step(
        line.clone(),
        vec![Act::Gm(line)],
        Proof::Line(Box::new(|_| true), "any answer".to_owned()),
        STEP_WAIT,
    )
}

/// The war board use from afar (2791, answered with 2722) and the sign-up `what` for `side`
/// of `territory` (2762, answered with 2169), as a client's war board screen does.
pub(crate) fn war_board_signup(territory: u16, side: u32, what: &str) -> [Step; 2] {
    [
        step(
            "use the war board from afar (2791)",
            vec![war_call(move |header| {
                WarSignupClientMsg::RequestRemoteInteract { header, territory }
            })],
            Proof::Message(TERRITORY_FACET_READY_TYPE_INDEX),
            STEP_WAIT,
        ),
        step(
            what,
            vec![war_call(move |header| WarSignupClientMsg::RequestSignup {
                header,
                side,
                territory,
            })],
            Proof::Message(SIGNUP_RESPONSE_RECEIVED_TYPE_INDEX),
            STEP_WAIT,
        ),
    ]
}

/// The accept (3495) of the raid invite in the own groups state; the move into the war's
/// instance (its 1635) proves it.
pub(crate) fn accept_raid_invite() -> Step {
    step(
        "accept the raid invite (3495) and move into the instance",
        vec![Act::Dynamic(Box::new(|sight| {
            let invite = sight.group_invite?;
            Some(accept_call(invite, sight.own).map(|call| vec![call]))
        }))],
        Proof::Message(LEVEL_INFO_CHANGED_MSG_TYPE_INDEX),
        INVITE_WAIT,
    )
}

/// The play of a war or invasion for `goal` until its result (245), then the leave of its raid
/// (2252) and the move back out of the instance (its 1635).
pub(crate) fn war_play_steps(goal: MatchGoal, longest: Duration) -> Vec<Step> {
    let described = format!("{goal:?}");
    vec![
        gm_done("!god on".to_owned()),
        step(
            format!("play the war ({described}) until its result (245)"),
            vec![Act::PlayMatch {
                end: MatchEnd::WarEnded,
                goal,
                longest,
            }],
            Proof::Message(SIEGE_WARFARE_ENDED_TYPE_INDEX),
            STEP_WAIT,
        ),
        step(
            "leave the raid (2252) and move back",
            vec![Act::Call(Box::new(|own| {
                let call = GroupCall::Leave { value: LEAVE_VALUE };
                Ok((
                    call.type_index(),
                    call.encode(SocialFacet::Groups.header(own))?,
                ))
            }))],
            Proof::Message(LEVEL_INFO_CHANGED_MSG_TYPE_INDEX),
            RETURN_WAIT,
        ),
    ]
}

/// The capture points of the war fort of `territory` in the attackers' order: the outer ones in
/// turn and the claim point ([`CLAIM_POINT_NUMBER`]) last.
///
/// # Errors
///
/// Names the territory when it has no war fort with capture points.
pub(crate) fn siege_points(
    data: &ActivityData<'_>,
    territory: u16,
) -> Result<Vec<[f32; 3]>, String> {
    let fort = data
        .war_forts
        .fort(territory)
        .filter(|fort| !fort.is_invasion_fort())
        .ok_or_else(|| format!("no war fort with capture points in territory {territory}"))?;
    let mut points: Vec<_> = fort.points.iter().collect();
    points.sort_by_key(|point| (point.number == CLAIM_POINT_NUMBER, point.number));
    Ok(points.into_iter().map(|point| point.place).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cold-stack refusal (6776 `EntranceActivationFailure`) fails the activation step
    /// instead of letting the teleport go out.
    #[test]
    fn a_match_accept_takes_each_new_pop_and_sends_5741_with_its_id_and_946() {
        const OWN: u64 = 77;
        const MATCH: &str = "00000000-0000-7000-8000-000000000001";
        const NEXT_MATCH: &str = "00000000-0000-7000-8000-000000000002";
        let popped = |id: &str| Matchmaking {
            match_id: Some(id.to_owned()),
            ..Matchmaking::default()
        };
        assert_eq!(
            pop_to_accept(&Matchmaking::default(), None),
            None,
            "no pop yet"
        );
        assert_eq!(pop_to_accept(&popped(MATCH), None).as_deref(), Some(MATCH));
        assert_eq!(pop_to_accept(&popped(MATCH), Some(MATCH)), None, "accepted");
        assert_eq!(
            pop_to_accept(&popped(NEXT_MATCH), Some(MATCH)).as_deref(),
            Some(NEXT_MATCH),
            "a new pop of a match that did not open"
        );
        let sent = accept_calls(OWN, MATCH).expect("the calls write");
        let types: Vec<u64> = sent.iter().map(|(type_index, _)| *type_index).collect();
        assert_eq!(
            types,
            [
                nw_messages::game_mode::ACCEPT_MATCHMAKING_MATCH_TYPE_INDEX,
                nw_messages::arena::TELEPORT_INTO_DUNGEON_TYPE_INDEX
            ]
        );
        let accept = GameModeClientMsg::decode(types[0], &sent[0].1)
            .expect("a matchmaking call")
            .expect("it reads");
        assert_eq!(
            accept,
            GameModeClientMsg::AcceptMatch {
                header: FacetHeader::of(facet_component::GAME_MODE_PARTICIPANT, OWN),
                match_id: MATCH.to_owned(),
                flag: ACCEPT_FLAG,
            }
        );
    }

    #[test]
    fn an_arena_answer_says_yes_only_when_it_allows_or_succeeds() {
        const OWN: u64 = 7;
        const GAME_MODE: u32 = 0x782c_103f;
        let header = FacetHeader::of(PLAYER_ARENA_COMPONENT, OWN);
        let verdict = |answer: ArenaServerMsg| arena_verdict(answer.type_index(), &answer.encode());
        assert!(
            verdict(ArenaServerMsg::ActivateResult {
                header,
                response: ActivateResponse::Success,
            })
            .is_ok()
        );
        assert!(
            verdict(ArenaServerMsg::ActivateResult {
                header,
                response: ActivateResponse::EntranceActivationFailure,
            })
            .is_err()
        );
        assert!(
            verdict(ArenaServerMsg::RemoteInteractResult {
                header,
                game_mode: GAME_MODE,
                allowed: true,
            })
            .is_ok()
        );
        assert!(
            verdict(ArenaServerMsg::RemoteInteractResult {
                header,
                game_mode: GAME_MODE,
                allowed: false,
            })
            .is_err()
        );
        assert!(arena_verdict(DUNGEON_ACTIVATE_RESULT_TYPE_INDEX, &[]).is_err());
    }

    #[test]
    fn a_camp_placement_has_the_retail_shape_with_the_owner_as_the_swapped_header() {
        const OWN: u64 = 7;
        const AT: [f32; 3] = [7_598.0, 4_149.0, 70.5];
        let steps = camp_steps(AT, 0, [3; ASSET_ID_LEN], &[("WoodT1".to_owned(), 5)]);
        assert_eq!(
            steps.len(),
            4,
            "the give, the placement, the build and its end"
        );
        assert!(matches!(steps[3].proof, Proof::Ended));
        let Some(Act::Call(make)) = steps[1].acts.first() else {
            panic!("a call");
        };
        let (type_index, body) = make(OWN).expect("encodes");
        let Some(Ok(TravelClientMsg::PlaceStructure { header, placement })) =
            TravelClientMsg::decode(type_index, &body)
        else {
            panic!("a placement");
        };
        assert_eq!(placement.position, AT);
        assert_eq!(
            (placement.socket, placement.value, placement.flag),
            (CAMP_SOCKET, CAMP_VALUE, CAMP_FLAG)
        );
        let mut swapped = header.entity.to_be_bytes().to_vec();
        swapped.extend(header.target.to_be_bytes());
        assert_eq!(placement.owner.to_vec(), swapped);
        assert!(matches!(
            steps[2].acts.first(),
            Some(Act::InteractCard {
                kind: APPLY_ALL_RESOURCES_KIND,
                ..
            })
        ));
    }

    #[test]
    fn a_state_line_gives_its_level() {
        assert_eq!(
            state_level(
                "state level 7 xp 0 coins 0 faction 0 pvp off inn none mount foot quests 0 done 0"
            ),
            Some(7)
        );
        assert_eq!(state_level("Done."), None);
    }

    #[test]
    fn the_inn_check_in_names_the_capital_of_the_innkeeper() {
        /// Made-up capital ids.
        const INN_HOUSE: &str = "00112233-4455-6677-8899-aabbccddeef0";
        const INNKEEPER: &str = "00112233-4455-6677-8899-aabbccddeeff";
        let capital = |id: &str, slice: &str| nw_world::records::Capital {
            id: id.to_owned(),
            position: [100.0, 200.0, 30.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: None,
            slice_name: slice.to_owned(),
            variant_name: None,
            slice_asset_id: String::new(),
        };
        let world = WorldFile {
            capitals: vec![
                capital(INN_HOUSE, "slices/social structures/inn/inn_a"),
                capital(INNKEEPER, "slices/characters/mission/NPC_1222_Innkeep"),
            ],
            ..WorldFile::default()
        };
        let tables = Tables::default();
        let town_stations = TownStations::default();
        let data = ActivityData {
            world: &world,
            tables: &tables,
            town_stations: &town_stations,
            war_forts: &WarForts::default(),
        };
        let steps = Activity::of(ActivityKind::Inn, &data).steps.expect("steps");
        let targets: Vec<u64> = steps
            .iter()
            .flat_map(|step| &step.acts)
            .filter_map(|act| match act {
                Act::Interact { request, .. } => Some(request.target_entity),
                _ => None,
            })
            .collect();
        assert!(!targets.is_empty(), "the open and the check in");
        assert!(
            targets
                .iter()
                .all(|target| Some(*target) == capital_id(INNKEEPER)),
            "{targets:?}"
        );
    }

    #[test]
    fn the_activities_without_a_flow_name_why() {
        let tables = Tables::default();
        let world = WorldFile::default();
        let town_stations = TownStations::default();
        let data = ActivityData {
            world: &world,
            tables: &tables,
            town_stations: &town_stations,
            war_forts: &WarForts::default(),
        };
        for kind in [ActivityKind::Group, ActivityKind::Music] {
            assert!(Activity::of(kind, &data).steps.is_err(), "{kind:?}");
        }
        assert_eq!(
            Activity::of(ActivityKind::Mount, &data)
                .steps
                .map(|steps| steps.len()),
            Err(format!("the item catalog has no {MOUNT_REWARD_BOX}")),
            "the mount opens its reward box: it needs the item catalog"
        );
        assert!(
            Activity::of(ActivityKind::Harvest, &data).steps.is_err(),
            "an empty level has no node"
        );
        assert!(
            Activity::of(ActivityKind::TownCraft, &data).steps.is_err(),
            "an empty content has no recipe and no settlement station"
        );
        assert!(
            Activity::of(ActivityKind::Company, &data).steps.is_err(),
            "an empty level has no town board and no war board"
        );
        assert_eq!(
            Activity::of(ActivityKind::EnemyCompany, &data)
                .steps
                .map(|steps| steps
                    .iter()
                    .map(|step| step.what.clone())
                    .collect::<Vec<_>>()),
            Ok(vec![
                "!faction covenant".to_owned(),
                "!company Bot War Company".to_owned()
            ]),
            "the enemy company needs no level data"
        );
        assert_eq!(
            Activity::of(ActivityKind::WarSignup, &data)
                .steps
                .map(|steps| steps.len()),
            Ok(7),
            "the war sign-up needs no level data"
        );
        assert_eq!(
            Activity::of(ActivityKind::InvasionSignup, &data)
                .steps
                .map(|steps| steps
                    .iter()
                    .map(|step| step.what.clone())
                    .collect::<Vec<_>>()),
            Ok(vec![
                "!faction syndicate".to_owned(),
                "!company Bot Test Company".to_owned(),
                "!war 16 cancel".to_owned(),
                "!owner 16 Bot Test Company".to_owned(),
                "!invasion 16 10".to_owned(),
                "use the war board from afar (2791)".to_owned(),
                "sign up as a defender of the invasion (2762)".to_owned(),
                "!war 16 cancel".to_owned(),
                "!owner 16 clear".to_owned(),
            ]),
            "the invasion sign-up needs no level data"
        );
        assert_eq!(
            Activity::of(ActivityKind::Level, &data)
                .steps
                .map(|steps| steps.len()),
            Ok(4)
        );
        for kind in [
            ActivityKind::Arena,
            ActivityKind::OutpostRush,
            ActivityKind::CaptureTheFlag,
            ActivityKind::QueueLeave,
        ] {
            assert!(
                Activity::of(kind, &data).steps.is_err(),
                "an empty content has no game mode of {kind:?}"
            );
        }
    }

    #[test]
    fn a_harvest_names_its_node_as_a_client_does() {
        let tables = Tables::default();
        let world = WorldFile {
            gatherables: vec![
                nw_world::records::GatherablePoint {
                    gatherable_id: lowercase_crc32("Other"),
                    ..Default::default()
                },
                nw_world::records::GatherablePoint {
                    gatherable_id: lowercase_crc32(HARVESTED_GATHERABLE),
                    position: [5.0, 6.0, 7.0],
                    ..Default::default()
                },
            ],
            ..WorldFile::default()
        };
        let town_stations = TownStations::default();
        let data = ActivityData {
            world: &world,
            tables: &tables,
            town_stations: &town_stations,
            war_forts: &WarForts::default(),
        };
        let steps = Activity::of(ActivityKind::Harvest, &data)
            .steps
            .expect("a node");
        let (target, entity) = node_id(1);
        assert!(
            matches!(
                steps[0].acts.as_slice(),
                [
                    Act::Teleport([5.0, 6.0, 7.0]),
                    Act::Interact { request, .. }
                ] if request.target == target
                    && request.target_entity == entity
                    && request.gatherable == Some(lowercase_crc32(HARVESTED_GATHERABLE))
            ),
            "an empty node id names the first node anyone gathered with it"
        );
    }

    #[test]
    fn a_state_line_shows_the_pvp_flag_on_or_on_its_way() {
        let line = |pvp: &str| {
            format!(
                "state level 65 xp 1 coins 2 faction 2 pvp {pvp} inn none mount foot quests 0 done 0"
            )
        };
        assert!(pvp_flag_shown(&line("on")));
        assert!(pvp_flag_shown(&line("pending")));
        assert!(!pvp_flag_shown(&line("off")));
        assert!(!pvp_flag_shown("pvp on"), "only a state line");
    }

    #[test]
    fn a_pvp_foe_drops_god_mode_flags_in_the_other_faction_and_gets_up_after_deaths() {
        let steps = pvp_foe();
        assert!(matches!(steps[0].acts.as_slice(), [Act::Gm(line)] if line == "!god off"));
        assert!(matches!(steps[1].acts.as_slice(), [Act::Gm(line)] if line == "!faction marauder"));
        assert_ne!(PVP_FOE_FACTION, PVP_KILLER_FACTION, "two factions are foes");
        let rises = steps
            .iter()
            .filter(|step| matches!(step.acts.as_slice(), [Act::Gm(line), Act::Pause(_)] if line == "!heal"))
            .count();
        assert_eq!(
            u64::try_from(rises).expect("count"),
            FOE_STAND.as_secs() / FOE_RISE_GAP.as_secs()
        );
        let killer = pvp_flag_steps(PVP_KILLER_FACTION);
        assert!(
            matches!(killer[0].acts.as_slice(), [Act::Gm(line)] if line == "!faction syndicate")
        );
        assert!(matches!(killer[2].acts.as_slice(), [Act::PvpFlagOn]));
        assert!(
            matches!(killer[4].proof, Proof::Line(..)),
            "the state shows the flag on"
        );
    }

    #[test]
    fn a_song_creates_picks_starts_waits_for_its_length_and_leaves() {
        const SONG: u32 = 0x1bfb_903e;
        const SHEET: u32 = 0x00d9_dc3e;
        const LENGTH_S: f32 = 60.888;
        const AT: [f32; 3] = [8904.8, 4223.9, 138.7];
        const OWN: u64 = 7;
        let steps = song_steps(SONG, SHEET, LENGTH_S, AT);
        assert!(matches!(
            steps[0].acts.as_slice(),
            [Act::Teleport(at), Act::Call(_)] if *at == AT
        ));
        assert!(matches!(
            steps[0].proof,
            Proof::Message(PERFORMANCE_CREATED_TYPE_INDEX)
        ));
        let calls: Vec<u64> = steps
            .iter()
            .flat_map(|step| &step.acts)
            .filter_map(|act| match act {
                Act::Call(make) => Some(make(1).expect("call").0),
                _ => None,
            })
            .collect();
        assert_eq!(calls, [4891, 4867, 4879, 4865, 5091, 5233, 4881]);
        let Act::Call(make) = &steps[0].acts[1] else {
            panic!("the create call");
        };
        let (_, body) = make(OWN).expect("call");
        assert_eq!(
            MusicClientMsg::decode(4891, &body)
                .expect("a music call")
                .expect("decodes")
                .header(),
            FacetHeader::of_child(MUSIC_FACET, MUSIC_CHILD, OWN),
            "the own player's music child, as the sim reads it"
        );
        assert!(steps.iter().any(|step| matches!(
            step.acts.as_slice(),
            [_, Act::Pause(wait)] if *wait > Duration::from_secs_f32(LENGTH_S)
        )));
    }

    #[test]
    fn a_course_rides_its_reference_path_node_by_node_and_waits_for_the_end() {
        use nw_content::quick_courses::QuickCourse;
        use nw_world::quick_courses::{QuickCourseNode, QuickCoursePath};
        const START: [f32; 3] = [8921.0, 5130.3, 151.1];
        const FINISH: [f32; 3] = [9492.9, 4260.6, 110.4];
        let course = |id: &str, path: Option<&str>| QuickCourse {
            id: id.to_owned(),
            timed: true,
            starting_seconds: None,
            path: path.map(str::to_owned),
            node_time_multiplier: 1.0,
        };
        let node = |position| QuickCourseNode {
            position,
            node_type: "Start".to_owned(),
        };
        let routes = CourseRoutes {
            rules: QuickCourseRules::build([course("EF_Rally_3", Some("EF_Rally_1"))], Vec::new()),
            paths: QuickCoursePaths::of([QuickCoursePath {
                course: "EF_Rally_1".to_owned(),
                nodes: vec![node(START), node(FINISH)],
            }]),
        };
        let nodes = routes.nodes("ef_rally_3").expect("the reference path");
        assert_eq!(nodes, vec![START, FINISH]);
        assert_eq!(routes.nodes("EF_Rally_9"), None);
        let steps = course_steps(&nodes, true);
        assert!(matches!(
            steps[0].acts.as_slice(),
            [Act::Teleport(at), Act::Pause(_)] if *at == START
        ));
        assert!(matches!(steps[0].proof, Proof::Sent));
        assert!(matches!(
            steps[1].proof,
            Proof::Message(QUICK_COURSE_ENDED_TYPE_INDEX)
        ));
        assert!(matches!(course_steps(&nodes, false)[1].proof, Proof::Sent));
    }

    /// Live 2026-11-22 10:28:46: a lion that moved in the fight died out of the bot's reach.
    #[test]
    fn a_skin_goes_to_the_corpse_then_gathers_it() {
        const CORPSE: [u8; RAW_ID_LEN] = [9; RAW_ID_LEN];
        const AT: [f32; 3] = [7250.7, 8173.0, 196.0];
        let gatherable = lowercase_crc32("Primal_Lion");
        let steps = skin_steps(CORPSE, gatherable, AT);
        assert!(
            matches!(
                steps[0].acts.as_slice(),
                [Act::Teleport(at), Act::Interact { request, .. }]
                    if *at == AT
                        && request.target == CORPSE
                        && request.kind == GATHER_KIND
                        && request.gatherable == Some(gatherable)
            ),
            "the bot stands at the corpse when it asks to skin it"
        );
    }
}
