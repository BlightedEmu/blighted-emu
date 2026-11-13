// SPDX-License-Identifier: AGPL-3.0-or-later
//! The season pass of a player (seasons finding 2026-11-12): the reward track, the journey, the
//! activity card and the tracked stats of the live season ([`nw_content::seasons`]), the
//! client's season calls (5611, 5645, 5647, 5648) and the two season states (5485, 5606).
//!
//! **Observed** (`Hive_Seasons_Rewards`, `season_track_activity_card_and_get_rewards`,
//! `reroll_activities`):
//! - The season experience is the `SeasonPass` categorical progression (type 911): each stamp,
//!   line and card raised its points at once; `SeasonXpBySeason` of 5485 came only with a redeem
//!   (71,945 = 26 levels of 2,750 and 445 points).
//! - A stamp (5645) is the card, then the task's place in `activitiesTasks`, the row, the column
//!   and a wild flag: a wild stamp (flag 1) takes one of `wildStampCount` and marks the square in
//!   `wildStampedSquares`; a task stamp (flag 0) claims the task (`claimedTasks`) on a square of
//!   the task's `Type` (2 of 2 retail stamps). Each stamp gives the square's experience (50), and
//!   each row or column it fills `LineBonusXp` (250) and one wild stamp, at most 3 (Observed: 7
//!   stamps of 7; no retail body holds more than 3).
//! - A new card (5647) gives `CardBonusXp` (150, 2 of 3 ledgers: the third gave 220, Unknown
//!   why), raises `cardCount` and the `CompleteActivityCard` stat, and deals a new template and
//!   21 tasks; a card with 4 stamps (`StampsToComplete`) was done (`reroll_activities`).
//! - A reroll (5648) raises `rerollCount` and deals new tasks (no retail body holds more than 2
//!   rerolls; a new card set it back to 0).
//! - A redeem (5611) names the reward's CRC-32 in the low half and the season's in the high half
//!   of its id, and sets the reward's `RewardIndex` bit in `RedeemBitmask`.
//!
//! **Inferred**: a new card needs `StampsToComplete` stamps; a reroll keeps the claimed tasks in
//! their places and deals the others again; at most 2 rerolls a card; the card's template and
//! tasks are random, at least `RequiredFromEachType` of each task type; a redeem goes straight to
//! `RedeemBitmask` (retail set an `EscrowBitmask` bit first, then cleared it); a journey task's
//! reward is redeemed with the same call once its stat reaches the task's goal, a chapter's once
//! `ChapterTaskCount` of its tasks are redeemed. Retail keeps the season per account; here it is
//! per character (world migration 0041).
//!
//! **Not built** (each named in the finding): rewards that give entitlements (skins, emotes,
//! titles) are refused, as the server grants no entitlement from a season reward; the premium
//! track needs the account's `PremiumEntitlementId`, which no store offer here sells; the
//! `wildStampAward*` rule and the pattern bonus (`PatternBonusXp`) are Unknown; a stat whose
//! filters the server does not read never counts ([`StatKind::Unread`]). The game data has no
//! season shop (no `ShopData` row names a season).

use std::collections::BTreeSet;
use std::sync::Arc;

use bevy_ecs::entity::Entity;
use nw_bus::{PlayerActivityCard, PlayerSeason};
use nw_content::seasons::{
    CARD_SIDE, CARD_TASKS_GROUP, RewardKind, SeasonCatalog, SeasonReward, StatKind,
};
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::BundleMember;
use nw_messages::items::FIRST_VERSION;
use nw_messages::seasons::{
    SQUARE_LEN, SeasonPassEntry, SeasonRewardsValues, SeasonsCall, StatGroup, TrackedStatsValues,
    WildStampAward, response,
};
use nw_sim_core::Dice;

use super::gear::unix_ns;
use super::status_effects::GainKind;
use super::tradeskill::ONE_FIRING;
use super::{Player, SimWorld, Vitals};

/// The categorical progression of the season experience (`CategoricalProgressionData`): the
/// account's season experience ([`nw_db::SEASON_PASS_PROGRESSION`]).
pub(super) const SEASON_PASS: &str = nw_db::SEASON_PASS_PROGRESSION;
/// The most wild stamps a card holds (Observed: no retail body holds more).
const MAX_WILD_STAMPS: u16 = 3;
/// The most rerolls of one card (Inferred: the highest retail `rerollCount`).
const MAX_REROLLS: u8 = 2;
/// The wild stamps of a player's first card (Observed: `season_track_activity_card_and_get_rewards`,
/// `cardCount` 0, held 3).
const FIRST_WILD_STAMPS: u16 = MAX_WILD_STAMPS;
/// The wild stamp award children of a first state (Observed in that ledger: the next midnight
/// UTC, 255 and 1; their rule is Unknown, so they stay as sent).
const FIRST_AWARDS_THIS_SESSION: u8 = u8::MAX;
const FIRST_AWARD_REMAINING: u16 = 1;
/// Nanoseconds in a day.
const NS_PER_DAY: u64 = 86_400_000_000_000;
/// Bits of a redeem word and of a claimed task word.
const REDEEM_WORD_BITS: u16 = 64;
const CLAIM_WORD_BITS: usize = 16;
/// A 5611 reward id: the reward's CRC-32 in the low half, the season's in the high half.
const REWARD_SEASON_SHIFT: u32 = 32;
/// The stamp call's square bytes: task, row, column, wild flag.
const SQUARE_TASK: usize = 0;
const SQUARE_ROW: usize = 1;
const SQUARE_COLUMN: usize = 2;
const SQUARE_WILD: usize = 3;
/// The items a reward's `ItemId` gives.
const ONE_ITEM: u32 = 1;
/// The game event names the season experience gains log.
const CARD_EVENT: &str = "SeasonsActivities";
const REWARD_EVENT: &str = "SeasonsRewards";

/// Why a season call was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SeasonRefusal {
    /// The content has no live season, or the call names another one.
    NoSeason,
    /// The square is off the card or stamped already.
    BadSquare,
    /// No wild stamp left.
    NoWildStamp,
    /// The task is not on the card, claimed already, short of its goal, or of another type than
    /// the square.
    TaskNotDone,
    /// The card has fewer stamps than `StampsToComplete`.
    CardNotDone,
    /// No reroll left on this card.
    NoReroll,
    /// No such reward, or redeemed already.
    UnknownReward,
    /// The reward's level, task or chapter is not reached.
    NotReached,
    /// A premium reward without the season's premium entitlement.
    NotPremium,
    /// The reward gives an entitlement, which this server does not grant.
    Entitlement,
}

