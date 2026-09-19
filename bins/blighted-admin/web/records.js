// SPDX-License-Identifier: AGPL-3.0-or-later
// The records page: search the client records of a table kind, change their fields in an
// override file, or save a new record made from one (admin studio spec sections 7.6, 7.7).

import { errorRows } from '/web/apply.js';
import { addFile, changedFields, overrideFile, simulationRequest, treeRows, valueText } from '/web/record-edit.js';

const RECORDS_URL = '/api/studio/records';
const FILE_URL = '/api/studio/file';
const SIMULATE_URL = '/api/studio/loot/simulate';
const NO_LOOT_URL = '/api/studio/loot/no-loot';
const LOOT_TABLE_KIND = 'loot_table';
const OBJECTIVE_KIND = 'objective';
const OBJECTIVE_TASK_KIND = 'objective_task';
const QUEST_TREE_URL = '/api/studio/quest-tree';
/** Indent of one tree level, in em. */
const TREE_INDENT_EM = 1.25;
const MOB_KIND = 'mob';
const PERCENT = 100;
const CHANCE_DECIMALS = 2;
const QUANTITY_DECIMALS = 2;
const ADMIN_HEADERS = Object.freeze({ 'X-Blighted-Admin': '1' });
const HTTP_REFUSED = 422;
/** The kinds and their names, in the order of the kind list. */
const KINDS = Object.freeze([
  ['item', 'Items'],
  ['weapon', 'Weapon stats'],
  ['armor', 'Armor stats'],
  ['consumable', 'Consumables'],
  ['perk', 'Perks'],
  ['loot_table', 'Loot tables'],
  ['mob', 'Mobs'],
  ['npc', 'NPCs'],
  ['objective', 'Quests'],
  ['objective_task', 'Quest tasks'],
]);
const KIND_PARAMETER = 'kind';
const ID_PARAMETER = 'id';

const elements = {
  status: document.getElementById('status'),
  kind: document.getElementById('kind'),
  query: document.getElementById('query'),
  hits: document.getElementById('hits'),
  problems: document.getElementById('problems'),
  record: document.getElementById('record'),
  title: document.getElementById('record-title'),
  file: document.getElementById('record-file'),
  copies: document.getElementById('copies'),
  newId: document.getElementById('new-id'),
  filter: document.getElementById('field-filter'),
  changedOnly: document.getElementById('changed-only'),
  fields: document.getElementById('fields'),
  simulate: document.getElementById('simulate'),
  simulation: document.getElementById('simulation'),
  simulationRows: document.getElementById('simulation-rows'),
  questTree: document.getElementById('quest-tree'),
  questTreeRows: document.getElementById('quest-tree-rows'),
};
/** The open record: { kind, folder, id, name, fields, field_names, files }, and the design file it edits. */
let open = null;
let editing = null;

function say(text, isError = false) {
  elements.status.classList.toggle('error', isError);
  elements.status.textContent = text;
}

function showProblems(errors) {
  elements.problems.replaceChildren(...errorRows(errors).map((row) => {
    const item = document.createElement('li');
    item.textContent = `${row.file}${row.field ? ` · ${row.field}` : ''}: ${row.problem}`;
    return item;
  }));
}

async function json(url) {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`${url}: ${response.status} ${await response.text()}`);
  }
  return response.json();
}

function showHits(kind, hits) {
  elements.hits.replaceChildren(...hits.map((hit) => {
    const item = document.createElement('li');
    item.className = 'clickable';
    item.textContent = hit.name ? `${hit.name} · ${hit.id}` : hit.id;
    item.addEventListener('click', () => openRecord(kind, hit.id).catch(fail));
    return item;
  }));
}

async function search() {
  const kind = elements.kind.value;
  const hits = await json(`${RECORDS_URL}/${kind}?q=${encodeURIComponent(elements.query.value)}`);
  showHits(kind, hits);
  say(`${hits.length} records`);
}

async function noLoot() {
  const mobs = await json(NO_LOOT_URL);
  elements.kind.value = MOB_KIND;
  showHits(MOB_KIND, mobs);
  say(`${mobs.length} mob types have no loot table.`);
}

