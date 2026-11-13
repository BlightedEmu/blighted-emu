// SPDX-License-Identifier: AGPL-3.0-or-later
//! Territory wars (wars and sieges study 2026-11-12; findings
//! `2026-11-12-wars-and-sieges.md`): the war data every client gets, the war board's sign-ups
//! and the roster.
//!
//! **War data** (Observed, 1739 in 837 ledgers): component [`WAR_DATA_COMPONENT`] of the
//! slot-2 world entity lists every war with its companies, factions, territory and phase
//! ([`nw_messages::war_states`]). The wars live in the world database (world migration 0040,
//! [`nw_db::territory_wars`]); every sim reads them with the territory owners
//! ([`super::land_claims::TerritoryJob`]), so a change on one sim reaches the clients of every
//! sim. The phase follows the clock (Observed in `arvaysa_ES_attacker_war_45vs45`): scheduled
//! until the start time, running for [`nw_db::WAR_LENGTH_MS`], then resolved, and the war leaves the
//! list [`nw_db::WAR_RESULT_SHOWN_MS`] after its result. A war whose time ran out without a
//! result goes to the defender (Inferred: wars 7015, 5452 and 6672 of the index resolved at
//! their end time exactly). When the attacker wins, it takes the territory's claim
//! ([`nw_db::resolve_war`]).
//!
//! **Declaration**: no capture holds a war declaration (the `Social` war calls of the Azoth
//! catalog are in no ledger), so the player flow (influence race, the declare call, its cost and
//! its window) is **Unknown**. A game master declares a war between the owner of a territory and
//! another company (`!war`, [`super::gm_chat`]) and gives its result; the war instance (the fort
//! siege, its capture points and siege weapons) is not built, so a war's result comes from the
//! game master or from the end of its time.
//!
//! **Sign-up** (Observed, every 2762, 2784, 2767, 2771, 2791 of the index): a sign-up names the
//! side and the territory; retail answers a done sign-up with 2169 `01 00000000` and a 2722 of
//! the territory and the side, a sign-up of a kicked player with `00 0000000c`, a leave with
//! 2230. The list (2767) of a side is answered with its sign-ups (2726), the count (2771) with a
//! 2729; the remote board use (2791) with a 2722 of the player's side. The roster call (2772) has
//! no answer message: the roster is the player's territory interactor state (1880, own
//! component [`INTERACTOR_COMPONENT`]). Inferred: a player signs up for the side whose
//! company is of its faction, while the war waits for its start; a refused sign-up or leave of
//! no such war carries result 0 and reason 0 (retail's reason codes other than 12 and 14 are
//! Unknown).
//!
//! **Raids** (Observed, `arvaysa_ES_attacker_war_45vs45` 1.410 s): each side of a war entry
//! names its raid from the war's first state on: the side's number is the raid id, then the
//! raid's ten group ids, the raid entity's id and that id's facet entity ([`war_side`]). A
//! roster player joins its side's raid at its move into the war ([`super::raids`]).
//!
//! **Roster** (Observed, `army-management`, `joining_defending_war_2`): 10 groups of 5; the
//! roster calls (2779, 2781, 2782, 1871, 1676, 1892) are each answered with a 2733 (done
//! `01 00000000`, refused `00 0000000e`). Inferred: the war leader is the side company's
//! governor and its consuls are officers; a roster player with the officer or leader permission,
//! or such a company member, edits its side's roster; a call that does not fit the roster is
//! refused.

use std::collections::BTreeSet;

use nw_bus::{ConnectionRef, RaidSpec};
use nw_db::{
    DbError, Declared, PgPool, RosterSeat, SeatChange, TerritoryWar, WAR_RESULT_SHOWN_MS,
    WarSignup, cancel_war, change_roster, declare_war, leave_war, resolve_war, sign_up_for_war,
};
use nw_messages::facet::FacetHeader;
use nw_messages::items::{FIRST_VERSION, FullList};
use nw_messages::keyed_list::{IndexedList, KeyedList};
use nw_messages::social::raid_data::{RAID_TYPE_INVASION, RAID_TYPE_WAR};
use nw_messages::war_states::WAR_READY;
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    BundleMember, CallResult, GROUP_SLOTS, MemberState, PERMISSION_LEADER, PERMISSION_MEMBER,
    PERMISSION_OFFICER, REASON_BANNED, REASON_NONE, REASON_ROSTER_REFUSED, ROSTER_GROUPS,
    RosterGroup, RosterPlace, RosterPlayer, SIDE_ATTACKER, SIDE_DEFENDER, ServerMessage,
    SignupData, SignupEntry, TerritoryFacetReadyMsg, TerritoryInteractorState, WAR_DATA_COMPONENT,
    WAR_KIND_INVASION, WAR_KIND_WAR, WAR_PHASE_RESOLVED, WAR_PHASE_RUNNING, WAR_PHASE_SCHEDULED,
    WAR_PLAYER_KIND, WarDataState, WarEntry, WarPlayer, WarSide, WarSignupClientMsg,
    WarSignupServerMsg, WarStatus, facet_of_raw_id,
};
use uuid::Uuid;

use super::gm_chat::DONE;
use super::raids::raid_spec;
use super::{Player, SimWorld, send_now};

/// The own player component of the territory interactor state (Observed: component 7 in the
/// 2,703 retail bodies).
const INTERACTOR_COMPONENT: u64 = 7;
/// The governor's and the consul's company ranks (company members, world migration 0014).
const RANK_GOVERNOR: u8 = 0;
const RANK_CONSUL: u8 = 1;
/// The 2722 side of a player with no sign-up (Observed: 0 for a player in no company at a
/// governor's desk; Inferred for a board).
const NO_SIDE: u32 = 0;
/// The war data's flag and value of a war (Observed: 0 and 0 on invasions, other values on wars;
/// meaning Unknown, Inferred 0 here).
const NO_FLAG: u8 = 0;
const NO_VALUE: u32 = 0;
/// The instance state of a war without an instance (Observed: 0 before the start).
const NO_INSTANCE: u8 = 0;
/// The war's number and the extra number of a war entry (Observed: 0).
const NO_EXTRA: u64 = 0;
/// The game mode field of a war that waits for its start (Observed: 0 in every scheduled entry
/// of `arvaysa_ES_attacker_war_45vs45`, 1783536276201 to 1783537182923). From the start on the
/// field holds the world's value ([`Wars::of_world`]).
const NO_GAME_MODE: u32 = 0;
/// The bytes of an empty id.
const NO_ID: [u8; RAW_ID_LEN] = [0; RAW_ID_LEN];
/// The group data's flags (Observed: 0).
const GROUP_FLAGS: [u8; 3] = [0; 3];
/// The sign-up's last byte (Observed: 0).
const SIGNUP_FLAG: u8 = 0;
/// Nanoseconds per millisecond.
const NS_PER_MS: u64 = 1_000_000;
/// The game master's answers of `!war`.
const NO_COMPANY: &str = "No company has that name.";
const NO_OWNER: &str = "That territory has no owner.";
const OWN_COMPANY: &str = "That company owns the territory.";
const AT_WAR: &str = "That territory has a war already.";
const NO_WAR: &str = "That territory has no war without a result.";
const NOT_STORED: &str = "The database did not take the change.";

