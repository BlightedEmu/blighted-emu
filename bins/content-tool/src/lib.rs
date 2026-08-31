// SPDX-License-Identifier: AGPL-3.0-or-later
//! Library part of `content-tool`: code generation and bundle building.

pub mod client_build;
pub mod codegen;
pub mod error;
pub mod source;

#[cfg(all(test, feature = "local-data"))]
mod local_data_tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use nw_formats::local_data::*;

    use crate::client_build::load_client_build;
    use crate::codegen::run_codegen;
    use crate::source::GameSource;

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn read_folder(dir: &Path) -> BTreeMap<String, String> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|item| {
                let path = item.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                (name, std::fs::read_to_string(&path).unwrap())
            })
            .collect()
    }

    #[test]
    fn committed_config_names_the_pinned_build() {
        let build = load_client_build(&repo_root().join("config/client-build.toml")).unwrap();
        assert_eq!(build, PINNED_STEAM_BUILD);
    }

    #[test]
    fn committed_generated_code_matches_codegen() {
        let out = tempfile::tempdir().unwrap();
        let source = GameSource::open(&game_dir()).unwrap();
        let summary = run_codegen(&source, PINNED_STEAM_BUILD, out.path()).unwrap();
        assert_eq!(summary.datasheet_files, PINNED_DATASHEET_FILES);
        assert_eq!(summary.table_types, PINNED_TABLE_TYPES);
        let fresh = read_folder(out.path());
        let committed = read_folder(&repo_root().join("crates/nw-content/src/generated"));
        assert_eq!(
            fresh.keys().collect::<Vec<_>>(),
            committed.keys().collect::<Vec<_>>()
        );
        for (name, contents) in &fresh {
            assert!(
                committed[name] == *contents,
                "{name} differs from fresh codegen output"
            );
        }
    }
}
