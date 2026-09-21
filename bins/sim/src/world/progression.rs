// SPDX-License-Identifier: AGPL-3.0-or-later
//! A player's weapon masteries as its client keeps them (docs/re/2026-09-18-progression-and-gm-
//! mode.md sections 2 and 4): the categorical progression list (type 911) and the weapon point
//! pools (type 3681). The sim keeps the total experience of each mastery; the rank and the
//! points inside it come from the rank table. Each rank gives one unspent point of the weapon's
//! pool.

use std::collections::{BTreeMap, BTreeSet};

use nw_bus::AdminReply;
use nw_content::progression::{Mastery, Progression};
use nw_messages::items::FIRST_VERSION;
use nw_messages::{
    CategoricalProgressionState, MasteryChange, MasteryEntry, MasteryFields, PointPool,
    PoolCategory, ProgressionPointState,
};

use super::{Player, SimWorld};

/// The most coins a save keeps (the database column is a signed 64-bit number).
const MAX_COINS: u64 = i64::MAX.unsigned_abs();
/// The answer for a level or mastery command when the content has no level table.
const NO_LEVELS: &str = "the content has no level table";

/// The weapon masteries of one player, in the order its client lists them.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Masteries {
    /// Each mastery id and its total experience, in list order.
    entries: Vec<(String, u32)>,
    /// Saved masteries the content does not know: kept as they are, so a content change never
    /// deletes them at the next save.
    kept: BTreeMap<String, u32>,
    /// The list positions that changed since the last bundle, and the fields that did.
    changed: BTreeMap<usize, MasteryFields>,
    /// The list positions whose rank changed since the last bundle.
    ranked: BTreeSet<usize>,
    /// The version of the newest 911 list the client has.
    version: u64,
    /// The version of the newest 3681 pool list the client has.
    pools_version: u64,
}

impl Masteries {
    /// The masteries of `saved` (id to total experience): the ones `progression` knows go to
    /// the client; another id is kept unsent, with a log line. The client's lists go on above
    /// `versions` (the 911 and 3681 versions it got from the sim before; 0 for none): retail's
    /// list versions only rise.
    pub(super) fn of(
        saved: &BTreeMap<String, u32>,
        progression: &Progression,
        versions: (u64, u64),
    ) -> Self {
        let mut entries = Vec::new();
        let mut kept = BTreeMap::new();
        for (id, experience) in saved {
            match progression.mastery(id) {
                Some(mastery) => {
                    entries.push((
                        mastery.id.clone(),
                        (*experience).min(mastery.max_experience()),
                    ));
                }
                None => {
                    tracing::warn!(mastery = %id, "unknown weapon mastery kept unsent");
                    kept.insert(id.clone(), *experience);
                }
            }
        }
        Self {
            entries,
            kept,
            version: versions.0,
            pools_version: versions.1,
            ..Self::default()
        }
    }

    /// Each mastery id and its total experience, as a save or a handoff carries them.
    pub(super) fn carried(&self) -> BTreeMap<String, u32> {
        self.kept
            .iter()
            .map(|(id, experience)| (id.clone(), *experience))
            .chain(self.entries.iter().cloned())
            .collect()
    }

    /// The versions of the newest 911 and 3681 lists the client has, for the next sim.
    pub(super) fn versions(&self) -> (u64, u64) {
        (self.version, self.pools_version)
    }

    /// The total experience of `mastery`.
    pub(super) fn experience(&self, mastery: &Mastery) -> u32 {
        self.position(mastery)
            .map_or(0, |index| self.entries[index].1)
    }

