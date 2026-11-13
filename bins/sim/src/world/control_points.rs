// SPDX-License-Identifier: AGPL-3.0-or-later
//! The faction control points of the open world: the fort of each claim and the influence
//! towers (findings `2026-11-12-wars-and-sieges.md`, "Control points and towers"; the shared
//! state in the world database, [`nw_db::control_points`]).
//!
//! **The claims' list** (Observed, every retail 3086): `replicatedFactionControlPointData` holds
//! [`CONTROL_POINT_BYTES`] bytes, two four-bit entries each, the low one first; an entry's bit 0
//! is contested, bit 1 active and bits 2 to 3 the owner faction. Entries 0 to 12 are the forts
//! of claims 0 to 12, 18 to 56 the towers ([`crate::control_point_rules`]). Entries 13 to 17
//! never changed in 1,108 ledgers (their first values differ from world to world; 9 ledgers
//! start with all five zero) and no client code reads them (Observed): the reader
//! (`FUN_142f9e110`) gives entry `i` the key `FUN_142f820d0(i)`, the claim key for the claims'
//! 13 entries, else the highest territory id less the territory count plus `1 + i`
//! (`FUN_142f8fc70` counts the territory list; Inferred from the 18 `IsTerritory` rows of
//! `javelindata_territorydefinitions`, highest id 75: keys 71 to 75 for entries 13 to 17 and
//! 76 to 114 for the towers, all above the claim keys); it sends every entry with its key on
//! `OnFactionControlPointChanged`, and the two listeners keep only their own key: the fort
//! HUD (`factioncontrolpointhud.luac` proto3) the key of the player's territory claim
//! (`Hud.LocalPlayer.CurrentAreaTerritory.ClaimKey`, a territory id of 2 to 17) or a tower's
//! `GetInfluenceTowerClaimKey(GetLandmarkIndex(actor))`, the capture point script
//! (`factioncontrol_capturepoint.luac` proto7) its own `landClaimDataKey`. Inferred: the five
//! are the entries of the five territories without a claim (18 territories less 13 claims) and
//! stay zero here; the list's last half byte (entry 57) is padding (13 + 5 + 39 entries).
//!
//! **Forts** (Observed, `fort_capping_pvp_flagged`, `raid_group_influence_race` and an open
//! world ledger): a fort's control point is a group of its claim's ownership, the card of
//! [`FORT_ASSET`], its place (the claim position) and a 333 state: number
//! [`FORT_POINT_NUMBER`], kind [`KIND_INACTIVE`] in a conflict, [`KIND_LOCKED`] while locked,
//! [`KIND_OPEN`] when it may be taken and [`KIND_CAPTURING`] while a faction takes it; the value
//! is the capturing faction, the hot data the capture percent. One PvP-flagged player took a
//! fort in [`FORT_CAPTURE_MS`] (600 s; another capture took 552 s), about one percent every
//! six seconds, and the progress falls about as fast with no capturer in. A fort captured
//! outside the race window locks for one hour, its lock timer counted from the lock anchor
//! (Observed: anchor + 6,690 s = capture + 3,599 s); a fort captured in the window stays open.
//! The capture shape is the slice's `FactionControlDetectionVolume`, a sphere of
//! [`FORT_RADIUS_M`] (Observed, `factioncontrol_capturepoint.dynamicslice`).
//!
//! **Fort experience** ([`nw_db::FORT_EXPERIENCE_BONUS_PERCENT`]): each tick marks the players
//! whose faction holds the fort of the territory they are in; their character experience gains
//! get the fort's territory buff (Observed value; Inferred scope: the open world sims only, as
//! the capture tick).
//!
//! **Towers** (Observed): active only in the race window of their territory
//! ([`nw_db::in_race_window`]); a tower's owner counts only in the window it was taken in. The
//! capture shape is the slice's `CaptureRadiusDetectionShape`, a cylinder of [`TOWER_RADIUS_M`]
//! and [`TOWER_HEIGHT_M`] (Observed, `factioncontrol_capturepoint_flag_01`). No 333 of a tower is
//! in any ledger, so a tower shows only in the claims' list.
//!
//! **Capture rule** (Inferred, from the observed rates and the tower table's
//! `CaptureTimeInSeconds`, `CaptureTimeMinimum` and `CaptureMaxPlayerDelta`): only living
//! PvP-flagged players of a faction count; the faction with the most players in the shape leads
//! by the difference to the next. A leader that is not the owner takes the point in the capture
//! time of its lead (the base time for one, falling evenly to the shortest at the table's delta;
//! a fort uses the tower table's ratio, 75 / 120, and delta); another faction's progress falls
//! first. With the owner leading or no one in, the progress falls at the one-player rate; equal
//! factions hold it. During a conflict the points are inactive (Observed: kind 0 at the conflict
//! start); a fort stays locked while a war of its territory waits or runs and one hour after its
//! result (Observed text `ui_factioncontrol_reminder_map_inactive_specific`; Inferred: from the
//! war's entry in the world database).

use std::collections::BTreeMap;
use std::sync::Arc;

use nw_db::{ControlPoint, FORT_LOCK_MS, FORT_POINTS, RACE_WINDOW_MS, in_race_window, is_tower};
use nw_messages::position::{PositionInTheWorldReplicatedState, WorldPosition};
use nw_messages::war_states::{CAPTURE_POINT_CAPTURING, CAPTURE_POINT_OPEN, CapturePointCold};
use nw_messages::{
    AssetId, BundleMember, CLAIM_KEYS, CapturePointState, ClaimOwner, DYNAMIC_SLICE_SUB_ID,
    GdeMetadataReplicatedState, MemberState,
};
use nw_sim_core::Position;

