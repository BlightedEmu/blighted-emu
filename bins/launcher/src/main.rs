// SPDX-License-Identifier: GPL-3.0-or-later
//! `launcher`: the player launcher window.

use std::process::ExitCode;

use launcher::app::LauncherApp;
use launcher::install_crypto_provider;
use launcher::settings::{Settings, default_settings_path};

/// Window title.
const APP_NAME: &str = "Blighted New World launcher";

fn main() -> ExitCode {
    install_crypto_provider();
    let path = match default_settings_path() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let settings = match Settings::load(&path) {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = eframe::run_native(
        APP_NAME,
        eframe::NativeOptions::default(),
        Box::new(move |_| Ok(Box::new(LauncherApp::new(path, settings)))),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
