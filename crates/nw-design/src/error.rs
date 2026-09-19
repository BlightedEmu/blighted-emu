// SPDX-License-Identifier: AGPL-3.0-or-later
//! What is wrong with a design file.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// One problem of one design file, with the field it is about when there is one.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DesignError {
    /// The file, relative to the design folder, with `/` separators.
    pub path: PathBuf,
    /// The field the problem is about, when it is about one field.
    pub field: Option<String>,
    /// What is wrong.
    pub problem: Problem,
}

impl DesignError {
    /// A problem of the whole file at `path`.
    pub fn file(path: &Path, problem: Problem) -> Self {
        Self {
            path: path.to_path_buf(),
            field: None,
            problem,
        }
    }

    /// A problem of `field` in the file at `path`.
    pub fn field(path: &Path, field: &str, problem: Problem) -> Self {
        Self {
            path: path.to_path_buf(),
            field: Some(field.to_owned()),
            problem,
        }
    }
}

impl fmt::Display for DesignError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.field {
            Some(field) => write!(
                formatter,
                "{}:{field}: {}",
                self.path.display(),
                self.problem
            ),
            None => write!(formatter, "{}: {}", self.path.display(), self.problem),
        }
    }
}

/// The kinds of problems. Serialized as `{ "code": "<snake_case name>", "detail": ... }`.
#[derive(Debug, Clone, PartialEq, Serialize, thiserror::Error)]
#[serde(tag = "code", content = "detail", rename_all = "snake_case")]
pub enum Problem {
    /// The file cannot be read as UTF-8 text.
    #[error("cannot read the file: {0}")]
    Unreadable(String),
    /// The file is not valid TOML.
    #[error("not valid TOML: {0}")]
    BadToml(String),
    /// A header key (`schema`, `kind`, `action`, `id`) is missing or not a string or number.
    #[error("missing or wrong header key `{0}`")]
    MissingHeader(String),
    /// The `schema` number is not one this program reads.
    #[error("schema {0} is not known")]
    UnknownSchema(i64),
    /// The `kind` is not known.
    #[error("unknown kind `{0}`")]
    UnknownKind(String),
    /// The `action` is not known.
    #[error("unknown action `{0}`")]
    UnknownAction(String),
    /// The file name (without `.toml`) is not the id.
    #[error("the file name must be the id")]
    NameMismatch,
    /// The file is not in the folder of its kind.
    #[error("this kind belongs in `{expected}/`")]
    WrongFolder {
        /// The folder the file must be in.
        expected: String,
    },
    /// Another file has the same kind, map and id.
    #[error("the same id is also in {}", other.display())]
    DuplicateId {
        /// The other file.
        other: PathBuf,
    },
    /// The kind does not allow the action.
    #[error("this kind does not allow this action")]
    ActionNotAllowed,
    /// `base` is set on an action other than `add`.
    #[error("`base` is only for `add`")]
    BaseNotAllowed,
    /// A point kind without `map`.
    #[error("this kind needs `map`")]
    MissingMap,
    /// A `disable` file names fields.
    #[error("`disable` takes no fields")]
    FieldsNotAllowed,
    /// A field the kind needs is missing.
    #[error("missing field")]
    MissingField,
    /// The field names a profile or path that no design file makes.
    #[error("no design file makes {0}")]
    UnknownReference(String),
    /// The kind has no field of this name.
    #[error("unknown field")]
    UnknownField,
    /// The id field of the record is set as a field.
    #[error("set the id with `id`, not as a field")]
    IdField,
    /// The value has the wrong type.
    #[error("wrong type: {0}")]
    WrongType(String),
    /// The value is outside its range.
    #[error("out of range: {0}")]
    OutOfRange(String),
    /// An `add` without `base` does not hold a full row.
    #[error("an `add` without `base` needs a full row: {0}")]
    IncompleteRow(String),
    /// No client record has the id.
    #[error("no client record has this id")]
    UnknownId,
    /// A client record already has the id.
    #[error("a client record already has this id")]
    IdExists,
    /// No client record has the base id.
    #[error("no client record has the base id")]
    UnknownBase,
    /// There is no retail world file for the map.
    #[error("no retail world file for this map")]
    UnknownMap,
    /// A script graph problem.
    #[error("script: {0}")]
    Script(String),
}
