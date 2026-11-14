// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool realm-pack make`: reads the pack tables of a content folder, scrubs a copy,
//! scans it, and writes the zip, its checksum file and the review report. The content folder is
//! only read.

use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use nw_content::ContentBundle;
use nw_content::breach_tiers::BreachTiers;
use nw_content::bundle::bundle_path;
use nw_content::dungeon_entrances::DungeonEntrances;
use nw_content::dungeon_gates::GateRules;
use nw_content::ftue_entities::FtueEntities;
use nw_content::invasion_forts::InvasionForts;
use nw_content::mob_prefabs::MobPrefabs;
use nw_content::outpost_structures::OutpostStructures;
use nw_content::player_sequences::PlayerSequences;
use nw_content::shot_entities::ShotEntities;
use nw_content::spell_slices::SpellSlices;
use nw_content::town_props::TownProps;
use nw_content::town_stations::TownStations;
use nw_content::war_forts::WarForts;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::manifest::{
    MANIFEST_FILE, Manifest, PACK_FORMAT, PACK_TABLES, PackFile, REPORT_FILE, SUMS_FILE,
    pack_zip_name, sha256_hex, table_file_name,
};
use super::scan::{ReviewRun, scan_table};
use super::scrub::scrub_table;
use super::strings::GameWords;
use super::{PackError, io_error};

/// The review report line when there is nothing to read.
const NO_REVIEW_RUNS: &str = "no review runs";

/// What a make wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MakeReport {
    /// The zip.
    pub zip: PathBuf,
    /// The owner, turret and territory interactor states looked at (and set to no one where
    /// they named someone).
    pub identity_states: usize,
    /// The capturing player's ids zeroed.
    pub player_ids: usize,
    /// The different words in the review report.
    pub review_words: usize,
}

/// Checks that `text` loads as pack table `table` with its `nw-content` loader.
///
/// # Errors
///
/// Returns [`PackError::TableLoad`] when it does not, or when `table` is no pack table.
pub fn parse_pack_table(table: &str, text: &str) -> Result<(), PackError> {
    fn loaded<T, E: ToString>(table: &str, result: Result<T, E>) -> Result<(), PackError> {
        result.map(drop).map_err(|error| PackError::TableLoad {
            table: table.to_owned(),
            reason: error.to_string(),
        })
    }
    match table {
        "mob-prefabs" => loaded(table, MobPrefabs::parse(text)),
        "ftue-entities" => loaded(table, FtueEntities::parse(text)),
        "breach-tiers" => loaded(table, BreachTiers::parse(text)),
        "player-sequences" => loaded(table, PlayerSequences::parse(text)),
        "town-props" | "encounter-roots" | "encounter-objects" => {
            loaded(table, TownProps::parse(text))
        }
        "spell-slices" => loaded(table, SpellSlices::parse(text)),
        "shot-entities" => loaded(table, ShotEntities::parse(text)),
        "dungeon-gate-rules" => loaded(table, GateRules::parse(text)),
        "dungeon-entrances" => loaded(table, DungeonEntrances::parse(text)),
        "invasion-forts" => loaded(table, InvasionForts::parse(text)),
        "war-forts" => loaded(table, WarForts::parse(text)),
        "outpost-structures" => loaded(table, OutpostStructures::parse(text)),
        "town-stations" => loaded(table, TownStations::parse(text)),
        _ => Err(PackError::TableLoad {
            table: table.to_owned(),
            reason: "not a pack table".to_owned(),
        }),
    }
}

/// Makes the pack of the tables in `content_dir` into `out_dir`. The game words come from the
/// merged bundle of the content folder.
///
/// # Errors
///
/// Returns [`PackError`] when a table is missing or does not load, the scan finds a hit, or a
/// file cannot be read or written.
pub fn make_pack(
    content_dir: &Path,
    out_dir: &Path,
    client_build: &str,
    pack_version: u32,
    source_commit: &str,
) -> Result<MakeReport, PackError> {
    let bundle = ContentBundle::load(&bundle_path(content_dir, client_build), client_build)?;
    let words = GameWords::from_bundle(&bundle);
    drop(bundle);
    make_pack_with_words(
        content_dir,
        out_dir,
        client_build,
        pack_version,
        source_commit,
        &words,
    )
}