/// The phase of `war` at Unix ms `now_ms` and its time, Unix ms (module doc).
fn phase_of(war: &TerritoryWar, now_ms: u64) -> (u32, u64) {
    let end_ms = war.ends_ms();
    match war.result {
        Some((_, at_ms)) => (
            WAR_PHASE_RESOLVED,
            at_ms.saturating_add(WAR_RESULT_SHOWN_MS),
        ),
        None if now_ms < war.starts_ms => (WAR_PHASE_SCHEDULED, war.starts_ms),
        None => (WAR_PHASE_RUNNING, end_ms),
    }
}

/// The company of `side` of `war`.
fn side_company(war: &TerritoryWar, side: u32) -> Option<(Uuid, u8)> {
    match side {
        SIDE_ATTACKER => Some(war.attacker),
        SIDE_DEFENDER => Some(war.defender),
        _ => None,
    }
}

/// The war data number of `side` of `war`: the attacker's is twice the war's number, the
/// defender's one more (Observed: 9428 and 9429 in `arvaysa_ES_attacker_war_45vs45`). It is the
/// side's raid id ([`super::raids`]).
fn side_number(war: &TerritoryWar, side: u32) -> u64 {
    if war.is_invasion() && side == SIDE_ATTACKER {
        return u64::from(NO_SIDE);
    }
    let attacker = war.number.saturating_mul(2);
    if side == SIDE_DEFENDER {
        attacker.saturating_add(1)
    } else {
        attacker
    }
}

/// The other side of `side`.
const fn other_side(side: u32) -> u32 {
    if side == SIDE_DEFENDER {
        SIDE_ATTACKER
    } else {
        SIDE_DEFENDER
    }
}

/// The raid of `side` of `war`: its side number as raid id, the other side's as opposing raid,
/// raid type war ([`super::raids`]).
fn war_raid(war: &TerritoryWar, side: u32) -> RaidSpec {
    let kind = if war.is_invasion() {
        RAID_TYPE_INVASION
    } else {
        RAID_TYPE_WAR
    };
    raid_spec(
        war.war,
        side_number(war, side),
        kind,
        side_number(war, other_side(side)),
    )
}

/// The war data side of `side` of `war`: its raid id, the raid's group ids, its raid entity and
/// that entity's facet entity as the key (Observed: `arvaysa_ES_attacker_war_45vs45` 1.410 s,
/// the side's `raid` is the raid entity's card id and its key the id's first 8 bytes as a
/// little-endian number, from the war's first state on).
fn war_side(war: &TerritoryWar, side: u32) -> WarSide {
    if war.is_invasion() && side == SIDE_ATTACKER {
        return super::invasions::empty_side();
    }
    let raid = war_raid(war, side);
    WarSide {
        number: raid.raid,
        groups: raid.groups.iter().map(|group| *group.as_bytes()).collect(),
        raid: raid.entity,
        key: facet_of_raw_id(&raid.entity),
    }
}

/// The war data entry of `war` at Unix ms `now_ms` on the world of value `world`
/// ([`Wars::of_world`]).
fn war_entry(war: &TerritoryWar, now_ms: u64, world: u32) -> WarEntry {
    let (phase, time_ms) = phase_of(war, now_ms);
    let game_mode = if phase == WAR_PHASE_SCHEDULED {
        NO_GAME_MODE
    } else {
        world
    };
    WarEntry {
        attacker_company: *war.attacker.0.as_bytes(),
        defender_company: *war.defender.0.as_bytes(),
        attacker: war_side(war, SIDE_ATTACKER),
        defender: war_side(war, SIDE_DEFENDER),
        attacker_faction: war.attacker.1,
        defender_faction: war.defender.1,
        territory: war.territory,
        kind: if war.is_invasion() {
            WAR_KIND_INVASION
        } else {
            WAR_KIND_WAR
        },
        war: *war.war.as_bytes(),
        extra: NO_EXTRA,
        status: WarStatus {
            winner: war.result.map_or(NO_ID, |(winner, _)| *winner.as_bytes()),
            flag: NO_FLAG,
            value: NO_VALUE,
            instance_state: NO_INSTANCE,
            ready: WAR_READY,
            phase,
            time_ns: time_ms.saturating_mul(NS_PER_MS),
            game_mode,
            instance: NO_ID,
            host: NO_ID,
        },
    }
}

/// The permission `signup` has on its side's roster: its stored one, or the one its rank in the
/// side's company gives (Inferred: the governor leads, the consuls are officers).
fn permission_of(signup: &WarSignup, side_company: Option<Uuid>) -> u8 {
    let ranked = match signup.company {
        Some((company, rank)) if Some(company) == side_company => match rank {
            RANK_GOVERNOR => PERMISSION_LEADER,
            RANK_CONSUL => PERMISSION_OFFICER,
            _ => PERMISSION_MEMBER,
        },
        _ => PERMISSION_MEMBER,
    };
    signup.roster.permission.max(ranked)
}

/// The war player form of `signup`.
fn war_player(signup: &WarSignup) -> WarPlayer {
    WarPlayer {
        character: signup.character.hyphenated().to_string(),
        name: signup.name.clone(),
        kind: WAR_PLAYER_KIND,
    }
}