use super::attack::Vitals;
use super::crafting::{Station, dynamic_ref};
use super::land_claims::CLAIM_POSITIONS;
use super::war_fight::claim_ownership;
use super::{Body, Player, SimWorld};

/// The bytes of the claims' list (Observed: 29 in every retail 3086).
pub(super) const CONTROL_POINT_BYTES: usize = 29;
/// A lock timer that runs no lock (Observed: on every claim without a lock).
pub(super) const NO_FCP_LOCK: u16 = u16::MAX;
/// The entries of the list: two per byte.
const ENTRIES: usize = CONTROL_POINT_BYTES * 2;
/// The bits of an entry (Observed).
const CONTESTED_BIT: u8 = 0b01;
const ACTIVE_BIT: u8 = 0b10;
const OWNER_SHIFT: u8 = 2;
const ENTRY_BITS: u8 = 4;
/// The faction byte of no faction.
const NO_FACTION: u8 = 0;
/// The factions, 1 to 3.
const FACTIONS: [u8; 3] = [1, 2, 3];

/// How often the captures move on (Inferred: retail's progress moved in steps of whole
/// seconds).
const CAPTURE_TICK_MS: u64 = 1_000;
/// The highest capture percent a running capture shows (Observed: the last hot values of a
/// capture are 97 to 99).
const TOP_PERCENT: u64 = 99;
/// The whole of a capture, percent.
const WHOLE_PERCENT: u64 = 100;
/// The fort capture time of one player (Observed, module doc).
const FORT_CAPTURE_MS: u64 = 600_000;
/// The tower table's capture times, whose ratio a fort's shortest capture takes (Observed:
/// `CaptureTimeInSeconds` 120, `CaptureTimeMinimum` 75 on every row).
const TOWER_TABLE_CAPTURE_S: u64 = 120;
const TOWER_TABLE_CAPTURE_MIN_S: u64 = 75;
/// The fort's shortest capture time (Inferred, module doc).
const FORT_CAPTURE_MIN_MS: u64 =
    FORT_CAPTURE_MS * TOWER_TABLE_CAPTURE_MIN_S / TOWER_TABLE_CAPTURE_S;
/// From how many players more a fort's capture is the shortest (Inferred: the tower table's
/// `CaptureMaxPlayerDelta` 5).
const FORT_MAX_PLAYER_DELTA: u32 = 5;
/// The fort's capture sphere (Observed, module doc).
const FORT_RADIUS_M: f32 = 55.0;
/// A tower's capture cylinder (Observed, module doc), centred on the tower's place (Inferred).
const TOWER_RADIUS_M: f32 = 25.0;
const TOWER_HEIGHT_M: f32 = 20.0;
const MS_PER_S: u64 = 1_000;

/// The fort control point's slice (Observed, every open world fort group).
const FORT_ASSET: AssetId = AssetId {
    id: [
        0x57, 0x38, 0x8a, 0x4f, 0x3a, 0x62, 0x56, 0xa6, 0x8f, 0x85, 0x3b, 0x96, 0x6d, 0x70, 0x1c,
        0xac,
    ],
    sub_id: DYNAMIC_SLICE_SUB_ID,
};
/// The components of a fort's group (Observed: ownership 1, card 0, position 2, 333 3).
const OWNERSHIP_COMPONENT: u64 = 1;
const CARD_COMPONENT: u64 = 0;
const POSITION_COMPONENT: u64 = 2;
const POINT_COMPONENT: u64 = 3;
/// A fort's 333 number and last byte (Observed: 10 and 2 at every fort).
const FORT_POINT_NUMBER: u32 = 10;
const FORT_POINT_FLAG: u8 = 2;
/// A fort's 333 kinds (Observed, module doc).
const KIND_INACTIVE: u8 = 0;
const KIND_LOCKED: u8 = 1;
const KIND_OPEN: u8 = CAPTURE_POINT_OPEN;
const KIND_CAPTURING: u8 = CAPTURE_POINT_CAPTURING;
/// A fort's ownership (Observed: permission rule 3, raid 0).
const FORT_PERMISSION_RULE: u8 = 3;
const FORT_RAID: u64 = 0;
/// The namespace of the fort entities' references.
const FORT_NAMESPACE: u64 = 0x6663_7066_6f72_7473;

/// The shared state of the control points, as this sim last read or changed it; and the claims'
/// list as the clients last got it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct ControlPointStore {
    points: BTreeMap<u8, ControlPoint>,
    /// The changes not yet given to a territory job.
    writes: Vec<ControlPoint>,
    data: Option<Vec<u8>>,
    locks: Option<Vec<u16>>,
    anchor_ms: Option<u64>,
}

impl ControlPointStore {
    /// Takes the rows a job read; the changes made since the job started stay.
    pub(super) fn read(&mut self, rows: Vec<ControlPoint>) {
        self.points = rows.into_iter().map(|point| (point.index, point)).collect();
        for point in &self.writes {
            self.points.insert(point.index, *point);
        }
    }

    /// The changes to store with the next job.
    pub(super) fn take_writes(&mut self) -> Vec<ControlPoint> {
        std::mem::take(&mut self.writes)
    }

