# Network warning icon and the exit buttons

Date: 2026-09-10
Build: Steam `25810808` (`NewWorld.exe`)
Method: static analysis only. The Ghidra project `<re-scratch>/ghidra-25810808` was opened read-only with analysis off (`analyzeHeadless -noanalysis -readOnly`). Dumps are under `<re-scratch>/out-netwarn/`. Captures were read with a scratch tool built on `nw-capture` and `nw-carrier`; it printed counts only, no payload bytes. Logs: the 2026-09-10 17:37 live run (`Game.log`, `local-data/logs/live-realclient/sim-1.log`). No live launch was done for this note.

Labels: **Observed** = seen in code, captures, or logs. **Inferred** = a conclusion from Observed facts. **Unknown** = not established.

## 1. What raises `OnLocalNetworkConnectionWarning`

**Observed.** The log line comes from `FUN_14644ee50`. It is slot 1 of the handler table `0x1484fdd00` inside `GameConnectionWrapper` (constructor `FUN_1464001c0` stores it at `this+0x30`). Slot 0 is `OnRemoteNetworkConnectionWarning` (`FUN_14644f6f0`). The handler logs `countReceived` and pushes the time into three windows (`+0x38`, `+0x60`, `+0x88`). `GameConnectionWrapper::UpdateWarnings` (`FUN_146470070`) reads those windows to set `Client.ConnectionWarning` (the icon) with the `client-connection.warnings.*` settings.

**Observed.** The GridMate event queue drain `FUN_145dd6860` calls the handler: event type 9 calls slot 1 (local), type 8 calls slot 0 (remote), with the count at event `+0x48`.

**Observed.** The carrier update `FUN_145de0020` queues the type-9 event. For each connection it tests the byte at connection `+0x1319`. When it is set, it clears it, adds 1 to the count at `+0x13bc`, sends system message 7 to the peer with that count as a big-endian u32 (`FUN_145ddb5f0(this, 7, ...)`), and queues event 9.

**Observed.** So carrier system opcode 7 (5 bytes `00 00 xx xx 07`, 196 in the captures, carrier note section 4) is the connection warning: the sender's warning count. A peer that receives it raises `OnRemoteNetworkConnectionWarning`.

**Observed.** The warnings in the live run fall on whole-second steps of each other (for example 17:37:36.190, 39.194; 17:38:08.255, 10.260, 13.256). The check runs on a fixed tick.

**Observed.** The flags at connection `+0x1318` are given to the traffic-control check. The carrier disconnect pass `FUN_145dd56b0` passes `conn+0x1318` to traffic-control slot `+0x78` with a float threshold. The default traffic control (table `0x1484819b0`, built in `FUN_145dbbfd0` when the carrier description has none) has slot `+0x78` = `FUN_140f82260`. It copies the connection statistics and sets `+0x131a` when a statistic is above the threshold.

**Unknown.** The instruction that sets `+0x1319` to 1. A search of every 32-bit displacement `0x1319` in `.text` finds only the test and the clear in `FUN_145de0020` (and its copy at `0x140f8ae6b`). The writer uses another base or a pointer.

**Inferred.** The flag comes from the traffic-control connection check, which works from datagram acks (round trip and loss).

## 2. What our server did differently from retail

**Observed (captures, two ledgers, counts only).** Retail acks every datagram of the peer, not only datagrams with a reliable frame:

| Sender | Datagram kind | Datagrams | Acked by the peer |
|---|---|---|---|
| server | reliable | 419 | 419 |
| server | unreliable data only | 7,249 | 7,245 |
| server | system only | 46 | 46 |
| client | reliable | 212 | 211 |
| client | unreliable data only | 2,777 | 2,776 |
| client | system only | 9 | 9 |

**Observed.** `nw-carrier` acked only datagrams with a reliable frame. The client sends its frequent input and many ack-only datagrams without a reliable frame. Our server never acked them.

**Inferred.** The client counted those datagrams as lost. That is the most likely cause of the warning flag. A live retest must confirm it.

**Observed (captures).** Every unreliable frame carries, as its reliable sequence, the number of the last reliable frame sent on its channel: difference 0 in 6,055 server frames (4 frames show 1, after loss). A client unreliable frame sent before any reliable frame on its channel carries 65535.

**Observed.** `nw-carrier` put the next reliable number (last + 1) in unreliable frames. That names a reliable frame not yet sent. The real client ignored every such frame (2026-09-08 live note, "BREAKTHROUGH"), so the stack had to send bundles reliably (`NW_RELIABLE_BUNDLES=1`).

