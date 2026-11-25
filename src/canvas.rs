//! The design canvas: drawing the bed and shapes, and the mouse tools.

use crate::gcode::{img_key, Device, PreviewSeg};
use crate::geom::*;
use crate::model::{palette_rgb, Doc, Shape};
use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use std::collections::{HashMap, HashSet};

pub const RULER: f32 = 22.0;
const HANDLE: f32 = 7.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Line,
    Rect,
    Ellipse,
    Polygon,
    Text,
    Pan,
    Measure,
    Spline,
    Nodes,
}
impl Tool {
    pub const ALL: [Tool; 10] = [Tool::Select, Tool::Nodes, Tool::Line, Tool::Spline, Tool::Rect, Tool::Ellipse, Tool::Polygon, Tool::Text, Tool::Pan, Tool::Measure];
    pub fn label(&self) -> &'static str {
        match self {
            Tool::Select => "Select (V)",
            Tool::Line => "Draw lines (L)",
            Tool::Rect => "Rectangle (R)",
            Tool::Ellipse => "Ellipse (E)",
            Tool::Polygon => "Polygon (P)",
            Tool::Text => "Text (T)",
            Tool::Pan => "Pan (H, or hold Space / middle mouse)",
            Tool::Measure => "Measure (M)",
            Tool::Spline => "Spline (S): click for sharp points, drag for curves; click the first point to close, Enter or double-click to finish",
            Tool::Nodes => "Edit points (N): drag points and handles, double-click a segment to add a point, Delete to remove one",
        }
    }
}

/// Layer color as drawn on the canvas: very dark layers turn light on the dark canvas
pub fn ink_for(i: usize, dark: bool) -> Color32 {
    let c = layer_color(i);
    let lum = 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
    if dark && lum < 70.0 {
        Color32::from_gray(0xd8)
    } else {
        c
    }
}

pub fn layer_color(i: usize) -> Color32 {
    let c = palette_rgb(i);
    Color32::from_rgb(c[0], c[1], c[2])
}

