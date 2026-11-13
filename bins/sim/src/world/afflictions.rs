// SPDX-License-Identifier: AGPL-3.0-or-later
//! The afflictions ([`nw_content::afflictions`], findings `2026-11-13-catacomb-erosion.md`,
//! sections C6 to C9): each player's affliction meters (blight, erosion, curse, poison and every
//! other kind of the vitals), the effects a full meter adds, and the abilities the catacombs'
//! erosion effects equip.
//!
//! **Observed** (871 retail ledgers with an own affliction list; client `FUN_1467dd9d0`, the
//! server's add of an affliction, and `FUN_145c27160`, its meter change):
//!
//! - A meter starts with a gain above `f32::EPSILON` (`DAT_147f41070`). A gain is cut by the
//!   share of the holder's `AFA<Kind>` effects, the equip load's among them (erosion +2.0 a hit
//!   on a light load, `CATACOMBS5` 133.048 s; +1.5 on a medium one, `CATACOMBS2` 36.440 s); a
//!   fall is not. The meter's top is the holder's `Res<Kind>` plus the kind's `Max` (blight 60,
//!   `CATACOMBS6` 1310.842 s; erosion 40). The new value is the value now plus the change,
//!   between 0 and the top: about 0 ends the meter; a meter that moved holds the new value
//!   for the kind's `DrainDelay` (blight 1.1 s), then falls at its `DrainRate` (blight 1 a
//!   second, `CATACOMBS6` 1315.939 s: 25 to 0 over 25 s) and ends at 0 (Garden of Genesis
//!   45.417 s to 65.459 s); one at the top is full.
//! - A full meter adds the kind's `StatusEffects` to its holder and falls from the top to 0 over
//!   the `AfflictedDrainTime` (blight 180 s, `CATACOMBS6` 1323.808 s; erosion 10 s, `CATACOMBS3`
//!   947.871 s); it ends at 0, or when its holder no longer holds the effect (curse: `Curse` lasts
//!   0.1 s, the lists empty 0.12 s to 0.13 s after the fill in four ledgers). A gain on a full
//!   meter of a kind without `IgnoreWhenAfflicted` moves it and its fall starts again from the
//!   new value (blight +5 each second keeps it at 60, `CATACOMBS6` 1324.841 s to 1356.770 s; +2
//!   gives 45.156 over 135.5 s, Hive Genesis 759.155 s); a kind with it takes none.
//! - A death ends every meter (`CATACOMBS6` death 1400.600 s, both lists empty 1400.638 s).
//! - The own vitals' slot 1 shows the meters ([`nw_messages::HotLong`],
//!   [`nw_messages::ColdLong`]): each meter by its `IntId` in key order (`CATACOMBS6` 1325.775 s:
//!   blight 8, erosion 15), the hot items at every change, the cold items when a meter comes,
//!   fills, goes or changes its top; no meter, both lists empty.
//! - Erosion's chain: a run player holds the erosion effects from its arrival
//!   ([`SimWorld::erosion_joined`]); each hit it takes adds the effects of the root's
//!   `OnHitTaken` ability ([`SimWorld::erosion_hit_taken`]); the full erosion meter's `Erosion`
//!   moves the erosion level one step up (`Erosion_lvN_Temp` for 10 s and
//!   `Erosion_lvN_Indicator`); an ability fires with the effects held before its event, so one
//!   fill moves the level one step (`CATACOMBS3` 947.871 s: lv0 to lv1 only). A potion of the
//!   erosion chest (`Catacombs_AbsorbErosion`, -20 a second) or an erosion shrine
//!   (`CAT_PowerUp_Health`, -100, [`SimWorld::erosion_gathered`]) empties it (`CATACOMBS2`
//!   303.897 s: 27 to 7; `CATACOMBS3` 976.819 s).
//! - An attack row's `AddAffliction` adds at the hit ([`SimWorld::affliction_hit_by`];
//!   `CATACOMBS6` 916.723 s +3.0, 937.542 s +8.0 erosion on a light load); a consumable's
//!   `OnUseAffliction` at its use ([`SimWorld::affliction_used`]).
//!
//! - A global affliction (the mutation curses, [`AfflictionRules::global_kinds`]) is one meter of
//!   the run's game mode, not of a player: a gain of its kind from a player's effect goes to the
//!   run's meter ([`super::expedition_mode`]; retail `20261002-184508-050`: the own vitals never
//!   listed it, the game mode's child 22 did). The same [`AfflictionMeters`] runs it.
//! - When a player performs an ability (its press starts it), the `OnPerform` abilities
//!   ([`EffectTrigger::Performed`]) of the curse effects it holds, and of the run's minor curse,
//!   add their effects ([`SimWorld::ability_performed`]; Observed: the run players held
//!   `Mut_Curse2_Affliction_Tick`, the minor's effect, 2,639 own and 6,134 remote map lines; no
//!   one held `Mut_Curse2_Minor_Buff`). The major curses' (Blood Offering
//!   `Mut_Curse2_<cat>_Major_Damage`, unless the player holds `Blunderbuss_MortarShotFired`;
//!   Exhaust `Mut_Curse4_Major_Exhaust`) fire the same way (Inferred from their rows and
//!   tooltips: no capture of a run with a major curse's damage tick).
//!
//! **Inferred**: a meter of a kind with no `DrainRate` keeps its value after its hold (the
//! erosion's 9,999 s hold never ended in a ledger); the held effects' `Res<Kind>` raise the top
//! as the vitals row's does; the top is taken again at every change (the client passes the
//! resistance to each change); the lists' id is the holder's own count of meter sends (retail's
//! ids only grew); an effect that another part of the server adds (a mob's attack effect such as
//! `AI_Fulgoris_Gas`, a consumable's, an effect's `OnEndStatusEffect`) starts its ticks at the
//! next tick; the first tick of an effect without `DelayInitialTick` falls at its application
//! (`CATACOMBS6`: the first +5.0 at 1310.842 s, before the own effects list shows
//! `AI_Fulgoris_Gas` at 1310.975 s); a meter does not go with its holder from one sim to
//! another (an instance exit ends it, as `RemoveOnGameModeExit` asks of the erosion).
//!
//! **Not built** (no change for the client): retail sends a full meter's line once more from the
//! value just after its effect is added, with the same end (`CATACOMBS6` 1323.908 s). **Unknown**:
//! why `AI_Fulgoris_Victory` (`AfflictionBlight=-99`) held on a full blight meter changed no list
//! (`CATACOMBS6` 1367.974 s; the only ledger with it); this runtime lets the fall through as
//! `FUN_145c27160` does.

use bevy_ecs::entity::Entity;
use nw_content::afflictions::{
    AffixShare, AfflictionGain, AfflictionKind, AfflictionRules, EROSION_ROOT_EFFECT, EffectTrigger,
};
use nw_content::status_effects::RowKey;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::{
    AfflictionId, AfflictionList, BundleMember, ColdLong, GlobalAffliction, HotLong, MemberState,
    MeterLine, PLAYER_VITALS_COMPONENT, VitalsSlot1, VitalsState, put_member,
};

use super::attack::Vitals;
use super::{Player, SimWorld};

/// The tile slot of the erosion chest (`Catacombs_Potion_RemoveErosion_Chest`, its loot the
/// erosion potion).
pub(super) const EROSION_CHEST_SLOT: &str = "ChestErosion";
/// The tile slot of the erosion shrine (`Catacombs_HealthChest`).
pub(super) const EROSION_SHRINE_SLOT: &str = "ErosionShrine";
/// The slices of the erosion chest and shrine: start objects a player gathers (their spawn
/// states: the retail cards of the ten catacomb ledgers, 132 chest and 37 shrine cards).
const EROSION_PROP_SLICES: [&str; 2] = [
    "slices/catacombs/gameplay/gatherables/catacombs_gatherable_chest_removeerosion.dynamicslice",
    "slices/catacombs/gameplay/gatherables/catacombs_gatherable_erosionshrine.dynamicslice",
];
/// The room kinds of the room tiles (`*_a` variations, `javelindata_dungeontiles`
/// `SupportedRoomTypes`), which hold both erosion slots.
pub(super) const ROOM_TILE_KINDS: [&str; 6] = [
    "EncounterStart",
    "EncounterBoss",
    "EncounterNamedEnemy",
    "EncounterGate",
    "EncounterHazard",
    "Reward",
];
/// The erosion prop a room tile draws and its weight. Observed over the 202 room tiles of the
/// ten retail ledgers: 17 shrines (distinct shrine places of the ledgers' cards), 15 erosion
/// chests and no silver chest. Inferred: one draw per room, as the corridors' chests.
pub(super) const ROOM_EROSION_PROPS: [(Option<&str>, u64); 3] = [
    (Some(EROSION_SHRINE_SLOT), 17),
    (Some(EROSION_CHEST_SLOT), 15),
    (None, 170),
];

/// Whether `slice` is the erosion chest's or shrine's slice.
pub(super) fn is_erosion_prop(slice: &str) -> bool {
    EROSION_PROP_SLICES.contains(&slice)
}

