// SPDX-License-Identifier: AGPL-3.0-or-later
//! The slot-2 world entity every client gets once it spawned: its spawn card, its position and
//! the territory claims ([`nw_messages::land_claim`]), with the company that owns each claim.
//!
//! Observed (client trace 2026-11-06): without the claims the map's shrine and town flyout finds
//! no governance data, stops with a script error and shows no name. Observed (retail ledgers
//! newchar 2026-09-18, general-other-01, travel-shrines-settlements-mount): the first slot-2
//! group is a spawn card: the mutations (component 6), the position (4), the card (0), the claims
//! (2), the global map data (1) and the war data (5), with the same asset, GDE ref and position in
//! all three ([`WORLD_ENTITY_ASSET`]); members sent to slot 2 without the card reach no entity
//! (live 2026-11-06: the claims alone changed nothing). The retail state has the 13
//! claims, their positions and one tax rate set for all ([`CLAIM_KEYS`], [`CLAIM_POSITIONS`],
//! [`TERRITORY_GOVERNANCE`]). The global map data goes with them as an empty list of map points
//! ([`NO_MAP_POINTS`]); the war data (component 5) follows them ([`super::wars`]).
//!
//! **Owners** (territory ownership study 2026-11-11, every retail 3086 body of the capture
//! index): companies own every retail claim; each owner entry is the company's id, name, crest
//! and faction and the time since it owns the claim ([`ClaimOwner`]). Here the owners live in the
//! world database (world migration 0038, [`nw_db::territory_owners`]): every sim reads them at
//! start and again every [`OWNER_READ_MS`], so a change on one sim reaches the clients of every
//! sim. A client whose claims are older than the sim's gets the whole state again, alone in a
//! slot-2 group (Observed: retail re-sends the whole state alone in a slot-2 group, for example
//! `pay-territory-tax` 1783335287227 after the upkeep payment). How a company gets a claim in
//! retail is **Unknown**: no capture holds a claim of a free territory (no fort, governor's desk
//! or treasury call of one). So only a game master sets and clears owners (`!owner`,
//! [`super::gm_chat`]). The darkness and the upgrade tiers of each claim come from the world
//! database with the wars ([`super::invasions`]); the town projects stay at the zero of each
//! value (Inferred: a claim with none; the town project flow is in no capture).
//!
//! **Taxes** (Observed, `sharedassets/genericassets/playerbaseattributes.pbadb`, fields keyed by
//! the CRC-32 of their lowercase names, client list `FUN_145cae240` +0xab4 to +0xad0): the
//! lowest and highest rate a governor may set are the same, `PropertyTaxRateMin/Max` 0.2,
//! `TradingTaxRateMin/Max` 1.0, `CraftingFeeRateMin/Max` 0.5, `RefiningFeeRateMin/Max` 0.5,
//! the values of every retail claim. So no owner changes them in this build, and the rates are
//! [`TERRITORY_GOVERNANCE`] on every claim, owned or not. A station's fee is the recipe's
//! `CraftingFee` times the crafts times the crafting or refining fee ([`station_fee`]). Which
//! share of a tax reaches the owner's treasury is **Unknown** (the same file has
//! `ControllingCompanyTaxModifier` 0.3, meaning Unknown; no capture shows a treasury rise from a
//! tax), so no tax goes to a treasury.
//!
//! **Upkeep** (territory upkeep study 2026-11-11, [`super::governance`]): each owner's upkeep due
//! time lives in the world database with the owner (world migration 0039,
//! [`ClaimUpkeep`]); the governor's desk shows it in its governance data ([`Territories::governance`]).
//! A payment moves it by `nw_db::UPKEEP_INTERVAL_MS`. What retail does when an upkeep is due and
//! not paid is **Unknown**: the claims' `failedToPayUpkeep` flag is seen to change
//! (`S8_ledger` 1782965690981 to failed, `Playthrough_Part_20` 1793559790536 back), but no capture
//! shows it at a known due time, nor what a failed claim loses. So the flag stays clear, and no
//! claim is ever taken for a missed upkeep.
//!
//! **Influence and conflict** (wars and sieges study 2026-11-12, every retail 3086 of the capture
//! index; [`nw_db::territory_conflicts`]): each claim shows each faction's influence percent, the
//! race start, the conflict faction and start and the lottery end, read from the world database
//! with the owners; times in Unix nanoseconds as retail sends them. The PvP mission gains of this
//! sim's players go to the database with the next read ([`super::influence`]); the social
//! service moves the races, conflicts and lotteries on. `replicatedWarDecThresholdMetFaction`
//! stays at no faction: retail sets it at 7 % to 54 % with no rule the captures show
//! (**Unknown**).

use std::collections::BTreeMap;

use nw_bus::ConnectionRef;
use nw_content::crafting::RecipeDetails;
use nw_db::{
    DbError, PgPool, TerritoryConflict, TerritoryOwner, TerritoryWar, UPKEEP_INTERVAL_MS,
    add_influence, clear_territory_owner, set_territory_owner, territory_conflicts,
    territory_owners, territory_wars,
};
use nw_messages::gde::AssetId;
use nw_messages::items::FullList;
use nw_messages::position::{PositionInTheWorldReplicatedState, WorldPosition};
use nw_messages::social::crest_of_bytes;
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    BundleGroup, BundleMember, CLAIM_KEYS, ClaimOwner, EarningsInterval,
    GdeMetadataReplicatedState, GovernanceTaxData, LandClaimManagerState, MemberState,
    MessageError, ReplicatedList, TAX_TYPES, TaxChange, TaxEarnings, TerritoryGovernanceData,
};
use uuid::Uuid;

use super::SimWorld;
use super::crafting::CARD_COMPONENT;
use super::gm_chat::DONE;
use super::wars::{WarAnswer, WarJob};

