//! The design document: shapes, cut layers, selection and undo history.
//! The project file format is the same JSON the browser version wrote.

use crate::geom::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

/// 30 layer colors (our own palette)
pub const PALETTE: [&str; 30] = [
    "#000000", "#1f4fff", "#e8202a", "#1fa83a", "#c8a400", "#ff6a00", "#00a8c8", "#c000c0", "#8a8a8a", "#6a3cff",
    "#ff3c8a", "#5c7a00", "#a05a28", "#0060a0", "#7a0028", "#00805a", "#ff9e3c", "#3cc8ff", "#b4b400", "#a03cff",
    "#3c3cff", "#ff3c3c", "#3cb43c", "#783c00", "#5a5a9a", "#9a5a5a", "#5a9a5a", "#2a2a2a", "#d26e9a", "#6e9ad2",
];

pub fn palette_rgb(i: usize) -> [u8; 3] {
    let h = PALETTE[i % 30];
    let v = |k: usize| u8::from_str_radix(&h[k..k + 2], 16).unwrap_or(0);
    [v(1), v(3), v(5)]
}

pub fn nearest_palette(rgb: [u8; 3]) -> usize {
    (0..30)
        .min_by_key(|&i| {
            let p = palette_rgb(i);
            (0..3).map(|k| (p[k] as i32 - rgb[k] as i32).pow(2)).sum::<i32>()
        })
        .unwrap_or(0)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Layer {
    pub index: usize,
    pub name: String,
    pub mode: String, // Line | Fill | Image
    pub speed: f64,   // mm/s
    pub power: f64,   // %
    pub min_power: f64,
    pub passes: u32,
    pub interval: f64, // mm
    pub bidir: bool,
    pub cross_hatch: bool,
    pub overscan: f64, // % of speed
    pub air: bool,
    pub output: bool,
    pub show: bool,
    pub dither: String,
    pub invert: bool,
    /// name of the material profile last applied to this layer (empty when none)
    pub material: String,
    // ---- router (CNC) settings; kept apart from the laser settings above
    /// Profile outside | Profile inside | On line | Pocket | Drill
    pub op: String,
    pub tool_id: u32,
    /// total cut depth below the stock top, mm
    pub depth: f64,
    /// depth per pass, mm
    pub stepdown: f64,
    /// mm/min
    pub feed: f64,
    /// mm/min
    pub plunge: f64,
    pub rpm: f64,
    /// % of tool diameter between pocket rings
    pub stepover: f64,
    pub climb: bool,
    pub tabs: bool,
    pub tab_count: u32,
    pub tab_width: f64,
    pub tab_height: f64,
    /// speeds & feeds entry last applied (empty when none)
    pub cnc_material: String,
}
impl Default for Layer {
    fn default() -> Self {
        Layer::new(0)
    }
}
impl Layer {
    pub fn new(i: usize) -> Self {
        Layer {
            index: i,
            name: format!("C{:02}", i),
            mode: "Line".into(),
            speed: 100.0,
            power: 20.0,
            min_power: 10.0,
            passes: 1,
            interval: 0.1,
            bidir: true,
            cross_hatch: false,
            overscan: 2.5,
            air: true,
            output: true,
            show: true,
            dither: "jarvis".into(),
            invert: false,
            material: String::new(),
            op: "Profile outside".into(),
            tool_id: 2,
            depth: 3.0,
            stepdown: 1.5,
            feed: 1500.0,
            plunge: 500.0,
            rpm: 18000.0,
            stepover: 40.0,
            climb: true,
            tabs: false,
            tab_count: 4,
            tab_width: 6.0,
            tab_height: 2.0,
            cnc_material: String::new(),
        }
    }
}

fn default_sides() -> u32 {
    6
}
fn default_size() -> f64 {
    12.0
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Shape {
    #[serde(default)]
    pub id: u64,
    #[serde(rename = "type")]
    pub kind: String, // rect | ellipse | polygon | path | text | image
    #[serde(default = "ident")]
    pub tf: M,
    #[serde(default)]
    pub layer: usize,
    #[serde(default)]
    pub w: f64,
    #[serde(default)]
    pub h: f64,
    #[serde(default)]
    pub r: f64,
    #[serde(default = "default_sides")]
    pub sides: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subpaths: Vec<Contour>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub font: String,
    #[serde(default = "default_size")]
    pub size: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub align: String,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    /// picture as a data URL (shared between undo copies)
    #[serde(default, skip_serializing_if = "str_empty")]
    pub src: Arc<str>,
    #[serde(default)]
    pub invert: bool,
    /// curves with points and handles (kind "spline"), in world coordinates
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub curves: Vec<crate::spline::Curve>,
}
fn ident() -> M {
    IDENT
}
fn str_empty(s: &Arc<str>) -> bool {
    s.is_empty()
}

impl Shape {
    pub fn new(kind: &str) -> Shape {
        Shape { kind: kind.into(), tf: IDENT, sides: 6, size: 12.0, ..Default::default() }
    }
    pub fn local_contours(&self) -> Vec<Contour> {
        let (w, h) = (self.w, self.h);
        match self.kind.as_str() {
            "rect" => {
                let r = self.r.min(w / 2.0).min(h / 2.0);
                if r <= 0.0 {
                    return vec![Contour { pts: vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h], [0.0, 0.0]], closed: true }];
                }
                use std::f64::consts::PI;
                let mut pts = ellipse_pts(w - r, r, r, r, -PI / 2.0, 0.0);
                pts.extend(ellipse_pts(w - r, h - r, r, r, 0.0, PI / 2.0));
                pts.extend(ellipse_pts(r, h - r, r, r, PI / 2.0, PI));
                pts.extend(ellipse_pts(r, r, r, r, PI, PI * 1.5));
                pts.push(pts[0]);
                vec![Contour { pts, closed: true }]
            }
            "ellipse" => {
                let mut pts = ellipse_pts(w / 2.0, h / 2.0, w / 2.0, h / 2.0, 0.0, std::f64::consts::TAU);
                let first = pts[0];
                if let Some(l) = pts.last_mut() {
                    *l = first;
                }
                vec![Contour { pts, closed: true }]
            }
            "polygon" => {
                let n = self.sides.max(3) as usize;
                let pts = (0..=n)
                    .map(|i| {
                        let a = std::f64::consts::FRAC_PI_2 + (i % n) as f64 * std::f64::consts::TAU / n as f64;
                        [w / 2.0 + w / 2.0 * a.cos(), h / 2.0 + h / 2.0 * a.sin()]
                    })
                    .collect();
                vec![Contour { pts, closed: true }]
            }
            "path" | "text" => self.subpaths.clone(),
            "spline" => self.curves.iter().map(|c| c.flatten()).filter(|c| c.pts.len() > 1).collect(),
            "image" => vec![Contour { pts: vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h], [0.0, 0.0]], closed: true }],
            _ => vec![],
        }
    }
    pub fn world_contours(&self) -> Vec<Contour> {
        self.local_contours()
            .into_iter()
            .map(|c| Contour { pts: c.pts.iter().map(|p| apply(&self.tf, *p)).collect(), closed: c.closed })
            .collect()
    }
    pub fn bbox(&self) -> Option<BBox> {
        bbox_of(&self.world_contours())
    }
    /// A key that changes whenever the outline changes (for caches)
    pub fn geom_key(&self) -> String {
        format!(
            "{}|{:?}|{}|{}|{}|{}|{}|{}",
            self.kind,
            self.tf,
            self.w,
            self.h,
            self.r,
            self.sides,
            self.subpaths.len(),
            self.subpaths.first().map(|c| c.pts.len()).unwrap_or(0)
        ) + &self.curves_sum()
    }
    /// Fold the scaling in `tf` into the shape's own size, so a stretched rectangle keeps round
    /// corners and resized text gets a new text size. Rotation, mirroring and slant stay in `tf`.
    /// Text keeps any width-vs-height difference in `tf` (see `text_stretch`).
    fn curves_sum(&self) -> String {
        if self.curves.is_empty() {
            return String::new();
        }
        let mut s = 0.0;
        let mut k = 1.0;
        for c in &self.curves {
            s += if c.closed { 7.0 } else { 3.0 } * k;
            for n in &c.nodes {
                for q in [n.p, n.i, n.o] {
                    s += (q[0] * 1.37 + q[1] * 2.11) * k;
                    k += 1.0;
                }
                s += n.kind.len() as f64 * k;
            }
        }
        format!("|c{}:{}", self.curves.len(), s)
    }
    pub fn bake_scale(&mut self) {
        if self.kind == "spline" {
            // curves keep everything in their points
            if self.tf != IDENT {
                let m = self.tf;
                for c in self.curves.iter_mut() {
                    for n in c.nodes.iter_mut() {
                        n.transform(&m);
                    }
                }
                self.tf = IDENT;
            }
            return;
        }
        let Some((q, sx, shear, sy)) = qr(&self.tf) else { return };
        if (sx - 1.0).abs() < 1e-9 && (sy - 1.0).abs() < 1e-9 {
            return;
        }
        let with = |kx: f64, sh: f64| -> M {
            // new linear part = Q * [[kx, sh], [0, 1]] after taking (sx', sy) into the geometry
            let l = [q[0] * kx, q[1] * kx, q[0] * sh + q[2], q[1] * sh + q[3]];
            [l[0], l[1], l[2], l[3], 0.0, 0.0]
        };
        match self.kind.as_str() {
            "rect" | "ellipse" | "polygon" | "image" => {
                self.w *= sx;
                self.h *= sy;
                let l = with(1.0, shear / sy);
                self.tf = [l[0], l[1], l[2], l[3], self.tf[4], self.tf[5]];
            }
            "text" => {
                // the height sets the text size; any extra width stays as a visible stretch
                let k = sy;
                self.size *= k;
                self.w *= k;
                self.h *= k;
                for c in self.subpaths.iter_mut() {
                    for p in c.pts.iter_mut() {
                        p[0] *= k;
                        p[1] *= k;
                    }
                }
                let l = with(sx / sy, shear / sy);
                self.tf = [l[0], l[1], l[2], l[3], self.tf[4], self.tf[5]];
            }
            _ => {}
        }
    }
    /// For text whose width was stretched (or slanted) apart from its height:
    /// (width as a share of the normal width, slant). None when proportions are standard.
    pub fn text_stretch(&self) -> Option<(f64, f64)> {
        if self.kind != "text" {
            return None;
        }
        let (_, sx, shear, sy) = qr(&self.tf)?;
        let ratio = sx / sy;
        if (ratio - 1.0).abs() > 0.005 || (shear / sy).abs() > 0.005 {
            Some((ratio, shear / sy))
        } else {
            None
        }
    }
    /// Put text back to normal proportions (keeps its height, position and rotation)
    pub fn unstretch_text(&mut self) {
        let Some((q, _sx, _shear, sy)) = qr(&self.tf) else { return };
        let b0 = self.bbox();
        self.tf = [q[0] * sy, q[1] * sy, q[2] * sy, q[3] * sy, self.tf[4], self.tf[5]];
        self.bake_scale();
        // keep it centered where it was
        if let (Some(a), Some(b)) = (b0, self.bbox()) {
            self.tf[4] += a.cx() - b.cx();
            self.tf[5] += a.cy() - b.cy();
        }
    }
    pub fn hit(&self, p: P, tol: f64, cs: &[Contour]) -> bool {
        for c in cs {
            for i in 1..c.pts.len() {
                if dist_to_seg(p, c.pts[i - 1], c.pts[i]) <= tol {
                    return true;
                }
            }
        }
        self.kind == "image" && cs.first().map(|c| point_in_poly(p, &c.pts)).unwrap_or(false)
    }
}

