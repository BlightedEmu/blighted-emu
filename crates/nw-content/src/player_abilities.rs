// SPDX-License-Identifier: AGPL-3.0-or-later
//! The player ability table (`player-abilities-<build>.json`, written by `content-tool
//! player-abilities`, read here): for each ability list of the player grid, the weapon classes it
//! is on for, every action of the list, the fragments they play, and the cooldown, mana and spell
//! rows they name (abilities design note, 2026-10-31).
//!
//! An ability is a chain of actions: the one whose activation tests the ability's name
//! (`IfActiveAbilityMoveName`) starts when its key is pressed, and others follow on what it does
//! (`IfActionStatus`, `IfFragmentPlaying`, tags, markers, inputs). The server runs them as the
//! client does, each tick: an action starts while its activation holds and stops when its
//! deactivation holds ([`nw_formats::mannequin::CageWorld`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nw_formats::mannequin::{AbilityUse, CageWorld, CastKind, ChargeTake, Condition, TagActivity};
use serde::{Deserialize, Serialize};

use crate::generated::SpellData;
use crate::player_attacks::{BasicAttack, Flight};

const FILE_PREFIX: &str = "player-abilities-";
const FILE_SUFFIX: &str = ".json";

/// The path of the table of `client_build` in `dir`.
pub fn player_abilities_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!("{FILE_PREFIX}{client_build}{FILE_SUFFIX}"))
}

/// A bad table file.
#[derive(Debug, thiserror::Error)]
pub enum PlayerAbilitiesError {
    /// The file cannot be read or written.
    #[error("player ability table {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table's JSON.
    #[error("player ability table: {0}")]
    Json(#[from] serde_json::Error),
}

/// A `CAGE-Named` clip of a fragment: the marker it sets while it plays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FragmentMarker {
    /// The marker (`Magic_IsReadying_Smite`).
    pub name: String,
    /// When it starts, ms after the fragment starts.
    pub start_ms: u64,
    /// When it ends, ms after the fragment starts.
    pub end_ms: u64,
}

/// A `CAGE-TransitionInstant` clip of a fragment: while it plays, the action it names takes the
/// place of the action that plays the fragment when its input is on and its condition holds
/// (the fire staff's `Ability_Fire_Staff_Incinerate_Intro` goes on to its `_Loop`, which only
/// starts this way). A `CAGE-TransitionBuffered` clip before it on its layer to the same place
/// keeps a key press for it: when the input and condition hold while the buffered clip plays,
/// the transition is taken once the instant clip starts (the flail's abilities: buffered from
/// 0.1 s, instant from 1.4 s of `Ability_Flail_Slam`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FragmentTransition {
    /// Where it leads.
    pub to: TransitionTo,
    /// `Input`: the input that must be on; `None` for none or `Test_AlwaysTrue`.
    pub input: Option<String>,
    /// `Condition`: the list's named condition, resolved; `None` for none.
    pub condition: Option<Condition>,
    /// When it starts, ms after the fragment starts.
    pub start_ms: u64,
    /// When it ends, ms after the fragment starts.
    pub end_ms: u64,
    /// When its `CAGE-TransitionBuffered` clip starts, ms after the fragment starts; `None`
    /// without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer_start_ms: Option<u64>,
}

/// Where a transition clip leads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionTo {
    /// Another action starts and the running one stops: `NewAction`, or the action of the list
    /// that plays the clip's `NewFragment`.
    Action(String),
    /// The running action goes on with another fragment: a `NewFragment` that no action of the
    /// list plays (the life staff's `Ability_Life_Staff_HealingWave` goes on to its
    /// `Ability_Life_Staff_HealingWave_Aim` after 0.43 s, and to `Magic_LowerWeapon`, whose
    /// `Magic_AimExit` marker ends the action, when the aim is left).
    Fragment(String),
}

/// A fragment the actions of a list play.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FragmentRow {
    /// Its name.
    pub name: String,
    /// The Mannequin scopes it plays on for the list's stance (`UpperBody`, `Weapon`): a
    /// fragment that starts on a scope takes it from the one that played there. Empty when the
    /// controller definitions do not name the fragment.
    #[serde(default)]
    pub scopes: Vec<String>,
    /// How long it plays, ms; 0 when the build found no clip of it.
    pub length_ms: u64,
    /// Its markers, by start.
    pub markers: Vec<FragmentMarker>,
    /// Its transitions, by start.
    #[serde(default)]
    pub transitions: Vec<FragmentTransition>,
    /// The hit of its clips (as a basic attack's); `None` for a fragment that holds none.
    pub hit: Option<BasicAttack>,
}

