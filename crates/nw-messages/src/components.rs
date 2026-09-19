// SPDX-License-Identifier: AGPL-3.0-or-later
//! Small replicated states with slot 0 only (component tables note, sections 1 to 3,
//! player-appearance note, section 3, and mob-cards note, section 2):
//! `MB::StaminaComponentReplicatedState` (type 4297),
//! `MB::CombatStatusComponentReplicatedState` (type 2913),
//! `MB::DelayedEventComponentReplicatedState` (type 3139),
//! `MB::LookTargetingComponentReplicatedState` (type 2443), `MB::ManaComponentReplicatedState`
//! (type 1652), `MB::DamageReceiverComponentReplicatedState` (type 1528) and
//! `MB::GatherableControllerReplicatedState` (type 12), `MB::InteractReplicatedState`
//! (type 2930) and the player's currency (type 3786). Each table ends every reached captured
//! body of its class.

use crate::slots::{children_mask, read_slot_0, write_known};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `MB::StaminaComponentReplicatedState`.
pub const STAMINA_TYPE_INDEX: u64 = 4297;
/// Type index of `MB::CombatStatusComponentReplicatedState`.
pub const COMBAT_STATUS_TYPE_INDEX: u64 = 2913;
/// Type index of `MB::DelayedEventComponentReplicatedState`.
pub const DELAYED_EVENT_TYPE_INDEX: u64 = 3139;
/// Type index of `MB::ManaComponentReplicatedState`.
pub const MANA_TYPE_INDEX: u64 = 1652;
/// Type index of `MB::DamageReceiverComponentReplicatedState`.
pub const DAMAGE_RECEIVER_TYPE_INDEX: u64 = 1528;
/// Type index of `MB::GatherableControllerReplicatedState`.
pub const GATHERABLE_CONTROLLER_TYPE_INDEX: u64 = 12;
/// Type index of `MB::InteractReplicatedState`.
pub const INTERACT_TYPE_INDEX: u64 = 2930;
/// Type index of the player's currency state (own player component 60).
pub const CURRENCY_TYPE_INDEX: u64 = 3786;
/// The own player component of the currency.
pub const CURRENCY_COMPONENT: u64 = 60;
/// Type index of `MB::BossPhaseComponentReplicatedState`.
pub const BOSS_PHASE_TYPE_INDEX: u64 = 403;
/// The slot byte and child mask of a boss phase change: slot 0, child 0 (the phase) only.
const BOSS_PHASE_CHANGE_HEAD: [u8; 2] = [0x01, 0x01];

/// Type index of `MB::LookTargetingComponentReplicatedState`.
pub const LOOK_TARGETING_TYPE_INDEX: u64 = 2443;
/// Children of the look targeting state: `currentLookAtTarget` (Azoth catalog field note).
const LOOK_TARGETING_CHILDREN: u8 = 1;

/// The body of a `MB::BossPhaseComponentReplicatedState` change to boss phase `phase`: child 0
/// as a big-endian `u32` (retail Thorpe: `01 01 00000001` at his intermission, `01 01 00000003`
/// at his end; child 1, a byte, only comes with the card).
pub fn boss_phase_change(phase: u32) -> Vec<u8> {
    [&BOSS_PHASE_CHANGE_HEAD[..], &phase.to_be_bytes()].concat()
}
/// Full stamina and mana of a player: 100 on the captured first bodies (vitals row `Player`).
const PLAYER_POOL: f32 = 100.0;
/// A stamina or mana value of 0 on the captured first bodies (`winded`, `regen`, `regenDelay`).
const NONE_YET: f32 = 0.0;
/// A multiplier or rate of 1 on the captured first bodies (`multMax`, `multRegen`, `regenRate`).
const UNIT_RATE: f32 = 1.0;
const COMBAT_STATUS_CHILDREN: u8 = 4;
const DELAYED_EVENT_CHILDREN: u8 = 2;
/// Children of the damage receiver: `isBlockActive` (a flag), `blockWeaponSlotAlias` and
/// `debugCritChance` (names from the Azoth catalog field note; the last two are four bytes each per
/// the damage-events note).
const DAMAGE_RECEIVER_CHILDREN: u8 = 3;
/// The `blockWeaponSlotAlias` of a player's first damage receiver state (retail own card).
const PLAYER_BLOCK_WEAPON_SLOT_ALIAS: u32 = 0x20;
/// The sized child of the gatherable controller: `gatherableState` (a byte). The width of child 1
/// (`replenishTime`) is Unknown, so a body that sets it is not read.
const GATHERABLE_CONTROLLER_CHILDREN: u8 = 1;
/// The sized child of the interact state: child 0, the interaction's `Enabled` flag (a byte;
/// retail `01 01 00` on a taken bag and a depleted node, `01 01 01` on a node that grew back). The
/// other children are Unknown, so a body that sets one is not read.
const INTERACT_CHILDREN: u8 = 1;
/// The sized child of the currency: child 0, the coins (a `u64`; beach-quests note: retail
/// `01 01 0000000000000177`, 375 coins after two quest rewards). The other children are Unknown.
const CURRENCY_CHILDREN: u8 = 1;

