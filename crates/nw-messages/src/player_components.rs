// SPDX-License-Identifier: AGPL-3.0-or-later
//! The replicated components of the player prefab (world-entry note, "Spawn condition traced").
//! A member's component index picks the component of the portrayed entity that takes the state
//! (`FUN_14167beb0`). When the player prefab is portrayed with no state for its other components,
//! the real client crashes while it applies the cached states. So the player's group carries one
//! state for every replicated component: the spawn card, the position, a connected type-3935
//! `PlayerComponentReplicatedState`, the look (type 1195), the vitals, stamina and mana (types
//! 15, 4297 and 1652), the gear (types 3183, 1755 and 5437) and the achievements (type 3210)
//! when known ([`player_members`]), and an empty state ("no slot set") for each other component below.
//!
//! The table is the client's own fragment log of a captured session (`NH-MSQ-10`, 2026-09-16):
//! every member of slot 1 it decoded, as (component index, class type index). All of these
//! classes use the generic slot reader `FUN_1417b4110`, whose first byte is the slot bitmap; on
//! every captured body only bits 0 to 2 are set, so one slot byte of 0 is a whole empty body.
//! Component 0 (the spawn card) and 16 (`ALCReplicatedState`) are not in the table.

use crate::achievements::{ACHIEVEMENT_TYPE_INDEX, AchievementState};
use crate::appearance::PlayerLook;
use crate::appearance::{PLAYER_APPEARANCE_TYPE_INDEX, PlayerAppearanceState};
use crate::components::{
    COMBAT_STATUS_TYPE_INDEX, CURRENCY_TYPE_INDEX, CombatStatusState, CurrencyState,
    DAMAGE_RECEIVER_TYPE_INDEX, DamageReceiverState, MANA_TYPE_INDEX, ManaState,
    STAMINA_TYPE_INDEX, StaminaState,
};
use crate::containers::{
    BAG_COMPONENT, CONTAINER_TYPE_INDEX, ContainerState, ITEM_MANAGEMENT_TYPE_INDEX,
    ItemManagementState,
};
use crate::cooldowns::{COOLDOWNS_TYPE_INDEX, CooldownsState};
use crate::gathering::{GATHERING_TYPE_INDEX, GatheringState};
use crate::gear::PlayerGear;
use crate::interactor::{INTERACTOR_TYPE_INDEX, InteractorState};
use crate::objectives::{OBJECTIVES_TYPE_INDEX, ObjectivesState};
use crate::paperdoll::PAPERDOLL_TYPE_INDEX;
use crate::payload::{BundleMember, MemberState};
use crate::progression::{PROGRESSION_TYPE_INDEX, ProgressionState};
use crate::slots::{children_mask, read_children, read_slot_byte, write_known_children};
use crate::vitals::{VITALS_TYPE_INDEX, VitalsData, VitalsState};
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// Type index of `MB::PlayerComponentReplicatedState`.
pub const PLAYER_COMPONENT_TYPE_INDEX: u64 = 3935;
/// The empty slot byte of a generic-reader body.
const NO_SLOT: u8 = 0;
/// Slots of type 3935: base slot 0 plus two add-slot calls (`FUN_146713160`).
const PLAYER_COMPONENT_SLOTS: u8 = 3;
/// Slot-byte bit for slot 1.
const SLOT_1_BIT: u8 = 0b010;
/// Children registered on slot 1 (death note, 2026-09-18).
const SLOT_1_CHILDREN: u8 = 22;
/// Slot 1 child 12: `onDeathRespawnCooldown`, an `f32`. A change of it to 0 or more opens the
/// client's death screen (`FUN_14683dba0`, `Hud.LocalPlayer.OnPlayerDeath`).
const CHILD_ON_DEATH_RESPAWN_COOLDOWN: u16 = 1 << 12;
/// The `onDeathRespawnCooldown` of an open-world death (retail: 0.0 in all three captured
/// deaths; the meaning of other values is Unknown).
const OPEN_WORLD_RESPAWN_COOLDOWN: f32 = 0.0;
/// The component of the player's 3935 state.
pub const PLAYER_COMPONENT_COMPONENT: u64 = 9;
/// Slot-byte bit for slot 2 (packet `group1`: `characterId`, `playerConnected`, …).
const SLOT_2_BIT: u8 = 0b100;
/// Children registered on slot 2, in constructor order.
const SLOT_2_CHILDREN: u8 = 9;
/// Slot 2 child 0: `characterId` (presence `FUN_14087ac50`).
const CHILD_CHARACTER_ID: u16 = 1 << 0;
/// Slot 2 child 3: `playerConnected` (one boolean byte, `FUN_1417b4c20`).
const CHILD_PLAYER_CONNECTED: u16 = 1 << 3;
/// Presence flag bit 0: take the fixed 16-byte path (`FUN_14087ac50`).
const PRESENCE_FLAG_FIXED: u8 = 0x01;
/// Presence flag bit 2: the client turns the 16 bytes into lowercase dashed UUID text. The
/// local-player check compares that text with the login token `CharacterId` (facet `+0xe8`
/// note), so `characterId` must use it; without it the text is 32 bare hex digits.
const PRESENCE_FLAG_UUID_TEXT: u8 = 0x04;
const FIELD: &str = "player component";

