// SPDX-License-Identifier: AGPL-3.0-or-later
//! The content folder layout (README "Content data"): one folder per client build
//! (`<content_dir>/<build>/`), one folder per topic in it, and the registry of every content file
//! ([`FILES`]) and of the `content-tool` steps that make them ([`STEPS`], in build order).
//! [`ContentLayout`] is the one place that knows the path of a content file. Every content write
//! goes through [`write_atomic`] or [`link_atomic`], so a reader never sees half a file and a
//! hard link is never written through.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::manifest::Manifest;

/// The environment variable that names the content folder of every program (README "Content
/// data", "Where the content folder lives").
pub const CONTENT_DIR_ENV: &str = "NW_CONTENT_DIR";
/// The content folder when [`CONTENT_DIR_ENV`] is not set, from the folder the program runs in.
pub const DEFAULT_CONTENT_DIR: &str = "local-data/content";
/// The folder of the admin map tiles, beside the build folders (`<content_dir>/map/<level>/`).
pub const MAP_FOLDER: &str = "map";
/// The folder of the level folders inside a topic folder.
const LEVELS_FOLDER: &str = "levels";
/// The extension a file has while it is written, before the rename.
const TEMP_EXTENSION: &str = "tmp";
/// What a level-scoped file shows in place of its level in [`ContentFile::pattern`].
pub const LEVEL_PLACEHOLDER: &str = "<level>";

/// The tutorial level.
pub const TUTORIAL_LEVEL: &str = "ftue_v2";
/// The open world level.
pub const OPEN_WORLD_LEVEL: &str = "newworld_vitaeeterna";
/// The catacombs level (its collision holds the art tiles the server places).
pub const CATACOMB_LEVEL: &str = "nw_catacomb_00";
/// The expedition levels: every dungeon level of the build's game mode and dungeon tables.
pub const EXPEDITION_LEVELS: [&str; 12] = [
    "nw_dungeon_brimstonesands_00",
    "nw_dungeon_cutlasskeys_00",
    "nw_dungeon_dunwood_02",
    "nw_dungeon_edengrove_00",
    "nw_dungeon_everfall_00",
    "nw_dungeon_firstlight_01",
    "nw_dungeon_greatcleave_00",
    "nw_dungeon_greatcleave_01",
    "nw_dungeon_reekwater_00",
    "nw_dungeon_restlessshores_01",
    "nw_dungeon_shattermtn_00",
    "nw_dungeon_windsward_00",
];
/// The trial and raid levels: every level of the build's game mode table that a quest trial, a
/// season trial or a raid opens as an instance (`GameModeMapData` `CoatlicueName` other than the
/// open world, the expeditions, the catacombs and the player-versus-player maps).
pub const TRIAL_LEVELS: [&str; 13] = [
    "nw_ori_eg_questmotherwell",
    "nw_ori_er_questliang",
    "nw_ori_fl_questadiana",
    "nw_ori_gc_questnihilo",
    "nw_ori_sm_questisabella",
    "nw_raid_cutlasskeys_00",
    "nw_raid_monarchbluff_00",
    "nw_trial_dw_dracula",
    "nw_trial_season_02",
    "nw_trial_season_02_q13",
    "nw_trial_season_04",
    "nw_trial_season_04_daichidojo",
    "nw_trial_season_04_deviceroom",
];
/// The player-versus-player levels: every map of the build's arena, Outpost Rush and capture
/// the flag game modes (the arena's map rotation `NW_Arena01`, `NW_Arena02`, `NW_Arena_3v3_04`;
/// the Outpost Rush maps Trench and Vertical, its island map is the open world; the capture the
/// flag maps Wide and Long). An instance sim of a level without a collision file stops at its
/// start (live 2026-11-23 03:08:48Z: three sims of `nw_arena_3v3_04` in a row).
pub const PVP_LEVELS: [&str; 7] = [
    "nw_arena01",
    "nw_arena02",
    "nw_arena_3v3_04",
    "nw_opr_004_trench",
    "nw_opr_005_vertical",
    "nw_ctf_002_wide",
    "nw_ctf_003_long",
];
/// The levels that are not instance levels and have a collision file: the tutorial and the
/// open world.
const WORLD_COLLISION_LEVELS: [&str; 2] = [TUTORIAL_LEVEL, OPEN_WORLD_LEVEL];
/// The number of [`COLLISION_LEVELS`].
const COLLISION_LEVEL_COUNT: usize =
    WORLD_COLLISION_LEVELS.len() + EXPEDITION_LEVELS.len() + TRIAL_LEVELS.len() + PVP_LEVELS.len();
/// The levels whose collision file `collision` builds: the tutorial, the open world, the
/// expeditions, the trial and raid levels and the player-versus-player levels (the catacombs
/// file comes from `catacomb-tiles`).
pub const COLLISION_LEVELS: [&str; COLLISION_LEVEL_COUNT] = collision_levels();

/// [`WORLD_COLLISION_LEVELS`], then [`EXPEDITION_LEVELS`], [`TRIAL_LEVELS`] and [`PVP_LEVELS`].
const fn collision_levels() -> [&'static str; COLLISION_LEVEL_COUNT] {
    let mut levels = [""; COLLISION_LEVEL_COUNT];
    let mut at = 0;
    let groups: [&[&str]; 4] = [
        &WORLD_COLLISION_LEVELS,
        &EXPEDITION_LEVELS,
        &TRIAL_LEVELS,
        &PVP_LEVELS,
    ];
    let mut group = 0;
    while group < groups.len() {
        let mut index = 0;
        while index < groups[group].len() {
            levels[at] = groups[group][index];
            at += 1;
            index += 1;
        }
        group += 1;
    }
    levels
}
/// The levels whose water file `water` builds.
pub const WATER_LEVELS: [&str; 2] = [TUTORIAL_LEVEL, OPEN_WORLD_LEVEL];
/// The levels whose road file `roads` builds: the open world, whose tract maps hold the roads.
pub const ROADS_LEVELS: [&str; 1] = [OPEN_WORLD_LEVEL];
/// The levels whose doors file `level-doors` builds.
pub const LEVEL_DOORS_LEVELS: [&str; 1] = [OPEN_WORLD_LEVEL];
/// The level of the collision file `catacomb-tiles` builds.
const CATACOMB_TILES_LEVELS: [&str; 1] = [CATACOMB_LEVEL];

/// The topic folder of a content file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Topic {
    /// The content bundle and the design data.
    GameData,
    /// The files of one level, a folder per level.
    Levels,
    /// Player attacks, abilities and spells.
    Combat,
    /// Mob looks, animations and AI.
    Mobs,
    /// Dungeons, breaches and the catacombs.
    Instances,
    /// Towns, encounters, claims and camps.
    OpenWorld,
    /// Wars, invasions and Outpost Rush.
    Wars,
    /// Music and courses.
    Activities,
    /// Tables of the services beside the game servers.
    Services,
    /// The raw files from the install, before the design files are merged.
    Retail,
}

