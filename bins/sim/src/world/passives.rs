// SPDX-License-Identifier: AGPL-3.0-or-later
//! The passive abilities that combat events fire on players ([`nw_content::passive_abilities`]):
//! a player's hit on a mob (`OnHit`, `OnCrit`, `OnKill`; on a mob whose block stops it,
//! `OnBlockedHit`, and `OnTargetBlockBreak` when it breaks the mob's guard), a mob's hit on a
//! player (`OnHitTaken`), its block (`OnBlockedHitTaken`) and its guard break (`OnBlockBreak`).
//!
//! A player holds the abilities of its mastery points, worn perks and attribute thresholds
//! ([`super::abilities::held_abilities`]), and the helpers of those
//! ([`nw_content::passive_abilities::PassiveAbility::helps`]). An event fires each held ability whose trigger it
//! is, whose required abilities the player also holds, whose `ActivationCooldown` ran out and
//! whose conditions the event meets.
//!
//! Two passes, both **Inferred** (no retail capture of a passive): before the hit lands, the
//! fired abilities' hit changes (`BaseDamage`, `BaseDamageReduction`, `CritChance`,
//! `CritDamage`) change it, with the state before the hit; after it lands, the fired abilities'
//! other effects run with the state after it, and their cooldowns start. A stackable ability
//! counts each held copy up to its `IsStackableMax`: its numbers grow with the copies.
//!
//! The other side of an event is a mob or, in PvP, a player of this sim: its view, its status
//! effects and its distance are read the same way, and an ability's effect on the other side
//! goes on that player.
//!
//! Not run: the passives of mobs (`AIAbilityTable`), a knockback of a player (Unknown), and a
//! hit on a mob of another sim gives no effect to that mob.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy_ecs::entity::Entity;
use nw_content::combat::CombatData;
use nw_content::passive_abilities::{
    Condition, CooldownScope, Effect, EffectMatch, PassiveAbilities, PassiveAbility, Side, Trigger,
    Vital,
};
use nw_content::status_effects::{RowKey, StatusEffectRules};
use nw_formats::objectstream::lowercase_crc32;
use nw_sim_core::EquipLoad;

use super::abilities::held_abilities;
use super::attack::Vitals;
use super::gear::Gear;
use super::status_effects::StatusEffects;
use super::{Body, Player, SimWorld};
use crate::mobs::spawn_index;
use crate::player_content::PlayerContent;

/// The vitals category of every player (`VitalsCategories` of `Player`), lowercase.
const PLAYER_CATEGORY: &str = "player";
/// The potency of an effect a passive applies.
const PASSIVE_POTENCY: f64 = 1.0;
/// The copies of a helper ability: one with its helped ability.
const ONE_HELPER: u32 = 1;
/// An `ExcludeFromGameModes` entry that names every mutated instance.
const MUTATION_MODE: &str = "Mutation";

/// One side of a combat event, as the engine reads it.
#[derive(Debug, Clone)]
pub(super) struct SideView {
    /// Its entity number.
    pub(super) number: u64,
    /// Its health as a share of its max.
    pub(super) health: f64,
    /// Its stamina as a share of its max; `None` for a mob.
    pub(super) stamina: Option<f64>,
    /// Its mana as a share of its max; `None` for a mob.
    pub(super) mana: Option<f64>,
    /// Its vitals categories, lowercase.
    pub(super) categories: Vec<String>,
}

/// One combat event, as the engine reads it.
#[derive(Debug, Clone)]
pub(super) struct PassiveEvent<'a> {
    /// The triggers it is.
    pub(super) triggers: &'a [Trigger],
    /// The hit's damage row.
    pub(super) row: Option<RowKey>,
    /// The hit dealt damage (`RequireReaction`).
    pub(super) dealt: bool,
    /// The distance between the two sides.
    pub(super) distance_m: f32,
    /// The ability's holder.
    pub(super) holder: SideView,
    /// The other side; `None` for a mob of another sim (its conditions do not hold).
    pub(super) other: Option<SideView>,
    /// The stance tags of the holder's weapon.
    pub(super) weapon_tags: &'a [String],
    /// The holder's equip load.
    pub(super) equip_load: Option<EquipLoad>,
    /// The game mode of this sim's instance; `None` in the open world.
    pub(super) game_mode: Option<&'a str>,
    /// This sim's instance is mutated.
    pub(super) mutated: bool,
}

