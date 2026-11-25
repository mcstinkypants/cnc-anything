//! Talking to a GRBL laser controller over a serial port, or to a built-in practice machine.
//! A worker thread streams lines with GRBL's character-counting method so the window never waits.

use crate::geom::P;
use std::collections::VecDeque;
use std::io;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub fn grbl_error(code: u32) -> &'static str {
    match code {
        1 => "A command letter was expected but not found.",
        2 => "A number is missing or badly formatted.",
        3 => "That '$' command isn't supported.",
        4 => "A negative value was given where a positive one is needed.",
        5 => "Homing is turned off in the controller settings.",
        8 => "'$' commands only work while the machine is idle.",
        9 => "Commands are locked while in alarm or door state (try Unlock or Home).",
        10 => "Soft limits need homing to be turned on.",
        11 => "The line is too long for the controller.",
        13 => "The safety door was opened.",
        15 => "The jog target is beyond the machine travel.",
        16 => "The jog command has no '=' or contains a command that isn't allowed.",
        17 => "Laser mode needs PWM output.",
        20 => "Unsupported or unknown G-code command.",
        21 => "More than one command from the same group on one line.",
        22 => "Feed rate has not been set.",
        24 => "Two commands on one line both need axis words.",
        25 => "A word was repeated on the line.",
        26 => "A motion command is missing its axis words.",
        33 => "The motion target is not valid.",
        _ => "Controller error.",
    }
}
pub fn grbl_alarm(code: u32) -> &'static str {
    match code {
        1 => "Hard limit switch triggered. Re-home the machine.",
        2 => "The move would go outside the machine travel (soft limit).",
        3 => "Reset while moving: position may be lost. Re-home.",
        4 | 5 => "Probe problem.",
        6..=10 => "Homing failed.",
        _ => "",
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    In,
    Out,
    Info,
    Err,
}

#[derive(Clone)]
pub struct JobProgress {
    #[allow(dead_code)]
    pub name: String,
    pub total: usize,
    pub acked: usize,
    pub started: Instant,
    pub paused: bool,
    pub errors: usize,
}
#[derive(Clone)]
pub struct JobDone {
    pub ok: bool,
    pub secs: u64,
    pub errors: usize,
    pub msg: String,
}

pub struct MachineState {
    pub connected: bool,
    pub sim: bool,
    pub state: String,
    pub mpos: P,
    pub wpos: P,
    pub wco: P,
    /// work Z (0 = stock top once Z zero is set) and its offset
    pub wz: f64,
    pub mz: f64,
    pub wco_z: f64,
    pub feed: f64,
    pub spindle: f64,
    pub log: VecDeque<(LogKind, String)>,
    pub log_gen: u64,
    pub job: Option<JobProgress>,
    pub done: Option<JobDone>,
}
impl MachineState {
    fn log(&mut self, k: LogKind, s: impl Into<String>) {
        self.log.push_back((k, s.into()));
        while self.log.len() > 1500 {
            self.log.pop_front();
        }
        self.log_gen += 1;
    }
}

pub enum Cmd {
    Send(String),
    Realtime(u8),
    Start { name: String, lines: Vec<String> },
    Pause,
    Resume,
    Stop,
    Disconnect,
}

pub trait Transport: Send {
    fn write(&mut self, b: &[u8]) -> io::Result<()>;
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
}

struct Serial(Box<dyn serialport::SerialPort>);
impl Transport for Serial {
    fn write(&mut self, b: &[u8]) -> io::Result<()> {
        io::Write::write_all(&mut self.0, b)?;
        io::Write::flush(&mut self.0)
    }
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match io::Read::read(&mut self.0, buf) {
            Ok(n) => Ok(n),
            Err(e) if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::Interrupted => Ok(0),
            Err(e) => Err(e),
        }
    }
}

/// Serial ports that look like laser controllers
pub fn list_ports() -> Vec<String> {
    let mut v: Vec<String> = serialport::available_ports().map(|p| p.into_iter().map(|x| x.port_name).collect()).unwrap_or_default();
    if let Ok(rd) = std::fs::read_dir("/dev") {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with("ttyUSB") || n.starts_with("ttyACM") || n.starts_with("cu.usb") {
                let p = format!("/dev/{n}");
                if !v.contains(&p) {
                    v.push(p);
                }
            }
        }
    }
    v.retain(|p| !p.starts_with("/dev/ttyS"));
    v.sort();
    v
}

