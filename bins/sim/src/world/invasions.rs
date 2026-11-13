// SPDX-License-Identifier: AGPL-3.0-or-later
//! Territory invasions (invasion study 2026-11-12, plan
//! `local-data/notes/2026-11-12-big-systems-plan.md` item 6; the forts from
//! [`nw_content::war_forts`], the encounter and the waves from [`nw_content::invasion_forts`]).
//!
//! An invasion is a war of no attacking company ([`nw_db::TerritoryWar::is_invasion`]): the
//! war code runs its war data, sign-up, roster, raid, instance, respawn and leave
//! ([`super::wars`], [`super::war_fight`]); this module adds what only an invasion has.
//!
//! **Schedule and sign-up** (Observed, war data 1739 and claims 3086 of `invasion`,
//! `Reekwater_Invasion`, `Playthrough_Part_21`, `sign_up_for_invasion`): the war data shows an
//! invasion as kind 2 with a zero attacker company and faction and an empty attacker side
//! (number 0, no group, no raid, key [`NO_SIDE_KEY`]); every sign-up is side 2. The claims show
//! each territory's darkness level and the end of its darkness cycle in nanoseconds since
//! 2000-01-01 ([`DARKNESS_EPOCH_MS`]: the value 847148465503933431 is Unix 1793833265, within
//! the day of `Playthrough_Part_21`), and the tiers of 19 upgrades with their sum as the claim's
//! level ([`UPGRADE_IDS`]). The director steps the darkness and schedules the invasions
//! (`world-director` `invasions`); a game master schedules one at once (`!invasion`).
//!
//! **Invite and move** (Observed, `Playthrough_Part_21`): the pre-raid status 715 with status
//! [`PRE_RAID_INVITED`] and the war id came with the raid invite (1483.633 s), the accept 3495
//! at 1485.049 s, the loading screen 3951 at once; the own groups state then names the
//! defender side's raid with raid type 2 and no opposing raid (`invasion` 62.077 s).
//!
//! **Waves** (Observed in the three invasions): the fort's invasion encounter entity (a level
//! entity of the fort with the defender's ownership) holds the manager and nine stages; at the
//! start the manager, the defense stage and the first wave run; the waves start at
//! [`WAVE_SCHEDULE_MS`] after the start, but a wave starts only once the mobs of the wave before
//! it are dead (territory 16: wave 4 at +535.8 s, as the last mob of wave 3 died, and wave 8 at
//! +1309.6 s); a wave's stage completes when the next one starts. Each wave's mobs come at their
//! retail times after the wave's start, at their retail places, of their vitals and at the fort's
//! mob level ([`nw_content::invasion_forts::WaveSpawn`]); each walks the retail lane of its
//! place, then on to the fort's claim position ([`lane_path`]; Inferred past the lane: the
//! captures show a lane as far as its mob lived).
//!
//! **Defense** (Observed, 4103 child 2 of `invasion`): six fort structures at 100 % fell to 86 %
//! in the last waves; the fort's turrets and siege structures are the war fort's
//! ([`super::turrets`], [`super::siege_structures`]). Inferred: a siege mob (the cow launcher
//! grunts and bombers) within [`SIEGE_REACH_M`] of a structure takes [`SIEGE_DAMAGE_PER_S`]
//! percent of its health a second. The invasion is lost when the corrupted hold the fort's
//! claim position: a living invasion mob within [`CAPTURE_RADIUS_M`] of it and no living player
//! for [`CLAIM_HOLD_MS`] (Inferred: neither lost retail invasion broke a structure, so the loss
//! is not the walls'; retail's rule is Unknown). An invasion whose time runs out is won
//! ([`super::wars`]: the defender wins at the end; Observed: the won invasion ended at its
//! start + 1,500 s).
//!
//! **Corruption areas** (Observed in the data, findings `2026-11-13-catacomb-erosion.md`
//! section C13): the fort slice `siege_fort_dunwood_3407430195` has nine encounter stage
//! entities, `Setup` and `Stage_1` to `Stage_8`; `Setup` spawns the claim, the claim zone clear,
//! two fallback objectives and four `Invasion_CorruptionArea` slices (`ApplyCurseL4`, remove on
//! exit), the others spawn the wave mobs. Observed (remote effect maps): other players of the
//! three captured invasions held `ApplyCurseL4` (`invasion` 97, `Reekwater_Invasion` 151,
//! `Playthrough_Part_21` 300 applications), in `invasion` from 22.9 s after the start (112.991 s)
//! to 1.0 s after the end (1591.050 s). **Inferred**: `Setup` is the defense stage (the one stage
//! beside the eight waves, as the encounter entity holds), so its corruption areas within the
//! fort's frame open at the start and close at the end ([`DEFENSE_STAGE`]).
//!
//! **End** (Observed): at the end the pre-raid status 715 with [`PRE_RAID_ENDED`] and the 245
//! ([`super::war_fight`]); every player of the instance gets the fort's reward event and
//! [`BATTLE_TOKEN_EVENT`] (both after the won invasion of `invasion` and the lost ones of
//! `Reekwater_Invasion` and `Playthrough_Part_21`); at a win every stage and the manager go
//! back to 1 at once, at a loss the manager, the defense stage and the running wave go to 4 and
//! everything to 1 [`CLEANUP_AFTER_LOSS_MS`] later (`Reekwater_Invasion` 956.522 s and
//! 966.588 s). A lost invasion lowers upgrades of its territory by one tier (Observed: six of the
//! 17 raised upgrades of territory 2, four of the 12 of territory 5); Inferred: each raised
//! upgrade with a chance of one in [`LOWER_CHANCE_IN`]. The mobs left go at the cleanup
//! (Inferred).

use std::collections::BTreeMap;
use std::sync::Arc;

use bevy_ecs::entity::Entity;
// The pre-raid status at the invite (a place on the roster) and at the end (module doc).
use nw_bus::ConnectionRef;
use nw_content::invasion_forts::{FortWall, InvasionFort, WaveSpawn};
use nw_db::{
    DbError, InvasionScheduled, PgPool, TerritoryDarkness, TerritoryUpgrade, TerritoryWar,
    UPGRADE_TIER_MAX, lose_invasion, schedule_invasion, set_darkness, set_territory_upgrade,
    territory_darkness, territory_upgrades,
};
use nw_messages::keyed_list::KeyedList;
use nw_messages::social::GroupPush;
use nw_messages::social::groups::{
    PRE_RAID_SELECTED as PRE_RAID_INVITED, PRE_RAID_WAR_ENDED as PRE_RAID_ENDED,
};
use nw_messages::war_states::{FortStructure, NO_SIDE_KEY, WarSide};
use nw_messages::{BundleMember, CLAIM_KEYS, ClaimProgression, MemberState, ProgressionTier};
use nw_sim_core::Position;
use nw_world::records::{PathMode, PatrolPath, VitalsSpawn};
use uuid::Uuid;

