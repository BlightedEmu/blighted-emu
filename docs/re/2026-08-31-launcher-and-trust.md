# Launcher address handling and certificate trust

Date: 2026-08-31
Build: Steam build `22469132`, `Bin64/NewWorld.exe`
Method: This note uses the notes already in this folder. No new disassembly was done. The game was not run. No game file was changed. Easy Anti-Cheat was not analyzed. The PEM body is not copied here.

Labels: **Observed** = stated as seen in code or data by a cited note. **Inferred** = a conclusion a cited note already marks as inferred. **Unknown** = not established. This note does not add function addresses.

Related notes:

- [2026-08-31 client endpoint configuration](2026-08-31-client-endpoint-configuration.md) (endpoint note)
- [2026-08-29 client protocol: login, GridMate, and world entry](2026-08-29-client-protocol-login-gridmate.md), section 1.1 for channel fields, section 1.4 for the certificate, and the implications section (protocol note)
- [2026-08-29 client install findings](2026-08-29-client-install-findings.md) (install note: build `22469132` only)
- Master architecture spec, local file `2026-08-29-emulator-architecture-design.md` (not in git). Sections 3, 4.1, 4.2, 4.3, and 5.
- `README.md` does not list listen addresses. It points at that local spec and at `config/client-build.toml`.

## Summary

- The client can point its HTTPS control-plane calls at another host without a binary change. It uses launch arguments, an optional override file, and the channel JSON. (Observed; endpoint note.)
- The game-server path for `gridmate-udp` and `gridmate-udp-bsd` trusts one certificate embedded in `NewWorld.exe`. No launch argument or config key replaces that certificate. (Observed; protocol note section 1.4 and the implications section; endpoint note.)
- The server config in this repo records listen addresses, the advertised `RepAddress` string, the cipher name, and the certificate subject. It does not contain the certificate. It does not contain a private key. This note does not generate a replacement CA. This note does not tell anyone to patch the client.

## Observed

### Launch arguments

The argument names and effects below are from the endpoint note. That note also lists the function addresses. This note does not add addresses.

The client reads named launch arguments with the Lumberyard command-line interface (`-Name value`).

| Argument | Effect |
|---|---|
| `-ChannelService <base-url>` | Replaces the base URL of the channel config request `<base>/<key>.json` |
| `-LoginChannelOverride <key>` | Replaces the channel key |
| `-GatewayMode dummy\|stubbed\|gateway` | Sets `client-connection.client-gateway.mode` |
| `-GatewayAddr` | Sets `client-connection.client-gateway.endpoint` |
| `-HttpGatewayAddr` | Sets `client-connection.client-gateway.http-endpoint` |
| `-GatewayRegion` | Sets `client-connection.client-gateway.region` |
| `-GatewaySigningHost` | Sets `client-connection.client-gateway.signing-host` |
| `-AuthEndpoint` | Sets `client-connection.authorization.auth-endpoint` |
| `-CognitoPool` | Sets `client-connection.authorization.cognito-pool-id` |
| `-Cluster` | Sets `client-connection.authorization.cluster` |
| `-DeveloperLoginAssumeRole` | Sets `client-connection.authorization.developer-login-role` |
| `-EnableLocalDev true` | Sets `client-connection.enable-local-dev` |
| `-ProductId`, `-AuthMode`, `-AuthBackend`, `-AuthManager` | Select product and auth backend (`steam`, `mock`, `sonic`, `scarlett`, `prospero`) |
| `-OverrideConfigPath <path>`, `-ConfigPath <path>` | Move the override and base config files |

In `gateway` mode with an empty endpoint, `ConfigureLogin` falls back to hardcoded API Gateway hosts. (Observed; endpoint note.)

### Channel service URL

Default base URL by Steam app id (Observed; endpoint note):

- `1063730` (retail) and unlisted ids: `https://d2c74t4zimux3r.cloudfront.net`
- `1426320`, `2069210`: `https://dvenz6p69b1j0.cloudfront.net`
- `1063620`, `1057210`, `1868770`, `2069200`, `1183900`, `0000000`: `https://db1l286nfcls8.cloudfront.net`

The request path is `<base>/<key>.json`. `-ChannelService` replaces `<base>`. `-LoginChannelOverride` replaces `<key>`. (Observed; endpoint note.)

When the product id is exactly `dev`, a built-in local channel is used and no HTTP request is made. That local channel has `apiEndpoint` `127.0.0.1` for `authStack` and `loginGateway`. The product-id function sets `dev` when Steam is not available. (Observed; endpoint note.)

### Channel JSON fields

The endpoint note says the channel JSON supplies `authStack` (`https://%s/Prod`), `loginGateway` and `JavelinGatewayServiceV2` (`https://%s/prod`), `JavelinGatewayService-CF`, and `platformMetadata` (`omniTokenUrl`, `nwTokenUrl`, `personaHostV2`).

