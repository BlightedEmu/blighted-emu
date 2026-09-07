// SPDX-License-Identifier: AGPL-3.0-or-later
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { openOutcome } from './slots.js';

test('an open waits while its own session indexes', () => {
  assert.equal(openOutcome({ state: 'indexing', id: 'a' }, 'a'), 'wait');
});

test('an open is done when its own session is ready or failed', () => {
  assert.equal(openOutcome({ state: 'ready', id: 'a' }, 'a'), 'done');
  assert.equal(openOutcome({ state: 'failed', id: 'a', error: 'x' }, 'a'), 'done');
});

test('an open is replaced when the slot holds another session', () => {
  assert.equal(openOutcome({ state: 'indexing', id: 'b' }, 'a'), 'replaced');
  assert.equal(openOutcome({ state: 'ready', id: 'b' }, 'a'), 'replaced');
});
