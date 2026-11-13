// SPDX-License-Identifier: AGPL-3.0-or-later
//! Players' raids (raid type 4): a group that grows into a raid of four groups by the
//! convert-to-raid vote, its leader and assistants, its invites, moves, swaps and leaves.
//!
//! Observed, the raid (`raid_group_influence_race`, `Playthrough_Part_18`, `WURM`,
//! `Hive gorgon raid third boss`, `arvaysa_gorgon_raid`, `Gorgon`, `Hatchery`, the elite beasts
//! quest ledger; findings `2026-11-12-wars-and-sieges.md`, "Raids - sixth pass"):
//!
//! - A member's own groups state (1994) names its raid group as `groupId`, the raid id, `raidType`
//!   4 and `createSource` [`PLAYER_RAID_CREATE_SOURCE`] (8 of 8 ledgers); the raid entity (28)
//!   has `lifetime` 0, four group ids and up to 20 members (`WURM`, `Playthrough_Part_18`: 0x14).
//!   After the raid the fields are zero, the create source too (`Playthrough_Part_18`
//!   6612.115 s).
//! - The raid's group entities carry the raid group index, the icon and the colour index; the
//!   member flags mark the raid leader (`0x02`) and its assistants (`0x04`)
//!   ([`nw_messages::social::group_data`]).
//! - A raid invite (`Playthrough_Part_18` 1868.823 s) is a group invite whose group is the
//!   inviter's chosen raid group and whose raid is the raid id, with the inviter named; the
//!   accept (3495, 1877.425 s) ends it with the 1677 of a group invite, and the invitee came into
//!   the raid's last group (index 3, 1877.509 s): the named group 0 and groups 1 and 2 held five
//!   each.
//! - A raid move (3501) puts a member into another raid group (`raid_group_influence_race`
//!   43.292 s to 65.476 s, by the player after its block gained the assistant bit at 39.778 s).
//! - The permission call (2422) to make a member the leader (`Hive gorgon raid third boss`
//!   230.534 s) gave that member the leader bit and the old leader the assistant bit 54 ms later.
//!
//! Observed, the client (no ledger holds the vote: 5308 to 5311, 5317 and 5318 are in none):
//! the wire forms of [`nw_messages::social::groups`]; `socialmenu.luac` offers "Convert to Raid"
//! to every member of a group outside a raid (not in an arena or an expedition) and sends 5317;
//! `socialscreen.luac` shows the vote of 5311 to the others with "Accept (Default)" and "Deny"
//! until the vote's end time, sends their choice with 5318, and shows the 5308, 5309 and 5310
//! errors; `playerflyouthandler.luac` gives the promote and demote choices to the raid leader
//! only, a promote one step up (`Normal`, `Assistant`, `Leader`). Localization: the player that
//! starts the vote becomes the raid's leader; a raid holds `maxRaidSize` players (the menu's
//! `raidGroupMaxMemberCount` 20).
//!
//! Inferred (no capture or client value): a vote converts the group when every member accepted,
//! or at its end when no member denied ("Accept (Default)"); one deny ends it with
//! [`CONVERT_TO_RAID_NOT_CONVERTED`]; a vote lasts [`RAID_VOTE_MS`] and a group starts one at
//! most every [`RAID_VOTE_COOLDOWN_MS`] (the client's `javelin.convert-to-raid-cooldown-ms` has
//! no value in the retail remote config). The converted group's members fill raid group 0, whose
//! id is the group's id; the group ends (its row is deleted, its open invites end). The leader
//! and assistants invite (a member's invite names the raid group to fill; another raid group
//! or none means the inviter's own), move and swap; an invitee fills the named group, or else
//! the first group with room; a move into a full group is refused (the client swaps then). The
//! leader sets permissions: a new leader makes the old one an assistant (Observed above), a leader
//! that leaves hands the raid to its first assistant or first member, and the raid ends when one
//! member is left. A players' raid keeps an offline member (a group does), but lives in the
//! service only: a restart of the service ends it. Raid ids count from [`PLAYER_RAID_ID_BASE`].

use std::collections::HashMap;

use nw_bus::RaidSpec;
use nw_messages::as_dynamic_raw_id;
use nw_messages::social::groups::{
    CONVERT_TO_RAID_ALREADY_VOTED, CONVERT_TO_RAID_CONVERTED, CONVERT_TO_RAID_NOT_CONVERTED,
    CONVERT_TO_RAID_TOO_FREQUENT, CONVERT_TO_RAID_VOTE_IN_PROGRESS,
    CONVERT_TO_RAID_VOTE_NOT_IN_PROGRESS, RAID_PERMISSION_ASSISTANT, RAID_PERMISSION_LEADER,
    RAID_PERMISSION_NORMAL,
};
use nw_messages::social::raid_data::{PLAYER_RAID_GROUPS, RAID_TYPE_PLAYER};
use nw_messages::social::{CharacterRef, GroupPush, SocialPush};
use uuid::Uuid;

use crate::effects::{Effect, StoreOp};
use crate::groups::GROUP_MEMBER_LIMIT;
use crate::raids::Raid;
use crate::world::{GroupInvite, METRIC_CALLS, REFUSED, RESULT_LABEL, SocialWorld};

