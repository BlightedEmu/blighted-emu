// SPDX-License-Identifier: AGPL-3.0-or-later
//! `mob-prefabs`: builds the mob prefab table (`mob-prefabs-<build>.json`, read by
//! `nw_content::mob_prefabs`) from the retail capture ledgers (mob-cards note, sections 1 and 2).
//!
//! A retail mob comes into view as one bundle group: its spawn card (component 0, with the asset
//! of its creature slice), its `ALCReplicatedState`, its vitals state (whose `vitalsId` is the
//! lowercase CRC-32 of its vitals row id), and other states of its prefab. Its death is a later
//! group of the same slot that sets health 0. The table keeps, per vitals id and asset, the most
//! common first group and death group, with the member bodies of the capture.
//!
//! A member body has no length: a member of a class without a reader in `nw-messages` or in
//! [`crate::extra_members`] ends the walk of its payload. The walk then looks for the next group
//! that starts with a spawn card or belongs to a mob in view, and goes on from there. A member
//! that cannot be read, or whose `nw-messages` encoding differs from the capture, is left out and
//! its class is listed in `skipped_classes`; a member read by [`crate::extra_members`] keeps its
//! capture bytes.
//!
//! The ledgers are large, so each one is read by its own worker process (`mob-prefabs-ledger`),
//! which prints a small tally; the main process only merges the tallies. The ledgers hold other
//! players' data: a tally keeps only mob prefab ids and the bodies of mob states.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsString;
use std::fmt;
use std::io::Read;
use std::num::NonZeroUsize;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use nw_capture::decode::INLINE_UUID_CLASS;
use nw_capture::{Decoder, SERVER_TO_CLIENT, ledger_records};
use nw_content::Tables;
use nw_content::ids::vitals_ids;
use nw_content::mob_prefabs::ASSET_ID_LEN;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::{
    ALC_TYPE_INDEX, AssetId, BodyCodec, BundleMember, BundlePayload, GDE_METADATA_COMPONENT,
    GDE_METADATA_TYPE_INDEX, MemberState, PAPERDOLL_TYPE_INDEX, PLAYER_SLICE_ASSET,
    REPLICATED_STATE_BUNDLE_TYPE_INDEX, Reader, ReplicatedStateBundle, VITALS_TYPE_INDEX, Writer,
};
use nw_reflect::{TypeId, decode_type_id};
use serde::{Deserialize, Serialize};

use crate::extra_members;

/// The capture folders scanned by default: the unpacked ledgers and the source archives.
pub const DEFAULT_CAPTURE_ROOTS: [&str; 2] =
    ["local-data/captures", "local-data/open world captures"];
/// Name of a ledger file, alone or as the last part of a zip entry name.
const LEDGER_FILE_NAME: &str = "ledger.bin";
/// Separator of the folders in a zip entry name.
const ZIP_SEPARATOR: char = '/';
const ZIP_EXTENSION: &str = "zip";
/// Archives of this kind cannot be read here; their ledgers are unpacked under `captures/`.
const RAR_EXTENSION: &str = "rar";
/// How long after its card a mob's first paperdoll change counts as its spawn draw: retail
/// changed the public paperdoll of most fresh mobs 50 to 150 ms after their card (sailor 1 of
/// newchar-retail-2026-09-18 at 102 ms, its main hand drawn), and a few up to 800 ms.
const DRAW_WINDOW_MS: u64 = 1_000;
/// Health at or below which a vitals state is a death.
const DEAD_HEALTH: f32 = 0.0;
/// Most different bodies counted for one member of one group form; later new bodies are not
/// counted, which keeps a tally small.
const MAX_BODY_VARIANTS: usize = 64;
/// Finished ledgers between two progress lines.
const PROGRESS_EVERY: usize = 50;
/// Mask of the slot id in a group integer: the client keeps its low 16 bits.
const INTEREST_MASK: u64 = 0xFFFF;
/// Worker subcommand and its arguments (see `main.rs`).
pub const WORKER_COMMAND: &str = "mob-prefabs-ledger";
const WORKER_LEDGER_ARG: &str = "--ledger";
const WORKER_ENTRY_ARG: &str = "--entry";

/// A failure of the mob prefab scan.
#[derive(Debug, thiserror::Error)]
pub enum MobPrefabsError {
    /// A file or folder cannot be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file or folder.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// A zip archive cannot be read.
    #[error("{path}: {source}")]
    Zip {
        /// The archive.
        path: PathBuf,
        /// Why.
        source: zip::result::ZipError,
    },
    /// A worker process failed or printed something that is not a tally.
    #[error("worker for {source_name}: {reason}")]
    Worker {
        /// The ledger.
        source_name: String,
        /// Why.
        reason: String,
    },
    /// A tally or the table cannot be written as JSON.
    #[error("mob prefab JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// One ledger: a file, or an entry of a zip archive.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LedgerSource {
    /// The ledger file, or the zip archive that holds it.
    pub path: PathBuf,
    /// The entry name in the archive; `None` for a plain file.
    pub entry: Option<String>,
}

impl fmt::Display for LedgerSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.entry {
            Some(entry) => write!(formatter, "{} ({entry})", self.path.display()),
            None => write!(formatter, "{}", self.path.display()),
        }
    }
}

impl LedgerSource {
    /// Reads the whole ledger.
    ///
    /// # Errors
    ///
    /// Returns [`MobPrefabsError`] when the file or the archive entry cannot be read.
    pub fn read(&self) -> Result<Vec<u8>, MobPrefabsError> {
        let io = |source| MobPrefabsError::Io {
            path: self.path.clone(),
            source,
        };
        let Some(entry) = &self.entry else {
            return std::fs::read(&self.path).map_err(io);
        };
        let zip = |source| MobPrefabsError::Zip {
            path: self.path.clone(),
            source,
        };
        let mut archive =
            zip::ZipArchive::new(std::fs::File::open(&self.path).map_err(io)?).map_err(zip)?;
        let mut file = archive.by_name(entry).map_err(zip)?;
        let mut bytes = Vec::with_capacity(usize::try_from(file.size()).unwrap_or_default());
        file.read_to_end(&mut bytes).map_err(io)?;
        Ok(bytes)
    }

    /// The worker arguments that name this ledger.
    pub fn worker_args(&self) -> Vec<OsString> {
        let mut args = vec![
            OsString::from(WORKER_LEDGER_ARG),
            self.path.clone().into_os_string(),
        ];
        if let Some(entry) = &self.entry {
            args.push(OsString::from(WORKER_ENTRY_ARG));
            args.push(OsString::from(entry));
        }
        args
    }
}