/// The territory interactor state of `character`: its sign-ups by territory and, for the war
/// it signed up for last, its side's whole roster.
fn interactor_state(wars: &[TerritoryWar], character: Uuid) -> TerritoryInteractorState {
    let signed: Vec<(&TerritoryWar, &WarSignup)> = wars
        .iter()
        .filter(|war| war.result.is_none())
        .filter_map(|war| {
            war.signups
                .iter()
                .find(|signup| signup.character == character && !signup.kicked)
                .map(|signup| (war, signup))
        })
        .collect();
    let signups = signed
        .iter()
        .map(|(war, signup)| {
            let company =
                side_company(war, u32::from(signup.side)).map_or(NO_ID, |(id, _)| *id.as_bytes());
            (
                war.territory,
                SignupData {
                    company,
                    side: u32::from(signup.side),
                    on_roster: u8::from(signup.roster.place.is_some()),
                    flag: SIGNUP_FLAG,
                },
            )
        })
        .collect();
    let mut state = TerritoryInteractorState {
        signups: Some(KeyedList::full(FIRST_VERSION, signups)),
        ..TerritoryInteractorState::default()
    };
    if let Some((war, own)) = signed.last() {
        let company = side_company(war, u32::from(own.side)).map(|(id, _)| id);
        for group in 0..ROSTER_GROUPS {
            for slot in 0..GROUP_SLOTS {
                let seated = war.signups.iter().find(|signup| {
                    signup.side == own.side && signup.roster.place == Some((group, slot))
                });
                let player = seated.map_or_else(RosterPlayer::default, |signup| RosterPlayer {
                    player: war_player(signup),
                    company: signup
                        .company
                        .map_or(NO_ID, |(company, _)| *company.as_bytes()),
                    gear_score: signup.gear_score,
                    permission: permission_of(signup, company),
                    flag: SIGNUP_FLAG,
                });
                state.players.push(((group, slot), player));
            }
            state.groups.push((
                group,
                RosterGroup {
                    flags: GROUP_FLAGS,
                    index: group,
                },
            ));
        }
    }
    state
}

/// What the client of a player has of the wars.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct WarView {
    /// The wars version and the phase of each war its war data showed.
    data: Option<(u64, Vec<u32>)>,
    /// The territory interactor state it got last.
    interactor: Option<TerritoryInteractorState>,
    /// The territory of the war board it uses (2791, 2762, 2784).
    board: Option<u16>,
}

/// A war change to make in the world database, with whom to answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WarJob {
    /// A game master declares a war.
    Declare {
        territory: u16,
        attacker: String,
        starts_ms: u64,
        answer_to: ConnectionRef,
    },
    /// A war gets its result (from a game master, or at its end).
    Resolve {
        war: Uuid,
        winner: Uuid,
        at_ms: u64,
        answer_to: Option<ConnectionRef>,
    },
    /// A game master takes back a war.
    Cancel {
        territory: u16,
        answer_to: ConnectionRef,
    },
    /// A player signs up.
    SignUp {
        war: Uuid,
        character: Uuid,
        side: u32,
        territory: u16,
        gear_score: u16,
        header: FacetHeader,
        answer_to: ConnectionRef,
    },
    /// A player leaves a sign-up.
    Leave {
        war: Uuid,
        character: Uuid,
        territory: u16,
        header: FacetHeader,
        answer_to: ConnectionRef,
    },
    /// A roster editor changes the roster.
    Roster {
        war: Uuid,
        changes: Vec<SeatChange>,
        header: FacetHeader,
        answer_to: ConnectionRef,
    },
    /// An invasion change ([`super::invasions`]).
    Invasion(super::invasions::InvasionJob),
}

/// The answer a war job's result gives.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum WarAnswer {
    /// Nobody to answer.
    Quiet,
    /// A game master's chat line.
    Gm(ConnectionRef, &'static str),
    /// A player's messages.
    Player(ConnectionRef, Vec<ServerMessage>),
}

impl WarJob {
    /// Runs the job against the world database.
    pub(crate) async fn run(self, world: &PgPool) -> WarAnswer {
        match self {
            Self::Declare {
                territory,
                attacker,
                starts_ms,
                answer_to,
            } => {
                let line = match declare_war(world, territory, &attacker, starts_ms).await {
                    Ok(Declared::Declared) => DONE,
                    Ok(Declared::NoCompany) => NO_COMPANY,
                    Ok(Declared::NoOwner) => NO_OWNER,
                    Ok(Declared::OwnCompany) => OWN_COMPANY,
                    Ok(Declared::AtWar) => AT_WAR,
                    Err(error) => not_stored(&error),
                };
                WarAnswer::Gm(answer_to, line)
            }
            Self::Resolve {
                war,
                winner,
                at_ms,
                answer_to,
            } => {
                let line = match resolve_war(world, war, winner, at_ms).await {
                    Ok(true) => DONE,
                    Ok(false) => NO_WAR,
                    Err(error) => not_stored(&error),
                };
                answer_to.map_or(WarAnswer::Quiet, |connection| {
                    WarAnswer::Gm(connection, line)
                })
            }
            Self::Cancel {
                territory,
                answer_to,
            } => {
                let line = match cancel_war(world, territory).await {
                    Ok(true) => DONE,
                    Ok(false) => NO_WAR,
                    Err(error) => not_stored(&error),
                };
                WarAnswer::Gm(answer_to, line)
            }
            Self::SignUp {
                war,
                character,
                side,
                territory,
                gear_score,
                header,
                answer_to,
            } => {
                let side_byte = u8::try_from(side).unwrap_or(u8::MAX);
                let result =
                    match sign_up_for_war(world, war, character, side_byte, gear_score).await {
                        Ok(true) => Some(CallResult::DONE),
                        Ok(false) => Some(CallResult::refused(REASON_BANNED)),
                        Err(error) => {
                            tracing::error!(%error, %character, "war sign-up not stored");
                            None
                        }
                    };
                let mut messages = vec![ServerMessage::WarSignup(
                    WarSignupServerMsg::SignupResponseReceived {
                        header,
                        result: result.unwrap_or(CallResult::refused(REASON_NONE)),
                    },
                )];
                if result == Some(CallResult::DONE) {
                    messages.push(ServerMessage::TerritoryFacetReady(TerritoryFacetReadyMsg {
                        header,
                        territory,
                        access: side,
                    }));
                }
                WarAnswer::Player(answer_to, messages)
            }
            Self::Leave {
                war,
                character,
                territory,
                header,
                answer_to,
            } => {
                let result = match leave_war(world, war, character).await {
                    Ok(true) => CallResult::DONE,
                    Ok(false) => CallResult::refused(REASON_NONE),
                    Err(error) => {
                        tracing::error!(%error, %character, "war leave not stored");
                        CallResult::refused(REASON_NONE)
                    }
                };
                WarAnswer::Player(
                    answer_to,
                    vec![ServerMessage::WarSignup(
                        WarSignupServerMsg::LeaveResponseReceived {
                            header,
                            territory,
                            result,
                        },
                    )],
                )
            }
            Self::Roster {
                war,
                changes,
                header,
                answer_to,
            } => {
                let result = match change_roster(world, war, &changes).await {
                    Ok(true) => CallResult::DONE,
                    Ok(false) => CallResult::refused(REASON_ROSTER_REFUSED),
                    Err(error) => {
                        tracing::error!(%error, %war, "war roster not stored");
                        CallResult::refused(REASON_ROSTER_REFUSED)
                    }
                };
                WarAnswer::Player(answer_to, vec![roster_answer(header, result)])
            }
            Self::Invasion(job) => job.run(world).await,
        }
    }
}

