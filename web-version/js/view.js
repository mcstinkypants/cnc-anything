// The design canvas: drawing the bed and shapes, and the mouse tools.
import { worldContours, bboxOf, hitShape, mul, apply, translate, scaleAbout, rotateAbout, IDENT, shapeBox } from './geom.js';
import { PALETTE } from './doc.js';

const RULER = 22;
const HANDLE = 7;

export class View {
  constructor(canvas, doc, app) {
    this.cv = canvas; this.g = canvas.getContext('2d'); this.doc = doc; this.app = app;
    this.z = 2; this.ox = 60; this.oy = 600; this.dpr = window.devicePixelRatio || 1;
    this.tool = 'select'; this.drag = null; this.mouse = [0, 0]; this.mouseScreen = [0, 0];
    this.cache = new WeakMap();
    this.preview = null; this.head = null; this.space = false;
    this.linePts = null; this.measure = null;
    new ResizeObserver(() => this.resize()).observe(canvas.parentElement);
    this.bind();
    this.resize();
  }
  get dev() { return this.app.dev; }

  // ---- coordinates ----
  toScreen([x, y]) { return [this.ox + x * this.z, this.oy - y * this.z]; }
  toWorld([sx, sy]) { return [(sx - this.ox) / this.z, (this.oy - sy) / this.z]; }
  resize() {
    const r = this.cv.parentElement.getBoundingClientRect();
    this.w = r.width; this.h = r.height; this.dpr = window.devicePixelRatio || 1;
    this.cv.width = Math.max(1, Math.round(r.width * this.dpr)); this.cv.height = Math.max(1, Math.round(r.height * this.dpr));
    if (!this.fitted && this.w > 50) { this.zoomFit(); this.fitted = true; }
    this.draw();
  }
  zoomTo(box, pad = 30) {
    const aw = this.w - RULER - pad * 2, ah = this.h - RULER - pad * 2;
    if (aw <= 0 || ah <= 0) return;
    this.z = Math.min(aw / Math.max(box.w, 1), ah / Math.max(box.h, 1), 200);
    this.ox = RULER + pad + (aw - box.w * this.z) / 2 - box.x0 * this.z;
    this.oy = RULER + pad + (ah - box.h * this.z) / 2 + box.y1 * this.z;
    this.draw();
  }
  zoomFit() { this.zoomTo({ x0: 0, y0: 0, x1: this.dev.bedW, y1: this.dev.bedH, w: this.dev.bedW, h: this.dev.bedH }); }
  zoomAt(f, sx, sy) {
    const [wx, wy] = this.toWorld([sx, sy]);
    this.z = Math.min(400, Math.max(0.05, this.z * f));
    this.ox = sx - wx * this.z; this.oy = sy + wy * this.z;
    this.draw();
  }

  contours(s) {
    const key = s.tf.join(',') + '|' + (s.w || 0) + '|' + (s.h || 0) + '|' + (s.r || 0) + '|' + (s.sides || 0) + '|' + (s.subpaths ? s.subpaths.length + ':' + (s.rev || 0) : '');
    const c = this.cache.get(s);
    if (c && c.key === key) return c.cs;
    const cs = worldContours(s);
    this.cache.set(s, { key, cs });
    return cs;
  }