/// The ledgers found under the capture folders.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceList {
    /// The ledgers, sorted.
    pub sources: Vec<LedgerSource>,
    /// RAR archives seen (not read).
    pub rar_archives: usize,
    /// Zip archives that cannot be read, with the reason.
    pub bad_archives: Vec<(PathBuf, String)>,
}

/// Finds every `ledger.bin` file and every zip entry named `ledger.bin` under `roots`.
///
/// # Errors
///
/// Returns [`MobPrefabsError::Io`] when a folder cannot be listed.
pub fn find_ledger_sources(roots: &[PathBuf]) -> Result<SourceList, MobPrefabsError> {
    let mut list = SourceList::default();
    let mut folders: Vec<PathBuf> = roots.to_vec();
    while let Some(folder) = folders.pop() {
        let io = |source| MobPrefabsError::Io {
            path: folder.clone(),
            source,
        };
        for item in std::fs::read_dir(&folder).map_err(io)? {
            let path = item.map_err(io)?.path();
            if path.is_dir() {
                folders.push(path);
            } else if path
                .file_name()
                .is_some_and(|name| name == LEDGER_FILE_NAME)
            {
                list.sources.push(LedgerSource { path, entry: None });
            } else if has_extension(&path, ZIP_EXTENSION) {
                match zip_ledgers(&path) {
                    Ok(entries) => {
                        list.sources
                            .extend(entries.into_iter().map(|entry| LedgerSource {
                                path: path.clone(),
                                entry: Some(entry),
                            }))
                    }
                    Err(error) => list.bad_archives.push((path, error.to_string())),
                }
            } else if has_extension(&path, RAR_EXTENSION) {
                list.rar_archives += 1;
            }
        }
    }
    list.sources.sort();
    list.bad_archives.sort();
    Ok(list)
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .is_some_and(|found| found.eq_ignore_ascii_case(extension))
}

/// The ledger entry names of a zip archive; only its directory is read.
fn zip_ledgers(path: &Path) -> Result<Vec<String>, zip::result::ZipError> {
    let archive = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    archive
        .file_names()
        .filter_map(|name| match name {
            Ok(name) if name.rsplit(ZIP_SEPARATOR).next() == Some(LEDGER_FILE_NAME) => {
                Some(Ok(name.into_owned()))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}

/// Identifies a ledger's bytes, so that a ledger found twice (unpacked and in its archive) is
/// counted once.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LedgerDigest {
    /// Bytes.
    pub len: u64,
    /// CRC-32 of the bytes.
    pub crc: u32,
}

impl LedgerDigest {
    /// The digest of `bytes`.
    pub fn of(bytes: &[u8]) -> Self {
        Self {
            len: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            crc: crc32fast::hash(bytes),
        }
    }
}

/// What the scan saw, for the report.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanStats {
    /// Server bundles read.
    pub bundles: u64,
    /// Bundles whose header does not decode.
    pub bad_bundles: u64,
    /// Mob first groups kept.
    pub first_groups: u64,
    /// Player spawn cards (not mobs).
    pub player_cards: u64,
    /// Whole spawn card groups without an `ALCReplicatedState` or a vitals id: entities that
    /// are not mobs.
    pub cards_without_vitals: u64,
    /// Spawn card groups cut short, by a member without a reader, before their
    /// `ALCReplicatedState` or vitals id.
    pub cut_cards: u64,
    /// Cut spawn card groups by the class of the member without a reader.
    pub cut_classes: BTreeMap<u64, u64>,
    /// Mob deaths kept.
    pub deaths: u64,
    /// Walks that went on after a member without a reader.
    pub resyncs: u64,
    /// Walks that ended before the end of their payload.
    pub lost_tails: u64,
    /// Carrier streams that failed.
    pub stream_failures: u64,
}

impl ScanStats {
    fn add(&mut self, other: &Self) {
        self.bundles += other.bundles;
        self.bad_bundles += other.bad_bundles;
        self.first_groups += other.first_groups;
        self.player_cards += other.player_cards;
        self.cards_without_vitals += other.cards_without_vitals;
        self.cut_cards += other.cut_cards;
        for (class, count) in &other.cut_classes {
            *self.cut_classes.entry(*class).or_default() += count;
        }
        self.deaths += other.deaths;
        self.resyncs += other.resyncs;
        self.lost_tails += other.lost_tails;
        self.stream_failures += other.stream_failures;
    }
}

/// One prefab: a vitals id and the slice asset of the spawn card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PrefabKey {
    /// The `vitalsId` of the vitals state.
    pub vitals_id: u32,
    /// The asset GUID of the spawn card.
    pub asset: [u8; ASSET_ID_LEN],
}

/// The layout of the members of a group, other than the card, the ALC and the vitals state.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MemberForm {
    /// Component and class of each member kept, sorted.
    pub members: Vec<(u64, u64)>,
    /// Classes of the members left out, sorted.
    pub skipped: Vec<u64>,
}

/// The layout of a first group.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FirstForm {
    /// The asset sub id of the spawn card.
    pub sub_id: u32,
    /// The component of the `ALCReplicatedState`.
    pub alc_component: u64,
    /// The component of the vitals state.
    pub vitals_component: u64,
    /// The other members.
    pub rest: MemberForm,
}

/// How often each body of one member was seen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodyCounts(#[serde(with = "pairs")] BTreeMap<Vec<u8>, u32>);

impl BodyCounts {
    fn add(&mut self, body: Vec<u8>, count: u32) {
        if self.0.len() < MAX_BODY_VARIANTS || self.0.contains_key(&body) {
            let seen = self.0.entry(body).or_default();
            *seen = seen.saturating_add(count);
        }
    }

    /// The body seen most often; the smallest one on a tie.
    fn most_common(&self) -> Option<&[u8]> {
        self.0
            .iter()
            .max_by(|left, right| left.1.cmp(right.1).then_with(|| right.0.cmp(left.0)))
            .map(|(body, _)| body.as_slice())
    }
}

/// How often a group form was seen, and the bodies of its members.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormCounts {
    /// Groups of this form.
    pub seen: u32,
    /// One entry per member kept, in the order of [`MemberForm::members`].
    pub bodies: Vec<BodyCounts>,
}

impl FormCounts {
    fn add(&mut self, bodies: Vec<Vec<u8>>) {
        self.seen = self.seen.saturating_add(1);
        self.bodies.resize_with(bodies.len(), BodyCounts::default);
        for (counts, body) in self.bodies.iter_mut().zip(bodies) {
            counts.add(body, 1);
        }
    }

    fn merge(&mut self, other: Self) {
        self.seen = self.seen.saturating_add(other.seen);
        self.bodies
            .resize_with(other.bodies.len(), BodyCounts::default);
        for (counts, other) in self.bodies.iter_mut().zip(other.bodies) {
            for (body, count) in other.0 {
                counts.add(body, count);
            }
        }
    }

