// SPDX-License-Identifier: AGPL-3.0-or-later
// The live 3D admin page: follows a selected player over the real terrain, with the in-game map
// laid on the ground, the sea at its level, and every player and mob the server sends for the
// render radius. The pure helpers are exported so node:test covers them without three.js.

import { visibleTiles } from './view.js';

const VIEW_URL = '/api/live/view';
const STREAM_URL = '/api/live/stream';
const GRID_URL = '/api/world/height-grid';
const INFO_URL = '/api/world/info';
const TILE_URL = '/api/world/tiles';

/** Half side of the terrain square around the followed player, in metres. */
const TERRAIN_HALF_M = 384;
/** Spacing of two height samples, in metres. */
const TERRAIN_STEP_M = 3;
/** Pixels per metre of the ground picture. */
const GROUND_PX_PER_M = 2;
/** The terrain loads again when the player is this far from its centre, in metres. */
const TERRAIN_RELOAD_M = TERRAIN_HALF_M / 2;
/** Depth used for a terrain sample with no height (off the terrain), below the sea, in metres. */
const NO_GROUND_DEPTH_M = 5;
/** Colour of the ground picture where a map tile is missing. */
const GROUND_FALLBACK = '#8a8061';
/** The sea. */
const SEA_COLOUR = 0x2d6e9e;
const SEA_OPACITY = 0.6;
/** The sea plane is this many terrain sides wide. */
const SEA_SIDES = 8;

/** The dead mob state in `MobState` order (`asleep, idle, chase, attack, return, dead`). */
export const MOB_STATE_DEAD = 5;
/** Opacity of a dead entity. */
const DEAD_OPACITY = 0.35;
/** Marker sizes, in metres. */
const MARKER_RADIUS_M = 0.5;
const PLAYER_HEIGHT_M = 2;
const MOB_HEIGHT_M = 1.6;
/** Height of the label above the top of the marker, in metres. */
const LABEL_GAP_M = 1;
/** Label texture size, in pixels, and its size in the scene, in metres. */
const LABEL_PX = [256, 64];
const LABEL_M = [6, 1.5];
/** Marker colours. */
const PLAYER_COLOUR = 0x4a90e2;
const BOT_COLOUR = 0x9aa5b1;
const MOB_COLOUR = 0xd43f3f;
const FOLLOWED_COLOUR = 0xffd23f;
/** Ring on the ground under the followed player, in metres. */
const RING_INNER_M = 1.2;
const RING_OUTER_M = 1.6;

/** Camera: nearest and farthest distance, and the start offset behind and above the player. */
const MIN_ZOOM_M = 5;
const MAX_ZOOM_M = 1500;
const START_BACK_M = 70;
const START_UP_M = 55;
const FAR_PLANE_M = 20000;
const FIELD_OF_VIEW_DEG = 55;
/** Pixel ratio cap. */
const MAX_PIXEL_RATIO = 2;

/** Gap between two states when none is measured yet, and its bounds, in milliseconds. */
const DEFAULT_STATE_GAP_MS = 250;
const MIN_STATE_GAP_MS = 50;
const MAX_STATE_GAP_MS = 1000;
/** The player list refresh, in milliseconds. */
const PICKER_REFRESH_MS = 5000;

/**
 * The scene point of a world point. The world is x east, y north, z up; the scene is y up and
 * right-handed, so north goes to -z (a plain swap of y and z would mirror the map).
 * @returns {{ x: number, y: number, z: number }}
 */
export function toScene(x, y, z) {
  return { x, y: z, z: -y };
}

/**
 * Where an entity is drawn `elapsedMs` after a state that moved it from `from` to `to`, when the
 * states come `gapMs` apart: a straight line, then it stays at `to`.
 */
export function lerpPosition(from, to, elapsedMs, gapMs) {
  const t = gapMs > 0 ? Math.max(0, Math.min(1, elapsedMs / gapMs)) : 1;
  return {
    x: from.x + (to.x - from.x) * t,
    y: from.y + (to.y - from.y) * t,
    z: from.z + (to.z - from.z) * t,
  };
}

/** The measured gap between two states, kept inside the bounds. */
export function stateGap(previousMs, nowMs) {
  if (previousMs == null) return DEFAULT_STATE_GAP_MS;
  return Math.max(MIN_STATE_GAP_MS, Math.min(MAX_STATE_GAP_MS, nowMs - previousMs));
}

