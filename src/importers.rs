//! Text outlines (from fonts), SVG drawings and pictures.

use crate::geom::*;
use base64::Engine;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------- fonts
static SANS: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular.ttf");
static SANS_B: &[u8] = include_bytes!("../assets/fonts/NotoSans-Bold.ttf");
static SANS_I: &[u8] = include_bytes!("../assets/fonts/NotoSans-Italic.ttf");
static SANS_BI: &[u8] = include_bytes!("../assets/fonts/NotoSans-BoldItalic.ttf");
static SERIF: &[u8] = include_bytes!("../assets/fonts/NotoSerif-Regular.ttf");
static SERIF_B: &[u8] = include_bytes!("../assets/fonts/NotoSerif-Bold.ttf");
static MONO: &[u8] = include_bytes!("../assets/fonts/NotoSansMono-Regular.ttf");
static MONO_B: &[u8] = include_bytes!("../assets/fonts/NotoSansMono-Bold.ttf");

pub const BUILTIN_FONTS: [&str; 3] = ["Sans", "Serif", "Mono"];

/// Font files installed on this computer (name -> path)
pub fn system_fonts() -> Vec<(String, PathBuf)> {
    let mut dirs: Vec<PathBuf> = vec!["/usr/share/fonts".into(), "/usr/local/share/fonts".into()];
    if let Ok(h) = std::env::var("HOME") {
        dirs.push(Path::new(&h).join(".local/share/fonts"));
        dirs.push(Path::new(&h).join(".fonts"));
    }
    let mut out = Vec::new();
    fn walk(d: &Path, out: &mut Vec<(String, PathBuf)>, depth: usize) {
        if depth > 5 || out.len() > 3000 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out, depth + 1);
            } else if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                if ext.eq_ignore_ascii_case("ttf") || ext.eq_ignore_ascii_case("otf") {
                    if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                        out.push((stem.to_string(), p.clone()));
                    }
                }
            }
        }
    }
    for d in dirs {
        walk(&d, &mut out, 0);
    }
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

fn builtin_family(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if n == "serif" || (n.contains("serif") && !n.contains("sans")) || n.contains("georgia") || n.contains("book") {
        "Serif"
    } else if n == "mono" || n.contains("mono") {
        "Mono"
    } else {
        "Sans"
    }
}

fn font_bytes(name: &str, bold: bool, italic: bool) -> std::borrow::Cow<'static, [u8]> {
    if !BUILTIN_FONTS.contains(&name) {
        if let Some((_, p)) = system_fonts().into_iter().find(|(n, _)| n == name) {
            if let Ok(b) = std::fs::read(p) {
                return std::borrow::Cow::Owned(b);
            }
        }
    }
    std::borrow::Cow::Borrowed(match (builtin_family(name), bold, italic) {
        ("Serif", true, _) => SERIF_B,
        ("Serif", false, _) => SERIF,
        ("Mono", true, _) => MONO_B,
        ("Mono", false, _) => MONO,
        (_, true, true) => SANS_BI,
        (_, true, false) => SANS_B,
        (_, false, true) => SANS_I,
        _ => SANS,
    })
}

struct Outline {
    cs: Vec<Contour>,
    cur: Vec<P>,
    s: f64,
    ox: f64,
    oy: f64,
    last: P,
}
impl Outline {
    fn p(&self, x: f32, y: f32) -> P {
        [self.ox + x as f64 * self.s, self.oy + y as f64 * self.s]
    }
    fn end(&mut self) {
        if self.cur.len() > 2 {
            let mut pts = std::mem::take(&mut self.cur);
            let f = pts[0];
            pts.push(f);
            self.cs.push(Contour { pts, closed: true });
        }
        self.cur.clear();
    }
}
impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.end();
        let p = self.p(x, y);
        self.cur.push(p);
        self.last = p;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.p(x, y);
        self.cur.push(p);
        self.last = p;
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (a, c, b) = (self.last, self.p(x1, y1), self.p(x, y));
        let n = 8;
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            self.cur.push([u * u * a[0] + 2.0 * u * t * c[0] + t * t * b[0], u * u * a[1] + 2.0 * u * t * c[1] + t * t * b[1]]);
        }
        self.last = b;
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (a, c1, c2, b) = (self.last, self.p(x1, y1), self.p(x2, y2), self.p(x, y));
        let mut out = vec![];
        cubic(a, c1, c2, b, &mut out);
        self.cur.extend(out);
        self.last = b;
    }
    fn close(&mut self) {
        self.end();
    }
}

