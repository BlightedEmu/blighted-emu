// SPDX-License-Identifier: AGPL-3.0-or-later
//! When the tutorial's asset entities come, change and go, from the retail causes of the level
//! table (ftue-level-entities note).
//!
//! Every group of an asset entity in the table names the nearest retail event before it and the
//! delay from it. The script plays each group once, the same delay after the same event in this
//! session:
//!
//! - an achievement the server sets (the expansion at login, the tutorial's unlocks);
//! - a client call (a video start, the quick-time lesson, a cutscene request, ...; a call with a
//!   `u32` id matches only that id);
//! - the player passing a static entity's trigger area (560), by the entity's `GdeRef` start;
//! - the death of a mob, by its place in the retail spawn order: retail's wave-ending changes
//!   follow the death of the wave's last mob, so the event is the death of the last mob of the
//!   batch that the mob spawned with.
//!
//! A spawn marker (the AI spawner slice) comes with the mob it marks: retail sends it at most
//! [`MARKER_LEAD_MS`] before the mob's card. An entity carded before the move into the water goes
//! with the move (retail stops them all then); another goes the retail time after its spawn. An
//! entity without a position is a spell slice of a mob's attack, not of the level: it plays
//! when its mob casts it ([`AssetScript::cast`]).

use nw_content::ftue_entities::{FtueLevel, GDE_REF_PREFIX_LEN, GroupCause, LevelEvent};
use nw_content::mob_prefabs::ASSET_ID_LEN;

/// The client calls of the retail causes.
const PASSTHROUGH: u64 = 560;
/// Bytes of a call id that the script compares (the `u32` of a video, cutscene or lesson).
const ID_BYTES: usize = 4;
/// Most time from a spawn marker to the card of the mob it marks (retail 0 to 64 ms).
const MARKER_LEAD_MS: u64 = 100;
/// When the table's capture moved the player into the water.
const RETAIL_WATER_MOVE_MS: u64 = 118_546;

/// Something that happened in this session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Trigger {
    /// The tutorial started.
    Start,
    /// The server set this achievement.
    Achievement(u32),
    /// The client sent this call, with its `u32` id when it has one.
    Client {
        /// The type index.
        message_type: u64,
        /// The id.
        id: Option<u32>,
    },
    /// The player passed the trigger area of the static entity with this `GdeRef` start.
    Passthrough([u8; GDE_REF_PREFIX_LEN]),
    /// The last mob of the batch of this retail mob died.
    BatchDead(usize),
    /// This retail mob spawned.
    MobSpawned(usize),
    /// The player moved into the water.
    WaterMove,
}

/// One cast of a mob's spell slice: the caster, where it aims (x and y, the bits of the `f32`
/// metres) and the cast's number, which makes its `GdeRef` new.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SpellCast {
    pub(super) caster: u64,
    pub(super) at_bits: [u32; 2],
    pub(super) number: u32,
}

/// What a planned group does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    /// The entity comes into view.
    Show,
    /// The spell slice comes into view for this cast.
    Cast(SpellCast),
    /// Its change group of this index goes out.
    Change(usize),
    /// It leaves the view.
    Hide,
}

/// When a planned group goes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum When {
    Start,
    /// Any of these achievements.
    Achievements(Vec<u32>),
    Client {
        message_type: u64,
        id: Option<u32>,
    },
    Passthrough([u8; GDE_REF_PREFIX_LEN]),
    BatchDead(usize),
    MobSpawned(usize),
    WaterMove,
}