impl Topic {
    /// Every topic, in folder order.
    pub const ALL: [Self; 10] = [
        Self::GameData,
        Self::Levels,
        Self::Combat,
        Self::Mobs,
        Self::Instances,
        Self::OpenWorld,
        Self::Wars,
        Self::Activities,
        Self::Services,
        Self::Retail,
    ];

    /// The folder name.
    pub const fn folder(self) -> &'static str {
        match self {
            Self::GameData => "game-data",
            Self::Levels => LEVELS_FOLDER,
            Self::Combat => "combat",
            Self::Mobs => "mobs",
            Self::Instances => "instances",
            Self::OpenWorld => "open-world",
            Self::Wars => "wars",
            Self::Activities => "activities",
            Self::Services => "services",
            Self::Retail => "retail",
        }
    }

    /// What the folder holds.
    pub const fn summary(self) -> &'static str {
        match self {
            Self::GameData => "the merged content bundle and the design data (scripts and events)",
            Self::Levels => "one folder per level: world, collision, roads, water and doors",
            Self::Combat => "player attacks, abilities, sequences, spells, shots and effect areas",
            Self::Mobs => "mob looks, creature animations and the AI data",
            Self::Instances => "dungeon scripts, gates, entrances, breach tiers and catacomb tiles",
            Self::OpenWorld => "town props and stations, encounters, claim plots and camps",
            Self::Wars => "war forts, invasion forts and Outpost Rush structures",
            Self::Activities => "music charts and quick courses",
            Self::Services => {
                "the store table (edge-http) and the leaderboard table (status-reporter)"
            }
            Self::Retail => {
                "the raw bundle and world files from the install, the input of the design merge"
            }
        }
    }
}

/// Where the data of a content file comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Raw from the install (`content-tool build` and `world`).
    Retail,
    /// Made by `content-tool` from the install (`NW_GAME_DIR`).
    Install,
    /// Capture-made: the realm data pack (`just realm-data`).
    Pack,
    /// Made from other content files (and the install; the AI data also reads `Server.pak`).
    Derived,
    /// The design merge (`just design`): the retail files with the design files over them.
    Merged,
}

impl Source {
    /// The name the manifest and the status table show.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Retail => "retail",
            Self::Install => "install",
            Self::Pack => "pack",
            Self::Derived => "derived",
            Self::Merged => "merged",
        }
    }
}

/// Which levels a content file exists for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// One file per build.
    Build,
    /// One file per level, in the level's folder.
    Level,
    /// One file per build, in the folder of this one level.
    FixedLevel(&'static str),
}

/// One content file of the registry.
#[derive(Debug, PartialEq, Eq)]
pub struct ContentFile {
    /// The registry name: the stem of the old flat file name (`mob-prefabs`), the manifest key
    /// and the realm pack entry stem.
    pub name: &'static str,
    /// The topic folder.
    pub topic: Topic,
    /// The file name in its folder.
    pub file_name: &'static str,
    /// Which levels it exists for.
    pub scope: Scope,
    /// Where its data comes from.
    pub source: Source,
}

impl ContentFile {
    /// The file's path in a build folder, with [`LEVEL_PLACEHOLDER`] for a level-scoped file
    /// (`levels/<level>/world.bin`).
    pub fn pattern(&self) -> String {
        relative_path(self, Some(LEVEL_PLACEHOLDER))
            .to_string_lossy()
            .into_owned()
    }

    /// The file's extension with its dot (`.json`).
    pub fn extension(&self) -> &'static str {
        self.file_name
            .rfind('.')
            .map_or("", |dot| &self.file_name[dot..])
    }
}

/// A content file with one file per build (or per build in one fixed level's folder).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildFile(pub &'static ContentFile);

/// A content file with one file per level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelFile(pub &'static ContentFile);