    /// The most common body of each member.
    fn bodies(&self) -> Vec<&[u8]> {
        self.bodies
            .iter()
            .map(|counts| counts.most_common().unwrap_or_default())
            .collect()
    }
}

/// The first groups and deaths of one prefab.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefabCounts {
    /// First groups, by form.
    #[serde(with = "pairs")]
    pub first: BTreeMap<FirstForm, FormCounts>,
    /// Death groups, by form.
    #[serde(with = "pairs")]
    pub death: BTreeMap<MemberForm, FormCounts>,
    /// The paperdoll changes right after the card, by form.
    #[serde(with = "pairs")]
    pub drawn: BTreeMap<MemberForm, FormCounts>,
}

impl PrefabCounts {
    fn merge(&mut self, other: Self) {
        for (form, counts) in other.first {
            self.first.entry(form).or_default().merge(counts);
        }
        for (form, counts) in other.death {
            self.death.entry(form).or_default().merge(counts);
        }
        for (form, counts) in other.drawn {
            self.drawn.entry(form).or_default().merge(counts);
        }
    }
}

/// What one ledger gives: the worker's output.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerTally {
    /// The ledger's digest.
    pub digest: LedgerDigest,
    /// What the scan saw.
    pub stats: ScanStats,
    /// The prefabs.
    #[serde(with = "pairs")]
    pub prefabs: BTreeMap<PrefabKey, PrefabCounts>,
}

/// Reads one ledger: every server bundle, in order, with the mob slots of each connection.
pub fn scan_ledger(bytes: &[u8]) -> LedgerTally {
    let mut scan = LedgerScan::default();
    let mut decoder = Decoder::new();
    for record in ledger_records(bytes) {
        let Ok(record) = record else {
            break;
        };
        let output = decoder.record(&record);
        scan.now_ms = record.time_ms;
        if output.new_connection {
            scan.live
                .retain(|(context, _), _| *context != record.context);
        }
        if record.direction == SERVER_TO_CLIENT {
            for delivered in &output.envelopes {
                if delivered.envelope.type_index() == Some(REPLICATED_STATE_BUNDLE_TYPE_INDEX) {
                    scan.bundle(record.context, &delivered.envelope.body);
                }
            }
        }
        if output.error.is_some() {
            scan.tally.stats.stream_failures += 1;
            decoder.stop(record.context, record.direction);
        }
    }
    scan.tally.digest = LedgerDigest::of(bytes);
    scan.tally
}

/// A mob in view.
#[derive(Debug, Clone, Copy)]
struct LiveMob {
    key: PrefabKey,
    dead: bool,
    /// When its card came (ledger ms).
    card_ms: u64,
    /// Its first paperdoll change was seen, or came too late.
    drawn: bool,
}

/// The scan of one ledger.
#[derive(Debug, Default)]
struct LedgerScan {
    /// Mobs in view, by connection context and slot.
    live: HashMap<(u64, u16), LiveMob>,
    tally: LedgerTally,
    /// The time of the record being read (ledger ms).
    now_ms: u64,
}

impl LedgerScan {
    /// One server bundle: its stopped slots first, then its groups.
    fn bundle(&mut self, context: u64, body: &[u8]) {
        let Ok(bundle) = ReplicatedStateBundle::decode(body) else {
            self.tally.stats.bad_bundles += 1;
            return;
        };
        self.tally.stats.bundles += 1;
        if let Some(block) = &bundle.index_block {
            let stops = usize::try_from(block.stop_count).unwrap_or(usize::MAX);
            for interest in block.indexes.iter().take(stops) {
                self.live.remove(&(context, *interest));
            }
        }
        self.payload(context, &bundle.payload);
    }

    /// The groups of one payload; after a member that cannot be read, the walk goes on from the
    /// next group that can be found.
    fn payload(&mut self, context: u64, payload: &[u8]) {
        let mut walk = walk_groups(payload, 0);
        loop {
            for group in &walk.groups {
                self.group(context, payload, group);
            }
            let Some(stop) = walk.stopped_at else {
                return;
            };
            match self.resync(context, payload, stop + 1) {
                Some(next) => {
                    self.tally.stats.resyncs += 1;
                    walk = next;
                }
                None => {
                    self.tally.stats.lost_tails += 1;
                    return;
                }
            }
        }
    }

    /// The first walk from `from` on whose first group starts with a spawn card or belongs to a
    /// mob in view, and is a whole group or a card group.
    fn resync(&self, context: u64, payload: &[u8], from: usize) -> Option<Walk> {
        (from..payload.len()).find_map(|at| {
            let header = group_header(payload, at)?;
            let card = is_card_start(payload, header.members_start);
            if header.count == 0 || !(card || self.live.contains_key(&(context, header.interest))) {
                return None;
            }
            let walk = walk_groups(payload, at);
            let first = walk.groups.first()?;
            (first.unread.is_none() || card).then_some(walk)
        })
    }

    fn group(&mut self, context: u64, payload: &[u8], group: &WalkedGroup) {
        let slot = (context, group.interest);
        if let Some(asset) = group.card_asset() {
            self.live.remove(&slot);
            if asset == PLAYER_SLICE_ASSET {
                self.tally.stats.player_cards += 1;
                return;
            }
            let Some((key, form, bodies)) = first_group(asset, group, payload) else {
                if let Some(class) = group.unread {
                    self.tally.stats.cut_cards += 1;
                    *self.tally.stats.cut_classes.entry(class).or_default() += 1;
                } else {
                    self.tally.stats.cards_without_vitals += 1;
                }
                return;
            };
            self.tally.stats.first_groups += 1;
            let counts = self.tally.prefabs.entry(key).or_default();
            counts.first.entry(form).or_default().add(bodies);
            // Only a fresh spawn draws: a mob already alive came into view as it was.
            let fresh = group.members.iter().any(|member| {
                matches!(&member.state, Some(MemberState::Alc(alc)) if alc.is_fresh_card())
            });
            self.live.insert(
                slot,
                LiveMob {
                    key,
                    dead: false,
                    card_ms: self.now_ms,
                    drawn: !fresh,
                },
            );
            return;
        }
        let Some(mob) = self.live.get_mut(&slot) else {
            return;
        };
        let has_paperdoll = group
            .members
            .iter()
            .any(|member| member.class == PAPERDOLL_TYPE_INDEX);
        if !mob.drawn && has_paperdoll {
            mob.drawn = true;
            if self.now_ms.saturating_sub(mob.card_ms) <= DRAW_WINDOW_MS {
                let key = mob.key;
                let (form, bodies) = other_members(group, payload, |member| {
                    member.class != PAPERDOLL_TYPE_INDEX
                });
                if !form.members.is_empty() {
                    let counts = self.tally.prefabs.entry(key).or_default();
                    counts.drawn.entry(form).or_default().add(bodies);
                }
            }
        }
        let Some(mob) = self.live.get_mut(&slot) else {
            return;
        };
        if mob.dead || !group.sets_zero_health() {
            return;
        }
        mob.dead = true;
        let key = mob.key;
        self.tally.stats.deaths += 1;
        let (form, bodies) = other_members(group, payload, |member| {
            matches!(member.class, ALC_TYPE_INDEX | VITALS_TYPE_INDEX)
        });
        let counts = self.tally.prefabs.entry(key).or_default();
        counts.death.entry(form).or_default().add(bodies);
    }
}