#[derive(Clone)]
pub enum Drag {
    Pan { start: Pos2, ox: f32, oy: f32 },
    Box { start: Pos2, cur: Pos2 },
    Move { start: P, orig: Vec<(u64, M)>, moved: bool, bx: BBox, centers: Vec<P> },
    Scale { handle: &'static str, bx: BBox, orig: Vec<Shape> },
    Rotate { start: P, bx: BBox, orig: Vec<(u64, M)> },
    Create { start: P, ghost: Option<Shape> },
    Measure,
}

/// What the canvas asks the app to do after a frame
pub enum CanvasEvent {
    AddShape(Shape),
    Edited,
    SelectionChanged,
    TextAt(P),
    EditText(u64),
    Message(String),
}

pub struct Preview {
    pub segs: Vec<PreviewSeg>,
    pub frac: f32,
    pub travel: bool,
    pub info: String,
    /// router job: segment "power" is the depth in mm; each layer's tool diameter (mm)
    pub router: bool,
    pub widths: Vec<f32>,
    pub max_depth: f64,
}

struct Cached {
    key: String,
    cs: Vec<Contour>,
    bb: Option<BBox>,
    hatch_key: f64,
    hatch: Vec<[P; 2]>,
}

pub struct View {
    pub z: f32,
    pub ox: f32,
    pub oy: f32,
    pub rect: Rect,
    pub tool: Tool,
    pub drag: Option<Drag>,
    pub mouse: P,
    pub mouse_screen: Pos2,
    pub line_pts: Option<Vec<P>>,
    pub measure: Option<[P; 2]>,
    pub guides: Vec<(bool, f64)>, // (vertical?, coordinate)
    pub snap_mark: Option<P>,
    pub head: Option<P>,
    pub frame_box: Option<BBox>,
    pub preview: Option<Preview>,
    pub fitted: bool,
    pub space: bool,
    /// dark canvas: a dark bed with light outlines
    pub dark: bool,
    /// spline being drawn (world coordinates)
    pub spline_new: Option<Vec<crate::spline::Node>>,
    /// selected curve point in the Edit-points tool
    pub node_sel: Option<crate::canvas_nodes::NodeRef>,
    pub node_drag: Option<crate::canvas_nodes::NodeDrag>,
    /// router machine: layers show their routing operation instead of laser modes
    pub router: bool,
    cache: HashMap<u64, Cached>,
}

pub struct SnapCfg {
    pub grid: bool,
    pub obj: bool,
    pub step: f64,
    pub lock_ratio: bool,
    pub poly_sides: u32,
}

impl View {
    pub fn new() -> View {
        View {
            z: 2.0,
            ox: 60.0,
            oy: 600.0,
            rect: Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)),
            tool: Tool::Select,
            drag: None,
            mouse: [0.0, 0.0],
            mouse_screen: Pos2::ZERO,
            line_pts: None,
            measure: None,
            guides: vec![],
            snap_mark: None,
            head: None,
            frame_box: None,
            preview: None,
            fitted: false,
            space: false,
            dark: false,
            spline_new: None,
            node_sel: None,
            node_drag: None,
            router: false,
            cache: HashMap::new(),
        }
    }
    pub fn to_screen(&self, p: P) -> Pos2 {
        Pos2::new(self.rect.min.x + self.ox + p[0] as f32 * self.z, self.rect.min.y + self.oy - p[1] as f32 * self.z)
    }
    pub fn to_world(&self, s: Pos2) -> P {
        [((s.x - self.rect.min.x - self.ox) / self.z) as f64, ((self.rect.min.y + self.oy - s.y) / self.z) as f64]
    }
    pub fn zoom_to(&mut self, b: BBox, pad: f32) {
        let aw = self.rect.width() - RULER - pad * 2.0;
        let ah = self.rect.height() - RULER - pad * 2.0;
        if aw <= 0.0 || ah <= 0.0 {
            return;
        }
        self.z = (aw / b.w().max(1.0) as f32).min(ah / b.h().max(1.0) as f32).min(200.0);
        self.ox = RULER + pad + (aw - b.w() as f32 * self.z) / 2.0 - b.x0 as f32 * self.z;
        self.oy = RULER + pad + (ah - b.h() as f32 * self.z) / 2.0 + b.y1 as f32 * self.z;
    }
    pub fn zoom_fit(&mut self, dev: &Device) {
        self.zoom_to(BBox { x0: 0.0, y0: 0.0, x1: dev.bed_w, y1: dev.bed_h }, 30.0);
    }
    pub fn zoom_at(&mut self, f: f32, s: Pos2) {
        let w = self.to_world(s);
        self.z = (self.z * f).clamp(0.05, 400.0);
        self.ox = s.x - self.rect.min.x - w[0] as f32 * self.z;
        self.oy = s.y - self.rect.min.y + w[1] as f32 * self.z;
    }
    pub fn grid_step(&self) -> f64 {
        let want = 12.0 / self.z as f64;
        for s in [0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0] {
            if s >= want {
                return s;
            }
        }
        500.0
    }
    fn cached(&mut self, s: &Shape) -> &mut Cached {
        let key = s.geom_key();
        let stale = self.cache.get(&s.id).map(|c| c.key != key).unwrap_or(true);
        if stale {
            let cs = s.world_contours();
            let bb = bbox_of(&cs);
            self.cache.insert(s.id, Cached { key, cs, bb, hatch_key: -1.0, hatch: vec![] });
        }
        self.cache.get_mut(&s.id).unwrap()
    }
    pub fn contours(&mut self, s: &Shape) -> Vec<Contour> {
        self.cached(s).cs.clone()
    }
    pub fn bbox(&mut self, s: &Shape) -> Option<BBox> {
        self.cached(s).bb
    }
    pub fn forget_cache(&mut self) {
        self.cache.clear();
    }

    // ---------------------------------------------------------------- snapping
    fn snap_step(&self, cfg: &SnapCfg) -> f64 {
        if cfg.step > 0.0 {
            cfg.step
        } else {
            self.grid_step()
        }
    }
    fn snap_grid_pt(&self, w: P, cfg: &SnapCfg, dev: &Device) -> P {
        let s = self.snap_step(cfg);
        let u = dev.to_user(w);
        dev.from_user([(u[0] / s).round() * s, (u[1] / s).round() * s])
    }
    fn snap_centers(&mut self, doc: &Doc, exclude: Option<&HashSet<u64>>, dev: &Device) -> Vec<P> {
        let mut out = vec![[dev.bed_w / 2.0, dev.bed_h / 2.0]];
        for s in &doc.shapes {
            if exclude.map(|e| e.contains(&s.id)).unwrap_or(false) || !doc.layers[s.layer].show {
                continue;
            }
            if let Some(b) = self.bbox(s) {
                out.push([b.cx(), b.cy()]);
            }
        }
        out
    }
    pub(crate) fn snap_point(&mut self, w: P, alt: bool, doc: &Doc, exclude: Option<&HashSet<u64>>, cfg: &SnapCfg, dev: &Device) -> P {
        self.snap_mark = None;
        if alt {
            return w;
        }
        if cfg.obj {
            let tol = 9.0 / self.z as f64;
            let mut best: Option<P> = None;
            let mut bd = tol;
            for c in self.snap_centers(doc, exclude, dev) {
                let d = (c[0] - w[0]).hypot(c[1] - w[1]);
                if d < bd {
                    bd = d;
                    best = Some(c);
                }
            }
            if let Some(b) = best {
                self.snap_mark = Some(b);
                return b;
            }
        }
        if cfg.grid {
            return self.snap_grid_pt(w, cfg, dev);
        }
        w
    }

    // ---------------------------------------------------------------- painting
    pub fn paint(&mut self, painter: &egui::Painter, doc: &Doc, dev: &Device, textures: &HashMap<usize, egui::TextureHandle>, sel_box: Option<BBox>) {
        let r = self.rect;
        painter.rect_filled(r, 0.0, Color32::from_rgb(0x24, 0x26, 0x2a));
        let b0 = self.to_screen([0.0, dev.bed_h]);
        let b1 = self.to_screen([dev.bed_w, 0.0]);
        let bed = Rect::from_min_max(b0, b1);
        let (bed_fill, minor, major, edge) = if self.dark {
            (Color32::from_rgb(0x17, 0x19, 0x1c), Color32::from_rgb(0x22, 0x25, 0x2a), Color32::from_rgb(0x32, 0x36, 0x3d), Color32::from_gray(0x5a))
        } else {
            (Color32::from_rgb(0xf4, 0xf4, 0xf2), Color32::from_rgb(0xe2, 0xe2, 0xdf), Color32::from_rgb(0xc9, 0xc9, 0xc5), Color32::from_gray(0x88))
        };
        painter.rect_filled(bed, 0.0, bed_fill);
        let step = self.grid_step();
        for (st, col) in [(step, minor), (step * 5.0, major)] {
            let stroke = Stroke::new(1.0_f32, col);
            let mut x = 0.0;
            while x <= dev.bed_w + 1e-6 {
                let sx = self.to_screen([x, 0.0]).x.round() + 0.5;
                painter.line_segment([Pos2::new(sx, bed.min.y), Pos2::new(sx, bed.max.y)], stroke);
                x += st;
            }
            let mut uy = 0.0;
            while uy <= dev.bed_h + 1e-6 {
                let sy = self.to_screen(dev.from_user([0.0, uy])).y.round() + 0.5;
                painter.line_segment([Pos2::new(bed.min.x, sy), Pos2::new(bed.max.x, sy)], stroke);
                uy += st;
            }
        }
        painter.rect_stroke(bed, 0.0, Stroke::new(1.0_f32, edge));
        painter.circle_filled(self.to_screen(dev.from_user([0.0, 0.0])), 4.0, Color32::from_rgb(0xff, 0x6a, 0x2a));

        if self.preview.is_some() {
            self.paint_preview(painter, doc);
        } else {
            self.paint_shapes(painter, doc, textures);
        }
        self.paint_overlay(painter, doc, dev, sel_box);
        if self.preview.is_none() {
            self.paint_nodes(painter, doc);
        }
        self.paint_rulers(painter, dev);
    }

    fn paint_shapes(&mut self, painter: &egui::Painter, doc: &Doc, textures: &HashMap<usize, egui::TextureHandle>) {
        let hatch_mm = {
            let want = 5.0 / self.z as f64;
            let mut h = 0.05;
            while h < want {
                h *= 2.0;
            }
            h
        };
        for s in &doc.shapes {
            let l = &doc.layers[s.layer];
            if !l.show {
                continue;
            }
            let sel = doc.sel.contains(&s.id);
            if s.kind == "image" {
                if let Some(tex) = textures.get(&img_key(&s.src)) {
                    let corners = [[0.0, s.h], [s.w, s.h], [s.w, 0.0], [0.0, 0.0]].map(|p| self.to_screen(apply(&s.tf, p)));
                    let uv = [Pos2::new(0.0, 0.0), Pos2::new(1.0, 0.0), Pos2::new(1.0, 1.0), Pos2::new(0.0, 1.0)];
                    let tint = if l.output { Color32::WHITE } else { Color32::from_white_alpha(110) };
                    let mut mesh = egui::Mesh::with_texture(tex.id());
                    for i in 0..4 {
                        mesh.vertices.push(egui::epaint::Vertex { pos: corners[i], uv: uv[i], color: tint });
                    }
                    mesh.add_triangle(0, 1, 2);
                    mesh.add_triangle(0, 2, 3);
                    painter.add(egui::Shape::mesh(mesh));
                }
            }
            let col = ink_for(s.layer, self.dark);
            let col = if l.output { col } else { col.gamma_multiply(0.45) };
            let fill_mode = (if self.router { l.op == "Pocket" } else { l.mode != "Line" }) && s.kind != "image";
            let z = self.z;
            let c = self.cached(s);
            if fill_mode {
                if c.hatch_key != hatch_mm {
                    c.hatch_key = hatch_mm;
                    c.hatch = scan_fill(&c.cs, hatch_mm, false).into_iter().flat_map(|row| row.segs.into_iter().map(move |(a, b)| [[a, row.y], [b, row.y]])).collect();
                }
            }
            let hatch = if fill_mode { c.hatch.clone() } else { vec![] };
            let cs = c.cs.clone();
            let _ = z;
            if !hatch.is_empty() {
                let hs = Stroke::new(1.0_f32, col.gamma_multiply(0.35));
                for seg in &hatch {
                    painter.line_segment([self.to_screen(seg[0]), self.to_screen(seg[1])], hs);
                }
            }
            let stroke = Stroke::new(if sel { 1.8 } else { 1.2_f32 }, col);
            for c in &cs {
                let pts: Vec<Pos2> = c.pts.iter().map(|p| self.to_screen(*p)).collect();
                if sel {
                    painter.extend(egui::Shape::dashed_line(&pts, stroke, 5.0, 3.0));
                } else {
                    painter.add(egui::Shape::line(pts, stroke));
                }
            }
        }
        // text stretched wider or narrower than its font (or slanted) gets an orange tag
        for s in doc.shapes.iter().filter(|s| s.kind == "text" && doc.layers[s.layer].show) {
            let Some((ratio, slant)) = s.text_stretch() else { continue };
            let Some(b) = self.bbox(s) else { continue };
            let t = if (ratio - 1.0).abs() > 0.005 { format!("↔ {:.0}% width", ratio * 100.0) } else { format!("slanted {:.0}°", slant.atan().to_degrees()) };
            let galley = painter.layout_no_wrap(t, egui::FontId::proportional(11.0), Color32::BLACK);
            let p = self.to_screen([b.x1, b.y1]) + Vec2::new(4.0, -16.0);
            let r = Rect::from_min_size(p, galley.size() + Vec2::new(8.0, 3.0));
            painter.rect_filled(r, 3.0, Color32::from_rgb(0xff, 0xa0, 0x30));
            painter.galley(r.min + Vec2::new(4.0, 1.5), galley, Color32::BLACK);
        }
    }

    fn paint_preview(&self, painter: &egui::Painter, doc: &Doc) {
        let Some(pv) = &self.preview else { return };
        let upto = (pv.segs.len() as f32 * pv.frac) as usize;
        let mut head = None;
        for &(x0, y0, x1, y1, p, li) in &pv.segs[..upto.min(pv.segs.len())] {
            let a = self.to_screen([x0, y0]);
            let b = self.to_screen([x1, y1]);
            head = Some(b);
            if p > 0.0 && pv.router {
                // the cut as wide as the tool, darker the deeper it goes
                let v = (p / pv.max_depth.max(0.001)).clamp(0.0, 1.0) as f32;
                let c = if self.dark {
                    Color32::from_rgb((90.0 + 150.0 * v) as u8, (70.0 + 100.0 * v) as u8, (50.0 + 40.0 * v) as u8)
                } else {
                    Color32::from_rgb((205.0 - 120.0 * v) as u8, (170.0 - 110.0 * v) as u8, (120.0 - 80.0 * v) as u8)
                };
                let w = (pv.widths.get(li).copied().unwrap_or(3.0) * self.z).max(1.0);
                painter.line_segment([a, b], Stroke::new(w, c));
                if w > 3.0 {
                    painter.circle_filled(b, w / 2.0, c);
                }
            } else if p > 0.0 {
                let l = &doc.layers[li];
                if l.mode == "Line" {
                    painter.line_segment([a, b], Stroke::new(1.4_f32, ink_for(li, self.dark)));
                } else {
                    let shade = (255.0 - (p / l.power.max(1.0)) * 200.0).clamp(0.0, 255.0);
                    let c = if self.dark {
                        // burnt areas glow warm on the dark bed
                        let v = 1.0 - shade / 255.0;
                        Color32::from_rgb((70.0 + 185.0 * v) as u8, (45.0 + 120.0 * v) as u8, (25.0 + 50.0 * v) as u8)
                    } else {
                        Color32::from_rgb((shade * 0.45) as u8, (shade * 0.35) as u8, (shade * 0.3) as u8)
                    };
                    painter.line_segment([a, b], Stroke::new((l.interval as f32 * self.z).max(1.0), c));
                }
            } else if pv.travel {
                painter.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(0.8_f32, Color32::from_rgba_unmultiplied(220, 40, 40, 140)), 4.0, 3.0));
            }
        }
        if let Some(h) = head {
            painter.circle_stroke(h, 6.0, Stroke::new(2.0_f32, Color32::from_rgb(255, 42, 42)));
        }
    }

    pub fn handles(&self, b: BBox) -> Vec<(&'static str, Pos2)> {
        let p0 = self.to_screen([b.x0, b.y1]);
        let p1 = self.to_screen([b.x1, b.y0]);
        let (mx, my) = ((p0.x + p1.x) / 2.0, (p0.y + p1.y) / 2.0);
        vec![
            ("nw", Pos2::new(p0.x, p0.y)),
            ("n", Pos2::new(mx, p0.y)),
            ("ne", Pos2::new(p1.x, p0.y)),
            ("w", Pos2::new(p0.x, my)),
            ("e", Pos2::new(p1.x, my)),
            ("sw", Pos2::new(p0.x, p1.y)),
            ("s", Pos2::new(mx, p1.y)),
            ("se", Pos2::new(p1.x, p1.y)),
            ("rot", Pos2::new(mx, p0.y - 24.0)),
        ]
    }

    fn label(&self, painter: &egui::Painter, text: &str, pos: Pos2) {
        let galley = painter.layout_no_wrap(text.to_string(), egui::FontId::proportional(12.0), Color32::WHITE);
        let r = Rect::from_min_size(pos - Vec2::new(0.0, 13.0), galley.size() + Vec2::new(10.0, 4.0));
        painter.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(0x1f, 0x21, 0x24, 235));
        painter.galley(r.min + Vec2::new(5.0, 2.0), galley, Color32::WHITE);
    }

    fn paint_overlay(&mut self, painter: &egui::Painter, doc: &Doc, dev: &Device, sel_box: Option<BBox>) {
        let blue = Color32::from_rgb(0x2f, 0x6f, 0xe0);
        if self.preview.is_none() && self.tool == Tool::Select {
            if let Some(b) = sel_box {
                let r = Rect::from_min_max(self.to_screen([b.x0, b.y1]), self.to_screen([b.x1, b.y0])).expand(0.5);
                let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.0_f32, blue), 3.0, 3.0));
                for (k, p) in self.handles(b) {
                    if k == "rot" {
                        painter.line_segment([p + Vec2::new(0.0, 5.0), Pos2::new(p.x, r.min.y)], Stroke::new(1.0_f32, blue));
                        painter.circle(p, 5.0, Color32::from_rgb(0xff, 0x6a, 0x2a), Stroke::new(1.0_f32, blue));
                    } else {
                        let hr = Rect::from_center_size(p, Vec2::splat(HANDLE));
                        painter.rect_filled(hr, 0.0, Color32::WHITE);
                        painter.rect_stroke(hr, 0.0, Stroke::new(1.0_f32, blue));
                    }
                }
                let c = r.center();
                painter.line_segment([c - Vec2::new(4.0, 0.0), c + Vec2::new(4.0, 0.0)], Stroke::new(1.0_f32, blue));
                painter.line_segment([c - Vec2::new(0.0, 4.0), c + Vec2::new(0.0, 4.0)], Stroke::new(1.0_f32, blue));
            }
        }
        match &self.drag {
            Some(Drag::Box { start, cur }) => {
                let ltr = cur.x >= start.x;
                let col = if ltr { blue } else { Color32::from_rgb(0x2f, 0xa0, 0x50) };
                let r = Rect::from_two_pos(*start, *cur);
                painter.rect_filled(r, 0.0, col.gamma_multiply(0.1));
                if ltr {
                    painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, col));
                } else {
                    let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                    painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.0_f32, col), 5.0, 3.0));
                }
            }
            Some(Drag::Create { ghost: Some(g), .. }) => {
                for c in g.world_contours() {
                    painter.add(egui::Shape::line(c.pts.iter().map(|p| self.to_screen(*p)).collect(), Stroke::new(1.2_f32, ink_for(g.layer, self.dark))));
                }
                if let Some(b) = g.bbox() {
                    self.label(painter, &format!("{:.1} × {:.1} mm", b.w(), b.h()), self.mouse_screen + Vec2::new(14.0, 18.0));
                }
            }
            _ => {}
        }
        if let Some(lp) = &self.line_pts {
            let mut pts: Vec<Pos2> = lp.iter().map(|p| self.to_screen(*p)).collect();
            pts.push(self.to_screen(self.mouse));
            painter.add(egui::Shape::line(pts, Stroke::new(1.2_f32, Color32::from_rgb(0xe8, 0x20, 0x2a))));
            painter.rect_stroke(Rect::from_center_size(self.to_screen(lp[0]), Vec2::splat(8.0)), 0.0, Stroke::new(1.0_f32, blue));
        }
        if let Some([a, b]) = self.measure {
            let (sa, sb) = (self.to_screen(a), self.to_screen(b));
            painter.line_segment([sa, sb], Stroke::new(1.5_f32, Color32::from_rgb(0xd0, 0x40, 0xd0)));
            let (ua, ub) = (dev.to_user(a), dev.to_user(b));
            let dist = (b[0] - a[0]).hypot(b[1] - a[1]);
            let ang = (b[1] - a[1]).atan2(b[0] - a[0]).to_degrees();
            self.label(painter, &format!("{dist:.2} mm  ·  {ang:.1}°  ·  dx {:.2}  dy {:.2}", ub[0] - ua[0], ub[1] - ua[1]), sb + Vec2::new(12.0, 16.0));
        }
        let mag = Color32::from_rgb(0xd0, 0x20, 0xd0);
        for &(vert, v) in &self.guides {
            if vert {
                let x = self.to_screen([v, 0.0]).x.round() + 0.5;
                painter.extend(egui::Shape::dashed_line(&[Pos2::new(x, self.rect.min.y + RULER), Pos2::new(x, self.rect.max.y)], Stroke::new(1.0_f32, mag), 6.0, 4.0));
            } else {
                let y = self.to_screen([0.0, v]).y.round() + 0.5;
                painter.extend(egui::Shape::dashed_line(&[Pos2::new(self.rect.min.x + RULER, y), Pos2::new(self.rect.max.x, y)], Stroke::new(1.0_f32, mag), 6.0, 4.0));
            }
        }
        if let Some(m) = self.snap_mark {
            let p = self.to_screen(m);
            painter.circle_stroke(p, 6.0, Stroke::new(1.5_f32, mag));
            painter.line_segment([p - Vec2::new(9.0, 0.0), p + Vec2::new(9.0, 0.0)], Stroke::new(1.5_f32, mag));
            painter.line_segment([p - Vec2::new(0.0, 9.0), p + Vec2::new(0.0, 9.0)], Stroke::new(1.5_f32, mag));
        }
        if let Some(h) = self.head {
            let p = self.to_screen(h);
            let g = Stroke::new(2.0_f32, Color32::from_rgb(0x18, 0xb0, 0x48));
            painter.circle_stroke(p, 7.0, g);
            painter.line_segment([p - Vec2::new(12.0, 0.0), p + Vec2::new(12.0, 0.0)], g);
            painter.line_segment([p - Vec2::new(0.0, 12.0), p + Vec2::new(0.0, 12.0)], g);
        }
        if let Some(b) = self.frame_box {
            let r = Rect::from_min_max(self.to_screen([b.x0, b.y1]), self.to_screen([b.x1, b.y0]));
            let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.5_f32, Color32::from_rgb(0x18, 0xb0, 0x48)), 6.0, 4.0));
        }
        let _ = doc;
    }

    fn paint_rulers(&self, painter: &egui::Painter, dev: &Device) {
        let r = self.rect;
        let bg = Color32::from_rgb(0x2b, 0x2d, 0x31);
        painter.rect_filled(Rect::from_min_size(r.min, Vec2::new(r.width(), RULER)), 0.0, bg);
        painter.rect_filled(Rect::from_min_size(r.min, Vec2::new(RULER, r.height())), 0.0, bg);
        let tick = Stroke::new(1.0_f32, Color32::from_gray(0x55));
        let txt = Color32::from_gray(0xaa);
        let font = egui::FontId::proportional(10.0);
        let mut major = self.grid_step() * 5.0;
        while major * (self.z as f64) < 60.0 {
            major *= 2.0;
        }
        let minor = major / 5.0;
        let wx0 = self.to_world(Pos2::new(r.min.x + RULER, 0.0))[0];
        let wx1 = self.to_world(Pos2::new(r.max.x, 0.0))[0];
        let mut x = (wx0 / minor).floor() * minor;
        while x <= wx1 {
            let sx = self.to_screen([x, 0.0]).x.round() + 0.5;
            if sx >= r.min.x + RULER {
                let maj = ((x / major) - (x / major).round()).abs() < 1e-6;
                painter.line_segment([Pos2::new(sx, r.min.y + RULER), Pos2::new(sx, r.min.y + if maj { 6.0 } else { 15.0 })], tick);
                if maj {
                    painter.text(Pos2::new(sx + 2.0, r.min.y + 1.0), egui::Align2::LEFT_TOP, format!("{}", x.round()), font.clone(), txt);
                }
            }
            x += minor;
        }
        let ua = dev.to_user([0.0, self.to_world(Pos2::new(0.0, r.min.y + RULER))[1]])[1];
        let ub = dev.to_user([0.0, self.to_world(Pos2::new(0.0, r.max.y))[1]])[1];
        let mut y = (ua.min(ub) / minor).floor() * minor;
        while y <= ua.max(ub) {
            let sy = self.to_screen(dev.from_user([0.0, y])).y.round() + 0.5;
            if sy >= r.min.y + RULER {
                let maj = ((y / major) - (y / major).round()).abs() < 1e-6;
                painter.line_segment([Pos2::new(r.min.x + RULER, sy), Pos2::new(r.min.x + if maj { 6.0 } else { 15.0 }, sy)], tick);
                if maj {
                    painter.add(egui::Shape::Text(
                        egui::epaint::TextShape::new(Pos2::new(r.min.x + 1.0, sy - 2.0), painter.layout_no_wrap(format!("{}", y.round()), font.clone(), txt), txt).with_angle(-std::f32::consts::FRAC_PI_2),
                    ));
                }
            }
            y += minor;
        }
        let o = Stroke::new(1.0_f32, Color32::from_rgb(0xff, 0x6a, 0x2a));
        let m = self.mouse_screen;
        if r.contains(m) {
            painter.line_segment([Pos2::new(m.x, r.min.y), Pos2::new(m.x, r.min.y + RULER)], o);
            painter.line_segment([Pos2::new(r.min.x, m.y), Pos2::new(r.min.x + RULER, m.y)], o);
        }
        painter.rect_filled(Rect::from_min_size(r.min, Vec2::splat(RULER)), 0.0, bg);
    }

    // ---------------------------------------------------------------- input
    pub(crate) fn hit_test(&mut self, doc: &Doc, p: P) -> Option<u64> {
        let tol = 5.0 / self.z as f64;
        for s in doc.shapes.iter().rev() {
            if !doc.layers[s.layer].show {
                continue;
            }
            let cs = self.contours(s);
            if s.hit(p, tol, &cs) {
                return Some(s.id);
            }
        }
        None
    }
    fn handle_at(&self, b: BBox, sp: Pos2) -> Option<&'static str> {
        self.handles(b).into_iter().find(|(_, p)| (p.x - sp.x).abs() <= 7.0 && (p.y - sp.y).abs() <= 7.0).map(|(k, _)| k)
    }
    fn snap_angle(&self, w: P, shift: bool) -> P {
        let Some(a) = self.line_pts.as_ref().and_then(|l| l.last().copied()) else { return w };
        if !shift {
            return w;
        }
        snap_from(a, w)
    }
    pub fn finish_line(&mut self, closed: bool, layer: usize, ev: &mut Vec<CanvasEvent>) {
        if let Some(pts) = self.line_pts.take() {
            if pts.len() > 1 {
                let mut s = Shape::new("path");
                s.layer = layer;
                s.subpaths = vec![Contour { pts, closed }];
                ev.push(CanvasEvent::AddShape(s));
            }
        }
    }
    pub fn cursor(&self, doc: &Doc) -> egui::CursorIcon {
        use egui::CursorIcon as C;
        if matches!(self.drag, Some(Drag::Pan { .. })) {
            return C::Grabbing;
        }
        if self.space || self.tool == Tool::Pan {
            return C::Grab;
        }
        match self.tool {
            Tool::Select => {
                if let Some(b) = doc.sel_box() {
                    if let Some(h) = self.handle_at(b, self.mouse_screen) {
                        return match h {
                            "n" | "s" => C::ResizeVertical,
                            "e" | "w" => C::ResizeHorizontal,
                            "nw" | "se" => C::ResizeNwSe,
                            "ne" | "sw" => C::ResizeNeSw,
                            _ => C::Alias,
                        };
                    }
                }
                C::Default
            }
            Tool::Text => C::Text,
            _ => C::Crosshair,
        }
    }

    /// Feed one frame of pointer input. Returns what the app should do.
    #[allow(clippy::too_many_arguments)]
    pub fn input(&mut self, ui: &egui::Ui, resp: &egui::Response, doc: &mut Doc, dev: &Device, cfg: &SnapCfg, active_layer: usize) -> Vec<CanvasEvent> {
        let mut ev = vec![];
        let (pointer, mods, scroll) = ui.input(|i| (i.pointer.clone(), i.modifiers, i.smooth_scroll_delta));
        let hover = resp.hover_pos();
        if let Some(h) = hover.or(pointer.interact_pos()) {
            self.mouse_screen = h;
        }
        if resp.hovered() && scroll.y != 0.0 {
            if let Some(h) = hover {
                self.zoom_at((scroll.y * 0.0025).exp(), h);
            }
        }
        let sp = self.mouse_screen;
        let wp = self.to_world(sp);
        self.mouse = wp;
        let alt = mods.alt;
        if matches!(self.tool, Tool::Spline | Tool::Nodes) && self.preview.is_none() && !self.space && !pointer.middle_down() && !matches!(self.drag, Some(Drag::Pan { .. })) {
            return self.node_input(ui, resp, doc, dev, cfg, active_layer);
        }

        // ---- press
        let pressed_here = resp.hovered() && (pointer.primary_pressed() || pointer.middle_down() && pointer.any_pressed());
        if pressed_here && self.drag.is_none() {
            if pointer.middle_down() || self.space || self.tool == Tool::Pan {
                self.drag = Some(Drag::Pan { start: sp, ox: self.ox, oy: self.oy });
            } else if self.preview.is_none() {
                match self.tool {
                    Tool::Select => {
                        let sb = doc.sel_box();
                        let handle = sb.and_then(|b| self.handle_at(b, sp));
                        if let (Some(b), Some(h)) = (sb, handle) {
                            let orig = doc.selected().iter().map(|s| (s.id, s.tf)).collect();
                            let shapes: Vec<Shape> = doc.selected().into_iter().cloned().collect();
                            doc.checkpoint();
                            self.drag = Some(if h == "rot" { Drag::Rotate { start: wp, bx: b, orig } } else { Drag::Scale { handle: h, bx: b, orig: shapes } });
                        } else if let Some(id) = self.hit_test(doc, wp) {
                            if mods.shift || mods.command {
                                if !doc.sel.remove(&id) {
                                    doc.sel.insert(id);
                                }
                            } else if !doc.sel.contains(&id) {
                                doc.sel.clear();
                                doc.sel.insert(id);
                            }
                            ev.push(CanvasEvent::SelectionChanged);
                            if doc.sel.contains(&id) {
                                self.drag = Some(self.move_drag(doc, wp, dev));
                            }
                        } else if sb.map(|b| wp[0] >= b.x0 && wp[0] <= b.x1 && wp[1] >= b.y0 && wp[1] <= b.y1).unwrap_or(false) && !mods.shift {
                            self.drag = Some(self.move_drag(doc, wp, dev));
                        } else {
                            if !mods.shift {
                                doc.sel.clear();
                                ev.push(CanvasEvent::SelectionChanged);
                            }
                            self.drag = Some(Drag::Box { start: sp, cur: sp });
                        }
                    }
                    Tool::Rect | Tool::Ellipse | Tool::Polygon => {
                        let st = self.snap_point(wp, alt, doc, None, cfg, dev);
                        self.drag = Some(Drag::Create { start: st, ghost: None });
                    }
                    Tool::Line => {
                        let w = self.snap_point(wp, alt, doc, None, cfg, dev);
                        let w = self.snap_angle(w, mods.shift);
                        match &mut self.line_pts {
                            None => self.line_pts = Some(vec![w]),
                            Some(lp) => {
                                let f = lp[0];
                                if lp.len() > 2 && ((f[0] - w[0]).hypot(f[1] - w[1]) * self.z as f64) < 8.0 {
                                    lp.push(f);
                                    self.finish_line(true, active_layer, &mut ev);
                                } else {
                                    lp.push(w);
                                }
                            }
                        }
                    }
                    Tool::Text => {
                        let w = self.snap_point(wp, alt, doc, None, cfg, dev);
                        self.snap_mark = None;
                        ev.push(CanvasEvent::TextAt(w));
                    }
                    Tool::Measure => {
                        let w = self.snap_point(wp, alt, doc, None, cfg, dev);
                        self.measure = Some([w, w]);
                        self.drag = Some(Drag::Measure);
                    }
                    Tool::Pan | Tool::Spline | Tool::Nodes => {}
                }
            }
        }
        if resp.hovered() && pointer.secondary_pressed() && self.line_pts.is_some() {
            self.finish_line(false, active_layer, &mut ev);
        }
        if resp.double_clicked() {
            if self.tool == Tool::Line && self.line_pts.is_some() {
                if let Some(lp) = &mut self.line_pts {
                    lp.pop();
                }
                self.finish_line(false, active_layer, &mut ev);
            } else if self.tool == Tool::Select {
                if let Some(id) = self.hit_test(doc, wp) {
                    if doc.shapes.iter().any(|s| s.id == id && s.kind == "text") {
                        ev.push(CanvasEvent::EditText(id));
                    }
                }
            }
        }

        // ---- move
        let down = pointer.primary_down() || pointer.middle_down();
        if let Some(drag) = self.drag.clone() {
            if down {
                match drag {
                    Drag::Pan { start, ox, oy } => {
                        self.ox = ox + sp.x - start.x;
                        self.oy = oy + sp.y - start.y;
                    }
                    Drag::Box { start, .. } => self.drag = Some(Drag::Box { start, cur: sp }),
                    Drag::Measure => {
                        let q = self.snap_point(wp, alt, doc, None, cfg, dev);
                        if let Some(m) = &mut self.measure {
                            m[1] = if mods.shift { snap_from(m[0], q) } else { q };
                        }
                    }
                    Drag::Move { start, orig, moved, bx, centers } => {
                        let (mut dx, mut dy) = (wp[0] - start[0], wp[1] - start[1]);
                        if !moved && (dx.hypot(dy) * self.z as f64) < 3.0 {
                            // not dragging yet
                        } else {
                            if !moved {
                                doc.checkpoint();
                            }
                            if mods.shift {
                                if dx.abs() > dy.abs() {
                                    dy = 0.0;
                                } else {
                                    dx = 0.0;
                                }
                            }
                            self.guides.clear();
                            if !alt {
                                let (cx, cy) = (bx.cx() + dx, bx.cy() + dy);
                                let (mut done_x, mut done_y) = (false, false);
                                if cfg.obj {
                                    let tol = 9.0 / self.z as f64;
                                    let (mut best_x, mut best_y, mut ex, mut ey) = (None, None, tol, tol);
                                    for c in &centers {
                                        if (c[0] - cx).abs() < ex {
                                            ex = (c[0] - cx).abs();
                                            best_x = Some(c[0]);
                                        }
                                        if (c[1] - cy).abs() < ey {
                                            ey = (c[1] - cy).abs();
                                            best_y = Some(c[1]);
                                        }
                                    }
                                    if let Some(x) = best_x {
                                        if !(mods.shift && dx == 0.0) {
                                            dx += x - cx;
                                            self.guides.push((true, x));
                                            done_x = true;
                                        }
                                    }
                                    if let Some(y) = best_y {
                                        if !(mods.shift && dy == 0.0) {
                                            dy += y - cy;
                                            self.guides.push((false, y));
                                            done_y = true;
                                        }
                                    }
                                }
                                if cfg.grid {
                                    // the selection's upper-left corner (as the rulers count) lands on the grid
                                    let ref_y = if dev.origin_top { bx.y1 } else { bx.y0 };
                                    let q = self.snap_grid_pt([bx.x0 + dx, ref_y + dy], cfg, dev);
                                    if !done_x && !(mods.shift && dx == 0.0) {
                                        dx = q[0] - bx.x0;
                                    }
                                    if !done_y && !(mods.shift && dy == 0.0) {
                                        dy = q[1] - ref_y;
                                    }
                                }
                            }
                            let m = translate(dx, dy);
                            set_tfs(doc, &orig, &m);
                            self.drag = Some(Drag::Move { start, orig, moved: true, bx, centers });
                            ev.push(CanvasEvent::Edited);
                        }
                    }
                    Drag::Scale { handle: h, bx: b, orig } => {
                        let w = self.snap_point(wp, alt, doc, Some(&doc.sel.clone()), cfg, dev);
                        let ax = if h.contains('w') { b.x1 } else if h.contains('e') { b.x0 } else { b.cx() };
                        let ay = if h.contains('n') { b.y0 } else if h.contains('s') { b.y1 } else { b.cy() };
                        let mut sx = if h.contains('e') || h.contains('w') { (w[0] - ax) / nz((if h.contains('e') { b.x1 } else { b.x0 }) - ax) } else { 1.0 };
                        let mut sy = if h.contains('n') || h.contains('s') { (w[1] - ay) / nz((if h.contains('n') { b.y1 } else { b.y0 }) - ay) } else { 1.0 };
                        let corner = h.len() == 2;
                        let keep = if corner { !mods.shift && cfg.lock_ratio } else { mods.shift };
                        if keep {
                            let s = if corner {
                                if sx.abs() > sy.abs() {
                                    sx
                                } else {
                                    sy
                                }
                            } else if h == "n" || h == "s" {
                                sy
                            } else {
                                sx
                            };
                            sx = s;
                            sy = s;
                        }
                        if sx.abs() < 1e-3 {
                            sx = 1e-3;
                        }
                        if sy.abs() < 1e-3 {
                            sy = 1e-3;
                        }
                        // start again from the shapes as they were, so sizes don't compound frame to frame
                        let m = scale_about(sx, sy, ax, ay);
                        for o in &orig {
                            if let Some(s) = doc.shapes.iter_mut().find(|s| s.id == o.id) {
                                *s = o.clone();
                                s.tf = mul(&m, &o.tf);
                                s.bake_scale();
                            }
                        }
                        ev.push(CanvasEvent::Edited);
                    }
                    Drag::Rotate { start, bx: b, orig } => {
                        let (cx, cy) = (b.cx(), b.cy());
                        let mut ang = ((wp[1] - cy).atan2(wp[0] - cx) - (start[1] - cy).atan2(start[0] - cx)).to_degrees();
                        if mods.shift {
                            ang = (ang / 15.0).round() * 15.0;
                        }
                        set_tfs(doc, &orig, &rotate_about(ang, cx, cy));
                        ev.push(CanvasEvent::Edited);
                    }
                    Drag::Create { start, .. } => {
                        let e = self.snap_point(wp, alt, doc, None, cfg, dev);
                        let (x0, y0) = (start[0], start[1]);
                        let (mut x1, mut y1) = (e[0], e[1]);
                        if mods.shift || mods.ctrl {
                            let m = (x1 - x0).abs().max((y1 - y0).abs());
                            x1 = x0 + sgn(x1 - x0) * m;
                            y1 = y0 + sgn(y1 - y0) * m;
                        }
                        let kind = match self.tool {
                            Tool::Rect => "rect",
                            Tool::Ellipse => "ellipse",
                            _ => "polygon",
                        };
                        let mut g = Shape::new(kind);
                        g.w = (x1 - x0).abs();
                        g.h = (y1 - y0).abs();
                        g.sides = cfg.poly_sides;
                        g.layer = active_layer;
                        g.tf = translate(x0.min(x1), y0.min(y1));
                        self.drag = Some(Drag::Create { start, ghost: Some(g) });
                    }
                }
            } else {
                // ---- release
                self.drag = None;
                self.guides.clear();
                self.snap_mark = None;
                match drag {
                    Drag::Box { start, cur } => {
                        let (a, b) = (self.to_world(start), self.to_world(cur));
                        let (x0, x1, y0, y1) = (a[0].min(b[0]), a[0].max(b[0]), a[1].min(b[1]), a[1].max(b[1]));
                        let enclose = cur.x >= start.x;
                        if (cur.x - start.x).abs() > 3.0 || (cur.y - start.y).abs() > 3.0 {
                            let ids: Vec<(u64, usize)> = doc.shapes.iter().map(|s| (s.id, s.layer)).collect();
                            for (id, layer) in ids {
                                if !doc.layers[layer].show {
                                    continue;
                                }
                                let s = doc.shapes.iter().find(|s| s.id == id).unwrap().clone();
                                let Some(sb) = self.bbox(&s) else { continue };
                                let inside = sb.x0 >= x0 && sb.x1 <= x1 && sb.y0 >= y0 && sb.y1 <= y1;
                                let touch = sb.x1 >= x0 && sb.x0 <= x1 && sb.y1 >= y0 && sb.y0 <= y1;
                                if if enclose { inside } else { touch } {
                                    doc.sel.insert(id);
                                }
                            }
                        }
                        ev.push(CanvasEvent::SelectionChanged);
                    }
                    Drag::Move { moved: true, .. } | Drag::Scale { .. } | Drag::Rotate { .. } => ev.push(CanvasEvent::Edited),
                    Drag::Create { ghost: Some(g), .. } => {
                        if g.w * (self.z as f64) > 2.0 && g.h * (self.z as f64) > 2.0 {
                            ev.push(CanvasEvent::AddShape(g));
                        }
                    }
                    _ => {}
                }
            }
        } else if resp.hovered() && matches!(self.tool, Tool::Rect | Tool::Ellipse | Tool::Polygon | Tool::Line | Tool::Text | Tool::Measure) && self.preview.is_none() {
            let w = self.snap_point(wp, alt, doc, None, cfg, dev);
            if self.line_pts.is_some() {
                self.mouse = self.snap_angle(w, mods.shift);
            }
        } else if self.drag.is_none() {
            self.snap_mark = None;
        }
        ev
    }

    fn move_drag(&mut self, doc: &Doc, wp: P, dev: &Device) -> Drag {
        let sel = doc.sel.clone();
        let centers = self.snap_centers(doc, Some(&sel), dev);
        Drag::Move { start: wp, orig: doc.selected().iter().map(|s| (s.id, s.tf)).collect(), moved: false, bx: doc.sel_box().unwrap_or(BBox { x0: wp[0], y0: wp[1], x1: wp[0], y1: wp[1] }), centers }
    }
}

