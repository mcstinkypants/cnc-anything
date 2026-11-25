// Talking to a GRBL laser controller: USB serial in the browser (Web Serial),
// or a built-in practice machine that behaves like one.

export const GRBL_ERRORS = {
  1: 'A command letter was expected but not found.', 2: 'A number is missing or badly formatted.', 3: "That '$' command isn't supported.",
  4: 'A negative value was given where a positive one is needed.', 5: 'Homing is turned off in the controller settings.', 6: 'Step pulse time must be at least 3 microseconds.',
  7: 'Controller settings could not be read; defaults were restored.', 8: "'$' commands only work while the machine is idle.", 9: 'Commands are locked while in alarm or door state.',
  10: 'Soft limits need homing to be turned on.', 11: 'The line is too long for the controller.', 12: 'Step rate is above what the controller can do.',
  13: 'The safety door was opened.', 14: 'Startup line or build info is too long.', 15: 'The jog target is beyond the machine travel.',
  16: "The jog command has no '=' or contains a command that isn't allowed.", 17: 'Laser mode needs PWM output.', 20: 'Unsupported or unknown G-code command.',
  21: 'More than one command from the same group on one line.', 22: 'Feed rate has not been set.', 23: 'A command needs a whole number.',
  24: 'Two commands on one line both need axis words.', 25: 'A word was repeated on the line.', 26: 'A motion command is missing its axis words.',
  27: 'Line number is out of range.', 28: 'A required value is missing.', 29: 'Only G54-G59 work coordinate systems are supported.',
  30: 'G53 only works with G0 or G1.', 31: 'Axis words given where nothing uses them.', 32: 'An arc has no axis words in its plane.',
  33: 'The motion target is not valid.', 34: 'Arc radius is invalid.', 35: 'An arc needs offset words in its plane.', 36: 'Unused value words were found.',
  37: 'Tool length offset was not applied to the right axis.', 38: 'Tool number is above the maximum.',
};
export const GRBL_ALARMS = {
  1: 'Hard limit switch triggered. Re-home the machine.', 2: 'The move would go outside the machine travel (soft limit).', 3: 'Reset while moving: position may be lost. Re-home.',
  4: 'Probe was not in the expected state.', 5: 'Probe did not make contact.', 6: 'Homing was reset.', 7: 'Safety door opened during homing.',
  8: 'Homing could not clear the limit switch.', 9: 'Homing could not find a limit switch.', 10: 'Homing: second switch did not trigger.',
};

// ---- transports -------------------------------------------------------------
class SerialTransport {
  constructor(baud) { this.baud = baud; this.port = null; this.reader = null; this.onData = null; this.onClose = null; }
  static available() { return 'serial' in navigator; }
  async open() {
    this.port = await navigator.serial.requestPort();
    await this.port.open({ baudRate: this.baud, bufferSize: 4096 });
    this.writer = this.port.writable.getWriter();
    this.alive = true;
    this.readLoop();
  }
  async readLoop() {
    const dec = new TextDecoder();
    while (this.alive && this.port.readable) {
      this.reader = this.port.readable.getReader();
      try {
        for (;;) { const { value, done } = await this.reader.read(); if (done) break; if (value && this.onData) this.onData(dec.decode(value)); }
      } catch (e) { /* port lost */ } finally { this.reader.releaseLock(); }
      if (!this.alive) break;
      break;
    }
    if (this.alive) { this.alive = false; this.onClose && this.onClose(); }
  }
  write(str) { return this.writer.write(new TextEncoder().encode(str)); }
  writeByte(b) { return this.writer.write(new Uint8Array([b])); }
  async close() {
    this.alive = false;
    try { await this.reader?.cancel(); } catch (e) {}
    try { this.writer?.releaseLock(); } catch (e) {}
    try { await this.port?.close(); } catch (e) {}
  }
}

