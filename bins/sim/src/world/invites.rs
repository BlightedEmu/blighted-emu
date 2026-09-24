// SPDX-License-Identifier: AGPL-3.0-or-later
//! Generic invites between two players of this sim (economy note
//! `local-data/scratch-work/run15/economy/REPORT.md`, section 4): the `Trade` activity, which
//! starts a player trade when the invited player accepts.
//!
//! The invite (3404) names the invited character. When both players play here, have no other
//! invite or trade, and stand within the activity's `MaxDistance`, the invite goes into the
//! invite state (3408) of both players: its id, the activity, the forward type, the inviter as
//! the one participant, and the expiry `DurationSec` later (retail: the same state on the inviter
//! and the invited player of four captures). The invited player's accept (3406) clears both
//! states, as retail does, and starts the trade (2652 to both); a decline clears them and tells
//! the inviter (2454 [`end::INSUFFICIENT_PARTICIPANTS`]). A refused invite or accept is 2454
//! [`end::ELIGIBILITY_FAILED`] to the caller (retail: 1 to an accept that failed). An invite
//! that runs out is cleared at the next economy round.
//!
//! The other activities (duel, musical performance, shard) are not served here: their invite is
//! refused with [`end::SYSTEM_ERROR`].

use std::collections::HashMap;

use bevy_ecs::entity::Entity;
use nw_messages::generic_invite::{
    GENERIC_INVITE_COMPONENT, GENERIC_INVITE_STATE_KEY, GenericInviteState, InviteCall,
    InviteDetails, InviteExpiry, InviteFailedMsg, InviteParticipant, NO_PLATFORM, end,
};
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{BundleMember, EconomyMessage, FacetHeader, MemberState, ServerMessage};
use uuid::Uuid;

use super::{Body, Player, SimWorld, send_now};

/// The activity this sim serves (`GenericInvites` `ActivityCrc`).
const TRADE_ACTIVITY: &str = "Trade";
/// Nanoseconds per millisecond: the expiry is server nanoseconds since 2000-01-01.
const NS_PER_MS: u64 = 1_000_000;
/// The activity of a failure that names no invite.
const NO_ACTIVITY: u32 = 0;

/// One invite that waits for its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingInvite {
    /// The CRC-32 of the lowercase activity.
    activity: u32,
    /// The inviter's number and entity.
    inviter: (u64, Entity),
    /// The invited player's number and entity.
    invitee: (u64, Entity),
    /// Server time (ms since 2000-01-01) when it runs out.
    expires_ms: u64,
}

/// The invites of this sim's players.
#[derive(Debug, Default)]
pub(super) struct Invites {
    /// The waiting invites by id.
    pending: HashMap<[u8; RAW_ID_LEN], PendingInvite>,
}

impl Invites {
    /// The id of the invite player `number` takes part in.
    fn of(&self, number: u64) -> Option<[u8; RAW_ID_LEN]> {
        self.pending
            .iter()
            .find(|(_, invite)| invite.inviter.0 == number || invite.invitee.0 == number)
            .map(|(id, _)| *id)
    }
}

impl SimWorld {
    /// Takes one generic invite call of player `entity`.
    pub(super) fn invite_call(&mut self, entity: Entity, call: &InviteCall) {
        let Some(number) = self.world.get::<Player>(entity).map(|player| player.number) else {
            return;
        };
        match call {
            InviteCall::Send {
                activity,
                forward,
                target,
                ..
            } => {
                if let Err(reason) = self.send_invite(entity, number, *activity, *forward, target)
                {
                    tracing::info!(player = number, reason, "invite refused");
                    self.invite_failed(entity, number, *activity, reason);
                }
            }
            InviteCall::Reply { accept, .. } => self.reply_to_invite(entity, number, *accept),
        }
    }

