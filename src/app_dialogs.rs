//! Dialog windows: text, device settings, file browser, confirmations and the material library.

use crate::app::{App, TextDlg};
use crate::dialogs::Purpose;
use crate::geom::*;
use crate::importers::{self, TextSpec};
use crate::materials::{CutProfile, Material};
use crate::model::Shape;
use egui::{Color32, RichText};

const BLUE: Color32 = Color32::from_rgb(0x2f, 0x5f, 0xb3);

impl App {
    pub fn open_text_dlg(&mut self, target: Option<u64>, at: P) {
        let d = match target.and_then(|id| self.doc.shapes.iter().find(|s| s.id == id)) {
            Some(s) => TextDlg {
                target,
                at,
                text: s.text.clone(),
                font: if s.font.is_empty() { "Sans".into() } else { s.font.clone() },
                size: s.size,
                align: if s.align.is_empty() { "left".into() } else { s.align.clone() },
                bold: s.bold,
                italic: s.italic,
            },
            None => TextDlg { target: None, at, text: "Hello".into(), font: "Sans".into(), size: 12.0, align: "left".into(), bold: false, italic: false },
        };
        self.text_dlg = Some(d);
    }

    pub fn dialogs(&mut self, ctx: &egui::Context) {
        // file browser
        if let Some(fd) = &mut self.file_dlg {
            if let Some(p) = fd.show(ctx) {
                let purpose = fd.purpose;
                self.file_dlg = None;
                if !p.as_os_str().is_empty() {
                    self.file_chosen(purpose, p);
                }
            }
        }
        self.text_window(ctx);
        self.profiles_window(ctx);
        if let Some((t, body)) = self.info.clone() {
            let mut open = true;
            egui::Window::new(t).collapsible(false).open(&mut open).default_width(560.0).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label(body);
            });
            if !open {
                self.info = None;
            }
        }
        if let Some((q, then)) = self.confirm.clone() {
            let mut ans = None;
            egui::Window::new("Please confirm").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label(q);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        ans = Some(false);
                    }
                    if ui.add(egui::Button::new("OK").fill(BLUE)).clicked() {
                        ans = Some(true);
                    }
                });
            });
            if let Some(a) = ans {
                self.confirm = None;
                if a {
                    self.cmd(then);
                }
            }
        }
        if self.mat_open {
            self.materials_window(ctx);
        }
        if self.tools_open {
            self.tools_window(ctx);
        }
        if self.feeds_open {
            self.feeds_window(ctx);
        }
    }

    fn text_window(&mut self, ctx: &egui::Context) {
        let (mut close, mut apply) = (false, false);
        let Some(d) = &mut self.text_dlg else { return };
        if self.sys_fonts.is_none() {
            self.sys_fonts = Some(importers::system_fonts().into_iter().map(|f| f.0).collect());
        }
        let fonts = self.sys_fonts.clone().unwrap_or_default();
        egui::Window::new("Text").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.add(egui::TextEdit::multiline(&mut d.text).desired_rows(3).desired_width(400.0));
            egui::Grid::new("textopts").num_columns(2).show(ui, |ui| {
                ui.label("Font");
                egui::ComboBox::from_id_salt("font").width(260.0).selected_text(d.font.clone()).show_ui(ui, |ui| {
                    for f in importers::BUILTIN_FONTS {
                        ui.selectable_value(&mut d.font, f.to_string(), format!("{f} (built in)"));
                    }
                    ui.separator();
                    for f in &fonts {
                        ui.selectable_value(&mut d.font, f.clone(), f);
                    }
                });
                ui.end_row();
                ui.label("Height");
                ui.add(egui::DragValue::new(&mut d.size).speed(0.2).range(1.0..=500.0).suffix(" mm"));
                ui.end_row();
                ui.label("Align");
                ui.horizontal(|ui| {
                    for a in ["left", "center", "right"] {
                        ui.selectable_value(&mut d.align, a.to_string(), a);
                    }
                });
                ui.end_row();
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut d.bold, "Bold");
                ui.checkbox(&mut d.italic, "Italic");
            });
            ui.horizontal(|ui| {
                close = ui.button("Cancel").clicked();
                apply = ui.add(egui::Button::new("OK").fill(BLUE)).clicked();
            });
        });
        if close {
            self.text_dlg = None;
        }
        if !apply {
            return;
        }
        let d = self.text_dlg.take().unwrap();
        if d.text.trim().is_empty() {
            return;
        }
        let (cs, w, h) = importers::text_outlines(&TextSpec { text: &d.text, font: &d.font, size: d.size, align: &d.align, bold: d.bold, italic: d.italic });
        if cs.is_empty() {
            return self.toast("That text produced no outline (does the font have these letters?).", true);
        }
        let fill = |s: &mut Shape| {
            s.text = d.text.clone();
            s.font = d.font.clone();
            s.size = d.size;
            s.align = d.align.clone();
            s.bold = d.bold;
            s.italic = d.italic;
            s.subpaths = cs.clone();
            s.w = w;
            s.h = h;
        };
        if let Some(id) = d.target {
            self.doc.checkpoint();
            if let Some(s) = self.doc.shapes.iter_mut().find(|s| s.id == id) {
                fill(s);
            }
            self.edited();
        } else {
            let mut s = Shape::new("text");
            fill(&mut s);
            s.layer = usize::MAX;
            s.tf = translate(d.at[0], d.at[1]);
            self.add_shape(s);
        }
    }

    fn materials_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut changed = false;
        let mut apply: Option<usize> = None;
        let mut delete: Option<usize> = None;
        let mut want_import = false;
        let mut want_export = false;
        let target_name = self.edit_layer.map(|l| self.doc.layers[l].name.clone());
        egui::Window::new("Material library").open(&mut open).default_size([720.0, 470.0]).resizable(true).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("🔍");
                ui.add(egui::TextEdit::singleline(&mut self.mat_filter).hint_text("Filter by name").desired_width(170.0));
                if ui.button("➕ New").clicked() {
                    self.lib.materials.push(Material::default());
                    self.mat_sel = Some(self.lib.materials.len() - 1);
                    changed = true;
                }
                if let Some(li) = self.edit_layer {
                    if ui.button(format!("➕ From layer {}", self.doc.layers[li].name)).on_hover_text("Make a new material from the selected layer's settings").clicked() {
                        let l = &self.doc.layers[li];
                        let op = match l.mode.as_str() {
                            "Line" => "Cut",
                            "Fill" => "Engrave",
                            _ => "Picture",
                        };
                        self.lib.materials.push(Material { name: "New material".into(), operation: op.into(), profile: CutProfile::from_layer(l), ..Default::default() });
                        self.mat_sel = Some(self.lib.materials.len() - 1);
                        changed = true;
                    }
                }
                ui.separator();
                want_import = ui.button("Import JSON…").on_hover_text("Add materials from a JSON file (same name, thickness and operation are replaced)").clicked();
                want_export = ui.button("Export JSON…").on_hover_text("Save the whole library as a JSON file").clicked();
            });
            ui.separator();
            ui.columns(2, |cols| {
                egui::ScrollArea::vertical().id_salt("matlist").auto_shrink([false, false]).max_height(380.0).show(&mut cols[0], |ui| {
                    let f = self.mat_filter.to_lowercase();
                    for (i, m) in self.lib.materials.iter().enumerate() {
                        if !f.is_empty() && !m.title().to_lowercase().contains(&f) {
                            continue;
                        }
                        let r = ui.selectable_label(self.mat_sel == Some(i), RichText::new(m.title()).strong());
                        ui.label(RichText::new(m.profile.summary()).small().weak());
                        if r.clicked() {
                            self.mat_sel = Some(i);
                        }
                        if r.double_clicked() {
                            apply = Some(i);
                        }
                        ui.add_space(3.0);
                    }
                    if self.lib.materials.is_empty() {
                        ui.label(RichText::new("No materials yet. Press New, or save a layer's settings.").weak());
                    }
                });
                let ui = &mut cols[1];
                let Some(i) = self.mat_sel.filter(|i| *i < self.lib.materials.len()) else {
                    ui.label(RichText::new("Pick a material on the left to see and edit its settings.\n\nDouble-click a material to apply it to the selected layer.").weak());
                    return;
                };
                let m = &mut self.lib.materials[i];
                let before = m.clone();
                egui::Grid::new("matedit").num_columns(2).spacing([10.0, 5.0]).show(ui, |ui| {
                    ui.label("Material");
                    ui.text_edit_singleline(&mut m.name);
                    ui.end_row();
                    ui.label("Thickness");
                    ui.add(egui::DragValue::new(&mut m.thickness).speed(0.1).range(0.0..=200.0).suffix(" mm"));
                    ui.end_row();
                    ui.label("Operation");
                    ui.text_edit_singleline(&mut m.operation);
                    ui.end_row();
                    let p = &mut m.profile;
                    ui.label("Mode");
                    egui::ComboBox::from_id_salt("matmode").selected_text(p.mode.clone()).show_ui(ui, |ui| {
                        for md in ["Line", "Fill", "Image"] {
                            ui.selectable_value(&mut p.mode, md.to_string(), md);
                        }
                    });
                    ui.end_row();
                    ui.label("Speed");
                    ui.add(egui::DragValue::new(&mut p.speed).speed(1.0).range(0.1..=2000.0).suffix(" mm/s"));
                    ui.end_row();
                    ui.label("Power");
                    ui.add(egui::DragValue::new(&mut p.power).speed(0.5).range(0.0..=100.0).suffix(" %"));
                    ui.end_row();
                    ui.label("Passes");
                    ui.add(egui::DragValue::new(&mut p.passes).range(1..=100));
                    ui.end_row();
                    if p.mode != "Line" {
                        ui.label("Interval");
                        ui.add(egui::DragValue::new(&mut p.interval).speed(0.01).range(0.01..=5.0).suffix(" mm"));
                        ui.end_row();
                        ui.label("Overscan");
                        ui.add(egui::DragValue::new(&mut p.overscan).speed(0.5).range(0.0..=50.0).suffix(" %"));
                        ui.end_row();
                        ui.label("Both directions");
                        ui.checkbox(&mut p.bidir, "");
                        ui.end_row();
                    }
                    ui.label("Air assist");
                    ui.checkbox(&mut p.air, "");
                    ui.end_row();
                });
                ui.label("Notes");
                ui.add(egui::TextEdit::multiline(&mut m.notes).desired_rows(2).desired_width(f32::INFINITY));
                if *m != before {
                    changed = true;
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let label = format!("Apply to layer {}", target_name.clone().unwrap_or_default());
                    if ui.add_enabled(target_name.is_some(), egui::Button::new(label).fill(BLUE)).clicked() {
                        apply = Some(i);
                    }
                    if let Some(li) = self.edit_layer {
                        if ui.button("Update from layer").on_hover_text("Replace these settings with the selected layer's").clicked() {
                            self.lib.materials[i].profile = CutProfile::from_layer(&self.doc.layers[li]);
                            changed = true;
                        }
                    }
                    if ui.button("🗑 Delete").clicked() {
                        delete = Some(i);
                    }
                });
            });
        });
        if let Some(i) = apply {
            if let Some(li) = self.edit_layer {
                let m = self.lib.materials[i].clone();
                self.doc.checkpoint();
                let l = &mut self.doc.layers[li];
                m.profile.apply_to(l);
                l.material = m.title();
                let name = l.name.clone();
                self.edited();
                self.toast(format!("Applied \"{}\" to layer {name}", m.title()), false);
            } else {
                self.toast("Draw something first: materials apply to a layer.", true);
            }
        }
        if let Some(i) = delete {
            self.lib.materials.remove(i);
            self.mat_sel = None;
            changed = true;
        }
        if changed {
            let _ = self.lib.save();
        }
        if want_import {
            self.open_file_dlg(Purpose::ImportMaterials);
        }
        if want_export {
            self.open_file_dlg(Purpose::ExportMaterials);
        }
        if !open {
            self.mat_open = false;
        }
    }
}
