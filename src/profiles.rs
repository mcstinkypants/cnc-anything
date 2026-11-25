//! Machine profiles: several named machines (laser cutters and routers), each with its own bed,
//! controller, port and router settings. Kept in the config folder; no machine needs to be connected.

use crate::gcode::Device;
use crate::materials::config_dir;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
struct ProfilesFile {
    #[serde(default)]
    app: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    version: u32,
    /// name of the machine in use
    #[serde(default)]
    active: String,
    machines: Vec<Device>,
}

pub struct Profiles {
    pub list: Vec<Device>,
    pub active: usize,
    path: Option<PathBuf>,
}

pub fn parse(text: &str) -> Result<(Vec<Device>, String), String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("Not valid JSON: {e}"))?;
    if v.is_array() {
        return Ok((serde_json::from_value(v).map_err(|e| format!("Machine list not readable: {e}"))?, String::new()));
    }
    if v.get("machines").is_some() {
        let f: ProfilesFile = serde_json::from_value(v).map_err(|e| format!("Machine list not readable: {e}"))?;
        return Ok((f.machines, f.active));
    }
    // a single machine
    let d: Device = serde_json::from_value(v).map_err(|e| format!("Machine not readable: {e}"))?;
    Ok((vec![d], String::new()))
}

impl Profiles {
    /// Load the profiles; the first time, start from the machine set up so far plus an example router
    pub fn load(write: bool, current: &Device) -> Profiles {
        let dir = config_dir();
        let path = dir.join("machines.json");
        let loaded = std::fs::read_to_string(&path).ok().and_then(|t| parse(&t).ok());
        let mut p = match loaded {
            Some((list, active)) if !list.is_empty() => {
                let a = list.iter().position(|d| d.name == active).unwrap_or(0);
                Profiles { list, active: a, path: None }
            }
            _ => {
                let mut first = current.clone();
                if first.name.trim().is_empty() || first.name == "My laser" && first.router() {
                    first.name = if first.router() { "My router".into() } else { "My laser".into() };
                }
                let mut other = Device { name: if first.router() { "My laser".into() } else { "My router".into() }, ..Default::default() };
                other.machine = if first.router() { "laser".into() } else { "router".into() };
                if other.router() {
                    other.bed_w = 600.0;
                    other.bed_h = 400.0;
                    other.s_max = 24000.0;
                    other.origin_top = false;
                }
                Profiles { list: vec![first, other], active: 0, path: None }
            }
        };
        p.dedupe_names();
        if write {
            p.path = Some(path);
            let _ = p.save();
        }
        p
    }
    fn dedupe_names(&mut self) {
        for i in 0..self.list.len() {
            if self.list[i].name.trim().is_empty() {
                self.list[i].name = "Machine".into();
            }
            let base = self.list[i].name.clone();
            let mut k = 2;
            while self.list[..i].iter().any(|d| d.name.eq_ignore_ascii_case(&self.list[i].name)) {
                self.list[i].name = format!("{base} ({k})");
                k += 1;
            }
        }
    }
    pub fn unique_name(&self, base: &str) -> String {
        let mut name = base.to_string();
        let mut k = 2;
        while self.list.iter().any(|d| d.name.eq_ignore_ascii_case(&name)) {
            name = format!("{base} ({k})");
            k += 1;
        }
        name
    }
    pub fn save(&self) -> Result<(), String> {
        let Some(p) = &self.path else { return Ok(()) };
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(p, self.export_json()).map_err(|e| e.to_string())
    }
    pub fn export_json(&self) -> String {
        let f = ProfilesFile { app: "cnc-anything".into(), kind: "machine-profiles".into(), version: 1, active: self.list.get(self.active).map(|d| d.name.clone()).unwrap_or_default(), machines: self.list.clone() };
        serde_json::to_string_pretty(&f).unwrap_or_default()
    }
    /// Machines with the same name are replaced; returns (added, replaced)
    pub fn import_json(&mut self, text: &str) -> Result<(usize, usize), String> {
        let (list, _) = parse(text)?;
        let (mut a, mut r) = (0, 0);
        for d in list {
            if let Some(old) = self.list.iter_mut().find(|o| o.name.eq_ignore_ascii_case(&d.name)) {
                *old = d;
                r += 1;
            } else {
                self.list.push(d);
                a += 1;
            }
        }
        self.dedupe_names();
        Ok((a, r))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiles_round_trip() {
        let mut p = Profiles::load(false, &Device::default());
        assert_eq!(p.list.len(), 2);
        assert!(p.list.iter().any(|d| d.router()) && p.list.iter().any(|d| !d.router()));
        let json = p.export_json();
        assert_eq!(p.import_json(&json).unwrap(), (0, 2));
        let one = serde_json::to_string(&Device { name: "Shop router".into(), machine: "router".into(), ..Default::default() }).unwrap();
        assert_eq!(p.import_json(&one).unwrap(), (1, 0));
        assert_eq!(p.unique_name("Shop router"), "Shop router (2)");
    }
}
