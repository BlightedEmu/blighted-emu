# Remote config layers: our own config sets and the network path

Date: 2026-09-18. Build: Steam `25810808` (`NewWorld.exe`, sha256 `30fe6461…`; the patched
`BlightedNewWorld.exe` is a copy of it). Labels: **Observed** (seen in code, data, a log or a
test), **Inferred** (concluded from observed facts), **Unknown** (not established).

## Summary for a non-programmer

- The game reads many settings (XP multipliers, disabled items, event dates, leaderboard and
  analytics switches) from 16 small files that Amazon keeps on its own storage service (S3). The
  game finds those files by a fixed Amazon address that no setting can change.
- We do not use Amazon's files. We wrote our own files with the same names and the same layout.
  The game rules in them (for example "XP x2") are the same as retail. Every Amazon account
  number, Amazon address and Amazon "role" name is removed or replaced by our own. Our login
  server (`edge-http`) now gives these files to the game, exactly in the way Amazon's storage
  does.
- The game must reach our server when it asks for the Amazon address. Our launcher starts the
  game in a small private "box" (`bwrap`). Inside the box only, the Amazon storage name (and the
  two analytics names) lead to our server, and the game trusts our server's certificate for those
  names only. Nothing else on the PC changes: not the system hosts file, not DNS, not Steam, not
  `NewWorld.exe`, not the retail Wine prefix.
- The launcher now refuses to start the game in Steam's retail prefix, and it no longer starts
  the game through Steam's own launcher. Before this change our setup used the retail prefix, and
  nothing redirected the game's config requests, so they went toward the real Amazon storage
  (Inferred). Both stop now.
- To change a value later, edit our file and restart `edge-http`. The game asks again from time
  to time, sees the new file version, and takes the new value.

## 1. The two ways, compared

Way A: keep the retail files as they are and make the game read them.
Way B: our own config sets that the server owns (chosen).

| Point | Way A: retail files as they are | Way B: our own config sets (chosen) |
|---|---|---|
| Fidelity to retail | Byte for byte, but the files name Amazon's analytics stream, two Amazon accounts, Amazon IAM roles and three Amazon stats hosts. The client would try to use them. | Same file names, same layer order, same key names, same gameplay values. Only Amazon ids, roles, stream names and hosts differ (section 6). |
| Layer ids | Retail ids (`iad-prod`, the Amazon Cognito pool, the retail world id). Our client asks for `blighted`, our pool and our world ids (Observed, our Game.log), so retail files would not match without client changes. | Files are stored under the ids our client asks for. |
| Changing values (admin tools) | Not possible: the files are Amazon's. | Edit a file under `config/remote-config/` (or the folder the server config names) and restart `edge-http`. |
| Per world / per region | One fixed set. | The `WorldId` layer is one file per world id; the `RegionId` layer follows our cluster id. `edge-http` refuses to start when a world has no file. |
| Polling / ETag | Would need Amazon's ETags. | `edge-http` gives an S3 ETag (MD5 of the body, as S3 does for an object stored in one part) and answers `If-None-Match` with `304`. A changed file gets a new ETag, so the client takes it on its next poll. |
| Leaderboards / analytics endpoints | Amazon hosts. | `omni.leaderboards.client-endpoint` = `game.<domain>` (filled by the server from its own domain). Kinesis and STS names lead to `edge-http`, which answers them (leaderboards study). |
| Safety and law | Shipping or serving Amazon's files repeats Amazon account data. | No Amazon file is committed or served. |
| After a game update | The build key part changes (`6033;6024067` now). | Retail answers every build key with `404 NoSuchKey` (Observed, 8 of 8). We do the same, so a new build needs no file. A new key name only needs a file edit. |

**Decision.** Way B. Reasons: it is the only way that keeps Amazon data out of our files and our
network traffic, it matches the ids our client asks for, and it lets the server owner change
values.

## 2. Study: other ways for the client to get these values

### 2.1 The command-line layer `cmdLineCfg`

- **Observed.** The layer factory `FUN_14084c410` makes four layer kinds by type id: S3 (`DAT_14a2e9e20`),
  file (`DAT_14a2e9e24`), command line (`DAT_14a2e9e28`, name `cmdLineCfg`) and plain memory.
