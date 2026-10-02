# Post-registration sequence

Date: 2026-09-02.

This note checks first-light claims against our ledger captures and our type table. It does not copy their code. The game was not run. No new traffic was recorded. Easy Anti-Cheat was not analyzed.

Labels: **Observed** means a capture, a type table, a packet file, or a JSON file showed the fact. **Inferred** means a conclusion from those facts. **Unknown** means the files do not show it. Each open item says why.

## Sources

- **Observed.** 285 `ledger.bin` files under `local-data/captures`. Two full parses and one part-count parse. Decode errors: 0. Datagrams: 7,211,869. Duplicate reliable frames dropped: 1,522.
- **Observed.** Type table `<game-dir>/typeindex.json`. Key `typeIndex`. 7,052 UUIDs. The list position is the type index. UUIDs have no hyphens.
- **Observed.** Name map `local-data/Aeternum-World-main/Catalog/uuid_to_class.json`. 3,486 entries. UUID keys have hyphens. Every `type_idx` equals the type-table position. Difference count: 0.
- **Observed.** Packet text `local-data/azoth-catalog/packets/<idx>.json`. The purpose text is the `what` field. The file has no key named `purpose`.
- **Observed.** HTTP archives under `local-data/open world captures`. Compare note: `docs/re/2026-09-02-login-json-shapes.md`.
- **Observed.** First-light sequence text: `local-data/first-light-main/docs/post-v3-sequence.md` and `post-v3-flow.md`. Codec size claims: files under `local-data/first-light-main/server/javelin/`. Facts only.
- **Observed.** Carrier rules: `docs/re/2026-09-02-carrier-captures.md`. LZ4 blocks were unpacked before the frame decode. Envelope flag `0x04` did not occur.

**Observed.** These parses count 7,211,869 datagrams, the same total as the committed carrier note (`content-tool capture-check`). An earlier draft of that note said 7,179,741; that number came from a first pass that stopped at two decode failures.

Privacy: this file has counts, sizes, type indexes, class names, key names, and value kinds. It has no message body bytes, player names, chat, account ids, Steam ids, tokens, ticket ids, IP addresses, or session strings.

## 1. Sequence check

A window starts at each server `RegistrationResponseMsg` (type 3). The next 200 game messages on that context are the window. Channel 3 is skipped. Chunks are joined before a message is counted. Order is record time.

**Observed.** 43 contexts have a type-3 message. They are in 3 files. Every window has 200 messages. The 200th message is at 897 ms to 2,784 ms (median 2,087 ms).

**Observed.** Every type-3 message is server to client, channel 0, reliable. Body length 85. Envelope length 88. Class `REPClient::RegistrationResponseMsg`.

**Observed.** Example window: `Hive Expedition Genesis/dtls/ledger.bin`, context index 14. That index is the 15th context in the file. It is not a network address. The tail after type 3 has 12,617 messages. The 200th message is at 2,230 ms.

### Common order

**Observed.** The first five game messages have the same type indexes on all 43 contexts.

| Slot | Offset ms | Direction | Channel | Reliable | Type | Class | Body |
|---|---|---|---|---|---|---|---|
| 1 | 5 to 103, median 59 | server to client | 0 | yes | 349 | `REPClient::PingMsg` | 8 |
| 2 | 10 to 106, median 63 | server to client | 0 | yes | 446 | `CoatlicueClientMessages::CoatlicueTimingConfigurationConnectedMsg` | 72 |
| 3 | same time as slot 2 | server to client | 0 | yes | 1034 | `CoatlicueClientMessages::CalendarConnectedMsg` | 72 |
| 4 | 10 to 117, median 64 | client to server | 0 | yes | 349 | `REPClient::PingMsg` | 8 |
| 5 | 46 to 322, median 109 | server to client | 0 | yes | 1628 | `Javelin::ClientMessagesTrait::PlayerManagerSelfIdentificationMsg` | 12,702 |

**Observed.** Slots 6 and 7 share the time of slot 5. Type 164 is first on 41 contexts. Type 1617 is first on 2 contexts.

