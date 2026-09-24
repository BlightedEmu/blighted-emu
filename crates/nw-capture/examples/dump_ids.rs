// SPDX-License-Identifier: AGPL-3.0-or-later
//! Dump sample bodies for given type indexes from ledger files.
//! Usage: dump_ids <id,id,...> <max_per_id> <ledger>...

use std::collections::HashMap;
use std::env;
use std::process::ExitCode;

use nw_capture::{Decoder, ledger_records};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: dump_ids <id,id,...> <max_per_id> <ledger>...");
        return ExitCode::FAILURE;
    }
    let want: Vec<u64> = args[1].split(',').filter_map(|s| s.trim().parse().ok()).collect();
    let max_per: usize = args[2].parse().unwrap_or(4);
    let mut found: HashMap<u64, usize> = HashMap::new();
    let mut counts: HashMap<(u64, u32), u64> = HashMap::new();
    let mut lens: HashMap<u64, Vec<usize>> = HashMap::new();
    for path in &args[3..] {
        if want.iter().all(|id| found.get(id).copied().unwrap_or(0) >= max_per) {
            break;
        }
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("skip {path}: {e}");
                continue;
            }
        };
        let mut decoder = Decoder::new();
        for record in ledger_records(&bytes) {
            let Ok(record) = record else { break };
            let out = decoder.record(&record);
            for delivered in &out.envelopes {
                let Some(id) = delivered.envelope.type_index() else { continue };
                if !want.contains(&id) {
                    continue;
                }
                *counts.entry((id, record.direction)).or_default() += 1;
                lens.entry(id).or_default().push(delivered.envelope.body.len());
                let n = found.entry(id).or_default();
                if *n < max_per {
                    println!(
                        "ID {id} dir={} len={} file={} hex={}",
                        record.direction,
                        delivered.envelope.body.len(),
                        path,
                        hex(&delivered.envelope.body)
                    );
                    *n += 1;
                }
            }
        }
        eprintln!("done {path} found={found:?}");
    }
    eprintln!("counts={counts:?}");
    for (id, mut ls) in lens {
        ls.sort_unstable();
        ls.dedup();
        eprintln!("id {id} distinct_lens={ls:?}");
    }
    ExitCode::SUCCESS
}