- **Observed.** Two functions write into the command-line layer: `FUN_14085df80` (manager vtable
  `0x147f4e740`, slot `+0x270`) replaces the whole layer with a key/value map, and
  `FUN_14084e220` (second vtable `0x147f4e9c0`, slot `+0x28`) is a two- or three-word command:
  two words list the keys that match, three words set one key to one string. It refuses a key the
  stack does not know: `Key "%s" not found, are you forgetting a prefix?  Ignoring command`.
- **Unknown.** Which code calls `FUN_14085df80`, and the command-line syntax (argument name,
  value format, nesting). A sweep search for the slot `+0x270` thunk (`0x140571778`) found only
  unrelated callers. No launch argument handler that reaches either function was found.
- **Unknown.** The place of `cmdLineCfg` in the stack relative to the remote layers.
- **Inferred.** Even with a syntax, the set command stores one string per known key. The layers
  hold arrays and nested objects (for example `javelin.dynamic-datatable-config`,
  `javelin.xp-multipliers.progression`). It cannot be shown to hold every key.
- **Result.** Rejected: syntax and precedence Unknown, and value shape too narrow.

### 2.2 A local (non-pak) stack file

- **Observed.** `FUN_14023cf60` defines six file-layer slots (`FUN_140fec8f0`, names from the table
  at `0x147fc4ed8`): `publicLegacy` (no file), `public` (`@assets@/stack.json`),
  `publicSpecialWorld` (default file), `publicOverride` (`@assets@/../stack.override.json`),
  `privateLegacy` and `private` (server only, `stack.server.override.json`).
- **Observed.** The client loader `FUN_146440360` makes a file layer only for slots whose flag
  byte is set. The flags are constants in that function: only slot 2 (`publicSpecialWorld`) is
  set. `stack.json` and `../stack.override.json` are never loaded by the client.
- **Observed (earlier note, run 12).** The file layer reads `@assets@/client.json` from a mounted
  pak only (`FUN_1470ca840`, pak priority 2).
- **Result.** No non-pak local stack file is read. Rejected.

### 2.3 Disabling remote config

- **Observed (endpoint note).** `client-connection.enable-remoteCfg = false` turns the S3 layers
  off, but that key is read from the pak-only `ClientOverride.json`. It would also drop every
  value in section 6. Rejected.

### 2.4 A byte patch of the host strings

- **Observed.** The S3 host is the literal `s3.amazonaws.com` with the bucket in front
  (virtual-host style; `FUN_140853a80` passes "use virtual addressing" = 1); Kinesis and STS hosts
  are built from `kinesis.` / `sts.` + region + `.amazonaws.com`.
- **Result.** Not needed: section 3 reaches the same goal without a change to any game file.
  Rejected (the user rule allows a patch only when no cleaner way exists).

## 3. Study: the network path (chosen route)

### 3.1 What the client does

- **Observed (our Game.log, 2026-09-16).** `Configuring remote config with bucket
  ags-javelin-remote-config`; it then asks for `applications/<app>/configuration-sets/<dim>/<id>/
  versionless` and `.../6033;6024067` for `ProductId/STEAM_APP_ID.1063730`, `RegionId/blighted`
  and, during login (`QueryForRemoteConfigClass` / `WaitingForRemoteConfigClass`),
  `WorldId/<our world id>`. It asks for no `CognitoId` key.
- **Observed (character-select note).** An empty dimension value skips that key. `RegionId` is
  `client-connection.authorization.cluster` (ours: `blighted`), `CognitoId` is
  `client-connection.authorization.cognito-pool-id`.
- **Observed.** `-CognitoPool <id>` sets that pool id (`FUN_146467680`). The other readers of the
  pool id are config parsers, the world list field `cognitoPoolId` sent to our own gateway
  (`FUN_146448d20`, `FUN_1461056f0`) and the two key builders (`FUN_14645e330`,
  `FUN_146469df0`). No format check was found.
- **Observed (run 12 note).** The S3 client (`FUN_140853a80`) uses region `us-east-1`, HTTPS,
  certificate checks on, no endpoint override and no proxy; it polls with `If-None-Match`.
  Credentials come from our auth stack. The client imports `WINHTTP.dll` and `WININET.dll` and the
  AWS SDK `WinINetSyncHttpClient` is present (Observed, imports and RTTI).
- **Observed (retail capture, private reference).** 8 `versionless` keys answer `200`
  `binary/octet-stream`; 8 build keys answer `404` `application/xml` `NoSuchKey`.
