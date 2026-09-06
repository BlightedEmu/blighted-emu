// SPDX-License-Identifier: AGPL-3.0-or-later
// The world map on a 2D canvas: map tiles or relief tiles, spawn points, paths and points, with
// pan, zoom, follow and pick. Shared by every admin page.

import {
  boxOf,
  parentTile,
  screenToWorld,
  tileLevelFor,
  visibleTiles,
  worldToScreen,
  zoomAt,
} from './view.js';

/** Pixels of a map tile and of a relief tile. */
export const MAP_TILE_PX = 1024;
export const RELIEF_TILE_PX = 256;
/** Spawn points are fetched only closer than this (metres per pixel). */
export const SPAWN_ZOOM_M_PER_PX = 4;
/** Base layers. */
export const LAYERS = Object.freeze({ TILES: 'tiles', RELIEF: 'relief', BOTH: 'both' });

const SPAWN_COLOURS = { vitals: '#d9534f', gatherables: '#4cae4c', landmarks: '#e8a33d' };
const LABEL_M_PER_PX = 0.6;
const WHEEL_STEP = 1.2;
const BUTTON_STEP = 2;
const PICK_PX = 8;
const POINT_RADIUS_PX = 4;
const SPAWN_RADIUS_PX = 3;
const PATH_WIDTH_PX = 2;
const OVERLAY_ALPHA = 0.5;
const SPAWN_FETCH_DELAY_MS = 150;
const LABEL_FONT = '11px system-ui, sans-serif';
const BACKGROUND = '#0d1013';

export class WorldMap {
  /**
   * @param canvas the canvas to draw on
   * @param info `/api/world/info`
   * @param onCursor called with the world point under the mouse, or null
   */
  constructor(canvas, info, onCursor = () => {}) {
    this.canvas = canvas;
    this.context = canvas.getContext('2d');
    this.info = info;
    this.onCursor = onCursor;
    this.layer = info.map_tiles ? LAYERS.TILES : LAYERS.RELIEF;
    this.spawnKinds = new Set();
    this.spawns = [];
    this.paths = [];
    this.points = [];
    this.followKey = null;
    this.selectHandler = () => {};
    this.images = new Map();
    this.spawnTimer = null;
    this.spawnRequest = 0;
    this.view = { centerX: 0, centerY: 0, metresPerPixel: 1, width: 1, height: 1 };
    this.drawQueued = false;
    // The first fit waits for the canvas to have its laid-out size.
    this.fitPending = true;
    this.resize();
    this.listen();
  }

  /** Sets the base layer (one of LAYERS). */
  setLayer(layer) {
    this.layer = layer;
    this.redraw();
  }

  /** Sets the spawn kinds to show ('vitals', 'gatherables', 'landmarks'). */
  setSpawnKinds(kinds) {
    this.spawnKinds = new Set(kinds);
    this.fetchSpawnsSoon();
  }

  /** Paths: [{ key, color, points: [{ x, y }] }]. */
  setPaths(paths) {
    this.paths = paths;
    this.redraw();
  }

  /** Points: [{ key, x, y, color, label }]. */
  setPoints(points) {
    this.points = points;
    this.redraw();
  }

  /** Keeps the point or the path end of `key` in the centre; null stops. */
  follow(key) {
    this.followKey = key;
    this.redraw();
  }

  /** Called with the picked point or path, or null. */
  onSelect(handler) {
    this.selectHandler = handler;
  }

  /** Shows a world box, or the whole world. */
  fit(box = { minX: 0, minY: 0, maxX: this.info.world_m, maxY: this.info.world_m }) {
    const width = Math.max(box.maxX - box.minX, 1);
    const height = Math.max(box.maxY - box.minY, 1);
    this.view = {
      ...this.view,
      centerX: (box.minX + box.maxX) / 2,
      centerY: (box.minY + box.maxY) / 2,
      metresPerPixel: Math.max(width / this.view.width, height / this.view.height) * 1.05,
    };
    this.changed();
  }

  /** Zooms in (`steps` > 0) or out around the centre. */
  zoom(steps) {
    this.view = zoomAt(
      this.view,
      this.view.width / 2,
      this.view.height / 2,
      BUTTON_STEP ** -steps,
      this.info.world_m,
    );
    this.changed();
  }

