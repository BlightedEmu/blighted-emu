// SPDX-License-Identifier: AGPL-3.0-or-later
//! The raids of this sim's wars and matches: each player of a war's roster or of an arena or
//! Outpost Rush team goes into the raid of its side or team. The social service owns the raid
//! (`nw_social` raids); this sim names it ([`RaidSpec`]) and tells the service when a player
//! joins and leaves ([`SocialInput::RaidJoin`], [`SocialInput::RaidLeave`]).
//!
//! Observed (findings `2026-11-11-group-finder-and-raids.md` and the 2026-11-12 war raid probe):
//!
//! - A war side's raid id is the side's number in the war data, and its other side's raid is the
//!   groups state's `opposingRaidId` (`arvaysa_ES_attacker_war_45vs45` 26.810 s: 9428 and 9429).
//!   The war data names the raid's ten group ids and its raid entity from the war's first state
//!   on (1.410 s, 925 s before the start), so the raid exists while the war waits.
//! - Before the move, the roster player gets an invite into its side's raid (25.108 s) and
//!   accepts it (3495, 26.724 s); the move's loading screen follows the accept by 57 ms (3951,
//!   26.781 s). This sim asks the social service for the invite ([`SocialInput::RaidInvite`])
//!   and keeps the move it stands for; the answer ([`RaidInviteAnswer`]) moves an accepting
//!   player in and drops a declined move.
//! - The war raid player joins at its move into the war (26.810 s, the move 26.781 s); an arena
//!   or Outpost Rush player at its arrival in the match (`20261010-125035-658` 298.790 s,
//!   `opr_match_2` 0.667 s) with its team's raid id of the game mode state, `raidType` 3 and no
//!   opposing raid.
//! - Capture the flag sends the same at the arrival (`CTF01` 317.411 s, after the instance's
//!   1635 at 317.353 s): `raidType` 3, the team's raid id (3017; the other team 3018), no
//!   opposing raid, the player's group the raid's first; its team mates in groups 0 and 1.
//! - An expedition and a catacombs run make one raid of the instance at the arrival (`EXP-Amrine
//!   Excavation` 3.278 s, `EXP-The-Depths` 4.043 s, `EXP-Garden-of-Genesis` 9.514 s,
//!   `EXP-Savage-Divide` 4.224 s, `CATACOMBS` 11.711 s, `CATACOMBS3` 0.707 s): `raidType` 3, no
//!   opposing raid, every player in the raid's first group (Amrine: 5 members, the raid entity at
//!   5.607 s). A player that leaves the run leaves its raid group (5283 to the others, Amrine
//!   283.037 s to 299.824 s).
//! - A soul trial makes no raid: the own groups state has no raid from the arrival to the exit
//!   (`Soul-Trial---Break-some-Eggs` 8.759 s, `Soul-Trial---Daichi-Saito` 11.829 s,
//!   `Soul-Trial---Eternal-Frost` 6.255 s, `Soul-Trial---The-Winter-Rune-Forge` 7.770 s, no
//!   class 28 in any of them).
//! - The leave clears the raid with the instance's exit (`20261010-125035-658` 516.078 s, the
//!   exit's 3951 at 516.051 s).
//!
//! Inferred: the raid entity's id and the group ids are name-based ids of the war (a war raid:
//! every sim names the same ones without a store) or of the instance (a match team); a war
//! player's raid group is its roster group (both have ten groups of five); a match player's raid
//! group is its team place divided by the group size (Observed: the arena team of
//! `20261010-125035-658` in group 0, the Outpost Rush player of `opr_match_2` in group 2); a run
//! player's raid group is its arrival order divided by the group size (Observed: group 0 for the
//! five of Amrine), and a player that comes back keeps its group; a run's raid id is
//! [`RAID_ID_BASE`] (retail numbers 1658, 3354, 5178 are counters of its servers; one raid per
//! instance, so the raid entity, named under the instance, tells two runs apart).
//!
//! **Pre-raid status** (715, Observed in all 22 of 13 ledgers: a status `u32`, then the war id):
//! the war roster's changes reach the player they are about, `army-management` 24.033 s to
//! 68.215 s ([`PRE_RAID_SELECTED`] as the player came onto the roster, then
//! [`PRE_RAID_PERMISSION_PROMOTED`], [`PRE_RAID_PERMISSION_DEMOTED`], [`PRE_RAID_UNSELECTED`] as
//! it left, again selected and unselected), `RS_offensive_war` (unselected 0.402 s, selected
//! 63.838 s); [`PRE_RAID_WAR_ENDED`] goes to the players in the war 7 ms before the war's end
//! (245; `everfall_war_2` 258.434 s, `RS_offensive_war` 957.259 s, `Reekwater_Invasion`
//! 956.482 s, `Playthrough_Part_21` 3322.250 s, and in `CK_offensive_war_2`,
//! `arvaysa_ES_attacker_war_45vs45`, `joining_defending_war_2`, `nighthaven_defensive_war`).
//! A player whose roster seat did not change gets none (`arvaysa_ES_attacker_war_45vs45`: none
//! before its invite). The invasions send theirs ([`super::invasions`]). Inferred: every sim
//! finds the roster changes in its wars read ([`super::wars::Wars::read`]) and tells its own
//! players; a permission change is the roster's stored permission (a company rank is no roster
//! change); the sim's first read tells nothing (the roster was as it is).

