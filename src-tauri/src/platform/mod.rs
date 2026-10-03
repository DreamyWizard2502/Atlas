//! OS abstraction. Everything that differs between operating systems lives behind
//! these functions so a macOS implementation can be added later without touching
//! the indexer, cleanup engine or commands.

use std::path::{Path, PathBuf};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use self::windows as imp;

#[cfg(not(windows))]
mod portable;
#[cfg(not(windows))]
use self::portable as imp;

/// Attribute bits stored in `files.attrs`. Kept platform-neutral.
pub const ATTR_HIDDEN: u32 = 0x2;
pub const ATTR_SYSTEM: u32 = 0x4;
/// Cloud placeholder (e.g. OneDrive "online-only"): reading the content would
/// trigger a download, so we never hash these.
pub const ATTR_CLOUD: u32 = 0x8;

#[derive(Debug, Clone, serde::Serialize)]
pub struct DiskSpace {
    pub mount: String,
    pub total: u64,
    pub free: u64,
}

/// Neutral attribute bits for a file's metadata.
pub fn attrs(meta: &std::fs::Metadata) -> u32 {
    imp::attrs(meta)
}

/// True for files the OS marks as system files (or hidden + system).
pub fn is_system(attrs: u32) -> bool {
    attrs & ATTR_SYSTEM != 0
}

/// Total/free bytes for the volume containing `path`.
pub fn disk_space(path: &Path) -> Option<DiskSpace> {
    imp::disk_space(path)
}

/// Root of the volume containing `path` (e.g. `C:\`).
pub fn volume_root(path: &Path) -> Option<PathBuf> {
    imp::volume_root(path)
}

/// Whether another process holds the file open in a way that prevents moving it.
pub fn is_locked(path: &Path) -> bool {
    imp::is_locked(path)
}

/// Directories that must never be touched by cleanup (OS and program installs).
pub fn system_roots() -> Vec<PathBuf> {
    imp::system_roots()
}

/// The OS temp directory under the system root that is nonetheless safe to clean.
pub fn system_temp_dir() -> Option<PathBuf> {
    imp::system_temp_dir()
}

/// Display names of installed applications (used to guess whether an installer
/// sitting in Downloads has already been run).
pub fn installed_app_names() -> Vec<String> {
    imp::installed_app_names()
}

/// Drives / volume roots available for browsing.
pub fn list_roots() -> Vec<PathBuf> {
    imp::list_roots()
}

/// Expand 8.3 short components (`PROGRA~1`) to their long names. Change
/// notifications can report either form; the index only ever stores long ones.
/// Paths without a `~` are returned unchanged (no syscall).
pub fn long_path(path: &Path) -> PathBuf {
    if !path.components().any(|c| c.as_os_str().to_string_lossy().contains('~')) {
        return path.to_path_buf();
    }
    imp::long_path(path)
}

/// The 8.3 short form of an existing path, if the volume has one.
pub fn short_path(path: &Path) -> Option<PathBuf> {
    imp::short_path(path)
}

/// Case-insensitive file systems compare paths case-insensitively.
pub fn paths_case_insensitive() -> bool {
    cfg!(windows) || cfg!(target_os = "macos")
}

/// Normalised form for prefix comparisons.
pub fn norm(p: &Path) -> String {
    let s = p.to_string_lossy().replace('/', std::path::MAIN_SEPARATOR_STR);
    let s = s.trim_end_matches(std::path::MAIN_SEPARATOR).to_string();
    if paths_case_insensitive() {
        s.to_lowercase()
    } else {
        s
    }
}

/// True if `path` equals `dir` or lies inside it.
pub fn is_within(path: &Path, dir: &Path) -> bool {
    let p = norm(path);
    let d = norm(dir);
    if d.is_empty() {
        return false;
    }
    p == d || (p.starts_with(&d) && p[d.len()..].starts_with(std::path::MAIN_SEPARATOR))
}

pub fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

pub fn downloads_dir() -> Option<PathBuf> {
    dirs::download_dir()
}

pub fn desktop_dir() -> Option<PathBuf> {
    dirs::desktop_dir()
}

/// Per-user roaming application data (Windows: `%APPDATA%`).
pub fn roaming_dir() -> Option<PathBuf> {
    dirs::data_dir()
}

/// Per-user local application data (Windows: `%LOCALAPPDATA%`).
pub fn local_data_dir() -> Option<PathBuf> {
    dirs::data_local_dir()
}

pub fn temp_dir() -> PathBuf {
    std::env::temp_dir()
}

/// Well-known user folders that cleanup must never remove even when empty.
pub fn protected_user_folders() -> Vec<PathBuf> {
    [
        dirs::home_dir(),
        dirs::desktop_dir(),
        dirs::document_dir(),
        dirs::download_dir(),
        dirs::picture_dir(),
        dirs::audio_dir(),
        dirs::video_dir(),
        dirs::public_dir(),
        dirs::template_dir(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Known browser and app cache directories. Patterns may contain a single `*`
/// segment that matches any profile directory.
pub fn known_cache_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let local = local_data_dir();
    let roaming = roaming_dir();
    let mut push_glob = |base: &Option<PathBuf>, rel: &str| {
        let Some(base) = base else { return };
        let parts: Vec<&str> = rel.split('/').collect();
        let mut current = vec![base.clone()];
        for part in parts {
            let mut next = Vec::new();
            for c in &current {
                if part == "*" {
                    if let Ok(rd) = std::fs::read_dir(c) {
                        for e in rd.flatten() {
                            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                                next.push(e.path());
                            }
                        }
                    }
                } else {
                    next.push(c.join(part));
                }
            }
            current = next;
        }
        out.extend(current.into_iter().filter(|p| p.is_dir()));
    };
    for rel in [
        "Google/Chrome/User Data/*/Cache",
        "Google/Chrome/User Data/*/Code Cache",
        "Google/Chrome/User Data/*/GPUCache",
        "Microsoft/Edge/User Data/*/Cache",
        "Microsoft/Edge/User Data/*/Code Cache",
        "Microsoft/Edge/User Data/*/GPUCache",
        "BraveSoftware/Brave-Browser/User Data/*/Cache",
        "BraveSoftware/Brave-Browser/User Data/*/Code Cache",
        "Mozilla/Firefox/Profiles/*/cache2",
        "Microsoft/Windows/INetCache",
        "CrashDumps",
    ] {
        push_glob(&local, rel);
    }
    for rel in roaming_cache_rel() {
        push_glob(&roaming, rel);
    }
    out
}

/// Cache dirs under the roaming profile that are exempt from the Roaming block.
pub fn roaming_cache_rel() -> &'static [&'static str] {
    &[
        "Code/Cache",
        "Code/CachedData",
        "Code/Code Cache",
        "Code/GPUCache",
        "Slack/Cache",
        "Slack/Code Cache",
        "discord/Cache",
        "discord/Code Cache",
        "Microsoft/Teams/Cache",
        "Microsoft/Teams/Code Cache",
    ]
}
