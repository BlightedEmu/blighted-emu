// SPDX-License-Identifier: AGPL-3.0-or-later
//! The player's faction (faction note 2026-10-30, `local-data/findings`): the client's
//! `RequestSetFaction` (3161) after a faction representative's `Faction_Selection` service sets
//! it; the faction state change goes with the next bundle (retail sends no answer), and the
//! quests take it (`TaskChooseFaction`).
//!
//! **Observed** (retail `Hive_join_yellow_faction_quest`, `Change-Faction`): a first join changes
//! the faction alone; a change from one faction to another also sets the change's time and adds
//! one to the change count. **Unknown**: whether retail refuses a change in the faction change
//! cooldown (the representative's conversation states `InCooldownSingleDay` and
//! `InCooldownMultipleDays` keep the client from asking then); this server takes every pick of a
//! joinable faction other than the player's.
//!
//! The PvP flag (`TogglePvpFlag` 2525, retail `Hive_flag_pvp_on_and_off`, faction state bodies in
//! `nw_messages::faction_state`): a toggle on makes the flag pending; in a sanctuary it waits;
//! out of one its countdown of [`PVP_FLAG_DELAY_MS`] runs (it starts when the player leaves the
//! sanctuary); at its end the flag is on, and only then does combat count the player as
//! flagged. Entering a sanctuary with the flag on takes it off and makes it pending again; a
//! toggle while pending cancels it. Inferred: a sanctuary is the reach of a settlement or outpost
//! capital ([`crate::trading_post::SETTLEMENT_REACH_M`]: the level data loaded here has no
//! sanctuary volumes); Observed: a spirit shrine is one too ([`SHRINE_SANCTUARY_REACH_M`]); a toggle outside a sanctuary starts the countdown at once, a toggle with
//! the flag on takes it off (`pvpFlag` 0, `notifyPending` 1), and entering a sanctuary during a
//! countdown stops it (`pvpFlagPendingEndTime` 0): the captured run toggled in a sanctuary only.
//! A handoff to another sim carries what is left of a countdown; the next sim runs the rest.
//!
//! The free-for-all flag ([`FfaWatch`]; Observed in `FFA_ExitFFAAndCleanse`,
//! `FFA_KillPlayerAndLoot` and `FFA_LightningStrikeDeath`, the bodies are in
//! [`nw_messages::faction_state`]): a player whose client says it is in a free-for-all area
//! (`TerritoryDefinitions.IsFFA`; retail set the countdown 0.17 s after the client's 2297 of
//! such an area) gets `ffaPendingEndTime` [`FFA_ON_DELAY_MS`] ahead; at that time its `pvpFlag`
//! and `ffaFlag` go on. A player that leaves before that time gets the end time 0. A flagged
//! player that leaves gets the end time [`FFA_OFF_DELAY_MS`] ahead (0.1 s after its client's
//! 6416), and at that time both flags go off. Two players with the free-for-all flag are foes
//! whatever their factions ([`SimWorld::pvp_foes`]). A player whose own PvP flag is on gets
//! the free-for-all flag alone (five retail ledgers). When the flag goes off, the bag's cursed
//! items become their `FFAConvertedItemId` items in the same bundle (`Gear::convert`), and the client gets 6526 after it (Observed: `FFA_ExitFFAAndCleanse` 246.447 s
//! and 246.46 s, `FFA_KillPlayerAndLoot` 237.905 s and 237.92 s); a bag with nothing to convert
//! gets no 6526 (Inferred: both retail exits had cursed items).
//!
//! The flag bonus: retail gives the status effect `PvP_Flag_Bonus` with every PvP flag on, its
//! own or the zone's, and ends it with the flag (Observed in 10 of the 13 free-for-all ledgers,
//! `FFA_ExitFFAAndCleanse` 107.514 s on and 246.512 s off; `FFA_KillPlayerAndLoot` 133.371 s, the
//! same bundle as the flag; a lightweight effect of value 0). Its row gives luck: `GlobalRollMod`
//! 1000 and `ROL_*` 3000 for each gathering tradeskill ([`super::gathering::LootBonuses`]).
//!
//! Inferred: a flagged player that comes back during the off countdown keeps its flags (the
//! end time 0); a player whose own PvP flag was on keeps `pvpFlag` when the free-for-all flag
//! goes off, and the change names `ffaFlag` alone (as at its entry; no retail exit of such a
//! player); a handoff starts the watch again. Not built (Unknown rules): the anti-grouping
//! curse (`ffaAntiGroupingIsCursing`, `ffaAntiGroupingCurseStacks`, effect `FFA_PvP_Curse`), the
//! zone's balance table (`FFAZoneBalanceData`), and a death in the zone (the cursed share that
//! drops for the killer, the conversion of the rest at the death and the killer's loot roll;
//! findings note 2026-11-11 `ffa-zone-rest`).

