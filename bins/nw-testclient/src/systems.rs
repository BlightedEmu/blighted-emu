// SPDX-License-Identifier: AGPL-3.0-or-later
//! `nw-testclient systems`: checks of the server's systems by game master bots, through the
//! client's own calls, each proved by what the server sends back.
//!
//! - [`SystemsCheck::SaveRecord`] gives one bot coins, items, a running quest, weapon mastery
//!   with a spent weapon point, spent attribute points, territory standing and a stored stack,
//!   then writes what the server shows of it to a record file: the `!state`, `!queststate` and
//!   `!sheet` lines and the bag as the client got it. [`SystemsCheck::SaveCheck`] logs the same
//!   bot in again (after a log out, or after a restart of the servers) with no game master setup
//!   and compares what the server sends now with the record.
//! - [`SystemsCheck::Economy`]: one bot buys at a shop, puts a stack into a settlement storage and
//!   takes it back, salvages an item and crafts a recipe.
//! - [`SystemsCheck::Trade`]: one bot posts a sell order at a settlement's trading post and a
//!   second bot buys it there.
//! - [`SystemsCheck::Social`]: two bots chat, become friends, form a group and leave it, and the
//!   first makes a company that the second joins.
//! - [`SystemsCheck::Progress`]: one bot gains a level, spends attribute and weapon points and
//!   dies; a second bot revives it.
//!
//! The bots of one check tell each other what they did on a [`SystemsBoard`] (their characters
//! come from their group seats, [`GroupSeat::members_ready`]).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use clap::ValueEnum;
use data_encoding::HEXLOWER;
use nw_content::character_points::{CharacterPoints, STRENGTH, territory_pool_crc};
use nw_content::progression::{MasteryKind, Progression};
use nw_content::starter::StarterKits;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::buildables::GuildCrest;
use nw_messages::client_input::REVIVE_PLAYER_KIND;
use nw_messages::contracts::{
    CONTRACT_CREATED_TYPE_INDEX, CONTRACTS_COMPONENT, CREATE_ITEM_BLOCK_LEN, ContractCall,
    ContractKind, ContractMessage, CreateOrder, RETAIL_REQUEST_TAIL, reason,
};
use nw_messages::entity_ids::{facet_of_raw_id, raw_id_of_entity};
use nw_messages::item_calls::{ContainerRef, ItemCallMsg};
use nw_messages::item_descriptor::ShortItemDescriptor;
use nw_messages::shop::ShopPurchaseCall;
use nw_messages::social::chat::{ChatCall, ChatChannel};
use nw_messages::social::groups::{GroupCall, LEAVE_VALUE};
use nw_messages::social::guilds::{GuildCall, GuildPush};
use nw_messages::social::players::{ONLINE_STATE_CHANGE_TYPE_INDEX, PlayerCall};
use nw_messages::social::{SocialFacet, SocialPush};
use nw_messages::war_signup::{SIDE_ATTACKER, SIDE_DEFENDER};
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{BodyCodec, WarSignupClientMsg};
use nw_messages::{ClientFacetMsg, FacetHeader, InteractionRequest, MessageError, PointsClientMsg};
use nw_world::capitals::capital_id;
use nw_world::records::Capital;
use sim::shop::{ShopCatalog, ShopOffer};
use sim::trading_post::{is_settlement_slice, settlement_storage, settlement_wire_id};
use sim::world::{SHEET_ATTRIBUTES, SHEET_LINE, SHEET_POINTS, SHEET_STORAGE};

use crate::activity::{
    Act, Activity, ActivityData, ActivityKind, BOT_FACTION, BotSight, ENEMY_FACTION, FILL_WAIT,
    INVASION_FIGHT_START_MINUTES, INVASION_FIGHT_TERRITORY, INVASION_LONGEST, LONG_WAIT, MatchGoal,
    OPEN_STORAGE_KIND, OPEN_STORAGE_NAME, Proof, STEP_WAIT, Step, WAR_FIGHT_START_MINUTES,
    WAR_FIGHT_TERRITORY, WAR_LONGEST, accept_raid_invite, faction_step, gm_any, gm_done,
    siege_points, step, teleport, war_board_signup, war_call, war_play_steps,
};
use crate::group::GroupSeat;
use crate::play::{CALL_COMPONENT, SALVAGE_FIRST, SALVAGE_ONE, SALVAGE_UNKNOWN};
use crate::rep::Received;

/// A check of `nw-testclient systems`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SystemsCheck {
    /// One bot gets what a save keeps and writes the record file.
    SaveRecord,
    /// The bot of a record logs in again and compares what the server sends with the record.
    SaveCheck,
    /// One bot buys at a shop, uses a settlement storage, salvages and crafts.
    Economy,
    /// One bot sells at a trading post, a second bot buys.
    Trade,
    /// Two bots chat, become friends, group, leave the group and share a company.
    Social,
    /// One bot levels up, spends points and dies; a second bot revives it.
    Progress,
    /// One new bot with no game master setup fights open world mobs until a level comes from
    /// kill experience alone.
    KillXp,
    /// Two companies of two bots each: the first pair defends a fort, the second declares a
    /// game master war on it; both sides sign up, fill their rosters, move into the war and
    /// play it to its result (the attackers take the capture points, the claim point last).
    War,
    /// A company of two bots owns the invasion fort; a game master schedules its invasion, both
    /// sign up, move in and hold the claim point to the invasion's result.
    Invasion,
}

impl SystemsCheck {
    /// How many bots the check takes.
    pub const fn bots(self) -> RangeInclusive<usize> {
        match self {
            Self::SaveRecord | Self::SaveCheck | Self::Economy | Self::KillXp => 1..=1,
            Self::Trade | Self::Social | Self::Progress => 2..=2,
            Self::Invasion => 2..=INVASION_MOST_BOTS,
            Self::War => WAR_BOTS..=WAR_BOTS,
        }
    }

    /// Whether its bots send the game master setup lines first: every check but the save
    /// check, which reads the character as its save gave it back, and the kill experience
    /// check, whose bot keeps its own level and gear.
    pub const fn game_master_setup(self) -> bool {
        !matches!(self, Self::SaveCheck | Self::KillXp)
    }
}

/// What the bots of one check tell each other: values by key.
#[derive(Debug, Default)]
pub struct SystemsBoard {
    values: Mutex<BTreeMap<String, String>>,
}

impl SystemsBoard {
    /// Notes `value` under `key`.
    pub fn post(&self, key: &str, value: String) {
        self.values
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.to_owned(), value);
    }

    /// The value under `key`, once a bot noted one.
    pub fn get(&self, key: &str) -> Option<String> {
        self.values
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .cloned()
    }
}

/// What the steps of a check read.
pub struct SystemsData<'a> {
    /// The level and the content.
    pub activity: &'a ActivityData<'a>,
    /// The record file of the save checks.
    pub record: Option<PathBuf>,
    /// The board of the check's bots.
    pub board: Arc<SystemsBoard>,
    /// The account of the first bot: it names the company of the social check.
    pub first_account: String,
    /// The vitals ids (CRC-32 of each lowercase id) of the wave mobs of the invasion fort of
    /// the invasion check, from the invasion fort table.
    pub wave_vitals: Vec<u32>,
}

