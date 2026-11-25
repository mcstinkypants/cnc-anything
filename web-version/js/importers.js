// Turning text, SVG drawings and pictures into shapes.
import { traceGrid, simplify, mul, apply, IDENT, translate } from './geom.js';

const PX_PER_MM = 12; // text tracing resolution

// Text: draw on a hidden canvas, trace the ink into closed outlines (local mm, Y up)
export function textOutlines(t) {
  const sizePx = Math.max(4, t.size * PX_PER_MM);
  const font = `${t.italic ? 'italic ' : ''}${t.bold ? 'bold ' : ''}${sizePx}px ${t.font || 'sans-serif'}`;
  const c = document.createElement('canvas');
  const g = c.getContext('2d', { willReadFrequently: true });
  g.font = font;
  const lines = String(t.text || '').split('\n');
  const lineH = sizePx * 1.2;
  let wmax = 1;
  for (const l of lines) wmax = Math.max(wmax, g.measureText(l).width);
  const pad = Math.ceil(sizePx * 0.3);
  c.width = Math.ceil(wmax + pad * 2);
  c.height = Math.ceil(lineH * lines.length + pad * 2);
  g.font = font;
  g.fillStyle = '#000'; g.fillRect(0, 0, c.width, c.height);
  g.fillStyle = '#fff'; g.textBaseline = 'alphabetic';
  const align = t.align || 'left';
  lines.forEach((l, i) => {
    const lw = g.measureText(l).width;
    const x = align === 'center' ? pad + (wmax - lw) / 2 : align === 'right' ? pad + wmax - lw : pad;
    g.fillText(l, x, pad + sizePx * 0.95 + i * lineH);
  });
  const img = g.getImageData(0, 0, c.width, c.height).data;
  const grid = new Float32Array(c.width * c.height);
  for (let i = 0; i < grid.length; i++) grid[i] = img[i * 4] / 255;
  const loops = traceGrid(grid, c.width, c.height);
  // find ink bounds so the shape's local origin sits at the ink's lower-left
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  for (const L of loops) for (const [x, y] of L) { x0 = Math.min(x0, x); x1 = Math.max(x1, x); y0 = Math.min(y0, y); y1 = Math.max(y1, y); }
  if (x0 === Infinity) return { subpaths: [], w: 0, h: 0 };
  const subpaths = loops.map((L) => ({
    pts: simplify(L.map(([x, y]) => [(x - x0) / PX_PER_MM, (y1 - y) / PX_PER_MM]), 0.015),
    closed: true,
  })).filter((s) => s.pts.length > 3);
  return { subpaths, w: (x1 - x0) / PX_PER_MM, h: (y1 - y0) / PX_PER_MM };
}

// ---- SVG -------------------------------------------------------------------
function parseTransform(str) {
  let m = IDENT();
  if (!str) return m;
  const re = /(matrix|translate|scale|rotate|skewX|skewY)\s*\(([^)]*)\)/g;
  let r;
  while ((r = re.exec(str))) {
    const a = r[2].split(/[\s,]+/).filter(Boolean).map(Number);
    let t = IDENT();
    switch (r[1]) {
      case 'matrix': t = a.slice(0, 6); break;
      case 'translate': t = [1, 0, 0, 1, a[0] || 0, a[1] || 0]; break;
      case 'scale': t = [a[0], 0, 0, a[1] ?? a[0], 0, 0]; break;
      case 'rotate': {
        const rad = ((a[0] || 0) * Math.PI) / 180, c = Math.cos(rad), s = Math.sin(rad);
        const cx = a[1] || 0, cy = a[2] || 0;
        t = mul(mul([1, 0, 0, 1, cx, cy], [c, s, -s, c, 0, 0]), [1, 0, 0, 1, -cx, -cy]);
        break;
      }
      case 'skewX': t = [1, 0, Math.tan(((a[0] || 0) * Math.PI) / 180), 1, 0, 0]; break;
      case 'skewY': t = [1, Math.tan(((a[0] || 0) * Math.PI) / 180), 0, 1, 0, 0]; break;
    }
    m = mul(m, t);
  }
  return m;
}