// A pretend GRBL 1.1 controller: moves at 20x speed, answers like the real thing.
class SimTransport {
  constructor(dev) { this.dev = dev; this.onData = null; this.onClose = null; this.q = []; this.pos = [0, 0]; this.wco = [0, 0]; this.state = 'Idle'; this.hold = false; this.busy = false; this.feed = 0; this.spindle = 0; this.abs = true; this.laser = 'M5'; }
  async open() { setTimeout(() => this.say('\r\nGrbl 1.1h [\'$\' for help]\r\n'), 150); }
  say(s) { this.onData && this.onData(s); }
  status() {
    const m = this.pos, w = [m[0] - this.wco[0], m[1] - this.wco[1]];
    return `<${this.state}|MPos:${m[0].toFixed(3)},${m[1].toFixed(3)},0.000|FS:${Math.round(this.feed)},${Math.round(this.spindle)}|WCO:${this.wco[0].toFixed(3)},${this.wco[1].toFixed(3)},0.000>\r\n`;
  }
  writeByte(b) {
    if (b === 0x3f) this.say(this.status());
    else if (b === 0x21) { this.hold = true; if (this.state === 'Run') this.state = 'Hold:0'; }
    else if (b === 0x7e) { this.hold = false; if (this.state.startsWith('Hold')) this.state = this.busy ? 'Run' : 'Idle'; this.pump(); }
    else if (b === 0x18) { const moving = this.busy; this.q = []; this.busy = false; clearTimeout(this.timer); this.hold = false; this.state = moving ? 'Alarm' : 'Idle'; this.say('\r\nGrbl 1.1h [\'$\' for help]\r\n'); if (moving) this.say('ALARM:3\r\n'); }
    else if (b === 0x85) { if (this.state === 'Jog') { this.q = this.q.filter((l) => !l.startsWith('$J=')); } }
    return Promise.resolve();
  }
  write(str) {
    for (const raw of str.split('\n')) {
      const l = raw.trim();
      if (!l) continue;
      if (l === '?') { this.writeByte(0x3f); continue; }
      this.q.push(l);
    }
    this.pump();
    return Promise.resolve();
  }
  pump() {
    if (this.busy || this.hold || !this.q.length) { if (!this.busy && !this.q.length && (this.state === 'Run' || this.state === 'Jog')) this.state = 'Idle'; return; }
    const line = this.q.shift().toUpperCase();
    if (this.state === 'Alarm' && !/^\$X|^\$H|^\$\$|^\$I/.test(line)) { this.say('error:9\r\n'); this.pump(); return; }
    if (line === '$X') { this.state = 'Idle'; this.say('[MSG:Caution: Unlocked]\r\nok\r\n'); this.pump(); return; }
    if (line === '$H') { this.state = 'Home'; this.busy = true; this.timer = setTimeout(() => { this.pos = [0, 0]; this.state = 'Idle'; this.busy = false; this.say('ok\r\n'); this.pump(); }, 800); return; }
    if (line === '$$') { this.say(`$30=${this.dev.sMax}\r\n$31=0\r\n$32=1\r\n$110=6000.000\r\n$111=6000.000\r\n$130=${this.dev.bedW}.000\r\n$131=${this.dev.bedH}.000\r\nok\r\n`); this.pump(); return; }
    if (line === '$I') { this.say('[VER:1.1h.20190825:Practice machine]\r\nok\r\n'); this.pump(); return; }
    if (line.startsWith('$J=')) { this.motion(line.slice(3), true); return; }
    if (line.startsWith('$')) { this.say('ok\r\n'); this.pump(); return; }
    this.motion(line, false);
  }
  motion(line, jog) {
    const words = {}; for (const m of line.matchAll(/([A-Z])\s*([-+]?[\d.]+)/g)) (words[m[1]] ||= []).push(parseFloat(m[2]));
    const gs = words.G || [];
    let abs = this.abs;
    if (gs.includes(90)) abs = true; if (gs.includes(91)) abs = false;
    if (!jog) this.abs = abs;
    if (gs.includes(10) && words.L?.[0] === 20) { // set work origin here
      if (words.X) this.wco[0] = this.pos[0] - words.X[0]; if (words.Y) this.wco[1] = this.pos[1] - words.Y[0];
      this.say('ok\r\n'); this.pump(); return;
    }
    if (words.F) this.feedSet = words.F[0];
    if (words.S) this.spindle = words.S[0];
    if (words.M) for (const m of words.M) if (m === 3 || m === 4 || m === 5) this.laser = 'M' + m;
    if (!words.X && !words.Y) { this.say('ok\r\n'); this.pump(); return; }
    let x = this.pos[0], y = this.pos[1];
    const wx = this.pos[0] - this.wco[0], wy = this.pos[1] - this.wco[1];
    if (words.X) x = abs ? words.X[0] + this.wco[0] : this.pos[0] + words.X[0];
    if (words.Y) y = abs ? words.Y[0] + this.wco[1] : this.pos[1] + words.Y[0];
    if (jog && (x < -0.001 || y < -0.001 || x > this.dev.bedW + 0.001 || y > this.dev.bedH + 0.001)) { this.say('error:15\r\n'); this.pump(); return; }
    const rapid = gs.includes(0) || (!gs.includes(1) && this.lastG === 0);
    if (gs.includes(0)) this.lastG = 0; if (gs.includes(1)) this.lastG = 1;
    const feed = rapid && !jog ? 6000 : (this.feedSet || 1000);
    const d = Math.hypot(x - this.pos[0], y - this.pos[1]);
    const ms = Math.max(2, (d / feed) * 60000 / 20);
    this.state = jog ? 'Jog' : 'Run'; this.busy = true; this.feed = feed;
    const from = this.pos.slice(), t0 = performance.now();
    this.say('ok\r\n'); // GRBL acknowledges once the line is in its planner
    const tick = () => {
      if (this.hold) { this.timer = setTimeout(tick, 30); return; }
      const f = Math.min(1, (performance.now() - t0) / ms);
      this.pos = [from[0] + (x - from[0]) * f, from[1] + (y - from[1]) * f];
      if (f < 1) { this.timer = setTimeout(tick, 16); return; }
      this.busy = false; this.pump();
    };
    this.timer = setTimeout(tick, 0);
    void wx; void wy;
  }
  close() { this.q = []; clearTimeout(this.timer); return Promise.resolve(); }
}

