// SPDX-License-Identifier: AGPL-3.0-or-later
// The server status line of the live page: one status per sim and per metrics page. A status is
// ready, strained (a full sim, a slow tick) or silent (a sim without heartbeats, a page without
// an answer). A status that changed is marked for a short time.

export const STATUS_STATES = Object.freeze({ READY: 'ready', STRAINED: 'strained', SILENT: 'silent' });
export const STATUS_GROUPS = Object.freeze({ SIMS: 'sims', PAGES: 'pages' });
/** A changed status is marked this long. */
export const CHANGE_HOLD_MS = 10_000;
/** The status of a sim that left stays this long after the sim went silent. */
export const SILENT_KEPT_MS = 300_000;

const SIM_KEY = 'sim:';
const PAGE_KEY = 'page:';
const GROUP_ORDER = [STATUS_GROUPS.SIMS, STATUS_GROUPS.PAGES];
const MS_PER_SECOND = 1000;
const P99_DECIMALS = 1;

function simStatus(sim) {
  const full = sim.max_players > 0 && sim.players >= sim.max_players;
  return {
    key: `${SIM_KEY}${sim.id}`,
    group: STATUS_GROUPS.SIMS,
    name: sim.id,
    state: full ? STATUS_STATES.STRAINED : STATUS_STATES.READY,
    players: sim.players,
    maxPlayers: sim.max_players,
    p99Ms: null,
  };
}

function pageStatus(page, budgetMs) {
  let state = STATUS_STATES.READY;
  if (!page.up) {
    state = STATUS_STATES.SILENT;
  } else if (page.tick_p99_ms !== null && page.tick_p99_ms > budgetMs) {
    state = STATUS_STATES.STRAINED;
  }
  return {
    key: `${PAGE_KEY}${page.url}`,
    group: STATUS_GROUPS.PAGES,
    name: new URL(page.url).host,
    state,
    players: null,
    maxPlayers: null,
    p99Ms: page.tick_p99_ms,
  };
}

/** `status` after `old` (its status in the state before, if any) at `nowMs`. */
function settle(old, status, nowMs, first) {
  if (!old) {
    return { ...status, sinceMs: nowMs, changedMs: first ? null : nowMs };
  }
  if (old.state !== status.state) {
    return { ...status, sinceMs: nowMs, changedMs: nowMs };
  }
  return { ...status, sinceMs: old.sinceMs, changedMs: old.changedMs };
}

function inOrder(left, right) {
  const group = GROUP_ORDER.indexOf(left.group) - GROUP_ORDER.indexOf(right.group);
  return group !== 0 ? group : left.name.localeCompare(right.name, undefined, { numeric: true });
}

/**
 * The statuses after one more live state at `nowMs`: a Map by key, sims first, each group by
 * name. `previous` is the Map before, or null for the first state (whose statuses are not
 * changes). A sim that left keeps a silent status for SILENT_KEPT_MS.
 */
export function nextStatuses(previous, view, nowMs) {
  const first = previous === null;
  const before = previous ?? new Map();
  const statuses = [
    ...view.sims.map(simStatus),
    ...view.metrics.map((page) => pageStatus(page, view.tick_budget_ms)),
  ].map((status) => settle(before.get(status.key), status, nowMs, first));
  const shown = new Set(statuses.map((status) => status.key));
  for (const old of before.values()) {
    if (shown.has(old.key) || old.group !== STATUS_GROUPS.SIMS) {
      continue;
    }
    const silent = settle(old, { ...old, state: STATUS_STATES.SILENT }, nowMs, first);
    if (nowMs - silent.sinceMs <= SILENT_KEPT_MS) {
      statuses.push(silent);
    }
  }
  return new Map(statuses.sort(inOrder).map((status) => [status.key, status]));
}

/** True while a status is marked as changed. */
export function isMarked(status, nowMs) {
  return status.changedMs !== null && nowMs - status.changedMs < CHANGE_HOLD_MS;
}

/** The status word and the detail line of a status. */
export function statusText(status, nowMs) {
  if (status.group === STATUS_GROUPS.SIMS) {
    if (status.state === STATUS_STATES.SILENT) {
      const silentSeconds = Math.floor((nowMs - status.sinceMs) / MS_PER_SECOND);
      return { reason: 'silent', detail: `left ${silentSeconds} s ago` };
    }
    const reason = status.state === STATUS_STATES.STRAINED ? 'full' : 'ready';
    return { reason, detail: `${status.players} / ${status.maxPlayers} players` };
  }
  if (status.state === STATUS_STATES.SILENT) {
    return { reason: 'no answer', detail: 'the last scrape failed' };
  }
  const reason = status.state === STATUS_STATES.STRAINED ? 'slow tick' : 'ready';
  const detail = status.p99Ms === null ? 'no tick metric' : `tick p99 ${status.p99Ms.toFixed(P99_DECIMALS)} ms`;
  return { reason, detail };
}
