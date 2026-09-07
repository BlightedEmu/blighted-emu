// SPDX-License-Identifier: AGPL-3.0-or-later
// The hoist of the live page: one signal flag per sim and per metrics page. A flag is ready,
// strained (a full sim, a slow tick) or silent (a sim without heartbeats, a page without an
// answer). A flag that changed is marked for a short time.

export const FLAG_STATES = Object.freeze({ READY: 'ready', STRAINED: 'strained', SILENT: 'silent' });
export const FLAG_GROUPS = Object.freeze({ SIMS: 'sims', PAGES: 'pages' });
/** A changed flag is marked this long. */
export const CHANGE_HOLD_MS = 10_000;
/** The flag of a sim that left stays this long after the sim went silent. */
export const SILENT_KEPT_MS = 300_000;

const SIM_KEY = 'sim:';
const PAGE_KEY = 'page:';
const GROUP_ORDER = [FLAG_GROUPS.SIMS, FLAG_GROUPS.PAGES];
const MS_PER_SECOND = 1000;
const P99_DECIMALS = 1;

function simFlag(sim) {
  const full = sim.max_players > 0 && sim.players >= sim.max_players;
  return {
    key: `${SIM_KEY}${sim.id}`,
    group: FLAG_GROUPS.SIMS,
    name: sim.id,
    state: full ? FLAG_STATES.STRAINED : FLAG_STATES.READY,
    players: sim.players,
    maxPlayers: sim.max_players,
    p99Ms: null,
  };
}

function pageFlag(page, budgetMs) {
  let state = FLAG_STATES.READY;
  if (!page.up) {
    state = FLAG_STATES.SILENT;
  } else if (page.tick_p99_ms !== null && page.tick_p99_ms > budgetMs) {
    state = FLAG_STATES.STRAINED;
  }
  return {
    key: `${PAGE_KEY}${page.url}`,
    group: FLAG_GROUPS.PAGES,
    name: new URL(page.url).host,
    state,
    players: null,
    maxPlayers: null,
    p99Ms: page.tick_p99_ms,
  };
}

/** `flag` after `old` (its flag of the state before, if any) at `nowMs`. */
function settle(old, flag, nowMs, first) {
  if (!old) {
    return { ...flag, sinceMs: nowMs, changedMs: first ? null : nowMs };
  }
  if (old.state !== flag.state) {
    return { ...flag, sinceMs: nowMs, changedMs: nowMs };
  }
  return { ...flag, sinceMs: old.sinceMs, changedMs: old.changedMs };
}

function inOrder(left, right) {
  const group = GROUP_ORDER.indexOf(left.group) - GROUP_ORDER.indexOf(right.group);
  return group !== 0 ? group : left.name.localeCompare(right.name, undefined, { numeric: true });
}

/**
 * The hoist after one more live state at `nowMs`: a Map of flags by key, sims first, each group
 * by name. `previous` is the hoist before, or null for the first state (whose flags are not
 * changes). A sim that left keeps a silent flag for SILENT_KEPT_MS.
 */
export function nextHoist(previous, view, nowMs) {
  const first = previous === null;
  const before = previous ?? new Map();
  const flags = [
    ...view.sims.map(simFlag),
    ...view.metrics.map((page) => pageFlag(page, view.tick_budget_ms)),
  ].map((flag) => settle(before.get(flag.key), flag, nowMs, first));
  const shown = new Set(flags.map((flag) => flag.key));
  for (const old of before.values()) {
    if (shown.has(old.key) || old.group !== FLAG_GROUPS.SIMS) {
      continue;
    }
    const silent = settle(old, { ...old, state: FLAG_STATES.SILENT }, nowMs, first);
    if (nowMs - silent.sinceMs <= SILENT_KEPT_MS) {
      flags.push(silent);
    }
  }
  return new Map(flags.sort(inOrder).map((flag) => [flag.key, flag]));
}

/** True while a flag is marked as changed. */
export function isMarked(flag, nowMs) {
  return flag.changedMs !== null && nowMs - flag.changedMs < CHANGE_HOLD_MS;
}

/** The reason word and the detail line under a flag. */
export function flagText(flag, nowMs) {
  if (flag.group === FLAG_GROUPS.SIMS) {
    if (flag.state === FLAG_STATES.SILENT) {
      const silentSeconds = Math.floor((nowMs - flag.sinceMs) / MS_PER_SECOND);
      return { reason: 'silent', detail: `left ${silentSeconds} s ago` };
    }
    const reason = flag.state === FLAG_STATES.STRAINED ? 'full' : 'ready';
    return { reason, detail: `${flag.players} / ${flag.maxPlayers} players` };
  }
  if (flag.state === FLAG_STATES.SILENT) {
    return { reason: 'no answer', detail: 'the last scrape failed' };
  }
  const reason = flag.state === FLAG_STATES.STRAINED ? 'slow tick' : 'ready';
  const detail = flag.p99Ms === null ? 'no tick metric' : `tick p99 ${flag.p99Ms.toFixed(P99_DECIMALS)} ms`;
  return { reason, detail };
}