use std::collections::HashMap;

use bevy_ecs::entity::Entity;
use nw_bus::{RaidInviteAnswer, RaidSpec, SimSlot, SocialInput};
use nw_db::{RosterSeat, TerritoryWar, WarSignup};
use nw_messages::as_dynamic_raw_id;
use nw_messages::social::GroupPush;
use nw_messages::social::group_data::GROUP_SIZE;
use nw_messages::social::groups::{
    PRE_RAID_PERMISSION_DEMOTED, PRE_RAID_PERMISSION_PROMOTED, PRE_RAID_SELECTED,
    PRE_RAID_UNSELECTED, PRE_RAID_WAR_ENDED,
};
use nw_messages::social::raid_data::{RAID_TYPE_GAME_MODE, groups_of};
use nw_sim_core::Position;
use uuid::Uuid;

use nw_content::instances::InstanceKind;

use super::arena_match::RAID_ID_BASE;
use super::{Player, SimWorld};

/// The opposing raid of a match team (Observed: none in the arena and Outpost Rush).
const NO_OPPOSING_RAID: u64 = 0;

/// The move into a war instance that a roster player's raid invite stands for.
#[derive(Debug, Clone, PartialEq)]
struct WarMove {
    war: Uuid,
    /// The instance, its sim and its phase.
    target: (Uuid, SimSlot, u64),
    arrival: Position,
}

/// This sim's raid traffic: the inputs for the social service and the war moves of the raid
/// invites it asked for, by character.
#[derive(Debug, Default)]
pub(super) struct SimRaids {
    inputs: Vec<SocialInput>,
    war_moves: HashMap<Uuid, WarMove>,
    /// The raid groups of the players of this sim's expedition or catacombs run.
    run_groups: HashMap<Uuid, u8>,
}

/// The raid `raid` of type `kind` named under `namespace` (a war or an instance id), with the
/// other side's raid `opposing` (module doc).
pub(super) fn raid_spec(namespace: Uuid, raid: u64, kind: u8, opposing: u64) -> RaidSpec {
    let raid_name = raid.to_be_bytes();
    let groups = (0..groups_of(kind) as u64)
        .map(|index| {
            let mut name = raid_name.to_vec();
            name.extend_from_slice(&index.to_be_bytes());
            Uuid::new_v5(&namespace, &name)
        })
        .collect();
    RaidSpec {
        raid,
        kind,
        opposing,
        entity: as_dynamic_raw_id(*Uuid::new_v5(&namespace, &raid_name).as_bytes()),
        groups,
    }
}

impl SimWorld {
    /// The raid joins, leaves and invites since the last call, for the social service.
    pub fn take_raid_inputs(&mut self) -> Vec<SocialInput> {
        std::mem::take(&mut self.raids.inputs)
    }