/// What a player did that a tracked stat counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SeasonEvent<'a> {
    /// It killed a mob.
    Kill {
        /// The mob's vitals id.
        vitals_id: &'a str,
        /// The mob's vitals categories.
        categories: &'a [String],
        /// The mob's level.
        level: u32,
        /// The weapon mastery of the weapon in its hand.
        weapon: Option<&'a str>,
    },
    /// It gathered `items` items of gatherable `gatherable`.
    Gathered {
        /// The gatherable id.
        gatherable: &'a str,
        /// The gatherable's tradeskill.
        tradeskill: Option<&'a str>,
        /// The items it gave.
        items: u32,
    },
    /// It crafted `count` items of `item`.
    Crafted {
        /// The item id.
        item: &'a str,
        /// The item's classes.
        classes: &'a [String],
        /// The items made.
        count: u32,
    },
    /// It finished a quest; `mission` for a town board mission.
    QuestDone {
        /// The quest is a mission.
        mission: bool,
    },
    /// Its categorical progression `id` gained `points` and `ranks`.
    Progression {
        /// The `CategoricalProgressionId`.
        id: &'a str,
        /// The points gained.
        points: u32,
        /// The ranks gained.
        ranks: u32,
    },
    /// It got game event `id` (today the capture the flag end events).
    GameEvent {
        /// The `EventID`.
        id: &'a str,
    },
    /// It finished an activity card.
    CardDone,
    /// It redeemed a journey task's reward; `challenge` for a task of the challenge chapter.
    JourneyTaskDone {
        /// The task is of the challenge chapter.
        challenge: bool,
    },
}

/// Whether `list` is empty or holds `value` (any case).
fn takes(list: &[String], value: &str) -> bool {
    list.is_empty() || list.iter().any(|item| item.eq_ignore_ascii_case(value))
}

/// How much `event` raises a stat of `kind`; 0 when it does not count for it.
fn counted(kind: &StatKind, event: &SeasonEvent<'_>) -> u32 {
    let once = |counts: bool| u32::from(counts);
    match (kind, *event) {
        (
            StatKind::Kill {
                categories,
                targets,
                level,
                weapons,
            },
            SeasonEvent::Kill {
                vitals_id,
                categories: mob_categories,
                level: mob_level,
                weapon,
            },
        ) => once(
            (categories.is_empty()
                || mob_categories
                    .iter()
                    .any(|category| takes(categories, category)))
                && takes(targets, vitals_id)
                && mob_level >= *level
                && (weapons.is_empty() || weapon.is_some_and(|weapon| takes(weapons, weapon))),
        ),
        (
            StatKind::Gather {
                gatherables,
                tradeskills,
                per_use,
            },
            SeasonEvent::Gathered {
                gatherable,
                tradeskill,
                items,
            },
        ) => {
            let matches = takes(gatherables, gatherable)
                && (tradeskills.is_empty()
                    || tradeskill.is_some_and(|tradeskill| takes(tradeskills, tradeskill)));
            match (matches, per_use) {
                (false, _) => 0,
                (true, true) => 1,
                (true, false) => items,
            }
        }
        (
            StatKind::Craft { items, classes },
            SeasonEvent::Crafted {
                item,
                classes: of,
                count,
            },
        ) => {
            let class_matches = classes.is_empty() || of.iter().any(|class| takes(classes, class));
            if takes(items, item) && class_matches {
                count
            } else {
                0
            }
        }
        (StatKind::CompleteQuest { missions_only }, SeasonEvent::QuestDone { mission }) => {
            once(mission || !missions_only)
        }
        (
            StatKind::CategoricalProgression {
                progressions,
                ranks,
            },
            SeasonEvent::Progression {
                id,
                points,
                ranks: gained,
            },
        ) if !progressions.is_empty() && takes(progressions, id) => {
            if *ranks {
                gained
            } else {
                points
            }
        }
        (StatKind::GameEvent { events }, SeasonEvent::GameEvent { id }) => once(takes(events, id)),
        (StatKind::CompleteActivityCard, SeasonEvent::CardDone) => 1,
        (
            StatKind::CompleteJourneyTask { challenge },
            SeasonEvent::JourneyTaskDone { challenge: done },
        ) => once(*challenge == done),
        _ => 0,
    }
}

/// The count of stat `crc` in `stats`; 0 for one never counted.
fn count_of(stats: &[(u32, u32)], crc: u32) -> u32 {
    stats
        .iter()
        .find(|(stat, _)| *stat == crc)
        .map_or(0, |(_, count)| *count)
}

/// Adds `amount` to stat `crc` of `stats`.
fn add_to(stats: &mut Vec<(u32, u32)>, crc: u32, amount: u32) {
    match stats.iter_mut().find(|(stat, _)| *stat == crc) {
        Some((_, count)) => *count = count.saturating_add(amount),
        None => stats.push((crc, amount)),
    }
}

/// The squares of row `row` or of column `column` of a card, as bits.
fn line_masks() -> impl Iterator<Item = u16> {
    let side = u16::try_from(CARD_SIDE).unwrap_or_default();
    let rows = (0..side).map(move |row| ((1_u16 << side) - 1) << (row * side));
    let columns = (0..side)
        .map(move |column| (0..side).fold(0_u16, |mask, row| mask | 1 << (row * side + column)));
    rows.chain(columns)
}

/// The rows and columns that `after` fills and `before` did not.
fn lines_filled(before: u16, after: u16) -> u32 {
    let full = |stamped: u16, mask: u16| stamped & mask == mask;
    let filled = line_masks()
        .filter(|mask| full(after, *mask) && !full(before, *mask))
        .count();
    u32::try_from(filled).unwrap_or_default()
}

/// The next midnight UTC after Unix ns `now_ns`.
const fn next_midnight_ns(now_ns: u64) -> u64 {
    (now_ns / NS_PER_DAY + 1) * NS_PER_DAY
}

/// The 16-bit words of the task bits `claimed`, without trailing empty words (Observed: one
/// word `0x4000` for task 14, two words for task 16).
fn claim_words(claimed: u32) -> Vec<u16> {
    let mut words: Vec<u16> = (0..u32::BITS as usize / CLAIM_WORD_BITS)
        .map(|word| {
            let bits = (claimed >> (word * CLAIM_WORD_BITS)) & u32::from(u16::MAX);
            u16::try_from(bits).unwrap_or_default()
        })
        .collect();
    while words.last() == Some(&0) {
        words.pop();
    }
    words
}