/// The key, form and bodies of a first group, when it has an `ALCReplicatedState` and a vitals
/// state with a `vitalsId`.
fn first_group(
    asset: AssetId,
    group: &WalkedGroup,
    payload: &[u8],
) -> Option<(PrefabKey, FirstForm, Vec<Vec<u8>>)> {
    let alc = group
        .members
        .iter()
        .position(|member| member.class == ALC_TYPE_INDEX)?;
    let (vitals, vitals_id) = group
        .members
        .iter()
        .enumerate()
        .find_map(|(index, member)| match &member.state {
            Some(MemberState::Vitals(vitals)) => Some((index, vitals.slot_0?.vitals_id?)),
            _ => None,
        })?;
    let card = group.members.iter().position(WalkedMember::is_card)?;
    // The ALC and vitals bodies stay with the other members: a server moves the ALC to its own
    // spawn point and writes its own health into the vitals.
    let excluded = [card];
    let mut index = 0;
    let (rest, bodies) = other_members(group, payload, |_| {
        let skip = excluded.contains(&index);
        index += 1;
        skip
    });
    let key = PrefabKey {
        vitals_id,
        asset: asset.id,
    };
    let form = FirstForm {
        sub_id: asset.sub_id,
        alc_component: group.members[alc].component,
        vitals_component: group.members[vitals].component,
        rest,
    };
    Some((key, form, bodies))
}

/// The form and bodies of the members of `group` that `exclude` does not take, sorted by
/// component (retail sends the members of one prefab in varying order). `exclude` sees every
/// member once, in order.
fn other_members(
    group: &WalkedGroup,
    payload: &[u8],
    mut exclude: impl FnMut(&WalkedMember) -> bool,
) -> (MemberForm, Vec<Vec<u8>>) {
    let mut kept = Vec::new();
    let mut skipped: Vec<u64> = group.unread.into_iter().collect();
    for walked in &group.members {
        if exclude(walked) {
            continue;
        }
        match walked.body(payload) {
            Some(body) => kept.push(((walked.component, walked.class), body.to_vec())),
            None => skipped.push(walked.class),
        }
    }
    kept.sort_unstable();
    skipped.sort_unstable();
    skipped.dedup();
    let (members, bodies) = kept.into_iter().unzip();
    (MemberForm { members, skipped }, bodies)
}

/// One member read from a payload.
#[derive(Debug, Clone, PartialEq)]
struct WalkedMember {
    component: u64,
    class: u64,
    /// The state, for a class with a reader in `nw-messages`; `None` for a class that
    /// [`crate::extra_members`] reads.
    state: Option<MemberState>,
    /// Its bytes in the payload.
    range: Range<usize>,
}

impl WalkedMember {
    fn is_card(&self) -> bool {
        self.component == GDE_METADATA_COMPONENT
            && matches!(self.state, Some(MemberState::GdeMetadata(_)))
    }

    /// The body (after the component and class): for an `nw-messages` class, when it decodes
    /// alone with the reader of its class and the member encodes to the same bytes; a member
    /// passed by its class's wire layout keeps its bytes as they are.
    fn body<'a>(&self, payload: &'a [u8]) -> Option<&'a [u8]> {
        let bytes = &payload[self.range.clone()];
        let mut reader = Reader::new(bytes);
        extra_members::read_member_header(&mut reader)?;
        let body = reader.rest();
        let Some(state) = self
            .state
            .as_ref()
            .filter(|state| !matches!(state, MemberState::Recorded { .. }))
        else {
            return Some(body);
        };
        let again = BundleMember {
            component: self.component,
            state: MemberState::decode(state.type_index(), body).ok()?,
        };
        (again.encode().ok()? == bytes).then_some(body)
    }
}

/// A member of a class that [`crate::extra_members`] reads, at `at`.
fn extra_member(payload: &[u8], at: usize) -> Option<WalkedMember> {
    let mut reader = Reader::new(payload.get(at..)?);
    let (component, class) = extra_members::read_member_header(&mut reader)?;
    extra_members::read_body(class, &mut reader)?;
    Some(WalkedMember {
        component,
        class,
        state: None,
        range: at..payload.len() - reader.remaining(),
    })
}

/// The members that `BundlePayload::decode_known` reads from one point of a payload, and the
/// class where it stopped.
struct KnownStream {
    members: std::iter::Peekable<std::vec::IntoIter<WalkedMember>>,
    unread: u64,
}

impl KnownStream {
    /// The members from the group that starts at `at`.
    fn at_group(payload: &[u8], at: usize) -> Option<Self> {
        Self::read(payload, at, &[])
    }

    /// The members from the member at `at`, the next of a group with `left` members to go: a
    /// group integer and the count `left` are put before it for the reader.
    fn in_group(payload: &[u8], at: usize, left: u8) -> Option<Self> {
        let mut header = Writer::new();
        header.prefixed(0).ok()?.u8(left);
        Self::read(payload, at, &header.finish())
    }

    fn read(payload: &[u8], at: usize, header: &[u8]) -> Option<Self> {
        let mut bytes = header.to_vec();
        bytes.extend_from_slice(payload.get(at..)?);
        let known = BundlePayload::decode_known(&bytes).ok()?;
        let unread = known
            .stopped_at
            .or(known.failed.map(|(class, _)| class))
            .map_or(INLINE_UUID_CLASS, |class| match class {
                TypeId::FileSlot(index) => index,
                TypeId::InlineUuid(_) => INLINE_UUID_CLASS,
            });
        let to_payload = |offset: usize| offset - header.len() + at;
        let members: Vec<WalkedMember> = known
            .members
            .into_iter()
            .map(|(member, range)| WalkedMember {
                component: member.component,
                class: member.state.type_index(),
                state: Some(member.state),
                range: to_payload(range.start)..to_payload(range.end),
            })
            .collect();
        Some(Self {
            members: members.into_iter().peekable(),
            unread,
        })
    }

    /// The next member, when it starts at `at`.
    fn next_at(&mut self, at: usize) -> Option<WalkedMember> {
        self.members.next_if(|member| member.range.start == at)
    }
}

