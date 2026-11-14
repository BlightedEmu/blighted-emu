// SPDX-License-Identifier: AGPL-3.0-or-later
//! The fight of a territory war in its instance (findings `2026-11-12-wars-and-sieges.md`,
//! section "The framed instance and the war fight"; the forts from [`nw_content::war_forts`]).
//!
//! **The instance** (Observed, `arvaysa_ES_attacker_war_45vs45`): the open world level cut to the
//! fort's 512 m frame, with its own phase ([`super::instances`] opens it; the director opens one
//! per war, `world-director` `war_instances`). Only the mobs whose spawns lie in the frame live
//! in it (Inferred: the client loads only the frame).
//!
//! **The move in** (Observed, same ledger, 1783536301515 to 1783536302144): the roster player
//! accepted a raid invite (3495), got 1677, then the loading screen (3951) and the instance's
//! 1635 with the frame. The invite came in the own groups state 1.6 s before (25.108 s: no
//! inviter, the war id as its activity, the war raid probe of 2026-11-12). So when the
//! director's news comes ([`SimWorld::war_instance_news`]), the roster's players on an
//! open-world sim get the raid invite, and the accept brings the loading screen and the move
//! ([`super::raids`]).
//! A player arrives at its side's Observed arrival place, else at the fort's first Observed one
//! (the spawn camps of the sides are Unknown).
//!
//! **The fight** (Observed in the six war forts of the table; times of the ES ledger from its
//! 3454 at 1783537199900): the siege data entity (card and 4103) is in the instance from the
//! entry; at the start come the 3454 and one 4062 per capture point, and the capture points'
//! groups (card, place, ownership, 333). The outer points start open ([`CAPTURE_POINT_OPEN`]),
//! the claim point locked ([`CAPTURE_POINT_LOCKED`]); a point with attackers in it goes to
//! [`CAPTURE_POINT_CAPTURING`] and its hot data rises about once a second, by 1 to 2 on an
//! outer point and 3 to 4 on the claim point (`everfall_war_2`); it falls while defenders
//! contest it (ES points 1, 2 and 4) and by about 2 a second back to 0 once no attacker is in
//! it; after 99 the point is taken (locked, hot 0). When the three outer points are taken the
//! claim point opens (ES 928.5 s, `everfall_war_2` 155.9 s), and when the claim point is taken
//! the war ends (ES 1216.5 s: four 4063, the 245, and the war data's result for the attacker).
//! Inferred: the rise per second is the number of attackers in the point less the defenders, at
//! most [`OUTER_STEP_MAX`] or [`CLAIM_STEP_MAX`]; a point is held from [`CAPTURE_RADIUS_M`]
//! (the point's radius is Unknown); a war whose time runs out goes to the defender
//! ([`super::wars`]).
//!
//! **PvP and respawn**: only the two sides fight each other, from the start to the result
//! (Inferred: the roster's sides; the open-world faction rule of `pvp_foes` does not hold here).
//! Retail sent the instance's 1635 again in pairs about every 155 s, each pair followed by a new
//! 3454 and the four 4062 (Inferred: the respawn waves): a dead player of a side respawns at its
//! side's arrival place at the next wave, every [`RESPAWN_WAVE_MS`] from the start (Inferred).
//!
//! **The end** (Observed, ES 2189.303 s): the player's leave (2252) got 3455 (value 0), the 5283
//! of its own member, then the loading screen and the return to where it was. Inferred: the
//! players that do not leave go back at the end of the result time (`nw_db::WAR_RESULT_SHOWN_MS`
//! after the result; the retail player left by itself 47.6 s after the end).

use bevy_ecs::entity::Entity;
use nw_bus::{ConnectionRef, WarInstance, WarInstanceNews};
use nw_content::war_forts::{FortPoint, SIDE_ATTACKER, SIDE_DEFENDER, WarFort, WarForts};
use nw_db::TerritoryWar;
use nw_messages::buildables::OwnerIdentification;
use nw_messages::facet::FacetHeader;
use nw_messages::keyed_list::KeyedList;
use nw_messages::level_info::OPEN_WORLD_LEVEL;
use nw_messages::social::groups::REMOVED_LEFT;
use nw_messages::social::{CharacterRef, GroupPush};
use nw_messages::war::WarServerMsg;
use nw_messages::war_states::{
    CAPTURE_POINT_CAPTURING, CAPTURE_POINT_FLAG, CAPTURE_POINT_LOCKED, CAPTURE_POINT_OPEN,
    CapturePointCold, FortStructure as MajorState,
};
use nw_messages::{
    AssetId, BundleMember, CapturePointState, ClaimOwner, DYNAMIC_SLICE_SUB_ID,
    GdeMetadataReplicatedState, LevelSpec, MemberState, OwnershipState, ServerMessage,
    SiegeWarfareDataState,
};
use nw_sim_core::Position;
use std::collections::HashMap;
use uuid::Uuid;

use super::attack::Vitals;
use super::crafting::Station;
use super::siege_areas::SiegeUses;
use super::siege_structures::{PendingBuild, StructureRun};
use super::turrets::TurretRun;
use super::{Body, Player, SimWorld, send_now};
use crate::combat::RespawnRefusal;

