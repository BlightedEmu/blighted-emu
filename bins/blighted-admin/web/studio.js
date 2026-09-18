// SPDX-License-Identifier: AGPL-3.0-or-later
// The studio page: the design files of content/, the full check, and Apply.

import { applyLine, errorRows } from '/web/apply.js';

const FILES_URL = '/api/studio/files';
const CHECK_URL = '/api/studio/check';
const APPLY_URL = '/api/studio/apply';
const ADMIN_HEADERS = Object.freeze({ 'X-Blighted-Admin': '1' });
const POLL_MS = 500;
const HTTP_CONFLICT = 409;
const NO_FILES = 'No design files yet. See content/README.md.';

const elements = {
  status: document.getElementById('status'),
  check: document.getElementById('check'),
  apply: document.getElementById('apply'),
  line: document.getElementById('apply-line'),
  bar: document.getElementById('apply-bar'),
  word: document.getElementById('apply-word'),
  detail: document.getElementById('apply-detail'),
  problems: document.getElementById('problems'),
  files: document.getElementById('files'),
  count: document.getElementById('files-count'),
};

function cell(text, className) {
  const td = document.createElement('td');
  td.textContent = text;
  if (className) {
    td.className = className;
  }
  return td;
}

function showProblems(errors) {
  const rows = errorRows(errors);
  if (rows.length === 0) {
    const none = document.createElement('li');
    none.className = 'muted';
    none.textContent = 'None found.';
    elements.problems.replaceChildren(none);
    return;
  }
  elements.problems.replaceChildren(...rows.map((row) => {
    const item = document.createElement('li');
    item.textContent = `${row.file}${row.field ? ` · ${row.field}` : ''}: ${row.problem}`;
    return item;
  }));
}

function showFiles(view) {
  elements.count.textContent = `${view.files.length} files`;
  if (view.files.length === 0) {
    const row = document.createElement('tr');
    const empty = cell(NO_FILES, 'muted');
    empty.colSpan = 5;
    row.append(empty);
    elements.files.replaceChildren(row);
  } else {
    elements.files.replaceChildren(...view.files.map((file) => {
      const row = document.createElement('tr');
      row.append(cell(file.kind), cell(file.id), cell(file.action), cell(file.map ?? ''), cell(file.path, 'muted'));
      return row;
    }));
  }
  showProblems(view.errors);
}

function showApply(status) {
  const line = applyLine(status);
  elements.line.dataset.phase = status.phase;
  elements.bar.style.setProperty('--done', String(line.bar));
  elements.word.textContent = line.word;
  if (line.detail) {
    elements.detail.textContent = line.detail;
  }
  const running = status.phase === 'running';
  elements.check.disabled = running;
  elements.apply.disabled = running;
  if (status.errors.length > 0) {
    showProblems(status.errors);
  }
}

async function json(url, options) {
  const response = await fetch(url, options);
  if (!response.ok) {
    throw new Error(`${url}: ${response.status} ${await response.text()}`);
  }
  return response.json();
}

function fail(error) {
  elements.status.classList.add('error');
  elements.status.textContent = String(error.message ?? error);
}

async function loadFiles() {
  showFiles(await json(FILES_URL));
}

async function pollApply() {
  const status = await json(APPLY_URL);
  if (status.phase === 'running') {
    showApply(status);
    setTimeout(() => pollApply().catch(fail), POLL_MS);
  } else {
    // The file list first, so the problems that stopped Apply stay on show.
    await loadFiles();
    showApply(status);
  }
}

elements.check.addEventListener('click', async () => {
  elements.status.classList.remove('error');
  elements.status.textContent = 'Checking…';
  elements.check.disabled = true;
  elements.apply.disabled = true;
  try {
    const report = await json(CHECK_URL, { method: 'POST', headers: ADMIN_HEADERS });
    showProblems(report.errors);
    elements.status.textContent = `${report.files} files, ${report.levels} levels, ${report.errors.length} problems`;
  } catch (error) {
    fail(error);
  } finally {
    elements.check.disabled = false;
    elements.apply.disabled = false;
  }
});

elements.apply.addEventListener('click', async () => {
  elements.status.classList.remove('error');
  elements.status.textContent = '';
  try {
    const response = await fetch(APPLY_URL, { method: 'POST', headers: ADMIN_HEADERS });
    if (!response.ok) {
      fail(new Error(`Apply: ${response.status} ${await response.text()}`));
    }
    if (response.ok || response.status === HTTP_CONFLICT) {
      await pollApply();
    }
  } catch (error) {
    fail(error);
  }
});

loadFiles().catch(fail);
pollApply().catch(fail);