  // ---- drawing ----
  draw() {
    if (this.raf) return;
    this.raf = requestAnimationFrame(() => { this.raf = 0; this.paint(); });
  }
  paint() {
    const g = this.g, d = this.dpr;
    g.setTransform(d, 0, 0, d, 0, 0);
    g.fillStyle = '#24262a'; g.fillRect(0, 0, this.w, this.h);
    // bed
    const [bx0, by0] = this.toScreen([0, this.dev.bedH]), [bx1, by1] = this.toScreen([this.dev.bedW, 0]);
    g.fillStyle = '#f4f4f2'; g.fillRect(bx0, by0, bx1 - bx0, by1 - by0);
    // grid
    const step = this.gridStep();
    g.lineWidth = 1;
    for (const [st, col] of [[step, '#e2e2df'], [step * 5, '#c9c9c5']]) {
      g.strokeStyle = col; g.beginPath();
      for (let x = 0; x <= this.dev.bedW + 1e-6; x += st) { const sx = Math.round(this.ox + x * this.z) + 0.5; g.moveTo(sx, by0); g.lineTo(sx, by1); }
      for (let uy = 0; uy <= this.dev.bedH + 1e-6; uy += st) { const sy = Math.round(this.oy - this.app.fromUser([0, uy])[1] * this.z) + 0.5; g.moveTo(bx0, sy); g.lineTo(bx1, sy); }
      g.stroke();
    }
    g.strokeStyle = '#888'; g.strokeRect(bx0 + 0.5, by0 + 0.5, bx1 - bx0, by1 - by0);
    // origin marker
    const [o0, o1] = this.toScreen(this.app.fromUser([0, 0]));
    g.fillStyle = '#ff6a2a'; g.beginPath(); g.arc(o0, o1, 4, 0, Math.PI * 2); g.fill();

    if (this.preview) this.paintPreview();
    else this.paintShapes();
    this.paintOverlay();
    this.paintRulers();
  }
  paintShapes() {
    const g = this.g, d = this.dpr;
    for (const s of this.doc.shapes) {
      const L = this.doc.layers[s.layer];
      if (!L.show) continue;
      if (s.type === 'image') {
        const im = this.app.imgCache.get(s.src);
        if (im) {
          const loc = [s.w / im.w, 0, 0, -s.h / im.h, 0, s.h];
          const m = mul(mul([this.z, 0, 0, -this.z, this.ox, this.oy], s.tf), loc);
          g.save(); g.setTransform(d * m[0], d * m[1], d * m[2], d * m[3], d * m[4], d * m[5]);
          g.filter = 'grayscale(1)'; g.globalAlpha = L.output ? 1 : 0.4;
          g.imageSmoothingEnabled = this.z * (s.w / im.w) < 3;
          g.drawImage(im.img, 0, 0); g.restore();
        }
      }
      const cs = this.contours(s);
      if (L.mode !== 'Line' && s.type !== 'image') {
        g.fillStyle = PALETTE[s.layer] + '30';
        g.beginPath();
        for (const c of cs) if (c.closed) this.pathOf(c.pts);
        g.fill('evenodd');
      }
      g.strokeStyle = PALETTE[s.layer]; g.lineWidth = this.doc.sel.has(s.id) ? 1.6 : 1.1;
      g.setLineDash(this.doc.sel.has(s.id) ? [5, 3] : []);
      if (!L.output) g.globalAlpha = 0.45;
      g.beginPath(); for (const c of cs) this.pathOf(c.pts); g.stroke();
      g.globalAlpha = 1; g.setLineDash([]);
    }
  }
  pathOf(pts) {
    const g = this.g;
    for (let i = 0; i < pts.length; i++) { const [x, y] = this.toScreen(pts[i]); if (i) g.lineTo(x, y); else g.moveTo(x, y); }
  }
  paintPreview() {
    const g = this.g, P = this.preview, segs = P.segs;
    const upto = Math.floor(segs.length * P.frac);
    let head = null;
    g.lineCap = 'round';
    // fills/images: draw cut segments thick (line width = interval) for a burn-like look
    for (let i = 0; i < upto; i++) {
      const [x0, y0, x1, y1, p, li] = segs[i];
      const a = this.toScreen([x0, y0]), b = this.toScreen([x1, y1]);
      head = b;
      if (p > 0) {
        const L = this.doc.layers[li];
        const wpx = L.mode === 'Line' ? 1.4 : Math.max(1, L.interval * this.z);
        const shade = Math.round(255 - (p / Math.max(1, L.power)) * 200);
        g.strokeStyle = L.mode === 'Line' ? PALETTE[li] : `rgb(${shade * 0.45 | 0},${shade * 0.35 | 0},${shade * 0.3 | 0})`;
        g.lineWidth = wpx; g.lineCap = L.mode === 'Line' ? 'round' : 'butt';
        g.beginPath(); g.moveTo(a[0], a[1]); g.lineTo(b[0], b[1]); g.stroke();
      } else if (P.travel) {
        g.strokeStyle = 'rgba(220,40,40,0.55)'; g.lineWidth = 0.8; g.setLineDash([4, 3]);
        g.beginPath(); g.moveTo(a[0], a[1]); g.lineTo(b[0], b[1]); g.stroke(); g.setLineDash([]);
      }
    }
    if (head) { g.strokeStyle = '#ff2a2a'; g.lineWidth = 2; g.beginPath(); g.arc(head[0], head[1], 6, 0, Math.PI * 2); g.stroke(); }
  }
  paintOverlay() {
    const g = this.g;
    // selection box & handles
    if (!this.preview) {
      const b = this.doc.selBox();
      if (b && this.tool === 'select') {
        const [x0, y0] = this.toScreen([b.x0, b.y1]), [x1, y1] = this.toScreen([b.x1, b.y0]);
        g.strokeStyle = '#2f6fe0'; g.lineWidth = 1; g.setLineDash([3, 3]);
        g.strokeRect(x0 - 0.5, y0 - 0.5, x1 - x0 + 1, y1 - y0 + 1); g.setLineDash([]);
        for (const h of this.handles(b)) {
          g.fillStyle = h.k === 'rot' ? '#ff6a2a' : '#fff'; g.strokeStyle = '#2f6fe0';
          if (h.k === 'rot') { g.beginPath(); g.arc(h.x, h.y, 5, 0, Math.PI * 2); g.fill(); g.stroke(); g.beginPath(); g.moveTo(h.x, h.y + 5); g.lineTo(h.x, y0); g.stroke(); }
          else { g.fillRect(h.x - HANDLE / 2, h.y - HANDLE / 2, HANDLE, HANDLE); g.strokeRect(h.x - HANDLE / 2, h.y - HANDLE / 2, HANDLE, HANDLE); }
        }
        // center cross
        const cx = (x0 + x1) / 2, cy = (y0 + y1) / 2;
        g.beginPath(); g.moveTo(cx - 4, cy); g.lineTo(cx + 4, cy); g.moveTo(cx, cy - 4); g.lineTo(cx, cy + 4); g.stroke();
      }
    }
    const D = this.drag;
    if (D && D.kind === 'box') {
      const ltr = D.cur[0] >= D.start[0];
      g.strokeStyle = ltr ? '#2f6fe0' : '#2fa050'; g.fillStyle = ltr ? '#2f6fe018' : '#2fa05018';
      g.setLineDash(ltr ? [] : [5, 3]);
      const x = Math.min(D.start[0], D.cur[0]), y = Math.min(D.start[1], D.cur[1]);
      g.fillRect(x, y, Math.abs(D.cur[0] - D.start[0]), Math.abs(D.cur[1] - D.start[1]));
      g.strokeRect(x + 0.5, y + 0.5, Math.abs(D.cur[0] - D.start[0]), Math.abs(D.cur[1] - D.start[1])); g.setLineDash([]);
    }
    if (D && D.kind === 'create' && D.ghost) {
      g.strokeStyle = PALETTE[this.app.activeLayer]; g.lineWidth = 1.2; g.beginPath();
      for (const c of worldContours(D.ghost)) this.pathOf(c.pts); g.stroke();
      const b = shapeBox(D.ghost);
      if (b) this.label(`${b.w.toFixed(1)} × ${b.h.toFixed(1)} mm`, this.mouseScreen[0] + 14, this.mouseScreen[1] + 18);
    }
    if (this.linePts) {
      g.strokeStyle = PALETTE[this.app.activeLayer]; g.lineWidth = 1.2; g.beginPath();
      this.pathOf([...this.linePts, this.mouse]); g.stroke();
      const [fx, fy] = this.toScreen(this.linePts[0]);
      g.strokeStyle = '#2f6fe0'; g.strokeRect(fx - 4, fy - 4, 8, 8);
    }
    if (this.measure) {
      const [a, b] = this.measure; const A = this.toScreen(a), B = this.toScreen(b);
      g.strokeStyle = '#d040d0'; g.lineWidth = 1.5; g.beginPath(); g.moveTo(...A); g.lineTo(...B); g.stroke();
      const ua = this.app.toUser(a), ub = this.app.toUser(b);
      const dist = Math.hypot(b[0] - a[0], b[1] - a[1]), ang = (Math.atan2(b[1] - a[1], b[0] - a[0]) * 180) / Math.PI;
      this.label(`${dist.toFixed(2)} mm  ·  ${ang.toFixed(1)}°  ·  dx ${(ub[0] - ua[0]).toFixed(2)}  dy ${(ub[1] - ua[1]).toFixed(2)}`, B[0] + 12, B[1] + 16);
    }
    // snap guides
    if (this.guides && this.guides.length) {
      g.strokeStyle = '#d020d0'; g.lineWidth = 1; g.setLineDash([6, 4]); g.beginPath();
      for (const gd of this.guides) {
        if (gd.x != null) { const sx = Math.round(this.ox + gd.x * this.z) + 0.5; g.moveTo(sx, RULER); g.lineTo(sx, this.h); }
        else { const sy = Math.round(this.oy - gd.y * this.z) + 0.5; g.moveTo(RULER, sy); g.lineTo(this.w, sy); }
      }
      g.stroke(); g.setLineDash([]);
    }
    if (this.snapMark) {
      const [mx, my] = this.toScreen(this.snapMark);
      g.strokeStyle = '#d020d0'; g.lineWidth = 1.5; g.beginPath(); g.arc(mx, my, 6, 0, Math.PI * 2);
      g.moveTo(mx - 9, my); g.lineTo(mx + 9, my); g.moveTo(mx, my - 9); g.lineTo(mx, my + 9); g.stroke();
    }
    // laser head
    if (this.head) {
      const [hx, hy] = this.toScreen(this.head);
      g.strokeStyle = '#18b048'; g.lineWidth = 2;
      g.beginPath(); g.arc(hx, hy, 7, 0, Math.PI * 2); g.moveTo(hx - 12, hy); g.lineTo(hx + 12, hy); g.moveTo(hx, hy - 12); g.lineTo(hx, hy + 12); g.stroke();
    }
    if (this.frameBox) {
      const b = this.frameBox; const [x0, y0] = this.toScreen([b.x0, b.y1]), [x1, y1] = this.toScreen([b.x1, b.y0]);
      g.strokeStyle = '#18b048'; g.setLineDash([6, 4]); g.lineWidth = 1.5; g.strokeRect(x0, y0, x1 - x0, y1 - y0); g.setLineDash([]);
    }
  }
  label(text, x, y) {
    const g = this.g; g.font = '12px system-ui, sans-serif';
    const w = g.measureText(text).width + 10;
    g.fillStyle = '#1f2124e8'; g.fillRect(x, y - 13, w, 19);
    g.fillStyle = '#fff'; g.fillText(text, x + 5, y + 1);
  }
  gridStep() {
    const want = 12 / this.z; // at least ~12px between minor lines
    for (const s of [0.5, 1, 2, 5, 10, 20, 50, 100, 200]) if (s >= want) return s;
    return 500;
  }
  paintRulers() {
    const g = this.g;
    g.fillStyle = '#2b2d31'; g.fillRect(0, 0, this.w, RULER); g.fillRect(0, 0, RULER, this.h);
    g.strokeStyle = '#555'; g.fillStyle = '#aaa'; g.font = '10px system-ui, sans-serif'; g.lineWidth = 1;
    let major = this.gridStep() * 5; while (major * this.z < 60) major *= 2;
    const minor = major / 5;
    const [wx0] = this.toWorld([RULER, 0]), [wx1] = this.toWorld([this.w, 0]);
    g.beginPath();
    for (let x = Math.floor(wx0 / minor) * minor; x <= wx1; x += minor) {
      const sx = Math.round(this.ox + x * this.z) + 0.5; if (sx < RULER) continue;
      const isMaj = Math.abs(x / major - Math.round(x / major)) < 1e-6;
      g.moveTo(sx, RULER); g.lineTo(sx, isMaj ? 6 : 15);
      if (isMaj) g.fillText(String(Math.round(x)), sx + 2, 10);
    }
    const uA = this.app.toUser([0, this.toWorld([0, RULER])[1]])[1], uB = this.app.toUser([0, this.toWorld([0, this.h])[1]])[1];
    for (let y = Math.floor(Math.min(uA, uB) / minor) * minor; y <= Math.max(uA, uB); y += minor) {
      const sy = Math.round(this.oy - this.app.fromUser([0, y])[1] * this.z) + 0.5; if (sy < RULER) continue;
      const isMaj = Math.abs(y / major - Math.round(y / major)) < 1e-6;
      g.moveTo(RULER, sy); g.lineTo(isMaj ? 6 : 15, sy);
      if (isMaj) { g.save(); g.translate(10, sy - 2); g.rotate(-Math.PI / 2); g.fillText(String(Math.round(y)), 0, 0); g.restore(); }
    }
    g.stroke();
    // mouse markers
    g.strokeStyle = '#ff6a2a'; g.beginPath();
    g.moveTo(this.mouseScreen[0] + 0.5, 0); g.lineTo(this.mouseScreen[0] + 0.5, RULER);
    g.moveTo(0, this.mouseScreen[1] + 0.5); g.lineTo(RULER, this.mouseScreen[1] + 0.5); g.stroke();
    g.fillStyle = '#2b2d31'; g.fillRect(0, 0, RULER, RULER);
  }