/// (component index, class type index) of each replicated component of the player prefab, other
/// than the spawn card and the position.
pub const PLAYER_EMPTY_COMPONENTS: [(u64, u64); 57] = [
    (1, 6951),
    (2, 5437),
    (4, 2406),
    (5, 3290),
    (6, 2768),
    (PLAYER_COMPONENT_COMPONENT, PLAYER_COMPONENT_TYPE_INDEX),
    (10, 3829),
    (12, PLAYER_APPEARANCE_TYPE_INDEX),
    (13, 603),
    (14, 1528),
    (17, 3563),
    (18, 5338),
    (19, 3133),
    (20, 3652),
    (22, 3857),
    (24, 3312),
    (25, 1292),
    (26, INTERACTOR_TYPE_INDEX),
    (27, 129),
    (28, COMBAT_STATUS_TYPE_INDEX),
    (29, 6234),
    (31, 1927),
    (32, 2895),
    (33, 982),
    (34, 4913),
    (35, 3765),
    (36, GATHERING_TYPE_INDEX),
    (39, 5620),
    (PLAYER_VITALS_COMPONENT, VITALS_TYPE_INDEX),
    (PLAYER_STAMINA_COMPONENT, STAMINA_TYPE_INDEX),
    (43, 1525),
    (44, 4236),
    (45, 2932),
    (46, 185),
    (47, MANA_TYPE_INDEX),
    (48, 5257),
    (49, 3152),
    (50, 4321),
    (51, 1994),
    (54, PLAYER_NAME_TAG_TYPE_INDEX),
    (55, 4176),
    (56, 3147),
    (57, 1566),
    (crate::components::CURRENCY_COMPONENT, CURRENCY_TYPE_INDEX),
    (61, 1755),
    (62, 3183),
    (65, 911),
    (66, 3210),
    (67, 3681),
    (68, 497),
    (70, 899),
    (71, 4878),
    (72, 5485),
    (73, 5606),
    (74, 5691),
    (76, 1755),
    (77, 2938),
];

/// Type index of `MB::PlayerNameTagComponentReplicatedState` (component 54).
pub const PLAYER_NAME_TAG_TYPE_INDEX: u64 = 100;
/// Type index of `MB::FactionComponentReplicatedState` (component 49).
const FACTION_TYPE_INDEX: u64 = 3152;
/// Type index of `MB::ChargeComponentReplicatedState` (component 48).
const CHARGE_TYPE_INDEX: u64 = 5257;
/// Type index of `MB::AudioProxyComponentReplicatedState` (component 13).
const AUDIO_PROXY_TYPE_INDEX: u64 = 603;
/// Type index of `MB::ObjectiveInteractorComponentReplicatedState` (component 10).
const OBJECTIVE_INTERACTOR_TYPE_INDEX: u64 = 3829;

/// The first bodies retail gives these classes on every new player's card, the same for all
/// (targeting note, 2026-09-18: beach capture q1). Our card left them empty, and the client then
/// never picked a lock-on or attack target (Inferred: its relation filter reads the faction).
/// Classes whose first body names the character (groups, abilities, attributes, status
/// effects, contribution) are not among them.
const RETAIL_FIRST_BODIES: [(u64, &[u8]); 5] = [
    // Slot 1 children 0 and 6, slot 2 children 1 and 3: all 0.
    (
        FACTION_TYPE_INDEX,
        &[0x06, 0x41, 0x00, 0x00, 0x0a, 0x00, 0x00],
    ),
    // Slot 0 child 0: 1.
    (
        PLAYER_NAME_TAG_TYPE_INDEX,
        &[0x01, 0x01, 0x00, 0x00, 0x00, 0x01],
    ),
    (CHARGE_TYPE_INDEX, &[0x01, 0x01, 0x00]),
    (AUDIO_PROXY_TYPE_INDEX, &[0x01, 0x01, 0x00]),
    (OBJECTIVE_INTERACTOR_TYPE_INDEX, &[0x01, 0x08, 0x00]),
];

/// The first body of player component `class`: retail's when it is in
/// [`RETAIL_FIRST_BODIES`], else one slot byte of 0 (no slot set).
fn first_body(class: u64) -> &'static [u8] {
    RETAIL_FIRST_BODIES
        .iter()
        .find(|(retail, _)| *retail == class)
        .map_or(&[NO_SLOT], |(_, body)| body)
}

/// The first state of a generic-reader class without a reader of its own here: retail's fixed
/// first body where it is known ([`RETAIL_FIRST_BODIES`]), else one slot byte of 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyState {
    /// The class type index.
    pub class: u64,
}

impl EmptyState {
    /// Whether `class` is a player component class that this state stands for.
    pub fn covers(class: u64) -> bool {
        !has_own_reader(class)
            && PLAYER_EMPTY_COMPONENTS
                .iter()
                .any(|(_, player_class)| *player_class == class)
    }

