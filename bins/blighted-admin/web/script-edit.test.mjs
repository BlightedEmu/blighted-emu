// SPDX-License-Identifier: AGPL-3.0-or-later
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { addLink, graphOf, newNodeId, removeNode, scriptFile, valueOf } from './script-edit.js';

const BLOCKS = [
  {
    name: 'player_enters_area',
    category: 'trigger',
    values: [{ name: 'center', kind: 'position', required: true }, { name: 'radius_m', kind: 'number', required: true }],
    outputs: ['next'],
  },
  { name: 'give_item', category: 'action', values: [{ name: 'item', kind: 'text', required: true }], outputs: ['next'] },
];

function graph() {
  return {
    nodes: [
      { id: 'enter', block: 'player_enters_area', x: 40, y: 60, values: { center: [1, 2, 3.5], radius_m: 10 } },
      { id: 'gift', block: 'give_item', x: 260, y: 60, values: { item: 'FlintT1' } },
    ],
    links: [{ from: 'enter', output: 'next', to: 'gift' }],
  };
}

test('a typed value comes from its text', () => {
  assert.deepEqual(valueOf('position', ' 1, 2.5 ,-3'), { value: [1, 2.5, -3] });
  assert.ok(valueOf('position', '1, 2').error);
  assert.deepEqual(valueOf('number', '12.5'), { value: 12.5 });
  assert.ok(valueOf('number', '0').error);
  assert.deepEqual(valueOf('whole', '3000'), { value: 3000 });
  assert.ok(valueOf('whole', '1.5').error);
  assert.ok(valueOf('percent', '101').error);
  assert.deepEqual(valueOf('text', ' FlintT1 '), { value: 'FlintT1' });
  assert.deepEqual(valueOf('text', ' '), { value: null }, 'empty leaves the value unset');
});

test('a script file writes nodes with their place and values, then links', () => {
  const file = scriptFile('greeting', 'lv', graph());
  assert.equal(file.path, 'scripts/lv/greeting.toml');
  assert.equal(
    file.text,
    'schema = 1\nkind = "script"\naction = "add"\nid = "greeting"\nmap = "lv"\n'
      + '\n[[nodes]]\nid = "enter"\nblock = "player_enters_area"\nx = 40\ny = 60\ncenter = [1, 2, 3.5]\nradius_m = 10\n'
      + '\n[[nodes]]\nid = "gift"\nblock = "give_item"\nx = 260\ny = 60\nitem = "FlintT1"\n'
      + '\n[[links]]\nfrom = "enter"\noutput = "next"\nto = "gift"\n',
  );
  assert.ok(scriptFile('../bad', 'lv', graph()).error);
});

test('a loaded file gives the graph back', () => {
  const fields = {
    nodes: [
      { id: 'enter', block: 'player_enters_area', x: 40, y: 60, center: [1, 2, 3.5], radius_m: 10 },
      { id: 'gift', block: 'give_item', x: 260, y: 60, item: 'FlintT1' },
    ],
    links: [{ from: 'enter', output: 'next', to: 'gift' }],
  };
  assert.deepEqual(graphOf(fields), graph());
});

test('nodes get new ids and their links go with them', () => {
  const start = graph();
  assert.equal(newNodeId(start, 'give_item'), 'give_item_1');
  const linked = addLink(addLink(start, 'enter', 'next', 'gift'), 'gift', 'next', 'gift');
  assert.equal(linked.links.length, 1, 'no duplicate, no link to itself');
  const removed = removeNode(start, 'gift');
  assert.deepEqual(removed.nodes.map((node) => node.id), ['enter']);
  assert.deepEqual(removed.links, []);
});

test('blocks are known by name', () => {
  assert.equal(BLOCKS[0].outputs[0], 'next');
});
