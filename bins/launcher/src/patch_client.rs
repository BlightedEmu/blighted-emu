// SPDX-License-Identifier: GPL-3.0-or-later
//! Applies the Amazon-account-link patch to the patched client copy before
//! each launch, with no external interpreter. The launcher runs this on
//! Windows and on Linux, so an older [`BlightedNewWorld.exe`] that was
//! written by a version of `scripts/patch-client.py` without the shadow
//! account fix still has its Amazon Games iD pop-up turned off at play.
//!
//! The full first-time patch (certificate bypass, exit to desktop, offline
//! Steam ticket, typed chat, Amazon account link and the optional AWS proxy
//! sites) stays in `scripts/patch-client.py`. This module mirrors only the
//! fifth edit (`SHADOW_ACCOUNT_CHECK`): find the one site where the Omni
//! shadow-account check returns `MOV AL,1`, flip the one immediate byte to
//! `0`, and log the result.
//!
//! Keep the signature bytes and offsets in this file in sync with
//! `scripts/patch-client.py` (`SHADOW_ACCOUNT_CHECK` and `NOT_SHADOW`).

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Head of the `SHADOW_ACCOUNT_CHECK` signature:
///   `PUSH RBX` / `SUB RSP,0x20` / `MOV RBX,RCX` / `CALL rel32` opcode.
pub const SHADOW_HEAD: &[u8] = &[0x40, 0x53, 0x48, 0x83, 0xEC, 0x20, 0x48, 0x8B, 0xD9, 0xE8];
/// After the call target: `TEST EAX,EAX` / `JNZ 0x10` / `CMP [RBX+disp32],EAX`.
pub const SHADOW_MID_A: &[u8] = &[0x85, 0xC0, 0x75, 0x10, 0x39, 0x83];
/// After the CMP displacement: `JNZ 0x08` / `MOV AL,` opcode.
pub const SHADOW_MID_B: &[u8] = &[0x75, 0x08, 0xB0];
/// After the MOV immediate: `ADD RSP,0x20` / `POP RBX` / `RET` /
/// `XOR AL,AL` / `ADD RSP,0x20` / `POP RBX` / `RET`.
pub const SHADOW_TAIL: &[u8] = &[
    0x48, 0x83, 0xC4, 0x20, 0x5B, 0xC3, 0x32, 0xC0, 0x48, 0x83, 0xC4, 0x20, 0x5B, 0xC3,
];
/// Wildcard length of the `CALL rel32` offset after the head.
const CALL_REL32_LEN: usize = 4;
/// Wildcard length of the `CMP` displacement after mid-A.
const CMP_DISP32_LEN: usize = 4;
/// Length of the MOV AL,imm8 immediate the patch flips (one byte).
const YES_LEN: usize = 1;
/// The yes-byte offset from the start of a match.
const YES_OFFSET_IN_MATCH: usize =
    SHADOW_HEAD.len() + CALL_REL32_LEN + SHADOW_MID_A.len() + CMP_DISP32_LEN + SHADOW_MID_B.len();
/// Full signature length.
const PATTERN_LEN: usize = YES_OFFSET_IN_MATCH + YES_LEN + SHADOW_TAIL.len();
/// Unpatched yes-byte: `MOV AL,1` (the retail client).
pub const SHADOW_YES: u8 = 0x01;
/// Patched yes-byte: `MOV AL,0` (the emulator).  Mirrors
/// `scripts/patch-client.py` constant `NOT_SHADOW`.
pub const SHADOW_NO: u8 = 0x00;

/// Marker file name next to the patched exe. The file records the exe's
/// size and mtime at the time the launcher last confirmed the patch, so a
/// repeat launch of the same file can skip the rescan.
pub const PATCH_MARKER_NAME: &str = ".blighted.shadow.patched";

