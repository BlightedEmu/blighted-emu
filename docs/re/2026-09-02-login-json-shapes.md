# Login flow JSON shapes

Date: 2026-09-02
Build: Steam build `22469132`, `Bin64/NewWorld.exe`, image base `0x140000000`, x86-64, Windows compiler spec. Ghidra project `<re-scratch>/ghidra` (program `NewWorld.exe`, 355,815 functions).
Method: static analysis only, on the already-analyzed project, opened read-only with analysis left off (`analyzeHeadless <re-scratch>/ghidra NewWorld.gpr -process NewWorld.exe -noanalysis -readOnly`). Two headless script passes ran this way. The first (`LoginJsonShapes.java`, `SuppDump.java`) decompiled 726 functions and wrote them to `<re-scratch>/out-20260902`. The second (`NwDecomp.java`, `NwCallers.java`, `NwFindStr.java`, `NwStrRange.java`, `NwDumpTable.java`, `NwFuncRange.java`, `NwDataAt.java`) decompiled about 80 more functions to `/tmp/nw-shapes` and logged string and cross-reference finds to the Ghidra script log. Keys that the compiler packs into 8-byte immediates were decoded from the decompiler output. The game was not run. No game file and no Ghidra project file was changed. Easy Anti-Cheat was not analyzed.

REA `open_binary` was not used. That Ghidra provider imports the executable into a new project and runs auto-analysis. This pass reused the existing analyzed project instead, as the earlier pass did.

Labels: **Observed** = an address, a string, or decompiler output in this build. **Inferred** = a conclusion from that evidence. **Unknown** = not established.

Related notes:

- [2026-08-29 client protocol: login, GridMate, and world entry](2026-08-29-client-protocol-login-gridmate.md) (protocol note). Its sections 1.1 to 1.4 stay correct. This note adds the JSON document shapes that note did not have.
- [2026-08-31 client endpoint configuration](2026-08-31-client-endpoint-configuration.md) (endpoint note)
- [2026-08-31 launcher address handling and certificate trust](2026-08-31-launcher-and-trust.md) (launcher note). This note resolves its unknowns 3, 6, 7, 8, and 9.

## 1. JSON library and field primitives

**Observed.** One JSON library (cJSON, through the OmniSDK wrapper `omni::common::util::cjson`) serves the gateway model layer. The decompiled getter bodies call `omni::common::util::cjson::cJSON_IsObject`, `cJSON_IsNull`, `cJSON_GetObjectItemCaseSensitive`, `cJSON_GetStringValue`, and `cJSON_GetArraySize` by name (`FUN_147465730`, `FUN_1474654e0`, `FUN_147465390`, `FUN_1474651a0` at `0x147465730`, `0x1474654e0`, `0x147465390`, `0x1474651a0`). The double getter `FUN_147465440` (`0x147465440`) returns `*(double *)(cjson + 0x30)`, which is the `valuedouble` offset of cJSON on x86-64.

**Observed.** Field readers:

| Function | Use |
|---|---|
| `FUN_147465730` (`0x147465730`) | presence check (item exists and is not JSON null) |
| `FUN_147465410` (`0x147465410`) | read boolean |
| `FUN_147465490` (`0x147465490`) | read integer |
| `FUN_147465440` (`0x147465440`) | read double |
| `FUN_1474654e0` (`0x1474654e0`) | read string |
| `FUN_1474654b0` (`0x1474654b0`) | read object |
| `FUN_147465390` (`0x147465390`) | read array (returns the array size) |
| `FUN_1474651a0` (`0x1474651a0`) | read array element value |
| `FUN_147465220` (`0x147465220`) | iterate object entries |

**Observed.** Writers:

| Function | Use |
|---|---|
| `FUN_147465da0` (`0x147465da0`) | write string field |
| `FUN_147465a00` (`0x147465a00`) | write boolean field |
| `FUN_147465b40` (`0x147465b40`) | write integer field |
| `FUN_147465bf0` (`0x147465bf0`) | attach a child object under a key |
| `FUN_147465790` (`0x147465790`) | attach an array under a key |
| `FUN_147465160` (`0x147465160`) | write one array element |
| `FUN_147465d10` (`0x147465d10`) | write one map (string to string) pair |
| `FUN_147465e20` (`0x147465e20`) | serialize the root object |

**Observed.** Every field in every gateway model below is optional. The reader first calls the presence check; the writer first tests a presence byte on the source struct. A missing field keeps the default value (`""`, `0`, `false`). The exceptions are the `/steam` reply (section 3), the Omni token reply (section 4), and the cluster `publicApis` array (section 2).

## 2. Channel configuration document (`<key>.json`)

**Observed.** `FUN_146450250` (`0x146450250`) walks the root object of the channel JSON. Top-level keys:

1. `channelName` (string). Logged as `Channel name: %s`.
2. `platformMetadata` (object). Section 2.1.
3. Any other key is a cluster. The key text is the cluster id. Logged as `Found cluster data for cluster id: %s`. Section 2.2.

**Observed.** So the cluster list is not an array. It is a set of objects keyed by cluster id at the top level.

### 2.1 `platformMetadata`

**Observed.** `FUN_146450250` iterates the entries of `platformMetadata` and logs `Found platform metadata for key: %s`. For each key it searches the key text for the substring `OMNI.` (byte compare `0x494e4d4f` plus `'.'`) or the substring `PERSONA.` (byte compare `0x2e414e4f53524550`). The full key names are not in the binary. Only the namespace substrings are matched.

**Observed.** OMNI value object, read by `FUN_1464211f0` (`0x1464211f0`), four optional strings:

```json
{
  "omniGameAlias": "",
  "omniStage": "",
  "omniTokenUrl": "",
  "nwTokenUrl": ""
}
```

The endpoint note knew only `omniTokenUrl` and `nwTokenUrl`. `omniGameAlias` and `omniStage` are new. `omniStage` was verified from the immediate `0x67617453696e6d6f` plus `'e'` (9 characters).

**Observed.** PERSONA value object, read by `FUN_146421a60` (`0x146421a60`), three optional strings plus one object:

```json
{
  "personaApiKey": "",
  "personaHostV2": "",
  "personaRegion": "",
  "personaAccessRoles": {
    "<clusterId>": "<role>"
  }
}
```

Each role entry is logged as `Found persona role for cluster id: %s - %s`. `personaHostV2` sits in the PERSONA object, not in the OMNI object. This corrects the endpoint note, which placed all three URL keys in one reader.

**Inferred.** The metadata key that selects the OMNI parser reads `<prefix>OMNI.<name>` and the PERSONA key `<prefix>PERSONA.<name>`. The prefix and the exact names are server-side data. **Unknown:** the exact full key strings.

### 2.2 Cluster object

**Observed.** Each cluster value object has these fields:

| Field | Kind | Required | Notes |
|---|---|---|---|
| `localizedName` | string | optional | preferred. Logged as `Localized Name: %s`. |
| `displayName` | string | optional | used only when `localizedName` is absent. Logged as `Display Name: %s`. |
| `poolId` | string | optional | |
| `publicApis` | array | required | when absent the client logs `No public APIs for cluster id: %s. Ignoring!!` and drops the cluster |

**Observed.** Each `publicApis` element is one object with `clientTag` (string) plus, per tag:

| `clientTag` | Fields read | Result |
|---|---|---|
| `authStack` | `apiEndpoint` (required for the URL), `awsRegion` (optional) | URL `https://%s/Prod` (format call at `0x146451050` region of `FUN_146450250`). Logged `Auth Stack: %s`, `Auth Stack Region: %s`. |
| `loginGateway` | `apiEndpoint` (required), `awsRegion` (optional) | the endpoint string as stored, copied into the `Gateway Host` slot. Logged `Gateway Host: %s`. |
| `JavelinGatewayServiceV2` | `apiEndpoint` (required), `awsRegion` (optional) | URL `https://%s/prod`. Logged `Http Gateway Stack: %s`. |
| `JavelinGatewayService-CF` | `apiEndpoint` (required), `awsRegion` (optional) | `apiEndpoint` stored raw (`Gateway Stack`) and also formatted as `https://%s/prod` (`Http Gateway Stack`). |

Any other `clientTag` logs `Unknown client tag: %s`.

**Observed.** When the `loginGateway` `apiEndpoint` is present and the `Gateway Stack` slot is still empty, the loginGateway endpoint is also copied into `Gateway Stack`. The cluster entry is stored into the channel-service map (`FUN_1463f5110` on `param_1 + 0x28`) only when the `Gateway Stack` string is non-empty at the end of the cluster. **Inferred:** a cluster is registered only when at least one `publicApis` entry with tag `loginGateway` (or `JavelinGatewayService-CF`) supplies an `apiEndpoint`.

**Observed.** For each cluster the client also stores the literal application ARN `amzn1.application.e007ac0be8564845b02ef6a5e7852f70` (`0x148501110`). No channel field supplies it.

**Observed.** Error shape: the parser has no error document and no error keys. Every field is behind a presence check. The failure paths are log lines only (`No public APIs for cluster id: %s. Ignoring!!`, `Unknown client tag: %s`). **Unknown:** how the channel-service HTTP download itself reports failure (the download callback was not decompiled in either pass).

**Observed.** A built-in channel document exists in `FUN_1464436a0` (`0x1464436a0`) for product id `dev` (compared at `0x146443760` with `dev`). It is parsed by the same `FUN_146450250` with no HTTP request. Its text is the shape template:

```json
{
  "channelName": "localhost",
  "localhost": {
    "displayName": "Local Host",
    "localizedName": "@mm_localhost",
    "publicApis": [
      { "awsRegion": "us-east-1", "version": "1.0", "apiEndpoint": "127.0.0.1", "clientTag": "authStack" },
      { "awsRegion": "us-west-2", "version": "1.0", "apiEndpoint": "127.0.0.1", "clientTag": "loginGateway" }
    ]
  }
}
```

`version` appears in this document but the parser does not read it. **Inferred:** `version` is ignored.

## 3. Auth stack `/steam` call (Steam ticket exchange)

**Observed.** `FUN_145a936f0` (`0x145a936f0`, `OnAuthSessionTicket`) reads config keys `client-connection.client-gateway.signing-host`, `.endpoint`, `.http-endpoint`, `.region`, and `client-connection.auth-testing.steam-auth-failure` (`0x145a936f0` body, line reference `FUN_14033d610(..., "client-connection.auth-testing.steam-auth-failure")`). It takes the stored auth stack endpoint (global at `DAT_14a3f9ab0 + 0x10`). When that endpoint is empty it logs `No auth stack endpoint!` and fails with `@mm_authresult_Error_InvalidAuthSetup`. Otherwise it appends `/steam` (`0x14844bbb8`) and issues the `SteamAuth` HTTP call (`FUN_141722280` with arguments `10000`, `3000`, `5`).

**Observed.** The request body is built by hand in `FUN_145a88e40` (`0x145a88e40`) from these exact literals:

```json
{"steamAuthTicket":"<ticket>", "steamAppId":"<appid>", "branch":"<branch>"}
```

