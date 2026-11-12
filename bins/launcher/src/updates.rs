// SPDX-License-Identifier: GPL-3.0-or-later
//! The launcher's own updates. At each start the launcher asks the endpoint in `tauri.conf.json`
//! (the `latest.json` of the newest GitHub Release) for a newer version. A newer version shows
//! the update popup; "Update now" downloads it, checks its signature against the public key in
//! `tauri.conf.json`, installs it and starts the new launcher. A failed check stays quiet: the
//! launcher works as before. An update is refused while the game runs, because the Windows
//! loopback proxy lives in the launcher process.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Event with the [`UpdateView`], sent at each change.
pub const UPDATE_EVENT: &str = "update";
/// Event with the [`UpdateProgress`] of a download.
pub const UPDATE_PROGRESS_EVENT: &str = "update-progress";
/// The start of a release note line that the page shows as a list item.
const NOTE_BULLET: &str = "- ";
/// Why an update cannot be installed now.
pub const GAME_RUNNING_REFUSAL: &str = "Close the game to update";
/// Why there is nothing to install.
pub const NO_UPDATE_REFUSAL: &str = "No update to install";

/// The update state the page shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum UpdateView {
    /// The check runs.
    Checking,
    /// This is the newest launcher.
    UpToDate,
    /// A newer launcher is ready to install.
    Available {
        /// The new version.
        version: String,
        /// This launcher's version.
        current: String,
        /// The release notes, one line each.
        notes: Vec<String>,
    },
    /// The update downloads and installs.
    Installing,
    /// The check failed; the launcher works as before.
    Failed,
}

/// How much of the update is downloaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    /// Bytes downloaded so far.
    pub downloaded: u64,
    /// Size of the update, when the server sends it.
    pub total: Option<u64>,
}

/// The lines of the release notes: a leading `- ` is cut and empty lines are dropped.
pub fn note_lines(body: Option<&str>) -> Vec<String> {
    body.unwrap_or_default()
        .lines()
        .map(str::trim)
        .map(|line| line.strip_prefix(NOTE_BULLET).unwrap_or(line).trim())
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Why the update shown cannot install, or `None` when it can. Whether the game runs is the
/// session's check ([`crate::session::Session::begin_update`]).
pub fn install_refusal(view: &UpdateView) -> Option<&'static str> {
    if matches!(view, UpdateView::Available { .. }) {
        None
    } else {
        Some(NO_UPDATE_REFUSAL)
    }
}

/// The update state of the running launcher.
pub struct UpdateState {
    view: Mutex<UpdateView>,
    pending: Mutex<Option<Update>>,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self {
            view: Mutex::new(UpdateView::Checking),
            pending: Mutex::new(None),
        }
    }
}

impl UpdateState {
    /// The state the page shows now.
    pub fn view(&self) -> UpdateView {
        self.view
            .lock()
            .map(|view| view.clone())
            .unwrap_or(UpdateView::Failed)
    }

    /// Sets the state and tells the page.
    fn show(&self, app: &AppHandle, view: UpdateView) {
        if let Ok(mut shown) = self.view.lock() {
            shown.clone_from(&view);
        }
        // A closed window has nobody to tell.
        let _ = app.emit(UPDATE_EVENT, view);
    }

    fn pending(&self) -> Option<Update> {
        self.pending.lock().ok().and_then(|pending| pending.clone())
    }
}

/// Starts the update check in the background. Login and play never wait for it.
pub fn start_check(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<UpdateState>();
        let checked = match app.updater() {
            Ok(updater) => updater.check().await,
            Err(error) => Err(error),
        };
        let view = match checked {
            Ok(Some(update)) => {
                let view = UpdateView::Available {
                    version: update.version.clone(),
                    current: update.current_version.clone(),
                    notes: note_lines(update.body.as_deref()),
                };
                if let Ok(mut pending) = state.pending.lock() {
                    *pending = Some(update);
                }
                view
            }
            Ok(None) => UpdateView::UpToDate,
            Err(_) => UpdateView::Failed,
        };
        state.show(&app, view);
    });
}

/// Downloads and installs the waiting update, then starts the new launcher. The caller has
/// marked the session as updating, so no game starts meanwhile.
///
/// # Errors
/// The text to show: the refusal of [`install_refusal`], or why the download or install failed
/// (the popup then stays, so the player can try again).
pub async fn install(app: AppHandle) -> Result<(), String> {
    let state = app.state::<UpdateState>();
    let shown = state.view();
    if let Some(refusal) = install_refusal(&shown) {
        return Err(refusal.to_owned());
    }
    let update = state
        .pending()
        .ok_or_else(|| NO_UPDATE_REFUSAL.to_owned())?;
    state.show(&app, UpdateView::Installing);
    let mut downloaded = 0_u64;
    let progress_app = app.clone();
    let result = update
        .download_and_install(
            |chunk, total| {
                downloaded = downloaded.saturating_add(u64::try_from(chunk).unwrap_or(u64::MAX));
                let _ =
                    progress_app.emit(UPDATE_PROGRESS_EVENT, UpdateProgress { downloaded, total });
            },
            || {},
        )
        .await;
    match result {
        Ok(()) => app.restart(),
        Err(error) => {
            state.show(&app, shown);
            Err(error.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn available() -> UpdateView {
        UpdateView::Available {
            version: "0.1.1".to_owned(),
            current: "0.1.0".to_owned(),
            notes: Vec::new(),
        }
    }

    #[test]
    fn only_an_available_update_installs() {
        assert_eq!(install_refusal(&available()), None);
        assert_eq!(
            install_refusal(&UpdateView::UpToDate),
            Some(NO_UPDATE_REFUSAL)
        );
        assert_eq!(
            install_refusal(&UpdateView::Installing),
            Some(NO_UPDATE_REFUSAL)
        );
        assert_eq!(GAME_RUNNING_REFUSAL, "Close the game to update");
    }

    #[test]
    fn a_failed_check_is_quiet() {
        assert_eq!(
            serde_json::to_value(UpdateView::Failed).expect("json"),
            serde_json::json!({ "state": "failed" })
        );
        assert_eq!(UpdateState::default().view(), UpdateView::Checking);
    }

    #[test]
    fn an_available_update_names_both_versions() {
        let json = serde_json::to_value(available()).expect("json");
        assert_eq!(json["state"], "available");
        assert_eq!(json["version"], "0.1.1");
        assert_eq!(json["current"], "0.1.0");
    }

    #[test]
    fn release_notes_become_list_lines() {
        assert_eq!(
            note_lines(Some("- New look\n\n- Faster login\n")),
            ["New look", "Faster login"]
        );
        assert_eq!(note_lines(Some("Plain line")), ["Plain line"]);
        assert!(note_lines(None).is_empty());
    }
}
