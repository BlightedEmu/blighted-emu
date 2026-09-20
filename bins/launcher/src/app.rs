// SPDX-License-Identifier: GPL-3.0-or-later
//! The launcher window (M2 spec section 6). Network calls run on a worker thread; the window
//! reads their results each frame.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use eframe::egui;
use nw_protocol::launcher_api::{AuthMode, DiscordPollReply, LauncherInfo, PasswordCredentials};

use crate::api::{ApiError, LauncherApi, discord_login_page};
use crate::game::{
    DEFAULT_STEAM, append_override_config, game_arguments, start_game, write_client_config,
};
use crate::server::api_base_url;
use crate::settings::Settings;

/// Time between Discord login polls.
pub const DISCORD_POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Longest wait for a Discord login.
pub const DISCORD_POLL_LIMIT: Duration = Duration::from_secs(300);
/// Repaint interval while a call runs.
const BUSY_REPAINT: Duration = Duration::from_millis(100);
/// Error code of an expired launcher session.
const NO_SESSION_CODE: &str = "no_session";

/// Result of a worker job.
enum Event {
    Connected(String, LauncherApi, LauncherInfo),
    LoggedIn(String),
    GameStarted,
    LoggedOut,
    SessionExpired,
    Failed(String),
}

/// The launcher window state.
pub struct LauncherApp {
    settings_path: PathBuf,
    settings: Settings,
    server_input: String,
    username: String,
    password: String,
    connection: Option<(LauncherApi, LauncherInfo)>,
    status: String,
    job: Option<Receiver<Event>>,
}

fn run_job(job: impl FnOnce() -> Event + Send + 'static) -> Receiver<Event> {
    let (sender, receiver) = channel();
    std::thread::spawn(move || {
        // The window may have closed; then nobody reads the result.
        let _ = sender.send(job());
    });
    receiver
}

