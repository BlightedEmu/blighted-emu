# Login flow follow-ups

Date: 2026-09-02
Build: Steam build `22469132`, `Bin64/NewWorld.exe`, image base `0x140000000`, x86-64, Windows compiler spec. Ghidra project `<re-scratch>/ghidra` (program `NewWorld.exe`, 355,815 functions).
Method: static analysis only, on the already-analyzed project, opened read-only with analysis left off (`analyzeHeadless <re-scratch>/ghidra NewWorld.gpr -process NewWorld.exe -noanalysis -readOnly`). One script pass (`FollowUps.java`) ran a fixed pool of 14 decompiler instances in parallel and decompiled 208 functions in 23 seconds. One small follow-up pass (`SuppDump.java`) added 179 functions. Offline inspection used the earlier corpora (`<re-scratch>/out-20260902`, 726 files; `/tmp/nw-shapes`, 97 files) with parallel ripgrep. Two small functions were disassembled straight from the executable with `objdump` (`FUN_1408e8550`, `FUN_141720c20`, `FUN_1479c94f0`). The game was not run. No game file, no Ghidra project file, and no repository code file was changed. Easy Anti-Cheat was not analyzed.

Labels: **Observed** = an address, a string, or decompiler output in this build. **Inferred** = a conclusion from that evidence. **Unknown** = not established.

Related notes:

- [2026-09-02 login flow JSON shapes](2026-09-02-login-json-shapes.md) (JSON note). Its section numbers are referenced below. This note resolves JSON-note unknowns 1, 3, 6 (partly), 7, and 9.
- [2026-08-29 client protocol: login, GridMate, and world entry](2026-08-29-client-protocol-login-gridmate.md) (protocol note)
- [2026-08-31 client endpoint configuration](2026-08-31-client-endpoint-configuration.md) (endpoint note)

## 1. Omni token call (`GetOrCreateToken`)

**Observed.** Endpoints. `FUN_1479c9160` (`0x1479c9160`) builds the token-service endpoint set for the `AgsFedAccTokenServGateway`:

- Paths (format call `FUN_1479d7aa0` with bound `0x400`): `/games/%s/tokens` at `0x1495da1f8` (the GetOrCreateToken action), `/games/%s/codes/generate` at `0x1495da210`, `/games/%s/merge` at `0x1495da230`, `/games/%s/platform/tokens/introspect` at `0x1495da240`. The `%s` is the game alias argument.
- Host by stage argument: `0` or any other value selects `https://tokenservice.amazongames.com` (`0x1495da1a0`); `1` selects `https://gamma.tokenservice.amazongames.com` (`0x1495da170`); `2` selects `https://beta.tokenservice.amazongames.com` (`0x1495da140`).

**Inferred.** The full GetOrCreateToken URL is `<token host>/games/<omniGameAlias>/tokens`, because the alias field of the OMNI metadata (JSON note section 2.1) is the only game-alias string on that path and the stage field selects the host.

**Observed.** Call chain. `FUN_1479c9e10` (`0x1479c9e10`) copies the four OMNI config strings (game alias, stage, token URL, NW token URL) and calls the request builder `FUN_1479c9a90` (`0x1479c9a90`) then the response handler `FUN_1479c9f00` (`0x1479c9f00`). The request body is the protocol-note section 1.2 object (`platformIdentityType`, `platformAuth`, `fallbackToken`, `agsAccountAuthToken`, `attributionData`). The reply is parsed by `FUN_1479b6d00` per JSON note section 4.

**Observed.** Session layer. `FUN_1479b3cb0` (`0x1479b3cb0`, OmniSDK `C:\JenkinsBuilds\OmniSDK\sdk\source\session\session.cpp`, log tag `Session` at `0x1495d71f0`) logs `CreateSession starts` (`0x1495d7230`), checks `SDK not initialized.` (`0x1495d7280`), and on token completion logs `Create federated account token result: %d. Updating gAgsFedToken.` (`0x1495d73b0`) or `Failed to create federated account token. Error: %d` (`0x1495d73f8`). `FUN_1479ca990` (`0x1479ca990`) refreshes the session: it reuses the request builder with the literal `refresh` (`0x1495da268`), and on `INVALID_FALLBACK_TOKEN` logs `Refresh session failed with invalid fallback token, retry creating a new session` (`0x1495da270`) and recreates the session. `FUN_1479b5420` (`0x1479b5420`) logs `GetOmniFederatedToken, buffer address: %p buffer length: %u` (`0x1495d7670`).