/// The capture progress at which a point is taken (Observed: the last hot value of a capture is
/// 97 to 99, then the point is locked with hot 0).
pub(super) const CAPTURE_DONE: u8 = 100;
/// Most rise of an outer point's progress per capture tick (Observed: steps of 1 and 2 in
/// `everfall_war_2`, median 1.94 a second).
pub(super) const OUTER_STEP_MAX: u8 = 2;
/// Most rise of the claim point's progress per capture tick (Observed: steps of 3 and 4 in
/// `everfall_war_2`, median 3.88 a second).
pub(super) const CLAIM_STEP_MAX: u8 = 4;
/// The fall of a point's progress per tick with no attacker in it (Observed: about 2 a second
/// back to 0 after a point went from capturing to open).
pub(super) const FALL_STEP: u8 = 2;
/// The fall per tick while the defenders in a point are at least as many as the attackers
/// (Inferred: the ES points fell while capturing; the rate is not readable).
pub(super) const CONTEST_STEP: u8 = 1;
/// How often the capture progress moves (Observed: the hot values change about once a second).
pub(super) const CAPTURE_TICK_MS: u64 = 1_000;
/// How near a capture point a player holds it, metres (Inferred: the point's radius is Unknown).
pub(super) const CAPTURE_RADIUS_M: f32 = 10.0;
/// The respawn wave period (Inferred: the retail 1635 pairs came about every 155 s).
pub(super) const RESPAWN_WAVE_MS: u64 = 155_000;
/// The 3455 value of a player that leaves the war (Observed: 0).
const COMPLETED_VALUE: u32 = 0;
/// The first byte of the 245 for a player whose side won, and for one whose side lost
/// ([`SimWorld::end_war`]).
const WON_FLAG: u8 = 1;
const LOST_FLAG: u8 = 0;
/// Nanoseconds per millisecond (the 245 time).
const NS_PER_MS: u64 = 1_000_000;
/// The permission rule of a capture point's ownership (Observed: 4 on every ES point).
const POINT_PERMISSION_RULE: u8 = 4;
/// The faction and the option byte of a capture point's ownership (Observed: 0).
const POINT_FACTION: u8 = 0;
const POINT_PERMISSION_OPTIONS: u8 = 0;
/// The cold data value of a point before the start (Observed: 0).
const NO_SIDE_NUMBER: u64 = 0;
/// The version of the siege data's lists.
const FIRST_VERSION: u64 = nw_messages::items::FIRST_VERSION;
/// The hot data of a point with no progress.
const NO_PROGRESS: u8 = 0;

/// Where a war fight is in its run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WarPhase {
    /// Before the start.
    Waiting,
    /// From the start to the result.
    Running,
    /// From the result on.
    Ended,
}

/// One capture point of the running fight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PointRun {
    kind: u8,
    progress: u8,
    /// The slot key of its entity, once it spawned.
    entity: Option<u64>,
}

/// The fight of the war an instance runs.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WarFight {
    war: WarInstance,
    level: LevelSpec,
    fort: WarFort,
    points: Vec<PointRun>,
    /// The slot key of the siege data entity, once it spawned.
    siege: Option<u64>,
    /// The war's start came.
    started: bool,
    /// The claim point opened (the outer points were taken).
    claim_opened: bool,
    /// Unix ms of the result, once the war ended here.
    ended_ms: Option<u64>,
    /// Unix ms of the next capture tick.
    next_capture_ms: u64,
    /// The fort's turrets, once they spawned ([`super::turrets`]).
    pub(super) turrets: Vec<TurretRun>,
    /// The fort's other siege structures, once they spawned ([`super::siege_structures`]).
    pub(super) structures: Vec<StructureRun>,
    /// The builds that wait for their time, and how many structures were built.
    pub(super) builds: Vec<PendingBuild>,
    pub(super) built: usize,
    /// What the uses of its droppers and horns opened ([`super::siege_areas`]).
    pub(super) uses: SiegeUses,
    /// The major structure entries the siege data last showed.
    majors_shown: Vec<(u64, MajorState)>,
}

impl WarFight {
    /// The slot key of its siege data entity, once it spawned.
    pub(super) const fn siege_entity(&self) -> Option<u64> {
        self.siege
    }

    /// The fight of `war` at `fort` in the framed `level`.
    pub(super) fn new(war: WarInstance, fort: WarFort, level: LevelSpec) -> Self {
        let points = fort
            .points
            .iter()
            .map(|point| PointRun {
                kind: point.kind,
                progress: NO_PROGRESS,
                entity: None,
            })
            .collect();
        Self {
            war,
            level,
            fort,
            points,
            siege: None,
            started: false,
            claim_opened: false,
            ended_ms: None,
            next_capture_ms: 0,
            turrets: Vec::new(),
            structures: Vec::new(),
            builds: Vec::new(),
            built: 0,
            uses: SiegeUses::default(),
            majors_shown: Vec::new(),
        }
    }

    /// Whether the war runs: started, with no result yet (its structures may be hurt).
    pub(super) const fn running(&self) -> bool {
        self.started && self.ended_ms.is_none()
    }

    /// Where the war is in its run.
    pub(super) const fn phase(&self) -> WarPhase {
        match (self.started, self.ended_ms) {
            (false, _) => WarPhase::Waiting,
            (true, None) => WarPhase::Running,
            (true, Some(_)) => WarPhase::Ended,
        }
    }

    /// The framed level of the instance.
    pub(super) const fn level(&self) -> LevelSpec {
        self.level
    }

    /// The war.
    pub(super) const fn war(&self) -> WarInstance {
        self.war
    }

    /// The fort.
    pub(super) const fn fort(&self) -> &WarFort {
        &self.fort
    }

