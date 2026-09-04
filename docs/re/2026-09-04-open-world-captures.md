# Open World captures

Date: 2026-09-04.

This note records what the second set of capture ledgers adds. The user downloaded them from the Open World community file sharing. They are local only, in git-ignored `local-data/`. This note holds counts and protocol facts only. It holds no body bytes, names, ids, positions, tokens or addresses.

Labels: **Observed** means the captures showed the fact. **Inferred** means a conclusion from those facts. **Unknown** means the files do not show it.

## 1. The set

**Observed.** The source folder `local-data/open world captures/` holds web page exports, videos, and 413 archives: 208 zip, 155 rar, 35 7z and 15 zst. The archives unpack to `local-data/captures/open-world/<archive name>/`. They hold 410 ledgers (`ledger.bin`, the same record format as the first set). The unpacked size is 22 GiB.

**Observed.** 155 rar archives need `unrar`. `7z` on this system does not unpack their compression method.

**Observed.** Some archives also hold the capturing player's own DTLS key file (`dtls_key.txt`), login token service responses, and catalog service responses. These files are private data of other people. No tool of this project reads them. They must never be printed, copied or committed.

## 2. Capture check on all ledgers

**Observed.** `content-tool capture-check` over the first set (285 ledgers) and this set (410 ledgers), at commit `7997552`:

| Item | Count |
|---|---|
| Ledger files | 695 |
| Datagrams, server to client | 24,944,621 |
| Datagrams, client to server | 11,079,061 |
| Messages, server to client | 25,064,838 |
| Messages, client to server | 11,317,119 |
| Registration responses | 904, all status 0, all flags `01 00 00 01`, all 32-character tokens |
| New connections inside one capture context | 40 |
| Decode failures | 1 |

**Observed.** Two client builds answer registration: `[RETAIL].Javelin.1.365.6031.6012284` (516 responses) and `[RETAIL].Javelin.1.365.6031.6017213` (388 responses). The second build is newer than the first set.

**Observed.** Every M4 server body of both builds decodes and encodes to the same bytes with `nw-messages`: type 8 (23,127,104), 164 (2,053), 335 (128,009), 446 (1,009), 1034 (1,009), 1617 (955), 4246 (8,172), 4247 (8,934). 11,345 position members (type 13) and 23 whole bundle payloads also encode to the same bytes.

**Inferred.** The type indexes and the layouts of these messages are the same in both builds.

## 3. Several connections in one capture context

**Observed.** In 40 places, one capture context holds a second connection after the first one. The server sends a new connect ack 3 to 15 minutes after the first one. The datagram that holds it has sequence number 2, and the ack frame has reliable sequence number 0. These are the start numbers of a new connection (carrier captures note).

**Observed.** These contexts are in dungeon, raid, expedition and group run captures.

**Inferred.** A move into or out of an instance opens a new carrier connection, and the capture tool keeps the same context for it.

**Observed.** Before commit `f2da14d`, the capture check kept the old stream state across the new connection. That gave 8 false "truncated message" failures. The check now starts new streams for a context when the client sends a connect request after the server has delivered messages.

## 4. The one failure left

**Observed.** One stream in `starstone_barrows_group_run_2` stops with "more than 1,024 reliable frames wait for a missing frame". The stream has no restart at datagram 2 at that point and no forward jump of more than 1,100 in reliable sequence numbers.

**Inferred.** The capture lost one reliable frame, and its resend is not in the ledger either. The cause is in the recording, not in the protocol.

**Unknown.** Whether the capture tool dropped that datagram, or the network did and the resend fell outside the capture.

## Files read

- `local-data/open world captures/` (archive lists only)
- `local-data/captures/**/ledger.bin`
- `docs/re/2026-09-02-carrier-captures.md`
