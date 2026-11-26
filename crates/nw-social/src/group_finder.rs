// SPDX-License-Identifier: AGPL-3.0-or-later
//! Group Finder lobbies, applications, subscriptions, lobby counts and conversion to a real group
//! (`nw_messages::social::group_finder`).
//!
//! All 19 client calls (`3509`, `2872`, `1683`, `1960`, `1147`, `3510`, `3515`, `1383`, `3513`,
//! `1553`, `105`, `3245`, `3518`, `3519`, `5355`, `4723`, `4739`, `4744`, `5463`) are handled
//! directly from `SocialWorld::call`, replicate `GroupFinderGroupDataState` (`1091`) and
//! `GroupsState` (`1994`) to members/subscribers, and convert confirmed lobbies into real
//! `SocialGroup`s.

use std::collections::BTreeSet;

use nw_db::SocialGroup;
use nw_messages::FullList;
use nw_messages::lookups::LOBBY_COUNT_SLOTS;
use nw_messages::social::group_data::GROUP_SIZE;
use nw_messages::social::group_finder::{
    GroupFinderCall, GroupFinderGroupDataState, GroupFinderGroupListEntry,
    GroupFinderGroupMetadata, GroupFinderListEntryTail, GroupFinderMemberBlob, GroupFinderPush,
    NO_PLATFORM_ID_KIND,
};
use nw_messages::social::{CharacterRef, ICON_LEN, SocialPush};
use uuid::Uuid;

use crate::effects::StoreOp;
use crate::world::{METRIC_CALLS, REFUSED, RESULT_LABEL, SocialWorld, TAKEN};