    /// Sets the total experience of `mastery` to `experience`, at most its top rank's.
    pub(super) fn set(&mut self, mastery: &Mastery, experience: u32) {
        let experience = experience.min(mastery.max_experience());
        let index = match self.position(mastery) {
            Some(index) => index,
            None => {
                self.entries.push((mastery.id.clone(), 0));
                self.changed
                    .insert(self.entries.len() - 1, MasteryFields::ALL);
                self.entries.len() - 1
            }
        };
        let before = self.entries[index].1;
        if before == experience {
            return;
        }
        self.entries[index].1 = experience;
        let rank_changed = mastery.rank_of(before).0 != mastery.rank_of(experience).0;
        if rank_changed {
            self.ranked.insert(index);
        }
        let fields = if rank_changed {
            MasteryFields::RANK_AND_POINTS
        } else {
            MasteryFields::POINTS
        };
        self.changed
            .entry(index)
            .and_modify(|known| {
                known.rank |= fields.rank;
                known.points |= fields.points;
            })
            .or_insert(fields);
    }

    /// The whole lists for the first card: the masteries and their point pools.
    pub(super) fn first_states(
        &mut self,
        progression: &Progression,
    ) -> (CategoricalProgressionState, ProgressionPointState) {
        self.changed.clear();
        self.ranked.clear();
        self.version = next_full_version(self.version);
        self.pools_version = next_full_version(self.pools_version);
        let entries: Vec<MasteryEntry> = self.wire_entries(progression).collect();
        (
            CategoricalProgressionState::full(self.version, &entries),
            ProgressionPointState::full(self.pools_version, &self.pools(progression), &[]),
        )
    }

    /// The changes since the last bundle: the 911 changes, and the whole pool list when a rank
    /// changed; `None` without a change.
    pub(super) fn take_change(
        &mut self,
        progression: &Progression,
    ) -> Option<(CategoricalProgressionState, Option<ProgressionPointState>)> {
        if self.changed.is_empty() {
            return None;
        }
        self.version += 1;
        let changes: Vec<MasteryChange> = std::mem::take(&mut self.changed)
            .into_iter()
            .filter_map(|(index, fields)| {
                Some(MasteryChange {
                    index: u32::try_from(index).ok()?,
                    entry: self.wire_entry(progression, index)?,
                    fields,
                })
            })
            .collect();
        let pools = (!std::mem::take(&mut self.ranked).is_empty()).then(|| {
            self.pools_version += 1;
            ProgressionPointState::pools(self.pools_version, &self.pools(progression))
        });
        Some((
            CategoricalProgressionState::changes(self.version, &changes),
            pools,
        ))
    }

    fn position(&self, mastery: &Mastery) -> Option<usize> {
        self.entries
            .iter()
            .position(|(id, _)| id.eq_ignore_ascii_case(&mastery.id))
    }

    fn wire_entry(&self, progression: &Progression, index: usize) -> Option<MasteryEntry> {
        let (id, experience) = self.entries.get(index)?;
        let mastery = progression.mastery(id)?;
        let (rank, points) = mastery.rank_of(*experience);
        Some(MasteryEntry {
            progression_id: mastery.crc,
            rank,
            points,
        })
    }

    fn wire_entries<'a>(
        &'a self,
        progression: &'a Progression,
    ) -> impl Iterator<Item = MasteryEntry> + 'a {
        (0..self.entries.len()).filter_map(|index| self.wire_entry(progression, index))
    }

    /// One weapon point pool per mastery with a rank: one unspent point per rank (spending is
    /// not taken yet, so every point stays unspent).
    fn pools(&self, progression: &Progression) -> Vec<PointPool> {
        self.wire_entries(progression)
            .filter(|entry| entry.rank > 0)
            .map(|entry| PointPool {
                pool: entry.progression_id,
                count: entry.rank,
                category: PoolCategory::Weapon,
            })
            .collect()
    }
}

/// The version of a full list after `last`: the first one, or the one above `last`.
fn next_full_version(last: u64) -> u64 {
    if last < FIRST_VERSION {
        FIRST_VERSION
    } else {
        last.saturating_add(1)
    }
}

