//! Dashboard widgets. All of these read the index/snapshots only.

use crate::cleanup::analyze::likely_installed;
use crate::cleanup::CleanupSummary;
use crate::db::{kv_get, now};
use crate::model::{kind_of, FileItem, FILE_COLS};
use crate::platform::{self, DiskSpace};
use crate::settings::Settings;
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn installed_apps() -> &'static Vec<String> {
    static APPS: OnceLock<Vec<String>> = OnceLock::new();
    APPS.get_or_init(platform::installed_app_names)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub drive: Option<DiskSpace>,
    pub indexed_bytes: i64,
    pub indexed_files: i64,
    pub growth_month: Option<i64>,
    pub growth_since: Option<String>,
    pub inbox_count: i64,
    pub inbox_bytes: i64,
    pub reclaimable: Option<i64>,
    pub reclaimable_at: Option<i64>,
}

pub fn metrics(conn: &Connection, settings: &Settings) -> Result<Metrics> {
    let drive = settings.root_paths().first().and_then(|r| platform::disk_space(r)).or_else(|| {
        platform::home_dir().and_then(|h| platform::disk_space(&h))
    });
    let (indexed_bytes, indexed_files): (i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(size), 0), COALESCE(SUM(file_count), 0) FROM files WHERE path = root",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    // Growth this month: latest total snapshot minus the earliest one this month.
    let month_start = chrono::Local::now().format("%Y-%m-01").to_string();
    let first: Option<(String, i64)> = conn
        .query_row(
            "SELECT day, size FROM snapshots WHERE kind = 'total' AND day >= ?1 ORDER BY day ASC LIMIT 1",
            [&month_start],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let last: Option<(String, i64)> = conn
        .query_row("SELECT day, size FROM snapshots WHERE kind = 'total' ORDER BY day DESC LIMIT 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?;
    let (growth_month, growth_since) = match (first, last) {
        (Some((d0, s0)), Some((d1, s1))) if d0 != d1 => (Some(s1 - s0), Some(d0)),
        (Some((d0, _)), Some(_)) => (Some(0), Some(d0)),
        _ => (None, None),
    };
    let inbox = inbox(conn, settings, 0)?;
    let summary: Option<CleanupSummary> =
        kv_get(conn, "cleanup_summary")?.and_then(|s| serde_json::from_str(&s).ok());
    Ok(Metrics {
        drive,
        indexed_bytes,
        indexed_files,
        growth_month,
        growth_since,
        inbox_count: inbox.total,
        inbox_bytes: inbox.total_size,
        reclaimable: summary.as_ref().map(|s| s.total_reclaimable),
        reclaimable_at: summary.map(|s| s.analyzed_at),
    })
}

// ---------------- Jump back in ----------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentFile {
    #[serde(flatten)]
    pub file: FileItem,
    pub last_touched: i64,
    pub opened: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub start: i64,
    pub end: i64,
    pub files: Vec<RecentFile>,
}

const NOISE_EXTS: &str = "'tmp','lnk','ini','log','db','db-journal','db-wal','db-shm','lock','lck','crdownload','part','etl','dat'";

pub fn jump_back_in(conn: &Connection, limit: usize, gap_min: u32) -> Result<Vec<Session>> {
    let mut by_path: HashMap<String, RecentFile> = HashMap::new();
    let mut stmt = conn.prepare(&format!(
        "SELECT {FILE_COLS} FROM files f
         WHERE f.is_dir = 0 AND f.modified IS NOT NULL AND f.modified <= ?1 + 86400
           AND f.ext NOT IN ({NOISE_EXTS}) AND f.name NOT LIKE '~$%' AND f.name NOT LIKE '.%'
         ORDER BY f.modified DESC LIMIT 200"
    ))?;
    for item in stmt.query_map([now()], FileItem::from_row)? {
        let item = item?;
        let t = item.modified.unwrap_or(0);
        by_path.insert(item.path.clone(), RecentFile { file: item, last_touched: t, opened: false });
    }
    let mut stmt = conn.prepare(&format!(
        "SELECT {FILE_COLS}, a.opened_at FROM activity a JOIN files f ON f.path = a.path
         WHERE f.is_dir = 0 ORDER BY a.opened_at DESC LIMIT 100"
    ))?;
    let rows = stmt.query_map([], |r| Ok((FileItem::from_row(r)?, r.get::<_, i64>(10)?)))?;
    for row in rows {
        let (item, opened_at) = row?;
        let e = by_path
            .entry(item.path.clone())
            .or_insert(RecentFile { last_touched: 0, opened: false, file: item });
        if opened_at >= e.last_touched {
            e.last_touched = opened_at;
            e.opened = true;
        }
    }
    let mut files: Vec<RecentFile> = by_path.into_values().collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.last_touched));
    files.truncate(limit);
    Ok(group_sessions(files, gap_min as i64 * 60))
}

