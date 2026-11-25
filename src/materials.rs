//! Material library: saved cut settings for different materials, applied to layers.
//! Kept in the config folder and exportable/importable as JSON.

use crate::model::Layer;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The cut settings a material stores (the parts of a layer that depend on the material)
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct CutProfile {
    pub mode: String,
    pub speed: f64,
    pub power: f64,
    pub min_power: f64,
    pub passes: u32,
    pub interval: f64,
    pub bidir: bool,
    pub cross_hatch: bool,
    pub overscan: f64,
    pub air: bool,
    pub dither: String,
    pub invert: bool,
}
impl Default for CutProfile {
    fn default() -> Self {
        CutProfile::from_layer(&Layer::new(0))
    }
}
impl CutProfile {
    pub fn from_layer(l: &Layer) -> Self {
        CutProfile {
            mode: l.mode.clone(),
            speed: l.speed,
            power: l.power,
            min_power: l.min_power,
            passes: l.passes,
            interval: l.interval,
            bidir: l.bidir,
            cross_hatch: l.cross_hatch,
            overscan: l.overscan,
            air: l.air,
            dither: l.dither.clone(),
            invert: l.invert,
        }
    }
    pub fn apply_to(&self, l: &mut Layer) {
        l.mode = if ["Line", "Fill", "Image"].contains(&self.mode.as_str()) { self.mode.clone() } else { "Line".into() };
        l.speed = self.speed.max(0.1);
        l.power = self.power.clamp(0.0, 100.0);
        l.min_power = self.min_power.clamp(0.0, 100.0);
        l.passes = self.passes.max(1);
        l.interval = self.interval.max(0.01);
        l.bidir = self.bidir;
        l.cross_hatch = self.cross_hatch;
        l.overscan = self.overscan.max(0.0);
        l.air = self.air;
        l.dither = self.dither.clone();
        l.invert = self.invert;
    }
    pub fn summary(&self) -> String {
        let mut s = format!("{} · {} mm/s · {}% · {} pass{}", self.mode, trim(self.speed), trim(self.power), self.passes, if self.passes == 1 { "" } else { "es" });
        if self.mode != "Line" {
            s += &format!(" · {} mm interval", trim(self.interval));
        }
        s
    }
}
fn trim(v: f64) -> String {
    crate::gcode::fmt(v, 2, false)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Material {
    /// e.g. "Birch plywood"
    pub name: String,
    /// mm (0 when it doesn't apply)
    pub thickness: f64,
    /// what the profile does, e.g. "Cut through" or "Engrave"
    pub operation: String,
    pub notes: String,
    pub profile: CutProfile,
}
impl Default for Material {
    fn default() -> Self {
        Material { name: "New material".into(), thickness: 3.0, operation: "Cut".into(), notes: String::new(), profile: CutProfile::default() }
    }
}
impl Material {
    pub fn title(&self) -> String {
        let mut t = self.name.clone();
        if self.thickness > 0.0 {
            t += &format!(" {} mm", trim(self.thickness));
        }
        if !self.operation.is_empty() {
            t += &format!(" — {}", self.operation);
        }
        t
    }
    fn same(&self, o: &Material) -> bool {
        self.name.eq_ignore_ascii_case(&o.name) && (self.thickness - o.thickness).abs() < 1e-6 && self.operation.eq_ignore_ascii_case(&o.operation)
    }
}

#[derive(Serialize, Deserialize)]
struct LibraryFile {
    #[serde(default)]
    app: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    version: u32,
    materials: Vec<Material>,
}

pub struct Library {
    pub materials: Vec<Material>,
    path: PathBuf,
}

pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))).unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("cnc-anything");
    // carry over settings and libraries from when the app was called Burner
    let old = base.join("burner");
    if !dir.exists() && old.is_dir() {
        if std::fs::create_dir_all(&dir).is_ok() {
            if let Ok(rd) = std::fs::read_dir(&old) {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().replace("autosave.burner", "autosave.cnca");
                    let _ = std::fs::copy(e.path(), dir.join(name));
                }
            }
        }
    }
    dir
}