/// The slice of the slot-2 world entity (Observed: the same in the three retail ledgers).
const WORLD_ENTITY_ASSET: AssetId = AssetId {
    id: [
        241, 133, 95, 148, 136, 148, 85, 215, 166, 176, 208, 3, 236, 39, 129, 38,
    ],
    sub_id: 2,
};
/// Its GDE ref (Observed: the same in the three retail ledgers).
const WORLD_ENTITY_GDE_REF: [u8; RAW_ID_LEN] = [
    76, 64, 180, 139, 78, 222, 12, 4, 15, 243, 158, 244, 233, 34, 253, 253,
];
/// Its position, no rotation or scale (Observed: the same in the three retail ledgers).
const WORLD_ENTITY_POSITION: WorldPosition = WorldPosition {
    x: 7168.0,
    y: 7168.0,
    z_raw: 4119,
};
/// The component of its position (Observed).
const POSITION_COMPONENT: u64 = 4;
/// The component and the class of its global map data
/// (`Javelin::GlobalMapDataManagerComponentReplicatedState`; Observed: component 1 of the same
/// group in 1,171 retail ledgers).
const MAP_DATA_COMPONENT: u64 = 1;
const MAP_DATA_TYPE_INDEX: u64 = 3780;
/// The global map data of a world with no map points: slot 0, child 0, the full form, the
/// first version, no entries. Observed (player state study 2026-11-11): the state is a list of
/// live world map points (corruption breaches and world events: a key, a place, a type, a
/// number and a flag); it holds no territory owner. Inferred: the empty list (no retail ledger
/// has one: retail always runs breaches); this server runs none yet.
const NO_MAP_POINTS: [u8; 6] = [0x01, 0x01, 0x00, 0x01, 0x03, 0x00];

/// The position of each claim, by claim index (Observed: the same in the three retail ledgers).
pub(super) const CLAIM_POSITIONS: [[f32; 3]; CLAIM_KEYS.len()] = [
    [6851.21, 1330.6776, 62.924_44],
    [6305.3696, 3352.3557, 53.653_294],
    [8928.186, 3289.11, 70.321_95],
    [10986.032, 2357.098, 46.180_897],
    [4344.5234, 4873.731, 120.192_15],
    [5902.4795, 5428.118, 57.516_47],
    [8913.23, 5783.515, 168.272_63],
    [8461.229, 4877.2725, 180.431_1],
    [11017.301, 5906.705, 146.002_23],
    [14046.733, 4820.699, 57.624_416],
    [5805.467, 7387.4556, 71.078],
    [13694.403, 6314.9653, 45.882_137],
    [11507.5625, 8964.927, 418.562_65],
];
/// The taxes and fees of every claim: the only rates of `playerbaseattributes.pbadb` (each
/// lowest rate equals its highest), and the governance of all 13 claims in every retail
/// ledger. The missed-upkeep flag stays clear: what a missed upkeep does is Unknown (module
/// doc).
pub(super) const TERRITORY_GOVERNANCE: GovernanceTaxData = GovernanceTaxData {
    house_fee: 0.2,
    crafting_fee: 0.5,
    refining_fee: 0.5,
    trading_fee: 1.0,
    is_valid: true,
    failed_to_pay_upkeep: false,
};
/// The least fee of a craft whose recipe has a fee (Observed, 2997 of
/// `refining_all_stations_single_batch_territorylvlup`: one craft of a 1-coin recipe at the 0.5
/// fee showed 1 coin, also for a player whose standing discount took the fee below 0.5).
const LEAST_STATION_FEE: u32 = 1;
/// How often a sim reads the owners again (Inferred: retail re-sends the claims about every
/// 30 s, `nw_messages::land_claim`).
pub(super) const OWNER_READ_MS: u64 = 30_000;
/// A fort lock timer that runs no lock (Observed: on every claim without a lock).
const NO_FCP_LOCK: u16 = u16::MAX;
/// Entries of `replicatedFactionControlPointData` (Observed: 29 in the three retail ledgers).
const FACTION_CONTROL_POINT_ENTRIES: usize = 29;
/// The faction byte of no faction.
const NO_FACTION: u8 = 0;
/// The zero time of a conflict, darkness cycle or influence race that does not run.
const NO_TIME: u64 = 0;
/// The influence percent of a claim without a race.
const NO_INFLUENCE: u8 = 0;

/// Unix ns of Unix ms `ms` (the 3086 times are nanoseconds, Observed).
const fn ns_of(ms: u64) -> u64 {
    ms.saturating_mul(NS_PER_MS)
}

/// Nanoseconds per millisecond.
const NS_PER_MS: u64 = 1_000_000;
/// The level of every claim of this server: no town project runs, so no claim has a level
/// (Inferred; the governance data's level is the claim's level, Observed in three desks).
pub(super) const CLAIM_LEVEL: u32 = 0;
/// The earnings of an interval: no tax or fee reaches an owner's treasury here (its share is
/// Unknown, module doc).
const NO_EARNINGS: u64 = 0;
/// The game master's answers of `!owner`.
pub(super) const NO_SUCH_CLAIM: &str = "No territory claim has that id.";
const NO_SUCH_COMPANY: &str = "No company has that name.";
const NO_OWNER: &str = "That territory has no owner.";
const NOT_STORED: &str = "The database did not take the change.";

/// The coins `crafts` crafts of a recipe of `details` cost at a station: its `CraftingFee` times
/// the crafts times the territory's refining fee (a refining recipe) or crafting fee (every
/// other one), rounded down, and at least [`LEAST_STATION_FEE`] when the recipe has a fee.
///
/// Observed (every retail 2997 of the capture index with a recipe fee, 211 crafts): the fee the
/// client showed was the recipe fee x crafts x 0.5 for crafting and refining recipes alike, or
/// less by a share that is the same within one capture (up to 17 %: the player's territory
/// standing discount, whose size per standing card is in no game table, so it is **Unknown**
/// and not taken off here). Odd totals went down, 12 of 12 (35 crafts of a 1-coin recipe showed
/// 17 in `20261008-114334-208` at 1791452808021, 75 showed 37 in `Playthrough_Part_07` at
/// 1793194380608).
pub(super) fn station_fee(details: &RecipeDetails, crafts: u32) -> u32 {
    let base = u64::from(details.fee) * u64::from(crafts);
    if base == 0 {
        return 0;
    }
    let rate = if details.refining {
        TERRITORY_GOVERNANCE.refining_fee
    } else {
        TERRITORY_GOVERNANCE.crafting_fee
    };
    // A fee and a count of u32 times a rate below 1 fit a u32.
    let fee = (base as f64 * f64::from(rate)).floor() as u32;
    fee.max(LEAST_STATION_FEE)
}