**Observed.** The frame encoder itself is byte-exact: 17,706 captured datagrams decoded and encoded again give the same bytes. Unreliable channel-1 frames in the captures use flags `0x20` (server, 11,265) and `0x08` (client, 72). Only the value in the reliable-sequence field was wrong.

## 3. Fix (nw-carrier)

- `Sender`: an unreliable frame carries the last reliable number sent on its channel (65535 before any).
- `Receiver`: every datagram is acked. A datagram that holds only acks asks for no ack; it rides with the next ack that goes out. An idle connection sends those pending acks in place of a keepalive. This keeps two idle peers from acking each other without end.
- `SystemMessage::ConnectionWarning { count }` replaces `ConnectionControl`. `rep-gateway` logs it at debug (`client raised a connection warning`), so a live test can count warnings on the server side.

With these, channel-1 bundles can go unreliable again. Large bundles are still split into reliable chunks, as retail does.

## 4. Exit to desktop and exit to main menu

**Observed.** In the live run the client sent no new message type when the exit buttons were used. The client messages after spawn (sim debug log) are all component requests: 2728 `WaypointsComponentServerFacet_RequestSetWaypoint`, 2709 `ObjectivesComponentServerFacet_SetObjectiveAutoPinning`, 1094 `PlayerComponentServerFacet_OnSetBandwidthModeRequest`, 3967 `PlayerComponentServerFacet_OnCullLimitPctChangeRequest`, 4143 `SocialComponentServerFacet_SendAvailableTitlesToClient`, 2617 `ObjectivesComponentServerFacet_NotifyOpenUiScreen`, 2556 `ContractsComponentServerFacet_LookupContractsByParticipant`, 843 `MetaAchievementComponentServerFacet_ServerRequestGetPendingMetaAchievements`, 6444 `SocialComponentServerFacet_RequestCrossplayPermissionsDataSync`, and at spawn 4272, 4122, 6268, 6271, 4142, 1643, 4248, 2515, 5463, 3092, 3970, 2776 (names from `local-data/azoth-catalog/packets`). None is a logout request.

**Observed.** The script method `RequestDisconnect` (bound in `FUN_144f418b0` through the thunk `0x14029e29c`, interface slot `+0x50`) is `FUN_141043770`. It sends nothing to the server. It queues a lambda (`FUN_1410042a0`) on the tick queue, or, while an async unload runs, stores the destination and logs `Async unload active, pending destination set to: `.

**Observed.** The lambda, destination 0 (desktop): logs `Calling IAuthManager::GetInstance()->EndSession()` and calls the Steam auth `EndSession`. Destinations 1 and 2 (main menu, attract screen): `CGame::ActorContainerDisconnect` (`FUN_1464218d0`, logs that name first), then the game's exit slot `+0x1d8`.

**Observed.** The live `Game.log` has `Calling IAuthManager::GetInstance()->EndSession()` at 17:39:39.764 and nothing after it from this path. The process then stays open. This is the same wait as in `2026-09-02-character-select-blockers.md` section 4: Steam `EndSession` stores the quit callback and runs it only through the Omni persona client, which this client does not have. That wait is inside the client. No server reply ends it.

**Observed.** The live `Game.log` has no `CGame::ActorContainerDisconnect`, no `CGame exiting to main menu`, and no `Async unload active`. The main-menu lambda did not run in this run.

**Unknown.** Why "Exit to main menu" did nothing. The log has no line from the main-menu path. A live check that clicks only that button is needed.

**Conclusion.** There is no logout message to implement in the sim: neither exit sends one. Exit to desktop hangs in the client's Steam sign-out. A fix for it is a client patch (for example the `BlightedNewWorld.exe` patch set), not a server reply.

## Evidence

- Ghidra dumps: `<re-scratch>/out-netwarn/` (`fn_14644ee50.c`, `s2/fn_146470070.c`, `src/fn_145dd6860.c`, `range/fn_145de0020.c`, `range/fn_145dd56b0.c`, `tc3/fn_140f82260.c`, `rd3/fn_141043770.c`, `rd3/fn_1410042a0.c`, `mm/fn_1464218d0.c`).
- New read-only script: `<re-scratch>/scripts/RangeDecomp.java`.
- Capture tools (scratch, not in the repository): counts of unreliable reliable-sequence values, peer acks per datagram kind, and the decode-encode byte check.
