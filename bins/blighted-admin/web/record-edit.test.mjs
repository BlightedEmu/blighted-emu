// SPDX-License-Identifier: AGPL-3.0-or-later
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { addFile, changedFields, overrideFile, simulationRequest, valueText } from './record-edit.js';

const CLIENT = Object.freeze({ tier: 2, name: '@Sword_Name', can_salvage: true, perk1: null, weight: 1.5 });

test('a value shows as the text a user types', () => {
  assert.equal(valueText(2), '2');
  assert.equal(valueText('@Sword_Name'), '@Sword_Name');
  assert.equal(valueText(true), 'true');
  assert.equal(valueText(null), '');
});

test('only fields that differ from the client value are changes', () => {
  const inputs = { tier: '3', name: '@Sword_Name', can_salvage: 'false', perk1: '', weight: '1.5' };
  assert.deepEqual(changedFields(CLIENT, inputs), { ok: { tier: 3, can_salvage: false } });
});

test('a value of the wrong type is refused', () => {
  const result = changedFields(CLIENT, { tier: 'many' });
  assert.match(result.error, /tier/);
});

test('a field with no client value takes a number, a flag or text', () => {
  assert.deepEqual(changedFields(CLIENT, { perk1: 'PerkID_Stat_Str' }), { ok: { perk1: 'PerkID_Stat_Str' } });
  assert.deepEqual(changedFields({ count: null }, { count: '4' }), { ok: { count: 4 } });
});

test('an override names only the changed fields', () => {
  const file = overrideFile({ kind: 'item', folder: 'items', id: '1hSwordT2' }, { tier: 3, name: 'A "big" one' });
  assert.equal(file.path, 'items/1hSwordT2.toml');
  assert.equal(
    file.text,
    'schema = 1\nkind = "item"\naction = "override"\nid = "1hSwordT2"\ntier = 3\nname = "A \\"big\\" one"\n',
  );
});

test('a simulation needs a roll count in range and whole levels', () => {
  assert.deepEqual(simulationRequest('LT_Wolf', '10000', '12', '1'), {
    request: { table: 'LT_Wolf', rolls: 10000, enemy_level: 12, content_level: 1 },
  });
  assert.ok(simulationRequest('LT_Wolf', '0', '1', '1').error);
  assert.ok(simulationRequest('LT_Wolf', '100001', '1', '1').error);
  assert.ok(simulationRequest('LT_Wolf', '10', 'x', '1').error);
});

test('a new record names its base and a new id', () => {
  const file = addFile({ kind: 'item', folder: 'items', id: '1hSwordT2' }, 'my_sword', { tier: 5, weight: 1.25 });
  assert.equal(file.path, 'items/my_sword.toml');
  assert.equal(
    file.text,
    'schema = 1\nkind = "item"\naction = "add"\nid = "my_sword"\nbase = "1hSwordT2"\ntier = 5\nweight = 1.25\n',
  );
  assert.ok(addFile({ kind: 'item', folder: 'items', id: 'x' }, '../bad', {}).error);
});
