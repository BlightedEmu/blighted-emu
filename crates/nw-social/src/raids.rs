// SPDX-License-Identifier: AGPL-3.0-or-later
//! Raids: the groups of a war side or a match team, and the raid entity each member gets
//! (`RaidDataComponentReplicatedState`, type 28; findings `2026-11-11-group-finder-and-raids.md`
//! and the 2026-11-12 war raid probe).
//!
//! Observed, the war raid of `arvaysa_ES_attacker_war_45vs45` (seconds from its first record):
//!
//! - 26.810 s, after the raid invite's accept: the own groups state (1994) names the raid group
//!   as `groupId`, the raid as `raidId` (9428, the war side's number), the other side's raid as
//!   `opposingRaidId` (9429) and `raidType` 1.
//! - 27.039 s: the raid's ten group entities (3451, each with its index in the raid's group list
//!   as raid group, icon and colour index) and the raid entity (28, whole: raid id, type, two
//!   members, lifetime 1, the ten group ids) with its card; then a `numMembers` change for each
//!   member that came (29.232 s to 42.598 s: 3, 4, 5, 6, 7).
//! - The arena (`20261010-125035-658` 298.790 s and 300.038 s) and Outpost Rush (`opr_match_2`
//!   0.667 s) send the same at the arrival in the instance, with `raidType` 3 and no
//!   `opposingRaidId`; the invasion (`invasion` 62.077 s) with `raidType` 2.
//! - The leave (`20261010-125035-658` 516.078 s, `EXP-Amrine Excavation` 379.230 s): the own
//!   groups state sets `groupId` all zero, `raidId` 0 and `raidType` 0.
//!
//! The war raid invite (`arvaysa_ES_attacker_war_45vs45`, `everfall_war` and `invasion`): at
//! 25.108 s, 1.7 s before the move, the own groups state lists one inbound invite with the
//! raid group as its group, the raid id, kind 0, no inviter, the player as its invitee, the war
//! id as its activity, source 4 and every flag clear; the client accepts it with 3495 (26.724
//! s), the service ends it with a 1677 that names no other player (26.777 s), and the move's
//! loading screen (3951) follows 4 ms later. The sim of the roster player asks for the invite
//! ([`nw_bus::SocialInput::RaidInvite`]); the service tells that sim the answer
//! ([`nw_bus::RaidInviteAnswer`]) and the sim moves an accepting player in, where it joins the
//! raid.
//!
//! A raid has one voice channel for all its groups (Observed: the 4200 answers of
//! `Hive gorgon raid third boss` named one channel for raid groups `ff2f3897...` and
//! `2c956e23...` of raid 5861 from 2.3 s to 2237.1 s, `Hatchery` one for groups `366f6d03...` and
//! `977be94d...` of raid 3058, `Playthrough_Part_21` one for groups `73b38ea7...` and
//! `7bb51eda...` of invasion raid 7004; the client asks again (4204) when its `groupId`
//! changes). A war's or a match's raid gets a new channel at its first member; a players' raid
//! keeps the channel of the group it grew from (Inferred: no ledger holds a conversion).
//!
//! A raid group has a group leader: its group entity sets the leader bit (`0x01`) of the member
//! flags on it ([`RaidGroupLead`]). Observed with a walk of every 3451 body of the 122 ledgers
//! that hold the class (tools repo `claimprobe` `raidflags`, 0 bodies refused):
//!
//! - The member that takes a seat in a raid group with no members gets the bit; when the
//!   holder leaves the group, the first member left in it (join order) gets it at once
//!   (`opr_match_2` 385.197 s, `raid_group_influence_race` 24.182 s).
//! - A game mode team (raid type 3: expeditions, catacombs, arenas, capture the flag, Outpost
//!   Rush) keeps it to the end: one holder per raid group in every such ledger (`OPR_TEST3`
//!   216.658 s, `opr_match_2`, `OPR-coral-divide2`, the `CATACOMBS` and `EXP-` ledgers).
//! - A war or invasion raid (types 1 and 2) clears it again: 24 seats in empty groups of
//!   `arvaysa_ES_attacker_war_45vs45`, `CK_won_offensive_war`, `everfall_war`,
//!   `RS_offensive_war` and `Playthrough_Part_21` lost it 0.169 s to 2.56 s later, 13 of them
//!   0.80 s to 0.85 s ([`RAID_GROUP_LEAD_SHOWN_MS`]).
//! - A players' raid (type 4): four seats in empty groups (`Hive gorgon raid third boss`,
//!   `arvaysa_gorgon_raid_3`) lost it 0.565 s to 0.783 s later; the raid leader and the
//!   first members of the first groups held it from the ledger start for 10 s to 564 s
//!   (`arvaysa_gorgon_raid`, `Hive gorgon raid first boss`, `Hatchery`, `WURM`,
//!   `Playthrough_Part_18`, `raid_group_influence_race`) and lost it at a change of another
//!   member's flags or a mount enable, which this service does not tie to the group.
//!   Inferred: a players' raid clears it after [`RAID_GROUP_LEAD_SHOWN_MS`] as the seats in
//!   empty groups show, and at its conversion the raid leader takes the lead of raid group 0.
//!
//! The client's raid window never shows the bit (`raidmember.luac` calls `SetLeaderStatus`
//! with false only); the group HUD (`grouphud.luac` `OnMemberLeaderStatusChanged`) writes the
//! local player's `Hud.LocalPlayer.Group.IsGroupLeader`, which pings and chat read. A raid
//! loaded from the database shows no new lead (Inferred: its seats were shown before the
//! restart).
//!
//! The service knows a raid by its raid entity id ([`RaidSpec::entity`], a name-based id of the
//! war or the instance): the raid ids are numbers each sim counts and can repeat across sims.
//!
//! The sim that runs the war or the match names the raid ([`RaidSpec`]) and the player's group
//! in it ([`nw_bus::SocialInput::RaidJoin`]); the service makes the raid at its first member and
//! ends it with its last one. A member's own group is its raid group: the group entity it gets
//! is that group's, and the raid entity comes with the raid's other groups
//! ([`nw_bus::RaidEntityDelivery`]).
//!
//! Inferred: a player that joins a raid keeps its own group in the service, but its groups
//! state names the raid group (a groups state holds one `groupId`) and its group entity is the
//! raid group's; it shows its own group again when it leaves the raid (the expedition ledgers
//! show the raid group from the arrival to the exit, and no player there leaves its group when
//! it enters); a raid member is refused group invites (both ways); a member that goes offline
//! leaves its raid (no capture shows a raid member's logout); a raid member's leave is told to the other members of
//! its raid group with the retail group leave (5283), as a group leave is. A raid member's raid
//! move (3501) or swap (541) is refused: the war's roster (1676) or the match's teams place the
//! players. A declined raid invite ends with the same 1677 and the player stays where it is; a
//! raid invite ends when its invitee goes offline, as a group invite does, or leaves the sim
//! that asked for it (the move starts from that sim); a new raid invite of the same activity
//! takes the place of the open one.

