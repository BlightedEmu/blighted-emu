// SPDX-License-Identifier: AGPL-3.0-or-later
//! The projectile entity of a player's shot ([`nw_content::shot_entities`]; projectile study
//! 2026-11-11). When a player fires a projectile, every client that sees the place gets a new
//! entity: a card of the projectile's slice and its projectile state (class 16) with the spawn
//! point, the velocity, the shooter, the ranged attack and the damage row. The shooter's client
//! gets it too (Observed: 8,922 own arrows held the same members as the remote ones).
//!
//! About [`TIME_ANCHOR_MS`] after the card the state gets its time anchor. When the shot gets
//! to its target the state gets the impact point, the target, its hit volume and the collided
//! flag. The entity leaves the views the slice's `m_stuckLifetime` plus [`AFTER_END_MS`] after
//! the hit, or `m_flyingLifetime` plus [`AFTER_END_MS`] after the card when it hits nothing.
//!
//! A shot that a solid of the level stops ([`super::projectile`] **Level stops**) gets the impact
//! point, the solid's level entity (none for a solid of no entity) and the collided flag, with no
//! hit volume (Observed: `opr_que_and_lost`, 940 states on the OPR Vertical outer perimeter wall).
//!
//! Unknown: the clock of the time anchor (the server time of the shot is sent); the weapon
//! effect of a weapon with a gem (the default effect is sent).

use bevy_ecs::entity::Entity;
use nw_content::shot_entities::ShotEntity;
use nw_content::town_props::ASSET_ID_LEN;
use nw_messages::facet::facet_entity;
use nw_messages::projectile_state::{
    COLLIDED_FLAGS, DEFAULT_WEAPON_EFFECT, PLAYER_DAMAGE_TABLE, ProjectileState,
};
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    AssetId, BundleMember, DYNAMIC_SLICE_SUB_ID, GdeMetadataReplicatedState, MemberState,
    PositionInTheWorldReplicatedState, WorldPosition,
};
use nw_sim_core::Position;

use super::crafting::{CARD_COMPONENT, Station, dynamic_ref};
use super::projectile::{AimLine, muzzle_of};
use super::{Player, SimWorld};

/// The namespace of the `GdeRef`s of shot entities ("shotenty").
const SHOT_ENTITY_NAMESPACE: u64 = 0x7368_6f74_656e_7479;
/// How long after the card the time anchor goes out (Observed: 65 to 85 ms; two ticks).
const TIME_ANCHOR_MS: u64 = 66;
/// How long after its end (a hit, or the end of its flight) the entity leaves the views
/// (Observed: the slice's lifetime plus 1.0 s in every slice).
const AFTER_END_MS: u64 = 1_000;
/// The hit volume a hit on a player or a mob names (`collidedWithHitVolumeIndex`, and the
/// 2720 result byte of a turret shot). The sim does not know the target's hit volumes, so it
/// sends retail's most common value (Observed: 0 in 21,158 of the 65,265 first hits of player
/// shots on mobs, 11,018 held no volume; 791 of the 1,377 indexed 2720 results of the 12
/// ledgers with 2720).
pub(super) const BODY_HIT_VOLUME: u8 = 0;
/// Milliseconds per second.
const MS_PER_SECOND: f32 = 1_000.0;
/// Microseconds per millisecond.
const US_PER_MS: u64 = 1_000;

/// A projectile entity to spawn ([`SimWorld::spawn_projectile`]): a player's shot or a turret's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ProjectileSpawn {
    /// Its slice asset.
    pub(super) asset: [u8; ASSET_ID_LEN],
    /// The component of its projectile state.
    pub(super) component: u64,
    /// The component of its place state, when its spawn group holds one.
    pub(super) position_component: Option<u64>,
    /// Its state without the spawn point (set from `muzzle`).
    pub(super) state: ProjectileState,
    /// Where it leaves.
    pub(super) muzzle: Position,
    /// How long it flies when it meets nothing, ms.
    pub(super) flying_ms: u64,
    /// How long it stays after a hit, ms.
    pub(super) stuck_ms: u64,
}

/// A shot entity in the world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct LiveShot {
    /// Its slot key.
    number: u64,
    /// The component of its projectile state.
    component: u64,
    /// When its time anchor goes out, and the anchor; `None` once it went.
    anchor: Option<(u64, u64)>,
    /// How long it stays after a hit, ms.
    stuck_ms: u64,
    /// When it leaves the views.
    until_ms: u64,
}