impl FragmentRow {
    /// The markers it sets `since_start_ms` after it starts. A marker that ends no later than
    /// it starts has no end: its fragment ends in a loop, whose length is the loop's start (the
    /// life staff's aim loop sets `Magic_IsReady_Heal` from 0.067 s on).
    pub fn markers_at(&self, since_start_ms: u64) -> Vec<&str> {
        self.markers
            .iter()
            .filter(|marker| {
                since_start_ms >= marker.start_ms
                    && (since_start_ms < marker.end_ms || marker.start_ms >= marker.end_ms)
            })
            .map(|marker| marker.name.as_str())
            .collect()
    }
}

/// A `SpellData` row, as far as the server reads it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpellRow {
    /// `SpellID`.
    pub id: String,
    /// `SpellTypes` (`Ability`, `Celestial`, ...).
    pub types: Option<String>,
    /// `ShapeTypes` (`Sphere`, `Cone`, ...); `None`: no area.
    pub shape: Option<String>,
    /// `Radius`, m.
    pub radius_m: Option<f32>,
    /// `CastDistance`, m.
    pub cast_distance_m: Option<f32>,
    /// `Duration`, ms; -1 lasts until its projectile ends.
    pub duration_ms: Option<i64>,
    /// `DamageTableRow`.
    pub damage_row: Option<String>,
    /// `StatusEffects`, in order.
    pub status_effects: Vec<String>,
    /// `TargetTypes` (`Enemy`, `Friend`, `Self`, `All`, `Group`).
    pub target_types: Option<String>,
    /// The mana its `ManaCost` id costs.
    pub mana_cost: Option<f32>,
    /// Its projectile, when it flies; `None`: it is where it is cast.
    #[serde(default)]
    pub projectile: Option<SpellProjectile>,
    /// `ClearOnExit`: a target that leaves its area loses its status effects.
    #[serde(default)]
    pub clear_on_exit: bool,
    /// `ClearOnEnd`: the targets in its area lose its status effects when it ends.
    #[serde(default)]
    pub clear_on_end: bool,
    /// `SecondarySpellId`: the spell its projectile casts where it ends (its own secondary left
    /// out).
    #[serde(default)]
    pub secondary: Option<Box<SpellRow>>,
}

/// The separator of a spell row's status effects (`A,B`).
const SPELL_EFFECT_SEPARATOR: char = ',';
/// A text cell that is on (`ClearOnExit`).
const CELL_TRUE: &str = "1";

impl SpellRow {
    /// The spell of the `SpellData` row `row`: its `ManaCost` id costs `mana_cost`, it flies as
    /// `projectile` and its projectile casts `secondary` where it ends.
    pub fn of_data(
        row: &SpellData,
        mana_cost: Option<f32>,
        projectile: Option<SpellProjectile>,
        secondary: Option<SpellRow>,
    ) -> Self {
        Self {
            id: row.spell_id.clone(),
            types: row.spell_types.clone(),
            shape: row.shape_types.clone(),
            // Lengths of metres.
            radius_m: row.radius.map(|radius| radius as f32),
            cast_distance_m: row.cast_distance.map(|distance| distance as f32),
            duration_ms: row.duration,
            damage_row: row.damage_table_row.clone(),
            status_effects: row
                .status_effects
                .iter()
                .flat_map(|effects| effects.split(SPELL_EFFECT_SEPARATOR))
                .map(str::trim)
                .filter(|effect| !effect.is_empty())
                .map(str::to_owned)
                .collect(),
            target_types: row.target_types.clone(),
            mana_cost,
            projectile,
            clear_on_exit: row.clear_on_exit.as_deref() == Some(CELL_TRUE),
            clear_on_end: row.clear_on_end == Some(true),
            secondary: secondary.map(Box::new),
        }
    }

    /// Whether its area lasts: a `Duration` above 0 (-1 and 0 are one pass).
    pub fn lasting_ms(&self) -> Option<u64> {
        self.duration_ms
            .and_then(|duration| u64::try_from(duration).ok())
            .filter(|duration| *duration > 0)
    }
}

/// The projectile of a spell: its `RangedAttackProfile` / `RangedAttackName` in the ranged
/// attack database and its prefab's flight.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpellProjectile {
    /// `m_maxRange`.
    pub range_m: f32,
    /// `m_projectileRadius`.
    pub radius_m: f32,
    /// How it flies; `None` for a hit scan (`m_isHitScan`).
    pub flight: Option<Flight>,
}

