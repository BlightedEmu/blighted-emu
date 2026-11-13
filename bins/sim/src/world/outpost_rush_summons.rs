// SPDX-License-Identifier: AGPL-3.0-or-later
//! The summons of the Outpost Rush maps: the corrupted portal and its stone, the summoning
//! stones, the summoning circles, the summoned bear, brute and wraith with their AI, and the
//! summon counts of the match's end values (game mode helper B11,
//! `local-data/findings/2026-11-13-outpost-rush-summons.md`).
//!
//! Observed (the 12 retail Outpost Rush ledgers of `local-data/capture-index.tsv`; seconds
//! from each ledger's first record; key and event names are the CRC-32 of the lowercase words):
//!
//! - **The stones** (`javelindata_loottables`, `javelindata_gameevents`): `OR_SummonStoneBear`
//!   drops from `OutpostRush_WolfAlpha`, `OR_SummonStoneWraiths` from `OutpostRush_Undead_Brute`
//!   (both by the mobs' own loot), and the game event `OutpostRush_GrantBruteTotem` gives
//!   `OR_SummonStoneBrute` (the portal's stone, below).
//! - **The circles**: six static entities per map, `Summon_Circle_Interact_<A|B|C>_<1|2>`, named
//!   `@ui_outpost_rush_summoning_circle`, two by each outpost; each summon appears on the place
//!   of its circle's `Summon_Circle_<outpost>_<n>` entity (the layout slices
//!   `outpostrush_layout_004_trench`, `gamemode_outpostrush_005_vertical` and
//!   `pois/outpostrush/outpostrush_layout_00`, plus each map's origin: Trench 1024, 1024,
//!   40.993; Vertical 1024, 1024, 31.986; the island's capital 1272.7565, 11016.635, 68.71661,
//!   the z offsets read off the portal stone's cards, `OPR-Coral-Divide-2` 1150.903 s,
//!   `opr_que_and_lost` 692.449 s). Every retail summon was first seen within 2 m of a circle's
//!   place: the bear of `opr_match_4` at 931, 1063 (circle `A_2` 931.03, 1062.54), the bears of
//!   `opr_que_and_lost` at `C_1` and `C_2`, `OPR---ISLAND-2` at `C_1`, the brutes of
//!   `OPR_BAGGUNS`, `OPR-coral-divide2` at `B_1`, `B_2`, of `OPR---ISLAND` at `A_1`, of
//!   `OPR_TEST3` at `B_2`, the wraith of `OPR---Tower-2` at `C_1`.
//! - **The use**: interaction kind 76 (`OpenSummoningSelection`) on a circle opens the client's
//!   choice (`outpostrushsummonpopup.luac`: the stones the bag holds; its accept calls
//!   `OutpostRushSummonBear`, `...Wraith` or `...Brute`); then kind 53 (`InteractWithItemCost`)
//!   names the stone and the action (`opr_match_4` 724.499 s: item `or_summonstonebear`, count
//!   1, action `addsummoningstonebear`; [`nw_messages::InteractionAction`]). Retail answered
//!   each with a 3743 of status 1 and the circle's name (722.677 s, 724.545 s), no 3746, and the
//!   bear's card came at 724.6 s.
//! - **The summons**: `OR_Bear` (level 65, tree `outpost_rush/or_bear`), `OR_Corrupted_Brute`
//!   (63, `or_brute_behavior`) and `OR_Ghost` (65, `or_ghost_base`) (`javelindata_basevitals`,
//!   the level variant tables, the server pak's trees). Each tree's first branch is
//!   `Base_ObjectiveBehaviors`: with a `Guard_Objective_Pos` the mob walks to it, wanders within
//!   5 m of it, fights a target and walks back past 100 m. A summon stays by its circle: the bear
//!   of `opr_match_4` stood within 19 m of `A_2` from 724.6 s to at least 1248.9 s, the bears of
//!   `opr_que_and_lost` by `C_1` and `C_2` for over 700 s; one bear for each circle at a time
//!   (the circle interact's tag `OR_SummonInUse`). The summons' cards carry the temporary
//!   affiliations of a team (`(0, 4), (1, 0)`: team 1's own and other kinds, the wraith of
//!   `OPR---Tower-2`, whose team 1 counted one wraith).
//! - **The counts**: `bearsummoncounts` (`66bac9f8`), `brutesummoncounts` (`6d17e78b`) and
//!   `wraithsummoncounts` (`75a00293`) in the ended state's values, team 1 << 16 | team 0 (the
//!   packing of the end statistics): `opr_match_4` bear 1 for team 0 (its one summon),
//!   `opr_que_and_lost` bear 2 for team 1, `OPR_TEST3` bear 1, brute 2 << 16, wraith 1 << 16.
//! - **The portal** (`slices/outpostrush/corruption/or_portal_corrupted_encounter_00`, one per
//!   map at the layout's `OR_Portal_Corrupted_Encounter_Spawner`): the timer `5543c08f` ends at
//!   the start + 270 s with the event `portalsoon`, the timer `7a0740b0` at the start + 300 s
//!   with `portalopen` (`OPR-Coral-Divide-2` 329.051 and 359.053 s, `opr_que_and_lost` 935.442
//!   and 965.443 s); its encounter has one `OR_Corrupted_Priest`, one `OR_Corrupted_Acolyte` and
//!   one `OR_Corrupted_Legionnaire` (its spawners `PriestSpawn`, `AcolyteSpawn`,
//!   `LegionnaireSpawn` at its place; all level 63), who stood by the portal until they died
//!   (`OPR-Coral-Divide-2` 1165.0, 1169.2 and 1174.3 s). The stone
//!   (`or_azothportal_interact_post_encounter`, at the layout's
//!   `OR_Azoth_Portal_Interact_Post_Encounter_Spawner`) stood with gatherable state 8 and went to
//!   4 at 1177.285 s; a 10 s gather (`OR_Corrupted_Portal_Encounter_Gatherable`: `BaseGatherTime`
//!   10, `GameEventID` `OutpostRush_GrantBruteTotem`, `ResetGatherTimeOnEndInteraction`;
//!   1188.349 to 1198.347 s) ended it: `portalclose` (1198.281 s), the totem (1198.364 s) and
//!   both timers again, `7a0740b0` 300 s and `5543c08f` 270 s later (1198.364 s; and
//!   `opr_que_and_lost` 1817.876 s, `portalsoon` at 2087.883 s).
//!
//! Inferred: any participant of a running match may summon at any free circle (no capture
//! shows a refused summon; the one own summon, `opr_match_4`, was at an outpost of its team); a
//! summon lives until it dies or the match ends, and its circle is free again at its death; it
//! guards its circle's place and never takes a player of its own team as its target; the stone
//! interaction's reason is the craft's use of an item; the portal's stone is ready
//! [`STONE_READY_DELAY_MS`] after the last of its three mobs died (2.99 s retail), one player
//! gathers it at a time, a cancel resets the gather, the gatherer gets the totem and the stone
//! goes at the close. The portal's place on the island has no capture: the layout's spawner plus
//! the capital.
//!
//! Not built: the summons' cards keep their prefab's affiliations (team 1's pair) for both
//! teams (a mob card has no per-mob member; `members.rs`), and a hit of a player on its own
//! team's summon is not refused (`attack.rs`).