/// Split a matrix's linear part into Q (rotation, maybe mirrored) times [[sx, shear], [0, sy]]
/// with sx, sy > 0. None for a degenerate matrix.
pub fn qr(m: &M) -> Option<([f64; 4], f64, f64, f64)> {
    let (a0, a1, c0, c1) = (m[0], m[1], m[2], m[3]);
    let sx = a0.hypot(a1);
    if sx < 1e-9 {
        return None;
    }
    let (q0, q1) = (a0 / sx, a1 / sx);
    let shear = q0 * c0 + q1 * c1;
    let (r0, r1) = (c0 - shear * q0, c1 - shear * q1);
    let sy = r0.hypot(r1);
    if sy < 1e-9 {
        return None;
    }
    Some(([q0, q1, r0 / sy, r1 / sy], sx, shear, sy))
}

#[derive(Clone)]
pub struct Snapshot {
    shapes: Vec<Shape>,
    layers: Vec<Layer>,
    order: Vec<usize>,
}

#[derive(Serialize, Deserialize)]
pub struct ProjectFile {
    #[serde(default)]
    pub app: String,
    #[serde(default)]
    pub version: u32,
    pub shapes: Vec<Shape>,
    #[serde(default)]
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub order: Vec<usize>,
}

pub struct Doc {
    pub shapes: Vec<Shape>,
    pub layers: Vec<Layer>,
    pub order: Vec<usize>,
    pub sel: HashSet<u64>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    next_id: u64,
    pub dirty: bool,
}

