//! Router libraries: cutting tools, and speeds & feeds (per material and tool).
//! Both are kept in the config folder and can be exported/imported as JSON.

use crate::gcode::fmt;
use crate::materials::config_dir;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const TOOL_KINDS: [&str; 4] = ["Flat end mill", "Ball end mill", "V-bit", "Drill"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Tool {
    /// tool number (T1, T2...)
    pub id: u32,
    pub name: String,
    pub kind: String,
    /// cutting diameter, mm
    pub diameter: f64,
    pub flutes: u32,
    /// longest cut the flutes allow, mm
    pub flute_length: f64,
    /// included angle for V-bits, degrees
    pub angle: f64,
    pub notes: String,
}
impl Default for Tool {
    fn default() -> Self {
        Tool { id: 0, name: "New tool".into(), kind: "Flat end mill".into(), diameter: 6.0, flutes: 2, flute_length: 20.0, angle: 90.0, notes: String::new() }
    }
}
impl Tool {
    pub fn title(&self) -> String {
        format!("T{} · {}", self.id, self.name)
    }
    pub fn summary(&self) -> String {
        let mut s = format!("{} · Ø{} mm · {} flute{}", self.kind, fmt(self.diameter, 3, false), self.flutes, if self.flutes == 1 { "" } else { "s" });
        if self.kind == "V-bit" {
            s += &format!(" · {}°", fmt(self.angle, 1, false));
        }
        s
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct FeedEntry {
    pub material: String,
    pub tool_id: u32,
    /// the tool's name when saved, to find it again after importing into another library
    pub tool_name: String,
    pub rpm: f64,
    /// mm per tooth
    pub chipload: f64,
    /// mm/min
    pub plunge: f64,
    /// depth per pass, mm
    pub stepdown: f64,
    /// % of the tool diameter between pocketing rings
    pub stepover: f64,
    pub notes: String,
}
impl Default for FeedEntry {
    fn default() -> Self {
        FeedEntry { material: "New material".into(), tool_id: 0, tool_name: String::new(), rpm: 18000.0, chipload: 0.05, plunge: 500.0, stepdown: 2.0, stepover: 40.0, notes: String::new() }
    }
}
impl FeedEntry {
    /// feed rate in mm/min: RPM x flutes x chip load
    pub fn feed(&self, tool: Option<&Tool>) -> f64 {
        self.rpm * tool.map(|t| t.flutes.max(1)).unwrap_or(2) as f64 * self.chipload
    }
}

#[derive(Serialize, Deserialize)]
struct ToolsFile {
    #[serde(default)]
    app: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    version: u32,
    tools: Vec<Tool>,
}
#[derive(Serialize, Deserialize)]
struct FeedsFile {
    #[serde(default)]
    app: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    version: u32,
    feeds: Vec<FeedEntry>,
}

pub struct Libraries {
    pub tools: Vec<Tool>,
    pub feeds: Vec<FeedEntry>,
    dir: Option<PathBuf>,
}

fn starter_tools() -> Vec<Tool> {
    let t = |id, name: &str, kind: &str, d, flutes, fl, angle, notes: &str| Tool { id, name: name.into(), kind: kind.into(), diameter: d, flutes, flute_length: fl, angle, notes: notes.into() };
    vec![
        t(1, "1/8\" flat end mill", "Flat end mill", 3.175, 2, 12.0, 0.0, "Details and small parts"),
        t(2, "1/4\" flat end mill", "Flat end mill", 6.35, 2, 22.0, 0.0, "General cutting and pockets"),
        t(3, "3 mm single-flute (O-flute)", "Flat end mill", 3.0, 1, 12.0, 0.0, "Plastics and aluminum"),
        t(4, "6 mm 3-flute end mill", "Flat end mill", 6.0, 3, 20.0, 0.0, "Hardwood and faster clearing"),
        t(5, "1/8\" ball end mill", "Ball end mill", 3.175, 2, 12.0, 0.0, "Rounded grooves"),
        t(6, "1/4\" 90° V-bit", "V-bit", 6.35, 2, 3.2, 90.0, "Chamfers and lettering"),
    ]
}

fn starter_feeds(tools: &[Tool]) -> Vec<FeedEntry> {
    // starting points for a hobby router; test on scrap first
    let mats: [(&str, f64, f64, f64, f64); 6] = [
        // material, chip load per mm of tool diameter, rpm, depth per pass as a share of the diameter, stepover %
        ("Softwood", 0.016, 18000.0, 0.5, 40.0),
        ("Hardwood", 0.012, 18000.0, 0.4, 40.0),
        ("Plywood", 0.013, 18000.0, 0.5, 40.0),
        ("MDF", 0.018, 18000.0, 0.5, 45.0),
        ("Acrylic", 0.012, 16000.0, 0.35, 35.0),
        ("Aluminum 6061", 0.0025, 18000.0, 0.1, 30.0),
    ];
    let mut out = vec![];
    for (m, cl, rpm, sd, so) in mats {
        for t in tools.iter().filter(|t| t.kind != "V-bit" && t.kind != "Drill") {
            // single-flute tools suit plastics and aluminum; multi-flute suit wood
            let plastic_or_metal = m == "Acrylic" || m.starts_with("Aluminum");
            if plastic_or_metal != (t.flutes == 1) {
                continue;
            }
            let chipload = (cl * t.diameter * 1000.0).round() / 1000.0;
            let feed = rpm * t.flutes as f64 * chipload;
            out.push(FeedEntry {
                material: m.into(),
                tool_id: t.id,
                tool_name: t.name.clone(),
                rpm,
                chipload,
                plunge: (feed / 3.0 / 10.0).round() * 10.0,
                stepdown: (sd * t.diameter * 10.0).round() / 10.0,
                stepover: so,
                notes: "Starting point, estimated. Test on scrap first.".into(),
            });
        }
    }
    // V-bit for lettering in wood
    if let Some(v) = tools.iter().find(|t| t.kind == "V-bit") {
        for m in ["Softwood", "Hardwood", "Plywood", "MDF"] {
            out.push(FeedEntry { material: m.into(), tool_id: v.id, tool_name: v.name.clone(), rpm: 18000.0, chipload: 0.04, plunge: 400.0, stepdown: 1.5, stepover: 40.0, notes: "Starting point, estimated.".into() });
        }
    }
    out
}

pub fn parse_tools(text: &str) -> Result<Vec<Tool>, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("Not valid JSON: {e}"))?;
    let list = if v.is_array() { v } else { v.get("tools").cloned().ok_or("No \"tools\" list in that file")? };
    serde_json::from_value(list).map_err(|e| format!("Tool list not readable: {e}"))
}
pub fn parse_feeds(text: &str) -> Result<Vec<FeedEntry>, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("Not valid JSON: {e}"))?;
    let list = if v.is_array() { v } else { v.get("feeds").cloned().ok_or("No \"feeds\" list in that file")? };
    serde_json::from_value(list).map_err(|e| format!("Speeds & feeds list not readable: {e}"))
}