/// When each passive ability of a player may fire again.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct PassiveReady {
    /// Server time from which each ability (lowercase id) fires again.
    ready_ms: HashMap<String, u64>,
}

impl PassiveReady {
    fn ready(&self, id: &str, now_ms: u64) -> bool {
        self.ready_ms
            .get(&id.to_lowercase())
            .is_none_or(|ready| *ready <= now_ms)
    }

    fn fired(&mut self, ability: &PassiveAbility, now_ms: u64) {
        if ability.cooldown_ms > 0 {
            self.ready_ms
                .insert(ability.id.to_lowercase(), now_ms + ability.cooldown_ms);
        }
    }
}

/// A fired ability and the copies of it that count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Fired<'a> {
    pub(super) ability: &'a PassiveAbility,
    pub(super) copies: u32,
}

/// Where a hit met its target: on the head (a ranged head shot), from behind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct HitAim {
    pub(super) headshot: bool,
    pub(super) behind: bool,
}

impl HitAim {
    /// The passive triggers of a hit so aimed, besides `OnHit`.
    pub(super) fn triggers(self) -> impl Iterator<Item = Trigger> {
        [
            self.headshot.then_some(Trigger::HeadShot),
            self.behind.then_some(Trigger::HitBehind),
        ]
        .into_iter()
        .flatten()
    }
}

/// What the fired abilities change in a hit.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct HitMods {
    /// A share added to the holder's hit.
    pub(super) damage_share: f64,
    /// A share taken from a hit on the holder.
    pub(super) taken_share: f64,
    /// Added to the holder's crit chance.
    pub(super) crit_chance: f64,
    /// Added to the holder's crit multiplier.
    pub(super) crit_damage: f64,
    /// A share added to the holder's head shot.
    pub(super) headshot_share: f64,
    /// A share added to the holder's hit from behind.
    pub(super) behind_share: f64,
}

