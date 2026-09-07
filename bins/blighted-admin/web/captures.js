// SPDX-License-Identifier: AGPL-3.0-or-later
// The capture viewer page (admin tools spec section 4.5).

import { hexRows } from './hex.js';
import { WorldMap } from './map.js';
import { SERIES, SIGNAL } from './palette.js';
import { openOutcome } from './slots.js';
import { factElements } from './tiles.js';
import { boxAround } from './view.js';
import {
  directionText,
  formatBytes,
  formatRate,
  largest,
  ratioText,
  sortRows,
} from './tables.js';

const ADMIN_HEADERS = { 'X-Blighted-Admin': '1', 'Content-Type': 'application/json' };
const POLL_MS = 500;
const SEARCH_DELAY_MS = 200;
const MESSAGES_PAGE = 300;
const DRAW_ALL_BATCH = 16;
const PLAYER_KEY = 'player';
const PLAYER_COLOUR = SIGNAL.YELLOW;
/** Track colours: the series without yellow, which is the player's. */
const TRACK_COLOURS = SERIES.filter((colour) => colour !== PLAYER_COLOUR);
const SELECTED_COLOUR = SIGNAL.WHITE;
const FIT_MARGIN_M = 50;

const $ = (id) => document.getElementById(id);
const api = (path) => fetch(path).then((response) => {
  if (!response.ok) {
    throw new Error(`${path}: ${response.status}`);
  }
  return response.json();
});

const page = {
  map: null,
  sessions: [],
  summary: null,
  types: [],
  typeSort: { column: 'count', descending: true },
  tracks: [],
  trackPoints: new Map(),
  player: [],
  selectedKey: null,
  messages: [],
  bytes: null,
};

function cell(row, text, className) {
  const element = document.createElement(row.parentElement?.tagName === 'THEAD' ? 'th' : 'td');
  element.textContent = text;
  if (className) {
    element.className = className;
  }
  row.append(element);
  return element;
}

/** Fills a table: columns [{ key, label, format, numeric }], rows of objects. */
function fillTable(table, columns, rows, onRow, sort) {
  table.replaceChildren();
  const head = table.createTHead().insertRow();
  for (const column of columns) {
    const th = cell(head, column.label, column.numeric ? 'num' : '');
    if (sort) {
      th.classList.add('sortable');
      if (sort.state.column === column.key) {
        th.dataset.sorted = sort.state.descending ? 'down' : 'up';
      }
      th.addEventListener('click', () => sort.change(column.key));
    }
  }
  const body = table.createTBody();
  for (const item of rows) {
    const row = body.insertRow();
    for (const column of columns) {
      const value = item[column.key];
      cell(row, column.format ? column.format(value, item) : String(value ?? '—'), column.numeric ? 'num' : '');
    }
    if (onRow) {
      row.classList.add('clickable');
      row.addEventListener('click', () => onRow(item, row));
    }
  }
}

function timeText(ms) {
  if (!page.summary) {
    return '—';
  }
  return `${((ms - page.summary.first_ms) / 1000).toFixed(1)} s`;
}

// Sessions -------------------------------------------------------------------------------------

async function loadSessions(query) {
  page.sessions = await api(`/api/captures/sessions?q=${encodeURIComponent(query)}`);
  const list = $('sessions');
  list.replaceChildren();
  for (const session of page.sessions) {
    const item = document.createElement('li');
    item.className = 'clickable';
    const title = document.createElement('strong');
    title.textContent = session.post_title ?? session.name ?? session.id;
    const facts = document.createElement('span');
    facts.className = 'muted';
    facts.textContent = `${session.id} · ${formatBytes(session.bytes)}`;
    item.append(title, facts);
    item.addEventListener('click', () => openSession(session));
    list.append(item);
  }
  const ids = $('compare-ids');
  ids.replaceChildren(...page.sessions.map((session) => {
    const option = document.createElement('option');
    option.value = session.id;
    return option;
  }));
}