/// `MB::StaminaComponentReplicatedState`: six big-endian `f32` children (`FUN_14671a230`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StaminaState {
    /// `cur`.
    pub cur: Option<f32>,
    /// `max`.
    pub max: Option<f32>,
    /// `winded`.
    pub winded: Option<f32>,
    /// `regen`.
    pub regen: Option<f32>,
    /// `multMax`.
    pub mult_max: Option<f32>,
    /// `multRegen`.
    pub mult_regen: Option<f32>,
}

impl StaminaState {
    /// The first stamina of a player, as the usual 26-byte captured first body: 100 of 100, not
    /// winded, rates 1.
    pub const fn player() -> Self {
        Self {
            cur: Some(PLAYER_POOL),
            max: Some(PLAYER_POOL),
            winded: Some(NONE_YET),
            regen: Some(NONE_YET),
            mult_max: Some(UNIT_RATE),
            mult_regen: Some(UNIT_RATE),
        }
    }

    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or has a slot or child the table has
    /// not.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let [cur, max, winded, regen, mult_max, mult_regen] = read_floats(reader, "stamina")?;
        Ok(Self {
            cur,
            max,
            winded,
            regen,
            mult_max,
            mult_regen,
        })
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        let values = [
            self.cur,
            self.max,
            self.winded,
            self.regen,
            self.mult_max,
            self.mult_regen,
        ];
        write_floats(writer, &values);
    }
}

/// `MB::CombatStatusComponentReplicatedState` (`FUN_1466f1f10`): two flags, two big-endian
/// `u64` time points.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CombatStatusState {
    /// `replicatedInCombatFlag`.
    pub in_combat: Option<bool>,
    /// `replicatedInPvpCombatFlag`.
    pub in_pvp_combat: Option<bool>,
    /// `replicatedCombatLoggedOutTimePoint`.
    pub logged_out_time: Option<u64>,
    /// `replicatedCombatConcludedTimePoint`.
    pub concluded_time: Option<u64>,
}

impl CombatStatusState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, a flag is not 0 or 1, or a slot or
    /// child is not in the table.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(
            reader,
            "combat status",
            COMBAT_STATUS_CHILDREN,
            |reader, child| {
                match child {
                    0 => state.in_combat = Some(reader.boolean()?),
                    1 => state.in_pvp_combat = Some(reader.boolean()?),
                    2 => state.logged_out_time = Some(reader.u64()?),
                    _ => state.concluded_time = Some(reader.u64()?),
                }
                Ok(())
            },
        )?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        let present = [
            self.in_combat.is_some(),
            self.in_pvp_combat.is_some(),
            self.logged_out_time.is_some(),
            self.concluded_time.is_some(),
        ];
        write_known(
            writer,
            children_mask(&present),
            COMBAT_STATUS_CHILDREN,
            |writer, child| {
                match child {
                    0 => self.in_combat.map(|flag| writer.boolean(flag)),
                    1 => self.in_pvp_combat.map(|flag| writer.boolean(flag)),
                    2 => self.logged_out_time.map(|time| writer.u64(time)),
                    _ => self.concluded_time.map(|time| writer.u64(time)),
                };
            },
        );
    }
}

