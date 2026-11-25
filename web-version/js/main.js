// Burner: app wiring (menus, panels, files, machine).
import { Doc, PALETTE, cloneShape, nearestPaletteIndex, newId, defaultLayer } from './doc.js';
import { View } from './view.js';
import { translate, mul, scaleAbout, rotateAbout, shapeBox, worldContours, matRotation, IDENT, localContours } from './geom.js';
import { textOutlines, importSVG, loadImage, imageGrayData } from './importers.js';
import { planJob, emitGcode, defaultDevice, fmtTime } from './gcode.js';
import { Grbl } from './machine.js';

const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];

class App {
  constructor() {
    this.dev = { ...defaultDevice(), ...JSON.parse(localStorage.getItem('ls.device') || '{}') };
    this.doc = new Doc();
    this.imgCache = new Map();
    this.activeLayer = 0; this.editLayer = null;
    this.lockRatio = true; this.polySides = 6;
    this.clip = null; this.docName = 'Untitled';
    this.anchor = 'bl';
    const snap = JSON.parse(localStorage.getItem('ls.snap') || '{"grid":true,"obj":true,"step":0}');
    this.snapGrid = snap.grid; this.snapObj = snap.obj; this.gridSnapStep = +snap.step || 0;
    this.grbl = new Grbl();
    this.view = new View($('#cv'), this.doc, this);
    this.bindSnap(); this.buildPalette(); this.bindMenus(); this.bindKeys(); this.bindNumeric(); this.bindLaser(); this.bindConsole();
    this.doc.on(() => { this.refreshCuts(); this.refreshNumeric(); this.scheduleSave(); });
    this.restore();
    this.refreshCuts(); this.refreshNumeric(); this.updateStatus();
  }

  // ---------------------------------------------------------------- coordinates
  // Inside, Y grows up from the front edge of the bed. What the person sees (rulers,
  // numbers, G-code) counts from the origin corner chosen in Device settings.
  toUser([x, y]) { return [x, this.dev.originTop ? this.dev.bedH - y : y]; }
  fromUser([x, y]) { return [x, this.dev.originTop ? this.dev.bedH - y : y]; }
  bindSnap() {
    const save = () => localStorage.setItem('ls.snap', JSON.stringify({ grid: this.snapGrid, obj: this.snapObj, step: this.gridSnapStep }));
    const show = () => { $('#snapGrid').classList.toggle('on', this.snapGrid); $('#snapObj').classList.toggle('on', this.snapObj); $('#snapStep').value = String(this.gridSnapStep); $('#snapStep').disabled = !this.snapGrid; };
    $('#snapGrid').onclick = () => { this.snapGrid = !this.snapGrid; save(); show(); this.toast(this.snapGrid ? 'Snap to grid on' : 'Snap to grid off'); };
    $('#snapObj').onclick = () => { this.snapObj = !this.snapObj; save(); show(); this.toast(this.snapObj ? 'Snap to object centers on' : 'Snap to object centers off'); };
    $('#snapStep').onchange = (e) => { this.gridSnapStep = +e.target.value || 0; save(); };
    show();
  }

