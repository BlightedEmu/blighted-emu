// SPDX-License-Identifier: AGPL-3.0-or-later
//! The war board of an Outpost Rush match: the manifests 3530 and 2968 and the stats 839 of two
//! team lists ([`nw_messages::warboard_stats::TeamsWarboardStats`]), with the stats of
//! `ORWarboardStatDefinitions` ([`nw_content::warboard_stats`]).
//!
//! Observed (all 12 retail Outpost Rush ledgers of `local-data/capture-index.tsv`, 13,129 board
//! pushes; seconds from each ledger's first record):
//!
//! - The header is the game mode entity's facet entity and that entity plus `61773d1b8e3ae07e`
//!   (the three maps' headers `cc1ccb66..`, `1ca08e46..`, `b98f65ef..`; the entity is the
//!   `ActiveGameMode` facet of the player's 3312, `OPR---ISLAND` 2.0 s).
//! - Each team has its own list and its own indexes, in join order from 0. The manifest's first
//!   list is team 0 and the 839's first list is team 1 (`opr_que_and_lost`, a team 0 player:
//!   its own stats in the 839's second list 101 times; `OPR_TEST3`, team 1: in the first list
//!   228 times; a joiner of the manifest's list `m` is named in the 839's list `1 - m` in 46 of
//!   the 52 joins, the rest index clashes).
//! - A player's join raises the sync by one and gives the others a 2968 of the joiner alone,
//!   then an 839 that names the joiner with no slot (`opr_que_and_lost` 582.364 and 582.598 s).
//!   The joiner gets the full manifest and an 839 with every participant and every in-game
//!   slot, zeros included, its own part empty (577.293 s). The full pair goes out again about
//!   every 150 s (gaps of 150.0 to 157.8 s, `opr_que_and_lost` 625.407 to 2015.952 s,
//!   `opr_match_4` 0.510 to 1147.375 s), and each client 3206 gets it within 50 ms (32 of 32).
//! - The 839 goes about every second (gaps of 1.0 s in 9,867 of 10,319; 2 s when nothing
//!   changed): the receiver's own in-game slots that changed, and each participant whose
//!   in-game slots changed with those slots, from the highest index in each list.
//! - The in-game slots are the table's `InGameStat` rows: `Score`, `TotalDamageDealt`,
//!   `Deaths`, `HealingDoneToAllies`, `DamageBlocked`, `PlayerKills`, `PlayerAssists`,
//!   `AITakedowns` and `TotalResourcesDeposited` (every list slot of every ledger). The summing
//!   stats sum their rows (`AITakedowns` = `AIKills` + `AIAssists`: `OPR-Coral-Divide-2` 5 + 6 =
//!   11; `TotalResourcesDeposited` = the four deposits: `opr_match_4` 90 + 78 + 60 = 228;
//!   `TotalDamageDealt` = the damage rows; `HealingDone` = self + allies).
//! - At the match's end, just before the ended state, each player's own part also carries its
//!   end-game slots that are not 0 (`opr_que_and_lost` 2105.777 s, 6 ms before the end;
//!   `OPR_TEST3` 1851.419 s; 11 of 11 ledgers that saw the end); no push follows.
//! - A participant who leaves is named once with the left bit, and the full pushes keep it with
//!   the bit (`opr_que_and_lost` 1254.911 s; 778.130 s `1L`).
//! - The board score ([`nw_content::warboard_stats`]) rises in lumps of 38 to 39 points
//!   (`CapturePointContests` 0.42 each: 90 to 93) about every 3 s while the player stands in an
//!   outpost (`opr_que_and_lost` 718.230 to 745.830 s, through the outpost's capture at
//!   734.177 s and after it).
//!
//! Inferred: the contests are 30 a second of a living participant inside an outpost's capture
//! area, counted at each score tick; the range bonus of a stat (`BonusDistanceRange` 14 m)
//! counts when the player stands within that range of an outpost's capture point; an assist is
//! a hit on the dead player or creature by another participant in the last
//! [`ASSIST_WINDOW_MS`]; the joiner's own board starts at 0 and a leaver's values stay on it.

use std::collections::HashMap;

use bevy_ecs::entity::Entity;
use nw_content::warboard_stats::{
    self as stat, BoardStats, SCORE, STAT_SLOTS,
};
use nw_messages::facet::FacetHeader;
use nw_messages::social::CharacterRef;
use nw_messages::warboard_stats::{
    BOARD_LISTS, ParticipantStats, TeamsManifest, TeamsWarboardStats,
};
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{ServerMessage, WarServerMsg};
use nw_sim_core::Position;

use super::super::{Body, SimWorld};
use super::{CAPTURE_RADIUS_M, ENDED, Outpost, SCORE_TICK_MS};
use crate::world::game_mode_entity::mode_facet;

