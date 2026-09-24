// SPDX-License-Identifier: AGPL-3.0-or-later
//! Show the messages around each hit of a type index, in ledger order.
//! Usage: dump_seq <id> <before> <after> <max_hits> <ledger>...

use std::env;
use std::process::ExitCode;

use nw_capture::{Decoder, ledger_records};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 6 {
        eprintln!("usage: dump_seq <id> <before> <after> <max_hits> <ledger>...");
        return ExitCode::FAILURE;
    }
    let want: u64 = args[1].parse().unwrap_or(0);
    let before: usize = args[2].parse().unwrap_or(5);
    let after: usize = args[3].parse().unwrap_or(5);
    let max_hits: usize = args[4].parse().unwrap_or(3);
    let mut hits = 0_usize;
    for path in &args[5..] {
        if hits >= max_hits {
            break;
        }
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("skip {path}: {e}");
                continue;
            }
        };
        // (time_ms, direction, type index) of every message, in order.
        let mut stream: Vec<(u64, u32, u64)> = Vec::new();
        let mut decoder = Decoder::new();
        for record in ledger_records(&bytes) {
            let Ok(record) = record else { break };
            let out = decoder.record(&record);
            for delivered in &out.envelopes {
                if let Some(id) = delivered.envelope.type_index() {
                    stream.push((record.time_ms, record.direction, id));
                }
            }
        }
        for (index, (time, dir, id)) in stream.iter().enumerate() {
            if *id != want || hits >= max_hits {
                continue;
            }
            hits += 1;
            println!("--- hit {hits} t={time} dir={dir} file={path}");
            let start = index.saturating_sub(before);
            let end = (index + after + 1).min(stream.len());
            for (at, (t, d, i)) in stream[start..end].iter().enumerate() {
                let mark = if start + at == index { ">>" } else { "  " };
                println!("{mark} {:>6} dir={} id={}", *t as i64 - *time as i64, d, i);
            }
        }
    }
    ExitCode::SUCCESS
}