fn not_stored(error: &DbError) -> &'static str {
    tracing::error!(%error, "war change not stored");
    NOT_STORED
}

fn roster_answer(header: FacetHeader, result: CallResult) -> ServerMessage {
    ServerMessage::WarSignup(WarSignupServerMsg::SetRosterResponseReceived { header, result })
}

/// The wars as this sim last read them, and the war jobs that wait for the territory job.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Wars {
    /// The wars, by territory.
    wars: Vec<TerritoryWar>,
    /// Rises with each change of the wars read.
    version: u64,
    /// A read came: later changes are roster changes for the pre-raid status ([`super::raids`]).
    primed: bool,
    /// The jobs that wait for the loop.
    jobs: Vec<WarJob>,
    /// The wars whose end-of-time result was asked for.
    ending: BTreeSet<Uuid>,
    /// The world's value: the game mode field of a running or resolved war.
    world: u32,
}

impl Wars {
    /// The wars of the world whose login messages carry the value `world` (the first `u32` of
    /// the 1034 `CalendarConnectedMsg`). Observed: the game mode field of every running and
    /// resolved war entry is that value (0x1ba81e0e in `arvaysa_ES_attacker_war_45vs45` from
    /// 1783537199924 and in `RS_offensive_war`, 0x131b9569 in `CK_offensive_war`: each the
    /// value of the ledger's own 1034; no `GameModeData` row has these CRCs). Retail sends the
    /// same value in the 446 and the 1034; this server's two are not the same, and the 1034's
    /// is used (Inferred).
    pub(super) fn of_world(world: u32) -> Self {
        Self {
            world,
            ..Self::default()
        }
    }

    /// Takes the wars a read found (a failed read keeps the last ones); the wars they replaced,
    /// none for the sim's first read ([`super::raids`]: the pre-raid statuses of a change).
    pub(super) fn read(&mut self, read: Result<Vec<TerritoryWar>, DbError>) -> Vec<TerritoryWar> {
        match read {
            Ok(wars) => {
                let primed = std::mem::replace(&mut self.primed, true);
                if wars == self.wars {
                    return Vec::new();
                }
                self.version += 1;
                let replaced = std::mem::replace(&mut self.wars, wars);
                if primed { replaced } else { Vec::new() }
            }
            Err(error) => {
                tracing::warn!(%error, "territory wars not read");
                Vec::new()
            }
        }
    }

    /// The wars as this sim last read them.
    pub(super) fn all(&self) -> &[TerritoryWar] {
        &self.wars
    }

    /// The jobs to run at Unix ms `now_ms`: the asked changes and the result of each war whose
    /// time ran out (the defender's, module doc).
    pub(super) fn take_jobs(&mut self, now_ms: u64) -> Vec<WarJob> {
        for war in &self.wars {
            let end_ms = war.ends_ms();
            if war.result.is_none() && now_ms >= end_ms && self.ending.insert(war.war) {
                self.jobs.push(WarJob::Resolve {
                    war: war.war,
                    winner: war.defender.0,
                    at_ms: end_ms,
                    answer_to: None,
                });
            }
        }
        std::mem::take(&mut self.jobs)
    }

    /// The war `war`, as this sim last read it.
    pub(super) fn war(&self, war: Uuid) -> Option<&TerritoryWar> {
        self.wars.iter().find(|read| read.war == war)
    }

    /// Asks for the attacker's result of `war`, won by `winner` at Unix ms `at_ms` in its
    /// instance ([`super::war_fight`]: the claim point was taken); once.
    pub(super) fn win(&mut self, war: Uuid, winner: Uuid, at_ms: u64) {
        if self.ending.insert(war) {
            self.jobs.push(WarJob::Resolve {
                war,
                winner,
                at_ms,
                answer_to: None,
            });
        }
    }

    /// The invasion `war` was lost at Unix ms `at_ms`; its territory loses a tier of each upgrade
    /// of `lowered` ([`super::invasions`]). Once per war, as [`Self::win`].
    pub(super) fn lose_invasion(&mut self, war: Uuid, at_ms: u64, lowered: Vec<u8>) {
        if self.ending.insert(war) {
            self.jobs
                .push(WarJob::Invasion(super::invasions::InvasionJob::Lose {
                    war,
                    at_ms,
                    lowered,
                }));
        }
    }

    /// Queues the invasion change `job` of a game master ([`super::invasions`]).
    pub(super) fn push_invasion_job(&mut self, job: super::invasions::InvasionJob) {
        self.jobs.push(WarJob::Invasion(job));
    }

    /// The war without a result of `territory`.
    pub(super) fn open_war(&self, territory: u16) -> Option<&TerritoryWar> {
        self.wars
            .iter()
            .find(|war| war.territory == territory && war.result.is_none())
    }

    /// The raid of the war side `character` is on the roster of, and its raid group (its roster
    /// group, [`super::raids`]); `None` for a player on no roster of a war without a result.
    pub(super) fn raid_seat(&self, character: Uuid) -> Option<(RaidSpec, u8)> {
        let (war, side) = self.own_war(character)?;
        let (group, _) = war
            .signups
            .iter()
            .find(|signup| signup.character == character && !signup.kicked)?
            .roster
            .place?;
        Some((war_raid(war, u32::from(side)), group))
    }