use super::attack::Vitals;
use super::crafting::Station;
use super::encounter_roots::{
    History, MANAGER_CLASS, STAGE_CLASS, STATUS_FAILED, STATUS_IN_PROGRESS, STATUS_NOT_STARTED,
    STATUS_SUCCEEDED, manager_body, recorded,
};
use super::gates::merge_members;
use super::gm_chat::DONE;
use super::land_claims::CLAIM_POSITIONS;
use super::town_projects::ProjectSlots;
use super::war_fight::{CAPTURE_RADIUS_M, point_ownership};
use super::wars::WarAnswer;
use super::{Body, Player, SimWorld};
use crate::mob_ai::script_mob_ai;

/// The fort slice stage that runs from the invasion's start to its end (module text).
const DEFENSE_STAGE: &str = "Setup";

/// The `!invasion` usage line.
pub(super) const INVASION_USAGE: &str = "!invasion <territory id> <minutes to start> | <territory id> lose | <territory id> darkness <0-3> | <territory id> upgrade <upgrade id> <0-3>";

/// The words of the `!invasion` changes.
const LOSE: &str = "lose";
const DARKNESS: &str = "darkness";
const UPGRADE: &str = "upgrade";

/// The game master answers.
const NO_OWNER: &str = "That territory has no owner.";
const AT_WAR: &str = "That territory has a war or an invasion already.";
const NO_INVASION: &str = "That territory has no invasion without a result.";
const NOT_STORED: &str = "The database did not take the change.";
const NO_INVASION_FORT: &str = "No capture shows an invasion of that fort.";

/// Milliseconds per minute.
const MS_PER_MINUTE: u64 = 60_000;
/// Nanoseconds per millisecond.
const NS_PER_MS: u64 = 1_000_000;

/// Unix ms of 2000-01-01, the epoch of the claims' darkness cycle end times (module doc).
pub(super) const DARKNESS_EPOCH_MS: u64 = 946_684_800_000;

/// The upgrade ids of a claim's progression, in retail order (Observed in every retail claim of
/// the invasion ledgers: 11 to 17, 8, 7, 0, 5, 4, 6, 9, 1, 3, 2, 20, 21).
pub(super) const UPGRADE_IDS: [u8; 19] = [
    11, 12, 13, 14, 15, 16, 17, 8, 7, 0, 5, 4, 6, 9, 1, 3, 2, 20, 21,
];

/// The game event every player of the instance gets at the end with the fort's reward event
/// (Observed: `War_Battle_Token_Gen` in the three invasion ledgers).
pub(super) const BATTLE_TOKEN_EVENT: &str = "War_Battle_Token_Gen";

/// When each wave is due, ms after the start (Observed: the wave starts of territories 2 and 5).
pub(super) const WAVE_SCHEDULE_MS: [u64; 8] = [
    0, 170_000, 340_000, 510_000, 700_000, 890_000, 1_080_000, 1_290_000,
];

/// How long after a loss the encounter is cleaned up (Observed: `Reekwater_Invasion` 956.522 s
/// to 966.588 s).
pub(super) const CLEANUP_AFTER_LOSS_MS: u64 = 10_000;

/// How often the waves, the siege and the claim are checked (Inferred: as the war's capture
/// tick).
const INVASION_TICK_MS: u64 = 1_000;

/// Milliseconds per second.
const MS_PER_SECOND: f32 = 1000.0;

/// [`INVASION_TICK_MS`] in seconds.
#[allow(clippy::cast_precision_loss, reason = "the tick is a second")]
const TICK_SECONDS: f32 = INVASION_TICK_MS as f32 / MS_PER_SECOND;

/// How near a siege mob hits a structure, m (Inferred).
pub(super) const SIEGE_REACH_M: f32 = 30.0;

/// The health percent a siege mob takes from a structure each second (Inferred).
pub(super) const SIEGE_DAMAGE_PER_S: f32 = 1.0;

/// How long the corrupted hold the claim position before the invasion is lost (Inferred).
pub(super) const CLAIM_HOLD_MS: u64 = 10_000;

/// A raised upgrade is lowered at a loss with a chance of one in this (Inferred, module doc).
pub(super) const LOWER_CHANCE_IN: u32 = 3;

/// The empty attacker side of an invasion in the war data (Observed, module doc).
pub(super) fn empty_side() -> WarSide {
    WarSide {
        number: 0,
        groups: Vec::new(),
        raid: [0; nw_messages::RAW_ID_LEN],
        key: NO_SIDE_KEY,
    }
}

/// The darkness, the upgrade tiers and the town projects of the territories, as read with the
/// wars.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Dominion {
    darkness: BTreeMap<u16, TerritoryDarkness>,
    upgrades: BTreeMap<(u16, u8), u8>,
    projects: ProjectSlots,
}

impl Dominion {
    /// Reads them from the world database at Unix ms `now_ms` (the town projects due end
    /// first); `None` (logged) when a read fails.
    pub(crate) async fn read(world: &PgPool, now_ms: u64) -> Option<Self> {
        let read = async {
            Ok::<_, DbError>((
                territory_darkness(world).await?,
                territory_upgrades(world).await?,
                ProjectSlots::read(world, now_ms).await?,
            ))
        };
        match read.await {
            Ok((darkness, upgrades, projects)) => Some(Self {
                projects,
                ..Self::of(darkness, upgrades)
            }),
            Err(error) => {
                tracing::warn!(%error, "territory darkness, upgrades and projects not read");
                None
            }
        }
    }

    fn of(darkness: Vec<TerritoryDarkness>, upgrades: Vec<TerritoryUpgrade>) -> Self {
        Self {
            darkness: darkness
                .into_iter()
                .map(|row| (row.territory, row))
                .collect(),
            upgrades: upgrades
                .into_iter()
                .map(|row| ((row.territory, row.upgrade), row.tier))
                .collect(),
            projects: ProjectSlots::default(),
        }
    }

