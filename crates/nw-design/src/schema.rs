// SPDX-License-Identifier: AGPL-3.0-or-later
//! The schema level check (spec section 5.3): everything that needs no client data.

use std::collections::HashMap;
use std::path::Path;

use crate::error::{DesignError, Problem};
use crate::file::{Action, DesignFile, DesignSet, Kind, Row, RowVisitor, struct_fields};

/// Lowest creature level of a spawn point.
pub(crate) const MIN_LEVEL: i64 = 1;
/// Highest creature level of a spawn point.
pub(crate) const MAX_LEVEL: i64 = 65;
/// Coordinates of a position.
pub(crate) const POSITION_AXES: usize = 3;

/// The value a point field holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PointValue {
    /// A datasheet id as text; the world file stores its lower-case crc32.
    Id,
    /// A creature level, [`MIN_LEVEL`] to [`MAX_LEVEL`].
    Level,
    /// A number 0 to 255.
    Byte,
    /// `[x, y, z]` in metres.
    Position,
}

/// The fields a point kind may change.
pub(crate) fn point_fields(kind: Kind) -> &'static [(&'static str, PointValue)] {
    match kind {
        Kind::SpawnPoint => &[
            ("category_id", PointValue::Id),
            ("level", PointValue::Level),
            ("position", PointValue::Position),
            ("vitals_id", PointValue::Id),
        ],
        Kind::NpcPoint => &[
            ("faction_type", PointValue::Byte),
            ("npc_id", PointValue::Id),
            ("position", PointValue::Position),
        ],
        _ => &[],
    }
}

/// Checks names, folders, actions, field names, point values, full `add` rows and duplicate ids.
pub fn check_schema(set: &DesignSet) -> Vec<DesignError> {
    let mut errors = Vec::new();
    let mut seen: HashMap<(Kind, Option<&str>, &str), &Path> = HashMap::new();
    for file in &set.files {
        check_file(file, &mut errors);
        match seen.get(&(file.kind, file.map.as_deref(), file.id.as_str())) {
            Some(other) => errors.push(DesignError::file(
                &file.path,
                Problem::DuplicateId {
                    other: other.to_path_buf(),
                },
            )),
            None => {
                seen.insert((file.kind, file.map.as_deref(), &file.id), &file.path);
            }
        }
    }
    errors
}

fn check_file(file: &DesignFile, errors: &mut Vec<DesignError>) {
    let path = &file.path;
    if path.file_stem().and_then(|stem| stem.to_str()) != Some(file.id.as_str()) {
        errors.push(DesignError::file(path, Problem::NameMismatch));
    }
    let expected = match (file.kind.is_point(), &file.map) {
        (false, _) => Some(file.kind.folder().to_owned()),
        (true, Some(map)) => Some(format!("{}/{map}", file.kind.folder())),
        (true, None) => {
            errors.push(DesignError::file(path, Problem::MissingMap));
            None
        }
    };
    if let Some(expected) = expected
        && path.parent() != Some(Path::new(&expected))
    {
        errors.push(DesignError::file(path, Problem::WrongFolder { expected }));
    }
    if !file.kind.allows(file.action) {
        errors.push(DesignError::file(path, Problem::ActionNotAllowed));
    }
    if file.base.is_some() && file.action != Action::Add {
        errors.push(DesignError::file(path, Problem::BaseNotAllowed));
    }
    if file.action == Action::Disable {
        if !file.fields.is_empty() {
            errors.push(DesignError::file(path, Problem::FieldsNotAllowed));
        }
        return;
    }
    match file.kind.visit_row(RowCheck { file }) {
        Some(row_errors) => errors.extend(row_errors),
        None => check_point_fields(file, errors),
    }
}

/// Field names of a row kind, and the full row of an `add` without `base`.
struct RowCheck<'a> {
    file: &'a DesignFile,
}

impl RowVisitor for RowCheck<'_> {
    type Output = Vec<DesignError>;

    fn visit<T: Row>(self) -> Vec<DesignError> {
        let file = self.file;
        let known = struct_fields::<T>();
        let mut errors = Vec::new();
        for name in file.fields.keys() {
            if name == T::KEY_FIELD {
                errors.push(DesignError::field(&file.path, name, Problem::IdField));
            } else if !known.contains(&name.as_str()) {
                errors.push(DesignError::field(&file.path, name, Problem::UnknownField));
            }
        }
        if errors.is_empty() && file.action == Action::Add && file.base.is_none() {
            let mut row = file.fields.clone();
            row.insert(
                T::KEY_FIELD.to_owned(),
                toml::Value::String(file.id.clone()),
            );
            if let Err(error) = row_from_toml::<T>(row) {
                errors.push(DesignError::file(&file.path, Problem::IncompleteRow(error)));
            }
        }
        errors
    }
}