/// The item the economy and the save checks give, store and salvage: a weapon with durability,
/// so its stack keeps its instance id in a storage (the sim's `stored_move`), and with a salvage.
const GIVEN_WEAPON: &str = "1hSwordT1";
/// The stackable item the save and trade checks give and store or sell.
const GIVEN_STACK: &str = "FlintT1";
/// How many of [`GIVEN_STACK`] a bot gets.
const STACK_GIVEN: u32 = 7;
/// How many of them the save check stores.
const STACK_STORED: u32 = 3;
/// How many of them the trade check sells.
const STACK_SOLD: u32 = 5;
/// The price of one sold unit, hundredths of a coin.
const UNIT_PRICE: u64 = 100;
/// The posting length of the sell order, days (one of the retail lengths).
const POSTING_DAYS: u32 = 3;
/// The coins a bot gets before it pays: more than any fee, purchase or order costs.
const COINS_GIVEN: u64 = 1_000_000;
/// The coins the save check sets: a value no setup gives.
const SAVED_COINS: u64 = 123_457;
/// The weapon mastery rank a bot gets before it spends a weapon point (as the client shows it).
const MASTERY_RANK: u32 = 3;
/// The territory standing experience the save check gives.
const STANDING_GIVEN: u32 = 1_000;
/// The weapon mastery experience the save check gives on top of [`MASTERY_RANK`].
const MASTERY_GIVEN: u32 = 777;
/// The attribute points a bot spends on Strength: one, which a character has from its second
/// level on.
const STRENGTH_SPENT: u32 = 1;
/// The quest the save check starts and leaves running ("My Kingdom for a Horse").
const SAVED_QUEST: &str = "9401_BefriendHorse";
/// The faction both bots of the social check join: a company invite needs one faction.
const COMPANY_FACTION: &str = "covenant";
/// The end of a company name after the first bot's account name.
const COMPANY_NAME_END: &str = " Company";
/// The name a trading post open carries (the sim's `open_trading_post` doc: the client's name
/// of the post).
const TRADING_POST_NAME: &[u8] = b"@TradingPostT5_MasterName";
/// The interaction kind of a trading post open (the sim's `economy::TRADING_POST_KIND`).
const TRADING_POST_KIND: u8 = 21;
/// `FriendRequestTypes` `Invite` and `Accept` (`nw_messages::social::players::FriendRequest`).
const FRIEND_INVITE: u32 = 1;
const FRIEND_ACCEPT: u32 = 2;
/// How long an answer of another bot or of a database job may take.
const OTHER_BOT_WAIT: Duration = Duration::from_secs(120);
/// How long the answer lines of a game master read take to arrive.
const READ_PAUSE: Duration = Duration::from_secs(2);
/// The first words of the sim's `!state` line, and the parts of it a save check compares
/// (each part a word and its value; the PvP flag and the mount phase are not saved).
const STATE_LEVEL: &str = "state level ";
const SAVED_STATE_PARTS: [&str; 7] = ["level", "xp", "coins", "faction", "inn", "quests", "done"];
/// The first words of each line of a record file.
const RECORD_STATE: &str = "state";
const RECORD_QUEST: &str = "quest";
const RECORD_BAG: &str = "bag";
/// The lines the save check leaves out: the answer of a read that names no player.
const NOT_HERE: &str = "That player is not on this server.";
/// The `!queststate` answer words of a quest (the sim's `quest_state_lines`).
const QUEST_ANSWER_WORDS: [&str; 5] = ["running", "finished", "done", "not", "step"];
/// The board keys.
const KEY_CONTRACT: &str = "contract";
const KEY_BOUGHT: &str = "bought";
const KEY_CHAT: &str = "chat";
const KEY_FRIEND: &str = "friend";
const KEY_GUILD: &str = "guild";
const KEY_COMPANY_INVITED: &str = "company invited";
const KEY_GROUP_LEFT: &str = "group left";
const KEY_DEAD: &str = "dead";
const KEY_REVIVED: &str = "revived";
/// The first words of a bot's own chat line in the social check.
const CHAT_MARK: &str = "systems check line of";

/// The steps of the bot of `seat` in `check`, or why it cannot play.
///
/// # Errors
///
/// Names what the content or the level lacks.
pub fn seat_steps(
    check: SystemsCheck,
    seat: &GroupSeat,
    data: &SystemsData<'_>,
) -> Result<Vec<Step>, String> {
    let mut steps = vec![meet_step(seat)];
    steps.extend(match check {
        SystemsCheck::SaveRecord => save_record(data)?,
        SystemsCheck::SaveCheck => save_check(data)?,
        SystemsCheck::Economy => economy(data)?,
        SystemsCheck::Trade => trade(seat, data)?,
        SystemsCheck::Social => social(seat, data)?,
        SystemsCheck::Progress => progress(seat, data)?,
        SystemsCheck::KillXp => kill_xp(),
        SystemsCheck::War => war(seat, data)?,
        SystemsCheck::Invasion => invasion(seat, data)?,
    });
    Ok(steps)
}

/// The step that waits until every bot of the check is in the world: each one's seat is ready
/// and names its character.
fn meet_step(seat: &GroupSeat) -> Step {
    let ready = seat.clone();
    let met = seat.clone();
    step(
        "meet the other bots of the check",
        vec![Act::Dynamic(Box::new(move |_| {
            ready.set_ready();
            Some(Ok(Vec::new()))
        }))],
        Proof::Check(
            Box::new(move |_| met.members_ready().map(|_| Ok(()))),
            "every bot in the world".to_owned(),
        ),
        OTHER_BOT_WAIT,
    )
}

/// The character (UUID text) of the bot of seat `wanted`, once every bot of the check is in the
/// world.
fn seat_character(seat: &GroupSeat, wanted: usize) -> Option<String> {
    let others = seat.members_ready()?;
    let index = if wanted > seat.number() {
        wanted - 1
    } else {
        wanted
    };
    others.into_iter().nth(index)
}

/// The seat of the other bot of a check of two.
const fn other_seat(seat: &GroupSeat) -> usize {
    if seat.leads() { 1 } else { 0 }
}

/// A facet call of the bot.
fn header(own: u64) -> FacetHeader {
    FacetHeader::of(CALL_COMPONENT, own)
}

/// An act that sends one call written for the bot's entity number.
fn call(make: impl Fn(u64) -> Result<(u64, Vec<u8>), MessageError> + 'static) -> Act {
    Act::Dynamic(Box::new(move |sight| {
        Some(make(sight.own).map(|call| vec![call]))
    }))
}

/// The total quantity of client item index `item_index` in `bag`.
fn bag_count(bag: &[nw_messages::ItemRecord], item_index: u32) -> u32 {
    bag.iter()
        .filter(|item| item.item_index == item_index)
        .map(|item| item.quantity)
        .sum()
}

/// The bag slot and the instance of the first stack of `item_index` in `bag`.
fn bag_stack(bag: &[nw_messages::ItemRecord], item_index: u32) -> Option<(u32, [u8; RAW_ID_LEN])> {
    bag.iter().enumerate().find_map(|(slot, item)| {
        (item.quantity > 0 && item.item_index == item_index)
            .then(|| Some((u32::try_from(slot).ok()?, item.instance)))
            .flatten()
    })
}

/// A proof that the bag holds `wanted` (as `holds` takes it) of `item_index`.
fn bag_shows(item_index: u32, holds: impl Fn(u32) -> bool + 'static, described: String) -> Proof {
    Proof::Check(
        Box::new(move |sight| holds(bag_count(sight.bag, item_index)).then_some(Ok(()))),
        described,
    )
}

/// The client item index of item `item_id`.
fn item_index(data: &SystemsData<'_>, item_id: &str) -> Result<u32, String> {
    StarterKits::from_tables(data.activity.tables)
        .item(item_id)
        .map(|item| item.item_index)
        .ok_or_else(|| format!("the item catalog has no {item_id}"))
}

/// A settlement of the level: the capital whose trading post and storage the checks use (the
/// first settlement capital by slice name, so every run uses the same one).
fn settlement<'a>(data: &SystemsData<'a>) -> Result<&'a Capital, String> {
    data.activity
        .world
        .capitals
        .iter()
        .filter(|capital| is_settlement_slice(&capital.slice_name))
        .filter(|capital| capital_id(&capital.id).is_some())
        .min_by(|a, b| a.slice_name.cmp(&b.slice_name))
        .ok_or_else(|| "the level has no settlement".to_owned())
}

/// The storage id of settlement capital `capital` ([`settlement_storage`]).
fn storage_of(capital: &Capital) -> [u8; RAW_ID_LEN] {
    settlement_storage(settlement_wire_id(
        capital_id(&capital.id).unwrap_or_default(),
    ))
}

/// The steps that go to settlement `capital` and open its storage (interaction 26 named as the
/// client names it): the storage stays open for the next item calls.
fn open_storage_steps(capital: &Capital) -> Vec<Step> {
    let storage = storage_of(capital);
    vec![step(
        format!(
            "go to the settlement of {} and open its storage",
            capital.slice_name
        ),
        vec![
            teleport(capital.position),
            Act::Interact {
                request: InteractionRequest {
                    target: storage,
                    target_entity: facet_of_raw_id(&storage),
                    interaction: 0,
                    kind: OPEN_STORAGE_KIND,
                    gatherable: None,
                },
                name: OPEN_STORAGE_NAME.to_vec(),
            },
        ],
        Proof::Accepted,
        STEP_WAIT,
    )]
}

/// A put of `quantity` (0: the whole stack) of the first bag stack of `item_index` into the
/// open storage (2801).
fn deposit(item_index: u32, quantity: u32) -> Act {
    Act::Dynamic(Box::new(move |sight| {
        let (_, instance) = bag_stack(sight.bag, item_index)?;
        let call = ItemCallMsg::GiveItem {
            header: header(sight.own),
            container: no_container(),
            instance,
            quantity,
        };
        Some(call.encode().map(|body| vec![(call.type_index(), body)]))
    }))
}