  handles(b) {
    const [x0, y0] = this.toScreen([b.x0, b.y1]), [x1, y1] = this.toScreen([b.x1, b.y0]);
    const mx = (x0 + x1) / 2, my = (y0 + y1) / 2;
    return [
      { k: 'nw', x: x0, y: y0 }, { k: 'n', x: mx, y: y0 }, { k: 'ne', x: x1, y: y0 },
      { k: 'w', x: x0, y: my }, { k: 'e', x: x1, y: my },
      { k: 'sw', x: x0, y: y1 }, { k: 's', x: mx, y: y1 }, { k: 'se', x: x1, y: y1 },
      { k: 'rot', x: mx, y: y0 - 24 },
    ];
  }

  // ---- input ----
  bind() {
    const cv = this.cv;
    const pos = (e) => { const r = cv.getBoundingClientRect(); return [e.clientX - r.left, e.clientY - r.top]; };
    cv.addEventListener('wheel', (e) => { e.preventDefault(); const [sx, sy] = pos(e); this.zoomAt(Math.exp(-e.deltaY * 0.0015), sx, sy); this.app.updateStatus(); }, { passive: false });
    cv.addEventListener('contextmenu', (e) => e.preventDefault());
    cv.addEventListener('pointerdown', (e) => { try { cv.setPointerCapture(e.pointerId); } catch (err) { /* synthetic */ } this.down(pos(e), e); });
    cv.addEventListener('pointermove', (e) => this.move(pos(e), e));
    cv.addEventListener('pointerup', (e) => this.up(pos(e), e));
    cv.addEventListener('dblclick', (e) => this.dbl(pos(e), e));
    window.addEventListener('keydown', (e) => { if (e.code === 'Space' && !/INPUT|TEXTAREA|SELECT/.test(document.activeElement.tagName)) { this.space = true; cv.style.cursor = 'grab'; } });
    window.addEventListener('keyup', (e) => { if (e.code === 'Space') { this.space = false; this.updateCursor(); } });
  }
  setTool(t) {
    this.tool = t; this.linePts = null; this.measure = null; this.drag = null;
    this.updateCursor(); this.draw();
  }
  updateCursor() {
    this.cv.style.cursor = this.tool === 'pan' ? 'grab' : this.tool === 'select' ? 'default' : this.tool === 'text' ? 'text' : 'crosshair';
  }