    /// Whether changes wait for a job.
    pub(super) const fn has_writes(&self) -> bool {
        !self.writes.is_empty()
    }

    fn store(&mut self, point: ControlPoint) {
        self.points.insert(point.index, point);
        self.writes.push(point);
    }

    /// The claims' list the clients get.
    pub(super) fn data(&self) -> Vec<u8> {
        self.data
            .clone()
            .unwrap_or_else(|| vec![0; CONTROL_POINT_BYTES])
    }

    /// The fort lock timers, by claim.
    pub(super) fn locks(&self) -> Vec<u16> {
        self.locks
            .clone()
            .unwrap_or_else(|| vec![NO_FCP_LOCK; CLAIM_KEYS.len()])
    }

    /// The lock anchor, Unix ms; `None` while no lock runs.
    pub(super) const fn anchor_ms(&self) -> Option<u64> {
        self.anchor_ms
    }

    /// Sets what the clients get; whether it changed.
    fn show(&mut self, data: Vec<u8>, locks: Vec<u16>, anchor_ms: Option<u64>) -> bool {
        let (data, locks) = (Some(data), Some(locks));
        if (&data, &locks, anchor_ms) == (&self.data, &self.locks, self.anchor_ms) {
            return false;
        }
        (self.data, self.locks, self.anchor_ms) = (data, locks, anchor_ms);
        true
    }
}

/// A capture that runs on this sim.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct CaptureRun {
    faction: u8,
    progress: u64,
}

/// What a fort's entity last showed.
#[derive(Debug, Clone, PartialEq)]
struct FortEntity {
    number: u64,
    cold: CapturePointCold,
    percent: u8,
    owner: ClaimOwner,
}

/// The control points this sim's players take.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct FactionControl {
    runs: BTreeMap<u8, CaptureRun>,
    forts: BTreeMap<u8, FortEntity>,
    next_tick_ms: u64,
}

/// How a point is taken.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shape {
    place: Position,
    /// The horizontal reach, metres.
    radius_m: f32,
    /// The height a tower's cylinder holds, metres; `None` for a fort's sphere.
    height_m: Option<f32>,
    capture_ms: u64,
    capture_min_ms: u64,
    max_player_delta: u32,
}

impl Shape {
    fn holds(&self, at: Position) -> bool {
        match self.height_m {
            None => {
                let dz = at.z - self.place.z;
                let flat = at.horizontal_distance(self.place);
                flat.mul_add(flat, dz * dz) <= self.radius_m * self.radius_m
            }
            Some(height) => {
                at.horizontal_distance(self.place) <= self.radius_m
                    && (at.z - self.place.z).abs() <= height / 2.0
            }
        }
    }

    /// The capture time of a lead of `delta` players.
    fn capture_time_ms(&self, delta: u32) -> u64 {
        let most = self.max_player_delta.max(1);
        let steps = u64::from(delta.clamp(1, most) - 1);
        let span = self.capture_ms.saturating_sub(self.capture_min_ms);
        let range = u64::from(most - 1).max(1);
        self.capture_ms - span * steps / range
    }

    /// The progress a lead of `delta` adds in one tick. Progress counts milliseconds of a
    /// one-player capture: a point is taken at [`Shape::capture_ms`].
    fn rise(&self, delta: u32) -> u64 {
        CAPTURE_TICK_MS * self.capture_ms / self.capture_time_ms(delta).max(CAPTURE_TICK_MS)
    }

    /// The progress that falls in one tick.
    const fn fall(&self) -> u64 {
        CAPTURE_TICK_MS
    }

    /// The 333 percent of `progress` (below 100 while a capture runs).
    fn percent_of(&self, progress: u64) -> u8 {
        let percent = progress * WHOLE_PERCENT / self.capture_ms.max(1);
        u8::try_from(percent.min(TOP_PERCENT)).unwrap_or(u8::MAX)
    }
}

/// The faction that leads among `counts` (players of factions 1 to 3) and its lead; `None`
/// when no one is in or the most are equal.
fn leader(counts: [u32; 3]) -> Option<(u8, u32)> {
    let mut sorted: Vec<(u32, u8)> = counts.iter().copied().zip(FACTIONS).collect();
    sorted.sort_unstable_by_key(|(count, _)| std::cmp::Reverse(*count));
    let (most, faction) = sorted[0];
    let delta = most - sorted[1].0;
    (delta > 0).then_some((faction, delta))
}

/// One tick of the run `run` of a point of `owner` with `counts` players in it; whether the
/// capture is done.
fn capture_tick(run: &mut CaptureRun, shape: &Shape, owner: u8, counts: [u32; 3]) -> bool {
    let someone = counts.iter().any(|count| *count > 0);
    match leader(counts) {
        Some((faction, delta)) if faction != owner => {
            if run.faction == faction || run.progress == 0 {
                run.faction = faction;
                run.progress = (run.progress + shape.rise(delta)).min(shape.capture_ms);
            } else {
                run.progress = run.progress.saturating_sub(shape.fall());
            }
        }
        None if someone => {}
        _ => run.progress = run.progress.saturating_sub(shape.fall()),
    }
    if run.progress == 0 {
        run.faction = NO_FACTION;
    }
    run.progress >= shape.capture_ms
}

