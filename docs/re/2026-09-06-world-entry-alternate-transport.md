# World entry: alternate transport study

Date: 2026-09-06.

## Scope

Read-only Ghidra inspection of the existing project:
`<re-scratch>/ghidra/NewWorld.gpr`.
The game was not run or changed.
No saved analysis changes were made.
The newer project was not opened because Cursor is analyzing the patch.

Old program SHA-256:
`8654f01d324636d9f74f1c793b0cc4a417c3c5fa9847d9913c358ca29e0fdc8e`.
The old notes identify this as Steam build 22469132.
The current Game.log reports client build 6024067 / Javelin 1.460.6033.
Do not apply old addresses to the patched executable.

The REA open_binary call could not open the zero-byte .gpr marker.
Analysis used installed Ghidra headless on the existing project with
`-readOnly -noanalysis` instead.
These are shell/Ghidra observations, not REA Evidence records.

## Live failure

Observed in local logs at 2026-09-06 19:05:24 UTC (15:05:24 America/New_York):

- Game.log line 3315: `@mm_csdkerr_transport_security_error (2)`.
- local-data/logs/live/gateway.log line 5: DTLS handshake failed,
  `tlsv1 alert unknown ca`, SSL alert number 48.

The game reached the gateway and sent a certificate trust rejection.
This establishes the immediate failure, not a complete proof of all later
world entry behavior.
No tokens or account details are copied into this note.

## Observed old-client paths

1. `FUN_146b6a270` reads
   `client-connection.transport-options.transport-type`.
2. Exact values `gridmate-udp` and `gridmate-udp-bsd` construct
   `FUN_146b34780`, passing the embedded certificate.
3. The other branch constructs `FUN_146b3de50` without that certificate
   argument. It logs `Initializing TransportLayerLibUV` from
   `FUN_146b3ee90`.
4. Its vtable is `0x14858d718`.
   Method `FUN_146b3e540` creates a session through `FUN_146b29d70`.
   That constructs `FUN_146b29ec0`, with session vtable `0x14858c588`.
5. Session vtable +0x30 points to `FUN_146b2d1c0`, the connect method.
   Session +0x68 points to `FUN_146b2d870`, the write processing method.
6. The write processing method calls `FUN_147945e80` on queued buffers.
   Its handle-type branch calls `FUN_147947e70`, which calls `WSASend`.
   The same session family has TCP bind/listen error strings next to its
   vtable, and a libUV worker loop.

Inference: this is a TCP transport alternative, not the existing GridMate
UDP carrier. A UDP packet relay alone is not sufficient for it.

Bounded observation: no embedded certificate parameter is passed to this
transport constructor. This does NOT establish that no later layer can
perform a security check. Deferred callbacks and the complete receive path
still need to be traced.

## Config input

Observed: `FUN_14645b520` queries the launch argument names `ConfigPath`
and `OverrideConfigPath` and replaces the corresponding path strings
before calling `FUN_14619f4d0`.
The default override is `@assets@/ClientOverride.json`.

The current Game.log line 535 reports that default override was not found.
The loader's source shows a path option; a live run has not shown that an
absolute loose file works for this transport setting.
The exact config tree must be checked against the current Client.json.
No launch arguments or client config were changed in this study.

External first-light archive notes report failed loose @assets@ override
attempts on an older client. Those are historical reports, not tests of
this project's patched client. They do not rule out an explicit absolute
OverrideConfigPath, but they also do not prove it works.

## Candidate bridge

Proposed, not implemented or validated:

```text
Game's built-in TCP transport
  -> loopback bridge (one TCP stream per player)
  -> emulator world message interface
  -> director and sim
```

The bridge would preserve ticket validation and use the existing world
routing. It must match TCP message framing, registration, and callbacks.
If the full TCP path has no certificate check, no game-binary patch would
be needed to avoid the DTLS trust path. A working config-selection method
is also required.

Do not claim this is a working certificate bypass.
Do not build a TCP-to-UDP byte copier: these paths have distinct transport
state and may have distinct message framing.

## Remaining checks

- Locate equivalent functions in the patched client by behavior and references.
- Trace deferred connect callbacks to their socket setup and completion.
- Trace receive framing and establish the message length/header rules.
- Check registration V3 and response framing on TCP.
- Check for security wrappers or other checks above TCP.
- Verify that OverrideConfigPath can select the transport without editing
  the executable or game archives.
- Test only against the local emulator after those checks.

## Work files

All new scripts and exports for this study are in /tmp:

- /tmp/NwTransportTrace.java
- /tmp/NwTransportDetail.java
- /tmp/NwSessionTrace.java
- /tmp/nw-transport-check-20260906/
- /tmp/nw-transport-trace-vtable/
- /tmp/nw-transport-detail/
- /tmp/nw-session-trace/
- /tmp/nw-transport-config-detail/

Those temporary files are not durable evidence storage.
The addresses and observations above preserve the useful findings.
The established work root for future RE tasks is
`<re-scratch>/`; notes belong in this repository's `docs/re/`.