/// The component of the board on the game mode entity (Observed, module text).
const BOARD_COMPONENT: u64 = 0x6177_3d1b_8e3a_e07e;
/// The 839 cadence, ms (Observed).
const PUSH_MS: u64 = 1_000;
/// The full resend, ms (Inferred from gaps of 150 to 158 s).
const FULL_RESEND_MS: u64 = 150_000;
/// How long a hit counts for an assist, ms (Inferred).
pub(super) const ASSIST_WINDOW_MS: u64 = 10_000;
/// The capture point contests of one second in an outpost (Inferred, module text).
const CONTESTS_PER_SECOND: u64 = 30;
const MS_PER_S: u64 = 1_000;
/// The contests of one score tick.
pub(super) const CONTESTS_PER_SCORE_TICK: u32 = (CONTESTS_PER_SECOND * SCORE_TICK_MS / MS_PER_S) as u32;
const SLOTS: usize = STAT_SLOTS as usize;
/// The resource items of the match and their deposit and gather stats (Observed: the items of
/// the loot tables `OutpostRush_Tree*`, `_OreVein*`, `_Wolf*` and the azoth grant).
const RESOURCE_STATS: [(&str, u8, u8); 4] = [
    ("OutpostRushWood", stat::INFUSED_WOOD_DEPOSITED, stat::INFUSED_WOOD_GATHERED),
    ("OutpostRushOre", stat::INFUSED_ORE_DEPOSITED, stat::INFUSED_ORE_GATHERED),
    ("OutpostRushRawhide", stat::INFUSED_HIDE_DEPOSITED, stat::INFUSED_HIDE_GATHERED),
    ("OutpostRushAzothEssence", stat::AZOTH_ESSENCE_DEPOSITED, stat::AZOTH_ESSENCE_GATHERED),
];

/// The 839 list of team `team` (Observed: team 1 first).
fn stats_list(team: u8) -> usize {
    usize::from(1 - team.min(1))
}

/// The manifest list of team `team` (Observed: team 0 first).
fn manifest_list(team: u8) -> usize {
    usize::from(team.min(1))
}

/// The board header of the game mode entity `gde_ref`.
pub(super) fn board_header(gde_ref: &[u8; RAW_ID_LEN]) -> FacetHeader {
    let entity = mode_facet(gde_ref);
    FacetHeader {
        target: entity.wrapping_add(BOARD_COMPONENT),
        entity,
    }
}

/// One participant on the board.
#[derive(Debug, Clone, PartialEq)]
struct Member {
    number: u64,
    entity: Entity,
    character: [u8; RAW_ID_LEN],
    team: u8,
    index: u8,
    values: [u32; SLOTS],
    /// The exact score; slot 0 holds it rounded down.
    score: f64,
    /// The in-game slots as last pushed in the lists, and in its own part.
    listed: [u32; SLOTS],
    own: [u32; SLOTS],
    /// It was named in the lists since its join.
    named: bool,
    left: bool,
    left_sent: bool,
}

/// The pushes for each player: its entity and the messages.
pub(super) type BoardPushes = Vec<(Entity, Vec<WarServerMsg>)>;

/// The board of one match.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Board {
    header: FacetHeader,
    members: Vec<Member>,
    sync: u16,
    /// The next 839 and the next full resend, server ms (0: no member yet).
    next_push_ms: u64,
    next_full_ms: u64,
    /// The hits of the participants on each player or creature: attacker and server ms.
    hits: HashMap<u64, Vec<(u64, u64)>>,
    /// The end push went out.
    closed: bool,
}

impl Board {
    /// The empty board of the game mode entity `gde_ref`.
    pub(super) fn new(gde_ref: &[u8; RAW_ID_LEN]) -> Self {
        Self {
            header: board_header(gde_ref),
            members: Vec::new(),
            sync: 0,
            next_push_ms: 0,
            next_full_ms: 0,
            hits: HashMap::new(),
            closed: false,
        }
    }

    fn member(&self, number: u64) -> Option<&Member> {
        self.members.iter().find(|member| member.number == number)
    }

    fn member_mut(&mut self, number: u64) -> Option<&mut Member> {
        self.members
            .iter_mut()
            .find(|member| member.number == number && !member.left)
    }

    /// The value of `slot` of player `number`.
    pub(super) fn value(&self, number: u64, slot: u8) -> u32 {
        self.member(number)
            .and_then(|member| member.values.get(usize::from(slot)).copied())
            .unwrap_or_default()
    }

    /// The board score of player `number`.
    pub(super) fn score_of(&self, number: u64) -> u32 {
        self.value(number, SCORE)
    }

    fn receivers(&self) -> impl Iterator<Item = &Member> {
        self.members.iter().filter(|member| !member.left)
    }

    fn manifest_push(&self, manifest: &TeamsManifest, full: bool) -> Option<WarServerMsg> {
        match manifest.encode_payload() {
            Ok(payload) if full => Some(WarServerMsg::UpdateFullWarboardManifest {
                header: self.header,
                payload,
            }),
            Ok(payload) => Some(WarServerMsg::UpdateWarboardManifest {
                header: self.header,
                payload,
            }),
            Err(error) => {
                tracing::warn!(%error, "Outpost Rush board manifest not sent");
                None
            }
        }
    }

    fn stats_push(&self, stats: &TeamsWarboardStats) -> Option<WarServerMsg> {
        match stats.encode_payload() {
            Ok(payload) => Some(WarServerMsg::UpdateWarboardStats {
                header: self.header,
                payload,
            }),
            Err(error) => {
                tracing::warn!(%error, "Outpost Rush board stats not sent");
                None
            }
        }
    }