/// One value per claim.
fn per_claim<T: Clone>(value: T) -> Option<ReplicatedList<T>> {
    all_claims(vec![value; CLAIM_KEYS.len()])
}

/// A full list of the claims' `values`, by claim index.
fn all_claims<T>(values: Vec<T>) -> Option<ReplicatedList<T>> {
    Some(ReplicatedList::Full(FullList::first(values)))
}

/// The claim entry of `owner`: the company id is its UUID bytes, as the company states of the
/// social service send it.
fn claim_owner(owner: &TerritoryOwner) -> ClaimOwner {
    ClaimOwner {
        guild_id: *owner.company.as_bytes(),
        guild_name: owner.name.clone().into_bytes(),
        crest: crest_of_bytes(&owner.crest),
        text: Vec::new(),
        faction: owner.faction,
        time_ns: owner.since_ms.saturating_mul(NS_PER_MS),
    }
}

/// A change of a claim's owner that a game master asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OwnerChange {
    /// The company of this name owns it from now on.
    Set(String),
    /// No company owns it.
    Clear,
}

/// A change a game master asked for: the territory, the change and the client to answer.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AskedChange {
    territory: u16,
    change: OwnerChange,
    answer_to: ConnectionRef,
}

/// Influence a player's faction gained in a territory ([`super::influence`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct InfluenceGain {
    /// The territory id.
    pub(super) territory: u16,
    /// The player's faction.
    pub(super) faction: u8,
    /// The player.
    pub(super) character: Uuid,
    /// The influence points.
    pub(super) points: u64,
    /// When, Unix ms.
    pub(super) at_ms: u64,
}

/// A database job of the territory owners: a change first, when one was asked, then a read of
/// every owner. It also carries the war jobs ([`super::wars`]), run before the owner change (a
/// war's result may give its claim to the attacker), and then reads the wars; and the influence
/// gains, stored before the conflicts are read with the wars.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TerritoryJob {
    change: Option<AskedChange>,
    /// The war jobs and the Unix ms of the wars' read; `None` reads no war and no conflict.
    wars: Option<(Vec<WarJob>, u64)>,
    /// The influence gains to store.
    gains: Vec<InfluenceGain>,
    /// The darkness and the upgrades read with the wars ([`super::invasions`]).
    dominion: Option<super::invasions::Dominion>,
}

impl TerritoryJob {
    /// Runs the job against the world database.
    pub(crate) async fn run(mut self, world: &PgPool) -> TerritoryDone {
        for gain in std::mem::take(&mut self.gains) {
            let InfluenceGain {
                territory,
                faction,
                character,
                points,
                at_ms,
            } = gain;
            if let Err(error) =
                add_influence(world, territory, faction, character, points, at_ms).await
            {
                tracing::error!(%error, territory, points, "influence gain not stored; lost");
            }
        }
        let (wars, conflicts) = match self.wars.take() {
            None => (None, None),
            Some((jobs, now_ms)) => {
                let mut answers = Vec::with_capacity(jobs.len());
                for job in jobs {
                    answers.push(job.run(world).await);
                }
                self.dominion = super::invasions::Dominion::read(world).await;
                (
                    Some((answers, territory_wars(world, now_ms).await)),
                    Some(territory_conflicts(world, now_ms).await),
                )
            }
        };
        let written = match &self.change {
            None => None,
            Some(asked) => Some(match &asked.change {
                OwnerChange::Set(company) => {
                    set_territory_owner(world, asked.territory, company).await
                }
                OwnerChange::Clear => clear_territory_owner(world, asked.territory).await,
            }),
        };
        TerritoryDone {
            owners: territory_owners(world).await,
            job: self,
            written,
            wars,
            conflicts,
        }
    }
}

/// What a [`TerritoryJob`] found: whether its change was made (false: no such company, or no
/// owner to clear) and every owner after it.
#[derive(Debug)]
pub(crate) struct TerritoryDone {
    job: TerritoryJob,
    written: Option<Result<bool, DbError>>,
    owners: Result<Vec<TerritoryOwner>, DbError>,
    /// The war jobs' answers and the wars read after them, when the job carried wars.
    wars: Option<WarsDone>,
    /// The races and conflicts, read with the wars.
    conflicts: Option<Result<Vec<TerritoryConflict>, DbError>>,
}

/// The answers of a job's war jobs and the wars read after them.
type WarsDone = (Vec<WarAnswer>, Result<Vec<TerritoryWar>, DbError>);

/// The upkeep of an owned claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ClaimUpkeep {
    /// The owner company.
    pub(super) company: Uuid,
    /// Since when it owns the claim, Unix ms.
    pub(super) since_ms: u64,
    /// When its upkeep is due, Unix ms.
    pub(super) due_ms: u64,
    /// From when the next payment is taken, Unix ms.
    pub(super) payable_ms: u64,
}

impl ClaimUpkeep {
    fn of(owner: &TerritoryOwner) -> Self {
        Self {
            company: owner.company,
            since_ms: owner.since_ms,
            due_ms: owner.upkeep_due_ms,
            payable_ms: owner.upkeep_payable_ms(),
        }
    }