use nw_bus::{BusId, RaidInviteAnswer, RaidSpec};
use nw_messages::social::group_data::{GroupDataState, GroupMemberView};
use nw_messages::social::groups::{
    INVITE_ENDED_ANSWERED, REMOVED_KICKED, REMOVED_LEFT, REPLY_ACCEPT,
};
use nw_messages::social::raid_data::{RAID_TYPE_GAME_MODE, RAID_TYPE_PLAYER};
use nw_messages::social::raid_data::{RaidDataState, groups_of};
use nw_messages::social::states::{GROUP_INVITE_KIND, GROUP_INVITE_SOURCE, GroupInviteData};
use nw_messages::social::{CharacterRef, GroupPush, SocialPush};
use uuid::Uuid;

use crate::effects::Effect;
use crate::groups::INVITE_REMOVED_FLAG;
use crate::player_raids::PLAYER_RAID_CREATE_SOURCE;
use crate::voice::new_group_channel;
use crate::world::{METRIC_CALLS, REFUSED, RESULT_LABEL, SocialWorld};

/// Nanoseconds in a millisecond: an invite's time is Unix nanoseconds.
const NANOS_PER_MS: u64 = 1_000_000;

/// An open raid invite (module doc): no inviter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RaidInvite {
    /// The sim that asked for it: the answer goes there.
    pub(crate) sim: BusId,
    pub(crate) invitee: Uuid,
    pub(crate) spec: RaidSpec,
    /// The raid group, from 0.
    pub(crate) group: u8,
    /// The activity (the war id).
    pub(crate) activity: Uuid,
    /// Unix milliseconds when it was made.
    pub(crate) created_ms: u64,
}

impl RaidInvite {
    /// The invite's entry of the groups state (Observed, module doc).
    pub(crate) fn data(&self, id: [u8; 16]) -> GroupInviteData {
        let group = self
            .spec
            .groups
            .get(usize::from(self.group))
            .map_or([0; 16], |group| *group.as_bytes());
        GroupInviteData {
            raid: self.spec.raid,
            kind: GROUP_INVITE_KIND,
            activity: *self.activity.as_bytes(),
            source: GROUP_INVITE_SOURCE,
            ..GroupInviteData::plain(
                id,
                group,
                CharacterRef(None),
                CharacterRef(Some(*self.invitee.as_bytes())),
                self.created_ms.saturating_mul(NANOS_PER_MS),
            )
        }
    }
}

/// The groups state's raid fields of a raid member: the raid id, the other side's raid id (0 for
/// none), the raid type and the create source (a players' raid's only,
/// [`PLAYER_RAID_CREATE_SOURCE`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RaidFields {
    pub(crate) raid: u64,
    pub(crate) opposing: u64,
    pub(crate) kind: u8,
    pub(crate) create_source: Option<u8>,
}

/// A raid and its members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Raid {
    pub(crate) spec: RaidSpec,
    /// The members and their raid groups, in join order.
    pub(crate) seats: Vec<(Uuid, u8)>,
    /// The leader of a players' raid; none for a war's or a match's raid (module doc).
    pub(crate) leader: Option<Uuid>,
    /// The assistants of a players' raid, in the order they got the permission.
    pub(crate) assistants: Vec<Uuid>,
    /// The id of the raid's voice channel: one for every raid group (module doc).
    pub(crate) voice_channel: String,
    /// The seats as the members' views last showed them: a raid group with no member there and
    /// members now gets a lead ([`SocialWorld::update_raid_group_leads`]).
    pub(crate) shown_seats: Vec<(Uuid, u8)>,
    /// The raid groups' leads whose leader bit the group entities show (module doc).
    pub(crate) leads: Vec<RaidGroupLead>,
}

/// How long the group entity of a war's, an invasion's or a players' raid group shows the
/// leader bit of a new lead: Observed 0.80 s to 0.85 s for 13 of the 24 war and invasion seats
/// of the module doc (0.169 s to 2.56 s for all), 0.565 s to 0.783 s for the four of players'
/// raids. The service's one-second raid step sends the clear 0.8 s to 1.8 s after the seat.
pub const RAID_GROUP_LEAD_SHOWN_MS: u64 = 800;