    /// Reads the first body of `class`: an empty one, or retail's fixed one.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Unsized`] for another body that sets a slot: this server does not
    /// know the children of these classes, so a bundle walk stops there.
    pub(crate) fn read(class: u64, reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let byte = reader.u8()?;
        let body = first_body(class);
        let unsized_body = MessageError::Unsized {
            field: "player component",
            byte,
        };
        if byte == NO_SLOT {
            return Ok(Self { class });
        }
        if body.first() != Some(&byte) || reader.bytes(body.len() - 1)? != &body[1..] {
            return Err(unsized_body);
        }
        Ok(Self { class })
    }

    /// Writes the first body.
    pub(crate) fn write(&self, writer: &mut Writer) {
        writer.bytes(first_body(self.class));
    }
}

/// `MB::PlayerComponentReplicatedState` (type 3935, component 9). Slot 2 holds
/// `characterId` and `playerConnected` (local-player-check note) of slot 2, and the
/// `onDeathRespawnCooldown` of slot 1 (death note). Other children and slot 0 are not encoded
/// here.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlayerComponentState {
    /// Slot 1 child 12: the respawn cooldown that opens the death screen, when sent.
    pub on_death_respawn_cooldown: Option<f32>,
    /// Slot 2 child 0: the 16-byte character id, when sent.
    pub character_id: Option<[u8; RAW_ID_LEN]>,
    /// Slot 2 child 3: whether this player is connected, when sent.
    pub player_connected: Option<bool>,
}

impl PlayerComponentState {
    /// The first body of a local player: `characterId` is `id`, `playerConnected` is true.
    pub const fn connected(id: [u8; RAW_ID_LEN]) -> Self {
        Self {
            on_death_respawn_cooldown: None,
            character_id: Some(id),
            player_connected: Some(true),
        }
    }

    /// The change at the player's death that opens its death screen (retail
    /// `09 9f3d 02 80 20 00000000`, 7 to 55 ms after the 4299).
    pub const fn died() -> Self {
        Self {
            on_death_respawn_cooldown: Some(OPEN_WORLD_RESPAWN_COOLDOWN),
            character_id: None,
            player_connected: None,
        }
    }

    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets slot 0, or sets a child this
    /// reader does not size.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let slots = read_slot_byte(reader, FIELD, PLAYER_COMPONENT_SLOTS)?;
        if slots & !(SLOT_1_BIT | SLOT_2_BIT) != 0 {
            return Err(MessageError::Unsized {
                field: FIELD,
                byte: slots,
            });
        }
        let mut state = Self::default();
        let unsized_child = |child| MessageError::Unsized {
            field: FIELD,
            byte: child,
        };
        if slots & SLOT_1_BIT != 0 {
            read_children(reader, FIELD, SLOT_1_CHILDREN, |reader, child| {
                if 1_u32 << child != u32::from(CHILD_ON_DEATH_RESPAWN_COOLDOWN) {
                    return Err(unsized_child(child));
                }
                state.on_death_respawn_cooldown = Some(reader.f32()?);
                Ok(())
            })?;
        }
        if slots & SLOT_2_BIT != 0 {
            read_children(reader, FIELD, SLOT_2_CHILDREN, |reader, child| {
                match 1 << child {
                    CHILD_CHARACTER_ID => state.character_id = Some(read_presence_id(reader)?),
                    CHILD_PLAYER_CONNECTED => state.player_connected = Some(reader.boolean()?),
                    _ => return Err(unsized_child(child)),
                }
                Ok(())
            })?;
        }
        Ok(state)
    }

    /// Writes the state for the bundle path. A slot without a known child is not sent.
    pub fn write(&self, writer: &mut Writer) {
        let cooldown = self.on_death_respawn_cooldown;
        let character_id = self.character_id;
        let player_connected = self.player_connected;
        let slot_2_children = children_mask(&[
            character_id.is_some(),
            false,
            false,
            player_connected.is_some(),
        ]);
        let slots = if cooldown.is_some() {
            SLOT_1_BIT
        } else {
            NO_SLOT
        } | if slot_2_children == 0 {
            NO_SLOT
        } else {
            SLOT_2_BIT
        };
        writer.u8(slots);
        if let Some(cooldown) = cooldown {
            write_known_children(
                writer,
                CHILD_ON_DEATH_RESPAWN_COOLDOWN,
                SLOT_1_CHILDREN,
                |writer, _| {
                    writer.f32(cooldown);
                },
            );
        }
        if slot_2_children != 0 {
            write_known_children(writer, slot_2_children, SLOT_2_CHILDREN, |writer, child| {
                if 1 << child == CHILD_CHARACTER_ID {
                    character_id.map(|id| {
                        writer
                            .u8(PRESENCE_FLAG_FIXED | PRESENCE_FLAG_UUID_TEXT)
                            .bytes(&id)
                    });
                } else {
                    player_connected.map(|connected| writer.boolean(connected));
                }
            });
        }
    }
}

/// Reads one presence id (`FUN_14087ac50`): a flag byte whose bit 0 must be set, then 16 bytes.
fn read_presence_id(reader: &mut Reader<'_>) -> Result<[u8; RAW_ID_LEN], MessageError> {
    let flag = reader.u8()?;
    if flag & PRESENCE_FLAG_FIXED == 0 {
        return Err(MessageError::BadTag {
            field: "PlayerComponentReplicatedState characterId flag",
            tag: flag,
        });
    }
    reader.raw_id()
}

