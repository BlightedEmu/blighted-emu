# Client protocol: login, GridMate, and world entry

Date: 2026-08-29
Build: Steam build `22469132`, `Bin64/NewWorld.exe`, image base `0x140000000`, x86-64, Windows compiler spec. Ghidra project `<re-scratch>/ghidra` (program `NewWorld.exe`, 355,815 functions).
Method: static analysis of the already-analyzed project, opened read-only with analysis left off (`analyzeHeadless -process NewWorld.exe -noanalysis -readOnly`). The game was not run. No game file was changed. Easy Anti-Cheat was not analyzed. Static analysis did not observe execution.

Labels: **Observed** = an address, a string, or decompiler output in this build. **Inferred** = a conclusion from that evidence. **Unknown** = not established.

Related notes: [2026-08-29 client install findings](2026-08-29-client-install-findings.md), [2026-08-31 endpoint configuration](2026-08-31-client-endpoint-configuration.md) (RE task 1). This note is RE tasks 2–5 in the architecture spec, section 11.3.

Where this note and `docs/research/02-network-protocol-and-login.md` disagree, the binary wins. Those disagreements are marked below.

REA `open_binary` was not used. That Ghidra provider always imports the executable into a new project and runs auto-analysis. This pass reused the existing project and did not re-analyze it.

## 1. Login

### 1.1 After the channel config

**Observed.** `FUN_146450250` (`0x146450250`) reads the channel JSON. It logs `Channel name: %s` from `channelName`, walks `platformMetadata`, and for each cluster reads `displayName`. Public API objects are selected by `clientTag`:

| `clientTag` | Fields read | URL the client builds |
|---|---|---|
| `authStack` | `apiEndpoint`, `awsRegion` | `https://%s/Prod` |
| `loginGateway` | `apiEndpoint`, `awsRegion` | the endpoint string as stored |
| `JavelinGatewayServiceV2` | `apiEndpoint` | `https://%s/prod` |
| `JavelinGatewayService-CF` | endpoint host | logged as `Gateway Stack` / `Http Gateway Stack` |

`FUN_1464211f0` (`0x1464211f0`), called from `0x146450250`, reads `omniTokenUrl`. The same parser also accepts `nwTokenUrl` and `personaHostV2` (see the endpoint note).

**Observed.** These path strings sit in one block at file region `0x84ff388` (VA `0x148500b88` and the following strings). Operation names in the same region have a pointer table at `0x1484fa070`:

| Operation name | Path string in the same block |
|---|---|
| `GetLoginInfo` | `/game/getlogininfo`, `/game/getlogininfo/jwt`, `/game/getlogininfo/jwt/omni` |
| `ListWorlds` | `/game/worlds/list`, `/game/worlds/list/jwt`, `/game/worlds/list/jwt/omni` |
| `ValidateCharacter` | `/game/worlds/%s/characters/validator`, `.../jwt`, `.../jwt/omni` |
| `PostGameWorldsWorldIdCharacters` (string only) | `/game/worlds/%s/characters`, `.../jwt`, `.../jwt/omni` |
| `QueueLogin` / `PostGameLoginQueueV2` | `/game/login/queue/v2`, `.../jwt`, `.../jwt/omni` |
| `RefreshQueuedLogin` / `PostGameLoginQueueV2TicketId` | `/game/login/queue/v2/%s`, `.../%s/jwt/omni` |
| `LinkIdentity` / `UnlinkIdentity` | `/game/linkIdentity`, `/game/unlinkIdentity` |

A header name `x-nw-auth` is in the same block. Absolute pointers to the path strings were not found. **Inferred:** each `.../jwt/omni` path is the Omni form of the named operation. The call that picks `jwt/omni` versus `jwt` versus the bare path was not found.

**Observed.** Connection-state names, pointer table `0x1484f9ff0`, used by `FUN_14644a070` (`0x14644a070`):

`Disconnected`, `QueryGameUpdateCheck`, `WaitingForGameUpdateCheck`, `QueueGameLogin`, `WaitingForQueuedLogin`, `QueryForRemoteConfigClass`, `WaitingForRemoteConfigClass`, `ObtainREPRequirements`, `WaitingForREPRequirements`, `StartREPConnection`, `WaitingForREPConnection`, `WaitingForActorGameConnection`, `WaitingForSpawnPoint`, `WaitingForPlayerSpawn`, `InGame`.

`StartREPConnection` logs `GameConnectionWrapper: start REP connection RepAddress = %s, worldId = %s`.

### 1.2 Steam ticket and token

**Observed.** `FUN_145a9fbe0` (`0x145a9fbe0`) logs `Getting steam auth session ticket.`, calls `SteamInternal_ContextInit`, then a virtual method at offset `0x70`. It stores the returned 32-bit handle at object `+0x2f0` and clears `0xa00` bytes at `+0x38c`. If a previous handle is non-zero it calls virtual offset `0x88` first. **Inferred:** these are the Steam user-interface ticket calls. The interface slot names are not in the binary.

**Observed.** `FUN_145a936f0` (`0x145a936f0`) is the `OnAuthSessionTicket` callback. It logs the handle and `ticket: %s, ticketNoPrefix: 0x%s`. It reads `client-connection.client-gateway.signing-host`, `.endpoint`, `.http-endpoint`, and `.region`. It appends the 6-byte string `/steam` (`0x14844bbb8`) and issues an HTTP call with arguments `10000`, `3000`, and `5` (`FUN_141722280`). Failure strings include `@mm_http_NoResponse`, `No auth stack endpoint!`, and `Persona failed to initialize!` (`@mm_authresult_Error_Persona`).