// ---------------------------------------------------------------- practice machine
struct Motion {
    from: P,
    to: P,
    z0: f64,
    z1: f64,
    dur: f64,
    done: f64,
}
/// A pretend GRBL 1.1 controller: moves at 20x speed and answers like the real thing.
pub struct Sim {
    out: Vec<u8>,
    inbuf: Vec<u8>,
    q: VecDeque<String>,
    pos: P,
    wco: P,
    z: f64,
    wco_z: f64,
    state: String,
    hold: bool,
    abs: bool,
    last_g: i32,
    feed_set: f64,
    feed: f64,
    spindle: f64,
    motion: Option<Motion>,
    home_until: Option<Instant>,
    last: Instant,
    bed: P,
    s_max: f64,
    started: Instant,
    greeted: bool,
}
impl Sim {
    pub fn new(bed: P, s_max: f64) -> Sim {
        Sim {
            out: vec![],
            inbuf: vec![],
            q: VecDeque::new(),
            pos: [0.0, 0.0],
            wco: [0.0, 0.0],
            z: 0.0,
            wco_z: 0.0,
            state: "Idle".into(),
            hold: false,
            abs: true,
            last_g: 0,
            feed_set: 1000.0,
            feed: 0.0,
            spindle: 0.0,
            motion: None,
            home_until: None,
            last: Instant::now(),
            bed,
            s_max,
            started: Instant::now(),
            greeted: false,
        }
    }
    fn say(&mut self, s: &str) {
        self.out.extend_from_slice(s.as_bytes());
    }
    fn status(&mut self) {
        let (m, w) = (self.pos, self.wco);
        let s = format!("<{}|MPos:{:.3},{:.3},{:.3}|FS:{},{}|WCO:{:.3},{:.3},{:.3}>\r\n", self.state, m[0], m[1], self.z, self.feed.round(), self.spindle.round(), w[0], w[1], self.wco_z);
        self.say(&s);
    }
    fn busy(&self) -> bool {
        self.motion.is_some() || self.home_until.is_some()
    }
    fn tick(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f64();
        self.last = now;
        if !self.greeted && now - self.started > Duration::from_millis(150) {
            self.greeted = true;
            self.say("\r\nGrbl 1.1h ['$' for help]\r\n");
        }
        if let Some(t) = self.home_until {
            if now >= t {
                self.home_until = None;
                self.pos = [0.0, 0.0];
                self.state = "Idle".into();
                self.say("ok\r\n");
            }
        }
        if !self.hold {
            if let Some(m) = &mut self.motion {
                m.done += dt;
                let f = (m.done / m.dur).min(1.0);
                self.pos = [m.from[0] + (m.to[0] - m.from[0]) * f, m.from[1] + (m.to[1] - m.from[1]) * f];
                self.z = m.z0 + (m.z1 - m.z0) * f;
                if f >= 1.0 {
                    self.motion = None;
                }
            }
        }
        while !self.busy() && !self.hold && !self.q.is_empty() {
            let l = self.q.pop_front().unwrap();
            self.exec(&l);
        }
        if !self.busy() && self.q.is_empty() && (self.state == "Run" || self.state == "Jog") {
            self.state = "Idle".into();
            self.feed = 0.0;
        }
    }
    fn exec(&mut self, line: &str) {
        let line = line.to_uppercase();
        if self.state == "Alarm" && !(line.starts_with("$X") || line.starts_with("$H") || line == "$$" || line == "$I") {
            self.say("error:9\r\n");
            return;
        }
        match line.as_str() {
            "$X" => {
                self.state = "Idle".into();
                self.say("[MSG:Caution: Unlocked]\r\nok\r\n");
                return;
            }
            "$H" => {
                self.state = "Home".into();
                self.home_until = Some(Instant::now() + Duration::from_millis(800));
                return;
            }
            "$$" => {
                let s = format!("$30={}\r\n$31=0\r\n$32=1\r\n$110=6000.000\r\n$111=6000.000\r\n$130={:.3}\r\n$131={:.3}\r\nok\r\n", self.s_max, self.bed[0], self.bed[1]);
                self.say(&s);
                return;
            }
            "$I" => {
                self.say("[VER:1.1h.20190825:Practice machine]\r\nok\r\n");
                return;
            }
            _ => {}
        }
        let jog = line.starts_with("$J=");
        if line.starts_with('$') && !jog {
            self.say("ok\r\n");
            return;
        }
        let body = if jog { &line[3..] } else { &line[..] };
        let mut words: Vec<(char, f64)> = vec![];
        let cs: Vec<char> = body.chars().collect();
        let mut i = 0;
        while i < cs.len() {
            if cs[i].is_ascii_alphabetic() {
                let l = cs[i];
                i += 1;
                let st = i;
                while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.' || cs[i] == '-' || cs[i] == '+' || cs[i] == ' ') {
                    i += 1;
                }
                let n: String = cs[st..i].iter().filter(|c| **c != ' ').collect();
                if let Ok(v) = n.parse::<f64>() {
                    words.push((l, v));
                }
            } else {
                i += 1;
            }
        }
        let g: Vec<f64> = words.iter().filter(|w| w.0 == 'G').map(|w| w.1).collect();
        let get = |c: char| words.iter().find(|w| w.0 == c).map(|w| w.1);
        let mut abs = self.abs;
        if g.contains(&90.0) {
            abs = true;
        }
        if g.contains(&91.0) {
            abs = false;
        }
        if !jog {
            self.abs = abs;
        }
        if g.contains(&10.0) && get('L') == Some(20.0) {
            if let Some(x) = get('X') {
                self.wco[0] = self.pos[0] - x;
            }
            if let Some(y) = get('Y') {
                self.wco[1] = self.pos[1] - y;
            }
            if let Some(z) = get('Z') {
                self.wco_z = self.z - z;
            }
            self.say("ok\r\n");
            return;
        }
        if let Some(f) = get('F') {
            self.feed_set = f;
        }
        if let Some(s) = get('S') {
            self.spindle = s;
        }
        if g.contains(&4.0) {
            self.say("ok\r\n");
            return;
        }
        // M0: program pause (tool change), resumed with cycle start
        if words.iter().any(|w| w.0 == 'M' && w.1 == 0.0) {
            self.hold = true;
            self.state = "Hold:0".into();
            self.say("ok\r\n");
            return;
        }
        if g.contains(&92.0) {
            if let Some(x) = get('X') {
                self.wco[0] = self.pos[0] - x;
            }
            if let Some(y) = get('Y') {
                self.wco[1] = self.pos[1] - y;
            }
            self.say("ok\r\n");
            return;
        }
        let (gx, gy, gz) = (get('X'), get('Y'), get('Z'));
        if gx.is_none() && gy.is_none() && gz.is_none() {
            self.say("ok\r\n");
            return;
        }
        let mut t = self.pos;
        if let Some(x) = gx {
            t[0] = if abs { x + self.wco[0] } else { self.pos[0] + x };
        }
        if let Some(y) = gy {
            t[1] = if abs { y + self.wco[1] } else { self.pos[1] + y };
        }
        let tz = match gz {
            Some(z) => if abs { z + self.wco_z } else { self.z + z },
            None => self.z,
        };
        if jog && (t[0] < -0.001 || t[1] < -0.001 || t[0] > self.bed[0] + 0.001 || t[1] > self.bed[1] + 0.001) {
            self.say("error:15\r\n");
            return;
        }
        if g.contains(&0.0) {
            self.last_g = 0;
        }
        if g.contains(&1.0) {
            self.last_g = 1;
        }
        let rapid = self.last_g == 0 && !jog;
        let feed = if rapid { 6000.0 } else { self.feed_set.max(1.0) };
        let d = ((t[0] - self.pos[0]).powi(2) + (t[1] - self.pos[1]).powi(2) + (tz - self.z).powi(2)).sqrt();
        self.state = if jog { "Jog".into() } else { "Run".into() };
        self.feed = feed;
        self.motion = Some(Motion { from: self.pos, to: t, z0: self.z, z1: tz, dur: (d / feed * 60.0 / 20.0).max(0.002), done: 0.0 });
        self.say("ok\r\n");
    }
}
impl Transport for Sim {
    fn write(&mut self, b: &[u8]) -> io::Result<()> {
        for &c in b {
            match c {
                b'?' => self.status(),
                b'!' => {
                    self.hold = true;
                    if self.state == "Run" || self.state == "Jog" {
                        self.state = "Hold:0".into();
                    }
                }
                b'~' => {
                    self.hold = false;
                    if self.state.starts_with("Hold") {
                        self.state = if self.busy() { "Run".into() } else { "Idle".into() };
                    }
                }
                0x18 => {
                    let moving = self.busy();
                    self.q.clear();
                    self.motion = None;
                    self.home_until = None;
                    self.hold = false;
                    self.inbuf.clear();
                    self.state = if moving { "Alarm".into() } else { "Idle".into() };
                    self.say("\r\nGrbl 1.1h ['$' for help]\r\n");
                    if moving {
                        self.say("ALARM:3\r\n");
                    }
                }
                0x85 => {
                    if self.state == "Jog" {
                        self.motion = None;
                        self.q.retain(|l| !l.starts_with("$J="));
                    }
                }
                b'\n' => {
                    let l = String::from_utf8_lossy(&self.inbuf).trim().to_string();
                    self.inbuf.clear();
                    if !l.is_empty() {
                        self.q.push_back(l);
                    }
                }
                b'\r' => {}
                _ => self.inbuf.push(c),
            }
        }
        Ok(())
    }
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.tick();
        if self.out.is_empty() {
            std::thread::sleep(Duration::from_millis(4));
            return Ok(0);
        }
        let n = self.out.len().min(buf.len());
        buf[..n].copy_from_slice(&self.out[..n]);
        self.out.drain(..n);
        Ok(n)
    }
}