function cubic(p0, p1, p2, p3, out) {
  const len = Math.hypot(p1[0] - p0[0], p1[1] - p0[1]) + Math.hypot(p2[0] - p1[0], p2[1] - p1[1]) + Math.hypot(p3[0] - p2[0], p3[1] - p2[1]);
  const n = Math.min(200, Math.max(4, Math.ceil(len / 1.5)));
  for (let i = 1; i <= n; i++) {
    const t = i / n, u = 1 - t;
    out.push([u * u * u * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t * t * t * p3[0],
      u * u * u * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t * t * t * p3[1]]);
  }
}
function arcTo(p0, rx, ry, phi, fa, fs, p1, out) {
  if (!rx || !ry) { out.push(p1); return; }
  rx = Math.abs(rx); ry = Math.abs(ry);
  const r = (phi * Math.PI) / 180, c = Math.cos(r), s = Math.sin(r);
  const dx = (p0[0] - p1[0]) / 2, dy = (p0[1] - p1[1]) / 2;
  const x1 = c * dx + s * dy, y1 = -s * dx + c * dy;
  let lam = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
  if (lam > 1) { rx *= Math.sqrt(lam); ry *= Math.sqrt(lam); }
  const num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
  let co = Math.sqrt(Math.max(0, num / (rx * rx * y1 * y1 + ry * ry * x1 * x1)));
  if (fa === fs) co = -co;
  const cxp = (co * rx * y1) / ry, cyp = (-co * ry * x1) / rx;
  const cx = c * cxp - s * cyp + (p0[0] + p1[0]) / 2, cy = s * cxp + c * cyp + (p0[1] + p1[1]) / 2;
  const ang = (ux, uy, vx, vy) => Math.atan2(ux * vy - uy * vx, ux * vx + uy * vy);
  const t1 = ang(1, 0, (x1 - cxp) / rx, (y1 - cyp) / ry);
  let dt = ang((x1 - cxp) / rx, (y1 - cyp) / ry, (-x1 - cxp) / rx, (-y1 - cyp) / ry);
  if (!fs && dt > 0) dt -= 2 * Math.PI; else if (fs && dt < 0) dt += 2 * Math.PI;
  const n = Math.max(4, Math.ceil((Math.abs(dt) * Math.max(rx, ry)) / 1));
  for (let i = 1; i <= n; i++) {
    const t = t1 + (dt * i) / n;
    out.push([cx + c * rx * Math.cos(t) - s * ry * Math.sin(t), cy + s * rx * Math.cos(t) + c * ry * Math.sin(t)]);
  }
}

