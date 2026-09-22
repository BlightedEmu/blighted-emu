# TCP transport is config-selectable (no certificate)

Date: 2026-09-06.

Offline decompile of the user's own client. The game was not run and no
file was changed. Build 25810808 (`Bin64/NewWorld.exe`).

## Result

The transport selector reads
`client-connection.transport-options.transport-type` and branches on the
exact string value.

- Value `gridmate-udp` or `gridmate-udp-bsd`: builds the UDP path with the
  embedded certificate. The certificate PEM literal is passed here.
- Any other value: the else branch builds the plain-TCP libUV path with no
  certificate argument.

So the TCP path is reachable by setting `transport-type` to any value that
is not `gridmate-udp` or `gridmate-udp-bsd`. This can be set from
`@assets@/ClientOverride.json`, one of the `client-connection.*` keys.
The client already looks for that override file (Game.log search entry).

## Evidence

Decompiled `FUN_146b6a270` (old build; address map marks this function
body unchanged in build 25810808, new address `FUN_146b6b8b0`).

Simplified control flow:

```c
read "client-connection.transport-options.transport-type" -> s
if (memcmp(s,"gridmate-udp",...)==0 && len==12) goto udp_cert;
if (memcmp(s,"gridmate-udp-bsd",...)==0 && len==16) goto udp_cert;
// else:
obj = alloc(0x2c8); obj.vtable = PTR_FUN_14858beb0;
FUN_146b3de50(obj+2, self, 1);   // TransportLayerLibUV, no certificate
return;
udp_cert:
obj = alloc(0x4f0); obj.vtable = PTR_FUN_1485914d8;
FUN_146b34780(obj+2, self, <embedded PEM>, ...);  // UDP + certificate
```

`FUN_146b34780` (new `FUN_146b35dc0`) and `FUN_146b3de50`
(new `FUN_146b3f490`) are both marked body-unchanged in the address map.

## Send path finding (carrier frames over a stream)

Decompiled the TCP write path `FUN_146b2d870` and the stream writer
`FUN_147945e80` (dispatch by handle type; type 0xc calls `FUN_147947e70`
which calls `WSASend`). The write method gathers each queued message into
a `{ptr,len}` descriptor and stream-writes the array with one libUV write.
No extra per-write prefix is added at this layer.

Conclusion: the TCP transport carries the same GridMate carrier frames as
the UDP path (see [crates/nw-carrier/src/frame.rs](../../crates/nw-carrier/src/frame.rs)).
The DTLS layer is absent. Each written buffer is one serialized
carrier "datagram body".

The carrier frame is `[flags][u16 BE length][channel?][...][payload]`, where
the length is the payload length, and the implicit channel/sequence rules
reference "the previous frame in the same datagram body". Over UDP, the
datagram edge delimits a body. Over TCP there is no datagram edge, so the
stream must mark where one body ends. That body-boundary rule is the one
missing fact.

## Still open before a build can be correct

- The TCP stream body-boundary rule: how the receiver knows where one
  carrier body ends and the next begins (a length prefix, or a total-length
  field inside the body). This needs the libUV read/parse path, or a live
  capture of the client's first TCP bytes. No recording of this path exists
  because the live servers used `gridmate-udp`.
- Registration V3 framing and the response framing over TCP.
- Whether any later layer on the TCP path performs a security check. No
  certificate is handed to the constructor, so no trust anchor exists for
  one, but the receive path is not yet fully traced.

See [world-entry alternate transport](2026-09-06-world-entry-alternate-transport.md)
and the backup [memory-hook fallback](2026-09-06-memory-hook-fallback.md).
