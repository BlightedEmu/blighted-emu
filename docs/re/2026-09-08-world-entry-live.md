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

## Deep dive 2026-09-08 (overnight): why the player does not spawn

Ran more decompiles (read-only, reusing the Ghidra project at
`<re-scratch>/ghidra-25810808`; helper scripts `scripts/StringFuncs.java`
and `scripts/Callers.java`). Build-25810808 addresses.

### The InGame trigger is a state bundle, not a message

`FUN_14644b330` (state machine, old `FUN_14644a070`) case `0xd`: InGame needs
all of `FUN_145a934f0(actor_game)` (`+0x252`, map loaded) != 0, `FUN_141026220()`
(client context) != 0, and its vtable `+0x30` != 0 (local player entity
spawned).

`FUN_146449f90` (LoadContextAndLevel, old `FUN_146448cd0`) is the only thing
that issues `map %s` and sets `+0x252`. It is NOT called by the 1628 or 1635
handlers. Its one caller is `FUN_14643fa70` — the GameMessagePort handler for
incoming replication. When a type-8 `ReplicatedStateBundle` arrives whose
clientContextInstanceId equals the port's expected context and the context
object is still null, `FUN_14643fa70` calls LoadContextAndLevel and logs
"HandleUnexpectedMessage skipping ReceiveStateBundle for bundle message due to
clientContext == nullptr". So the client bootstraps its context/level from the
FIRST matching state bundle. No message on channel 0 does this.

### The bundle's clientContext is the step-3 header byte, and ours matches

`FUN_1406c1240` returns the in-memory bundle byte `+0x30`. The type-8 header
reader `FUN_146b08e40` (old `FUN_146b07870`) writes the wire step-3 byte to
`+0x30` (step 1/2 -> `+0x28`, step 3 -> `+0x30`, step 4 -> `+0x31`, step-5/6
flags -> `+0x8`/`+0x9`, index block -> `+0x38`, payload -> `+0x110`). So
clientContextInstanceId = bundle `byte_3`. The sim sets `byte_3 = 1`
(`OBSERVED_BUNDLE_BYTE_3`) and 1635 sends clientCtx = 1, so they already match.

### The actual blocker: no channel-1 bundle reaches the client

Game.log shows NONE of `FUN_14643fa70`'s log lines (no "skipping
ReceiveStateBundle…", no "MayHandleReplicationUnreliable returning false", no
"received bundle for a previous ClientContext"). So the client's game port is
not receiving any type-8 bundle. With the context byte already matching, the
only explanation is delivery: the real client is not getting the sim's
channel-1 (unreliable) state bundles on the game connection.

Live metrics at the time: `sim_players 0`, `sim_deaths_total{player} 1`,
`rep_gateway_datagrams_out_total 2314`. The player joined with full health
(`world.rs` admit: `Health::full(player_max_health())`) but then was killed by
mobs while idling ~60 s at WaitingForPlayerSpawn (115,522 mobs in the world);
that death is a consequence of not spawning, not the cause.

### Leading hypothesis for the morning

The real client's "actor game connection" (the second channel it starts right
after registration) may be where it expects the state-bundle stream, separate
from how `nw-testclient` (one connection) receives it. The emulator sends
bundles on carrier channel 1 of the one REP connection; confirm whether the
real client reads bundles there or on a distinct actor-game channel/connection.

### Server send path verified (so the blocker is client-side or was masked)

`rep-gateway` peer `send_envelopes` (peer.rs): once `Phase::Registered`, it
sends every channel via `send_now` -> `carrier.send(channel, reliable, ...)`,
including channel-1 unreliable bundles. During ticket checking it holds
reliable messages and drops unreliable ones, but the player had registered, so
channel-1 bundles were being sent. So the gateway delivery path is correct
(the same path `nw-testclient` uses to see positions).