The protocol note, section 1.1, is the per-tag reader. It does not use one template for `loginGateway` and `JavelinGatewayServiceV2`. This note follows that table for the URL the client builds. The endpoint note remains the source for the tag names and for the `platformMetadata` keys.

| `clientTag` | Fields the client reads | URL the client builds |
|---|---|---|
| `authStack` | `apiEndpoint`, `awsRegion` | `https://%s/Prod` |
| `loginGateway` | `apiEndpoint`, `awsRegion` | the endpoint string as stored |
| `JavelinGatewayServiceV2` | `apiEndpoint` | `https://%s/prod` |
| `JavelinGatewayService-CF` | endpoint host | logged as `Gateway Stack` / `Http Gateway Stack` |

`%s` is `apiEndpoint`. `authStack` uses `/Prod`. `JavelinGatewayServiceV2` uses `/prod`. Those spellings differ. (Observed; protocol note section 1.1.)

The same reader logs `Channel name: %s` from `channelName`, walks `platformMetadata`, and reads `displayName` for each cluster. It reads `omniTokenUrl`. The same parser also accepts `nwTokenUrl` and `personaHostV2`. (Observed; protocol note section 1.1, which points at the endpoint note for those two keys.)

The JSON key of the cluster array is not named. The JSON key that holds the public API objects is not named. The host field name for `JavelinGatewayService-CF` is not named. See Unknown.

`edge-http` must serve `<key>.json` and the endpoints that document names. (Endpoint note, effect on the design.)

### Config files

- The loader runs at start. No build-type check was seen before it. (Observed; endpoint note.)
- Base config: `@assets@/Client.json` in `DataStrm.pak`. It holds the transport type, `rep-default-port` `33435`, metrics and canary settings, and no HTTPS hosts. (Observed; endpoint note.)
- Override: optional loose file `@assets@/ClientOverride.json`. It can set all 55 `client-connection.*` keys, including the gateway and auth keys above. (Observed; endpoint note.)
- `client-connection.enable-remoteCfg = false` turns off the remote-config download from the fixed S3 buckets `ags-javelin-remote-config` (`-staging-`, `-dev-`). (Observed; endpoint note.)

Which file wins when a launch argument and `ClientOverride.json` set the same key is not stated. See Unknown.

### Hosts with no built-in setting

No argument or config key was found for `client.{catalog,entitlement,transaction,moderation,agsprivacysettings}service.amazongames.com`, the stats-service hosts, the CMS host `https://d1hkbwzm1bktgo.cloudfront.net`, or the Twitch redirect API. (Observed; endpoint note.)

### Game-server certificate

A PEM block in the executable decodes as one self-signed X.509 certificate. Subject and issuer are `C=US, ST=California, L=SNA11, O=Amazon, OU=Amazon Game Studios, CN=New World`. `notBefore` is 2025-08-04 23:15:36 GMT. `notAfter` is 2027-01-06 23:15:36 GMT. No `PRIVATE KEY` block is next to it. The PEM body is not copied here. (Observed; protocol note section 1.4.)

The endpoint note records the same certificate, adds RSA 2048, and records that no argument or config key changes it.

The transport reader uses that embedded PEM for the exact strings `gridmate-udp` and `gridmate-udp-bsd`. Any other transport string does not receive that PEM. (Observed; protocol note section 1.4.)

The secure-context builder sets the cipher list to the literal `ECDHE-RSA-AES256-GCM-SHA384`. When the PEM text pointer on the driver is set, the builder finds each certificate span, parses it, and adds each parsed certificate. Verify is then set with a null callback. (Observed; protocol note section 1.4.)

The implications section says: the game-server path for `gridmate-udp` and `gridmate-udp-bsd` is given the embedded PEM and a cipher list of `ECDHE-RSA-AES256-GCM-SHA384`. No launch argument or config key in the endpoint note replaces that certificate. That note does not describe a way to change that trust decision.

A separate string mentions `caPath` and a RETAIL / sandbox certificate choice. It has no code reference. It is not the REP trust path. (Protocol note section 1.4.)

### `RepAddress`

`RepAddress` is a string inside the login-queue `Token` object. It is not a field of the outer `LoginQueueResponse` object. `StartREPConnection` logs it as one `%s`. (Observed; protocol note section 1.1 and section 1.3.)

The architecture spec (section 4.2 and section 6) says `edge-http` returns `RepAddress` for a `rep-gateway` chosen by `world-director`. The client log prints that value as one string.

## Inferred

- `FUN_1478eff50` is `SSL_CTX_set_verify`. Modes `1` and `2` are the OpenSSL peer-verify modes. No hostname check appears in the two functions the protocol note names for that path. The PEM passed into the transport constructor is the text later parsed on the driver. (Inferred; protocol note section 1.4.)
- HTTPS control-plane calls use WinHTTP. `WinHttpQueryOption` is not imported, so those calls probably use the normal Windows trust store. (Inferred; endpoint note.) This is the control plane. It is not the game-server certificate above.
- Our `launcher` program (architecture spec section 4.1) can pass the launch arguments in the table above. That is the design. Whether the stock `NewWorldLauncher.exe` passes arguments is Unknown (endpoint note).