use bevy_ecs::entity::Entity;
use nw_bus::Point;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::client_input::GATHER_KIND;
use nw_messages::game_mode_state::Keyed;
use nw_messages::wire::RAW_ID_LEN;
use nw_bus::ConnectionRef;
use nw_messages::{BundleMember, InteractionAction, InteractionRequest, MemberState};
use nw_sim_core::Position;
use std::collections::HashMap;

use super::crafting::{Station, dynamic_ref};
use super::encounter_roots::{card, place_member};
use super::gates::merge_members;
use super::outpost_rush::{TEAMS, TRENCH_FOLDER, VERTICAL_FOLDER, team_scores};
use super::outpost_rush_doors::ISLAND_FOLDER;
use super::outpost_rush_events::NO_NUMBER;
use super::outpost_rush_structures::template_members;
use super::statics::{GatherRun, gather_ended, gather_started};
use super::turrets::{ITEM_COST_KIND, USE_ACCEPTED, USE_REFUSED};
use super::{Body, Player, SimWorld, gather_answer, gather_end};
use crate::mob_ai::SpawnAi;

/// Interaction kind 76 (`InteractionUIActions` `OpenSummoningSelection`).
pub(super) const OPEN_SUMMONING_SELECTION_KIND: u8 = 76;
/// The name of every summoning circle (Observed).
const CIRCLE_NAME: &[u8] = b"@ui_outpost_rush_summoning_circle";
/// How far from a circle its user may stand (Observed: the circle interact's 36 m field;
/// Inferred: its reach).
const CIRCLE_REACH_M: f32 = 36.0;
/// How far from the portal's stone its gatherer may stand (Inferred: the reach of the sim's
/// other gathers).
const STONE_REACH_M: f32 = 15.0;
/// The circles of a map (Observed: two per outpost).
const CIRCLES: usize = 6;
/// One summon.
const ONE: u32 = 1;

/// `portalsoon`, `portalopen` and `portalclose` (Observed).
const PORTAL_SOON: u32 = 0x8d4f_3fa5;
const PORTAL_OPEN: u32 = 0x7a15_db45;
const PORTAL_CLOSE: u32 = 0xc4d2_3672;
/// The timer of `portalsoon` and the timer of the portal's opening (Observed; their names are
/// Unknown: no word list gave their CRCs).
const PORTAL_SOON_TIMER: u32 = 0x5543_c08f;
const PORTAL_TIMER: u32 = 0x7a07_40b0;
/// From the start, and from each close, to the opening (Observed).
const PORTAL_DELAY_MS: u64 = 300_000;
/// From `portalsoon` to the opening (Observed).
const PORTAL_WARNING_MS: u64 = 30_000;
/// From the death of the portal's last mob to its stone's readiness (Inferred from 2.99 s).
pub(super) const STONE_READY_DELAY_MS: u64 = 3_000;
/// The stone's gather (Observed: `BaseGatherTime` 10).
const STONE_GATHER_MS: u64 = 10_000;
/// The stone's gatherable (Observed: the request's gatherable CRC `e42d3184`).
const STONE_GATHERABLE: &str = "OR_Corrupted_Portal_Encounter_Gatherable";
/// The game event of the stone's gather (Observed).
const GRANT_BRUTE_TOTEM: &str = "OutpostRush_GrantBruteTotem";
/// The stone's slice (`or_azothportal_interact_post_encounter`).
const STONE_ASSET: [u8; 16] = [
    0xdc, 0x74, 0x7f, 0x00, 0xa5, 0x48, 0x5b, 0xe5, 0x84, 0x27, 0xa6, 0x11, 0x5d, 0xc7, 0x19,
    0x0a,
];
/// The stone's gatherable state while its mobs live, and when it is ready (Observed).
const STONE_WAITING: u8 = 8;
const STONE_READY: u8 = 4;
/// The namespace of the stones' ids.
const STONE_NAMESPACE: u64 = 0x6f70_7273_746f_6e65;
/// The stone's gather run (Observed: `BaseGatherTime` 10 s, the efficiency of a hand gather).
const STONE_RUN: GatherRun = GatherRun {
    amount: 10.0,
    efficiency: 1.0,
    gatherers: 1,
};
/// The mobs of the portal's encounter and their level (Observed).
const PORTAL_MOBS: [&str; 3] = [
    "OR_Corrupted_Priest",
    "OR_Corrupted_Acolyte",
    "OR_Corrupted_Legionnaire",
];
const PORTAL_MOB_LEVEL: u32 = 63;

