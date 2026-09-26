// SPDX-License-Identifier: GPL-3.0-or-later
//! The launch options the player sets in the launcher's Options section: the game copy, the
//! Proton build, the prefix, the Steam install, the override-config file, Proton switches and
//! extra environment variables. They are saved with the other settings ([`crate::settings`]).
//! The older `NW_LAUNCHER_*` environment variables still fill options that are empty.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[cfg(any(windows, test))]
use crate::game::WindowsLaunch;
use crate::game::{DirectLaunch, GameError};
use crate::steam::Found;

/// The game executable to run.
pub const GAME_EXE_ENV: &str = "NW_LAUNCHER_GAME_EXE";
/// The Proton program used for a direct launch.
pub const PROTON_ENV: &str = "NW_LAUNCHER_PROTON";
/// The game's Proton prefix for a direct launch.
pub const PREFIX_ENV: &str = "NW_LAUNCHER_STEAM_COMPAT_DATA";
/// The Steam install directory for a direct launch.
pub const STEAM_INSTALL_ENV: &str = "NW_LAUNCHER_STEAM_CLIENT_INSTALL";
/// An absolute override-config file for the client (`-OverrideConfigPath`).
pub const OVERRIDE_CONFIG_ENV: &str = "NW_LAUNCHER_OVERRIDE_CONFIG";
/// Folder of the launcher's own Proton prefix, under the settings folder, used when the prefix
/// option is empty.
pub const DEFAULT_PREFIX_DIR: &str = "prefix";

/// The start of the names of GE-Proton builds, the Proton the game is run with by default.
const GE_PROTON_PREFIX: &str = "GE-Proton";

/// Value of a Proton switch variable that turns it on.
const SWITCH_ON: &str = "1";
/// Separator of a `NAME=value` line, and the start of a comment line.
const ENV_SEPARATOR: char = '=';
const COMMENT_START: char = '#';
/// The variable underscore, allowed in a name with ASCII letters and digits.
const NAME_UNDERSCORE: char = '_';

/// A Proton switch: one environment variable set to `1` for the game process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtonSwitch {
    /// `PROTON_LOG`: write `steam-1063730.log` to the home folder.
    Log,
    /// `PROTON_USE_WINED3D`: OpenGL instead of DXVK.
    UseWined3d,
    /// `PROTON_NO_ESYNC`: no eventfd synchronisation.
    NoEsync,
    /// `PROTON_NO_FSYNC`: no futex synchronisation.
    NoFsync,
    /// `PROTON_ENABLE_NVAPI`: NVIDIA's NVAPI (DLSS) for the game.
    EnableNvapi,
}

impl ProtonSwitch {
    /// All switches, in the order the options show them.
    pub const ALL: [Self; 5] = [
        Self::Log,
        Self::UseWined3d,
        Self::NoEsync,
        Self::NoFsync,
        Self::EnableNvapi,
    ];

    /// The environment variable the switch sets.
    pub fn variable(self) -> &'static str {
        match self {
            Self::Log => "PROTON_LOG",
            Self::UseWined3d => "PROTON_USE_WINED3D",
            Self::NoEsync => "PROTON_NO_ESYNC",
            Self::NoFsync => "PROTON_NO_FSYNC",
            Self::EnableNvapi => "PROTON_ENABLE_NVAPI",
        }
    }

    /// The text next to the check box.
    pub fn label(self) -> &'static str {
        match self {
            Self::Log => "Write a Proton log",
            Self::UseWined3d => "Use WineD3D (no DXVK)",
            Self::NoEsync => "Turn off esync",
            Self::NoFsync => "Turn off fsync",
            Self::EnableNvapi => "Turn on NVAPI (DLSS)",
        }
    }
}

/// What the player set. An empty text is not set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchOptions {
    /// The patched game copy, `.../Bin64/BlightedNewWorld.exe`.
    pub game_exe: String,
    /// The `proton` program (Linux).
    pub proton: String,
    /// The launcher's own Proton prefix folder (Linux); empty gives [`DEFAULT_PREFIX_DIR`].
    pub prefix: String,
    /// The Steam install folder (Linux).
    pub steam_install: String,
    /// An absolute override-config file; empty passes none.
    pub override_config: String,
    /// The Proton switches that are on (Linux).
    pub proton_switches: BTreeSet<ProtonSwitch>,
    /// Extra `NAME=value` lines for the game process; `#` starts a comment line.
    pub environment: String,
}