fn set_tfs(doc: &mut Doc, orig: &[(u64, M)], m: &M) {
    for (id, tf) in orig {
        if let Some(s) = doc.shapes.iter_mut().find(|s| s.id == *id) {
            s.tf = mul(m, tf);
        }
    }
}
fn nz(v: f64) -> f64 {
    if v.abs() < 1e-9 {
        1e-9
    } else {
        v
    }
}
fn sgn(v: f64) -> f64 {
    if v < 0.0 {
        -1.0
    } else {
        1.0
    }
}
fn snap_from(a: P, w: P) -> P {
    let step = std::f64::consts::PI / 12.0;
    let ang = ((w[1] - a[1]).atan2(w[0] - a[0]) / step).round() * step;
    let d = (w[0] - a[0]).hypot(w[1] - a[1]);
    [a[0] + d * ang.cos(), a[1] + d * ang.sin()]
}

/// Small drawn icons for the tool buttons
pub fn tool_icon(painter: &egui::Painter, r: Rect, t: Tool, col: Color32) {
    let s = Stroke::new(1.6_f32, col);
    let c = r.center();
    let p = |x: f32, y: f32| Pos2::new(c.x + x, c.y + y);
    match t {
        Tool::Select => {
            painter.add(egui::Shape::convex_polygon(vec![p(-6.0, -9.0), p(7.0, 1.0), p(1.0, 2.0), p(-2.0, 8.0)], Color32::TRANSPARENT, s));
        }
        Tool::Line => {
            painter.add(egui::Shape::line(vec![p(-8.0, 6.0), p(-3.0, -5.0), p(3.0, 4.0), p(8.0, -6.0)], s));
        }
        Tool::Rect => {
            painter.rect_stroke(Rect::from_center_size(c, Vec2::new(16.0, 12.0)), 0.0, s);
        }
        Tool::Ellipse => {
            let pts: Vec<Pos2> = (0..=24).map(|i| {
                let a = i as f32 / 24.0 * std::f32::consts::TAU;
                p(8.0 * a.cos(), 6.0 * a.sin())
            }).collect();
            painter.add(egui::Shape::line(pts, s));
        }
        Tool::Polygon => {
            let pts: Vec<Pos2> = (0..=6).map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + i as f32 / 6.0 * std::f32::consts::TAU;
                p(8.0 * a.cos(), 8.0 * a.sin())
            }).collect();
            painter.add(egui::Shape::line(pts, s));
        }
        Tool::Text => {
            painter.line_segment([p(-7.0, -7.0), p(7.0, -7.0)], s);
            painter.line_segment([p(0.0, -7.0), p(0.0, 8.0)], s);
            painter.line_segment([p(-3.0, 8.0), p(3.0, 8.0)], s);
        }
        Tool::Pan => {
            painter.line_segment([p(0.0, -9.0), p(0.0, 9.0)], s);
            painter.line_segment([p(-9.0, 0.0), p(9.0, 0.0)], s);
            for (a, b, c2) in [((0.0, -9.0), (-3.0, -6.0), (3.0, -6.0)), ((0.0, 9.0), (-3.0, 6.0), (3.0, 6.0)), ((-9.0, 0.0), (-6.0, -3.0), (-6.0, 3.0)), ((9.0, 0.0), (6.0, -3.0), (6.0, 3.0))] {
                painter.line_segment([p(a.0, a.1), p(b.0, b.1)], s);
                painter.line_segment([p(a.0, a.1), p(c2.0, c2.1)], s);
            }
        }
        Tool::Measure => {
            painter.add(egui::Shape::closed_line(vec![p(-9.0, 5.0), p(5.0, -9.0), p(9.0, -5.0), p(-5.0, 9.0)], s));
            for k in [-3.0f32, 0.0, 3.0] {
                painter.line_segment([p(k - 1.0, k * -1.0 - 1.0 + 0.0), p(k + 1.0, -k + 1.0)], s);
            }
        }
        Tool::Spline => {
            let pts: Vec<Pos2> = (0..=20).map(|k| {
                let t = k as f32 / 20.0;
                let (a, b, c, d) = ((-9.0f32, 7.0f32), (-6.0f32, -12.0f32), (6.0f32, 12.0f32), (9.0f32, -7.0f32));
                let u = 1.0 - t;
                p(u * u * u * a.0 + 3.0 * u * u * t * b.0 + 3.0 * u * t * t * c.0 + t * t * t * d.0, u * u * u * a.1 + 3.0 * u * u * t * b.1 + 3.0 * u * t * t * c.1 + t * t * t * d.1)
            }).collect();
            painter.add(egui::Shape::line(pts, s));
            painter.rect_filled(Rect::from_center_size(p(-9.0, 7.0), Vec2::splat(4.0)), 0.0, col);
            painter.rect_filled(Rect::from_center_size(p(9.0, -7.0), Vec2::splat(4.0)), 0.0, col);
        }
        Tool::Nodes => {
            painter.line_segment([p(-8.0, 6.0), p(8.0, -6.0)], s);
            painter.circle_filled(p(0.0, 0.0), 2.5, col);
            painter.rect_stroke(Rect::from_center_size(p(-8.0, 6.0), Vec2::splat(5.0)), 0.0, s);
            painter.circle_stroke(p(8.0, -6.0), 2.5, s);
            painter.add(egui::Shape::convex_polygon(vec![p(1.0, 1.0), p(9.0, 4.0), p(5.0, 5.0), p(4.0, 9.0)], col, Stroke::NONE));
        }
    }
}
