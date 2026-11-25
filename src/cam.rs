//! 2D CNC routing: toolpaths from the design (profiles offset by the tool radius, pockets,
//! on-line cuts, drilling) cut in depth passes, with holding tabs, then G-code for GRBL routers.
//! Z = 0 is the top of the stock; cuts go to negative Z.

use crate::gcode::{fmt, Device, Job, PreviewSeg, StartFrom, APP_NAME, APP_VERSION};
use crate::geom::*;
use crate::model::{Doc, Layer, Shape};
use crate::tools::{Libraries, Tool};
use cavalier_contours::polyline::{PlineSource, PlineSourceMut, Polyline};
use cavalier_contours::shape_algorithms::{Shape as CShape, ShapeOffsetOptions};

pub const OPS: [&str; 5] = ["Profile outside", "Profile inside", "On line", "Pocket", "Drill"];

pub fn op_help(op: &str) -> &'static str {
    match op {
        "Profile outside" => "Cuts around the outside of each shape, so the part keeps its drawn size",
        "Profile inside" => "Cuts just inside each shape, so a hole keeps its drawn size",
        "On line" => "The tool's center follows the drawn line",
        "Pocket" => "Clears all material inside the shapes to the cut depth",
        "Drill" => "Plunges at the center of each shape, in steps",
        _ => "",
    }
}

/// One machine move: Rapid or Feed to (x, y, z) with a feed rate (mm/min)
#[derive(Clone, Debug)]
pub enum Mv {
    Layer { layer: Layer, tool: Tool },
    Rapid([f64; 3]),
    Feed([f64; 3], f64),
}

// ---------------------------------------------------------------- offsetting
fn signed_area(p: &[P]) -> f64 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (x0, y0) = (p[i][0], p[i][1]);
        let (x1, y1) = (p[(i + 1) % p.len()][0], p[(i + 1) % p.len()][1]);
        a += x0 * y1 - x1 * y0;
    }
    a / 2.0
}

fn open_pts(c: &Contour) -> Vec<P> {
    let mut v = c.pts.clone();
    while v.len() > 1 && dist(v[0], *v.last().unwrap()) < 1e-9 {
        v.pop();
    }
    v.dedup_by(|a, b| dist(*a, *b) < 1e-9);
    v
}
fn dist(a: P, b: P) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Flatten a cavalier polyline (with arc bulges) into points; closed loops repeat the first point
fn flatten(pl: &Polyline<f64>) -> Vec<P> {
    let n = pl.vertex_count();
    let mut out = vec![];
    if n == 0 {
        return out;
    }
    let segs = if pl.is_closed() { n } else { n - 1 };
    for i in 0..segs {
        let a = pl.at(i);
        let b = pl.at((i + 1) % n);
        let (p0, p1) = ([a.x, a.y], [b.x, b.y]);
        out.push(p0);
        if a.bulge.abs() > 1e-9 {
            let th = 4.0 * a.bulge.atan();
            let d = dist(p0, p1);
            if d < 1e-12 {
                continue;
            }
            let r = d / (2.0 * (th.abs() / 2.0).sin());
            let m = [(p0[0] + p1[0]) / 2.0, (p0[1] + p1[1]) / 2.0];
            let nrm = [-(p1[1] - p0[1]) / d, (p1[0] - p0[0]) / d];
            let h = d / 2.0 / (th / 2.0).tan();
            let c = [m[0] + nrm[0] * h, m[1] + nrm[1] * h];
            let a0 = (p0[1] - c[1]).atan2(p0[0] - c[0]);
            let k = ((th.abs() * r / 0.2).ceil() as usize).clamp(2, 400);
            for j in 1..k {
                let t = a0 + th * j as f64 / k as f64;
                out.push([c[0] + r * t.cos(), c[1] + r * t.sin()]);
            }
        }
    }
    if pl.is_closed() {
        out.push(out[0]);
    } else {
        let l = pl.at(n - 1);
        out.push([l.x, l.y]);
    }
    out
}