fn non_empty(value: &str) -> Option<&str> {
    Some(value).filter(|value| !value.is_empty())
}

/// Whether `name` can be an environment variable name: ASCII letters, digits and `_`, not
/// starting with a digit.
fn is_variable_name(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|first| !first.is_ascii_digit())
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == NAME_UNDERSCORE)
}

impl LaunchOptions {
    /// Fills each empty option from its `NW_LAUNCHER_*` variable.
    pub fn fill_from_env(&mut self, lookup: impl Fn(&str) -> Option<String>) {
        for (value, name) in [
            (&mut self.game_exe, GAME_EXE_ENV),
            (&mut self.proton, PROTON_ENV),
            (&mut self.prefix, PREFIX_ENV),
            (&mut self.steam_install, STEAM_INSTALL_ENV),
            (&mut self.override_config, OVERRIDE_CONFIG_ENV),
        ] {
            if value.is_empty()
                && let Some(found) = lookup(name).filter(|found| !found.is_empty())
            {
                *value = found;
            }
        }
    }

    /// Fills the empty game copy, Steam install and Proton build from a scan of this PC. The
    /// default Proton is the newest GE-Proton build found, else the last build by name: without
    /// one the game cannot start at all.
    pub fn fill_from_scan(&mut self, found: &Found) {
        if self.proton.is_empty()
            && let Some(build) = found
                .protons
                .iter()
                .rev()
                .find(|build| build.name.starts_with(GE_PROTON_PREFIX))
                .or_else(|| found.protons.last())
        {
            self.proton = build.program.display().to_string();
        }
        for (value, path) in [
            (&mut self.game_exe, &found.game_exe),
            (&mut self.steam_install, &found.steam_install),
        ] {
            if value.is_empty()
                && let Some(path) = path
            {
                *value = path.display().to_string();
            }
        }
    }

    /// The prefix folder: the option, or `<settings folder>/prefix` when it is empty.
    pub fn prefix_or_default(&self, settings_dir: &Path) -> PathBuf {
        non_empty(&self.prefix).map_or_else(|| settings_dir.join(DEFAULT_PREFIX_DIR), PathBuf::from)
    }

    /// The direct Proton launch, or none when the game copy, Proton or Steam install is empty.
    pub fn direct_launch(&self, settings_dir: &Path) -> Option<DirectLaunch> {
        Some(DirectLaunch {
            proton: non_empty(&self.proton)?.into(),
            exe: non_empty(&self.game_exe)?.into(),
            compat_data: self.prefix_or_default(settings_dir),
            client_install: non_empty(&self.steam_install)?.into(),
        })
    }

    /// The native Windows launch, or none when the game copy is empty.
    #[cfg(any(windows, test))]
    pub fn windows_launch(&self) -> Option<WindowsLaunch> {
        Some(WindowsLaunch {
            exe: non_empty(&self.game_exe)?.into(),
        })
    }

    /// The override-config file, or none when it is empty.
    pub fn override_config(&self) -> Option<&str> {
        non_empty(&self.override_config)
    }

