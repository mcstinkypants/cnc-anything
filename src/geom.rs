//! Geometry: affine matrices, outlines, bounding boxes, hit tests and scanline fills.
//! World units are millimeters with Y pointing up from the front edge of the bed.

use serde::{Deserialize, Serialize};

pub type P = [f64; 2];
/// Affine matrix [a, b, c, d, e, f]: x' = a*x + c*y + e, y' = b*x + d*y + f
pub type M = [f64; 6];
pub const IDENT: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// m * n: apply n first, then m
pub fn mul(m: &M, n: &M) -> M {
    [
        m[0] * n[0] + m[2] * n[1],
        m[1] * n[0] + m[3] * n[1],
        m[0] * n[2] + m[2] * n[3],
        m[1] * n[2] + m[3] * n[3],
        m[0] * n[4] + m[2] * n[5] + m[4],
        m[1] * n[4] + m[3] * n[5] + m[5],
    ]
}
pub fn apply(m: &M, p: P) -> P {
    [m[0] * p[0] + m[2] * p[1] + m[4], m[1] * p[0] + m[3] * p[1] + m[5]]
}
pub fn inv(m: &M) -> M {
    let mut det = m[0] * m[3] - m[1] * m[2];
    if det.abs() < 1e-12 {
        det = 1e-12;
    }
    let (a, b, c, d) = (m[3] / det, -m[1] / det, -m[2] / det, m[0] / det);
    [a, b, c, d, -(a * m[4] + c * m[5]), -(b * m[4] + d * m[5])]
}
pub fn translate(x: f64, y: f64) -> M {
    [1.0, 0.0, 0.0, 1.0, x, y]
}
pub fn scale_about(sx: f64, sy: f64, cx: f64, cy: f64) -> M {
    [sx, 0.0, 0.0, sy, cx - sx * cx, cy - sy * cy]
}
pub fn rotate_about(deg: f64, cx: f64, cy: f64) -> M {
    let r = deg.to_radians();
    let (s, c) = r.sin_cos();
    [c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy]
}
pub fn mat_rotation(m: &M) -> f64 {
    m[1].atan2(m[0]).to_degrees()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Contour {
    pub pts: Vec<P>,
    #[serde(default)]
    pub closed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BBox {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}
impl BBox {
    pub fn w(&self) -> f64 {
        self.x1 - self.x0
    }
    pub fn h(&self) -> f64 {
        self.y1 - self.y0
    }
    pub fn cx(&self) -> f64 {
        (self.x0 + self.x1) / 2.0
    }
    pub fn cy(&self) -> f64 {
        (self.y0 + self.y1) / 2.0
    }
    pub fn union(a: Option<BBox>, b: Option<BBox>) -> Option<BBox> {
        match (a, b) {
            (None, x) | (x, None) => x,
            (Some(a), Some(b)) => Some(BBox {
                x0: a.x0.min(b.x0),
                y0: a.y0.min(b.y0),
                x1: a.x1.max(b.x1),
                y1: a.y1.max(b.y1),
            }),
        }
    }
    pub fn add_point(b: Option<BBox>, p: P) -> Option<BBox> {
        BBox::union(b, Some(BBox { x0: p[0], y0: p[1], x1: p[0], y1: p[1] }))
    }
}

pub fn bbox_of(cs: &[Contour]) -> Option<BBox> {
    let mut b: Option<BBox> = None;
    for c in cs {
        for p in &c.pts {
            b = BBox::add_point(b, *p);
        }
    }
    b
}

/// Points along an elliptical arc (angles in radians)
pub fn ellipse_pts(cx: f64, cy: f64, rx: f64, ry: f64, a0: f64, a1: f64) -> Vec<P> {
    let per = std::f64::consts::PI * (rx + ry) * (a1 - a0).abs() / (std::f64::consts::PI * 2.0);
    let n = ((per / 0.4).ceil() as usize).max(12);
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f64 / n as f64;
            [cx + rx * a.cos(), cy + ry * a.sin()]
        })
        .collect()
}

