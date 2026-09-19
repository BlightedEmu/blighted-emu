// SPDX-License-Identifier: AGPL-3.0-or-later
//! The schema level check (spec section 5.3): everything that needs no client data.

use std::collections::HashMap;
use std::path::Path;

use crate::error::{DesignError, Problem};
use crate::file::{Action, DesignFile, DesignSet, Kind, Row, RowVisitor, struct_fields};
use crate::scripts::{LINKS, NODES, WAVES, check_event_references, check_script_or_event};

/// Lowest creature level of a spawn point.
pub(crate) const MIN_LEVEL: i64 = 1;
/// Highest creature level of a spawn point.
pub(crate) const MAX_LEVEL: i64 = 65;
/// Coordinates of a position.
pub(crate) const POSITION_AXES: usize = 3;

/// The value a field of a point or own kind holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldValue {
    /// A datasheet id as text; the world file stores its lower-case crc32.
    Id,
    /// A creature level, [`MIN_LEVEL`] to [`MAX_LEVEL`].
    Level,
    /// A number 0 to 255.
    Byte,
    /// `[x, y, z]` in metres.
    Position,
    /// One or more `[x, y, z]`.
    Positions,
    /// The id of an AI profile file.
    ProfileRef,
    /// The id of a path file of the same level.
    PathRef,
    /// `wander`, `guard`, `patrol` or `idle`.
    Behaviour,
    /// `loop`, `back_and_forth` or `once`.
    PathMode,
    /// A number of metres (or metres per second) above 0.
    Metres,
    /// A whole number of milliseconds, 0 or more.
    Millis,
    /// A list of whole numbers of milliseconds.
    MillisList,
    /// `true` or `false`.
    Bool,
    /// The `[[nodes]]` of a script (read by `scripts::script_of`).
    ScriptNodes,
    /// The `[[links]]` of a script.
    ScriptLinks,
    /// The `[[waves]]` of an event (read by `scripts::event_of`).
    EventWaves,
}

/// One field of a point or own kind: name, value, and whether every `add` needs it.
pub(crate) type FieldSpec = (&'static str, FieldValue, bool);

/// Behaviour names in files.
pub(crate) const BEHAVIOURS: [&str; 4] = ["wander", "guard", "patrol", "idle"];
/// Path mode names in files.
pub(crate) const PATH_MODES: [&str; 3] = ["loop", "back_and_forth", "once"];

/// The fields of a point or own kind.
pub(crate) fn record_fields(kind: Kind) -> &'static [FieldSpec] {
    match kind {
        Kind::SpawnPoint => &[
            ("ai_profile", FieldValue::ProfileRef, false),
            ("category_id", FieldValue::Id, false),
            ("level", FieldValue::Level, false),
            ("path", FieldValue::PathRef, false),
            ("position", FieldValue::Position, false),
            ("vitals_id", FieldValue::Id, false),
        ],
        Kind::NpcPoint => &[
            ("faction_type", FieldValue::Byte, false),
            ("npc_id", FieldValue::Id, false),
            ("position", FieldValue::Position, false),
        ],
        Kind::AiProfile => &[
            ("attack_interval_ms", FieldValue::Millis, false),
            ("attack_reach_m", FieldValue::Metres, false),
            ("behaviour", FieldValue::Behaviour, false),
            ("leash_radius_m", FieldValue::Metres, false),
            ("respawn_ms", FieldValue::Millis, false),
            ("sight_m", FieldValue::Metres, false),
            ("speed_mps", FieldValue::Metres, false),
            ("wake_radius_m", FieldValue::Metres, false),
            ("wander_pause_ms", FieldValue::Millis, false),
            ("wander_radius_m", FieldValue::Metres, false),
        ],
        Kind::Path => &[
            ("mode", FieldValue::PathMode, false),
            ("points", FieldValue::Positions, true),
            ("waits_ms", FieldValue::MillisList, false),
        ],
        Kind::Script => &[
            (LINKS, FieldValue::ScriptLinks, false),
            (NODES, FieldValue::ScriptNodes, true),
        ],
        Kind::Event => &[
            ("end_after_ms", FieldValue::Millis, false),
            ("end_when_cleared", FieldValue::Bool, false),
            (WAVES, FieldValue::EventWaves, true),
        ],
        Kind::SpawnGroup => &[
            ("ai_profile", FieldValue::ProfileRef, false),
            ("level", FieldValue::Level, true),
            ("path", FieldValue::PathRef, false),
            ("points", FieldValue::Positions, true),
            ("vitals", FieldValue::Id, true),
        ],
        _ => &[],
    }
}