/// The potency of an affliction effect (the rows give none).
const PLAIN_POTENCY: f64 = 1.0;
/// Nanoseconds in a millisecond: the meter's times are ns since 2000-01-01, the server's
/// clock (`SERVER_EPOCH_UNIX_MS`).
const NS_PER_MS: u64 = 1_000_000;
/// Milliseconds in a second.
const MS_PER_S: f32 = 1_000.0;
/// The deepest chain of abilities one event may start (the data's chains are two deep).
const MAX_CHAIN: u8 = 4;
/// The whole of a gain: the absorption takes at most this share.
const WHOLE_SHARE: f32 = 1.0;
/// The smallest change a meter tells apart (client `DAT_147f41070`, 0x34000000).
const METER_EPSILON: f32 = f32::EPSILON;

/// A held effect that changes meters, and when its next tick falls (server ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TickRun {
    effect: u32,
    next_ms: u64,
}

/// The vitals' slot 1 affliction lists of one send.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct AfflictionLists {
    hot: AfflictionList<HotLong>,
    cold: Option<AfflictionList<ColdLong>>,
}

/// Where a meter is on its way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// It holds its value after a change, until server ms `until_ms`.
    Holding { until_ms: u64 },
    /// It falls at its kind's `DrainRate`.
    Draining,
    /// It is full: it falls from its top over the kind's `AfflictedDrainTime`.
    Afflicted,
}

/// One affliction meter of a player.
#[derive(Debug, Clone, PartialEq)]
struct Meter {
    /// The kind ([`AfflictionKind::crc`]).
    kind: u32,
    /// The kind's `IntId`: the item key.
    int_id: u8,
    /// The top: the holder's resistance plus the kind's `Max`.
    top: f32,
    /// The value at `since_ms`.
    value: f32,
    /// When the value and the phase were set (server ms).
    since_ms: u64,
    phase: Phase,
    /// The full meter's effect is on its holder: the meter ends when the holder no longer
    /// holds it.
    effect_added: bool,
}

impl Meter {
    /// The value at server ms `now_ms`.
    fn value_at(&self, kind: &AfflictionKind, now_ms: u64) -> f32 {
        // A few minutes at most.
        let elapsed_ms = now_ms.saturating_sub(self.since_ms) as f32;
        let fall = match self.phase {
            Phase::Holding { .. } => 0.0,
            Phase::Draining => kind.drain_per_s * elapsed_ms / MS_PER_S,
            Phase::Afflicted if kind.afflicted_ms == 0 => self.value,
            Phase::Afflicted => self.top * elapsed_ms / kind.afflicted_ms as f32,
        };
        (self.value - fall).max(0.0)
    }

    /// When the meter reaches 0 (server ms), if it falls.
    fn empty_ms(&self, kind: &AfflictionKind) -> Option<u64> {
        let fall_ms = match self.phase {
            Phase::Holding { .. } => return None,
            Phase::Draining if kind.drain_per_s <= 0.0 => return None,
            Phase::Draining => self.value / kind.drain_per_s * MS_PER_S,
            Phase::Afflicted if self.top <= 0.0 => 0.0,
            Phase::Afflicted => self.value / self.top * kind.afflicted_ms as f32,
        };
        // A few minutes at most.
        Some(self.since_ms.saturating_add(fall_ms.round() as u64))
    }

    /// The meter's line from the value at `since_ms`: a held value to its hold's end, a fall
    /// to 0 when it is empty.
    fn points(&self, kind: &AfflictionKind) -> MeterLine {
        let (to, to_ms) = match self.phase {
            Phase::Holding { until_ms } => (self.value, until_ms),
            Phase::Draining | Phase::Afflicted => (0.0, self.empty_ms(kind).unwrap_or(u64::MAX)),
        };
        MeterLine {
            last: self.value,
            last_ns: self.since_ms.saturating_mul(NS_PER_MS),
            target: to,
            target_ns: to_ms.saturating_mul(NS_PER_MS),
        }
    }

    /// The hot long form of [`Self::points`].
    fn line(&self, kind: &AfflictionKind) -> HotLong {
        let line = self.points(kind);
        HotLong::line(line.last, line.target, line.last_ns, line.target_ns)
    }

    /// The cold long form.
    fn cold(&self) -> ColdLong {
        ColdLong::of(self.top, self.int_id, self.phase == Phase::Afflicted)
    }
}

/// What a change did to a meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Change {
    /// Nothing.
    None,
    /// The meter holds a new value.
    Moved,
    /// The meter filled.
    Filled,
    /// The meter ended; whether it was full.
    Ended { afflicted: bool },
}

/// One player's affliction meters.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct AfflictionMeters {
    /// The meters, in key order.
    meters: Vec<Meter>,
    /// The lines of the meters, by key, as the client was last told.
    lines: Vec<(u8, HotLong)>,
    /// The cold items the client's cold list holds.
    shown_cold: Vec<(u8, ColdLong)>,
    /// The count of meter sends: the lists' id.
    sends: u64,
    /// The held effects that tick the meters.
    runs: Vec<TickRun>,
    /// The watched effects held at the last look ([`watched_effects`]).
    watched: Vec<u32>,
    /// The meters changed since the last [`AfflictionMeters::take_lists`].
    changed: bool,
}

impl AfflictionMeters {
    /// The lists of the next own state, once after a change: every meter's line, and the cold
    /// items when they differ from what the client holds.
    pub(super) fn take_lists(&mut self) -> Option<AfflictionLists> {
        if !std::mem::take(&mut self.changed) {
            return None;
        }
        self.sends += 1;
        let id = AfflictionId::Wide(self.sends);
        let cold: Vec<(u8, ColdLong)> = self
            .meters
            .iter()
            .map(|meter| (meter.int_id, meter.cold()))
            .collect();
        let cold = (cold != self.shown_cold).then(|| {
            self.shown_cold.clone_from(&cold);
            AfflictionList::Full { id, items: cold }
        });
        Some(AfflictionLists {
            hot: AfflictionList::Full {
                id,
                items: self.lines.clone(),
            },
            cold,
        })
    }

    /// Takes the meters' lines again after a change.
    fn note_change(&mut self, rules: &AfflictionRules) {
        self.lines = self
            .meters
            .iter()
            .filter_map(|meter| Some((meter.int_id, meter.line(rules.meter_kind(meter.kind)?))))
            .collect();
        self.changed = true;
    }

    /// Changes the meter of `kind` by `amount` at `now_ms` (module text): a gain less the
    /// `absorbed` share, the meter's top `top`.
    pub(super) fn change(
        &mut self,
        kind: &AfflictionKind,
        amount: f32,
        absorbed: f32,
        top: f32,
        now_ms: u64,
    ) -> Change {
        let amount = if amount >= 0.0 {
            amount * (WHOLE_SHARE - absorbed.min(WHOLE_SHARE))
        } else {
            amount
        };
        let index = match self.meters.iter().position(|meter| meter.kind == kind.crc) {
            Some(index) => index,
            None if amount > METER_EPSILON && top > METER_EPSILON => {
                let index = self
                    .meters
                    .partition_point(|meter| meter.int_id < kind.int_id);
                self.meters.insert(
                    index,
                    Meter {
                        kind: kind.crc,
                        int_id: kind.int_id,
                        top,
                        value: 0.0,
                        since_ms: now_ms,
                        phase: Phase::Holding { until_ms: now_ms },
                        effect_added: false,
                    },
                );
                index
            }
            None => return Change::None,
        };
        let meter = &mut self.meters[index];
        meter.top = top;
        let current = meter.value_at(kind, now_ms);
        let value = (current + amount).clamp(0.0, top);
        if value.abs() <= METER_EPSILON {
            let ended = self.meters.remove(index);
            return Change::Ended {
                afflicted: ended.phase == Phase::Afflicted,
            };
        }
        let afflicted = meter.phase == Phase::Afflicted;
        if (current - value).abs() <= METER_EPSILON
            || (amount >= 0.0 && afflicted && kind.ignore_when_afflicted)
        {
            return Change::None;
        }
        meter.value = value;
        meter.since_ms = now_ms;
        if afflicted {
            Change::Moved
        } else if value < top - METER_EPSILON {
            meter.phase = Phase::Holding {
                until_ms: now_ms.saturating_add(kind.drain_delay_ms),
            };
            Change::Moved
        } else {
            meter.phase = Phase::Afflicted;
            Change::Filled
        }
    }

    /// Moves each meter on at `now_ms`: a hold that ran out starts the fall; a meter at 0 ends.
    /// The kinds of the ended meters that were full.
    pub(super) fn advance(&mut self, rules: &AfflictionRules, now_ms: u64) -> Vec<u32> {
        let mut moved = false;
        let mut ended_full = Vec::new();
        self.meters.retain_mut(|meter| {
            let Some(kind) = rules.meter_kind(meter.kind) else {
                moved = true;
                return false;
            };
            match meter.phase {
                Phase::Holding { until_ms } if now_ms >= until_ms && kind.drain_per_s > 0.0 => {
                    meter.phase = Phase::Draining;
                    meter.since_ms = now_ms;
                    moved = true;
                    true
                }
                Phase::Holding { .. } => true,
                Phase::Draining | Phase::Afflicted => {
                    if meter.empty_ms(kind).is_some_and(|empty| now_ms >= empty) {
                        moved = true;
                        if meter.phase == Phase::Afflicted {
                            ended_full.push(meter.kind);
                        }
                        false
                    } else {
                        true
                    }
                }
            }
        });
        if moved {
            self.note_change(rules);
        }
        ended_full
    }

