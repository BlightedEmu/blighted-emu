// SPDX-License-Identifier: AGPL-3.0-or-later
// The record editor's files: the fields a user changed against the client record, written as an
// override of the record or as a new record from it (`base`). Design files never copy client
// values (admin studio spec section 5.1).

import { badId, header, quoted } from './design.js';

const TRUE_TEXT = 'true';
const FALSE_TEXT = 'false';

/** The text a user sees and types for a client value; empty for no value. */
export function valueText(value) {
  return value === null || value === undefined ? '' : String(value);
}

/** The value of `text` for a field whose client value is `client`, or { error }. */
function parsed(name, text, client) {
  const trimmed = text.trim();
  const number = Number(trimmed);
  switch (typeof client) {
    case 'number':
      return Number.isFinite(number) && trimmed !== '' ? { value: number } : { error: `${name} must be a number.` };
    case 'boolean':
      return trimmed === TRUE_TEXT || trimmed === FALSE_TEXT
        ? { value: trimmed === TRUE_TEXT }
        : { error: `${name} must be true or false.` };
    case 'string':
      return { value: text };
    default:
      if (trimmed === TRUE_TEXT || trimmed === FALSE_TEXT) {
        return { value: trimmed === TRUE_TEXT };
      }
      return Number.isFinite(number) ? { value: number } : { value: text };
  }
}

/**
 * The changes of `inputs` (field name to typed text) against `client` (the client fields): a
 * field whose text is empty or shows the client value is no change. { ok: changes } or { error }.
 */
export function changedFields(client, inputs) {
  const changes = {};
  for (const [name, text] of Object.entries(inputs)) {
    if (text.trim() === '' || text === valueText(client[name])) {
      continue;
    }
    const result = parsed(name, text, client[name]);
    if (result.error) {
      return { error: result.error };
    }
    if (result.value !== client[name]) {
      changes[name] = result.value;
    }
  }
  return { ok: changes };
}

function valueLine(name, value) {
  return `${name} = ${typeof value === 'string' ? quoted(value) : String(value)}`;
}

function file(path, lines, changes) {
  return { path, text: `${[...lines, ...Object.entries(changes).map(([name, value]) => valueLine(name, value))].join('\n')}\n` };
}

/** Most rolls of one simulation (the server's limit). */
export const MAX_ROLLS = 100000;

/** The drop simulator request of a loot table and the form texts, or { error }. */
export function simulationRequest(table, rollsText, enemyText, contentText) {
  const [rolls, enemy, content] = [rollsText, enemyText, contentText].map((text) => Number(String(text).trim()));
  if (!Number.isInteger(rolls) || rolls < 1 || rolls > MAX_ROLLS) {
    return { error: `Rolls must be 1 to ${MAX_ROLLS}.` };
  }
  if (![enemy, content].every((level) => Number.isInteger(level) && level >= 0)) {
    return { error: 'Levels must be whole numbers.' };
  }
  return { request: { table, rolls, enemy_level: enemy, content_level: content } };
}

/** The override file of `record` (`/api/studio/records/...`) with `changes`. */
export function overrideFile(record, changes) {
  return file(`${record.folder}/${record.id}.toml`, header(record.kind, 'override', record.id), changes);
}

/** The file of a new record `newId` made from `record` with `changes`, or { error }. */
export function addFile(record, newId, changes) {
  const error = badId(newId);
  if (error) {
    return { error };
  }
  return file(`${record.folder}/${newId}.toml`, header(record.kind, 'add', newId, { base: record.id }), changes);
}
