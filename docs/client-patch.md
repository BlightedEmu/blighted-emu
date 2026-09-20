# Blighted client patch (BlightedNewWorld.exe)

This is the authoritative procedure for making our patched client. Keep it
current. The emulator cannot be entered by the stock client without it.

## What and why

The real New World client trusts only a certificate embedded in
`NewWorld.exe`, and we do not have its private key. So the client refuses
the emulator gateway's self-signed certificate and fails world entry with
`@mm_csdkerr_transport_security_error`. We cannot select the client's
no-certificate transport by config either (the retail client reads no
external override).

The fix is one tiny change to the client: stop it verifying the server
certificate. Then its normal `gridmate-udp` DTLS connection accepts the
emulator gateway, and the whole UDP/DTLS path is reused.

We never modify the official `NewWorld.exe`. We make a **separate patched
copy**, `BlightedNewWorld.exe`, next to it:

- Retail play through Steam uses the untouched `NewWorld.exe`.
- Our launcher runs `BlightedNewWorld.exe` directly (so Easy Anti-Cheat,
  started by `NewWorldLauncher.exe`, never runs).

## Exactly what is changed

One function, OpenSSL `SSL_CTX_set_verify(ctx, mode, callback)`:

```text
MOV dword ptr [RCX + 0x158], EDX    89 91 58 01 00 00   ; ctx->verify_mode  = mode
MOV qword ptr [RCX + 0x188], R8     4C 89 81 88 01 00 00 ; ctx->verify_callback = callback
RET                                 C3
```

The patch replaces the first instruction (the `verify_mode` write) with six
`NOP` bytes:

```text
before:  89 91 58 01 00 00
after:   90 90 90 90 90 90
```

A fresh `SSL_CTX` starts with `verify_mode = 0` (`SSL_VERIFY_NONE`). Not
writing the mode leaves it at 0, so the client verifies no certificate. With
mode 0 OpenSSL never calls the verify callback, so leaving the callback write
in place is harmless. The change is six bytes and does not change the file
length.

This setter is shared, so after the patch no TLS connection in the client
verifies the server certificate. That is acceptable: the control plane
(`edge-http`) presents a certificate the client already trusts, so making it
more permissive changes nothing there, and the game-server path is exactly
what we want to accept. The patch only weakens our own client for connecting
to our own server. **Restore the official exe before playing on any real or
official server.**

Build where this was derived: Steam build `25810808`, patch at file offset
`0x78F0B90`. Reasoning and the reverse-engineering trail:
[docs/re/2026-09-06-cert-bypass-patch-plan.md](re/2026-09-06-cert-bypass-patch-plan.md).

## Second change: Exit to desktop

Without it, "Exit to desktop" hangs the client. The exit path calls the Steam
auth manager's `EndSession`, which only stores the quit callback and asks the
Omni persona client to finish later. Our setup has no persona client, so the
callback never runs and the game never quits.

The Sonic auth manager's `EndSession` runs the callback at once (the callback
logs `In callback function for IAuthManager::GetInstance()->EndSession()` and
runs the console command `quit`). The patch points the Steam `EndSession`
vtable slot at the Sonic function: one 8-byte pointer in `.rdata`, in build
25810808 at file offset `0x844AB78`, `0x145A8D380` -> `0x145A8D310`. The Sonic
function also writes 1 to the byte at `this+0x58`; no Steam or base
constructor sets that byte, and the write comes after the quit has started.

The tool finds both functions by byte signatures (the Steam one with a
wildcard for its call offset) and then the single pointer to the Steam
function. Reasoning and addresses:
[docs/re/2026-09-10-exit-to-desktop-patch.md](re/2026-09-10-exit-to-desktop-patch.md).

"Exit to main menu" is a different path and is not changed by this patch.

## Third change: camera lock at mobs with mouse and keyboard

Without it, the middle mouse button never locks the camera on a mob. The lock
candidate check (build 25810808: `FUN_142c866e0`) refuses every target that is
not a player unless the input device is a gamepad or the config key
`javelin.enable-camera-lock-kbam-pve-targeting` is true. Retail gets that key
from remote StackConfig layers (S3 bucket `ags-javelin-remote-config`), which
need a signed-in player. The client reads no local file for it: its
override-config file is looked up only inside a mounted pak, and a `javelin`
object in it has no config component.

