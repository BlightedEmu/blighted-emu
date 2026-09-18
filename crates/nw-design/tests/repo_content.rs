// SPDX-License-Identifier: AGPL-3.0-or-later
//! The schema level check of the repository's own `content/` folder (spec section 5.3). This is
//! how `just check` checks design files on a PC without the game.

use std::path::Path;

use nw_design::{check_schema, load_dir};

#[test]
fn the_repository_design_files_pass_the_schema_check() {
    let content = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    let (set, mut errors) = load_dir(&content);
    errors.extend(check_schema(&set));
    let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
    assert!(
        lines.is_empty(),
        "design file errors:\n{}",
        lines.join("\n")
    );
}