impl HitMods {
    /// The changes of the hit-changing effects of `fired`.
    pub(super) fn of(fired: &[Fired<'_>]) -> Self {
        let mut mods = Self::default();
        for Fired { ability, copies } in fired {
            let copies = f64::from(*copies);
            for effect in &ability.effects {
                match effect {
                    Effect::DamageShare(share) => mods.damage_share += share * copies,
                    Effect::TakenShare(share) => mods.taken_share += share * copies,
                    Effect::CritChance(chance) => mods.crit_chance += chance * copies,
                    Effect::CritDamage(damage) => mods.crit_damage += damage * copies,
                    Effect::HeadshotShare(share) => mods.headshot_share += share * copies,
                    Effect::BehindShare(share) => mods.behind_share += share * copies,
                    _ => {}
                }
            }
        }
        mods
    }

    /// The share these abilities add to the holder's hit aimed as `aim`: its head shot and
    /// from-behind shares count only on such a hit. It goes into the additive sum of the hit's
    /// modifiers ([`nw_sim_core::player_hit`]; Observed: retail's attribute threshold abilities
    /// and base damage perks add there, as a crit does).
    pub(super) fn dealt_share_for(self, aim: HitAim) -> f64 {
        let headshot = if aim.headshot {
            self.headshot_share
        } else {
            0.0
        };
        let behind = if aim.behind { self.behind_share } else { 0.0 };
        self.damage_share + headshot + behind
    }

    /// The multiple of a hit on the holder.
    pub(super) fn taken_scale(self) -> f64 {
        (1.0 - self.taken_share).max(0.0)
    }
}

/// The abilities of `held` (ids, a copy per entry) that `event` fires at `now_ms` by `book`,
/// with the effects of each side read from `effects` (holder, other) by `rules`.
pub(super) fn fired<'a>(
    book: &'a PassiveAbilities,
    held: &[&str],
    ready: &PassiveReady,
    event: &PassiveEvent<'_>,
    (rules, effects): (
        &StatusEffectRules,
        (Option<&StatusEffects>, Option<&StatusEffects>),
    ),
    now_ms: u64,
) -> Vec<Fired<'a>> {
    let held_set: HashSet<String> = held.iter().map(|id| id.to_lowercase()).collect();
    let mut copies: HashMap<String, u32> = HashMap::new();
    for id in held {
        *copies.entry(id.to_lowercase()).or_default() += 1;
    }
    // A helper ability comes with the ability it helps.
    let helpers: Vec<(String, u32)> = held_set
        .iter()
        .flat_map(|id| book.helpers_of(id))
        .map(|helper| (helper.id.to_lowercase(), ONE_HELPER))
        .collect();
    for (id, count) in helpers {
        copies.entry(id).or_insert(count);
    }
    let mut ids: Vec<&String> = copies.keys().collect();
    ids.sort();
    ids.into_iter()
        .filter_map(|id| {
            let ability = book.get(id)?;
            let fires = ability.fires()
                && event
                    .triggers
                    .iter()
                    .any(|trigger| ability.fired_by(*trigger))
                && ready.ready(&ability.id, now_ms)
                && ability
                    .requires
                    .iter()
                    .all(|required| held_set.contains(&required.to_lowercase()))
                && ability
                    .conditions
                    .iter()
                    .all(|condition| holds(condition, event, book, (rules, effects)));
            fires.then(|| Fired {
                ability,
                copies: copies[id].min(ability.stack_max),
            })
        })
        .collect()
}

/// Whether `event` meets `condition`.
fn holds(
    condition: &Condition,
    event: &PassiveEvent<'_>,
    book: &PassiveAbilities,
    (rules, (holder_effects, other_effects)): (
        &StatusEffectRules,
        (Option<&StatusEffects>, Option<&StatusEffects>),
    ),
) -> bool {
    let traits = event.row.and_then(|row| book.hit(row));
    let named = |names: &[String], name: Option<&str>| {
        name.is_some_and(|name| names.iter().any(|entry| entry.eq_ignore_ascii_case(name)))
    };
    let side = |side: Side| match side {
        Side::Holder => (Some(&event.holder), holder_effects),
        Side::Other => (event.other.as_ref(), other_effects),
    };
    match condition {
        Condition::WeaponTag(tag) => event
            .weapon_tags
            .iter()
            .any(|own| own.eq_ignore_ascii_case(tag)),
        Condition::AttackTypes(types) => named(
            types,
            traits.and_then(|traits| traits.attack_type.as_deref()),
        ),
        Condition::NotAttackTypes(types) => !named(
            types,
            traits.and_then(|traits| traits.attack_type.as_deref()),
        ),
        Condition::DamageRows(rows) => event.row.is_some_and(|row| rows.contains(&row.row)),
        Condition::Melee => traits.is_some_and(|traits| !traits.ranged),
        Condition::Ranged => traits.is_some_and(|traits| traits.ranged),
        Condition::DamageTypes(types) => named(
            types,
            traits.and_then(|traits| traits.damage_type.as_deref()),
        ),
        Condition::Vital {
            side: which,
            vital,
            compare,
            share,
        } => {
            let (view, _) = side(*which);
            let value = view.and_then(|view| match vital {
                Vital::Health => Some(view.health),
                Vital::Stamina => view.stamina,
                Vital::Mana => view.mana,
            });
            value.is_some_and(|value| compare.holds(value, *share))
        }
        Condition::Effects {
            side: which,
            matched,
            stacks,
            owned,
        } => {
            let (_, effects) = side(*which);
            let source = owned.then_some(event.holder.number);
            let held = effects.map_or(0, |effects| effects.stacks_of(rules, matched, source));
            stacks.holds(held)
        }
        Condition::LacksEffects(ids) => holder_effects.is_none_or(|effects| {
            effects.stacks_of(rules, &EffectMatch::Ids(ids.clone()), None) == 0
        }),
        Condition::Category {
            side: which,
            category,
        } => {
            let (view, _) = side(*which);
            view.is_some_and(|view| {
                view.categories
                    .iter()
                    .any(|own| own.eq_ignore_ascii_case(category))
            })
        }
        Condition::Distance { compare, metres } => {
            compare.holds(f64::from(event.distance_m), *metres)
        }
        Condition::Reaction => event.dealt,
        Condition::NotInGameModes(modes) => !modes.iter().any(|mode| {
            (event.mutated && mode.eq_ignore_ascii_case(MUTATION_MODE))
                || event.game_mode.is_some_and(|own| {
                    own.eq_ignore_ascii_case(mode)
                        || own.to_lowercase().starts_with(&mode.to_lowercase())
                })
        }),
        Condition::EquipLoads(loads) => event
            .equip_load
            .is_some_and(|load| named(loads, Some(equip_load_name(load)))),
    }
}

