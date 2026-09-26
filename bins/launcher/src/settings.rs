// SPDX-License-Identifier: GPL-3.0-or-later
//! Saved launcher settings (M2 spec section 6): server address, launcher token, theme and launch
//! options.
//! The file is readable only by the user on Unix.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::options::LaunchOptions;
use crate::theme::ThemeChoice;

/// Folder under the user config folder.
pub const SETTINGS_DIR: &str = "blighted-launcher";
/// Settings file name.
pub const SETTINGS_FILE: &str = "settings.json";
/// File mode of the settings file on Unix: owner read and write only.
#[cfg(unix)]
const OWNER_ONLY_MODE: u32 = 0o600;

/// What the launcher remembers. Keys of older launchers (`steam_path`, from when the game started
/// through Steam's launcher) are ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The server address as typed.
    pub server: String,
    /// Launcher session token, when logged in.
    pub launcher_token: Option<String>,
    /// The window theme; files of older launchers have none and give night.
    pub theme: ThemeChoice,
    /// How the game starts; files of older launchers have none and give empty options.
    pub options: LaunchOptions,
}

/// The settings file could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    /// No user config folder is known on this system.
    #[error("no user config folder found")]
    NoConfigDir,
    /// The file could not be read or written.
    #[error("cannot access {}: {source}", .path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// The I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The file is not valid JSON settings.
    #[error("settings file {} is not valid: {source}", .path.display())]
    Parse {
        /// The file.
        path: PathBuf,
        /// The JSON error.
        #[source]
        source: serde_json::Error,
    },
}

/// Environment variable of the user config folder on Windows.
#[cfg(windows)]
const CONFIG_DIR_ENV: &str = "APPDATA";
/// Environment variable of the user config folder on Linux (XDG base directories).
#[cfg(not(windows))]
const CONFIG_DIR_ENV: &str = "XDG_CONFIG_HOME";
/// Home folder variable; the XDG default config folder is `$HOME/.config`.
#[cfg(not(windows))]
const HOME_ENV: &str = "HOME";
#[cfg(not(windows))]
const XDG_DEFAULT_CONFIG_DIR: &str = ".config";

/// The user config folder from environment variables.
fn config_dir(lookup: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let set = |name: &str| {
        lookup(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    #[cfg(windows)]
    {
        set(CONFIG_DIR_ENV)
    }
    #[cfg(not(windows))]
    {
        set(CONFIG_DIR_ENV).or_else(|| set(HOME_ENV).map(|home| home.join(XDG_DEFAULT_CONFIG_DIR)))
    }
}

/// `<user config folder>/blighted-launcher/settings.json`.
pub fn default_settings_path() -> Result<PathBuf, SettingsError> {
    config_dir(|name| std::env::var(name).ok())
        .map(|dir| dir.join(SETTINGS_DIR).join(SETTINGS_FILE))
        .ok_or(SettingsError::NoConfigDir)
}

impl Settings {
    /// Records the server the launcher is connected to. A launcher token belongs to one server,
    /// so another server drops it (review finding 5).
    pub fn set_server(&mut self, server: &str) {
        if self.server != server {
            self.server = server.to_owned();
            self.launcher_token = None;
        }
    }

    /// Reads the settings; a missing file gives the defaults.
    pub fn load(path: &Path) -> Result<Self, SettingsError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(source) => {
                return Err(SettingsError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        serde_json::from_str(&text).map_err(|source| SettingsError::Parse {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Writes the settings, creating the folder. On Unix only the owner can read the file.
    pub fn save(&self, path: &Path) -> Result<(), SettingsError> {
        let io_error = |source| SettingsError::Io {
            path: path.to_path_buf(),
            source,
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io_error)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|source| SettingsError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            options.mode(OWNER_ONLY_MODE);
            // An older file may have wider rights; narrow them before writing the token.
            if path.exists() {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(OWNER_ONLY_MODE))
                    .map_err(io_error)?;
            }
        }
        let mut file = options.open(path).map_err(io_error)?;
        file.write_all(text.as_bytes()).map_err(io_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Settings {
        Settings {
            server: "example.org".to_owned(),
            launcher_token: Some("token".to_owned()),
            theme: ThemeChoice::Day,
            options: LaunchOptions {
                game_exe: "/g/BlightedNewWorld.exe".to_owned(),
                environment: "DXVK_HUD=fps".to_owned(),
                ..LaunchOptions::default()
            },
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn config_dir_follows_xdg() {
        let lookup = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| (*value).to_owned())
            }
        };
        assert_eq!(
            config_dir(lookup(&[("XDG_CONFIG_HOME", "/cfg"), ("HOME", "/home/a")])),
            Some(PathBuf::from("/cfg"))
        );
        assert_eq!(
            config_dir(lookup(&[("HOME", "/home/a")])),
            Some(PathBuf::from("/home/a/.config"))
        );
        assert_eq!(config_dir(lookup(&[])), None);
    }

    #[test]
    fn a_new_server_drops_the_token() {
        let mut settings = sample();
        settings.set_server("example.org");
        assert_eq!(settings, sample());
        settings.set_server("other.example.org");
        assert_eq!(settings.server, "other.example.org");
        assert_eq!(settings.launcher_token, None);
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().expect("dir");
        assert_eq!(
            Settings::load(&dir.path().join("none.json")).expect("load"),
            Settings::default()
        );
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join(SETTINGS_DIR).join(SETTINGS_FILE);
        sample().save(&path).expect("save");
        assert_eq!(Settings::load(&path).expect("load"), sample());
    }

    /// Mode of a file that others can read.
    #[cfg(unix)]
    const WORLD_READABLE_MODE: u32 = 0o644;
    /// Permission bits of a mode.
    #[cfg(unix)]
    const PERMISSION_BITS: u32 = 0o777;

    #[cfg(unix)]
    #[test]
    fn settings_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join(SETTINGS_FILE);
        std::fs::write(&path, "{}").expect("old file");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(WORLD_READABLE_MODE))
            .expect("open rights");
        sample().save(&path).expect("save");
        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & PERMISSION_BITS, OWNER_ONLY_MODE);
    }

    #[test]
    fn an_older_file_with_a_steam_path_still_loads() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join(SETTINGS_FILE);
        std::fs::write(
            &path,
            r#"{"server":"example.org","launcher_token":"token","steam_path":null}"#,
        )
        .expect("write");
        assert_eq!(
            Settings::load(&path).expect("load"),
            Settings {
                theme: ThemeChoice::Night,
                options: LaunchOptions::default(),
                ..sample()
            }
        );
    }

    #[test]
    fn broken_file_is_an_error() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join(SETTINGS_FILE);
        std::fs::write(&path, "not json").expect("write");
        assert!(matches!(
            Settings::load(&path),
            Err(SettingsError::Parse { .. })
        ));
    }
}