    /// The capture points that stand in the instance (spawned at the war's start): the facet
    /// entity of each, its key ([`nw_content::war_forts::FortPoint::key`]; Observed: the 2720
    /// target of a turret shot that met point 2 of territory 11, `joining_defending_war_2`
    /// 1282.655 s), and its place.
    pub(super) fn standing_points(&self) -> impl Iterator<Item = (u64, Position)> + '_ {
        self.fort
            .points
            .iter()
            .zip(&self.points)
            .filter(|(_, run)| run.entity.is_some())
            .map(|(point, _)| {
                let [x, y, z] = point.place;
                (point.key(), Position { x, y, z })
            })
    }

    /// Whether `[x, y]` is inside the instance's frame.
    pub(super) fn frames(&self, x: f32, y: f32) -> bool {
        self.fort.frames(x, y)
    }

    fn cold(&self, index: usize, value: u64) -> Option<CapturePointCold> {
        let point = self.fort.points.get(index)?;
        let run = self.points.get(index)?;
        Some(CapturePointCold {
            place: point.place,
            number: point.number,
            kind: run.kind,
            value,
            flag: CAPTURE_POINT_FLAG,
        })
    }

    /// The fort's major structures as they are now: each gate of
    /// [`nw_content::war_forts::WarFort::major_structures`] by its facet entity, with its place,
    /// its kind, its health in percent and its buildable state (Observed, `RS_offensive_war`;
    /// [`nw_content::war_forts::MajorStructure`]).
    fn majors(&self) -> Vec<(u64, MajorState)> {
        self.structures
            .iter()
            .filter_map(|run| {
                let body = &run.body;
                let kind = self.fort.major_kind(&body.structure.gde_ref)?;
                Some((
                    body.structure.facet_entity(),
                    MajorState {
                        place: body.structure.place,
                        kind,
                        health: body.health_percent(),
                        flag: body.state(),
                    },
                ))
            })
            .collect()
    }

    /// The siege warfare data: the points' cold and hot data and the major structures
    /// (Inferred: the weapons and buildables are left out).
    fn siege_state(&self, value: u64) -> SiegeWarfareDataState {
        let cold = (0..self.points.len())
            .filter_map(|index| Some((self.fort.points[index].key(), self.cold(index, value)?)))
            .collect();
        let hot = self
            .fort
            .points
            .iter()
            .zip(&self.points)
            .map(|(point, run)| (point.key(), run.progress))
            .collect();
        let majors = self.majors();
        SiegeWarfareDataState {
            points_cold: Some(KeyedList::full(FIRST_VERSION, cold)),
            points_hot: Some(KeyedList::full(FIRST_VERSION, hot)),
            structures: (!majors.is_empty()).then(|| KeyedList::full(FIRST_VERSION, majors)),
            ..SiegeWarfareDataState::default()
        }
    }

    /// The 4062 (began) or 4063 (ended) of every capture point.
    fn siege_messages(&self, began: bool) -> Vec<ServerMessage> {
        self.fort
            .points
            .iter()
            .map(|point| {
                let header = FacetHeader {
                    target: point.facet,
                    entity: point.key(),
                };
                ServerMessage::War(if began {
                    WarServerMsg::SiegeBegan { header }
                } else {
                    WarServerMsg::SiegeEnded { header }
                })
            })
            .collect()
    }
}

/// The framed level of every fort of `forts`, by territory: the open world's map and terrain
/// with the fort's frame, a private instance (Observed: the 1635 of every retail war instance).
/// Each is built once (its strings live for the rest of the process).
pub(super) fn war_levels(forts: &WarForts) -> HashMap<u16, LevelSpec> {
    let text = |bytes: &'static [u8]| std::str::from_utf8(bytes).unwrap_or_default();
    forts
        .iter()
        .map(|fort| {
            (
                fort.territory,
                LevelSpec::instance(
                    OPEN_WORLD_LEVEL.folder,
                    text(OPEN_WORLD_LEVEL.map_name),
                    text(OPEN_WORLD_LEVEL.coatlicue_path),
                    fort.frame,
                ),
            )
        })
        .collect()
}

/// A capture point's next kind and progress after one capture tick with `attackers` and
/// `defenders` in it, and whether the attackers took it (module doc). Only an open or capturing
/// point moves.
pub(super) fn capture_step(
    (kind, progress): (u8, u8),
    attackers: usize,
    defenders: usize,
    claim: bool,
) -> (u8, u8, bool) {
    if kind != CAPTURE_POINT_OPEN && kind != CAPTURE_POINT_CAPTURING {
        return (kind, progress, false);
    }
    if attackers == 0 {
        return (
            CAPTURE_POINT_OPEN,
            progress.saturating_sub(FALL_STEP),
            false,
        );
    }
    if defenders >= attackers {
        return (
            CAPTURE_POINT_CAPTURING,
            progress.saturating_sub(CONTEST_STEP),
            false,
        );
    }
    let most = if claim {
        CLAIM_STEP_MAX
    } else {
        OUTER_STEP_MAX
    };
    let step = u8::try_from(attackers - defenders)
        .unwrap_or(u8::MAX)
        .min(most);
    let progress = progress.saturating_add(step);
    if progress >= CAPTURE_DONE {
        return (CAPTURE_POINT_LOCKED, NO_PROGRESS, true);
    }
    (CAPTURE_POINT_CAPTURING, progress, false)
}

/// Unix ms of the first respawn wave at or after `now_ms` of a war that started at `starts_ms`
/// (Inferred, module doc); before the start a dead player respawns at once.
pub(super) const fn next_wave_ms(starts_ms: u64, now_ms: u64) -> u64 {
    if now_ms <= starts_ms {
        return now_ms;
    }
    let waves = (now_ms - starts_ms).div_ceil(RESPAWN_WAVE_MS);
    starts_ms + waves * RESPAWN_WAVE_MS
}

/// The side of `war` whose company is `winner`; `None` for the nil id (a lost invasion).
fn winning_side(war: &TerritoryWar, winner: Uuid) -> Option<u8> {
    if winner.is_nil() {
        None
    } else if winner == war.attacker.0 {
        Some(SIDE_ATTACKER)
    } else if winner == war.defender.0 {
        Some(SIDE_DEFENDER)
    } else {
        None
    }
}

/// The side of `character` in `war`: its roster side.
fn side_of(war: &TerritoryWar, character: Uuid) -> Option<u8> {
    war.roster()
        .find(|(roster, _)| *roster == character)
        .map(|(_, side)| side)
}

/// The ownership of a capture point of the fort of `owner`, the company that holds the
/// territory (Observed, ES outer points at the start: the defending company's id, name and
/// crest, no owner player, rule 4, the defender side's number as the raid id; Inferred for the
/// claim point, which retail showed with no company in ES and with it in the five others).
pub(super) fn point_ownership(owner: &ClaimOwner, defender_number: u64) -> OwnershipState {
    claim_ownership(owner, POINT_PERMISSION_RULE, defender_number, POINT_FACTION)
}