/// Declares the registry: one constant per content file and [`FILES`], the list of them all.
macro_rules! content_files {
    ($($(#[$doc:meta])* $constant:ident: $handle:ident = $name:literal, $topic:ident, $file_name:literal, $scope:expr, $source:ident;)*) => {
        $(
            $(#[$doc])*
            pub const $constant: $handle = $handle(&ContentFile {
                name: $name,
                topic: Topic::$topic,
                file_name: $file_name,
                scope: $scope,
                source: Source::$source,
            });
        )*
        /// Every content file of the registry.
        pub const FILES: &[&ContentFile] = &[$($constant.0),*];
    };
}

content_files! {
    /// The retail content bundle (`content-tool build`).
    RETAIL_BUNDLE: BuildFile = "retail-bundle", Retail, "bundle.bin", Scope::Build, Retail;
    /// The retail world file of a level (`content-tool world`).
    RETAIL_WORLD: LevelFile = "retail-world", Retail, "world.bin", Scope::Level, Retail;
    /// The merged content bundle the servers read.
    BUNDLE: BuildFile = "bundle", GameData, "bundle.bin", Scope::Build, Merged;
    /// The scripts and events of the design files.
    DESIGN: BuildFile = "design", GameData, "design.json", Scope::Build, Merged;
    /// The merged world file of a level.
    WORLD: LevelFile = "world", Levels, "world.bin", Scope::Level, Merged;
    /// The water surfaces of a level.
    WATER: LevelFile = "water", Levels, "water.bin", Scope::Level, Install;
    /// The solid shapes of a level.
    COLLISION: LevelFile = "collision", Levels, "collision.bin", Scope::Level, Install;
    /// The road cells of a level.
    ROADS: LevelFile = "roads", Levels, "roads.bin", Scope::Level, Install;
    /// The door capitals of a level.
    LEVEL_DOORS: LevelFile = "level-doors", Levels, "doors.json", Scope::Level, Install;
    /// The tutorial's level entities.
    FTUE_ENTITIES: BuildFile = "ftue-entities", Levels, "entities.json", Scope::FixedLevel(TUTORIAL_LEVEL), Pack;
    /// The weapon stances' basic attacks.
    PLAYER_ATTACKS: BuildFile = "player-attacks", Combat, "player-attacks.json", Scope::Build, Install;
    /// The weapon abilities.
    PLAYER_ABILITIES: BuildFile = "player-abilities", Combat, "player-abilities.json", Scope::Build, Install;
    /// The attack, block, aim, dodge and jump sequences.
    PLAYER_SEQUENCES: BuildFile = "player-sequences", Combat, "player-sequences.json", Scope::Build, Pack;
    /// The instance type of every ability instance slice.
    ABILITY_INSTANCES: BuildFile = "ability-instances", Combat, "ability-instances.json", Scope::Build, Install;
    /// The entities a cast spell shows.
    SPELL_SLICES: BuildFile = "spell-slices", Combat, "spell-slices.json", Scope::Build, Pack;
    /// The entities a shot shows.
    SHOT_ENTITIES: BuildFile = "shot-entities", Combat, "shot-entities.json", Scope::Build, Pack;
    /// The status effect areas of levels and spells.
    EFFECT_AREAS: BuildFile = "effect-areas", Combat, "effect-areas.json", Scope::Build, Derived;
    /// The mob prefabs (looks) of the captures.
    MOB_PREFABS: BuildFile = "mob-prefabs", Mobs, "mob-prefabs.json", Scope::Build, Pack;
    /// The creature animations.
    CREATURE_ANIMATIONS: BuildFile = "creature-animations", Mobs, "creature-animations.json", Scope::Build, Derived;
    /// The behavior trees and the AI of every creature slice.
    AI_DATA: BuildFile = "ai-data", Mobs, "ai-data.bin", Scope::Build, Derived;
    /// The encounters and loot containers of every instance level.
    DUNGEON_SCRIPTS: BuildFile = "dungeon-scripts", Instances, "dungeon-scripts.json", Scope::Build, Derived;
    /// The gates, doors and blockers of every instance level.
    DUNGEON_GATES: BuildFile = "dungeon-gates", Instances, "dungeon-gates.json", Scope::Build, Install;
    /// When each gate opens.
    DUNGEON_GATE_RULES: BuildFile = "dungeon-gate-rules", Instances, "dungeon-gate-rules.json", Scope::Build, Pack;
    /// The runs each dungeon entrance shows.
    DUNGEON_ENTRANCES: BuildFile = "dungeon-entrances", Instances, "dungeon-entrances.json", Scope::Build, Pack;
    /// The difficulty of each corrupted breach place.
    BREACH_TIERS: BuildFile = "breach-tiers", Instances, "breach-tiers.json", Scope::Build, Pack;
    /// The catacombs tile variations.
    CATACOMB_TILES: BuildFile = "catacomb-tiles", Instances, "catacomb-tiles.json", Scope::Build, Derived;
    /// The station buildings, lights and house plots of the towns.
    TOWN_PROPS: BuildFile = "town-props", OpenWorld, "town-props.json", Scope::Build, Pack;
    /// The crafting and refining stations of the towns.
    TOWN_STATIONS: BuildFile = "town-stations", OpenWorld, "town-stations.json", Scope::Build, Pack;
    /// The encounter states of the world events.
    ENCOUNTER_ROOTS: BuildFile = "encounter-roots", OpenWorld, "encounter-roots.json", Scope::Build, Pack;
    /// The start objects of the world events.
    ENCOUNTER_OBJECTS: BuildFile = "encounter-objects", OpenWorld, "encounter-objects.json", Scope::Build, Pack;
    /// The land claim entities of every territory.
    CLAIM_PLOTS: BuildFile = "claim-plots", OpenWorld, "claim-plots.json", Scope::Build, Install;
    /// The camp slices.
    CAMP_SLICES: BuildFile = "camp-slices", OpenWorld, "camp-slices.json", Scope::Build, Install;
    /// The war instances of every fort.
    WAR_FORTS: BuildFile = "war-forts", Wars, "war-forts.json", Scope::Build, Pack;
    /// The invasions of every fort.
    INVASION_FORTS: BuildFile = "invasion-forts", Wars, "invasion-forts.json", Scope::Build, Pack;
    /// The Outpost Rush structures.
    OUTPOST_STRUCTURES: BuildFile = "outpost-structures", Wars, "outpost-structures.json", Scope::Build, Pack;
    /// The notes of every music sheet.
    MUSIC_CHARTS: BuildFile = "music-charts", Activities, "music-charts.json", Scope::Build, Install;
    /// The nodes of every mount race, rally, tour and time trial.
    QUICK_COURSES: BuildFile = "quick-courses", Activities, "quick-courses.json", Scope::Build, Install;
    /// The store mounts and the entitlements every account holds (edge-http).
    STORE: BuildFile = "store", Services, "store.json", Scope::Build, Derived;
    /// Every board of a world with its English name and group (status-reporter).
    LEADERBOARDS: BuildFile = "leaderboards", Services, "leaderboards.json", Scope::Build, Derived;
}

/// A content file of a step: one file per build, or the step's levels of a level file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepFile {
    /// A file per build.
    Build(BuildFile),
    /// A file per level, at the step's levels.
    Level(LevelFile),
}

impl StepFile {
    /// The registry entry.
    pub const fn file(self) -> &'static ContentFile {
        match self {
            Self::Build(file) => file.0,
            Self::Level(file) => file.0,
        }
    }
}

/// The levels of a step's level files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepLevels {
    /// The step has no level file.
    None,
    /// These levels.
    Named(&'static [&'static str]),
    /// Every level of the install (the retail world files).
    Every,
}

/// A `content-tool` step that makes content files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// `build`: the retail bundle.
    RetailBundle,
    /// `world`: the retail world files.
    RetailWorld,
    /// `design`: the merged bundle, world files and design data.
    Design,
    /// `water`.
    Water,
    /// `collision`.
    Collision,
    /// `roads`.
    Roads,
    /// `level-doors`.
    LevelDoors,
    /// `player-attacks`.
    PlayerAttacks,
    /// `player-abilities`.
    PlayerAbilities,
    /// `dungeon-gates`.
    DungeonGates,
    /// `ability-instances`.
    AbilityInstances,
    /// `claim-plots`.
    ClaimPlots,
    /// `music-charts`.
    MusicCharts,
    /// `camp-slices`.
    CampSlices,
    /// `quick-courses`.
    QuickCourses,
    /// `store`.
    Store,
    /// `leaderboards`.
    Leaderboards,
    /// `creature-animations`.
    CreatureAnimations,
    /// `catacomb-tiles`.
    CatacombTiles,
    /// `dungeon-scripts`.
    DungeonScripts,
    /// `effect-areas`.
    EffectAreas,
    /// `ai-data`.
    AiData,
}

/// What a step makes and reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepSpec {
    /// The step.
    pub step: Step,
    /// The `content-tool` subcommand.
    pub command: &'static str,
    /// The levels of its level files.
    pub levels: StepLevels,
    /// The files it writes.
    pub outputs: &'static [StepFile],
    /// The content files it reads.
    pub inputs: &'static [StepFile],
    /// True when it reads the install (`NW_GAME_DIR`).
    pub needs_game_dir: bool,
    /// True when it reads `Server.pak`.
    pub needs_server_pak: bool,
}

impl StepSpec {
    /// True when the step makes a derived file (`content-tool content build --derived`).
    pub fn is_derived(&self) -> bool {
        self.outputs
            .iter()
            .any(|output| output.file().source == Source::Derived)
    }