    /// The director opened war `war`'s instance `target`: roster player `entity` gets the
    /// invite into its side's raid, and this sim keeps its move to `arrival` until the answer
    /// (module doc); a player on no roster gets none. The moves of wars with a result go.
    pub(super) fn invite_to_war_raid(
        &mut self,
        entity: Entity,
        war: Uuid,
        target: (Uuid, SimSlot, u64),
        arrival: Position,
    ) {
        let wars = &self.wars;
        self.raids
            .war_moves
            .retain(|_, open| wars.war(open.war).is_some_and(|war| war.result.is_none()));
        let Some(character) = self
            .world
            .get::<Player>(entity)
            .map(|player| player.character)
        else {
            return;
        };
        let Some((raid, group)) = self.wars.raid_seat(character) else {
            return;
        };
        self.raids.war_moves.insert(
            character,
            WarMove {
                war,
                target,
                arrival,
            },
        );
        self.raids.inputs.push(SocialInput::RaidInvite {
            character,
            raid,
            group,
            activity: war,
        });
        self.invasion_invited(entity, war);
    }

    /// The social service's answers to this sim's raid invites: an accepting player of a war
    /// without a result moves in (it joins the raid at the move); a declined move goes.
    pub(super) fn raid_invites_answered(&mut self, answers: &[RaidInviteAnswer]) {
        for answer in answers {
            let Some(open) = self
                .raids
                .war_moves
                .remove(&answer.character)
                .filter(|open| open.war == answer.activity)
            else {
                continue;
            };
            let running = self
                .wars
                .war(open.war)
                .is_some_and(|war| war.result.is_none());
            if !answer.accepted || !running {
                continue;
            }
            if let Some(entity) = self.player_of_character(answer.character) {
                self.enter_war_instance(entity, open.target, open.arrival);
            }
        }
    }

    fn join_raid(&mut self, entity: Entity, raid: RaidSpec, group: u8) {
        let Some(character) = self
            .world
            .get::<Player>(entity)
            .map(|player| player.character)
        else {
            return;
        };
        self.raids.inputs.push(SocialInput::RaidJoin {
            character,
            raid,
            group,
        });
    }

    /// Player `entity` moves into its war: it joins its side's raid in its roster group (module
    /// doc); a player on no roster joins none.
    pub(super) fn join_war_raid(&mut self, entity: Entity) {
        let Some(character) = self
            .world
            .get::<Player>(entity)
            .map(|player| player.character)
        else {
            return;
        };
        if let Some((raid, group)) = self.wars.raid_seat(character) {
            self.join_raid(entity, raid, group);
        }
    }

    /// Player `entity` arrived in the arena or Outpost Rush match this instance sim runs: it
    /// joins its team's raid (module doc); outside a match it joins none.
    pub(super) fn join_match_raid(&mut self, entity: Entity) {
        let Some(number) = self.world.get::<Player>(entity).map(|player| player.number) else {
            return;
        };
        let Some(instance) = self.instance_run.as_ref().map(|run| run.instance()) else {
            return;
        };
        let Some((raid, place)) = self.arena_matches.raid_seat_of(number) else {
            return;
        };
        let group = place / u8::try_from(GROUP_SIZE).unwrap_or(u8::MAX);
        let spec = raid_spec(instance, raid, RAID_TYPE_GAME_MODE, NO_OPPOSING_RAID);
        self.join_raid(entity, spec, group);
    }

    /// Player `entity` arrived in the expedition or catacombs run this instance sim runs: it
    /// joins the run's raid in the group of its arrival order (module doc); in another instance
    /// (a soul trial, a raid trial) it joins none.
    pub(super) fn join_run_raid(&mut self, entity: Entity) {
        let Some(instance) = self.instance_run.as_ref() else {
            return;
        };
        let (id, mode) = (instance.instance(), instance.game_mode());
        let runs_raid = self.content.instances.by_id(mode).is_some_and(|mode| {
            matches!(
                mode.kind,
                InstanceKind::Expedition | InstanceKind::Catacombs
            )
        });
        let Some(character) = self
            .world
            .get::<Player>(entity)
            .map(|player| player.character)
        else {
            return;
        };
        if !runs_raid {
            return;
        }
        let spec = raid_spec(id, RAID_ID_BASE, RAID_TYPE_GAME_MODE, NO_OPPOSING_RAID);
        let last_group = u8::try_from(spec.groups.len().saturating_sub(1)).unwrap_or(u8::MAX);
        let seated = self.raids.run_groups.len() / GROUP_SIZE;
        let next = u8::try_from(seated).unwrap_or(u8::MAX).min(last_group);
        let group = *self.raids.run_groups.entry(character).or_insert(next);
        self.join_raid(entity, spec, group);
    }