/// Why the account-link patch pass could not finish.
#[derive(Debug, thiserror::Error)]
pub enum PatchError {
    /// The patched exe is not at `path`.
    #[error("the patched client is not at {}", .0.display())]
    Missing(PathBuf),
    /// The patched exe could not be read.
    #[error("could not read {}: {source}", .path.display())]
    Read {
        /// The file that failed.
        path: PathBuf,
        /// The read error.
        #[source]
        source: std::io::Error,
    },
    /// The yes-byte could not be written.
    #[error("could not write {}: {source}", .path.display())]
    Write {
        /// The file that failed.
        path: PathBuf,
        /// The write error.
        #[source]
        source: std::io::Error,
    },
    /// No site matched. The client build may have changed, or the file is
    /// not a copy produced by `scripts/patch-client.py`.
    #[error(
        "the Amazon-account-link patch site was not found in {}: run scripts/patch-client.py \
         against a fresh NewWorld.exe to make a patched copy that matches this client build",
        .0.display()
    )]
    NoSite(PathBuf),
    /// More than one site matched.  The file is not a readable patched exe.
    #[error("the Amazon-account-link patch site was found more than once in {}", .0.display())]
    ManySites(PathBuf),
    /// The yes-byte at the one site is neither 0 nor 1.
    #[error(
        "the Amazon-account-link site in {} has an unexpected immediate 0x{byte:02X}: the \
         client build may have changed",
        .path.display()
    )]
    UnexpectedByte {
        /// The file.
        path: PathBuf,
        /// The byte read.
        byte: u8,
    },
}

/// What happened in a patch pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchOutcome {
    /// The marker said the file was already known patched; no bytes read.
    SkippedByMarker,
    /// The site was found and its yes-byte was already 0.
    AlreadyPatched,
    /// The site's yes-byte was 1 and has been flipped to 0.
    Patched,
}

/// Find in `data` the single site that matches the `SHADOW_ACCOUNT_CHECK`
/// signature with any yes-byte. Returns `(file offset of the yes-byte,
/// yes-byte value)`.
pub fn find_shadow_site(data: &[u8]) -> Result<(usize, u8), FindError> {
    if data.len() < PATTERN_LEN {
        return Err(FindError::NotFound);
    }
    let mut hits: Vec<(usize, u8)> = Vec::new();
    let last = data.len() - PATTERN_LEN;
    // Scan for the head. The two-byte prefix `40 53` and the fixed stride
    // of the signature already reject nearly every address, so a plain
    // byte-by-byte loop is fast enough for a ~1 GB file.
    let mut cursor = 0usize;
    while cursor <= last {
        if data[cursor..cursor + SHADOW_HEAD.len()] != *SHADOW_HEAD {
            cursor += 1;
            continue;
        }
        let mid_a_start = cursor + SHADOW_HEAD.len() + CALL_REL32_LEN;
        let mid_b_start = mid_a_start + SHADOW_MID_A.len() + CMP_DISP32_LEN;
        let yes_offset = cursor + YES_OFFSET_IN_MATCH;
        let tail_start = yes_offset + YES_LEN;
        if data[mid_a_start..mid_a_start + SHADOW_MID_A.len()] == *SHADOW_MID_A
            && data[mid_b_start..mid_b_start + SHADOW_MID_B.len()] == *SHADOW_MID_B
            && data[tail_start..tail_start + SHADOW_TAIL.len()] == *SHADOW_TAIL
        {
            hits.push((yes_offset, data[yes_offset]));
        }
        cursor += 1;
    }
    match hits.len() {
        0 => Err(FindError::NotFound),
        1 => Ok(hits[0]),
        _ => Err(FindError::Repeated),
    }
}

/// Why `find_shadow_site` could not pick one site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindError {
    /// No site matched.
    NotFound,
    /// More than one site matched.
    Repeated,
}

/// The marker's size and mtime fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MarkerFacts {
    /// File length in bytes.
    size: u64,
    /// Modified time as whole seconds since the Unix epoch.
    mtime_secs: i64,
}

impl MarkerFacts {
    fn of(file: &Path) -> Option<Self> {
        let meta = std::fs::metadata(file).ok()?;
        let mtime = meta.modified().ok()?;
        let mtime_secs = match mtime.duration_since(UNIX_EPOCH) {
            Ok(positive) => i64::try_from(positive.as_secs()).ok()?,
            Err(error) => -i64::try_from(error.duration().as_secs()).ok()?,
        };
        Some(Self {
            size: meta.len(),
            mtime_secs,
        })
    }

    fn write_to(&self, path: &Path) -> std::io::Result<()> {
        let mut file = File::create(path)?;
        writeln!(file, "{}", self.size)?;
        writeln!(file, "{}", self.mtime_secs)?;
        file.sync_all()
    }

    fn read_from(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        let mut lines = text.lines();
        let size: u64 = lines.next()?.trim().parse().ok()?;
        let mtime_secs: i64 = lines.next()?.trim().parse().ok()?;
        Some(Self { size, mtime_secs })
    }
}

/// Marker file path for `exe`.
pub fn marker_path(exe: &Path) -> PathBuf {
    exe.with_file_name(PATCH_MARKER_NAME)
}

