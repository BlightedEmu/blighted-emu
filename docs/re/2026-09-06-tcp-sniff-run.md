# How to sniff the client's first TCP bytes

Date: 2026-09-06.

Goal: learn the TCP transport body-boundary rule by reading the real
client's first bytes. The tool is `bins/nw-tcp-sniff`. It accepts TCP
connections, writes every received byte to a dump file, and logs a hex
preview. It never replies, so the client will send its opening frames and
then disconnect. The opening bytes are what we need.

## 1. Start the sniff listener

```bash
cargo run -p nw-tcp-sniff -- --listen 127.0.0.1:33435 --out-dir local-data/tcp-sniff
```

`33435` is the client `rep-default-port`. Dumps land in
`local-data/tcp-sniff/conn-0000-*.bin`.

## 2. Make the client pick the TCP transport

Set `client-connection.transport-options.transport-type` to any value that
is not `gridmate-udp` or `gridmate-udp-bsd`, for example `tcp`. The
selector then builds the no-certificate libUV TCP path (see
[tcp-transport-selectable](2026-09-06-tcp-transport-selectable.md)).

Put it in the loose override file `@assets@/ClientOverride.json` in the
client assets directory. Its shape mirrors `Client.json`:

```json
{ "client-connection": { "transport-options": { "transport-type": "tcp" } } }
```

If the retail build does not read the loose file (endpoint note, open item
2), pass the absolute path with the launch argument
`-OverrideConfigPath <abs-path>`. Confirm it loaded: `Game.log` prints a
"client config override" line, and later "Initializing TransportLayerLibUV".

## 3. Point the client's REP address at the sniff

The client opens the REP connection to the `RepAddress` string in its login
`Token`, not to the gateway endpoint. So the emulator login stack must hand
the client `RepAddress = 127.0.0.1:33435`:

- Set `rep_address` in `config/edge.toml` to `127.0.0.1:33435`.
- Run the normal login path (`edge-http` and `world-director`) so login
  returns that address.
- Log in, pick a character, and press Enter World. That triggers the REP
  TCP connection to the sniff.

## 4. Easy Anti-Cheat

The client must launch without Easy Anti-Cheat for a local run. This is not
yet set up and is the user's step.

## 5. Read the result

Open the newest dump in `local-data/tcp-sniff/`. The first bytes are the
client's opening carrier body on TCP. Compare them against the carrier
frame header in
[crates/nw-carrier/src/frame.rs](../../crates/nw-carrier/src/frame.rs):
`[flags][u16 BE length]...`. The bytes before the first recognizable frame
flags, or a length that covers the whole body, reveal the stream
body-boundary rule. Record that rule, then build the full TCP listener.
