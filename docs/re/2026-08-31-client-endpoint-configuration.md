# Client endpoint configuration (RE task 1)

Date: 2026-08-31
Build: Steam build `22469132`, `Bin64/NewWorld.exe`
Method: static, offline analysis of the user's own client (Ghidra project and disassembly listings). The game was not run, no game file was changed, and Easy Anti-Cheat was not analyzed.

Labels: **observed** = seen in code or data at the given address; **inferred** = concluded from layout or naming; **Unknown** = not established.

## Summary

- The HTTPS control-plane endpoints can be set without a binary change, through launch arguments and an optional config override file. (observed)
- No built-in setting was found that changes the certificate the game-server (UDP/DTLS) connection trusts. (observed: the transport code reads no certificate path, argument or config key)
- That embedded game-server certificate is valid until **2027-01-06**, 25 days before the announced shutdown. (observed)

## Launch arguments

The client reads 35 named launch arguments with the Lumberyard command-line interface (`FindArg` type 1, the `-Name value` form). (observed call pattern; the type-1 meaning is inferred from the Lumberyard interface layout)

Endpoint and login related arguments:

| Argument | Effect | Status |
|---|---|---|
| `-ChannelService <base-url>` | Replaces the base URL of the channel config request `<base>/<key>.json` (setup function `0x1464436a0`, URL builder `0x146431480`) | observed |
| `-LoginChannelOverride <key>` | Replaces the channel key (key builder `0x146430df0`) | observed |
| `-GatewayMode dummy\|stubbed\|gateway` | Sets `client-connection.client-gateway.mode` | observed |
| `-GatewayAddr` | Sets `client-connection.client-gateway.endpoint` | observed |
| `-HttpGatewayAddr` | Sets `client-connection.client-gateway.http-endpoint` | observed |
| `-GatewayRegion` | Sets `client-connection.client-gateway.region` | observed |
| `-GatewaySigningHost` | Sets `client-connection.client-gateway.signing-host` | observed |
| `-AuthEndpoint` | Sets `client-connection.authorization.auth-endpoint` | observed |
| `-CognitoPool` | Sets `client-connection.authorization.cognito-pool-id` | observed |
| `-Cluster` | Sets `client-connection.authorization.cluster` | observed |
| `-DeveloperLoginAssumeRole` | Sets `client-connection.authorization.developer-login-role` | observed |
| `-EnableLocalDev true` | Sets `client-connection.enable-local-dev` | observed |
| `-ProductId`, `-AuthMode`, `-AuthBackend`, `-AuthManager` | Select product and auth backend (`steam`, `mock`, `sonic`, `scarlett`, `prospero`) | observed |
| `-OverrideConfigPath <path>`, `-ConfigPath <path>` | Move the override and base config files | observed |

In `gateway` mode with an empty endpoint, `ConfigureLogin` (`0x146425000`) falls back to hardcoded API Gateway hosts. (observed)

## Channel service

- Default base URL by Steam app id: `1063730` (retail) and unlisted ids → `https://d2c74t4zimux3r.cloudfront.net`; `1426320`, `2069210` → `https://dvenz6p69b1j0.cloudfront.net`; `1063620`, `1057210`, `1868770`, `2069200`, `1183900`, `0000000` → `https://db1l286nfcls8.cloudfront.net`. (observed)
- The channel JSON supplies `authStack` (`https://%s/Prod`), `loginGateway` and `JavelinGatewayServiceV2` (`https://%s/prod`), `JavelinGatewayService-CF`, and `platformMetadata` (`omniTokenUrl`, `nwTokenUrl`, `personaHostV2`). (observed)
- A built-in local channel config with `apiEndpoint 127.0.0.1` for `authStack` and `loginGateway` is used, with no HTTP request, when the product id is exactly `dev`. The product-id function (`0x146439520`) sets `dev` when Steam is not available. (observed)

## Config files

- Loader at `0x14645b520`, runs at start-up; no build-type check was seen before it. (observed)
- Base config: `@assets@/Client.json` in `DataStrm.pak`. It holds the transport type, `rep-default-port` `33435`, metrics and canary settings, and no HTTPS hosts. (observed)
- Override: optional loose file `@assets@/ClientOverride.json`. It can set all 55 `client-connection.*` keys, including the gateway and auth keys above. (observed key list)
- `client-connection.enable-remoteCfg = false` turns off the remote-config download from the fixed S3 buckets `ags-javelin-remote-config` (`-staging-`, `-dev-`). (observed)

## Hosts without a built-in setting

No argument or config key was found for `client.{catalog,entitlement,transaction,moderation,agsprivacysettings}service.amazongames.com`, the stats-service hosts, the CMS host `https://d1hkbwzm1bktgo.cloudfront.net`, or the Twitch redirect API. These need DNS or hosts-file redirection plus a locally trusted HTTPS certificate. HTTPS uses WinHTTP; `WinHttpQueryOption` is not imported, so HTTPS probably uses the normal Windows trust store (inferred).

## Game-server certificate

The UDP transports use a certificate embedded in `NewWorld.exe`: self-signed, `CN=New World`, `O=Amazon`, `OU=Amazon Game Studios`, RSA 2048, valid 2025-08-04 to 2027-01-06. No argument or config key that changes it was found. (observed)

## Unknown

1. Whether `NewWorldLauncher.exe` passes launch arguments through to `NewWorld.exe`.
2. Whether the retail file system reads a loose `@assets@/ClientOverride.json` (a check: `Game.log` line "Using client config override file").
3. The exact JSON shape of `ClientOverride.json` (inferred: the same nested tree as `Client.json`).
4. Whether the CMS cvars (`g_worldsCMSCloudfrontKey` and similar) are honored in a retail build.

## Effect on the design (master spec 4.1, 4.2)

- The launcher can set the control-plane endpoints with `-ChannelService`, `-LoginChannelOverride` and the gateway/auth arguments, and can turn off remote config. RE task 1 for the HTTPS side is answered.
- `edge-http` must serve the channel JSON `<key>.json` and the endpoints it names.
