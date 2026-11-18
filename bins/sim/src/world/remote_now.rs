// SPDX-License-Identifier: AGPL-3.0-or-later
//! Other players' states at the half tick. Retail sent a player's states to the players who
//! see it on a steady 16 ms beat: a new state number came with each input record of its client
//! (33 or 50 ms), the first state of a new number reached a viewer 12 ms (median) after the
//! player's own record, and the gaps between states were 12 to 16 ms in 66 % of them, under
//! 8 ms in under 1 % (Observed, 2026-11-18 timing study of 398 ledgers). The viewer's client
//! sets its delay for another player from the gaps between that player's arrivals
//! (`FUN_144873bf0`, `FUN_144872440`, RTT study 2026-11-18). Ours built a player's state once per
//! tick, and then sent each new state at its input and once more 16 ms later: a third of the
//! gaps fell under 8 ms (live 2026-11-18).
//!
//! Half a tick after each tick, the players whose input gave them a new state number since
//! that tick send it to their viewers ([`SimWorld::states_now`]); the next tick sends the next
//! one. Only players' states go at the half tick: no mob, card or other entity, and no mob work.
//! The states without a change count against the next tick's budget of remote states
//! ([`super::replica`]).

use nw_messages::{BundleGroup, MessageError};
use nw_sim_core::Position;

use super::members::{log_player_state, own_state_counter};
use super::replica::{RemoteCandidate, Replica};
use super::{
    Body, Outgoing, Player, SimWorld, Vitals, others_held, positions_only, reliable_bundles,
    replication_outgoing, sequenced_bundles,
};

impl SimWorld {
    /// The states of the players whose input since the last tick gave them a new state number,
    /// at the half tick `now_ms` (server time `server_time`), for the players who see them:
    /// each viewer gets them in one bundle. A player whose state already moved on since its last
    /// own state waits for the tick, so each own state carries one step of its slayer state.
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
