# Character select blockers

Date: 2026-09-02 (section 6 added after the live test the same day)
Build: Steam build `22469132`, `Bin64/NewWorld.exe`, image base `0x140000000`. Ghidra project `<re-scratch>/ghidra` (program `NewWorld.exe`).
Method: static analysis only. The existing project was opened read-only with analysis left off (`analyzeHeadless -process NewWorld.exe -noanalysis -readOnly`). REA `open_binary` was not called. The game was not run. Easy Anti-Cheat was not analyzed. Dumps are under `<re-scratch>/out-charselect/`. Shipped Lua, canvas, and localization files were read from the decoded copies in `q2-icon/`.

Labels: **Live test** = a fact from the running client, not from this decompile. **Observed** = an address, a string, a decompiler line, or a decoded script in this build. **Inferred** = a conclusion from that evidence. **Unknown** = not established.

**Live test.** The client reaches character select. The world line shows `0/2`. Two slots say `Create Character`, and each shows a `!` icon. A click on `Create Character` produces no log line and no HTTP request. `Exit to desktop` logs `[Debug] Calling IAuthManager::GetInstance()->EndSession()` and the process stays open. Right after login the log has `Using remote config: true`, StackConfig queries to the bucket `ags-javelin-remote-config`, then hundreds of `[ERROR] 0xCCC7000` lines.

## 1. Create Character click

Status: Resolved for the empty-list click. The request bus is not on that path.

**Observed.** The select screen is the canvas `LyShineUI/Main Menu/LandingScreenV2.uicanvas` (`0x14831c4e0`). `FUN_1441e3500` (`0x1441e3500`) loads it at `0x1441e3767` with no compare. The text `Create Character` is not in `NewWorld.exe` as ASCII or UTF-16. The slot label in localization is the key `@ui_createcharacter`.

**Observed.** The press handler is `OnPress` in `lyshineui/main menu/charactercreatebutton.luac` (lines 411–415). It does not test `CHARSTATUS_EMPTY_AND_BLOCKED`. If `createCharacterCallback` is a function, it calls that callback.

**Observed.** `SetEmptyCharacterSlot` in `lyshineui/main menu/landingscreenv2.luac` (lines 4039–4064) sets that callback to `OnCreateCharacterPressed` before the status check. `OnCreateCharacterPressed` (lines 2421–2443) returns when `#self.selectableWorldList` is 0. It shows the `@mm_serverunavailable` popup and does not call `HandleCreateCharacter`.

**Observed.** `HandleCreateCharacter` (lines 2445–2460) only opens a yes/no popup. A yes result calls `RequestGoToIntro`. Neither function calls `UiCharacterServiceRequestBus.CreateCharacter`. The only `UiCharacterServiceRequestBus` call in `landingscreenv2.luac` is `DeactivateCharacter`. `charactercreatebutton.luac` has no such call.

**Inferred.** On the live screen the list length is 0, so the click reaches `OnCreateCharacterPressed` and returns. That return sends no HTTP and writes no log line. The script does show the `@mm_serverunavailable` popup. The live note did not record a popup.

**Observed.** The native bus method still exists. `UiCharacterServiceRequestBus::CreateCharacter` is `FUN_144ba2a00` (`0x144ba2a00`). It calls `FUN_144ba2ac0` (`0x144ba2ac0`). The gate is:

```
144ba2af7  CMP qword ptr [RCX + 0x10], RDI
144ba2afb  JZ 0x144ba30a6
```

`RDI` is 0. A zero qword returns at `0x144ba30a6` and does not call `FUN_14642b130`, `FUN_1474ecf20`, or `FUN_1474ec2c0`. The same compare gates `DeactivateCharacter` (`0x144ba37e7`), `ValidateName` (`0x144ba31c7`), `SubmitCrestData` (`0x144ba2537`), and `PreValidateNameCheck` (`0x144ba1f77`).

**Observed.** The record is three qwords from `FUN_144af8690` (`0x144af8690`). It has no vtable. The only stores of `+0x10` are in that function: `0x144af8dae` stores the new bus object, and `0x144af8dcb` stores 0. The zero store runs when `FUN_1414671e0(context, 2, 0)` returns 0, or when `FUN_140ecc130` returns non-zero. `FUN_140ecc130` returns the byte at context `+8`. `FUN_1414671e0` returns 0 only when byte `+9` is set and the dword at `+0xe8` is not 0 and not 2.

**Observed.** The context is the behavior binder behind `LyShineGameManager`. `FUN_14054a0f0` (`0x14054a0f0`) builds that object and stores vtable `0x147f09af8`. It writes 0 at `+0xe8` (`0x14054a182`) and clears the qword at `+8` through `FUN_1412f3d40`. With those values `FUN_1414671e0` returns 1 and `FUN_140ecc130` returns 0, so the constructor does not force the zero store. Those fields do not come from GetLoginInfo, a world record, an entitlement, or StackConfig.

**Observed.** A second native gate loads a different canvas. `FUN_1441e3500` stores byte 0 at `[RBP+0x60]` (`0x1441e3d68`) and then reads `javelin.use-new-character-creation-flow` (`0x14831c6f8`) through `FUN_14033d610`. The compare at `0x1441e3d96` skips `LyShineUI/Main Menu/CharacterCreation.uicanvas` when that byte is not 0. `FUN_14616c670` passes byte 1 in `R8B` at `0x14616dea7` to virtual slot `+0x60` with that key. `LandingScreenV2` is the canvas that loads with no compare. The live screen is that canvas.