- **Observed (leaderboards study, captures).** Analytics uses `sts.us-east-1.amazonaws.com`
  (`AssumeRole`) and `kinesis.us-east-1.amazonaws.com` (`PutRecords`).

### 3.2 The route

1. `edge-http` answers the bucket host `ags-javelin-remote-config.s3.amazonaws.com` with our config
   sets (`crates/nw-login/src/remote_config.rs`): `GET /<key>` gives `200` with the body, type
   `binary/octet-stream` and the ETag, `304` when `If-None-Match` names the ETag (or `*`), and
   `404` S3 `NoSuchKey` XML for a key with no file. Requests are not signature-checked.
2. `edge-http` shows its own self-signed certificate for the `[aws_mirror]` names (SNI choice,
   `bins/edge-http/src/tls.rs`) and the ACME certificate for `login.` / `game.`. The mirror
   certificate is not a CA (`CA:FALSE`, server use only), so trusting it cannot make any other
   name trusted. `edge-http aws-mirror-certificate` makes it; it never replaces a file.
3. The launcher gets the host list and the certificate from `GET /launcher/info` (over the server's
   normal, publicly trusted HTTPS). It checks every host is a remote-config bucket, Kinesis or STS
   host, and that the certificate is one non-CA server certificate valid for each host (rustls
   WebPKI check; a CA is refused).
4. The launcher writes two private files next to its settings (`game-network/`): a hosts file
   (mirror names -> the server's addresses, then the system hosts file) and a trust file (system
   roots + the mirror certificate). It starts `bwrap --dev-bind / / --ro-bind <hosts> /etc/hosts
   --ro-bind <trust> /etc/ssl/certs/ca-certificates.crt -- <proton> run BlightedNewWorld.exe ...`.
   Only that process tree sees the two files.
5. The server sends `-CognitoPool <our id>` in the launch arguments, so the client also asks for
   its two `CognitoId` layers, which hold most gameplay values.

### 3.3 Evidence for the route

- **Observed (test).** `bwrap` 0.13.1 runs here without root. Inside `bwrap` with the private
  hosts file the bucket name resolves to `127.0.0.1`; outside it resolves to Amazon. `curl` inside
  the box trusts a self-signed `CA:FALSE` certificate added to the private trust file (`200`);
  outside the box the same request fails (`exit 60`).
- **Observed (tests).** A rustls client that trusts only the mirror certificate reaches every
  mirror host on `edge-http`'s TLS settings, and is refused on `login.`; the reverse also holds.
- **Inferred (Wine source).** Wine name lookups use the system resolver, so `/etc/hosts` applies.
  Wine's `crypt32` reads its roots from the first readable file in its list, and the first entry is
  `/etc/ssl/certs/ca-certificates.crt` (present here). **Unknown** until a live run: that this
  Proton build does both.

### 3.4 The Wine prefix

- **Observed.** The launcher setting recorded for live runs (`NW_LAUNCHER_STEAM_COMPAT_DATA`) and
  `docs/client-patch.md` pointed at `<SteamLibrary>/steamapps/compatdata/1063730`. That is Steam's
  own retail prefix: the retail Game.log of 2026-09-18 is in the same folder. **Our prefix was the
  same as retail's.**
- **Change.** The launcher refuses any prefix that ends in `steamapps/compatdata/1063730`. Use a
  folder of its own (for example `<drive>/Blighted/prefix`); Proton makes a new prefix there on
  the first start. `docs/client-patch.md` is updated.
- **Inferred.** The route above does not write the certificate into any prefix. Whether Wine
  copies the system roots it reads into the prefix registry is **Unknown**; with a prefix of our
  own, a copy can only land in our prefix.
- **Unknown.** Whether the game runs in a fresh Proton prefix with no Steam-installed runtime
  packages (Wine has built-in `msvcp140` / `vcruntime140`). Live test needed.

### 3.5 Rejected network routes

| Route | Why rejected |
|---|---|
| System `/etc/hosts` or DNS entry | Forbidden: it changes every program, including retail play. |
| CA in the system trust store | Forbidden: system-wide. |
| Wine registry `Root` store in a prefix | Works only per prefix; needs a Wine tool run per prefix; `bwrap` needs no prefix change at all. |
| `HOSTALIASES` | glibc applies it only to names without dots. |
| `LD_PRELOAD` resolver shim | Native code in the game process; more risk than `bwrap`. |
| `unshare -rm` + `mount --bind` | Maps the user to root inside the namespace and needs a shell step for the binds. `bwrap` keeps the user id and does the binds itself. |
| Steam launch (`steam -applaunch`) | Runs the retail exe in the retail prefix; we cannot give it a private view. Removed from the launcher. |

