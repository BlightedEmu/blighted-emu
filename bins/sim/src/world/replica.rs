// SPDX-License-Identifier: AGPL-3.0-or-later
//! What other players see of a player (replication note, 2026-09-18): its look, the items that
//! show on it, its health, its block, where it faces and its movement mode, on its own ALC clock.
//! Retail sends a remote player in every tick of a small fight: every state with its number and
//! time, the place, the facing, the look direction and the slayer state, and the changes of the
//! vitals (15), the paperdoll (3183) and the damage receiver (1528). Before, our remote players
//! were a position alone, with an empty look, gear and health: others saw no facing, weapon,
//! block, health or death.
//!
//! A player is built once per tick into a [`Replica`] that every viewer shares; what changed is
//! found against [`ReplicaMemory`], the last state the viewers were sent. A player near a border
//! goes to the other sim as a [`PlayerGhost`], which carries the same [`ReplicaSource`].
//!
//! Each viewer gets the remote players by priority ([`RemotePlan`]): one in combat or near every
//! tick, farther ones less often, at most [`REMOTE_GROUPS_PER_TICK`] per tick, a card refresh and
//! the longest waiting first; a new player's card and a change always go.

use std::collections::HashMap;

use nw_bus::{GHOST_VISIBLE_ITEMS, GhostItem, PlayerGhost};
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    AlcStamp, AlcState, BundleGroup, BundleMember, DamageReceiverState, EquippedItem, GearItem,
    Hands, HealthChange, InputPosition, LookDir, MemberState, MessageError, PlayerGear, PlayerLook,
    PlayerStates, RemoteStep, SlayerState, VitalsData, VitalsState,
};
use nw_sim_core::Position;

use super::members::{EntityKind, OwnClock, PLAYER_ALC_COMPONENT, entity_group};

/// The own player component of the vitals.
const VITALS_COMPONENT: u64 = nw_messages::PLAYER_VITALS_COMPONENT;
/// The player component of the damage receiver (`isBlockActive`; player component table).
const DAMAGE_RECEIVER_COMPONENT: u64 = 14;
/// The player component of the paperdoll (player component table).
const PAPERDOLL_COMPONENT: u64 = 62;
/// The slayer state and sequence of the run toggle's mode, on and off (Observed: the retail own
/// state in the run-toggle and crouch captures, `13=1a 15=aa2b` while the mode was on and
/// `13=1b 15=b625` after it went off; the sequence of the off state varies, its meaning is
/// Unknown).
const RUN_MODE_STATE: u64 = 0x1a;
const RUN_MODE_SEQUENCE: u64 = 2794;
const WALK_MODE_STATE: u64 = 0x1b;
const WALK_MODE_SEQUENCE: u64 = 2422;
/// A remote player this near the viewer goes every tick (our choice: a melee and short-range
/// fight; retail sent the duel's remote player every 17 ms tick).
const NEAR_M: f32 = 30.0;
/// One this near goes every second tick; farther ones every [`FAR_PERIOD_TICKS`] (our choice:
/// retail's 45-vs-45 war sent remote players at a median 33 ms and a p90 135 ms).
const MID_M: f32 = 60.0;
const MID_PERIOD_TICKS: u64 = 2;
const FAR_PERIOD_TICKS: u64 = 4;
/// Most remote players one viewer gets in one tick besides cards and changes (our choice: about
/// 1 KB of 40-byte remote states, retail's p99 datagram of 1,028 bytes).
pub(super) const REMOTE_GROUPS_PER_TICK: usize = 24;
/// The ghost item of a worn item: one of a full stack, as other players see it (the durability,
/// bind flag and instance do not show; Inferred).
const SHOWN_QUANTITY: u32 = 1;

/// What a player is now, for the players who see it: from its own sim, or from a ghost.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ReplicaSource {
    pub(super) character: [u8; RAW_ID_LEN],
    pub(super) look: Option<PlayerLook>,
    pub(super) health: u32,
    pub(super) max_health: u32,
    pub(super) view: Option<LookDir>,
    pub(super) run_mode: Option<bool>,
    pub(super) mode_started: u16,
    pub(super) blocking: bool,
    pub(super) spawned_ms: u64,
    pub(super) gear_version: u64,
    pub(super) items: Vec<EquippedItem>,
    pub(super) hands: Hands,
    pub(super) in_combat: bool,
}