    /// Player `number` (`entity`) invites the character of UUID text `target` to `activity`.
    fn send_invite(
        &mut self,
        entity: Entity,
        number: u64,
        activity: u32,
        forward: u8,
        target: &[u8],
    ) -> Result<(), u32> {
        let rules = self
            .content
            .invites
            .activity(activity)
            .filter(|rules| rules.name.eq_ignore_ascii_case(TRADE_ACTIVITY))
            .ok_or(end::SYSTEM_ERROR)?;
        let (duration_ms, max_distance_m) = (rules.duration_ms, rules.max_distance_m);
        let character = std::str::from_utf8(target)
            .ok()
            .and_then(|text| Uuid::parse_str(text).ok())
            .ok_or(end::ELIGIBILITY_FAILED)?;
        let (invitee, invitee_number) = self
            .by_connection
            .values()
            .find_map(|other| {
                let player = self.world.get::<Player>(*other)?;
                (player.character == character).then_some((*other, player.number))
            })
            .ok_or(end::ELIGIBILITY_FAILED)?;
        let busy = |world: &Self, number| {
            world.economy.invites.of(number).is_some() || world.trading(number)
        };
        if invitee_number == number || busy(self, number) || busy(self, invitee_number) {
            return Err(end::ELIGIBILITY_FAILED);
        }
        if !self.within(entity, invitee, max_distance_m) {
            return Err(end::ELIGIBILITY_FAILED);
        }
        let inviter_character = self
            .world
            .get::<Player>(entity)
            .map(|player| player.character)
            .ok_or(end::ELIGIBILITY_FAILED)?;
        let id = *Uuid::new_v4().as_bytes();
        let expires_ms = self.server_ms.saturating_add(duration_ms);
        self.economy.invites.pending.insert(
            id,
            PendingInvite {
                activity,
                inviter: (number, entity),
                invitee: (invitee_number, invitee),
                expires_ms,
            },
        );
        let state = GenericInviteState {
            invite_id: Some(id),
            details: Some(InviteDetails {
                activity,
                forward,
                participants: vec![InviteParticipant {
                    character: *inviter_character.as_bytes(),
                    platform: NO_PLATFORM,
                    platform_id: Vec::new(),
                }],
                expiry: InviteExpiry {
                    word: 0,
                    since_2000_ns: expires_ms.saturating_mul(NS_PER_MS),
                },
            }),
        };
        self.put_invite_state(entity, state.clone());
        self.put_invite_state(invitee, state);
        tracing::debug!(inviter = number, invitee = invitee_number, "trade invite sent");
        Ok(())
    }

    /// Player `number` (`entity`) answers the invite it got.
    fn reply_to_invite(&mut self, entity: Entity, number: u64, accept: bool) {
        let found = self
            .economy
            .invites
            .pending
            .iter()
            .find(|(_, invite)| invite.invitee.0 == number)
            .map(|(id, invite)| (*id, *invite));
        let Some((id, invite)) = found else {
            if accept {
                // No invite waits for this player: the activity is unknown, so none is named.
                self.invite_failed(entity, number, NO_ACTIVITY, end::ELIGIBILITY_FAILED);
            }
            return;
        };
        self.drop_invite(id);
        if invite.expires_ms <= self.server_ms {
            if accept {
                self.invite_failed(entity, number, invite.activity, end::ELIGIBILITY_FAILED);
            }
            return;
        }
        let (inviter_number, inviter) = invite.inviter;
        if !accept {
            self.invite_failed(
                inviter,
                inviter_number,
                invite.activity,
                end::INSUFFICIENT_PARTICIPANTS,
            );
            return;
        }
        let max_distance_m = self
            .content
            .invites
            .activity(invite.activity)
            .map_or(0.0, |rules| rules.max_distance_m);
        if !self.within(inviter, entity, max_distance_m) || !self.start_trade(inviter, entity) {
            self.invite_failed(entity, number, invite.activity, end::ELIGIBILITY_FAILED);
        }
    }

