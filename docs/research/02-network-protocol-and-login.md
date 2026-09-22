# 02 - New World client-to-server network layer and login flow

Research date: 2026-08-29. Scope: public sources only. Purpose: interoperability and
preservation research for a server emulator that runs on a normal dedicated server
with no AWS dependency.

Conventions in this document:

- Each claim has a source link. Section 7 lists all sources.
- **Unknown** = no public source found.
- "unconfirmed" = one weak or community source only, not verified.
- "Community RE" = reverse-engineering notes from community repositories. These are
  not official. They can be wrong. They are build-specific.
- This document describes anti-cheat and certificate trust. It does not give bypass
  steps.

---

## 1. Summary

1. **Shutdown.** Amazon announced on 2026-01-15 that New World: Aeternum goes offline
   on all platforms on **2027-01-31**
   ([newworld.com](https://www.newworld.com/en-us/news/articles/the-future-of-new-world-aeternum-what-to-expect)).
   The announcement does not mention an offline mode, private servers, or a server
   software release.
2. **Two network planes.**
   - **HTTPS control plane**: channel configuration, Steam/OmniSDK authentication,
     AWS credential exchange, login info, character validation and creation, login
     queue, remote config from S3, news and MOTD. All of it uses fixed CloudFront,
     API Gateway, `*.amazongames.com` and S3 hostnames
     ([pastebin game log 2021](https://pastebin.com/4ynb16Rb),
     [first-light auth_mock.py](https://github.com/nw-private-server/first-light/blob/main/server/auth_mock.py)).
   - **Game data plane ("REP")**: UDP, DTLS 1.2, then a GridMate "Carrier" datagram
     layer, then typed AZ-reflected messages and replication "state bundles"
     ([first-light post-v3-flow.md](https://github.com/nw-private-server/first-light/blob/main/docs/post-v3-flow.md),
     [Aeternum-World packet_system.md](https://github.com/Coldzer0/Aeternum-World/blob/main/docs/Network/packet_system.md)).
3. **How the client gets the world server address.** The login-queue HTTPS response
   contains a field `RepAddress` = `host:port`. The client then opens the REP
   connection to that address (Community RE,
   [first-light auth_mock.py](https://github.com/nw-private-server/first-light/blob/main/server/auth_mock.py)).
4. **Engine networking.** Community binary scans say the client network library
   ("Javelin") is a fork of **Lumberyard GridMate**, not O3DE AzNetworking
   ([first-light gridmate-reference.md](https://github.com/nw-private-server/first-light/blob/main/docs/gridmate-reference.md)).
   The public Lumberyard GridMate source matches the observed wire format: big-endian,
   system channel 3, `SM_CONNECT_REQUEST = 1`, OpenSSL DTLS secure driver
   ([aws/lumberyard Carrier.cpp](https://github.com/aws/lumberyard/blob/master/dev/Code/Framework/GridMate/GridMate/Carrier/Carrier.cpp)).
5. **Encryption and compression.** DTLS 1.2 with cipher `ECDHE-RSA-AES256-GCM-SHA384`.
   Optional LZ4 raw-block compression of the Carrier datagram body (Community RE).
6. **Main blocker for an unmodified client.** The REP DTLS client trusts a
   **certificate embedded in NewWorld.exe**, not the Windows root store. A local
   HTTPS stub works with a local CA plus hosts-file redirect and no binary change. The
   DTLS handshake to a private REP server fails with `unknown_ca` unless the client is
   changed. A changed file on disk was refused at launch by EAC
   ([NewWorldPreservation README](https://github.com/AScriver/NewWorldPreservation),
   [REP_TRUST_POLICY.md](https://github.com/AScriver/NewWorldPreservation/blob/main/docs/REP_TRUST_POLICY.md)).
7. **Existing projects.** Two active community emulator efforts exist:
   `nw-private-server/first-light` (now defunct on GitHub, moved to an "OpenWorld"
   Discord) and `AScriver/NewWorldPreservation` (research workspace). A third
   repository, `Coldzer0/Aeternum-World` (AGPL-3.0), documents the packet layers. No
   project reached a playable in-world state as of their last public commits.

---

## 2. Findings by sub-topic

### 2.1 Ports, transport, hostnames, regions

#### 2.1.1 Official port guidance

Amazon help center article "Port Forwarding" (updated 2025-07-09), read through the
public Zendesk API
([JSON](https://help.newworld.com/api/v2/help_center/en-us/articles/36341884420507.json),
[HTML](https://help.newworld.com/hc/en-us/articles/36341884420507-Port-Forwarding)):

| Purpose | Protocol | Direction | Ports |
|---|---|---|---|
| New World | TCP | Inbound/Outbound | 80, 443 |
| New World | UDP | Inbound | 27000-27050 |
| New World | UDP | Outbound | 20000-59999 |
| Steam | UDP | Outbound | 3478, 4739, 4380 |
| Steam | UDP | Outbound | 27050-27100 |
| Steam | TCP | Inbound | 27015, 27036 |

- Strict NAT (Type 3) routers: forward 27000-27050 (same article).
- Firewall exception executable named in the article: `NewWorldLauncher.exe`.

Amazon help center article "Troubleshooting EAC Issues" (updated 2025-07-09)
([JSON](https://help.newworld.com/api/v2/help_center/en-us/articles/36413457011355.json),
[HTML](https://help.newworld.com/hc/en-us/articles/36413457011355-Troubleshooting-EAC-Issues)):

- "Forward Ports 18887 - 18903 set to Open". If blocked: "periodic disconnections or
  failures to enter the world".
- Third-party site portforward.com lists TCP 18887-18903, 27015, 27036 and UDP
  18887-18903, 27000-27050 for Steam ([portforward.com](https://portforward.com/new-world/)).

Interpretation (not confirmed): the UDP outbound range 20000-59999 covers the observed
REP server ports (see 2.1.3). The meaning of 18887-18903 is **Unknown**. It can be an
older REP port range or an EAC range.

#### 2.1.2 Known HTTPS hostnames (from client logs and community stubs)

Source: a public 2021 client `Game.log`, build `[RETAIL].Javelin.1.9.232.1441593`
([pastebin 4ynb16Rb](https://pastebin.com/4ynb16Rb)), and a 2025-12-27 log summary
plus hosts list in first-light
([connection-flow.md](https://github.com/nw-private-server/first-light/blob/main/docs/connection-flow.md),
[setup_hosts.py](https://github.com/nw-private-server/first-light/blob/main/tools/setup_hosts.py)).

Channel service (first request after start):

- `https://d2c74t4zimux3r.cloudfront.net/STEAM_APP_ID.1063730.json`
  (log line `Channel service uri: ...`, `Channel name: Retail`).
- The JSON gives, per cluster: display name, Cognito pool id, an "authStack" API
  endpoint and a "JavelinGatewayService-CF" endpoint, plus `platformMetadata` with
  OmniSDK settings (`omniTokenUrl`, `omniStage`, `omniGameAlias = new-world`)
  (Community RE, fallback schema in
  [auth_mock.py](https://github.com/nw-private-server/first-light/blob/main/server/auth_mock.py)).

Clusters and endpoints (identical in the 2021 log and the 2025 summary):

| Cluster id | Display | Auth Stack (API Gateway) | Gateway Host (API Gateway) | Gateway Stack (CloudFront) |
|---|---|---|---|---|
| fra-prod | EU Central | `2mfrik7h83.execute-api.us-east-1.amazonaws.com/Prod` | `u433g9r00c.execute-api.eu-central-1.amazonaws.com` | `d1w0bfy6smo4d1.cloudfront.net` |
| gru-prod | (SA East) | `kqqt5twsi7.execute-api.us-east-1.amazonaws.com/Prod` | `j4n6whncmi.execute-api.sa-east-1.amazonaws.com` | `d1cjlmzk0xrm0z.cloudfront.net` |
| iad-prod | (US East) | `eudbjx6mig.execute-api.us-east-1.amazonaws.com/Prod` | `0prplal5u1.execute-api.us-east-1.amazonaws.com` | `d2oeuvxi3kfsrw.cloudfront.net` |
| pdx-prod | US West | `q8hqllbg6k.execute-api.us-east-1.amazonaws.com/Prod` | `v7irlu1nrl.execute-api.us-west-2.amazonaws.com` | `d3bj4csovi1fe8.cloudfront.net` |
| syd-prod | (AP Southeast) | `hhf8nn71vb.execute-api.us-east-1.amazonaws.com/Prod` | `ep1m9qoir8.execute-api.ap-southeast-2.amazonaws.com` | `de4mfzk9wkelz.cloudfront.net` |

- All Auth Stacks are in `us-east-1`. Gateways are regional
  ([pastebin 4ynb16Rb](https://pastebin.com/4ynb16Rb)).
- The 2021 log also names non-production cluster ids: `fra-qa`, `fra-staging`,
  `iad-staging`, `pdx-devo`, `pdx-qa` (same source).
- Display names in parentheses are not in the quoted log lines; they are inferred from
  the AWS region. "EU Central" and "US West" are in the sources.

Other hostnames in the first-light hosts list
([setup_hosts.py](https://github.com/nw-private-server/first-light/blob/main/tools/setup_hosts.py)):

- Five more API Gateway hosts: `8otrnl8e8d` (eu-central-1), `ei4rb8pwvd` (sa-east-1),
  `3khxmonavl` (us-east-1), `mwsyzhfoe5` (us-west-2), `9jlv7mxmw1` (ap-southeast-2),
  all `*.execute-api.<region>.amazonaws.com`. Their role is **Unknown**.
- `d1hkbwzm1bktgo.cloudfront.net`: MOTD (`/motd/worlds_*.json`), news, marketing tiles.
- `client.content-service.amazongames.com`, `client.catalogservice.amazongames.com`,
  `client.entitlementservice.amazongames.com`,
  `client.agsprivacysettingsservice.amazongames.com`.
- `tokenservice.amazongames.com`: OmniSDK token service.
- `ags-javelin-remote-config.s3.amazonaws.com`: remote configuration layers.

Remote configuration (S3 keys from the log, [pastebin 4ynb16Rb](https://pastebin.com/4ynb16Rb)):

- `applications/public/configuration-sets/RegionId/<cluster>/versionless`
- `applications/public/configuration-sets/RegionId/<cluster>/<branch>;<build>`
  (example `232;1441593`)
- Same keys under `applications/publicGameplay/...`, and per `CognitoId`, `ProductId`,
  and (2025) per `WorldId`
  ([connection-flow.md](https://github.com/nw-private-server/first-light/blob/main/docs/connection-flow.md)).
- Log line `Using remote config: true`.

Client override hook (important for redirection):

- The 2021 log contains `Client config override file not found:
  @assets@/ClientOverride.json` ([pastebin 4ynb16Rb](https://pastebin.com/4ynb16Rb)).
- The file format and the keys it accepts are **Unknown**. No public source documents
  it. Whether the current build still reads it is **Unknown**.

#### 2.1.3 Game server (REP) transport and ports

- AWS blog: each world has **four remote entry points (REPs)**. The REPs are the only
  public IP addresses. Each REP is a C5.2xlarge EC2 instance. Behind them, seven
  C5.9xlarge instances form a "hub". The world map has a grid; each hub owns grid
  cells. 30 simulation updates per second. Up to 2,500 players and more than 7,000 AI
  entities per world. State is written to DynamoDB
  ([AWS for Games blog, 2022-04-06](https://aws.amazon.com/blogs/gametech/the-unique-architecture-behind-amazon-games-seamless-mmo-new-world)).
- Transport: UDP with DTLS 1.2. A Wireshark text export of a community capture shows
  `User Datagram Protocol` then `Datagram Transport Layer Security`,
  `Version: DTLS 1.2 (0xfefd)`, then a "New World GridMate Carrier" layer
  ([wireshark_carrier_handshake_redacted_session1.txt](https://github.com/nw-private-server/first-light/blob/main/info/wireshark_carrier_handshake_redacted_session1.txt)).
- Contradiction: first-light `connection-flow.md` says "Direct TCP connection to game
  server ... port 25493" and "NOT WebSocket". Later first-light and other docs say UDP
  and DTLS. The capture supports **UDP**. The "TCP" statement is probably an error in
  the early notes (unconfirmed)
  ([connection-flow.md](https://github.com/nw-private-server/first-light/blob/main/docs/connection-flow.md)).
- Observed REP ports: 25493 on a live server IP in an AWS Global Accelerator range
  (2025 log summary). Community stubs listen on UDP 24083 (first-light README) and
  23971 (NewWorldPreservation reading of `rep_responder.py`). The live port selection
  rule is **Unknown**. The client reads config keys
  `client-connection.transport-options.transport-type` (values `gridmate-udp` or
  `gridmate-udp-bsd`) and `client-connection.transport-options.rep-default-port`
  (Community RE,
  [REP_TRUST_POLICY.md](https://github.com/AScriver/NewWorldPreservation/blob/main/docs/REP_TRUST_POLICY.md)).
- World-list status for players: the public server-status web page
  `https://www.newworld.com/en-us/support/server-status`. Community bots scrape its
  HTML; no public JSON API was found
  ([nothub/newworld-servers](https://github.com/nothub/newworld-servers),
  [ArtiomSu getData.js](https://github.com/ArtiomSu/NewWorld-Server-Status-Telegram-Bot/blob/main/getData.js)).
  A 2021 forum thread says the team had no public API
  ([forums.newworld.com](https://forums.newworld.com/t/public-api-planned/252130)).

#### 2.1.4 Regions

- Amazon "Supported Regions" lists countries per platform (Steam, Xbox, PlayStation)
  ([help article 36341196232859](https://help.newworld.com/hc/en-us/articles/36341196232859-Supported-Regions)).
- Server transfers: only within a region; cross-region transfer is not supported
  ([help article 36303140415515](https://help.newworld.com/hc/en-us/articles/36303140415515-Server-Transfers)).
- Cross-play is on by default; platform-only worlds exist for PS5 and Xbox. No cross
  progression ([help article 36303863369499](https://help.newworld.com/hc/en-us/articles/36303863369499-Cross-Play-Features)).

### 2.2 Login and authentication flow

Order of events, combined from the 2021 log
([pastebin 4ynb16Rb](https://pastebin.com/4ynb16Rb)) and the 2025 log summary
([connection-flow.md](https://github.com/nw-private-server/first-light/blob/main/docs/connection-flow.md)).
Endpoint paths come from community stubs
([auth_mock.py](https://github.com/nw-private-server/first-light/blob/main/server/auth_mock.py)).

1. **Steam start.** `Steam Initialized!`, `Using Steam App ID: 1063730`. Steam must
   run.
2. **Channel config.** GET channel JSON from CloudFront (2.1.2). Client selects the
   cluster (`ClusterId: fra-prod`, etc.).
3. **EAC client module.** `[EasyAntiCheat] Initializing EAC game client module...`
   `...initialized successfully.` This occurs before sign-in.
4. **Steam session ticket.** `[SteamAuth] Getting steam auth session ticket.` then
   `OnAuthSessionTicket`.
5. **Persona session.**
   - 2021 build: `CreateSteamSessionJob: Calling Persona to create steam session`,
     then a persona id of form `amzn1.developerPersonaId.<uuid>`, then
     `Calling Http API: https://<gateway-cloudfront>/prod/credentials`.
   - 2025 build: `Omni metadata is present for this channel, using OMNI
     Authentication`, `Initializing OmniSDK 1.6`, then
     `https://<gateway-cloudfront>/prod/credentials/omni`.
   - OmniSDK token request (Community RE,
     [omnisdk-notes.md](https://github.com/nw-private-server/first-light/blob/main/docs/omnisdk-notes.md)):
     `POST https://tokenservice.amazongames.com/games/{game}/tokens` with JSON
     `platformIdentityType = "steam"`, `platformAuth.ticket = <Steam ticket hex>`,
     optional `fallbackToken` (JWT). Response fields include `fallbackToken`,
     `limitedUseToken`, `platformAccount`, `suspension`, `isNewAccount`. Error strings
     include `PLAYER_IS_BANNED`, `INVALID_PLATFORM_AUTH`, `UNSUPPORTED_REGION`. The stub
     also serves `/.well-known/openid-configuration` and `jwks.json`, so the client
     probably checks JWT signatures through JWKS (unconfirmed).
6. **AWS credentials.** `[AuthStackCredentialsProvider] Successfully refreshed gateway
   AWS credentials` and `persona AWS credentials`, refresh about every 55 minutes. The
   community stub returns STS-shaped values (`accessKeyId` `ASIA...`,
   `secretAccessKey`, `sessionToken`, `expiration`) because the AWS C++ SDK checks the
   shape before it signs requests (Community RE,
   [auth_mock.py](https://github.com/nw-private-server/first-light/blob/main/server/auth_mock.py)).
   Whether the stub must produce valid SigV4 verification is **Unknown**; the stub
   does not verify signatures.
7. **Login complete.** `CGame::OnCampfireLoginComplete, login successful.`
   ("Campfire" is an internal name; meaning **Unknown**.) Then
   `[GameConnection] ConfigureLogin MODE_GATEWAY: endpoint=<cf>, httpEntpoint=https://<cf>/prod, region=<region>`.
8. **Entitlements.** `GET/POST /players/{personaId}/games/new-world/platforms/steam/entitlements[/sync]`
   on `client.entitlementservice.amazongames.com` (Community RE).
9. **Login info / world list / characters.** `GET /prod/game/getlogininfo/jwt/omni?channelId=...&includeNames=true`
   returns worlds and characters (Community RE). MOTD world data from
   `d1hkbwzm1bktgo.cloudfront.net/motd/worlds_*.json`.
10. **Character select and create.**
    `POST /prod/game/worlds/{worldId}/characters/validator/jwt/omni` and
    `POST /prod/game/worlds/{worldId}/characters/jwt/omni` (Community RE).
11. **Login queue.** States `Disconnected -> QueryGameUpdateCheck -> QueueGameLogin ->
    WaitingForQueuedLogin`. Paths `/prod/game/login/queue/v2/...` and legacy
    `/prod/users/login_queue` (Community RE). Request JSON top key
    `LoginQueueRequest` (`CharacterId`, `WorldId`, `SteamAppId`). Response top key
    `LoginQueueResponse` with PascalCase fields: `TicketId`, `RepAddress`
    (`host:port`), `CharacterId`, `PersonaId`, `WorldId`, `TokenVersion`, `Signature`,
    `IsPermanentAppOwner`, `Location`, etc. Queue refresh form has `Position`,
    `QueueName`, `RefreshInterval`, `EstimatedTime`, `AllowQueueTransfer`,
    `RecommendedTransferWorldId`, and a nested `Token` object. A flat lower-case JSON
    gives the client error "No login ticket received" (Community RE). The same error
    text is seen by real players
    ([Steam discussion](https://steamcommunity.com/app/1063730/discussions/0/3803901365998643021/?ctp=2)).
    Launch-era queue errors: "Failed to join login queue"
    ([GameRevolution](https://www.gamerevolution.com/guides/695948-new-world-failed-to-join-login-queue-error-message-fix)).
12. **Login ticket.** Log: "Received login ticket", format `<worldId>_<uuid>`, world
    name like `live-2-02-1`.
13. **Per-world remote config** from S3 (2.1.2).
14. **REP connection.** `StartREPConnection -> WaitingForREPConnection`,
    "REP socket connection established, now waiting to register", "received
    registration response from REP", server version string
    `[RETAIL].Javelin.1.365.6030.5950962`.
15. **World entry.** `WaitingForActorGameConnection -> WaitingForSpawnPoint ->
    WaitingForPlayerSpawn -> InGame`. Map `NewWorld_VitaeEterna`. Vivox voice connects.
16. **Logout.** `EndSession`, `Disconnected from REP`.

Additional facts:

- The 2025 log shows two Steam tickets and two OmniSDK sessions against two regional
  gateways. Purpose **Unknown**
  ([connection-flow.md](https://github.com/nw-private-server/first-light/blob/main/docs/connection-flow.md)).
- Amazon Games iD: optional identity layer; linking moves save data to the Amazon
  Games iD account
  ([help article 36414421720091](https://help.newworld.com/hc/en-us/articles/36414421720091-Amazon-Games-iD),
  [help article 36413919166875](https://help.newworld.com/hc/en-us/articles/36413919166875-Link-or-Unlink-Accounts)).
- Known issue text: "Internal Error with Request ID" at login for latent-name
  characters. This shows HTTP-style request IDs in error paths
  ([Known Issues](https://help.newworld.com/hc/en-us/articles/36299773707931-Known-Issues)).
- Backend services named in the AWS blog: API Gateway (about 200 million calls per
  minute); Lambda microservices for achievements, channels, unique naming (global) and
  queues, contracts, sessions, guilds, characters (local)
  ([AWS for Games blog](https://aws.amazon.com/blogs/gametech/the-unique-architecture-behind-amazon-games-seamless-mmo-new-world)).
- The 2026 client also has a `/Javelin.RPC.*` path on HTTPS. Framing (HTTP/2, gRPC,
  protobuf) is **Unknown**
  ([NewWorldPreservation CLIENT_FLOW.md](https://github.com/AScriver/NewWorldPreservation/blob/main/docs/CLIENT_FLOW.md)).

### 2.3 Encryption, compression, packet framing

All items in this section are Community RE unless the source is Amazon or Lumberyard.

Layer stack (top to bottom), from
[first-light protocol-overview.md](https://github.com/nw-private-server/first-light/blob/main/docs/protocol-overview.md),
[post-v3-flow.md](https://github.com/nw-private-server/first-light/blob/main/docs/post-v3-flow.md)
and [Aeternum-World packet_system.md](https://github.com/Coldzer0/Aeternum-World/blob/main/docs/Network/packet_system.md):

1. **UDP.**
2. **DTLS 1.2.** Cipher `ECDHE-RSA-AES256-GCM-SHA384`. Server presents an RSA
   certificate. The client verifies it against a certificate embedded in the
   executable (see 2.5.2). Lumberyard's `SecureSocketDriver` has the same design:
   OpenSSL, DTLS, cookie exchange state, `m_maxDTLSConnectionsPerIP`
   ([SecureSocketDriver.h](https://github.com/aws/lumberyard/blob/master/dev/Code/Framework/GridMate/GridMate/Carrier/SecureSocketDriver.h)).
   DTLS plaintext cap 16,384 bytes limits record size.
3. **GridMate datagram header**, 4 bytes: `u8 mode` (`0x80` plain, `0x81` LZ4
   compressed), `u8 format` (`0x01`), `u16be dgram_seq`. In stock Lumberyard the
   header is a 1-byte compression hint (only when a compressor is set) plus a `u16`
   sequence number, big-endian (`kCarrierEndian = BigEndian`)
   ([Carrier.cpp](https://github.com/aws/lumberyard/blob/master/dev/Code/Framework/GridMate/GridMate/Carrier/Carrier.cpp)).
4. **Compression.** LZ4 raw block, no frame header, no size prefix
   ([compression_algorithm.md](https://github.com/nw-private-server/first-light/blob/main/analysis/compression_algorithm.md)).
5. **Carrier message records.** `u8 flags`, `u16be data_size`, optional channel,
   chunk count, sequence and reliable sequence, by flag bits: `RELIABLE 0x01`,
   `CHUNKS 0x04`, `SEQUENTIAL_ID 0x08`, `SEQUENTIAL_REL_ID 0x10`,
   `DATA_CHANNEL 0x20`, `CONNECTING 0x80`. Stock Lumberyard defines the same
   `MessageFlags` values.
6. **Channels.** 0 reliable events and RPC; 1 main replication (state bundles); 3
   system (connect, ACK, clock sync, bandwidth). Lumberyard: `k_maxNumberOfChannels =
   4`, `k_systemChannel = 3`.
7. **System messages** (channel 3, no envelope, id is the last payload byte):
   `SM_CONNECT_REQUEST = 1`, `SM_CONNECT_ACK = 2`, `SM_DISCONNECT = 3`,
   `SM_CLOCK_SYNC = 4`, `SM_CT_ACKS = 6`, `SM_CT_CONN_CONTROL`, `SM_CT_BANDWIDTH`
   ([Carrier.cpp `SystemMessageId`](https://github.com/aws/lumberyard/blob/master/dev/Code/Framework/GridMate/GridMate/Carrier/Carrier.cpp)).
   Captured connect request handshake data: `00000005`. Lumberyard's
   `DefaultHandshake` writes a version value and rejects a mismatch with
   `VERSION_MISMATCH`
   ([DefaultHandshake.cpp](https://github.com/aws/lumberyard/blob/master/dev/Code/Framework/GridMate/GridMate/Carrier/DefaultHandshake.cpp)).
   That `5` is the handshake version is unconfirmed.
8. **Application framing, asymmetric:**
   - Server to client: `[varint/VLQ32 length][message]`.
   - Client to server: `[u32be CRC32][u32be length][16-byte correlation field][message]`.
9. **AZ-Reflect envelope:** `u8 flags`, optional hashes, `u8 kind`, `varint
   type_idx`, optional inline 16-byte UUID. `type_idx` is a build-stable class index.
   `type_idx == 8` is a **state bundle**.
10. **State bundle:** header, then inner records `[varint interest_id][u8
    member_count]` and members `[varint replication_index][varint type_idx][body]`.
    A worked example is `PositionInTheWorldReplicatedState` in the Aeternum-World doc.

Registration (after DTLS and Carrier connect):

- Client sends `RegistrationRequestV3Msg` (about 0x470 / 832 bytes). It contains a
  session UUID, persona id and character display name. Server answers
  `RegistrationResponseMsg` (88-byte body). The response echoes the session token
  (32-hex form of the session UUID)
  ([post-v3-flow.md](https://github.com/nw-private-server/first-light/blob/main/docs/post-v3-flow.md)).
- After registration the server sends a "22-phase" sequence: time sync, self
  identification, level info, a large initial burst on channel 1 (about 285 KB),
  entity definitions, spawn. Keepalive `PingMsg` (type 0x15d) every 500 ms; without
  it the client disconnects after about 9 s
  ([post-v3-sequence.md](https://github.com/nw-private-server/first-light/blob/main/docs/post-v3-sequence.md)).
- Message catalog: 2,025 registered message types, 174 namespaces, for example
  `Javelin::ClientMessagesTrait`, `MB::*`, `ActorMover::*`, `Aoi::*`
  ([protocol-overview.md](https://github.com/nw-private-server/first-light/blob/main/docs/protocol-overview.md)).
  A UUID-to-class map exists in
  [Aeternum-World Catalog](https://github.com/Coldzer0/Aeternum-World/tree/main/Catalog).

Public captures and dissectors:

- Redacted login-to-world capture (message dump, not raw pcap):
  [first-light info/nw-login-safe-20260502-153840](https://github.com/nw-private-server/first-light/tree/main/info/nw-login-safe-20260502-153840).
- Wireshark text export with a "New World GridMate Carrier" dissector layer:
  [wireshark_carrier_handshake_redacted_session1.txt](https://github.com/nw-private-server/first-light/blob/main/info/wireshark_carrier_handshake_redacted_session1.txt).
  The dissector source (Lua or C) is **not** in the public tree. Its location is
  **Unknown**.
- Python decoders: `server/javelin/frame.py`, `bitstream.py`, codec modules in
  [first-light server/javelin](https://github.com/nw-private-server/first-light/tree/main/server/javelin).
- Capture tooling (runtime hook on the client, needs a client without active EAC):
  [Aeternum-World Tools/nw_capture](https://github.com/Coldzer0/Aeternum-World/tree/main/Tools/nw_capture).

### 2.4 Replication model: Lumberyard, O3DE and "Azoth"

Facts:

- Client log tags show the Lumberyard/CryEngine heritage: `(AZCore)`,
  `CryAnimation`, `NewWorld.exe [AZ_RETAIL_CONFIG]`, `CGame::InitCoatlicueTerrain`,
  version string `[RETAIL].Javelin.<branch>.<build>`
  ([pastebin 4ynb16Rb](https://pastebin.com/4ynb16Rb)). "Javelin" is the internal name
  of the game/network stack (Community RE).
- Asset files use AzCore `AZCS` compressed object streams (ZLib, compressor id
  `crc32("zlib")`) and Oodle in `.pak` (ZIP) archives
  ([new-world-tools reader/azcs/reader.go](https://github.com/new-world-tools/new-world-tools/blob/master/reader/azcs/reader.go),
  [pak/parser.go](https://github.com/new-world-tools/new-world-tools/blob/master/pak/parser.go)).
- Lumberyard became the base of O3DE, announced July 2021
  ([Wikipedia: Amazon Lumberyard](https://en.wikipedia.org/wiki/Amazon_Lumberyard)).
- Community binary scan: zero AzNetworking class markers, about 5,001 `Javelin::*`
  matches and 30 `GridMate` matches. Conclusion: Javelin descends from **GridMate**,
  not O3DE **AzNetworking** / **Multiplayer Gem**
  ([gridmate-reference.md](https://github.com/nw-private-server/first-light/blob/main/docs/gridmate-reference.md)).
  Transport names in the binary: `gridmate-udp`, `gridmate-udp-bsd`
  ([REP_TRUST_POLICY.md](https://github.com/AScriver/NewWorldPreservation/blob/main/docs/REP_TRUST_POLICY.md)).
- First-light expects the replica-creation message to be a GridMate `NewProxy`-style
  message; this is unconfirmed ("runtime-dependent step")
  ([first-light README](https://github.com/nw-private-server/first-light)).

For comparison only (O3DE, not used by New World according to the above):

- O3DE AzNetworking uses OpenSSL DTLS (`DtlsSocket`, `DtlsEndpoint`), a compression
  flag bit in the first header byte, and pluggable `ICompressor`
  ([O3DE encryption docs](https://docs.o3de.org/docs/user-guide/networking/encryption/),
  [O3DE AzNetworking](https://www.docs.o3de.org/docs/user-guide/networking/aznetworking/),
  [IPacketHeader](https://docs.o3de.org/docs/api/frameworks/aznetworking/class_az_networking_1_1_i_packet_header)).

"Azoth" engine:

- One low-quality site says New World uses a customized Lumberyard "known as Azoth"
  ([ora.it.com](https://ora.it.com/game-engine/what-engine-does-new-world-use)).
  This is **unconfirmed**. No primary Amazon source was found. The binary evidence
  above says "Javelin" for the game/network layer.

Server authority (official):

- Amazon (2021-11-01): the simulation is entirely server-based. The client sends
  controller inputs. The server checks them against limits and applies them. Physics
  and rules run on the server. The invulnerability bug occurred because in some cases
  the server waited for client input before it finished a calculation, combined with
  a weapon effect that gives short invulnerability
  ([MMORPG.com](https://www.mmorpg.com/news/new-world-apologizes-for-invulnerability-bug-explains-how-serverclient-authority-works-updated-2000123500),
  [PC Gamer headline](https://www.pcgamer.com/new-world-is-not-client-authoritative-amazon-says/)).

### 2.5 Easy Anti-Cheat and the connection

#### 2.5.1 What is public

- EAC client module initializes before Steam sign-in
  ([pastebin 4ynb16Rb](https://pastebin.com/4ynb16Rb)).
- Error "Cannot connect to the server. Exiting the game. [EAC Offline]". Fix: EAC
  "Repair Service" ([help article 36413457011355](https://help.newworld.com/hc/en-us/articles/36413457011355-Troubleshooting-EAC-Issues)).
- Error "Login Blocked: Suspicious Software or Corrupted Install Detected" is an EAC
  detection and block (same article).
- Amazon does not disclose detections made "while connected to a secured server"
  (same article).
- 2021: a Steam client update caused EAC failures
  ([Vulkk](https://vulkk.com/2021/10/08/new-world-steam-eac-error-potential-fixes-and-workarounds/),
  [Windows Central](https://windowscentral.com/how-fix-new-world-unable-launch-due-eac-connection-error)).
- EAC refuses a client with a changed file on disk: "Unrecognized game client" /
  "Unknown file version"
  ([NewWorldPreservation README](https://github.com/AScriver/NewWorldPreservation)).
- EAC blocks runtime instrumentation attach on the live build (Community RE,
  [first-light faq.md](https://github.com/nw-private-server/first-light/blob/main/docs/faq.md)).

#### 2.5.2 How EAC relates to the handshake (description only)

- "[EAC Offline]" shows that the client requires a working EAC session before or
  during server connection. Whether the game server or the backend checks EAC status
  per connection is **Unknown**. Whether EAC traffic is inside the REP stream or uses
  separate EAC service endpoints is **Unknown**.
- After 2027-01-31, the status of the EAC backend for this title is **Unknown**. If
  the client needs a live EAC backend to log in, an emulator must deal with this.
- REP trust is separate from EAC. The REP DTLS client loads an embedded certificate
  into a new OpenSSL store and uses `SSL_VERIFY_PEER`. No expected-hostname check was
  found statically. The Windows root store is not used for REP
  ([REP_TRUST_POLICY.md](https://github.com/AScriver/NewWorldPreservation/blob/main/docs/REP_TRUST_POLICY.md)).
  Six private attempts returned fatal `unknown_ca`. In-game error
  `@mm_csdkerr_transport_security_error (2)`
  ([first-light dtls-trust-bypass.md](https://github.com/nw-private-server/first-light/blob/main/docs/dtls-trust-bypass.md)).
- First-light reports a working runtime memory change for the trust check. This
  document does not repeat the method. NewWorldPreservation intentionally did not use
  it and recorded the problem as open.

### 2.6 Client failure modes that reveal protocol

| Event | What it shows | Source |
|---|---|---|
| 2021 chat HTML injection: images, large boxes, crashes, kicks through chat text | Chat text reaches other clients without escaping and is rendered by an HTML-capable UI. Server relays chat text as data. The UI middleware name is **Unknown** (Coherent was searched; no source). | [Inven Global](https://www.invenglobal.com/articles/15523/new-world-players-discover-potentially-game-breaking-code-injection-exploits), [Inven Global consultant](https://www.invenglobal.com/articles/15527/it-consultant-responds-new-world-code-injection-exploit), [Gfinity](https://www.gfinityesports.com/new-world/bug-kick-other-players-through-chat/) |
| 2021 window-drag invulnerability | Server sometimes waited for client input before it resolved damage. Stalling the client main loop stalled resolution. | [PC Gamer](https://pcgamer.com/new-world-invincibility-exploit), [GGRecon](https://www.ggrecon.com/articles/new-world-players-discovered-exploit-that-made-them-invincible), [MMORPG.com](https://www.mmorpg.com/news/new-world-apologizes-for-invulnerability-bug-explains-how-serverclient-authority-works-updated-2000123500) |
| 2021 gold duplication through server transfer (`Character_Persist_Failure`) | Sender and receiver persistence were not atomic. Character state is saved per character. | [PCGamesN](https://www.pcgamesn.com/new-world/gold-duping-bug), [Wccftech](https://wccftech.com/new-world-character-transfers-paused-as-amazon-promises-bans-to-players-using-an-exploit/amp/) |
| 2021 trade lag-switch duplication | Trade completion and reconnect were not atomic. Amazon disabled all wealth transfer. | [TechRaptor](https://techraptor.net/gaming/news/new-world-gold-dupe-exploit-new-world-trading-disabled), [NME](https://www.nme.com/news/new-world-gold-cheat-fix-causes-another-gold-cheat-3086260) |
| Login errors "No login ticket received", "Steam authentication Failure", "Failed to join login queue", "cannot connect to log in server" | Names client checks in the queue and ticket stage. | [Steam discussion](https://steamcommunity.com/app/1063730/discussions/0/3803901365998643021/?ctp=2), [WePC](https://www.wepc.com/gaming/new-world/unable-to-connect-to-log-in-server/) |
| Known issue "Internal Error with Request ID" | HTTP backend errors are shown with a request id. | [Known Issues](https://help.newworld.com/hc/en-us/articles/36299773707931-Known-Issues) |

### 2.7 Existing projects and tools

Emulator and protocol projects:

| Project | URL | Status (public) | Notes |
|---|---|---|---|
| New World: First Light | https://github.com/nw-private-server/first-light | Created 2026-04-17, last push 2026-05-16, README says DEFUNCT, moved to "OpenWorld" Discord. No license file. | Python HTTPS auth mock, DTLS REP responder, Javelin codecs, replay of a captured session. Reached V3 registration; stuck at client state 10 (`WaitingForREPConnection`). Live dashboard https://nw-private-server.github.io/first-light/ |
| NewWorldPreservation | https://github.com/AScriver/NewWorldPreservation | No license. | Research workspace. Stock client 1.400.6031.6004151 reached a private HTTPS server with a local CA and hosts redirect. REP DTLS fails with `unknown_ca`. Detailed evidence ledger. |
| Aeternum-World | https://github.com/Coldzer0/Aeternum-World | Created 2026-06-09, last push 2026-06-14. AGPL-3.0. | Packet layer documentation, type catalog, capture tools. Author also has https://github.com/Coldzer0/OodlePak and https://github.com/Coldzer0/LuaDecompiler |
| "Aeternum Legacy" | https://www.reddit.com/r/newworldgame/comments/1teenm8/beware_aeternum_legacy/ | Content **Unknown** (Reddit not fetchable here). First-light links it as a warning. | Treat any group with this name with care. |
| Ragezone request thread | https://forum.ragezone.com/threads/request-new-world-aeternum.1255035/ | Request only. | No server files. |

Statements that "no emulator exists" (for example
[mutop100](https://www.mutop100.com/news/new-world-servers-shutting-down)) are out of
date. Listing sites that advertise New World private servers
([mmotopforge](https://mmotopforge.com/new-world)) are unverified.

Data mining and file tools (no traffic parsing):

- https://github.com/new-world-tools/new-world-tools: pak extractor, AZCS object
  stream and datasheet converters (Go).
- https://github.com/giniedp/nw-buddy: companion app and data viewer; also
  https://github.com/giniedp/nw-buddy-data.
- https://github.com/imunproductive/NewWorldExplorer,
  https://github.com/PeterZhenhh/New-World-Pak-Extractor,
  https://github.com/Json-x-ly/NewWorldDatasheetDecoder.

Overlays and minimaps (screen OCR or map data, not network):

- https://github.com/CptWesley/NewWorldMinimap,
  https://github.com/TinusHeystek/new-world-minimap-release,
  https://github.com/Nezzquikk/NWMM-New-World-MiniMap.

Status scrapers (HTML of the official status page):

- https://github.com/nothub/newworld-servers,
  https://github.com/ArtiomSu/NewWorld-Server-Status-Telegram-Bot,
  https://github.com/BraXon-Devs/new_world_api.

Searches on UnknownCheats, OwnedCore and ElitePVPers for protocol discussion returned
no indexed public results with this toolset. Their content is **Unknown** here.

---

## 3. Implications for an emulator

### 3.1 Redirect the HTTPS control plane (works with an unmodified client)

Evidence: NewWorldPreservation reached its own HTTPS server from the stock client with
no memory or binary change. Requirements it found: a local CA in the current-user root
store, a leaf certificate with the correct hostname, and hosts-file redirects
([NewWorldPreservation README](https://github.com/AScriver/NewWorldPreservation)).

Plan:

1. Run a local HTTPS service on TCP 443 that routes by `Host` header.
2. Map every hostname in 2.1.2 to the emulator IP. Use the hosts file or a local DNS
   server (for example dnsmasq or Unbound with local overrides). A DNS server is
   better for many client PCs.
3. Issue leaf certificates for each hostname (or SAN list) from a private CA. Each
   player installs the CA once.
4. Serve the channel JSON first. Give one cluster that points all `apiEndpoint` values
   to emulator hostnames. Keep the client's fixed hostnames, because the channel host
   itself is fixed in the client.
5. Investigate `@assets@/ClientOverride.json`. If it can override channel or endpoint
   URLs, it can remove the need for hosts edits and some certificates. Format is
   **Unknown**.
6. Serve S3 remote-config keys from a local static store with the same key layout.
7. Stub content services (MOTD, news, catalog, entitlements, privacy settings) with
   minimal valid JSON.

### 3.2 Local authentication stub

- The client still uses Steam for a session ticket. Steam must run and own app 1063730.
- The emulator cannot verify Steam tickets with `ISteamUserAuth/AuthenticateUserTicket`
  without a publisher Web API key for this app (not available). Options: accept the
  ticket without verification and use a separate emulator account (password or
  invite), or map the SteamID from the ticket body without trust. Ticket body parsing
  is **Unknown** here.
- Implement the OmniSDK token service: issue JWTs signed by an emulator key; publish
  `/.well-known/openid-configuration` and `jwks.json`. First-light reports error 203
  (JSON parse failed) during development; strict JSON shape matters.
- Return STS-shaped AWS credentials. The client signs later gateway calls with them
  (SigV4). The emulator can ignore signatures or verify them with its own secret.
- Implement `getlogininfo`, character validator and create, and the login queue. Put
  the emulator REP address in `RepAddress`. Fill `Signature` with a non-empty value
  (Community RE says an empty value stops the hand-off).

### 3.3 Game data plane

- Listen on UDP. Accept DTLS 1.2 with `ECDHE-RSA-AES256-GCM-SHA384` (OpenSSL or
  another DTLS library with cookie exchange).
- Implement GridMate Carrier: datagram header, flags, channels, reliability, ACK
  vectors, chunking, LZ4. Use the archived Lumberyard source as a semantic reference.
  Note: its license is a custom "Amazon Lumberyard Archive" license, not MIT
  ([LICENSE.txt](https://github.com/aws/lumberyard/blob/master/LICENSE.txt)). First-light
  calls it MIT; that is contradicted. Write a clean implementation; do not copy code
  without a license review.
- Implement registration V3, the post-V3 sequence, keepalive, and then a real world
  simulation. The message schemas are build-specific (`type_idx` is assigned at
  startup). Pin one client build and generate the type table from it.
- The server must be authoritative (inputs in, state out), as in the official design.
- Persistence: AWS used DynamoDB; any database works for an emulator.

### 3.4 Hard blockers

1. **REP certificate trust.** The client trusts only the embedded certificate for
   REP. Amazon's private key is not public. Without a client change the emulator
   cannot complete DTLS. Any client change can conflict with EAC. This needs a project
   decision (for example: a separate launch path, an approved patch, or an EAC-free
   build) and a legal review. Details of any change are out of scope for this
   document.
2. **EAC backend after shutdown.** Status **Unknown**. Test the client offline
   behavior before 2027-01-31 if possible.
3. **Captures.** Live captures are only possible until 2027-01-31. Both community
   projects say captures of in-world traffic are the most valuable asset
   ([first-light docs/capture-guide.md](https://github.com/nw-private-server/first-light/blob/main/docs/capture-guide.md)).
   Capture of encrypted traffic gives only metadata. Plain-text capture needs
   instrumentation that EAC blocks on the live build.
4. **Server-side data.** World simulation data (loot, AI, quests) must come from
   client datasheets and slices (see new-world-tools). Server-only logic is **Unknown**.
5. **Legal and terms.** The EULA/ToS restrictions on emulation, and the effect of the
   shutdown on them, are **Unknown** here and need separate review.

---

## 4. Open questions

1. What does `@assets@/ClientOverride.json` accept? Is it still read in build
   1.400.x?
2. What is the purpose of ports 18887-18903 (EAC article) versus the 20000-59999 UDP
   outbound range?
3. How does the client pick the REP port? Is `rep-default-port` used when
   `RepAddress` has no port?
4. Does the client need a live EAC backend to reach "login successful" or to open REP?
   What happens after 2027-01-31?
5. Is there a supported or configurable way to change the REP trust anchor (for
   example a config key that supplies CA text)? Static analysis found none.
6. Does the client verify OmniSDK JWT signatures through JWKS, or only parse them?
7. Does the client verify anything in the AWS credential response besides shape?
8. Exact role of the five extra API Gateway hosts in the first-light hosts list.
9. Meaning of the Carrier handshake value `00000005` (version?) and the 16-byte
   correlation field in client-to-server frames.
10. Which message creates the player replica (state 13 to 14)?
11. Schema of `/Javelin.RPC.*` HTTPS calls.
12. Which UI middleware renders chat (relevant for safe chat handling)?
13. Is "Azoth" a real internal engine name? No primary source.
14. Location of the "New World GridMate Carrier" Wireshark dissector source.
15. Content of the "Beware Aeternum Legacy" Reddit thread.

---

## 5. Notes on evidence quality

- Amazon help center and AWS blog: official, but they do not describe the protocol.
- Client logs on pastebin: primary evidence of endpoints and state names; 2021 build.
- First-light and NewWorldPreservation: detailed but community-made, partly
  AI-assisted ("wakes", "agent handoff" files). Some statements conflict (TCP versus
  UDP; MIT license). Verify against your own client build before use.
- All binary addresses in community docs are valid only for the exact build hash they
  name.

---

## 6. Data not copied into this document

- Persona IDs, player names, character IDs, IAM role ARNs and AWS account numbers that
  appear in public logs were not copied.
- Trust-check patch locations and methods were not copied.

---

## 7. Source list

Official (Amazon / AWS):

- https://www.newworld.com/en-us/news/articles/the-future-of-new-world-aeternum-what-to-expect
- https://help.newworld.com/hc/en-us/articles/36341884420507-Port-Forwarding
- https://help.newworld.com/api/v2/help_center/en-us/articles/36341884420507.json
- https://help.newworld.com/hc/en-us/articles/36413457011355-Troubleshooting-EAC-Issues
- https://help.newworld.com/api/v2/help_center/en-us/articles/36413457011355.json
- https://help.newworld.com/hc/en-us/articles/36412584554523-Troubleshooting-Lag-and-Connectivity-Issues
- https://help.newworld.com/hc/en-us/articles/36341196232859-Supported-Regions
- https://help.newworld.com/hc/en-us/articles/36414421720091-Amazon-Games-iD
- https://help.newworld.com/hc/en-us/articles/36413919166875-Link-or-Unlink-Accounts
- https://help.newworld.com/hc/en-us/articles/36303863369499-Cross-Play-Features
- https://help.newworld.com/hc/en-us/articles/36303140415515-Server-Transfers
- https://help.newworld.com/hc/en-us/articles/36299773707931-Known-Issues
- https://www.newworld.com/en-us/support/server-status
- https://forums.newworld.com/t/public-api-planned/252130
- https://aws.amazon.com/blogs/gametech/the-unique-architecture-behind-amazon-games-seamless-mmo-new-world
- https://github.com/aws/lumberyard (archived; `dev/Code/Framework/GridMate/GridMate/Carrier/`)
- https://github.com/aws/lumberyard/blob/master/LICENSE.txt
- https://docs.o3de.org/docs/user-guide/networking/encryption/
- https://www.docs.o3de.org/docs/user-guide/networking/aznetworking/
- https://docs.o3de.org/docs/api/frameworks/aznetworking/class_az_networking_1_1_i_packet_header

Client logs:

- https://pastebin.com/4ynb16Rb (2021-10-09, build 1.9.232.1441593)
- Other public logs found but not analyzed: https://pastebin.com/muJwcS6Q,
  https://pastebin.com/djEWH3VD, https://pastebin.com/zx705xYd,
  https://pastebin.com/E3BYFuNK, https://pastebin.com/wTtexWJF,
  https://pastebin.com/mYWGqFte

Community projects:

- https://github.com/nw-private-server/first-light
  - docs/protocol-overview.md, docs/connection-flow.md, docs/post-v3-flow.md,
    docs/post-v3-sequence.md, docs/gridmate-reference.md, docs/omnisdk-notes.md,
    docs/dtls-trust-bypass.md, docs/faq.md, docs/capture-guide.md
  - analysis/compression_algorithm.md
  - server/auth_mock.py, tools/setup_hosts.py
  - info/wireshark_carrier_handshake_redacted_session1.txt
- https://github.com/AScriver/NewWorldPreservation
  - docs/REP_TRUST_POLICY.md, docs/CLIENT_FLOW.md
- https://github.com/Coldzer0/Aeternum-World
  - docs/Network/packet_system.md, Tools/nw_capture/README.md, Catalog/
- https://github.com/new-world-tools/new-world-tools
- https://github.com/giniedp/nw-buddy
- https://github.com/nothub/newworld-servers
- https://github.com/ArtiomSu/NewWorld-Server-Status-Telegram-Bot
- https://github.com/BraXon-Devs/new_world_api
- https://github.com/CptWesley/NewWorldMinimap

Press and forums:

- https://www.mmorpg.com/news/new-world-apologizes-for-invulnerability-bug-explains-how-serverclient-authority-works-updated-2000123500
- https://www.pcgamer.com/new-world-is-not-client-authoritative-amazon-says/
- https://pcgamer.com/new-world-invincibility-exploit
- https://www.ggrecon.com/articles/new-world-players-discovered-exploit-that-made-them-invincible
- https://www.invenglobal.com/articles/15523/new-world-players-discover-potentially-game-breaking-code-injection-exploits
- https://www.invenglobal.com/articles/15527/it-consultant-responds-new-world-code-injection-exploit
- https://www.gfinityesports.com/new-world/bug-kick-other-players-through-chat/
- https://www.pcgamesn.com/new-world/gold-duping-bug
- https://wccftech.com/new-world-character-transfers-paused-as-amazon-promises-bans-to-players-using-an-exploit/amp/
- https://techraptor.net/gaming/news/new-world-gold-dupe-exploit-new-world-trading-disabled
- https://www.nme.com/news/new-world-gold-cheat-fix-causes-another-gold-cheat-3086260
- https://steamcommunity.com/app/1063730/discussions/0/3803901365998643021/?ctp=2
- https://www.gamerevolution.com/guides/695948-new-world-failed-to-join-login-queue-error-message-fix
- https://www.wepc.com/gaming/new-world/unable-to-connect-to-log-in-server/
- https://vulkk.com/2021/10/08/new-world-steam-eac-error-potential-fixes-and-workarounds/
- https://windowscentral.com/how-fix-new-world-unable-launch-due-eac-connection-error
- https://portforward.com/new-world/
- https://en.wikipedia.org/wiki/Amazon_Lumberyard
- https://ora.it.com/game-engine/what-engine-does-new-world-use (weak source)
- https://www.reddit.com/r/newworldgame/comments/1teenm8/beware_aeternum_legacy/ (not read)
- https://forum.ragezone.com/threads/request-new-world-aeternum.1255035/
- https://www.mutop100.com/news/new-world-servers-shutting-down
- https://mmotopforge.com/new-world