/// Reads a whole row from TOML values, through JSON so that numbered columns (`"1" = ...`)
/// read as `BTreeMap<u32, _>`.
pub(crate) fn row_from_toml<T: Row>(row: toml::Table) -> Result<T, String> {
    let json = serde_json::to_value(row).map_err(|error| error.to_string())?;
    serde_json::from_value(json).map_err(|error| error.to_string())
}

fn check_point_fields(file: &DesignFile, errors: &mut Vec<DesignError>) {
    let allowed = point_fields(file.kind);
    for (name, value) in &file.fields {
        let Some((_, expected)) = allowed.iter().find(|(known, _)| known == name) else {
            errors.push(DesignError::field(&file.path, name, Problem::UnknownField));
            continue;
        };
        if let Err(problem) = check_point_value(*expected, value) {
            errors.push(DesignError::field(&file.path, name, problem));
        }
    }
}

fn check_point_value(expected: PointValue, value: &toml::Value) -> Result<(), Problem> {
    match expected {
        PointValue::Id => value
            .as_str()
            .map(drop)
            .ok_or_else(|| Problem::WrongType("a text id".to_owned())),
        PointValue::Level => in_range(value, MIN_LEVEL, MAX_LEVEL),
        PointValue::Byte => in_range(value, 0, i64::from(u8::MAX)),
        PointValue::Position => position_of(value)
            .map(drop)
            .ok_or_else(|| Problem::WrongType("[x, y, z] in metres".to_owned())),
    }
}

fn in_range(value: &toml::Value, min: i64, max: i64) -> Result<(), Problem> {
    let number = value
        .as_integer()
        .ok_or_else(|| Problem::WrongType("a whole number".to_owned()))?;
    if (min..=max).contains(&number) {
        Ok(())
    } else {
        Err(Problem::OutOfRange(format!("{min} to {max}")))
    }
}

