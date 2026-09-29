// SPDX-License-Identifier: AGPL-3.0-or-later
//! Merges design files of point kinds into one level's world file (spec section 5.1).

use nw_formats::objectstream::lowercase_crc32;
use nw_world::WorldFile;
use nw_world::records::{
    AiBehaviour, AiProfile, LevelInfo, NpcPoint, PathMode, PatrolPath, Position, SpawnAi,
    VitalsSpawn,
};

use crate::error::{DesignError, Problem};
use crate::file::{Action, DesignFile, DesignSet, Kind};
use crate::schema::{MAX_LEVEL, MIN_LEVEL, position_of};

const AI_PROFILE_FIELD: &str = "ai_profile";
const PATH_FIELD: &str = "path";
const VITALS_FIELD: &str = "vitals";
const LEVEL_FIELD: &str = "level";
const POINTS_FIELD: &str = "points";
/// Number of the first spawn of a group in its keys (`wolves_1`).
const FIRST_GROUP_SPAWN: usize = 1;

/// Applies the files of `set` for level `map` to `world`: retail point changes, the AI
/// profiles (of every level), this level's paths, and its spawn groups. Points are found by their
/// retail key; the key lists keep the retail keys. A file with an error changes nothing.
pub fn apply_world(set: &DesignSet, map: &str, world: &mut WorldFile) -> Vec<DesignError> {
    let mut errors = Vec::new();
    let level = world.level.clone();
    for file in set.files.iter().filter(|file| file.action == Action::Add) {
        match file.kind {
            Kind::AiProfile => {
                world.ai_profiles.insert(file.id.clone(), profile_of(file));
            }
            Kind::Path if file.map.as_deref() == Some(map) => {
                world.paths.insert(file.id.clone(), path_of(file));
            }
            _ => {}
        }
    }
    for file in set
        .files
        .iter()
        .filter(|file| file.map.as_deref() == Some(map))
    {
        let result = match file.kind {
            Kind::SpawnPoint => apply_spawn_point(world, file, &level),
            Kind::NpcPoint => apply_point(
                &mut world.npcs,
                &mut world.npc_keys,
                file,
                &level,
                set_npc_field,
            ),
            Kind::SpawnGroup => add_group(world, file, &level),
            _ => Ok(()),
        };
        if let Err(error) = result {
            errors.push(error);
        }
    }
    errors
}

/// A spawn point change, with its AI entry: a disabled point takes it away, a profile or path
/// field sets it.
fn apply_spawn_point(
    world: &mut WorldFile,
    file: &DesignFile,
    level: &LevelInfo,
) -> Result<(), DesignError> {
    let index = world.vitals_keys.iter().position(|key| *key == file.id);
    apply_point(
        &mut world.vitals,
        &mut world.vitals_keys,
        file,
        level,
        set_spawn_field,
    )?;
    let Some(index) = index else {
        return Ok(());
    };
    if file.action == Action::Disable {
        if index < world.vitals_ai.len() {
            world.vitals_ai.remove(index);
        }
        return Ok(());
    }
    let profile = text_field(file, AI_PROFILE_FIELD);
    let path = text_field(file, PATH_FIELD);
    if profile.is_some() || path.is_some() {
        let ai = spawn_ai_mut(world, index);
        if profile.is_some() {
            ai.profile = profile;
        }
        if path.is_some() {
            ai.path = path;
        }
    }
    Ok(())
}

/// The AI entry of vitals spawn `index`; the list is filled to the spawns first.
fn spawn_ai_mut(world: &mut WorldFile, index: usize) -> &mut SpawnAi {
    if world.vitals_ai.len() < world.vitals.len() {
        world
            .vitals_ai
            .resize(world.vitals.len(), SpawnAi::default());
    }
    &mut world.vitals_ai[index]
}