struct CurveOutline {
    curves: Vec<crate::spline::Curve>,
    cur: Vec<crate::spline::Node>,
    s: f64,
    ox: f64,
    oy: f64,
}
impl CurveOutline {
    fn p(&self, x: f32, y: f32) -> P {
        [self.ox + x as f64 * self.s, self.oy + y as f64 * self.s]
    }
    fn end(&mut self) {
        let mut nodes = std::mem::take(&mut self.cur);
        if nodes.len() > 1 {
            let (f, l) = (nodes[0].p, nodes.last().unwrap().p);
            if (f[0] - l[0]).hypot(f[1] - l[1]) < 1e-9 {
                let last = nodes.pop().unwrap();
                nodes[0].i = last.i;
            }
        }
        for n in nodes.iter_mut() {
            crate::spline::classify(n);
        }
        if nodes.len() > 2 {
            self.curves.push(crate::spline::Curve { nodes, closed: true });
        }
    }
}
impl ttf_parser::OutlineBuilder for CurveOutline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.end();
        let p = self.p(x, y);
        self.cur.push(crate::spline::Node::sharp(p));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.p(x, y);
        self.cur.push(crate::spline::Node::sharp(p));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (c, p) = (self.p(x1, y1), self.p(x, y));
        if let Some(last) = self.cur.last_mut() {
            let a = last.p;
            last.o = [a[0] + 2.0 / 3.0 * (c[0] - a[0]), a[1] + 2.0 / 3.0 * (c[1] - a[1])];
        }
        let i = [p[0] + 2.0 / 3.0 * (c[0] - p[0]), p[1] + 2.0 / 3.0 * (c[1] - p[1])];
        self.cur.push(crate::spline::Node::with(p, i, p, "corner"));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (c1, c2, p) = (self.p(x1, y1), self.p(x2, y2), self.p(x, y));
        if let Some(last) = self.cur.last_mut() {
            last.o = c1;
        }
        self.cur.push(crate::spline::Node::with(p, c2, p, "corner"));
    }
    fn close(&mut self) {
        self.end();
    }
}

/// The text as editable curves (the font's own Bézier outlines), placed like `text_outlines`
pub fn text_curves(t: &TextSpec) -> Vec<crate::spline::Curve> {
    let data = font_bytes(t.font, t.bold, t.italic);
    let Ok(face) = ttf_parser::Face::parse(&data, 0) else { return vec![] };
    let upem = face.units_per_em() as f64;
    let s = t.size / upem;
    let line_h = t.size * 1.2;
    let lines: Vec<&str> = t.text.split('\n').collect();
    let width_of = |l: &str| -> f64 {
        l.chars().map(|ch| face.glyph_index(ch).and_then(|g| face.glyph_hor_advance(g)).unwrap_or((upem * 0.5) as u16) as f64 * s).sum()
    };
    let wmax = lines.iter().map(|l| width_of(l)).fold(0.0, f64::max);
    let mut ol = CurveOutline { curves: vec![], cur: vec![], s, ox: 0.0, oy: 0.0 };
    for (li, line) in lines.iter().enumerate() {
        let lw = width_of(line);
        let mut x = match t.align {
            "center" => (wmax - lw) / 2.0,
            "right" => wmax - lw,
            _ => 0.0,
        };
        let y = -(li as f64) * line_h;
        for ch in line.chars() {
            let Some(g) = face.glyph_index(ch) else {
                x += upem * 0.5 * s;
                continue;
            };
            ol.ox = x;
            ol.oy = y;
            face.outline_glyph(g, &mut ol);
            ol.end();
            x += face.glyph_hor_advance(g).unwrap_or(0) as f64 * s;
        }
    }
    // same placement as the outlines: ink lower-left at 0,0
    let flat: Vec<Contour> = ol.curves.iter().map(|c| c.flatten()).collect();
    let Some(b) = bbox_of(&flat) else { return vec![] };
    let m = translate(-b.x0, -b.y0);
    let mut cs = ol.curves;
    for c in cs.iter_mut() {
        for n in c.nodes.iter_mut() {
            n.transform(&m);
        }
    }
    cs
}