/// A region from closed outlines (even-odd nesting: outlines inside outlines are holes)
fn region(cs: &[Contour]) -> CShape<f64> {
    let closed: Vec<Contour> = cs.iter().filter(|c| c.closed && open_pts(c).len() >= 3).cloned().collect();
    let depth = containment_depth(&closed);
    let plines = closed.iter().zip(depth).map(|(c, d)| {
        let mut pts = open_pts(c);
        let ccw = signed_area(&pts) > 0.0;
        let want_ccw = d % 2 == 0;
        if ccw != want_ccw {
            pts.reverse();
        }
        let mut pl = Polyline::new_closed();
        for p in pts {
            pl.add(p[0], p[1], 0.0);
        }
        pl
    });
    CShape::from_plines(plines)
}

/// Loops of a region: (points, is_hole). Points run with the region on their left.
fn loops(s: &CShape<f64>) -> Vec<(Vec<P>, bool)> {
    let mut v: Vec<(Vec<P>, bool)> = s.ccw_plines.iter().map(|p| (flatten(&p.polyline), false)).collect();
    v.extend(s.cw_plines.iter().map(|p| (flatten(&p.polyline), true)));
    v.retain(|l| l.0.len() > 2);
    v
}

/// Grow (negative) or shrink (positive) a region by `d` mm
fn offset(s: &CShape<f64>, d: f64) -> CShape<f64> {
    if s.ccw_plines.is_empty() && s.cw_plines.is_empty() {
        return CShape::empty();
    }
    s.parallel_offset(d, &ShapeOffsetOptions::default())
}

// ---------------------------------------------------------------- tabs
/// Split a closed loop into points with a z each: inside a tab the cut stays at `tab_top`
fn with_tabs(pts: &[P], z: f64, tabs: Option<(u32, f64, f64)>) -> Vec<[f64; 3]> {
    let Some((count, width, tab_top)) = tabs.filter(|t| t.0 > 0 && z < t.2) else {
        return pts.iter().map(|p| [p[0], p[1], z]).collect();
    };
    let mut len = vec![0.0];
    for i in 1..pts.len() {
        len.push(len[i - 1] + dist(pts[i - 1], pts[i]));
    }
    let total = *len.last().unwrap();
    if total <= width * count as f64 * 1.5 {
        return pts.iter().map(|p| [p[0], p[1], z]).collect();
    }
    // tab spans along the loop, evenly spaced
    let spans: Vec<(f64, f64)> = (0..count).map(|i| {
        let c = total * (i as f64 + 0.5) / count as f64;
        (c - width / 2.0, c + width / 2.0)
    }).collect();
    let in_tab = |s: f64| spans.iter().any(|(a, b)| s > *a && s < *b);
    let mut cuts: Vec<f64> = spans.iter().flat_map(|(a, b)| [*a, *b]).collect();
    cuts.retain(|c| *c > 0.0 && *c < total);
    let mut out: Vec<[f64; 3]> = vec![];
    let zat = |s: f64| if in_tab(s) { tab_top } else { z };
    out.push([pts[0][0], pts[0][1], zat(0.0)]);
    for i in 1..pts.len() {
        let (s0, s1) = (len[i - 1], len[i]);
        let mut inner: Vec<f64> = cuts.iter().copied().filter(|c| *c > s0 && *c < s1).collect();
        inner.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for c in inner {
            let t = (c - s0) / (s1 - s0).max(1e-12);
            let p = [pts[i - 1][0] + (pts[i][0] - pts[i - 1][0]) * t, pts[i - 1][1] + (pts[i][1] - pts[i - 1][1]) * t];
            // step up or down exactly at the tab edge
            let before = zat(c - 1e-7);
            let after = zat(c + 1e-7);
            out.push([p[0], p[1], before]);
            out.push([p[0], p[1], after]);
        }
        out.push([pts[i][0], pts[i][1], zat(s1 - 1e-7)]);
    }
    out
}

// ---------------------------------------------------------------- planning
pub struct RouterOpts {
    pub selected_only: bool,
    pub start: P,
}

/// Depth of each pass: equal steps no deeper than `stepdown`
pub fn pass_depths(depth: f64, stepdown: f64) -> Vec<f64> {
    let depth = depth.max(0.0);
    if depth <= 0.0 {
        return vec![];
    }
    let n = (depth / stepdown.max(0.05) - 1e-9).ceil().max(1.0) as usize;
    (1..=n).map(|k| -depth * k as f64 / n as f64).collect()
}