    /// The end of the earnings interval that runs at Unix ms `now_ms`: the intervals are
    /// `UPKEEP_INTERVAL_MS` long from the time the company took the claim (Inferred: the ends of
    /// the retail intervals were 604,800 s apart).
    fn interval_end_ms(&self, now_ms: u64) -> u64 {
        let done = now_ms.saturating_sub(self.since_ms) / UPKEEP_INTERVAL_MS;
        self.since_ms
            .saturating_add(done.saturating_add(1).saturating_mul(UPKEEP_INTERVAL_MS))
    }

    /// The governance data of the claim at Unix ms `now_ms` ([`nw_messages::territory_governance`]):
    /// the due and payable times; the times the rates were set are the time the company took the
    /// claim (Inferred: no owner sets a rate in this build); the current interval earned nothing
    /// in each tax or fee type, and no earlier interval is listed (no tax reaches a treasury
    /// here); the claim's level.
    fn governance(&self, now_ms: u64) -> TerritoryGovernanceData {
        let ns = |ms: u64| ms.saturating_mul(NS_PER_MS);
        TerritoryGovernanceData {
            upkeep_due_ns: ns(self.due_ms),
            second_time_ns: NO_TIME,
            upkeep_payable_ns: ns(self.payable_ms),
            tax_changes: TAX_TYPES
                .iter()
                .map(|tax| TaxChange {
                    tax: *tax,
                    changed_ns: ns(self.since_ms),
                })
                .collect(),
            current: EarningsInterval {
                total: NO_EARNINGS,
                taxes: TAX_TYPES.iter().copied().map(TaxEarnings::none).collect(),
                end_ns: ns(self.interval_end_ms(now_ms)),
            },
            history: Vec::new(),
            level: CLAIM_LEVEL,
        }
    }
}

/// The claim owners as this sim last read them, and the jobs that read and change them.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Territories {
    /// The owner of each owned claim, by territory id.
    owners: BTreeMap<u16, ClaimOwner>,
    /// The upkeep of each owned claim, by territory id.
    upkeep: BTreeMap<u16, ClaimUpkeep>,
    /// Rises with each change of the owners: a client with an older one gets the claims again.
    version: u64,
    /// Rises with each change of an owner's upkeep or of the owners: a desk shown with an older
    /// one gets its governance data again ([`super::governance`]).
    upkeep_version: u64,
    /// When the next read is due, sim ms.
    next_read_ms: u64,
    /// The jobs that wait for the loop.
    jobs: Vec<TerritoryJob>,
    /// The jobs the loop runs now: no read is added while one runs.
    running: usize,
    /// The race and conflict of each territory with one, as last read.
    conflicts: BTreeMap<u16, TerritoryConflict>,
    /// The influence gains that wait for the next read job.
    gains: Vec<InfluenceGain>,
    /// The darkness and the upgrades, as last read ([`super::invasions`]).
    dominion: super::invasions::Dominion,
}

impl Territories {
    /// The manager state of the claims.
    fn state(&self) -> LandClaimManagerState {
        LandClaimManagerState {
            fcp_lock_timers: per_claim(NO_FCP_LOCK),
            // Observed: one retail ledger leaves the anchor out.
            fcp_lock_anchor: None,
            claim_keys: all_claims(CLAIM_KEYS.to_vec()),
            conflict_faction: self.per_conflict(|conflict| conflict.conflict_faction, NO_FACTION),
            conflict_lottery_end_time: self
                .per_conflict(|conflict| ns_of(conflict.lottery_end_ms), NO_TIME),
            conflict_start_time: self
                .per_conflict(|conflict| ns_of(conflict.conflict_start_ms), NO_TIME),
            darkness_threshold: all_claims(
                CLAIM_KEYS
                    .iter()
                    .map(|key| self.dominion.level(*key))
                    .collect(),
            ),
            darkness_cycle_end_time: all_claims(
                CLAIM_KEYS
                    .iter()
                    .map(|key| self.dominion.cycle_end_ns(*key))
                    .collect(),
            ),
            faction_control_point_data: all_claims(vec![NO_FACTION; FACTION_CONTROL_POINT_ENTRIES]),
            faction_influence: [0, 1, 2].map(|faction| {
                self.per_conflict(|conflict| conflict.influence[faction], NO_INFLUENCE)
            }),
            governance: per_claim(TERRITORY_GOVERNANCE),
            influence_race_start_time: self
                .per_conflict(|conflict| ns_of(conflict.race_start_ms), NO_TIME),
            positions: all_claims(CLAIM_POSITIONS.to_vec()),
            progression: all_claims(
                CLAIM_KEYS
                    .iter()
                    .map(|key| self.dominion.progression(*key))
                    .collect(),
            ),
            owners: all_claims(
                CLAIM_KEYS
                    .iter()
                    .map(|key| self.owners.get(key).cloned().unwrap_or(ClaimOwner::NONE))
                    .collect(),
            ),
            war_dec_threshold_met_faction: per_claim(NO_FACTION),
        }
    }

    /// One value per claim: `value` of the claim's race, `none` for a claim without one.
    fn per_conflict<T>(
        &self,
        value: impl Fn(&TerritoryConflict) -> T,
        none: T,
    ) -> Option<ReplicatedList<T>>
    where
        T: Clone,
    {
        all_claims(
            CLAIM_KEYS
                .iter()
                .map(|key| self.conflicts.get(key).map_or(none.clone(), &value))
                .collect(),
        )
    }

    /// Keeps the influence gain `gain` for the next read job.
    pub(super) fn gain(&mut self, gain: InfluenceGain) {
        self.gains.push(gain);
    }