async function simulate() {
  const form = elements.simulate.elements;
  const made = simulationRequest(open.id, form.rolls.value, form.enemy.value, form.content.value, form.tags.value);
  if (made.error) {
    say(made.error, true);
    return;
  }
  say('Simulating with the design files merged…');
  const response = await fetch(SIMULATE_URL, {
    method: 'POST',
    headers: { ...ADMIN_HEADERS, 'Content-Type': 'application/json' },
    body: JSON.stringify(made.request),
  });
  if (!response.ok) {
    say(`Not simulated: ${response.status} ${await response.text()}`, true);
    return;
  }
  const result = await response.json();
  elements.simulation.hidden = false;
  elements.simulationRows.replaceChildren(...result.items.map((item) => {
    const row = document.createElement('tr');
    const cells = [
      item.item,
      `${(item.chance * PERCENT).toFixed(CHANCE_DECIMALS)} %`,
      item.mean_quantity.toFixed(QUANTITY_DECIMALS),
    ];
    row.append(...cells.map((text, index) => {
      const cell = document.createElement('td');
      cell.textContent = text;
      if (index > 0) {
        cell.className = 'num';
      }
      return cell;
    }));
    return row;
  }));
  say(`${result.rolls} rolls of ${result.table}: ${result.items.length} items dropped.`);
}

/** The text inputs of the fields table, by field name. */
function inputs() {
  return Object.fromEntries(
    [...elements.fields.querySelectorAll('input')].map((input) => [input.name, input.value]),
  );
}

function drawFields() {
  const filter = elements.filter.value.trim().toLowerCase();
  for (const row of elements.fields.rows) {
    const name = row.dataset.field;
    const changed = row.querySelector('input').value.trim() !== '';
    row.hidden = (filter !== '' && !name.includes(filter)) || (elements.changedOnly.checked && !changed);
  }
  elements.file.textContent = editing ? editing.path : 'no design file';
}

function fieldRow(name, clientValue, designValue) {
  const row = document.createElement('tr');
  row.dataset.field = name;
  const label = document.createElement('td');
  label.textContent = name;
  const client = document.createElement('td');
  client.className = 'muted';
  client.textContent = valueText(clientValue);
  const cell = document.createElement('td');
  const input = document.createElement('input');
  input.type = 'text';
  input.name = name;
  input.className = 'wide';
  input.value = valueText(designValue);
  input.setAttribute('aria-label', `${name} design value`);
  input.addEventListener('input', drawFields);
  cell.append(input);
  row.append(label, cell, client);
  return row;
}

/** Shows `record` with the values of `file` (a design file of it, or null) in the inputs. */
function show(record, file) {
  open = record;
  editing = file;
  elements.record.hidden = false;
  elements.title.textContent = file?.action === 'add'
    ? `${file.id} (new, from ${record.name ?? record.id})`
    : `${record.name ?? record.id} · ${record.id}`;
  const values = file?.fields ?? {};
  elements.fields.replaceChildren(
    ...record.field_names.map((name) => fieldRow(name, record.fields[name], values[name])),
  );
  elements.copies.replaceChildren(...record.files.filter((copy) => copy !== file).map((copy) => {
    const button = document.createElement('button');
    button.type = 'button';
    button.textContent = copy.action === 'add' ? `Open new record ${copy.id}` : `Open ${copy.action} file`;
    button.addEventListener('click', () => show(record, copy));
    return button;
  }));
  drawFields();
  elements.simulate.hidden = record.kind !== LOOT_TABLE_KIND;
  elements.simulation.hidden = true;
  elements.questTree.hidden = record.kind !== OBJECTIVE_KIND;
  if (record.kind === OBJECTIVE_KIND) {
    showQuestTree(record.id).catch(fail);
  }
  const url = new URL(window.location.href);
  url.searchParams.set(KIND_PARAMETER, record.kind);
  url.searchParams.set(ID_PARAMETER, record.id);
  window.history.replaceState(null, '', url);
}

