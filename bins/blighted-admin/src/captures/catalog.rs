// SPDX-License-Identifier: AGPL-3.0-or-later
//! The sessions under the captures folder (admin tools spec section 4.4): every `ledger.bin`,
//! with the name and start time of its `metadata.json` and, for an open-world session, the
//! title of the saved Discord post that holds its archive.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// File name of a ledger.
pub const LEDGER_FILE: &str = "ledger.bin";
/// File name of the capture tool's session facts.
pub const METADATA_FILE: &str = "metadata.json";
/// Folder of the open-world sessions in the captures folder.
pub const OPEN_WORLD_DIR: &str = "open-world";
/// End of a saved Discord post's file folder.
const POST_FOLDER_END: &str = ".html_Files";
/// Start of a saved Discord post's name.
const POST_NAME_START: &str = "Open World - file-sharing - ";
/// Deepest folder searched below the captures folder.
const MAX_DEPTH: usize = 10;

/// One recorded session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionEntry {
    /// The ledger path relative to the captures folder, with `/`.
    pub id: String,
    /// The ledger file.
    #[serde(skip)]
    pub path: PathBuf,
    /// Ledger bytes.
    pub bytes: u64,
    /// `metadata.json` `name`.
    pub name: Option<String>,
    /// `metadata.json` `started_at_utc`.
    pub started_at: Option<String>,
    /// Title of the Discord post that holds its archive.
    pub post_title: Option<String>,
}

#[derive(Deserialize)]
struct Metadata {
    name: Option<String>,
    started_at_utc: Option<String>,
}

/// Every ledger under `captures_dir`, sorted by id. Unreadable folders and files are skipped
/// with a warning.
pub fn scan(captures_dir: &Path, discord_dir: &Path) -> Vec<SessionEntry> {
    let titles = post_titles(discord_dir);
    let mut ledgers = Vec::new();
    find_ledgers(captures_dir, 0, &mut ledgers);
    let mut sessions: Vec<SessionEntry> = ledgers
        .into_iter()
        .filter_map(|path| {
            let relative = path.strip_prefix(captures_dir).ok()?;
            let parts: Vec<String> = relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect();
            let bytes = std::fs::metadata(&path).ok()?.len();
            let metadata = read_metadata(&path);
            let post_title = match parts.as_slice() {
                [open_world, stem, ..] if open_world == OPEN_WORLD_DIR => titles.get(stem).cloned(),
                _ => None,
            };
            Some(SessionEntry {
                id: parts.join("/"),
                bytes,
                name: metadata.as_ref().and_then(|facts| facts.name.clone()),
                started_at: metadata.and_then(|facts| facts.started_at_utc),
                post_title,
                path,
            })
        })
        .collect();
    sessions.sort_by(|left, right| left.id.cmp(&right.id));
    sessions
}

/// The sessions whose id, name or post title holds `query` (any case); all for an empty one.
pub fn search<'a>(sessions: &'a [SessionEntry], query: &str) -> Vec<&'a SessionEntry> {
    let query = query.to_lowercase();
    sessions
        .iter()
        .filter(|entry| {
            [
                Some(&entry.id),
                entry.name.as_ref(),
                entry.post_title.as_ref(),
            ]
            .into_iter()
            .flatten()
            .any(|text| text.to_lowercase().contains(&query))
        })
        .collect()
}

fn find_ledgers(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::warn!(dir = %dir.display(), %error, "folder skipped");
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // `metadata` follows links, as `find -L` of `just capture-check`.
        let Ok(kind) = std::fs::metadata(&path) else {
            continue;
        };
        if kind.is_dir() {
            find_ledgers(&path, depth + 1, found);
        } else if entry.file_name() == LEDGER_FILE {
            found.push(path);
        }
    }
}

/// `metadata.json` beside the ledger, else in the folder above (sessions with a `dtls` folder).
fn read_metadata(ledger: &Path) -> Option<Metadata> {
    let folder = ledger.parent()?;
    [Some(folder), folder.parent()]
        .into_iter()
        .flatten()
        .map(|dir| dir.join(METADATA_FILE))
        .find(|file| file.is_file())
        .and_then(|file| {
            let bytes = std::fs::read(&file).ok()?;
            serde_json::from_slice(&bytes)
                .inspect_err(|error| {
                    tracing::warn!(file = %file.display(), %error, "metadata skipped");
                })
                .ok()
        })
}

/// Archive stem to post title, from the saved posts' file folders.
fn post_titles(discord_dir: &Path) -> HashMap<String, String> {
    let mut titles = HashMap::new();
    let Ok(posts) = std::fs::read_dir(discord_dir) else {
        return titles;
    };
    for post in posts.flatten() {
        let folder = post.file_name().to_string_lossy().into_owned();
        let Some(title) = post_title(&folder) else {
            continue;
        };
        let Ok(files) = std::fs::read_dir(post.path()) else {
            continue;
        };
        for file in files.flatten() {
            let name = file.file_name().to_string_lossy().into_owned();
            if let Some((stem, _)) = name.rsplit_once('.') {
                titles.insert(stem.to_owned(), title.clone());
            }
        }
    }
    titles
}

