// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Outpost Rush match on its instance sim (Outpost Rush study 2026-11-12,
//! `local-data/findings/2026-11-12-outpost-rush.md`): the game mode entity (2343), the players'
//! participant states (3312), the three outposts, the score, the kills, the respawn, the end and
//! the return.
//!
//! Observed (retail `opr_que_and_lost` on `NW_OPR_005_Vertical`, the whole match from the
//! queue; `OPR-coral-divide2` on `NW_OPR_004_Trench` and `OPR_TEST3` on the open world map,
//! both joined into a running match; times in seconds from the ledger's first record):
//!
//! - The game mode entity: a card with no asset and the game mode state: the mode `91b99a18`, the
//!   map's CRC, each participant with status 3 and its team, an empty character list, one raid id
//!   per team (two in a row), the values and the timers `GameModeTimeout` (+3 h) and
//!   `PregameTimer` (+90 s): script state 2 (`opr_que_and_lost` 578.634).
//! - At `PregameTimer` the script state turns to 0 (665.446) with `TimeLimit` 2,700 s on (3 of 3
//!   ledgers); 3 s later `TeamScores` 0 and the value `e78c48f6` 1 (668.478).
//! - The outposts are the values `CapturePointA_Status`, `CapturePointB_Status` and
//!   `CapturePointC_Status` (the names of the client slice `GameMode_OutpostRush_005_Vertical`;
//!   their CRC-32 are the keys): the owner team in the low byte, the capturing team in the
//!   second, the progress in the third, `ff` for no team; `ffff` at the start. The progress moves
//!   about 2 a second (683.712 to 733.667: 0 to 98), falls when the other team pushes it back,
//!   and at its end the capturing team owns the outpost with no capturer and progress 0 (734.177);
//!   an owned outpost goes straight to the other team (`06a8de5f` 1108.472 to 1151.305).
//! - `TeamScores` holds team 1's score in the high half and team 0's in the low half. Every 3 s
//!   each team gains one point per outpost it owns (734.879 to 751.113: 1 a tick with one
//!   outpost, 2 with two; 1109.403 to 1148.538: 3 with all three); a kill gives the killer's
//!   team 1 point (the foe team's score rose by 1 within 60 ms of 3 of 3 own deaths).
//! - While one team owns all three outposts the value `e78c48f6` rises: 2 after 60 s, then 1 more
//!   every 30 s (`OPR-coral-divide2` 249.857 to 309.856, 340.036, 370.203, ...; and 532.366 to
//!   593.549), and a tick gives that team its outposts times the value (309.856: 6 a tick); it is
//!   1 again when the team loses an outpost (451.736).
//! - The first team with 1,000 points wins (`opr_que_and_lost` 2105.783: team 1 at `3e8`,
//!   `OPR_TEST3` 1851.453): script state 1, `WinningTeamIdx`, `Cleanup` 180 s on.
//! - Statuses: 3 alive, `0x20` set by a ready-up (6824) in the pregame and kept (636.312), `0x04`
//!   while dead (`07`, `27`).
//! - The participant flags are `13ca61080000` for the whole match (3 of 3 ledgers). At its
//!   arrival the player gets the temporary affiliations of its team (own team kind 0, the other
//!   kind 4: `opr_que_and_lost` 577.371 s, `OPR-coral-divide2` 101.950 s, `OPR_TEST3`
//!   215.432 s), emptied at the match's end (2105.799 s, 660.032 s, 1851.453 s).
//! - The island (the open world map) runs the same match: the same mode and values, its
//!   outposts the entities `OR_Outpost_A_LUNA`, `OR_Outpost_B_SOL` and `OR_Outpost_C_STAR` of its
//!   layout slice `POIs/OutpostRush/OutpostRush_Layout_00` (the same `radius` 21.35 and heights
//!   18.0, 16.5 and 16.0) at their slice places plus the slice's open world capital (1272.7565,
//!   11016.635, 68.71661; the boss and the barriers of the island stood there to the
//!   centimetre); its map id is the mode's (`91b99a18`, `OPR_TEST3` 216.746 s); its own extras
//!   are the Baroness's barriers.
//! - Each map has the Baroness ([`super::outpost_rush_boss`]) and the event feed
//!   ([`super::outpost_rush_events`]): the first state holds `armorybuilt` for each outpost.
//! - The client's 1980 `Leave` is answered as in the arena: 3455 (`0`), 5283 (own character),
//!   3951 and the move back (`OPR-coral-divide2` 674.171, `OPR_TEST3` 1862.570).
//! - A dead player asks for its respawn (1938) 8 to 30 s after its death, at one respawn point
//!   per match (the same point id in every request of a ledger).
//!
//! - The capture rule (game mode helper B13, findings `2026-11-13-outpost-rush-shed-and-points.md`):
//!   the capture point slices hold `m_captureTime` 60 s, `m_captureTimeDecreasePerPlayer` 10 s,
//!   `m_captureTimeMinimum` 10 s and `m_captureMaxPlayerDelta` 5; retail captures took 61, 51,
//!   41, 30 to 32 and 11 to 19 s (7-step windows of all 8 Trench and Vertical ledgers); the
//!   progress shown is a whole number (steps 2, 2, 1 for a 60 s capture); a built Protection Ward
//!   of the owner stops the capture ([`capture_points`]).
//!
//! Inferred: the capture rule (each second the team with more living players in the outpost
//! leads by the difference; a capture takes `60 - 10 x min(lead - 1, 5)` s; the owner's lead
//! takes another team's progress back at the same rate; an equal count holds it; an empty
//! outpost falls back 1 a second, as the retail falls of about 102 s); the outpost places on a
//! map (the slice's places from the map's middle, 1024, 1024: Observed exactly for the B outposts of both maps); the capture area (the slice's
//! `radius` 21.35 m around the outpost, its `height` above and below); PvP between the two teams
//! while the match runs; the respawn at the team's spot as soon as the client asks; at
//! `TimeLimit` the team with more points wins (no capture ran that long), and a tie has no
//! winner; a team with no participant left loses; the players still in the match at the end of
//! `Cleanup` go back; a leave of a running match bars the queue as the arena's does.
//!
//! - The outpost structures (gates, command posts, turrets, wards, storage sheds, armories and
//!   the AI objective managers) spawn with the match, are built, upgraded, ruined and repaired
//!   ([`super::outpost_rush_structures`]); their outpost statuses are the values
//!   `OutpostStructureStatusesA` to `C`. A built turret is used, fired and taken as a war
//!   turret ([`super::turrets`]).
//!
//! - The summoning circles, the summons, the corrupted portal with its timers `7a0740b0` and
//!   `5543c08f` and its events, and the end statistics `bearsummoncounts`,
//!   `brutesummoncounts` and `wraithsummoncounts` ([`super::outpost_rush_summons`]).
//!
//! - The capture point entities (333) show each outpost's capture ([`capture_points`]); the
//!   storage sheds hold their team's match items ([`storage_shed`]); the armory shop opens for
//!   the owner team ([`SimWorld::armory_status`]).
//!
//! Not built: a backfill into a running match.
//!
//! The war board (839, 3530, 2968) of the two teams runs with the match ([`board`]). The end
//! statistics of the outposts are built ([`capture_counts`]).

use bevy_ecs::entity::Entity;
use nw_bus::ArenaSeat;
use nw_content::instances::{InstanceKind, InstanceMap};
use nw_content::outpost_structures::StructureRole;
use nw_messages::game_mode_participant::PARTICIPANT_FLAGS_LEN;
use nw_messages::game_mode_state::{GameModeState, Keyed};
use nw_messages::list_delta::{ListChange, ReplicatedList};
use nw_messages::wire::RAW_ID_LEN;
use nw_sim_core::Position;

use super::arena_match::{
    CLEANUP_TIMER, GAME_MODE_TIMEOUT, GAME_MODE_TIMEOUT_MS, LEAVER_PENALTY_NS, PREGAME_MS,
    PREGAME_TIMER, RAID_ID_BASE, READY_BIT, WINNING_TEAM, middle_of,
};
use super::game_mode_entity::team_affiliations;
use super::game_mode_entity::{
    FIRST_LIST_VERSION, MatchEntry, MatchParticipant, PLAYING_STATUS, full, participant_lists,
};
use self::board::Board;
use self::capture_points::CapturePoints;
use self::storage_shed::StorageSheds;
use super::outpost_rush_boss::BaronessRun;
use super::outpost_rush_doors::{FortBarrier, ISLAND_FOLDER};
use super::outpost_rush_events::{
    ARMORY_BUILT, AttackWatch, EventFeed, OUTPOST_ATTACKED, command_post_events,
};
use super::camps::APPLY_ALL_RESOURCES_KIND;
use super::outpost_rush_structures::{OutpostStructures, RUIN_BIT, UPGRADE_BUILDABLE_KIND};
use super::outpost_rush_summons::SummonRun;
use super::turrets::{USE_ACCEPTED, USE_REFUSED};
use super::queue::team_spot;
use super::{Body, Player, SimWorld, unix_ns};

/// The script states (Observed).
const RUNNING: u8 = 0;
const ENDED: u8 = 1;
const PREGAME: u8 = 2;
/// `TimeLimit`: the match's end, from the start of state 0 (Observed: 2,700 s in 3 of 3).
pub(super) const TIME_LIMIT_TIMER: u32 = 0x8f0e_9156;
const TIME_LIMIT_MS: u64 = 2_700_000;
/// `Cleanup` after the end (Observed: 180 s, `opr_que_and_lost` 2105.783 s).
const CLEANUP_MS: u64 = 180_000;
/// The value keys: `TeamScores`, the three outposts (CRC-32 of the lowercase names), and the
/// score multiplier of the team that owns every outpost (name Unknown; meaning Inferred from the
/// values).
const TEAM_SCORES: u32 = 0x4a6e_9282;
const OUTPOST_A_STATUS: u32 = 0x448d_d922;
const OUTPOST_B_STATUS: u32 = 0xca02_dec1;
const OUTPOST_C_STATUS: u32 = 0x06a8_de5f;
const DOMINATION_MULTIPLIER: u32 = 0xe78c_48f6;
/// The end statistics of the outposts A, B and C: `OutpostACaptureCounts`,
/// `OutpostBCaptureCounts` and `OutpostCCaptureCounts` (the keys are the CRC-32 of the lowercase
/// names, found in the client's `outpostrushhud.luac` words; Observed only in the end state of
/// every ledger that saw an end: `opr_que_and_lost` 2105.783 s, `OPR_TEST3` 1851.453 s,
/// `OPR-coral-divide2` 660.014 s and eight more).
const OUTPOST_CAPTURE_COUNTS: [u32; 3] = [0x89f1_06dc, 0xf590_2307, 0x689f_c271];
/// The structure statuses of the outposts A, B and C: `OutpostStructureStatusesA`, `..B` and
/// `..C` (CRC-32 of the lowercase names), three bits a structure in the client's
/// `OutpostRush_OutpostStructure` order (Armory first, then CommandPost, DefensiveStructure1-4,
/// Gate1-4, ProtectionWard, StorageShed, AIObjectiveManager): the tier in the low two bits, 4
/// for a ruin. Observed: each is 1 in the match's first state (the armory at tier 1, with the
/// three `armorybuilt` events; 8 of 8 ledgers that saw the opening, `opr_match_2` 0.793 s,
/// `OPR_TEST3` 216.746 s); the values of the match's structures
/// ([`super::outpost_rush_structures::OutpostStructures::statuses`]).
const OUTPOST_STRUCTURE_STATUSES: [u32; 3] = [0xe691_e6e4, 0x7f98_b75e, 0x089f_87c8];
/// The value of no team in an outpost status (Observed `ff`).
mod board;
mod capture_points;
mod storage_shed;