/// The empty state of player component `class`: the default of a class with its own reader here
/// (it writes the same single slot byte of 0), else an [`EmptyState`].
pub fn empty_state(class: u64) -> MemberState {
    match class {
        VITALS_TYPE_INDEX => MemberState::Vitals(VitalsState::default()),
        STAMINA_TYPE_INDEX => MemberState::Stamina(StaminaState::default()),
        COMBAT_STATUS_TYPE_INDEX => MemberState::CombatStatus(CombatStatusState::default()),
        PLAYER_COMPONENT_TYPE_INDEX => {
            MemberState::PlayerComponent(PlayerComponentState::default())
        }
        PLAYER_APPEARANCE_TYPE_INDEX => MemberState::Appearance(PlayerAppearanceState::default()),
        MANA_TYPE_INDEX => MemberState::Mana(ManaState::default()),
        PAPERDOLL_TYPE_INDEX => MemberState::Paperdoll(Box::default()),
        CONTAINER_TYPE_INDEX => MemberState::Container(ContainerState::default()),
        ITEM_MANAGEMENT_TYPE_INDEX => MemberState::ItemManagement(ItemManagementState::default()),
        ACHIEVEMENT_TYPE_INDEX => MemberState::Achievement(AchievementState::default()),
        DAMAGE_RECEIVER_TYPE_INDEX => MemberState::DamageReceiver(DamageReceiverState::default()),
        OBJECTIVES_TYPE_INDEX => MemberState::Objectives(ObjectivesState::default()),
        PROGRESSION_TYPE_INDEX => MemberState::Progression(ProgressionState::default()),
        CURRENCY_TYPE_INDEX => MemberState::Currency(CurrencyState::default()),
        COOLDOWNS_TYPE_INDEX => MemberState::Cooldowns(CooldownsState::default()),
        GATHERING_TYPE_INDEX => MemberState::Gathering(GatheringState::default()),
        INTERACTOR_TYPE_INDEX => MemberState::Interactor(InteractorState::default()),
        _ => MemberState::Empty(EmptyState { class }),
    }
}

/// What a player's card says of the player besides its prefab and position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerStates<'a> {
    /// The 3935 `characterId`: the login character of the client's own player.
    pub character_id: [u8; RAW_ID_LEN],
    /// The saved look, when known.
    pub look: Option<PlayerLook>,
    /// Current and max health, when known; with them go full stamina and mana.
    pub health: Option<(u32, u32)>,
    /// The gear states, when known.
    pub gear: Option<&'a PlayerGear>,
    /// The achievements, when known.
    pub achievements: Option<&'a AchievementState>,
    /// The objectives (quest tracker), when known.
    pub objectives: Option<&'a ObjectivesState>,
    /// The level and experience, when known.
    pub progression: Option<ProgressionState>,
    /// The coins, with the card and after a change.
    pub currency: Option<CurrencyState>,
    /// The new health after it changed (a hit or a heal), when it did.
    pub health_change: Option<HealthChange>,
    /// The consumable cooldowns after a change, when there is one.
    pub cooldowns: Option<&'a CooldownsState>,
    /// The stamina children that changed, when some did.
    pub stamina: Option<StaminaState>,
    /// The combat status children that changed, when some did.
    pub combat_status: Option<CombatStatusState>,
}

/// A change of a player's health.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthChange {
    /// The new health.
    pub health: u32,
    /// It is the first damage from full health (retail `healthChangeFlags` 3).
    pub first_damage: bool,
    /// The player died or lives again: its new `vitalsData`.
    pub life: Option<VitalsData>,
}

/// The members of every player component in [`PLAYER_EMPTY_COMPONENTS`] for `states`: a connected
/// 3935 state, the look, vitals and gear that `states` knows, and an empty state for the rest.
pub fn player_members(states: PlayerStates<'_>) -> impl Iterator<Item = BundleMember> {
    PLAYER_EMPTY_COMPONENTS
        .iter()
        .map(move |(component, class)| BundleMember {
            component: *component,
            state: player_state(states, *component, *class),
        })
}

/// The own player component of the vitals.
pub const PLAYER_VITALS_COMPONENT: u64 = 41;
/// The own player component of the stamina.
pub const PLAYER_STAMINA_COMPONENT: u64 = 42;

