// SPDX-License-Identifier: AGPL-3.0-or-later
import { test } from 'node:test';
import assert from 'node:assert/strict';

const { reduceState, fightLines, terrainColor } = await import('./live3d.js');

test('reduceState keeps the newer SSE payload', () => {
  const older = { events: [{ time_ms: 1 }], players: [], mobs: [] };
  const newer = { events: [{ time_ms: 2 }], players: [], mobs: [] };
  assert.equal(reduceState(newer, older).events[0].time_ms, 2);
  assert.equal(reduceState(older, newer).events[0].time_ms, 2);
});

test('reduceState accepts the first event', () => {
  const first = { events: [{ time_ms: 1 }], players: [], mobs: [] };
  assert.equal(reduceState(null, first).events[0].time_ms, 1);
});

test('fightLines skips a target that is not present', () => {
  const view = {
    players: [{ entity: 1, dead: false, target_entity: 99 }],
    mobs: [],
  };
  assert.deepEqual(fightLines(view), []);
});

test('fightLines skips a dead attacker and a dead target', () => {
  const view = {
    players: [
      { entity: 1, dead: true, target_entity: 2, health: 0 },
      { entity: 2, dead: false, target_entity: null, health: 10 },
    ],
    mobs: [
      { entity: 10, state: 2, health: 5, target_entity: 2 },
      { entity: 11, state: 5, health: 0, target_entity: 2 },
    ],
  };
  const lines = fightLines(view);
  assert.deepEqual(lines, [{ fromEntity: 10, toEntity: 2 }]);
});

test('fightLines pairs an attacker with its target when both are alive', () => {
  const view = {
    players: [{ entity: 1, dead: false, target_entity: 10, health: 100 }],
    mobs: [{ entity: 10, state: 3, health: 5, target_entity: 1, max_health: 10 }],
  };
  const lines = fightLines(view).map((line) => `${line.fromEntity}->${line.toEntity}`);
  assert.deepEqual(lines.sort(), ['1->10', '10->1']);
});

test('terrainColor stays inside [0, 1] for any slope', () => {
  for (const slope of [-10, 0, 0.3, 1, 100]) {
    const [r, g, b] = terrainColor(slope);
    for (const channel of [r, g, b]) {
      assert.ok(channel >= 0 && channel <= 1, `${channel} out of range`);
    }
  }
});