pub struct TextSpec<'a> {
    pub text: &'a str,
    pub font: &'a str,
    pub size: f64, // em size in mm
    pub align: &'a str,
    pub bold: bool,
    pub italic: bool,
}

/// Outlines of the text in local mm (ink lower-left at 0,0); returns (outlines, width, height)
pub fn text_outlines(t: &TextSpec) -> (Vec<Contour>, f64, f64) {
    let data = font_bytes(t.font, t.bold, t.italic);
    let Ok(face) = ttf_parser::Face::parse(&data, 0) else { return (vec![], 0.0, 0.0) };
    let upem = face.units_per_em() as f64;
    let s = t.size / upem;
    let line_h = t.size * 1.2;
    let lines: Vec<&str> = t.text.split('\n').collect();
    let width_of = |l: &str| -> f64 {
        l.chars().map(|ch| face.glyph_index(ch).and_then(|g| face.glyph_hor_advance(g)).unwrap_or((upem * 0.5) as u16) as f64 * s).sum()
    };
    let wmax = lines.iter().map(|l| width_of(l)).fold(0.0, f64::max);
    let mut ol = Outline { cs: vec![], cur: vec![], s, ox: 0.0, oy: 0.0, last: [0.0, 0.0] };
    for (li, line) in lines.iter().enumerate() {
        let lw = width_of(line);
        let mut x = match t.align {
            "center" => (wmax - lw) / 2.0,
            "right" => wmax - lw,
            _ => 0.0,
        };
        let y = -(li as f64) * line_h;
        for ch in line.chars() {
            let Some(g) = face.glyph_index(ch) else {
                x += upem * 0.5 * s;
                continue;
            };
            ol.ox = x;
            ol.oy = y;
            face.outline_glyph(g, &mut ol);
            ol.end();
            x += face.glyph_hor_advance(g).unwrap_or(0) as f64 * s;
        }
    }
    let cs: Vec<Contour> = ol.cs.into_iter().map(|c| Contour { pts: simplify(&c.pts, 0.005), closed: true }).filter(|c| c.pts.len() > 3).collect();
    let Some(b) = bbox_of(&cs) else { return (vec![], 0.0, 0.0) };
    let cs = cs.into_iter().map(|c| Contour { pts: c.pts.iter().map(|p| [p[0] - b.x0, p[1] - b.y0]).collect(), closed: true }).collect();
    (cs, b.w(), b.h())
}

// ---------------------------------------------------------------- SVG
fn cubic(p0: P, p1: P, p2: P, p3: P, out: &mut Vec<P>) {
    let d = |a: P, b: P| ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
    let len = d(p0, p1) + d(p1, p2) + d(p2, p3);
    let n = ((len / 1.5).ceil() as usize).clamp(4, 200);
    for i in 1..=n {
        let t = i as f64 / n as f64;
        let u = 1.0 - t;
        out.push([
            u * u * u * p0[0] + 3.0 * u * u * t * p1[0] + 3.0 * u * t * t * p2[0] + t * t * t * p3[0],
            u * u * u * p0[1] + 3.0 * u * u * t * p1[1] + 3.0 * u * t * t * p2[1] + t * t * t * p3[1],
        ]);
    }
}