**Observed.** No `OMNI.` metadata entry. The OMNI config getter `FUN_146437fe0` (section 2) falls back to a built-in default entry (the map slot at channel-service object `+0x218`) when the keyed lookup misses, and the endpoint set of `FUN_1479c9160` has prod/gamma/beta defaults with no channel input. **Inferred:** with no `OMNI.` entry the SDK still calls the token service on the default host, so the call is not skipped and login continues. **Observed** support: the gateway path selector (protocol note section 1.1) uses the non-omni paths (`/jwt` or bare) when the auth manager is not in an omni mode, so an omni token is not required for the default flow.

**Observed.** Order against `/steam`. `FUN_145a936f0` (`0x145a936f0`, `OnAuthSessionTicket`) does the following in one callback, in this order:

1. Get the Steam auth session ticket and log `OnAuthSessionTicket: Authenticating using steam auth session ticket handle: %d`.
2. Append `/steam` to the auth stack endpoint and issue the `SteamAuth` HTTP call (`FUN_141722280`); the reply is parsed by `FUN_145a8ef80` into `AccessKeyId`/`SecretKey`/`SessionToken`/`Expiration` (JSON note section 3).
3. Call the persona client (section 3).

The Omni GetOrCreateToken call sits in the AGS session layer (session.cpp, above), which needs the Steam ticket as `platformAuth.ticket`. **Inferred:** the order is Steam ticket, then `/steam`, then the persona call, and the Omni token call after the ticket exists, in the AGS session creation. **Unknown:** the exact interleaving between the persona call of step 3 and the AGS session creation; both are driven by the auth manager (`FUN_145a8dce0` at `0x145a8dce0`) through indirect calls.

**Unknown:** which of `omniTokenUrl` and `nwTokenUrl` the SDK prefers, and where that override enters `FUN_1479c9160`'s endpoint set. Both strings are stored and returned by the getter; the consumer that applies them was not bound (all calls into the gateway are indirect; a raw scan of the whole `.text` for direct `call` encodings to `FUN_1479c9e10` and `FUN_146437fe0` found none).

## 2. `platformMetadata` keys (JSON note unknown 1)

**Observed.** Two separate mechanisms use the namespace strings:

1. The channel reader `FUN_146450250` (`0x146450250`) accepts any `platformMetadata` key whose text contains the substring `OMNI.` (string `0x148501274`) or `PERSONA.` (string `0x148501220`). It stores each parsed entry into a keyed map on the channel-service object (`FUN_1463f4c40` for OMNI at `param_1 + 0x218`, `FUN_1463f4ec0` for PERSONA at `param_1 + 0x188`).
2. The getters `FUN_146437fe0` (`0x146437fe0`, OMNI) and `FUN_146439090` (`0x146439090`, PERSONA) build the lookup key with `FUN_1408e8550` (`0x1408e8550`), which concatenates its two string arguments: the literal `OMNI.` or `PERSONA.` plus a suffix passed by the caller (the third argument, register `r8`, forwarded unchanged; verified by disassembly). The assembled key is looked up in a hash map with the FNV-1a constant `0xcbf29ce484222325` (bucket mask at `param_1 + 0x268`/`param_1 + 0x270` for OMNI, `param_1 + 0x1d8`/`param_1 + 0x1e0` for PERSONA).

**Observed.** Fallback inside both getters: when the direct lookup misses, the getter searches the assembled key text for the substring `.BRANCH` (string `0x1485012d0`; byte compare `.BRA`+`NC`+`H`), truncates the key at that position, and retries the lookup with the shorter key.

**Inferred.** The channel document keys are exactly `OMNI.<suffix>` and `PERSONA.<suffix>` with nothing before the namespace: the reader's substring rule accepts them, and the getter's exact-match lookup requires them. The `.BRANCH` retry shows the suffix can carry a branch decoration that the client strips for a second try. This matches the channel-key builder `FUN_146430df0`, which applies `.BRANCH` adjustments to the channel key itself (JSON note section 7).

