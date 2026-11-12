// SPDX-License-Identifier: GPL-3.0-or-later
//! The launcher state apart from any window (M2 spec section 6): the saved servers, the
//! connection, the login, the game start and the status line. A call that waits on the network
//! comes back as a [`Job`]: the caller runs it off the window thread and gives its [`Event`] to
//! [`Session::apply`]. [`Session::view`] is everything the page shows.

use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant};

use nw_protocol::launcher_api::{AuthMode, DiscordPollReply, LauncherInfo, PasswordCredentials};
use serde::Serialize;

use crate::api::{ApiError, LauncherApi, discord_login_page};
use crate::aws_proxy::AwsProxy;
use crate::game::{
    PATCHED_EXE_NAME, append_override_config, game_arguments, is_retail_prefix, open_outside,
};
#[cfg(not(windows))]
use crate::game::{checked_direct_launch, start_game};
#[cfg(any(windows, test))]
use crate::game::{checked_windows_launch, start_game_windows};
#[cfg(not(windows))]
use crate::mirror::{NETWORK_DIR, write_network_files};
use crate::options::{DEFAULT_PREFIX_DIR, LaunchOptions, ProtonSwitch};
use crate::server::api_base_url;
use crate::settings::{Settings, ThemeChoice, same_server};
use crate::steam::{self, Found};
use crate::updates::GAME_RUNNING_REFUSAL;

/// Time between Discord login polls.
pub const DISCORD_POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Longest wait for a Discord login.
pub const DISCORD_POLL_LIMIT: Duration = Duration::from_secs(300);
/// Error code of an expired launcher session.
const NO_SESSION_CODE: &str = "no_session";
/// Whether the Proton options apply: the game runs under Proton on every system but Windows.
const USES_PROTON: bool = cfg!(not(windows));
/// The launcher version, shown at the bottom of the page.
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The status detail when a job ended without a result.
const CALL_STOPPED: &str = "The call stopped.";
/// Why an update cannot start while a call runs (a running Play call is about to start the game).
pub const BUSY_REFUSAL: &str = "Wait until the launcher has finished the running call";

/// Kind of a state, which picks its colour on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    /// Nothing runs and nothing is connected.
    Idle,
    /// A call runs.
    Busy,
    /// Connected, logged in, or the game started.
    Ready,
    /// The last call failed.
    Fault,
}

/// The status line: a state word and a detail line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    /// Colour of the dot.
    pub tone: Tone,
    /// The state word, lower case; the page starts it with a capital.
    pub word: &'static str,
    /// What happened, in one sentence.
    pub detail: String,
}

impl Status {
    fn new(tone: Tone, word: &'static str, detail: impl Into<String>) -> Self {
        Self {
            tone,
            word,
            detail: detail.into(),
        }
    }

    fn not_connected() -> Self {
        Self::new(
            Tone::Idle,
            "not connected",
            "Pick a saved server, or enter the server domain, for example example.org.",
        )
    }

    fn error(detail: impl Into<String>) -> Self {
        Self::new(Tone::Fault, "error", detail)
    }
}

/// What a path option must point at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathCheck {
    /// A file needed to play.
    RequiredFile,
    /// A folder needed to play.
    RequiredFolder,
    /// A file that may stay empty.
    OptionalFile,
    /// The Proton prefix: empty gives the default, a new folder is made by Proton, and Steam's
    /// retail prefix is refused.
    Prefix,
}

/// The state shown after a path field: its tone and word.
pub fn path_state(value: &str, check: PathCheck) -> (Tone, &'static str) {
    let path = Path::new(value);
    match check {
        PathCheck::Prefix if value.is_empty() => (Tone::Idle, "default"),
        PathCheck::Prefix if is_retail_prefix(path) => (Tone::Fault, "retail"),
        PathCheck::Prefix if path.is_dir() => (Tone::Ready, "found"),
        PathCheck::Prefix => (Tone::Idle, "new"),
        PathCheck::OptionalFile if value.is_empty() => (Tone::Idle, "not set"),
        _ if value.is_empty() => (Tone::Fault, "not set"),
        PathCheck::RequiredFolder if path.is_dir() => (Tone::Ready, "found"),
        PathCheck::RequiredFile | PathCheck::OptionalFile if path.is_file() => {
            (Tone::Ready, "found")
        }
        _ => (Tone::Fault, "missing"),
    }
}