impl Libraries {
    pub fn load(write: bool) -> Libraries {
        let dir = config_dir();
        let tools = std::fs::read_to_string(dir.join("tools.json")).ok().and_then(|t| parse_tools(&t).ok()).unwrap_or_else(starter_tools);
        let feeds = std::fs::read_to_string(dir.join("feeds.json")).ok().and_then(|t| parse_feeds(&t).ok()).unwrap_or_else(|| starter_feeds(&tools));
        let lib = Libraries { tools, feeds, dir: if write { Some(dir) } else { None } };
        if write && (!lib.dir.as_ref().unwrap().join("tools.json").exists() || !lib.dir.as_ref().unwrap().join("feeds.json").exists()) {
            let _ = lib.save();
        }
        lib
    }
    #[allow(dead_code)]
    pub fn in_memory() -> Libraries {
        let tools = starter_tools();
        let feeds = starter_feeds(&tools);
        Libraries { tools, feeds, dir: None }
    }
    pub fn save(&self) -> Result<(), String> {
        let Some(d) = &self.dir else { return Ok(()) };
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        std::fs::write(d.join("tools.json"), self.export_tools()).map_err(|e| e.to_string())?;
        std::fs::write(d.join("feeds.json"), self.export_feeds()).map_err(|e| e.to_string())
    }
    pub fn tool(&self, id: u32) -> Option<&Tool> {
        self.tools.iter().find(|t| t.id == id)
    }
    pub fn next_tool_id(&self) -> u32 {
        self.tools.iter().map(|t| t.id).max().unwrap_or(0) + 1
    }
    pub fn feeds_for(&self, tool_id: u32) -> Vec<(usize, &FeedEntry)> {
        self.feeds.iter().enumerate().filter(|(_, f)| f.tool_id == tool_id).collect()
    }
    pub fn export_tools(&self) -> String {
        serde_json::to_string_pretty(&ToolsFile { app: "cnc-anything".into(), kind: "tool-library".into(), version: 1, tools: self.tools.clone() }).unwrap_or_default()
    }
    pub fn export_feeds(&self) -> String {
        let feeds = self.feeds.iter().map(|f| FeedEntry { tool_name: self.tool(f.tool_id).map(|t| t.name.clone()).unwrap_or(f.tool_name.clone()), ..f.clone() }).collect();
        serde_json::to_string_pretty(&FeedsFile { app: "cnc-anything".into(), kind: "speeds-and-feeds".into(), version: 1, feeds }).unwrap_or_default()
    }
    /// Tools with the same number are replaced; returns (added, replaced)
    pub fn import_tools(&mut self, text: &str) -> Result<(usize, usize), String> {
        let list = parse_tools(text)?;
        let (mut a, mut r) = (0, 0);
        for mut t in list {
            if t.diameter <= 0.0 {
                continue;
            }
            if t.id == 0 {
                t.id = self.next_tool_id();
            }
            if let Some(old) = self.tools.iter_mut().find(|o| o.id == t.id) {
                *old = t;
                r += 1;
            } else {
                self.tools.push(t);
                a += 1;
            }
        }
        self.tools.sort_by_key(|t| t.id);
        Ok((a, r))
    }
    /// Entries for the same material and tool are replaced; a tool is found by number, or by name
    /// when the number belongs to a different tool here. Returns (added, replaced).
    pub fn import_feeds(&mut self, text: &str) -> Result<(usize, usize), String> {
        let list = parse_feeds(text)?;
        let (mut a, mut r) = (0, 0);
        for mut f in list {
            let by_id_ok = self.tool(f.tool_id).map(|t| f.tool_name.is_empty() || t.name == f.tool_name).unwrap_or(false);
            if !by_id_ok {
                if let Some(t) = self.tools.iter().find(|t| !f.tool_name.is_empty() && t.name == f.tool_name) {
                    f.tool_id = t.id;
                }
            }
            if let Some(old) = self.feeds.iter_mut().find(|o| o.tool_id == f.tool_id && o.material.eq_ignore_ascii_case(&f.material)) {
                *old = f;
                r += 1;
            } else {
                self.feeds.push(f);
                a += 1;
            }
        }
        Ok((a, r))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn libraries_round_trip() {
        let mut lib = Libraries::in_memory();
        assert!(lib.tools.len() >= 5 && !lib.feeds.is_empty());
        let n = lib.feeds.len();
        let (a, r) = lib.import_feeds(&lib.export_feeds()).unwrap();
        assert_eq!((a, r), (0, n));
        let (a, r) = lib.import_tools(r#"[{"id":7,"name":"2 mm drill","kind":"Drill","diameter":2,"flutes":2}]"#).unwrap();
        assert_eq!((a, r), (1, 0));
        let f = &lib.feeds[0];
        let t = lib.tool(f.tool_id).unwrap();
        assert!((f.feed(Some(t)) - f.rpm * t.flutes as f64 * f.chipload).abs() < 1e-9);
    }
}