- `<ticket>` is the Steam auth session ticket string.
- `<appid>` is the 32-bit Steam app id written as decimal text.
- `<branch>` is the Steam branch name, lower-cased by the client before it is written. When the Steam interface returns no branch, the client omits the `", "branch":"<branch>"` part; the closing literal still closes the open value, so the document stays valid JSON.
- The client sets request header `accept: application/json` and `Accept-Encoding`.

**Observed.** The response is parsed by `FUN_145a8ef80` (`0x145a8ef80`):

| Reply key | Kind | Required | Evidence |
|---|---|---|---|
| `AccessKeyId` | string | required | presence check then `FUN_1474654e0` at `0x145a8f2xx`; immediate `0x654b737365636341` + `yId`, length 11 |
| `SecretKey` | string | required | immediate `0x654b746572636553` + `y`, length 9 |
| `SessionToken` | string | required | immediate `0x546e6f6973736553` + `oken`, length 12 |
| `Expiration` | string | optional | read through `FUN_147466a80` with format argument `2` (date conversion) |

The three required keys were re-verified this pass: the first is `AccessKeyId` (11 characters, immediate `0x654b737365636341` plus `0x644979`), not `AccessKey`.

**Observed.** Error shape, from the same function:

- HTTP status not 200: error result with the fixed message `HTTP request failed; responseCode=<status>` (message built at `0x145a8f150` region, status text from `FUN_1402b8820`), error code `0x10`, and a retry flag set when the status is above 499.
- HTTP status 200 with a body that is not JSON: message `JSON response parsing error; error=<raw body>`, error code `0x80`.
- HTTP status 200 with JSON but a missing required key: message `No credentials in response`, error code `1`.
- No JSON error object and no error key is parsed on this route.

**Inferred.** These four values are temporary AWS credentials, because `FUN_145a9f360` (`0x145a9f360`) names the provider `AuthStackCredentialsProvider` and logs a refresh (`Refreshed credentials...`), and the gateway request builder signs with `SignatureV4` (section 8).

## 4. Omni token call (`GetOrCreateToken`)

**Observed.** Protocol note section 1.2 records the request. Response handler `FUN_1479c9f00` (`0x1479c9f00`) parses the reply body as JSON and calls `FUN_1479b6d00` (`0x1479b6d00`). The keys and kinds, all read at the top level of the reply:

| Reply key | Kind | Required | Notes |
|---|---|---|---|
| `errorCode` | string | optional | section 4.1 |
| `penalties` | object | optional | checked at `0x1479b72xx` |
| `suspension` | object | optional | sub-reader `FUN_1479c1a40` (`0x1479c1a40`): `reason` (string), `expiration` (string) |
| `region` | **object** | optional | sub-reader `FUN_1479d5f50` (`0x1479d5f50`). Reads no string keys; **Unknown** internals |
| `limitedUseToken` | string | optional | |
| `accessToken` | string | required | required-key table `0x1495d7bf8` (points to `accessToken` at `0x1483539a8`) |
| `fallbackToken` | string | required | `0x1495d7c00` (points to `fallbackToken` at `0x1495d7ea0`) |
| `platformAccount` | **object** | required | sub-reader `FUN_1479c0fd0` (`0x1479c0fd0`) |
| `account` | **object** | required | when absent, error `0x133` with message `No AGS account info returned by federated account token service.` Sub-reader `FUN_1479c0620` (`0x1479c0620`) |
| `isNewAccount` | boolean | optional | read through `FUN_1479d6630` inside the `account` object |
| `conflictingAccount` | object | optional | read only when `errorCode` equals `0x134`; when it cannot be parsed the client logs `Merge required but no conflicting account provided by federated account token service.` |
| `expiresIn` | number (integer seconds) | optional | read through `FUN_1479d66e0`; the client multiplies by 1000 and logs `Federated token duration: %llds, received at %lldms` |

This answers the open question from the design spec: `platformAccount`, `account`, and `region` are all JSON objects, not strings.

**Observed.** `platformAccount` sub-object (`FUN_1479c0fd0`) reads string keys assembled from immediates: `ageGroup`, `platform`, `type`, `identity` (8-character immediate; full key not assembled), `personaId` (prefix `personaI`; full key not assembled), and the error value `ACCOUNT_IS_BANNED`. The `account` sub-object (`FUN_1479c0620`) reads the same family plus `shadow`. **Unknown:** the exact full key strings beyond `ageGroup`, `platform`, `type`, and `shadow`.

### 4.1 Error shape

**Observed.**

- HTTP-level, in `FUN_1479c9f00`: no response at all logs `No http response for GetOrCreateToken API request` (string at `0x1495da470`) and fails with result `1`; a body that is not JSON logs `Failed to parse create token response as JSON. Result code: %d. Http code: %d, Request ID: %s` and fails with result `0xcb`.
- Body-level, in `FUN_1479b6d00`: the client first checks the string key `errorCode`. When present it logs `Create token response error: %s`. The value `INVALID_FALLBACK_TOKEN` maps to client error `0xca`. Other values are looked up in a string-to-code table (`DAT_14a828fc8`, populated together with `PLAYER_IS_BANNED` and `INVALID_PLATFORM_AUTH`, `FUN_14028ed40` per the protocol note); an unknown value logs `Unrecognized response error code: %s`. A missing required key fails with `0xcb`.
- So the Omni error document is `{"errorCode":"<code>"}` at the top level, not a nested error object.

## 5. Gateway HTTP login APIs

