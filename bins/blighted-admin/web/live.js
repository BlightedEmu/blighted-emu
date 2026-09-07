// SPDX-License-Identifier: AGPL-3.0-or-later
// The live panel: the state stream of blighted-admin (server status, figures, map, players,
// events) and the bot batch buttons.

import { WorldMap } from './map.js';
import { SERIES, SIGNAL } from './palette.js';
import { STATUS_GROUPS, isMarked, nextStatuses, statusText } from './status.js';
import { sortRows } from './tables.js';
import { TILE_KINDS, TRAIL_POINTS, factElements, formatTile, trailPush } from './tiles.js';

const STREAM_URL = '/api/live/stream';
const BOTS_URL = '/api/live/bots';
const STOP_ALL_URL = '/api/live/bots/stop-all';
const STATE_EVENT = 'state';
const ADMIN_HEADERS = { 'X-Blighted-Admin': '1', 'Content-Type': 'application/json' };
const BATCH_SIZES = [1, 10, 100, 1000];
const BATCH_REFRESH_MS = 1000;
/** Most rows of the player table. */
const PLAYER_ROWS = 500;
const PLAYER_COLUMNS = 6;
const EVENTS_SHOWN = 200;
const SOCKET_LIVE = 'live';
const SOCKET_CLOSED = 'closed';
const SOCKET_CONNECTING = 'connecting';
const MOB_STATES = ['asleep', 'idle', 'chase', 'attack', 'return', 'dead'];
/** Mob colours in the order of MOB_STATES. */
const MOB_COLOURS = [SIGNAL.STONE, SIGNAL.YELLOW, SIGNAL.ORANGE, SIGNAL.RED, SIGNAL.BLUE, SIGNAL.INK];
const PLAYER_COLOUR = SIGNAL.WHITE;
const DEAD_COLOUR = SIGNAL.PURPLE;
const FROZEN_COLOUR = SIGNAL.ICE;
/** The player colour at 60 % (an 8-digit hex colour). */
const TRAIL_ALPHA_HEX = '99';
const TRAIL_COLOUR = `${PLAYER_COLOUR}${TRAIL_ALPHA_HEX}`;
const SIM_COLOURS = SERIES;
const BAD_FACT = 'bad';
const MARKED_STATUS = 'marked';
const CHANGED_STATUS = 'changed';
const PLAYER_KEY = 'player-';
const MOB_KEY = 'mob-';

const status = document.getElementById('status');
const socketBox = document.getElementById('socket');
const serversBox = document.getElementById('servers');
const tilesBox = document.getElementById('tiles');
const moreTilesBox = document.getElementById('tiles-more');
const playersCount = document.getElementById('players-count');
const batchesList = document.getElementById('batches');
const eventsList = document.getElementById('events');
const selectionBox = document.getElementById('selection');
const playersBody = document.getElementById('players');
const trailsBox = document.getElementById('trails');
const sleepingBox = document.getElementById('sleeping');

const info = await (await fetch('/api/world/info')).json();
const map = new WorldMap(document.getElementById('map'), info);
let source = null;
let socket = SOCKET_CONNECTING;
let view = null;
/** The server statuses by key (status.js), or null before the first state. */
let statuses = null;
/** The drawn status elements by key: kept, so the bar only moves when its state changes. */
const statusElements = new Map();
let trails = new Map();
let selectedKey = null;
/** The batch list as last drawn: it is drawn again only when it changes, so a click lands. */
let shownBatches = '';
let sortColumn = 'entity';
let sortDescending = false;

function playerState(player) {
  if (player.dead) {
    return 'dead';
  }
  return player.frozen ? 'frozen' : 'alive';
}

function playerColour(player) {
  if (player.dead) {
    return DEAD_COLOUR;
  }
  return player.frozen ? FROZEN_COLOUR : PLAYER_COLOUR;
}