/// [`make_pack`] with the game words given.
///
/// # Errors
///
/// As [`make_pack`].
pub(crate) fn make_pack_with_words(
    content_dir: &Path,
    out_dir: &Path,
    client_build: &str,
    pack_version: u32,
    source_commit: &str,
    words: &GameWords,
) -> Result<MakeReport, PackError> {
    // First every table is scrubbed (the tutorial gives the player ids), then every table is
    // scanned for them and the other rules.
    let mut scrubbed_tables = Vec::new();
    let mut player_ids: Vec<Vec<u8>> = Vec::new();
    let mut identity_states = 0;
    for table in PACK_TABLES {
        let name = table_file_name(table, client_build);
        let path = content_dir.join(&name);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Err(PackError::MissingFile { file: name });
            }
            Err(source) => return Err(PackError::Io { path, source }),
        };
        parse_pack_table(table, &text)?;
        let mut value: serde_json::Value = serde_json::from_str(&text)?;
        let scrubbed = scrub_table(table, &mut value)?;
        identity_states += scrubbed.identity_states;
        for id in scrubbed.player_ids {
            if !player_ids.contains(&id) {
                player_ids.push(id);
            }
        }
        let bytes = serde_json::to_vec_pretty(&value)?;
        parse_pack_table(table, &String::from_utf8_lossy(&bytes))?;
        scrubbed_tables.push((table, name, value, bytes));
    }
    let mut hits = Vec::new();
    let mut review = Vec::new();
    let mut files = Vec::new();
    for (table, name, value, bytes) in scrubbed_tables {
        let (table_hits, table_review) = scan_table(table, &value, words, &player_ids)?;
        hits.extend(table_hits);
        review.extend(table_review);
        files.push((name, bytes));
    }
    if !hits.is_empty() {
        return Err(PackError::PrivacyHits(hits));
    }
    let manifest = Manifest {
        pack_format: PACK_FORMAT,
        pack_version,
        client_build: client_build.to_owned(),
        source_commit: source_commit.to_owned(),
        files: files
            .iter()
            .map(|(name, bytes)| PackFile {
                name: name.clone(),
                bytes: bytes.len() as u64,
                sha256: sha256_hex(bytes),
            })
            .collect(),
    };
    let zip_name = pack_zip_name(client_build);
    let zip_bytes = zip_files(&files, &serde_json::to_vec_pretty(&manifest)?)?;
    std::fs::create_dir_all(out_dir).map_err(io_error(out_dir))?;
    let zip = out_dir.join(&zip_name);
    std::fs::write(&zip, &zip_bytes).map_err(io_error(&zip))?;
    let sums = out_dir.join(SUMS_FILE);
    std::fs::write(&sums, format!("{}  {zip_name}\n", sha256_hex(&zip_bytes)))
        .map_err(io_error(&sums))?;
    let report = out_dir.join(REPORT_FILE);
    let (report_text, review_words) = review_report(&review);
    std::fs::write(&report, report_text).map_err(io_error(&report))?;
    Ok(MakeReport {
        zip,
        identity_states,
        player_ids: player_ids.len(),
        review_words,
    })
}

fn zip_files(files: &[(String, Vec<u8>)], manifest: &[u8]) -> Result<Vec<u8>, PackError> {
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .chain(std::iter::once((MANIFEST_FILE, manifest)))
    {
        zip.start_file(name, options)?;
        zip.write_all(bytes).map_err(io_error(name))?;
    }
    Ok(zip.finish()?.into_inner())
}

/// The review report (one line per table and word: the word, how often it showed, and the first
/// place) and the number of lines.
fn review_report(review: &[ReviewRun]) -> (String, usize) {
    if review.is_empty() {
        return (format!("{NO_REVIEW_RUNS}\n"), 0);
    }
    let mut words: BTreeMap<(&str, &str), (usize, &str)> = BTreeMap::new();
    for run in review {
        words
            .entry((run.table.as_str(), run.text.as_str()))
            .or_insert((0, run.path.as_str()))
            .0 += 1;
    }
    let text = words
        .iter()
        .map(|((table, word), (count, path))| format!("{table} {word} {count} {path}\n"))
        .collect();
    (text, words.len())
}

#[cfg(test)]
pub(crate) mod tests {
    use serde_json::{Value, json};

    use super::super::owner::tests::{
        MADE_UP_COMPANY, STRUCTURE_NAME, owned_state, owner_member_hex,
    };
    use super::*;

    pub(crate) const BUILD: &str = "12345";
    const VERSION: u32 = 1;
    const COMMIT: &str = "0123abcd";
    /// A made-up asset id (16 bytes).
    const ASSET: &str = "00112233445566778899aabbccddeeff";

    /// The smallest JSON each pack table's loader reads; `town-props` holds a made-up owner
    /// member and `dungeon-gate-rules` made-up capture names.
    pub(crate) fn fixture_tables() -> Vec<(&'static str, Value)> {
        vec![
            ("mob-prefabs", json!([])),
            (
                "ftue-entities",
                json!({"level": "ftue_v2", "entities": [], "asset_entities": [], "mobs": []}),
            ),
            ("breach-tiers", json!([])),
            (
                "player-sequences",
                json!({"stances": [], "follow_ups": [], "abilities": [], "reactions": []}),
            ),
            (
                "town-props",
                json!({"props": [{"asset": ASSET, "members": [owner_member_hex(owned_state())]}]}),
            ),
            ("encounter-roots", json!({"props": []})),
            ("encounter-objects", json!({"props": []})),
            ("spell-slices", json!({"spells": []})),
            (
                "shot-entities",
                json!({"source": "made-up tool", "shots": []}),
            ),
            (
                "dungeon-gate-rules",
                json!({"source": "made-up tool", "levels": []}),
            ),
            (
                "dungeon-entrances",
                json!({"entrances": [], "source": "made-up tool"}),
            ),
            ("invasion-forts", json!({"forts": []})),
            (
                "war-forts",
                json!({"encounter_turrets": [], "forts": [], "level_droppers": [], "structure_kinds": [], "turret_kinds": []}),
            ),
            ("outpost-structures", json!({"kinds": [], "maps": []})),
            ("town-stations", json!({"slots": [], "slices": []})),
        ]
    }