    /// Changes the global meter of `kind` (a game mode's: nothing absorbs a share, the kind's
    /// `Max` is its top) by `amount` at `now_ms`.
    pub(super) fn change_global(
        &mut self,
        kind: &AfflictionKind,
        amount: f32,
        now_ms: u64,
    ) -> Change {
        let change = self.change(kind, amount, 0.0, kind.max, now_ms);
        if change != Change::None {
            self.changed = true;
        }
        change
    }

    /// Whether the meters changed since the last call.
    pub(super) fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// The global affliction data of the meter of `kind` at `now_ms` (game mode child 22): its
    /// line, its top, its `IntID` and whether it is full; with no meter, 0 held from `now_ms`
    /// for the kind's `DrainDelay` with the kind's `IntID` and `Max` (Observed: the Fiendish
    /// meter emptied by a -1, `20261002-184508-050` Unix ms 1790964503188: 0 to 0 over 9,999 s,
    /// id 11; other sends at 0 keep the old target, Unknown why).
    pub(super) fn global_data(&self, kind: &AfflictionKind, now_ms: u64) -> GlobalAffliction {
        match self.meters.iter().find(|meter| meter.kind == kind.crc) {
            Some(meter) => GlobalAffliction::of(
                meter.points(kind),
                meter.top,
                meter.int_id,
                meter.phase == Phase::Afflicted,
            ),
            None => GlobalAffliction::of(
                MeterLine {
                    last: 0.0,
                    last_ns: now_ms.saturating_mul(NS_PER_MS),
                    target: 0.0,
                    target_ns: now_ms
                        .saturating_add(kind.drain_delay_ms)
                        .saturating_mul(NS_PER_MS),
                },
                kind.max,
                kind.int_id,
                false,
            ),
        }
    }

    /// Ends every meter (a death). The kinds of the ended meters that were full.
    fn end_all(&mut self, rules: &AfflictionRules) -> Vec<u32> {
        let ended_full = self
            .meters
            .drain(..)
            .filter(|meter| meter.phase == Phase::Afflicted)
            .map(|meter| meter.kind)
            .collect();
        self.note_change(rules);
        ended_full
    }
}

/// The part of kind `kind` that `player`'s worn perks give ([`AfflictionRules::worn_part`]).
fn worn_part(rules: &AfflictionRules, player: &Player, kind: u32) -> AffixShare {
    player
        .gear
        .as_ref()
        .map_or_else(AffixShare::default, |gear| {
            rules.worn_part(&gear.worn_perks(), kind)
        })
}

/// The share of a gain of kind `kind` that `player`'s held `AFA<Kind>` effects and worn perks
/// take.
fn absorbed_share(rules: &AfflictionRules, player: &Player, kind: u32) -> f32 {
    let held: f32 = rules
        .absorptions
        .iter()
        .filter(|share| share.kind == kind && player.status_effects.holds(share.effect))
        .map(|share| share.value)
        .sum();
    held + worn_part(rules, player, kind).absorption
}

/// The top of `player`'s meter of `kind`: its resistance (the vitals row's, its held effects'
/// `Res<Kind>` and its worn perks' `RES<Kind>`) plus the kind's `Max`.
fn meter_top(rules: &AfflictionRules, player: &Player, kind: &AfflictionKind) -> f32 {
    let held: f32 = rules
        .resistances
        .iter()
        .filter(|share| share.kind == kind.crc && player.status_effects.holds(share.effect))
        .map(|share| share.value)
        .sum();
    let worn = worn_part(rules, player, kind.crc).resistance;
    (kind.resistance + held + worn + kind.max).max(0.0)
}

/// Puts the affliction `lists` into the own group `members`: into the vitals member the group
/// carries already (a health or mana change), else as its own vitals member.
pub(super) fn put_afflictions(members: &mut Vec<BundleMember>, lists: AfflictionLists) {
    let known = members
        .iter_mut()
        .find_map(|member| match &mut member.state {
            MemberState::Vitals(vitals) if member.component == PLAYER_VITALS_COMPONENT => {
                Some(vitals)
            }
            _ => None,
        });
    let put = |slot: &mut VitalsSlot1| {
        slot.afflictions_hot = Some(lists.hot.clone());
        slot.afflictions_cold.clone_from(&lists.cold);
    };
    match known {
        Some(vitals) => put(vitals.slot_1.get_or_insert_with(VitalsSlot1::default)),
        None => {
            let mut slot = VitalsSlot1::default();
            put(&mut slot);
            put_member(
                members,
                BundleMember {
                    component: PLAYER_VITALS_COMPONENT,
                    state: MemberState::Vitals(VitalsState {
                        slot_0: None,
                        slot_1: Some(slot),
                    }),
                },
            );
        }
    }
}

/// The effects whose application another part of the server may make: those that tick the
/// meters and those whose own application fires their ability.
fn watched_effects(rules: &AfflictionRules) -> impl Iterator<Item = u32> + '_ {
    rules.ticks.iter().map(|tick| tick.effect).chain(
        rules
            .abilities
            .iter()
            .filter(|ability| ability.trigger == EffectTrigger::Applied(None))
            .map(|ability| ability.holder),
    )
}

impl SimWorld {
    /// Player `entity` arrived in the catacombs run: it gets the erosion root effect and what
    /// its ability equips (module text), once.
    pub(super) fn erosion_joined(&mut self, entity: Entity) {
        let root = lowercase_crc32(EROSION_ROOT_EFFECT);
        if self.holds_effect(entity, root) {
            return;
        }
        self.add_affliction_effect(entity, EROSION_ROOT_EFFECT, 0);
        self.watch_afflictions(entity);
    }

    /// Player `entity` took a hit: the `OnHitTaken` abilities of its held affliction effects
    /// (Observed: only the erosion root's) add their effects.
    pub(super) fn erosion_hit_taken(&mut self, entity: Entity) {
        let content = std::sync::Arc::clone(&self.content);
        let fired: Vec<&String> = content
            .afflictions
            .abilities
            .iter()
            .filter(|ability| ability.trigger == EffectTrigger::HitTaken)
            .filter(|ability| self.holds_effect(entity, ability.holder))
            .flat_map(|ability| &ability.apply)
            .collect();
        for id in fired {
            self.add_affliction_effect(entity, id, 0);
        }
        self.watch_afflictions(entity);
    }

    /// Player `entity` took a hit of attack row `row`: a row that adds an affliction changes
    /// its meter by its `AddAffliction` at once (module text).
    pub(super) fn affliction_hit_by(&mut self, entity: Entity, row: Option<RowKey>) {
        let Some(gain) = row.and_then(|row| self.content.afflictions.hit_gain(row)) else {
            return;
        };
        self.change_affliction(entity, gain, self.server_ms);
        self.watch_afflictions(entity);
    }

    /// Player `entity` used consumable `consumable`: its `OnUseAffliction` changes the meters
    /// it names (a tincture's -9999 ends a meter, a full one with its effect).
    pub(super) fn affliction_used(&mut self, entity: Entity, consumable: &str) {
        let content = std::sync::Arc::clone(&self.content);
        let now_ms = self.server_ms;
        for gain in content.afflictions.use_gains(consumable) {
            self.change_affliction(entity, *gain, now_ms);
        }
        self.watch_afflictions(entity);
    }

    /// Player `number` gathered `gatherable`: an erosion shrine adds its effect.
    pub(super) fn erosion_gathered(&mut self, number: u64, gatherable: &str) {
        let content = std::sync::Arc::clone(&self.content);
        let Some(effect) = content
            .afflictions
            .gathered_effect(lowercase_crc32(gatherable))
        else {
            return;
        };
        let Some(entity) = self.player_by_number(number) else {
            return;
        };
        self.add_affliction_effect(entity, effect, 0);
        self.watch_afflictions(entity);
    }

    /// One affliction tick of every player: a death ends its meters; the watched effects that
    /// another part of the server applied start; the held effects' ticks fall due; the meters
    /// move on; a full meter whose effect ended goes.
    pub(super) fn tick_afflictions(&mut self) {
        let content = std::sync::Arc::clone(&self.content);
        let rules = &content.afflictions;
        if rules.kinds.is_empty() && rules.global_kinds.is_empty() {
            return;
        }
        let now_ms = self.server_ms;
        let players: Vec<(Entity, bool)> = self
            .world
            .query::<(Entity, &Player, &Vitals)>()
            .iter(&self.world)
            .map(|(entity, _, vitals)| (entity, vitals.health.is_dead()))
            .collect();
        for (entity, dead) in players {
            if dead {
                let ended = self
                    .world
                    .get_mut::<Player>(entity)
                    .filter(|player| !player.afflictions.meters.is_empty())
                    .map(|mut player| player.afflictions.end_all(rules));
                if let Some(ended) = ended {
                    self.remove_afflicted_effects(entity, &ended);
                }
                continue;
            }
            let newly: Vec<u32> = watched_effects(rules)
                .filter(|effect| self.holds_effect(entity, *effect))
                .filter(|effect| {
                    self.world
                        .get::<Player>(entity)
                        .is_some_and(|player| !player.afflictions.watched.contains(effect))
                })
                .collect();
            for effect in newly {
                let held = self.held_chain_effects(entity);
                self.start_affliction_tick(entity, effect);
                self.effect_applied(entity, effect, &held, 0);
            }
            self.run_affliction_ticks(entity, now_ms);
            let mut ended = self
                .world
                .get_mut::<Player>(entity)
                .map(|mut player| player.afflictions.advance(rules, now_ms))
                .unwrap_or_default();
            ended.extend(self.end_meters_without_effect(entity));
            self.remove_afflicted_effects(entity, &ended);
            self.watch_afflictions(entity);
        }
    }