/// The interaction kind `OpenArmory` (Observed: the client's `InteractionUIActions`).
const OPEN_ARMORY_KIND: u8 = 78;
pub(super) const NO_TEAM: u8 = 0xff;
/// The bits of a team's score in `TeamScores` (Observed: team 1 high, team 0 low).
const SCORE_BITS: u32 = 16;
/// The shifts of an outpost status: the capturing team, the progress (Observed).
const CAPTURER_SHIFT: u32 = 8;
const PROGRESS_SHIFT: u32 = 16;
/// Points that win the match (Observed: `3e8` at the end of 2 matches).
const SCORE_TO_WIN: u32 = 1_000;
/// The score tick, and the points an owned outpost gives a tick (Observed).
const SCORE_TICK_MS: u64 = 3_000;
const OUTPOST_POINTS: u32 = 1;
/// The points of a kill for the killer's team (Observed).
const KILL_POINTS: u32 = 1;
/// The multiplier with no team owning every outpost, its first rise after the third outpost and
/// each later rise (Observed: 2 runs of `OPR-coral-divide2`).
const BASE_MULTIPLIER: u32 = 1;
const DOMINATION_FIRST_STEP_MS: u64 = 60_000;
const DOMINATION_STEP_MS: u64 = 30_000;
/// The capture tick (Observed: about one progress change a second).
const CAPTURE_TICK_MS: u64 = 1_000;
/// The capture point slices' `m_captureTime`, `m_captureTimeDecreasePerPlayer` and
/// `m_captureMaxPlayerDelta` (Observed, both point slices); the shortest capture is
/// `m_captureTimeMinimum` (10 s), which the most counted players reach.
const CAPTURE_TIME_MS: u64 = 60_000;
const CAPTURE_DECREASE_PER_PLAYER_MS: u64 = 10_000;
const CAPTURE_MAX_PLAYER_DELTA: usize = 5;
/// The progress units of one percent (the status shows whole percents, module doc).
const UNITS_PER_PERCENT: u32 = 1_000;
/// The progress that ends a capture: 100 percent (Inferred: the last retail value was 99).
const CAPTURE_DONE: u32 = 100 * UNITS_PER_PERCENT;
/// The fall of a tick in an empty outpost: 1 percent (Inferred, module doc).
const EMPTY_FALL: u32 = UNITS_PER_PERCENT;
/// The dead bit of a participant status (Observed `07` and `27`).
const DEAD_BIT: u8 = 0x04;
/// The participant flags of the whole match (Observed).
const MATCH_FLAGS: [u8; PARTICIPANT_FLAGS_LEN] = [0x13, 0xca, 0x61, 0x08, 0x00, 0x00];
/// The teams of a match.
pub(super) const TEAMS: usize = 2;
/// The namespace of the match entity's `GdeRef` ("oprmatch").
const MATCH_NAMESPACE: u64 = 0x6f70_726d_6174_6368;
/// The capture radius of an outpost (Observed: the field `radius` 21.35 of every outpost of the
/// client slices of both maps).
const CAPTURE_RADIUS_M: f32 = 21.35;

/// One outpost of a map: its status key, its place and the `height` of its capture area.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Outpost {
    key: u32,
    position: [f32; 3],
    height_m: f32,
}

/// The outposts of one map (Observed: the entities `OR_Outpost_A_LUNA`, `OR_Outpost_B_SOL` and
/// `OR_Outpost_C_STAR` of the client slices `GameMode_OutpostRush_005_Vertical` and
/// `OutpostRush_Layout_004_Trench`, their slice places plus the map's middle 1024, 1024; the
/// retail capture point of B stood at that place in both maps).
#[derive(Debug, Clone, Copy, PartialEq)]
struct OutpostMap {
    folder: &'static str,
    outposts: [Outpost; 3],
}

/// The folder of the Trench map (`NW_OPR_004_Trench`).
pub(super) const TRENCH_FOLDER: &str = "nw_opr_004_trench";
/// The folder of the Vertical map (`NW_OPR_005_Vertical`).
pub(super) const VERTICAL_FOLDER: &str = "nw_opr_005_vertical";

const OUTPOST_MAPS: [OutpostMap; 3] = [
    OutpostMap {
        folder: TRENCH_FOLDER,
        outposts: [
            Outpost {
                key: OUTPOST_A_STATUS,
                position: [981.46, 887.39, 13.5],
                height_m: 18.0,
            },
            Outpost {
                key: OUTPOST_B_STATUS,
                position: [1042.11, 1026.18, 16.0],
                height_m: 16.5,
            },
            Outpost {
                key: OUTPOST_C_STATUS,
                position: [1101.98, 887.4, 13.61],
                height_m: 16.0,
            },
        ],
    },
    OutpostMap {
        folder: VERTICAL_FOLDER,
        outposts: [
            Outpost {
                key: OUTPOST_A_STATUS,
                position: [962.38, 1077.42, 107.0],
                height_m: 18.0,
            },
            Outpost {
                key: OUTPOST_B_STATUS,
                position: [1023.75, 1024.1, 119.6],
                height_m: 16.5,
            },
            Outpost {
                key: OUTPOST_C_STATUS,
                position: [1086.71, 971.59, 107.0],
                height_m: 16.0,
            },
        ],
    },
    OutpostMap {
        folder: ISLAND_FOLDER,
        outposts: [
            Outpost {
                key: OUTPOST_A_STATUS,
                position: [1368.2405, 10875.8, 80.562_61],
                height_m: 18.0,
            },
            Outpost {
                key: OUTPOST_B_STATUS,
                position: [1272.0815, 11016.969, 65.561_61],
                height_m: 16.5,
            },
            Outpost {
                key: OUTPOST_C_STATUS,
                position: [1173.2115, 11155.818, 55.968_61],
                height_m: 16.0,
            },
        ],
    },
];

/// The bits of one structure in an outpost's structure status, the tier's bits among them
/// (Observed: [`super::outpost_rush_structures`], helper B10).
const STATUS_SLOT_BITS: u32 = 3;
const STATUS_SLOT_MASK: u64 = 0b111;
const STATUS_TIER_MASK: u64 = 0b011;

/// The outposts whose Protection Ward stands built and whole (its status shows a tier and no
/// ruin), which stops their capture ([`capture_points`]).
fn wards_standing(structures: &OutpostStructures) -> [bool; 3] {
    let Some(slot) = StructureRole::ProtectionWard.status_slot(0) else {
        return [false; 3];
    };
    structures.statuses().map(|status| {
        let code = (status >> (slot * STATUS_SLOT_BITS)) & STATUS_SLOT_MASK;
        code & STATUS_TIER_MASK != 0 && code & RUIN_BIT == 0
    })
}

/// The state of one outpost; `progress` in [`UNITS_PER_PERCENT`] of a percent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OutpostState {
    owner: u8,
    capturer: u8,
    progress: u32,
}

/// The progress a lead of `lead` players gives in one capture tick (module doc).
fn rise_of(lead: usize) -> u32 {
    let counted = lead.saturating_sub(1).min(CAPTURE_MAX_PLAYER_DELTA) as u64;
    let capture_ms = CAPTURE_TIME_MS - CAPTURE_DECREASE_PER_PLAYER_MS * counted;
    u32::try_from(u64::from(CAPTURE_DONE) * CAPTURE_TICK_MS / capture_ms).unwrap_or(CAPTURE_DONE)
}

impl OutpostState {
    const NEUTRAL: Self = Self {
        owner: NO_TEAM,
        capturer: NO_TEAM,
        progress: 0,
    };

    /// The whole percent the status and the capture point show.
    fn shown_progress(self) -> u8 {
        u8::try_from(self.progress / UNITS_PER_PERCENT).unwrap_or(u8::MAX)
    }

    /// The status value of the outpost.
    fn value(self) -> u64 {
        u64::from(self.owner)
            | u64::from(self.capturer) << CAPTURER_SHIFT
            | u64::from(self.shown_progress()) << PROGRESS_SHIFT
    }

    /// One capture tick with `present` living players of each team in the outpost; while
    /// `protected` (the owner's Protection Ward stands) a capture names its team and does not
    /// rise ([`capture_points`]).
    fn step(self, present: [usize; TEAMS], protected: bool) -> Self {
        let (pusher, lead) = match present[0].cmp(&present[1]) {
            std::cmp::Ordering::Greater => (Some(0), present[0] - present[1]),
            std::cmp::Ordering::Less => (Some(1), present[1] - present[0]),
            std::cmp::Ordering::Equal => (None, 0),
        };
        let back = |state: Self, fall: u32, idle_capturer: u8| {
            let progress = state.progress.saturating_sub(fall);
            Self {
                progress,
                capturer: if progress == 0 {
                    idle_capturer
                } else {
                    state.capturer
                },
                ..state
            }
        };
        match pusher {
            // An equal count of players holds the progress.
            None if present[0] > 0 => self,
            None => back(self, EMPTY_FALL, NO_TEAM),
            Some(team) if team == self.owner => back(self, rise_of(lead), NO_TEAM),
            Some(team) if self.capturer == team || self.capturer == NO_TEAM => {
                let rise = if protected { 0 } else { rise_of(lead) };
                let progress = self.progress.saturating_add(rise);
                if progress >= CAPTURE_DONE {
                    Self {
                        owner: team,
                        ..Self::NEUTRAL
                    }
                } else {
                    Self {
                        capturer: team,
                        progress,
                        ..self
                    }
                }
            }
            Some(team) => back(self, rise_of(lead), team),
        }
    }
}

/// The end events of a player's team (Observed, the 497 entry of each ledger's end): a win
/// `OutpostRush_T1` (`OPR-coral-divide2` 660.0 s, `opr_match_2` 381.0 s, `OPR---Tower-2`
/// 317.2 s, `OPR---ISLAND-2` 1408.8 s), a loss `OutpostRush_LOSS_T1` (`OPR_BAGGUNS` 932.4 s,
/// `OPR-Coral-Divide-2` 1308.2 s, `opr_match_4` 1245.3 s, `opr_que_and_lost` 2105.8 s). Two wins
/// gave a higher tier (`OPR---Tower` 325.4 s `_T2`, `OPR_TEST3` 1851.5 s `_T4`): retail's rule
/// that picks `_T2` to `_T4` is Unknown (it is server script; no client function or table names
/// the tiers). No value of the match picks the tier (helper B9, 2026-11-13,
/// all 13 Outpost Rush ledgers of the capture index: the end values, the war board 839 of each
/// player and the 497 entries): the `_T2` win (`OPR---Tower`: own board score 853, 1,001 to 329,
/// about 703 s) lies between `_T1` wins of a higher own score (`opr_match_2` 894,
/// `OPR---ISLAND-2` 1,981, `OPR-coral-divide2` 1,856), a closer result (`OPR---ISLAND-2` 1,002
/// to 393) and a longer match (`OPR---ISLAND-2` about 1,346 s, joined at its start); the `_T4`
/// win (`OPR_TEST3`: 6,441, rank 12 of 20 in its team, 1,000 to 716, about 1,664 s) is no top
/// rank either. No function of the client decompile names the tier rows; the 497 list's daily
/// bonuses are empty in every ledger. The tier rows `javelindata_gameevents` `OutpostRush_T1` to
/// `_T4` and their `javelindata_rewardmodifiers` rows hold amounts only (`UniversalExpAmount`
/// 1,000, 2,500, 5,000 and 10,000; `PvpXp` 500, 1,500, 2,000 and 2,500), no condition.
///
/// Inferred (player's design choice, 2026-11-13): a winner's tier rises one step for each
/// [`TIER_STEP_SCORE`] points of its own war board score ([`board`]), up to `_T4`: below 2,000
/// `_T1`, from 2,000 `_T2`, from 4,000 `_T3`, from 6,000 `_T4`. It rewards the play the board
/// scores (kills, assists, damage, healing, captures, deposits), and it fits 5 of the 6 retail
/// wins (`_T1` at 570, 894, 1,856 and 1,981; `_T4` at 6,441; not `OPR---Tower`'s `_T2` at 853).
/// Losses stay `_LOSS_T1` (Observed, every loss). A tie at the time limit (no ledger) is
/// `_LOSS_T1`, as no team won (Inferred).
const WIN_EVENTS: [&str; 4] = [
    "OutpostRush_T1",
    "OutpostRush_T2",
    "OutpostRush_T3",
    "OutpostRush_T4",
];
const LOSS_EVENT: &str = "OutpostRush_LOSS_T1";
/// The board score of one tier step (Inferred, player's design choice: module text above).
const TIER_STEP_SCORE: u32 = 2_000;
/// The game event a player gets when it joins the match: its loot table
/// `OutpostRush_Azoth_Grant` gives 50 of the match's azoth (Observed: the 497 entry with 50 of
/// item 324048019 at the first arrival of seven OPR ledgers, `opr_match_4` 0.6 s,
/// `opr_match_2` 0.7 s, `opr_que_and_lost` 577.4 s after its 5449, `OPR-coral-divide2`
/// 102.0 s, `OPR_TEST3` 215.4 s). Some ledgers show it again later at a level resend
/// (`opr_match_4` 153.9 and 618.1 s, not at 307.0, 464.8 or 770.0 s): the rule of those repeats
/// is Unknown, so a player gets it once, at its join.
const PRE_GAME_EVENT: &str = "OutpostRush_PreGameGrant";

/// The end event of a player of team `team` with the war board score `board_score` when
/// `winner` won (`None`: a tie).
fn end_event(team: u8, winner: Option<u8>, board_score: u32) -> &'static str {
    if winner != Some(team) {
        return LOSS_EVENT;
    }
    let steps = usize::try_from(board_score / TIER_STEP_SCORE).unwrap_or(usize::MAX);
    WIN_EVENTS[steps.min(WIN_EVENTS.len() - 1)]
}

/// `TeamScores` of `scores`.
pub(super) fn team_scores(scores: [u32; TEAMS]) -> u64 {
    u64::from(scores[1]) << SCORE_BITS | u64::from(scores[0])
}