/// A player's season pass.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct SeasonPass {
    /// The state a save and a handoff carry.
    state: PlayerSeason,
    /// What its client has of the season rewards state; `None` before the first.
    rewards_sent: Option<SeasonRewardsValues>,
    /// What its client has of the tracked stats state.
    stats_sent: Option<TrackedStatsValues>,
    /// The season experience its client was last told in `SeasonXpBySeason`.
    shown_experience: u64,
    /// The wild stamp award children its client has.
    award: WildStampAward,
}

impl SeasonPass {
    /// The season pass of `carried` for the live season of `catalog` at Unix ns `now_ns`: a
    /// state of another season (or none) starts the live one with a new card (a past season's
    /// rewards and stats end with it: Inferred).
    pub(super) fn arrive(
        carried: &PlayerSeason,
        catalog: &SeasonCatalog,
        now_ns: u64,
        dice: &mut Dice,
    ) -> Self {
        let mut state = carried.clone();
        if catalog.season_crc != 0 && state.season != catalog.season_crc {
            state = PlayerSeason {
                season: catalog.season_crc,
                first_connect_ns: if carried.first_connect_ns == 0 {
                    now_ns
                } else {
                    carried.first_connect_ns
                },
                card: PlayerActivityCard {
                    wild_stamps: FIRST_WILD_STAMPS,
                    ..PlayerActivityCard::default()
                },
                versions: carried.versions,
                ..PlayerSeason::default()
            };
        }
        let mut pass = Self {
            state,
            ..Self::default()
        };
        if catalog.season_crc != 0 && pass.state.card.tasks.is_empty() {
            pass.deal_card(catalog, dice);
        }
        pass
    }

    /// The state a save or a handoff carries.
    pub(super) fn carried(&self) -> PlayerSeason {
        self.state.clone()
    }

    /// A new template and new tasks for the card; claims, stamps and the card's stats clear.
    fn deal_card(&mut self, catalog: &SeasonCatalog, dice: &mut Dice) {
        let card = &mut self.state.card;
        let templates = u32::try_from(catalog.cards.len()).unwrap_or_default();
        card.template = u8::try_from(dice.below(templates.max(1))).unwrap_or_default();
        card.tasks = deal_tasks(catalog, &[], dice);
        card.claimed = 0;
        card.stamped = 0;
        card.wild_stamped = 0;
        card.rerolls = 0;
        card.stats.clear();
    }

    /// The season rewards state of this pass, as plain values.
    fn values(&self, catalog: &SeasonCatalog) -> SeasonRewardsValues {
        let card = &self.state.card;
        SeasonRewardsValues {
            card_template: card.template,
            claimed_tasks: claim_words(card.claimed),
            stamped_squares: card.stamped,
            wild_stamped_squares: card.wild_stamped,
            reroll_count: card.rerolls,
            activities_tasks: card.tasks.clone(),
            card_count: card.cards_done,
            wild_stamp_count: card.wild_stamps,
            award: self.award,
            seasons: (catalog.season_crc != 0)
                .then(|| SeasonPassEntry {
                    season: catalog.season_crc,
                    experience: self.shown_experience,
                    redeemed: self.state.redeemed.clone(),
                    escrow: Vec::new(),
                })
                .into_iter()
                .collect(),
            first_character_connect_ns: self.state.first_connect_ns,
        }
    }

    /// The count of season stat `index` for a character of level `level`: an `ObtainLevel` stat
    /// is 1 once the level is reached, another stat its count.
    fn season_count(&self, catalog: &SeasonCatalog, index: u16, level: u32) -> u32 {
        let Some(stat) = catalog.stats.get(usize::from(index)) else {
            return 0;
        };
        match stat.kind {
            StatKind::ObtainLevel { level: wanted } => u32::from(level >= wanted),
            _ => count_of(&self.state.stats, stat.crc),
        }
    }

    /// The stat indexes of the card's tasks, ascending.
    fn card_stats(&self, catalog: &SeasonCatalog) -> Vec<u16> {
        let stats: BTreeSet<u16> = self
            .state
            .card
            .tasks
            .iter()
            .filter_map(|task| catalog.card_task(*task))
            .map(|task| task.stat)
            .collect();
        stats.into_iter().collect()
    }

    fn stats_values(&self, catalog: &SeasonCatalog, level: u32) -> TrackedStatsValues {
        if catalog.season_crc == 0 {
            return TrackedStatsValues::default();
        }
        let season = StatGroup {
            group: catalog.season_crc,
            stats: catalog
                .journey_stats()
                .into_iter()
                .map(|index| (index, self.season_count(catalog, index, level)))
                .collect(),
        };
        let card = StatGroup {
            group: lowercase_crc32(CARD_TASKS_GROUP),
            stats: self
                .card_stats(catalog)
                .into_iter()
                .filter_map(|index| {
                    let stat = catalog.stats.get(usize::from(index))?;
                    Some((index, count_of(&self.state.card.stats, stat.crc)))
                })
                .collect(),
        };
        TrackedStatsValues {
            groups: vec![season, card],
        }
    }

    /// The season states its client should get now: both whole the first time (in one bundle,
    /// as retail), then the ones that changed. `experience` is the season experience, `level`
    /// the character level, `now_ns` Unix ns.
    pub(super) fn take_members(
        &mut self,
        catalog: &SeasonCatalog,
        experience: u64,
        level: u32,
        now_ns: u64,
    ) -> Vec<BundleMember> {
        let first = self.rewards_sent.is_none();
        if first {
            self.shown_experience = experience;
            self.award = WildStampAward {
                bound_ns: next_midnight_ns(now_ns),
                this_session: FIRST_AWARDS_THIS_SESSION,
                remaining: FIRST_AWARD_REMAINING,
            };
        }
        let rewards = self.values(catalog);
        let stats = self.stats_values(catalog, level);
        let (rewards_version, stats_version) = &mut self.state.versions;
        let rewards_state = match &self.rewards_sent {
            None => {
                *rewards_version = (*rewards_version + 1).max(FIRST_VERSION);
                Some(rewards.full(*rewards_version))
            }
            Some(sent) => rewards
                .change_from(sent, *rewards_version + 1)
                .inspect(|_| {
                    *rewards_version += 1;
                }),
        };
        let stats_state = match &self.stats_sent {
            None => {
                *stats_version = (*stats_version + 1).max(FIRST_VERSION);
                Some(stats.full(*stats_version))
            }
            Some(sent) => stats.change_from(sent, *stats_version + 1).inspect(|_| {
                *stats_version += 1;
            }),
        };
        self.rewards_sent = Some(rewards);
        self.stats_sent = Some(stats);
        let members = [
            rewards_state.map(|state| state.member()),
            stats_state.map(|state| state.member()),
        ];
        members
            .into_iter()
            .flatten()
            .filter_map(|member| {
                member
                    .inspect_err(|error| tracing::warn!(%error, "season state not written"))
                    .ok()
            })
            .collect()
    }

