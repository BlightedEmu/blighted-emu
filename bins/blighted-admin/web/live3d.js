// SPDX-License-Identifier: AGPL-3.0-or-later
// The live 3D admin page: orbits a selected player, draws terrain and every entity inside the
// server's render-radius square. Keeps logic pure in exported helpers so node:test can cover
// the parts that are not three.js.

const VIEW_URL = '/api/live/view';
const STREAM_URL = '/api/live/stream';
const GRID_URL = '/api/world/height-grid';

/** The half-side of the terrain square around the selected player, in metres. */
const TERRAIN_HALF_M = 250;
/** The spacing between two height samples, in metres. */
const TERRAIN_STEP_M = 2;
/** The dead mob state in `MobState` order (`asleep, idle, chase, attack, return, dead`). */
const MOB_STATE_DEAD = 5;
/** Opacity of a dead entity's capsule. */
const DEAD_OPACITY = 0.4;
/** Vertical offset of a name/HP sprite above the capsule, in metres. */
const SPRITE_Y_M = 2.4;
/** Side of the sprite texture, in pixels. */
const SPRITE_TEXTURE_PX = 256;
/** Width of a capsule, in metres. */
const CAPSULE_RADIUS_M = 0.3;
/** Height of a player capsule, in metres. */
const PLAYER_HEIGHT_M = 1.8;
/** Height of a mob capsule, in metres. */
const MOB_HEIGHT_M = 1.2;
/** The nearest and farthest camera distance, in metres. */
const MIN_ZOOM_M = 5;
const MAX_ZOOM_M = 1500;
/** The eye offset above the player, metres, when the camera first opens. */
const START_EYE_M = 20;
/** Pixel ratio cap: cheap devices do not render 3x. */
const MAX_PIXEL_RATIO = 2;

/**
 * Keep the newer of two SSE payloads. An older event that arrives after a newer one is dropped
 * so the browser does not fall behind.
 * @param {object | null} prev The current state.
 * @param {object} event The incoming event payload.
 * @returns {object} Whichever has the larger `time_ms`.
 */
export function reduceState(prev, event) {
  if (!prev) return event;
  const prevMs = Number(prev?.tiles?.sims ?? 0) || 0;
  const prevTime = Number(prev?.events?.[0]?.time_ms ?? 0) || 0;
  const eventTime = Number(event?.events?.[0]?.time_ms ?? 0) || 0;
  if (eventTime < prevTime) return prev;
  // Fallback: no event timestamp — compare the top-level time_ms the server may add.
  const prevTop = Number(prev?.time_ms ?? prevTime ?? prevMs) || 0;
  const eventTop = Number(event?.time_ms ?? eventTime ?? 0) || 0;
  return eventTop >= prevTop ? event : prev;
}

/**
 * Fight lines from each attacker to its target, when both are present and alive.
 * @param {{ players?: Array<object>, mobs?: Array<object> }} view
 * @returns {Array<{ fromEntity: number, toEntity: number }>}
 */
export function fightLines(view) {
  const players = view.players ?? [];
  const mobs = view.mobs ?? [];
  const alive = new Map();
  for (const player of players) {
    if (!player.dead) alive.set(player.entity, player);
  }
  for (const mob of mobs) {
    if (mob.state !== MOB_STATE_DEAD && mob.health > 0) alive.set(mob.entity, mob);
  }
  const lines = [];
  const push = (from) => {
    if (from.target_entity == null) return;
    if (from.dead || from.state === MOB_STATE_DEAD) return;
    if (!alive.has(from.target_entity)) return;
    lines.push({ fromEntity: from.entity, toEntity: from.target_entity });
  };
  for (const player of players) push(player);
  for (const mob of mobs) push(mob);
  return lines;
}

/**
 * Terrain color from slope: green on the flat, brown on the steep. Each channel is in [0, 1].
 * @param {number} slope Slope magnitude in metres per metre; clamped to [0, 1].
 * @returns {[number, number, number]} r, g, b channels.
 */
