// SPDX-License-Identifier: AGPL-3.0-or-later
//! The turrets of a war fort (turret study 2026-11-12; the forts' turrets and the turret kinds
//! from [`nw_content::war_forts`]; the calls from [`nw_messages::turrets`]).
//!
//! **Spawn** (Observed: every turret group of the six war captures is in the instance from the
//! entry, `joining_defending_war_2` 0.5 s): each turret of the fort spawns with the war's
//! instance, its members as retail sent them first, its turret state idle (yaw, pitch and heat 0,
//! no interactor, not overheated, not blocked), its interact with no interactor, its projectile
//! spawner full and free, and the ownership of its side's company (Observed on the repeaters of
//! `joining_defending_war_2`: the company's id, name and crest, no owner, permission rule 3,
//! raid 0, faction 0; rule 4 and its side's raid id while the war runs, the
//! [`super::siege_structures`] module doc, **The war's run**).
//!
//! **Use** (Observed, `RS_offensive_war` 857.974 s and `joining_defending_war_2` 194.538 s): the
//! client's interaction of kind [`USE_TURRET_KIND`] on the turret is accepted (3743) about 45 ms
//! later; the turret's interact shows one interactor and its turret state names the player
//! (raw id and facet entity); its ownership names the user as a dropper's use does
//! ([`SimWorld::name_use_owner`]): the owner alone in a war (`RS_offensive_war` 858.810 s,
//! `joining_defending_war_2` 194.609 s), the user's company (id, name and crest), the user and
//! the user's faction in an Outpost Rush match (`OPR_TEST3` 1718.191 s, `opr_que_and_lost`
//! 1710.646 s). The release sends no ownership (`OPR_TEST3` 1725.923 s): the full ownership
//! keeps the last user until the next use names the next one (1743.941 s). The client then sends its aim about every 33 to 50 ms (4273 for a
//! cannon, 4145 for a repeater) and sends no call to leave: the retail server let the turret go
//! 132 and 147 ms after the last call (`joining_defending_war_2` 199.373 -> 199.505 s,
//! `RS_offensive_war` 877.477 -> 877.624 s): the last yaw and pitch, no interactor, no
//! interactors, and for a repeater with heat the interaction blocked until the heat is gone
//! (201.737 s: heat 0 and the block off). Inferred: a use ends [`USE_TIMEOUT_MS`] after the
//! last call, at the client's cancel (2473), or when the player leaves the instance; only a
//! player of the turret's side uses it, one at a time (a refused use answers 3743 with
//! [`USE_REFUSED`], the status of a refused build).
//!
//! **A repeater** (`m_holdToFireRepeatedly`; Observed, `joining_defending_war_2` 195.438 to
//! 199.256 s): each 4145 whose flags say the client fired gets one 2720 from the turret: the
//! point where the shot met the ground (55 to 92 m away; heading the turret's facing plus the
//! call's yaw, rising by its pitch, from [`REPEATER_MUZZLE`] over the turret's place, fitted on 16
//! shots), no target and result `ff` when it met no body; the turret's ammo falls by one a shot
//! (200 -> 0 over the ledger); the heat (half float) rises about 20 a second while the trigger
//! is held (sent about every 0.53 s, [`HEAT_RISE_SEND_MS`]) and falls 40 a second from 0.5 s
//! after the fire stopped (sent each tick), the slice's `m_heatMeterGainRate`,
//! `m_heatMeterLossRate` and `m_heatLossDelay`. Inferred: a shot meets the first foe or mob on
//! its line within the slice's ranged attack range (125 m) before the ground; the shots of one
//! use are spaced by at least `m_repeatFireDelay` on the client's clock; a turret at its heat
//! capacity is overheated and fires no more until it cooled to 0.
//!
//! **The 2720 result byte** (Observed in the client: the message's `Unmarshal` reads the
//! position, the target and the byte; its handler (`TurretComponentClientFacet` at
//! `0x144167b00`) puts the byte in the impact it sends on the impact bus for a shot with a
//! target; the impact handler (`0x14451ab80`) asks the target's hit volume bus for the volume of
//! that index (`0x144514b30`), which the target's `HitVolumeComponent` answers from its list of
//! hit volumes (`0x1467fe580`: none for a negative index or one past the list), and plays the
//! volume's impact, else the target entity's own): the byte is the index of the target's hit
//! volume that the shot met, `ff` for none ([`NO_HIT_VOLUME`]). Observed in the 12 ledgers with
//! 2720 (2,628 bodies): 1,251 `ff` (every shot with no target, and the shots on entities that
//! are no body), 1,377 an index from 0 to 9 (0 in 791, 1 in 437); one mob always gets the same
//! index (Tempest's Heart: each target only 0 or only 1), a player gets several (Outpost Rush:
//! 0, 1, 2, 6 and 7 on one target), a structure none (`OPR_TEST3`: three repeater shots on a
//! built `or_turretfire`, facet entity `d08ea7b0680df518`, all `ff`). Inferred: the sim does
//! not hold the bodies' hit volumes, so a shot on a player or a mob names [`BODY_HIT_VOLUME`];
//! a shot on a turret, a structure as that one, names none.
//!
//! **Shots on structures** (Observed, `OPR_TEST3` 1719.807, 1720.042 and 1720.273 s): three
//! shots of the repeater of slot 79 (team raid 4501) met the built vat of slot 123 of the same
//! raid, each 2720 naming the vat's facet entity with result `ff`; the vat's vitals (component 1)
//! did not change from its build at 1706.442 s (30,000) to 1724 s. So a shot stops at a
//! structure of its own side and does it no harm. Inferred: a turret's shot meets every whole
//! structure of the fight that is no turret, of both sides ([`SimWorld::turret_shot_targets`]:
//! the war fight's structures; the built outpost structures with vitals of the owned outposts),
//! with [`TURRET_BODY`] and no hit volume; one of the other side takes the shot's health as a
//! turret of that side does (a war structure its own health; an outpost structure as a hit of
//! the shooter, with the shooter's `DamageWithSiegeWeapons`). The turrets of the shooter's side
//! stay out of a shot's way (no capture holds a shot on one), and the siege areas meet bodies
//! and turrets only.
//!
//! **The other `ff` targets** (Observed: the cards of the 2720 targets of the 12 ledgers with
//! 2720; a target is the facet entity of a card's `GdeRef`): a war capture point
//! (`joining_defending_war_2` 1282.655 and 1282.887 s, point 2 of territory 11, 0.5 m from its
//! place), which stops the shot and has no health: the sim's war capture points stand in the
//! shots' way from the war's start ([`TurretShotTarget::Obstacle`], [`CAPTURE_POINT_BODY`]).
//! The others are things the sim does not hold: a fire trap (`trap_fire`, 1181.693 s), an
//! invasion fort gate (`siege_fortgate_l_t1`, `Reekwater_Invasion`), an invasion cow launcher
//! (`invasion_cowlauncher`, `invasion` 18 shots), the shield spell of an invasion priest
//! (`invasion_priest_shield`) and the relic bubble of a Shattered Mountain encounter; and level
//! entities that no card names: retail projectiles name them as `collidedWithGDEId`, spells
//! hang on them (`attch`), and the same ids come back in other matches of the same map
//! (`d2667a566f022663` and `0975e411d7be28b3` in `opr_que_and_lost`, `opr_match_4` and both
//! Tower ledgers; `0000000008c3e501`, `83fca943ae57c09b` and `5212565aa54af8b3` in
//! `OPR_TEST3`; the fort walls of `joining_defending_war_2`, 22 shots on `65c2f1618e936d97`
//! over 40 m of wall). No level mission file holds them as written (Unknown: how the client
//! makes a level entity's id), and the sim's level collision names no entity (and holds no
//! Outpost Rush level), so the sim's shot goes on to the ground ([`ground_along`]) or a body
//! behind it.
//!
//! **A cannon** (a turret with a projectile; Observed, `RS_offensive_war` 859.971 to 876.921 s,
//! six shots): each 4274 gets a projectile entity (card and class 16 state: spawn point
//! [`CANNON_MUZZLE`] off the turret's place, velocity the slice's `m_fireSpeed` along the aim,
//! the shooter, the ranged attack, the damage table and row) and a 327 to the shooter naming it,
//! 46 to 97 ms later; the ammo falls by one and firing is blocked, free again the slice's
//! `m_fireCooldown` (3.0 s) later. The server flies the ball with the slice's
//! `m_projectileGravity` (13 m/s², Observed on six shots) and sends its impact 1.5 to 2.1 s
//! later. Inferred: the ball meets the first foe or mob within the attack's radius of its path,
//! else the ground; it hurts the body it met. The other players get the spawner's 2900
//! (`ProjectileServerSpawn`, the spawner's facet header) at the shot (Observed on another
//! player's Outpost Rush ballista: `opr_que_and_lost` 1758.346 s firing blocked, 1758.364 s the
//! 2900, free again 2.5 s later at 1760.844 s, the slice's `m_fireCooldown`; the shooter of
//! `RS_offensive_war` got its 327 and no 2900).
//!
//! **A ballista and an explosive launcher** fire as a cannon (Inferred: no capture holds a shot
//! of either; both are turrets without `m_holdToFireRepeatedly` whose spawner fires a
//! projectile, as the cannon's does). Their projectile comes from their slices
//! ([`nw_content::war_forts::TurretKind::projectile`]: the spawner's `m_ammoId` slice, the
//! replication index of its projectile component, the ranged attack and the damage row; the
//! same rule gives the captured cannon ball). They leave from [`CANNON_MUZZLE`] (Inferred: the
//! slices place the shot at the `projectile_spawn` joint of their model, which the server does
//! not read).
//!
//! **Damage of a shot** (Inferred: no capture shows a turret hit on a player whose health the
//! capture holds): a hit takes the slice's damage row's coefficient (`DamageTable_Structures`
//! `RepeaterTurret_Projectile` 2.2, `Cannon_Projectile` 1.0) times the base damage of the
//! shooter's level.
//!
//! **Damage a turret takes** (Observed in the vitals (15) and buildable controller (1594) states
//! of the war turrets: `arvaysa_ES_attacker_war_45vs45` 925.121 to 2141.723 s, `everfall_war_2`
//! 52.315 to 258.524 s, `CK_won_offensive_war` 1148.978 s, `Reekwater_Invasion` 273.585 s,
//! `joining_defending_war_2` 83.7 and 1883.7 s): a turret is invulnerable until the war starts
//! (its vitals' `invulnerability` goes to 0 at the start, ES 925.121 s) and again from the
//! result (1 at ES 2141.723 s, with its full health; `joining_defending_war_2` 1883.7 s also
//! filled its ammo). A hit sends its health and `healthChangeFlags` 3. Its committed
//! `SiegeSupply` (its build cost in its first buildable state) is that cost times its health
//! share, rounded up (a T3 repeater of cost 120: 117 at 105,266.1 of 108,000 health, 1 at 640.3),
//! sent as a full list with a new version when it changes. At 0 health it breaks
//! (`everfall_war_2` 102.646 s): its vitals block gets the dead head, its buildable state goes
//! to 6 with no committed resource and ruin damage type 10, and its interact is disabled; it
//! stays broken until the result. Inferred: the shots of a turret meet the turrets of the other
//! side ([`TURRET_BODY`]) with the damage of a shot; a user of a turret that breaks is let go.
//!
//! **Player weapon hits** on a structure ([`SimWorld::structure_hit`]): a player's swing or shot
//! meets the whole structures of the other side while the war runs
//! ([`SimWorld::structure_places`], [`TURRET_BODY`]); a hit takes the base damage of the
//! player's level times its attack row's coefficient (Inferred: the structure damage rows of
//! player weapons are not in the captures; the vitals of a hit are the Observed ones above).
//! The remote status effect 791 that a retail turret carried while it lost health
//! (`everfall_war_2` 52.315 s, stacks 1 then 2 at 54.451 s) is key 791 of the status effect
//! tables: `Status_Perk_Artifact_Set1_Bow_StaticCharge` (`Debuff+Tracking+Boltcaster`, 3 s, up
//! to 3 stacks, no `Health` or damage column): the mark of a player's artifact bow perk, not a
//! burn. The turret's health loss came from the bow's hits, so no damage over time is built for
//! it.
//!
//! **Refill and repair** (Observed, `joining_defending_war_2` 1148.839 and 1421.699 s): the
//! client's interaction of kind [`ITEM_COST_KIND`] on a repeater is accepted (3743) about 40 ms
//! later; one bag item goes with sync reason 2 (a stack of two went to one, then to none; retail
//! item index 10591, five below this table's `AmmoKit`, which `RepairKit`, `BattleToken` and
//! `SiegeSupply` follow here as there); the turret's ammo goes from 130 to 200, and only the ammo
//! is sent. The slices give the amounts: their `AddAmmo` option adds `m_ammoAmount` ammo (200 a
//! repeater, 15 a cannon), their `AddHealth` option adds `m_amount` health (5,000), both up to
//! the most. Inferred: a refill costs one [`AMMO_KIT`] ("Reloads turrets."), a repair one
//! [`REPAIR_KIT`] ("A kit ... used to repair siege emplacements."); one request refills when
//! the turret lacks ammo and the player holds an ammo kit, else repairs when it lacks health and
//! the player holds a repair kit, else it is refused; only a player of the turret's side refills
//! or repairs, and a broken turret takes neither. The call 2175
//! `PlayerTurretComponentServerFacet_RequestAmmoRefill` is in no ledger of the capture index,
//! and no function of the client decompile names it (the Aeternum catalog's handler addresses
//! are of another build): it has no codec, and its body is Unknown.
//!
//! **The holder.** The turrets of a fight are the war fight's ([`WarFight::turrets`]) or the
//! Outpost Rush match's ([`super::outpost_rush_structures::OutpostStructures::turrets`]); every
//! use, call, release and tick reaches them through [`SimWorld::with_turret_runs`], by the side
//! of [`SimWorld::turret_side`] (a war side, or an Outpost Rush team).
//!
//! **Outpost Rush turrets** (the slices `or_turretrepeater`, `or_turretballista` and
//! `or_turretcannon` have the turret fields the war turrets have; `or_turretfire` and
//! `or_turretfire_trench` have no turret component: they are droppers, [`super::siege_areas`];
//! no OPR layout places a cannon, Observed in the 12 OPR ledgers). Observed: a built repeater (`opr_que_and_lost`
//! 1708.830 s) is used by a player (its turret state names the player and its use interact,
//! component 11, has one interactor, 1710.646 s), its shots go to every player as 2720 from
//! 1712.565 s (77 in `opr_que_and_lost`, 40 in `OPR_TEST3`), its heat rises while it fires;
//! the facet rules of the slices give the retail 2720 and 2900 headers (repeater turret
//! `8950d4fd13fe7af2`, ballista spawner `4f41fa1d207f33f4`). The spawners use no ammo
//! (`m_usesAmmo` off): every spawner state of the 12 ledgers holds ammo `ffff`
//! ([`UNCOUNTED_AMMO`]), so a turret needs no refill; a ballista reloads in its fire cooldown.
//! A turret's side is its outpost's owner team; when the outpost changes hands, a user of the
//! other team is let go (Inferred). Its health is its outpost structure's: a hit of a turret
//! shot on it is a hit on the structure ([`SimWorld::hit_outpost_rush_structure`]); at the ruin
//! (Observed: `opr_que_and_lost` 1241.411 and 1888.872 s, the use interact disabled with no
//! interactor) its user is let go and its use is disabled; a rebuild makes it as its first
//! build did (Inferred: idle, cold, its use interact enabled). The damage of a turret's hit
//! counts as the shooter's `DamageWithSiegeWeapons` on the war board (Inferred: the board's row
//! for the damage a player dealt with a siege weapon). A kit refill or repair of an Outpost
//! Rush turret is refused (Inferred: it lacks no ammo, and its health is repaired with
//! resources as any outpost structure).
//!
//! **The Isle of Night ballista** (batch E1; Observed, game mode `RaidMonarchsBluffs00`, the
//! encounter `EncounterCh01Boss01` spawns `ion_ballista` as its target 22; `ION-a9280e800e2eccdc`
//! 440.476 to 816.116 s, the ballistas other players used): each spawns with one bolt (spawner
//! ammo 1, not blocked); a use shows one interactor, its shot leaves it with no ammo and firing
//! blocked (530.724 s), the user goes (531.192 s); the `AddAmmo` option
//! (`RaidMonarchsBluffs00AddAmmo`, `m_ammoAmount` 1) then shows its cooldown (option 1, ending
//! 1 s later, [`AMMO_ADD_COOLDOWN_MS`]) and the ammo 1 (532.259 s), and the firing block goes
//! (533.228 s). An encounter's turret is the level's ([`TurretHost::Level`]:
//! [`SimWorld::spawn_encounter_turret`], [`SimWorld::remove_encounter_turret`]), of the level's
//! side: every player uses it. Inferred: its ammo add costs the status effect of the gatherable
//! `IoN_Balista_Bolt` (`Raid_IoN_Balista_Bolt`; the Isle of Night tables name no other cost and
//! no item), which the player loses ([`nw_content::war_forts::TurretKind::ammo_effect`]); it
//! takes no harm (no capture shows a hit on one).

use bevy_ecs::entity::Entity;
use nw_bus::ConnectionRef;
use nw_content::war_forts::{FortStructure, SiegeArea, TurretKind};
use nw_messages::buildables::OwnerIdentification;
use nw_messages::client_input::TRIGGER_KIND;
use nw_messages::combat_effects::{AUTHORITATIVE_PROJECTILE_FLAG, CombatEffectMsg};
use nw_messages::entity_ids::{facet_of_raw_id, raw_id_of_entity};
use nw_messages::facet::facet_entity;
use nw_messages::items::SYNC_CRAFT_USED;
use nw_messages::projectile_state::ProjectileState;
use nw_messages::turrets::{
    PROJECTILE_SPAWNER_TYPE_INDEX, ProjectileSpawnerState, TURRET_GDE_REF_LEN, TurretAim,
    TurretClientMsg, gde_ref_of, turret_half,
};
use nw_messages::war_states::GDE_REF_LEN;
use nw_messages::position::POSITION_IN_THE_WORLD_TYPE_INDEX;
use nw_messages::{
    BundleMember, ClaimOwner, FacetHeader, FullList, InteractCooldown, InteractCooldownState,
    InteractState, InteractionRequest, MemberState, OwnershipState, ServerMessage, TurretState,
};
use nw_sim_core::{Ground, Position};

use super::attack::Vitals;
use super::crafting::{Station, dynamic_ref};
use super::projectile::AimLine;
use super::shot_entity::{BODY_HIT_VOLUME, ProjectileSpawn, velocity};
use super::siege_areas::{AreaSource, LEVEL_SIDE, level_ownership, rotation_of};
use super::siege_structures::{
    Hurt, NO_INTERACTORS, OPEN_SELECTION_KIND, StructureBody, StructureRun, spawn_members,
};
use super::supply_generators::CONTAINER_KIND;
use super::war_fight::{WarFight, replace_member};
use super::{Body, Player, SimWorld, gather_answer, send_now};
use crate::combat::{AttackOutcome, AttackRefusal};
use crate::mobs::Capsule;