use bevy_ecs::entity::Entity;
use nw_content::factions::{NO_FACTION, joinable};
use nw_messages::faction_state::{FactionChange, FactionStateChange, PublicFaction};
use nw_messages::{ItemsConvertedMsg, ServerMessage};

use super::gear::unix_ns;
use super::{Body, Player, SimWorld, send_now};
use crate::travel_rules::SHRINE_LANDMARK_TYPE;

/// The status effect a player holds while a PvP flag is on (Observed, see the module note).
const PVP_FLAG_BONUS: &str = "PvP_Flag_Bonus";

/// The PvP flag countdown after the player leaves a sanctuary. Observed: in
/// `Hive_flag_pvp_on_and_off` the flag went on 30.0 s after the bundle that left the sanctuary
/// (1782821022777 to 1782821052796).
pub(super) const PVP_FLAG_DELAY_MS: u64 = 30_000;
/// The reach of a spirit shrine's sanctuary, metres from its landmark. Observed (scan of 1,080
/// retail ledgers, 2026-11-06, the own player's `Sanctuary` effect 1316 near a shrine landmark
/// and 30 m nearer to it than to any other landmark): 498 walks out turned it off at a median
/// of 7.6 m (p10 5.0, p90 11.8) and 380 walks in turned it on at a median of 5.4 m (p10 2.5,
/// p90 8.6); every fast travel to a shrine arrives with it on. The edge between the two medians
/// is Inferred (the samples lag the crossing by up to 300 ms).
const SHRINE_SANCTUARY_REACH_M: f32 = 6.5;

/// The free-for-all flag goes on this long after the player came into a free-for-all area.
/// Observed: 30.0 s in `FFA_ExitFFAAndCleanse` (77.307 s to 107.267 s) and in
/// `FFA_KillPlayerAndLoot` (103.324 s to 133.355 s).
const FFA_ON_DELAY_MS: u64 = 30_000;
/// The free-for-all flag goes off this long after the player left the area. Observed: 5.0 s
/// in `FFA_ExitFFAAndCleanse` (241.429 s to 246.447 s) and `FFA_KillPlayerAndLoot` (232.873 s
/// to 237.905 s).
const FFA_OFF_DELAY_MS: u64 = 5_000;

/// A player's free-for-all flag and its countdown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct FfaWatch {
    /// The flag is on (`ffaFlag`).
    flagged: bool,
    /// Sim ms when the flag changes: on for an unflagged player, off for a flagged one.
    changes_ms: Option<u64>,
}

impl FfaWatch {
    /// Whether the free-for-all flag is on.
    pub(super) const fn flagged(&self) -> bool {
        self.flagged
    }

    /// Follows a player that is in a free-for-all area or not (`inside`) at sim ms `now_ms`
    /// (server ms `server_ms`); `pvp_flagged` is its own PvP flag. The state change for its
    /// client, when there is one.
    fn step(
        &mut self,
        inside: bool,
        pvp_flagged: bool,
        now_ms: u64,
        server_ms: u64,
    ) -> Option<FactionStateChange> {
        let ends_ns = |delay_ms: u64| unix_ns(server_ms.saturating_add(delay_ms));
        let change = match (self.flagged, inside, self.changes_ms) {
            // It came in, or it went out with its flag on: the countdown starts.
            (false, true, None) | (true, false, None) => {
                let delay_ms = if self.flagged {
                    FFA_OFF_DELAY_MS
                } else {
                    FFA_ON_DELAY_MS
                };
                self.changes_ms = Some(now_ms.saturating_add(delay_ms));
                FactionStateChange {
                    ffa_pending_end_ns: Some(ends_ns(delay_ms)),
                    ..FactionStateChange::default()
                }
            }
            // It left before the flag went on, or came back before it went off.
            (false, false, Some(_)) | (true, true, Some(_)) => {
                self.changes_ms = None;
                FactionStateChange {
                    ffa_pending_end_ns: Some(0),
                    ..FactionStateChange::default()
                }
            }
            // A player with its own PvP flag on gets the free-for-all flag alone (`06 20 <0>
            // 08 01`, retail `FFA_BallLightningDeath` 103.216 s, `FFA_TreasureMaps` 82.689 s and
            // three more ledgers).
            (false, true, Some(ends_ms)) if now_ms >= ends_ms => {
                self.flagged = true;
                self.changes_ms = None;
                FactionStateChange {
                    notify_pending: (!pvp_flagged).then_some(true),
                    ffa_pending_end_ns: Some(0),
                    pvp_flag: (!pvp_flagged).then_some(true),
                    ffa_flag: Some(true),
                    ..FactionStateChange::default()
                }
            }
            (true, false, Some(ends_ms)) if now_ms >= ends_ms => {
                self.flagged = false;
                self.changes_ms = None;
                FactionStateChange {
                    ffa_pending_end_ns: Some(0),
                    pvp_flag: (!pvp_flagged).then_some(false),
                    ffa_flag: Some(false),
                    ..FactionStateChange::default()
                }
            }
            _ => return None,
        };
        Some(change)
    }
}