export function terrainColor(slope) {
  const t = Math.max(0, Math.min(1, slope));
  // Interpolate from a grass green to a rock brown.
  const grass = [0.33, 0.52, 0.28];
  const rock = [0.45, 0.35, 0.25];
  return [
    grass[0] * (1 - t) + rock[0] * t,
    grass[1] * (1 - t) + rock[1] * t,
    grass[2] * (1 - t) + rock[2] * t,
  ];
}

async function fetchJson(url) {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`${url}: ${response.status} ${await response.text()}`);
  }
  return response.json();
}

function hpSpriteDataUrl(name, hp, maxHp, ratio) {
  const canvas = document.createElement('canvas');
  canvas.width = SPRITE_TEXTURE_PX;
  canvas.height = SPRITE_TEXTURE_PX / 4;
  const context = canvas.getContext('2d');
  context.font = 'bold 36px system-ui, sans-serif';
  context.textAlign = 'center';
  context.fillStyle = 'rgba(0, 0, 0, 0.6)';
  context.fillRect(0, 0, canvas.width, canvas.height);
  context.fillStyle = '#fff';
  context.fillText(name, canvas.width / 2, 36);
  const barW = canvas.width - 20;
  context.fillStyle = '#333';
  context.fillRect(10, 44, barW, 10);
  context.fillStyle = ratio > 0.5 ? '#4caf50' : ratio > 0.2 ? '#ffb300' : '#e53935';
  context.fillRect(10, 44, barW * ratio, 10);
  context.fillStyle = '#eee';
  context.font = '12px system-ui, sans-serif';
  context.fillText(`${hp} / ${maxHp}`, canvas.width / 2, 56);
  return canvas.toDataURL('image/png');
}