/// A lifetime of `seconds` in whole milliseconds.
fn lifetime_ms(seconds: f32) -> u64 {
    // A lifetime of at most a few seconds.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a lifetime of a few seconds"
    )]
    let ms = (seconds.max(0.0) * MS_PER_SECOND) as u64;
    ms
}

/// The velocity of a shot of `speed_mps` along `aim`: yaw 0 faces +y and grows toward -x, a
/// positive pitch looks down.
pub(super) fn velocity(aim: AimLine, speed_mps: f32) -> [f32; 3] {
    let level = aim.pitch.cos() * speed_mps;
    [
        -aim.yaw.sin() * level,
        aim.yaw.cos() * level,
        -aim.pitch.sin() * speed_mps,
    ]
}

/// The projectile state member of `live` with the children of `state`.
fn state_member(component: u64, state: ProjectileState) -> BundleMember {
    BundleMember {
        component,
        state: MemberState::Projectile(state),
    }
}

/// Puts the children that `change` sets into the projectile state among `members`.
fn merge_state(members: &mut [BundleMember], change: ProjectileState) {
    for member in members {
        if let MemberState::Projectile(state) = &mut member.state {
            *state = ProjectileState {
                position: change.position.or(state.position),
                velocity: change.velocity.or(state.velocity),
                time_anchor_us: change.time_anchor_us.or(state.time_anchor_us),
                collided_with: change.collided_with.or(state.collided_with),
                hit_volume: change.hit_volume.or(state.hit_volume),
                flags: change.flags.or(state.flags),
                owner: change.owner.or(state.owner),
                ranged_attack: change.ranged_attack.or(state.ranged_attack),
                weapon_effect: change.weapon_effect.or(state.weapon_effect),
                damage_table: change.damage_table.or(state.damage_table),
                damage_row: change.damage_row.or(state.damage_row),
            };
        }
    }
}

impl SimWorld {
    /// Spawns the entity of a shot of the damage row `damage_id` that player `shooter` fires
    /// from `from` along `aim` at `now_ms`; gives its slot key. `None` for a row without an
    /// entity in the table (a hit scan, a row no retail capture shows).
    pub(super) fn spawn_shot_entity(
        &mut self,
        shooter: Entity,
        damage_id: &str,
        (from, aim): (Position, AimLine),
        now_ms: u64,
    ) -> Option<u64> {
        let shot: ShotEntity = *self.content.shot_entities.of(damage_id)?;
        let owner = facet_entity(self.world.get::<Player>(shooter)?.number);
        // The shot leaves the middle of the shooter's body ([`AimLine::of`]).
        let muzzle = muzzle_of(from);
        let state = ProjectileState {
            velocity: Some(velocity(aim, shot.base_speed_mps)),
            owner: Some(owner),
            ranged_attack: Some(shot.ranged_attack),
            weapon_effect: shot.weapon_effect.then_some(DEFAULT_WEAPON_EFFECT),
            damage_table: Some(PLAYER_DAMAGE_TABLE),
            damage_row: Some(shot.row_index),
            ..ProjectileState::default()
        };
        let spawn = ProjectileSpawn {
            asset: shot.asset,
            component: shot.component,
            position_component: shot.position_component,
            state,
            muzzle,
            flying_ms: lifetime_ms(shot.flying_lifetime_s),
            stuck_ms: lifetime_ms(shot.stuck_lifetime_s),
        };
        self.spawn_projectile(spawn, now_ms)
            .map(|(number, _)| number)
    }