  /** Matches the canvas to its displayed size. */
  resize() {
    const ratio = window.devicePixelRatio || 1;
    const width = Math.max(this.canvas.clientWidth, 1);
    const height = Math.max(this.canvas.clientHeight, 1);
    this.canvas.width = Math.round(width * ratio);
    this.canvas.height = Math.round(height * ratio);
    this.context.setTransform(ratio, 0, 0, ratio, 0, 0);
    this.view = { ...this.view, width, height };
    if (this.fitPending && this.canvas.clientWidth > 0 && this.canvas.clientHeight > 0) {
      this.fitPending = false;
      this.fit();
      return;
    }
    this.changed();
  }

  /** The current view. */
  currentView() {
    return { ...this.view };
  }

  listen() {
    new ResizeObserver(() => this.resize()).observe(this.canvas);
    let drag = null;
    this.canvas.addEventListener('pointerdown', (event) => {
      drag = { x: event.offsetX, y: event.offsetY, moved: false };
      this.canvas.setPointerCapture(event.pointerId);
    });
    this.canvas.addEventListener('pointermove', (event) => {
      this.onCursor(screenToWorld(this.view, event.offsetX, event.offsetY));
      if (!drag) {
        return;
      }
      const dx = event.offsetX - drag.x;
      const dy = event.offsetY - drag.y;
      if (Math.abs(dx) + Math.abs(dy) > 2) {
        drag.moved = true;
      }
      this.view = {
        ...this.view,
        centerX: this.view.centerX - dx * this.view.metresPerPixel,
        centerY: this.view.centerY + dy * this.view.metresPerPixel,
      };
      drag.x = event.offsetX;
      drag.y = event.offsetY;
      if (drag.moved) {
        this.followKey = null;
      }
      this.changed();
    });
    this.canvas.addEventListener('pointerup', (event) => {
      if (drag && !drag.moved) {
        this.selectHandler(this.pick(event.offsetX, event.offsetY));
      }
      drag = null;
    });
    this.canvas.addEventListener('pointerleave', () => this.onCursor(null));
    this.canvas.addEventListener(
      'wheel',
      (event) => {
        event.preventDefault();
        const factor = event.deltaY > 0 ? WHEEL_STEP : 1 / WHEEL_STEP;
        this.view = zoomAt(this.view, event.offsetX, event.offsetY, factor, this.info.world_m);
        this.changed();
        this.onCursor(screenToWorld(this.view, event.offsetX, event.offsetY));
      },
      { passive: false },
    );
  }

  changed() {
    this.fetchSpawnsSoon();
    this.redraw();
  }

  redraw() {
    if (this.drawQueued) {
      return;
    }
    this.drawQueued = true;
    requestAnimationFrame(() => {
      this.drawQueued = false;
      this.draw();
    });
  }

  fetchSpawnsSoon() {
    clearTimeout(this.spawnTimer);
    if (this.spawnKinds.size === 0 || this.view.metresPerPixel > SPAWN_ZOOM_M_PER_PX) {
      this.spawns = [];
      this.redraw();
      return;
    }
    this.spawnTimer = setTimeout(() => this.fetchSpawns(), SPAWN_FETCH_DELAY_MS);
  }

  async fetchSpawns() {
    const request = ++this.spawnRequest;
    const box = boxOf(this.view);
    const query = new URLSearchParams({
      min_x: box.minX,
      min_y: box.minY,
      max_x: box.maxX,
      max_y: box.maxY,
      kinds: [...this.spawnKinds].join(','),
    });
    try {
      const response = await fetch(`/api/world/spawns?${query}`);
      const spawns = response.ok ? await response.json() : [];
      if (request === this.spawnRequest) {
        this.spawns = spawns;
        this.redraw();
      }
    } catch {
      // A failed fetch keeps the last points; the next move fetches again.
    }
  }

  followed() {
    if (this.followKey === null) {
      return null;
    }
    const point = this.points.find((item) => item.key === this.followKey);
    if (point) {
      return point;
    }
    const path = this.paths.find((item) => item.key === this.followKey);
    return path && path.points.length > 0 ? path.points[path.points.length - 1] : null;
  }

  draw() {
    const target = this.followed();
    if (target) {
      this.view = { ...this.view, centerX: target.x, centerY: target.y };
    }
    const context = this.context;
    context.fillStyle = BACKGROUND;
    context.fillRect(0, 0, this.view.width, this.view.height);
    if (this.layer !== LAYERS.RELIEF && this.info.map_tiles) {
      this.drawTiles('tiles', MAP_TILE_PX, 1);
    }
    if (this.layer !== LAYERS.TILES || !this.info.map_tiles) {
      const alpha = this.layer === LAYERS.BOTH && this.info.map_tiles ? OVERLAY_ALPHA : 1;
      this.drawTiles('relief', RELIEF_TILE_PX, alpha);
    }
    this.drawSpawns();
    this.drawPaths();
    this.drawPoints();
  }