/// Checks names, folders, actions, field names, point values, full `add` rows and duplicate ids.
pub fn check_schema(set: &DesignSet) -> Vec<DesignError> {
    let mut errors = Vec::new();
    // Ids are compared without case, as the server finds records (see `rows::same_id`).
    let mut seen: HashMap<(Kind, Option<&str>, String), &Path> = HashMap::new();
    for file in &set.files {
        check_file(file, &mut errors);
        let key = (file.kind, file.map.as_deref(), file.id.to_ascii_lowercase());
        match seen.get(&key) {
            Some(other) => errors.push(DesignError::file(
                &file.path,
                Problem::DuplicateId {
                    other: other.to_path_buf(),
                },
            )),
            None => {
                seen.insert(key, &file.path);
            }
        }
    }
    check_references(set, &mut errors);
    check_event_references(set, &mut errors);
    errors
}

/// Every `ai_profile` names a profile file, and every `path` a path file of the same level.
fn check_references(set: &DesignSet, errors: &mut Vec<DesignError>) {
    let made = |kind: Kind, map: Option<&str>, id: &str| {
        set.files.iter().any(|file| {
            file.kind == kind
                && file.action == Action::Add
                && file.id.eq_ignore_ascii_case(id)
                && (map.is_none() || file.map.as_deref() == map)
        })
    };
    for file in &set.files {
        for (field, kind, map) in [
            ("ai_profile", Kind::AiProfile, None),
            ("path", Kind::Path, file.map.as_deref()),
        ] {
            if !record_fields(file.kind)
                .iter()
                .any(|(name, _, _)| *name == field)
            {
                continue;
            }
            let Some(id) = file.fields.get(field).and_then(toml::Value::as_str) else {
                continue;
            };
            if !made(kind, map, id) {
                errors.push(DesignError::field(
                    &file.path,
                    field,
                    Problem::UnknownReference(format!("{} {id}", kind.name())),
                ));
            }
        }
    }
}