/// The container a storage call names: the open one (the sim reads the open storage).
const fn no_container() -> ContainerRef {
    ContainerRef {
        id: [0; RAW_ID_LEN],
        entity: 0,
        extra: 0,
    }
}

/// The weapon point a bot spends: the first ability point (by pool, then point id) of a weapon
/// mastery's pool that needs no other point; with its pool's mastery id.
fn weapon_point(data: &SystemsData<'_>) -> Result<(String, String), String> {
    let tables = data.activity.tables;
    let progression = Progression::of_tables(tables);
    let points = CharacterPoints::of_tables(tables);
    tables
        .progression_point_data
        .iter()
        .flat_map(|table| &table.rows)
        .filter_map(|row| {
            let rule = points.point(lowercase_crc32(&row.progression_point_id))?;
            let mastery = progression.mastery_by_crc(rule.pool)?;
            (rule.requires.is_none() && rule.max_level > 0)
                .then(|| (mastery.id.clone(), rule.id.clone()))
        })
        .min()
        .ok_or_else(|| "the content has no weapon point without a requirement".to_owned())
}

/// The territory standing a save check raises: the first ranked progression (by id) that has a
/// territory standing pool.
fn territory_standing(data: &SystemsData<'_>) -> Result<String, String> {
    let tables = data.activity.tables;
    let progression = Progression::of_tables(tables);
    let points = CharacterPoints::of_tables(tables);
    progression
        .categoricals()
        .filter(|mastery| mastery.kind == MasteryKind::Ranked)
        .filter(|mastery| {
            points
                .territory_pool(territory_pool_crc(&mastery.id))
                .is_some()
        })
        .map(|mastery| mastery.id.clone())
        .min()
        .ok_or_else(|| "the content has no territory standing".to_owned())
}

/// The steps that spend the attribute points `STRENGTH_SPENT` on Strength (1502) and the weapon
/// point `point` of mastery `pool` (1112, then 2857 applies it), each proved by the `!sheet`
/// line of its part.
fn spend_steps(pool: &str, point: &str) -> Vec<Step> {
    let pool_crc = lowercase_crc32(pool);
    let point_crc = lowercase_crc32(point);
    let strength = format!("{STRENGTH}:");
    let point_entry = format!("{point}:");
    vec![
        step(
            format!("spend {STRENGTH_SPENT} attribute points on Strength"),
            vec![
                call(|own| {
                    let call = PointsClientMsg::SpendAttributePoints {
                        header: header(own),
                        attributes: vec![STRENGTH],
                        points: vec![STRENGTH_SPENT],
                    };
                    Ok((call.type_index(), call.encode()))
                }),
                Act::Gm("!sheet".to_owned()),
                Act::Pause(READ_PAUSE),
            ],
            Proof::Check(
                Box::new(move |sight| {
                    let line = sheet_part(sight.lines, SHEET_ATTRIBUTES)?;
                    Some(if line.contains(&strength) {
                        Ok(())
                    } else {
                        Err(format!("the sheet shows no spent Strength: {line}"))
                    })
                }),
                "spent Strength on the sheet".to_owned(),
            ),
            STEP_WAIT,
        ),
        gm_done(format!("!mastery {pool} {MASTERY_RANK}")),
        step(
            format!("assign and apply weapon point {point} of {pool}"),
            vec![
                call(move |own| {
                    let call = PointsClientMsg::AssignPoolPoints {
                        header: header(own),
                        pool: pool_crc,
                        point: point_crc,
                        points: 1,
                    };
                    Ok((call.type_index(), call.encode()))
                }),
                call(|own| {
                    let call = PointsClientMsg::ApplyAbilityPoints {
                        header: header(own),
                    };
                    Ok((call.type_index(), call.encode()))
                }),
                Act::Gm("!sheet".to_owned()),
                Act::Pause(READ_PAUSE),
            ],
            Proof::Check(
                Box::new(move |sight| {
                    let line = sheet_part(sight.lines, SHEET_POINTS)?;
                    Some(if line.contains(&point_entry) {
                        Ok(())
                    } else {
                        Err(format!("the sheet shows no assigned point: {line}"))
                    })
                }),
                "the assigned point on the sheet".to_owned(),
            ),
            STEP_WAIT,
        ),
    ]
}

/// The `!sheet` line of part `part` among `lines`.
fn sheet_part<'a>(lines: &'a [String], part: &str) -> Option<&'a str> {
    let start = format!("{SHEET_LINE} {part} ");
    lines
        .iter()
        .find(|line| line.starts_with(&start))
        .map(String::as_str)
}

/// The compared parts of a `!state` line `line` ([`SAVED_STATE_PARTS`]), as one record line.
fn saved_state(line: &str) -> Option<String> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let mut saved = String::from(RECORD_STATE);
    for part in SAVED_STATE_PARTS {
        let at = words.iter().position(|word| *word == part)?;
        let _ = write!(saved, " {part} {}", words.get(at + 1)?);
    }
    Some(saved)
}

/// The record lines of what the bot saw: the `!state` line's saved parts, the `!queststate`
/// lines of [`SAVED_QUEST`], the `!sheet` lines and the bag (each item index with its total,
/// by index); `None` until the reads answered.
fn record_lines(sight: &BotSight<'_>) -> Option<Vec<String>> {
    let state = sight
        .lines
        .iter()
        .find(|line| line.starts_with(STATE_LEVEL))
        .and_then(|line| saved_state(line))?;
    let quest: Vec<String> = sight
        .lines
        .iter()
        .filter(|line| {
            line.split_whitespace()
                .next()
                .is_some_and(|word| QUEST_ANSWER_WORDS.contains(&word))
        })
        .map(|line| format!("{RECORD_QUEST} {line}"))
        .collect();
    let sheet: Vec<String> = sight
        .lines
        .iter()
        .filter(|line| line.starts_with(SHEET_LINE) && line.as_str() != NOT_HERE)
        .cloned()
        .collect();
    sheet_part(sight.lines, SHEET_STORAGE)?;
    let mut bag: BTreeMap<u32, u32> = BTreeMap::new();
    for item in sight.bag.iter().filter(|item| item.quantity > 0) {
        *bag.entry(item.item_index).or_default() += item.quantity;
    }
    let mut lines = vec![state];
    lines.extend(quest);
    lines.extend(sheet);
    lines.extend(
        bag.into_iter()
            .map(|(index, quantity)| format!("{RECORD_BAG} {index}:{quantity}")),
    );
    Some(lines)
}

/// The acts that read the state, the saved quest and the sheet, then wait for their answers.
fn read_acts() -> Vec<Act> {
    vec![
        Act::Gm("!state".to_owned()),
        Act::Gm(format!("!queststate {SAVED_QUEST}")),
        Act::Gm("!sheet".to_owned()),
        Act::Pause(READ_PAUSE),
    ]
}

/// The differences between the record `saved` and what the server sends now (`now`), one line
/// each; empty when they agree.
pub fn record_differences(saved: &[String], now: &[String]) -> Vec<String> {
    let missing = saved
        .iter()
        .filter(|line| !now.contains(line))
        .map(|line| format!("saved, not sent now: {line}"));
    let added = now
        .iter()
        .filter(|line| !saved.contains(line))
        .map(|line| format!("sent now, not saved: {line}"));
    missing.chain(added).collect()
}