    /// Ends player `entity`'s full meters whose effect it no longer holds; their kinds.
    fn end_meters_without_effect(&mut self, entity: Entity) -> Vec<u32> {
        let content = std::sync::Arc::clone(&self.content);
        let rules = &content.afflictions;
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return Vec::new();
        };
        let player = &mut *player;
        let gone: Vec<u32> = player
            .afflictions
            .meters
            .iter()
            .filter(|meter| meter.effect_added)
            .filter(|meter| {
                rules.kind(meter.kind).is_some_and(|kind| {
                    !kind
                        .afflicted_effects
                        .iter()
                        .any(|id| player.status_effects.holds(lowercase_crc32(id)))
                })
            })
            .map(|meter| meter.kind)
            .collect();
        if !gone.is_empty() {
            player
                .afflictions
                .meters
                .retain(|meter| !gone.contains(&meter.kind));
            player.afflictions.note_change(rules);
        }
        gone
    }

    /// Changes player `entity`'s meter of `gain`'s kind by its amount at `at_ms`: a filled
    /// meter adds its effects, an ended full one takes them away.
    fn change_affliction(&mut self, entity: Entity, gain: AfflictionGain, at_ms: u64) {
        let content = std::sync::Arc::clone(&self.content);
        let rules = &content.afflictions;
        if rules.global_kind(gain.kind).is_some() {
            self.run_curse_gain(gain, at_ms);
            return;
        }
        let Some(kind) = rules.kind(gain.kind) else {
            return;
        };
        let change = {
            let Some(mut player) = self.world.get_mut::<Player>(entity) else {
                return;
            };
            let player = &mut *player;
            let absorbed = absorbed_share(rules, player, kind.crc);
            let top = meter_top(rules, player, kind);
            let change = player
                .afflictions
                .change(kind, gain.amount, absorbed, top, at_ms);
            if change != Change::None {
                player.afflictions.note_change(rules);
            }
            change
        };
        match change {
            Change::Filled => {
                for id in &kind.afflicted_effects {
                    self.add_affliction_effect(entity, id, 0);
                }
                let held = kind
                    .afflicted_effects
                    .iter()
                    .any(|id| self.holds_effect(entity, lowercase_crc32(id)));
                if let Some(mut player) = self.world.get_mut::<Player>(entity)
                    && let Some(meter) = player
                        .afflictions
                        .meters
                        .iter_mut()
                        .find(|meter| meter.kind == kind.crc)
                {
                    meter.effect_added = held;
                }
            }
            Change::Ended { afflicted: true } => {
                self.remove_afflicted_effects(entity, &[kind.crc]);
            }
            Change::None | Change::Moved | Change::Ended { afflicted: false } => {}
        }
    }

    /// Takes the effects of the full meters of `kinds` away from player `entity`.
    fn remove_afflicted_effects(&mut self, entity: Entity, kinds: &[u32]) {
        let content = std::sync::Arc::clone(&self.content);
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        for kind in kinds
            .iter()
            .filter_map(|kind| content.afflictions.kind(*kind))
        {
            for id in &kind.afflicted_effects {
                player.status_effects.remove(lowercase_crc32(id));
            }
        }
    }

    /// Whether player `entity` holds effect `effect`.
    fn holds_effect(&self, entity: Entity, effect: u32) -> bool {
        self.world
            .get::<Player>(entity)
            .is_some_and(|player| player.status_effects.holds(effect))
    }

    /// The affliction ability holders and owned effects player `entity` holds now.
    fn held_chain_effects(&self, entity: Entity) -> Vec<u32> {
        self.content
            .afflictions
            .abilities
            .iter()
            .flat_map(|ability| std::iter::once(ability.holder).chain(ability.owned))
            .filter(|effect| self.holds_effect(entity, *effect))
            .collect()
    }

    /// Marks the watched effects player `entity` holds now as seen.
    fn watch_afflictions(&mut self, entity: Entity) {
        let held: Vec<u32> = watched_effects(&self.content.afflictions)
            .filter(|effect| self.holds_effect(entity, *effect))
            .collect();
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.afflictions.watched = held;
        }
    }

    /// Adds affliction effect `id` to player `entity` with what its ability equips, starts its
    /// meter ticks and fires the abilities its application fires, `depth` abilities deep.
    fn add_affliction_effect(&mut self, entity: Entity, id: &str, depth: u8) {
        let crc = lowercase_crc32(id);
        let held = self.held_chain_effects(entity);
        let now_ms = self.server_ms;
        let content = std::sync::Arc::clone(&self.content);
        let added = self
            .world
            .get_mut::<Player>(entity)
            .is_some_and(|mut player| {
                let number = player.number;
                player.status_effects.add_by_id(
                    &content.status_effects,
                    id,
                    PLAIN_POTENCY,
                    number,
                    now_ms,
                )
            });
        if !added {
            return;
        }
        for equipped in content.afflictions.equipped_by(crc) {
            self.add_affliction_effect(entity, equipped, depth);
        }
        self.start_affliction_tick(entity, crc);
        self.effect_applied(entity, crc, &held, depth);
    }

    /// Starts the meter ticks of effect `effect` on player `entity`: a first tick at once runs
    /// now.
    fn start_affliction_tick(&mut self, entity: Entity, effect: u32) {
        let Some(first_ms) = self
            .content
            .afflictions
            .tick(effect)
            .map(|tick| tick.first_ms)
        else {
            return;
        };
        let now_ms = self.server_ms;
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            let runs = &mut player.afflictions.runs;
            runs.retain(|run| run.effect != effect);
            runs.push(TickRun {
                effect,
                next_ms: now_ms.saturating_add(first_ms),
            });
        }
        self.run_affliction_ticks(entity, now_ms);
    }

    /// The due meter ticks of player `entity`'s held effects at `now_ms`, in time order; an
    /// effect no longer held, or past its end, ticks no more.
    fn run_affliction_ticks(&mut self, entity: Entity, now_ms: u64) {
        let content = std::sync::Arc::clone(&self.content);
        let rules = &content.afflictions;
        let mut due: Vec<(u64, AfflictionGain)> = Vec::new();
        {
            let Some(mut player) = self.world.get_mut::<Player>(entity) else {
                return;
            };
            let player = &mut *player;
            let effects = &player.status_effects;
            player.afflictions.runs.retain_mut(|run| {
                let Some(tick) = rules.tick(run.effect) else {
                    return false;
                };
                let ends_ms = effects.ends_ms(run.effect);
                loop {
                    if !effects.holds(run.effect) || ends_ms.is_some_and(|end| run.next_ms > end) {
                        return false;
                    }
                    if run.next_ms > now_ms {
                        return true;
                    }
                    due.extend(tick.gains.iter().map(|gain| (run.next_ms, *gain)));
                    if tick.period_ms == 0 {
                        return false;
                    }
                    run.next_ms = run.next_ms.saturating_add(tick.period_ms);
                }
            });
        }
        due.sort_by_key(|(at_ms, _)| *at_ms);
        for (at_ms, gain) in due {
            self.change_affliction(entity, gain, at_ms);
        }
    }

    /// Player `entity` performed an ability (its press started it, [`super::abilities`]): the
    /// `OnPerform` abilities of the run's minor curse (when it is a run player) and of the
    /// effects it holds add their effects, each when the player owns the effect it asks for and
    /// not the one it must not hold (module text).
    pub(super) fn ability_performed(&mut self, entity: Entity) {
        let content = std::sync::Arc::clone(&self.content);
        let rules = &content.afflictions;
        let held: Vec<u32> = rules
            .abilities
            .iter()
            .filter(|ability| ability.trigger == EffectTrigger::Performed)
            .map(|ability| ability.holder)
            .filter(|holder| self.holds_effect(entity, *holder))
            .collect();
        let mut holders: Vec<u32> = self
            .run_minor_curse(entity)
            .into_iter()
            .chain(held)
            .collect();
        holders.sort_unstable();
        holders.dedup();
        let fired: Vec<&String> = holders
            .iter()
            .flat_map(|holder| rules.performed_by(*holder))
            .filter(|ability| {
                ability
                    .owned
                    .is_none_or(|owned| self.holds_effect(entity, owned))
            })
            .filter(|ability| {
                ability
                    .not_owned
                    .is_none_or(|barred| !self.holds_effect(entity, barred))
            })
            .flat_map(|ability| &ability.apply)
            .collect();
        for id in fired {
            self.add_affliction_effect(entity, id, 0);
        }
        self.watch_afflictions(entity);
    }

    /// Effect `effect` was applied to player `entity`, which held `held` before: the
    /// abilities it fires remove and add their effects (module text).
    fn effect_applied(&mut self, entity: Entity, effect: u32, held: &[u32], depth: u8) {
        if depth >= MAX_CHAIN {
            return;
        }
        let content = std::sync::Arc::clone(&self.content);
        let fired: Vec<_> = content
            .afflictions
            .abilities
            .iter()
            .filter(|ability| match ability.trigger {
                EffectTrigger::Applied(Some(applied)) => {
                    applied == effect && held.contains(&ability.holder)
                }
                EffectTrigger::Applied(None) => ability.holder == effect,
                EffectTrigger::HitTaken | EffectTrigger::Performed => false,
            })
            .filter(|ability| ability.owned.is_none_or(|owned| held.contains(&owned)))
            .collect();
        for ability in fired {
            if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                for removed in &ability.remove {
                    player.status_effects.remove(*removed);
                }
            }
            for id in &ability.apply {
                self.add_affliction_effect(entity, id, depth + 1);
            }
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use nw_content::afflictions::{AfflictionTick, EffectAbility, EffectShare, PerkShares};
    use nw_content::perk_effects::GearScoreScale;
    use nw_content::status_effects::StatusEffectRules;
    use nw_sim_core::Health;

    use super::*;
    use crate::world::tests::{NOW, SERVER_TIME, admit_at, world_with_spawns};

    const PLAYER: u64 = 3;
    const ROOT: &str = EROSION_ROOT_EFFECT;
    const APPLY: &str = "Catacombs_ApplyErosion";
    const ON_HIT: &str = "Catacombs_Erosion_OnHitTaken";
    const AFFLICTED: &str = "Erosion";
    const LV0: &str = "Erosion_lv0_Indicator";
    const LV1: &str = "Erosion_lv1_Indicator";
    const LV2: &str = "Erosion_lv2_Indicator";
    const LV1_TEMP: &str = "Erosion_lv1_Temp";
    const LV2_TEMP: &str = "Erosion_lv2_Temp";
    const LEVEL_1: &str = "Catacombs_Erosion_Lv1_Ability_v2";
    const LEVEL_2: &str = "Catacombs_Erosion_Lv2_Ability_v2";
    const CHECK_1: &str = "Catacombs_Erosion_Remove_CheckLv1_V2";
    const SHRINE_EFFECT: &str = "CAT_PowerUp_Health";
    const SHRINE: &str = "Catacombs_HealthChest";
    const ABSORB: &str = "Catacombs_AbsorbErosion";
    const EROSION: &str = nw_content::afflictions::EROSION_AFFLICTION;
    const BLIGHT: &str = "AfflictionBlight";
    const BLIGHTED: &str = "Blight";
    const CURSE: &str = "AfflictionCurse";
    const CURSED: &str = "Curse";
    /// `AI_Fulgoris_Gas`: blight 5 a second for 20 s.
    const GAS: &str = "AI_Fulgoris_Gas";
    const GAS_GAIN: f32 = 5.0;
    const GAS_PERIOD_MS: u64 = 1_000;
    /// An effect that raises the blight resistance by 1 (`AI_PalatineGuard_Sacrifice_AbyssCloak`).
    const CLOAK: &str = "AI_PalatineGuard_Sacrifice_AbyssCloak";
    const TINCTURE: &str = "TinctureBlightT2";
    const TINCTURE_FALL: f32 = -9_999.0;
    /// Retail erosion meter: item 15, max 40, full for 10 s, held 9,999 s.
    const INT_ID: u8 = 15;
    const MAX: f32 = 40.0;
    const FULL_MS: u64 = 10_000;
    const HOLD_MS: u64 = 9_999_000;
    /// Retail blight meter: item 8, the players' resistance 60, held 1.1 s, falls 1 a second,
    /// full for 180 s.
    const BLIGHT_ID: u8 = 8;
    const BLIGHT_TOP: f32 = 60.0;
    const BLIGHT_HOLD_MS: u64 = 1_100;
    const BLIGHT_DRAIN_PER_S: f32 = 1.0;
    const BLIGHT_FULL_MS: u64 = 180_000;
    /// Retail curse meter: item 5, resistance 60, full for 0.89 s, no gain while full.
    const CURSE_ID: u8 = 5;
    /// A hit adds 2 at 0.1 s (a light load); the potion takes 20 a second for 10 s.
    const HIT_GAIN: f32 = 2.0;
    const HIT_FIRST_MS: u64 = 100;
    const HIT_PERIOD_MS: u64 = 750;
    const ABSORB_FALL: f32 = -20.0;
    const ABSORB_PERIOD_MS: u64 = 1_000;
    /// Hits that fill the meter: 20 x 2 = 40.
    const HITS_TO_FILL: usize = 20;
    /// The medium equip load's effect and its `AFAErosion` share.
    const MEDIUM_LOAD: &str = "EquipLoad_CritDmgReduction_Medium";
    const MEDIUM_SHARE: f32 = 0.25;
    /// A catacomb boss row that adds 3 erosion (`javelindata_damagetable_catacombs_agheavyboss`).
    const BOSS_TABLE: &str = "DamageTable_Catacombs_AGHeavyBoss";
    const BOSS_SLASH: &str = "Attack_Elite_Double_Slash_1st_Hit";
    const BOSS_SLASH_GAIN: f32 = 3.0;
    /// Made-up rows that add 25 blight and 60 curse at a hit.
    const BLIGHT_ROW: &str = "Attack_Blight";
    const BLIGHT_ROW_GAIN: f32 = 25.0;
    const CURSE_ROW: &str = "Attack_Curse";
    const CURSE_ROW_GAIN: f32 = 60.0;

    /// `PerkID_Armor_ResistBlight` (build 25810808: `Affix_Armor_ResistBlight` `RESBlight` 5,
    /// `AFABlight` 0.05, scale 0.0038 a gear score point to 550) on an item of gear score 200.
    const WARD_PERK: &str = "PerkID_Armor_ResistBlight";
    const WARD_SCALE: &str = "0.00381818181818182,550:0.00";
    const WARD_RESISTANCE: f32 = 5.0;
    const WARD_ABSORPTION: f32 = 0.05;
    const WARD_GEAR_SCORE: u32 = 200;

    /// A status effect row of `id` lasting `duration_s` (-1: no end), one stack.
    pub(in crate::world) fn effect_row(
        id: &str,
        duration_s: f64,
    ) -> nw_content::generated::StatusEffectData {
        serde_json::from_value(serde_json::json!({
            "status_id": id, "base_duration": duration_s, "stack_max": 1
        }))
        .expect("effect row")
    }

    fn kind(
        id: &str,
        int_id: u8,
        (max, resistance): (f32, f32),
        (drain_delay_ms, drain_per_s, afflicted_ms): (u64, f32, u64),
        ignore_when_afflicted: bool,
        effect: &str,
    ) -> AfflictionKind {
        AfflictionKind {
            crc: lowercase_crc32(id),
            id: id.to_owned(),
            int_id,
            max,
            resistance,
            drain_delay_ms,
            drain_per_s,
            afflicted_ms,
            ignore_when_afflicted,
            afflicted_effects: vec![effect.to_owned()],
            timer: false,
        }
    }

    fn tick(
        effect: &str,
        kind: &str,
        amount: f32,
        first_ms: u64,
        period_ms: u64,
    ) -> AfflictionTick {
        AfflictionTick {
            effect: lowercase_crc32(effect),
            gains: vec![gain(kind, amount)],
            first_ms,
            period_ms,
        }
    }

    fn gain(kind: &str, amount: f32) -> AfflictionGain {
        AfflictionGain {
            kind: lowercase_crc32(kind),
            amount,
        }
    }

    fn share(effect: &str, kind: &str, value: f32) -> EffectShare {
        EffectShare {
            effect: lowercase_crc32(effect),
            kind: lowercase_crc32(kind),
            value,
        }
    }

    /// The retail affliction rules of build 25810808: erosion cut to one level step up and
    /// down, blight and curse.
    fn rules() -> AfflictionRules {
        let crc = lowercase_crc32;
        let applied = |holder: &str, being: &str, owned: &str, apply: &[&str]| EffectAbility {
            id: holder.to_owned(),
            holder: crc(holder),
            trigger: EffectTrigger::Applied(Some(crc(being))),
            owned: Some(crc(owned)),
            not_owned: None,
            remove: vec![crc(owned), crc(being)]
                .into_iter()
                .filter(|removed| *removed != crc(AFFLICTED))
                .collect(),
            apply: apply.iter().map(|id| (*id).to_owned()).collect(),
        };
        AfflictionRules {
            kinds: vec![
                kind(
                    CURSE,
                    CURSE_ID,
                    (0.0, BLIGHT_TOP),
                    (3_100, 2.5, 890),
                    true,
                    CURSED,
                ),
                kind(
                    BLIGHT,
                    BLIGHT_ID,
                    (0.0, BLIGHT_TOP),
                    (BLIGHT_HOLD_MS, BLIGHT_DRAIN_PER_S, BLIGHT_FULL_MS),
                    false,
                    BLIGHTED,
                ),
                kind(
                    EROSION,
                    INT_ID,
                    (MAX, 0.0),
                    (HOLD_MS, 0.0, FULL_MS),
                    false,
                    AFFLICTED,
                ),
            ],
            global_kinds: Vec::new(),
            ticks: vec![
                tick(APPLY, EROSION, HIT_GAIN, HIT_FIRST_MS, HIT_PERIOD_MS),
                tick(SHRINE_EFFECT, EROSION, -100.0, 0, ABSORB_PERIOD_MS),
                tick(ABSORB, EROSION, ABSORB_FALL, 0, ABSORB_PERIOD_MS),
                tick(GAS, BLIGHT, GAS_GAIN, 0, GAS_PERIOD_MS),
            ],
            absorptions: vec![
                share(AFFLICTED, EROSION, 1.0),
                share(MEDIUM_LOAD, EROSION, MEDIUM_SHARE),
            ],
            resistances: vec![share(CLOAK, BLIGHT, 1.0)],
            perks: std::collections::HashMap::from([(
                WARD_PERK.to_lowercase(),
                PerkShares {
                    scale: GearScoreScale::of_cell(Some(WARD_SCALE)),
                    shares: vec![(
                        crc(BLIGHT),
                        AffixShare {
                            resistance: WARD_RESISTANCE,
                            absorption: WARD_ABSORPTION,
                        },
                    )],
                },
            )]),
            equips: vec![(
                crc(ROOT),
                [LEVEL_1, LEVEL_2, LV0, CHECK_1]
                    .iter()
                    .map(|id| (*id).to_owned())
                    .collect(),
            )],
            abilities: vec![
                EffectAbility {
                    id: "AbilityID_Catacombs_OnHitTaken".to_owned(),
                    holder: crc(ROOT),
                    trigger: EffectTrigger::HitTaken,
                    owned: None,
                    not_owned: None,
                    remove: Vec::new(),
                    apply: vec![ON_HIT.to_owned(), APPLY.to_owned()],
                },
                applied(LEVEL_1, AFFLICTED, LV0, &[LV1_TEMP, LV1]),
                applied(LEVEL_2, AFFLICTED, LV1, &[LV2_TEMP, LV2]),
                applied(CHECK_1, SHRINE_EFFECT, LV1, &[LV0]),
            ],
            gathered: vec![(crc(SHRINE), SHRINE_EFFECT.to_owned())],
            hits: std::collections::HashMap::from([
                (
                    RowKey::of(BOSS_TABLE, BOSS_SLASH),
                    gain(EROSION, BOSS_SLASH_GAIN),
                ),
                (
                    RowKey::of(BOSS_TABLE, BLIGHT_ROW),
                    gain(BLIGHT, BLIGHT_ROW_GAIN),
                ),
                (
                    RowKey::of(BOSS_TABLE, CURSE_ROW),
                    gain(CURSE, CURSE_ROW_GAIN),
                ),
            ]),
            uses: std::collections::HashMap::from([(
                TINCTURE.to_lowercase(),
                vec![gain(BLIGHT, TINCTURE_FALL)],
            )]),
        }
    }

    /// A world with one admitted player and the affliction effects' rows (`Erosion`, `Blight`
    /// and the level indicators without an end, the level debuffs for 10 s, the hit's effect
    /// for 0.75 s, the shrine's for 1 s, the potion's for 10 s, the gas for 20 s, the curse for
    /// 0.1 s).
    fn world() -> (SimWorld, Entity) {
        let mut world = world_with_spawns(Vec::new());
        let content = std::sync::Arc::make_mut(&mut world.content);
        content.afflictions = rules();
        let timed = [
            (AFFLICTED, -1.0),
            (BLIGHTED, -1.0),
            (CURSED, 0.1),
            (APPLY, 0.75),
            (SHRINE_EFFECT, 1.0),
            (ABSORB, 10.0),
            (LV1_TEMP, 10.0),
            (LV2_TEMP, 10.0),
            (GAS, 20.0),
        ];
        let rows: Vec<_> = [
            ROOT,
            ON_HIT,
            LV0,
            LV1,
            LV2,
            LEVEL_1,
            LEVEL_2,
            CHECK_1,
            MEDIUM_LOAD,
            CLOAK,
        ]
        .iter()
        .map(|id| effect_row(id, -1.0))
        .chain(timed.iter().map(|(id, duration)| effect_row(id, *duration)))
        .collect();
        content.status_effects = StatusEffectRules::from_tables(&nw_content::Tables {
            status_effect_data: vec![nw_content::table::Table {
                source_path: String::new(),
                unique_id: String::new(),
                rows,
            }],
            ..nw_content::Tables::default()
        });
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
        let entity = world.player_by_number(PLAYER).expect("player");
        (world, entity)
    }

    fn holds(world: &SimWorld, entity: Entity, id: &str) -> bool {
        world.holds_effect(entity, lowercase_crc32(id))
    }

    fn add_effect(world: &mut SimWorld, entity: Entity, id: &str) {
        let rules = std::sync::Arc::clone(&world.content);
        let now = world.server_ms;
        world
            .world
            .get_mut::<Player>(entity)
            .expect("player")
            .status_effects
            .add_by_id(&rules.status_effects, id, PLAIN_POTENCY, PLAYER, now);
    }

    /// The value of `entity`'s meter of `kind` now; 0 without one.
    fn value(world: &SimWorld, entity: Entity, kind: &str) -> f32 {
        let rules = &world.content.afflictions;
        let kind = rules.kind(lowercase_crc32(kind)).expect("kind");
        world
            .world
            .get::<Player>(entity)
            .expect("player")
            .afflictions
            .meters
            .iter()
            .find(|meter| meter.kind == kind.crc)
            .map_or(0.0, |meter| meter.value_at(kind, world.server_ms))
    }

    fn take(world: &mut SimWorld, entity: Entity) -> Option<AfflictionLists> {
        world
            .world
            .get_mut::<Player>(entity)
            .expect("player")
            .afflictions
            .take_lists()
    }

    fn at(world: &mut SimWorld, ms: u64) {
        world.server_ms = ms;
        world.tick_afflictions();
    }

    fn hot_items(lists: &AfflictionLists) -> Vec<(u8, HotLong)> {
        match &lists.hot {
            AfflictionList::Full { items, .. } => items.clone(),
            AfflictionList::Items(_) => Vec::new(),
        }
    }

    fn hot_item(lists: &AfflictionLists) -> Option<HotLong> {
        hot_items(lists).first().map(|(_, long)| *long)
    }

    fn cold_items(lists: &AfflictionLists) -> Option<Vec<(u8, ColdLong)>> {
        match &lists.cold {
            Some(AfflictionList::Full { items, .. }) => Some(items.clone()),
            Some(AfflictionList::Items(_)) | None => None,
        }
    }

    fn ns(ms: u64) -> u64 {
        ms * NS_PER_MS
    }

    fn hit_row(world: &mut SimWorld, entity: Entity, row: &str) {
        world.affliction_hit_by(entity, Some(RowKey::of(BOSS_TABLE, row)));
    }

    #[test]
    fn only_the_erosion_chest_and_shrine_are_erosion_props() {
        assert!(
            EROSION_PROP_SLICES
                .iter()
                .all(|slice| is_erosion_prop(slice))
        );
        assert!(!is_erosion_prop(
            "slices/catacombs/gameplay/gatherables/catacombs_gatherable_chest_cursedsilver_s.dynamicslice"
        ));
    }

    #[test]
    fn a_joining_player_holds_the_root_and_level_zero_once() {
        let (mut world, entity) = world();
        world.erosion_joined(entity);
        for id in [ROOT, LEVEL_1, LEVEL_2, LV0, CHECK_1] {
            assert!(holds(&world, entity, id), "{id}");
        }
        world
            .world
            .get_mut::<Player>(entity)
            .expect("player")
            .status_effects
            .remove(lowercase_crc32(LV0));
        world.erosion_joined(entity);
        assert!(!holds(&world, entity, LV0), "a second join adds nothing");
    }

    #[test]
    fn a_hit_adds_two_at_its_first_tick_and_the_lists_show_it() {
        let (mut world, entity) = world();
        world.erosion_joined(entity);
        let start = world.server_ms;
        world.erosion_hit_taken(entity);
        assert!(holds(&world, entity, ON_HIT) && holds(&world, entity, APPLY));
        assert_eq!(
            take(&mut world, entity),
            None,
            "no gain before the first tick"
        );
        at(&mut world, start + HIT_FIRST_MS);
        let lists = take(&mut world, entity).expect("lists");
        let first = start + HIT_FIRST_MS;
        assert_eq!(
            hot_item(&lists),
            Some(HotLong::line(
                HIT_GAIN,
                HIT_GAIN,
                ns(first),
                ns(first + HOLD_MS)
            ))
        );
        assert_eq!(
            lists.cold,
            Some(AfflictionList::Full {
                id: AfflictionId::Wide(1),
                items: vec![(INT_ID, ColdLong::of(MAX, INT_ID, false))],
            })
        );
        // The 0.75 s effect has no second tick.
        at(&mut world, start + HIT_FIRST_MS + HIT_PERIOD_MS);
        assert_eq!(value(&world, entity, EROSION), HIT_GAIN);
        world.erosion_hit_taken(entity);
        at(&mut world, start + 2 * HIT_PERIOD_MS);
        let lists = take(&mut world, entity).expect("second gain");
        assert_eq!(lists.cold, None, "the cold item is shown already");
    }

    #[test]
    fn a_full_meter_moves_the_level_up_one_step_then_goes_after_ten_seconds() {
        let (mut world, entity) = world();
        world.erosion_joined(entity);
        let mut now = world.server_ms;
        for _ in 0..HITS_TO_FILL {
            world.erosion_hit_taken(entity);
            now += HIT_PERIOD_MS;
            at(&mut world, now);
        }
        let lists = take(&mut world, entity).expect("full meter");
        let line = hot_item(&lists).expect("line");
        assert_eq!(line.d - line.c, ns(FULL_MS));
        assert!(cold_items(&lists).is_some_and(|items| items[0].1.flag));
        assert!(holds(&world, entity, AFFLICTED));
        assert!(holds(&world, entity, LV1) && holds(&world, entity, LV1_TEMP));
        assert!(!holds(&world, entity, LV0) && !holds(&world, entity, LV2));
        // A hit while full adds nothing: `Erosion` absorbs the whole gain.
        world.erosion_hit_taken(entity);
        at(&mut world, now + HIT_PERIOD_MS);
        assert_eq!(take(&mut world, entity), None);
        let filled = now - HIT_PERIOD_MS + HIT_FIRST_MS;
        at(&mut world, filled + FULL_MS);
        let gone = take(&mut world, entity).expect("gone");
        assert!(hot_items(&gone).is_empty());
        assert!(!holds(&world, entity, AFFLICTED));
        assert!(holds(&world, entity, LV1), "the level stays");
        assert_eq!(value(&world, entity, EROSION), 0.0);
    }

    #[test]
    fn the_shrine_empties_the_meter_and_moves_the_level_down() {
        let (mut world, entity) = world();
        world.erosion_joined(entity);
        let mut now = world.server_ms;
        for _ in 0..HITS_TO_FILL {
            world.erosion_hit_taken(entity);
            now += HIT_PERIOD_MS;
            at(&mut world, now);
        }
        at(&mut world, now + FULL_MS);
        world.erosion_hit_taken(entity);
        at(&mut world, now + FULL_MS + HIT_PERIOD_MS);
        take(&mut world, entity);
        world.erosion_gathered(PLAYER, SHRINE);
        let gone = take(&mut world, entity).expect("emptied");
        assert!(hot_items(&gone).is_empty());
        assert!(holds(&world, entity, LV0) && !holds(&world, entity, LV1));
        assert!(
            !holds(&world, entity, SHRINE_EFFECT),
            "the check removes the shrine's effect"
        );
        world.erosion_gathered(PLAYER, "Catacombs_Other");
        assert_eq!(take(&mut world, entity), None);
    }

    /// Retail `CATACOMBS2` 303.897 s: the potion takes the meter from 27 to 7 at once and
    /// 304.864 s the lists are empty. The own group takes the lists at every tick, so the
    /// client holds the meter's cold item before the potion.
    #[test]
    fn a_potion_another_part_adds_falls_twenty_a_second_from_the_next_tick() {
        let (mut world, entity) = world();
        world.erosion_joined(entity);
        let mut now = world.server_ms;
        for _ in 0..HITS_TO_FILL - 1 {
            world.erosion_hit_taken(entity);
            now += HIT_PERIOD_MS;
            at(&mut world, now);
        }
        take(&mut world, entity).expect("the gains");
        let gained = value(&world, entity, EROSION);
        world.server_ms = now;
        add_effect(&mut world, entity, ABSORB);
        at(&mut world, now);
        assert_eq!(value(&world, entity, EROSION), gained + ABSORB_FALL);
        at(&mut world, now + ABSORB_PERIOD_MS);
        assert_eq!(value(&world, entity, EROSION), 0.0);
        let gone = take(&mut world, entity).expect("emptied");
        assert_eq!(cold_items(&gone), Some(Vec::new()));
    }

    #[test]
    fn a_medium_load_takes_a_quarter_of_each_gain() {
        /// Retail `CATACOMBS2`: +1.5 a hit on a medium load.
        const MEDIUM_HIT: f32 = 1.5;
        /// Retail `CATACOMBS3`: a mob row's 3 on a medium load.
        const MEDIUM_SLASH: f32 = 2.25;
        let (mut world, entity) = world();
        world.erosion_joined(entity);
        let start = world.server_ms;
        add_effect(&mut world, entity, MEDIUM_LOAD);
        world.erosion_hit_taken(entity);
        at(&mut world, start + HIT_FIRST_MS);
        assert_eq!(value(&world, entity, EROSION), MEDIUM_HIT);
        hit_row(&mut world, entity, BOSS_SLASH);
        assert_eq!(value(&world, entity, EROSION), MEDIUM_HIT + MEDIUM_SLASH);
    }

    #[test]
    fn a_mob_row_with_erosion_adds_its_affliction_at_the_hit_and_others_add_nothing() {
        /// Hits of the row that fill the meter: 14 x 3 = 42.
        const SLASHES_TO_FILL: usize = 14;
        let (mut world, entity) = world();
        world.erosion_joined(entity);
        hit_row(&mut world, entity, BOSS_SLASH);
        assert_eq!(value(&world, entity, EROSION), BOSS_SLASH_GAIN);
        let lists = take(&mut world, entity).expect("lists");
        assert_eq!(
            hot_item(&lists).map(|line| line.a),
            Some(HotLong::line(BOSS_SLASH_GAIN, BOSS_SLASH_GAIN, 0, 0).a)
        );
        hit_row(&mut world, entity, "Attack_Elite_Kick");
        world.affliction_hit_by(entity, None);
        assert_eq!(value(&world, entity, EROSION), BOSS_SLASH_GAIN);
        for _ in 1..SLASHES_TO_FILL {
            hit_row(&mut world, entity, BOSS_SLASH);
        }
        assert!(holds(&world, entity, AFFLICTED), "a full meter afflicts");
        assert!(holds(&world, entity, LV1));
    }

    #[test]
    fn the_lists_go_into_a_vitals_member_the_group_holds() {
        let (mut world, entity) = world();
        world.erosion_joined(entity);
        let start = world.server_ms;
        world.erosion_hit_taken(entity);
        at(&mut world, start + HIT_FIRST_MS);
        let lists = take(&mut world, entity).expect("lists");
        let mut members = vec![BundleMember {
            component: PLAYER_VITALS_COMPONENT,
            state: MemberState::Vitals(VitalsState {
                slot_0: None,
                slot_1: Some(VitalsSlot1 {
                    mana_amount: Some(MAX),
                    ..VitalsSlot1::default()
                }),
            }),
        }];
        put_afflictions(&mut members, lists.clone());
        let MemberState::Vitals(vitals) = &members[0].state else {
            panic!("vitals");
        };
        let slot = vitals.slot_1.as_ref().expect("slot 1");
        assert_eq!(slot.mana_amount, Some(MAX));
        assert_eq!(slot.afflictions_hot.as_ref(), Some(&lists.hot));
        let mut alone = Vec::new();
        put_afflictions(&mut alone, lists);
        assert_eq!(alone.len(), 1);
    }

    /// Retail `CATACOMBS6` 1310.842 s to 1314.806 s: 5, 10, 15, 20, 25, each held 1.1 s, item
    /// 8 of top 60.
    #[test]
    fn the_gas_a_mob_puts_on_fills_the_blight_five_a_second_to_a_top_of_its_resistance() {
        let (mut world, entity) = world();
        let start = world.server_ms;
        add_effect(&mut world, entity, GAS);
        at(&mut world, start);
        let lists = take(&mut world, entity).expect("first gain");
        assert_eq!(
            hot_items(&lists),
            [(
                BLIGHT_ID,
                HotLong::line(GAS_GAIN, GAS_GAIN, ns(start), ns(start + BLIGHT_HOLD_MS))
            )]
        );
        assert_eq!(
            cold_items(&lists),
            Some(vec![(
                BLIGHT_ID,
                ColdLong::of(BLIGHT_TOP, BLIGHT_ID, false)
            )])
        );
        at(&mut world, start + GAS_PERIOD_MS);
        assert_eq!(value(&world, entity, BLIGHT), 2.0 * GAS_GAIN);
    }

    /// Retail `CATACOMBS6` 1315.939 s: 25 held, then 25 to 0 over 25 s; Garden of Genesis
    /// 65.459 s: both lists empty when it reaches 0.
    #[test]
    fn a_held_meter_falls_at_its_rate_after_its_hold_and_goes_at_zero() {
        let (mut world, entity) = world();
        let start = world.server_ms;
        hit_row(&mut world, entity, BLIGHT_ROW);
        take(&mut world, entity);
        at(&mut world, start + BLIGHT_HOLD_MS - 1);
        assert_eq!(take(&mut world, entity), None, "still held");
        let fall = start + BLIGHT_HOLD_MS;
        at(&mut world, fall);
        let lists = take(&mut world, entity).expect("the fall");
        let fall_ms = (BLIGHT_ROW_GAIN / BLIGHT_DRAIN_PER_S * MS_PER_S) as u64;
        assert_eq!(
            hot_item(&lists),
            Some(HotLong::line(
                BLIGHT_ROW_GAIN,
                0.0,
                ns(fall),
                ns(fall + fall_ms)
            ))
        );
        assert_eq!(lists.cold, None);
        at(&mut world, fall + fall_ms);
        let gone = take(&mut world, entity).expect("gone");
        assert!(hot_items(&gone).is_empty());
        assert_eq!(cold_items(&gone), Some(Vec::new()));
    }

    /// Retail Hive Genesis 673.078 s full at 60 for 180 s, 759.155 s +2 on 43.2 gives 45.156
    /// over 135.5 s.
    #[test]
    fn a_full_blight_afflicts_and_a_gain_starts_its_fall_again_from_the_new_value() {
        /// Half of the full time: the meter is at 30.
        const HALF_MS: u64 = BLIGHT_FULL_MS / 2;
        let (mut world, entity) = world();
        let start = world.server_ms;
        for _ in 0..3 {
            hit_row(&mut world, entity, BLIGHT_ROW);
        }
        assert!(holds(&world, entity, BLIGHTED));
        let lists = take(&mut world, entity).expect("full");
        assert_eq!(
            hot_items(&lists),
            [(
                BLIGHT_ID,
                HotLong::line(BLIGHT_TOP, 0.0, ns(start), ns(start + BLIGHT_FULL_MS))
            )]
        );
        assert_eq!(
            cold_items(&lists),
            Some(vec![(BLIGHT_ID, ColdLong::of(BLIGHT_TOP, BLIGHT_ID, true))])
        );
        world.server_ms = start + HALF_MS;
        hit_row(&mut world, entity, BLIGHT_ROW);
        let refreshed = BLIGHT_TOP / 2.0 + BLIGHT_ROW_GAIN;
        let lists = take(&mut world, entity).expect("refreshed");
        let fall_ms = (refreshed / BLIGHT_TOP * BLIGHT_FULL_MS as f32).round() as u64;
        assert_eq!(
            hot_item(&lists),
            Some(HotLong::line(
                refreshed,
                0.0,
                ns(start + HALF_MS),
                ns(start + HALF_MS + fall_ms)
            ))
        );
        assert_eq!(lists.cold, None, "still full");
        at(&mut world, start + HALF_MS + fall_ms);
        assert!(take(&mut world, entity).is_some_and(|gone| hot_items(&gone).is_empty()));
        assert!(!holds(&world, entity, BLIGHTED));
    }

    /// Retail curse (four ledgers): a full meter's lists empty 0.12 s to 0.13 s after the fill,
    /// when its 0.1 s `Curse` ends.
    #[test]
    fn a_full_curse_takes_no_gain_and_goes_when_its_effect_ends() {
        let (mut world, entity) = world();
        let start = world.server_ms;
        hit_row(&mut world, entity, CURSE_ROW);
        assert!(holds(&world, entity, CURSED));
        take(&mut world, entity);
        hit_row(&mut world, entity, CURSE_ROW);
        assert_eq!(take(&mut world, entity), None, "`IgnoreWhenAfflicted`");
        world
            .world
            .get_mut::<Player>(entity)
            .expect("player")
            .status_effects
            .remove(lowercase_crc32(CURSED));
        at(&mut world, start + 1);
        let gone = take(&mut world, entity).expect("gone");
        assert!(hot_items(&gone).is_empty());
    }

    /// Retail `CATACOMBS6` 1325.775 s: blight 8, then erosion 15, in one full list.
    #[test]
    fn two_meters_go_in_one_list_in_key_order() {
        let (mut world, entity) = world();
        hit_row(&mut world, entity, BOSS_SLASH);
        hit_row(&mut world, entity, BLIGHT_ROW);
        let lists = take(&mut world, entity).expect("lists");
        let keys: Vec<u8> = hot_items(&lists).iter().map(|(key, _)| *key).collect();
        assert_eq!(keys, [BLIGHT_ID, INT_ID]);
        assert_eq!(
            cold_items(&lists).map(|items| items.len()),
            Some(keys.len())
        );
    }

    /// Retail `CATACOMBS6` 1400.600 s death, 1400.638 s both lists empty.
    #[test]
    fn a_death_ends_every_meter_and_the_full_meters_effects() {
        let (mut world, entity) = world();
        hit_row(&mut world, entity, BOSS_SLASH);
        for _ in 0..3 {
            hit_row(&mut world, entity, BLIGHT_ROW);
        }
        take(&mut world, entity);
        {
            let mut vitals = world.world.get_mut::<Vitals>(entity).expect("vitals");
            let max = vitals.health.max();
            vitals.health = Health::at(0, max);
        }
        let now = world.server_ms;
        at(&mut world, now);
        let gone = take(&mut world, entity).expect("gone");
        assert!(hot_items(&gone).is_empty());
        assert_eq!(cold_items(&gone), Some(Vec::new()));
        assert!(!holds(&world, entity, BLIGHTED));
        at(&mut world, now + 1);
        assert_eq!(take(&mut world, entity), None, "once");
    }

    #[test]
    fn a_held_resistance_raises_the_top() {
        let (mut world, entity) = world();
        add_effect(&mut world, entity, CLOAK);
        hit_row(&mut world, entity, BLIGHT_ROW);
        let lists = take(&mut world, entity).expect("lists");
        assert_eq!(
            cold_items(&lists),
            Some(vec![(
                BLIGHT_ID,
                ColdLong::of(BLIGHT_TOP + 1.0, BLIGHT_ID, false)
            )])
        );
    }

    #[test]
    fn a_worn_resist_perk_raises_the_top_and_takes_a_share_of_each_gain_by_its_gear_score() {
        use crate::world::gear::tests::{gear_with_staff, kits};
        let (mut world, entity) = world();
        let mut chest = kits()
            .item("MysticArchetypeChest")
            .expect("the kit's chest")
            .clone();
        chest.gear_score = WARD_GEAR_SCORE;
        chest.perk_ids = vec![WARD_PERK.to_owned()];
        let mut gear = gear_with_staff();
        gear.wear_new(&chest, nw_messages::paperdoll::slot::CHEST)
            .expect("worn");
        world.world.get_mut::<Player>(entity).expect("player").gear = Some(gear);
        hit_row(&mut world, entity, BLIGHT_ROW);
        let potency = GearScoreScale::of_cell(Some(WARD_SCALE)).potency(WARD_GEAR_SCORE) as f32;
        let top = BLIGHT_TOP + WARD_RESISTANCE * potency;
        let lists = take(&mut world, entity).expect("lists");
        assert_eq!(
            cold_items(&lists),
            Some(vec![(BLIGHT_ID, ColdLong::of(top, BLIGHT_ID, false))])
        );
        let gained = BLIGHT_ROW_GAIN * (1.0 - WARD_ABSORPTION * potency);
        assert!((value(&world, entity, BLIGHT) - gained).abs() < 1e-4);
    }

    #[test]
    fn a_tincture_ends_a_full_meter_and_its_effect() {
        let (mut world, entity) = world();
        for _ in 0..3 {
            hit_row(&mut world, entity, BLIGHT_ROW);
        }
        take(&mut world, entity);
        world.affliction_used(entity, "PotionHeal");
        assert_eq!(take(&mut world, entity), None, "no affliction on its use");
        world.affliction_used(entity, TINCTURE);
        let gone = take(&mut world, entity).expect("gone");
        assert!(hot_items(&gone).is_empty());
        assert!(!holds(&world, entity, BLIGHTED));
    }

    /// Retail `CATACOMBS6` 1310.842 s to 1312.942 s: a player in the Fulgoris gas (0.5 s
    /// delays) gets `AI_Fulgoris_Gas` again every 0.5 s, and its blight ticks keep their 1 s
    /// beat: 5, 10, 15. The effect stays after the player leaves the gas.
    #[test]
    fn a_gas_area_gives_its_effect_again_and_its_blight_keeps_its_beat() {
        use nw_world::collision::{Geometry, IDENTITY_AFFINE};
        use nw_world::dungeon_scripts::Detector;
        use nw_world::effect_areas::{AreaRules, EffectArea};
        /// A sphere around the whole test world, metres.
        const WIDE_M: f32 = 100_000.0;
        const STEP_MS: u64 = 100;
        const DELAY_MS: u64 = 500;
        let (mut world, entity) = world();
        let gas = EffectArea {
            effects: vec![GAS.to_owned()],
            removes: Vec::new(),
            volume: Detector {
                shape: Geometry::Sphere { radius: WIDE_M },
                place: IDENTITY_AFFINE,
            },
            rules: AreaRules {
                remove_on_exit: false,
                initial_delay_ms: DELAY_MS,
                reapply_ms: DELAY_MS,
            },
        };
        std::sync::Arc::make_mut(&mut world.content)
            .effect_areas
            .levels
            .insert(String::new(), vec![gas]);
        let start = world.server_ms;
        let mut applications = Vec::new();
        for now in (start..=start + DELAY_MS + 2 * GAS_PERIOD_MS).step_by(STEP_MS as usize) {
            world.server_ms = now;
            world.tick_effect_areas();
            world.tick_afflictions();
            let held = world
                .world
                .get::<Player>(entity)
                .and_then(|player| player.status_effects.applications(lowercase_crc32(GAS)));
            if applications.last() != Some(&held) {
                applications.push(held);
            }
        }
        assert_eq!(
            applications.first(),
            Some(&None),
            "nothing before the delay"
        );
        assert_eq!(
            applications.len(),
            6,
            "applications 1 to 5: {applications:?}"
        );
        assert_eq!(value(&world, entity, BLIGHT), 3.0 * GAS_GAIN);
        std::sync::Arc::make_mut(&mut world.content)
            .effect_areas
            .levels
            .clear();
        world.tick_effect_areas();
        assert!(holds(&world, entity, GAS), "the gas's effect stays");
    }
}