/// The groups state's `createSource` of a players' raid member (Observed in 8 of 8 ledgers,
/// module doc).
pub(crate) const PLAYER_RAID_CREATE_SOURCE: u8 = 2;
/// The first raid id of a players' raid. Inferred: retail's numbers (819 to 7004) are counters of
/// its servers; this range stays apart from the sims' raids, which count from `0x200`.
pub(crate) const PLAYER_RAID_ID_BASE: u64 = 0x1_0000;
/// How long a convert-to-raid vote stays open (Inferred, module doc).
pub(crate) const RAID_VOTE_MS: u64 = 30_000;
/// The shortest time between two vote starts of one group (Inferred, module doc).
pub(crate) const RAID_VOTE_COOLDOWN_MS: u64 = 60_000;
/// The most members of a players' raid: four groups of five (Observed, module doc).
pub(crate) const PLAYER_RAID_SIZE: usize = PLAYER_RAID_GROUPS * GROUP_MEMBER_LIMIT;
/// The opposing raid of a players' raid: none.
const NO_OPPOSING_RAID: u64 = 0;
/// The raid group a converted group fills.
const CONVERTED_GROUP: u8 = 0;
/// Nanoseconds in a millisecond: the vote's end goes out in Unix nanoseconds.
const NANOS_PER_MS: u64 = 1_000_000;

/// An open convert-to-raid vote of a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RaidVote {
    /// The member that started it: the raid's leader.
    pub(crate) initiator: Uuid,
    /// When it ends, Unix milliseconds.
    pub(crate) ends_ms: u64,
    /// Each member's vote so far (`true` accepts); the initiator's is an accept.
    pub(crate) votes: HashMap<Uuid, bool>,
}

/// The permission `member` has in `raid`.
fn permission_of(raid: &Raid, member: Uuid) -> u8 {
    if raid.leader == Some(member) {
        RAID_PERMISSION_LEADER
    } else if raid.assistants.contains(&member) {
        RAID_PERMISSION_ASSISTANT
    } else {
        RAID_PERMISSION_NORMAL
    }
}

fn refused() {
    metrics::counter!(METRIC_CALLS, RESULT_LABEL => REFUSED).increment(1);
}

impl SocialWorld {
    /// The raid entity id of the players' raid of `character`; `None` outside one.
    pub(crate) fn player_raid_of(&self, character: Uuid) -> Option<[u8; 16]> {
        let (raid, _) = self.raid_seat(character)?;
        (raid.spec.kind == RAID_TYPE_PLAYER).then_some(raid.spec.entity)
    }

    /// The group of `character` and its members, when it is in one.
    fn group_members(&self, character: Uuid) -> Option<(Uuid, Vec<Uuid>)> {
        let group = self.people.get(&character)?.group?;
        let members = self.groups.get(&group)?.members.clone();
        members.contains(&character).then_some((group, members))
    }

    /// 5317: `character` starts the vote to turn its group into a raid (module doc). A vote
    /// already open gets a 5308 [`CONVERT_TO_RAID_VOTE_IN_PROGRESS`], a start within
    /// [`RAID_VOTE_COOLDOWN_MS`] of the group's last one a 5308 [`CONVERT_TO_RAID_TOO_FREQUENT`];
    /// a character in no group is refused.
    pub(crate) fn initiate_convert_to_raid(&mut self, character: Uuid, now_ms: u64) {
        let Some((group, members)) = self.group_members(character) else {
            refused();
            return;
        };
        let error = if self.raid_votes.contains_key(&group) {
            Some(CONVERT_TO_RAID_VOTE_IN_PROGRESS)
        } else if self
            .raid_vote_starts
            .get(&group)
            .is_some_and(|last| now_ms < last.saturating_add(RAID_VOTE_COOLDOWN_MS))
        {
            Some(CONVERT_TO_RAID_TOO_FREQUENT)
        } else {
            None
        };
        if let Some(error) = error {
            let push = GroupPush::InitiateConvertToRaidFailed { error };
            self.push(&[character], SocialPush::Group(push));
            return;
        }
        self.raid_vote_starts.insert(group, now_ms);
        let ends_ms = now_ms.saturating_add(RAID_VOTE_MS);
        self.raid_votes.insert(
            group,
            RaidVote {
                initiator: character,
                ends_ms,
                votes: HashMap::from([(character, true)]),
            },
        );
        let push = GroupPush::ConvertToRaidVoteInitiated {
            initiator: CharacterRef(Some(*character.as_bytes())),
            ends_ns: ends_ms.saturating_mul(NANOS_PER_MS),
        };
        self.push(&members, SocialPush::Group(push));
        self.settle_raid_vote(group, false);
    }