    /// The game process's extra variables: the extra lines, then the Proton switches when
    /// `with_proton` is set. A line that is not `NAME=value` is an error.
    pub fn environment(&self, with_proton: bool) -> Result<Vec<(String, String)>, GameError> {
        let mut variables = Vec::new();
        for line in self.environment.lines().map(str::trim) {
            if line.is_empty() || line.starts_with(COMMENT_START) {
                continue;
            }
            match line.split_once(ENV_SEPARATOR) {
                Some((name, value)) if is_variable_name(name.trim()) => {
                    variables.push((name.trim().to_owned(), value.trim().to_owned()));
                }
                _ => return Err(GameError::BadEnvironment(line.to_owned())),
            }
        }
        if with_proton {
            variables.extend(
                self.proton_switches
                    .iter()
                    .map(|switch| (switch.variable().to_owned(), SWITCH_ON.to_owned())),
            );
        }
        Ok(variables)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn env_variables_fill_only_empty_options() {
        let mut options = LaunchOptions {
            game_exe: "/saved/BlightedNewWorld.exe".to_owned(),
            ..LaunchOptions::default()
        };
        options.fill_from_env(lookup(&[
            (GAME_EXE_ENV, "/env/BlightedNewWorld.exe"),
            (PROTON_ENV, "/env/proton"),
            (PREFIX_ENV, ""),
            (OVERRIDE_CONFIG_ENV, "Z:/x/ClientOverride.json"),
        ]));
        assert_eq!(options.game_exe, "/saved/BlightedNewWorld.exe");
        assert_eq!(options.proton, "/env/proton");
        assert_eq!(options.prefix, "");
        assert_eq!(options.override_config(), Some("Z:/x/ClientOverride.json"));
    }

    #[test]
    fn a_scan_fills_the_game_steam_and_the_newest_ge_proton() {
        let build = |name: &str| crate::steam::ProtonBuild {
            name: name.to_owned(),
            program: PathBuf::from(format!("/s/{name}/proton")),
        };
        let found = Found {
            steam_install: Some(PathBuf::from("/s")),
            game_exe: Some(PathBuf::from("/g/BlightedNewWorld.exe")),
            protons: vec![
                build("GE-Proton10-11"),
                build("GE-Proton10-12"),
                build("Proton 9.0"),
            ],
        };
        let mut options = LaunchOptions::default();
        options.fill_from_scan(&found);
        assert_eq!(options.steam_install, "/s");
        assert_eq!(options.game_exe, "/g/BlightedNewWorld.exe");
        assert_eq!(options.proton, "/s/GE-Proton10-12/proton");
        let mut chosen = LaunchOptions {
            proton: "/mine/proton".to_owned(),
            ..LaunchOptions::default()
        };
        chosen.fill_from_scan(&found);
        assert_eq!(chosen.proton, "/mine/proton", "the player's pick stays");
        let mut plain = LaunchOptions::default();
        plain.fill_from_scan(&Found {
            protons: vec![build("Proton 8.0"), build("Proton 9.0")],
            ..Found::default()
        });
        assert_eq!(plain.proton, "/s/Proton 9.0/proton");
    }

    #[test]
    fn the_direct_launch_needs_three_paths_and_defaults_the_prefix() {
        let settings_dir = Path::new("/cfg/blighted-launcher");
        let mut options = LaunchOptions {
            game_exe: "/g/BlightedNewWorld.exe".to_owned(),
            proton: "/p/proton".to_owned(),
            ..LaunchOptions::default()
        };
        assert_eq!(options.direct_launch(settings_dir), None);
        options.steam_install = "/s".to_owned();
        let launch = options.direct_launch(settings_dir).expect("launch");
        assert_eq!(launch.compat_data, settings_dir.join(DEFAULT_PREFIX_DIR));
        options.prefix = "/own/prefix".to_owned();
        assert_eq!(
            options
                .direct_launch(settings_dir)
                .expect("launch")
                .compat_data,
            PathBuf::from("/own/prefix")
        );
        assert_eq!(LaunchOptions::default().windows_launch(), None);
        assert_eq!(
            options.windows_launch().expect("windows").exe,
            PathBuf::from("/g/BlightedNewWorld.exe")
        );
    }

    #[test]
    fn environment_lines_and_switches_become_variables() {
        let options = LaunchOptions {
            environment: "# comment\n\nDXVK_HUD = fps\nWINEDEBUG=-all\n".to_owned(),
            proton_switches: BTreeSet::from([ProtonSwitch::NoFsync, ProtonSwitch::Log]),
            ..LaunchOptions::default()
        };
        let pair = |name: &str, value: &str| (name.to_owned(), value.to_owned());
        assert_eq!(
            options.environment(true).expect("variables"),
            [
                pair("DXVK_HUD", "fps"),
                pair("WINEDEBUG", "-all"),
                pair("PROTON_LOG", "1"),
                pair("PROTON_NO_FSYNC", "1"),
            ]
        );
        assert_eq!(options.environment(false).expect("variables").len(), 2);
    }

    #[test]
    fn a_bad_environment_line_is_refused() {
        for bad in ["NO_SEPARATOR", "=value", "1ST=x", "BAD NAME=x"] {
            let options = LaunchOptions {
                environment: bad.to_owned(),
                ..LaunchOptions::default()
            };
            assert!(
                matches!(
                    options.environment(true),
                    Err(GameError::BadEnvironment(line)) if line == bad
                ),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn switches_save_by_name() {
        let options = LaunchOptions {
            proton_switches: BTreeSet::from([ProtonSwitch::EnableNvapi]),
            ..LaunchOptions::default()
        };
        let text = serde_json::to_string(&options).expect("json");
        assert!(text.contains("\"enable_nvapi\""), "{text}");
        assert_eq!(
            serde_json::from_str::<LaunchOptions>(&text).expect("load"),
            options
        );
    }
}