/// The group leader of a raid group: its group entity sets the member's leader bit (module doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RaidGroupLead {
    pub(crate) member: Uuid,
    /// The raid group, from 0.
    pub(crate) group: u8,
    /// Unix milliseconds when the group entity clears the bit; `None` for a game mode team's
    /// lead, kept while the member stays in the group (module doc).
    pub(crate) clears_ms: Option<u64>,
}

impl Raid {
    /// Unix milliseconds when a lead that starts at `now_ms` ends: never for a game mode team,
    /// [`RAID_GROUP_LEAD_SHOWN_MS`] later for the other raids (module doc).
    fn lead_clears_ms(&self, now_ms: u64) -> Option<u64> {
        (self.spec.kind != RAID_TYPE_GAME_MODE)
            .then(|| now_ms.saturating_add(RAID_GROUP_LEAD_SHOWN_MS))
    }

    /// Whether the group entity shows `member` of raid group `group` as its group's leader.
    fn leads(&self, member: Uuid, group: u8) -> bool {
        self.leads
            .iter()
            .any(|lead| lead.member == member && lead.group == group)
    }

    /// Gives the raid groups their leads for the seats taken since [`Raid::shown_seats`]
    /// (module doc): a lead whose member left its group passes to the first member left in it;
    /// a group with no member there and members now goes to the raid leader when it sits there,
    /// else to its first member. The groups whose lead changed, in order.
    fn take_new_leads(&mut self, now_ms: u64) -> Vec<u8> {
        let clears_ms = self.lead_clears_ms(now_ms);
        let mut changed = Vec::new();
        let (kept, gone): (Vec<RaidGroupLead>, Vec<RaidGroupLead>) = self
            .leads
            .iter()
            .copied()
            .partition(|lead| self.seats.contains(&(lead.member, lead.group)));
        self.leads = kept;
        for lead in gone {
            changed.push(lead.group);
            if let Some(next) = self.members_of(lead.group).first() {
                self.leads.push(RaidGroupLead {
                    member: *next,
                    group: lead.group,
                    clears_ms,
                });
            }
        }
        for group in (0..self.spec.groups.len()).filter_map(|group| u8::try_from(group).ok()) {
            let members = self.members_of(group);
            let was_empty = !self.shown_seats.iter().any(|(_, seat)| *seat == group);
            let led = self.leads.iter().any(|lead| lead.group == group);
            let Some(first) = members.first() else {
                continue;
            };
            if !was_empty || led {
                continue;
            }
            let member = self
                .leader
                .filter(|leader| members.contains(leader))
                .unwrap_or(*first);
            self.leads.push(RaidGroupLead {
                member,
                group,
                clears_ms,
            });
            changed.push(group);
        }
        self.shown_seats.clone_from(&self.seats);
        changed
    }

    /// Ends the leads due at Unix ms `now_ms`; the groups whose lead ended.
    fn end_due_leads(&mut self, now_ms: u64) -> Vec<u8> {
        let (due, kept): (Vec<RaidGroupLead>, Vec<RaidGroupLead>) = self
            .leads
            .iter()
            .copied()
            .partition(|lead| lead.clears_ms.is_some_and(|clears| now_ms >= clears));
        self.leads = kept;
        due.into_iter().map(|lead| lead.group).collect()
    }

    /// The members of raid group `group`, in join order.
    pub(crate) fn members_of(&self, group: u8) -> Vec<Uuid> {
        self.seats
            .iter()
            .filter(|(_, seat)| *seat == group)
            .map(|(member, _)| *member)
            .collect()
    }
}

/// What a raid member's sim shows of its raid but its own group: the raid entity and the raid's
/// other groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RaidView {
    pub(crate) entity: [u8; 16],
    pub(crate) state: RaidDataState,
    pub(crate) groups: Vec<(Uuid, GroupDataState)>,
}

impl SocialWorld {
    /// The raid of `character` and its raid group.
    pub(crate) fn raid_seat(&self, character: Uuid) -> Option<(&Raid, u8)> {
        let raid = self.people.get(&character)?.raid?;
        let raid = self.raids.get(&raid)?;
        let group = raid
            .seats
            .iter()
            .find(|(member, _)| *member == character)
            .map(|(_, group)| *group)?;
        Some((raid, group))
    }

    /// Puts `character` into group `group` of raid `spec` (module doc). A group the raid does
    /// not have, a raid whose group count is not its type's, or a character the service does not
    /// know is refused.
    pub(crate) fn join_raid(&mut self, character: Uuid, spec: RaidSpec, group: u8) {
        let fits = usize::from(group) < spec.groups.len()
            && spec.groups.len() == groups_of(spec.kind)
            && self.people.contains_key(&character);
        if !fits {
            metrics::counter!(METRIC_CALLS, RESULT_LABEL => REFUSED).increment(1);
            return;
        }
        if let Some(known) = self.people.get(&character).and_then(|person| person.raid)
            && known != spec.entity
        {
            self.leave_raid(character, known);
        }
        let raid = self.raids.entry(spec.entity).or_insert_with(|| Raid {
            spec: spec.clone(),
            seats: Vec::new(),
            leader: None,
            assistants: Vec::new(),
            voice_channel: new_group_channel(),
            shown_seats: Vec::new(),
            leads: Vec::new(),
        });
        raid.seats.retain(|(member, _)| *member != character);
        raid.seats.push((character, group));
        let members: Vec<Uuid> = raid.seats.iter().map(|(member, _)| *member).collect();
        if let Some(person) = self.people.get_mut(&character) {
            person.raid = Some(spec.entity);
        }
        for member in members {
            self.touch(member);
        }
        if spec.kind == RAID_TYPE_PLAYER {
            self.store_player_raid(spec.entity);
        }
    }

