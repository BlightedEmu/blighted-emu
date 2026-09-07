// SPDX-License-Identifier: AGPL-3.0-or-later
//! CPU per program from `/proc/<pid>/stat` (admin tools spec section 5.2), as % of one core
//! between two samples. Linux only: without `/proc` every value is unknown.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where Linux shows its processes.
pub const PROC_DIR: &str = "/proc";
/// The status file of a process.
pub const STAT_FILE: &str = "stat";
/// `utime` and `stime` count in `USER_HZ` ticks, fixed at 100 per second for `/proc`.
pub const CLOCK_TICKS_PER_SECOND: f64 = 100.0;
const PERCENT: f64 = 100.0;
const MS_PER_SECOND: f64 = 1_000.0;
/// Fields after the name, from 0 (the state): `utime` is field 11 and `stime` field 12.
const UTIME_FIELD: usize = 11;
const STIME_FIELD: usize = 12;

/// The CPU ticks of every process at one time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CpuSample {
    /// Unix milliseconds of the sample.
    pub at_ms: u64,
    /// Name and `utime + stime` of each process id.
    pub ticks: BTreeMap<u32, (String, u64)>,
}

/// The name and `utime + stime` of a `/proc/<pid>/stat` text. The name is between the first
/// `(` and the last `)`, so it may hold spaces and brackets.
pub fn parse_stat(text: &str) -> Option<(String, u64)> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let name = text.get(open + 1..close)?;
    let fields: Vec<&str> = text.get(close + 1..)?.split_whitespace().collect();
    let utime: u64 = fields.get(UTIME_FIELD)?.parse().ok()?;
    let stime: u64 = fields.get(STIME_FIELD)?.parse().ok()?;
    Some((name.to_owned(), utime.checked_add(stime)?))
}

/// Every process of `proc_dir` at `at_ms`; folders that are not a process id, and processes
/// that ended while read, are skipped.
///
/// # Errors
///
/// When `proc_dir` cannot be listed.
pub fn read_sample(proc_dir: &Path, at_ms: u64) -> std::io::Result<CpuSample> {
    let mut ticks = BTreeMap::new();
    for entry in std::fs::read_dir(proc_dir)?.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse().ok())
        else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(entry.path().join(STAT_FILE)) else {
            continue;
        };
        if let Some(process) = parse_stat(&text) {
            ticks.insert(pid, process);
        }
    }
    Ok(CpuSample { at_ms, ticks })
}

/// CPU per name between two samples, in % of one core: the ticks of the processes in both
/// samples. `None` when no time passed.
pub fn cpu_percent(
    earlier: &CpuSample,
    later: &CpuSample,
    names: &[String],
) -> Vec<(String, Option<f64>)> {
    let seconds = later.at_ms.saturating_sub(earlier.at_ms) as f64 / MS_PER_SECOND;
    names
        .iter()
        .map(|name| {
            let percent = (seconds > 0.0).then(|| {
                let ticks: u64 = later
                    .ticks
                    .iter()
                    .filter(|(_, (later_name, _))| later_name == name)
                    .filter_map(|(pid, (_, now))| {
                        earlier
                            .ticks
                            .get(pid)
                            .filter(|(earlier_name, _)| earlier_name == name)
                            .map(|(_, before)| now.saturating_sub(*before))
                    })
                    .sum();
                ticks as f64 / CLOCK_TICKS_PER_SECOND / seconds * PERCENT
            });
            (name.clone(), percent)
        })
        .collect()
}

/// Samples CPU per name; each value is between this sample and the one before.
#[derive(Debug)]
pub struct CpuSampler {
    proc_dir: PathBuf,
    names: Vec<String>,
    last: Option<CpuSample>,
}

impl CpuSampler {
    /// A sampler of the processes named `names` in `proc_dir`.
    pub fn new(proc_dir: PathBuf, names: Vec<String>) -> Self {
        Self {
            proc_dir,
            names,
            last: None,
        }
    }

    /// CPU per name since the last sample; `None` for the first sample and without `proc_dir`.
    pub fn sample(&mut self, at_ms: u64) -> Vec<(String, Option<f64>)> {
        let now = read_sample(&self.proc_dir, at_ms).ok();
        let values = match (&self.last, &now) {
            (Some(earlier), Some(later)) => cpu_percent(earlier, later, &self.names),
            _ => self.names.iter().map(|name| (name.clone(), None)).collect(),
        };
        self.last = now;
        values
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    fn sample(at_ms: u64, processes: &[(u32, &str, u64)]) -> CpuSample {
        CpuSample {
            at_ms,
            ticks: processes
                .iter()
                .map(|(pid, name, ticks)| (*pid, ((*name).to_owned(), *ticks)))
                .collect(),
        }
    }

    #[test]
    fn parse_stat_reads_a_name_with_spaces_and_brackets() {
        let text = "4242 (my (odd) name) S 1 4242 4242 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 1\n";
        assert_eq!(parse_stat(text), Some(("my (odd) name".to_owned(), 300)));
        assert_eq!(parse_stat("4242 (no end"), None);
        assert_eq!(parse_stat("4242 (sim) S 1 2 3"), None);
        assert_eq!(parse_stat(""), None);
    }

    #[test]
    fn cpu_of_two_samples_is_percent_of_one_core() {
        let earlier = sample(0, &[(1, "sim", 100), (2, "sim", 0), (3, "bash", 5)]);
        let later = sample(
            2_000,
            &[
                (1, "sim", 200),
                (2, "sim", 100),
                (3, "bash", 5),
                (4, "sim", 9_999),
            ],
        );
        assert_eq!(
            cpu_percent(&earlier, &later, &names(&["sim", "rep-gateway"])),
            vec![
                ("sim".to_owned(), Some(100.0)),
                ("rep-gateway".to_owned(), Some(0.0)),
            ]
        );
        assert_eq!(
            cpu_percent(&later, &later, &names(&["sim"])),
            vec![("sim".to_owned(), None)]
        );
    }

    #[test]
    fn a_proc_folder_is_read_by_pid() {
        let dir = tempfile::tempdir().expect("dir");
        let write = |folder: &str, text: &str| {
            std::fs::create_dir_all(dir.path().join(folder)).expect("dir");
            std::fs::write(dir.path().join(folder).join(STAT_FILE), text).expect("write");
        };
        write("7", "7 (sim) S 1 7 7 0 -1 0 0 0 0 0 30 12 0 0\n");
        write("self", "8 (sim) S 1 8 8 0 -1 0 0 0 0 0 1 1 0 0\n");
        write("9", "broken");
        std::fs::create_dir_all(dir.path().join("10")).expect("dir");
        let read = read_sample(dir.path(), 5).expect("read");
        assert_eq!(read, sample(5, &[(7, "sim", 42)]));
    }

    #[test]
    fn the_first_sample_and_a_missing_proc_are_unknown() {
        let dir = tempfile::tempdir().expect("dir");
        let mut sampler = CpuSampler::new(dir.path().to_path_buf(), names(&["sim"]));
        assert_eq!(sampler.sample(0), vec![("sim".to_owned(), None)]);
        assert_eq!(sampler.sample(1_000), vec![("sim".to_owned(), Some(0.0))]);
        let mut missing = CpuSampler::new(dir.path().join("missing"), names(&["sim"]));
        missing.sample(0);
        assert_eq!(missing.sample(1_000), vec![("sim".to_owned(), None)]);
    }
}
