// SPDX-License-Identifier: AGPL-3.0-or-later
//! `nw-testclient arena`: bots of game master accounts play PvP as players do, and the run writes
//! what each match was: the 3v3 arena (six bots, the world director's `[instances.arena]
//! match_size` 6), the 1v1 arena (two bots, `match_size` 2) and a duel (`--duel`, two bots).
//!
//! The arena (arena flow study 2026-11-11, `local-data/findings/2026-11-11-arena-3v3-flow.md`):
//! each bot queues alone as the client does ([`crate::activity::ActivityKind::Arena`]: 6211, 6254
//! answered with 6295, two 6272, the pop, 5741 with 946 answered with 5449, 5450 and the move),
//! readies up (6824) in the pregame and between rounds, fights the participants of the other team
//! in the rounds (script states 3 and 5) and leaves with the end screen's button (1980 `Leave`)
//! once the match ended (script state 2).
//!
//! The duel (game mode study 2026-11-11, the sim's `duel` module text): the second bot stands
//! next to the first (a game master's teleport), the first invites it (3404 with the activity
//! `duel`), the second accepts the invite its own invite state shows (3406), both fight after
//! the countdown (script state 3) until one goes down or the fight timer runs out (script state
//! 0), and the server takes both out of the mode 8 s later.
//!
//! The bots set up as game masters (level 65, gear score 800) before they queue or meet; the PvP
//! part is the client's calls alone. Each bot's seat ([`ArenaSeat`]) keeps what its views saw of
//! each match: the match id, its team, the game mode entity's state (2343: the script states,
//! the participants' teams and statuses, the values with the round wins and the winning team),
//! the deaths (4977), its hits dealt and taken (2071, 3601) and its return. [`run_markdown`]
//! writes the report.

use std::fmt::Write as _;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use nw_formats::objectstream::lowercase_crc32;
use nw_messages::game_mode::character_text;
use nw_messages::generic_invite::{
    GENERIC_INVITE_COMPONENT, INVITE_FAILED_TYPE_INDEX, InviteCall, InviteFailedMsg,
};
use nw_messages::{DeathRecapForAllMsg, FacetHeader, MessageError};

use crate::activity::{
    Act, BotSight, MatchEnd, MatchGoal, Proof, Step, gm_done, matchmaking_shows, step,
};
use crate::play::{PlayReport, Verdict};
use crate::view::{GameModeView, WorldView};

/// The arena's script states (Observed, arena flow study section 3.2): the pregame, between
/// rounds, the end, a round and its overtime.
pub const PREGAME_STATE: u8 = 0;
/// Between two rounds.
pub const BETWEEN_ROUNDS_STATE: u8 = 1;
/// The match ended.
pub const ARENA_ENDED_STATE: u8 = 2;
/// A round's fight.
pub const ROUND_STATE: u8 = 3;
/// A round's overtime.
pub const OVERTIME_STATE: u8 = 5;
/// The duel's script states (Observed, the sim's `duel` module text): the fight and the end.
pub const DUEL_FIGHT_STATE: u8 = 3;
/// The duel ended.
pub const DUEL_ENDED_STATE: u8 = 0;
/// The status bit of a participant that readied up (Observed: `23`, arena flow study 3.3).
pub const READY_BIT: u8 = 0x20;
/// The status of a duel's loser from the end until it stands up (Observed: 11).
pub const LOST_STATUS: u8 = 11;
/// The value keys (CRC-32 of the lowercase names; arena flow study 3.1): team 0's and team 1's
/// round wins (names Unknown, Inferred from the values) and `WinningTeamIdx`.
pub const TEAM_0_WINS: u32 = 0x0eda_c881;
/// Team 1's round wins.
pub const TEAM_1_WINS: u32 = 0x97d3_993b;
/// `WinningTeamIdx`.
pub const WINNING_TEAM: u32 = 0x428a_d202;
/// The CRC-32 of `leave`: the 1980 event of the end screen's leave button (arena flow study
/// 4.2, exe strings).
pub const LEAVE_EVENT: u32 = 0x9bb0_80d0;
/// The `GameModeId` of a duel and the activity of its invite.
pub const DUEL_MODE: &str = "Duel";
/// `forwardType` of an invite (retail 1).
const INVITE_FORWARD: u8 = 1;
/// The bot that invites to the duel, and the one it invites.
const INVITER: usize = 0;
const INVITEE: usize = 1;
/// The bots of a duel.
pub const DUELISTS: usize = 2;
/// How far east of the inviter the invitee stands, metres: well within the duel's
/// `MaxDistance`.
const DUEL_GAP_M: f32 = 2.0;
/// The invitee stands next to the inviter once the server shows it within this many metres of
/// its teleport's place.
const ARRIVED_M: f32 = 3.0;
/// How long a bot waits for the other one: a new character first leaves the tutorial (about
/// 30 s) and takes its game master setup.
const MEET_WAIT: Duration = Duration::from_secs(240);
/// How long the invite and its accept may take.
const INVITE_WAIT: Duration = Duration::from_secs(30);
/// How long the duel's start may take after the invite: the accept and the 6 s countdown come
/// within it.
const DUEL_START_WAIT: Duration = Duration::from_secs(90);
/// The longest duel: the fight ends 90 s after its last hit (the sim's `duel` module text), and
/// a fight of two bots with hits that land lasts a few minutes at most (Inferred).
const DUEL_LONGEST: Duration = Duration::from_secs(600);
/// How long the leave of an ended duel may take (Observed: 8.05 s after the end).
const DUEL_LEAVE_WAIT: Duration = Duration::from_secs(30);

