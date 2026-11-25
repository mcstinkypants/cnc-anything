//! The window's panels and the per-frame loop.

use crate::app::{App, Tab};
use crate::canvas::{layer_color, tool_icon, CanvasEvent, SnapCfg, Tool};
use crate::gcode::*;
use crate::geom::*;
use crate::machine::{Cmd, LogKind};
use crate::materials::{CutProfile, Material};
use crate::model::*;
use egui::{Color32, RichText};
use std::sync::Arc;
use std::time::Duration;

const BLUE: Color32 = Color32::from_rgb(0x2f, 0x5f, 0xb3);
const GREEN: Color32 = Color32::from_rgb(0x2b, 0x7a, 0x45);
const RED: Color32 = Color32::from_rgb(0x8a, 0x2b, 0x2b);

impl App {
    pub fn handle_events(&mut self, ev: Vec<CanvasEvent>) {
        for e in ev {
            match e {
                CanvasEvent::AddShape(s) => self.add_shape(s),
                CanvasEvent::Edited => self.edited(),
                CanvasEvent::SelectionChanged => {}
                CanvasEvent::TextAt(p) => self.open_text_dlg(None, p),
                CanvasEvent::EditText(id) => self.open_text_dlg(Some(id), [0.0, 0.0]),
                CanvasEvent::Message(m) => self.toast(m, false),
            }
        }
    }

    fn ensure_textures(&mut self, ctx: &egui::Context) {
        for s in &self.doc.shapes {
            if s.kind != "image" {
                continue;
            }
            let k = img_key(&s.src);
            if self.textures.contains_key(&k) {
                continue;
            }
            let Some(g) = self.imgs.get(&k) else { continue };
            // pictures larger than 4096 px are shown at a reduced size (the job uses full size)
            let step = ((g.w.max(g.h) + 4095) / 4096).max(1);
            let (w, h) = (g.w / step, g.h / step);
            let mut px = Vec::with_capacity(w * h);
            for y in 0..h {
                for x in 0..w {
                    px.push((g.gray[(y * step) * g.w + x * step] * 255.0) as u8);
                }
            }
            let img = egui::ColorImage::from_gray([w.max(1), h.max(1)], &px);
            self.textures.insert(k, ctx.load_texture(format!("img{k}"), img, egui::TextureOptions::LINEAR));
        }
    }

    fn canvas_ui(&mut self, ui: &mut egui::Ui) {
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        self.view.rect = rect;
        self.view.router = self.dev.router();
        if !self.view.fitted && rect.width() > 100.0 {
            self.view.zoom_fit(&self.dev);
            self.view.fitted = true;
        }
        self.ensure_textures(ui.ctx());
        // laser head from the controller (work position, counted from the origin corner)
        {
            let s = self.mach.st.lock().unwrap();
            self.view.head = if s.connected { Some(self.dev.from_user(s.wpos)) } else { None };
        }
        if self.frame_until.map(|t| std::time::Instant::now() > t).unwrap_or(false) {
            self.view.frame_box = None;
            self.frame_until = None;
        }
        let cfg = SnapCfg { grid: self.snap_grid, obj: self.snap_obj, step: self.snap_step, lock_ratio: self.lock_ratio, poly_sides: self.poly_sides };
        let blocked = self.file_dlg.is_some() || self.text_dlg.is_some() || self.settings.is_some() || self.confirm.is_some();
        if !blocked {
            let ev = self.view.input(ui, &resp, &mut self.doc, &self.dev, &cfg, self.active_layer);
            self.handle_events(ev);
            if resp.hovered() {
                ui.ctx().set_cursor_icon(self.view.cursor(&self.doc));
            }
        }
        let painter = ui.painter_at(rect);
        let sb = self.doc.sel_box();
        self.view.paint(&painter, &self.doc, &self.dev, &self.textures, sb);
    }

