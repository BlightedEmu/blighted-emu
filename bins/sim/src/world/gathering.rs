// SPDX-License-Identifier: AGPL-3.0-or-later
//! The open world's gather nodes (gather note, 2026-09-18): flint, bushes and the corpses to
//! skin. The client has the nodes in its level data and names one in its gather request, so a
//! node is known here from its first gather. Retail cards a node only once it left its default
//! state: after a gather it sends the node's card with its state Depleted and its interaction off,
//! and when the node grows back (60 to 90 s later for flint) its state Healthy and its interaction
//! on again.

use std::collections::HashMap;

use bevy_ecs::entity::Entity;
use nw_content::gathering::Gatherable;
use nw_content::loot::LootContext;
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    BundleGroup, BundleMember, GatherableControllerState, GdeMetadataReplicatedState,
    InteractState, InteractionEndedMsg, InteractionRequest, MemberState, ServerMessage,
    entity_of_raw_id,
};
use nw_sim_core::Position;

use super::slots::Slot;
use super::statics::{GatherRun, chest_gather_ended, gather_started, gather_stopped};
use super::{Body, Player, SimWorld, gather_answer, send_now};
use crate::mobs::spawn_index;
use crate::objectives::Event;

/// First slot key of a gather node: its index is added. Keys are never sent; they stay above
/// every player and mob number and below the tutorial's static entities.
pub(super) const GATHERABLE_ENTITY_BASE: u64 = 1 << 39;
/// How near a player a node that left its default state is shown (our choice: retail cards every
/// nearby such node on a new connection; its radius is Unknown).
const NODE_VIEW_M: f32 = 100.0;
/// The components of a node's group: its card, its gatherable controller (class 12) and its
/// interact state (class 2930).
const CARD_COMPONENT: u64 = 0;
const CONTROLLER_COMPONENT: u64 = 3;
const INTERACT_COMPONENT: u64 = 4;
/// `gatherableState` values (client enum): Healthy 4, Depleted 8.
const HEALTHY: u8 = 4;
const DEPLETED: u8 = 8;

/// The status of a refused gather request (gather note: retail 2 for a node another player
/// gathers; Inferred the same for a depleted node).
const GATHER_BUSY: u8 = 2;
/// The status of an accepted gather request.
const GATHER_ACCEPTED: u8 = 1;
/// The name an open-world gather's accept carries (retail: empty).
const NO_NAME: &[u8] = &[];
/// The server tick a hand gather ends on (retail flint 0.25 s ended 300 to 307 ms after the
/// accept: the gather time rounded up to the tick, plus one).
const GATHER_TICK_MS: u64 = 33;
/// The efficiency of a first skinning (retail boar 15 / 2.0: 7.6 s) and of a hand gather.
const SKINNING_EFFICIENCY: f32 = 2.0;
const HAND_EFFICIENCY: f32 = 1.0;
/// The gatherers of an open-world node (retail 1).
const ONE_GATHERER: u8 = 1;
/// The level loot tables read for the player (`MinContLevel`; our choice: the server keeps no
/// player level yet) and for a mob without one.
pub(super) const PLAYER_LOOT_LEVEL: u32 = 1;
pub(super) const MOB_LEVEL: u32 = 1;
/// Milliseconds per second.
const MS_PER_SECOND: f32 = 1_000.0;
/// The paperdoll slot of each tradeskill's tool (beach-quests note: retail equips of the
/// skinning knife, the pick and the hatchet). A gatherable that needs its tool is gathered only
/// with an item in that slot.
const TOOL_SLOTS: [(&str, u32); 3] = [("Skinning", 35), ("Mining", 36), ("Logging", 33)];
/// The roll of a quest drop's chance, in percent.
const PERCENT: u32 = 100;

/// A gather that runs.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct OpenGather {
    number: u64,
    request: InteractionRequest,
    gatherable: Gatherable,
    /// The mob skinned, for a corpse.
    mob: Option<u64>,
    end_ms: u64,
}

