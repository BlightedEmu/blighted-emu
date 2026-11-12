// SPDX-License-Identifier: GPL-3.0-or-later
// The launcher page. It keeps no state of its own besides the open tab, the open server list
// and the text being typed: Rust sends the whole View (session.rs) at each change, and every
// button calls one command (commands.rs).

import { forgetPictures, renderNews } from "./news.js";
import { setGameRunning, setUpdate } from "./update.js";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const element = (id) => document.getElementById(id);
/** The heading of the server row when no server is connected. */
const NO_SERVER = "No server";

let view = null;
let serversOpen = false;

/** The state word with a capital first letter. */
function capitalized(word) {
  return word ? word[0].toUpperCase() + word.slice(1) : "";
}

/** Shows a failed command in the status line until the next view. */
function showError(message) {
  element("status").dataset.tone = "fault";
  element("status-word").textContent = "Error";
  element("status-detail").textContent = String(message);
}

/** Runs a command; its error shows in the status line. */
function call(command, args = {}) {
  return invoke(command, args).catch(showError);
}

/** Sets the value of an input unless the player is typing in it. */
function setValue(input, value) {
  if (document.activeElement !== input && input.value !== value) input.value = value;
}

/** Sends the options with `change` applied. */
function saveOptions(change) {
  call("set_options", { options: { ...view.options, ...change } });
}

function renderServers() {
  const list = element("server-list");
  list.replaceChildren(
    ...view.servers.map((server) => {
      const item = document.createElement("li");
      const button = document.createElement("button");
      button.type = "button";
      button.setAttribute("aria-current", String(server.selected));
      button.disabled = view.busy;
      const name = document.createElement("span");
      name.textContent = server.name;
      const address = document.createElement("span");
      address.className = "meta";
      address.textContent = server.server;
      button.append(name, address);
      button.addEventListener("click", () => call("connect", { server: server.server }));
      item.append(button);
      return item;
    }),
  );
  const input = element("server-input");
  setValue(input, view.serverInput);
  const typed = input.value.trim();
  const known = view.servers.some((server) => server.server === typed);
  element("new-server-hint").hidden = !typed || known;
  element("connect").disabled = view.busy;
  element("saved-as").hidden = view.savedName === null;
  setValue(element("saved-name"), view.savedName ?? "");
  element("remove-server").disabled = view.busy;
}

function renderAccount() {
  const connected = view.authMode !== null;
  element("no-server").hidden = connected;
  element("logged-in").hidden = !connected || !view.loggedIn;
  element("password-login").hidden = !connected || view.loggedIn || view.authMode !== "password";
  element("discord-login").hidden = !connected || view.loggedIn || view.authMode !== "discord";
  element("make-account").hidden = !view.allowRegistration;
  for (const id of ["log-in", "make-account", "discord-log-in", "log-out"]) {
    element(id).disabled = view.busy;
  }
  element("play").disabled = view.busy || view.gameRunning || view.updating;
}

function renderSide() {
  element("server-name").textContent = view.serverName ?? NO_SERVER;
  element("server-address").textContent = view.facts?.address ?? "";
  element("servers").hidden = !(serversOpen || view.serverName === null);
  renderServers();
  const status = element("status");
  status.dataset.tone = view.status.tone;
  element("status-word").textContent = capitalized(view.status.word);
  element("status-detail").textContent = view.status.detail;
  element("facts").hidden = view.facts === null;
  if (view.facts) {
    element("fact-login").textContent = view.facts.login;
    element("fact-sign-up").textContent = view.facts.signUp ?? "";
    element("fact-sign-up").hidden = view.facts.signUp === null;
    element("fact-sign-up-label").hidden = view.facts.signUp === null;
  }
  renderAccount();
  element("version").textContent = `Launcher ${view.version}`;
}

function pathRow(path) {
  const row = document.createElement("div");
  row.className = "path-row";
  row.dataset.field = path.field;
  const label = document.createElement("label");
  label.className = "field-label";
  label.htmlFor = `path-${path.field}`;
  label.textContent = path.label;
  const input = document.createElement("input");
  input.className = "field";
  input.id = `path-${path.field}`;
  input.spellcheck = false;
  input.autocomplete = "off";
  const save = () => {
    if (input.value !== view.options[path.field]) saveOptions({ [path.field]: input.value });
  };
  input.addEventListener("change", save);
  const state = document.createElement("span");
  state.className = "path-state";
  row.append(label, input, state);
  return row;
}