/// The name of a login mode.
pub fn auth_mode_name(mode: AuthMode) -> &'static str {
    match mode {
        AuthMode::Password => "Username and password",
        AuthMode::Discord => "Discord",
    }
}

/// Result of a [`Job`].
pub enum Event {
    /// Connected to the server typed as the string.
    Connected(String, LauncherApi, LauncherInfo),
    /// Logged in with this launcher token.
    LoggedIn(String),
    /// The game process started.
    GameStarted(Child),
    /// The session ended.
    LoggedOut,
    /// The server no longer knows the launcher token.
    SessionExpired,
    /// The call failed, with the text to show.
    Failed(String),
}

/// A call that waits on the network; run it off the window thread.
pub type Job = Box<dyn FnOnce() -> Event + Send + 'static>;

/// Runs `job`. A job that panics gives a failure, so the launcher is not left busy.
pub fn run_guarded(job: Job) -> Event {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
        .unwrap_or_else(|_| Event::Failed(CALL_STOPPED.to_owned()))
}

fn failure(error: &ApiError) -> Event {
    match error {
        ApiError::Server { code, .. } if code == NO_SESSION_CODE => Event::SessionExpired,
        other => Event::Failed(other.to_string()),
    }
}

/// The job that gets a game session key and starts the patched game directly on Windows (the
/// loopback proxy is started first, by [`Session::play`]). Compiled on Linux too, so tests
/// type-check it, but used only on Windows.
#[cfg(any(windows, test))]
fn windows_play_job(
    api: LauncherApi,
    info: LauncherInfo,
    token: String,
    options: LaunchOptions,
) -> impl FnOnce() -> Event + Send + 'static {
    move || {
        let launch = match checked_windows_launch(options.windows_launch()) {
            Ok(launch) => launch,
            Err(error) => return Event::Failed(error.to_string()),
        };
        let environment = match options.environment(false) {
            Ok(environment) => environment,
            Err(error) => return Event::Failed(error.to_string()),
        };
        let key = match api.play(&token) {
            Ok(reply) => reply.game_session_key,
            Err(error) => return failure(&error),
        };
        match game_arguments(&info.launch_arguments, &key).and_then(|arguments| {
            start_game_windows(
                &launch,
                &append_override_config(arguments, options.override_config()),
                &environment,
            )
        }) {
            Ok(child) => Event::GameStarted(child),
            Err(error) => Event::Failed(error.to_string()),
        }
    }
}

/// The job that writes the private network files into `network_dir`, gets a game session key
/// and starts the game under Proton inside `bwrap` (the loopback proxy is started first, by
/// [`Session::play`]). `settings_dir` holds the default prefix.
#[cfg(not(windows))]
fn linux_play_job(
    api: LauncherApi,
    info: LauncherInfo,
    token: String,
    options: LaunchOptions,
    network_dir: PathBuf,
    settings_dir: PathBuf,
) -> impl FnOnce() -> Event + Send + 'static {
    move || {
        let launch = match checked_direct_launch(options.direct_launch(&settings_dir)) {
            Ok(launch) => launch,
            Err(error) => return Event::Failed(error.to_string()),
        };
        let environment = match options.environment(true) {
            Ok(environment) => environment,
            Err(error) => return Event::Failed(error.to_string()),
        };
        let files = match write_network_files(&network_dir, &info.aws_mirror, api.host_name()) {
            Ok(files) => files,
            Err(error) => return Event::Failed(error.to_string()),
        };
        let key = match api.play(&token) {
            Ok(reply) => reply.game_session_key,
            Err(error) => return failure(&error),
        };
        match game_arguments(&info.launch_arguments, &key).and_then(|arguments| {
            start_game(
                &launch,
                &files,
                &append_override_config(arguments, options.override_config()),
                &environment,
            )
        }) {
            Ok(child) => Event::GameStarted(child),
            Err(error) => Event::Failed(error.to_string()),
        }
    }
}

fn wait_for_discord(api: &LauncherApi, state: &str) -> Event {
    let started = Instant::now();
    while started.elapsed() < DISCORD_POLL_LIMIT {
        match api.discord_poll(state) {
            Ok(DiscordPollReply::Ready(session)) => return Event::LoggedIn(session.launcher_token),
            Ok(DiscordPollReply::Pending) => std::thread::sleep(DISCORD_POLL_INTERVAL),
            Err(error) => return failure(&error),
        }
    }
    Event::Failed("Discord login timed out".to_owned())
}

