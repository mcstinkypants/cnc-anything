//! Menu commands and keyboard shortcuts.

use crate::app::{App, Tab};
use crate::canvas::Tool;
use crate::dialogs::Purpose;
use crate::gcode::fmt;
use crate::geom::*;
use crate::machine::Cmd;
use crate::model::Shape;

impl App {
    pub fn zoom_fit(&mut self) {
        self.view.zoom_fit(&self.dev);
    }

    pub fn cmd(&mut self, c: &str) {
        let b = self.doc.sel_box();
        match c {
            "new" => {
                if self.doc.dirty && !self.doc.shapes.is_empty() {
                    self.confirm = Some(("Start a new project? Unsaved changes will be lost.".into(), "newConfirmed"));
                } else {
                    self.cmd("newConfirmed");
                }
            }
            "newConfirmed" => {
                self.doc.clear();
                self.view.forget_cache();
                self.doc_name = "Untitled".into();
                self.doc_path = None;
                self.view.preview = None;
                self.edited();
            }
            "open" => self.open_file_dlg(Purpose::OpenProject),
            "save" => {
                if self.doc_path.as_ref().map(|p| p.extension().map(|e| e == "cnca").unwrap_or(false)).unwrap_or(false) {
                    let p = self.doc_path.clone().unwrap();
                    match std::fs::write(&p, serde_json::to_string(&self.doc.to_file()).unwrap_or_default()) {
                        Ok(()) => {
                            self.doc.dirty = false;
                            self.toast(format!("Saved {}", p.display()), false);
                        }
                        Err(e) => self.toast(format!("Could not save: {e}"), true),
                    }
                } else {
                    self.open_file_dlg(Purpose::SaveProject);
                }
            }
            "saveAs" => self.open_file_dlg(Purpose::SaveProject),
            "import" => self.open_file_dlg(Purpose::Import),
            "exportGcode" => self.open_file_dlg(Purpose::ExportGcode),
            "exportSvg" => self.open_file_dlg(Purpose::ExportSvg),
            "importMaterials" => self.open_file_dlg(Purpose::ImportMaterials),
            "exportMaterials" => self.open_file_dlg(Purpose::ExportMaterials),
            "sample" => {
                self.load_sample();
                self.edited();
            }
            "undo" => {
                if self.doc.undo() {
                    self.edited();
                }
            }
            "redo" => {
                if self.doc.redo() {
                    self.edited();
                }
            }
            "copy" => self.clip = self.doc.selected().into_iter().cloned().collect(),
            "cut" => {
                self.clip = self.doc.selected().into_iter().cloned().collect();
                if !self.clip.is_empty() {
                    self.doc.checkpoint();
                    self.doc.delete_sel();
                    self.edited();
                }
            }
            "paste" | "duplicate" => {
                let off = if c == "paste" { 10.0 } else { 0.0 };
                let list: Vec<Shape> = if c == "paste" { self.clip.clone() } else { self.doc.selected().into_iter().cloned().collect() };
                if list.is_empty() {
                    return;
                }
                self.doc.checkpoint();
                self.doc.sel.clear();
                // paste lands down-right on screen whichever corner the origin is in
                let m = translate(off, -off);
                for mut s in list {
                    s.id = 0;
                    s.tf = mul(&m, &s.tf);
                    let id = self.doc.add(s);
                    self.doc.sel.insert(id);
                }
                if c == "paste" {
                    for s in self.clip.iter_mut() {
                        s.tf = mul(&m, &s.tf);
                    }
                }
                self.edited();
            }
            "delete" => {
                if !self.doc.sel.is_empty() {
                    self.doc.checkpoint();
                    self.doc.delete_sel();
                    self.edited();
                }
            }
            "selectAll" => self.doc.sel = self.doc.shapes.iter().filter(|s| self.doc.layers[s.layer].show).map(|s| s.id).collect(),
            "alignL" | "alignR" | "alignCH" | "alignT" | "alignB" | "alignCV" => {
                let Some(b) = b.filter(|_| self.doc.sel.len() >= 2) else {
                    return self.toast("Select two or more shapes to align.", false);
                };
                self.doc.checkpoint();
                let ids: Vec<u64> = self.doc.sel.iter().copied().collect();
                for id in ids {
                    let Some(sb) = self.shape_box(id) else { continue };
                    let m = match c {
                        "alignL" => translate(b.x0 - sb.x0, 0.0),
                        "alignR" => translate(b.x1 - sb.x1, 0.0),
                        "alignCH" => translate(b.cx() - sb.cx(), 0.0),
                        "alignT" => translate(0.0, b.y1 - sb.y1),
                        "alignB" => translate(0.0, b.y0 - sb.y0),
                        _ => translate(0.0, b.cy() - sb.cy()),
                    };
                    if let Some(s) = self.doc.shapes.iter_mut().find(|s| s.id == id) {
                        s.tf = mul(&m, &s.tf);
                    }
                }
                self.edited();
            }
            "mirrorH" | "mirrorV" | "rot90" | "rot-90" | "centerBed" => {
                let Some(b) = b else { return };
                self.doc.checkpoint();
                let m = match c {
                    "mirrorH" => scale_about(-1.0, 1.0, b.cx(), 0.0),
                    "mirrorV" => scale_about(1.0, -1.0, 0.0, b.cy()),
                    "rot90" => rotate_about(-90.0, b.cx(), b.cy()),
                    "rot-90" => rotate_about(90.0, b.cx(), b.cy()),
                    _ => translate(self.dev.bed_w / 2.0 - b.cx(), self.dev.bed_h / 2.0 - b.cy()),
                };
                self.doc.transform_sel(&m);
                self.edited();
            }
            "toPath" => self.convert_to_curves(),
            "zoomFit" => self.zoom_fit(),
            "zoomSel" => {
                if let Some(b) = b.or(self.doc.all_box()) {
                    self.view.zoom_to(b, 60.0);
                }
            }
            "preview" => self.open_preview(),
            "closePreview" => self.view.preview = None,
            "settings" => self.open_profiles(self.profiles.active),
            "materials" => self.mat_open = true,
            "tools" => self.tools_open = true,
            "feeds" => self.feeds_open = true,
            "importTools" => self.open_file_dlg(Purpose::ImportTools),
            "exportTools" => self.open_file_dlg(Purpose::ExportTools),
            "importFeeds" => self.open_file_dlg(Purpose::ImportFeeds),
            "exportFeeds" => self.open_file_dlg(Purpose::ExportFeeds),
            "machineLaser" | "machineRouter" => {
                self.dev.machine = if c == "machineRouter" { "router".into() } else { "laser".into() };
                self.view.preview = None;
                self.save_prefs();
                self.toast(if c == "machineRouter" { "Router mode: layers now cut with tools in depth passes." } else { "Laser mode." }, false);
            }
            "setZero" => {
                if self.need() {
                    self.mach.send("G10 L20 P1 Z0");
                    self.toast("Z zero set: the bit's tip is now the top of the stock.", false);
                }
            }
            "help" => {
                self.info = Some((
                    "Getting started".into(),
                    "1. Design: draw with the tools on the left, or import an SVG drawing or a picture (File → Import).\n\n\
                     2. Layers: click a color in the bar at the bottom to put the selected shapes on that layer. Each layer has its own speed, power, passes and mode (Line follows outlines, Fill scans the inside, Image engraves pictures).\n\n\
                     3. Materials: open the Material library to keep settings for each material and apply them to a layer in one click. Libraries can be exported and imported as JSON files to share them.\n\n\
                     4. Check: press Preview to see every move the laser will make and how long it will take.\n\n\
                     Router: switch the toolbar to Router. Each layer then has an operation (profile outside or inside, on the line, pocket, drill), a tool from the Tool library, a cut depth and depth per pass, and speeds you can fill in from the Speeds & feeds library. Set Z zero on top of the stock before starting; the job pauses when it needs a different tool.\n\n\
                     5. Connect: plug in a GRBL laser or router over USB, pick its port in the Laser tab and press Connect. Try the practice machine first to see how it works without a laser.\n\n\
                     6. Burn: Home, then Frame to trace the job outline, then Start. Pause holds the head; Stop halts and turns the laser off.\n\n\
                     Always stay with a running laser and wear the right eye protection."
                        .into(),
                ))
            }
            "keys" => {
                self.info = Some((
                    "Keyboard shortcuts".into(),
                    "V / N / L / S / R / E / P / T / M / H — Select, Edit points, Lines, Spline, Rectangle, Ellipse, Polygon, Text, Measure, Pan\n\
                     Spline — click for sharp points, drag for curved ones (Shift+drag for a corner), click the first point to close\n\
                     Edit points — drag points and handles, double-click a segment to add a point, double-click a point for sharp/smooth, Delete removes it\n\
                     Space + drag or middle mouse — pan      Mouse wheel — zoom\n\
                     Shift while drawing — square / circle / 15° angles\n\
                     Shift on a corner handle — stretch freely\n\
                     Alt while dragging — turn snapping off for a moment\n\
                     Arrow keys — nudge 1 mm (Shift 10 mm, Ctrl 0.1 mm)\n\
                     Ctrl+Z / Ctrl+Y — undo / redo      Ctrl+C / V / X / D — copy, paste, cut, duplicate\n\
                     Delete — delete selection      Alt+P — preview\n\
                     Enter, double-click or right-click — finish a line      Esc — cancel, deselect, close preview"
                        .into(),
                ))
            }
            "connectSim" => {
                self.mach.connect_sim([self.dev.bed_w, self.dev.bed_h], self.dev.s_max);
                if self.dev.auto_home {
                    self.mach.send("$H");
                }
            }
            "connectSerial" => {
                let port = self.dev.port.clone();
                if port.is_empty() {
                    return self.toast("No serial port found. Plug the laser in, then press ⟳ to look again.", true);
                }
                match self.mach.connect_serial(&port, self.dev.baud, self.dev.buffer_size) {
                    Ok(()) => {
                        self.save_prefs();
                        if self.dev.auto_home {
                            self.mach.send("$H");
                        }
                    }
                    Err(e) => self.toast(format!("Could not open {port}: {e}"), true),
                }
            }
            "disconnect" => self.mach.disconnect(),
            "start" => self.start_job(),
            "pause" => {
                let (j, state) = {
                    let s = self.mach.st.lock().unwrap();
                    (s.job.as_ref().map(|j| j.paused), s.state.clone())
                };
                match j {
                    Some(true) => self.mach.cmd(Cmd::Resume),
                    // a job paused itself (tool change): carry on
                    Some(false) if state.starts_with("Hold") => self.mach.cmd(Cmd::Realtime(b'~')),
                    Some(false) => self.mach.cmd(Cmd::Pause),
                    None if state.starts_with("Hold") => self.mach.cmd(Cmd::Realtime(b'~')),
                    None => self.mach.cmd(Cmd::Realtime(b'!')),
                }
            }
            "stop" => self.mach.cmd(Cmd::Stop),
            "home" => {
                if self.need() {
                    self.mach.send("$H")
                }
            }
            "unlock" => {
                if self.need() {
                    self.mach.send("$X")
                }
            }
            "frame" => self.frame(),
            "setOrigin" => {
                if self.need() {
                    self.mach.send("G10 L20 P1 X0 Y0");
                    self.toast("Origin set at the current head position.", false);
                }
            }
            "gotoOrigin" => {
                if self.need() {
                    self.mach.send("G90");
                    self.mach.send("G0 X0 Y0");
                }
            }
            "gotoSel" => {
                if self.need() {
                    if let Some(b) = b.or(self.doc.all_box()) {
                        let u = self.dev.to_user([b.cx(), b.cy()]);
                        self.mach.send("G90");
                        self.mach.send(&format!("G0 X{} Y{}", fmt(u[0], 3, false), fmt(u[1], 3, false)));
                    }
                }
            }
            "pulse" => {
                if self.need() {
                    self.confirm = Some(("Fire the laser for a short moment at 1% power? Make sure the area is safe and you are wearing eye protection.".into(), "pulseConfirmed"));
                }
            }
            "pulseConfirmed" => {
                let s = (self.dev.s_max * 0.01).round().max(1.0);
                for l in ["M3".to_string(), format!("G1 F100 S{s}"), "G4 P0.2".into(), "G1 S0".into(), "M5".into()] {
                    self.mach.send(&l);
                }
            }
            _ => {}
        }
    }

