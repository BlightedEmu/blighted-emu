# Admin Studio S1 exit test (2026-09-16)

Step S1 of the admin studio spec (design layer, `content-tool design`, the `/studio` page with
Check and Apply, the restart FIFO of `just live`). Commits ef88799..6568492 on main.

## Set-up

- `cargo build --release`, then `just live 4` (edge-http, world-director, 4 sims, rep-gateway,
  blighted-admin on 127.0.0.1:9180). Retail and merged content from `just bundle`, `just world`,
  `just design` (0 design files: the merged files are byte-equal to the retail files).
- 1,000 bots from the live page (`nw-testclient swarm`) keep the mobs near the start points awake.
- Test spawn, picked from `/api/world/spawns` (which now gives the design key): a `Reindeer`,
  level 65, at (2976.2, 5656.1), 18 m from the bot start (2984, 5640); the next spawn is 91 m
  away. Key `133782293_2976.2_5656.1_148.0`.
- The mob was read from the live view (`/api/live/view`, the sim snapshots): the one mob within
  30 m of the spawn, by its `max_health`.

## Results

| Step | Observed |
|---|---|
| Baseline | `max_health` 9,479 (level 65). |
| Design file `spawns/newworld_vitaeeterna/<key>.toml`, `level = 10`; Apply | Check → Write → Wait for sims → Done "4 of 4 sims back" (18:27:22 to 18:29:05). Events: each sim left and joined. |
| After Apply | `max_health` 579 (level 10), same spot, 6 reads. |
| File renamed to a key that does not exist (`133782293_1.0_1.0_1.0`) | Check: one `unknown_id` on that file, nothing written. Apply: Failed at Check, "1 problems in design files"; the merged bundle and world file kept their modification times. |
| File removed; Apply | Merged bundle and world file byte-equal to retail again. `max_health` 9,479 (level 65) once the sims were back. |

Pass: a design file changes a spawn in the running world through Apply, a bad file stops Apply
before anything is written, and removing the file brings the retail value back.

## Found during the test

- **Restart time on a cold cache.** The second Apply ended Failed: "sims did not return in 120 s".
  The project folder is on a spinning disk (Toshiba HDWR440). With the world files no longer in
  the page cache, and another program reading the game disk at the same time, the 4 sims each
  read the 456 MB world file and took 15 minutes to start (18:39 to 18:54). The first Apply, with
  a warm cache, took under 2 minutes. `[studio] restart_timeout_s` is now 600 (6568492); Apply
  still writes the files and restarts the sims when the wait runs out.
- **Bots after a restart.** The running bot batch did not come back after the 15-minute restart;
  a new batch was started from the live page.
- The page check by eye (both themes, Check, a failed Apply) was done earlier on a lone admin;
  see the plan ledger.

## Unknown

- The level in the game client: not looked at in a client in this test. The sim's mob health
  follows the level (9,479 at 65, 579 at 10), and the sim reads the merged world file.