/// One group read from a payload.
#[derive(Debug, Clone, PartialEq)]
struct WalkedGroup {
    /// The slot: the low 16 bits of the group integer.
    interest: u16,
    /// The members read, in order.
    members: Vec<WalkedMember>,
    /// The class of the member where the reading stopped, when the group was not read to its
    /// end ([`INLINE_UUID_CLASS`] for an inline UUID class).
    unread: Option<u64>,
}

impl WalkedGroup {
    /// The asset of the spawn card in the group.
    fn card_asset(&self) -> Option<AssetId> {
        self.members
            .iter()
            .filter(|member| member.is_card())
            .find_map(|member| match &member.state {
                Some(MemberState::GdeMetadata(card)) => card.asset,
                _ => None,
            })
    }

    fn sets_zero_health(&self) -> bool {
        self.members.iter().any(|member| match &member.state {
            Some(MemberState::Vitals(vitals)) => vitals
                .slot_0
                .and_then(|slot| slot.health_amount)
                .is_some_and(|health| health <= DEAD_HEALTH),
            _ => false,
        })
    }
}

/// The groups read from one start in a payload.
#[derive(Debug, Clone, PartialEq)]
struct Walk {
    groups: Vec<WalkedGroup>,
    /// The payload offset where the reading stopped before the end.
    stopped_at: Option<usize>,
}

/// Reads the groups of `payload` from offset `from` until its end or a member that cannot be
/// read; the group of that member is kept with the members before it.
fn walk_groups(payload: &[u8], from: usize) -> Walk {
    let Some(mut known) = KnownStream::at_group(payload, from) else {
        return Walk {
            groups: Vec::new(),
            stopped_at: Some(from),
        };
    };
    let mut groups = Vec::new();
    let mut at = from;
    while at < payload.len() {
        let Some(header) = group_header(payload, at) else {
            return Walk {
                groups,
                stopped_at: Some(at),
            };
        };
        at = header.members_start;
        let mut group = WalkedGroup {
            interest: header.interest,
            members: Vec::new(),
            unread: None,
        };
        for index in 0..header.count {
            let member = match known.next_at(at) {
                Some(member) => member,
                None => {
                    let extra = extra_member(payload, at).and_then(|member| {
                        let left = header.count - index - 1;
                        known = KnownStream::in_group(payload, member.range.end, left)?;
                        Some(member)
                    });
                    let Some(member) = extra else {
                        group.unread = Some(known.unread);
                        groups.push(group);
                        return Walk {
                            groups,
                            stopped_at: Some(at),
                        };
                    };
                    member
                }
            };
            at = member.range.end;
            group.members.push(member);
        }
        groups.push(group);
    }
    Walk {
        groups,
        stopped_at: None,
    }
}

/// The integer and member count at the start of a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GroupHeader {
    interest: u16,
    count: u8,
    /// Offset of the first member.
    members_start: usize,
}

fn group_header(payload: &[u8], at: usize) -> Option<GroupHeader> {
    let mut reader = Reader::new(payload.get(at..)?);
    let interest = reader.prefixed().ok()?;
    let count = reader.u8().ok()?;
    Some(GroupHeader {
        interest: u16::try_from(interest & INTEREST_MASK).ok()?,
        count,
        members_start: payload.len() - reader.remaining(),
    })
}

/// Whether the member at `at` is a spawn card by its component and class.
fn is_card_start(payload: &[u8], at: usize) -> bool {
    let Some(bytes) = payload.get(at..) else {
        return false;
    };
    let mut reader = Reader::new(bytes);
    reader.prefixed().ok() == Some(GDE_METADATA_COMPONENT)
        && decode_type_id(reader.rest())
            .is_ok_and(|(class, _)| class == TypeId::FileSlot(GDE_METADATA_TYPE_INDEX))
}

/// One state of a table entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemberEntry {
    /// The component index.
    pub component: u64,
    /// The class type index.
    pub class: u64,
    /// The body bytes, as lowercase hex.
    pub body: String,
}

/// One entry of the table: one vitals id and asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrefabEntry {
    /// The `vitalsId`.
    pub vitals_id: u32,
    /// The vitals row id whose lowercase CRC-32 is `vitals_id`, when the bundle has one.
    pub vitals_name: Option<String>,
    /// The asset GUID, as lowercase hex.
    pub asset: String,
    /// The asset sub id.
    pub sub_id: u32,
    /// The component of the `ALCReplicatedState`.
    pub alc_component: u64,
    /// The component of the vitals state.
    pub vitals_component: u64,
    /// The other states of the most common first group.
    pub first_members: Vec<MemberEntry>,
    /// The other states of the most common death group; empty when no death was seen.
    pub death_members: Vec<MemberEntry>,
    /// The paperdoll states of the most common change right after the card; empty when none was
    /// seen.
    pub drawn_members: Vec<MemberEntry>,
    /// First groups seen.
    pub seen: u32,
    /// Classes of the members left out of `first_members` and `death_members`.
    pub skipped_classes: Vec<u64>,
}

/// The merged tallies of every ledger.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrefabTable {
    /// Ledgers merged.
    pub ledgers: usize,
    /// Ledgers left out because the same bytes were merged before.
    pub duplicates: usize,
    /// What the scans saw.
    pub stats: ScanStats,
    prefabs: BTreeMap<PrefabKey, PrefabCounts>,
    digests: HashSet<LedgerDigest>,
}

impl PrefabTable {
    /// Merges the tally of one ledger, unless a ledger with the same bytes was merged before.
    pub fn add(&mut self, tally: LedgerTally) {
        if !self.digests.insert(tally.digest) {
            self.duplicates += 1;
            return;
        }
        self.ledgers += 1;
        self.stats.add(&tally.stats);
        for (key, counts) in tally.prefabs {
            self.prefabs.entry(key).or_default().merge(counts);
        }
    }

    /// The entries, sorted by vitals id then asset; `names` maps a vitals id to its row id.
    pub fn entries(&self, names: &HashMap<u32, String>) -> Vec<PrefabEntry> {
        self.prefabs
            .iter()
            .filter_map(|(key, counts)| {
                let (first_form, first) = most_common(&counts.first)?;
                let death = most_common(&counts.death);
                let drawn = most_common(&counts.drawn);
                let mut skipped = first_form.rest.skipped.clone();
                if let Some((form, _)) = death {
                    skipped.extend(&form.skipped);
                }
                skipped.sort_unstable();
                skipped.dedup();
                Some(PrefabEntry {
                    vitals_id: key.vitals_id,
                    vitals_name: names.get(&key.vitals_id).cloned(),
                    asset: hex::encode(key.asset),
                    sub_id: first_form.sub_id,
                    alc_component: first_form.alc_component,
                    vitals_component: first_form.vitals_component,
                    first_members: member_entries(&first_form.rest, first),
                    death_members: death
                        .map(|(form, counts)| member_entries(form, counts))
                        .unwrap_or_default(),
                    drawn_members: drawn
                        .map(|(form, counts)| member_entries(form, counts))
                        .unwrap_or_default(),
                    seen: counts
                        .first
                        .values()
                        .fold(0, |seen, form| seen.saturating_add(form.seen)),
                    skipped_classes: skipped,
                })
            })
            .collect()
    }
}