impl When {
    fn of(cause: Option<&GroupCause>) -> Self {
        let Some(cause) = cause else {
            return Self::Start;
        };
        match &cause.event {
            LevelEvent::Achievements { bits } => Self::Achievements(bits.clone()),
            LevelEvent::Client { message_type, id } if *message_type == PASSTHROUGH => {
                let mut prefix = [0; GDE_REF_PREFIX_LEN];
                match id.get(..GDE_REF_PREFIX_LEN) {
                    Some(start) => {
                        prefix.copy_from_slice(start);
                        Self::Passthrough(prefix)
                    }
                    None => Self::Start,
                }
            }
            LevelEvent::Client { message_type, id } => Self::Client {
                message_type: *message_type,
                id: <[u8; ID_BYTES]>::try_from(id.as_slice())
                    .ok()
                    .map(u32::from_be_bytes),
            },
            LevelEvent::Mob { mob, .. } => Self::BatchDead(*mob),
        }
    }

    fn matches(&self, trigger: Trigger) -> bool {
        match (self, trigger) {
            (Self::Start, Trigger::Start) | (Self::WaterMove, Trigger::WaterMove) => true,
            (Self::Achievements(bits), Trigger::Achievement(bit)) => bits.contains(&bit),
            (
                Self::Client { message_type, id },
                Trigger::Client {
                    message_type: seen,
                    id: seen_id,
                },
            ) => *message_type == seen && (id.is_none() || *id == seen_id),
            (Self::Passthrough(prefix), Trigger::Passthrough(seen)) => *prefix == seen,
            (Self::BatchDead(mob), Trigger::BatchDead(seen))
            | (Self::MobSpawned(mob), Trigger::MobSpawned(seen)) => *mob == seen,
            _ => false,
        }
    }
}

/// One group of one entity, waiting for its event.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Plan {
    asset: usize,
    step: Step,
    when: When,
    delay_ms: u64,
    done: bool,
}

/// A spell slice of the level table: when it came in the capture, when its changes came and
/// when it left.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Spell {
    asset: usize,
    asset_id: [u8; ASSET_ID_LEN],
    spawn_ms: u64,
    change_ms: Vec<u64>,
    gone_ms: Option<u64>,
}

/// The asset entity groups of one player's tutorial.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AssetScript {
    plans: Vec<Plan>,
    spells: Vec<Spell>,
    /// Groups whose event happened, with their times.
    due: Vec<(u64, usize, Step)>,
}

impl AssetScript {
    /// The plans of the asset entities of `level`.
    pub(super) fn of(level: &FtueLevel) -> Self {
        let mut plans = Vec::new();
        let mut spells = Vec::new();
        for (asset, entity) in level.assets.iter().enumerate() {
            let Some(spawn) = entity.spawns.first() else {
                continue;
            };
            if entity.position.is_none() {
                spells.push(Spell {
                    asset,
                    asset_id: entity.asset,
                    spawn_ms: spawn.group.time_ms,
                    change_ms: entity
                        .changes
                        .iter()
                        .map(|change| change.group.time_ms)
                        .collect(),
                    gone_ms: entity.despawns.first().copied(),
                });
                continue;
            }
            let spawn_ms = spawn.group.time_ms;
            let marked = level.mobs.iter().position(|mob| {
                mob.first_card_ms >= spawn_ms && mob.first_card_ms - spawn_ms <= MARKER_LEAD_MS
            });
            let (when, delay_ms) = match marked {
                Some(mob) => (When::MobSpawned(mob), 0),
                None => (
                    When::of(spawn.cause.as_ref()),
                    spawn.cause.as_ref().map_or(0, |cause| cause.delay_ms),
                ),
            };
            for (change, group) in entity.changes.iter().enumerate() {
                plans.push(Plan {
                    asset,
                    step: Step::Change(change),
                    when: When::of(group.cause.as_ref()),
                    delay_ms: group.cause.as_ref().map_or(0, |cause| cause.delay_ms),
                    done: false,
                });
            }
            let last_spawn_ms = entity
                .spawns
                .last()
                .map_or(spawn_ms, |last| last.group.time_ms);
            // A stop that a later change of the same entity follows is the capture's player
            // leaving and coming back into view, not a step of the level (retail: the Thorpe
            // encounter, asset 14, stopped at 303772 and was carded again at 309357; its change
            // at bit 15206 + 2316 ms is what makes the client ask for the boss cutscene).
            let last_change_ms = entity.changes.iter().map(|change| change.group.time_ms).max();
            let level_stop = entity.despawns.last().filter(|gone| {
                **gone > last_spawn_ms && last_change_ms.is_none_or(|change| change <= **gone)
            });
            if let Some(gone_ms) = level_stop {
                // The hide follows the last change before it, so the change goes out first
                // (retail: the boss area trigger's change at 15206 + 2316 ms, its stop 14 ms
                // later; timed from its spawn it went 2.1 s early live and the change was lost).
                let last_change = entity
                    .changes
                    .iter()
                    .rev()
                    .find(|change| change.group.time_ms <= *gone_ms)
                    .and_then(|change| Some((change.group.time_ms, change.cause.as_ref()?)));
                let (hide_when, hide_delay) = if *gone_ms == RETAIL_WATER_MOVE_MS {
                    (When::WaterMove, 0)
                } else if let Some((change_ms, cause)) = last_change {
                    (
                        When::of(Some(cause)),
                        cause.delay_ms + (gone_ms - change_ms),
                    )
                } else {
                    (when.clone(), delay_ms + (gone_ms - spawn_ms))
                };
                plans.push(Plan {
                    asset,
                    step: Step::Hide,
                    when: hide_when,
                    delay_ms: hide_delay,
                    done: false,
                });
            }
            plans.push(Plan {
                asset,
                step: Step::Show,
                when,
                delay_ms,
                done: false,
            });
        }
        Self {
            plans,
            spells,
            due: Vec::new(),
        }
    }