**Observed.** Operation names sit in a pointer table at `0x1484fa070` (14 slots). Paths with a template exist in the string block at `0x148500b88` and after:

| Slot | Operation | Path |
|---|---|---|
| 0 | `GetLoginInfo` | `/game/getlogininfo`, `/game/getlogininfo/jwt`, `/game/getlogininfo/jwt/omni` |
| 1 | `CreateCharacter` | `/game/worlds/%s/characters` (+`/jwt`, `/jwt/omni`) (**Inferred** from the path block) |
| 2 | `DeactivateCharacter` | Unknown |
| 3 | `QueueLogin` | `/game/login/queue/v2` (+`/jwt`, `/jwt/omni`) |
| 4 | `RefreshQueuedLogin` | `/game/login/queue/v2/%s` (+`/jwt/omni`) |
| 5 | `ValidateCharacter` | `/game/worlds/%s/characters/validator` (+`/jwt`, `/jwt/omni`) |
| 6 | `RenameCharacter` | Unknown |
| 7 | `PatchWorldId` | Unknown |
| 8 | `LinkIdentity` | `/game/linkIdentity` |
| 9 | `UnlinkIdentity` | `/game/unlinkIdentity` |
| 10 | `SetCharacterToTransferable` | Unknown |
| 11 | `FreeTransfer` | Unknown |
| 12 | `QueueTransfer` | Unknown |
| 13 | `ListWorlds` | `/game/worlds/list` (+`/jwt`, `/jwt/omni`) |

The path selector for `GetLoginInfo` is `FUN_1464154e0` (`0x1464154e0`): no auth token or no `.cloudfront.net` gateway host selects the bare path; otherwise the auth manager state selects `/jwt` or `/jwt/omni` (protocol note section 1.1 gives the same rule for the queue paths). The queue selector pair is `FUN_146417490` (`0x146417490`) and `FUN_146417a70` (`0x146417a70`) with `FUN_14646fca0` (`0x14646fca0`).

**Observed.** Error shape for these routes: each reply parser looks up its envelope object by presence check only. When the envelope key is absent, the parsed result keeps its default values; no error document and no error key is read on these routes in this pass. On a failed HTTP call the callback receives the failure instead (protocol note section 1.3, `RefreshLoginQueueTicket failed: no login ticket.`). A generic AWS error marshaller exists in the same model layer, `FUN_147481a00` (`0x147481a00`, log name `AWSErrorMarshaller`): it reads the response header `x-amzn-ErrorType` (`0x1486f9f78`) or the JSON key `__type` (`0x1486f9f8c`), and the message keys `Message`/`message`. **Unknown:** whether the login-gateway routes ever reach that marshaller; no call from the login operations to it was bound in either pass. `statusCode` (integer), `errorCode` (string), `errorDescription` (string), and `details` (object) are read by `FUN_14752b4c0` (`0x14752b4c0`) and `FUN_14752c290` (`0x14752c290`), but those two parsers belong to the player-penalty API cluster (`FUN_1475399c0`, `FUN_14753b5e0`, `FUN_14753c250`), not to the routes in this note.

### 5.1 GetLoginInfo

**Observed.** Request body writer `FUN_1474ea680` (`0x1474ea680`), three optional fields, all written as JSON strings:

```json
{
  "channelId": "string",
  "includeNames": "string",
  "isTrialOwner": "string"
}
```

`includeNames` and `isTrialOwner` are strings, not booleans. **Inferred:** the server is expected to read them as string-encoded flags. A sibling writer `FUN_1474ea9b0` (`0x1474ea9b0`) writes `channelId` plus `tokenVersion`, and `FUN_1474eae20` writes `channelId` only. **Unknown:** which caller sends which variant.

**Observed.** Response: `FUN_1474e01e0` (`0x1474e01e0`) looks up the object `LoginInfoList` (`0x148703a90`) and passes it to `FUN_1474e42e0` (`0x1474e42e0`):

```json
{
  "LoginInfoList": {
    "Characters": [ { "": "character summary, 5.1.1" } ],
    "CrossRegionTransferCooldownMins": 0,
    "MaxChannelCharacters": 0,
    "NameReservations": [ { "": "name record, 5.1.2" } ],
    "PendingWorldMerges": [ { "": "merge record, 5.1.3" } ],
    "Worlds": [ { "": "world model, 5.2" } ]
  }
}
```

`Characters`, `NameReservations`, `PendingWorldMerges`, and `Worlds` are arrays (`FUN_147465390`). `CrossRegionTransferCooldownMins` and `MaxChannelCharacters` are integers. All six fields are optional (presence flag stored next to each).

#### 5.1.1 Character summary (element of `Characters`)

**Observed.** Reader `FUN_1474e1f60` (`0x1474e1f60`), entered through `FUN_1474df950` (`0x1474df950`) from the `Characters` array, and through `FUN_1474e1090` (`0x1474e1090`, envelope key `Character`) as the create-character reply. Writer mirror `FUN_1474ebec0` (`0x1474ebec0`). All fields optional; the field-name strings sit in one block at `0x148702c58` through `0x148702e30`:

| Field | Kind |
|---|---|
| `CharacterId` | string |
| `CreatedDate` | string |
| `FtueCompleted` | boolean |
| `IsFreshStart` | boolean |
| `IsNameLatent` | boolean |
| `IsTrialOwner` | boolean |
| `LocationGroupId` | string |
| `LocationId` | string |
| `ModifiedDate` | string |
| `MustRename` | boolean |
| `MustRenameReason` | string |
| `NameLatentDate` | string |
| `NameModifiedDate` | string |
| `NormalizedName` | string |
| `NeedsTransfer` | boolean |
| `NeedsTransferDate` | string |
| `OwnerState` | string |
| `PersonaId` | string |
| `PublishedData` | string |
| `PublishedElapsedSeconds` | integer |
| `PublishedSocialElapsedSeconds` | integer |
| `PublishedSocialSource` | string |
| `PublishedSource` | string |
| `RegionTransferDate` | string |
| `SocialData` | string |
| `TransferCrossRegionCooldownEndTime` | integer |
| `TransferData` | string |
| `TransferDate` | string |
| `TransferFreeCooldownEndTime` | integer |
| `TransferReason` | string |
| `Transferrable` | boolean |
| `WorldId` | string |

Dates are strings, not numbers. The character display name is not in this object; `NormalizedName` is the closest field.

#### 5.1.2 Name record (element of `NameReservations`)

**Observed.** Reader `FUN_1474e7090` (`0x1474e7090`), entered through `FUN_1474e0de0` (`0x1474e0de0`):

| Field | Kind |
|---|---|
| `CreatedDate` | string |
| `DeletedDate` | string |
| `DisplayName` | string |
| `IsLatent` | boolean |
| `IsTrialOwner` | boolean |
| `ModifiedDate` | string |
| `NameLatentDate` | string |
| `NormalizedName` | string |
| `Namespace` | string |
| `OwnerState` | string |
| `OwningResourceId` | string |
| `PersonaId` | string |

**Inferred:** the request flag `includeNames` (section 5.1) controls whether the server fills this array, because the character summary itself carries no display name.

#### 5.1.3 Pending world merge (element of `PendingWorldMerges`)

**Observed.** Reader `FUN_1474e8510` (`0x1474e8510`), three optional strings: `ScheduledDate`, `SourceWorld`, `TargetWorld`.

### 5.2 World model

**Observed.** Reader `FUN_1474e8e90` (`0x1474e8e90`), entered through `FUN_1474e1ae0` (`0x1474e1ae0`). All fields optional:

| Field | Kind |
|---|---|
| `ConnectionCount` | integer |
| `IsFull` | boolean |
| `IsRecommended` | boolean |
| `MaxAccountCharacters` | integer |
| `MaxConnectionCount` | integer |
| `PublicName` | string |
| `PublicStatusCode` | integer |
| `TransferToRegion` | string |
| `WorldId` | string |
| `WorldMetrics` | object, section 5.2.1 |
| `WorldName` | string |
| `WorldSet` | string (8-character key, immediate `0x746553646c726f57`) |
| `WorldStatus` | string |
| `WorldType` | string |
| `WorldVersion` | string |

#### 5.2.1 `WorldMetrics`

**Observed.** Reader `FUN_1474ea130` (`0x1474ea130`):

| Field | Kind |
|---|---|
| `QueueSize` | integer |
| `QueueWaitTimeSec` | integer |
| `WorldAgeDays` | integer |
| `WorldPopulationStatus` | string |

`WorldAgeDays` and `WorldPopulationStatus` were re-verified from the immediates (`0x656741646c726f57` + `Days`; string getter `FUN_1474654e0` on `WorldPopulationStatus`).

### 5.3 ListWorlds

**Observed.** Request wrapper `FUN_1474ecb00` (`0x1474ecb00`) emits the object `ListWorldsRequestBody` (`0x148703ad8`). Field writer `FUN_1474ec500` (`0x1474ec500`):

```json
{
  "ListWorldsRequestBody": {
    "CognitoPoolId": "string",
    "FilterParams": {
      "FilterType": "string",
      "Filters": { "<key>": "<value>" }
    }
  }
}
```

`Filters` is a map of string to string (`FUN_147465d10` writes each pair). The RPC descriptor in section 6 names the same entry type `FilterParamsEntry`.

**Observed.** Response: `FUN_1474e4110` (`0x1474e4110`) looks up the object `ListWorldsResponseBody` (`0x148703b08`) and passes it to `FUN_1474e3c30` (`0x1474e3c30`):

```json
{
  "ListWorldsResponseBody": {
    "Worlds": [ { "": "world model, 5.2" } ],
    "RecommendedWorlds": [
      { "RecommendationType": "string", "World": { "": "world model, 5.2" } }
    ]
  }
}
```

`RecommendedWorlds` element reader is `FUN_1474e8b80` (`0x1474e8b80`): `RecommendationType` (string) and `World` (object, parsed by the world model reader). Both arrays optional.

### 5.4 ValidateCharacter

**Observed.** Request writer `FUN_1474ed0d0` (`0x1474ed0d0`):

```json
{
  "ValidateCharacterBody": {
    "Name": "string"
  }
}
```

`Name` is the field at `DAT_147f020a8`. The bytes there read `4e 61 6d` (`Nam`), which completes to `Name` in both this pass and the first pass. The field is optional (presence flag at `+0x1f8`). The world id is not in the body. **Inferred:** it rides in the URL path `/game/worlds/%s/characters/validator`, because the path template takes one `%s`.

**Observed.** Client-side name rules: none found. The strings that exist are UI wiring (`UiCharacterNameValidationStatusHandler`, `UiCharacterNameValidationStatusBus` at `0x1483b0b60`, `0x1483b7380`) and a debug console command (`lc_validateCharacter`, usage line `Usage: lc_validateCharacter [world-id] [character-name]` at `0x147fd2de0`). **Inferred:** name validation is server-side only; the client asks the validator route and shows the result.

**Unknown:** the response body model. No `ValidateCharacterResponseBody` string exists and no reader was bound to it in either pass.

