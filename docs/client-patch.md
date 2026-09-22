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

## Third change: AWS proxy (Windows only, `--aws-proxy`)

This change is for Windows, where there is no `bwrap` sandbox to give the game
process a private hosts file and trust file. On Linux leave it off.

The game has three AWS clients that call fixed Amazon names with certificate
checks on: remote-config (S3), analytics (Kinesis) and analytics credentials
(STS). Each builds an `Aws::Client::ClientConfiguration` (constructor
`FUN_147467a40`) and uses the WinHTTP client (`FUN_147491570`; the factory
`FUN_146b73730` picks WinINet only when the configuration byte `+0x220` is `2`,
and the constructor sets it to `0`). With `--aws-proxy` the patch sends all
three through the launcher's loopback proxy instead
(`docs/re/2026-09-20-windows-launcher-network.md`).

After each builder's constructor call (the fresh config is in `RAX`), the patch
replaces the following `NOP` and one instruction with a jump to a code cave in
the `.text` INT3 padding. The cave sets, through `RAX`:

```text
proxyHost  +0xA0  = "127.0.0.1"   (MSVC std::string: inline buffer, size 9 at +0xB0)
proxyPort  +0xC0  = 47443         (AWS_PROXY_PORT; bins/launcher/src/aws_proxy.rs uses the same)
verifySSL  +0x1B8 = 0
```

then replays the instruction the jump replaced and jumps back. The remote-config
builder also writes `verifySSL = 1` itself a little later
(`MOV byte ptr [RSP+0x208],1`); the patch flips that immediate to `0` too. No
change alters the file length.

The three builders are `FUN_140853a80` (S3), `FUN_1461c3cc0` (Kinesis) and
`FUN_1461c5220` (STS). The tool finds each by the bytes around its constructor
call, so a build change that keeps the shape still matches; a build change that
breaks it makes the tool stop rather than patch the wrong place.

With `verifySSL` off, the WinHTTP request sets `WINHTTP_OPTION_SECURITY_FLAGS`
to `0x1100` (ignore unknown CA and CN mismatch; the date check stays on), so the
weakened check reaches only the launcher on loopback. The launcher proxy keeps
the real check: it opens its own TLS to the server and pins the server's mirror
certificate (`bins/launcher/src/aws_proxy.rs`, the same rule as `check_mirror`),
and it accepts a `CONNECT` only for the three mirror host kinds.

Run it with `--aws-proxy`:

```bash
scripts/patch-client.py "<Bin64>" --aws-proxy
scripts/patch-client.py "<Bin64>" --verify --aws-proxy
```

Reasoning, options and the safety analysis:
[docs/re/2026-09-20-windows-launcher-network.md](re/2026-09-20-windows-launcher-network.md).

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

For the AWS proxy patch, re-find the `ClientConfiguration` constructor
(`FUN_147467a40`: writes the two 5,000 ms timeouts and `verifySSL = 1` into a
fresh object) and the three builders that call it with the S3, Kinesis and STS
regions. Update `CONFIG_CONSTRUCTOR`, the three `AWS_CONFIG_SITES` and the S3
`S3_VERIFY_SSL_WRITE` in `scripts/patch-client.py`. If the struct offsets change,
read them from the WinHTTP client `FUN_147491570` (it reads `proxyHost +0xA0`,
`proxyPort +0xC0`) and update `CONFIG_PROXY_HOST`, `CONFIG_PROXY_PORT` and
`CONFIG_VERIFY_SSL`.

## Launching the patched client (no EAC)

The project `launcher` runs the patched exe directly under Proton. It needs
these environment variables (Linux/Proton):

```bash
NW_LAUNCHER_GAME_EXE="<Bin64>/BlightedNewWorld.exe"
NW_LAUNCHER_PROTON="<Steam>/compatibilitytools.d/<Proton>/proton"
NW_LAUNCHER_STEAM_COMPAT_DATA="<a folder of its own, for example <drive>/Blighted/prefix>"
NW_LAUNCHER_STEAM_CLIENT_INSTALL="<Steam install dir>"
```

The prefix must be the launcher's own folder. The launcher refuses Steam's
retail prefix (`<SteamLibrary>/steamapps/compatdata/1063730`), so retail play
through Steam keeps its prefix untouched. Proton makes a new prefix in an empty
folder on the first start.