/// What one bot saw of one match or duel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MatchRecord {
    /// The match id of the pop (none for a duel).
    pub match_id: Option<String>,
    /// The own team (`lastTeamIndex` of the own participant state).
    pub team: Option<u8>,
    /// The bot was in the game mode.
    pub entered: bool,
    /// The game mode entity's state, as the bot last saw it in the mode.
    pub game_mode: Option<GameModeView>,
    /// The deaths in the mode (4977) since the bot came in.
    pub deaths: Vec<DeathRecapForAllMsg>,
    /// The hits the bot dealt (2071) and took (3601) in the mode.
    pub hits_dealt: u64,
    /// The hits it took.
    pub hits_taken: u64,
    /// The mode went from the own participant state after the bot was in it.
    pub returned: bool,
    /// The view's hit counts and death count when the bot came in.
    base: (u64, u64, usize),
}

/// What one bot's seat holds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SeatFacts {
    /// Its character id (UUID text), from its login token.
    pub character: Option<String>,
    /// Its matches, in order.
    pub matches: Vec<MatchRecord>,
    /// The step it played last.
    pub step: Option<String>,
    /// What it saw last of its queue and its game mode.
    pub last_seen: String,
    /// The place the server last showed it at (the duel's meeting).
    place: Option<[f32; 3]>,
    /// The invitee stands next to the inviter.
    arrived: bool,
}

/// What the bots of one run tell each other and the report.
#[derive(Debug)]
pub struct ArenaBoard {
    seats: Mutex<Vec<SeatFacts>>,
    /// The game mode the run plays: the CRC-32 of its lowercase id.
    mode: u32,
}

/// One bot's seat on an [`ArenaBoard`].
#[derive(Debug, Clone)]
pub struct ArenaSeat {
    board: Arc<ArenaBoard>,
    seat: usize,
}

impl ArenaBoard {
    /// The seats of `bots` bots that play the game mode of CRC `mode`.
    pub fn seats(bots: usize, mode: u32) -> Vec<ArenaSeat> {
        let board = Arc::new(Self {
            seats: Mutex::new(vec![SeatFacts::default(); bots]),
            mode,
        });
        (0..bots)
            .map(|seat| ArenaSeat {
                board: Arc::clone(&board),
                seat,
            })
            .collect()
    }
}

impl ArenaSeat {
    /// Runs `change` on the seats; a panic of another bot's thread leaves them readable.
    fn with<T>(&self, change: impl FnOnce(&mut Vec<SeatFacts>) -> T) -> T {
        let mut seats = self
            .board
            .seats
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        change(&mut seats)
    }

    /// The seat's number: 0 for the first bot.
    pub const fn number(&self) -> usize {
        self.seat
    }

    /// The game mode the run plays.
    pub fn mode(&self) -> u32 {
        self.board.mode
    }

    /// This bot's character id (UUID text) is `character`.
    pub fn set_character(&self, character: Option<String>) {
        self.with(|seats| seats[self.seat].character = character);
    }

    /// What this bot's seat holds now.
    pub fn facts(&self) -> SeatFacts {
        self.with(|seats| seats[self.seat].clone())
    }

    /// The facts of seat `seat`.
    fn facts_of(&self, seat: usize) -> Option<SeatFacts> {
        self.with(|seats| seats.get(seat).cloned())
    }