fn folder_of(path: &Path) -> PathBuf {
    path.parent().map(Path::to_path_buf).unwrap_or_default()
}

/// One saved server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerView {
    /// The address as typed.
    pub server: String,
    /// The name the player sees.
    pub name: String,
    /// Whether it is the selected server.
    pub selected: bool,
}

/// The facts of the connected server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FactsView {
    /// The launcher API host and port.
    pub address: String,
    /// The login mode, in words.
    pub login: &'static str,
    /// "Open" or "Closed" for a password server; none for a Discord server.
    pub sign_up: Option<&'static str>,
}

/// One path option with its state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathView {
    /// The [`LaunchOptions`] field it edits.
    pub field: &'static str,
    /// The label before the field.
    pub label: &'static str,
    /// The hint in an empty field.
    pub hint: String,
    /// The state colour and word ([`path_state`]).
    pub tone: Tone,
    /// The state word.
    pub word: &'static str,
}

/// One Proton build found on this PC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtonView {
    /// The build folder name.
    pub name: String,
    /// The `proton` program.
    pub program: String,
}

/// One Proton switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchView {
    /// The value in [`LaunchOptions::proton_switches`].
    pub key: ProtonSwitch,
    /// The text next to the check box.
    pub label: &'static str,
    /// Whether it is on.
    pub on: bool,
}

/// Everything the page shows, apart from the news and the update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    /// The status line.
    pub status: Status,
    /// Whether a call runs; the page then turns its buttons off.
    pub busy: bool,
    /// The saved servers.
    pub servers: Vec<ServerView>,
    /// The address of the server in use, as typed.
    pub server_input: String,
    /// The connected server's own name.
    pub server_name: Option<String>,
    /// The saved name of the connected, selected server ("Saved as").
    pub saved_name: Option<String>,
    /// The connected server's facts.
    pub facts: Option<FactsView>,
    /// The connected server's login mode.
    pub auth_mode: Option<AuthMode>,
    /// Whether the connected server lets a player make an account.
    pub allow_registration: bool,
    /// Whether the selected server has a launcher token.
    pub logged_in: bool,
    /// The launch options.
    pub options: LaunchOptions,
    /// The path options in page order, with their states.
    pub paths: Vec<PathView>,
    /// The Proton builds found on this PC.
    pub proton_builds: Vec<ProtonView>,
    /// The Proton switches.
    pub proton_switches: Vec<SwitchView>,
    /// Whether the Proton options apply (not on Windows).
    pub uses_proton: bool,
    /// The page theme.
    pub theme: ThemeChoice,
    /// Where the settings are saved.
    pub settings_file: String,
    /// Whether the game the launcher started still runs.
    pub game_running: bool,
    /// Whether a launcher update downloads and installs; Play is off meanwhile.
    pub updating: bool,
    /// The launcher version.
    pub version: &'static str,
}

/// The launcher state.
pub struct Session {
    settings_path: PathBuf,
    settings: Settings,
    /// The address of the server in use, as typed.
    server_input: String,
    connection: Option<(LauncherApi, LauncherInfo)>,
    status: Status,
    busy: bool,
    /// What the last scan of this PC found, for the Proton pick list.
    found: Found,
    /// The loopback proxy that carries the game's AWS calls to the server. It must outlive the
    /// game, so the session keeps it; dropping it stops the proxy.
    proxy: Option<AwsProxy>,
    game_running: bool,
    /// A launcher update downloads and installs; the launcher restarts after it.
    updating: bool,
}

impl Session {
    /// The state from saved settings.
    pub fn new(settings_path: PathBuf, settings: Settings, found: Found) -> Self {
        let server_input = settings
            .selected_profile()
            .map(|profile| profile.server.clone())
            .unwrap_or_default();
        Self {
            settings_path,
            settings,
            server_input,
            connection: None,
            status: Status::not_connected(),
            busy: false,
            found,
            proxy: None,
            game_running: false,
            updating: false,
        }
    }

    /// The first job: a connection to the selected saved server, when there is one.
    pub fn start_job(&mut self) -> Option<Job> {
        let server = self.server_input.clone();
        (!server.is_empty())
            .then(|| self.connect(&server))
            .flatten()
    }

    /// The status line now.
    pub fn status(&self) -> &Status {
        &self.status
    }

    /// The folder of the settings file, which also holds the default prefix and the news cache.
    pub fn settings_dir(&self) -> PathBuf {
        folder_of(&self.settings_path)
    }