    /// Takes `character` out of the raid of raid entity `raid` as a leave
    /// ([`Self::remove_from_raid`] with [`REMOVED_LEFT`]).
    pub(crate) fn leave_raid(&mut self, character: Uuid, raid: [u8; 16]) {
        self.remove_from_raid(character, raid, REMOVED_LEFT);
    }

    /// Takes `character` out of the raid of raid entity `raid` for `reason` (a 5283 reason); the raid ends with its last
    /// member, a players' raid with its last but one ([`crate::player_raids`]: the last member
    /// leaves too). A players' raid leader that leaves hands the raid to its first assistant, or
    /// else to its first member. The other members of its raid group get a 5283 with `reason`;
    /// a kicked member gets it too (Observed: `raidgroup.luac` shows `@ui_kickedfromraidpmessage`
    /// for a 5283 of the player itself with reason [`REMOVED_KICKED`]; Inferred: a member that
    /// leaves knows it, as a group leave tells the leaver nothing about itself). A character not
    /// in that raid is left as it is.
    pub(crate) fn remove_from_raid(&mut self, character: Uuid, raid: [u8; 16], reason: u32) {
        if self.people.get(&character).and_then(|person| person.raid) != Some(raid) {
            return;
        }
        if let Some(person) = self.people.get_mut(&character) {
            person.raid = None;
        }
        self.touch(character);
        let Some(known) = self.raids.get_mut(&raid) else {
            return;
        };
        let group = known
            .seats
            .iter()
            .find(|(member, _)| *member == character)
            .map(|(_, group)| *group);
        known.seats.retain(|(member, _)| *member != character);
        known.assistants.retain(|assistant| *assistant != character);
        if known.leader == Some(character) {
            let next = known
                .assistants
                .first()
                .copied()
                .or_else(|| known.seats.first().map(|(member, _)| *member));
            known
                .assistants
                .retain(|assistant| Some(*assistant) != next);
            known.leader = next;
        }
        let players_raid = known.spec.kind == RAID_TYPE_PLAYER;
        let last_player = (players_raid && known.seats.len() == 1)
            .then(|| known.seats.first().map(|(member, _)| *member))
            .flatten();
        let others: Vec<Uuid> = known.seats.iter().map(|(member, _)| *member).collect();
        let mut told = group
            .map(|group| known.members_of(group))
            .unwrap_or_default();
        if reason == REMOVED_KICKED {
            told.push(character);
        }
        if others.is_empty() {
            self.raids.remove(&raid);
        }
        for other in others {
            self.touch(other);
        }
        let push = GroupPush::MemberRemoved {
            character: CharacterRef(Some(*character.as_bytes())),
            reason,
        };
        self.push(&told, SocialPush::Group(push));
        if players_raid {
            self.store_player_raid(raid);
        }
        if let Some(last) = last_player {
            self.leave_raid(last, raid);
        }
    }

    /// Invites `character` into group `group` of raid `spec` for `activity` (module doc). A
    /// group the raid does not have, a raid whose group count is not its type's, a character that
    /// is offline or already in that raid is refused.
    pub(crate) fn invite_to_raid(
        &mut self,
        sim: &BusId,
        character: Uuid,
        spec: RaidSpec,
        group: u8,
        activity: Uuid,
        now_ms: u64,
    ) {
        let fits = usize::from(group) < spec.groups.len()
            && spec.groups.len() == groups_of(spec.kind)
            && self.is_online(character);
        let member =
            self.people.get(&character).and_then(|person| person.raid) == Some(spec.entity);
        if !fits || member {
            metrics::counter!(METRIC_CALLS, RESULT_LABEL => REFUSED).increment(1);
            return;
        }
        self.raid_invites
            .retain(|_, open| open.invitee != character || open.activity != activity);
        let mut id = [0; 16];
        id.copy_from_slice(Uuid::new_v4().as_bytes());
        self.raid_invites.insert(
            id,
            RaidInvite {
                sim: sim.clone(),
                invitee: character,
                spec,
                group,
                activity,
                created_ms: now_ms,
            },
        );
        self.touch(character);
    }

    /// The answer `answer` of `invitee` to raid invite `id` (3495): the invite ends with a 1677
    /// and the answer goes to the sim that asked for it (module doc). `false` when `id` is no
    /// raid invite of `invitee`.
    pub(crate) fn reply_to_raid(&mut self, invitee: Uuid, id: [u8; 16], answer: u32) -> bool {
        if self
            .raid_invites
            .get(&id)
            .is_none_or(|open| open.invitee != invitee)
        {
            return false;
        }
        let Some(open) = self.raid_invites.remove(&id) else {
            return false;
        };
        self.end_raid_invite(id, invitee);
        let answer = RaidInviteAnswer {
            character: invitee,
            activity: open.activity,
            accepted: answer == REPLY_ACCEPT,
        };
        self.out.push(Effect::RaidInviteAnswered {
            sim: open.sim,
            answer,
        });
        true
    }

