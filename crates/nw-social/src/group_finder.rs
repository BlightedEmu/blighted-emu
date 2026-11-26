// SPDX-License-Identifier: AGPL-3.0-or-later
//! Group Finder lobbies, applications, subscriptions and conversion to a real group
//! (`nw_messages::social::group_finder`).
//!
//! Handles all 5 observed client calls (`1960`, `1147`, `3245`, `5355`, `4739`), pushes all 4
//! observed server answers (`3461`, `2010`, `3485`, `3475`), replicates `groupFinderGroupId` and
//! `isGroupFinderGroupCreator` on `GroupsState` (`1994`), and converts confirmed lobbies into real
//! `SocialGroup`s (`createSource = 1`, Observed in `07-Tempests_Heart` and `09-The_Ennead`).

use std::collections::BTreeSet;

use nw_db::SocialGroup;
use nw_messages::social::group_data::GROUP_SIZE;
use nw_messages::social::group_finder::{
    GroupFinderCall, GroupFinderGroupListEntry, GroupFinderGroupMetadata, GroupFinderMemberBlob,
    GroupFinderPush, NO_PLATFORM_ID_KIND,
};
use nw_messages::social::{ICON_LEN, SocialPush};
use uuid::Uuid;

use crate::effects::StoreOp;
use crate::world::{METRIC_CALLS, REFUSED, RESULT_LABEL, SocialWorld, TAKEN};

/// Maximum listings returned in one `3461` page.
const PAGE_SIZE: usize = 20;
/// `game_mode_crc` wildcard that matches listings for any activity.
const ANY_GAME_MODE: u32 = 0;

/// Status of a Group Finder application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplicationStatus {
    /// Waiting for the lobby leader to accept or reject.
    Pending,
    /// Accepted by the lobby leader; waiting for the applicant to confirm or decline.
    Accepted,
}

/// One member inside a Group Finder lobby.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupFinderMember {
    /// The member's character id.
    pub character: Uuid,
    /// Picked `GroupFinderRole`.
    pub role: u8,
    /// Gear score advertised in the lobby.
    pub gear_score: u32,
}

/// One application to a Group Finder lobby.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupFinderApplication {
    /// Application id (UUID).
    pub id: Uuid,
    /// Applicant character id.
    pub applicant: Uuid,
    /// Picked `GroupFinderRole`.
    pub role: u8,
    /// Gear score advertised in the application.
    pub gear_score: u32,
    /// Current status.
    pub status: ApplicationStatus,
}

/// One open Group Finder listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupFinderLobby {
    /// Listing id (16-byte UUID).
    pub id: Uuid,
    /// Character that created/leads the listing.
    pub creator: Uuid,
    /// Metadata (`game_mode_crc`, level/GS requirements, title, timestamp).
    pub metadata: GroupFinderGroupMetadata,
    /// Current members (creator first).
    pub members: Vec<GroupFinderMember>,
    /// Open applications.
    pub applications: Vec<GroupFinderApplication>,
    /// Whether the leader initiated conversion to a real group.
    pub pending_conversion: bool,
    /// Members that confirmed conversion (`4739`).
    pub conversion_confirmations: BTreeSet<Uuid>,
}