/// One kind of summon: its stone, the action that uses it, its mob and its count's key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SummonKind {
    pub(super) stone: &'static str,
    action: &'static str,
    pub(super) vitals: &'static str,
    pub(super) level: u32,
    counts_key: u32,
}

/// The bear, the brute and the wraith (Observed: module doc).
const SUMMON_KINDS: [SummonKind; 3] = [
    SummonKind {
        stone: "OR_SummonStoneBear",
        action: "AddSummoningStoneBear",
        vitals: "OR_Bear",
        level: 65,
        counts_key: 0x66ba_c9f8,
    },
    SummonKind {
        stone: "OR_SummonStoneBrute",
        action: "AddSummoningStoneBrute",
        vitals: "OR_Corrupted_Brute",
        level: 63,
        counts_key: 0x6d17_e78b,
    },
    SummonKind {
        stone: "OR_SummonStoneWraiths",
        action: "AddSummoningStoneWraith",
        vitals: "OR_Ghost",
        level: 65,
        counts_key: 0x75a0_0293,
    },
];

/// A point of a map.
const fn at(x: f32, y: f32, z: f32) -> Point {
    Point { x, y, z }
}

/// The circles and the portal of one map (Observed: module doc).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct SummonMap {
    folder: &'static str,
    /// The place of each circle: `A_1`, `A_2`, `B_1`, `B_2`, `C_1`, `C_2`.
    circles: [Point; CIRCLES],
    /// The portal's encounter and its stone.
    portal: Point,
    stone: Point,
}

const SUMMON_MAPS: [SummonMap; 3] = [
    SummonMap {
        folder: TRENCH_FOLDER,
        circles: [
            at(1013.46, 858.95, 53.27),
            at(925.38, 890.09, 54.27),
            at(1025.47, 978.6, 54.08),
            at(1054.57, 978.93, 54.13),
            at(1067.84, 858.47, 52.42),
            at(1141.5, 883.5, 52.49),
        ],
        portal: at(1054.87, 1130.7, 50.67),
        stone: at(1054.87, 1130.7, 50.67),
    },
    SummonMap {
        folder: VERTICAL_FOLDER,
        circles: [
            at(976.64, 1108.51, 139.09),
            at(931.03, 1062.54, 139.11),
            at(1002.86, 1049.84, 151.61),
            at(1045.59, 998.3, 151.65),
            at(1071.06, 939.44, 139.08),
            at(1116.81, 984.43, 139.14),
        ],
        portal: at(1042.29, 998.27, 135.61),
        stone: at(1042.01, 998.16, 136.61),
    },
    SummonMap {
        folder: ISLAND_FOLDER,
        circles: [
            at(1352.88, 10839.16, 80.44),
            at(1383.74, 10904.34, 80.18),
            at(1270.28, 10985.44, 65.54),
            at(1276.04, 11052.48, 65.25),
            at(1158.97, 11123.34, 55.7),
            at(1190.66, 11194.83, 55.03),
        ],
        portal: at(1095.12, 10951.82, 57.01),
        stone: at(1094.7, 10957.03, 57.52),
    },
];

/// A summon standing on its circle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Summon {
    mob: u64,
    team: u8,
}

/// A player's gather of the portal's stone.
#[derive(Debug, Clone, Copy, PartialEq)]
struct StoneGather {
    entity: Entity,
    number: u64,
    request: InteractionRequest,
    ends_ms: u64,
}

/// The portal's stone in the world: its slot key and id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stone {
    number: u64,
    gde_ref: [u8; RAW_ID_LEN],
}

/// The corrupted portal.
#[derive(Debug, Clone, PartialEq)]
enum Portal {
    /// Before the start.
    Idle,
    /// Closed: `portalsoon` at `soon_ms` (`None` once sent), the opening at `open_ms`.
    Closed { soon_ms: Option<u64>, open_ms: u64 },
    /// Open: its mobs live, or died with the stone ready at `ready_ms`.
    Open {
        mobs: Vec<u64>,
        stone: Stone,
        ready_ms: Option<u64>,
    },
    /// Its stone is ready, maybe gathered.
    Ready {
        stone: Stone,
        gather: Option<StoneGather>,
    },
}

/// The summons of one match.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SummonRun {
    map: Option<SummonMap>,
    /// The kinds of summons (the tests give them their own mobs and stones).
    pub(super) kinds: [SummonKind; 3],
    /// The mobs of the portal and their level.
    pub(super) portal_mobs: [&'static str; 3],
    pub(super) portal_level: u32,
    standing: [Option<Summon>; CIRCLES],
    /// The summons of each kind and team.
    counts: [[u32; TEAMS]; 3],
    /// The last item choice of each player's input ([`SimWorld::note_summoning_choice`]).
    choices: HashMap<u64, InteractionAction>,
    portal: Portal,
    /// The stones spawned so far.
    stones: usize,
}

/// Why a summon was refused: the part it misses (a chosen summon, a free circle, the stone in
/// the bag, or a spawn of the summon).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    Choice,
    Circle,
    Stone,
    Spawn,
}

impl SummonRun {
    /// The summons of a match on the map in `folder`; a map with no circles has none.
    pub(super) fn of_map(folder: &str) -> Self {
        Self {
            map: SUMMON_MAPS.iter().find(|map| map.folder == folder).copied(),
            kinds: SUMMON_KINDS,
            portal_mobs: PORTAL_MOBS,
            portal_level: PORTAL_MOB_LEVEL,
            standing: [None; CIRCLES],
            counts: [[0; TEAMS]; 3],
            choices: HashMap::new(),
            portal: Portal::Idle,
            stones: 0,
        }
    }

