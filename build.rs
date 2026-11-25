//! Build step: stamp the git commit id into the program (shown in the title bar).
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    let commit = git(&["rev-parse", "--short=7", "HEAD"]).unwrap_or_else(|| "unknown".into());
    // a build made from uncommitted changes says so
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).map(|s| !s.is_empty()).unwrap_or(false);
    let id = if dirty && commit != "unknown" { format!("{commit}-modified") } else { commit };
    println!("cargo:rustc-env=GIT_COMMIT={id}");
    // rebuild the stamp when the commit (or the working tree) changes
    for p in [".git/HEAD", ".git/index", ".git/refs/heads", ".git/packed-refs"] {
        if std::path::Path::new(p).exists() {
            println!("cargo:rerun-if-changed={p}");
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}