    /// The API of the connected server.
    pub fn api(&self) -> Option<&LauncherApi> {
        self.connection.as_ref().map(|(api, _)| api)
    }

    fn save(&mut self) {
        if let Err(error) = self.settings.save(&self.settings_path) {
            self.status = Status::error(error.to_string());
        }
    }

    /// Marks a call as running with `status`, and boxes its job.
    fn begin(&mut self, status: Status, job: impl FnOnce() -> Event + Send + 'static) -> Job {
        self.status = status;
        self.busy = true;
        Box::new(job)
    }

    /// The connection and the token of the selected server, when both are there.
    fn logged_in_connection(&self) -> Option<(LauncherApi, LauncherInfo, String)> {
        let (api, info) = self.connection.clone()?;
        let token = self.settings.token()?.to_owned();
        Some((api, info, token))
    }

    /// Connects to the server typed as `input`. A bad address is an error and starts no call.
    pub fn connect(&mut self, input: &str) -> Option<Job> {
        if self.busy {
            return None;
        }
        self.server_input = input.trim().to_owned();
        let base = match api_base_url(&self.server_input) {
            Ok(base) => base,
            Err(error) => {
                self.status = Status::error(error.to_string());
                return None;
            }
        };
        self.connection = None;
        let server = self.server_input.clone();
        let status = Status::new(Tone::Busy, "connecting", format!("Connecting to {base}."));
        Some(self.begin(status, move || {
            match LauncherApi::new(base).and_then(|api| api.info().map(|info| (api, info))) {
                Ok((api, info)) => Event::Connected(server, api, info),
                Err(error) => Event::Failed(error.to_string()),
            }
        }))
    }

    /// Logs in, or makes the account first when `register` is set.
    pub fn log_in(&mut self, username: String, password: String, register: bool) -> Option<Job> {
        if self.busy {
            return None;
        }
        let (api, _) = self.connection.clone()?;
        let credentials = PasswordCredentials { username, password };
        let detail = if register {
            "Making the account."
        } else {
            "Checking the username and password."
        };
        let status = Status::new(Tone::Busy, "logging in", detail);
        Some(self.begin(status, move || {
            let result = if register {
                api.register(&credentials)
            } else {
                api.login(&credentials)
            };
            match result {
                Ok(session) => Event::LoggedIn(session.launcher_token),
                Err(error) => Event::Failed(error.to_string()),
            }
        }))
    }

    /// Opens Discord's login page in the browser and waits for the login.
    pub fn discord_log_in(&mut self) -> Option<Job> {
        if self.busy {
            return None;
        }
        let (api, _) = self.connection.clone()?;
        let status = Status::new(
            Tone::Busy,
            "logging in",
            "Finish the login in your browser.",
        );
        Some(self.begin(status, move || {
            let start = match api.discord_start() {
                Ok(start) => start,
                Err(error) => return Event::Failed(error.to_string()),
            };
            let page = match discord_login_page(&start.authorize_url) {
                Ok(page) => page,
                Err(error) => return Event::Failed(error.to_string()),
            };
            if let Err(error) = open_outside(page.as_str()) {
                return Event::Failed(format!("cannot open the browser: {error}"));
            }
            wait_for_discord(&api, &start.state)
        }))
    }

    /// Starts the loopback proxy, then returns the job that checks the launch, gets a game
    /// session key and starts the game. Nothing starts while the game runs.
    pub fn play(&mut self) -> Option<Job> {
        if self.busy || self.game_running || self.updating {
            return None;
        }
        let (api, info, token) = self.logged_in_connection()?;
        // Start the loopback proxy before the game and keep it for the session, so the game's
        // AWS calls reach the server for its whole run. A new play replaces it.
        self.proxy = None;
        match AwsProxy::start(&info.aws_mirror, api.host_name(), api.port()) {
            Ok(proxy) => self.proxy = Some(proxy),
            Err(error) => {
                self.status = Status::error(error.to_string());
                return None;
            }
        }
        let options = self.settings.options.clone();
        let status = Status::new(
            Tone::Busy,
            "starting",
            "Getting a game session and starting the game.",
        );
        #[cfg(not(windows))]
        let job = linux_play_job(
            api,
            info,
            token,
            options,
            self.settings_path.with_file_name(NETWORK_DIR),
            self.settings_dir(),
        );
        #[cfg(windows)]
        let job = windows_play_job(api, info, token, options);
        Some(self.begin(status, job))
    }