    /// The match started at `now_ms`: the portal's timers run.
    pub(super) fn start(&mut self, now_ms: u64) {
        if self.map.is_some() {
            self.portal = closed_from(now_ms);
        }
    }

    /// The end values: the counts of each kind, team 1 << 16 | team 0.
    pub(super) fn end_values(&self) -> Vec<Keyed> {
        self.kinds
            .iter()
            .zip(self.counts)
            .map(|(kind, counts)| (kind.counts_key, team_scores(counts)))
            .collect()
    }

    /// The portal's timers while it is closed.
    pub(super) fn timers(&self) -> Vec<Keyed> {
        match self.portal {
            Portal::Closed { soon_ms, open_ms } => soon_ms
                .map(|soon| (PORTAL_SOON_TIMER, soon))
                .into_iter()
                .chain([(PORTAL_TIMER, open_ms)])
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The summon kind of choice `choice`.
    fn kind_of(&self, choice: InteractionAction) -> Option<usize> {
        self.kinds
            .iter()
            .position(|kind| lowercase_crc32(kind.action) == choice.action)
    }

    /// The free circle nearest to `position` within its reach.
    fn free_circle(&self, position: Position) -> Option<usize> {
        let map = self.map?;
        (0..CIRCLES)
            .filter(|circle| self.standing[*circle].is_none())
            .map(|circle| (circle, position.horizontal_distance(position_of(map.circles[circle]))))
            .filter(|(_, distance)| *distance <= CIRCLE_REACH_M)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(circle, _)| circle)
    }

    /// The mobs this run put in the world: summons and the portal's mobs.
    fn mobs(&self) -> Vec<u64> {
        let portal = match &self.portal {
            Portal::Open { mobs, .. } => mobs.clone(),
            _ => Vec::new(),
        };
        self.standing
            .iter()
            .flatten()
            .map(|summon| summon.mob)
            .chain(portal)
            .collect()
    }

    /// The stone in the world, if one stands.
    const fn stone(&self) -> Option<Stone> {
        match self.portal {
            Portal::Open { stone, .. } | Portal::Ready { stone, .. } => Some(stone),
            Portal::Idle | Portal::Closed { .. } => None,
        }
    }
}

/// The closed portal from `now_ms`: `portalsoon` and the opening to come.
const fn closed_from(now_ms: u64) -> Portal {
    let open_ms = now_ms.saturating_add(PORTAL_DELAY_MS);
    Portal::Closed {
        soon_ms: Some(open_ms.saturating_sub(PORTAL_WARNING_MS)),
        open_ms,
    }
}

const fn position_of(point: Point) -> Position {
    Position {
        x: point.x,
        y: point.y,
        z: point.z,
    }
}

/// The gatherable controller member of the stone at state `state`, with the component of the
/// template's own one.
fn stone_state(members: &[BundleMember], state: u8) -> Option<BundleMember> {
    let mut member = members
        .iter()
        .find(|member| matches!(member.state, MemberState::GatherableController(_)))?
        .clone();
    if let MemberState::GatherableController(controller) = &mut member.state {
        controller.gatherable_state = Some(state);
    }
    Some(member)
}

impl SimWorld {
    /// The item choice of the input of `connection` (kind 53 names the stone and its action):
    /// kept for the player's next circle use while a match runs.
    pub(crate) fn note_summoning_choice(
        &mut self,
        connection: &ConnectionRef,
        choice: Option<InteractionAction>,
    ) {
        let Some(number) = self
            .by_connection
            .get(connection)
            .and_then(|entity| self.world.get::<Player>(*entity))
            .map(|player| player.number)
        else {
            return;
        };
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        match choice {
            Some(choice) => running.summons.choices.insert(number, choice),
            None => running.summons.choices.remove(&number),
        };
    }

    /// A use of a summoning circle by player `entity` (`number`): kind 76 opens the choice,
    /// kind 53 summons the chosen stone's creature. Whether it was such a use.
    pub(super) fn outpost_rush_summoning(
        &mut self,
        entity: Entity,
        number: u64,
        request: InteractionRequest,
        name: &[u8],
    ) -> bool {
        if !matches!(request.kind, OPEN_SUMMONING_SELECTION_KIND | ITEM_COST_KIND)
            || name != CIRCLE_NAME
        {
            return false;
        }
        let Some(running) = self.arena_matches.outpost_rush.as_ref() else {
            return false;
        };
        let team = running.team_of(number).filter(|_| running.is_running());
        let accepted = match (request.kind, team) {
            (OPEN_SUMMONING_SELECTION_KIND, Some(_)) => true,
            (ITEM_COST_KIND, Some(team)) => match self.summon_at_circle(entity, number, team) {
                Ok(()) => true,
                Err(refusal) => {
                    tracing::debug!(player = number, ?refusal, "summon refused");
                    false
                }
            },
            _ => false,
        };
        let status = if accepted { USE_ACCEPTED } else { USE_REFUSED };
        self.send_travel(entity, &[gather_answer(number, request, status, name)]);
        true
    }

