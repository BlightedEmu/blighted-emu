// SPDX-License-Identifier: AGPL-3.0-or-later
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { designPath, editHeaders, fieldsOf, groupFile, pathFile, profileFile } from './design.js';

const MAP = 'newworld_vitaeeterna';

test('a design path follows the kind folder', () => {
  assert.equal(designPath('ai_profile', 'gate_guard'), 'ai/gate_guard.toml');
  assert.equal(designPath('path', 'walk', MAP), `paths/${MAP}/walk.toml`);
  assert.equal(designPath('spawn_group', 'wolves', MAP), `spawns/${MAP}/wolves.toml`);
});

test('a profile file holds only the numbers that are set', () => {
  const file = profileFile({ id: 'gate_guard', behaviour: 'guard', numbers: { sight_m: '25', respawn_ms: '60000', speed_mps: '' } });
  assert.equal(file.path, 'ai/gate_guard.toml');
  assert.equal(
    file.text,
    'schema = 1\nkind = "ai_profile"\naction = "add"\nid = "gate_guard"\nbehaviour = "guard"\n'
      + 'respawn_ms = 60000\nsight_m = 25.0\n',
  );
});

test('a profile with a bad id or number is refused', () => {
  assert.ok(profileFile({ id: '../x', behaviour: 'guard', numbers: {} }).error);
  assert.ok(profileFile({ id: 'a', behaviour: 'dance', numbers: {} }).error);
  assert.ok(profileFile({ id: 'a', behaviour: 'guard', numbers: { sight_m: '-1' } }).error);
  assert.ok(profileFile({ id: 'a', behaviour: 'guard', numbers: { respawn_ms: '1.5' } }).error);
});

test('a path file lists its points, waits and mode', () => {
  const file = pathFile({
    id: 'walk',
    map: MAP,
    mode: 'back_and_forth',
    waits: '3000, 0',
    points: [{ x: 100, y: 200.25, z: 30 }, { x: 140, y: 200, z: 31.5 }],
  });
  assert.equal(file.path, `paths/${MAP}/walk.toml`);
  assert.equal(
    file.text,
    `schema = 1\nkind = "path"\naction = "add"\nid = "walk"\nmap = "${MAP}"\n`
      + 'points = [[100.0, 200.25, 30.0], [140.0, 200.0, 31.5]]\nwaits_ms = [3000, 0]\nmode = "back_and_forth"\n',
  );
});

test('a path needs a point and whole waits', () => {
  assert.ok(pathFile({ id: 'walk', map: MAP, mode: 'loop', waits: '', points: [] }).error);
  assert.ok(pathFile({ id: 'walk', map: MAP, mode: 'loop', waits: '1, x', points: [{ x: 1, y: 2, z: 3 }] }).error);
  const bare = pathFile({ id: 'walk', map: MAP, mode: 'loop', waits: ' ', points: [{ x: 1, y: 2, z: 3 }] });
  assert.ok(!bare.text.includes('waits_ms'));
});

test('a spawn group names its vitals, level and optional profile and path', () => {
  const file = groupFile({
    id: 'wolves',
    map: MAP,
    vitals: 'Wolf_Alpha',
    level: '20',
    profile: 'gate_guard',
    path: '',
    points: [{ x: 1, y: 2, z: 3 }],
  });
  assert.equal(file.path, `spawns/${MAP}/wolves.toml`);
  assert.equal(
    file.text,
    `schema = 1\nkind = "spawn_group"\naction = "add"\nid = "wolves"\nmap = "${MAP}"\n`
      + 'vitals = "Wolf_Alpha"\nlevel = 20\npoints = [[1.0, 2.0, 3.0]]\nai_profile = "gate_guard"\n',
  );
  assert.ok(groupFile({ id: 'w', map: MAP, vitals: 'Wolf', level: '66', profile: '', path: '', points: [{ x: 1, y: 2, z: 3 }] }).error);
  assert.ok(groupFile({ id: 'w', map: MAP, vitals: '', level: '5', profile: '', path: '', points: [{ x: 1, y: 2, z: 3 }] }).error);
});

test('fields of a loaded file fill the form', () => {
  const form = fieldsOf({
    kind: 'path',
    id: 'walk',
    map: MAP,
    fields: { points: [[1, 2, 3]], waits_ms: [5, 0], mode: 'once' },
  });
  assert.deepEqual(form, { id: 'walk', map: MAP, mode: 'once', waits: '5, 0', points: [{ x: 1, y: 2, z: 3 }] });
  const profile = fieldsOf({ kind: 'ai_profile', id: 'g', map: null, fields: { sight_m: 25 } });
  assert.deepEqual(profile, { id: 'g', map: null, behaviour: 'wander', numbers: { sight_m: '25' } });
});

test('an edit names the version it opened, or asks for a new file', () => {
  const admin = { 'X-Blighted-Admin': '1' };
  assert.deepEqual(editHeaders(admin, 'a1b2'), { 'X-Blighted-Admin': '1', 'If-Match': 'a1b2' });
  assert.deepEqual(editHeaders(admin, null), { 'X-Blighted-Admin': '1', 'If-None-Match': '*' });
  assert.deepEqual(admin, { 'X-Blighted-Admin': '1' });
});