**Observed.** When the remote layers have no value for that key, the byte at `[RBP+0x60]` becomes 1. `FUN_14033d610` calls thunk `0x1405714a4` (`JMP [RAX+0x1F0]`). `FUN_140845950` stores vtable `0x147f4d700` at `0x1408459ac`. Slot `+0x1F0` is `FUN_140850100`. Slot `+0x60` is `FUN_140856420`. A non-zero `R8B` makes `FUN_140856420` store the text `true` (type byte 6) in the layer list. `FUN_140852900` skips a layer whose type byte is `0xe`. A remote layer that lacks the key is that miss. The walk then finds `true`. `FUN_14083fba0` turns `true` into byte 1, and `FUN_140850100` returns 1. That 1 overwrites the caller’s 0, so `CharacterCreation.uicanvas` is skipped. `LandingScreenV2` does not use this compare.

## 2. The `!` icon

Status: Resolved.

**Observed.** `SetEmptyCharacterSlot` calls `CurrentRegionContainsSelectableWorlds` (lines 5729–5731). That function returns false when `#self.selectableWorldList` is 0. A false result sets `CHARSTATUS_EMPTY_AND_BLOCKED` and passes the tooltip key to `SetWorldMessageTooltip`. A true result sets `CHARSTATUS_CANCREATE` and passes no tooltip, which hides the icon.

**Observed.** `SetWorldMessageTooltip` in `charactercreatebutton.luac` (lines 249–261) enables `WorldMessageHintIcon` when the tooltip is not nil and is not `""`. The canvas element is `CharacterMergeHint`. Its image is `lyshineui/images/landingscreen/iconexclamation.dds`.

**Observed.** The tooltip key is `@mm_noactiveworldsinregion_characterselectbutton_tooltip`. The English sentence in `localization/en-us/frontendcommontext.loc.xml` is `No active worlds in your region. Please try again later.` Flavor `SetItem` in `dynamictooltip_flavor.luac` looks the key up through `LyShineScriptBindRequestBus.Broadcast.LocalizeWithDataSheetData` and sets the text with `eUiTextSet_SetLocalized`.

**Observed.** A world stays off `selectableWorldList` unless it is not a transfer world, it is not a pool world, and `EntitlementRequestBus.Broadcast.IsEntitledForWorld` returns true.

**Observed.** The native handler is `FUN_142eb6eb0` (`0x142eb6eb0`). `FUN_142ea6a70` stores vtable `0x148157250` at object `+0x28` (`0x142ea6aca`). The Lua thunk `FUN_1405714d0` is `JMP [RAX+0x148]`. The qword at `0x148157250+0x148` is `0x142eb6eb0`. The address `0x148157278` has no references. Its slot `+0x148` is not this handler.

**Observed.** `FUN_142eb6eb0` takes the world id in `EDX` and one extra byte in `R8B`.

1. Each token of `javelin.worlds-that-bypass-entitlement` (`0x148157580`) is a CRC32 from `FUN_1412f4730` (A–Z made lower case). A hash equal to the world id returns 1.
2. A world row is loaded with type code `0x11`. If the qword at row `+0x18` is 0, there is no entitlement list. The return is the extra byte XOR 1. Byte 0 returns 1. Any other byte returns 0.
3. If the row has entries, each entitlement id is the dword at node `+0x10`. Slot `+0x130` (`FUN_142eb7080` on vtable `0x148157250`) tests that id in the bitset at `this+0xd0`. The first set bit returns 1. If every test fails, the function returns 0.

**Observed.** This function does not read `StackConfig`. The only config name in it is `javelin.worlds-that-bypass-entitlement`.

**Unknown. Static limit.** The bit index comes from vtable `+0x18` on the singleton at `0x14a335a58`. That global is 0 in the image. Its writers do not store a vtable immediate, so that lookup stays a run-time slot.

**Inferred.** The live `!` icons match an empty selectable list. A world dropped by `IsEntitledForWorld` is one way that list stays empty. The three Lua functions that set the icon do not read the remote-config download themselves.

## 3. `[ERROR] 0xCCC7000`

Status: Resolved.

**Observed.** The line is a fixed string at `0x14834e8f0`. The image has no integer `0xCCC7000` and no format `[ERROR] 0x%X`. `FUN_1444f6d00` (`0x1444f6d00`) loads the string at `0x1444f706f` and passes it to `FUN_1444f27a0`. That function calls `FUN_141022a90` with the format `%s` at `0x147f01b10`.

**Observed.** The log runs in state `1` when the `std::string` at `param+0x318` has size 0. The state stays `1`. The string comes from Steam auth vtable `0x14844a8b0` slot `+0x18` (thunk `0x145a95110`), which jumps to `FUN_145a8e970`. That function returns auth-backend slot `+0x50` from the object at `SteamAuth+0x228`, or the empty string at `DAT_149f2e1e8`. `FUN_145a8d1a0` prefixes a non-empty result with `Bearer `.

**Observed.** The log site has no backward branch. Each later call of tick thunk `0x1444fb630` (vtable `0x149f14fb8` slot `+0x18`) prints the line again. This path does not reference `StackConfig`, `Using remote config`, or `ags-javelin-remote-config`.

