// SPDX-License-Identifier: GPL-3.0-or-later
//! Tauri's build step: reads `tauri.conf.json`, puts the icon into the Windows program file and
//! makes the permission of each command that `capabilities/default.json` grants the page.

/// The commands of `src/commands.rs`, in the order `main.rs` registers them.
const COMMANDS: [&str; 16] = [
    "view",
    "connect",
    "rename_server",
    "remove_server",
    "log_in",
    "discord_log_in",
    "play",
    "log_out",
    "set_options",
    "find_paths",
    "set_theme",
    "open_link",
    "news_view",
    "news_picture",
    "update_view",
    "install_update",
];

fn main() {
    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(&COMMANDS));
    if let Err(error) = tauri_build::try_build(attributes) {
        panic!("the launcher's Tauri build step failed: {error:#}");
    }
}