    /// The step's input files of source [`Source::Pack`].
    pub fn pack_inputs(&self) -> impl Iterator<Item = &'static ContentFile> {
        self.inputs
            .iter()
            .map(|input| input.file())
            .filter(|file| file.source == Source::Pack)
    }

    /// The levels of the step's level files: its named levels, or the levels that have a retail
    /// world file in `layout` for a step of every level.
    ///
    /// # Errors
    ///
    /// Returns [`LayoutError::Io`] when the levels of `layout` cannot be listed.
    pub fn level_names(&self, layout: &ContentLayout) -> Result<Vec<String>, LayoutError> {
        Ok(match self.levels {
            StepLevels::None => Vec::new(),
            StepLevels::Named(levels) => levels.iter().map(|&level| level.to_owned()).collect(),
            StepLevels::Every => layout.levels_with(RETAIL_WORLD)?,
        })
    }

    /// True when the step writes `file`.
    pub fn makes(&self, file: &ContentFile) -> bool {
        self.outputs.iter().any(|output| output.file() == file)
    }

    /// True when the step writes `file` at `level` (`None` for a file per build).
    pub fn makes_at(&self, file: &ContentFile, level: Option<&str>) -> bool {
        self.makes(file)
            && match (self.levels, level) {
                (_, None) => true,
                (StepLevels::Named(levels), Some(level)) => levels.contains(&level),
                (StepLevels::Every, Some(_)) => true,
                (StepLevels::None, Some(_)) => false,
            }
    }
}

/// A step of [`STEPS`] that reads the install.
const fn install_step(
    step: Step,
    command: &'static str,
    levels: StepLevels,
    outputs: &'static [StepFile],
    inputs: &'static [StepFile],
) -> StepSpec {
    StepSpec {
        step,
        command,
        levels,
        outputs,
        inputs,
        needs_game_dir: true,
        needs_server_pak: false,
    }
}

/// Every step, in build order: each step's inputs come from steps before it or the pack.
pub const STEPS: [StepSpec; 22] = [
    install_step(
        Step::RetailBundle,
        "build",
        StepLevels::None,
        &[StepFile::Build(RETAIL_BUNDLE)],
        &[],
    ),
    install_step(
        Step::RetailWorld,
        "world",
        StepLevels::Every,
        &[StepFile::Level(RETAIL_WORLD)],
        &[StepFile::Build(RETAIL_BUNDLE)],
    ),
    StepSpec {
        step: Step::Design,
        command: "design",
        levels: StepLevels::Every,
        outputs: &[
            StepFile::Build(BUNDLE),
            StepFile::Build(DESIGN),
            StepFile::Level(WORLD),
        ],
        inputs: &[
            StepFile::Build(RETAIL_BUNDLE),
            StepFile::Level(RETAIL_WORLD),
        ],
        needs_game_dir: false,
        needs_server_pak: false,
    },
    install_step(
        Step::Water,
        "water",
        StepLevels::Named(&WATER_LEVELS),
        &[StepFile::Level(WATER)],
        &[],
    ),
    install_step(
        Step::Collision,
        "collision",
        StepLevels::Named(&COLLISION_LEVELS),
        &[StepFile::Level(COLLISION)],
        &[StepFile::Level(WORLD), StepFile::Level(WATER)],
    ),
    install_step(
        Step::Roads,
        "roads",
        StepLevels::Named(&ROADS_LEVELS),
        &[StepFile::Level(ROADS)],
        &[],
    ),
    install_step(
        Step::LevelDoors,
        "level-doors",
        StepLevels::Named(&LEVEL_DOORS_LEVELS),
        &[StepFile::Level(LEVEL_DOORS)],
        &[StepFile::Level(WORLD)],
    ),
    install_step(
        Step::PlayerAttacks,
        "player-attacks",
        StepLevels::None,
        &[StepFile::Build(PLAYER_ATTACKS)],
        &[StepFile::Build(BUNDLE)],
    ),
    install_step(
        Step::PlayerAbilities,
        "player-abilities",
        StepLevels::None,
        &[StepFile::Build(PLAYER_ABILITIES)],
        &[StepFile::Build(BUNDLE)],
    ),
    install_step(
        Step::DungeonGates,
        "dungeon-gates",
        StepLevels::None,
        &[StepFile::Build(DUNGEON_GATES)],
        &[StepFile::Build(BUNDLE)],
    ),
    install_step(
        Step::AbilityInstances,
        "ability-instances",
        StepLevels::None,
        &[StepFile::Build(ABILITY_INSTANCES)],
        &[],
    ),
    install_step(
        Step::ClaimPlots,
        "claim-plots",
        StepLevels::None,
        &[StepFile::Build(CLAIM_PLOTS)],
        &[],
    ),
    install_step(
        Step::MusicCharts,
        "music-charts",
        StepLevels::None,
        &[StepFile::Build(MUSIC_CHARTS)],
        &[],
    ),
    install_step(
        Step::CampSlices,
        "camp-slices",
        StepLevels::None,
        &[StepFile::Build(CAMP_SLICES)],
        &[],
    ),
    install_step(
        Step::QuickCourses,
        "quick-courses",
        StepLevels::None,
        &[StepFile::Build(QUICK_COURSES)],
        &[],
    ),
    StepSpec {
        step: Step::Store,
        command: "store",
        levels: StepLevels::None,
        outputs: &[StepFile::Build(STORE)],
        inputs: &[StepFile::Build(BUNDLE)],
        needs_game_dir: false,
        needs_server_pak: false,
    },
    StepSpec {
        step: Step::Leaderboards,
        command: "leaderboards",
        levels: StepLevels::None,
        outputs: &[StepFile::Build(LEADERBOARDS)],
        inputs: &[StepFile::Build(BUNDLE)],
        needs_game_dir: false,
        needs_server_pak: false,
    },
    install_step(
        Step::CreatureAnimations,
        "creature-animations",
        StepLevels::None,
        &[StepFile::Build(CREATURE_ANIMATIONS)],
        &[StepFile::Build(MOB_PREFABS), StepFile::Build(BUNDLE)],
    ),
    install_step(
        Step::CatacombTiles,
        "catacomb-tiles",
        StepLevels::Named(&CATACOMB_TILES_LEVELS),
        &[StepFile::Build(CATACOMB_TILES), StepFile::Level(COLLISION)],
        &[StepFile::Build(MOB_PREFABS), StepFile::Build(BUNDLE)],
    ),
    install_step(
        Step::DungeonScripts,
        "dungeon-scripts",
        StepLevels::None,
        &[StepFile::Build(DUNGEON_SCRIPTS)],
        &[
            StepFile::Build(BREACH_TIERS),
            StepFile::Build(MOB_PREFABS),
            StepFile::Build(BUNDLE),
        ],
    ),
    install_step(
        Step::EffectAreas,
        "effect-areas",
        StepLevels::None,
        &[StepFile::Build(EFFECT_AREAS)],
        &[StepFile::Build(DUNGEON_SCRIPTS), StepFile::Build(BUNDLE)],
    ),
    StepSpec {
        step: Step::AiData,
        command: "ai-data",
        levels: StepLevels::None,
        outputs: &[StepFile::Build(AI_DATA)],
        inputs: &[StepFile::Build(CREATURE_ANIMATIONS)],
        needs_game_dir: true,
        needs_server_pak: true,
    },
];