impl SimWorld {
    /// Adds `amount` experience points to the player `entity`, at most the top level's start;
    /// the next own state sends them and a new level.
    pub(super) fn give_experience(&mut self, entity: u64, amount: u32) -> AdminReply {
        let top = self
            .content
            .progression
            .experience_for_level(self.content.progression.max_level())
            .unwrap_or(u32::MAX);
        self.with_player(entity, |player| {
            player.experience = player.experience.saturating_add(amount).min(top);
            player.experience_changed = true;
            AdminReply::Done
        })
    }

    /// Sets the player `entity` at the start of `level` (zero-based), at most the top level.
    pub(super) fn set_level(&mut self, entity: u64, level: u32) -> AdminReply {
        let progression = &self.content.progression;
        let Some(experience) = progression.experience_for_level(level.min(progression.max_level()))
        else {
            return AdminReply::Failed {
                reason: NO_LEVELS.to_owned(),
            };
        };
        self.with_player(entity, |player| {
            player.experience = experience;
            player.experience_changed = true;
            AdminReply::Done
        })
    }

    /// Adds `amount` experience to the weapon mastery `id` of the player `entity`.
    pub(super) fn give_mastery_experience(
        &mut self,
        entity: u64,
        id: &str,
        amount: u32,
    ) -> AdminReply {
        let Some(mastery) = self.content.progression.mastery(id).cloned() else {
            return unknown_mastery(id);
        };
        self.with_player(entity, |player| {
            let experience = player.masteries.experience(&mastery).saturating_add(amount);
            player.masteries.set(&mastery, experience);
            AdminReply::Done
        })
    }

    /// Sets the weapon mastery `id` of the player `entity` (every one for `None`) at the start
    /// of `rank` (zero-based), at most the top rank.
    pub(super) fn set_mastery_rank(
        &mut self,
        entity: u64,
        id: Option<&str>,
        rank: u16,
    ) -> AdminReply {
        let masteries: Vec<Mastery> = match id {
            Some(id) => match self.content.progression.mastery(id) {
                Some(mastery) => vec![mastery.clone()],
                None => return unknown_mastery(id),
            },
            None => self.content.progression.masteries().to_vec(),
        };
        if masteries.is_empty() {
            return AdminReply::Failed {
                reason: "the content has no weapon masteries".to_owned(),
            };
        }
        self.with_player(entity, |player| {
            for mastery in &masteries {
                player
                    .masteries
                    .set(mastery, mastery.experience_for_rank(rank));
            }
            AdminReply::Done
        })
    }

    /// Sets the coins of the player `entity`, at most what a save keeps.
    pub(super) fn set_coins(&mut self, entity: u64, coins: u64) -> AdminReply {
        self.with_player(entity, |player| {
            player.coins = coins.min(MAX_COINS);
            player.coins_changed = true;
            AdminReply::Done
        })
    }

    /// Runs `change` on the player `entity`; [`AdminReply::NotMine`] when it is not here.
    fn with_player(
        &mut self,
        entity: u64,
        change: impl FnOnce(&mut Player) -> AdminReply,
    ) -> AdminReply {
        let Some(target) = self.player_by_number(entity) else {
            return AdminReply::NotMine;
        };
        match self.world.get_mut::<Player>(target) {
            Some(mut player) => change(&mut player),
            None => AdminReply::NotMine,
        }
    }
}