| Type | Class | Body |
|---|---|---|
| 164 | `ClientActorRoutingAuthorizationTrait::ClientAddEntryMsg` | 16 |
| 1617 | `Javelin::ClientViewListenerTrait::ReceivePlayerSpawnPointMsg` | 0 |

**Inferred.** Several first-light sizes match the envelope length. The envelope is 1 flag byte, 1 presence byte, the type prefix, and the body. Types 446 and 1034 use a 2-byte type prefix, so body 72 has envelope 76.

**Observed.** Later server `PingMsg` gaps are about 1 s (4,665 gaps in 1,000 ms to 1,099 ms). Server `TimeSynchMsg` gaps are about 3 s (1,596 gaps in 3,000 ms to 3,099 ms).

### Step verdicts

The step text is from `post-v3-sequence.md`. Wire bytes are mapped with our prefix code. Class names are from the UUID join, except type 3051.

| Step | First-light label | Our type | Class | Verdict |
|---|---|---|---|---|
| 1 | VERSION `0x03`, 89 bytes | 3 | `REPClient::RegistrationResponseMsg` | Corrected. Body 85. Envelope 88. |
| 2 | HEARTBEAT `0x9d`, 13 bytes, +50 ms | 349 | `REPClient::PingMsg` | Corrected. Body 8. Envelope 12. First delay median 59 ms. Later period about 1 s. |
| 3 | INIT `0x8a` and `0xbe`, both 76 bytes | 1034 and 446 | calendar and timing messages above | Corrected. Envelope 76 matches. Classes are calendar and timing. Type 446 is before type 1034. |
| 4 | WORLD DATA `0x9c`, 12.7 KB, 12 parts | 1628 | `PlayerManagerSelfIdentificationMsg` | Corrected. Body 12,702. 12 parts on all 43. The class is self-identification. |
| 5 | `0x91(0x19)` and small `0xa4` | 1617 and 164 | spawn point and `ClientAddEntryMsg` | Corrected. Spawn body 0, envelope 4. Add-entry body 16, envelope 20. |
| 6 | SESSION `0xa4`, 75 or 195 bytes | 3236 | `EventNotificationComponentClientFacet_SyncNotifications` | Corrected. Body 122 (22 windows) or 154 (21 windows). |
| 7 | HEARTBEAT `0x8f`, 13 bytes, 500 ms | 335 | `REPClient::TimeSynchMsg` | Corrected. Body 8. Common gap about 3 s. |
| 8 | `0xa6` and `0x88` | 6310 and 7048 | `UpdatePhasing` and `ReceiveStorageSearchItems` | Corrected. Phasing body 19 to 121. Storage body 38. |
| 9 | `0x88` plus a small `0xa4` | 7048 and 164 | same classes as steps 5 and 8 | Corrected. No new type index. |
| 9b | SelfIdentification `0x91(0x17)`, 4 bytes | 1489 | `Javelin::CharacterServiceProxyActor` | Not found. Absent after type 3 on all 43 contexts. |
| 10 | `0x88` times 20 | 7048 | `ReceiveStorageSearchItems` | Corrected. A run of 20 or more is on 9 of 43 windows. The common run is 2. |
| 11 | SESSION `0xaa`, 29 bytes | 4970 | `OnLeftQueueForGameModeMsg` | Corrected. Body 24. Envelope 28. |
| 11a | WORLD SPAWN `0xa3` | 1635 | `LevelInfoChangedMsg` | Corrected. Body 112, 157, or 106. |
| 11b | Channel 1, 47 units, 285 KB | 8 | `Amazon::Hub::ReplicatedStateBundle` | Corrected. Channel 1 count in the window is 42 to 120 (median 98). Median body sum is 457,127. |
| 12 and 13 | SESSION `0xae`, 22 bytes, two trail values | 4142 | `RequestStreamerModeDataSync` | Not found as a 22-byte server form. The `0xae` message present is client to server, body 18, on all 43 tails. |
| 14 | ENTITY DEFS `0x95` plus large `0x9d` plus `0xa0` | 2709, 349, 608 | auto-pin, ping, and `AdjustFXScripts` | Corrected. These are separate messages. They are not one 2.5 KB group. |
| 15 | `0xb3` plus VIVOX `0xa7` | 4147 and 4199 | titles list and `OnVoiceChatServiceRegistered` | Corrected. Bodies 655 to 911, and 92. |
| 16 | SPAWN `0x96` and `0x97` | 4246 and 4247 | `SyncTimeChange` and `SyncDayPhase` | Corrected. Bodies 76 and 20. These are time messages. |
| 17 | Continuous `0x08` on channel 0 | 8 | `ReplicatedStateBundle` | Corrected. The stream is on channel 1. |
| 18 | Player data `0xa0`, 234 KB, 210 parts | 5792 | `OnQueryInventories` | Corrected. One message has body 233,564 and 210 parts. See the part table. |
| 19 | Player data `0xa0`, 171 KB, 154 parts | none | none | Not found. No body near 171,000 bytes. No 154-part message of this type. |
| 20 | Entity `0xac`, 2.6 KB, chunked | 4140 | `PlayerDataResponse` | Corrected. Largest body is 2,433 in 3 parts. |
| 21 | `0x8f` and `0x9d` every 500 ms | 335 and 349 | time sync and ping | Corrected. The periods are about 3 s and about 1 s. |
| 22 | Continuous `0x94`, `0xa5`, `0xac`, `0x9a` | several | see the wire table | Corrected. These prefixes are more than one class. They are not one loop. |