/// The interaction kind of a turret's use (`InteractionUIActions` `UseTurret`, 0x34, registered
/// by `FUN_1435eaa20`; Observed on every turret use of the captures).
pub(super) const USE_TURRET_KIND: u8 = 52;
/// The 3743 status of an accepted use (Observed) and of a refused one (Inferred).
pub(super) const USE_ACCEPTED: u8 = 1;
pub(super) const USE_REFUSED: u8 = 2;
/// How long after the user's last call a use ends (Inferred from the retail 132 and 147 ms).
pub(super) const USE_TIMEOUT_MS: u64 = 150;
/// The interactor of a turret nobody uses (Observed: 20 zero bytes, then `ffffffff`).
const NO_INTERACTOR: [u8; GDE_REF_LEN] = {
    let mut none = [0; GDE_REF_LEN];
    let mut at = GDE_REF_LEN - NO_INTERACTOR_TAIL;
    while at < GDE_REF_LEN {
        none[at] = u8::MAX;
        at += 1;
    }
    none
};
const NO_INTERACTOR_TAIL: usize = 4;
/// The interactors of a turret in use and of a free one (Observed).
const ONE_INTERACTOR: u32 = 1;
/// The 2720 target of a shot that met no body (Observed).
const NO_TARGET: u64 = 0x0000_0000_ffff_ffff;
/// The 2720 result byte that names no hit volume (Observed: the client handler reads an
/// index of `0xff` as none; module doc, **The 2720 result byte**).
pub(super) const NO_HIT_VOLUME: u8 = 0xff;
/// Where a repeater's shot leaves: metres forward of and above the turret's place (Observed:
/// 3.14 to 3.48 m above on 16 shots of `joining_defending_war_2`, no forward offset fitted).
const REPEATER_MUZZLE: (f32, f32) = (0.0, 3.3);
/// Where a cannon's ball leaves (Observed: the first ball of `RS_offensive_war` spawned 2.5 m
/// forward of and 3.24 m above its turret's place).
const CANNON_MUZZLE: (f32, f32) = (2.5, 3.24);
/// How often a rising heat is sent (Observed: about 0.53 s in `joining_defending_war_2`).
pub(super) const HEAT_RISE_SEND_MS: u64 = 530;
/// The ammo of a projectile spawner that uses none (`m_usesAmmo` off). Observed: every
/// projectile spawner state (57) of the Outpost Rush turrets in the 12 OPR ledgers holds ammo
/// `ffff` (`opr_que_and_lost` 1708.830 s and on).
const UNCOUNTED_AMMO: u16 = u16::MAX;
/// The ownership of a turret (Observed, module doc).
const TURRET_PERMISSION_RULE: u8 = 3;
const TURRET_RAID: u64 = 0;
const TURRET_FACTION: u8 = 0;
const TURRET_PERMISSION_OPTIONS: u8 = 0;
/// The step of the walk along a shot's line to the ground, metres.
const GROUND_STEP_M: f32 = 0.5;
/// The step of a cannon ball's flight, seconds.
const FLIGHT_STEP_S: f32 = 0.02;
/// Milliseconds and nanoseconds per second.
const MS_PER_SECOND: f32 = 1_000.0;
const NS_PER_SECOND: f32 = 1_000_000_000.0;
/// Half of a height.
const HALF: f32 = 0.5;
/// The base level of a level index ([`nw_content::progression::Progression::level_of`] counts
/// from 0).
const FIRST_LEVEL: u32 = 1;
/// The interaction kind of a refill or a repair for an item (`InteractionUIActions`
/// `InteractWithItemCost`, 0x35, registered by `FUN_1435eaa20`; Observed on both retail
/// refills).
pub(super) const ITEM_COST_KIND: u8 = 53;
/// The item a refill costs and the item a repair costs, and how many (Inferred, module doc).
pub(super) const AMMO_KIT: &str = "AmmoKit";
pub(super) const REPAIR_KIT: &str = "RepairKit";
const KIT_COST: u32 = 1;
/// The option of an encounter turret's ammo add and how long it stays on cooldown after it
/// (Observed: option 1, `RaidMonarchsBluffs00AddAmmo`, its end 1 s after the add,
/// `ION-a9280e800e2eccdc` 532.259 and 689.652 s; module doc, **The Isle of Night ballista**).
const AMMO_ADD_OPTION: u32 = 1;
const AMMO_ADD_COOLDOWN_MS: u64 = 1_000;
/// The namespace of the `GdeRef`s of encounter turrets ("encturet").
const ENCOUNTER_TURRET_NAMESPACE: u64 = 0x656e_6374_7572_6574;
/// The body a shot meets on a turret (Inferred: the turret's collision shape is not read; about
/// its base, 3 m wide and 3 m high).
const TURRET_BODY: Capsule = Capsule {
    radius_m: 1.5,
    height_m: 3.0,
};
/// The body a hit or a shot meets on the structure `body`: a gate's reaches its slice's
/// physical radius and as high (Inferred: the gate slices' collision shapes are not read; their
/// `m_slicePhysicalGridRadius`, 10.23 m for `siege_fortgate_l_t2`, bounds the slice), every other
/// structure's is [`TURRET_BODY`].
fn structure_body(body: &StructureBody) -> Capsule {
    body.reach_m.map_or(TURRET_BODY, |reach_m| Capsule {
        radius_m: reach_m,
        height_m: reach_m,
    })
}

/// The body a shot meets on a war capture point (Inferred: the point slice's collision shape is
/// not read; the two retail impacts, `joining_defending_war_2` 1282.655 and 1282.887 s, met it
/// 0.5 m from its place and 0.2 m above it, so a low disc around its place).
const CAPTURE_POINT_BODY: Capsule = Capsule {
    radius_m: 1.0,
    height_m: 1.0,
};

/// The player who uses a turret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TurretUser {
    entity: Entity,
    number: u64,
    interaction: u64,
    last_call_ms: u64,
}

/// The heat of a repeater (module doc).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct Heat {
    value: f32,
    overheated: bool,
    held: bool,
    /// Sim ms of the last change and of the last shot.
    at_ms: u64,
    fired_ms: u64,
    /// The value last sent, and when.
    sent: f32,
    sent_ms: u64,
}

impl Heat {
    /// The heat at `now_ms` of a turret of `kind`: it rises while the trigger is held and the
    /// turret is not overheated, up to the capacity (then it is overheated); it falls from the
    /// loss delay after the last shot down to 0 (then it is no longer overheated).
    pub(super) fn advance(&mut self, kind: &TurretKind, now_ms: u64) {
        let seconds = now_ms.saturating_sub(self.at_ms) as f32 / MS_PER_SECOND;
        self.at_ms = now_ms;
        if kind.heat_capacity <= 0.0 {
            return;
        }
        if self.held && !self.overheated {
            self.value = (self.value + kind.heat_gain_per_s * seconds).min(kind.heat_capacity);
            self.overheated = self.value >= kind.heat_capacity;
            return;
        }
        let delay_ms = (kind.heat_loss_delay_s * MS_PER_SECOND) as u64;
        if now_ms.saturating_sub(self.fired_ms) >= delay_ms {
            self.value = (self.value - kind.heat_loss_per_s * seconds).max(0.0);
            if self.value <= 0.0 {
                self.overheated = false;
            }
        }
    }

    /// Whether the heat is due to go out at `now_ms`: a rise every [`HEAT_RISE_SEND_MS`], a fall
    /// and the last 0 at once.
    pub(super) fn due(&self, now_ms: u64) -> bool {
        if self.value == self.sent {
            return false;
        }
        self.value < self.sent || now_ms.saturating_sub(self.sent_ms) >= HEAT_RISE_SEND_MS
    }

    fn mark_sent(&mut self, now_ms: u64) {
        self.sent = self.value;
        self.sent_ms = now_ms;
    }
}

/// An area a landed ball opens: the area, where, and who fired it.
type ShellArea = (SiegeArea, Position, AreaSource);

/// A shot's hit on a structure of the fight, the health it takes and the shooter's number.
type HeldShotHit = (HeldHit, u32, u64);

/// What one repeater shot did: the outcomes of the bodies it hit, and the structure of the
/// fight it hurt with the health it takes.
type RepeaterShot = (
    Vec<Result<AttackOutcome, AttackRefusal>>,
    Option<(HeldHit, u32)>,
);

/// What a turret shot meets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShotTarget {
    /// A player or a mob, by its number.
    Body(u64),
    /// A turret of the fight: its place in the fight's list and its facet entity.
    Turret { index: usize, facet: u64 },
}

impl ShotTarget {
    /// The 2720 target of a hit on it (Observed for a body: its facet entity).
    fn facet(self) -> u64 {
        match self {
            Self::Body(number) => facet_entity(number),
            Self::Turret { facet, .. } => facet,
        }
    }

    /// The 2720 result byte of a hit on it (module doc, **The 2720 result byte**): a body's
    /// hit volume, none for a turret.
    const fn hit_volume(self) -> u8 {
        match self {
            Self::Body(_) => BODY_HIT_VOLUME,
            Self::Turret { .. } => NO_HIT_VOLUME,
        }
    }
}

/// What a turret's shot meets ([`SimWorld::turret_shot_targets`]): what a siege area also
/// meets, or a structure of the fight that is no turret (module doc, **Shots on structures**).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TurretShotTarget {
    /// A body or a turret of the other side.
    Shot(ShotTarget),
    /// A whole structure: its slot key, its facet entity, and whether it is of the other side.
    Structure { number: u64, facet: u64, foe: bool },
    /// A thing of the fight that stops a shot and takes no harm, by its facet entity: a war
    /// capture point (module doc, **Shots on structures**).
    Obstacle { facet: u64 },
}

impl TurretShotTarget {
    /// The 2720 target of a hit on it (Observed for a structure: the facet entity of the vat,
    /// `OPR_TEST3` 1719.807 s).
    fn facet(self) -> u64 {
        match self {
            Self::Shot(target) => target.facet(),
            Self::Structure { facet, .. } | Self::Obstacle { facet } => facet,
        }
    }

    /// The 2720 result byte of a hit on it: none for a structure (Observed, `OPR_TEST3`) and a
    /// capture point (`joining_defending_war_2` 1282.655 s).
    const fn hit_volume(self) -> u8 {
        match self {
            Self::Shot(target) => target.hit_volume(),
            Self::Structure { .. } | Self::Obstacle { .. } => NO_HIT_VOLUME,
        }
    }

    /// The structure of the fight that a hit on it hurts; `None` for a body, for a structure of
    /// the shooter's side and for an obstacle, which stop the shot unharmed (module doc).
    pub(super) const fn held(self) -> Option<HeldHit> {
        match self {
            Self::Shot(ShotTarget::Turret { index, .. }) => Some(HeldHit::Turret(index)),
            Self::Structure {
                number, foe: true, ..
            } => Some(HeldHit::Structure(number)),
            Self::Shot(ShotTarget::Body(_))
            | Self::Structure { foe: false, .. }
            | Self::Obstacle { .. } => None,
        }
    }
}

/// A structure of the fight that a turret's shot hurts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HeldHit {
    /// A turret, by its place in the fight's list.
    Turret(usize),
    /// A structure that is no turret, by its slot key.
    Structure(u64),
}

/// The fight whose turrets and trigger structures a call reaches ([`SimWorld::with_turret_runs`],
/// [`SimWorld::with_use_runs`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TurretHost {
    /// The war fight of this instance ([`WarFight::turrets`]).
    War,
    /// The Outpost Rush match ([`super::outpost_rush_structures::OutpostStructures::turrets`]).
    OutpostRush,
    /// The level itself, outside a fight ([`super::siege_areas::LevelSiege`]).
    Level,
}

/// A cannon ball on its way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct BallFlight {
    /// The slot key of its projectile entity.
    entity: u64,
    /// The shooter's number.
    shooter: u64,
    /// Sim ms of its impact, where, and what it met.
    pub(super) land_ms: u64,
    at: Position,
    target: Option<TurretShotTarget>,
    /// The hit's health.
    amount: u32,
}

/// One turret of the fight.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TurretRun {
    /// Its vitals, buildable and interact parts ([`StructureBody`]).
    pub(super) body: StructureBody,
    kind: TurretKind,
    /// The component of its turret state.
    turret_component: u64,
    /// The packed turn of its retail first turret state (`initialOrientation`).
    orientation: Option<nw_messages::war_states::PackedTurn>,
    user: Option<TurretUser>,
    /// The last aim, radians.
    yaw: f32,
    pitch: f32,
    ammo: u16,
    heat: Heat,
    /// Sim ms a cannon's firing is free again.
    blocked_until_ms: Option<u64>,
    /// A cannon's firing is blocked.
    firing_blocked: bool,
    /// No one may use it until its heat is gone.
    interaction_blocked: bool,
    /// A use named its user in its ownership, which keeps the user after the use (module doc,
    /// **Use**).
    used: bool,
    /// The client time of the last shot of the use, ns.
    last_shot_ns: Option<u64>,
    /// Its balls on their way.
    pub(super) balls: Vec<BallFlight>,
    /// How many ammo adds paid with a status effect it took (the version of its cooldown list).
    effect_adds: u64,
}

impl TurretRun {
    /// `turret` of `kind` as it spawns (module doc): idle, full of ammo, whole, invulnerable,
    /// with the turn, the interact, the vitals and the buildable state of its retail first
    /// members; `None` for a structure with no turret state.
    fn new(turret: FortStructure, kind: TurretKind) -> Option<Self> {
        let turret_component = turret.turret_component?;
        let orientation = turret.members.iter().find_map(|bytes| {
            match BundleMember::read_from(bytes).ok()?.0.state {
                MemberState::Turret(state) => state.orientation,
                _ => None,
            }
        });
        Some(Self {
            ammo: full_ammo(&kind),
            body: StructureBody::new(turret),
            kind,
            turret_component,
            orientation,
            user: None,
            yaw: 0.0,
            pitch: 0.0,
            heat: Heat::default(),
            blocked_until_ms: None,
            firing_blocked: false,
            interaction_blocked: false,
            used: false,
            last_shot_ns: None,
            balls: Vec::new(),
            effect_adds: 0,
        })
    }

    /// The turret of an Outpost Rush outpost structure (`turret`, of `kind`, at slot key
    /// `number`) as its build makes it (module doc): idle, whole and open to damage; its
    /// vitals and buildable states are the outpost structure's
    /// ([`super::outpost_rush_structures`]), so `turret` carries none.
    pub(super) fn outpost_turret(turret: FortStructure, kind: TurretKind, number: u64) -> Option<Self> {
        let mut run = Self::new(turret, kind)?;
        run.body.number = number;
        run.body.invulnerable = false;
        Some(run)
    }

    /// It stands built again after the ruin of its outpost structure (module doc): as its first
    /// build made it, with its balls in flight and the last use its ownership names.
    pub(super) fn mend(&mut self) {
        if let Some(mut mended) =
            Self::outpost_turret(self.body.structure.clone(), self.kind.clone(), self.body.number)
        {
            mended.balls = std::mem::take(&mut self.balls);
            mended.used = self.used;
            *self = mended;
        }
    }

    /// The number of the player who uses it.
    pub(super) fn user_number(&self) -> Option<u64> {
        self.user.map(|user| user.number)
    }

    /// Whether a player uses it.
    pub(super) const fn in_use(&self) -> bool {
        self.user.is_some()
    }

    /// Whether a use named its user in its ownership (module doc, **Use**).
    pub(super) const fn used(&self) -> bool {
        self.used
    }

    /// Its own state in the place of the retail `member`: its turret, interact and projectile
    /// spawner states; `None` for a member of the body or a member it keeps as retail sent it.
    pub(super) fn own_member(&self, member: &BundleMember) -> Option<BundleMember> {
        if member.component == self.turret_component {
            return Some(self.turret_member(self.state()));
        }
        if Some(member.component) == self.body.structure.interact_component {
            return self.interact_member();
        }
        if Some(member.component) == self.body.structure.spawner_component {
            return self.spawner_member(self.full_spawner());
        }
        None
    }

    /// The turret's facet header for its 2720.
    fn turret_header(&self) -> FacetHeader {
        let entity = self.body.structure.facet_entity();
        FacetHeader {
            target: self.kind.turret_facet.target(entity),
            entity,
        }
    }

    /// The turret's facet header for its 327.
    fn spawner_header(&self) -> FacetHeader {
        let entity = self.body.structure.facet_entity();
        FacetHeader {
            target: self.kind.spawner_facet.target(entity),
            entity,
        }
    }

    /// Its full turret state now.
    fn state(&self) -> TurretState {
        TurretState {
            yaw: Some(turret_half(self.yaw)),
            pitch: Some(turret_half(self.pitch)),
            heat: Some(turret_half(self.heat.value)),
            orientation: self.orientation,
            interactor: Some(
                self.user
                    .map_or(NO_INTERACTOR, |user| interactor_of(user.number)),
            ),
            overheated: Some(self.heat.overheated),
            blocked: Some(self.interaction_blocked),
        }
    }

    fn turret_member(&self, state: TurretState) -> BundleMember {
        BundleMember {
            component: self.turret_component,
            state: MemberState::Turret(state),
        }
    }

    fn interact_member(&self) -> Option<BundleMember> {
        self.body.interact_member(if self.user.is_some() {
            ONE_INTERACTOR
        } else {
            NO_INTERACTORS
        })
    }

    fn spawner_member(&self, state: ProjectileSpawnerState) -> Option<BundleMember> {
        Some(BundleMember {
            component: self.body.structure.spawner_component?,
            state: MemberState::Recorded {
                class: PROJECTILE_SPAWNER_TYPE_INDEX,
                body: state.encode(),
            },
        })
    }

    fn full_spawner(&self) -> ProjectileSpawnerState {
        ProjectileSpawnerState {
            ammo: Some(self.ammo),
            firing_blocked: Some(self.firing_blocked),
        }
    }

    /// Where its shots leave and along which line ([`AimLine`]: the facing plus the aim's yaw;
    /// the player convention's pitch looks down).
    fn muzzle(&self) -> (Position, AimLine) {
        let (forward, up) = if self.kind.hold_to_fire {
            REPEATER_MUZZLE
        } else {
            CANNON_MUZZLE
        };
        let line = AimLine {
            yaw: self.body.structure.yaw + self.yaw,
            pitch: -self.pitch,
        };
        let [x, y, z] = self.body.structure.place;
        let muzzle = Position {
            x: x - line.yaw.sin() * forward,
            y: y + line.yaw.cos() * forward,
            z: z + up,
        };
        (muzzle, line)
    }