    /// Notes what the bot's `world` view shows of its queue and its game mode, its hit counts
    /// `hits` (dealt, taken) and the step it plays.
    pub fn observe(&self, world: &WorldView, hits: (u64, u64), step: Option<&str>) {
        let mode = self.board.mode;
        let matchmaking = world.matchmaking();
        let active = matchmaking.active.contains(&mode);
        let view = world.game_mode(mode);
        let deaths = world.game_mode_deaths();
        let place = world
            .own_position()
            .map(|place| [place.x, place.y, place.z()]);
        self.with(|seats| {
            let facts = &mut seats[self.seat];
            facts.step = step.map(str::to_owned);
            facts.place = place;
            facts.last_seen = format!(
                "queue status {}, match id {}, own modes {:?}, script state {}",
                matchmaking
                    .status
                    .map_or_else(|| "none".to_owned(), |status| status.to_string()),
                matchmaking.match_id.as_deref().unwrap_or("none"),
                matchmaking.active,
                view.and_then(|view| view.script_state)
                    .map_or_else(|| "none".to_owned(), |state| state.to_string()),
            );
            let finished = |record: &MatchRecord| record.entered && record.returned;
            if active {
                if facts.matches.last().is_none_or(finished) {
                    facts.matches.push(MatchRecord::default());
                }
            } else if let Some(id) = &matchmaking.match_id {
                let known = facts
                    .matches
                    .iter()
                    .any(|record| record.match_id.as_deref() == Some(id.as_str()));
                match facts.matches.last_mut() {
                    Some(record) if !record.entered => record.match_id = Some(id.clone()),
                    Some(record) if !finished(record) => {}
                    _ if known => {}
                    _ => facts.matches.push(MatchRecord {
                        match_id: Some(id.clone()),
                        ..MatchRecord::default()
                    }),
                }
            }
            let Some(record) = facts.matches.last_mut() else {
                return;
            };
            if active {
                if !record.entered {
                    record.entered = true;
                    record.base = (hits.0, hits.1, deaths.len());
                }
                if let Some(id) = &matchmaking.match_id {
                    record.match_id = Some(id.clone());
                }
                record.team = matchmaking.team.or(record.team);
                record.game_mode = view.cloned().or_else(|| record.game_mode.take());
                record.hits_dealt = hits.0.saturating_sub(record.base.0);
                record.hits_taken = hits.1.saturating_sub(record.base.1);
                record.deaths = deaths.get(record.base.2..).unwrap_or_default().to_vec();
            } else if record.entered {
                record.returned = true;
            }
        });
    }

    /// The place of seat `seat`, once the server showed one.
    fn place_of(&self, seat: usize) -> Option<[f32; 3]> {
        self.facts_of(seat)?.place
    }

    /// The character of seat `seat`, once it logged in.
    fn character_of(&self, seat: usize) -> Option<String> {
        self.facts_of(seat)?.character
    }

    /// The invitee stands next to the inviter.
    fn set_arrived(&self) {
        self.with(|seats| seats[self.seat].arrived = true);
    }

    /// Whether the bot of seat `seat` stands next to the inviter.
    fn arrived(&self, seat: usize) -> bool {
        self.facts_of(seat).is_some_and(|facts| facts.arrived)
    }
}

/// The CRC-32 of the duel's lowercase `GameModeId`.
pub fn duel_mode() -> u32 {
    lowercase_crc32(DUEL_MODE)
}