    /// The slot-2 members a client with the claims version `sent` needs, in retail order (after
    /// the mutations): for a client without the entity (`None`) the position, the card, the
    /// claims and the global map data; for one with older claims the claims alone. Sets `sent`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the claims cannot be encoded.
    pub(super) fn members_due(
        &self,
        sent: &mut Option<u64>,
    ) -> Result<Option<Vec<BundleMember>>, MessageError> {
        match sent.replace(self.version) {
            None => Ok(Some(vec![
                BundleMember {
                    component: POSITION_COMPONENT,
                    state: MemberState::Position(PositionInTheWorldReplicatedState {
                        position: Some(WORLD_ENTITY_POSITION),
                        rotation: None,
                        scale: None,
                    }),
                },
                BundleMember {
                    component: CARD_COMPONENT,
                    state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                        asset: Some(WORLD_ENTITY_ASSET),
                        gde_ref: Some(WORLD_ENTITY_GDE_REF),
                    }),
                },
                self.state().member()?,
                BundleMember {
                    component: MAP_DATA_COMPONENT,
                    state: MemberState::Recorded {
                        class: MAP_DATA_TYPE_INDEX,
                        body: NO_MAP_POINTS.to_vec(),
                    },
                },
            ])),
            Some(version) if version != self.version => Ok(Some(vec![self.state().member()?])),
            Some(_) => Ok(None),
        }
    }

    /// Asks for `change` of the claim of `territory` for the game master of `answer_to`; the
    /// refusal line when no claim has that territory id.
    pub(super) fn ask(
        &mut self,
        territory: u16,
        change: OwnerChange,
        answer_to: ConnectionRef,
    ) -> Result<(), &'static str> {
        if !CLAIM_KEYS.contains(&territory) {
            return Err(NO_SUCH_CLAIM);
        }
        self.jobs.push(TerritoryJob {
            change: Some(AskedChange {
                territory,
                change,
                answer_to,
            }),
            wars: None,
            gains: Vec::new(),
            dominion: None,
        });
        Ok(())
    }

    /// The darkness and the upgrades as last read ([`super::invasions`]).
    pub(super) const fn dominion(&self) -> &super::invasions::Dominion {
        &self.dominion
    }

    /// The owner of the claim of `territory`; `None` when no company owns it.
    pub(super) fn owner(&self, territory: u16) -> Option<&ClaimOwner> {
        self.owners.get(&territory)
    }

    /// The claim owner entry of company `company` on one of the claims it owns; `None` for a
    /// company that owns none.
    pub(super) fn company(
        &self,
        company: [u8; nw_messages::buildables::GUILD_ID_LEN],
    ) -> Option<&ClaimOwner> {
        self.owners.values().find(|owner| owner.guild_id == company)
    }

    /// The upkeep of the claim of `territory`; `None` when no company owns it.
    pub(super) fn upkeep(&self, territory: u16) -> Option<ClaimUpkeep> {
        self.upkeep.get(&territory).copied()
    }

    /// The governance data of the claim of `territory` at Unix ms `now_ms`; `None` when no company
    /// owns it (Inferred: every retail claim had an owner and governance data).
    pub(super) fn governance(
        &self,
        territory: u16,
        now_ms: u64,
    ) -> Option<TerritoryGovernanceData> {
        self.upkeep
            .get(&territory)
            .map(|upkeep| upkeep.governance(now_ms))
    }

    /// Rises with each change of an owner or an upkeep.
    pub(super) const fn upkeep_version(&self) -> u64 {
        self.upkeep_version
    }

    /// Makes the next read due at once (an upkeep was paid on another service).
    pub(super) fn read_soon(&mut self) {
        self.next_read_ms = 0;
    }

    /// The jobs to run at sim ms `now_ms`: the asked changes, and a read when one is due and none
    /// runs or war jobs wait.
    /// The war jobs `wars` go with a read job, which reads the wars at Unix ms `unix_ms`; a read
    /// due without war jobs reads them as well.
    pub(super) fn take_jobs(
        &mut self,
        now_ms: u64,
        wars: Vec<WarJob>,
        unix_ms: u64,
    ) -> Vec<TerritoryJob> {
        let read_due = now_ms >= self.next_read_ms && self.running == 0 && self.jobs.is_empty();
        if read_due || !wars.is_empty() || !self.gains.is_empty() {
            self.next_read_ms = now_ms.saturating_add(OWNER_READ_MS);
            self.jobs.push(TerritoryJob {
                change: None,
                wars: Some((wars, unix_ms)),
                gains: std::mem::take(&mut self.gains),
                dominion: None,
            });
        }
        self.running += self.jobs.len();
        std::mem::take(&mut self.jobs)
    }

    /// A job whose task ended without a result: the next read may start.
    fn lost(&mut self) {
        self.running = self.running.saturating_sub(1);
    }

    /// Takes the owners a job read (a failed read keeps the last ones); the game master's client
    /// and its answer line when the job made a change.
    pub(super) fn done(
        &mut self,
        mut done: TerritoryDone,
    ) -> Option<(ConnectionRef, &'static str)> {
        self.running = self.running.saturating_sub(1);
        if let Some(dominion) = done.job.dominion.take()
            && dominion != self.dominion
        {
            self.dominion = dominion;
            self.version += 1;
        }
        match done.owners {
            Ok(read) => {
                let owners: BTreeMap<u16, ClaimOwner> = read
                    .iter()
                    .map(|owner| (owner.territory, claim_owner(owner)))
                    .collect();
                let upkeep: BTreeMap<u16, ClaimUpkeep> = read
                    .iter()
                    .map(|owner| (owner.territory, ClaimUpkeep::of(owner)))
                    .collect();
                if owners != self.owners {
                    self.owners = owners;
                    self.version += 1;
                }
                if upkeep != self.upkeep {
                    self.upkeep = upkeep;
                    self.upkeep_version += 1;
                }
            }
            Err(error) => tracing::warn!(%error, "territory owners not read"),
        }
        match done.conflicts {
            Some(Ok(read)) => {
                let conflicts: BTreeMap<u16, TerritoryConflict> = read
                    .into_iter()
                    .map(|conflict| (conflict.territory, conflict))
                    .collect();
                if conflicts != self.conflicts {
                    self.conflicts = conflicts;
                    self.version += 1;
                }
            }
            Some(Err(error)) => tracing::warn!(%error, "territory conflicts not read"),
            None => {}
        }
        let asked = done.job.change?;
        let line = match done.written? {
            Ok(true) => DONE,
            Ok(false) => match asked.change {
                OwnerChange::Set(_) => NO_SUCH_COMPANY,
                OwnerChange::Clear => NO_OWNER,
            },
            Err(error) => {
                tracing::error!(%error, territory = asked.territory, "territory owner not stored");
                NOT_STORED
            }
        };
        Some((asked.answer_to, line))
    }
}