fn save_record(data: &SystemsData<'_>) -> Result<Vec<Step>, String> {
    let record = data
        .record
        .clone()
        .ok_or("the save record needs --record <file>")?;
    let weapon = item_index(data, GIVEN_WEAPON)?;
    let stack = item_index(data, GIVEN_STACK)?;
    let (pool, point) = weapon_point(data)?;
    let standing = territory_standing(data)?;
    let capital = settlement(data)?;
    let mut steps = vec![
        gm_done(format!("!give {GIVEN_WEAPON} 1")),
        gm_done(format!("!give {GIVEN_STACK} {STACK_GIVEN}")),
        step(
            format!("the bag holds the given {GIVEN_WEAPON} and {GIVEN_STACK}"),
            Vec::new(),
            Proof::Check(
                Box::new(move |sight| {
                    (bag_count(sight.bag, weapon) > 0 && bag_count(sight.bag, stack) >= STACK_GIVEN)
                        .then_some(Ok(()))
                }),
                format!("{GIVEN_WEAPON} and {STACK_GIVEN} {GIVEN_STACK} in the bag"),
            ),
            STEP_WAIT,
        ),
        gm_done(format!("!coins {SAVED_COINS}")),
        gm_done(format!("!quest {SAVED_QUEST}")),
        gm_done(format!("!masteryxp {standing} {STANDING_GIVEN}")),
    ];
    steps.extend(spend_steps(&pool, &point));
    steps.push(gm_done(format!("!masteryxp {pool} {MASTERY_GIVEN}")));
    steps.extend(open_storage_steps(capital));
    steps.push(step(
        format!("store {STACK_STORED} {GIVEN_STACK}"),
        vec![deposit(stack, STACK_STORED)],
        bag_shows(
            stack,
            |count| count == STACK_GIVEN - STACK_STORED,
            format!(
                "{} {GIVEN_STACK} left in the bag",
                STACK_GIVEN - STACK_STORED
            ),
        ),
        STEP_WAIT,
    ));
    steps.push(step(
        format!("write the record to {}", record.display()),
        read_acts(),
        Proof::Check(
            Box::new(move |sight| {
                let lines = record_lines(sight)?;
                Some(
                    std::fs::write(&record, lines.join("\n") + "\n")
                        .map_err(|error| format!("{}: {error}", record.display())),
                )
            }),
            "the state, quest, sheet and bag".to_owned(),
        ),
        STEP_WAIT,
    ));
    Ok(steps)
}

fn save_check(data: &SystemsData<'_>) -> Result<Vec<Step>, String> {
    let record = data
        .record
        .clone()
        .ok_or("the save check needs --record <file>")?;
    let saved: Vec<String> = std::fs::read_to_string(&record)
        .map_err(|error| format!("{}: {error}", record.display()))?
        .lines()
        .map(str::to_owned)
        .collect();
    Ok(vec![step(
        format!(
            "the server sends what the record {} holds ({} lines)",
            record.display(),
            saved.len()
        ),
        read_acts(),
        Proof::Check(
            Box::new(move |sight| {
                let now = record_lines(sight)?;
                let differences = record_differences(&saved, &now);
                Some(if differences.is_empty() {
                    Ok(())
                } else {
                    Err(differences.join("; "))
                })
            }),
            "the record's lines".to_owned(),
        ),
        STEP_WAIT,
    )])
}

/// The shop offer the economy check buys: the first (by `ConversionID`) that gives an item of
/// the catalog for coins and categorical points only (no item cost, no entitlement, no
/// achievement), with no cooldown, and one only.
fn shop_offer(data: &SystemsData<'_>) -> Result<(ShopOffer, String, u32, u32), String> {
    let kits = StarterKits::from_tables(data.activity.tables);
    ShopCatalog::from_tables(data.activity.tables)
        .offers()
        .filter(|offer| {
            offer.item_cost.is_none()
                && !offer.needs_entitlement
                && offer.achievement.is_none()
                && offer.cooldown_ms.is_none()
        })
        .filter_map(|offer| {
            let (item, quantity) = offer.item.clone()?;
            let index = kits.item(&item)?.item_index;
            Some((offer.clone(), item, index, quantity))
        })
        .min_by(|a, b| a.0.conversion_id.cmp(&b.0.conversion_id))
        .ok_or_else(|| "the shops sell no item for coins and points alone".to_owned())
}

fn economy(data: &SystemsData<'_>) -> Result<Vec<Step>, String> {
    let (offer, item, index, quantity) = shop_offer(data)?;
    let weapon = item_index(data, GIVEN_WEAPON)?;
    let capital = settlement(data)?;
    let mut steps = vec![gm_done(format!("!coins {COINS_GIVEN}"))];
    for cost in &offer.progression_costs {
        steps.push(gm_done(format!(
            "!masteryxp {} {}",
            cost.progression, cost.points
        )));
    }
    if let Some((progression, rank)) = &offer.required_rank {
        steps.push(gm_done(format!("!mastery {progression} {}", rank + 1)));
    }
    let before = Arc::new(Mutex::new(None));
    let noted = Arc::clone(&before);
    let conversion = lowercase_crc32(&offer.conversion_id);
    steps.push(step(
        format!(
            "buy {} at a shop ({quantity} {item} for {} hundredths of a coin and {} point costs)",
            offer.conversion_id,
            offer.coins,
            offer.progression_costs.len()
        ),
        vec![Act::Dynamic(Box::new(move |sight| {
            *noted.lock().unwrap_or_else(PoisonError::into_inner) =
                Some(bag_count(sight.bag, index));
            let call = ShopPurchaseCall {
                header: header(sight.own),
                conversion,
                count: 1,
            };
            Some(Ok(vec![(
                nw_messages::shop::PURCHASE_ITEM_TYPE_INDEX,
                call.encode(),
            )]))
        }))],
        Proof::Check(
            Box::new(move |sight| {
                let had = (*before.lock().unwrap_or_else(PoisonError::into_inner))?;
                (bag_count(sight.bag, index) >= had + quantity).then_some(Ok(()))
            }),
            format!("{quantity} more {item} in the bag"),
        ),
        STEP_WAIT,
    ));
    steps.push(gm_done(format!("!give {GIVEN_WEAPON} 1")));
    let kept = Arc::new(Mutex::new(None));
    let noted = Arc::clone(&kept);
    steps.push(step(
        format!("the bag holds the given {GIVEN_WEAPON}; note its instance"),
        Vec::new(),
        Proof::Check(
            Box::new(move |sight| {
                let (_, instance) = bag_stack(sight.bag, weapon)?;
                *noted.lock().unwrap_or_else(PoisonError::into_inner) = Some(instance);
                Some(Ok(()))
            }),
            format!("a {GIVEN_WEAPON} in the bag"),
        ),
        STEP_WAIT,
    ));
    steps.extend(open_storage_steps(capital));
    steps.push(step(
        format!("put the {GIVEN_WEAPON} into the storage"),
        vec![deposit(weapon, 0)],
        bag_shows(
            weapon,
            |count| count == 0,
            format!("no {GIVEN_WEAPON} in the bag"),
        ),
        STEP_WAIT,
    ));
    steps.push(step(
        format!("take the {GIVEN_WEAPON} back from the storage"),
        vec![Act::Dynamic(Box::new(move |sight| {
            let instance = (*kept.lock().unwrap_or_else(PoisonError::into_inner))?;
            let call = ItemCallMsg::TakeItem {
                header: header(sight.own),
                container: no_container(),
                instance,
                quantity: 0,
            };
            Some(call.encode().map(|body| vec![(call.type_index(), body)]))
        }))],
        bag_shows(
            weapon,
            |count| count == 1,
            format!("the {GIVEN_WEAPON} back in the bag"),
        ),
        STEP_WAIT,
    ));
    let salvaged_before = Arc::new(Mutex::new(None));
    let noted = Arc::clone(&salvaged_before);
    steps.push(step(
        format!("salvage the {GIVEN_WEAPON}"),
        vec![Act::Dynamic(Box::new(move |sight| {
            let (bag_slot, instance) = bag_stack(sight.bag, weapon)?;
            *noted.lock().unwrap_or_else(PoisonError::into_inner) =
                Some(other_items(sight.bag, weapon));
            let call = ClientFacetMsg::InitiateSalvage {
                header: header(sight.own),
                first: SALVAGE_FIRST,
                bag_slot,
                instance,
                unknown: SALVAGE_UNKNOWN,
                quantity: SALVAGE_ONE,
            };
            Some(call.encode().map(|body| vec![(call.type_index(), body)]))
        }))],
        Proof::Check(
            Box::new(move |sight| {
                let before = (*salvaged_before
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner))?;
                (bag_count(sight.bag, weapon) == 0 && other_items(sight.bag, weapon) > before)
                    .then_some(Ok(()))
            }),
            format!("no {GIVEN_WEAPON} and more salvage items in the bag"),
        ),
        LONG_WAIT,
    ));
    match Activity::of(ActivityKind::Craft, data.activity).steps {
        Ok(craft) => steps.extend(craft),
        Err(why) => return Err(format!("the craft: {why}")),
    }
    Ok(steps)
}

/// The total quantity of every bag item but `item_index`.
fn other_items(bag: &[nw_messages::ItemRecord], item_index: u32) -> u32 {
    bag.iter()
        .filter(|item| item.item_index != item_index)
        .map(|item| item.quantity)
        .sum()
}