export function parsePathD(d) {
  const toks = d.match(/[a-zA-Z]|[-+]?(?:\d*\.\d+|\d+\.?)(?:[eE][-+]?\d+)?/g) || [];
  const subs = []; let cur = null, i = 0, cmd = '', p = [0, 0], start = [0, 0], lastC = null, lastQ = null;
  const num = () => parseFloat(toks[i++]);
  const isNum = () => i < toks.length && !/[a-zA-Z]/.test(toks[i]);
  const begin = (pt) => { cur = { pts: [pt], closed: false }; subs.push(cur); };
  while (i < toks.length) {
    if (/[a-zA-Z]/.test(toks[i])) cmd = toks[i++];
    const rel = cmd === cmd.toLowerCase(), C = cmd.toUpperCase();
    const ox = rel ? p[0] : 0, oy = rel ? p[1] : 0;
    if (C === 'Z') { if (cur) { cur.closed = true; cur.pts.push(start.slice()); } p = start.slice(); cur = null; lastC = lastQ = null; continue; }
    if (!isNum()) { i++; continue; }
    if (!cur && C !== 'M') begin(p.slice());
    switch (C) {
      case 'M': p = [ox + num(), oy + num()]; start = p.slice(); begin(p.slice()); cmd = rel ? 'l' : 'L'; lastC = lastQ = null; break;
      case 'L': p = [ox + num(), oy + num()]; cur.pts.push(p); lastC = lastQ = null; break;
      case 'H': p = [ox + num(), p[1]]; cur.pts.push(p); lastC = lastQ = null; break;
      case 'V': p = [p[0], oy + num()]; cur.pts.push(p); lastC = lastQ = null; break;
      case 'C': { const c1 = [ox + num(), oy + num()], c2 = [ox + num(), oy + num()], e = [ox + num(), oy + num()]; cubic(p, c1, c2, e, cur.pts); lastC = c2; lastQ = null; p = e; break; }
      case 'S': { const c1 = lastC ? [2 * p[0] - lastC[0], 2 * p[1] - lastC[1]] : p.slice(); const c2 = [ox + num(), oy + num()], e = [ox + num(), oy + num()]; cubic(p, c1, c2, e, cur.pts); lastC = c2; lastQ = null; p = e; break; }
      case 'Q': { const q = [ox + num(), oy + num()], e = [ox + num(), oy + num()]; cubic(p, [p[0] + (2 / 3) * (q[0] - p[0]), p[1] + (2 / 3) * (q[1] - p[1])], [e[0] + (2 / 3) * (q[0] - e[0]), e[1] + (2 / 3) * (q[1] - e[1])], e, cur.pts); lastQ = q; lastC = null; p = e; break; }
      case 'T': { const q = lastQ ? [2 * p[0] - lastQ[0], 2 * p[1] - lastQ[1]] : p.slice(); const e = [ox + num(), oy + num()]; cubic(p, [p[0] + (2 / 3) * (q[0] - p[0]), p[1] + (2 / 3) * (q[1] - p[1])], [e[0] + (2 / 3) * (q[0] - e[0]), e[1] + (2 / 3) * (q[1] - e[1])], e, cur.pts); lastQ = q; lastC = null; p = e; break; }
      case 'A': { const rx = num(), ry = num(), phi = num(), fa = num(), fs = num(); const e = [ox + num(), oy + num()]; arcTo(p, rx, ry, phi, !!fa, !!fs, e, cur.pts); p = e; lastC = lastQ = null; break; }
      default: i++;
    }
  }
  return subs.filter((s) => s.pts.length > 1);
}

function unitToMm(v, fallbackPx = true) {
  if (v == null) return null;
  const m = String(v).trim().match(/^([-+]?[\d.]+(?:e[-+]?\d+)?)\s*(mm|cm|in|pt|pc|px)?$/i);
  if (!m) return null;
  const n = parseFloat(m[1]);
  const u = (m[2] || (fallbackPx ? 'px' : 'mm')).toLowerCase();
  return n * { mm: 1, cm: 10, in: 25.4, pt: 25.4 / 72, pc: 25.4 / 6, px: 25.4 / 96 }[u];
}

function parseColor(str) {
  if (!str || str === 'none') return null;
  const c = document.createElement('canvas').getContext('2d');
  c.fillStyle = '#000'; c.fillStyle = str;
  const v = c.fillStyle;
  if (v.startsWith('#')) return [parseInt(v.slice(1, 3), 16), parseInt(v.slice(3, 5), 16), parseInt(v.slice(5, 7), 16)];
  const m = v.match(/\d+/g); return m ? m.slice(0, 3).map(Number) : null;
}