/// The proof that a generic invite step fails on the server's 2454 (with its reason), else
/// worked once `worked` says so.
fn invite_proof(worked: impl Fn(&BotSight<'_>) -> bool + 'static, described: &str) -> Proof {
    Proof::Check(
        Box::new(move |sight| {
            let failed = sight.messages.iter().find_map(|received| {
                let type_index = received.envelope.type_index()?;
                (type_index == INVITE_FAILED_TYPE_INDEX)
                    .then(|| InviteFailedMsg::decode(type_index, &received.envelope.body))
                    .flatten()
            });
            match failed {
                Some(Ok(failure)) => Some(Err(format!(
                    "the invite failed (2454 reason {})",
                    failure.reason
                ))),
                Some(Err(error)) => Some(Err(format!("a 2454 that does not read: {error}"))),
                None => worked(sight).then_some(Ok(())),
            }
        }),
        described.to_owned(),
    )
}

/// The generic invite call `make` with the bot's invite component header.
fn invite_call(
    own: u64,
    make: impl FnOnce(FacetHeader) -> InviteCall,
) -> Result<Vec<(u64, Vec<u8>)>, MessageError> {
    let call = make(FacetHeader::of(GENERIC_INVITE_COMPONENT, own));
    Ok(vec![(call.type_index(), call.encode()?)])
}

/// The steps of the bot of `seat` in a duel of the two bots of its board: god mode off, the
/// meeting (the invitee's teleport next to the inviter), the invite (3404) and its accept
/// (3406), the fight to the duel's end and the leave of the mode.
pub fn duel_steps(seat: &ArenaSeat) -> Vec<Step> {
    let duel = duel_mode();
    let mut steps = vec![gm_done("!god off".to_owned())];
    if seat.number() == INVITER {
        let keeper = seat.clone();
        steps.push(step(
            "wait for the other bot next to this one",
            Vec::new(),
            Proof::Check(
                Box::new(move |_| keeper.arrived(INVITEE).then_some(Ok(()))),
                "the other bot next to this one".to_owned(),
            ),
            MEET_WAIT,
        ));
        let inviter = seat.clone();
        steps.push(step(
            "invite the other bot to a duel (3404)",
            vec![Act::Dynamic(Box::new(move |sight| {
                let target = inviter.character_of(INVITEE)?;
                Some(invite_call(sight.own, |header| InviteCall::Send {
                    header,
                    activity: duel,
                    forward: INVITE_FORWARD,
                    target: target.into_bytes(),
                }))
            }))],
            // The accept clears both invite states at once (retail): the duel may have started
            // before the bot looked at its invite state.
            invite_proof(
                move |sight| {
                    sight
                        .world
                        .generic_invite()
                        .is_some_and(|(_, activity)| activity == duel)
                        || sight.matchmaking.active.contains(&duel)
                },
                "the duel invite in the own invite state, or the duel",
            ),
            INVITE_WAIT,
        ));
        steps.push(step(
            "the other bot accepts and the duel starts",
            Vec::new(),
            matchmaking_shows(
                move |matchmaking| matchmaking.active.contains(&duel),
                "the duel in the own participant state",
            ),
            DUEL_START_WAIT,
        ));
    } else {
        let walker = seat.clone();
        let keeper = seat.clone();
        steps.push(step(
            "stand next to the inviter (a game master's teleport)",
            vec![Act::TeleportTo(Box::new(move |_| {
                walker
                    .place_of(INVITER)
                    .map(|[x, y, z]| [x + DUEL_GAP_M, y, z])
            }))],
            Proof::Check(
                Box::new(move |sight| {
                    let target = keeper.place_of(INVITER)?;
                    let at = sight.world.own_position()?;
                    let near = (at.x - target[0] - DUEL_GAP_M).hypot(at.y - target[1]) <= ARRIVED_M;
                    near.then(|| {
                        keeper.set_arrived();
                        Ok(())
                    })
                }),
                "the server's own place next to the inviter".to_owned(),
            ),
            MEET_WAIT,
        ));
        steps.push(step(
            "accept the duel invite (3406)",
            vec![Act::Dynamic(Box::new(move |sight| {
                let (_, activity) = sight.world.generic_invite()?;
                (activity == duel).then(|| {
                    invite_call(sight.own, |header| InviteCall::Reply {
                        header,
                        accept: true,
                    })
                })
            }))],
            invite_proof(
                move |sight| sight.matchmaking.active.contains(&duel),
                "the duel in the own participant state",
            ),
            DUEL_START_WAIT,
        ));
    }
    steps.push(step(
        "fight the duel to its end (script state 0)",
        vec![Act::PlayMatch {
            end: MatchEnd::ScriptState {
                mode: duel,
                state: DUEL_ENDED_STATE,
            },
            goal: MatchGoal::Fight { ready_up: false },
            longest: DUEL_LONGEST,
        }],
        Proof::Sent,
        Duration::ZERO,
    ));
    steps.push(step(
        "leave the duel: the mode gone from the own participant state",
        Vec::new(),
        matchmaking_shows(
            move |matchmaking| !matchmaking.active.contains(&duel),
            "the duel gone from the own participant state",
        ),
        DUEL_LEAVE_WAIT,
    ));
    steps
}

/// The winner of each round of an arena match, in order, from the values lists of its game mode
/// state: a team whose round wins rose won a round.
pub fn round_winners(view: &GameModeView) -> Vec<u8> {
    let mut wins = [0_u64; 2];
    let mut winners = Vec::new();
    for values in &view.values_seen {
        for (team, key) in [TEAM_0_WINS, TEAM_1_WINS].into_iter().enumerate() {
            let Some(now) = values
                .iter()
                .find(|(own, _)| *own == key)
                .map(|(_, value)| *value)
            else {
                continue;
            };
            for _ in wins[team]..now {
                winners.push(u8::try_from(team).unwrap_or(u8::MAX));
            }
            wins[team] = wins[team].max(now);
        }
    }
    winners
}

/// How a match ended, from its game mode state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchResult {
    /// The arena match ended (script state 2) with this `WinningTeamIdx`.
    Won(u64),
    /// The duel ended (script state 0 after its fight) and the participant of this index went
    /// down; `None`: the fight timer ran out with no loser.
    Duel(Option<usize>),
    /// No end was seen.
    Open,
}

/// How the match of `view` ended: the arena's (`duel` false) or a duel's end.
pub fn match_result(view: &GameModeView, duel: bool) -> MatchResult {
    if duel {
        let fought = view
            .states
            .iter()
            .position(|change| change.state == DUEL_FIGHT_STATE);
        let end = fought.and_then(|from| {
            view.states[from..]
                .iter()
                .find(|change| change.state == DUEL_ENDED_STATE)
        });
        return end.map_or(MatchResult::Open, |change| {
            MatchResult::Duel(
                change
                    .statuses
                    .iter()
                    .position(|status| *status == LOST_STATUS),
            )
        });
    }
    if view
        .states
        .iter()
        .any(|change| change.state == ARENA_ENDED_STATE)
    {
        return view
            .value(WINNING_TEAM)
            .map_or(MatchResult::Open, MatchResult::Won);
    }
    MatchResult::Open
}

/// One bot of a run for the report: its account, what its seat holds and how its play ended.
pub struct BotRun {
    /// The bot's account.
    pub account: String,
    /// What its seat holds at the end.
    pub facts: SeatFacts,
    /// Its play, or why it did not run.
    pub play: Result<PlayReport, String>,
}

/// The label of the character of id bytes `character` among `bots`: its bot, else the id.
fn label_of(bots: &[BotRun], character: [u8; nw_messages::wire::RAW_ID_LEN]) -> String {
    let text = character_text(character);
    bot_label(bots, &text).unwrap_or_else(|| format!("character {text}"))
}

