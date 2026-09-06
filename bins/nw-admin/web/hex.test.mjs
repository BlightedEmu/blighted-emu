// SPDX-License-Identifier: AGPL-3.0-or-later
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { hexRows } from './hex.js';

test('hexRows marks the highlighted bytes', () => {
  const bytes = Uint8Array.from({ length: 20 }, (_, index) => index);
  const rows = hexRows(bytes, { start: 15, end: 17 });
  assert.equal(rows.length, 2);
  assert.equal(rows[0].offset, '0000');
  assert.equal(rows[1].offset, '0010');
  assert.equal(rows[0].cells[15].hex, '0f');
  assert.deepEqual(
    rows.flatMap((row) => row.cells).filter((cell) => cell.inside).map((cell) => cell.hex),
    ['0f', '10'],
  );
  assert.equal(rows[1].cells.length, 4);
});

test('hexRows without a highlight marks nothing', () => {
  const rows = hexRows(new Uint8Array([255]), null);
  assert.deepEqual(rows, [{ offset: '0000', cells: [{ hex: 'ff', inside: false }] }]);
});