    /// Player `entity` (`number`, team `team`) summons the creature of its chosen stone at the
    /// free circle nearest to it: the stone leaves its bag, the summon guards the circle and
    /// spares its team, and its kind's count rises.
    fn summon_at_circle(&mut self, entity: Entity, number: u64, team: u8) -> Result<(), Refusal> {
        let position = self
            .world
            .get::<Body>(entity)
            .map(|body| body.motion.position())
            .ok_or(Refusal::Circle)?;
        let run = &self
            .arena_matches
            .outpost_rush
            .as_ref()
            .ok_or(Refusal::Choice)?
            .summons;
        let choice = run.choices.get(&number).copied().ok_or(Refusal::Choice)?;
        let kind_index = run.kind_of(choice).ok_or(Refusal::Choice)?;
        let kind = run.kinds[kind_index];
        let circle = run.free_circle(position).ok_or(Refusal::Circle)?;
        let place = run.map.ok_or(Refusal::Circle)?.circles[circle];
        let taken = self
            .world
            .get_mut::<Player>(entity)
            .and_then(|mut player| {
                let gear = player.gear.as_mut()?;
                Some(gear.take_as(kind.stone, ONE, nw_messages::items::SYNC_CRAFT_USED))
            })
            .unwrap_or(false);
        if !taken {
            return Err(Refusal::Stone);
        }
        let ai = SpawnAi {
            guard_objective: Some([place.x, place.y, place.z]),
            ..SpawnAi::default()
        };
        let mob = self
            .spawn_gm(kind.vitals, kind.level, place, ONE, Some(&ai))
            .ok()
            .and_then(|spawned| spawned.first().copied())
            .ok_or(Refusal::Spawn)?;
        if let Some(run) = self
            .arena_matches
            .outpost_rush
            .as_mut()
            .map(|running| &mut running.summons)
        {
            run.standing[circle] = Some(Summon { mob, team });
            run.counts[kind_index][usize::from(team.min(1))] += 1;
            run.choices.remove(&number);
        }
        self.spare_team(mob, team);
        tracing::info!(player = number, vitals = kind.vitals, circle, "summoned");
        Ok(())
    }

    /// Summon `mob` of team `team` never takes a player of that team as its target.
    fn spare_team(&mut self, mob: u64, team: u8) {
        let Some(running) = self.arena_matches.outpost_rush.as_ref() else {
            return;
        };
        let own: Vec<u64> = self
            .by_connection
            .values()
            .filter_map(|entity| self.world.get::<Player>(*entity))
            .map(|player| player.number)
            .filter(|number| running.team_of(*number) == Some(team))
            .collect();
        if let Some(mind) = self.mobs.tree_mut(mob) {
            mind.spare(own);
        }
    }

    /// The team of summon `mob` standing on a circle; `None` for any other mob.
    pub(super) fn outpost_rush_summon_team(&self, mob: u64) -> Option<u8> {
        let run = &self.arena_matches.outpost_rush.as_ref()?.summons;
        run.standing.iter().flatten().find(|summon| summon.mob == mob).map(|summon| summon.team)
    }

    /// The summons and the portal at `now_ms`: a dead summon frees its circle, the summons
    /// spare their teams, and the portal goes on. Whether a timer or a value changed.
    pub(super) fn tick_outpost_rush_summons(&mut self, now_ms: u64) -> bool {
        let Some(run) = self
            .arena_matches
            .outpost_rush
            .as_ref()
            .map(|running| running.summons.clone())
        else {
            return false;
        };
        let living = |world: &Self, mob: u64| world.mobs.health(mob).is_some_and(|health| health > 0);
        let mut standing = run.standing;
        for circle in &mut standing {
            if circle.is_some_and(|summon| !living(self, summon.mob)) {
                *circle = None;
            }
        }
        for summon in standing.iter().flatten() {
            self.spare_team(summon.mob, summon.team);
        }
        let portal = self.tick_portal(&run, now_ms, living);
        let changed = portal != run.portal;
        if let Some(running) = self.arena_matches.outpost_rush.as_mut() {
            running.summons.standing = standing;
            running.summons.portal = portal;
        }
        changed
    }

    /// The portal of `run` at `now_ms`: its events, its mobs, its stone and its gather.
    fn tick_portal(&mut self, run: &SummonRun, now_ms: u64, living: impl Fn(&Self, u64) -> bool) -> Portal {
        let Some(map) = run.map else {
            return run.portal.clone();
        };
        match run.portal.clone() {
            Portal::Closed { soon_ms: Some(soon), open_ms } if now_ms >= soon => {
                self.note_portal_event(PORTAL_SOON, now_ms);
                Portal::Closed {
                    soon_ms: None,
                    open_ms,
                }
            }
            Portal::Closed { open_ms, .. } if now_ms >= open_ms => self.open_portal(run, map, now_ms),
            Portal::Open {
                mobs,
                stone,
                ready_ms: None,
            } if mobs.iter().all(|mob| !living(self, *mob)) => Portal::Open {
                mobs,
                stone,
                ready_ms: Some(now_ms.saturating_add(STONE_READY_DELAY_MS)),
            },
            Portal::Open {
                stone,
                ready_ms: Some(ready),
                ..
            } if now_ms >= ready => {
                self.set_stone(stone, STONE_READY);
                Portal::Ready {
                    stone,
                    gather: None,
                }
            }
            Portal::Ready {
                stone,
                gather: Some(gather),
            } => self.tick_stone_gather(stone, gather, now_ms),
            portal => portal,
        }
    }

    /// The portal opens at `now_ms`: `portalopen`, its three mobs and its waiting stone.
    fn open_portal(&mut self, run: &SummonRun, map: SummonMap, now_ms: u64) -> Portal {
        self.note_portal_event(PORTAL_OPEN, now_ms);
        let mobs: Vec<u64> = run
            .portal_mobs
            .iter()
            .filter_map(|vitals| match self.spawn_gm(vitals, run.portal_level, map.portal, ONE, None) {
                Ok(spawned) => spawned.first().copied(),
                Err(reason) => {
                    tracing::warn!(%reason, vitals, "a portal mob did not spawn");
                    None
                }
            })
            .collect();
        let gde_ref = dynamic_ref(STONE_NAMESPACE, run.stones);
        let content = std::sync::Arc::clone(&self.content);
        let template = content
            .outpost_structures
            .slice(&STONE_ASSET)
            .and_then(|slice| slice.foundation.as_ref());
        let members: Vec<BundleMember> = template.map_or_else(Vec::new, |template| {
            std::iter::once(card(STONE_ASSET, gde_ref))
                .chain([place_member(
                    template.place_component,
                    [map.stone.x, map.stone.y, map.stone.z],
                    0.0,
                )])
                .chain(template_members(template))
                .collect()
        });
        if let Some(running) = self.arena_matches.outpost_rush.as_mut() {
            running.summons.stones += 1;
        }
        let number = self.stations.place_camp(|_| Station {
            gde_ref,
            position: position_of(map.stone),
            station_type: String::new(),
            name: Vec::new(),
            members,
        });
        let stone = Stone { number, gde_ref };
        self.set_stone(stone, STONE_WAITING);
        tracing::info!(mobs = mobs.len(), "the corrupted portal opened");
        Portal::Open {
            mobs,
            stone,
            ready_ms: None,
        }
    }