/// Appends the spawns of a spawn group, keyed `<group>_<n>` from 1.
fn add_group(
    world: &mut WorldFile,
    file: &DesignFile,
    level: &LevelInfo,
) -> Result<(), DesignError> {
    let field_error = |name: &str, problem| DesignError::field(&file.path, name, problem);
    let vitals = file
        .fields
        .get(VITALS_FIELD)
        .ok_or_else(|| field_error(VITALS_FIELD, Problem::MissingField))
        .and_then(|value| id_crc(value).map_err(|problem| field_error(VITALS_FIELD, problem)))?;
    let creature_level = file
        .fields
        .get(LEVEL_FIELD)
        .ok_or_else(|| field_error(LEVEL_FIELD, Problem::MissingField))
        .and_then(|value| {
            creature_level(value).map_err(|problem| field_error(LEVEL_FIELD, problem))
        })?;
    let points = file
        .fields
        .get(POINTS_FIELD)
        .and_then(toml::Value::as_array)
        .ok_or_else(|| field_error(POINTS_FIELD, Problem::MissingField))?
        .iter()
        .map(|point| {
            position_in(point, level).map_err(|problem| field_error(POINTS_FIELD, problem))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let ai = SpawnAi {
        profile: text_field(file, AI_PROFILE_FIELD),
        path: text_field(file, PATH_FIELD),
    };
    if world.vitals_ai.len() < world.vitals.len() {
        world
            .vitals_ai
            .resize(world.vitals.len(), SpawnAi::default());
    }
    for (number, position) in (FIRST_GROUP_SPAWN..).zip(points) {
        world.vitals.push(VitalsSpawn {
            vitals_id: vitals,
            level: creature_level,
            position,
            ..VitalsSpawn::default()
        });
        world.vitals_keys.push(format!("{}_{number}", file.id));
        world.vitals_ai.push(ai.clone());
    }
    Ok(())
}

fn text_field(file: &DesignFile, name: &str) -> Option<String> {
    file.fields
        .get(name)
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
}

fn number_field<T>(
    file: &DesignFile,
    name: &str,
    of: impl Fn(&toml::Value) -> Option<T>,
) -> Option<T> {
    file.fields.get(name).and_then(of)
}

#[allow(clippy::cast_possible_truncation)]
fn as_f32(value: &toml::Value) -> Option<f32> {
    match value {
        toml::Value::Float(number) => Some(*number as f32),
        toml::Value::Integer(number) => Some(*number as f32),
        _ => None,
    }
}

fn as_u64(value: &toml::Value) -> Option<u64> {
    value
        .as_integer()
        .and_then(|number| u64::try_from(number).ok())
}

/// The AI profile of an `ai_profile` file; a missing number stays `None`.
fn profile_of(file: &DesignFile) -> AiProfile {
    let behaviour = match text_field(file, "behaviour").as_deref() {
        Some("guard") => AiBehaviour::Guard,
        Some("patrol") => AiBehaviour::Patrol,
        Some("idle") => AiBehaviour::Idle,
        _ => AiBehaviour::Wander,
    };
    AiProfile {
        behaviour,
        wake_radius_m: number_field(file, "wake_radius_m", as_f32),
        sight_m: number_field(file, "sight_m", as_f32),
        wander_radius_m: number_field(file, "wander_radius_m", as_f32),
        wander_pause_ms: number_field(file, "wander_pause_ms", as_u64),
        leash_radius_m: number_field(file, "leash_radius_m", as_f32),
        attack_reach_m: number_field(file, "attack_reach_m", as_f32),
        attack_interval_ms: number_field(file, "attack_interval_ms", as_u64),
        speed_mps: number_field(file, "speed_mps", as_f32),
        respawn_ms: number_field(file, "respawn_ms", as_u64),
    }
}

/// The patrol path of a `path` file.
fn path_of(file: &DesignFile) -> PatrolPath {
    let mode = match text_field(file, "mode").as_deref() {
        Some("back_and_forth") => PathMode::BackAndForth,
        Some("once") => PathMode::Once,
        _ => PathMode::Loop,
    };
    PatrolPath {
        points: file
            .fields
            .get(POINTS_FIELD)
            .and_then(toml::Value::as_array)
            .map(|points| points.iter().filter_map(position_of).collect())
            .unwrap_or_default(),
        waits_ms: file
            .fields
            .get("waits_ms")
            .and_then(toml::Value::as_array)
            .map(|waits| waits.iter().filter_map(as_u64).collect())
            .unwrap_or_default(),
        mode,
        tasks: Vec::new(),
    }
}

/// Sets one field of a point; the field name and the value come from a design file.
type SetField<P> = fn(&mut P, &str, &toml::Value, &LevelInfo) -> Result<(), Problem>;

fn apply_point<P: Clone>(
    points: &mut Vec<P>,
    keys: &mut Vec<String>,
    file: &DesignFile,
    level: &LevelInfo,
    set_field: SetField<P>,
) -> Result<(), DesignError> {
    let index = keys
        .iter()
        .position(|key| *key == file.id)
        .ok_or_else(|| DesignError::file(&file.path, Problem::UnknownId))?;
    match file.action {
        Action::Disable => {
            points.remove(index);
            keys.remove(index);
        }
        Action::Override => {
            let mut point = points[index].clone();
            for (name, value) in &file.fields {
                set_field(&mut point, name, value, level)
                    .map_err(|problem| DesignError::field(&file.path, name, problem))?;
            }
            points[index] = point;
        }
        Action::Add => return Err(DesignError::file(&file.path, Problem::ActionNotAllowed)),
    }
    Ok(())
}

fn set_spawn_field(
    spawn: &mut VitalsSpawn,
    name: &str,
    value: &toml::Value,
    level: &LevelInfo,
) -> Result<(), Problem> {
    match name {
        // The AI entry is set by `apply_spawn_point`.
        AI_PROFILE_FIELD | PATH_FIELD => {}
        "vitals_id" => spawn.vitals_id = id_crc(value)?,
        "category_id" => spawn.category_id = id_crc(value)?,
        "level" => spawn.level = creature_level(value)?,
        "position" => spawn.position = position_in(value, level)?,
        _ => return Err(Problem::UnknownField),
    }
    Ok(())
}

fn set_npc_field(
    npc: &mut NpcPoint,
    name: &str,
    value: &toml::Value,
    level: &LevelInfo,
) -> Result<(), Problem> {
    match name {
        "npc_id" => npc.npc_id = id_crc(value)?,
        "faction_type" => {
            npc.faction_type = value
                .as_integer()
                .and_then(|number| u8::try_from(number).ok())
                .ok_or_else(|| Problem::OutOfRange(format!("0 to {}", u8::MAX)))?;
        }
        "position" => npc.position = position_in(value, level)?,
        _ => return Err(Problem::UnknownField),
    }
    Ok(())
}

/// The lower-case crc32 of a text id, as world files store it.
fn id_crc(value: &toml::Value) -> Result<u32, Problem> {
    value
        .as_str()
        .map(lowercase_crc32)
        .ok_or_else(|| Problem::WrongType("a text id".to_owned()))
}

fn creature_level(value: &toml::Value) -> Result<u32, Problem> {
    value
        .as_integer()
        .filter(|number| (MIN_LEVEL..=MAX_LEVEL).contains(number))
        .and_then(|number| u32::try_from(number).ok())
        .ok_or_else(|| Problem::OutOfRange(format!("{MIN_LEVEL} to {MAX_LEVEL}")))
}

/// A position inside the level: x from 0 to its width, y from 0 to its height.
fn position_in(value: &toml::Value, level: &LevelInfo) -> Result<Position, Problem> {
    let position =
        position_of(value).ok_or_else(|| Problem::WrongType("[x, y, z] in metres".to_owned()))?;
    #[allow(clippy::cast_precision_loss)]
    let (width, height) = (
        (level.grid_width * level.region_size) as f32,
        (level.grid_height * level.region_size) as f32,
    );
    let [x, y, _] = position;
    if (0.0..=width).contains(&x) && (0.0..=height).contains(&y) {
        Ok(position)
    } else {
        Err(Problem::OutOfRange(format!(
            "x 0 to {width}, y 0 to {height}"
        )))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;

    use nw_formats::objectstream::lowercase_crc32;
    use nw_world::records::{AiBehaviour, LevelInfo, NpcPoint, PathMode, SpawnAi, VitalsSpawn};

    use super::*;
    use crate::error::Problem;
    use crate::file::{Action, DesignFile, Kind};

    pub(crate) const MAP: &str = "lv";
    const REGION_SIZE: u32 = 256;
    const GRID: u32 = 4;

    pub(crate) fn world() -> WorldFile {
        let mut world = WorldFile {
            level: LevelInfo {
                name: MAP.to_owned(),
                region_size: REGION_SIZE,
                grid_width: GRID,
                grid_height: GRID,
                ..LevelInfo::default()
            },
            npcs: vec![NpcPoint {
                npc_id: 9,
                position: [10.0, 10.0, 0.0],
                ..NpcPoint::default()
            }],
            vitals: vec![
                VitalsSpawn {
                    vitals_id: 1,
                    level: 5,
                    position: [1.0, 2.0, 3.0],
                    ..VitalsSpawn::default()
                },
                VitalsSpawn {
                    vitals_id: 2,
                    level: 6,
                    position: [4.0, 5.0, 6.0],
                    ..VitalsSpawn::default()
                },
            ],
            ..WorldFile::default()
        };
        world.fill_point_keys();
        world
    }

    pub(crate) fn point(
        kind: Kind,
        action: Action,
        id: &str,
        map: &str,
        fields: &str,
    ) -> DesignFile {
        let folder = kind.folder();
        DesignFile {
            path: PathBuf::from(format!("{folder}/{map}/{id}.toml")),
            kind,
            action,
            id: id.to_owned(),
            base: None,
            map: Some(map.to_owned()),
            fields: fields.parse().unwrap(),
            version: String::new(),
        }
    }

    fn apply(file: DesignFile) -> (WorldFile, Vec<DesignError>) {
        let mut world = world();
        let errors = apply_world(&DesignSet { files: vec![file] }, MAP, &mut world);
        (world, errors)
    }

    fn own(kind: Kind, id: &str, map: Option<&str>, fields: &str) -> DesignFile {
        DesignFile {
            path: PathBuf::from(format!("{}/{id}.toml", kind.folder())),
            kind,
            action: Action::Add,
            id: id.to_owned(),
            base: None,
            map: map.map(str::to_owned),
            fields: fields.parse().unwrap(),
            version: String::new(),
        }
    }

    #[test]
    fn profiles_paths_and_groups_reach_the_world_file() {
        let mut world = world();
        let key = world.vitals_keys[0].clone();
        let files = vec![
            point(
                Kind::SpawnPoint,
                Action::Override,
                &key,
                MAP,
                "ai_profile = \"guards\"",
            ),
            own(
                Kind::AiProfile,
                "guards",
                None,
                "behaviour = \"guard\"\nsight_m = 9.5\nrespawn_ms = 1000",
            ),
            own(
                Kind::Path,
                "gate",
                Some(MAP),
                "points = [[1, 2, 3], [4.5, 5, 6]]\nwaits_ms = [100]\nmode = \"once\"",
            ),
            own(
                Kind::Path,
                "elsewhere",
                Some("other"),
                "points = [[1, 2, 3]]",
            ),
            own(
                Kind::SpawnGroup,
                "wolves",
                Some(MAP),
                "vitals = \"Wolf\"\nlevel = 12\npoints = [[10, 20, 5], [11, 21, 5]]\npath = \"gate\"",
            ),
        ];
        let errors = apply_world(&DesignSet { files }, MAP, &mut world);
        assert_eq!(errors, []);
        let profile = &world.ai_profiles["guards"];
        assert_eq!(profile.behaviour, AiBehaviour::Guard);
        assert_eq!(
            (profile.sight_m, profile.respawn_ms, profile.speed_mps),
            (Some(9.5), Some(1000), None)
        );
        assert_eq!(world.paths.len(), 1, "only this level's paths");
        assert_eq!(world.paths["gate"].points[1], [4.5, 5.0, 6.0]);
        assert_eq!(world.paths["gate"].mode, PathMode::Once);
        assert_eq!(world.vitals.len(), 4, "two retail and two group spawns");
        assert_eq!(world.vitals[2].vitals_id, lowercase_crc32("Wolf"));
        assert_eq!(world.vitals[3].level, 12);
        assert_eq!(&world.vitals_keys[2..], ["wolves_1", "wolves_2"]);
        assert_eq!(world.vitals_ai.len(), 4);
        assert_eq!(world.vitals_ai[0].profile.as_deref(), Some("guards"));
        assert_eq!(world.vitals_ai[1], SpawnAi::default());
        assert_eq!(world.vitals_ai[3].path.as_deref(), Some("gate"));
    }

    #[test]
    fn a_disabled_spawn_takes_its_ai_entry_with_it() {
        let mut world = world();
        let first = world.vitals_keys[0].clone();
        let second = world.vitals_keys[1].clone();
        let files = vec![
            point(Kind::SpawnPoint, Action::Disable, &first, MAP, ""),
            point(
                Kind::SpawnPoint,
                Action::Override,
                &second,
                MAP,
                "path = \"gate\"",
            ),
            own(Kind::Path, "gate", Some(MAP), "points = [[1, 2, 3]]"),
        ];
        assert_eq!(apply_world(&DesignSet { files }, MAP, &mut world), []);
        assert_eq!(world.vitals.len(), 1);
        assert_eq!(world.vitals_ai.len(), 1);
        assert_eq!(world.vitals_ai[0].path.as_deref(), Some("gate"));
    }

    #[test]
    fn override_sets_the_spawn_level() {
        let key = world().vitals_keys[0].clone();
        let (world, errors) = apply(point(
            Kind::SpawnPoint,
            Action::Override,
            &key,
            MAP,
            "level = 30",
        ));
        assert_eq!(errors, []);
        assert_eq!(world.vitals[0].level, 30);
        assert_eq!(world.vitals_keys[0], key);
    }

    #[test]
    fn a_text_id_is_stored_as_its_crc() {
        let key = world().vitals_keys[1].clone();
        let (world, errors) = apply(point(
            Kind::SpawnPoint,
            Action::Override,
            &key,
            MAP,
            "vitals_id = \"Wolf_Alpha\"\nposition = [7, 8.5, 9]",
        ));
        assert_eq!(errors, []);
        assert_eq!(world.vitals[1].vitals_id, lowercase_crc32("Wolf_Alpha"));
        assert_eq!(world.vitals[1].position, [7.0, 8.5, 9.0]);
    }

    #[test]
    fn disable_removes_the_point_and_its_key() {
        let key = world().vitals_keys[0].clone();
        let (world, errors) = apply(point(Kind::SpawnPoint, Action::Disable, &key, MAP, ""));
        assert_eq!(errors, []);
        assert_eq!(world.vitals.len(), 1);
        assert_eq!(world.vitals_keys.len(), 1);
        assert_eq!(world.vitals[0].vitals_id, 2);
    }

    #[test]
    fn an_npc_point_moves() {
        let key = world().npc_keys[0].clone();
        let (world, errors) = apply(point(
            Kind::NpcPoint,
            Action::Override,
            &key,
            MAP,
            "position = [20.0, 30.0, 1.0]\nfaction_type = 3",
        ));
        assert_eq!(errors, []);
        assert_eq!(world.npcs[0].position, [20.0, 30.0, 1.0]);
        assert_eq!(world.npcs[0].faction_type, 3);
    }

    #[test]
    fn other_map_is_ignored() {
        let key = world().vitals_keys[0].clone();
        let (changed, errors) = apply(point(
            Kind::SpawnPoint,
            Action::Override,
            &key,
            "other",
            "level = 30",
        ));
        assert_eq!(errors, []);
        assert_eq!(changed, world());
    }

    #[test]
    fn unknown_key_is_an_error() {
        let (_, errors) = apply(point(
            Kind::SpawnPoint,
            Action::Override,
            "1_0.0_0.0_0.0",
            MAP,
            "level = 30",
        ));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].problem, Problem::UnknownId);
    }

    #[test]
    fn position_off_the_level_is_out_of_range() {
        let key = world().vitals_keys[0].clone();
        let (world, errors) = apply(point(
            Kind::SpawnPoint,
            Action::Override,
            &key,
            MAP,
            "position = [2000.0, 5.0, 1.0]",
        ));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].field.as_deref(), Some("position"));
        assert!(matches!(errors[0].problem, Problem::OutOfRange(_)));
        assert_eq!(world.vitals[0].position, [1.0, 2.0, 3.0]);
    }
}