    /// 5318: the vote `accept` of `character` (module doc). No open vote of its group gets a
    /// 5310 [`CONVERT_TO_RAID_VOTE_NOT_IN_PROGRESS`], a second vote a 5310
    /// [`CONVERT_TO_RAID_ALREADY_VOTED`].
    pub(crate) fn vote_convert_to_raid(&mut self, character: Uuid, accept: bool, now_ms: u64) {
        let group = self.group_members(character).map(|(group, _)| group);
        let error = match group.and_then(|group| self.raid_votes.get_mut(&group)) {
            None => Some(CONVERT_TO_RAID_VOTE_NOT_IN_PROGRESS),
            Some(vote) if now_ms >= vote.ends_ms => Some(CONVERT_TO_RAID_VOTE_NOT_IN_PROGRESS),
            Some(vote) if vote.votes.contains_key(&character) => {
                Some(CONVERT_TO_RAID_ALREADY_VOTED)
            }
            Some(vote) => {
                vote.votes.insert(character, accept);
                None
            }
        };
        match (error, group) {
            (Some(error), _) => {
                let push = GroupPush::SubmitConvertToRaidVoteFailed { error };
                self.push(&[character], SocialPush::Group(push));
            }
            (None, Some(group)) => self.settle_raid_vote(group, false),
            (None, None) => {}
        }
    }

    /// Ends the votes whose time ran out at Unix ms `now_ms` (module doc: no deny converts);
    /// the effects of the ends.
    pub fn end_due_raid_votes(&mut self, now_ms: u64) -> Vec<Effect> {
        let due: Vec<Uuid> = self
            .raid_votes
            .iter()
            .filter(|(_, vote)| now_ms >= vote.ends_ms)
            .map(|(group, _)| *group)
            .collect();
        for group in due {
            self.settle_raid_vote(group, true);
        }
        self.refresh_views(now_ms);
        std::mem::take(&mut self.out)
    }

    /// Ends the vote of `group` when a member denied, every member accepted, or its time ran out
    /// (`timed_out`): the members get the 5309 and a converting group becomes a raid. A vote of a
    /// group that ended goes without a word.
    fn settle_raid_vote(&mut self, group: Uuid, timed_out: bool) {
        let Some(vote) = self.raid_votes.get(&group) else {
            return;
        };
        let Some(members) = self.groups.get(&group).map(|group| group.members.clone()) else {
            self.raid_votes.remove(&group);
            return;
        };
        let denied = members
            .iter()
            .any(|member| vote.votes.get(member) == Some(&false));
        let all_accepted = members
            .iter()
            .all(|member| vote.votes.get(member) == Some(&true));
        if !denied && !all_accepted && !timed_out {
            return;
        }
        let initiator = vote.initiator;
        self.raid_votes.remove(&group);
        let result = if denied {
            CONVERT_TO_RAID_NOT_CONVERTED
        } else {
            CONVERT_TO_RAID_CONVERTED
        };
        let push = GroupPush::ConvertToRaidVoteEnded { result };
        self.push(&members, SocialPush::Group(push));
        if !denied {
            self.convert_group_to_raid(group, initiator);
        }
    }

    /// Turns `group` into a players' raid led by `initiator` (or by the group's leader when the
    /// initiator left it): the members fill raid group 0 in their group order (module doc).
    fn convert_group_to_raid(&mut self, group: Uuid, initiator: Uuid) {
        let Some(known) = self.groups.remove(&group) else {
            return;
        };
        self.store(StoreOp::DeleteGroup(group));
        let leader = if known.members.contains(&initiator) {
            initiator
        } else {
            known.leader
        };
        for member in &known.members {
            self.drop_group_invites_of(*member);
            if let Some(person) = self.people.get_mut(member) {
                person.group = None;
            }
        }
        let raid = self.next_player_raid;
        self.next_player_raid = self.next_player_raid.saturating_add(1);
        let entity = as_dynamic_raw_id(*Uuid::new_v4().as_bytes());
        let spec = RaidSpec {
            raid,
            kind: RAID_TYPE_PLAYER,
            opposing: NO_OPPOSING_RAID,
            entity,
            groups: std::iter::once(group)
                .chain((1..PLAYER_RAID_GROUPS).map(|_| Uuid::new_v4()))
                .collect(),
        };
        self.raids.insert(
            entity,
            Raid {
                spec: spec.clone(),
                seats: Vec::new(),
                leader: Some(leader),
                assistants: Vec::new(),
            },
        );
        for member in known.members {
            self.join_raid(member, spec.clone(), CONVERTED_GROUP);
        }
    }

    /// Invites `invitee` into the players' raid of `inviter`, to fill the raid group of id
    /// `group` first (module doc). An inviter without the permission, an invitee that blocks the
    /// inviter or is in a group or a raid, and a full raid get the 3453.
    pub(crate) fn invite_to_player_raid(
        &mut self,
        inviter: Uuid,
        invitee: Uuid,
        group: [u8; 16],
        now_ms: u64,
    ) {
        let Some((raid, own)) = self.raid_seat(inviter) else {
            return;
        };
        let may = permission_of(raid, inviter) >= RAID_PERMISSION_ASSISTANT;
        let full = raid.seats.len() >= PLAYER_RAID_SIZE;
        let fill = raid
            .spec
            .groups
            .iter()
            .position(|id| id.as_bytes() == &group)
            .and_then(|index| u8::try_from(index).ok())
            .unwrap_or(own);
        let busy = self
            .people
            .get(&invitee)
            .is_none_or(|person| person.group.is_some() || person.raid.is_some());
        let blocked = self
            .people
            .get(&invitee)
            .is_some_and(|person| person.blocks.contains(&inviter));
        if !may || full || busy || blocked {
            refused();
            self.refuse_group(inviter, invitee);
            return;
        }
        let mut id = [0; 16];
        id.copy_from_slice(Uuid::new_v4().as_bytes());
        self.group_invites.insert(
            id,
            GroupInvite {
                inviter,
                invitee,
                created_ms: now_ms,
                raid_group: Some(fill),
            },
        );
        self.touch(inviter);
        self.touch(invitee);
    }