// ---------------------------------------------------------------- controller
pub struct Machine {
    pub st: Arc<Mutex<MachineState>>,
    tx: Option<Sender<Cmd>>,
}

impl Machine {
    pub fn new() -> Machine {
        Machine {
            st: Arc::new(Mutex::new(MachineState {
                connected: false,
                sim: false,
                state: "Disconnected".into(),
                mpos: [0.0, 0.0],
                wpos: [0.0, 0.0],
                wco: [0.0, 0.0],
                wz: 0.0,
                mz: 0.0,
                wco_z: 0.0,
                feed: 0.0,
                spindle: 0.0,
                log: VecDeque::new(),
                log_gen: 0,
                job: None,
                done: None,
            })),
            tx: None,
        }
    }
    pub fn connected(&self) -> bool {
        self.st.lock().unwrap().connected
    }
    pub fn connect_serial(&mut self, port: &str, baud: u32, buffer: usize) -> Result<(), String> {
        let p = serialport::new(port, baud).timeout(Duration::from_millis(5)).open().map_err(|e| {
            let s = e.to_string();
            if s.to_lowercase().contains("permission") {
                format!("{s}. On Linux, add yourself to the 'dialout' group (then log out and back in).")
            } else {
                s
            }
        })?;
        self.start(Box::new(Serial(p)), buffer, false, &format!("Opened {port} at {baud} baud, waiting for the controller..."));
        Ok(())
    }
    pub fn connect_sim(&mut self, bed: P, s_max: f64) {
        self.start(Box::new(Sim::new(bed, s_max)), 127, true, "Practice machine connected.");
    }
    fn start(&mut self, t: Box<dyn Transport>, buffer: usize, sim: bool, msg: &str) {
        self.disconnect();
        let (tx, rx) = channel();
        {
            let mut s = self.st.lock().unwrap();
            s.connected = true;
            s.sim = sim;
            s.state = "Connecting".into();
            s.job = None;
            s.log(LogKind::Info, msg);
        }
        let st = self.st.clone();
        std::thread::spawn(move || worker(t, rx, st, buffer.max(32)));
        self.tx = Some(tx);
    }
    pub fn disconnect(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(Cmd::Disconnect);
        }
    }
    pub fn cmd(&self, c: Cmd) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(c);
        }
    }
    pub fn send(&self, line: &str) {
        self.cmd(Cmd::Send(line.to_string()));
    }
    pub fn jog_z(&self, dz: f64, feed: f64) {
        self.send(&format!("$J=G91 G21 Z{} F{}", crate::gcode::fmt(dz, 3, false), feed.round()));
    }
    pub fn jog(&self, dx: f64, dy: f64, feed: f64) {
        // jog command family observed in the original: "$J=", "G91", "G21"
        let mut c = String::from("$J=G91 G21");
        if dx != 0.0 {
            c += &format!(" X{}", crate::gcode::fmt(dx, 3, false));
        }
        if dy != 0.0 {
            c += &format!(" Y{}", crate::gcode::fmt(dy, 3, false));
        }
        c += &format!(" F{}", feed.round());
        self.send(&c);
    }
}