impl ReplicaSource {
    /// The ghost of this player for a sim next door.
    pub(super) fn ghost(&self) -> PlayerGhost {
        let mut items = [None; GHOST_VISIBLE_ITEMS];
        for (shown, worn) in items.iter_mut().zip(&self.items) {
            *shown = u8::try_from(worn.slot).ok().map(|slot| GhostItem {
                slot,
                item_index: worn.item.item_index,
                gear_score: worn.item.gear_score,
            });
        }
        PlayerGhost {
            character: uuid::Uuid::from_bytes(self.character),
            look: self.look.map(|look| look.choices()),
            health: self.health,
            max_health: self.max_health,
            facing: self.view.map(|view| view.yaw),
            pitch: self.view.map(|view| view.pitch),
            run_mode: self.run_mode,
            mode_started: self.mode_started,
            blocking: self.blocking,
            spawned_ms: self.spawned_ms,
            gear_version: self.gear_version,
            drawn: self.hands.drawn,
            attached: self.hands.attached,
            items,
        }
    }

    /// The player of a ghost from a sim next door.
    pub(super) fn of_ghost(ghost: &PlayerGhost) -> Self {
        Self {
            character: *ghost.character.as_bytes(),
            look: ghost.look.map(PlayerLook::of_choices),
            health: ghost.health,
            max_health: ghost.max_health,
            view: ghost.facing.map(|yaw| LookDir {
                yaw,
                pitch: ghost.pitch.unwrap_or_default(),
            }),
            run_mode: ghost.run_mode,
            mode_started: ghost.mode_started,
            blocking: ghost.blocking,
            spawned_ms: ghost.spawned_ms,
            gear_version: ghost.gear_version,
            items: ghost
                .items
                .iter()
                .flatten()
                .map(|item| EquippedItem {
                    slot: u32::from(item.slot),
                    item: GearItem {
                        item_index: item.item_index,
                        quantity: SHOWN_QUANTITY,
                        gear_score: item.gear_score,
                        durability: 0,
                        bound: false,
                        instance: [0; RAW_ID_LEN],
                    },
                })
                .collect(),
            hands: Hands {
                drawn: ghost.drawn,
                attached: ghost.attached,
                ammo: [None; 2],
            },
            // A ghost's fight is its own sim's: near, it goes every tick anyway.
            in_combat: false,
        }
    }

    /// Its slayer state: its movement mode, when its input sent one.
    fn slayer(&self) -> Option<SlayerState> {
        self.run_mode.map(|run| SlayerState {
            state: if run { RUN_MODE_STATE } else { WALK_MODE_STATE },
            started: self.mode_started,
            sequence: if run {
                RUN_MODE_SEQUENCE
            } else {
                WALK_MODE_SEQUENCE
            },
        })
    }

    /// Its gear as others see it: the paperdoll of the items that show; its bag and other
    /// containers stay its own (empty states).
    fn gear(&self) -> PlayerGear {
        PlayerGear {
            paperdoll: PlayerGear::of(&self.items, &[], self.gear_version, self.hands).paperdoll,
            ..PlayerGear::default()
        }
    }
}

/// What the viewers were last sent of one player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ReplicaMemory {
    health: u32,
    blocking: bool,
    gear_version: u64,
    /// The server time of its last new life (ms since 2000-01-01).
    life_ms: u64,
}

/// One player as every viewer gets it this tick.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Replica {
    source: ReplicaSource,
    gear: PlayerGear,
    stamp: AlcStamp,
    health_change: Option<HealthChange>,
    gear_changed: bool,
    block_changed: bool,
}

impl Replica {
    /// The player of `source` at server time `server_ms`, with what changed since `seen` (none
    /// for a player first seen), and what the viewers saw then.
    pub(super) fn new(
        source: ReplicaSource,
        seen: Option<ReplicaMemory>,
        server_ms: u64,
    ) -> (Self, ReplicaMemory) {
        let clock = OwnClock {
            since_spawn_ms: server_ms.saturating_sub(source.spawned_ms),
            spawn_ms: source.spawned_ms,
        };
        let health = source.health;
        let (health_change, gear_changed, block_changed, life_ms) = match seen {
            None => (None, false, false, source.spawned_ms),
            Some(seen) => {
                let life = match (seen.health, health) {
                    (shown, 0) if shown > 0 => Some(VitalsData::player_dead(seen.life_ms)),
                    (0, now) if now > 0 => Some(VitalsData::player_alive(server_ms)),
                    _ => None,
                };
                let life_ms = if health > 0 && seen.health == 0 {
                    server_ms
                } else {
                    seen.life_ms
                };
                let change = (seen.health != health).then_some(HealthChange {
                    health,
                    first_damage: seen.health == source.max_health,
                    life,
                });
                (
                    change,
                    seen.gear_version != source.gear_version,
                    seen.blocking != source.blocking,
                    life_ms,
                )
            }
        };
        let memory = ReplicaMemory {
            health,
            blocking: source.blocking,
            gear_version: source.gear_version,
            life_ms,
        };
        let replica = Self {
            gear: source.gear(),
            stamp: clock.stamp(),
            source,
            health_change,
            gear_changed,
            block_changed,
        };
        (replica, memory)
    }