    /// The accepted raid invite of `inviter`: `invitee` joins its players' raid in raid group
    /// `fill`, or else in the first group with room (module doc). A full raid, an inviter that
    /// left its raid and an invitee that joined a group or a raid meanwhile get the 3453.
    pub(crate) fn join_player_raid(&mut self, inviter: Uuid, invitee: Uuid, fill: u8) {
        let busy = self
            .people
            .get(&invitee)
            .is_none_or(|person| person.group.is_some() || person.raid.is_some());
        let seat = self
            .player_raid_of(inviter)
            .and_then(|entity| self.raids.get(&entity))
            .filter(|raid| !busy && raid.seats.len() < PLAYER_RAID_SIZE)
            .and_then(|raid| {
                let room = |index: u8| raid.members_of(index).len() < GROUP_MEMBER_LIMIT;
                let groups =
                    (0..raid.spec.groups.len()).filter_map(|index| u8::try_from(index).ok());
                let group = std::iter::once(fill)
                    .chain(groups)
                    .find(|index| usize::from(*index) < raid.spec.groups.len() && room(*index))?;
                Some((raid.spec.clone(), group))
            });
        let Some((spec, group)) = seat else {
            refused();
            self.refuse_group(invitee, inviter);
            return;
        };
        self.join_raid(invitee, spec, group);
    }

    /// 2252 of a players' raid member: it leaves the raid ([`Self::leave_raid`]).
    pub(crate) fn leave_player_raid(&mut self, character: Uuid) {
        if let Some(entity) = self.player_raid_of(character) {
            self.leave_raid(character, entity);
        }
    }

    /// The players' raid of `character` when it may arrange the raid (its leader or an
    /// assistant), and `member`'s seat index in it.
    fn arranged_seat(&self, character: Uuid, member: Uuid) -> Option<([u8; 16], usize)> {
        let entity = self.player_raid_of(character)?;
        let raid = self.raids.get(&entity)?;
        if permission_of(raid, character) < RAID_PERMISSION_ASSISTANT {
            return None;
        }
        let at = raid.seats.iter().position(|(known, _)| *known == member)?;
        Some((entity, at))
    }

    /// Tells every member of raid `entity` that the raid changed.
    fn touch_raid(&mut self, entity: [u8; 16]) {
        let members: Vec<Uuid> = self
            .raids
            .get(&entity)
            .map(|raid| raid.seats.iter().map(|(member, _)| *member).collect())
            .unwrap_or_default();
        for member in members {
            self.touch(member);
        }
    }

    /// 3501: `character` moves `member` to the end of raid group `group` (module doc). Without
    /// the permission, for a member or a group the raid lacks, or into a full group: refused.
    pub(crate) fn move_raid_member(&mut self, character: Uuid, member: &str, group: u8) {
        let seat = Uuid::parse_str(member)
            .ok()
            .and_then(|member| self.arranged_seat(character, member));
        let Some((entity, at)) = seat else {
            refused();
            return;
        };
        let Some(raid) = self.raids.get_mut(&entity) else {
            return;
        };
        let fits = usize::from(group) < raid.spec.groups.len()
            && raid.seats[at].1 != group
            && raid.members_of(group).len() < GROUP_MEMBER_LIMIT;
        if !fits {
            refused();
            return;
        }
        let (moved, _) = raid.seats.remove(at);
        raid.seats.push((moved, group));
        self.touch_raid(entity);
    }

    /// 541: `character` swaps the places of two members of its players' raid (module doc).
    /// Without the permission or for a member the raid lacks: refused.
    pub(crate) fn swap_raid_members(&mut self, character: Uuid, first: &str, second: &str) {
        let seats = Uuid::parse_str(first)
            .ok()
            .zip(Uuid::parse_str(second).ok())
            .and_then(|(first, second)| {
                let (entity, one) = self.arranged_seat(character, first)?;
                let (_, two) = self.arranged_seat(character, second)?;
                Some((entity, one, two))
            });
        let Some((entity, one, two)) = seats else {
            refused();
            return;
        };
        let Some(raid) = self.raids.get_mut(&entity) else {
            return;
        };
        let (group_one, group_two) = (raid.seats[one].1, raid.seats[two].1);
        raid.seats.swap(one, two);
        raid.seats[one].1 = group_one;
        raid.seats[two].1 = group_two;
        self.touch_raid(entity);
    }

