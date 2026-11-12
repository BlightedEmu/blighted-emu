// SPDX-License-Identifier: GPL-3.0-or-later
//! `launcher`: the player launcher window.

// A window program on Windows: no console window opens beside the launcher.
#![windows_subsystem = "windows"]

use std::fmt::Display;
use std::process::ExitCode;

use eframe::egui;
use launcher::app::LauncherApp;
use launcher::settings::{Settings, default_settings_path};
use launcher::steam;
use launcher::theme;
use launcher::{install_crypto_provider, window_icon};

/// Window title.
const APP_NAME: &str = "Blighted New World launcher";
/// Window size at start (wide enough for the play column beside the news), and the smallest
/// size (one column).
const WINDOW_SIZE: [f32; 2] = [1080.0, 720.0];
const MIN_WINDOW_SIZE: [f32; 2] = [420.0, 520.0];

fn main() -> ExitCode {
    install_crypto_provider();
    let path = match default_settings_path() {
        Ok(path) => path,
        Err(error) => {
            report_start_error(&error);
            return ExitCode::FAILURE;
        }
    };
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
    let icon = match window_icon() {
        Ok(icon) => icon,
        Err(error) => {
            report_start_error(&error);
            return ExitCode::FAILURE;
        }
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_icon(icon)
            .with_inner_size(WINDOW_SIZE)
            .with_min_inner_size(MIN_WINDOW_SIZE),
        ..Default::default()
    };
    let result = eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |creation| {
            theme::install(&creation.egui_ctx, settings.theme);
            Ok(Box::new(LauncherApp::new(path, settings, found)))
        }),
    );
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
    eprintln!("error: {error}");
}