  drawTiles(kind, tilePx, alpha) {
    const tiles = this.info.tiles;
    const level = tileLevelFor(this.view, tiles, tilePx);
    this.context.globalAlpha = alpha;
    for (const tile of visibleTiles(this.view, tiles, level)) {
      this.drawTile(kind, tile, tile);
    }
    this.context.globalAlpha = 1;
  }

  /** Draws `area` (a tile rectangle) from `tile`'s image, or from a coarser tile's. */
  drawTile(kind, tile, area) {
    const image = this.image(kind, tile);
    if (!image.complete || image.naturalWidth === 0) {
      const parent = parentTile(this.info.tiles, tile);
      if (parent) {
        this.drawTile(kind, parent, area);
      }
      return;
    }
    const scale = image.naturalWidth / tile.size;
    const sourceX = (area.west - tile.west) * scale;
    const sourceY = (tile.south + tile.size - (area.south + area.size)) * scale;
    const topLeft = worldToScreen(this.view, area.west, area.south + area.size);
    const side = area.size / this.view.metresPerPixel;
    this.context.drawImage(
      image,
      sourceX,
      sourceY,
      area.size * scale,
      area.size * scale,
      topLeft.px,
      topLeft.py,
      side + 0.5,
      side + 0.5,
    );
  }

  image(kind, tile) {
    const url = `/api/world/${kind}/${tile.level}/${tile.row}/${tile.column}`;
    let image = this.images.get(url);
    if (!image) {
      image = new Image();
      image.onload = () => this.redraw();
      image.src = url;
      this.images.set(url, image);
    }
    return image;
  }

  drawSpawns() {
    const context = this.context;
    const labels = this.view.metresPerPixel <= LABEL_M_PER_PX;
    context.font = LABEL_FONT;
    for (const spawn of this.spawns) {
      const { px, py } = worldToScreen(this.view, spawn.x, spawn.y);
      context.fillStyle = SPAWN_COLOURS[spawn.kind];
      context.beginPath();
      context.arc(px, py, SPAWN_RADIUS_PX, 0, Math.PI * 2);
      context.fill();
      if (labels) {
        const level = spawn.level === null ? '' : ` ${spawn.level}`;
        context.fillText(`${spawn.id}${level}`, px + SPAWN_RADIUS_PX + 2, py + 4);
      }
    }
  }

  drawPaths() {
    const context = this.context;
    context.lineWidth = PATH_WIDTH_PX;
    for (const path of this.paths) {
      if (path.points.length === 0) {
        continue;
      }
      context.strokeStyle = path.color;
      context.beginPath();
      path.points.forEach((point, index) => {
        const { px, py } = worldToScreen(this.view, point.x, point.y);
        if (index === 0) {
          context.moveTo(px, py);
        } else {
          context.lineTo(px, py);
        }
      });
      context.stroke();
    }
  }

  drawPoints() {
    const context = this.context;
    context.font = LABEL_FONT;
    for (const point of this.points) {
      const { px, py } = worldToScreen(this.view, point.x, point.y);
      context.fillStyle = point.color;
      context.beginPath();
      context.arc(px, py, POINT_RADIUS_PX, 0, Math.PI * 2);
      context.fill();
      if (point.label) {
        context.fillText(point.label, px + POINT_RADIUS_PX + 2, py + 4);
      }
    }
  }

  /** The point, path or spawn nearest to a screen pixel within PICK_PX, or null. */
  pick(px, py) {
    let best = null;
    let bestDistance = PICK_PX;
    const consider = (item, x, y) => {
      const screen = worldToScreen(this.view, x, y);
      const distance = Math.hypot(screen.px - px, screen.py - py);
      if (distance <= bestDistance) {
        best = item;
        bestDistance = distance;
      }
    };
    for (const point of this.points) {
      consider(point, point.x, point.y);
    }
    for (const path of this.paths) {
      for (const point of path.points) {
        consider(path, point.x, point.y);
      }
    }
    for (const spawn of this.spawns) {
      consider(spawn, spawn.x, spawn.y);
    }
    return best;
  }
}