/// Find the single `SHADOW_ACCOUNT_CHECK` site in the patched `exe` and, if
/// its yes-byte is 1, flip it to 0.  Idempotent: a repeat call on the same
/// file is a no-op after the first.  A marker file next to `exe` records
/// the file's size and mtime at the time of the last confirmed patch, so a
/// repeat launch of the same unchanged exe skips the scan.
pub fn apply_shadow_patch(exe: &Path) -> Result<PatchOutcome, PatchError> {
    if !exe.is_file() {
        return Err(PatchError::Missing(exe.to_path_buf()));
    }
    let marker = marker_path(exe);
    let facts_now = MarkerFacts::of(exe).ok_or_else(|| PatchError::Read {
        path: exe.to_path_buf(),
        source: std::io::Error::other("the patched client has no size or mtime"),
    })?;
    if let Some(recorded) = MarkerFacts::read_from(&marker)
        && recorded == facts_now
    {
        eprintln!(
            "patch: {} is already patched (marker {})",
            exe.display(),
            marker.display()
        );
        return Ok(PatchOutcome::SkippedByMarker);
    }
    let data = read_all(exe)?;
    let (site, yes) = match find_shadow_site(&data) {
        Ok(pair) => pair,
        Err(FindError::NotFound) => return Err(PatchError::NoSite(exe.to_path_buf())),
        Err(FindError::Repeated) => return Err(PatchError::ManySites(exe.to_path_buf())),
    };
    eprintln!(
        "patch: {} has the Amazon-account-link site at file offset 0x{:X}, immediate 0x{:02X}",
        exe.display(),
        site,
        yes
    );
    let outcome = match yes {
        SHADOW_NO => PatchOutcome::AlreadyPatched,
        SHADOW_YES => {
            write_byte(exe, site, SHADOW_NO)?;
            eprintln!(
                "patch: {} wrote 0x{:02X} at file offset 0x{:X} (1 byte flipped)",
                exe.display(),
                SHADOW_NO,
                site
            );
            PatchOutcome::Patched
        }
        byte => {
            return Err(PatchError::UnexpectedByte {
                path: exe.to_path_buf(),
                byte,
            });
        }
    };
    // Record the file's new size and mtime so the next launch can skip.
    let facts_after = MarkerFacts::of(exe).unwrap_or(facts_now);
    if let Err(error) = facts_after.write_to(&marker) {
        eprintln!(
            "patch: could not write marker {}: {} (the next launch will rescan)",
            marker.display(),
            error
        );
    }
    Ok(outcome)
}

fn read_all(exe: &Path) -> Result<Vec<u8>, PatchError> {
    let mut file = File::open(exe).map_err(|source| PatchError::Read {
        path: exe.to_path_buf(),
        source,
    })?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .map_err(|source| PatchError::Read {
            path: exe.to_path_buf(),
            source,
        })?;
    Ok(buffer)
}

