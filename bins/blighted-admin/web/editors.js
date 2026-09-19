// SPDX-License-Identifier: AGPL-3.0-or-later
// The studio editors: forms for AI profiles, patrol paths and spawn groups, and a map to place
// their points (right-click adds one). Each Save writes one design file through the file route.

import { BEHAVIOURS, PATH_MODES, PROFILE_NUMBERS, designPath, fieldsOf, groupFile, pathFile, profileFile } from './design.js';
import { WorldMap } from './map.js';
import { SIGNAL } from './palette.js';
import { boxAround, screenToWorld } from './view.js';

const FILE_URL = '/api/studio/file';
const HEIGHT_URL = '/api/world/height';
const ADMIN_HEADERS = Object.freeze({ 'X-Blighted-Admin': '1' });
const HTTP_REFUSED = 422;
/** Points are kept to a tenth of a metre, as point keys are. */
const POINT_SCALE = 10;
/** Metres around a "Go" point or a file's points when the map shows them. */
const GOTO_MARGIN_M = 60;
const PATH_COLOUR = SIGNAL.YELLOW;
const POINT_COLOUR = SIGNAL.RED;
const PROFILE = 'ai_profile';
const PATH = 'path';
const GROUP = 'spawn_group';
const WRITERS = Object.freeze({ [PROFILE]: profileFile, [PATH]: pathFile, [GROUP]: groupFile });

function tenth(value) {
  return Math.round(value * POINT_SCALE) / POINT_SCALE;
}

function fileUrl(path) {
  return `${FILE_URL}?path=${encodeURIComponent(path)}`;
}

function options(select, values) {
  select.replaceChildren(...values.map((value) => {
    const option = document.createElement('option');
    option.value = value;
    option.textContent = value;
    return option;
  }));
}

function idOptions(list, ids) {
  list.replaceChildren(...ids.map((id) => {
    const option = document.createElement('option');
    option.value = id;
    return option;
  }));
}

/**
 * Starts the editors. `info` is `/api/world/info`; `showProblems` shows design errors;
 * `onSaved` is called after a file is written or removed.
 */