/// The step that goes to settlement `capital` and opens its trading post.
fn open_trading_post(capital: &Capital) -> Step {
    step(
        format!(
            "go to the settlement of {} and open its trading post",
            capital.slice_name
        ),
        vec![
            teleport(capital.position),
            Act::Interact {
                request: InteractionRequest {
                    target: [0; RAW_ID_LEN],
                    target_entity: 0,
                    interaction: 0,
                    kind: TRADING_POST_KIND,
                    gatherable: None,
                },
                name: TRADING_POST_NAME.to_vec(),
            },
        ],
        Proof::Accepted,
        STEP_WAIT,
    )
}

/// A request id of the bot `own`'s call `salt`: unique per bot and call.
fn request_id(own: u64, salt: u64) -> [u8; RAW_ID_LEN] {
    let mut id = [0; RAW_ID_LEN];
    id[..size_of::<u64>()].copy_from_slice(&own.to_be_bytes());
    id[size_of::<u64>()..].copy_from_slice(&salt.to_be_bytes());
    id
}

/// The request salts of the trade calls.
const CREATE_SALT: u64 = 1;
const COMPLETE_SALT: u64 = 2;

/// The contract id of a 634 among `messages`.
fn created_contract(messages: &[Received]) -> Option<[u8; RAW_ID_LEN]> {
    messages.iter().find_map(|received| {
        let index = received.envelope.type_index()?;
        if index != CONTRACT_CREATED_TYPE_INDEX {
            return None;
        }
        match ContractMessage::decode(index, &received.envelope.body)? {
            Ok(ContractMessage::Notice(notice)) => Some(notice.contract.id),
            _ => None,
        }
    })
}

/// The coins of the newest `!state` line among `lines`.
fn state_coins(lines: &[String]) -> Option<u64> {
    let line = lines
        .iter()
        .rev()
        .find(|line| line.starts_with(STATE_LEVEL))?;
    let words: Vec<&str> = line.split_whitespace().collect();
    let at = words.iter().position(|word| *word == "coins")?;
    words.get(at + 1)?.parse().ok()
}

fn trade(seat: &GroupSeat, data: &SystemsData<'_>) -> Result<Vec<Step>, String> {
    let stack = item_index(data, GIVEN_STACK)?;
    let capital = settlement(data)?;
    let board = Arc::clone(&data.board);
    let mut steps = vec![
        gm_done(format!("!coins {COINS_GIVEN}")),
        open_trading_post(capital),
    ];
    if seat.leads() {
        steps.push(gm_done(format!("!give {GIVEN_STACK} {STACK_GIVEN}")));
        steps.push(step(
            "the bag holds the stack to sell",
            Vec::new(),
            bag_shows(
                stack,
                |count| count >= STACK_GIVEN,
                format!("{STACK_GIVEN} {GIVEN_STACK}"),
            ),
            STEP_WAIT,
        ));
        let posted = Arc::clone(&board);
        steps.push(step(
            format!("post a sell order of {STACK_SOLD} {GIVEN_STACK}"),
            vec![call(|own| {
                let call = ContractCall::Create {
                    header: FacetHeader::of(CONTRACTS_COMPONENT, own),
                    order: CreateOrder {
                        kind: ContractKind::Sell.value(),
                        price: UNIT_PRICE,
                        count: STACK_SOLD,
                        days: POSTING_DAYS,
                        item_crc: lowercase_crc32(GIVEN_STACK),
                        item_block: [0; CREATE_ITEM_BLOCK_LEN],
                    },
                    request: request_id(own, CREATE_SALT),
                    tail: RETAIL_REQUEST_TAIL,
                };
                Ok((call.type_index(), call.encode()?))
            })],
            Proof::Check(
                Box::new(move |sight| {
                    let contract = created_contract(sight.messages)?;
                    posted.post(KEY_CONTRACT, HEXLOWER.encode(&contract));
                    Some(Ok(()))
                }),
                "the created contract (634)".to_owned(),
            ),
            STEP_WAIT,
        ));
        let coins = Arc::new(Mutex::new(None));
        let noted = Arc::clone(&coins);
        steps.push(step(
            "note the coins after the posting",
            vec![Act::Gm("!state".to_owned()), Act::Pause(READ_PAUSE)],
            Proof::Check(
                Box::new(move |sight| {
                    let now = state_coins(sight.lines)?;
                    *noted.lock().unwrap_or_else(PoisonError::into_inner) = Some(now);
                    Some(Ok(()))
                }),
                "a state line".to_owned(),
            ),
            STEP_WAIT,
        ));
        let bought = Arc::clone(&board);
        steps.push(step(
            "wait for the other bot's buy",
            Vec::new(),
            Proof::Check(
                Box::new(move |_| bought.get(KEY_BOUGHT).map(|_| Ok(()))),
                "the other bot's buy".to_owned(),
            ),
            OTHER_BOT_WAIT,
        ));
        steps.push(step(
            "the sale's coins arrived",
            vec![
                Act::Pause(LONG_WAIT),
                Act::Gm("!state".to_owned()),
                Act::Pause(READ_PAUSE),
            ],
            Proof::Check(
                Box::new(move |sight| {
                    let before = (*coins.lock().unwrap_or_else(PoisonError::into_inner))?;
                    let now = state_coins(sight.lines)?;
                    Some(if now > before {
                        Ok(())
                    } else {
                        Err(format!("coins {now}, {before} before the sale"))
                    })
                }),
                "more coins than before the sale".to_owned(),
            ),
            STEP_WAIT,
        ));
    } else {
        let contract = Arc::clone(&board);
        steps.push(step(
            format!("buy the other bot's {STACK_SOLD} {GIVEN_STACK}"),
            vec![Act::Dynamic(Box::new(move |sight| {
                let id = HEXLOWER
                    .decode(contract.get(KEY_CONTRACT)?.as_bytes())
                    .ok()?;
                let contract: [u8; RAW_ID_LEN] = id.try_into().ok()?;
                let call = ContractCall::Complete {
                    header: FacetHeader::of(CONTRACTS_COMPONENT, sight.own),
                    contract,
                    request: request_id(sight.own, COMPLETE_SALT),
                    kind: ContractKind::Sell.value(),
                    count: STACK_SOLD,
                    reason: reason::SUCCEEDED,
                    item: ShortItemDescriptor {
                        item_crc: lowercase_crc32(GIVEN_STACK),
                        quantity: STACK_SOLD,
                        gear_score: 0,
                        perks: Vec::new(),
                    },
                    tail: RETAIL_REQUEST_TAIL,
                };
                Some(call.encode().map(|body| vec![(call.type_index(), body)]))
            }))],
            bag_shows(
                stack,
                |count| count >= STACK_SOLD,
                format!("{STACK_SOLD} {GIVEN_STACK} in the bag"),
            ),
            OTHER_BOT_WAIT,
        ));
        let bought = Arc::clone(&board);
        steps.push(step(
            "tell the seller",
            vec![Act::Dynamic(Box::new(move |_| {
                bought.post(KEY_BOUGHT, String::new());
                Some(Ok(Vec::new()))
            }))],
            Proof::Sent,
            Duration::ZERO,
        ));
    }
    Ok(steps)
}

/// A step that waits until the other bot noted `key`.
fn wait_for(board: &Arc<SystemsBoard>, key: &'static str, what: &str) -> Step {
    let board = Arc::clone(board);
    step(
        format!("wait for the other bot: {what}"),
        Vec::new(),
        Proof::Check(
            Box::new(move |_| board.get(key).map(|_| Ok(()))),
            format!("the other bot's {what}"),
        ),
        OTHER_BOT_WAIT,
    )
}

/// An act that notes `key` (with `value` of what the bot saw) for the other bot.
fn tell(board: &Arc<SystemsBoard>, key: &'static str, value: fn(&BotSight<'_>) -> String) -> Act {
    let board = Arc::clone(board);
    Act::Dynamic(Box::new(move |sight| {
        board.post(key, value(sight));
        Some(Ok(Vec::new()))
    }))
}

/// A social call of the bot on facet `facet`, written with the character of the bot of seat
/// `other`.
fn social_call(
    seat: &GroupSeat,
    other: usize,
    facet: SocialFacet,
    make: impl Fn(String) -> SocialCallBody + 'static,
) -> Act {
    let seat = seat.clone();
    Act::Dynamic(Box::new(move |sight| {
        let other = seat_character(&seat, other)?;
        Some(
            make(other)
                .encode(facet.header(sight.own))
                .map(|call| vec![call]),
        )
    }))
}

/// The social calls the checks send.
enum SocialCallBody {
    Player(PlayerCall),
    Guild(GuildCall),
}

impl SocialCallBody {
    fn encode(&self, header: FacetHeader) -> Result<(u64, Vec<u8>), MessageError> {
        match self {
            Self::Player(call) => Ok((call.type_index(), call.encode(header)?)),
            Self::Guild(call) => Ok((call.type_index(), call.encode(header)?)),
        }
    }
}