/// The pack tables: the capture-made files, in registry order.
pub fn pack_files() -> impl Iterator<Item = &'static ContentFile> {
    FILES
        .iter()
        .copied()
        .filter(|file| file.source == Source::Pack)
}

/// One content file at one level (`level` is `None` for a file per build).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRef {
    /// The registry entry.
    pub file: &'static ContentFile,
    /// The level of a level-scoped file.
    pub level: Option<String>,
}

/// The separator of a manifest key's name and level.
const KEY_LEVEL_SEPARATOR: char = '/';

impl FileRef {
    /// A file per build.
    pub const fn build(file: BuildFile) -> Self {
        Self {
            file: file.0,
            level: None,
        }
    }

    /// A level file at `level`.
    pub fn level(file: LevelFile, level: &str) -> Self {
        Self {
            file: file.0,
            level: Some(level.to_owned()),
        }
    }

    /// The manifest key: `<name>` or `<name>/<level>`.
    pub fn key(&self) -> String {
        match &self.level {
            Some(level) => format!("{}{KEY_LEVEL_SEPARATOR}{level}", self.file.name),
            None => self.file.name.to_owned(),
        }
    }

    /// The content files the step that makes this file reads for it: its level files at this
    /// file's level, or at every level of the step for a file per build (the retail world files
    /// of `layout` for the design step).
    ///
    /// # Errors
    ///
    /// Returns [`LayoutError::Io`] when the levels of `layout` cannot be listed.
    pub fn inputs(&self, layout: &ContentLayout) -> Result<Vec<Self>, LayoutError> {
        let Some(step) = STEPS
            .iter()
            .find(|step| step.makes_at(self.file, self.level.as_deref()))
        else {
            return Ok(Vec::new());
        };
        let levels = match &self.level {
            Some(level) => vec![level.clone()],
            None => step.level_names(layout)?,
        };
        Ok(step
            .inputs
            .iter()
            .flat_map(|input| match *input {
                StepFile::Build(file) => vec![Self::build(file)],
                StepFile::Level(file) => levels
                    .iter()
                    .map(|level| Self::level(file, level))
                    .collect(),
            })
            .collect())
    }

    /// The manifest keys of [`FileRef::inputs`].
    ///
    /// # Errors
    ///
    /// Returns [`LayoutError::Io`] when the levels of `layout` cannot be listed.
    pub fn input_keys(&self, layout: &ContentLayout) -> Result<Vec<String>, LayoutError> {
        Ok(self.inputs(layout)?.iter().map(Self::key).collect())
    }
}

/// The path of `file` inside a build folder; `level` names the level folder of a level-scoped
/// file.
fn relative_path(file: &ContentFile, level: Option<&str>) -> PathBuf {
    let level = match file.scope {
        Scope::FixedLevel(fixed) => Some(fixed),
        Scope::Build => None,
        Scope::Level => level,
    };
    match level {
        Some(level) => levels_folder(file.topic).join(level),
        None => PathBuf::from(file.topic.folder()),
    }
    .join(file.file_name)
}

/// The folder of the level folders of `topic`: `levels/` itself, else `<topic>/levels/`.
fn levels_folder(topic: Topic) -> PathBuf {
    let folder = PathBuf::from(topic.folder());
    if topic == Topic::Levels {
        folder
    } else {
        folder.join(LEVELS_FOLDER)
    }
}

/// Why a content folder cannot be opened or its manifest written.
#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    /// A file cannot be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The manifest is not valid JSON.
    #[error("{path}: {source}")]
    Json {
        /// The manifest.
        path: PathBuf,
        /// Why.
        source: serde_json::Error,
    },
    /// The manifest names another build than its folder.
    #[error(
        "{path}: the manifest is for build {manifest}, its folder is for build {folder}: run content-tool content build again"
    )]
    BuildMismatch {
        /// The manifest.
        path: PathBuf,
        /// The build of the manifest.
        manifest: String,
        /// The build of the folder.
        folder: String,
    },
}

/// The content folder: the value of [`CONTENT_DIR_ENV`] (a relative value starts at the folder
/// the program runs in, an absolute one works anywhere), else [`DEFAULT_CONTENT_DIR`].
pub fn content_dir() -> PathBuf {
    content_dir_of(std::env::var_os(CONTENT_DIR_ENV))
}

/// The content folder for the value `value` of [`CONTENT_DIR_ENV`]: an unset or empty value
/// gives [`DEFAULT_CONTENT_DIR`].
fn content_dir_of(value: Option<std::ffi::OsString>) -> PathBuf {
    value
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from(DEFAULT_CONTENT_DIR), PathBuf::from)
}

/// The paths of the content files of one client build in a content folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentLayout {
    content_dir: PathBuf,
    client_build: String,
    build_dir: PathBuf,
}

impl ContentLayout {
    /// The layout of `client_build` in `content_dir`, for writing; nothing is read.
    pub fn new(content_dir: impl Into<PathBuf>, client_build: &str) -> Self {
        let content_dir = content_dir.into();
        let build_dir = content_dir.join(client_build);
        Self {
            content_dir,
            client_build: client_build.to_owned(),
            build_dir,
        }
    }

    /// The layout of `client_build` in `content_dir`, for reading: when the build folder has a
    /// manifest, its build must be the folder's build.
    ///
    /// # Errors
    ///
    /// Returns [`LayoutError`] when the manifest does not read or names another build.
    pub fn open(content_dir: impl Into<PathBuf>, client_build: &str) -> Result<Self, LayoutError> {
        let layout = Self::new(content_dir, client_build);
        if let Some(manifest) = layout.read_manifest()?
            && manifest.build != layout.client_build
        {
            return Err(LayoutError::BuildMismatch {
                path: layout.manifest_path(),
                manifest: manifest.build,
                folder: layout.client_build.clone(),
            });
        }
        Ok(layout)
    }

    /// The content folder (from [`content_dir`] for the programs).
    pub fn content_dir(&self) -> &Path {
        &self.content_dir
    }

    /// The client build.
    pub fn client_build(&self) -> &str {
        &self.client_build
    }

