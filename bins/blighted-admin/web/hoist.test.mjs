// SPDX-License-Identifier: AGPL-3.0-or-later
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { CHANGE_HOLD_MS, FLAG_STATES, SILENT_KEPT_MS, flagText, isMarked, nextHoist } from './hoist.js';

const BUDGET_MS = 33;
const START_MS = 1_000_000;
const STEP_MS = 1000;
const PAGE_URL = 'http://127.0.0.1:9102/metrics';

const sim = (id, players = 1, max = 10) => ({ id, players, max_players: max });
const page = (up, p99) => ({ url: PAGE_URL, up, tick_avg_ms: p99, tick_p99_ms: p99 });
const view = (sims, metrics = []) => ({ sims, metrics, tick_budget_ms: BUDGET_MS });
const states = (hoist) => [...hoist.values()].map((flag) => [flag.name, flag.state]);

test('the first state has no changes and later joins are changes', () => {
  const first = nextHoist(null, view([sim('sim-2'), sim('sim-10'), sim('sim-1')]), START_MS);
  assert.deepEqual([...first.values()].map((flag) => flag.name), ['sim-1', 'sim-2', 'sim-10']);
  assert.ok([...first.values()].every((flag) => !isMarked(flag, START_MS)));
  const joined = nextHoist(first, view([sim('sim-1'), sim('sim-3')]), START_MS + STEP_MS);
  const added = joined.get('sim:sim-3');
  assert.ok(isMarked(added, START_MS + STEP_MS));
  assert.equal(isMarked(added, START_MS + STEP_MS + CHANGE_HOLD_MS), false, 'the mark ends');
});

test('a sim that left stays silent, then its flag is taken down', () => {
  let hoist = nextHoist(null, view([sim('sim-1'), sim('sim-2')]), START_MS);
  const leftMs = START_MS + STEP_MS;
  hoist = nextHoist(hoist, view([sim('sim-1')]), leftMs);
  assert.deepEqual(states(hoist), [['sim-1', FLAG_STATES.READY], ['sim-2', FLAG_STATES.SILENT]]);
  assert.ok(isMarked(hoist.get('sim:sim-2'), leftMs));
  const later = leftMs + 5 * STEP_MS;
  hoist = nextHoist(hoist, view([sim('sim-1')]), later);
  assert.equal(hoist.get('sim:sim-2').sinceMs, leftMs, 'silent since it left');
  assert.deepEqual(flagText(hoist.get('sim:sim-2'), later), { reason: 'silent', detail: 'left 5 s ago' });
  hoist = nextHoist(hoist, view([sim('sim-1')]), leftMs + SILENT_KEPT_MS + 1);
  assert.equal(hoist.has('sim:sim-2'), false);
});

test('a full sim and a slow tick are strained, a failed scrape is silent', () => {
  const hoist = nextHoist(null, view([sim('sim-1', 10, 10), sim('sim-2', 0, 0)], [page(true, 41.24)]), START_MS);
  assert.deepEqual(states(hoist), [
    ['sim-1', FLAG_STATES.STRAINED],
    ['sim-2', FLAG_STATES.READY],
    ['127.0.0.1:9102', FLAG_STATES.STRAINED],
  ]);
  assert.deepEqual(flagText(hoist.get('sim:sim-1'), START_MS), { reason: 'full', detail: '10 / 10 players' });
  assert.deepEqual(flagText(hoist.get(`page:${PAGE_URL}`), START_MS), { reason: 'slow tick', detail: 'tick p99 41.2 ms' });
  const down = nextHoist(hoist, view([], [page(false, null)]), START_MS + STEP_MS);
  assert.equal(down.get(`page:${PAGE_URL}`).state, FLAG_STATES.SILENT);
  assert.equal(flagText(down.get(`page:${PAGE_URL}`), START_MS).reason, 'no answer');
  const quiet = nextHoist(null, view([], [page(true, null)]), START_MS);
  assert.deepEqual(flagText(quiet.get(`page:${PAGE_URL}`), START_MS), { reason: 'ready', detail: 'no tick metric' });
});