    /// The aim of a call, kept within the slice's limits.
    fn aim(&mut self, aim: &TurretAim) {
        let yaw_max = self.kind.yaw_max_deg.to_radians();
        self.yaw = aim.yaw.clamp(-yaw_max, yaw_max);
        self.pitch = aim.pitch.clamp(
            self.kind.pitch_min_deg.to_radians(),
            self.kind.pitch_max_deg.to_radians(),
        );
    }

    /// Whether a repeater may fire a shot at the client's time `time_ns`: not overheated, with
    /// ammo, and `m_repeatFireDelay` after the last shot of the use.
    fn repeater_may_fire(&self, time_ns: u64) -> bool {
        let delay_ns = (self.kind.repeat_fire_delay_s * NS_PER_SECOND) as u64;
        !self.heat.overheated
            && (!self.kind.uses_ammo || self.ammo > 0)
            && self
                .last_shot_ns
                .is_none_or(|last| time_ns.saturating_sub(last) >= delay_ns)
    }

    /// Whether a cannon may fire: with ammo and not blocked.
    const fn cannon_may_fire(&self) -> bool {
        (!self.kind.uses_ammo || self.ammo > 0) && !self.firing_blocked
    }
}

/// The ammo of a turret of `kind` that is full: its spawner's most, or [`UNCOUNTED_AMMO`] for a
/// spawner that uses no ammo.
const fn full_ammo(kind: &TurretKind) -> u16 {
    if kind.uses_ammo {
        kind.max_ammo
    } else {
        UNCOUNTED_AMMO
    }
}

/// The 24-byte reference of player `number` in a turret's interactor (Observed: its raw id and
/// its facet entity).
fn interactor_of(number: u64) -> [u8; GDE_REF_LEN] {
    gde_ref_of(raw_id_of_entity(number), facet_entity(number))
}

/// The ownership of a turret of the company `owner` (module doc).
pub(super) fn turret_ownership(owner: &ClaimOwner) -> OwnershipState {
    OwnershipState {
        guild_id: Some(owner.guild_id),
        guild_owner: Some(owner.guild_name.clone()),
        guild_crest: Some(owner.crest),
        owner: Some(OwnerIdentification::none()),
        permission_rule: Some(TURRET_PERMISSION_RULE),
        structure_name: Some(Vec::new()),
        raid_id: Some(TURRET_RAID),
        faction_type: Some(TURRET_FACTION),
        owner_pvp: Some(false),
        owner_ffa: Some(false),
        permission_options: Some(TURRET_PERMISSION_OPTIONS),
    }
}

/// The direction of `line` (east, north, up) as a unit vector.
fn direction(line: AimLine) -> [f32; 3] {
    let level = line.pitch.cos();
    [
        -line.yaw.sin() * level,
        line.yaw.cos() * level,
        -line.pitch.sin(),
    ]
}

/// The point `along_m` metres from `from` on `line`.
fn point_on(from: Position, line: AimLine, along_m: f32) -> Position {
    let [east, north, up] = direction(line);
    Position {
        x: from.x + east * along_m,
        y: from.y + north * along_m,
        z: from.z + up * along_m,
    }
}

/// How far along `line` from `from` the ground is met within `range_m` (`ground` gives the
/// ground height at a place); `None` when the line stays above it.
pub(super) fn ground_along(
    from: Position,
    line: AimLine,
    range_m: f32,
    ground: impl Fn(f32, f32) -> Option<f32>,
) -> Option<f32> {
    let steps = (range_m / GROUND_STEP_M).ceil() as u32;
    (1..=steps)
        .map(|step| (step as f32 * GROUND_STEP_M).min(range_m))
        .find(|along| {
            let at = point_on(from, line, *along);
            ground(at.x, at.y).is_some_and(|height| at.z <= height)
        })
}