**Observed.** Omni token request builder `FUN_1479c9a90` (`0x1479c9a90`), source path `OmniSDK\sdk\source\gateway\ags_fed_acc_token_serv_gateway.cpp`. The JSON object is:

- `platformIdentityType` (string from the platform enum)
- `platformAuth` (object). Enum value `1` writes `ticket`. Values `3`, `5`, and `10` write `authCode`. Values `6`, `7`, `8`, `11`, and `12` write `token`, or `nsaToken` plus `naToken`. Value `100` writes `platformAccountId` and then `token`. Values `2`, `4`, `9`, and `13` write `token`.
- `fallbackToken`
- `agsAccountAuthToken` when a fourth argument is non-null
- `attributionData`

**Inferred:** Steam uses enum value `1` and therefore `platformAuth.ticket`. The enum-to-platform table was not in this function.

**Observed.** Token response parser `FUN_1479b6d00` (`0x1479b6d00`) reads `limitedUseToken`, `fallbackToken`, `platformAccount`, `account`, `isNewAccount`, `conflictingAccount`, `accessToken`, `expiresIn`, and `region`. Error strings it handles: `INVALID_FALLBACK_TOKEN`, `INVALID_AGS_AUTH_TOKEN`. `FUN_14028ed40` (`0x14028ed40`) references `PLAYER_IS_BANNED` and `INVALID_PLATFORM_AUTH`.

The strings `jwks.json` and `openid-configuration` are absent. **Unknown:** whether this build verifies the Omni JWT through JWKS.

### 1.3 Login-queue reply

**Observed.** Three functions, all optional-field readers (a key is stored only when `FUN_147465730` says it is present):

1. `FUN_1474e89d0` (`0x1474e89d0`) looks up the object `LoginQueueResponse` and passes it to (2).
2. `FUN_1474e4f20` (`0x1474e4f20`) reads the queue object and, if `Token` is present, passes that object to (3).
3. `FUN_1474e5990` (`0x1474e5990`) reads the token object.

JSON shape:

```json
{
  "LoginQueueResponse": {
    "AllowQueueTransfer": true,
    "EstimatedTime": 0,
    "Position": 0,
    "QueueName": "",
    "RecommendedTransferWorldId": "",
    "RefreshInterval": 0,
    "Token": {
      "AccountAge": 0,
      "AccountIsLocked": false,
      "ChannelId": "",
      "CharacterId": "",
      "ClientCapabilities": "",
      "GenerateTime": "",
      "IsPermanentAppOwner": false,
      "IsTrialOwner": false,
      "IssueTime": "",
      "LocationGroupId": "",
      "LocationId": "",
      "PersonaId": "",
      "RepAddress": "",
      "Signature": "",
      "SteamAppId": 0,
      "SteamUserId": "",
      "TicketId": "",
      "TokenVersion": 0,
      "WorldId": ""
    }
  }
}
```

Getters: `FUN_147465410` is the boolean reader (`AllowQueueTransfer`, `AccountIsLocked`, `IsPermanentAppOwner`, `IsTrialOwner`). `FUN_147465490` is the number reader (`EstimatedTime`, `Position`, `RefreshInterval`, `AccountAge`, `SteamAppId`, `TokenVersion`). `FUN_1474654e0` is the string reader (every other field above, including `RepAddress`, `Signature`, and `TicketId`).

`RepAddress` and `Signature` are strings inside `Token`. They are not fields of the outer `LoginQueueResponse` object. Research file 02 places them on the outer object. The binary disagrees.

**Observed.** The request writer `FUN_1474ecc80` (`0x1474ecc80`) emits an object `LoginQueueRequest` when a presence flag at `+0x250` is set. `FUN_1474ec7b0` (`0x1474ec7b0`) writes, each behind its own presence flag:

| Field | JSON kind |
|---|---|
| `CharacterId` | string |
| `ClientCapabilities` | array of strings |
| `IsTrialOwner` | boolean |
| `SteamAppId` | number |
| `SteamAuthTicket` | string |
| `WorldId` | string |

**Observed.** Ticket log strings, not the community text `No login ticket received` (that text is absent):

- `Received a login ticket: %s, location: %s, %s` in `FUN_14643d110` (`0x14643d110`)
- `RefreshLoginQueueTicket failed: no login ticket.`
- `LoginQueueTicket failed: no login ticket.` (no code reference recorded)
- `Waiting in login queue... Ticket (%s), Position (%d)`
- `Queuing login for world (%s) and character (%s)`

**Unknown:** which of the optional reply fields the client requires before it logs "received a login ticket", and whether an empty `Signature` is rejected. The host/port split of `RepAddress` was not recovered. The log prints it as one `%s`.

### 1.4 Certificate trust

**Observed.** A PEM block in the executable (file `0x858ec60`) decodes as one self-signed X.509 certificate. Subject and issuer are `C=US, ST=California, L=SNA11, O=Amazon, OU=Amazon Game Studios, CN=New World`. `notBefore` is 2025-08-04 23:15:36 GMT. `notAfter` is 2027-01-06 23:15:36 GMT. No `PRIVATE KEY` block is next to it. The PEM body is not copied here. The endpoint note records the same certificate and that no launch argument or config key replaces it.

**Observed.** `FUN_146b6a270` (`0x146b6a270`) reads `client-connection.transport-options.transport-type`. For the exact strings `gridmate-udp` and `gridmate-udp-bsd` it constructs the transport with `FUN_146b34780`, and it passes the embedded PEM (`0x149f80d90`). Any other transport string uses `FUN_146b3de50` and does not receive that PEM.