    /// The town projects ([`super::town_projects`]).
    pub(super) const fn projects(&self) -> &ProjectSlots {
        &self.projects
    }

    /// The claims' darkness threshold of `territory`.
    pub(super) fn level(&self, territory: u16) -> u32 {
        self.darkness
            .get(&territory)
            .map_or(0, |darkness| u32::from(darkness.level))
    }

    /// The claims' darkness cycle end of `territory`, ns since 2000-01-01; 0 without darkness.
    pub(super) fn cycle_end_ns(&self, territory: u16) -> u64 {
        self.darkness.get(&territory).map_or(0, |darkness| {
            darkness
                .cycle_end_ms
                .saturating_sub(DARKNESS_EPOCH_MS)
                .saturating_mul(NS_PER_MS)
        })
    }

    /// The tier of `upgrade` of `territory`.
    pub(super) fn tier(&self, territory: u16, upgrade: u8) -> u8 {
        self.upgrades
            .get(&(territory, upgrade))
            .copied()
            .unwrap_or(0)
    }

    /// The claims' progression of `territory`: every upgrade's tier and their sum as the level
    /// (module doc), and one town project entry per progression type
    /// ([`ProjectSlots::entries`]).
    pub(super) fn progression(&self, territory: u16) -> ClaimProgression {
        let tiers: Vec<ProgressionTier> = UPGRADE_IDS
            .iter()
            .map(|upgrade| ProgressionTier {
                id: u32::from(*upgrade),
                tier: self.tier(territory, *upgrade),
            })
            .collect();
        let level = tiers
            .iter()
            .map(|tier| tier.tier)
            .fold(0_u8, u8::saturating_add);
        ClaimProgression {
            tiers,
            entries: self.projects.entries(territory),
            level,
        }
    }

    /// The upgrades of `territory` a loss lowers: each raised one where `lowered` says so.
    pub(super) fn lowered(&self, territory: u16, mut lowered: impl FnMut() -> bool) -> Vec<u8> {
        UPGRADE_IDS
            .iter()
            .copied()
            .filter(|upgrade| self.tier(territory, *upgrade) > 0)
            .filter(|_| lowered())
            .collect()
    }
}

/// What `!invasion` asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum InvasionCommand {
    /// An invasion of the territory's owner starts in `minutes`.
    Schedule { minutes: u64 },
    /// The territory's invasion is lost now.
    Lose,
    /// The territory's darkness gets `level`, with its cycle end due now (the director steps
    /// it at its next read).
    Darkness { level: u8 },
    /// An upgrade of the territory gets `tier`.
    Upgrade { upgrade: u8, tier: u8 },
}

impl InvasionCommand {
    /// The change the words after the territory id name.
    pub(super) fn read(words: &[&str]) -> Option<Self> {
        let number = |word: &str| word.parse::<u64>().ok();
        let small = |word: &str| word.parse::<u8>().ok();
        match words {
            [word] if word.eq_ignore_ascii_case(LOSE) => Some(Self::Lose),
            [word, level] if word.eq_ignore_ascii_case(DARKNESS) => {
                let level = small(level)?;
                (level <= nw_db::DARKNESS_INVASION_LEVEL).then_some(Self::Darkness { level })
            }
            [word, upgrade, tier] if word.eq_ignore_ascii_case(UPGRADE) => {
                let (upgrade, tier) = (small(upgrade)?, small(tier)?);
                (UPGRADE_IDS.contains(&upgrade) && tier <= UPGRADE_TIER_MAX)
                    .then_some(Self::Upgrade { upgrade, tier })
            }
            [minutes] => Some(Self::Schedule {
                minutes: number(minutes)?,
            }),
            _ => None,
        }
    }
}

/// An invasion change to make in the world database, with whom to answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InvasionJob {
    /// A game master schedules an invasion.
    Schedule {
        territory: u16,
        starts_ms: u64,
        answer_to: ConnectionRef,
    },
    /// An invasion is lost, its territory's `lowered` upgrades lose a tier.
    Lose {
        war: Uuid,
        at_ms: u64,
        lowered: Vec<u8>,
    },
    /// A game master sets a territory's darkness.
    Darkness {
        darkness: TerritoryDarkness,
        answer_to: ConnectionRef,
    },
    /// A game master sets an upgrade's tier.
    Upgrade {
        upgrade: TerritoryUpgrade,
        answer_to: ConnectionRef,
    },
}

fn stored_line(stored: Result<(), DbError>) -> &'static str {
    match stored {
        Ok(()) => DONE,
        Err(error) => {
            tracing::error!(%error, "invasion change not stored");
            NOT_STORED
        }
    }
}

impl InvasionJob {
    /// Runs the job against the world database.
    pub(crate) async fn run(self, world: &PgPool) -> WarAnswer {
        match self {
            Self::Schedule {
                territory,
                starts_ms,
                answer_to,
            } => {
                let line = match schedule_invasion(world, territory, starts_ms).await {
                    Ok(InvasionScheduled::Scheduled) => DONE,
                    Ok(InvasionScheduled::NoOwner) => NO_OWNER,
                    Ok(InvasionScheduled::AtWar) => AT_WAR,
                    Err(error) => stored_line(Err(error)),
                };
                WarAnswer::Gm(answer_to, line)
            }
            Self::Lose {
                war,
                at_ms,
                lowered,
            } => {
                match lose_invasion(world, war, at_ms, &lowered).await {
                    Ok(true) => tracing::info!(%war, ?lowered, "invasion lost"),
                    Ok(false) => tracing::warn!(%war, "lost invasion had a result already"),
                    Err(error) => tracing::error!(%error, %war, "invasion loss not stored"),
                }
                WarAnswer::Quiet
            }
            Self::Darkness {
                darkness,
                answer_to,
            } => WarAnswer::Gm(answer_to, stored_line(set_darkness(world, &darkness).await)),
            Self::Upgrade { upgrade, answer_to } => WarAnswer::Gm(
                answer_to,
                stored_line(set_territory_upgrade(world, &upgrade).await),
            ),
        }
    }
}