/// The members of the states that `states` holds besides the card's: the achievements, the
/// objectives, the progression, the gear, the combat status, a health change and a stamina
/// change. A bundle without the card sends
/// these changes.
pub fn change_members(states: PlayerStates<'_>) -> Vec<BundleMember> {
    let mut members: Vec<BundleMember> = player_members(states)
        .filter(|member| match member.state.type_index() {
            ACHIEVEMENT_TYPE_INDEX => states.achievements.is_some(),
            OBJECTIVES_TYPE_INDEX => states.objectives.is_some(),
            PROGRESSION_TYPE_INDEX => states.progression.is_some(),
            CURRENCY_TYPE_INDEX => states.currency.is_some(),
            COOLDOWNS_TYPE_INDEX => states.cooldowns.is_some(),
            COMBAT_STATUS_TYPE_INDEX => states.combat_status.is_some(),
            PAPERDOLL_TYPE_INDEX | CONTAINER_TYPE_INDEX | ITEM_MANAGEMENT_TYPE_INDEX => {
                states.gear.is_some()
            }
            _ => false,
        })
        .collect();
    if let Some(change) = states.health_change {
        // A death opens the client's death screen through the 3935 respawn cooldown, which goes
        // with the dead vitals (death note).
        if change.life.is_some_and(|life| life.is_dead()) {
            members.push(BundleMember {
                component: PLAYER_COMPONENT_COMPONENT,
                state: MemberState::PlayerComponent(PlayerComponentState::died()),
            });
        }
        let vitals = match change.life {
            Some(life) => VitalsState::player_life(change.health, life),
            None if change.first_damage => VitalsState::creature_first_damage(change.health),
            None => VitalsState::creature_health(change.health),
        };
        members.push(BundleMember {
            component: PLAYER_VITALS_COMPONENT,
            state: MemberState::Vitals(vitals),
        });
    }
    if let Some(stamina) = states.stamina {
        members.push(BundleMember {
            component: PLAYER_STAMINA_COMPONENT,
            state: MemberState::Stamina(stamina),
        });
    }
    members
}

/// The members of the gear components of a player who has `gear`: a gear change sends them
/// again whole (see [`PlayerGear`]).
pub fn gear_members(gear: &PlayerGear) -> impl Iterator<Item = BundleMember> + '_ {
    let states = PlayerStates {
        character_id: [0; RAW_ID_LEN],
        look: None,
        health: None,
        gear: Some(gear),
        achievements: None,
        objectives: None,
        progression: None,
        currency: None,
        health_change: None,
        cooldowns: None,
        stamina: None,
        combat_status: None,
    };
    player_members(states).filter(|member| {
        matches!(
            member.state.type_index(),
            PAPERDOLL_TYPE_INDEX | CONTAINER_TYPE_INDEX | ITEM_MANAGEMENT_TYPE_INDEX
        )
    })
}

/// The state of player component `component` of class `class` for `states`.
fn player_state(states: PlayerStates<'_>, component: u64, class: u64) -> MemberState {
    if class == ACHIEVEMENT_TYPE_INDEX
        && let Some(achievements) = states.achievements
    {
        return MemberState::Achievement(achievements.clone());
    }
    if class == OBJECTIVES_TYPE_INDEX
        && let Some(objectives) = states.objectives
    {
        return MemberState::Objectives(objectives.clone());
    }
    if class == PROGRESSION_TYPE_INDEX
        && let Some(progression) = states.progression
    {
        return MemberState::Progression(progression);
    }
    if class == CURRENCY_TYPE_INDEX
        && let Some(currency) = states.currency
    {
        return MemberState::Currency(currency);
    }
    if class == COOLDOWNS_TYPE_INDEX
        && let Some(cooldowns) = states.cooldowns
    {
        return MemberState::Cooldowns(cooldowns.clone());
    }
    if class == COMBAT_STATUS_TYPE_INDEX
        && let Some(status) = states.combat_status
    {
        return MemberState::CombatStatus(status);
    }
    if let Some(gear) = states.gear {
        match class {
            PAPERDOLL_TYPE_INDEX => {
                return MemberState::Paperdoll(Box::new(gear.paperdoll.clone()));
            }
            CONTAINER_TYPE_INDEX if component == BAG_COMPONENT => {
                return MemberState::Container(gear.bag.clone());
            }
            CONTAINER_TYPE_INDEX => {
                return MemberState::Container(gear.second_container.clone());
            }
            ITEM_MANAGEMENT_TYPE_INDEX => {
                return MemberState::ItemManagement(gear.owned.clone());
            }
            _ => {}
        }
    }
    match (class, states.look, states.health) {
        (PLAYER_COMPONENT_TYPE_INDEX, _, _) => {
            MemberState::PlayerComponent(PlayerComponentState::connected(states.character_id))
        }
        (PLAYER_APPEARANCE_TYPE_INDEX, Some(look), _) => {
            MemberState::Appearance(PlayerAppearanceState::of(look))
        }
        (VITALS_TYPE_INDEX, _, Some((health, health_max))) => {
            MemberState::Vitals(VitalsState::player(health, health_max))
        }
        (STAMINA_TYPE_INDEX, _, Some(_)) => MemberState::Stamina(StaminaState::player()),
        (DAMAGE_RECEIVER_TYPE_INDEX, _, _) => {
            MemberState::DamageReceiver(DamageReceiverState::player())
        }
        (MANA_TYPE_INDEX, _, Some(_)) => MemberState::Mana(ManaState::player()),
        _ => empty_state(class),
    }
}