    /// The full manifest: each team's members from index 0.
    fn manifest(&self) -> TeamsManifest {
        let mut lists: [Vec<(CharacterRef, u8)>; BOARD_LISTS] = Default::default();
        for member in &self.members {
            lists[manifest_list(member.team)].push((CharacterRef(Some(member.character)), member.index));
        }
        TeamsManifest {
            sync: self.sync,
            lists,
        }
    }

    /// The 839 lists of `entries` (team, index, stats), from the highest index in each list.
    fn lists(
        mut entries: Vec<(u8, u8, ParticipantStats)>,
    ) -> [Vec<(u8, ParticipantStats)>; BOARD_LISTS] {
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        let mut lists: [Vec<(u8, ParticipantStats)>; BOARD_LISTS] = Default::default();
        for (team, index, stats) in entries {
            lists[stats_list(team)].push((index, stats));
        }
        lists
    }

    /// The full 839 of the receiver of index `local_index`: every member's in-game slots.
    fn full_stats(&self, local_index: u8, rules: &BoardStats) -> TeamsWarboardStats {
        let entries = self
            .members
            .iter()
            .map(|member| {
                let stats = ParticipantStats {
                    left: member.left,
                    slots: rules
                        .in_game_slots()
                        .map(|slot| (slot, member.values[usize::from(slot)]))
                        .collect(),
                };
                (member.team, member.index, stats)
            })
            .collect();
        TeamsWarboardStats {
            sync: self.sync,
            local_index,
            local: ParticipantStats::default(),
            lists: Self::lists(entries),
        }
    }

    /// The full manifest and 839 of each receiver, and every member counts as pushed.
    fn full_pushes(&mut self, rules: &BoardStats) -> BoardPushes {
        let manifest = self.manifest_push(&self.manifest(), true);
        let pushes = self
            .receivers()
            .map(|member| {
                let stats = self.stats_push(&self.full_stats(member.index, rules));
                (member.entity, manifest.iter().cloned().chain(stats).collect())
            })
            .collect();
        for member in &mut self.members {
            member.listed = member.values;
            member.named = true;
            member.left_sent = member.left;
        }
        pushes
    }

    /// The full pair for player `number` alone (its 3206); `None` when it is not on the board.
    pub(super) fn full_for(&self, number: u64, rules: &BoardStats) -> Option<(Entity, Vec<WarServerMsg>)> {
        let member = self.member(number).filter(|member| !member.left)?;
        let manifest = self.manifest_push(&self.manifest(), true);
        let stats = self.stats_push(&self.full_stats(member.index, rules));
        Some((member.entity, manifest.into_iter().chain(stats).collect()))
    }

    /// Player `number` (`entity`, `character`) of `team` joins at `now_ms`: the joiner gets the
    /// full pair, the others the 2968 of the joiner.
    pub(super) fn join(
        &mut self,
        number: u64,
        entity: Entity,
        character: [u8; RAW_ID_LEN],
        team: u8,
        now_ms: u64,
        rules: &BoardStats,
    ) -> BoardPushes {
        if self.closed || self.member(number).is_some() {
            return Vec::new();
        }
        let index = self.members.iter().filter(|member| member.team == team).count();
        self.members.push(Member {
            number,
            entity,
            character,
            team,
            index: u8::try_from(index).unwrap_or(u8::MAX),
            values: [0; SLOTS],
            score: 0.0,
            listed: [0; SLOTS],
            own: [0; SLOTS],
            named: false,
            left: false,
            left_sent: false,
        });
        self.sync = self.sync.wrapping_add(1);
        if self.next_push_ms == 0 {
            self.next_push_ms = now_ms.saturating_add(PUSH_MS);
            self.next_full_ms = now_ms.saturating_add(FULL_RESEND_MS);
        }
        let mut lists: [Vec<(CharacterRef, u8)>; BOARD_LISTS] = Default::default();
        lists[manifest_list(team)].push((CharacterRef(Some(character)), u8::try_from(index).unwrap_or(u8::MAX)));
        let update = self.manifest_push(
            &TeamsManifest {
                sync: self.sync,
                lists,
            },
            false,
        );
        let mut pushes: BoardPushes = self
            .receivers()
            .filter(|member| member.number != number)
            .map(|member| (member.entity, update.iter().cloned().collect()))
            .collect();
        if let Some(pair) = self.full_for(number, rules) {
            pushes.push(pair);
        }
        pushes
    }

    /// Player `number` left the board.
    pub(super) fn leave(&mut self, number: u64) {
        if let Some(member) = self.member_mut(number) {
            member.left = true;
        }
    }

    /// `amount` of the stat `slot` for player `number`, `in_range` of its bonus or not: the
    /// slot, its sums and the score rise. A slot the table lacks counts nothing.
    pub(super) fn note(&mut self, number: u64, slot: u8, amount: u32, in_range: bool, rules: &BoardStats) {
        if self.closed || amount == 0 {
            return;
        }
        let Some(rule) = rules.rule(slot) else {
            return;
        };
        let Some(member) = self.member_mut(number) else {
            return;
        };
        for slot in std::iter::once(rule.slot).chain(rule.sums.iter().copied()) {
            let value = &mut member.values[usize::from(slot)];
            *value = value.saturating_add(amount);
        }
        member.score += rule.score_of(amount, in_range);
        // A score of thousands.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a score of thousands")]
        let score = member.score.floor().clamp(0.0, f64::from(u32::MAX)) as u32;
        member.values[usize::from(SCORE)] = score;
    }