/// Maximum listings returned in one `3461` page.
const PAGE_SIZE: usize = 20;
/// `game_mode_crc` wildcard that matches listings for any activity.
const ANY_GAME_MODE: u32 = 0;
/// Application state byte on class `1091` Slot 3 (`applicationStates`): pending vs accepted.
const APP_STATE_PENDING: u8 = 1;
const APP_STATE_ACCEPTED: u8 = 2;

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
    /// Handles any Group Finder client call (`3509`..`5463`).
    pub(crate) fn group_finder_call(&mut self, character: Uuid, call: GroupFinderCall) {
        match call {
            GroupFinderCall::CreateGroup { metadata, role, .. } => {
                if self
                    .create_group_finder_lobby(character, metadata, role, 0)
                    .is_some()
                {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => TAKEN).increment(1);
                    self.push(
                        &[character],
                        SocialPush::GroupFinder(GroupFinderPush::CreateGroupSuccess),
                    );
                } else {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => REFUSED).increment(1);
                }
            }
            GroupFinderCall::UpdateGroup { metadata } => {
                if self.update_group_finder_lobby(character, metadata) {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => TAKEN).increment(1);
                    self.push(
                        &[character],
                        SocialPush::GroupFinder(GroupFinderPush::UpdateGroupSuccess),
                    );
                } else {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => REFUSED).increment(1);
                }
            }
            GroupFinderCall::RemoveGroup => {
                if self.remove_group_finder_lobby(character) {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => TAKEN).increment(1);
                    self.push(
                        &[character],
                        SocialPush::GroupFinder(GroupFinderPush::RemoveGroupSuccess),
                    );
                } else {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => REFUSED).increment(1);
                }
            }
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
            GroupFinderCall::SubmitApplication { group_id, role } => {
                if self
                    .submit_group_finder_application(character, Uuid::from_bytes(group_id), role, 0)
                    .is_some()
                {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => TAKEN).increment(1);
                    self.push(
                        &[character],
                        SocialPush::GroupFinder(GroupFinderPush::SubmitApplicationSuccess),
                    );
                } else {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => REFUSED).increment(1);
                }
            }
            GroupFinderCall::AcceptApplication {
                group_id,
                application_id,
            } => {
                if let Ok(app_id) = Uuid::parse_str(&application_id)
                    && self.accept_group_finder_application(
                        character,
                        Uuid::from_bytes(group_id),
                        app_id,
                    )
                {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => TAKEN).increment(1);
                    self.push(
                        &[character],
                        SocialPush::GroupFinder(GroupFinderPush::AcceptApplicationSuccess),
                    );
                } else {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => REFUSED).increment(1);
                }
            }
            GroupFinderCall::RejectApplication {
                group_id,
                application_id,
            } => {
                if let Ok(app_id) = Uuid::parse_str(&application_id)
                    && self.reject_group_finder_application(
                        character,
                        Uuid::from_bytes(group_id),
                        app_id,
                    )
                {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => TAKEN).increment(1);
                    self.push(
                        &[character],
                        SocialPush::GroupFinder(GroupFinderPush::RejectApplicationSuccess),
                    );
                } else {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => REFUSED).increment(1);
                }
            }
            GroupFinderCall::RemoveApplication {
                group_id,
                application_id,
            } => {
                if let Ok(app_id) = Uuid::parse_str(&application_id)
                    && self.remove_group_finder_application(
                        character,
                        Uuid::from_bytes(group_id),
                        app_id,
                    )
                {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => TAKEN).increment(1);
                    self.push(
                        &[character],
                        SocialPush::GroupFinder(GroupFinderPush::RemoveApplicationSuccess),
                    );
                } else {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => REFUSED).increment(1);
                }
            }
            GroupFinderCall::RemoveAllApplications => {
                metrics::counter!(METRIC_CALLS, RESULT_LABEL => TAKEN).increment(1);
                self.remove_all_group_finder_applications(character);
                self.push(
                    &[character],
                    SocialPush::GroupFinder(GroupFinderPush::RemoveAllApplicationsSuccess),
                );
            }
            GroupFinderCall::ConfirmApplicationInvite {
                group_id,
                application_id,
                ..
            } => {
                if let Ok(app_id) = Uuid::parse_str(&application_id)
                    && self.confirm_group_finder_application_invite(
                        character,
                        Uuid::from_bytes(group_id),
                        app_id,
                    )
                {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => TAKEN).increment(1);
                    self.push(
                        &[character],
                        SocialPush::GroupFinder(GroupFinderPush::ConfirmApplicationInviteSuccess),
                    );
                } else {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => REFUSED).increment(1);
                }
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
            GroupFinderCall::UpdateMember { role } => {
                if self.update_group_finder_member_role(character, role) {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => TAKEN).increment(1);
                    self.push(
                        &[character],
                        SocialPush::GroupFinder(GroupFinderPush::UpdateMemberSuccess),
                    );
                } else {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => REFUSED).increment(1);
                }
            }
            GroupFinderCall::RemoveMember { character_id } => {
                if let Ok(target) = Uuid::parse_str(&character_id)
                    && self.remove_group_finder_member(character, target)
                {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => TAKEN).increment(1);
                    self.push(
                        &[character],
                        SocialPush::GroupFinder(GroupFinderPush::RemoveMemberSuccess),
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
                self.touch(character);
            }
            GroupFinderCall::ConvertToGroup => {
                let initiated = self
                    .group_finder_lobby_of(character)
                    .is_some_and(|id| self.initiate_group_finder_conversion(character, id));
                if initiated {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => TAKEN).increment(1);
                } else {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => REFUSED).increment(1);
                }
            }
            GroupFinderCall::ConfirmConvertToGroup => {
                if self.confirm_group_finder_conversion(character) {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => TAKEN).increment(1);
                } else {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => REFUSED).increment(1);
                }
            }
            GroupFinderCall::DeclineConvertToGroup => {
                if self.decline_group_finder_conversion(character) {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => TAKEN).increment(1);
                } else {
                    metrics::counter!(METRIC_CALLS, RESULT_LABEL => REFUSED).increment(1);
                }
            }
            GroupFinderCall::LobbyCounts { activity } => {
                metrics::counter!(METRIC_CALLS, RESULT_LABEL => TAKEN).increment(1);
                let mut counts = vec![0u32; LOBBY_COUNT_SLOTS];
                if let Some(first) = counts.first_mut() {
                    *first = self.group_finder_lobby_count(ANY_GAME_MODE);
                }
                self.push(
                    &[character],
                    SocialPush::GroupFinder(GroupFinderPush::LobbyCounts { activity, counts }),
                );
            }
        }
    }

    fn touch_lobby(&mut self, lobby_id: Uuid) {
        let members: Vec<Uuid> = self
            .group_finder_lobbies
            .get(&lobby_id)
            .map(|lobby| lobby.members.iter().map(|m| m.character).collect())
            .unwrap_or_default();
        for member in members {
            self.touch(member);
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

    /// Updates metadata of `creator`'s Group Finder listing (`2872`).
    fn update_group_finder_lobby(
        &mut self,
        creator: Uuid,
        metadata: GroupFinderGroupMetadata,
    ) -> bool {
        let Some(lobby_id) = self
            .group_finder_lobbies
            .values()
            .find(|lobby| lobby.creator == creator)
            .map(|lobby| lobby.id)
        else {
            return false;
        };
        if let Some(lobby) = self.group_finder_lobbies.get_mut(&lobby_id) {
            lobby.metadata = metadata;
            self.touch_lobby(lobby_id);
            return true;
        }
        false
    }

    /// Removes the Group Finder listing owned by `creator` (`1683`).
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

    /// Submits an application from `applicant` to `lobby_id` (`3510`).
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
        let app_id = {
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
            app_id
        };
        self.touch_lobby(lobby_id);
        Some(app_id)
    }

    /// Leader accepts `application_id` in `lobby_id` (`3515`) and pushes `3485` to the applicant.
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
        self.touch_lobby(lobby_id);
        self.push(
            &[applicant],
            SocialPush::GroupFinder(GroupFinderPush::ApplicationAccepted {
                group_id: *lobby_id.as_bytes(),
                application_id: application_id.hyphenated().to_string(),
            }),
        );
        true
    }

    /// Leader rejects `application_id` in `lobby_id` (`1383`) and pushes `2985` to the applicant.
    fn reject_group_finder_application(
        &mut self,
        leader: Uuid,
        lobby_id: Uuid,
        application_id: Uuid,
    ) -> bool {
        let applicant = {
            let Some(lobby) = self.group_finder_lobbies.get_mut(&lobby_id) else {
                return false;
            };
            if lobby.creator != leader {
                return false;
            }
            let Some(pos) = lobby
                .applications
                .iter()
                .position(|a| a.id == application_id)
            else {
                return false;
            };
            lobby.applications.remove(pos).applicant
        };
        self.touch_lobby(lobby_id);
        self.push(
            &[applicant],
            SocialPush::GroupFinder(GroupFinderPush::ApplicationRejected {
                group_id: *lobby_id.as_bytes(),
                application_id: application_id.hyphenated().to_string(),
            }),
        );
        true
    }

    /// Applicant withdraws one pending application (`3513`).
    fn remove_group_finder_application(
        &mut self,
        applicant: Uuid,
        lobby_id: Uuid,
        application_id: Uuid,
    ) -> bool {
        let removed = {
            let Some(lobby) = self.group_finder_lobbies.get_mut(&lobby_id) else {
                return false;
            };
            let before = lobby.applications.len();
            lobby
                .applications
                .retain(|a| !(a.id == application_id && a.applicant == applicant));
            lobby.applications.len() != before
        };
        if removed {
            self.touch_lobby(lobby_id);
        }
        removed
    }

    /// Applicant withdraws all pending applications (`1553`).
    fn remove_all_group_finder_applications(&mut self, applicant: Uuid) {
        let mut changed = Vec::new();
        for lobby in self.group_finder_lobbies.values_mut() {
            let before = lobby.applications.len();
            lobby.applications.retain(|a| a.applicant != applicant);
            if lobby.applications.len() != before {
                changed.push(lobby.id);
            }
        }
        for lobby_id in changed {
            self.touch_lobby(lobby_id);
        }
    }

    /// Applicant confirms an accepted invite (`105`) and joins `lobby_id`.
    pub fn confirm_group_finder_application_invite(
        &mut self,
        applicant: Uuid,
        lobby_id: Uuid,
        application_id: Uuid,
    ) -> bool {
        if !self.is_online(applicant) || self.group_finder_lobby_of(applicant).is_some() {
            return false;
        }
        let joined = {
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
            true
        };
        if joined {
            self.touch_lobby(lobby_id);
        }
        joined
    }

    /// Applicant declines an accepted invite (`3245`) and receives `3475`.
    pub fn decline_group_finder_application_invite(
        &mut self,
        applicant: Uuid,
        lobby_id: Uuid,
        application_id: Uuid,
    ) {
        let mut changed = false;
        if let Some(lobby) = self.group_finder_lobbies.get_mut(&lobby_id) {
            let before = lobby.applications.len();
            lobby
                .applications
                .retain(|a| !(a.id == application_id && a.applicant == applicant));
            changed = lobby.applications.len() != before;
        }
        if changed {
            self.touch_lobby(lobby_id);
        }
        self.push(
            &[applicant],
            SocialPush::GroupFinder(GroupFinderPush::DeclineApplicationInviteSuccess),
        );
    }

    /// Member updates its role inside its Group Finder listing (`3518`).
    fn update_group_finder_member_role(&mut self, member: Uuid, role: u8) -> bool {
        let Some(lobby_id) = self.group_finder_lobby_of(member) else {
            return false;
        };
        if let Some(lobby) = self.group_finder_lobbies.get_mut(&lobby_id)
            && let Some(slot) = lobby.members.iter_mut().find(|m| m.character == member)
        {
            slot.role = role;
            self.touch_lobby(lobby_id);
            return true;
        }
        false
    }

    /// Leader kicks `target` (or `actor` leaves itself) from the Group Finder listing (`3519`).
    fn remove_group_finder_member(&mut self, actor: Uuid, target: Uuid) -> bool {
        let Some(lobby_id) = self.group_finder_lobby_of(actor) else {
            return false;
        };
        let (removed, kicked, disband) = {
            let Some(lobby) = self.group_finder_lobbies.get_mut(&lobby_id) else {
                return false;
            };
            if actor != target && lobby.creator != actor {
                return false;
            }
            let before = lobby.members.len();
            lobby.members.retain(|m| m.character != target);
            lobby.conversion_confirmations.remove(&target);
            if lobby.members.len() == before {
                return false;
            }
            let disband = lobby.members.is_empty();
            if !disband && lobby.creator == target {
                lobby.creator = lobby.members[0].character;
            }
            (true, actor != target, disband)
        };
        if !removed {
            return false;
        }
        if disband {
            self.group_finder_lobbies.remove(&lobby_id);
        } else {
            self.touch_lobby(lobby_id);
        }
        self.touch(target);
        if kicked {
            self.push(
                &[target],
                SocialPush::GroupFinder(GroupFinderPush::KickedFromGroup),
            );
        }
        true
    }

    /// Leader initiates converting `lobby_id` into a real group (`4723`).
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
        } else {
            self.touch_lobby(lobby_id);
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
        } else {
            self.touch_lobby(lobby_id);
        }
        true
    }

    /// Member declines converting its Group Finder lobby (`4744`).
    fn decline_group_finder_conversion(&mut self, member: Uuid) -> bool {
        let Some(lobby_id) = self.group_finder_lobby_of(member) else {
            return false;
        };
        if let Some(lobby) = self.group_finder_lobbies.get_mut(&lobby_id) {
            lobby.pending_conversion = false;
            lobby.conversion_confirmations.clear();
            self.touch_lobby(lobby_id);
            return true;
        }
        false
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
        let world_name = self.rules.world_id.hyphenated().to_string();
        let matching: Vec<GroupFinderGroupListEntry> = self
            .group_finder_lobbies
            .values()
            .filter(|lobby| {
                game_mode_crc == ANY_GAME_MODE || lobby.metadata.game_mode_crc == game_mode_crc
            })
            .map(|lobby| GroupFinderGroupListEntry {
                group_id: *lobby.id.as_bytes(),
                metadata: lobby.metadata.clone(),
                members: lobby.members.iter().map(|m| self.member_blob(m)).collect(),
                world_name: world_name.clone(),
                leader_id: CharacterRef(Some(*lobby.creator.as_bytes())),
                tail: GroupFinderListEntryTail::default(),
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

    /// Builds the replicated `GroupFinderGroupDataState` (`1091`) for `character` when it is in a
    /// Group Finder listing. Slot 3 (applications) is replicated to the lobby creator or to any
    /// member subscribed via `5355`.
    pub(crate) fn group_finder_entity_view(
        &self,
        character: Uuid,
    ) -> Option<(Uuid, GroupFinderGroupDataState)> {
        let lobby_id = self.group_finder_lobby_of(character)?;
        let lobby = self.group_finder_lobbies.get(&lobby_id)?;
        let members: Vec<GroupFinderMemberBlob> =
            lobby.members.iter().map(|m| self.member_blob(m)).collect();
        let has_pending = !lobby.applications.is_empty();
        let send_apps =
            lobby.creator == character || self.group_finder_subscribers.contains(&character);
        let (application_ids, application_member_blobs, application_states) = if send_apps {
            let ids = lobby
                .applications
                .iter()
                .map(|a| a.id.hyphenated().to_string())
                .collect();
            let blobs = lobby
                .applications
                .iter()
                .map(|a| {
                    self.member_blob(&GroupFinderMember {
                        character: a.applicant,
                        role: a.role,
                        gear_score: a.gear_score,
                    })
                })
                .collect();
            let states = lobby
                .applications
                .iter()
                .map(|a| match a.status {
                    ApplicationStatus::Pending => APP_STATE_PENDING,
                    ApplicationStatus::Accepted => APP_STATE_ACCEPTED,
                })
                .collect();
            (
                Some(FullList::first(ids)),
                Some(FullList::first(blobs)),
                Some(FullList::first(states)),
            )
        } else {
            (None, None, None)
        };
        let state = GroupFinderGroupDataState {
            group_id: Some(*lobby.id.as_bytes()),
            metadata: Some(lobby.metadata.clone()),
            member_blobs: Some(FullList::first(members)),
            is_pending_conversion: Some(lobby.pending_conversion),
            pending_conversion_confirmations: Some(
                u8::try_from(lobby.conversion_confirmations.len()).unwrap_or(u8::MAX),
            ),
            has_pending_applications: Some(has_pending),
            application_ids,
            application_member_blobs,
            application_states,
        };
        Some((lobby.id, state))
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
        let mut touched = Vec::new();
        for lobby in self.group_finder_lobbies.values_mut() {
            let app_before = lobby.applications.len();
            lobby.applications.retain(|a| a.applicant != character);
            lobby.conversion_confirmations.remove(&character);
            let before = lobby.members.len();
            lobby.members.retain(|m| m.character != character);
            if lobby.members.len() != before {
                if lobby.members.is_empty() {
                    empty.push(lobby.id);
                } else {
                    if lobby.creator == character {
                        lobby.creator = lobby.members[0].character;
                    }
                    touched.push(lobby.id);
                }
            } else if lobby.applications.len() != app_before {
                touched.push(lobby.id);
            }
        }
        for id in empty {
            self.group_finder_lobbies.remove(&id);
        }
        for id in touched {
            self.touch_lobby(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use nw_messages::lookups::REQUEST_LOBBY_COUNTS_TYPE_INDEX;
    use nw_messages::social::group_finder::{
        ACCEPT_APPLICATION_TYPE_INDEX, CONFIRM_APPLICATION_INVITE_TYPE_INDEX,
        CONFIRM_CONVERT_TO_GROUP_TYPE_INDEX, CONVERT_TO_GROUP_TYPE_INDEX, CREATE_GROUP_TYPE_INDEX,
        DECLINE_APPLICATION_INVITE_TYPE_INDEX, GET_GROUP_LIST_TYPE_INDEX, GET_GROUP_TYPE_INDEX,
        MODIFY_APPLICATION_SUBSCRIPTION_TYPE_INDEX, SUBMIT_APPLICATION_TYPE_INDEX,
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

    fn send(world: &mut SocialWorld, who: u128, gf_call: GroupFinderCall) -> Vec<Effect> {
        let type_index = gf_call.type_index();
        let body = gf_call.encode(HEADER).expect("encode");
        call(world, who, "sim-1", type_index, &body[16..], NOW_MS)
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
    fn full_group_finder_rpc_flow_end_to_end() {
        let mut world = world();
        join(&mut world, 1, "Ann", "sim-1");
        join(&mut world, 2, "Bob", "sim-1");
        let ann = character(1);
        let bob = character(2);

        // 1. Ann creates a listing over RPC (3509 -> 3374 + 1091 entity delivery).
        let effects = send(
            &mut world,
            1,
            GroupFinderCall::CreateGroup {
                metadata: metadata(),
                role: 4,
                role_sets: vec![vec![2, 3, 4]],
            },
        );
        assert_eq!(
            pushes_for(&effects, ann),
            [GroupFinderPush::CreateGroupSuccess]
        );
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::GroupFinderEntity { delivery, .. } if delivery.character == ann && delivery.view.is_some())),
            "creator receives 1091 entity on create"
        );
        let lobby_id = world.group_finder_lobby_of(ann).expect("ann has lobby");
        assert_eq!(CREATE_GROUP_TYPE_INDEX, 3509);

        // 2. Bob asks for live lobby counts (5463 -> 5458 with count 1).
        let effects = send(&mut world, 2, GroupFinderCall::LobbyCounts { activity: 1 });
        assert_eq!(REQUEST_LOBBY_COUNTS_TYPE_INDEX, 5463);
        let pushes = pushes_for(&effects, bob);
        let [GroupFinderPush::LobbyCounts { activity, counts }] = pushes.as_slice() else {
            panic!("expected 5458 LobbyCounts");
        };
        assert_eq!(
            (*activity, counts[0], counts.len()),
            (1, 1, LOBBY_COUNT_SLOTS)
        );

        // 3. Bob lists (1960 -> 3461) and inspects (1147 -> 2010).
        let effects = send(
            &mut world,
            2,
            GroupFinderCall::GetGroupList {
                filter_flag: 1,
                game_mode_crc: AMRINE_CRC,
                page: 0,
            },
        );
        assert_eq!(GET_GROUP_LIST_TYPE_INDEX, 1960);
        let list_pushes = pushes_for(&effects, bob);
        let [GroupFinderPush::GetGroupListSuccess { groups, .. }] = list_pushes.as_slice() else {
            panic!("expected 3461");
        };
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].group_id, *lobby_id.as_bytes());
        assert_eq!(groups[0].members.len(), 1);

        let effects = send(
            &mut world,
            2,
            GroupFinderCall::GetGroup {
                group_id: *lobby_id.as_bytes(),
            },
        );
        assert_eq!(GET_GROUP_TYPE_INDEX, 1147);
        assert!(matches!(
            pushes_for(&effects, bob).as_slice(),
            [GroupFinderPush::GetGroupSuccess { .. }]
        ));

        // 4. Bob applies (3510 -> 861), Ann accepts (3515 -> 3470 + 3485 to Bob), Bob declines (3245 -> 3475).
        let effects = send(
            &mut world,
            2,
            GroupFinderCall::SubmitApplication {
                group_id: *lobby_id.as_bytes(),
                role: 3,
            },
        );
        assert_eq!(SUBMIT_APPLICATION_TYPE_INDEX, 3510);
        assert_eq!(
            pushes_for(&effects, bob),
            [GroupFinderPush::SubmitApplicationSuccess]
        );
        let app1 = world.group_finder_lobbies[&lobby_id].applications[0].id;
        let effects = send(
            &mut world,
            1,
            GroupFinderCall::AcceptApplication {
                group_id: *lobby_id.as_bytes(),
                application_id: app1.hyphenated().to_string(),
            },
        );
        assert_eq!(ACCEPT_APPLICATION_TYPE_INDEX, 3515);
        assert_eq!(
            pushes_for(&effects, ann),
            [GroupFinderPush::AcceptApplicationSuccess]
        );
        assert_eq!(
            pushes_for(&effects, bob),
            [GroupFinderPush::ApplicationAccepted {
                group_id: *lobby_id.as_bytes(),
                application_id: app1.hyphenated().to_string(),
            }]
        );
        let effects = send(
            &mut world,
            2,
            GroupFinderCall::DeclineApplicationInvite {
                group_id: *lobby_id.as_bytes(),
                application_id: app1.hyphenated().to_string(),
                reason: 0,
            },
        );
        assert_eq!(DECLINE_APPLICATION_INVITE_TYPE_INDEX, 3245);
        assert_eq!(
            pushes_for(&effects, bob),
            [GroupFinderPush::DeclineApplicationInviteSuccess]
        );

        // 5. Bob re-applies, Ann accepts, Bob confirms invite (105 -> 1708) and subscribes (5355).
        send(
            &mut world,
            2,
            GroupFinderCall::SubmitApplication {
                group_id: *lobby_id.as_bytes(),
                role: 3,
            },
        );
        let app2 = world.group_finder_lobbies[&lobby_id].applications[0].id;
        send(
            &mut world,
            1,
            GroupFinderCall::AcceptApplication {
                group_id: *lobby_id.as_bytes(),
                application_id: app2.hyphenated().to_string(),
            },
        );
        let effects = send(
            &mut world,
            2,
            GroupFinderCall::ConfirmApplicationInvite {
                group_id: *lobby_id.as_bytes(),
                application_id: app2.hyphenated().to_string(),
                role: 3,
            },
        );
        assert_eq!(CONFIRM_APPLICATION_INVITE_TYPE_INDEX, 105);
        assert_eq!(
            pushes_for(&effects, bob),
            [GroupFinderPush::ConfirmApplicationInviteSuccess]
        );
        let effects = send(
            &mut world,
            2,
            GroupFinderCall::ModifyApplicationSubscription { subscribed: true },
        );
        assert_eq!(MODIFY_APPLICATION_SUBSCRIPTION_TYPE_INDEX, 5355);
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::GroupFinderEntity { delivery, .. } if delivery.character == bob && delivery.view.is_some())),
            "subscribing pushes updated 1091 with Slot 3 applications"
        );

        // 6. Ann initiates conversion (4723), Bob confirms (4739) -> real group saved, 1091 cleared.
        assert_eq!(CONVERT_TO_GROUP_TYPE_INDEX, 4723);
        send(&mut world, 1, GroupFinderCall::ConvertToGroup);
        assert_eq!(CONFIRM_CONVERT_TO_GROUP_TYPE_INDEX, 4739);
        let effects = send(&mut world, 2, GroupFinderCall::ConfirmConvertToGroup);
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::Store(StoreOp::SaveGroup(g)) if g.members.len() == 2)),
            "converting lobby creates and stores a real 2-player group"
        );
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::GroupFinderEntity { delivery, .. } if delivery.character == bob && delivery.view.is_none())),
            "1091 entity removed on conversion"
        );
        assert_eq!(world.group_finder_lobby_count(AMRINE_CRC), 0);
        assert_eq!(world.people[&ann].group, world.people[&bob].group);
    }
}