/// The ownership of an entity of the claim of `owner` with permission rule `permission_rule`,
/// raid id `raid_id` and faction `faction` (the war points above; the open world fort control
/// point, [`super::control_points`]): the company's id, name and crest, no owner player.
pub(super) fn claim_ownership(
    owner: &ClaimOwner,
    permission_rule: u8,
    raid_id: u64,
    faction: u8,
) -> OwnershipState {
    OwnershipState {
        guild_id: Some(owner.guild_id),
        guild_owner: Some(owner.guild_name.clone()),
        guild_crest: Some(owner.crest),
        owner: Some(OwnerIdentification::none()),
        permission_rule: Some(permission_rule),
        structure_name: Some(Vec::new()),
        raid_id: Some(raid_id),
        faction_type: Some(faction),
        owner_pvp: Some(false),
        owner_ffa: Some(false),
        permission_options: Some(POINT_PERMISSION_OPTIONS),
    }
}

/// The card of a fort entity of slice `asset` with `gde_ref` at `component`.
fn card(component: u64, asset: [u8; 16], gde_ref: [u8; 16]) -> BundleMember {
    BundleMember {
        component,
        state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
            asset: Some(AssetId {
                id: asset,
                sub_id: DYNAMIC_SLICE_SUB_ID,
            }),
            gde_ref: Some(gde_ref),
        }),
    }
}

/// The 333 member of `point` with `cold` and `progress`.
fn point_member(point: &FortPoint, cold: CapturePointCold, progress: u8) -> BundleMember {
    BundleMember {
        component: point.point_component,
        state: MemberState::CapturePoint(CapturePointState {
            cold: Some(cold),
            hot: Some(progress),
        }),
    }
}

/// The war side numbers of `war`, (attacker, defender): the war data's side numbers, which are
/// the sides' raid ids ([`super::wars`], [`super::raids`]).
///
/// Observed (the war data 1739 of 320 ledgers, 173 wars and 127 invasions; findings
/// `2026-11-13-outpost-rush-turrets.md`, **Batch E1**): a war's defender number is its attacker
/// number + 1 in every entry, and the attacker number has no parity rule (88 even, 85 odd:
/// `arvaysa_ES` 10120/10121 and 9428/9429, `joining_defending_war_2` 535/536, `everfall_war_2`
/// 80/81, `CK_won_offensive_war` 5142/5143, `nighthaven_defensive_war` 1280/1281,
/// `RS_offensive_war` 1370/1371); an invasion's attacker side is empty (0) and its defender takes
/// one number; no two entries of one world list share a number. So retail counts the raid ids of
/// a world: a war takes two in a row, an invasion one. The war's database number is that count's
/// value (world migration 0049: a war takes two of its sequence, an invasion one).
pub(super) const fn side_numbers(war: &TerritoryWar) -> (u64, u64) {
    if war.is_invasion() {
        return (NO_SIDE_NUMBER, war.number);
    }
    (war.number, war.number.saturating_add(1))
}

impl SimWorld {
    /// The fight of the war this instance sim runs.
    pub(super) fn war_fight(&self) -> Option<&WarFight> {
        self.instance_run.as_ref().and_then(|run| run.war_fight())
    }

    /// The roster side of `entity`'s player in the war this sim runs.
    pub(super) fn war_side(&self, entity: Entity) -> Option<u8> {
        self.side_in(self.war_fight()?, entity)
    }

    /// The roster side of `entity`'s player in the war of `fight` (for a fight taken out of
    /// the instance, as [`SimWorld::with_turrets`] does).
    pub(super) fn side_in(&self, fight: &WarFight, entity: Entity) -> Option<u8> {
        let war = self.wars.war(fight.war.war)?;
        let character = self.world.get::<Player>(entity)?.character;
        side_of(war, character)
    }

    /// The director opened the instance of a war (`news`): this open-world sim invites its
    /// players of the war's roster into their side's raid, and their accept moves them in
    /// (module doc; [`SimWorld::invite_war_roster`]).
    pub fn war_instance_news(&mut self, news: WarInstanceNews) {
        if self.instance_sim {
            return;
        }
        self.invite_war_roster(news.war.war, (news.instance, news.sim, news.phase), None);
    }

    /// Whether players `first` and `second` are foes in the war this sim runs: of two sides,
    /// from the start to the result; `None` when this sim runs no war.
    pub(super) fn war_foes(&self, first: Entity, second: Entity) -> Option<bool> {
        let fight = self.war_fight()?;
        let running = fight.started && fight.ended_ms.is_none();
        Some(
            first != second
                && running
                && self
                    .war_side(first)
                    .zip(self.war_side(second))
                    .is_some_and(|(one, other)| one != other),
        )
    }

    /// Where the dead player `entity` respawns in the war this sim runs, at sim ms `now_ms`: its
    /// side's arrival place at the next wave (module doc); `Ok(None)` when this sim runs no war.
    ///
    /// # Errors
    ///
    /// [`RespawnRefusal::TooSoon`] before the next wave.
    pub(super) fn war_respawn(
        &self,
        entity: Entity,
        now_ms: u64,
    ) -> Result<Option<Position>, RespawnRefusal> {
        let Some(fight) = self.war_fight() else {
            return Ok(None);
        };
        let Some(war) = self.wars.war(fight.war.war) else {
            return Ok(None);
        };
        let dead_since = self
            .world
            .get::<Vitals>(entity)
            .and_then(|vitals| vitals.dead_since_ms)
            .ok_or(RespawnRefusal::NotDead)?;
        let unix_now_ms = self.unix_ms();
        let died_unix_ms = unix_now_ms.saturating_sub(now_ms.saturating_sub(dead_since));
        if fight.started
            && fight.ended_ms.is_none()
            && unix_now_ms < next_wave_ms(war.starts_ms, died_unix_ms)
        {
            return Err(RespawnRefusal::TooSoon);
        }
        let side = self.war_side(entity).unwrap_or(SIDE_DEFENDER);
        Ok(fight
            .fort
            .arrival(side)
            .map(|[x, y, z]| Position { x, y, z }))
    }

