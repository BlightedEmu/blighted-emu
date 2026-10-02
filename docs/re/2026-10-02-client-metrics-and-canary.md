# Client metrics and canary mode

Date: 2026-10-02. Build: Steam `25810808`. Labels: **Observed**, **Inferred**, **Unknown**.

Sources: `Client.json` from `assets/DataStrm.pak` (read with the local pak tool); the client
decompile (`local-data/re-sweep`, streamed with `zcat | grep`); the retail remote config replies
of 2026-09-18 (`local-data/remote-config/retail-2026-09-18/`).

## 1. Summary

- **Observed.** The client has a metrics reporter for a test-bot mode named "canary". It can
  send three metrics: the ping time, the number of connected players and a server health flag.
- **Observed.** In the shipped `Client.json` every metric is off, the canary is off, no output
  is set and both key fields are empty. No retail remote config layer of 2026-09-18 sets any
  `client-metrics` or `canary` key.
- **Inferred.** Retail players never send these metrics. The mode is for Amazon's own
  automated test clients.
- Our server does not need this to run. It is a source of ideas for the server status page
  (section 5).

## 2. `Client.json` keys

**Observed.** `client-metrics`:

| Key | Value |
|---|---|
| `metrics[0]` | `metric-name` `ping-time`, `enabled` `false`, `publish-period-seconds` `20`, `identifier` `testing` |
| `metrics[1]` | `metric-name` `num-connected-players`, `enabled` `false`, `publish-period-seconds` `20`, `identifier` `testing` |
| `metrics[2]` | `metric-name` `canary-server-status`, `enabled` `false`, `publish-period-seconds` `5`, `identifier` `testing` |
| `output` | empty |
| `aws-access-key-id`, `aws-secret-access-key` | empty |

**Observed.** `canary`:

| Key | Value |
|---|---|
| `enabled` | `false` |
| `connection-timeout.force-quit` | `true` |
| `connection-timeout.timeout-threshold-seconds` | `60` |
| `server-status.unhealthy-ping-threshold-milli` | `2000` |

## 3. Client code

| Function | What it does |
|---|---|
| `FUN_145a9ac50` | **Observed.** Reads `client-metrics`: the `metrics` list (`metric-name`, `enabled`, `publish-period-seconds` with default 60, `identifier`), `output`, `aws-access-key-id`, `aws-secret-access-key`. |
| `FUN_145a9aae0` | **Observed.** Reads `canary`: `enabled`, `connection-timeout` (`force-quit`, `timeout-threshold-seconds` with default 60) and `server-status.unhealthy-ping-threshold-milli` (default 3000 when the key is missing). |
| `FUN_145aa2800` | **Observed.** Maps `output` (lower case) to a sink: `cloudwatch` = 1, `pmet` = 2, `file` = 0, any other value = 3. |
| `FUN_145a990a0` | **Observed.** The `cloudwatch` sink: an HTTP query to `http://monitoring.us-west-2.amazonaws.com:80/` with `Action=PutMetricData`, `Version=2010-08-01`, signature version 2 (`HmacSHA256`), the key of the config, one value in `Milliseconds` and the dimension `Identifier`. |
| `FUN_145a99f10` | **Observed.** The `pmet` sink: `http://monitor-api.amazon.com:80/?Version=2007-07-07` with `Action=PutMetricDataForAggregation`, `Namespace=Amazon/Schema/Service`, `Unit=Count`, `ServiceName`, `MethodName`, `MetricName` and `Values`. The access key, a fixed signature and an expiry (2031) are built into the exe. The key is not copied into any note or repository. |
| `FUN_140221450`, `FUN_1402213c0` | **Observed.** Hold the names `canary-server-status` and `canary-lost-connection`. |
| `FUN_14644dbe0`, `FUN_14644e610`, `FUN_14646d9e0` | **Observed.** In canary mode the client logs "Canary failed to connect to server.. exiting..", "Canary lost connection to server.. exiting.." and "Canary's connection to the server timed out.. exiting..", and quits. |

**Inferred.** `canary-server-status` reports the server as unhealthy when the ping is over
`unhealthy-ping-threshold-milli` (2000 ms in `Client.json`).

## 4. What our server does

- Nothing. Our remote config layers set no `client-metrics` or `canary` key, so the client
  keeps the shipped values and sends no metrics.
- The launcher redirects only the remote config, Kinesis and STS hosts
  (`bins/launcher/src/mirror.rs`). The two metrics hosts are not redirected, and no client
  sends to them.

## 5. Use for the server status page

The status page is planned in `docs/progress.html`. The server has better numbers than the
canary metrics, so the page uses its own counters. The canary gives three checks to copy:

| Canary metric | Status page check |
|---|---|
| `ping-time` | The round trip time of each connected player (gateway) |
| `num-connected-players` | Players online, per world |
| `canary-server-status` | Healthy or unhealthy: unhealthy when the round trip time is over 2000 ms or a server part does not answer |

## 6. Unknown

- **Unknown.** Whether `ClientOverride.json` or a remote config layer can turn on
  `client-metrics` or `canary` (the readers have no direct caller in the decompile; they are
  called through a table).
- **Unknown.** What sends `num-connected-players`: a normal client does not know the player
  count of a world.