    /// Spawns the projectile entity `spawn` at `now_ms` (a card of its slice and its state, the
    /// spawn point set); gives its slot key and its `GdeRef`. `None` when the muzzle is not a
    /// place.
    pub(super) fn spawn_projectile(
        &mut self,
        spawn: ProjectileSpawn,
        now_ms: u64,
    ) -> Option<(u64, [u8; RAW_ID_LEN])> {
        let muzzle = spawn.muzzle;
        let start = WorldPosition::from_metres(muzzle.x, muzzle.y, muzzle.z)?;
        let state = ProjectileState {
            position: Some(start),
            ..spawn.state
        };
        let namespace = SHOT_ENTITY_NAMESPACE ^ self.me.generation;
        let mut made = [0; RAW_ID_LEN];
        let number = self.stations.place_camp(|number| {
            // A slot key is far below `usize::MAX` on the 64-bit targets the server runs on.
            let gde_ref = dynamic_ref(namespace, number as usize);
            made = gde_ref;
            let mut members = vec![BundleMember {
                component: CARD_COMPONENT,
                state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                    asset: Some(AssetId {
                        id: spawn.asset,
                        sub_id: DYNAMIC_SLICE_SUB_ID,
                    }),
                    gde_ref: Some(gde_ref),
                }),
            }];
            if let Some(component) = spawn.position_component {
                members.push(BundleMember {
                    component,
                    state: MemberState::Position(PositionInTheWorldReplicatedState {
                        position: Some(start),
                        rotation: None,
                        scale: None,
                    }),
                });
            }
            members.push(state_member(spawn.component, state));
            Station {
                gde_ref,
                position: muzzle,
                station_type: String::new(),
                name: Vec::new(),
                members,
            }
        });
        self.live_shots.push(LiveShot {
            number,
            component: spawn.component,
            anchor: Some((
                now_ms.saturating_add(TIME_ANCHOR_MS),
                self.server_ms.saturating_mul(US_PER_MS),
            )),
            stuck_ms: spawn.stuck_ms,
            until_ms: now_ms
                .saturating_add(spawn.flying_ms)
                .saturating_add(AFTER_END_MS),
        });
        Some((number, made))
    }

    /// The shot entity of slot key `number` hit the entity `target` (a mob's or a player's
    /// number) at `at` at `now_ms`: its state names the hit, and it stays its stuck lifetime.
    pub(super) fn shot_entity_hit(&mut self, number: u64, target: u64, at: Position, now_ms: u64) {
        self.shot_entity_impact(number, Some(target), at, now_ms);
    }

    /// The shot entity of slot key `number` ended its flight at `at` at `now_ms`: on the entity
    /// `target` (a mob's or a player's number), or on the world with none (Inferred: the impact
    /// point and the collided flag without a target, as no capture shows a miss of a shot the
    /// server flew); it stays its stuck lifetime.
    pub(super) fn shot_entity_impact(
        &mut self,
        number: u64,
        target: Option<u64>,
        at: Position,
        now_ms: u64,
    ) {
        let collided = target.map(|target| (facet_entity(target), Some(BODY_HIT_VOLUME)));
        self.shot_entity_collided(number, collided, at, now_ms);
    }

    /// The shot entity of slot key `number` met a solid of the level at `at` at `now_ms`: its
    /// state names the solid's level entity `level` (its GDE id; `None` for a solid of no
    /// entity, a hit of the world) with no hit volume (module doc); it stays its stuck lifetime.
    pub(super) fn shot_entity_on_level(
        &mut self,
        number: u64,
        level: Option<u64>,
        at: Position,
        now_ms: u64,
    ) {
        self.shot_entity_collided(number, level.map(|id| (id, None)), at, now_ms);
    }

    /// The shot entity of slot key `number` ended its flight at `at` at `now_ms`, on the entity
    /// of facet entity and hit volume `collided` (`None`: none); it stays its stuck lifetime.
    fn shot_entity_collided(
        &mut self,
        number: u64,
        collided: Option<(u64, Option<u8>)>,
        at: Position,
        now_ms: u64,
    ) {
        let Some(live) = self
            .live_shots
            .iter_mut()
            .find(|live| live.number == number)
        else {
            return;
        };
        live.until_ms = now_ms
            .saturating_add(live.stuck_ms)
            .saturating_add(AFTER_END_MS);
        let component = live.component;
        let Some(impact) = WorldPosition::from_metres(at.x, at.y, at.z) else {
            return;
        };
        let hit = ProjectileState {
            position: Some(impact),
            collided_with: collided.map(|(facet, _)| facet),
            hit_volume: collided.and_then(|(_, volume)| volume),
            flags: Some(COLLIDED_FLAGS),
            ..ProjectileState::default()
        };
        self.stations.change_camp(
            number,
            |members| merge_state(members, hit),
            vec![state_member(component, hit)],
        );
    }

    /// Sends the time anchors that are due at `now_ms` and takes away the shot entities whose
    /// time is over; counts the tick off the state changes that go out.
    pub(super) fn tick_shot_entities(&mut self, now_ms: u64) {
        self.stations.tick_changes();
        let mut live = std::mem::take(&mut self.live_shots);
        live.retain_mut(|shot| {
            if now_ms >= shot.until_ms {
                self.stations.remove_camp(shot.number);
                return false;
            }
            if let Some((due_ms, anchor_us)) = shot.anchor
                && now_ms >= due_ms
            {
                shot.anchor = None;
                let anchor = ProjectileState {
                    time_anchor_us: Some(anchor_us),
                    ..ProjectileState::default()
                };
                self.stations.change_camp(
                    shot.number,
                    |members| merge_state(members, anchor),
                    vec![state_member(shot.component, anchor)],
                );
            }
            true
        });
        self.live_shots = live;
    }
}