    /// Player `entity` leaves its instance: it leaves the raid it is in (module doc).
    pub(super) fn leave_raid(&mut self, entity: Entity) {
        let Some((character, raid)) = self
            .world
            .get::<Player>(entity)
            .and_then(|player| Some((player.character, player.social.raid()?)))
        else {
            return;
        };
        self.raids
            .inputs
            .push(SocialInput::RaidLeave { character, raid });
    }

    /// Sends this sim's players the pre-raid statuses of the change of the wars from `before`
    /// to the wars read now (module doc); nothing for an empty `before` (no change or the first
    /// read).
    pub(super) fn send_pre_raid_statuses(&mut self, before: &[TerritoryWar]) {
        if before.is_empty() {
            return;
        }
        let statuses = pre_raid_statuses(before, self.wars.all());
        if statuses.is_empty() {
            return;
        }
        let players: Vec<(Entity, Uuid, u64)> = self
            .by_connection
            .values()
            .filter_map(|entity| {
                let player = self.world.get::<Player>(*entity)?;
                Some((*entity, player.character, player.number))
            })
            .collect();
        for (character, status, war) in statuses {
            for (entity, _, number) in players.iter().filter(|(_, known, _)| *known == character) {
                self.send_group_push(
                    *entity,
                    *number,
                    GroupPush::PreRaidStatus {
                        status,
                        raid: *war.as_bytes(),
                    },
                );
            }
        }
    }

    /// Player `entity` (`number`) is in war `war` as it ends: the pre-raid status
    /// [`PRE_RAID_WAR_ENDED`], sent before the war's end (module doc). An invasion's end sends its
    /// own ([`super::invasions`]).
    pub(super) fn send_war_ended_status(&mut self, entity: Entity, number: u64, war: Uuid) {
        if self.wars.war(war).is_some_and(TerritoryWar::is_invasion) {
            return;
        }
        self.send_group_push(
            entity,
            number,
            GroupPush::PreRaidStatus {
                status: PRE_RAID_WAR_ENDED,
                raid: *war.as_bytes(),
            },
        );
    }
}

/// The stored roster permission of `character` in `war` while it holds a roster place; `None`
/// off the roster or kicked.
fn roster_permission(war: &TerritoryWar, character: Uuid) -> Option<u8> {
    war.signups
        .iter()
        .find(|signup| signup.character == character && !signup.kicked)
        .and_then(|WarSignup { roster, .. }| match roster {
            RosterSeat {
                place: Some(_),
                permission,
            } => Some(*permission),
            RosterSeat { place: None, .. } => None,
        })
}

/// The pre-raid statuses of the roster changes from `before` to `after`, as (character, status,
/// war): of each war without a result that is no invasion (module doc).
pub(super) fn pre_raid_statuses(
    before: &[TerritoryWar],
    after: &[TerritoryWar],
) -> Vec<(Uuid, u32, Uuid)> {
    let mut statuses = Vec::new();
    for war in after
        .iter()
        .filter(|war| war.result.is_none() && !war.is_invasion())
    {
        let old = before.iter().find(|old| old.war == war.war);
        let mut characters: Vec<Uuid> = war.signups.iter().map(|signup| signup.character).collect();
        if let Some(old) = old {
            characters.extend(old.signups.iter().map(|signup| signup.character));
        }
        characters.sort_unstable();
        characters.dedup();
        for character in characters {
            let was = old.and_then(|old| roster_permission(old, character));
            let status = match (was, roster_permission(war, character)) {
                (None, Some(_)) => Some(PRE_RAID_SELECTED),
                (Some(_), None) => Some(PRE_RAID_UNSELECTED),
                (Some(was), Some(now)) if now > was => Some(PRE_RAID_PERMISSION_PROMOTED),
                (Some(was), Some(now)) if now < was => Some(PRE_RAID_PERMISSION_DEMOTED),
                _ => None,
            };
            if let Some(status) = status {
                statuses.push((character, status, war.war));
            }
        }
    }
    statuses
}

#[cfg(test)]
mod tests {
    use nw_messages::social::raid_data::{EVENT_RAID_GROUPS, RAID_TYPE_WAR};

    use super::*;
    use crate::world::tests::{NOW, SERVER_TIME, admit_at, world_with_spawns};
    use crate::world::wars::tests::{signup, war};