pub fn point_in_poly(pt: P, pts: &[P]) -> bool {
    let mut inside = false;
    let n = pts.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (pts[i][0], pts[i][1]);
        let (xj, yj) = (pts[j][0], pts[j][1]);
        if (yi > pt[1]) != (yj > pt[1]) && pt[0] < (xj - xi) * (pt[1] - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

pub fn dist_to_seg(p: P, a: P, b: P) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l = dx * dx + dy * dy;
    let t = if l > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l).clamp(0.0, 1.0) } else { 0.0 };
    ((p[0] - (a[0] + t * dx)).powi(2) + (p[1] - (a[1] + t * dy)).powi(2)).sqrt()
}

/// Ramer-Douglas-Peucker simplification
pub fn simplify(pts: &[P], tol: f64) -> Vec<P> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0usize, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let mut md = 0.0;
        let mut mi = 0;
        for i in a + 1..b {
            let d = dist_to_seg(pts[i], pts[a], pts[b]);
            if d > md {
                md = d;
                mi = i;
            }
        }
        if md > tol && mi > 0 {
            keep[mi] = true;
            stack.push((a, mi));
            stack.push((mi, b));
        }
    }
    pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

pub struct Row {
    pub y: f64,
    pub segs: Vec<(f64, f64)>,
}

/// Even-odd fill of closed outlines with lines `interval` mm apart (bottom to top).
/// With `vertical`, x and y are swapped (rows run along X = const).
pub fn scan_fill(cs: &[Contour], interval: f64, vertical: bool) -> Vec<Row> {
    let polys: Vec<Vec<P>> = cs
        .iter()
        .filter(|c| c.closed && c.pts.len() > 2)
        .map(|c| if vertical { c.pts.iter().map(|p| [p[1], p[0]]).collect() } else { c.pts.clone() })
        .collect();
    let mut bb: Option<BBox> = None;
    for p in &polys {
        for q in p {
            bb = BBox::add_point(bb, *q);
        }
    }
    let Some(bb) = bb else { return vec![] };
    if interval <= 0.0 {
        return vec![];
    }
    let mut edges: Vec<[f64; 4]> = Vec::new();
    for pts in &polys {
        for i in 1..pts.len() {
            let (a, b) = (pts[i - 1], pts[i]);
            if a[1] == b[1] {
                continue;
            }
            edges.push(if a[1] < b[1] { [a[0], a[1], b[0], b[1]] } else { [b[0], b[1], a[0], a[1]] });
        }
    }
    edges.sort_by(|e, f| e[1].partial_cmp(&f[1]).unwrap_or(std::cmp::Ordering::Equal));
    let mut rows = Vec::new();
    let mut ei = 0;
    let mut active: Vec<[f64; 4]> = Vec::new();
    let mut y = bb.y0 + interval / 2.0;
    while y <= bb.y1 {
        while ei < edges.len() && edges[ei][1] <= y {
            active.push(edges[ei]);
            ei += 1;
        }
        active.retain(|e| e[3] > y);
        let mut xs: Vec<f64> = active.iter().map(|e| e[0] + (y - e[1]) * (e[2] - e[0]) / (e[3] - e[1])).collect();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut segs = Vec::new();
        let mut k = 0;
        while k + 1 < xs.len() {
            if xs[k + 1] - xs[k] > 1e-6 {
                segs.push((xs[k], xs[k + 1]));
            }
            k += 2;
        }
        if !segs.is_empty() {
            rows.push(Row { y, segs });
        }
        y += interval;
    }
    rows
}

/// How many closed outlines each outline sits inside (inner shapes get higher numbers)
pub fn containment_depth(cs: &[Contour]) -> Vec<usize> {
    let boxes: Vec<Option<BBox>> = cs.iter().map(|c| bbox_of(std::slice::from_ref(c))).collect();
    (0..cs.len())
        .map(|i| {
            if !cs[i].closed {
                return 0;
            }
            let Some(a) = boxes[i] else { return 0 };
            let mut d = 0;
            for j in 0..cs.len() {
                if i == j || !cs[j].closed {
                    continue;
                }
                let Some(b) = boxes[j] else { continue };
                if a.x0 >= b.x0 && a.x1 <= b.x1 && a.y0 >= b.y0 && a.y1 <= b.y1 && (a.w() < b.w() || a.h() < b.h()) && point_in_poly(cs[i].pts[0], &cs[j].pts) {
                    d += 1;
                }
            }
            d
        })
        .collect()
}