    /// Clears the invites that ran out by now.
    pub(super) fn expire_invites(&mut self) {
        let now_ms = self.server_ms;
        let expired: Vec<[u8; RAW_ID_LEN]> = self
            .economy
            .invites
            .pending
            .iter()
            .filter(|(_, invite)| invite.expires_ms <= now_ms)
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            self.drop_invite(id);
        }
    }

    /// Player `number` left: its invite ends.
    pub(super) fn invites_left(&mut self, number: u64) {
        if let Some(id) = self.economy.invites.of(number) {
            self.drop_invite(id);
        }
    }

    /// Ends invite `id` without its activity: both states are cleared.
    fn drop_invite(&mut self, id: [u8; RAW_ID_LEN]) {
        if let Some(invite) = self.economy.invites.pending.remove(&id) {
            self.put_invite_state(invite.inviter.1, GenericInviteState::cleared());
            self.put_invite_state(invite.invitee.1, GenericInviteState::cleared());
        }
    }

    /// Whether players `first` and `second` stand within `max_m` of each other on the ground.
    fn within(&self, first: Entity, second: Entity, max_m: f32) -> bool {
        let place = |entity| {
            self.world
                .get::<Body>(entity)
                .map(|body| body.motion.position())
        };
        match (place(first), place(second)) {
            (Some(a), Some(b)) => a.horizontal_distance(b) <= max_m,
            _ => false,
        }
    }

    fn put_invite_state(&mut self, entity: Entity, state: GenericInviteState) {
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.extra_members.push(BundleMember {
                component: GENERIC_INVITE_STATE_KEY,
                state: MemberState::GenericInvite(state),
            });
        }
    }

    /// Tells player `number` (`entity`) that its invite to `activity` failed for `reason`.
    fn invite_failed(&mut self, entity: Entity, number: u64, activity: u32, reason: u32) {
        let Some(connection) = self
            .world
            .get::<Player>(entity)
            .map(|player| player.connection.clone())
        else {
            return;
        };
        let failed = ServerMessage::Economy(EconomyMessage::InviteFailed(InviteFailedMsg {
            header: FacetHeader::of(GENERIC_INVITE_COMPONENT, number),
            activity,
            reason,
        }));
        send_now(&mut self.combat_messages, &connection, &[failed]);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nw_messages::p2p_trade::TradeMessage;

    use super::*;
    use crate::invite_rules::{InviteActivity, InviteRules};
    use crate::player_content::PlayerContent;
    use crate::world::tests::{NOW, SERVER_TIME, admit_at, messages_to, world_with_spawns};

    const INVITER: u64 = 1;
    const INVITEE: u64 = 2;
    /// The CRC-32 of `trade` and of `duel`.
    const TRADE: u32 = 0x7e1a_4366;
    const DUEL: u32 = 0x9bb4_a762;
    /// The client's `Trade` row.
    const TRADE_SECONDS_MS: u64 = 10_000;
    const TRADE_DISTANCE_M: f32 = 10.0;
    /// Retail forward type.
    const FORWARD: u8 = 1;
    const NEAR_X: f32 = 1.0;
    const FAR_X: f32 = 50.0;

    /// A world with the trade rules, the inviter at x 0 and the invited player at `x`.
    fn two_players(x: f32) -> (SimWorld, Entity, Entity) {
        let mut world = world_with_spawns(Vec::new());
        world.content = Arc::new(PlayerContent {
            invites: InviteRules::of([InviteActivity {
                name: TRADE_ACTIVITY.to_owned(),
                duration_ms: TRADE_SECONDS_MS,
                max_distance_m: TRADE_DISTANCE_M,
            }]),
            ..PlayerContent::default()
        });
        for (number, at) in [(INVITER, 0.0), (INVITEE, x)] {
            world
                .admit(&admit_at(number, at, true), NOW, SERVER_TIME)
                .expect("admit");
        }
        let inviter = world.player_by_number(INVITER).expect("inviter");
        let invitee = world.player_by_number(INVITEE).expect("invitee");
        (world, inviter, invitee)
    }

    fn invite(world: &mut SimWorld, from: Entity, to: Entity, activity: u32) {
        let target = world
            .world
            .get::<Player>(to)
            .expect("player")
            .character
            .hyphenated()
            .to_string()
            .into_bytes();
        let call = InviteCall::Send {
            header: FacetHeader::of(GENERIC_INVITE_COMPONENT, INVITER),
            activity,
            forward: FORWARD,
            target,
        };
        world.invite_call(from, &call);
    }

    fn reply(world: &mut SimWorld, from: Entity, accept: bool) {
        let call = InviteCall::Reply {
            header: FacetHeader::of(GENERIC_INVITE_COMPONENT, INVITEE),
            accept,
        };
        world.invite_call(from, &call);
    }

    /// The invite states queued for player `entity`.
    fn invite_states(world: &SimWorld, entity: Entity) -> Vec<GenericInviteState> {
        world
            .world
            .get::<Player>(entity)
            .expect("player")
            .extra_members
            .iter()
            .filter_map(|member| match &member.state {
                MemberState::GenericInvite(state) => Some(state.clone()),
                _ => None,
            })
            .collect()
    }

    /// The reasons of the 2454 messages sent to player `number`.
    fn failures(world: &mut SimWorld, number: u64) -> Vec<u32> {
        messages_to(&world.take_messages(), number)
            .into_iter()
            .filter_map(|message| match message {
                ServerMessage::Economy(EconomyMessage::InviteFailed(failed)) => {
                    Some(failed.reason)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_accepted_trade_invite_starts_the_trade() {
        let (mut world, inviter, invitee) = two_players(NEAR_X);
        invite(&mut world, inviter, invitee, TRADE);
        let sent = invite_states(&world, invitee);
        let details = sent[0].details.clone().expect("the invite");
        assert_eq!(details.activity, TRADE);
        assert_eq!(details.forward, FORWARD);
        let inviter_id = world.world.get::<Player>(inviter).expect("player").character;
        assert_eq!(details.participants[0].character, *inviter_id.as_bytes());
        assert_eq!(details.participants[0].platform, NO_PLATFORM);
        assert_eq!(
            details.expiry.since_2000_ns,
            (world.server_ms + TRADE_SECONDS_MS) * NS_PER_MS
        );
        assert_eq!(invite_states(&world, inviter), sent, "both states hold the invite");
        reply(&mut world, invitee, true);
        assert_eq!(
            invite_states(&world, invitee).last(),
            Some(&GenericInviteState::cleared())
        );
        assert!(world.trading(INVITER) && world.trading(INVITEE));
        let started = messages_to(&world.take_messages(), INVITEE)
            .into_iter()
            .any(|message| {
                matches!(
                    message,
                    ServerMessage::Economy(EconomyMessage::Trade(TradeMessage::Started { .. }))
                )
            });
        assert!(started, "2652 to the invited player");
    }

    #[test]
    fn a_decline_tells_the_inviter_and_a_late_accept_fails() {
        let (mut world, inviter, invitee) = two_players(NEAR_X);
        invite(&mut world, inviter, invitee, TRADE);
        reply(&mut world, invitee, false);
        assert_eq!(
            failures(&mut world, INVITER),
            vec![end::INSUFFICIENT_PARTICIPANTS]
        );
        reply(&mut world, invitee, true);
        assert_eq!(failures(&mut world, INVITEE), vec![end::ELIGIBILITY_FAILED]);
        assert!(!world.trading(INVITEE));
    }

    #[test]
    fn a_far_player_and_another_activity_are_refused() {
        let (mut world, inviter, invitee) = two_players(FAR_X);
        invite(&mut world, inviter, invitee, TRADE);
        assert_eq!(failures(&mut world, INVITER), vec![end::ELIGIBILITY_FAILED]);
        assert!(invite_states(&world, invitee).is_empty());
        invite(&mut world, inviter, invitee, DUEL);
        assert_eq!(failures(&mut world, INVITER), vec![end::SYSTEM_ERROR]);
    }

    #[test]
    fn an_invite_that_ran_out_is_cleared() {
        let (mut world, inviter, invitee) = two_players(NEAR_X);
        invite(&mut world, inviter, invitee, TRADE);
        world.server_ms += TRADE_SECONDS_MS;
        world.expire_invites();
        assert_eq!(
            invite_states(&world, inviter).last(),
            Some(&GenericInviteState::cleared())
        );
        reply(&mut world, invitee, true);
        assert_eq!(failures(&mut world, INVITEE), vec![end::ELIGIBILITY_FAILED]);
    }
}