    const PLAYER: u64 = 3;
    /// The roster group of the test player.
    const ROSTER_GROUP: u8 = 2;

    /// A world with one admitted player on the roster of a war without a result.
    fn roster_world() -> (SimWorld, Entity, Uuid, Uuid) {
        let mut world = world_with_spawns(Vec::new());
        let admit = admit_at(PLAYER, 0.0, true);
        world.admit(&admit, NOW, SERVER_TIME).expect("admit");
        let entry = war(vec![signup(admit.character, None, Some((ROSTER_GROUP, 0)))]);
        let war_id = entry.war;
        world.wars.read(Ok(vec![entry]));
        let entity = world.player_by_number(PLAYER).expect("player");
        (world, entity, admit.character, war_id)
    }

    fn target() -> (Uuid, SimSlot, u64) {
        let sim = SimSlot {
            id: nw_bus::BusId::try_from("inst-1".to_owned()).expect("id"),
            generation: 1,
        };
        (Uuid::from_u128(0x1a), sim, 9)
    }

    const ARRIVAL: Position = Position {
        x: 1.0,
        y: 2.0,
        z: 3.0,
    };

    fn answer(character: Uuid, activity: Uuid, accepted: bool) -> RaidInviteAnswer {
        RaidInviteAnswer {
            character,
            activity,
            accepted,
        }
    }

    #[test]
    fn a_roster_player_gets_the_raid_invite_and_moves_on_its_accept() {
        let (mut world, entity, character, war_id) = roster_world();
        world.invite_to_war_raid(entity, war_id, target(), ARRIVAL);
        let inputs = world.take_raid_inputs();
        let [
            SocialInput::RaidInvite {
                character: invited,
                raid,
                group,
                activity,
            },
        ] = inputs.as_slice()
        else {
            panic!("one raid invite: {inputs:?}");
        };
        assert_eq!(*invited, character);
        assert_eq!(*group, ROSTER_GROUP);
        assert_eq!(*activity, war_id);
        assert_eq!(raid.kind, RAID_TYPE_WAR);
        // An answer of another activity moves nothing and keeps no move.
        world.raid_invites_answered(&[answer(character, Uuid::from_u128(1), true)]);
        assert!(world.take_raid_inputs().is_empty());
        assert!(world.raids.war_moves.is_empty());
        world.invite_to_war_raid(entity, war_id, target(), ARRIVAL);
        world.take_raid_inputs();
        world.raid_invites_answered(&[answer(character, war_id, true)]);
        let inputs = world.take_raid_inputs();
        assert!(
            matches!(inputs.as_slice(), [SocialInput::RaidJoin { group, .. }] if *group == ROSTER_GROUP),
            "the move joins the raid: {inputs:?}"
        );
        assert!(world.raids.war_moves.is_empty());
    }

    #[test]
    fn a_declined_raid_invite_moves_nothing_and_a_player_off_the_roster_gets_none() {
        let (mut world, entity, character, war_id) = roster_world();
        world.invite_to_war_raid(entity, war_id, target(), ARRIVAL);
        world.take_raid_inputs();
        world.raid_invites_answered(&[answer(character, war_id, false)]);
        assert!(world.take_raid_inputs().is_empty(), "no move, no join");
        assert!(world.raids.war_moves.is_empty());
        // A player on no roster gets no invite.
        let mut other = war(Vec::new());
        other.war = Uuid::from_u128(0x78);
        world.wars.read(Ok(vec![other]));
        world.invite_to_war_raid(entity, war_id, target(), ARRIVAL);
        assert!(world.take_raid_inputs().is_empty());
        assert!(world.raids.war_moves.is_empty());
    }

