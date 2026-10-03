//! Quick Look support: file details and bounded text reads. Media is streamed
//! to the webview through Tauri's asset protocol.

use crate::db::{ext_of, to_unix};
use crate::model::kind_of;
use crate::platform;
use anyhow::{anyhow, Result};
use rusqlite::Connection;
use serde::Serialize;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDetails {
    pub path: String,
    pub name: String,
    pub parent: String,
    pub ext: String,
    pub kind: &'static str,
    pub is_dir: bool,
    pub size: i64,
    pub file_count: Option<i64>,
    pub created: Option<i64>,
    pub modified: Option<i64>,
    pub accessed: Option<i64>,
    pub readonly: bool,
    pub hidden: bool,
    pub tags: Vec<String>,
    pub spaces: Vec<String>,
}

pub fn details(conn: &Connection, path: &str) -> Result<FileDetails> {
    let p = Path::new(path);
    let meta = std::fs::metadata(p).map_err(|e| anyhow!("{e}"))?;
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(path.to_string());
    let ext = if meta.is_dir() { String::new() } else { ext_of(&name) };
    let indexed: Option<(i64, i64)> = conn
        .query_row("SELECT size, file_count FROM files WHERE path = ?1", [path], |r| Ok((r.get(0)?, r.get(1)?)))
        .ok();
    let spaces = crate::spaces::list(conn)
        .unwrap_or_default()
        .into_iter()
        .filter(|s| {
            let (w, mut vals) = crate::spaces::space_where(s.id, &s.rules);
            vals.push(rusqlite::types::Value::Text(path.to_string()));
            conn.query_row(
                &format!("SELECT 1 FROM files f WHERE {w} AND f.path = ?"),
                rusqlite::params_from_iter(vals),
                |_| Ok(()),
            )
            .is_ok()
        })
        .map(|s| s.name)
        .collect();
    Ok(FileDetails {
        path: path.to_string(),
        parent: p.parent().map(|x| x.to_string_lossy().to_string()).unwrap_or_default(),
        kind: if meta.is_dir() { "folder" } else { kind_of(&ext) },
        is_dir: meta.is_dir(),
        size: if meta.is_dir() { indexed.map(|i| i.0).unwrap_or(0) } else { meta.len() as i64 },
        file_count: if meta.is_dir() { indexed.map(|i| i.1) } else { None },
        created: to_unix(meta.created()),
        modified: to_unix(meta.modified()),
        accessed: to_unix(meta.accessed()),
        readonly: meta.permissions().readonly(),
        hidden: platform::attrs(&meta) & platform::ATTR_HIDDEN != 0,
        tags: crate::spaces::tags_for(conn, path).unwrap_or_default(),
        spaces,
        ext,
        name,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPreview {
    pub text: String,
    pub truncated: bool,
}

pub fn read_text(path: &str, max_bytes: usize) -> Result<TextPreview> {
    let meta = std::fs::metadata(path)?;
    if platform::attrs(&meta) & platform::ATTR_CLOUD != 0 {
        return Err(anyhow!("This file is stored online only; open it to download"));
    }
    let mut f = std::fs::File::open(path)?;
    let mut buf = Vec::with_capacity(max_bytes.min(meta.len() as usize));
    f.by_ref().take(max_bytes as u64).read_to_end(&mut buf)?;
    if buf.iter().take(8000).any(|&b| b == 0) {
        // UTF-16 text (with BOM) is common on Windows.
        if buf.starts_with(&[0xFF, 0xFE]) {
            let units: Vec<u16> = buf[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            return Ok(TextPreview { text: String::from_utf16_lossy(&units), truncated: meta.len() as usize > max_bytes });
        }
        return Err(anyhow!("Binary file"));
    }
    Ok(TextPreview {
        text: String::from_utf8_lossy(&buf).to_string(),
        truncated: meta.len() as usize > max_bytes,
    })
}