/** Every entity of the view by map key. */
function entities() {
  const all = new Map();
  for (const player of view?.players ?? []) {
    all.set(`${PLAYER_KEY}${player.entity}`, { kind: 'player', ...player, state: playerState(player) });
  }
  for (const mob of view?.mobs ?? []) {
    all.set(`${MOB_KEY}${mob.entity}`, { kind: 'mob', ...mob, state: MOB_STATES[mob.state] ?? mob.state });
  }
  return all;
}

/** The six main figures: [label, value, bad] each. */
function mainFigures() {
  const tiles = view?.tiles;
  const p99 = tiles?.tick_p99_ms;
  const slow = p99 !== null && p99 !== undefined && p99 > view.tick_budget_ms;
  return [
    ['Players', formatTile(TILE_KINDS.COUNT, tiles?.players), false],
    ['Bots', formatTile(TILE_KINDS.COUNT, tiles?.bots), false],
    ['Worst tick p99', formatTile(TILE_KINDS.MS, p99), slow],
    ['Members sent', formatTile(TILE_KINDS.RATE, tiles?.members_s), false],
    ['Bandwidth out', formatTile(TILE_KINDS.BYTES, tiles?.bytes_out_s), false],
    ['Mobs awake', formatTile(TILE_KINDS.COUNT, tiles?.mobs_awake), false],
  ];
}

/** The other figures, in the small line under the main ones: [label, value] each. */
function moreFigures() {
  const tiles = view?.tiles;
  const rows = [
    ['Sims', formatTile(TILE_KINDS.COUNT, tiles?.sims)],
    ['Tick avg', formatTile(TILE_KINDS.MS, tiles?.tick_avg_ms)],
    ['Packets in', formatTile(TILE_KINDS.RATE, tiles?.datagrams_in_s)],
    ['Packets out', formatTile(TILE_KINDS.RATE, tiles?.datagrams_out_s)],
    ['Bandwidth in', formatTile(TILE_KINDS.BYTES, tiles?.bytes_in_s)],
    ['Mobs asleep', formatTile(TILE_KINDS.COUNT, tiles?.mobs_asleep)],
  ];
  for (const [name, percent] of tiles?.cpu ?? []) {
    rows.push([`CPU ${name}`, formatTile(TILE_KINDS.CPU, percent)]);
  }
  return rows;
}

function showFigures() {
  const main = mainFigures();
  const facts = factElements(main.map(([name, value]) => [name, value]));
  facts.forEach((fact, index) => fact.classList.toggle(BAD_FACT, main[index][2]));
  tilesBox.replaceChildren(...facts);
  moreTilesBox.replaceChildren(...factElements(moreFigures()));
}

function showSocket() {
  socketBox.dataset.state = socket;
  socketBox.textContent = socket === SOCKET_LIVE ? 'stream live' : `stream ${socket}`;
}

function statusElement(key) {
  let item = statusElements.get(key);
  if (!item) {
    item = document.createElement('div');
    item.className = 'server';
    const state = document.createElement('span');
    state.className = 'state';
    item.append(document.createElement('b'), state, document.createElement('small'));
    item.addEventListener('animationend', () => item.classList.remove(CHANGED_STATUS));
    statusElements.set(key, item);
  }
  return item;
}

function groupElement(title, items) {
  const group = document.createElement('div');
  group.className = 'server-group';
  const head = document.createElement('h2');
  head.textContent = title;
  const row = document.createElement('div');
  row.className = 'server-row';
  row.append(...items);
  group.append(head, row);
  return group;
}

