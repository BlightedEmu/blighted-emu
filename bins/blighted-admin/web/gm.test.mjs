// SPDX-License-Identifier: AGPL-3.0-or-later
import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  MAX_SPAWN, PLACE_KINDS, bringAllRequests, coinsRequest, eventStartRequest, eventStopRequest, experienceRequest, itemRequest, levelRequest, masteryName,
  mapDistance, masteryRankRequest, masteryRequest, placeGroups, placeKindCounts, placeTeleportRequest, questRequest,
  spawnRequest, teleportRequest,
} from './gm.js';

const PLACES = [
  { kind: 'settlement', name: 'Prydwen', region: "Monarch's Bluffs", x: 7348, y: 3740, z: 104 },
  { kind: 'shrine', name: 'Hiraeth Shrine', region: "Monarch's Bluffs", x: 7079, y: 3609, z: 90 },
  { kind: 'shrine', name: 'Church of the Courts', region: 'Nighthaven', x: 3991, y: 5604, z: 119 },
  { kind: 'expedition', name: 'Amrine Excavation', region: 'Windsward', x: 9306, y: 3214, z: 89 },
];

test('places count by kind, with every kind listed', () => {
  const counts = placeKindCounts(PLACES);
  assert.deepEqual([...counts.keys()], PLACE_KINDS.map(([kind]) => kind));
  assert.equal(counts.get('shrine'), 2);
  assert.equal(counts.get('raid'), 0);
});

test('place groups keep the region order and filter by kind and by name or region', () => {
  const regions = (groups) => groups.map((group) => `${group.region}:${group.places.length}`);
  assert.deepEqual(regions(placeGroups(PLACES, '', '')), ["Monarch's Bluffs:2", 'Nighthaven:1', 'Windsward:1']);
  assert.deepEqual(regions(placeGroups(PLACES, '', 'shrine')), ["Monarch's Bluffs:1", 'Nighthaven:1']);
  assert.deepEqual(regions(placeGroups(PLACES, ' NIGHT ', '')), ['Nighthaven:1'], 'by region, any case');
  assert.deepEqual(regions(placeGroups(PLACES, 'amrine', '')), ['Windsward:1'], 'by name');
  assert.deepEqual(placeGroups(PLACES, 'amrine', 'shrine'), []);
});

test('a place teleport sends the player to the place point', () => {
  assert.deepEqual(placeTeleportRequest(41, PLACES[3]), { entity: 41, x: 9306, y: 3214, z: 89 });
});

test('map distance leaves the height out', () => {
  assert.equal(mapDistance({ x: 0, y: 0, z: 0 }, { x: 3, y: 4, z: 100 }), 5);
});

test('bring everyone moves each other player but no bot and not the target', () => {
  const players = [{ entity: 1 }, { entity: 2 }, { entity: 3, bot: true }, { entity: 4, bot: false }];
  assert.deepEqual(bringAllRequests(1, players), [{ entity: 2, to_entity: 1 }, { entity: 4, to_entity: 1 }]);
});

test('an event start needs an event and a point', () => {
  assert.deepEqual(eventStartRequest('invasion', '1', '2', '3'), { request: { event: 'invasion', x: 1, y: 2, z: 3 } });
  assert.ok(eventStartRequest('', '1', '2', '3').error);
  assert.ok(eventStartRequest('invasion', '', '2', '3').error);
});

test('a quest start names its objective', () => {
  assert.deepEqual(questRequest(41, ' 12_GetOffBeach '), { request: { entity: 41, objective: '12_GetOffBeach' } });
  assert.ok(questRequest(41, ' ').error);
});

test('numbers make a teleport request', () => {
  assert.deepEqual(teleportRequest(41, '10', ' 20.5', '-3'), { request: { entity: 41, x: 10, y: 20.5, z: -3 } });
});

test('an empty or wrong teleport field is an error', () => {
  assert.ok(teleportRequest(41, '', '1', '2').error);
  assert.ok(teleportRequest(41, '1', 'north', '2').error);
});

test('an item request needs an id and a whole quantity', () => {
  assert.deepEqual(itemRequest(41, ' Potion ', ''), { request: { entity: 41, item: 'Potion', quantity: 1 } });
  assert.deepEqual(itemRequest(41, 'Potion', '5').request.quantity, 5);
  assert.ok(itemRequest(41, '', '1').error);
  assert.ok(itemRequest(41, 'Potion', '0').error);
  assert.ok(itemRequest(41, 'Potion', '1.5').error);
});

test('a spawn request checks the level and the count', () => {
  const fields = { vitals: 'Wolf', level: '10', count: '3', x: '1', y: '2', z: '3' };
  assert.deepEqual(spawnRequest(fields), {
    request: { vitals: 'Wolf', level: 10, count: 3, x: 1, y: 2, z: 3 },
  });
  assert.ok(spawnRequest({ ...fields, vitals: ' ' }).error);
  assert.ok(spawnRequest({ ...fields, level: '66' }).error);
  assert.ok(spawnRequest({ ...fields, count: String(MAX_SPAWN + 1) }).error);
  assert.ok(spawnRequest({ ...fields, x: 'far' }).error);
});

test('a stop names an event', () => {
  assert.deepEqual(eventStopRequest(' invasion '), { request: { event: 'invasion' } });
  assert.ok(eventStopRequest('').error);
});

test('progression requests check their numbers and keep the shown level and rank', () => {
  assert.deepEqual(experienceRequest(4, ' 500 '), { request: { entity: 4, amount: 500 } });
  assert.ok(experienceRequest(4, '0').error);
  assert.ok(experienceRequest(4, '1e21').error);
  assert.deepEqual(levelRequest(4, '60', 70), { request: { entity: 4, level: 60 } });
  assert.ok(levelRequest(4, '71', 70).error);
  assert.ok(levelRequest(4, '0', 70).error);
  assert.deepEqual(masteryRequest(4, 'BowAbilityTable', '20'), { request: { entity: 4, mastery: 'BowAbilityTable', amount: 20 } });
  assert.ok(masteryRequest(4, '', '20').error, 'one weapon only');
  assert.deepEqual(masteryRankRequest(4, '', '20', 20), { request: { entity: 4, mastery: null, rank: 20 } });
  assert.ok(masteryRankRequest(4, 'BowAbilityTable', '21', 20).error);
  assert.deepEqual(coinsRequest(4, '0'), { request: { entity: 4, coins: 0 } });
  assert.ok(coinsRequest(4, '-1').error);
  assert.equal(masteryName('GreatSwordAbilityTable'), 'GreatSword');
  assert.equal(masteryName('Cooking'), 'Cooking');
});