/// `MB::DelayedEventComponentReplicatedState` (`FUN_143066370`): a big-endian `u16` and a flag.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DelayedEventState {
    /// `totalDelayDuration`.
    pub total_delay: Option<u16>,
    /// `delayedEventCompleted`.
    pub completed: Option<bool>,
}

impl DelayedEventState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, the flag is not 0 or 1, or a slot or
    /// child is not in the table.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(
            reader,
            "delayed event",
            DELAYED_EVENT_CHILDREN,
            |reader, child| {
                if child == 0 {
                    state.total_delay = Some(reader.u16()?);
                } else {
                    state.completed = Some(reader.boolean()?);
                }
                Ok(())
            },
        )?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        let present = [self.total_delay.is_some(), self.completed.is_some()];
        write_known(
            writer,
            children_mask(&present),
            DELAYED_EVENT_CHILDREN,
            |writer, child| {
                match child {
                    0 => self.total_delay.map(|delay| writer.u16(delay)),
                    _ => self.completed.map(|completed| writer.boolean(completed)),
                };
            },
        );
    }
}

/// `MB::LookTargetingComponentReplicatedState`: what a creature looks at. Retail: each of the 10
/// creatures of the tutorial, right after it first picked the player (2415), sent
/// `01 01 bfa96f3ea2e36a5a`, the player's facet entity as a big-endian `u64`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LookTargetingState {
    /// `currentLookAtTarget`: a facet entity.
    pub target: Option<u64>,
}

impl LookTargetingState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or a slot or child is not in the table.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(
            reader,
            "look targeting",
            LOOK_TARGETING_CHILDREN,
            |reader, _| {
                state.target = Some(reader.u64()?);
                Ok(())
            },
        )?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        write_known(
            writer,
            children_mask(&[self.target.is_some()]),
            LOOK_TARGETING_CHILDREN,
            |writer, _| {
                self.target.map(|target| writer.u64(target));
            },
        );
    }
}

/// `MB::DamageReceiverComponentReplicatedState`: whether a block is up and with which weapon slot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DamageReceiverState {
    /// `isBlockActive`.
    pub is_block_active: Option<bool>,
    /// `blockWeaponSlotAlias`.
    pub block_weapon_slot_alias: Option<u32>,
    /// `debugCritChance`, kept as its four raw big-endian bytes.
    pub debug_crit_chance: Option<u32>,
}

impl DamageReceiverState {
    /// The first state of a player (retail own card: `01 03 00 00000020`, not blocking, block
    /// weapon slot alias 0x20).
    pub const fn player() -> Self {
        Self {
            is_block_active: Some(false),
            block_weapon_slot_alias: Some(PLAYER_BLOCK_WEAPON_SLOT_ALIAS),
            debug_crit_chance: None,
        }
    }

    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or a slot or child is not in the table.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(
            reader,
            "damage receiver",
            DAMAGE_RECEIVER_CHILDREN,
            |reader, child| {
                match child {
                    0 => state.is_block_active = Some(reader.boolean()?),
                    1 => state.block_weapon_slot_alias = Some(reader.u32()?),
                    _ => state.debug_crit_chance = Some(reader.u32()?),
                }
                Ok(())
            },
        )?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        let present = [
            self.is_block_active.is_some(),
            self.block_weapon_slot_alias.is_some(),
            self.debug_crit_chance.is_some(),
        ];
        write_known(
            writer,
            children_mask(&present),
            DAMAGE_RECEIVER_CHILDREN,
            |writer, child| {
                match child {
                    0 => self.is_block_active.map(|active| writer.boolean(active)),
                    1 => self.block_weapon_slot_alias.map(|alias| writer.u32(alias)),
                    _ => self.debug_crit_chance.map(|chance| writer.u32(chance)),
                };
            },
        );
    }
}

/// `MB::GatherableControllerReplicatedState`: the state of a gatherable or of a creature's
/// gatherable remains (4 on every living mob of the captures).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GatherableControllerState {
    /// `gatherableState`.
    pub gatherable_state: Option<u8>,
}

