// SPDX-License-Identifier: AGPL-3.0-or-later
// The design files the studio editors write (content/README.md): AI profiles, patrol paths and
// spawn groups, as TOML text with their path under content/.

import { HIGHEST_LEVEL, LOWEST_LEVEL } from './gm.js';

/** Ids that are also safe file names. */
const ID_PATTERN = /^[A-Za-z0-9_-]+$/;
const DESIGN_SCHEMA = 1;
/** Behaviours of a profile; the first is the default. */
export const BEHAVIOURS = Object.freeze(['wander', 'guard', 'patrol', 'idle']);
/** Modes of a path; the first is the default. */
export const PATH_MODES = Object.freeze(['loop', 'back_and_forth', 'once']);
/** The numbers of a profile: metres (above 0) or whole milliseconds (0 or more). */
export const PROFILE_NUMBERS = Object.freeze([
  { name: 'attack_interval_ms', whole: true },
  { name: 'attack_reach_m', whole: false },
  { name: 'leash_radius_m', whole: false },
  { name: 'respawn_ms', whole: true },
  { name: 'sight_m', whole: false },
  { name: 'speed_mps', whole: false },
  { name: 'wake_radius_m', whole: false },
  { name: 'wander_pause_ms', whole: true },
  { name: 'wander_radius_m', whole: false },
]);
const FOLDERS = Object.freeze({ ai_profile: 'ai', path: 'paths', spawn_group: 'spawns', script: 'scripts', event: 'events' });

/** The path under content/ of a file of `kind`; `map` adds its level folder. */
export function designPath(kind, id, map) {
  const folder = FOLDERS[kind];
  return map ? `${folder}/${map}/${id}.toml` : `${folder}/${id}.toml`;
}

/** A TOML float: always with a decimal point, so the files read like the README. */
function float(value) {
  return Number.isInteger(value) ? value.toFixed(1) : String(value);
}

/** A TOML basic string (JSON escapes are TOML escapes). */
export function quoted(text) {
  return JSON.stringify(text);
}

/** The header lines of a design file; `map` and `base` only when given. */
export function header(kind, action, id, { map = null, base = null } = {}) {
  const lines = [`schema = ${DESIGN_SCHEMA}`, `kind = ${quoted(kind)}`, `action = ${quoted(action)}`, `id = ${quoted(id)}`];
  if (base) {
    lines.push(`base = ${quoted(base)}`);
  }
  if (map) {
    lines.push(`map = ${quoted(map)}`);
  }
  return lines;
}

function pointsLine(points) {
  return `points = [${points.map((point) => `[${[point.x, point.y, point.z].map(float).join(', ')}]`).join(', ')}]`;
}

/** The error text of an id that is not also a safe file name; null for a good one. */
export function badId(id) {
  return ID_PATTERN.test(id) ? null : 'The id must be letters, digits, _ or - only.';
}

function file(kind, id, map, lines) {
  return { path: designPath(kind, id, map), text: `${[...header(kind, 'add', id, { map }), ...lines].join('\n')}\n` };
}

/** The AI profile file of the form, or { error }. Empty numbers are left out. */
export function profileFile({ id, behaviour, numbers }) {
  const error = badId(id) ?? (BEHAVIOURS.includes(behaviour) ? null : 'Pick a behaviour.');
  if (error) {
    return { error };
  }
  const lines = [`behaviour = ${quoted(behaviour)}`];
  for (const { name, whole } of PROFILE_NUMBERS) {
    const text = String(numbers[name] ?? '').trim();
    if (text === '') {
      continue;
    }
    const value = Number(text);
    const good = whole ? Number.isInteger(value) && value >= 0 : Number.isFinite(value) && value > 0;
    if (!good) {
      return { error: `${name} must be ${whole ? 'a whole number, 0 or more' : 'a number above 0'}.` };
    }
    lines.push(`${name} = ${whole ? String(value) : float(value)}`);
  }
  return file('ai_profile', id, null, lines);
}

/** Whole numbers of a comma list; [] for blank text, null when one is not a whole number. */
function wholeList(text) {
  const parts = String(text).split(',').map((part) => part.trim());
  if (parts.every((part) => part === '')) {
    return [];
  }
  const values = parts.map(Number);
  return parts.every((part) => part !== '') && values.every((value) => Number.isInteger(value) && value >= 0)
    ? values
    : null;
}

/** The path file of the form, or { error }. */
export function pathFile({ id, map, mode, waits, points }) {
  const waitsMs = wholeList(waits);
  const error = badId(id)
    ?? (points.length === 0 ? 'Click the map to add the path points.' : null)
    ?? (waitsMs === null ? 'Waits must be whole milliseconds, split by commas.' : null)
    ?? (PATH_MODES.includes(mode) ? null : 'Pick a mode.');
  if (error) {
    return { error };
  }
  const lines = [pointsLine(points)];
  if (waitsMs.length > 0) {
    lines.push(`waits_ms = [${waitsMs.join(', ')}]`);
  }
  lines.push(`mode = ${quoted(mode)}`);
  return file('path', id, map, lines);
}

/** The spawn group file of the form, or { error }. An empty profile or path is left out. */
export function groupFile({ id, map, vitals, level, profile, path, points }) {
  const levelValue = Number(String(level).trim());
  const vitalsId = vitals.trim();
  const error = badId(id)
    ?? (vitalsId === '' ? 'Name a vitals id.' : null)
    ?? (Number.isInteger(levelValue) && levelValue >= LOWEST_LEVEL && levelValue <= HIGHEST_LEVEL
      ? null
      : `The level must be ${LOWEST_LEVEL} to ${HIGHEST_LEVEL}.`)
    ?? (points.length === 0 ? 'Click the map to add the spawn points.' : null);
  if (error) {
    return { error };
  }
  const lines = [`vitals = ${quoted(vitalsId)}`, `level = ${levelValue}`, pointsLine(points)];
  if (profile.trim() !== '') {
    lines.push(`ai_profile = ${quoted(profile.trim())}`);
  }
  if (path.trim() !== '') {
    lines.push(`path = ${quoted(path.trim())}`);
  }
  return file('spawn_group', id, map, lines);
}

function pointsOf(fields) {
  return (fields.points ?? []).map(([x, y, z]) => ({ x, y, z }));
}

/** The form values of a loaded design file (`/api/studio/files` entry). */
export function fieldsOf({ kind, id, map, fields }) {
  switch (kind) {
    case 'ai_profile':
      return {
        id,
        map,
        behaviour: fields.behaviour ?? BEHAVIOURS[0],
        numbers: Object.fromEntries(
          PROFILE_NUMBERS.filter(({ name }) => name in fields).map(({ name }) => [name, String(fields[name])]),
        ),
      };
    case 'path':
      return {
        id,
        map,
        mode: fields.mode ?? PATH_MODES[0],
        waits: (fields.waits_ms ?? []).join(', '),
        points: pointsOf(fields),
      };
    default:
      return {
        id,
        map,
        vitals: fields.vitals ?? '',
        level: String(fields.level ?? ''),
        profile: fields.ai_profile ?? '',
        path: fields.path ?? '',
        points: pointsOf(fields),
      };
  }
}