**Inferred.** The hundreds of lines are one empty bearer-token read, printed again on each tick while state stays `1`. The text `0xCCC7000` is not a decoded facility code.

## 4. EndSession on exit

Status: The wait is named. The call that would finish it is a static limit.

**Observed.** The log string `Calling IAuthManager::GetInstance()->EndSession()` is at `0x147fc8e08`, in `FUN_141004100` (`0x141004100`). `GetInstance` is `FUN_145a8dce0` (`0x145a8dce0`) and returns `DAT_14a3f8378`. The Steam constructor `FUN_145a7f3e0` stores vtable `0x14844a718`. Slot `+0x130` is `FUN_145a8c250` (`0x145a8c250`). The Sonic vtable `0x14844a418` has a different slot `+0x130`, `FUN_145a8c1e0`, which calls the quit callback immediately.

**Observed.** Steam `EndSession` copies the quit callback to the auth object at `+0x30` and returns. If the persona client at Steam `+0x228` is null, it posts no job and the quit callback is never called. If that pointer is set, it posts a thread-pool job. The job calls Omni `Session::EndSession` (`FUN_1479b4950`, `0x1479b4950`), which clears the SDK session, then queues `OnEndSessionComplete`.

**Observed.** The game closes only when persona slot `+0x38` (`FUN_145aa25d0`, `0x145aa25d0`) drains that queue. That path calls `FUN_145a95070`, which jumps to `FUN_141004e60` (`0x141004e60`). That function logs the callback line and then runs the console command `quit`. There is no call instruction to `0x145aa25d0`. The only selector is the vtable slot at `0x14844ab08`. A search from `0x145000000` to `0x147800000` found 144 loads of `[reg+0x228]`. None is followed within 50 instructions by `CALL [reg+0x38]`. The 32-bit form of that call does not occur in the image.

**Observed.** No HTTP host or route is on the path that must finish before `quit`. Omni disconnect has no host string. The only URL is inside the quit callback, after its log line: cvar `g_gotoDesktopSurvey`, key `@url_quitgamesurvey`, format `%s?Free=%s&TID=%s`. The host is the runtime value of that key.

**Observed.** `DAT_14a828e70` is only stored as zero. `FUN_14028eb10` stores 0 at `0x14028eb3e`. `FUN_1479b1680` stores 0 after disconnect and at SDK shutdown. No writer publishes an object, so disconnect slot `+0x18` has no function body.

**Observed.** This path does not read StackConfig or `ags-javelin-remote-config`. Steam versus Sonic is chosen earlier by `AuthManager` and `client-connection.authorization.mode`.

**Inferred.** The live log stops on the Calling line. That matches a null persona client, or a completion queue that nothing drains. The callback log is the first action in `FUN_141004e60`, so this run did not reach it.

**Unknown. Static limit.** The function that would call persona slot `+0x38` is not in the image under the searches above. The disconnect target at `DAT_14a828e70` is never an object in the static image.

## 5. Remote config from `ags-javelin-remote-config`

Status: The download, the bucket, and the keys that questions 1–4 read are named. The byte left for one missing bool is a static limit.

**Observed.** `FUN_1410376e0` (`0x1410376e0`) logs `Using remote config: %s`. The caller is `FUN_14102ee20` (`0x14102ee20`). `client-connection.enable-remoteCfg` defaults to true. The caller stores 1 before the read. A false value skips the bucket.

**Observed.** The bucket int is `*(object + 0x728) + 0xb8` (`FUN_141024ca0`):

| Int | Bucket |
|---|---|
| `3` | `ags-javelin-staging-remote-config` |
| `4` | `ags-javelin-remote-config` |
| other | `ags-javelin-dev-remote-config` |

**Live test.** `Using remote config: true` and bucket `ags-javelin-remote-config` match this function with the cvar true and the bucket int equal to 4.

**Observed.** `FUN_146422200` (`0x146422200`) builds `applications/%s/configuration-sets/%s/%s/%s`. The same format is in `FUN_14645d070` and `FUN_146468b30`. Those builders request 12 keys. Scope is `public` or `publicGameplay`. Dimension is `ProductId`, `RegionId`, or `CognitoId`. The last component is `versionless` or `<field +0xc8>;<field +0xe8>` from `FUN_146449a30`. An empty dimension value skips that key. `publicGM`, `private`, and `WorldId` records are initialized and then skipped by these loops. Host helper `FUN_1475f63f0` builds `s3.<region>.amazonaws.com`, or the literal `s3.amazonaws.com`.

**Observed.** A missing bool key keeps the byte the caller stored first (`FUN_14033d610`). Refresh failure (`FUN_141038670`, byte `+0x398`) queues `RemoteConfigRefreshFailedPopupEvent`, with `@channel_service_failed_title` and `@channel_service_failed_message`. State `5` in `FUN_14644a070` walks the class list and then enters state `7`. The remote-config functions do not reference the string `[ERROR] 0xCCC7000`.

**Observed.** Direct callees of `FUN_1410376e0` are `FUN_140fc6080`, `FUN_14083b380`, `FUN_1464456f0`, `FUN_1402b0e70`, `FUN_14143e010`, `FUN_14149ca40`, `FUN_141024ca0`, `FUN_140298e80`, and `FUN_140fc6400`. `FUN_146422200` calls `FUN_1402b0e70`. None of those callees reference `http`, `s3`, or `amazonaws`.