    /// `amount` of the stat `slot` for player `number` that another row already counted with
    /// its sums and its score: only the slot rises. A slot the table lacks counts nothing.
    /// Inferred (player's design choice): a turret's hit counts in its target's row
    /// (`DamageToPlayers`, `DamageToAI`, `DamageToOther`) and in `DamageWithSiegeWeapons`, but
    /// `TotalDamageDealt` and the score count it once; retail's rule is Unknown (no capture
    /// holds the board of a player who used a turret).
    pub(super) fn note_row_alone(&mut self, number: u64, slot: u8, amount: u32, rules: &BoardStats) {
        if self.closed || amount == 0 || rules.rule(slot).is_none() {
            return;
        }
        let Some(member) = self.member_mut(number) else {
            return;
        };
        let value = &mut member.values[usize::from(slot)];
        *value = value.saturating_add(amount);
    }

    /// A hit of participant `attacker` on `target` (a player or a creature) at `now_ms`.
    pub(super) fn note_hit(&mut self, attacker: u64, target: u64, now_ms: u64) {
        let hits = self.hits.entry(target).or_default();
        hits.retain(|(by, at)| *by != attacker && now_ms.saturating_sub(*at) <= ASSIST_WINDOW_MS);
        hits.push((attacker, now_ms));
    }

    /// The assisters of the kill of `target` by `killer` at `now_ms`: the other participants
    /// whose hit on it lies within [`ASSIST_WINDOW_MS`]; the hits go.
    pub(super) fn take_assisters(&mut self, target: u64, killer: u64, now_ms: u64) -> Vec<u64> {
        self.hits
            .remove(&target)
            .unwrap_or_default()
            .into_iter()
            .filter(|(by, at)| *by != killer && now_ms.saturating_sub(*at) <= ASSIST_WINDOW_MS)
            .map(|(by, _)| by)
            .collect()
    }

    /// The 839s due at `now_ms` (and the full resend when due).
    pub(super) fn flush(&mut self, now_ms: u64, rules: &BoardStats) -> BoardPushes {
        if self.closed || self.next_push_ms == 0 || now_ms < self.next_push_ms {
            return Vec::new();
        }
        while self.next_push_ms <= now_ms {
            self.next_push_ms = self.next_push_ms.saturating_add(PUSH_MS);
        }
        if now_ms >= self.next_full_ms {
            while self.next_full_ms <= now_ms {
                self.next_full_ms = self.next_full_ms.saturating_add(FULL_RESEND_MS);
            }
            return self.full_pushes(rules);
        }
        self.changes(rules, false)
    }

    /// The end push: the changes, and each player's own end-game slots that are not 0. No push
    /// follows.
    pub(super) fn close(&mut self, rules: &BoardStats) -> BoardPushes {
        if self.closed {
            return Vec::new();
        }
        let pushes = self.changes(rules, true);
        self.closed = true;
        pushes
    }

    /// The 839 of the changes since the last push for each receiver (`end`: with its end-game
    /// slots).
    fn changes(&mut self, rules: &BoardStats, end: bool) -> BoardPushes {
        let in_game: Vec<u8> = rules.in_game_slots().collect();
        let changed = |values: &[u32; SLOTS], sent: &[u32; SLOTS]| -> std::collections::BTreeMap<u8, u32> {
            in_game
                .iter()
                .filter(|slot| values[usize::from(**slot)] != sent[usize::from(**slot)])
                .map(|slot| (*slot, values[usize::from(*slot)]))
                .collect()
        };
        let entries: Vec<(u8, u8, ParticipantStats)> = self
            .members
            .iter()
            .filter_map(|member| {
                let slots = changed(&member.values, &member.listed);
                let left = member.left && !member.left_sent;
                (left || !member.named || !slots.is_empty())
                    .then_some((member.team, member.index, ParticipantStats { left: member.left, slots }))
            })
            .collect();
        let lists = Self::lists(entries);
        let mut pushes = Vec::new();
        for member in self.receivers() {
            let mut local = changed(&member.values, &member.own);
            if end {
                local.extend(
                    (0..STAT_SLOTS)
                        .filter(|slot| !in_game.contains(slot) && rules.shows_at_end(*slot))
                        .map(|slot| (slot, member.values[usize::from(slot)]))
                        .filter(|(_, value)| *value != 0),
                );
            }
            if local.is_empty() && lists.iter().all(Vec::is_empty) {
                continue;
            }
            let stats = TeamsWarboardStats {
                sync: self.sync,
                local_index: member.index,
                local: ParticipantStats {
                    left: false,
                    slots: local,
                },
                lists: lists.clone(),
            };
            if let Some(push) = self.stats_push(&stats) {
                pushes.push((member.entity, vec![push]));
            }
        }
        for member in &mut self.members {
            member.listed = member.values;
            member.own = member.values;
            member.named = true;
            member.left_sent = member.left;
        }
        pushes
    }
}

/// Whether `at` stands within `range_m` of one of `outposts` (its capture point).
fn near_outpost(outposts: &[Outpost], at: Position, range_m: f32) -> bool {
    outposts.iter().any(|outpost| {
        let [x, y, z] = outpost.position;
        let (east, north) = (at.x - x, at.y - y);
        (east * east + north * north).sqrt() <= range_m && (at.z - z).abs() <= outpost.height_m
    })
}