/// Whether `messages` hold a push of the social service that `wanted` takes.
fn social_push(messages: &[Received], wanted: impl Fn(&SocialPush) -> bool) -> bool {
    messages.iter().any(|received| {
        received
            .envelope
            .type_index()
            .and_then(|index| SocialPush::decode(index, &received.envelope.body))
            .and_then(Result::ok)
            .is_some_and(|(_, push)| wanted(&push))
    })
}

/// The company id of a 5429 among `messages` that names one.
fn company_of(messages: &[Received]) -> Option<[u8; RAW_ID_LEN]> {
    messages.iter().find_map(|received| {
        let index = received.envelope.type_index()?;
        match SocialPush::decode(index, &received.envelope.body)?.ok()? {
            (_, SocialPush::Guild(GuildPush::DataUpdated { guild }))
                if guild != [0; RAW_ID_LEN] =>
            {
                Some(guild)
            }
            _ => None,
        }
    })
}

fn social(seat: &GroupSeat, data: &SystemsData<'_>) -> Result<Vec<Step>, String> {
    let capital = settlement(data)?;
    let board = &data.board;
    let mark = format!("{CHAT_MARK} {}", data.first_account);
    let company = format!("{}{COMPANY_NAME_END}", capitalized(&data.first_account));
    let mut steps = vec![
        step(
            "go to the settlement".to_owned(),
            vec![teleport(capital.position)],
            Proof::Sent,
            Duration::ZERO,
        ),
        faction_step(COMPANY_FACTION),
    ];
    if seat.leads() {
        steps.push(wait_for(board, KEY_CHAT, "arrival for the chat"));
        steps.push(step(
            format!("say \"{mark}\" in area chat"),
            vec![call(move |own| {
                let call = ChatCall::Send {
                    channel: ChatChannel::Area.wire(),
                    body: mark.clone(),
                    recipient_name: String::new(),
                    links: Vec::new(),
                };
                Ok((
                    call.type_index(),
                    call.encode(SocialFacet::Chat.header(own))?,
                ))
            })],
            Proof::Sent,
            Duration::ZERO,
        ));
        steps.push(step(
            "send the other bot a friend invite".to_owned(),
            vec![
                social_call(seat, other_seat(seat), SocialFacet::Social, |character| {
                    SocialCallBody::Player(PlayerCall::Friend {
                        request: FRIEND_INVITE,
                        character,
                    })
                }),
                tell(board, KEY_FRIEND, |_| String::new()),
            ],
            Proof::Message(ONLINE_STATE_CHANGE_TYPE_INDEX),
            OTHER_BOT_WAIT,
        ));
        steps.push(step(
            "invite the other bot into a group".to_owned(),
            vec![Act::InviteGroup(seat.clone())],
            Proof::GroupFull(seat.clone()),
            OTHER_BOT_WAIT,
        ));
        steps.push(wait_for(board, KEY_GROUP_LEFT, "leave of the group"));
        steps.push(company_create_step(board, KEY_GUILD, company));
        steps.push(company_invite_step(
            seat,
            other_seat(seat),
            board,
            KEY_COMPANY_INVITED,
        ));
    } else {
        let heard = mark.clone();
        steps.push(step(
            "hear the other bot's area chat line".to_owned(),
            vec![tell(board, KEY_CHAT, |_| String::new())],
            Proof::Check(
                Box::new(move |sight| sight.heard.contains(&heard).then_some(Ok(()))),
                format!("the line \"{mark}\""),
            ),
            OTHER_BOT_WAIT,
        ));
        steps.push(wait_for(board, KEY_FRIEND, "friend invite"));
        steps.push(step(
            "accept the friend invite".to_owned(),
            vec![social_call(
                seat,
                other_seat(seat),
                SocialFacet::Social,
                |character| {
                    SocialCallBody::Player(PlayerCall::Friend {
                        request: FRIEND_ACCEPT,
                        character,
                    })
                },
            )],
            Proof::Message(ONLINE_STATE_CHANGE_TYPE_INDEX),
            STEP_WAIT,
        ));
        steps.push(step(
            "accept the group invite".to_owned(),
            vec![Act::AcceptGroupInvite(seat.clone())],
            Proof::InGroup(seat.clone()),
            OTHER_BOT_WAIT,
        ));
        steps.push(step(
            "leave the group".to_owned(),
            vec![
                call(|own| {
                    let call = GroupCall::Leave { value: LEAVE_VALUE };
                    Ok((
                        call.type_index(),
                        call.encode(SocialFacet::Groups.header(own))?,
                    ))
                }),
                tell(board, KEY_GROUP_LEFT, |_| String::new()),
            ],
            Proof::Check(
                Box::new(|sight| (!sight.in_group).then_some(Ok(()))),
                "no group in the own groups state".to_owned(),
            ),
            STEP_WAIT,
        ));
        steps.extend(company_join_steps(board, KEY_COMPANY_INVITED, KEY_GUILD));
    }
    Ok(steps)
}

/// The step that makes the company `name` (3611) and notes its id under `guild_key`.
fn company_create_step(board: &Arc<SystemsBoard>, guild_key: &'static str, name: String) -> Step {
    let posted = Arc::clone(board);
    step(
        format!("make the company {name}"),
        vec![call(move |own| {
            let call = GuildCall::Create {
                name: name.clone(),
                crest: GuildCrest::NONE,
            };
            Ok((
                call.type_index(),
                call.encode(SocialFacet::Guilds.header(own))?,
            ))
        })],
        Proof::Check(
            Box::new(move |sight| {
                let guild = company_of(sight.messages)?;
                posted.post(guild_key, HEXLOWER.encode(&guild));
                Some(Ok(()))
            }),
            "the new company's id (5429)".to_owned(),
        ),
        STEP_WAIT,
    )
}

/// The step that invites the bot of seat `member` into the bot's company (3612) and notes it
/// under `invited_key`.
fn company_invite_step(
    seat: &GroupSeat,
    member: usize,
    board: &Arc<SystemsBoard>,
    invited_key: &'static str,
) -> Step {
    step(
        format!("invite the bot of seat {member} into the company"),
        vec![
            social_call(seat, member, SocialFacet::Guilds, |character| {
                SocialCallBody::Guild(GuildCall::Invite { character })
            }),
            tell(board, invited_key, |_| String::new()),
        ],
        Proof::Sent,
        Duration::ZERO,
    )
}

/// The steps that wait for the invite under `invited_key` and join the company whose id is
/// under `guild_key` (118), proved by 3247 accepted and the company's 5429.
fn company_join_steps(
    board: &Arc<SystemsBoard>,
    invited_key: &'static str,
    guild_key: &'static str,
) -> Vec<Step> {
    let guild = Arc::clone(board);
    let joined = Arc::clone(board);
    vec![
        wait_for(board, invited_key, "company invite"),
        step(
            "accept the company invite".to_owned(),
            vec![Act::Dynamic(Box::new(move |sight| {
                let id = HEXLOWER.decode(guild.get(guild_key)?.as_bytes()).ok()?;
                let guild: [u8; RAW_ID_LEN] = id.try_into().ok()?;
                let call = GuildCall::RespondToInvite {
                    guild,
                    accept: true,
                };
                Some(
                    call.encode(SocialFacet::Guilds.header(sight.own))
                        .map(|body| vec![(call.type_index(), body)]),
                )
            }))],
            Proof::Check(
                Box::new(move |sight| {
                    let wanted = joined.get(guild_key)?;
                    let accepted = social_push(sight.messages, |push| {
                        matches!(
                            push,
                            SocialPush::Guild(GuildPush::InviteResponded { accepted: true })
                        )
                    });
                    let member = company_of(sight.messages)
                        .is_some_and(|guild| HEXLOWER.encode(&guild) == wanted);
                    (accepted && member).then_some(Ok(()))
                }),
                "the accepted answer (3247) and the company's id (5429)".to_owned(),
            ),
            STEP_WAIT,
        ),
    ]
}

