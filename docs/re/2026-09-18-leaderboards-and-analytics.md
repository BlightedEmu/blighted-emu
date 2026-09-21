# Leaderboards and analytics: what the client sends and expects

Date: 2026-09-18. Build: Steam `25810808`. Labels: **Observed**, **Inferred**, **Unknown**.

Sources: the 16 retail remote config replies of 2026-09-18
(`local-data/remote-config/retail-2026-09-18/`); the client decompile (`local-data/re-sweep`,
streamed with `zcat | grep`); string reads from `NewWorld.exe`; the retail HTTPS captures
`Hive_leaderboards` and `Check_Leaderboard` (`local-data/captures/open-world/`, EU and US
players opening the leaderboard screen; shapes and counts only, no ids or values kept here);
the content bundle tables `LeaderboardData`, `LeaderboardStatData` and `GameEventData`.

## 1. Retail config keys

**Observed.** publicGameplay `RegionId/iad-prod`:

| Key | Value |
|---|---|
| `omni.leaderboards.region` | `us-east-1` |
| `omni.leaderboards.server-endpoint` | `us-east-1.stats-service.amazongames.com` |
| `omni.leaderboards.client-endpoint` | `us-east-1.client.stats-service.amazongames.com` |
| `omni.leaderboards.ingestion-endpoint` | `us-east-1.stats-ingestion-service.amazongames.com` |
| `omni.leaderboards.stats-assume-role` | an Amazon IAM role (`AGSStatsARestService-...`) |
| `omni.leaderboards.ingestion-assume-role` | an Amazon IAM role (`AGSStatsIngestion-...`) |
| `javelin.leaderboard-rotation-startDate` | `2023-02-07` |
| `javelin.weekly-leaderboard-reset-hour` | `2` (the EU capture's layer has `8`) |

publicGameplay `CognitoId`: `javelin.leaderboard-season-end-datetime` `2027-01-31T00:00:00`.

public `RegionId/iad-prod`: `analytics.region-name` `us-east-1`,
`analytics.kinesis-stream-name` `regional-client-javelin-stream`, `analytics.kinesis-put-role`
and `analytics.overflow-kinesis-put-role` (an Amazon IAM role), `analytics.kinesis-region-name`
`us-east-1`, `analytics.overflow-kinesis-stream-name` empty, `analytics.event-types-to-disable`
`["set_seasons_tracked_stats_for_persona"]`.

**Observed.** No captured layer sets `javelin.enable-leaderboards`,
`javelin.enable-leaderboards-ui`, `javelin.enable-leaderboards-stat-queries`,
`javelin.query-leaderboards-service-from-client`, `javelin.enable-leaderboard-rotations`,
`javelin.use-season-end-time-config`, `javelin.leaderboard-season-rotation` or
`javelin.leaderboard-weeks-per-season`. The cvar reader `FUN_1407cf3b0` starts every bool at
`false` before the read. **Inferred.** Retail gets these values from a source that is not in the
16 layers (the captures show the client querying, so the switches are on there). **Unknown:**
that source. Our layers set them explicitly (section 7).

**Observed.** `analytics.kinesis-stream-name`, `analytics.kinesis-put-role` and
`analytics.kinesis-region-name` appear in no decompiled function. The reader `FUN_14618b430`
reads `analytics.enable`, `target` (default `kinesis`), `stream-name` (default
`regional-client`), `stream-suffix` (default `javelin-stream`), `stream-role` (default
`KinesisPutRole`), `stream-role-name`, `region-name`, `account-id`,
`stream-name-delimiter`, `event-types-to-disable`, `ring-buffer-size` and the overflow keys.
**Inferred.** The stream name is `<stream-name>-<stream-suffix>` and the role
`<stream-role-name>-<region>-<stream-role>`: both match the retail values.

## 2. The stats client

**Observed.** `FUN_141689690` reads `omni.leaderboards.client-mode`; `stubbed` builds the stub
`FUN_141628060`, any other value the real client `FUN_141627230`. That function builds up to two
`Aws::statsserviceapi::StatsserviceapiClient` objects (AWS SDK clients, signing service
`execute-api`, `SignatureV4`): one on `omni.leaderboards.ingestion-endpoint` when
`javelin.enable-leaderboards-stat-ingestion` is on, one on `omni.leaderboards.client-endpoint`
when `javelin.enable-leaderboards-stat-queries` is on. Both use `omni.leaderboards.region`,
a 10,000 ms request time limit, 3,000 ms connect time limit and 5 retries (arguments of
`FUN_1417230a0`). An assume-role credential provider (900 s) wraps them only when
`FUN_1436ca420` got one: that needs `omni.leaderboards.allow-dev-assume-role`.

**Observed (captures).** Neither capture has an STS call for the stats roles; the one STS
`AssumeRole` asks for the Kinesis role. **Inferred.** The stats client signs with the
credentials it already has (our `/steam` reply gives them; JSON shapes note section 3).

**Observed.** Operations of the query client (`FUN_147644180` group), path parts joined by `/`:

| Operation | Method | Path | Body or query |
|---|---|---|---|
| `GetLeaderboard` (`FUN_1476486b0`) | GET | `/<GameAlias>/<Namespace>/leaderboards/<LeaderboardId>` | `pageNumber`, `pageSize` |
| `GetLeaderboardForEntities` (`FUN_147649590`) | POST | same | `{"entityIds":[...]}` |
| `GetStats` (`FUN_14764cb80`) | POST | `/<GameAlias>/<Namespace>/stats` | `{"entityIds":[],"statIds":[...],"statScope":"..."}` |
| `GetProgressionEvents` (`FUN_14764a350`) | POST | `/<GameAlias>/<Namespace>/<StatScope>/<EntityId>/progression-events` | `{"progressionEventIds":[...]}` |
| `ResetEntityStats` (`FUN_14764d800`) | DELETE | `/<GameAlias>/<Namespace>/<StatScope>/<EntityId>/stats` | none |
| `SpingStats` (`FUN_14764ec40`) | | `/spingstats` | `NullSigner` |

The ingestion client (`FUN_14763e6f0`, service name `statsingestionserviceapi`) has one operation,
`SubmitStats` (`FUN_147640960`): `/<GameAlias>/<Namespace>/stats` with
`{"statsPayload":[{"entities":[{"id","scope"}],"matchId","statCollections":[{"dimensions":{},"id","statCollections":[...],"stats":{}}]}]}`
(`FUN_147640770`, `FUN_1476403f0`, `FUN_1476400d0`). **Inferred.** It is the game server's: the
caller `FUN_1417b7110` is under `LeaderboardComponentServerFacet::SendStatsToBroker`.

**Observed.** Reply readers: `GetLeaderboard` reads `leaderboardEntries` (each `entityId`
string, `rank` int, `value` double; `FUN_147646bc0`) and `leaderboardSize` int
(`FUN_147644eb0`). `GetStats` reads `lastUpdatedDate` and `stats` (each `entityId`, `statId`,
`value`; `FUN_147646f30`, `FUN_147645ae0`). Doubles are cJSON `valuedouble` (`+0x30`), ints
`valueint` (`+0x28`). Every key is checked with `ValueExists` (`FUN_147466d70`) before it is read.

## 3. Who asks: the client, not the game server

**Observed.** `FUN_1436e20f0` sends a leaderboard request through the game server
(`FUN_1436e21c0`, the message `LeaderboardComponentServerFacet_OnClientLeaderboardDataRequest`)
when `javelin.query-leaderboards-service-from-client` is off, and straight to the stats client
(`FUN_1436e23f0`) when it is on.

**Observed (captures).** Both retail captures query the stats service from the client:

| Capture | `GET .../leaderboards/` | `POST .../leaderboards/` | `POST .../stats` | Kinesis posts |
|---|---|---|---|---|
| `Hive_leaderboards` (EU) | 285 | 340 | 40 | 132 |
| `Check_Leaderboard` (US) | 75 | 48 | 15 | 104 (with STS) |

Every stats reply was `200`. Path game alias `new-world`, namespace `pc`, page `pageNumber=1`,
`pageSize=100`. **Inferred.** Retail runs with `javelin.query-leaderboards-service-from-client`
on; the game-server message path is not used by the leaderboard screen.

**Observed (captures).** Reply shapes:

- `GetLeaderboard`: `{"leaderboardEntries":[{"value":int,"entityId":"...","matchId":null,"rank":int}],"leaderboardSize":int,"isBackedUp":true|null}`.
  An empty board: `{"leaderboardEntries":[],"leaderboardSize":0,"isBackedUp":null}`.
- `GetLeaderboardForEntities`: one row per asked entity; an entity with no value gets
  `{"value":null,"entityId":"...","matchId":null,"rank":null}`.
- `GetStats` (scope `Global`, no entity): `{"stats":[{"value":int,"statId":"...","entityId":null}],"lastUpdatedDate":"YYYY-MM-DDTHH:MM:SS.mmmZ"}`.
- Entity ids: a character id (36-character UUID), a company id (`{UUID}` in upper case), or the
  member ids of a group joined by `_`.
- **Unknown:** the meaning of `isBackedUp`.

## 4. Board names

**Observed.** `LeaderboardData` (424 rows) gives each board a template in
`LeaderboardDefinitionId`, for example `top-war-score.{world}.{month}`; 105 start with `min-`
(clear times), 319 with `top-`. Placeholders: `{world}`, `{week}`, `{month}`, `{season}`, and in
two rows `{territory}` and `{influenceRaceId}`; `{faction}` in `FactionLeaderboardDefinitionId`.
`FUN_1436e2c30` fills `{world}` with the world id and `{season}`, `{week}` and `{month}` all with
the one rotation name its caller passes (from the Lua screen). `FUN_1436dcef0` fills
`{dungeon}`, `{territory}`, `{mutator}` and `{hotspot}`.

**Observed.** `FUN_1436d0db0` numbers the rotations (`javelin.enable-leaderboard-rotations`
must be on): from the start date at the reset hour (`FUN_1436d1300` reads it with `%d-%d-%d`),
the week is the number of whole weeks passed plus 1, the month is
`(year - start year) * 12 + month - start month + 1`, and the season is
`javelin.leaderboard-season-rotation` when `javelin.use-season-end-time-config` is on (else
counted in blocks of `javelin.leaderboard-weeks-per-season` weeks).

**Observed (captures).** On 2026-06-30 09:53 UTC the client asked for `w178` and `w177`, `m41`
and `m40`, `s11`, `s10` and `s1`. With start 2023-02-07 the formulas give week 178 and month 41.
**Inferred.** The names are `w<week>`, `m<month>`, `s<season>`; the screen also asks for the
previous rotation. **Unknown:** the weeks per season (retail season 11 fits 17 weeks, not
proven).

**Observed.** `LeaderboardStatData` (385 rows): `LeaderboardStatId`, `StatType`, `GameMode`,
`Scope` (`Character` 153, `Company` 66, `Group` 4, `Global` 162), `Aggregation` (`Sum` 229,
`Max` 154, `Min` 2), `Rotation`, `ServiceStatId`. 77 rows have a `LeaderboardData` row with the
same id. **Unknown:** how the retail service joins the other stat rows to boards; the
`ServiceStatId` names match the board names for most rows but not all (`war-wins` against
`top-character-war-wins`).

**Observed.** `GameEventData.LeaderboardValue` is set on 29 Darkness rows; no stat row with a
board counts them.

## 5. Analytics (Kinesis)

**Observed.** The client builds `Aws::Kinesis::KinesisClient` (`FUN_1474f2aa0`, signer service
`kinesis`); the endpoint comes from the region alone (`FUN_1475142c0`:
`kinesis.<region>.amazonaws.com`). No config key overrides the host. The operation is
`Kinesis_20131202.PutRecords` (`FUN_14750e7f0`).

**Observed (captures).** Before the first put, the client posts STS `AssumeRole` to
`sts.us-east-1.amazonaws.com` (form `Action`, `RoleArn` = the analytics role of its region,
`RoleSessionName`, `DurationSeconds=900`, `Version=2011-06-15`) and gets the XML
`AssumeRoleResponse` with `AccessKeyId`, `SecretAccessKey`, `SessionToken`, `Expiration`. Then
`POST /` to `kinesis.<region>.amazonaws.com` with body
`{"Records":[{"Data":"<base64>","PartitionKey":"<UUID>"}],"StreamName":"regional-client-javelin-stream"}`
and reply `{"FailedRecordCount":0,"Records":[{"SequenceNumber":"...","ShardId":"..."}]}`
(`application/x-amz-json-1.1`).

**Observed (captures).** Each `Data` is one JSON event with the keys `applicationName`,
`applicationVersion`, `attributes`, `eventPriority`, `graphName`, `id`, `metrics`,
`platformName`, `platformVersion`, `sourceTimeMillis`, `type`, `utcOffset`, `version`. Event
types in `Hive_leaderboards` (count): `leaderboard_view` 136,
`character_id_request_uuid_failed` 76, `connection_state_data` 59, `Chat_ChannelMute` 39,
`get_store` 28, `lua_script_error` 10, `store_on_catalog_ready` 8, `store_get_catalog_request` 7,
`get_catalog` 7, `loading_screen_instance` 6, `sync_entitlements` 5, and 14 more below 5.

## 6. What this server does

- **Stats service** (`crates/nw-login/src/stats.rs`, on `game.<domain>`, no session):
  `GetLeaderboard`, `GetLeaderboardForEntities` and `GetStats` with the retail shapes. A board
  whose id starts with `min-` ranks its lowest value first. Equal values share a rank (our
  choice; the retail rule is Unknown). Page size 1 to 100 and 100 ids per request are our
  limits. `GetProgressionEvents`, `ResetEntityStats` and `SpingStats` are not served: no capture
  calls them.
- **Storage** (`crates/nw-db`, global migration `0003`): `stat_value` (one value per namespace,
  stat or board id, and entity; merged by add, greatest or least) and `analytics_event`.
- **Stats from the sims** (`bins/sim/src/stats.rs`): a game event the sim fires
  (`fire_game_event`) counts 1 on every character board of a `LeaderboardStatData` row with
  stat type `GameEvent` and that event as its game mode (Inferred: these boards count
  completions; in build 25810808 only `HercyneRaid_Reward` has such boards). The board name
  uses the world id and the current rotation of `[leaderboards]` in `config/sim.toml`. The sim
  writes to the global database in a task; a refused batch is lost and counted
  (`sim_failed_stat_batches_total`), as a retry could count twice. The game server's own
  `SubmitStats` ingestion path is not needed: our sim is the game server.
- **Analytics** (`crates/nw-login/src/analytics.rs`): `edge-http` answers the hosts
  `kinesis.<region>.amazonaws.com` (`PutRecords`, up to 500 records, stored as JSON with their
  `type`) and `sts[.<region>].amazonaws.com` (`AssumeRole`, the same keys as the `/steam` reply;
  the server checks no signature).

## 7. Config layers our server must serve

publicGameplay `RegionId` layer:

```json
{
  "javelin": {
    "enable-leaderboards": true,
    "enable-leaderboards-ui": true,
    "enable-leaderboards-stat-queries": true,
    "query-leaderboards-service-from-client": true,
    "enable-leaderboard-rotations": true,
    "leaderboard-rotation-startDate": "2023-02-07",
    "weekly-leaderboard-reset-hour": 2,
    "use-season-end-time-config": true,
    "leaderboard-season-rotation": 11
  },
  "omni": {
    "leaderboards": {
      "region": "us-east-1",
      "client-endpoint": "game.<domain>"
    }
  }
}
```

The three rotation keys must equal `[leaderboards]` in `config/sim.toml`. No Amazon endpoint or
IAM role is served. public `RegionId` layer: `analytics.enable` `true`, `analytics.region-name`
`us-east-1`. For analytics, the client must resolve `kinesis.us-east-1.amazonaws.com` and
`sts.us-east-1.amazonaws.com` to `edge-http` (the same way as the remote config host); the
patched client checks no TLS certificate.

## 8. Unknown

1. The source of the retail values of the `javelin.*` leaderboard switches (section 1).
2. Whether the client accepts `host:port` in `omni.leaderboards.client-endpoint`.
3. How the retail service maps the stat rows without a board row, and the meaning of
   `isBackedUp`.
4. The retail ranking rule for equal values.
5. Whether the client starts its Kinesis client when `analytics.account-id` is empty.
