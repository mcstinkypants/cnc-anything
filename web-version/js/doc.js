// The design document: shapes, cut layers, selection, undo history.
import { IDENT, mul, shapeBox, unionBox } from './geom.js';

// 30 layer colors (our own palette; the original also offers numbered color layers)
export const PALETTE = [
  '#000000', '#1f4fff', '#e8202a', '#1fa83a', '#c8a400', '#ff6a00', '#00a8c8', '#c000c0',
  '#8a8a8a', '#6a3cff', '#ff3c8a', '#5c7a00', '#a05a28', '#0060a0', '#7a0028', '#00805a',
  '#ff9e3c', '#3cc8ff', '#b4b400', '#a03cff', '#3c3cff', '#ff3c3c', '#3cb43c', '#783c00',
  '#5a5a9a', '#9a5a5a', '#5a9a5a', '#2a2a2a', '#d26e9a', '#6e9ad2',
];

export function defaultLayer(i) {
  return {
    index: i,
    name: 'C' + String(i).padStart(2, '0'),
    mode: 'Line', // Line | Fill | Image
    speed: 100, // mm/s
    power: 20, // % of S-value max
    minPower: 10, // %
    passes: 1,
    interval: 0.1, // mm between fill / image lines
    bidir: true,
    crossHatch: false,
    overscan: 2.5, // % of speed, in mm
    air: true,
    output: true,
    show: true,
    dither: 'jarvis', // threshold | ordered | floyd | jarvis | stucki | atkinson | grayscale
    invert: false,
    passThrough: false,
  };
}

let nextId = 1;
export const newId = () => nextId++;

export class Doc {
  constructor() {
    this.shapes = [];
    this.layers = PALETTE.map((_, i) => defaultLayer(i));
    this.order = []; // layer indices in cut order
    this.sel = new Set();
    this.undoStack = []; this.redoStack = [];
    this.listeners = [];
    this.dirty = false;
  }
  on(fn) { this.listeners.push(fn); }
  changed(kind = 'shapes') { this.dirty = true; for (const f of this.listeners) f(kind); }

  snapshot() {
    return {
      shapes: this.shapes.map(cloneShape),
      layers: this.layers.map((l) => ({ ...l })),
      order: this.order.slice(),
    };
  }
  restore(s) {
    this.shapes = s.shapes.map(cloneShape);
    this.layers = s.layers.map((l) => ({ ...l }));
    this.order = s.order.slice();
    const ids = new Set(this.shapes.map((x) => x.id));
    for (const id of [...this.sel]) if (!ids.has(id)) this.sel.delete(id);
  }
  checkpoint() {
    this.undoStack.push(this.snapshot());
    if (this.undoStack.length > 200) this.undoStack.shift();
    this.redoStack.length = 0;
  }
  undo() { if (!this.undoStack.length) return; this.redoStack.push(this.snapshot()); this.restore(this.undoStack.pop()); this.changed(); }
  redo() { if (!this.redoStack.length) return; this.undoStack.push(this.snapshot()); this.restore(this.redoStack.pop()); this.changed(); }

  usedLayers() {
    const used = new Set(this.shapes.map((s) => s.layer));
    this.order = this.order.filter((i) => used.has(i));
    for (const i of [...used].sort((a, b) => a - b)) if (!this.order.includes(i)) this.order.push(i);
    return this.order.slice();
  }

  add(shape) {
    shape.id = shape.id || newId();
    shape.tf = shape.tf || IDENT();
    if (shape.layer == null) shape.layer = 0;
    this.shapes.push(shape);
    if (shape.type === 'image' && this.layers[shape.layer].mode !== 'Image') this.layers[shape.layer].mode = 'Image';
    return shape;
  }
  byId(id) { return this.shapes.find((s) => s.id === id); }
  selected() { return this.shapes.filter((s) => this.sel.has(s.id)); }
  selBox() { let b = null; for (const s of this.selected()) b = unionBox(b, shapeBox(s)); return b; }
  allBox(onlyOutput = false) {
    let b = null;
    for (const s of this.shapes) {
      if (onlyOutput && !this.layers[s.layer].output) continue;
      b = unionBox(b, shapeBox(s));
    }
    return b;
  }
  transformSel(m) { for (const s of this.selected()) s.tf = mul(m, s.tf); }
  deleteSel() { this.shapes = this.shapes.filter((s) => !this.sel.has(s.id)); this.sel.clear(); }

  toJSON() {
    return { app: 'burner', version: 1, shapes: this.shapes, layers: this.layers, order: this.order };
  }
  load(obj) {
    if (!obj || !Array.isArray(obj.shapes)) throw new Error('This is not a project file');
    this.shapes = obj.shapes.map(cloneShape);
    for (const s of this.shapes) nextId = Math.max(nextId, (s.id || 0) + 1);
    this.layers = PALETTE.map((_, i) => ({ ...defaultLayer(i), ...((obj.layers || [])[i] || {}) }));
    this.order = obj.order || [];
    this.sel.clear(); this.undoStack = []; this.redoStack = [];
    this.dirty = false;
  }
}

export function cloneShape(s) {
  const c = { ...s, tf: s.tf.slice() };
  if (s.subpaths) c.subpaths = s.subpaths.map((p) => ({ closed: p.closed, pts: p.pts.map((q) => q.slice()) }));
  return c;
}

export function nearestPaletteIndex(rgb) {
  if (!rgb) return 0;
  let best = 0, bd = Infinity;
  PALETTE.forEach((hex, i) => {
    const r = parseInt(hex.slice(1, 3), 16), g = parseInt(hex.slice(3, 5), 16), b = parseInt(hex.slice(5, 7), 16);
    const d = (r - rgb[0]) ** 2 + (g - rgb[1]) ** 2 + (b - rgb[2]) ** 2;
    if (d < bd) { bd = d; best = i; }
  });
  return best;
}
