//! Machine profiles window: create, edit, save, switch, import and export machines, connected or not.

use crate::app::App;
use crate::dialogs::Purpose;
use crate::gcode::Device;
use egui::{Color32, RichText};

const BLUE: Color32 = Color32::from_rgb(0x2f, 0x5f, 0xb3);

/// All of a machine's settings
pub fn device_editor(ui: &mut egui::Ui, d: &mut Device, ports: &[String]) {
    egui::Grid::new("dev").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
        ui.label("Name");
        ui.text_edit_singleline(&mut d.name);
        ui.end_row();
        ui.label("Machine");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut d.machine, "laser".to_string(), "Laser cutter");
            ui.selectable_value(&mut d.machine, "router".to_string(), "CNC router");
        });
        ui.end_row();
        ui.label("Bed size");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut d.bed_w).range(10.0..=5000.0).suffix(" mm"));
            ui.label("×");
            ui.add(egui::DragValue::new(&mut d.bed_h).range(10.0..=5000.0).suffix(" mm"));
        });
        ui.end_row();
        ui.label("Serial port");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut d.port).hint_text("e.g. /dev/ttyUSB0").desired_width(150.0));
            egui::ComboBox::from_id_salt("devport").width(24.0).selected_text("").show_ui(ui, |ui| {
                if ports.is_empty() {
                    ui.label(RichText::new("No ports found right now").weak());
                }
                for p in ports {
                    ui.selectable_value(&mut d.port, p.clone(), p);
                }
            });
        });
        ui.end_row();
        ui.label("Baud rate");
        egui::ComboBox::from_id_salt("baud").selected_text(d.baud.to_string()).show_ui(ui, |ui| {
            for b in [9600u32, 57600, 115200, 230400, 250000, 500000] {
                ui.selectable_value(&mut d.baud, b, b.to_string());
            }
        });
        ui.end_row();
        if d.router() {
            ui.label("S-value max (max spindle RPM, $30)");
        } else {
            ui.label("S-value max (match GRBL $30)");
        }
        ui.add(egui::DragValue::new(&mut d.s_max).range(1.0..=100000.0));
        ui.end_row();
        if !d.router() {
            ui.label("Laser controller");
            let cur = if d.kind == "grbl-m3" { "GRBL 1.1e or older (constant power, M3)" } else { "GRBL 1.1f or newer (variable power, M4)" };
            egui::ComboBox::from_id_salt("ctl").width(300.0).selected_text(cur).show_ui(ui, |ui| {
                ui.selectable_value(&mut d.kind, "grbl".to_string(), "GRBL 1.1f or newer (variable power, M4)");
                ui.selectable_value(&mut d.kind, "grbl-m3".to_string(), "GRBL 1.1e or older (constant power, M3)");
            });
            ui.end_row();
        } else {
            ui.label("Safe height (above stock)");
            ui.add(egui::DragValue::new(&mut d.safe_z).speed(0.1).range(0.5..=100.0).suffix(" mm"));
            ui.end_row();
            ui.label("Stock thickness");
            ui.add(egui::DragValue::new(&mut d.stock).speed(0.1).range(0.0..=500.0).suffix(" mm"));
            ui.end_row();
            ui.label("Spindle start-up wait");
            ui.add(egui::DragValue::new(&mut d.spindle_delay).speed(0.1).range(0.0..=30.0).suffix(" s"));
            ui.end_row();
        }
        ui.label("Controller buffer");
        ui.add(egui::DragValue::new(&mut d.buffer_size).range(32..=8192).suffix(" bytes"));
        ui.end_row();
        ui.label("Travel speed (for estimates)");
        ui.add(egui::DragValue::new(&mut d.rapid).range(1.0..=2000.0).suffix(" mm/s"));
        ui.end_row();
        ui.label("Finish position");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut d.finish_x).prefix("X "));
            ui.add(egui::DragValue::new(&mut d.finish_y).prefix("Y "));
        });
        ui.end_row();
    });
    ui.checkbox(&mut d.origin_top, "Origin (0,0) is the upper-left corner (Y counts down toward the front)");
    if d.router() {
        ui.checkbox(&mut d.tool_change_pause, "Pause for a tool change when layers use different tools");
    }
    ui.checkbox(&mut d.return_to_finish, "Return to the finish position after a job");
    ui.checkbox(&mut d.auto_home, "Home the machine when connecting");
    if !d.router() {
        ui.checkbox(&mut d.use_g0_overscan, "Use fast moves (G0) for overscan");
    }
    ui.checkbox(&mut d.strip_leading_zero, "Shorten numbers (.5 instead of 0.5)");
}

