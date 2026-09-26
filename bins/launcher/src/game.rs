// SPDX-License-Identifier: GPL-3.0-or-later
//! Starting New World (M2 spec section 6): the game executable runs directly under Proton,
//! inside `bwrap` with the private network files of [`crate::mirror`]. Steam must run, because the
//! game asks it for a Steam ticket. The launcher never starts the game through Steam's own
//! launcher: that path runs the retail executable in Steam's retail prefix, and there the game
//! would call Amazon's remote-config, analytics and STS hosts.

use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::mirror::{BWRAP_PROGRAM, NetworkFiles, bwrap_arguments};

use nw_protocol::launcher_api::{SESSION_PLACEHOLDER, is_forbidden_host};
use reqwest::Url;

/// Steam application id of New World: Aeternum.
pub const STEAM_APP_ID: &str = "1063730";
/// The only game arguments a server may ask for (endpoint RE note; `-CognitoPool` names the
/// `CognitoId` remote-config layers, remote-config note).
pub const ALLOWED_ARGUMENTS: [&str; 8] = [
    "-ChannelService",
    "-LoginChannelOverride",
    "-GatewayMode",
    "-GatewayAddr",
    "-HttpGatewayAddr",
    "-AuthEndpoint",
    "-AuthBackend",
    "-CognitoPool",
];
/// The end of Steam's own prefix of New World (`<library>/steamapps/compatdata/1063730`). Retail
/// play uses it, so the launcher refuses it as the direct-launch prefix.
const RETAIL_PREFIX_PARTS: [&str; 3] = ["steamapps", "compatdata", STEAM_APP_ID];
/// Launch argument that points the client at an absolute override-config file, bypassing the
/// packed `@assets@` lookup.
pub const OVERRIDE_CONFIG_ARG: &str = "-OverrideConfigPath";
/// Environment variable that, when set to a non-empty value, makes the launcher pass
/// [`OVERRIDE_CONFIG_ARG`] with that value, so the client reads an absolute override-config file
/// instead of the packed `@assets@/ClientOverride.json` that a retail client does not read.
pub const OVERRIDE_CONFIG_ENV: &str = "NW_LAUNCHER_OVERRIDE_CONFIG";
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
    /// The four direct-launch variables are not all set.
    #[error(
        "set NW_LAUNCHER_PROTON, NW_LAUNCHER_GAME_EXE, NW_LAUNCHER_STEAM_COMPAT_DATA and \
         NW_LAUNCHER_STEAM_CLIENT_INSTALL: the game runs directly under Proton, never through \
         Steam's launcher"
    )]
    NoDirectLaunch,
    /// The direct-launch prefix is Steam's retail prefix of New World.
    #[error(
        "{} is Steam's retail prefix of New World; give the launcher a prefix of its own",
        .0.display()
    )]
    RetailPrefix(PathBuf),
    /// The Windows game executable is not set.
    #[error(
        "set {DIRECT_EXE_ENV} to the patched {PATCHED_EXE_NAME}: on Windows the launcher starts it \
         directly, never Steam's launcher"
    )]
    NoWindowsLaunch,
    /// The Windows game executable is not the patched copy.
    #[error("{} is not the patched {PATCHED_EXE_NAME}; start the patched copy, never the retail exe", .0.display())]
    NotPatchedExe(PathBuf),
    /// The game program (`bwrap` running Proton) could not be started.
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
/// Easy Anti-Cheat). Set by the four [`DIRECT_EXE_ENV`] family variables. The prefix must be the
/// launcher's own, never Steam's retail prefix ([`is_retail_prefix`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectLaunch {
    /// The Proton program.
    pub proton: PathBuf,
    /// The game executable to run (for example the patched `BlightedNewWorld.exe`).
    pub exe: PathBuf,
    /// `STEAM_COMPAT_DATA_PATH`: the launcher's own Proton prefix folder.
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

/// Whether `prefix` ends in `steamapps/compatdata/1063730`: Steam's retail prefix of New World.
pub fn is_retail_prefix(prefix: &Path) -> bool {
    let parts: Vec<_> = prefix.components().map(|part| part.as_os_str()).collect();
    parts.ends_with(&RETAIL_PREFIX_PARTS.map(std::ffi::OsStr::new))
}

