//! CNC Anything's window: menus, toolbar, layers, libraries, machine panel, files and dialogs.
//! The canvas (drawing, tools, snapping) is canvas::View.

use crate::canvas::{Preview, Tool, View};
use crate::dialogs::{FileDlg, Purpose};
use crate::gcode::*;
use crate::geom::*;
use crate::importers::{self, TextSpec};
use crate::machine::{Cmd, Machine};
use crate::materials::{self, Library};
use crate::model::*;
use egui::Color32;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Laser,
    Move,
    Console,
}

pub struct TextDlg {
    pub target: Option<u64>,
    pub at: P,
    pub text: String,
    pub font: String,
    pub size: f64,
    pub align: String,
    pub bold: bool,
    pub italic: bool,
}

pub struct App {
    pub doc: Doc,
    pub dev: Device,
    pub lib: Library,
    pub profiles: crate::profiles::Profiles,
    pub profiles_open: bool,
    pub prof_sel: usize,
    pub libs: crate::tools::Libraries,
    pub tools_open: bool,
    pub feeds_open: bool,
    pub tool_sel: Option<usize>,
    pub feed_sel: Option<usize>,
    pub feed_filter: String,
    pub feed_tool: Option<u32>,
    pub jog_z: f64,
    /// which point of the selection X/Y refer to: (row 0 top..2 bottom, column 0 left..2 right)
    pub pos_anchor: (u8, u8),
    pub mach: Machine,
    pub imgs: ImgCache,
    pub textures: HashMap<usize, egui::TextureHandle>,
    pub view: View,
    pub snap_grid: bool,
    pub snap_obj: bool,
    pub snap_step: f64,
    pub lock_ratio: bool,
    pub poly_sides: u32,
    pub active_layer: usize,
    pub edit_layer: Option<usize>,
    pub clip: Vec<Shape>,
    pub doc_name: String,
    pub doc_path: Option<PathBuf>,
    pub last_dir: PathBuf,
    pub autosave_due: Option<Instant>,
    pub start_from: StartFrom,
    pub anchor: (u8, u8),
    pub sel_only: bool,
    pub inner_first: bool,
    pub last_job_time: f64,
    pub frame_until: Option<Instant>,
    pub tab: Tab,
    pub jog_dist: f64,
    pub jog_speed: f64,
    pub console_in: String,
    pub ports: Vec<String>,
    pub toast: Option<(String, bool, Instant)>,
    pub settings: Option<Device>,
    pub text_dlg: Option<TextDlg>,
    pub file_dlg: Option<FileDlg>,
    pub info: Option<(String, String)>,
    pub confirm: Option<(String, &'static str)>,
    pub mat_open: bool,
    pub mat_sel: Option<usize>,
    pub mat_filter: String,
    pub sys_fonts: Option<Vec<String>>,
    pub num: [f64; 5],
    pub shot: Option<PathBuf>,
    pub frame_no: u64,
    pub shot_asked: bool,
    pub write_config: bool,
}

fn read_json<T: serde::de::DeserializeOwned>(p: &Path) -> Option<T> {
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Prefs {
    snap_grid: bool,
    snap_obj: bool,
    snap_step: f64,
    last_dir: Option<PathBuf>,
    #[serde(default)]
    dark_canvas: bool,
    #[serde(default)]
    pos_anchor: Option<(u8, u8)>,
}

const BLUE: Color32 = Color32::from_rgb(0x2f, 0x5f, 0xb3);

impl App {
    pub fn new(cc: &eframe::CreationContext, shot: Option<PathBuf>) -> App {
        let mut vis = egui::Visuals::dark();
        vis.panel_fill = Color32::from_rgb(0x33, 0x36, 0x3b);
        vis.window_fill = Color32::from_rgb(0x33, 0x36, 0x3b);
        vis.extreme_bg_color = Color32::from_rgb(0x23, 0x25, 0x2a);
        vis.selection.bg_fill = BLUE;
        cc.egui_ctx.set_visuals(vis);
        cc.egui_ctx.style_mut(|s| {
            s.spacing.item_spacing = egui::vec2(6.0, 5.0);
            s.spacing.button_padding = egui::vec2(8.0, 3.0);
        });
        let cfg = materials::config_dir();
        let write_config = shot.is_none();
        let dev0: Device = read_json(&cfg.join("device.json")).unwrap_or_default();
        let profiles = crate::profiles::Profiles::load(write_config, &dev0);
        let dev = profiles.list[profiles.active].clone();
        let prefs: Option<Prefs> = read_json(&cfg.join("prefs.json"));
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        let lib = if write_config { Library::load() } else { Library::in_memory(Library::load_readonly()) };
        let dark_canvas = prefs.as_ref().map(|p| p.dark_canvas).unwrap_or(false);
        let mut app = App {
            doc: Doc::new(),
            dev,
            lib,
            profiles,
            profiles_open: false,
            prof_sel: 0,
            libs: crate::tools::Libraries::load(write_config),
            tools_open: false,
            feeds_open: false,
            tool_sel: None,
            feed_sel: None,
            feed_filter: String::new(),
            feed_tool: None,
            jog_z: 1.0,
            pos_anchor: prefs.as_ref().and_then(|p| p.pos_anchor).unwrap_or((0, 0)),
            mach: Machine::new(),
            imgs: ImgCache::new(),
            textures: HashMap::new(),
            view: View::new(),
            snap_grid: prefs.as_ref().map(|p| p.snap_grid).unwrap_or(true),
            snap_obj: prefs.as_ref().map(|p| p.snap_obj).unwrap_or(true),
            snap_step: prefs.as_ref().map(|p| p.snap_step).unwrap_or(0.0),
            lock_ratio: true,
            poly_sides: 6,
            active_layer: 0,
            edit_layer: None,
            clip: vec![],
            doc_name: "Untitled".into(),
            doc_path: None,
            last_dir: prefs.and_then(|p| p.last_dir).filter(|d| d.is_dir()).unwrap_or(home),
            autosave_due: None,
            start_from: StartFrom::Absolute,
            anchor: (2, 0),
            sel_only: false,
            inner_first: true,
            last_job_time: 0.0,
            frame_until: None,
            tab: Tab::Laser,
            jog_dist: 10.0,
            jog_speed: 50.0,
            console_in: String::new(),
            ports: crate::machine::list_ports(),
            toast: None,
            settings: None,
            text_dlg: None,
            file_dlg: None,
            info: None,
            confirm: None,
            mat_open: false,
            mat_sel: None,
            mat_filter: String::new(),
            sys_fonts: None,
            num: [0.0; 5],
            shot,
            frame_no: 0,
            shot_asked: false,
            write_config,
        };
        app.view.dark = dark_canvas;
        if app.dev.port.is_empty() || !app.ports.contains(&app.dev.port) {
            app.dev.port = app.ports.first().cloned().unwrap_or_default();
        }
        let restored = if write_config { std::fs::read_to_string(cfg.join("autosave.cnca")).ok().and_then(|t| app.load_project_text(&t, "Untitled").ok()) } else { None };
        if restored.is_none() {
            app.load_sample();
        }
        // for checking screens without clicking: CNCA_SHOW=materials, preview, dark or dark-preview
        match std::env::var("CNCA_SHOW").as_deref() {
            Ok("materials") => {
                app.mat_open = true;
                app.mat_sel = Some(0);
            }
            Ok("preview") => app.cmd("preview"),
            Ok("dark") => app.view.dark = true,
            Ok("profiles") => app.open_profiles(app.profiles.active),
            Ok("nodes") => {
                // the outline converted to a curve, editing its second point; plus a drawn spline
                if let Some(i) = app.doc.shapes.iter().position(|s| s.kind == "rect") {
                    let id = app.doc.shapes[i].id;
                    if let Some(c) = crate::spline::to_spline(&app.doc.shapes[i]) {
                        app.doc.shapes[i] = c;
                    }
                    let c = &mut app.doc.shapes[i].curves[0];
                    c.move_handle(1, true, [c.nodes[1].o[0] + 25.0, c.nodes[1].o[1] - 12.0]);
                    app.doc.sel = std::iter::once(id).collect();
                    app.view.tool = Tool::Nodes;
                    app.view.node_sel = Some(crate::canvas_nodes::NodeRef { shape: id, curve: 0, node: 1 });
                }
                let (cx, cy) = (app.dev.bed_w / 2.0, app.dev.bed_h / 2.0);
                let mut sp = Shape::new("spline");
                sp.layer = 5;
                sp.curves = vec![crate::spline::Curve {
                    nodes: vec![
                        crate::spline::Node::sharp([cx - 90.0, cy + 60.0]),
                        crate::spline::Node::with([cx - 30.0, cy + 75.0], [cx - 55.0, cy + 75.0], [cx - 5.0, cy + 75.0], "symmetric"),
                        crate::spline::Node::with([cx + 30.0, cy + 55.0], [cx + 15.0, cy + 45.0], [cx + 50.0, cy + 70.0], "smooth"),
                        crate::spline::Node::sharp([cx + 90.0, cy + 60.0]),
                    ],
                    closed: false,
                }];
                app.doc.add(sp);
            }
            Ok("stretch") => {
                // a stretched rounded rectangle and stretched text, text selected
                for s in app.doc.shapes.iter_mut() {
                    let k = match s.kind.as_str() {
                        "rect" => 1.35,
                        "text" => 1.6,
                        _ => continue,
                    };
                    let b = s.bbox().unwrap();
                    s.tf = mul(&scale_about(k, 1.0, b.x0, b.y0), &s.tf);
                    s.bake_scale();
                }
                app.doc.sel = app.doc.shapes.iter().filter(|s| s.kind == "text").map(|s| s.id).collect();
                app.pos_anchor = (1, 1);
            }
            Ok("router") | Ok("router-preview") | Ok("tools") | Ok("feeds") => {
                app.dev.machine = "router".into();
                let l = &mut app.doc.layers;
                l[2].op = "Profile outside".into();
                l[2].depth = 6.0;
                l[2].stepdown = 2.0;
                l[2].tabs = true;
                l[1].op = "Pocket".into();
                l[1].tool_id = 1;
                l[1].depth = 1.5;
                l[1].stepdown = 0.8;
                l[5].op = "On line".into();
                l[5].tool_id = 6;
                l[5].depth = 1.0;
                match std::env::var("CNCA_SHOW").as_deref() {
                    Ok("router-preview") => app.cmd("preview"),
                    Ok("tools") => {
                        app.tools_open = true;
                        app.tool_sel = Some(1);
                    }
                    Ok("feeds") => {
                        app.feeds_open = true;
                        app.feed_sel = Some(0);
                    }
                    _ => {}
                }
            }
            Ok("dark-preview") => {
                app.view.dark = true;
                app.cmd("preview");
            }
            _ => {}
        }
        app
    }

    // ------------------------------------------------------------ helpers
    pub fn toast(&mut self, msg: impl Into<String>, err: bool) {
        self.toast = Some((msg.into(), err, Instant::now()));
    }
    pub fn edited(&mut self) {
        self.doc.dirty = true;
        self.autosave_due = Some(Instant::now() + Duration::from_millis(1200));
    }
    pub fn save_prefs(&mut self) {
        // the machine in use is also kept in its profile
        let a = self.profiles.active;
        if let Some(p) = self.profiles.list.get_mut(a) {
            if p.name == self.dev.name || !self.profiles_open {
                *p = self.dev.clone();
            }
        }
        if self.write_config {
            let _ = self.profiles.save();
        }
        if !self.write_config {
            return;
        }
        let d = materials::config_dir();
        let _ = std::fs::create_dir_all(&d);
        let _ = std::fs::write(d.join("device.json"), serde_json::to_string_pretty(&self.dev).unwrap_or_default());
        let p = Prefs { snap_grid: self.snap_grid, snap_obj: self.snap_obj, snap_step: self.snap_step, last_dir: Some(self.last_dir.clone()), dark_canvas: self.view.dark, pos_anchor: Some(self.pos_anchor) };
        let _ = std::fs::write(d.join("prefs.json"), serde_json::to_string_pretty(&p).unwrap_or_default());
    }
    pub fn write_autosave(&self) {
        if self.write_config {
            let d = materials::config_dir();
            let _ = std::fs::create_dir_all(&d);
            let _ = std::fs::write(d.join("autosave.cnca"), serde_json::to_string(&self.doc.to_file()).unwrap_or_default());
        }
    }
    pub fn autosave(&mut self) {
        if self.autosave_due.map(|t| Instant::now() >= t).unwrap_or(false) {
            self.autosave_due = None;
            self.write_autosave();
        }
    }
    pub fn shape_box(&mut self, id: u64) -> Option<BBox> {
        let s = self.doc.shapes.iter().find(|s| s.id == id)?;
        self.view.bbox(s)
    }
    pub fn add_shape(&mut self, mut s: Shape) {
        self.doc.checkpoint();
        if s.layer >= 30 {
            s.layer = self.active_layer;
        }
        let id = self.doc.add(s);
        self.doc.sel.clear();
        self.doc.sel.insert(id);
        self.set_tool(Tool::Select);
        self.edited();
    }
    pub fn set_tool(&mut self, t: Tool) {
        if self.view.tool == Tool::Spline && t != Tool::Spline {
            // keep a spline that was being drawn
            self.finish_open_spline();
        }
        if t != Tool::Nodes {
            self.view.node_sel = None;
        }
        self.view.tool = t;
        self.view.line_pts = None;
        self.view.measure = None;
        self.view.drag = None;
    }

    pub fn load_sample(&mut self) {
        self.doc.clear();
        self.view.forget_cache();
        let (cx, cy) = (self.dev.bed_w / 2.0, self.dev.bed_h / 2.0);
        {
            let l = &mut self.doc.layers;
            l[2].name = "Outline".into();
            l[2].speed = 10.0;
            l[2].power = 80.0;
            l[2].passes = 2;
            l[1].name = "Lettering".into();
            l[1].mode = "Fill".into();
            l[1].speed = 150.0;
            l[1].power = 35.0;
            l[5].name = "Details".into();
            l[5].speed = 40.0;
            l[5].power = 30.0;
        }
        let mut r = Shape::new("rect");
        r.w = 180.0;
        r.h = 70.0;
        r.r = 8.0;
        r.layer = 2;
        r.tf = translate(cx - 90.0, cy - 35.0);
        self.doc.add(r);
        for x in [cx - 82.0, cx + 76.0] {
            let mut e = Shape::new("ellipse");
            e.w = 6.0;
            e.h = 6.0;
            e.layer = 2;
            e.tf = translate(x, cy - 3.0);
            self.doc.add(e);
        }
        let (cs, w, h) = importers::text_outlines(&TextSpec { text: "MAKE IT", font: "Sans", size: 22.0, align: "left", bold: true, italic: false });
        let mut t = Shape::new("text");
        t.text = "MAKE IT".into();
        t.font = "Sans".into();
        t.size = 22.0;
        t.bold = true;
        t.subpaths = cs;
        t.w = w;
        t.h = h;
        t.layer = 1;
        t.tf = translate(cx - w / 2.0 + 14.0, cy - h / 2.0 + 6.0);
        self.doc.add(t);
        let mut p = Shape::new("polygon");
        p.w = 30.0;
        p.h = 30.0;
        p.sides = 6;
        p.layer = 5;
        p.tf = translate(cx - 70.0, cy - 15.0);
        self.doc.add(p);
        let mut l = Shape::new("path");
        l.subpaths = vec![Contour { pts: vec![[cx - 50.0, cy - 24.0], [cx + 70.0, cy - 24.0]], closed: false }];
        l.layer = 5;
        self.doc.add(l);
        self.doc.order = vec![5, 1, 2];
        self.doc.dirty = false;
        self.doc_name = "Sample nameplate".into();
        self.doc_path = None;
        self.edit_layer = Some(2);
        self.active_layer = 2;
    }

    fn load_project_text(&mut self, text: &str, name: &str) -> Result<(), String> {
        let f: ProjectFile = serde_json::from_str(text).map_err(|e| format!("This is not a project file ({e})"))?;
        self.doc.load(f);
        self.view.forget_cache();
        self.view.preview = None;
        for s in &self.doc.shapes {
            if s.kind == "image" && !self.imgs.contains_key(&img_key(&s.src)) {
                if let Some(g) = importers::data_url_bytes(&s.src).and_then(|b| importers::decode_gray(&b).ok()) {
                    self.imgs.insert(img_key(&s.src), g);
                }
            }
        }
        self.doc_name = name.to_string();
        self.edit_layer = None;
        Ok(())
    }

    fn import_path(&mut self, p: &Path) {
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let Ok(bytes) = std::fs::read(p) else {
            return self.toast(format!("Could not read {}", p.display()), true);
        };
        let fname = p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        if ext == "svg" {
            match importers::import_svg(&String::from_utf8_lossy(&bytes)) {
                Ok(items) if !items.is_empty() => {
                    self.doc.checkpoint();
                    self.doc.sel.clear();
                    for it in &items {
                        let mut s = Shape::new("path");
                        s.subpaths = it.subpaths.clone();
                        s.layer = it.color.map(nearest_palette).unwrap_or(self.active_layer);
                        let id = self.doc.add(s);
                        self.doc.sel.insert(id);
                    }
                    if let Some(b) = self.doc.sel_box() {
                        if b.x0 < 0.0 || b.y0 < 0.0 || b.x1 > self.dev.bed_w || b.y1 > self.dev.bed_h {
                            let tx = (self.dev.bed_w - b.w()).clamp(0.0, 10.0) - b.x0;
                            let ty = (self.dev.bed_h - b.h()).clamp(0.0, 10.0) - b.y0;
                            self.doc.transform_sel(&translate(tx, ty));
                        }
                    }
                    self.edited();
                    self.toast(format!("Imported {} shape{} from {fname}", items.len(), if items.len() > 1 { "s" } else { "" }), false);
                }
                Ok(_) => self.toast("No shapes found in that drawing.", true),
                Err(e) => self.toast(e, true),
            }
        } else {
            match importers::decode_gray(&bytes) {
                Ok(g) => {
                    let src: Arc<str> = Arc::from(importers::data_url(&bytes, p));
                    let dpi = 254.0; // pictures arrive at 0.1 mm per pixel and can be resized
                    let (mut w, mut h) = (g.w as f64 / dpi * 25.4, g.h as f64 / dpi * 25.4);
                    let k = (self.dev.bed_w * 0.8 / w).min(self.dev.bed_h * 0.8 / h).min(1.0);
                    w *= k;
                    h *= k;
                    self.imgs.insert(img_key(&src), g);
                    let used: std::collections::HashSet<usize> = self.doc.shapes.iter().map(|s| s.layer).collect();
                    let li = (1..30).find(|i| !used.contains(i)).unwrap_or(self.active_layer);
                    self.doc.layers[li].mode = "Image".into();
                    if self.doc.layers[li].speed == 100.0 {
                        self.doc.layers[li].speed = 150.0;
                    }
                    let mut s = Shape::new("image");
                    s.src = src;
                    s.w = w;
                    s.h = h;
                    s.layer = li;
                    let y = if self.dev.origin_top { self.dev.bed_h - 10.0 - h } else { 10.0 };
                    s.tf = translate(10.0, y);
                    self.add_shape(s);
                    self.edit_layer = Some(li);
                    self.toast(format!("Picture placed on layer C{li:02} (Image mode)."), false);
                }
                Err(e) => self.toast(e, true),
            }
        }
    }

    fn export_svg(&self) -> String {
        let (w, h) = (self.dev.bed_w, self.dev.bed_h);
        let mut body = String::new();
        for s in &self.doc.shapes {
            if s.kind == "image" {
                let m = mul(&[1.0, 0.0, 0.0, -1.0, 0.0, h], &s.tf);
                body += &format!(
                    "<image href=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" transform=\"matrix({} {} {} {} {} {}) translate(0 {}) scale(1 -1)\"/>\n",
                    s.src, s.w, s.h, m[0], m[1], m[2], m[3], m[4], m[5], s.h
                );
                continue;
            }
            let mut d = String::new();
            for c in s.world_contours() {
                for (i, p) in c.pts.iter().enumerate() {
                    d += &format!("{}{},{}", if i == 0 { "M" } else { "L" }, fmt(p[0], 3, false), fmt(h - p[1], 3, false));
                }
                if c.closed {
                    d += "Z";
                }
            }
            body += &format!("<path d=\"{d}\" fill=\"none\" stroke=\"{}\" stroke-width=\"0.1\"/>\n", PALETTE[s.layer]);
        }
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"0 0 {w} {h}\">\n{body}</svg>\n")
    }

    pub fn file_chosen(&mut self, purpose: Purpose, p: PathBuf) {
        if let Some(d) = p.parent() {
            self.last_dir = d.to_path_buf();
            self.save_prefs();
        }
        let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let write = |p: &Path, data: &str| std::fs::write(p, data).map_err(|e| format!("Could not save {}: {e}", p.display()));
        match purpose {
            Purpose::OpenProject => match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| self.load_project_text(&t, &name)) {
                Ok(()) => {
                    self.doc_path = Some(p.clone());
                    self.toast(format!("Opened {}", p.display()), false);
                    self.autosave_due = Some(Instant::now());
                }
                Err(e) => self.toast(format!("Could not open that file: {e}"), true),
            },
            Purpose::SaveProject => match write(&p, &serde_json::to_string(&self.doc.to_file()).unwrap_or_default()) {
                Ok(()) => {
                    self.doc_name = name;
                    self.doc_path = Some(p.clone());
                    self.doc.dirty = false;
                    self.toast(format!("Saved {}", p.display()), false);
                }
                Err(e) => self.toast(e, true),
            },
            Purpose::Import => self.import_path(&p),
            Purpose::ExportGcode => {
                if let Some(j) = self.build_job() {
                    match write(&p, &j.text) {
                        Ok(()) => self.toast(format!("G-code saved ({} lines, about {}).", j.lines, fmt_time(j.time)), false),
                        Err(e) => self.toast(e, true),
                    }
                }
            }
            Purpose::ExportSvg => match write(&p, &self.export_svg()) {
                Ok(()) => self.toast(format!("Saved {}", p.display()), false),
                Err(e) => self.toast(e, true),
            },
            Purpose::ImportMaterials => match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| self.lib.import_json(&t)) {
                Ok((a, r)) => {
                    let _ = self.lib.save();
                    self.mat_sel = None;
                    self.mat_open = true;
                    self.toast(format!("Material library: {a} added, {r} updated."), false);
                }
                Err(e) => self.toast(format!("Could not import materials: {e}"), true),
            },
            Purpose::ImportTools | Purpose::ImportFeeds => {
                let tools = purpose == Purpose::ImportTools;
                let res = std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| if tools { self.libs.import_tools(&t) } else { self.libs.import_feeds(&t) });
                match res {
                    Ok((a, r)) => {
                        let _ = self.libs.save();
                        if tools {
                            self.tools_open = true;
                        } else {
                            self.feeds_open = true;
                        }
                        self.toast(format!("{}: {a} added, {r} updated.", if tools { "Tool library" } else { "Speeds & feeds" }), false);
                    }
                    Err(e) => self.toast(format!("Could not import: {e}"), true),
                }
            }
            Purpose::ImportMachines => match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| self.profiles.import_json(&t)) {
                Ok((a, r)) => {
                    let _ = self.profiles.save();
                    // a replaced machine in use takes effect now
                    let d = self.profiles.list[self.profiles.active].clone();
                    self.dev = d;
                    self.open_profiles(self.prof_sel);
                    self.toast(format!("Machine profiles: {a} added, {r} updated."), false);
                }
                Err(e) => self.toast(format!("Could not import machines: {e}"), true),
            },
            Purpose::ExportMachines => match write(&p, &self.profiles.export_json()) {
                Ok(()) => self.toast(format!("Exported {} machine profiles to {}", self.profiles.list.len(), p.display()), false),
                Err(e) => self.toast(e, true),
            },
            Purpose::ExportTools => match write(&p, &self.libs.export_tools()) {
                Ok(()) => self.toast(format!("Exported {} tools to {}", self.libs.tools.len(), p.display()), false),
                Err(e) => self.toast(e, true),
            },
            Purpose::ExportFeeds => match write(&p, &self.libs.export_feeds()) {
                Ok(()) => self.toast(format!("Exported {} speeds & feeds entries to {}", self.libs.feeds.len(), p.display()), false),
                Err(e) => self.toast(e, true),
            },
            Purpose::ExportMaterials => match write(&p, &self.lib.export_json()) {
                Ok(()) => self.toast(format!("Exported {} materials to {}", self.lib.materials.len(), p.display()), false),
                Err(e) => self.toast(e, true),
            },
        }
    }

    pub fn open_file_dlg(&mut self, purpose: Purpose) {
        let name = match purpose {
            Purpose::SaveProject => format!("{}.cnca", self.doc_name),
            Purpose::ExportGcode => format!("{}.gc", self.doc_name),
            Purpose::ExportSvg => format!("{}.svg", self.doc_name),
            Purpose::ExportMaterials => "cnc-anything-laser-materials.json".into(),
            Purpose::ExportTools => "cnc-anything-tools.json".into(),
            Purpose::ExportMachines => "cnc-anything-machines.json".into(),
            Purpose::ExportFeeds => "cnc-anything-speeds-and-feeds.json".into(),
            _ => String::new(),
        };
        let dir = if purpose == Purpose::SaveProject { self.doc_path.as_ref().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or(self.last_dir.clone()) } else { self.last_dir.clone() };
        self.file_dlg = Some(FileDlg::new(purpose, &dir, &name));
    }

    // ------------------------------------------------------------ job
    pub fn job_box(&mut self) -> Option<BBox> {
        let ids: Vec<u64> = self.doc.shapes.iter().filter(|s| self.doc.layers[s.layer].output && (!self.sel_only || self.doc.sel.contains(&s.id))).map(|s| s.id).collect();
        let mut b = None;
        for id in ids {
            b = BBox::union(b, self.shape_box(id));
        }
        b
    }
    fn anchor_point(&self, b: &BBox) -> P {
        // anchor rows are screen rows: 0 = top, 2 = bottom
        let x = match self.anchor.1 {
            0 => b.x0,
            2 => b.x1,
            _ => b.cx(),
        };
        let y = match self.anchor.0 {
            0 => b.y1,
            2 => b.y0,
            _ => b.cy(),
        };
        [x, y]
    }
    pub fn build_job(&mut self) -> Option<Job> {
        let Some(bx) = self.job_box() else {
            self.toast("Nothing to burn: draw or import something first (and check the layer's Output box).", true);
            return None;
        };
        let origin = if self.start_from == StartFrom::Absolute { [0.0, 0.0] } else { self.anchor_point(&bx) };
        let start = if self.start_from == StartFrom::Absolute { self.dev.from_user([0.0, 0.0]) } else { origin };
        if self.dev.router() {
            return match crate::cam::plan_router(&mut self.doc, &self.libs, &self.dev, &crate::cam::RouterOpts { selected_only: self.sel_only, start }) {
                Ok((mv, warnings)) => {
                    for w in warnings {
                        self.toast(w, true);
                    }
                    Some(crate::cam::emit_router(&mv, &self.dev, self.start_from, origin, start))
                }
                Err(e) => {
                    self.toast(e, true);
                    None
                }
            };
        }
        match plan_job(&mut self.doc, &self.imgs, &PlanOpts { selected_only: self.sel_only, inner_first: self.inner_first, start }) {
            Ok((ops, warnings)) => {
                for w in warnings {
                    self.toast(w, true);
                }
                if self.start_from == StartFrom::Absolute && (bx.x0 < -0.01 || bx.y0 < -0.01 || bx.x1 > self.dev.bed_w + 0.01 || bx.y1 > self.dev.bed_h + 0.01) {
                    self.toast("Warning: part of the job is outside the bed.", true);
                }
                Some(emit_gcode(&ops, &self.dev, self.start_from, origin, start))
            }
            Err(e) => {
                self.toast(e, true);
                None
            }
        }
    }
    pub fn open_preview(&mut self) {
        if let Some(j) = self.build_job() {
            let info = format!("Estimated time {} · cut {:.2} m · travel {:.2} m · {} lines", fmt_time(j.time), j.cut_len / 1000.0, j.travel / 1000.0, j.lines);
            let router = self.dev.router();
            let widths = (0..30).map(|i| self.libs.tool(self.doc.layers[i].tool_id).map(|t| t.diameter as f32).unwrap_or(3.0)).collect();
            let max_depth = j.preview.iter().map(|s| s.4).fold(0.0, f64::max);
            self.view.preview = Some(Preview { segs: j.preview, frac: 1.0, travel: true, info, router, widths, max_depth });
        }
    }
    pub fn need(&mut self) -> bool {
        if self.mach.connected() {
            return true;
        }
        self.toast("Connect to a laser first (or use the practice machine).", true);
        false
    }
    pub fn start_job(&mut self) {
        if !self.need() {
            return;
        }
        let (state, busy) = {
            let s = self.mach.st.lock().unwrap();
            (s.state.clone(), s.job.is_some())
        };
        if busy {
            return self.toast("A job is already running.", false);
        }
        if !state.starts_with("Idle") {
            return self.toast(format!("The machine is \"{state}\". It must be Idle to start (try Unlock or Home)."), true);
        }
        if let Some(j) = self.build_job() {
            self.view.preview = None;
            self.last_job_time = j.time;
            self.mach.cmd(Cmd::Start { name: self.doc_name.clone(), lines: j.text.lines().map(|s| s.to_string()).collect() });
        }
    }
    pub fn frame(&mut self) {
        if !self.need() {
            return;
        }
        let Some(bx) = self.job_box() else { return self.toast("Nothing to frame yet.", true) };
        let sp = self.jog_speed.max(1.0) * 60.0;
        let mut lines = vec!["G21".to_string()];
        if self.dev.router() {
            // never drag the bit across the stock: lift to the safe height first
            lines.push(format!("G90 G0 Z{}", fmt(self.dev.safe_z, 3, false)));
        }
        let corners = [[bx.x0, bx.y0], [bx.x1, bx.y0], [bx.x1, bx.y1], [bx.x0, bx.y1], [bx.x0, bx.y0]];
        if self.start_from == StartFrom::Current {
            let a = self.anchor_point(&bx);
            let ys = if self.dev.origin_top { -1.0 } else { 1.0 };
            let (mut px, mut py) = (a[0], a[1]);
            lines.push("G91".into());
            for p in corners.iter().chain(std::iter::once(&a)) {
                lines.push(format!("G0 X{} Y{} F{}", fmt(p[0] - px, 3, false), fmt(ys * (p[1] - py), 3, false), sp.round()));
                px = p[0];
                py = p[1];
            }
            lines.push("G90".into());
        } else {
            let o = if self.start_from == StartFrom::User { self.dev.to_user(self.anchor_point(&bx)) } else { [0.0, 0.0] };
            lines.push("G90".into());
            for p in corners {
                let u = self.dev.to_user(p);
                lines.push(format!("G0 X{} Y{}", fmt(u[0] - o[0], 3, false), fmt(u[1] - o[1], 3, false)));
            }
        }
        for l in lines {
            self.mach.send(&l);
        }
        self.view.frame_box = Some(bx);
        self.frame_until = Some(Instant::now() + Duration::from_secs(4));
    }
    pub fn jog_up_down(&mut self, dir: f64) {
        if self.need() {
            self.mach.jog_z(dir * self.jog_z.max(0.01), (self.jog_speed.max(1.0) * 60.0).min(1200.0));
        }
    }
    pub fn jog(&mut self, dx: f64, dy: f64) {
        if !self.need() {
            return;
        }
        // arrows are screen directions; with the origin at the top, "up" means smaller Y
        let dy = if self.dev.origin_top { -dy } else { dy };
        self.mach.jog(dx * self.jog_dist.max(0.01), dy * self.jog_dist.max(0.01), self.jog_speed.max(1.0) * 60.0);
    }
}

impl App {
    /// Open a project (or import a drawing) given on the command line
    pub fn open_on_start(&mut self, p: PathBuf) {
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if ["cnca", "burner", "lsproj", "json"].contains(&ext.as_str()) {
            self.file_chosen(Purpose::OpenProject, p);
        } else {
            self.import_path(&p);
        }
    }
}