/// The first body of `bodies` that a hit scan from `from` along `line` meets within `range_m`
/// and `radius_m`, with how far along it is.
pub(super) fn body_on_line<T: Copy>(
    from: Position,
    line: AimLine,
    (range_m, radius_m): (f32, f32),
    bodies: &[(T, Position, Capsule)],
) -> Option<(T, f32)> {
    let [east, north, up] = direction(line);
    bodies
        .iter()
        .filter_map(|(number, at, body)| {
            let middle = Position {
                z: at.z + body.height_m * HALF,
                ..*at
            };
            let (dx, dy, dz) = (middle.x - from.x, middle.y - from.y, middle.z - from.z);
            let along = dx * east + dy * north + dz * up;
            if along < 0.0 || along > range_m {
                return None;
            }
            let point = point_on(from, line, along);
            let aside = point.horizontal_distance(middle);
            let height = (point.z - middle.z).abs();
            (aside <= body.radius_m + radius_m && height <= body.height_m * HALF + radius_m)
                .then_some((*number, along))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// Where a ball fired from `from` with `velocity` under `gravity` ends within `range_m`: when
/// (s), where, and the body of `bodies` it met within `radius_m` (else the ground, else the end
/// of its range).
pub(super) fn ball_flight<T: Copy>(
    from: Position,
    velocity: [f32; 3],
    (gravity, range_m, radius_m): (f32, f32, f32),
    bodies: &[(T, Position, Capsule)],
    ground: impl Fn(f32, f32) -> Option<f32>,
) -> (f32, Position, Option<T>) {
    let speed = velocity[0].hypot(velocity[1]).max(f32::EPSILON);
    let end_s = range_m / speed;
    let mut time_s = 0.0;
    let mut at = from;
    while time_s < end_s {
        time_s = (time_s + FLIGHT_STEP_S).min(end_s);
        at = Position {
            x: from.x + velocity[0] * time_s,
            y: from.y + velocity[1] * time_s,
            z: from.z + velocity[2] * time_s - HALF * gravity * time_s * time_s,
        };
        let met = bodies.iter().find(|(_, body_at, body)| {
            at.horizontal_distance(*body_at) <= body.radius_m + radius_m
                && at.z >= body_at.z - radius_m
                && at.z <= body_at.z + body.height_m + radius_m
        });
        if let Some((number, _, _)) = met {
            return (time_s, at, Some(*number));
        }
        if ground(at.x, at.y).is_some_and(|height| at.z <= height) {
            return (time_s, at, None);
        }
    }
    (time_s, at, None)
}

impl SimWorld {
    /// Spawns the siege structures of the fort of `fight` (module doc, and
    /// [`super::siege_structures`]): each turret of a known kind as a turret, the others as
    /// structures.
    pub(super) fn spawn_structures(&mut self, fight: &mut WarFight) {
        let structures = fight.fort().structures.clone();
        let content = std::sync::Arc::clone(&self.content);
        for structure in structures {
            match (
                structure.turret_component,
                content.war_forts.kind(&structure.asset),
            ) {
                (Some(_), Some(kind)) => self.spawn_turret(fight, structure, kind.clone()),
                (Some(_), None) => {
                    tracing::warn!(turret = ?structure.gde_ref, "turret of no known kind");
                }
                (None, _) => self.spawn_structure(fight, structure),
            }
        }
    }

    /// Spawns `turret` of `kind` in `fight`, open to damage while the war runs.
    pub(super) fn spawn_turret(
        &mut self,
        fight: &mut WarFight,
        turret: FortStructure,
        kind: TurretKind,
    ) {
        let ownership = self.fort_structure_ownership(fight, &turret);
        if let Some(run) = self.place_turret(turret, kind, ownership, fight.running()) {
            fight.turrets.push(run);
        }
    }

    /// Places `turret` of `kind` in this sim with `ownership`, open to damage when
    /// `vulnerable` (module doc: idle, full of ammo, its retail first members): its run with
    /// its slot key, which the war fight or game mode that made it keeps; `None` for a structure
    /// with no turret state. No war fight is needed.
    pub(crate) fn place_turret(
        &mut self,
        turret: FortStructure,
        kind: TurretKind,
        ownership: OwnershipState,
        vulnerable: bool,
    ) -> Option<TurretRun> {
        let mut run = TurretRun::new(turret, kind)?;
        run.body.invulnerable = !vulnerable;
        let members = spawn_members(&run.body, |member| run.own_member(member), ownership);
        let gde_ref = run.body.structure.gde_ref;
        let position = run.body.place();
        run.body.number = self.stations.place_camp(|_| Station {
            gde_ref,
            position,
            station_type: String::new(),
            name: Vec::new(),
            members,
        });
        Some(run)
    }

    /// Runs `body` on the war fight of this sim and puts it back; `None` when it runs none (the
    /// war's structures, builds, areas and generators; the turrets alone go through
    /// [`Self::with_turret_runs`]).
    pub(super) fn with_turrets<T>(
        &mut self,
        body: impl FnOnce(&mut Self, &mut WarFight) -> T,
    ) -> Option<T> {
        let mut fight = self.instance_run.as_mut()?.take_war_fight()?;
        let result = body(self, &mut fight);
        if let Some(run) = self.instance_run.as_mut() {
            run.put_war_fight(fight);
        }
        Some(result)
    }

    /// The fight whose turrets and trigger structures this sim runs: the war fight of its
    /// instance, else its Outpost Rush match (module doc).
    pub(super) fn turret_host(&self) -> Option<TurretHost> {
        if self.war_fight().is_some() {
            return Some(TurretHost::War);
        }
        if self.arena_matches.outpost_rush.is_some() {
            return Some(TurretHost::OutpostRush);
        }
        Some(TurretHost::Level)
    }

    /// Runs `body` on the turrets of this sim's fight ([`Self::turret_host`]) and puts them
    /// back; `None` when it runs no fight with turrets. Only the turret list leaves its fight
    /// while `body` runs: the war fight and the match stay in place.
    pub(super) fn with_turret_runs<T>(
        &mut self,
        body: impl FnOnce(&mut Self, &mut Vec<TurretRun>) -> T,
    ) -> Option<T> {
        let host = self.turret_host()?;
        let mut turrets = match host {
            TurretHost::War => self
                .with_turrets(|_, fight| std::mem::take(&mut fight.turrets))
                .unwrap_or_default(),
            TurretHost::OutpostRush => self
                .arena_matches
                .outpost_rush
                .as_mut()
                .map(|running| std::mem::take(&mut running.structures.turrets))
                .unwrap_or_default(),
            TurretHost::Level => std::mem::take(&mut self.wars.level.turrets),
        };
        let result = body(self, &mut turrets);
        match host {
            TurretHost::War => {
                self.with_turrets(|_, fight| fight.turrets = turrets);
            }
            TurretHost::OutpostRush => {
                if let Some(running) = self.arena_matches.outpost_rush.as_mut() {
                    running.structures.turrets = turrets;
                }
            }
            TurretHost::Level => self.wars.level.turrets = turrets,
        }
        Some(result)
    }

    /// The side by which player `entity` uses turrets and trigger structures and meets them with
    /// its shots: its war side, or its team in the Outpost Rush match (the side of an outpost
    /// turret or vat is its outpost's owner team, [`super::outpost_rush_structures`]).
    pub(super) fn turret_side(&self, entity: Entity) -> Option<u8> {
        if let Some(side) = self.war_side(entity) {
            return Some(side);
        }
        let number = self.world.get::<Player>(entity)?.number;
        match self.turret_host() {
            Some(TurretHost::Level) => Some(LEVEL_SIDE),
            _ => self.arena_matches.outpost_rush.as_ref()?.team_of(number),
        }
    }

    /// Turret `index` of this sim's fight takes `amount` from a shot of player `shooter`: a war
    /// turret its own health ([`Self::hurt_turret`]); an Outpost Rush turret is an outpost
    /// structure, which takes the hit as a player's ([`Self::hit_outpost_rush_structure`]), and
    /// the hit counts as the shooter's `DamageWithSiegeWeapons` on the war board.
    pub(super) fn hurt_held_turret(&mut self, index: usize, amount: u32, shooter: u64) {
        match self.turret_host() {
            Some(TurretHost::War) => {
                self.with_turrets(|world, fight| {
                    if let Some(mut run) = fight.turrets.get(index).cloned() {
                        world.hurt_turret(&mut run, amount);
                        fight.turrets[index] = run;
                    }
                });
            }
            Some(TurretHost::OutpostRush) => {
                let target = self
                    .arena_matches
                    .outpost_rush
                    .as_ref()
                    .and_then(|running| running.structures.turrets.get(index))
                    .map(|run| run.body.number);
                if let Some(target) = target {
                    self.hit_outpost_target(target, amount, shooter);
                }
            }
            Some(TurretHost::Level) => {
                if let Some(mut run) = self.wars.level.turrets.get(index).cloned() {
                    self.hurt_turret(&mut run, amount);
                    self.wars.level.turrets[index] = run;
                }
            }
            None => {}
        }
    }

    /// Structure `hit` of this sim's fight takes `amount` from a turret shot of player
    /// `shooter` (module doc, **Shots on structures**).
    pub(super) fn hurt_held(&mut self, hit: HeldHit, amount: u32, shooter: u64) {
        match (hit, self.turret_host()) {
            (HeldHit::Turret(index), _) => self.hurt_held_turret(index, amount, shooter),
            (HeldHit::Structure(number), Some(TurretHost::War)) => {
                self.with_turrets(|world, fight| {
                    if let Some(index) = fight
                        .structures
                        .iter()
                        .position(|run| run.body.number == number)
                    {
                        let mut run = fight.structures[index].clone();
                        world.hurt_structure_run(&mut run, amount);
                        fight.structures[index] = run;
                    }
                });
            }
            (HeldHit::Structure(number), Some(TurretHost::OutpostRush)) => {
                self.hit_outpost_target(number, amount, shooter);
            }
            (HeldHit::Structure(number), Some(TurretHost::Level)) => {
                if let Some(index) = self
                    .wars
                    .level
                    .droppers
                    .iter()
                    .position(|run| run.body.number == number)
                {
                    let mut run = self.wars.level.droppers[index].clone();
                    self.hurt_structure_run(&mut run, amount);
                    self.wars.level.droppers[index] = run;
                }
            }
            (HeldHit::Structure(_), None) => {}
        }
    }

    /// An Outpost Rush structure of slot key `target` takes `amount` from a turret shot of
    /// player `shooter` as a hit of the shooter ([`Self::hit_outpost_rush_structure`]); the hit
    /// counts as the shooter's `DamageWithSiegeWeapons` on the war board.
    fn hit_outpost_target(&mut self, target: u64, amount: u32, shooter: u64) {
        let Some(entity) = self.player_by_number(shooter) else {
            return;
        };
        if self.hit_outpost_rush_structure(entity, target, amount).is_some() {
            self.outpost_rush_siege_hit(shooter, amount);
        }
    }

    /// War structure `run` (no turret) takes `amount`: its changed members go out
    /// ([`StructureBody::take`]).
    fn hurt_structure_run(&mut self, run: &mut StructureRun, amount: u32) {
        if let Some(Hurt::Hit(partial) | Hurt::Broke(partial)) = run.body.take(amount) {
            self.sync_structure(run, partial);
        }
    }

    /// A turret shot of player `shooter` hits body `number` for `amount` at `now_ms`: its
    /// outcome; a hit that landed counts as the shooter's `DamageWithSiegeWeapons` on the
    /// Outpost Rush war board (Inferred: the board's row of the damage a player dealt with a
    /// siege weapon; [`super::outpost_rush::board`]).
    fn turret_body_hit(
        &mut self,
        number: u64,
        shooter: u64,
        amount: u32,
        now_ms: u64,
    ) -> Option<Result<AttackOutcome, AttackRefusal>> {
        let outcome = self.apply_hit(number, shooter, amount, 0, now_ms)?;
        self.count_siege_hit(&outcome, shooter, amount);
        Some(Ok(outcome))
    }

    /// A siege weapon hit of `amount` of player `shooter` (a turret's shot, a siege area) with
    /// `outcome`: one that landed counts as the shooter's `DamageWithSiegeWeapons` on the
    /// Outpost Rush war board ([`Self::turret_body_hit`]).
    pub(super) fn count_siege_hit(&mut self, outcome: &AttackOutcome, shooter: u64, amount: u32) {
        if matches!(outcome, AttackOutcome::Hit { .. } | AttackOutcome::Killed { .. }) {
            self.outpost_rush_siege_hit(shooter, amount);
        }
    }

    /// Sends the turret of `run`'s full state to its spawn members and `partial` to its viewers.
    fn sync_turret(&mut self, run: &TurretRun, partial: Vec<BundleMember>) {
        let full: Vec<BundleMember> = Self::turret_members(run)
            .into_iter()
            .chain(run.body.vitals_member())
            .chain(run.body.buildable_member())
            .collect();
        self.stations.change_camp(
            run.body.number,
            |members| {
                for member in &full {
                    replace_member(members, member);
                }
            },
            partial,
        );
    }

    /// The interaction `request` of player `entity` (`number`) at `now_ms`: a use, a refill or
    /// a repair of a turret of this sim's fight (module doc); `false` when it names no turret.
    pub(super) fn turret_interaction(
        &mut self,
        entity: Entity,
        number: u64,
        request: InteractionRequest,
        name: &[u8],
        now_ms: u64,
    ) -> bool {
        match request.kind {
            USE_TURRET_KIND => {}
            ITEM_COST_KIND => return self.turret_item_cost(entity, number, request, name),
            OPEN_SELECTION_KIND => {
                return self.open_structure_selection(entity, number, request, name);
            }
            TRIGGER_KIND => return self.structure_trigger(entity, number, request, name, now_ms),
            CONTAINER_KIND => return self.open_generator(entity, number, request, name),
            _ => return false,
        }
        let side = self.turret_side(entity);
        let raid = self.use_raid_of(entity);
        let Some(connection) = self
            .world
            .get::<Player>(entity)
            .map(|player| player.connection.clone())
        else {
            return false;
        };
        self.with_turret_runs(|world, turrets| {
            let Some(index) = turrets
                .iter()
                .position(|run| run.body.structure.gde_ref == request.target)
            else {
                return false;
            };
            let run = &mut turrets[index];
            let free = run.user.is_none() && !run.interaction_blocked && !run.body.broken;
            if !free || side != Some(run.body.structure.side) {
                let refused = gather_answer(number, request, USE_REFUSED, name);
                send_now(&mut world.combat_messages, &connection, &[refused]);
                return true;
            }
            run.user = Some(TurretUser {
                entity,
                number,
                interaction: request.interaction,
                last_call_ms: now_ms,
            });
            run.last_shot_ns = None;
            run.used = true;
            let accepted = gather_answer(number, request, USE_ACCEPTED, name);
            send_now(&mut world.combat_messages, &connection, &[accepted]);
            let run = turrets[index].clone();
            if let Some(component) = run.body.structure.ownership_component {
                let user = (run.body.structure.side, entity, raid);
                world.name_use_owner(run.body.number, component, user);
            }
            let partial = vec![
                run.interact_member(),
                Some(run.turret_member(TurretState {
                    interactor: Some(interactor_of(number)),
                    ..TurretState::default()
                })),
            ];
            world.sync_turret(&run, partial.into_iter().flatten().collect());
            true
        })
        .unwrap_or(false)
    }

    /// The refill or repair `request` of player `entity` (`number`) on a turret of this sim's
    /// war (module doc): the 3743 answer; `false` when it names no turret.
    fn turret_item_cost(
        &mut self,
        entity: Entity,
        number: u64,
        request: InteractionRequest,
        name: &[u8],
    ) -> bool {
        let side = self.turret_side(entity);
        let Some(connection) = self
            .world
            .get::<Player>(entity)
            .map(|player| player.connection.clone())
        else {
            return false;
        };
        self.with_turret_runs(|world, turrets| {
            let Some(run) = turrets
                .iter_mut()
                .find(|run| run.body.structure.gde_ref == request.target)
            else {
                return false;
            };
            let done = side == Some(run.body.structure.side)
                && !run.body.broken
                && world.spend_kit_on(entity, run);
            let status = if done { USE_ACCEPTED } else { USE_REFUSED };
            let answer = gather_answer(number, request, status, name);
            send_now(&mut world.combat_messages, &connection, &[answer]);
            true
        })
        .unwrap_or(false)
    }

    /// Player `entity` spends one kit on `run` (module doc): an ammo kit on a turret that lacks
    /// ammo, else a repair kit on one that lacks health; whether it spent one. An encounter's
    /// turret whose ammo costs a status effect takes that effect instead of the ammo kit
    /// ([`Self::add_ammo_for_effect`]).
    fn spend_kit_on(&mut self, entity: Entity, run: &mut TurretRun) -> bool {
        let refill = run.kind.ammo_refill.filter(|_| run.kind.uses_ammo);
        let lacks_ammo = refill.is_some() && run.ammo < run.kind.max_ammo;
        if let Some(effect) = run.kind.ammo_effect.clone() {
            return lacks_ammo && self.add_ammo_for_effect(entity, run, &effect);
        }
        let lacks_health = run.body.health < run.body.max_health;
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return false;
        };
        let Some(gear) = player.gear.as_mut() else {
            return false;
        };
        if let Some(refill) = refill.filter(|_| lacks_ammo)
            && gear.take_as(AMMO_KIT, KIT_COST, SYNC_CRAFT_USED)
        {
            run.ammo = run.ammo.saturating_add(refill).min(run.kind.max_ammo);
            let partial = run
                .spawner_member(ProjectileSpawnerState {
                    ammo: Some(run.ammo),
                    firing_blocked: None,
                })
                .into_iter()
                .collect();
            self.sync_turret(run, partial);
            return true;
        }
        if lacks_health && gear.take_as(REPAIR_KIT, KIT_COST, SYNC_CRAFT_USED) {
            let partial = run.body.repair(run.kind.repair_health);
            self.sync_turret(run, partial);
            return true;
        }
        false
    }

    /// Player `entity` adds the ammo of `run`'s `AddAmmo` option (`m_ammoAmount`) for status
    /// effect `effect`, which it loses (module doc, **The Isle of Night ballista**): the spawner
    /// shows the ammo, and the interact the option's cooldown; `false` when the player holds no
    /// such effect.
    fn add_ammo_for_effect(&mut self, entity: Entity, run: &mut TurretRun, effect: &str) -> bool {
        let crc = nw_formats::objectstream::lowercase_crc32(effect);
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return false;
        };
        if !player.status_effects.holds(crc) {
            return false;
        }
        player.status_effects.remove(crc);
        let refill = run.kind.ammo_refill.unwrap_or_default();
        run.ammo = run.ammo.saturating_add(refill).min(run.kind.max_ammo);
        run.effect_adds = run.effect_adds.saturating_add(1);
        let cooldown = run.body.structure.interact_component.map(|component| BundleMember {
            component,
            state: MemberState::InteractCooldown(InteractCooldownState {
                interact: InteractState {
                    enabled: None,
                    has_interactors: Some(NO_INTERACTORS),
                },
                cooldown_updates: FullList {
                    version: Some(run.effect_adds),
                    elements: vec![InteractCooldown {
                        option: AMMO_ADD_OPTION,
                        ends_unix_ns: super::unix_ns(
                            self.server_ms.saturating_add(AMMO_ADD_COOLDOWN_MS),
                        ),
                    }],
                },
            }),
        });
        let partial = cooldown
            .into_iter()
            .chain(run.spawner_member(ProjectileSpawnerState {
                ammo: Some(run.ammo),
                firing_blocked: None,
            }))
            .collect();
        self.sync_turret(run, partial);
        true
    }

    /// Spawns the turret of slice `slice` that an encounter of this instance puts at `at` turned
    /// `yaw_deg` (module doc, **The Isle of Night ballista**; the dungeon script's object spawn):
    /// its retail template ([`nw_content::war_forts::WarForts::encounter_turret`]) with that place
    /// and a new `GdeRef`, of the level's side, held by the level
    /// ([`super::siege_areas::LevelSiege`]): its slot key; `None` when the table holds no turret
    /// of that slice.
    pub(crate) fn spawn_encounter_turret(
        &mut self,
        slice: &str,
        at: [f32; 3],
        yaw_deg: f32,
    ) -> Option<u64> {
        const HALF: f32 = 0.5;
        let content = std::sync::Arc::clone(&self.content);
        let (template, kind) = content.war_forts.encounter_turret(slice)?;
        let yaw = yaw_deg.to_radians();
        let members = template
            .members
            .iter()
            .map(|bytes| match BundleMember::read_from(bytes) {
                Ok((member, _))
                    if member.state.type_index() == POSITION_IN_THE_WORLD_TYPE_INDEX =>
                {
                    super::encounter_roots::place_member(member.component, at, yaw * HALF)
                        .encode()
                        .unwrap_or_else(|_| bytes.clone())
                }
                _ => bytes.clone(),
            })
            .collect();
        self.wars.level.turrets_made = self.wars.level.turrets_made.saturating_add(1);
        let turret = FortStructure {
            gde_ref: dynamic_ref(ENCOUNTER_TURRET_NAMESPACE, self.wars.level.turrets_made),
            place: at,
            yaw,
            side: LEVEL_SIDE,
            members,
            ..template.clone()
        };
        let run = self.place_turret(turret, kind.clone(), level_ownership(), false)?;
        let number = run.body.number;
        self.wars.level.turrets.push(run);
        Some(number)
    }

    /// The encounter's turret of slot key `number` goes (the dungeon script removed its object):
    /// its user is let go and its slot stops; `false` when `number` is none of them.
    pub(crate) fn remove_encounter_turret(&mut self, number: u64) -> bool {
        let Some(index) = self
            .wars
            .level
            .turrets
            .iter()
            .position(|run| run.body.number == number)
        else {
            return false;
        };
        let mut run = self.wars.level.turrets.remove(index);
        if run.user.is_some() {
            self.release_turret(&mut run);
        }
        self.stations.remove_camp(run.body.number);
        true
    }

    /// Turret `run` takes `amount` health (module doc): a hit, or its break at 0 (a user of a
    /// turret that breaks is let go first).
    pub(super) fn hurt_turret(&mut self, run: &mut TurretRun, amount: u32) {
        if run.body.breaks_at(amount) && run.user.is_some() {
            self.release_turret(run);
        }
        match run.body.take(amount) {
            Some(Hurt::Hit(partial) | Hurt::Broke(partial)) => self.sync_turret(run, partial),
            None => {}
        }
    }

    /// The war of `fight` started (module doc): its structures may be hurt, and they show the
    /// war's run in their ownership ([`Self::show_war_run_ownership`]).
    pub(super) fn turrets_at_start(&mut self, fight: &mut WarFight) {
        for run in &mut fight.turrets {
            let partial = run.body.open_to_damage();
            self.sync_turret(run, partial);
        }
        for run in &mut fight.structures {
            let partial = run.body.open_to_damage();
            self.sync_structure(run, partial);
        }
        self.show_war_run_ownership(fight);
    }

    /// The war of `fight` has its result (module doc): its structures are whole and
    /// invulnerable again, its turrets full of ammo, and their ownership leaves the war's run
    /// ([`Self::show_war_run_ownership`]).
    pub(super) fn turrets_at_result(&mut self, fight: &mut WarFight) {
        for run in &mut fight.turrets {
            if run.user.is_some() {
                self.release_turret(run);
            }
            run.ammo = full_ammo(&run.kind);
            let mut partial = run.body.restore();
            partial.extend(run.spawner_member(ProjectileSpawnerState {
                ammo: Some(run.ammo),
                firing_blocked: None,
            }));
            self.sync_turret(run, partial);
        }
        for run in &mut fight.structures {
            let partial = run.body.restore();
            self.sync_structure(run, partial);
        }
        self.show_war_run_ownership(fight);
    }

    /// The bodies and the turrets a shot from a turret of `side` that player `shooter` fires
    /// meets: the mobs, the shooter's foes and the whole turrets of the other side.
    pub(super) fn shot_targets(
        &mut self,
        shooter: u64,
        turrets: &[TurretRun],
        side: u8,
    ) -> Vec<(ShotTarget, Position, Capsule)> {
        let mut bodies = self.target_places();
        if let Some(entity) = self.player_by_number(shooter) {
            bodies.extend(self.foe_places(entity));
        }
        let structures = turrets
            .iter()
            .enumerate()
            .filter(|(_, run)| run.body.structure.side != side && !run.body.broken)
            .map(|(index, run)| {
                let [x, y, z] = run.body.structure.place;
                (
                    ShotTarget::Turret {
                        index,
                        facet: run.body.structure.facet_entity(),
                    },
                    Position { x, y, z },
                    TURRET_BODY,
                )
            });
        bodies
            .into_iter()
            .map(|(number, at, body)| (ShotTarget::Body(number), at, body))
            .chain(structures)
            .collect()
    }

    /// What a shot of a turret of `side` that player `shooter` fires meets (module doc, **Shots
    /// on structures**): the targets of [`Self::shot_targets`], the whole structures of the
    /// fight that are no turret of `turrets`, of both sides, and the war's standing capture
    /// points.
    pub(super) fn turret_shot_targets(
        &mut self,
        shooter: u64,
        turrets: &[TurretRun],
        side: u8,
    ) -> Vec<(TurretShotTarget, Position, Capsule)> {
        let mut targets: Vec<(TurretShotTarget, Position, Capsule)> = self
            .shot_targets(shooter, turrets, side)
            .into_iter()
            .map(|(target, at, body)| (TurretShotTarget::Shot(target), at, body))
            .collect();
        let structure = |number, facet, at, foe, capsule| {
            (TurretShotTarget::Structure { number, facet, foe }, at, capsule)
        };
        if let Some(fight) = self.war_fight() {
            targets.extend(
                fight
                    .structures
                    .iter()
                    .filter(|run| !run.body.broken)
                    .map(|run| {
                        let body = &run.body;
                        let foe = body.structure.side != side;
                        structure(
                            body.number,
                            body.structure.facet_entity(),
                            body.place(),
                            foe,
                            structure_body(body),
                        )
                    }),
            );
            targets.extend(fight.standing_points().map(|(facet, at)| {
                (TurretShotTarget::Obstacle { facet }, at, CAPTURE_POINT_BODY)
            }));
            return targets;
        }
        let raid = self
            .player_by_number(shooter)
            .and_then(|entity| self.outpost_rush_raid_of(entity));
        let turret_numbers: Vec<u64> = turrets.iter().map(|run| run.body.number).collect();
        if let (Some(raid), Some(running)) = (raid, self.arena_matches.outpost_rush.as_ref()) {
            targets.extend(
                running
                    .structures
                    .shot_structures(raid, &turret_numbers)
                    .into_iter()
                    .map(|(number, facet, at, foe)| structure(number, facet, at, foe, TURRET_BODY)),
            );
        }
        targets
    }

    /// The client of `connection` sent turret call `call` at `now_ms` (module doc); the hits
    /// of a repeater's shot.
    pub fn turret_call(
        &mut self,
        connection: &ConnectionRef,
        call: TurretClientMsg,
        now_ms: u64,
    ) -> Vec<Result<AttackOutcome, AttackRefusal>> {
        let Some(&entity) = self.by_connection.get(connection) else {
            return Vec::new();
        };
        let aim = call.aim();
        let shot = self.with_turret_runs(|world, turrets| {
            let Some(index) = turrets.iter().position(|run| {
                run.body.structure.gde_ref == aim.turret_id()
                    && run.user.is_some_and(|user| user.entity == entity)
            }) else {
                tracing::debug!(turret = ?aim.turret_id(), "turret call of a player who does not use it");
                return (Vec::new(), None, 0);
            };
            let mut run = turrets[index].clone();
            if let Some(user) = run.user.as_mut() {
                user.last_call_ms = now_ms;
            }
            run.aim(&aim);
            let shooter = run.user.map_or(0, |user| user.number);
            let (outcomes, structure_hit) = match call {
                TurretClientMsg::RepeaterInput { flags, .. } if run.kind.hold_to_fire => {
                    run.heat.advance(&run.kind, now_ms);
                    run.heat.held = flags.held();
                    if flags.fired() && run.repeater_may_fire(aim.time_ns) {
                        run.last_shot_ns = Some(aim.time_ns);
                        let targets =
                            world.turret_shot_targets(shooter, turrets, run.body.structure.side);
                        world.fire_repeater(&mut run, &targets, now_ms)
                    } else {
                        (Vec::new(), None)
                    }
                }
                TurretClientMsg::FireProjectile { .. }
                    if !run.kind.hold_to_fire && run.cannon_may_fire() =>
                {
                    let targets =
                        world.turret_shot_targets(shooter, turrets, run.body.structure.side);
                    world.fire_cannon(&mut run, &targets, now_ms);
                    (Vec::new(), None)
                }
                _ => (Vec::new(), None),
            };
            turrets[index] = run;
            (outcomes, structure_hit, shooter)
        });
        let Some((outcomes, structure_hit, shooter)) = shot else {
            return Vec::new();
        };
        if let Some((hit, amount)) = structure_hit {
            self.hurt_held(hit, amount, shooter);
        }
        outcomes
    }

    /// The health of a hit of a turret of `kind` that player `shooter` fires (module doc).
    fn turret_hit_amount(&mut self, kind: &TurretKind, shooter: u64) -> u32 {
        let coefficient = self
            .combat
            .data
            .table_attack(&kind.damage_table, &kind.damage_row)
            .map_or(0.0, |row| row.coefficient);
        self.level_hit_amount(shooter, coefficient)
    }

    /// The health of a hit of player `shooter` with a row of `coefficient`: the base damage of
    /// its level times the coefficient (module doc).
    fn level_hit_amount(&mut self, shooter: u64, coefficient: f64) -> u32 {
        let level = self
            .player_by_number(shooter)
            .and_then(|entity| self.world.get::<Player>(entity))
            .map_or(FIRST_LEVEL, |player| {
                self.content.progression.level_of(player.experience) + FIRST_LEVEL
            });
        let base = self
            .combat
            .data
            .level(level)
            .map_or(0.0, |row| row.base_damage);
        // A hit of a few thousand points at most.
        (base * coefficient).round() as u32
    }

    /// The siege structures of the war this sim runs that an attack of player `entity` can meet
    /// (module doc): while the war runs, the whole structures of the other side, by their slot
    /// keys, with [`TURRET_BODY`].
    /// On an Outpost Rush instance they are the outpost structures open to the player
    /// ([`super::outpost_rush_structures`], the same body; Inferred).
    pub(super) fn structure_places(&self, entity: Entity) -> Vec<(u64, Position, Capsule)> {
        let (Some(fight), Some(side)) = (self.war_fight(), self.war_side(entity)) else {
            return self
                .outpost_rush_structure_targets(entity)
                .into_iter()
                .map(|(number, place)| (number, place, TURRET_BODY))
                .collect();
        };
        fight
            .turrets
            .iter()
            .map(|run| &run.body)
            .chain(fight.structures.iter().map(|run| &run.body))
            .filter(|body| body.structure.side != side && !body.broken && !body.invulnerable)
            .map(|body| (body.number, body.place(), structure_body(body)))
            .collect()
    }

    /// A hit of player `entity` (`shooter`) with its attack `damage_id` on `target` (module
    /// doc): `None` when `target` is no siege structure of this sim's war. A structure of the
    /// player's own side, or one before the start or from the result, is no target.
    pub(super) fn structure_hit(
        &mut self,
        entity: Entity,
        shooter: u64,
        target: u64,
        damage_id: &str,
    ) -> Option<Result<AttackOutcome, AttackRefusal>> {
        let side = self.war_side(entity);
        let coefficient = self
            .combat
            .data
            .attack(damage_id)
            .map(|row| row.coefficient);
        if self.is_outpost_rush_structure(target) {
            let Some(coefficient) = coefficient else {
                return Some(Err(AttackRefusal::NotWeaponAttack));
            };
            let amount = self.level_hit_amount(shooter, coefficient);
            return Some(match self.hit_outpost_rush_structure(entity, target, amount) {
                Some(_) => Ok(AttackOutcome::Hit { target, amount }),
                None => Err(AttackRefusal::UnknownTarget),
            });
        }
        self.with_turrets(|world, fight| {
            let turret = fight
                .turrets
                .iter()
                .position(|run| run.body.number == target);
            let other = fight
                .structures
                .iter()
                .position(|run| run.body.number == target);
            if turret.is_none() && other.is_none() {
                return None;
            }
            let Some(coefficient) = coefficient else {
                return Some(Err(AttackRefusal::NotWeaponAttack));
            };
            let body = match (turret, other) {
                (Some(index), _) => &fight.turrets[index].body,
                (None, Some(index)) => &fight.structures[index].body,
                (None, None) => return None,
            };
            if side.is_none_or(|side| side == body.structure.side) || body.invulnerable {
                return Some(Err(AttackRefusal::UnknownTarget));
            }
            if body.broken {
                return Some(Err(AttackRefusal::TargetDead));
            }
            let amount = world.level_hit_amount(shooter, coefficient);
            if let Some(index) = turret {
                let mut run = fight.turrets[index].clone();
                world.hurt_turret(&mut run, amount);
                fight.turrets[index] = run;
            } else if let Some(index) = other {
                let mut run = fight.structures[index].clone();
                world.hurt_structure_run(&mut run, amount);
                fight.structures[index] = run;
            }
            Some(Ok(AttackOutcome::Hit { target, amount }))
        })
        .flatten()
    }

    /// A repeater's shot along the aim of `run` at `targets` (module doc): the 2720 to every
    /// player of the sim, the ammo and the hit: a body's outcome, or the structure of the fight
    /// it hurts with the health it takes.
    fn fire_repeater(
        &mut self,
        run: &mut TurretRun,
        bodies: &[(TurretShotTarget, Position, Capsule)],
        now_ms: u64,
    ) -> RepeaterShot {
        let Some(shooter) = run.user.map(|user| user.number) else {
            return (Vec::new(), None);
        };
        let (muzzle, line) = run.muzzle();
        let ground = std::sync::Arc::clone(&self.ground);
        let ground_m = ground_along(muzzle, line, run.kind.range_m, |x, y| {
            ground.height_at(x, y)
        });
        let body = body_on_line(muzzle, line, (run.kind.range_m, run.kind.radius_m), bodies)
            .filter(|(_, along)| ground_m.is_none_or(|ground_m| *along <= ground_m));
        let along = body.map_or(ground_m.unwrap_or(run.kind.range_m), |(_, along)| along);
        // A solid of the level on the way stops the shot: its level entity is the target, no
        // hit volume (projectile module doc, **Level stops**; `opr_que_and_lost` 54 shots).
        let level = self.level_stop(muzzle, point_on(muzzle, line, along));
        let body = body.filter(|_| level.is_none());
        let at = level.map_or_else(|| point_on(muzzle, line, along), |stop| stop.at);
        let result = CombatEffectMsg::TurretHitscanResult {
            header: run.turret_header(),
            position: [at.x, at.y, at.z],
            target: level.map_or_else(
                || body.map_or(NO_TARGET, |(target, _)| target.facet()),
                |stop| stop.entity.unwrap_or(NO_TARGET),
            ),
            result: body.map_or(NO_HIT_VOLUME, |(target, _)| target.hit_volume()),
        };
        self.send_to_all(&[ServerMessage::CombatEffect(result)]);
        if run.kind.uses_ammo {
            run.ammo = run.ammo.saturating_sub(1);
        }
        run.heat.fired_ms = now_ms;
        let mut partial: Vec<BundleMember> = run
            .spawner_member(ProjectileSpawnerState {
                ammo: Some(run.ammo),
                firing_blocked: None,
            })
            .into_iter()
            .collect();
        if run.heat.due(now_ms) {
            run.heat.mark_sent(now_ms);
            partial.push(run.turret_member(TurretState {
                heat: Some(turret_half(run.heat.value)),
                overheated: Some(run.heat.overheated),
                ..TurretState::default()
            }));
        }
        self.sync_turret(run, partial);
        let Some((target, _)) = body else {
            return (Vec::new(), None);
        };
        let amount = self.turret_hit_amount(&run.kind, shooter);
        if let TurretShotTarget::Shot(ShotTarget::Body(number)) = target {
            return (
                self.turret_body_hit(number, shooter, amount, now_ms)
                    .into_iter()
                    .collect(),
                None,
            );
        }
        (Vec::new(), target.held().map(|hit| (hit, amount)))
    }

    /// A cannon's shot along the aim of `run` at `bodies` (module doc): the ball, its 327 to the
    /// shooter, the ammo and the block.
    fn fire_cannon(
        &mut self,
        run: &mut TurretRun,
        bodies: &[(TurretShotTarget, Position, Capsule)],
        now_ms: u64,
    ) {
        let (Some(user), Some(projectile)) = (run.user, run.kind.projectile) else {
            return;
        };
        let (muzzle, line) = run.muzzle();
        let speed = velocity(line, run.kind.fire_speed_mps);
        let ground = std::sync::Arc::clone(&self.ground);
        let (flight_s, at, target) = ball_flight(
            muzzle,
            speed,
            (run.kind.gravity_mps2, run.kind.range_m, run.kind.radius_m),
            bodies,
            |x, y| ground.height_at(x, y),
        );
        // A solid of the level on the ball's way stops it (Inferred: the straight line from the
        // muzzle to its end, the flight time by the share of the way).
        let (flight_s, at, target) = match self.level_stop(muzzle, at) {
            Some(stop) => {
                let way_m = muzzle.horizontal_distance(at).max(f32::EPSILON);
                (
                    flight_s * stop.along_m / way_m,
                    stop.at,
                    stop.entity.map(|facet| TurretShotTarget::Obstacle { facet }),
                )
            }
            None => (flight_s, at, target),
        };
        let flight_ms = (flight_s * MS_PER_SECOND) as u64;
        let spawn = ProjectileSpawn {
            asset: projectile.asset,
            component: projectile.component,
            position_component: None,
            state: ProjectileState {
                velocity: Some(speed),
                owner: Some(facet_entity(user.number)),
                ranged_attack: Some(projectile.ranged_attack),
                damage_table: Some(projectile.damage_table_crc),
                damage_row: Some(projectile.row_index),
                ..ProjectileState::default()
            },
            muzzle,
            flying_ms: flight_ms,
            stuck_ms: 0,
        };
        let Some((entity, gde_ref)) = self.spawn_projectile(spawn, now_ms) else {
            return;
        };
        let spawned = CombatEffectMsg::AuthoritativeProjectile {
            header: run.spawner_header(),
            flag: AUTHORITATIVE_PROJECTILE_FLAG,
            projectile: projectile_ref(gde_ref),
        };
        let shooter = self
            .world
            .get::<Player>(user.entity)
            .map(|player| player.connection.clone());
        if let Some(connection) = &shooter {
            send_now(
                &mut self.combat_messages,
                connection,
                &[ServerMessage::CombatEffect(spawned)],
            );
        }
        let others: Vec<ConnectionRef> = self
            .by_connection
            .keys()
            .filter(|connection| Some(*connection) != shooter.as_ref())
            .cloned()
            .collect();
        let server_spawn = ServerMessage::CombatEffect(CombatEffectMsg::ProjectileServerSpawn {
            header: run.spawner_header(),
        });
        for connection in others {
            send_now(&mut self.combat_messages, &connection, std::slice::from_ref(&server_spawn));
        }
        let amount = self.turret_hit_amount(&run.kind, user.number);
        run.balls.push(BallFlight {
            entity,
            shooter: user.number,
            land_ms: now_ms.saturating_add(flight_ms),
            at,
            target,
            amount,
        });
        if run.kind.uses_ammo {
            run.ammo = run.ammo.saturating_sub(1);
        }
        run.blocked_until_ms =
            Some(now_ms.saturating_add((run.kind.fire_cooldown_s * MS_PER_SECOND) as u64));
        run.firing_blocked = true;
        let partial = run
            .spawner_member(ProjectileSpawnerState {
                ammo: Some(run.ammo),
                firing_blocked: Some(true),
            })
            .into_iter()
            .collect();
        self.sync_turret(run, partial);
    }

    /// Sends `messages` to every player of this sim.
    pub(super) fn send_to_all(&mut self, messages: &[ServerMessage]) {
        let connections: Vec<ConnectionRef> = self.by_connection.keys().cloned().collect();
        for connection in connections {
            send_now(&mut self.combat_messages, &connection, messages);
        }
    }

    /// The client of player `entity` cancelled interaction `interaction` (2473): a use of a
    /// turret with it ends, and a supply generator it opened with it closes for it.
    pub(super) fn turret_interaction_ended(&mut self, entity: Entity, interaction: u64) {
        self.generator_interaction_ended(entity, interaction);
        self.with_turret_runs(|world, turrets| {
            for run in turrets.iter_mut().filter(|run| {
                run.user
                    .is_some_and(|user| user.entity == entity && user.interaction == interaction)
            }) {
                world.release_turret(run);
            }
        });
    }

    /// The use of `run` ends (module doc): the last aim, no interactor, the block while hot.
    pub(super) fn release_turret(&mut self, run: &mut TurretRun) {
        run.user = None;
        run.heat.held = false;
        run.interaction_blocked = run.heat.value > 0.0;
        let partial: Vec<BundleMember> = run
            .interact_member()
            .into_iter()
            .chain(std::iter::once(run.turret_member(TurretState {
                yaw: Some(turret_half(run.yaw)),
                pitch: Some(turret_half(run.pitch)),
                interactor: Some(NO_INTERACTOR),
                blocked: run.interaction_blocked.then_some(true),
                ..TurretState::default()
            })))
            .collect();
        self.sync_turret(run, partial);
    }

    /// The full members of `run` that change: its turret, interact and projectile spawner states.
    fn turret_members(run: &TurretRun) -> Vec<BundleMember> {
        std::iter::once(run.turret_member(run.state()))
            .chain(run.interact_member())
            .chain(run.spawner_member(run.full_spawner()))
            .collect()
    }

    /// The outpost structure of Outpost Rush turret `run` turned to a ruin (module doc): its user
    /// is let go and its use is disabled until its rebuild.
    pub(super) fn break_outpost_turret(&mut self, run: &mut TurretRun) {
        if run.user.is_some() {
            self.release_turret(run);
        }
        run.body.broken = true;
        let partial = run.interact_member().into_iter().collect();
        self.sync_turret(run, partial);
    }

    /// The outpost of Outpost Rush turret `run` has the owner team `side` now (module doc): a
    /// user of another team is let go.
    pub(super) fn own_outpost_turret(&mut self, run: &mut TurretRun, side: u8) {
        run.body.structure.side = side;
        let user_team = run.user_number().and_then(|number| {
            self.arena_matches
                .outpost_rush
                .as_ref()
                .and_then(|running| running.team_of(number))
        });
        if run.in_use() && user_team != Some(side) {
            self.release_turret(run);
        }
    }

    /// One tick of the turrets of this sim's fight at sim ms `now_ms` (module doc): uses that
    /// timed out or whose player is gone end, the heat moves, a cannon's block ends, and the
    /// balls that landed hit; then the uses of its trigger structures and their barrels and
    /// areas ([`super::siege_areas`]), and the war's builds and generators.
    pub(super) fn tick_turrets(
        &mut self,
        now_ms: u64,
    ) -> Vec<Result<AttackOutcome, AttackRefusal>> {
        self.spawn_level_droppers();
        self.with_turrets(|world, fight| world.tick_siege_builds(fight, now_ms));
        let mut outcomes = Vec::new();
        let (structure_hits, shell_areas) = self
            .with_turret_runs(|world, turrets| {
                let mut structure_hits = Vec::new();
                let mut shell_areas = Vec::new();
                for run in turrets.iter_mut() {
                    world.tick_turret(
                        run,
                        now_ms,
                        &mut outcomes,
                        (&mut structure_hits, &mut shell_areas),
                    );
                }
                (structure_hits, shell_areas)
            })
            .unwrap_or_default();
        self.with_use_runs(|world, runs, uses| {
            for (area, at, source) in shell_areas {
                world.open_siege_area(uses, &area, at, source, now_ms, &mut outcomes);
            }
            world.tick_siege_uses(runs, uses, now_ms, &mut outcomes);
        });
        self.with_turrets(|world, fight| world.tick_generators(fight, now_ms));
        for (hit, amount, shooter) in structure_hits {
            self.hurt_held(hit, amount, shooter);
        }
        outcomes
    }

    /// One tick of `run` at `now_ms`: its use, heat and block; the balls that landed hit (the
    /// hits go into `outcomes`, a turret's place and its health into the first list), and a
    /// ball of a kind with an area opens it where it landed (the second list).
    fn tick_turret(
        &mut self,
        run: &mut TurretRun,
        now_ms: u64,
        outcomes: &mut Vec<Result<AttackOutcome, AttackRefusal>>,
        (structure_hits, shell_areas): (&mut Vec<HeldShotHit>, &mut Vec<ShellArea>),
    ) {
        if let Some(user) = run.user {
            let alive = self
                .world
                .get::<Vitals>(user.entity)
                .is_some_and(|vitals| !vitals.health.is_dead())
                && self.world.get::<Body>(user.entity).is_some();
            if !alive || now_ms.saturating_sub(user.last_call_ms) >= USE_TIMEOUT_MS {
                self.release_turret(run);
            }
        }
        let mut partial = Vec::new();
        let overheated = run.heat.overheated;
        run.heat.advance(&run.kind, now_ms);
        if run.heat.due(now_ms) || overheated != run.heat.overheated {
            run.heat.mark_sent(now_ms);
            let cooled = run.interaction_blocked && run.heat.value <= 0.0;
            if cooled {
                run.interaction_blocked = false;
            }
            partial.push(run.turret_member(TurretState {
                heat: Some(turret_half(run.heat.value)),
                overheated: (overheated != run.heat.overheated).then_some(run.heat.overheated),
                blocked: cooled.then_some(false),
                ..TurretState::default()
            }));
        }
        if run.blocked_until_ms.is_some_and(|until| now_ms >= until) {
            run.blocked_until_ms = None;
            run.firing_blocked = false;
            partial.extend(run.spawner_member(ProjectileSpawnerState {
                ammo: None,
                firing_blocked: Some(false),
            }));
        }
        if !partial.is_empty() {
            self.sync_turret(run, partial);
        }
        let (landed, flying): (Vec<_>, Vec<_>) = std::mem::take(&mut run.balls)
            .into_iter()
            .partition(|ball| now_ms >= ball.land_ms);
        run.balls = flying;
        for ball in landed {
            // Inferred: a ball that met a turret ends at its impact point (the hit volume of a
            // structure is Unknown).
            let body = match ball.target {
                Some(TurretShotTarget::Shot(ShotTarget::Body(number))) => Some(number),
                _ => None,
            };
            match ball.target {
                Some(TurretShotTarget::Obstacle { facet }) => {
                    self.shot_entity_on_level(ball.entity, Some(facet), ball.at, now_ms);
                }
                _ => self.shot_entity_impact(ball.entity, body, ball.at, now_ms),
            }
            if let Some(area) = &run.kind.area {
                let source = self.area_source(
                    run.body.structure.side,
                    ball.shooter,
                    rotation_of(&run.body.structure),
                );
                shell_areas.push((area.clone(), ball.at, source));
            }
            if let Some(number) = body {
                outcomes.extend(self.turret_body_hit(number, ball.shooter, ball.amount, now_ms));
            }
            if let Some(hit) = ball.target.and_then(TurretShotTarget::held) {
                structure_hits.push((hit, ball.amount, ball.shooter));
            }
        }
    }
}

/// The 327 form of a projectile's `GdeRef`: its raw id and its facet entity.
fn projectile_ref(gde_ref: [u8; nw_messages::wire::RAW_ID_LEN]) -> [u8; TURRET_GDE_REF_LEN] {
    gde_ref_of(gde_ref, facet_of_raw_id(&gde_ref))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use nw_bus::WarInstance;
    use nw_content::war_forts::WarForts;
    use nw_db::{RosterSeat, TerritoryWar, WarSignup};
    use nw_messages::keyed_list::KeyedList;
    use nw_messages::{BuildableControllerState, InteractState, VitalsSlot0};

    use crate::world::siege_structures::{
        BROKEN_RUIN_TYPE, BROKEN_STATE, BUILT_STATE, INVULNERABLE, VULNERABLE,
    };
    use nw_messages::turrets::{RepeaterFlags, TURRET_REPEATER_INPUT_TYPE_INDEX};
    use uuid::Uuid;

    use super::*;
    use crate::player_content::PlayerContent;
    use crate::world::tests::{NOW, SERVER_TIME, admit_at, messages_to, world_with_spawns};
    use crate::world::war_fight::war_levels;

    /// The retail repeater `fe2d8806...` of territory 11 (`joining_defending_war_2`: its first
    /// members and its slice values), a made-up cannon of the attackers with the retail cannon
    /// kind of `RS_offensive_war`, and capture point 4 with its retail position member (the war
    /// forts table of territory 11).
    pub(crate) const TABLE: &str = r#"{"forts": [{
        "territory": 11, "frame": [13312.0, 6144.0, 512.0, 512.0],
        "arrivals": [{"side": 2, "position": [13664.976, 6312.062, 50.63449]}],
        "siege": {"asset": "11111111111111111111111111111111", "gde_ref": "22222222222222222222222222222222",
                  "card_component": 0, "state_component": 2},
        "points": [{"number": 4, "asset": "33333333333333333333333333333333",
           "gde_ref": "08070605040302011111111111111111", "facet": "0a0b0c0d0e0f0001",
           "place": [13694.4, 6314.97, 57.7], "kind": 6, "position": "020d01034655f99d45c557b911c943f846",
           "card_component": 0, "position_component": 2, "ownership_component": 1, "point_component": 4}],
        "structures": [
          {"asset": "d1000d79ba715eae9e2382d252d9e6d5", "card_component": 0, "gde_ref": "fe2d8806c5c5b3da3218b9ad4b3dcb4f",
           "interact_component": 13, "members": ["010d010346561b6a45c6e19412f2037a6a",
           "020f01ff47afc80000000000000b9790c0aee250f80000000000000000ff0047afc80000003f8000005300001eaefb73015f90",
           "03b81701030000000020", "079b1b01140000000000", "0a8b22010100", "0c39010300c800", "0db22d010200000001",
           "0e921b010201", "0fb442017f00000000000066770000000000000000000000000000000000000000ffffffff0000",
           "05ba1801070500000103010b5369656765537570706c79573a918000000000003c00ffffffff00000033000000000000"],
           "ownership_component": 4, "place": [13702.854, 6364.1973, 55.413147], "side": 2, "spawner_component": 12,
           "turret_component": 15, "yaw": -0.08727877},
          {"asset": "4e41fea1158d570397ce292035800042", "card_component": 0, "gde_ref": "c8e5b15b50b29d5867767e3572dd4a63",
           "interact_component": 13, "members": ["0f390103000f00",
           "10b442017f000000000000466c0000000000000000000000000000000000000000ffffffff0000"],
           "ownership_component": 5, "place": [13650.0, 6300.0, 55.0], "side": 1, "spawner_component": 15,
           "turret_component": 16, "yaw": 0.0}]}],
      "turret_kinds": [
       {"asset": "d1000d79ba715eae9e2382d252d9e6d5", "damage_row": "RepeaterTurret_Projectile",
        "damage_table": "DamageTable_Structures", "fire_cooldown_s": 0.0, "fire_speed_mps": 1000.0, "gravity_mps2": 0.0,
        "heat_capacity": 100.0, "heat_gain_per_s": 20.0, "heat_loss_delay_s": 0.5, "heat_loss_per_s": 40.0,
        "hold_to_fire": true, "max_ammo": 200, "ammo_refill": 200, "repair_health": 5000.0, "pitch_max_deg": 45.0, "pitch_min_deg": -50.0, "projectile": null,
        "radius_m": 0.2, "range_m": 125.0, "repeat_fire_delay_s": 0.2, "slice": "turret_t2_repeater",
        "spawner_facet": {"component": "372fc35a6778d259", "xor": "06f03eba159de653"},
        "turret_facet": {"component": "36b800a37ce6d7c1", "xor": "06f03ea2159de653"}, "uses_ammo": true, "yaw_max_deg": 85.0},
       {"asset": "4e41fea1158d570397ce292035800042", "damage_row": "Cannon_Projectile",
        "damage_table": "DamageTable_Structures", "fire_cooldown_s": 3.0, "fire_speed_mps": 50.0, "gravity_mps2": 13.0,
        "heat_capacity": 0.0, "heat_gain_per_s": 0.0, "heat_loss_delay_s": 0.0, "heat_loss_per_s": 0.0,
        "hold_to_fire": false, "max_ammo": 15, "ammo_refill": 15, "repair_health": 5000.0, "pitch_max_deg": 25.0, "pitch_min_deg": -20.0,
        "projectile": {"asset": "30a2e9cadf6b5622a0571e468690f128", "component": 1, "ranged_attack": 261,
                       "damage_table_crc": 81017683, "row_index": 6},
        "radius_m": 0.3, "range_m": 115.0, "repeat_fire_delay_s": 0.0, "slice": "platform_t3_cannon_autobuild",
        "spawner_facet": {"component": "603842316ab22336", "xor": "9aaa09ee99dfb635"},
        "turret_facet": {"component": "26eb5da726214d6d", "xor": "9aaa09e499dfb635"}, "uses_ammo": true, "yaw_max_deg": 75.0}]}"#;

    pub(crate) const REPEATER_REF: [u8; 16] =
        *b"\xfe\x2d\x88\x06\xc5\xc5\xb3\xda\x32\x18\xb9\xad\x4b\x3d\xcb\x4f";
    pub(crate) const CANNON_REF: [u8; 16] =
        *b"\xc8\xe5\xb1\x5b\x50\xb2\x9d\x58\x67\x76\x7e\x35\x72\xdd\x4a\x63";
    pub(crate) const DEFENDER: u64 = 7;
    const INTERACTION: u64 = 0xbfde_0000_0000_0001;
    /// The client clock of the first shot (ns) and the retail spacing of two shots.
    const SHOT_NS: u64 = 0x0b97_927a_bd0a_dcad;
    const SHOT_SPACING_NS: u64 = 250_000_000;
    const TOO_SOON_NS: u64 = 100_000_000;

    fn forts() -> WarForts {
        WarForts::parse(TABLE).expect("table")
    }

    /// A war instance sim of territory 11 with its fight open and one defender of its roster.
    fn war_world() -> (SimWorld, Entity) {
        war_world_of(nw_content::war_forts::SIDE_DEFENDER)
    }

    /// [`war_world`] with its player on `side`.
    pub(crate) fn war_world_of(side: u8) -> (SimWorld, Entity) {
        war_world_with(forts(), side, |content| content)
    }

    /// [`war_world_of`] with the forts and kinds of `forts`, its content changed by `content`
    /// before the fight spawns.
    pub(crate) fn war_world_with(
        forts: WarForts,
        side: u8,
        content: impl FnOnce(PlayerContent) -> PlayerContent,
    ) -> (SimWorld, Entity) {
        let mut world = world_with_spawns(Vec::new());
        world.instance_sim = true;
        world.war_levels = war_levels(&forts);
        world.content = Arc::new(content(PlayerContent {
            war_forts: forts,
            kits: crate::world::gear::tests::kits_with(&[
                (AMMO_KIT, KIT_CLASS),
                (REPAIR_KIT, KIT_CLASS),
            ]),
            ..PlayerContent::default()
        }));
        let admit = admit_at(DEFENDER, 0.0, true);
        let character = admit.character;
        let war = TerritoryWar {
            war: Uuid::from_u128(0x77),
            territory: 11,
            attacker: (Uuid::from_u128(0xa1), 2),
            defender: (Uuid::from_u128(0xd1), 1),
            number: 4714,
            starts_ms: u64::MAX / 2,
            result: None,
            signups: vec![WarSignup {
                character,
                name: "Defender".to_owned(),
                company: Some((Uuid::from_u128(0xd1), 0)),
                side,
                gear_score: 600,
                roster: RosterSeat {
                    place: Some((0, 0)),
                    permission: 0,
                },
                kicked: false,
            }],
        };
        world.wars.read(Ok(vec![war]));
        let opened = world.open_war_instance(
            Uuid::from_u128(0x1),
            1,
            (vec![character], None),
            WarInstance {
                war: Uuid::from_u128(0x77),
                territory: 11,
            },
            NOW,
        );
        assert!(matches!(opened, nw_bus::InstanceOpened::Opened));
        world.admit(&admit, NOW, SERVER_TIME).expect("admit");
        let entity = world.player_by_number(DEFENDER).expect("defender");
        let gear = crate::world::gear::Gear::starter(
            &world.content.kits,
            character,
            Some(crate::world::gear::tests::BACKSTORY),
        )
        .expect("starter gear");
        world.world.get_mut::<Player>(entity).expect("player").gear = Some(gear);
        world.tick_war_fight();
        world.take_messages();
        (world, entity)
    }

    pub(crate) fn use_request(target: [u8; 16]) -> InteractionRequest {
        InteractionRequest {
            target,
            target_entity: u64::from_le_bytes(target[..8].try_into().expect("8 bytes")),
            interaction: INTERACTION,
            kind: USE_TURRET_KIND,
            gatherable: None,
        }
    }

    fn repeater_call(time_ns: u64, flags: u8) -> TurretClientMsg {
        let kind = forts().kind(&REPEATER_REF_ASSET).cloned().expect("kind");
        let entity = u64::from_le_bytes(REPEATER_REF[..8].try_into().expect("8 bytes"));
        TurretClientMsg::RepeaterInput {
            header: FacetHeader::of(0, DEFENDER),
            aim: TurretAim {
                turret: gde_ref_of(REPEATER_REF, entity),
                turret_facet: kind.turret_facet.target(entity),
                time_ns,
                yaw: 0.0,
                pitch: 0.0,
            },
            flags: RepeaterFlags(flags),
            frame_s: 1.0 / 30.0,
        }
    }

    const REPEATER_REF_ASSET: [u8; 16] =
        *b"\xd1\x00\x0d\x79\xba\x71\x5e\xae\x9e\x23\x82\xd2\x52\xd9\xe6\xd5";

    fn turret_state(world: &SimWorld, gde_ref: &[u8; 16]) -> (TurretState, Option<u32>, Vec<u8>) {
        let number = turret_number(world, gde_ref).expect("spawned");
        let members = world.stations.members(number).expect("members");
        let state = members
            .iter()
            .find_map(|member| match &member.state {
                MemberState::Turret(state) => Some(*state),
                _ => None,
            })
            .expect("turret state");
        let interactors = members.iter().find_map(|member| match &member.state {
            MemberState::Interact(state) => state.has_interactors,
            _ => None,
        });
        let spawner = members
            .iter()
            .find_map(|member| match &member.state {
                MemberState::Recorded { class, body }
                    if *class == PROJECTILE_SPAWNER_TYPE_INDEX =>
                {
                    Some(body.clone())
                }
                _ => None,
            })
            .expect("spawner");
        (state, interactors, spawner)
    }

    #[test]
    fn a_turret_is_placed_with_no_war_fight() {
        let forts = forts();
        let turret = forts
            .iter()
            .flat_map(|fort| &fort.structures)
            .find(|structure| structure.turret_component.is_some())
            .cloned()
            .expect("turret");
        let kind = forts.kind(&turret.asset).cloned().expect("kind");
        let mut world = world_with_spawns(Vec::new());
        assert!(world.war_fight().is_none());
        let owner = turret_ownership(&ClaimOwner::NONE);
        let run = world
            .place_turret(turret.clone(), kind.clone(), owner.clone(), true)
            .expect("placed");
        assert!(!run.body.invulnerable && !run.in_use());
        assert_eq!(run.ammo, kind.max_ammo, "full of ammo");
        let members = world.stations.members(run.body.number).expect("members");
        assert!(
            members
                .iter()
                .any(|member| matches!(member.state, MemberState::Turret(_))),
            "its turret state"
        );
        assert!(members.iter().any(|member| member.state == MemberState::Ownership(owner.clone())));
        let closed = world
            .place_turret(turret, kind, owner, false)
            .expect("placed");
        assert!(closed.body.invulnerable);
        assert_ne!(closed.body.number, run.body.number, "a slot key of its own");
    }

    fn turret_number(world: &SimWorld, gde_ref: &[u8; 16]) -> Option<u64> {
        world
            .war_fight()?
            .turrets
            .iter()
            .find(|run| &run.body.structure.gde_ref == gde_ref)
            .map(|run| run.body.number)
    }

    #[test]
    fn the_turrets_spawn_idle_with_their_retail_members_and_their_side_ownership() {
        let (world, _) = war_world();
        let (state, interactors, spawner) = turret_state(&world, &REPEATER_REF);
        assert_eq!(state.interactor, Some(NO_INTERACTOR));
        assert_eq!(state.heat, Some(0));
        assert_eq!(
            state.orientation.map(|turn| turn.flag),
            Some(0x66),
            "retail orientation"
        );
        assert_eq!(
            interactors,
            Some(NO_INTERACTORS),
            "retail first interact had one"
        );
        assert_eq!(
            spawner,
            ProjectileSpawnerState {
                ammo: Some(200),
                firing_blocked: Some(false)
            }
            .encode()
        );
        let number = turret_number(&world, &REPEATER_REF).expect("spawned");
        let members = world.stations.members(number).expect("members");
        assert!(members.iter().any(|member| matches!(&member.state,
            MemberState::Ownership(owner) if owner.guild_id == Some(*Uuid::from_u128(0xd1).as_bytes())
                && owner.permission_rule == Some(TURRET_PERMISSION_RULE))));
        assert!(
            members
                .iter()
                .any(|member| matches!(member.state, MemberState::Position(_)))
        );
        assert!(turret_number(&world, &CANNON_REF).is_some());
    }

    #[test]
    fn a_defender_uses_its_turret_and_the_attackers_cannon_is_refused() {
        let (mut world, entity) = war_world();
        assert!(world.turret_interaction(entity, DEFENDER, use_request(CANNON_REF), &[], NOW));
        let refused = messages_to(&world.take_messages(), DEFENDER);
        assert!(
            matches!(&refused[..], [ServerMessage::InteractionValidated(answer)] if answer.status == USE_REFUSED)
        );
        assert!(world.turret_interaction(entity, DEFENDER, use_request(REPEATER_REF), &[], NOW));
        let accepted = messages_to(&world.take_messages(), DEFENDER);
        assert!(
            matches!(&accepted[..], [ServerMessage::InteractionValidated(answer)] if answer.status == USE_ACCEPTED)
        );
        let (state, interactors, _) = turret_state(&world, &REPEATER_REF);
        assert_eq!(state.interactor, Some(interactor_of(DEFENDER)));
        assert_eq!(interactors, Some(ONE_INTERACTOR));
        assert!(!world.turret_interaction(
            entity,
            DEFENDER,
            InteractionRequest {
                kind: 51,
                ..use_request(REPEATER_REF)
            },
            &[],
            NOW
        ));
    }

    /// The ownership of the card of slot key `number`, and the last ownership of its state
    /// change that goes out.
    fn ownerships(world: &SimWorld, number: u64) -> (Option<OwnershipState>, Option<OwnershipState>) {
        let ownership = |members: Vec<BundleMember>| {
            members.into_iter().rev().find_map(|member| match member.state {
                MemberState::Ownership(state) => Some(state),
                _ => None,
            })
        };
        let card = world.stations.members(number).map(<[BundleMember]>::to_vec);
        let slot = crate::world::slots::Slot {
            interest: 1,
            new: false,
        };
        let sent = world.stations.group(number, slot).map(|group| group.members);
        (card.and_then(ownership), sent.and_then(ownership))
    }

    /// Observed (`RS_offensive_war` 858.810 s, `joining_defending_war_2` 194.609 s): a war
    /// turret's use sends the user as the owner alone; its card keeps the user, also when the
    /// side company's look comes later.
    #[test]
    fn a_war_turret_use_sends_the_user_as_its_owner_alone_and_its_card_keeps_it() {
        let (mut world, entity) = war_world();
        let number = turret_number(&world, &REPEATER_REF).expect("spawned");
        assert!(world.turret_interaction(entity, DEFENDER, use_request(REPEATER_REF), &[], NOW));
        let owner = world.owner_of(entity);
        let (card, sent) = ownerships(&world, number);
        assert_eq!(
            sent,
            Some(OwnershipState {
                owner: owner.clone(),
                ..OwnershipState::default()
            }),
            "the owner alone"
        );
        let card = card.expect("its card's ownership");
        assert_eq!(card.owner, owner, "the card names the user");
        assert_eq!(
            card.permission_rule,
            Some(TURRET_PERMISSION_RULE),
            "the rest of the card stays"
        );
        world.tick_turrets(NOW + USE_TIMEOUT_MS);
        assert_eq!(
            world.with_turret_runs(|_, turrets| turrets.iter().any(|run| run.used() && !run.in_use())),
            Some(true),
            "released and used"
        );
        world.show_side_looks(&[Uuid::from_u128(0xd1)]);
        assert_eq!(ownerships(&world, number).0.and_then(|card| card.owner), owner, "a used turret keeps its user");
    }

    /// Observed (`joining_defending_war_2` 83.729 s and 1883.719 s, `siege_structures` module
    /// doc, **The war's run**): at the start the fort's turrets get rule 4 and their side's raid
    /// id, a supply generator the zero company id with its raid id; at the result they go back
    /// to rule 3 and raid 0, the generator to raid 0. The cards follow, and a company look that
    /// comes while the war runs keeps the war's values.
    #[test]
    fn the_war_start_and_result_move_the_fort_structures_in_and_out_of_the_war_run() {
        use crate::world::siege_structures::{WAR_RUN_PERMISSION_RULE, generator_ownership};
        /// A supply generator of the fort (made-up: its ownership component alone) and its kind.
        const GENERATOR_ASSET: &str = "55245fed30cd5320a05871053318e674";
        const GENERATOR_REF: [u8; 16] = [0x46; 16];
        /// The side numbers of the test war (number 4714: the war's number and the next).
        const ATTACKER_RAID: u64 = 4_714;
        const DEFENDER_RAID: u64 = 4_715;
        const NO_RAID: u64 = 0;
        const NO_COMPANY: [u8; 16] = [0; 16];
        const RESULT_MS: u64 = 1;
        let mut table: serde_json::Value = serde_json::from_str(TABLE).expect("test table");
        table["forts"][0]["structures"].as_array_mut().expect("structures").push(serde_json::json!({
            "asset": GENERATOR_ASSET, "gde_ref": hex::encode(GENERATOR_REF), "card_component": 0,
            "interact_component": null, "spawner_component": null, "ownership_component": 4,
            "place": [13660.0, 6330.0, 55.0], "side": nw_content::war_forts::SIDE_DEFENDER,
            "yaw": 0.0, "members": []
        }));
        table["structure_kinds"] = serde_json::json!([{"asset": GENERATOR_ASSET,
            "slice": "slices/structures/siege/siege_supplygenerator_t1.dynamicslice", "template": null}]);
        let forts = WarForts::parse(&table.to_string()).expect("table");
        let (mut world, _) = war_world_with(forts, nw_content::war_forts::SIDE_DEFENDER, |content| content);
        let repeater = turret_number(&world, &REPEATER_REF).expect("repeater");
        let cannon = turret_number(&world, &CANNON_REF).expect("cannon");
        let generator = world
            .war_fight()
            .and_then(|fight| {
                fight
                    .structures
                    .iter()
                    .find(|run| run.body.structure.gde_ref == GENERATOR_REF)
            })
            .map(|run| run.body.number)
            .expect("generator");
        assert_eq!(ownerships(&world, generator).0, Some(generator_ownership()), "no company");
        let change = |rule: Option<u8>, guild: Option<[u8; 16]>, raid: u64| OwnershipState {
            guild_id: guild,
            permission_rule: rule,
            raid_id: Some(raid),
            ..OwnershipState::default()
        };
        let war = Uuid::from_u128(0x77);
        let mut read = world.wars.war(war).cloned().expect("the war");
        // The war starts now: a start 30 minutes back or more is a war that ended already.
        read.starts_ms = world.unix_ms();
        world.wars.read(Ok(vec![read.clone()]));
        world.tick_war_fight();
        let started = |rule| Some(change(rule, None, DEFENDER_RAID));
        assert_eq!(ownerships(&world, repeater).1, started(Some(WAR_RUN_PERMISSION_RULE)));
        assert_eq!(
            ownerships(&world, cannon).1,
            Some(change(Some(WAR_RUN_PERMISSION_RULE), None, ATTACKER_RAID)),
            "the attackers' raid"
        );
        assert_eq!(
            ownerships(&world, generator).1,
            Some(change(None, Some(NO_COMPANY), DEFENDER_RAID)),
            "rule 4 already"
        );
        world.show_side_looks(&[Uuid::from_u128(0xd1)]);
        let card = ownerships(&world, repeater).0.expect("card");
        assert_eq!(card.guild_id, Some(*Uuid::from_u128(0xd1).as_bytes()));
        assert_eq!(
            (card.permission_rule, card.raid_id),
            (Some(WAR_RUN_PERMISSION_RULE), Some(DEFENDER_RAID)),
            "the look keeps the war's run"
        );
        read.result = Some((read.defender.0, RESULT_MS));
        world.wars.read(Ok(vec![read]));
        world.tick_war_fight();
        assert_eq!(
            ownerships(&world, repeater).1,
            Some(change(Some(TURRET_PERMISSION_RULE), None, NO_RAID))
        );
        assert_eq!(
            ownerships(&world, generator).1,
            Some(change(None, Some(NO_COMPANY), NO_RAID))
        );
        let card = ownerships(&world, generator).0.expect("card");
        assert_eq!(card, generator_ownership(), "back to its first ownership");
    }

    #[test]
    fn a_repeater_shot_answers_2720_spends_ammo_and_keeps_its_rate() {
        let (mut world, entity) = war_world();
        world.turret_interaction(entity, DEFENDER, use_request(REPEATER_REF), &[], NOW);
        world.take_messages();
        let connection = world
            .world
            .get::<Player>(entity)
            .expect("player")
            .connection
            .clone();
        let fired = RepeaterFlags::PRESSED | RepeaterFlags::HELD | RepeaterFlags::FIRED;
        world.turret_call(&connection, repeater_call(SHOT_NS, fired), NOW + 1);
        let sent = messages_to(&world.take_messages(), DEFENDER);
        let [
            ServerMessage::CombatEffect(CombatEffectMsg::TurretHitscanResult {
                header,
                position,
                target,
                result,
            }),
        ] = &sent[..]
        else {
            panic!("one 2720: {sent:?}");
        };
        assert_eq!(
            *header,
            FacetHeader {
                target: 0x12fb_fc0a_8ffc_a36e,
                entity: 0xdab3_c5c5_0688_2dfe
            },
            "the retail 2720 header"
        );
        assert_eq!((*target, *result), (NO_TARGET, NO_HIT_VOLUME));
        let from = Position {
            x: 13_702.854,
            y: 6_364.197,
            z: 55.413 + REPEATER_MUZZLE.1,
        };
        let at = Position {
            x: position[0],
            y: position[1],
            z: position[2],
        };
        assert!(
            (from.horizontal_distance(at) - 125.0).abs() < 0.5,
            "no ground: the range end"
        );
        let (_, _, spawner) = turret_state(&world, &REPEATER_REF);
        assert_eq!(
            spawner,
            ProjectileSpawnerState {
                ammo: Some(199),
                firing_blocked: Some(false)
            }
            .encode()
        );
        world.turret_call(
            &connection,
            repeater_call(SHOT_NS + TOO_SOON_NS, fired),
            NOW + 2,
        );
        assert!(
            messages_to(&world.take_messages(), DEFENDER).is_empty(),
            "too soon"
        );
        world.turret_call(
            &connection,
            repeater_call(SHOT_NS + SHOT_SPACING_NS, fired),
            NOW + 3,
        );
        assert_eq!(messages_to(&world.take_messages(), DEFENDER).len(), 1);
        let _ = TURRET_REPEATER_INPUT_TYPE_INDEX;
    }

    #[test]
    fn a_use_ends_after_the_calls_stop_and_a_hot_turret_stays_blocked_until_cool() {
        let (mut world, entity) = war_world();
        world.turret_interaction(entity, DEFENDER, use_request(REPEATER_REF), &[], NOW);
        let connection = world
            .world
            .get::<Player>(entity)
            .expect("player")
            .connection
            .clone();
        let held = RepeaterFlags::HELD | RepeaterFlags::FIRED;
        world.turret_call(&connection, repeater_call(SHOT_NS, held), NOW);
        world.turret_call(
            &connection,
            repeater_call(SHOT_NS + SHOT_SPACING_NS, held),
            NOW + 1_000,
        );
        world.tick_turrets(NOW + 1_000 + USE_TIMEOUT_MS - 1);
        assert_eq!(
            turret_state(&world, &REPEATER_REF).0.interactor,
            Some(interactor_of(DEFENDER))
        );
        world.tick_turrets(NOW + 1_000 + USE_TIMEOUT_MS);
        let (state, interactors, _) = turret_state(&world, &REPEATER_REF);
        assert_eq!(state.interactor, Some(NO_INTERACTOR));
        assert_eq!(interactors, Some(NO_INTERACTORS));
        assert_eq!(state.blocked, Some(true), "hot");
        assert!(world.turret_interaction(
            entity,
            DEFENDER,
            use_request(REPEATER_REF),
            &[],
            NOW + 1_200
        ));
        let refused = messages_to(&world.take_messages(), DEFENDER);
        assert!(refused.iter().any(|message| matches!(message, ServerMessage::InteractionValidated(answer) if answer.status == USE_REFUSED)));
        world.tick_turrets(NOW + 10_000);
        let (state, _, _) = turret_state(&world, &REPEATER_REF);
        assert_eq!(
            (state.heat, state.blocked),
            (Some(0), Some(false)),
            "cooled"
        );
    }

    pub(crate) const CANNON_ASSET: [u8; 16] =
        *b"\x4e\x41\xfe\xa1\x15\x8d\x57\x03\x97\xce\x29\x20\x35\x80\x00\x42";

    pub(crate) fn cannon_call() -> TurretClientMsg {
        let kind = kind(&CANNON_ASSET);
        let entity = u64::from_le_bytes(CANNON_REF[..8].try_into().expect("8 bytes"));
        TurretClientMsg::FireProjectile {
            header: FacetHeader::of(0, DEFENDER),
            aim: TurretAim {
                turret: gde_ref_of(CANNON_REF, entity),
                turret_facet: kind.turret_facet.target(entity),
                time_ns: SHOT_NS,
                yaw: 0.0,
                pitch: 0.2,
            },
            tail: nw_messages::turrets::FIRE_TAIL,
        }
    }

    #[test]
    fn a_cannon_shot_spawns_its_ball_tells_the_shooter_and_blocks_for_its_cooldown() {
        let (mut world, entity) = war_world_of(nw_content::war_forts::SIDE_ATTACKER);
        assert!(world.turret_interaction(entity, DEFENDER, use_request(CANNON_REF), &[], NOW));
        world.take_messages();
        let connection = world
            .world
            .get::<Player>(entity)
            .expect("player")
            .connection
            .clone();
        world.turret_call(&connection, cannon_call(), NOW + 10);
        let sent = messages_to(&world.take_messages(), DEFENDER);
        let [
            ServerMessage::CombatEffect(CombatEffectMsg::AuthoritativeProjectile {
                header,
                flag,
                projectile,
            }),
        ] = &sent[..]
        else {
            panic!("one 327: {sent:?}");
        };
        assert_eq!(
            header.target, 0x226f_fdf0_2d20_7733,
            "the retail 327 target"
        );
        assert_eq!(*flag, AUTHORITATIVE_PROJECTILE_FLAG);
        let ball: [u8; 16] = projectile[..16].try_into().expect("16 bytes");
        assert_eq!(projectile[16..], facet_of_raw_id(&ball).to_be_bytes());
        let (_, _, spawner) = turret_state(&world, &CANNON_REF);
        assert_eq!(
            spawner,
            ProjectileSpawnerState {
                ammo: Some(14),
                firing_blocked: Some(true)
            }
            .encode()
        );
        world.turret_call(&connection, cannon_call(), NOW + 20);
        assert!(
            messages_to(&world.take_messages(), DEFENDER).is_empty(),
            "blocked"
        );
        let flying = |world: &SimWorld| {
            world
                .war_fight()
                .expect("fight")
                .turrets
                .iter()
                .map(|run| run.balls.len())
                .sum::<usize>()
        };
        assert_eq!(flying(&world), 1);
        world.tick_turrets(NOW + 10 + 3_000);
        let (_, _, spawner) = turret_state(&world, &CANNON_REF);
        assert_eq!(
            spawner,
            ProjectileSpawnerState {
                ammo: Some(14),
                firing_blocked: Some(false)
            }
            .encode()
        );
        assert_eq!(flying(&world), 0, "landed at the end of its range");
    }

    fn kind(asset: &[u8; 16]) -> TurretKind {
        forts().kind(asset).cloned().expect("kind")
    }

    /// The client build of the content table the slice tests read (`NW_CONTENT_DIR`).
    pub(crate) const CONTENT_BUILD: &str = "25810808";

    /// The test table with the cannon's kind replaced by the kind of the content table whose
    /// slice ends with `slice` (on the cannon's asset, so that the cannon of the test fort is of
    /// that kind).
    pub(crate) fn forts_with_cannon_of(slice: &str) -> WarForts {
        let dir = std::env::var("NW_CONTENT_DIR").expect("NW_CONTENT_DIR");
        let path = nw_content::war_forts::war_forts_path(std::path::Path::new(&dir), CONTENT_BUILD);
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("content table"))
                .expect("json");
        let mut kind = content["turret_kinds"]
            .as_array()
            .expect("kinds")
            .iter()
            .find(|kind| {
                kind["slice"]
                    .as_str()
                    .is_some_and(|name| name.ends_with(slice))
            })
            .cloned()
            .expect("kind of the slice");
        let mut table: serde_json::Value = serde_json::from_str(TABLE).expect("test table");
        let kinds = table["turret_kinds"].as_array_mut().expect("kinds");
        let cannon = kinds
            .iter_mut()
            .find(|row| row["asset"] == hex::encode(CANNON_ASSET))
            .expect("cannon kind");
        kind["asset"] = cannon["asset"].clone();
        *cannon = kind;
        WarForts::parse(&table.to_string()).expect("table")
    }

    /// The slices of the T3 ballista and explosive launcher with the projectile slice each
    /// fires (`m_ammoId`: `turret_t1_ballista_projectile`, `turret_t3_explosivelauncher_projectile`).
    const SHOT_SLICES: [(&str, &str); 2] = [
        (
            "turret_t3_ballista.dynamicslice",
            "047a2b6174ae57e49a703b1f72516490",
        ),
        (
            "turret_t3_explosivelauncher.dynamicslice",
            "14546b51be8c508f9a7b3768a3761944",
        ),
    ];

    #[test]
    fn a_ballista_and_an_explosive_launcher_fire_the_projectile_of_their_slices_as_a_cannon() {
        for (slice, shot_asset) in SHOT_SLICES {
            let forts = forts_with_cannon_of(slice);
            let kind = forts.kind(&CANNON_ASSET).cloned().expect("kind");
            let projectile = kind.projectile.expect("a projectile from the slices");
            assert_eq!(hex::encode(projectile.asset), shot_asset, "{slice}");
            let (mut world, entity) =
                war_world_with(forts, nw_content::war_forts::SIDE_ATTACKER, |content| {
                    content
                });
            assert!(world.turret_interaction(entity, DEFENDER, use_request(CANNON_REF), &[], NOW));
            world.take_messages();
            let connection = world
                .world
                .get::<Player>(entity)
                .expect("player")
                .connection
                .clone();
            world.turret_call(&connection, cannon_call(), NOW + 10);
            let sent = messages_to(&world.take_messages(), DEFENDER);
            assert!(
                matches!(
                    &sent[..],
                    [ServerMessage::CombatEffect(
                        CombatEffectMsg::AuthoritativeProjectile { .. }
                    )]
                ),
                "{slice}: one 327: {sent:?}"
            );
            let fight = world.war_fight().expect("fight");
            let ball = fight
                .turrets
                .iter()
                .find_map(|run| run.balls.first())
                .expect("a ball in flight");
            let members = world
                .stations
                .members(ball.entity)
                .expect("projectile entity");
            let card = members.iter().find_map(|member| match &member.state {
                MemberState::GdeMetadata(card) => card.asset,
                _ => None,
            });
            assert_eq!(
                card.map(|asset| hex::encode(asset.id)).as_deref(),
                Some(shot_asset)
            );
            let state = members
                .iter()
                .find(|member| member.component == projectile.component)
                .and_then(|member| match &member.state {
                    MemberState::Projectile(state) => Some(*state),
                    _ => None,
                })
                .expect("projectile state on the slice's component");
            assert_eq!(
                state.ranged_attack,
                Some(projectile.ranged_attack),
                "{slice}"
            );
            assert_eq!(state.damage_row, Some(projectile.row_index), "{slice}");
            let (_, _, spawner) = turret_state(&world, &CANNON_REF);
            assert_eq!(
                spawner,
                ProjectileSpawnerState {
                    ammo: Some(kind.max_ammo - 1),
                    firing_blocked: Some(true)
                }
                .encode(),
                "{slice}"
            );
        }
    }

    #[test]
    fn the_heat_rises_while_held_overheats_and_falls_after_its_delay() {
        let kind = kind(&REPEATER_REF_ASSET);
        let mut heat = Heat {
            held: true,
            ..Heat::default()
        };
        heat.advance(&kind, 1_000);
        assert!((heat.value - 20.0).abs() < 1e-3, "20 a second");
        assert!(
            !heat.due(1_000 + HEAT_RISE_SEND_MS - 1 - 1_000 + heat.sent_ms) || heat.sent_ms == 0
        );
        heat.advance(&kind, 6_000);
        assert!(heat.overheated && (heat.value - 100.0).abs() < 1e-3);
        heat.held = false;
        heat.fired_ms = 6_000;
        heat.advance(&kind, 6_400);
        assert!((heat.value - 100.0).abs() < 1e-3, "within the loss delay");
        heat.advance(&kind, 7_400);
        assert!((heat.value - 60.0).abs() < 1e-3, "40 a second");
        heat.advance(&kind, 9_000);
        assert!(heat.value == 0.0 && !heat.overheated);
    }

    #[test]
    fn a_rising_heat_goes_out_every_half_second_and_a_falling_one_at_once() {
        let mut heat = Heat {
            value: 5.0,
            ..Heat::default()
        };
        assert!(!heat.due(HEAT_RISE_SEND_MS - 1));
        assert!(heat.due(HEAT_RISE_SEND_MS));
        heat.mark_sent(HEAT_RISE_SEND_MS);
        heat.value = 4.0;
        assert!(heat.due(HEAT_RISE_SEND_MS + 1));
    }

    /// Retail `joining_defending_war_2` 197.937 s: aim yaw 78.1 and pitch -11.1 degrees from the
    /// repeater facing -5 degrees met the ground 80.4 m away, at 43.0 m.
    #[test]
    fn a_shot_meets_the_ground_where_the_retail_one_did() {
        let from = Position {
            x: 0.0,
            y: 0.0,
            z: 55.413 + REPEATER_MUZZLE.1,
        };
        let line = AimLine {
            yaw: (78.1_f32 - 5.0).to_radians(),
            pitch: 11.1_f32.to_radians(),
        };
        let along = ground_along(from, line, 125.0, |_, _| Some(43.0)).expect("the ground");
        let at = point_on(from, line, along);
        assert!(
            (from.horizontal_distance(at) - 80.4).abs() < 1.5,
            "{}",
            from.horizontal_distance(at)
        );
        let heading = (-at.x).atan2(at.y).to_degrees();
        assert!((heading - 73.1).abs() < 0.1);
        assert!(
            ground_along(
                from,
                AimLine {
                    pitch: -0.2,
                    ..line
                },
                125.0,
                |_, _| Some(43.0)
            )
            .is_none()
        );
    }

    #[test]
    fn a_shot_meets_the_nearest_body_on_its_line_only() {
        let from = Position {
            x: 0.0,
            y: 0.0,
            z: 2.0,
        };
        let line = AimLine {
            yaw: 0.0,
            pitch: 0.0,
        };
        let capsule = Capsule {
            radius_m: 0.4,
            height_m: 1.8,
        };
        let bodies = [
            (
                1,
                Position {
                    x: 0.0,
                    y: 40.0,
                    z: 1.2,
                },
                capsule,
            ),
            (
                2,
                Position {
                    x: 0.0,
                    y: 20.0,
                    z: 1.2,
                },
                capsule,
            ),
            (
                3,
                Position {
                    x: 3.0,
                    y: 10.0,
                    z: 1.2,
                },
                capsule,
            ),
            (
                4,
                Position {
                    x: 0.0,
                    y: -10.0,
                    z: 1.2,
                },
                capsule,
            ),
        ];
        let (number, along) = body_on_line(from, line, (125.0, 0.2), &bodies).expect("a body");
        assert_eq!(number, 2);
        assert!((along - 20.0).abs() < 0.1);
        assert!(
            body_on_line(from, line, (15.0, 0.2), &bodies).is_none(),
            "out of range"
        );
    }

    /// Retail `RS_offensive_war` 860.037 s: the first ball (horizontal velocity 20.4, -42.1 m/s
    /// from 58.2 m) landed 1.7 m higher after 1.546 s of flight with the slice's gravity 13 m/s²,
    /// so it rose at 11.15 m/s.
    #[test]
    fn a_ball_flies_with_the_slice_gravity_to_the_retail_impact() {
        let from = Position {
            x: 0.0,
            y: 0.0,
            z: 58.2,
        };
        // The ground under the turret is low; the ball came down on ground 1.7 m above its spawn.
        let ground = |x: f32, y: f32| Some(if x.hypot(y) < 50.0 { 50.0 } else { 59.9 });
        let (time_s, at, target) =
            ball_flight::<u64>(from, [20.4, -42.1, 11.15], (13.0, 115.0, 0.3), &[], ground);
        assert!(target.is_none());
        assert!((time_s - 1.546).abs() < 0.03, "{time_s}");
        assert!((at.z - 59.9).abs() < 0.5);
        let capsule = Capsule {
            radius_m: 0.4,
            height_m: 1.8,
        };
        let body = [(
            9,
            Position {
                x: 20.4 * 0.5,
                y: -42.1 * 0.5,
                z: 58.2 + 11.15 * 0.5 - 6.5 * 0.25 - 0.9,
            },
            capsule,
        )];
        let (_, _, met) = ball_flight(
            from,
            [20.4, -42.1, 11.15],
            (13.0, 115.0, 0.3),
            &body,
            |_, _| None,
        );
        assert_eq!(met, Some(9));
    }

    #[test]
    fn the_aim_stays_in_the_slice_limits_and_the_headers_follow_the_facet_rules() {
        let forts = forts();
        let turret = forts.fort(11).expect("fort").structures[1].clone();
        let kind = kind(&turret.asset);
        let mut run = TurretRun::new(turret, kind).expect("a turret");
        assert_eq!(
            run.turret_header(),
            FacetHeader {
                target: 0xe923_195b_e88f_a16a,
                entity: 0x589d_b250_5bb1_e5c8
            }
        );
        assert_eq!(
            run.spawner_header(),
            FacetHeader {
                target: 0x226f_fdf0_2d20_7733,
                entity: 0x589d_b250_5bb1_e5c8
            }
        );
        let aim = TurretAim {
            turret: [0; TURRET_GDE_REF_LEN],
            turret_facet: 0,
            time_ns: 0,
            yaw: 3.0,
            pitch: -1.0,
        };
        run.aim(&aim);
        assert!((run.yaw - 75_f32.to_radians()).abs() < 1e-6);
        assert!((run.pitch - (-20_f32).to_radians()).abs() < 1e-6);
        let (muzzle, line) = run.muzzle();
        assert!((muzzle.z - (55.0 + CANNON_MUZZLE.1)).abs() < 1e-3);
        assert!(
            (line.pitch - 20_f32.to_radians()).abs() < 1e-6,
            "the player convention looks down"
        );
        assert!(run.cannon_may_fire());
        run.firing_blocked = true;
        assert!(!run.cannon_may_fire());
        run.firing_blocked = false;
        run.ammo = 0;
        assert!(!run.cannon_may_fire());
    }

    #[test]
    fn the_no_interactor_and_the_interactor_are_the_retail_forms() {
        let mut none = vec![0; 20];
        none.extend([0xff; 4]);
        assert_eq!(NO_INTERACTOR.to_vec(), none);
        let interactor = interactor_of(DEFENDER);
        assert_eq!(interactor[16..], facet_entity(DEFENDER).to_be_bytes());
        assert_eq!(
            projectile_ref(REPEATER_REF)[16..],
            0xdab3_c5c5_0688_2dfe_u64.to_be_bytes()
        );
    }
    /// The item class of the kits (their `MasterItemDefinitions_Faction` rows).
    pub(crate) const KIT_CLASS: &str = "Resource+SiegeWarOnly";
    /// The retail repeater's health (its first vitals state) and its committed `SiegeSupply`
    /// (its first buildable state, `joining_defending_war_2`).
    const REPEATER_HEALTH: f32 = 90_000.0;
    const REPEATER_SUPPLY: u32 = 60;
    /// The ammo of the retail repeater before its second refill (`joining_defending_war_2`
    /// 1416 s).
    const LOW_AMMO: u16 = 130;
    const HIT: u32 = 10_000;

    pub(crate) fn give(world: &mut SimWorld, entity: Entity, item_id: &str, quantity: u32) {
        let item = world.content.kits.item(item_id).cloned().expect("kit item");
        let mut player = world.world.get_mut::<Player>(entity).expect("player");
        assert!(player.gear.as_mut().expect("gear").add(&item, quantity));
    }

    pub(crate) fn bag_count(world: &SimWorld, entity: Entity, item_id: &str) -> u32 {
        world
            .world
            .get::<Player>(entity)
            .and_then(|player| player.gear.as_ref())
            .map_or(0, |gear| gear.bag_count(item_id))
    }

    fn item_cost_request(target: [u8; 16]) -> InteractionRequest {
        InteractionRequest {
            kind: ITEM_COST_KIND,
            ..use_request(target)
        }
    }

    pub(crate) fn answer_status(world: &mut SimWorld) -> Option<u8> {
        messages_to(&world.take_messages(), DEFENDER)
            .iter()
            .find_map(|message| match message {
                ServerMessage::InteractionValidated(answer) => Some(answer.status),
                _ => None,
            })
    }

    fn repeater(world: &SimWorld) -> &TurretRun {
        world
            .war_fight()
            .expect("fight")
            .turrets
            .iter()
            .find(|run| run.body.structure.gde_ref == REPEATER_REF)
            .expect("repeater")
    }

    fn with_repeater(world: &mut SimWorld, change: impl FnOnce(&mut SimWorld, &mut TurretRun)) {
        world.with_turrets(|world, fight| {
            let run = fight
                .turrets
                .iter_mut()
                .find(|run| run.body.structure.gde_ref == REPEATER_REF)
                .expect("repeater");
            change(world, run);
        });
    }

    fn vitals_of(world: &SimWorld) -> VitalsSlot0 {
        let number = turret_number(world, &REPEATER_REF).expect("spawned");
        world
            .stations
            .members(number)
            .expect("members")
            .iter()
            .find_map(|member| match &member.state {
                MemberState::Vitals(state) => state.slot_0,
                _ => None,
            })
            .expect("vitals")
    }

    fn buildable_of(world: &SimWorld) -> BuildableControllerState {
        let number = turret_number(world, &REPEATER_REF).expect("spawned");
        world
            .stations
            .members(number)
            .expect("members")
            .iter()
            .find_map(|member| match &member.state {
                MemberState::BuildableController(state) => Some(state.clone()),
                _ => None,
            })
            .expect("buildable")
    }

    fn supply_shown(state: &BuildableControllerState) -> Vec<u32> {
        match &state.committed_resources {
            Some(KeyedList::Full(list)) => list
                .elements
                .iter()
                .map(|(_, descriptor)| descriptor.quantity)
                .collect(),
            other => panic!("a full list: {other:?}"),
        }
    }

    #[test]
    fn a_refill_spends_one_ammo_kit_and_adds_the_slice_ammo_up_to_the_most() {
        let (mut world, entity) = war_world();
        give(&mut world, entity, AMMO_KIT, 2);
        with_repeater(&mut world, |_, run| run.ammo = LOW_AMMO);
        assert!(world.turret_interaction(
            entity,
            DEFENDER,
            item_cost_request(REPEATER_REF),
            &[],
            NOW
        ));
        assert_eq!(answer_status(&mut world), Some(USE_ACCEPTED));
        assert_eq!(repeater(&world).ammo, 200, "130 + 200, at most 200");
        assert_eq!(bag_count(&world, entity, AMMO_KIT), 1);
        let (_, _, spawner) = turret_state(&world, &REPEATER_REF);
        assert_eq!(
            spawner,
            ProjectileSpawnerState {
                ammo: Some(200),
                firing_blocked: Some(false)
            }
            .encode()
        );
        assert!(world.turret_interaction(
            entity,
            DEFENDER,
            item_cost_request(REPEATER_REF),
            &[],
            NOW
        ));
        assert_eq!(
            answer_status(&mut world),
            Some(USE_REFUSED),
            "full and whole: nothing to do"
        );
        assert_eq!(bag_count(&world, entity, AMMO_KIT), 1);
    }

    #[test]
    fn a_refill_or_repair_of_the_other_side_or_without_a_kit_is_refused() {
        let (mut world, entity) = war_world();
        with_repeater(&mut world, |_, run| run.ammo = LOW_AMMO);
        world.turret_interaction(entity, DEFENDER, item_cost_request(REPEATER_REF), &[], NOW);
        assert_eq!(answer_status(&mut world), Some(USE_REFUSED), "no kit");
        give(&mut world, entity, AMMO_KIT, 1);
        world.turret_interaction(entity, DEFENDER, item_cost_request(CANNON_REF), &[], NOW);
        assert_eq!(
            answer_status(&mut world),
            Some(USE_REFUSED),
            "the attackers' cannon"
        );
        assert_eq!(bag_count(&world, entity, AMMO_KIT), 1);
        assert!(!world.turret_interaction(
            entity,
            DEFENDER,
            item_cost_request([0x42; 16]),
            &[],
            NOW
        ));
    }

    #[test]
    fn a_repair_spends_one_repair_kit_and_adds_the_slice_health() {
        let (mut world, entity) = war_world();
        give(&mut world, entity, REPAIR_KIT, 1);
        with_repeater(&mut world, |world, run| {
            run.body.invulnerable = false;
            world.hurt_turret(run, HIT);
        });
        world.turret_interaction(entity, DEFENDER, item_cost_request(REPEATER_REF), &[], NOW);
        assert_eq!(answer_status(&mut world), Some(USE_ACCEPTED));
        let health = REPEATER_HEALTH - HIT as f32 + 5_000.0;
        assert!((repeater(&world).body.health - health).abs() < 1e-3);
        assert_eq!(vitals_of(&world).health_amount, Some(health));
        assert_eq!(bag_count(&world, entity, REPAIR_KIT), 0);
        let share = (REPEATER_SUPPLY as f32 * health / REPEATER_HEALTH).ceil() as u32;
        assert_eq!(supply_shown(&buildable_of(&world)), vec![share]);
    }

    #[test]
    fn a_turret_is_hurt_only_in_the_war_and_its_supply_follows_its_health() {
        let (mut world, _) = war_world();
        let first = buildable_of(&world);
        assert_eq!(
            supply_shown(&first),
            vec![REPEATER_SUPPLY],
            "the retail first state"
        );
        assert_eq!(vitals_of(&world).invulnerability, Some(INVULNERABLE));
        with_repeater(&mut world, |world, run| world.hurt_turret(run, HIT));
        assert!(
            (repeater(&world).body.health - REPEATER_HEALTH).abs() < 1e-3,
            "before the start"
        );
        let mut fight = world
            .instance_run
            .as_mut()
            .and_then(|run| run.take_war_fight())
            .expect("fight");
        world.turrets_at_start(&mut fight);
        if let Some(run) = world.instance_run.as_mut() {
            run.put_war_fight(fight);
        }
        assert_eq!(vitals_of(&world).invulnerability, Some(VULNERABLE));
        world.take_messages();
        with_repeater(&mut world, |world, run| world.hurt_turret(run, HIT));
        let vitals = vitals_of(&world);
        assert_eq!(vitals.health_amount, Some(REPEATER_HEALTH - HIT as f32));
        assert_eq!(
            supply_shown(&buildable_of(&world)),
            vec![54],
            "60 x 80,000 / 90,000 rounded up"
        );
        let version = |state: &BuildableControllerState| match &state.committed_resources {
            Some(KeyedList::Full(list)) => list.version,
            _ => None,
        };
        assert_eq!(
            version(&buildable_of(&world)),
            version(&first).map(|v| v + 1)
        );
    }

    #[test]
    fn a_turret_breaks_at_no_health_and_is_whole_again_at_the_result() {
        let (mut world, entity) = war_world();
        world.turret_interaction(entity, DEFENDER, use_request(REPEATER_REF), &[], NOW);
        with_repeater(&mut world, |world, run| {
            run.body.invulnerable = false;
            world.hurt_turret(run, u32::MAX);
        });
        let run = repeater(&world);
        assert!(
            run.body.broken && run.user.is_none(),
            "broken, its user let go"
        );
        let vitals = vitals_of(&world);
        assert_eq!(vitals.health_amount, Some(0.0));
        assert_eq!(
            vitals.vitals_data.map(|data| data.head),
            Some(1),
            "the dead head"
        );
        let state = buildable_of(&world);
        assert_eq!(
            (state.current_state, state.ruin_damage_type),
            (Some(BROKEN_STATE), Some(BROKEN_RUIN_TYPE))
        );
        assert!(supply_shown(&state).is_empty());
        let number = turret_number(&world, &REPEATER_REF).expect("spawned");
        assert!(
            world
                .stations
                .members(number)
                .expect("members")
                .iter()
                .any(|member| {
                    matches!(
                        member.state,
                        MemberState::Interact(InteractState {
                            enabled: Some(false),
                            ..
                        })
                    )
                })
        );
        world.take_messages();
        world.turret_interaction(entity, DEFENDER, use_request(REPEATER_REF), &[], NOW + 1);
        assert_eq!(
            answer_status(&mut world),
            Some(USE_REFUSED),
            "a broken turret"
        );
        let mut fight = world
            .instance_run
            .as_mut()
            .and_then(|run| run.take_war_fight())
            .expect("fight");
        world.turrets_at_result(&mut fight);
        if let Some(run) = world.instance_run.as_mut() {
            run.put_war_fight(fight);
        }
        let run = repeater(&world);
        assert!(!run.body.broken && run.body.invulnerable);
        let vitals = vitals_of(&world);
        assert_eq!(vitals.health_amount, Some(REPEATER_HEALTH));
        assert_eq!(vitals.vitals_data.map(|data| data.head), Some(0));
        assert_eq!(vitals.invulnerability, Some(INVULNERABLE));
        let state = buildable_of(&world);
        assert_eq!(state.current_state, Some(BUILT_STATE));
        assert_eq!(supply_shown(&state), vec![REPEATER_SUPPLY]);
    }

    #[test]
    fn a_shot_meets_the_whole_turrets_of_the_other_side_only() {
        let (mut world, _) = war_world();
        let turrets = world.war_fight().expect("fight").turrets.clone();
        let targets = world.shot_targets(DEFENDER, &turrets, nw_content::war_forts::SIDE_DEFENDER);
        let cannon_facet = u64::from_le_bytes(CANNON_REF[..8].try_into().expect("8 bytes"));
        assert!(targets.iter().any(|(target, _, _)| *target
            == ShotTarget::Turret {
                index: 1,
                facet: cannon_facet
            }));
        assert!(
            !targets
                .iter()
                .any(|(target, _, _)| matches!(target, ShotTarget::Turret { index: 0, .. })),
            "not its own side"
        );
        let mut broken = turrets;
        broken[1].body.broken = true;
        assert!(
            world
                .shot_targets(DEFENDER, &broken, nw_content::war_forts::SIDE_DEFENDER)
                .iter()
                .all(|(target, _, _)| matches!(target, ShotTarget::Body(_))),
            "not a broken one"
        );
        let from = Position {
            x: 13_650.0,
            y: 6_280.0,
            z: 55.0 + TURRET_BODY.height_m * HALF,
        };
        let line = AimLine {
            yaw: 0.0,
            pitch: 0.0,
        };
        let structure = [(
            ShotTarget::Turret {
                index: 1,
                facet: cannon_facet,
            },
            Position {
                x: 13_650.0,
                y: 6_300.0,
                z: 55.0,
            },
            TURRET_BODY,
        )];
        let (met, along) = body_on_line(from, line, (125.0, 0.2), &structure).expect("met");
        assert_eq!(met.facet(), cannon_facet, "the 2720 target of a turret hit");
        assert_eq!(met.hit_volume(), NO_HIT_VOLUME, "a turret has no hit volume");
        assert!((along - 20.0).abs() < 0.1);
    }

    /// Observed (`joining_defending_war_2` 1282.655 and 1282.887 s): two repeater shots met
    /// capture point 2 of territory 11, each 2720 naming the point's key with no hit volume; a
    /// point has no health. The points stand from the war's start.
    #[test]
    fn a_turret_shot_meets_a_standing_capture_point_unharmed() {
        use nw_content::war_forts::SIDE_DEFENDER;
        /// The key of the table's point: its `GdeRef`'s first eight bytes, little-endian.
        const POINT_KEY: u64 = 0x0102_0304_0506_0708;
        let (mut world, _) = war_world();
        let turrets = world.war_fight().expect("fight").turrets.clone();
        let point = |targets: Vec<(TurretShotTarget, Position, Capsule)>| {
            targets
                .into_iter()
                .find(|(target, _, _)| matches!(target, TurretShotTarget::Obstacle { .. }))
        };
        assert_eq!(
            point(world.turret_shot_targets(DEFENDER, &turrets, SIDE_DEFENDER)),
            None,
            "no point stands before the start"
        );
        let mut war = world.wars.war(Uuid::from_u128(0x77)).cloned().expect("the war");
        war.starts_ms = world.unix_ms();
        world.wars.read(Ok(vec![war]));
        world.tick_war_fight();
        let (target, at, body) =
            point(world.turret_shot_targets(DEFENDER, &turrets, SIDE_DEFENDER)).expect("the point");
        assert_eq!(target.facet(), POINT_KEY, "the 2720 target: the point's key");
        assert_eq!(target.hit_volume(), NO_HIT_VOLUME);
        assert_eq!(target.held(), None, "a point takes no harm");
        assert_eq!((at.x, at.y, at.z), (13_694.4, 6_314.97, 57.7), "its place");
        assert_eq!(body, CAPTURE_POINT_BODY);
    }

    #[test]
    fn a_body_hit_names_the_first_hit_volume_and_a_turret_hit_none() {
        assert_eq!(ShotTarget::Body(DEFENDER).hit_volume(), BODY_HIT_VOLUME);
        assert_eq!(BODY_HIT_VOLUME, 0, "retail's most common index");
        assert_eq!(
            ShotTarget::Turret {
                index: 0,
                facet: NO_TARGET
            }
            .hit_volume(),
            NO_HIT_VOLUME
        );
        assert_eq!(NO_HIT_VOLUME, 0xff, "the client's none");
    }

    /// Module doc, **The Isle of Night ballista**: an encounter's ballista spawns on the level
    /// holder at its place with its template; with no bolt effect an ammo add is refused, with
    /// one it takes the effect for one bolt and shows the add's 1 s cooldown on option 1.
    #[test]
    fn an_encounter_ballista_adds_a_bolt_for_the_bolt_effect() {
        use nw_content::status_effects::{EffectSourceRow, StatusEffectRules, StatusEffectSource};
        const ION: [u8; 16] = *b"\xf6\x8a\x9e\x17\x8a\xb3\x52\x69\xaa\x84\x07\x0e\x17\x87\x74\x43";
        const BOLT: &str = "Raid_IoN_Balista_Bolt";
        const BOLT_S: f64 = 600.0;
        const POTENCY: f64 = 1.0;
        let table = r#"{"forts": [],
          "encounter_turrets": [{"asset": "f68a9e178ab35269aa84070e17877443", "gde_ref": "ea26269102231a28c9bba7d9f6e172d0",
            "card_component": 0, "turret_component": 18, "interact_component": 16, "spawner_component": 20,
            "ownership_component": 4, "members": ["020d010346561b6a45c6e19412f2037a6a", "14390103000100"]}],
          "turret_kinds": [{"asset": "f68a9e178ab35269aa84070e17877443", "slice": "slices/characters/expeditions/isleofnight/ion_ballista.dynamicslice",
            "hold_to_fire": false, "repeat_fire_delay_s": 0.0, "fire_speed_mps": 50.0, "yaw_max_deg": 75.0,
            "pitch_min_deg": -20.0, "pitch_max_deg": 25.0, "heat_capacity": 0.0, "heat_gain_per_s": 0.0,
            "heat_loss_per_s": 0.0, "heat_loss_delay_s": 0.0, "fire_cooldown_s": 3.0, "gravity_mps2": 13.0,
            "uses_ammo": true, "max_ammo": 1, "ammo_refill": 1, "repair_health": 5000.0,
            "damage_table": "DamageTable_Structures", "damage_row": "Ballista_Projectile", "range_m": 115.0, "radius_m": 0.3,
            "turret_facet": {"component": "0000000000000001", "xor": "0000000000000002"},
            "spawner_facet": {"component": "0000000000000003", "xor": "0000000000000004"},
            "projectile": {"asset": "047a2b6174ae57e49a703b1f72516490", "component": 1, "ranged_attack": 261,
                           "damage_table_crc": 81017683, "row_index": 6},
            "ammo_effect": "Raid_IoN_Balista_Bolt"}]}"#;
        let mut world = world_with_spawns(Vec::new());
        world.content = Arc::new(PlayerContent {
            war_forts: WarForts::parse(table).expect("table"),
            status_effects: StatusEffectRules::build(StatusEffectSource {
                effects: vec![EffectSourceRow {
                    status_id: BOLT.to_owned(),
                    base_duration_s: Some(BOLT_S),
                    stack_max: Some(1),
                    ..EffectSourceRow::default()
                }],
                ..StatusEffectSource::default()
            }),
            ..PlayerContent::default()
        });
        let admit = admit_at(DEFENDER, 0.0, true);
        world.admit(&admit, NOW, SERVER_TIME).expect("admit");
        let entity = world.player_by_number(DEFENDER).expect("player");
        const SLICE: &str = "slices/characters/expeditions/isleofnight/ion_ballista.dynamicslice";
        let number = world.spawn_encounter_turret(SLICE, [5.0, 0.0, 0.0], 0.0).expect("spawned");
        assert!(world.spawn_encounter_turret("slices/none.dynamicslice", [5.0, 0.0, 0.0], 0.0).is_none(), "no such turret");
        let ion_ref = world.wars.level.turrets[0].body.structure.gde_ref;
        assert_eq!(world.wars.level.turrets[0].body.structure.asset, ION);
        let run = &world.wars.level.turrets[0];
        assert_eq!((run.ammo, run.body.structure.side), (1, LEVEL_SIDE), "one bolt");
        world.wars.level.turrets[0].ammo = 0;
        let add = InteractionRequest {
            kind: ITEM_COST_KIND,
            ..use_request(ion_ref)
        };
        assert!(world.turret_interaction(entity, DEFENDER, add, &[], NOW));
        assert_eq!(world.wars.level.turrets[0].ammo, 0, "no bolt effect: refused");
        let content = Arc::clone(&world.content);
        let crc = nw_formats::objectstream::lowercase_crc32(BOLT);
        {
            let mut player = world.world.get_mut::<Player>(entity).expect("player");
            assert!(player.status_effects.add_by_id(&content.status_effects, BOLT, POTENCY, DEFENDER, NOW));
        }
        assert!(world.turret_interaction(entity, DEFENDER, add, &[], NOW));
        assert_eq!(world.wars.level.turrets[0].ammo, 1, "one bolt added");
        assert!(!world.world.get::<Player>(entity).expect("player").status_effects.holds(crc), "the effect is spent");
        let slot = crate::world::slots::Slot { interest: 1, new: false };
        let sent = world.stations.group(number, slot).map(|group| group.members).unwrap_or_default();
        assert!(sent.iter().any(|member| matches!(&member.state,
            MemberState::InteractCooldown(state) if state.cooldown_updates.elements.iter().any(|entry| entry.option == AMMO_ADD_OPTION))));
        assert!(!world.remove_encounter_turret(number.saturating_add(1)), "not a turret");
        assert!(world.remove_encounter_turret(number));
        assert!(world.wars.level.turrets.is_empty() && world.stations.members(number).is_none());
    }
}