/** True when an entity of `kind` ('player' or 'mob') is dead. */
export function isDead(entity, kind) {
  return kind === 'mob' ? entity.state === MOB_STATE_DEAD || entity.health === 0 : entity.dead;
}

/** The text of an entity's label. */
export function labelText(entity, kind) {
  const name = kind === 'mob' ? `Mob ${entity.entity}` : entity.name;
  return `${name}  ${entity.health ?? 0}/${entity.max_health ?? 0}`;
}

/**
 * Fight lines from each live attacker to its target when the target is present and alive.
 * @returns {Array<{ fromEntity: number, toEntity: number }>}
 */
export function fightLines(view) {
  const players = view.players ?? [];
  const mobs = view.mobs ?? [];
  const alive = new Set();
  for (const player of players) {
    if (!isDead(player, 'player')) alive.add(player.entity);
  }
  for (const mob of mobs) {
    if (!isDead(mob, 'mob')) alive.add(mob.entity);
  }
  const lines = [];
  for (const attacker of [...players, ...mobs]) {
    if (attacker.target_entity == null) continue;
    if (!alive.has(attacker.entity) || !alive.has(attacker.target_entity)) continue;
    lines.push({ fromEntity: attacker.entity, toEntity: attacker.target_entity });
  }
  return lines;
}

/**
 * The terrain square around (cx, cy): its south-west corner, side and sample count. The corner
 * snaps to the sample step so a reload lines up with the last one.
 */
export function terrainSquare(cx, cy) {
  const x0 = Math.floor((cx - TERRAIN_HALF_M) / TERRAIN_STEP_M) * TERRAIN_STEP_M;
  const y0 = Math.floor((cy - TERRAIN_HALF_M) / TERRAIN_STEP_M) * TERRAIN_STEP_M;
  return { x0, y0, side: 2 * TERRAIN_HALF_M };
}

/**
 * The finest map tiles over a square, each with where it goes on the ground picture (pixels from
 * the north-west corner; the picture is north up).
 */
export function groundTiles(tilesIndex, square, pxPerM) {
  const level = tilesIndex.levels[0].level;
  const view = {
    centerX: square.x0 + square.side / 2,
    centerY: square.y0 + square.side / 2,
    width: square.side,
    height: square.side,
    metresPerPixel: 1,
  };
  const north = square.y0 + square.side;
  return visibleTiles(view, tilesIndex, level).map((tile) => ({
    tile,
    px: (tile.west - square.x0) * pxPerM,
    py: (north - (tile.south + tile.size)) * pxPerM,
    sizePx: tile.size * pxPerM,
  }));
}

/** The height of grid row `row` counted from the north edge, with no-ground samples filled. */
export function northRowHeight(heights, size, row, column, fill) {
  const height = heights?.[size - 1 - row]?.[column];
  return height == null ? fill : height;
}

async function fetchJson(url) {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`${url}: ${response.status} ${await response.text()}`);
  }
  return response.json();
}

function labelCanvas(text, ratio) {
  const canvas = document.createElement('canvas');
  [canvas.width, canvas.height] = LABEL_PX;
  const context = canvas.getContext('2d');
  context.fillStyle = 'rgba(0, 0, 0, 0.65)';
  context.fillRect(0, 0, canvas.width, canvas.height);
  context.font = 'bold 26px system-ui, sans-serif';
  context.textAlign = 'center';
  context.fillStyle = '#fff';
  context.fillText(text, canvas.width / 2, 30);
  context.fillStyle = '#333';
  context.fillRect(10, 42, canvas.width - 20, 12);
  context.fillStyle = ratio > 0.5 ? '#4caf50' : ratio > 0.2 ? '#ffb300' : '#e53935';
  context.fillRect(10, 42, (canvas.width - 20) * ratio, 12);
  return canvas;
}