#[allow(clippy::too_many_arguments)]
fn arc_to(p0: P, mut rx: f64, mut ry: f64, phi: f64, fa: bool, fs: bool, p1: P, out: &mut Vec<P>) {
    if rx == 0.0 || ry == 0.0 {
        out.push(p1);
        return;
    }
    rx = rx.abs();
    ry = ry.abs();
    let (s, c) = phi.to_radians().sin_cos();
    let (dx, dy) = ((p0[0] - p1[0]) / 2.0, (p0[1] - p1[1]) / 2.0);
    let (x1, y1) = (c * dx + s * dy, -s * dx + c * dy);
    let lam = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
    if lam > 1.0 {
        rx *= lam.sqrt();
        ry *= lam.sqrt();
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let mut co = (num / (rx * rx * y1 * y1 + ry * ry * x1 * x1)).max(0.0).sqrt();
    if co.is_nan() {
        co = 0.0;
    }
    if fa == fs {
        co = -co;
    }
    let (cxp, cyp) = (co * rx * y1 / ry, -co * ry * x1 / rx);
    let (cx, cy) = (c * cxp - s * cyp + (p0[0] + p1[0]) / 2.0, s * cxp + c * cyp + (p0[1] + p1[1]) / 2.0);
    let ang = |ux: f64, uy: f64, vx: f64, vy: f64| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let t1 = ang(1.0, 0.0, (x1 - cxp) / rx, (y1 - cyp) / ry);
    let mut dt = ang((x1 - cxp) / rx, (y1 - cyp) / ry, (-x1 - cxp) / rx, (-y1 - cyp) / ry);
    if !fs && dt > 0.0 {
        dt -= std::f64::consts::TAU;
    } else if fs && dt < 0.0 {
        dt += std::f64::consts::TAU;
    }
    let n = ((dt.abs() * rx.max(ry)).ceil() as usize).max(4);
    for i in 1..=n {
        let t = t1 + dt * i as f64 / n as f64;
        out.push([cx + c * rx * t.cos() - s * ry * t.sin(), cy + s * rx * t.cos() + c * ry * t.sin()]);
    }
}

fn tokens(d: &str) -> Vec<String> {
    let mut out = vec![];
    let b: Vec<char> = d.chars().collect();
    let mut i = 0;
    while i < b.len() {
        let ch = b[i];
        if ch.is_ascii_alphabetic() && ch != 'e' && ch != 'E' {
            out.push(ch.to_string());
            i += 1;
        } else if ch.is_ascii_digit() || ch == '.' || ch == '-' || ch == '+' {
            let st = i;
            let mut seen_dot = false;
            let mut seen_e = false;
            if ch == '-' || ch == '+' {
                i += 1;
            }
            while i < b.len() {
                let c = b[i];
                if c.is_ascii_digit() {
                    i += 1;
                } else if c == '.' && !seen_dot && !seen_e {
                    seen_dot = true;
                    i += 1;
                } else if (c == 'e' || c == 'E') && !seen_e {
                    seen_e = true;
                    i += 1;
                    if i < b.len() && (b[i] == '-' || b[i] == '+') {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            out.push(b[st..i].iter().collect());
        } else {
            i += 1;
        }
    }
    out
}

pub fn parse_path_d(d: &str) -> Vec<Contour> {
    let t = tokens(d);
    let mut subs: Vec<Contour> = vec![];
    let mut cur: Option<usize> = None;
    let mut i = 0;
    let mut cmd = 'M';
    let (mut p, mut start): (P, P) = ([0.0, 0.0], [0.0, 0.0]);
    let (mut last_c, mut last_q): (Option<P>, Option<P>) = (None, None);
    let is_cmd = |s: &str| s.len() == 1 && s.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false);
    while i < t.len() {
        if is_cmd(&t[i]) {
            cmd = t[i].chars().next().unwrap();
            i += 1;
        }
        let rel = cmd.is_ascii_lowercase();
        let up = cmd.to_ascii_uppercase();
        let (ox, oy) = if rel { (p[0], p[1]) } else { (0.0, 0.0) };
        if up == 'Z' {
            if let Some(c) = cur {
                subs[c].closed = true;
                subs[c].pts.push(start);
            }
            p = start;
            cur = None;
            last_c = None;
            last_q = None;
            continue;
        }
        if i >= t.len() || is_cmd(&t[i]) {
            i += 1;
            continue;
        }
        let mut num = || -> f64 {
            let v = t.get(i).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
            i += 1;
            v
        };
        if cur.is_none() && up != 'M' {
            subs.push(Contour { pts: vec![p], closed: false });
            cur = Some(subs.len() - 1);
        }
        match up {
            'M' => {
                p = [ox + num(), oy + num()];
                start = p;
                subs.push(Contour { pts: vec![p], closed: false });
                cur = Some(subs.len() - 1);
                cmd = if rel { 'l' } else { 'L' };
                last_c = None;
                last_q = None;
            }
            'L' => {
                p = [ox + num(), oy + num()];
                subs[cur.unwrap()].pts.push(p);
                last_c = None;
                last_q = None;
            }
            'H' => {
                p = [ox + num(), p[1]];
                subs[cur.unwrap()].pts.push(p);
                last_c = None;
                last_q = None;
            }
            'V' => {
                p = [p[0], oy + num()];
                subs[cur.unwrap()].pts.push(p);
                last_c = None;
                last_q = None;
            }
            'C' | 'S' => {
                let c1 = if up == 'C' {
                    [ox + num(), oy + num()]
                } else {
                    last_c.map(|lc| [2.0 * p[0] - lc[0], 2.0 * p[1] - lc[1]]).unwrap_or(p)
                };
                let c2 = [ox + num(), oy + num()];
                let e = [ox + num(), oy + num()];
                cubic(p, c1, c2, e, &mut subs[cur.unwrap()].pts);
                last_c = Some(c2);
                last_q = None;
                p = e;
            }
            'Q' | 'T' => {
                let q = if up == 'Q' {
                    [ox + num(), oy + num()]
                } else {
                    last_q.map(|lq| [2.0 * p[0] - lq[0], 2.0 * p[1] - lq[1]]).unwrap_or(p)
                };
                let e = [ox + num(), oy + num()];
                let c1 = [p[0] + 2.0 / 3.0 * (q[0] - p[0]), p[1] + 2.0 / 3.0 * (q[1] - p[1])];
                let c2 = [e[0] + 2.0 / 3.0 * (q[0] - e[0]), e[1] + 2.0 / 3.0 * (q[1] - e[1])];
                cubic(p, c1, c2, e, &mut subs[cur.unwrap()].pts);
                last_q = Some(q);
                last_c = None;
                p = e;
            }
            'A' => {
                let (rx, ry, phi, fa, fs) = (num(), num(), num(), num(), num());
                let e = [ox + num(), oy + num()];
                arc_to(p, rx, ry, phi, fa != 0.0, fs != 0.0, e, &mut subs[cur.unwrap()].pts);
                p = e;
                last_c = None;
                last_q = None;
            }
            _ => {
                i += 1;
            }
        }
    }
    subs.into_iter().filter(|s| s.pts.len() > 1).collect()
}

fn parse_transform(s: &str) -> M {
    let mut m = IDENT;
    let mut rest = s;
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim().trim_start_matches(',').trim();
        let Some(close) = rest[open..].find(')') else { break };
        let args: Vec<f64> = rest[open + 1..open + close].split(|c: char| c == ',' || c.is_whitespace()).filter(|x| !x.is_empty()).filter_map(|x| x.parse().ok()).collect();
        let a = |k: usize| args.get(k).copied().unwrap_or(0.0);
        let t: M = match name {
            "matrix" if args.len() >= 6 => [a(0), a(1), a(2), a(3), a(4), a(5)],
            "translate" => [1.0, 0.0, 0.0, 1.0, a(0), a(1)],
            "scale" => [a(0), 0.0, 0.0, args.get(1).copied().unwrap_or(a(0)), 0.0, 0.0],
            "rotate" => {
                let (sn, cs) = a(0).to_radians().sin_cos();
                mul(&mul(&translate(a(1), a(2)), &[cs, sn, -sn, cs, 0.0, 0.0]), &translate(-a(1), -a(2)))
            }
            "skewX" => [1.0, 0.0, a(0).to_radians().tan(), 1.0, 0.0, 0.0],
            "skewY" => [1.0, a(0).to_radians().tan(), 0.0, 1.0, 0.0, 0.0],
            _ => IDENT,
        };
        m = mul(&m, &t);
        rest = &rest[open + close + 1..];
    }
    m
}

fn unit_to_mm(v: Option<&str>) -> Option<f64> {
    let v = v?.trim();
    let split = v.find(|c: char| c.is_ascii_alphabetic() || c == '%').unwrap_or(v.len());
    let n: f64 = v[..split].trim().parse().ok()?;
    let k = match v[split..].trim() {
        "mm" => 1.0,
        "cm" => 10.0,
        "in" => 25.4,
        "pt" => 25.4 / 72.0,
        "pc" => 25.4 / 6.0,
        "" | "px" => 25.4 / 96.0,
        _ => return None,
    };
    Some(n * k)
}

pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().to_lowercase();
    if s.is_empty() || s == "none" || s.starts_with("url(") {
        return None;
    }
    if let Some(h) = s.strip_prefix('#') {
        let h: String = if h.len() == 3 { h.chars().flat_map(|c| [c, c]).collect() } else { h.to_string() };
        if h.len() >= 6 {
            let v = |k: usize| u8::from_str_radix(&h[k..k + 2], 16).ok();
            return Some([v(0)?, v(2)?, v(4)?]);
        }
        return None;
    }
    if s.starts_with("rgb") {
        let nums: Vec<f64> = s.trim_start_matches(|c: char| c != '(').trim_matches(|c| c == '(' || c == ')').split(',').filter_map(|x| x.trim().trim_end_matches('%').parse().ok()).collect();
        if nums.len() >= 3 {
            let pct = s.contains('%');
            let f = |v: f64| (if pct { v * 2.55 } else { v }).clamp(0.0, 255.0) as u8;
            return Some([f(nums[0]), f(nums[1]), f(nums[2])]);
        }
    }
    Some(match s.as_str() {
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" => [0, 128, 0],
        "lime" => [0, 255, 0],
        "blue" => [0, 0, 255],
        "yellow" => [255, 255, 0],
        "cyan" | "aqua" => [0, 255, 255],
        "magenta" | "fuchsia" => [255, 0, 255],
        "orange" => [255, 165, 0],
        "purple" => [128, 0, 128],
        "gray" | "grey" => [128, 128, 128],
        "navy" => [0, 0, 128],
        "brown" => [165, 42, 42],
        "pink" => [255, 192, 203],
        "teal" => [0, 128, 128],
        "olive" => [128, 128, 0],
        "maroon" => [128, 0, 0],
        _ => return None,
    })
}

pub struct SvgItem {
    pub subpaths: Vec<Contour>, // world mm, Y up
    pub color: Option<[u8; 3]>,
}

pub fn import_svg(text: &str) -> Result<Vec<SvgItem>, String> {
    let opt = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(text, opt).map_err(|e| format!("Not a readable SVG drawing ({e})"))?;
    let svg = doc.root_element();
    if svg.tag_name().name() != "svg" {
        return Err("Not an SVG drawing".into());
    }
    let vb: Vec<f64> = svg.attribute("viewBox").unwrap_or("").split(|c: char| c == ',' || c.is_whitespace()).filter(|x| !x.is_empty()).filter_map(|x| x.parse().ok()).collect();
    let wmm = unit_to_mm(svg.attribute("width"));
    let hmm = unit_to_mm(svg.attribute("height"));
    let (root, height_mm) = if vb.len() == 4 && vb[2] > 0.0 {
        let sx = wmm.map(|w| w / vb[2]).unwrap_or(25.4 / 96.0);
        let sy = hmm.map(|h| h / vb[3]).unwrap_or(sx);
        ([sx, 0.0, 0.0, sy, -vb[0] * sx, -vb[1] * sy], hmm.unwrap_or(vb[3] * sy))
    } else {
        let s = 25.4 / 96.0;
        ([s, 0.0, 0.0, s, 0.0, 0.0], hmm.unwrap_or(0.0))
    };
    let mut out = vec![];
    #[derive(Clone, Default)]
    struct St {
        stroke: Option<String>,
        fill: Option<String>,
    }
    fn style_get(n: &roxmltree::Node, k: &str) -> Option<String> {
        if let Some(css) = n.attribute("style") {
            for part in css.split(';') {
                if let Some((a, b)) = part.split_once(':') {
                    if a.trim() == k {
                        return Some(b.trim().to_string());
                    }
                }
            }
        }
        n.attribute(k).map(|s| s.to_string())
    }
    fn walk(n: roxmltree::Node, m: M, st: St, root: &M, hmm: f64, out: &mut Vec<SvgItem>) {
        let tag = n.tag_name().name();
        if ["defs", "clipPath", "mask", "symbol", "style", "title", "desc", "metadata", "pattern", "marker"].contains(&tag) {
            return;
        }
        if style_get(&n, "display").as_deref() == Some("none") {
            return;
        }
        let mm = mul(&m, &parse_transform(n.attribute("transform").unwrap_or("")));
        let mut st = st;
        if let Some(s) = style_get(&n, "stroke") {
            st.stroke = Some(s);
        }
        if let Some(f) = style_get(&n, "fill") {
            st.fill = Some(f);
        }
        let num = |k: &str| n.attribute(k).and_then(|v| v.trim_end_matches("px").parse::<f64>().ok()).unwrap_or(0.0);
        let subs: Vec<Contour> = match tag {
            "path" => parse_path_d(n.attribute("d").unwrap_or("")),
            "rect" => {
                let (x, y, w, h) = (num("x"), num("y"), num("width"), num("height"));
                let mut rx = if num("rx") > 0.0 { num("rx") } else { num("ry") };
                let mut ry = if num("ry") > 0.0 { num("ry") } else { rx };
                rx = rx.min(w / 2.0);
                ry = ry.min(h / 2.0);
                let d = if rx > 0.0 {
                    format!("M{},{}H{}A{},{} 0 0 1 {},{}V{}A{},{} 0 0 1 {},{}H{}A{},{} 0 0 1 {},{}V{}A{},{} 0 0 1 {},{}Z",
                        x + rx, y, x + w - rx, rx, ry, x + w, y + ry, y + h - ry, rx, ry, x + w - rx, y + h, x + rx, rx, ry, x, y + h - ry, y + ry, rx, ry, x + rx, y)
                } else {
                    format!("M{},{}H{}V{}H{}Z", x, y, x + w, y + h, x)
                };
                parse_path_d(&d)
            }
            "circle" | "ellipse" => {
                let (cx, cy) = (num("cx"), num("cy"));
                let (rx, ry) = if tag == "circle" { (num("r"), num("r")) } else { (num("rx"), num("ry")) };
                parse_path_d(&format!("M{},{}A{},{} 0 1 1 {},{}A{},{} 0 1 1 {},{}Z", cx + rx, cy, rx, ry, cx - rx, cy, rx, ry, cx + rx, cy))
            }
            "line" => vec![Contour { pts: vec![[num("x1"), num("y1")], [num("x2"), num("y2")]], closed: false }],
            "polyline" | "polygon" => {
                let v: Vec<f64> = n.attribute("points").unwrap_or("").split(|c: char| c == ',' || c.is_whitespace()).filter(|x| !x.is_empty()).filter_map(|x| x.parse().ok()).collect();
                let mut pts: Vec<P> = v.chunks(2).filter(|c| c.len() == 2).map(|c| [c[0], c[1]]).collect();
                if tag == "polygon" && !pts.is_empty() {
                    pts.push(pts[0]);
                }
                vec![Contour { pts, closed: tag == "polygon" }]
            }
            _ => vec![],
        };
        if !subs.is_empty() {
            let full = mul(root, &mm);
            let color = st.stroke.as_deref().and_then(parse_color).or_else(|| st.fill.as_deref().and_then(parse_color));
            out.push(SvgItem {
                subpaths: subs
                    .into_iter()
                    .map(|c| Contour {
                        closed: c.closed,
                        pts: c.pts.iter().map(|p| {
                            let q = apply(&full, *p);
                            [q[0], hmm - q[1]]
                        }).collect(),
                    })
                    .filter(|c| c.pts.len() > 1)
                    .collect(),
                color,
            });
        }
        for ch in n.children().filter(|c| c.is_element()) {
            walk(ch, mm, st.clone(), root, hmm, out);
        }
    }
    walk(svg, IDENT, St::default(), &root, height_mm, &mut out);
    out.retain(|i| !i.subpaths.is_empty());
    Ok(out)
}

// ---------------------------------------------------------------- pictures
pub struct Gray {
    pub w: usize,
    pub h: usize,
    pub gray: Vec<f32>, // 0 = black, 1 = white
}

pub fn decode_gray(bytes: &[u8]) -> Result<Gray, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("Could not read picture: {e}"))?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    let mut gray = Vec::with_capacity(w * h);
    for p in rgba.pixels() {
        let a = p[3] as f32 / 255.0;
        let l = (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) / 255.0;
        gray.push(l * a + (1.0 - a));
    }
    Ok(Gray { w, h, gray })
}

