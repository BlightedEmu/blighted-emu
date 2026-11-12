// SPDX-License-Identifier: GPL-3.0-or-later
//! `BlightedLauncher`: the player launcher window.

// A window program on Windows: no console window opens beside the launcher.
#![windows_subsystem = "windows"]

use std::fmt::Display;
use std::process::ExitCode;

use launcher::commands::{self, AppState};
use launcher::install_crypto_provider;
use launcher::news::{NewsCache, NewsState};
use launcher::session::Session;
use launcher::settings::{Settings, default_settings_path};
use launcher::steam;
use launcher::updates::{self, UpdateState};

/// Title of the error box. The window title is in `tauri.conf.json`.
const APP_NAME: &str = "Blighted New World launcher";

fn main() -> ExitCode {
    install_crypto_provider();
    let path = match default_settings_path() {
        Ok(path) => path,
        Err(error) => {
            report_start_error(&error);
            return ExitCode::FAILURE;
        }
    };
    // A settings file that cannot be read stops the start, so it is never overwritten.
    let mut settings = match Settings::load(&path) {
        Ok(settings) => settings,
        Err(error) => {
            report_start_error(&error);
            return ExitCode::FAILURE;
        }
    };
    let lookup = |name: &str| std::env::var(name).ok();
    settings.options.fill_from_env(lookup);
    let found = steam::scan(lookup);
    settings.options.fill_from_scan(&found);
    let session = Session::new(path, settings, found);
    let news = NewsState::open(NewsCache::new(session.settings_dir()));
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState::new(session, news))
        .manage(UpdateState::default())
        .invoke_handler(tauri::generate_handler![
            commands::view,
            commands::connect,
            commands::rename_server,
            commands::remove_server,
            commands::log_in,
            commands::discord_log_in,
            commands::play,
            commands::log_out,
            commands::set_options,
            commands::find_paths,
            commands::set_theme,
            commands::open_link,
            commands::news_view,
            commands::news_picture,
            commands::update_view,
            commands::install_update,
        ])
        .setup(|app| {
            commands::start(app.handle());
            updates::start_check(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!());
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report_start_error(&error);
            ExitCode::FAILURE
        }
    }
}

/// Shows an error that stops the launcher before or while its window opens: a message box on
/// Windows, where the launcher has no console, and standard error elsewhere.
fn report_start_error(error: &dyn Display) {
    #[cfg(windows)]
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(APP_NAME)
        .set_description(error.to_string())
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
    #[cfg(not(windows))]
    eprintln!("{APP_NAME}: {error}");
}