/// The `EquipLoadCategory` name of `load`.
const fn equip_load_name(load: EquipLoad) -> &'static str {
    match load {
        EquipLoad::Fast => "Fast",
        EquipLoad::Normal => "Normal",
        EquipLoad::Slow => "Slow",
        EquipLoad::Overburdened => "Overburdened",
    }
}

/// The CRC-32s of the lowercase ability ids of the cooldowns of the ability lists of weapon item
/// `item`.
fn weapon_cooldowns(data: &CombatData, item: &str) -> HashSet<u32> {
    data.ability_lists_of_item(item)
        .into_iter()
        .flat_map(|list| list.cooldowns_ms.keys())
        .map(|id| lowercase_crc32(id))
        .collect()
}

/// What a player's side of an event is: its view, its abilities (borrowed from the player and
/// the content), its weapon's tags and its equip load.
struct PlayerFacts<'a> {
    view: SideView,
    abilities: Vec<&'a str>,
    weapon_tags: Vec<String>,
    equip_load: Option<EquipLoad>,
}

impl SimWorld {
    /// The view of player `entity`, its abilities by `content` and its weapon's tags.
    fn player_facts<'a>(
        &'a self,
        entity: Entity,
        content: &'a PlayerContent,
    ) -> Option<PlayerFacts<'a>> {
        let player = self.world.get::<Player>(entity)?;
        let vitals = self.world.get::<Vitals>(entity)?;
        let share = |current: f64, max: f64| if max > 0.0 { current / max } else { 0.0 };
        let view = SideView {
            number: player.number,
            health: share(
                f64::from(vitals.health.current()),
                f64::from(vitals.health.max()),
            ),
            stamina: Some(player.stamina.share()),
            mana: Some(share(
                f64::from(player.abilities.mana.current),
                f64::from(player.abilities.mana.max),
            )),
            categories: vec![PLAYER_CATEGORY.to_owned()],
        };
        let abilities = held_abilities(
            (
                &player.masteries,
                player.gear.as_ref(),
                &player.attributes,
                &player.status_effects,
            ),
            content,
        );
        let weapon_tags = player
            .gear
            .as_ref()
            .and_then(Gear::main_weapon)
            .and_then(|(item, _)| super::abilities::weapon_tags(&self.combat.data, item))
            .unwrap_or_default();
        Some(PlayerFacts {
            view,
            abilities,
            weapon_tags,
            equip_load: player.status_effects.equip_load(),
        })
    }

    /// The view of mob `mob` of this sim.
    fn mob_view(&self, mob: u64) -> Option<SideView> {
        let live = self.mobs.get(mob)?;
        let max = f64::from(live.health.max());
        let categories = self
            .mobs
            .vitals_of(mob)
            .and_then(|vitals| self.content.kill_types.of(vitals))
            .map_or_else(Vec::new, |(_, categories)| {
                categories
                    .iter()
                    .map(|category| category.to_lowercase())
                    .collect()
            });
        Some(SideView {
            number: mob,
            health: if max > 0.0 {
                f64::from(live.health.current()) / max
            } else {
                0.0
            },
            stamina: None,
            mana: None,
            categories,
        })
    }

    /// The player of this sim with entity number `number`.
    fn player_entity(&self, number: u64) -> Option<Entity> {
        self.by_connection.values().copied().find(|entity| {
            self.world
                .get::<Player>(*entity)
                .is_some_and(|player| player.number == number)
        })
    }

    /// The view of the other side `other` of an event: a mob of this sim, or a player.
    fn other_view(&self, other: u64, content: &PlayerContent) -> Option<SideView> {
        if spawn_index(other).is_some() {
            return self.mob_view(other);
        }
        let entity = self.player_entity(other)?;
        self.player_facts(entity, content).map(|facts| facts.view)
    }

    /// The status effects the other side `other` of an event holds.
    fn other_effects(&self, other: u64) -> Option<&StatusEffects> {
        if spawn_index(other).is_some() {
            return self.mob_effects.get(&other);
        }
        let entity = self.player_entity(other)?;
        self.world
            .get::<Player>(entity)
            .map(|player| &player.status_effects)
    }

    /// The distance in the ground plane between player `entity` and the other side `other` of
    /// an event (Inferred: the data's distances are a few metres, as reaches are).
    fn distance_to_other(&self, entity: Entity, other: u64) -> f32 {
        let place = |entity: Entity| {
            self.world
                .get::<Body>(entity)
                .map(|body| body.motion.position())
        };
        let other_at = if spawn_index(other).is_some() {
            self.mobs.get(other).map(|live| live.position)
        } else {
            self.player_entity(other).and_then(place)
        };
        place(entity)
            .zip(other_at)
            .map_or(f32::MAX, |(at, other_at)| at.horizontal_distance(other_at))
    }

    /// The abilities of player `entity` that `triggers` of a hit between it and the other side
    /// `mob` (a mob, or a player in PvP; of damage row `row`; `dealt` whether it dealt damage)
    /// fire at `now_ms`, by `content`.
    fn fired_on_player<'a>(
        &self,
        content: &'a PlayerContent,
        (entity, mob): (Entity, u64),
        triggers: &[Trigger],
        (row, dealt): (Option<RowKey>, bool),
        now_ms: u64,
    ) -> Vec<Fired<'a>> {
        let Some(facts) = self.player_facts(entity, content) else {
            return Vec::new();
        };
        if facts.abilities.is_empty() {
            return Vec::new();
        }
        let other = self.other_view(mob, content);
        let Some(player) = self.world.get::<Player>(entity) else {
            return Vec::new();
        };
        let event = PassiveEvent {
            triggers,
            row,
            dealt,
            distance_m: self.distance_to_other(entity, mob),
            holder: facts.view,
            other,
            weapon_tags: &facts.weapon_tags,
            equip_load: facts.equip_load,
            game_mode: self.instance_run.as_ref().map(|run| run.game_mode()),
            mutated: self.instance_mutation().is_some(),
        };
        fired(
            &content.passive_abilities,
            &facts.abilities,
            &player.passives,
            &event,
            (
                &content.status_effects,
                (Some(&player.status_effects), self.other_effects(mob)),
            ),
            now_ms,
        )
    }

    /// What the passives of player `entity` change in its hit on mob `mob` (damage row `row`)
    /// before it lands at `now_ms`.
    pub(super) fn passive_hit_mods(
        &self,
        entity: Entity,
        mob: u64,
        triggers: &[Trigger],
        row: Option<RowKey>,
        now_ms: u64,
    ) -> HitMods {
        let content = Arc::clone(&self.content);
        HitMods::of(&self.fired_on_player(&content, (entity, mob), triggers, (row, true), now_ms))
    }

    /// Runs the passives of player `entity` that `triggers` of a landed hit between it and mob
    /// `mob` fire at `now_ms`: the hit was of damage row `row` and dealt `amount` (0: none). Their
    /// effects other than the hit's changes run, and their cooldowns start.
    pub(super) fn run_passives(
        &mut self,
        (entity, mob): (Entity, u64),
        triggers: &[Trigger],
        (row, amount): (Option<RowKey>, u32),
        now_ms: u64,
    ) {
        let content = Arc::clone(&self.content);
        let fired =
            self.fired_on_player(&content, (entity, mob), triggers, (row, amount > 0), now_ms);
        if fired.is_empty() {
            return;
        }
        let rules = &content.status_effects;
        let server_ms = self.server_ms;
        let data = Arc::clone(&self.combat.data);
        let weapon_cooldowns = self
            .world
            .get::<Player>(entity)
            .and_then(|player| {
                player
                    .gear
                    .as_ref()
                    .and_then(Gear::main_weapon)
                    .map(|(item, _)| weapon_cooldowns(&data, item))
            })
            .unwrap_or_default();
        let Some(number) = self.world.get::<Player>(entity).map(|player| player.number) else {
            return;
        };
        let by_holder = triggers.iter().any(|trigger| trigger.by_holder());
        let mut knockback_m = 0.0;
        for Fired { ability, copies } in &fired {
            let copies_f = f64::from(*copies);
            for effect in &ability.effects {
                match effect {
                    Effect::Knockback(metres) => {
                        if by_holder {
                            knockback_m += metres * copies_f;
                        }
                    }
                    Effect::Apply {
                        side: Side::Holder,
                        ids,
                    } => {
                        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                            for id in ids {
                                player.status_effects.add_by_id(
                                    rules,
                                    id,
                                    PASSIVE_POTENCY,
                                    number,
                                    server_ms,
                                );
                            }
                        }
                    }
                    Effect::Apply {
                        side: Side::Other,
                        ids,
                    } => {
                        let effects = if spawn_index(mob).is_some() {
                            self.mobs
                                .get(mob)
                                .is_some()
                                .then(|| self.mob_effects.entry(mob).or_default())
                        } else {
                            self.player_entity(mob)
                                .and_then(|other| self.world.get_mut::<Player>(other))
                                .map(|player| &mut player.into_inner().status_effects)
                        };
                        let Some(effects) = effects else {
                            continue;
                        };
                        for id in ids {
                            effects.add_by_id(rules, id, PASSIVE_POTENCY, number, server_ms);
                        }
                    }
                    Effect::RemoveOwn(ids) => {
                        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                            for id in ids {
                                player.status_effects.remove(lowercase_crc32(id));
                            }
                        }
                    }
                    Effect::LifeSteal(share) => {
                        if let Some(mut vitals) = self.world.get_mut::<Vitals>(entity)
                            && !vitals.health.is_dead()
                        {
                            // A share of a hit of a few thousand points.
                            let healed = (f64::from(amount) * share * copies_f).round() as u32;
                            vitals.health.heal(healed);
                        }
                    }
                    Effect::ManaShare(share) => {
                        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                            let max = player.abilities.mana.max;
                            // A share of a mana max of about a hundred.
                            player.abilities.mana.add(max * (share * copies_f) as f32);
                        }
                    }
                    Effect::Stamina(points) => {
                        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                            // A few dozen stamina points.
                            player.stamina.gain((points * copies_f) as f32);
                        }
                    }
                    Effect::Cooldown { scope, seconds } => {
                        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                            let cut = |ability: u32| match scope {
                                CooldownScope::Ability(id) => ability == lowercase_crc32(id),
                                CooldownScope::ActiveWeapon => weapon_cooldowns.contains(&ability),
                                CooldownScope::AllExcept(id) => {
                                    weapon_cooldowns.contains(&ability)
                                        && ability != lowercase_crc32(id)
                                }
                            };
                            player.abilities.cut_cooldowns(
                                cut,
                                seconds * copies_f,
                                (now_ms, server_ms),
                            );
                        }
                    }
                    Effect::DamageShare(_)
                    | Effect::TakenShare(_)
                    | Effect::CritChance(_)
                    | Effect::CritDamage(_)
                    | Effect::HeadshotShare(_)
                    | Effect::BehindShare(_) => {}
                }
            }
            if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                player.passives.fired(ability, now_ms);
            }
        }
        if knockback_m > 0.0 && spawn_index(mob).is_some() {
            self.knock_back_mob(entity, mob, knockback_m);
        }
    }

    /// Mob `mob` goes `metres` back from player `entity`, stopped by the first wall on its way
    /// ([`crate::mobs::Mobs::knock_back`]).
    fn knock_back_mob(&mut self, entity: Entity, mob: u64, metres: f64) {
        let Some(from) = self
            .world
            .get::<Body>(entity)
            .map(|body| body.motion.position())
        else {
            return;
        };
        // A knockback of a few metres.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a knockback of a few metres"
        )]
        let landed = self
            .mobs
            .knock_back(mob, from, metres as f32, &*self.mob_ground);
        tracing::debug!(mob, metres, ?landed, "mob knocked back");
    }
}