**Observed.** Type 5792 after registration. Three messages.

| Parts | Body |
|---|---|
| 9 | 8,965 |
| 200 | 222,931 |
| 210 | 233,564 |

The 210-part count matches the first-light segment count for the large form. The body is 233,564 bytes. One capture has this large form. It is inside the first 200 messages on 1 of 43 windows.

**Observed.** Type 4140 after registration. 152 messages. 100 messages are 1 part (body 181 to 483). 27 messages are 2 parts (body 186, 1,585, or 1,825). 25 messages are 3 parts (body 2,275, 2,426, or 2,433).

**Observed.** These types are on all 43 contexts: 349, 446, 1034, 1628, 164, 1617, 3236, 335, 7048, 4970, 1635, 8, 4246, 4247, 4199, 4147, 2278, 2556, 6310, 6745, 4140, 4142.

**Observed.** Type 19 (`RegistrationRequestV3Msg`) is absent after type 3. The request is before the response. A second type 3 is absent.

**Observed.** Wire prefix `0x94` also matches type 6484, class `EOSAntiCheatClientTrait::UpdateMsg`, on all 43 tails, body 25. This note does not describe that message.

### Counter pairs

| Pair | Verdict |
|---|---|
| `0x18a6` server and `0x1a59` client | Corrected. Types 6310 and 6745. Classes `UpdatePhasing` and `ClientAckPhasingUpdate`. Ack body 17. Phasing body 19 to 121. |
| `0x15d` ping and ack | Corrected. Both directions are type 349. Both bodies are 8. |
| `0x14f` with no client reply | Corrected on direction. Type 335 is server to client only. Body 8. |
| `0x8e6` server and `0x9fc` client | Corrected. Types 2278 and 2556. Contract lookup. Server body 38 to 4,448. Client body 72. |

**Unknown.** Whether a counter byte matches inside the phasing pair, and whether the contract pair shares 16 body bytes. Those checks read body bytes. Body bytes are not reported.

**Unknown.** The trail byte that first-light uses to split the two `0xae` steps. That byte is a body byte. It was not read. No server body of length 22 uses that prefix.

### Messages they do not mention

**Observed.** These types are in the first 200 messages and are not assigned to a step above. The offset is the median first-seen offset in milliseconds. Counts are windows out of 43.