/// Sets entry `index` of the claims' list `data` to `entry`.
fn set_entry(data: &mut [u8], index: usize, entry: u8) {
    let shift = ENTRY_BITS * u8::from(!index.is_multiple_of(2));
    let byte = &mut data[index / 2];
    *byte = (*byte & !(0x0f << shift)) | ((entry & 0x0f) << shift);
}

/// The entry of a point: active, contested, owner.
const fn entry_of(active: bool, owner: u8, capturing: u8) -> u8 {
    let contested = capturing != NO_FACTION && capturing != owner;
    (owner << OWNER_SHIFT)
        | (if active { ACTIVE_BIT } else { 0 })
        | (if contested { CONTESTED_BIT } else { 0 })
}

impl SimWorld {
    /// The fort of claim `claim` as a point: its territory, entry and shape.
    fn fort_shape(claim: usize) -> Shape {
        let [x, y, z] = CLAIM_POSITIONS[claim];
        Shape {
            place: Position { x, y, z },
            radius_m: FORT_RADIUS_M,
            height_m: None,
            capture_ms: FORT_CAPTURE_MS,
            capture_min_ms: FORT_CAPTURE_MIN_MS,
            max_player_delta: FORT_MAX_PLAYER_DELTA,
        }
    }

    /// Every point: its entry, territory and shape.
    fn control_point_shapes(&self) -> Vec<(u8, u16, Shape)> {
        let forts = CLAIM_KEYS
            .iter()
            .enumerate()
            .filter_map(|(claim, territory)| {
                Some((
                    u8::try_from(claim).ok()?,
                    *territory,
                    Self::fort_shape(claim),
                ))
            });
        let towers = self.content.control_points.towers.iter().map(|tower| {
            let [x, y, z] = tower.position;
            (
                tower.entry,
                tower.territory,
                Shape {
                    place: Position { x, y, z },
                    radius_m: TOWER_RADIUS_M,
                    height_m: Some(TOWER_HEIGHT_M),
                    capture_ms: u64::from(tower.capture_s) * MS_PER_S,
                    capture_min_ms: u64::from(tower.capture_min_s) * MS_PER_S,
                    max_player_delta: tower.max_player_delta,
                },
            )
        });
        forts.chain(towers).collect()
    }

    /// Whether a war of `territory` waits, runs or ended less than an hour ago at Unix ms
    /// `now_ms`.
    fn war_holds_fort(&self, territory: u16, now_ms: u64) -> bool {
        self.wars.all().iter().any(|war| {
            war.territory == territory
                && war
                    .result
                    .is_none_or(|(_, at_ms)| now_ms < at_ms.saturating_add(FORT_LOCK_MS))
        })
    }

    /// The owner of point `entry` as the clients see it at `now_ms`, whether it is active, and
    /// whether a fort is locked.
    fn point_status(&self, entry: u8, territory: u16, now_ms: u64) -> (u8, bool, bool) {
        let stored = self.territories.control().points.get(&entry);
        let conflict = self.territories.conflict(territory);
        let in_conflict = conflict.is_some_and(|conflict| conflict.conflict_faction != NO_FACTION);
        let next_slot = conflict.map_or(0, |conflict| conflict.next_slot_ms);
        if is_tower(entry) {
            let window = in_race_window(next_slot, now_ms) && !in_conflict;
            let owner = stored
                .filter(|point| window && point.captured_ms + RACE_WINDOW_MS >= next_slot)
                .map_or(NO_FACTION, |point| point.owner);
            return (owner, window, false);
        }
        let owner = stored.map_or(NO_FACTION, |point| point.owner);
        let locked = stored.is_some_and(|point| point.locked_until_ms > now_ms)
            || self.war_holds_fort(territory, now_ms);
        (owner, !in_conflict && !locked, locked && !in_conflict)
    }

    /// Moves the captures on once a second ([`FactionControl`]) and shows the claims' list.
    pub(super) fn tick_control_points(&mut self) {
        if self.instance_sim || self.is_tutorial() {
            return;
        }
        let now_ms = self.unix_ms();
        if now_ms < self.faction_control.next_tick_ms {
            return;
        }
        self.faction_control.next_tick_ms = now_ms + CAPTURE_TICK_MS;
        let mut query = self.world.query::<(&Player, &Body, &Vitals)>();
        let flagged: Vec<(u8, Position)> = query
            .iter(&self.world)
            .filter(|(player, _, vitals)| {
                player.pvp_flagged
                    && !vitals.health.is_dead()
                    && player.faction.faction != NO_FACTION
            })
            .map(|(player, body, _)| (player.faction.faction, body.motion.position()))
            .collect();
        for (entry, territory, shape) in self.control_point_shapes() {
            if self.owns_position(shape.place) {
                self.capture_point(entry, territory, &shape, &flagged, now_ms);
            } else {
                self.faction_control.runs.remove(&entry);
            }
            if !is_tower(entry) {
                self.show_fort(entry, territory, &shape, now_ms);
            }
        }
        self.show_control_points(now_ms);
        self.mark_fort_experience_bonus(now_ms);
    }