**Unknown:** the concrete suffix value. It is supplied by the indirect caller of the getter, which was not bound in either pass. Candidates are the channel or branch name.

## 3. Persona (`personaHostV2`)

**Observed.** The persona service is the OmniSDK identity gateway. `FUN_1479c6720` (`0x1479c6720`, source `C:\JenkinsBuilds\OmniSDK\sdk\source\gateway\ags_identity_gateway.cpp`, log tag `AgsIdentityGateway` at `0x1495d99b0`):

- Request: `GET <identity host>/user` (path string `0x1495d9bd0`), header `Authorization` (`0x1482e7f18`; the packed immediate `Authoriz`+`ation` is also written inline).
- Host select: `FUN_1479c4560` (`0x1479c4560`) picks `https://prod.identity-service.amazongames.com` (`0x1495d9a28`), `https://gamma.identity-service.amazongames.com` (`0x1495d99f8`), or `https://beta.identity-service.amazongames.com` (`0x1495d99c8`) by stage, the same pattern as the token service.
- Reply: logged `GetGamerInfo result: %d` (`0x1495d9bd8`); a non-JSON body logs `Failed to parse GetGamerInfo response as JSON. Result code: %d. Http code: %d, Request ID: %s` (`0x1495d9c40`).

**Observed.** Other identity-gateway routes in the same cluster: `/federated/identities/user` (`0x1495d9db0`, GetAccountLinks, `FUN_1479c7900` at `0x1479c7900`), `/auth/token` (`0x1495d9cd0`), `/auth/platform/user/code` (`0x1495d9ce0`), `/auth/revoke` (`0x1495d9ef8`), `/auth/device/code` (`0x1495d9f08`), and an update-display-name call (`FUN_1479c7e40`, reply key `frozenUtil`, `Missing frozenUtil key from response.` at `0x1495da058`).

**Inferred.** `personaHostV2` overrides the identity-service host, `personaRegion` the stage/region, and `personaApiKey` the key used on those calls; the reader `FUN_146421a60` (JSON note section 2.1) stores exactly these three plus the role map. **Unknown:** where the override is applied; the wiring function was not bound.

**Observed.** Missing `PERSONA.` entry and the failure guard. In `FUN_145a936f0`, after the `/steam` call, the client checks the persona client object at `param_1[0x45]`: when the pointer is null, or its virtual method at `+0x10` reports not-ready, the client logs `Persona failed to initialize!` (`0x14844bbd8`), builds `@mm_authresult_Error_Persona`, and calls `FUN_145a9ae20` (`0x145a9ae20`) with the "gateway" provider tag. When the persona client is ready, the client calls its virtual slot `+0x58`, then slot `+0x20` or `+0x28` with the ticket string (`param_1 + 0x60`) and the Steam id string (`param_1 + 0x6a`). **Inferred:** the `@mm_authresult_Error_Persona` path stops the login, because the same `FUN_145a9ae20` channel carries the other fatal auth results (`@mm_authresult_Error_Steam`, `@mm_authresult_Error_InvalidAuthSetup`). **Unknown:** whether the persona client can be ready when the channel document has no `PERSONA.` entry (the SDK default host exists, so **Inferred:** yes, with the default identity-service host).

## 4. Gateway errors on the section 5 routes (JSON note unknown 7)

**Observed.** Every login route goes through one deserializer family: `FUN_1463e5e30` (`0x1463e5e30`, GetLoginInfo), `FUN_1463e63a0` (`0x1463e63a0`, ListWorlds), `FUN_1463e6650` (`0x1463e6650`, QueueLogin/RefreshQueuedLogin), `FUN_1463e68a0` (`0x1463e68a0`, ValidateCharacter and DeactivateCharacter), `FUN_1463e6ad0` (`0x1463e6ad0`, CreateCharacter), `FUN_1463e6d30` (`0x1463e6d30`). All share this structure:

1. Build the URL `https://<endpoint text>/prod<path>` and sign it with `SignatureV4` (`FUN_14747b0f0` with literal at `0x1486f9618`) using the `/steam` credentials.
2. Test the outcome's failure flag (for example `local_4c0` in `FUN_1463e5e30`).
3. On failure: skip the model parse entirely, build an error outcome through `FUN_1415ca580` (`0x1415ca580`), `FUN_1474e03b0` (`0x1474e03b0`), and `FUN_1463facb0` (`0x1463facb0`), and record the HTTP status at request-context `+0x80` and an operation id at `+0x84`.
4. On success: parse the envelope object (JSON note section 5).

**Observed.** So on a non-200 reply the client does not read the body of these routes at all. No error keys, no `x-amzn-ErrorType`, no `__type` are read on the login routes. The generic `AWSErrorMarshaller` `FUN_147481a00` (`0x147481a00`) does read `x-amzn-ErrorType`/`__type`/`Message`, but no caller of it was bound to any login route in either pass (its own error is `Failed to parse error payload`, code 100). **Inferred:** the login gateway routes never reach that marshaller; the preservation server's error body content is ignored by the client on these routes. Only the HTTP status matters.

**Observed.** What the user sees. The failure outcome reaches the connection layer, which:

- logs and reports through `FUN_146460240` (`0x146460240`), which logs `Report Error: Reason: ` (`0x1484feb08`) and drives the screen state (state ids `0x10`, `0x11`, `0x12` seen from `FUN_14641b020`);
- the main-menu UI exposes `ResetScreenAfterError` (`0x14831c150`) on its event bus (`FUN_1441d5100` at `0x1441d5100`, `FUN_1441ee020`, `FUN_1441f7210`);
- `FUN_14642aca0` (`0x14642aca0`) produces the gateway-level failure `Login gateway is not available` with code `NO_GATEWAY` (`0x1484fe258`, `0x1484fe278`) when the auth manager reports no gateway;
- `FUN_146419420` (`0x146419420`) shows the per-operation callback format for transfer operations: `Set Character To Transferable Callback - Is Success: %s` and `Set Character To Transferable Callback - Error Code: %s Message: %s` (`0x148500e60`, `0x148500ea0`).

**Inferred:** the visible text comes from the client's own localized strings keyed by the recorded status and operation id, not from the server's body. **Unknown:** the full mapping from HTTP status to the exact UI string; the localization lookup was not decompiled.

**Observed.** The stubbed mode short-circuits the network: the request builders (for example `FUN_146419ef0` for ValidateCharacter, `FUN_146416ab0` for ListWorlds) contain a local branch that fakes HTTP status `200` and an operation-specific id (`5` for the validator, `0xd` for ListWorlds) and returns a locally built result (ListWorlds uses `FUN_146427390` at `0x146427390` to build a fake world list).

## 5. `ValidateCharacter` (JSON note unknown 3)

**Observed.** Request side (`FUN_146419ef0` at `0x146419ef0`): path selector identical to the other routes (`/game/worlds/%s/characters/validator`, `.../jwt`, `.../jwt/omni`), world id as the `%s` argument (passed to `FUN_1463e68a0`), body `{ "ValidateCharacterBody": { "Name": "..." } }` (JSON note section 5.4).

**Observed.** Response side: the shared deserializer `FUN_1463e68a0` parses no model on success. It is the same function the DeactivateCharacter builder `FUN_1464148e0` (`0x1464148e0`, path `/game/characters/%s`) uses. Its success branch copies only outcome bookkeeping (`CommandData`-tagged record, `FUN_1461b3240`), no JSON fields. **Inferred:** validity is decided by the HTTP outcome alone: HTTP success means the name is valid; a non-200 or transport failure means invalid, with no body key consulted. This also resolves why no `ValidateCharacterResponseBody` string exists (JSON note negative searches).

**Observed.** UI wiring: the script-bind layer exposes `ENameValidationState_Valid`, `ENameValidationState_Invalid`, `ENameValidationState_Validating` (`0x14831c2a8` region), `ValidateName`, `ValidateNameResult`, and `UiCharacterNameValidationStatus` (strings in `FUN_144f40780` at `0x144f40780`; bus names `UiCharacterNameValidationStatusHandler`/`UiCharacterNameValidationStatusBus` at `0x1483b0b60`/`0x1483b7380`). So the character-create screen shows a three-state validation status driven by the validator call.

## 6. Date string formats (`FUN_147466a80`)