/// The end statistics of the outposts: for each outpost the points each team scored from it,
/// packed as `TeamScores` (team 1 in the high half). Observed: in `opr_que_and_lost` team 1's
/// three values 98 + 339 + 444 = 881 of its 1,000 points and team 0's 339 + 107 + 5 = 451 of its
/// 535, the rest of each score the kills (`OPR_TEST3` and `OPR-coral-divide2` fit the same).
/// Inferred: a team the Baroness froze scores nothing, so its outposts count nothing then.
fn capture_counts(outpost_points: &[[u32; TEAMS]; 3]) -> [Keyed; 3] {
    std::array::from_fn(|index| {
        (
            OUTPOST_CAPTURE_COUNTS[index],
            team_scores(outpost_points[index]),
        )
    })
}

/// The multiplier of a team that owns every outpost since `held_ms` ms.
const fn domination_multiplier(held_ms: u64) -> u32 {
    if held_ms < DOMINATION_FIRST_STEP_MS {
        return BASE_MULTIPLIER;
    }
    let steps = (held_ms - DOMINATION_FIRST_STEP_MS) / DOMINATION_STEP_MS;
    // A match lasts at most `TIME_LIMIT_MS`: the step count is far below `u32::MAX`.
    BASE_MULTIPLIER + 1 + steps as u32
}

/// The running Outpost Rush match of this instance sim.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct OutpostRushMatch {
    /// The slot key of the game mode entity, and its raw id.
    entity: u64,
    gde_ref: [u8; RAW_ID_LEN],
    crc: u32,
    map: InstanceMap,
    outposts: [Outpost; 3],
    participants: Vec<MatchParticipant>,
    state: u8,
    /// The server ms of the current state's timer: `PregameTimer`, `TimeLimit` or `Cleanup`.
    ends_ms: u64,
    timeout_ms: u64,
    points: [OutpostState; 3],
    scores: [u32; TEAMS],
    /// The points each team scored from each outpost (its end statistic, [`capture_counts`]).
    outpost_points: [[u32; TEAMS]; 3],
    /// The team that owns every outpost and since when (server ms).
    dominating: Option<(u8, u64)>,
    multiplier: u32,
    next_score_ms: u64,
    next_capture_ms: u64,
    raids: [u64; TEAMS],
    winner: Option<u8>,
    version: u64,
    list_version: u64,
    both_teams_seen: bool,
    /// The fort barriers of the map ([`super::outpost_rush_doors`]).
    barriers: Vec<FortBarrier>,
    /// The Baroness of the island ([`super::outpost_rush_boss`]); `None` on the other maps.
    pub(super) baroness: Option<BaronessRun>,
    /// The event feed ([`super::outpost_rush_events`]).
    pub(super) feed: EventFeed,
    /// The outpost structures ([`super::outpost_rush_structures`]) and the server ms of their
    /// last tick.
    pub(super) structures: OutpostStructures,
    structures_ms: u64,
    /// The summons and the corrupted portal ([`super::outpost_rush_summons`]).
    pub(super) summons: SummonRun,
    /// The war board ([`board`]).
    board: Board,
    /// The last `outpostattacked` of each outpost ([`super::outpost_rush_events`]).
    attacks: AttackWatch,
    /// The capture point entities ([`capture_points`]).
    capture_points: CapturePoints,
    /// The storage sheds ([`storage_shed`]).
    sheds: StorageSheds,
}

impl OutpostRushMatch {
    fn next_version(&mut self) -> u64 {
        self.version += 1;
        self.version
    }

    fn of(&self, number: u64) -> Option<&MatchParticipant> {
        self.participants
            .iter()
            .find(|participant| participant.number == number)
    }

    /// The team raid id and the team place of player `number` ([`super::raids`]).
    pub(super) fn raid_seat_of(&self, number: u64) -> Option<(u64, u8)> {
        let participant = self.of(number)?;
        let raid = *self.raids.get(usize::from(participant.team))?;
        Some((raid, participant.place))
    }

    /// The team of player `number`.
    pub(super) fn team_of(&self, number: u64) -> Option<u8> {
        self.of(number).map(|participant| participant.team)
    }

    /// The team whose raid is `raid` (no team for raid 0).
    pub(super) fn team_of_raid(&self, raid: u64) -> Option<u8> {
        let team = self.raids.iter().position(|team_raid| raid != 0 && *team_raid == raid)?;
        u8::try_from(team).ok()
    }

    /// Whether the match runs (state 0).
    pub(super) const fn is_running(&self) -> bool {
        self.state == RUNNING
    }

    /// The score of team `team`.
    pub(super) fn score_of(&self, team: u8) -> u32 {
        self.scores[usize::from(team.min(1))]
    }

    fn present(&self, team: u8) -> bool {
        self.participants
            .iter()
            .any(|participant| participant.team == team)
    }

    /// The values: the outposts, and from the start the scores and the multiplier, and
    /// `WinningTeamIdx` once a team won.
    fn values(&self) -> Vec<Keyed> {
        let mut values: Vec<Keyed> = self
            .outposts
            .iter()
            .zip(self.points)
            .map(|(outpost, point)| (outpost.key, point.value()))
            .collect();
        values.extend(OUTPOST_STRUCTURE_STATUSES.into_iter().zip(self.structures.statuses()));
        if self.state != PREGAME {
            values.push((TEAM_SCORES, team_scores(self.scores)));
            values.push((DOMINATION_MULTIPLIER, u64::from(self.multiplier)));
            values.extend(self.baroness.iter().flat_map(BaronessRun::values));
        }
        if let Some(winner) = self.winner {
            values.push((WINNING_TEAM, u64::from(winner)));
        }
        if self.state == ENDED {
            values.extend(capture_counts(&self.outpost_points));
            values.extend(self.summons.end_values());
        }
        values
    }

    /// The timers of the current state.
    fn timers(&self) -> Vec<Keyed> {
        let own = match self.state {
            PREGAME => PREGAME_TIMER,
            RUNNING => TIME_LIMIT_TIMER,
            _ => CLEANUP_TIMER,
        };
        let mut timers = vec![(own, self.ends_ms), (GAME_MODE_TIMEOUT, self.timeout_ms)];
        if self.state == RUNNING {
            timers.extend(self.baroness.iter().flat_map(BaronessRun::timers));
            timers.extend(self.summons.timers());
        }
        timers
    }

    /// The change of the values and the timers at a new version.
    fn values_and_timers_change(&mut self) -> GameModeState {
        let version = self.next_version();
        GameModeState {
            values: Some(full(version, self.values())),
            timers: Some(full(version, self.timers())),
            events: self.feed.take(),
            ..GameModeState::default()
        }
    }

    /// The change of the values at a new version.
    fn values_change(&mut self) -> GameModeState {
        let version = self.next_version();
        GameModeState {
            values: Some(full(version, self.values())),
            events: self.feed.take(),
            ..GameModeState::default()
        }
    }

    /// The change to script state `state` whose timer ends at `ends_ms`.
    fn script_change(&mut self, state: u8, ends_ms: u64) -> GameModeState {
        self.state = state;
        self.ends_ms = ends_ms;
        let version = self.next_version();
        GameModeState {
            script_state: Some(state),
            values: Some(full(version, self.values())),
            timers: Some(full(version, self.timers())),
            events: self.feed.take(),
            ..GameModeState::default()
        }
    }

    /// The participant lists at a new version (no character ids).
    fn participant_lists(&mut self) -> GameModeState {
        let version = self.next_version();
        participant_lists(&self.participants, version, false)
    }

    /// The change of participant `index`'s status to `status`.
    fn status_change(&mut self, index: usize, status: u8) -> GameModeState {
        self.participants[index].status = status;
        let version = self.next_version();
        GameModeState {
            statuses: Some(ReplicatedList::Changes(vec![ListChange {
                index: u32::try_from(index).unwrap_or(u32::MAX),
                version,
                value: status,
            }])),
            ..GameModeState::default()
        }
    }

    /// Whether the Baroness froze team `team`.
    fn frozen(&self, team: u8) -> bool {
        self.baroness
            .as_ref()
            .is_some_and(|run| run.frozen_team() == Some(team.min(1)))
    }

    /// Team `team` scores `points` (none while the Baroness froze it); `true` when it reached
    /// the win.
    fn score(&mut self, team: u8, points: u32) -> bool {
        if self.frozen(team) {
            return false;
        }
        let score = &mut self.scores[usize::from(team.min(1))];
        *score = score.saturating_add(points);
        *score >= SCORE_TO_WIN
    }

    /// The team that owns every outpost.
    fn owner_of_all(&self) -> Option<u8> {
        let owner = self.points[0].owner;
        (owner != NO_TEAM && self.points.iter().all(|point| point.owner == owner)).then_some(owner)
    }

    /// The team with more points; `None` on a tie.
    fn leader(&self) -> Option<u8> {
        match self.scores[0].cmp(&self.scores[1]) {
            std::cmp::Ordering::Greater => Some(0),
            std::cmp::Ordering::Less => Some(1),
            std::cmp::Ordering::Equal => None,
        }
    }
}

impl SimWorld {
    /// Whether this instance sim's game mode is Outpost Rush.
    pub(super) fn runs_outpost_rush(&self) -> bool {
        self.instance_kind() == Some(InstanceKind::OutpostRush)
    }

    /// Player `entity` arrived with its seat `seat`: it joins the match (the first one opens it)
    /// with its participant state.
    pub(super) fn join_outpost_rush(&mut self, entity: Entity, seat: ArenaSeat) {
        let Some((number, character, eligible)) = self.world.get::<Player>(entity).map(|player| {
            (
                player.number,
                *player.character.as_bytes(),
                player.arena.queue_eligible().to_vec(),
            )
        }) else {
            return;
        };
        if self.arena_matches.outpost_rush.is_none() && !self.open_outpost_rush() {
            return;
        }
        let server_ms = self.server_ms;
        let phase = self.instance_run.as_ref().map_or(0, |run| run.phase());
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        if running.of(number).is_some() || running.state == ENDED {
            return;
        }
        running.participants.push(MatchParticipant {
            number,
            entity,
            character,
            team: seat.team,
            place: seat.place,
            status: PLAYING_STATUS,
        });
        running.both_teams_seen |= (0..TEAMS as u8).all(|team| running.present(team));
        let lists = running.participant_lists();
        running.list_version += 1;
        let list_version = running.list_version;
        let entry = MatchEntry {
            gde_ref: running.gde_ref,
            mode: running.crc,
            map: running.map.map_crc,
            phase,
            flags: MATCH_FLAGS,
            team: seat.team,
            list_version,
            eligible,
        };
        let mode_entity = running.entity;
        self.change_game_mode_state(mode_entity, lists);
        self.put_match_joined(entity, entry, server_ms);
        self.put_affiliations(entity, list_version, team_affiliations(seat.team));
        self.give_mode_event(entity, number, PRE_GAME_EVENT);
        let content = std::sync::Arc::clone(&self.content);
        let board = self.arena_matches.outpost_rush.as_mut().map(|running| {
            running.board.join(
                number,
                entity,
                character,
                seat.team,
                server_ms,
                &content.outpost_rush_board,
            )
        });
        self.send_outpost_rush_board(board.unwrap_or_default());
        tracing::info!(
            player = number,
            team = seat.team,
            "player joined the Outpost Rush match"
        );
    }

