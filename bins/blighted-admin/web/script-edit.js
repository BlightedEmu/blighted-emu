// SPDX-License-Identifier: AGPL-3.0-or-later
// The script editor's graph changes and file text, with no page access (admin studio spec
// section 7.5). A graph is { nodes: [{ id, block, x, y, values }], links: [{ from, output, to }] }.

import { badId, designPath, header, quoted } from './design.js';

const SCRIPT_KIND = 'script';
const POSITION_AXES = 3;
const MAX_PERCENT = 100;
/** The node keys that are not block values. */
const NODE_KEYS = Object.freeze(['id', 'block', 'x', 'y']);

/** A TOML float: always with a decimal point. */
function float(value) {
  return Number.isInteger(value) ? value.toFixed(1) : String(value);
}

/** The value of `text` for a value of `kind` (the block registry's kinds), or { error }; empty text is { value: null }. */
export function valueOf(kind, text) {
  const trimmed = String(text).trim();
  if (trimmed === '') {
    return { value: null };
  }
  if (kind === 'text') {
    return { value: trimmed };
  }
  if (kind === 'position') {
    const parts = trimmed.split(',').map((part) => Number(part.trim()));
    return parts.length === POSITION_AXES && parts.every(Number.isFinite)
      ? { value: parts }
      : { error: 'A position is x, y, z in metres.' };
  }
  const number = Number(trimmed);
  const whole = Number.isInteger(number) && number >= 0;
  switch (kind) {
    case 'number':
      return Number.isFinite(number) && number > 0 ? { value: number } : { error: 'A number above 0.' };
    case 'whole':
      return whole ? { value: number } : { error: 'A whole number, 0 or more.' };
    default:
      return whole && number <= MAX_PERCENT ? { value: number } : { error: 'A whole number 0 to 100.' };
  }
}

/** The text of a value in a form field. */
export function valueText(value) {
  if (value === null || value === undefined) {
    return '';
  }
  return Array.isArray(value) ? value.join(', ') : String(value);
}

function valueLine(name, value) {
  if (Array.isArray(value)) {
    return `${name} = [${value.map(float).join(', ')}]`;
  }
  return `${name} = ${typeof value === 'number' ? float(value) : quoted(value)}`;
}

/** The file of script `id` of level `map`, or { error }. */
export function scriptFile(id, map, graph) {
  const error = badId(id);
  if (error) {
    return { error };
  }
  const lines = header(SCRIPT_KIND, 'add', id, { map });
  for (const node of graph.nodes) {
    lines.push('', '[[nodes]]', `id = ${quoted(node.id)}`, `block = ${quoted(node.block)}`);
    lines.push(`x = ${Math.round(node.x)}`, `y = ${Math.round(node.y)}`);
    for (const [name, value] of Object.entries(node.values)) {
      lines.push(valueLine(name, value));
    }
  }
  for (const link of graph.links) {
    lines.push('', '[[links]]', `from = ${quoted(link.from)}`, `output = ${quoted(link.output)}`, `to = ${quoted(link.to)}`);
  }
  return { path: designPath(SCRIPT_KIND, id, map), text: `${lines.join('\n')}\n` };
}

/** The graph of a loaded script file's fields (`/api/studio/files` entry). */
export function graphOf(fields) {
  return {
    nodes: (fields.nodes ?? []).map((node) => ({
      id: node.id,
      block: node.block,
      x: node.x ?? 0,
      y: node.y ?? 0,
      values: Object.fromEntries(Object.entries(node).filter(([name]) => !NODE_KEYS.includes(name))),
    })),
    links: (fields.links ?? []).map((link) => ({ from: link.from, output: link.output, to: link.to })),
  };
}

/** A node id for a new node of `block` that `graph` does not use. */
export function newNodeId(graph, block) {
  const used = new Set(graph.nodes.map((node) => node.id));
  let number = 1;
  while (used.has(`${block}_${number}`)) {
    number += 1;
  }
  return `${block}_${number}`;
}

/** `graph` with a link from `from`'s `output` to `to`; a link to itself or a repeat changes nothing. */
export function addLink(graph, from, output, to) {
  const repeat = graph.links.some((link) => link.from === from && link.output === output && link.to === to);
  if (from === to || repeat) {
    return graph;
  }
  return { ...graph, links: [...graph.links, { from, output, to }] };
}

/** `graph` without node `id` and its links. */
export function removeNode(graph, id) {
  return {
    nodes: graph.nodes.filter((node) => node.id !== id),
    links: graph.links.filter((link) => link.from !== id && link.to !== id),
  };
}