    /// The stone's gatherable state goes to `state`.
    fn set_stone(&mut self, stone: Stone, state: u8) {
        let Some(changed) = self
            .stations
            .members(stone.number)
            .and_then(|members| stone_state(members, state))
        else {
            return;
        };
        self.stations.change_camp(
            stone.number,
            |members| merge_members(members, std::slice::from_ref(&changed)),
            vec![changed.clone()],
        );
    }

    /// Notes portal event `name` at `now_ms` in the match's feed.
    fn note_portal_event(&mut self, name: u32, now_ms: u64) {
        if let Some(running) = self.arena_matches.outpost_rush.as_mut() {
            running.feed.note(name, now_ms, NO_NUMBER);
        }
    }

    /// A gather of the portal's stone by player `entity` (`number`): accepted when the stone is
    /// ready, within reach, and no one else gathers it. Whether the request was on the stone.
    pub(super) fn outpost_rush_portal_gather(
        &mut self,
        entity: Entity,
        number: u64,
        request: InteractionRequest,
        now_ms: u64,
    ) -> bool {
        let Some(running) = self.arena_matches.outpost_rush.as_ref() else {
            return false;
        };
        let Some(stone) = running.summons.stone().filter(|stone| stone.gde_ref == request.target)
        else {
            return false;
        };
        let near = self
            .world
            .get::<Body>(entity)
            .zip(running.summons.map)
            .is_some_and(|(body, map)| {
                body.motion.position().horizontal_distance(position_of(map.stone)) <= STONE_REACH_M
            });
        let gatherable = request.gatherable == Some(lowercase_crc32(STONE_GATHERABLE));
        let free = matches!(running.summons.portal, Portal::Ready { gather: None, .. });
        let joined = running.team_of(number).is_some() && running.is_running();
        let accepted = request.kind == GATHER_KIND && near && gatherable && free && joined;
        let status = if accepted { USE_ACCEPTED } else { USE_REFUSED };
        let answer = gather_answer(number, request, status, &[]);
        self.send_travel(entity, &[answer]);
        if !accepted {
            return true;
        }
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            let started = gather_started(request.target_entity, &mut player.gathers, STONE_RUN);
            player.extra_members.extend(started);
        }
        if let Some(running) = self.arena_matches.outpost_rush.as_mut() {
            running.summons.portal = Portal::Ready {
                stone,
                gather: Some(StoneGather {
                    entity,
                    number,
                    request,
                    ends_ms: now_ms.saturating_add(STONE_GATHER_MS),
                }),
            };
        }
        true
    }

    /// The stone's gather `gather` at `now_ms`: a cancel resets it; its end gives the totem,
    /// closes the portal and takes the stone away.
    fn tick_stone_gather(&mut self, stone: Stone, gather: StoneGather, now_ms: u64) -> Portal {
        let still = self
            .world
            .get::<Player>(gather.entity)
            .is_some_and(|player| player.interacting);
        let done = now_ms >= gather.ends_ms;
        if !still || done {
            if let Some(mut player) = self.world.get_mut::<Player>(gather.entity) {
                let target = gather.request.target_entity;
                let ended = gather_ended(target, &mut player.gathers, STONE_RUN.gatherers, 0);
                player.extra_members.push(ended);
            }
            self.send_travel(gather.entity, &gather_end(gather.number, gather.request));
        }
        if !done {
            return Portal::Ready {
                stone,
                gather: still.then_some(gather),
            };
        }
        self.give_mode_event(gather.entity, gather.number, GRANT_BRUTE_TOTEM);
        self.note_portal_event(PORTAL_CLOSE, now_ms);
        self.stations.remove_camp(stone.number);
        tracing::info!(player = gather.number, "the corrupted portal closed");
        closed_from(now_ms)
    }