/// One spell an action casts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbilityCast {
    /// The conditions of the branches the cast is in.
    pub condition: Condition,
    /// How it aims.
    pub kind: CastKind,
    /// Its spell.
    pub spell: SpellRow,
}

/// One action of an ability list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListAction {
    /// Its name.
    pub name: String,
    /// Its fragment; empty for an action that plays none.
    pub fragment: String,
    /// It starts while this holds; `None`: it does not start by itself.
    pub activation: Option<Condition>,
    /// It stops when this holds; `None`: when its fragment is done.
    pub deactivation: Option<Condition>,
    /// Its tag activities: the Mannequin tags its start, its end and its branches set and unset.
    pub tag_activities: Vec<TagActivity>,
    /// Its ability parts (the ability it starts, costs, cooldowns, effects).
    pub ability: Option<AbilityUse>,
    /// The spells it casts.
    pub casts: Vec<AbilityCast>,
    /// The heart rune charge it takes as it starts (`ConsumeCharge`).
    #[serde(default)]
    pub charge_take: Option<ChargeTake>,
}

impl ListAction {
    /// The casts whose conditions hold in `world`.
    pub fn casts_in(&self, world: &dyn CageWorld) -> Vec<&AbilityCast> {
        self.casts
            .iter()
            .filter(|cast| cast.condition.holds_in(world))
            .collect()
    }
}

/// The item class of a heart rune: the player grid's rune list has no `WeaponRequirements` and
/// runs while an item of this class is in the heart rune slot (Observed: the rune items
/// `RuneDetonateT1` ... are of class `HeartGem`, and the rune actions set the weapon option
/// `heart-gem-slot`).
pub const HEARTGEM_CLASS: &str = "HeartGem";

/// One ability list of the player grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbilityList {
    /// The item classes of the weapons it is on for (`WeaponRequirements`, any of them).
    pub weapon_classes: Vec<String>,
    /// Its actions, in file order.
    pub actions: Vec<ListAction>,
    /// The fragments its actions play.
    pub fragments: Vec<FragmentRow>,
    /// The `Cooldowns_Player` times its actions start or test, ms, by ability id.
    pub cooldowns_ms: BTreeMap<String, u64>,
    /// The `ManaCosts_Player` costs its actions test, by cost id.
    pub mana_costs: BTreeMap<String, f32>,
}

impl AbilityList {
    /// The fragment `name` (any case).
    pub fn fragment(&self, name: &str) -> Option<&FragmentRow> {
        self.fragments
            .iter()
            .find(|fragment| fragment.name.eq_ignore_ascii_case(name))
    }

    /// The action `name`.
    pub fn action(&self, name: &str) -> Option<&ListAction> {
        self.actions.iter().find(|action| action.name == name)
    }

    /// The cooldown time of ability `id` (any case), ms.
    pub fn cooldown_ms(&self, id: &str) -> Option<u64> {
        self.cooldowns_ms
            .iter()
            .find(|(own, _)| own.eq_ignore_ascii_case(id))
            .map(|(_, time)| *time)
    }

    /// The mana of cost `id` (any case).
    pub fn mana_cost(&self, id: &str) -> Option<f32> {
        self.mana_costs
            .iter()
            .find(|(own, _)| own.eq_ignore_ascii_case(id))
            .map(|(_, cost)| *cost)
    }
}

/// Every ability list of the table.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerAbilities {
    /// The lists, in grid order.
    pub lists: Vec<AbilityList>,
    /// The client's input action names, in input-number order
    /// ([`nw_formats::input_profile`]): the CAGE input names the lists test.
    #[serde(default)]
    pub inputs: Vec<String>,
}

impl PlayerAbilities {
    /// Loads the table of `client_build` from `dir`.
    ///
    /// # Errors
    ///
    /// Returns [`PlayerAbilitiesError`] when the file is missing or bad.
    pub fn load(dir: &Path, client_build: &str) -> Result<Self, PlayerAbilitiesError> {
        let path = player_abilities_path(dir, client_build);
        let text = std::fs::read_to_string(&path)
            .map_err(|source| PlayerAbilitiesError::Io { path, source })?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Writes the table at `path`, creating parent folders.
    ///
    /// # Errors
    ///
    /// Returns [`PlayerAbilitiesError`] when the file cannot be written.
    pub fn write(&self, path: &Path) -> Result<(), PlayerAbilitiesError> {
        let io = |source| PlayerAbilitiesError::Io {
            path: path.to_path_buf(),
            source,
        };
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(io)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(self)?).map_err(io)
    }

