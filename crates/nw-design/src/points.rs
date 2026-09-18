// SPDX-License-Identifier: AGPL-3.0-or-later
//! Merges design files of point kinds into one level's world file (spec section 5.1).

use nw_formats::objectstream::lowercase_crc32;
use nw_world::WorldFile;
use nw_world::records::{LevelInfo, NpcPoint, Position, VitalsSpawn};

use crate::error::{DesignError, Problem};
use crate::file::{Action, DesignFile, DesignSet, Kind};
use crate::schema::{MAX_LEVEL, MIN_LEVEL, position_of};

/// Applies the point files of `set` whose `map` is `map` to `world`. Points are found by their
/// retail key; the key lists keep the retail keys. A file with an error changes nothing.
pub fn apply_world(set: &DesignSet, map: &str, world: &mut WorldFile) -> Vec<DesignError> {
    let mut errors = Vec::new();
    let level = world.level.clone();
    for file in set
        .files
        .iter()
        .filter(|file| file.kind.is_point() && file.map.as_deref() == Some(map))
    {
        let result = match file.kind {
            Kind::SpawnPoint => apply_point(
                &mut world.vitals,
                &mut world.vitals_keys,
                file,
                &level,
                set_spawn_field,
            ),
            Kind::NpcPoint => apply_point(
                &mut world.npcs,
                &mut world.npc_keys,
                file,
                &level,
                set_npc_field,
            ),
            _ => Ok(()),
        };
        if let Err(error) = result {
            errors.push(error);
        }
    }
    errors
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
    use nw_world::records::{LevelInfo, NpcPoint, VitalsSpawn};

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
        }
    }

    fn apply(file: DesignFile) -> (WorldFile, Vec<DesignError>) {
        let mut world = world();
        let errors = apply_world(&DesignSet { files: vec![file] }, MAP, &mut world);
        (world, errors)
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
