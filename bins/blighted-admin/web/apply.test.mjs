// SPDX-License-Identifier: AGPL-3.0-or-later
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { applyLine, errorRows } from './apply.js';

test('a running step names its word, detail and share of the bar', () => {
  assert.deepEqual(applyLine({ phase: 'running', step: 'wait_sims', detail: '1 of 4 sims back' }), {
    word: 'Waiting for sims',
    detail: '1 of 4 sims back',
    bar: 0.75,
  });
  assert.equal(applyLine({ phase: 'running', step: 'check', detail: '' }).bar, 0);
});

test('the end states', () => {
  assert.deepEqual(applyLine({ phase: 'idle', step: null, detail: '' }), { word: 'Idle', detail: '', bar: 0 });
  assert.equal(applyLine({ phase: 'done', step: 'wait_sims', detail: 'x' }).bar, 1);
  const failed = applyLine({ phase: 'failed', step: 'write', detail: 'disk full' });
  assert.equal(failed.word, 'Failed');
  assert.equal(failed.detail, 'disk full');
  assert.equal(failed.bar, 0.25);
});

test('error rows are sorted by file, then field, and name the problem', () => {
  const rows = errorRows([
    { path: 'spawns/b.toml', field: null, problem: { code: 'unknown_id' } },
    { path: 'loot/a.toml', field: 'max_roll', problem: { code: 'wrong_type', detail: 'a whole number' } },
    { path: 'loot/a.toml', field: null, problem: { code: 'wrong_folder', detail: { expected: 'loot/tables' } } },
  ]);
  assert.deepEqual(rows, [
    { file: 'loot/a.toml', field: '', problem: 'wrong_folder: expected loot/tables' },
    { file: 'loot/a.toml', field: 'max_roll', problem: 'wrong_type: a whole number' },
    { file: 'spawns/b.toml', field: '', problem: 'unknown_id' },
  ]);
});