/// The index of the outpost of `outposts` whose capture area holds `at`.
pub(super) fn outpost_index_at(outposts: &[Outpost], at: Position) -> Option<usize> {
    outposts
        .iter()
        .position(|outpost| near_outpost(std::slice::from_ref(outpost), at, CAPTURE_RADIUS_M))
}

impl SimWorld {
    /// Sends `pushes` to their players.
    pub(super) fn send_outpost_rush_board(&mut self, pushes: BoardPushes) {
        for (entity, messages) in pushes {
            let messages: Vec<ServerMessage> = messages.into_iter().map(ServerMessage::War).collect();
            self.send_to(entity, &messages);
        }
    }

    /// `amount` of stat `slot` for participant `number` of the match ([`Board::note`]), with
    /// the stat's range bonus when it stands near an outpost.
    pub(super) fn note_outpost_rush_stat(&mut self, number: u64, slot: u8, amount: u32) {
        let content = std::sync::Arc::clone(&self.content);
        let rules = &content.outpost_rush_board;
        let range_m = rules.rule(slot).map_or(0.0, |rule| rule.bonus_range_m);
        let at = self
            .player_by_number(number)
            .and_then(|entity| self.world.get::<Body>(entity))
            .map(|body| body.motion.position());
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        if running.state == ENDED {
            return;
        }
        let in_range = range_m > 0.0 && at.is_some_and(|at| near_outpost(&running.outposts, at, range_m));
        running.board.note(number, slot, amount, in_range, rules);
    }

    /// A hit of `amount` by `attacker` on `target` (a player or a creature of this sim): the
    /// board counts the damage of a participant to a foe participant (`DamageToPlayers`) or to a
    /// creature (`DamageToAI`), and the hit for an assist.
    pub(in crate::world) fn outpost_rush_hit(&mut self, target: u64, attacker: u64, amount: u32) {
        let now_ms = self.server_ms;
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        let Some(team) = running.team_of(attacker) else {
            return;
        };
        let slot = match running.team_of(target) {
            Some(target_team) if target_team != team => stat::DAMAGE_TO_PLAYERS,
            Some(_) => return,
            None if self.mobs.get(target).is_some() => stat::DAMAGE_TO_AI,
            None => return,
        };
        running.board.note_hit(attacker, target, now_ms);
        self.note_outpost_rush_stat(attacker, slot, amount);
    }

    /// A heal of `amount` by `healer` on `target`: `HealingDoneToSelf` or, for a team mate,
    /// `HealingDoneToAllies`.
    pub(in crate::world) fn outpost_rush_healed(&mut self, healer: u64, target: u64, amount: u32) {
        let Some(running) = self.arena_matches.outpost_rush.as_ref() else {
            return;
        };
        let slot = if healer == target {
            stat::HEALING_DONE_TO_SELF
        } else if running.team_of(healer).is_some() && running.team_of(healer) == running.team_of(target) {
            stat::HEALING_DONE_TO_ALLIES
        } else {
            return;
        };
        self.note_outpost_rush_stat(healer, slot, amount);
    }

    /// A block of participant `blocker` stopped `amount` of damage.
    pub(in crate::world) fn outpost_rush_blocked(&mut self, blocker: u64, amount: u32) {
        self.note_outpost_rush_stat(blocker, stat::DAMAGE_BLOCKED, amount);
    }

    /// Participant `number` gathered `quantity` of resource `item`.
    pub(in crate::world) fn outpost_rush_gathered(&mut self, number: u64, item: &str, quantity: u32) {
        if let Some((_, _, gathered)) = RESOURCE_STATS.iter().find(|(id, _, _)| id.eq_ignore_ascii_case(item)) {
            self.note_outpost_rush_stat(number, *gathered, quantity);
        }
    }

    /// A structure took `quantity` of resource `item` from participant `number`.
    pub(super) fn outpost_rush_deposited(&mut self, number: u64, item: &str, quantity: u32) {
        if let Some((_, deposited, _)) = RESOURCE_STATS.iter().find(|(id, _, _)| id.eq_ignore_ascii_case(item)) {
            self.note_outpost_rush_stat(number, *deposited, quantity);
        }
    }

    /// The death of participant `target` by `killer` (`killer_team`: the killer's team when it is
    /// a foe participant): its `Deaths`, the killer's `PlayerKills` and the assisters'
    /// `PlayerAssists`.
    pub(super) fn board_player_death(&mut self, target: u64, killer: u64, killer_team: Option<u8>) {
        let now_ms = self.server_ms;
        self.note_outpost_rush_stat(target, stat::DEATHS, 1);
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        let assisters: Vec<u64> = running
            .board
            .take_assisters(target, killer, now_ms)
            .into_iter()
            .filter(|number| killer_team.is_some() && running.team_of(*number) == killer_team)
            .collect();
        if killer_team.is_some() {
            self.note_outpost_rush_stat(killer, stat::PLAYER_KILLS, 1);
        }
        for assister in assisters {
            self.note_outpost_rush_stat(assister, stat::PLAYER_ASSISTS, 1);
        }
    }