| Question | Reads this download? | What is read |
|---|---|---|
| 1 Create Character click on the empty list | No | The click returns in Lua from the list length. It does not read a remote key. |
| 1 Old creation canvas | Yes | `FUN_1441e3500` reads `javelin.use-new-character-creation-flow`. The caller stores 0. A missing remote value still returns 1 from the registrar text `true`, so `CharacterCreation.uicanvas` is skipped. `LandingScreenV2` does not use this compare. |
| 2 `!` icon Lua | No | `SetEmptyCharacterSlot` does not read a `javelin.` key. |
| 2 World kept on the list | Yes, one key | `IsEntitledForWorld` (`FUN_142eb6eb0`) reads `javelin.worlds-that-bypass-entitlement`. It does not read StackConfig. A false result drops the world. |
| 3 `0xCCC7000` | No | The string is in `FUN_1444f6d00`. |
| 4 EndSession | No | The EndSession functions reference the log strings only. |

**Observed.** Outside that table, `FUN_14103dc40` reads `javelin.use-omni-entitlements` (the caller default recorded there is 0). It is not the Lua slot `+0x148`.

**Observed.** `FUN_1464436a0` writes the bucket int at `+0xb8`. Int `3` selects the strings `https://dvenz6p69b1j0.cloudfront.net` and the app-id text `1426320` and `2069210`. Int `4` selects `https://d2c74t4zimux3r.cloudfront.net`. `FUN_1410376e0` still maps `3` to `ags-javelin-staging-remote-config` and `4` to `ags-javelin-remote-config`. The image has no enum symbol for these ints.

**Observed.** `FUN_14640b860` stores vtable `0x1484fb2f0` at `0x14640b8bc` (`MOV [RCX+0x28], RAX`). Slot `+0x168` is `FUN_146439520`. It returns the product id: `dev`, `STEAM_APP_ID.0000000.BRANCH.local-dev`, `STEAM_PID.` plus the app id, or the provider string. Slot `+0x158` is `FUN_146432c10`. It returns the string at `this+0x8a8`. `FUN_146466050` writes that field from `client-connection.authorization.cluster` and names it `cluster-id`.

**Observed.** `FUN_146449a30` returns the copy from `FUN_147207920`. The field at `+0xc8` is `6031`. The field at `+0xe8` is `6004151`.

**Unknown. Static limit.** The function that joins scheme, bucket, host, and key. The key leaves `FUN_146422200` and enters `FUN_1463e4550`. The handler is loaded by `MOV RCX, [RBX+0x28]` at `0x1463e4654`. The next call is the thunk at `0x1405717d8`: `MOV RAX, [RCX]` / `JMP [RAX+0x238]`. The 114 callers of `FUN_1406d17b0` and their 143 direct callees do not store a vtable immediate in that slot. On the bus path, `FUN_140854fa0` calls `FUN_140844e70`, and that store at `0x140844e97` receives `R9` after `XOR R9D, R9D` (`0x1408557d2` and `0x1408557ea`), so the stored value is 0. `FUN_140846460` and `FUN_140849e00` copy a runtime qword from a stack temporary (`LEA RDX, [RBP+0x110]` at `0x140855bb1`, and `LEA RDX, [RSP+0x70]` at `0x140855c5c`). The image does not give this handler a vtable. `FUN_1463e5e30` and `FUN_1463e6ad0` contain `https://` plus the host at object `+0xa0`. Their callers are signed login and character requests, not this StackConfig join.

**Observed.** The failure-popup button is `FUN_146442450` (`0x146442450`). `FUN_140fec280` stores vtable `0x147fc61f8` on the client and vtable `0x147fc6980` at `+0xbe0`. `FUN_14103db20` writes event `+0x30` as `this-0xbe0`. Slot `+0x68` of `0x147fc61f8` is `FUN_146442450`. It calls the callback at `[this+0x220]+0xd8` with argument `2` and clears that callback. It creates the environment object at `+0x728` when that pointer is empty, then calls `FUN_1464436a0`, which writes the bucket int and the CDN host. When `this+0x170` is already set, it returns. The body does not quit, open a URL, or only dismiss.

**Inferred.** A failed download does not print `0xCCC7000`, does not block `EndSession`, and does not by itself hide the `!` icon. It can change which worlds pass `IsEntitledForWorld` only through `javelin.worlds-that-bypass-entitlement`, and it can change whether the old `CharacterCreation.uicanvas` is loaded. The live screen is `LandingScreenV2`, which loads either way.

## 6. Live test after the world and queue fixes

Status: Resolved. The client reaches the REP connection.

**Live test.** Commit `c787527` adds `WorldType: "OpenWorld"`, `TransferToRegion: ""`, `WorldSet: "live"`, and `WorldVersion: "1.0.0"` to each world in `LoginInfoList.Worlds` and `ListWorldsResponseBody.Worlds`. `WorldMetrics` is still left out. With these fields the `!` icons are gone. A click on `Create Character` opens the world type picker (`Standard`), the intro, archetype, appearance, identity, and world pages. The client then sends the validator and `CreateCharacter` requests, and the server stores the character.