    /// The 2252 of the client of `connection`: a player in a war instance leaves the war (module
    /// doc); `false` when it is in none (the call goes to the social service).
    pub fn leave_war(&mut self, connection: &ConnectionRef) -> bool {
        let Some(&entity) = self.by_connection.get(connection) else {
            return false;
        };
        if self.war_fight().is_none() {
            return false;
        }
        self.exit_instance(entity);
        true
    }

    /// The messages a player that leaves the war instance gets before its loading screen: 3455
    /// and the 5283 of its own member (Observed, module doc).
    pub(super) fn war_exit(&mut self, entity: Entity) {
        if self.war_fight().is_none() {
            return;
        }
        let Some((number, character)) = self
            .world
            .get::<Player>(entity)
            .map(|player| (player.number, player.character))
        else {
            return;
        };
        self.send_group_push(
            entity,
            number,
            GroupPush::SiegeWarfareCompleted {
                value: COMPLETED_VALUE,
            },
        );
        self.send_group_push(
            entity,
            number,
            GroupPush::MemberRemoved {
                character: CharacterRef(Some(*character.as_bytes())),
                reason: REMOVED_LEFT,
            },
        );
    }

    /// A player arrived in the war instance: after the start it gets the 3454 and the 4062s
    /// (Inferred: retail sent them again after each 1635 of the instance).
    pub(super) fn war_arrived(&mut self, entity: Entity) {
        let Some(fight) = self.war_fight() else {
            return;
        };
        if !fight.started || fight.ended_ms.is_some() {
            return;
        }
        let messages = fight.siege_messages(true);
        let war = fight.war.war;
        self.send_war_start(entity, war, &messages);
    }

    fn send_war_start(&mut self, entity: Entity, war: Uuid, messages: &[ServerMessage]) {
        let Some((number, connection)) = self
            .world
            .get::<Player>(entity)
            .map(|player| (player.number, player.connection.clone()))
        else {
            return;
        };
        // Observed: the raid id of the 3454 is the raid invite's activity, the war's id
        // (`invasion` 62.077 s, the war raid probe of 2026-11-12).
        self.send_group_push(
            entity,
            number,
            GroupPush::SiegeWarfareStarted {
                raid: *war.as_bytes(),
            },
        );
        send_now(&mut self.combat_messages, &connection, messages);
    }

    /// One tick of the war this instance sim runs (module doc): the siege entity, the start, the
    /// capture ticks and the end.
    pub(super) fn tick_war_fight(&mut self) {
        let Some(mut fight) = self
            .instance_run
            .as_mut()
            .and_then(|run| run.take_war_fight())
        else {
            return;
        };
        let now_ms = self.unix_ms();
        if let Some(war) = self.wars.war(fight.war.war).cloned() {
            self.run_war_fight(&mut fight, &war, now_ms);
        }
        if let Some(run) = self.instance_run.as_mut() {
            run.put_war_fight(fight);
        }
    }

    fn run_war_fight(&mut self, fight: &mut WarFight, war: &TerritoryWar, now_ms: u64) {
        let (attacker_number, defender_number) = side_numbers(war);
        self.ask_company_ids([war.attacker.0, war.defender.0]);
        if fight.siege.is_none() {
            self.spawn_structures(fight);
            self.spawn_siege(fight, NO_SIDE_NUMBER);
            fight.majors_shown = fight.majors();
        }
        self.show_majors(fight, attacker_number);
        if !fight.started && now_ms >= war.starts_ms && war.result.is_none() {
            self.start_war(fight, war, (attacker_number, defender_number));
            fight.next_capture_ms = now_ms.saturating_add(CAPTURE_TICK_MS);
        }
        if !fight.started || fight.ended_ms.is_some() {
            return;
        }
        if let Some((winner, at_ms)) = war.result {
            self.end_war(fight, at_ms, winner);
            return;
        }
        if now_ms >= war.ends_ms() {
            // The defender's result comes from the wars' own end rule ([`super::wars`]).
            self.end_war(fight, war.ends_ms(), war.defender.0);
            return;
        }
        if now_ms < fight.next_capture_ms {
            return;
        }
        fight.next_capture_ms = now_ms.saturating_add(CAPTURE_TICK_MS);
        if self.capture_tick(fight, war, attacker_number) {
            self.wars.win(war.war, war.attacker.0, now_ms);
            self.end_war(fight, now_ms, war.attacker.0);
        }
    }

    /// Spawns the siege data entity of the fort, shown to the players near the claim point
    /// (Inferred: the retail group has no place state).
    fn spawn_siege(&mut self, fight: &mut WarFight, value: u64) {
        let fort = &fight.fort;
        // An invasion fort has no claim point: its entity stands at its invasion's place.
        let place = fort
            .points
            .iter()
            .find(|point| point.is_claim())
            .map(|claim| claim.place)
            .or_else(|| self.invasion_place(fort.territory));
        let Some([x, y, z]) = place else {
            return;
        };
        let members = vec![
            card(
                fort.siege.card_component,
                fort.siege.asset,
                fort.siege.gde_ref,
            ),
            BundleMember {
                component: fort.siege.state_component,
                state: MemberState::SiegeWarfareData(Box::new(fight.siege_state(value))),
            },
        ];
        let gde_ref = fort.siege.gde_ref;
        fight.siege = Some(self.stations.place_camp(|_| Station {
            gde_ref,
            position: Position { x, y, z },
            station_type: String::new(),
            name: Vec::new(),
            members,
        }));
    }

