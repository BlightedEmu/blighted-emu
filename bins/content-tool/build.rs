// SPDX-License-Identifier: AGPL-3.0-or-later
//! Names the commit of this build of `content-tool` (`CONTENT_TOOL_COMMIT`): the `made_by` of the
//! files it records in a content manifest, with `-dirty` when tracked files have changes that are
//! not committed. A build outside a git checkout gets `unknown`. The script runs again when the
//! commit or the git index changes (a commit, a checkout, `git add`); an edit of a tracked file
//! alone does not run it again.

use std::process::Command;

/// The variable the code reads with `env!`.
const COMMIT_ENV: &str = "CONTENT_TOOL_COMMIT";
/// The commit when git cannot name it.
const UNKNOWN_COMMIT: &str = "unknown";
/// What `HEAD` holds when a branch is checked out: `ref: <the branch ref>`.
const REF_PREFIX: &str = "ref: ";
/// The file of the packed branch refs.
const PACKED_REFS: &str = "packed-refs";
/// The git index: it changes with what is staged.
const INDEX: &str = "index";
/// What follows the commit when tracked files have changes that are not committed.
const DIRTY_SUFFIX: &str = "-dirty";

/// The output of `git <args>`, trimmed; `None` when git fails.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Asks cargo to run this script again when the git file `name` changes (when it exists).
fn watch(name: &str) {
    if let Some(path) = git(&["rev-parse", "--git-path", name])
        && std::path::Path::new(&path).exists()
    {
        println!("cargo:rerun-if-changed={path}");
    }
}

fn main() {
    watch("HEAD");
    watch(INDEX);
    if let Some(head) = git(&["rev-parse", "--git-path", "HEAD"])
        .and_then(|path| std::fs::read_to_string(path).ok())
        && let Some(branch) = head.trim().strip_prefix(REF_PREFIX)
    {
        watch(branch);
        watch(PACKED_REFS);
    }
    let commit = git(&["rev-parse", "HEAD"]).map_or_else(
        || UNKNOWN_COMMIT.to_owned(),
        |commit| {
            let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
                .is_some_and(|changes| !changes.is_empty());
            if dirty {
                format!("{commit}{DIRTY_SUFFIX}")
            } else {
                commit
            }
        },
    );
    println!("cargo:rustc-env={COMMIT_ENV}={commit}");
}