    /// The build folder: `<content_dir>/<build>/`.
    pub fn build_dir(&self) -> &Path {
        &self.build_dir
    }

    /// The folder of the admin map tiles: `<content_dir>/map/`.
    pub fn map_dir(&self) -> PathBuf {
        self.content_dir.join(MAP_FOLDER)
    }

    /// The path of a file per build.
    pub fn file(&self, file: BuildFile) -> PathBuf {
        self.build_dir.join(relative_path(file.0, None))
    }

    /// The path of a level file at `level`.
    pub fn level_file(&self, file: LevelFile, level: &str) -> PathBuf {
        self.build_dir.join(relative_path(file.0, Some(level)))
    }

    /// The path of `file`.
    pub fn path(&self, file: &FileRef) -> PathBuf {
        self.build_dir
            .join(relative_path(file.file, file.level.as_deref()))
    }

    /// The levels that have `file` in this build folder.
    ///
    /// # Errors
    ///
    /// Returns [`LayoutError::Io`] when the levels folder exists and cannot be listed.
    pub fn levels_with(&self, file: LevelFile) -> Result<Vec<String>, LayoutError> {
        let levels_dir = self.build_dir.join(levels_folder(file.0.topic));
        let io = |source| LayoutError::Io {
            path: levels_dir.clone(),
            source,
        };
        let entries = match std::fs::read_dir(&levels_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(io(error)),
        };
        let mut levels = Vec::new();
        for entry in entries {
            let entry = entry.map_err(io)?;
            if let Some(level) = entry.file_name().to_str()
                && self.level_file(file, level).is_file()
            {
                levels.push(level.to_owned());
            }
        }
        levels.sort();
        Ok(levels)
    }

    /// The manifest: `<build>/manifest.json`.
    pub fn manifest_path(&self) -> PathBuf {
        self.build_dir.join(crate::manifest::MANIFEST_FILE)
    }

    /// The manifest, or `None` when the build folder has none.
    ///
    /// # Errors
    ///
    /// Returns [`LayoutError`] when it does not read.
    pub fn read_manifest(&self) -> Result<Option<Manifest>, LayoutError> {
        Manifest::read(&self.manifest_path())
    }
}

/// Temporary names made so far by this process: with the process id, each one is new.
static TEMP_COUNT: AtomicU64 = AtomicU64::new(0);

/// Temporary names tried before a write gives up: each try takes the next counter value, so
/// files a crashed run with the same process id left (pid 1 in a container) are stepped over.
const TEMP_NAME_TRIES: usize = 1_000;

/// A file written under a new temporary name in its target's folder, not yet renamed over the
/// target ([`PendingFile::commit`]). The temporary name is new for every file (process id and a
/// counter) and is created only when it does not exist, so no write ever goes into a file that
/// is there already (a hard link left by a crash, another writer's temporary file). A target
/// that holds the new bytes already (or is the file to link) is left as it is, so its time does
/// not change. Dropped without a commit, the temporary file is removed.
#[derive(Debug)]
pub struct PendingFile {
    /// The temporary file; `None` when the target needs no change.
    temp: Option<PathBuf>,
    target: PathBuf,
    committed: bool,
}

impl PendingFile {
    /// `bytes` under a new temporary name beside `target`, synced to disk; nothing when
    /// `target` holds `bytes` already. Creates parent folders.
    ///
    /// # Errors
    ///
    /// Returns the I/O error; no temporary file is left.
    pub fn write(target: &Path, bytes: &[u8]) -> std::io::Result<Self> {
        if holds(target, bytes) {
            return Ok(Self::unchanged(target));
        }
        let (pending, mut file) = Self::create(target)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(pending)
    }

    /// A hard link to `source` under a new temporary name beside `target`; a synced copy when
    /// the link fails (another file system); nothing when `target` is `source` already (the
    /// same file). Creates parent folders.
    ///
    /// # Errors
    ///
    /// Returns the I/O error; no temporary file is left.
    pub fn link(source: &Path, target: &Path) -> std::io::Result<Self> {
        if same_file(source, target) {
            return Ok(Self::unchanged(target));
        }
        create_parent(target)?;
        match with_new_temp_name(target, |temp| std::fs::hard_link(source, temp)) {
            Ok((temp, ())) => Ok(Self {
                temp: Some(temp),
                target: target.to_path_buf(),
                committed: false,
            }),
            Err(_) => {
                let (pending, mut file) = Self::create(target)?;
                std::io::copy(&mut std::fs::File::open(source)?, &mut file)?;
                file.sync_all()?;
                Ok(pending)
            }
        }
    }

    /// The target as it is: nothing to commit.
    fn unchanged(target: &Path) -> Self {
        Self {
            temp: None,
            target: target.to_path_buf(),
            committed: false,
        }
    }

    /// A new, empty temporary file beside `target`.
    fn create(target: &Path) -> std::io::Result<(Self, std::fs::File)> {
        create_parent(target)?;
        let (temp, file) = with_new_temp_name(target, |temp| {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(temp)
        })?;
        Ok((
            Self {
                temp: Some(temp),
                target: target.to_path_buf(),
                committed: false,
            },
            file,
        ))
    }

    /// Renames the temporary file over the target (a new file there, so a hard link to the old
    /// target keeps the old bytes) and syncs the folder; nothing when the target needs no
    /// change.
    ///
    /// # Errors
    ///
    /// Returns the I/O error; the temporary file is removed.
    pub fn commit(mut self) -> std::io::Result<()> {
        let Some(temp) = self.temp.clone() else {
            return Ok(());
        };
        std::fs::rename(&temp, &self.target)?;
        self.committed = true;
        // A rename onto a hard link of the same file does nothing and keeps both names.
        if temp.exists() {
            std::fs::remove_file(&temp)?;
        }
        sync_folder(&self.target)
    }
}

impl Drop for PendingFile {
    fn drop(&mut self) {
        if let Some(temp) = &self.temp
            && !self.committed
        {
            // A temporary file that is gone already needs nothing.
            let _ = std::fs::remove_file(temp);
        }
    }
}

/// Runs `make` on new temporary names beside `target` until one is free (`make` gives
/// `AlreadyExists` for a taken one), at most [`TEMP_NAME_TRIES`] times.
fn with_new_temp_name<T>(
    target: &Path,
    mut make: impl FnMut(&Path) -> std::io::Result<T>,
) -> std::io::Result<(PathBuf, T)> {
    let mut last = None;
    for _ in 0..TEMP_NAME_TRIES {
        let temp = new_temp_name(target);
        match make(&temp) {
            Ok(made) => return Ok((temp, made)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => last = Some(error),
            Err(error) => return Err(error),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::from(std::io::ErrorKind::AlreadyExists)))
}

/// A temporary name beside `target` that this process has not used: `<name>.<pid>.<n>.tmp`.
fn new_temp_name(target: &Path) -> PathBuf {
    let count = TEMP_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.{count}.{TEMP_EXTENSION}", std::process::id()));
    target.with_file_name(name)
}

/// True when the file `path` holds exactly `bytes`.
fn holds(path: &Path, bytes: &[u8]) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| {
        metadata.is_file()
            && metadata.len() == bytes.len() as u64
            && std::fs::read(path).is_ok_and(|held| held == bytes)
    })
}

