# Admin Studio S2 and S3 exit tests (2026-09-18)

Steps S2 (GM command channel, server mode, logins, ranks, audit log) and S3 (GM tools) of the
admin studio spec. Commits 8b71d23..9ca01db on main.

## Set-up

- A second test world next to any running one (`LIVE_STACK_INSTANCE=2 just live 2`): edge-http on
  8543, gateway UDP 33535, metrics 92xx, admin 127.0.0.1:9280, world id `…000000000002`, bots named
  `Instbot…`. The user asked for tests on other ports, so a world that runs for play is not touched.
- 10 bots from the live page.
- Server mode: a second admin on `https://127.0.0.1:9380` with the edge's test certificate, the
  local global database, and ranks given with `blighted-admin grant-rank` to two bot accounts
  (`game_master`, `csr`); the ranks were removed after the test.

## S2 results (server mode, `curl` over HTTPS)

| Check | Observed |
|---|---|
| Plain HTTP on the HTTPS port | refused (no answer) |
| A page without a session | 303 to `/login` |
| The data API without a session | 401 |
| A wrong password | 401 |
| CSR teleport | 403 "this needs the game_master rank" |
| GM teleport | 200 "done" |
| GM Apply (design route) | 403 (local mode only) |
| GM rank list | 403 (head admin only) |
| Audit log | the login and the teleport rows, rank `game_master` |
| Logout, then `/api/auth/me` | 204, then 401 |

Found and fixed: over HTTPS, browsers and `curl` speak HTTP/2, which names the host in the URI and
sends no `Host` header; the request check refused everything (e61ad84, test
`an_http2_request_names_its_host_in_the_uri`).

## S3 results (local mode on world 2)

| Command | Observed |
|---|---|
| Teleport to a point | bot moved to (2700, 4600, 60) and stayed |
| Teleport to a player | bot moved to the other bot's position |
| God mode on, heal | done |
| Kill | done; a bot that was already dead (killed by mobs) gives "dead or moving between sims" |
| Give / take item `PotionHealthT1` | the bots have no bag (their characters have no starter kit row): now "the player has no bag" (9ca01db) |
| Unknown item | "no item NoSuchItem" |
| Spawn 3 `Dunwood_Rabbit` level 20 at (2700, 4600) | 3 GM mobs at the point, max health 94 |
| Despawn all | every sim answered done; no GM mob left |
| Kick | the bot left the world view |
| Unknown player, point off the level | 404 "no sim shows this player"; 422 "is off the level" |

## Not built (Unknown)

- Messages to one or all players, mute: the server has no chat; the client message is Unknown.
- Give coin: no currency in the server yet.
- "Teleport self to player" for a CSR: the admin site does not know which game character is the
  logged-in admin's own (the snapshots carry no account). GMs move any player instead.
- In-game GM commands (spec 8.5, step S10).

## Unknown

- How the client shows a GM teleport, kill or heal to the player (no client in this test); the
  sim sends its usual own-state, health and death messages.