impl Doc {
    pub fn new() -> Doc {
        Doc { shapes: vec![], layers: (0..30).map(Layer::new).collect(), order: vec![], sel: HashSet::new(), undo: vec![], redo: vec![], next_id: 1, dirty: false }
    }
    pub fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id - 1
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot { shapes: self.shapes.clone(), layers: self.layers.clone(), order: self.order.clone() }
    }
    fn restore(&mut self, s: Snapshot) {
        self.shapes = s.shapes;
        self.layers = s.layers;
        self.order = s.order;
        let ids: HashSet<u64> = self.shapes.iter().map(|s| s.id).collect();
        self.sel.retain(|i| ids.contains(i));
    }
    pub fn checkpoint(&mut self) {
        self.undo.push(self.snapshot());
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }
    pub fn undo(&mut self) -> bool {
        if let Some(s) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.restore(s);
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self) -> bool {
        if let Some(s) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore(s);
            true
        } else {
            false
        }
    }
    /// Layers in use, in cut order
    pub fn used_layers(&mut self) -> Vec<usize> {
        let used: HashSet<usize> = self.shapes.iter().map(|s| s.layer).collect();
        self.order.retain(|i| used.contains(i));
        let mut u: Vec<usize> = used.into_iter().collect();
        u.sort();
        for i in u {
            if !self.order.contains(&i) {
                self.order.push(i);
            }
        }
        self.order.clone()
    }
    pub fn add(&mut self, mut s: Shape) -> u64 {
        if s.id == 0 {
            s.id = self.new_id();
        }
        if s.kind == "image" && self.layers[s.layer].mode != "Image" {
            self.layers[s.layer].mode = "Image".into();
        }
        let id = s.id;
        self.shapes.push(s);
        id
    }
    pub fn selected(&self) -> Vec<&Shape> {
        self.shapes.iter().filter(|s| self.sel.contains(&s.id)).collect()
    }
    pub fn sel_box(&self) -> Option<BBox> {
        self.shapes.iter().filter(|s| self.sel.contains(&s.id)).fold(None, |b, s| BBox::union(b, s.bbox()))
    }
    pub fn all_box(&self) -> Option<BBox> {
        self.shapes.iter().fold(None, |b, s| BBox::union(b, s.bbox()))
    }
    pub fn transform_sel(&mut self, m: &M) {
        for s in self.shapes.iter_mut() {
            if self.sel.contains(&s.id) {
                s.tf = mul(m, &s.tf);
                s.bake_scale();
            }
        }
    }
    pub fn delete_sel(&mut self) {
        let sel = std::mem::take(&mut self.sel);
        self.shapes.retain(|s| !sel.contains(&s.id));
    }
    pub fn to_file(&self) -> ProjectFile {
        ProjectFile { app: "cnc-anything".into(), version: 2, shapes: self.shapes.clone(), layers: self.layers.clone(), order: self.order.clone() }
    }
    pub fn load(&mut self, f: ProjectFile) {
        self.shapes = f.shapes;
        let mut max = 0;
        for s in self.shapes.iter_mut() {
            if s.id == 0 {
                max += 1;
                s.id = 1_000_000 + max;
            }
        }
        self.next_id = self.shapes.iter().map(|s| s.id).max().unwrap_or(0) + 1;
        self.layers = (0..30)
            .map(|i| {
                let mut l = f.layers.get(i).cloned().unwrap_or_else(|| Layer::new(i));
                l.index = i;
                l
            })
            .collect();
        self.order = f.order;
        self.sel.clear();
        self.undo.clear();
        self.redo.clear();
        self.dirty = false;
    }
    pub fn clear(&mut self) {
        self.load(ProjectFile { app: String::new(), version: 2, shapes: vec![], layers: vec![], order: vec![] });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stretched_rectangle_keeps_round_corners() {
        let mut s = Shape::new("rect");
        s.w = 20.0;
        s.h = 10.0;
        s.r = 3.0;
        s.tf = mul(&scale_about(2.0, 1.0, 0.0, 0.0), &translate(5.0, 5.0));
        s.bake_scale();
        assert!((s.w - 40.0).abs() < 1e-9 && (s.h - 10.0).abs() < 1e-9 && s.r == 3.0);
        assert!((s.tf[0] - 1.0).abs() < 1e-9 && (s.tf[3] - 1.0).abs() < 1e-9, "no stretch left in the matrix: {:?}", s.tf);
        let b = s.bbox().unwrap();
        assert!((b.x0 - 10.0).abs() < 1e-9 && (b.w() - 40.0).abs() < 1e-9);
        // mirrored stays mirrored, size stays positive
        s.tf = mul(&scale_about(-1.0, 1.0, 0.0, 0.0), &s.tf);
        s.bake_scale();
        assert!(s.w > 0.0 && (s.w - 40.0).abs() < 1e-9);
    }
    #[test]
    fn resized_text_changes_size_and_stretch_is_reported() {
        let mut t = Shape::new("text");
        t.size = 10.0;
        t.w = 30.0;
        t.h = 8.0;
        t.subpaths = vec![Contour { pts: vec![[0.0, 0.0], [30.0, 0.0], [30.0, 8.0], [0.0, 0.0]], closed: true }];
        t.tf = scale_about(1.5, 1.5, 0.0, 0.0);
        t.bake_scale();
        assert!((t.size - 15.0).abs() < 1e-9 && t.text_stretch().is_none());
        t.tf = mul(&scale_about(2.0, 1.0, 0.0, 0.0), &t.tf);
        t.bake_scale();
        assert!((t.size - 15.0).abs() < 1e-9, "height unchanged, so the size is too");
        let (ratio, _) = t.text_stretch().expect("stretch is reported");
        assert!((ratio - 2.0).abs() < 1e-9);
        t.unstretch_text();
        assert!(t.text_stretch().is_none() && (t.size - 15.0).abs() < 1e-9);
    }
}
