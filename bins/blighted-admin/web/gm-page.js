// SPDX-License-Identifier: AGPL-3.0-or-later
// The GM page: who is logged in, bans, ranks (head admin) and the audit log.

import { sendGm } from '/web/gm.js';

const ME_URL = '/api/auth/me';
const LOGOUT_URL = '/api/auth/logout';
const AUDIT_URL = '/api/gm/audit?limit=200';
const RANKS_URL = '/api/gm/ranks';
const ADMIN_HEADERS = Object.freeze({ 'X-Blighted-Admin': '1' });
const HEAD_ADMIN = 'head_admin';
const RANK_NAMES = Object.freeze({ csr: 'CSR', game_master: 'Game master', head_admin: 'Head admin' });

const status = document.getElementById('status');

function cell(text) {
  const td = document.createElement('td');
  td.textContent = text;
  return td;
}

async function loadAudit() {
  const response = await fetch(AUDIT_URL);
  const rows = response.ok ? await response.json() : [];
  document.getElementById('audit-count').textContent = `${rows.length} rows`;
  document.getElementById('audit').replaceChildren(...rows.map((row) => {
    const tr = document.createElement('tr');
    const when = row.at_unix_ms ? new Date(row.at_unix_ms).toLocaleString() : '';
    const entry = row.entry;
    tr.append(
      cell(when),
      cell(entry.account ?? 'local'),
      cell(RANK_NAMES[entry.rank] ?? ''),
      cell(entry.command),
      cell(entry.target),
      cell(entry.result),
    );
    return tr;
  }));
}

async function loadRanks() {
  const response = await fetch(RANKS_URL);
  if (!response.ok) {
    return;
  }
  const rows = await response.json();
  document.getElementById('ranks').replaceChildren(...rows.map((row) => {
    const item = document.createElement('li');
    item.textContent = `${row.username ?? row.account}: ${RANK_NAMES[row.rank] ?? row.rank}`;
    return item;
  }));
}

async function run(path, body) {
  status.textContent = 'Sending…';
  try {
    const answer = await sendGm(path, body);
    status.textContent = answer.ok ? `Done: ${answer.text}` : answer.text;
  } catch (error) {
    status.textContent = `Not sent: ${error.message ?? error}`;
  }
  await Promise.all([loadAudit(), loadRanks()]);
}

const banName = () => document.getElementById('ban-name').value;
document.getElementById('ban').addEventListener('submit', (event) => {
  event.preventDefault();
  run('ban', { username: banName(), banned: true });
});
document.getElementById('unban').addEventListener('click', () => run('ban', { username: banName(), banned: false }));

document.getElementById('rank').addEventListener('submit', (event) => {
  event.preventDefault();
  const rank = document.getElementById('rank-value').value || null;
  run('ranks', { username: document.getElementById('rank-name').value, rank });
});

document.getElementById('logout').addEventListener('click', async () => {
  await fetch(LOGOUT_URL, { method: 'POST', headers: ADMIN_HEADERS });
  window.location.assign('/login');
});

const meResponse = await fetch(ME_URL);
if (meResponse.ok) {
  const me = await meResponse.json();
  document.getElementById('me').textContent = `${me.actor.username} · ${RANK_NAMES[me.actor.rank]} · ${me.mode} mode`;
  document.getElementById('logout').hidden = me.mode !== 'server';
  document.getElementById('ranks-block').hidden = me.actor.rank !== HEAD_ADMIN;
}
await Promise.all([loadAudit(), loadRanks()]);