/// A player's PvP flag on its way on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct PvpFlagWait {
    /// The flag is pending (`pvpFlagPending`).
    pending: bool,
    /// Sim ms when the pending flag goes on; `None` while it waits in a sanctuary.
    ends_ms: Option<u64>,
    /// Whether the player is in a sanctuary (`hasSanctuary`); `None` before the first check.
    in_sanctuary: Option<bool>,
    /// What was left of the countdown on the sim the player came from (ms); the first check
    /// outside a sanctuary runs this rest, not a whole countdown.
    carried_left_ms: Option<u64>,
}

impl PvpFlagWait {
    /// The wait of a player that wants its flag on but does not have it: the flag goes on after
    /// the countdown outside a sanctuary (a login of a player whose flag was on or on its way
    /// on, Inferred: as a sanctuary takes the flag off and keeps it pending).
    /// `carried_left_ms` is the rest of a countdown that ran on the sim it came from.
    pub(super) const fn wanted(wanted: bool, carried_left_ms: Option<u64>) -> Self {
        Self {
            pending: wanted,
            ends_ms: None,
            in_sanctuary: None,
            carried_left_ms,
        }
    }

    /// What is left of a running countdown at sim ms `now_ms`.
    pub(super) fn left_ms(&self, now_ms: u64) -> Option<u64> {
        self.ends_ms.map(|ends_ms| ends_ms.saturating_sub(now_ms))
    }

    /// Whether the flag is on its way on.
    pub(super) const fn is_pending(&self) -> bool {
        self.pending
    }

    /// Whether the player is in a sanctuary (`hasSanctuary`), as the last check saw it.
    pub(super) fn in_sanctuary(&self) -> bool {
        self.in_sanctuary == Some(true)
    }
}
use crate::objectives::Event;

impl SimWorld {
    /// Player `number` (`entity`) picks faction `faction` at `now_ms` (3161).
    pub(super) fn set_faction(&mut self, entity: Entity, number: u64, faction: u8, now_ms: u64) {
        let changed_ns = unix_ns(self.server_ms);
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let before = player.faction.faction;
        if !joinable(faction) || faction == before {
            tracing::debug!(player = number, faction, before, "faction pick refused");
            return;
        }
        let switched = (before != NO_FACTION).then(|| {
            player.faction.changes = player.faction.changes.saturating_add(1);
            player.faction.changed_ns = changed_ns;
            (changed_ns, player.faction.changes)
        });
        player.faction.faction = faction;
        push_faction(&mut player, switched);
        tracing::info!(player = number, faction, before, "faction set");
        self.quest_event(number, Event::FactionChosen(faction), now_ms);
    }

    /// The client of player `number`, which just spawned, gets its faction when it has one
    /// (retail logs a character with a faction in with it; the card's first body has none).
    pub(super) fn send_faction(&mut self, number: u64) {
        if let Some(mut player) = self
            .player_by_number(number)
            .and_then(|entity| self.world.get_mut::<Player>(entity))
            && player.faction.faction != NO_FACTION
        {
            push_faction(&mut player, None);
        }
    }
}

impl SimWorld {
    /// 2525: player `entity` toggles its PvP flag at sim ms `now_ms`.
    pub(super) fn toggle_pvp_flag(&mut self, entity: Entity, now_ms: u64) {
        let end_ns = unix_ns(self.server_ms.saturating_add(PVP_FLAG_DELAY_MS));
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let wait = player.pvp_wait;
        // A toggle starts a whole countdown: the rest of a carried one no longer counts.
        player.pvp_wait.carried_left_ms = None;
        let change = if player.pvp_flagged {
            player.pvp_flagged = false;
            FactionStateChange {
                notify_pending: Some(true),
                pvp_flag: Some(false),
                ..FactionStateChange::default()
            }
        } else if wait.pending {
            player.pvp_wait.pending = false;
            player.pvp_wait.ends_ms = None;
            FactionStateChange {
                pvp_flag_pending: Some(false),
                notify_pending: Some(true),
                pvp_flag_pending_end_ns: wait.ends_ms.map(|_| 0),
                ..FactionStateChange::default()
            }
        } else {
            player.pvp_wait.pending = true;
            let outside = wait.in_sanctuary == Some(false);
            if outside {
                player.pvp_wait.ends_ms = Some(now_ms.saturating_add(PVP_FLAG_DELAY_MS));
            }
            FactionStateChange {
                pvp_flag_pending: Some(true),
                notify_pending: Some(true),
                pvp_flag_pending_end_ns: outside.then_some(end_ns),
                ..FactionStateChange::default()
            }
        };
        push_state(&mut player, &change);
    }