impl SimWorld {
    /// Player `number` (`entity`) asks at `now_ms` to gather `request` in the open world: a node
    /// of the level (flint, a bush) or a dead mob to skin, named by its gatherable. A known
    /// gatherable that nobody else gathers is accepted (3743, the running gather in the own
    /// states) and ends after its gather time ([`SimWorld::gathers_due`]); a busy one is refused
    /// (3743 status 2). The client repeats the request in each input until the end: a repeat
    /// starts nothing.
    pub(super) fn gather_open_world(
        &mut self,
        entity: Entity,
        number: u64,
        request: InteractionRequest,
        now_ms: u64,
    ) {
        let Some(gatherable) = request
            .gatherable
            .and_then(|crc| self.content.gatherables.get(crc))
            .cloned()
        else {
            tracing::debug!(player = number, ?request, "gather of an unknown gatherable");
            return;
        };
        if self.open_gathers.iter().any(|gather| {
            gather.number == number && gather.request.interaction == request.interaction
        }) {
            return;
        }
        let Some(position) = self
            .world
            .get::<Body>(entity)
            .map(|body| body.motion.position())
        else {
            return;
        };
        let mob = entity_of_raw_id(&request.target).filter(|mob| spawn_index(*mob).is_some());
        if !self.holds_tool(entity, &gatherable) {
            tracing::debug!(player = number, gatherable = %gatherable.id, "gather without its tool");
            if let Some(player) = self.world.get::<Player>(entity) {
                let refusal = gather_answer(number, request, GATHER_BUSY, NO_NAME);
                send_now(&mut self.combat_messages, &player.connection, &[refusal]);
            }
            return;
        }
        let started = match mob {
            Some(mob) => self.start_skinning(mob, number),
            None => self
                .nodes
                .start(request.target, position, number)
                .map_err(|busy| tracing::debug!(player = number, ?busy, "gather refused")),
        };
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        if started.is_err() {
            let refusal = gather_answer(number, request, GATHER_BUSY, NO_NAME);
            send_now(&mut self.combat_messages, &player.connection, &[refusal]);
            return;
        }
        let (duration_ms, run) = if gatherable.skinning() {
            #[allow(clippy::cast_precision_loss, reason = "a gather time of seconds")]
            let skin_ms = (gatherable.gather_ms as f32 / SKINNING_EFFICIENCY) as u64;
            (
                skin_ms,
                GatherRun {
                    #[allow(clippy::cast_precision_loss, reason = "a gather time of seconds")]
                    amount: gatherable.gather_ms as f32 / MS_PER_SECOND,
                    efficiency: SKINNING_EFFICIENCY,
                    gatherers: ONE_GATHERER,
                },
            )
        } else {
            (
                gatherable.gather_ms.div_ceil(GATHER_TICK_MS) * GATHER_TICK_MS + GATHER_TICK_MS,
                GatherRun {
                    #[allow(clippy::cast_precision_loss, reason = "a gather time of seconds")]
                    amount: gatherable.gather_ms as f32 / MS_PER_SECOND,
                    efficiency: HAND_EFFICIENCY,
                    gatherers: ONE_GATHERER,
                },
            )
        };
        let accept = gather_answer(number, request, GATHER_ACCEPTED, NO_NAME);
        send_now(&mut self.combat_messages, &player.connection, &[accept]);
        let started = gather_started(request.target_entity, player.gathers, run);
        player.extra_members.extend(started);
        tracing::debug!(player = number, gatherable = %gatherable.id, duration_ms, "gather");
        self.open_gathers.push(OpenGather {
            number,
            request,
            gatherable,
            mob,
            end_ms: now_ms.saturating_add(duration_ms),
        });
    }

    /// Whether player `entity` may gather `gatherable`: one that needs its tradeskill's tool
    /// (`Restriction` ToolRequired) only with an item in the tool's slot (Inferred: retail
    /// answered the skinning knife's absence with the client's "No equipped Knife" prompt, live
    /// 2026-09-18). A tradeskill without a known slot needs no tool.
    fn holds_tool(&self, entity: Entity, gatherable: &Gatherable) -> bool {
        let slot = TOOL_SLOTS
            .iter()
            .find(|(tradeskill, _)| gatherable.tradeskill.as_deref() == Some(*tradeskill))
            .map(|(_, slot)| *slot);
        match slot {
            Some(slot) if gatherable.tool_required => self
                .world
                .get::<Player>(entity)
                .and_then(|player| player.gear.as_ref())
                .is_some_and(|gear| gear.worn_item(slot).is_some()),
            _ => true,
        }
    }