    /// The game words of the fixture: the owner state's slice structure name.
    fn fixture_words() -> GameWords {
        GameWords::from_strings([String::from_utf8(STRUCTURE_NAME.to_vec()).unwrap()])
    }

    /// Writes the fixture tables into `dir`.
    pub(crate) fn write_fixture(dir: &Path) {
        for (table, value) in fixture_tables() {
            std::fs::write(
                dir.join(table_file_name(table, BUILD)),
                serde_json::to_vec(&value).unwrap(),
            )
            .unwrap();
        }
    }

    /// Makes a pack of the fixture into `out`.
    pub(crate) fn make_fixture_pack(content: &Path, out: &Path) -> MakeReport {
        write_fixture(content);
        make_pack_with_words(content, out, BUILD, VERSION, COMMIT, &fixture_words()).unwrap()
    }

    fn folder_sums(dir: &Path) -> Vec<(String, String)> {
        let mut sums: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    sha256_hex(&std::fs::read(entry.path()).unwrap()),
                )
            })
            .collect();
        sums.sort();
        sums
    }

    #[test]
    fn make_writes_zip_manifest_sums_and_report() {
        let content = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let report = make_fixture_pack(content.path(), out.path());
        assert_eq!(report.identity_states, 1);
        assert_eq!(report.zip, out.path().join(pack_zip_name(BUILD)));
        let zip_bytes = std::fs::read(&report.zip).unwrap();
        let sums = std::fs::read_to_string(out.path().join(SUMS_FILE)).unwrap();
        assert_eq!(
            sums,
            format!("{}  {}\n", sha256_hex(&zip_bytes), pack_zip_name(BUILD))
        );
        assert_eq!(
            std::fs::read_to_string(out.path().join(REPORT_FILE)).unwrap(),
            format!("{NO_REVIEW_RUNS}\n")
        );
        let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes)).unwrap();
        let manifest: Manifest =
            serde_json::from_reader(archive.by_name(MANIFEST_FILE).unwrap()).unwrap();
        assert_eq!(manifest.files.len(), PACK_TABLES.len());
        assert_eq!(
            (manifest.pack_version, manifest.source_commit.as_str()),
            (VERSION, COMMIT)
        );
        let mut props = String::new();
        std::io::Read::read_to_string(
            &mut archive
                .by_name(&table_file_name("town-props", BUILD))
                .unwrap(),
            &mut props,
        )
        .unwrap();
        assert!(!props.contains(&hex::encode(MADE_UP_COMPANY)));
        let mut shots = String::new();
        std::io::Read::read_to_string(
            &mut archive
                .by_name(&table_file_name("shot-entities", BUILD))
                .unwrap(),
            &mut shots,
        )
        .unwrap();
        assert!(!shots.contains("made-up tool"));
    }

    #[test]
    fn make_leaves_the_content_folder_unchanged() {
        let content = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        write_fixture(content.path());
        let before = folder_sums(content.path());
        make_fixture_pack(content.path(), out.path());
        assert_eq!(folder_sums(content.path()), before);
    }

    #[test]
    fn make_fails_on_a_two_word_hit_left_after_scrub() {
        let content = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        write_fixture(content.path());
        let mut body = vec![0x01];
        body.extend_from_slice(MADE_UP_COMPANY);
        let props = json!({"props": [{"asset": ASSET, "members": [], "note": "Made Up Company", "body": hex::encode(body)}]});
        std::fs::write(
            content
                .path()
                .join(table_file_name("encounter-roots", BUILD)),
            props.to_string(),
        )
        .unwrap();
        match make_pack_with_words(
            content.path(),
            out.path(),
            BUILD,
            VERSION,
            COMMIT,
            &GameWords::default(),
        ) {
            Err(PackError::PrivacyHits(hits)) => {
                assert_eq!(hits.len(), 1, "the plain note is no hit: {hits:?}");
                assert_eq!(hits[0].path, "/props/0/body");
            }
            other => panic!("expected a privacy hit, got {other:?}"),
        }
        assert!(!out.path().join(pack_zip_name(BUILD)).exists());
    }

    #[test]
    fn make_fails_on_a_missing_table() {
        let content = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        write_fixture(content.path());
        std::fs::remove_file(content.path().join(table_file_name("war-forts", BUILD))).unwrap();
        assert!(matches!(
            make_pack_with_words(
                content.path(),
                out.path(),
                BUILD,
                VERSION,
                COMMIT,
                &GameWords::default()
            ),
            Err(PackError::MissingFile { .. })
        ));
    }
}