async function openSlot(id, slot) {
  const response = await fetch('/api/captures/open', {
    method: 'POST',
    headers: ADMIN_HEADERS,
    body: JSON.stringify({ id, slot }),
  });
  if (!response.ok) {
    throw new Error(`open: ${response.status}`);
  }
  for (;;) {
    const state = await api(`/api/captures/${slot}/state`);
    const outcome = openOutcome(state, id);
    if (outcome !== 'wait') {
      return { ...state, replaced: outcome === 'replaced' };
    }
    await new Promise((resolve) => setTimeout(resolve, POLL_MS));
  }
}

async function openSession(session) {
  $('session-title').textContent = `Reading ${session.id}`;
  const state = await openSlot(session.id, 'main');
  if (state.replaced) {
    // A later click opened another session; that open shows it.
    return;
  }
  if (state.state !== 'ready') {
    $('session-title').textContent = `${session.id}: ${state.error ?? state.state}`;
    return;
  }
  $('session-title').textContent = session.post_title ?? session.name ?? session.id;
  const [summary, types, tracks, player, coverage] = await Promise.all([
    api('/api/captures/main/summary'),
    api('/api/captures/main/types'),
    api('/api/captures/main/tracks'),
    api('/api/captures/main/player'),
    api('/api/captures/main/coverage'),
  ]);
  Object.assign(page, { summary, types, tracks, player, trackPoints: new Map(), selectedKey: null });
  showSummary();
  showTypes();
  showEntities();
  showCoverage(coverage);
  const time = $('time');
  time.min = summary.first_ms;
  time.max = summary.last_ms;
  time.value = summary.last_ms;
  drawPaths();
  fitToPlayer();
  await loadMessages();
}

function showSummary() {
  const summary = page.summary;
  const facts = [
    ['Length', timeText(summary.last_ms)],
    ['Datagrams', summary.records.toLocaleString()],
    ['Messages', summary.messages.toLocaleString()],
    ['Bytes', formatBytes(summary.bytes)],
    ['Entities', summary.tracks.toLocaleString()],
    ['Player points', summary.player_points.toLocaleString()],
    ['Stream stops', summary.failure_count.toString()],
  ];
  if (summary.error) {
    facts.push(['Ledger error', summary.error]);
  }
  $('summary').replaceChildren(...factElements(facts));
}

// Types ----------------------------------------------------------------------------------------

function showTypes() {
  const columns = [
    { key: 'type_index', label: 'idx', numeric: true },
    { key: 'name', label: 'Name' },
    { key: 'azoth_status', label: 'Azoth' },
    { key: 'direction', label: 'Direction', format: directionText },
    { key: 'count', label: 'Count', numeric: true, format: (value) => value.toLocaleString() },
    { key: 'bytes', label: 'Bytes', numeric: true, format: formatBytes },
    { key: 'per_second', label: 'Per second', numeric: true, format: formatRate },
  ];
  const sort = {
    state: page.typeSort,
    change: (column) => {
      const descending = page.typeSort.column === column ? !page.typeSort.descending : true;
      page.typeSort = { column, descending };
      showTypes();
    },
  };
  const rows = sortRows(page.types, page.typeSort.column, page.typeSort.descending);
  fillTable($('types-table'), columns, rows, showRate, sort);
}

async function showRate(type) {
  const bars = await api(`/api/captures/main/types/${type.type_index}/rate?direction=${type.direction}`);
  const canvas = $('rate');
  const context = canvas.getContext('2d');
  canvas.width = canvas.clientWidth;
  const height = canvas.height;
  const top = largest(bars, 1);
  const width = canvas.width / Math.max(bars.length, 1);
  context.clearRect(0, 0, canvas.width, height);
  context.fillStyle = getComputedStyle(document.body).getPropertyValue('--accent');
  bars.forEach((count, index) => {
    const barHeight = (count / top) * (height - 12);
    context.fillRect(index * width, height - barHeight, Math.max(width - 1, 1), barHeight);
  });
  context.fillText(`${type.name ?? type.type_index}: up to ${top} per second`, 4, 10);
  page.messageType = type.type_index;
  await loadMessages();
}

// Entities and the map --------------------------------------------------------------------------

function trackKey(track) {
  return `${track.key}/${track.appearance}`;
}