    /// 2422: the raid leader `character` gives `member` the permission `permission` (module
    /// doc). Another caller, the leader itself, a member the raid lacks or an unknown permission:
    /// refused.
    pub(crate) fn set_raid_permission(&mut self, character: Uuid, member: &str, permission: u8) {
        let entity = self.player_raid_of(character);
        let target = Uuid::parse_str(member)
            .ok()
            .filter(|member| *member != character);
        let raid = entity.and_then(|entity| self.raids.get_mut(&entity));
        let (Some(raid), Some(member)) = (raid, target) else {
            refused();
            return;
        };
        let fits = raid.leader == Some(character)
            && raid.seats.iter().any(|(known, _)| *known == member)
            && permission <= RAID_PERMISSION_LEADER;
        if !fits {
            refused();
            return;
        }
        raid.assistants.retain(|assistant| *assistant != member);
        match permission {
            RAID_PERMISSION_LEADER => {
                raid.leader = Some(member);
                raid.assistants.push(character);
            }
            RAID_PERMISSION_ASSISTANT => raid.assistants.push(member),
            _ => {}
        }
        let entity = raid.spec.entity;
        self.touch_raid(entity);
    }
}

#[cfg(test)]
mod tests {
    use nw_bus::{LeaveReason, SocialInput, SocialStateKind};
    use nw_messages::payload::MemberState;
    use nw_messages::social::group_data::{FLAG_CONNECTED, FLAG_RAID_ASSISTANT, FLAG_RAID_LEADER};
    use nw_messages::social::groups::{
        CONVERT_TO_RAID_VOTE_ENDED_TYPE_INDEX, CONVERT_TO_RAID_VOTE_INITIATED_TYPE_INDEX,
        CONVERT_TO_RAID_VOTE_TYPE_INDEX, GROUP_ERROR_TYPE_INDEX, GROUP_INVITE_TYPE_INDEX,
        INITIATE_CONVERT_TO_RAID_FAILED_TYPE_INDEX, INITIATE_CONVERT_TO_RAID_TYPE_INDEX,
        LEAVE_GROUP_TYPE_INDEX, LEAVE_VALUE, MOVE_RAID_MEMBER_TYPE_INDEX, REPLY_ACCEPT,
        REPLY_TO_INVITE_TYPE_INDEX, SET_RAID_PERMISSION_TYPE_INDEX,
        SUBMIT_CONVERT_TO_RAID_VOTE_FAILED_TYPE_INDEX, SWAP_RAID_MEMBERS_TYPE_INDEX,
    };
    use nw_messages::social::raid_data::{PLAYER_RAID_LIFETIME, RAID_DATA_TYPE_INDEX};
    use nw_messages::social::states::{GROUPS_STATE_TYPE_INDEX, GroupsState};

    use super::*;
    use crate::groups::tests::invite_args;
    use crate::world::tests::{NOW_MS, call, character, join, pushes, text, world};

    const ON: &str = "sim-a";
    /// Members of a raid of four full groups.
    const FULL_RAID: u128 = PLAYER_RAID_SIZE as u128;

    fn send(world: &mut SocialWorld, from: u128, type_index: u64, args: &[u8]) -> Vec<Effect> {
        call(world, from, ON, type_index, args, NOW_MS)
    }

    fn accept_last_invite(world: &mut SocialWorld, invitee: u128) -> Vec<Effect> {
        let invite = *world
            .group_invites
            .iter()
            .find(|(_, open)| open.invitee == character(invitee))
            .expect("an open invite")
            .0;
        let mut reply = invite.to_vec();
        reply.extend(REPLY_ACCEPT.to_be_bytes());
        send(world, invitee, REPLY_TO_INVITE_TYPE_INDEX, &reply)
    }

    /// A world with characters 1 to `count` online and 1 to `grouped` in the group of 1.
    fn grouped(count: u128, grouped: u128) -> SocialWorld {
        let mut world = world();
        for number in 1..=count {
            join(&mut world, number, &format!("P{number}"), ON);
        }
        for invitee in 2..=grouped {
            send(
                &mut world,
                1,
                GROUP_INVITE_TYPE_INDEX,
                &invite_args(invitee),
            );
            accept_last_invite(&mut world, invitee);
        }
        world
    }

    fn vote(world: &mut SocialWorld, from: u128, accept: bool) -> Vec<Effect> {
        send(
            world,
            from,
            CONVERT_TO_RAID_VOTE_TYPE_INDEX,
            &[u8::from(accept)],
        )
    }

    /// Characters 1 to `members` in a players' raid led by 1 (converted from a group of three,
    /// the rest invited by 1 into group 0).
    fn raid_of(members: u128) -> (SocialWorld, [u8; 16]) {
        let mut world = grouped(members, 3);
        send(&mut world, 1, INITIATE_CONVERT_TO_RAID_TYPE_INDEX, &[]);
        vote(&mut world, 2, true);
        vote(&mut world, 3, true);
        let entity = world.player_raid_of(character(1)).expect("a raid");
        for invitee in 4..=members {
            send(
                &mut world,
                1,
                GROUP_INVITE_TYPE_INDEX,
                &invite_args(invitee),
            );
            accept_last_invite(&mut world, invitee);
        }
        (world, entity)
    }

