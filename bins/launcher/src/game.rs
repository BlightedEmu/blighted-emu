// SPDX-License-Identifier: GPL-3.0-or-later
//! Starting New World through Steam (M2 spec section 6). Steam must run, because the game asks it
//! for a Steam ticket.

use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use nw_protocol::launcher_api::{SESSION_PLACEHOLDER, is_forbidden_host};
use reqwest::Url;

/// Steam application id of New World: Aeternum.
pub const STEAM_APP_ID: &str = "1063730";
/// Steam argument that starts an application.
pub const APPLAUNCH_ARGUMENT: &str = "-applaunch";
/// The only game arguments a server may ask for (endpoint RE note).
pub const ALLOWED_ARGUMENTS: [&str; 7] = [
    "-ChannelService",
    "-LoginChannelOverride",
    "-GatewayMode",
    "-GatewayAddr",
    "-HttpGatewayAddr",
    "-AuthEndpoint",
    "-AuthBackend",
];
/// Launch argument that points the client at an absolute override-config file, bypassing the
/// packed `@assets@` lookup.
pub const OVERRIDE_CONFIG_ARG: &str = "-OverrideConfigPath";
/// Environment variable that, when set to a non-empty value, makes the launcher pass
/// [`OVERRIDE_CONFIG_ARG`] with that value, so the client reads an absolute override-config file
/// instead of the packed `@assets@/ClientOverride.json` that a retail client does not read.
pub const OVERRIDE_CONFIG_ENV: &str = "NW_LAUNCHER_OVERRIDE_CONFIG";
/// Default Steam program.
#[cfg(windows)]
pub const DEFAULT_STEAM: &str = r"C:\Program Files (x86)\Steam\steam.exe";
/// Default Steam program (found on `PATH`).
#[cfg(not(windows))]
pub const DEFAULT_STEAM: &str = "steam";
/// A value must not start with these, so that it cannot act as an argument name.
const ARGUMENT_SIGILS: [char; 2] = ['-', '+'];
/// Characters that could change how the game or Steam splits the arguments.
const REFUSED_VALUE_CHARACTERS: [char; 3] = ['"', '\'', '\\'];
/// Separator between a host and its port.
const PORT_SEPARATOR: char = ':';

/// The game could not be started.
#[derive(Debug, thiserror::Error)]
pub enum GameError {
    /// The server asked for an argument that is not allowed.
    #[error("the server asked for the game argument {0}, which is not allowed")]
    NotAllowed(String),
    /// The server asked for an argument more than once.
    #[error("the server asked for the game argument {0} more than once")]
    Repeated(String),
    /// An argument has no value after it.
    #[error("the server's game argument {0} has no value")]
    MissingValue(String),
    /// An argument value is empty, starts with `-` or `+`, or has spaces, quotes, back-slashes
    /// or control characters.
    #[error("the server asked for the game argument value {0:?}, which is not allowed")]
    BadValue(String),
    /// The server's arguments do not pass the session key.
    #[error("the server's launch arguments have no session placeholder")]
    NoPlaceholder,
    /// An argument value points to an Amazon or CloudFront host.
    #[error("the server asked for the Amazon or CloudFront address {0}")]
    Forbidden(String),
    /// The game program (Steam or Proton) could not be started.
    #[error("could not start the game: {} failed: {source}", .program.display())]
    Start {
        /// The program that failed to start.
        program: PathBuf,
        /// The start error.
        #[source]
        source: std::io::Error,
    },
}

/// A direct Proton launch of a game executable, bypassing Steam's own game launcher (and so
/// Easy Anti-Cheat). Set by the four [`DIRECT_EXE_ENV`] family variables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectLaunch {
    /// The Proton program.
    pub proton: PathBuf,
    /// The game executable to run (for example the patched `BlightedNewWorld.exe`).
    pub exe: PathBuf,
    /// `STEAM_COMPAT_DATA_PATH`: the game's Proton prefix (`compatdata/<app-id>`).
    pub compat_data: PathBuf,
    /// `STEAM_COMPAT_CLIENT_INSTALL_PATH`: the Steam install directory.
    pub client_install: PathBuf,
}