    /// Counts `event` on the season's journey stats and on the card's task stats.
    pub(super) fn count(&mut self, catalog: &SeasonCatalog, event: &SeasonEvent<'_>) {
        let journey: BTreeSet<u16> = catalog.journey_stats().into_iter().collect();
        let card = self.card_stats(catalog);
        for (index, stat) in catalog.stats.iter().enumerate() {
            let Ok(index) = u16::try_from(index) else {
                break;
            };
            let amount = counted(&stat.kind, event);
            if amount == 0 {
                continue;
            }
            if journey.contains(&index) {
                add_to(&mut self.state.stats, stat.crc, amount);
            }
            if card.contains(&index) {
                add_to(&mut self.state.card.stats, stat.crc, amount);
            }
        }
    }

    /// Stamps a square (5645 `square`); the season experience it gives.
    pub(super) fn stamp(
        &mut self,
        catalog: &SeasonCatalog,
        square: [u8; SQUARE_LEN],
    ) -> Result<u32, SeasonRefusal> {
        let (row, column) = (
            usize::from(square[SQUARE_ROW]),
            usize::from(square[SQUARE_COLUMN]),
        );
        let card_squares = catalog
            .cards
            .get(usize::from(self.state.card.template))
            .ok_or(SeasonRefusal::NoSeason)?;
        if row >= CARD_SIDE || column >= CARD_SIDE {
            return Err(SeasonRefusal::BadSquare);
        }
        let place = row * CARD_SIDE + column;
        let bit = 1_u16 << place;
        let card_square = card_squares.get(place).ok_or(SeasonRefusal::BadSquare)?;
        let before = self.state.card.stamped;
        if before & bit != 0 {
            return Err(SeasonRefusal::BadSquare);
        }
        if square[SQUARE_WILD] == 0 {
            let task_place = usize::from(square[SQUARE_TASK]);
            let task = self
                .state
                .card
                .tasks
                .get(task_place)
                .and_then(|task| catalog.card_task(*task))
                .ok_or(SeasonRefusal::TaskNotDone)?;
            let stat_crc = catalog
                .stats
                .get(usize::from(task.stat))
                .map(|stat| stat.crc)
                .ok_or(SeasonRefusal::TaskNotDone)?;
            let task_bit = 1_u32
                .checked_shl(u32::from(square[SQUARE_TASK]))
                .ok_or(SeasonRefusal::TaskNotDone)?;
            let done = count_of(&self.state.card.stats, stat_crc) >= task.goal;
            if self.state.card.claimed & task_bit != 0
                || !done
                || !task.kind.eq_ignore_ascii_case(&card_square.kind)
            {
                return Err(SeasonRefusal::TaskNotDone);
            }
            self.state.card.claimed |= task_bit;
        } else {
            if self.state.card.wild_stamps == 0 {
                return Err(SeasonRefusal::NoWildStamp);
            }
            self.state.card.wild_stamps -= 1;
            self.state.card.wild_stamped |= bit;
        }
        let after = before | bit;
        self.state.card.stamped = after;
        let lines = lines_filled(before, after);
        self.state.card.wild_stamps = self
            .state
            .card
            .wild_stamps
            .saturating_add(u16::try_from(lines).unwrap_or_default())
            .min(MAX_WILD_STAMPS);
        Ok(card_square
            .experience
            .saturating_add(lines.saturating_mul(catalog.card_bonus.line)))
    }

    /// Starts a new card (5647) once this one has its stamps; the season experience it gives.
    pub(super) fn new_card(
        &mut self,
        catalog: &SeasonCatalog,
        dice: &mut Dice,
    ) -> Result<u32, SeasonRefusal> {
        if catalog.season_crc == 0 {
            return Err(SeasonRefusal::NoSeason);
        }
        if self.state.card.stamped.count_ones() < catalog.card_bonus.stamps_to_complete {
            return Err(SeasonRefusal::CardNotDone);
        }
        self.state.card.cards_done = self.state.card.cards_done.saturating_add(1);
        self.deal_card(catalog, dice);
        self.count(catalog, &SeasonEvent::CardDone);
        Ok(catalog.card_bonus.card)
    }

    /// Deals the card's unclaimed tasks again (5648).
    pub(super) fn reroll(
        &mut self,
        catalog: &SeasonCatalog,
        dice: &mut Dice,
    ) -> Result<(), SeasonRefusal> {
        if catalog.season_crc == 0 {
            return Err(SeasonRefusal::NoSeason);
        }
        let card = &self.state.card;
        if card.rerolls >= MAX_REROLLS {
            return Err(SeasonRefusal::NoReroll);
        }
        let kept: Vec<Option<u32>> = card
            .tasks
            .iter()
            .enumerate()
            .map(|(place, task)| {
                let claimed = 1_u32
                    .checked_shl(u32::try_from(place).unwrap_or(u32::MAX))
                    .is_some_and(|bit| card.claimed & bit != 0);
                claimed.then_some(*task)
            })
            .collect();
        let claimed: Vec<u32> = kept.iter().flatten().copied().collect();
        let mut fresh = deal_tasks(catalog, &claimed, dice).into_iter();
        let tasks = kept
            .into_iter()
            .map(|kept| kept.or_else(|| fresh.next()).unwrap_or_default())
            .collect();
        let card = &mut self.state.card;
        card.tasks = tasks;
        card.rerolls += 1;
        card.stats.clear();
        Ok(())
    }

