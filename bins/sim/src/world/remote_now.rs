// SPDX-License-Identifier: AGPL-3.0-or-later
//! Other players' states between the ticks. Retail sent a player's states to the players who
//! see it every 16 ms: a new state number came with each input record of its client (33 or
//! 50 ms), the first state of a new number reached a viewer 12 ms (median) after the player's
//! own record, and each state went 2 to 4 times (Observed, 2026-11-18 timing study of 398
//! ledgers). Ours built a player's state once per tick and sent it once, so a viewer saw it
//! up to a tick late.
//!
//! When an input gives a player a new state number, its viewers get that state at once
//! ([`SimWorld::states_now`]), and once more [`STATE_REPEAT_MS`] later
//! ([`SimWorld::repeat_states`]), or with the next tick when that comes first. Only players'
//! states go between ticks: no mob, card or other entity, and no mob work. The states without
//! a change count against the next tick's budget of remote states ([`super::replica`]).

use std::collections::HashMap;

use nw_messages::{BundleGroup, MessageError};
use nw_sim_core::Position;

use super::members::{log_player_state, own_state_counter};
use super::replica::{RemoteCandidate, Replica};
use super::{
    Body, Outgoing, Player, SimWorld, Vitals, others_held, positions_only, reliable_bundles,
    replication_outgoing, sequenced_bundles,
};

/// How long after a player's new state its viewers get it once more, ms (Observed, 2026-11-18
/// timing study of 398 retail ledgers: other players' states came a median 16 ms apart, and 44 %
/// of them repeated the state before).
pub(super) const STATE_REPEAT_MS: u64 = 16;

/// A player's new state sent between ticks, which its viewers get once more.
#[derive(Debug)]
pub(super) struct StateRepeat {
    replica: Replica,
    /// When it goes once more (sim clock, ms).
    due_ms: u64,
    /// The players who got it.
    viewers: Vec<u64>,
}

impl SimWorld {
    /// The states of the players whose input since the last call gave them a new state number,
    /// at `now_ms` (server time `server_time`), for the players who see them: each viewer gets
    /// them now in one bundle, and once more [`STATE_REPEAT_MS`] later. A player whose state
    /// already moved on since its last own state waits for the tick, so each own state carries
    /// one step of its slayer state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a state cannot be encoded.
    pub fn states_now(
        &mut self,
        now_ms: u64,
        server_time: u64,
    ) -> Result<Vec<Outgoing>, MessageError> {
        let progression = &self.content.progression;
        let mut sources: Vec<(u64, Position, Replica)> = Vec::new();
        let mut query = self.world.query::<(&mut Player, &Body, &Vitals)>();
        for (mut player, body, vitals) in query.iter_mut(&mut self.world) {
            let new_number = player.input_counter.is_some_and(|counter| {
                player
                    .state_number
                    .is_none_or(|number| number.number != own_state_counter(counter))
            });
            if player.state_stepped || !new_number {
                continue;
            }
            Self::step_state(&mut player, now_ms, server_time);
            player.state_stepped = true;
            let position = body.motion.position();
            let source = Self::replica_source(&player, vitals, server_time, progression);
            let seen = self.replica_memory.get(&player.number).copied();
            let (replica, memory) = Replica::new(source, position, seen, server_time);
            self.replica_memory.insert(player.number, memory);
            sources.push((player.number, position, replica));
        }
        if sources.is_empty() || positions_only() {
            return Ok(Vec::new());
        }
        let local_player_card = self.settings.local_player_card;
        let home_world = *self.settings.world_id.as_bytes();
        let mut viewers: HashMap<u64, Vec<u64>> = HashMap::new();
        let mut outgoing = Vec::new();
        let mut query = self.world.query::<(&mut Player, &Body, &Vitals)>();
        for (mut viewer, body, vitals) in query.iter_mut(&mut self.world) {
            if viewer.frozen
                || others_held(
                    local_player_card,
                    vitals.spawned,
                    viewer.others_due_ms,
                    now_ms,
                )
            {
                continue;
            }
            let own = viewer.number;
            let own_at = body.motion.position();
            let mut groups: Vec<BundleGroup> = Vec::new();
            for (number, position, replica) in sources.iter().filter(|(number, ..)| *number != own)
            {
                // A player without a slot yet gets its card with the tick.
                let Some(&interest) = viewer.slots.assigned().get(number) else {
                    continue;
                };
                let candidate = RemoteCandidate {
                    number: *number,
                    distance_m: own_at.horizontal_distance(*position),
                    replica,
                    new: false,
                };
                if !viewer.remote_plan.send_now(&candidate) {
                    continue;
                }
                let sight = viewer.remote_plan.sight(*number);
                let Some(group) =
                    replica.group(interest, *number, *position, false, home_world, sight)?
                else {
                    continue;
                };
                log_player_state(own, *number, &group);
                groups.push(group);
                viewers.entry(*number).or_default().push(own);
            }
            outgoing.extend(Self::remote_bundles(&mut viewer, groups)?);
        }
        let due_ms = now_ms.saturating_add(STATE_REPEAT_MS);
        for (number, _, replica) in sources {
            if let Some(viewers) = viewers.remove(&number) {
                self.state_repeats.insert(
                    number,
                    StateRepeat {
                        replica,
                        due_ms,
                        viewers,
                    },
                );
            }
        }
        Ok(outgoing)
    }

    /// The states sent between ticks whose repeat is due at `now_ms`, once more to the players
    /// who got them and still have their slots ([`Replica::repeat_group`]).
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a state cannot be encoded.
    pub fn repeat_states(&mut self, now_ms: u64) -> Result<Vec<Outgoing>, MessageError> {
        let due: Vec<u64> = self
            .state_repeats
            .iter()
            .filter(|(_, repeat)| repeat.due_ms <= now_ms)
            .map(|(number, _)| *number)
            .collect();
        let repeats: Vec<(u64, StateRepeat)> = due
            .into_iter()
            .filter_map(|number| Some((number, self.state_repeats.remove(&number)?)))
            .collect();
        if repeats.is_empty() {
            return Ok(Vec::new());
        }
        let mut outgoing = Vec::new();
        for mut viewer in self.world.query::<&mut Player>().iter_mut(&mut self.world) {
            if viewer.frozen {
                continue;
            }
            let own = viewer.number;
            let mut groups: Vec<BundleGroup> = Vec::new();
            for (number, repeat) in &repeats {
                if !repeat.viewers.contains(&own) {
                    continue;
                }
                let Some(&interest) = viewer.slots.assigned().get(number) else {
                    continue;
                };
                if !viewer.remote_plan.repeat_now() {
                    continue;
                }
                let sight = viewer.remote_plan.sight(*number);
                let group = repeat.replica.repeat_group(interest, sight)?;
                log_player_state(own, *number, &group);
                groups.push(group);
            }
            outgoing.extend(Self::remote_bundles(&mut viewer, groups)?);
        }
        Ok(outgoing)
    }

    /// The bundles of the remote states `groups` for `viewer`, on the replication channel.
    fn remote_bundles(
        viewer: &mut Player,
        groups: Vec<BundleGroup>,
    ) -> Result<Vec<Outgoing>, MessageError> {
        if groups.is_empty() {
            return Ok(Vec::new());
        }
        let envelopes = sequenced_bundles(viewer, groups)?;
        Ok(replication_outgoing(&viewer.connection, reliable_bundles(), envelopes).collect())
    }
}
