//! Curves made of points with Bézier handles: drawing, editing, and turning other shapes into curves.
//! A "spline" shape keeps its curves in world coordinates (its matrix stays the identity).

use crate::geom::*;
use crate::model::Shape;
use serde::{Deserialize, Serialize};

pub const KINDS: [(&str, &str, &str); 4] = [
    ("sharp", "Sharp", "A hard corner with no handles: straight lines meet here"),
    ("corner", "Corner", "Curved on both sides, with handles that move on their own"),
    ("smooth", "Smooth", "The curve flows through: handles stay in line, each its own length"),
    ("symmetric", "Symmetric", "The curve flows through: handles stay in line and equal in length"),
];
/// Bézier circle constant: handle length = 0.5523 x radius
const KAPPA: f64 = 0.552_284_75;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Node {
    /// the point itself
    pub p: P,
    /// incoming handle (absolute position)
    pub i: P,
    /// outgoing handle (absolute position)
    pub o: P,
    #[serde(default = "sharp")]
    pub kind: String,
}
fn sharp() -> String {
    "sharp".into()
}
impl Node {
    pub fn sharp(p: P) -> Node {
        Node { p, i: p, o: p, kind: "sharp".into() }
    }
    pub fn with(p: P, i: P, o: P, kind: &str) -> Node {
        Node { p, i, o, kind: kind.into() }
    }
    pub fn transform(&mut self, m: &M) {
        self.p = apply(m, self.p);
        self.i = apply(m, self.i);
        self.o = apply(m, self.o);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Curve {
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub closed: bool,
}

fn d(a: P, b: P) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
fn lerp(a: P, b: P, t: f64) -> P {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}
pub fn bez(p0: P, p1: P, p2: P, p3: P, t: f64) -> P {
    let u = 1.0 - t;
    [
        u * u * u * p0[0] + 3.0 * u * u * t * p1[0] + 3.0 * u * t * t * p2[0] + t * t * t * p3[0],
        u * u * u * p0[1] + 3.0 * u * u * t * p1[1] + 3.0 * u * t * t * p2[1] + t * t * t * p3[1],
    ]
}

impl Curve {
    /// Segments as (from node, to node) index pairs
    pub fn segments(&self) -> Vec<(usize, usize)> {
        let n = self.nodes.len();
        let mut v: Vec<(usize, usize)> = (0..n.saturating_sub(1)).map(|k| (k, k + 1)).collect();
        if self.closed && n > 2 {
            v.push((n - 1, 0));
        }
        v
    }
    fn is_line(&self, a: usize, b: usize) -> bool {
        let (na, nb) = (&self.nodes[a], &self.nodes[b]);
        d(na.o, na.p) < 1e-9 && d(nb.i, nb.p) < 1e-9
    }
    /// Points along the curve, 0.25 mm chords or finer
    pub fn flatten(&self) -> Contour {
        let mut pts: Vec<P> = vec![];
        if let Some(f) = self.nodes.first() {
            pts.push(f.p);
        }
        for (a, b) in self.segments() {
            let (na, nb) = (&self.nodes[a], &self.nodes[b]);
            if self.is_line(a, b) {
                pts.push(nb.p);
                continue;
            }
            let len = d(na.p, na.o) + d(na.o, nb.i) + d(nb.i, nb.p);
            let n = ((len / 0.25).ceil() as usize).clamp(4, 400);
            for k in 1..=n {
                pts.push(bez(na.p, na.o, nb.i, nb.p, k as f64 / n as f64));
            }
        }
        Contour { pts, closed: self.closed && self.nodes.len() > 2 }
    }
    /// The nearest point on a segment: (segment index, t, distance)
    pub fn nearest(&self, w: P) -> Option<(usize, f64, f64)> {
        let mut best: Option<(usize, f64, f64)> = None;
        for (si, (a, b)) in self.segments().into_iter().enumerate() {
            let (na, nb) = (&self.nodes[a], &self.nodes[b]);
            let n = 64;
            for k in 0..=n {
                let t = k as f64 / n as f64;
                let q = bez(na.p, na.o, nb.i, nb.p, t);
                let dd = d(q, w);
                if best.map(|x| dd < x.2).unwrap_or(true) {
                    best = Some((si, t, dd));
                }
            }
        }
        best
    }
    /// Add a point on segment `si` at `t` without changing the curve's shape (de Casteljau split)
    pub fn insert(&mut self, si: usize, t: f64) -> usize {
        let (a, b) = self.segments()[si];
        let line = self.is_line(a, b);
        let (p0, p1, p2, p3) = (self.nodes[a].p, self.nodes[a].o, self.nodes[b].i, self.nodes[b].p);
        let (q0, q1, q2) = (lerp(p0, p1, t), lerp(p1, p2, t), lerp(p2, p3, t));
        let (r0, r1) = (lerp(q0, q1, t), lerp(q1, q2, t));
        let m = lerp(r0, r1, t);
        let node = if line { Node::sharp(m) } else { Node::with(m, r0, r1, "smooth") };
        if !line {
            self.nodes[a].o = q0;
            self.nodes[b].i = q2;
        }
        let at = if b == 0 { self.nodes.len() } else { b };
        self.nodes.insert(at, node);
        at
    }
    fn neighbors(&self, ni: usize) -> (Option<P>, Option<P>) {
        let n = self.nodes.len();
        let prev = if ni > 0 { Some(self.nodes[ni - 1].p) } else if self.closed && n > 2 { Some(self.nodes[n - 1].p) } else { None };
        let next = if ni + 1 < n { Some(self.nodes[ni + 1].p) } else if self.closed && n > 2 { Some(self.nodes[0].p) } else { None };
        (prev, next)
    }
    /// Change a point's kind, making handles when it needs them
    pub fn set_kind(&mut self, ni: usize, kind: &str) {
        let (prev, next) = self.neighbors(ni);
        let n = &mut self.nodes[ni];
        let p = n.p;
        match kind {
            "sharp" => {
                n.i = p;
                n.o = p;
            }
            "smooth" | "symmetric" => {
                let has = d(n.i, p) > 1e-6 || d(n.o, p) > 1e-6;
                // direction of the flow through the point
                let dir = if has {
                    let v = if d(n.o, p) > 1e-6 { [n.o[0] - p[0], n.o[1] - p[1]] } else { [p[0] - n.i[0], p[1] - n.i[1]] };
                    v
                } else {
                    match (prev, next) {
                        (Some(a), Some(b)) => [b[0] - a[0], b[1] - a[1]],
                        (None, Some(b)) => [b[0] - p[0], b[1] - p[1]],
                        (Some(a), None) => [p[0] - a[0], p[1] - a[1]],
                        _ => [1.0, 0.0],
                    }
                };
                let l = dir[0].hypot(dir[1]).max(1e-9);
                let u = [dir[0] / l, dir[1] / l];
                let mut li = if d(n.i, p) > 1e-6 { d(n.i, p) } else { prev.map(|a| d(a, p) / 3.0).unwrap_or(5.0) };
                let mut lo = if d(n.o, p) > 1e-6 { d(n.o, p) } else { next.map(|b| d(b, p) / 3.0).unwrap_or(5.0) };
                if kind == "symmetric" {
                    let avg = (li + lo) / 2.0;
                    li = avg;
                    lo = avg;
                }
                n.o = [p[0] + u[0] * lo, p[1] + u[1] * lo];
                n.i = [p[0] - u[0] * li, p[1] - u[1] * li];
            }
            _ => {}
        }
        n.kind = kind.to_string();
    }
    /// Move one handle; the other follows the point's kind
    pub fn move_handle(&mut self, ni: usize, out: bool, pos: P) {
        let n = &mut self.nodes[ni];
        let p = n.p;
        if out {
            n.o = pos;
        } else {
            n.i = pos;
        }
        let moved = if out { n.o } else { n.i };
        let other = if out { n.i } else { n.o };
        let v = [moved[0] - p[0], moved[1] - p[1]];
        let lv = v[0].hypot(v[1]);
        let new_other = match n.kind.as_str() {
            "symmetric" => Some([p[0] - v[0], p[1] - v[1]]),
            "smooth" if lv > 1e-9 => {
                let lo = d(other, p);
                let lo = if lo < 1e-9 { lv } else { lo };
                Some([p[0] - v[0] / lv * lo, p[1] - v[1] / lv * lo])
            }
            _ => None,
        };
        if let Some(o) = new_other {
            if out {
                n.i = o;
            } else {
                n.o = o;
            }
        }
        if n.kind == "sharp" && lv > 1e-9 {
            n.kind = "corner".into();
        }
    }
    pub fn move_node(&mut self, ni: usize, to: P) {
        let n = &mut self.nodes[ni];
        let dv = [to[0] - n.p[0], to[1] - n.p[1]];
        for q in [&mut n.p, &mut n.i, &mut n.o] {
            q[0] += dv[0];
            q[1] += dv[1];
        }
    }
}

/// Name the kind of each point from its handles
pub fn classify(n: &mut Node) {
    let (hi, ho) = (d(n.i, n.p) > 1e-6, d(n.o, n.p) > 1e-6);
    n.kind = if !hi && !ho {
        "sharp".into()
    } else if hi && ho {
        let a = [n.p[0] - n.i[0], n.p[1] - n.i[1]];
        let b = [n.o[0] - n.p[0], n.o[1] - n.p[1]];
        let cross = (a[0] * b[1] - a[1] * b[0]) / (a[0].hypot(a[1]) * b[0].hypot(b[1]));
        let dot = a[0] * b[0] + a[1] * b[1];
        if cross.abs() < 0.01 && dot > 0.0 {
            if (d(n.i, n.p) - d(n.o, n.p)).abs() < 1e-3 { "symmetric".into() } else { "smooth".into() }
        } else {
            "corner".into()
        }
    } else {
        "corner".into()
    };
}

/// Curves (local coordinates) for a shape, exact where the shape allows it
fn local_curves(s: &Shape) -> Option<Vec<Curve>> {
    let (w, h) = (s.w, s.h);
    Some(match s.kind.as_str() {
        "rect" => {
            let r = s.r.min(w / 2.0).min(h / 2.0).max(0.0);
            if r <= 1e-9 {
                vec![Curve { nodes: [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]].into_iter().map(Node::sharp).collect(), closed: true }]
            } else {
                let k = KAPPA * r;
                let n = |p: P, i: P, o: P| Node::with(p, i, o, "smooth");
                let mut v = vec![
                    n([r, 0.0], [r - k, 0.0], [r, 0.0]),
                    n([w - r, 0.0], [w - r, 0.0], [w - r + k, 0.0]),
                    n([w, r], [w, r - k], [w, r]),
                    n([w, h - r], [w, h - r], [w, h - r + k]),
                    n([w - r, h], [w - r + k, h], [w - r, h]),
                    n([r, h], [r, h], [r - k, h]),
                    n([0.0, h - r], [0.0, h - r + k], [0.0, h - r]),
                    n([0.0, r], [0.0, r], [0.0, r - k]),
                ];
                // where the rounding fills a whole side, the two points meet: keep one
                v.dedup_by(|b, a| {
                    if d(a.p, b.p) < 1e-9 {
                        a.o = b.o;
                        true
                    } else {
                        false
                    }
                });
                if v.len() > 1 && d(v[0].p, v.last().unwrap().p) < 1e-9 {
                    let l = v.pop().unwrap();
                    v[0].i = l.i;
                }
                for x in v.iter_mut() {
                    classify(x);
                }
                vec![Curve { nodes: v, closed: true }]
            }
        }
        "ellipse" => {
            let (cx, cy, rx, ry) = (w / 2.0, h / 2.0, w / 2.0, h / 2.0);
            let (kx, ky) = (KAPPA * rx, KAPPA * ry);
            vec![Curve {
                nodes: vec![
                    Node::with([cx + rx, cy], [cx + rx, cy - ky], [cx + rx, cy + ky], "symmetric"),
                    Node::with([cx, cy + ry], [cx + kx, cy + ry], [cx - kx, cy + ry], "symmetric"),
                    Node::with([cx - rx, cy], [cx - rx, cy + ky], [cx - rx, cy - ky], "symmetric"),
                    Node::with([cx, cy - ry], [cx - kx, cy - ry], [cx + kx, cy - ry], "symmetric"),
                ],
                closed: true,
            }]
        }
        "polygon" | "path" => s.local_contours().into_iter().map(polyline_curve).collect(),
        "text" => {
            let spec = crate::importers::TextSpec { text: &s.text, font: &s.font, size: s.size, align: if s.align.is_empty() { "left" } else { &s.align }, bold: s.bold, italic: s.italic };
            let cs = crate::importers::text_curves(&spec);
            if cs.is_empty() {
                s.local_contours().into_iter().map(polyline_curve).collect()
            } else {
                cs
            }
        }
        "spline" => s.curves.clone(),
        _ => return None,
    })
}

pub fn polyline_curve(c: Contour) -> Curve {
    let mut pts = c.pts;
    let closed = c.closed;
    if closed && pts.len() > 1 && d(pts[0], *pts.last().unwrap()) < 1e-9 {
        pts.pop();
    }
    pts.dedup_by(|a, b| d(*a, *b) < 1e-9);
    Curve { nodes: pts.into_iter().map(Node::sharp).collect(), closed }
}

/// Turn a shape into an editable curve shape (world coordinates). None for pictures.
pub fn to_spline(s: &Shape) -> Option<Shape> {
    let mut curves = local_curves(s)?;
    for c in curves.iter_mut() {
        for n in c.nodes.iter_mut() {
            n.transform(&s.tf);
        }
    }
    curves.retain(|c| c.nodes.len() > 1);
    if curves.is_empty() {
        return None;
    }
    let mut out = Shape::new("spline");
    out.id = s.id;
    out.layer = s.layer;
    out.curves = curves;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rounded_rectangle_converts_exactly() {
        let mut r = Shape::new("rect");
        r.w = 40.0;
        r.h = 20.0;
        r.r = 5.0;
        r.tf = translate(10.0, 10.0);
        let s = to_spline(&r).unwrap();
        assert_eq!(s.curves[0].nodes.len(), 8);
        let a = bbox_of(&r.world_contours()).unwrap();
        let b = bbox_of(&s.world_contours()).unwrap();
        assert!((a.x0 - b.x0).abs() < 1e-6 && (a.y1 - b.y1).abs() < 1e-6 && (a.w() - b.w()).abs() < 1e-6);
    }
    #[test]
    fn inserting_a_point_keeps_the_shape() {
        let mut e = Shape::new("ellipse");
        e.w = 20.0;
        e.h = 20.0;
        let mut s = to_spline(&e).unwrap();
        let before = bez(s.curves[0].nodes[0].p, s.curves[0].nodes[0].o, s.curves[0].nodes[1].i, s.curves[0].nodes[1].p, 0.5);
        let at = s.curves[0].insert(0, 0.5);
        assert!(d(s.curves[0].nodes[at].p, before) < 1e-9);
        assert_eq!(s.curves[0].nodes.len(), 5);
    }
    #[test]
    fn text_converts_to_font_curves_in_place() {
        let spec = crate::importers::TextSpec { text: "Ob", font: "Sans", size: 20.0, align: "left", bold: false, italic: false };
        let (cs, w, h) = crate::importers::text_outlines(&spec);
        let mut t = Shape::new("text");
        t.text = "Ob".into();
        t.font = "Sans".into();
        t.size = 20.0;
        t.subpaths = cs;
        t.w = w;
        t.h = h;
        t.tf = translate(30.0, 40.0);
        let s = to_spline(&t).unwrap();
        assert!(s.curves.len() >= 4, "O and b have inner and outer outlines");
        assert!(s.curves.iter().flat_map(|c| &c.nodes).any(|n| n.kind != "sharp"), "letters keep real curves");
        let a = bbox_of(&t.world_contours()).unwrap();
        let b = bbox_of(&s.world_contours()).unwrap();
        assert!((a.x0 - b.x0).abs() < 0.05 && (a.y0 - b.y0).abs() < 0.05 && (a.w() - b.w()).abs() < 0.05 && (a.h() - b.h()).abs() < 0.05, "{a:?} vs {b:?}");
    }
    #[test]
    fn handle_rules() {
        let mut c = Curve { nodes: vec![Node::sharp([0.0, 0.0]), Node::sharp([10.0, 0.0]), Node::sharp([20.0, 0.0])], closed: false };
        c.set_kind(1, "symmetric");
        c.move_handle(1, true, [14.0, 3.0]);
        let n = &c.nodes[1];
        assert!(d(n.i, [6.0, -3.0]) < 1e-9, "symmetric mirrors the other handle");
        c.set_kind(1, "smooth");
        c.move_handle(1, true, [10.0, 8.0]);
        let n = &c.nodes[1];
        assert!((n.i[0] - 10.0).abs() < 1e-9 && n.i[1] < 0.0, "smooth keeps the handles in line");
        c.set_kind(1, "sharp");
        assert!(d(c.nodes[1].i, c.nodes[1].p) < 1e-12);
    }
}