    /// Opens the match of this sim's Outpost Rush instance: its game mode entity in the
    /// pregame. `false` when this sim runs no Outpost Rush or its map has no outposts.
    fn open_outpost_rush(&mut self) -> bool {
        let content = std::sync::Arc::clone(&self.content);
        let Some(mode) = self
            .instance_run
            .as_ref()
            .and_then(|run| content.instances.by_id(run.game_mode()))
            .filter(|mode| mode.kind == InstanceKind::OutpostRush)
        else {
            return false;
        };
        let folder = self.settings.level.folder;
        let (Some(map), Some(outposts)) = (
            mode.map_of(folder).cloned(),
            OUTPOST_MAPS.iter().find(|map| map.folder == folder),
        ) else {
            tracing::warn!(level = folder, "Outpost Rush map without outposts");
            return false;
        };
        let server_ms = self.server_ms;
        let started = self.arena_matches.started;
        self.arena_matches.started += 1;
        let raid = RAID_ID_BASE + started * TEAMS as u64;
        let pregame_end = server_ms.saturating_add(PREGAME_MS);
        let mut running = OutpostRushMatch {
            entity: 0,
            gde_ref: [0; RAW_ID_LEN],
            crc: mode.crc,
            map,
            outposts: outposts.outposts,
            participants: Vec::new(),
            state: PREGAME,
            ends_ms: pregame_end,
            timeout_ms: server_ms.saturating_add(GAME_MODE_TIMEOUT_MS),
            points: [OutpostState::NEUTRAL; 3],
            scores: [0; TEAMS],
            outpost_points: [[0; TEAMS]; 3],
            dominating: None,
            multiplier: BASE_MULTIPLIER,
            next_score_ms: pregame_end.saturating_add(SCORE_TICK_MS),
            next_capture_ms: pregame_end.saturating_add(CAPTURE_TICK_MS),
            raids: [raid, raid + 1],
            winner: None,
            version: FIRST_LIST_VERSION,
            list_version: FIRST_LIST_VERSION,
            both_teams_seen: false,
            barriers: Vec::new(),
            baroness: None,
            feed: EventFeed::default(),
            structures: self.spawn_outpost_structures(folder, started),
            capture_points: self.spawn_capture_points(folder, started),
            sheds: StorageSheds::default(),
            structures_ms: server_ms,
            summons: SummonRun::of_map(folder),
            board: Board::new(&[0; RAW_ID_LEN]),
            attacks: AttackWatch::default(),
        };
        for outpost in 0..running.outposts.len() {
            let number = u32::try_from(outpost).unwrap_or(u32::MAX);
            running.feed.note(ARMORY_BUILT, server_ms, number);
        }
        let version = running.version;
        let state = GameModeState {
            script_state: Some(PREGAME),
            spawned: Some(full(version, Vec::new())),
            mode: Some(running.crc),
            map: Some(running.map.map_crc),
            participants: Some(ReplicatedList::Full(full(version, Vec::new()))),
            statuses: Some(ReplicatedList::Full(full(version, Vec::new()))),
            teams: Some(ReplicatedList::Full(full(version, Vec::new()))),
            characters: Some(ReplicatedList::Full(full(version, Vec::new()))),
            raids: Some(ReplicatedList::Full(full(version, running.raids.to_vec()))),
            values: Some(full(version, running.values())),
            timers: Some(full(version, running.timers())),
            events: running.feed.take(),
            no_affliction: true,
            linked_mode: Some(0),
            ..GameModeState::default()
        };
        let middle = middle_of(&running.map);
        let (entity, gde_ref) = self.place_game_mode_entity(MATCH_NAMESPACE, None, middle, state);
        running.entity = entity;
        running.barriers = self.spawn_fort_barriers(folder, started);
        running.baroness =
            BaronessRun::of_map(folder, self.spawn_baroness_barriers(folder, started));
        running.gde_ref = gde_ref;
        running.board = Board::new(&gde_ref);
        running.sheds = self.storage_sheds_of(&running.structures, &mode.items_removed_on_exit);
        tracing::info!(level = %running.map.folder, "Outpost Rush match opened");
        self.arena_matches.outpost_rush = Some(running);
        true
    }

    /// 6824: player `number` readied up (or not) in the pregame.
    pub(super) fn outpost_rush_ready(&mut self, number: u64, ready: bool) {
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        let Some(index) = running
            .participants
            .iter()
            .position(|participant| participant.number == number)
        else {
            return;
        };
        if running.state != PREGAME {
            return;
        }
        let status = running.participants[index].status;
        let status = if ready {
            status | READY_BIT
        } else {
            status & !READY_BIT
        };
        let change = running.status_change(index, status);
        let entity = running.entity;
        self.change_game_mode_state(entity, change);
    }

    /// Player `target` died to `attacker`: its status is dead, the board counts the death, the
    /// kill and the assists ([`board`]) and, in the running match, a participant of the other
    /// team scores the kill for its team.
    pub(super) fn outpost_rush_death(&mut self, target: u64, attacker: u64) {
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        let Some(index) = running
            .participants
            .iter()
            .position(|participant| participant.number == target)
        else {
            return;
        };
        let dead = running.participants[index].clone();
        let mut change = running.status_change(index, dead.status | DEAD_BIT);
        let killer = running
            .of(attacker)
            .map(|killer| killer.team)
            .filter(|team| *team != dead.team && running.state == RUNNING);
        let won = killer.map(|team| (team, running.score(team, KILL_POINTS)));
        if won.is_some() {
            change.values = running.values_change().values;
        }
        let entity = running.entity;
        self.change_game_mode_state(entity, change);
        self.board_player_death(target, attacker, killer);
        if let Some((team, true)) = won {
            self.end_outpost_rush(Some(team));
        }
    }

    /// Where dead player `entity` of the match respawns: its team's spot, with its status alive
    /// again. `None` when it plays no Outpost Rush match here.
    pub(super) fn outpost_rush_respawn(&mut self, entity: Entity) -> Option<Position> {
        if self.arena_matches.capture_the_flag.is_some() {
            return self.capture_the_flag_respawn(entity);
        }
        let running = self.arena_matches.outpost_rush.as_mut()?;
        let index = running
            .participants
            .iter()
            .position(|participant| participant.entity == entity)?;
        let participant = running.participants[index].clone();
        let spot = team_spot(&running.map, participant.team, participant.place)?;
        let change = running.status_change(index, participant.status & !DEAD_BIT);
        let mode_entity = running.entity;
        self.change_game_mode_state(mode_entity, change);
        Some(spot)
    }

    /// Whether players `first` and `second` are foes in the match: participants of two teams
    /// while it runs. `None` when this sim runs no Outpost Rush match.
    pub(super) fn outpost_rush_foes(&self, first: Entity, second: Entity) -> Option<bool> {
        if self.arena_matches.capture_the_flag.is_some() {
            return self.capture_the_flag_foes(first, second);
        }
        let running = self.arena_matches.outpost_rush.as_ref()?;
        let team = |entity: Entity| {
            running
                .participants
                .iter()
                .find(|participant| participant.entity == entity)
                .map(|participant| participant.team)
        };
        Some(
            running.state == RUNNING
                && team(first)
                    .zip(team(second))
                    .is_some_and(|(first, second)| first != second),
        )
    }

    /// Runs the match: the board's pushes ([`board`]), the pregame's end, the captures, the
    /// scores, the time limit, a team with no participant left, and the end of `Cleanup`.
    pub(super) fn tick_outpost_rush(&mut self) {
        let now_ms = self.server_ms;
        let Some(running) = self.arena_matches.outpost_rush.as_ref() else {
            return;
        };
        let participants: Vec<Entity> = running
            .participants
            .iter()
            .map(|participant| participant.entity)
            .collect();
        self.ask_company_looks(&participants);
        let Some(running) = self.arena_matches.outpost_rush.as_ref() else {
            return;
        };
        let (state, ends_ms) = (running.state, running.ends_ms);
        let absent = (0..TEAMS as u8).find(|team| !running.present(*team));
        if state != ENDED
            && let Some(team) = absent
            && (running.both_teams_seen || (state == PREGAME && now_ms >= ends_ms))
        {
            self.end_outpost_rush(Some(1 - team));
            return;
        }
        if state != ENDED {
            let content = std::sync::Arc::clone(&self.content);
            let pushes = self.arena_matches.outpost_rush.as_mut().map(|running| {
                running.board.flush(now_ms, &content.outpost_rush_board)
            });
            self.send_outpost_rush_board(pushes.unwrap_or_default());
        }
        match state {
            PREGAME if now_ms >= ends_ms => self.start_outpost_rush(),
            PREGAME => self.run_outpost_structures(now_ms),
            RUNNING => {
                self.run_outpost_rush(now_ms);
                self.run_outpost_structures(now_ms);
                let still_running = self
                    .arena_matches
                    .outpost_rush
                    .as_ref()
                    .is_some_and(|running| running.state == RUNNING);
                if still_running
                    && (self.tick_baroness(now_ms) | self.tick_outpost_rush_summons(now_ms))
                {
                    self.send_outpost_rush_values_and_timers();
                }
            }
            ENDED if now_ms >= ends_ms => self.cleanup_outpost_rush(),
            _ => {}
        }
    }

    /// `PregameTimer` ended: state 0 with `TimeLimit`, and the fort barriers take their teams'
    /// raids ([`super::outpost_rush_doors`]).
    fn start_outpost_rush(&mut self) {
        let server_ms = self.server_ms;
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        running.next_score_ms = server_ms.saturating_add(SCORE_TICK_MS);
        running.next_capture_ms = server_ms.saturating_add(CAPTURE_TICK_MS);
        if let Some(run) = running.baroness.as_mut() {
            run.start(server_ms);
        }
        running.summons.start(server_ms);
        let change = running.script_change(RUNNING, server_ms.saturating_add(TIME_LIMIT_MS));
        let entity = running.entity;
        let (barriers, raids) = (running.barriers.clone(), running.raids);
        self.change_game_mode_state(entity, change);
        self.open_fort_barriers(&barriers, &raids);
        tracing::info!("Outpost Rush match started");
    }

    /// The running match at server ms `now_ms`: the capture ticks and the score ticks that are
    /// due, then the time limit.
    fn run_outpost_rush(&mut self, now_ms: u64) {
        let Some(running) = self.arena_matches.outpost_rush.as_ref() else {
            return;
        };
        let outposts = running.outposts;
        let inside: Vec<(u64, u8, usize)> = running
            .participants
            .iter()
            .filter(|participant| participant.status & DEAD_BIT == 0)
            .filter_map(|participant| {
                let at = self.world.get::<Body>(participant.entity)?.motion.position();
                let index = board::outpost_index_at(&outposts, at)?;
                Some((participant.number, participant.team, index))
            })
            .collect();
        let protected = wards_standing(&running.structures);
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        let mut changed = false;
        let mut won = None;
        let mut score_ticks: u32 = 0;
        let ticked = now_ms >= running.next_capture_ms;
        if ticked {
            running.capture_points.note_presence(&inside);
        }
        let present = running.capture_points.present;
        while now_ms >= running.next_capture_ms {
            running.next_capture_ms = running.next_capture_ms.saturating_add(CAPTURE_TICK_MS);
            for (index, (point, present)) in running.points.iter_mut().zip(&present).enumerate() {
                let next = point.step(*present, protected[index]);
                changed |= next != *point;
                *point = next;
                let taken_from_owner =
                    next.owner != NO_TEAM && next.capturer != NO_TEAM && next.capturer != next.owner;
                if taken_from_owner && running.attacks.attacked(index, now_ms) {
                    running.feed.note(OUTPOST_ATTACKED, now_ms, u32::try_from(index).unwrap_or(u32::MAX));
                    changed = true;
                }
            }
        }
        let dominating = running.owner_of_all();
        if dominating != running.dominating.map(|(team, _)| team) {
            running.dominating = dominating.map(|team| (team, now_ms));
        }
        let multiplier = running.dominating.map_or(BASE_MULTIPLIER, |(_, since)| {
            domination_multiplier(now_ms.saturating_sub(since))
        });
        changed |= multiplier != running.multiplier;
        running.multiplier = multiplier;
        while now_ms >= running.next_score_ms && won.is_none() {
            running.next_score_ms = running.next_score_ms.saturating_add(SCORE_TICK_MS);
            score_ticks = score_ticks.saturating_add(1);
            for team in 0..TEAMS as u8 {
                let factor = if running.dominating.is_some_and(|(owner, _)| owner == team) {
                    running.multiplier
                } else {
                    BASE_MULTIPLIER
                };
                let per_outpost = OUTPOST_POINTS * factor;
                let mut points = 0;
                let unfrozen = !running.frozen(team);
                for (point, scored) in running.points.iter().zip(&mut running.outpost_points) {
                    if point.owner == team {
                        points += per_outpost;
                        if unfrozen {
                            let scored = &mut scored[usize::from(team)];
                            *scored = scored.saturating_add(per_outpost);
                        }
                    }
                }
                if points > 0 {
                    changed = true;
                    if running.score(team, points) {
                        won = Some(team);
                    }
                }
            }
        }
        let timed_out = now_ms >= running.ends_ms;
        let leader = running.leader();
        let change = changed.then(|| running.values_change());
        let entity = running.entity;
        if let Some(change) = change {
            self.change_game_mode_state(entity, change);
        }
        self.board_contests(score_ticks);
        self.show_capture_points(protected);
        if let Some(team) = won {
            self.end_outpost_rush(Some(team));
        } else if timed_out {
            self.end_outpost_rush(leader);
        }
    }

    /// The outpost structures at `now_ms` ([`super::outpost_rush_structures`]): each takes the
    /// raid of its outpost's owner, then the build accepts and the rising health run; the
    /// resources taken count on the board ([`board`]); a command post's new tier goes to the
    /// event feed ([`super::outpost_rush_events`]); a status change goes out with the values.
    fn run_outpost_structures(&mut self, now_ms: u64) {
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        let raids = running.points.map(|point| {
            running
                .raids
                .get(usize::from(point.owner))
                .copied()
                .unwrap_or_default()
        });
        let teams: Vec<(u64, u64)> = running
            .participants
            .iter()
            .filter_map(|participant| {
                let raid = running.raids.get(usize::from(participant.team))?;
                Some((participant.number, *raid))
            })
            .collect();
        let elapsed_ms = now_ms.saturating_sub(running.structures_ms);
        running.structures_ms = now_ms;
        let mut set = std::mem::take(&mut running.structures);
        let before = set.statuses();
        self.own_outpost_structures(&mut set, raids);
        let raid_of = |number: u64| {
            teams
                .iter()
                .find(|(player, _)| *player == number)
                .map(|(_, raid)| *raid)
        };
        let changed = self.tick_outpost_structures(&mut set, raid_of, now_ms, elapsed_ms);
        let deposits = std::mem::take(&mut set.deposits);
        let events = command_post_events(&before, &set.statuses());
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        running.structures = set;
        for (name, number) in events {
            running.feed.note(name, now_ms, number);
        }
        for (number, item, quantity) in deposits {
            self.outpost_rush_deposited(number, &item, quantity);
        }
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        if changed {
            let change = running.values_change();
            let entity = running.entity;
            self.change_game_mode_state(entity, change);
        }
    }

