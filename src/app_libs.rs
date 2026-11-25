//! Router mode in the window: layer settings, the tool library and the speeds & feeds library.

use crate::app::App;
use crate::cam::{op_help, pass_depths, OPS};
use crate::dialogs::Purpose;
use crate::gcode::fmt;
use crate::tools::{FeedEntry, Tool, TOOL_KINDS};
use egui::{Color32, RichText};

const BLUE: Color32 = Color32::from_rgb(0x2f, 0x5f, 0xb3);

impl App {
    /// Router settings for the selected layer
    pub fn router_layer_editor(&mut self, ui: &mut egui::Ui) {
        let Some(li) = self.edit_layer else { return };
        let mut l = self.doc.layers[li].clone();
        let before = l.clone();
        egui::Grid::new("routered").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut l.name).desired_width(150.0));
            ui.end_row();
            ui.label("Operation");
            egui::ComboBox::from_id_salt("op").width(150.0).selected_text(l.op.clone()).show_ui(ui, |ui| {
                for o in OPS {
                    ui.selectable_value(&mut l.op, o.to_string(), o).on_hover_text(op_help(o));
                }
            });
            ui.end_row();
            ui.label("Tool");
            let cur = self.libs.tool(l.tool_id).map(|t| t.title()).unwrap_or_else(|| format!("T{} (not in library)", l.tool_id));
            egui::ComboBox::from_id_salt("tool").width(220.0).selected_text(cur).show_ui(ui, |ui| {
                for t in &self.libs.tools {
                    ui.selectable_value(&mut l.tool_id, t.id, t.title()).on_hover_text(t.summary());
                }
            });
            ui.end_row();
        });
        ui.label(RichText::new(op_help(&l.op)).small().weak());
        // speeds & feeds for this tool
        let tool = self.libs.tool(l.tool_id).cloned();
        ui.horizontal(|ui| {
            ui.label("Material");
            let cur = if l.cnc_material.is_empty() { "— choose to fill in speeds —".to_string() } else { l.cnc_material.clone() };
            let mut chosen: Option<FeedEntry> = None;
            egui::ComboBox::from_id_salt("feedpick").width(200.0).selected_text(cur).show_ui(ui, |ui| {
                let list = self.libs.feeds_for(l.tool_id);
                if list.is_empty() {
                    ui.label(RichText::new("No speeds & feeds for this tool yet").weak());
                }
                for (_, f) in list {
                    let feed = f.feed(tool.as_ref());
                    if ui.selectable_label(false, &f.material).on_hover_text(format!("{} RPM · {} mm/min · {} mm per pass", fmt(f.rpm, 0, false), fmt(feed, 0, false), fmt(f.stepdown, 2, false))).clicked() {
                        chosen = Some(f.clone());
                    }
                }
            });
            if let Some(f) = chosen {
                l.rpm = f.rpm;
                l.feed = f.feed(tool.as_ref()).round();
                l.plunge = f.plunge;
                l.stepdown = f.stepdown;
                l.stepover = f.stepover;
                l.cnc_material = f.material.clone();
                self.toast(format!("Applied {} speeds & feeds to layer {}", f.material, l.name), false);
            }
            if ui.small_button("Save as…").on_hover_text("Keep these speeds in the speeds & feeds library").clicked() {
                let flutes = tool.as_ref().map(|t| t.flutes.max(1)).unwrap_or(2) as f64;
                self.libs.feeds.push(FeedEntry {
                    material: if l.cnc_material.is_empty() { "New material".into() } else { l.cnc_material.trim_end_matches(" (edited)").to_string() },
                    tool_id: l.tool_id,
                    tool_name: tool.as_ref().map(|t| t.name.clone()).unwrap_or_default(),
                    rpm: l.rpm,
                    chipload: ((l.feed / (l.rpm.max(1.0) * flutes)) * 10000.0).round() / 10000.0,
                    plunge: l.plunge,
                    stepdown: l.stepdown,
                    stepover: l.stepover,
                    notes: String::new(),
                });
                self.feed_sel = Some(self.libs.feeds.len() - 1);
                self.feed_tool = None;
                self.feeds_open = true;
                let _ = self.libs.save();
            }
        });
        egui::Grid::new("routernums").num_columns(4).spacing([6.0, 4.0]).show(ui, |ui| {
            ui.label("Cut depth");
            ui.add(egui::DragValue::new(&mut l.depth).speed(0.1).range(0.0..=200.0).suffix(" mm"));
            ui.label("Per pass");
            ui.add(egui::DragValue::new(&mut l.stepdown).speed(0.05).range(0.05..=50.0).suffix(" mm"));
            ui.end_row();
            ui.label("Feed");
            ui.add(egui::DragValue::new(&mut l.feed).speed(10.0).range(1.0..=20000.0).suffix(" mm/min"));
            ui.label("Plunge");
            ui.add(egui::DragValue::new(&mut l.plunge).speed(5.0).range(1.0..=10000.0).suffix(" mm/min"));
            ui.end_row();
            ui.label("Spindle");
            ui.add(egui::DragValue::new(&mut l.rpm).speed(100.0).range(0.0..=60000.0).suffix(" RPM"));
            if l.op == "Pocket" {
                ui.label("Stepover");
                ui.add(egui::DragValue::new(&mut l.stepover).speed(1.0).range(5.0..=100.0).suffix(" %"));
            }
            ui.end_row();
        });
        let n = pass_depths(l.depth, l.stepdown).len();
        let mut info = format!("{n} pass{} of {} mm", if n == 1 { "" } else { "es" }, fmt(if n > 0 { l.depth / n as f64 } else { 0.0 }, 2, false));
        if let Some(t) = &tool {
            let cl = l.feed / (l.rpm.max(1.0) * t.flutes.max(1) as f64);
            info += &format!(" · chip load {} mm/tooth", fmt(cl, 3, false));
            if l.depth > t.flute_length && t.flute_length > 0.0 {
                info += " · deeper than the flutes!";
            }
        }
        ui.label(RichText::new(info).small().weak());
        ui.horizontal_wrapped(|ui| {
            if l.op != "Drill" {
                ui.checkbox(&mut l.climb, "Climb milling").on_hover_text("Off for conventional milling (gentler on light machines)");
            }
            if l.op.starts_with("Profile") {
                ui.checkbox(&mut l.tabs, "Holding tabs").on_hover_text("Leave small bridges so the part doesn't come loose on the last pass");
            }
            if ui.small_button("Reset").on_hover_text("Reset this layer's router settings to defaults").clicked() {
                let d = crate::model::Layer::new(li);
                l.op = d.op;
                l.tool_id = d.tool_id;
                l.depth = d.depth;
                l.stepdown = d.stepdown;
                l.feed = d.feed;
                l.plunge = d.plunge;
                l.rpm = d.rpm;
                l.stepover = d.stepover;
                l.climb = d.climb;
                l.tabs = d.tabs;
                l.cnc_material.clear();
            }
        });
        if l.tabs && l.op.starts_with("Profile") {
            ui.horizontal(|ui| {
                ui.label("Tabs");
                ui.add(egui::DragValue::new(&mut l.tab_count).range(1..=40)).on_hover_text("Tabs per shape");
                ui.label("width");
                ui.add(egui::DragValue::new(&mut l.tab_width).speed(0.1).range(0.5..=50.0).suffix(" mm"));
                ui.label("height");
                ui.add(egui::DragValue::new(&mut l.tab_height).speed(0.1).range(0.2..=50.0).suffix(" mm"));
            });
        }
        if l != before {
            let speeds_changed = (l.rpm, l.feed, l.plunge, l.stepdown, l.stepover) != (before.rpm, before.feed, before.plunge, before.stepdown, before.stepover);
            if speeds_changed && l.cnc_material == before.cnc_material && !before.cnc_material.is_empty() && !before.cnc_material.ends_with(" (edited)") {
                l.cnc_material = format!("{} (edited)", before.cnc_material);
            }
            if l.tool_id != before.tool_id {
                l.cnc_material.clear();
            }
            self.doc.checkpoint();
            self.doc.layers[li] = l;
            self.edited();
        }
    }

    pub fn tools_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut changed = false;
        let mut delete = None;
        let mut use_on_layer = None;
        egui::Window::new("Tool library").open(&mut open).default_size([680.0, 440.0]).resizable(true).show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("➕ New tool").clicked() {
                    let id = self.libs.next_tool_id();
                    self.libs.tools.push(Tool { id, ..Default::default() });
                    self.tool_sel = Some(self.libs.tools.len() - 1);
                    changed = true;
                }
                ui.separator();
                if ui.button("Import JSON…").on_hover_text("Add tools from a JSON file (same tool number is replaced)").clicked() {
                    self.open_file_dlg(Purpose::ImportTools);
                }
                if ui.button("Export JSON…").clicked() {
                    self.open_file_dlg(Purpose::ExportTools);
                }
            });
            ui.separator();
            ui.columns(2, |cols| {
                egui::ScrollArea::vertical().id_salt("toollist").auto_shrink([false, false]).max_height(360.0).show(&mut cols[0], |ui| {
                    for (i, t) in self.libs.tools.iter().enumerate() {
                        let r = ui.selectable_label(self.tool_sel == Some(i), RichText::new(t.title()).strong());
                        ui.label(RichText::new(t.summary()).small().weak());
                        if r.clicked() {
                            self.tool_sel = Some(i);
                        }
                        if r.double_clicked() {
                            use_on_layer = Some(t.id);
                        }
                        ui.add_space(3.0);
                    }
                });
                let ui = &mut cols[1];
                let Some(i) = self.tool_sel.filter(|i| *i < self.libs.tools.len()) else {
                    ui.label(RichText::new("Pick a tool on the left to edit it.\n\nDouble-click a tool to use it on the selected layer.").weak());
                    return;
                };
                let t = &mut self.libs.tools[i];
                let before = t.clone();
                egui::Grid::new("tooledit").num_columns(2).spacing([10.0, 5.0]).show(ui, |ui| {
                    ui.label("Number");
                    ui.add(egui::DragValue::new(&mut t.id).range(1..=999).prefix("T"));
                    ui.end_row();
                    ui.label("Name");
                    ui.text_edit_singleline(&mut t.name);
                    ui.end_row();
                    ui.label("Type");
                    egui::ComboBox::from_id_salt("toolkind").selected_text(t.kind.clone()).show_ui(ui, |ui| {
                        for k in TOOL_KINDS {
                            ui.selectable_value(&mut t.kind, k.to_string(), k);
                        }
                    });
                    ui.end_row();
                    ui.label("Diameter");
                    ui.add(egui::DragValue::new(&mut t.diameter).speed(0.01).range(0.05..=100.0).suffix(" mm"));
                    ui.end_row();
                    ui.label("Flutes");
                    ui.add(egui::DragValue::new(&mut t.flutes).range(1..=12));
                    ui.end_row();
                    ui.label("Flute length");
                    ui.add(egui::DragValue::new(&mut t.flute_length).speed(0.1).range(0.0..=200.0).suffix(" mm"));
                    ui.end_row();
                    if t.kind == "V-bit" {
                        ui.label("Angle");
                        ui.add(egui::DragValue::new(&mut t.angle).speed(0.5).range(10.0..=170.0).suffix("°"));
                        ui.end_row();
                    }
                });
                ui.label("Notes");
                ui.add(egui::TextEdit::multiline(&mut t.notes).desired_rows(2).desired_width(f32::INFINITY));
                if *t != before {
                    changed = true;
                }
                let (id, d_in) = (t.id, t.diameter / 25.4);
                ui.label(RichText::new(format!("{} in", fmt(d_in, 4, false))).small().weak());
                ui.horizontal(|ui| {
                    let target = self.edit_layer.map(|l| self.doc.layers[l].name.clone());
                    if ui.add_enabled(target.is_some(), egui::Button::new(format!("Use on layer {}", target.unwrap_or_default())).fill(BLUE)).clicked() {
                        use_on_layer = Some(id);
                    }
                    if ui.button("🗑 Delete").clicked() {
                        delete = Some(i);
                    }
                });
            });
        });
        if let Some(id) = use_on_layer {
            if let Some(li) = self.edit_layer {
                self.doc.checkpoint();
                self.doc.layers[li].tool_id = id;
                self.doc.layers[li].cnc_material.clear();
                self.edited();
                self.toast(format!("Layer {} now uses T{id}. Pick its material to fill in the speeds.", self.doc.layers[li].name), false);
            }
        }
        if let Some(i) = delete {
            let id = self.libs.tools[i].id;
            let used = self.doc.layers.iter().any(|l| l.tool_id == id) && self.doc.shapes.iter().any(|s| self.doc.layers[s.layer].tool_id == id);
            self.libs.tools.remove(i);
            self.tool_sel = None;
            changed = true;
            if used {
                self.toast(format!("T{id} is still chosen on a layer of this design: pick another tool there."), true);
            }
        }
        if changed {
            let _ = self.libs.save();
        }
        if !open {
            self.tools_open = false;
        }
    }

    pub fn feeds_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut changed = false;
        let mut delete = None;
        let mut apply = None;
        egui::Window::new("Speeds & feeds").open(&mut open).default_size([760.0, 470.0]).resizable(true).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("🔍");
                ui.add(egui::TextEdit::singleline(&mut self.feed_filter).hint_text("Filter by material").desired_width(150.0));
                let label = self.feed_tool.and_then(|id| self.libs.tool(id)).map(|t| t.title()).unwrap_or_else(|| "All tools".into());
                egui::ComboBox::from_id_salt("feedtool").width(190.0).selected_text(label).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.feed_tool, None, "All tools");
                    for t in &self.libs.tools {
                        ui.selectable_value(&mut self.feed_tool, Some(t.id), t.title());
                    }
                });
                if ui.button("➕ New").clicked() {
                    let tid = self.feed_tool.or(self.edit_layer.map(|l| self.doc.layers[l].tool_id)).unwrap_or(1);
                    self.libs.feeds.push(FeedEntry { tool_id: tid, tool_name: self.libs.tool(tid).map(|t| t.name.clone()).unwrap_or_default(), ..Default::default() });
                    self.feed_sel = Some(self.libs.feeds.len() - 1);
                    changed = true;
                }
                ui.separator();
                if ui.button("Import JSON…").on_hover_text("Add entries from a JSON file (same material and tool are replaced)").clicked() {
                    self.open_file_dlg(Purpose::ImportFeeds);
                }
                if ui.button("Export JSON…").clicked() {
                    self.open_file_dlg(Purpose::ExportFeeds);
                }
            });
            ui.separator();
            ui.columns(2, |cols| {
                egui::ScrollArea::vertical().id_salt("feedlist").auto_shrink([false, false]).max_height(380.0).show(&mut cols[0], |ui| {
                    let f = self.feed_filter.to_lowercase();
                    for (i, e) in self.libs.feeds.iter().enumerate() {
                        if (!f.is_empty() && !e.material.to_lowercase().contains(&f)) || self.feed_tool.map(|t| t != e.tool_id).unwrap_or(false) {
                            continue;
                        }
                        let tool = self.libs.tool(e.tool_id);
                        let tname = tool.map(|t| t.title()).unwrap_or_else(|| format!("T{} (missing)", e.tool_id));
                        let r = ui.selectable_label(self.feed_sel == Some(i), RichText::new(format!("{} — {}", e.material, tname)).strong());
                        ui.label(RichText::new(format!("{} RPM · {} mm/min · {} mm per pass · {}% stepover", fmt(e.rpm, 0, false), fmt(e.feed(tool), 0, false), fmt(e.stepdown, 2, false), fmt(e.stepover, 0, false))).small().weak());
                        if r.clicked() {
                            self.feed_sel = Some(i);
                        }
                        if r.double_clicked() {
                            apply = Some(i);
                        }
                        ui.add_space(3.0);
                    }
                });
                let ui = &mut cols[1];
                let Some(i) = self.feed_sel.filter(|i| *i < self.libs.feeds.len()) else {
                    ui.label(RichText::new("Pick an entry on the left to edit it.\n\nFeed rate = spindle RPM × flutes × chip load.\nDouble-click an entry to apply it to the selected layer.").weak());
                    return;
                };
                let tools: Vec<(u32, String)> = self.libs.tools.iter().map(|t| (t.id, t.title())).collect();
                let tool = self.libs.tool(self.libs.feeds[i].tool_id).cloned();
                let e = &mut self.libs.feeds[i];
                let before = e.clone();
                let mut feed = e.feed(tool.as_ref());
                egui::Grid::new("feededit").num_columns(2).spacing([10.0, 5.0]).show(ui, |ui| {
                    ui.label("Material");
                    ui.text_edit_singleline(&mut e.material);
                    ui.end_row();
                    ui.label("Tool");
                    let cur = tools.iter().find(|t| t.0 == e.tool_id).map(|t| t.1.clone()).unwrap_or_else(|| format!("T{} (missing)", e.tool_id));
                    egui::ComboBox::from_id_salt("feededittool").width(200.0).selected_text(cur).show_ui(ui, |ui| {
                        for (id, t) in &tools {
                            ui.selectable_value(&mut e.tool_id, *id, t);
                        }
                    });
                    ui.end_row();
                    ui.label("Spindle");
                    ui.add(egui::DragValue::new(&mut e.rpm).speed(100.0).range(100.0..=60000.0).suffix(" RPM"));
                    ui.end_row();
                    ui.label("Chip load");
                    ui.add(egui::DragValue::new(&mut e.chipload).speed(0.001).range(0.0005..=2.0).max_decimals(4).suffix(" mm/tooth"));
                    ui.end_row();
                    ui.label("Feed");
                    let r = ui.add(egui::DragValue::new(&mut feed).speed(10.0).range(1.0..=30000.0).max_decimals(0).suffix(" mm/min"));
                    if r.changed() {
                        // typing a feed works the chip load out for you
                        let flutes = tool.as_ref().map(|t| t.flutes.max(1)).unwrap_or(2) as f64;
                        e.chipload = feed / (e.rpm.max(1.0) * flutes);
                    }
                    ui.end_row();
                    ui.label("Plunge");
                    ui.add(egui::DragValue::new(&mut e.plunge).speed(5.0).range(1.0..=10000.0).suffix(" mm/min"));
                    ui.end_row();
                    ui.label("Depth per pass");
                    ui.add(egui::DragValue::new(&mut e.stepdown).speed(0.05).range(0.05..=50.0).suffix(" mm"));
                    ui.end_row();
                    ui.label("Stepover");
                    ui.add(egui::DragValue::new(&mut e.stepover).speed(1.0).range(5.0..=100.0).suffix(" %"));
                    ui.end_row();
                });
                ui.label("Notes");
                ui.add(egui::TextEdit::multiline(&mut e.notes).desired_rows(2).desired_width(f32::INFINITY));
                if *e != before {
                    changed = true;
                }
                ui.horizontal(|ui| {
                    let target = self.edit_layer.map(|l| self.doc.layers[l].name.clone());
                    if ui.add_enabled(target.is_some(), egui::Button::new(format!("Apply to layer {}", target.unwrap_or_default())).fill(BLUE)).clicked() {
                        apply = Some(i);
                    }
                    if ui.button("🗑 Delete").clicked() {
                        delete = Some(i);
                    }
                });
            });
        });
        if let Some(i) = apply {
            if let Some(li) = self.edit_layer {
                let e = self.libs.feeds[i].clone();
                let tool = self.libs.tool(e.tool_id).cloned();
                self.doc.checkpoint();
                let l = &mut self.doc.layers[li];
                l.tool_id = e.tool_id;
                l.rpm = e.rpm;
                l.feed = e.feed(tool.as_ref()).round();
                l.plunge = e.plunge;
                l.stepdown = e.stepdown;
                l.stepover = e.stepover;
                l.cnc_material = e.material.clone();
                let name = l.name.clone();
                self.edited();
                self.toast(format!("Applied {} with T{} to layer {name}", e.material, e.tool_id), false);
            } else {
                self.toast("Draw something first: speeds apply to a layer.", true);
            }
        }
        if let Some(i) = delete {
            self.libs.feeds.remove(i);
            self.feed_sel = None;
            changed = true;
        }
        if changed {
            for e in self.libs.feeds.iter_mut() {
                if let Some(t) = self.libs.tools.iter().find(|t| t.id == e.tool_id) {
                    e.tool_name = t.name.clone();
                }
            }
            let _ = self.libs.save();
        }
        if !open {
            self.feeds_open = false;
        }
    }
}