/// The form seen most often; the smallest one on a tie.
fn most_common<F: Ord>(forms: &BTreeMap<F, FormCounts>) -> Option<(&F, &FormCounts)> {
    forms.iter().max_by(|left, right| {
        left.1
            .seen
            .cmp(&right.1.seen)
            .then_with(|| right.0.cmp(left.0))
    })
}

fn member_entries(form: &MemberForm, counts: &FormCounts) -> Vec<MemberEntry> {
    form.members
        .iter()
        .zip(counts.bodies())
        .map(|(&(component, class), body)| MemberEntry {
            component,
            class,
            body: hex::encode(body),
        })
        .collect()
}

/// The row id of each vitals id of `VitalsData`, `VitalsBaseData` and `VitalsLevelVariantData`;
/// the first row wins.
pub fn vitals_names(tables: &Tables) -> HashMap<u32, String> {
    let mut names = HashMap::new();
    for id in vitals_ids(tables) {
        names
            .entry(lowercase_crc32(id))
            .or_insert_with(|| id.to_owned());
    }
    names
}

/// Writes the entries as a JSON array, making the folder when needed.
///
/// # Errors
///
/// Returns [`MobPrefabsError`] when the file cannot be written.
pub fn write_entries(path: &Path, entries: &[PrefabEntry]) -> Result<(), MobPrefabsError> {
    let io = |source| MobPrefabsError::Io {
        path: path.to_path_buf(),
        source,
    };
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(io)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(entries)?).map_err(io)
}

/// Runs `worker` on every source, at most `jobs` at a time, and returns the results in the
/// order of `sources`. A progress line goes to stderr every [`PROGRESS_EVERY`] ledgers.
pub fn run_workers<W>(
    sources: &[LedgerSource],
    jobs: NonZeroUsize,
    worker: W,
) -> Vec<Result<LedgerTally, MobPrefabsError>>
where
    W: Fn(&LedgerSource) -> Result<LedgerTally, MobPrefabsError> + Sync,
{
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let mut results: Vec<(usize, Result<LedgerTally, MobPrefabsError>)> =
        std::thread::scope(|scope| {
            let threads: Vec<_> = (0..jobs.get().min(sources.len()))
                .map(|_| {
                    scope.spawn(|| {
                        let mut mine = Vec::new();
                        loop {
                            let index = next.fetch_add(1, Ordering::Relaxed);
                            let Some(source) = sources.get(index) else {
                                return mine;
                            };
                            mine.push((index, worker(source)));
                            let finished = done.fetch_add(1, Ordering::Relaxed) + 1;
                            if finished.is_multiple_of(PROGRESS_EVERY) {
                                eprintln!("mob-prefabs: {finished} of {} ledgers", sources.len());
                            }
                        }
                    })
                })
                .collect();
            threads
                .into_iter()
                .flat_map(|thread| thread.join().unwrap_or_default())
                .collect()
        });
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, result)| result).collect()
}