    /// Ends the open raid invites of `character`: those sim `from` asked for (the player left
    /// it), or every one when `from` is `None` (it went offline).
    pub(crate) fn drop_raid_invites(&mut self, character: Uuid, from: Option<&BusId>) {
        let ended: Vec<[u8; 16]> = self
            .raid_invites
            .iter()
            .filter(|(_, open)| {
                open.invitee == character && from.is_none_or(|from| open.sim == *from)
            })
            .map(|(id, _)| *id)
            .collect();
        for id in ended {
            self.raid_invites.remove(&id);
            self.end_raid_invite(id, character);
        }
    }

    /// Tells `invitee` that raid invite `id` ended: the retail 1677 without another player.
    fn end_raid_invite(&mut self, id: [u8; 16], invitee: Uuid) {
        self.touch(invitee);
        let push = GroupPush::InviteRemoved {
            invite: id,
            other: CharacterRef(None),
            kind: INVITE_ENDED_ANSWERED,
            second: CharacterRef(None),
            outbound: false,
            flag: INVITE_REMOVED_FLAG,
        };
        self.push(&[invitee], SocialPush::Group(push));
    }

    /// The groups state's raid fields of `character`; `None` outside a raid.
    pub(crate) fn raid_fields(&self, character: Uuid) -> Option<RaidFields> {
        self.raid_seat(character).map(|(raid, _)| RaidFields {
            raid: raid.spec.raid,
            opposing: raid.spec.opposing,
            kind: raid.spec.kind,
            create_source: (raid.spec.kind == RAID_TYPE_PLAYER)
                .then_some(PLAYER_RAID_CREATE_SOURCE),
        })
    }

    /// Gives every raid its new leads at Unix ms `now_ms` ([`Raid::take_new_leads`]) and
    /// touches the members of each group whose lead changed; the views refresh calls it first.
    pub(crate) fn update_raid_group_leads(&mut self, now_ms: u64) {
        let mut changed = Vec::new();
        for raid in self.raids.values_mut() {
            if raid.seats == raid.shown_seats {
                continue;
            }
            for group in raid.take_new_leads(now_ms) {
                changed.extend(raid.members_of(group));
            }
        }
        for member in changed {
            self.touch(member);
        }
    }

    /// Ends the raid group leads due at Unix ms `now_ms` (module doc); the effects: the group
    /// entities without the leader bit.
    pub fn end_due_raid_group_leads(&mut self, now_ms: u64) -> Vec<Effect> {
        let mut changed = Vec::new();
        for raid in self.raids.values_mut() {
            for group in raid.end_due_leads(now_ms) {
                changed.extend(raid.members_of(group));
            }
        }
        for member in changed {
            self.touch(member);
        }
        self.refresh_views(now_ms);
        std::mem::take(&mut self.out)
    }

    /// The group entity of `group` of `raid`: its members in join order, the group's lead
    /// (module doc), a players' raid's leader and assistants marked.
    fn raid_group_entity(&self, raid: &Raid, group: u8) -> Option<(Uuid, GroupDataState)> {
        let id = *raid.spec.groups.get(usize::from(group))?;
        let members: Vec<GroupMemberView> = raid
            .members_of(group)
            .into_iter()
            .filter_map(|member| {
                self.member_view(member, raid.leads(member, group))
                    .map(|view| GroupMemberView {
                        raid_leader: raid.leader == Some(member),
                        raid_assistant: raid.assistants.contains(&member),
                        ..view
                    })
            })
            .collect();
        Some((
            id,
            GroupDataState::whole(*id.as_bytes(), &members).in_raid_group(group),
        ))
    }

    /// The group entity of the raid group of `character`; `None` outside a raid.
    pub(crate) fn own_raid_group_entity(&self, character: Uuid) -> Option<(Uuid, GroupDataState)> {
        let (raid, group) = self.raid_seat(character)?;
        self.raid_group_entity(raid, group)
    }

    /// The raid entity of `character` with the raid's other groups; `None` outside a raid.
    pub(crate) fn raid_view(&self, character: Uuid) -> Option<RaidView> {
        let (raid, own) = self.raid_seat(character)?;
        let groups = (0..raid.spec.groups.len())
            .filter_map(|index| u8::try_from(index).ok())
            .filter(|index| *index != own)
            .filter_map(|index| self.raid_group_entity(raid, index))
            .collect();
        let members = u8::try_from(raid.seats.len()).unwrap_or(u8::MAX);
        Some(RaidView {
            entity: raid.spec.entity,
            state: RaidDataState::whole(
                raid.spec.raid,
                raid.spec.kind,
                members,
                raid.spec
                    .groups
                    .iter()
                    .map(|group| *group.as_bytes())
                    .collect(),
            ),
            groups,
        })
    }
}

#[cfg(test)]
mod tests {
    use nw_bus::{LeaveReason, SocialInput, SocialStateKind};
    use nw_messages::payload::MemberState;
    use nw_messages::social::group_data::{FLAG_CONNECTED, FLAG_LEADER};
    use nw_messages::social::groups::{
        GROUP_INVITE_TYPE_INDEX, INVITE_REMOVED_TYPE_INDEX, MEMBER_REMOVED_TYPE_INDEX,
        REPLY_TO_INVITE_TYPE_INDEX,
    };
    use nw_messages::social::raid_data::{
        EVENT_RAID_GROUPS, RAID_DATA_TYPE_INDEX, RAID_TYPE_GAME_MODE, RAID_TYPE_WAR,
    };
    use nw_messages::social::states::{GROUPS_STATE_TYPE_INDEX, GroupsState};

    use super::*;
    use crate::effects::Effect;
    use crate::groups::tests::invite_args;
    use crate::world::tests::{NOW_MS, call, character, deliveries, join, pushes, sim, world};