    /// The war's start: the capture points spawn, the siege data names the attacker side, and
    /// every player in the instance gets the 3454 and the 4062s.
    fn start_war(
        &mut self,
        fight: &mut WarFight,
        war: &TerritoryWar,
        (attacker, defender): (u64, u64),
    ) {
        fight.started = true;
        self.turrets_at_start(fight);
        let owner = self
            .territories
            .owner(war.territory)
            .cloned()
            .unwrap_or(ClaimOwner::NONE);
        for index in 0..fight.points.len() {
            let Some(cold) = fight.cold(index, attacker) else {
                continue;
            };
            let point = &fight.fort.points[index];
            let Ok((position, _)) = BundleMember::read_from(&point.position) else {
                tracing::warn!(
                    territory = war.territory,
                    point = point.number,
                    "capture point place does not read"
                );
                continue;
            };
            let members = vec![
                card(point.card_component, point.asset, point.gde_ref),
                position,
                BundleMember {
                    component: point.ownership_component,
                    state: MemberState::Ownership(point_ownership(&owner, defender)),
                },
                point_member(point, cold, NO_PROGRESS),
            ];
            let gde_ref = point.gde_ref;
            let [x, y, z] = point.place;
            let number = self.stations.place_camp(|_| Station {
                gde_ref,
                position: Position { x, y, z },
                station_type: String::new(),
                name: Vec::new(),
                members,
            });
            fight.points[index].entity = Some(number);
        }
        self.change_siege(fight, attacker);
        let messages = fight.siege_messages(true);
        let war_id = fight.war.war;
        let players: Vec<Entity> = self.by_connection.values().copied().collect();
        for entity in players {
            self.send_war_start(entity, war_id, &messages);
        }
        tracing::info!(war = %war_id, territory = war.territory, "war started in its instance");
    }

    /// The siege data shows the major structures again when one changed its health percent or
    /// its state (Observed: the entries changed with the gates' hits and break,
    /// `RS_offensive_war`); its cold value is the attacker's number from the start on.
    fn show_majors(&mut self, fight: &mut WarFight, attacker: u64) {
        let majors = fight.majors();
        if majors == fight.majors_shown {
            return;
        }
        fight.majors_shown = majors;
        let value = if fight.started {
            attacker
        } else {
            NO_SIDE_NUMBER
        };
        self.change_siege(fight, value);
    }

    /// The siege data entity gets the points' data as they are now.
    fn change_siege(&mut self, fight: &WarFight, value: u64) {
        let Some(number) = fight.siege else {
            return;
        };
        let member = BundleMember {
            component: fight.fort.siege.state_component,
            state: MemberState::SiegeWarfareData(Box::new(fight.siege_state(value))),
        };
        self.stations.change_camp(
            number,
            |members| replace_member(members, &member),
            vec![member.clone()],
        );
    }

    /// One capture tick: each open or capturing point moves by the players of each side in it
    /// ([`capture_step`]); the claim point opens once the outer points are taken. Whether the
    /// claim point was taken.
    fn capture_tick(&mut self, fight: &mut WarFight, war: &TerritoryWar, attacker: u64) -> bool {
        let mut query = self.world.query::<(&Player, &Body, &Vitals)>();
        let fighters: Vec<(u8, Position)> = query
            .iter(&self.world)
            .filter(|(_, _, vitals)| !vitals.health.is_dead())
            .filter_map(|(player, body, _)| {
                Some((side_of(war, player.character)?, body.motion.position()))
            })
            .collect();
        let mut changed = Vec::new();
        let mut claim_taken = false;
        for index in 0..fight.points.len() {
            let point = &fight.fort.points[index];
            let [x, y, z] = point.place;
            let place = Position { x, y, z };
            let near = |side: u8| {
                fighters
                    .iter()
                    .filter(|(fighter, at)| {
                        *fighter == side && at.horizontal_distance(place) <= CAPTURE_RADIUS_M
                    })
                    .count()
            };
            let run = fight.points[index];
            let (kind, progress, taken) = capture_step(
                (run.kind, run.progress),
                near(SIDE_ATTACKER),
                near(SIDE_DEFENDER),
                point.is_claim(),
            );
            if (kind, progress) != (run.kind, run.progress) {
                fight.points[index].kind = kind;
                fight.points[index].progress = progress;
                changed.push(index);
            }
            claim_taken |= taken && point.is_claim();
        }
        let outer_taken = fight
            .fort
            .points
            .iter()
            .zip(&fight.points)
            .filter(|(point, _)| !point.is_claim())
            .all(|(_, run)| run.kind == CAPTURE_POINT_LOCKED);
        if outer_taken
            && !fight.claim_opened
            && let Some(index) = fight.fort.points.iter().position(FortPoint::is_claim)
        {
            fight.claim_opened = true;
            fight.points[index].kind = CAPTURE_POINT_OPEN;
            changed.push(index);
        }
        if changed.is_empty() {
            return claim_taken;
        }
        for index in changed {
            let (Some(cold), Some(number)) =
                (fight.cold(index, attacker), fight.points[index].entity)
            else {
                continue;
            };
            let member = point_member(
                &fight.fort.points[index],
                cold,
                fight.points[index].progress,
            );
            self.stations.change_camp(
                number,
                |members| replace_member(members, &member),
                vec![member.clone()],
            );
        }
        self.change_siege(fight, attacker);
        claim_taken
    }

