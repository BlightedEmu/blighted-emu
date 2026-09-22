# Ghidra tools

Scripts for reading your own installed New World client with Ghidra. They write nothing into
this repository: every output (decompiled functions, name tables, string lists) is client data
and goes into the ignored `local-data/` folder.

- `scripts/`: Ghidra Java scripts. Run them headless with
  `-scriptPath tools/ghidra/scripts` (example in `docs/re/2026-09-14-can-take-items.md`).
- `sweep/`: the full function sweep (`docs/re/2026-09-16-full-function-sweep.md`).
  `sweep.sh` needs `GHIDRA_DIR` (the Ghidra install) and `NW_GHIDRA_PROJECT_DIR` (the analysed
  project); it writes to `local-data/re-sweep/` unless `NW_SWEEP_OUTDIR` is set. The type names
  come from `$NW_GAME_DIR/typeindex.json`. Check the name rules with
  `python3 -m unittest test_rules` in `sweep/`.