The launcher logs in as usual and starts `BlightedNewWorld.exe` through Proton
inside `bwrap`, skipping `NewWorldLauncher.exe` and so Easy Anti-Cheat. `bwrap`
binds two private files for the game process only: a hosts file that leads the
server's `[aws_mirror]` names (remote-config bucket, Kinesis, STS) to the
server, and a trust file with the server's mirror certificate added
(`docs/re/2026-09-18-remote-config-layers.md`). Without the four variables the
launcher does not start the game: the Steam launch path ran the stock client in
the retail prefix, where it called Amazon's remote-config, analytics and STS
hosts.

## Launching the patched client on Windows

Windows has no `bwrap`, so the private hosts file and trust file are not used.
Instead the client is patched with `--aws-proxy` (above) and the launcher runs a
loopback proxy on `127.0.0.1:47443` (`bins/launcher/src/aws_proxy.rs`). The
patched client sends its three AWS calls there; the proxy checks the server's
mirror certificate and relays them to the server.

The launcher starts the patched exe directly (no Proton, no `bwrap`). It needs
only the game exe path:

```bat
set NW_LAUNCHER_GAME_EXE=<Bin64>\BlightedNewWorld.exe
```

Steam must be running for the Steam ticket, as on Linux. The launcher sets
`SteamAppId` and `SteamGameId` to `1063730` for the game process and refuses any
exe whose file name is not `BlightedNewWorld.exe`, so it never starts the retail
client.

### Windows live test

On a Windows PC with Steam and New World installed, and the project launcher
built (`cargo build -p launcher --release`):

1. Make the patched copy with the proxy patch, then check it:

   ```bat
   python scripts\patch-client.py "<Bin64>" --aws-proxy
   python scripts\patch-client.py "<Bin64>" --verify --aws-proxy
   ```

   Expect `patched copy: OK`. This writes `NewWorld.exe.orig` and
   `BlightedNewWorld.exe`; it never changes `NewWorld.exe`.
2. Start Steam and log in.
3. Set `NW_LAUNCHER_GAME_EXE` to the patched exe and start the launcher:

   ```bat
   set NW_LAUNCHER_GAME_EXE=<Bin64>\BlightedNewWorld.exe
   launcher.exe
   ```

4. Log in to the community server in the launcher and press play.
5. **What passes:** the game reaches character select with the server's
   remote-config layers (the same client-read values as on Linux: UI flags and
   feature toggles). Check the launcher has the proxy port `47443` open
   (`netstat -ano | findstr 47443` shows it listening on `127.0.0.1`).
6. **If it fails:**
   - The game asks the real Amazon bucket (the config layers are missing): the
     client was not patched with `--aws-proxy`, or the proxy did not start. Run
     `--verify --aws-proxy` and check port `47443` is listening.
   - The game cannot connect at all: Steam is not running, or the server address
     or mirror certificate is wrong (the launcher refuses a mirror certificate
     that is not a non-CA certificate valid for exactly the three names).
   - A TLS error toward the proxy: the `--aws-proxy` patch did not clear
     `verifySSL`; re-run the patch and `--verify --aws-proxy`.
   - The retail `NewWorld.exe` starts, or anti-cheat runs: `NW_LAUNCHER_GAME_EXE`
     does not point at `BlightedNewWorld.exe`.

## Files

- `scripts/patch-client.py` — makes/checks/reverts the patched copy (with
  `--aws-proxy` for Windows).
- `scripts/patch-client.test.sh` — tests the tool on a synthetic exe
  (`just test-scripts`).
- `bins/launcher/src/aws_proxy.rs` — the Windows loopback proxy.
- `bins/launcher/testdata/proxy-tls.crt` / `proxy-tls-key.pem` — the proxy's own
  game-facing certificate (the patched client does not check it).
- `bins/launcher/testdata/proxy-mirror.crt` / `proxy-mirror-key.pem` — a stand-in
  server mirror certificate for the proxy tests.
- `<Bin64>/NewWorld.exe` — official client, never modified.
- `<Bin64>/NewWorld.exe.orig` — one-time backup of the official client.
- `<Bin64>/BlightedNewWorld.exe` — the patched client our launcher runs.