    /// The war ended here at Unix ms `at_ms` for `winner` (the nil id: a lost invasion): every
    /// player gets the four 4063 and the 245, and the instance ends at the end of the result
    /// time (module doc). The 245 carries 1 for a player whose side won, else 0, and the result
    /// time in Unix ns (Observed: 1 and 0x18c066e76a1b6afc, the result time, for the winning
    /// attacker of `arvaysa_ES_attacker_war_45vs45`; 1 at the won invasion of `invasion`, 0 at
    /// the lost ones of `Reekwater_Invasion` and `Playthrough_Part_21`, each with its result
    /// time).
    fn end_war(&mut self, fight: &mut WarFight, at_ms: u64, winner: Uuid) {
        fight.ended_ms = Some(at_ms);
        self.turrets_at_result(fight);
        if let Some(run) = self.instance_run.as_mut() {
            run.end_at(at_ms.saturating_add(nw_db::WAR_RESULT_SHOWN_MS));
        }
        let messages = fight.siege_messages(false);
        let players: Vec<Entity> = self.by_connection.values().copied().collect();
        for entity in players {
            let Some((number, connection)) = self
                .world
                .get::<Player>(entity)
                .map(|player| (player.number, player.connection.clone()))
            else {
                continue;
            };
            send_now(&mut self.combat_messages, &connection, &messages);
            let winning_side = self
                .wars
                .war(fight.war.war)
                .and_then(|war| winning_side(war, winner));
            let won = winning_side.is_some() && self.war_side(entity) == winning_side;
            self.send_war_ended_status(entity, number, fight.war.war);
            self.send_group_push(
                entity,
                number,
                GroupPush::SiegeWarfareEnded {
                    flag: if won { WON_FLAG } else { LOST_FLAG },
                    value: at_ms.saturating_mul(NS_PER_MS),
                },
            );
        }
        tracing::info!(war = %fight.war.war, at_ms, "war ended in its instance");
    }
}

