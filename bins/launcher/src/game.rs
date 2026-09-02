// SPDX-License-Identifier: GPL-3.0-or-later
//! Starting New World through Steam (M2 spec section 6). Steam must run, because the game asks it
//! for a Steam ticket.

use std::path::{Path, PathBuf};
use std::process::Command;

use nw_protocol::launcher_api::{SESSION_PLACEHOLDER, is_forbidden_host};

/// Steam application id of New World: Aeternum.
pub const STEAM_APP_ID: &str = "1063730";
/// Steam argument that starts an application.
pub const APPLAUNCH_ARGUMENT: &str = "-applaunch";
/// The only game arguments a server may ask for (endpoint RE note).
pub const ALLOWED_ARGUMENTS: [&str; 6] = [
    "-ChannelService",
    "-LoginChannelOverride",
    "-GatewayMode",
    "-GatewayAddr",
    "-HttpGatewayAddr",
    "-AuthEndpoint",
];
/// Default Steam program.
#[cfg(windows)]
pub const DEFAULT_STEAM: &str = r"C:\Program Files (x86)\Steam\steam.exe";
/// Default Steam program (found on `PATH`).
#[cfg(not(windows))]
pub const DEFAULT_STEAM: &str = "steam";
const ARGUMENT_PREFIX: char = '-';
const URL_SCHEME_SEPARATOR: &str = "://";

/// The game could not be started.
#[derive(Debug, thiserror::Error)]
pub enum GameError {
    /// The server asked for an argument that is not allowed.
    #[error("the server asked for the game argument {0}, which is not allowed")]
    NotAllowed(String),
    /// The server's arguments do not pass the session key.
    #[error("the server's launch arguments have no session placeholder")]
    NoPlaceholder,
    /// An argument value points to an Amazon or CloudFront host.
    #[error("the server asked for the Amazon or CloudFront address {0}")]
    Forbidden(String),
    /// Steam could not be started.
    #[error("Steam could not be started from {}: {source}", .steam.display())]
    Start {
        /// The Steam program.
        steam: PathBuf,
        /// The start error.
        #[source]
        source: std::io::Error,
    },
}

fn host_of(value: &str) -> Option<&str> {
    let rest = value.split_once(URL_SCHEME_SEPARATOR)?.1;
    rest.split(['/', ':', '?']).next()
}

/// The game arguments: the server's template with the session key in place of the placeholder.
pub fn game_arguments(template: &[String], session_key: &str) -> Result<Vec<String>, GameError> {
    let mut has_placeholder = false;
    let mut arguments = Vec::with_capacity(template.len());
    for item in template {
        if item.starts_with(ARGUMENT_PREFIX) {
            if !ALLOWED_ARGUMENTS.contains(&item.as_str()) {
                return Err(GameError::NotAllowed(item.clone()));
            }
            arguments.push(item.clone());
            continue;
        }
        if let Some(host) = host_of(item).filter(|host| is_forbidden_host(host)) {
            return Err(GameError::Forbidden(host.to_owned()));
        }
        has_placeholder |= item.contains(SESSION_PLACEHOLDER);
        arguments.push(item.replace(SESSION_PLACEHOLDER, session_key));
    }
    if has_placeholder {
        Ok(arguments)
    } else {
        Err(GameError::NoPlaceholder)
    }
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

/// Starts the game and returns at once.
pub fn start_game(steam: &Path, arguments: &[String]) -> Result<(), GameError> {
    steam_command(steam, arguments)
        .spawn()
        .map(drop)
        .map_err(|source| GameError::Start {
            steam: steam.to_path_buf(),
            source,
        })
}

#[cfg(test)]
mod tests {
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
    fn missing_steam_is_reported() {
        assert!(matches!(
            start_game(Path::new("/nonexistent/steam"), &[]),
            Err(GameError::Start { .. })
        ));
    }
}