/// The label of the bot whose character id is `text`.
fn bot_label(bots: &[BotRun], text: &str) -> Option<String> {
    bots.iter().enumerate().find_map(|(number, bot)| {
        bot.facts
            .character
            .as_deref()
            .is_some_and(|own| own.eq_ignore_ascii_case(text))
            .then(|| format!("bot {number} ({})", bot.account))
    })
}

/// The run's report as Markdown and whether every step and every check passed: one section per
/// match (its id, the bots and their teams, hits dealt and taken, deaths and kills, each round's
/// winner, the end and the winning team, and each bot's return), then each bot's steps. A step or
/// a check that never came is a FAIL line with what the bot saw last. `title` heads it; `duel`
/// reads the ends as a duel's.
pub fn run_markdown(title: &str, bots: &[BotRun], duel: bool) -> (String, bool) {
    let mut markdown = format!("# {title}\n");
    let mut passed = !bots.is_empty();
    let mut keys: Vec<Option<String>> = Vec::new();
    for bot in bots {
        for record in bot.facts.matches.iter().filter(|record| record.entered) {
            if !keys.contains(&record.match_id) {
                keys.push(record.match_id.clone());
            }
        }
    }
    if keys.is_empty() {
        passed = false;
        let _ = writeln!(markdown, "\n- FAIL: no bot came into a match");
        for (number, bot) in bots.iter().enumerate() {
            let _ = writeln!(
                markdown,
                "  - bot {number} ({}): last step {}; last seen: {}",
                bot.account,
                bot.facts.step.as_deref().unwrap_or("none"),
                bot.facts.last_seen
            );
        }
    }
    for key in &keys {
        passed &= match_section(&mut markdown, bots, key.as_ref(), duel);
    }
    let _ = writeln!(markdown, "\n## Steps per bot");
    for (number, bot) in bots.iter().enumerate() {
        let _ = writeln!(markdown, "\n### Bot {number} ({})\n", bot.account);
        match &bot.play {
            Err(error) => {
                passed = false;
                let _ = writeln!(markdown, "- FAIL: the bot did not run: {error}");
            }
            Ok(report) if report.activities.is_empty() => {
                passed = false;
                let _ = writeln!(
                    markdown,
                    "- FAIL: the bot played no step (last seen: {})",
                    bot.facts.last_seen
                );
            }
            Ok(report) => {
                for play in &report.activities {
                    for action in &play.actions {
                        let _ = writeln!(markdown, "- PASS: {action}");
                    }
                    match &play.verdict {
                        Verdict::Completed => {}
                        Verdict::Broken(stop) | Verdict::NotPlayed(stop) => {
                            passed = false;
                            let _ = writeln!(
                                markdown,
                                "- FAIL: {stop} (last seen: {})",
                                bot.facts.last_seen
                            );
                        }
                    }
                }
            }
        }
    }
    let _ = writeln!(
        markdown,
        "\n**Result: {}**",
        if passed { "PASS" } else { "FAIL" }
    );
    (markdown, passed)
}