/// Puts `member` in place of the member of its component in `members`.
pub(super) fn replace_member(members: &mut [BundleMember], member: &BundleMember) {
    if let Some(slot) = members
        .iter_mut()
        .find(|existing| existing.component == member.component)
    {
        *slot = member.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_outer_point_rises_by_the_attackers_at_most_two_and_is_taken_at_one_hundred() {
        assert_eq!(
            capture_step((CAPTURE_POINT_OPEN, 0), 1, 0, false),
            (CAPTURE_POINT_CAPTURING, 1, false)
        );
        assert_eq!(
            capture_step((CAPTURE_POINT_CAPTURING, 10), 5, 0, false),
            (CAPTURE_POINT_CAPTURING, 10 + OUTER_STEP_MAX, false)
        );
        assert_eq!(
            capture_step((CAPTURE_POINT_CAPTURING, 98), 5, 1, false),
            (CAPTURE_POINT_LOCKED, NO_PROGRESS, true),
            "taken after 99"
        );
        assert_eq!(
            capture_step((CAPTURE_POINT_CAPTURING, 90), 9, 0, true),
            (CAPTURE_POINT_CAPTURING, 90 + CLAIM_STEP_MAX, false),
            "the claim point rises faster"
        );
    }

    #[test]
    fn a_contested_or_left_point_falls_and_a_locked_point_stays() {
        assert_eq!(
            capture_step((CAPTURE_POINT_CAPTURING, 40), 2, 2, false),
            (CAPTURE_POINT_CAPTURING, 40 - CONTEST_STEP, false)
        );
        assert_eq!(
            capture_step((CAPTURE_POINT_CAPTURING, 40), 0, 3, false),
            (CAPTURE_POINT_OPEN, 40 - FALL_STEP, false)
        );
        assert_eq!(
            capture_step((CAPTURE_POINT_OPEN, 1), 0, 0, false),
            (CAPTURE_POINT_OPEN, 0, false)
        );
        assert_eq!(
            capture_step((CAPTURE_POINT_LOCKED, 0), 9, 0, true),
            (CAPTURE_POINT_LOCKED, 0, false)
        );
    }

    #[test]
    fn the_respawn_waves_follow_the_start() {
        const STARTS_MS: u64 = 1_783_537_200_000;
        assert_eq!(
            next_wave_ms(STARTS_MS, STARTS_MS - 5),
            STARTS_MS - 5,
            "before the start"
        );
        assert_eq!(
            next_wave_ms(STARTS_MS, STARTS_MS + 1),
            STARTS_MS + RESPAWN_WAVE_MS
        );
        assert_eq!(
            next_wave_ms(STARTS_MS, STARTS_MS + RESPAWN_WAVE_MS),
            STARTS_MS + RESPAWN_WAVE_MS
        );
        assert_eq!(
            next_wave_ms(STARTS_MS, STARTS_MS + RESPAWN_WAVE_MS + 1),
            STARTS_MS + 2 * RESPAWN_WAVE_MS
        );
    }

    fn fight() -> WarFight {
        let forts = WarForts::parse(nw_content_table()).expect("forts");
        let fort = forts.fort(8).expect("fort").clone();
        let levels = war_levels(&forts);
        WarFight::new(
            WarInstance {
                war: Uuid::from_u128(0x77),
                territory: 8,
            },
            fort,
            levels[&8],
        )
    }

    /// A made-up fort of territory 8 with two points of the retail ES fort: outer point 2 and
    /// the claim point, each with its retail place and position member (the war forts table of
    /// territory 8, `arvaysa_ES_attacker_war_45vs45`).
    fn nw_content_table() -> &'static str {
        r#"{"forts": [{
        "territory": 8, "frame": [5632.0, 5120.0, 512.0, 512.0],
        "arrivals": [{"side": 1, "position": [6001.5, 5291.0, 56.6]}],
        "siege": {"asset": "11111111111111111111111111111111", "gde_ref": "22222222222222222222222222222222",
                  "card_component": 0, "state_component": 2},
        "points": [
          {"number": 1, "asset": "33333333333333333333333333333333",
           "gde_ref": "aabbccddeeff00112222222222222222", "facet": "0a0b0c0d0e0f0002",
           "place": [5972.198, 5363.1045, 56.932175], "kind": 4, "position": "020d010345baa19645a798d6132103b309",
           "card_component": 0, "position_component": 2, "ownership_component": 1, "point_component": 3},
          {"number": 4, "asset": "33333333333333333333333333333333",
           "gde_ref": "08070605040302011111111111111111", "facet": "0a0b0c0d0e0f0001",
           "place": [5902.4795, 5428.118, 57.691372], "kind": 6, "position": "020d010345b873d645a9a0f2133903b309",
           "card_component": 0, "position_component": 2, "ownership_component": 1, "point_component": 4}]}]}"#
    }

    /// The frame of the made-up fort (the retail ES frame).
    const FRAME: [f32; 4] = [5632.0, 5120.0, 512.0, 512.0];

    #[test]
    fn the_war_level_is_the_open_world_cut_to_the_fort_frame_in_private() {
        let fight = fight();
        let level = fight.level();
        assert_eq!(level.map_name, OPEN_WORLD_LEVEL.map_name);
        assert_eq!(level.coatlicue_path, OPEN_WORLD_LEVEL.coatlicue_path);
        assert_eq!(level.phasing_origin, [FRAME[0], FRAME[1]]);
        assert_eq!(level.phasing_bounds, [FRAME[2], FRAME[3]]);
        assert!(level.private);
        assert!(fight.frames(5902.0, 5428.0));
        assert!(!fight.frames(6200.0, 5428.0));
    }

    /// Observed: the siege data keys each point by its key, with its number, kind and place;
    /// the 4062 names the point's facet and key.
    #[test]
    fn the_siege_data_and_the_siege_messages_name_each_point() {
        let fight = fight();
        let state = fight.siege_state(9428);
        let Some(KeyedList::Full(cold)) = &state.points_cold else {
            panic!("full cold data");
        };
        assert_eq!(cold.elements.len(), 2);
        assert_eq!(cold.elements[0].0, 0x1100_ffee_ddcc_bbaa);
        assert_eq!(cold.elements[0].1.kind, CAPTURE_POINT_OPEN);
        assert_eq!(cold.elements[1].1.kind, CAPTURE_POINT_LOCKED);
        assert_eq!(cold.elements[1].1.value, 9428);
        assert!(state.structures.is_none() && state.weapons.is_none());
        let messages = fight.siege_messages(true);
        assert_eq!(
            messages[1],
            ServerMessage::War(WarServerMsg::SiegeBegan {
                header: FacetHeader {
                    target: 0x0a0b_0c0d_0e0f_0001,
                    entity: 0x0102_0304_0506_0708,
                },
            })
        );
        assert!(matches!(
            fight.siege_messages(false)[0],
            ServerMessage::War(WarServerMsg::SiegeEnded { .. })
        ));
    }

    /// Observed: each capture point's place state is the retail member of its point (component,
    /// class 13, place and turn); it reads, at the point's place (a cut-short member does not
    /// read, and the war's start leaves such a point out).
    #[test]
    fn each_point_place_member_reads_at_its_place() {
        const PLACE_PRECISION_M: f32 = 0.01;
        let fight = fight();
        for point in &fight.fort().points {
            let (member, _) = BundleMember::read_from(&point.position).expect("the member reads");
            assert_eq!(member.component, point.position_component);
            let MemberState::Position(state) = member.state else {
                panic!("a place state");
            };
            let at = state.position.expect("a place");
            assert!((at.x - point.place[0]).abs() < PLACE_PRECISION_M);
            assert!((at.y - point.place[1]).abs() < PLACE_PRECISION_M);
            assert_eq!(member.encode().expect("encodes"), point.position, "the retail bytes");
        }
    }

    /// The war's start spawns each capture point with its retail place member, the defender's
    /// company ownership with the defender's raid id, and the attacker's number as its cold value
    /// (Observed, module doc; the table of territory 11 of the turret tests).
    #[test]
    fn the_start_spawns_the_points_with_their_retail_place_and_the_side_numbers() {
        use crate::world::turrets::tests::war_world_of;
        let (mut world, _) = war_world_of(SIDE_DEFENDER);
        let mut war = world.wars.war(Uuid::from_u128(0x77)).cloned().expect("the war");
        war.starts_ms = world.unix_ms();
        world.wars.read(Ok(vec![war.clone()]));
        world.tick_war_fight();
        let (attacker, defender) = side_numbers(&war);
        let fight = world.war_fight().expect("fight");
        let point = &fight.fort().points[0];
        let number = fight.points[0].entity.expect("the point spawned");
        let members = world.stations.members(number).expect("members");
        let place = members
            .iter()
            .find(|member| member.component == point.position_component)
            .and_then(|member| member.encode().ok());
        assert_eq!(place.as_ref(), Some(&point.position), "the retail place member");
        let ownership = members.iter().find_map(|member| match &member.state {
            MemberState::Ownership(state) => Some(state.raid_id),
            _ => None,
        });
        assert_eq!(ownership, Some(Some(defender)), "the defender's raid");
        let cold = members.iter().find_map(|member| match &member.state {
            MemberState::CapturePoint(state) => state.cold.map(|cold| cold.value),
            _ => None,
        });
        assert_eq!(cold, Some(attacker), "the attacker's number");
    }

    /// Observed: the attacker number is the war's count value of either parity, the defender's
    /// the next (`joining_defending_war_2` 535/536, `arvaysa_ES` 9428/9429); an invasion has an
    /// empty attacker side and one defender number.
    #[test]
    fn the_side_numbers_are_the_war_number_and_the_next() {
        let war = |number: u64, attacker: Uuid| TerritoryWar {
            war: Uuid::from_u128(0x77),
            territory: 8,
            attacker: (attacker, 2),
            defender: (Uuid::from_u128(0xd1), 1),
            number,
            starts_ms: 0,
            result: None,
            signups: Vec::new(),
        };
        const ODD_WAR: u64 = 535;
        const EVEN_WAR: u64 = 9428;
        const INVASION: u64 = 5338;
        assert_eq!(side_numbers(&war(ODD_WAR, Uuid::from_u128(0xa1))), (535, 536));
        assert_eq!(side_numbers(&war(EVEN_WAR, Uuid::from_u128(0xa1))), (9428, 9429));
        let invasion = war(INVASION, Uuid::nil());
        assert!(invasion.is_invasion());
        assert_eq!(side_numbers(&invasion), (NO_SIDE_NUMBER, INVASION));
    }
}