    /// The build or upgrade `request` (named `name`) of player `entity` (`number`) on a
    /// structure of the match ([`super::outpost_rush_structures`]), or its use of an armory
    /// shop ([`Self::armory_status`]); `false` when it is none.
    pub(super) fn outpost_rush_interaction(
        &mut self,
        entity: Entity,
        number: u64,
        request: nw_messages::InteractionRequest,
        name: &[u8],
    ) -> bool {
        if request.kind == OPEN_ARMORY_KIND {
            let Some(status) = self.armory_status(entity, &request.target) else {
                return false;
            };
            self.send_travel(entity, &[super::gather_answer(number, request, status, name)]);
            return true;
        }
        if !matches!(request.kind, APPLY_ALL_RESOURCES_KIND | UPGRADE_BUILDABLE_KIND) {
            return false;
        }
        let now_ms = self.server_ms;
        self.arena_matches
            .outpost_rush
            .as_mut()
            .is_some_and(|running| running.structures.ask(entity, number, request, name, now_ms))
    }

    /// The answer to player `entity`'s use of the armory `target` (kind 78 `OpenArmory`): its
    /// accept when the armory stands built for the player's team, else its refusal; `None` when
    /// `target` is no armory of the match.
    ///
    /// Observed (`OPR_TEST3`, the player's team owning outpost A): the request at 227.800 s is
    /// accepted (3743, status 1, no name) 39 ms later; the shop's purchase is the 3741 of the
    /// `ItemCurrencyConversionData` rows `OR_*` (`OR_FoodPvP` at 230.585 s, paid with
    /// `OutpostRushAzothEssence`: [`crate::shop`]), and the client's 2473 closes it (231.023 s)
    /// with no answer. Inferred: the other team's players are refused (no capture shows a use
    /// of an enemy armory).
    fn armory_status(&self, entity: Entity, target: &[u8; RAW_ID_LEN]) -> Option<u8> {
        let running = self.arena_matches.outpost_rush.as_ref()?;
        let owner = running.structures.built_raid(target, StructureRole::Armory)?;
        let raid = self.outpost_rush_raid_of(entity);
        Some(if raid == Some(owner) {
            USE_ACCEPTED
        } else {
            USE_REFUSED
        })
    }

    /// Player `shooter` hit for `amount` with a turret it uses ([`super::turrets`]): the
    /// shooter's `DamageWithSiegeWeapons` on the war board, in its row alone, as the hit's own
    /// row already counted it in `TotalDamageDealt` and the score ([`board::Board::note_row_alone`]).
    pub(super) fn outpost_rush_siege_hit(&mut self, shooter: u64, amount: u32) {
        let content = std::sync::Arc::clone(&self.content);
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        if running.state == ENDED {
            return;
        }
        running.board.note_row_alone(
            shooter,
            nw_content::warboard_stats::DAMAGE_WITH_SIEGE_WEAPONS,
            amount,
            &content.outpost_rush_board,
        );
    }

    /// The raid of the team of player `entity` in the match.
    pub(super) fn outpost_rush_raid_of(&self, entity: Entity) -> Option<u64> {
        let number = self.world.get::<Player>(entity)?.number;
        let running = self.arena_matches.outpost_rush.as_ref()?;
        running.raid_seat_of(number).map(|(raid, _)| raid)
    }

    /// The outpost structures an attack of player `entity` can meet while the match runs:
    /// slot key and place ([`super::outpost_rush_structures`]).
    pub(super) fn outpost_rush_structure_targets(&self, entity: Entity) -> Vec<(u64, Position)> {
        let Some(raid) = self.outpost_rush_raid_of(entity) else {
            return Vec::new();
        };
        self.arena_matches
            .outpost_rush
            .as_ref()
            .filter(|running| running.state == RUNNING)
            .map(|running| running.structures.targets_of(raid))
            .unwrap_or_default()
    }

    /// Whether slot key `number` is a structure of the match.
    pub(super) fn is_outpost_rush_structure(&self, number: u64) -> bool {
        self.arena_matches
            .outpost_rush
            .as_ref()
            .is_some_and(|running| running.structures.holds(number))
    }

    /// A hit of `amount` by player `entity` on the structure of slot key `target`: `None` when
    /// it meets no structure open to that player; the board counts its `DamageToOther`
    /// ([`board`]); a foe's hit on an owned outpost reports it attacked
    /// ([`super::outpost_rush_events`]); a ruin changes the outpost's status.
    pub(super) fn hit_outpost_rush_structure(
        &mut self,
        entity: Entity,
        target: u64,
        amount: u32,
    ) -> Option<bool> {
        let raid = self.outpost_rush_raid_of(entity)?;
        let running = self.arena_matches.outpost_rush.as_mut()?;
        if running.state != RUNNING {
            return None;
        }
        let mut set = std::mem::take(&mut running.structures);
        let outpost = set
            .structures
            .iter()
            .find(|structure| structure.number == target)
            .map(|structure| usize::from(structure.outpost));
        let ruined = self.hurt_outpost_structure(&mut set, raid, target, amount);
        let number = self.world.get::<Player>(entity).map(|player| player.number);
        if ruined.is_some()
            && let Some(number) = number
        {
            self.note_outpost_rush_stat(number, nw_content::warboard_stats::DAMAGE_TO_OTHER, amount);
        }
        let running = self.arena_matches.outpost_rush.as_mut()?;
        running.structures = set;
        let now_ms = self.server_ms;
        let attacker_team = number.and_then(|number| running.team_of(number));
        let attacked = ruined.is_some()
            && outpost.is_some_and(|outpost| {
                let owner = running.points.get(outpost).map_or(NO_TEAM, |point| point.owner);
                owner != NO_TEAM
                    && attacker_team.is_some_and(|team| team != owner)
                    && running.attacks.attacked(outpost, now_ms)
            });
        if let Some(outpost) = outpost.filter(|_| attacked) {
            running.feed.note(OUTPOST_ATTACKED, now_ms, u32::try_from(outpost).unwrap_or(u32::MAX));
        }
        if ruined == Some(true) || attacked {
            let change = running.values_change();
            let entity = running.entity;
            self.change_game_mode_state(entity, change);
        }
        ruined
    }

    /// Sends the match's values and timers.
    fn send_outpost_rush_values_and_timers(&mut self) {
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        let change = running.values_and_timers_change();
        let entity = running.entity;
        self.change_game_mode_state(entity, change);
    }

    /// Mob `mob` died by a hit of player `killer`: the board counts the kill ([`board`]) and
    /// the Baroness's death freezes a team ([`super::outpost_rush_boss`]).
    pub(super) fn outpost_rush_mob_killed(&mut self, mob: u64, killer: u64) {
        self.board_mob_death(mob, killer);
        if self.baroness_killed(mob, killer) {
            self.send_outpost_rush_values_and_timers();
        }
    }

    /// The match ended with `winner` (`None`: a tie at the time limit): the board's end push
    /// ([`board`]), then state 1 with
    /// `WinningTeamIdx` and `Cleanup`, each participant gets its end event ([`end_event`]) and
    /// the participants' temporary affiliations go.
    fn end_outpost_rush(&mut self, winner: Option<u8>) {
        let server_ms = self.server_ms;
        let content = std::sync::Arc::clone(&self.content);
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        let board = running.board.close(&content.outpost_rush_board);
        self.send_outpost_rush_board(board);
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        running.winner = winner;
        let change = running.script_change(ENDED, server_ms.saturating_add(CLEANUP_MS));
        let entity = running.entity;
        running.list_version += 1;
        let list_version = running.list_version;
        let players: Vec<(Entity, u64, u8, u32)> = running
            .participants
            .iter()
            .map(|participant| {
                let score = running.board.score_of(participant.number);
                (participant.entity, participant.number, participant.team, score)
            })
            .collect();
        self.change_game_mode_state(entity, change);
        for (player, number, team, score) in players {
            self.give_mode_event(player, number, end_event(team, winner, score));
            self.put_affiliations(player, list_version, Vec::new());
        }
        tracing::info!(?winner, "Outpost Rush match ended");
    }

    /// `Cleanup` ended: the players still in the match go back, and the instance closes.
    fn cleanup_outpost_rush(&mut self) {
        let players: Vec<Entity> = self
            .arena_matches
            .outpost_rush
            .iter()
            .flat_map(|running| {
                running
                    .participants
                    .iter()
                    .map(|participant| participant.entity)
            })
            .collect();
        for player in players {
            self.leave_outpost_rush(player);
        }
        if let Some(running) = self.arena_matches.outpost_rush.take() {
            self.stations.remove_camp(running.entity);
            self.remove_fort_barriers(&running.barriers);
            if let Some(run) = &running.baroness {
                self.remove_baroness(run);
            }
            self.remove_outpost_structures(&running.structures);
            self.remove_capture_points(&running.capture_points);
            self.remove_outpost_rush_summons(&running.summons);
        }
        if let Some(instance) = self.instance_run.as_ref().map(|run| run.id()) {
            self.close_instance(instance);
        }
    }

    /// Takes player `entity` out of the match's lists; a leave of a running match bars its
    /// queue. The participant and the list version, when it played.
    fn drop_outpost_rush_participant(&mut self, entity: Entity) -> Option<(MatchParticipant, u64)> {
        self.outpost_rush_shed_closed(entity, None);
        let now_ns = unix_ns(self.server_ms);
        let running = self.arena_matches.outpost_rush.as_mut()?;
        let index = running
            .participants
            .iter()
            .position(|participant| participant.entity == entity)?;
        let participant = running.participants.remove(index);
        running.board.leave(participant.number);
        let running_match = running.state != ENDED;
        let crc = running.crc;
        running.list_version += 1;
        let list_version = running.list_version;
        let lists = running.participant_lists();
        let mode_entity = running.entity;
        self.change_game_mode_state(mode_entity, lists);
        if running_match && let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player
                .arena
                .bar_queue(crc, now_ns.saturating_add(LEAVER_PENALTY_NS));
        }
        Some((participant, list_version))
    }

    /// Player `entity` leaves the match (1980 `Leave`, 3309, or the end of `Cleanup`): what a
    /// player who leaves gets, and it goes back to where it came from.
    pub(super) fn leave_outpost_rush(&mut self, entity: Entity) {
        if let Some((participant, list_version)) = self.drop_outpost_rush_participant(entity) {
            self.send_match_left(entity, &participant, list_version);
            self.stand_arena_player(entity);
        }
        self.exit_instance(entity);
    }

    /// A player of the match leaves the sim (a logout or a lost connection).
    pub(super) fn outpost_rush_left_sim(&mut self, entity: Entity) {
        self.drop_outpost_rush_participant(entity);
    }
}

#[cfg(test)]
pub(super) mod tests {
    use nw_bus::{InstanceOpened, InstanceStay, StoredPoint};
    use nw_content::instances::{
        EntryCounter, InstanceCatalog, InstanceMode, NO_INDEX, OUTPOST_RUSH_GAME_MODE, Teleport,
    };
    use nw_formats::objectstream::lowercase_crc32;
    use nw_messages::{LevelSpec, MemberState, ServerMessage};
    use uuid::Uuid;

    use super::*;
    use crate::world::Placement;
    use crate::world::outpost_rush_structures::tests as structure_tests;
    use crate::world::outpost_rush_structures::{ACCEPT_DELAY_MS, BUILD_SEND_MS, Phase, RUIN_BIT};
    use crate::world::tests::{NOW, admit_at, messages_to, slot};
    use crate::world::turrets::tests::{bag_count, give};


    /// Players of the test sim.
    const MANY_PLAYERS: usize = 10;
    /// The two players, one per team.
    pub(in crate::world) const FIRST: u64 = 41;
    pub(in crate::world) const SECOND: u64 = 42;
    /// The test instance and its phase.
    const INSTANCE: u128 = 8;
    const PHASE: u64 = 0x486f_26c0_6aa9_8117;
    /// The server time of the opening.
    pub(in crate::world) const START_MS: u64 = 1_000_000;