impl App {
    pub fn open_profiles(&mut self, sel: usize) {
        self.prof_sel = sel.min(self.profiles.list.len().saturating_sub(1));
        self.settings = self.profiles.list.get(self.prof_sel).cloned();
        self.profiles_open = true;
    }

    /// Put the edit buffer back into the selected profile and save the file
    fn save_profile(&mut self) {
        let Some(mut d) = self.settings.clone() else { return };
        let i = self.prof_sel;
        if i >= self.profiles.list.len() {
            return;
        }
        if d.name.trim().is_empty() {
            d.name = "Machine".into();
        }
        if self.profiles.list.iter().enumerate().any(|(k, o)| k != i && o.name.eq_ignore_ascii_case(&d.name)) {
            d.name = self.profiles.unique_name(&d.name);
        }
        self.settings = Some(d.clone());
        self.profiles.list[i] = d.clone();
        if i == self.profiles.active {
            self.apply_device(d);
        }
        match self.profiles.save() {
            Ok(()) => self.toast(format!("Saved machine \"{}\".", self.profiles.list[i].name), false),
            Err(e) => self.toast(format!("Could not save machine profiles: {e}"), true),
        }
    }

    /// Make a device the one in use (keeps the view sensible when the bed changes)
    fn apply_device(&mut self, d: Device) {
        let bed_changed = d.bed_w != self.dev.bed_w || d.bed_h != self.dev.bed_h;
        let kind_changed = d.machine != self.dev.machine;
        self.dev = d;
        if bed_changed {
            self.zoom_fit();
        }
        if kind_changed || bed_changed {
            self.view.preview = None;
        }
        self.save_prefs();
    }

    /// Switch to another machine profile
    pub fn use_profile(&mut self, i: usize) {
        if i >= self.profiles.list.len() || i == self.profiles.active {
            return;
        }
        self.profiles.active = i;
        let d = self.profiles.list[i].clone();
        let name = d.name.clone();
        self.apply_device(d);
        let _ = self.profiles.save();
        if self.mach.connected() {
            self.toast(format!("Now using \"{name}\". Disconnect and connect again to use its port and settings."), false);
        } else {
            self.toast(format!("Now using \"{name}\"."), false);
        }
    }