    /// Ends the launcher session on the server.
    pub fn log_out(&mut self) -> Option<Job> {
        if self.busy {
            return None;
        }
        let (api, _, token) = self.logged_in_connection()?;
        let status = Status::new(Tone::Busy, "logging out", "Ending the session.");
        Some(self.begin(status, move || match api.logout(&token) {
            Ok(()) => Event::LoggedOut,
            Err(error) => failure(&error),
        }))
    }

    /// Saves `name` as the selected server's name; an empty name keeps the old one.
    pub fn rename(&mut self, name: &str) {
        let Some(server) = self.settings.selected.clone() else {
            return;
        };
        self.settings.rename(&server, name);
        self.save();
    }

    /// Forgets the selected server and its login, and leaves it.
    pub fn remove(&mut self) {
        if self.busy {
            return;
        }
        let Some(server) = self.settings.selected.clone() else {
            return;
        };
        self.settings.remove(&server);
        self.connection = None;
        self.server_input.clear();
        self.status = Status::not_connected();
        self.save();
    }

    /// Saves new launch options.
    pub fn set_options(&mut self, options: LaunchOptions) {
        self.settings.options = options;
        self.save();
    }

    /// Scans this PC again and fills the empty game and Steam paths.
    pub fn find_paths(&mut self) {
        self.found = steam::scan(|name| std::env::var(name).ok());
        self.settings.options.fill_from_scan(&self.found);
        self.save();
    }

    /// Saves the page theme.
    pub fn set_theme(&mut self, theme: ThemeChoice) {
        if self.settings.theme != theme {
            self.settings.theme = theme;
            self.save();
        }
    }

    /// Takes the result of a job. Returns the game process when the game started, for the
    /// caller to watch ([`Self::game_ended`]).
    pub fn apply(&mut self, event: Event) -> Option<Child> {
        self.busy = false;
        match event {
            Event::Connected(server, api, info) => {
                self.status = Status::new(
                    Tone::Ready,
                    "connected",
                    format!("Connected to {}.", info.server_name),
                );
                self.settings.connected(&server, &info.server_name);
                self.connection = Some((api, info));
                self.save();
                None
            }
            Event::LoggedIn(token) => {
                self.status =
                    Status::new(Tone::Ready, "logged in", "Press Play to start the game.");
                self.settings.set_token(Some(token));
                self.save();
                None
            }
            Event::GameStarted(child) => {
                self.status = Status::new(Tone::Ready, "game started", "The game process started.");
                self.game_running = true;
                Some(child)
            }
            Event::LoggedOut => {
                self.status = Status::new(Tone::Idle, "logged out", "Log in to play.");
                self.settings.set_token(None);
                self.save();
                None
            }
            Event::SessionExpired => {
                self.status = Status::new(
                    Tone::Fault,
                    "login expired",
                    "Your login expired. Log in again.",
                );
                self.settings.set_token(None);
                self.save();
                None
            }
            Event::Failed(message) => {
                self.status = Status::error(message);
                None
            }
        }
    }

    /// The game process ended. The status of a running call stays.
    pub fn game_ended(&mut self) {
        self.game_running = false;
        if !self.busy {
            self.status = Status::new(
                Tone::Ready,
                "game closed",
                "Press Play to start the game again.",
            );
        }
    }

