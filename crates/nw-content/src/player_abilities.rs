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
    /// When it ends, ms after the fragment starts; `None` for a clip that is the last of its
    /// layer: it lasts while its fragment plays ([`FragmentRow::markers_at`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_ms: Option<u64>,
}

/// Whether a clip from `start_ms` to `end_ms` (`None`: while its fragment plays) is on
/// `since_start_ms` after its fragment started. A clip that is the last of its procedural layer
/// has no end of its own and stays on while its action keeps the fragment, also past the end of
/// the fragment's animation layers (Inferred from the data: the fire staff's
/// `Ability_Fire_Staff_Speed_Loop` sets `BlockDisabled` from 0 s with no end, and its animation
/// loops; the client data has no other end for it).
fn clip_on(start_ms: u64, end_ms: Option<u64>, since_start_ms: u64) -> bool {
    since_start_ms >= start_ms && end_ms.is_none_or(|end_ms| since_start_ms < end_ms)
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
    /// When it ends, ms after the fragment starts; `None` for a clip that is the last of its
    /// layer: it lasts while its fragment plays ([`FragmentTransition::on_at`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_ms: Option<u64>,
    /// When its `CAGE-TransitionBuffered` clip starts, ms after the fragment starts; `None`
    /// without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer_start_ms: Option<u64>,
}

impl FragmentTransition {
    /// Whether it is on `since_start_ms` after its fragment started: from its start to its end,
    /// or while the fragment plays when it has no end.
    pub fn on_at(&self, since_start_ms: u64) -> bool {
        clip_on(self.start_ms, self.end_ms, since_start_ms)
    }
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
    /// How long its animation layers play, ms: the longest layer, its last clip played to the
    /// clip's end; for a fragment that ends in a loop ([`FragmentRow::loops`]), until that loop
    /// starts. 0 when the build found no clip of it, or for an entry without clips (the musket's
    /// `Ability_Musket_LoadChargeShot_UB` for the tags `2H_Ranged+Rifle`).
    pub length_ms: u64,
    /// Whether an animation layer ends in a looping clip (`flags="Loop"`, no clip after it): the
    /// fragment never finishes by itself, and plays until its action stops or a transition
    /// moves it on (Observed: 30 retail fire staff Flame Thrower plays, whose
    /// `Ability_Fire_Staff_FlameThrower` loops from 0.633 s and whose action stops when it is
    /// done, lasted 1.39 to 3.82 s (p10 to p90) until its exit transition; 19 Ice Gauntlet
    /// Entombed plays 2.2 to 10.5 s, whose `Ability_IceMagic_Entombed_Loop` loops from 0 s;
    /// ability ends of `abilityends`, all 1,195 retail ledgers of the capture index).
    #[serde(default)]
    pub loops: bool,
    /// Its markers, by start.
    pub markers: Vec<FragmentMarker>,
    /// Its transitions, by start.
    #[serde(default)]
    pub transitions: Vec<FragmentTransition>,
    /// The hit of its clips (as a basic attack's); `None` for a fragment that holds none.
    pub hit: Option<BasicAttack>,
}

impl FragmentRow {
    /// The markers it sets `since_start_ms` after it starts: each from its start to its end,
    /// one without an end while the fragment plays (the life staff's aim loop sets
    /// `Magic_IsReady_Heal` from 0.067 s on). A marker past the end of the animation layers is
    /// on while the action keeps the fragment (`IfFragmentDone` holds at the end of the
    /// animation layers, not of the procedural ones: Observed, 133 retail blunderbuss Vortex
    /// Shot plays, whose `Ability_Blunderbuss_VortexShot` animation lasts 1.333 s and whose
    /// `CanDodgeCancel` clip lasts to 3.05 s, ended 1.19 to 1.26 s after the server's answer in
    /// 105 of them; its action's only deactivation is `IfFragmentDone`).
    pub fn markers_at(&self, since_start_ms: u64) -> Vec<&str> {
        self.markers
            .iter()
            .filter(|marker| clip_on(marker.start_ms, marker.end_ms, since_start_ms))
            .map(|marker| marker.name.as_str())
            .collect()
    }

