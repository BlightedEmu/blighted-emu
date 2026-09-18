# Full function sweep of build 25810808

Date: 2026-09-16. Labels: **Observed**, **Inferred**, **Unknown**.

This note tells a later reader where the finished sweep output is. It does not copy the note bodies.

## What was swept

**Observed.** The program is `NewWorld.exe`, Steam build `25810808`. `local-data/re-sweep/HANDOVER.md` and `local-data/prompts/ghidra-full-sweep.md` name that build. `local-data/re-sweep/meta.json` records the program name `NewWorld.exe`.

**Observed.** The read-only Ghidra project is `~/nw-re/ghidra-25810808`. `LEDGER.md` says the sweep used that project and passed `-readOnly -noanalysis`. This note did not open Ghidra.

**Observed.** The sweep plan said to write notes under `local-data/re-sweep/`. It said to leave other repository files unchanged. It did not ask for a note in `docs/re`.

## Live counts

**Observed.** `local-data/re-sweep/meta.json` records image base `0x140000000` and function count `354594`. Probe time: `2026-09-16T01:12:05.173554712Z`.

**Observed.** `local-data/re-sweep/checkpoint.json` is the live result:

`{"next_index": 354594, "total": 354594, "done_at": "2026-09-16T04:09:46.075259929Z"}`

**Observed.** `local-data/re-sweep/COMPLETE` contains `2026-09-16T04:10:18Z`.

**Observed.** `local-data/re-sweep/LEDGER.md` records SHA-256 `30fe6461f929ba44178376a72f0765dc5da28ccba20e5006a60a229babd74033` for `NewWorld.exe`.

**Observed.** `local-data/re-sweep/index.md` records 354594 functions written to batch files, 354577 functions with decompiled C text, 17 decompile errors, and 0 skipped indexes. Elapsed time in that file is Unknown. Label counts in that file: Observed 10128, Inferred 79556, Unknown 264910.

## Old progress text

**Observed.** `local-data/re-sweep/LEDGER.md` still shows an early checkpoint: `next_index` 5500 of 354594 at `2026-09-16T01:15:25Z`. The end section of that file says the values 5500 and 10500 are old progress points.

**Observed.** `local-data/re-sweep/HANDOVER.md` now states the finished checkpoint: `next_index` 354594 of 354594, done at `2026-09-16T04:09:46Z`.

**Observed.** `HANDOVER.md` and `LEDGER.md` can still show an early checkpoint. `checkpoint.json` is the live result. Read `checkpoint.json` when those two notes disagree with it.

## Where the findings are

Paths below are relative to the repository root.

**Observed.** `local-data/re-sweep/index.md` gives the totals, how to read the files, and the counts per label and per rule.

**Observed.** `local-data/re-sweep/classes.md` groups functions by namespace or RTTI class. The file records class count 999. Each class lists function count, vtable address, and methods.

**Observed.** `local-data/re-sweep/network.md` lists type-index types whose names or UUIDs appear in a function. The file records 1663 types with at least one function. Each type lists functions and a handler, reader, or writer role.

**Observed.** `local-data/re-sweep/strings.md` lists the 2000 strings that the most functions reference. Each row shows up to 20 function addresses. The count column is the full function count.

**Observed.** `local-data/re-sweep/unknowns.md` groups functions whose label is Unknown. The file records Unknown function count 264910, grouped by address range, and then lists large functions with hints.

**Observed.** `local-data/re-sweep/names.tsv` has one row per function that was in the batch files. The columns are entry, old name, suggested name, label, rule, and evidence.

**Observed.** `local-data/re-sweep/HANDOVER.md` is the handover note for the finished sweep and the name-rule fixes of 2026-09-16.

**Observed.** `local-data/re-sweep/LEDGER.md` is the sweep ledger. It records the binary path, the SHA-256, the probe, the timed batch, and old service progress.

**Observed.** Each line in `local-data/re-sweep/funcs/batch-<start>.jsonl.gz` is one function. The line has the address, the name, the callers, the callees, the strings, and the decompiled C text or an error. The directory listing shows these batch files:

- `local-data/re-sweep/funcs/batch-0.jsonl.gz`
- `local-data/re-sweep/funcs/batch-500.jsonl.gz`
- `local-data/re-sweep/funcs/batch-20500.jsonl.gz`
- `local-data/re-sweep/funcs/batch-40500.jsonl.gz`
- `local-data/re-sweep/funcs/batch-60500.jsonl.gz`
- `local-data/re-sweep/funcs/batch-80500.jsonl.gz`
- `local-data/re-sweep/funcs/batch-100500.jsonl.gz`
- `local-data/re-sweep/funcs/batch-120500.jsonl.gz`
- `local-data/re-sweep/funcs/batch-140500.jsonl.gz`
- `local-data/re-sweep/funcs/batch-160500.jsonl.gz`
- `local-data/re-sweep/funcs/batch-180500.jsonl.gz`
- `local-data/re-sweep/funcs/batch-200500.jsonl.gz`
- `local-data/re-sweep/funcs/batch-220500.jsonl.gz`
- `local-data/re-sweep/funcs/batch-240500.jsonl.gz`
- `local-data/re-sweep/funcs/batch-260500.jsonl.gz`
- `local-data/re-sweep/funcs/batch-280500.jsonl.gz`
- `local-data/re-sweep/funcs/batch-300500.jsonl.gz`
- `local-data/re-sweep/funcs/batch-320500.jsonl.gz`
- `local-data/re-sweep/funcs/batch-340500.jsonl.gz`

## Commit

**Observed.** No commit was made for this note.
