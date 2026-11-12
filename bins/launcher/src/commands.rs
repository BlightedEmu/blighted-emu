// SPDX-License-Identifier: GPL-3.0-or-later
//! The bridge between the page (`bins/launcher/ui`) and the launcher: the Tauri commands the page
//! calls and the events it listens to. Each command changes the [`Session`] and sends the new
//! [`View`] as the `view` event. A call that waits on the network runs on a worker thread; the
//! news refresh and the game watch run on their own threads too.

use std::process::Child;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::api::LauncherApi;
use crate::game::open_outside;
use crate::news::{NewsCache, NewsState, NewsView};
use crate::news_html::safe_link;
use crate::options::LaunchOptions;
use crate::session::{Job, Session, View, run_guarded};
use crate::settings::ThemeChoice;
use crate::updates::{self, UpdateState, UpdateView};

/// Event with the [`View`], sent at each change.
pub const VIEW_EVENT: &str = "view";
/// Event with the [`NewsView`], sent when the news or a picture arrives.
pub const NEWS_EVENT: &str = "news";
/// Time between two looks at a running news refresh.
const NEWS_POLL_INTERVAL: Duration = Duration::from_millis(200);
/// Start of a picture address the page shows.
const JPEG_DATA_PREFIX: &str = "data:image/jpeg;base64,";
/// Why a news link does not open.
const REFUSED_LINK: &str = "This link does not open: only web and mail links do.";

/// The launcher state the commands share.
pub struct AppState {
    session: Mutex<Session>,
    news: Mutex<NewsState>,
}

impl AppState {
    /// The shared state.
    pub fn new(session: Session, news: NewsState) -> Self {
        Self {
            session: Mutex::new(session),
            news: Mutex::new(news),
        }
    }
}

/// The value behind `mutex`; a thread that panicked while holding it leaves a usable value.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Sends the [`View`] to the page.
fn send_view(app: &AppHandle) {
    let view = lock(&app.state::<AppState>().session).view();
    // A closed window has nobody to tell.
    let _ = app.emit(VIEW_EVENT, view);
}

/// Starts the news refresh of the server of `api`, unless this launch already fetched it.
fn start_news(app: &AppHandle, api: LauncherApi) {
    {
        let state = app.state::<AppState>();
        let mut news = lock(&state.news);
        if !news.refresh_due(api.host()) {
            return;
        }
        news.start_refresh(api);
    }
    let app = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(NEWS_POLL_INTERVAL);
            let (changed, busy, view) = {
                let state = app.state::<AppState>();
                let mut news = lock(&state.news);
                let changed = news.poll();
                (changed, news.busy(), news.view())
            };
            if changed {
                let _ = app.emit(NEWS_EVENT, view);
            }
            if !busy {
                return;
            }
        }
    });
}

/// Waits for the game process to end, then tells the session and the page.
fn watch_game(app: &AppHandle, mut child: Child) {
    let app = app.clone();
    std::thread::spawn(move || {
        // An error means the process is gone too.
        let _ = child.wait();
        lock(&app.state::<AppState>().session).game_ended();
        send_view(&app);
    });
}

/// Sends the new view, then runs `job` (when there is one) on a worker thread and gives its
/// result to the session.
fn run_job(app: &AppHandle, job: Option<Job>) {
    send_view(app);
    let Some(job) = job else {
        return;
    };
    let app = app.clone();
    std::thread::spawn(move || {
        let event = run_guarded(job);
        let (child, api) = {
            let state = app.state::<AppState>();
            let mut session = lock(&state.session);
            let child = session.apply(event);
            (child, session.api().cloned())
        };
        if let Some(api) = api {
            start_news(&app, api);
        }
        if let Some(child) = child {
            watch_game(&app, child);
        }
        send_view(&app);
    });
}

/// The first job of the launcher: connecting to the selected saved server.
pub fn start(app: &AppHandle) {
    let job = lock(&app.state::<AppState>().session).start_job();
    run_job(app, job);
}

/// The link of a news post when it may open: web and mail links only.
fn checked_link(url: &str) -> Result<String, String> {
    safe_link(url).ok_or_else(|| REFUSED_LINK.to_owned())
}

/// The cached picture of post `id` as an address the page can show.
fn picture_data_url(cache: &NewsCache, id: i64) -> Option<String> {
    cache
        .load_picture(id)
        .map(|jpeg| format!("{JPEG_DATA_PREFIX}{}", STANDARD.encode(jpeg)))
}

