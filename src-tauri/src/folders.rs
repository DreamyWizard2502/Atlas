//! Classic folder browsing. Reads the directory from disk (so it is always
//! accurate) and annotates folder sizes from the index when known.

use crate::db::{ext_of, to_unix};
use crate::model::kind_of;
use crate::platform;
use anyhow::{anyhow, Result};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub path: String,
    pub name: String,
    pub ext: String,
    pub is_dir: bool,
    pub size: Option<i64>,
    pub modified: Option<i64>,
    pub created: Option<i64>,
    pub hidden: bool,
    pub kind: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub name: String,
    pub path: String,
    pub kind: &'static str,
}

pub fn places(settings: &crate::settings::Settings) -> Vec<Place> {
    let mut out = Vec::new();
    let known = [
        ("Home", platform::home_dir()),
        ("Desktop", platform::desktop_dir()),
        ("Documents", dirs::document_dir()),
        ("Downloads", platform::downloads_dir()),
        ("Pictures", dirs::picture_dir()),
    ];
    for (name, p) in known {
        if let Some(p) = p {
            out.push(Place { name: name.into(), path: p.to_string_lossy().to_string(), kind: "known" });
        }
    }
    for r in &settings.roots {
        if !out.iter().any(|p| platform::norm(Path::new(&p.path)) == platform::norm(Path::new(r))) {
            let name = Path::new(r).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(r.clone());
            out.push(Place { name, path: r.clone(), kind: "root" });
        }
    }
    for d in platform::list_roots() {
        let s = d.to_string_lossy().to_string();
        out.push(Place { name: s.trim_end_matches('\\').to_string(), path: s, kind: "drive" });
    }
    out
}

pub fn list_dir(conn: &Connection, path: &str) -> Result<Vec<DirEntry>> {
    let dir = PathBuf::from(path);
    let rd = std::fs::read_dir(&dir).map_err(|e| anyhow!("Can't open {}: {e}", dir.display()))?;
    let indexed: HashMap<String, i64> = {
        let mut stmt = conn.prepare("SELECT path, size FROM files WHERE parent = ?1 AND is_dir = 1")?;
        let v = stmt.query_map([path.trim_end_matches('\\')], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        v
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        let Ok(meta) = e.metadata() else { continue };
        let attrs = platform::attrs(&meta);
        if platform::is_system(attrs) && attrs & platform::ATTR_HIDDEN != 0 {
            continue; // hide protected OS files like Explorer does
        }
        let name = e.file_name().to_string_lossy().to_string();
        let is_dir = meta.is_dir();
        let ps = p.to_string_lossy().to_string();
        let ext = if is_dir { String::new() } else { ext_of(&name) };
        out.push(DirEntry {
            size: if is_dir { indexed.get(&ps).copied() } else { Some(meta.len() as i64) },
            kind: if is_dir { "folder" } else { kind_of(&ext) },
            ext,
            path: ps,
            name,
            is_dir,
            modified: to_unix(meta.modified()),
            created: to_unix(meta.created()),
            hidden: attrs & platform::ATTR_HIDDEN != 0,
        });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

pub fn list_subdirs(path: &str) -> Result<Vec<(String, String)>> {
    let rd = std::fs::read_dir(path)?;
    let mut out: Vec<(String, String)> = rd
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter(|e| {
            e.metadata().map(|m| platform::attrs(&m) & (platform::ATTR_HIDDEN | platform::ATTR_SYSTEM) == 0).unwrap_or(false)
        })
        .map(|e| (e.file_name().to_string_lossy().to_string(), e.path().to_string_lossy().to_string()))
        .collect();
    out.sort_by_key(|(n, _)| n.to_lowercase());
    Ok(out)
}