### 5.5 QueueLogin and RefreshQueuedLogin

**Observed.** Protocol note section 1.3 records both documents. Both passes confirm the readers and writers:

- Request: `FUN_1474ecc80` (`0x1474ecc80`) emits `LoginQueueRequest` (`0x148703c00`) when the presence flag at `+0x250` is set. Field writer `FUN_1474ec7b0` (`0x1474ec7b0`): `CharacterId` (string), `ClientCapabilities` (array of strings, elements written by `FUN_147465160`), `IsTrialOwner` (boolean), `SteamAppId` (integer), `SteamAuthTicket` (string), `WorldId` (string), each behind its own presence flag.
- Response: `LoginQueueResponse` object lookup `FUN_1474e89d0` (`0x1474e89d0`), body reader `FUN_1474e4f20` (`0x1474e4f20`), token reader `FUN_1474e5990` (`0x1474e5990`). Shape per protocol note section 1.3: `AllowQueueTransfer` (boolean), `EstimatedTime` (integer), `Position` (integer), `QueueName` (string), `RecommendedTransferWorldId` (string), `RefreshInterval` (integer), `TicketId` (string), and the optional nested `Token` object with the 21 fields the protocol note lists (`AccountAge` integer, `AccountIsLocked` boolean, `ChannelId` string, `CharacterId` string, `ClientCapabilities` string, `GenerateTime` double, `HostHash` string, `IsPermanentAppOwner` boolean, `IsTrialOwner` boolean, `IssueTime` double, `JwtClaims` string, `LocationGroupId` string, `LocationId` string, `PersonaId` string, `RepAddress` string, `Signature` string, `SteamAppId` integer, `SteamUserId` string, `TicketId` string, `TokenVersion` integer, `WorldId` string).
- `RefreshQueuedLogin` builder `FUN_146417a70` (`0x146417a70`) only selects the path `/game/login/queue/v2/%s` (or the `jwt/omni` form). No request body writer was observed for it. **Inferred:** the ticket id in the path is the whole request.

### 5.6 CreateCharacter

**Observed.** Request wrapper `FUN_1474ecf20` (`0x1474ecf20`) emits the object `CreateCharacterRequest` (`0x148702f30`) when the presence flag at `+0x250` is set. Field writer `FUN_1474ec2c0` (`0x1474ec2c0`):

```json
{
  "CreateCharacterRequest": {
    "CharacterCreationParams": "string",
    "ClientCapabilities": ["string"],
    "Name": "string",
    "SkipFtue": true,
    "SteamAppId": 0,
    "SteamAuthTicket": "string"
  }
}
```

All six fields are optional. `CharacterCreationParams` is written through the string writer (`FUN_147465da0`), so it is one JSON string, not an object. **Unknown:** the content of that string. **Inferred:** the world id rides in the URL path `/game/worlds/%s/characters`, so the body does not carry it.

**Observed.** Response: the operation builder `FUN_1463e6ad0` (`0x1463e6ad0`) parses the reply through `FUN_1474e1090` (`0x1474e1090`), which looks up the object `Character` (immediate `0x6574636172616843` plus `r`, 9 characters) and passes it to the character summary reader `FUN_1474e1f60`. So the create reply is:

```json
{ "Character": { "": "character summary, 5.1.1" } }
```

**Observed.** A separate writer `FUN_1474ecbc0` (`0x1474ecbc0`) emits an optional object `Character` (`0x147f837c8`) whose value is the full character summary. **Unknown:** which request carries it. No caller was bound in either pass.

### 5.7 LinkIdentity and UnlinkIdentity

**Observed.** Writers `FUN_1474ec9f0` (`0x1474ec9f0`) and `FUN_1474ecfe0` (`0x1474ecfe0`):

```json
{ "LinkIdentityRequestBody": { "SteamToken": "string", "TwitchToken": "string" } }
```

```json
{ "UnlinkIdentityRequestBody": { "TwitchToken": "string" } }
```

Both inner fields are optional strings.

## 6. Javelin.RPC descriptor layer

**Observed.** A second, separate message layer uses descriptor strings in `.rdata` around `0x1484bc000` to `0x1484c4000`. Each message has a full name and field-name descriptors. Login-relevant descriptors:

| Message | Field descriptors |
|---|---|
| `Javelin.RPC.GetLoginInfoRequest` | `channel_id` |
| `Javelin.RPC.ListWorldsRequest` | `cognitoPoolId`, `Filter` (`filterType`, `FilterParamsEntry.key`, `FilterParamsEntry.value`) |
| `Javelin.RPC.QueueLoginRequest` | `character_id`, `steam_user_id`, `world_id` |
| `Javelin.RPC.QueueLoginResult` | `ticket_id` |
| `Javelin.RPC.RefreshQueuedLoginRequest` | `ticket_id` |
| `Javelin.RPC.ValidateCharacterRequest` | `name`, `world_id` |
| `Javelin.RPC.LinkIdentityRequest` | `steam_token`, `twitch_token` |
| `Javelin.RPC.UnlinkIdentityRequest` | `twitch_token` |
| `Javelin.RPC.CreateCharacterRequest` | `world_id` |
| `Javelin.RPC.CharacterCreationParameters` | `name` |
| `Javelin.RPC.CreateCharacterResult` | `character_id` |
| `Javelin.RPC.DeactivateCharacterRequest` | `character_id` |
| `Javelin.RPC.CharacterMetadata` | `character_id`, `modified_date`, `must_rename_reason`, `must_transfer_date`, `name`, `name_latent_date`, `name_modified_date`, `persona_id`, `published_social_source`, `published_source`, `region_transfer_date`, `status`, `transfer_date`, `world_id` |
| `Javelin.RPC.CharacterNameData` | `character_name`, `creation_date`, `modified_date`, `name_latent_date`, `owning_resource_id`, `persona_id`, `region`, `world_id` |
| `Javelin.RPC.CharacterTransferData` | `from_world_id` |