**Observed.** `FUN_145dce750` (`0x145dce750`) builds the secure context:

- It sets the cipher list to the literal `ECDHE-RSA-AES256-GCM-SHA384` (`FUN_1478efe40`).
- If a PEM text pointer on the driver is non-null (`+0x288`), `FUN_145dc8de0` (`0x145dc8de0`) finds each `-----BEGIN CERTIFICATE-----` … `-----END CERTIFICATE-----` span, parses it (`FUN_145dc91f0` / `FUN_14777f1e0`), and adds each parsed certificate (`FUN_14777d770`). Verify is then set with a null callback (`FUN_1478eff50`).
- The mode argument is `1` or `2`, from a flag byte at driver `+0x4e` (`0` yields `1`, non-zero yields `2`).
- If that PEM pointer is null, the same call uses `FUN_1402a1a70` (`0x1402a1a70`), which returns `1`.

**Inferred:** `FUN_1478eff50` is `SSL_CTX_set_verify`, and modes `1` and `2` are the OpenSSL peer-verify modes. The cipher string and the PEM parse are the evidence. No hostname check appears in `FUN_145dce750` or `FUN_145dc8de0`. **Inferred:** the PEM passed into `FUN_146b34780` is the text later parsed at driver `+0x288`. The store inside `FUN_146b34780` was not decompiled.

A separate string mentions `caPath` and a RETAIL / sandbox certificate choice. It has no code reference. It is not the REP trust path above.

## 2. GridMate Carrier

**Observed.**

- Log and thread names: `GridMate-Carrier` (`FUN_140f4c5a0`, `0x140f4c5a0`), `GridMate-Carrier Packet Send Thread` (`FUN_140f69c00`, `0x140f69c00`).
- Config keys in one string block: `javelin.enable-lz4-compression`, `javelin.gridmate.bind-to-specific-interface`, `javelin.gridmate.shared-recv-buffers`, `javelin.gridmate.send-warning-msgs`, `javelin.gridmate.process-resends-last`, `javelin.gridmate.warning-conn-threshold-divisor`, `javelin.gridmate.warning-no-sends-threshold-ms`, `javelin.gridmate.warning-check-period-ms`, `javelin.gridmate.client.initial-send-pool-size`, `javelin.gridmate.client.max-send-pool-size`.
- `javelin.enable-lz4-compression` is referenced from `FUN_140279760` (`0x140279760`) and from `FUN_14616c670` (`0x14616c670`, 111,309 bytes, not decompiled).
- `LZ4_compressBound` is present at `0x1479709c0` (library signature). For an input below `0x7e000000` it returns `n + (n / 0xff) + 0x10`. `FUN_146abec70` (`0x146abec70`) only calls it. Callers are data pointers (`0x148588dd8`, `0x14ac113a8`), not a decompiled send function.
- The ASCII string `LZ4` is absent. The config key uses lowercase `lz4`.
- Transport error strings: `Carrier driver error ConnectionID: %p ErrorCode: 0x%08x` (`FUN_140f6f7a0`), `Carrier security error ConnectionID: %p ErrorCode: 0x%08x` (`FUN_140f724d0`).

**Observed.** The connection object keeps four sequence groups. `FUN_140f76d60` (`0x140f76d60`) prints them:

| Log label | Connection offsets | Width |
|---|---|---|
| `conn.sendSeqNum` | `+0x6c`, `+0x6e`, `+0x70`, `+0x72` | four `uint16` |
| `conn.sendReliableSeqNum` | `+0x74`, `+0x76`, `+0x78`, `+0x7a` | four `uint16` |
| `conn.toSend` | `+0x90`, `+0xd0`, `+0x110`, `+0x150` | four pointers, stride `0x40` |
| `conn.toReceive` | `+0x1a0`, `+0x1e0`, `+0x220`, `+0x260` | four pointers, stride `0x40` |

**Inferred:** the carrier has four channels, indexes 0 through 3, and a separate reliable sequence per channel. Research file 02 names channels 0, 1, and 3. Slot 2 is present in this object. Which slots are used on the wire was not shown.

**Observed.** The send thread body is `FUN_140f8b810` (`0x140f8b810`), started by `FUN_140f69c00`. It sends buffers that `FUN_140f69c00` sizes at `0x600` bytes, with a side record of `0x1c` bytes per slot. On a return code it calls `WS2_32` ordinal 111 and retries while the code is `0x2747` (10055). It does not write a mode byte, a format byte, or a sequence into the buffer.

**Observed.** LZ4 sits in one cluster. `LZ4_compressBound` is `0x1479709c0`. The pointer table at `0x148588dc0` holds:

| Slot | Function | What it calls |
|---|---|---|
| `0x148588dd8` | `0x146abec70` | `LZ4_compressBound` |
| `0x148588de0` | `0x146ab3bd0` | `0x147970930` (0x90 bytes before `LZ4_compressBound`). Returns 2 when the callee result is below 1. |
| `0x148588de8` | `0x146ab6540` | `0x1479714e0`. On success it stores the output pointer and the byte count. |

**Observed.** The three wrappers are slots in a vtable. The code takes the base `0x148588db8`. `FUN_146ab5400` (`0x146ab5400`) allocates 8 bytes and stores that base as the object's only pointer. Slot order from that base:

| Offset | Function |
|---|---|
| `+0x00` | `0x146aad330` |
| `+0x08` | `0x14029f880` |
| `+0x10` | `0x146ae0240` |
| `+0x18` | `0x140c9f3f0` |
| `+0x20` | `0x146abec70` (`LZ4_compressBound` wrapper) |
| `+0x28` | `0x146ab3bd0` |
| `+0x30` | `0x146ab6540` |
| `+0x38` | `0x141752700` |
| `+0x40` | `0x141754050` |

No call instruction targets `0x146abec70`, `0x146ab3bd0`, or `0x146ab6540`. The other references to those three addresses are in `.pdata` (`0x14ac113a8`, `0x14ac10a90`, `0x14ac10bb0`). `FUN_146ab5400` has no code caller. Its address is stored at `0x14858d228`. `FUN_146b34780` (`0x146b34780`) and `FUN_146b35740` (`0x146b35740`) store the table base `0x14858d200` at object `+0x30`. Neither function calls the slot at `+0x28`. **Inferred:** a later virtual call through object `+0x30` could reach `FUN_146ab5400`, and a later virtual call through the object it builds could reach the three wrappers. That call instruction is not in a decompiled function.

**Observed.** Inside `FUN_14616c670`, the only use of `javelin.enable-lz4-compression` is at `0x14618048a`. The instruction loads the string, calls `FUN_1412ef6d0` (`0x1412ef6d0`), then calls the virtual method at offset `0x60` with the byte `1`. The same instruction sequence is used for the strings at `0x14814e298`, `0x1484d33a8`, `0x1484d33d8`, and `0x1484d3400`. The function was not decompiled. This site does not call the LZ4 wrappers.

**Observed.** The secure-socket state machine `FUN_145dce4c0` (`0x145dce4c0`) names `CS_COOKIE_EXCHANGE` and `CS_ESTABLISHED`. Disconnect text in `FUN_145dd83f0` (`0x145dd83f0`) includes `Version mismatch when establishing a connection.` and `Handshake rejected`.

**Observed.** `FUN_140f7fc60` (`0x140f7fc60`) is the function that fills a send slot. If the length argument is above `0x600` it returns `0x3e9` and copies nothing. Otherwise it copies `0x1c` bytes of side data to `object + 0x1a8 + slot * 0x1c`, then `memcpy`s the caller buffer to `object + 0x1a0 + slot * 0x600`. The slot index is the `int` at object `+0xe8`. It stores that length in the descriptor at `object + 0x150`, adds 1 to the counter at `object + 0x48`, and wakes the condition variable at `object + 0x40`. It does not write a mode byte, a format byte, or a sequence. The send thread `FUN_140f8b810` then calls the function pointer at `object + 0x70` with the descriptor. That call does not write the buffer.

**Observed.** `FUN_140f7fc60` has no call instruction. Its address is the vtable slot at `0x147fbe548`, which is offset `0x18` from the vtable `0x147fbe530`. `FUN_140f47000` (`0x140f47000`) stores that vtable on a new carrier object. The same fill address is also the slot at `0x1484807a8` (offset `0x18` from `0x148480790`). The bytes at `0x14a8c2538` are not a pointer to this function. Functions that merely use the displacement `0x600` (`FUN_140f6e5c0`, `FUN_140f7f980`, `FUN_140f817a0`, `FUN_140f8a820`, `FUN_141049c20`, `FUN_14104a790`, `FUN_145d8c7c0`, `FUN_145dd9b30`) do not copy into the send slots.

**Not found.** A function that writes the research-file-02 header (`u8` mode `0x80` or `0x81`, `u8` format `0x01`, `u16` sequence). The `0x600`-byte slot is born in `FUN_140f7fc60` as a copy of a buffer that already exists. The search stopped at that function. The 4-byte header is not confirmed.

Negative searches, still empty: `SM_CONNECT`, `CONNECT_REQUEST`, `DATA_CHANNEL`, `kCarrierEndian`, `k_systemChannel`, `NewProxy`, `LZ4_decompress_safe`. Code references to `0x146abec70`, `0x146ab3bd0`, and `0x146ab6540`: none. Code references to the vtable base `0x148588db8`: only `FUN_146ab5400`.

## 3. V3 registration

**Observed.** Three request types and one response type are registered by the same UUID parser (16 bytes, dashes and braces skipped). Each name string is followed by its UUID string:

| Name | UUID string | Registrar | File index (section 4) |
|---|---|---|---|
| `RegistrationRequestMsg` | `8673a3cc-2848-4c87-aa72-cc860589d1b5` | `0x1407f27f0` | 1 |
| `RegistrationRequestV2Msg` | `DA4E5889-A65C-4480-8642-0278160125A7` | `0x1407f2a20` | 2 |
| `RegistrationRequestV3Msg` | `0B826B33-89F5-49E0-B8CB-FE4433427778` | `0x1407f2c50` | 19 |
| `RegistrationResponseMsg` | `104145a7-ff95-44f1-9468-21fb41c8ac2b` | `0x1407f2e80` | 3 |

`FUN_1407f2c50` stores the parsed UUID and installs vtable `0x147f47278`. `FUN_1407f2e80` installs vtable `0x147f46910` for the response. Neither serializer references field-name strings. The order below is the order of writes and reads. Names are not in the binary.

**Observed.** Integer endian. `FUN_140876cd0` (`0x140876cd0`) loads a `uint32` and calls `0x1461656f0`, which jumps to the `WS2_32.dll` import at ordinal 8, then writes 4 bytes. `FUN_14087a220` (`0x14087a220`) reads 4 bytes and calls `0x146167950`, which jumps to `WS2_32.dll` ordinal 14. The same import table's other ordinals are 4, 9, 13, 15, 16, and 19. **Inferred:** ordinal 8 is `htonl` and ordinal 14 is `ntohl`, so these `uint32` fields are big-endian. The type-index prefix code in section 4 is a different encoding.

