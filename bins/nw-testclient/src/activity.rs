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
use nw_content::starter::StarterKits;
use nw_content::town_props::ASSET_ID_LEN;
use nw_content::town_stations::TownStations;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::arena::{
    ActivateResponse, ArenaClientMsg, ArenaServerMsg, DUNGEON_ACTIVATE_RESULT_TYPE_INDEX,
    PLAYER_ARENA_COMPONENT, REMOTE_INTERACT_RESULT_TYPE_INDEX,
};
use nw_messages::client_input::{
    APPLY_ALL_RESOURCES_KIND, CRAFTING_STATION_KIND, GATHER_KIND, LOOT_KIND, LORE_KIND,
    TOWN_BOARD_KIND, TOWN_BOARD_NAME, WAR_BOARD_KIND, WAR_BOARD_NAME,
};
use nw_messages::conversation_client::ObjectiveInteractorClientMsg;
use nw_messages::facet::{
    CRAFTING_PROGRESS_TYPE_INDEX, INGREDIENT_ITEM, TELEPORT_WITH_LOADING_SCREEN_TYPE_INDEX,
};
use nw_messages::mount::{MOUNT_FACET, MountClientMsg, SUMMON_PENDING_TYPE_INDEX};
use nw_messages::travel::{
    LandmarkRef, PLACEMENT_RESULTS_TYPE_INDEX, StructurePlacement, TravelClientMsg, component,
};
use nw_messages::war_signup::{
    SET_ROSTER_RESPONSE_RECEIVED_TYPE_INDEX, SIDE_ATTACKER, SIDE_DEFENDER,
    SIGNUP_LIST_RECEIVED_TYPE_INDEX, SIGNUP_RESPONSE_RECEIVED_TYPE_INDEX,
};
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    BundleMember, ClientFacetMsg, CraftIngredient, FacetHeader, InteractionRequest,
    LEVEL_INFO_CHANGED_MSG_TYPE_INDEX, MemberState, MessageError, TERRITORY_FACET_READY_TYPE_INDEX,
    TERRITORY_INTERACTOR_COMPONENT, WarSignupClientMsg,
};
use nw_world::WorldFile;
use nw_world::capitals::capital_id;
/// A game master line the sim carried out.
use sim::world::GM_DONE as DONE;

use crate::gather::{GatherNode, node_id};

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
const STEP_WAIT: Duration = Duration::from_secs(10);
/// How long a gather or a fast travel may take: the gather time, the 10 s channel.
const LONG_WAIT: Duration = Duration::from_secs(25);
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
}

/// One call of a step, written once the bot knows its own entity number.
pub type Call = Box<dyn Fn(u64) -> Result<(u64, Vec<u8>), MessageError>>;

/// What a step sends.
pub enum Act {
    /// A game master line.
    Gm(String),
    /// A game master's teleport to the place; the bot's next inputs stand there.
    Teleport([f32; 3]),
    /// A press of the summon mount key in the bot's input.
    SummonKey,
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
}

/// A step of `what` that sends `acts` and waits `wait` for `proof`.
fn step(what: impl Into<String>, acts: Vec<Act>, proof: Proof, wait: Duration) -> Step {
    Step {
        what: what.into(),
        acts,
        proof,
        wait,
    }
}

