# Carrier facts from the local capture ledgers

Date: 2026-09-02
Build: Steam build `22469132` (client version text `1.365.6031`)
Method: A throwaway script read every local ledger and decoded every datagram, frame, and message header with the rules below. It counted values. It printed no payload bytes, names, ids, tokens, or addresses. The game was not run. No capture tool was run. Easy Anti-Cheat was not analyzed.

Labels: **Observed** = seen in the ledgers or in the client type table. **Inferred** = a conclusion from Observed facts. **Unknown** = not established.

Inputs (git-ignored, under `local-data/`):

- 285 `ledger.bin` files from the archives in `local-data/open world captures/`, unpacked into `local-data/captures/`. The files hold decrypted carrier datagrams from retail sessions. They hold personal data of other players, so they stay local.
- The client type table `typeindex.json` in the game folder (7,052 UUIDs; the list position is the type index).
- The Aeternum-World name map `Catalog/uuid_to_class.json` (AGPL-3.0, 3,486 entries).

Totals (`content-tool capture-check`): 7,211,869 datagrams (4,917,149 server to client, 2,294,720 client to server), 4,906,503 server envelopes, 2,317,475 client messages, 90 server message types, 102 client message types, 43 registrations. 0 decode failures with all rules below applied.

## 1. Ledger record

**Observed.** Every file is a list of records:

| Offset | Size | Field |
|---|---|---|
| 0 | 4 | `NWDL` |
| 4 | 8 | Time, Unix milliseconds, little-endian |
| 12 | 4 | Direction, little-endian: 0 = server to client, 1 = client to server |
| 16 | 8 | Connection context, little-endian. One file can hold more than one connection. |
| 24 | 4 | Datagram length, little-endian |
| 28 | n | One carrier datagram after DTLS decryption |

## 2. Datagram and frames

**Observed.** Datagram header: byte 0 is `0x80` or `0x81`, byte 1 is `0x01`, bytes 2-3 are a big-endian sequence number. With `0x81` the rest of the datagram is one LZ4 block. 99 % of datagrams in the small files use `0x81`.

**Observed.** Frames follow the header until the body ends. A frame is: flags (1 byte), payload length (u16 big-endian), channel (1 byte, when flag `0x20`), chunks remaining (u16 BE, when flag `0x04`), sequence (u16 BE, when flag `0x08` is clear), reliable sequence (u16 BE, when flag `0x10` is clear), then the payload.

| Flag | Meaning |
|---|---|
| `0x01` | Reliable |
| `0x04` | Chunk of a longer message; more chunks follow |
| `0x08` | Sequence not written (implicit) |
| `0x10` | Reliable sequence not written: previous frame's value + 1 |
| `0x20` | Channel written; otherwise the previous frame's channel |
| `0x80` | Connect request (client only) |

**Observed.** Flag values in the files: `0x20`, `0x21`, `0x25`, `0x08`, `0x18`, `0x19`, `0x01`, and on connect requests `0xA0` and `0x88`.

**Observed.** Channels: 0 = messages (registration, ping, most game messages); 1 = the large server stream and the client's frequent input message; 3 = system. Channel 2 is not used.

## 3. Reliable order and chunks

**Observed.** Packet loss is in the files: datagram sequence numbers jump, and resent frames arrive later, sometimes in reverse order inside one datagram. Two side-quest ledgers fail to decode when frames are joined in arrival order. All files decode when reliable frames are delivered in reliable-sequence order per connection, direction, and channel, and a frame older than the next expected one is dropped as a duplicate (at least 1,522 duplicates).

**Observed.** 43 connections start inside a ledger (connect request seen). In all of them the datagram sequence numbers start at 2 in both directions, and the reliable sequence numbers start at 0 on every channel in both directions.

**Observed.** A chunked message is a run of reliable frames with flag `0x04`, then one frame with `0x04` clear. The count field is the number of chunks left, this frame included: the first count is 2 or more, each next count is 1 less (57,746 steps), and the last frame (flag clear) follows the frame with count 2 (11,531 messages). No unreliable frame has flag `0x04`. The largest chunk payload is 1,115 bytes.

