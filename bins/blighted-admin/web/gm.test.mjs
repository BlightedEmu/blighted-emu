// SPDX-License-Identifier: AGPL-3.0-or-later
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { teleportRequest } from './gm.js';

test('numbers make a teleport request', () => {
  assert.deepEqual(teleportRequest(41, '10', ' 20.5', '-3'), { request: { entity: 41, x: 10, y: 20.5, z: -3 } });
});

test('an empty or wrong field is an error', () => {
  assert.ok(teleportRequest(41, '', '1', '2').error);
  assert.ok(teleportRequest(41, '1', 'north', '2').error);
});