fn check_file(file: &DesignFile, errors: &mut Vec<DesignError>) {
    let path = &file.path;
    if path.file_stem().and_then(|stem| stem.to_str()) != Some(file.id.as_str()) {
        errors.push(DesignError::file(path, Problem::NameMismatch));
    }
    let expected = match (file.kind.needs_map(), &file.map) {
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
        None => {
            check_record_fields(file, errors);
            check_script_or_event(file, errors);
        }
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

fn check_record_fields(file: &DesignFile, errors: &mut Vec<DesignError>) {
    let fields = record_fields(file.kind);
    for (name, value) in &file.fields {
        let Some((_, expected, _)) = fields.iter().find(|(known, _, _)| known == name) else {
            errors.push(DesignError::field(&file.path, name, Problem::UnknownField));
            continue;
        };
        if let Err(problem) = check_field_value(*expected, value) {
            errors.push(DesignError::field(&file.path, name, problem));
        }
    }
    if file.action == Action::Add {
        for (name, _, required) in fields {
            if *required && !file.fields.contains_key(*name) {
                errors.push(DesignError::field(&file.path, name, Problem::MissingField));
            }
        }
    }
}

fn wrong(what: &str) -> Problem {
    Problem::WrongType(what.to_owned())
}

fn check_field_value(expected: FieldValue, value: &toml::Value) -> Result<(), Problem> {
    let text = |names: &[&str]| -> Result<(), Problem> {
        let found = value.as_str().ok_or_else(|| wrong(&names.join(", ")))?;
        if names.contains(&found) {
            Ok(())
        } else {
            Err(Problem::OutOfRange(names.join(", ")))
        }
    };
    match expected {
        FieldValue::Id | FieldValue::ProfileRef | FieldValue::PathRef => {
            value.as_str().map(drop).ok_or_else(|| wrong("a text id"))
        }
        FieldValue::Level => in_range(value, MIN_LEVEL, MAX_LEVEL),
        FieldValue::Byte => in_range(value, 0, i64::from(u8::MAX)),
        FieldValue::Position => position_of(value)
            .map(drop)
            .ok_or_else(|| wrong("[x, y, z] in metres")),
        FieldValue::Positions => {
            let points = value
                .as_array()
                .filter(|points| !points.is_empty())
                .ok_or_else(|| wrong("a list of [x, y, z]"))?;
            if points.iter().all(|point| position_of(point).is_some()) {
                Ok(())
            } else {
                Err(wrong("a list of [x, y, z]"))
            }
        }
        FieldValue::Behaviour => text(&BEHAVIOURS),
        FieldValue::PathMode => text(&PATH_MODES),
        FieldValue::Metres => match value {
            toml::Value::Float(number) if number.is_finite() && *number > 0.0 => Ok(()),
            toml::Value::Integer(number) if *number > 0 => Ok(()),
            _ => Err(wrong("a number above 0")),
        },
        FieldValue::Millis => in_range(value, 0, i64::MAX),
        FieldValue::Bool => value
            .as_bool()
            .map(drop)
            .ok_or_else(|| wrong("true or false")),
        FieldValue::ScriptNodes | FieldValue::ScriptLinks | FieldValue::EventWaves => value
            .as_array()
            .map(drop)
            .ok_or_else(|| wrong("a list of tables")),
        FieldValue::MillisList => {
            let list = value
                .as_array()
                .ok_or_else(|| wrong("a list of milliseconds"))?;
            if list
                .iter()
                .all(|item| item.as_integer().is_some_and(|ms| ms >= 0))
            {
                Ok(())
            } else {
                Err(wrong("a list of milliseconds"))
            }
        }
    }
}

fn in_range(value: &toml::Value, min: i64, max: i64) -> Result<(), Problem> {
    let number = value.as_integer().ok_or_else(|| wrong("a whole number"))?;
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

    const PROFILE: &str = "schema = 1\nkind = \"ai_profile\"\naction = \"add\"\nid = \"guard_fast\"\nbehaviour = \"guard\"\nspeed_mps = 4.5\nrespawn_ms = 60000\n";

    fn path_file(id: &str, map: &str) -> String {
        format!(
            "schema = 1\nkind = \"path\"\naction = \"add\"\nid = \"{id}\"\nmap = \"{map}\"\npoints = [[1.0, 2.0, 3.0], [4, 5, 6]]\nwaits_ms = [1000, 0]\nmode = \"back_and_forth\"\n"
        )
    }

    fn group_file(fields: &str) -> String {
        format!(
            "schema = 1\nkind = \"spawn_group\"\naction = \"add\"\nid = \"wolves\"\nmap = \"{MAP}\"\nvitals = \"Wolf\"\nlevel = 10\npoints = [[10.0, 20.0, 5.0]]\n{fields}"
        )
    }

    #[test]
    fn profiles_paths_and_groups_pass_with_their_references() {
        let errors = problems(&[
            ("ai/guard_fast.toml", PROFILE),
            (&format!("paths/{MAP}/gate.toml"), &path_file("gate", MAP)),
            (
                &format!("spawns/{MAP}/wolves.toml"),
                &group_file("ai_profile = \"guard_fast\"\npath = \"gate\"\n"),
            ),
        ]);
        assert_eq!(errors, []);
    }

    #[test]
    fn a_missing_profile_or_a_path_of_another_level_is_an_error() {
        let errors = problems(&[
            ("paths/other/gate.toml", &path_file("gate", "other")),
            (
                &format!("spawns/{MAP}/wolves.toml"),
                &group_file("ai_profile = \"nope\"\npath = \"gate\"\n"),
            ),
        ]);
        let fields: Vec<_> = errors.iter().map(|error| error.field.as_deref()).collect();
        assert_eq!(fields, [Some("ai_profile"), Some("path")]);
        assert!(
            errors
                .iter()
                .all(|error| matches!(error.problem, Problem::UnknownReference(_)))
        );
    }

    #[test]
    fn a_group_needs_its_fields_and_a_profile_its_values() {
        let group = "schema = 1\nkind = \"spawn_group\"\naction = \"add\"\nid = \"wolves\"\nmap = \"lv\"\nlevel = 10\n";
        let profile = PROFILE
            .replace("\"guard\"", "\"dance\"")
            .replace("4.5", "-1");
        let errors = problems(&[
            ("spawns/lv/wolves.toml", group),
            ("ai/guard_fast.toml", &profile),
        ]);
        let found: Vec<_> = errors
            .iter()
            .map(|error| (error.field.as_deref().unwrap_or(""), error.problem.clone()))
            .collect();
        assert!(found.contains(&("points", Problem::MissingField)));
        assert!(found.contains(&("vitals", Problem::MissingField)));
        assert!(
            found.iter().any(|(field, problem)| *field == "behaviour"
                && matches!(problem, Problem::OutOfRange(_)))
        );
        assert!(
            found.iter().any(|(field, problem)| *field == "speed_mps"
                && matches!(problem, Problem::WrongType(_)))
        );
    }

    #[test]
    fn own_kinds_can_only_be_added() {
        let errors = problems(&[(
            "ai/guard_fast.toml",
            &PROFILE.replace("\"add\"", "\"override\""),
        )]);
        assert_eq!(errors[0].problem, Problem::ActionNotAllowed);
    }

    #[test]
    fn a_spawn_point_may_name_a_profile() {
        let errors = problems(&[
            ("ai/guard_fast.toml", PROFILE),
            (&spawn_path(), &spawn("ai_profile = \"guard_fast\"\n")),
        ]);
        assert_eq!(errors, []);
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
    fn ids_that_differ_only_in_case_are_one_id() {
        let errors = problems(&[
            ("loot/tables/LT_A.toml", LOOT_OVERRIDE),
            (
                "loot/tables/lt_a.toml",
                &LOOT_OVERRIDE.replace("LT_A", "lt_a"),
            ),
        ]);
        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0].problem, Problem::DuplicateId { .. }));
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