| Type | Class | Windows | Offset ms | Direction | Channel | Reliable | Body |
|---|---|---|---|---|---|---|---|
| 2515 | `PlayerSettingsComponentServerFacet_ReceiveSettingsFromClient` | 43 | 933 | client to server | 0 | yes | 20 |
| 3639 | `GuildsComponentServerFacet_RequestGetGuilds` | 43 | 925 | client to server | 0 | yes | 49 |
| 4248 | `TimeComponentServerFacet_SyncToClient` | 43 | 933 | client to server | 0 | yes | 16 |
| 5429 | `GuildsComponentClientFacet_OnGuildDataUpdated` | 43 | 226 | server to client | 0 | yes | 32 |
| 1234 | `OnQueueGroupRemovedMsg` | 42 | 186 | server to client | 0 | yes | 16 |
| 1458 | `ObjectivesComponentServerFacet_UpdateTrackedObjectives` | 42 | 962 | client to server | 0 | yes | 17, 33, or 41 |
| 4122 | `SpectatedPlayerComponentServerFacet_SetIsPublic` | 42 | 941 | client to server | 0 | yes | 17 |
| 4143 | `SocialComponentServerFacet_SendAvailableTitlesToClient` | 42 | 949 | client to server | 0 | yes | 16 |
| 6268 | `MountComponentServerFacet_OnMountDashToggleChanged` | 42 | 941 | client to server | 0 | yes | 17 |
| 6271 | `MountComponentServerFacet_OnMountSlowWalkChanged` | 42 | 945 | client to server | 0 | yes | 17 |
| 6444 | `RequestCrossplayPermissionsDataSync` | 42 | 949 | client to server | 0 | yes | 26 |
| 1589 | `ActionListComponentServerFacet_ReceiveInputFromClient` | 41 | 1375 | client to server | 1 | no | varies |
| 1643 | `VoiceChatComponentServerFacet_RegisterVoiceChatService` | 41 | 978 | client to server | 0 | yes | 16 |
| 3610 | `GuildsComponentClientFacet_OnGetUnorderedGuildsMsg` | 41 | 965 | server to client | 0 | yes | 150 or 157 |
| 998 | `ActionListComponentServerFacet_SetInputsThrottled` | 40 | 1422 | client to server | 0 | yes | 17 |
| 2407 | `GroupsComponentServerFacet_RequestPing` | 40 | 978 | client to server | 0 | yes | 30 |
| 2724 | `GatherableControllerComponentClientFacet_GatheringInterrupted` | 28 | 268 | server to client | 0 | yes | 16 |
| 1094 | `PlayerComponentServerFacet_OnSetBandwidthModeRequest` | 27 | 1741 | client to server | 0 | yes | 20 |
| 2077 | `VoiceChatComponentServerFacet_GenerateLoginAccessToken` | 21 | 1192 | client to server | 0 | yes | 18 |
| 3051 | `VoiceChatComponentClientFacet_OnAccessTokenReceived` | 21 | 1222 | server to client | 0 | yes | 271, 414, or 460 |
| 608 | `StatusEffectsComponentClientFacet_AdjustFXScripts` | 20 | 212 | server to client | 0 | yes | 21 |
| 188 | `SyncVoiceChatClientActivationState` | 15 | 1818 | client to server | 0 | yes | 17 |
| 5458 | `OnJoinableLobbyCountsForActivityReceived` | 15 | 1995 | server to client | 0 | yes | 547 |
| 5463 | `RequestJoinableLobbyCountsForActivity` | 15 | 1944 | client to server | 0 | yes | 17 |
| 4204 | `VoiceChatComponentServerFacet_RequestGroupChannelUri` | 14 | 1818 | client to server | 0 | yes | 16 |
| 4200 | `VoiceChatComponentClientFacet_On2dChannelUriReceived` | 12 | 1849 | server to client | 0 | yes | 198 |
| 2934 | `VoiceChatComponentServerFacet_GenerateJoinAccessToken` | 11 | 1895 | client to server | 0 | yes | 66 or 31 |
| 1793 | `CharacterComponentClientFacet_RequestDisableCollision` | 7 | 288 | server to client | 0 | yes | 16 |
| 3135 | `TriggerEntityComponentClientFacet_SyncExecuteEntities` | 2 | 290 | server to client | 0 | yes | 16 |
| 3916 | `AITargetableComponentClientFacet_OnUnSelectedAsTarget` | 2 | 169 | server to client | 0 | yes | 24 |
| 405 | `VoiceChatComponentClientFacet_On3dChannelUrisReceived` | 1 | 2248 | server to client | 0 | yes | 239 |
| 2805 | `VoiceChatComponentServerFacet_RequestWorldChannelUris` | 1 | 2217 | client to server | 0 | yes | 16 |
| 4118 | `ChatComponentClientFacet_ReceiveChatMessage` | 1 | 201 | server to client | 0 | yes | 127 |

