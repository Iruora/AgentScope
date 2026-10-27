//! Locates the bundled `agentscope-hook`. Tauri installs sidecars next to the
//! app executable with the target triple stripped from the name.

use std::path::{Path, PathBuf};

const SHIM_NAME: &str = "agentscope-hook";

fn file_name() -> String {
    format!("{SHIM_NAME}{}", std::env::consts::EXE_SUFFIX)
}

fn usable(path: &Path) -> bool {
    // Builds without the sidecar step contain an empty placeholder (see `build.rs`).
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() > 0)
}

/// The shim path to write into harness configs, if a usable binary exists.
pub fn path(data_dir: &Path) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let bundled = exe.parent()?.join(file_name());
    if !usable(&bundled) {
        return None;
    }
    // An AppImage is mounted at a new path on every launch, so hooks must
    // point at a stable copy instead.
    if std::env::var_os("APPIMAGE").is_some() {
        return stable_copy(&bundled, data_dir).ok();
    }
    Some(bundled)
}

fn stable_copy(bundled: &Path, data_dir: &Path) -> std::io::Result<PathBuf> {
    let dir = data_dir.join("bin");
    std::fs::create_dir_all(&dir)?;
    let copy = dir.join(file_name());
    let same = std::fs::read(&copy).ok() == Some(std::fs::read(bundled)?);
    if !same {
        let tmp = dir.join(format!("{}.tmp", file_name()));
        std::fs::copy(bundled, &tmp)?;
        std::fs::rename(&tmp, &copy)?;
    }
    Ok(copy)
}