**Inferred.** `WorldType` is the field that fixed the empty `selectableWorldList`. The first-light archive (`local-data/first-light-main/server/auth_mock.py`) records that only the text `OpenWorld` maps to the open-world type. This build was not checked statically for that mapping. The four fields were added in one change, so the live test does not isolate one field.

**Live test.** The client logs `Queuing login for world () and character (<id>)`. The world id is empty right after character creation and also on `Play` with an existing character. Before commit `3101b42` the server answered 404, and the client showed `Errors::login_invalid_ticket` ("Invalid login ticket provided"). Commit `3101b42` searches the configured worlds for the account's character when `WorldId` is empty or absent.

**Live test.** After that fix the client logs `Received a login ticket`, loads the remote config classes, and then logs `start REP connection RepAddress = 127.0.0.1:33435, worldId = <configured world id>`. No REP server listens there (M3). After about 15 seconds the client logs `Errors::connection_rep_registration_timeout` and shows "Unable to connect to the server".

**Live test.** `Exit to desktop` still does not close the client (section 4).

## Evidence

- `<re-scratch>/out-charselect/q1-button/FINDINGS.md` and `pass5/`, `pass6/`
- `<re-scratch>/out-charselect/q2-icon/FINDINGS.md` and `native/FINDINGS.md`
- `<re-scratch>/out-charselect/q3-error/FINDINGS.md`
- `<re-scratch>/out-charselect/q4-endsession/findings.txt` and `pass7/findings.txt`
- `<re-scratch>/out-charselect/q5-remote/FINDINGS.md` and `pass2/`, `pass3/`, `pass4/`

## Second pass: selectable worlds

Date context: 2026-09-02. This pass is static. The game was not run. Easy Anti-Cheat was not opened.

**Observed.** The binary is `<game-dir>/Bin64/NewWorld.exe`. SHA-256 is `8654f01d324636d9f74f1c793b0cc4a417c3c5fa9847d9913c358ca29e0fdc8e`. Image base is `0x140000000`. REA `open_binary` with provider `ghidra` returned that identity. The tool result did not include an Evidence ID. `analyze_function` and `search_strings` then timed out. REA starts a new import. The existing project `<re-scratch>/ghidra` already contains this program. This pass opened that project read-only with analysis left off. The heap limit for that open was 16 GB. `open_binary` has no processor field and no memory field.

**Observed-from-user.** `GetLoginInfo` and `ListWorlds` return one world. `WorldId` is `8d0f4c2e-0000-4000-8000-000000000001`. `WorldName` is `Aeternum`. `WorldStatus` is `ACTIVE`. `PublicStatusCode` is `0`. `IsFull` is false. There is no `TransferToRegion` field. The character-select screen shows the world and `0/2` slots. `selectableWorldList` stays empty.

### 1. IsEntitledForWorld (FUN_142eb6eb0)

**Observed.** `FUN_142eb6eb0` is at `0x142eb6eb0`. File offset is `0x2eb64b0`. The world id arrives in `EDX` as an int. The function does not read the `WorldId` text. Lua builds that int first.

**Observed.** In `lyshineui/main menu/landingscreenv2.luac`, the fill at source lines 4777-4815 calls `Math.CreateCrc32` with `world.worldData.worldId`. That result is the first argument of `EntitlementRequestBus.Broadcast.IsEntitledForWorld`. The method name `CreateCrc32` is at `0x147fecce0` (file offset `0x7feb4e0`). The registrar site is `0x14142681b` in `FUN_141425a60`. That site passes the name to `FUN_141292540`. The next instructions do not store a code address for the method.

**Observed.** The bypass list uses `FUN_1412f4730` at `0x1412f4730` (file offset `0x12f3d30`). A null input stores `0`. Otherwise the start value is `0xffffffff`. For each byte, if `byte + 0xbf` is below `0x1a`, the byte is `A` through `Z` and the code adds `0x20`. The index is that byte XOR the low 8 bits of the running value. The new value is the running value shifted right 8 bits, XOR the dword in the table at `0x147fe6730` (file offset `0x7fe4f30`). The stored result is the bitwise NOT of that value. The table matches the reflected CRC-32 table for polynomial `0xedb88320`.

**Observed.** The live `WorldId` text is 36 ASCII bytes: `8d0f4c2e-0000-4000-8000-000000000001`. The text has hyphens. It has no braces. It has no `A` through `Z`. The `A` through `Z` step does not change these bytes. CRC-32 of these exact bytes is `0x9e80bc45`. CRC-32 of the same text in upper case is `0xb8ff35dc`. CRC-32 with the hyphens removed is `0x532c6323`.

**Observed.** `Math.CreateCrc32` is `FUN_1411b41e0` at `0x1411b41e0` (file offset `0x11b37e0`). `FUN_141292540` registers the name `CreateCrc32` and passes `FUN_1411b41e0` to `FUN_1412ebed0`. That helper stores the function at object `+0x16` and sets the return type name `Crc32`. The body matches `FUN_1412f4730`. A null string stores `0`. Otherwise the start value is `0xffffffff`. `A` through `Z` become lower case. The table is `0x147fe6730`. The result is the bitwise NOT. The function does not remove hyphens. The function does not remove braces. The 36 input bytes are `8d0f4c2e-0000-4000-8000-000000000001`. Those bytes have no `A` through `Z`. The result is `0x9e80bc45`. That int is the value Lua passes in `EDX`.