    /// Whether it changed this tick: it goes to every viewer now, whatever its priority.
    pub(super) const fn changed(&self) -> bool {
        self.health_change.is_some() || self.gear_changed || self.block_changed
    }

    /// Whether it is in combat: it goes first.
    pub(super) const fn in_combat(&self) -> bool {
        self.source.in_combat
    }

    /// Its group in slot `interest` for a viewer, at `position`: with its card when `carded`
    /// (its look, the items that show and its health, then its full first state on its clock),
    /// else its state of this tick and its changes. `None` for a position that is not finite.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a state cannot be encoded.
    pub(super) fn group(
        &self,
        interest: u16,
        entity: u64,
        position: Position,
        carded: bool,
    ) -> Result<Option<BundleGroup>, MessageError> {
        let Some(pos) = InputPosition::from_metres(position.x, position.y, position.z) else {
            return Ok(None);
        };
        let source = &self.source;
        if carded {
            let states = PlayerStates {
                character_id: source.character,
                look: source.look,
                health: Some((source.health, source.max_health)),
                gear: Some(&self.gear),
                achievements: None,
                objectives: None,
                progression: None,
                currency: None,
                health_change: None,
                cooldowns: None,
                stamina: None,
                combat_status: None,
            };
            let Some(mut group) = entity_group(
                interest,
                entity,
                EntityKind::PLAYER,
                position,
                true,
                Some(states),
            ) else {
                return Ok(None);
            };
            if let Some(alc) = group.members.last_mut() {
                alc.state = MemberState::Alc(AlcState::spawn(pos, self.stamp.anchor_us)?);
            }
            return Ok(Some(group));
        }
        let mut members = vec![BundleMember {
            component: PLAYER_ALC_COMPONENT,
            state: MemberState::Alc(AlcState::remote_step(&RemoteStep {
                stamp: self.stamp,
                position: Some(pos),
                facing: source.view.map(|view| view.yaw),
                look: source.view,
                slayer: source.slayer(),
            })?),
        }];
        if let Some(change) = self.health_change {
            let vitals = match change.life {
                Some(life) => VitalsState::player_life(change.health, life),
                None if change.first_damage => VitalsState::creature_first_damage(change.health),
                None => VitalsState::creature_health(change.health),
            };
            members.push(BundleMember {
                component: VITALS_COMPONENT,
                state: MemberState::Vitals(vitals),
            });
        }
        if self.gear_changed {
            members.push(BundleMember {
                component: PAPERDOLL_COMPONENT,
                state: MemberState::Paperdoll(Box::new(self.gear.paperdoll.clone())),
            });
        }
        if self.block_changed {
            members.push(BundleMember {
                component: DAMAGE_RECEIVER_COMPONENT,
                state: MemberState::DamageReceiver(DamageReceiverState {
                    is_block_active: Some(source.blocking),
                    ..DamageReceiverState::default()
                }),
            });
        }
        Ok(Some(BundleGroup {
            interest: u64::from(interest),
            members,
        }))
    }
}

/// How long a viewer may wait for a remote player `distance_m` away (in ticks): every tick near
/// or in combat, then less often with distance.
const fn period_ticks(distance_m: f32, in_combat: bool) -> u64 {
    if in_combat || distance_m <= NEAR_M {
        1
    } else if distance_m <= MID_M {
        MID_PERIOD_TICKS
    } else {
        FAR_PERIOD_TICKS
    }
}

/// One remote player a viewer may get this tick.
#[derive(Debug, Clone, Copy)]
pub(super) struct RemoteCandidate<'a> {
    pub(super) number: u64,
    pub(super) distance_m: f32,
    pub(super) replica: &'a Replica,
    /// The viewer has no slot for it yet: its card goes now.
    pub(super) new: bool,
    /// Its card refresh is due.
    pub(super) refresh: bool,
}

/// Which remote players one viewer gets this tick.
#[derive(Debug, Default)]
pub(super) struct RemotePlan {
    /// The bundle step each remote player was last sent in, by entity number.
    last_sent: HashMap<u64, u64>,
}