## 4. What was built

| Part | File |
|---|---|
| Mirror host rules, `AwsMirror` (launcher info) | `crates/nw-protocol/src/aws_mirror.rs`, `launcher_api.rs` |
| S3 answers, loading and checks of the config sets | `crates/nw-login/src/remote_config.rs` |
| Host routing (bucket host), launcher info | `crates/nw-login/src/host.rs`, `launcher.rs` |
| Config: `[remote_config]`, `[aws_mirror]`, `-CognitoPool` argument | `crates/nw-login/src/config.rs`, `config/edge.toml` |
| SNI certificate choice, `aws-mirror-certificate` command | `bins/edge-http/src/tls.rs`, `run.rs`, `main.rs` |
| Private network files, `bwrap` start, retail-prefix refusal | `bins/launcher/src/mirror.rs`, `game.rs`, `app.rs` |
| Our config sets (8 files) | `config/remote-config/applications/...` |

Server-side checks at start: every file is a JSON object; every layer the client asks for has a
`versionless` file (both applications x `ProductId STEAM_APP_ID.1063730`, `RegionId blighted`,
`CognitoId <cognito_pool_id>`, `WorldId <each world>`); the leaderboards layer has
`omni.leaderboards` and does not set `client-endpoint` itself.

## 5. Layers and file titles

The file path under `config/remote-config/` is the S3 key, so each file has the title of the
retail object. Layer order (lowest first): `ProductId`, `RegionId`, `CognitoId`, `WorldId`
(**Inferred** from the key builder order; retail `CognitoId` overrides `ProductId`, for example
`Musician:2` over `Music:2`). Both are kept as in retail.

| Retail key (private reference) | Our key |
|---|---|
| `public/.../ProductId/STEAM_APP_ID.1063730/versionless` | same |
| `public/.../RegionId/iad-prod/versionless` | `public/.../RegionId/blighted/versionless` |
| `public/.../CognitoId/us-east-1:<Amazon pool>/versionless` | `public/.../CognitoId/b04fbfcd-942c-472d-88c0-c762c4076b2f/versionless` |
| `public/.../WorldId/<retail world>/versionless` | `public/.../WorldId/8d0f4c2e-0000-4000-8000-000000000001/versionless` |
| the same four for `publicGameplay` | the same four for `publicGameplay` |
| 8 build keys `.../6033;6024067` (all `404`) | no file, so `404 NoSuchKey` |

Our pool id is a new random UUID; it names no Amazon pool. It has no `:` so the path is valid on
every file system.

## 6. Values: kept, replaced, added

Kept as in retail (game rules): every key of `ProductId`, `CognitoId` and `WorldId` in both
applications, and in `RegionId`: `analytics.region-name` and `kinesis-region-name` (`us-east-1`,
the region the client builds the Kinesis and STS names from), `overflow-kinesis-stream-name`
(empty), `event-types-to-disable`, `javelin.use-omni-penalties`, `use-persona-penalties`,
`leaderboard-rotation-startDate`, `weekly-leaderboard-reset-hour`,
`javelin-server.enable-twitch-system`, `omni.leaderboards.region`.

| Key (layer) | Retail value kind | Ours | How handled |
|---|---|---|---|
| `analytics.kinesis-put-role`, `overflow-kinesis-put-role` (public RegionId) | Amazon IAM role ARN, Amazon account | `arn:aws:iam::000000000000:role/blighted-analytics-put` | Replaced. **Observed (leaderboards study):** no decompiled function reads these two keys; the client builds its role from other keys. Our STS accepts any role. |
| `analytics.kinesis-stream-name` (public RegionId) | Amazon stream name | `blighted-analytics` | Replaced; also read by no decompiled function (same study). |
| `omni.leaderboards.client-endpoint` (publicGameplay RegionId) | Amazon stats host | `game.<domain>` | Not in the file; `edge-http` fills it from `domain`. |
| `omni.leaderboards.server-endpoint`, `ingestion-endpoint` | Amazon stats hosts | not served | Removed (leaderboards study: the game server side is our sim). |
| `omni.leaderboards.stats-assume-role`, `ingestion-assume-role` | Amazon IAM role ARNs | not served | Removed (same study). |
| `analytics.enable` (public RegionId) | not in retail layers | `true` | Added at the leaderboards study's request. |
| `javelin.enable-leaderboards`, `enable-leaderboards-ui`, `enable-leaderboards-stat-queries`, `query-leaderboards-service-from-client`, `enable-leaderboard-rotations`, `use-season-end-time-config` (publicGameplay RegionId) | not in retail layers | `true` | Added at the leaderboards study's request (hidden retail defaults). |
| `javelin.leaderboard-season-rotation` (same) | not in retail layers | `11` | Added; must equal `config/sim.toml` `[leaderboards] season`. |