Two things therefore remain for the morning: (a) in the 04:19 run the player
died almost immediately (no spawn protection yet, commit 81a5b09 adds it), so
few/no bundles may have gone out before it left replication; with protection
the player survives and bundles flow every tick, so the first matching-context
bundle should reach the client -> retest first. (b) if the client still gets no
bundle, it is the real client's channel-1 ingestion on its actor game
connection (client-side), which the UDP capture will show.

Morning plan:
1. Retest with spawn protection (81a5b09): the player no longer dies, so
   channel-1 bundles flow the whole time; check Game.log for "skipping
   ReceiveStateBundle" / "map NewWorld_VitaeEterna" / InGame.
2. If still stuck, capture the UDP the gateway sends to the real client
   (`sudo tcpdump -i lo -w /tmp/nw.pcap udp port 33435` during the test) and
   compare channel-1 frames to a retail capture. (or add a per-connection
   channel-1 send counter) to see whether channel-1 bundles are actually sent to
   this connection and in what form.
2. If sent but ignored, RE how the client sets up the actor game connection and
   where it binds the bundle receive (around `FUN_145a905d0` /
   `CJavelinActorGame` and the gridmate channel setup).
3. If not sent, fix the gateway so joined real-client connections get channel-1
   bundles.
4. Separately, stop the idle player being killed by mobs before InGame (spawn
   protection or no mob aggro until spawned) so the death does not confuse the
   next test.

## Morning retest 2026-09-08 (spawn protection build 81a5b09)

Spawn protection works: `sim_players 1` and no death during the wait (the idle
player is no longer killed by mobs). The client still stops at
WaitingForPlayerSpawn and loops `OnLocalNetworkConnectionWarning`, and Game.log
still shows NONE of `FUN_14643fa70`'s bundle-handler lines. Metrics during the
run: `datagrams_out ~2580`, `datagrams_in ~819` (the client is sending us acks/
pings), `sim_players 1`.

So, firmly isolated: the real client ingests channel-0 reliable messages fully
(it processed every login message including 1635) but does NOT ingest our
channel-1 UNRELIABLE state bundles. The gateway sends them (verified in
`send_envelopes`), so this is the carrier framing/handling of unreliable
channel-1 frames — which `nw-carrier` has only ever exchanged with our own
`nw-testclient`, never with the real gridmate client.

A raw UDP capture does not help: the REP traffic is DTLS-encrypted, so tcpdump
would only show ciphertext (and no capture tool is installed anyway). Plaintext
frames are only visible server-side (log them in `nw-carrier`/gateway before
DTLS) or client-side (Frida on the receive path).

### Candidate next steps (pick one)
1. Bisection: send the bundles RELIABLY on channel 1 for one test (reliable
   frames share the framing that channel 0 already works with). If the client
   then ingests them, the bug is specifically our unreliable framing; if not,
   it is channel 1 itself. (sim world.rs ~line 1000 `reliable: false`.)
2. Log the plaintext channel-1 frames the gateway sends (flags/channel/seq) in
   `nw-carrier` and compare to the carrier note's documented frame format
   (docs/re/2026-09-02-carrier-captures.md), looking for an unreliable-frame
   flag/sequence mismatch.
3. Frida-instrument the client's receive path (`FUN_140f89a80` and the frame
   dispatch) to see why it drops channel-1 unreliable frames.

## How the live test runs

`scripts/live-stack.sh` forces edge to 8443 for `nw-testclient`. The real
client needs edge on 443, so this test used a scratch variant: the setcap'd
`~/.local/nw-edge/edge-http` with `local-data/edge-http/edge.toml`
(`https_listen = 0.0.0.0:443`), plus `world-director`, `sim`, `rep-gateway` on
udp 33435, all with `NW_GAME_DIR` set and the `m3-test.env` secrets.
`login/game.faqfirebase.com` resolve to 127.0.0.1 in `/etc/hosts`. Launch the
patched client with the launcher's direct-Proton env vars (EAC off). Game.log:
`<prefix>/drive_c/users/steamuser/AppData/Local/AGS/New World/Game.log`.