    const RAID: u64 = 9428;
    /// The raid entity's id of [`spec`].
    const RAID_ENTITY: [u8; 16] = [0x24; 16];
    const OPPOSING: u64 = 9429;
    const ON: &str = "sim-a";
    /// The war of the raid invites.
    const WAR: u128 = 0x096d;
    /// A 3495 answer that is no accept.
    const REPLY_DECLINE: u32 = 1;
    /// The bytes of the retail war raid 1677 after the invite id (`arvaysa_ES_attacker_war_45vs45`
    /// 26.777 s): no other player, kind 0, no second player, inbound, flag 0.
    const RETAIL_RAID_INVITE_REMOVED_TAIL: [u8; 10] = [0; 10];

    fn spec(kind: u8) -> RaidSpec {
        RaidSpec {
            raid: RAID,
            kind,
            opposing: OPPOSING,
            entity: RAID_ENTITY,
            groups: (1..=EVENT_RAID_GROUPS as u128)
                .map(Uuid::from_u128)
                .collect(),
        }
    }

    fn input(world: &mut SocialWorld, input: SocialInput) -> Vec<Effect> {
        world.input(&sim(ON), input, NOW_MS)
    }

    fn raid_join(world: &mut SocialWorld, member: u128, group: u8, kind: u8) -> Vec<Effect> {
        input(
            world,
            SocialInput::RaidJoin {
                character: character(member),
                raid: spec(kind),
                group,
            },
        )
    }

    fn raid_leave(world: &mut SocialWorld, member: u128) -> Vec<Effect> {
        input(
            world,
            SocialInput::RaidLeave {
                character: character(member),
                raid: RAID_ENTITY,
            },
        )
    }

    fn last_groups_state(effects: &[Effect], to: Uuid) -> Option<GroupsState> {
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

    /// The last raid entity delivery to `to`: `Some(None)` when it removes the entity.
    fn raid_state(effects: &[Effect], to: Uuid) -> Option<Option<(RaidDataState, usize)>> {
        effects.iter().rev().find_map(|effect| match effect {
            Effect::RaidEntity { delivery, .. } if delivery.character == to => {
                Some(delivery.view.as_ref().map(|view| {
                    match MemberState::decode(RAID_DATA_TYPE_INDEX, &view.body) {
                        Ok(MemberState::RaidData(state)) => (state, view.groups.len()),
                        _ => (RaidDataState::default(), 0),
                    }
                }))
            }
            _ => None,
        })
    }

    fn raid_invite(world: &mut SocialWorld, member: u128, group: u8) -> Vec<Effect> {
        input(
            world,
            SocialInput::RaidInvite {
                character: character(member),
                raid: spec(RAID_TYPE_WAR),
                group,
                activity: Uuid::from_u128(WAR),
            },
        )
    }

    fn reply(world: &mut SocialWorld, member: u128, invite: [u8; 16], answer: u32) -> Vec<Effect> {
        let mut body = invite.to_vec();
        body.extend(answer.to_be_bytes());
        call(world, member, ON, REPLY_TO_INVITE_TYPE_INDEX, &body, NOW_MS)
    }

    fn answers(effects: &[Effect]) -> Vec<(BusId, RaidInviteAnswer)> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::RaidInviteAnswered { sim, answer } => Some((sim.clone(), *answer)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_war_raid_invite_shows_without_inviter_and_its_accept_goes_to_the_sim() {
        let mut world = world();
        join(&mut world, 1, "Ann", ON);
        let effects = raid_invite(&mut world, 1, 1);
        let state = last_groups_state(&effects, character(1)).expect("groups state");
        let inbound = state.inbound_invites.expect("inbound list").elements;
        assert_eq!(inbound.len(), 1);
        let invite = &inbound[0];
        assert_eq!(
            invite.group,
            *Uuid::from_u128(2).as_bytes(),
            "the raid group"
        );
        assert_eq!(invite.raid, RAID);
        assert_eq!(invite.inviter, CharacterRef(None));
        assert_eq!(invite.invitee, CharacterRef(Some(*character(1).as_bytes())));
        assert_eq!(invite.activity, *Uuid::from_u128(WAR).as_bytes());
        assert_eq!(invite.source, GROUP_INVITE_SOURCE);
        assert_eq!(invite.created_ns, NOW_MS * NANOS_PER_MS);
        let id = invite.invite;
        let effects = reply(&mut world, 1, id, REPLY_ACCEPT);
        let removed = deliveries(&effects, INVITE_REMOVED_TYPE_INDEX);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].recipients, vec![character(1)]);
        let mut retail = id.to_vec();
        retail.extend(RETAIL_RAID_INVITE_REMOVED_TAIL);
        assert_eq!(removed[0].args, retail, "the retail 1677");
        assert_eq!(
            answers(&effects),
            vec![(
                sim(ON),
                RaidInviteAnswer {
                    character: character(1),
                    activity: Uuid::from_u128(WAR),
                    accepted: true,
                }
            )]
        );
        let state = last_groups_state(&effects, character(1)).expect("groups state");
        assert_eq!(
            state.inbound_invites.map(|list| list.elements.len()),
            Some(0)
        );
        assert!(world.raid_invites.is_empty());
        assert_eq!(
            world.people[&character(1)].raid,
            None,
            "the sim's move joins it"
        );
    }