    pub fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            ui.label(RichText::new("⚙ CNC Anything").strong().color(Color32::from_rgb(0xff, 0x8a, 0x3a))).on_hover_text(format!("Version {}, built from commit {}", crate::gcode::APP_VERSION, crate::gcode::GIT_COMMIT));
            ui.label(RichText::new(crate::gcode::version_label()).small().weak());
            ui.add_space(8.0);
            let item = |ui: &mut egui::Ui, label: &str, short: &str, c: &str, me: &mut App| {
                let text = if short.is_empty() { label.to_string() } else { format!("{label}    ({short})") };
                if ui.button(text).clicked() {
                    ui.close_menu();
                    me.cmd(c);
                }
            };
            ui.menu_button("File", |ui| {
                item(ui, "New project", "Ctrl+N", "new", self);
                item(ui, "Open project…", "Ctrl+O", "open", self);
                item(ui, "Save project", "Ctrl+S", "save", self);
                item(ui, "Save project as…", "", "saveAs", self);
                ui.separator();
                item(ui, "Import drawing or picture…", "Ctrl+I", "import", self);
                item(ui, "Save G-code…", "", "exportGcode", self);
                item(ui, "Export SVG…", "", "exportSvg", self);
                ui.separator();
                item(ui, "Load sample design", "", "sample", self);
            });
            ui.menu_button("Edit", |ui| {
                item(ui, "Undo", "Ctrl+Z", "undo", self);
                item(ui, "Redo", "Ctrl+Y", "redo", self);
                ui.separator();
                item(ui, "Cut", "Ctrl+X", "cut", self);
                item(ui, "Copy", "Ctrl+C", "copy", self);
                item(ui, "Paste", "Ctrl+V", "paste", self);
                item(ui, "Duplicate", "Ctrl+D", "duplicate", self);
                item(ui, "Delete", "Del", "delete", self);
                ui.separator();
                item(ui, "Select all", "Ctrl+A", "selectAll", self);
                item(ui, "Machine profiles…", "", "settings", self);
            });
            ui.menu_button("Arrange", |ui| {
                item(ui, "Align left edges", "", "alignL", self);
                item(ui, "Align centers horizontally", "", "alignCH", self);
                item(ui, "Align right edges", "", "alignR", self);
                item(ui, "Align top edges", "", "alignT", self);
                item(ui, "Align centers vertically", "", "alignCV", self);
                item(ui, "Align bottom edges", "", "alignB", self);
                ui.separator();
                item(ui, "Mirror horizontally", "", "mirrorH", self);
                item(ui, "Mirror vertically", "", "mirrorV", self);
                item(ui, "Rotate 90° clockwise", "", "rot90", self);
                item(ui, "Rotate 90° counter-clockwise", "", "rot-90", self);
                ui.separator();
                item(ui, "Move to center of bed", "", "centerBed", self);
                item(ui, "Convert to curve (editable points)", "", "toPath", self);
            });
            ui.menu_button("View", |ui| {
                item(ui, "Zoom to the whole bed", "", "zoomFit", self);
                item(ui, "Zoom to selection", "", "zoomSel", self);
                ui.separator();
                let mut changed = ui.checkbox(&mut self.view.dark, "Dark canvas").changed();
                changed |= ui.checkbox(&mut self.snap_grid, "Snap to grid").changed();
                changed |= ui.checkbox(&mut self.snap_obj, "Snap to object centers").changed();
                if changed {
                    self.save_prefs();
                }
            });
            ui.menu_button("Libraries", |ui| {
                item(ui, "Tool library…", "", "tools", self);
                item(ui, "Speeds & feeds…", "", "feeds", self);
                item(ui, "Laser materials…", "", "materials", self);
                ui.separator();
                item(ui, "Import tools (JSON)…", "", "importTools", self);
                item(ui, "Export tools (JSON)…", "", "exportTools", self);
                item(ui, "Import speeds & feeds (JSON)…", "", "importFeeds", self);
                item(ui, "Export speeds & feeds (JSON)…", "", "exportFeeds", self);
                item(ui, "Import laser materials (JSON)…", "", "importMaterials", self);
                item(ui, "Export laser materials (JSON)…", "", "exportMaterials", self);
            });
            ui.menu_button("Machine", |ui| {
                item(ui, "Use as laser cutter", "", "machineLaser", self);
                item(ui, "Use as CNC router", "", "machineRouter", self);
                ui.separator();
                item(ui, "Connect practice machine", "", "connectSim", self);
                item(ui, "Disconnect", "", "disconnect", self);
                ui.separator();
                item(ui, "Preview job", "Alt+P", "preview", self);
                item(ui, "Start job", "", "start", self);
                item(ui, "Frame", "", "frame", self);
                item(ui, "Home", "", "home", self);
                ui.separator();
                item(ui, "Machine profiles…", "", "settings", self);
            });
            ui.menu_button("Help", |ui| {
                item(ui, "Getting started", "", "help", self);
                item(ui, "Keyboard shortcuts", "", "keys", self);
                item(ui, "About CNC Anything", "", "about", self);
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("{}{}", self.doc_name, if self.doc.dirty { " •" } else { "" })).weak());
            });
        });
    }

    pub fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (label, tip, c) in [("🗋", "New project", "new"), ("🗁", "Open project", "open"), ("🖴", "Save project", "save"), ("⮋", "Import drawing or picture", "import")] {
                if ui.button(label).on_hover_text(tip).clicked() {
                    self.cmd(c);
                }
            }
            ui.separator();
            if ui.button("⟲").on_hover_text("Undo").clicked() {
                self.cmd("undo");
            }
            if ui.button("⟳").on_hover_text("Redo").clicked() {
                self.cmd("redo");
            }
            ui.separator();
            if ui.button("⛶").on_hover_text("Zoom to the whole bed").clicked() {
                self.cmd("zoomFit");
            }
            if ui.button("🔍").on_hover_text("Zoom to selection").clicked() {
                self.cmd("zoomSel");
            }
            if ui.button("👁 Preview").on_hover_text("Preview the job").clicked() {
                self.cmd("preview");
            }
            if ui.button("⚙").on_hover_text("Machine profiles").clicked() {
                self.cmd("settings");
            }
            ui.separator();
            self.profile_picker(ui);
            let mut router = self.dev.router();
            ui.selectable_value(&mut router, false, "🔆 Laser").on_hover_text("Laser cutter or engraver");
            ui.selectable_value(&mut router, true, "🛠 Router").on_hover_text("2D CNC router: cuts with a spinning bit in depth passes");
            if router != self.dev.router() {
                self.cmd(if router { "machineRouter" } else { "machineLaser" });
            }
            if router {
                if ui.button("🔧 Tools").on_hover_text("Tool library").clicked() {
                    self.cmd("tools");
                }
                if ui.button("📊 Speeds & feeds").on_hover_text("Speeds & feeds library").clicked() {
                    self.cmd("feeds");
                }
            } else if ui.button("📚 Materials").on_hover_text("Laser material library").clicked() {
                self.cmd("materials");
            }
            ui.separator();
            let old = (self.snap_grid, self.snap_obj, self.snap_step, self.view.dark);
            ui.toggle_value(&mut self.snap_grid, "# Grid snap").on_hover_text("Snap to the grid (hold Alt to turn snapping off for a moment)");
            ui.add_enabled_ui(self.snap_grid, |ui| {
                let label = if self.snap_step == 0.0 { "follow grid".to_string() } else { format!("{} mm", fmt(self.snap_step, 1, false)) };
                egui::ComboBox::from_id_salt("snapstep").width(92.0).selected_text(label).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.snap_step, 0.0, "follow grid");
                    for v in [0.1, 0.5, 1.0, 2.0, 5.0, 10.0] {
                        ui.selectable_value(&mut self.snap_step, v, format!("{} mm", fmt(v, 1, false)));
                    }
                });
            });
            ui.toggle_value(&mut self.snap_obj, "◎ Center snap").on_hover_text("Snap to the centers of other objects and the bed");
            ui.toggle_value(&mut self.view.dark, "🌙 Dark canvas").on_hover_text("Dark bed with light outlines, easier on the eyes");
            if old != (self.snap_grid, self.snap_obj, self.snap_step, self.view.dark) {
                self.save_prefs();
            }
        });
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            if !self.node_numeric(ui) {
                ui.label(RichText::new("Position").weak());
                self.numeric(ui);
            }
        });
    }

    /// The selection's anchor point in world coordinates
    fn anchor_of(&self, b: &BBox) -> P {
        let x = match self.pos_anchor.1 {
            0 => b.x0,
            2 => b.x1,
            _ => b.cx(),
        };
        let y = match self.pos_anchor.0 {
            0 => b.y1,
            2 => b.y0,
            _ => b.cy(),
        };
        [x, y]
    }

    /// 3 x 3 dots to pick which point of the selection X and Y refer to
    fn anchor_picker(&mut self, ui: &mut egui::Ui) {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
        let resp = resp.on_hover_text("Position anchor: X and Y give this point of the selection, and typed sizes keep it in place. Click a corner, a side or the center.");
        let p = ui.painter();
        p.rect_stroke(rect.shrink(4.0), 1.0, egui::Stroke::new(1.0_f32, Color32::from_gray(0x70)));
        for r in 0..3u8 {
            for c in 0..3u8 {
                let pos = rect.min + egui::vec2(4.0 + c as f32 * 9.0, 4.0 + r as f32 * 9.0);
                let on = self.pos_anchor == (r, c);
                p.circle_filled(pos, if on { 3.5 } else { 2.0 }, if on { Color32::from_rgb(0xff, 0x8a, 0x3a) } else { Color32::from_gray(0xa0) });
            }
        }
        if resp.clicked() {
            if let Some(m) = resp.interact_pointer_pos() {
                let c = (((m.x - rect.min.x - 4.0) / 9.0).round().clamp(0.0, 2.0)) as u8;
                let r = (((m.y - rect.min.y - 4.0) / 9.0).round().clamp(0.0, 2.0)) as u8;
                self.pos_anchor = (r, c);
                self.num = [0.0; 5];
                self.save_prefs();
            }
        }
    }

    fn numeric(&mut self, ui: &mut egui::Ui) {
        let b = self.doc.sel_box();
        let rot_now = if self.doc.sel.len() == 1 { mat_rotation(&self.doc.selected()[0].tf) } else { 0.0 };
        if let Some(b) = b {
            let a = self.dev.to_user(self.anchor_of(&b));
            if !ui.ctx().is_using_pointer() || self.num == [0.0; 5] {
                self.num = [a[0], a[1], b.w(), b.h(), rot_now];
            }
        } else {
            self.num = [0.0; 5];
        }
        let mut changed: Option<usize> = None;
        self.anchor_picker(ui);
        ui.add_enabled_ui(b.is_some(), |ui| {
            for (i, l) in ["X", "Y", "W", "H"].iter().enumerate() {
                ui.label(*l);
                let r = ui.add(egui::DragValue::new(&mut self.num[i]).speed(0.1).fixed_decimals(2));
                if r.drag_started() || r.gained_focus() {
                    self.doc.checkpoint();
                }
                if r.changed() {
                    changed = Some(i);
                }
            }
            if ui.selectable_label(self.lock_ratio, if self.lock_ratio { "🔒" } else { "🔓" }).on_hover_text("Keep proportions").clicked() {
                self.lock_ratio = !self.lock_ratio;
            }
            ui.label("Rotate");
            let r = ui.add(egui::DragValue::new(&mut self.num[4]).speed(1.0).suffix("°").max_decimals(1));
            if r.drag_started() || r.gained_focus() {
                self.doc.checkpoint();
            }
            if r.changed() {
                changed = Some(4);
            }
            ui.label(RichText::new("mm").weak());
        });
        if let (Some(i), Some(b)) = (changed, b) {
            let v = self.num[i];
            let a = self.anchor_of(&b);
            let m = match i {
                0 => translate(v - a[0], 0.0),
                1 => translate(0.0, self.dev.from_user([0.0, v])[1] - a[1]),
                2 | 3 if v > 0.001 => {
                    let (mut sx, mut sy) = if i == 2 { (v / b.w().max(1e-6), 1.0) } else { (1.0, v / b.h().max(1e-6)) };
                    if self.lock_ratio {
                        if i == 2 {
                            sy = sx;
                        } else {
                            sx = sy;
                        }
                    }
                    scale_about(sx, sy, a[0], a[1])
                }
                4 => rotate_about(v - rot_now, b.cx(), b.cy()),
                _ => IDENT,
            };
            self.doc.transform_sel(&m);
            self.edited();
        }
        // options for one selected shape
        let sel: Vec<u64> = self.doc.sel.iter().copied().collect();
        if sel.len() == 1 {
            let id = sel[0];
            let Some(idx) = self.doc.shapes.iter().position(|s| s.id == id) else { return };
            let kind = self.doc.shapes[idx].kind.clone();
            ui.separator();
            match kind.as_str() {
                "rect" => {
                    ui.label("Corner radius");
                    let mut r = self.doc.shapes[idx].r;
                    let resp = ui.add(egui::DragValue::new(&mut r).speed(0.2).range(0.0..=1000.0).suffix(" mm"));
                    if resp.drag_started() || resp.gained_focus() {
                        self.doc.checkpoint();
                    }
                    if resp.changed() {
                        self.doc.shapes[idx].r = r;
                        self.edited();
                    }
                }
                "polygon" => {
                    ui.label("Sides");
                    let mut n = self.doc.shapes[idx].sides;
                    let resp = ui.add(egui::DragValue::new(&mut n).range(3..=64));
                    if resp.drag_started() || resp.gained_focus() {
                        self.doc.checkpoint();
                    }
                    if resp.changed() {
                        self.doc.shapes[idx].sides = n;
                        self.poly_sides = n;
                        self.edited();
                    }
                }
                "text" => {
                    if ui.button("Edit text…").clicked() {
                        self.open_text_dlg(Some(id), [0.0, 0.0]);
                    }
                    ui.label("Size");
                    let mut size = self.doc.shapes[idx].size;
                    let resp = ui.add(egui::DragValue::new(&mut size).speed(0.1).range(0.5..=1000.0).max_decimals(2).suffix(" mm")).on_hover_text("Text size (font height). Resizing the text on the canvas changes it too.");
                    if resp.drag_started() || resp.gained_focus() {
                        self.doc.checkpoint();
                    }
                    if resp.changed() && size > 0.0 {
                        let s = &mut self.doc.shapes[idx];
                        let k = size / s.size.max(1e-6);
                        s.tf = mul(&s.tf, &[k, 0.0, 0.0, k, 0.0, 0.0]);
                        s.bake_scale();
                        self.edited();
                    }
                    if let Some((ratio, slant)) = self.doc.shapes[idx].text_stretch() {
                        let what = if (ratio - 1.0).abs() > 0.005 { format!("↔ stretched to {:.0}% width", ratio * 100.0) } else { format!("slanted {:.0}°", slant.atan().to_degrees()) };
                        ui.label(RichText::new(what).color(Color32::from_rgb(0xff, 0xa0, 0x30))).on_hover_text("This text isn't in its font's normal proportions");
                        if ui.small_button("Unstretch").on_hover_text("Back to the font's normal proportions, keeping the height").clicked() {
                            self.doc.checkpoint();
                            self.doc.shapes[idx].unstretch_text();
                            self.edited();
                        }
                    }
                }
                "image" => {
                    let mut inv = self.doc.shapes[idx].invert;
                    if ui.checkbox(&mut inv, "Invert this picture").changed() {
                        self.doc.checkpoint();
                        self.doc.shapes[idx].invert = inv;
                        self.edited();
                    }
                }
                _ => {}
            }
            if kind == "spline" {
                if ui.button("✎ Edit points").on_hover_text("Move this curve's points and handles (N)").clicked() {
                    self.set_tool(Tool::Nodes);
                }
            } else if kind != "image" && ui.button("Convert to curve").on_hover_text("Turn this shape into a curve whose points and handles you can move").clicked() {
                self.convert_to_curves();
            }
        } else if self.view.tool == Tool::Polygon {
            ui.separator();
            ui.label("Sides");
            ui.add(egui::DragValue::new(&mut self.poly_sides).range(3..=64));
        }
    }

    pub fn tools_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        for t in Tool::ALL {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::click());
            let resp = resp.on_hover_text(t.label());
            let active = self.view.tool == t;
            let p = ui.painter();
            if active {
                p.rect_filled(rect, 4.0, BLUE);
            } else if resp.hovered() {
                p.rect_filled(rect, 4.0, Color32::from_rgb(0x45, 0x4a, 0x51));
            }
            tool_icon(p, rect, t, Color32::from_gray(0xe3));
            if resp.clicked() {
                self.set_tool(t);
            }
            ui.add_space(2.0);
        }
    }

    pub fn cuts_panel(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("CUTS / LAYERS").small().strong().weak());
        let used = self.doc.used_layers();
        if self.edit_layer.map(|l| !used.contains(&l)).unwrap_or(true) {
            self.edit_layer = used.first().copied();
        }
        if used.is_empty() {
            ui.label(RichText::new("Draw something, then pick a color below to choose its layer.").weak());
            return;
        }
        egui::Grid::new("cuts").striped(true).num_columns(6).spacing([7.0, 4.0]).show(ui, |ui| {
            let heads = if self.dev.router() { ["#", "Layer", "Operation", "Tool / Depth", "Out", "Show"] } else { ["#", "Layer", "Mode", "Speed / Power", "Out", "Show"] };
            for h in heads {
                ui.label(RichText::new(h).small().weak());
            }
            ui.end_row();
            for (n, &li) in used.iter().enumerate() {
                let sel = self.edit_layer == Some(li);
                let (mode, sp) = {
                    let l = &self.doc.layers[li];
                    if self.dev.router() {
                        let short = match l.op.as_str() {
                            "Profile outside" => "Outside",
                            "Profile inside" => "Inside",
                            o => o,
                        };
                        (short.to_string(), format!("T{} / {} mm", l.tool_id, fmt(l.depth, 2, false)))
                    } else {
                        (l.mode.clone(), format!("{} / {}%", fmt(l.speed, 1, false), fmt(l.power, 1, false)))
                    }
                };
                let r1 = ui.selectable_label(sel, format!("{}", n + 1));
                let (rect, r2) = ui.allocate_exact_size(egui::vec2(30.0, 16.0), egui::Sense::click());
                ui.painter().rect_filled(rect, 3.0, layer_color(li));
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, format!("{li:02}"), egui::FontId::proportional(10.0), Color32::WHITE);
                let r3 = ui.selectable_label(sel, mode);
                let r4 = ui.selectable_label(sel, sp);
                let (mut out, mut show) = (self.doc.layers[li].output, self.doc.layers[li].show);
                let c1 = ui.checkbox(&mut out, "").changed();
                let c2 = ui.checkbox(&mut show, "").changed();
                if c1 || c2 {
                    self.doc.checkpoint();
                    self.doc.layers[li].output = out;
                    self.doc.layers[li].show = show;
                    self.edited();
                }
                if r1.clicked() || r2.clicked() || r3.clicked() || r4.clicked() {
                    self.edit_layer = Some(li);
                    self.active_layer = li;
                }
                if r1.double_clicked() || r2.double_clicked() {
                    self.doc.sel = self.doc.shapes.iter().filter(|s| s.layer == li).map(|s| s.id).collect();
                }
                ui.end_row();
            }
        });
        ui.horizontal(|ui| {
            let mut d = 0;
            if ui.small_button("⬆").on_hover_text("Cut this layer earlier").clicked() {
                d = -1;
            }
            if ui.small_button("⬇").on_hover_text("Cut this layer later").clicked() {
                d = 1;
            }
            if d != 0 {
                let mut o = self.doc.used_layers();
                if let Some(i) = self.edit_layer.and_then(|l| o.iter().position(|x| *x == l)) {
                    let j = i as i32 + d;
                    if j >= 0 && (j as usize) < o.len() {
                        self.doc.checkpoint();
                        o.swap(i, j as usize);
                        self.doc.order = o;
                        self.edited();
                    }
                }
            }
        });
        ui.separator();
        self.layer_editor(ui);
    }

    fn layer_editor(&mut self, ui: &mut egui::Ui) {
        if self.dev.router() {
            return self.router_layer_editor(ui);
        }
        let Some(li) = self.edit_layer else { return };
        let mut l = self.doc.layers[li].clone();
        let before = l.clone();
        ui.horizontal(|ui| {
            ui.label("Material");
            let current = if l.material.is_empty() { "— none —".to_string() } else { l.material.clone() };
            let mut chosen: Option<usize> = None;
            egui::ComboBox::from_id_salt("layermat").width(170.0).selected_text(current).show_ui(ui, |ui| {
                if self.lib.materials.is_empty() {
                    ui.label(RichText::new("The library is empty").weak());
                }
                for (i, m) in self.lib.materials.iter().enumerate() {
                    if ui.selectable_label(false, m.title()).on_hover_text(m.profile.summary()).clicked() {
                        chosen = Some(i);
                    }
                }
            });
            if let Some(i) = chosen {
                let m = self.lib.materials[i].clone();
                m.profile.apply_to(&mut l);
                l.material = m.title();
                self.toast(format!("Applied \"{}\" to layer {}", m.title(), l.name), false);
            }
            if ui.button("Save as…").on_hover_text("Keep these settings in the material library").clicked() {
                let op = match l.mode.as_str() {
                    "Line" => "Cut",
                    "Fill" => "Engrave",
                    _ => "Picture",
                };
                let name = if l.name.len() == 3 && l.name.starts_with('C') { "New material".to_string() } else { l.name.clone() };
                self.lib.materials.push(Material { name, thickness: if l.mode == "Image" { 0.0 } else { 3.0 }, operation: op.into(), notes: String::new(), profile: CutProfile::from_layer(&l) });
                self.mat_sel = Some(self.lib.materials.len() - 1);
                self.mat_open = true;
                let _ = self.lib.save();
                self.toast("Saved to the material library: give it a name there.", false);
            }
        });
        egui::Grid::new("layered").num_columns(4).spacing([6.0, 4.0]).show(ui, |ui| {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut l.name).desired_width(90.0));
            ui.label("Mode");
            egui::ComboBox::from_id_salt("mode").width(70.0).selected_text(l.mode.clone()).show_ui(ui, |ui| {
                for m in ["Line", "Fill", "Image"] {
                    ui.selectable_value(&mut l.mode, m.to_string(), m);
                }
            });
            ui.end_row();
            ui.label("Speed");
            ui.add(egui::DragValue::new(&mut l.speed).speed(1.0).range(0.1..=2000.0).suffix(" mm/s"));
            ui.label("Power");
            ui.add(egui::DragValue::new(&mut l.power).speed(0.5).range(0.0..=100.0).suffix(" %"));
            ui.end_row();
            ui.label("Passes");
            ui.add(egui::DragValue::new(&mut l.passes).range(1..=100));
            if l.mode == "Image" && l.dither == "grayscale" {
                ui.label("Min power");
                ui.add(egui::DragValue::new(&mut l.min_power).speed(0.5).range(0.0..=100.0).suffix(" %"));
            }
            ui.end_row();
            if l.mode != "Line" {
                ui.label("Interval");
                ui.add(egui::DragValue::new(&mut l.interval).speed(0.01).range(0.01..=5.0).suffix(" mm"));
                ui.label("Overscan");
                ui.add(egui::DragValue::new(&mut l.overscan).speed(0.5).range(0.0..=50.0).suffix(" %"));
                ui.end_row();
            }
        });
        if l.mode == "Image" {
            ui.horizontal(|ui| {
                ui.label("Picture style");
                let names = [("jarvis", "Jarvis dither"), ("stucki", "Stucki dither"), ("floyd", "Floyd-Steinberg"), ("atkinson", "Atkinson dither"), ("ordered", "Ordered pattern"), ("threshold", "Black & white"), ("grayscale", "Grayscale (varies power)")];
                let cur = names.iter().find(|n| n.0 == l.dither).map(|n| n.1).unwrap_or("Jarvis dither");
                egui::ComboBox::from_id_salt("dither").selected_text(cur).show_ui(ui, |ui| {
                    for (v, t) in names {
                        ui.selectable_value(&mut l.dither, v.to_string(), t);
                    }
                });
                ui.label(RichText::new(format!("{} DPI", (25.4 / l.interval).round())).weak());
            });
        }
        ui.horizontal_wrapped(|ui| {
            if l.mode != "Line" {
                ui.checkbox(&mut l.bidir, "Both directions");
            }
            if l.mode == "Fill" {
                ui.checkbox(&mut l.cross_hatch, "Cross-hatch");
            }
            if l.mode == "Image" {
                ui.checkbox(&mut l.invert, "Invert");
            }
            ui.checkbox(&mut l.air, "Air assist");
            if ui.small_button("Reset").on_hover_text("Reset this layer to defaults").clicked() {
                let mut d = Layer::new(li);
                d.name = l.name.clone();
                d.mode = l.mode.clone();
                l = d;
            }
        });
        if l != before {
            // a hand change means the layer no longer matches its material exactly
            if CutProfile::from_layer(&l) != CutProfile::from_layer(&before) && l.material == before.material && !before.material.is_empty() {
                l.material = format!("{} (edited)", before.material.trim_end_matches(" (edited)"));
            }
            self.doc.checkpoint();
            self.doc.layers[li] = l;
            self.edited();
        }
    }

    pub fn laser_panel(&mut self, ui: &mut egui::Ui) {
        let (connected, sim, state, wpos, wz, job, done, tool_note) = {
            let s = self.mach.st.lock().unwrap();
            let note = s.log.iter().rev().take(60).find(|l| l.1.starts_with("Tool change:")).map(|l| l.1.clone());
            (s.connected, s.sim, s.state.clone(), s.wpos, s.wz, s.job.clone(), s.done.clone(), note)
        };
        let router = self.dev.router();
        egui::Frame::none().fill(Color32::from_rgb(0x26, 0x28, 0x2c)).rounding(4.0).inner_margin(egui::Margin::same(6.0)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let c = if !connected {
                    Color32::GRAY
                } else if state.starts_with("Idle") {
                    Color32::from_rgb(0x3c, 0xc4, 0x6a)
                } else if ["Run", "Jog", "Home", "Connecting"].iter().any(|s| state.starts_with(s)) {
                    Color32::from_rgb(0xff, 0xb0, 0x20)
                } else {
                    Color32::from_rgb(0xff, 0x4f, 0x4f)
                };
                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 5.0, c);
                ui.label(if connected { format!("{}: {state}", if sim { "Practice machine" } else if router { "Router" } else { "Laser" }) } else { "Not connected".into() });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let pos = if !connected {
                        if router { "X —  Y —  Z —".to_string() } else { "X —  Y —".into() }
                    } else if router {
                        format!("X {:.2}  Y {:.2}  Z {:.2}", wpos[0], wpos[1], wz)
                    } else {
                        format!("X {:.2}  Y {:.2}", wpos[0], wpos[1])
                    };
                    ui.monospace(pos);
                });
            });
        });
        if !connected {
            ui.horizontal(|ui| {
                ui.label("Port");
                let label = if self.dev.port.is_empty() { "none found".to_string() } else { self.dev.port.clone() };
                egui::ComboBox::from_id_salt("port").width(150.0).selected_text(label).show_ui(ui, |ui| {
                    for p in self.ports.clone() {
                        if ui.selectable_value(&mut self.dev.port, p.clone(), p).changed() {
                            self.save_prefs();
                        }
                    }
                });
                if ui.button("⟳").on_hover_text("Look for serial ports again").clicked() {
                    self.ports = crate::machine::list_ports();
                    if !self.ports.contains(&self.dev.port) {
                        self.dev.port = self.ports.first().cloned().unwrap_or_default();
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new("Connect").fill(BLUE)).clicked() {
                    self.cmd("connectSerial");
                }
                if ui.button("Practice machine").clicked() {
                    self.cmd("connectSim");
                }
            });
        } else if ui.button("Disconnect").clicked() {
            self.cmd("disconnect");
        }
        let w = ((ui.available_width() - 12.0) / 3.0).max(60.0);
        egui::Grid::new("lbtns").num_columns(3).spacing([6.0, 5.0]).show(ui, |ui| {
            for (t, c) in [("Home", "home"), ("Frame", "frame"), ("Unlock", "unlock")] {
                if ui.add_sized([w, 24.0], egui::Button::new(t)).clicked() {
                    self.cmd(c);
                }
            }
            ui.end_row();
            if ui.add_sized([w, 24.0], egui::Button::new("▶ Start").fill(GREEN)).clicked() {
                self.cmd("start");
            }
            let paused = job.as_ref().map(|j| j.paused).unwrap_or(false) || state.starts_with("Hold");
            if ui.add_sized([w, 24.0], egui::Button::new(if paused { "⏵ Resume" } else { "⏸ Pause" })).clicked() {
                self.cmd("pause");
            }
            if ui.add_sized([w, 24.0], egui::Button::new("⏹ Stop").fill(RED)).clicked() {
                self.cmd("stop");
            }
            ui.end_row();
        });
        ui.horizontal(|ui| {
            if ui.button("Preview").clicked() {
                self.cmd("preview");
            }
            if ui.button("Save G-code").clicked() {
                self.cmd("exportGcode");
            }
        });
        if job.is_some() && state.starts_with("Hold") && !job.as_ref().map(|j| j.paused).unwrap_or(false) {
            egui::Frame::none().fill(Color32::from_rgb(0x5a, 0x45, 0x10)).rounding(4.0).inner_margin(egui::Margin::same(6.0)).show(ui, |ui| {
                ui.label(RichText::new(format!("⏸ Paused by the job. {}", tool_note.as_deref().unwrap_or("Press Resume to carry on."))).color(Color32::WHITE));
            });
        }
        if let Some(j) = &job {
            let f = if j.total > 0 { j.acked as f32 / j.total as f32 } else { 0.0 };
            ui.add(egui::ProgressBar::new(f).show_percentage());
            let el = j.started.elapsed().as_secs_f64();
            let est = if self.last_job_time > 0.0 { format!(" of about {}", fmt_time(self.last_job_time)) } else { String::new() };
            ui.label(RichText::new(format!("{}{}/{} lines · {} elapsed{est}", if j.paused { "Paused · " } else { "" }, j.acked, j.total, fmt_time(el))).small());
        } else if let Some(d) = &done {
            let t = if d.ok {
                format!("Last job finished in {}{}", fmt_time(d.secs as f64), if d.errors > 0 { format!(" · {} error(s), see Console", d.errors) } else { String::new() })
            } else {
                format!("Last job stopped: {}", d.msg)
            };
            ui.label(RichText::new(t).small());
        }
        ui.horizontal(|ui| {
            ui.label("Start from");
            let label = match self.start_from {
                StartFrom::Absolute => "Absolute coordinates",
                StartFrom::User => "User origin",
                StartFrom::Current => "Current position",
            };
            egui::ComboBox::from_id_salt("startfrom").selected_text(label).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.start_from, StartFrom::Absolute, "Absolute coordinates");
                ui.selectable_value(&mut self.start_from, StartFrom::User, "User origin");
                ui.selectable_value(&mut self.start_from, StartFrom::Current, "Current position");
            });
        });
        ui.add_enabled_ui(self.start_from != StartFrom::Absolute, |ui| {
            ui.horizontal(|ui| {
                ui.label("Job origin");
                egui::Grid::new("anchor").spacing([2.0, 2.0]).show(ui, |ui| {
                    for r in 0..3u8 {
                        for c in 0..3u8 {
                            if ui.radio(self.anchor == (r, c), "").clicked() {
                                self.anchor = (r, c);
                            }
                        }
                        ui.end_row();
                    }
                });
            });
        });
        ui.checkbox(&mut self.sel_only, "Cut selected graphics only");
        ui.checkbox(&mut self.inner_first, "Cut inner shapes first");
    }

    pub fn move_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            egui::Grid::new("jog").spacing([4.0, 4.0]).show(ui, |ui| {
                let rows = [[("↖", -1.0, 1.0), ("↑", 0.0, 1.0), ("↗", 1.0, 1.0)], [("←", -1.0, 0.0), ("⌂", 0.0, 0.0), ("→", 1.0, 0.0)], [("↙", -1.0, -1.0), ("↓", 0.0, -1.0), ("↘", 1.0, -1.0)]];
                for cells in rows {
                    for (t, dx, dy) in cells {
                        if ui.add_sized([42.0, 38.0], egui::Button::new(RichText::new(t).size(18.0))).clicked() {
                            if dx == 0.0 && dy == 0.0 {
                                self.cmd("home");
                            } else {
                                self.jog(dx, dy);
                            }
                        }
                    }
                    ui.end_row();
                }
            });
            ui.vertical(|ui| {
                ui.label("Distance");
                ui.add(egui::DragValue::new(&mut self.jog_dist).speed(0.5).range(0.01..=1000.0).suffix(" mm"));
                ui.label("Speed");
                ui.add(egui::DragValue::new(&mut self.jog_speed).speed(1.0).range(1.0..=500.0).suffix(" mm/s"));
            });
            if self.dev.router() {
                ui.vertical(|ui| {
                    if ui.add_sized([46.0, 38.0], egui::Button::new(RichText::new("Z+").size(16.0))).on_hover_text("Raise the bit").clicked() {
                        self.jog_up_down(1.0);
                    }
                    if ui.add_sized([46.0, 38.0], egui::Button::new(RichText::new("Z−").size(16.0))).on_hover_text("Lower the bit").clicked() {
                        self.jog_up_down(-1.0);
                    }
                    ui.add(egui::DragValue::new(&mut self.jog_z).speed(0.05).range(0.01..=50.0).suffix(" mm")).on_hover_text("Z step");
                });
            }
        });
        ui.add_space(4.0);
        let w = ((ui.available_width() - 6.0) / 2.0).max(80.0);
        egui::Grid::new("movebtns").num_columns(2).spacing([6.0, 5.0]).show(ui, |ui| {
            let buttons: Vec<(&str, &str)> = if self.dev.router() {
                vec![("Set XY zero here", "setOrigin"), ("Set Z zero here", "setZero"), ("Go to origin", "gotoOrigin"), ("Move to selection", "gotoSel")]
            } else {
                vec![("Set origin here", "setOrigin"), ("Go to origin", "gotoOrigin"), ("Move laser to selection", "gotoSel"), ("Fire (pulse)", "pulse")]
            };
            for (i, (t, c)) in buttons.iter().enumerate() {
                if ui.add_sized([w, 24.0], egui::Button::new(*t)).clicked() {
                    self.cmd(c);
                }
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        ui.label(RichText::new("Arrow keys jog the head while this tab is open and nothing is selected.").small().weak());
    }

    pub fn console_panel(&mut self, ui: &mut egui::Ui) {
        let log: Vec<(LogKind, String)> = {
            let s = self.mach.st.lock().unwrap();
            s.log.iter().rev().take(400).cloned().collect::<Vec<_>>().into_iter().rev().collect()
        };
        let h = (ui.available_height() - 64.0).max(140.0);
        egui::Frame::none().fill(Color32::from_rgb(0x1d, 0x1f, 0x22)).inner_margin(egui::Margin::same(4.0)).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("console").max_height(h).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                for (k, t) in &log {
                    let (c, pre) = match k {
                        LogKind::Out => (Color32::from_rgb(0x8f, 0xc1, 0xff), "> "),
                        LogKind::Err => (Color32::from_rgb(0xff, 0x80, 0x80), ""),
                        LogKind::Info => (Color32::from_rgb(0xb6, 0xe3, 0xa0), ""),
                        LogKind::In => (Color32::from_gray(0xdd), ""),
                    };
                    ui.label(RichText::new(format!("{pre}{t}")).monospace().size(11.5).color(c));
                }
            });
        });
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.console_in).hint_text("Type a command, e.g. $$ or G0 X10").desired_width(ui.available_width() - 60.0).font(egui::TextStyle::Monospace));
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Send").clicked() || enter {
                let v = self.console_in.trim().to_string();
                if !v.is_empty() && self.need() {
                    if v == "?" || v == "!" || v == "~" {
                        self.mach.cmd(Cmd::Realtime(v.as_bytes()[0]));
                    } else {
                        self.mach.send(&v);
                    }
                    self.console_in.clear();
                }
                if enter {
                    r.request_focus();
                }
            }
        });
        ui.horizontal(|ui| {
            for c in ["$$", "$I", "$G", "$#"] {
                if ui.small_button(c).clicked() && self.need() {
                    self.mach.send(c);
                }
            }
            if ui.small_button("Clear").clicked() {
                self.mach.st.lock().unwrap().log.clear();
            }
        });
    }

    pub fn palette(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Layer color").weak());
            for i in 0..30 {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(30.0, 20.0), egui::Sense::click());
                let p = ui.painter();
                p.rect_filled(rect, 2.0, layer_color(i));
                p.text(rect.center(), egui::Align2::CENTER_CENTER, format!("{i:02}"), egui::FontId::proportional(10.0), Color32::WHITE);
                if i == self.active_layer {
                    p.rect_stroke(rect.expand(1.0), 2.0, egui::Stroke::new(2.0_f32, Color32::WHITE));
                }
                let resp = resp.on_hover_text(format!("Layer C{i:02}: click to put the selected shapes on this layer"));
                if resp.clicked() {
                    self.active_layer = i;
                    if !self.doc.sel.is_empty() {
                        self.doc.checkpoint();
                        let sel = self.doc.sel.clone();
                        let mut has_img = false;
                        for s in self.doc.shapes.iter_mut().filter(|s| sel.contains(&s.id)) {
                            s.layer = i;
                            has_img |= s.kind == "image";
                        }
                        if has_img {
                            self.doc.layers[i].mode = "Image".into();
                        }
                        self.edit_layer = Some(i);
                        self.edited();
                        self.toast(format!("Moved {} shape{} to layer C{i:02}", sel.len(), if sel.len() > 1 { "s" } else { "" }), false);
                    } else {
                        self.toast(format!("New shapes will go on layer C{i:02}"), false);
                    }
                }
            }
        });
    }

    pub fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let u = self.dev.to_user(self.view.mouse);
            ui.monospace(format!("x: {:>7.2}  y: {:>7.2} mm", u[0], u[1]));
            ui.separator();
            let n = self.doc.sel.len();
            ui.label(if n > 0 { format!("{n} selected") } else { format!("{} shape{}", self.doc.shapes.len(), if self.doc.shapes.len() == 1 { "" } else { "s" }) });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.monospace(format!("{:.0}%", self.view.z as f64 * 100.0 / 3.78));
                if let Some((m, err, _)) = &self.toast {
                    ui.label(RichText::new(m).color(if *err { Color32::from_rgb(0xff, 0x9a, 0x9a) } else { Color32::LIGHT_GRAY }));
                }
            });
        });
    }

    fn preview_bar(&mut self, ctx: &egui::Context) {
        let Some(pv) = &mut self.view.preview else { return };
        let (mut close, mut start, mut save) = (false, false, false);
        egui::Area::new(egui::Id::new("pvbar")).anchor(egui::Align2::CENTER_BOTTOM, [0.0, -72.0]).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(10.0)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Preview");
                    ui.add(egui::Slider::new(&mut pv.frac, 0.0..=1.0).show_value(false));
                    ui.checkbox(&mut pv.travel, "Show travel moves");
                    ui.label(RichText::new(&pv.info).weak());
                    save = ui.button("Save G-code").clicked();
                    start = ui.add(egui::Button::new("▶ Start").fill(GREEN)).clicked();
                    close = ui.button("Close preview").clicked();
                });
            });
        });
        if close {
            self.view.preview = None;
        }
        if save {
            self.cmd("exportGcode");
        }
        if start {
            self.cmd("start");
        }
    }

    fn screenshot_step(&mut self, ctx: &egui::Context) {
        let Some(path) = self.shot.clone() else { return };
        self.frame_no += 1;
        ctx.request_repaint();
        let shots: Vec<Arc<egui::ColorImage>> = ctx.input(|i| i.raw.events.iter().filter_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None }).collect());
        if let Some(img) = shots.into_iter().next() {
            let [w, h] = img.size;
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(d) = path.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            match image::RgbaImage::from_raw(w as u32, h as u32, bytes).map(|im| im.save(&path)) {
                Some(Ok(())) => println!("saved {}", path.display()),
                _ => {
                    eprintln!("error: could not save {}", path.display());
                    std::process::exit(1);
                }
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.frame_no >= 120 && !self.shot_asked {
            self.shot_asked = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
        }
        if self.frame_no > 1500 {
            eprintln!("error: no picture arrived from the window");
            std::process::exit(1);
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.screenshot_step(ctx);
        self.keys(ctx);
        let bar = Color32::from_rgb(0x1f, 0x21, 0x24);
        let panel = ctx.style().visuals.panel_fill;
        egui::TopBottomPanel::top("menu").frame(egui::Frame::none().fill(bar).inner_margin(egui::Margin::symmetric(8.0, 3.0))).show(ctx, |ui| self.menu_bar(ui));
        egui::TopBottomPanel::top("toolbar").frame(egui::Frame::none().fill(panel).inner_margin(egui::Margin::symmetric(8.0, 5.0))).show(ctx, |ui| self.toolbar(ui));
        egui::TopBottomPanel::bottom("status").frame(egui::Frame::none().fill(bar).inner_margin(egui::Margin::symmetric(10.0, 3.0))).show(ctx, |ui| self.status_bar(ui));
        egui::TopBottomPanel::bottom("palette").frame(egui::Frame::none().fill(panel).inner_margin(egui::Margin::symmetric(8.0, 4.0))).show(ctx, |ui| self.palette(ui));
        egui::SidePanel::left("tools").exact_width(46.0).resizable(false).frame(egui::Frame::none().fill(panel).inner_margin(egui::Margin::symmetric(5.0, 4.0))).show(ctx, |ui| self.tools_panel(ui));
        egui::SidePanel::right("side").exact_width(350.0).resizable(false).frame(egui::Frame::none().fill(panel).inner_margin(egui::Margin::same(8.0))).show(ctx, |ui| {
            ui.set_max_width(334.0);
            let half = ui.available_height() * 0.52;
            egui::ScrollArea::vertical().id_salt("cuts").max_height(half).auto_shrink([false, true]).show(ui, |ui| self.cuts_panel(ui));
            ui.separator();
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Laser, if self.dev.router() { "Router" } else { "Laser" });
                ui.selectable_value(&mut self.tab, Tab::Move, "Move");
                ui.selectable_value(&mut self.tab, Tab::Console, "Console");
            });
            ui.separator();
            egui::ScrollArea::vertical().id_salt("tabs").auto_shrink([false, false]).show(ui, |ui| match self.tab {
                Tab::Laser => self.laser_panel(ui),
                Tab::Move => self.move_panel(ui),
                Tab::Console => self.console_panel(ui),
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::none().fill(Color32::from_rgb(0x24, 0x26, 0x2a))).show(ctx, |ui| self.canvas_ui(ui));
        self.preview_bar(ctx);
        self.dialogs(ctx);
        if self.toast.as_ref().map(|t| t.2.elapsed() > Duration::from_secs(6)).unwrap_or(false) {
            self.toast = None;
        }
        self.autosave();
        if self.mach.connected() || self.autosave_due.is_some() || self.toast.is_some() || self.frame_until.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.doc.dirty {
            self.write_autosave();
        }
        self.mach.disconnect();
    }
}
