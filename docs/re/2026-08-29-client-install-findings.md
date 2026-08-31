# Client install findings

Date: 2026-08-29
Build: Steam build `22469132` (app 1063730)
Method: read-only inspection of the user's own install. No files were changed. No game data is copied here.

## Observed

| Topic | Finding | Evidence |
|---|---|---|
| Engine base | `engine.json`: `"LumberyardVersion": "1.12.0.1"` | File content |
| Executable | `Bin64/NewWorld.exe`, 179,204,176 bytes, SHA-256 `8654f01d324636d9f74f1c793b0cc4a417c3c5fa9847d9913c358ca29e0fdc8e`. Sections are standard (`.text` 0x07ec70bd bytes); no packer section names. | `objdump -h`, REA `open_binary` |
| Oodle | Statically linked in `NewWorld.exe` (`oo2::OodleLZ_Decompress` and Oodle error strings present). No `oo2core` DLL anywhere in the install. | `strings`, `find` |
| Pak container | Standard ZIP, not encrypted. 130 paks, 71 GB, 1,446,650 entries. | REA `inspect_artifact` Evidence `ev_c05ea4c9bcc14be2aa2e5a508d3ec29978e5002452b8755390e4e4650f4484c0`; Python `zipfile` scan |
| Entry methods | 15 = Oodle (1,340,017), 0 = store (106,030), 8 = deflate (603) | Scan |
| Oodle codec | Kraken in 1,600 of 1,600 sampled method-15 entries (block header low nibble `0xC`, decoder type `6`) | Header sample |
| Kraken decoding | `oozextract` 0.5.5 decodes all 2,052 Oodle datasheets with matching size and ZIP CRC32 | Throwaway spike |
| Datasheets | 2,250 files, 220 table types, 285,031 rows. Column kinds: 1 = string, 2 = number, 3 = boolean (values `1`/`0`/`TRUE`/`FALSE` only). | Header and value scan |
| `typeindex.json` | In the install root. JSON keys `generationTime` (`2025-10-22T03:13:16Z`) and `typeIndex`: an array of 7,052 unique 32-hex-digit type UUIDs. Entry 0 is the all-zero UUID. | File content |

## Inferred (not verified)

- `typeindex.json` is the ordered `type_idx` → AZ type UUID table that the network message envelope uses (research file 02, section 2.3: `type_idx` is assigned at startup; index 0 = inline UUID). Verification: RE task 4 (master spec 11.3).

## Unknown

- The meaning of the 4 raw value bytes in each datasheet cell.
- The Oodle library version linked into `NewWorld.exe`.