    /// Player `number` starts to skin `mob`: only a dead mob that nobody skinned or skins.
    fn start_skinning(&self, mob: u64, number: u64) -> Result<(), ()> {
        let dead = self.mobs.health(mob) == Some(0);
        let taken = self.skinned.contains(&mob)
            || self
                .open_gathers
                .iter()
                .any(|gather| gather.mob == Some(mob) && gather.number != number);
        if dead && !taken {
            Ok(())
        } else {
            tracing::debug!(player = number, mob, dead, taken, "skinning refused");
            Err(())
        }
    }

    /// The gathers whose time came at `now_ms` end: the interaction ends (3746), the own gather
    /// state is done, the loot of the gatherable goes into the bag and to the open-world quest,
    /// and the node is depleted until its respawn (a corpse is skinned). The depleted nodes whose
    /// time came grow back.
    pub(super) fn gathers_due(&mut self, now_ms: u64) {
        self.nodes.tick(now_ms);
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.open_gathers)
            .into_iter()
            .partition(|gather| now_ms >= gather.end_ms);
        self.open_gathers = later;
        for gather in due {
            self.end_gather(&gather, now_ms);
        }
    }

    fn end_gather(&mut self, gather: &OpenGather, now_ms: u64) {
        let levels = LootContext {
            enemy: gather
                .mob
                .and_then(|mob| self.mobs.level(mob))
                .unwrap_or(MOB_LEVEL),
            content: PLAYER_LOOT_LEVEL,
            tags: &[],
        };
        let dice = &mut self.dice;
        let mut loot = gather
            .gatherable
            .loot_table
            .as_deref()
            .map(|table| {
                self.content
                    .loot
                    .roll(table, levels, &mut |below| dice.below(below))
            })
            .unwrap_or_default();
        if let Some(mob) = gather.mob {
            loot.extend(self.quest_drops(gather.number, mob));
        }
        match gather.mob {
            Some(mob) => {
                self.skinned.insert(mob);
            }
            None => {
                let (shortest, longest) = gather.gatherable.respawn_ms;
                let spread = u32::try_from(longest.saturating_sub(shortest)).unwrap_or(u32::MAX);
                let respawn_at_ms = now_ms
                    .saturating_add(shortest)
                    .saturating_add(u64::from(self.dice.below(spread.saturating_add(1))));
                self.nodes.deplete(&gather.request.target, respawn_at_ms);
            }
        }
        let Some(entity) = self.player_by_number(gather.number) else {
            return;
        };
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let end = ServerMessage::InteractionEnded(InteractionEndedMsg {
            header: nw_messages::FacetHeader::of(
                nw_messages::facet::component::INTERACTOR,
                gather.number,
            ),
        });
        send_now(&mut self.combat_messages, &player.connection, &[end]);
        let ended = chest_gather_ended(gather.request.target_entity, player.gathers);
        player.extra_members.push(ended);
        player.gathers = player.gathers.saturating_add(1);
        self.give_loot(gather.number, loot, now_ms);
    }

    /// The items the quest of player `number` drops for the corpse of `mob` it skinned: one of
    /// each waiting gather task's item whose enemy the mob is, at the task's chance.
    fn quest_drops(&mut self, number: u64, mob: u64) -> Vec<(String, u32)> {
        let Some(quest) = self.quests.get(&number) else {
            return Vec::new();
        };
        let Some((vitals_id, categories)) = self
            .mobs
            .vitals_of(mob)
            .and_then(|vitals| self.content.kill_types.of(vitals))
        else {
            return Vec::new();
        };
        let drops = quest.run.quest_drops(vitals_id, categories);
        drops
            .into_iter()
            .filter(|(_, percent)| self.dice.below(PERCENT) < *percent)
            .map(|(item, _)| (item, 1))
            .collect()
    }

    /// Puts rolled `loot` into the bag of player `number`; each item that fits is an event of
    /// its quest. An item with no master row, or that does not fit, is lost with a log line.
    pub(super) fn give_loot(&mut self, number: u64, loot: Vec<(String, u32)>, now_ms: u64) {
        let Some(entity) = self.player_by_number(number) else {
            return;
        };
        let mut gained = Vec::new();
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            for (item_id, quantity) in loot {
                let Some(item) = self.content.kits.item(&item_id) else {
                    tracing::debug!(item = %item_id, "loot item without a master row");
                    continue;
                };
                let added = player
                    .gear
                    .as_mut()
                    .is_some_and(|gear| gear.add(item, quantity));
                if added {
                    tracing::debug!(player = number, item = %item_id, quantity, "looted");
                    gained.push((item_id, quantity));
                } else {
                    tracing::info!(player = number, item = %item_id, quantity, "loot lost: no bag or a full bag");
                }
            }
        }
        for (item, quantity) in &gained {
            self.quest_event(
                number,
                Event::ItemGained {
                    item,
                    quantity: *quantity,
                },
                now_ms,
            );
        }
    }

    /// The client of player `entity` ends interaction `interaction`: a gather that still runs
    /// stops without a harvest; the client's acknowledgement of a gather that ended changes
    /// nothing.
    pub(super) fn cancel_gather(&mut self, entity: Entity, interaction: u64) {
        let Some(number) = self.world.get::<Player>(entity).map(|player| player.number) else {
            return;
        };
        let Some(at) = self.open_gathers.iter().position(|gather| {
            gather.number == number && gather.request.interaction == interaction
        }) else {
            return;
        };
        let gather = self.open_gathers.remove(at);
        if gather.mob.is_none() {
            self.nodes.release(&gather.request.target);
        }
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            let stopped = gather_stopped(player.gathers);
            player.extra_members.push(stopped);
            player.gathers = player.gathers.saturating_add(1);
        }
    }
}