/// Writes the section of the match `key` (its match id; none for a duel) into `markdown`;
/// whether its checks passed.
fn match_section(markdown: &mut String, bots: &[BotRun], key: Option<&String>, duel: bool) -> bool {
    let mut passed = true;
    let records: Vec<(usize, &BotRun, &MatchRecord)> = bots
        .iter()
        .enumerate()
        .filter_map(|(number, bot)| {
            bot.facts
                .matches
                .iter()
                .find(|record| record.entered && record.match_id.as_ref() == key)
                .map(|record| (number, bot, record))
        })
        .collect();
    let _ = writeln!(
        markdown,
        "\n## {}\n",
        key.map_or_else(|| "Duel".to_owned(), |id| format!("Match {id}"))
    );
    let fullest = records
        .iter()
        .filter_map(|(_, _, record)| record.game_mode.as_ref())
        .max_by_key(|view| view.values_seen.len() + view.states.len());
    let deaths = records
        .iter()
        .map(|(_, _, record)| &record.deaths)
        .max_by_key(|deaths| deaths.len())
        .cloned()
        .unwrap_or_default();
    let _ = writeln!(
        markdown,
        "| Bot | Account | Team | Hits dealt | Hits taken | Deaths | Kills | Back in the open world |\n|---|---|---|---|---|---|---|---|"
    );
    for (number, bot) in bots.iter().enumerate() {
        let record = records
            .iter()
            .find(|(own, _, _)| *own == number)
            .map(|(_, _, record)| *record);
        let own = bot.facts.character.as_deref().unwrap_or_default();
        let is_own = |character: [u8; nw_messages::wire::RAW_ID_LEN]| {
            character_text(character).eq_ignore_ascii_case(own)
        };
        let died = deaths.iter().filter(|death| is_own(death.dead)).count();
        let kills = deaths.iter().filter(|death| is_own(death.killer)).count();
        let Some(record) = record else {
            passed = false;
            let _ = writeln!(
                markdown,
                "| {number} | {} | FAIL: not in this match | | | | | |",
                bot.account
            );
            continue;
        };
        let team = record
            .team
            .or_else(|| fullest.and_then(|view| view.team_of(own)))
            .map_or_else(|| "Unknown".to_owned(), |team| team.to_string());
        passed &= record.returned;
        let _ = writeln!(
            markdown,
            "| {number} | {} | {team} | {} | {} | {died} | {kills} | {} |",
            bot.account,
            record.hits_dealt,
            record.hits_taken,
            if record.returned { "yes" } else { "FAIL: no" }
        );
    }
    let last_seen = |records: &[(usize, &BotRun, &MatchRecord)]| {
        records
            .first()
            .map_or_else(String::new, |(_, bot, _)| bot.facts.last_seen.clone())
    };
    if !duel {
        let _ = writeln!(markdown, "\nRounds:\n");
        let winners = fullest.map(round_winners).unwrap_or_default();
        if winners.is_empty() {
            passed = false;
            let _ = writeln!(
                markdown,
                "- FAIL: no round winner seen (last seen: {})",
                last_seen(&records)
            );
        }
        for (round, team) in winners.iter().enumerate() {
            let _ = writeln!(markdown, "- round {}: team {team}", round + 1);
        }
    }
    let _ = writeln!(markdown, "\nDeaths:\n");
    if deaths.is_empty() {
        let _ = writeln!(markdown, "- none");
    }
    for death in &deaths {
        let _ = writeln!(
            markdown,
            "- {} by {}",
            label_of(bots, death.dead),
            label_of(bots, death.killer)
        );
    }
    let _ = writeln!(markdown, "\nEnd:\n");
    let result = fullest.map_or(MatchResult::Open, |view| match_result(view, duel));
    match (result, fullest) {
        (MatchResult::Won(team), _) => {
            let won: Vec<String> = records
                .iter()
                .filter(|(_, _, record)| record.team.map(u64::from) == Some(team))
                .map(|(number, bot, _)| format!("bot {number} ({})", bot.account))
                .collect();
            let _ = writeln!(
                markdown,
                "- PASS: the match ended (script state 2); winning team {team}: {}",
                won.join(", ")
            );
        }
        (MatchResult::Duel(Some(loser)), Some(view)) => {
            let name = |index: usize| {
                view.characters.get(index).map_or_else(
                    || format!("participant {index}"),
                    |own| label_of(bots, *own),
                )
            };
            let _ = writeln!(
                markdown,
                "- PASS: the duel ended (script state 0); {} went down, {} won",
                name(loser),
                name(1 - loser.min(1))
            );
        }
        (MatchResult::Duel(_), _) => {
            let _ = writeln!(
                markdown,
                "- PASS: the duel ended (script state 0) with no loser: the fight timer ran out"
            );
        }
        (MatchResult::Open, _) => {
            passed = false;
            let _ = writeln!(
                markdown,
                "- FAIL: no end seen (last seen: {})",
                last_seen(&records)
            );
        }
    }
    let _ = writeln!(markdown, "\nReturn:\n");
    for (number, bot, record) in &records {
        if record.returned {
            let _ = writeln!(
                markdown,
                "- PASS: bot {number} ({}) out of the mode, back in the open world",
                bot.account
            );
        } else {
            let _ = writeln!(
                markdown,
                "- FAIL: bot {number} ({}) still in the mode (last step {}; last seen: {})",
                bot.account,
                bot.facts.step.as_deref().unwrap_or("none"),
                bot.facts.last_seen
            );
        }
    }
    passed
}

#[cfg(test)]
mod tests {
    use nw_messages::game_mode_state::{GameModeState, ParticipantRef};
    use nw_messages::items::FullList;
    use nw_messages::list_delta::{ListChange, ReplicatedList};
    use nw_messages::wire::RAW_ID_LEN;

    use super::*;
    use crate::play::QuestPlay;
    use crate::view::ScriptChange;

    /// Made-up characters of two bots.
    const FIRST: [u8; RAW_ID_LEN] = [0x11; RAW_ID_LEN];
    const SECOND: [u8; RAW_ID_LEN] = [0x22; RAW_ID_LEN];
    const MATCH: &str = "00000000-0000-7000-8000-00000000000a";

    #[test]
    fn the_keys_are_the_crc_of_their_names() {
        assert_eq!(lowercase_crc32("WinningTeamIdx"), WINNING_TEAM);
        assert_eq!(lowercase_crc32("Leave"), LEAVE_EVENT);
        assert_eq!(duel_mode(), 0x9bb4_a762);
    }

    fn values(team_0: u64, team_1: u64) -> Vec<(u32, u64)> {
        vec![(TEAM_1_WINS, team_1), (TEAM_0_WINS, team_0)]
    }

    #[test]
    fn a_team_whose_round_wins_rose_won_the_round() {
        let view = GameModeView {
            values_seen: vec![
                values(0, 0),
                values(0, 1),
                values(0, 1),
                values(1, 1),
                vec![(WINNING_TEAM, 1)],
                values(1, 3),
            ],
            ..GameModeView::default()
        };
        assert_eq!(round_winners(&view), [1, 0, 1, 1]);
    }