/// The bots of the war check: a war leader and a member on each side.
const WAR_BOTS: usize = 4;
/// The most bots of the invasion check (Inferred: enough to clear a wave of the fort with its
/// Brute, 563,000 health in the retail `invasion` ledger).
const INVASION_MOST_BOTS: usize = 10;
/// The seats of the war check: the defenders' leader and member, the attackers' leader and
/// member.
const DEFENDER_LEADER: usize = 0;
const DEFENDER_MEMBER: usize = 1;
const ATTACKER_LEADER: usize = 2;
const ATTACKER_MEMBER: usize = 3;
/// The ends of the made-up company names after the first bot's account name: one company per
/// run, so its governor is the run's war leader (a bot stays in its first company).
const FORT_COMPANY_END: &str = " Fort";
const SIEGE_COMPANY_END: &str = " Siege";
/// The board keys of the war and invasion checks.
const KEY_FORT_GUILD: &str = "fort guild";
const KEY_SIEGE_GUILD: &str = "siege guild";
const KEY_FORT_INVITED: &str = "fort invited";
const KEY_SIEGE_INVITED: &str = "siege invited";
const KEY_FORT_JOINED: &str = "fort joined";
const KEY_SIEGE_JOINED: &str = "siege joined";
const KEY_OWNED: &str = "owned";
const KEY_DECLARED: &str = "declared";
const KEY_FORT_SIGNED: &str = "fort member signed";
const KEY_SIEGE_SIGNED: &str = "siege member signed";

/// One side of the war check: its seats, faction, company and board keys.
struct WarSide {
    side: u32,
    leader: usize,
    member: usize,
    faction: &'static str,
    company_end: &'static str,
    guild: &'static str,
    invited: &'static str,
    joined: &'static str,
    signed: &'static str,
}

/// The two sides of the war check.
const FORT_SIDE: WarSide = WarSide {
    side: SIDE_DEFENDER,
    leader: DEFENDER_LEADER,
    member: DEFENDER_MEMBER,
    faction: BOT_FACTION,
    company_end: FORT_COMPANY_END,
    guild: KEY_FORT_GUILD,
    invited: KEY_FORT_INVITED,
    joined: KEY_FORT_JOINED,
    signed: KEY_FORT_SIGNED,
};
const SIEGE_SIDE: WarSide = WarSide {
    side: SIDE_ATTACKER,
    leader: ATTACKER_LEADER,
    member: ATTACKER_MEMBER,
    faction: ENEMY_FACTION,
    company_end: SIEGE_COMPANY_END,
    guild: KEY_SIEGE_GUILD,
    invited: KEY_SIEGE_INVITED,
    joined: KEY_SIEGE_JOINED,
    signed: KEY_SIEGE_SIGNED,
};

/// The company steps of the bot of `seat` on side `side`: the leader makes the side's company
/// and invites the member; the member joins and tells the leader.
fn side_company_steps(seat: &GroupSeat, side: &WarSide, data: &SystemsData<'_>) -> Vec<Step> {
    let board = &data.board;
    let mut steps = vec![faction_step(side.faction)];
    if seat.number() == side.leader {
        steps.push(company_create_step(
            board,
            side.guild,
            company_name(side, data),
        ));
        steps.push(company_invite_step(seat, side.member, board, side.invited));
        steps.push(wait_for(board, side.joined, "join of the company"));
    } else {
        steps.extend(company_join_steps(board, side.invited, side.guild));
        steps.push(step(
            "tell the leader",
            vec![tell(board, side.joined, |_| String::new())],
            Proof::Sent,
            Duration::ZERO,
        ));
    }
    steps
}

/// The company name of side `side` in the run of `data`.
fn company_name(side: &WarSide, data: &SystemsData<'_>) -> String {
    format!("{}{}", capitalized(&data.first_account), side.company_end)
}

/// The war leader's roster fill (2777), once its member signed up (noted under `signed`).
fn fill_roster_step(board: &Arc<SystemsBoard>, signed: &'static str) -> Vec<Step> {
    vec![
        wait_for(board, signed, "sign-up"),
        step(
            "fill the roster as the war leader (2777)",
            vec![
                Act::Pause(FILL_WAIT),
                war_call(|header| WarSignupClientMsg::FillRoster { header }),
            ],
            Proof::Message(nw_messages::war_signup::SET_ROSTER_RESPONSE_RECEIVED_TYPE_INDEX),
            STEP_WAIT,
        ),
    ]
}

/// The steps of a side member's sign-up: the war board from afar and the sign-up, then a note
/// for its leader under `signed`.
fn signup_steps(
    board: &Arc<SystemsBoard>,
    territory: u16,
    side: u32,
    signed: Option<&'static str>,
) -> Vec<Step> {
    let what = if side == SIDE_ATTACKER {
        "sign up as an attacker (2762)"
    } else {
        "sign up as a defender (2762)"
    };
    let mut steps: Vec<Step> = war_board_signup(territory, side, what).into();
    if let Some(signed) = signed {
        steps.push(step(
            "tell the leader",
            vec![tell(board, signed, |_| String::new())],
            Proof::Sent,
            Duration::ZERO,
        ));
    }
    steps
}

fn war(seat: &GroupSeat, data: &SystemsData<'_>) -> Result<Vec<Step>, String> {
    let territory = WAR_FIGHT_TERRITORY;
    let points = siege_points(data.activity, territory)?;
    let board = &data.board;
    let side = if seat.number() < ATTACKER_LEADER {
        FORT_SIDE
    } else {
        SIEGE_SIDE
    };
    let leads = seat.number() == side.leader;
    let mut steps = side_company_steps(seat, &side, data);
    if leads && side.side == SIDE_DEFENDER {
        steps.push(gm_done(format!(
            "!owner {territory} {}",
            company_name(&FORT_SIDE, data)
        )));
        steps.push(step(
            "tell the attackers",
            vec![tell(board, KEY_OWNED, |_| String::new())],
            Proof::Sent,
            Duration::ZERO,
        ));
    }
    if leads && side.side == SIDE_ATTACKER {
        steps.push(wait_for(board, KEY_OWNED, "ownership of the fort"));
        steps.push(gm_any(format!(
            "!war {territory} {WAR_FIGHT_START_MINUTES} {}",
            company_name(&SIEGE_SIDE, data)
        )));
        steps.push(step(
            "tell the defenders",
            vec![tell(board, KEY_DECLARED, |_| String::new())],
            Proof::Sent,
            Duration::ZERO,
        ));
    } else {
        steps.push(wait_for(board, KEY_DECLARED, "declaration of the war"));
    }
    if leads {
        steps.extend(signup_steps(board, territory, side.side, None));
        steps.extend(fill_roster_step(board, side.signed));
    } else {
        steps.extend(signup_steps(board, territory, side.side, Some(side.signed)));
    }
    steps.push(accept_raid_invite());
    let goal = if side.side == SIDE_ATTACKER {
        MatchGoal::Siege(points)
    } else {
        MatchGoal::Siege(Vec::new())
    };
    steps.extend(war_play_steps(goal, WAR_LONGEST));
    if seat.number() == DEFENDER_LEADER {
        steps.push(gm_any(format!("!war {territory} cancel")));
        steps.push(gm_any(format!("!owner {territory} clear")));
    }
    Ok(steps)
}

fn invasion(seat: &GroupSeat, data: &SystemsData<'_>) -> Result<Vec<Step>, String> {
    let territory = INVASION_FIGHT_TERRITORY;
    let claim = sim::world::claim_position(territory)
        .ok_or_else(|| format!("territory {territory} has no claim position"))?;
    let board = &data.board;
    let mut steps = if seat.number() <= DEFENDER_MEMBER {
        side_company_steps(seat, &FORT_SIDE, data)
    } else {
        // The other defenders sign up by their faction, as players outside the company do.
        vec![faction_step(FORT_SIDE.faction)]
    };
    if seat.leads() {
        steps.push(gm_done(format!(
            "!owner {territory} {}",
            company_name(&FORT_SIDE, data)
        )));
        steps.push(gm_any(format!(
            "!invasion {territory} {INVASION_FIGHT_START_MINUTES}"
        )));
        steps.push(step(
            "tell the member",
            vec![tell(board, KEY_DECLARED, |_| String::new())],
            Proof::Sent,
            Duration::ZERO,
        ));
    } else {
        steps.push(wait_for(board, KEY_DECLARED, "invasion schedule"));
    }
    steps.extend(signup_steps(board, territory, SIDE_DEFENDER, None));
    steps.push(accept_raid_invite());
    // The leader holds the claim; the others clear each wave of its mobs, so the next one
    // starts.
    let goal = if seat.leads() {
        MatchGoal::Defend(claim)
    } else {
        MatchGoal::Clear {
            claim,
            vitals: data.wave_vitals.clone(),
        }
    };
    steps.extend(war_play_steps(goal, INVASION_LONGEST));
    if seat.leads() {
        steps.push(gm_any(format!("!war {territory} cancel")));
        steps.push(gm_any(format!("!owner {territory} clear")));
    }
    Ok(steps)
}