/// The way invasion mob `spawn` walks from its place (offsets from it, [`PatrolPath`]): its lane,
/// then the claim position `claim` unless the lane ends within [`CAPTURE_RADIUS_M`] of it
/// (module doc).
pub(super) fn lane_path(spawn: &WaveSpawn, claim: [f32; 3]) -> PatrolPath {
    let [x, y, z] = spawn.place;
    let offset = |[px, py, pz]: [f32; 3]| [px - x, py - y, pz - z];
    let position = |[px, py, pz]: [f32; 3]| Position {
        x: px,
        y: py,
        z: pz,
    };
    let end = spawn.path.last().copied().unwrap_or(spawn.place);
    let at_claim = position(end).horizontal_distance(position(claim)) <= CAPTURE_RADIUS_M;
    let mut points: Vec<[f32; 3]> = spawn.path.iter().copied().map(offset).collect();
    if !at_claim {
        points.push(offset(claim));
    }
    PatrolPath {
        points,
        waits_ms: Vec::new(),
        mode: PathMode::Once,
        tasks: Vec::new(),
    }
}

/// When wave `index` is due, Unix ms, for an invasion that started at `starts_ms`.
pub(super) fn wave_due_ms(starts_ms: u64, index: usize) -> Option<u64> {
    WAVE_SCHEDULE_MS
        .get(index)
        .map(|after| starts_ms.saturating_add(*after))
}

/// Whether wave `next` starts at Unix ms `now_ms` (module doc): it is due, and the running wave
/// spawned all its mobs and they died.
pub(super) fn next_wave_starts(starts_ms: u64, next: usize, now_ms: u64, cleared: bool) -> bool {
    cleared && wave_due_ms(starts_ms, next).is_some_and(|due| now_ms >= due)
}

/// The health of a structure at `health` percent after `seconds` with `siege_mobs` siege mobs in
/// reach.
pub(super) fn sieged_health(health: f32, siege_mobs: usize, seconds: f32) -> f32 {
    #[allow(clippy::cast_precision_loss, reason = "a few hundred mobs at most")]
    let mobs = siege_mobs as f32;
    (health - mobs * SIEGE_DAMAGE_PER_S * seconds).max(0.0)
}

/// The claim hold after a check at Unix ms `now_ms`: since when the corrupted hold the claim
/// position (`None`: they do not), and whether they held it [`CLAIM_HOLD_MS`].
pub(super) fn claim_hold(
    held_since_ms: Option<u64>,
    corrupted_in: bool,
    defenders_in: bool,
    now_ms: u64,
) -> (Option<u64>, bool) {
    if !corrupted_in || defenders_in {
        return (None, false);
    }
    let since = held_since_ms.unwrap_or(now_ms);
    (Some(since), now_ms.saturating_sub(since) >= CLAIM_HOLD_MS)
}

/// A mob of a wave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WaveMob {
    entity: u64,
    wave: usize,
    siege: bool,
}

/// How an invasion ended here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Ended {
    won: bool,
    /// Unix ms of the encounter's cleanup, until it is done.
    cleanup_ms: Option<u64>,
}

/// The invasion an instance sim runs.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InvasionRun {
    war: Uuid,
    fort: InvasionFort,
    /// The claim position: the encounter entity's place and the corrupted's goal.
    claim: [f32; 3],
    /// The slot key of the encounter entity, once it spawned.
    entity: Option<u64>,
    manager: u32,
    defense: History,
    waves: Vec<History>,
    started: bool,
    /// The running wave and when it started, Unix ms.
    wave: Option<(usize, u64)>,
    /// The next spawn of the running wave.
    next_spawn: usize,
    mobs: Vec<WaveMob>,
    /// The fort's structures and their health, percent.
    walls: Vec<(FortWall, f32)>,
    walls_version: u64,
    held_since_ms: Option<u64>,
    ended: Option<Ended>,
    next_tick_ms: u64,
}

impl InvasionRun {
    /// The run of invasion `war` at `fort`, whose claim position is `claim`.
    pub(super) fn new(war: Uuid, fort: InvasionFort, claim: [f32; 3]) -> Self {
        let waves = vec![History::default(); fort.waves.len()];
        let walls = fort
            .walls
            .iter()
            .map(|wall| (*wall, f32::from(wall.health)))
            .collect();
        Self {
            war,
            fort,
            claim,
            entity: None,
            manager: STATUS_NOT_STARTED,
            defense: History::default(),
            waves,
            started: false,
            wave: None,
            next_spawn: 0,
            mobs: Vec::new(),
            walls,
            walls_version: 1,
            held_since_ms: None,
            ended: None,
            next_tick_ms: 0,
        }
    }

    /// The manager and stage members as they are now.
    fn state_members(&self) -> Vec<BundleMember> {
        let encounter = &self.fort.encounter;
        let manager_component = encounter
            .members
            .iter()
            .filter_map(|bytes| BundleMember::read_from(bytes).ok())
            .find(|(member, _)| member.state.type_index() == MANAGER_CLASS)
            .map(|(member, _)| member.component);
        manager_component
            .map(|component| recorded(component, MANAGER_CLASS, manager_body(self.manager)))
            .into_iter()
            .chain(std::iter::once(recorded(
                encounter.defense_component,
                STAGE_CLASS,
                self.defense.body(),
            )))
            .chain(
                self.fort
                    .waves
                    .iter()
                    .zip(&self.waves)
                    .map(|(wave, history)| recorded(wave.component, STAGE_CLASS, history.body())),
            )
            .collect()
    }

    /// The wave's mobs that live (`alive` says which do).
    fn living(&self, wave: usize, alive: impl Fn(u64) -> bool) -> usize {
        self.mobs
            .iter()
            .filter(|mob| mob.wave == wave && alive(mob.entity))
            .count()
    }

    /// The stages at the end: a win sets every stage and the manager back to waiting, a loss
    /// fails the manager, the defense stage and the running wave (module doc).
    fn end_stages(&mut self, won: bool) {
        if won {
            self.cleanup_stages();
            return;
        }
        self.manager = STATUS_FAILED;
        self.defense.set(STATUS_FAILED);
        if let Some((index, _)) = self.wave
            && let Some(history) = self.waves.get_mut(index)
        {
            history.set(STATUS_FAILED);
        }
    }

    /// Every stage and the manager go back to waiting.
    fn cleanup_stages(&mut self) {
        self.manager = STATUS_NOT_STARTED;
        self.defense.set(STATUS_NOT_STARTED);
        for history in &mut self.waves {
            history.set(STATUS_NOT_STARTED);
        }
    }

