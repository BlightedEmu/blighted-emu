// SPDX-License-Identifier: AGPL-3.0-or-later
// GM commands of the live page: checks what the form holds and sends it.

const TELEPORT_URL = '/api/gm/teleport';
const ADMIN_HEADERS = Object.freeze({ 'X-Blighted-Admin': '1', 'Content-Type': 'application/json' });

/**
 * The teleport request for a player entity and the text of the x, y and z fields, or an error
 * text when a field is not a number.
 */
export function teleportRequest(entity, xText, yText, zText) {
  const texts = [xText, yText, zText].map((text) => String(text).trim());
  const values = texts.map(Number);
  if (texts.some((text) => text === '') || values.some((value) => !Number.isFinite(value))) {
    return { error: 'x, y and z must be numbers (metres).' };
  }
  const [x, y, z] = values;
  return { request: { entity, x, y, z } };
}

/** Sends a teleport; gives the answer text and whether it worked. */
export async function sendTeleport(request) {
  const response = await fetch(TELEPORT_URL, {
    method: 'POST',
    headers: ADMIN_HEADERS,
    body: JSON.stringify(request),
  });
  return { ok: response.ok, text: await response.text() };
}