    /// The catacombs instance sim of the test content with six arriving players, and its
    /// instance.
    fn run_world() -> (SimWorld, Uuid) {
        use crate::world::catacombs::tests::{catacombs, content};
        use crate::world::tests::{settings, slot};
        const PLAYERS: u64 = GROUP_SIZE as u64 + 1;
        const PHASE: u64 = 0x5e;
        let mode = catacombs();
        let mut settings = settings(PLAYERS as usize, PLAYERS as u32);
        settings.instance_role = true;
        settings.level = nw_messages::LevelSpec::instance(
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
            std::sync::Arc::new(content()),
        );
        let instance = Uuid::from_u128(0x1b);
        let admits: Vec<_> = (0..PLAYERS)
            .map(|offset| {
                let mut admit = admit_at(PLAYER + offset, 492.0, false);
                admit.progression.arena.stay = Some(nw_bus::InstanceStay {
                    instance,
                    game_mode: mode.id.clone(),
                    phase: PHASE,
                    return_level: "newworld_vitaeeterna".to_owned(),
                    return_position: nw_bus::StoredPoint::default(),
                    activated_ns: 1,
                    mutation_difficulty: None,
                    seat: None,
                });
                admit
            })
            .collect();
        let members = admits.iter().map(|admit| admit.character).collect();
        assert_eq!(
            world.open_instance(instance, &mode.id, PHASE, members, None, NOW),
            nw_bus::InstanceOpened::Opened
        );
        for admit in &admits {
            world.admit(admit, NOW, SERVER_TIME).expect("admit");
        }
        (world, instance)
    }

    #[test]
    fn run_players_join_the_instance_raid_five_to_a_group() {
        let (mut world, instance) = run_world();
        let spec = raid_spec(
            instance,
            RAID_ID_BASE,
            RAID_TYPE_GAME_MODE,
            NO_OPPOSING_RAID,
        );
        let joins: Vec<(RaidSpec, u8)> = world
            .take_raid_inputs()
            .into_iter()
            .filter_map(|input| match input {
                SocialInput::RaidJoin { raid, group, .. } => Some((raid, group)),
                _ => None,
            })
            .collect();
        let groups: Vec<u8> = joins.iter().map(|(_, group)| *group).collect();
        assert_eq!(
            groups,
            vec![0, 0, 0, 0, 0, 1],
            "five to a group in arrival order"
        );
        assert!(
            joins.iter().all(|(raid, _)| *raid == spec),
            "one raid of the instance"
        );
        assert_eq!(spec.opposing, NO_OPPOSING_RAID);
    }

    #[test]
    fn a_raid_spec_is_the_same_on_every_sim_and_differs_by_raid() {
        let war = Uuid::from_u128(5);
        let first = raid_spec(war, 9428, RAID_TYPE_WAR, 9429);
        assert_eq!(first, raid_spec(war, 9428, RAID_TYPE_WAR, 9429));
        assert_eq!(first.groups.len(), EVENT_RAID_GROUPS);
        let distinct: std::collections::BTreeSet<Uuid> = first.groups.iter().copied().collect();
        assert_eq!(distinct.len(), EVENT_RAID_GROUPS, "ten different groups");
        let other = raid_spec(war, 9429, RAID_TYPE_WAR, 9428);
        assert_ne!(first.entity, other.entity);
        assert!(
            first
                .groups
                .iter()
                .all(|group| !other.groups.contains(group))
        );
        assert_eq!(
            first.entity,
            as_dynamic_raw_id(first.entity),
            "a dynamic id"
        );
    }

    /// The pre-raid statuses `world` sent to the test player since the last call.
    fn pre_raid_sent(world: &mut SimWorld) -> Vec<(u32, [u8; 16])> {
        use nw_messages::social::SocialPush;
        use nw_messages::social::groups::PRE_RAID_STATUS_TYPE_INDEX;
        use nw_reflect::Envelope;

        std::mem::take(&mut world.combat_messages)
            .into_iter()
            .filter(|out| out.connection == crate::world::tests::connection(PLAYER))
            .flat_map(|out| out.envelopes)
            .filter_map(|bytes| {
                let envelope = Envelope::decode(&bytes).ok()?;
                let type_index = envelope.type_index()?;
                (type_index == PRE_RAID_STATUS_TYPE_INDEX).then_some(())?;
                match SocialPush::decode(type_index, &envelope.body)?.ok()? {
                    (_, SocialPush::Group(GroupPush::PreRaidStatus { status, raid })) => {
                        Some((status, raid))
                    }
                    _ => None,
                }
            })
            .collect()
    }