    /// Redeems reward `reward_id` (5611) at season level `season_level`; `premium` when the
    /// account holds the season's premium entitlement. Returns the reward.
    pub(super) fn redeem(
        &mut self,
        catalog: &SeasonCatalog,
        reward_id: u64,
        season_level: u16,
        premium: bool,
        level: u32,
    ) -> Result<SeasonReward, SeasonRefusal> {
        let season = u32::try_from(reward_id >> REWARD_SEASON_SHIFT).unwrap_or_default();
        if catalog.season_crc == 0 || season != catalog.season_crc {
            return Err(SeasonRefusal::NoSeason);
        }
        let low = u32::try_from(reward_id & u64::from(u32::MAX)).unwrap_or_default();
        let position = catalog
            .rewards
            .iter()
            .position(|reward| reward.crc == low)
            .ok_or(SeasonRefusal::UnknownReward)?;
        let reward = &catalog.rewards[position];
        if self.redeemed(reward.index) {
            return Err(SeasonRefusal::UnknownReward);
        }
        let reached = match reward.kind {
            RewardKind::Free => reward.level.is_some_and(|wanted| season_level >= wanted),
            RewardKind::Premium => {
                if !premium {
                    return Err(SeasonRefusal::NotPremium);
                }
                reward.level.is_some_and(|wanted| season_level >= wanted)
            }
            RewardKind::Journey => catalog
                .journey
                .iter()
                .find(|task| task.reward == Some(position))
                .is_some_and(|task| self.season_count(catalog, task.stat, level) >= task.goal),
            RewardKind::Chapter => catalog
                .chapters
                .iter()
                .position(|chapter| chapter.reward == Some(position))
                .is_some_and(|chapter| {
                    self.chapter_redeemed(catalog, chapter)
                        >= catalog.chapters[chapter].tasks_needed
                }),
        };
        if !reached {
            return Err(SeasonRefusal::NotReached);
        }
        if reward.entitlements.is_some() {
            return Err(SeasonRefusal::Entitlement);
        }
        let word = usize::from(reward.index / REDEEM_WORD_BITS);
        if self.state.redeemed.len() <= word {
            self.state.redeemed.resize(word + 1, 0);
        }
        self.state.redeemed[word] |= 1 << (reward.index % REDEEM_WORD_BITS);
        if reward.kind == RewardKind::Journey
            && let Some(chapter) = catalog
                .journey
                .iter()
                .find(|task| task.reward == Some(position))
                .and_then(|task| task.chapter)
        {
            let challenge = catalog.chapters[chapter].challenge;
            self.count(catalog, &SeasonEvent::JourneyTaskDone { challenge });
        }
        Ok(reward.clone())
    }

    /// The season experience its client is told at the next state: a redeem tells it
    /// (Observed: retail sent `SeasonXpBySeason` only with a redeem).
    pub(super) fn show_experience(&mut self, experience: u64) {
        self.shown_experience = experience;
    }

    fn redeemed(&self, index: u16) -> bool {
        self.state
            .redeemed
            .get(usize::from(index / REDEEM_WORD_BITS))
            .is_some_and(|word| word & 1 << (index % REDEEM_WORD_BITS) != 0)
    }

    /// The redeemed tasks of chapter `chapter`.
    fn chapter_redeemed(&self, catalog: &SeasonCatalog, chapter: usize) -> u32 {
        let redeemed = catalog
            .journey
            .iter()
            .filter(|task| task.chapter == Some(chapter))
            .filter_map(|task| task.reward)
            .filter(|reward| self.redeemed(catalog.rewards[*reward].index))
            .count();
        u32::try_from(redeemed).unwrap_or_default()
    }
}

/// `TasksPerList` card tasks other than `keep` and the tasks in it: `RequiredFromEachType` of each
/// task type first, then any; in a random order (Inferred).
fn deal_tasks(catalog: &SeasonCatalog, keep: &[u32], dice: &mut Dice) -> Vec<u32> {
    let wanted = catalog.tasks_per_card.saturating_sub(keep.len());
    let mut left: Vec<&nw_content::seasons::SeasonTask> = catalog
        .card_tasks
        .iter()
        .filter(|task| !keep.contains(&task.crc))
        .collect();
    let mut take = |left: &mut Vec<&nw_content::seasons::SeasonTask>, kind: Option<&str>| {
        let fitting: Vec<usize> = (0..left.len())
            .filter(|at| kind.is_none_or(|kind| left[*at].kind == kind))
            .collect();
        let count = u32::try_from(fitting.len()).unwrap_or_default();
        (count > 0).then(|| {
            let pick = usize::try_from(dice.below(count)).unwrap_or_default();
            left.remove(fitting[pick]).crc
        })
    };
    let mut dealt = Vec::with_capacity(wanted);
    if keep.is_empty() {
        for kind in catalog.card_task_kinds() {
            for _ in 0..catalog.required_per_type {
                if let Some(task) = take(&mut left, Some(kind)) {
                    dealt.push(task);
                }
            }
        }
    }
    while dealt.len() < wanted {
        let Some(task) = take(&mut left, None) else {
            break;
        };
        dealt.push(task);
    }
    for at in (1..dealt.len()).rev() {
        let other = usize::try_from(dice.below(u32::try_from(at + 1).unwrap_or(u32::MAX)))
            .unwrap_or_default();
        dealt.swap(at, other);
    }
    dealt.truncate(wanted);
    dealt
}

impl SimWorld {
    /// The season experience of `player`: its `SeasonPass` progression.
    fn season_experience(&self, player: &Player) -> u32 {
        self.content
            .progression
            .categorical(SEASON_PASS)
            .map_or(0, |mastery| player.masteries.experience(mastery))
    }