/// Split a newest-first list into sessions wherever the gap exceeds `gap`.
pub fn group_sessions(files: Vec<RecentFile>, gap: i64) -> Vec<Session> {
    let mut sessions: Vec<Session> = Vec::new();
    for f in files {
        match sessions.last_mut() {
            Some(s) if s.start - f.last_touched <= gap => {
                s.start = f.last_touched;
                s.files.push(f);
            }
            _ => sessions.push(Session { start: f.last_touched, end: f.last_touched, files: vec![f] }),
        }
    }
    sessions
}

// ---------------- Treemap ----------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    pub path: String,
    pub name: String,
    pub size: i64,
    pub is_dir: bool,
    pub kind: &'static str,
    pub file_count: i64,
    pub other: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Treemap {
    pub path: Option<String>,
    pub name: String,
    pub size: i64,
    pub crumbs: Vec<(String, String)>,
    pub children: Vec<TreeNode>,
}

pub fn treemap(conn: &Connection, path: Option<String>, max_children: usize) -> Result<Treemap> {
    let roots: Vec<String> = {
        let mut stmt = conn.prepare("SELECT path FROM files WHERE path = root ORDER BY size DESC")?;
        let v = stmt.query_map([], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
        v
    };
    let path = path.or_else(|| if roots.len() == 1 { roots.first().cloned() } else { None });
    let mut children: Vec<TreeNode> = Vec::new();
    let (name, size, crumbs) = match &path {
        None => {
            let mut stmt = conn.prepare("SELECT path, name, size, file_count FROM files WHERE path = root ORDER BY size DESC")?;
            let rows = stmt.query_map([], |r| {
                Ok(TreeNode {
                    path: r.get(0)?,
                    name: r.get::<_, String>(0)?,
                    size: r.get(2)?,
                    is_dir: true,
                    kind: "folder",
                    file_count: r.get(3)?,
                    other: false,
                })
            })?;
            for n in rows {
                children.push(n?);
            }
            let total = children.iter().map(|c| c.size).sum();
            ("All locations".to_string(), total, vec![])
        }
        Some(p) => {
            let (name, size, root): (String, i64, String) = conn
                .query_row("SELECT name, size, root FROM files WHERE path = ?1", [p], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .optional()?
                .unwrap_or_else(|| (p.clone(), 0, p.clone()));
            let mut stmt = conn.prepare(
                "SELECT path, name, size, is_dir, ext, file_count FROM files WHERE parent = ?1 AND size > 0 ORDER BY size DESC",
            )?;
            let rows = stmt.query_map([p], |r| {
                let is_dir: bool = r.get(3)?;
                let ext: String = r.get(4)?;
                Ok(TreeNode {
                    path: r.get(0)?,
                    name: r.get(1)?,
                    size: r.get(2)?,
                    is_dir,
                    kind: if is_dir { "folder" } else { kind_of(&ext) },
                    file_count: if is_dir { r.get(5)? } else { 1 },
                    other: false,
                })
            })?;
            let mut other = (0i64, 0i64);
            for (i, n) in rows.enumerate() {
                let n = n?;
                if i < max_children {
                    children.push(n);
                } else {
                    other.0 += n.size;
                    other.1 += n.file_count;
                }
            }
            if other.0 > 0 {
                children.push(TreeNode {
                    path: String::new(),
                    name: "Everything else".into(),
                    size: other.0,
                    is_dir: false,
                    kind: "other",
                    file_count: other.1,
                    other: true,
                });
            }
            // Breadcrumbs from the root (or "All locations") down to here.
            let mut crumbs = Vec::new();
            let mut cur = Some(PathBuf::from(p));
            let root_p = PathBuf::from(&root);
            while let Some(c) = cur {
                if !platform::is_within(&c, &root_p) {
                    break;
                }
                let label = if platform::norm(&c) == platform::norm(&root_p) {
                    c.to_string_lossy().to_string()
                } else {
                    c.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
                };
                crumbs.push((label, c.to_string_lossy().to_string()));
                cur = c.parent().map(Path::to_path_buf);
            }
            crumbs.reverse();
            if roots.len() > 1 {
                crumbs.insert(0, ("All locations".into(), String::new()));
            }
            (name, size, crumbs)
        }
    };
    Ok(Treemap { path, name, size, crumbs, children })
}

// ---------------- Growth ----------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrowthPoint {
    pub day: String,
    pub size: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderGrowth {
    pub path: String,
    pub name: String,
    pub delta: i64,
    pub size: i64,
    pub since: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Growth {
    pub points: Vec<GrowthPoint>,
    pub fastest: Option<FolderGrowth>,
    pub top_types: Vec<(String, i64, i64)>,
}

pub fn growth(conn: &Connection, days: i64) -> Result<Growth> {
    let since = (chrono::Local::now() - chrono::Duration::days(days)).format("%Y-%m-%d").to_string();
    let mut stmt = conn.prepare("SELECT day, size FROM snapshots WHERE kind = 'total' AND day >= ?1 ORDER BY day")?;
    let points = stmt
        .query_map([&since], |r| Ok(GrowthPoint { day: r.get(0)?, size: r.get(1)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut fastest = None;
    if let (Some(first), Some(last)) = (points.first(), points.last()) {
        if first.day != last.day {
            let mut stmt = conn.prepare(
                "SELECT b.key, b.size, b.size - COALESCE(a.size, 0) AS delta
                 FROM snapshots b LEFT JOIN snapshots a ON a.kind = 'folder' AND a.key = b.key AND a.day = ?1
                 WHERE b.kind = 'folder' AND b.day = ?2 ORDER BY delta DESC LIMIT 1",
            )?;
            fastest = stmt
                .query_row(params![first.day, last.day], |r| {
                    let path: String = r.get(0)?;
                    Ok(FolderGrowth {
                        name: Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(path.clone()),
                        path,
                        size: r.get(1)?,
                        delta: r.get(2)?,
                        since: first.day.clone(),
                    })
                })
                .optional()?
                .filter(|f| f.delta > 0);
        }
    }
    let mut stmt = conn.prepare(
        "SELECT key, size, count FROM snapshots WHERE kind = 'type' AND day = (SELECT MAX(day) FROM snapshots) ORDER BY size DESC LIMIT 8",
    )?;
    let top_types = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
    Ok(Growth { points, fastest, top_types })
}

// ---------------- Inbox (Downloads / Desktop) ----------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxItem {
    #[serde(flatten)]
    pub file: FileItem,
    pub source: &'static str,
    pub age_days: i64,
    pub installed_as: Option<String>,
    pub duplicate_of: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inbox {
    pub items: Vec<InboxItem>,
    pub total: i64,
    pub total_size: i64,
    pub threshold_days: u32,
}

/// If `name` looks like "file (2).pdf" or "file - Copy.pdf", the base name.
pub fn duplicate_base(name: &str) -> Option<String> {
    let p = Path::new(name);
    let stem = p.file_stem()?.to_string_lossy().to_string();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let mut base = stem.trim_end().to_string();
    let mut changed = false;
    // " (2)"
    if base.ends_with(')') {
        if let Some(open) = base.rfind(" (") {
            let inner = &base[open + 2..base.len() - 1];
            if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()) {
                base.truncate(open);
                changed = true;
            }
        }
    }
    // " - Copy"
    for suffix in [" - Copy", " - copy", " copy", " Copy"] {
        if let Some(b) = base.strip_suffix(suffix) {
            base = b.to_string();
            changed = true;
            break;
        }
    }
    if let Some(b) = base.strip_prefix("Copy of ") {
        base = b.to_string();
        changed = true;
    }
    (changed && !base.is_empty()).then(|| format!("{base}{ext}"))
}

pub fn inbox(conn: &Connection, settings: &Settings, limit: usize) -> Result<Inbox> {
    let cutoff = now() - settings.inbox_days as i64 * 86_400;
    let mut items = Vec::new();
    let (mut total, mut total_size) = (0i64, 0i64);
    let sources = [("Downloads", platform::downloads_dir()), ("Desktop", platform::desktop_dir())];
    for (label, dir) in sources {
        let Some(dir) = dir else { continue };
        let dir_s = dir.to_string_lossy().to_string();
        let mut stmt = conn.prepare(&format!(
            "SELECT {FILE_COLS} FROM files f WHERE f.parent = ?1
             AND f.path NOT IN (SELECT path FROM inbox_dismissed)"
        ))?;
        let all: Vec<FileItem> = stmt.query_map([&dir_s], FileItem::from_row)?.collect::<Result<_, _>>()?;
        let names: HashSet<String> = all.iter().map(|f| f.name.to_lowercase()).collect();
        for f in all {
            let arrived = f.created.unwrap_or(0).max(f.modified.unwrap_or(0));
            if arrived == 0 || arrived > cutoff || f.ext == "lnk" || f.name.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            total += 1;
            total_size += f.size;
            if items.len() >= limit {
                continue;
            }
            let installed_as = if ["exe", "msi", "msix", "appx"].contains(&f.ext.as_str()) {
                likely_installed(&f.name, installed_apps())
            } else {
                None
            };
            let duplicate_of = duplicate_base(&f.name).filter(|b| names.contains(&b.to_lowercase()));
            items.push(InboxItem {
                age_days: (now() - arrived) / 86_400,
                source: label,
                installed_as,
                duplicate_of,
                file: f,
            });
        }
    }
    items.sort_by_key(|i| std::cmp::Reverse(i.age_days));
    Ok(Inbox { items, total, total_size, threshold_days: settings.inbox_days })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_names() {
        assert_eq!(duplicate_base("invoice (2).pdf").as_deref(), Some("invoice.pdf"));
        assert_eq!(duplicate_base("invoice - Copy.pdf").as_deref(), Some("invoice.pdf"));
        assert_eq!(duplicate_base("Copy of plan.dwg").as_deref(), Some("plan.dwg"));
        assert_eq!(duplicate_base("report (final).pdf"), None);
        assert_eq!(duplicate_base("report.pdf"), None);
        assert_eq!(duplicate_base("(1).txt"), None);
    }

    fn rf(t: i64) -> RecentFile {
        RecentFile {
            file: FileItem {
                id: t,
                path: format!("C:\\f{t}"),
                parent: "C:\\".into(),
                name: format!("f{t}"),
                ext: String::new(),
                is_dir: false,
                size: 0,
                created: None,
                modified: Some(t),
                accessed: None,
                kind: "other",
            },
            last_touched: t,
            opened: false,
        }
    }

    #[test]
    fn sessions_split_on_gap() {
        let gap = 45 * 60;
        let files = vec![rf(10_000), rf(9_000), rf(9_000 - gap), rf(9_000 - gap - gap - 1)];
        let s = group_sessions(files, gap);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].files.len(), 3);
        assert_eq!(s[1].files.len(), 1);
    }
}
