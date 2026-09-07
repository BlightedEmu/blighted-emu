// SPDX-License-Identifier: AGPL-3.0-or-later
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { KINDS, mobEntity, playerEntity, selectionRows } from './entity.js';

const player = (overrides) => ({
  entity: 7,
  name: 'Ada',
  bot: false,
  sim: 'sim-a',
  x: 1.25,
  y: -2,
  z: 300.04,
  dead: false,
  frozen: false,
  ...overrides,
});

test('a player and a bot are told apart and get their rows', () => {
  const ada = playerEntity(player({}));
  assert.equal(ada.kind, KINDS.PLAYER);
  assert.deepEqual(selectionRows(ada), [
    ['Name', 'Ada'],
    ['Kind', 'Player'],
    ['Entity', '7'],
    ['Sim', 'sim-a'],
    ['Position', '1.3, -2.0, 300.0'],
    ['State', 'alive'],
  ]);
  const bot = playerEntity(player({ name: 'Adminbotab', bot: true, frozen: true }));
  assert.equal(bot.kind, KINDS.BOT);
  assert.deepEqual(selectionRows(bot).slice(0, 2), [['Name', 'Adminbotab'], ['Kind', 'Bot']]);
  assert.equal(bot.state, 'frozen');
  assert.equal(playerEntity(player({ dead: true, frozen: true })).state, 'dead', 'dead wins');
});

test('a mob shows its state name and health', () => {
  const mob = mobEntity({ entity: 17179869184, sim: 'sim-b', x: 10, y: 20, state: 3, health: 40, max_health: 90 });
  assert.deepEqual(selectionRows(mob), [
    ['Kind', 'Mob'],
    ['Entity', '17179869184'],
    ['Sim', 'sim-b'],
    ['Position', '10.0, 20.0'],
    ['State', 'attack'],
    ['Health', '40 / 90'],
  ]);
  assert.equal(mobEntity({ entity: 1, state: 9 }).state, '9', 'an unknown state shows its number');
});