**Observed.** The world row load is the virtual call at vtable slot `+0x20`. The slot value on vtable `0x1483415c8` (file offset `0x833fdc8`) is `FUN_1443fa960` at `0x1443fa960` (file offset `0x43f9f60`). The call passes type code `0x11` and the world id int. `FUN_1443fa960` masks the type with `0xff` and uses it as an index. The hash bucket is at `this + 0x278 + index * 0x90`. The key compare is the dword at node `+0x10`. A hit returns `node + 0x18` as the row pointer in the decompiler (`puVar4 + 3`). A miss returns `this + 0x21c0`.

**Observed.** The object is `EntitlementDataManager`. RTTI text is `.?AVEntitlementDataManager@Javelin@@` at `0x14a17ffc8`. The constructor `FUN_1443f18a0` (`0x1443f18a0`, file offset `0x43f0ea0`) stores vtable `0x1483415c8` and builds `0x1b` tables of stride `0x90`. It registers the name `EntitlementData` (`0x148340540` and `0x148340490`). `FUN_1443fc1f0` loads that name through `FUN_1443ea120`. The shipped file whose header text is `EntitlementData` is `sharedassets/springboardentitites/datatables/javelindata_entitlements.datasheet` in `assets/SharedDataStrm-part7.pak`. Its columns include `UniqueTagID`, `EntitlementIndex`, `IsEnabled`, `DisplayName`, `RequiredEntitlements`, and `SourceType`. That file does not contain the live `WorldId` text.

**Observed.** `FUN_1443f18a0` stores `0` at byte offset `0x21d8`. That address is the miss sentinel `this + 0x21c0` plus `0x18`. `FUN_142eb6eb0` treats a zero qword at row `+0x18` as an empty entitlement list. The return is then the extra byte XOR `1`. Byte `0` returns `1`. Any other byte returns `0`.

**Observed.** The image hash tables are empty. A miss uses the sentinel, so the return is the extra byte XOR `1`. A loaded row with a zero count at `+0x18` does the same. A loaded row with entries returns `1` on the first owned entitlement bit, and `0` when every bit test fails.

**Observed.** All 2,250 decoded `.datasheet` files under the client extract were scanned by type name. One file has type `EntitlementData`: `sharedassets/springboardentitites/datatables/javelindata_entitlements.datasheet` in `assets/SharedDataStrm-part7.pak`. It has 3,342 rows and 15 columns: `UniqueTagID`, `EntitlementIndex`, `IsEnabled`, `DisplayName`, `Description`, `IsConsumable`, `Icon`, `RewardType`, `Reward(s)`, `EntitlementInfo`, `SourceType`, `AchievementId`, `Notes`, `ObjectiveId`, `RequiredEntitlements`. It has no world-id column. The live `WorldId` text is absent from that file. Names such as `ReturnToAeternum1` are entitlement labels.

**Observed.** The load method is `FUN_1443fc1f0` at `0x1443fc1f0` (file offset `0x43fb7f0`). It passes the type name `EntitlementData` at `0x148340540` to `FUN_1443ea120`. That call does not open a path string. The shipped member with that type name is `sharedassets/springboardentitites/datatables/javelindata_entitlements.datasheet` in `assets/SharedDataStrm-part7.pak`. That file has no world-id column. `FUN_1443ef640` leaves the hash at `this + 0x278 + 0x11 * 0x90` empty. `FUN_1443fa960` is the only instruction that uses that index formula. No store writes a world-id key into that hash. The type `0x11` row list is empty. The shipped file has no world-id keys, so the list cannot be built.

### 2. ProcessWorlds and PopulateWorldList

**Observed.** The symbol is Lua, not a native function. The file is `lyshineui/_common/worldlistcommon.luac` in `assets/DataStrm-part3.pak`. The module name in `landingscreenv2.luac` is `LyShineUI._Common.WorldListCommon`. The function name in the bytecode is `ProccessWorlds` (source lines 1725-1729). `PopulateWorldList` is a method on `lyshineui/worldlist/worldlist.luac` (source lines 485-517).

**Observed.** `ProccessWorlds` walks the table with `pairs` and calls the function it was given. It does not read world fields. The landing screen calls it on `selectableWorldList` after that list is built.

**Observed.** `PopulateWorldList` enables one server-list child per entry, up to the child count. It sets the child name from `worldData.worldId` and calls `SetWorldInfo`. When the list length is `0`, it calls `SetWorldSelectButtonEnabled(false)`. It does not test transfer, pool, status, or entitlement.

**Observed.** The list that becomes `selectableWorldList` is built in `landingscreenv2.luac` lines 4777-4815. `WorldVectorToTable` copies `status`, `type`, `publicStatusCode`, `publicName`, `worldId`, `worldSet`, `maxAccountCharacters`, `name`, `worldMetrics`, `version`, `transferToRegion`, `isFull`, and `isRecommended` from each login world. A world is inserted into `selectableWorldList` only when all of these pass:

1. `IsTransferWorld` is false. That test is `TestFlag(worldData.publicStatusCode, SERVERSTATUS_TRANSFERONLY)`. `SERVERSTATUS_TRANSFERONLY` is `32`. `TestFlag` is `((1073741824 + code) % (2 * flag)) >= flag`. Code `0` fails this test, so the world is not a transfer world.
2. `IsPoolWorld` is false. That test is `worldData.type == eWorldType_Unknown`. A nil world returns false. The global `eWorldType_Unknown` is not a numeric constant in this Lua file.
3. `IsEntitledForWorld` is true. The arguments are `Math.CreateCrc32(worldData.worldId)` and the return of `IsBattleForNewWorldWorld`.

A true `IsTransferWorld` or a true `IsPoolWorld` inserts the world into `regionTransferWorldList` instead. A false entitlement result inserts the world into neither list.

**Observed.** `IsBattleForNewWorldWorld` is `TestFlag(publicStatusCode, SERVERSTATUS_BATTLEFORNEWWORLD)`. That flag is `512`. Code `0` fails the test.

**Observed.** Other status bits in `lyshineui/_common/bitwisehelpers.luac` are: `SERVERSTATUS_AVAILABLE` `0`, `HIDDEN` `1`, `DISABLED` `2`, `DOWNFORMAINTENANCE` `4`, `CHARACTERCREATIONDISABLED` `8`, `NOCHARACTERTRANSFER` `16`, `FREEWORLD` `64`, `TEMPORARYMAINTENANCE` `128`, `FRESHSTARTWORLD` `256`, `NOOUTTRANSFER` `1024`, `FREETRANSFERAVAILABLE` `2048`, `SEASONAL` `4096`, `NOQUEUEFULLTRANSFER` `8192`. `IsWorldHidden` tests bit `2` (`DISABLED`), not bit `1`.

**Observed.** `GetProcessFunctionForCharacterCreation` returns the callback used by a later `ProccessWorlds` call. That callback sets `isSelectable`. It does not add or remove rows from `selectableWorldList`. A world stays `isSelectable == true` when all of these pass:

1. The cvar `g_createCharacterAccountLimitBypass` equals `1`, or `IsWorldAtCharacterLimit` is false. The limit test is `characterCount` against `worldData.maxAccountCharacters`.
2. `IsWorldCharacterCreationDisabled` is false. That flag is bit `8`.
3. `IsWorldFull` is false. That field is `worldData.isFull`.
4. `IsWorldInMaintenanceMode` is false. That flag is bit `4`.
5. `IsWorldStatusActive` is true. That test is `worldData.status == "ACTIVE"`.
6. `IsWorldDisabled` is false. That flag is bit `2`.

When `IsMaintenanceBypass` is true, the failing branches still store that true value into `isSelectable`.

**Observed-from-user and inferred.** The given world has `PublicStatusCode` `0`, `IsFull` false, and `WorldStatus` `ACTIVE`. Those values pass the transfer test, the battle-for-new-world test, the full test, and the active-status test. The pool test needs `type`. The live note for this question does not give `type`. If `type` equals `eWorldType_Unknown`, the fill puts the world on `regionTransferWorldList` and leaves `selectableWorldList` empty. If `type` is any other value, the fill keeps the world when `IsEntitledForWorld` returns true. A missing type-`0x11` row with extra byte `0` returns true. An empty selectable list then requires a loaded row whose entitlement tests all fail, or an extra byte that is not `0`. The screen can still show the world name and the `0/2` character slots from `worldList` and the character records. `PopulateWorldList` only draws `selectableWorldList`.

**Observed.** Section 6 records a later live test. The server field `WorldType` with the text `OpenWorld` clears the empty selectable list. This pass shows the Lua pool test on `worldData.type`. The static map from the JSON name `WorldType` to that Lua field is still open.

### 3. Extra byte in R8B for IsEntitledForWorld

**Observed.** The Lua call passes two values. The first is the CRC int. The second is the return of `IsBattleForNewWorldWorld(world)`.

**Observed.** The binder in `FUN_14513cb70` stores thunk `0x1405714d0` at `0x145143035` and sets `R8D` to `2` at `0x145143057` (file offset `0x5142657`). The name operand is `IsEntitledForWorld` at `0x1483e2110`. The thunk is `MOV RAX, [RCX]` / `JMP [RAX+0x148]`. On vtable `0x148157250`, slot `+0x148` is `FUN_142eb6eb0`. That function reads the extra byte from `R8B` (`param_3`).

**Inferred.** The reflection call with two arguments places the CRC in `EDX` and the battle-for-new-world result in `R8B`. For `PublicStatusCode` `0`, `IsBattleForNewWorldWorld` is false. A false Lua boolean that the binder stores as byte `0` makes a missing row return `1`. Thunk `0x1405714d0` is `MOV RAX, [RCX]` / `JMP [RAX+0x148]`. It does not store the byte. The store sits in the shared Lua-to-native argument move. That move has no world-specific source. One more pass cannot name a field.

**Observed.** `FUN_146589ce0` also calls thunk `0x1405714d0`, with the byte at `param+0x20`. That thunk is shared by many vtables. That call is not the character-select world path.

### 4. Launch argument for javelin.worlds-that-bypass-entitlement

**Observed.** `FUN_142eb6eb0` reads the key through `FUN_142e96610`. It does not read `argv`. Each returned string is one token. `FUN_1412f4730` hashes that token. A hash equal to the world id int returns `1`. The separator that splits the config value is not in `FUN_142eb6eb0`.