/// Everything the page shows, apart from the news and the update.
#[tauri::command]
pub fn view(state: State<'_, AppState>) -> View {
    lock(&state.session).view()
}

/// Connects to the server typed as `server`.
#[tauri::command]
pub fn connect(app: AppHandle, state: State<'_, AppState>, server: String) {
    let job = lock(&state.session).connect(&server);
    run_job(&app, job);
}

/// Renames the selected server.
#[tauri::command]
pub fn rename_server(app: AppHandle, state: State<'_, AppState>, name: String) {
    lock(&state.session).rename(&name);
    send_view(&app);
}

/// Forgets the selected server and its login.
#[tauri::command]
pub fn remove_server(app: AppHandle, state: State<'_, AppState>) {
    lock(&state.session).remove();
    send_view(&app);
}

/// Logs in with a username and password, or makes the account first.
#[tauri::command]
pub fn log_in(
    app: AppHandle,
    state: State<'_, AppState>,
    username: String,
    password: String,
    register: bool,
) {
    let job = lock(&state.session).log_in(username, password, register);
    run_job(&app, job);
}

/// Logs in through Discord in the browser.
#[tauri::command]
pub fn discord_log_in(app: AppHandle, state: State<'_, AppState>) {
    let job = lock(&state.session).discord_log_in();
    run_job(&app, job);
}

/// Starts the game.
#[tauri::command]
pub fn play(app: AppHandle, state: State<'_, AppState>) {
    let job = lock(&state.session).play();
    run_job(&app, job);
}

/// Ends the launcher session.
#[tauri::command]
pub fn log_out(app: AppHandle, state: State<'_, AppState>) {
    let job = lock(&state.session).log_out();
    run_job(&app, job);
}

/// Saves new launch options.
#[tauri::command]
pub fn set_options(app: AppHandle, state: State<'_, AppState>, options: LaunchOptions) {
    lock(&state.session).set_options(options);
    send_view(&app);
}

/// Scans this PC for the game, Steam and Proton paths.
#[tauri::command]
pub fn find_paths(app: AppHandle, state: State<'_, AppState>) {
    lock(&state.session).find_paths();
    send_view(&app);
}

/// Saves the page theme.
#[tauri::command]
pub fn set_theme(app: AppHandle, state: State<'_, AppState>, theme: ThemeChoice) {
    lock(&state.session).set_theme(theme);
    send_view(&app);
}

/// Opens a news link in the browser.
///
/// # Errors
/// The link is not a web or mail link, or the browser did not start.
#[tauri::command]
pub fn open_link(url: String) -> Result<(), String> {
    let link = checked_link(&url)?;
    open_outside(&link).map_err(|error| format!("cannot open the browser: {error}"))
}

/// The news as the page shows it.
#[tauri::command]
pub fn news_view(state: State<'_, AppState>) -> NewsView {
    lock(&state.news).view()
}

/// The cached picture of post `id`, as a `data:` address.
#[tauri::command]
pub fn news_picture(state: State<'_, AppState>, id: i64) -> Option<String> {
    picture_data_url(lock(&state.news).cache(), id)
}

/// The update state.
#[tauri::command]
pub fn update_view(state: State<'_, UpdateState>) -> UpdateView {
    state.view()
}

/// Installs the waiting update and starts the new launcher.
///
/// # Errors
/// The text to show when the update cannot install now or failed.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    lock(&app.state::<AppState>().session)
        .begin_update()
        .map_err(str::to_owned)?;
    send_view(&app);
    let result = updates::install(app.clone()).await;
    if result.is_err() {
        lock(&app.state::<AppState>().session).end_update();
        send_view(&app);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_link_refuses_other_schemes() {
        assert_eq!(
            checked_link("javascript:alert(1)"),
            Err(REFUSED_LINK.to_owned())
        );
        assert!(checked_link("file:///etc/passwd").is_err());
        assert!(checked_link("https://example.org/x").is_ok());
        assert!(checked_link("mailto:team@example.org").is_ok());
    }

    #[test]
    fn a_news_picture_is_a_jpeg_data_url() {
        let dir = tempfile::tempdir().expect("dir");
        let cache = NewsCache::new(dir.path().to_path_buf());
        cache.save_picture(7, &[0xff, 0xd8, 0xff]).expect("picture");
        assert_eq!(
            picture_data_url(&cache, 7).as_deref(),
            Some("data:image/jpeg;base64,/9j/")
        );
        assert_eq!(picture_data_url(&cache, 8), None);
    }
}