**Observed.** Method paths for a stubbed service exist: `/Javelin.RPC.StubbedGatewayService/GetLoginInfo`, `/Javelin.RPC.StubbedGatewayService/ListWorlds`, `/Javelin.RPC.StubbedGatewayService/QueueLogin`, `/Javelin.RPC.StubbedGatewayService/ValidateCharacter`, and others. The same block contains gRPC markers (`grpc-status-details-bin`, `/helloworld.Greeter/SayHello`). The descriptor provider functions, for example `FUN_1460e37b0` (`0x1460e37b0`, returns `Javelin.RPC.CreateCharacterRequest`), only return the name strings. They do not serialize JSON.

**Inferred.** This layer is protobuf over gRPC-style method paths, used when `client-connection.client-gateway.mode` is `stubbed` or `dummy`. The field names above are protobuf field names, not JSON keys. **Unknown:** the protobuf wire encoding of these messages. No serializer for them was decompiled in either pass.

## 7. How `apiEndpoint` text becomes a URL, and the channel key length

**Observed.** The gateway request builder `FUN_1463e6650` (`0x1463e6650`, called with the operation path by `FUN_146417490` for the queue, and in the same shape by `FUN_1463e5e30`, `FUN_1463e63a0`, `FUN_1463e6ad0`, `FUN_1463e6d30`, `FUN_1464154e0`) builds the URL as:

1. write the literal `https://` into a 1024-byte stack buffer (`0x2f2f3a7370747468`);
2. `strcpy_s` the endpoint string stored at gateway object `+0xa0` directly after it, with bound `0x3f8`;
3. append the literal `/prod`;
4. `sprintf_s` the operation path (for example `/game/login/queue/v2`) after that.

So the request URL is `https://<endpoint text>/prod<operation path>`. The endpoint text is copied verbatim: no host or port parsing, no scheme strip, no userinfo strip, and no rejection of a `/` inside the text appears anywhere in this builder or in `FUN_14646fca0` (`0x14646fca0`), which only runs a `memchr` for `.` and a 15-byte compare for `.cloudfront.net` over the same string. **Inferred:** a path after the host in the `loginGateway`/`JavelinGatewayService` endpoint text is kept, so the path form `game.<domain>/s/<session>` produces `https://game.<domain>/s/<session>/prod/game/...`, and the host form `<session>.game.<domain>` produces `https://<session>.game.<domain>/prod/game/...`. Both forms survive. The endpoint text must not contain its own scheme, because the builder prepends `https://` itself; a scheme inside the text would be doubled (no strip was observed).

**Observed.** The buffer bound is the practical limit: the endpoint text, `/prod`, and the operation path must fit in `0x3f8` (1016) characters together, or the `strcpy_s`/`sprintf_s` calls fail and the request errors out (`local_7a0` failure branch). The request is also AWS SignatureV4-signed: `FUN_14747b0f0` is called with the literal `SignatureV4`, using the credentials from the `/steam` reply (section 3). **Inferred:** the server can ignore the `Authorization` header; the client sends it regardless.

**Observed.** Channel key length: no maximum was found. The key builder `FUN_146430df0` (`0x146430df0`) copies the `-LoginChannelOverride` value (or the built-in channel string) into an AZStd dynamic string, applies an optional `SANDBOX_ID.%s.` prefix (format at `0x146430e70` region) and `.CROSSPLAY.off`/`.BRANCH` adjustments, and formats the final key with `%s%s` (format string at `0x147fa18b0`, verified by raw dump: `25 73 25 73`). The channel document URL builder `FUN_146431480` (`0x146431480`) formats `%s/%s.json` into an AZStd dynamic string. The `-LoginChannelOverride` and `-ChannelService` values are read from the command line in `FUN_1464436a0` (`0x1464436a0`, lookups of `LoginChannelOverride` at `0x14644370e` and `ChannelService` at `0x1464441e5`). No length check or fixed buffer appears on the key anywhere in this chain. **Unknown:** whether any limit exists at all. **Inferred:** none; AZStd strings grow on demand. The 43-character base64url session key of the design spec is far below any observed bound.

## 8. Launch arguments needed besides `-ChannelService` and `-LoginChannelOverride`

**Observed.** `-ChannelService` and `-LoginChannelOverride` are read directly from the command line in `FUN_1464436a0` (`0x1464436a0`) and need no config file.

**Observed.** The other endpoint arguments are read once in the config loader `FUN_14645b520` (`0x14645b520`) and written into the cvar system; the loader also reads `@assets@/ClientOverride.json` (logs `Using client config override file: %s` / `Client config override file not found: %s`):

| Argument | Cvar set | Consumer |
|---|---|---|
| `-GatewayMode` | `client-connection.client-gateway.mode` | `FUN_146425000`, `FUN_146b6df50` |
| `-GatewayAddr` | `client-connection.client-gateway.endpoint` | `FUN_146425000` |
| `-HttpGatewayAddr` | `client-connection.client-gateway.http-endpoint` | `FUN_146425000` |
| `-GatewayRegion` | `client-connection.client-gateway.region` | `FUN_146425000` |
| `-GatewaySigningHost` | `client-connection.client-gateway.signing-host` | `FUN_146425000` |
| `-AuthEndpoint` | `client-connection.authorization.auth-endpoint` | read by `FUN_146465540` (`0x146465540`) |
| `-AuthMode`, `-AuthBackend`, `-DeveloperLoginAssumeRole`, `-ConfigPath`, `-OverrideConfigPath` | other `client-connection.*` keys | same loader |

