// Geometry helpers: affine matrices, shape outlines, polygon tests, fills.
// World units are millimeters, Y points up (machine coordinates).

export const IDENT = () => [1, 0, 0, 1, 0, 0];

// m * n : apply n first, then m
export function mul(m, n) {
  return [
    m[0] * n[0] + m[2] * n[1],
    m[1] * n[0] + m[3] * n[1],
    m[0] * n[2] + m[2] * n[3],
    m[1] * n[2] + m[3] * n[3],
    m[0] * n[4] + m[2] * n[5] + m[4],
    m[1] * n[4] + m[3] * n[5] + m[5],
  ];
}
export const apply = (m, p) => [m[0] * p[0] + m[2] * p[1] + m[4], m[1] * p[0] + m[3] * p[1] + m[5]];
export function inv(m) {
  const det = m[0] * m[3] - m[1] * m[2] || 1e-12;
  const a = m[3] / det, b = -m[1] / det, c = -m[2] / det, d = m[0] / det;
  return [a, b, c, d, -(a * m[4] + c * m[5]), -(b * m[4] + d * m[5])];
}
export const translate = (x, y) => [1, 0, 0, 1, x, y];
export const scaleAbout = (sx, sy, cx, cy) => [sx, 0, 0, sy, cx - sx * cx, cy - sy * cy];
export function rotateAbout(deg, cx, cy) {
  const r = (deg * Math.PI) / 180, c = Math.cos(r), s = Math.sin(r);
  return [c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy];
}
export const matRotation = (m) => (Math.atan2(m[1], m[0]) * 180) / Math.PI;

export function bboxOf(contours) {
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  for (const c of contours) for (const [x, y] of c.pts) {
    if (x < x0) x0 = x; if (y < y0) y0 = y; if (x > x1) x1 = x; if (y > y1) y1 = y;
  }
  if (x0 === Infinity) return null;
  return { x0, y0, x1, y1, w: x1 - x0, h: y1 - y0 };
}
export function unionBox(a, b) {
  if (!a) return b; if (!b) return a;
  const r = { x0: Math.min(a.x0, b.x0), y0: Math.min(a.y0, b.y0), x1: Math.max(a.x1, b.x1), y1: Math.max(a.y1, b.y1) };
  r.w = r.x1 - r.x0; r.h = r.y1 - r.y0; return r;
}

// ---- local outlines -------------------------------------------------------
function ellipsePts(cx, cy, rx, ry, a0 = 0, a1 = Math.PI * 2) {
  const per = Math.PI * (rx + ry) * Math.abs(a1 - a0) / (Math.PI * 2);
  const n = Math.max(12, Math.ceil(per / 0.4));
  const pts = [];
  for (let i = 0; i <= n; i++) {
    const a = a0 + ((a1 - a0) * i) / n;
    pts.push([cx + rx * Math.cos(a), cy + ry * Math.sin(a)]);
  }
  return pts;
}

export function localContours(s) {
  switch (s.type) {
    case 'rect': {
      const { w, h } = s; const r = Math.min(s.r || 0, w / 2, h / 2);
      if (r <= 0) return [{ pts: [[0, 0], [w, 0], [w, h], [0, h], [0, 0]], closed: true }];
      const P = Math.PI, pts = [];
      pts.push(...ellipsePts(w - r, r, r, r, -P / 2, 0));
      pts.push(...ellipsePts(w - r, h - r, r, r, 0, P / 2));
      pts.push(...ellipsePts(r, h - r, r, r, P / 2, P));
      pts.push(...ellipsePts(r, r, r, r, P, P * 1.5));
      pts.push(pts[0].slice());
      return [{ pts, closed: true }];
    }
    case 'ellipse': {
      const pts = ellipsePts(s.w / 2, s.h / 2, s.w / 2, s.h / 2);
      pts[pts.length - 1] = pts[0].slice();
      return [{ pts, closed: true }];
    }
    case 'polygon': {
      const n = Math.max(3, s.sides | 0), pts = [];
      for (let i = 0; i <= n; i++) {
        const a = Math.PI / 2 + (i % n) * (2 * Math.PI) / n;
        pts.push([s.w / 2 + (s.w / 2) * Math.cos(a), s.h / 2 + (s.h / 2) * Math.sin(a)]);
      }
      return [{ pts, closed: true }];
    }
    case 'path':
    case 'text':
      return s.subpaths || [];
    case 'image':
      return [{ pts: [[0, 0], [s.w, 0], [s.w, s.h], [0, s.h], [0, 0]], closed: true }];
  }
  return [];
}