    /// Creature `mob` died by a hit of `killer`: the killer's `AIKills` and the assisters'
    /// `AIAssists`.
    pub(super) fn board_mob_death(&mut self, mob: u64, killer: u64) {
        let now_ms = self.server_ms;
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        let assisters = running.board.take_assisters(mob, killer, now_ms);
        self.note_outpost_rush_stat(killer, stat::AI_KILLS, 1);
        for assister in assisters {
            self.note_outpost_rush_stat(assister, stat::AI_ASSISTS, 1);
        }
    }

    /// Player `entity` (`number`) asked for the board (3206): the full pair; `false` when it
    /// plays no Outpost Rush match here.
    pub(in crate::world) fn outpost_rush_board_enabled(&mut self, number: u64) -> bool {
        let content = std::sync::Arc::clone(&self.content);
        let Some(pair) = self
            .arena_matches
            .outpost_rush
            .as_ref()
            .and_then(|running| running.board.full_for(number, &content.outpost_rush_board))
        else {
            return false;
        };
        self.send_outpost_rush_board(vec![pair]);
        true
    }

    /// The participants who stood alive in an outpost's capture point at the last capture tick
    /// ([`super::capture_points`]) gain its contests at each of `ticks` score ticks.
    pub(super) fn board_contests(&mut self, ticks: u32) {
        let Some(running) = self.arena_matches.outpost_rush.as_ref() else {
            return;
        };
        let inside = running.capture_points.inside.clone();
        for number in inside {
            self.note_outpost_rush_stat(number, stat::CAPTURE_POINT_CONTESTS, CONTESTS_PER_SCORE_TICK.saturating_mul(ticks));
        }
    }
}

#[cfg(test)]
pub(in crate::world) mod tests {
    use nw_content::warboard_stats::StatRule;

    use super::*;

    /// The rows of `ORWarboardStatDefinitions` the tests use (client build 25810808).
    pub(in crate::world) fn rules() -> BoardStats {
        let rule = |slot: u8, in_game: bool, score: f64, range: f32, sums: &[u8]| StatRule {
            slot,
            in_game,
            end_game: true,
            score,
            bonus_range_m: range,
            in_range_multiplier: if range > 0.0 { 1.25 } else { 1.0 },
            sums: sums.to_vec(),
        };
        BoardStats::build([
            rule(SCORE, true, 0.0, 0.0, &[]),
            rule(stat::TOTAL_DAMAGE_DEALT, true, 0.0, 0.0, &[]),
            rule(stat::DEATHS, true, 0.0, 0.0, &[]),
            rule(stat::DAMAGE_TO_PLAYERS, false, 0.005, 14.0, &[stat::TOTAL_DAMAGE_DEALT]),
            rule(stat::DAMAGE_TO_AI, false, 0.0025, 0.0, &[stat::TOTAL_DAMAGE_DEALT]),
            rule(stat::DAMAGE_TO_OTHER, false, 0.0025, 0.0, &[stat::TOTAL_DAMAGE_DEALT]),
            rule(stat::DAMAGE_WITH_SIEGE_WEAPONS, false, 0.005, 0.0, &[stat::TOTAL_DAMAGE_DEALT]),
            rule(stat::PLAYER_KILLS, true, 50.0, 14.0, &[stat::PLAYER_TAKEDOWNS]),
            rule(stat::PLAYER_ASSISTS, true, 50.0, 14.0, &[stat::PLAYER_TAKEDOWNS]),
            rule(stat::PLAYER_TAKEDOWNS, false, 50.0, 0.0, &[]),
            rule(stat::AI_KILLS, false, 20.0, 0.0, &[stat::AI_TAKEDOWNS]),
            rule(stat::AI_TAKEDOWNS, true, 0.0, 0.0, &[]),
            rule(stat::CAPTURE_POINT_CONTESTS, false, 0.42, 0.0, &[]),
            rule(stat::INFUSED_WOOD_DEPOSITED, false, 2.4, 0.0, &[stat::TOTAL_RESOURCES_DEPOSITED]),
            rule(stat::TOTAL_RESOURCES_DEPOSITED, true, 0.0, 0.0, &[]),
        ])
    }

    const GDE_REF: [u8; RAW_ID_LEN] = [0x5b, 0x88, 0x63, 0x46, 0x4b, 0x8e, 0xa5, 0x6a, 0xb3, 0x83, 0xa9, 0x6a, 0xc0, 0x26, 0x6f, 0x48];

    fn entity(index: u32) -> Entity {
        Entity::from_bits(u64::from(index))
    }

    fn decoded(push: &WarServerMsg) -> TeamsWarboardStats {
        let WarServerMsg::UpdateWarboardStats { payload, .. } = push else {
            panic!("not an 839: {push:?}");
        };
        TeamsWarboardStats::decode_payload(payload).expect("839")
    }

    fn manifest_of(push: &WarServerMsg) -> TeamsManifest {
        match push {
            WarServerMsg::UpdateFullWarboardManifest { payload, .. }
            | WarServerMsg::UpdateWarboardManifest { payload, .. } => {
                TeamsManifest::decode_payload(payload).expect("manifest")
            }
            other => panic!("not a manifest: {other:?}"),
        }
    }