/// A game master line answered with [`DONE`].
fn gm_done(line: String) -> Step {
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
const fn teleport(at: [f32; 3]) -> Act {
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
            ActivityKind::Harvest => harvest(data),
            ActivityKind::Craft => craft(data),
            ActivityKind::TownCraft => town_craft(data),
            ActivityKind::Inn => inn(data),
            ActivityKind::Shrine => shrine(data),
            ActivityKind::Dungeon => dungeon(data),
            ActivityKind::Board => board(data),
            ActivityKind::Group => Err(
                "needs a second bot in the world at the same time; a play run has one bot"
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

fn pvp() -> Vec<Step> {
    let toggle = || facet(|header| ClientFacetMsg::TogglePvpFlag { header }, 0);
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
    let reward_box = StarterKits::from_tables(data.tables)
        .item(MOUNT_REWARD_BOX)
        .map(|item| item.item_index)
        .ok_or_else(|| format!("the item catalog has no {MOUNT_REWARD_BOX}"))?;
    Ok(vec![
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
    ])
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

/// The steps of a skin of the corpse of raw id `corpse` as gatherable `gatherable` (the
/// CRC-32 of its lowercase id): the gather request of retail's skin (beach capture `12-3`:
/// kind 14, the corpse's raw id and facet entity, gatherable `Boar`), then its end. The bot
/// stands at the corpse after its fight.
pub fn skin_steps(corpse: [u8; RAW_ID_LEN], gatherable: u32) -> Vec<Step> {
    gather_of(
        "skin the corpse".to_owned(),
        Vec::new(),
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
        step(
            format!("teleport into game mode {mode}"),
            vec![arena_call(|header| ArenaClientMsg::TeleportIntoDungeon {
                header,
            })],
            Proof::Message(LEVEL_INFO_CHANGED_MSG_TYPE_INDEX),
            LONG_WAIT,
        ),
    ]
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
const BOT_FACTION: &str = "syndicate";
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
const ENEMY_FACTION: &str = "covenant";

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
fn war_call(make: impl Fn(FacetHeader) -> WarSignupClientMsg + 'static) -> Act {
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
    vec![
        step(
            cancel.clone(),
            vec![Act::Gm(cancel.clone())],
            Proof::Line(Box::new(|_| true), "any answer".to_owned()),
            STEP_WAIT,
        ),
        gm_done(format!(
            "!war {WAR_TERRITORY} {WAR_START_MINUTES} {ENEMY_COMPANY}"
        )),
        step(
            "use the war board from afar (2791)",
            vec![war_call(|header| {
                WarSignupClientMsg::RequestRemoteInteract {
                    header,
                    territory: WAR_TERRITORY,
                }
            })],
            Proof::Message(TERRITORY_FACET_READY_TYPE_INDEX),
            STEP_WAIT,
        ),
        step(
            "sign up as the attacker (2762)",
            vec![war_call(|header| WarSignupClientMsg::RequestSignup {
                header,
                side: SIDE_ATTACKER,
                territory: WAR_TERRITORY,
            })],
            Proof::Message(SIGNUP_RESPONSE_RECEIVED_TYPE_INDEX),
            STEP_WAIT,
        ),
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
    ]
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
        step(
            cancel.clone(),
            vec![Act::Gm(cancel.clone())],
            Proof::Line(Box::new(|_| true), "any answer".to_owned()),
            STEP_WAIT,
        ),
        gm_done(format!("!owner {INVASION_TERRITORY} {BOT_COMPANY}")),
        gm_done(format!(
            "!invasion {INVASION_TERRITORY} {INVASION_START_MINUTES}"
        )),
        step(
            "use the war board from afar (2791)",
            vec![war_call(|header| {
                WarSignupClientMsg::RequestRemoteInteract {
                    header,
                    territory: INVASION_TERRITORY,
                }
            })],
            Proof::Message(TERRITORY_FACET_READY_TYPE_INDEX),
            STEP_WAIT,
        ),
        step(
            "sign up as a defender of the invasion (2762)",
            vec![war_call(|header| WarSignupClientMsg::RequestSignup {
                header,
                side: SIDE_DEFENDER,
                territory: INVASION_TERRITORY,
            })],
            Proof::Message(SIGNUP_RESPONSE_RECEIVED_TYPE_INDEX),
            STEP_WAIT,
        ),
        gm_done(cancel),
        gm_done(format!("!owner {INVASION_TERRITORY} clear")),
    ]);
    steps
}

/// The bot joins [`ENEMY_FACTION`] and makes [`ENEMY_COMPANY`] ([`ActivityKind::EnemyCompany`]).
fn enemy_company() -> Vec<Step> {
    faction_and_company(ENEMY_FACTION, ENEMY_COMPANY)
}

/// The bot joins `faction` and makes the company `company` (or is in it already, or the name
/// is taken).
fn faction_and_company(faction: &str, company: &str) -> Vec<Step> {
    let made = sim::world::company_made(company);
    vec![
        step(
            format!("!faction {faction}"),
            vec![Act::Gm(format!("!faction {faction}"))],
            Proof::Line(
                Box::new(|line| line == DONE || line == sim::world::FACTION_UNCHANGED),
                "a faction".to_owned(),
            ),
            STEP_WAIT,
        ),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The cold-stack refusal (6776 `EntranceActivationFailure`) fails the activation step
    /// instead of letting the teleport go out.
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
}