**Observed.** `RegistrationRequestV3Msg` write is vtable slot 4, `FUN_1407ce860` (`0x1407ce860`). The matching read is slot 5, `FUN_1407ce8e0` (`0x1407ce8e0`), which calls `FUN_1407d9620` (`0x1407d9620`). Write order:

1. Big-endian `uint32` from object `+0x8`.
2. `FUN_1407d62b0` (`0x1407d62b0`) on object `+0x10`. It writes a prefix-coded count (section 4). If the container length is above `0x2000000` it writes 0. Otherwise, for each list node, a big-endian `uint32` and then a length-prefixed byte string (`FUN_140876d80`, `0x140876d80`). A string longer than `0x2ffff` is replaced by a single 0 byte.
3. Length-prefixed string at object `+0xa0`. The length uses the same prefix code. A length of `0x30000` or more is written as 0 and the bytes are omitted (`FUN_1407d54a0`, `0x1407d54a0`).
4. `FUN_1407d5f30` (`0x1407d5f30`) on object `+0xc0`: one length-prefixed string, then `FUN_1407d4920` (`0x1407d4920`). That helper writes four length-prefixed strings and one big-endian `uint32`, then `FUN_1407d4b50` (`0x1407d4b50`). `FUN_1407d4b50` writes four more length-prefixed strings, then calls `FUN_1407d42c0` (`0x1407d42c0`). `FUN_1407d42c0` writes, in order: one length-prefixed string (length 0 when the string length is `0x30000` or more), three big-endian `uint32` values (`FUN_140876cd0`), one length-prefixed string, one big-endian `uint32`, one raw byte, one length-prefixed string, one byte that is 0 or 1, and, when that byte is 1, one more byte that is 0 or 1. It then calls `FUN_1407d46a0`, which is already recorded above.
5. On object `+0x2c8`: a big-endian `uint32`, one byte that is 0 or 1, another big-endian `uint32`, a length-prefixed string, then `FUN_1407d4540` (`0x1407d4540`): two length-prefixed strings, one big-endian `uint32`, two length-prefixed strings, then `FUN_1407d46a0` (`0x1407d46a0`). That helper writes one length-prefixed string, then `FUN_1407d4490` (`0x1407d4490`), which writes two length-prefixed strings and returns. That chain ends there.
6. `FUN_1407d3f70` (`0x1407d3f70`) on object `+0x3e0` and the byte at `+0x460`. Each of the two `FUN_140876fa0` (`0x140876fa0`) calls writes a flag byte. If bit 0 of that flag is set, the next field is 16 raw bytes. Otherwise it calls `FUN_1407d60d0` (`0x1407d60d0`), which writes one length-prefixed string (length 0 when the string length is `0x30000` or more). Then one byte, 0 or 1.

The read path checks the 0/1 byte and rejects a value of 2 or more. Field names are absent.

**Observed.** `RegistrationResponseMsg` write is `FUN_1407cceb0` (`0x1407cceb0`). The read is `FUN_1407cd040` (`0x1407cd040`). Order:

1. Big-endian `uint32` at object `+0x8`.
2. An 8-byte integer. `FUN_1408770e0` (`0x1408770e0`) writes it after `FUN_146165760` (`0x146165760`), which calls `WS2_32` ordinal 8 (`htonl`) on each 32-bit half. `FUN_14087ad90` (`0x14087ad90`) reads 8 bytes and calls `FUN_1461679c0` (`0x1461679c0`), which calls ordinal 14 (`ntohl`) on each half. **Inferred:** the field is a big-endian `uint64`. The name is absent.
3. Length-prefixed string at `+0x18` (`FUN_1407fc670`, `0x1407fc670`).
4. Length-prefixed string at `+0x38`.
5. Four bytes at `+0x58` through `+0x5b`. Each is written as 0 or 1. The reader (`FUN_1407da5e0`, `0x1407da5e0`) rejects a byte of 2 or more.

**Observed.** After the socket is up, the state names and logs are:

1. `StartREPConnection` logs `RepAddress` and `worldId` (`0x14644a070`).
2. String `GameConnectionWrapper: REP socket connection established, now waiting to register` (file `0x84fd140`).
3. `FUN_1464755e0` (`0x1464755e0`) logs `received registration response from REP`, then sets a value named `Server version` from the response argument.
4. `WaitingForActorGameConnection`, then `WaitingForSpawnPoint` (`spawn point found`), then `WaitingForPlayerSpawn` (`player spawn succeeds, in game`), then `InGame`.

**Observed.** The direct callers of the three registrars do not send a request. `FUN_1407c3120` (`0x1407c3120`) calls the V1 registrar and then `FUN_1461a9740` (the `Registered type` log). `FUN_1407c39e0` (`0x1407c39e0`) does the same for V2. `FUN_1407c3e20` (`0x1407c3e20`) does the same for V3. `FUN_14007a280` (`0x14007a280`) calls the V1 registrar and then `FUN_1461a9640` (`0x1461a9640`). `FUN_14007a450` (`0x14007a450`) calls the V2 registrar. `FUN_14007a620` (`0x14007a620`) calls the V3 registrar. Those last two bodies were not decompiled. The name strings have no other code reference.