    /// A mob casts the spell slice of asset `asset_id` with `cast` in an attack that started at
    /// `started_ms`; the capture's cast of it started at `retail_started_ms`. The slice comes,
    /// changes and goes the capture's times after that start.
    pub(super) fn cast(
        &mut self,
        asset_id: &[u8; ASSET_ID_LEN],
        cast: SpellCast,
        started_ms: u64,
        retail_started_ms: u64,
    ) {
        let Some(spell) = self.spells.iter().find(|spell| spell.asset_id == *asset_id) else {
            return;
        };
        let at = |retail_ms: u64| started_ms + retail_ms.saturating_sub(retail_started_ms);
        self.due
            .push((at(spell.spawn_ms), spell.asset, Step::Cast(cast)));
        for (change, change_ms) in spell.change_ms.iter().enumerate() {
            self.due
                .push((at(*change_ms), spell.asset, Step::Change(change)));
        }
        if let Some(gone_ms) = spell.gone_ms {
            self.due.push((at(gone_ms), spell.asset, Step::Hide));
        }
    }

    /// `trigger` happened at `now_ms`: the groups that wait for it are due after their delays.
    pub(super) fn on(&mut self, trigger: Trigger, now_ms: u64) {
        let mut fired = Vec::new();
        for plan in self.plans.iter_mut().filter(|plan| !plan.done) {
            if plan.when.matches(trigger) {
                plan.done = true;
                fired.push((now_ms.saturating_add(plan.delay_ms), plan.asset, plan.step));
            }
        }
        for (at_ms, asset, step) in fired {
            if matches!(step, Step::Change(_)) {
                self.show_by(asset, at_ms);
            }
            self.due.push((at_ms, asset, step));
        }
    }

    /// Asset entity `asset` is in view by `at_ms`, when one of its changes goes out. Retail
    /// cards a level entity when the player comes near it, so a card timed from an earlier
    /// event can come after the change that needs it (the boss area trigger, asset 15: timed
    /// 22565 ms after its passthrough, but in the retail run of 2026-09-18 carded 14165 ms after
    /// the passthrough, 1081 ms before the bundle that set bit 15206).
    fn show_by(&mut self, asset: usize, at_ms: u64) {
        let waiting = self
            .plans
            .iter_mut()
            .find(|plan| plan.asset == asset && plan.step == Step::Show && !plan.done);
        if let Some(plan) = waiting {
            plan.done = true;
            self.due.push((at_ms, asset, Step::Show));
            return;
        }
        for (due_ms, due_asset, step) in &mut self.due {
            if *due_asset == asset && *step == Step::Show && *due_ms > at_ms {
                *due_ms = at_ms;
            }
        }
    }