function showEntities() {
  $('entities-head').hidden = false;
  const list = $('entities');
  list.hidden = false;
  list.replaceChildren();
  const tracks = sortRows(page.tracks, 'points', true);
  for (const track of tracks) {
    const item = document.createElement('li');
    item.className = 'clickable';
    item.textContent = `${track.key} #${track.appearance} · ${track.points} points · classes ${track.classes.join(', ')}`;
    item.addEventListener('click', () => selectTrack(track));
    list.append(item);
  }
}

async function trackPoints(track) {
  const key = trackKey(track);
  if (!page.trackPoints.has(key)) {
    const points = track.points === 0
      ? []
      : await api(`/api/captures/main/tracks/${track.key}/${track.appearance}`);
    page.trackPoints.set(key, points);
  }
  return page.trackPoints.get(key);
}

async function selectTrack(track) {
  await trackPoints(track);
  page.selectedKey = trackKey(track);
  $('selection').textContent = JSON.stringify(track, null, 2);
  drawPaths();
  page.map.follow(page.selectedKey);
}

async function drawAll() {
  for (let start = 0; start < page.tracks.length; start += DRAW_ALL_BATCH) {
    await Promise.all(page.tracks.slice(start, start + DRAW_ALL_BATCH).map(trackPoints));
    drawPaths();
  }
}

function until(points) {
  const time = Number($('time').value);
  return points.filter((point) => point.time_ms <= time);
}

function drawPaths() {
  if (!page.map) {
    return;
  }
  const paths = [];
  let colour = 0;
  for (const track of page.tracks) {
    const key = trackKey(track);
    const points = page.trackPoints.get(key);
    if (!points) {
      continue;
    }
    paths.push({
      key,
      track,
      color: key === page.selectedKey ? SELECTED_COLOUR : TRACK_COLOURS[colour++ % TRACK_COLOURS.length],
      points: until(points),
    });
  }
  const player = until(page.player);
  paths.push({ key: PLAYER_KEY, color: PLAYER_COLOUR, points: player });
  page.map.setPaths(paths);
  const last = player[player.length - 1];
  page.map.setPoints(last ? [{ key: PLAYER_KEY, x: last.x, y: last.y, color: PLAYER_COLOUR, label: 'player' }] : []);
  $('time-label').textContent = timeText(Number($('time').value));
}

function fitToPlayer() {
  const points = page.player.length > 0 ? page.player : [...page.trackPoints.values()].flat();
  const box = boxAround(points, FIT_MARGIN_M);
  if (box) {
    page.map.fit(box);
  }
}

// Messages and bytes ---------------------------------------------------------------------------

async function loadMessages() {
  if (!page.summary) {
    return;
  }
  const from = Math.max(Number($('time').value) - 1000, page.summary.first_ms);
  const type = page.messageType === undefined ? '' : `&type=${page.messageType}`;
  page.messages = await api(`/api/captures/main/messages?from_ms=${from}&limit=${MESSAGES_PAGE}${type}`);
  const columns = [
    { key: 'n', label: '#', numeric: true },
    { key: 'time_ms', label: 'Time', numeric: true, format: timeText },
    { key: 'direction', label: 'Direction', format: directionText },
    { key: 'channel', label: 'Ch', numeric: true },
    { key: 'type_index', label: 'idx', numeric: true },
    { key: 'name', label: 'Name' },
    { key: 'size', label: 'Bytes', numeric: true },
    { key: 'members', label: 'Members', numeric: true },
    { key: 'stopped_at', label: 'Stopped at', numeric: true },
  ];
  fillTable($('messages-table'), columns, page.messages, showBytes);
}

async function showBytes(message) {
  $('selection').textContent = JSON.stringify(message, null, 2);
  page.bytes = await api(`/api/captures/main/messages/${message.n}/bytes`);
  const members = $('members');
  members.replaceChildren();
  page.bytes.members.forEach((member, index) => {
    const item = document.createElement('li');
    item.className = 'clickable';
    item.textContent = `${member.key} · ${member.name ?? member.class} · ${member.start}–${member.end}`;
    item.addEventListener('click', () => drawHex(index));
    members.append(item);
  });
  if (page.bytes.failed_class !== null) {
    const item = document.createElement('li');
    item.className = 'muted';
    item.textContent = `failed at ${page.bytes.failed_name ?? page.bytes.failed_class}: ${page.bytes.failed_reason}`;
    members.append(item);
  }
  if (page.bytes.stopped_at !== null) {
    const item = document.createElement('li');
    item.className = 'muted';
    item.textContent = `stopped at ${page.bytes.stopped_name ?? page.bytes.stopped_at}`;
    members.append(item);
  }
  drawHex(null);
  switchTab('bytes');
}