    /// The 4103 structures child as it is now.
    fn walls_state(&self) -> KeyedList<u64, FortStructure> {
        let walls = self
            .walls
            .iter()
            .map(|(wall, health)| {
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "the health is 0 to 100"
                )]
                let percent = health.round() as u8;
                (
                    wall.key,
                    FortStructure {
                        place: wall.place,
                        kind: wall.kind,
                        health: percent,
                        flag: wall.flag,
                    },
                )
            })
            .collect();
        KeyedList::full(self.walls_version, walls)
    }
}

impl SimWorld {
    /// Where the siege data entity of the invasion fort of `territory` stands: its claim position
    /// (Inferred: a war fort's claim point stands at its territory's claim position, Observed in
    /// the six war forts).
    pub(super) fn invasion_place(&self, territory: u16) -> Option<[f32; 3]> {
        CLAIM_KEYS
            .iter()
            .position(|key| *key == territory)
            .map(|index| CLAIM_POSITIONS[index])
    }

    /// Player `entity` got the raid invite of `war`: an invasion's comes with the pre-raid status
    /// [`PRE_RAID_INVITED`] (module doc).
    pub(super) fn invasion_invited(&mut self, entity: Entity, war: Uuid) {
        if !self.wars.war(war).is_some_and(TerritoryWar::is_invasion) {
            return;
        }
        let Some(number) = self.world.get::<Player>(entity).map(|player| player.number) else {
            return;
        };
        self.send_group_push(
            entity,
            number,
            GroupPush::PreRaidStatus {
                status: PRE_RAID_INVITED,
                raid: *war.as_bytes(),
            },
        );
    }

    /// Carries out the game master's `!invasion` `change` of `territory`; its answer lines (a
    /// job's answer comes when the database took it).
    pub(super) fn invasion_command(
        &mut self,
        territory: u16,
        change: InvasionCommand,
        answer_to: ConnectionRef,
    ) -> Vec<String> {
        let unix_ms = self.unix_ms();
        let job = match change {
            InvasionCommand::Schedule { minutes } => {
                if self.content.invasion_forts.fort(territory).is_none() {
                    return vec![NO_INVASION_FORT.to_owned()];
                }
                InvasionJob::Schedule {
                    territory,
                    starts_ms: unix_ms.saturating_add(minutes.saturating_mul(MS_PER_MINUTE)),
                    answer_to,
                }
            }
            InvasionCommand::Lose => {
                let Some(war) = self
                    .wars
                    .open_war(territory)
                    .filter(|war| war.is_invasion())
                    .map(|war| war.war)
                else {
                    return vec![NO_INVASION.to_owned()];
                };
                let lowered = self.invasion_lowered(territory);
                self.wars.lose_invasion(war, unix_ms, lowered);
                return vec![DONE.to_owned()];
            }
            InvasionCommand::Darkness { level } => InvasionJob::Darkness {
                darkness: TerritoryDarkness {
                    territory,
                    level,
                    cycle_end_ms: unix_ms,
                },
                answer_to,
            },
            InvasionCommand::Upgrade { upgrade, tier } => InvasionJob::Upgrade {
                upgrade: TerritoryUpgrade {
                    territory,
                    upgrade,
                    tier,
                },
                answer_to,
            },
        };
        self.wars.push_invasion_job(job);
        Vec::new()
    }

    /// The upgrades of `territory` its lost invasion lowers (module doc).
    fn invasion_lowered(&mut self, territory: u16) -> Vec<u8> {
        let dice = &mut self.dice;
        self.territories
            .dominion()
            .lowered(territory, || dice.below(LOWER_CHANCE_IN) == 0)
    }

    /// Runs the invasion of the war this instance sim runs (module doc).
    pub(super) fn tick_invasion(&mut self) {
        let Some((war_id, territory)) = self
            .war_fight()
            .map(|fight| (fight.war().war, fight.fort().territory))
        else {
            return;
        };
        let Some(war) = self
            .wars
            .war(war_id)
            .filter(|war| war.is_invasion())
            .cloned()
        else {
            return;
        };
        if self.invasion.as_ref().is_none_or(|run| run.war != war_id) {
            let content = Arc::clone(&self.content);
            let (Some(fort), Some(claim)) = (
                content.invasion_forts.fort(territory),
                self.invasion_place(territory),
            ) else {
                return;
            };
            self.invasion = Some(InvasionRun::new(war_id, fort.clone(), claim));
        }
        let Some(mut run) = self.invasion.take() else {
            return;
        };
        self.run_invasion(&mut run, &war, self.unix_ms());
        self.invasion = Some(run);
    }

    fn run_invasion(&mut self, run: &mut InvasionRun, war: &TerritoryWar, now_ms: u64) {
        if run.entity.is_none() {
            self.spawn_invasion_entity(run, war);
        }
        if let Some(ended) = run.ended {
            if ended.cleanup_ms.is_some_and(|at| now_ms >= at) {
                self.clean_up_invasion(run);
            }
            return;
        }
        if !run.started && now_ms >= war.starts_ms && war.result.is_none() {
            self.start_invasion(run, war.starts_ms);
        }
        if !run.started {
            return;
        }
        if let Some((winner, _)) = war.result {
            self.end_invasion(run, winner == war.defender.0, now_ms);
            return;
        }
        if now_ms >= war.ends_ms() {
            self.end_invasion(run, true, now_ms);
            return;
        }
        if now_ms < run.next_tick_ms {
            return;
        }
        run.next_tick_ms = now_ms.saturating_add(INVASION_TICK_MS);
        self.spawn_due(run, now_ms);
        self.advance_wave(run, war.starts_ms, now_ms);
        self.siege_walls(run);
        if self.claim_lost(run, now_ms) {
            let lowered = self.invasion_lowered(war.territory);
            self.wars.lose_invasion(war.war, now_ms, lowered);
            self.end_invasion(run, false, now_ms);
        }
    }