// ---- controller -------------------------------------------------------------
export class Grbl {
  constructor() {
    this.t = null; this.buf = ''; this.inflight = []; this.pending = []; this.connected = false;
    this.status = { state: 'Disconnected', mpos: [0, 0], wpos: [0, 0], wco: [0, 0], feed: 0, spindle: 0 };
    this.job = null; this.listeners = {};
    this.bufferSize = 127;
  }
  on(ev, fn) { (this.listeners[ev] ||= []).push(fn); }
  emit(ev, ...a) { for (const f of this.listeners[ev] || []) f(...a); }
  log(text, kind = 'in') { this.emit('log', text, kind); }

  async connect(kind, dev) {
    this.bufferSize = dev.bufferSize || 127;
    const t = kind === 'serial' ? new SerialTransport(dev.baud) : new SimTransport(dev);
    t.onData = (s) => this.onData(s);
    t.onClose = () => this.lost();
    await t.open();
    this.t = t; this.connected = true; this.kind = kind;
    this.status.state = 'Connecting';
    this.emit('status', this.status);
    this.log(kind === 'serial' ? 'Port opened, waiting for the controller...' : 'Practice machine connected.', 'info');
    clearInterval(this.poll);
    this.poll = setInterval(() => { if (this.connected) this.realtime(0x3f, true); }, 250);
    // wake the controller (a fresh connection may need a newline before it answers)
    setTimeout(() => { if (this.connected) { this.t.write('\n'); } }, 400);
    setTimeout(() => { if (this.connected && this.status.state === 'Connecting') this.send('$I'); }, 1200);
  }
  async disconnect() {
    clearInterval(this.poll);
    if (this.t) await this.t.close();
    this.t = null; this.connected = false; this.job = null; this.inflight = []; this.pending = [];
    this.status.state = 'Disconnected'; this.emit('status', this.status); this.emit('job', null);
    this.log('Disconnected.', 'info');
  }
  lost() { clearInterval(this.poll); this.connected = false; this.t = null; this.status.state = 'Disconnected'; this.emit('status', this.status); this.log('Connection lost.', 'err'); }

