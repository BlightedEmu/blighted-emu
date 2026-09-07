// SPDX-License-Identifier: AGPL-3.0-or-later
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { CHANGE_HOLD_MS, STATUS_STATES, SILENT_KEPT_MS, statusText, isMarked, nextStatuses } from './status.js';

const BUDGET_MS = 33;
const START_MS = 1_000_000;
const STEP_MS = 1000;
const PAGE_URL = 'http://127.0.0.1:9102/metrics';

const sim = (id, players = 1, max = 10) => ({ id, players, max_players: max });
const page = (up, p99) => ({ url: PAGE_URL, up, tick_avg_ms: p99, tick_p99_ms: p99 });
const view = (sims, metrics = []) => ({ sims, metrics, tick_budget_ms: BUDGET_MS });
const states = (statuses) => [...statuses.values()].map((status) => [status.name, status.state]);

test('the first state has no changes and later joins are changes', () => {
  const first = nextStatuses(null, view([sim('sim-2'), sim('sim-10'), sim('sim-1')]), START_MS);
  assert.deepEqual([...first.values()].map((status) => status.name), ['sim-1', 'sim-2', 'sim-10']);
  assert.ok([...first.values()].every((status) => !isMarked(status, START_MS)));
  const joined = nextStatuses(first, view([sim('sim-1'), sim('sim-3')]), START_MS + STEP_MS);
  const added = joined.get('sim:sim-3');
  assert.ok(isMarked(added, START_MS + STEP_MS));
  assert.equal(isMarked(added, START_MS + STEP_MS + CHANGE_HOLD_MS), false, 'the mark ends');
});

test('a sim that left stays silent, then its status is taken down', () => {
  let statuses = nextStatuses(null, view([sim('sim-1'), sim('sim-2')]), START_MS);
  const leftMs = START_MS + STEP_MS;
  statuses = nextStatuses(statuses, view([sim('sim-1')]), leftMs);
  assert.deepEqual(states(statuses), [['sim-1', STATUS_STATES.READY], ['sim-2', STATUS_STATES.SILENT]]);
  assert.ok(isMarked(statuses.get('sim:sim-2'), leftMs));
  const later = leftMs + 5 * STEP_MS;
  statuses = nextStatuses(statuses, view([sim('sim-1')]), later);
  assert.equal(statuses.get('sim:sim-2').sinceMs, leftMs, 'silent since it left');
  assert.deepEqual(statusText(statuses.get('sim:sim-2'), later), { reason: 'silent', detail: 'left 5 s ago' });
  statuses = nextStatuses(statuses, view([sim('sim-1')]), leftMs + SILENT_KEPT_MS + 1);
  assert.equal(statuses.has('sim:sim-2'), false);
});

test('a full sim and a slow tick are strained, a failed scrape is silent', () => {
  const statuses = nextStatuses(null, view([sim('sim-1', 10, 10), sim('sim-2', 0, 0)], [page(true, 41.24)]), START_MS);
  assert.deepEqual(states(statuses), [
    ['sim-1', STATUS_STATES.STRAINED],
    ['sim-2', STATUS_STATES.READY],
    ['127.0.0.1:9102', STATUS_STATES.STRAINED],
  ]);
  assert.deepEqual(statusText(statuses.get('sim:sim-1'), START_MS), { reason: 'full', detail: '10 / 10 players' });
  assert.deepEqual(statusText(statuses.get(`page:${PAGE_URL}`), START_MS), { reason: 'slow tick', detail: 'tick p99 41.2 ms' });
  const down = nextStatuses(statuses, view([], [page(false, null)]), START_MS + STEP_MS);
  assert.equal(down.get(`page:${PAGE_URL}`).state, STATUS_STATES.SILENT);
  assert.equal(statusText(down.get(`page:${PAGE_URL}`), START_MS).reason, 'no answer');
  const quiet = nextStatuses(null, view([], [page(true, null)]), START_MS);
  assert.deepEqual(statusText(quiet.get(`page:${PAGE_URL}`), START_MS), { reason: 'ready', detail: 'no tick metric' });
});
