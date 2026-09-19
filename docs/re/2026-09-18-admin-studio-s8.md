# Admin Studio S8: scripts and events (2026-09-18)

Step S8 (visual block scripts, script editor, events, Events panel) of the admin studio spec.
Commits 6394810..e98852b on main.

## What was built

- Block registry (`nw-world/src/script.rs`), shared by the design check, the sim and the editor:
  triggers `player_enters_area`, `player_leaves_area`, `mob_dies`, `timer`, `quest_done`,
  `event_starts`; conditions `random_chance`, `has_item`, `quest_state`, `flag_is`,
  `mobs_alive`; actions `spawn_mobs`, `despawn_mobs`, `give_item`, `start_quest`, `set_flag`,
  `start_event`; flow `wait`. Graph checks: a trigger, known nodes and outputs, no link into a
  trigger, every other node linked into, typed values, no loop without a `wait`.
- Design kinds `script` (`scripts/<map>/`) and `event` (`events/<map>/`). Apply writes them to
  `design-<build>.json` beside the merged files, so the world files keep their schema.
- Sim: each script of the level compiled once; a trigger starts a run, conditions branch, a
  `wait` holds the run, several links from one output fork it, 1,000 steps per script per tick.
  A block that acts at a point (`spawn_mobs`, `start_event`) runs only on the sim that owns the
  point. Events spawn their waves of GM mobs around the GM's point after their delays, and end
  after `end_after_ms`, when cleared, or on a GM stop; their mobs end with them.
- Admin: `/scripts` (blocks palette, canvas with drag, links from an output dot to a block,
  typed values, Save through the design file route, run log); live page "GM events" (the level's
  events, start at the map point, stop, running state); `/api/live/script-log` from a new bus
  subject.

## Tests

| Test | Result |
|---|---|
| graph checks: values, links, trigger, reached nodes, loop with and without a wait | pass |
| script and event files read, checked, merged into the design data | pass |
| sim: area enter and leave; a player who leaves the sim fires no leave | pass |
| sim: timer, wait, branch, spawn, mobs alive, despawn, with the run log in order | pass |
| sim: a 1,500-node run stops at 1,000 steps and ends in the next tick | pass |
| sim: `mob_dies` runs with the killer; a kill by a player of another sim does not | pass |
| sim: only the owner of the point spawns | pass |
| sim: event waves on time, end by time, end when cleared, stop removes the mobs | pass |
| admin: event start goes to the point's sim; blocks route; script log keeps the newest lines | pass |
| page tests: typed values, script file text, graph from a file, links and node removal | pass |

## Live check (test world 2, port 9280)

| Step | Observed |
|---|---|
| A timer script and an event written through the file route; the editor opens the script and its Save writes it again | saved, check passed |
| Apply | both sims load 1 script and 1 event |
| Run log, first run | both sims ran the timer and both spawned 2 mobs: **bug** |
| Fix (e98852b), Apply | sim-2 spawned 2; sim-1 logged "the point is another sim's" |
| GM start `test_invasion` at (8360, 8150) | running on sim-2: 3 mobs, then 5 after 10 s; the `event_starts` script ran |
| GM stop | stopped on sim-2; no event runs |

The test files were moved to `local-data/design-tests/s8/` and Apply ran again (0 scripts,
0 events, no mobs).

## Unknown and not built

- **NPC says a line, message to the area, event start and end messages: Unknown** (no chat or
  NPC message in the server; S3, S4). Left out of the registry.
- The spec's exit test "player enters area, spawn group, NPC says line, in the client" needs the
  game client and the NPC line (Unknown); the area trigger and the spawn are tested in the sim.
- Script values naming client ids (vitals, items, objectives) are not checked against the client
  data; a wrong id shows in the run log.