**Observed.** `FUN_14712c240` (`0x14712c240`, file offset `0x712b840`) walks command-line arguments. When the argument type from vtable slot `+0x18` equals `2`, it copies the argument text, appends one space byte `0x20` plus a second string when that string exists, logs `Executing command from command line:`, and calls console vtable slot `+0x120`. The references to this function are data references at `0x14ac42884` and `0x1485e3098`. No function in this pass assigns type `2` from a `+` or `-` prefix for this key. No instruction in this pass parses the literal `javelin.worlds-that-bypass-entitlement` from `argv`.

**Observed.** The key has two references. `FUN_14616c670` registers it at `0x1461781fe` (file offset `0x61777fe`). `FUN_142eb6eb0` reads it. No argv parser contains the key text.

**Observed.** The register call uses vtable `0x147f4d700` slot `+0x70`, which is `FUN_140856040` (`0x140856040`, file offset `0x855640`). The default list at that site is empty. `FUN_140856040` joins the list into one string and stores it with `FUN_1408589c0`.

**Observed.** The read uses thunk `0x140571584`, which jumps to vtable slot `+0x200`. On vtable `0x147f4d700` that slot is `FUN_140850080` (`0x140850080`, file offset `0x84f680`). It calls `FUN_140839d90` (`0x140839d90`, file offset `0x839390`). That function splits the stored text on the comma byte `0x2c`. It copies each slice as a token. It does not remove spaces. `FUN_1411b41e0` then folds `A` through `Z` and returns the CRC. A token is world-id text, not a CRC number.

**Observed.** `FUN_1463dc350` (`0x1463dc350`, file offset `0x63db950`) skips one leading `+` byte (`0x2b`) and compares the rest, without case, to a fixed name list. The first name is `r_Fullscreen` at `0x1483539d8`. This key is not in that function.

**Observed.** A type-2 launch argument cannot set this key. `FUN_147135730` stores the console at system `+0x100` (`0x14713636d`, file offset `0x713596d`). The console vtable is `0x1485e3720`. Slot `+0x120` is `FUN_14712c510`. That function calls `FUN_14712c830`. `FUN_14712c830` looks up the command text in the map at console `+0x178`. A nil node (`0x14712d0b8`) or the head node (`0x14712d0db`) logs `Unknown command: %s` at `0x1485ede20` and does not run the text. `FUN_140856040` registers the key in the config layer. It does not insert the key into this map. The same check rejects the key as console text.

**Observed.** The named config file is `user.cfg` at `0x1485e67a8`. `FUN_147135730` passes that name to system slot `+0x388` (`FUN_14713c280`). `FUN_147144040` reads the file. A line with no `=` byte takes the invalid-line path at `0x147144f63`. The text after `=` is the stored value. That value is comma-separated world-id text. Case of `A` through `Z` does not change the CRC. Hyphens stay in the token.

### Unresolved

1. **Unknown.** The integer value of the Lua global `eWorldType_Unknown`. The C++ helpers `FUN_144b1e940`, `FUN_144b1dda0`, and `FUN_144b1def0` pass the names `eWorldType_FTUE`, `eWorldType_OpenWorld`, and `eWorldType_Unknown` onward. They do not show the integer in those bodies.

### Evidence index

| Item | Address | File offset or path |
|---|---|---|
| Client binary | image base `0x140000000` | `<game-dir>/Bin64/NewWorld.exe` |
| `FUN_142eb6eb0` | `0x142eb6eb0` | `0x2eb64b0` |
| `FUN_1412f4730` and CRC table | `0x1412f4730`, `0x147fe6730` | `0x12f3d30`, `0x7fe4f30` |
| Bypass key text | `0x148157580` | `0x8155d80` |
| `CreateCrc32` body `FUN_1411b41e0` | `0x1411b41e0` | `0x11b37e0` |
| `CreateCrc32` text and registrar | `0x147fecce0`, `0x14142681b` | `0x7feb4e0` |
| Config list slot `+0x70` | `0x140856040` | `0x855640` |
| Config read slot `+0x200` | `0x140850080` | `0x84f680` |
| Comma split | `0x140839d90` | `0x839390` |
| Leading-`+` command compare | `0x1463dc350` | `0x63db950` |
| Key register site | `0x1461781fe` | `0x61777fe` |
| Row getter and vtable | `0x1443fa960`, `0x1483415c8` | `0x43f9f60`, `0x833fdc8` |
| Manager constructor | `0x1443f18a0` | `0x43f0ea0` |
| Argument-count site | `0x145143057` | `0x5142657` |
| Command executor | `0x14712c240` | `0x712b840` |
| `EntitlementData` text | `0x148340490` | `0x833ec90` |
| World list Lua |  | `assets/DataStrm-part3.pak` member `lyshineui/_common/worldlistcommon.luac` |
| Landing fill | lines 4777-4815 | `lyshineui/main menu/landingscreenv2.luac` in the same pak |
| `PopulateWorldList` | lines 485-517 | `lyshineui/worldlist/worldlist.luac` in the same pak |
| Status bits |  | `lyshineui/_common/bitwisehelpers.luac` in the same pak |
| Entitlement catalog |  | `assets/SharedDataStrm-part7.pak` member `sharedassets/springboardentitites/datatables/javelindata_entitlements.datasheet` |

REA `open_binary` did not return an Evidence ID. The decompiler text is from the existing Ghidra project, opened read-only.