/// True when `left` and `right` are one file (hard links of one inode).
fn same_file(left: &Path, right: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::metadata(left), std::fs::metadata(right)) {
            (Ok(left), Ok(right)) => left.dev() == right.dev() && left.ino() == right.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (left, right);
        false
    }
}

/// Syncs the folder of `path`, so a rename in it is on disk.
fn sync_folder(path: &Path) -> std::io::Result<()> {
    match path.parent() {
        Some(parent) if cfg!(unix) && !parent.as_os_str().is_empty() => {
            std::fs::File::open(parent)?.sync_all()
        }
        _ => Ok(()),
    }
}

/// Writes `bytes` to `path` as one step ([`PendingFile::write`], then commit). Creates parent
/// folders.
///
/// # Errors
///
/// Returns the I/O error; no temporary file is left.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    PendingFile::write(path, bytes)?.commit()
}

/// Makes `path` a hard link to `source` (a copy on another file system) as one step
/// ([`PendingFile::link`], then commit). Creates parent folders.
///
/// # Errors
///
/// Returns the I/O error; no temporary file is left.
pub fn link_atomic(source: &Path, path: &Path) -> std::io::Result<()> {
    PendingFile::link(source, path)?.commit()
}

/// True when `path` is a temporary file of [`PendingFile`].
pub fn is_temp_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == TEMP_EXTENSION)
}

fn create_parent(path: &Path) -> std::io::Result<()> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => std::fs::create_dir_all(parent),
        _ => Ok(()),
    }
}