impl GatherableControllerState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or sets the unsized child 1.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(
            reader,
            "gatherable controller",
            GATHERABLE_CONTROLLER_CHILDREN,
            |reader, _| {
                state.gatherable_state = Some(reader.u8()?);
                Ok(())
            },
        )?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        write_known(
            writer,
            children_mask(&[self.gatherable_state.is_some()]),
            GATHERABLE_CONTROLLER_CHILDREN,
            |writer, _| {
                self.gatherable_state.map(|state| writer.u8(state));
            },
        );
    }
}

/// `MB::InteractReplicatedState`: whether an object can be interacted with now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InteractState {
    /// Child 0, `Enabled`.
    pub enabled: Option<bool>,
}

impl InteractState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or sets an unsized child.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, "interact", INTERACT_CHILDREN, |reader, _| {
            state.enabled = Some(reader.u8()? != 0);
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        write_known(
            writer,
            children_mask(&[self.enabled.is_some()]),
            INTERACT_CHILDREN,
            |writer, _| {
                self.enabled.map(|enabled| writer.u8(u8::from(enabled)));
            },
        );
    }
}

/// The player's currency: its coins.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CurrencyState {
    /// Child 0, the coins.
    pub coins: Option<u64>,
}

impl CurrencyState {
    /// The state of a player with `coins` coins.
    pub const fn of(coins: u64) -> Self {
        Self { coins: Some(coins) }
    }

    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or sets an unsized child.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, "currency", CURRENCY_CHILDREN, |reader, _| {
            state.coins = Some(reader.u64()?);
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        write_known(
            writer,
            children_mask(&[self.coins.is_some()]),
            CURRENCY_CHILDREN,
            |writer, _| {
                self.coins.map(|coins| writer.u64(coins));
            },
        );
    }
}

/// `MB::ManaComponentReplicatedState` (`FUN_1467088c0`): four big-endian `f32` children. The
/// names of the first two are Unknown (short strings the decompiler does not show); both are 100
/// on the captured first bodies.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ManaState {
    /// Child 0 (name Unknown).
    pub first: Option<f32>,
    /// Child 1 (name Unknown).
    pub second: Option<f32>,
    /// `regenDelay`.
    pub regen_delay: Option<f32>,
    /// `regenRate`.
    pub regen_rate: Option<f32>,
}

impl ManaState {
    /// The first mana state of a player, as the usual 18-byte captured first body: 100, 100,
    /// delay 0, rate 1.
    pub const fn player() -> Self {
        Self {
            first: Some(PLAYER_POOL),
            second: Some(PLAYER_POOL),
            regen_delay: Some(NONE_YET),
            regen_rate: Some(UNIT_RATE),
        }
    }

    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or has a slot or child the table has
    /// not.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let [first, second, regen_delay, regen_rate] = read_floats(reader, "mana")?;
        Ok(Self {
            first,
            second,
            regen_delay,
            regen_rate,
        })
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        write_floats(
            writer,
            &[self.first, self.second, self.regen_delay, self.regen_rate],
        );
    }
}

/// Reads slot 0 of a class whose `N` children are all big-endian `f32`.
fn read_floats<const N: usize>(
    reader: &mut Reader<'_>,
    field: &'static str,
) -> Result<[Option<f32>; N], MessageError> {
    let mut values = [None; N];
    // The tables have at most 7 children.
    read_slot_0(reader, field, N as u8, |reader, child| {
        values[usize::from(child)] = Some(reader.f32()?);
        Ok(())
    })?;
    Ok(values)
}