/** The browser entry. Wires the SSE stream, the terrain, and the three.js scene. */
export async function run(THREE, { OrbitControls }) {
  const canvas = document.getElementById('scene');
  const statusElement = document.getElementById('status');
  const playerSelect = document.getElementById('player');
  const say = (text, isError = false) => {
    statusElement.classList.toggle('error', isError);
    statusElement.textContent = text;
  };

  const renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio ?? 1, MAX_PIXEL_RATIO));
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x11161c);
  const camera = new THREE.PerspectiveCamera(60, 1, 0.1, 4000);
  camera.position.set(0, START_EYE_M, START_EYE_M);
  const controls = new OrbitControls(camera, renderer.domElement);
  controls.minDistance = MIN_ZOOM_M;
  controls.maxDistance = MAX_ZOOM_M;
  controls.maxPolarAngle = Math.PI / 2.05;
  scene.add(new THREE.HemisphereLight(0xffffff, 0x202a35, 1.2));
  const sun = new THREE.DirectionalLight(0xfff2cc, 0.8);
  sun.position.set(100, 300, 100);
  scene.add(sun);

  const resize = () => {
    const rect = canvas.getBoundingClientRect();
    renderer.setSize(rect.width, rect.height, false);
    camera.aspect = rect.width / Math.max(rect.height, 1);
    camera.updateProjectionMatrix();
  };
  resize();
  window.addEventListener('resize', resize);

  const terrainGroup = new THREE.Group();
  scene.add(terrainGroup);
  let terrainCenter = null;

  const entityGroup = new THREE.Group();
  scene.add(entityGroup);
  const entityMeshes = new Map(); // entity id -> { mesh, sprite, kind }

  const lineGroup = new THREE.Group();
  scene.add(lineGroup);

  let state = null;
  let focus = null;
  let source = null;

  function dropEntities() {
    for (const { mesh, sprite } of entityMeshes.values()) {
      entityGroup.remove(mesh);
      if (sprite) entityGroup.remove(sprite);
      mesh.geometry.dispose();
      mesh.material.dispose();
      if (sprite) {
        sprite.material.map?.dispose();
        sprite.material.dispose();
      }
    }
    entityMeshes.clear();
    while (lineGroup.children.length > 0) {
      const line = lineGroup.children.pop();
      line.geometry.dispose();
      line.material.dispose();
    }
  }

  async function loadTerrain(cx, cy) {
    const url = `${GRID_URL}?x=${cx}&y=${cy}&half=${TERRAIN_HALF_M}&step=${TERRAIN_STEP_M}`;
    const grid = await fetchJson(url);
    while (terrainGroup.children.length > 0) {
      const old = terrainGroup.children.pop();
      old.geometry.dispose();
      old.material.dispose();
    }
    const { x0, y0, step, size, heights } = grid;
    const side = (size - 1) * step;
    const geometry = new THREE.PlaneGeometry(side, side, size - 1, size - 1);
    geometry.rotateX(-Math.PI / 2);
    const positions = geometry.attributes.position;
    const colors = new Float32Array(positions.count * 3);
    const sampleAt = (row, column) => {
      const h = heights?.[row]?.[column];
      return h == null ? 0 : h;
    };
    for (let row = 0; row < size; row += 1) {
      for (let column = 0; column < size; column += 1) {
        const index = row * size + column;
        const z = sampleAt(row, column);
        positions.setY(index, z);
        const prev = sampleAt(row, Math.max(0, column - 1));
        const next = sampleAt(row, Math.min(size - 1, column + 1));
        const slope = Math.abs(next - prev) / (2 * step);
        const [r, g, b] = terrainColor(slope);
        colors[index * 3] = r;
        colors[index * 3 + 1] = g;
        colors[index * 3 + 2] = b;
      }
    }
    geometry.setAttribute('color', new THREE.BufferAttribute(colors, 3));
    geometry.computeVertexNormals();
    const material = new THREE.MeshLambertMaterial({ vertexColors: true, flatShading: true });
    const mesh = new THREE.Mesh(geometry, material);
    mesh.position.set(x0 + side / 2, 0, y0 + side / 2);
    terrainGroup.add(mesh);
    terrainCenter = { x: cx, y: cy };
  }

  function colorFor(entity, kind) {
    if (kind === 'mob') return 0xd43f3f;
    if (entity.bot) return 0x9aa5b1;
    return 0x4a90e2;
  }

  function ensureEntity(id, kind, color) {
    if (entityMeshes.has(id)) return entityMeshes.get(id);
    const height = kind === 'mob' ? MOB_HEIGHT_M : PLAYER_HEIGHT_M;
    const geometry = new THREE.CapsuleGeometry(CAPSULE_RADIUS_M, Math.max(0.1, height - 2 * CAPSULE_RADIUS_M), 4, 8);
    const material = new THREE.MeshLambertMaterial({ color, transparent: true, opacity: 1 });
    const mesh = new THREE.Mesh(geometry, material);
    entityGroup.add(mesh);
    const spriteMaterial = new THREE.SpriteMaterial({ transparent: true });
    const sprite = new THREE.Sprite(spriteMaterial);
    sprite.scale.set(4, 1, 1);
    entityGroup.add(sprite);
    const record = { mesh, sprite, kind, color };
    entityMeshes.set(id, record);
    return record;
  }

  function updateSprite(record, name, hp, maxHp) {
    const ratio = maxHp > 0 ? Math.max(0, Math.min(1, hp / maxHp)) : 0;
    const url = hpSpriteDataUrl(name, hp, maxHp, ratio);
    const texture = new THREE.TextureLoader().load(url);
    record.sprite.material.map?.dispose();
    record.sprite.material.map = texture;
    record.sprite.material.needsUpdate = true;
  }

  function paintEntities(view) {
    const seen = new Set();
    const place = (entity, kind) => {
      const dead = kind === 'mob' ? entity.state === MOB_STATE_DEAD || entity.health === 0 : entity.dead;
      const record = ensureEntity(entity.entity, kind, colorFor(entity, kind));
      const height = kind === 'mob' ? MOB_HEIGHT_M : PLAYER_HEIGHT_M;
      record.mesh.position.set(entity.x, (entity.z ?? 0) + height / 2, entity.y);
      record.mesh.material.opacity = dead ? DEAD_OPACITY : 1;
      record.mesh.material.needsUpdate = true;
      const name = entity.name ?? `#${entity.entity}`;
      updateSprite(record, name, entity.health ?? 0, entity.max_health ?? 0);
      record.sprite.position.set(entity.x, (entity.z ?? 0) + height + SPRITE_Y_M, entity.y);
      record.alive = !dead;
      seen.add(entity.entity);
    };
    for (const player of view.players ?? []) place(player, 'player');
    for (const mob of view.mobs ?? []) place(mob, 'mob');
    for (const [id, record] of entityMeshes) {
      if (seen.has(id)) continue;
      entityGroup.remove(record.mesh);
      entityGroup.remove(record.sprite);
      record.mesh.geometry.dispose();
      record.mesh.material.dispose();
      record.sprite.material.map?.dispose();
      record.sprite.material.dispose();
      entityMeshes.delete(id);
    }
  }

  function paintFightLines(view) {
    while (lineGroup.children.length > 0) {
      const line = lineGroup.children.pop();
      line.geometry.dispose();
      line.material.dispose();
    }
    for (const { fromEntity, toEntity } of fightLines(view)) {
      const from = entityMeshes.get(fromEntity);
      const to = entityMeshes.get(toEntity);
      if (!from || !to) continue;
      const points = [from.mesh.position.clone(), to.mesh.position.clone()];
      const geometry = new THREE.BufferGeometry().setFromPoints(points);
      const material = new THREE.LineBasicMaterial({ color: from.color });
      lineGroup.add(new THREE.Line(geometry, material));
    }
  }

  function focusOn(view) {
    if (focus == null) return;
    const self = (view.players ?? []).find((p) => p.entity === focus);
    if (!self) return;
    controls.target.set(self.x, (self.z ?? 0) + PLAYER_HEIGHT_M / 2, self.y);
    const far = Math.hypot(camera.position.x - controls.target.x, camera.position.z - controls.target.z);
    if (!Number.isFinite(far) || far > MAX_ZOOM_M) {
      camera.position.set(self.x, (self.z ?? 0) + START_EYE_M, self.y + START_EYE_M);
    }
    controls.update();
    const needsTerrain = !terrainCenter
      || Math.hypot(self.x - terrainCenter.x, self.y - terrainCenter.y) > TERRAIN_HALF_M / 2;
    if (needsTerrain) {
      loadTerrain(self.x, self.y).catch((error) => say(error.message, true));
    }
  }

  function onState(view) {
    state = reduceState(state, view);
    paintEntities(state);
    paintFightLines(state);
    focusOn(state);
    say(`${state.players.length} players · ${state.mobs.length} mobs`);
  }

  function openStream() {
    if (source) source.close();
    const url = focus == null ? STREAM_URL : `${STREAM_URL}?focus=${focus}`;
    source = new EventSource(url);
    source.addEventListener('state', (event) => {
      try {
        onState(JSON.parse(event.data));
      } catch (error) {
        say(error.message, true);
      }
    });
    source.addEventListener('error', () => say('live stream broken', true));
  }

  function pickPlayer(entity) {
    focus = entity;
    dropEntities();
    state = null;
    terrainCenter = null;
    openStream();
  }

  function fillPicker(view) {
    const players = view.players ?? [];
    playerSelect.replaceChildren(...players.map((player) => {
      const option = document.createElement('option');
      option.value = String(player.entity);
      option.textContent = `${player.name} · ${player.entity}`;
      return option;
    }));
    if (players.length > 0 && focus == null) {
      focus = players[0].entity;
      playerSelect.value = String(focus);
    }
  }

  playerSelect.addEventListener('change', () => {
    const entity = Number(playerSelect.value);
    if (Number.isFinite(entity)) pickPlayer(entity);
  });
  window.addEventListener('keydown', (event) => {
    if (event.key.toLowerCase() === 'r' && focus != null && state) {
      focusOn(state);
    }
  });

  try {
    const first = await fetchJson(VIEW_URL);
    fillPicker(first);
    openStream();
  } catch (error) {
    say(error.message, true);
  }

  function tick() {
    controls.update();
    renderer.render(scene, camera);
    requestAnimationFrame(tick);
  }
  tick();
}

// Browser entry: load three.js and start.
if (typeof window !== 'undefined' && typeof document !== 'undefined') {
  const [THREE, controls] = await Promise.all([
    import('three'),
    import('three/addons/controls/OrbitControls.js'),
  ]);
  run(THREE, controls).catch((error) => {
    const statusElement = document.getElementById('status');
    if (statusElement) {
      statusElement.classList.add('error');
      statusElement.textContent = error.message;
    } else {
      console.error(error);
    }
  });
}