    pub fn keys(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() || self.file_dlg.is_some() || self.text_dlg.is_some() || self.settings.is_some() || self.confirm.is_some() {
            return;
        }
        use egui::Key;
        let (m, presses, copy, cut, paste) = ctx.input(|i| {
            let presses: Vec<Key> = i.events.iter().filter_map(|e| if let egui::Event::Key { key, pressed: true, .. } = e { Some(*key) } else { None }).collect();
            (
                i.modifiers,
                presses,
                i.events.iter().any(|e| matches!(e, egui::Event::Copy)),
                i.events.iter().any(|e| matches!(e, egui::Event::Cut)),
                i.events.iter().any(|e| matches!(e, egui::Event::Paste(_))),
            )
        });
        self.view.space = ctx.input(|i| i.key_down(Key::Space));
        if copy {
            self.cmd("copy");
        }
        if cut {
            self.cmd("cut");
        }
        if paste {
            self.cmd("paste");
        }
        for k in presses {
            if m.command {
                match k {
                    Key::Z => self.cmd(if m.shift { "redo" } else { "undo" }),
                    Key::Y => self.cmd("redo"),
                    Key::C if !copy => self.cmd("copy"),
                    Key::X if !cut => self.cmd("cut"),
                    Key::V if !paste => self.cmd("paste"),
                    Key::D => self.cmd("duplicate"),
                    Key::A => self.cmd("selectAll"),
                    Key::S => self.cmd("save"),
                    Key::O => self.cmd("open"),
                    Key::N => self.cmd("new"),
                    Key::I => self.cmd("import"),
                    Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => self.arrow(k, 0.1),
                    _ => {}
                }
                continue;
            }
            if m.alt {
                if k == Key::P {
                    self.cmd("preview");
                }
                continue;
            }
            match k {
                Key::Delete | Key::Backspace => {
                    if self.view.tool == Tool::Nodes && self.view.node_sel.is_some() {
                        self.delete_node();
                    } else {
                        self.cmd("delete");
                    }
                }
                Key::Escape => {
                    if self.view.preview.is_some() {
                        self.view.preview = None;
                    } else if self.view.spline_new.is_some() {
                        self.view.spline_new = None;
                        self.view.node_drag = None;
                    } else if self.view.node_sel.is_some() {
                        self.view.node_sel = None;
                    } else if self.view.line_pts.is_some() {
                        self.view.line_pts = None;
                    } else {
                        self.doc.sel.clear();
                        self.set_tool(Tool::Select);
                    }
                }
                Key::Enter => {
                    if self.view.spline_new.is_some() {
                        self.finish_open_spline();
                    }
                    if self.view.line_pts.is_some() {
                        let mut ev = vec![];
                        self.view.finish_line(false, self.active_layer, &mut ev);
                        self.handle_events(ev);
                    }
                }
                Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => self.arrow(k, if m.shift { 10.0 } else { 1.0 }),
                Key::V => self.set_tool(Tool::Select),
                Key::L => self.set_tool(Tool::Line),
                Key::R => self.set_tool(Tool::Rect),
                Key::E => self.set_tool(Tool::Ellipse),
                Key::P => self.set_tool(Tool::Polygon),
                Key::T => self.set_tool(Tool::Text),
                Key::M => self.set_tool(Tool::Measure),
                Key::H => self.set_tool(Tool::Pan),
                Key::S => self.set_tool(Tool::Spline),
                Key::N => self.set_tool(Tool::Nodes),
                _ => {}
            }
        }
    }

    fn arrow(&mut self, k: egui::Key, d: f64) {
        use egui::Key;
        let (dx, dy) = match k {
            Key::ArrowLeft => (-1.0, 0.0),
            Key::ArrowRight => (1.0, 0.0),
            Key::ArrowUp => (0.0, 1.0),
            _ => (0.0, -1.0),
        };
        if !self.doc.sel.is_empty() {
            self.doc.checkpoint();
            self.doc.transform_sel(&translate(dx * d, dy * d));
            self.edited();
        } else if self.tab == Tab::Move && self.mach.connected() {
            self.jog(dx, dy);
        }
    }
}