/// One gather node that left its default state.
#[derive(Debug, Clone, PartialEq)]
struct Node {
    gde_ref: [u8; RAW_ID_LEN],
    /// Where its first gatherer stood (Inferred: a node is within reach of its gatherer).
    position: Position,
    /// The player who gathers it now.
    gatherer: Option<u64>,
    /// When it grows back, while it is depleted.
    respawn_at_ms: Option<u64>,
    /// Counts its state changes, so each player gets each one once.
    version: u32,
}

impl Node {
    const fn depleted(&self) -> bool {
        self.respawn_at_ms.is_some()
    }

    /// Its card, controller and interact members (`with_card`), or its last two alone.
    fn members(&self, with_card: bool) -> Vec<BundleMember> {
        let (state, enabled) = if self.depleted() {
            (DEPLETED, false)
        } else {
            (HEALTHY, true)
        };
        let card = with_card.then_some(BundleMember {
            component: CARD_COMPONENT,
            state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                asset: None,
                gde_ref: Some(self.gde_ref),
            }),
        });
        card.into_iter()
            .chain([
                BundleMember {
                    component: CONTROLLER_COMPONENT,
                    state: MemberState::GatherableController(GatherableControllerState {
                        gatherable_state: Some(state),
                    }),
                },
                BundleMember {
                    component: INTERACT_COMPONENT,
                    state: MemberState::Interact(InteractState {
                        enabled: Some(enabled),
                    }),
                },
            ])
            .collect()
    }
}

/// Why a gather request is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NodeBusy {
    /// Another player gathers it.
    Gathered,
    /// It is depleted.
    Depleted,
}

/// The gather nodes of one sim.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Nodes {
    nodes: Vec<Node>,
    by_ref: HashMap<[u8; RAW_ID_LEN], usize>,
}

impl Nodes {
    /// Player `player` at `position` starts to gather node `gde_ref`; refused while another
    /// player gathers it or it is depleted.
    pub(super) fn start(
        &mut self,
        gde_ref: [u8; RAW_ID_LEN],
        position: Position,
        player: u64,
    ) -> Result<(), NodeBusy> {
        let index = *self.by_ref.entry(gde_ref).or_insert_with(|| {
            self.nodes.push(Node {
                gde_ref,
                position,
                gatherer: None,
                respawn_at_ms: None,
                version: 0,
            });
            self.nodes.len() - 1
        });
        let node = &mut self.nodes[index];
        if node.depleted() {
            return Err(NodeBusy::Depleted);
        }
        if node.gatherer.is_some_and(|gatherer| gatherer != player) {
            return Err(NodeBusy::Gathered);
        }
        node.gatherer = Some(player);
        Ok(())
    }

    /// The gather of node `gde_ref` ends without a harvest (a cancel).
    pub(super) fn release(&mut self, gde_ref: &[u8; RAW_ID_LEN]) {
        if let Some(node) = self
            .by_ref
            .get(gde_ref)
            .map(|index| &mut self.nodes[*index])
        {
            node.gatherer = None;
        }
    }

    /// Node `gde_ref` was harvested: depleted until `respawn_at_ms`.
    pub(super) fn deplete(&mut self, gde_ref: &[u8; RAW_ID_LEN], respawn_at_ms: u64) {
        if let Some(node) = self
            .by_ref
            .get(gde_ref)
            .map(|index| &mut self.nodes[*index])
        {
            node.gatherer = None;
            node.respawn_at_ms = Some(respawn_at_ms);
            node.version += 1;
        }
    }