    /// The encounter entity spawns at the claim position with the defender's ownership.
    fn spawn_invasion_entity(&mut self, run: &mut InvasionRun, war: &TerritoryWar) {
        let encounter = &run.fort.encounter;
        let owner = self.territories.owner(war.territory).cloned();
        let defender_number = war.number.saturating_mul(2).saturating_add(1);
        let members: Vec<BundleMember> = std::iter::once(BundleMember {
            component: encounter.card_component,
            state: MemberState::GdeMetadata(nw_messages::GdeMetadataReplicatedState {
                asset: None,
                gde_ref: Some(encounter.gde_ref),
            }),
        })
        .chain(
            encounter
                .members
                .iter()
                .filter_map(|bytes| BundleMember::read_from(bytes).ok())
                .filter_map(|(member, _)| match &member.state {
                    MemberState::Ownership(_) => owner.as_ref().map(|owner| BundleMember {
                        component: member.component,
                        state: MemberState::Ownership(point_ownership(owner, defender_number)),
                    }),
                    _ => Some(member),
                }),
        )
        .collect();
        let mut members = members;
        merge_members(&mut members, &run.state_members());
        let [x, y, z] = run.claim;
        let gde_ref = encounter.gde_ref;
        run.entity = Some(self.stations.place_camp(|_| Station {
            gde_ref,
            position: Position { x, y, z },
            station_type: String::new(),
            name: Vec::new(),
            members,
        }));
        self.change_invasion_walls(run);
    }

    /// The encounter's states change: its viewers get them.
    fn change_invasion_stages(&mut self, run: &InvasionRun) {
        let Some(number) = run.entity else {
            return;
        };
        let changed = run.state_members();
        self.stations.change_camp(
            number,
            |members| merge_members(members, &changed),
            changed.clone(),
        );
    }

    /// The siege data entity shows the fort's structures as they are now.
    fn change_invasion_walls(&mut self, run: &mut InvasionRun) {
        let Some(siege) = self
            .war_fight()
            .and_then(super::war_fight::WarFight::siege_entity)
        else {
            return;
        };
        run.walls_version += 1;
        let walls = run.walls_state();
        let mut changed = Vec::new();
        self.stations.change_camp(
            siege,
            |members| {
                for member in members.iter_mut() {
                    if let MemberState::SiegeWarfareData(state) = &mut member.state {
                        state.structures = Some(walls.clone());
                        changed.push(member.clone());
                    }
                }
            },
            Vec::new(),
        );
        self.stations.change_camp(siege, |_| {}, changed);
    }

    /// The start: the manager, the defense stage and the first wave run.
    fn start_invasion(&mut self, run: &mut InvasionRun, starts_ms: u64) {
        run.started = true;
        run.manager = STATUS_IN_PROGRESS;
        run.defense.set(STATUS_IN_PROGRESS);
        if let Some(first) = run.waves.first_mut() {
            first.set(STATUS_IN_PROGRESS);
            run.wave = Some((0, starts_ms));
            run.next_spawn = 0;
        }
        self.change_invasion_stages(run);
        let content = Arc::clone(&self.content);
        if let Some(fort) = content.war_forts.fort(run.fort.territory) {
            self.open_stage_areas(DEFENSE_STAGE, |[x, y, _]| fort.frames(x, y));
        }
        tracing::info!(war = %run.war, territory = run.fort.territory, "invasion started");
    }

    /// The running wave's mobs that are due spawn, walking to the claim position; a siege
    /// mob shows its cow launcher slice ([`WaveSpawn::slice`]).
    fn spawn_due(&mut self, run: &mut InvasionRun, now_ms: u64) {
        let Some((index, started_ms)) = run.wave else {
            return;
        };
        let Some(wave) = run.fort.waves.get(index) else {
            return;
        };
        let level = run.fort.mob_level;
        while let Some(spawn) = wave.spawns.get(run.next_spawn) {
            if now_ms < started_ms.saturating_add(spawn.after_ms) {
                break;
            }
            run.next_spawn += 1;
            let [x, y, z] = spawn.place;
            let path = lane_path(spawn, run.claim);
            let ai = script_mob_ai(&path, Position { x, y, z }, self.mobs.ai());
            let vitals = VitalsSpawn {
                vitals_id: spawn.vitals,
                level,
                position: spawn.place,
                ..VitalsSpawn::default()
            };
            match self.mobs.spawn_gm(vitals, Some(ai), spawn.slice()) {
                Some(entity) => run.mobs.push(WaveMob {
                    entity,
                    wave: index,
                    siege: spawn.siege,
                }),
                None => tracing::warn!(vitals = spawn.vitals, level, "invasion mob not spawned"),
            }
        }
    }

    /// Whether mob `entity` lives.
    fn invasion_mob_alive(&self, entity: u64) -> bool {
        self.mobs.health(entity).is_some_and(|health| health > 0)
    }

    /// The next wave starts when it is due and the running one is cleared (module doc).
    fn advance_wave(&mut self, run: &mut InvasionRun, starts_ms: u64, now_ms: u64) {
        let Some((index, _)) = run.wave else {
            return;
        };
        let next = index + 1;
        if next >= run.waves.len() {
            return;
        }
        let all_spawned = run
            .fort
            .waves
            .get(index)
            .is_none_or(|wave| run.next_spawn >= wave.spawns.len());
        let cleared =
            all_spawned && run.living(index, |entity| self.invasion_mob_alive(entity)) == 0;
        if !next_wave_starts(starts_ms, next, now_ms, cleared) {
            return;
        }
        run.waves[index].set(STATUS_SUCCEEDED);
        run.waves[next].set(STATUS_IN_PROGRESS);
        run.wave = Some((next, now_ms));
        run.next_spawn = 0;
        self.change_invasion_stages(run);
        tracing::info!(war = %run.war, wave = next + 1, "invasion wave started");
    }

    /// The living siege mobs hit the structures in reach (module doc).
    fn siege_walls(&mut self, run: &mut InvasionRun) {
        let siege_places: Vec<Position> = run
            .mobs
            .iter()
            .filter(|mob| mob.siege && self.invasion_mob_alive(mob.entity))
            .filter_map(|mob| self.mobs.places(mob.entity).map(|(at, _)| at))
            .collect();
        let mut changed = false;
        let seconds = TICK_SECONDS;
        for (wall, health) in &mut run.walls {
            let [x, y, z] = wall.place;
            let place = Position { x, y, z };
            let near = siege_places
                .iter()
                .filter(|at| at.horizontal_distance(place) <= SIEGE_REACH_M)
                .count();
            if near == 0 || *health <= 0.0 {
                continue;
            }
            let before = health.round();
            *health = sieged_health(*health, near, seconds);
            changed |= (health.round() - before).abs() > f32::EPSILON;
        }
        if changed {
            self.change_invasion_walls(run);
        }
    }