function renderOptions() {
  const rows = element("path-rows");
  const fields = view.paths.map((path) => path.field).join(",");
  if (rows.dataset.fields !== fields) {
    rows.dataset.fields = fields;
    rows.replaceChildren(...view.paths.map(pathRow));
  }
  for (const path of view.paths) {
    const row = rows.querySelector(`[data-field="${path.field}"]`);
    const input = row.querySelector("input");
    input.placeholder = path.hint;
    setValue(input, view.options[path.field]);
    const state = row.querySelector(".path-state");
    state.dataset.tone = path.tone;
    state.textContent = path.word;
  }

  const pick = element("proton-pick");
  element("proton-pick-row").hidden = !view.usesProton || view.protonBuilds.length === 0;
  const builds = view.protonBuilds.map((build) => `${build.name}\u0000${build.program}`).join("\n");
  if (pick.dataset.builds !== builds) {
    pick.dataset.builds = builds;
    const none = new Option("Pick a Proton build", "");
    none.disabled = true;
    pick.replaceChildren(none, ...view.protonBuilds.map((build) => new Option(build.name, build.program)));
  }
  const picked = view.protonBuilds.some((build) => build.program === view.options.proton);
  pick.value = picked ? view.options.proton : "";

  const switches = element("switches");
  switches.hidden = !view.usesProton;
  switches.querySelectorAll("label").forEach((label) => label.remove());
  for (const option of view.protonSwitches) {
    const label = document.createElement("label");
    const box = document.createElement("input");
    box.type = "checkbox";
    box.checked = option.on;
    box.addEventListener("change", () => {
      const on = new Set(view.options.proton_switches);
      if (box.checked) on.add(option.key);
      else on.delete(option.key);
      saveOptions({ proton_switches: [...on] });
    });
    label.append(box, document.createTextNode(option.label));
    switches.append(label);
  }
  setValue(element("environment"), view.options.environment);
}

function renderSettings() {
  document.documentElement.dataset.theme = view.theme;
  document.querySelectorAll("[data-theme-choice]").forEach((button) => {
    button.setAttribute("aria-pressed", String(button.dataset.themeChoice === view.theme));
  });
  element("settings-file").textContent = view.settingsFile;
}

function render(next) {
  view = next;
  renderSide();
  renderOptions();
  renderSettings();
  setGameRunning(view.gameRunning);
}

function showTab(name) {
  document.querySelectorAll(".tab").forEach((tab) => {
    tab.setAttribute("aria-selected", String(tab.dataset.tab === name));
  });
  document.querySelectorAll(".page").forEach((page) => {
    page.hidden = page.dataset.page !== name;
  });
}

function showNews(news) {
  renderNews(news, element("news-status"), element("news-posts"), showError);
}

// Controls.
document.querySelectorAll(".tab").forEach((tab) => {
  tab.addEventListener("click", () => showTab(tab.dataset.tab));
});
element("change-server").addEventListener("click", () => {
  serversOpen = !serversOpen;
  renderSide();
});
element("connect").addEventListener("click", () => call("connect", { server: element("server-input").value }));
element("server-input").addEventListener("keydown", (event) => {
  if (event.key === "Enter") call("connect", { server: event.target.value });
});
element("server-input").addEventListener("input", renderServers);
element("saved-name").addEventListener("change", (event) => call("rename_server", { name: event.target.value }));
element("remove-server").addEventListener("click", () => call("remove_server"));
element("password-login").addEventListener("submit", (event) => {
  event.preventDefault();
  const password = element("password");
  call("log_in", { username: element("username").value, password: password.value, register: false });
  password.value = "";
});
element("make-account").addEventListener("click", () => {
  const password = element("password");
  call("log_in", { username: element("username").value, password: password.value, register: true });
  password.value = "";
});
element("discord-log-in").addEventListener("click", () => call("discord_log_in"));
element("play").addEventListener("click", () => call("play"));
element("log-out").addEventListener("click", () => call("log_out"));
element("proton-pick").addEventListener("change", (event) => saveOptions({ proton: event.target.value }));
element("environment").addEventListener("change", (event) => saveOptions({ environment: event.target.value }));
element("find-paths").addEventListener("click", () => call("find_paths"));
document.querySelectorAll("[data-theme-choice]").forEach((button) => {
  button.addEventListener("click", () => call("set_theme", { theme: button.dataset.themeChoice }));
});

// State from Rust.
listen("view", ({ payload }) => render(payload));
listen("news", ({ payload }) => {
  forgetPictures();
  showNews(payload);
});
listen("update", ({ payload }) => setUpdate(payload));

showTab("news");
invoke("view").then(render);
invoke("news_view").then(showNews);
invoke("update_view").then(setUpdate);
