//! Canvas: drawing splines and editing curve points and handles.

use crate::canvas::{CanvasEvent, SnapCfg, Tool, View};
use crate::gcode::Device;
use crate::geom::*;
use crate::model::{Doc, Shape};
use crate::spline::{bez, Curve, Node};
use egui::{Color32, Pos2, Stroke};

/// A point (or one of its handles) on a curve shape
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeRef {
    pub shape: u64,
    pub curve: usize,
    pub node: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Point,
    In,
    Out,
}

#[derive(Clone)]
pub enum NodeDrag {
    /// drawing a spline: dragging out the handles of the point just placed
    Handles { start: P },
    Edit { part: Part, at: NodeRef, moved: bool },
}

const ORANGE: Color32 = Color32::from_rgb(0xff, 0x8a, 0x3a);

impl View {
    pub fn node_shape<'a>(&self, doc: &'a Doc) -> Option<&'a Shape> {
        let id = self.node_sel.map(|r| r.shape).or_else(|| if doc.sel.len() == 1 { doc.sel.iter().next().copied() } else { None })?;
        doc.shapes.iter().find(|s| s.id == id && s.kind == "spline")
    }

    /// One frame of input for the Spline and Edit-points tools
    #[allow(clippy::too_many_arguments)]
    pub fn node_input(&mut self, ui: &egui::Ui, resp: &egui::Response, doc: &mut Doc, dev: &Device, cfg: &SnapCfg, active_layer: usize) -> Vec<CanvasEvent> {
        let mut ev = vec![];
        let (pointer, mods) = ui.input(|i| (i.pointer.clone(), i.modifiers));
        let sp = self.mouse_screen;
        let wp = self.to_world(sp);
        let alt = mods.alt;
        let pressed = resp.hovered() && pointer.primary_pressed();
        let tol = 8.0 / self.z as f64;

        if self.tool == Tool::Spline {
            let w = self.snap_point(wp, alt, doc, None, cfg, dev);
            self.mouse = w;
            if resp.double_clicked() || (resp.hovered() && pointer.secondary_pressed()) {
                // a double-click's second press added a point on top of the last one: drop it
                if let Some(n) = &mut self.spline_new {
                    if n.len() > 1 && (n[n.len() - 1].p[0] - n[n.len() - 2].p[0]).hypot(n[n.len() - 1].p[1] - n[n.len() - 2].p[1]) < tol {
                        n.pop();
                    }
                }
                self.finish_spline(false, active_layer, &mut ev);
                return ev;
            }
            if pressed {
                let close = self.spline_new.as_ref().map(|n| n.len() >= 2 && (n[0].p[0] - w[0]).hypot(n[0].p[1] - w[1]) < tol).unwrap_or(false);
                if close {
                    self.finish_spline(true, active_layer, &mut ev);
                    return ev;
                }
                self.spline_new.get_or_insert_with(Vec::new).push(Node::sharp(w));
                self.node_drag = Some(NodeDrag::Handles { start: w });
            }
            if let Some(NodeDrag::Handles { start }) = self.node_drag.clone() {
                if pointer.primary_down() {
                    if (w[0] - start[0]).hypot(w[1] - start[1]) * self.z as f64 > 3.0 {
                        if let Some(n) = self.spline_new.as_mut().and_then(|v| v.last_mut()) {
                            // dragging pulls out symmetric handles
                            n.o = w;
                            n.i = [2.0 * n.p[0] - w[0], 2.0 * n.p[1] - w[1]];
                            n.kind = if mods.shift { "corner".into() } else { "symmetric".into() };
                            if mods.shift {
                                n.i = n.p;
                            }
                        }
                    }
                } else {
                    self.node_drag = None;
                }
            }
            return ev;
        }

        // ---- edit points
        // the curve being edited: the selected spline
        if pressed {
            let target = self.node_shape(doc).map(|s| s.id);
            // 1) a handle of the selected point, 2) a point, 3) another shape
            let mut hit: Option<(Part, NodeRef)> = None;
            if let (Some(id), Some(r)) = (target, self.node_sel) {
                if r.shape == id {
                    if let Some(s) = doc.shapes.iter().find(|s| s.id == id) {
                        if let Some(n) = s.curves.get(r.curve).and_then(|c| c.nodes.get(r.node)) {
                            for (part, q) in [(Part::Out, n.o), (Part::In, n.i)] {
                                if (q[0] - n.p[0]).hypot(q[1] - n.p[1]) > 1e-6 && (q[0] - wp[0]).hypot(q[1] - wp[1]) < tol {
                                    hit = Some((part, r));
                                }
                            }
                        }
                    }
                }
            }
            if hit.is_none() {
                if let Some(s) = target.and_then(|id| doc.shapes.iter().find(|s| s.id == id)) {
                    let mut best = tol;
                    for (ci, c) in s.curves.iter().enumerate() {
                        for (ni, n) in c.nodes.iter().enumerate() {
                            let dd = (n.p[0] - wp[0]).hypot(n.p[1] - wp[1]);
                            if dd < best {
                                best = dd;
                                hit = Some((Part::Point, NodeRef { shape: s.id, curve: ci, node: ni }));
                            }
                        }
                    }
                }
            }
            match hit {
                Some((part, r)) => {
                    doc.checkpoint();
                    self.node_sel = Some(r);
                    self.node_drag = Some(NodeDrag::Edit { part, at: r, moved: false });
                    ev.push(CanvasEvent::SelectionChanged);
                }
                None => {
                    self.node_sel = None;
                    if let Some(id) = self.hit_test(doc, wp) {
                        doc.sel.clear();
                        doc.sel.insert(id);
                        ev.push(CanvasEvent::SelectionChanged);
                        if let Some(s) = doc.shapes.iter().find(|s| s.id == id) {
                            if s.kind != "spline" {
                                ev.push(CanvasEvent::Message(if s.kind == "image" { "Pictures have no points to edit.".into() } else { "Press \"Convert to curve\" to edit this shape's points.".into() }));
                            }
                        }
                    } else if target.is_none() {
                        doc.sel.clear();
                    }
                }
            }
        }
        if let Some(NodeDrag::Edit { part, at, moved }) = self.node_drag.clone() {
            if pointer.primary_down() {
                let start_ok = moved || pointer.is_decidedly_dragging();
                if start_ok {
                    let w = if part == Part::Point { self.snap_point(wp, alt, doc, Some(&std::iter::once(at.shape).collect()), cfg, dev) } else { wp };
                    if let Some(s) = doc.shapes.iter_mut().find(|s| s.id == at.shape) {
                        s.bake_scale();
                        if let Some(c) = s.curves.get_mut(at.curve) {
                            if at.node < c.nodes.len() {
                                match part {
                                    Part::Point => c.move_node(at.node, w),
                                    Part::Out => c.move_handle(at.node, true, w),
                                    Part::In => c.move_handle(at.node, false, w),
                                }
                            }
                        }
                    }
                    self.node_drag = Some(NodeDrag::Edit { part, at, moved: true });
                    ev.push(CanvasEvent::Edited);
                }
            } else {
                self.node_drag = None;
                self.snap_mark = None;
            }
        }
        // double-click: on a segment adds a point; on a point switches sharp <-> smooth
        if resp.double_clicked() {
            if let Some(id) = self.node_shape(doc).map(|s| s.id) {
                let on_point = self.node_sel.filter(|r| r.shape == id).and_then(|r| {
                    let s = doc.shapes.iter().find(|s| s.id == id)?;
                    let n = s.curves.get(r.curve)?.nodes.get(r.node)?;
                    ((n.p[0] - wp[0]).hypot(n.p[1] - wp[1]) < tol).then_some(r)
                });
                if let Some(s) = doc.shapes.iter_mut().find(|s| s.id == id) {
                    s.bake_scale();
                    if let Some(r) = on_point {
                        let c = &mut s.curves[r.curve];
                        let k = if c.nodes[r.node].kind == "sharp" { "smooth" } else { "sharp" };
                        c.set_kind(r.node, k);
                    } else {
                        let best = s.curves.iter().enumerate().filter_map(|(ci, c)| c.nearest(wp).map(|(si, t, dd)| (ci, si, t, dd))).min_by(|a, b| a.3.partial_cmp(&b.3).unwrap());
                        if let Some((ci, si, t, dd)) = best {
                            if dd < tol {
                                let at = s.curves[ci].insert(si, t);
                                self.node_sel = Some(NodeRef { shape: id, curve: ci, node: at });
                            }
                        }
                    }
                }
                ev.push(CanvasEvent::Edited);
            }
        }
        ev
    }

    pub fn finish_spline(&mut self, closed: bool, layer: usize, ev: &mut Vec<CanvasEvent>) {
        self.node_drag = None;
        let Some(nodes) = self.spline_new.take() else { return };
        if nodes.len() < 2 {
            return;
        }
        let mut s = Shape::new("spline");
        s.layer = layer;
        s.curves = vec![Curve { nodes, closed: closed && true }];
        ev.push(CanvasEvent::AddShape(s));
    }

    fn handle_line(&self, painter: &egui::Painter, p: P, h: P) {
        if (p[0] - h[0]).hypot(p[1] - h[1]) < 1e-6 {
            return;
        }
        let (a, b) = (self.to_screen(p), self.to_screen(h));
        painter.line_segment([a, b], Stroke::new(1.0_f32, Color32::from_rgb(0x2f, 0x9f, 0xe0)));
        painter.circle(b, 4.0, Color32::WHITE, Stroke::new(1.2_f32, Color32::from_rgb(0x2f, 0x9f, 0xe0)));
    }

    /// Points and handles of the curve being edited, and the spline being drawn
    pub fn paint_nodes(&self, painter: &egui::Painter, doc: &Doc) {
        if self.tool == Tool::Nodes {
            if let Some(s) = self.node_shape(doc) {
                let sel = self.node_sel.filter(|r| r.shape == s.id);
                for (ci, c) in s.curves.iter().enumerate() {
                    let n = c.nodes.len();
                    for (ni, node) in c.nodes.iter().enumerate() {
                        let is_sel = sel.map(|r| r.curve == ci && r.node == ni).unwrap_or(false);
                        let near_sel = sel.map(|r| r.curve == ci && (r.node + 1) % n.max(1) == ni || (ni + 1) % n.max(1) == r.node && r.curve == ci).unwrap_or(false);
                        if is_sel {
                            self.handle_line(painter, node.p, node.i);
                            self.handle_line(painter, node.p, node.o);
                        } else if near_sel {
                            // the neighbors' handles that shape the selected point's segments
                            let r = sel.unwrap();
                            if (r.node + 1) % n == ni {
                                self.handle_line(painter, node.p, node.i);
                            } else {
                                self.handle_line(painter, node.p, node.o);
                            }
                        }
                        let q = self.to_screen(node.p);
                        let fill = if is_sel { ORANGE } else { Color32::WHITE };
                        let st = Stroke::new(1.3_f32, if is_sel { Color32::from_rgb(0x90, 0x40, 0x10) } else { Color32::from_rgb(0x2f, 0x6f, 0xe0) });
                        if node.kind == "smooth" || node.kind == "symmetric" {
                            painter.circle(q, 4.5, fill, st);
                        } else {
                            let r = egui::Rect::from_center_size(q, egui::vec2(8.0, 8.0));
                            painter.rect_filled(r, 0.0, fill);
                            painter.rect_stroke(r, 0.0, st);
                        }
                        if ni == 0 && !c.closed {
                            painter.circle_stroke(q, 7.5, Stroke::new(1.0_f32, Color32::from_rgb(0x2f, 0x6f, 0xe0)));
                        }
                    }
                }
            }
        }
        if let Some(nodes) = &self.spline_new {
            let stroke = Stroke::new(1.4_f32, ORANGE);
            let mut pts: Vec<Pos2> = vec![];
            for k in 0..nodes.len() {
                let a = &nodes[k];
                pts.push(self.to_screen(a.p));
                if let Some(b) = nodes.get(k + 1) {
                    for j in 1..=24 {
                        pts.push(self.to_screen(bez(a.p, a.o, b.i, b.p, j as f64 / 24.0)));
                    }
                }
            }
            // rubber band to the mouse
            if let (Some(l), true) = (nodes.last(), self.node_drag.is_none()) {
                for j in 1..=24 {
                    pts.push(self.to_screen(bez(l.p, l.o, self.mouse, self.mouse, j as f64 / 24.0)));
                }
            }
            painter.add(egui::Shape::line(pts, stroke));
            for (k, n) in nodes.iter().enumerate() {
                if k + 1 == nodes.len() {
                    self.handle_line(painter, n.p, n.i);
                    self.handle_line(painter, n.p, n.o);
                }
                let q = self.to_screen(n.p);
                painter.rect_filled(egui::Rect::from_center_size(q, egui::vec2(7.0, 7.0)), 0.0, if k == 0 { ORANGE } else { Color32::WHITE });
            }
        }
    }
}
