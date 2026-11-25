//! Editing curve points from the toolbar: typed positions for points and handles, point types.

use crate::app::App;
use crate::canvas::Tool;
use crate::canvas_nodes::NodeRef;
use crate::geom::P;
use crate::spline::KINDS;
use egui::RichText;

impl App {
    fn node_at(&self, r: NodeRef) -> Option<&crate::spline::Node> {
        self.doc.shapes.iter().find(|s| s.id == r.shape)?.curves.get(r.curve)?.nodes.get(r.node)
    }

    /// Toolbar row for the selected curve point: X/Y of the point and both handles, and its type
    pub fn node_numeric(&mut self, ui: &mut egui::Ui) -> bool {
        if self.view.tool != Tool::Nodes {
            return false;
        }
        let Some(r) = self.view.node_sel else {
            ui.label(RichText::new("Click a point to edit it · drag points and handles · double-click a segment to add a point · double-click a point for sharp/smooth").weak());
            return true;
        };
        let Some(n) = self.node_at(r).cloned() else {
            self.view.node_sel = None;
            return false;
        };
        // shown in the person's coordinates (counted from the origin corner)
        let u = |p: P| self.dev.to_user(p);
        let mut vals = [u(n.p)[0], u(n.p)[1], u(n.i)[0], u(n.i)[1], u(n.o)[0], u(n.o)[1]];
        let before = vals;
        let curved = n.kind != "sharp";
        let mut started = false;
        ui.label(RichText::new("Point").strong());
        for (k, l) in ["X", "Y"].iter().enumerate() {
            ui.label(*l);
            let resp = ui.add(egui::DragValue::new(&mut vals[k]).speed(0.1).fixed_decimals(3));
            started |= resp.drag_started() || resp.gained_focus();
        }
        ui.add_enabled_ui(curved, |ui| {
            ui.separator();
            ui.label(RichText::new("In handle").strong()).on_hover_text("Shapes the curve arriving at this point");
            for (k, l) in ["X", "Y"].iter().enumerate() {
                ui.label(*l);
                let resp = ui.add(egui::DragValue::new(&mut vals[2 + k]).speed(0.1).fixed_decimals(3));
                started |= resp.drag_started() || resp.gained_focus();
            }
            ui.separator();
            ui.label(RichText::new("Out handle").strong()).on_hover_text("Shapes the curve leaving this point");
            for (k, l) in ["X", "Y"].iter().enumerate() {
                ui.label(*l);
                let resp = ui.add(egui::DragValue::new(&mut vals[4 + k]).speed(0.1).fixed_decimals(3));
                started |= resp.drag_started() || resp.gained_focus();
            }
        });
        if started {
            self.doc.checkpoint();
        }
        if vals != before {
            let w = |x: f64, y: f64| self.dev.from_user([x, y]);
            let (pp, pi, po) = (w(vals[0], vals[1]), w(vals[2], vals[3]), w(vals[4], vals[5]));
            if let Some(s) = self.doc.shapes.iter_mut().find(|s| s.id == r.shape) {
                s.bake_scale();
                let c = &mut s.curves[r.curve];
                if vals[0] != before[0] || vals[1] != before[1] {
                    c.move_node(r.node, pp);
                } else if vals[2] != before[2] || vals[3] != before[3] {
                    c.move_handle(r.node, false, pi);
                } else {
                    c.move_handle(r.node, true, po);
                }
            }
            self.edited();
        }
        ui.separator();
        for (k, label, tip) in KINDS {
            if ui.selectable_label(n.kind == k, label).on_hover_text(tip).clicked() && n.kind != k {
                self.doc.checkpoint();
                if let Some(s) = self.doc.shapes.iter_mut().find(|s| s.id == r.shape) {
                    s.curves[r.curve].set_kind(r.node, k);
                }
                self.edited();
            }
        }
        ui.separator();
        if ui.button("🗑 Point").on_hover_text("Delete this point (Delete key)").clicked() {
            self.delete_node();
        }
        let closed = self.doc.shapes.iter().find(|s| s.id == r.shape).map(|s| s.curves[r.curve].closed).unwrap_or(false);
        if ui.button(if closed { "Open curve" } else { "Close curve" }).on_hover_text(if closed { "Remove the segment from the last point back to the first" } else { "Join the last point back to the first" }).clicked() {
            self.doc.checkpoint();
            if let Some(s) = self.doc.shapes.iter_mut().find(|s| s.id == r.shape) {
                let c = &mut s.curves[r.curve];
                c.closed = !c.closed && c.nodes.len() > 2;
            }
            self.edited();
        }
        true
    }

    pub fn delete_node(&mut self) {
        let Some(r) = self.view.node_sel else { return };
        self.doc.checkpoint();
        let mut remove_shape = false;
        if let Some(s) = self.doc.shapes.iter_mut().find(|s| s.id == r.shape) {
            if let Some(c) = s.curves.get_mut(r.curve) {
                if r.node < c.nodes.len() {
                    c.nodes.remove(r.node);
                }
                if c.nodes.len() < 3 {
                    c.closed = false;
                }
            }
            s.curves.retain(|c| c.nodes.len() > 1);
            remove_shape = s.curves.is_empty();
        }
        if remove_shape {
            self.doc.shapes.retain(|s| s.id != r.shape);
            self.doc.sel.remove(&r.shape);
            self.view.node_sel = None;
        } else {
            let left = self.doc.shapes.iter().find(|s| s.id == r.shape).and_then(|s| s.curves.get(r.curve)).map(|c| c.nodes.len()).unwrap_or(0);
            self.view.node_sel = if left > 0 && r.curve < self.doc.shapes.iter().find(|s| s.id == r.shape).map(|s| s.curves.len()).unwrap_or(0) { Some(NodeRef { node: r.node.min(left - 1), ..r }) } else { None };
        }
        self.edited();
    }

    /// Turn the selected shapes into curves with editable points
    pub fn convert_to_curves(&mut self) {
        let sel = self.doc.sel.clone();
        let targets: Vec<usize> = self.doc.shapes.iter().enumerate().filter(|(_, s)| sel.contains(&s.id) && s.kind != "spline" && s.kind != "image").map(|(i, _)| i).collect();
        if targets.is_empty() {
            return self.toast(if sel.is_empty() { "Select shapes to convert first." } else { "Those are already curves (pictures can't be converted)." }, false);
        }
        self.doc.checkpoint();
        let mut n = 0;
        for i in targets {
            if let Some(c) = crate::spline::to_spline(&self.doc.shapes[i]) {
                self.doc.shapes[i] = c;
                n += 1;
            }
        }
        self.view.forget_cache();
        self.edited();
        self.toast(format!("Converted {n} shape{} to editable curves. Use Edit points (N) to move points and handles.", if n == 1 { "" } else { "s" }), false);
    }

    pub fn finish_open_spline(&mut self) {
        if self.view.spline_new.is_some() {
            let mut ev = vec![];
            self.view.finish_spline(false, self.active_layer, &mut ev);
            self.handle_events(ev);
        }
    }
}