    #[test]
    fn an_arena_match_ends_with_its_winning_team_and_a_duel_with_its_loser() {
        let change = |state, statuses: &[u8]| ScriptChange {
            state,
            statuses: statuses.to_vec(),
        };
        let arena = GameModeView {
            states: vec![change(PREGAME_STATE, &[3, 3]), change(ROUND_STATE, &[3, 3])],
            values: vec![(WINNING_TEAM, 1)],
            ..GameModeView::default()
        };
        assert_eq!(match_result(&arena, false), MatchResult::Open, "no state 2");
        let ended = GameModeView {
            states: vec![
                change(ROUND_STATE, &[3, 3]),
                change(ARENA_ENDED_STATE, &[3, 7]),
            ],
            ..arena
        };
        assert_eq!(match_result(&ended, false), MatchResult::Won(1));
        let duel = GameModeView {
            states: vec![
                change(1, &[3, 3]),
                change(DUEL_FIGHT_STATE, &[3, 3]),
                change(DUEL_ENDED_STATE, &[3, LOST_STATUS]),
            ],
            ..GameModeView::default()
        };
        assert_eq!(match_result(&duel, true), MatchResult::Duel(Some(1)));
        let timed_out = GameModeView {
            states: vec![
                change(DUEL_FIGHT_STATE, &[3, 3]),
                change(DUEL_ENDED_STATE, &[3, 3]),
            ],
            ..GameModeView::default()
        };
        assert_eq!(match_result(&timed_out, true), MatchResult::Duel(None));
        let countdown = GameModeView {
            states: vec![change(1, &[3, 3])],
            ..GameModeView::default()
        };
        assert_eq!(match_result(&countdown, true), MatchResult::Open);
    }

    /// A world view that read `messages`.
    fn world_of(messages: &[crate::rep::Received]) -> WorldView {
        WorldView::from_messages(messages).expect("the messages read")
    }

    #[test]
    fn a_seat_keeps_the_pop_and_opens_its_record_when_the_bot_comes_into_the_mode() {
        let seats = ArenaBoard::seats(1, 0x1fad_9d42);
        let seat = &seats[0];
        seat.observe(&world_of(&[]), (5, 6), Some("join the queue"));
        assert!(seat.facts().matches.is_empty(), "no pop yet");
        assert_eq!(seat.facts().step.as_deref(), Some("join the queue"));
        assert!(seat.facts().last_seen.contains("match id none"));
    }

    fn bot(account: &str, character: [u8; RAW_ID_LEN], record: MatchRecord) -> BotRun {
        BotRun {
            account: account.to_owned(),
            facts: SeatFacts {
                character: Some(character_text(character)),
                matches: vec![record],
                last_seen: "queue status 5".to_owned(),
                ..SeatFacts::default()
            },
            play: Ok(PlayReport {
                outcome: crate::udp::RepOutcome::default(),
                quests: Vec::new(),
                activities: vec![QuestPlay {
                    quest: "Arena".to_owned(),
                    verdict: Verdict::Completed,
                    actions: vec!["join the Arena3v3 queue alone (6254)".to_owned()],
                }],
            }),
        }
    }

    #[test]
    fn the_report_names_the_match_its_rounds_deaths_end_and_returns() {
        let view = GameModeView {
            characters: vec![FIRST, SECOND],
            teams: vec![0, 1],
            values_seen: vec![values(0, 1), values(0, 2), values(0, 3)],
            values: vec![(WINNING_TEAM, 1)],
            states: vec![ScriptChange {
                state: ARENA_ENDED_STATE,
                statuses: vec![7, 3],
            }],
            ..GameModeView::default()
        };
        let death = DeathRecapForAllMsg::of(1, FIRST, SECOND);
        let record = |team, dealt| MatchRecord {
            match_id: Some(MATCH.to_owned()),
            team: Some(team),
            entered: true,
            game_mode: Some(view.clone()),
            deaths: vec![death.clone(); 3],
            hits_dealt: dealt,
            hits_taken: 4,
            returned: true,
            ..MatchRecord::default()
        };
        let bots = [
            bot("qaarenaa", FIRST, record(0, 2)),
            bot("qaarenab", SECOND, record(1, 9)),
        ];
        let (markdown, passed) = run_markdown("Arena run: 1v1", &bots, false);
        assert!(passed, "{markdown}");
        assert!(markdown.contains(&format!("## Match {MATCH}")));
        assert!(markdown.contains("| 0 | qaarenaa | 0 | 2 | 4 | 3 | 0 | yes |"));
        assert!(markdown.contains("| 1 | qaarenab | 1 | 9 | 4 | 0 | 3 | yes |"));
        assert!(markdown.contains("- round 3: team 1"));
        assert!(markdown.contains("- bot 0 (qaarenaa) by bot 1 (qaarenab)"));
        assert!(markdown.contains("winning team 1: bot 1 (qaarenab)"));
        assert!(markdown.contains("- PASS: join the Arena3v3 queue alone (6254)"));
        assert!(markdown.ends_with("**Result: PASS**\n"));
    }

