// SPDX-License-Identifier: AGPL-3.0-or-later
//! The sim's side of the instance bus (`nw_bus::instance`): the expedition requests of its
//! players to the director and their answers, and the director's commands to an instance sim
//! (open, close, add a member). The world decides ([`crate::world::instances`]); this module
//! only carries the messages.

use std::time::Duration;

use nw_bus::{
    Bus, ConnectionRef, Incoming, InstanceCommand, InstanceOpened, InstanceReply, InstanceRequest,
    subject,
};
use tokio::task::{JoinError, JoinSet};
use uuid::Uuid;

use crate::world::SimWorld;

/// The director's answer to one player's request; `Err` with the reason when there is none.
pub type InstanceAnswer = (ConnectionRef, Result<InstanceReply, String>);

/// The requests that wait for the director.
#[derive(Debug, Default)]
pub struct InstanceLink {
    answers: JoinSet<InstanceAnswer>,
}

impl InstanceLink {
    /// Sends `request` of `connection`'s player to the director of world `world_id`; the answer
    /// comes from [`InstanceLink::next`] within `timeout`.
    pub fn ask(
        &mut self,
        bus: &Bus,
        world_id: Uuid,
        timeout: Duration,
        connection: ConnectionRef,
        request: InstanceRequest,
    ) {
        let bus = bus.clone();
        self.answers.spawn(async move {
            let reply = bus
                .request::<InstanceRequest, InstanceReply>(
                    subject::director_instance(world_id),
                    &request,
                    timeout,
                )
                .await
                .map_err(|error| error.to_string());
            (connection, reply)
        });
    }

    /// The next answer; `None` while no request waits.
    pub async fn next(&mut self) -> Option<InstanceAnswer> {
        let joined: Result<InstanceAnswer, JoinError> = self.answers.join_next().await?;
        match joined {
            Ok(answer) => Some(answer),
            Err(error) => {
                tracing::warn!(%error, "instance request task failed");
                None
            }
        }
    }
}

/// Carries out the director's `command` on `world` at sim time `now_ms`; an opening is answered
/// on `bus`.
pub fn command(world: &mut SimWorld, bus: &Bus, command: Incoming<InstanceCommand>, now_ms: u64) {
    match command.message.clone() {
        InstanceCommand::Open {
            instance,
            game_mode,
            phase,
            members,
            ends_at_ms,
        } => {
            let opened =
                world.open_instance(instance, &game_mode, phase, members, ends_at_ms, now_ms);
            if let InstanceOpened::Refused { reason } = &opened {
                tracing::warn!(%instance, %reason, "instance not opened");
            }
            let bus = bus.clone();
            tokio::spawn(async move {
                if let Err(error) = bus.reply(&command, &opened).await {
                    tracing::warn!(%error, "instance opening not answered");
                }
            });
        }
        InstanceCommand::Close { instance } => world.close_instance(instance),
        InstanceCommand::Admit {
            instance,
            character,
        } => world.add_instance_member(instance, character),
    }
}