  // ---- snapping ----
  snapStep() { return this.app.gridSnapStep ? this.app.gridSnapStep : this.gridStep(); }
  snapGridPt(wp) {
    const s = this.snapStep(), [ux, uy] = this.app.toUser(wp);
    return this.app.fromUser([Math.round(ux / s) * s, Math.round(uy / s) * s]);
  }
  // centers of shapes (not in `exclude`) plus the bed center
  snapCenters(exclude) {
    const out = [[this.dev.bedW / 2, this.dev.bedH / 2]];
    for (const s of this.doc.shapes) {
      if (exclude && exclude.has(s.id)) continue;
      if (!this.doc.layers[s.layer].show) continue;
      const b = bboxOf(this.contours(s)); if (b) out.push([(b.x0 + b.x1) / 2, (b.y0 + b.y1) / 2]);
    }
    return out;
  }
  snapPoint(wp, e, exclude) {
    this.snapMark = null;
    if (e && e.altKey) return wp;
    if (this.app.snapObj) {
      let best = null, bd = 9 / this.z;
      for (const c of this.snapCenters(exclude)) { const d = Math.hypot(c[0] - wp[0], c[1] - wp[1]); if (d < bd) { bd = d; best = c; } }
      if (best) { this.snapMark = best; return best.slice(); }
    }
    if (this.app.snapGrid) return this.snapGridPt(wp);
    return wp;
  }