    /// Marks each player whose faction holds the fort of the territory it is in (the latest it
    /// entered, an area counting as its territory) for the fort's experience bonus
    /// ([`nw_db::FORT_EXPERIENCE_BONUS_PERCENT`]; module doc).
    fn mark_fort_experience_bonus(&mut self, now_ms: u64) {
        let holders: Vec<(u32, u8)> = CLAIM_KEYS
            .iter()
            .enumerate()
            .filter_map(|(claim, territory)| {
                let entry = u8::try_from(claim).ok()?;
                let (owner, _, _) = self.point_status(entry, *territory, now_ms);
                (owner != NO_FACTION).then_some((u32::from(*territory), owner))
            })
            .collect();
        let content = Arc::clone(&self.content);
        let mut query = self.world.query::<&mut Player>();
        for mut player in query.iter_mut(&mut self.world) {
            let faction = player.faction.faction;
            let territory = player.territories.last().map(|id| {
                let id = u32::from(*id);
                content.charting.territory_of(id).unwrap_or(id)
            });
            let bonus = faction != NO_FACTION
                && territory.is_some_and(|territory| holders.contains(&(territory, faction)));
            if player.fort_experience_bonus != bonus {
                player.fort_experience_bonus = bonus;
            }
        }
    }

    /// One capture tick of point `entry` of `territory`, with the flagged players `flagged`.
    fn capture_point(
        &mut self,
        entry: u8,
        territory: u16,
        shape: &Shape,
        flagged: &[(u8, Position)],
        now_ms: u64,
    ) {
        let (owner, active, _) = self.point_status(entry, territory, now_ms);
        if !active {
            self.faction_control.runs.remove(&entry);
            self.store_capturing(entry, territory, owner, NO_FACTION);
            return;
        }
        let mut counts = [0; 3];
        for (faction, at) in flagged {
            if shape.holds(*at)
                && let Some(count) = usize::from(*faction)
                    .checked_sub(1)
                    .and_then(|index| counts.get_mut(index))
            {
                *count += 1;
            }
        }
        let mut run = self
            .faction_control
            .runs
            .get(&entry)
            .copied()
            .unwrap_or_default();
        let done = capture_tick(&mut run, shape, owner, counts);
        if done {
            self.faction_control.runs.remove(&entry);
            let next_slot = self
                .territories
                .conflict(territory)
                .map_or(0, |conflict| conflict.next_slot_ms);
            let locked_until_ms = if is_tower(entry) || in_race_window(next_slot, now_ms) {
                0
            } else {
                now_ms + FORT_LOCK_MS
            };
            tracing::info!(
                entry,
                territory,
                faction = run.faction,
                "control point taken"
            );
            self.territories.control_mut().store(ControlPoint {
                index: entry,
                territory,
                owner: run.faction,
                captured_ms: now_ms,
                capturing: NO_FACTION,
                locked_until_ms,
            });
            return;
        }
        self.faction_control.runs.insert(entry, run);
        self.store_capturing(entry, territory, owner, run.faction);
    }

    /// Stores that `capturing` takes point `entry` of `owner` now, when that changed.
    fn store_capturing(&mut self, entry: u8, territory: u16, owner: u8, capturing: u8) {
        let control = self.territories.control_mut();
        let stored = control.points.get(&entry).copied();
        if stored.map_or(NO_FACTION, |point| point.capturing) == capturing {
            return;
        }
        let point = stored.map_or(
            ControlPoint {
                index: entry,
                territory,
                owner,
                captured_ms: 0,
                capturing,
                locked_until_ms: 0,
            },
            |point| ControlPoint { capturing, ..point },
        );
        control.store(point);
    }

