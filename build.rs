//! Build script that locates the espeak-ng-data directory built by espeak-rs-sys
//! and copies it into vox's OUT_DIR so it can be embedded via `include_dir!`.
//!
//! espeak-ng bakes a hard-coded data path into the compiled library (the path on
//! the build machine). On end-user machines that path does not exist, which
//! breaks piper TTS with "No such file or directory". To avoid that, we ship
//! the data inside the binary and extract it at first use, then point espeak-rs
//! at it via the `PIPER_ESPEAKNG_DATA_DIRECTORY` env var.

use std::env;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR not set"));

    // OUT_DIR layout: <target>/<profile>/build/<crate>-<hash>/out
    // Sibling crates' build artifacts live in <target>/<profile>/build/
    let build_dir = out_dir
        .ancestors()
        .nth(2)
        .expect("OUT_DIR has unexpected layout");

    let espeak_data = wait_for_espeak_data(build_dir).unwrap_or_else(|| {
        panic!(
            "espeak-ng-data not found under {}. \
             espeak-rs-sys should have built it before this script runs \
             (with -j1 cargo cannot run both build scripts at once: re-run the build).",
            build_dir.display()
        )
    });

    let dst = out_dir.join("espeak-ng-data");
    copy_dir_recursive(&espeak_data, &dst).expect("failed to stage espeak-ng-data");

    println!("cargo:rerun-if-changed={}", espeak_data.display());
}

/// Cargo gives no ordering guarantee between vox's build script and the
/// espeak-rs-sys one (that crate has no `links` key), so the data directory may
/// not exist yet when we start. Poll for it: espeak-rs-sys is a dependency of
/// this crate, so its build script is guaranteed to run during the same cargo
/// invocation. With a single job slot it cannot run concurrently, so bail out
/// immediately in that case.
fn wait_for_espeak_data(build_dir: &Path) -> Option<PathBuf> {
    const TIMEOUT: Duration = Duration::from_secs(20 * 60);
    const POLL: Duration = Duration::from_secs(2);

    if let Some(p) = find_espeak_data(build_dir) {
        return Some(p);
    }
    let jobs: usize = env::var("NUM_JOBS")
        .ok()
        .and_then(|j| j.parse().ok())
        .unwrap_or(2);
    if jobs < 2 {
        return None;
    }
    println!("cargo:warning=espeak-ng-data not built yet, waiting for espeak-rs-sys...");
    let start = Instant::now();
    while start.elapsed() < TIMEOUT {
        std::thread::sleep(POLL);
        if let Some(p) = find_espeak_data(build_dir) {
            // Give the producer a moment to finish copying the tree.
            std::thread::sleep(POLL);
            return Some(p);
        }
    }
    None
}

fn find_espeak_data(build_dir: &Path) -> Option<PathBuf> {
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(build_dir).ok()?.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("espeak-rs-sys-") {
            continue;
        }
        let candidate = entry
            .path()
            .join("out")
            .join("share")
            .join("espeak-ng-data");
        if !candidate.is_dir() {
            continue;
        }
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        match &newest {
            Some((t, _)) if *t >= mtime => {}
            _ => newest = Some((mtime, candidate)),
        }
    }
    newest.map(|(_, p)| p)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if dst.exists() {
        std::fs::remove_dir_all(dst)?;
    }
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_recursive(&path, &target)?;
        } else if file_type.is_symlink() {
            // Resolve symlinks so include_dir! sees real files.
            let resolved = std::fs::read_link(&path)?;
            let resolved = if resolved.is_absolute() {
                resolved
            } else {
                path.parent().unwrap_or(Path::new("")).join(resolved)
            };
            if resolved.is_dir() {
                copy_dir_recursive(&resolved, &target)?;
            } else {
                std::fs::copy(&resolved, &target)?;
            }
        } else {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}