    #[test]
    fn a_declined_raid_invite_ends_without_a_move_and_a_new_one_replaces_the_open_one() {
        let mut world = world();
        join(&mut world, 1, "Ann", ON);
        raid_invite(&mut world, 1, 0);
        raid_invite(&mut world, 1, 3);
        assert_eq!(world.raid_invites.len(), 1, "one invite per activity");
        let (id, open) = world.raid_invites.iter().next().expect("invite");
        assert_eq!(open.group, 3);
        let id = *id;
        let effects = reply(&mut world, 1, id, REPLY_DECLINE);
        assert_eq!(deliveries(&effects, INVITE_REMOVED_TYPE_INDEX).len(), 1);
        let answered = answers(&effects);
        assert_eq!(answered.len(), 1);
        assert!(!answered[0].1.accepted, "the sim forgets the move");
        assert!(world.raid_invites.is_empty());
        // A second answer to the ended invite does nothing.
        let effects = reply(&mut world, 1, id, REPLY_ACCEPT);
        assert!(answers(&effects).is_empty());
    }

    #[test]
    fn a_raid_invite_ends_offline_and_is_refused_for_a_member_or_a_stranger() {
        let mut world = world();
        join(&mut world, 1, "Ann", ON);
        join(&mut world, 2, "Bob", ON);
        raid_invite(&mut world, 1, 0);
        input(
            &mut world,
            SocialInput::Left {
                character: character(1),
                reason: LeaveReason::Logout,
            },
        );
        assert!(world.raid_invites.is_empty(), "offline ends it");
        raid_invite(&mut world, 2, 0);
        input(
            &mut world,
            SocialInput::Left {
                character: character(2),
                reason: LeaveReason::Handoff,
            },
        );
        assert!(
            world.raid_invites.is_empty(),
            "leaving the asking sim ends it"
        );
        raid_join(&mut world, 2, 0, RAID_TYPE_WAR);
        raid_invite(&mut world, 2, 0);
        assert!(world.raid_invites.is_empty(), "already in the raid");
        raid_invite(&mut world, 9, 0);
        assert!(world.raid_invites.is_empty(), "an unknown character");
        let effects = reply(&mut world, 2, [7; 16], REPLY_ACCEPT);
        assert!(answers(&effects).is_empty(), "no such invite");
    }

    #[test]
    fn a_war_raid_member_gets_the_raid_fields_its_group_and_the_raid_entity() {
        let mut world = world();
        join(&mut world, 1, "Ann", ON);
        join(&mut world, 2, "Bob", ON);
        let effects = raid_join(&mut world, 1, 1, RAID_TYPE_WAR);
        let state = last_groups_state(&effects, character(1)).expect("groups state");
        assert_eq!(state.group_id, Some(*Uuid::from_u128(2).as_bytes()));
        assert_eq!(state.raid_id, Some(RAID));
        assert_eq!(state.opposing_raid_id, Some(OPPOSING));
        assert_eq!(state.raid_type, Some(RAID_TYPE_WAR));
        let (raid, groups) = raid_state(&effects, character(1))
            .flatten()
            .expect("raid entity");
        assert_eq!(raid.members, Some(1));
        assert_eq!(raid.group_ids.map(|ids| ids.len()), Some(EVENT_RAID_GROUPS));
        assert_eq!(groups, EVENT_RAID_GROUPS - 1, "the other nine groups");
        let own_group = effects.iter().rev().find_map(|effect| match effect {
            Effect::GroupEntity { delivery, .. } if delivery.character == character(1) => {
                delivery.view.as_ref().map(|view| view.group)
            }
            _ => None,
        });
        assert_eq!(own_group, Some(Uuid::from_u128(2)));
        // The second member raises the first one's member count.
        let effects = raid_join(&mut world, 2, 1, RAID_TYPE_WAR);
        let (raid, _) = raid_state(&effects, character(1))
            .flatten()
            .expect("raid entity again");
        assert_eq!(raid.members, Some(2));
    }

    #[test]
    fn a_raid_leave_clears_the_fields_tells_the_group_and_the_last_one_ends_the_raid() {
        let mut world = world();
        join(&mut world, 1, "Ann", ON);
        join(&mut world, 2, "Bob", ON);
        raid_join(&mut world, 1, 0, RAID_TYPE_GAME_MODE);
        raid_join(&mut world, 2, 0, RAID_TYPE_GAME_MODE);
        let effects = raid_leave(&mut world, 1);
        let state = last_groups_state(&effects, character(1)).expect("groups state");
        assert_eq!(state.group_id, Some([0; 16]));
        assert_eq!(state.raid_id, Some(0));
        assert_eq!(state.raid_type, Some(0));
        assert_eq!(raid_state(&effects, character(1)), Some(None));
        let removed = pushes(&effects, MEMBER_REMOVED_TYPE_INDEX);
        assert_eq!(removed.len(), 1);
        assert_eq!(
            removed[0].0,
            vec![character(2)],
            "the group mate gets the 5283"
        );
        assert!(world.raids.contains_key(&RAID_ENTITY));
        raid_leave(&mut world, 2);
        assert!(world.raids.is_empty());
    }