struct Item {
    line: String,
    job: Option<usize>,
    quiet: bool,
}

fn worker(mut t: Box<dyn Transport>, rx: Receiver<Cmd>, st: Arc<Mutex<MachineState>>, buffer: usize) {
    let mut pending: VecDeque<Item> = VecDeque::new();
    let mut inflight: VecDeque<Item> = VecDeque::new();
    let mut job_lines: Vec<String> = vec![];
    let mut job_next = 0usize;
    let mut job_paused = false;
    let mut job_started = Instant::now();
    let mut job_errors = 0usize;
    let mut has_job = false;
    let mut line_buf = String::new();
    let mut last_poll = Instant::now();
    let mut reset_at: Option<Instant> = None;
    let mut buf = [0u8; 2048];
    let connected_at = Instant::now();
    let mut woke = false;
    let mut asked_info = false;

    let end_job = |st: &Arc<Mutex<MachineState>>, ok: bool, msg: &str, started: Instant, errors: usize, has_job: &mut bool| {
        let mut s = st.lock().unwrap();
        s.job = None;
        let secs = started.elapsed().as_secs();
        s.done = Some(JobDone { ok, secs, errors, msg: msg.to_string() });
        let m = if ok { format!("Job finished in {secs}s{}.", if errors > 0 { format!(" with {errors} error(s)") } else { String::new() }) } else { format!("Job ended: {msg}") };
        s.log(if ok { LogKind::Info } else { LogKind::Err }, m);
        *has_job = false;
    };

    'outer: loop {
        // commands from the window
        loop {
            match rx.try_recv() {
                Ok(Cmd::Send(l)) => {
                    let l = l.split(';').next().unwrap_or("").trim().to_string();
                    if !l.is_empty() {
                        pending.push_back(Item { line: l, job: None, quiet: false });
                    }
                }
                Ok(Cmd::Realtime(b)) => {
                    let _ = t.write(&[b]);
                    if b != b'?' {
                        st.lock().unwrap().log(LogKind::Out, match b {
                            0x18 => "[Ctrl-X reset]".to_string(),
                            0x85 => "[jog cancel]".to_string(),
                            _ => (b as char).to_string(),
                        });
                    }
                }
                Ok(Cmd::Start { name, lines }) => {
                    if has_job {
                        continue;
                    }
                    let total = lines.iter().filter(|l| !l.split(';').next().unwrap_or("").trim().is_empty()).count();
                    job_lines = lines;
                    job_next = 0;
                    job_paused = false;
                    job_errors = 0;
                    job_started = Instant::now();
                    has_job = true;
                    let mut s = st.lock().unwrap();
                    s.log(LogKind::Info, format!("Starting job \"{name}\" ({total} lines)"));
                    s.job = Some(JobProgress { name, total, acked: 0, started: job_started, paused: false, errors: 0 });
                    s.done = None;
                }
                Ok(Cmd::Pause) => {
                    job_paused = true;
                    let _ = t.write(b"!");
                    if let Some(j) = &mut st.lock().unwrap().job {
                        j.paused = true;
                    }
                }
                Ok(Cmd::Resume) => {
                    job_paused = false;
                    let _ = t.write(b"~");
                    if let Some(j) = &mut st.lock().unwrap().job {
                        j.paused = false;
                    }
                }
                Ok(Cmd::Stop) => {
                    let _ = t.write(b"!"); // feed hold first so the head slows down
                    reset_at = Some(Instant::now() + Duration::from_millis(150));
                }
                Ok(Cmd::Disconnect) | Err(std::sync::mpsc::TryRecvError::Disconnected) => break 'outer,
                Err(_) => break,
            }
        }
        if let Some(at) = reset_at {
            if Instant::now() >= at {
                reset_at = None;
                let _ = t.write(&[0x18]); // reset also turns the laser off
                inflight.clear();
                pending.clear();
                st.lock().unwrap().log(LogKind::Out, "[Ctrl-X reset]");
                if has_job {
                    end_job(&st, false, "Stopped", job_started, job_errors, &mut has_job);
                }
            }
        }
        if !woke && connected_at.elapsed() > Duration::from_millis(400) {
            woke = true;
            let _ = t.write(b"\n");
        }
        if !asked_info && connected_at.elapsed() > Duration::from_millis(2500) {
            asked_info = true;
            if st.lock().unwrap().state == "Connecting" {
                pending.push_back(Item { line: "$I".into(), job: None, quiet: false });
            }
        }
        if last_poll.elapsed() >= Duration::from_millis(250) {
            last_poll = Instant::now();
            let _ = t.write(b"?");
        }
        // keep at most `buffer` bytes unacknowledged in the controller
        let mut used: usize = inflight.iter().map(|i| i.line.len() + 1).sum();
        loop {
            let next = if let Some(p) = pending.front() {
                Some(Item { line: p.line.clone(), job: None, quiet: p.quiet })
            } else if has_job && !job_paused && job_next < job_lines.len() {
                let l = job_lines[job_next].split(';').next().unwrap_or("").trim().to_string();
                if l.is_empty() {
                    if let Some(note) = job_lines[job_next].split_once("; TOOL CHANGE:") {
                        st.lock().unwrap().log(LogKind::Info, format!("Tool change:{}", note.1));
                    }
                    job_next += 1;
                    continue;
                }
                Some(Item { line: l, job: Some(job_next), quiet: true })
            } else {
                None
            };
            let Some(item) = next else { break };
            if used + item.line.len() + 1 > buffer && !inflight.is_empty() {
                break;
            }
            if item.job.is_some() {
                job_next += 1;
            } else {
                pending.pop_front();
                if !item.quiet {
                    st.lock().unwrap().log(LogKind::Out, item.line.clone());
                }
            }
            used += item.line.len() + 1;
            if t.write(format!("{}\n", item.line).as_bytes()).is_err() {
                st.lock().unwrap().log(LogKind::Err, "Write to the controller failed.");
                break 'outer;
            }
            inflight.push_back(item);
        }
        // read replies
        let n = match t.read(&mut buf) {
            Ok(n) => n,
            Err(e) => {
                st.lock().unwrap().log(LogKind::Err, format!("Connection lost: {e}"));
                break 'outer;
            }
        };
        line_buf.push_str(&String::from_utf8_lossy(&buf[..n]));
        while let Some(i) = line_buf.find('\n') {
            let line = line_buf[..i].trim().to_string();
            line_buf.drain(..=i);
            if line.is_empty() {
                continue;
            }
            let mut s = st.lock().unwrap();
            if line.starts_with('<') {
                parse_status(&mut s, &line);
            } else if line == "ok" || line.starts_with("error:") {
                let item = inflight.pop_front();
                if let Some(code) = line.strip_prefix("error:") {
                    let code: u32 = code.trim().parse().unwrap_or(0);
                    let what = item.as_ref().map(|i| format!("  ({})", i.line)).unwrap_or_default();
                    s.log(LogKind::Err, format!("{line} - {}{what}", grbl_error(code)));
                    if item.as_ref().and_then(|i| i.job).is_some() {
                        job_errors += 1;
                    }
                } else if let Some(i) = &item {
                    if i.job.is_none() && !i.quiet {
                        s.log(LogKind::In, "ok");
                    }
                }
                if item.as_ref().and_then(|i| i.job).is_some() {
                    if let Some(j) = &mut s.job {
                        j.acked += 1;
                        j.errors = job_errors;
                    }
                }
            } else if let Some(code) = line.strip_prefix("ALARM:") {
                let code: u32 = code.trim().parse().unwrap_or(0);
                s.log(LogKind::Err, format!("{line} - {}", grbl_alarm(code)));
                inflight.clear();
                if has_job {
                    drop(s);
                    end_job(&st, false, &line, job_started, job_errors, &mut has_job);
                    continue;
                }
            } else if line.starts_with("Grbl") || line.starts_with("GrblHAL") {
                inflight.clear();
                if s.state == "Connecting" {
                    s.state = "Idle".into();
                }
                s.log(LogKind::In, line.clone());
            } else {
                if line.starts_with("[VER:") && s.state == "Connecting" {
                    s.state = "Idle".into();
                }
                s.log(LogKind::In, line.clone());
            }
        }
        // finished?
        if has_job && job_next >= job_lines.len() && inflight.is_empty() {
            let (acked, total, idle) = {
                let s = st.lock().unwrap();
                (s.job.as_ref().map(|j| j.acked).unwrap_or(0), s.job.as_ref().map(|j| j.total).unwrap_or(0), s.state.starts_with("Idle"))
            };
            if acked >= total && idle {
                end_job(&st, true, "", job_started, job_errors, &mut has_job);
            }
        }
    }
    let mut s = st.lock().unwrap();
    s.connected = false;
    s.state = "Disconnected".into();
    s.job = None;
    s.log(LogKind::Info, "Disconnected.");
}