export function startEditors(info, { showProblems, onSaved }) {
  const status = document.getElementById('editor-status');
  const forms = Object.fromEntries([PROFILE, PATH, GROUP].map((kind) => [kind, document.getElementById(`form-${kind}`)]));
  const tabs = [...document.querySelectorAll('[data-editor]')];
  const pointsText = document.getElementById('editor-points');
  const pointTools = [pointsText, document.getElementById('editor-undo'), document.getElementById('editor-clear')];
  const mapBox = document.getElementById('editor-map-box');
  const numbersBox = document.getElementById('profile-numbers');
  let kind = PROFILE;
  let points = [];
  /** The path of the file the form was loaded from; null for a new file. */
  let loaded = null;
  /** The level of the file in the form; the map shows only `info.level`. */
  let level = info.level;

  options(forms[PROFILE].elements.behaviour, BEHAVIOURS);
  options(forms[PATH].elements.mode, PATH_MODES);
  numbersBox.replaceChildren(...PROFILE_NUMBERS.map(({ name, whole }) => {
    const input = document.createElement('input');
    input.type = 'text';
    input.name = name;
    input.inputMode = whole ? 'numeric' : 'decimal';
    input.placeholder = name;
    input.setAttribute('aria-label', `${name} (optional)`);
    input.className = 'wide';
    return input;
  }));

  const map = new WorldMap(document.getElementById('editor-map'), info);

  function say(text, isError = false) {
    status.classList.toggle('error', isError);
    status.textContent = text;
  }

  function drawPoints() {
    pointsText.textContent = `${points.length} points on ${level}`;
    map.setPaths(kind === PATH ? [{ key: 'edit', color: PATH_COLOUR, points }] : []);
    map.setPoints(points.map((point, index) => ({
      key: `edit-${index}`,
      x: point.x,
      y: point.y,
      color: POINT_COLOUR,
      label: String(index + 1),
    })));
  }

  function show(next) {
    kind = next;
    for (const [formKind, form] of Object.entries(forms)) {
      form.hidden = formKind !== kind;
    }
    for (const tab of tabs) {
      tab.classList.toggle('active', tab.dataset.editor === kind);
    }
    const placesPoints = kind !== PROFILE;
    for (const tool of pointTools) {
      tool.hidden = !placesPoints;
    }
    mapBox.hidden = !placesPoints;
    if (placesPoints) {
      map.resize();
    }
    drawPoints();
  }

  function clear(next) {
    forms[next].reset();
    points = [];
    loaded = null;
    level = info.level;
    show(next);
    say('New file.');
  }

  function values() {
    const elements = forms[kind].elements;
    const text = (name) => elements[name].value;
    switch (kind) {
      case PROFILE:
        return {
          id: text('id').trim(),
          behaviour: text('behaviour'),
          numbers: Object.fromEntries(PROFILE_NUMBERS.map(({ name }) => [name, text(name)])),
        };
      case PATH:
        return { id: text('id').trim(), map: level, mode: text('mode'), waits: text('waits'), points };
      default:
        return {
          id: text('id').trim(),
          map: level,
          vitals: text('vitals'),
          level: text('level'),
          profile: text('profile'),
          path: text('path'),
          points,
        };
    }
  }

  function fill(form) {
    const elements = forms[kind].elements;
    const set = (name, value) => {
      elements[name].value = value ?? '';
    };
    set('id', form.id);
    if (kind === PROFILE) {
      set('behaviour', form.behaviour);
      for (const { name } of PROFILE_NUMBERS) {
        set(name, form.numbers[name]);
      }
    } else if (kind === PATH) {
      set('mode', form.mode);
      set('waits', form.waits);
    } else {
      for (const name of ['vitals', 'level', 'profile', 'path']) {
        set(name, form[name]);
      }
    }
  }

  /** Adds the ground point under the screen pixel (`px`, `py`) of the map. */
  async function addPoint(px, py) {
    if (level !== info.level) {
      say(`The map shows ${info.level}; points of ${level} cannot be placed on it.`, true);
      return;
    }
    const { x, y } = screenToWorld(map.currentView(), px, py);
    const response = await fetch(`${HEIGHT_URL}?x=${x}&y=${y}`);
    const { z } = await response.json();
    if (z === null) {
      say('No ground there.', true);
      return;
    }
    points.push({ x: tenth(x), y: tenth(y), z: tenth(z) });
    drawPoints();
    say(`Point ${points.length} at ${tenth(x)}, ${tenth(y)}, ${tenth(z)}.`);
  }

  async function save() {
    const written = WRITERS[kind](values());
    if (written.error) {
      say(written.error, true);
      return;
    }
    const response = await fetch(fileUrl(written.path), { method: 'PUT', headers: ADMIN_HEADERS, body: written.text });
    if (response.status === HTTP_REFUSED) {
      showProblems((await response.json()).errors);
      say(`Not saved: ${written.path} has problems (see Problems).`, true);
      return;
    }
    if (!response.ok) {
      say(`Not saved: ${response.status} ${await response.text()}`, true);
      return;
    }
    if (loaded && loaded !== written.path) {
      say(`Saved ${written.path} as a new file; ${loaded} is still there.`);
    } else {
      say(`Saved ${written.path}. Apply to use it.`);
    }
    loaded = written.path;
    await onSaved();
  }

  async function remove() {
    const path = loaded ?? designPath(kind, values().id, kind === PROFILE ? null : level);
    if (!window.confirm(`Remove ${path}?`)) {
      return;
    }
    const response = await fetch(fileUrl(path), { method: 'DELETE', headers: ADMIN_HEADERS });
    if (response.status === HTTP_REFUSED) {
      showProblems((await response.json()).errors);
      say(`Not removed: other files need ${path} (see Problems).`, true);
      return;
    }
    if (!response.ok) {
      say(`Not removed: ${response.status} ${await response.text()}`, true);
      return;
    }
    clear(kind);
    say(`Removed ${path}. Apply to use the change.`);
    await onSaved();
  }

  const failed = (error) => say(String(error.message ?? error), true);
  for (const tab of tabs) {
    tab.addEventListener('click', () => {
      // Another kind is another file; the points stay, so a group can reuse a path's points.
      if (tab.dataset.editor !== kind) {
        loaded = null;
        level = info.level;
      }
      show(tab.dataset.editor);
    });
  }
  document.getElementById('editor-map').addEventListener('contextmenu', (event) => {
    event.preventDefault();
    addPoint(event.offsetX, event.offsetY).catch(failed);
  });
  document.getElementById('editor-undo').addEventListener('click', () => {
    points.pop();
    drawPoints();
  });
  document.getElementById('editor-clear').addEventListener('click', () => {
    points = [];
    drawPoints();
  });
  document.getElementById('editor-goto').addEventListener('submit', (event) => {
    event.preventDefault();
    const fields = event.target.elements;
    const x = Number(fields.x.value);
    const y = Number(fields.y.value);
    if (fields.x.value.trim() === '' || fields.y.value.trim() === '' || !Number.isFinite(x) || !Number.isFinite(y)) {
      say('x and y must be numbers (metres).', true);
      return;
    }
    map.fit(boxAround([{ x, y }], GOTO_MARGIN_M));
  });
  document.getElementById('editor-zoom-in').addEventListener('click', () => map.zoom(1));
  document.getElementById('editor-zoom-out').addEventListener('click', () => map.zoom(-1));
  document.getElementById('editor-fit').addEventListener('click', () => map.fit());
  document.getElementById('editor-new').addEventListener('click', () => clear(kind));
  document.getElementById('editor-save').addEventListener('click', () => save().catch(failed));
  document.getElementById('editor-delete').addEventListener('click', () => remove().catch(failed));
  show(PROFILE);
  say('Right-click the map to add a point to a path or spawn group.');

  return {
    /** Fills the id lists of the spawn group form from the design files. */
    setFiles(files) {
      idOptions(document.getElementById('profile-ids'), files.filter((file) => file.kind === PROFILE).map((file) => file.id));
      idOptions(
        document.getElementById('path-ids'),
        files.filter((file) => file.kind === PATH && file.map === info.level).map((file) => file.id),
      );
    },
    /** True for the files an editor opens. */
    opens(file) {
      return file.kind in WRITERS;
    },
    /** Opens a design file of an editor's kind. */
    open(file) {
      show(file.kind);
      const form = fieldsOf(file);
      fill(form);
      points = form.points ?? [];
      loaded = file.path;
      level = file.map ?? info.level;
      drawPoints();
      if (points.length > 0 && level === info.level) {
        map.fit(boxAround(points, GOTO_MARGIN_M));
      }
      const elsewhere = file.map && file.map !== info.level ? ` Its level is ${file.map}; the map shows ${info.level}.` : '';
      say(`Opened ${file.path}.${elsewhere}`);
    },
  };
}