    /// The war without a result `character` signed up for, and its side.
    fn own_war(&self, character: Uuid) -> Option<(&TerritoryWar, u8)> {
        self.wars
            .iter()
            .filter(|war| war.result.is_none())
            .find_map(|war| {
                war.signups
                    .iter()
                    .find(|signup| signup.character == character && !signup.kicked)
                    .map(|signup| (war, signup.side))
            })
    }

    /// The war of the board a player of `character` uses: the war of the board's territory, or
    /// the war it signed up for.
    fn board_war(&self, board: Option<u16>, character: Uuid) -> Option<&TerritoryWar> {
        board
            .and_then(|territory| self.open_war(territory))
            .or_else(|| self.own_war(character).map(|(war, _)| war))
    }

    /// Asks for a war of the company named `attacker` against the owner of `territory`, from
    /// Unix ms `starts_ms`, for the game master of `answer_to`.
    pub(super) fn declare(
        &mut self,
        territory: u16,
        attacker: String,
        starts_ms: u64,
        answer_to: ConnectionRef,
    ) {
        self.jobs.push(WarJob::Declare {
            territory,
            attacker,
            starts_ms,
            answer_to,
        });
    }

    /// Asks for the result of the war of `territory`: the attacker won (`attacker`) or the
    /// defender, at Unix ms `now_ms`; the refusal line when it has no war without a result.
    pub(super) fn resolve(
        &mut self,
        territory: u16,
        attacker: bool,
        now_ms: u64,
        answer_to: ConnectionRef,
    ) -> Result<(), &'static str> {
        let war = self.open_war(territory).ok_or(NO_WAR)?;
        let winner = if attacker {
            war.attacker.0
        } else {
            war.defender.0
        };
        let job = WarJob::Resolve {
            war: war.war,
            winner,
            at_ms: now_ms,
            answer_to: Some(answer_to),
        };
        self.jobs.push(job);
        Ok(())
    }

    /// Asks to take back the war of `territory`.
    pub(super) fn cancel(&mut self, territory: u16, answer_to: ConnectionRef) {
        self.jobs.push(WarJob::Cancel {
            territory,
            answer_to,
        });
    }

    /// The war data member a client with the view `view` needs at Unix ms `now_ms`: the whole
    /// list when the wars or a phase changed since it got them. Sets the view.
    pub(super) fn member_due(&self, view: &mut WarView, now_ms: u64) -> Option<BundleMember> {
        let phases: Vec<u32> = self
            .wars
            .iter()
            .map(|war| phase_of(war, now_ms).0)
            .collect();
        let shown = (self.version, phases);
        if view.data.as_ref() == Some(&shown) {
            return None;
        }
        let version = FIRST_VERSION.saturating_add(self.version);
        let first = view.data.is_none();
        view.data = Some(shown);
        let state = WarDataState {
            wars: Some(IndexedList::Full(FullList {
                version: Some(version),
                elements: self
                    .wars
                    .iter()
                    .map(|war| war_entry(war, now_ms, self.world))
                    .collect(),
            })),
            // Inferred: no siege window change and no influence race runs here; the first state
            // carries them empty, as retail's carries all three children.
            schedule: first.then(|| KeyedList::full(version, Vec::new())),
            influence: first.then(|| KeyedList::full(version, Vec::new())),
        };
        Some(BundleMember {
            component: WAR_DATA_COMPONENT,
            state: MemberState::WarData(Box::new(state)),
        })
    }

    /// The territory interactor member of `character` when it changed since the view.
    fn interactor_due(&self, view: &mut WarView, character: Uuid) -> Option<BundleMember> {
        let state = interactor_state(&self.wars, character);
        if view.interactor.as_ref() == Some(&state) {
            return None;
        }
        view.interactor = Some(state.clone());
        Some(BundleMember {
            component: INTERACTOR_COMPONENT,
            state: MemberState::TerritoryInteractor(Box::new(state)),
        })
    }
}

/// A roster call's change of the seats of `war`, or `None` when it does not fit the roster.
fn roster_changes(
    war: &TerritoryWar,
    side: u8,
    call: &WarSignupClientMsg,
) -> Option<Vec<SeatChange>> {
    let signup_of = |player: &WarPlayer| {
        let character = Uuid::parse_str(&player.character).ok()?;
        war.signups
            .iter()
            .find(|signup| signup.character == character && signup.side == side && !signup.kicked)
    };
    let seated_at = |player: &WarPlayer, place: RosterPlace| {
        let signup = signup_of(player)?;
        let at = (
            u8::try_from(place.group).ok()?,
            u8::try_from(place.slot).ok()?,
        );
        (signup.roster.place == Some(at)).then_some(signup)
    };
    let free_slot = |group: u32| {
        let group = u8::try_from(group)
            .ok()
            .filter(|group| *group < ROSTER_GROUPS)?;
        (0..GROUP_SLOTS)
            .find(|slot| {
                !war.signups.iter().any(|signup| {
                    signup.side == side && signup.roster.place == Some((group, *slot))
                })
            })
            .map(|slot| (group, slot))
    };
    let seat = |signup: &WarSignup, place: Option<(u8, u8)>| SeatChange {
        character: signup.character,
        seat: RosterSeat {
            place,
            permission: signup.roster.permission,
        },
        kicked: false,
    };
    match call {
        WarSignupClientMsg::AddToRoster { player, group, .. } => {
            let signup = signup_of(player).filter(|signup| signup.roster.place.is_none())?;
            Some(vec![seat(signup, Some(free_slot(*group)?))])
        }
        WarSignupClientMsg::RemoveFromRoster { player, place, .. } => {
            Some(vec![seat(seated_at(player, *place)?, None)])
        }
        WarSignupClientMsg::SetPermissions {
            player,
            place,
            permission,
            ..
        } => {
            let signup = seated_at(player, *place)?;
            (*permission <= PERMISSION_LEADER).then_some(())?;
            Some(vec![SeatChange {
                character: signup.character,
                seat: RosterSeat {
                    place: signup.roster.place,
                    permission: *permission,
                },
                kicked: false,
            }])
        }
        WarSignupClientMsg::SwapPlayers { first, second, .. } => {
            let one = seated_at(&first.0, first.1)?;
            let other = seated_at(&second.0, second.1)?;
            (one.character != other.character).then_some(())?;
            Some(vec![
                seat(one, other.roster.place),
                seat(other, one.roster.place),
            ])
        }
        WarSignupClientMsg::MovePlayer {
            player,
            place,
            group,
            ..
        } => {
            let signup = seated_at(player, *place)?;
            (u32::from(signup.roster.place?.0) != *group).then_some(())?;
            Some(vec![seat(signup, Some(free_slot(*group)?))])
        }
        WarSignupClientMsg::KickPlayer { player, .. } => {
            let signup = signup_of(player)?;
            Some(vec![SeatChange {
                character: signup.character,
                seat: RosterSeat::default(),
                kicked: true,
            }])
        }
        _ => None,
    }
}