    /// Carries out the season call `call` of player `entity`: the [`response`] value of its
    /// answer.
    pub(super) fn season_call(&mut self, entity: Entity, call: &SeasonsCall) -> u32 {
        let content = Arc::clone(&self.content);
        let catalog = &content.seasons;
        let Some(player) = self.world.get::<Player>(entity) else {
            return response::FAILURE;
        };
        let (number, level) = (
            player.number,
            content.progression.level_of(player.experience),
        );
        let season_level = player.masteries.rank(&content.progression, SEASON_PASS);
        let premium = player.entitlements.contains(&catalog.premium_entitlement);
        let mut pass = player.season.clone();
        let outcome = match *call {
            SeasonsCall::StampSquare { square, card, .. } => {
                tracing::debug!(player = number, card, ?square, "season stamp");
                pass.stamp(catalog, square)
                    .map(|experience| (experience, None, CARD_EVENT))
            }
            SeasonsCall::NewCard { .. } => pass
                .new_card(catalog, &mut self.dice)
                .map(|experience| (experience, None, CARD_EVENT)),
            SeasonsCall::Reroll { .. } => pass
                .reroll(catalog, &mut self.dice)
                .map(|()| (0, None, CARD_EVENT)),
            SeasonsCall::Redeem { reward, .. } => pass
                .redeem(catalog, reward, season_level, premium, level)
                .map(|reward| (reward.experience, reward.item, REWARD_EVENT)),
        };
        let (experience, item, event) = match outcome {
            Ok(done) => done,
            Err(refusal) => {
                tracing::debug!(player = number, ?refusal, "season call refused");
                return response::FAILURE;
            }
        };
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.season = pass;
        }
        if experience > 0 {
            let points = f64::from(experience);
            self.gain_categorical(
                number,
                event,
                SEASON_PASS,
                GainKind::Progression,
                points,
                ONE_FIRING,
            );
        }
        if let Some(item) = item {
            self.give_loot(number, vec![(item, ONE_ITEM)], self.last_now_ms);
        }
        if matches!(call, SeasonsCall::Redeem { .. }) {
            let shown = self
                .world
                .get::<Player>(entity)
                .map_or(0, |player| self.season_experience(player));
            if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                player.season.show_experience(u64::from(shown));
            }
        }
        response::SUCCESS
    }

    /// Player `entity` did `event`: its season stats count it.
    pub(super) fn season_event(&mut self, entity: Entity, event: SeasonEvent<'_>) {
        let content = Arc::clone(&self.content);
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.season.count(&content.seasons, &event);
        }
    }

    /// Queues the season states of each player in game: both whole once its client is in the
    /// world (retail sent them in one bundle then; they start the client's pin of the tracked
    /// quest: season rewards state of `nw_messages::seasons`), then their changes. A change waits
    /// while an earlier one is still queued (one member per component goes in a bundle).
    pub(super) fn tick_seasons(&mut self) {
        let content = Arc::clone(&self.content);
        let season_pass = content.progression.categorical(SEASON_PASS);
        let now_ns = unix_ns(self.server_ms);
        let mut query = self.world.query::<(&mut Player, &Vitals)>();
        for (mut player, vitals) in query.iter_mut(&mut self.world) {
            if !player.own_card_sent || !vitals.spawned {
                continue;
            }
            let queued = player.extra_members.iter().any(|member| {
                member.component == nw_messages::seasons::SEASONS_REWARDS_COMPONENT
                    || member.component == nw_messages::seasons::SEASONS_TRACKED_STATS_COMPONENT
            });
            if queued {
                continue;
            }
            let experience = season_pass.map_or(0, |mastery| player.masteries.experience(mastery));
            let level = content.progression.level_of(player.experience);
            let members =
                player
                    .season
                    .take_members(&content.seasons, u64::from(experience), level, now_ns);
            player.extra_members.extend(members);
        }
    }
}

#[cfg(test)]
mod tests {
    use nw_content::seasons::{CardBonus, CardSquare, SeasonTask, TrackedStat};

    use super::*;

    /// A made-up season: two cards of one task type, three card tasks, one journey task.
    const SEASON: u32 = 0x2221_5ad9;
    const KILL_STAT: u32 = 11;
    const CARD_STAT: u32 = 22;
    const GOAL: u32 = 10;

    fn catalog() -> SeasonCatalog {
        let squares = (0..CARD_SIDE * CARD_SIDE)
            .map(|_| CardSquare {
                kind: "Pve".to_owned(),
                experience: 50,
            })
            .collect::<Vec<_>>();
        let task = |id: &str, stat: u16, kind: &str| SeasonTask {
            id: id.to_owned(),
            crc: lowercase_crc32(id),
            stat,
            goal: GOAL,
            chapter: None,
            reward: None,
            kind: kind.to_owned(),
        };
        SeasonCatalog {
            season: "Season10".to_owned(),
            season_crc: SEASON,
            premium_entitlement: "Entitlement_SeasonPassPurchase_Season10".to_owned(),
            rewards: vec![
                SeasonReward {
                    id: "S10_BattlePass_Free_01".to_owned(),
                    crc: lowercase_crc32("S10_BattlePass_Free_01"),
                    index: 1,
                    kind: RewardKind::Free,
                    level: Some(1),
                    item: Some("Welcome_Box_S10".to_owned()),
                    experience: 0,
                    entitlements: None,
                },
                SeasonReward {
                    id: "S10_BattlePass_Premium_01".to_owned(),
                    crc: lowercase_crc32("S10_BattlePass_Premium_01"),
                    index: 101,
                    kind: RewardKind::Premium,
                    level: Some(1),
                    item: None,
                    experience: 100,
                    entitlements: None,
                },
                SeasonReward {
                    id: "S10_Journey_Ch01_T01".to_owned(),
                    crc: lowercase_crc32("S10_Journey_Ch01_T01"),
                    index: 201,
                    kind: RewardKind::Journey,
                    level: None,
                    item: None,
                    experience: 3_750,
                    entitlements: None,
                },
                SeasonReward {
                    id: "S10_BattlePass_Free_05".to_owned(),
                    crc: lowercase_crc32("S10_BattlePass_Free_05"),
                    index: 5,
                    kind: RewardKind::Free,
                    level: Some(5),
                    item: None,
                    experience: 0,
                    entitlements: Some("PremiumEmote_Firework1:2".to_owned()),
                },
            ],
            stats: vec![
                TrackedStat {
                    id: "KillLost".to_owned(),
                    crc: KILL_STAT,
                    kind: StatKind::Kill {
                        categories: vec!["Lost".to_owned()],
                        targets: Vec::new(),
                        level: 0,
                        weapons: Vec::new(),
                    },
                },
                TrackedStat {
                    id: "CompleteActivityCard".to_owned(),
                    crc: CARD_STAT,
                    kind: StatKind::CompleteActivityCard,
                },
            ],
            chapters: vec![nw_content::seasons::Chapter {
                id: "Season10Chapter1".to_owned(),
                challenge: false,
                tasks_needed: 1,
                reward: None,
            }],
            journey: vec![SeasonTask {
                chapter: Some(0),
                reward: Some(2),
                ..task("JourneyS10C1T5", 0, "")
            }],
            card_tasks: vec![
                task("ActivitiesS1Task3", 0, "Pve"),
                task("ActivitiesS1Task4", 0, "Pve"),
                task("ActivitiesS1Task5", 1, "Pve"),
            ],
            cards: vec![squares.clone(), squares],
            card_bonus: CardBonus {
                card: 150,
                line: 250,
                stamps_to_complete: 4,
            },
            tasks_per_card: 3,
            required_per_type: 1,
        }
    }