Class names use the short suffix when the full name is in section 3. Type 1589 is the client channel-1 input message. Its common bodies in one example window step by 15.

**Observed.** Type 3051. The name-map entry exists and the name string is empty. `packets/3051.json` names `Javelin::ClientMessages::VoiceChatComponentClientFacet_OnAccessTokenReceived`. A search of `NewWorld.exe` found no text form of that type-table UUID. The class name in this note is the packet-file name.

## 2. Codec body sizes

**Observed.** 29 codec files. 17 match the claimed body size or size rule. 12 do not. 0 are absent from the captures.

A file title is often not a class name. The file integer is the type index. The class is the name-map class at that index. When a codec states a size that includes the type header, the claimed body is that size minus the header (3 bytes under index 128, 4 bytes from 128 through 16,383). A full client record also drops the 24 bytes before the envelope. That subtraction is **Inferred**. The verdict names it.

| File | Type | Class | Verdict | Claimed body | Observed body |
|---|---|---|---|---|---|
| `empty_marker_651.py` | 1617 | `ReceivePlayerSpawnPointMsg` | Match | 0 | 0 (43) |
| `frame_config_1096.py` | 4246 | `SyncTimeChange` | Match | 76 | 76 (270) |
| `handshake_blob_76.py` | 446 and 1034 | timing and calendar | Match | 72 | 72 (43 each) |
| `heartbeat_15d.py` | 349 | `PingMsg` | Match | 8 | 8 (78,413 each way) |
| `identity_fingerprint_5b2.py` | 1458 | `UpdateTrackedObjectives` | Match | 17 + 8 × count | 17, 25, 33, 41, 49 |
| `keybinding_config_12f6.py` | 4854 | `OnTelemetryOptionsDataChanged` | Match | 271 | 271 (50) |
| `level_info_changed.py` | 1635 | `LevelInfoChangedMsg` | Match | minimum 48 | 106, 112, 157 |
| `permission_bitmap_a95.py` | 2709 | `SetObjectiveAutoPinning` | Match | 53 when count is 36 | 53 (722) |
| `result_token_1097.py` | 4247 | `SyncDayPhase` | Match | 20 | 20 (348) |
| `result_token_136a.py` | 4970 | `OnLeftQueueForGameModeMsg` | Match | 24 | 24 (171) |
| `session_clock_beacon.py` | 335 | `TimeSynchMsg` | Match | 8 | 8 (26,162) |
| `session_message_a4.py` | 164 | `ClientAddEntryMsg` | Match | 16 | 16 (94) |
| `session_subkey_1a59.py` | 6745 | `ClientAckPhasingUpdate` | Match | 17 | 17 (6,826) |
| `subkey_beacon.py` | 15 indexes | see scratch note | Match | 16 + trailer | one length each, all on the rule |
| `v3_response.py` | 3 | `RegistrationResponseMsg` | Match | 85 | 85 (43) |
| `vivox_config_1067.py` | 4199 | `OnVoiceChatServiceRegistered` | Match | 19 + three string lengths | 92 (51). Example body 82 is absent. |
| `world_data_blob_65c.py` | 1628 | `PlayerManagerSelfIdentificationMsg` | Match | 12,702 | 12,702 (43) |
| `action_history_635.py` | 1589 | `ReceiveInputFromClient` | Mismatch | 65 + 15 × count | 554 lengths, from 63 to 720. 142,309 lengths sit on the rule. |
| `asset_blob_16a0.py` | 5792 | `OnQueryInventories` | Mismatch | 149 and 99,815 | 8,965, 222,931, 233,564 |
| `asset_count_table_ca4.py` | 3236 | `SyncNotifications` | Mismatch | 98 | 122 and 154 |
| `chunked_stream_08.py` | 8 | `ReplicatedStateBundle` | Mismatch | 78 to 46,423 | 18 to 105,438. Count 4,709,247. |
| `identity_blob_8e6.py` | 2278 | `OnLookupContractsByParticipant` | Mismatch | 38 | 38 (68), 1,233 (25), 4,448 (1) |
| `init_message_18a6.py` | 6310 | `UpdatePhasing` | Mismatch | 36 | 19, 36, 53, 70, 87, 104, 121. Steps of 17 from 19. Server to client only. |
| `level_descriptor_663.py` | 1635 | `LevelInfoChangedMsg` | Mismatch | 106 | 106 (5), 112 (78), 157 (11) |
| `opaque_blob_1033.py` | 4147 | `ReceiveAvailableTitles` | Mismatch | 494 | 655, 707, 911 |
| `receipt_handshake_9fc.py` | 2556 | `LookupContractsByParticipant` | Mismatch | 74 | 72 (94) |
| `self_ident.py` | 1628, and 1489 | self-identification, and `CharacterServiceProxyActor` | Mismatch | minimum 21, and a 4-byte trigger | Index 1628 body is 12,702. Index 1489 is absent. |
| `session_identity_beacon.py` | 7048 | `ReceiveStorageSearchItems` | Mismatch | 38 | 38 (21,248) and 53 (3) |
| `v3_request.py` | 19 | `RegistrationRequestV3Msg` | Mismatch | 832 | 2,660 (22) and 2,708 (21) |