#[cfg(test)]
mod tests {
    use nw_content::generated::AbilityData;

    use super::*;

    const NOW: u64 = 10_000;
    const EXECUTE: &str = "Passive_Test_Execute";
    const PARENT: &str = "Passive_Test_Parent";
    const HELPER: &str = "Passive_Test_Parent_Helper";
    const KILL: &str = "Passive_Test_OnKill";

    fn row(fields: serde_json::Value) -> AbilityData {
        serde_json::from_value(fields).expect("row")
    }

    /// An execute (more damage on a target below 30 %, stackable twice, a 2 s cooldown), a
    /// parent and its helper, and a kill passive that needs the parent.
    fn book() -> PassiveAbilities {
        let rows = [
            row(serde_json::json!({
                "ability_id": EXECUTE,
                "on_hit": true,
                "base_damage": 0.1,
                "target_comparison_type": "LessThan",
                "target_health_percent": 30,
                "is_stackable_ability": true,
                "is_stackable_max": 2,
                "activation_cooldown": 2.0
            })),
            row(serde_json::json!({
                "ability_id": PARENT,
                "on_hit_taken": true,
                "stamina": 5
            })),
            row(serde_json::json!({
                "ability_id": HELPER,
                "on_hit_taken": true,
                "required_equipped_ability_id": PARENT,
                "mana": 0.05
            })),
            row(serde_json::json!({
                "ability_id": KILL,
                "on_kill": true,
                "required_ability_id": PARENT,
                "dmg_pct_to_health": 0.1
            })),
        ];
        PassiveAbilities::build(&rows, [])
    }

