// Turns the design into a laser job (G-code for GRBL-type controllers).
// Rules come from app/MECHANICS.md (read from the original program).
import { worldContours, scanFill, containmentDepth, bboxOf, inv, apply, shapeBox, unionBox } from './geom.js';

export const APP_NAME = 'Burner';
export const APP_VERSION = '1.0';

export function defaultDevice() {
  return {
    name: 'My laser',
    type: 'grbl', // grbl (M4 variable power) | grbl-m3 (M3 constant power)
    bedW: 400, bedH: 400,
    sMax: 1000, // must match GRBL $30
    baud: 115200,
    bufferSize: 127,
    rapid: 100, // mm/s, used for time estimates and returning home
    stripLeadingZero: false,
    useG0Overscan: false,
    autoHome: false,
    finishX: 0, finishY: 0, returnToFinish: true,
    originTop: true, // (0,0) at the upper-left corner, Y grows toward the front
  };
}

// ---- number format (sub_4166f0): round to N decimals, drop trailing zeros
export function fmt(v, dec, stripLead = false) {
  const p = Math.pow(10, dec);
  let n = Math.round(v * p) / p;
  if (Object.is(n, -0) || Math.abs(n) < 0.5 / p) n = 0;
  let s = n.toFixed(dec);
  if (s.includes('.')) s = s.replace(/0+$/, '').replace(/\.$/, '');
  if (stripLead) s = s.replace(/^(-?)0\./, '$1.');
  return s;
}

// ---- dithering --------------------------------------------------------------
const KERNELS = {
  floyd: [[1, 0, 7 / 16], [-1, 1, 3 / 16], [0, 1, 5 / 16], [1, 1, 1 / 16]],
  jarvis: [[1, 0, 7], [2, 0, 5], [-2, 1, 3], [-1, 1, 5], [0, 1, 7], [1, 1, 5], [2, 1, 3], [-2, 2, 1], [-1, 2, 3], [0, 2, 5], [1, 2, 3], [2, 2, 1]].map(([a, b, c]) => [a, b, c / 48]),
  stucki: [[1, 0, 8], [2, 0, 4], [-2, 1, 2], [-1, 1, 4], [0, 1, 8], [1, 1, 4], [2, 1, 2], [-2, 2, 1], [-1, 2, 2], [0, 2, 4], [1, 2, 2], [2, 2, 1]].map(([a, b, c]) => [a, b, c / 42]),
  atkinson: [[1, 0, 1 / 8], [2, 0, 1 / 8], [-1, 1, 1 / 8], [0, 1, 1 / 8], [1, 1, 1 / 8], [0, 2, 1 / 8]],
};
const BAYER = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
// darkness grid (1 = burn fully) -> burn amount per cell (0/1, or 0..1 for grayscale)
export function dither(grid, w, h, mode) {
  const out = new Float32Array(w * h);
  if (mode === 'grayscale') { out.set(grid); return out; }
  if (mode === 'threshold') { for (let i = 0; i < grid.length; i++) out[i] = grid[i] >= 0.5 ? 1 : 0; return out; }
  if (mode === 'ordered') {
    for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) out[y * w + x] = grid[y * w + x] > (BAYER[(y & 3) * 4 + (x & 3)] + 0.5) / 16 ? 1 : 0;
    return out;
  }
  const k = KERNELS[mode] || KERNELS.jarvis;
  const g = Float32Array.from(grid);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    const i = y * w + x, v = g[i], q = v >= 0.5 ? 1 : 0, e = v - q;
    out[i] = q;
    for (const [dx, dy, f] of k) {
      const xx = x + dx, yy = y + dy;
      if (xx >= 0 && xx < w && yy < h) g[yy * w + xx] += e * f;
    }
  }
  return out;
}

