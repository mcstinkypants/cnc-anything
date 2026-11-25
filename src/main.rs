//! CNC Anything: design jobs and send them to GRBL laser cutters and CNC routers.

mod app;
mod app_cmd;
mod app_dialogs;
mod app_libs;
mod app_nodes;
mod app_profiles;
mod app_ui;
mod cam;
mod canvas;
mod canvas_nodes;
mod dialogs;
mod gcode;
mod geom;
mod importers;
mod machine;
mod materials;
mod model;
mod profiles;
mod spline;
mod tools;

use std::path::PathBuf;

fn usage() -> ! {
    println!("CNC Anything {}\n\nUsage: cnc-anything [--screenshot FILE.png] [--write-icon FILE.png] [PROJECT.cnca]", env!("CARGO_PKG_VERSION"));
    std::process::exit(0);
}

/// The app icon (an end mill cutting a glowing groove), drawn in code so nothing is copied
fn write_icon(path: &PathBuf) -> Result<(), String> {
    let n = 256u32;
    let mut img = image::RgbaImage::new(n, n);
    for (x, y, p) in img.enumerate_pixels_mut() {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        let r = 44.0;
        let dx = (fx - 128.0).abs() - (128.0 - r - 8.0);
        let dy = (fy - 128.0).abs() - (128.0 - r - 8.0);
        let d = dx.max(0.0).hypot(dy.max(0.0)) - r;
        if d > 0.5 {
            *p = image::Rgba([0, 0, 0, 0]);
            continue;
        }
        let mut c = [0x26 as f32, 0x28 as f32, 0x2c as f32];
        // glowing groove across the bottom
        let groove = ((fy - 186.0).abs() - 7.0).max(0.0);
        let glow = (1.0 - groove / 26.0).clamp(0.0, 1.0).powf(2.0) * if fx > 128.0 { 0.35 } else { 1.0 };
        for (k, v) in [255.0f32, 120.0, 30.0].iter().enumerate() {
            c[k] = c[k] * (1.0 - glow) + v * glow;
        }
        // shank and flutes of the end mill
        let half = 15.0;
        if (fx - 128.0).abs() < half + 7.0 && fy > 34.0 && fy < 96.0 {
            c = [150.0, 156.0, 166.0];
        } else if (fx - 128.0).abs() < half && fy >= 96.0 && fy < 190.0 {
            let stripe = ((fx - 128.0) + (fy * 0.9)).rem_euclid(22.0) < 9.0;
            c = if stripe { [200.0, 205.0, 214.0] } else { [110.0, 116.0, 128.0] };
        }
        let a = (0.5 - d).clamp(0.0, 1.0) * 255.0;
        *p = image::Rgba([c[0] as u8, c[1] as u8, c[2] as u8, a as u8]);
    }
    img.save(path).map_err(|e| e.to_string())
}

fn main() {
    let mut shot: Option<PathBuf> = None;
    let mut open: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => shot = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            "--write-icon" => {
                let p = PathBuf::from(args.next().unwrap_or_else(|| usage()));
                match write_icon(&p) {
                    Ok(()) => std::process::exit(0),
                    Err(e) => {
                        eprintln!("error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            "-h" | "--help" => usage(),
            "--version" | "-V" => {
                println!("CNC Anything {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            s if s.starts_with("--") => {} // ignore options added by launchers
            s => open = Some(PathBuf::from(s)),
        }
    }
    let mut viewport = egui::ViewportBuilder::default().with_title("CNC Anything").with_inner_size([1440.0, 900.0]).with_min_inner_size([960.0, 600.0]).with_app_id("cnc-anything");
    if let Some(icon) = icon_data() {
        viewport = viewport.with_icon(icon);
    }
    if shot.is_some() {
        // a check takes the picture: don't steal focus from what the person is doing
        viewport = viewport.with_active(false);
        #[cfg(target_os = "windows")]
        {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
    let options = eframe::NativeOptions { viewport, run_and_return: true, ..Default::default() };
    let res = eframe::run_native(
        "CNC Anything",
        options,
        Box::new(move |cc| {
            let mut a = app::App::new(cc, shot);
            if let Some(p) = open {
                a.open_on_start(p);
            }
            Ok(Box::new(a))
        }),
    );
    if let Err(e) = res {
        eprintln!("error: could not open the window: {e}");
        std::process::exit(1);
    }
}

fn icon_data() -> Option<egui::IconData> {
    let tmp = std::env::temp_dir().join(format!("cnc-anything-icon-{}.png", std::process::id()));
    write_icon(&tmp).ok()?;
    let img = image::open(&tmp).ok()?.to_rgba8();
    let _ = std::fs::remove_file(&tmp);
    Some(egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() })
}