/// The content layout of this repository at the pinned build of `config/client-build.toml`, for
/// the tests that read the live content: the content folder of [`content_dir`], a relative one
/// from the repository folder (a test runs in its crate's folder).
///
/// # Panics
///
/// Panics when the client build file or the manifest does not read.
#[cfg(any(test, feature = "test-util"))]
pub fn repo_layout() -> ContentLayout {
    /// The repository folder, from this crate's folder.
    const REPO_FROM_CRATE: &str = "../..";
    /// The client build file of the repository.
    const REPO_CLIENT_BUILD_CONFIG: &str = "config/client-build.toml";
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join(REPO_FROM_CRATE);
    let build = crate::client_build::load_client_build(&repo.join(REPO_CLIENT_BUILD_CONFIG))
        .expect("client build");
    ContentLayout::open(repo.join(content_dir()), &build).expect("content layout")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    const BUILD: &str = "12345";
    /// The section of README.md with the content file table.
    const README_SECTION: &str = "## Content data";
    /// The README table cells of the content files start with this.
    const README_TABLE_CELL: &str = "| `";

    #[test]
    fn the_content_folder_is_the_variable_or_the_default() {
        assert_eq!(content_dir_of(None), Path::new(DEFAULT_CONTENT_DIR));
        assert_eq!(
            content_dir_of(Some("".into())),
            Path::new(DEFAULT_CONTENT_DIR)
        );
        let relative = content_dir_of(Some("game/content".into()));
        assert_eq!(relative, Path::new("game/content"));
        assert!(relative.is_relative());
        assert_eq!(
            content_dir_of(Some("/srv/blighted/content".into())),
            Path::new("/srv/blighted/content")
        );
    }

    #[test]
    fn the_collision_levels_are_the_world_the_expedition_the_trial_and_the_pvp_levels_once_each() {
        let expected: Vec<&str> = WORLD_COLLISION_LEVELS
            .iter()
            .chain(&EXPEDITION_LEVELS)
            .chain(&TRIAL_LEVELS)
            .chain(&PVP_LEVELS)
            .copied()
            .collect();
        assert_eq!(COLLISION_LEVELS.to_vec(), expected);
        let unique: BTreeSet<_> = COLLISION_LEVELS.iter().collect();
        assert_eq!(unique.len(), COLLISION_LEVELS.len());
        assert!(COLLISION_LEVELS.contains(&"nw_ori_gc_questnihilo"));
        assert!(COLLISION_LEVELS.contains(&"nw_arena_3v3_04"));
        assert!(!COLLISION_LEVELS.contains(&CATACOMB_LEVEL));
    }

    #[test]
    fn names_and_paths_are_unique() {
        let names: BTreeSet<_> = FILES.iter().map(|file| file.name).collect();
        assert_eq!(names.len(), FILES.len());
        let patterns: BTreeSet<_> = FILES.iter().map(|file| file.pattern()).collect();
        assert_eq!(patterns.len(), FILES.len());
    }

    #[test]
    fn the_paths_follow_the_topics() {
        let layout = ContentLayout::new("content", BUILD);
        let build = Path::new("content").join(BUILD);
        assert_eq!(
            layout.file(MOB_PREFABS),
            build.join("mobs/mob-prefabs.json")
        );
        assert_eq!(
            layout.level_file(WORLD, OPEN_WORLD_LEVEL),
            build.join("levels/newworld_vitaeeterna/world.bin")
        );
        assert_eq!(
            layout.level_file(RETAIL_WORLD, TUTORIAL_LEVEL),
            build.join("retail/levels/ftue_v2/world.bin")
        );
        assert_eq!(
            layout.file(FTUE_ENTITIES),
            build.join("levels/ftue_v2/entities.json")
        );
        assert_eq!(layout.file(STORE), build.join("services/store.json"));
        assert_eq!(layout.map_dir(), Path::new("content").join(MAP_FOLDER));
        assert_eq!(layout.manifest_path(), build.join("manifest.json"));
    }

    #[test]
    fn the_pack_tables_are_the_15_capture_made_files() {
        let names: Vec<_> = pack_files().map(|file| file.name).collect();
        assert_eq!(names.len(), 15);
        assert!(names.contains(&"ftue-entities"));
        assert!(names.contains(&"breach-tiers"));
    }

    #[test]
    fn every_step_input_is_made_before_it_or_comes_from_the_pack() {
        for (index, step) in STEPS.iter().enumerate() {
            for input in step.inputs {
                let file = input.file();
                assert!(
                    file.source == Source::Pack
                        || STEPS[..index].iter().any(|before| before.makes(file)),
                    "{} reads {}",
                    step.command,
                    file.name
                );
            }
        }
    }

    #[test]
    fn every_file_but_the_pack_tables_has_a_step() {
        for file in FILES {
            assert_eq!(
                STEPS.iter().any(|step| step.makes(file)),
                file.source != Source::Pack,
                "{}",
                file.name
            );
        }
    }

    #[test]
    fn input_keys_carry_the_level_of_a_level_file() {
        let dir = tempfile::tempdir().unwrap();
        let layout = ContentLayout::new(dir.path(), BUILD);
        let collision = FileRef::level(COLLISION, TUTORIAL_LEVEL);
        assert_eq!(collision.key(), "collision/ftue_v2");
        let keys = |file: &FileRef| file.input_keys(&layout).unwrap();
        assert_eq!(keys(&collision), ["world/ftue_v2", "water/ftue_v2"]);
        let catacombs = FileRef::level(COLLISION, CATACOMB_LEVEL);
        assert_eq!(keys(&catacombs), ["mob-prefabs", "bundle"]);
        assert!(keys(&FileRef::build(MOB_PREFABS)).is_empty());
        write_atomic(&layout.level_file(RETAIL_WORLD, TUTORIAL_LEVEL), b"x").unwrap();
        assert_eq!(
            keys(&FileRef::build(BUNDLE)),
            ["retail-bundle", "retail-world/ftue_v2"]
        );
    }

    #[test]
    fn levels_with_lists_the_level_folders_that_hold_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let layout = ContentLayout::new(dir.path(), BUILD);
        write_atomic(&layout.level_file(WORLD, TUTORIAL_LEVEL), b"x").unwrap();
        write_atomic(&layout.level_file(WATER, OPEN_WORLD_LEVEL), b"x").unwrap();
        assert_eq!(layout.levels_with(WORLD).unwrap(), [TUTORIAL_LEVEL]);
        assert!(layout.levels_with(RETAIL_WORLD).unwrap().is_empty());
    }

    /// The temporary files in `dir`.
    fn temp_files(dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .map(|entry| entry.unwrap().path())
                    .filter(|path| is_temp_file(path))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn write_atomic_replaces_the_file_and_leaves_a_link_alone() {
        let dir = tempfile::tempdir().unwrap();
        let retail = dir.path().join("a/retail.bin");
        let merged = dir.path().join("b/merged.bin");
        write_atomic(&retail, b"old").unwrap();
        link_atomic(&retail, &merged).unwrap();
        write_atomic(&retail, b"new").unwrap();
        assert_eq!(std::fs::read(&retail).unwrap(), b"new");
        assert_eq!(std::fs::read(&merged).unwrap(), b"old");
        assert!(temp_files(&dir.path().join("a")).is_empty());
        assert!(temp_files(&dir.path().join("b")).is_empty());
    }

    #[test]
    fn a_left_over_temporary_link_is_never_written_through() {
        let dir = tempfile::tempdir().unwrap();
        let retail = dir.path().join("retail.bin");
        let merged = dir.path().join("merged.bin");
        write_atomic(&retail, b"retail").unwrap();
        // A crash between the link and the rename of an older writer left this.
        let left_over = dir.path().join("merged.bin.tmp");
        std::fs::hard_link(&retail, &left_over).unwrap();
        write_atomic(&merged, b"merged").unwrap();
        assert_eq!(std::fs::read(&retail).unwrap(), b"retail");
        assert_eq!(std::fs::read(&left_over).unwrap(), b"retail");
        assert_eq!(std::fs::read(&merged).unwrap(), b"merged");
    }

    #[test]
    fn every_temporary_name_is_new() {
        let target = Path::new("folder/file.bin");
        assert_ne!(new_temp_name(target), new_temp_name(target));
        assert!(is_temp_file(&new_temp_name(target)));
    }

    #[test]
    fn a_dropped_pending_file_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("table.json");
        drop(PendingFile::write(&path, b"x").unwrap());
        assert!(!path.exists());
        assert!(temp_files(dir.path()).is_empty());
    }

    #[test]
    fn linking_a_file_that_is_already_the_link_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let retail = dir.path().join("retail.bin");
        let merged = dir.path().join("merged.bin");
        write_atomic(&retail, b"retail").unwrap();
        link_atomic(&retail, &merged).unwrap();
        link_atomic(&retail, &merged).unwrap();
        assert!(same_file(&retail, &merged));
        assert!(temp_files(dir.path()).is_empty());
    }

    #[test]
    fn writing_the_same_bytes_leaves_the_file_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("table.json");
        write_atomic(&path, b"table").unwrap();
        let before = std::fs::metadata(&path).unwrap();
        write_atomic(&path, b"table").unwrap();
        let after = std::fs::metadata(&path).unwrap();
        assert!(same_file(&path, &path));
        assert_eq!(before.modified().unwrap(), after.modified().unwrap());
        write_atomic(&path, b"other").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"other");
    }

    #[test]
    fn a_taken_temporary_name_is_stepped_over() {
        let mut taken = 3;
        let (temp, ()) = with_new_temp_name(Path::new("folder/file.bin"), |_| {
            if taken == 0 {
                Ok(())
            } else {
                taken -= 1;
                Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
            }
        })
        .unwrap();
        assert!(is_temp_file(&temp));
        let error = with_new_temp_name(Path::new("file.bin"), |_| {
            Err::<(), _>(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        })
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn a_failed_write_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("table.json");
        std::fs::create_dir_all(&path).unwrap();
        assert!(write_atomic(&path, b"x").is_err());
        assert!(temp_files(dir.path()).is_empty());
    }

    #[test]
    fn open_refuses_a_manifest_of_another_build() {
        let dir = tempfile::tempdir().unwrap();
        let layout = ContentLayout::new(dir.path(), BUILD);
        let manifest = Manifest::new("999");
        manifest.write(&layout.manifest_path()).unwrap();
        assert!(matches!(
            ContentLayout::open(dir.path(), BUILD),
            Err(LayoutError::BuildMismatch { .. })
        ));
        Manifest::new(BUILD).write(&layout.manifest_path()).unwrap();
        assert!(ContentLayout::open(dir.path(), BUILD).is_ok());
    }

    #[test]
    fn open_without_a_manifest_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        assert!(ContentLayout::open(dir.path(), BUILD).is_ok());
    }

    /// The content file table of README.md names every registry file, and only those.
    #[test]
    fn the_readme_table_names_every_registry_file() {
        let readme =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../README.md"))
                .expect("README.md");
        let section = readme
            .split(README_SECTION)
            .nth(1)
            .expect("content data section");
        let named: BTreeSet<String> = section
            .lines()
            .take_while(|line| !line.starts_with("### "))
            .filter_map(|line| line.strip_prefix(README_TABLE_CELL))
            .filter_map(|rest| rest.split('`').next())
            .map(str::to_owned)
            .collect();
        let registry: BTreeSet<String> = FILES.iter().map(|file| file.pattern()).collect();
        assert_eq!(named, registry);
    }
}