// Sample image shapes onto a world grid with `interval` spacing (rows bottom->top).
export function rasterImages(shapes, imgCache, interval, layer) {
  let box = null; for (const s of shapes) box = unionBox(box, shapeBox(s));
  if (!box) return null;
  const w = Math.max(1, Math.round(box.w / interval)), h = Math.max(1, Math.round(box.h / interval));
  if (w * h > 25e6) throw new Error('Picture too large for this line interval. Increase the interval or shrink the picture.');
  const grid = new Float32Array(w * h), mask = new Uint8Array(w * h);
  for (const s of shapes) {
    const im = imgCache.get(s.src); if (!im) continue;
    const iv = inv(s.tf), b = shapeBox(s);
    const gx0 = Math.max(0, Math.floor((b.x0 - box.x0) / interval)), gx1 = Math.min(w, Math.ceil((b.x1 - box.x0) / interval));
    const gy0 = Math.max(0, Math.floor((b.y0 - box.y0) / interval)), gy1 = Math.min(h, Math.ceil((b.y1 - box.y0) / interval));
    for (let gy = gy0; gy < gy1; gy++) for (let gx = gx0; gx < gx1; gx++) {
      const wx = box.x0 + (gx + 0.5) * interval, wy = box.y0 + (gy + 0.5) * interval;
      const [u, v] = apply(iv, [wx, wy]);
      if (u < 0 || v < 0 || u >= s.w || v >= s.h) continue;
      const px = Math.min(im.w - 1, Math.floor((u / s.w) * im.w)), py = Math.min(im.h - 1, Math.floor(((s.h - v) / s.h) * im.h));
      let dark = 1 - im.gray[py * im.w + px];
      if (s.invert || layer.invert) dark = 1 - dark;
      const i = gy * w + gx; grid[i] = dark; mask[i] = 1;
    }
  }
  const burn = dither(grid, w, h, layer.dither);
  for (let i = 0; i < burn.length; i++) if (!mask[i]) burn[i] = 0;
  return { box, w, h, burn };
}

// ---- planning: design -> list of moves in world mm --------------------------
// ops: {k:'layer', layer} | {k:'rapid', x, y} | {k:'cut', x, y, p (power %), f (mm/s)}
export function planJob(doc, dev, imgCache, opts = {}) {
  const ops = [], warnings = [];
  let pos = opts.startPos ? opts.startPos.slice() : [0, 0];
  const rapid = (x, y) => { ops.push({ k: 'rapid', x, y }); pos = [x, y]; };
  const cut = (x, y, p, f) => { ops.push({ k: 'cut', x, y, p, f }); pos = [x, y]; };

  for (const li of doc.usedLayers()) {
    const L = doc.layers[li];
    if (!L.output) continue;
    let shapes = doc.shapes.filter((s) => s.layer === li);
    if (opts.selectedOnly) shapes = shapes.filter((s) => doc.sel.has(s.id));
    if (!shapes.length) continue;
    ops.push({ k: 'layer', layer: L });
    const vec = shapes.filter((s) => s.type !== 'image');
    const imgs = shapes.filter((s) => s.type === 'image');
    if (imgs.length && L.mode !== 'Image') warnings.push(`Pictures on layer ${L.name} only burn when that layer is in Image mode.`);
    const passes = Math.max(1, L.passes | 0);
    for (let pass = 0; pass < passes; pass++) {
      if (L.mode === 'Line') {
        let cs = [];
        for (const s of vec) cs.push(...worldContours(s));
        cs = cs.filter((c) => c.pts.length > 1);
        const depth = opts.innerFirst === false ? cs.map(() => 0) : containmentDepth(cs);
        const left = cs.map((c, i) => ({ c, d: depth[i] }));
        while (left.length) {
          const maxD = Math.max(...left.map((o) => o.d));
          let bi = -1, bd = Infinity, rev = false;
          left.forEach((o, i) => {
            if (o.d !== maxD) return;
            const a = o.c.pts[0], b = o.c.pts[o.c.pts.length - 1];
            const da = Math.hypot(a[0] - pos[0], a[1] - pos[1]);
            if (da < bd) { bd = da; bi = i; rev = false; }
            if (!o.c.closed) { const db = Math.hypot(b[0] - pos[0], b[1] - pos[1]); if (db < bd) { bd = db; bi = i; rev = true; } }
          });
          const { c } = left.splice(bi, 1)[0];
          const pts = rev ? c.pts.slice().reverse() : c.pts;
          rapid(pts[0][0], pts[0][1]);
          for (let i = 1; i < pts.length; i++) cut(pts[i][0], pts[i][1], L.power, L.speed);
        }
      } else {
        const over = (L.speed * (L.overscan || 0)) / 100;
        const scanRows = (rows, vertical) => {
          let dir = 1;
          const P = (a, b) => (vertical ? [b, a] : [a, b]);
          for (const row of rows) {
            const segs = dir > 0 ? row.segs : row.segs.map(([a, b]) => [b, a]).reverse();
            const s0 = segs[0][0], sEnd = segs[segs.length - 1][1];
            const [rx, ry] = P(s0 - dir * over, row.y); rapid(rx, ry);
            for (const seg of segs) {
              const [ax, ay] = P(seg[0], row.y);
              if (Math.hypot(ax - pos[0], ay - pos[1]) > 1e-6) cut(ax, ay, 0, L.speed);
              const [bx, by] = P(seg[1], row.y); cut(bx, by, L.power, L.speed);
            }
            if (over > 0) { const [ex, ey] = P(sEnd + dir * over, row.y); cut(ex, ey, 0, L.speed); }
            if (L.bidir) dir = -dir;
          }
        };
        let fillCs = [];
        for (const s of vec) fillCs.push(...worldContours(s).filter((c) => c.closed));
        if (fillCs.length) {
          scanRows(scanFill(fillCs, L.interval), false);
          if (L.crossHatch) scanRows(scanFill(fillCs, L.interval, true), true);
        }
        if (L.mode === 'Image' && imgs.length) {
          const r = rasterImages(imgs, imgCache, L.interval, L);
          if (r) {
            const gray = L.dither === 'grayscale';
            let dir = 1;
            for (let gy = 0; gy < r.h; gy++) {
              const y = r.box.y0 + (gy + 0.5) * L.interval;
              // spans of equal power along the row: [x0, x1, power]
              const spans = [];
              for (let gx = 0; gx < r.w; gx++) {
                const v = r.burn[gy * r.w + gx];
                if (v <= 0.004) continue;
                const p = gray ? L.minPower + v * (L.power - L.minPower) : L.power;
                const x0 = r.box.x0 + gx * L.interval, x1 = x0 + L.interval;
                const last = spans[spans.length - 1];
                if (last && Math.abs(last[1] - x0) < 1e-9 && Math.abs(last[2] - p) < 0.5) last[1] = x1;
                else spans.push([x0, x1, p]);
              }
              if (!spans.length) continue;
              const seq = dir > 0 ? spans : spans.map(([a, b, p]) => [b, a, p]).reverse();
              const first = seq[0][0], lastX = seq[seq.length - 1][1];
              rapid(first - dir * over, y);
              for (const [a, b, p] of seq) {
                if (Math.abs(a - pos[0]) > 1e-6) cut(a, y, 0, L.speed);
                cut(b, y, p, L.speed);
              }
              if (over > 0) cut(lastX + dir * over, y, 0, L.speed);
              if (L.bidir) dir = -dir;
            }
          }
        }
      }
    }
  }
  return { ops, warnings };
}