/// `bwrap <private files> -- <proton> run <exe> <arguments>` with the Steam compatibility
/// environment, so a patched game executable runs under Proton without Steam's own launcher and
/// without Easy Anti-Cheat, and sees the private network files.
pub fn direct_command(
    launch: &DirectLaunch,
    files: &NetworkFiles,
    arguments: &[String],
) -> Command {
    let mut command = Command::new(BWRAP_PROGRAM);
    command
        .args(bwrap_arguments(files))
        .arg(&launch.proton)
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

/// The direct launch from the environment, refused when it is missing or uses the retail prefix.
pub fn checked_direct_launch(launch: Option<DirectLaunch>) -> Result<DirectLaunch, GameError> {
    let launch = launch.ok_or(GameError::NoDirectLaunch)?;
    if is_retail_prefix(&launch.compat_data) {
        return Err(GameError::RetailPrefix(launch.compat_data));
    }
    Ok(launch)
}

/// The patched client copy (`scripts/patch-client.py`). The Windows launcher starts only this,
/// never the retail `NewWorld.exe`.
pub const PATCHED_EXE_NAME: &str = "BlightedNewWorld.exe";

/// A native Windows launch: the patched game executable started directly, with no `bwrap`, no
/// Proton and no Easy Anti-Cheat. The private network view there is the launcher's loopback proxy
/// ([`crate::aws_proxy`]), not the private files of [`crate::mirror`]. Set by [`DIRECT_EXE_ENV`].
#[cfg(any(windows, test))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsLaunch {
    /// The patched game executable to run (`...\Bin64\BlightedNewWorld.exe`).
    pub exe: PathBuf,
}

/// A [`WindowsLaunch`] from [`DIRECT_EXE_ENV`], or none when it is unset.
#[cfg(any(windows, test))]
pub fn windows_launch_from_env() -> Option<WindowsLaunch> {
    Some(WindowsLaunch {
        exe: std::env::var_os(DIRECT_EXE_ENV)?.into(),
    })
}

/// The launch from the environment, refused when it is missing or is not the patched copy.
#[cfg(any(windows, test))]
pub fn checked_windows_launch(launch: Option<WindowsLaunch>) -> Result<WindowsLaunch, GameError> {
    let launch = launch.ok_or(GameError::NoWindowsLaunch)?;
    if launch.exe.file_name() != Some(std::ffi::OsStr::new(PATCHED_EXE_NAME)) {
        return Err(GameError::NotPatchedExe(launch.exe));
    }
    Ok(launch)
}

/// `BlightedNewWorld.exe <arguments>` with the Steam context, so the patched game runs directly
/// without Steam's own launcher and without Easy Anti-Cheat. Steam must run for the Steam ticket,
/// as on Linux.
#[cfg(any(windows, test))]
pub fn windows_command(launch: &WindowsLaunch, arguments: &[String]) -> Command {
    let mut command = Command::new(&launch.exe);
    command
        .args(arguments)
        .env(STEAM_APP_ID_ENV, STEAM_APP_ID)
        .env(STEAM_GAME_ID_ENV, STEAM_APP_ID);
    command
}

/// Starts the patched game on Windows and returns at once. The caller starts
/// [`crate::aws_proxy::AwsProxy`] first, so the game's AWS calls reach the server.
#[cfg(any(windows, test))]
pub fn start_game_windows(launch: &WindowsLaunch, arguments: &[String]) -> Result<(), GameError> {
    let mut command = windows_command(launch, arguments);
    let program = launch.exe.clone();
    command
        .spawn()
        .map(drop)
        .map_err(|source| GameError::Start { program, source })
}

