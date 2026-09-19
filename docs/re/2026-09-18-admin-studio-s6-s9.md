# Admin Studio S6 (loot) and S9 (to-do list) (2026-09-18)

Steps S6 and S9 of the admin studio spec. Commits ea5c08f..5700148 on main.

## S6: what was built

- `nw-content/src/loot.rs`: the loot roll of gathering became `LootTables` for gathering, mob
  kills and the drop simulator. New: `[LBID]` bucket entries (one bucket item per count, equal
  chance), the condition `GlobalMod` (a plain roll), and any other condition read as a loot tag
  that the mob must have (Inferred from the retail creature tables: `CreatureLootMaster` has
  `GlobalMod`, `CreatureLootCommon` has `Common`).
- Mob types: `MobLoot` reads `VitalsBaseData` (`CanDropLoot`, `LootTableId`, `LootDropChance`,
  `LootTags`), then `VitalsData`. The design kind `mob` now edits `VitalsBaseData`: `VitalsData`
  has only 22 rows (war objects), the creatures are in `VitalsBaseData`.
- A mob killed by a player of the same sim rolls its table at its drop chance (read as a percent)
  and the items go into the killer's bag, as a gather does. The tutorial gives no kill loot.
- Studio: `POST /api/studio/loot/simulate` rolls a table up to 100,000 times over the retail
  tables with the design files merged in memory (nothing written); `GET /api/studio/loot/no-loot`
  lists the mob types without a table. The records page shows the simulator on a loot table and a
  "Mobs with no loot" button.

## S6: tests and live check

| Check | Result |
|---|---|
| 10,000 kills through the sim's loot step against 10,000 simulator rolls (sim test) | within 1 percentage point; the 30 % item at 30 % within 1 point |
| bucket entry, self-rolling table stops, GlobalMod, tag condition (content tests) | pass |
| simulator on test world 2: `CreatureLootMaster`, 10,000 rolls, level 20, tag `Common` | 0.7 s; 972 different items |
| no-loot on test world 2 | 777 mob types |

## S6: Unknown and blocked

- **A loot bag at the mob: blocked.** The only loot container the server makes is the tutorial's
  static bag, which the client builds from its level slice. A dropped bag needs: the client entity
  of a loot-drop prefab (slice asset and components), a server container store (items, owner,
  version, expiry), a take path by the container's entity id (`TakeAllItems` 2939, `TakeItem`
  2942 is not decoded), unique instance ids per drop, and the LootTracker (982) state.
- **Retail roll rules: Unknown.** Not read: `ChanceToExceedIndex`, `RollBonusSetting`,
  `GSBonus`, `HWMMult`, gear score ranges, perks, loot limits, bucket tags (for example
  `{!clvl}11-20`, a character level range on nwdb.info) and bucket odds. So the simulator's drop
  set is wider than retail (level-gated items such as `BagofJuniperBerriesLVL60` drop at level
  20).
- `LootDropChance` units: Unknown (read as a percent).
- The test bots have no bag (S5), so no live kill gave loot; the sim test is the 10,000-kill check.

## S9: what was built

- `content/todo.toml`: one `[[task]]` per task (id `t<n>`, title, text, kind, state, taken by,
  issue, linked design files).
- `/todo` page: add a task; import a GitHub issue once (title, text, address) through the public
  API (`[studio] github_token_env` names a token for a private repository); set the state and who
  took it; link design files (each opens its editor: the records page for a client table kind,
  the studio for the others); "Changed design files" lists the files of `content/` that differ
  from the last commit (`git ls-files` and `git diff --cached`, read-only).
- Changes are local mode only; server mode reads the list.

## S9: live check (test world 2)

- Import of `https://github.com/rust-lang/rust/issues/1` made task `t1` with the issue title, text
  and address; the page showed it; "Changed design files" showed none (the list itself is left
  out). The test `todo.toml` was moved to `local-data/design-tests/s9/`.