  realtime(byte, quiet = false) { if (!this.t) return; this.t.writeByte(byte); if (!quiet) this.log(byte === 0x18 ? '[Ctrl-X reset]' : byte === 0x85 ? '[jog cancel]' : String.fromCharCode(byte), 'out'); }
  send(line, quiet = false) {
    if (!this.t) return;
    line = line.replace(/;.*$/, '').trim();
    if (!line) return;
    this.pending.push({ line, quiet });
    this.flush();
  }
  flush() {
    // character-counting: keep at most bufferSize bytes unacknowledged in the controller
    let used = this.inflight.reduce((a, l) => a + l.line.length + 1, 0);
    const take = () => {
      if (this.pending.length) return this.pending[0];
      if (this.job && !this.job.paused && this.job.next < this.job.lines.length) {
        const raw = this.job.lines[this.job.next];
        const l = raw.replace(/;.*$/, '').trim();
        if (!l) { this.job.next++; return take(); }
        return { line: l, job: true, idx: this.job.next };
      }
      return null;
    };
    for (;;) {
      const item = take();
      if (!item) break;
      if (used + item.line.length + 1 > this.bufferSize && this.inflight.length) break;
      if (item.job) this.job.next++; else this.pending.shift();
      this.inflight.push(item); used += item.line.length + 1;
      this.t.write(item.line + '\n');
      if (!item.quiet && !item.job) this.log(item.line, 'out');
    }
  }
  onData(s) {
    this.buf += s;
    let i;
    while ((i = this.buf.search(/\r?\n/)) >= 0) {
      const line = this.buf.slice(0, i).trim();
      this.buf = this.buf.slice(i + (this.buf[i] === '\r' ? 2 : 1));
      if (line) this.handleLine(line);
    }
    // status reports sometimes arrive without newline at the end of buffer
    if (/^<[^>]*>$/.test(this.buf.trim())) { this.handleLine(this.buf.trim()); this.buf = ''; }
  }
  handleLine(line) {
    if (line.startsWith('<')) { this.parseStatus(line); return; }
    if (line === 'ok' || line.startsWith('error:')) {
      const item = this.inflight.shift();
      if (line.startsWith('error:')) {
        const code = parseInt(line.slice(6), 10);
        const msg = GRBL_ERRORS[code] || 'Unknown error';
        this.log(`${line} - ${msg}${item ? '  (' + item.line + ')' : ''}`, 'err');
        if (item?.job && this.job) this.job.errors.push({ line: item.idx + 1, text: item.line, code, msg });
      } else if (item && !item.quiet && !item.job) this.log('ok', 'in');
      if (item?.job && this.job) { this.job.acked++; this.emit('job', this.job); this.checkDone(); }
      this.flush();
      return;
    }
    if (line.startsWith('ALARM:')) {
      const code = parseInt(line.slice(6), 10);
      this.log(`${line} - ${GRBL_ALARMS[code] || ''}`, 'err');
      if (this.job) { this.job.failed = line; this.endJob(false); }
      this.inflight = [];
      return;
    }
    if (/^Grbl|^GrblHAL/i.test(line)) { this.inflight = []; this.pending = this.pending.filter(() => false); this.log(line, 'in'); this.emit('hello', line); if (this.status.state === 'Connecting') this.status.state = 'Idle'; return; }
    if (line.startsWith('[VER:') && this.status.state === 'Connecting') this.status.state = 'Idle';
    this.log(line, 'in');
  }
  parseStatus(line) {
    const parts = line.slice(1, -1).split('|');
    const st = this.status;
    st.state = parts[0];
    for (const p of parts.slice(1)) {
      const [k, v] = p.split(':'); if (!v) continue;
      const n = v.split(',').map(Number);
      if (k === 'MPos') { st.mpos = n; st.wpos = [n[0] - st.wco[0], n[1] - st.wco[1]]; }
      else if (k === 'WPos') { st.wpos = n; st.mpos = [n[0] + st.wco[0], n[1] + st.wco[1]]; }
      else if (k === 'WCO') { st.wco = n; st.wpos = [st.mpos[0] - n[0], st.mpos[1] - n[1]]; }
      else if (k === 'FS') { st.feed = n[0]; st.spindle = n[1]; }
      else if (k === 'F') st.feed = n[0];
    }
    this.emit('status', st);
    this.checkDone();
  }

  // ---- jobs ------------------------------------------------------------------
  startJob(gcodeText, name) {
    if (!this.connected) throw new Error('Connect to the laser first.');
    if (this.job) throw new Error('A job is already running.');
    const lines = gcodeText.split('\n');
    this.job = { name, lines, next: 0, acked: 0, total: lines.filter((l) => l.replace(/;.*$/, '').trim()).length, started: Date.now(), paused: false, errors: [] };
    this.log(`Starting job "${name}" (${this.job.total} lines)`, 'info');
    this.emit('job', this.job);
    this.flush();
  }
  pause() { if (!this.job) return; this.job.paused = true; this.realtime(0x21); this.emit('job', this.job); }
  resume() { if (!this.job) return; this.job.paused = false; this.realtime(0x7e); this.flush(); this.emit('job', this.job); }
  stop() {
    if (!this.t) return;
    const hadJob = !!this.job;
    this.realtime(0x21); // feed hold first so the head decelerates
    setTimeout(() => {
      this.realtime(0x18); // then reset, which also turns the laser off
      this.inflight = []; this.pending = [];
      if (hadJob) { this.job.failed = 'Stopped'; this.endJob(false); }
    }, 150);
  }
  checkDone() {
    const j = this.job;
    if (j && j.next >= j.lines.length && j.acked >= j.total && /^Idle/.test(this.status.state)) this.endJob(true);
  }
  endJob(ok) {
    const j = this.job; if (!j) return;
    this.job = null;
    const secs = Math.round((Date.now() - j.started) / 1000);
    this.log(ok ? `Job finished in ${secs}s${j.errors.length ? ` with ${j.errors.length} error(s)` : ''}.` : `Job ended: ${j.failed}`, ok ? 'info' : 'err');
    this.emit('job', null, { ok, secs, errors: j.errors, name: j.name });
  }

  jog(dx, dy, feedMmMin) {
    // $J=G91 G21 X.. F..  (jog command family observed in the original: "$J=", "G91", "G21")
    let cmd = '$J=G91 G21';
    if (dx) cmd += ' X' + +dx.toFixed(3);
    if (dy) cmd += ' Y' + +dy.toFixed(3);
    cmd += ' F' + Math.round(feedMmMin);
    this.send(cmd);
  }
}
