// SPDX-License-Identifier: AGPL-3.0-or-later
// The live panel: the state stream of blighted-admin (tiles, map, players, events) and the bot
// batch buttons.

import { WorldMap } from './map.js';
import { sortRows } from './tables.js';
import { TILE_KINDS, TRAIL_POINTS, formatTile, trailPush } from './tiles.js';

const STREAM_URL = '/api/live/stream';
const BOTS_URL = '/api/live/bots';
const STOP_ALL_URL = '/api/live/bots/stop-all';
const STATE_EVENT = 'state';
const ADMIN_HEADERS = { 'X-Blighted-Admin': '1', 'Content-Type': 'application/json' };
const BATCH_SIZES = [1, 10, 100, 1000];
const BATCH_REFRESH_MS = 1000;
/** Most rows of the player table. */
const PLAYER_ROWS = 500;
const EVENTS_SHOWN = 200;
const SOCKET_LIVE = 'live';
const SOCKET_CLOSED = 'closed';
const SOCKET_CONNECTING = 'connecting';
const MOB_STATES = ['asleep', 'idle', 'chase', 'attack', 'return', 'dead'];
const MOB_COLOURS = ['#8a8f96', '#e8a33d', '#f07f3c', '#d9534f', '#5bc0de', '#4a4d52'];
const PLAYER_COLOUR = '#4aa3df';
const DEAD_COLOUR = '#9b59b6';
const FROZEN_COLOUR = '#f5f5f5';
const TRAIL_COLOUR = 'rgba(74, 163, 223, 0.5)';
const SIM_COLOURS = ['#2f6f8f', '#8f2f6f', '#6f8f2f', '#8f6f2f', '#2f8f6f', '#6f2f8f', '#8f2f2f', '#2f2f8f'];
const PLAYER_KEY = 'player-';
const MOB_KEY = 'mob-';

const status = document.getElementById('status');
const tilesBox = document.getElementById('tiles');
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

function tileRow() {
  const tiles = view?.tiles;
  const rows = [
    ['Socket', socket],
    ['Sims', formatTile(TILE_KINDS.COUNT, tiles?.sims)],
    ['Players', formatTile(TILE_KINDS.COUNT, tiles?.players)],
    ['Bots', formatTile(TILE_KINDS.COUNT, tiles?.bots)],
    ['Tick avg', formatTile(TILE_KINDS.MS, tiles?.tick_avg_ms)],
    ['Tick p99', formatTile(TILE_KINDS.MS, tiles?.tick_p99_ms)],
    ['Packets in', formatTile(TILE_KINDS.RATE, tiles?.datagrams_in_s)],
    ['Packets out', formatTile(TILE_KINDS.RATE, tiles?.datagrams_out_s)],
    ['Bandwidth in', formatTile(TILE_KINDS.BYTES, tiles?.bytes_in_s)],
    ['Bandwidth out', formatTile(TILE_KINDS.BYTES, tiles?.bytes_out_s)],
    ['Members', formatTile(TILE_KINDS.RATE, tiles?.members_s)],
    ['Mobs awake', formatTile(TILE_KINDS.COUNT, tiles?.mobs_awake)],
    ['Mobs asleep', formatTile(TILE_KINDS.COUNT, tiles?.mobs_asleep)],
  ];
  for (const [name, percent] of tiles?.cpu ?? []) {
    rows.push([`CPU ${name}`, formatTile(TILE_KINDS.CPU, percent)]);
  }
  for (const page of view?.metrics ?? []) {
    const where = new URL(page.url).host;
    rows.push([`Tick ${where}`, `${formatTile(TILE_KINDS.MS, page.tick_avg_ms)} · p99 ${formatTile(TILE_KINDS.MS, page.tick_p99_ms)}`]);
  }
  return rows;
}

function showTiles() {
  tilesBox.replaceChildren(
    ...tileRow().map(([name, value]) => {
      const tile = document.createElement('div');
      tile.className = 'fact';
      const label = document.createElement('span');
      label.textContent = name;
      tile.append(label, value);
      if (name === 'Socket') {
        tile.classList.add(value === SOCKET_LIVE ? 'socket-live' : 'socket-closed');
      }
      return tile;
    }),
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

function showPlayers() {
  const rows = (view?.players ?? []).map((player) => ({ ...player, state: playerState(player) }));
  const sorted = sortRows(rows, sortColumn, sortDescending).slice(0, PLAYER_ROWS);
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
  eventsList.replaceChildren(
    ...events.map((event) => {
      const item = document.createElement('li');
      const time = document.createElement('span');
      time.className = 'muted';
      time.textContent = new Date(event.time_ms).toLocaleTimeString();
      item.append(time, event.text);
      return item;
    }),
  );
}

function showAll() {
  showTiles();
  showMap();
  showSelection();
  showPlayers();
  showEvents();
}

function connect() {
  source?.close();
  socket = SOCKET_CONNECTING;
  showTiles();
  source = new EventSource(STREAM_URL);
  source.addEventListener('open', () => {
    socket = SOCKET_LIVE;
    showTiles();
  });
  source.addEventListener('error', () => {
    socket = source.readyState === EventSource.CLOSED ? SOCKET_CLOSED : SOCKET_CONNECTING;
    showTiles();
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

connect();
refreshBatches();
setInterval(refreshBatches, BATCH_REFRESH_MS);