export function worldContours(s) {
  return localContours(s).map((c) => ({ pts: c.pts.map((p) => apply(s.tf, p)), closed: c.closed }));
}
export function shapeBox(s) { return bboxOf(worldContours(s)); }

// ---- tests ---------------------------------------------------------------
export function pointInPoly(pt, pts) {
  let inside = false;
  for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) {
    const [xi, yi] = pts[i], [xj, yj] = pts[j];
    if ((yi > pt[1]) !== (yj > pt[1]) && pt[0] < ((xj - xi) * (pt[1] - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}
export function distToSeg(p, a, b) {
  const dx = b[0] - a[0], dy = b[1] - a[1];
  const l = dx * dx + dy * dy;
  let t = l ? ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l : 0;
  t = Math.max(0, Math.min(1, t));
  return Math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy));
}
export function hitShape(s, p, tol) {
  const cs = worldContours(s);
  for (const c of cs) for (let i = 1; i < c.pts.length; i++) if (distToSeg(p, c.pts[i - 1], c.pts[i]) <= tol) return true;
  if (s.type === 'image' && cs[0] && pointInPoly(p, cs[0].pts)) return true;
  return false;
}

// Ramer-Douglas-Peucker
export function simplify(pts, tol) {
  if (pts.length < 3) return pts;
  const keep = new Uint8Array(pts.length); keep[0] = keep[pts.length - 1] = 1;
  const stack = [[0, pts.length - 1]];
  while (stack.length) {
    const [a, b] = stack.pop(); let md = 0, mi = -1;
    for (let i = a + 1; i < b; i++) { const d = distToSeg(pts[i], pts[a], pts[b]); if (d > md) { md = d; mi = i; } }
    if (md > tol && mi > 0) { keep[mi] = 1; stack.push([a, mi], [mi, b]); }
  }
  return pts.filter((_, i) => keep[i]);
}

// ---- scanline fill ---------------------------------------------------------
// Even-odd fill of closed contours with horizontal lines `interval` mm apart.
// Returns rows: [{y, segs:[[x0,x1],...]}] bottom to top.
export function scanFill(contours, interval, vertical = false) {
  const polys = contours.filter((c) => c.closed && c.pts.length > 2).map((c) =>
    vertical ? c.pts.map(([x, y]) => [y, x]) : c.pts);
  const box = bboxOf(polys.map((pts) => ({ pts })));
  if (!box || interval <= 0) return [];
  const edges = [];
  for (const pts of polys) for (let i = 1; i < pts.length; i++) {
    const a = pts[i - 1], b = pts[i];
    if (a[1] === b[1]) continue;
    edges.push(a[1] < b[1] ? [a[0], a[1], b[0], b[1]] : [b[0], b[1], a[0], a[1]]);
  }
  edges.sort((e, f) => e[1] - f[1]);
  const rows = [];
  let ei = 0; const active = [];
  for (let y = box.y0 + interval / 2; y <= box.y1; y += interval) {
    while (ei < edges.length && edges[ei][1] <= y) active.push(edges[ei++]);
    const xs = [];
    for (let k = active.length - 1; k >= 0; k--) {
      const e = active[k];
      if (e[3] <= y) { active.splice(k, 1); continue; }
      xs.push(e[0] + ((y - e[1]) * (e[2] - e[0])) / (e[3] - e[1]));
    }
    xs.sort((a, b) => a - b);
    const segs = [];
    for (let k = 0; k + 1 < xs.length; k += 2) if (xs[k + 1] - xs[k] > 1e-6) segs.push([xs[k], xs[k + 1]]);
    if (segs.length) rows.push({ y, segs });
  }
  return rows;
}

// ---- marching squares tracing (text & bitmaps to outlines) -----------------
// grid: Float32Array w*h values 0..1 (1 = ink); returns contours in pixel coords.
export function traceGrid(grid, w, h, thr = 0.5) {
  const W = w + 2, H = h + 2;
  const v = new Float32Array(W * H);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) v[(y + 1) * W + x + 1] = grid[y * w + x];
  const pt = new Map();
  const key = (t, x, y) => t + ':' + x + ':' + y;
  const ptOf = (t, x, y) => {
    const k = key(t, x, y);
    if (!pt.has(k)) {
      let p;
      if (t === 'h') { const a = v[y * W + x], b = v[y * W + x + 1]; p = [x + (thr - a) / (b - a || 1e-9), y]; }
      else { const a = v[y * W + x], b = v[(y + 1) * W + x]; p = [x, y + (thr - a) / (b - a || 1e-9)]; }
      pt.set(k, p);
    }
    return k;
  };
  const adj = new Map();
  const link = (a, b) => {
    if (!adj.has(a)) adj.set(a, []); if (!adj.has(b)) adj.set(b, []);
    adj.get(a).push(b); adj.get(b).push(a);
  };
  for (let y = 0; y < H - 1; y++) for (let x = 0; x < W - 1; x++) {
    const tl = v[y * W + x] >= thr, tr = v[y * W + x + 1] >= thr, br = v[(y + 1) * W + x + 1] >= thr, bl = v[(y + 1) * W + x] >= thr;
    const c = (tl ? 8 : 0) | (tr ? 4 : 0) | (br ? 2 : 0) | (bl ? 1 : 0);
    if (c === 0 || c === 15) continue;
    const T = () => ptOf('h', x, y), B = () => ptOf('h', x, y + 1), L = () => ptOf('v', x, y), R = () => ptOf('v', x + 1, y);
    const center = (v[y * W + x] + v[y * W + x + 1] + v[(y + 1) * W + x] + v[(y + 1) * W + x + 1]) / 4 >= thr;
    switch (c) {
      case 1: case 14: link(L(), B()); break;
      case 2: case 13: link(B(), R()); break;
      case 3: case 12: link(L(), R()); break;
      case 4: case 11: link(T(), R()); break;
      case 6: case 9: link(T(), B()); break;
      case 7: case 8: link(L(), T()); break;
      case 5: if (center) { link(L(), T()); link(B(), R()); } else { link(L(), B()); link(T(), R()); } break;
      case 10: if (center) { link(T(), R()); link(L(), B()); } else { link(L(), T()); link(B(), R()); } break;
    }
  }
  const seen = new Set(), out = [];
  for (const start of adj.keys()) {
    if (seen.has(start)) continue;
    const loop = [start]; seen.add(start);
    let prev = null, cur = start;
    for (;;) {
      const nb = adj.get(cur).filter((n) => n !== prev && !seen.has(n));
      if (!nb.length) break;
      prev = cur; cur = nb[0]; seen.add(cur); loop.push(cur);
    }
    if (loop.length > 2) {
      const pts = loop.map((k) => { const p = pt.get(k); return [p[0] - 1, p[1] - 1]; });
      pts.push(pts[0].slice());
      out.push(pts);
    }
  }
  return out;
}

// Containment depth of closed contours (inner shapes get higher depth)
export function containmentDepth(contours) {
  const boxes = contours.map((c) => bboxOf([c]));
  return contours.map((c, i) => {
    if (!c.closed) return 0;
    let d = 0;
    for (let j = 0; j < contours.length; j++) {
      if (i === j || !contours[j].closed) continue;
      const a = boxes[i], b = boxes[j];
      if (a.x0 >= b.x0 && a.x1 <= b.x1 && a.y0 >= b.y0 && a.y1 <= b.y1 && (a.w < b.w || a.h < b.h) && pointInPoly(c.pts[0], contours[j].pts)) d++;
    }
    return d;
  });
}