**Inferred.** A 3-byte header makes the 2,708-byte request body a 2,711-byte envelope. The carrier note states that envelope size. 21 requests have it. 22 requests have body 2,660.

`level_info_changed.py` and `level_descriptor_663.py` are the same type index. The minimum of 48 holds. The fixed body of 106 does not.

## 3. Azoth catalog status

**Observed.** 2,163 packet files. Index range 1 through 7,049. Status counts: `solved` 31, `captured` 229, `wanted` 1,903. `packets/574.json` is absent.

**Later finding.** The status counts changed. The same 2,163 files in `local-data/azoth-catalog/packets/` now give `solved` 139, `captured` 454, `wanted` 1,570. The status and purpose texts come from Azoth, another emulator. They are not retail data.

The sequence messages below use our type index from section 1. Purpose text is the first sentence of `what`, except where a later sentence is the whole field in section 1's solved rows.

### Solved

| Type | Class | Purpose |
|---|---|---|
| 3 | `REPClient::RegistrationResponseMsg` | The server's answer to your login: it sends back the game version, the server time and your session token. |
| 349 | `REPClient::PingMsg` | A once-per-second ping. |
| 335 | `REPClient::TimeSynchMsg` | The server's clock, sent to your game about every three seconds so both stay in step. |
| 1617 | `ReceivePlayerSpawnPointMsg` | Part of the login: the server tells your game where your character is placed in the world. |
| 1635 | `LevelInfoChangedMsg` | Tells your game which level to load. |
| 4246 | `SyncTimeChange` | Tells your game how long the day and night cycle is and where it is right now. |
| 4248 | `SyncToClient` | Your game asks the server what time of day it is, which also tells the server you are ready. |

### Captured