    /// The fort entity of claim `entry` in this sim's cells, as the fort is now.
    fn show_fort(&mut self, entry: u8, territory: u16, shape: &Shape, now_ms: u64) {
        let shown = self.faction_control.forts.get(&entry).cloned();
        if !self.owns_position(shape.place) {
            if let Some(fort) = shown {
                self.stations.remove_camp(fort.number);
                self.faction_control.forts.remove(&entry);
            }
            return;
        }
        let (_, active, locked) = self.point_status(entry, territory, now_ms);
        let run = self
            .faction_control
            .runs
            .get(&entry)
            .copied()
            .unwrap_or_default();
        let kind = match (active, locked, run.faction) {
            (true, _, NO_FACTION) => KIND_OPEN,
            (true, _, _) => KIND_CAPTURING,
            (false, true, _) => KIND_LOCKED,
            (false, false, _) => KIND_INACTIVE,
        };
        let cold = CapturePointCold {
            place: [shape.place.x, shape.place.y, shape.place.z],
            number: FORT_POINT_NUMBER,
            kind,
            value: u64::from(run.faction),
            flag: FORT_POINT_FLAG,
        };
        let percent = shape.percent_of(run.progress);
        let owner = self
            .territories
            .owner(territory)
            .cloned()
            .unwrap_or(ClaimOwner::NONE);
        let point = BundleMember {
            component: POINT_COMPONENT,
            state: MemberState::CapturePoint(CapturePointState {
                cold: Some(cold),
                hot: Some(percent),
            }),
        };
        let ownership = BundleMember {
            component: OWNERSHIP_COMPONENT,
            state: MemberState::Ownership(claim_ownership(
                &owner,
                FORT_PERMISSION_RULE,
                FORT_RAID,
                owner.faction,
            )),
        };
        let Some(fort) = shown else {
            let generation = self.me.generation;
            let place = shape.place;
            let members = vec![ownership, point];
            let number = self.stations.place_camp(|number| {
                // A slot key is far below `usize::MAX` on the 64-bit targets the server runs on.
                let gde_ref = dynamic_ref(FORT_NAMESPACE ^ generation, number as usize);
                let mut all = vec![
                    BundleMember {
                        component: CARD_COMPONENT,
                        state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                            asset: Some(FORT_ASSET),
                            gde_ref: Some(gde_ref),
                        }),
                    },
                    BundleMember {
                        component: POSITION_COMPONENT,
                        state: MemberState::Position(PositionInTheWorldReplicatedState {
                            position: WorldPosition::from_metres(place.x, place.y, place.z),
                            ..PositionInTheWorldReplicatedState::default()
                        }),
                    },
                ];
                all.extend(members);
                Station {
                    gde_ref,
                    position: place,
                    station_type: String::new(),
                    name: Vec::new(),
                    members: all,
                }
            });
            self.faction_control.forts.insert(
                entry,
                FortEntity {
                    number,
                    cold,
                    percent,
                    owner,
                },
            );
            return;
        };
        let mut changed = Vec::new();
        if (fort.cold, fort.percent) != (cold, percent) {
            changed.push(point);
        }
        if fort.owner != owner {
            changed.push(ownership);
        }
        if changed.is_empty() {
            return;
        }
        let merged = changed.clone();
        self.stations.change_camp(
            fort.number,
            |members| {
                for member in &merged {
                    super::war_fight::replace_member(members, member);
                }
            },
            changed,
        );
        self.faction_control.forts.insert(
            entry,
            FortEntity {
                number: fort.number,
                cold,
                percent,
                owner,
            },
        );
    }

    /// The claims' list and the lock timers at `now_ms`, sent again when they changed.
    fn show_control_points(&mut self, now_ms: u64) {
        let mut data = vec![0; CONTROL_POINT_BYTES];
        let mut locks = vec![NO_FCP_LOCK; CLAIM_KEYS.len()];
        let mut lock_ends = Vec::new();
        for (entry, territory, _) in self.control_point_shapes() {
            let (owner, active, locked) = self.point_status(entry, territory, now_ms);
            let capturing = self
                .territories
                .control()
                .points
                .get(&entry)
                .map_or(NO_FACTION, |point| point.capturing);
            let capturing = if active { capturing } else { NO_FACTION };
            if usize::from(entry) < ENTRIES {
                set_entry(
                    &mut data,
                    usize::from(entry),
                    entry_of(active, owner, capturing),
                );
            }
            if locked
                && entry < FORT_POINTS
                && let Some(point) = self.territories.control().points.get(&entry)
                && point.locked_until_ms > now_ms
            {
                lock_ends.push((usize::from(entry), point.locked_until_ms));
            }
        }
        // Inferred: the anchor is the start of the oldest running lock, so every timer fits.
        let anchor_ms = lock_ends
            .iter()
            .map(|(_, end)| end.saturating_sub(FORT_LOCK_MS))
            .min();
        if let Some(anchor) = anchor_ms {
            for (claim, end) in lock_ends {
                let seconds = (end - anchor).div_ceil(MS_PER_S);
                locks[claim] = u16::try_from(seconds).unwrap_or(NO_FCP_LOCK - 1);
            }
        }
        self.territories.show_control_points(data, locks, anchor_ms);
    }
}