/// `[x, y, z]` of finite numbers (whole numbers allowed).
pub(crate) fn position_of(value: &toml::Value) -> Option<[f32; POSITION_AXES]> {
    let items = value.as_array()?;
    if items.len() != POSITION_AXES {
        return None;
    }
    let mut position = [0.0; POSITION_AXES];
    for (slot, item) in position.iter_mut().zip(items) {
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        let number = match item {
            toml::Value::Float(number) => *number as f32,
            toml::Value::Integer(number) => *number as f32,
            _ => return None,
        };
        if !number.is_finite() {
            return None;
        }
        *slot = number;
    }
    Some(position)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::error::Problem;
    use crate::load::load_dir;
    use crate::load::tests::{LOOT_OVERRIDE, write};

    const MAP: &str = "newworld_vitaeeterna";

    fn problems(files: &[(&str, &str)]) -> Vec<DesignError> {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in files {
            write(dir.path(), path, text);
        }
        let (set, errors) = load_dir(dir.path());
        assert_eq!(errors, [], "load errors");
        check_schema(&set)
    }

    fn spawn(fields: &str) -> String {
        format!(
            "schema = 1\nkind = \"spawn_point\"\naction = \"override\"\nid = \"12_1.0_2.0_3.0\"\nmap = \"{MAP}\"\n{fields}"
        )
    }

    fn spawn_path() -> String {
        format!("spawns/{MAP}/12_1.0_2.0_3.0.toml")
    }

    #[test]
    fn a_valid_override_passes() {
        assert_eq!(problems(&[("loot/tables/LT_A.toml", LOOT_OVERRIDE)]), []);
    }

    #[test]
    fn a_valid_spawn_override_passes() {
        let text = spawn("level = 30\nposition = [1.0, 2, 3.5]\nvitals_id = \"Wolf_Alpha\"\n");
        assert_eq!(problems(&[(&spawn_path(), &text)]), []);
    }

    #[test]
    fn two_files_with_one_id_are_an_error() {
        let errors = problems(&[
            ("loot/tables/LT_A.toml", LOOT_OVERRIDE),
            ("loot/tables/sub/LT_A.toml", LOOT_OVERRIDE),
        ]);
        let duplicates: Vec<_> = errors
            .iter()
            .filter(|error| matches!(error.problem, Problem::DuplicateId { .. }))
            .collect();
        assert_eq!(duplicates.len(), 1);
        assert_eq!(duplicates[0].path, Path::new("loot/tables/sub/LT_A.toml"));
        assert_eq!(
            duplicates[0].problem,
            Problem::DuplicateId {
                other: "loot/tables/LT_A.toml".into()
            }
        );
    }

    #[test]
    fn the_file_name_must_be_the_id() {
        let errors = problems(&[(
            "loot/tables/LT_A.toml",
            &LOOT_OVERRIDE.replace("\"LT_A\"", "\"LT_B\""),
        )]);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].problem, Problem::NameMismatch);
    }

    #[test]
    fn the_kind_must_match_the_folder() {
        let errors = problems(&[("items/LT_A.toml", LOOT_OVERRIDE)]);
        assert_eq!(
            errors[0].problem,
            Problem::WrongFolder {
                expected: "loot/tables".to_owned()
            }
        );
    }

    #[test]
    fn unknown_field_is_named() {
        let errors = problems(&[(
            "loot/tables/LT_A.toml",
            &LOOT_OVERRIDE.replace("max_roll", "max_rol"),
        )]);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].field.as_deref(), Some("max_rol"));
        assert_eq!(errors[0].problem, Problem::UnknownField);
    }

    #[test]
    fn the_id_field_is_not_a_field() {
        let errors = problems(&[(
            "loot/tables/LT_A.toml",
            &format!("{LOOT_OVERRIDE}loot_table_id = \"X\"\n"),
        )]);
        assert_eq!(errors[0].problem, Problem::IdField);
    }

    #[test]
    fn a_point_needs_its_map_folder() {
        let text = spawn("level = 30\n").replace(MAP, "other");
        let errors = problems(&[(&spawn_path(), &text)]);
        assert_eq!(
            errors[0].problem,
            Problem::WrongFolder {
                expected: "spawns/other".to_owned()
            }
        );
    }

    #[test]
    fn a_point_without_map_is_an_error() {
        let text = spawn("level = 30\n").replace(&format!("map = \"{MAP}\"\n"), "");
        let errors = problems(&[(&spawn_path(), &text)]);
        assert_eq!(errors[0].problem, Problem::MissingMap);
    }

    #[test]
    fn points_cannot_be_added() {
        let text = spawn("level = 30\n").replace("override", "add");
        let errors = problems(&[(&spawn_path(), &text)]);
        assert_eq!(errors[0].problem, Problem::ActionNotAllowed);
    }

    #[test]
    fn spawn_level_out_of_range() {
        let errors = problems(&[(&spawn_path(), &spawn("level = 66\n"))]);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].field.as_deref(), Some("level"));
        assert!(matches!(errors[0].problem, Problem::OutOfRange(_)));
    }

    #[test]
    fn spawn_values_need_their_types() {
        let errors = problems(&[(
            &spawn_path(),
            &spawn("position = [1.0, 2.0]\nvitals_id = 5\nlevel = \"high\"\n"),
        )]);
        let fields: Vec<_> = errors.iter().map(|error| error.field.as_deref()).collect();
        assert_eq!(fields, [Some("level"), Some("position"), Some("vitals_id")]);
        assert!(
            errors
                .iter()
                .all(|error| matches!(error.problem, Problem::WrongType(_)))
        );
    }

    #[test]
    fn base_only_on_add() {
        let errors = problems(&[(
            "loot/tables/LT_A.toml",
            &format!("{LOOT_OVERRIDE}base = \"LT_B\"\n"),
        )]);
        assert_eq!(errors[0].problem, Problem::BaseNotAllowed);
    }

    #[test]
    fn disable_takes_no_fields() {
        let errors = problems(&[(
            "loot/tables/LT_A.toml",
            &LOOT_OVERRIDE.replace("override", "disable"),
        )]);
        assert_eq!(errors[0].problem, Problem::FieldsNotAllowed);
    }

    #[test]
    fn add_without_base_needs_a_full_row() {
        let errors = problems(&[(
            "loot/tables/LT_A.toml",
            &LOOT_OVERRIDE.replace("override", "add"),
        )]);
        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0].problem, Problem::IncompleteRow(_)));
    }

    #[test]
    fn add_with_base_passes() {
        let text = format!(
            "{}base = \"LT_B\"\n",
            LOOT_OVERRIDE.replace("override", "add")
        );
        assert_eq!(problems(&[("loot/tables/LT_A.toml", &text)]), []);
    }
}