    fn arrived() -> SeasonPass {
        let mut dice = Dice::new(7);
        SeasonPass::arrive(&PlayerSeason::default(), &catalog(), 1_000, &mut dice)
    }

    /// A kill of a mob of `categories` (pass `Lost` for the made-up kill stat).
    fn kill_of(categories: &[String]) -> SeasonEvent<'_> {
        SeasonEvent::Kill {
            vitals_id: "Undead_Grunt",
            categories,
            level: 10,
            weapon: None,
        }
    }

    fn lost() -> Vec<String> {
        vec!["Lost".to_owned()]
    }

    #[test]
    fn a_new_season_deals_a_card_of_every_type_with_three_wild_stamps() {
        let pass = arrived();
        let card = &pass.state.card;
        assert_eq!(pass.state.season, SEASON);
        assert_eq!(pass.state.first_connect_ns, 1_000);
        assert_eq!(card.wild_stamps, FIRST_WILD_STAMPS);
        assert_eq!(card.tasks.len(), 3);
        let distinct: BTreeSet<u32> = card.tasks.iter().copied().collect();
        assert_eq!(distinct.len(), 3, "no task twice");
        // A later login keeps it.
        let mut dice = Dice::new(1);
        let again = SeasonPass::arrive(&pass.carried(), &catalog(), 2_000, &mut dice);
        assert_eq!(again.state, pass.state);
    }

    #[test]
    fn a_wild_stamp_then_a_full_row_give_the_retail_experience() {
        let catalog = catalog();
        let mut pass = arrived();
        // `Hive_Seasons_Rewards`: a stamp alone gives 50; one that fills a line 50 + 250 and a
        // wild stamp back.
        assert_eq!(pass.stamp(&catalog, [0, 0, 0, 1]), Ok(50));
        assert_eq!(pass.stamp(&catalog, [0, 0, 1, 1]), Ok(50));
        assert_eq!(pass.state.card.wild_stamps, 1);
        assert_eq!(pass.stamp(&catalog, [0, 0, 2, 1]), Ok(50));
        assert_eq!(
            pass.stamp(&catalog, [0, 0, 1, 1]),
            Err(SeasonRefusal::BadSquare)
        );
        assert_eq!(
            pass.stamp(&catalog, [0, 0, 3, 1]),
            Err(SeasonRefusal::NoWildStamp)
        );
        pass.state.card.wild_stamps = 1;
        assert_eq!(pass.stamp(&catalog, [0, 0, 3, 1]), Ok(300));
        assert_eq!(pass.state.card.stamped, 0b1111);
        assert_eq!(pass.state.card.wild_stamped, 0b1111);
        assert_eq!(pass.state.card.wild_stamps, 1);
        assert_eq!(
            pass.stamp(&catalog, [0, 4, 0, 1]),
            Err(SeasonRefusal::BadSquare)
        );
    }

    #[test]
    fn a_task_stamp_needs_its_stat_and_claims_the_task() {
        let lost = lost();
        let catalog = catalog();
        let mut pass = arrived();
        let place = pass
            .state
            .card
            .tasks
            .iter()
            .position(|task| catalog.card_task(*task).is_some_and(|task| task.stat == 0))
            .expect("a kill task");
        let square = [u8::try_from(place).expect("small"), 1, 0, 0];
        assert_eq!(
            pass.stamp(&catalog, square),
            Err(SeasonRefusal::TaskNotDone)
        );
        for _ in 0..GOAL {
            pass.count(&catalog, &kill_of(&lost));
        }
        assert_eq!(pass.stamp(&catalog, square), Ok(50));
        assert_eq!(pass.state.card.claimed, 1 << place);
        assert_eq!(pass.state.card.wild_stamped, 0, "no wild bit");
        let other = [u8::try_from(place).expect("small"), 1, 1, 0];
        assert_eq!(
            pass.stamp(&catalog, other),
            Err(SeasonRefusal::TaskNotDone),
            "claimed"
        );
        // The journey stat counted the same kills.
        assert_eq!(count_of(&pass.state.stats, KILL_STAT), GOAL);
    }

    #[test]
    fn a_new_card_needs_four_stamps_and_counts_the_card() {
        let catalog = catalog();
        let mut pass = arrived();
        let mut dice = Dice::new(3);
        assert_eq!(
            pass.new_card(&catalog, &mut dice),
            Err(SeasonRefusal::CardNotDone)
        );
        pass.state.card.stamped = 0b1111_0000;
        pass.state.card.claimed = 1;
        assert_eq!(pass.new_card(&catalog, &mut dice), Ok(150));
        assert_eq!(pass.state.card.cards_done, 1);
        assert_eq!((pass.state.card.stamped, pass.state.card.claimed), (0, 0));
        assert_eq!(pass.state.card.rerolls, 0);
    }

    #[test]
    fn a_reroll_keeps_the_claimed_tasks_twice_at_most() {
        let catalog = catalog();
        let mut pass = arrived();
        let mut dice = Dice::new(5);
        pass.state.card.claimed = 0b010;
        let claimed = pass.state.card.tasks[1];
        for _ in 0..MAX_REROLLS {
            assert_eq!(pass.reroll(&catalog, &mut dice), Ok(()));
            assert_eq!(pass.state.card.tasks[1], claimed);
            assert_eq!(pass.state.card.tasks.len(), 3);
        }
        assert_eq!(
            pass.reroll(&catalog, &mut dice),
            Err(SeasonRefusal::NoReroll)
        );
    }

    #[test]
    fn a_redeem_checks_the_level_the_task_and_the_premium() {
        let lost = lost();
        let catalog = catalog();
        let mut pass = arrived();
        let id = |name: &str| {
            u64::from(SEASON) << REWARD_SEASON_SHIFT | u64::from(lowercase_crc32(name))
        };
        let free = id("S10_BattlePass_Free_01");
        assert_eq!(
            pass.redeem(&catalog, free, 0, false, 1),
            Err(SeasonRefusal::NotReached)
        );
        let reward = pass.redeem(&catalog, free, 1, false, 1).expect("level 1");
        assert_eq!(reward.item.as_deref(), Some("Welcome_Box_S10"));
        // Retail `season_track_activity_card_and_get_rewards`: index 1 sets bit 1.
        assert_eq!(pass.state.redeemed, [0b10]);
        assert_eq!(
            pass.redeem(&catalog, free, 1, false, 1),
            Err(SeasonRefusal::UnknownReward)
        );
        let premium = id("S10_BattlePass_Premium_01");
        assert_eq!(
            pass.redeem(&catalog, premium, 1, false, 1),
            Err(SeasonRefusal::NotPremium)
        );
        assert!(pass.redeem(&catalog, premium, 1, true, 1).is_ok());
        assert_eq!(pass.state.redeemed, [0b10, 1 << (101 - 64)]);
        let journey = id("S10_Journey_Ch01_T01");
        assert_eq!(
            pass.redeem(&catalog, journey, 1, false, 1),
            Err(SeasonRefusal::NotReached)
        );
        for _ in 0..GOAL {
            pass.count(&catalog, &kill_of(&lost));
        }
        assert_eq!(
            pass.redeem(&catalog, journey, 1, false, 1)
                .map(|reward| reward.experience),
            Ok(3_750)
        );
        let emote = id("S10_BattlePass_Free_05");
        assert_eq!(
            pass.redeem(&catalog, emote, 5, false, 1),
            Err(SeasonRefusal::Entitlement)
        );
        let other_season = u64::from(lowercase_crc32("s10_battlepass_free_01"));
        assert_eq!(
            pass.redeem(&catalog, other_season, 1, false, 1),
            Err(SeasonRefusal::NoSeason)
        );
    }

    #[test]
    fn the_states_go_whole_then_as_changes() {
        let lost = lost();
        let catalog = catalog();
        let mut pass = arrived();
        let first = pass.take_members(&catalog, 3_173, 1, 1_000);
        assert_eq!(first.len(), 2, "both states whole");
        assert!(
            pass.take_members(&catalog, 3_173, 1, 1_000).is_empty(),
            "no change"
        );
        pass.state.card.stamped = 1;
        let change = pass.take_members(&catalog, 3_223, 1, 1_000);
        assert_eq!(change.len(), 1);
        assert_eq!(
            change[0].component,
            nw_messages::seasons::SEASONS_REWARDS_COMPONENT
        );
        pass.count(&catalog, &kill_of(&lost));
        let change = pass.take_members(&catalog, 3_223, 1, 1_000);
        assert_eq!(change.len(), 1);
        assert_eq!(
            change[0].component,
            nw_messages::seasons::SEASONS_TRACKED_STATS_COMPONENT
        );
        assert!(pass.state.versions.0 > FIRST_VERSION && pass.state.versions.1 > FIRST_VERSION);
    }

    /// The bit of the square in row 0 and column 0.
    const FIRST_SQUARE: u16 = 1;

    #[test]
    fn rows_and_columns_count_as_lines() {
        assert_eq!(lines_filled(0, 0b1111), 1);
        // Bit `row * 4 + column`: row 0 is full, rows 1 to 3 miss column 0; the 3 squares
        // fill rows 1 to 3 and column 0.
        assert_eq!(lines_filled(0b1110_1110_1110_1111, u16::MAX), 4);
        // The one missing square of row 0 and column 0 fills both.
        assert_eq!(lines_filled(!FIRST_SQUARE, u16::MAX), 2);
        assert_eq!(lines_filled(u16::MAX, u16::MAX), 0);
        assert_eq!(claim_words(1 << 14), [0x4000]);
        assert_eq!(claim_words(1 << 14 | 1 << 16), [0x4000, 1]);
        assert_eq!(claim_words(0), Vec::<u16>::new());
        assert_eq!(next_midnight_ns(1), NS_PER_DAY);
    }

    #[test]
    fn a_stat_counts_only_its_events() {
        let lost = vec!["Lost".to_owned()];
        let kill = StatKind::Kill {
            categories: lost.clone(),
            targets: Vec::new(),
            level: 60,
            weapons: vec!["SwordAbilityTable".to_owned()],
        };
        let event = |level, weapon| SeasonEvent::Kill {
            vitals_id: "Undead_Grunt",
            categories: &lost,
            level,
            weapon,
        };
        assert_eq!(counted(&kill, &event(60, Some("swordabilitytable"))), 1);
        assert_eq!(counted(&kill, &event(59, Some("SwordAbilityTable"))), 0);
        assert_eq!(counted(&kill, &event(60, None)), 0);
        let gather = StatKind::Gather {
            gatherables: Vec::new(),
            tradeskills: vec!["Mining".to_owned()],
            per_use: false,
        };
        let mined = SeasonEvent::Gathered {
            gatherable: "IronVeinSmall",
            tradeskill: Some("Mining"),
            items: 4,
        };
        assert_eq!(counted(&gather, &mined), 4);
        let points = StatKind::CategoricalProgression {
            progressions: vec!["UmbralCurrency".to_owned()],
            ranks: false,
        };
        let gained = SeasonEvent::Progression {
            id: "UmbralCurrency",
            points: 30,
            ranks: 0,
        };
        assert_eq!(counted(&points, &gained), 30);
        assert_eq!(counted(&StatKind::Unread, &gained), 0);
        let missions = StatKind::CompleteQuest {
            missions_only: true,
        };
        assert_eq!(
            counted(&missions, &SeasonEvent::QuestDone { mission: false }),
            0
        );
        assert_eq!(
            counted(&missions, &SeasonEvent::QuestDone { mission: true }),
            1
        );
    }

    #[test]
    fn a_game_event_stat_counts_its_events_once_each() {
        // Content bundle 25810808: `WinCTF` counts `CaptureTheFlag_T4` only.
        let win = StatKind::GameEvent {
            events: vec!["CaptureTheFlag_T4".to_owned()],
        };
        let won = SeasonEvent::GameEvent {
            id: "capturetheflag_t4",
        };
        let lost = SeasonEvent::GameEvent {
            id: "CaptureTheFlag_T2",
        };
        assert_eq!(counted(&win, &won), 1);
        assert_eq!(counted(&win, &lost), 0);
        assert_eq!(counted(&win, &SeasonEvent::CardDone), 0);
    }
}