/// The game executable to run directly under Proton.
pub const DIRECT_EXE_ENV: &str = "NW_LAUNCHER_GAME_EXE";
/// The Proton program used for a direct launch.
pub const DIRECT_PROTON_ENV: &str = "NW_LAUNCHER_PROTON";
/// The game's Proton prefix for a direct launch.
pub const DIRECT_COMPAT_DATA_ENV: &str = "NW_LAUNCHER_STEAM_COMPAT_DATA";
/// The Steam install directory for a direct launch.
pub const DIRECT_CLIENT_INSTALL_ENV: &str = "NW_LAUNCHER_STEAM_CLIENT_INSTALL";

/// The Proton verb that runs a program in the prefix.
const PROTON_VERB: &str = "run";
/// Steam app-id environment variables the game reads to find its Steam context.
const STEAM_APP_ID_ENV: &str = "SteamAppId";
const STEAM_GAME_ID_ENV: &str = "SteamGameId";
const STEAM_COMPAT_DATA_PATH_ENV: &str = "STEAM_COMPAT_DATA_PATH";
const STEAM_COMPAT_CLIENT_INSTALL_PATH_ENV: &str = "STEAM_COMPAT_CLIENT_INSTALL_PATH";

/// A [`DirectLaunch`] from the environment, or none when any of the four variables is missing.
pub fn direct_launch_from_env() -> Option<DirectLaunch> {
    Some(DirectLaunch {
        proton: std::env::var_os(DIRECT_PROTON_ENV)?.into(),
        exe: std::env::var_os(DIRECT_EXE_ENV)?.into(),
        compat_data: std::env::var_os(DIRECT_COMPAT_DATA_ENV)?.into(),
        client_install: std::env::var_os(DIRECT_CLIENT_INSTALL_ENV)?.into(),
    })
}

/// `<proton> run <exe> <arguments>` with the Steam compatibility environment, so a patched game
/// executable runs under Proton without Steam's own launcher and without Easy Anti-Cheat.
pub fn direct_command(launch: &DirectLaunch, arguments: &[String]) -> Command {
    let mut command = Command::new(&launch.proton);
    command
        .arg(PROTON_VERB)
        .arg(&launch.exe)
        .args(arguments)
        .env(STEAM_COMPAT_DATA_PATH_ENV, &launch.compat_data)
        .env(STEAM_COMPAT_CLIENT_INSTALL_PATH_ENV, &launch.client_install)
        .env(STEAM_APP_ID_ENV, STEAM_APP_ID)
        .env(STEAM_GAME_ID_ENV, STEAM_APP_ID);
    command
}

fn is_plain_value(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with(ARGUMENT_SIGILS)
        && !value.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || REFUSED_VALUE_CHARACTERS.contains(&character)
        })
}

/// The host a value names: the host of a URL, else the part before `:port`.
fn value_host(value: &str) -> String {
    Url::parse(value)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| {
            value
                .rsplit_once(PORT_SEPARATOR)
                .map_or(value, |(host, _)| host)
                .to_owned()
        })
}

/// The game arguments: the server's template of `name value` pairs with the session key in
/// place of the placeholder. Each name is allowed and appears once; each value is plain and
/// names no Amazon or CloudFront host (review finding 4).
pub fn game_arguments(template: &[String], session_key: &str) -> Result<Vec<String>, GameError> {
    let mut has_placeholder = false;
    let mut names = HashSet::new();
    let mut arguments = Vec::with_capacity(template.len());
    let mut items = template.iter();
    while let Some(name) = items.next() {
        if !ALLOWED_ARGUMENTS.contains(&name.as_str()) {
            return Err(GameError::NotAllowed(name.clone()));
        }
        if !names.insert(name) {
            return Err(GameError::Repeated(name.clone()));
        }
        let value = items
            .next()
            .ok_or_else(|| GameError::MissingValue(name.clone()))?;
        if !is_plain_value(value) {
            return Err(GameError::BadValue(value.clone()));
        }
        let host = value_host(value);
        if is_forbidden_host(&host) {
            return Err(GameError::Forbidden(host));
        }
        has_placeholder |= value.contains(SESSION_PLACEHOLDER);
        arguments.push(name.clone());
        arguments.push(value.replace(SESSION_PLACEHOLDER, session_key));
    }
    if has_placeholder {
        Ok(arguments)
    } else {
        Err(GameError::NoPlaceholder)
    }
}

