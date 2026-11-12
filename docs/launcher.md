# The launcher: install, updates and releases

The player launcher is `bins/launcher`. Its window is a web page (`bins/launcher/ui`) in a Tauri
shell. The Rust part logs in, starts the game and reads the news. The page only shows what Rust
sends and calls Rust for each button. How the launcher starts the patched game is in
[client-patch.md](client-patch.md).

## Install

| System | File | What to do |
|---|---|---|
| Windows | `BlightedLauncher_<version>_x64-setup.exe` | Run it once. It installs for the current user only (no admin prompt). Windows 10 and 11 already have the web view it needs. |
| Linux | `BlightedLauncher_<version>_amd64.AppImage` | Make it executable (`chmod +x`) and run it. |

The setup file has no paid code-signing certificate, so on the first install Windows shows
"Windows protected your PC". Click "More info", then "Run anyway". Updates after that do not ask.

The launcher keeps its settings where older launchers kept them
(`<user config folder>/blighted-launcher/settings.json`), so saved servers and logins stay.

## Updates

Each time the launcher starts, it reads `latest.json` from the newest GitHub Release of this
repository. Login and Play never wait for this check.

- If the release is newer, a pop-up says an update is available. "Update now" downloads the
  update, checks its signature, installs it and opens the new launcher. "Later" closes the pop-up
  until the next start; "Update ready" at the top right opens it again.
- While the game runs, "Update now" is off and says "Close the game to update". On Windows the
  game's network calls pass through the launcher, so a restart would cut them.
- If GitHub cannot be reached, or the check fails, there is no pop-up. The top right then says
  "Could not check for updates" and the launcher works as before.

Every update is signed. The public key is in `bins/launcher/tauri.conf.json`. The private key
and its password are GitHub secrets (`TAURI_SIGNING_PRIVATE_KEY`,
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`), and a copy is kept on the owner's PC outside git. If the
private key is lost, installed launchers cannot update again: players must install a new setup
file by hand.

## Making a release

Only launcher releases are made in this repository, and only by hand. Pushes never start one.

1. Raise `version` in `bins/launcher/Cargo.toml` by one step (0.1.0, then 0.1.1, 0.1.2 and so on)
   and commit it.
2. Merge the change to `main`.
3. Start the "launcher release" workflow on GitHub (Actions tab, "Run workflow", branch `main`),
   or run:

   ```bash
   gh workflow run launcher-release.yml --ref main -f notes="- What changed"
   ```

   Write the notes as `- ` lines. The update pop-up shows each line as a list item.

The workflow (`.github/workflows/launcher-release.yml`) does this:

1. It stops unless it runs on `main`, and stops if the tag `launcher-v<version>` or a release of it
   already exists.
2. It builds the Windows setup file on a Windows machine and the AppImage on Ubuntu 22.04, at the
   same time. These two jobs make no tag and no release.
3. Only when both builds pass, it writes `latest.json`
   (`scripts/launcher-release-manifest.py`), makes the release as a draft with the five files
   (setup file, AppImage, their two `.sig` files and `latest.json`), checks that all five are
   there, and then makes it public.

If any step fails, the draft is deleted. A failed run leaves no tag and no public release.

## Building on this PC

The Linux build needs WebKitGTK 4.1 (`libwebkit2gtk-4.1-dev`) and the Tauri command
(`cargo install tauri-cli --version 2.12.4 --locked`).

```bash
cd bins/launcher && cargo tauri build --no-bundle
```

That makes the plain program with the page built in (`target/release/BlightedLauncher`). The
AppImage needs the signing key in the environment:

```bash
cd bins/launcher && cargo tauri build --bundles appimage
```

Release files always come from the workflow, not from this PC.
