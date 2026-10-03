use crate::cleanup::rules::{default_junk_rules, JunkRule};
use crate::db::{kv_get, kv_set};
use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Folders/drives to index.
    pub roots: Vec<String>,
    /// Absolute paths, or bare folder names (matched anywhere), to skip.
    pub excluded: Vec<String>,
    pub inbox_days: u32,
    pub cold_days: u32,
    pub cold_min_mb: u64,
    pub retention_days: u32,
    pub dup_min_kb: u64,
    pub session_gap_min: u32,
    /// Where inbox "archive" moves files (dated subfolders are created inside).
    pub archive_dir: String,
    pub junk_rules: Vec<JunkRule>,
    pub onboarded: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let home = crate::platform::home_dir();
        let mut excluded = vec![".git".to_string(), "$RECYCLE.BIN".to_string(), "System Volume Information".to_string()];
        if let Some(h) = &home {
            excluded.push(h.join("AppData").to_string_lossy().to_string());
        }
        Settings {
            roots: home.map(|h| vec![h.to_string_lossy().to_string()]).unwrap_or_default(),
            excluded,
            inbox_days: 14,
            cold_days: 90,
            cold_min_mb: 500,
            retention_days: 30,
            dup_min_kb: 64,
            session_gap_min: 45,
            archive_dir: dirs::document_dir()
                .map(|d| d.join("Archive").to_string_lossy().to_string())
                .unwrap_or_default(),
            junk_rules: default_junk_rules(),
            onboarded: false,
        }
    }
}

impl Settings {
    pub fn load(conn: &Connection) -> Result<Self> {
        Ok(match kv_get(conn, "settings")? {
            Some(s) => serde_json::from_str(&s).unwrap_or_default(),
            None => Settings::default(),
        })
    }

    pub fn save(&self, conn: &Connection) -> Result<()> {
        kv_set(conn, "settings", &serde_json::to_string(self)?)
    }

    pub fn root_paths(&self) -> Vec<PathBuf> {
        self.roots.iter().map(PathBuf::from).collect()
    }
}

/// Compiled exclusion matcher used by the indexer and the watcher.
#[derive(Debug, Clone, Default)]
pub struct Exclusions {
    paths: Vec<PathBuf>,
    names: Vec<String>,
}

impl Exclusions {
    pub fn new(settings: &Settings, always: &[PathBuf]) -> Self {
        let mut ex = Exclusions::default();
        for e in &settings.excluded {
            let e = e.trim();
            if e.is_empty() {
                continue;
            }
            if e.contains('\\') || e.contains('/') || e.contains(':') {
                ex.paths.push(PathBuf::from(e));
            } else {
                ex.names.push(e.to_lowercase());
            }
        }
        ex.paths.extend(always.iter().cloned());
        ex.names.push(crate::cleanup::holding::HOLDING_DIR_NAME.to_lowercase());
        ex
    }

    pub fn is_excluded(&self, path: &Path) -> bool {
        if let Some(name) = path.file_name() {
            let n = name.to_string_lossy().to_lowercase();
            if self.names.iter().any(|x| *x == n) {
                return true;
            }
        }
        self.paths.iter().any(|p| crate::platform::is_within(path, p))
    }
}