    fn side(number: u64, health: f64) -> SideView {
        SideView {
            number,
            health,
            stamina: Some(1.0),
            mana: Some(1.0),
            categories: Vec::new(),
        }
    }

    fn event(triggers: &[Trigger], other_health: f64) -> PassiveEvent<'_> {
        PassiveEvent {
            triggers,
            row: None,
            dealt: true,
            distance_m: 1.0,
            holder: side(1, 1.0),
            other: Some(side(2, other_health)),
            weapon_tags: &[],
            equip_load: None,
            game_mode: None,
            mutated: false,
        }
    }

    fn fired_ids(
        book: &PassiveAbilities,
        held: &[&str],
        ready: &PassiveReady,
        event: &PassiveEvent<'_>,
    ) -> Vec<(String, u32)> {
        fired(
            book,
            held,
            ready,
            event,
            (&StatusEffectRules::default(), (None, None)),
            NOW,
        )
        .into_iter()
        .map(|fired| (fired.ability.id.clone(), fired.copies))
        .collect()
    }

    #[test]
    fn a_hit_fires_the_held_ability_whose_condition_holds_with_its_copies_capped() {
        let book = book();
        let ready = PassiveReady::default();
        let held = [EXECUTE, EXECUTE, EXECUTE];
        assert!(
            fired_ids(&book, &held, &ready, &event(&[Trigger::Hit], 0.5)).is_empty(),
            "a target above 30 %"
        );
        assert_eq!(
            fired_ids(&book, &held, &ready, &event(&[Trigger::Hit], 0.2)),
            [(EXECUTE.to_owned(), 2)],
            "three copies, at most two count"
        );
        assert!(
            fired_ids(&book, &[], &ready, &event(&[Trigger::Hit], 0.2)).is_empty(),
            "not held"
        );
        assert!(
            fired_ids(&book, &held, &ready, &event(&[Trigger::HitTaken], 0.2)).is_empty(),
            "another trigger"
        );
    }

    #[test]
    fn a_fired_ability_waits_its_activation_cooldown() {
        let book = book();
        let mut ready = PassiveReady::default();
        let execute = book.get(EXECUTE).expect("execute");
        ready.fired(execute, NOW - 1);
        assert!(
            fired_ids(&book, &[EXECUTE], &ready, &event(&[Trigger::Hit], 0.2)).is_empty(),
            "inside its 2 s"
        );
        ready.fired(execute, NOW - 2_000);
        assert_eq!(
            fired_ids(&book, &[EXECUTE], &ready, &event(&[Trigger::Hit], 0.2)).len(),
            1,
            "after it"
        );
    }

    #[test]
    fn a_helper_comes_with_its_ability_and_a_required_ability_must_be_held() {
        let book = book();
        let ready = PassiveReady::default();
        assert_eq!(
            fired_ids(&book, &[PARENT], &ready, &event(&[Trigger::HitTaken], 1.0)),
            [(PARENT.to_owned(), 1), (HELPER.to_owned(), 1)]
        );
        assert!(
            fired_ids(&book, &[KILL], &ready, &event(&[Trigger::Kill], 0.0)).is_empty(),
            "the kill passive needs its parent"
        );
        assert_eq!(
            fired_ids(
                &book,
                &[KILL, PARENT],
                &ready,
                &event(&[Trigger::Kill], 0.0)
            ),
            [(KILL.to_owned(), 1)]
        );
    }

    #[test]
    fn the_hit_changes_add_up_by_copies() {
        let book = book();
        let execute = book.get(EXECUTE).expect("execute");
        let mods = HitMods::of(&[Fired {
            ability: execute,
            copies: 2,
        }]);
        assert!((mods.damage_share - 0.2).abs() < 1e-9);
        assert!((mods.dealt_share_for(HitAim::default()) - 0.2).abs() < 1e-9);
        let aimed = HitMods {
            headshot_share: 0.1,
            behind_share: 0.3,
            ..mods
        };
        assert!(
            (aimed.dealt_share_for(HitAim::default()) - 0.2).abs() < 1e-9,
            "a plain hit"
        );
        let head = HitAim {
            headshot: true,
            behind: false,
        };
        assert!((aimed.dealt_share_for(head) - 0.3).abs() < 1e-9);
        assert_eq!(
            head.triggers().collect::<Vec<_>>(),
            [Trigger::HeadShot],
            "its own trigger"
        );
        assert!((mods.taken_scale() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_condition_on_a_mob_of_another_sim_does_not_hold() {
        let book = book();
        let mut remote = event(&[Trigger::Hit], 0.2);
        remote.other = None;
        assert!(fired_ids(&book, &[EXECUTE], &PassiveReady::default(), &remote).is_empty());
    }
}