    #[test]
    fn roster_changes_give_the_retail_pre_raid_statuses() {
        use nw_messages::war_roster::{PERMISSION_MEMBER, PERMISSION_OFFICER};

        let character = Uuid::from_u128(0x51);
        let seated = |permission: u8| {
            let mut entry = signup(character, None, Some((ROSTER_GROUP, 0)));
            entry.roster.permission = permission;
            war(vec![entry])
        };
        let off = war(vec![signup(character, None, None)]);
        let steps = [
            (&off, seated(PERMISSION_MEMBER), Some(PRE_RAID_SELECTED)),
            (
                &seated(PERMISSION_MEMBER),
                seated(PERMISSION_OFFICER),
                Some(PRE_RAID_PERMISSION_PROMOTED),
            ),
            (
                &seated(PERMISSION_OFFICER),
                seated(PERMISSION_MEMBER),
                Some(PRE_RAID_PERMISSION_DEMOTED),
            ),
            (
                &seated(PERMISSION_MEMBER),
                off.clone(),
                Some(PRE_RAID_UNSELECTED),
            ),
            (
                &seated(PERMISSION_MEMBER),
                war(Vec::new()),
                Some(PRE_RAID_UNSELECTED),
            ),
            (&seated(PERMISSION_MEMBER), seated(PERMISSION_MEMBER), None),
        ];
        for (before, after, expected) in steps {
            let id = after.war;
            assert_eq!(
                pre_raid_statuses(std::slice::from_ref(before), &[after]),
                expected
                    .map(|status| vec![(character, status, id)])
                    .unwrap_or_default()
            );
        }
        let mut kicked = seated(PERMISSION_MEMBER);
        kicked.signups[0].kicked = true;
        assert_eq!(
            pre_raid_statuses(&[seated(PERMISSION_MEMBER)], &[kicked]),
            vec![(character, PRE_RAID_UNSELECTED, Uuid::from_u128(0x77))],
            "a kick leaves the roster"
        );
        let mut ended = seated(PERMISSION_MEMBER);
        ended.result = Some((Uuid::from_u128(1), 1));
        assert!(
            pre_raid_statuses(std::slice::from_ref(&off), &[ended]).is_empty(),
            "a war with a result"
        );
        let mut invasion = seated(PERMISSION_MEMBER);
        invasion.attacker.0 = Uuid::nil();
        assert!(invasion.is_invasion());
        assert!(
            pre_raid_statuses(&[off], &[invasion]).is_empty(),
            "the invasions send theirs"
        );
    }

    #[test]
    fn a_read_roster_change_reaches_the_player_and_the_war_end_sends_war_ended() {
        let mut world = world_with_spawns(Vec::new());
        let admit = admit_at(PLAYER, 0.0, true);
        world.admit(&admit, NOW, SERVER_TIME).expect("admit");
        let off = war(vec![signup(admit.character, None, None)]);
        let id = off.war;
        world.wars_done(Vec::new(), Ok(vec![off.clone()]));
        world.combat_messages.clear();
        let seated = war(vec![signup(admit.character, None, Some((ROSTER_GROUP, 0)))]);
        world.wars_done(Vec::new(), Ok(vec![seated.clone()]));
        assert_eq!(
            pre_raid_sent(&mut world),
            vec![(PRE_RAID_SELECTED, *id.as_bytes())]
        );
        world.wars_done(Vec::new(), Ok(vec![seated]));
        assert!(pre_raid_sent(&mut world).is_empty(), "no change, no status");
        let entity = world.player_by_number(PLAYER).expect("player");
        world.send_war_ended_status(entity, PLAYER, id);
        assert_eq!(
            pre_raid_sent(&mut world),
            vec![(PRE_RAID_WAR_ENDED, *id.as_bytes())]
        );
        let mut invasion = off;
        invasion.attacker.0 = Uuid::nil();
        world.wars_done(Vec::new(), Ok(vec![invasion]));
        world.combat_messages.clear();
        world.send_war_ended_status(entity, PLAYER, id);
        assert!(
            pre_raid_sent(&mut world).is_empty(),
            "an invasion's end sends its own"
        );
    }

    #[test]
    fn the_first_wars_read_sends_no_pre_raid_status() {
        let mut world = world_with_spawns(Vec::new());
        let admit = admit_at(PLAYER, 0.0, true);
        world.admit(&admit, NOW, SERVER_TIME).expect("admit");
        world.combat_messages.clear();
        let seated = war(vec![signup(admit.character, None, Some((ROSTER_GROUP, 0)))]);
        world.wars_done(Vec::new(), Ok(vec![seated]));
        assert!(
            pre_raid_sent(&mut world).is_empty(),
            "the roster was as it is"
        );
    }
}