A test (`tracked_sets_name_no_amazon_host_or_account`) fails if any served file names
`amazon`, `cloudfront`, `iad-prod` or either retail account number.

## 7. Safety and legal

- **No Amazon file is committed or served.** The retail replies stay in
  `local-data/remote-config/retail-2026-09-18/` (git-ignored, private reference). Our files were
  written by us: same key names and layout (needed so the client reads them) and gameplay values
  that are game rules. No Amazon account id, IAM role ARN, Kinesis stream name, Amazon host or
  Cognito pool id is in any tracked file.
- **No contact with Amazon.** Our server answers only requests that reach it. It never calls,
  signs for, or uses credentials of any Amazon service. The launcher maps only the three allowed
  name kinds, only for the game process, and only to the server the player chose. The certificate
  it trusts is not a CA and is checked to be valid for those names.
- **No system change.** No system hosts file, DNS, trust store or Steam setting changes. Retail
  play through Steam (`NewWorld.exe`, Easy Anti-Cheat, Steam's prefix) is not touched: the
  launcher refuses the retail prefix and no longer starts anything through Steam's launcher. No
  byte patch was added; `BlightedNewWorld.exe` is unchanged by this work (`docs/client-patch.md`).
- **No personal data.** No account id, token or character name from the capture is in a tracked
  file. The capture log lines quoted above contain only bucket and key names.
- **Licence and provenance.** All code is AGPL-3.0-or-later (launcher GPL-3.0-or-later). The key
  names and gameplay values come from the client's own replies seen by the user's own capture
  (private reference, not redistributed). Client facts come from our own static analysis of the
  user's installed client (Ghidra sweep `local-data/re-sweep`, build `25810808`). No third-party
  project's code or data was copied.

## 8. What the user must do before a live test

1. Make a new, empty folder for the launcher's prefix (not under `steamapps/compatdata/1063730`)
   and set `NW_LAUNCHER_STEAM_COMPAT_DATA` to it.
2. In the live `edge-http` config (`local-data/edge-http/edge.toml`): add the `[remote_config]`
   and `[aws_mirror]` blocks from `config/edge.toml` (use an absolute `directory` path when the
   setcap copy of `edge-http` runs from another folder). Remove the old `[client_config.javelin]`
   block: the server refuses unknown keys, so that block already stops it from starting
   (**Observed:** `EdgeConfig` has `deny_unknown_fields` and no `client_config` field since
   `db89c9b`).
3. Run `edge-http --config local-data/edge-http/edge.toml aws-mirror-certificate` once.
4. `edge-http` must listen on port 443 of the address the launcher looks up for `login.<domain>`:
   the client calls the Amazon names on the default HTTPS port. The live-stack script's port 8443
   does not catch them.
5. `bwrap` must be installed (it is: 0.13.1).

## 9. Unknown

1. The `cmdLineCfg` command-line syntax and its stack position (section 2.1).
2. Whether this Proton build reads `/etc/hosts` and `/etc/ssl/certs/ca-certificates.crt` as
   inferred (section 3.3). First live run: check Game.log for `Retrieved layer's
   "remoteCfg.public.ProductId" ... successfully`.
3. Whether setting `-CognitoPool` changes anything else in login (no other reader was found).
4. The retail ETag text (the capture kept no headers). Ours is the S3 single-part form.
5. The client's poll interval (`query-for-remote-config-class.interval-seconds` and
   `waiting-for-remote-config-class.interval-*` exist; values not read).
6. Whether the game starts in a fresh Proton prefix without extra runtime packages.
7. Whether Wine copies system roots into the prefix registry.
8. The exact layer precedence (section 5, Inferred).
9. The Windows path: the launcher's direct launch needs Proton and `bwrap`, so it is Linux only.