    /// The match ends its cleanup: its summons, the portal's mobs and its stone go.
    pub(super) fn remove_outpost_rush_summons(&mut self, run: &SummonRun) {
        self.remove_gm(&run.mobs());
        if let Some(stone) = run.stone() {
            self.stations.remove_camp(stone.number);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::test_support::{TEST_LEVEL, TEST_MOB};
    use crate::world::arena_match::PREGAME_MS;
    use crate::world::outpost_rush::tests::{FIRST, SECOND, START_MS, outpost_rush, rush_sim_on};
    use crate::world::outpost_rush_structures::tests::WOOD;
    use crate::world::tests::{NOW, messages_to};
    use crate::world::turrets::tests::{bag_count, give};
    use crate::world::Placement;
    use nw_messages::ServerMessage;

    /// The test's stones.
    const STONES: u32 = 2;

    /// The summons of the test match.
    fn summons(world: &SimWorld) -> &SummonRun {
        &world
            .arena_matches
            .outpost_rush
            .as_ref()
            .expect("the match")
            .summons
    }

    /// A Trench match past its pregame whose summons and portal mobs are test mobs and whose
    /// stones are wood; player `FIRST` holds two.
    fn started() -> SimWorld {
        let mut world = rush_sim_on(outpost_rush());
        if let Some(running) = world.arena_matches.outpost_rush.as_mut() {
            let run = &mut running.summons;
            for kind in &mut run.kinds {
                kind.vitals = TEST_MOB;
                kind.level = TEST_LEVEL;
                kind.stone = WOOD;
            }
            run.portal_mobs = [TEST_MOB; 3];
            run.portal_level = TEST_LEVEL;
        }
        world.tick(NOW, START_MS + PREGAME_MS).expect("tick");
        let first = world.player_by_number(FIRST).expect("player");
        give(&mut world, first, WOOD, STONES);
        world
    }

    /// Player `number` stands on circle `circle` of the Trench.
    fn stand_on(world: &mut SimWorld, number: u64, circle: usize) {
        let place = position_of(SUMMON_MAPS[0].circles[circle]);
        let entity = world.player_by_number(number).expect("player");
        world.place_player(entity, place, None, Placement::Drop, NOW);
    }

    /// A circle request of kind `kind`.
    fn circle_request(kind: u8) -> InteractionRequest {
        InteractionRequest {
            kind,
            ..crate::world::turrets::tests::use_request([1; RAW_ID_LEN])
        }
    }

    /// The choice of the bear stone.
    fn bear_choice() -> InteractionAction {
        InteractionAction {
            item: lowercase_crc32(SUMMON_KINDS[0].stone),
            count: 1,
            action: lowercase_crc32(SUMMON_KINDS[0].action),
        }
    }

    /// The status of the last 3743 to player `number`.
    fn answer(world: &mut SimWorld, number: u64) -> Option<u8> {
        messages_to(&world.take_messages(), number)
            .iter()
            .rev()
            .find_map(|message| match message {
                ServerMessage::InteractionValidated(answer) => Some(answer.status),
                _ => None,
            })
    }

    /// Player `number` uses the circle it stands on with the bear stone chosen.
    fn use_circle(world: &mut SimWorld, number: u64) -> bool {
        let entity = world.player_by_number(number).expect("player");
        if let Some(running) = world.arena_matches.outpost_rush.as_mut() {
            running.summons.choices.insert(number, bear_choice());
        }
        world.outpost_rush_summoning(entity, number, circle_request(ITEM_COST_KIND), CIRCLE_NAME)
    }

    #[test]
    fn the_keys_and_events_are_the_crcs_of_their_names() {
        let crc = |name: &str| lowercase_crc32(name);
        assert_eq!(SUMMON_KINDS.map(|kind| kind.counts_key), [
            crc("bearsummoncounts"),
            crc("brutesummoncounts"),
            crc("wraithsummoncounts"),
        ]);
        assert_eq!([PORTAL_SOON, PORTAL_OPEN, PORTAL_CLOSE], [
            crc("portalsoon"),
            crc("portalopen"),
            crc("portalclose"),
        ]);
        assert_eq!(lowercase_crc32(SUMMON_KINDS[0].action), 0x2b33_9e09);
        assert_eq!(lowercase_crc32(SUMMON_KINDS[0].stone), 0xe861_084d);
        // Observed big-endian in `OPR-Coral-Divide-2` (the kind 14 gather of the stone).
        assert_eq!(lowercase_crc32(STONE_GATHERABLE), 0xe42d_a604);
    }

    #[test]
    fn the_counts_pack_team_one_high_and_team_zero_low() {
        let mut run = SummonRun::of_map(TRENCH_FOLDER);
        // `OPR_TEST3`: bear 1 for team 0, brute 2 and wraith 1 for team 1.
        run.counts = [[1, 0], [0, 2], [0, 1]];
        assert_eq!(run.end_values(), vec![
            (0x66ba_c9f8, 1),
            (0x6d17_e78b, 2 << 16),
            (0x75a0_0293, 1 << 16),
        ]);
    }

    #[test]
    fn the_portal_warns_after_four_and_a_half_minutes_and_opens_after_five() {
        let mut run = SummonRun::of_map(VERTICAL_FOLDER);
        assert!(run.timers().is_empty(), "no timer before the start");
        run.start(START_MS);
        assert_eq!(run.timers(), vec![
            (PORTAL_SOON_TIMER, START_MS + 270_000),
            (PORTAL_TIMER, START_MS + 300_000),
        ]);
        assert!(SummonRun::of_map("elsewhere").timers().is_empty());
    }

    #[test]
    fn a_circle_takes_the_nearest_free_one_within_its_reach() {
        let mut run = SummonRun::of_map(TRENCH_FOLDER);
        let on_b1 = position_of(SUMMON_MAPS[0].circles[2]);
        assert_eq!(run.free_circle(on_b1), Some(2));
        run.standing[2] = Some(Summon { mob: 1, team: 0 });
        assert_eq!(run.free_circle(on_b1), Some(3), "B_2 stands 29 m away");
        let far = Position {
            x: on_b1.x,
            y: on_b1.y + 100.0,
            z: on_b1.z,
        };
        assert_eq!(run.free_circle(far), None);
        assert_eq!(run.kind_of(bear_choice()), Some(0));
    }

    #[test]
    fn a_stone_on_a_circle_summons_a_guard_that_spares_its_team_and_counts() {
        let mut world = started();
        let first = world.player_by_number(FIRST).expect("player");
        stand_on(&mut world, FIRST, 2);
        let open = circle_request(OPEN_SUMMONING_SELECTION_KIND);
        assert!(world.outpost_rush_summoning(first, FIRST, open, CIRCLE_NAME));
        assert_eq!(answer(&mut world, FIRST), Some(USE_ACCEPTED));
        assert!(use_circle(&mut world, FIRST));
        assert_eq!(answer(&mut world, FIRST), Some(USE_ACCEPTED));
        assert_eq!(bag_count(&world, first, WOOD), STONES - 1, "the stone is used");
        let summon = summons(&world).standing[2].expect("a summon on B_1");
        assert_eq!(summon.team, 0);
        assert_eq!(summons(&world).counts[0], [1, 0]);
        let place = SUMMON_MAPS[0].circles[2];
        let (spawned, _) = world.mobs.places(summon.mob).expect("the mob");
        assert!(spawned.horizontal_distance(position_of(place)) < 1.0);
        let mind = world.mobs.tree(summon.mob).expect("a tree");
        assert_eq!(mind.spared(), [FIRST], "its team is spared");
        assert_eq!(mind.guard_objective(), Some([place.x, place.y, place.z]));
    }

    #[test]
    fn a_circle_in_use_and_a_bag_without_the_stone_refuse_the_summon() {
        let mut world = started();
        stand_on(&mut world, FIRST, 0);
        assert!(use_circle(&mut world, FIRST));
        assert_eq!(answer(&mut world, FIRST), Some(USE_ACCEPTED));
        stand_on(&mut world, SECOND, 0);
        assert!(use_circle(&mut world, SECOND));
        assert_eq!(answer(&mut world, SECOND), Some(USE_REFUSED), "no stone, B_1 too far");
        let first = world.player_by_number(FIRST).expect("player");
        let other = circle_request(ITEM_COST_KIND);
        assert!(!world.outpost_rush_summoning(first, FIRST, other, b"@ui_turret"));
        assert_eq!(summons(&world).counts[0], [1, 0]);
    }

    #[test]
    fn a_dead_summon_frees_its_circle() {
        let mut world = started();
        stand_on(&mut world, FIRST, 1);
        assert!(use_circle(&mut world, FIRST));
        let mob = summons(&world).standing[1].expect("a summon").mob;
        let first = world.player_by_number(FIRST).expect("player");
        world.apply_hit(mob, SECOND, u32::MAX / 2, 0, NOW);
        world.tick_outpost_rush_summons(START_MS + PREGAME_MS + 1);
        assert_eq!(summons(&world).standing[1], None);
        assert_eq!(summons(&world).counts[0], [1, 0], "the count stays");
        assert!(bag_count(&world, first, WOOD) < STONES);
    }

    #[test]
    fn the_portal_opens_its_mobs_die_its_stone_readies_and_its_gather_gives_the_totem() {
        let mut world = started();
        let start = START_MS + PREGAME_MS;
        let soon = start + PORTAL_DELAY_MS - PORTAL_WARNING_MS;
        assert!(world.tick_outpost_rush_summons(soon));
        assert!(matches!(summons(&world).portal, Portal::Closed { soon_ms: None, .. }));
        let open = start + PORTAL_DELAY_MS;
        assert!(world.tick_outpost_rush_summons(open));
        let Portal::Open { mobs, stone, .. } = summons(&world).portal.clone() else {
            panic!("the portal is open");
        };
        assert_eq!(mobs.len(), PORTAL_MOBS.len());
        for mob in &mobs {
            world.apply_hit(*mob, FIRST, u32::MAX / 2, 0, open);
        }
        assert!(world.tick_outpost_rush_summons(open + 1));
        assert!(!world.tick_outpost_rush_summons(open + 2), "not ready yet");
        let ready = open + 1 + STONE_READY_DELAY_MS;
        assert!(world.tick_outpost_rush_summons(ready));
        assert!(matches!(summons(&world).portal, Portal::Ready { gather: None, .. }));
        let first = world.player_by_number(FIRST).expect("player");
        world.place_player(first, position_of(SUMMON_MAPS[0].stone), None, Placement::Drop, NOW);
        let gather = InteractionRequest {
            kind: GATHER_KIND,
            gatherable: Some(lowercase_crc32(STONE_GATHERABLE)),
            ..crate::world::turrets::tests::use_request(stone.gde_ref)
        };
        assert!(world.outpost_rush_portal_gather(first, FIRST, gather, ready));
        assert_eq!(answer(&mut world, FIRST), Some(USE_ACCEPTED));
        if let Some(mut player) = world.world.get_mut::<Player>(first) {
            player.interacting = true;
        }
        let second = world.player_by_number(SECOND).expect("player");
        assert!(world.outpost_rush_portal_gather(second, SECOND, gather, ready));
        assert_eq!(answer(&mut world, SECOND), Some(USE_REFUSED), "one gatherer at a time");
        assert!(world.tick_outpost_rush_summons(ready + STONE_GATHER_MS));
        assert_eq!(summons(&world).portal, closed_from(ready + STONE_GATHER_MS));
        assert!(world.stations.members(stone.number).is_none(), "the stone went");
    }

    #[test]
    fn a_cancelled_stone_gather_resets() {
        let mut world = started();
        let start = START_MS + PREGAME_MS;
        // A tick takes the portal one step: the warning, then the open.
        world.tick_outpost_rush_summons(start + PORTAL_DELAY_MS - PORTAL_WARNING_MS);
        let open = start + PORTAL_DELAY_MS;
        world.tick_outpost_rush_summons(open);
        let Portal::Open { mobs, stone, .. } = summons(&world).portal.clone() else {
            panic!("the portal is open");
        };
        for mob in &mobs {
            world.apply_hit(*mob, FIRST, u32::MAX / 2, 0, open);
        }
        world.tick_outpost_rush_summons(open + 1);
        let ready = open + 1 + STONE_READY_DELAY_MS;
        world.tick_outpost_rush_summons(ready);
        let first = world.player_by_number(FIRST).expect("player");
        world.place_player(first, position_of(SUMMON_MAPS[0].stone), None, Placement::Drop, NOW);
        let gather = InteractionRequest {
            kind: GATHER_KIND,
            gatherable: Some(lowercase_crc32(STONE_GATHERABLE)),
            ..crate::world::turrets::tests::use_request(stone.gde_ref)
        };
        assert!(world.outpost_rush_portal_gather(first, FIRST, gather, ready));
        if let Some(mut player) = world.world.get_mut::<Player>(first) {
            player.interacting = false;
        }
        world.tick_outpost_rush_summons(ready + 1);
        assert!(matches!(summons(&world).portal, Portal::Ready { gather: None, .. }));
    }
}