/// Whether the player `character` of `company` may edit the roster of `side` of `war`
/// (module doc).
fn edits_roster(war: &TerritoryWar, side: u8, character: Uuid) -> bool {
    let company = side_company(war, u32::from(side)).map(|(id, _)| id);
    war.signups
        .iter()
        .find(|signup| signup.character == character && signup.side == side && !signup.kicked)
        .is_some_and(|signup| permission_of(signup, company) >= PERMISSION_OFFICER)
}

impl SimWorld {
    /// Carries out the war sign-up or roster call `call` of the client of `connection`.
    pub fn war_signup_call(&mut self, connection: &ConnectionRef, call: WarSignupClientMsg) {
        let Some(&entity) = self.by_connection.get(connection) else {
            return;
        };
        let now_ms = self.unix_ms();
        let header = call.header();
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let character = player.character;
        let faction = player.faction.faction;
        let gear_score = player
            .gear
            .as_ref()
            .map_or(0, super::gear::Gear::mean_worn_gear_score);
        let mut answers: Vec<ServerMessage> = Vec::new();
        match &call {
            WarSignupClientMsg::RequestSignup {
                side, territory, ..
            } => {
                player.war.board = Some(*territory);
                let war = self.wars.open_war(*territory).filter(|war| {
                    phase_of(war, now_ms).0 == WAR_PHASE_SCHEDULED
                        && side_company(war, *side)
                            .is_some_and(|(_, side_faction)| side_faction == faction)
                });
                match war {
                    Some(war) => self.wars.jobs.push(WarJob::SignUp {
                        war: war.war,
                        character,
                        side: *side,
                        territory: *territory,
                        gear_score,
                        header,
                        answer_to: connection.clone(),
                    }),
                    None => answers.push(ServerMessage::WarSignup(
                        WarSignupServerMsg::SignupResponseReceived {
                            header,
                            result: CallResult::refused(REASON_NONE),
                        },
                    )),
                }
            }
            WarSignupClientMsg::RequestLeave { territory, .. } => {
                player.war.board = Some(*territory);
                match self.wars.open_war(*territory) {
                    Some(war) => self.wars.jobs.push(WarJob::Leave {
                        war: war.war,
                        character,
                        territory: *territory,
                        header,
                        answer_to: connection.clone(),
                    }),
                    None => answers.push(ServerMessage::WarSignup(
                        WarSignupServerMsg::LeaveResponseReceived {
                            header,
                            territory: *territory,
                            result: CallResult::refused(REASON_NONE),
                        },
                    )),
                }
            }
            WarSignupClientMsg::RequestSignupList { side, .. } => {
                let entries = self
                    .wars
                    .board_war(player.war.board, character)
                    .map(|war| {
                        war.signups
                            .iter()
                            .filter(|signup| u32::from(signup.side) == *side && !signup.kicked)
                            .map(|signup| SignupEntry {
                                player: war_player(signup),
                                company: signup
                                    .company
                                    .map_or(NO_ID, |(company, _)| *company.as_bytes()),
                                gear_score: signup.gear_score,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                answers.push(ServerMessage::WarSignup(
                    WarSignupServerMsg::SignupListReceived {
                        header,
                        side: *side,
                        // Observed: equal to the side in every retail body.
                        extra: *side,
                        entries,
                    },
                ));
            }
            WarSignupClientMsg::RequestSignupCount { side, .. } => {
                let count = self
                    .wars
                    .board_war(player.war.board, character)
                    .map_or(0, |war| {
                        war.signups
                            .iter()
                            .filter(|signup| u32::from(signup.side) == *side && !signup.kicked)
                            .count()
                    });
                answers.push(ServerMessage::WarSignup(
                    WarSignupServerMsg::SignupCountReceived {
                        header,
                        side: *side,
                        count: u32::try_from(count).unwrap_or(u32::MAX),
                    },
                ));
            }
            WarSignupClientMsg::RequestRemoteInteract { territory, .. } => {
                player.war.board = Some(*territory);
                let side = self.wars.open_war(*territory).and_then(|war| {
                    war.signups
                        .iter()
                        .find(|signup| signup.character == character && !signup.kicked)
                        .map(|signup| u32::from(signup.side))
                });
                answers.push(ServerMessage::TerritoryFacetReady(TerritoryFacetReadyMsg {
                    header,
                    territory: *territory,
                    access: side.unwrap_or(NO_SIDE),
                }));
            }
            WarSignupClientMsg::RequestRoster { .. } => {
                // Observed: no answer message; the client gets the roster state again.
                player.war.interactor = None;
                if let Some(member) = self.wars.interactor_due(&mut player.war, character) {
                    player.extra_members.push(member);
                }
            }
            roster_call => {
                let changes = self.wars.own_war(character).and_then(|(war, side)| {
                    edits_roster(war, side, character)
                        .then(|| roster_changes(war, side, roster_call))
                        .flatten()
                        .map(|changes| (war.war, changes))
                });
                match changes {
                    Some((war, changes)) => self.wars.jobs.push(WarJob::Roster {
                        war,
                        changes,
                        header,
                        answer_to: connection.clone(),
                    }),
                    None => answers.push(roster_answer(
                        header,
                        CallResult::refused(REASON_ROSTER_REFUSED),
                    )),
                }
            }
        }
        send_now(&mut self.combat_messages, connection, &answers);
    }

    /// Takes the answers of the war jobs and the wars read after them: the players get their
    /// answers, and each player whose sign-ups or roster changed gets its territory interactor
    /// state at its next tick.
    pub(super) fn wars_done(
        &mut self,
        answers: Vec<WarAnswer>,
        read: Result<Vec<TerritoryWar>, DbError>,
    ) {
        let replaced = self.wars.read(read);
        self.send_pre_raid_statuses(&replaced);
        self.invite_late_roster_players(&replaced);
        for answer in answers {
            match answer {
                WarAnswer::Quiet => {}
                WarAnswer::Gm(connection, line) => {
                    self.answer_gm_chat(&connection, vec![line.to_owned()]);
                }
                WarAnswer::Player(connection, messages) => {
                    send_now(&mut self.combat_messages, &connection, &messages);
                }
            }
        }
        let entities: Vec<_> = self.by_connection.values().copied().collect();
        for entity in entities {
            if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                let character = player.character;
                if let Some(member) = self.wars.interactor_due(&mut player.war, character) {
                    player.extra_members.push(member);
                }
            }
        }
    }

    /// The war jobs to run at Unix ms `now_ms` ([`Wars::take_jobs`]).
    pub(super) fn take_war_jobs(&mut self) -> Vec<WarJob> {
        let now_ms = self.unix_ms();
        self.wars.take_jobs(now_ms)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use nw_db::WAR_LENGTH_MS;
    use nw_messages::social::raid_data::EVENT_RAID_GROUPS;
    use nw_messages::war_roster::{group_child, player_child};

    /// Made-up companies, characters and a war of territory 8.
    const ATTACKER: Uuid = Uuid::from_u128(0xa1);
    const DEFENDER: Uuid = Uuid::from_u128(0xd1);
    const LEADER: Uuid = Uuid::from_u128(0x11);
    const MEMBER: Uuid = Uuid::from_u128(0x12);
    const OTHER: Uuid = Uuid::from_u128(0x13);
    const TERRITORY: u16 = 8;
    const STARTS_MS: u64 = 1_783_537_200_000;
    const ATTACKER_FACTION: u8 = 2;
    const DEFENDER_FACTION: u8 = 1;

    pub(in crate::world) fn signup(
        character: Uuid,
        rank: Option<u8>,
        place: Option<(u8, u8)>,
    ) -> WarSignup {
        WarSignup {
            character,
            name: "Testa".to_owned(),
            company: rank.map(|rank| (ATTACKER, rank)),
            side: 1,
            gear_score: 800,
            roster: RosterSeat {
                place,
                permission: PERMISSION_MEMBER,
            },
            kicked: false,
        }
    }

    pub(in crate::world) fn war(signups: Vec<WarSignup>) -> TerritoryWar {
        TerritoryWar {
            war: Uuid::from_u128(0x77),
            territory: TERRITORY,
            attacker: (ATTACKER, ATTACKER_FACTION),
            defender: (DEFENDER, DEFENDER_FACTION),
            number: 4714,
            starts_ms: STARTS_MS,
            result: None,
            signups,
        }
    }

    fn player(character: Uuid) -> WarPlayer {
        WarPlayer {
            character: character.hyphenated().to_string(),
            name: "Testa".to_owned(),
            kind: WAR_PLAYER_KIND,
        }
    }

    const HEADER: FacetHeader = FacetHeader {
        target: 0,
        entity: 0,
    };

    /// Observed timing (`arvaysa_ES_attacker_war_45vs45`): scheduled with the start time, then
    /// running with the end 1,800 s later, then resolved with the result's end 180 s later.
    #[test]
    fn the_phase_follows_the_clock_and_the_result() {
        let mut entry = war(Vec::new());
        assert_eq!(
            phase_of(&entry, STARTS_MS - 1),
            (WAR_PHASE_SCHEDULED, STARTS_MS)
        );
        assert_eq!(
            phase_of(&entry, STARTS_MS),
            (WAR_PHASE_RUNNING, STARTS_MS + WAR_LENGTH_MS)
        );
        entry.result = Some((ATTACKER, STARTS_MS + 1000));
        assert_eq!(
            phase_of(&entry, STARTS_MS + 2000),
            (WAR_PHASE_RESOLVED, STARTS_MS + 1000 + WAR_RESULT_SHOWN_MS)
        );
        let shown = war_entry(&entry, STARTS_MS + 2000, WORLD);
        assert_eq!(shown.status.winner, *ATTACKER.as_bytes());
        assert_eq!(shown.defender.number, shown.attacker.number + 1);
        // Each side names its raid: ten groups, the raid entity and its facet key.
        let attacker = war_raid(&entry, SIDE_ATTACKER);
        assert_eq!(shown.attacker.number, attacker.raid);
        assert_eq!(attacker.opposing, shown.defender.number);
        assert_eq!(shown.attacker.groups.len(), EVENT_RAID_GROUPS);
        assert_eq!(shown.attacker.raid, attacker.entity);
        assert_eq!(shown.attacker.key, facet_of_raw_id(&attacker.entity));
        assert_ne!(shown.attacker.raid, shown.defender.raid);
        assert_eq!(shown.attacker_faction, ATTACKER_FACTION);
    }

    /// A made-up world value.
    const WORLD: u32 = 0x5eed_0001;

    /// Observed (`arvaysa_ES_attacker_war_45vs45`): the game mode field is 0 while the war waits
    /// and the world's value once it runs and after its result.
    #[test]
    fn the_game_mode_field_is_the_world_value_from_the_start() {
        let mut entry = war(Vec::new());
        assert_eq!(
            war_entry(&entry, STARTS_MS - 1, WORLD).status.game_mode,
            NO_GAME_MODE
        );
        assert_eq!(war_entry(&entry, STARTS_MS, WORLD).status.game_mode, WORLD);
        entry.result = Some((DEFENDER, STARTS_MS + 1000));
        assert_eq!(
            war_entry(&entry, STARTS_MS + 2000, WORLD).status.game_mode,
            WORLD
        );
        let mut wars = Wars::of_world(WORLD);
        wars.read(Ok(vec![war(Vec::new())]));
        let Some(BundleMember {
            state: MemberState::WarData(state),
            ..
        }) = wars.member_due(&mut WarView::default(), STARTS_MS)
        else {
            panic!("war data");
        };
        let Some(IndexedList::Full(list)) = &state.wars else {
            panic!("full list");
        };
        assert_eq!(list.elements[0].status.game_mode, WORLD);
    }

    /// A roster player's raid is its side's, in its roster group; a sign-up off the roster has
    /// none.
    #[test]
    fn a_roster_player_joins_its_side_raid_in_its_roster_group() {
        let on_roster = Uuid::from_u128(0x501);
        let off_roster = Uuid::from_u128(0x502);
        let mut wars = Wars::of_world(WORLD);
        wars.read(Ok(vec![war(vec![
            signup(on_roster, None, Some((3, 1))),
            signup(off_roster, None, None),
        ])]));
        let (raid, group) = wars.raid_seat(on_roster).expect("a raid");
        assert_eq!(group, 3);
        assert_eq!(raid.raid, side_number(&war(Vec::new()), SIDE_ATTACKER));
        assert_eq!(raid.kind, RAID_TYPE_WAR);
        assert_eq!(raid, war_raid(&war(Vec::new()), SIDE_ATTACKER));
        assert!(wars.raid_seat(off_roster).is_none());
    }

    /// A war won in its instance asks for the winner's result once, and its end of time then
    /// asks for no other.
    #[test]
    fn a_war_won_in_its_instance_gets_that_result_once() {
        let mut wars = Wars::default();
        wars.read(Ok(vec![war(Vec::new())]));
        let id = Uuid::from_u128(0x77);
        assert_eq!(wars.war(id).map(|war| war.territory), Some(TERRITORY));
        wars.win(id, ATTACKER, STARTS_MS + 1);
        wars.win(id, ATTACKER, STARTS_MS + 2);
        let jobs = wars.take_jobs(STARTS_MS + WAR_LENGTH_MS);
        assert!(matches!(
            jobs.as_slice(),
            [WarJob::Resolve { winner, at_ms, .. }] if *winner == ATTACKER && *at_ms == STARTS_MS + 1
        ));
    }

    /// A war whose time ran out gets the defender's result once.
    #[test]
    fn a_war_past_its_end_goes_to_the_defender_once() {
        let mut wars = Wars::default();
        wars.read(Ok(vec![war(Vec::new())]));
        assert!(wars.take_jobs(STARTS_MS).is_empty());
        let end = STARTS_MS + WAR_LENGTH_MS;
        let jobs = wars.take_jobs(end);
        assert!(matches!(
            jobs.as_slice(),
            [WarJob::Resolve { winner, at_ms, answer_to: None, .. }] if *winner == DEFENDER && *at_ms == end
        ));
        assert!(wars.take_jobs(end + 1).is_empty());
    }

    /// The governor leads and edits; a member does not; an add takes the first free slot; a
    /// swap trades places; a kick clears the seat.
    #[test]
    fn roster_rules() {
        let entry = war(vec![
            signup(LEADER, Some(RANK_GOVERNOR), Some((0, 0))),
            signup(MEMBER, Some(3), Some((0, 1))),
            signup(OTHER, None, None),
        ]);
        assert!(edits_roster(&entry, 1, LEADER));
        assert!(!edits_roster(&entry, 1, MEMBER));
        assert!(!edits_roster(&entry, 2, LEADER), "not on that side");
        let add = WarSignupClientMsg::AddToRoster {
            header: HEADER,
            player: player(OTHER),
            group: 0,
        };
        let changes = roster_changes(&entry, 1, &add).expect("fits");
        assert_eq!(changes[0].seat.place, Some((0, 2)));
        let bad_group = WarSignupClientMsg::AddToRoster {
            header: HEADER,
            player: player(OTHER),
            group: u32::from(ROSTER_GROUPS),
        };
        assert_eq!(roster_changes(&entry, 1, &bad_group), None);
        let swap = WarSignupClientMsg::SwapPlayers {
            header: HEADER,
            first: (player(LEADER), RosterPlace { group: 0, slot: 0 }),
            second: (player(MEMBER), RosterPlace { group: 0, slot: 1 }),
        };
        let changes = roster_changes(&entry, 1, &swap).expect("fits");
        assert_eq!(changes[0].seat.place, Some((0, 1)));
        assert_eq!(changes[1].seat.place, Some((0, 0)));
        let self_swap = WarSignupClientMsg::SwapPlayers {
            header: HEADER,
            first: (player(LEADER), RosterPlace { group: 0, slot: 0 }),
            second: (player(LEADER), RosterPlace { group: 0, slot: 0 }),
        };
        assert_eq!(
            roster_changes(&entry, 1, &self_swap),
            None,
            "retail refused it"
        );
        let kick = WarSignupClientMsg::KickPlayer {
            header: HEADER,
            player: player(MEMBER),
        };
        let changes = roster_changes(&entry, 1, &kick).expect("fits");
        assert!(changes[0].kicked);
        assert_eq!(changes[0].seat.place, None);
    }

    /// The interactor state holds the player's sign-up and its side's 50 slots and 10 groups,
    /// with the governor as the war leader.
    #[test]
    fn the_interactor_state_shows_the_roster() {
        let wars = vec![war(vec![signup(LEADER, Some(RANK_GOVERNOR), Some((2, 4)))])];
        let state = interactor_state(&wars, LEADER);
        assert_eq!(
            state.players.len(),
            usize::from(ROSTER_GROUPS * GROUP_SLOTS)
        );
        assert_eq!(state.groups.len(), usize::from(ROSTER_GROUPS));
        let (_, seated) = state
            .players
            .iter()
            .find(|(place, _)| *place == (2, 4))
            .expect("seated");
        assert_eq!(seated.permission, PERMISSION_LEADER);
        assert_eq!(player_child(2, 4), 17);
        assert_eq!(group_child(2), 18);
        let Some(KeyedList::Full(signups)) = &state.signups else {
            panic!("full sign-ups");
        };
        assert_eq!(signups.elements[0].0, TERRITORY);
        assert_eq!(signups.elements[0].1.on_roster, 1);
        assert_eq!(
            interactor_state(&wars, OTHER).players.len(),
            0,
            "no roster without a sign-up"
        );
    }

    /// The war data goes out once per change of the wars or of a phase.
    #[test]
    fn the_war_data_goes_out_on_a_change() {
        let mut wars = Wars::default();
        wars.read(Ok(vec![war(Vec::new())]));
        let mut view = WarView::default();
        assert!(wars.member_due(&mut view, STARTS_MS - 1).is_some());
        assert!(wars.member_due(&mut view, STARTS_MS - 1).is_none());
        let Some(member) = wars.member_due(&mut view, STARTS_MS) else {
            panic!("the phase changed");
        };
        let MemberState::WarData(state) = member.state else {
            panic!("war data");
        };
        assert_eq!(
            state.schedule, None,
            "only the first state has the empty lists"
        );
    }
}
