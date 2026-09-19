// SPDX-License-Identifier: AGPL-3.0-or-later
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { changeBody, tasksIn, withRecord, withoutRecord } from './todo-model.js';

const TASK = Object.freeze({
  id: 't1',
  title: 'Wolves drop nothing',
  text: '',
  kind: 'loot',
  state: 'open',
  records: ['mobs/Wolf.toml'],
});

test('a state filter keeps the tasks in that state', () => {
  const tasks = [TASK, { ...TASK, id: 't2', state: 'done' }];
  assert.deepEqual(tasksIn(tasks, 'done').map((task) => task.id), ['t2']);
  assert.equal(tasksIn(tasks, 'all').length, 2);
});

test('a record is linked once and can be taken off', () => {
  assert.deepEqual(withRecord(TASK, ' loot/tables/LT_Wolf.toml ').records, ['mobs/Wolf.toml', 'loot/tables/LT_Wolf.toml']);
  assert.deepEqual(withRecord(TASK, 'mobs/Wolf.toml').records, ['mobs/Wolf.toml']);
  assert.deepEqual(withRecord(TASK, '  ').records, ['mobs/Wolf.toml']);
  assert.deepEqual(withoutRecord(TASK, 'mobs/Wolf.toml').records, []);
});

test('a change sends every field but the id', () => {
  const body = changeBody({ ...TASK, state: 'taken', taken_by: 'Ada', issue: undefined });
  assert.deepEqual(body, {
    title: TASK.title,
    text: '',
    kind: 'loot',
    state: 'taken',
    taken_by: 'Ada',
    issue: null,
    records: ['mobs/Wolf.toml'],
  });
});