fn has_own_reader(class: u64) -> bool {
    matches!(
        class,
        VITALS_TYPE_INDEX
            | STAMINA_TYPE_INDEX
            | COMBAT_STATUS_TYPE_INDEX
            | PLAYER_COMPONENT_TYPE_INDEX
            | PLAYER_APPEARANCE_TYPE_INDEX
            | MANA_TYPE_INDEX
            | PAPERDOLL_TYPE_INDEX
            | CONTAINER_TYPE_INDEX
            | ITEM_MANAGEMENT_TYPE_INDEX
            | ACHIEVEMENT_TYPE_INDEX
            | DAMAGE_RECEIVER_TYPE_INDEX
            | OBJECTIVES_TYPE_INDEX
            | PROGRESSION_TYPE_INDEX
            | COOLDOWNS_TYPE_INDEX
            | GATHERING_TYPE_INDEX
            | INTERACTOR_TYPE_INDEX
            | CURRENCY_TYPE_INDEX
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::{BundleGroup, BundlePayload};

    const LOCAL_SLOT: u64 = 1;
    /// A member: a 1-byte component index, a class id of at most 2 bytes, a 1-byte body.
    const MAX_EMPTY_MEMBER_BYTES: usize = 4;

    const CHARACTER_ID: [u8; RAW_ID_LEN] = [5; RAW_ID_LEN];
    const HEALTH: (u32, u32) = (800, 856);
    /// The classes that carry a body when the look and the health are known.
    const KNOWN_CLASSES: [u64; 5] = [
        PLAYER_COMPONENT_TYPE_INDEX,
        PLAYER_APPEARANCE_TYPE_INDEX,
        VITALS_TYPE_INDEX,
        STAMINA_TYPE_INDEX,
        MANA_TYPE_INDEX,
    ];

    fn reads_back(members: &[BundleMember]) {
        let payload = BundlePayload {
            groups: vec![BundleGroup {
                interest: LOCAL_SLOT,
                members: members.to_vec(),
            }],
        };
        let read = BundlePayload::decode(&payload.encode().expect("payload")).expect("read");
        assert_eq!(read.groups[0].members, members);
    }

    #[test]
    fn without_look_and_health_every_other_component_is_empty_or_its_retail_first_body() {
        let states = PlayerStates {
            character_id: CHARACTER_ID,
            look: None,
            health: None,
            gear: None,
            achievements: None,
            objectives: None,
            progression: None,
            currency: None,
            health_change: None,
            cooldowns: None,
            stamina: None,
            combat_status: None,
        };
        let members: Vec<BundleMember> = player_members(states).collect();
        assert_eq!(members.len(), PLAYER_EMPTY_COMPONENTS.len());
        for member in &members {
            if member.state.type_index() == PLAYER_COMPONENT_TYPE_INDEX {
                assert_eq!(
                    member.state,
                    MemberState::PlayerComponent(PlayerComponentState::connected(CHARACTER_ID))
                );
                continue;
            }
            let bytes = member.encode().expect("member");
            let class = member.state.type_index();
            if class == DAMAGE_RECEIVER_TYPE_INDEX {
                assert_eq!(
                    member.state,
                    MemberState::DamageReceiver(DamageReceiverState::player())
                );
                continue;
            }
            assert!(bytes.ends_with(first_body(class)), "{member:?}");
            if first_body(class) == [NO_SLOT] {
                assert!(bytes.len() <= MAX_EMPTY_MEMBER_BYTES, "{member:?}");
            }
        }
        // Retail: the faction is `064100000a0000`, the name tag `010100000001`.
        let body_of = |class: u64| {
            members
                .iter()
                .find(|member| member.state.type_index() == class)
                .map(|member| member.encode().expect("member"))
        };
        assert!(
            body_of(FACTION_TYPE_INDEX)
                .is_some_and(|bytes| bytes.ends_with(&[0x06, 0x41, 0x00, 0x00, 0x0a, 0x00, 0x00]))
        );
        assert!(
            body_of(PLAYER_NAME_TAG_TYPE_INDEX)
                .is_some_and(|bytes| bytes.ends_with(&[0x01, 0x01, 0x00, 0x00, 0x00, 0x01]))
        );
        reads_back(&members);
    }

    #[test]
    fn a_known_look_and_health_fill_the_appearance_and_vitals() {
        let look = PlayerLook::default();
        let states = PlayerStates {
            character_id: CHARACTER_ID,
            look: Some(look),
            health: Some(HEALTH),
            gear: None,
            achievements: None,
            objectives: None,
            progression: None,
            currency: None,
            health_change: None,
            cooldowns: None,
            stamina: None,
            combat_status: None,
        };
        let members: Vec<BundleMember> = player_members(states).collect();
        for member in &members {
            let class = member.state.type_index();
            let expected = match class {
                PLAYER_COMPONENT_TYPE_INDEX => {
                    MemberState::PlayerComponent(PlayerComponentState::connected(CHARACTER_ID))
                }
                PLAYER_APPEARANCE_TYPE_INDEX => {
                    MemberState::Appearance(PlayerAppearanceState::of(look))
                }
                VITALS_TYPE_INDEX => MemberState::Vitals(VitalsState::player(HEALTH.0, HEALTH.1)),
                STAMINA_TYPE_INDEX => MemberState::Stamina(StaminaState::player()),
                MANA_TYPE_INDEX => MemberState::Mana(ManaState::player()),
                DAMAGE_RECEIVER_TYPE_INDEX => {
                    MemberState::DamageReceiver(DamageReceiverState::player())
                }
                _ => empty_state(class),
            };
            assert_eq!(member.state, expected);
        }
        for class in KNOWN_CLASSES {
            assert_eq!(
                members
                    .iter()
                    .filter(|member| member.state.type_index() == class)
                    .count(),
                1,
                "class {class} is one player component"
            );
        }
        reads_back(&members);
    }

    #[test]
    fn the_death_cooldown_is_the_retail_slot_1_child_12() {
        // Retail: `09 9f3d 02 80 20 00000000` (death note: all three captured deaths).
        const RETAIL_BODY: [u8; 7] = [0x02, 0x80, 0x20, 0, 0, 0, 0];
        let state = PlayerComponentState::read(&mut Reader::new(&RETAIL_BODY)).expect("read");
        assert_eq!(state, PlayerComponentState::died());
        let mut writer = Writer::new();
        state.write(&mut writer);
        assert_eq!(writer.finish(), RETAIL_BODY);
    }

    #[test]
    fn a_death_sends_the_respawn_cooldown_with_the_dead_vitals() {
        const SPAWNED_MS: u64 = 5_000;
        let death = PlayerStates {
            character_id: CHARACTER_ID,
            look: None,
            health: Some(HEALTH),
            gear: None,
            achievements: None,
            objectives: None,
            progression: None,
            currency: None,
            health_change: Some(HealthChange {
                health: 0,
                first_damage: false,
                life: Some(VitalsData::player_dead(SPAWNED_MS)),
            }),
            cooldowns: None,
            stamina: None,
            combat_status: None,
        };
        let members = change_members(death);
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].component, PLAYER_COMPONENT_COMPONENT);
        assert_eq!(
            members[0].state,
            MemberState::PlayerComponent(PlayerComponentState::died())
        );
        assert_eq!(members[1].component, PLAYER_VITALS_COMPONENT);
        let alive = PlayerStates {
            health_change: Some(HealthChange {
                health: HEALTH.1,
                first_damage: false,
                life: Some(VitalsData::player_alive(SPAWNED_MS)),
            }),
            ..death
        };
        assert_eq!(
            change_members(alive).len(),
            1,
            "a respawn sends the vitals only"
        );
    }

    #[test]
    fn the_changes_are_the_states_given_and_a_health_change() {
        const HEALED: u32 = 865;
        let objectives = ObjectivesState::default();
        // Retail after a land dodge: `01 09 425c0000 3f800000` (stamina-wire note, section 1).
        const RETAIL_SPEND: [u8; 10] = [0x01, 0x09, 0x42, 0x5c, 0, 0, 0x3f, 0x80, 0, 0];
        let spend = StaminaState {
            cur: Some(55.0),
            regen: Some(1.0),
            ..StaminaState::default()
        };
        let states = PlayerStates {
            character_id: CHARACTER_ID,
            look: None,
            health: Some(HEALTH),
            gear: None,
            achievements: None,
            objectives: Some(&objectives),
            progression: Some(ProgressionState::experience(20)),
            currency: None,
            health_change: Some(HealthChange {
                health: HEALED,
                first_damage: false,
                life: None,
            }),
            cooldowns: None,
            stamina: Some(spend),
            combat_status: None,
        };
        let members = change_members(states);
        let classes: Vec<u64> = members
            .iter()
            .map(|member| member.state.type_index())
            .collect();
        assert_eq!(
            classes,
            [
                OBJECTIVES_TYPE_INDEX,
                PROGRESSION_TYPE_INDEX,
                VITALS_TYPE_INDEX,
                STAMINA_TYPE_INDEX
            ]
        );
        assert_eq!(members[3].component, PLAYER_STAMINA_COMPONENT);
        let mut writer = crate::wire::Writer::new();
        spend.write(&mut writer);
        assert_eq!(writer.finish(), RETAIL_SPEND, "retail bytes");
        assert_eq!(members[3].state, MemberState::Stamina(spend));
        assert_eq!(members[2].component, PLAYER_VITALS_COMPONENT);
        assert_eq!(
            members[2].state,
            MemberState::Vitals(VitalsState::creature_health(HEALED))
        );
    }

    #[test]
    fn a_combat_status_change_goes_out_on_component_28() {
        // Retail 78148 of the tutorial capture.
        const RETAIL: [u8; 19] = [
            0x01, 0x0d, 0x01, 0x0b, 0xbc, 0xa3, 0x48, 0x1f, 0xb3, 0xeb, 0x61, 0x0b, 0xbc, 0xa3,
            0x45, 0xcb, 0xa8, 0x07, 0x61,
        ];
        let status = CombatStatusState {
            in_combat: Some(true),
            in_pvp_combat: None,
            logged_out_time: Some(0x0bbc_a348_1fb3_eb61),
            concluded_time: Some(0x0bbc_a345_cba8_0761),
        };
        let states = PlayerStates {
            character_id: CHARACTER_ID,
            look: None,
            health: Some(HEALTH),
            gear: None,
            achievements: None,
            objectives: None,
            progression: None,
            currency: None,
            health_change: None,
            cooldowns: None,
            stamina: None,
            combat_status: Some(status),
        };
        let members = change_members(states);
        assert_eq!(
            members,
            [BundleMember {
                component: 28,
                state: MemberState::CombatStatus(status),
            }]
        );
        let mut writer = crate::wire::Writer::new();
        status.write(&mut writer);
        assert_eq!(writer.finish(), RETAIL, "retail bytes");
    }

    #[test]
    fn gear_members_are_the_four_gear_components() {
        let gear = PlayerGear::first(&[]);
        let classes: Vec<u64> = gear_members(&gear)
            .map(|member| member.state.type_index())
            .collect();
        assert_eq!(classes.len(), 4);
        assert!(classes.contains(&PAPERDOLL_TYPE_INDEX));
        assert!(classes.contains(&ITEM_MANAGEMENT_TYPE_INDEX));
    }

    #[test]
    fn known_gear_fills_the_paperdoll_the_two_containers_and_the_owned_items() {
        const SWORD_INDEX: u32 = 1228;
        const SWORD_DURABILITY: u32 = 1000;
        const SWORD_GEAR_SCORE: u32 = 60;
        let gear = PlayerGear::first(&[crate::gear::EquippedItem {
            slot: crate::paperdoll::slot::MAIN_HAND_OPTION_1,
            item: crate::gear::GearItem {
                item_index: SWORD_INDEX,
                quantity: 1,
                gear_score: SWORD_GEAR_SCORE,
                durability: SWORD_DURABILITY,
                bound: true,
                instance: CHARACTER_ID,
            },
        }]);
        let states = PlayerStates {
            character_id: CHARACTER_ID,
            look: None,
            health: None,
            gear: Some(&gear),
            achievements: None,
            objectives: None,
            progression: None,
            currency: None,
            health_change: None,
            cooldowns: None,
            stamina: None,
            combat_status: None,
        };
        let members: Vec<BundleMember> = player_members(states).collect();
        let state_of = |component: u64| {
            members
                .iter()
                .find(|member| member.component == component)
                .map(|member| member.state.clone())
        };
        const PAPERDOLL_COMPONENT: u64 = 62;
        const SECOND_CONTAINER_COMPONENT: u64 = 76;
        const ITEM_MANAGEMENT_COMPONENT: u64 = 2;
        assert_eq!(
            state_of(PAPERDOLL_COMPONENT),
            Some(MemberState::Paperdoll(Box::new(gear.paperdoll.clone())))
        );
        assert_eq!(
            state_of(BAG_COMPONENT),
            Some(MemberState::Container(gear.bag.clone()))
        );
        assert_eq!(
            state_of(SECOND_CONTAINER_COMPONENT),
            Some(MemberState::Container(gear.second_container.clone()))
        );
        assert_eq!(
            state_of(ITEM_MANAGEMENT_COMPONENT),
            Some(MemberState::ItemManagement(gear.owned.clone()))
        );
        reads_back(&members);
    }

    #[test]
    fn a_set_slot_of_an_empty_class_stops_the_walk() {
        let class = PLAYER_EMPTY_COMPONENTS[0].1;
        assert!(EmptyState::covers(class));
        assert!(
            !EmptyState::covers(VITALS_TYPE_INDEX),
            "vitals has its own reader"
        );
        assert!(
            !EmptyState::covers(PLAYER_COMPONENT_TYPE_INDEX),
            "player component has its own reader"
        );
        let mut reader = Reader::new(&[1]);
        assert!(matches!(
            EmptyState::read(class, &mut reader),
            Err(MessageError::Unsized { .. })
        ));
    }

    #[test]
    fn a_connected_player_component_round_trips() {
        const ID: [u8; RAW_ID_LEN] = [9; RAW_ID_LEN];
        let state = PlayerComponentState::connected(ID);
        let mut writer = Writer::new();
        state.write(&mut writer);
        let bytes = writer.finish();
        assert_eq!(bytes.first(), Some(&SLOT_2_BIT));
        let mut reader = Reader::new(&bytes);
        let read = PlayerComponentState::read(&mut reader).expect("read");
        reader.finish().expect("to the end");
        assert_eq!(read, state);
        assert_eq!(read.player_connected, Some(true));
        assert_eq!(read.character_id, Some(ID));
    }

    #[test]
    fn a_connected_body_carries_the_character_id_as_uuid_text() {
        const ID: [u8; RAW_ID_LEN] = [7; RAW_ID_LEN];
        let mut writer = Writer::new();
        PlayerComponentState::connected(ID).write(&mut writer);
        let mut expected = vec![
            SLOT_2_BIT,
            0x09,
            PRESENCE_FLAG_FIXED | PRESENCE_FLAG_UUID_TEXT,
        ];
        expected.extend_from_slice(&ID);
        expected.push(1);
        assert_eq!(writer.finish(), expected, "facet +0xe8 note, section 6");
    }

    #[test]
    fn slot_0_or_another_slot_1_child_stops_the_walk() {
        // Slot 0; slot 1 child 0, whose width is Unknown.
        for body in [&[0b001][..], &[0b010, 0b001, 0]] {
            assert!(matches!(
                PlayerComponentState::read(&mut Reader::new(body)),
                Err(MessageError::Unsized { .. })
            ));
        }
    }
}