    /// Whether the corrupted held the claim position long enough (module doc).
    fn claim_lost(&mut self, run: &mut InvasionRun, now_ms: u64) -> bool {
        let [x, y, z] = run.claim;
        let claim = Position { x, y, z };
        let corrupted_in = run.mobs.iter().any(|mob| {
            self.invasion_mob_alive(mob.entity)
                && self
                    .mobs
                    .places(mob.entity)
                    .is_some_and(|(at, _)| at.horizontal_distance(claim) <= CAPTURE_RADIUS_M)
        });
        let defenders_in = self.by_connection.values().any(|entity| {
            let alive = self
                .world
                .get::<Vitals>(*entity)
                .is_some_and(|vitals| vitals.dead_since_ms.is_none());
            alive
                && self.world.get::<Body>(*entity).is_some_and(|body| {
                    body.motion.position().horizontal_distance(claim) <= CAPTURE_RADIUS_M
                })
        });
        let (held, lost) = claim_hold(run.held_since_ms, corrupted_in, defenders_in, now_ms);
        run.held_since_ms = held;
        lost
    }

    /// The invasion ended here: the stages, the 715 and the rewards (module doc).
    fn end_invasion(&mut self, run: &mut InvasionRun, won: bool, now_ms: u64) {
        run.end_stages(won);
        self.close_stage_areas(DEFENSE_STAGE);
        run.ended = Some(Ended {
            won,
            cleanup_ms: (!won).then(|| now_ms.saturating_add(CLEANUP_AFTER_LOSS_MS)),
        });
        self.change_invasion_stages(run);
        if won {
            self.remove_invasion_mobs(run);
        }
        let players: Vec<(Entity, u64)> = self
            .by_connection
            .values()
            .filter_map(|entity| Some((*entity, self.world.get::<Player>(*entity)?.number)))
            .collect();
        for (entity, number) in players {
            self.send_group_push(
                entity,
                number,
                GroupPush::PreRaidStatus {
                    status: PRE_RAID_ENDED,
                    raid: *run.war.as_bytes(),
                },
            );
            self.fire_game_event(number, &run.fort.reward_event, now_ms);
            self.fire_game_event(number, BATTLE_TOKEN_EVENT, now_ms);
        }
        tracing::info!(war = %run.war, won, "invasion ended");
    }

    /// The cleanup after a loss: the stages go back to waiting and the mobs go.
    fn clean_up_invasion(&mut self, run: &mut InvasionRun) {
        run.cleanup_stages();
        if let Some(ended) = run.ended.as_mut() {
            ended.cleanup_ms = None;
        }
        self.change_invasion_stages(run);
        self.remove_invasion_mobs(run);
    }