// Returns [{subpaths (world mm, Y up), color:[r,g,b]|null}]
export function importSVG(text) {
  const doc = new DOMParser().parseFromString(text, 'image/svg+xml');
  const svg = doc.querySelector('svg');
  if (!svg) throw new Error('Not an SVG drawing');
  const vb = (svg.getAttribute('viewBox') || '').split(/[\s,]+/).map(Number);
  const wmm = unitToMm(svg.getAttribute('width')), hmm = unitToMm(svg.getAttribute('height'));
  let root = IDENT(), heightMm;
  if (vb.length === 4 && vb[2] > 0) {
    const sx = wmm ? wmm / vb[2] : 25.4 / 96, sy = hmm ? hmm / vb[3] : sx;
    root = [sx, 0, 0, sy, -vb[0] * sx, -vb[1] * sy];
    heightMm = hmm || vb[3] * sy;
  } else {
    const s = 25.4 / 96; root = [s, 0, 0, s, 0, 0];
    heightMm = hmm || 0;
  }
  const out = [];
  const styleOf = (el, inh) => {
    const st = { ...inh };
    const css = el.getAttribute('style') || '';
    const get = (k) => { const m = css.match(new RegExp('(?:^|;)\\s*' + k + '\\s*:\\s*([^;]+)')); return m ? m[1].trim() : el.getAttribute(k); };
    const s = get('stroke'), f = get('fill'), d = get('display');
    if (s != null) st.stroke = s; if (f != null) st.fill = f; if (d === 'none') st.hidden = true;
    return st;
  };
  const walk = (el, m, inh) => {
    const tag = el.tagName.replace(/^.*:/, '');
    if (['defs', 'clipPath', 'mask', 'symbol', 'style', 'title', 'desc', 'metadata', 'pattern', 'marker'].includes(tag)) return;
    const mm = mul(m, parseTransform(el.getAttribute('transform')));
    const st = styleOf(el, inh);
    if (st.hidden) return;
    const n = (k) => parseFloat(el.getAttribute(k)) || 0;
    let subs = null;
    switch (tag) {
      case 'path': subs = parsePathD(el.getAttribute('d') || ''); break;
      case 'rect': {
        const x = n('x'), y = n('y'), w = n('width'), h = n('height');
        let rx = n('rx') || n('ry'), ry = n('ry') || rx;
        rx = Math.min(rx, w / 2); ry = Math.min(ry, h / 2);
        const d = rx > 0 ? `M${x + rx},${y}H${x + w - rx}A${rx},${ry} 0 0 1 ${x + w},${y + ry}V${y + h - ry}A${rx},${ry} 0 0 1 ${x + w - rx},${y + h}H${x + rx}A${rx},${ry} 0 0 1 ${x},${y + h - ry}V${y + ry}A${rx},${ry} 0 0 1 ${x + rx},${y}Z`
          : `M${x},${y}H${x + w}V${y + h}H${x}Z`;
        subs = parsePathD(d); break;
      }
      case 'circle': case 'ellipse': {
        const cx = n('cx'), cy = n('cy'), rx = tag === 'circle' ? n('r') : n('rx'), ry = tag === 'circle' ? n('r') : n('ry');
        subs = parsePathD(`M${cx + rx},${cy}A${rx},${ry} 0 1 1 ${cx - rx},${cy}A${rx},${ry} 0 1 1 ${cx + rx},${cy}Z`); break;
      }
      case 'line': subs = [{ pts: [[n('x1'), n('y1')], [n('x2'), n('y2')]], closed: false }]; break;
      case 'polyline': case 'polygon': {
        const v = (el.getAttribute('points') || '').split(/[\s,]+/).filter(Boolean).map(Number);
        const pts = []; for (let k = 0; k + 1 < v.length; k += 2) pts.push([v[k], v[k + 1]]);
        if (tag === 'polygon' && pts.length) pts.push(pts[0].slice());
        subs = [{ pts, closed: tag === 'polygon' }]; break;
      }
    }
    if (subs && subs.length) {
      const full = mul(root, mm);
      out.push({
        subpaths: subs.map((s) => ({ closed: s.closed, pts: s.pts.map((p) => { const q = apply(full, p); return [q[0], heightMm - q[1]]; }) })),
        color: parseColor(st.stroke) || parseColor(st.fill),
      });
    }
    for (const ch of el.children) walk(ch, mm, st);
  };
  walk(svg, IDENT(), { stroke: null, fill: null });
  return out;
}

// Picture: load as an <img>, return its size in mm at the given dpi
export function loadImage(src) {
  return new Promise((res, rej) => { const im = new Image(); im.onload = () => res(im); im.onerror = () => rej(new Error('Could not read picture')); im.src = src; });
}

export function imageGrayData(img) {
  const c = document.createElement('canvas'); c.width = img.naturalWidth; c.height = img.naturalHeight;
  const g = c.getContext('2d', { willReadFrequently: true });
  g.fillStyle = '#fff'; g.fillRect(0, 0, c.width, c.height);
  g.drawImage(img, 0, 0);
  const d = g.getImageData(0, 0, c.width, c.height).data;
  const gray = new Float32Array(c.width * c.height);
  for (let i = 0; i < gray.length; i++) gray[i] = (0.299 * d[i * 4] + 0.587 * d[i * 4 + 1] + 0.114 * d[i * 4 + 2]) / 255;
  return { gray, w: c.width, h: c.height };
}

export { translate };