    fn groups_state(effects: &[Effect], to: Uuid) -> Option<GroupsState> {
        effects.iter().rev().find_map(|effect| match effect {
            Effect::OwnState { delivery, .. }
                if delivery.character == to && delivery.kind == SocialStateKind::Groups =>
            {
                match MemberState::decode(GROUPS_STATE_TYPE_INDEX, &delivery.body) {
                    Ok(MemberState::Groups(state)) => Some(state),
                    _ => None,
                }
            }
            _ => None,
        })
    }

    fn group_push(effects: &[Effect], type_index: u64) -> Vec<(Vec<Uuid>, GroupPush)> {
        pushes(effects, type_index)
            .into_iter()
            .filter_map(|(to, push)| match push {
                SocialPush::Group(push) => Some((to, push)),
                _ => None,
            })
            .collect()
    }

    fn seat_of(world: &SocialWorld, member: u128) -> u8 {
        world.raid_seat(character(member)).expect("a seat").1
    }

    #[test]
    fn a_vote_that_every_member_accepts_turns_the_group_into_a_players_raid() {
        let mut world = grouped(3, 3);
        let group = world.people[&character(1)].group.expect("a group");
        let effects = send(&mut world, 2, INITIATE_CONVERT_TO_RAID_TYPE_INDEX, &[]);
        let started = group_push(&effects, CONVERT_TO_RAID_VOTE_INITIATED_TYPE_INDEX);
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].0, [character(1), character(2), character(3)]);
        assert_eq!(
            started[0].1,
            GroupPush::ConvertToRaidVoteInitiated {
                initiator: CharacterRef(Some(*character(2).as_bytes())),
                ends_ns: (NOW_MS + RAID_VOTE_MS) * NANOS_PER_MS,
            }
        );
        assert!(
            world.player_raid_of(character(1)).is_none(),
            "the vote is open"
        );
        vote(&mut world, 1, true);
        let effects = vote(&mut world, 3, true);
        let ended = group_push(&effects, CONVERT_TO_RAID_VOTE_ENDED_TYPE_INDEX);
        assert_eq!(
            ended,
            vec![(
                vec![character(1), character(2), character(3)],
                GroupPush::ConvertToRaidVoteEnded {
                    result: CONVERT_TO_RAID_CONVERTED
                }
            )]
        );
        assert!(world.groups.is_empty(), "the group became the raid");
        let entity = world.player_raid_of(character(3)).expect("a players' raid");
        let raid = &world.raids[&entity];
        assert_eq!(raid.leader, Some(character(2)), "the starter leads");
        assert_eq!(raid.spec.groups.len(), PLAYER_RAID_GROUPS);
        assert_eq!(raid.spec.groups[0], group, "raid group 0 is the group");
        assert_eq!(raid.spec.raid, PLAYER_RAID_ID_BASE);
        let state = groups_state(&effects, character(1)).expect("groups state");
        assert_eq!(state.group_id, Some(*group.as_bytes()));
        assert_eq!(state.raid_id, Some(PLAYER_RAID_ID_BASE));
        assert_eq!(state.raid_type, Some(RAID_TYPE_PLAYER));
        assert_eq!(state.create_source, Some(PLAYER_RAID_CREATE_SOURCE));
        let view = world.raid_view(character(1)).expect("raid entity");
        assert_eq!(view.state.lifetime, Some(PLAYER_RAID_LIFETIME));
        assert_eq!(view.state.members, Some(3));
        assert_eq!(view.groups.len(), PLAYER_RAID_GROUPS - 1);
        let (_, entity_state) = world
            .own_raid_group_entity(character(1))
            .expect("group entity");
        assert_eq!(
            entity_state.members[1].flags,
            Some([FLAG_CONNECTED | FLAG_RAID_LEADER, 0]),
            "the leader's bit"
        );
        assert!(
            deliveries_of_class(&effects, RAID_DATA_TYPE_INDEX),
            "the raid entity went out"
        );
    }

    /// Whether `effects` deliver a raid entity whose body reads as class `class`.
    fn deliveries_of_class(effects: &[Effect], class: u64) -> bool {
        effects.iter().any(|effect| match effect {
            Effect::RaidEntity { delivery, .. } => delivery.view.as_ref().is_some_and(|view| {
                matches!(
                    MemberState::decode(class, &view.body),
                    Ok(MemberState::RaidData(_))
                )
            }),
            _ => false,
        })
    }

    #[test]
    fn a_deny_ends_the_vote_and_the_errors_answer_wrong_steps() {
        let mut world = grouped(3, 3);
        let effects = send(&mut world, 1, INITIATE_CONVERT_TO_RAID_TYPE_INDEX, &[]);
        assert_eq!(
            group_push(&effects, CONVERT_TO_RAID_VOTE_INITIATED_TYPE_INDEX).len(),
            1
        );
        let effects = send(&mut world, 2, INITIATE_CONVERT_TO_RAID_TYPE_INDEX, &[]);
        assert_eq!(
            group_push(&effects, INITIATE_CONVERT_TO_RAID_FAILED_TYPE_INDEX),
            vec![(
                vec![character(2)],
                GroupPush::InitiateConvertToRaidFailed {
                    error: CONVERT_TO_RAID_VOTE_IN_PROGRESS
                }
            )]
        );
        let effects = vote(&mut world, 1, true);
        assert_eq!(
            group_push(&effects, SUBMIT_CONVERT_TO_RAID_VOTE_FAILED_TYPE_INDEX)[0].1,
            GroupPush::SubmitConvertToRaidVoteFailed {
                error: CONVERT_TO_RAID_ALREADY_VOTED
            },
            "the starter voted with its start"
        );
        let effects = vote(&mut world, 2, false);
        assert_eq!(
            group_push(&effects, CONVERT_TO_RAID_VOTE_ENDED_TYPE_INDEX)[0].1,
            GroupPush::ConvertToRaidVoteEnded {
                result: CONVERT_TO_RAID_NOT_CONVERTED
            }
        );
        assert!(world.player_raid_of(character(1)).is_none());
        assert_eq!(world.groups.len(), 1, "the group stays");
        let effects = vote(&mut world, 3, true);
        assert_eq!(
            group_push(&effects, SUBMIT_CONVERT_TO_RAID_VOTE_FAILED_TYPE_INDEX)[0].1,
            GroupPush::SubmitConvertToRaidVoteFailed {
                error: CONVERT_TO_RAID_VOTE_NOT_IN_PROGRESS
            }
        );
        let effects = send(&mut world, 3, INITIATE_CONVERT_TO_RAID_TYPE_INDEX, &[]);
        assert_eq!(
            group_push(&effects, INITIATE_CONVERT_TO_RAID_FAILED_TYPE_INDEX)[0].1,
            GroupPush::InitiateConvertToRaidFailed {
                error: CONVERT_TO_RAID_TOO_FREQUENT
            }
        );
        let effects = call(
            &mut world,
            3,
            ON,
            INITIATE_CONVERT_TO_RAID_TYPE_INDEX,
            &[],
            NOW_MS + RAID_VOTE_COOLDOWN_MS,
        );
        assert_eq!(
            group_push(&effects, CONVERT_TO_RAID_VOTE_INITIATED_TYPE_INDEX).len(),
            1,
            "after the cooldown"
        );
    }

    #[test]
    fn a_vote_without_a_deny_converts_at_its_end_and_a_lone_player_cannot_start_one() {
        let mut world = grouped(4, 3);
        let effects = send(&mut world, 4, INITIATE_CONVERT_TO_RAID_TYPE_INDEX, &[]);
        assert!(
            group_push(&effects, CONVERT_TO_RAID_VOTE_INITIATED_TYPE_INDEX).is_empty(),
            "no group"
        );
        send(&mut world, 1, INITIATE_CONVERT_TO_RAID_TYPE_INDEX, &[]);
        vote(&mut world, 2, true);
        let effects = world.end_due_raid_votes(NOW_MS + RAID_VOTE_MS - 1);
        assert!(group_push(&effects, CONVERT_TO_RAID_VOTE_ENDED_TYPE_INDEX).is_empty());
        let effects = world.end_due_raid_votes(NOW_MS + RAID_VOTE_MS);
        assert_eq!(
            group_push(&effects, CONVERT_TO_RAID_VOTE_ENDED_TYPE_INDEX)[0].1,
            GroupPush::ConvertToRaidVoteEnded {
                result: CONVERT_TO_RAID_CONVERTED
            },
            "Accept is the default"
        );
        assert!(world.player_raid_of(character(3)).is_some());
        assert!(world.raid_votes.is_empty());
    }

    #[test]
    fn raid_invites_fill_the_named_group_then_the_first_with_room_up_to_twenty() {
        let (mut world, entity) = raid_of(5);
        assert!((1..=5).all(|member| seat_of(&world, member) == 0));
        join(&mut world, 6, "P6", ON);
        let group_two = world.raids[&entity].spec.groups[2];
        let mut args = text(&character(6).hyphenated().to_string());
        args.extend(group_two.as_bytes());
        args.push(0);
        let effects = send(&mut world, 1, GROUP_INVITE_TYPE_INDEX, &args);
        let state = groups_state(&effects, character(6)).expect("groups state");
        let invite = &state.inbound_invites.expect("inbound").elements[0];
        assert_eq!(invite.group, *group_two.as_bytes());
        assert_eq!(invite.raid, PLAYER_RAID_ID_BASE);
        assert_eq!(invite.inviter, CharacterRef(Some(*character(1).as_bytes())));
        accept_last_invite(&mut world, 6);
        assert_eq!(seat_of(&world, 6), 2, "the named group");
        join(&mut world, 7, "P7", ON);
        send(&mut world, 1, GROUP_INVITE_TYPE_INDEX, &invite_args(7));
        accept_last_invite(&mut world, 7);
        assert_eq!(
            seat_of(&world, 7),
            1,
            "group 0 is full: the first with room"
        );
        for number in 8..=FULL_RAID {
            join(&mut world, number, &format!("P{number}"), ON);
            send(&mut world, 1, GROUP_INVITE_TYPE_INDEX, &invite_args(number));
            accept_last_invite(&mut world, number);
        }
        assert_eq!(world.raids[&entity].seats.len(), PLAYER_RAID_SIZE);
        join(&mut world, FULL_RAID + 1, "Late", ON);
        let effects = send(
            &mut world,
            1,
            GROUP_INVITE_TYPE_INDEX,
            &invite_args(FULL_RAID + 1),
        );
        assert_eq!(
            group_push(&effects, GROUP_ERROR_TYPE_INDEX).len(),
            1,
            "a full raid"
        );
        let effects = send(
            &mut world,
            2,
            GROUP_INVITE_TYPE_INDEX,
            &invite_args(FULL_RAID + 1),
        );
        assert_eq!(
            group_push(&effects, GROUP_ERROR_TYPE_INDEX).len(),
            1,
            "a member without the permission"
        );
    }

    #[test]
    fn the_leader_sets_permissions_and_assistants_move_and_swap_members() {
        let (mut world, entity) = raid_of(5);
        let mut move_args = text(&character(5).hyphenated().to_string());
        move_args.push(1);
        send(&mut world, 2, MOVE_RAID_MEMBER_TYPE_INDEX, &move_args);
        assert_eq!(seat_of(&world, 5), 0, "a normal member may not move");
        let mut promote = text(&character(2).hyphenated().to_string());
        promote.push(RAID_PERMISSION_ASSISTANT);
        send(&mut world, 3, SET_RAID_PERMISSION_TYPE_INDEX, &promote);
        assert!(
            world.raids[&entity].assistants.is_empty(),
            "only the leader"
        );
        send(&mut world, 1, SET_RAID_PERMISSION_TYPE_INDEX, &promote);
        assert_eq!(world.raids[&entity].assistants, [character(2)]);
        send(&mut world, 2, MOVE_RAID_MEMBER_TYPE_INDEX, &move_args);
        assert_eq!(seat_of(&world, 5), 1, "the assistant moved it");
        let (_, state) = world
            .own_raid_group_entity(character(2))
            .expect("group entity");
        assert_eq!(
            state.members[1].flags,
            Some([FLAG_CONNECTED | FLAG_RAID_ASSISTANT, 0])
        );
        let swap = [
            text(&character(5).hyphenated().to_string()),
            text(&character(3).hyphenated().to_string()),
        ]
        .concat();
        send(&mut world, 2, SWAP_RAID_MEMBERS_TYPE_INDEX, &swap);
        assert_eq!((seat_of(&world, 5), seat_of(&world, 3)), (0, 1));
        let mut lead = text(&character(4).hyphenated().to_string());
        lead.push(RAID_PERMISSION_LEADER);
        send(&mut world, 1, SET_RAID_PERMISSION_TYPE_INDEX, &lead);
        let raid = &world.raids[&entity];
        assert_eq!(raid.leader, Some(character(4)));
        assert_eq!(
            raid.assistants,
            [character(2), character(1)],
            "the old leader is an assistant"
        );
        let mut demote = text(&character(2).hyphenated().to_string());
        demote.push(RAID_PERMISSION_NORMAL);
        send(&mut world, 4, SET_RAID_PERMISSION_TYPE_INDEX, &demote);
        assert_eq!(world.raids[&entity].assistants, [character(1)]);
        let mut past = text(&character(5).hyphenated().to_string());
        past.push(u8::try_from(PLAYER_RAID_GROUPS).expect("small"));
        send(&mut world, 4, MOVE_RAID_MEMBER_TYPE_INDEX, &past);
        assert_eq!(seat_of(&world, 5), 0, "a group the raid lacks");
    }

    #[test]
    fn leaving_hands_on_the_lead_and_the_raid_ends_with_one_member_left() {
        let (mut world, entity) = raid_of(3);
        let mut promote = text(&character(3).hyphenated().to_string());
        promote.push(RAID_PERMISSION_ASSISTANT);
        send(&mut world, 1, SET_RAID_PERMISSION_TYPE_INDEX, &promote);
        let leave = LEAVE_VALUE.to_be_bytes();
        send(&mut world, 1, LEAVE_GROUP_TYPE_INDEX, &leave);
        assert_eq!(
            world.raids[&entity].leader,
            Some(character(3)),
            "the first assistant"
        );
        assert!(world.raids[&entity].assistants.is_empty());
        let effects = send(&mut world, 3, LEAVE_GROUP_TYPE_INDEX, &leave);
        assert!(world.raids.is_empty(), "one member left ends the raid");
        assert!(world.player_raid_of(character(2)).is_none());
        let state = groups_state(&effects, character(2)).expect("groups state");
        assert_eq!(state.raid_id, Some(0));
        assert_eq!(state.raid_type, Some(0));
        assert_eq!(
            state.create_source,
            Some(0),
            "the retail zeros after a raid"
        );
    }

    #[test]
    fn an_offline_member_stays_in_its_players_raid() {
        let (mut world, entity) = raid_of(3);
        world.input(
            &crate::world::tests::sim(ON),
            SocialInput::Left {
                character: character(2),
                reason: LeaveReason::Logout,
            },
            NOW_MS,
        );
        assert_eq!(world.raids[&entity].seats.len(), 3);
        let (_, state) = world
            .own_raid_group_entity(character(1))
            .expect("group entity");
        assert_eq!(state.members[1].flags, Some([0, 0]), "offline");
        assert!(world.people[&character(2)].group.is_none());
    }
}