## 4. System channel

**Observed.** The opcode is the last payload byte.

| Opcode | Direction | Payload | Count (at least) |
|---|---|---|---|
| 1 connect | client | `00 00 00 05 01`, plus one `01` per retry; flags `0xA0` or `0x88` | 265 |
| 2 connect ack | server | `00 00 00 05 02`, flags `0x21` | 44 |
| 3 disconnect | both | `00 03` | 51 |
| 6 ack | both | `20 06` (no data), `00 <u16 last> 06`, `40 <u16 last> <u16 first> 06`, longer forms | 7,092,644 |
| 7 | both | 5 bytes `00 00 xx xx 07` | 196 |

**Inferred.** The value 5 in the connect payload is the carrier version. **Unknown.** The meaning of opcode 7 and of the longer ACK forms.

## 5. Messages

**Observed.** Client to server, one message per delivered payload: `[u32 BE check][u32 BE length][16 bytes][envelope]`. The length is the message size minus 8. The check is CRC-32 (ISO-HDLC, the zlib CRC) over every byte after the first 8. All 2,317,475 client messages match.

**Observed.** The 16 bytes are zero in registration and early messages. Later they hold one value that stays the same for the session. **Unknown.** Meaning.

**Observed.** Server to client: one or more `[prefix-coded length][envelope]` items per delivered payload.

**Observed.** Envelope: flags (1 byte), then 8 bytes when flag `0x01` is set, then 8 bytes when flag `0x02` is set, then a presence byte, then the type index (prefix code), then the body. Index 0 is followed by a 16-byte UUID. Flag values seen: `0x00` (all client messages, most server envelopes) and `0x03` (376 server envelopes). Flag `0x04` is not seen. **Unknown.** The layout of the `0x04` routing header.

## 6. Type names

**Observed.** The Aeternum-World map joins to `typeindex.json` by UUID for all 3,486 entries, and every `type_idx` value equals the list position. Names used here: 3 `REPClient::RegistrationResponseMsg`, 19 `RegistrationRequestV3Msg`, 8 `Amazon::Hub::ReplicatedStateBundle`, 9 `Amazon::Hub::ReplicationControl`, 209 `PingRequestMsg`, 335 `TimeSynchMsg`, 349 `REPClient::PingMsg`, 5867 `CrossWorldReconnectNotifierTrait`.

**Observed.** The protocol note constant "`PingMsg` = 5867" is wrong. `PingMsg` is 349.

## 7. Registration

**Observed.** V3 request: type 19, 2,711-byte envelope, reliable, channel 0, in chunks.

**Observed.** Group 4 of every V3 request (43) holds the login-queue `Token` fields in the order of the `Token` struct (protocol note section 3). Shapes, no values: `Signature` 512 hexadecimal characters; `RepAddress` `host:port`; `WorldId`, `CharacterId`, and `TicketId` 36-character UUID text; `PersonaId` `amzn1.` text; `SteamAppId` 1063730; `TokenVersion` 10; `ClientCapabilities` and `JwtClaims` empty; the three time values non-zero; `AccountIsLocked` present.

**Observed.** Response: type 3, reliable, channel 0, frame flags `0x21`. In all 43 responses:

| Field | Value |
|---|---|
| u32 | 0 |
| u64 | Server clock: nanoseconds since 2000-01-01 00:00:00 UTC (within 1.14 s of the record time) |
| string 1 | 32 hexadecimal characters |
| string 2 | `[RETAIL].Javelin.1.365.6031.6012284` |
| 4 bytes | `01 00 00 01` |

**Unknown.** The meaning of each of the four bytes.

## 8. Ping and time

**Observed.** The server sends `PingMsg` (349), reliable, channel 0, with an 8-byte body, about every 1,000 ms (71,342 of 78,064 gaps are 1,000-1,099 ms). The client sends a `PingMsg` with the same 8 bytes back within 50 ms. Both directions have 78,064 pings.

**Observed.** The server sends `TimeSynchMsg` (335), 8-byte body, every 2 s or more.
