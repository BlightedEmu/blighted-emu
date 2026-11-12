// SPDX-License-Identifier: GPL-3.0-or-later
// The launcher's own update: the state at the right of the tab row and the update popup
// (updates.rs). "Later" closes the popup for this launch; "Update ready" opens it again.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

/** The text at the right of the tab row, by update state. */
const STATE_TEXT = {
  checking: "Checking for updates",
  upToDate: "Up to date",
  available: "Update ready",
  installing: "Installing the update",
  failed: "Could not check for updates",
};
/** Why "Update now" is off while the game runs (updates.rs GAME_RUNNING_REFUSAL). */
const GAME_RUNNING_TEXT = "Close the game to update";
const UPDATE_NOW_TEXT = "Update now";
/** Percent of a whole, and bytes in a kilobyte (when the size is not known). */
const PERCENT = 100;
const BYTES_PER_KB = 1024;

const element = (id) => document.getElementById(id);
const popup = element("update-popup");
const state = element("update-state");
const updateNow = element("update-now");
const later = element("update-later");
const progress = element("update-progress");
const errorLine = element("update-error");

let update = { state: "checking" };
let gameRunning = false;
/** Whether the player closed the popup with "Later" in this launch. */
let postponed = false;

function showPopup() {
  const available = update.state === "available";
  const installing = update.state === "installing";
  popup.hidden = !(installing || (available && !postponed));
  if (popup.hidden || !available) {
    updateNow.disabled = true;
    later.disabled = true;
    return;
  }
  element("update-version").textContent = update.version;
  element("update-text").textContent =
    `You have ${update.current}. The update takes about a minute. The launcher opens again by itself.`;
  element("update-notes").replaceChildren(
    ...update.notes.map((note) => {
      const item = document.createElement("li");
      item.textContent = note;
      return item;
    }),
  );
  updateNow.disabled = gameRunning;
  updateNow.textContent = gameRunning ? GAME_RUNNING_TEXT : UPDATE_NOW_TEXT;
  later.disabled = false;
}

function render() {
  state.textContent = STATE_TEXT[update.state] ?? "";
  state.disabled = update.state !== "available";
  progress.hidden = update.state !== "installing";
  showPopup();
}

/** Shows a new update state (the UpdateView of updates.rs). */
export function setUpdate(next) {
  update = next;
  render();
}

/** Tells the popup whether the game runs. */
export function setGameRunning(running) {
  gameRunning = running;
  render();
}

state.addEventListener("click", () => {
  postponed = false;
  errorLine.hidden = true;
  render();
});

later.addEventListener("click", () => {
  postponed = true;
  render();
});

updateNow.addEventListener("click", () => {
  errorLine.hidden = true;
  element("update-step").textContent = "Downloading";
  element("update-percent").textContent = "";
  element("update-fill").style.width = "0";
  invoke("install_update").catch((message) => {
    errorLine.textContent = String(message);
    errorLine.hidden = false;
  });
});

listen("update-progress", ({ payload }) => {
  const { downloaded, total } = payload;
  if (total) {
    const percent = Math.min(PERCENT, Math.round((downloaded / total) * PERCENT));
    element("update-percent").textContent = `${percent}%`;
    element("update-fill").style.width = `${percent}%`;
    if (downloaded >= total) element("update-step").textContent = "Installing";
  } else {
    element("update-percent").textContent = `${Math.round(downloaded / BYTES_PER_KB)} KB`;
  }
});
