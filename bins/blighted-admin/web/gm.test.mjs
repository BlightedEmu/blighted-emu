// SPDX-License-Identifier: AGPL-3.0-or-later
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { MAX_SPAWN, eventStartRequest, itemRequest, questRequest, spawnRequest, teleportRequest } from './gm.js';

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