    /// The groups due at `now_ms`, in time order: (asset entity, step).
    pub(super) fn take_due(&mut self, now_ms: u64) -> Vec<(usize, Step)> {
        let (mut due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.due)
            .into_iter()
            .partition(|(at, _, _)| now_ms >= *at);
        self.due = later;
        // A show goes before the changes due with it, so a change has a shown entity.
        due.sort_by_key(|(at, _, step)| (*at, !matches!(step, Step::Show | Step::Cast(_))));
        due.into_iter()
            .map(|(_, asset, step)| (asset, step))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use nw_content::ftue_entities::{
        AssetEntity, AssetGroup, EntityGroup, FtueEntities, RetailMob,
    };

    use super::*;

    const QTE: u64 = 6638;
    const QTE_ID: u32 = 0xf9d8_0a82;
    const EXPANSION: u32 = 11_979;

    fn group(time_ms: u64, event: Option<LevelEvent>, delay_ms: u64) -> AssetGroup {
        AssetGroup {
            group: EntityGroup {
                time_ms,
                interest: 1,
                members: Vec::new(),
            },
            refs: Vec::new(),
            cause: event.map(|event| GroupCause {
                event,
                event_ms: time_ms - delay_ms,
                delay_ms,
            }),
        }
    }

    fn entity(spawn: AssetGroup, changes: Vec<AssetGroup>, despawns: Vec<u64>) -> AssetEntity {
        AssetEntity {
            asset: [0; 16],
            asset_sub_id: 2,
            retail_gde_ref: [0; 16],
            position: Some([0.0; 3]),
            spawns: vec![spawn],
            changes,
            despawns,
        }
    }

    /// The boatwreck encounter (two changes), a spawn marker, a loot sack that goes 6 s after
    /// the death that brings it, and a spell slice without a position.
    fn level() -> FtueLevel {
        let expansion = || {
            Some(LevelEvent::Achievements {
                bits: vec![EXPANSION],
            })
        };
        let qte = LevelEvent::Client {
            message_type: QTE,
            id: QTE_ID.to_be_bytes().to_vec(),
        };
        let death = |mob| LevelEvent::Mob {
            message_type: 3916,
            mob,
        };
        let mut spell = entity(group(450_000, expansion(), 1), Vec::new(), Vec::new());
        spell.position = None;
        FtueLevel {
            statics: FtueEntities::default(),
            static_causes: Vec::new(),
            assets: vec![
                entity(
                    group(5115, expansion(), 16),
                    vec![
                        group(73_488, Some(qte), 42),
                        group(81_580, Some(death(0)), 6),
                    ],
                    vec![RETAIL_WATER_MOVE_MS],
                ),
                entity(
                    group(16_134, None, 0),
                    Vec::new(),
                    vec![RETAIL_WATER_MOVE_MS],
                ),
                entity(
                    group(209_491, Some(death(3)), 20),
                    Vec::new(),
                    vec![215_759],
                ),
                spell,
            ],
            mobs: vec![RetailMob {
                first_card_ms: 16_198,
                vitals_id: 1,
                asset: [0; 16],
                retail_gde_ref: [0; 16],
            }],
        }
    }

    #[test]
    fn a_cast_spell_plays_at_the_capture_times_after_its_attack_starts() {
        // The capture's attack started 650 ms before the spell's card; the spell changed 1016 ms
        // and left 2483 ms after its card.
        const RETAIL_ATTACK_MS: u64 = 449_350;
        const SPELL: [u8; 16] = [7; 16];
        const STARTED: u64 = 10_000;
        let mut spell = entity(
            group(450_000, None, 0),
            vec![group(451_016, None, 0)],
            vec![452_483],
        );
        spell.position = None;
        spell.asset = SPELL;
        let mut script = AssetScript::of(&FtueLevel {
            statics: FtueEntities::default(),
            static_causes: Vec::new(),
            assets: vec![spell],
            mobs: Vec::new(),
        });
        let cast = SpellCast {
            caster: 9,
            at_bits: [1, 2],
            number: 1,
        };
        script.cast(&[8; 16], cast, STARTED, RETAIL_ATTACK_MS);
        assert!(script.take_due(u64::MAX).is_empty(), "no such spell");
        script.cast(&SPELL, cast, STARTED, RETAIL_ATTACK_MS);
        assert!(script.take_due(STARTED + 649).is_empty());
        assert_eq!(script.take_due(STARTED + 650), [(0, Step::Cast(cast))]);
        assert_eq!(script.take_due(STARTED + 1_666), [(0, Step::Change(0))]);
        assert_eq!(script.take_due(STARTED + 3_133), [(0, Step::Hide)]);
    }

    #[test]
    fn a_hide_after_a_change_waits_for_the_change() {
        // The boss area trigger (asset 15): shown 22565 ms after a passthrough, changed 2316 ms
        // after bit 15206, stopped 14 ms after the change.
        const TF_BIT: u32 = 15_206;
        const PREFIX: [u8; GDE_REF_PREFIX_LEN] = [0xff, 0x78, 0x60, 0x38, 0x81, 0x41, 0x00, 0x84];
        let passthrough = LevelEvent::Client {
            message_type: PASSTHROUGH,
            id: PREFIX.to_vec(),
        };
        let tf = LevelEvent::Achievements { bits: vec![TF_BIT] };
        let trigger = entity(
            group(392_153, Some(passthrough), 22_565),
            vec![group(394_754, Some(tf), 2_316)],
            vec![394_768],
        );
        let mut script = AssetScript::of(&FtueLevel {
            statics: FtueEntities::default(),
            static_causes: Vec::new(),
            assets: vec![trigger],
            mobs: Vec::new(),
        });
        script.on(Trigger::Passthrough(PREFIX), 0);
        assert_eq!(script.take_due(22_565), [(0, Step::Show)]);
        // Live: the bit came 24.97 s after the passthrough.
        script.on(Trigger::Achievement(TF_BIT), 24_967);
        assert!(script.take_due(27_282).is_empty(), "no early hide");
        assert_eq!(
            script.take_due(27_297),
            [(0, Step::Change(0)), (0, Step::Hide)]
        );
    }

    #[test]
    fn a_stop_before_a_later_change_is_a_view_exit_and_hides_nothing() {
        // The Thorpe encounter (asset 14): shown 5433 ms after bit 15551, stopped at 303772 when
        // the capture's player walked out of view, carded again, then changed 2316 ms after
        // bit 15206 (retail 2026-09-18: the change came 2051 ms after the bundle that set that
        // bit, and the client asked for the boss cutscene 228 ms later).
        const CLIMB_BIT: u32 = 15_551;
        const TF_BIT: u32 = 15_206;
        let climb = LevelEvent::Achievements {
            bits: vec![CLIMB_BIT],
        };
        let tf = LevelEvent::Achievements { bits: vec![TF_BIT] };
        let encounter = entity(
            group(292_857, Some(climb), 5_433),
            vec![group(394_754, Some(tf), 2_316)],
            vec![303_772],
        );
        let mut script = AssetScript::of(&FtueLevel {
            statics: FtueEntities::default(),
            static_causes: Vec::new(),
            assets: vec![encounter],
            mobs: Vec::new(),
        });
        script.on(Trigger::Achievement(CLIMB_BIT), 0);
        assert_eq!(script.take_due(5_433), [(0, Step::Show)]);
        assert!(script.take_due(60_000).is_empty(), "no hide at the view exit");
        script.on(Trigger::Achievement(TF_BIT), 100_000);
        assert_eq!(script.take_due(102_316), [(0, Step::Change(0))]);
        assert!(script.take_due(u64::MAX).is_empty());
    }

    #[test]
    fn a_change_brings_its_entity_into_view_first() {
        // The boss area trigger (asset 15) is timed 22565 ms after its passthrough; the retail
        // player of 2026-09-18 reached the boss trigger 15022 ms after it.
        const TF_BIT: u32 = 15_206;
        const PREFIX: [u8; GDE_REF_PREFIX_LEN] = [0xff, 0x78, 0x60, 0x38, 0x81, 0x41, 0x00, 0x84];
        let passthrough = LevelEvent::Client {
            message_type: PASSTHROUGH,
            id: PREFIX.to_vec(),
        };
        let tf = LevelEvent::Achievements { bits: vec![TF_BIT] };
        let trigger = || {
            entity(
                group(392_153, Some(passthrough.clone()), 22_565),
                vec![group(394_754, Some(tf.clone()), 2_316)],
                vec![394_768],
            )
        };
        let level = |asset| FtueLevel {
            statics: FtueEntities::default(),
            static_causes: Vec::new(),
            assets: vec![asset],
            mobs: Vec::new(),
        };
        let mut script = AssetScript::of(&level(trigger()));
        script.on(Trigger::Passthrough(PREFIX), 0);
        script.on(Trigger::Achievement(TF_BIT), 15_022);
        assert!(script.take_due(17_337).is_empty());
        assert_eq!(
            script.take_due(17_338),
            [(0, Step::Show), (0, Step::Change(0))],
            "the card goes with the change"
        );
        assert_eq!(script.take_due(17_352), [(0, Step::Hide)]);
        assert!(script.take_due(u64::MAX).is_empty(), "no second card");
        // Without the passthrough the change still has its card.
        let mut script = AssetScript::of(&level(trigger()));
        script.on(Trigger::Achievement(TF_BIT), 0);
        assert_eq!(
            script.take_due(2_316),
            [(0, Step::Show), (0, Step::Change(0))]
        );
        script.on(Trigger::Passthrough(PREFIX), 3_000);
        assert_eq!(script.take_due(2_330), [(0, Step::Hide)]);
        assert!(script.take_due(u64::MAX).is_empty(), "the show is spent");
    }

    #[test]
    fn groups_go_on_their_events_after_their_delays() {
        let mut script = AssetScript::of(&level());
        script.on(Trigger::Achievement(EXPANSION), 0);
        assert!(script.take_due(15).is_empty());
        assert_eq!(script.take_due(16), [(0, Step::Show)]);
        script.on(
            Trigger::Client {
                message_type: QTE,
                id: Some(QTE_ID + 1),
            },
            100,
        );
        assert!(script.take_due(1000).is_empty(), "another id");
        script.on(
            Trigger::Client {
                message_type: QTE,
                id: Some(QTE_ID),
            },
            100,
        );
        assert_eq!(script.take_due(142), [(0, Step::Change(0))]);
        script.on(Trigger::MobSpawned(0), 200);
        assert_eq!(
            script.take_due(200),
            [(1, Step::Show)],
            "the marker with its mob"
        );
        script.on(Trigger::BatchDead(0), 300);
        assert_eq!(script.take_due(306), [(0, Step::Change(1))]);
        script.on(Trigger::WaterMove, 400);
        assert_eq!(script.take_due(400), [(0, Step::Hide), (1, Step::Hide)]);
        script.on(Trigger::BatchDead(3), 500);
        assert_eq!(script.take_due(520), [(2, Step::Show)]);
        assert_eq!(script.take_due(520 + 6268), [(2, Step::Hide)]);
        assert!(
            script.take_due(u64::MAX).is_empty(),
            "the spell is not played"
        );
    }
}