    /// Whether it is done `since_start_ms` after it started, `exit_offset_ms` before its end
    /// (`IfFragmentDone`): its animation layers played to their end; a fragment that ends in a
    /// loop is never done.
    pub fn done_at(&self, since_start_ms: u64, exit_offset_ms: u32) -> bool {
        !self.loops && since_start_ms + u64::from(exit_offset_ms) >= self.length_ms
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
                loops: false,
                markers: vec![FragmentMarker {
                    name: MARKER.to_owned(),
                    start_ms: MARKER_START_MS,
                    end_ms: Some(MARKER_END_MS),
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

    /// The fire staff's `Ability_Fire_Staff_Speed_Loop` (build 25810808): its animation loops
    /// from 0 s, `BlockDisabled` has no end, and its transition to `Ability_Fire_Staff_Speed_Stop`
    /// is on from 0.867 s with no end.
    #[test]
    fn a_looping_fragment_is_never_done_and_its_open_clips_stay_on() {
        const BLOCK_DISABLED: &str = "BlockDisabled";
        const STOP_START_MS: u64 = 867;
        const LONG_AFTER_MS: u64 = 60_000;
        let mut fragment = FragmentRow {
            name: "Ability_Fire_Staff_Speed_Loop".to_owned(),
            scopes: Vec::new(),
            length_ms: 0,
            loops: true,
            markers: vec![FragmentMarker {
                name: BLOCK_DISABLED.to_owned(),
                start_ms: 0,
                end_ms: None,
            }],
            transitions: vec![FragmentTransition {
                to: TransitionTo::Action("Ability_Fire_Staff_Speed_Stop".to_owned()),
                input: None,
                condition: None,
                start_ms: STOP_START_MS,
                end_ms: None,
                buffer_start_ms: None,
            }],
            hit: None,
        };
        assert!(!fragment.done_at(LONG_AFTER_MS, 0), "a loop is never done");
        assert_eq!(fragment.markers_at(LONG_AFTER_MS), [BLOCK_DISABLED]);
        let transition = &fragment.transitions[0];
        assert!(!transition.on_at(STOP_START_MS - 1));
        assert!(transition.on_at(STOP_START_MS));
        assert!(transition.on_at(LONG_AFTER_MS), "no end: on while it plays");
        fragment.loops = false;
        assert!(
            fragment.done_at(0, 0),
            "without the loop, a fragment of no length is done at once"
        );
    }

    #[test]
    fn a_fragment_is_done_at_the_end_of_its_animation_less_its_offset() {
        const EXIT_OFFSET_MS: u32 = 100;
        let list = smite_list();
        let fragment = list.fragment("Smite").expect("fragment");
        let length = fragment.length_ms;
        assert!(!fragment.done_at(length - u64::from(EXIT_OFFSET_MS) - 1, EXIT_OFFSET_MS));
        assert!(fragment.done_at(length - u64::from(EXIT_OFFSET_MS), EXIT_OFFSET_MS));
        assert!(fragment.done_at(length, 0));
        assert!(!fragment.done_at(length - 1, 0));
    }

    #[test]
    fn a_table_with_numbered_clip_ends_reads_them_as_ends() {
        let text = r#"{"name":"Smite","start_ms":200,"end_ms":600}"#;
        let marker: FragmentMarker = serde_json::from_str(text).expect("marker");
        assert_eq!(marker.end_ms, Some(MARKER_END_MS));
        let open: FragmentMarker =
            serde_json::from_str(r#"{"name":"Smite","start_ms":200}"#).expect("open marker");
        assert_eq!(open.end_ms, None);
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

    #[test]
    #[ignore = "needs the build 25810808 content in NW_CONTENT_DIR"]
    fn a_built_fragment_that_an_empty_clip_ends_lasts_its_clip() {
        // Observed (client data): `player_blunderbuss.adb`
        // `Ability_Blunderbuss_ClawShot_Hit_Confirm` plays `blunderbuss_a1_clawshot_wait` (0.333
        // s long) and `player.adb`
        // `Ability_Rune_Sandworm_Cast` plays `heartgems_ability_sandworm_cast` (1.167 s), each
        // then an empty clip whose blend has `ExitTime -1`: the fragment ends with its clip.
        // Before `nw_formats::mannequin::AnimClip::chained_end` both lasted 0 ms, and every
        // marker and transition of them ended at 0. Retail length: Unknown (no capture tells
        // these fragments' ends apart from the ability's later states).
        const BUILD: &str = "25810808";
        const CLAW_SHOT: (&str, u64) = ("Ability_Blunderbuss_ClawShot_Hit_Confirm", 333);
        const SANDWORM: (&str, u64) = ("Ability_Rune_Sandworm_Cast", 1_167);
        let dir = std::env::var("NW_CONTENT_DIR").expect("NW_CONTENT_DIR");
        let table = PlayerAbilities::load(Path::new(&dir), BUILD).expect("table");
        for (name, length_ms) in [CLAW_SHOT, SANDWORM] {
            let fragment = table
                .lists
                .iter()
                .find_map(|list| list.fragment(name))
                .expect(name);
            assert_eq!(fragment.length_ms, length_ms, "{name}");
            assert!(
                fragment
                    .markers
                    .iter()
                    .map(|marker| marker.end_ms)
                    .chain(fragment.transitions.iter().map(|to| to.end_ms))
                    .any(|end_ms| end_ms.is_none()),
                "{name}: a marker or transition lasts while the fragment plays: {fragment:?}"
            );
            assert!(!fragment.done_at(length_ms - 1, 0), "{name}");
            assert!(fragment.done_at(length_ms, 0), "{name}");
        }
    }

    #[test]
    #[ignore = "needs the build 25810808 content in NW_CONTENT_DIR"]
    fn the_built_loops_beast_form_and_dry_fire_have_their_clip_data() {
        // Observed (client data, build 25810808): the aim and loop fragments end in a looping
        // clip (`Bow_Aim_UpperBody` `bow_aim_halfdrw_idle_ub`, `Ability_VoidGauntlet_Harvest`
        // `void_gnt_harvest_loop_ub`, the fire staff's Flame Thrower loop from 0.633 s); the
        // heart rune's beast form plays `archetype_beast_combat_taunt_01` (3.3333335 s at speed 2)
        // and `_02` (2.6666667 s at speed 1.5) of the beast archetype's folder, which
        // `player_shared.chrparams` names; the musket's dry fire for `2H_Ranged+Crouch+HeavyAim`
        // (the tags `Musket_ShootersStance_Ready` sets) plays `rifle_crch_aim_idle` to an empty
        // clip at 0.8 s, sets `ForceReload` from 0.633 s and goes back to
        // `Ability_Musket_ShootersStance_Aim` from 0.667 s. Before, each was 0 ms long.
        const BUILD: &str = "25810808";
        const LOOPS: [(&str, u64); 4] = [
            ("Bow_Aim_UpperBody", 0),
            ("Ability_VoidGauntlet_Harvest", 0),
            ("Ability_Fire_Staff_FlameThrower", 633),
            ("Ability_IceMagic_Entombed_Loop", 0),
        ];
        const BEAST_FORM: [(&str, u64); 2] = [
            ("Ability_Rune_BeastMode_Activate", 1_667),
            ("Ability_Rune_BeastMode_Deactivate", 1_778),
        ];
        const DRY_FIRE: &str = "Firearm_Shoot_DryFire_UpperBody";
        const DRY_FIRE_MS: u64 = 800;
        const FORCE_RELOAD: (&str, u64) = ("ForceReload", 633);
        // The action of the list that plays `Ability_Musket_ShootersStance_Aim`.
        const BACK_TO_AIM: (&str, u64) = ("Musket_ShootersStance_Aim_SubState", 667);
        let dir = std::env::var("NW_CONTENT_DIR").expect("NW_CONTENT_DIR");
        let table = PlayerAbilities::load(Path::new(&dir), BUILD).expect("table");
        let fragment = |name: &str| {
            table
                .lists
                .iter()
                .find_map(|list| list.fragment(name))
                .unwrap_or_else(|| panic!("{name}"))
        };
        for (name, loop_start_ms) in LOOPS {
            let row = fragment(name);
            assert!(row.loops, "{name}");
            assert_eq!(row.length_ms, loop_start_ms, "{name}");
            assert!(!row.done_at(u64::MAX / 2, 0), "{name}");
        }
        for (name, length_ms) in BEAST_FORM {
            let row = fragment(name);
            assert!(!row.loops, "{name}");
            assert_eq!(row.length_ms, length_ms, "{name}");
        }
        let back_to_aim = |transition: &FragmentTransition| {
            transition.to == TransitionTo::Action(BACK_TO_AIM.0.to_owned())
                && transition.start_ms == BACK_TO_AIM.1
        };
        let musket = table
            .lists
            .iter()
            .filter(|list| list.weapon_classes.iter().any(|class| class == "Musket"))
            .find_map(|list| {
                list.fragment(DRY_FIRE)
                    .filter(|row| row.transitions.iter().any(back_to_aim))
            })
            .expect(DRY_FIRE);
        assert_eq!((musket.length_ms, musket.loops), (DRY_FIRE_MS, false));
        assert!(musket.markers_at(FORCE_RELOAD.1).contains(&FORCE_RELOAD.0));
    }
}