    #[test]
    fn the_keys_are_the_crcs_of_their_names() {
        for (key, name) in [
            (TIME_LIMIT_TIMER, "TimeLimit"),
            (TEAM_SCORES, "TeamScores"),
            (OUTPOST_A_STATUS, "CapturePointA_Status"),
            (OUTPOST_B_STATUS, "CapturePointB_Status"),
            (OUTPOST_C_STATUS, "CapturePointC_Status"),
        ] {
            assert_eq!(key, lowercase_crc32(name), "{name}");
        }
        assert_eq!(lowercase_crc32(OUTPOST_RUSH_GAME_MODE), 0x91b9_9a18);
        for (key, name) in OUTPOST_CAPTURE_COUNTS
            .into_iter()
            .chain(OUTPOST_STRUCTURE_STATUSES)
            .zip([
                "OutpostACaptureCounts",
                "OutpostBCaptureCounts",
                "OutpostCCaptureCounts",
                "OutpostStructureStatusesA",
                "OutpostStructureStatusesB",
                "OutpostStructureStatusesC",
            ])
        {
            assert_eq!(key, lowercase_crc32(name), "{name}");
        }
    }

    #[test]
    fn the_values_pack_as_retail() {
        // `opr_que_and_lost`: 683.712 `0000ff`, 733.667 `6200ff`, 734.177 `00ff00`.
        let capturing = OutpostState {
            owner: NO_TEAM,
            capturer: 0,
            progress: 0x62 * UNITS_PER_PERCENT,
        };
        assert_eq!(OutpostState::NEUTRAL.value(), 0xffff);
        assert_eq!(capturing.value(), 0x62_00ff);
        let owned = OutpostState {
            owner: 0,
            ..OutpostState::NEUTRAL
        };
        assert_eq!(owned.value(), 0xff00);
        // 2105.783: team 1 at 1,000 and team 0 at 0x217.
        assert_eq!(team_scores([0x217, 1_000]), 0x03e8_0217);
    }

    #[test]
    fn the_end_statistics_pack_as_retail() {
        // `OPR_TEST3` 1851.453 s: A 118/407, B 324/167, C 407/73 (team 1 / team 0).
        let points = [[407, 118], [167, 324], [73, 407]];
        assert_eq!(
            capture_counts(&points),
            [
                (OUTPOST_CAPTURE_COUNTS[0], 7_733_655),
                (OUTPOST_CAPTURE_COUNTS[1], 21_233_831),
                (OUTPOST_CAPTURE_COUNTS[2], 26_673_225),
            ]
        );
    }

    /// The capture ticks a lead of `lead` players needs from 0 to the end.
    pub(in crate::world) fn capture_ticks(lead: usize) -> u64 {
        u64::from(CAPTURE_DONE.div_ceil(rise_of(lead)))
    }

    #[test]
    fn a_capture_takes_sixty_seconds_less_ten_for_each_extra_player_down_to_ten() {
        assert_eq!(capture_ticks(1), 61, "60 s: 1,666 units a second, steps 2, 2, 1");
        assert_eq!(capture_ticks(2), 50);
        assert_eq!(capture_ticks(3), 40);
        assert_eq!(capture_ticks(6), 10, "m_captureTimeMinimum");
        assert_eq!(capture_ticks(9), 10, "m_captureMaxPlayerDelta counts five extra players");
        let shown: Vec<u8> = (1..=5)
            .scan(OutpostState::NEUTRAL, |point, _| {
                *point = point.step([1, 0], false);
                Some(point.shown_progress())
            })
            .collect();
        assert_eq!(shown, [1, 3, 4, 6, 8], "retail steps of 1 and 2");
    }

    #[test]
    fn a_capture_rises_toward_the_bigger_team_and_ends_at_one_hundred() {
        let mut point = OutpostState::NEUTRAL;
        point = point.step([1, 0], false);
        assert_eq!((point.capturer, point.progress), (0, rise_of(1)));
        assert_eq!(point.step([1, 1], false), point, "an equal count holds it");
        assert_eq!(
            point.step([0, 0], false).progress,
            rise_of(1) - EMPTY_FALL,
            "an empty outpost falls back 1 a second"
        );
        let pushed = point.step([0, 2], false);
        assert_eq!(
            (pushed.capturer, pushed.progress),
            (1, 0),
            "the other team pushes it back"
        );
        for _ in 0..capture_ticks(2) {
            point = point.step([3, 1], false);
        }
        assert_eq!(
            point,
            OutpostState {
                owner: 0,
                ..OutpostState::NEUTRAL
            }
        );
        assert_eq!(point.step([2, 0], false), point, "the owner holds its outpost");
        let taken = (0..capture_ticks(1)).fold(point, |point, _| point.step([0, 1], false));
        assert_eq!(
            taken.owner, 1,
            "an owned outpost goes straight to the other team"
        );
    }

    #[test]
    fn a_standing_ward_names_the_capturer_and_holds_its_progress() {
        let owned = OutpostState {
            owner: 0,
            ..OutpostState::NEUTRAL
        };
        let held = (0..capture_ticks(1)).fold(owned, |point, _| point.step([0, 1], true));
        assert_eq!(
            held,
            OutpostState {
                capturer: 1,
                ..owned
            },
            "retail: capturer team 1 at progress 0 (opr_match_4 731.330 to 742.798 s)"
        );
        assert_eq!(held.step([0, 1], false).progress, rise_of(1), "the ruined ward lets it rise");
    }

    #[test]
    fn the_multiplier_rises_after_a_minute_then_every_thirty_seconds() {
        assert_eq!(domination_multiplier(59_999), 1);
        assert_eq!(domination_multiplier(60_000), 2);
        assert_eq!(domination_multiplier(89_999), 2);
        assert_eq!(domination_multiplier(90_000), 3);
        assert_eq!(
            domination_multiplier(180_000),
            6,
            "OPR-coral-divide2 430.568 s"
        );
    }

    /// The retail `OutpostRush` mode on `NW_OPR_004_Trench` (made from its rows).
    pub(in crate::world) fn outpost_rush() -> InstanceMode {
        let map = InstanceMap {
            folder: "nw_opr_004_trench".to_owned(),
            map_name: "NW_OPR_004_Trench".to_owned(),
            coatlicue_path: "coatlicue/NW_OPR_004_Trench".to_owned(),
            frame: [832.0, 768.0, 512.0, 512.0],
            teleports: vec![
                Teleport {
                    position: [931.2498, 983.3751, 49.75],
                    yaw_deg: 270.0,
                },
                Teleport {
                    position: [1170.75, 987.75, 49.3],
                    yaw_deg: 90.0,
                },
            ],
            formation: Vec::new(),
            map_crc: lowercase_crc32("OutpostRush_004_Trench"),
        };
        InstanceMode {
            id: OUTPOST_RUSH_GAME_MODE.to_owned(),
            crc: lowercase_crc32(OUTPOST_RUSH_GAME_MODE),
            kind: InstanceKind::OutpostRush,
            map: map.clone(),
            capacity: 40,
            min_group: 0,
            required_level: Some(60),
            remote_entry: false,
            respawns: false,
            mutable: false,
            timeout_seconds: None,
            victory_delay_seconds: None,
            items_removed_on_exit: vec!["OutpostRushOnly".to_owned()],
            required_item_classes: Vec::new(),
            entry_achievement: None,
            loot_tags: vec!["OutpostRush".to_owned()],
            container_level: None,
            counter: EntryCounter::Uncounted,
            game_mode_index: NO_INDEX,
            generated: false,
            rotation: vec![map],
            rotation_span_seconds: Some(3600),
        }
    }

    /// Outpost `index` of the match of `world` is team `team`'s, as a finished capture leaves it.
    pub(in crate::world) fn own_outpost(world: &mut SimWorld, index: usize, team: u8) {
        if let Some(running) = world.arena_matches.outpost_rush.as_mut() {
            running.points[index] = OutpostState {
                owner: team,
                ..OutpostState::NEUTRAL
            };
        }
    }

    /// The Outpost Rush instance sim with the match open and two players, one per team.
    fn rush_sim() -> SimWorld {
        rush_sim_on(outpost_rush())
    }

    /// The retail `OutpostRush` mode on the island (made from its rows): the open world's level
    /// in the frame of its `WorldBounds`.
    pub(in crate::world) fn island_rush() -> InstanceMode {
        let map = InstanceMap {
            folder: ISLAND_FOLDER.to_owned(),
            map_name: "NewWorld_VitaeEterna".to_owned(),
            coatlicue_path: "coatlicue/NewWorld_VitaeEterna".to_owned(),
            frame: [937.0, 10562.0, 704.0, 1056.0],
            teleports: vec![
                Teleport {
                    position: [1241.0, 10746.0, 96.0],
                    yaw_deg: 0.0,
                },
                Teleport {
                    position: [1309.0, 11279.0, 66.0],
                    yaw_deg: 180.0,
                },
            ],
            formation: Vec::new(),
            map_crc: lowercase_crc32(OUTPOST_RUSH_GAME_MODE),
        };
        InstanceMode {
            map: map.clone(),
            rotation: vec![map],
            ..outpost_rush()
        }
    }

    /// The Outpost Rush instance sim of `mode` with the match open and two players, one per
    /// team.
    pub(in crate::world) fn rush_sim_on(mode: InstanceMode) -> SimWorld {
        let table = structure_tests::table_on(&mode.map.folder);
        rush_sim_with(mode, table)
    }

    /// The match of [`rush_sim_on`] with the outpost structures of `table`; its players arrive
    /// with the gear of a kit, and the kits hold the match's wood and ore.
    pub(in crate::world) fn rush_sim_with(
        mode: InstanceMode,
        table: nw_content::outpost_structures::OutpostStructures,
    ) -> SimWorld {
        let mut settings = crate::world::tests::settings(MANY_PLAYERS, MANY_PLAYERS as u32);
        settings.instance_role = true;
        settings.level = LevelSpec::instance(
            &mode.map.folder,
            &mode.map.map_name,
            &mode.map.coatlicue_path,
            mode.map.frame,
        );
        let mut world = SimWorld::new(
            settings,
            crate::login::WorldIds::random(0).expect("ids"),
            std::sync::Arc::new(nw_sim_core::test_support::NoGround),
            slot("inst-1"),
            crate::combat::tests::test_combat(),
            std::sync::Arc::new(crate::player_content::PlayerContent {
                instances: InstanceCatalog::build([mode.clone()]),
                mode_rewards: nw_content::game_mode_rewards::ModeRewards::build([(
                    PRE_GAME_EVENT.to_owned(),
                    nw_content::game_mode_rewards::ModeReward::default(),
                )]),
                outpost_structures: table,
                outpost_blueprints: structure_tests::gate_blueprints(),
                structure_costs: structure_tests::gate_costs(),
                kits: crate::world::gear::tests::kits_with_resources(&[
                    (structure_tests::WOOD, structure_tests::WOOD_CLASS),
                    (structure_tests::ORE, structure_tests::ORE_CLASS),
                ]),
                ..crate::player_content::PlayerContent::default()
            }),
        );
        let admits: Vec<_> = [(FIRST, 0), (SECOND, 1)]
            .into_iter()
            .map(|(number, team)| {
                let mut admit = admit_at(number, 940.0, false);
                admit.backstory = Some(crate::world::gear::tests::BACKSTORY.to_owned());
                admit.progression.arena.stay = Some(InstanceStay {
                    instance: Uuid::from_u128(INSTANCE),
                    game_mode: mode.id.clone(),
                    phase: PHASE,
                    return_level: "newworld_vitaeeterna".to_owned(),
                    return_position: StoredPoint::default(),
                    activated_ns: 1,
                    mutation_difficulty: None,
                    seat: Some(ArenaSeat { team, place: 0 }),
                });
                admit
            })
            .collect();
        let members = admits.iter().map(|admit| admit.character).collect();
        assert_eq!(
            world.open_instance(
                Uuid::from_u128(INSTANCE),
                &mode.id,
                PHASE,
                members,
                None,
                NOW
            ),
            InstanceOpened::Opened
        );
        for admit in &admits {
            world.admit(admit, NOW, START_MS).expect("admit");
        }
        world
    }