/// `word` with its first letter in capitals.
fn capitalized(word: &str) -> String {
    let mut letters = word.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

fn progress(seat: &GroupSeat, data: &SystemsData<'_>) -> Result<Vec<Step>, String> {
    let capital = settlement(data)?;
    let board = &data.board;
    let mut steps = vec![step(
        "go to the settlement".to_owned(),
        vec![teleport(capital.position)],
        Proof::Sent,
        Duration::ZERO,
    )];
    if seat.leads() {
        let (pool, point) = weapon_point(data)?;
        steps.extend(
            Activity::of(ActivityKind::Level, data.activity)
                .steps
                .map_err(|why| format!("the level: {why}"))?,
        );
        steps.extend(spend_steps(&pool, &point));
        steps.push(gm_done("!god off".to_owned()));
        let down = Arc::clone(board);
        steps.push(step(
            "go down (death's door) and tell the other bot",
            vec![Act::Gm("!kill".to_owned())],
            // The other bot revives only once this bot saw itself down: a revive before that
            // left it alive before its proof (live 2026-11-23 03:10:50).
            Proof::Check(
                Box::new(move |sight| {
                    sight.dead.then(|| {
                        down.post(KEY_DEAD, sight.own.to_string());
                        Ok(())
                    })
                }),
                "no health in the own vitals".to_owned(),
            ),
            STEP_WAIT,
        ));
        steps.push(step(
            "live again by the other bot's revive",
            Vec::new(),
            Proof::Check(
                Box::new(|sight| (!sight.dead).then_some(Ok(()))),
                "alive own vitals".to_owned(),
            ),
            OTHER_BOT_WAIT,
        ));
        steps.push(wait_for(board, KEY_REVIVED, "revive answer"));
    } else {
        let dead = Arc::clone(board);
        steps.push(wait_for(board, KEY_DEAD, "death"));
        steps.push(step(
            "revive the other bot".to_owned(),
            vec![Act::InteractDynamic(Box::new(move |_| {
                let target: u64 = dead.get(KEY_DEAD)?.parse().ok()?;
                Some(InteractionRequest {
                    target: raw_id_of_entity(target),
                    target_entity: target,
                    interaction: 0,
                    kind: REVIVE_PLAYER_KIND,
                    gatherable: None,
                })
            }))],
            Proof::Accepted,
            STEP_WAIT,
        ));
        steps.push(step(
            "tell the other bot",
            vec![tell(board, KEY_REVIVED, |_| String::new())],
            Proof::Sent,
            Duration::ZERO,
        ));
    }
    Ok(steps)
}

/// How far from where a hunt round starts the bot fights mobs, metres.
const HUNT_RADIUS_M: f32 = 120.0;
/// How long one hunt round lasts before the bot reads its level.
const HUNT_ROUND: Duration = Duration::from_secs(60);
/// How many hunt rounds the kill experience check plays at most.
const HUNT_ROUNDS: usize = 12;

/// The level of the newest `!state` line among `lines` (as the client shows it).
fn state_level(lines: &[String]) -> Option<u32> {
    let line = lines
        .iter()
        .rev()
        .find(|line| line.starts_with(STATE_LEVEL))?;
    line.strip_prefix(STATE_LEVEL)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// A respawn request (1938) when the bot is dead, else nothing: a hunt bot with no god mode dies
/// to the mobs it fights.
fn respawn_if_dead() -> Act {
    Act::Dynamic(Box::new(|sight| {
        if !sight.dead {
            return Some(Ok(Vec::new()));
        }
        Some(
            nw_messages::RequestRespawnMsg::default()
                .encode()
                .map(|body| vec![(nw_messages::REQUEST_RESPAWN_TYPE_INDEX, body)]),
        )
    }))
}

/// The kill experience check: the bot notes its level, hunts the mobs around it in rounds of
/// [`HUNT_ROUND`] (reading its level after each) and proves that its level rose. The game
/// master lines only read; no line gives experience.
fn kill_xp() -> Vec<Step> {
    let start = Arc::new(Mutex::new(None));
    let leveled = Arc::new(Mutex::new(false));
    let noted = Arc::clone(&start);
    let mut steps = vec![step(
        "note the level before the hunt",
        vec![Act::Gm("!state".to_owned()), Act::Pause(READ_PAUSE)],
        Proof::Check(
            Box::new(move |sight| {
                let level = state_level(sight.lines)?;
                *noted.lock().unwrap_or_else(PoisonError::into_inner) = Some(level);
                Some(Ok(()))
            }),
            "a state line".to_owned(),
        ),
        STEP_WAIT,
    )];
    for round in 1..=HUNT_ROUNDS {
        let start = Arc::clone(&start);
        let leveled = Arc::clone(&leveled);
        steps.push(step(
            format!("hunt round {round} of {HUNT_ROUNDS}"),
            vec![
                respawn_if_dead(),
                Act::Hunt(Some(HUNT_RADIUS_M)),
                Act::Pause(HUNT_ROUND),
                Act::Gm("!state".to_owned()),
                Act::Pause(READ_PAUSE),
            ],
            Proof::Check(
                Box::new(move |sight| {
                    let level = state_level(sight.lines)?;
                    let before = (*start.lock().unwrap_or_else(PoisonError::into_inner))?;
                    if level > before {
                        *leveled.lock().unwrap_or_else(PoisonError::into_inner) = true;
                    }
                    Some(Ok(()))
                }),
                "a state line".to_owned(),
            ),
            HUNT_ROUND,
        ));
    }
    steps.push(step(
        "end the hunt: the level rose by kill experience",
        vec![Act::Hunt(None)],
        Proof::Check(
            Box::new(move |_| {
                Some(if *leveled.lock().unwrap_or_else(PoisonError::into_inner) {
                    Ok(())
                } else {
                    Err(format!("no level up in {HUNT_ROUNDS} rounds"))
                })
            }),
            "a level above the start".to_owned(),
        ),
        STEP_WAIT,
    ));
    steps
}

#[cfg(test)]
mod tests {
    use nw_messages::social::players::FriendRequest;

    use super::*;

    #[test]
    fn the_friend_values_are_the_clients() {
        assert_eq!(
            FriendRequest::from_wire(FRIEND_INVITE),
            Some(FriendRequest::Invite)
        );
        assert_eq!(
            FriendRequest::from_wire(FRIEND_ACCEPT),
            Some(FriendRequest::Accept)
        );
    }

    #[test]
    fn a_state_line_keeps_its_saved_parts() {
        let line =
            "state level 65 xp 100 coins 5 faction 2 pvp on inn none mount riding quests 1 done 3";
        assert_eq!(
            saved_state(line).as_deref(),
            Some("state level 65 xp 100 coins 5 faction 2 inn none quests 1 done 3")
        );
        assert_eq!(saved_state("state level 65"), None);
        assert_eq!(state_coins(&[line.to_owned()]), Some(5));
        assert_eq!(state_level(&[line.to_owned()]), Some(65));
    }

    #[test]
    fn a_record_differs_by_each_line_missing_or_added() {
        let saved = ["a".to_owned(), "b".to_owned()];
        let now = ["b".to_owned(), "c".to_owned()];
        assert_eq!(
            record_differences(&saved, &now),
            ["saved, not sent now: a", "sent now, not saved: c"]
        );
        assert!(record_differences(&saved, &saved).is_empty());
    }

    #[test]
    fn the_board_keeps_the_newest_value() {
        let board = SystemsBoard::default();
        assert_eq!(board.get(KEY_GUILD), None);
        board.post(KEY_GUILD, "one".to_owned());
        board.post(KEY_GUILD, "two".to_owned());
        assert_eq!(board.get(KEY_GUILD).as_deref(), Some("two"));
    }

    #[test]
    fn a_request_id_names_the_bot_and_the_call() {
        assert_ne!(request_id(1, CREATE_SALT), request_id(1, COMPLETE_SALT));
        assert_ne!(request_id(1, CREATE_SALT), request_id(2, CREATE_SALT));
        assert_eq!(capitalized("qbsys"), "Qbsys");
    }

    #[test]
    fn the_checks_take_their_bots_and_only_the_save_check_skips_the_setup() {
        assert_eq!(SystemsCheck::Trade.bots(), 2..=2);
        assert_eq!(SystemsCheck::SaveRecord.bots(), 1..=1);
        assert!(SystemsCheck::Invasion.bots().contains(&INVASION_MOST_BOTS));
        assert!(!SystemsCheck::SaveCheck.game_master_setup());
        assert!(SystemsCheck::Economy.game_master_setup());
    }
}