fn unknown_mastery(id: &str) -> AdminReply {
    AdminReply::Failed {
        reason: format!("no weapon mastery {id}"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nw_bus::{AdminAction, AdminCommand};
    use uuid::Uuid;

    use super::*;
    use crate::player_content::PlayerContent;
    use crate::world::tests::{NOW, SERVER_TIME, admit_at, world_with_spawns};

    const PLAYER: u64 = 41;

    const SWORD: &str = "GreatSwordAbilityTable";
    const BUSS: &str = "BlunderbussAbilityTable";
    const RANK_CAPS: [i64; 3] = [10, 50, 70];
    const TOP_RANK: i64 = 3;

    fn progression() -> Progression {
        Progression::build(
            [(0, 0), (1, 150)],
            (0..).zip(RANK_CAPS),
            [(SWORD, TOP_RANK), (BUSS, TOP_RANK)],
        )
    }

    #[test]
    fn a_first_kill_adds_entries_and_a_rank_up_sends_the_pools() {
        let progression = progression();
        let sword = progression.mastery(SWORD).expect("sword").clone();
        let buss = progression.mastery(BUSS).expect("buss").clone();
        let mut masteries = Masteries::of(&BTreeMap::new(), &progression, (0, 0));
        let (first, pools) = masteries.first_states(&progression);
        assert_eq!(first, CategoricalProgressionState::full(FIRST_VERSION, &[]));
        assert_eq!(pools, ProgressionPointState::full(FIRST_VERSION, &[], &[]));
        assert!(masteries.take_change(&progression).is_none());
        // Retail first kill: greatsword +20 (rank 1, 10 points), blunderbuss +4.
        masteries.set(&sword, 20);
        masteries.set(&buss, 4);
        let (change, pools) = masteries.take_change(&progression).expect("change");
        let entry = |crc, rank, points| MasteryEntry {
            progression_id: crc,
            rank,
            points,
        };
        assert_eq!(
            change,
            CategoricalProgressionState::changes(
                FIRST_VERSION + 1,
                &[
                    MasteryChange {
                        index: 0,
                        entry: entry(sword.crc, 1, 10),
                        fields: MasteryFields::ALL,
                    },
                    MasteryChange {
                        index: 1,
                        entry: entry(buss.crc, 0, 4),
                        fields: MasteryFields::ALL,
                    },
                ]
            )
        );
        let pool = PointPool {
            pool: sword.crc,
            count: 1,
            category: PoolCategory::Weapon,
        };
        assert_eq!(
            pools,
            Some(ProgressionPointState::pools(FIRST_VERSION + 1, &[pool]))
        );
        // Points alone: no pool list.
        masteries.set(&buss, 5);
        let (change, pools) = masteries.take_change(&progression).expect("change");
        assert_eq!(
            change,
            CategoricalProgressionState::changes(
                FIRST_VERSION + 2,
                &[MasteryChange {
                    index: 1,
                    entry: entry(buss.crc, 0, 5),
                    fields: MasteryFields::POINTS,
                }]
            )
        );
        assert_eq!(pools, None);
    }

    #[test]
    fn a_handed_off_player_gets_lists_above_the_versions_it_had() {
        let progression = progression();
        let sword = progression.mastery(SWORD).expect("sword").clone();
        let carried = BTreeMap::from([(SWORD.to_owned(), 20)]);
        let mut masteries = Masteries::of(&carried, &progression, (12, 7));
        let (first, pools) = masteries.first_states(&progression);
        let entry = MasteryEntry {
            progression_id: sword.crc,
            rank: 1,
            points: 10,
        };
        assert_eq!(first, CategoricalProgressionState::full(13, &[entry]));
        let pool = PointPool {
            pool: sword.crc,
            count: 1,
            category: PoolCategory::Weapon,
        };
        assert_eq!(pools, ProgressionPointState::full(8, &[pool], &[]));
        assert_eq!(masteries.versions(), (13, 8));
    }

    #[test]
    fn experience_stops_at_the_top_rank_and_unknown_ids_are_kept() {
        let progression = progression();
        let sword = progression.mastery(SWORD).expect("sword").clone();
        let saved = BTreeMap::from([(SWORD.to_owned(), u32::MAX), ("Cooking".to_owned(), 7)]);
        let mut masteries = Masteries::of(&saved, &progression, (0, 0));
        assert_eq!(masteries.experience(&sword), sword.max_experience());
        assert_eq!(
            masteries.carried(),
            BTreeMap::from([
                (SWORD.to_owned(), sword.max_experience()),
                ("Cooking".to_owned(), 7),
            ]),
            "an id the content does not know is kept for the save"
        );
        masteries.set(&sword, u32::MAX);
        assert!(
            masteries.take_change(&progression).is_none(),
            "no change at the top"
        );
        let (first, _) = masteries.first_states(&progression);
        assert_eq!(
            first,
            CategoricalProgressionState::full(
                FIRST_VERSION,
                &[MasteryEntry {
                    progression_id: sword.crc,
                    rank: 3,
                    points: 0,
                }]
            )
        );
    }

    fn command(action: AdminAction) -> AdminCommand {
        AdminCommand {
            request: Uuid::from_u128(1),
            actor: None,
            action,
        }
    }

    /// A world with the test progression and one player, outside the tutorial.
    fn admitted() -> SimWorld {
        let mut world = world_with_spawns(Vec::new());
        world.content = Arc::new(PlayerContent {
            progression: progression(),
            ..PlayerContent::default()
        });
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
        world
    }

    fn saved_progress(world: &mut SimWorld) -> Option<nw_bus::PlayerProgression> {
        world
            .positions()
            .into_iter()
            .find_map(|departure| departure.progression)
    }

    #[test]
    fn gm_commands_change_the_progress_and_it_is_saved_once() {
        let mut world = admitted();
        assert_eq!(saved_progress(&mut world), None, "nothing changed");
        let run = |world: &mut SimWorld, action| world.admin(&command(action), NOW);
        assert_eq!(
            run(
                &mut world,
                AdminAction::SetLevel {
                    entity: PLAYER,
                    level: 9
                }
            ),
            AdminReply::Done
        );
        assert_eq!(
            run(
                &mut world,
                AdminAction::GiveMasteryExperience {
                    entity: PLAYER,
                    mastery: SWORD.to_lowercase(),
                    amount: 20,
                }
            ),
            AdminReply::Done
        );
        assert_eq!(
            run(
                &mut world,
                AdminAction::SetCoins {
                    entity: PLAYER,
                    coins: 77
                }
            ),
            AdminReply::Done
        );
        let progress = saved_progress(&mut world).expect("a save");
        // Level 9 is above the top level (1): the top level's start.
        assert_eq!(progress.experience, 150);
        assert_eq!(progress.coins, 77);
        assert_eq!(progress.masteries, BTreeMap::from([(SWORD.to_owned(), 20)]));
        assert_eq!(saved_progress(&mut world), None, "saved once");
        assert_eq!(
            run(
                &mut world,
                AdminAction::SetMasteryRank {
                    entity: PLAYER,
                    mastery: None,
                    rank: u16::MAX,
                }
            ),
            AdminReply::Done
        );
        let top = progression()
            .mastery(SWORD)
            .expect("sword")
            .max_experience();
        let progress = saved_progress(&mut world).expect("a save");
        assert_eq!(
            progress.masteries,
            BTreeMap::from([(SWORD.to_owned(), top), (BUSS.to_owned(), top)])
        );
        assert_eq!(
            run(
                &mut world,
                AdminAction::GiveExperience {
                    entity: PLAYER,
                    amount: u32::MAX
                }
            ),
            AdminReply::Done
        );
        assert_eq!(
            saved_progress(&mut world),
            None,
            "experience stops at the top level's start"
        );
    }

    #[test]
    fn an_unknown_mastery_fails_and_an_absent_player_is_not_mine() {
        let mut world = admitted();
        assert!(matches!(
            world.admin(
                &command(AdminAction::GiveMasteryExperience {
                    entity: PLAYER,
                    mastery: "Cooking".to_owned(),
                    amount: 1,
                }),
                NOW
            ),
            AdminReply::Failed { .. }
        ));
        assert_eq!(
            world.admin(
                &command(AdminAction::GiveExperience {
                    entity: PLAYER + 1,
                    amount: 1,
                }),
                NOW
            ),
            AdminReply::NotMine
        );
    }
}