fn parse_status(s: &mut MachineState, line: &str) {
    let inner = line.trim_start_matches('<').trim_end_matches('>');
    let mut parts = inner.split('|');
    if let Some(state) = parts.next() {
        s.state = state.to_string();
    }
    for p in parts {
        let Some((k, v)) = p.split_once(':') else { continue };
        let n: Vec<f64> = v.split(',').filter_map(|x| x.parse().ok()).collect();
        if n.len() < 2 && k != "F" {
            if k == "FS" || k == "Ov" {
                continue;
            }
        }
        match k {
            "MPos" if n.len() >= 2 => {
                if n.len() >= 3 {
                    s.mz = n[2];
                    s.wz = n[2] - s.wco_z;
                }
                s.mpos = [n[0], n[1]];
                s.wpos = [n[0] - s.wco[0], n[1] - s.wco[1]];
            }
            "WPos" if n.len() >= 2 => {
                if n.len() >= 3 {
                    s.wz = n[2];
                    s.mz = n[2] + s.wco_z;
                }
                s.wpos = [n[0], n[1]];
                s.mpos = [n[0] + s.wco[0], n[1] + s.wco[1]];
            }
            "WCO" if n.len() >= 2 => {
                if n.len() >= 3 {
                    s.wco_z = n[2];
                    s.wz = s.mz - n[2];
                }
                s.wco = [n[0], n[1]];
                s.wpos = [s.mpos[0] - n[0], s.mpos[1] - n[1]];
            }
            "FS" if n.len() >= 2 => {
                s.feed = n[0];
                s.spindle = n[1];
            }
            "F" if !n.is_empty() => s.feed = n[0],
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn practice_machine_runs_a_job() {
        let mut m = Machine::new();
        m.connect_sim([400.0, 400.0], 1000.0);
        let wait = |m: &Machine, f: &dyn Fn(&MachineState) -> bool, secs: u64| {
            let t = Instant::now();
            while t.elapsed() < Duration::from_secs(secs) {
                if f(&m.st.lock().unwrap()) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            false
        };
        assert!(wait(&m, &|s| s.state.starts_with("Idle"), 5), "controller never became idle");
        let lines: Vec<String> = "G00 G17 G40 G21 G54\nG90\nM4\nG0X10Y10\nG1X20S500F6000\nY20\nX10\nY10\nG1 S0\nM5\nG0 X0 Y0\nM2".lines().map(String::from).collect();
        m.cmd(Cmd::Start { name: "test".into(), lines });
        assert!(wait(&m, &|s| s.done.is_some(), 20), "job did not finish");
        let s = m.st.lock().unwrap();
        let d = s.done.as_ref().unwrap();
        assert!(d.ok && d.errors == 0, "job failed: {}", d.msg);
        drop(s);
        // a router job that pauses for a tool change (M0) carries on after Resume
        let lines: Vec<String> = "G21 G90 G17 G94\nG0 Z5\nM3 S18000\nG0X10Y10\nG1Z-2F300\nG1X20F1500\nG0Z5\nM5\n; TOOL CHANGE: fit T1\nM0\nM3 S18000\nG0X30Y30\nG1Z-1F300\nG0Z5\nM5\nM2".lines().map(String::from).collect();
        m.cmd(Cmd::Start { name: "router".into(), lines });
        assert!(wait(&m, &|s| s.state.starts_with("Hold"), 10), "job did not pause for the tool change");
        assert!(m.st.lock().unwrap().log.iter().any(|l| l.1.starts_with("Tool change:")));
        m.cmd(Cmd::Realtime(b'~'));
        assert!(wait(&m, &|s| s.done.as_ref().map(|d| d.ok).unwrap_or(false) && s.done.as_ref().unwrap().secs < 60 && s.job.is_none() && (s.wz - 5.0).abs() < 0.01, 20), "router job did not finish after resuming");
        let x0 = m.st.lock().unwrap().wpos[0];
        m.jog(5.0, 0.0, 3000.0);
        assert!(wait(&m, &|s| (s.wpos[0] - x0 - 5.0).abs() < 0.01 && s.state.starts_with("Idle"), 5), "jog did not arrive");
        m.disconnect();
    }
}
