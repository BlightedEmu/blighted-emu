# Exit to desktop: client patch

Date: 2026-09-10. Build: Steam `25810808` (`NewWorld.exe`), image base `0x140000000`.

Method: static only. Ghidra project `<re-scratch>/ghidra-25810808`, headless
`-readOnly -noanalysis`, script `DecompAddr.java`; dumps in `<re-scratch>/out-exitpatch/`.
`objdump` on the official exe for exact bytes. The game was not run for this note.

Labels: **Observed**, **Inferred**, **Unknown**.

## Why it hangs

**Observed.** The exit lambda `FUN_1410042a0`, destination 0 (desktop), logs
`Calling IAuthManager::GetInstance()->EndSession()` and calls auth manager slot `+0x130` with a
callback object (`docs/re/2026-09-10-network-warning-and-exit.md` section 4).

**Observed.** Steam auth manager: constructor `FUN_145a80510` stores vtable `0x14844BA48`.
Slot `+0x130` (`0x14844BB78`, file offset `0x844AB78`) holds `0x145A8D380`. That function moves
the callback into `this+0x30` (`FUN_14056d200`), calls the persona client at `this+0x228` slot
`+0x30` when it is set, and frees the now-empty argument. It never runs the callback.

**Observed.** Sonic vtable `0x14844B748`, slot `+0x130` (`0x14844B878`) holds `0x145A8D310`.
That function calls the callback (`(*(*cb + 8))(cb + 8)`), writes `1` to the byte at `this+0x58`,
then frees the callback.

**Observed.** The callback is `FUN_141005000`: it logs
`In callback function for IAuthManager::GetInstance()->EndSession()`, disconnects the actor
container, and runs the console command `quit` (old-build analysis in
`docs/re/2026-09-02-character-select-blockers.md` section 4; same code in this build per the
address map).

**Observed.** The 2026-09-10 17:37Z live `Game.log` stops at the `Calling` line. The persona
client is not present in our setup, so nothing finishes the Steam path.

## Is the Sonic function safe on a Steam object

**Observed.** The shared base constructor `FUN_145a7fa40` writes `this+0x30` (8 bytes),
`this+0x38`..`0x57` (two 16-byte stores), and `this+0x60` = `0xF`. It does not write
`this+0x58`. The Steam constructor `FUN_145a80510` does not write it either.

**Inferred.** The Steam object does not use byte `+0x58`, so the Sonic function's write there
is harmless. The write also comes after the callback has started the quit.

## Patch

Point Steam slot `+0x130` at the Sonic function: 8 bytes at file offset `0x844AB78`,
`80 d3 a8 45 01 00 00 00` -> `10 d3 a8 45 01 00 00 00`. **Observed:** a `DIR64` base relocation covers the slot, so
the new value is relocated like the old one.

`scripts/patch-client.py` finds the Sonic function by its whole body (no address inside), the
Steam function by its body with a wildcard for its one call offset, then the single pointer to
the Steam function. A dry run on the official exe found exactly the offsets above.

## Unknown

- "Exit to main menu" (destinations 1 and 2) logged nothing in the live run. This patch does
  not touch that path.
- No live test yet.
