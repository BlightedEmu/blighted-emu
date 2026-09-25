// SPDX-License-Identifier: AGPL-3.0-or-later
//! Check bundle members logged by the sim (`mob group ... members="..."` debug lines):
//! each member's bytes are read back with the member readers and re-encoded; a member
//! whose reader fails, or whose re-encode differs, is printed.
//! Usage: check_members <log> [carded|classes]
//!        check_members ledger <ledger.bin>   (tally a retail ledger's member pairs)
//!        check_members steps <ledger.bin>    (step sizes of a retail ledger's positions)

use std::env;
use std::process::ExitCode;

use nw_messages::BundlePayload;
use nw_messages::wire::Writer;

fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).ok())
        .collect()
}

/// Tallies the (entity key, class) pairs of every bundle in a retail ledger, and how
/// many of its bundles went on the reliable channel.
fn ledger_pairs(path: &str) -> ExitCode {
    use nw_capture::{Decoder, ledger_records};
    use nw_messages::{BodyCodec, BundlePayload as Payload, ReplicatedStateBundle};
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("{path}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut decoder = Decoder::new();
    let mut pairs: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut bundles = 0usize;
    let mut reliable = 0usize;
    for record in ledger_records(&bytes) {
        let Ok(record) = record else { continue };
        for delivered in &decoder.record(&record).envelopes {
            let envelope = &delivered.envelope;
            if envelope.type_index() != Some(nw_messages::REPLICATED_STATE_BUNDLE_TYPE_INDEX) {
                continue;
            }
            bundles += 1;
            reliable += usize::from(delivered.reliable);
            let Ok(bundle) = ReplicatedStateBundle::decode(&envelope.body) else {
                continue;
            };
            if let Ok(known) = Payload::decode_known(&bundle.payload) {
                for (member, _) in &known.members {
                    *pairs
                        .entry(format!("{}:{:?}", member.component, member.state.class()))
                        .or_default() += 1;
                }
            }
        }
    }
    let mut rows: Vec<_> = pairs.iter().collect();
    rows.sort();
    for (pair, count) in rows {
        println!("{count:6} {pair}");
    }
    println!("bundles={bundles} reliable={reliable}");
    ExitCode::SUCCESS
}

/// Step sizes of the class-11 (ALC) positions of the most-sent entity keys in a retail
/// ledger: how smooth the retail server's sent positions are.
fn ledger_steps(path: &str) -> ExitCode {
    use nw_capture::{Decoder, Detail, describe, ledger_records};
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("{path}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut decoder = Decoder::new();
    let mut last: std::collections::HashMap<u64, [f32; 3]> = std::collections::HashMap::new();
    let mut steps: std::collections::HashMap<u64, Vec<f32>> = std::collections::HashMap::new();
    for record in ledger_records(&bytes) {
        let Ok(record) = record else { continue };
        let output = decoder.record(&record);
        for (slot, delivered) in output.envelopes.iter().enumerate() {
            let slot = u16::try_from(slot).unwrap_or(u16::MAX);
            let decoded = describe(0, slot, &record, delivered);
            let Detail::Bundle { members, .. } = decoded.detail else {
                continue;
            };
            for member in members {
                if member.class != 11 {
                    continue;
                }
                let Some(at) = member.position else { continue };
                if let Some(before) = last.get(&member.key) {
                    let d = ((at[0] - before[0]).powi(2) + (at[1] - before[1]).powi(2)).sqrt();
                    steps.entry(member.key).or_default().push(d);
                }
                last.insert(member.key, at);
            }
        }
    }
    let mut keys: Vec<_> = steps.iter().collect();
    keys.sort_by_key(|(_, list)| std::cmp::Reverse(list.len()));
    for (key, list) in keys.iter().take(4) {
        let mut sorted = (*list).clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let moving: Vec<f32> = sorted.iter().copied().filter(|d| *d > 0.02).collect();
        let over = moving.iter().filter(|d| **d > 0.35).count();
        println!(
            "key={key} steps={} moving={} median={:.3} p90={:.3} max={:.3} over_0.35m={over}",
            list.len(),
            moving.len(),
            moving.get(moving.len() / 2).copied().unwrap_or(0.0),
            moving.get(moving.len() * 9 / 10).copied().unwrap_or(0.0),
            moving.last().copied().unwrap_or(0.0),
        );
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "usage: check_members <log> [carded|classes] | check_members ledger|steps <ledger.bin>"
        );
        return ExitCode::FAILURE;
    }
    if args[1] == "ledger" {
        return match args.get(2) {
            Some(path) => ledger_pairs(path),
            None => ExitCode::FAILURE,
        };
    }
    if args[1] == "steps" {
        return match args.get(2) {
            Some(path) => ledger_steps(path),
            None => ExitCode::FAILURE,
        };
    }
    let carded_only = args.get(2).map(String::as_str) == Some("carded");
    let classes_mode = args.get(2).map(String::as_str) == Some("classes");
    let mut classes: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let log = match std::fs::read_to_string(&args[1]) {
        Ok(log) => log,
        Err(error) => {
            eprintln!("{}: {error}", args[1]);
            return ExitCode::FAILURE;
        }
    };
    let mut checked = 0usize;
    let mut bad = 0usize;
    for line in log.lines() {
        if carded_only && !line.contains("carded=true") {
            continue;
        }
        let Some(start) = line.find("members=\"") else {
            continue;
        };
        let Some(end) = line[start + 9..].find('"') else {
            continue;
        };
        let stamp = &line[..line.len().min(27)];
        for member_hex in line[start + 9..start + 9 + end].split(' ') {
            let Some(member) = unhex(member_hex) else {
                continue;
            };
            checked += 1;
            // One payload of one group of one member.
            let mut writer = Writer::new();
            let Ok(writer) = writer.prefixed(1) else {
                continue;
            };
            let payload = writer.u8(1).bytes(&member).finish();
            if classes_mode {
                if let Ok(known) = BundlePayload::decode_known(&payload) {
                    if let Some((member_read, _)) = known.members.first() {
                        *classes
                            .entry(format!(
                                "{}:{:?}",
                                member_read.component,
                                member_read.state.class()
                            ))
                            .or_default() += 1;
                    }
                }
                continue;
            }
            let report = match BundlePayload::decode_known(&payload) {
                Ok(known) => {
                    if let Some((class, error)) = &known.failed {
                        Some(format!("reader failed class={class:?} error={error}"))
                    } else if let Some(class) = &known.stopped_at {
                        Some(format!("no reader class={class:?}"))
                    } else if known.members.len() != 1 {
                        Some(format!("read {} members", known.members.len()))
                    } else {
                        let (member_read, range) = &known.members[0];
                        if range.end != payload.len() {
                            Some(format!(
                                "short read to {} of {} bytes",
                                range.end,
                                payload.len()
                            ))
                        } else {
                            match member_read.encode() {
                                Ok(bytes) if bytes == member => None,
                                Ok(_) => Some("re-encode differs".to_owned()),
                                Err(error) => Some(format!("re-encode failed {error}")),
                            }
                        }
                    }
                }
                Err(error) => Some(format!("payload failed {error}")),
            };
            if let Some(report) = report {
                bad += 1;
                println!("{stamp} {report} member={member_hex}");
            }
        }
    }
    if classes_mode {
        let mut rows: Vec<_> = classes.iter().collect();
        rows.sort();
        for (class, count) in rows {
            println!("{count:5} {class}");
        }
        return ExitCode::SUCCESS;
    }
    println!("checked={checked} bad={bad}");
    ExitCode::SUCCESS
}