impl super::land_claims::Territories {
    /// Sets the claims' control point list; the clients get the claims again when it changed.
    pub(super) fn show_control_points(
        &mut self,
        data: Vec<u8>,
        locks: Vec<u16>,
        anchor_ms: Option<u64>,
    ) {
        if self.control_mut().show(data, locks, anchor_ms) {
            self.claims_changed();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nw_db::TerritoryConflict;

    use super::*;
    use crate::control_point_rules::TowerPoint;
    use crate::world::tests::{NOW, SERVER_TIME, admit_at, table, world_with_spawns};
    use nw_sim_core::Motion;

    const PLAYER: u64 = 3;
    /// Claim 0 is territory 10; tower entry 23 is one of territory 6's.
    const CLAIM: u8 = 0;
    const TOWER_ENTRY: u8 = 23;
    const TOWER_TERRITORY: u16 = 6;
    const TOWER_PLACE: [f32; 3] = [8536.25, 3152.75, 62.5];
    const CAPTURER: u8 = 2;
    /// Server ms of the first tick; the window's slot is ten minutes later.
    const START_MS: u64 = 1_000_000;
    const SLOT_AFTER_MS: u64 = 600_000;

    const TOWER_CAPTURE_MS: u64 = TOWER_TABLE_CAPTURE_S * MS_PER_S;
    const TOWER_CAPTURE_MIN_MS: u64 = TOWER_TABLE_CAPTURE_MIN_S * MS_PER_S;
    const TOWER_MAX_DELTA: u32 = 5;

    fn tower() -> Shape {
        Shape {
            place: Position {
                x: 100.0,
                y: 100.0,
                z: 10.0,
            },
            radius_m: TOWER_RADIUS_M,
            height_m: Some(TOWER_HEIGHT_M),
            capture_ms: TOWER_CAPTURE_MS,
            capture_min_ms: TOWER_CAPTURE_MIN_MS,
            max_player_delta: TOWER_MAX_DELTA,
        }
    }

    fn ticks_to_take(shape: &Shape, owner: u8, counts: [u32; 3]) -> u64 {
        let mut run = CaptureRun::default();
        let mut ticks = 0;
        while !capture_tick(&mut run, shape, owner, counts) {
            ticks += 1;
            assert!(ticks < 10_000, "no capture");
        }
        ticks + 1
    }

    #[test]
    fn one_player_takes_a_fort_in_600_s_and_a_tower_in_120_s() {
        let fort = SimWorld::fort_shape(0);
        assert_eq!(
            ticks_to_take(&fort, 3, [0, 1, 0]) * CAPTURE_TICK_MS,
            FORT_CAPTURE_MS
        );
        assert_eq!(
            ticks_to_take(&tower(), 0, [1, 0, 0]) * CAPTURE_TICK_MS,
            TOWER_CAPTURE_MS
        );
    }

    #[test]
    fn a_bigger_lead_takes_it_faster_down_to_the_minimum() {
        let shape = tower();
        assert_eq!(shape.capture_time_ms(1), TOWER_CAPTURE_MS);
        assert_eq!(shape.capture_time_ms(TOWER_MAX_DELTA), TOWER_CAPTURE_MIN_MS);
        assert_eq!(
            shape.capture_time_ms(TOWER_MAX_DELTA + 3),
            TOWER_CAPTURE_MIN_MS
        );
        assert!(shape.capture_time_ms(3) < shape.capture_time_ms(2));
    }

    #[test]
    fn equal_factions_hold_and_the_owner_or_no_one_makes_it_fall() {
        let shape = tower();
        let mut run = CaptureRun::default();
        capture_tick(&mut run, &shape, 1, [0, 1, 0]);
        let after_one = run;
        assert_eq!(run.faction, 2);
        capture_tick(&mut run, &shape, 1, [1, 1, 0]);
        assert_eq!(run, after_one);
        capture_tick(&mut run, &shape, 1, [2, 1, 0]);
        assert!(run.progress < after_one.progress);
        let mut run = after_one;
        capture_tick(&mut run, &shape, 1, [0, 0, 0]);
        assert_eq!(run, CaptureRun::default());
    }

    #[test]
    fn another_factions_progress_falls_before_its_own_rises() {
        let shape = tower();
        let mut run = CaptureRun {
            faction: 2,
            progress: shape.fall() * 2,
        };
        capture_tick(&mut run, &shape, 1, [0, 0, 1]);
        assert_eq!(run.faction, 2);
        capture_tick(&mut run, &shape, 1, [0, 0, 1]);
        assert_eq!(run, CaptureRun::default());
        capture_tick(&mut run, &shape, 1, [0, 0, 1]);
        assert_eq!(run.faction, 3);
    }

    #[test]
    fn a_tower_cylinder_and_a_fort_sphere_hold_their_players() {
        let shape = tower();
        let at = |x: f32, z: f32| Position { x, y: 100.0, z };
        assert!(shape.holds(at(124.0, 19.0)));
        assert!(!shape.holds(at(126.0, 10.0)));
        assert!(!shape.holds(at(100.0, 21.0)));
        let fort = SimWorld::fort_shape(0);
        let mut near = fort.place;
        near.z += FORT_RADIUS_M - 1.0;
        assert!(fort.holds(near));
        near.z += 2.0;
        assert!(!fort.holds(near));
    }

    #[test]
    fn entries_pack_low_nibble_first_with_owner_active_and_contested_bits() {
        let mut data = vec![0; CONTROL_POINT_BYTES];
        set_entry(&mut data, 0, entry_of(true, 3, 2));
        set_entry(&mut data, 1, entry_of(false, 2, NO_FACTION));
        set_entry(&mut data, 56, entry_of(true, 0, 1));
        // Observed: 15 = active, contested, owner 3; 8 = inactive, owner 2; 3 = active,
        // contested, no owner.
        assert_eq!(data[0], 0x8f);
        assert_eq!(data[28], 0x03);
        assert_eq!(entry_of(true, 2, 2), 0x0a);
    }

    #[test]
    fn the_capture_percent_stays_below_100_while_it_runs() {
        let fort = SimWorld::fort_shape(0);
        assert_eq!(fort.percent_of(0), 0);
        assert_eq!(fort.percent_of(FORT_CAPTURE_MS / 2), 50);
        assert_eq!(fort.percent_of(FORT_CAPTURE_MS), 99);
    }

    /// A world of one sim with player `PLAYER` of faction `CAPTURER`, flagged, at `place`.
    fn world_with_player_at(place: [f32; 3]) -> SimWorld {
        let mut world = world_with_spawns(Vec::new());
        world.set_table(&table(&["sim-a"])).expect("table");
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
        let entity = world.player_by_number(PLAYER).expect("player");
        let limits = world.settings.limits;
        let [x, y, z] = place;
        if let Some(mut body) = world.world.get_mut::<Body>(entity) {
            body.motion = Motion::new(Position { x, y, z }, NOW, &limits);
        }
        let mut player = world.world.get_mut::<Player>(entity).expect("player");
        player.faction.faction = CAPTURER;
        player.pvp_flagged = true;
        world
    }

    /// Runs the control point ticks of `seconds` seconds from `START_MS`.
    fn run_seconds(world: &mut SimWorld, seconds: u64) {
        for second in 0..seconds {
            world.server_ms = START_MS + second * CAPTURE_TICK_MS;
            world.tick_control_points();
        }
    }

    fn entry(world: &SimWorld, index: u8) -> u8 {
        let data = world.territories.control().data();
        let byte = data[usize::from(index) / 2];
        if index.is_multiple_of(2) {
            byte & 0x0f
        } else {
            byte >> ENTRY_BITS
        }
    }

    #[test]
    fn a_flagged_player_takes_a_free_fort_which_locks_outside_the_window() {
        let mut world = world_with_player_at(CLAIM_POSITIONS[usize::from(CLAIM)]);
        run_seconds(&mut world, 1);
        // The capture runs: active and contested, no owner yet; the fort entity is there.
        assert_eq!(entry(&world, CLAIM), ACTIVE_BIT | CONTESTED_BIT);
        assert!(world.faction_control.forts.contains_key(&CLAIM));
        run_seconds(&mut world, FORT_CAPTURE_MS / CAPTURE_TICK_MS + 1);
        let point = world.territories.control().points[&CLAIM];
        assert_eq!(point.owner, CAPTURER);
        assert_eq!(point.territory, CLAIM_KEYS[usize::from(CLAIM)]);
        assert_eq!(point.locked_until_ms, point.captured_ms + FORT_LOCK_MS);
        // Locked: inactive with the new owner, its timer one hour after the anchor.
        assert_eq!(entry(&world, CLAIM), CAPTURER << OWNER_SHIFT);
        let seconds = FORT_LOCK_MS / MS_PER_S;
        assert_eq!(
            u64::from(world.territories.control().locks()[usize::from(CLAIM)]),
            seconds
        );
        assert_eq!(
            world.territories.control().anchor_ms(),
            Some(point.captured_ms)
        );
        assert!(world.territories.control().has_writes());
        let fort = &world.faction_control.forts[&CLAIM];
        assert_eq!(fort.cold.kind, KIND_LOCKED);
    }

    #[test]
    fn the_fort_holders_players_in_its_territory_gain_more_experience() {
        /// A kill's points: with the bonus rounded down, 32 gives 33.
        const POINTS: u32 = 32;
        const WITH_BONUS: u32 = 33;
        const OTHER_FACTION: u8 = 3;
        let mut world = world_with_player_at(CLAIM_POSITIONS[usize::from(CLAIM)]);
        run_seconds(&mut world, FORT_CAPTURE_MS / CAPTURE_TICK_MS + 2);
        assert_eq!(world.territories.control().points[&CLAIM].owner, CAPTURER);
        let entity = world.player_by_number(PLAYER).expect("player");
        let gain = |world: &mut SimWorld| {
            let xp = world.content.gameplay.xp.clone();
            let mut player = world.world.get_mut::<Player>(entity).expect("player");
            let before = player.experience;
            crate::world::quest_objects::gain_experience(&mut player, POINTS, &xp);
            player.experience - before
        };
        assert_eq!(gain(&mut world), POINTS, "outside every territory");
        world
            .world
            .get_mut::<Player>(entity)
            .expect("player")
            .territories = vec![CLAIM_KEYS[usize::from(CLAIM)]];
        world.server_ms += CAPTURE_TICK_MS;
        world.tick_control_points();
        assert_eq!(gain(&mut world), WITH_BONUS, "in the held fort's territory");
        world
            .world
            .get_mut::<Player>(entity)
            .expect("player")
            .faction
            .faction = OTHER_FACTION;
        world.server_ms += CAPTURE_TICK_MS;
        world.tick_control_points();
        assert_eq!(gain(&mut world), POINTS, "another faction's player");
    }

    #[test]
    fn a_tower_is_taken_only_in_its_window_and_shows_no_owner_after_it() {
        let mut world = world_with_player_at(TOWER_PLACE);
        let mut content = (*world.content).clone();
        content.control_points.towers = vec![TowerPoint {
            entry: TOWER_ENTRY,
            territory: TOWER_TERRITORY,
            tower_id: "Made_Up_Tower_01".to_owned(),
            position: TOWER_PLACE,
            capture_s: u32::try_from(TOWER_TABLE_CAPTURE_S).expect("seconds"),
            capture_min_s: u32::try_from(TOWER_TABLE_CAPTURE_MIN_S).expect("seconds"),
            max_player_delta: FORT_MAX_PLAYER_DELTA,
        }];
        world.content = Arc::new(content);
        // No race window: the tower stays inactive.
        run_seconds(&mut world, 2);
        assert_eq!(entry(&world, TOWER_ENTRY), 0);
        world.server_ms = START_MS;
        let unix_start = world.unix_ms();
        world
            .territories
            .read_conflicts_for_test(vec![TerritoryConflict {
                territory: TOWER_TERRITORY,
                influence: [0, 0, 0],
                race_start_ms: 0,
                conflict_faction: NO_FACTION,
                conflict_start_ms: 0,
                lottery_end_ms: 0,
                war_dec_faction: NO_FACTION,
                next_slot_ms: unix_start + SLOT_AFTER_MS,
            }]);
        world.faction_control.next_tick_ms = 0;
        run_seconds(&mut world, TOWER_TABLE_CAPTURE_S + 1);
        let point = world.territories.control().points[&TOWER_ENTRY];
        assert_eq!(point.owner, CAPTURER);
        assert_eq!(point.locked_until_ms, 0);
        assert_eq!(
            entry(&world, TOWER_ENTRY),
            ACTIVE_BIT | (CAPTURER << OWNER_SHIFT)
        );
        // At the slot the window ends: inactive and no owner.
        world.server_ms = START_MS + SLOT_AFTER_MS;
        world.faction_control.next_tick_ms = 0;
        world.tick_control_points();
        assert_eq!(entry(&world, TOWER_ENTRY), 0);
    }
}
