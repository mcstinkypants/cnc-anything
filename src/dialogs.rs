//! A small built-in file browser (works the same everywhere, including inside the AppImage).

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Purpose {
    OpenProject,
    SaveProject,
    Import,
    ExportGcode,
    ExportSvg,
    ImportMaterials,
    ExportMaterials,
    ImportTools,
    ExportTools,
    ImportFeeds,
    ExportFeeds,
    ImportMachines,
    ExportMachines,
}

pub struct FileDlg {
    pub purpose: Purpose,
    pub dir: PathBuf,
    pub name: String,
    pub save: bool,
    pub exts: Vec<&'static str>,
    pub show_all: bool,
    entries: Vec<(String, bool)>,
    pub err: Option<String>,
    path_text: String,
}

impl FileDlg {
    pub fn new(purpose: Purpose, dir: &Path, name: &str) -> FileDlg {
        let (save, exts): (bool, Vec<&'static str>) = match purpose {
            Purpose::OpenProject => (false, vec!["cnca", "burner", "lsproj", "json"]),
            Purpose::SaveProject => (true, vec!["cnca"]),
            Purpose::Import => (false, vec!["svg", "png", "jpg", "jpeg", "gif", "bmp", "webp"]),
            Purpose::ExportGcode => (true, vec!["gc", "gcode", "nc"]),
            Purpose::ExportSvg => (true, vec!["svg"]),
            Purpose::ImportMaterials => (false, vec!["json"]),
            Purpose::ExportMaterials => (true, vec!["json"]),
            Purpose::ImportTools | Purpose::ImportFeeds | Purpose::ImportMachines => (false, vec!["json"]),
            Purpose::ExportMachines => (true, vec!["json"]),
            Purpose::ExportTools | Purpose::ExportFeeds => (true, vec!["json"]),
        };
        let mut d = FileDlg { purpose, dir: dir.to_path_buf(), name: name.to_string(), save, exts, show_all: false, entries: vec![], err: None, path_text: String::new() };
        d.refresh();
        d
    }
    pub fn title(&self) -> &'static str {
        match self.purpose {
            Purpose::OpenProject => "Open project",
            Purpose::SaveProject => "Save project",
            Purpose::Import => "Import a drawing or picture",
            Purpose::ExportGcode => "Save G-code",
            Purpose::ExportSvg => "Export SVG",
            Purpose::ImportMaterials => "Import material library",
            Purpose::ExportMaterials => "Export material library",
            Purpose::ImportTools => "Import tool library",
            Purpose::ExportTools => "Export tool library",
            Purpose::ImportFeeds => "Import speeds & feeds",
            Purpose::ImportMachines => "Import machine profiles",
            Purpose::ExportMachines => "Export machine profiles",
            Purpose::ExportFeeds => "Export speeds & feeds",
        }
    }
    fn refresh(&mut self) {
        self.entries.clear();
        self.path_text = self.dir.display().to_string();
        match std::fs::read_dir(&self.dir) {
            Ok(rd) => {
                for e in rd.flatten() {
                    let n = e.file_name().to_string_lossy().to_string();
                    if n.starts_with('.') {
                        continue;
                    }
                    let is_dir = e.path().is_dir();
                    if !is_dir && !self.show_all {
                        let ext = Path::new(&n).extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
                        if !self.exts.contains(&ext.as_str()) {
                            continue;
                        }
                    }
                    self.entries.push((n, is_dir));
                }
                self.entries.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
                self.err = None;
            }
            Err(e) => self.err = Some(format!("Can't open this folder: {e}")),
        }
    }
    fn go(&mut self, p: PathBuf) {
        self.dir = p;
        self.refresh();
    }

    /// Shows the browser. Returns Some(path) when the person picks a file, Some(empty) when canceled.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<PathBuf> {
        let mut result = None;
        let mut open = true;
        egui::Window::new(self.title()).collapsible(false).resizable(true).default_size([620.0, 460.0]).open(&mut open).show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("⬆ Up").clicked() {
                    if let Some(p) = self.dir.parent() {
                        let p = p.to_path_buf();
                        self.go(p);
                    }
                }
                if ui.button("🏠 Home").clicked() {
                    if let Some(h) = std::env::var_os("HOME") {
                        self.go(PathBuf::from(h));
                    }
                }
                let r = ui.add(egui::TextEdit::singleline(&mut self.path_text).desired_width(f32::INFINITY));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let p = PathBuf::from(self.path_text.trim());
                    if p.is_dir() {
                        self.go(p);
                    } else if !self.save && p.is_file() {
                        result = Some(p);
                    }
                }
            });
            ui.separator();
            let h = ui.available_height() - 70.0;
            egui::ScrollArea::vertical().max_height(h.max(120.0)).auto_shrink([false, false]).show(ui, |ui| {
                if let Some(e) = &self.err {
                    ui.colored_label(egui::Color32::LIGHT_RED, e);
                }
                let mut nav = None;
                for (n, is_dir) in &self.entries {
                    let label = if *is_dir { format!("📁 {n}") } else { format!("📄 {n}") };
                    let sel = !*is_dir && *n == self.name;
                    let r = ui.selectable_label(sel, label);
                    if r.clicked() {
                        if *is_dir {
                            nav = Some(self.dir.join(n));
                        } else {
                            self.name = n.clone();
                        }
                    }
                    if r.double_clicked() && !*is_dir {
                        result = Some(self.dir.join(n));
                    }
                }
                if let Some(p) = nav {
                    self.go(p);
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(if self.save { "Save as:" } else { "File:" });
                ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(320.0));
                if ui.checkbox(&mut self.show_all, "Show all files").changed() {
                    self.refresh();
                }
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("Types: {}", self.exts.iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(" "))).weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ok = ui.add_enabled(!self.name.trim().is_empty(), egui::Button::new(if self.save { "Save" } else { "Open" })).clicked();
                    if ui.button("Cancel").clicked() {
                        result = Some(PathBuf::new());
                    }
                    if ok {
                        let mut name = self.name.trim().to_string();
                        if self.save && Path::new(&name).extension().is_none() {
                            name = format!("{name}.{}", self.exts[0]);
                        }
                        let p = if Path::new(&name).is_absolute() { PathBuf::from(&name) } else { self.dir.join(&name) };
                        if !self.save && !p.is_file() {
                            self.err = Some("That file doesn't exist.".into());
                        } else {
                            result = Some(p);
                        }
                    }
                });
            });
        });
        if !open {
            result = Some(PathBuf::new());
        }
        result
    }
}