    pub fn profiles_window(&mut self, ctx: &egui::Context) {
        if !self.profiles_open {
            return;
        }
        let mut open = true;
        let mut select: Option<usize> = None;
        let (mut save, mut revert, mut use_it, mut delete, mut new_kind, mut dup) = (false, false, false, false, None::<&str>, false);
        let (mut want_import, mut want_export) = (false, false);
        let dirty = self.settings.as_ref().map(|d| self.profiles.list.get(self.prof_sel).map(|o| serde_json::to_string(o).ok() != serde_json::to_string(d).ok()).unwrap_or(false)).unwrap_or(false);
        let ports = self.ports.clone();
        egui::Window::new("Machine profiles").open(&mut open).default_size([820.0, 560.0]).resizable(true).show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("➕ New laser").clicked() {
                    new_kind = Some("laser");
                }
                if ui.button("➕ New router").clicked() {
                    new_kind = Some("router");
                }
                if ui.button("⎘ Duplicate").on_hover_text("Copy the selected machine").clicked() {
                    dup = true;
                }
                ui.separator();
                want_import = ui.button("Import JSON…").on_hover_text("Add machines from a JSON file (same name is replaced)").clicked();
                want_export = ui.button("Export JSON…").on_hover_text("Save all machine profiles as a JSON file").clicked();
                ui.separator();
                let connected = self.mach.connected();
                ui.label(RichText::new(if connected { "Connected: changes to the port or speed apply when you reconnect" } else { "Not connected: you can still set up and save machines" }).small().weak());
            });
            ui.separator();
            ui.columns(2, |cols| {
                egui::ScrollArea::vertical().id_salt("proflist").auto_shrink([false, false]).max_height(470.0).show(&mut cols[0], |ui| {
                    for (i, d) in self.profiles.list.iter().enumerate() {
                        let active = i == self.profiles.active;
                        let title = format!("{}{}", if d.router() { "🛠 " } else { "🔆 " }, d.name);
                        let r = ui.selectable_label(self.prof_sel == i, RichText::new(title).strong());
                        let mut sub = format!("{} · {} × {} mm · {}", if d.router() { "CNC router" } else { "Laser" }, d.bed_w, d.bed_h, if d.port.is_empty() { "no port set".to_string() } else { d.port.clone() });
                        if active {
                            sub += " · in use";
                        }
                        ui.label(RichText::new(sub).small().color(if active { Color32::from_rgb(0x9c, 0xd6, 0x8a) } else { Color32::GRAY }));
                        if r.clicked() {
                            select = Some(i);
                        }
                        if r.double_clicked() {
                            select = Some(i);
                            use_it = true;
                        }
                        ui.add_space(3.0);
                    }
                });
                let ui = &mut cols[1];
                egui::ScrollArea::vertical().id_salt("profedit").auto_shrink([false, false]).max_height(470.0).show(ui, |ui| {
                    if let Some(d) = &mut self.settings {
                        device_editor(ui, d, &ports);
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        save = ui.add_enabled(dirty, egui::Button::new("💾 Save").fill(if dirty { BLUE } else { Color32::TRANSPARENT })).clicked();
                        revert = ui.add_enabled(dirty, egui::Button::new("Revert")).clicked();
                        let active = self.prof_sel == self.profiles.active;
                        use_it |= ui.add_enabled(!active, egui::Button::new(if active { "✔ In use" } else { "Use this machine" })).clicked();
                        delete = ui.add_enabled(self.profiles.list.len() > 1, egui::Button::new("🗑 Delete")).clicked();
                    });
                    if dirty {
                        ui.label(RichText::new("Unsaved changes").color(Color32::from_rgb(0xff, 0xa0, 0x30)).small());
                    }
                });
            });
        });
        if save {
            self.save_profile();
        }
        if revert {
            self.settings = self.profiles.list.get(self.prof_sel).cloned();
        }
        if let Some(i) = select {
            if i != self.prof_sel {
                if dirty {
                    // keep what was typed before showing another machine
                    self.save_profile();
                }
                self.prof_sel = i;
                self.settings = self.profiles.list.get(i).cloned();
            }
        }
        if use_it {
            if dirty {
                self.save_profile();
            }
            self.use_profile(self.prof_sel);
        }
        if let Some(kind) = new_kind {
            if dirty {
                self.save_profile();
            }
            let mut d = Device { name: self.profiles.unique_name(if kind == "router" { "New router" } else { "New laser" }), machine: kind.into(), ..Default::default() };
            if kind == "router" {
                d.bed_w = 600.0;
                d.bed_h = 400.0;
                d.s_max = 24000.0;
                d.origin_top = false;
            }
            self.profiles.list.push(d.clone());
            self.prof_sel = self.profiles.list.len() - 1;
            self.settings = Some(d);
            let _ = self.profiles.save();
        }
        if dup {
            if let Some(mut d) = self.settings.clone() {
                d.name = self.profiles.unique_name(&format!("{} copy", d.name));
                self.profiles.list.push(d.clone());
                self.prof_sel = self.profiles.list.len() - 1;
                self.settings = Some(d);
                let _ = self.profiles.save();
            }
        }
        if delete && self.profiles.list.len() > 1 {
            let i = self.prof_sel;
            let name = self.profiles.list[i].name.clone();
            self.profiles.list.remove(i);
            if self.profiles.active == i {
                self.profiles.active = 0;
                let d = self.profiles.list[0].clone();
                self.apply_device(d);
            } else if self.profiles.active > i {
                self.profiles.active -= 1;
            }
            self.prof_sel = self.profiles.active;
            self.settings = self.profiles.list.get(self.prof_sel).cloned();
            let _ = self.profiles.save();
            self.toast(format!("Deleted machine \"{name}\"."), false);
        }
        if want_import {
            self.open_file_dlg(Purpose::ImportMachines);
        }
        if want_export {
            self.open_file_dlg(Purpose::ExportMachines);
        }
        if !open {
            if dirty {
                self.save_profile();
            }
            self.profiles_open = false;
            self.settings = None;
        }
    }

    /// Machine picker for the toolbar
    pub fn profile_picker(&mut self, ui: &mut egui::Ui) {
        let mut pick = None;
        let mut edit = false;
        let name = self.dev.name.clone();
        egui::ComboBox::from_id_salt("machinepick").width(150.0).selected_text(format!("🖧 {name}")).show_ui(ui, |ui| {
            for (i, d) in self.profiles.list.iter().enumerate() {
                let label = format!("{}{}", if d.router() { "🛠 " } else { "🔆 " }, d.name);
                if ui.selectable_label(i == self.profiles.active, label).clicked() {
                    pick = Some(i);
                }
            }
            ui.separator();
            edit = ui.selectable_label(false, "⚙ Edit machine profiles…").clicked();
        });
        if let Some(i) = pick {
            self.use_profile(i);
        }
        if edit {
            self.open_profiles(self.profiles.active);
        }
    }
}