impl SocialWorld {
    /// Handles an observed Group Finder client call (`1960`, `1147`, `3245`, `5355`, `4739`).
    pub(crate) fn group_finder_call(&mut self, character: Uuid, call: GroupFinderCall) {
        match call {
            GroupFinderCall::GetGroupList {
                game_mode_crc,
                page,
                ..
            } => {
                metrics::counter!(METRIC_CALLS, RESULT_LABEL => TAKEN).increment(1);
                self.group_finder_get_group_list(character, game_mode_crc, page);
            }
            GroupFinderCall::GetGroup { group_id } => {
                metrics::counter!(METRIC_CALLS, RESULT_LABEL => TAKEN).increment(1);
                self.group_finder_get_group(character, Uuid::from_bytes(group_id));
            }
            GroupFinderCall::DeclineApplicationInvite {
                group_id,
                application_id,
                ..
            } => {
                if let Ok(app_id) = Uuid::parse_str(&application_id) {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => TAKEN).increment(1);
                    self.decline_group_finder_application_invite(
                        character,
                        Uuid::from_bytes(group_id),
                        app_id,
                    );
                } else {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => REFUSED).increment(1);
                }
            }
            GroupFinderCall::ModifyApplicationSubscription { subscribed } => {
                metrics::counter!(METRIC_CALLS, RESULT_LABEL => TAKEN).increment(1);
                if subscribed {
                    self.group_finder_subscribers.insert(character);
                } else {
                    self.group_finder_subscribers.remove(&character);
                }
            }
            GroupFinderCall::ConfirmConvertToGroup => {
                if self.confirm_group_finder_conversion(character) {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => TAKEN).increment(1);
                } else {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => REFUSED).increment(1);
                }
            }
        }
    }

    /// Creates a new Group Finder listing owned by `creator`. Returns its lobby id when created.
    pub fn create_group_finder_lobby(
        &mut self,
        creator: Uuid,
        metadata: GroupFinderGroupMetadata,
        role: u8,
        gear_score: u32,
    ) -> Option<Uuid> {
        if !self.is_online(creator) || self.group_finder_lobby_of(creator).is_some() {
            return None;
        }
        let id = Uuid::new_v4();
        self.group_finder_lobbies.insert(
            id,
            GroupFinderLobby {
                id,
                creator,
                metadata,
                members: vec![GroupFinderMember {
                    character: creator,
                    role,
                    gear_score,
                }],
                applications: Vec::new(),
                pending_conversion: false,
                conversion_confirmations: BTreeSet::new(),
            },
        );
        self.touch(creator);
        Some(id)
    }

    /// Removes the Group Finder listing owned by `creator`.
    pub fn remove_group_finder_lobby(&mut self, creator: Uuid) -> bool {
        let Some(lobby_id) = self
            .group_finder_lobbies
            .values()
            .find(|lobby| lobby.creator == creator)
            .map(|lobby| lobby.id)
        else {
            return false;
        };
        if let Some(lobby) = self.group_finder_lobbies.remove(&lobby_id) {
            for member in lobby.members {
                self.touch(member.character);
            }
            return true;
        }
        false
    }

    /// Submits an application from `applicant` to `lobby_id`. Returns the application id.
    pub fn submit_group_finder_application(
        &mut self,
        applicant: Uuid,
        lobby_id: Uuid,
        role: u8,
        gear_score: u32,
    ) -> Option<Uuid> {
        if !self.is_online(applicant) || self.group_finder_lobby_of(applicant).is_some() {
            return None;
        }
        let lobby = self.group_finder_lobbies.get_mut(&lobby_id)?;
        if lobby.members.len() >= GROUP_SIZE
            || lobby.applications.iter().any(|a| a.applicant == applicant)
        {
            return None;
        }
        let app_id = Uuid::new_v4();
        lobby.applications.push(GroupFinderApplication {
            id: app_id,
            applicant,
            role,
            gear_score,
            status: ApplicationStatus::Pending,
        });
        Some(app_id)
    }

    /// Leader accepts `application_id` in `lobby_id` and pushes `3485` to the applicant.
    pub fn accept_group_finder_application(
        &mut self,
        leader: Uuid,
        lobby_id: Uuid,
        application_id: Uuid,
    ) -> bool {
        let applicant = {
            let Some(lobby) = self.group_finder_lobbies.get_mut(&lobby_id) else {
                return false;
            };
            if lobby.creator != leader || lobby.members.len() >= GROUP_SIZE {
                return false;
            }
            let Some(app) = lobby
                .applications
                .iter_mut()
                .find(|a| a.id == application_id && a.status == ApplicationStatus::Pending)
            else {
                return false;
            };
            app.status = ApplicationStatus::Accepted;
            app.applicant
        };
        self.push(
            &[applicant],
            SocialPush::GroupFinder(GroupFinderPush::ApplicationAccepted {
                group_id: *lobby_id.as_bytes(),
                application_id: application_id.hyphenated().to_string(),
            }),
        );
        true
    }

    /// Applicant confirms an accepted invite and joins `lobby_id`.
    pub fn confirm_group_finder_application_invite(
        &mut self,
        applicant: Uuid,
        lobby_id: Uuid,
        application_id: Uuid,
    ) -> bool {
        if !self.is_online(applicant) || self.group_finder_lobby_of(applicant).is_some() {
            return false;
        }
        let Some(lobby) = self.group_finder_lobbies.get_mut(&lobby_id) else {
            return false;
        };
        if lobby.members.len() >= GROUP_SIZE {
            return false;
        }
        let Some(pos) = lobby.applications.iter().position(|a| {
            a.id == application_id
                && a.applicant == applicant
                && a.status == ApplicationStatus::Accepted
        }) else {
            return false;
        };
        let app = lobby.applications.remove(pos);
        lobby.members.push(GroupFinderMember {
            character: applicant,
            role: app.role,
            gear_score: app.gear_score,
        });
        self.touch(applicant);
        true
    }

    /// Applicant declines an accepted invite (`3245`) and receives `3475`.
    pub fn decline_group_finder_application_invite(
        &mut self,
        applicant: Uuid,
        lobby_id: Uuid,
        application_id: Uuid,
    ) {
        if let Some(lobby) = self.group_finder_lobbies.get_mut(&lobby_id) {
            lobby
                .applications
                .retain(|a| !(a.id == application_id && a.applicant == applicant));
        }
        self.push(
            &[applicant],
            SocialPush::GroupFinder(GroupFinderPush::DeclineApplicationInviteSuccess),
        );
    }

    /// Leader initiates converting `lobby_id` into a real group.
    pub fn initiate_group_finder_conversion(&mut self, leader: Uuid, lobby_id: Uuid) -> bool {
        let ready = {
            let Some(lobby) = self.group_finder_lobbies.get_mut(&lobby_id) else {
                return false;
            };
            if lobby.creator != leader {
                return false;
            }
            lobby.pending_conversion = true;
            lobby.conversion_confirmations.insert(leader);
            lobby
                .members
                .iter()
                .all(|m| lobby.conversion_confirmations.contains(&m.character))
        };
        if ready {
            self.finish_group_finder_conversion(lobby_id);
        }
        true
    }

    /// Member confirms converting its Group Finder lobby into a real group (`4739`).
    pub fn confirm_group_finder_conversion(&mut self, member: Uuid) -> bool {
        let Some(lobby_id) = self.group_finder_lobby_of(member) else {
            return false;
        };
        let ready = {
            let Some(lobby) = self.group_finder_lobbies.get_mut(&lobby_id) else {
                return false;
            };
            lobby.pending_conversion = true;
            lobby.conversion_confirmations.insert(member);
            lobby
                .members
                .iter()
                .all(|m| lobby.conversion_confirmations.contains(&m.character))
        };
        if ready {
            self.finish_group_finder_conversion(lobby_id);
        }
        true
    }

    fn finish_group_finder_conversion(&mut self, lobby_id: Uuid) {
        let Some(lobby) = self.group_finder_lobbies.remove(&lobby_id) else {
            return;
        };
        let members: Vec<Uuid> = lobby.members.iter().map(|m| m.character).collect();
        if members.is_empty() {
            return;
        }
        let leader = lobby.creator;
        let group_id = Uuid::new_v4();
        let group = SocialGroup {
            id: group_id,
            leader,
            voice_channel: String::new(),
            members: members.clone(),
        };
        for member in &members {
            if let Some(person) = self.people.get_mut(member) {
                if let Some(role) = lobby
                    .members
                    .iter()
                    .find(|m| m.character == *member)
                    .map(|m| m.role)
                {
                    person.group_role = role;
                }
                person.group = Some(group_id);
            }
            self.touch(*member);
        }
        self.groups.insert(group_id, group.clone());
        self.store(StoreOp::SaveGroup(group));
    }

    /// Answers `1960` with a page of listings (`3461`).
    fn group_finder_get_group_list(&mut self, requester: Uuid, game_mode_crc: u32, page: u16) {
        let matching: Vec<GroupFinderGroupListEntry> = self
            .group_finder_lobbies
            .values()
            .filter(|lobby| {
                game_mode_crc == ANY_GAME_MODE || lobby.metadata.game_mode_crc == game_mode_crc
            })
            .map(|lobby| GroupFinderGroupListEntry {
                group_id: *lobby.id.as_bytes(),
                metadata: lobby.metadata.clone(),
                member_count: u8::try_from(lobby.members.len()).unwrap_or(u8::MAX),
            })
            .collect();
        let start = usize::from(page).saturating_mul(PAGE_SIZE);
        let end = start.saturating_add(PAGE_SIZE).min(matching.len());
        let groups = if start < matching.len() {
            matching[start..end].to_vec()
        } else {
            Vec::new()
        };
        let is_last_page = end >= matching.len();
        self.push(
            &[requester],
            SocialPush::GroupFinder(GroupFinderPush::GetGroupListSuccess {
                groups,
                page_token: 0,
                is_last_page,
            }),
        );
    }

    /// Answers `1147` with full listing details (`2010`).
    fn group_finder_get_group(&mut self, requester: Uuid, lobby_id: Uuid) {
        let Some(lobby) = self.group_finder_lobbies.get(&lobby_id) else {
            return;
        };
        let metadata = lobby.metadata.clone();
        let members = lobby
            .members
            .iter()
            .map(|member| self.member_blob(member))
            .collect();
        self.push(
            &[requester],
            SocialPush::GroupFinder(GroupFinderPush::GetGroupSuccess {
                group_id: *lobby_id.as_bytes(),
                metadata,
                members,
            }),
        );
    }

    fn member_blob(&self, member: &GroupFinderMember) -> GroupFinderMemberBlob {
        let (level, icon) = self
            .people
            .get(&member.character)
            .map(|person| {
                let level = person
                    .online
                    .as_ref()
                    .map_or(person.settings.level, |o| o.level);
                let icon = person
                    .online
                    .as_ref()
                    .map(|o| o.icon)
                    .or(person.settings.icon)
                    .unwrap_or([0; ICON_LEN]);
                (level, icon)
            })
            .unwrap_or((1, [0; ICON_LEN]));
        GroupFinderMemberBlob {
            character_id: member.character.hyphenated().to_string(),
            platform_id_kind: NO_PLATFORM_ID_KIND,
            platform_id: String::new(),
            role: member.role,
            level,
            gear_score: member.gear_score,
            icon,
        }
    }

    /// The Group Finder lobby `character` is currently in, if any.
    pub(crate) fn group_finder_lobby_of(&self, character: Uuid) -> Option<Uuid> {
        self.group_finder_lobbies
            .values()
            .find(|lobby| lobby.members.iter().any(|m| m.character == character))
            .map(|lobby| lobby.id)
    }

    /// Whether `character` created its current Group Finder lobby.
    pub(crate) fn is_group_finder_creator(&self, character: Uuid) -> bool {
        self.group_finder_lobbies
            .values()
            .any(|lobby| lobby.creator == character)
    }

    /// Number of open joinable Group Finder lobbies for `game_mode_crc` (or all when `0`).
    pub fn group_finder_lobby_count(&self, game_mode_crc: u32) -> u32 {
        let count = self
            .group_finder_lobbies
            .values()
            .filter(|lobby| {
                lobby.members.len() < GROUP_SIZE
                    && (game_mode_crc == ANY_GAME_MODE
                        || lobby.metadata.game_mode_crc == game_mode_crc)
            })
            .count();
        u32::try_from(count).unwrap_or(u32::MAX)
    }

    /// Cleans up Group Finder state when `character` goes offline.
    pub(crate) fn group_finder_went_offline(&mut self, character: Uuid) {
        self.group_finder_subscribers.remove(&character);
        let mut empty = Vec::new();
        for lobby in self.group_finder_lobbies.values_mut() {
            lobby.applications.retain(|a| a.applicant != character);
            lobby.conversion_confirmations.remove(&character);
            let before = lobby.members.len();
            lobby.members.retain(|m| m.character != character);
            if lobby.members.len() != before {
                if lobby.members.is_empty() {
                    empty.push(lobby.id);
                } else if lobby.creator == character {
                    lobby.creator = lobby.members[0].character;
                }
            }
        }
        for id in empty {
            self.group_finder_lobbies.remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use nw_messages::social::group_finder::{
        CONFIRM_CONVERT_TO_GROUP_TYPE_INDEX, DECLINE_APPLICATION_INVITE_TYPE_INDEX,
        GET_GROUP_LIST_SUCCESS_TYPE_INDEX, GET_GROUP_LIST_TYPE_INDEX, GET_GROUP_SUCCESS_TYPE_INDEX,
        GET_GROUP_TYPE_INDEX, MODIFY_APPLICATION_SUBSCRIPTION_TYPE_INDEX,
    };

    use super::*;
    use crate::effects::Effect;
    use crate::world::tests::{HEADER, NOW_MS, call, character, join, world};

    const AMRINE_CRC: u32 = 0x6a64_c798;

    fn metadata() -> GroupFinderGroupMetadata {
        GroupFinderGroupMetadata {
            flags: [1, 0, 0],
            game_mode_crc: AMRINE_CRC,
            mutator_crc: 0,
            min_level: 25,
            min_gear_score: 100,
            tail_flags: [0, 0x1c, 0],
            title: b"Amrine Run".to_vec(),
            created_ns: 1_700_000_000_000_000_000,
        }
    }

    fn pushes_for(effects: &[Effect], target: Uuid) -> Vec<GroupFinderPush> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Deliver { delivery, .. } if delivery.recipients.contains(&target) => {
                    match SocialPush::decode_args(delivery.type_index, &delivery.args) {
                        Some(Ok(SocialPush::GroupFinder(push))) => Some(push),
                        _ => None,
                    }
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn group_finder_lifecycle_lists_gets_declines_and_converts_to_real_group() {
        let mut world = world();
        join(&mut world, 1, "Ann", "sim-1");
        join(&mut world, 2, "Bob", "sim-1");
        let ann = character(1);
        let bob = character(2);

        // Empty list returns `3461` with `00 00 01`.
        let list_body = GroupFinderCall::GetGroupList {
            filter_flag: 1,
            game_mode_crc: AMRINE_CRC,
            page: 0,
        }
        .encode(HEADER)
        .expect("encode");
        let effects = call(
            &mut world,
            2,
            "sim-1",
            GET_GROUP_LIST_TYPE_INDEX,
            &list_body[16..],
            NOW_MS,
        );
        assert_eq!(
            pushes_for(&effects, bob),
            [GroupFinderPush::GetGroupListSuccess {
                groups: Vec::new(),
                page_token: 0,
                is_last_page: true,
            }]
        );

        // Ann creates a lobby.
        let lobby_id = world
            .create_group_finder_lobby(ann, metadata(), 4, 650)
            .expect("lobby created");
        assert_eq!(world.group_finder_lobby_count(AMRINE_CRC), 1);

        // Bob lists and inspects the lobby (`1960` -> `3461`, `1147` -> `2010`).
        let effects = call(
            &mut world,
            2,
            "sim-1",
            GET_GROUP_LIST_TYPE_INDEX,
            &list_body[16..],
            NOW_MS,
        );
        let list_pushes = pushes_for(&effects, bob);
        assert_eq!(list_pushes.len(), 1);
        assert_eq!(
            list_pushes[0].type_index(),
            GET_GROUP_LIST_SUCCESS_TYPE_INDEX
        );
        let GroupFinderPush::GetGroupListSuccess { groups, .. } = &list_pushes[0] else {
            panic!("expected list");
        };
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].group_id, *lobby_id.as_bytes());

        let get_body = GroupFinderCall::GetGroup {
            group_id: *lobby_id.as_bytes(),
        }
        .encode(HEADER)
        .expect("encode");
        let effects = call(
            &mut world,
            2,
            "sim-1",
            GET_GROUP_TYPE_INDEX,
            &get_body[16..],
            NOW_MS,
        );
        let get_pushes = pushes_for(&effects, bob);
        assert_eq!(get_pushes.len(), 1);
        assert_eq!(get_pushes[0].type_index(), GET_GROUP_SUCCESS_TYPE_INDEX);

        // Bob subscribes (`5355`), applies, Ann accepts (`3485`), Bob declines (`3245` -> `3475`).
        let sub_body = GroupFinderCall::ModifyApplicationSubscription { subscribed: true }
            .encode(HEADER)
            .expect("encode");
        call(
            &mut world,
            2,
            "sim-1",
            MODIFY_APPLICATION_SUBSCRIPTION_TYPE_INDEX,
            &sub_body[16..],
            NOW_MS,
        );
        assert!(world.group_finder_subscribers.contains(&bob));

        let app1 = world
            .submit_group_finder_application(bob, lobby_id, 3, 620)
            .expect("apply");
        assert!(world.accept_group_finder_application(ann, lobby_id, app1));
        let decline_body = GroupFinderCall::DeclineApplicationInvite {
            group_id: *lobby_id.as_bytes(),
            application_id: app1.hyphenated().to_string(),
            reason: 0,
        }
        .encode(HEADER)
        .expect("encode");
        let effects = call(
            &mut world,
            2,
            "sim-1",
            DECLINE_APPLICATION_INVITE_TYPE_INDEX,
            &decline_body[16..],
            NOW_MS,
        );
        assert_eq!(
            pushes_for(&effects, bob),
            [
                GroupFinderPush::ApplicationAccepted {
                    group_id: *lobby_id.as_bytes(),
                    application_id: app1.hyphenated().to_string(),
                },
                GroupFinderPush::DeclineApplicationInviteSuccess,
            ]
        );

        // Bob re-applies, Ann accepts, Bob confirms invite, both confirm conversion (`4739`).
        let app2 = world
            .submit_group_finder_application(bob, lobby_id, 3, 620)
            .expect("re-apply");
        assert!(world.accept_group_finder_application(ann, lobby_id, app2));
        assert!(world.confirm_group_finder_application_invite(bob, lobby_id, app2));
        assert!(world.initiate_group_finder_conversion(ann, lobby_id));
        let effects = call(
            &mut world,
            2,
            "sim-1",
            CONFIRM_CONVERT_TO_GROUP_TYPE_INDEX,
            &[],
            NOW_MS,
        );
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::Store(StoreOp::SaveGroup(g)) if g.members.len() == 2)),
            "converting lobby creates and stores a real 2-player group"
        );
        assert_eq!(world.group_finder_lobby_count(AMRINE_CRC), 0);
        assert!(world.people[&ann].group.is_some());
        assert_eq!(world.people[&ann].group, world.people[&bob].group);
    }
}