    /// Marks a launcher update as started, so no game starts until the launcher restarts.
    ///
    /// # Errors
    /// The text to show when the update cannot start now: the game runs, or a call runs.
    pub fn begin_update(&mut self) -> Result<(), &'static str> {
        if self.game_running {
            return Err(GAME_RUNNING_REFUSAL);
        }
        if self.busy {
            return Err(BUSY_REFUSAL);
        }
        self.updating = true;
        Ok(())
    }

    /// The update failed; Play works again.
    pub fn end_update(&mut self) {
        self.updating = false;
    }

    fn path_views(&self) -> Vec<PathView> {
        let options = &self.settings.options;
        let default_prefix = self
            .settings_dir()
            .join(DEFAULT_PREFIX_DIR)
            .display()
            .to_string();
        let mut rows = vec![(
            "game_exe",
            "Game",
            &options.game_exe,
            PATCHED_EXE_NAME.to_owned(),
            PathCheck::RequiredFile,
        )];
        if USES_PROTON {
            rows.push((
                "proton",
                "Proton",
                &options.proton,
                "proton".to_owned(),
                PathCheck::RequiredFile,
            ));
            rows.push((
                "prefix",
                "Prefix",
                &options.prefix,
                default_prefix,
                PathCheck::Prefix,
            ));
            rows.push((
                "steam_install",
                "Steam",
                &options.steam_install,
                "Steam folder".to_owned(),
                PathCheck::RequiredFolder,
            ));
        }
        rows.push((
            "override_config",
            "Override",
            &options.override_config,
            "ClientOverride.json (optional)".to_owned(),
            PathCheck::OptionalFile,
        ));
        rows.into_iter()
            .map(|(field, label, value, hint, check)| {
                let (tone, word) = path_state(value, check);
                PathView {
                    field,
                    label,
                    hint,
                    tone,
                    word,
                }
            })
            .collect()
    }

    /// Everything the page shows now.
    pub fn view(&self) -> View {
        let selected = self.settings.selected.as_deref();
        let servers = self
            .settings
            .profiles
            .iter()
            .map(|profile| ServerView {
                server: profile.server.clone(),
                name: profile.name.clone(),
                selected: selected.is_some_and(|server| same_server(server, &profile.server)),
            })
            .collect();
        let info = self.connection.as_ref().map(|(_, info)| info);
        let facts = self.connection.as_ref().map(|(api, info)| FactsView {
            address: api.host().to_owned(),
            login: auth_mode_name(info.auth_mode),
            sign_up: (info.auth_mode == AuthMode::Password).then_some(if info.allow_registration {
                "Open"
            } else {
                "Closed"
            }),
        });
        let saved_name = self
            .connection
            .as_ref()
            .and(self.settings.selected_profile())
            .map(|profile| profile.name.clone());
        View {
            status: self.status.clone(),
            busy: self.busy,
            servers,
            server_input: self.server_input.clone(),
            server_name: info.map(|info| info.server_name.clone()),
            saved_name,
            facts,
            auth_mode: info.map(|info| info.auth_mode),
            allow_registration: info.is_some_and(|info| info.allow_registration),
            logged_in: self.settings.token().is_some(),
            options: self.settings.options.clone(),
            paths: self.path_views(),
            proton_builds: self
                .found
                .protons
                .iter()
                .map(|build| ProtonView {
                    name: build.name.clone(),
                    program: build.program.display().to_string(),
                })
                .collect(),
            proton_switches: ProtonSwitch::ALL
                .iter()
                .map(|&key| SwitchView {
                    key,
                    label: key.label(),
                    on: self.settings.options.proton_switches.contains(&key),
                })
                .collect(),
            uses_proton: USES_PROTON,
            theme: self.settings.theme,
            settings_file: self.settings_path.display().to_string(),
            game_running: self.game_running,
            updating: self.updating,
            version: VERSION,
        }
    }
}

#[cfg(test)]
mod tests {
    use nw_protocol::aws_mirror::{AwsMirror, CLIENT_PING_PORT};

    use super::*;

    fn session() -> (tempfile::TempDir, Session) {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("settings.json");
        let session = Session::new(path, Settings::default(), Found::default());
        (dir, session)
    }

    fn api() -> LauncherApi {
        LauncherApi::new("https://login.example.org".to_owned()).expect("api")
    }

    fn info() -> LauncherInfo {
        LauncherInfo {
            server_name: "Test world".to_owned(),
            auth_mode: AuthMode::Password,
            allow_registration: true,
            launch_arguments: Vec::new(),
            aws_mirror: AwsMirror {
                hosts: Vec::new(),
                certificate_pem: String::new(),
                ping_port: CLIENT_PING_PORT,
            },
        }
    }

    /// A process that ends at once.
    fn short_process() -> Child {
        #[cfg(windows)]
        let mut command = std::process::Command::new("cmd");
        #[cfg(windows)]
        command.args(["/C", "exit"]);
        #[cfg(not(windows))]
        let mut command = std::process::Command::new("true");
        command.spawn().expect("process")
    }

    #[test]
    fn the_windows_play_job_is_a_sendable_worker() {
        // Building the job (without running it) type-checks the whole Windows start path on Linux;
        // a job must be `Send + 'static`.
        fn assert_send_static<T: Send + 'static>(_: &T) {}
        let job = windows_play_job(api(), info(), "token".to_owned(), LaunchOptions::default());
        assert_send_static(&job);
    }