  hitTest(p) {
    const tol = 5 / this.z;
    for (let i = this.doc.shapes.length - 1; i >= 0; i--) {
      const s = this.doc.shapes[i];
      if (!this.doc.layers[s.layer].show) continue;
      if (hitShape(s, p, tol)) return s;
    }
    return null;
  }

  down(sp, e) {
    let wp = this.toWorld(sp);
    this.mouseDown = sp;
    if (e.button === 1 || this.space || this.tool === 'pan') { this.drag = { kind: 'pan', start: sp, ox: this.ox, oy: this.oy }; this.cv.style.cursor = 'grabbing'; return; }
    if (this.preview) return;
    if (e.button === 2) { if (this.linePts) this.finishLine(false); return; }
    const doc = this.doc;
    if (this.tool === 'select') {
      const b = doc.selBox();
      if (b) {
        const h = this.handles(b).find((h) => Math.abs(h.x - sp[0]) <= 7 && Math.abs(h.y - sp[1]) <= 7);
        if (h) { this.drag = { kind: h.k === 'rot' ? 'rotate' : 'scale', h: h.k, box: b, start: wp, orig: doc.selected().map((s) => [s, s.tf.slice()]) }; doc.checkpoint(); return; }
      }
      const hit = this.hitTest(wp);
      if (hit) {
        if (e.shiftKey || e.ctrlKey) { if (doc.sel.has(hit.id)) doc.sel.delete(hit.id); else doc.sel.add(hit.id); }
        else if (!doc.sel.has(hit.id)) { doc.sel.clear(); doc.sel.add(hit.id); }
        this.app.selectionChanged();
        if (doc.sel.has(hit.id)) this.drag = this.moveDrag(wp);
        this.draw(); return;
      }
      // inside selection box also moves
      if (b && wp[0] >= b.x0 && wp[0] <= b.x1 && wp[1] >= b.y0 && wp[1] <= b.y1 && !e.shiftKey) {
        this.drag = this.moveDrag(wp); return;
      }
      if (!e.shiftKey) { doc.sel.clear(); this.app.selectionChanged(); }
      this.drag = { kind: 'box', start: sp, cur: sp, add: e.shiftKey };
      this.draw(); return;
    }
    if (['rect', 'ellipse', 'polygon'].includes(this.tool)) { this.drag = { kind: 'create', start: this.snapPoint(wp, e), ghost: null }; return; }
    if (this.tool === 'line') {
      wp = this.snapPoint(wp, e);
      if (!this.linePts) this.linePts = [wp];
      else {
        const f = this.linePts[0];
        if (this.linePts.length > 2 && Math.hypot(f[0] - wp[0], f[1] - wp[1]) * this.z < 8) { this.linePts.push(f.slice()); this.finishLine(true); return; }
        this.linePts.push(this.snapAngle(wp, e));
      }
      this.draw(); return;
    }
    if (this.tool === 'text') { this.app.textDialog(null, this.snapPoint(wp, e)); this.snapMark = null; return; }
    if (this.tool === 'measure') { wp = this.snapPoint(wp, e); this.measure = [wp, wp]; this.drag = { kind: 'measure' }; this.draw(); }
  }
  moveDrag(wp) {
    const doc = this.doc;
    return { kind: 'move', start: wp, orig: doc.selected().map((s) => [s, s.tf.slice()]), moved: false, box: doc.selBox(), centers: this.snapCenters(doc.sel) };
  }
  snapAngle(wp, e) {
    if (!e.shiftKey || !this.linePts?.length) return wp;
    const a = this.linePts[this.linePts.length - 1];
    const ang = Math.round(Math.atan2(wp[1] - a[1], wp[0] - a[0]) / (Math.PI / 12)) * (Math.PI / 12);
    const d = Math.hypot(wp[0] - a[0], wp[1] - a[1]);
    return [a[0] + d * Math.cos(ang), a[1] + d * Math.sin(ang)];
  }
  finishLine(closed) {
    const pts = this.linePts; this.linePts = null;
    if (pts && pts.length > 1) this.app.addShape({ type: 'path', subpaths: [{ pts, closed }] });
    this.draw();
  }
  move(sp, e) {
    let wp = this.toWorld(sp);
    this.mouse = this.linePts ? this.snapAngle(wp, e) : wp; this.mouseScreen = sp;
    this.app.cursorMoved(wp);
    const D = this.drag;
    if (!D && ['rect', 'ellipse', 'polygon', 'line', 'text', 'measure'].includes(this.tool)) {
      const sp2 = this.snapPoint(wp, e);
      if (this.linePts) this.mouse = this.snapAngle(sp2, e);
    } else if (!D) this.snapMark = null;
    if (!D) {
      if (this.tool === 'select') {
        const b = this.doc.selBox();
        const h = b && this.handles(b).find((h) => Math.abs(h.x - sp[0]) <= 7 && Math.abs(h.y - sp[1]) <= 7);
        this.cv.style.cursor = h ? ({ n: 'ns-resize', s: 'ns-resize', e: 'ew-resize', w: 'ew-resize', nw: 'nwse-resize', se: 'nwse-resize', ne: 'nesw-resize', sw: 'nesw-resize', rot: 'alias' })[h.k] : this.hitTest(wp) ? 'move' : this.space ? 'grab' : 'default';
      }
      this.draw(); return;
    }
    const doc = this.doc;
    if (D.kind === 'pan') { this.ox = D.ox + sp[0] - D.start[0]; this.oy = D.oy + sp[1] - D.start[1]; this.draw(); return; }
    if (D.kind === 'box') { D.cur = sp; this.draw(); return; }
    if (D.kind === 'measure') { const q = this.snapPoint(wp, e); this.measure[1] = e.shiftKey ? this.snapFrom(this.measure[0], q) : q; this.draw(); return; }
    if (D.kind === 'move') {
      let dx = wp[0] - D.start[0], dy = wp[1] - D.start[1];
      if (!D.moved && Math.hypot(dx, dy) * this.z < 3) return;
      if (!D.moved) { doc.checkpoint(); D.moved = true; }
      if (e.shiftKey) { if (Math.abs(dx) > Math.abs(dy)) dy = 0; else dx = 0; }
      this.guides = [];
      if (!e.altKey && D.box) {
        const b = D.box, cx = (b.x0 + b.x1) / 2 + dx, cy = (b.y0 + b.y1) / 2 + dy;
        let doneX = false, doneY = false;
        if (this.app.snapObj) {
          const tol = 9 / this.z; let bx = null, by = null, ex = tol, ey = tol;
          for (const c of D.centers) {
            if (Math.abs(c[0] - cx) < ex) { ex = Math.abs(c[0] - cx); bx = c[0]; }
            if (Math.abs(c[1] - cy) < ey) { ey = Math.abs(c[1] - cy); by = c[1]; }
          }
          if (bx != null && !(e.shiftKey && dx === 0)) { dx += bx - cx; this.guides.push({ x: bx }); doneX = true; }
          if (by != null && !(e.shiftKey && dy === 0)) { dy += by - cy; this.guides.push({ y: by }); doneY = true; }
        }
        if (this.app.snapGrid) {
          // the selection's upper-left corner (as the rulers count) lands on the grid
          const refY = this.app.dev.originTop ? b.y1 : b.y0;
          const q = this.snapGridPt([b.x0 + dx, refY + dy]);
          if (!doneX && !(e.shiftKey && dx === 0)) dx = q[0] - b.x0;
          if (!doneY && !(e.shiftKey && dy === 0)) dy = q[1] - refY;
        }
      }
      for (const [s, tf] of D.orig) s.tf = mul(translate(dx, dy), tf);
      this.app.selectionMoved(); this.draw(); return;
    }
    if (D.kind === 'scale') {
      wp = this.snapPoint(wp, e, this.doc.sel);
      const b = D.box, h = D.h;
      const ax = h.includes('w') ? b.x1 : h.includes('e') ? b.x0 : (b.x0 + b.x1) / 2;
      const ay = h.includes('n') ? b.y0 : h.includes('s') ? b.y1 : (b.y0 + b.y1) / 2;
      let sx = h.includes('e') || h.includes('w') ? (wp[0] - ax) / ((h.includes('e') ? b.x1 : b.x0) - ax || 1e-9) : 1;
      let sy = h.includes('n') || h.includes('s') ? (wp[1] - ay) / ((h.includes('n') ? b.y1 : b.y0) - ay || 1e-9) : 1;
      const corner = h.length === 2;
      const keep = corner ? !e.shiftKey && this.app.lockRatio : e.shiftKey;
      if (keep) { const s = corner ? (Math.abs(sx) > Math.abs(sy) ? sx : sy) : (h === 'n' || h === 's' ? sy : sx); sx = s; sy = s; }
      if (Math.abs(sx) < 1e-3) sx = 1e-3; if (Math.abs(sy) < 1e-3) sy = 1e-3;
      const M = scaleAbout(sx, sy, ax, ay);
      for (const [s, tf] of D.orig) s.tf = mul(M, tf);
      this.app.selectionMoved(); this.draw(); return;
    }
    if (D.kind === 'rotate') {
      const b = D.box, cx = (b.x0 + b.x1) / 2, cy = (b.y0 + b.y1) / 2;
      let ang = (Math.atan2(wp[1] - cy, wp[0] - cx) - Math.atan2(D.start[1] - cy, D.start[0] - cx)) * 180 / Math.PI;
      if (e.shiftKey) ang = Math.round(ang / 15) * 15;
      const M = rotateAbout(ang, cx, cy);
      for (const [s, tf] of D.orig) s.tf = mul(M, tf);
      this.rotAngle = ang; this.app.selectionMoved(); this.draw(); return;
    }
    if (D.kind === 'create') {
      let [x0, y0] = D.start, [x1, y1] = this.snapPoint(wp, e);
      if (e.shiftKey || e.ctrlKey) { const m = Math.max(Math.abs(x1 - x0), Math.abs(y1 - y0)); x1 = x0 + Math.sign(x1 - x0 || 1) * m; y1 = y0 + Math.sign(y1 - y0 || 1) * m; }
      const w = Math.abs(x1 - x0), hh = Math.abs(y1 - y0);
      D.ghost = { type: this.tool, w, h: hh, r: 0, sides: this.app.polySides, tf: translate(Math.min(x0, x1), Math.min(y0, y1)) };
      this.draw();
    }
  }
  snapFrom(a, wp) {
    const ang = Math.round(Math.atan2(wp[1] - a[1], wp[0] - a[0]) / (Math.PI / 12)) * (Math.PI / 12);
    const d = Math.hypot(wp[0] - a[0], wp[1] - a[1]);
    return [a[0] + d * Math.cos(ang), a[1] + d * Math.sin(ang)];
  }
  up(sp, e) {
    const D = this.drag; this.drag = null;
    this.guides = []; this.snapMark = null;
    if (!D) return;
    const doc = this.doc;
    if (D.kind === 'pan') { this.updateCursor(); return; }
    if (D.kind === 'box') {
      const a = this.toWorld(D.start), b = this.toWorld(sp);
      const x0 = Math.min(a[0], b[0]), x1 = Math.max(a[0], b[0]), y0 = Math.min(a[1], b[1]), y1 = Math.max(a[1], b[1]);
      const enclose = sp[0] >= D.start[0];
      if (Math.abs(sp[0] - D.start[0]) > 3 || Math.abs(sp[1] - D.start[1]) > 3) {
        for (const s of doc.shapes) {
          if (!doc.layers[s.layer].show) continue;
          const sb = shapeBox(s); if (!sb) continue;
          const inside = sb.x0 >= x0 && sb.x1 <= x1 && sb.y0 >= y0 && sb.y1 <= y1;
          const touch = sb.x1 >= x0 && sb.x0 <= x1 && sb.y1 >= y0 && sb.y0 <= y1;
          if (enclose ? inside : touch) doc.sel.add(s.id);
        }
      }
      this.app.selectionChanged(); this.draw(); return;
    }
    if (D.kind === 'move') { if (D.moved) this.app.afterEdit(); return; }
    if (D.kind === 'scale' || D.kind === 'rotate') { this.rotAngle = null; this.app.afterEdit(); return; }
    if (D.kind === 'measure') return;
    if (D.kind === 'create') {
      if (D.ghost && D.ghost.w * this.z > 2 && D.ghost.h * this.z > 2) {
        const g = D.ghost;
        this.app.addShape({ type: g.type, w: g.w, h: g.h, r: 0, sides: g.sides, tf: g.tf });
      }
      this.draw();
    }
  }
  dbl(sp) {
    if (this.tool === 'line' && this.linePts) { this.linePts.pop(); this.finishLine(false); return; }
    if (this.tool !== 'select') return;
    const hit = this.hitTest(this.toWorld(sp));
    if (hit && hit.type === 'text') this.app.textDialog(hit);
  }
}

export { IDENT, bboxOf, apply };
