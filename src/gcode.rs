//! Turns the design into a laser job (G-code for GRBL-type controllers).
//! Rules come from MECHANICS.md (read from the original program).

use crate::geom::*;
use crate::importers::Gray;
use crate::model::{Doc, Layer, Shape};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

pub const APP_NAME: &str = "CNC Anything";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
/// short git commit id the program was built from ("-modified" when built from uncommitted changes)
pub const GIT_COMMIT: &str = env!("GIT_COMMIT");
/// "0.1.0 (abc1234)"
pub fn version_label() -> String {
    format!("{APP_VERSION} ({GIT_COMMIT})")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Device {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String, // grbl | grbl-m3
    pub bed_w: f64,
    pub bed_h: f64,
    pub s_max: f64,
    pub baud: u32,
    pub buffer_size: usize,
    pub rapid: f64,
    pub strip_leading_zero: bool,
    pub use_g0_overscan: bool,
    pub auto_home: bool,
    pub finish_x: f64,
    pub finish_y: f64,
    pub return_to_finish: bool,
    /// (0,0) at the upper-left corner, Y grows toward the front
    pub origin_top: bool,
    pub port: String,
    /// "laser" or "router"
    pub machine: String,
    /// router: height for moves between cuts, mm above the stock top
    pub safe_z: f64,
    /// router: seconds to wait after starting the spindle
    pub spindle_delay: f64,
    /// router: stock thickness, mm (used to warn about cutting too deep)
    pub stock: f64,
    /// router: pause for a tool change when the tool changes between layers
    pub tool_change_pause: bool,
}
impl Default for Device {
    fn default() -> Self {
        Device {
            name: "My laser".into(),
            kind: "grbl".into(),
            bed_w: 400.0,
            bed_h: 400.0,
            s_max: 1000.0,
            baud: 115200,
            buffer_size: 127,
            rapid: 100.0,
            strip_leading_zero: false,
            use_g0_overscan: false,
            auto_home: false,
            finish_x: 0.0,
            finish_y: 0.0,
            return_to_finish: true,
            origin_top: true,
            port: String::new(),
            machine: "laser".into(),
            safe_z: 5.0,
            spindle_delay: 2.0,
            stock: 18.0,
            tool_change_pause: true,
        }
    }
}
impl Device {
    pub fn router(&self) -> bool {
        self.machine == "router"
    }
    /// world (Y up from the front) <-> what the person sees (counted from the origin corner)
    pub fn to_user(&self, p: P) -> P {
        [p[0], if self.origin_top { self.bed_h - p[1] } else { p[1] }]
    }
    pub fn from_user(&self, p: P) -> P {
        self.to_user(p)
    }
}

pub type ImgCache = HashMap<usize, Gray>;
pub fn img_key(src: &Arc<str>) -> usize {
    Arc::as_ptr(src) as *const u8 as usize
}

/// Number format (sub_4166f0): round to N decimals, drop trailing zeros
pub fn fmt(v: f64, dec: i32, strip_lead: bool) -> String {
    let p = 10f64.powi(dec);
    let mut n = (v * p).round() / p;
    if n.abs() < 0.5 / p {
        n = 0.0;
    }
    let mut s = format!("{:.*}", dec as usize, n);
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if s == "-0" {
        s = "0".into();
    }
    if strip_lead {
        if let Some(r) = s.strip_prefix("0.") {
            s = format!(".{r}");
        } else if let Some(r) = s.strip_prefix("-0.") {
            s = format!("-.{r}");
        }
    }
    s
}

// ---------------------------------------------------------------- dithering
const BAYER: [f32; 16] = [0., 8., 2., 10., 12., 4., 14., 6., 3., 11., 1., 9., 15., 7., 13., 5.];
fn kernel(mode: &str) -> Vec<(i32, i32, f32)> {
    let k = |v: &[(i32, i32, f32)], d: f32| v.iter().map(|&(a, b, c)| (a, b, c / d)).collect::<Vec<_>>();
    match mode {
        "floyd" => k(&[(1, 0, 7.), (-1, 1, 3.), (0, 1, 5.), (1, 1, 1.)], 16.),
        "stucki" => k(&[(1, 0, 8.), (2, 0, 4.), (-2, 1, 2.), (-1, 1, 4.), (0, 1, 8.), (1, 1, 4.), (2, 1, 2.), (-2, 2, 1.), (-1, 2, 2.), (0, 2, 4.), (1, 2, 2.), (2, 2, 1.)], 42.),
        "atkinson" => k(&[(1, 0, 1.), (2, 0, 1.), (-1, 1, 1.), (0, 1, 1.), (1, 1, 1.), (0, 2, 1.)], 8.),
        _ => k(&[(1, 0, 7.), (2, 0, 5.), (-2, 1, 3.), (-1, 1, 5.), (0, 1, 7.), (1, 1, 5.), (2, 1, 3.), (-2, 2, 1.), (-1, 2, 3.), (0, 2, 5.), (1, 2, 3.), (2, 2, 1.)], 48.),
    }
}
/// darkness grid (1 = burn fully) -> burn amount per cell
pub fn dither(grid: &[f32], w: usize, h: usize, mode: &str) -> Vec<f32> {
    match mode {
        "grayscale" => grid.to_vec(),
        "threshold" => grid.iter().map(|&v| if v >= 0.5 { 1.0 } else { 0.0 }).collect(),
        "ordered" => (0..w * h).map(|i| if grid[i] > (BAYER[(i / w & 3) * 4 + (i % w & 3)] + 0.5) / 16.0 { 1.0 } else { 0.0 }).collect(),
        _ => {
            let k = kernel(mode);
            let mut g = grid.to_vec();
            let mut out = vec![0.0; w * h];
            for y in 0..h {
                for x in 0..w {
                    let i = y * w + x;
                    let v = g[i];
                    let q = if v >= 0.5 { 1.0 } else { 0.0 };
                    out[i] = q;
                    let e = v - q;
                    for &(dx, dy, f) in &k {
                        let (xx, yy) = (x as i32 + dx, y as i32 + dy);
                        if xx >= 0 && (xx as usize) < w && (yy as usize) < h {
                            g[yy as usize * w + xx as usize] += e * f;
                        }
                    }
                }
            }
            out
        }
    }
}

pub struct Raster {
    pub b: BBox,
    pub w: usize,
    pub h: usize,
    pub burn: Vec<f32>,
}
/// Sample picture shapes onto a world grid with `interval` spacing (rows bottom to top)
pub fn raster_images(shapes: &[&Shape], cache: &ImgCache, interval: f64, layer: &Layer) -> Result<Option<Raster>, String> {
    let Some(b) = shapes.iter().fold(None, |a, s| BBox::union(a, s.bbox())) else { return Ok(None) };
    let w = ((b.w() / interval).round() as usize).max(1);
    let h = ((b.h() / interval).round() as usize).max(1);
    if w * h > 25_000_000 {
        return Err("Picture too large for this line interval. Increase the interval or shrink the picture.".into());
    }
    let mut grid = vec![0f32; w * h];
    let mut mask = vec![false; w * h];
    for s in shapes {
        let Some(im) = cache.get(&img_key(&s.src)) else { continue };
        let iv = inv(&s.tf);
        let Some(sb) = s.bbox() else { continue };
        let gx0 = ((sb.x0 - b.x0) / interval).floor().max(0.0) as usize;
        let gx1 = (((sb.x1 - b.x0) / interval).ceil() as usize).min(w);
        let gy0 = ((sb.y0 - b.y0) / interval).floor().max(0.0) as usize;
        let gy1 = (((sb.y1 - b.y0) / interval).ceil() as usize).min(h);
        for gy in gy0..gy1 {
            for gx in gx0..gx1 {
                let wp = [b.x0 + (gx as f64 + 0.5) * interval, b.y0 + (gy as f64 + 0.5) * interval];
                let [u, v] = apply(&iv, wp);
                if u < 0.0 || v < 0.0 || u >= s.w || v >= s.h {
                    continue;
                }
                let px = ((u / s.w * im.w as f64) as usize).min(im.w - 1);
                let py = (((s.h - v) / s.h * im.h as f64) as usize).min(im.h - 1);
                let mut dark = 1.0 - im.gray[py * im.w + px];
                if s.invert != layer.invert {
                    dark = 1.0 - dark;
                }
                grid[gy * w + gx] = dark;
                mask[gy * w + gx] = true;
            }
        }
    }
    let mut burn = dither(&grid, w, h, &layer.dither);
    for i in 0..burn.len() {
        if !mask[i] {
            burn[i] = 0.0;
        }
    }
    Ok(Some(Raster { b, w, h, burn }))
}

// ---------------------------------------------------------------- planning
#[derive(Clone)]
pub enum Op {
    Layer(Layer),
    Rapid(f64, f64),
    Cut { x: f64, y: f64, p: f64, f: f64 },
}

pub struct PlanOpts {
    pub selected_only: bool,
    pub inner_first: bool,
    pub start: P,
}

pub fn plan_job(doc: &mut Doc, cache: &ImgCache, o: &PlanOpts) -> Result<(Vec<Op>, Vec<String>), String> {
    let mut ops = vec![];
    let mut warnings = vec![];
    let mut pos = o.start;
    for li in doc.used_layers() {
        let l = doc.layers[li].clone();
        if !l.output {
            continue;
        }
        let shapes: Vec<&Shape> = doc.shapes.iter().filter(|s| s.layer == li && (!o.selected_only || doc.sel.contains(&s.id))).collect();
        if shapes.is_empty() {
            continue;
        }
        ops.push(Op::Layer(l.clone()));
        let vecs: Vec<&Shape> = shapes.iter().copied().filter(|s| s.kind != "image").collect();
        let imgs: Vec<&Shape> = shapes.iter().copied().filter(|s| s.kind == "image").collect();
        if !imgs.is_empty() && l.mode != "Image" {
            warnings.push(format!("Pictures on layer {} only burn when that layer is in Image mode.", l.name));
        }
        let rapid = |ops: &mut Vec<Op>, pos: &mut P, x: f64, y: f64| {
            ops.push(Op::Rapid(x, y));
            *pos = [x, y];
        };
        let cut = |ops: &mut Vec<Op>, pos: &mut P, x: f64, y: f64, p: f64, f: f64| {
            ops.push(Op::Cut { x, y, p, f });
            *pos = [x, y];
        };
        for _pass in 0..l.passes.max(1) {
            if l.mode == "Line" {
                let cs: Vec<Contour> = vecs.iter().flat_map(|s| s.world_contours()).filter(|c| c.pts.len() > 1).collect();
                let depth = if o.inner_first { containment_depth(&cs) } else { vec![0; cs.len()] };
                let mut left: Vec<(Contour, usize)> = cs.into_iter().zip(depth).collect();
                while !left.is_empty() {
                    let max_d = left.iter().map(|x| x.1).max().unwrap_or(0);
                    let (mut bi, mut bd, mut rev) = (0, f64::INFINITY, false);
                    for (i, (c, d)) in left.iter().enumerate() {
                        if *d != max_d {
                            continue;
                        }
                        let a = c.pts[0];
                        let b = *c.pts.last().unwrap();
                        let da = (a[0] - pos[0]).hypot(a[1] - pos[1]);
                        if da < bd {
                            bd = da;
                            bi = i;
                            rev = false;
                        }
                        if !c.closed {
                            let db = (b[0] - pos[0]).hypot(b[1] - pos[1]);
                            if db < bd {
                                bd = db;
                                bi = i;
                                rev = true;
                            }
                        }
                    }
                    let (c, _) = left.remove(bi);
                    let pts: Vec<P> = if rev { c.pts.iter().rev().copied().collect() } else { c.pts };
                    rapid(&mut ops, &mut pos, pts[0][0], pts[0][1]);
                    for p in &pts[1..] {
                        cut(&mut ops, &mut pos, p[0], p[1], l.power, l.speed);
                    }
                }
            } else {
                let over = l.speed * l.overscan.max(0.0) / 100.0;
                let fill: Vec<Contour> = vecs.iter().flat_map(|s| s.world_contours()).filter(|c| c.closed).collect();
                if !fill.is_empty() {
                    let mut passes = vec![(scan_fill(&fill, l.interval, false), false)];
                    if l.cross_hatch && l.mode == "Fill" {
                        passes.push((scan_fill(&fill, l.interval, true), true));
                    }
                    for (rows, vertical) in passes {
                        let pt = |a: f64, b: f64| if vertical { [b, a] } else { [a, b] };
                        let mut dir = 1.0;
                        for row in rows {
                            let segs: Vec<(f64, f64)> = if dir > 0.0 { row.segs.clone() } else { row.segs.iter().rev().map(|&(a, b)| (b, a)).collect() };
                            let s0 = segs[0].0;
                            let s_end = segs[segs.len() - 1].1;
                            let r = pt(s0 - dir * over, row.y);
                            rapid(&mut ops, &mut pos, r[0], r[1]);
                            for (a, b) in segs {
                                let pa = pt(a, row.y);
                                if (pa[0] - pos[0]).hypot(pa[1] - pos[1]) > 1e-6 {
                                    cut(&mut ops, &mut pos, pa[0], pa[1], 0.0, l.speed);
                                }
                                let pb = pt(b, row.y);
                                cut(&mut ops, &mut pos, pb[0], pb[1], l.power, l.speed);
                            }
                            if over > 0.0 {
                                let e = pt(s_end + dir * over, row.y);
                                cut(&mut ops, &mut pos, e[0], e[1], 0.0, l.speed);
                            }
                            if l.bidir {
                                dir = -dir;
                            }
                        }
                    }
                }
                if l.mode == "Image" && !imgs.is_empty() {
                    if let Some(r) = raster_images(&imgs, cache, l.interval, &l)? {
                        let gray = l.dither == "grayscale";
                        let mut dir = 1.0;
                        for gy in 0..r.h {
                            let y = r.b.y0 + (gy as f64 + 0.5) * l.interval;
                            let mut spans: Vec<(f64, f64, f64)> = vec![];
                            for gx in 0..r.w {
                                let v = r.burn[gy * r.w + gx] as f64;
                                if v <= 0.004 {
                                    continue;
                                }
                                let p = if gray { l.min_power + v * (l.power - l.min_power) } else { l.power };
                                let x0 = r.b.x0 + gx as f64 * l.interval;
                                let x1 = x0 + l.interval;
                                if let Some(last) = spans.last_mut() {
                                    if (last.1 - x0).abs() < 1e-9 && (last.2 - p).abs() < 0.5 {
                                        last.1 = x1;
                                        continue;
                                    }
                                }
                                spans.push((x0, x1, p));
                            }
                            if spans.is_empty() {
                                continue;
                            }
                            let seq: Vec<(f64, f64, f64)> = if dir > 0.0 { spans } else { spans.iter().rev().map(|&(a, b, p)| (b, a, p)).collect() };
                            let first = seq[0].0;
                            let last_x = seq[seq.len() - 1].1;
                            rapid(&mut ops, &mut pos, first - dir * over, y);
                            for (a, b, p) in seq {
                                if (a - pos[0]).abs() > 1e-6 {
                                    cut(&mut ops, &mut pos, a, y, 0.0, l.speed);
                                }
                                cut(&mut ops, &mut pos, b, y, p, l.speed);
                            }
                            if over > 0.0 {
                                cut(&mut ops, &mut pos, last_x + dir * over, y, 0.0, l.speed);
                            }
                            if l.bidir {
                                dir = -dir;
                            }
                        }
                    }
                }
            }
        }
    }
    Ok((ops, warnings))
}

// ---------------------------------------------------------------- emitting
#[derive(Clone, Copy, PartialEq)]
pub enum StartFrom {
    Absolute,
    User,
    Current,
}

/// One preview move: x0, y0, x1, y1 (world), power %, layer index
pub type PreviewSeg = (f64, f64, f64, f64, f64, usize);

pub struct Job {
    pub text: String,
    pub lines: usize,
    pub preview: Vec<PreviewSeg>,
    pub time: f64,
    pub cut_len: f64,
    pub travel: f64,
}

pub fn emit_gcode(ops: &[Op], dev: &Device, start_from: StartFrom, origin: P, start: P) -> Job {
    let rel = start_from == StartFrom::Current;
    let sl = dev.strip_leading_zero;
    let n3 = |v: f64| fmt(v, 3, sl);
    let (flip, hh) = (dev.origin_top, dev.bed_h);
    let to_out = |x: f64, y: f64| -> P {
        if start_from == StartFrom::Absolute {
            [x, if flip { hh - y } else { y }]
        } else {
            [x - origin[0], if flip { origin[1] - y } else { y - origin[1] }]
        }
    };
    let mut lines: Vec<String> = vec![];
    let mut bx: Option<BBox> = None;
    for o in ops {
        if let Op::Cut { x, y, p, .. } = o {
            if *p > 0.0 {
                bx = BBox::add_point(bx, [*x, *y]);
            }
        }
    }
    lines.push(format!("; {APP_NAME} {APP_VERSION}"));
    lines.push(format!(
        "; {} device profile, {}",
        if dev.kind == "grbl-m3" { "GRBL-M3" } else { "GRBL" },
        match start_from {
            StartFrom::Absolute => "absolute coords",
            StartFrom::User => "user origin",
            StartFrom::Current => "current position",
        }
    ));
    if let Some(b) = bx {
        let (a, c) = (to_out(b.x0, b.y0), to_out(b.x1, b.y1));
        lines.push(format!("; Bounds: X{} Y{} to X{} Y{}", n3(a[0].min(c[0])), n3(a[1].min(c[1])), n3(a[0].max(c[0])), n3(a[1].max(c[1]))));
    }
    lines.push("G00 G17 G40 G21 G54".into());
    lines.push(if rel { "G91".into() } else { "G90".into() });
    lines.push(if dev.kind == "grbl-m3" { "M3".into() } else { "M4".into() });

    let mut mode = -1;
    let (mut lx, mut ly): (Option<f64>, Option<f64>) = if rel { (Some(0.0), Some(0.0)) } else { (None, None) };
    let (mut lf, mut ls): (Option<f64>, Option<f64>) = (None, None);
    let (mut wx, mut wy) = (start[0], start[1]);
    let (mut time, mut cut_len, mut travel) = (0.0, 0.0, 0.0);
    let mut air_on = false;
    let mut cur_layer = 0;
    let mut preview = vec![];
    const EPS: f64 = 0.01; // 0x13742bc: axis words are skipped when they move less than this

    let mut mv = |lines: &mut Vec<String>, g: i32, x: f64, y: f64, s: f64, f: f64| {
        let o = to_out(x, y);
        let (tx, ty) = if rel { (o[0] - lx.unwrap(), o[1] - ly.unwrap()) } else { (o[0], o[1]) };
        let need_x = if rel { tx.abs() >= 0.005 } else { lx.map(|v| (tx - v).abs() >= EPS).unwrap_or(true) };
        let need_y = if rel { ty.abs() >= 0.005 } else { ly.map(|v| (ty - v).abs() >= EPS).unwrap_or(true) };
        if !need_x && !need_y && g == 0 {
            return;
        }
        let mut line = String::new();
        if g != mode {
            line += if g == 0 { "G0" } else { "G1" };
            mode = g;
        }
        let (sx, sy) = (n3(tx), n3(ty));
        if need_x {
            line += &format!("X{sx}");
        }
        if need_y {
            line += &format!("Y{sy}");
        }
        if g == 1 {
            if ls.map(|v| (s - v).abs() > 1e-6).unwrap_or(true) {
                line += &format!("S{}", fmt(s, 1, false));
                ls = Some(s);
            }
            let fm = f * 60.0; // mm/s -> mm/min (sub_4176c0)
            if lf.map(|v| (fm - v).abs() > EPS).unwrap_or(true) {
                line += &format!("F{}", fmt(fm, 2, sl));
                lf = Some(fm);
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
        let parse = |s: &str| s.parse::<f64>().unwrap_or(0.0);
        if rel {
            if need_x {
                lx = Some(lx.unwrap() + parse(&sx));
            }
            if need_y {
                ly = Some(ly.unwrap() + parse(&sy));
            }
        } else {
            if need_x {
                lx = Some(tx);
            }
            if need_y {
                ly = Some(ty);
            }
        }
    };

    for o in ops {
        match o {
            Op::Layer(l) => {
                cur_layer = l.index;
                lines.push(format!("; {} @ {} mm/min, {}% power", l.name, fmt(l.speed * 60.0, 3, false), fmt(l.power, 1, false)));
                if l.air && !air_on {
                    lines.push("M8".into());
                    air_on = true;
                } else if !l.air && air_on {
                    lines.push("M9".into());
                    air_on = false;
                }
            }
            Op::Rapid(x, y) => {
                let d = (x - wx).hypot(y - wy);
                mv(&mut lines, 0, *x, *y, 0.0, 0.0);
                time += d / dev.rapid.max(1.0);
                travel += d;
                preview.push((wx, wy, *x, *y, 0.0, cur_layer));
                wx = *x;
                wy = *y;
            }
            Op::Cut { x, y, p, f } => {
                let d = (x - wx).hypot(y - wy);
                let s = p / 100.0 * dev.s_max;
                mv(&mut lines, 1, *x, *y, s, *f);
                time += d / f.max(0.1);
                if *p > 0.0 {
                    cut_len += d;
                }
                preview.push((wx, wy, *x, *y, *p, cur_layer));
                wx = *x;
                wy = *y;
            }
        }
    }
    if air_on {
        lines.push("M9".into());
    }
    lines.push("G1 S0".into());
    lines.push("M5".into());
    if rel {
        lines.push("; return to starting pos".into());
        lines.push(format!("G0 X{} Y{}", n3(-lx.unwrap()), n3(-ly.unwrap())));
        lines.push("G90".into());
        time += lx.unwrap().hypot(ly.unwrap()) / dev.rapid.max(1.0);
    } else {
        lines.push("G90".into());
        if dev.return_to_finish {
            lines.push("; return to user-defined finish pos".into());
            let (fx, fy) = if start_from == StartFrom::Absolute { (dev.finish_x, dev.finish_y) } else { (0.0, 0.0) };
            lines.push(format!("G0 X{} Y{}", n3(fx), n3(fy)));
            let o = to_out(wx, wy);
            time += (o[0] - fx).hypot(o[1] - fy) / dev.rapid.max(1.0);
        }
    }
    lines.push("M2".into());
    let n = lines.len();
    Job { text: lines.join("\n") + "\n", lines: n, preview, time, cut_len, travel }
}

pub fn fmt_time(sec: f64) -> String {
    let sec = sec.round() as u64;
    let (h, m, s) = (sec / 3600, sec % 3600 / 60, sec % 60);
    let mut out = String::new();
    if h > 0 {
        out += &format!("{h}h ");
    }
    if h > 0 || m > 0 {
        out += &format!("{m}m ");
    }
    out + &format!("{s}s")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn number_format() {
        assert_eq!(fmt(10.0, 3, false), "10");
        assert_eq!(fmt(0.5, 3, true), ".5");
        assert_eq!(fmt(-0.0001, 3, false), "0");
        assert_eq!(fmt(1.23456, 3, false), "1.235");
    }
    #[test]
    fn simple_job() {
        let mut d = Doc::new();
        let mut s = Shape::new("rect");
        s.w = 20.0;
        s.h = 10.0;
        s.layer = 2;
        s.tf = translate(10.0, 10.0);
        d.add(s);
        let dev = Device { origin_top: false, ..Default::default() };
        let (ops, _) = plan_job(&mut d, &ImgCache::new(), &PlanOpts { selected_only: false, inner_first: true, start: [0.0, 0.0] }).unwrap();
        let j = emit_gcode(&ops, &dev, StartFrom::Absolute, [0.0, 0.0], [0.0, 0.0]);
        assert!(j.text.contains("G00 G17 G40 G21 G54\nG90\nM4\n"));
        assert!(j.text.contains("G0X10Y10\nG1X30S200F6000\nY20\nX10\nY10\n"), "{}", j.text);
        assert!(j.text.contains("; C02 @ 6000 mm/min, 20% power"));
    }
}