**Observed.** `FUN_146425000` (`0x146425000`, `ConfigureLogin`) consumes the gateway cvars:

- mode `dummy` logs `ConfigureLogin MODE_DUMMY` and sets a flag.
- mode `stubbed` parses the endpoint as `host:port` (`FUN_140870880` into `FUN_140878e80`, the same splitter the protocol note records for `RepAddress`); port 0 becomes 5999.
- mode `gateway` (the default; any other value, including an absent argument, lands here through `FUN_146b6df50` returning 0): when `endpoint` is empty the client uses the hardcoded host `nsmby71o2f.execute-api.us-west-2.amazonaws.com` (46 characters, built from immediates at `0x146425356` region); when `http-endpoint` is empty it uses `https://82elumhw4i.execute-api.us-west-2.amazonaws.com/prod` (59 characters); when `region` is empty it uses `us-west-2`. An empty `signing-host` is filled from `endpoint`.

**Observed.** So none of `-GatewayMode`, `-GatewayAddr`, `-HttpGatewayAddr`, `-AuthEndpoint`, `-GatewayRegion`, `-GatewaySigningHost` is required: every one has a default, and the channel document (section 2) supplies the live values for `loginGateway` and `JavelinGatewayService` clusters. The retail client passes none of them (endpoint note). **Inferred:** the launcher needs only `-ChannelService` and `-LoginChannelOverride`; the channel document then carries the session in its endpoint values. **Unknown:** the precedence order when both a gateway cvar (set by an argument) and the channel document supply a value; the channel-document-to-gateway-object copy step was not decompiled in either pass.

## 9. Implications for the preservation login server

- Serve the channel document in the section 2 shape. Clusters are top-level keys, not an array. `publicApis` is required per cluster, and each cluster needs a `loginGateway` entry with `apiEndpoint` for the cluster to register. Include `localizedName` (or `displayName`). Put the omni metadata under a key that contains `OMNI.` and the persona metadata under a key that contains `PERSONA.`.
- The `/steam` endpoint on the auth stack must accept the hand-built body of section 3 (exact `", "` spacing) and return `{AccessKeyId, SecretKey, SessionToken, Expiration}`. All values are strings. The values are used as AWS SignatureV4 credentials for the later gateway calls, so any non-empty strings work.
- The Omni token reply must be a flat object with `accessToken`, `fallbackToken`, `platformAccount` (object), and `account` (object). `errorCode` is the error key. `region` is an object. `expiresIn` is integer seconds.
- `GetLoginInfo` returns the `LoginInfoList` object; `ListWorlds` returns `ListWorldsResponseBody`; `CreateCharacter` takes `CreateCharacterRequest` and returns `{Character:{...}}`; `ValidateCharacter` takes `ValidateCharacterBody.Name` only, with the world id in the path.
- Both endpoint forms of the design spec work: the endpoint text is inserted verbatim between `https://` and `/prod`. The channel key has no observed maximum.
- `QueueLogin` and its reply keep the protocol-note shapes. `Token.Signature` must be non-empty and `Token.RepAddress` must be `<host>:<port>` (default port `33435`) so the client leaves the queue (protocol note section 1.3).
- The stubbed-gateway RPC layer is a separate protobuf path. Do not serve it from the JSON endpoints above.

## Negative searches

Looked up and not found as strings in either pass:

- `GetLoginInfoResponseBody`, `ValidateCharacterResponseBody`, `CreateCharacterResponseBody` (no reply envelope names for these routes; the create reply uses `Character` instead).
- `GetLoginInfoRequestBody` (the request fields are written directly, section 5.1).
- `x-amzn-errortype` (lower-case; only `x-amzn-ErrorType` at `0x1486f9f78` exists).
- Client-side character name rules (no rule strings, regexes, or length checks found near the validator; only UI status-bus names and the `lc_validateCharacter` debug command).
- A length limit for the channel key (no fixed buffer on the `-LoginChannelOverride` value, the key builder, or the `<base>/<key>.json` URL builder).
- `jwks.json`, `openid-configuration` (already absent per the protocol note; still absent).

## Unknowns

1. The exact full key strings of the `platformMetadata` entries (only `OMNI.` and `PERSONA.` substrings are matched).
2. Which caller sends the `FUN_1474ea9b0` variant of the GetLoginInfo body (`channelId` plus `tokenVersion`), and what fills the `CharacterCreationParams` string in the CreateCharacter body.
3. The `ValidateCharacter` response body model (no envelope string, no bound reader).
4. Which request carries the optional `Character` object written by `FUN_1474ecbc0`.
5. The internals of the Omni `region` object (`FUN_1479d5f50` reads no string keys) and the exact full key strings of the `platformAccount` and `account` sub-objects beyond `ageGroup`, `platform`, `type`, and `shadow`.
6. The protobuf wire encoding of the `Javelin.RPC` messages.
7. Whether the login-gateway routes ever reach the AWS `AWSErrorMarshaller` (`FUN_147481a00`, `x-amzn-ErrorType` / `__type` / `message`), and the error document, if any, that the login gateway returns on a non-200 reply for the section 5 routes.
8. The precedence order between a gateway cvar (set by `-GatewayAddr` and friends) and the channel-document value for the same slot.
9. How the channel-service download itself reports failure (its HTTP callback was not decompiled).