**Observed.** `FUN_147466a80` (`0x147466a80`) is a thin wrapper: it zeroes the output and calls `FUN_147466d90` (`0x147466d90`, 761 bytes) with the same string. `FUN_147466d90` selects a parser by its third argument (the format):

| Format | Parser | Evidence |
|---|---|---|
| `0` | `RFC822DateParser` (vftable symbol, parse `FUN_147467960` at `0x147467960`) | decompiled body |
| `1` | `ISO_8601DateParser` (parse `FUN_147467550` at `0x147467550`) | decompiled body |
| `2` | try `RFC822DateParser` first; if it fails, try `ISO_8601DateParser` | decompiled body |

**Observed.** Both parsers are hand-written digit scanners (calls to `isdigit`, a length guard that logs `Incoming String to parse too long with len ` at `0x1486f89b8`); neither uses `std::get_time`. The parsed fields fill a `tm`. The result is converted with `_mkgmtime64` (UTC) or `_mktime64` (local) and multiplied by `10,000,000`, giving 100-nanosecond ticks.

**Observed.** UTC rule: when the successful parse is the non-UTC form, the client logs `Non-UTC timestamp detected. This is always a bug. Make the world a better place and fix whatever sent you this timestamp: <value>` (`0x1486f8a10`, tag `DateTime`, verbosity 3 and above) and converts with `_mktime64`.

**Inferred:** format `2` (used for `Expiration` and the model date strings per JSON note sections 3 and 5.1.1) accepts RFC 822 text (for example `Wed, 21 Oct 2015 07:28:00 GMT`) or ISO 8601 text, with ISO 8601 read as UTC. **Unknown:** whether the ISO 8601 scanner accepts a trailing `Z`, a numeric UTC offset, or fractional seconds; the scanner was not walked field by field.

## 7. Character select screen

**Observed.** Data path. The GetLoginInfo reply is deserialized by `FUN_1463e5e30` through `FUN_1474e01e0` (envelope `LoginInfoList`) and `FUN_1474e42e0` (body). The world-selection handler `FUN_14641b020` (`0x14641b020`) then:

- reads the current world info with `FUN_146453440` (`0x146453440`) and logs `Selected World (%s): %s (%s)` (`0x1484fe700`) using the world id, the name at world-info `+0x28`, and the string at `+0x00`;
- iterates the character entries (`0x420` bytes each) and keeps the entries whose world-id field matches the selected world id (`memcmp`), copying matches with `FUN_140feab80`;
- compares one string field of each entry (entry `+0x10`) case-insensitively (`_stricmp`) with the string stored at `GameConnection + 0x1050` (the remembered selection);
- gates on the UI element `bt_selectExistingCharacter` (`0x147fcd708`) and offers `bt_deleteExistingCharacters` (`0x147fcd6a8`).

**Observed.** Name display. The script-bind layer `FUN_144f40780` exposes the character data to the UI with these binding names (string block `0x147f4bd50` through `0x148044b38`): `characterId`, `characterIdString`, `worldId`, `personaId`, `publishedSource`, `ftueCompleted`, `mustRename`, `mustRenameReason`, `nameLatentDate`, `nameModifiedDate`, `isNameLatent`, `CharacterNameData`, `displayName`, `displayNamePriority`, `guildName`, `publicName`, `nameString`, `nameIndex`. **Inferred:** the character tile shows `displayName` with `displayNamePriority`, sourced from the `NameReservations` array (the only `DisplayName` in the GetLoginInfo reply, JSON note section 5.1.2), because the character summary object itself carries no display name (JSON note section 5.1.1). **Unknown:** the exact join between a `Characters` entry and a `NameReservations` entry. The RPC descriptor pair (`Javelin.RPC.CharacterMetadata.owning_resource_id` / `character_id`) suggests `OwningResourceId` equals the owning character id, but the join code was not bound.

**Observed.** What must be set for a character to appear: an entry in `Characters` whose world id equals the selected world's id (`FUN_14641b020` match), plus a selectable world (section 8). No other required field was found: every model field is optional (JSON note section 1), and no minimum field set is enforced in `FUN_14641b020`. **Unknown:** whether the UI hides entries with `IsNameLatent` or `OwnerState` values; those checks would sit in the canvas layer, which was not decompiled.

