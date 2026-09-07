// SPDX-License-Identifier: AGPL-3.0-or-later
// Rows of a hex view.

/** Bytes per row. */
export const BYTES_PER_ROW = 16;
const OFFSET_DIGITS = 4;

/**
 * Rows of 16 bytes: { offset, cells: [{ hex, inside }] }, where `inside` marks the bytes of
 * `highlight` ({ start, end }, end not included), or none when it is null.
 */
export function hexRows(bytes, highlight) {
  const rows = [];
  for (let start = 0; start < bytes.length; start += BYTES_PER_ROW) {
    const cells = [];
    for (let index = start; index < Math.min(start + BYTES_PER_ROW, bytes.length); index += 1) {
      cells.push({
        hex: bytes[index].toString(16).padStart(2, '0'),
        inside: highlight !== null && index >= highlight.start && index < highlight.end,
      });
    }
    rows.push({ offset: start.toString(16).padStart(OFFSET_DIGITS, '0'), cells });
  }
  return rows;
}