## Unknown

1. Whether `NewWorldLauncher.exe` passes launch arguments through to `NewWorld.exe`. (Endpoint note.)
2. Whether the retail file system reads a loose `@assets@/ClientOverride.json`. (Endpoint note.)
3. The exact JSON shape of `ClientOverride.json`. (Endpoint note.)
4. Whether the CMS cvars (`g_worldsCMSCloudfrontKey` and similar) are honored in a retail build. (Endpoint note.)
5. Which source wins when a launch argument and `ClientOverride.json` set the same key.
6. The JSON key of the cluster array, and the JSON key of the public API objects.
7. The host field name for `clientTag` `JavelinGatewayService-CF`.
8. Where `channelName` sits in the JSON object.
9. How `RepAddress` is split into host and port. The log prints one string. (Protocol note, Unknowns item 2.)
10. Whether the client rejects an empty `RepAddress` or an empty `Signature`. (Protocol note section 1.3.)
11. Where the private key for the embedded certificate is. It is not next to the PEM in the executable. It is not in this repo.
12. No function address is added here. A behavior that the cited notes do not tie to an address stays Unknown.

## What the server must present

Control plane:

- Serve the channel document at `<base>/<key>.json`. The launcher sets `<base>` with `-ChannelService` and `<key>` with `-LoginChannelOverride` when those arguments are passed.
- For `authStack`, the client requests `https://<apiEndpoint>/Prod`.
- For `loginGateway`, the client uses the `apiEndpoint` string as stored.
- For `JavelinGatewayServiceV2`, the client requests `https://<apiEndpoint>/prod`.
- Serve `platformMetadata.omniTokenUrl` at the URL in that field. The same object may include `nwTokenUrl` and `personaHostV2`.
- Put the game-server address in login-queue `Token.RepAddress` as one string.

Game server:

- The client keeps the embedded certificate as the trust anchor.
- The server presents a certificate with that subject and that issuer (the certificate is self-signed). The endpoint note also records RSA 2048. Validity is 2025-08-04 23:15:36 GMT through 2027-01-06 23:15:36 GMT.
- The cipher is `ECDHE-RSA-AES256-GCM-SHA384`.
- The private key is not in the client executable and is not in this repo.
- Do not replace that trust anchor with a new CA. Do not patch the client. No launch argument selects a different game-server certificate.

**Later finding.** The project now patches a copy of the client. `scripts/patch-client.py` writes a separate patched executable and keeps the official file. `docs/client-patch.md` gives the procedure.

## Server config in this repo

`nw-common` loads a TOML file and applies environment overrides. It does not define edge listen addresses. The workspace members line is `crates/*`, so `crates/nw-edge-config` is already a member.

| Path | Role |
|---|---|
| `crates/nw-edge-config` | Listen addresses, advertised `RepAddress`, recorded cipher and certificate subject, channel-field parser |
| `config/edge.toml` | Example file an operator can read |

`config/edge.toml` holds:

- `https_listen`: the `edge-http` HTTPS socket. The example uses port `443` (architecture spec section 3).
- `udp_listen`: the `rep-gateway` UDP socket. The example uses port `33435` (`rep-default-port` in the endpoint note).
- `rep_address`: the string placed in `Token.RepAddress`. The example text is `127.0.0.1:33435`. That spelling is the example value. This note does not claim the client splits that string on `:`.
- `[trust]`: the cipher name and the certificate subject fields. Load fails when they differ from the embedded certificate. The file has no PEM.

**Later finding.** The crate `crates/nw-edge-config` is gone. The config code is now in `crates/nw-common/src/config.rs` and `crates/nw-login/src/config.rs`. `config/edge.toml` has `https_listen` and `rep_address` (in `[[world]]`). It has `[tls]` and `[auth]` sections, not `[trust]`. `udp_listen` is in `config/rep-gateway.toml`.

The channel parser reads `apiEndpoint`, `awsRegion`, `clientTag`, and `omniTokenUrl`. It also reads `nwTokenUrl` and `personaHostV2` when those keys are present on `platformMetadata`. It counts `JavelinGatewayService-CF` objects and does not invent a host field for them. It finds `clientTag` objects at any depth outside `platformMetadata` because the parent key is Unknown. That walk is a parser rule, not an observed client loop.

Environment prefix: `NW_EDGE_`. Nested keys use `__`.

## Effect on the design

Architecture spec section 4.1 says the launcher sets endpoints and the trusted server certificate. The endpoint result is in the endpoint note: launch arguments and `ClientOverride.json` can set the control-plane endpoints, and remote config can be turned off.

The certificate half of section 4.1 has no launch-argument or config-key method. The client keeps the embedded trust anchor. Section 4.3 still names the cipher `ECDHE-RSA-AES256-GCM-SHA384` for `rep-gateway`. The server must present the embedded certificate's subject with that cipher. This repo does not ship a new CA and does not describe a client patch.