/** The browser entry: the scene, the terrain, the stream and the controls. */
export async function run(THREE, { OrbitControls }) {
  const canvas = document.getElementById('scene');
  const statusElement = document.getElementById('status');
  const playerSelect = document.getElementById('player');
  const say = (text, isError = false) => {
    statusElement.classList.toggle('error', isError);
    statusElement.textContent = text;
  };

  const info = await fetchJson(INFO_URL);

  const renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio ?? 1, MAX_PIXEL_RATIO));
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x9cc3de);
  const camera = new THREE.PerspectiveCamera(FIELD_OF_VIEW_DEG, 1, 0.5, FAR_PLANE_M);
  const controls = new OrbitControls(camera, renderer.domElement);
  controls.minDistance = MIN_ZOOM_M;
  controls.maxDistance = MAX_ZOOM_M;
  controls.maxPolarAngle = Math.PI / 2.05;
  controls.enableDamping = true;
  scene.add(new THREE.HemisphereLight(0xffffff, 0x3a4a5a, 1.6));
  const sun = new THREE.DirectionalLight(0xfff2cc, 1.2);
  sun.position.set(300, 600, 200);
  scene.add(sun);

  const resize = () => {
    const rect = canvas.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return;
    renderer.setSize(rect.width, rect.height, false);
    camera.aspect = rect.width / rect.height;
    camera.updateProjectionMatrix();
  };
  new ResizeObserver(resize).observe(canvas);

  const sea = new THREE.Mesh(
    new THREE.PlaneGeometry(1, 1),
    new THREE.MeshLambertMaterial({ color: SEA_COLOUR, transparent: true, opacity: SEA_OPACITY }),
  );
  sea.geometry.rotateX(-Math.PI / 2);
  sea.visible = false;
  scene.add(sea);

  let terrain = null;
  let terrainCentre = null;
  let terrainLoading = false;

  async function loadTerrain(cx, cy) {
    terrainLoading = true;
    try {
      const square = terrainSquare(cx, cy);
      const half = square.side / 2;
      const grid = await fetchJson(
        `${GRID_URL}?x=${square.x0 + half}&y=${square.y0 + half}&half=${half}&step=${TERRAIN_STEP_M}`,
      );
      const { size, heights } = grid;
      const fill = info.ocean_level - NO_GROUND_DEPTH_M;
      const geometry = new THREE.PlaneGeometry(square.side, square.side, size - 1, size - 1);
      geometry.rotateX(-Math.PI / 2);
      const positions = geometry.attributes.position;
      for (let row = 0; row < size; row += 1) {
        for (let column = 0; column < size; column += 1) {
          positions.setY(row * size + column, northRowHeight(heights, size, row, column, fill));
        }
      }
      geometry.computeVertexNormals();

      const picture = document.createElement('canvas');
      picture.width = picture.height = square.side * GROUND_PX_PER_M;
      const context = picture.getContext('2d');
      context.fillStyle = GROUND_FALLBACK;
      context.fillRect(0, 0, picture.width, picture.height);
      const texture = new THREE.CanvasTexture(picture);
      texture.colorSpace = THREE.SRGBColorSpace;
      texture.anisotropy = renderer.capabilities.getMaxAnisotropy();
      if (info.map_tiles) {
        for (const { tile, px, py, sizePx } of groundTiles(info.tiles, square, GROUND_PX_PER_M)) {
          const image = new Image();
          image.onload = () => {
            context.drawImage(image, px, py, sizePx, sizePx);
            texture.needsUpdate = true;
          };
          image.src = `${TILE_URL}/${tile.level}/${tile.row}/${tile.column}`;
        }
      }

      const mesh = new THREE.Mesh(geometry, new THREE.MeshLambertMaterial({ map: texture }));
      const centre = toScene(square.x0 + half, square.y0 + half, 0);
      mesh.position.set(centre.x, 0, centre.z);
      if (terrain) {
        scene.remove(terrain);
        terrain.geometry.dispose();
        terrain.material.map.dispose();
        terrain.material.dispose();
      }
      terrain = mesh;
      scene.add(terrain);

      const seaSide = square.side * SEA_SIDES;
      sea.scale.set(seaSide, 1, seaSide);
      sea.position.set(centre.x, info.ocean_level, centre.z);
      sea.visible = true;
      terrainCentre = { x: cx, y: cy };
    } finally {
      terrainLoading = false;
    }
  }

  const markerGroup = new THREE.Group();
  scene.add(markerGroup);
  const markers = new Map();
  const ring = new THREE.Mesh(
    new THREE.RingGeometry(RING_INNER_M, RING_OUTER_M, 32),
    new THREE.MeshBasicMaterial({ color: FOLLOWED_COLOUR, side: THREE.DoubleSide }),
  );
  ring.geometry.rotateX(-Math.PI / 2);
  ring.visible = false;
  scene.add(ring);
  const lines = new THREE.LineSegments(
    new THREE.BufferGeometry(),
    new THREE.LineBasicMaterial({ color: 0xffffff }),
  );
  scene.add(lines);
  let linePairs = [];

  function colourOf(entity, kind) {
    if (kind === 'mob') return MOB_COLOUR;
    return entity.bot ? BOT_COLOUR : PLAYER_COLOUR;
  }

  function newMarker(entity, kind) {
    const height = kind === 'mob' ? MOB_HEIGHT_M : PLAYER_HEIGHT_M;
    const body = new THREE.Mesh(
      new THREE.CapsuleGeometry(MARKER_RADIUS_M, height - 2 * MARKER_RADIUS_M, 4, 12),
      new THREE.MeshLambertMaterial({ color: colourOf(entity, kind), transparent: true }),
    );
    body.userData = { entity: entity.entity, kind };
    const label = new THREE.Sprite(new THREE.SpriteMaterial({ transparent: true, depthTest: false }));
    label.scale.set(LABEL_M[0], LABEL_M[1], 1);
    label.renderOrder = 1;
    markerGroup.add(body, label);
    const at = toScene(entity.x, entity.y, entity.z);
    return { body, label, kind, height, from: at, to: at, shown: at, labelKey: '' };
  }

  function dropMarker(entity) {
    const marker = markers.get(entity);
    markerGroup.remove(marker.body, marker.label);
    marker.body.geometry.dispose();
    marker.body.material.dispose();
    marker.label.material.map?.dispose();
    marker.label.material.dispose();
    markers.delete(entity);
  }

  function setLabel(marker, entity) {
    const text = labelText(entity, marker.kind);
    if (text === marker.labelKey) return;
    const ratio = entity.max_health > 0 ? Math.min(1, entity.health / entity.max_health) : 0;
    marker.label.material.map?.dispose();
    marker.label.material.map = new THREE.CanvasTexture(labelCanvas(text, ratio));
    marker.label.material.needsUpdate = true;
    marker.labelKey = text;
  }

  let focus = null;
  let lastStateMs = null;
  let stateAtMs = performance.now();
  let gapMs = DEFAULT_STATE_GAP_MS;
  let source = null;

  function onState(view) {
    const now = performance.now();
    gapMs = stateGap(lastStateMs, now);
    lastStateMs = now;
    stateAtMs = now;
    const seen = new Set();
    const place = (entity, kind) => {
      seen.add(entity.entity);
      const marker = markers.get(entity.entity) ?? newMarker(entity, kind);
      markers.set(entity.entity, marker);
      marker.from = marker.shown;
      marker.to = toScene(entity.x, entity.y, entity.z);
      marker.body.material.opacity = isDead(entity, kind) ? DEAD_OPACITY : 1;
      const followed = kind === 'player' && entity.entity === focus;
      marker.body.material.color.setHex(followed ? FOLLOWED_COLOUR : colourOf(entity, kind));
      setLabel(marker, entity);
    };
    for (const player of view.players ?? []) place(player, 'player');
    for (const mob of view.mobs ?? []) place(mob, 'mob');
    for (const entity of [...markers.keys()]) {
      if (!seen.has(entity)) dropMarker(entity);
    }
    linePairs = fightLines(view);

    const self = (view.players ?? []).find((player) => player.entity === focus);
    if (self) {
      const moved = terrainCentre && Math.hypot(self.x - terrainCentre.x, self.y - terrainCentre.y);
      if (!terrainLoading && (!terrainCentre || moved > TERRAIN_RELOAD_M)) {
        loadTerrain(self.x, self.y).catch((error) => say(error.message, true));
      }
      const where = `${self.x.toFixed(1)}, ${self.y.toFixed(1)}, ${self.z.toFixed(1)}`;
      say(`${self.name} at ${where} · HP ${self.health}/${self.max_health} · `
        + `${(view.players ?? []).length} players · ${(view.mobs ?? []).length} mobs near`);
    } else if (focus != null) {
      say('The player is not in the world.', true);
    }
  }

  function placeCamera() {
    const marker = markers.get(focus);
    if (!marker) return;
    const at = marker.shown;
    controls.target.set(at.x, at.y + marker.height / 2, at.z);
    camera.position.set(at.x, at.y + START_UP_M, at.z + START_BACK_M);
    controls.update();
  }

  function openStream() {
    if (source) source.close();
    source = new EventSource(`${STREAM_URL}?focus=${focus}`);
    let first = true;
    source.addEventListener('state', (event) => {
      try {
        onState(JSON.parse(event.data));
        if (first) {
          first = false;
          placeCamera();
        }
      } catch (error) {
        say(error.message, true);
      }
    });
    source.addEventListener('error', () => say('Live stream lost; it reconnects by itself.', true));
  }

  function follow(entity) {
    focus = entity;
    playerSelect.value = String(entity);
    for (const id of [...markers.keys()]) dropMarker(id);
    terrainCentre = null;
    lastStateMs = null;
    openStream();
  }

  async function refreshPicker() {
    const view = await fetchJson(VIEW_URL);
    const players = [...(view.players ?? [])].sort((a, b) => Number(a.bot) - Number(b.bot)
      || a.name.localeCompare(b.name));
    playerSelect.replaceChildren(...players.map((player) => {
      const option = document.createElement('option');
      option.value = String(player.entity);
      option.textContent = `${player.bot ? 'Bot · ' : ''}${player.name} · ${player.entity}`;
      return option;
    }));
    if (focus != null && players.some((player) => player.entity === focus)) {
      playerSelect.value = String(focus);
    } else if (focus == null && players.length > 0) {
      follow((players.find((player) => !player.bot) ?? players[0]).entity);
    } else if (players.length === 0) {
      say('No player is in the world.');
    }
  }

  playerSelect.addEventListener('change', () => {
    const entity = Number(playerSelect.value);
    if (Number.isFinite(entity)) follow(entity);
  });
  window.addEventListener('keydown', (event) => {
    if (event.key.toLowerCase() === 'r' && event.target === document.body) placeCamera();
  });

  const raycaster = new THREE.Raycaster();
  let pressAt = null;
  canvas.addEventListener('pointerdown', (event) => {
    pressAt = { x: event.clientX, y: event.clientY };
  });
  canvas.addEventListener('pointerup', (event) => {
    if (!pressAt || Math.hypot(event.clientX - pressAt.x, event.clientY - pressAt.y) > 6) return;
    const rect = canvas.getBoundingClientRect();
    const pointer = new THREE.Vector2(
      ((event.clientX - rect.left) / rect.width) * 2 - 1,
      -((event.clientY - rect.top) / rect.height) * 2 + 1,
    );
    raycaster.setFromCamera(pointer, camera);
    const bodies = [...markers.values()].map((marker) => marker.body);
    const hit = raycaster.intersectObjects(bodies)[0];
    if (hit?.object.userData.kind === 'player' && hit.object.userData.entity !== focus) {
      follow(hit.object.userData.entity);
    }
  });

  function frame() {
    const now = performance.now();
    const followed = markers.get(focus);
    const before = followed ? { ...followed.shown } : null;
    for (const marker of markers.values()) {
      const at = lerpPosition(marker.from, marker.to, now - stateAtMs, gapMs);
      marker.shown = at;
      marker.body.position.set(at.x, at.y + marker.height / 2, at.z);
      marker.label.position.set(at.x, at.y + marker.height + LABEL_GAP_M, at.z);
    }
    if (followed && before) {
      const dx = followed.shown.x - before.x;
      const dy = followed.shown.y - before.y;
      const dz = followed.shown.z - before.z;
      controls.target.x += dx;
      controls.target.y += dy;
      controls.target.z += dz;
      camera.position.x += dx;
      camera.position.y += dy;
      camera.position.z += dz;
      ring.position.set(followed.shown.x, followed.shown.y + 0.05, followed.shown.z);
      ring.visible = true;
    } else {
      ring.visible = false;
    }
    const points = [];
    for (const { fromEntity, toEntity } of linePairs) {
      const from = markers.get(fromEntity);
      const to = markers.get(toEntity);
      if (!from || !to) continue;
      points.push(from.body.position.clone(), to.body.position.clone());
    }
    lines.geometry.dispose();
    lines.geometry = new THREE.BufferGeometry().setFromPoints(points);
    controls.update();
    renderer.render(scene, camera);
    requestAnimationFrame(frame);
  }

  resize();
  await refreshPicker();
  setInterval(() => refreshPicker().catch((error) => say(error.message, true)), PICKER_REFRESH_MS);
  frame();
}

if (typeof window !== 'undefined' && typeof document !== 'undefined') {
  const [THREE, controls] = await Promise.all([
    import('three'),
    import('three/addons/controls/OrbitControls.js'),
  ]);
  run(THREE, controls).catch((error) => {
    const statusElement = document.getElementById('status');
    statusElement.classList.add('error');
    statusElement.textContent = error.message;
  });
}
