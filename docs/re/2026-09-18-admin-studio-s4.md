# Admin Studio S4 exit test (2026-09-18)

Step S4 (AI profiles, patrol paths, spawn groups, Studio editors) of the admin studio spec.
Commits 96c5b3b..c0c64ff on main.

## Set-up

- The second test world (`LIVE_STACK_INSTANCE=2 just live 2`): admin on 127.0.0.1:9280, other
  ports as in the S2/S3 record. No bots and no players.
- All three design files were made with the Studio editors in the browser, not by hand.

## Steps and results

| Step | Observed |
|---|---|
| AI profile form: `test_patrol`, behaviour `patrol`, `speed_mps` 2.5, `respawn_ms` 30000 | saved `ai/test_patrol.toml` |
| Path form: Go to (8330, 8130), three right-clicks on the road, waits `2000, 0, 2000`, mode `back_and_forth` | saved `paths/newworld_vitaeeterna/test_walk.toml`; z of each point from `/api/world/height` (636.6, 629.4, 626.5) |
| Spawn group form: `Corruption_Swarmer_03GC03_ELITE`, level 46, two right-clicks, profile and path | saved `spawns/newworld_vitaeeterna/test_swarmers.toml` |
| Apply | done, "2 of 2 sims back", no problems |
| Live view, every 3 s | both mobs walk from point 1 to point 3 at about 2.5 m/s, wait, and walk back |

The test files were moved to `local-data/design-tests/s4/` (not kept) and Apply ran again; the two mobs are
gone and the merged content is the retail content again.

## Found and fixed

- A patrol mob stood still in a world with no player: its brain slept until a player came near,
  and slept again after quiet time. A patrol brain now wakes and never sleeps (8927871, tests
  `a_patrol_wakes_and_walks_with_no_player_near`, `a_patrol_spawn_walks_its_route_with_no_player_in_the_world`).
  The first sim test had a player near, so it did not see this.
- A right-click on the editor map used the point of the last mouse move, so a click with no move
  before it added nothing. The point now comes from the click (c0c64ff). The editor map also got
  Go (x, y), zoom and whole-world tools.

## Not in S4

- NPC editor: the sim spawns no NPC, and how the client shows an NPC entity is **Unknown**.
- A guard profile was not live-tested; the sim tests cover it (`a_guard_spawn_stands_...`).