fn starter() -> Vec<Material> {
    let p = |mode: &str, speed: f64, power: f64, passes: u32, interval: f64| CutProfile { mode: mode.into(), speed, power, passes, interval, ..CutProfile::default() };
    let note = "Starting point for a 5-10 W diode laser. Test on a scrap piece first.".to_string();
    vec![
        Material { name: "Plywood".into(), thickness: 3.0, operation: "Cut".into(), notes: note.clone(), profile: p("Line", 6.0, 100.0, 2, 0.1) },
        Material { name: "Plywood".into(), thickness: 3.0, operation: "Engrave".into(), notes: note.clone(), profile: p("Fill", 120.0, 35.0, 1, 0.1) },
        Material { name: "Plywood".into(), thickness: 3.0, operation: "Score".into(), notes: note.clone(), profile: p("Line", 40.0, 30.0, 1, 0.1) },
        Material { name: "Cardboard".into(), thickness: 2.0, operation: "Cut".into(), notes: note.clone(), profile: p("Line", 15.0, 80.0, 1, 0.1) },
        Material { name: "Leather".into(), thickness: 2.0, operation: "Engrave".into(), notes: note.clone(), profile: p("Fill", 150.0, 20.0, 1, 0.1) },
        Material { name: "Photo on wood".into(), thickness: 0.0, operation: "Picture".into(), notes: note, profile: CutProfile { dither: "jarvis".into(), ..p("Image", 150.0, 40.0, 1, 0.1) } },
    ]
}

impl Library {
    /// Load the library from the config folder (a starter set on first use)
    pub fn load() -> Library {
        let path = config_dir().join("materials.json");
        let materials = std::fs::read_to_string(&path).ok().and_then(|t| parse(&t).ok()).unwrap_or_else(starter);
        let lib = Library { materials, path };
        if !lib.path.exists() {
            let _ = lib.save();
        }
        lib
    }
    /// The saved materials without creating or changing any file
    pub fn load_readonly() -> Vec<Material> {
        std::fs::read_to_string(config_dir().join("materials.json")).ok().and_then(|t| parse(&t).ok()).unwrap_or_else(starter)
    }
    pub fn in_memory(materials: Vec<Material>) -> Library {
        Library { materials, path: PathBuf::new() }
    }
    pub fn save(&self) -> Result<(), String> {
        if self.path.as_os_str().is_empty() {
            return Ok(());
        }
        if let Some(d) = self.path.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(&self.path, self.export_json()).map_err(|e| e.to_string())
    }
    pub fn export_json(&self) -> String {
        serde_json::to_string_pretty(&LibraryFile { app: "cnc-anything".into(), kind: "material-library".into(), version: 1, materials: self.materials.clone() }).unwrap_or_default()
    }
    /// Add materials from JSON. One with the same name, thickness and operation is replaced.
    /// Returns (added, replaced).
    pub fn import_json(&mut self, text: &str) -> Result<(usize, usize), String> {
        let list = parse(text)?;
        let (mut added, mut replaced) = (0, 0);
        for m in list {
            if let Some(old) = self.materials.iter_mut().find(|o| o.same(&m)) {
                *old = m;
                replaced += 1;
            } else {
                self.materials.push(m);
                added += 1;
            }
        }
        self.sort();
        Ok((added, replaced))
    }
    pub fn sort(&mut self) {
        self.materials.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.thickness.partial_cmp(&b.thickness).unwrap_or(std::cmp::Ordering::Equal)).then(a.operation.cmp(&b.operation)));
    }
}

/// Accepts a library file ({"materials": [...]}) or a bare list of materials
pub fn parse(text: &str) -> Result<Vec<Material>, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("Not valid JSON: {e}"))?;
    let list = if v.is_array() { v } else { v.get("materials").cloned().ok_or("No \"materials\" list in that file")? };
    let mats: Vec<Material> = serde_json::from_value(list).map_err(|e| format!("Materials list not readable: {e}"))?;
    Ok(mats.into_iter().filter(|m| !m.name.trim().is_empty()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_merge() {
        let mut lib = Library::in_memory(starter());
        let n = lib.materials.len();
        let json = lib.export_json();
        let (a, r) = lib.import_json(&json).unwrap();
        assert_eq!((a, r), (0, n));
        let (a, _) = lib.import_json(r#"[{"name":"Acrylic","thickness":3,"operation":"Cut","profile":{"mode":"Line","speed":5,"power":90,"passes":3}}]"#).unwrap();
        assert_eq!(a, 1);
        let m = lib.materials.iter().find(|m| m.name == "Acrylic").unwrap();
        let mut l = Layer::new(4);
        m.profile.apply_to(&mut l);
        assert_eq!((l.speed, l.power, l.passes, l.mode.as_str()), (5.0, 90.0, 3, "Line"));
        assert_eq!(l.interval, 0.1); // missing fields fall back to defaults
    }
}
