// SPDX-License-Identifier: AGPL-3.0-or-later
// Fact elements (a label under a figure), tile texts and the player trails of the live page.

import { formatBytes, formatRate } from './tables.js';

/** The text of a value the servers did not give. */
export const UNKNOWN = 'Unknown';
/** Positions kept per player trail. */
export const TRAIL_POINTS = 120;
/** How a tile shows its value. */
export const TILE_KINDS = Object.freeze({
  COUNT: 'count',
  MS: 'ms',
  RATE: 'rate',
  BYTES: 'bytes',
  CPU: 'cpu',
});

const MS_DECIMALS = 2;
const CPU_DECIMALS = 1;

/** The text of a tile value of `kind`; "Unknown" for null. */
export function formatTile(kind, value) {
  if (value === null || value === undefined) {
    return UNKNOWN;
  }
  switch (kind) {
    case TILE_KINDS.MS:
      return `${value.toFixed(MS_DECIMALS)} ms`;
    case TILE_KINDS.RATE:
      return `${formatRate(value)}/s`;
    case TILE_KINDS.BYTES:
      return `${formatBytes(Math.round(value))}/s`;
    case TILE_KINDS.CPU:
      return `${value.toFixed(CPU_DECIMALS)} %`;
    default:
      return String(value);
  }
}

/**
 * One `.fact` element per [label, value] pair (style.css shows the label under the value). An
 * unknown value is marked, so it shows small and muted.
 */
export function factElements(pairs) {
  return pairs.map(([label, value]) => {
    const fact = document.createElement('div');
    fact.className = value === UNKNOWN ? 'fact unknown' : 'fact';
    const name = document.createElement('span');
    name.textContent = label;
    fact.append(name, String(value));
    return fact;
  });
}

/**
 * The trails after one more state: each player's position is added to its trail, which keeps
 * its last `maxPoints`; players that left lose their trail.
 */
export function trailPush(trails, players, maxPoints) {
  const next = new Map();
  for (const player of players) {
    const points = trails.get(player.entity) ?? [];
    points.push({ x: player.x, y: player.y });
    if (points.length > maxPoints) {
      points.splice(0, points.length - maxPoints);
    }
    next.set(player.entity, points);
  }
  return next;
}