pub fn plan_router(doc: &mut Doc, lib: &Libraries, dev: &Device, o: &RouterOpts) -> Result<(Vec<Mv>, Vec<String>), String> {
    let mut mv = vec![];
    let mut warn = vec![];
    let safe = dev.safe_z.max(0.5);
    let mut pos = [o.start[0], o.start[1], safe];
    for li in doc.used_layers() {
        let l = doc.layers[li].clone();
        if !l.output {
            continue;
        }
        let shapes: Vec<&Shape> = doc.shapes.iter().filter(|s| s.layer == li && (!o.selected_only || doc.sel.contains(&s.id)) && s.kind != "image").collect();
        if doc.shapes.iter().any(|s| s.layer == li && s.kind == "image") {
            warn.push(format!("Pictures on layer {} are skipped: routing needs outlines.", l.name));
        }
        if shapes.is_empty() {
            continue;
        }
        let Some(tool) = lib.tool(l.tool_id).cloned() else {
            warn.push(format!("Layer {} has no tool from the library (T{}); it was skipped.", l.name, l.tool_id));
            continue;
        };
        if l.depth > tool.flute_length + 1e-6 && tool.flute_length > 0.0 {
            warn.push(format!("Layer {}: cut depth {} mm is longer than {}'s flutes ({} mm).", l.name, fmt(l.depth, 2, false), tool.name, fmt(tool.flute_length, 2, false)));
        }
        if dev.stock > 0.0 && l.depth > dev.stock + 0.5 {
            warn.push(format!("Layer {}: cut depth {} mm goes more than 0.5 mm below the {} mm stock: check the spoilboard.", l.name, fmt(l.depth, 2, false), fmt(dev.stock, 2, false)));
        }
        let r = tool.diameter / 2.0;
        let feed = l.feed.max(1.0);
        let plunge = l.plunge.max(1.0);
        let levels = pass_depths(l.depth, l.stepdown);
        if levels.is_empty() {
            continue;
        }
        mv.push(Mv::Layer { layer: l.clone(), tool: tool.clone() });
        let cs: Vec<Contour> = shapes.iter().flat_map(|s| s.world_contours()).filter(|c| c.pts.len() > 1).collect();
        let open: Vec<Vec<P>> = cs.iter().filter(|c| !c.closed).map(|c| c.pts.clone()).collect();

        // closed loops to cut at every level: (points, is_closed, inner-ness for ordering, tabs allowed)
        let mut paths: Vec<(Vec<P>, bool, usize, bool)> = vec![];
        match l.op.as_str() {
            "Profile outside" | "Profile inside" => {
                let reg = region(&cs);
                let outside = l.op == "Profile outside";
                let res = offset(&reg, if outside { -r } else { r });
                if res.ccw_plines.is_empty() && !reg.ccw_plines.is_empty() {
                    warn.push(format!("Layer {}: some shapes are too small for a {} mm tool and were skipped.", l.name, fmt(tool.diameter, 3, false)));
                }
                for (mut p, hole) in loops(&res) {
                    // the kept side is on the left for outside cuts (climb); inside cuts keep the right side
                    if l.climb != outside {
                        p.reverse();
                    }
                    paths.push((p, true, if hole { 1 } else { 0 }, true));
                }
                for p in &open {
                    paths.push((p.clone(), false, 0, false));
                }
            }
            "On line" => {
                for c in &cs {
                    paths.push((c.pts.clone(), c.closed, 0, c.closed));
                }
            }
            "Pocket" => {
                let reg = region(&cs);
                let step = (tool.diameter * l.stepover / 100.0).clamp(0.02, tool.diameter);
                let mut rings: Vec<Vec<Vec<P>>> = vec![];
                let mut cur = offset(&reg, r);
                let mut guard = 0;
                while !(cur.ccw_plines.is_empty() && cur.cw_plines.is_empty()) && guard < 5000 {
                    rings.push(loops(&cur).into_iter().map(|(mut p, _)| {
                        if l.climb {
                            p.reverse();
                        }
                        p
                    }).collect());
                    cur = offset(&cur, step);
                    guard += 1;
                }
                if rings.is_empty() {
                    warn.push(format!("Layer {}: the pocket is narrower than the {} mm tool.", l.name, fmt(tool.diameter, 3, false)));
                }
                // innermost rings first, the finishing wall pass last
                let n = rings.len();
                for (k, ring) in rings.into_iter().enumerate().rev() {
                    for p in ring {
                        paths.push((p, true, n - k, false));
                    }
                }
            }
            "Drill" => {
                for c in cs.iter().filter(|c| c.closed) {
                    if let Some(b) = bbox_of(std::slice::from_ref(c)) {
                        paths.push((vec![[b.cx(), b.cy()]], false, 0, false));
                    }
                }
            }
            other => warn.push(format!("Layer {}: unknown operation \"{other}\".", l.name)),
        }

        let tabs = if l.tabs && l.op.starts_with("Profile") && l.tab_height > 0.0 && l.tab_height < l.depth {
            Some((l.tab_count, l.tab_width + tool.diameter, -l.depth + l.tab_height))
        } else {
            None
        };

        if l.op == "Pocket" {
            // pockets: every ring at one depth, then the next depth
            let mut prev = 0.0;
            for &z in &levels {
                for (p, _, _, _) in &paths {
                    // rings link at 0.5 mm above the stock top, so a link never crosses uncut material
                    cut_loop(&mut mv, &mut pos, p, true, &[z], prev, 0.5, feed, plunge, None, false);
                }
                prev = z;
            }
            mv.push(Mv::Rapid([pos[0], pos[1], safe]));
            pos[2] = safe;
            continue;
        }
        // other operations: each path cut to full depth before the next (holes and inner shapes first)
        let mut left: Vec<(Vec<P>, bool, usize, bool)> = paths;
        while !left.is_empty() {
            let max_in = left.iter().map(|p| p.2).max().unwrap_or(0);
            let (mut bi, mut bd) = (0, f64::INFINITY);
            for (i, p) in left.iter().enumerate() {
                if p.2 != max_in {
                    continue;
                }
                let d = dist(p.0[0], [pos[0], pos[1]]);
                if d < bd {
                    bd = d;
                    bi = i;
                }
            }
            let (p, closed, _, tab_ok) = left.remove(bi);
            if l.op == "Drill" {
                drill(&mut mv, &mut pos, p[0], &levels, safe, plunge);
            } else {
                cut_loop(&mut mv, &mut pos, &p, closed, &levels, 0.0, safe, feed, plunge, if tab_ok { tabs } else { None }, true);
            }
        }
    }
    Ok((mv, warn))
}