async function showQuestTree(objective) {
  const tree = await json(`${QUEST_TREE_URL}/${encodeURIComponent(objective)}`);
  elements.questTreeRows.replaceChildren(...treeRows(tree).map((row) => {
    const item = document.createElement('li');
    item.className = 'clickable';
    item.style.paddingLeft = `${row.depth * TREE_INDENT_EM}em`;
    item.textContent = `${row.id} · ${row.kind ?? 'no task record'}`;
    item.addEventListener('click', () => openRecord(OBJECTIVE_TASK_KIND, row.id).catch(fail));
    return item;
  }));
}

async function openRecord(kind, id) {
  const record = { kind, ...(await json(`${RECORDS_URL}/${kind}/${encodeURIComponent(id)}`)) };
  const own = record.files.find((file) => file.action === 'override' && file.id.toLowerCase() === id.toLowerCase());
  show(record, own ?? null);
  say(`Opened ${record.id}.`);
}

async function put(written) {
  const response = await fetch(`${FILE_URL}?path=${encodeURIComponent(written.path)}`, {
    method: 'PUT',
    headers: ADMIN_HEADERS,
    body: written.text,
  });
  if (response.status === HTTP_REFUSED) {
    showProblems((await response.json()).errors);
    say(`Not saved: ${written.path} has problems.`, true);
    return false;
  }
  if (!response.ok) {
    say(`Not saved: ${response.status} ${await response.text()}`, true);
    return false;
  }
  showProblems([]);
  say(`Saved ${written.path}. Apply on the Studio page to use it.`);
  return true;
}

async function save(asNew) {
  const changes = changedFields(open.fields, inputs());
  if (changes.error) {
    say(changes.error, true);
    return;
  }
  const newId = asNew ? elements.newId.value.trim() : editing?.action === 'add' ? editing.id : null;
  const written = newId === null ? overrideFile(open, changes.ok) : addFile(open, newId, changes.ok);
  if (written.error) {
    say(written.error, true);
    return;
  }
  if (newId === null && Object.keys(changes.ok).length === 0) {
    say('No field differs from the client value.', true);
    return;
  }
  if (await put(written)) {
    await openRecord(open.kind, open.id);
  }
}

async function remove() {
  if (!editing || !window.confirm(`Remove ${editing.path}? The client values come back.`)) {
    return;
  }
  const response = await fetch(`${FILE_URL}?path=${encodeURIComponent(editing.path)}`, {
    method: 'DELETE',
    headers: ADMIN_HEADERS,
  });
  if (response.status === HTTP_REFUSED) {
    showProblems((await response.json()).errors);
    say(`Not removed: other files need ${editing.path}.`, true);
    return;
  }
  if (!response.ok) {
    say(`Not removed: ${response.status} ${await response.text()}`, true);
    return;
  }
  say(`Removed ${editing.path}.`);
  await openRecord(open.kind, open.id);
}

function fail(error) {
  say(String(error.message ?? error), true);
}

elements.kind.replaceChildren(...KINDS.map(([kind, name]) => {
  const option = document.createElement('option');
  option.value = kind;
  option.textContent = name;
  return option;
}));
document.getElementById('search').addEventListener('submit', (event) => {
  event.preventDefault();
  search().catch(fail);
});
document.getElementById('save').addEventListener('click', () => save(false).catch(fail));
document.getElementById('save-new').addEventListener('click', () => save(true).catch(fail));
document.getElementById('remove').addEventListener('click', () => remove().catch(fail));
elements.filter.addEventListener('input', drawFields);
document.getElementById('no-loot').addEventListener('click', () => noLoot().catch(fail));
elements.simulate.addEventListener('submit', (event) => {
  event.preventDefault();
  simulate().catch(fail);
});
elements.changedOnly.addEventListener('change', drawFields);

const start = new URL(window.location.href).searchParams;
if (start.get(KIND_PARAMETER) && start.get(ID_PARAMETER)) {
  elements.kind.value = start.get(KIND_PARAMETER);
  openRecord(start.get(KIND_PARAMETER), start.get(ID_PARAMETER)).catch(fail);
}
say('Search for a record. The first search reads the client data and takes a few seconds.');