function drawHex(memberIndex) {
  const hex = page.bytes.hex;
  const bytes = new Uint8Array(hex.length / 2);
  for (let index = 0; index < bytes.length; index += 1) {
    bytes[index] = parseInt(hex.slice(index * 2, index * 2 + 2), 16);
  }
  const member = memberIndex === null ? null : page.bytes.members[memberIndex];
  const view = $('hex');
  view.replaceChildren();
  for (const row of hexRows(bytes, member ? { start: member.start, end: member.end } : null)) {
    const line = document.createElement('div');
    const offset = document.createElement('span');
    offset.className = 'muted';
    offset.textContent = `${row.offset}  `;
    line.append(offset);
    for (const byte of row.cells) {
      const span = document.createElement('span');
      span.textContent = `${byte.hex} `;
      if (byte.inside) {
        span.className = 'mark';
      }
      line.append(span);
    }
    view.append(line);
  }
  if (member) {
    $('selection').textContent = JSON.stringify(member, null, 2);
  }
}

// Coverage and compare -------------------------------------------------------------------------

function showCoverage(rows) {
  fillTable($('coverage-table'), [
    { key: 'class', label: 'Class', numeric: true },
    { key: 'name', label: 'Name' },
    { key: 'count', label: 'Walks stopped here', numeric: true, format: (value) => value.toLocaleString() },
  ], sortRows(rows, 'count', true));
}

async function compare() {
  const id = $('compare-search').value;
  $('compare-status').textContent = `Reading ${id}`;
  const state = await openSlot(id, 'compare');
  if (state.replaced) {
    return;
  }
  if (state.state !== 'ready') {
    $('compare-status').textContent = `${id}: ${state.error ?? state.state}`;
    return;
  }
  $('compare-status').textContent = `Comparing with ${id}`;
  const rows = await api('/api/captures/compare');
  fillTable($('compare-table'), [
    { key: 'type_index', label: 'idx', numeric: true },
    { key: 'name', label: 'Name' },
    { key: 'direction', label: 'Direction', format: directionText },
    { key: 'main_per_second', label: 'This session /s', numeric: true, format: formatRate },
    { key: 'compare_per_second', label: 'Other /s', numeric: true, format: formatRate },
    { key: 'ratio', label: 'Other ÷ this', numeric: true, format: ratioText },
  ], sortRows(rows, 'main_per_second', true));
}

// Page -----------------------------------------------------------------------------------------

function switchTab(name) {
  for (const button of $('tabs').querySelectorAll('button')) {
    button.classList.toggle('active', button.dataset.tab === name);
  }
  for (const panel of document.querySelectorAll('[data-panel]')) {
    panel.hidden = panel.dataset.panel !== name;
  }
}

async function start() {
  const info = await api('/api/world/info');
  page.map = new WorldMap($('map'), info);
  page.map.onSelect((item) => {
    if (item?.track) {
      selectTrack(item.track);
    } else if (item) {
      $('selection').textContent = JSON.stringify(item.key === PLAYER_KEY ? { player: true } : item, null, 2);
    }
  });
  let searchTimer = null;
  $('search').addEventListener('input', (event) => {
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => loadSessions(event.target.value), SEARCH_DELAY_MS);
  });
  $('time').addEventListener('input', drawPaths);
  $('time').addEventListener('change', loadMessages);
  $('show-all').addEventListener('click', drawAll);
  $('compare-open').addEventListener('click', compare);
  for (const button of $('tabs').querySelectorAll('button')) {
    button.addEventListener('click', () => switchTab(button.dataset.tab));
  }
  await loadSessions('');
}

start().catch((error) => {
  $('session-title').textContent = `The page cannot start: ${error}`;
});
