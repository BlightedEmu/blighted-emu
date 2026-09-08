# Plan: static cert-verify bypass patch for world entry

Date: 2026-09-06. Build: Steam `25810808`, `Bin64/NewWorld.exe`
(SHA size 179,214,008 bytes). Offline analysis of the user's own client.

## Why

Everything up to world entry works: login, character list, character
select all run through the emulator. World entry fails: the client starts
the REP connection to `127.0.0.1:33435` over `gridmate-udp` DTLS and
rejects the gateway certificate with
`@mm_csdkerr_transport_security_error (2)`. The client trusts only its
embedded self-signed certificate and we do not have its private key.

The config route to avoid this (select the no-cert TCP transport) is dead:
the retail client reads no external `transport-type` override from any path
(see [memory-hook / config-failure note](2026-09-06-memory-hook-fallback.md)).

So the chosen path is to stop the client verifying the server certificate.
Then its normal `gridmate-udp` DTLS connection accepts the existing
`rep-gateway` self-signed certificate, and the whole working UDP/DTLS path
(M3 to M6) is reused unchanged.

## Live result (2026-09-06): the patch works; next wall is the actor connection

Applied as a separate patched copy `Bin64/BlightedNewWorld.exe` (official
`NewWorld.exe` kept untouched; `.orig` backup made). The launcher gained a
direct-Proton launch mode (env `NW_LAUNCHER_GAME_EXE`, `NW_LAUNCHER_PROTON`,
`NW_LAUNCHER_STEAM_COMPAT_DATA`, `NW_LAUNCHER_STEAM_CLIENT_INSTALL`) that
runs the patched exe without Steam's `NewWorldLauncher.exe`, so Easy
Anti-Cheat does not run.

With the real `rep-gateway` on UDP 33435 (self-signed cert), the patched
client got past the cert wall:

- Game.log: "REP socket connection established", "received registration
  response from REP", no `transport_security_error`.
- `gateway.log`: `registration accepted`.
- `world-director` and `sim`: `joined character=<id>`.

So the certificate bypass is proven end-to-end.

New blocker: the client then moves to `WaitingForActorGameConnection` and
loops `GameConnectionWrapper::OnLocalNetworkConnectionWarning` every ~3s; it
stays on the character-select screen with the Play spinner and never
spawns. There is one `[ERROR] 0xDDD1000` right after the registration
response. After about a minute the client gives up with a connection
timeout popup: "timed out while waiting for gameplay on the server ... check
your network settings / bandwidth" (a misleading message; the network is
fine). The server accepted registration and placed the character, but the
client does not get the post-registration "actor game connection"
world/state stream it needs. That is the next milestone: make the emulator
satisfy the real client's post-registration world handoff (the M4-M6
replication path, which until now was only exercised by `nw-testclient`,
not the real game).

## The patch

One function sets the OpenSSL verify state:

```c
// FUN_1478f1590 == SSL_CTX_set_verify(ctx, mode, callback)
void FUN_1478f1590(longlong ctx, undefined4 mode, undefined8 cb) {
    *(undefined4 *)(ctx + 0x158) = mode;   // ctx->verify_mode  = mode
    *(undefined8 *)(ctx + 0x188) = cb;      // ctx->verify_callback = cb
    return;
}
```

The gridmate DTLS context builder `FUN_145dcf890` (the function that sets
cipher `ECDHE-RSA-AES256-GCM-SHA384`) calls this with a non-zero mode
(`SSL_VERIFY_PEER`). A fresh `SSL_CTX` starts with `verify_mode = 0`
(`SSL_VERIFY_NONE`). So if the first instruction (the `verify_mode` write)
never runs, the mode stays 0 and the client accepts any server
certificate. With mode 0, OpenSSL does not call the verify callback, so the
second write is harmless.

Patch target (verified present in the installed exe):

- File offset: `0x78F0B90`
- Virtual address: `0x1478F1590` (image base `0x140000000`)
- Before: `89 91 58 01 00 00`  (`MOV dword ptr [RCX+0x158], EDX`)
- After:  `90 90 90 90 90 90`  (six `NOP`)
- Next bytes (unchanged): `4C 89 81 88 01 00 00 C3` (`MOV [RCX+0x188],R8; RET`)

Length-preserving, one site, six bytes.

## Scope and effect

This setter is shared. It is also called by the gRPC TLS builder
`FUN_147ba8db0` (control plane) and twice by `FUN_145dcf890`. After the
patch, no `SSL_CTX` in the client verifies the server certificate.

This is acceptable here:

- The control-plane (`edge-http`) already presents a certificate the client
  trusts; `verify_mode = 0` only makes the client more permissive, so login
  and character select keep working.
- The game-server path is exactly what we want to make permissive.

The patch weakens the client's own TLS verification. It is for connecting a
private local client to this emulator only.

## How to apply (next step, not yet done)

1. Copy `Bin64/NewWorld.exe` to `Bin64/NewWorld.exe.orig` (keep the
   original; never overwrite without a backup).
2. Write the six bytes `90 90 90 90 90 90` at file offset `0x78F0B90` of
   `NewWorld.exe`, only if the current bytes are `89 91 58 01 00 00`.
3. Re-check the bytes after writing.

## Launch and Easy Anti-Cheat

A patched `NewWorld.exe` fails Easy Anti-Cheat integrity checks. The game
must launch without EAC: run `Bin64/NewWorld.exe` directly with the login
arguments, bypassing `NewWorldLauncher.exe` (which starts EAC). The project
`launcher` currently starts the game through Steam's `NewWorldLauncher.exe`,
so an EAC-free launch path is a dependency to settle before the test. This
is an open item.

## How to test

1. Run the live stack with the real `rep-gateway` on UDP `33435` (not the
   byte-sniffer), presenting its self-signed certificate, same world id and
   signing key as `edge-http`.
2. Launch the patched client without EAC, log in, pick a character, enter
   world.
3. Expect: `Game.log` has no `transport_security_error`; `gateway.log`
   shows the DTLS handshake succeeding and a registration. The client loads
   into the world.

## Build specificity

Addresses are for build `25810808`. For another build, re-find the setter
by its body (`MOV [RCX+0x158],EDX; MOV [RCX+0x188],R8; RET`) or by callers
of the gridmate cipher string `ECDHE-RSA-AES256-GCM-SHA384`.