/// The `-OverrideConfigPath <path>` pair for a path value, or none when it is absent or empty.
fn override_config_pair(path: Option<&str>) -> Option<(String, String)> {
    match path {
        Some(value) if !value.is_empty() => {
            Some((OVERRIDE_CONFIG_ARG.to_owned(), value.to_owned()))
        }
        _ => None,
    }
}

/// Appends `-OverrideConfigPath <path>` when [`OVERRIDE_CONFIG_ENV`] holds a non-empty value, so
/// the client reads that absolute override-config file. Returns `arguments` unchanged otherwise.
pub fn append_override_config(mut arguments: Vec<String>) -> Vec<String> {
    if let Some((name, value)) = override_config_pair(env::var(OVERRIDE_CONFIG_ENV).ok().as_deref())
    {
        arguments.push(name);
        arguments.push(value);
    }
    arguments
}

/// `steam -applaunch 1063730 <arguments>`.
pub fn steam_command(steam: &Path, arguments: &[String]) -> Command {
    let mut command = Command::new(steam);
    command
        .arg(APPLAUNCH_ARGUMENT)
        .arg(STEAM_APP_ID)
        .args(arguments);
    command
}

/// Starts the game and returns at once. Uses a direct Proton launch when
/// [`direct_launch_from_env`] is configured, otherwise Steam.
pub fn start_game(steam: &Path, arguments: &[String]) -> Result<(), GameError> {
    let mut command = match direct_launch_from_env() {
        Some(launch) => direct_command(&launch, arguments),
        None => steam_command(steam, arguments),
    };
    let program = PathBuf::from(command.get_program());
    command
        .spawn()
        .map(drop)
        .map_err(|source| GameError::Start { program, source })
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    const KEY: &str = "abcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrst";

    fn template(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn session_key_replaces_the_placeholder() {
        let arguments = game_arguments(
            &template(&[
                "-ChannelService",
                "https://login.example.org",
                "-LoginChannelOverride",
                SESSION_PLACEHOLDER,
            ]),
            KEY,
        )
        .expect("arguments");
        assert_eq!(arguments[3], KEY);
        assert_eq!(arguments[1], "https://login.example.org");
    }

    #[test]
    fn the_auth_backend_argument_is_allowed() {
        let arguments = game_arguments(
            &template(&[
                "-LoginChannelOverride",
                SESSION_PLACEHOLDER,
                "-AuthBackend",
                "nw",
            ]),
            KEY,
        )
        .expect("arguments");
        assert_eq!(arguments[2..], ["-AuthBackend", "nw"]);
    }

    #[test]
    fn other_arguments_are_refused() {
        assert!(matches!(
            game_arguments(&template(&["-exec", "evil.cfg", SESSION_PLACEHOLDER]), KEY),
            Err(GameError::NotAllowed(name)) if name == "-exec"
        ));
        assert!(matches!(
            game_arguments(
                &template(&["-ChannelService", "https://login.example.org"]),
                KEY
            ),
            Err(GameError::NoPlaceholder)
        ));
        assert!(matches!(
            game_arguments(
                &template(&["-ChannelService", "https://d2c74t4zimux3r.cloudfront.net/x", SESSION_PLACEHOLDER]),
                KEY
            ),
            Err(GameError::Forbidden(host)) if host == "d2c74t4zimux3r.cloudfront.net"
        ));
    }

    /// The arguments `name value` followed by a valid session pair.
    fn with_session(name: &str, value: &str) -> Vec<String> {
        template(&[name, value, "-LoginChannelOverride", SESSION_PLACEHOLDER])
    }

    #[test]
    fn host_and_port_values_are_checked() {
        assert!(matches!(
            game_arguments(&with_session("-GatewayAddr", "x.cloudfront.net:443"), KEY),
            Err(GameError::Forbidden(host)) if host == "x.cloudfront.net"
        ));
        assert!(game_arguments(&with_session("-GatewayAddr", "rep.example.org:443"), KEY).is_ok());
    }

    #[test]
    fn values_must_be_plain() {
        for bad in [
            "+exec evil.cfg",
            "a b",
            "\"quoted\"",
            "back\\slash",
            "-ChannelService",
            "",
        ] {
            assert!(
                matches!(
                    game_arguments(&with_session("-GatewayAddr", bad), KEY),
                    Err(GameError::BadValue(value)) if value == bad
                ),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn names_need_one_value_and_appear_once() {
        assert!(matches!(
            game_arguments(&template(&["-LoginChannelOverride", SESSION_PLACEHOLDER, "-GatewayMode"]), KEY),
            Err(GameError::MissingValue(name)) if name == "-GatewayMode"
        ));
        let mut twice = with_session("-GatewayMode", "1");
        twice.extend(template(&["-GatewayMode", "2"]));
        assert!(matches!(
            game_arguments(&twice, KEY),
            Err(GameError::Repeated(name)) if name == "-GatewayMode"
        ));
    }

    #[test]
    fn steam_command_launches_the_app() {
        let command = steam_command(
            Path::new(DEFAULT_STEAM),
            &template(&["-LoginChannelOverride", KEY]),
        );
        let arguments: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            arguments,
            [
                APPLAUNCH_ARGUMENT,
                STEAM_APP_ID,
                "-LoginChannelOverride",
                KEY
            ]
        );
    }

    #[test]
    fn override_config_pair_is_added_only_for_a_non_empty_path() {
        assert_eq!(
            override_config_pair(Some("Z:/x/ClientOverride.json")),
            Some((
                "-OverrideConfigPath".to_owned(),
                "Z:/x/ClientOverride.json".to_owned()
            ))
        );
        assert_eq!(override_config_pair(Some("")), None);
        assert_eq!(override_config_pair(None), None);
    }

    #[test]
    fn append_override_config_keeps_the_original_arguments_first() {
        let base = template(&["-LoginChannelOverride", KEY]);
        let result = append_override_config(base.clone());
        assert_eq!(result[..base.len()], base[..]);
    }

    #[test]
    fn direct_command_runs_the_exe_under_proton_with_steam_env() {
        let launch = DirectLaunch {
            proton: PathBuf::from("/opt/GE-Proton/proton"),
            exe: PathBuf::from("/games/New World/Bin64/BlightedNewWorld.exe"),
            compat_data: PathBuf::from("/steam/steamapps/compatdata/1063730"),
            client_install: PathBuf::from("/home/p/.local/share/Steam"),
        };
        let command = direct_command(&launch, &template(&["-LoginChannelOverride", KEY]));
        assert_eq!(command.get_program(), OsStr::new("/opt/GE-Proton/proton"));
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "run",
                "/games/New World/Bin64/BlightedNewWorld.exe",
                "-LoginChannelOverride",
                KEY
            ]
        );
        let envs: std::collections::HashMap<String, String> = command
            .get_envs()
            .filter_map(|(key, value)| {
                value.map(|value| {
                    (
                        key.to_string_lossy().into_owned(),
                        value.to_string_lossy().into_owned(),
                    )
                })
            })
            .collect();
        assert_eq!(
            envs.get("SteamAppId").map(String::as_str),
            Some(STEAM_APP_ID)
        );
        assert_eq!(
            envs.get("STEAM_COMPAT_DATA_PATH").map(String::as_str),
            Some("/steam/steamapps/compatdata/1063730")
        );
        assert_eq!(
            envs.get("STEAM_COMPAT_CLIENT_INSTALL_PATH")
                .map(String::as_str),
            Some("/home/p/.local/share/Steam")
        );
    }

    #[test]
    fn missing_steam_is_reported() {
        assert!(matches!(
            start_game(Path::new("/nonexistent/steam"), &[]),
            Err(GameError::Start { .. })
        ));
    }
}
