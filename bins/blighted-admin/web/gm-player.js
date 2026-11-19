// SPDX-License-Identifier: AGPL-3.0-or-later
// The GM controls of one online player, shared by the live page and the GM page: teleport (to a
// point, to a place, to another player, or another player to it), heal, god mode, kill, kick,
// items, quests, level, weapon mastery and coins.

import {
  PLACE_KINDS, bringAllRequests, coinsRequest, experienceRequest, itemRequest, levelRequest, mapDistance, masteryName,
  masteryRankRequest, masteryRequest, placeGroups, placeKindCounts, placeTeleportRequest, questRequest, sendGmAndSay,
  teleportRequest,
} from './gm.js';

const PROGRESSION_URL = '/api/gm/progression';
const PLACES_URL = '/api/world/places';
const POSITION_DECIMALS = 1;
/** Places and the players table show whole metres. */
const WHOLE_METRES = 0;
const KIND_WORDS = new Map(PLACE_KINDS);
const PLAYER_COMMANDS = Object.freeze({
  heal: (entity) => ['heal', { entity }],
  kill: (entity) => ['kill', { entity }],
  kick: (entity) => ['kick', { entity }],
  'god-on': (entity) => ['god', { entity, on: true }],
  'god-off': (entity) => ['god', { entity, on: false }],
});
const MARKUP = `
  <section class="teleport" aria-label="Teleport">
    <form class="gm" data-form="teleport">
      <strong>Teleport</strong>
      <input type="text" name="x" inputmode="decimal" aria-label="x (metres)" placeholder="x">
      <input type="text" name="y" inputmode="decimal" aria-label="y (metres)" placeholder="y">
      <input type="text" name="z" inputmode="decimal" aria-label="z (metres)" placeholder="z">
      <button type="submit">Move</button>
    </form>
    <div class="teleport-head">
      <h3>Other players</h3>
      <button type="button" data-action="bring-all">Bring everyone here</button>
    </div>
    <div class="table-scroll teleport-players">
      <table class="grid">
        <thead><tr><th>Name</th><th>Position</th><th class="num">Distance</th><th></th></tr></thead>
        <tbody data-list="players"></tbody>
      </table>
    </div>
    <details class="places" data-places>
      <summary><h3>Places</h3> <span class="status" data-count="places"></span></summary>
      <p class="hint">Click a place and the player goes there at once.</p>
      <div class="place-filter">
        <input type="search" data-field="place-query" aria-label="Find a place by name or region" placeholder="Name or region">
        <div class="place-chips" data-list="kinds"></div>
      </div>
      <div class="place-groups" data-list="places"></div>
    </details>
  </section>
  <div class="gm">
    <button type="button" data-command="heal">Heal</button>
    <button type="button" data-command="god-on">God on</button>
    <button type="button" data-command="god-off">God off</button>
    <button type="button" class="danger" data-command="kill">Kill</button>
    <button type="button" class="danger" data-command="kick">Kick</button>
  </div>
  <form class="gm" data-form="item">
    <strong>Item</strong>
    <input type="text" name="item" aria-label="item id" placeholder="item id" class="wide">
    <input type="text" name="quantity" inputmode="numeric" aria-label="quantity" placeholder="1">
    <button type="submit">Give</button>
    <button type="button" data-action="take">Take</button>
  </form>
  <form class="gm" data-form="quest">
    <input type="text" name="objective" aria-label="objective id" placeholder="objective id" class="wide">
    <button type="submit">Start quest</button>
  </form>
  <form class="gm" data-form="level">
    <strong>Level</strong>
    <input type="text" name="level" inputmode="numeric" aria-label="level" placeholder="level">
    <button type="submit">Set level</button>
    <input type="text" name="xp" inputmode="numeric" aria-label="experience points" placeholder="xp">
    <button type="button" data-action="give-xp">Give XP</button>
  </form>
  <form class="gm" data-form="mastery">
    <strong>Weapon mastery</strong>
    <select name="mastery" aria-label="weapon mastery"><option value="">All weapons</option></select>
    <input type="text" name="rank" inputmode="numeric" aria-label="mastery rank" placeholder="rank">
    <button type="submit">Set rank</button>
    <input type="text" name="xp" inputmode="numeric" aria-label="mastery points" placeholder="xp">
    <button type="button" data-action="give-mastery">Give XP</button>
  </form>
  <form class="gm" data-form="coins">
    <strong>Coins</strong>
    <input type="text" name="coins" inputmode="numeric" aria-label="coins" placeholder="coins">
    <button type="submit">Set coins</button>
  </form>`;

