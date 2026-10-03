//! Fallback implementation for non-Windows targets. Functional but conservative;
//! a proper macOS backend would replace pieces of this.

use super::{DiskSpace, ATTR_HIDDEN};
use std::path::{Path, PathBuf};

pub fn attrs(_meta: &std::fs::Metadata) -> u32 {
    0
}

#[allow(dead_code)]
pub fn hidden_by_name(name: &str) -> u32 {
    if name.starts_with('.') {
        ATTR_HIDDEN
    } else {
        0
    }
}

pub fn volume_root(_path: &Path) -> Option<PathBuf> {
    Some(PathBuf::from("/"))
}

pub fn disk_space(_path: &Path) -> Option<DiskSpace> {
    None
}

pub fn is_locked(_path: &Path) -> bool {
    false
}

pub fn system_roots() -> Vec<PathBuf> {
    ["/System", "/Library", "/Applications", "/usr", "/bin", "/sbin", "/etc", "/var", "/private"]
        .iter()
        .map(PathBuf::from)
        .collect()
}

pub fn system_temp_dir() -> Option<PathBuf> {
    None
}

pub fn installed_app_names() -> Vec<String> {
    Vec::new()
}

pub fn long_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}

pub fn short_path(_path: &Path) -> Option<PathBuf> {
    None
}

pub fn list_roots() -> Vec<PathBuf> {
    vec![PathBuf::from("/")]
}