function showServers(nowMs) {
  if (statuses === null || statuses.size === 0) {
    const empty = document.createElement('p');
    empty.className = 'muted';
    empty.textContent = statuses === null ? 'Waiting for the first state.' : 'No sim heartbeat and no metrics page yet.';
    serversBox.replaceChildren(empty);
    return;
  }
  const groups = { [STATUS_GROUPS.SIMS]: [], [STATUS_GROUPS.PAGES]: [] };
  for (const [key, server] of statuses) {
    const item = statusElement(key);
    const { reason, detail } = statusText(server, nowMs);
    const [name, stateBox, detailBox] = item.children;
    if (item.dataset.state !== server.state && item.dataset.state !== undefined) {
      item.classList.remove(CHANGED_STATUS);
      // Reading the layout restarts the bar animation.
      void item.offsetWidth;
      item.classList.add(CHANGED_STATUS);
    }
    item.dataset.state = server.state;
    name.textContent = server.name;
    stateBox.textContent = reason;
    detailBox.textContent = detail;
    item.classList.toggle(MARKED_STATUS, isMarked(server, nowMs));
    groups[server.group].push(item);
  }
  for (const key of statusElements.keys()) {
    if (!statuses.has(key)) {
      statusElements.delete(key);
    }
  }
  serversBox.replaceChildren(
    ...[['Sims', groups[STATUS_GROUPS.SIMS]], ['Metrics pages', groups[STATUS_GROUPS.PAGES]]]
      .filter(([, items]) => items.length > 0)
      .map(([title, items]) => groupElement(title, items)),
  );
}

function showMap() {
  const table = view?.table;
  map.setCells(
    table
      ? { size: table.cell_size_m, columns: table.columns, rows: table.rows, owners: table.owners, colors: SIM_COLOURS }
      : null,
  );
  const points = [];
  for (const mob of view?.mobs ?? []) {
    points.push({ key: `${MOB_KEY}${mob.entity}`, x: mob.x, y: mob.y, color: MOB_COLOURS[mob.state] ?? MOB_COLOURS[0] });
  }
  for (const player of view?.players ?? []) {
    points.push({ key: `${PLAYER_KEY}${player.entity}`, x: player.x, y: player.y, color: playerColour(player) });
  }
  map.setPoints(points);
  map.setPaths(
    trailsBox.checked
      ? [...trails].map(([entity, trail]) => ({ key: `${PLAYER_KEY}${entity}`, color: TRAIL_COLOUR, points: trail }))
      : [],
  );
}

function showSelection() {
  if (selectedKey === null) {
    return;
  }
  const entity = entities().get(selectedKey);
  selectionBox.textContent = entity ? JSON.stringify(entity, null, 2) : `${selectedKey} is not in view.`;
}

function select(key) {
  selectedKey = key;
  map.follow(key);
  showSelection();
}

/** A list item that says the list is empty. */
function emptyItem(text) {
  const item = document.createElement('li');
  item.className = 'empty';
  item.textContent = text;
  return item;
}

function showPlayers() {
  const rows = (view?.players ?? []).map((player) => ({ ...player, state: playerState(player) }));
  const sorted = sortRows(rows, sortColumn, sortDescending).slice(0, PLAYER_ROWS);
  playersCount.textContent = rows.length > PLAYER_ROWS ? `first ${PLAYER_ROWS} of ${rows.length}` : `${rows.length}`;
  if (sorted.length === 0) {
    const row = document.createElement('tr');
    row.className = 'empty';
    const cell = document.createElement('td');
    cell.colSpan = PLAYER_COLUMNS;
    cell.textContent = 'No player in the shown snapshots.';
    row.append(cell);
    playersBody.replaceChildren(row);
    return;
  }
  playersBody.replaceChildren(
    ...sorted.map((player) => {
      const row = document.createElement('tr');
      row.className = 'clickable';
      row.dataset.key = `${PLAYER_KEY}${player.entity}`;
      for (const [value, numeric] of [
        [player.entity, false],
        [player.sim, false],
        [player.x.toFixed(1), true],
        [player.y.toFixed(1), true],
        [player.z.toFixed(1), true],
        [player.state, false],
      ]) {
        const cell = document.createElement('td');
        cell.textContent = String(value);
        if (numeric) {
          cell.className = 'num';
        }
        row.append(cell);
      }
      return row;
    }),
  );
}

function showEvents() {
  const events = [...(view?.events ?? [])].reverse().slice(0, EVENTS_SHOWN);
  if (events.length === 0) {
    eventsList.replaceChildren(emptyItem('No event yet.'));
    return;
  }
  eventsList.replaceChildren(
    ...events.map((event) => {
      const item = document.createElement('li');
      const time = document.createElement('time');
      time.dateTime = new Date(event.time_ms).toISOString();
      time.textContent = new Date(event.time_ms).toLocaleTimeString();
      item.append(time, event.text);
      return item;
    }),
  );
}