    /// Follows each player's sanctuary and PvP flag countdown at sim ms `now_ms`.
    pub(super) fn tick_pvp_flags(&mut self, now_ms: u64) {
        let server_ms = self.server_ms;
        let content = &self.content;
        let settlements = &content.settlements;
        let travel = &content.travel;
        let ffa_areas = &content.ffa_areas;
        let effects = &content.status_effects;
        let mut converted = Vec::new();
        let mut query = self.world.query::<(&mut Player, &Body)>();
        for (mut player, body) in query.iter_mut(&mut self.world) {
            let at = body.motion.position();
            let inside = settlements.nearest(at.x, at.y).is_some()
                || travel.landmark_within(
                    SHRINE_LANDMARK_TYPE,
                    at.x,
                    at.y,
                    SHRINE_SANCTUARY_REACH_M,
                );
            let mut change = FactionStateChange::default();
            let was = player.pvp_wait.in_sanctuary;
            if was != Some(inside) {
                player.pvp_wait.in_sanctuary = Some(inside);
                // Only the first check after a handoff runs the rest of a carried countdown.
                let carried_left_ms = player.pvp_wait.carried_left_ms.take();
                // A first check outside changes nothing the client shows (its default is 0).
                if was.is_some() || inside {
                    change.has_sanctuary = Some(inside);
                }
                // A wait that came with the player: its client learns it is pending.
                if was.is_none() && player.pvp_wait.pending {
                    change.pvp_flag_pending = Some(true);
                }
                if inside && player.pvp_flagged {
                    player.pvp_flagged = false;
                    player.pvp_wait.pending = true;
                    change.pvp_flag = Some(false);
                    change.pvp_flag_pending = Some(true);
                } else if inside && player.pvp_wait.ends_ms.take().is_some() {
                    change.pvp_flag_pending_end_ns = Some(0);
                } else if !inside && player.pvp_wait.pending {
                    let left_ms = carried_left_ms.unwrap_or(PVP_FLAG_DELAY_MS);
                    player.pvp_wait.ends_ms = Some(now_ms.saturating_add(left_ms));
                    change.pvp_flag_pending_end_ns =
                        Some(unix_ns(server_ms.saturating_add(left_ms)));
                }
            }
            if player.pvp_wait.pending && player.pvp_wait.ends_ms.is_some_and(|ends| now_ms >= ends)
            {
                player.pvp_wait.pending = false;
                player.pvp_wait.ends_ms = None;
                player.pvp_flagged = true;
                change.pvp_flag_pending = Some(false);
                change.notify_pending = Some(false);
                change.pvp_flag_pending_end_ns = Some(0);
                change.pvp_flag = Some(true);
            }
            if change != FactionStateChange::default() {
                push_state(&mut player, &change);
            }
            let in_ffa = player
                .territories
                .iter()
                .any(|area| ffa_areas.contains(area));
            let pvp_flagged = player.pvp_flagged;
            if let Some(change) = player.ffa.step(in_ffa, pvp_flagged, now_ms, server_ms) {
                push_state(&mut player, &change);
                if change.ffa_flag == Some(false)
                    && player
                        .gear
                        .as_mut()
                        .is_some_and(|gear| gear.convert(&content.kits, &content.ffa_conversions))
                {
                    converted.push((player.connection.clone(), player.number));
                }
            }
            let flagged = player.pvp_flagged || player.ffa.flagged();
            let number = player.number;
            player
                .status_effects
                .hold_while(effects, PVP_FLAG_BONUS, flagged, number, server_ms);
        }
        for (connection, number) in converted {
            let message = ServerMessage::ItemsConverted(ItemsConvertedMsg::to(number));
            send_now(&mut self.after_bundles, &connection, &[message]);
        }
    }
}

impl Player {
    /// Its faction and PvP flags as other players get them: the PvP flag is its own or the
    /// free-for-all zone's.
    pub(super) fn public_faction(&self) -> PublicFaction {
        PublicFaction {
            faction: self.faction.faction,
            pvp_flag: self.pvp_flagged || self.ffa.flagged(),
            sanctuary: self.pvp_wait.in_sanctuary(),
            ffa_flag: self.ffa.flagged(),
        }
    }
}

/// Queues faction state change `change` of `player`.
fn push_state(player: &mut Player, change: &FactionStateChange) {
    push_member(player, change.member());
}

/// Queues the encoded faction state member `member` of `player`.
fn push_member(
    player: &mut Player,
    member: Result<nw_messages::BundleMember, nw_messages::wire::MessageError>,
) {
    match member {
        Ok(member) => player.extra_members.push(member),
        Err(error) => tracing::warn!(%error, "faction state not encoded"),
    }
}