    #[test]
    fn a_raid_member_keeps_its_group_for_after_is_refused_invites_and_leaves_at_logout() {
        let mut world = world();
        join(&mut world, 1, "Ann", ON);
        join(&mut world, 2, "Bob", ON);
        join(&mut world, 3, "Cid", ON);
        call(
            &mut world,
            1,
            ON,
            GROUP_INVITE_TYPE_INDEX,
            &invite_args(2),
            NOW_MS,
        );
        let invite = *world.group_invites.keys().next().expect("invite");
        let mut reply = invite.to_vec();
        reply.extend(REPLY_ACCEPT.to_be_bytes());
        call(
            &mut world,
            2,
            ON,
            REPLY_TO_INVITE_TYPE_INDEX,
            &reply,
            NOW_MS,
        );
        assert!(world.people[&character(1)].group.is_some());
        let group = world.people[&character(1)].group.expect("a group");
        let effects = raid_join(&mut world, 1, 0, RAID_TYPE_WAR);
        let state = last_groups_state(&effects, character(1)).expect("groups state");
        assert_eq!(
            state.group_id,
            Some(*Uuid::from_u128(1).as_bytes()),
            "the raid group shows"
        );
        assert_eq!(world.people[&character(1)].group, Some(group), "kept");
        let effects = raid_leave(&mut world, 1);
        let state = last_groups_state(&effects, character(1)).expect("groups state");
        assert_eq!(state.group_id, Some(*group.as_bytes()), "its group again");
        raid_join(&mut world, 1, 0, RAID_TYPE_WAR);
        call(
            &mut world,
            3,
            ON,
            GROUP_INVITE_TYPE_INDEX,
            &invite_args(1),
            NOW_MS,
        );
        assert!(
            world.group_invites.is_empty(),
            "a raid member is not invited"
        );
        input(
            &mut world,
            SocialInput::Left {
                character: character(1),
                reason: LeaveReason::Logout,
            },
        );
        assert!(world.raids.is_empty(), "the offline member left the raid");
    }

    #[test]
    fn a_group_the_raid_lacks_or_a_wrong_group_count_is_refused() {
        let mut world = world();
        join(&mut world, 1, "Ann", ON);
        raid_join(&mut world, 1, 10, RAID_TYPE_WAR);
        let short = RaidSpec {
            groups: vec![Uuid::from_u128(1)],
            ..spec(RAID_TYPE_WAR)
        };
        input(
            &mut world,
            SocialInput::RaidJoin {
                character: character(1),
                raid: short,
                group: 0,
            },
        );
        assert!(world.raids.is_empty());
        assert_eq!(world.people[&character(1)].raid, None);
    }

    /// The member flags of the raid group of `member` as its group entity shows them.
    fn shown_flags(world: &SocialWorld, member: u128) -> Vec<Option<[u8; 2]>> {
        let (_, state) = world
            .own_raid_group_entity(character(member))
            .expect("a raid group entity");
        state.members.iter().map(|block| block.flags).collect()
    }

    const LEADING: [u8; 2] = [FLAG_CONNECTED | FLAG_LEADER, 0];
    const SEATED: [u8; 2] = [FLAG_CONNECTED, 0];
    const EMPTY: [u8; 2] = [0, 0];

    #[test]
    fn a_war_raid_group_shows_its_first_member_leading_until_the_lead_clears() {
        let mut world = world();
        join(&mut world, 1, "Ann", ON);
        join(&mut world, 2, "Bob", ON);
        raid_join(&mut world, 1, 1, RAID_TYPE_WAR);
        raid_join(&mut world, 2, 1, RAID_TYPE_WAR);
        assert_eq!(
            shown_flags(&world, 2)[..3],
            [Some(LEADING), Some(SEATED), Some(EMPTY)],
            "the seat in the empty group leads, the next one does not"
        );
        let early = world.end_due_raid_group_leads(NOW_MS + RAID_GROUP_LEAD_SHOWN_MS - 1);
        assert!(early.is_empty(), "nothing is due yet");
        let effects = world.end_due_raid_group_leads(NOW_MS + RAID_GROUP_LEAD_SHOWN_MS);
        assert_eq!(shown_flags(&world, 1)[..2], [Some(SEATED), Some(SEATED)]);
        let told: Vec<Uuid> = effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::GroupEntity { delivery, .. } => Some(delivery.character),
                _ => None,
            })
            .collect();
        assert!(
            told.contains(&character(1)) && told.contains(&character(2)),
            "both members get the group entity without the bit"
        );
        raid_leave(&mut world, 1);
        assert_eq!(
            shown_flags(&world, 2)[0],
            Some(SEATED),
            "a cleared lead has no successor"
        );
    }

    #[test]
    fn a_game_mode_team_keeps_its_lead_and_hands_it_to_the_next_member() {
        let mut world = world();
        for (number, name) in [(1, "Ann"), (2, "Bob"), (3, "Cid")] {
            join(&mut world, number, name, ON);
            raid_join(&mut world, number, 0, RAID_TYPE_GAME_MODE);
        }
        world.end_due_raid_group_leads(u64::MAX);
        assert_eq!(
            shown_flags(&world, 3)[..3],
            [Some(LEADING), Some(SEATED), Some(SEATED)],
            "a team's lead never clears"
        );
        raid_leave(&mut world, 1);
        assert_eq!(
            shown_flags(&world, 3)[..2],
            [Some(LEADING), Some(SEATED)],
            "the first member left in the group leads at once"
        );
        raid_join(&mut world, 1, 1, RAID_TYPE_GAME_MODE);
        assert_eq!(
            shown_flags(&world, 1)[0],
            Some(LEADING),
            "a new group's seat"
        );
        raid_join(&mut world, 2, 1, RAID_TYPE_GAME_MODE);
        assert_eq!(
            shown_flags(&world, 3)[0],
            Some(LEADING),
            "the move out of group 0 hands its lead on"
        );
        assert_eq!(
            shown_flags(&world, 1)[..2],
            [Some(LEADING), Some(SEATED)],
            "a member that comes to a led group does not lead"
        );
    }
}