/// Cut one path at the given depths. Closed loops return to their start each pass;
/// open paths reverse direction each pass.
#[allow(clippy::too_many_arguments)]
fn cut_loop(mv: &mut Vec<Mv>, pos: &mut [f64; 3], p: &[P], closed: bool, levels: &[f64], above: f64, lift: f64, feed: f64, plunge: f64, tabs: Option<(u32, f64, f64)>, retract_after: bool) {
    if p.is_empty() {
        return;
    }
    let start = p[0];
    let entry = above + 0.5;
    if dist(start, [pos[0], pos[1]]) > 1e-6 {
        // lift clear, travel, then drop to just above the last cleared depth
        if pos[2] < lift - 1e-9 {
            mv.push(Mv::Rapid([pos[0], pos[1], lift]));
            pos[2] = lift;
        }
        mv.push(Mv::Rapid([start[0], start[1], pos[2]]));
        mv.push(Mv::Rapid([start[0], start[1], entry]));
        *pos = [start[0], start[1], entry];
    } else if pos[2] > entry + 1e-9 {
        mv.push(Mv::Rapid([start[0], start[1], entry]));
        pos[2] = entry;
    }
    let mut forward = true;
    for &z in levels {
        let seq: Vec<P> = if forward || closed { p.to_vec() } else { p.iter().rev().copied().collect() };
        let pts3 = if closed { with_tabs(&seq, z, tabs) } else { seq.iter().map(|q| [q[0], q[1], z]).collect() };
        // plunge at the start of this pass
        let first = pts3[0];
        if dist([first[0], first[1]], [pos[0], pos[1]]) > 1e-6 {
            mv.push(Mv::Feed([first[0], first[1], pos[2]], feed));
        }
        mv.push(Mv::Feed(first, plunge));
        for q in &pts3[1..] {
            let f = if q[2] < pos[2] - 1e-9 && (q[0] - pos[0]).abs() < 1e-9 && (q[1] - pos[1]).abs() < 1e-9 { plunge } else { feed };
            mv.push(Mv::Feed(*q, f));
            *pos = *q;
        }
        *pos = *pts3.last().unwrap();
        if !closed {
            forward = !forward;
        }
    }
    if retract_after {
        mv.push(Mv::Rapid([pos[0], pos[1], lift]));
        pos[2] = lift;
    }
}