    #[test]
    fn a_new_launcher_with_no_server_is_not_connected() {
        let (_dir, mut session) = session();
        assert_eq!(session.status(), &Status::not_connected());
        assert_eq!(session.status().tone, Tone::Idle);
        assert!(session.start_job().is_none());
    }

    #[test]
    fn a_bad_address_is_an_error_and_starts_no_call() {
        let (_dir, mut session) = session();
        assert!(session.connect("localhost").is_none());
        assert_eq!(session.status().tone, Tone::Fault);
        assert!(!session.view().busy);
    }

    #[test]
    fn a_second_call_while_busy_starts_nothing() {
        let (_dir, mut session) = session();
        assert!(session.connect("example.org").is_some());
        assert!(session.view().busy);
        assert_eq!(session.status().word, "connecting");
        assert!(session.connect("example.org").is_none());
        session.apply(Event::Failed("server unreachable".to_owned()));
        assert!(!session.view().busy);
        assert!(session.connect("example.org").is_some());
    }

    #[test]
    fn events_set_the_state_word_and_the_saved_settings() {
        let (_dir, mut session) = session();
        session.apply(Event::Connected("example.org".to_owned(), api(), info()));
        assert_eq!(
            (session.status().tone, session.status().word),
            (Tone::Ready, "connected")
        );
        assert_eq!(session.status().detail, "Connected to Test world.");
        session.apply(Event::LoggedIn("token".to_owned()));
        assert_eq!(session.status().word, "logged in");
        let saved = Settings::load(&session.settings_path).expect("load");
        let profile = saved.selected_profile().expect("the server is saved");
        assert_eq!(
            (profile.name.as_str(), profile.server.as_str()),
            ("Test world", "example.org"),
            "saved under the server's own name"
        );
        assert_eq!(saved.token(), Some("token"));
        assert_eq!(session.view().saved_name.as_deref(), Some("Test world"));
        assert!(session.view().logged_in);
        session.apply(Event::SessionExpired);
        assert_eq!(
            (session.status().tone, session.status().word),
            (Tone::Fault, "login expired")
        );
        assert!(!session.view().logged_in);
        session.apply(Event::Failed("server unreachable".to_owned()));
        assert_eq!(session.status(), &Status::error("server unreachable"));
    }

    #[test]
    fn a_saved_server_is_picked_renamed_and_removed() {
        let (_dir, mut session) = session();
        session.apply(Event::Connected("example.org".to_owned(), api(), info()));
        session.rename(" ");
        assert_eq!(
            session.view().saved_name.as_deref(),
            Some("Test world"),
            "an empty name keeps the old one"
        );
        session.rename("Main");
        let saved = Settings::load(&session.settings_path).expect("load");
        assert_eq!(saved.profiles[0].name, "Main");
        assert!(session.connect("example.org").is_some());
        assert_eq!(session.view().server_input, "example.org");
        session.apply(Event::Failed("stopped".to_owned()));
        session.remove();
        assert_eq!(session.status(), &Status::not_connected());
        let view = session.view();
        assert!(view.facts.is_none() && view.server_input.is_empty());
        let saved = Settings::load(&session.settings_path).expect("load");
        assert!(saved.profiles.is_empty() && saved.selected.is_none());
    }

    #[test]
    fn the_launcher_starts_on_the_selected_saved_server() {
        let dir = tempfile::tempdir().expect("dir");
        let mut settings = Settings::default();
        settings.connected("example.org", "Test world");
        settings.connected("local.example.org", "Local world");
        settings.connected("example.org", "Test world");
        let mut session =
            Session::new(dir.path().join("settings.json"), settings, Found::default());
        assert_eq!(session.view().server_input, "example.org");
        assert!(session.start_job().is_some());
        assert_eq!(session.status().word, "connecting");
        let servers = session.view().servers;
        assert_eq!(servers.len(), 2);
        assert!(servers[0].selected && !servers[1].selected);
    }

    #[test]
    fn game_started_returns_the_child_and_marks_the_game_running() {
        let (_dir, mut session) = session();
        let mut child = session
            .apply(Event::GameStarted(short_process()))
            .expect("the child to watch");
        assert!(session.view().game_running);
        assert!(
            session.play().is_none(),
            "no second start while the game runs"
        );
        child.wait().expect("wait");
        session.game_ended();
        assert!(!session.view().game_running);
        assert_eq!(session.status().word, "game closed");
    }