  // ---------------------------------------------------------------- helpers
  toast(msg, err = false) {
    const t = $('#toast'); t.textContent = msg; t.className = 'show' + (err ? ' err' : '');
    clearTimeout(this.toastT); this.toastT = setTimeout(() => (t.className = ''), err ? 5000 : 2600);
    $('#sbMsg').textContent = msg;
  }
  info(title, html) { $('#infoTitle').textContent = title; $('#infoBody').innerHTML = html; $('#dlgInfo').showModal(); }
  download(name, data, type) {
    const a = document.createElement('a');
    a.href = URL.createObjectURL(new Blob([data], { type }));
    a.download = name; document.body.appendChild(a); a.click(); a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 2000);
  }
  saveDevice() { localStorage.setItem('ls.device', JSON.stringify(this.dev)); }
  scheduleSave() {
    clearTimeout(this.saveT);
    this.saveT = setTimeout(() => {
      try { localStorage.setItem('ls.autosave', JSON.stringify({ name: this.docName, doc: this.doc.toJSON() })); } catch (e) { /* too big for local storage */ }
    }, 600);
  }
  async restore() {
    const raw = localStorage.getItem('ls.autosave');
    if (raw) {
      try { const o = JSON.parse(raw); await this.loadProject(o.doc, o.name); return; } catch (e) { /* ignore */ }
    }
    this.loadSample();
  }

  // ---------------------------------------------------------------- shapes
  addShape(props, select = true) {
    this.doc.checkpoint();
    const s = this.doc.add({ layer: this.activeLayer, ...props });
    if (select) { this.doc.sel.clear(); this.doc.sel.add(s.id); }
    this.view.setTool('select'); this.setToolButtons('select');
    this.afterEdit(); this.selectionChanged();
    return s;
  }
  afterEdit() { this.doc.changed(); this.view.draw(); }
  selectionChanged() { this.refreshNumeric(); this.refreshShapeOpts(); this.updateStatus(); this.view.draw(); }
  selectionMoved() { this.refreshNumeric(); }
  cursorMoved(wp) { const [x, y] = this.toUser(wp); $('#sbPos').textContent = `x: ${x.toFixed(2)}  y: ${y.toFixed(2)} mm`; }
  updateStatus() {
    const n = this.doc.sel.size;
    $('#sbSel').textContent = n ? `${n} selected` : `${this.doc.shapes.length} shape${this.doc.shapes.length === 1 ? '' : 's'}`;
    $('#sbZoom').textContent = `${Math.round(this.view.z * 100 / 3.78)}%`;
  }

  // ---------------------------------------------------------------- text
  textDialog(shape, wp) {
    const dlg = $('#dlgText'), f = dlg.querySelector('form');
    const t = shape || { text: 'Hello', font: 'sans-serif', size: 12, align: 'left', bold: false, italic: false };
    f.text.value = t.text; f.font.value = t.font; f.size.value = t.size; f.align.value = t.align || 'left'; f.bold.checked = !!t.bold; f.italic.checked = !!t.italic;
    dlg.returnValue = '';
    dlg.onclose = () => {
      if (dlg.returnValue !== 'ok' || !f.text.value.trim()) return;
      const spec = { text: f.text.value, font: f.font.value, size: Math.max(1, +f.size.value || 12), align: f.align.value, bold: f.bold.checked, italic: f.italic.checked };
      const o = textOutlines(spec);
      if (!o.subpaths.length) { this.toast('That text produced no outline.', true); return; }
      if (shape) {
        this.doc.checkpoint();
        Object.assign(shape, spec, { subpaths: o.subpaths, w: o.w, h: o.h, rev: (shape.rev || 0) + 1 });
        this.afterEdit(); this.selectionChanged();
      } else {
        this.addShape({ type: 'text', ...spec, subpaths: o.subpaths, w: o.w, h: o.h, tf: translate(wp[0], wp[1]) });
      }
    };
    dlg.showModal(); f.text.select();
  }

  // ---------------------------------------------------------------- palette
  buildPalette() {
    const p = $('#palette');
    p.innerHTML = '<span class="label">Layer color</span>';
    PALETTE.forEach((c, i) => {
      const s = document.createElement('span');
      s.className = 'swatch'; s.style.background = c; s.textContent = String(i).padStart(2, '0');
      s.title = `Layer C${String(i).padStart(2, '0')}: click to put the selected shapes on this layer`;
      s.onclick = () => {
        this.activeLayer = i;
        const sel = this.doc.selected();
        if (sel.length) {
          this.doc.checkpoint();
          for (const sh of sel) sh.layer = i;
          if (sel.some((sh) => sh.type === 'image')) this.doc.layers[i].mode = 'Image';
          this.editLayer = i;
          this.afterEdit();
          this.toast(`Moved ${sel.length} shape${sel.length > 1 ? 's' : ''} to layer C${String(i).padStart(2, '0')}`);
        } else this.toast(`New shapes will go on layer C${String(i).padStart(2, '0')}`);
        this.refreshPalette();
      };
      p.appendChild(s);
    });
    this.refreshPalette();
  }
  refreshPalette() { $$('#palette .swatch').forEach((s, i) => (s.style.outline = i === this.activeLayer ? '2px solid #fff' : '')); }

  // ---------------------------------------------------------------- cuts
  refreshCuts() {
    const used = this.doc.usedLayers();
    const tb = $('#cutsTable tbody');
    tb.innerHTML = '';
    if (this.editLayer == null || !used.includes(this.editLayer)) this.editLayer = used[0] ?? null;
    used.forEach((li, n) => {
      const L = this.doc.layers[li];
      const tr = document.createElement('tr');
      if (li === this.editLayer) tr.className = 'sel';
      tr.innerHTML = `<td>${n + 1}</td><td><span class="swatch" style="background:${PALETTE[li]}">${String(li).padStart(2, '0')}</span></td>
        <td>${L.mode}</td><td>${+L.speed.toFixed(1)} / ${+L.power.toFixed(1)}%</td>
        <td><input type="checkbox" data-k="output" ${L.output ? 'checked' : ''}></td>
        <td><input type="checkbox" data-k="show" ${L.show ? 'checked' : ''}></td>`;
      tr.onclick = (e) => {
        if (e.target.matches('input')) return;
        this.editLayer = li; this.activeLayer = li; this.refreshPalette(); this.refreshCuts();
      };
      tr.ondblclick = () => {
        this.doc.sel.clear(); for (const s of this.doc.shapes) if (s.layer === li) this.doc.sel.add(s.id); this.selectionChanged();
      };
      for (const cb of tr.querySelectorAll('input')) cb.onchange = () => { this.doc.checkpoint(); L[cb.dataset.k] = cb.checked; this.afterEdit(); };
      tb.appendChild(tr);
    });
    $('#cutsHint').hidden = used.length > 0;
    this.renderLayerEditor();
  }
  renderLayerEditor() {
    const box = $('#layerEditor');
    if (this.editLayer == null) { box.innerHTML = ''; return; }
    const L = this.doc.layers[this.editLayer];
    const fill = L.mode !== 'Line', img = L.mode === 'Image';
    box.innerHTML = `
      <div class="cols">
        <div class="field"><label>Name</label><input data-f="name" value="${L.name.replace(/"/g, '&quot;')}"></div>
        <div class="field"><label>Mode</label><select data-f="mode">${['Line', 'Fill', 'Image'].map((m) => `<option ${m === L.mode ? 'selected' : ''}>${m}</option>`).join('')}</select></div>
        <div class="field"><label>Speed (mm/s)</label><input type="number" data-f="speed" min="0.1" step="1" value="${L.speed}"></div>
        <div class="field"><label>Power (%)</label><input type="number" data-f="power" min="0" max="100" step="1" value="${L.power}"></div>
        ${img && L.dither === 'grayscale' ? `<div class="field"><label>Min power (%)</label><input type="number" data-f="minPower" min="0" max="100" step="1" value="${L.minPower}"></div>` : ''}
        <div class="field"><label>Passes</label><input type="number" data-f="passes" min="1" step="1" value="${L.passes}"></div>
        ${fill ? `<div class="field"><label>Interval (mm)</label><input type="number" data-f="interval" min="0.01" step="0.01" value="${L.interval}"></div>
        <div class="field"><label>Overscan (%)</label><input type="number" data-f="overscan" min="0" step="0.5" value="${L.overscan}"></div>` : ''}
        ${img ? `<div class="field"><label>Picture style</label><select data-f="dither">${[['jarvis', 'Jarvis dither'], ['stucki', 'Stucki dither'], ['floyd', 'Floyd-Steinberg dither'], ['atkinson', 'Atkinson dither'], ['ordered', 'Ordered pattern'], ['threshold', 'Black & white'], ['grayscale', 'Grayscale (varies power)']].map(([v, t]) => `<option value="${v}" ${v === L.dither ? 'selected' : ''}>${t}</option>`).join('')}</select></div>
        <div class="field"><label>Resolution</label><span class="hint">${Math.round(25.4 / L.interval)} DPI</span></div>` : ''}
      </div>
      ${fill ? `<label class="check"><input type="checkbox" data-f="bidir" ${L.bidir ? 'checked' : ''}> Scan both directions</label>` : ''}
      ${L.mode === 'Fill' ? `<label class="check"><input type="checkbox" data-f="crossHatch" ${L.crossHatch ? 'checked' : ''}> Cross-hatch (second pass at 90°)</label>` : ''}
      ${img ? `<label class="check"><input type="checkbox" data-f="invert" ${L.invert ? 'checked' : ''}> Invert picture</label>` : ''}
      <label class="check"><input type="checkbox" data-f="air" ${L.air ? 'checked' : ''}> Air assist</label>
      <div class="row small"><button id="layerDefaults">Reset to defaults</button></div>`;
    for (const el of box.querySelectorAll('[data-f]')) {
      el.onchange = () => {
        this.doc.checkpoint();
        const k = el.dataset.f;
        let v = el.type === 'checkbox' ? el.checked : el.type === 'number' ? parseFloat(el.value) : el.value;
        if (el.type === 'number') { if (!isFinite(v)) v = L[k]; if (k === 'power' || k === 'minPower') v = Math.min(100, Math.max(0, v)); if (k === 'passes') v = Math.max(1, Math.round(v)); if (k === 'interval') v = Math.max(0.01, v); if (k === 'speed') v = Math.max(0.1, v); }
        L[k] = v;
        this.afterEdit();
      };
    }
    $('#layerDefaults').onclick = () => { this.doc.checkpoint(); const d = defaultLayer(L.index); Object.assign(L, { ...d, mode: L.mode, name: L.name }); this.afterEdit(); };
  }
  moveLayer(dir) {
    const o = this.doc.usedLayers(), i = o.indexOf(this.editLayer), j = i + dir;
    if (i < 0 || j < 0 || j >= o.length) return;
    this.doc.checkpoint();
    [o[i], o[j]] = [o[j], o[i]]; this.doc.order = o; this.afterEdit();
  }

  // ---------------------------------------------------------------- numeric bar
  bindNumeric() {
    const apply = (k) => {
      const b = this.doc.selBox(); if (!b) return;
      const v = parseFloat($('#n' + k).value); if (!isFinite(v)) return;
      this.doc.checkpoint();
      if (k === 'X') this.doc.transformSel(translate(v - b.x0, 0));
      if (k === 'Y') this.doc.transformSel(translate(0, this.dev.originTop ? (this.dev.bedH - v) - b.y1 : v - b.y0));
      if (k === 'W' || k === 'H') {
        if (v <= 0) return;
        let sx = k === 'W' ? v / (b.w || 1) : 1, sy = k === 'H' ? v / (b.h || 1) : 1;
        if (this.lockRatio) { if (k === 'W') sy = sx; else sx = sy; }
        this.doc.transformSel(scaleAbout(sx, sy, b.x0, this.dev.originTop ? b.y1 : b.y0));
      }
      if (k === 'R') {
        const cur = this.doc.selected().length === 1 ? matRotation(this.doc.selected()[0].tf) : 0;
        this.doc.transformSel(rotateAbout(v - cur, (b.x0 + b.x1) / 2, (b.y0 + b.y1) / 2));
      }
      this.afterEdit();
    };
    for (const k of ['X', 'Y', 'W', 'H', 'R']) $('#n' + k).addEventListener('change', () => apply(k));
    $('#nLock').onclick = () => { this.lockRatio = !this.lockRatio; $('#nLock').classList.toggle('on', this.lockRatio); $('#nLock').textContent = this.lockRatio ? '🔒' : '🔓'; };
  }
  refreshNumeric() {
    const b = this.doc.selBox();
    for (const k of ['X', 'Y', 'W', 'H', 'R']) $('#n' + k).disabled = !b;
    if (!b) { for (const k of ['X', 'Y', 'W', 'H', 'R']) $('#n' + k).value = ''; return; }
    const f = (v) => +v.toFixed(2);
    if (document.activeElement !== $('#nX')) $('#nX').value = f(b.x0);
    if (document.activeElement !== $('#nY')) $('#nY').value = f(this.dev.originTop ? this.dev.bedH - b.y1 : b.y0);
    if (document.activeElement !== $('#nW')) $('#nW').value = f(b.w);
    if (document.activeElement !== $('#nH')) $('#nH').value = f(b.h);
    const sel = this.doc.selected();
    if (document.activeElement !== $('#nR')) $('#nR').value = sel.length === 1 ? f(matRotation(sel[0].tf)) : 0;
  }
  refreshShapeOpts() {
    const box = $('#shapeOpts'); const sel = this.doc.selected();
    box.innerHTML = '';
    if (sel.length !== 1) {
      if (this.view.tool === 'polygon') { box.innerHTML = `<label>Sides <input id="optSides" type="number" min="3" max="64" value="${this.polySides}"></label>`; $('#optSides').onchange = (e) => (this.polySides = Math.max(3, Math.min(64, +e.target.value | 0))); }
      return;
    }
    const s = sel[0];
    if (s.type === 'rect') {
      box.innerHTML = `<label>Corner radius <input id="optR" type="number" min="0" step="0.5" value="${s.r || 0}"> mm</label>`;
      $('#optR').onchange = (e) => { this.doc.checkpoint(); s.r = Math.max(0, +e.target.value || 0); this.afterEdit(); };
    } else if (s.type === 'polygon') {
      box.innerHTML = `<label>Sides <input id="optSides" type="number" min="3" max="64" value="${s.sides}"></label>`;
      $('#optSides').onchange = (e) => { this.doc.checkpoint(); s.sides = Math.max(3, Math.min(64, +e.target.value | 0)); this.polySides = s.sides; this.afterEdit(); };
    } else if (s.type === 'text') {
      box.innerHTML = `<button id="optEditText">Edit text…</button>`;
      $('#optEditText').onclick = () => this.textDialog(s);
    } else if (s.type === 'image') {
      box.innerHTML = `<label class="check"><input id="optInv" type="checkbox" ${s.invert ? 'checked' : ''}> Invert this picture</label>`;
      $('#optInv').onchange = (e) => { this.doc.checkpoint(); s.invert = e.target.checked; this.afterEdit(); };
    }
  }

  // ---------------------------------------------------------------- menus & commands
  setToolButtons(t) { $$('.tool').forEach((b) => b.classList.toggle('active', b.dataset.tool === t)); }
  bindMenus() {
    for (const m of $$('.menu')) {
      m.addEventListener('mousedown', (e) => {
        if (e.target.closest('.dropdown')) return;
        const open = m.classList.contains('open');
        $$('.menu').forEach((x) => x.classList.remove('open'));
        if (!open) m.classList.add('open');
      });
      m.addEventListener('mouseenter', () => { if ($$('.menu.open').length && !m.classList.contains('open')) { $$('.menu').forEach((x) => x.classList.remove('open')); m.classList.add('open'); } });
    }
    document.addEventListener('mousedown', (e) => { if (!e.target.closest('.menu')) $$('.menu').forEach((x) => x.classList.remove('open')); });
    document.addEventListener('click', (e) => {
      const b = e.target.closest('[data-cmd]');
      if (!b) return;
      $$('.menu').forEach((x) => x.classList.remove('open'));
      this.cmd(b.dataset.cmd);
    });
    for (const b of $$('.tool')) b.onclick = () => { this.view.setTool(b.dataset.tool); this.setToolButtons(b.dataset.tool); this.refreshShapeOpts(); };
    $('#layerUp').onclick = () => this.moveLayer(-1);
    $('#layerDown').onclick = () => this.moveLayer(1);
    for (const t of $$('.tab')) t.onclick = () => { $$('.tab').forEach((x) => x.classList.toggle('active', x === t)); $$('.tabpage').forEach((p) => p.classList.toggle('active', p.id === t.dataset.tab)); };
    $('#fileOpen').onchange = async (e) => { const f = e.target.files[0]; e.target.value = ''; if (f) this.openFile(f); };
    $('#fileImport').onchange = async (e) => { const fs = [...e.target.files]; e.target.value = ''; for (const f of fs) await this.importFile(f); };
    const stage = $('#stage');
    stage.addEventListener('dragover', (e) => e.preventDefault());
    stage.addEventListener('drop', async (e) => { e.preventDefault(); for (const f of e.dataTransfer.files) { if (/\.(burner|lsproj|json)$/i.test(f.name)) await this.openFile(f); else await this.importFile(f); } });
    $('#pvSlider').oninput = () => { if (this.view.preview) { this.view.preview.frac = $('#pvSlider').value / 1000; this.view.draw(); } };
    $('#pvTravel').onchange = () => { if (this.view.preview) { this.view.preview.travel = $('#pvTravel').checked; this.view.draw(); } };
    window.addEventListener('beforeunload', (e) => { if (this.grbl.job) { e.preventDefault(); e.returnValue = ''; } });
  }

  cmd(c) {
    const doc = this.doc;
    const b = doc.selBox();
    const alignTo = (fn) => { if (doc.sel.size < 2) return this.toast('Select two or more shapes to align.'); doc.checkpoint(); for (const s of doc.selected()) { const sb = shapeBox(s); s.tf = mul(fn(sb), s.tf); } this.afterEdit(); };
    switch (c) {
      case 'new': if (doc.shapes.length && !confirm('Start a new project? Unsaved changes will be lost.')) return; doc.load({ shapes: [] }); this.docName = 'Untitled'; this.setDocName(); this.afterEdit(); this.selectionChanged(); break;
      case 'open': $('#fileOpen').click(); break;
      case 'save': this.download(this.docName.replace(/[^\w\- ]+/g, '_') + '.burner', JSON.stringify(doc.toJSON()), 'application/json'); doc.dirty = false; this.toast('Project saved to your downloads.'); break;
      case 'import': $('#fileImport').click(); break;
      case 'exportGcode': { const j = this.buildJob(); if (!j) return; this.download(this.docName.replace(/[^\w\- ]+/g, '_') + '.gc', j.text, 'text/plain'); this.toast(`G-code saved (${j.lines.length} lines, about ${fmtTime(j.time)}).`); break; }
      case 'exportSvg': this.exportSvg(); break;
      case 'sample': this.loadSample(); break;
      case 'undo': doc.undo(); this.selectionChanged(); break;
      case 'redo': doc.redo(); this.selectionChanged(); break;
      case 'copy': this.clip = doc.selected().map(cloneShape); if (this.clip.length) this.toast(`Copied ${this.clip.length}`); break;
      case 'cut': this.clip = doc.selected().map(cloneShape); if (!this.clip.length) return; doc.checkpoint(); doc.deleteSel(); this.afterEdit(); this.selectionChanged(); break;
      case 'paste': this.pasteShapes(this.clip, 10); break;
      case 'duplicate': this.pasteShapes(doc.selected().map(cloneShape), 0); break;
      case 'delete': if (!doc.sel.size) return; doc.checkpoint(); doc.deleteSel(); this.afterEdit(); this.selectionChanged(); break;
      case 'selectAll': doc.sel = new Set(doc.shapes.filter((s) => doc.layers[s.layer].show).map((s) => s.id)); this.selectionChanged(); break;
      case 'alignL': alignTo((sb) => translate(b.x0 - sb.x0, 0)); break;
      case 'alignR': alignTo((sb) => translate(b.x1 - sb.x1, 0)); break;
      case 'alignCH': alignTo((sb) => translate((b.x0 + b.x1) / 2 - (sb.x0 + sb.x1) / 2, 0)); break;
      case 'alignT': alignTo((sb) => translate(0, b.y1 - sb.y1)); break;
      case 'alignB': alignTo((sb) => translate(0, b.y0 - sb.y0)); break;
      case 'alignCV': alignTo((sb) => translate(0, (b.y0 + b.y1) / 2 - (sb.y0 + sb.y1) / 2)); break;
      case 'mirrorH': if (!b) return; doc.checkpoint(); doc.transformSel(scaleAbout(-1, 1, (b.x0 + b.x1) / 2, 0)); this.afterEdit(); break;
      case 'mirrorV': if (!b) return; doc.checkpoint(); doc.transformSel(scaleAbout(1, -1, 0, (b.y0 + b.y1) / 2)); this.afterEdit(); break;
      case 'rot90': case 'rot-90': if (!b) return; doc.checkpoint(); doc.transformSel(rotateAbout(c === 'rot90' ? -90 : 90, (b.x0 + b.x1) / 2, (b.y0 + b.y1) / 2)); this.afterEdit(); break;
      case 'centerBed': if (!b) return; doc.checkpoint(); doc.transformSel(translate(this.dev.bedW / 2 - (b.x0 + b.x1) / 2, this.dev.bedH / 2 - (b.y0 + b.y1) / 2)); this.afterEdit(); break;
      case 'toPath': {
        const sel = doc.selected().filter((s) => !['path', 'image'].includes(s.type));
        if (!sel.length) return; doc.checkpoint();
        for (const s of sel) { s.subpaths = localContours(s).map((p) => ({ closed: p.closed, pts: p.pts.map((q) => q.slice()) })); s.type = 'path'; }
        this.afterEdit(); this.selectionChanged(); break;
      }
      case 'zoomFit': this.view.zoomFit(); this.updateStatus(); break;
      case 'zoomSel': { const bb = b || doc.allBox(); if (bb) this.view.zoomTo(bb, 60); this.updateStatus(); break; }
      case 'preview': this.openPreview(); break;
      case 'closePreview': this.closePreview(); break;
      case 'settings': this.settingsDialog(); break;
      case 'help': this.showHelp(); break;
      case 'keys': this.showKeys(); break;
      case 'connectSerial': this.connect('serial'); break;
      case 'connectSim': this.connect('sim'); break;
      case 'disconnect': this.grbl.disconnect(); break;
      case 'start': this.startJob(); break;
      case 'pause': if (!this.grbl.job) { this.grbl.realtime(0x21); break; } if (this.grbl.job.paused) this.grbl.resume(); else this.grbl.pause(); break;
      case 'stop': this.grbl.stop(); break;
      case 'home': if (this.need()) this.grbl.send('$H'); break;
      case 'unlock': if (this.need()) this.grbl.send('$X'); break;
      case 'frame': this.frame(); break;
      case 'setOrigin': if (this.need()) { this.grbl.send('G10 L20 P1 X0 Y0'); this.toast('Origin set at the current head position.'); } break;
      case 'gotoOrigin': if (this.need()) { this.grbl.send('G90'); this.grbl.send(`G0 X0 Y0`); } break;
      case 'gotoSel': { if (!this.need()) return; const bb = b || doc.allBox(); if (!bb) return; const [ux, uy] = this.toUser([(bb.x0 + bb.x1) / 2, (bb.y0 + bb.y1) / 2]); this.grbl.send('G90'); this.grbl.send(`G0 X${ux.toFixed(3)} Y${uy.toFixed(3)}`); break; }
      case 'pulse': this.pulse(); break;
    }
    this.updateStatus();
  }
  pasteShapes(list, offset) {
    if (!list || !list.length) return;
    this.doc.checkpoint(); this.doc.sel.clear();
    for (const s0 of list) { const s = cloneShape(s0); s.id = newId(); s.tf = mul(translate(offset, -offset), s.tf); this.doc.add(s); this.doc.sel.add(s.id); }
    if (offset) this.clip = this.clip.map((s) => ({ ...cloneShape(s), tf: mul(translate(offset, -offset), s.tf) }));
    this.afterEdit(); this.selectionChanged();
  }
  setDocName() { $('#docname').textContent = this.docName; document.title = `${this.docName} - Burner`; }

  // ---------------------------------------------------------------- keyboard
  bindKeys() {
    window.addEventListener('keydown', (e) => {
      const inField = /INPUT|TEXTAREA|SELECT/.test(document.activeElement.tagName) || document.querySelector('dialog[open]');
      if (inField) return;
      const k = e.key.toLowerCase(), ctrl = e.ctrlKey || e.metaKey;
      const map = { z: e.shiftKey ? 'redo' : 'undo', y: 'redo', c: 'copy', x: 'cut', v: 'paste', d: 'duplicate', a: 'selectAll', s: 'save', o: 'open', n: 'new', i: 'import' };
      if (ctrl && map[k]) { e.preventDefault(); this.cmd(map[k]); return; }
      if (e.altKey && k === 'p') { e.preventDefault(); this.cmd('preview'); return; }
      if (k === 'delete' || k === 'backspace') { e.preventDefault(); this.cmd('delete'); return; }
      if (k === 'escape') {
        if (this.view.preview) { this.closePreview(); return; }
        if (this.view.linePts) { this.view.linePts = null; this.view.draw(); return; }
        this.doc.sel.clear(); this.view.setTool('select'); this.setToolButtons('select'); this.selectionChanged(); return;
      }
      if (k === 'enter' && this.view.linePts) { this.view.finishLine(false); return; }
      if (k.startsWith('arrow')) {
        e.preventDefault();
        const d = e.shiftKey ? 10 : ctrl ? 0.1 : 1;
        const dx = k === 'arrowleft' ? -1 : k === 'arrowright' ? 1 : 0, dy = k === 'arrowdown' ? -1 : k === 'arrowup' ? 1 : 0;
        if (this.doc.sel.size) { this.doc.checkpoint(); this.doc.transformSel(translate(dx * d, dy * d)); this.afterEdit(); }
        else if ($('#moveTab').classList.contains('active') && this.grbl.connected) this.jog(dx, dy);
        return;
      }
      if (ctrl || e.altKey) return;
      const tools = { v: 'select', l: 'line', r: 'rect', e: 'ellipse', p: 'polygon', t: 'text', m: 'measure', h: 'pan' };
      if (tools[k]) { this.view.setTool(tools[k]); this.setToolButtons(tools[k]); this.refreshShapeOpts(); }
    });
  }

  // ---------------------------------------------------------------- files
  async openFile(f) {
    try {
      const o = JSON.parse(await f.text());
      await this.loadProject(o, f.name.replace(/\.(burner|lsproj|json)$/i, ''));
      this.toast(`Opened ${f.name}`);
    } catch (e) { this.toast('Could not open that file: ' + e.message, true); }
  }
  async loadProject(o, name) {
    this.doc.load(o);
    for (const s of this.doc.shapes) if (s.type === 'image') await this.cacheImage(s.src);
    this.docName = name || 'Untitled'; this.setDocName();
    this.editLayer = null; this.afterEdit(); this.selectionChanged();
    this.doc.dirty = false;
  }
  async cacheImage(src) {
    if (this.imgCache.has(src)) return this.imgCache.get(src);
    const img = await loadImage(src);
    const g = imageGrayData(img);
    const e = { img, gray: g.gray, w: g.w, h: g.h };
    this.imgCache.set(src, e);
    return e;
  }
  async importFile(f) {
    try {
      if (/\.svg$/i.test(f.name) || f.type === 'image/svg+xml') {
        const items = importSVG(await f.text());
        if (!items.length) { this.toast('No shapes found in that drawing.', true); return; }
        this.doc.checkpoint(); this.doc.sel.clear();
        let box = null; const added = [];
        for (const it of items) {
          const s = this.doc.add({ type: 'path', subpaths: it.subpaths, layer: it.color ? nearestPaletteIndex(it.color) : this.activeLayer });
          added.push(s);
        }
        for (const s of added) { const sb = shapeBox(s); if (sb) box = box ? { x0: Math.min(box.x0, sb.x0), y0: Math.min(box.y0, sb.y0), x1: Math.max(box.x1, sb.x1), y1: Math.max(box.y1, sb.y1) } : { ...sb }; this.doc.sel.add(s.id); }
        // keep the drawing on the bed
        if (box && (box.x0 < 0 || box.y0 < 0 || box.x1 > this.dev.bedW || box.y1 > this.dev.bedH)) {
          const m = translate(Math.max(0, Math.min(this.dev.bedW - (box.x1 - box.x0), 10)) - box.x0, Math.max(0, Math.min(this.dev.bedH - (box.y1 - box.y0), 10)) - box.y0);
          for (const s of added) s.tf = mul(m, s.tf);
        }
        this.afterEdit(); this.selectionChanged();
        this.toast(`Imported ${items.length} shape${items.length > 1 ? 's' : ''} from ${f.name}`);
      } else if (/^image\//.test(f.type)) {
        const src = await new Promise((res, rej) => { const r = new FileReader(); r.onload = () => res(r.result); r.onerror = rej; r.readAsDataURL(f); });
        const e = await this.cacheImage(src);
        const dpi = 254; // pictures come in at 254 DPI (0.1 mm per pixel) and can be resized
        let w = (e.w / dpi) * 25.4, h = (e.h / dpi) * 25.4;
        const maxW = this.dev.bedW * 0.8, maxH = this.dev.bedH * 0.8;
        const k = Math.min(1, maxW / w, maxH / h); w *= k; h *= k;
        // pick a free layer for the picture so vector layers keep their settings
        const used = new Set(this.doc.shapes.map((s) => s.layer));
        let li = this.doc.layers.findIndex((L, i) => !used.has(i) && i > 0);
        if (li < 0) li = this.activeLayer;
        this.doc.layers[li].mode = 'Image';
        if (this.doc.layers[li].speed === 100) this.doc.layers[li].speed = 150;
        this.addShape({ type: 'image', src, w, h, layer: li, tf: translate(10, 10) });
        this.toast(`Picture placed on layer C${String(li).padStart(2, '0')} (Image mode).`);
      } else this.toast(`Can't import ${f.name}: use SVG, PNG, JPG, GIF, WEBP or BMP.`, true);
    } catch (e) { this.toast('Import failed: ' + e.message, true); }
  }
  exportSvg() {
    const box = this.doc.allBox();
    const W = this.dev.bedW, H = this.dev.bedH;
    let body = '';
    for (const s of this.doc.shapes) {
      if (s.type === 'image') {
        const m = mul([1, 0, 0, -1, 0, H], s.tf);
        body += `<image href="${s.src}" width="${s.w}" height="${s.h}" preserveAspectRatio="none" transform="matrix(${[m[0], m[1], m[2], m[3], m[4], m[5]].map((v) => +v.toFixed(4)).join(' ')}) translate(0 ${s.h}) scale(1 -1)"/>\n`;
        continue;
      }
      const d = worldContours(s).map((c) => 'M' + c.pts.map(([x, y]) => `${+x.toFixed(3)},${+(H - y).toFixed(3)}`).join('L') + (c.closed ? 'Z' : '')).join('');
      body += `<path d="${d}" fill="none" stroke="${PALETTE[s.layer]}" stroke-width="0.1"/>\n`;
    }
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}mm" height="${H}mm" viewBox="0 0 ${W} ${H}">\n${body}</svg>\n`;
    if (!box) return this.toast('Nothing to export yet.');
    this.download(this.docName.replace(/[^\w\- ]+/g, '_') + '.svg', svg, 'image/svg+xml');
  }

  loadSample() {
    const doc = this.doc;
    doc.load({ shapes: [] });
    const L = doc.layers;
    Object.assign(L[2], { name: 'Outline', mode: 'Line', speed: 10, power: 80, passes: 2 });
    Object.assign(L[1], { name: 'Lettering', mode: 'Fill', speed: 150, power: 35, interval: 0.1 });
    Object.assign(L[5], { name: 'Details', mode: 'Line', speed: 40, power: 30 });
    const cx = this.dev.bedW / 2, cy = this.dev.bedH / 2;
    doc.add({ type: 'rect', w: 180, h: 70, r: 8, layer: 2, tf: translate(cx - 90, cy - 35) });
    doc.add({ type: 'ellipse', w: 6, h: 6, layer: 2, tf: translate(cx - 82, cy - 3) });
    doc.add({ type: 'ellipse', w: 6, h: 6, layer: 2, tf: translate(cx + 76, cy - 3) });
    const spec = { text: 'MAKE IT', font: 'Impact, sans-serif', size: 22, align: 'left', bold: true, italic: false };
    try {
      const o = textOutlines(spec);
      doc.add({ type: 'text', ...spec, subpaths: o.subpaths, w: o.w, h: o.h, layer: 1, tf: translate(cx - o.w / 2 + 14, cy - o.h / 2 + 6) });
    } catch (e) { /* no canvas text */ }
    doc.add({ type: 'polygon', w: 30, h: 30, sides: 6, layer: 5, tf: translate(cx - 70, cy - 15) });
    doc.add({ type: 'path', subpaths: [{ closed: false, pts: [[cx - 50, cy - 24], [cx + 70, cy - 24]] }], layer: 5 });
    doc.order = [5, 1, 2];
    this.docName = 'Sample nameplate'; this.setDocName();
    this.editLayer = 2; this.activeLayer = 2;
    this.refreshPalette(); this.afterEdit(); this.selectionChanged();
  }

  // ---------------------------------------------------------------- settings & help
  settingsDialog() {
    const dlg = $('#dlgSettings'), f = dlg.querySelector('form');
    for (const el of f.elements) { if (!el.name) continue; if (el.type === 'checkbox') el.checked = !!this.dev[el.name]; else el.value = this.dev[el.name]; }
    dlg.returnValue = '';
    dlg.onclose = () => {
      if (dlg.returnValue !== 'ok') return;
      const old = { ...this.dev };
      for (const el of f.elements) {
        if (!el.name) continue;
        if (el.type === 'checkbox') this.dev[el.name] = el.checked;
        else if (el.type === 'number' || el.name === 'baud') { const v = parseFloat(el.value); if (isFinite(v)) this.dev[el.name] = v; }
        else this.dev[el.name] = el.value;
      }
      this.dev.bufferSize = Math.max(32, Math.round(this.dev.bufferSize));
      this.saveDevice();
      if (old.bedW !== this.dev.bedW || old.bedH !== this.dev.bedH) this.view.zoomFit();
      this.refreshNumeric();
      this.view.draw(); this.toast('Device settings saved.');
    };
    dlg.showModal();
  }
  showHelp() {
    this.info('Getting started', `
      <ol>
        <li><b>Design</b>: draw with the tools on the left, or import an SVG drawing or a picture (File → Import, or drag a file onto the bed).</li>
        <li><b>Layers</b>: click a color in the bar at the bottom to put the selected shapes on that layer. Each layer has its own speed, power, passes and mode (<i>Line</i> follows outlines, <i>Fill</i> scans the inside, <i>Image</i> engraves pictures).</li>
        <li><b>Check</b>: press <i>Preview</i> to see every move the laser will make and how long it will take.</li>
        <li><b>Connect</b>: plug in a GRBL laser over USB and press <i>Connect USB</i> (needs Chrome or Edge). Try <i>Practice machine</i> first to see how it works without a laser.</li>
        <li><b>Burn</b>: <i>Home</i>, then <i>Frame</i> to trace the job outline, then <i>Start</i>. <i>Pause</i> holds the head; <i>Stop</i> halts and turns the laser off.</li>
      </ol>
      <p class="hint">Always stay with a running laser and wear the right eye protection.</p>`);
  }
  showKeys() {
    this.info('Keyboard shortcuts', `<table>
      <tr><td>V / L / R / E / P / T / M</td><td>Select, Lines, Rectangle, Ellipse, Polygon, Text, Measure</td></tr>
      <tr><td>Space + drag, middle mouse</td><td>Pan</td></tr><tr><td>Mouse wheel</td><td>Zoom</td></tr>
      <tr><td>Shift while drawing</td><td>Square / circle / 15° angles</td></tr>
      <tr><td>Shift on a corner handle</td><td>Stretch freely</td></tr>
      <tr><td>Arrow keys</td><td>Nudge 1 mm (Shift 10 mm, Ctrl 0.1 mm)</td></tr>
      <tr><td>Ctrl+Z / Ctrl+Y</td><td>Undo / Redo</td></tr><tr><td>Ctrl+C / V / X / D</td><td>Copy, Paste, Cut, Duplicate</td></tr>
      <tr><td>Delete</td><td>Delete selection</td></tr><tr><td>Alt+P</td><td>Preview</td></tr>
      <tr><td>Enter / double-click / right-click</td><td>Finish a line</td></tr><tr><td>Esc</td><td>Cancel, deselect, close preview</td></tr></table>`);
  }

  // ---------------------------------------------------------------- job building
  jobBox() {
    let b = null;
    const selOnly = $('#selOnly').checked;
    for (const s of this.doc.shapes) {
      if (!this.doc.layers[s.layer].output) continue;
      if (selOnly && !this.doc.sel.has(s.id)) continue;
      const sb = shapeBox(s); if (!sb) continue;
      b = b ? { x0: Math.min(b.x0, sb.x0), y0: Math.min(b.y0, sb.y0), x1: Math.max(b.x1, sb.x1), y1: Math.max(b.y1, sb.y1) } : { ...sb };
    }
    if (b) { b.w = b.x1 - b.x0; b.h = b.y1 - b.y0; }
    return b;
  }
  anchorPoint(b) {
    const a = this.anchor;
    const x = a[1] === 'l' ? b.x0 : a[1] === 'r' ? b.x1 : (b.x0 + b.x1) / 2;
    const y = a[0] === 'b' ? b.y0 : a[0] === 't' ? b.y1 : (b.y0 + b.y1) / 2;
    return [x, y];
  }
  buildJob() {
    const startFrom = $('#startFrom').value;
    const box = this.jobBox();
    if (!box) { this.toast('Nothing to burn: draw or import something first (and check its layer Output is on).', true); return null; }
    const origin = startFrom === 'absolute' ? [0, 0] : this.anchorPoint(box);
    try {
      const home = this.fromUser([0, 0]);
      const plan = planJob(this.doc, this.dev, this.imgCache, { selectedOnly: $('#selOnly').checked, innerFirst: $('#innerFirst').checked, startPos: startFrom === 'absolute' ? home : origin });
      const job = emitGcode(plan, this.dev, { startFrom, origin, startPos: startFrom === 'absolute' ? home : origin });
      for (const w of plan.warnings) this.toast(w, true);
      if (startFrom === 'absolute' && (box.x0 < -0.01 || box.y0 < -0.01 || box.x1 > this.dev.bedW + 0.01 || box.y1 > this.dev.bedH + 0.01)) this.toast('Warning: part of the job is outside the bed.', true);
      job.origin = origin; job.box = box; job.startFrom = startFrom;
      return job;
    } catch (e) { this.toast(e.message, true); return null; }
  }
  openPreview() {
    const j = this.buildJob(); if (!j) return;
    this.view.preview = { segs: j.preview, frac: 1, travel: $('#pvTravel').checked };
    if (j.startFrom !== 'absolute') {
      // show relative jobs where they would land if the head were at the origin point
      this.view.preview.segs = j.preview;
    }
    $('#pvSlider').value = 1000;
    $('#pvInfo').textContent = `Estimated time ${fmtTime(j.time)} · cut ${(j.cutLen / 1000).toFixed(2)} m · travel ${(j.travel / 1000).toFixed(2)} m · ${j.lines.length} lines`;
    $('#previewBar').hidden = false;
    this.view.draw();
  }
  closePreview() { this.view.preview = null; $('#previewBar').hidden = true; this.view.draw(); }

  // ---------------------------------------------------------------- machine
  need() {
    if (this.grbl.connected) return true;
    this.toast('Connect to a laser first (or use the practice machine).', true);
    return false;
  }
  async connect(kind) {
    if (this.grbl.connected) await this.grbl.disconnect();
    if (kind === 'serial' && !('serial' in navigator)) {
      this.info('USB connection not available', '<p>This browser can\'t talk to USB devices. Please open this page in <b>Chrome</b> or <b>Edge</b> on a computer (it must be served from <i>localhost</i> or <i>https</i>).</p><p>You can still design, preview and save G-code, or try the <b>practice machine</b>.</p>');
      return;
    }
    try {
      await this.grbl.connect(kind, this.dev);
      if (this.dev.autoHome) setTimeout(() => this.grbl.send('$H'), 1600);
    } catch (e) { if (e.name !== 'NotFoundError') this.toast('Could not connect: ' + e.message, true); }
  }
  jog(dx, dy) {
    if (!this.need()) return;
    const d = Math.max(0.01, +$('#jogDist').value || 10), sp = Math.max(1, +$('#jogSpeed').value || 50);
    // arrows are screen directions; with the origin at the top, "up" is toward smaller Y
    this.grbl.jog(dx * d, (this.dev.originTop ? -dy : dy) * d, sp * 60);
  }
  frame() {
    if (!this.need()) return;
    const box = this.jobBox(); if (!box) return this.toast('Nothing to frame yet.', true);
    const startFrom = $('#startFrom').value;
    const sp = Math.max(1, +$('#jogSpeed').value || 50) * 60;
    const lines = ['G21'];
    if (startFrom === 'current') {
      const [ax, ay] = this.anchorPoint(box);
      const c = [[box.x0, box.y0], [box.x1, box.y0], [box.x1, box.y1], [box.x0, box.y1], [box.x0, box.y0], [ax, ay]];
      let px = ax, py = ay; lines.push('G91');
      const ys = this.dev.originTop ? -1 : 1;
      for (const [x, y] of c) { lines.push(`G0 X${(x - px).toFixed(3)} Y${(ys * (y - py)).toFixed(3)} F${sp}`); px = x; py = y; }
      lines.push('G90');
    } else {
      const o = startFrom === 'user' ? this.toUser(this.anchorPoint(box)) : [0, 0];
      lines.push('G90');
      for (const p of [[box.x0, box.y0], [box.x1, box.y0], [box.x1, box.y1], [box.x0, box.y1], [box.x0, box.y0]]) { const [x, y] = this.toUser(p); lines.push(`G0 X${(x - o[0]).toFixed(3)} Y${(y - o[1]).toFixed(3)}`); }
    }
    for (const l of lines) this.grbl.send(l);
    this.view.frameBox = box; this.view.draw();
    clearTimeout(this.frameT); this.frameT = setTimeout(() => { this.view.frameBox = null; this.view.draw(); }, 4000);
  }
  pulse() {
    if (!this.need()) return;
    if (!confirm('Fire the laser for a short moment at 1% power? Make sure the area is safe and you are wearing eye protection.')) return;
    const s = Math.max(1, Math.round(this.dev.sMax * 0.01));
    for (const l of ['M3', `G1 F100 S${s}`, 'G4 P0.2', 'G1 S0', 'M5']) this.grbl.send(l);
  }
  startJob() {
    if (!this.grbl.connected) { this.need(); return; }
    if (this.grbl.job) return this.toast('A job is already running.');
    if (!/^Idle/.test(this.grbl.status.state)) return this.toast(`The machine is "${this.grbl.status.state}". It must be Idle to start (try Unlock or Home).`, true);
    const j = this.buildJob(); if (!j) return;
    this.closePreview();
    this.grbl.startJob(j.text, this.docName);
    this.lastJob = j;
  }
  bindLaser() {
    $('#btnConnect').onclick = () => this.connect('serial');
    $('#btnSim').onclick = () => this.connect('sim');
    $('#btnDisconnect').onclick = () => this.grbl.disconnect();
    for (const b of $$('[data-jog]')) b.onclick = () => { const [dx, dy] = b.dataset.jog.split(',').map(Number); this.jog(dx, dy); };
    const anchor = $('#anchor');
    for (const r of ['t', 'm', 'b']) for (const c of ['l', 'c', 'r']) {
      const k = r + c, b = document.createElement('button');
      b.title = `Job origin: ${{ t: 'top', m: 'middle', b: 'bottom' }[r]} ${{ l: 'left', c: 'center', r: 'right' }[c]}`;
      b.className = k === this.anchor ? 'on' : '';
      b.onclick = () => { this.anchor = k; $$('#anchor button').forEach((x) => x.classList.remove('on')); b.classList.add('on'); };
      anchor.appendChild(b);
    }
    const sf = () => $('#anchor').classList.toggle('disabled', $('#startFrom').value === 'absolute');
    $('#startFrom').onchange = sf; sf();
    this.grbl.on('status', (st) => {
      const s = st.state || '';
      const dot = $('#stateDot');
      dot.className = 'dot ' + (s === 'Disconnected' ? 'off' : /^Idle/.test(s) ? 'ok' : /^(Run|Jog|Home|Connecting)/.test(s) ? 'busy' : 'bad');
      $('#stateText').textContent = s === 'Disconnected' ? 'Not connected' : `${this.grbl.kind === 'sim' ? 'Practice machine' : 'Laser'}: ${s}`;
      const on = this.grbl.connected;
      $('#btnConnect').hidden = on; $('#btnSim').hidden = on; $('#btnDisconnect').hidden = !on;
      if (on && st.wpos) { $('#posText').textContent = `X ${st.wpos[0].toFixed(2)}  Y ${st.wpos[1].toFixed(2)}`; this.view.head = this.fromUser([st.wpos[0], st.wpos[1]]); }
      else { $('#posText').textContent = 'X — Y —'; this.view.head = null; }
      this.view.draw();
    });
    this.grbl.on('job', (j, done) => {
      $('#jobBox').hidden = !j && !done;
      $('#btnPause').textContent = j?.paused ? 'Resume' : 'Pause';
      if (j) {
        const f = j.total ? j.acked / j.total : 0;
        $('#jobBar').style.width = (f * 100).toFixed(1) + '%';
        const el = (Date.now() - j.started) / 1000;
        const est = this.lastJob ? this.lastJob.time : 0;
        $('#jobText').textContent = `${j.paused ? 'Paused · ' : ''}${Math.round(f * 100)}% · ${j.acked}/${j.total} lines · ${fmtTime(el)} elapsed${est ? ' of about ' + fmtTime(est) : ''}`;
      } else if (done) {
        $('#jobBar').style.width = done.ok ? '100%' : $('#jobBar').style.width;
        $('#jobText').textContent = done.ok ? `Finished in ${fmtTime(done.secs)}${done.errors.length ? ` · ${done.errors.length} error(s), see Console` : ''}` : 'Job stopped.';
        this.toast(done.ok ? 'Job finished.' : 'Job stopped.', !done.ok);
      }
    });
  }
  bindConsole() {
    const con = $('#console');
    this.grbl.on('log', (text, kind) => {
      const d = document.createElement('div'); d.className = kind; d.textContent = (kind === 'out' ? '> ' : '') + text;
      con.appendChild(d);
      while (con.childNodes.length > 1500) con.firstChild.remove();
      con.scrollTop = con.scrollHeight;
      if (kind === 'err') $('#sbMsg').textContent = text;
    });
    const send = () => {
      const v = $('#consoleIn').value.trim(); if (!v) return;
      if (!this.need()) return;
      if (v === '?' || v === '!' || v === '~') this.grbl.realtime(v.charCodeAt(0)); else this.grbl.send(v);
      this.hist = [...(this.hist || []).filter((h) => h !== v), v]; this.histI = this.hist.length;
      $('#consoleIn').value = '';
    };
    $('#consoleSend').onclick = send;
    $('#consoleIn').addEventListener('keydown', (e) => {
      if (e.key === 'Enter') send();
      if (e.key === 'ArrowUp' && this.hist?.length) { this.histI = Math.max(0, this.histI - 1); $('#consoleIn').value = this.hist[this.histI]; }
      if (e.key === 'ArrowDown' && this.hist?.length) { this.histI = Math.min(this.hist.length, this.histI + 1); $('#consoleIn').value = this.hist[this.histI] || ''; }
    });
    for (const b of $$('[data-send]')) b.onclick = () => { if (this.need()) this.grbl.send(b.dataset.send); };
    $('#consoleClear').onclick = () => (con.innerHTML = '');
  }
}

window.app = new App();
export { IDENT };