/// Runs one ledger in a worker process: `exe` with `global_args`, the worker command and the
/// source, and reads the tally it prints.
///
/// # Errors
///
/// Returns [`MobPrefabsError::Worker`] when the process cannot start, fails, or prints no tally.
pub fn worker_process(
    exe: &Path,
    global_args: &[OsString],
    source: &LedgerSource,
) -> Result<LedgerTally, MobPrefabsError> {
    let failed = |reason: String| MobPrefabsError::Worker {
        source_name: source.to_string(),
        reason,
    };
    let output = Command::new(exe)
        .args(global_args)
        .arg(WORKER_COMMAND)
        .args(source.worker_args())
        .output()
        .map_err(|error| failed(error.to_string()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(failed(format!("{}: {}", output.status, stderr.trim())));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| failed(error.to_string()))
}

/// Serde form of a map whose keys are not text: a list of key and value pairs.
mod pairs {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(super) fn serialize<K: Serialize, V: Serialize, S: Serializer>(
        map: &BTreeMap<K, V>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(map.iter())
    }

    pub(super) fn deserialize<'de, K, V, D>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
    where
        K: Deserialize<'de> + Ord,
        V: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        Ok(Vec::<(K, V)>::deserialize(deserializer)?
            .into_iter()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use nw_capture::test_support::{envelope, record, server_datagram};
    use nw_messages::{
        AlcState, DYNAMIC_SLICE_SUB_ID, DamageReceiverState, DelayedEventState,
        GatherableControllerState, GdeMetadataReplicatedState, InputPosition, VitalsSlot0,
        VitalsState, Writer,
    };
    use nw_reflect::encode_type_id;

    use super::*;

    const SAILOR_VITALS: u32 = 2_288_616_137;
    const SAILOR_NAME: &str = "Corrupted_Sailor";
    const SAILOR_ASSET: &str = "b7e33b294bcf57b8a476abdc3a31fae7";
    const CONTEXT: u64 = 1;
    const INTEREST: u8 = 17;
    const OTHER_INTEREST: u8 = 5;
    const ALC_COMPONENT: u64 = 5;
    const VITALS_COMPONENT: u64 = 9;
    const DAMAGE_COMPONENT: u64 = 3;
    const GATHERABLE_COMPONENT: u64 = 6;
    const EVENT_COMPONENT: u64 = 17;
    const HEALTH: f32 = 120.0;
    /// `StatusEffects`: a class without a reader here.
    const UNREAD_CLASS: u64 = 4236;
    const UNREAD_BODY: [u8; 3] = [1, 2, 3];
    const BUNDLE_SEQUENCE: u64 = 1;

    fn asset() -> AssetId {
        AssetId {
            id: hex::decode(SAILOR_ASSET)
                .expect("hex")
                .try_into()
                .expect("16 bytes"),
            sub_id: DYNAMIC_SLICE_SUB_ID,
        }
    }

    fn member(component: u64, state: MemberState) -> BundleMember {
        BundleMember { component, state }
    }

    fn card(asset: AssetId) -> BundleMember {
        member(
            GDE_METADATA_COMPONENT,
            MemberState::GdeMetadata(GdeMetadataReplicatedState {
                asset: Some(asset),
                gde_ref: None,
            }),
        )
    }

    fn alc() -> BundleMember {
        let at = InputPosition::from_metres(1.0, 2.0, 3.0).expect("finite");
        member(ALC_COMPONENT, MemberState::Alc(AlcState::at_position(at)))
    }

    fn vitals(health: f32) -> BundleMember {
        member(
            VITALS_COMPONENT,
            MemberState::Vitals(VitalsState {
                slot_0: Some(VitalsSlot0 {
                    health_amount: Some(health),
                    vitals_id: Some(SAILOR_VITALS),
                    ..VitalsSlot0::default()
                }),
                slot_1: None,
            }),
        )
    }

    fn damage_receiver() -> BundleMember {
        member(
            DAMAGE_COMPONENT,
            MemberState::DamageReceiver(DamageReceiverState {
                is_block_active: Some(false),
                ..DamageReceiverState::default()
            }),
        )
    }

    fn gatherable() -> BundleMember {
        member(
            GATHERABLE_COMPONENT,
            MemberState::GatherableController(GatherableControllerState {
                gatherable_state: Some(1),
            }),
        )
    }

    fn delayed_event(completed: bool) -> BundleMember {
        member(
            EVENT_COMPONENT,
            MemberState::DelayedEvent(DelayedEventState {
                total_delay: Some(7),
                completed: Some(completed),
            }),
        )
    }

    fn bytes(member: &BundleMember) -> Vec<u8> {
        member.encode().expect("member")
    }

    /// A member of a class without a reader.
    fn unread_member(component: u64) -> Vec<u8> {
        let mut writer = Writer::new();
        writer
            .prefixed(component)
            .expect("component")
            .bytes(&encode_type_id(&TypeId::FileSlot(UNREAD_CLASS)).expect("class"))
            .bytes(&UNREAD_BODY);
        writer.finish()
    }

    /// The body of `member`: its bytes after the component and class.
    fn body_hex(member: &BundleMember) -> String {
        let mut header = Writer::new();
        header
            .prefixed(member.component)
            .expect("component")
            .bytes(&encode_type_id(&member.state.class()).expect("class"));
        hex::encode(&bytes(member)[header.finish().len()..])
    }

    fn payload(groups: &[(u8, Vec<Vec<u8>>)]) -> Vec<u8> {
        let mut writer = Writer::new();
        for (interest, members) in groups {
            writer
                .prefixed(u64::from(*interest))
                .expect("interest")
                .u8(u8::try_from(members.len()).expect("few"));
            for member in members {
                writer.bytes(member);
            }
        }
        writer.finish()
    }

    /// A sailor first group, its members not in component order.
    fn first_group() -> Vec<Vec<u8>> {
        [
            card(asset()),
            delayed_event(false),
            damage_receiver(),
            alc(),
            gatherable(),
            vitals(HEALTH),
        ]
        .iter()
        .map(bytes)
        .collect()
    }

    fn entries_of(scan: LedgerScan) -> Vec<PrefabEntry> {
        let mut table = PrefabTable::default();
        table.add(scan.tally);
        let names = HashMap::from([(SAILOR_VITALS, SAILOR_NAME.to_owned())]);
        table.entries(&names)
    }

    fn entry(component: u64, member: &BundleMember) -> MemberEntry {
        MemberEntry {
            component,
            class: member.state.type_index(),
            body: body_hex(member),
        }
    }

    #[test]
    fn a_first_group_gives_an_entry_with_the_other_members_by_component() {
        let mut scan = LedgerScan::default();
        scan.payload(CONTEXT, &payload(&[(INTEREST, first_group())]));
        assert_eq!(
            entries_of(scan),
            vec![PrefabEntry {
                vitals_id: SAILOR_VITALS,
                vitals_name: Some(SAILOR_NAME.to_owned()),
                asset: SAILOR_ASSET.to_owned(),
                sub_id: DYNAMIC_SLICE_SUB_ID,
                alc_component: ALC_COMPONENT,
                vitals_component: VITALS_COMPONENT,
                first_members: vec![
                    entry(DAMAGE_COMPONENT, &damage_receiver()),
                    entry(ALC_COMPONENT, &alc()),
                    entry(GATHERABLE_COMPONENT, &gatherable()),
                    entry(VITALS_COMPONENT, &vitals(HEALTH)),
                    entry(EVENT_COMPONENT, &delayed_event(false)),
                ],
                death_members: Vec::new(),
                drawn_members: Vec::new(),
                seen: 1,
                skipped_classes: Vec::new(),
            }]
        );
    }

    #[test]
    fn the_first_zero_health_group_of_the_slot_is_the_death() {
        let mut scan = LedgerScan::default();
        scan.payload(CONTEXT, &payload(&[(INTEREST, first_group())]));
        let death = vec![bytes(&vitals(DEAD_HEALTH)), bytes(&delayed_event(true))];
        scan.payload(CONTEXT, &payload(&[(INTEREST, death.clone())]));
        scan.payload(CONTEXT, &payload(&[(INTEREST, death)]));
        assert_eq!(scan.tally.stats.deaths, 1);
        let entries = entries_of(scan);
        assert_eq!(
            entries[0].death_members,
            vec![entry(EVENT_COMPONENT, &delayed_event(true))]
        );
    }

    /// Retail sailor 1's public paperdoll change 102 ms after its card (newchar-retail-2026-09-18,
    /// 16421 ms): its main hand drawn.
    const DRAWN_BODY: &str = "04030000000800000000000001000000";
    const PAPERDOLL_COMPONENT: u64 = 15;

    fn drawn() -> BundleMember {
        let body = hex::decode(DRAWN_BODY).expect("hex");
        member(
            PAPERDOLL_COMPONENT,
            MemberState::decode(PAPERDOLL_TYPE_INDEX, &body).expect("a paperdoll"),
        )
    }

    #[test]
    fn the_first_paperdoll_change_after_the_card_is_the_draw() {
        let mut scan = LedgerScan::default();
        scan.payload(CONTEXT, &payload(&[(INTEREST, first_group())]));
        scan.now_ms = DRAW_WINDOW_MS;
        scan.payload(
            CONTEXT,
            &payload(&[(INTEREST, vec![bytes(&drawn()), bytes(&alc())])]),
        );
        let entries = entries_of(scan);
        assert_eq!(
            entries[0].drawn_members,
            vec![MemberEntry {
                component: PAPERDOLL_COMPONENT,
                class: PAPERDOLL_TYPE_INDEX,
                body: DRAWN_BODY.to_owned(),
            }]
        );
        let mut late = LedgerScan::default();
        late.payload(CONTEXT, &payload(&[(INTEREST, first_group())]));
        late.now_ms = DRAW_WINDOW_MS + 1;
        late.payload(CONTEXT, &payload(&[(INTEREST, vec![bytes(&drawn())])]));
        assert!(entries_of(late)[0].drawn_members.is_empty(), "too late");
    }

    #[test]
    fn a_stopped_slot_has_no_death() {
        let mut scan = LedgerScan::default();
        scan.payload(CONTEXT, &payload(&[(INTEREST, first_group())]));
        let stop = ReplicatedStateBundle::of_groups(
            BUNDLE_SEQUENCE,
            0,
            vec![u16::from(INTEREST)],
            Vec::new(),
        )
        .and_then(|bundle| bundle.encode())
        .expect("bundle");
        scan.bundle(CONTEXT, &stop);
        scan.payload(
            CONTEXT,
            &payload(&[(INTEREST, vec![bytes(&vitals(DEAD_HEALTH))])]),
        );
        assert_eq!(scan.tally.stats.deaths, 0);
    }

    #[test]
    fn a_card_group_after_a_member_without_a_reader_is_found() {
        let mut scan = LedgerScan::default();
        scan.payload(
            CONTEXT,
            &payload(&[
                (OTHER_INTEREST, vec![unread_member(ALC_COMPONENT)]),
                (INTEREST, first_group()),
            ]),
        );
        assert_eq!(scan.tally.stats.resyncs, 1);
        assert_eq!(scan.tally.stats.first_groups, 1);
        assert_eq!(entries_of(scan)[0].first_members.len(), 5);
    }

    #[test]
    fn a_member_without_a_reader_is_a_skipped_class() {
        let mut group = first_group();
        group.push(unread_member(EVENT_COMPONENT + 1));
        let mut scan = LedgerScan::default();
        scan.payload(CONTEXT, &payload(&[(INTEREST, group)]));
        let entries = entries_of(scan);
        assert_eq!(entries[0].skipped_classes, vec![UNREAD_CLASS]);
        assert_eq!(entries[0].first_members.len(), 5);
    }

    #[test]
    fn a_member_read_by_extra_members_keeps_its_capture_body() {
        const BOSS_COMPONENT: u64 = 14;
        /// Slot 0, children 0 and 1: phase 0 and a flag of 1.
        const BOSS_BODY: [u8; 7] = [0x01, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01];
        let mut boss = Writer::new();
        boss.prefixed(BOSS_COMPONENT)
            .expect("component")
            .bytes(
                &encode_type_id(&TypeId::FileSlot(extra_members::BOSS_PHASE_TYPE_INDEX))
                    .expect("class"),
            )
            .bytes(&BOSS_BODY);
        let mut group = first_group();
        group.insert(1, boss.finish());
        let mut scan = LedgerScan::default();
        scan.payload(CONTEXT, &payload(&[(INTEREST, group)]));
        let entries = entries_of(scan);
        assert!(entries[0].skipped_classes.is_empty());
        assert_eq!(entries[0].first_members.len(), 6);
        assert!(entries[0].first_members.contains(&MemberEntry {
            component: BOSS_COMPONENT,
            class: extra_members::BOSS_PHASE_TYPE_INDEX,
            body: hex::encode(BOSS_BODY),
        }));
    }

    #[test]
    fn a_player_card_is_not_a_mob() {
        let mut group = first_group();
        group[0] = bytes(&card(PLAYER_SLICE_ASSET));
        let mut scan = LedgerScan::default();
        scan.payload(CONTEXT, &payload(&[(INTEREST, group)]));
        assert_eq!(scan.tally.stats.player_cards, 1);
        assert!(entries_of(scan).is_empty());
    }

    #[test]
    fn ledgers_merge_once_and_the_most_common_body_wins() {
        let tally = |events: &[bool], crc| {
            let mut scan = LedgerScan::default();
            for completed in events {
                let mut group = first_group();
                group[1] = bytes(&delayed_event(*completed));
                scan.payload(CONTEXT, &payload(&[(INTEREST, group)]));
            }
            scan.tally.digest = LedgerDigest { len: 1, crc };
            scan.tally
        };
        let mut table = PrefabTable::default();
        table.add(tally(&[true], 1));
        table.add(tally(&[false, false], 2));
        table.add(tally(&[true, true, true], 2));
        assert_eq!((table.ledgers, table.duplicates), (2, 1));
        let entries = table.entries(&HashMap::new());
        assert_eq!(entries[0].seen, 3);
        assert_eq!(entries[0].vitals_name, None);
        assert_eq!(
            entries[0].first_members[4],
            entry(EVENT_COMPONENT, &delayed_event(false))
        );
    }

    #[test]
    fn a_ledger_tally_survives_json() {
        let bundle = ReplicatedStateBundle::of_groups(BUNDLE_SEQUENCE, 0, Vec::new(), Vec::new())
            .expect("bundle");
        let body = ReplicatedStateBundle {
            payload: payload(&[(INTEREST, first_group())]),
            ..bundle
        }
        .encode()
        .expect("bundle");
        let ledger = record(
            SERVER_TO_CLIENT,
            &server_datagram(&[envelope(REPLICATED_STATE_BUNDLE_TYPE_INDEX, body)]),
        );
        let tally = scan_ledger(&ledger);
        assert_eq!(tally.stats.first_groups, 1);
        assert_eq!(tally.digest, LedgerDigest::of(&ledger));
        let text = serde_json::to_string(&tally).expect("json");
        assert_eq!(
            serde_json::from_str::<LedgerTally>(&text).expect("tally"),
            tally
        );
    }

    #[test]
    fn ledgers_are_found_in_folders_and_zip_archives() {
        const LEDGER: &[u8] = b"ledger bytes";
        const ZIP_ENTRY: &str = "session/ledger.bin";
        let dir = tempfile::tempdir().expect("dir");
        let folder = dir.path().join("plain");
        std::fs::create_dir(&folder).expect("folder");
        std::fs::write(folder.join(LEDGER_FILE_NAME), LEDGER).expect("ledger");
        std::fs::write(dir.path().join("old.rar"), b"").expect("rar");
        let archive = dir.path().join("session.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).expect("zip"));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file(ZIP_ENTRY, options).expect("entry");
        zip.write_all(LEDGER).expect("write");
        zip.start_file("session/metadata.json", options)
            .expect("entry");
        zip.finish().expect("finish");
        let list = find_ledger_sources(&[dir.path().to_path_buf()]).expect("list");
        assert_eq!(list.rar_archives, 1);
        assert_eq!(list.sources.len(), 2);
        for source in &list.sources {
            assert_eq!(source.read().expect("read"), LEDGER);
        }
        assert!(
            list.sources
                .iter()
                .any(|source| source.entry.as_deref() == Some(ZIP_ENTRY))
        );
    }
}