**Observed.** Three jump thunks forward to the registrars: `0x1407ce3a0` jumps to V1, `0x1407cf9b0` jumps to V2, and `0x1407cfca0` jumps to V3. Each jump address is stored once, at `0x147f47860`, `0x147f47920`, and `0x147f479d8`. Those three slots have no code reference.

**Not found.** A send-side call that picks V1, V2, or V3. All three types are registered. Which request is written to the socket was not shown. The order of datagrams on the new connection is still unknown. The about-832-byte and 88-byte sizes in research file 02 were not measured on the wire.

## 4. Message type table

**Observed.** `typeindex.json` in the install has keys `generationTime` (`2025-10-22T03:13:16Z`) and `typeIndex` (7,052 strings of 32 hex digits, no dashes). Index 0 is 32 zero digits.

**Observed.** Loader `FUN_1461661c0` (`0x1461661c0`):

- Opens `typeindex.json`. On failure it logs `No type index file could be opened. Proceeding without index.` and returns 0.
- Requires a `generationTime` field and a non-empty `typeIndex` array (JSON type code 4). Otherwise it logs `typeindex failed validation`.
- Builds a vector of 16-byte values at object `+0x40`.
- The first slot is always the 16 zero bytes from `FUN_1407f7d80` (`0x1407f7d80`).
- For file index 0, the parsed UUID is appended only when it is not those 16 zero bytes. Later file entries are appended in order.
- So when file index 0 is the zero UUID, vector slot `i` is file entry `i`.

**Observed.** `FUN_14615f720` (`0x14615f720`) runs after a successful load. For each vector slot `i` with `i >= 1`, it looks up that UUID and writes `i` into the type object at `+0x54`. The registration log `Registered type: {uuid= … localIndex= …}` is `FUN_1461a9740` (`0x1461a9740`).

**Observed.** `GetTypeIdForIndex` is `FUN_1461acfe0` (`0x1461acfe0`). It reads the index with `FUN_14087b5c0` (`0x14087b5c0`):

- Index `0`: the next 16 bytes in the stream are the UUID (`FUN_140878610` with length `0x10`). The vector is not used.
- Index `> 0` and `<` vector length: the UUID is the 16 bytes at that slot.
- Otherwise it logs `Invalid index requested in GetTypeIdForIndex`.

The index integer is a prefix code, not standard LEB128 (research file 02 says LEB128; this function does not):

| First byte | Extra bytes | Value |
|---|---|---|
| `0x00`–`0x7F` | 0 | the byte |
| `0x80`–`0xBF` | 1 | low 6 bits of the first byte, extra byte shifted left 6 |
| `0xC0`–`0xDF` | 2 | low 5 bits, extra bytes shifted left 5 |
| `0xE0`–`0xEF` | 3 | low 4 bits, extra bytes shifted left 4 |
| `0xF0`–`0xFF` | 4 | low 3 bits, extra bytes shifted left 3 |

**Observed.** These file indexes match the UUID strings in the binary (hex digits only, case ignored):

| Index | Name |
|---|---|
| 0 | zero UUID (inline-UUID slot on the wire) |
| 1 | `RegistrationRequestMsg` |
| 2 | `RegistrationRequestV2Msg` |
| 3 | `RegistrationResponseMsg` |
| 19 | `RegistrationRequestV3Msg` |
| 723 | `ProcessDeferredMovementRequestsMsg` |
| 5867 | `PingMsg` |

The full UUID list is not in this repository.

**Conclusion.** For this install, `typeindex.json` is the `type_idx` → type UUID table used by `GetTypeIdForIndex`. Index 0 on the wire means "inline 16-byte UUID", and it is also the zero UUID in the file. Indexes `1` and above are file slots. This disproves the idea that the table is only a content-pipeline artifact. It does not by itself prove every gameplay message's field layout.

One envelope reader that calls `GetTypeIdForIndex` is `FUN_1461ace70` (`0x1461ace70`). It reads a flags byte, then 8 bytes when bit 0 is set, 8 bytes when bit 1 is set, a further typed value when bit 2 is set, then a kind byte. The kind byte must be `1` before the type id is accepted. **Unknown:** the names of those flag bits, and the writer that emits `localIndex`.

## 5. First world messages

**Observed.** These names sit in the message catalog with a UUID string immediately after the name. The file index is the `typeindex.json` slot from section 4.

| Name | UUID | File index | Reader |
|---|---|---|---|
| `MoveActorMsg` | `5570cfb7-0c5c-475e-bb19-d8fbc3fd8d32` | 710 | `FUN_146a54050` (`0x146a54050`) |
| `MoveActorToHubMsg` | `0a8f6818-915c-4700-9c33-463862bbe5f0` | 718 | registrar `0x146ac9d70` only |
| `StartMovementCommunicationMsg` | `767fc9bf-1c8d-417f-ad19-c51e363d4df0` | 726 | `FUN_146a55240` (`0x146a55240`) |
| `AckMovementMsg` | `5ad005ae-375c-4f2d-a00e-3ff9f6f01069` | 739 | `FUN_146a55ce0` (`0x146a55ce0`) |
| `AddActorMessage` | `96C829CE-DFBC-4763-959C-32A6B22D81EF` | 121 | `FUN_146a31f80` (`0x146a31f80`) |
| `ReceiveActorMsg` | `764599E8-3D44-4EB3-BF44-77097CB414E3` | 921 | `FUN_146a56950` (`0x146a56950`) |
| `DeleteActorMessage` | `77CF6891-CE44-4F37-9E97-B8254E23D547` | 919 | not opened |
| `TimeSynchMsg` | `038cd847-0653-4243-9a26-936e3bd7f312` | 335 | not opened |
| `PingRequestMsg` | `ad4c28bd-4208-4b47-9679-9a46dd9e5287` | 209 | not opened |
| `PingMsg` | `{85C7E01E-55DE-4A6E-AFC5-80FA9A8AD260}` | 5867 | not opened |
| `ProcessDeferredMovementRequestsMsg` | `4c220b64-c728-4d31-9c1e-6244e8089215` | 723 | registrar `0x146acf530` only |