impl RemotePlan {
    /// The remote players of `candidates` that the viewer gets in its bundle step `step`: every
    /// new one and every change, then the ones whose wait came, card refreshes first, then those
    /// in combat, then the longest waiting, then the nearest, at most [`REMOTE_GROUPS_PER_TICK`].
    /// Players no longer seen are forgotten.
    pub(super) fn pick(&mut self, step: u64, candidates: &[RemoteCandidate<'_>]) -> Vec<u64> {
        self.last_sent.retain(|number, _| {
            candidates
                .iter()
                .any(|candidate| candidate.number == *number)
        });
        let mut picked: Vec<u64> = Vec::new();
        let mut waiting: Vec<(bool, bool, u64, f32, u64)> = Vec::new();
        for candidate in candidates {
            let replica = candidate.replica;
            if candidate.new || replica.changed() {
                picked.push(candidate.number);
                continue;
            }
            let age = self
                .last_sent
                .get(&candidate.number)
                .map_or(u64::MAX, |sent| step.saturating_sub(*sent));
            if candidate.refresh || age >= period_ticks(candidate.distance_m, replica.in_combat()) {
                waiting.push((
                    !candidate.refresh,
                    !replica.in_combat(),
                    u64::MAX - age,
                    candidate.distance_m,
                    candidate.number,
                ));
            }
        }
        waiting.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(a.1.cmp(&b.1))
                .then(a.2.cmp(&b.2))
                .then(a.3.total_cmp(&b.3))
                .then(a.4.cmp(&b.4))
        });
        picked.extend(
            waiting
                .into_iter()
                .take(REMOTE_GROUPS_PER_TICK)
                .map(|(.., number)| number),
        );
        for number in &picked {
            self.last_sent.insert(*number, step);
        }
        picked
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    const SPAWNED_MS: u64 = 1_000_000;
    const MAX_HEALTH: u32 = 856;

    pub(in crate::world) fn source() -> ReplicaSource {
        ReplicaSource {
            character: [7; RAW_ID_LEN],
            look: None,
            health: MAX_HEALTH,
            max_health: MAX_HEALTH,
            view: Some(LookDir {
                yaw: 1.0,
                pitch: 0.25,
            }),
            run_mode: Some(true),
            mode_started: 2,
            blocking: false,
            spawned_ms: SPAWNED_MS,
            gear_version: 3,
            items: vec![EquippedItem {
                slot: nw_messages::paperdoll::slot::MAIN_HAND_OPTION_1,
                item: GearItem {
                    item_index: 42,
                    quantity: SHOWN_QUANTITY,
                    gear_score: 75,
                    durability: 0,
                    bound: false,
                    instance: [0; RAW_ID_LEN],
                },
            }],
            hands: Hands::default(),
            in_combat: false,
        }
    }

    #[test]
    fn a_ghost_carries_the_whole_replica() {
        let source = source();
        let back = ReplicaSource::of_ghost(&source.ghost());
        assert_eq!(back, source);
    }

    #[test]
    fn changes_are_found_against_what_viewers_saw() {
        let (first, seen) = Replica::new(source(), None, SPAWNED_MS + 10);
        assert!(!first.changed(), "a first sight is its card");
        let hit = ReplicaSource {
            health: 500,
            blocking: true,
            gear_version: 4,
            ..source()
        };
        let (changed, seen) = Replica::new(hit.clone(), Some(seen), SPAWNED_MS + 20);
        assert_eq!(
            changed
                .health_change
                .map(|change| (change.health, change.first_damage)),
            Some((500, true)),
            "the first damage from full health"
        );
        assert!(changed.gear_changed && changed.block_changed);
        let (dead, seen) = Replica::new(
            ReplicaSource { health: 0, ..hit },
            Some(seen),
            SPAWNED_MS + 30,
        );
        assert!(
            dead.health_change
                .and_then(|change| change.life)
                .is_some_and(|life| life.is_dead())
        );
        let (again, _) = Replica::new(source(), Some(seen), SPAWNED_MS + 40);
        assert!(
            again
                .health_change
                .and_then(|change| change.life)
                .is_some_and(|life| !life.is_dead()),
            "a new life"
        );
    }

    #[test]
    fn near_and_fighting_players_go_first_and_far_ones_wait() {
        let (calm, _) = Replica::new(source(), None, SPAWNED_MS);
        let (fighting, _) = Replica::new(
            ReplicaSource {
                in_combat: true,
                ..source()
            },
            None,
            SPAWNED_MS,
        );
        let candidate = |number, distance_m, replica| RemoteCandidate {
            number,
            distance_m,
            replica,
            new: false,
            refresh: false,
        };
        let mut plan = RemotePlan::default();
        let far = MID_M + 1.0;
        let candidates = [candidate(1, far, &calm), candidate(2, far, &fighting)];
        assert_eq!(plan.pick(0, &candidates), [2, 1], "a fight first");
        assert_eq!(plan.pick(1, &candidates), [2], "the far one waits");
        assert_eq!(plan.pick(FAR_PERIOD_TICKS, &candidates), [2, 1]);
        let many: Vec<RemoteCandidate<'_>> = (0..100_u64)
            .map(|number| candidate(number, 1.0, &calm))
            .collect();
        let mut plan = RemotePlan::default();
        let first = plan.pick(0, &many);
        assert_eq!(first.len(), REMOTE_GROUPS_PER_TICK, "the budget holds");
        let second = plan.pick(1, &many);
        assert!(
            second.iter().all(|number| !first.contains(number)),
            "the ones that waited go next"
        );
    }
}