    fn remove_invasion_mobs(&mut self, run: &mut InvasionRun) {
        let entities: Vec<u64> = run.mobs.drain(..).map(|mob| mob.entity).collect();
        self.remove_gm(&entities);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STARTS_MS: u64 = 1_781_463_600_000;
    /// The full health of a structure, percent.
    const FULL_HEALTH: f32 = 100.0;
    const TERRITORY: u16 = 16;

    fn dominion() -> Dominion {
        Dominion::of(
            vec![TerritoryDarkness {
                territory: TERRITORY,
                level: 3,
                cycle_end_ms: 1_793_833_265_503,
            }],
            vec![
                TerritoryUpgrade {
                    territory: TERRITORY,
                    upgrade: 12,
                    tier: 2,
                },
                TerritoryUpgrade {
                    territory: TERRITORY,
                    upgrade: 0,
                    tier: 3,
                },
                TerritoryUpgrade {
                    territory: TERRITORY,
                    upgrade: 8,
                    tier: 0,
                },
            ],
        )
    }

    #[test]
    fn the_invasion_attacker_side_is_empty() {
        let side = empty_side();
        assert_eq!(
            (side.number, side.groups.len(), side.key),
            (0, 0, NO_SIDE_KEY)
        );
        assert_eq!(side.raid, [0; nw_messages::RAW_ID_LEN]);
    }

    #[test]
    fn the_claims_show_the_darkness_since_2000_and_the_upgrade_tiers_with_their_sum() {
        let dominion = dominion();
        assert_eq!(dominion.level(TERRITORY), 3);
        // Observed value of `Playthrough_Part_21` for Unix 1793833265.503 s.
        assert_eq!(dominion.cycle_end_ns(TERRITORY), 847_148_465_503_000_000);
        assert_eq!((dominion.level(5), dominion.cycle_end_ns(5)), (0, 0));
        let progression = dominion.progression(TERRITORY);
        assert_eq!(progression.tiers.len(), UPGRADE_IDS.len());
        assert_eq!(progression.tiers[0].id, 11);
        assert_eq!(progression.level, 5);
        assert_eq!(
            progression.entries.len(),
            5,
            "one town project entry per progression type"
        );
    }

    #[test]
    fn a_loss_lowers_only_raised_upgrades_the_roll_picks() {
        let dominion = dominion();
        assert_eq!(dominion.lowered(TERRITORY, || true), vec![12, 0]);
        assert!(dominion.lowered(TERRITORY, || false).is_empty());
        let mut rolls = [false, true].into_iter();
        assert_eq!(
            dominion.lowered(TERRITORY, || rolls.next().unwrap_or(false)),
            vec![0]
        );
    }

    #[test]
    fn the_commands_read() {
        assert_eq!(
            InvasionCommand::read(&["10"]),
            Some(InvasionCommand::Schedule { minutes: 10 })
        );
        assert_eq!(
            InvasionCommand::read(&["LOSE"]),
            Some(InvasionCommand::Lose)
        );
        assert_eq!(
            InvasionCommand::read(&["darkness", "2"]),
            Some(InvasionCommand::Darkness { level: 2 })
        );
        assert_eq!(InvasionCommand::read(&["darkness", "4"]), None);
        assert_eq!(
            InvasionCommand::read(&["upgrade", "12", "3"]),
            Some(InvasionCommand::Upgrade {
                upgrade: 12,
                tier: 3
            })
        );
        assert_eq!(
            InvasionCommand::read(&["upgrade", "10", "1"]),
            None,
            "no upgrade 10"
        );
        assert_eq!(InvasionCommand::read(&["upgrade", "12", "4"]), None);
        assert_eq!(InvasionCommand::read(&[]), None);
    }

    #[test]
    fn a_wave_starts_when_due_and_the_wave_before_is_cleared() {
        assert_eq!(wave_due_ms(STARTS_MS, 1), Some(STARTS_MS + 170_000));
        assert_eq!(wave_due_ms(STARTS_MS, WAVE_SCHEDULE_MS.len()), None);
        assert!(!next_wave_starts(STARTS_MS, 1, STARTS_MS + 169_999, true));
        assert!(next_wave_starts(STARTS_MS, 1, STARTS_MS + 170_000, true));
        // Observed, territory 16: wave 4 due at +510 s waited for wave 3's last mob (+535.8 s).
        assert!(!next_wave_starts(STARTS_MS, 3, STARTS_MS + 520_000, false));
        assert!(next_wave_starts(STARTS_MS, 3, STARTS_MS + 535_800, true));
    }

    #[test]
    fn siege_mobs_wear_a_structure_down_to_nothing() {
        assert!((sieged_health(FULL_HEALTH, 2, 1.0) - 98.0).abs() < f32::EPSILON);
        assert!((sieged_health(FULL_HEALTH, 0, 1.0) - FULL_HEALTH).abs() < f32::EPSILON);
        assert!(sieged_health(1.0, 3, 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_mob_walks_its_lane_then_on_to_the_claim() {
        let claim = [100.0, 0.0, 10.0];
        let spawn = WaveSpawn {
            after_ms: 0,
            vitals: 1,
            place: [0.0, 0.0, 10.0],
            siege: false,
            path: vec![[10.0, 0.0, 10.0], [20.0, 5.0, 11.0]],
        };
        let path = lane_path(&spawn, claim);
        assert_eq!(
            path.points,
            vec![[10.0, 0.0, 0.0], [20.0, 5.0, 1.0], [100.0, 0.0, 0.0]],
            "the lane as offsets from the place, then the claim"
        );
        assert_eq!(path.mode, PathMode::Once);
        let ending = WaveSpawn {
            path: vec![[95.0, 1.0, 10.0]],
            ..spawn.clone()
        };
        assert_eq!(
            lane_path(&ending, claim).points,
            vec![[95.0, 1.0, 0.0]],
            "a lane that ends at the claim"
        );
        let no_lane = WaveSpawn {
            path: Vec::new(),
            ..spawn
        };
        assert_eq!(
            lane_path(&no_lane, claim).points,
            vec![[100.0, 0.0, 0.0]],
            "no lane: straight to the claim"
        );
    }

    #[test]
    fn the_claim_is_lost_when_the_corrupted_hold_it_alone_long_enough() {
        let now = STARTS_MS;
        assert_eq!(claim_hold(None, true, false, now), (Some(now), false));
        assert_eq!(
            claim_hold(Some(now), true, false, now + CLAIM_HOLD_MS - 1),
            (Some(now), false)
        );
        assert_eq!(
            claim_hold(Some(now), true, false, now + CLAIM_HOLD_MS),
            (Some(now), true)
        );
        assert_eq!(
            claim_hold(Some(now), true, true, now + CLAIM_HOLD_MS),
            (None, false)
        );
        assert_eq!(
            claim_hold(Some(now), false, false, now + CLAIM_HOLD_MS),
            (None, false)
        );
    }

    fn fort() -> InvasionFort {
        let table = r#"{"forts": [{
            "territory": 16,
            "encounter": {"gde_ref": "1b36e3993bd273d89a2e1f6aa2b56ebd", "card_component": 0,
                          "defense_component": 7, "members": []},
            "structures": [{"key": 7, "place": [5805.5, 7387.5, 71.1], "kind": 4, "health": 100, "flag": 5}],
            "mob_level": 63, "reward_event": "Invasion_T4",
            "waves": [
              {"component": 6, "start_after_ms": 0, "spawns": []},
              {"component": 8, "start_after_ms": 170000, "spawns": []}]}]}"#;
        nw_content::invasion_forts::InvasionForts::parse(table)
            .expect("table")
            .fort(TERRITORY)
            .expect("fort")
            .clone()
    }

    fn statuses(run: &InvasionRun) -> (u32, u32, Vec<u32>) {
        (
            run.manager,
            run.defense.status(),
            run.waves.iter().map(History::status).collect(),
        )
    }

    #[test]
    fn a_win_sets_every_stage_back_and_a_loss_fails_the_defense_and_the_running_wave() {
        let mut run = InvasionRun::new(Uuid::from_u128(1), fort(), [0.0; 3]);
        assert_eq!(
            statuses(&run),
            (STATUS_NOT_STARTED, STATUS_NOT_STARTED, vec![1, 1])
        );
        run.manager = STATUS_IN_PROGRESS;
        run.defense.set(STATUS_IN_PROGRESS);
        run.waves[0].set(STATUS_SUCCEEDED);
        run.waves[1].set(STATUS_IN_PROGRESS);
        run.wave = Some((1, STARTS_MS));
        let mut lost = run.clone();
        lost.end_stages(false);
        assert_eq!(
            statuses(&lost),
            (
                STATUS_FAILED,
                STATUS_FAILED,
                vec![STATUS_SUCCEEDED, STATUS_FAILED]
            )
        );
        lost.cleanup_stages();
        assert_eq!(
            statuses(&lost),
            (STATUS_NOT_STARTED, STATUS_NOT_STARTED, vec![1, 1])
        );
        run.end_stages(true);
        assert_eq!(
            statuses(&run),
            (STATUS_NOT_STARTED, STATUS_NOT_STARTED, vec![1, 1])
        );
    }

    #[test]
    fn the_stage_members_name_the_defense_and_each_wave_and_the_walls_keep_their_keys() {
        let mut run = InvasionRun::new(Uuid::from_u128(1), fort(), [0.0; 3]);
        let components: Vec<u64> = run
            .state_members()
            .iter()
            .map(|member| member.component)
            .collect();
        assert_eq!(
            components,
            vec![7, 6, 8],
            "no manager member in the table: none sent"
        );
        run.walls[0].1 = 86.4;
        let KeyedList::Full(list) = run.walls_state() else {
            panic!("a full list");
        };
        assert_eq!(list.elements[0].0, 7);
        assert_eq!(list.elements[0].1.health, 86);
    }
}