**Observed.** Bodies that the decompiler shows:

- `MoveActorMsg` read copies 16 raw bytes and then stops (`FUN_140878610` with length `0x10`).
- `StartMovementCommunicationMsg` write `FUN_146a546d0` (`0x146a546d0`) writes 16 raw bytes from object `+0x8`, then a big-endian `uint32` from `+0x18`. The read `FUN_146a6b1b0` (`0x146a6b1b0`) reads those same two fields.
- `ReceiveActorMsg` read `FUN_146a6b3f0` (`0x146a6b3f0`) starts with one byte. A value of 2 or more fails. A value of 0 or 1 continues into `FUN_146a6a8a0` (`0x146a6a8a0`). A value of 1 first calls `FUN_146a86df0` (`0x146a86df0`), which allocates a `0xac0`-byte object and reads no wire bytes. It calls `FUN_146a923f0` (`0x146a923f0`), which was not opened. `FUN_146b074a0` (`0x146b074a0`) then reads:
  1. `FUN_146a6c940` (`0x146a6c940`) reads one byte that must be 0 or 1. A value of 2 or more fails. It then calls `FUN_146a6d390` (`0x146a6d390`). That helper reads two bytes that must each be 0 or 1, calls `FUN_1415bc630` (`0x1415bc630`, not opened), reads one more byte that must be 0 or 1, and, when that byte is 1, reads 16 raw bytes. It then reads two length-prefixed strings (`FUN_1407fc670`) and calls `FUN_146a6b260` (`0x146a6b260`, not opened). After that helper returns, `FUN_146a6c940` calls `FUN_1461ac530` (`0x1461ac530`, not opened), `FUN_14087a820` (`0x14087a820`, not opened), reads one byte that must be 0 or 1, and calls `FUN_146a6b5b0` (`0x146a6b5b0`, not opened). The bytes this pair reads directly are raw bytes, 16 raw bytes, and length-prefixed strings. No new multi-byte integer endianness appears in these two functions.
  2. If the byte already stored at object `+0x408` is 0: 16 raw bytes, then `FUN_146abe4b0` (`0x146abe4b0`) and `FUN_146ae35e0` (`0x146ae35e0`), not opened.
  3. If that byte is not 0: one byte that must be 0 or 1. A value of 1 calls `FUN_1461ad130` (`0x1461ad130`), the type-id reader in section 4, then two virtual calls. Those virtual calls were not opened.
  4. If the byte at object `+0xa0` is not 0: four big-endian `uint64` values.
  The common tail, for flag 0 and flag 1, is:
  1. 16 raw bytes, a big-endian `uint64`, a big-endian `uint64`, 16 raw bytes, a big-endian `uint32`, and one raw byte (`FUN_1461ac260`, `0x1461ac260`).
  2. A big-endian `uint32`, then 16 raw bytes, then 16 raw bytes (`FUN_1415acc30`, `0x1415acc30`).
  3. A big-endian `uint32`, a big-endian `uint32`, a big-endian `uint64`, and the same 8-byte big-endian integer as response field 2 (`FUN_146a6a5f0`, `0x146a6a5f0`, through `FUN_1461acd90`).
- `AddActorMessage` read `FUN_146a6a130` (`0x146a6a130`) then `FUN_146a6cde0` (`0x146a6cde0`):
  1. 16 raw bytes.
  2. One raw byte (`FUN_14087a190`, `0x14087a190`).
  3. One byte that must be 0 or 1 (`FUN_146a6d930`, `0x146a6d930`). A value of 1 calls `FUN_142d67640` (`0x142d67640`). That function calls `FUN_1461ad130`, the type-id reader in section 4, then virtual calls at offsets `0x10` and `0x28` on the type object. It reads no further integer of its own. A value of 0 reads nothing more in `FUN_146a6d930`.
  4. Two big-endian `uint64` values (`FUN_14087ad90`).
  5. One big-endian `uint64`.
  6. Two 4-byte values (`FUN_14087acf0`, `0x14087acf0`). Each 4 bytes is passed through `FUN_146167960` (`0x146167960`), which calls `WS2_32` ordinal 14. **Inferred:** ordinal 14 is `ntohl`, so each value is a big-endian `uint32`.
  7. One raw byte.
  8. Three length-prefixed strings (`FUN_14087a3d0`, `0x14087a3d0`). The length is the prefix code from section 4. A length of `0x30000` or more fails. The bytes that follow are raw.
  9. One big-endian `uint32`.
  10. One byte that must be 0 or 1.
  11. `FUN_1415b5870` (`0x1415b5870`): the three fields of `FUN_1415acc30` (one big-endian `uint32`, 16 raw bytes, 16 raw bytes), then one byte that must be 0 or 1.

**Observed.** `FUN_1469ec8b0` (`0x1469ec8b0`) reads grammar tokens `SpawnPlayerSpawner`, `SpawnEntity`, `SpawnCluster`, `ForceSpawnEntity`, and `Honeycomb`. `FUN_1469fccf0` allocates an object named `SpawnPlayerSpawner`. These three functions are not network message serializers.

