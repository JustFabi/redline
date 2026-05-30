use std::path::{Path, PathBuf};

/// Resolve Syzygy tablebase directory (relative paths avoid Windows drive-letter issues in Pyrrhic).
pub fn resolve_syzygy_path(configured: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = configured {
        if !p.is_empty() {
            let path = PathBuf::from(p);
            if path.is_dir() {
                return Some(path);
            }
        }
    }

    if let Ok(p) = std::env::var("REDLINE_SYZYGY") {
        let path = PathBuf::from(&p);
        if path.is_dir() {
            return Some(path);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let exe_syzygy = dir.join("syzygy");
            if exe_syzygy.is_dir() {
                return Some(exe_syzygy);
            }
        }
    }

    for c in [PathBuf::from("syzygy"), PathBuf::from("./syzygy")] {
        if c.is_dir() {
            return Some(c);
        }
    }

    None
}

pub fn syzygy_path_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