/// Starts the game with the private network `files` and returns at once.
pub fn start_game(
    launch: &DirectLaunch,
    files: &NetworkFiles,
    arguments: &[String],
) -> Result<(), GameError> {
    let mut command = direct_command(launch, files, arguments);
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

    fn launch(compat_data: &str) -> DirectLaunch {
        DirectLaunch {
            proton: PathBuf::from("/opt/GE-Proton/proton"),
            exe: PathBuf::from("/games/New World/Bin64/BlightedNewWorld.exe"),
            compat_data: PathBuf::from(compat_data),
            client_install: PathBuf::from("/home/p/.local/share/Steam"),
        }
    }

    const OWN_PREFIX: &str = "/games/blighted-prefix";

    fn files() -> NetworkFiles {
        NetworkFiles {
            hosts: PathBuf::from("/run/x/hosts"),
            trust: PathBuf::from("/run/x/ca-certificates.crt"),
        }
    }

    #[test]
    fn direct_command_runs_proton_inside_bwrap_with_steam_env() {
        let command = direct_command(
            &launch(OWN_PREFIX),
            &files(),
            &template(&["-LoginChannelOverride", KEY]),
        );
        assert_eq!(command.get_program(), OsStr::new(BWRAP_PROGRAM));
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let sandbox: Vec<String> = bwrap_arguments(&files())
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[..sandbox.len()], sandbox[..]);
        assert_eq!(
            args[sandbox.len()..],
            [
                "/opt/GE-Proton/proton",
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
            Some(OWN_PREFIX)
        );
        assert_eq!(
            envs.get("STEAM_COMPAT_CLIENT_INSTALL_PATH")
                .map(String::as_str),
            Some("/home/p/.local/share/Steam")
        );
    }

    #[test]
    fn the_retail_prefix_and_a_missing_launch_are_refused() {
        for retail in [
            "/mnt/disk/SteamLibrary/steamapps/compatdata/1063730",
            "/home/p/.local/share/Steam/steamapps/compatdata/1063730/",
        ] {
            assert!(matches!(
                checked_direct_launch(Some(launch(retail))),
                Err(GameError::RetailPrefix(path)) if path == Path::new(retail)
            ));
        }
        for own in [
            OWN_PREFIX,
            "/mnt/disk/SteamLibrary/steamapps/compatdata/1063731",
            "/x/compatdata/1063730",
        ] {
            assert_eq!(
                checked_direct_launch(Some(launch(own))).expect("own prefix"),
                launch(own)
            );
        }
        assert!(matches!(
            checked_direct_launch(None),
            Err(GameError::NoDirectLaunch)
        ));
    }

    fn windows_launch(exe: &str) -> WindowsLaunch {
        WindowsLaunch {
            exe: PathBuf::from(exe),
        }
    }

    #[test]
    fn the_windows_command_starts_the_exe_directly_with_steam_env() {
        let launch = windows_launch("C:/games/New World/Bin64/BlightedNewWorld.exe");
        let command = windows_command(&launch, &template(&["-LoginChannelOverride", KEY]));
        assert_eq!(command.get_program(), launch.exe.as_os_str());
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["-LoginChannelOverride", KEY]);
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
            envs.get("SteamGameId").map(String::as_str),
            Some(STEAM_APP_ID)
        );
    }

    #[test]
    fn only_the_patched_exe_is_launched_on_windows() {
        assert_eq!(
            checked_windows_launch(Some(windows_launch("D:/x/Bin64/BlightedNewWorld.exe")))
                .expect("patched exe"),
            windows_launch("D:/x/Bin64/BlightedNewWorld.exe")
        );
        assert!(matches!(
            checked_windows_launch(Some(windows_launch("D:/x/Bin64/NewWorld.exe"))),
            Err(GameError::NotPatchedExe(path)) if path == Path::new("D:/x/Bin64/NewWorld.exe")
        ));
        assert!(matches!(
            checked_windows_launch(None),
            Err(GameError::NoWindowsLaunch)
        ));
    }

    #[test]
    fn the_cognito_pool_argument_is_allowed() {
        let arguments = game_arguments(
            &template(&[
                "-LoginChannelOverride",
                SESSION_PLACEHOLDER,
                "-CognitoPool",
                "b04fbfcd-942c-472d-88c0-c762c4076b2f",
            ]),
            KEY,
        )
        .expect("arguments");
        assert_eq!(arguments[2], "-CognitoPool");
    }
}