fn drill(mv: &mut Vec<Mv>, pos: &mut [f64; 3], c: P, levels: &[f64], safe: f64, plunge: f64) {
    if pos[2] < safe - 1e-9 {
        mv.push(Mv::Rapid([pos[0], pos[1], safe]));
    }
    mv.push(Mv::Rapid([c[0], c[1], safe]));
    mv.push(Mv::Rapid([c[0], c[1], 0.5]));
    let mut prev = 0.0;
    for &z in levels {
        // peck: go back down near the last depth, drill on, then lift out to clear chips
        if prev < 0.0 {
            mv.push(Mv::Rapid([c[0], c[1], prev + 0.5]));
        }
        mv.push(Mv::Feed([c[0], c[1], z], plunge));
        mv.push(Mv::Rapid([c[0], c[1], 0.5]));
        prev = z;
    }
    mv.push(Mv::Rapid([c[0], c[1], safe]));
    *pos = [c[0], c[1], safe];
}

// ---------------------------------------------------------------- G-code
pub fn emit_router(moves: &[Mv], dev: &Device, start_from: StartFrom, origin: P, start: P) -> Job {
    let n3 = |v: f64| fmt(v, 3, dev.strip_leading_zero);
    let rel = start_from == StartFrom::Current;
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
    let mut min_z: f64 = 0.0;
    for m in moves {
        if let Mv::Feed(p, _) = m {
            if p[2] < 0.0 {
                bx = BBox::add_point(bx, [p[0], p[1]]);
                min_z = min_z.min(p[2]);
            }
        }
    }
    lines.push(format!("; {APP_NAME} {APP_VERSION}"));
    lines.push(format!(
        "; GRBL router profile, {}",
        match start_from {
            StartFrom::Absolute => "absolute coords",
            StartFrom::User => "user origin",
            StartFrom::Current => "current position",
        }
    ));
    if let Some(b) = bx {
        let (a, c) = (to_out(b.x0, b.y0), to_out(b.x1, b.y1));
        lines.push(format!("; Bounds: X{} Y{} to X{} Y{}, deepest Z{}", n3(a[0].min(c[0])), n3(a[1].min(c[1])), n3(a[0].max(c[0])), n3(a[1].max(c[1])), n3(min_z)));
    }
    lines.push("; Z0 is the top of the stock".into());
    lines.push("G21 G90 G17 G94".into());
    if rel {
        // current position: make here the job origin, then work in absolute coordinates
        lines.push("G92 X0 Y0".into());
    }
    lines.push(format!("G0 Z{}", n3(dev.safe_z)));

    let (mut lx, mut ly, mut lz): (Option<f64>, Option<f64>, Option<f64>) = (None, None, None);
    let mut lf: Option<f64> = None;
    let mut mode = -1;
    let mut w = [start[0], start[1], dev.safe_z];
    let mut preview: Vec<PreviewSeg> = vec![];
    let (mut time, mut cut_len, mut travel) = (0.0, 0.0, 0.0);
    let mut cur_tool: Option<u32> = None;
    let mut cur_rpm: Option<f64> = None;
    let mut cur_layer = 0usize;
    let mut first_tool = true;
    for m in moves {
        match m {
            Mv::Layer { layer, tool } => {
                cur_layer = layer.index;
                lines.push(format!(
                    "; {}: {} with T{} {} (Ø{} mm), depth {} mm in {} pass{}, feed {} mm/min, plunge {} mm/min, {} RPM",
                    layer.name,
                    layer.op,
                    tool.id,
                    tool.name,
                    fmt(tool.diameter, 3, false),
                    fmt(layer.depth, 3, false),
                    pass_depths(layer.depth, layer.stepdown).len(),
                    if pass_depths(layer.depth, layer.stepdown).len() == 1 { "" } else { "es" },
                    fmt(layer.feed, 1, false),
                    fmt(layer.plunge, 1, false),
                    fmt(layer.rpm, 0, false)
                ));
                if cur_tool != Some(tool.id) {
                    if !first_tool && dev.tool_change_pause {
                        lines.push(format!("G0 Z{}", n3(dev.safe_z)));
                        lines.push("M5".into());
                        lines.push(format!("; TOOL CHANGE: fit T{} {} (Ø{} mm), set Z zero again, then press Resume", tool.id, tool.name, fmt(tool.diameter, 3, false)));
                        lines.push("M0".into());
                        cur_rpm = None;
                        mode = -1;
                    }
                    first_tool = false;
                    cur_tool = Some(tool.id);
                }
                if cur_rpm != Some(layer.rpm) {
                    lines.push(format!("M3 S{}", fmt(layer.rpm, 0, false)));
                    if dev.spindle_delay > 0.0 {
                        lines.push(format!("G4 P{}", fmt(dev.spindle_delay, 1, false)));
                        time += dev.spindle_delay;
                    }
                    cur_rpm = Some(layer.rpm);
                }
            }
            Mv::Rapid(p) | Mv::Feed(p, _) => {
                let (g, f) = match m {
                    Mv::Feed(_, f) => (1, *f),
                    _ => (0, 0.0),
                };
                let o = to_out(p[0], p[1]);
                let need_x = lx.map(|v| (o[0] - v).abs() >= 0.001).unwrap_or(true);
                let need_y = ly.map(|v| (o[1] - v).abs() >= 0.001).unwrap_or(true);
                let need_z = lz.map(|v| (p[2] - v).abs() >= 0.001).unwrap_or(true);
                if !(need_x || need_y || need_z) {
                    continue;
                }
                let mut line = String::new();
                if g != mode {
                    line += if g == 0 { "G0" } else { "G1" };
                    mode = g;
                }
                if need_x {
                    line += &format!("X{}", n3(o[0]));
                    lx = Some(o[0]);
                }
                if need_y {
                    line += &format!("Y{}", n3(o[1]));
                    ly = Some(o[1]);
                }
                if need_z {
                    line += &format!("Z{}", n3(p[2]));
                    lz = Some(p[2]);
                }
                if g == 1 && lf.map(|v| (v - f).abs() > 0.01).unwrap_or(true) {
                    line += &format!("F{}", fmt(f, 1, false));
                    lf = Some(f);
                }
                lines.push(line);
                let dxy = (p[0] - w[0]).hypot(p[1] - w[1]);
                let dz = (p[2] - w[2]).abs();
                let d = (dxy * dxy + dz * dz).sqrt();
                if g == 0 {
                    time += dxy / dev.rapid.max(1.0) + dz / 10.0;
                    travel += d;
                } else {
                    time += d / (f / 60.0).max(0.01);
                    cut_len += d;
                }
                if dxy > 1e-9 {
                    // preview: depth below the stock top (positive mm) for cuts, 0 for travel
                    let depth = if g == 1 { (-p[2].min(w[2])).max(0.0) } else { 0.0 };
                    preview.push((w[0], w[1], p[0], p[1], depth, cur_layer));
                }
                w = *p;
            }
        }
    }
    lines.push(format!("G0 Z{}", n3(dev.safe_z)));
    lines.push("M5".into());
    if dev.return_to_finish {
        let (fx, fy) = if start_from == StartFrom::Absolute { (dev.finish_x, dev.finish_y) } else { (0.0, 0.0) };
        lines.push("; return to finish position".into());
        lines.push(format!("G0 X{} Y{}", n3(fx), n3(fy)));
    }
    if rel {
        lines.push("G92.1".into());
    }
    lines.push("M2".into());
    let n = lines.len();
    Job { text: lines.join("\n") + "\n", lines: n, preview, time, cut_len, travel }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::translate;

    fn square_doc(op: &str, size: f64) -> Doc {
        let mut d = Doc::new();
        let mut s = Shape::new("rect");
        s.w = size;
        s.h = size;
        s.layer = 2;
        s.tf = translate(50.0, 50.0);
        d.add(s);
        let l = &mut d.layers[2];
        l.op = op.into();
        l.tool_id = 2; // 6.35 mm
        l.depth = 6.0;
        l.stepdown = 2.5;
        d
    }
    fn cut_box(mv: &[Mv]) -> (BBox, f64) {
        let mut b = None;
        let mut z: f64 = 0.0;
        for m in mv {
            if let Mv::Feed(p, _) = m {
                if p[2] < 0.0 {
                    b = BBox::add_point(b, [p[0], p[1]]);
                    z = z.min(p[2]);
                }
            }
        }
        (b.unwrap(), z)
    }

    #[test]
    fn depth_passes_are_even() {
        assert_eq!(pass_depths(6.0, 2.5), vec![-2.0, -4.0, -6.0]);
        assert_eq!(pass_depths(3.0, 3.0), vec![-3.0]);
    }

    #[test]
    fn outside_and_inside_profiles_offset_by_the_tool_radius() {
        let lib = Libraries::in_memory();
        let dev = Device { origin_top: false, ..Default::default() };
        let r = lib.tool(2).unwrap().diameter / 2.0;
        let mut d = square_doc("Profile outside", 40.0);
        let (mv, _) = plan_router(&mut d, &lib, &dev, &RouterOpts { selected_only: false, start: [0.0, 0.0] }).unwrap();
        let (b, z) = cut_box(&mv);
        assert!((b.x0 - (50.0 - r)).abs() < 0.05 && (b.x1 - (90.0 + r)).abs() < 0.05, "{b:?}");
        assert!((z + 6.0).abs() < 1e-9);
        let mut d = square_doc("Profile inside", 40.0);
        let (mv, _) = plan_router(&mut d, &lib, &dev, &RouterOpts { selected_only: false, start: [0.0, 0.0] }).unwrap();
        let (b, _) = cut_box(&mv);
        assert!((b.x0 - (50.0 + r)).abs() < 0.05 && (b.x1 - (90.0 - r)).abs() < 0.05, "{b:?}");
    }

    #[test]
    fn pocket_clears_the_inside_and_tabs_hold_the_part() {
        let lib = Libraries::in_memory();
        let dev = Device { origin_top: false, ..Default::default() };
        let mut d = square_doc("Pocket", 40.0);
        let (mv, _) = plan_router(&mut d, &lib, &dev, &RouterOpts { selected_only: false, start: [0.0, 0.0] }).unwrap();
        let rings = mv.iter().filter(|m| matches!(m, Mv::Feed(p, _) if (p[2] + 2.0).abs() < 1e-9)).count();
        assert!(rings > 20, "pocket made several rings at the first depth");
        let mut d = square_doc("Profile outside", 40.0);
        d.layers[2].tabs = true;
        d.layers[2].tab_count = 4;
        d.layers[2].tab_height = 2.0;
        let (mv, _) = plan_router(&mut d, &lib, &dev, &RouterOpts { selected_only: false, start: [0.0, 0.0] }).unwrap();
        let at_tab_top = mv.iter().filter(|m| matches!(m, Mv::Feed(p, _) if (p[2] + 4.0).abs() < 1e-9)).count();
        assert!(at_tab_top >= 8, "the last pass rides over the tabs ({at_tab_top})");
        let j = emit_router(&mv, &dev, StartFrom::Absolute, [0.0, 0.0], [0.0, 0.0]);
        assert!(j.text.contains("G21 G90 G17 G94") && j.text.contains("M3 S18000") && j.text.contains("Z-6"));
    }

    #[test]
    fn tool_change_pauses_between_tools() {
        let lib = Libraries::in_memory();
        let dev = Device::default();
        let mut d = square_doc("Profile outside", 40.0);
        let mut s = Shape::new("ellipse");
        s.w = 10.0;
        s.h = 10.0;
        s.layer = 3;
        s.tf = translate(150.0, 50.0);
        d.add(s);
        d.layers[3].op = "Drill".into();
        d.layers[3].tool_id = 1;
        let (mv, _) = plan_router(&mut d, &lib, &dev, &RouterOpts { selected_only: false, start: [0.0, 0.0] }).unwrap();
        let j = emit_router(&mv, &dev, StartFrom::Absolute, [0.0, 0.0], [0.0, 0.0]);
        assert_eq!(j.text.matches("\nM0\n").count(), 1);
    }
}