/// `Open World - file-sharing - <title> [<n>].html_Files` to `<title>`.
fn post_title(folder: &str) -> Option<String> {
    let name = folder.strip_suffix(POST_FOLDER_END)?;
    let name = name.strip_prefix(POST_NAME_START).unwrap_or(name);
    let title = match name.rsplit_once(" [") {
        Some((title, number)) if number.ends_with(']') => title,
        _ => name,
    };
    Some(title.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEDGER_BYTES: &[u8] = b"NWDL";

    fn ledger(dir: &Path, folder: &str) -> PathBuf {
        let folder = dir.join(folder);
        std::fs::create_dir_all(&folder).expect("dir");
        std::fs::write(folder.join(LEDGER_FILE), LEDGER_BYTES).expect("ledger");
        folder
    }

    #[test]
    fn sessions_are_found_with_their_metadata() {
        let root = tempfile::tempdir().expect("dir");
        let captures = root.path().join("captures");
        let folder = ledger(&captures, "Craft-Orbs");
        std::fs::write(
            folder.join(METADATA_FILE),
            r#"{"name":"Craft orbs","started_at_utc":"2026-10-01T10:00:00Z","id":"x"}"#,
        )
        .expect("metadata");
        ledger(&captures, "side-quests/zone/session/dtls");
        let sessions = scan(&captures, &root.path().join("discord"));
        let found: Vec<(&str, Option<&str>, u64)> = sessions
            .iter()
            .map(|entry| (entry.id.as_str(), entry.name.as_deref(), entry.bytes))
            .collect();
        assert_eq!(
            found,
            vec![
                ("Craft-Orbs/ledger.bin", Some("Craft orbs"), 4),
                ("side-quests/zone/session/dtls/ledger.bin", None, 4),
            ]
        );
        assert_eq!(
            sessions[0].started_at.as_deref(),
            Some("2026-10-01T10:00:00Z")
        );
    }

    #[test]
    fn metadata_in_the_parent_of_a_dtls_folder_counts() {
        let root = tempfile::tempdir().expect("dir");
        let captures = root.path().join("captures");
        ledger(&captures, "run/dtls");
        std::fs::write(
            captures.join("run").join(METADATA_FILE),
            r#"{"name":"Run"}"#,
        )
        .expect("metadata");
        let sessions = scan(&captures, &root.path().join("discord"));
        assert_eq!(sessions[0].name.as_deref(), Some("Run"));
    }

    #[test]
    fn a_bad_metadata_file_keeps_the_session_without_a_name() {
        let root = tempfile::tempdir().expect("dir");
        let captures = root.path().join("captures");
        let folder = ledger(&captures, "broken");
        std::fs::write(folder.join(METADATA_FILE), b"{not json").expect("metadata");
        let sessions = scan(&captures, &root.path().join("discord"));
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, None);
    }

    #[test]
    fn an_open_world_session_gets_its_post_title() {
        let root = tempfile::tempdir().expect("dir");
        let captures = root.path().join("captures");
        ledger(&captures, "open-world/run-1-abc/run_1/dtls");
        let post = root
            .path()
            .join("discord")
            .join("Open World - file-sharing - Gorgon raid - part 2 [123].html_Files");
        std::fs::create_dir_all(&post).expect("dir");
        std::fs::write(post.join("run-1-abc.rar"), b"archive").expect("archive");
        std::fs::write(post.join("style.css"), b"").expect("page file");
        let sessions = scan(&captures, &root.path().join("discord"));
        assert_eq!(
            sessions[0].post_title.as_deref(),
            Some("Gorgon raid - part 2")
        );
    }

    #[test]
    fn the_search_matches_name_and_title() {
        let entry = |id: &str, name: Option<&str>, title: Option<&str>| SessionEntry {
            id: id.to_owned(),
            path: PathBuf::from(id),
            bytes: 0,
            name: name.map(str::to_owned),
            started_at: None,
            post_title: title.map(str::to_owned),
        };
        let sessions = vec![
            entry("a/ledger.bin", Some("Training Dummy"), None),
            entry("b/ledger.bin", None, Some("Gorgon raid")),
            entry("catacombs/ledger.bin", None, None),
        ];
        let ids = |query: &str| -> Vec<String> {
            search(&sessions, query)
                .into_iter()
                .map(|entry| entry.id.clone())
                .collect()
        };
        assert_eq!(ids("dummy"), vec!["a/ledger.bin"]);
        assert_eq!(ids("GORGON"), vec!["b/ledger.bin"]);
        assert_eq!(ids("catacombs"), vec!["catacombs/ledger.bin"]);
        assert_eq!(ids("").len(), 3);
    }
}