    /// A board with players 1 (team 0) and 2, 3 (team 1) joined at 0 ms.
    fn three_players() -> (Board, BoardStats) {
        let rules = rules();
        let mut board = Board::new(&GDE_REF);
        for (number, team) in [(1, 0), (2, 1), (3, 1)] {
            board.join(number, entity(number as u32), [number as u8; RAW_ID_LEN], team, 0, &rules);
        }
        (board, rules)
    }

    #[test]
    fn the_header_is_the_game_mode_facet_and_the_board_component() {
        // `OPR---ISLAND`: the 3312's `ActiveGameMode` facet `6aa58e4b4663885b`, the board
        // header `cc1ccb66d49e68d9 6aa58e4b4663885b`.
        let header = board_header(&GDE_REF);
        assert_eq!(header.target.wrapping_sub(header.entity), BOARD_COMPONENT);
        assert_eq!(header.entity, mode_facet(&GDE_REF));
    }

    #[test]
    fn a_joiner_gets_the_full_pair_and_the_others_its_manifest_update() {
        let rules = rules();
        let mut board = Board::new(&GDE_REF);
        let first = board.join(1, entity(1), [1; RAW_ID_LEN], 0, 0, &rules);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].1.len(), 2, "3530 and 839");
        let pushes = board.join(2, entity(2), [2; RAW_ID_LEN], 1, 500, &rules);
        let to_first = &pushes.iter().find(|(to, _)| *to == entity(1)).expect("first").1;
        let update = manifest_of(&to_first[0]);
        assert!(matches!(to_first[0], WarServerMsg::UpdateWarboardManifest { .. }));
        assert_eq!(update.sync, 2);
        assert!(update.lists[0].is_empty(), "team 1 is the manifest's second list");
        assert_eq!(update.lists[1], vec![(CharacterRef(Some([2; RAW_ID_LEN])), 0)]);
        let to_joiner = &pushes.iter().find(|(to, _)| *to == entity(2)).expect("joiner").1;
        let full = manifest_of(&to_joiner[0]);
        assert_eq!(full.lists[0].len() + full.lists[1].len(), 2);
        let stats = decoded(&to_joiner[1]);
        assert!(stats.local.slots.is_empty(), "the full 839's own part is empty");
        assert_eq!(stats.lists[0].len(), 1, "team 1 is the 839's first list");
        assert_eq!(stats.lists[0][0].1.slots.len(), rules.in_game_slots().count(), "every in-game slot");
        // The next push names the joiner with no slot.
        let named = board.flush(PUSH_MS, &rules);
        let stats = decoded(&named[0].1[0]);
        assert!(stats.lists[0].iter().any(|(index, slots)| *index == 0 && slots.slots.is_empty()));
    }

    /// A backfill into a running match (Observed: `OPR_Original_map` 0.53 s own index 16 of a
    /// team list of 17, 30 of 33 members with values; `OPR-coral-divide2` 101.88 s index 13 of
    /// 14): the joiner takes the next index of its team, leavers included, and its full pair
    /// holds every member's values of the match so far, its own part empty.
    #[test]
    fn a_backfill_takes_the_next_index_and_gets_the_values_so_far() {
        let (mut board, rules) = three_players();
        board.note(1, stat::PLAYER_KILLS, 1, false, &rules);
        board.leave(2);
        let pushes = board.join(4, entity(4), [4; RAW_ID_LEN], 1, PUSH_MS * 10, &rules);
        let to_joiner = &pushes.iter().find(|(to, _)| *to == entity(4)).expect("joiner").1;
        let full = manifest_of(&to_joiner[0]);
        assert_eq!(full.lists[1].len(), 3, "team 1 keeps its leaver: 2, 3 and the joiner");
        assert!(full.lists[1].contains(&(CharacterRef(Some([4; RAW_ID_LEN])), 2)));
        let stats = decoded(&to_joiner[1]);
        assert_eq!(stats.local_index, 2);
        assert!(stats.local.slots.is_empty(), "its own part is empty");
        let first = stats.lists[1]
            .iter()
            .find(|(index, _)| *index == 0)
            .map(|(_, slots)| slots.slots.get(&stat::PLAYER_KILLS).copied())
            .expect("team 0's member");
        assert_eq!(first, Some(1), "the values of the match so far");
    }

    #[test]
    fn a_kill_scores_with_its_range_bonus_and_its_takedown_without_score() {
        let (mut board, rules) = three_players();
        board.note(1, stat::PLAYER_KILLS, 1, false, &rules);
        board.note(1, stat::PLAYER_KILLS, 1, true, &rules);
        assert_eq!(board.score_of(1), 112, "50 + 62.5, rounded down");
        assert_eq!(board.value(1, stat::PLAYER_TAKEDOWNS), 2);
        board.note(2, stat::DAMAGE_TO_PLAYERS, 1_000, false, &rules);
        board.note(2, stat::DAMAGE_TO_AI, 1_000, false, &rules);
        assert_eq!(board.value(2, stat::TOTAL_DAMAGE_DEALT), 2_000);
        assert_eq!(board.score_of(2), 7, "5 + 2.5");
        board.note(3, stat::DEATHS, 1, false, &rules);
        assert_eq!(board.score_of(3), 0);
        board.note(3, stat::DAMAGE_TAKEN, 5, false, &rules);
        assert_eq!(board.value(3, stat::DAMAGE_TAKEN), 0, "a stat the table lacks counts nothing");
    }

    #[test]
    fn a_turret_hit_fills_both_rows_and_counts_once_in_the_total_and_the_score() {
        let (mut board, rules) = three_players();
        board.note(1, stat::DAMAGE_TO_PLAYERS, 1_000, false, &rules);
        board.note_row_alone(1, stat::DAMAGE_WITH_SIEGE_WEAPONS, 1_000, &rules);
        assert_eq!(board.value(1, stat::DAMAGE_TO_PLAYERS), 1_000);
        assert_eq!(board.value(1, stat::DAMAGE_WITH_SIEGE_WEAPONS), 1_000);
        assert_eq!(board.value(1, stat::TOTAL_DAMAGE_DEALT), 1_000, "once in the total");
        assert_eq!(board.score_of(1), 5, "once in the score: 1,000 x 0.005");
        board.note_row_alone(1, stat::DAMAGE_TAKEN, 5, &rules);
        assert_eq!(board.value(1, stat::DAMAGE_TAKEN), 0, "a stat the table lacks counts nothing");
    }

    #[test]
    fn a_push_carries_the_changes_once_each_second_and_the_end_the_end_game_slots() {
        let (mut board, rules) = three_players();
        board.flush(PUSH_MS, &rules);
        board.note(2, stat::PLAYER_KILLS, 1, false, &rules);
        board.note(2, stat::CAPTURE_POINT_CONTESTS, CONTESTS_PER_SCORE_TICK, false, &rules);
        assert!(board.flush(PUSH_MS + 500, &rules).is_empty(), "not due");
        let pushes = board.flush(2 * PUSH_MS, &rules);
        assert_eq!(pushes.len(), 3, "every player sees the change");
        let to_killer = decoded(&pushes.iter().find(|(to, _)| *to == entity(2)).expect("killer").1[0]);
        assert_eq!(to_killer.local.slots.get(&stat::PLAYER_KILLS), Some(&1));
        assert_eq!(to_killer.local.slots.get(&SCORE), Some(&87), "50 + 90 x 0.42 = 87.8");
        assert!(!to_killer.local.slots.contains_key(&stat::CAPTURE_POINT_CONTESTS), "an end-game slot");
        assert_eq!(to_killer.lists[0], vec![(0, ParticipantStats { left: false, slots: [(SCORE, 87), (stat::PLAYER_KILLS, 1)].into() })]);
        let to_other = decoded(&pushes.iter().find(|(to, _)| *to == entity(1)).expect("other").1[0]);
        assert!(to_other.local.slots.is_empty());
        assert!(board.flush(3 * PUSH_MS, &rules).is_empty(), "nothing changed");
        let end = board.close(&rules);
        let own = decoded(&end.iter().find(|(to, _)| *to == entity(2)).expect("killer").1[0]);
        assert_eq!(own.local.slots.get(&stat::CAPTURE_POINT_CONTESTS), Some(&CONTESTS_PER_SCORE_TICK));
        assert_eq!(own.local.slots.get(&stat::PLAYER_TAKEDOWNS), Some(&1));
        assert!(!own.local.slots.contains_key(&stat::DAMAGE_TO_AI), "a 0 stays out");
        assert!(board.flush(10 * PUSH_MS, &rules).is_empty(), "no push after the end");
    }

    #[test]
    fn a_leaver_is_named_once_with_the_left_bit_and_kept_in_the_full_pushes() {
        let (mut board, rules) = three_players();
        board.flush(PUSH_MS, &rules);
        board.leave(3);
        let pushes = board.flush(2 * PUSH_MS, &rules);
        assert_eq!(pushes.len(), 2, "the leaver gets nothing");
        let stats = decoded(&pushes[0].1[0]);
        assert_eq!(stats.lists[0], vec![(1, ParticipantStats { left: true, slots: Default::default() })]);
        assert!(board.flush(3 * PUSH_MS, &rules).is_empty());
        let full = board.flush(FULL_RESEND_MS, &rules);
        let stats = decoded(&full[0].1[1]);
        assert!(stats.lists[0].iter().any(|(index, slots)| *index == 1 && slots.left));
    }

    #[test]
    fn an_assist_is_a_recent_hit_of_another_participant() {
        let (mut board, _) = three_players();
        board.note_hit(2, 1, 0);
        board.note_hit(3, 1, 5_000);
        board.note_hit(2, 1, 6_000);
        assert_eq!(board.take_assisters(1, 3, 12_000), vec![2]);
        assert!(board.take_assisters(1, 3, 12_000).is_empty(), "taken once");
        board.note_hit(2, 1, 0);
        assert!(board.take_assisters(1, 3, ASSIST_WINDOW_MS + 1).is_empty(), "too old");
    }

    #[test]
    fn the_resources_name_their_deposit_and_gather_stats() {
        for (item, deposited, gathered) in RESOURCE_STATS {
            assert!(item.starts_with("OutpostRush"));
            assert_eq!(gathered - deposited, stat::INFUSED_WOOD_GATHERED - stat::INFUSED_WOOD_DEPOSITED);
        }
        assert_eq!(CONTESTS_PER_SCORE_TICK, 90);
    }
}
