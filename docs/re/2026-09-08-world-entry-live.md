# Real client world entry: live results

Date: 2026-09-08. Build: Steam `25810808`, patched `Bin64/BlightedNewWorld.exe`.

This note records live tests of the real New World client entering the
emulator, after the certificate bypass
([cert-bypass plan](2026-09-06-cert-bypass-patch-plan.md)). Facts only; no
capture bytes were copied into the server.

## Summary

The client advances through the post-registration state machine
(`docs/re/2026-08-29-client-protocol-login-gridmate.md`, resolved item 7). Two
walls are cleared; one remains.

| State | Result |
|---|---|
| `WaitingForREPConnection` -> `WaitingForActorGameConnection` | Reached (registration accepted). |
| `WaitingForActorGameConnection` -> `WaitingForSpawnPoint` | **Cleared** by sending type 1628 (`PlayerManagerSelfIdentificationMsg`). |
| `WaitingForSpawnPoint` -> `WaitingForPlayerSpawn` | Reached ("spawn point found"). |
| `WaitingForPlayerSpawn` -> `InGame` | **Not reached.** Still stuck here. |

## What the two fixes did

### Type 1628 — PlayerManagerSelfIdentificationMsg

The sim never sent it, so the client could not leave
`WaitingForActorGameConnection`. Its handler `FUN_146454c00` always calls the
`CJavelinActorGame` `ConnectionSuccess` setter (`FUN_145a87010`) at the end, so
the client only needs a body its unmarshal (`FUN_1414fd530`, build `25810808`
`FUN_141516e40`) accepts. The field readers fail only on stream underflow (two
flag bytes must be 0 or 1), and the bulk reader (`FUN_1417ad260`) reads a
prefix-coded count then that many big-endian `u32`, so count 0 reads nothing.

So the smallest accepted body is 157 bytes: a presence block (flag `0x01` + 16
bytes), three `FUN_1415acc30` blocks (`u32` + two 16-byte ids), the
`FUN_1415bad60` block (`u32`, count `0`, two `0` flags, two `u32`), and a
second presence block. All zero except the two `0x01` presence flags.

Live: Game.log logs "actor game connection succeeds" and advances to
`WaitingForSpawnPoint` and then `WaitingForPlayerSpawn`. Committed in `1268829`.

### Type 1635 — LevelInfoChangedMsg

Needed so the client loads the level/context. Layout taken from existing data,
not a fresh decompile: the field list of
`local-data/azoth-catalog/packets/1635.json` plus the captured body length
(open world is 106 bytes, from `scratch-bootstrap/layout-profile.txt`). The
published field list sums to 89 bytes; the 17-byte remainder is exactly one
phasing entry. The entry comes **right after the phasing count**, before the
flag bytes (`flagA0`, `clientCtx`, `isMaster`, `flagA3`, `instance`): a first
live try with the entry at the end made the client read `isMaster` as a zero
byte and log "LevelInfoChanged javContext.IsMasterPlayer is false".

Open-world body (big-endian), 106 bytes:

| Field | Type | Value |
|---|---|---|
| map name | vlq32 len + utf8 | `NewWorld_VitaeEterna` |
| coatlicue path | vlq32 len + utf8 | `coatlicue/NewWorld_VitaeEterna` |
| phasing origin X, Y | 2x f32 | 2048, 16 |
| phasing bounds X, Y | 2x f32 | 12272, 10250 |
| reserved | u64 | 0 |
| phasing count | vlq32 | 1 |
| phasing entry | u8 keyType, u32 keyId, u64 phaseId (!=0), u32 extra | 1, 0, 1, 0 |
| flagA0 | u8 | 1 |
| clientCtx | u8 | 1 (every state bundle must carry this) |
| isMaster | u8 | 1 (client drops the message if 0) |
| flagA3 | u8 | 0 |
| instance | u64 | a non-zero clock, must differ from the previous |

Live: Game.log logs "LevelInfoChanged m_clientContextInstanceId 0 changing to
levelInfo.m_clientContextInstanceId 1" (accepted, no "IsMasterPlayer is false").

## The remaining wall: WaitingForPlayerSpawn -> InGame

The client stays in `WaitingForPlayerSpawn` and logs
`GameConnectionWrapper::OnLocalNetworkConnectionWarning` every few seconds. No
`map NewWorld_VitaeEterna` load line and no `InGame` transition appear.

Per resolved item 7, `0xd` -> `0xe` needs all of: `LoadContextAndLevel`
(`FUN_146448cd0`) to have run (it sets `actor_game+0x252` and issues `map %s`,
after both 1628 and 1635); the client context pointer (`FUN_141026080`) to be
non-null; and the local player entity to be spawned (its vtable `+0x30`).

So the open work is the **channel-1 world-state stream**: the client, now on
client context 1, waits for its own player entity in the state bundles
(`ReplicatedStateBundle`, type 8, channel 1). The sim already puts each
player's own member in its bundles (`bins/sim/src/world.rs`), but it is not yet
confirmed that the gateway forwards channel-1 bundles to this real client's
connection, nor that the bundle carries the client-context id (1) the client
now expects, nor that the player's own entity is marked so the client treats it
as the local player. The `0xDDD1000` error after registration is logged once
and is not fatal (the client proceeds past it every time).

Next steps:
1. Confirm the gateway delivers channel-1 bundles to the real client's
   connection (add a per-connection send count, or capture the UDP).
2. Confirm the bundle header carries client-context 1 (candidate fields:
   `byte_3` / `byte_4`, both 1 in captures; see bundle header note).
3. Confirm the player's own entity (matching `ClientAddEntryMsg`'s entity
   number) is in the stream and recognised as the local player.

## How the live test runs

`scripts/live-stack.sh` forces edge to 8443 for `nw-testclient`. The real
client needs edge on 443, so this test used a scratch variant: the setcap'd
`~/.local/nw-edge/edge-http` with `local-data/edge-http/edge.toml`
(`https_listen = 0.0.0.0:443`), plus `world-director`, `sim`, `rep-gateway` on
udp 33435, all with `NW_GAME_DIR` set and the `m3-test.env` secrets.
`login/game.faqfirebase.com` resolve to 127.0.0.1 in `/etc/hosts`. Launch the
patched client with the launcher's direct-Proton env vars (EAC off). Game.log:
`<prefix>/drive_c/users/steamuser/AppData/Local/AGS/New World/Game.log`.