function showAll() {
  const nowMs = Date.now();
  statuses = nextStatuses(statuses, view, nowMs);
  showServers(nowMs);
  showFigures();
  showMap();
  showSelection();
  showPlayers();
  showEvents();
}

function connect() {
  source?.close();
  socket = SOCKET_CONNECTING;
  showSocket();
  source = new EventSource(STREAM_URL);
  source.addEventListener('open', () => {
    socket = SOCKET_LIVE;
    showSocket();
  });
  source.addEventListener('error', () => {
    socket = source.readyState === EventSource.CLOSED ? SOCKET_CLOSED : SOCKET_CONNECTING;
    showSocket();
  });
  source.addEventListener(STATE_EVENT, (event) => {
    view = JSON.parse(event.data);
    trails = trailPush(trails, view.players, TRAIL_POINTS);
    showAll();
  });
}

async function post(url, body) {
  const response = await fetch(url, { method: 'POST', headers: ADMIN_HEADERS, body: body === undefined ? undefined : JSON.stringify(body) });
  if (!response.ok) {
    status.textContent = `${response.status}: ${await response.text()}`;
  }
  await refreshBatches();
}

async function refreshBatches() {
  try {
    const response = await fetch(BOTS_URL);
    const batches = response.ok ? await response.json() : [];
    const drawn = JSON.stringify(batches);
    if (drawn === shownBatches) {
      return;
    }
    shownBatches = drawn;
    if (batches.length === 0) {
      batchesList.replaceChildren(emptyItem('No batch. A Bots button starts one.'));
      return;
    }
    batchesList.replaceChildren(
      ...batches.map((batch) => {
        const item = document.createElement('li');
        item.className = 'batch';
        const text = document.createElement('span');
        text.textContent = `batch ${batch.batch}: ${batch.size} bots, ${batch.state}`;
        const stop = document.createElement('button');
        stop.type = 'button';
        stop.textContent = 'Stop';
        stop.disabled = batch.state !== 'running';
        stop.addEventListener('click', () => post(`${BOTS_URL}/${batch.batch}/stop`));
        item.append(text, stop);
        return item;
      }),
    );
  } catch {
    // The next refresh tries again.
  }
}

const buttons = document.getElementById('bot-buttons');
for (const size of BATCH_SIZES) {
  const button = document.createElement('button');
  button.type = 'button';
  button.textContent = `+${size}`;
  button.addEventListener('click', () => {
    status.textContent = '';
    post(BOTS_URL, { size });
  });
  buttons.append(button);
}
document.getElementById('stop-all').addEventListener('click', () => post(STOP_ALL_URL));
document.getElementById('reconnect').addEventListener('click', connect);
document.getElementById('fit').addEventListener('click', () => map.fit());
trailsBox.addEventListener('change', showMap);
sleepingBox.addEventListener('change', () => map.setSpawnKinds(sleepingBox.checked ? ['vitals'] : []));
// The table is drawn again with every state: a press selects, a click could miss its row.
playersBody.addEventListener('pointerdown', (event) => {
  const key = event.target.closest('tr')?.dataset.key;
  if (key) {
    select(key);
  }
});
map.onSelect((item) => {
  if (item?.key) {
    select(item.key);
  }
});
for (const header of document.querySelectorAll('th.sortable')) {
  header.addEventListener('click', () => {
    const column = header.dataset.column;
    sortDescending = column === sortColumn ? !sortDescending : false;
    sortColumn = column;
    for (const other of document.querySelectorAll('th.sortable')) {
      delete other.dataset.sorted;
    }
    header.dataset.sorted = sortDescending ? 'down' : 'up';
    showPlayers();
  });
}

showServers(Date.now());
showFigures();
showEvents();
showPlayers();
connect();
refreshBatches();
setInterval(refreshBatches, BATCH_REFRESH_MS);