    /// The depleted nodes whose time came at `now_ms` grow back.
    pub(super) fn tick(&mut self, now_ms: u64) {
        for node in &mut self.nodes {
            if node.respawn_at_ms.is_some_and(|at| now_ms >= at) {
                node.respawn_at_ms = None;
                node.version += 1;
            }
        }
    }

    /// The slot keys of the nodes shown to a player at `position`: those that left their default
    /// state, within [`NODE_VIEW_M`].
    pub(super) fn numbers_near(&self, position: Position) -> impl Iterator<Item = u64> + '_ {
        (0_u64..)
            .zip(&self.nodes)
            .filter(move |(_, node)| {
                node.version > 0 && node.position.horizontal_distance(position) <= NODE_VIEW_M
            })
            .map(|(index, _)| GATHERABLE_ENTITY_BASE + index)
    }

    /// The group of the node of slot key `number` in `slot` for a player who got its version
    /// `sent`: its card and states when the slot is new, its states after a change, else none.
    /// Updates `sent`.
    pub(super) fn group(
        &self,
        number: u64,
        slot: Slot,
        sent: &mut HashMap<u64, u32>,
    ) -> Option<BundleGroup> {
        let index = usize::try_from(number.checked_sub(GATHERABLE_ENTITY_BASE)?).ok()?;
        let node = self.nodes.get(index)?;
        if !slot.new && sent.get(&number) == Some(&node.version) {
            return None;
        }
        sent.insert(number, node.version);
        Some(BundleGroup {
            interest: u64::from(slot.interest),
            members: node.members(slot.new),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLINT: [u8; RAW_ID_LEN] = [0x5b; RAW_ID_LEN];
    const AT: Position = Position {
        x: 7305.0,
        y: 3093.0,
        z: 20.0,
    };
    const PLAYER: u64 = 7;
    const RESPAWN_AT_MS: u64 = 61_500;

    fn state_of(group: &BundleGroup) -> Option<u8> {
        group.members.iter().find_map(|member| match &member.state {
            MemberState::GatherableController(state) => state.gatherable_state,
            _ => None,
        })
    }

    #[test]
    fn a_node_is_gathered_once_until_it_grows_back() {
        let mut nodes = Nodes::default();
        assert_eq!(nodes.start(FLINT, AT, PLAYER), Ok(()));
        assert_eq!(nodes.start(FLINT, AT, PLAYER + 1), Err(NodeBusy::Gathered));
        assert_eq!(
            nodes.numbers_near(AT).count(),
            0,
            "a node in its default state is not shown"
        );
        nodes.deplete(&FLINT, RESPAWN_AT_MS);
        assert_eq!(nodes.start(FLINT, AT, PLAYER), Err(NodeBusy::Depleted));
        let number = GATHERABLE_ENTITY_BASE;
        assert_eq!(nodes.numbers_near(AT).collect::<Vec<_>>(), [number]);
        let mut sent = HashMap::new();
        let new = Slot {
            interest: 9,
            new: true,
        };
        let card = nodes.group(number, new, &mut sent).expect("card");
        assert_eq!(card.members.len(), 3, "card, controller, interact");
        assert_eq!(state_of(&card), Some(DEPLETED));
        let kept = Slot { new: false, ..new };
        assert_eq!(nodes.group(number, kept, &mut sent), None, "sent once");
        nodes.tick(RESPAWN_AT_MS - 1);
        assert_eq!(nodes.start(FLINT, AT, PLAYER), Err(NodeBusy::Depleted));
        nodes.tick(RESPAWN_AT_MS);
        let grown = nodes.group(number, kept, &mut sent).expect("change");
        assert_eq!(grown.members.len(), 2, "the states without the card");
        assert_eq!(state_of(&grown), Some(HEALTHY));
        assert_eq!(nodes.start(FLINT, AT, PLAYER + 1), Ok(()));
        nodes.release(&FLINT);
        assert_eq!(nodes.start(FLINT, AT, PLAYER), Ok(()), "a cancel frees it");
        let far = Position {
            x: AT.x + NODE_VIEW_M + 1.0,
            ..AT
        };
        assert_eq!(nodes.numbers_near(far).count(), 0);
    }
}