The patch replaces the `JZ` after the key's read with six `NOP` bytes, as if
the key were true. In build 25810808 it is at file offset `0x2C8629D` (VA
`0x142C86C9D`), `0F 84 5E 08 00 00`. The tool finds it by its signature: the
player-target test `CMP [RSI+0x81]`, `JNZ`, the gamepad test `CMP [RSI+0x88]`,
`JNZ`, `LEA RCX, key`, `CALL`, `TEST AL, AL`, `JZ`. No other function reads
the key. The PvP lock keys stay unpatched; their retail values are Unknown.

## How to make the patched copy

The tool finds the function by its byte signature, not by a fixed address,
so it keeps working across game updates. Run it with the game `Bin64`
directory:

```bash
scripts/patch-client.py "/path/to/SteamLibrary/steamapps/common/New World/Bin64"
```

It writes `NewWorld.exe.orig` (a one-time backup of the official exe) and
`BlightedNewWorld.exe` (the patched copy). It never changes `NewWorld.exe`.

Check an existing copy, or revert:

```bash
scripts/patch-client.py "<Bin64>" --verify   # reports OK / WRONG, writes nothing
scripts/patch-client.py "<Bin64>" --revert   # deletes BlightedNewWorld.exe only
```

## After a game update (the important case)

When Steam updates New World, `NewWorld.exe` is replaced and the patched copy
is stale. Re-make it:

```bash
scripts/patch-client.py "<Bin64>"
```

The tool reads the new official exe, finds the signature again (new address is
fine), writes a fresh `BlightedNewWorld.exe`, and refreshes the `.orig`
backup. If the update changed the function so the signature is gone, the tool
stops with an error instead of patching the wrong place — then re-derive the
signature (below).

## Re-deriving the signature if a build changes it

If `patch-client.py` reports "patch signature not found", the client build
changed the function. Re-find it with Ghidra (use `MAXMEM=24G` and
`-max-cpu 28`):

1. Find `SSL_CTX_set_verify`: a two-line function that writes its 2nd argument
   to `ctx+0x158` and its 3rd to `ctx+0x188`, then returns. (In build
   25810808 it is `FUN_1478f1590`.)
2. Confirm it is the game path by its caller: the gridmate DTLS context
   builder that sets the cipher string `ECDHE-RSA-AES256-GCM-SHA384` (build
   25810808: `FUN_145dcf890`).
3. Read the bytes of the `verify_mode` write (the `MOV [ctx+0x158], mode`
   instruction) and the two that follow, and update `SIGNATURE` and
   `VERIFY_MODE_WRITE` in `scripts/patch-client.py`.

If the struct offsets `0x158` / `0x188` ever change, update them in the
signature too.

For the exit patch, re-find the two `EndSession` functions: the Steam auth
manager vtable is the one its constructor stores (build 25810808:
`FUN_145a80510` stores `0x14844BA48`), slot `+0x130`; the Sonic vtable's slot
`+0x130` is the function that calls the callback first. Update
`SONIC_END_SESSION` and `STEAM_END_SESSION` in `scripts/patch-client.py`.

## Launching the patched client (no EAC)

The project `launcher` runs the patched exe directly under Proton when these
environment variables are set (Linux/Proton):

```bash
NW_LAUNCHER_GAME_EXE="<Bin64>/BlightedNewWorld.exe"
NW_LAUNCHER_PROTON="<Steam>/compatibilitytools.d/<Proton>/proton"
NW_LAUNCHER_STEAM_COMPAT_DATA="<SteamLibrary>/steamapps/compatdata/1063730"
NW_LAUNCHER_STEAM_CLIENT_INSTALL="<Steam install dir>"
```

With them set, the launcher logs in as usual but starts
`BlightedNewWorld.exe` through Proton, skipping `NewWorldLauncher.exe` and so
Easy Anti-Cheat. Without them it launches the stock client through Steam as
before.

## Files

- `scripts/patch-client.py` — makes/checks/reverts the patched copy.
- `scripts/patch-client.test.sh` — tests the tool on a synthetic exe
  (`just test-scripts`).
- `<Bin64>/NewWorld.exe` — official client, never modified.
- `<Bin64>/NewWorld.exe.orig` — one-time backup of the official client.
- `<Bin64>/BlightedNewWorld.exe` — the patched client our launcher runs.