/// Writes slot 0 of a class whose children are all big-endian `f32`.
fn write_floats(writer: &mut Writer, values: &[Option<f32>]) {
    let present: Vec<bool> = values.iter().map(Option::is_some).collect();
    // The tables have at most 7 children.
    write_known(
        writer,
        children_mask(&present),
        values.len() as u8,
        |writer, child| {
            if let Some(value) = values[usize::from(child)] {
                writer.f32(value);
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn a_boss_phase_change_is_the_retail_body() {
        // Retail Thorpe (493283 and 546981 of the tutorial capture).
        assert_eq!(boss_phase_change(1), [0x01, 0x01, 0, 0, 0, 0x01]);
        assert_eq!(boss_phase_change(3), [0x01, 0x01, 0, 0, 0, 0x03]);
    }

    #[test]
    fn a_look_target_is_the_retail_body() {
        // Retail sailor 1 at 35050 of the tutorial capture: `0101bfa96f3ea2e36a5a`.
        const RETAIL: [u8; 10] = [0x01, 0x01, 0xbf, 0xa9, 0x6f, 0x3e, 0xa2, 0xe3, 0x6a, 0x5a];
        let state = LookTargetingState {
            target: Some(0xbfa9_6f3e_a2e3_6a5a),
        };
        assert_eq!(encoded(|writer| state.write(writer)), RETAIL);
        let mut reader = Reader::new(&RETAIL);
        assert_eq!(LookTargetingState::read(&mut reader).expect("read"), state);
        assert!(reader.is_empty());
    }

    const STAMINA: f32 = 120.5;
    const DELAY_MS: u16 = 1500;
    const TIME_POINT: u64 = 0x0102_0304_0506_0708;

    fn encoded(write: impl FnOnce(&mut Writer)) -> Vec<u8> {
        let mut writer = Writer::new();
        write(&mut writer);
        writer.finish()
    }

    fn finite() -> impl Strategy<Value = Option<f32>> {
        proptest::option::of(-1.0e9_f32..1.0e9_f32)
    }

    proptest! {
        #[test]
        fn stamina_round_trips(
            (cur, max, winded) in (finite(), finite(), finite()),
            (regen, mult_max, mult_regen) in (finite(), finite(), finite()),
        ) {
            let state = StaminaState { cur, max, winded, regen, mult_max, mult_regen };
            let bytes = encoded(|writer| state.write(writer));
            let mut reader = Reader::new(&bytes);
            prop_assert_eq!(StaminaState::read(&mut reader).expect("read"), state);
            prop_assert!(reader.is_empty());
        }

        #[test]
        fn combat_status_round_trips(
            in_combat in proptest::option::of(any::<bool>()),
            in_pvp_combat in proptest::option::of(any::<bool>()),
            logged_out_time in proptest::option::of(any::<u64>()),
            concluded_time in proptest::option::of(any::<u64>()),
        ) {
            let state = CombatStatusState { in_combat, in_pvp_combat, logged_out_time, concluded_time };
            let bytes = encoded(|writer| state.write(writer));
            let mut reader = Reader::new(&bytes);
            prop_assert_eq!(CombatStatusState::read(&mut reader).expect("read"), state);
            prop_assert!(reader.is_empty());
        }

        #[test]
        fn delayed_event_round_trips(
            total_delay in proptest::option::of(any::<u16>()),
            completed in proptest::option::of(any::<bool>()),
        ) {
            let state = DelayedEventState { total_delay, completed };
            let bytes = encoded(|writer| state.write(writer));
            let mut reader = Reader::new(&bytes);
            prop_assert_eq!(DelayedEventState::read(&mut reader).expect("read"), state);
            prop_assert!(reader.is_empty());
        }
    }

    #[test]
    fn stamina_children_are_floats_in_table_order() {
        let mut bytes = vec![0x01, 0b10_0010];
        bytes.extend(STAMINA.to_be_bytes());
        bytes.extend(STAMINA.to_be_bytes());
        let state = StaminaState::read(&mut Reader::new(&bytes)).expect("read");
        assert_eq!(
            (state.max, state.mult_regen),
            (Some(STAMINA), Some(STAMINA))
        );
        assert_eq!(state.cur, None);
    }

    #[test]
    fn combat_status_flags_then_time_points() {
        let mut bytes = vec![0x01, 0b1001, 1];
        bytes.extend(TIME_POINT.to_be_bytes());
        let state = CombatStatusState::read(&mut Reader::new(&bytes)).expect("read");
        assert_eq!(
            (state.in_combat, state.concluded_time),
            (Some(true), Some(TIME_POINT))
        );
    }

    #[test]
    fn a_combat_flag_above_one_is_refused() {
        assert!(CombatStatusState::read(&mut Reader::new(&[0x01, 0b01, 2])).is_err());
    }

    #[test]
    fn mob_damage_receiver_and_gatherable_are_the_retail_bodies() {
        // A retail sailor (mob-cards note, section 2.1).
        const RETAIL_DAMAGE_RECEIVER: [u8; 7] = [0x01, 0x03, 0x00, 0x00, 0x00, 0x00, 0x20];
        const RETAIL_GATHERABLE: [u8; 3] = [0x01, 0x01, 0x04];
        let receiver =
            DamageReceiverState::read(&mut Reader::new(&RETAIL_DAMAGE_RECEIVER)).expect("read");
        assert_eq!(
            receiver,
            DamageReceiverState {
                is_block_active: Some(false),
                block_weapon_slot_alias: Some(0x20),
                debug_crit_chance: None,
            }
        );
        let mut writer = Writer::new();
        receiver.write(&mut writer);
        assert_eq!(writer.finish(), RETAIL_DAMAGE_RECEIVER);
        let gatherable =
            GatherableControllerState::read(&mut Reader::new(&RETAIL_GATHERABLE)).expect("read");
        assert_eq!(gatherable.gatherable_state, Some(4));
        let mut writer = Writer::new();
        gatherable.write(&mut writer);
        assert_eq!(writer.finish(), RETAIL_GATHERABLE);
    }

    #[test]
    fn the_currency_is_its_coins() {
        // Retail after the second quest reward: 375 coins (beach-quests note).
        const RETAIL: [u8; 10] = [0x01, 0x01, 0, 0, 0, 0, 0, 0, 0x01, 0x77];
        const COINS: u64 = 375;
        let state = CurrencyState::read(&mut Reader::new(&RETAIL)).expect("read");
        assert_eq!(state, CurrencyState::of(COINS));
        let mut writer = Writer::new();
        state.write(&mut writer);
        assert_eq!(writer.finish(), RETAIL);
    }

    #[test]
    fn the_interact_state_is_its_enabled_flag() {
        // Retail: a taken bag and a depleted node, then a node that grew back.
        for (body, enabled) in [([0x01, 0x01, 0x00], false), ([0x01, 0x01, 0x01], true)] {
            let state = InteractState::read(&mut Reader::new(&body)).expect("read");
            assert_eq!(state.enabled, Some(enabled));
            let mut writer = Writer::new();
            state.write(&mut writer);
            assert_eq!(writer.finish(), body);
        }
    }

    #[test]
    fn delayed_event_is_a_u16_then_a_flag() {
        let mut bytes = vec![0x01, 0b11];
        bytes.extend(DELAY_MS.to_be_bytes());
        bytes.push(0);
        let state = DelayedEventState::read(&mut Reader::new(&bytes)).expect("read");
        assert_eq!(
            (state.total_delay, state.completed),
            (Some(DELAY_MS), Some(false))
        );
    }

    #[test]
    fn a_player_start_stamina_is_all_six_floats() {
        let bytes = encoded(|writer| StaminaState::player().write(writer));
        let mut expected = vec![0x01, 0b11_1111];
        for value in [100.0_f32, 100.0, 0.0, 0.0, 1.0, 1.0] {
            expected.extend(value.to_be_bytes());
        }
        assert_eq!(bytes, expected, "the 26-byte retail first body");
    }

    #[test]
    fn a_player_start_mana_is_four_floats() {
        let bytes = encoded(|writer| ManaState::player().write(writer));
        let mut expected = vec![0x01, 0b1111];
        for value in [100.0_f32, 100.0, 0.0, 1.0] {
            expected.extend(value.to_be_bytes());
        }
        assert_eq!(bytes, expected, "the 18-byte retail first body");
        let mut reader = Reader::new(&bytes);
        assert_eq!(
            ManaState::read(&mut reader).expect("read"),
            ManaState::player()
        );
        assert!(reader.is_empty());
    }

    #[test]
    fn a_child_beyond_the_table_is_refused() {
        assert!(DelayedEventState::read(&mut Reader::new(&[0x01, 0b100])).is_err());
    }
}