| Type | Purpose |
|---|---|
| 8 | The main stream that keeps your game up to date with your character and everything around you. |
| 19 | The first message your game sends when you enter the world: it asks the server to register your session. |
| 164 | Part of the login: the server adds an entry for an entity so your game accepts messages from it, probably your own character. |
| 188 | Your game tells the server whether voice chat is switched on for you. |
| 293 | Delivers several chat messages to you at once, for example when you log in or when a channel catches up. |
| 405 | The server sends your game the nearby voice channels for the area you are in. |
| 446 | Sent right at login with the game's timing settings. |
| 608 | Tells your game to adjust the visual effects of a buff or debuff. |
| 998 | Sent once right when you enter the world, just before your first movement. |
| 1034 | Tells your game that the in-game event calendar is connected. |
| 1094 | Tells the server which network mode your game wants. |
| 1458 | Your game tells the server which quests you have pinned to your tracker. |
| 1589 | Sends the server what you are doing: moving, turning the camera, pressing hotkeys, interacting and attacking. |
| 1628 | Part of the login: the server tells your game who your character is in this session. |
| 1643 | Your game asks the server to set up voice chat for your session. |
| 1679 | Sends your game a summary of your achievements. |
| 1793 | Probably tells your game to stop bumping into other characters or objects for a moment. |
| 2077 | Your game asks the server for a pass to sign in to the voice chat service. |
| 2278 | The server tells your game which trading-post orders you are part of. |
| 2515 | Sends some of your game options to the server. |
| 2556 | Your game asks the server which trading-post orders you are part of. |
| 2709 | Probably your game telling the server whether a quest should be pinned automatically. |
| 2724 | The server tells your game that your gathering was interrupted. |
| 2805 | Your game asks the server for the voice channels of the area around you. |
| 2934 | Your game asks the server for a pass to join a voice channel. |
| 3051 | The server sends your game a voice-chat pass so it can sign in or join a voice channel. |
| 3092 | Asks the server for a summary of your achievements. |
| 3135 | Probably tells your game that a scripted trigger in the world just ran. |
| 3236 | At login, the server sends your game the list of notifications you have waiting. |
| 3610 | Gives your game details about companies. |
| 3639 | Asks the server for details about specific companies. |
| 3916 | Tells your game that a creature is no longer targeting you. |
| 3967 | Tells the server how many other players your game is willing to draw. |
| 4118 | Delivers a chat message from another player to you. |
| 4122 | Tells the server whether other players may watch your game. |
| 4125 | Sets whether the chat profanity filter is on. |
| 4140 | The server sends back details of players your game asked about. |
| 4142 | Your game asks the server for your streamer-mode settings. |
| 4143 | Asks the server for the list of titles your character can use. |
| 4147 | The server sends the list of titles your character can use. |
| 4199 | The server answers that voice chat is set up and sends the details your game needs to connect. |
| 4247 | Tells your game which part of the day it is. |
| 4272 | Tells the server whether the automatic climb and vault option is on. |
| 4854 | Tells the server about your choice on sharing usage information. |
| 4970 | Tells your game that you are not in a game-mode queue. |
| 5181 | Your game tells the server that it received your level information. |
| 5429 | Tells your game that your company's information changed. |
| 5458 | The server answers with how many open Group Finder lobbies are waiting. |
| 5463 | Your game asks how many open Group Finder lobbies are waiting. |
| 5792 | Sends your game the contents of your inventories. |
| 6268 | Your game tells the server that you turned the mount dash on or off. |
| 6271 | Your game tells the server that you turned the mount slow walk on or off. |
| 6310 | Tells your game which parts of the world you may see right now. |
| 6444 | Your game asks the server about your cross-play permissions. |
| 6745 | Your game tells the server it received a phasing update. |
| 7048 | Sends your game the items found in your storage, one batch at a time. |

Type 6484 is `captured`. This note does not copy that purpose text.

### Wanted

| Type | Class | Purpose |
|---|---|---|
| 1234 | `OnQueueGroupRemovedMsg` | Tells your game that the group you queued with was removed from the queue. |
| 1489 | `Javelin::CharacterServiceProxyActor` | Probably the character service that talks for your character at login. |
| 2407 | `RequestPing` | Your game asks the server to place a ping on the map spot you picked. |
| 4200 | `On2dChannelUriReceived` | The server sends your game the address of a voice channel that is not tied to where you stand. |
| 4204 | `RequestGroupChannelUri` | Your game asks the server for the voice channel of your group. |

**Observed.** Flow name `Amazon::Hub::ScaleTestManager::SpawnActorsMsg` is type 574. The packet file is absent. These captures were not searched for a separate top-level count of that type beyond the first-200 type list. It is not in the unmentioned table.

**Corrected.** The flow word StateBundle is class `Amazon::Hub::ReplicatedStateBundle`.

## 4. HTTP JSON shapes

**Observed.** 26 archives were opened. Five archives hold HTTP catalogs (`channel_config`, `gateway_cf`, `content_motd`). The other 21 have no entry name that contains `channel_config`, `getlogininfo`, `worlds`, or `queue`. No `.har` file and no `.mitm` file is in the folder. Every matched response has HTTP status 200. Kinds below are object, array, string, number, boolean, or null.

### channel_config

**Observed.** 6 response files. 1 distinct body.

In the capture. Absent in the login-shape note:

- `platformMetadata` key `ENTITLEMENTS.STEAM_APP_ID.<number>`. Kind object.
- That object's key `app`. Kind string.
- `publicApis[].stage`. Kind string. Present on 3 of 4 items.

In the note. Absent in this capture:

- No `platformMetadata` key contains `PERSONA.`
- `personaApiKey`, `personaHostV2`, `personaRegion`, and `personaAccessRoles` are absent.

No type mismatch on the shared channel keys.

**Observed.** Root keys: `channelName` (string), five `*-prod` cluster objects, and `platformMetadata` (object). Each cluster has `displayName`, `localizedName`, `poolId` (strings) and `publicApis` (array, count 4). Each API item has `apiEndpoint`, `awsRegion`, `clientTag`, and `version` (strings). The OMNI metadata object has `nwTokenUrl`, `omniGameAlias`, `omniStage`, and `omniTokenUrl` (strings).

### getlogininfo

**Observed.** 47 response files. Path `/prod/game/getlogininfo/jwt/omni`. Query key names: `channelId`, `includeNames`, `isTrialOwner`.

In the capture. Absent in note section 5.1:

- `LoginInfoList.RecommendedWorlds` (array, count 1 on each response).
- `RecommendedWorlds[].RecommendationType` (string) and `RecommendedWorlds[].World` (object). The note puts that pair on `ListWorldsResponseBody`.
- Character keys `Name` (string, 75 of 75), `PermanentAppOwner` (boolean), `PermanentAppOwnerCreated` (boolean), `Status` (string), `SrcWorldId` (string, 2 of 75).
- World key `IsSelectable` (boolean) on all 329 `Worlds` items and all 47 recommended worlds.
- Name-record keys `Channel`, `Region`, `State`, and `WorldId` (strings, all 115).

In the note. Absent in this capture:

- Character key `RegionTransferDate`.
- Name-record key `Namespace`.
- Pending-merge keys `ScheduledDate`, `SourceWorld`, and `TargetWorld`. `PendingWorldMerges` count is 0, so no element was present.

Type mismatch:

- `Characters[].TransferData` is null on all 75. The note says string.
- `NameReservations[].DeletedDate` is null on all 115. The note says string.

### worlds

**Not found.** No response is `/game/worlds`, `/game/worlds/list`, or `ListWorlds`. The note keys `ListWorldsResponseBody`, `ListWorldsResponseBody.Worlds`, and `ListWorldsResponseBody.RecommendedWorlds` are absent because that call is absent.

**Observed.** `motd_worlds` is a different document. Top-level keys: `globalAnnouncement`, `worldSets` (count 104), `worlds` (count 76). Those keys are absent in the login-shape note.

### queue

**Observed.** 94 files. Two shapes. They do not overlap.

Shape A. Path `/prod/game/login/queue/v2/jwt/omni`. 47 files. Keys: `AllowQueueTransfer`, `EstimatedTime`, `Position`, `QueueName`, `RefreshInterval`, `TicketId`.

Shape B. Path `/prod/game/login/queue/v2/<one segment>/jwt/omni`. 47 files. Keys: `AllowQueueTransfer`, `TicketId`, `Token`.

In the note. Absent in this capture:

- `RecommendedTransferWorldId` on both shapes.
- `Token` on shape A.
- `EstimatedTime`, `Position`, `QueueName`, and `RefreshInterval` on shape B.
- `Token.AccountIsLocked` and `Token.ClientCapabilities` on every `Token` object.

No type mismatch on the shared queue keys.

## Unresolved

| Item | Why it stays open |
|---|---|
| Phasing counter byte, and 16 shared contract bytes | The check reads body bytes. Body bytes are not reported. |
| Two `0xae` trail values | The split byte is a body byte. It was not read. No 22-byte server body uses that prefix. |
| Envelope flag `0x04` layout | The flag did not occur. The carrier note already marks the layout Unknown. |
| Empty name-map string for type 3051 | The packet file names the class. `NewWorld.exe` has no text form of the type-table UUID. A name-map fill needs a source the string search did not find. |
| Login `/game/worlds` | The 26 archives have no entry and no URL for that call. `motd_worlds` is a different document. |