**Observed.** `WaitingForPlayerSpawnPointMsg`, `ReceivePlayerSpawnPointMsg`, `DebugRequestSpawnPlayerMsg`, and `AddPortrayalToClientsMsg` occur only inside `InstallRegistrationHook` RTTI names. They do not have the plain name-plus-UUID catalog entry used above, and no field list was recovered for them. `PositionInTheWorldReplicatedState` is the same kind of RTTI name (`0x14a22c52e`).

**Observed.** In `FUN_14644a070`, state case `0xd` (`WaitingForPlayerSpawn`) polls `FUN_145a923c0` (`0x145a923c0`, 8 bytes, not opened) and `FUN_141026080` (`0x141026080`). When both are non-zero and a virtual call at offset `0x30` is non-zero, it logs `player spawn succeeds, in game` and stores state `0xe` (`InGame`). No message type name appears in this function.

**Observed.** One level out from those strings: `FUN_14646d460` (`0x14646d460`) calls `FUN_14644a070` and names no message. `FUN_146446800` (`0x146446800`) logs `Update state %s to new state %s` with the `InGame` string and `WaitingForPlayerSpawn` when the connection state at `+0x1530` is `0xe`, then calls `FUN_14645fd70` (`0x14645fd70`) with `0xd` and stores `0xd`. It names no message type. The address of the substring `player spawn succeeds` has no reference of its own. The log call in `FUN_14644a070` uses the longer text.

**Observed.** These names occur only inside mangled RTTI text. None of them has a code reference, and the bytes after the name are more mangled text, not a UUID catalog entry: `PlayerSpawnPointSuccessMsg`, `PlayerSpawnPointFailureMsg`, `PlayerSpawnPointMsg`, `SpawnPlayerMsg`, `WaitingForPlayerSpawnPointMsg`, `ReceivePlayerSpawnPointMsg`, `DebugRequestSpawnPlayerMsg`, `AddPortrayalToClientsMsg`. `NewReplica` and `CreateReplica` are absent. `FUN_1407f3500` (`0x1407f3500`) reads the string `Replicate`. `FUN_1407f3720` (`0x1407f3720`) reads `ReplicateClient`. Callers of `FUN_1407f3500` are `FUN_146a68f00`, `FUN_146a690d0`, `FUN_146a68d30`, and `FUN_146a70040`. None of those is the game-connection state function, and none of these names is a spawn message in the catalog of section 5.

**Unknown.** Which message creates the local player. `ReceiveActorMsg` and `AddActorMessage` have the layouts above. No network message whose name contains Spawn or Player was found. Still unopened from earlier passes: `FUN_146abe4b0`, `FUN_146ae35e0`, and `FUN_146a923f0`. The helpers named above and left closed are `FUN_1415bc630`, `FUN_146a6b260`, `FUN_1461ac530`, `FUN_14087a820`, and `FUN_146a6b5b0`.

## Negative searches

Looked up and not found as strings: `No login ticket received`, `credentials/omni`, `jwks.json`, `openid-configuration`, `CreateSteamSessionJob`, `Received login ticket` (the build says `Received a login ticket:`), `type_idx`, `SM_CONNECT`, `CONNECT_REQUEST`, `DATA_CHANNEL`, `kCarrierEndian`, `NewProxy`, `LZ4` (uppercase token), `LZ4_decompress_safe`, `SSL_set_verify`, `SSL_VERIFY_PEER`, `d2i_X509`. `ClientOverride.json` exists and has no code reference from this search; the endpoint note covers the override file. `X509_STORE_add_cert` and `PEM_read_bio_X509` exist as OpenSSL name strings and have no direct code references; the client calls the un-named functions in section 1.4.

## Unknowns

1. Which login-queue `Token` fields are mandatory, and whether `Signature` is checked.
2. How `RepAddress` is split into host and port.
3. Whether enum value `1` in the Omni token builder is Steam.
4. Whether Omni JWTs are verified.
5. The datagram header bytes. The `0x600`-byte send slot is filled by `memcpy` in `FUN_140f7fc60`. That function does not write a mode, format, or sequence byte, and it has no direct caller.
6. Which registration version is sent. All three request types are registered. No send call picks one of them. V3 field names are absent. The body of `FUN_1407d42c0` is recorded above.
7. The message `FUN_14644a070` waits on before `InGame`. The state function, its caller `FUN_14646d460`, and `FUN_146446800` name no message type. Spawn and Player message names in this build are RTTI text with no code reference.
8. The writer that emits `localIndex` (the reader and the file mapping are known).

## Implications for the preservation login server

The stock client, after it has a channel config, still gets a Steam auth-session ticket and can send that ticket on the auth-stack `/steam` call and as `SteamAuthTicket` inside `LoginQueueRequest`. The queue reply it parses is an object `LoginQueueResponse`. Queue position and transfer fields are on that object. `RepAddress`, `Signature`, `TicketId`, `WorldId`, `CharacterId`, and `PersonaId` are strings on the nested `Token` object when those keys are present. `RepAddress` is the string later printed when `StartREPConnection` runs.

The game-server path for `gridmate-udp` and `gridmate-udp-bsd` is given the embedded PEM and a cipher list of `ECDHE-RSA-AES256-GCM-SHA384`. No launch argument or config key in the endpoint note replaces that certificate. This note does not describe a way to change that trust decision.

`typeindex.json` from this install is the type-index table the message reader uses. A preservation server that speaks this build's messages has to use that same index, with index 0 meaning a following 16-byte UUID.