    /// A table of one storage shed (container 4, interact 5) at outpost A and one standing
    /// tier 1 Protection Ward at outpost C of the map in `folder`.
    pub(in crate::world) fn shed_and_ward_table(folder: &str) -> nw_content::outpost_structures::OutpostStructures {
        use nw_messages::{BundleMember, ContainerState, InteractState};
        let encoded = |member: BundleMember| hex::encode(member.encode().expect("encode"));
        let container = encoded(BundleMember {
            component: SHED_CONTAINER_COMPONENT,
            state: MemberState::Container(ContainerState {
                bonus_max_encumbrance: Some(0),
                was_emptied: Some(false),
                ..ContainerState::default()
            }),
        });
        let interact = encoded(BundleMember {
            component: SHED_CONTAINER_COMPONENT + 1,
            state: MemberState::Interact(InteractState {
                enabled: None,
                has_interactors: Some(0),
            }),
        });
        let shed = hex::encode(storage_shed::SHED_ASSET);
        let text = format!(
            r#"{{"kinds": [
            {{"asset": "{shed}", "slice": "slices/outpostrush/structures/or_storage.dynamicslice",
             "blueprint": null, "tier": 0, "foundation": null, "built":
             {{"card_component": 0, "place_component": 1, "ownership_component": 2,
               "members": ["{container}", "{interact}"]}}}},
            {{"asset": "{WARD_ASSET}", "slice": "slices/outpostrush/structures/or_protection_ward.dynamicslice",
             "blueprint": null, "tier": 1, "foundation": null, "built":
             {{"card_component": 0, "place_component": 1, "ownership_component": 2, "members": []}}}}],
            "maps": [{{"folder": "{folder}", "spawners": [
              {{"name": "StorageShedA", "outpost": 0, "role": "storage_shed", "number": 1,
                "asset": "{shed}", "place": [-60.0, 40.0, 107.125], "yaw": 0.0}},
              {{"name": "ProtectionWardC", "outpost": 2, "role": "protection_ward", "number": 1,
                "asset": "{WARD_ASSET}", "place": [62.0, 44.0, 107.125], "yaw": 0.0}}]}}]}}"#
        );
        nw_content::outpost_structures::OutpostStructures::parse(&text).expect("table")
    }

    /// The container component of the test shed.
    pub(in crate::world) const SHED_CONTAINER_COMPONENT: u64 = 4;
    /// The slice asset of the test ward (Observed: `or_protection_ward`, `opr_match_4`).
    const WARD_ASSET: &str = "646ec6cf1b715b1492f95bf1cb130647";

    pub(in crate::world) fn running(world: &SimWorld) -> &OutpostRushMatch {
        world
            .arena_matches
            .outpost_rush
            .as_ref()
            .expect("the match")
    }

    /// The held game mode state of the test match.
    pub(in crate::world) fn held_state(world: &SimWorld) -> GameModeState {
        world
            .stations
            .members(running(world).entity)
            .and_then(|members| {
                members.iter().find_map(|member| match &member.state {
                    MemberState::GameMode(state) => Some((**state).clone()),
                    _ => None,
                })
            })
            .expect("the game mode state")
    }

    #[test]
    fn the_first_state_holds_the_armory_of_each_outpost() {
        use super::super::outpost_rush_events::ARMORY_BUILT;
        use nw_messages::game_mode_state::GameModeEvent;
        let world = rush_sim();
        let events = held_state(&world).events;
        // The server ms of the opening: the end of the pregame less its length.
        let opened_ms = running(&world).ends_ms - PREGAME_MS;
        let armories: Vec<(u8, GameModeEvent)> = (0..3)
            .map(|outpost| {
                (
                    outpost,
                    GameModeEvent {
                        name: ARMORY_BUILT,
                        at_ms: opened_ms,
                        number: u32::from(outpost),
                    },
                )
            })
            .collect();
        assert_eq!(events, armories);
    }

    /// Retail: each player gets `OutpostRush_PreGameGrant` when it joins the match, once.
    #[test]
    fn a_joining_player_gets_the_pre_game_grant_once() {
        use nw_messages::ReplicatedList;
        let mut world = rush_sim();
        for number in [FIRST, SECOND] {
            let entity = world.player_by_number(number).expect("player");
            let seat = ArenaSeat { team: 0, place: 0 };
            world.join_outpost_rush(entity, seat);
            let mut log = world
                .world
                .get::<Player>(entity)
                .expect("player")
                .game_events
                .clone();
            let member = log.take_member(1).expect("the grant's entry");
            let MemberState::GameEvents(state) = member.state else {
                panic!("a game event member");
            };
            let Some(ReplicatedList::Full(list)) = state.game_events else {
                panic!("the first entry goes in full");
            };
            let events: Vec<u32> = list.elements.iter().map(|entry| entry.event).collect();
            assert_eq!(
                events,
                [lowercase_crc32(PRE_GAME_EVENT)],
                "one grant, a second join gives none"
            );
        }
    }

    pub(in crate::world) fn tick_at(world: &mut SimWorld, server_ms: u64) {
        tick_sent(world, server_ms);
    }

    /// [`tick_at`] with what the tick sends: the messages sent while it runs go out with it.
    pub(in crate::world) fn tick_sent(
        world: &mut SimWorld,
        server_ms: u64,
    ) -> Vec<crate::world::Outgoing> {
        world.tick(NOW, server_ms).expect("tick").messages
    }

    /// Puts player `number` at outpost `index` of the Trench map.
    pub(in crate::world) fn stand_in_outpost(world: &mut SimWorld, number: u64, index: usize) {
        let [x, y, z] = OUTPOST_MAPS[0].outposts[index].position;
        let entity = world.player_by_number(number).expect("player");
        world.place_player(entity, Position { x, y, z }, None, Placement::Drop, NOW);
    }

    #[test]
    fn the_island_match_has_its_outposts_its_barriers_and_team_affiliations() {
        let mut world = rush_sim_on(island_rush());
        let match_state = running(&world);
        assert_eq!(match_state.outposts, OUTPOST_MAPS[2].outposts);
        assert_eq!(
            match_state.map.map_crc, 0x91b9_9a18,
            "the retail island map id"
        );
        assert_eq!(match_state.barriers.len(), 6, "three fort barriers a team");
        let entity = world.player_by_number(SECOND).expect("player");
        let player = world.world.get::<Player>(entity).expect("player");
        assert!(
            player.extra_members.iter().any(|member| matches!(
                &member.state,
                MemberState::TemporaryAffiliation(state)
                    if matches!(&state.affiliations, Some(ReplicatedList::Full(list))
                        if list.elements == team_affiliations(1))
            )),
            "own team 1, the other team 0"
        );
        let start = START_MS + PREGAME_MS;
        tick_at(&mut world, start);
        let timers = running(&world).timers();
        assert!(timers.contains(&(
            super::super::outpost_rush_boss::BOSS_SPAWN_TIMER,
            start + 600_000
        )));
        world.end_outpost_rush(Some(0));
        let player = world.world.get::<Player>(entity).expect("player");
        assert!(
            player.extra_members.iter().any(|member| matches!(
                &member.state,
                MemberState::TemporaryAffiliation(state)
                    if matches!(&state.affiliations, Some(ReplicatedList::Full(list))
                        if list.elements.is_empty())
            )),
            "emptied at the end"
        );
    }

    #[test]
    fn the_winners_get_the_tier_of_their_board_score_and_the_others_the_loss_event() {
        // The retail wins: `_T1` at 570, 894, 1,856 and 1,981, `_T4` at 6,441.
        for score in [570, 894, 1_856, 1_981] {
            assert_eq!(end_event(0, Some(0), score), "OutpostRush_T1", "{score}");
        }
        assert_eq!(end_event(1, Some(1), 6_441), "OutpostRush_T4");
        assert_eq!(end_event(0, Some(0), 2_000), "OutpostRush_T2");
        assert_eq!(end_event(0, Some(0), 5_999), "OutpostRush_T3");
        assert_eq!(end_event(0, Some(0), u32::MAX), "OutpostRush_T4", "no tier above T4");
        assert_eq!(end_event(1, Some(0), 9_000), LOSS_EVENT, "a loss stays T1");
        assert_eq!(end_event(0, None, 9_000), LOSS_EVENT, "a tie");
    }

    #[test]
    fn an_island_instance_owns_the_spawns_of_its_frame_only() {
        let world = rush_sim_on(island_rush());
        assert_eq!(world.instance_frame(), Some(island_rush().map.frame));
        let trench = rush_sim_on(outpost_rush());
        assert_eq!(trench.instance_frame(), None, "a level of its own");
    }

    #[test]
    fn a_held_outpost_scores_until_the_owner_wins() {
        let mut world = rush_sim();
        let first = world.player_by_number(FIRST).expect("player");
        let second = world.player_by_number(SECOND).expect("player");
        let match_state = running(&world);
        assert_eq!(
            (match_state.state, match_state.participants.len()),
            (PREGAME, 2)
        );
        assert_eq!(
            match_state.values()[match_state.outposts.len()..],
            [
                (OUTPOST_STRUCTURE_STATUSES[0], ARMORY_TIER_ONE),
                (OUTPOST_STRUCTURE_STATUSES[1], 0),
                (OUTPOST_STRUCTURE_STATUSES[2], 0),
            ],
            "the outposts and their structure statuses (the test armory A) only in the pregame"
        );
        assert_eq!(
            world.outpost_rush_foes(first, second),
            Some(false),
            "no fight in the pregame"
        );
        world.outpost_rush_ready(FIRST, true);
        assert_eq!(
            running(&world).participants[0].status,
            PLAYING_STATUS | READY_BIT
        );
        let start_ms = START_MS + PREGAME_MS;
        tick_at(&mut world, start_ms);
        assert_eq!(running(&world).state, RUNNING);
        assert_eq!(world.outpost_rush_foes(first, second), Some(true));
        stand_in_outpost(&mut world, FIRST, 0);
        let captured_ms = start_ms + capture_ticks(1) * CAPTURE_TICK_MS;
        tick_at(&mut world, captured_ms);
        assert_eq!(running(&world).points[0].owner, 0, "team 0 owns outpost A");
        let score = running(&world).scores[0];
        assert!(score > 0, "a held outpost scores each tick");
        world.outpost_rush_death(SECOND, FIRST);
        let match_state = running(&world);
        assert_eq!(match_state.scores[0], score + KILL_POINTS, "a kill scores");
        assert_eq!(match_state.participants[1].status & DEAD_BIT, DEAD_BIT);
        let spot = world.outpost_rush_respawn(second).expect("a respawn spot");
        assert_eq!((spot.x, spot.y), (1170.75, 987.75), "team 1's spot");
        assert_eq!(running(&world).participants[1].status & DEAD_BIT, 0);
        let ticks_to_win = u64::from(SCORE_TO_WIN);
        tick_at(&mut world, captured_ms + ticks_to_win * SCORE_TICK_MS);
        let match_state = running(&world);
        assert_eq!((match_state.state, match_state.winner), (ENDED, Some(0)));
        assert!(match_state.values().contains(&(WINNING_TEAM, 0)));
        let from_outpost_a = match_state.scores[0] - KILL_POINTS;
        let capture_counts: Vec<Keyed> = match_state
            .values()
            .into_iter()
            .filter(|(key, _)| OUTPOST_CAPTURE_COUNTS.contains(key))
            .collect();
        assert_eq!(
            capture_counts,
            [
                (OUTPOST_CAPTURE_COUNTS[0], u64::from(from_outpost_a)),
                (OUTPOST_CAPTURE_COUNTS[1], 0),
                (OUTPOST_CAPTURE_COUNTS[2], 0),
            ],
            "the end statistics: team 0 scored all but its kill from outpost A"
        );
        tick_at(
            &mut world,
            captured_ms + ticks_to_win * SCORE_TICK_MS + CLEANUP_MS,
        );
        assert!(
            world.arena_matches.outpost_rush.is_none(),
            "the players go back"
        );
    }

    /// The status of outpost A whose armory stands at tier 1 (Observed in every first state).
    const ARMORY_TIER_ONE: u64 = 1;
    /// The status bits of the gate 1 (slot 6, three bits a slot).
    const GATE_ONE_SHIFT: u32 = 18;

    fn build_request(target: [u8; 16]) -> nw_messages::InteractionRequest {
        nw_messages::InteractionRequest {
            kind: APPLY_ALL_RESOURCES_KIND,
            target,
            ..crate::world::turrets::tests::use_request(target)
        }
    }

    fn status_of_a(world: &SimWorld) -> u64 {
        running(world)
            .values()
            .into_iter()
            .find(|(key, _)| *key == OUTPOST_STRUCTURE_STATUSES[0])
            .map_or(0, |(_, value)| value)
    }

    #[test]
    fn the_owner_team_builds_a_gate_with_its_wood_and_the_foes_ruin_it() {
        let mut world = rush_sim();
        let start_ms = START_MS + PREGAME_MS;
        tick_at(&mut world, start_ms);
        stand_in_outpost(&mut world, FIRST, 0);
        let captured_ms = start_ms + capture_ticks(1) * CAPTURE_TICK_MS;
        tick_at(&mut world, captured_ms);
        let first = world.player_by_number(FIRST).expect("player");
        let second = world.player_by_number(SECOND).expect("player");
        let (gate, number) = running(&world)
            .structures
            .structures
            .iter()
            .find(|structure| structure.phase == Phase::Foundation)
            .map(|structure| (structure.gde_ref, structure.number))
            .expect("the gate foundation");
        assert_eq!(status_of_a(&world), ARMORY_TIER_ONE, "no gate yet");
        give(&mut world, first, structure_tests::WOOD, structure_tests::GATE_WOOD);
        world.take_messages();
        assert!(world.outpost_rush_interaction(first, FIRST, build_request(gate), &[]));
        assert!(world.outpost_rush_interaction(second, SECOND, build_request(gate), &[]));
        let outgoing = tick_sent(&mut world, captured_ms + ACCEPT_DELAY_MS);
        let answered = |number: u64, status: u8| {
            messages_to(&outgoing, number).iter().any(|message| {
                matches!(message, ServerMessage::InteractionValidated(answer) if answer.status == status)
            })
        };
        assert!(answered(FIRST, USE_ACCEPTED), "the owner team builds");
        assert!(answered(SECOND, USE_REFUSED), "the other team does not");
        assert_eq!(bag_count(&world, first, structure_tests::WOOD), 0, "the whole cost");
        let build_ms = (structure_tests::GATE_BUILD_S * 1_000.0) as u64;
        tick_at(&mut world, captured_ms + ACCEPT_DELAY_MS + build_ms + BUILD_SEND_MS);
        assert_eq!(
            status_of_a(&world),
            ARMORY_TIER_ONE | 1 << GATE_ONE_SHIFT,
            "the gate stands at tier 1"
        );
        let whole = structure_tests::GATE_HEALTH as u32;
        assert_eq!(world.hit_outpost_rush_structure(first, number, whole), None, "own team");
        assert_eq!(world.hit_outpost_rush_structure(second, number, whole), Some(true));
        assert_eq!(
            status_of_a(&world),
            ARMORY_TIER_ONE | (1 | RUIN_BIT) << GATE_ONE_SHIFT,
            "a ruin keeps its tier (Observed gate 1 -> 5)"
        );
        assert_eq!(world.hit_outpost_rush_structure(second, number, whole), None, "a ruin");
    }


    #[test]
    fn an_upgrade_replaces_the_built_gate_with_the_next_tier_built_from_nothing() {
        let mut world = rush_sim();
        let start_ms = START_MS + PREGAME_MS;
        tick_at(&mut world, start_ms);
        stand_in_outpost(&mut world, FIRST, 0);
        let mut now_ms = start_ms + capture_ticks(1) * CAPTURE_TICK_MS;
        tick_at(&mut world, now_ms);
        let first = world.player_by_number(FIRST).expect("player");
        let gate = |world: &SimWorld, phase: Phase| {
            running(world)
                .structures
                .structures
                .iter()
                .find(|structure| structure.phase == phase && structure.max_health > 0.0)
                .map(|structure| structure.gde_ref)
        };
        let t1 = gate(&world, Phase::Foundation).expect("the t1 foundation");
        give(&mut world, first, structure_tests::WOOD, structure_tests::GATE_WOOD);
        assert!(world.outpost_rush_interaction(first, FIRST, build_request(t1), &[]));
        let t1_build_ms = (structure_tests::GATE_BUILD_S * 1_000.0) as u64;
        now_ms += ACCEPT_DELAY_MS;
        tick_at(&mut world, now_ms);
        now_ms += t1_build_ms + BUILD_SEND_MS;
        tick_at(&mut world, now_ms);
        assert_eq!(gate(&world, Phase::Built), Some(t1));
        let upgrade = nw_messages::InteractionRequest {
            kind: UPGRADE_BUILDABLE_KIND,
            ..build_request(t1)
        };
        assert!(world.outpost_rush_interaction(first, FIRST, upgrade, &[]));
        now_ms += ACCEPT_DELAY_MS;
        tick_at(&mut world, now_ms);
        assert_eq!(gate(&world, Phase::Replaced), Some(t1), "state 7 while the t2 builds");
        let t2 = gate(&world, Phase::Foundation).expect("the t2 foundation");
        assert_ne!(t2, t1, "a new entity");
        assert_eq!(
            status_of_a(&world),
            ARMORY_TIER_ONE | 1 << GATE_ONE_SHIFT,
            "the status keeps tier 1 while the t2 builds (Observed 1054.1 to 1060.1 s)"
        );
        give(&mut world, first, structure_tests::WOOD, structure_tests::GATE_WOOD);
        give(&mut world, first, structure_tests::ORE, structure_tests::GATE_T2_ORE);
        assert!(world.outpost_rush_interaction(first, FIRST, build_request(t2), &[]));
        now_ms += ACCEPT_DELAY_MS;
        tick_at(&mut world, now_ms);
        assert_eq!(bag_count(&world, first, structure_tests::ORE), 0);
        now_ms += (structure_tests::GATE_T2_BUILD_S * 1_000.0) as u64 + BUILD_SEND_MS;
        tick_at(&mut world, now_ms);
        assert_eq!(
            status_of_a(&world),
            ARMORY_TIER_ONE | 2 << GATE_ONE_SHIFT,
            "tier 2 when it is built"
        );
        assert_eq!(gate(&world, Phase::Replaced), None, "the old tier left");
        assert_eq!(gate(&world, Phase::Built), Some(t2));
    }

    #[test]
    fn a_leave_of_a_running_match_bars_the_queue_and_the_other_team_wins() {
        let mut world = rush_sim();
        tick_at(&mut world, START_MS + PREGAME_MS);
        let second = world.player_by_number(SECOND).expect("player");
        world.leave_outpost_rush(second);
        let barred = world
            .world
            .get::<Player>(second)
            .expect("player")
            .arena
            .queue_eligible()
            .to_vec();
        assert_eq!(
            barred.first().map(|bar| bar.0),
            Some(lowercase_crc32(OUTPOST_RUSH_GAME_MODE))
        );
        tick_at(&mut world, START_MS + PREGAME_MS + 1);
        let match_state = running(&world);
        assert_eq!((match_state.state, match_state.winner), (ENDED, Some(0)));
    }

    #[test]
    fn the_time_limit_gives_the_match_to_the_leader_and_a_tie_to_no_team() {
        let mut world = rush_sim();
        let start_ms = START_MS + PREGAME_MS;
        tick_at(&mut world, start_ms);
        tick_at(&mut world, start_ms + TIME_LIMIT_MS);
        let match_state = running(&world);
        assert_eq!((match_state.state, match_state.winner), (ENDED, None));
    }

    /// The raid of the ownership and the door body of the barrier of slot key `number`, as a new
    /// viewer gets it.
    fn barrier_view(world: &SimWorld, number: u64) -> Option<(u64, MemberState)> {
        let group = world.stations.group(
            number,
            crate::world::slots::Slot {
                interest: 9,
                new: true,
            },
        )?;
        let raid = group
            .members
            .iter()
            .find_map(|member| match &member.state {
                MemberState::Ownership(ownership) => ownership.raid_id,
                _ => None,
            })?;
        let door = group
            .members
            .iter()
            .find(|member| member.state.type_index() == nw_messages::DOOR_TYPE_INDEX)?
            .state
            .clone();
        Some((raid, door))
    }

    #[test]
    fn the_fort_barriers_stay_closed_take_their_team_raid_at_the_start_and_go_at_the_end() {
        let mut world = rush_sim();
        let barriers = running(&world).barriers.clone();
        assert_eq!(barriers.len(), 6, "three barriers per Trench fort");
        let closed = MemberState::Door(nw_messages::DoorComponentState::of(
            nw_messages::DoorState::Closed,
        ));
        for barrier in &barriers {
            assert_eq!(
                barrier_view(&world, barrier.number),
                Some((0, closed.clone())),
                "raid 0 in the pregame"
            );
        }
        let start_ms = START_MS + PREGAME_MS;
        tick_at(&mut world, start_ms);
        let raids = running(&world).raids;
        for barrier in &barriers {
            assert_eq!(
                barrier_view(&world, barrier.number),
                Some((raids[usize::from(barrier.team)], closed.clone())),
                "the team's raid once the match runs, the door still closed"
            );
        }
        world.end_outpost_rush(Some(0));
        tick_at(&mut world, start_ms + CLEANUP_MS);
        for barrier in &barriers {
            assert!(barrier_view(&world, barrier.number).is_none(), "gone");
        }
    }

    /// The test match with the board rules of [`board::tests::rules`].
    fn board_sim() -> SimWorld {
        let mut world = rush_sim();
        with_board_rules(&mut world);
        world
    }

    /// Gives the content of `world` the board's test rules.
    pub(in crate::world) fn with_board_rules(world: &mut SimWorld) {
        let mut content = (*world.content).clone();
        content.outpost_rush_board = board::tests::rules();
        world.content = std::sync::Arc::new(content);
    }

    /// The board value of stat `slot` of player `number` in the test match.
    pub(in crate::world) fn board_value(world: &SimWorld, number: u64, slot: u8) -> u32 {
        running(world).board.value(number, slot)
    }

    #[test]
    fn the_board_counts_the_hits_the_kills_the_deaths_and_the_contests_of_the_match() {
        use nw_content::warboard_stats::{
            CAPTURE_POINT_CONTESTS, DAMAGE_TO_PLAYERS, DEATHS, PLAYER_KILLS, TOTAL_DAMAGE_DEALT,
        };
        let mut world = board_sim();
        let start_ms = START_MS + PREGAME_MS;
        tick_at(&mut world, start_ms);
        stand_in_outpost(&mut world, FIRST, 0);
        tick_at(&mut world, start_ms + SCORE_TICK_MS);
        let board = &running(&world).board;
        assert_eq!(board.value(FIRST, CAPTURE_POINT_CONTESTS), board::CONTESTS_PER_SCORE_TICK);
        assert_eq!(board.value(SECOND, CAPTURE_POINT_CONTESTS), 0, "not in an outpost");
        world.outpost_rush_hit(SECOND, FIRST, 2_000);
        world.outpost_rush_hit(FIRST, FIRST, 2_000);
        world.outpost_rush_death(SECOND, FIRST);
        let board = &running(&world).board;
        assert_eq!(board.value(FIRST, DAMAGE_TO_PLAYERS), 2_000, "a hit on a team mate counts nothing");
        assert_eq!(board.value(FIRST, TOTAL_DAMAGE_DEALT), 2_000);
        assert_eq!(board.value(FIRST, PLAYER_KILLS), 1);
        assert_eq!(board.value(SECOND, DEATHS), 1);
        assert!(board.score_of(FIRST) >= 50, "the kill's 50 points");
        assert!(world.outpost_rush_board_enabled(FIRST), "a 3206 gets the full pair");
        assert!(!world.outpost_rush_board_enabled(FIRST + SECOND + 1), "not a participant");
    }

    #[test]
    fn a_foe_taking_an_owned_outpost_reports_it_attacked() {
        use super::super::outpost_rush_events::OUTPOST_ATTACKED;
        let mut world = rush_sim();
        let start_ms = START_MS + PREGAME_MS;
        tick_at(&mut world, start_ms);
        stand_in_outpost(&mut world, FIRST, 0);
        let captured_ms = start_ms + capture_ticks(1) * CAPTURE_TICK_MS;
        tick_at(&mut world, captured_ms);
        assert_eq!(running(&world).points[0].owner, 0);
        let attacked = |world: &SimWorld| {
            held_state(world)
                .events
                .iter()
                .filter(|(_, event)| event.name == OUTPOST_ATTACKED && event.number == 0)
                .count()
        };
        assert_eq!(attacked(&world), 0, "no foe yet");
        stand_in_outpost(&mut world, FIRST, 2);
        stand_in_outpost(&mut world, SECOND, 0);
        tick_at(&mut world, captured_ms + CAPTURE_TICK_MS);
        assert_eq!(running(&world).points[0].capturer, 1);
        assert_eq!(attacked(&world), 1, "outpost A attacked");
        tick_at(&mut world, captured_ms + 2 * CAPTURE_TICK_MS);
        assert_eq!(attacked(&world), 1, "once in twenty seconds");
    }

    #[test]
    fn the_armory_opens_for_the_team_that_owns_its_outpost() {
        let mut world = rush_sim();
        let first = world.player_by_number(FIRST).expect("player");
        let second = world.player_by_number(SECOND).expect("player");
        let armory = running(&world)
            .structures
            .structures
            .iter()
            .map(|structure| structure.gde_ref)
            .find(|gde_ref| {
                running(&world)
                    .structures
                    .built_raid(gde_ref, StructureRole::Armory)
                    .is_some()
            })
            .expect("the test armory A");
        assert_eq!(world.armory_status(first, &armory), Some(USE_REFUSED), "no owner yet");
        let start_ms = START_MS + PREGAME_MS;
        tick_at(&mut world, start_ms);
        stand_in_outpost(&mut world, FIRST, 0);
        let captured_ms = start_ms + capture_ticks(1) * CAPTURE_TICK_MS;
        tick_at(&mut world, captured_ms);
        tick_at(&mut world, captured_ms + CAPTURE_TICK_MS);
        assert_eq!(world.armory_status(first, &armory), Some(USE_ACCEPTED), "the owner team");
        assert_eq!(world.armory_status(second, &armory), Some(USE_REFUSED), "the other team");
        assert_eq!(world.armory_status(first, &[0; RAW_ID_LEN]), None, "no armory");
    }
}