impl SimWorld {
    /// The territory owner jobs to run at `now_ms` ([`Territories::take_jobs`]).
    pub(crate) fn take_territory_jobs(&mut self, now_ms: u64) -> Vec<TerritoryJob> {
        let wars = self.take_war_jobs();
        let unix_ms = self.unix_ms();
        self.territories.take_jobs(now_ms, wars, unix_ms)
    }

    /// Takes the result of a territory owner job: the clients get the changed claims at their
    /// next tick, and the game master who asked gets its answer.
    pub(crate) fn territories_done(&mut self, mut done: TerritoryDone) {
        if let Some((answers, read)) = done.wars.take() {
            self.wars_done(answers, read);
        }
        let answer = self.territories.done(done);
        self.show_claim_owners();
        if let Some((connection, line)) = answer {
            self.answer_gm_chat(&connection, vec![line.to_owned()]);
        }
    }

    /// A territory owner job whose task failed ([`Territories::lost`]).
    pub(crate) fn territory_job_lost(&mut self) {
        self.territories.lost();
    }
}

#[cfg(test)]
impl Territories {
    /// The claims after a read of `owners`.
    pub(super) fn read_for_test(owners: Vec<TerritoryOwner>) -> Self {
        let mut territories = Self::default();
        territories.read_again_for_test(owners);
        territories
    }

    /// Takes a later read of `owners`.
    pub(super) fn read_again_for_test(&mut self, owners: Vec<TerritoryOwner>) {
        self.done(TerritoryDone {
            job: TerritoryJob::default(),
            written: None,
            owners: Ok(owners),
            wars: None,
            conflicts: None,
        });
    }

    /// Takes a read of `conflicts` with no owner change.
    pub(super) fn read_conflicts_for_test(&mut self, conflicts: Vec<TerritoryConflict>) {
        self.done(TerritoryDone {
            job: TerritoryJob::default(),
            written: None,
            owners: Ok(Vec::new()),
            wars: None,
            conflicts: Some(Ok(conflicts)),
        });
    }

    /// The influence gains that wait for the next read job.
    pub(super) fn gains_for_test(&self) -> &[InfluenceGain] {
        &self.gains
    }
}