    #[test]
    fn play_and_log_out_need_a_login() {
        let (_dir, mut session) = session();
        assert!(session.play().is_none() && session.log_out().is_none());
        session.apply(Event::Connected("example.org".to_owned(), api(), info()));
        assert!(session.play().is_none() && session.log_out().is_none());
        assert!(
            session
                .log_in("a".to_owned(), "b".to_owned(), false)
                .is_some()
        );
    }

    #[test]
    fn the_view_serializes_in_camel_case() {
        let (_dir, session) = session();
        let json = serde_json::to_value(session.view()).expect("json");
        for key in [
            "gameRunning",
            "usesProton",
            "settingsFile",
            "serverInput",
            "protonSwitches",
        ] {
            assert!(json.get(key).is_some(), "{key}");
        }
        assert_eq!(json["status"]["tone"], "idle");
        assert_eq!(json["theme"], "night");
        assert_eq!(json["protonSwitches"][0]["key"], "log");
        assert_eq!(json["version"], VERSION);
    }

    #[test]
    fn path_states_name_what_is_wrong() {
        let dir = tempfile::tempdir().expect("dir");
        let folder = dir.path().display().to_string();
        let file = dir.path().join("BlightedNewWorld.exe");
        std::fs::write(&file, "").expect("file");
        let file = file.display().to_string();
        let missing = dir.path().join("none").display().to_string();
        let cases = [
            ("", PathCheck::RequiredFile, (Tone::Fault, "not set")),
            ("", PathCheck::OptionalFile, (Tone::Idle, "not set")),
            ("", PathCheck::Prefix, (Tone::Idle, "default")),
            (
                file.as_str(),
                PathCheck::RequiredFile,
                (Tone::Ready, "found"),
            ),
            (
                file.as_str(),
                PathCheck::RequiredFolder,
                (Tone::Fault, "missing"),
            ),
            (
                folder.as_str(),
                PathCheck::RequiredFolder,
                (Tone::Ready, "found"),
            ),
            (
                missing.as_str(),
                PathCheck::OptionalFile,
                (Tone::Fault, "missing"),
            ),
            (folder.as_str(), PathCheck::Prefix, (Tone::Ready, "found")),
            (missing.as_str(), PathCheck::Prefix, (Tone::Idle, "new")),
            (
                "/x/steamapps/compatdata/1063730",
                PathCheck::Prefix,
                (Tone::Fault, "retail"),
            ),
        ];
        for (value, check, state) in cases {
            assert_eq!(path_state(value, check), state, "{value:?} {check:?}");
        }
    }

    #[test]
    fn login_modes_have_plain_names() {
        assert_eq!(auth_mode_name(AuthMode::Discord), "Discord");
        assert_eq!(auth_mode_name(AuthMode::Password), "Username and password");
    }

    #[test]
    fn an_update_waits_for_the_game_and_stops_play() {
        let (_dir, mut session) = session();
        let mut child = session
            .apply(Event::GameStarted(short_process()))
            .expect("the child to watch");
        assert_eq!(session.begin_update(), Err(GAME_RUNNING_REFUSAL));
        child.wait().expect("wait");
        session.game_ended();
        assert!(session.connect("example.org").is_some());
        assert_eq!(session.begin_update(), Err(BUSY_REFUSAL));
        session.apply(Event::Connected("example.org".to_owned(), api(), info()));
        session.apply(Event::LoggedIn("token".to_owned()));
        assert_eq!(session.begin_update(), Ok(()));
        assert!(session.view().updating);
        assert!(
            session.play().is_none(),
            "no game starts while the update installs"
        );
        session.end_update();
        assert!(!session.view().updating);
    }

    #[test]
    fn a_job_that_panics_ends_as_a_failure() {
        let job: Job = Box::new(|| panic!("broken job"));
        assert!(matches!(run_guarded(job), Event::Failed(text) if text == CALL_STOPPED));
        let job: Job = Box::new(|| Event::LoggedOut);
        assert!(matches!(run_guarded(job), Event::LoggedOut));
    }

    #[test]
    fn the_end_of_the_game_keeps_the_status_of_a_running_call() {
        let (_dir, mut session) = session();
        let mut child = session
            .apply(Event::GameStarted(short_process()))
            .expect("the child to watch");
        child.wait().expect("wait");
        assert!(session.connect("example.org").is_some());
        session.game_ended();
        assert_eq!(session.status().word, "connecting");
    }
}