// ---- emit G-code -----------------------------------------------------------
export function emitGcode(plan, dev, opts = {}) {
  const startFrom = opts.startFrom || 'absolute';
  const rel = startFrom === 'current';
  const org = opts.origin || [0, 0];
  const sl = dev.stripLeadingZero;
  const n3 = (v) => fmt(v, 3, sl);
  const lines = [];
  const preview = [];
  let box = null;
  for (const o of plan.ops) if (o.k === 'cut' && o.p > 0) box = unionBox(box, { x0: o.x, y0: o.y, x1: o.x, y1: o.y, w: 0, h: 0 });

  lines.push(`; ${APP_NAME} ${APP_VERSION}`);
  lines.push(`; ${dev.type === 'grbl-m3' ? 'GRBL-M3' : 'GRBL'} device profile, ${startFrom === 'absolute' ? 'absolute coords' : startFrom === 'user' ? 'user origin' : 'current position'}`);
  const flip = !!dev.originTop, H = dev.bedH;
  // world (Y up from the front edge) -> output coordinates
  const toOut = (x, y) => (startFrom === 'absolute' ? [x, flip ? H - y : y] : [x - org[0], flip ? org[1] - y : y - org[1]]);
  if (box) {
    const a = toOut(box.x0, box.y0), b = toOut(box.x1, box.y1);
    lines.push(`; Bounds: X${n3(Math.min(a[0], b[0]))} Y${n3(Math.min(a[1], b[1]))} to X${n3(Math.max(a[0], b[0]))} Y${n3(Math.max(a[1], b[1]))}`);
  }
  lines.push('G00 G17 G40 G21 G54');
  lines.push(rel ? 'G91' : 'G90');
  lines.push(dev.type === 'grbl-m3' ? 'M3' : 'M4');

  // machine-side position tracking (output coords)
  let mode = -1, lx = rel ? 0 : null, ly = rel ? 0 : null, lf = null, ls = null;
  let wx = opts.startPos ? opts.startPos[0] : org[0], wy = opts.startPos ? opts.startPos[1] : org[1];
  if (startFrom === 'absolute' && !opts.startPos) { wx = 0; wy = 0; }
  let time = 0, cutLen = 0, travel = 0, airOn = false, curLayer = 0;
  const EPS = 0.01; // 0x13742bc: axis words are skipped when they move less than this

  const move = (g, x, y, s, f) => {
    const [ox, oy] = toOut(x, y);
    let tx, ty;
    if (rel) { tx = ox - lx; ty = oy - ly; } else { tx = ox; ty = oy; }
    const needX = rel ? Math.abs(tx) >= 0.005 : lx === null || Math.abs(tx - lx) >= EPS;
    const needY = rel ? Math.abs(ty) >= 0.005 : ly === null || Math.abs(ty - ly) >= EPS;
    let line = '';
    if (!needX && !needY && g === 0) return;
    if (g !== mode) { line += g === 0 ? 'G0' : 'G1'; mode = g; }
    if (needX) line += 'X' + n3(tx);
    if (needY) line += 'Y' + n3(ty);
    if (g === 1) {
      if (ls === null || Math.abs(s - ls) > 1e-6) { line += 'S' + fmt(s, 1, false); ls = s; }
      const fm = f * 60; // mm/s -> mm/min (sub_4176c0)
      if (lf === null || Math.abs(fm - lf) > EPS) { line += 'F' + fmt(fm, 2, sl); lf = fm; }
    }
    if (line) lines.push(line);
    if (rel) { if (needX) lx += parseFloat(n3(tx)); if (needY) ly += parseFloat(n3(ty)); }
    else { if (needX) lx = tx; if (needY) ly = ty; }
  };

  for (const o of plan.ops) {
    if (o.k === 'layer') {
      const L = o.layer; curLayer = L.index;
      lines.push(`; ${L.name} @ ${fmt(L.speed * 60, 3)} mm/min, ${fmt(L.power, 1)}% power`);
      if (L.air && !airOn) { lines.push('M8'); airOn = true; } else if (!L.air && airOn) { lines.push('M9'); airOn = false; }
      continue;
    }
    const d = Math.hypot(o.x - wx, o.y - wy);
    if (o.k === 'rapid') {
      move(0, o.x, o.y);
      time += d / dev.rapid; travel += d;
      preview.push([wx, wy, o.x, o.y, 0, curLayer]);
    } else {
      const s = (o.p / 100) * dev.sMax;
      move(1, o.x, o.y, s, o.f);
      time += d / o.f; if (o.p > 0) cutLen += d;
      preview.push([wx, wy, o.x, o.y, o.p, curLayer]);
    }
    wx = o.x; wy = o.y;
  }
  if (airOn) lines.push('M9');
  lines.push('G1 S0');
  lines.push('M5');
  if (rel) {
    lines.push('; return to starting pos');
    lines.push(`G0 X${n3(-lx)} Y${n3(-ly)}`);
    lines.push('G90');
    time += Math.hypot(lx, ly) / dev.rapid;
  } else {
    lines.push('G90');
    if (dev.returnToFinish) {
      lines.push('; return to user-defined finish pos');
      const [fx, fy] = startFrom === 'absolute' ? [dev.finishX, dev.finishY] : [0, 0];
      lines.push(`G0 X${n3(fx)} Y${n3(fy)}`);
      const fo = toOut(wx, wy); time += Math.hypot(fo[0] - fx, fo[1] - fy) / dev.rapid;
    }
  }
  lines.push('M2');
  return { text: lines.join('\n') + '\n', lines, preview, time, cutLen, travel, box };
}

export function fmtTime(sec) {
  sec = Math.round(sec);
  const h = Math.floor(sec / 3600), m = Math.floor((sec % 3600) / 60), s = sec % 60;
  return (h ? h + 'h ' : '') + (h || m ? m + 'm ' : '') + s + 's';
}