/// Adds `members` to the slot-2 group `group` of this tick, or makes the group (retail sends the
/// mutations and the spawn members in one slot-2 group).
pub(super) fn with_slot_2_members(
    group: Option<BundleGroup>,
    members: Vec<BundleMember>,
) -> BundleGroup {
    match group {
        Some(mut group) => {
            group.members.extend(members);
            group
        }
        None => BundleGroup {
            interest: nw_messages::LAND_CLAIM_MANAGER_INTEREST,
            members,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nw_messages::{LAND_CLAIM_MANAGER_COMPONENT, LAND_CLAIM_MANAGER_INTEREST};
    use uuid::Uuid;

    /// A made-up company and the claim it owns.
    const TERRITORY: u16 = 6;
    const COMPANY_NAME: &str = "Made Up Company";
    const COMPANY_FACTION: u8 = 2;
    const SINCE_MS: u64 = 1_782_069_297_824;
    /// Its upkeep is due two intervals after it took the claim (made up).
    const DUE_MS: u64 = SINCE_MS + 2 * UPKEEP_INTERVAL_MS;
    const NOW_MS: u64 = 5_000;

    fn connection() -> ConnectionRef {
        crate::world::tests::connection(1)
    }

    fn owner() -> TerritoryOwner {
        TerritoryOwner {
            territory: TERRITORY,
            company: Uuid::from_u128(0x1111),
            name: COMPANY_NAME.to_owned(),
            faction: COMPANY_FACTION,
            crest: [0; nw_db::CREST_LEN],
            since_ms: SINCE_MS,
            upkeep_due_ms: DUE_MS,
        }
    }

    fn read_done(owners: Vec<TerritoryOwner>) -> TerritoryDone {
        TerritoryDone {
            job: TerritoryJob::default(),
            written: None,
            owners: Ok(owners),
            wars: None,
            conflicts: None,
        }
    }

    fn claims_of(member: &BundleMember) -> LandClaimManagerState {
        let MemberState::Recorded { body, .. } = &member.state else {
            panic!("recorded state");
        };
        LandClaimManagerState::decode(body).expect("decode")
    }

    fn owners_of(state: &LandClaimManagerState) -> Vec<ClaimOwner> {
        state
            .owners
            .as_ref()
            .and_then(ReplicatedList::full)
            .expect("full owners")
            .elements
            .clone()
    }

    #[test]
    fn the_entity_spawns_once_with_its_card_and_every_claim() {
        let territories = Territories::default();
        let mut sent = None;
        let members = territories
            .members_due(&mut sent)
            .expect("encode")
            .expect("first time");
        assert_eq!(sent, Some(0));
        let components: Vec<u64> = members.iter().map(|member| member.component).collect();
        assert_eq!(
            components,
            [
                POSITION_COMPONENT,
                CARD_COMPONENT,
                LAND_CLAIM_MANAGER_COMPONENT,
                MAP_DATA_COMPONENT
            ]
        );
        assert_eq!(
            members[3].state,
            MemberState::Recorded {
                class: MAP_DATA_TYPE_INDEX,
                body: NO_MAP_POINTS.to_vec()
            },
            "an empty list of map points"
        );
        let MemberState::GdeMetadata(card) = &members[1].state else {
            panic!("card");
        };
        assert_eq!(card.asset, Some(WORLD_ENTITY_ASSET));
        assert_eq!(card.gde_ref, Some(WORLD_ENTITY_GDE_REF));
        let state = claims_of(&members[2]);
        let claims = CLAIM_KEYS.len();
        let governance = state
            .governance
            .as_ref()
            .and_then(ReplicatedList::full)
            .expect("governance");
        assert_eq!(governance.elements, vec![TERRITORY_GOVERNANCE; claims]);
        assert_eq!(owners_of(&state), vec![ClaimOwner::NONE; claims]);
        assert_eq!(
            state
                .positions
                .as_ref()
                .and_then(ReplicatedList::full)
                .expect("positions")
                .elements
                .len(),
            claims
        );
        assert_eq!(territories.members_due(&mut sent), Ok(None));
    }

    #[test]
    fn a_read_owner_shows_on_its_claim_and_goes_again_to_every_client() {
        let mut territories = Territories::default();
        let mut sent = None;
        territories.members_due(&mut sent).expect("encode");
        assert_eq!(territories.done(read_done(vec![owner()])), None);
        let members = territories
            .members_due(&mut sent)
            .expect("encode")
            .expect("changed claims");
        assert_eq!(members.len(), 1, "the claims alone");
        let owners = owners_of(&claims_of(&members[0]));
        let index = CLAIM_KEYS
            .iter()
            .position(|key| *key == TERRITORY)
            .expect("a claim");
        assert_eq!(owners[index].guild_name, COMPANY_NAME.as_bytes());
        assert_eq!(owners[index].guild_id, *Uuid::from_u128(0x1111).as_bytes());
        assert_eq!(owners[index].faction, COMPANY_FACTION);
        assert_eq!(owners[index].time_ns, SINCE_MS * NS_PER_MS);
        assert_eq!(
            owners
                .iter()
                .filter(|owner| **owner == ClaimOwner::NONE)
                .count(),
            CLAIM_KEYS.len() - 1
        );
        assert_eq!(territories.members_due(&mut sent), Ok(None));
        assert_eq!(territories.done(read_done(vec![owner()])), None);
        assert_eq!(
            territories.members_due(&mut sent),
            Ok(None),
            "the same owners go once"
        );
    }

    #[test]
    fn a_read_conflict_shows_its_influence_and_times_on_its_claim() {
        const RACE_MS: u64 = 1_790_305_214_065;
        const START_MS: u64 = 1_790_361_914_396;
        const LOTTERY_MS: u64 = 1_790_362_577_626;
        let mut territories = Territories::default();
        let mut sent = None;
        territories.members_due(&mut sent).expect("encode");
        territories.read_conflicts_for_test(vec![TerritoryConflict {
            territory: TERRITORY,
            influence: [0, 58, 25],
            race_start_ms: RACE_MS,
            conflict_faction: 2,
            conflict_start_ms: START_MS,
            lottery_end_ms: LOTTERY_MS,
        }]);
        let members = territories
            .members_due(&mut sent)
            .expect("encode")
            .expect("changed claims");
        let state = claims_of(&members[0]);
        let index = CLAIM_KEYS
            .iter()
            .position(|key| *key == TERRITORY)
            .expect("a claim");
        let full = |list: &Option<ReplicatedList<u64>>| {
            list.as_ref()
                .and_then(ReplicatedList::full)
                .expect("full")
                .elements
                .clone()
        };
        let percents: Vec<u8> = state
            .faction_influence
            .iter()
            .map(|list| {
                list.as_ref()
                    .and_then(ReplicatedList::full)
                    .expect("full")
                    .elements[index]
            })
            .collect();
        assert_eq!(percents, [0, 58, 25]);
        let factions = state
            .conflict_faction
            .as_ref()
            .and_then(ReplicatedList::full);
        assert_eq!(factions.expect("full").elements[index], 2);
        assert_eq!(
            full(&state.conflict_start_time)[index],
            START_MS * NS_PER_MS
        );
        assert_eq!(
            full(&state.conflict_lottery_end_time)[index],
            LOTTERY_MS * NS_PER_MS
        );
        assert_eq!(
            full(&state.influence_race_start_time)[index],
            RACE_MS * NS_PER_MS
        );
        let other = (index + 1) % CLAIM_KEYS.len();
        assert_eq!(
            full(&state.conflict_start_time)[other],
            NO_TIME,
            "no race there"
        );
        territories.read_conflicts_for_test(Vec::new());
        assert!(
            territories
                .members_due(&mut sent)
                .expect("encode")
                .is_some(),
            "an ended race goes again"
        );
    }

    #[test]
    fn influence_gains_go_with_the_next_read_job() {
        let mut territories = Territories::default();
        let gain = InfluenceGain {
            territory: TERRITORY,
            faction: 2,
            character: Uuid::from_u128(0x77),
            points: 100,
            at_ms: SINCE_MS,
        };
        territories.next_read_ms = u64::MAX;
        territories.gain(gain);
        let jobs = territories.take_jobs(0, Vec::new(), SINCE_MS);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].gains, vec![gain]);
        assert!(territories.gains_for_test().is_empty());
    }

    #[test]
    fn an_owned_claim_has_its_upkeep_times_and_no_earnings_in_its_governance() {
        let mut territories = Territories::default();
        assert_eq!(
            territories.governance(TERRITORY, SINCE_MS),
            None,
            "no owner"
        );
        territories.done(read_done(vec![owner()]));
        let version = territories.upkeep_version();
        let now_ms = SINCE_MS + UPKEEP_INTERVAL_MS + 1;
        let governance = territories.governance(TERRITORY, now_ms).expect("an owner");
        assert_eq!(governance.upkeep_due_ns, DUE_MS * NS_PER_MS);
        assert_eq!(
            governance.upkeep_payable_ns,
            (DUE_MS - nw_db::UPKEEP_PAYABLE_BEFORE_MS) * NS_PER_MS,
            "5 days before the due time (retail)"
        );
        assert_eq!(
            governance.current.end_ns,
            (SINCE_MS + 2 * UPKEEP_INTERVAL_MS) * NS_PER_MS,
            "the interval that runs now ends a whole interval count after the claim"
        );
        assert_eq!(governance.current.total, NO_EARNINGS);
        assert_eq!(governance.current.taxes.len(), TAX_TYPES.len());
        assert!(governance.history.is_empty());
        assert_eq!(governance.level, CLAIM_LEVEL);
        assert_eq!(
            governance.tax_changes,
            TAX_TYPES
                .iter()
                .map(|tax| TaxChange {
                    tax: *tax,
                    changed_ns: SINCE_MS * NS_PER_MS
                })
                .collect::<Vec<_>>()
        );
        let mut paid = owner();
        paid.upkeep_due_ms += UPKEEP_INTERVAL_MS;
        territories.done(read_done(vec![paid]));
        assert_eq!(territories.upkeep_version(), version + 1, "a paid upkeep");
        assert_eq!(
            territories.upkeep(TERRITORY).expect("owned").due_ms,
            DUE_MS + UPKEEP_INTERVAL_MS
        );
    }

    #[test]
    fn a_paid_upkeep_makes_a_read_due_at_once() {
        let mut territories = Territories::default();
        assert_eq!(territories.take_jobs(NOW_MS, Vec::new(), 0).len(), 1);
        territories.done(read_done(Vec::new()));
        assert!(
            territories.take_jobs(NOW_MS + 1, Vec::new(), 0).is_empty(),
            "not due"
        );
        territories.read_soon();
        assert_eq!(territories.take_jobs(NOW_MS + 1, Vec::new(), 0).len(), 1);
    }

    #[test]
    fn a_failed_read_keeps_the_owners() {
        let mut territories = Territories::default();
        territories.done(read_done(vec![owner()]));
        let before = territories.clone();
        territories.done(TerritoryDone {
            job: TerritoryJob::default(),
            written: None,
            owners: Err(DbError::OutOfRange { field: "test" }),
            wars: None,
            conflicts: None,
        });
        assert_eq!(territories.owners, before.owners);
        assert_eq!(territories.version, before.version);
    }

    #[test]
    fn reads_are_due_every_interval_and_never_two_at_once() {
        let mut territories = Territories::default();
        assert_eq!(
            territories.take_jobs(NOW_MS, Vec::new(), 0).len(),
            1,
            "the first read"
        );
        assert!(
            territories
                .take_jobs(NOW_MS + OWNER_READ_MS, Vec::new(), 0)
                .is_empty(),
            "one runs"
        );
        territories.done(read_done(Vec::new()));
        assert!(
            territories
                .take_jobs(NOW_MS + OWNER_READ_MS - 1, Vec::new(), 0)
                .is_empty()
        );
        assert_eq!(
            territories
                .take_jobs(NOW_MS + OWNER_READ_MS, Vec::new(), 0)
                .len(),
            1
        );
    }

    #[test]
    fn a_game_masters_change_is_asked_and_answered() {
        let mut territories = Territories::default();
        assert_eq!(
            territories.ask(1, OwnerChange::Clear, connection()),
            Err(NO_SUCH_CLAIM)
        );
        let set = OwnerChange::Set(COMPANY_NAME.to_owned());
        assert_eq!(
            territories.ask(TERRITORY, set.clone(), connection()),
            Ok(())
        );
        let jobs = territories.take_jobs(NOW_MS, Vec::new(), 0);
        let [job] = jobs.as_slice() else {
            panic!("the change alone: {jobs:?}");
        };
        let answer = |written| {
            territories.clone().done(TerritoryDone {
                job: job.clone(),
                written: Some(written),
                owners: Ok(vec![owner()]),
                wars: None,
                conflicts: None,
            })
        };
        assert_eq!(answer(Ok(true)), Some((connection(), DONE)));
        assert_eq!(answer(Ok(false)), Some((connection(), NO_SUCH_COMPANY)));
        assert_eq!(
            answer(Err(DbError::OutOfRange { field: "test" })),
            Some((connection(), NOT_STORED))
        );
        territories
            .ask(TERRITORY, OwnerChange::Clear, connection())
            .expect("a claim");
        let [clear] = territories
            .take_jobs(NOW_MS, Vec::new(), 0)
            .try_into()
            .expect("one job");
        assert_eq!(
            territories.done(TerritoryDone {
                job: clear,
                written: Some(Ok(false)),
                owners: Ok(Vec::new()),
                wars: None,
                conflicts: None,
            }),
            Some((connection(), NO_OWNER))
        );
    }

    #[test]
    fn a_station_fee_is_the_recipe_fee_times_the_crafts_times_the_territory_rate() {
        let crafting = |fee| RecipeDetails {
            fee,
            ..RecipeDetails::default()
        };
        let refining = |fee| RecipeDetails {
            fee,
            refining: true,
            ..RecipeDetails::default()
        };
        // Observed (retail 2997 fees): 35 crafts of a 1-coin refining recipe showed 17, 75
        // showed 37, 5 crafts of a 4-coin recipe 10, one craft of a 1-coin recipe 1, one of a
        // 50-coin research note 25.
        assert_eq!(station_fee(&refining(1), 35), 17);
        assert_eq!(station_fee(&refining(1), 75), 37);
        assert_eq!(station_fee(&refining(4), 5), 10);
        assert_eq!(station_fee(&refining(1), 1), LEAST_STATION_FEE);
        assert_eq!(station_fee(&crafting(50), 1), 25);
        assert_eq!(station_fee(&crafting(0), 10), 0, "a free recipe stays free");
    }

    #[test]
    fn the_spawn_members_join_the_mutations_group_or_make_their_own() {
        let members = Territories::default()
            .members_due(&mut None)
            .expect("encode")
            .expect("first time");
        let alone = with_slot_2_members(None, members.clone());
        assert_eq!(alone.interest, LAND_CLAIM_MANAGER_INTEREST);
        assert_eq!(alone.members, members);
        let joined = with_slot_2_members(Some(alone), members.clone());
        assert_eq!(joined.members.len(), 2 * members.len());
    }
}
