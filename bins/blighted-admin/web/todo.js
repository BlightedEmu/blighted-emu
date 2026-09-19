// SPDX-License-Identifier: AGPL-3.0-or-later
// The to-do page: tasks of content/todo.toml, GitHub issue import, and the changed design files
// to link before a commit (admin studio spec section 10).

import { ALL_STATES, changeBody, taskHeaders, tasksIn, withRecord, withoutRecord } from '/web/todo-model.js';

const TODO_URL = '/api/todo';
const TASK_URL = '/api/todo/task';
const IMPORT_URL = '/api/todo/import';
const CHANGED_URL = '/api/todo/changed';
const HEADERS = Object.freeze({ 'X-Blighted-Admin': '1', 'Content-Type': 'application/json' });
/** The status of a change refused because the task changed after the list was read. */
const HTTP_CHANGED = 412;
const KINDS = Object.freeze(['quest', 'loot', 'spawn', 'path', 'ai', 'script', 'item', 'event', 'other']);
const STATES = Object.freeze(['open', 'taken', 'done']);
/** Only a GitHub address becomes a link (a hand-edited todo.toml could hold another scheme). */
const GITHUB_PREFIX = 'https://github.com/';

const elements = {
  status: document.getElementById('status'),
  tasks: document.getElementById('tasks'),
  filter: document.getElementById('state-filter'),
  changed: document.getElementById('changed'),
  changedFiles: document.getElementById('changed-files'),
  newKind: document.getElementById('new-kind'),
};
let view = { tasks: [], editors: {} };

function say(text, isError = false) {
  elements.status.classList.toggle('error', isError);
  elements.status.textContent = text;
}

function options(select, values, labels = values) {
  select.replaceChildren(...values.map((value, index) => {
    const option = document.createElement('option');
    option.value = value;
    option.textContent = labels[index];
    return option;
  }));
}

/** Sends `body` to `url` with `headers` and shows the list it answers with. */
async function send(url, method, body, headers = HEADERS) {
  const response = await fetch(url, { method, headers, body: body ? JSON.stringify(body) : undefined });
  if (response.status === HTTP_CHANGED) {
    await loadList();
    throw new Error('Someone changed this task after you read the list. The list is fresh now: make the change again.');
  }
  if (!response.ok) {
    throw new Error(`${response.status} ${await response.text()}`);
  }
  view = await response.json();
  draw();
}

function element(tag, className, text) {
  const made = document.createElement(tag);
  if (className) {
    made.className = className;
  }
  if (text !== undefined) {
    made.textContent = text;
  }
  return made;
}

function button(text, onClick, className) {
  const made = element('button', className, text);
  made.type = 'button';
  made.addEventListener('click', () => onClick().catch(fail));
  return made;
}

function save(task) {
  return send(`${TASK_URL}/${encodeURIComponent(task.id)}`, 'PUT', changeBody(task), taskHeaders(HEADERS, task));
}

function taskItem(task) {
  const item = element('li', 'todo-task');
  const head = element('div', 'gm');
  head.append(element('b', null, `${task.id} · ${task.title}`), element('span', 'muted', task.kind));
  const state = element('select');
  options(state, STATES);
  state.value = task.state;
  state.setAttribute('aria-label', `state of ${task.id}`);
  const takenBy = element('input', 'wide');
  takenBy.type = 'text';
  takenBy.placeholder = 'taken by';
  takenBy.value = task.taken_by ?? '';
  takenBy.setAttribute('aria-label', `taken by, ${task.id}`);
  head.append(
    state,
    takenBy,
    button('Save', () => save({ ...task, state: state.value, taken_by: takenBy.value })),
    button('Delete', async () => {
      if (window.confirm(`Delete task ${task.id}?`)) {
        await send(`${TASK_URL}/${encodeURIComponent(task.id)}`, 'DELETE', null, taskHeaders(HEADERS, task));
      }
    }, 'danger'),
  );
  item.append(head);
  if (task.issue?.startsWith(GITHUB_PREFIX)) {
    const link = element('a', null, task.issue);
    link.href = task.issue;
    link.rel = 'noreferrer';
    link.target = '_blank';
    item.append(link);
  }
  if (task.text) {
    item.append(element('p', 'todo-text', task.text));
  }
  const records = element('div', 'gm');
  for (const record of task.records) {
    const link = element('a', null, record);
    link.href = view.editors[record] ?? '/studio';
    records.append(link, button('×', () => save(withoutRecord(task, record))));
  }
  const add = element('input', 'wide');
  add.type = 'text';
  add.placeholder = 'design file, e.g. items/1hSwordT2.toml';
  add.setAttribute('list', 'changed-files');
  add.setAttribute('aria-label', `link a design file to ${task.id}`);
  records.append(add, button('Link file', () => save(withRecord(task, add.value))));
  item.append(records);
  return item;
}

function draw() {
  const tasks = tasksIn(view.tasks, elements.filter.value);
  if (tasks.length === 0) {
    elements.tasks.replaceChildren(element('li', 'empty', 'No tasks.'));
  } else {
    elements.tasks.replaceChildren(...tasks.map(taskItem));
  }
  say(`${view.tasks.length} tasks`);
}

async function loadChanged() {
  const response = await fetch(CHANGED_URL);
  if (!response.ok) {
    throw new Error(`changed files: ${response.status} ${await response.text()}`);
  }
  const files = await response.json();
  elements.changed.replaceChildren(
    ...(files.length === 0 ? [element('li', 'empty', 'None.')] : files.map((file) => element('li', null, file))),
  );
  elements.changedFiles.replaceChildren(...files.map((file) => {
    const option = document.createElement('option');
    option.value = file;
    return option;
  }));
}

function fail(error) {
  say(String(error.message ?? error), true);
}

options(elements.newKind, KINDS);
elements.newKind.value = 'other';
options(elements.filter, [ALL_STATES, ...STATES], ['All tasks', 'Open', 'Taken', 'Done']);
elements.filter.addEventListener('change', draw);
document.getElementById('add').addEventListener('submit', (event) => {
  event.preventDefault();
  const title = document.getElementById('new-title');
  send(TODO_URL, 'POST', { title: title.value, kind: elements.newKind.value })
    .then(() => { title.value = ''; })
    .catch(fail);
});
document.getElementById('import').addEventListener('submit', (event) => {
  event.preventDefault();
  const url = document.getElementById('issue-url');
  say('Reading the issue…');
  send(IMPORT_URL, 'POST', { url: url.value })
    .then(() => { url.value = ''; })
    .catch(fail);
});
document.getElementById('refresh-changed').addEventListener('click', () => loadChanged().catch(fail));
/** Reads the list again and shows it. */
async function loadList() {
  const response = await fetch(TODO_URL);
  view = await response.json();
  draw();
}

loadList().catch(fail);
loadChanged().catch(fail);