function element(tag, className, text) {
  const made = document.createElement(tag);
  if (className) made.className = className;
  if (text !== undefined) made.textContent = text;
  return made;
}

function button(text, onClick, className) {
  const made = element('button', className, text);
  made.type = 'button';
  made.addEventListener('click', onClick);
  return made;
}

function metres(...values) {
  return values.map((value) => value.toFixed(WHOLE_METRES)).join(', ');
}

/**
 * Builds the player controls into `box`. `entity()` gives the player they act on (an entity of
 * entity.js, or undefined); `players()` gives every player online (entities too); `say` shows a
 * line to the viewer; `sent` runs after each command; `placesOpen` opens the places list at the
 * start. Returns `fill(entity)`, which puts the player's position in the teleport fields, and
 * `refresh()`, which shows the other players again (call it when the players change).
 */
export function mountPlayerControls(box, { entity, players = () => [], say, sent = () => {}, placesOpen = false }) {
  box.innerHTML = MARKUP;
  const form = (name) => box.querySelector(`[data-form="${name}"]`);
  const field = (formName, name) => form(formName).elements.namedItem(name);
  const tops = { maxLevel: 0, maxRank: 0 };

  async function send(path, body) {
    await sendGmAndSay(path, body, say);
    sent();
  }

  /** Runs the request that `build` makes from the player's entity number, or shows its error. */
  function sendBuilt(path, build) {
    const player = entity();
    if (!player) {
      say('Pick a player first.');
      return;
    }
    const { request, error } = build(player.entity);
    if (error) {
      say(error);
    } else {
      send(path, request);
    }
  }

  function onSubmit(name, handler) {
    form(name).addEventListener('submit', (event) => {
      event.preventDefault();
      handler();
    });
  }

  onSubmit('teleport', () => sendBuilt('teleport', (player) =>
    teleportRequest(player, ...['x', 'y', 'z'].map((name) => field('teleport', name).value))));

  const list = (name) => box.querySelector(`[data-list="${name}"]`);
  const bringAll = box.querySelector('[data-action="bring-all"]');

  /** The players online other than the picked one, by name. */
  function others() {
    const picked = entity();
    return players()
      .filter((player) => player.entity !== picked?.entity)
      .sort((a, b) => a.name.localeCompare(b.name));
  }

  function refresh() {
    const picked = entity();
    const rows = others();
    bringAll.disabled = !picked || bringAllRequests(picked.entity, rows).length === 0;
    if (rows.length === 0) {
      const empty = element('tr', 'empty');
      const text = element('td', '', 'No other player is online.');
      text.colSpan = 4;
      empty.append(text);
      list('players').replaceChildren(empty);
      return;
    }
    list('players').replaceChildren(...rows.map((player) => {
      const row = element('tr');
      const name = element('td', '', player.name);
      if (player.bot) name.append(element('span', 'tag', 'bot'));
      const distance = picked ? `${mapDistance(picked, player).toFixed(WHOLE_METRES)} m` : '';
      const actions = element('td', 'row-actions');
      actions.append(
        button('Bring here', () => sendBuilt('teleport-to-player', (target) =>
          ({ request: { entity: player.entity, to_entity: target } }))),
        button('Go to', () => sendBuilt('teleport-to-player', (mover) =>
          ({ request: { entity: mover, to_entity: player.entity } }))),
      );
      row.append(name, element('td', '', metres(player.x, player.y, player.z)), element('td', 'num', distance), actions);
      return row;
    }));
  }

  bringAll.addEventListener('click', async () => {
    const picked = entity();
    if (!picked) {
      say('Pick a player first.');
      return;
    }
    const requests = bringAllRequests(picked.entity, others());
    for (const request of requests) {
      await sendGmAndSay('teleport-to-player', request, say);
    }
    say(`Brought ${requests.length} players to ${picked.name}.`);
    sent();
  });

  const placesBox = box.querySelector('[data-places]');
  placesBox.open = placesOpen;
  const placeQuery = box.querySelector('[data-field="place-query"]');
  const places = { all: [], kind: '' };

  function showPlaces() {
    const counts = placeKindCounts(places.all);
    const chip = (kind, label, count) => {
      const made = button(`${label} ${count}`, () => {
        places.kind = kind;
        showPlaces();
      }, 'chip');
      made.setAttribute('aria-pressed', String(places.kind === kind));
      return made;
    };
    list('kinds').replaceChildren(
      chip('', 'All', places.all.length),
      ...PLACE_KINDS.filter(([kind]) => counts.get(kind) > 0).map(([kind, label]) => chip(kind, label, counts.get(kind))),
    );
    const groups = placeGroups(places.all, placeQuery.value, places.kind);
    const shown = groups.reduce((sum, group) => sum + group.places.length, 0);
    box.querySelector('[data-count="places"]').textContent = `${shown} of ${places.all.length}`;
    if (groups.length === 0) {
      list('places').replaceChildren(element('p', 'empty-note', places.all.length === 0
        ? 'No places: the world file or the game data has none.'
        : 'No place matches.'));
      return;
    }
    list('places').replaceChildren(...groups.flatMap(({ region, places: members }) => {
      const grid = element('div', 'place-grid');
      grid.append(...members.map((place) => {
        const made = button('', () => {
          sendBuilt('teleport', (player) => ({ request: placeTeleportRequest(player, place) }));
          ['x', 'y', 'z'].forEach((name) => {
            field('teleport', name).value = place[name].toFixed(POSITION_DECIMALS);
          });
        }, `place place-${place.kind}`);
        made.title = `${place.name}, ${region}: ${metres(place.x, place.y, place.z)}`;
        made.append(
          element('span', 'place-kind', KIND_WORDS.get(place.kind) ?? place.kind),
          element('span', 'place-name', place.name),
          element('span', 'place-at', metres(place.x, place.y)),
        );
        return made;
      }));
      return [element('h4', 'place-region', `${region} (${members.length})`), grid];
    }));
  }

  placeQuery.addEventListener('input', showPlaces);
  for (const button of box.querySelectorAll('[data-command]')) {
    button.addEventListener('click', () => {
      const player = entity();
      if (player) {
        send(...PLAYER_COMMANDS[button.dataset.command](player.entity));
      } else {
        say('Pick a player first.');
      }
    });
  }
  const itemFields = () => [field('item', 'item').value, field('item', 'quantity').value];
  onSubmit('item', () => sendBuilt('give-item', (player) => itemRequest(player, ...itemFields())));
  box.querySelector('[data-action="take"]').addEventListener('click', () => sendBuilt('take-item', (player) => {
    const { request, error } = itemRequest(player, ...itemFields());
    return error ? { error } : { request: { entity: request.entity, item: request.item } };
  }));
  onSubmit('quest', () => sendBuilt('start-quest', (player) => questRequest(player, field('quest', 'objective').value)));
  onSubmit('level', () => sendBuilt('set-level', (player) => levelRequest(player, field('level', 'level').value, tops.maxLevel)));
  box.querySelector('[data-action="give-xp"]').addEventListener('click', () =>
    sendBuilt('give-experience', (player) => experienceRequest(player, field('level', 'xp').value)));
  const mastery = field('mastery', 'mastery');
  onSubmit('mastery', () => sendBuilt('set-mastery-rank', (player) =>
    masteryRankRequest(player, mastery.value, field('mastery', 'rank').value, tops.maxRank)));
  box.querySelector('[data-action="give-mastery"]').addEventListener('click', () =>
    sendBuilt('give-mastery', (player) => masteryRequest(player, mastery.value, field('mastery', 'xp').value)));
  onSubmit('coins', () => sendBuilt('set-coins', (player) => coinsRequest(player, field('coins', 'coins').value)));

  fetch(PROGRESSION_URL)
    .then(async (response) => {
      if (!response.ok) {
        throw new Error(`Progression: ${response.status} ${await response.text()}`);
      }
      const progression = await response.json();
      tops.maxLevel = progression.max_level;
      tops.maxRank = progression.max_rank;
      mastery.append(...progression.masteries.map((id) => {
        const option = document.createElement('option');
        option.value = id;
        option.textContent = masteryName(id);
        return option;
      }));
    })
    .catch((error) => say(String(error.message ?? error)));

  fetch(PLACES_URL)
    .then(async (response) => {
      if (!response.ok) {
        throw new Error(`Places: ${response.status} ${await response.text()}`);
      }
      places.all = await response.json();
      showPlaces();
    })
    .catch((error) => say(String(error.message ?? error)));

  showPlaces();
  refresh();
  return {
    fill(player) {
      ['x', 'y', 'z'].forEach((name) => {
        field('teleport', name).value = player[name].toFixed(POSITION_DECIMALS);
      });
      refresh();
    },
    refresh,
  };
}