    /// The input action names whose numbers `on` holds.
    pub fn inputs_on(&self, on: impl Fn(u32) -> bool) -> Vec<String> {
        self.inputs
            .iter()
            .enumerate()
            .filter(|(number, _)| u32::try_from(*number).is_ok_and(&on))
            .map(|(_, name)| name.clone())
            .collect()
    }

    /// The lists that a weapon with the item classes `weapon_classes` (any case) turns on, in
    /// grid order.
    pub fn lists_for<'a>(&'a self, weapon_classes: &[&str]) -> Vec<&'a AbilityList> {
        self.lists
            .iter()
            .filter(|list| {
                list.weapon_classes.iter().any(|class| {
                    weapon_classes
                        .iter()
                        .any(|own| own.eq_ignore_ascii_case(class))
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use nw_formats::mannequin::CageState;

    use super::*;

    const MARKER: &str = "Magic_IsReadying_Smite";
    const MARKER_START_MS: u64 = 200;
    const MARKER_END_MS: u64 = 600;
    const COOLDOWN: &str = "Ability_Life_Staff_Smite";
    const COOLDOWN_MS: u64 = 4_000;

    fn smite_list() -> AbilityList {
        AbilityList {
            weapon_classes: vec!["LifeStaff".to_owned()],
            actions: vec![ListAction {
                charge_take: None,
                name: "Smite".to_owned(),
                fragment: "Smite".to_owned(),
                activation: Some(Condition::True),
                deactivation: None,
                tag_activities: Vec::new(),
                ability: None,
                casts: vec![AbilityCast {
                    condition: Condition::All(vec![
                        Condition::Input("attack_shoot".to_owned()),
                        Condition::Marker(MARKER.to_owned()),
                    ]),
                    kind: CastKind::Raycast,
                    spell: SpellRow {
                        id: "LifeStaff_Smite".to_owned(),
                        types: None,
                        shape: Some("Sphere".to_owned()),
                        radius_m: Some(3.0),
                        cast_distance_m: Some(20.0),
                        duration_ms: Some(500),
                        damage_row: None,
                        status_effects: Vec::new(),
                        target_types: Some("Enemy".to_owned()),
                        mana_cost: Some(16.0),
                        projectile: None,
                        clear_on_exit: false,
                        clear_on_end: false,
                        secondary: None,
                    },
                }],
            }],
            fragments: vec![FragmentRow {
                name: "Smite".to_owned(),
                scopes: Vec::new(),
                length_ms: 1_000,
                markers: vec![FragmentMarker {
                    name: MARKER.to_owned(),
                    start_ms: MARKER_START_MS,
                    end_ms: MARKER_END_MS,
                }],
                transitions: Vec::new(),
                hit: None,
            }],
            cooldowns_ms: BTreeMap::from([(COOLDOWN.to_owned(), COOLDOWN_MS)]),
            mana_costs: BTreeMap::from([("LifeStaff_Smite".to_owned(), 16.0)]),
        }
    }

    #[test]
    fn an_action_casts_while_its_marker_is_on_and_its_key_is() {
        let list = smite_list();
        let fragment = list.fragment("smite").expect("fragment");
        assert_eq!(fragment.markers_at(MARKER_START_MS), [MARKER]);
        assert!(
            fragment.markers_at(MARKER_END_MS).is_empty(),
            "the end is out"
        );
        let markers = fragment.markers_at(MARKER_START_MS);
        let state = |inputs: &'static [&'static str]| CageState {
            tags: &[],
            markers: &markers,
            inputs,
        };
        let action = list.action("Smite").expect("action");
        assert_eq!(action.casts_in(&state(&["attack_shoot"])).len(), 1);
        assert!(action.casts_in(&state(&[])).is_empty());
        assert_eq!(
            list.cooldown_ms("ability_life_staff_smite"),
            Some(COOLDOWN_MS)
        );
        assert_eq!(list.mana_cost("lifestaff_smite"), Some(16.0));
    }

    #[test]
    fn a_weapon_finds_the_lists_of_its_classes() {
        let table = PlayerAbilities {
            lists: vec![smite_list()],
            inputs: vec!["ability1".to_owned(), "attack_shoot".to_owned()],
        };
        assert_eq!(
            table.lists_for(&["EquippableTwoHand", "lifestaff"]).len(),
            1
        );
        assert!(
            table.lists_for(&["Sword"]).is_empty(),
            "another weapon's list"
        );
        assert_eq!(table.inputs_on(|number| number == 1), ["attack_shoot"]);
        let text = serde_json::to_string(&table).expect("json");
        assert_eq!(
            serde_json::from_str::<PlayerAbilities>(&text).expect("back"),
            table
        );
    }
}