    #[test]
    fn a_match_with_no_end_and_a_bot_that_stayed_is_a_fail_with_what_was_seen_last() {
        let record = MatchRecord {
            match_id: Some(MATCH.to_owned()),
            team: Some(0),
            entered: true,
            game_mode: Some(GameModeView::default()),
            ..MatchRecord::default()
        };
        let mut stuck = bot("qaarenaa", FIRST, record);
        stuck.facts.step = Some("play the match".to_owned());
        if let Ok(report) = &mut stuck.play {
            report.activities[0].verdict = Verdict::Broken("no proof within 10 s".to_owned());
        }
        let (markdown, passed) = run_markdown("Arena run", &[stuck], false);
        assert!(!passed);
        assert!(markdown.contains("- FAIL: no round winner seen (last seen: queue status 5)"));
        assert!(markdown.contains("- FAIL: no end seen (last seen: queue status 5)"));
        assert!(markdown.contains("still in the mode (last step play the match"));
        assert!(markdown.contains("- FAIL: no proof within 10 s (last seen: queue status 5)"));
        let (none, passed) = run_markdown(
            "Arena run",
            &[bot("qa", FIRST, MatchRecord::default())],
            false,
        );
        assert!(!passed);
        assert!(none.contains("- FAIL: no bot came into a match"));
    }

    #[test]
    fn a_duel_report_names_who_went_down() {
        let view = GameModeView {
            characters: vec![FIRST, SECOND],
            teams: vec![0, 1],
            states: vec![
                ScriptChange {
                    state: DUEL_FIGHT_STATE,
                    statuses: vec![3, 3],
                },
                ScriptChange {
                    state: DUEL_ENDED_STATE,
                    statuses: vec![LOST_STATUS, 3],
                },
            ],
            ..GameModeView::default()
        };
        let record = |team| MatchRecord {
            team: Some(team),
            entered: true,
            game_mode: Some(view.clone()),
            returned: true,
            ..MatchRecord::default()
        };
        let bots = [
            bot("qaduela", FIRST, record(0)),
            bot("qaduelb", SECOND, record(1)),
        ];
        let (markdown, passed) = run_markdown("Duel run", &bots, true);
        assert!(passed, "{markdown}");
        assert!(markdown.contains("## Duel"));
        assert!(markdown.contains("bot 0 (qaduela) went down, bot 1 (qaduelb) won"));
        assert!(!markdown.contains("Rounds"));
    }

    #[test]
    fn the_duel_steps_meet_invite_accept_fight_and_leave() {
        let seats = ArenaBoard::seats(DUELISTS, duel_mode());
        let what = |seat: &ArenaSeat| -> Vec<String> {
            duel_steps(seat).into_iter().map(|step| step.what).collect()
        };
        assert_eq!(
            what(&seats[INVITER]),
            [
                "!god off",
                "wait for the other bot next to this one",
                "invite the other bot to a duel (3404)",
                "the other bot accepts and the duel starts",
                "fight the duel to its end (script state 0)",
                "leave the duel: the mode gone from the own participant state",
            ]
        );
        assert_eq!(
            what(&seats[INVITEE])[1..3],
            [
                "stand next to the inviter (a game master's teleport)",
                "accept the duel invite (3406)"
            ]
        );
        assert!(!seats[INVITER].arrived(INVITEE));
        seats[INVITEE].set_arrived();
        assert!(seats[INVITER].arrived(INVITEE), "every seat sees it");
    }

    #[test]
    fn a_game_mode_view_takes_each_child_and_names_the_foes() {
        let participant = |byte| ParticipantRef {
            raw_id: [byte; RAW_ID_LEN],
            ..ParticipantRef::default()
        };
        let first = GameModeState {
            script_state: Some(PREGAME_STATE),
            mode: Some(0x1fad_9d42),
            participants: Some(ReplicatedList::Full(FullList::first(vec![
                participant(0xa1),
                participant(0xb1),
            ]))),
            statuses: Some(ReplicatedList::Full(FullList::first(vec![3, 3]))),
            teams: Some(ReplicatedList::Full(FullList::first(vec![0, 1]))),
            characters: Some(ReplicatedList::Full(FullList::first(vec![FIRST, SECOND]))),
            values: Some(FullList::first(values(0, 0))),
            ..GameModeState::default()
        };
        let ready = GameModeState {
            statuses: Some(ReplicatedList::Changes(vec![ListChange {
                index: 1,
                version: 3,
                value: 3 | READY_BIT,
            }])),
            ..GameModeState::default()
        };
        let mut view = GameModeView::default();
        view.apply(first);
        view.apply(ready);
        assert_eq!(view.statuses, [3, 3 | READY_BIT]);
        assert_eq!(view.team_of(&character_text(SECOND)), Some(1));
        assert_eq!(view.states.len(), 1);
        assert_eq!(
            view.foes_of(&character_text(FIRST)),
            Some(Vec::new()),
            "no participant entity of this process yet"
        );
        assert_eq!(view.foes_of("not a participant"), None);
    }
}