/// Queues the faction state change of `player`: its faction, and `switched` (the change's time
/// and the change count) for a change from one faction to another.
fn push_faction(player: &mut Player, switched: Option<(u64, u16)>) {
    let change = FactionChange {
        faction: player.faction.faction,
        switched,
    };
    push_member(player, change.member());
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nw_content::factions::{COVENANT, NO_FACTION, SYNDICATE};
    use nw_messages::MemberState;
    use nw_messages::faction_state::FACTION_TYPE_INDEX;

    use super::*;
    use crate::objectives::{TaskBook, TaskRow};
    use crate::player_content::{ObjectiveRow, QuestBook};
    use crate::trading_post::{Settlement, Settlements};
    use crate::world::QuestRefusal;
    use crate::world::tests::{NOW, SERVER_TIME, admit_at, connection, world_with_spawns};

    const PLAYER: u64 = 1;
    /// The Covenant recruitment of build 25810808 (cut): its join task, and a Covenant-only
    /// quest.
    const RECRUITMENT: &str = "FactionIntro_Covenant_Recruitment";
    const COVENANT_ONLY: &str = "Covenant_Story";

    fn world() -> (SimWorld, Entity) {
        let mut world = world_with_spawns(Vec::new());
        let task = |id: &str, kind: &str, children: &[&str]| TaskRow {
            task_id: id.to_owned(),
            kind: kind.to_owned(),
            sub_tasks: children.iter().map(|child| (*child).to_owned()).collect(),
            ..TaskRow::default()
        };
        let quests = QuestBook::build(
            [
                ObjectiveRow {
                    id: RECRUITMENT.to_owned(),
                    root: "root".to_owned(),
                    required_faction: Some("!Covenant".to_owned()),
                    ..ObjectiveRow::default()
                },
                ObjectiveRow {
                    id: COVENANT_ONLY.to_owned(),
                    root: "root".to_owned(),
                    required_faction: Some("Covenant".to_owned()),
                    ..ObjectiveRow::default()
                },
            ],
            TaskBook::new(vec![
                task("root", "ConsecutiveTaskContainer", &["join"]),
                TaskRow {
                    item_name: Some("9814_Beyza".to_owned()),
                    ..task("join", "TaskChooseFaction", &[])
                },
            ]),
        );
        Arc::make_mut(&mut world.content).quests = quests;
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
        let entity = world.by_connection[&connection(PLAYER)];
        (world, entity)
    }

    /// The faction state bodies queued for the player, and the queue emptied.
    fn faction_bodies(world: &mut SimWorld, entity: Entity) -> Vec<Vec<u8>> {
        let mut player = world.world.get_mut::<Player>(entity).expect("player");
        let bodies = player
            .extra_members
            .iter()
            .filter_map(|member| match &member.state {
                MemberState::Recorded { class, body } if *class == FACTION_TYPE_INDEX => {
                    Some(body.clone())
                }
                _ => None,
            })
            .collect();
        player.extra_members.clear();
        bodies
    }

    #[test]
    fn a_pick_joins_the_faction_ends_the_join_task_and_a_switch_counts() {
        let (mut world, entity) = world();
        assert_eq!(
            world.accept_quest(PLAYER, COVENANT_ONLY, None, NOW),
            Err(QuestRefusal::Faction),
            "not of the Covenant yet"
        );
        world
            .accept_quest(PLAYER, RECRUITMENT, None, NOW)
            .expect("the recruitment");
        faction_bodies(&mut world, entity);
        world.set_faction(entity, PLAYER, NO_FACTION, NOW);
        assert!(
            faction_bodies(&mut world, entity).is_empty(),
            "no faction is not a pick"
        );
        world.set_faction(entity, PLAYER, COVENANT, NOW);
        assert_eq!(
            faction_bodies(&mut world, entity),
            [vec![0x04, 0x01, COVENANT]]
        );
        let recruitment = world
            .quests_of_player(PLAYER)
            .iter()
            .find(|quest| quest.def.id == RECRUITMENT)
            .map(|quest| quest.run.finished());
        assert_ne!(recruitment, Some(false), "the join task is done");
        world
            .accept_quest(PLAYER, COVENANT_ONLY, None, NOW)
            .expect("of the Covenant now");
        world.set_faction(entity, PLAYER, SYNDICATE, NOW);
        let faction = world.world.get::<Player>(entity).expect("player").faction;
        assert_eq!((faction.faction, faction.changes), (SYNDICATE, 1));
        assert_eq!(faction.changed_ns, unix_ns(world.server_ms));
        let bodies = faction_bodies(&mut world, entity);
        assert_eq!(bodies.len(), 1);
        assert_eq!(
            &bodies[0][..2],
            [0x06, 0x18],
            "the time and the count with it"
        );
        assert_eq!(
            world
                .world
                .get::<Player>(entity)
                .expect("player")
                .progression()
                .faction,
            faction,
            "it is saved"
        );
    }

    /// Puts the test player at `x` metres east (the settlement's capital is at the origin).
    fn move_to(world: &mut SimWorld, entity: Entity, x: f32) {
        let limits = world.settings.limits;
        world.world.get_mut::<Body>(entity).expect("body").motion =
            nw_sim_core::Motion::new(nw_sim_core::Position { x, y: 0.0, z: 50.0 }, NOW, &limits);
    }

    fn flagged(world: &SimWorld, entity: Entity) -> bool {
        world
            .world
            .get::<Player>(entity)
            .expect("player")
            .pvp_flagged
    }

    /// Retail `Hive_flag_pvp_on_and_off`: a toggle in a sanctuary waits; leaving it starts the
    /// countdown; the flag (and combat's view of it) is on only at its end; entering a
    /// sanctuary takes it off and makes it pending; a toggle then cancels it.
    #[test]
    fn the_pvp_flag_waits_out_the_sanctuary_and_its_countdown() {
        const OUTSIDE_M: f32 = 1_000.0;
        let (mut world, entity) = world();
        Arc::make_mut(&mut world.content).settlements = Settlements::of(vec![Settlement {
            id: 1,
            x: 0.0,
            y: 0.0,
        }]);
        move_to(&mut world, entity, 0.0);
        faction_bodies(&mut world, entity);
        world.tick_pvp_flags(NOW);
        assert_eq!(faction_bodies(&mut world, entity), [vec![0x04, 0x04, 0x01]]);
        world.toggle_pvp_flag(entity, NOW);
        assert_eq!(
            faction_bodies(&mut world, entity),
            [vec![0x02, 0x03, 0x01, 0x01]]
        );
        world.tick_pvp_flags(NOW + PVP_FLAG_DELAY_MS * 2);
        assert!(!flagged(&world, entity), "it waits in the sanctuary");
        move_to(&mut world, entity, OUTSIDE_M);
        let left_ms = NOW + PVP_FLAG_DELAY_MS * 2;
        world.tick_pvp_flags(left_ms);
        let end_ns = unix_ns(world.server_ms + PVP_FLAG_DELAY_MS);
        let mut left = vec![0x06, 0x04];
        left.extend(end_ns.to_be_bytes());
        left.extend([0x04, 0x00]);
        assert_eq!(faction_bodies(&mut world, entity), [left]);
        world.tick_pvp_flags(left_ms + PVP_FLAG_DELAY_MS - 1);
        assert!(!flagged(&world, entity), "the countdown runs");
        world.tick_pvp_flags(left_ms + PVP_FLAG_DELAY_MS);
        assert!(flagged(&world, entity), "on at the end");
        assert_eq!(
            faction_bodies(&mut world, entity),
            [vec![0x06, 0x07, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02, 0x01]]
        );
        move_to(&mut world, entity, 0.0);
        world.tick_pvp_flags(left_ms + PVP_FLAG_DELAY_MS + 1);
        assert!(!flagged(&world, entity), "a sanctuary takes it off");
        assert_eq!(
            faction_bodies(&mut world, entity),
            [vec![0x06, 0x01, 0x01, 0x06, 0x00, 0x01]]
        );
        world.toggle_pvp_flag(entity, NOW);
        assert_eq!(
            faction_bodies(&mut world, entity),
            [vec![0x02, 0x03, 0x00, 0x01]]
        );
        move_to(&mut world, entity, OUTSIDE_M);
        world.tick_pvp_flags(left_ms * 2);
        world.tick_pvp_flags(left_ms * 4);
        assert!(!flagged(&world, entity), "a cancelled flag stays off");
    }

    /// Retail ledgers: a spirit shrine is a sanctuary out to [`SHRINE_SANCTUARY_REACH_M`] of its
    /// landmark, so a flagged player who walks onto one is flagged no more.
    #[test]
    fn the_flag_bonus_is_held_while_a_pvp_flag_is_on() {
        use nw_content::status_effects::{EffectSourceRow, StatusEffectRules, StatusEffectSource};
        let (mut world, entity) = world();
        Arc::make_mut(&mut world.content).status_effects =
            StatusEffectRules::build(StatusEffectSource {
                effects: vec![EffectSourceRow {
                    status_id: PVP_FLAG_BONUS.to_owned(),
                    lightweight: true,
                    ..EffectSourceRow::default()
                }],
                ..StatusEffectSource::default()
            });
        let bonus = nw_formats::objectstream::lowercase_crc32(PVP_FLAG_BONUS);
        let holds = |world: &SimWorld| {
            world
                .world
                .get::<Player>(entity)
                .expect("player")
                .status_effects
                .holds(bonus)
        };
        world.tick_pvp_flags(NOW);
        assert!(!holds(&world));
        world
            .world
            .get_mut::<Player>(entity)
            .expect("player")
            .pvp_flagged = true;
        world.tick_pvp_flags(NOW);
        assert!(holds(&world), "on with the own flag");
        world
            .world
            .get_mut::<Player>(entity)
            .expect("player")
            .pvp_flagged = false;
        world.tick_pvp_flags(NOW);
        assert!(!holds(&world), "off with the flag");
    }

    #[test]
    fn a_spirit_shrine_is_a_sanctuary_within_its_reach() {
        /// The shrine landmark, far from every settlement (there is none in this world).
        const SHRINE_X_M: f32 = 2_000.0;
        const OUTSIDE_M: f32 = SHRINE_X_M + SHRINE_SANCTUARY_REACH_M + 1.0;
        let (mut world, entity) = world();
        Arc::make_mut(&mut world.content).travel = crate::travel_rules::TravelRules::build(
            vec![crate::travel_rules::TravelLandmark {
                landmark_type: SHRINE_LANDMARK_TYPE,
                position: [SHRINE_X_M, 0.0, 50.0],
                areas: Vec::new(),
                territory: None,
            }],
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            None,
            nw_content::gameplay_config::TravelKeys::default(),
        );
        move_to(&mut world, entity, OUTSIDE_M);
        world.tick_pvp_flags(NOW);
        world.toggle_pvp_flag(entity, NOW);
        world.tick_pvp_flags(NOW + PVP_FLAG_DELAY_MS);
        world.tick_pvp_flags(NOW + PVP_FLAG_DELAY_MS * 2);
        assert!(flagged(&world, entity), "flagged outside the shrine");
        faction_bodies(&mut world, entity);
        move_to(&mut world, entity, SHRINE_X_M + SHRINE_SANCTUARY_REACH_M);
        world.tick_pvp_flags(NOW + PVP_FLAG_DELAY_MS * 3);
        assert!(
            !flagged(&world, entity),
            "the shrine's sanctuary takes it off"
        );
        assert_eq!(
            faction_bodies(&mut world, entity),
            [vec![0x06, 0x01, 0x01, 0x06, 0x00, 0x01]],
            "the same change as a settlement's sanctuary"
        );
    }

    #[test]
    fn the_free_for_all_flag_goes_on_after_the_entry_and_off_after_the_exit() {
        const ENTERED_MS: u64 = 50_000;
        const SERVER_MS: u64 = 800_000_000_000;
        let mut watch = FfaWatch::default();
        assert_eq!(watch.step(false, false, ENTERED_MS - 1, SERVER_MS), None);
        let pending = watch
            .step(true, false, ENTERED_MS, SERVER_MS)
            .expect("the countdown");
        assert_eq!(
            pending.ffa_pending_end_ns,
            Some(unix_ns(SERVER_MS + FFA_ON_DELAY_MS))
        );
        assert_eq!(
            watch.step(true, false, ENTERED_MS + FFA_ON_DELAY_MS - 1, SERVER_MS),
            None
        );
        let on = watch
            .step(true, false, ENTERED_MS + FFA_ON_DELAY_MS, SERVER_MS)
            .expect("the flag");
        assert_eq!(
            (on.pvp_flag, on.ffa_flag, on.ffa_pending_end_ns),
            (Some(true), Some(true), Some(0))
        );
        assert!(watch.flagged());
        // Out: the flag stays for the off countdown, then both flags go off.
        let left_ms = ENTERED_MS + 100_000;
        let leaving = watch
            .step(false, false, left_ms, SERVER_MS)
            .expect("the countdown");
        assert_eq!(
            leaving.ffa_pending_end_ns,
            Some(unix_ns(SERVER_MS + FFA_OFF_DELAY_MS))
        );
        assert!(watch.flagged());
        let off = watch
            .step(false, false, left_ms + FFA_OFF_DELAY_MS, SERVER_MS)
            .expect("the flags off");
        assert_eq!((off.pvp_flag, off.ffa_flag), (Some(false), Some(false)));
        assert!(!watch.flagged());
    }

    #[test]
    fn a_player_that_leaves_before_the_flag_keeps_no_flag_and_its_own_flag_stays() {
        const NOW_MS: u64 = 10_000;
        const SERVER_MS: u64 = 800_000_000_000;
        let mut watch = FfaWatch::default();
        watch.step(true, false, NOW_MS, SERVER_MS);
        let stopped = watch
            .step(false, false, NOW_MS + 1_000, SERVER_MS)
            .expect("the countdown ends");
        assert_eq!(
            (stopped.ffa_pending_end_ns, stopped.ffa_flag),
            (Some(0), None)
        );
        assert_eq!(
            watch.step(false, false, NOW_MS + FFA_ON_DELAY_MS, SERVER_MS),
            None
        );
        // A player with its own PvP flag on gets the zone's flag alone (retail `06 20 <0> 08
        // 01`) and keeps `pvpFlag` when the zone's flag goes off.
        let mut flagged = FfaWatch::default();
        flagged.step(true, true, NOW_MS, SERVER_MS);
        let on = flagged
            .step(true, true, NOW_MS + FFA_ON_DELAY_MS, SERVER_MS)
            .expect("the zone flag on");
        let mut retail_on = vec![0x06, 0x20];
        retail_on.extend([0; 8]);
        retail_on.extend([0x08, 0x01]);
        let MemberState::Recorded { body, .. } = on.member().expect("encode").state else {
            panic!("a recorded faction state");
        };
        assert_eq!(body, retail_on);
        flagged.step(false, true, NOW_MS + FFA_ON_DELAY_MS + 1, SERVER_MS);
        let off = flagged
            .step(
                false,
                true,
                NOW_MS + FFA_ON_DELAY_MS + 1 + FFA_OFF_DELAY_MS,
                SERVER_MS,
            )
            .expect("the zone flag off");
        assert_eq!((off.pvp_flag, off.ffa_flag), (None, Some(false)));
    }

    /// Retail `FFA_ExitFFAAndCleanse` 246.447 s: at the flag's end the cursed doubloons join the
    /// doubloon stack, the cursed hide becomes hide in the first free slot, the item without a
    /// conversion stays, and the client gets 6526 after the bundle.
    #[test]
    fn the_flag_off_converts_the_cursed_items_and_tells_the_client() {
        const CURSED_COIN: &str = "CK_PVPResource_DoubloonToken";
        const COIN: &str = "CK_PVEResource_DoubloonToken";
        const CURSED_HIDE: &str = "Cursed10ws_RawhideT52";
        const HIDE: &str = "RawhideT52";
        const KEEP: &str = "MadeUpKeepsake";
        const STACK: Option<u32> = Some(10_000);
        const COINS_HELD: u32 = 1_218;
        const CURSED_COINS: u32 = 27;
        const CURSED_HIDES: u32 = 39;
        const ENTERED_MS: u64 = 1_000;
        /// A free-for-all area (the retail client's 2297 named 0x23f5).
        const FFA_AREA: u16 = 0x23f5;
        let (mut world, entity) = world();
        let kits = crate::world::gear::tests::kits_with_stacks(&[
            (CURSED_COIN, "Resource", STACK),
            (COIN, "Resource", STACK),
            (CURSED_HIDE, "Resource", STACK),
            (HIDE, "Resource", STACK),
            (KEEP, "Resource", STACK),
        ]);
        {
            let content = Arc::make_mut(&mut world.content);
            content.ffa_areas = [FFA_AREA].into_iter().collect();
            content.ffa_conversions = [(CURSED_COIN, COIN), (CURSED_HIDE, HIDE)]
                .into_iter()
                .map(|(from, into)| (from.to_owned(), into.to_owned()))
                .collect();
            content.kits = kits.clone();
        }
        let item = |id: &str| kits.item(id).expect("item").clone();
        let slot_of = |gear: &crate::world::gear::Gear, id: &str| {
            gear.bag_stacks()
                .into_iter()
                .find(|(_, item_id, _, _)| item_id == id)
                .map(|(slot, ..)| slot)
        };
        let cursed_coin_slot = {
            let mut player = world.world.get_mut::<Player>(entity).expect("player");
            let mut gear = crate::world::gear::tests::gear_with_staff();
            assert!(gear.add(&item(COIN), COINS_HELD));
            assert!(gear.add(&item(CURSED_COIN), CURSED_COINS));
            assert!(gear.add(&item(KEEP), 1));
            assert!(gear.add(&item(CURSED_HIDE), CURSED_HIDES));
            let slot = slot_of(&gear, CURSED_COIN);
            player.gear = Some(gear);
            player.territories = vec![FFA_AREA];
            slot
        };
        world.tick_pvp_flags(ENTERED_MS);
        world.tick_pvp_flags(ENTERED_MS + FFA_ON_DELAY_MS);
        world
            .world
            .get_mut::<Player>(entity)
            .expect("player")
            .territories
            .clear();
        let left_ms = ENTERED_MS + FFA_ON_DELAY_MS + 1;
        world.tick_pvp_flags(left_ms);
        world.after_bundles.clear();
        world.tick_pvp_flags(left_ms + FFA_OFF_DELAY_MS);
        let player = world.world.get::<Player>(entity).expect("player");
        let gear = player.gear.as_ref().expect("gear");
        assert_eq!(gear.bag_count(COIN), COINS_HELD + CURSED_COINS);
        assert_eq!(gear.bag_count(HIDE), CURSED_HIDES);
        assert_eq!(gear.bag_count(KEEP), 1);
        assert_eq!(gear.bag_count(CURSED_COIN) + gear.bag_count(CURSED_HIDE), 0);
        assert_eq!(
            slot_of(gear, HIDE),
            cursed_coin_slot,
            "the hide takes the slot the merged doubloons freed"
        );
        assert_eq!(world.after_bundles.len(), 1, "6526 after the bundle");
        world.after_bundles.clear();
        world.tick_pvp_flags(left_ms + FFA_OFF_DELAY_MS * 2);
        assert!(world.after_bundles.is_empty(), "one 6526 per flag end");
    }
}