fn write_byte(exe: &Path, offset: usize, value: u8) -> Result<(), PatchError> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(exe)
        .map_err(|source| PatchError::Write {
            path: exe.to_path_buf(),
            source,
        })?;
    let seek_to = u64::try_from(offset).map_err(|_| PatchError::Write {
        path: exe.to_path_buf(),
        source: std::io::Error::other("file offset does not fit in u64"),
    })?;
    file.seek(SeekFrom::Start(seek_to))
        .map_err(|source| PatchError::Write {
            path: exe.to_path_buf(),
            source,
        })?;
    file.write_all(&[value])
        .map_err(|source| PatchError::Write {
            path: exe.to_path_buf(),
            source,
        })?;
    file.sync_all().map_err(|source| PatchError::Write {
        path: exe.to_path_buf(),
        source,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a byte buffer that holds one SHADOW_ACCOUNT_CHECK match with
    /// `yes` as the immediate, surrounded by `lead` and `trail` padding.
    fn sample(lead: usize, trail: usize, yes: u8) -> Vec<u8> {
        let mut buffer = vec![0xAA_u8; lead];
        buffer.extend_from_slice(SHADOW_HEAD);
        buffer.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]); // CALL rel32
        buffer.extend_from_slice(SHADOW_MID_A);
        buffer.extend_from_slice(&[0x5C, 0x02, 0x00, 0x00]); // CMP disp32
        buffer.extend_from_slice(SHADOW_MID_B);
        buffer.push(yes);
        buffer.extend_from_slice(SHADOW_TAIL);
        buffer.extend(std::iter::repeat_n(0xBB_u8, trail));
        buffer
    }

    #[test]
    fn find_shadow_site_locates_one_match_and_reports_yes() {
        let data = sample(128, 128, SHADOW_YES);
        let (offset, yes) = find_shadow_site(&data).expect("site");
        assert_eq!(yes, SHADOW_YES);
        assert_eq!(data[offset], SHADOW_YES);
        assert_eq!(offset, 128 + YES_OFFSET_IN_MATCH);
    }

    #[test]
    fn find_shadow_site_reports_already_patched_match() {
        let data = sample(64, 64, SHADOW_NO);
        let (offset, yes) = find_shadow_site(&data).expect("site");
        assert_eq!(yes, SHADOW_NO);
        assert_eq!(data[offset], SHADOW_NO);
    }

    #[test]
    fn find_shadow_site_errors_when_there_is_no_match() {
        assert_eq!(find_shadow_site(&[0_u8; 1024]), Err(FindError::NotFound));
    }

    #[test]
    fn find_shadow_site_errors_when_there_are_many_matches() {
        let mut data = sample(32, 32, SHADOW_YES);
        let more = sample(32, 32, SHADOW_YES);
        data.extend(more);
        assert_eq!(find_shadow_site(&data), Err(FindError::Repeated));
    }

    #[test]
    fn apply_shadow_patch_flips_the_yes_byte_once_and_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let exe = dir.path().join("BlightedNewWorld.exe");
        std::fs::write(&exe, sample(1024, 1024, SHADOW_YES)).expect("exe");
        let first = apply_shadow_patch(&exe).expect("first pass");
        assert_eq!(first, PatchOutcome::Patched);
        let after = std::fs::read(&exe).expect("read");
        let (_, yes) = find_shadow_site(&after).expect("site");
        assert_eq!(yes, SHADOW_NO);
        let second = apply_shadow_patch(&exe).expect("second pass");
        assert_eq!(second, PatchOutcome::SkippedByMarker);
    }

    #[test]
    fn apply_shadow_patch_reports_already_patched_when_byte_is_zero_and_no_marker() {
        let dir = tempfile::tempdir().expect("tempdir");
        let exe = dir.path().join("BlightedNewWorld.exe");
        std::fs::write(&exe, sample(128, 128, SHADOW_NO)).expect("exe");
        assert_eq!(
            apply_shadow_patch(&exe).expect("pass"),
            PatchOutcome::AlreadyPatched
        );
    }

    #[test]
    fn apply_shadow_patch_errors_when_the_site_is_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let exe = dir.path().join("BlightedNewWorld.exe");
        std::fs::write(&exe, [0_u8; 4096]).expect("exe");
        assert!(matches!(
            apply_shadow_patch(&exe),
            Err(PatchError::NoSite(path)) if path == exe
        ));
    }

    #[test]
    fn apply_shadow_patch_errors_when_the_file_is_missing() {
        let missing = PathBuf::from("/does/not/exist/BlightedNewWorld.exe");
        assert!(matches!(
            apply_shadow_patch(&missing),
            Err(PatchError::Missing(path)) if path == missing
        ));
    }

    #[test]
    fn apply_shadow_patch_errors_on_an_unexpected_immediate() {
        let dir = tempfile::tempdir().expect("tempdir");
        let exe = dir.path().join("BlightedNewWorld.exe");
        std::fs::write(&exe, sample(32, 32, 0x7F)).expect("exe");
        assert!(matches!(
            apply_shadow_patch(&exe),
            Err(PatchError::UnexpectedByte { byte: 0x7F, path }) if path == exe
        ));
    }

    #[test]
    fn apply_shadow_patch_rescans_when_the_file_changed_after_the_marker() {
        let dir = tempfile::tempdir().expect("tempdir");
        let exe = dir.path().join("BlightedNewWorld.exe");
        std::fs::write(&exe, sample(64, 64, SHADOW_YES)).expect("exe");
        assert_eq!(
            apply_shadow_patch(&exe).expect("first"),
            PatchOutcome::Patched
        );
        // Simulate a client update: write a new file (new mtime, same
        // size) whose yes-byte is 1 again.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(&exe, sample(64, 64, SHADOW_YES)).expect("new exe");
        assert_eq!(
            apply_shadow_patch(&exe).expect("rescan"),
            PatchOutcome::Patched
        );
    }

    #[test]
    fn pattern_offsets_match_the_signature_in_scripts_patch_client_py() {
        // Python: len = 2+4+3+1 + 4 + 2+2+2 + 4 + 2+1 + 1 + 4+1+1+2+4+1+1
        assert_eq!(PATTERN_LEN, 42);
        assert_eq!(YES_OFFSET_IN_MATCH, 27);
    }
}