## 8. World list

**Observed.** Selectability. `FUN_1464460d0` (`0x1464460d0`) is the world-selectable check. It returns selectable (`1`) when either:

1. a global override chain answers non-zero (vtable `+0x1b0`, then `+0x50`, then `+0x10` from `DAT_14a7ba0e0 + 0x60`; **Inferred:** a dev/override gate), or
2. `FUN_141720c20` (`0x141720c20`) accepts the `PublicStatusCode` and the world-info string at `+0x58` equals `ACTIVE` (string `0x148030eb4`, exact-length compare). The world-info layout from `FUN_14641b020` puts the status string at `+0x58` (the same field printed in the `(%s, %d)` message) and the integer at `+0x80` (`PublicStatusCode`).

**Observed.** `FUN_141720c20` is three instructions: `test ecx, 0xffffe8b7; sete al; ret`. It returns true only when `(PublicStatusCode & 0xffffe8b7) == 0`, that is, when the status code uses only bits inside `0x1748`. In particular, status `0` passes and status `1` fails.

**Observed.** World selection modes. `FUN_146453440` (`0x146453440`) resolves the world to use with the modes `user specified` (`0x1484fe0a0`), `first available` (`0x1484fe0b0`), and `none available` (`0x1484fe090`), and reads the config key `javelin.bot-distribute-world-logins` (`0x1484d03f8`). It calls `FUN_1464460d0` to validate the chosen world. **Inferred:** "first available" picks the first world that passes the ACTIVE/status check, and "none available" is the error when none pass.

**Observed.** `IsFull`, `MaxConnectionCount`, `MaxAccountCharacters`, and `WorldMetrics` have no consumer outside the model reader `FUN_1474e8e90` and the script-bind exposure in `FUN_144f40780`. No blocking or gating logic on them was found in either pass. **Inferred:** they are advisory display data. **Unknown:** any use inside the un-decompiled canvas layer.

**Observed.** The stubbed mode builds its own world list locally (`FUN_146427390` from the ListWorlds flow `FUN_146416ab0`).

## 9. Other hosts and routes between channel download and character select

**Observed.** The connection state machine `FUN_14644a070` (`0x14644a070`) orders the flow (state ids from the pointer table `0x1484f9ff0`):

| State | Name | What happens |
|---|---|---|
| 1 | `QueryGameUpdateCheck` | `FUN_1463e4000` (`0x1463e4000`) runs the update check job. Result 0 logs `QueryGameUpdateCheck: no game update check needed.` (`0x1484fe440`). |
| 2 | `WaitingForGameUpdateCheck` | Results: keep waiting; `failed to fetch game update status from first party.` (`0x1484fe4b0`); `no game update available.` (`0x1484fe478`); `a game update is available.` (`0x1484fe500`, then `FUN_146460240` with id `0x15`). The failure result falls through to the next state, like the no-update result. |
| 3 | `QueueGameLogin` | queue login begins (world and character already chosen at the UI level). |
| 5/6 | `QueryForRemoteConfigClass` / `WaitingForRemoteConfigClass` | iterate a class list (stride `0xb8` at `+0x1508`) through `FUN_1463e4550` (`0x1463e4550`); these run after the queue, not before character select. |

**Inferred:** the "first party" of the update check is the Steam SDK (no HTTP host string exists in the update-check chain, and the check result comes from a job object), and its failure is non-fatal (**Observed:** the failure text shares the fall-through path into `QueueGameLogin`).

**Observed.** Complete host list on the path from channel download to character select, with sources:

| Host / route | Source of the address | Blocking? |
|---|---|---|
| `<ChannelService base>/<key>.json` | `-ChannelService` argument or built-in CloudFront default (endpoint note) | fatal if it fails (no channel data) |
| auth stack `https://<apiEndpoint>/Prod/steam` | channel `authStack` publicApi | fatal: `@mm_authresult_Error_InvalidAuthSetup` / `@mm_http_NoResponse` |
| identity service `<personaHostV2 or identity-service host>/user` | channel `PERSONA.` metadata or stage default | fatal when the persona client is not ready: `@mm_authresult_Error_Persona` |
| token service `<omniTokenUrl or tokenservice host>/games/<alias>/tokens` | channel `OMNI.` metadata or stage default | not skipped without metadata (defaults); the non-omni `/jwt` paths work without its token |
| gateway `https://<endpoint>/prod/game/...` | channel `loginGateway`/`JavelinGatewayService` publicApi | fatal per operation (section 4); `NO_GATEWAY` when absent |
| CMS world/marketing/news data (`d1hkbwzm1bktgo.cloudfront.net` per the endpoint note) | CMS cvars | **Unknown.** The character-select UI has the handlers `OnWorldCMSDataSet`, `OnMarketingTilesCMSDataSet`, `OnNewsStoriesCMSDataSet` (`0x14831c1e0`-`0x14831c218` in `FUN_1441d5100`/`FUN_1441ee020`/`FUN_1441f7210`), so the fetch runs on this path. **Inferred:** non-blocking, because the handlers only fill optional display data and the screen has `ResetScreenAfterError` wiring independent of CMS. |
| notification service `client.agsnotificationservice.amazongames.com` (`/games/%s/platforms/%s/notification-credentials`, `0x1495da738`) | SDK stage defaults (`FUN_1479cb070` at `0x1479cb070`) | **Unknown.** SDK-level registration; no caller on the login path was bound. **Inferred:** async. |
| Twitch redirect APIs `wztz33l18j.execute-api.us-west-2.amazonaws.com/Live/twitchconvert|twitchredirect|twitchrefresh` (`0x1484d1340`-`0x1484d13e0`) | literal defaults | not on the base path; only the Twitch identity link uses them |
| remote config S3 buckets `ags-javelin-remote-config` (endpoint note) | fixed bucket names | after the login queue (states 5/6), not before character select |
| telemetry channel (`channelId`/`subchannelId` writers `FUN_147ae1be0`, `FUN_147ae9880`, `FUN_147ae3410`) | internal | **Inferred:** async fire-and-forget; no state depends on it |

**Observed.** No catalog, entitlement, or e-commerce route runs on this path: those model readers (`FUN_14744ce70` through `FUN_14754xxx`, `FUN_1474c2xxx`) belong to the store UI cluster and no call from the login state machine or the character-select flow to them was bound.

## Negative searches

Looked up and not found in this pass:

- Direct `call` encodings (raw `.text` scan, `E8 rel32`) to `FUN_146437fe0`, `FUN_146439090`, `FUN_1479c9e10`, `FUN_146416ab0`: none. All are invoked through interfaces or `std::function` targets.
- A caller of the `AWSErrorMarshaller` `FUN_147481a00` from any login route: none bound.
- A body model or body field read in `FUN_1463e68a0` (ValidateCharacter/DeactivateCharacter deserializer): none. The success branch parses no JSON.
- A gating use of `IsFull`, `MaxConnectionCount`, `MaxAccountCharacters`, or `WorldMetrics` outside the model reader and the script-bind: none found.
- A string `ValidateCharacterResult` body model, or any `x-` error header read on the section 5 routes: none (JSON note negative searches stand).
- An HTTP host string inside the game-update-check chain (`FUN_1463e4000`, `FUN_1463e4550`): none; both wrap job objects.

## Unknowns

1. The concrete suffix that completes the `OMNI.` / `PERSONA.` metadata keys (section 2); it is supplied by an indirect caller that was not bound.
2. Which of `omniTokenUrl` and `nwTokenUrl` the SDK applies, and where it overrides the stage-default token host (section 1).
3. The exact interleaving of the persona call and the AGS session creation inside the auth manager (section 1).
4. Where `personaHostV2`, `personaApiKey`, and `personaRegion` are applied to the identity-gateway calls (section 3).
5. The full HTTP-status-to-UI-text mapping on the section 5 routes, and the localization lookup for the `Report Error: Reason:` text (section 4).
6. Whether the ISO 8601 scanner accepts `Z`, numeric offsets, or fractional seconds (section 6).
7. The join between a `Characters` entry and a `NameReservations` entry for the displayed name, and any canvas-layer hiding rules for `IsNameLatent` or `OwnerState` (section 7).
8. Whether the CMS fetch failure blocks the character-select screen, and when the notification-service registration runs (section 9).