fn failure(error: &ApiError) -> Event {
    match error {
        ApiError::Server { code, .. } if code == NO_SESSION_CODE => Event::SessionExpired,
        other => Event::Failed(other.to_string()),
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

impl LauncherApp {
    /// The window state from saved settings; connects at once when a server is saved.
    pub fn new(settings_path: PathBuf, settings: Settings) -> Self {
        let mut app = Self {
            server_input: settings.server.clone(),
            settings_path,
            settings,
            username: String::new(),
            password: String::new(),
            connection: None,
            status: String::new(),
            job: None,
        };
        if !app.server_input.is_empty() {
            app.connect();
        }
        app
    }

    fn save(&mut self) {
        if let Err(error) = self.settings.save(&self.settings_path) {
            self.status = error.to_string();
        }
    }

    fn connect(&mut self) {
        let base = match api_base_url(&self.server_input) {
            Ok(base) => base,
            Err(error) => {
                self.status = error.to_string();
                return;
            }
        };
        self.status = format!("Connecting to {base} ...");
        self.connection = None;
        let server = self.server_input.clone();
        self.job = Some(run_job(move || {
            match LauncherApi::new(base).and_then(|api| api.info().map(|info| (api, info))) {
                Ok((api, info)) => Event::Connected(server, api, info),
                Err(error) => Event::Failed(error.to_string()),
            }
        }));
    }

    fn password_login(&mut self, api: LauncherApi, register: bool) {
        let credentials = PasswordCredentials {
            username: self.username.clone(),
            password: std::mem::take(&mut self.password),
        };
        self.status = "Logging in ...".to_owned();
        self.job = Some(run_job(move || {
            let result = if register {
                api.register(&credentials)
            } else {
                api.login(&credentials)
            };
            match result {
                Ok(session) => Event::LoggedIn(session.launcher_token),
                Err(error) => Event::Failed(error.to_string()),
            }
        }));
    }

    fn discord_login(&mut self, api: LauncherApi) {
        self.status = "Finish the login in your browser ...".to_owned();
        self.job = Some(run_job(move || {
            let start = match api.discord_start() {
                Ok(start) => start,
                Err(error) => return Event::Failed(error.to_string()),
            };
            let page = match discord_login_page(&start.authorize_url) {
                Ok(page) => page,
                Err(error) => return Event::Failed(error.to_string()),
            };
            if let Err(error) = open::that(page.as_str()) {
                return Event::Failed(format!("cannot open the browser: {error}"));
            }
            wait_for_discord(&api, &start.state)
        }));
    }

    fn play(&mut self, api: LauncherApi, info: LauncherInfo, token: String) {
        let steam = self
            .settings
            .steam_path
            .clone()
            .unwrap_or_else(|| PathBuf::from(DEFAULT_STEAM));
        // The game's override-config file goes next to the launcher settings.
        let config_dir = self
            .settings_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_default();
        self.status = "Starting the game ...".to_owned();
        self.job = Some(run_job(move || {
            let key = match api.play(&token) {
                Ok(reply) => reply.game_session_key,
                Err(error) => return failure(&error),
            };
            let started = game_arguments(&info.launch_arguments, &key).and_then(|arguments| {
                let written = write_client_config(&info.client_config, &config_dir)?;
                start_game(
                    &steam,
                    &append_override_config(arguments, written.as_deref()),
                )
            });
            match started {
                Ok(()) => Event::GameStarted,
                Err(error) => Event::Failed(error.to_string()),
            }
        }));
    }

    fn logout(&mut self, api: LauncherApi, token: String) {
        self.job = Some(run_job(move || match api.logout(&token) {
            Ok(()) => Event::LoggedOut,
            Err(error) => failure(&error),
        }));
    }

    fn apply(&mut self, event: Event) {
        match event {
            Event::Connected(server, api, info) => {
                self.status = format!("Connected to {}", info.server_name);
                self.connection = Some((api, info));
                self.settings.set_server(&server);
                self.save();
            }
            Event::LoggedIn(token) => {
                self.status = "Logged in".to_owned();
                self.settings.launcher_token = Some(token);
                self.save();
            }
            Event::GameStarted => self.status = "Game started through Steam".to_owned(),
            Event::LoggedOut => {
                self.status = "Logged out".to_owned();
                self.settings.launcher_token = None;
                self.save();
            }
            Event::SessionExpired => {
                self.status = "Your login expired; log in again".to_owned();
                self.settings.launcher_token = None;
                self.save();
            }
            Event::Failed(message) => self.status = message,
        }
    }

    fn poll_job(&mut self, ctx: &egui::Context) {
        let Some(receiver) = &self.job else {
            return;
        };
        match receiver.try_recv() {
            Ok(event) => {
                self.job = None;
                self.apply(event);
            }
            Err(TryRecvError::Empty) => ctx.request_repaint_after(BUSY_REPAINT),
            Err(TryRecvError::Disconnected) => {
                self.job = None;
                self.status = "The call stopped".to_owned();
            }
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        ui.heading("Blighted New World");
        let busy = self.job.is_some();
        ui.add_enabled_ui(!busy, |ui| {
            ui.horizontal(|ui| {
                ui.label("Server");
                ui.text_edit_singleline(&mut self.server_input);
                if ui.button("Connect").clicked() {
                    self.connect();
                }
            });
            let Some((api, info)) = self.connection.clone() else {
                return;
            };
            ui.separator();
            ui.label(&info.server_name);
            match self.settings.launcher_token.clone() {
                Some(token) => {
                    ui.horizontal(|ui| {
                        if ui.button("Play").clicked() {
                            self.play(api.clone(), info.clone(), token.clone());
                        }
                        if ui.button("Log out").clicked() {
                            self.logout(api.clone(), token.clone());
                        }
                    });
                }
                None => match info.auth_mode {
                    AuthMode::Password => {
                        ui.horizontal(|ui| {
                            ui.label("Username");
                            ui.text_edit_singleline(&mut self.username);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Password");
                            ui.add(egui::TextEdit::singleline(&mut self.password).password(true));
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Log in").clicked() {
                                self.password_login(api.clone(), false);
                            }
                            if info.allow_registration && ui.button("Make account").clicked() {
                                self.password_login(api.clone(), true);
                            }
                        });
                    }
                    AuthMode::Discord => {
                        if ui.button("Log in with Discord").clicked() {
                            self.discord_login(api.clone());
                        }
                    }
                },
            }
        });
        ui.separator();
        ui.label(&self.status);
    }
}

impl eframe::App for LauncherApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_job(ui.ctx());
        egui::Frame::central_panel(ui.style()).show(ui, |ui| self.draw(ui));
    }
}