pub fn data_url(bytes: &[u8], path: &Path) -> String {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("png").to_lowercase();
    let mime = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        _ => "image/png",
    };
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

pub fn data_url_bytes(src: &str) -> Option<Vec<u8>> {
    let (_, b64) = src.split_once(',')?;
    base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn svg_sizes_and_colors() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="60mm" height="40mm" viewBox="0 0 60 40"><g transform="translate(5,5)"><rect x="0" y="0" width="20" height="10" stroke="#ff0000" fill="none"/><circle cx="40" cy="15" r="8" stroke="blue" fill="none"/></g></svg>"##;
        let items = import_svg(svg).unwrap();
        assert_eq!(items.len(), 2);
        let b = bbox_of(&items[0].subpaths).unwrap();
        assert!((b.w() - 20.0).abs() < 1e-6 && (b.h() - 10.0).abs() < 1e-6);
        assert!((b.y1 - 35.0).abs() < 1e-6, "y flipped from the top: {:?}", b);
        assert_eq!(items[0].color, Some([255, 0, 0]));
        let c = bbox_of(&items[1].subpaths).unwrap();
        assert!((c.w() - 16.0).abs() < 0.05);
    }
    #[test]
    fn text_has_outlines() {
        let (cs, w, h) = text_outlines(&TextSpec { text: "Hi", font: "Sans", size: 10.0, align: "left", bold: false, italic: false });
        assert!(cs.len() >= 3, "H and i give at least 3 outlines");
        assert!(w > 5.0 && h > 6.0 && h < 11.0, "{w} x {h}");
    }
}
