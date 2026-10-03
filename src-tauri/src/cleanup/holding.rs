//! Holding area: "clean" moves items here (same volume → a rename, never a
//! copy) and records the original path. Restore moves them back. After the
//! retention period items go to the Recycle Bin. Nothing is hard-deleted.

use super::blocklist::Blocklist;
use super::{CleanItem, ItemOutcome, RunReport};
use crate::db::now;
use crate::platform;
use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const HOLDING_DIR_NAME: &str = ".atlas-holding";

#[derive(Debug, Clone)]
pub struct HoldingArea {
    /// App-managed holding dir on the volume where Atlas keeps its data.
    pub primary: PathBuf,
}

impl HoldingArea {
    pub fn new(primary: PathBuf) -> Self {
        HoldingArea { primary }
    }

    /// Holding directory on the same volume as `path`.
    pub fn dir_for(&self, path: &Path) -> PathBuf {
        let pv = platform::volume_root(&self.primary).map(|p| platform::norm(&p));
        let v = platform::volume_root(path);
        match (pv, v) {
            (Some(pv), Some(v)) if pv != platform::norm(&v) => v.join(HOLDING_DIR_NAME),
            _ => self.primary.clone(),
        }
    }

    /// Every holding dir that might exist (for blocklisting / exclusion).
    pub fn all_dirs(&self) -> Vec<PathBuf> {
        let mut v = vec![self.primary.clone()];
        for root in platform::list_roots() {
            v.push(root.join(HOLDING_DIR_NAME));
        }
        v
    }
}

pub struct RunContext<'a> {
    pub blocklist: &'a Blocklist,
    pub holding: &'a HoldingArea,
    pub retention_days: u32,
    /// Duplicate groups from the latest analysis: path → all member paths.
    pub dup_groups: &'a HashMap<String, Vec<String>>,
}

pub fn new_batch_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{}-{:05x}", chrono::Local::now().format("%Y%m%d-%H%M%S"), nanos & 0xfffff)
}

pub fn log(conn: &Connection, batch: &str, action: &str, path: &str, detail: &str, dry_run: bool) -> Result<()> {
    conn.execute(
        "INSERT INTO cleanup_log(ts, batch_id, action, path, detail, dry_run) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![now(), batch, action, path, detail, dry_run],
    )?;
    Ok(())
}

pub(crate) fn dir_has_files(dir: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else { return true };
    for e in rd.flatten() {
        match e.file_type() {
            Ok(t) if t.is_dir() => {
                if dir_has_files(&e.path()) {
                    return true;
                }
            }
            _ => return true,
        }
    }
    false
}

/// Validate one item immediately before acting on it.
fn validate(item: &CleanItem, ctx: &RunContext, selected: &HashSet<String>) -> Result<std::fs::Metadata, String> {
    let path = Path::new(&item.path);
    ctx.blocklist.check_live(path)?;
    let meta = std::fs::symlink_metadata(path).map_err(|_| "No longer exists".to_string())?;
    if meta.is_dir() {
        if item.category != "empty_folders" {
            return Err("Folders can only be cleaned as empty folders".into());
        }
        if dir_has_files(path) {
            return Err("Folder is no longer empty".into());
        }
    } else if meta.len() as i64 != item.size {
        return Err("File changed since review".into());
    }
    if item.category == "duplicates" {
        let Some(members) = ctx.dup_groups.get(&item.path) else {
            return Err("Not in the current duplicate analysis; re-run analysis".into());
        };
        // At least one other copy must remain untouched and still exist.
        let survivor = members.iter().any(|m| {
            m != &item.path
                && !selected.contains(m)
                && std::fs::metadata(m).map(|mm| mm.len() == meta.len()).unwrap_or(false)
        });
        if !survivor {
            return Err("Keeping at least one copy: every copy of this file was selected".into());
        }
    }
    Ok(meta)
}

/// Move the selected items into the holding area (or just validate in dry-run).
pub fn run(conn: &Connection, ctx: &RunContext, items: &[CleanItem], dry_run: bool) -> Result<RunReport> {
    let batch = new_batch_id();
    let mut report = RunReport { batch_id: batch.clone(), dry_run, ..Default::default() };
    let selected: HashSet<String> = items.iter().map(|i| i.path.clone()).collect();
    // Items already moved in this run count as "selected" for the survivor rule.
    let mut seen = HashSet::new();
    for item in items {
        if !seen.insert(item.path.clone()) {
            continue;
        }
        let outcome = match validate(item, ctx, &selected) {
            Err(reason) => {
                log(conn, &batch, "skip", &item.path, &reason, dry_run)?;
                ItemOutcome { path: item.path.clone(), ok: false, detail: reason }
            }
            Ok(meta) if dry_run => {
                log(conn, &batch, "would_hold", &item.path, &item.category, true)?;
                report.bytes += if meta.is_dir() { 0 } else { meta.len() as i64 };
                ItemOutcome { path: item.path.clone(), ok: true, detail: "Would move to holding area".into() }
            }
            Ok(meta) => match hold(conn, ctx, &batch, item, meta.is_dir()) {
                Ok(held) => {
                    log(conn, &batch, "hold", &item.path, &held.to_string_lossy(), false)?;
                    report.bytes += if meta.is_dir() { 0 } else { meta.len() as i64 };
                    ItemOutcome { path: item.path.clone(), ok: true, detail: "Moved to holding area".into() }
                }
                Err(e) => {
                    log(conn, &batch, "error", &item.path, &e.to_string(), false)?;
                    ItemOutcome { path: item.path.clone(), ok: false, detail: e.to_string() }
                }
            },
        };
        if outcome.ok {
            report.moved += 1;
        } else {
            report.skipped += 1;
        }
        report.outcomes.push(outcome);
    }
    Ok(report)
}

fn hold(conn: &Connection, ctx: &RunContext, batch: &str, item: &CleanItem, is_dir: bool) -> Result<PathBuf> {
    let src = Path::new(&item.path);
    let name = src.file_name().ok_or_else(|| anyhow!("Invalid path"))?;
    let base = ctx.holding.dir_for(src);
    let t = now();
    conn.execute(
        "INSERT INTO holding(batch_id, original_path, held_path, size, category, is_dir, held_at, expires_at, status)
         VALUES (?1, ?2, '', ?3, ?4, ?5, ?6, ?7, 'pending')",
        params![batch, item.path, item.size, item.category, is_dir, t, t + ctx.retention_days as i64 * 86_400],
    )?;
    let id = conn.last_insert_rowid();
    let dest_dir = base.join(batch).join(id.to_string());
    let dest = dest_dir.join(name);
    let result = std::fs::create_dir_all(&dest_dir).and_then(|_| std::fs::rename(src, &dest));
    match result {
        Ok(()) => {
            conn.execute(
                "UPDATE holding SET held_path = ?1, status = 'held' WHERE id = ?2",
                params![dest.to_string_lossy(), id],
            )?;
            Ok(dest)
        }
        Err(e) => {
            conn.execute("DELETE FROM holding WHERE id = ?1", [id])?;
            let _ = std::fs::remove_dir(&dest_dir);
            // ERROR_NOT_SAME_DEVICE: we refuse to fall back to copy + delete.
            if e.raw_os_error() == Some(17) {
                Err(anyhow!("Holding area is on a different drive; not moving"))
            } else {
                Err(anyhow!("Could not move: {e}"))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeldItem {
    pub id: i64,
    pub batch_id: String,
    pub original_path: String,
    pub held_path: String,
    pub size: i64,
    pub category: String,
    pub is_dir: bool,
    pub held_at: i64,
    pub expires_at: i64,
    pub status: String,
}

pub fn list(conn: &Connection, include_done: bool) -> Result<Vec<HeldItem>> {
    let sql = format!(
        "SELECT id, batch_id, original_path, held_path, size, category, is_dir, held_at, expires_at, status
         FROM holding WHERE {} ORDER BY held_at DESC, id DESC LIMIT 5000",
        if include_done { "status != 'pending'" } else { "status = 'held'" }
    );
    let mut stmt = conn.prepare(&sql)?;
    let v = stmt
        .query_map([], |r| {
            Ok(HeldItem {
                id: r.get(0)?,
                batch_id: r.get(1)?,
                original_path: r.get(2)?,
                held_path: r.get(3)?,
                size: r.get(4)?,
                category: r.get(5)?,
                is_dir: r.get(6)?,
                held_at: r.get(7)?,
                expires_at: r.get(8)?,
                status: r.get(9)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(v)
}

/// Pick a free destination: the original path, or "name (restored).ext".
fn free_destination(original: &Path) -> PathBuf {
    if std::fs::symlink_metadata(original).is_err() {
        return original.to_path_buf();
    }
    let stem = original.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = original.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let parent = original.parent().unwrap_or(Path::new(""));
    for n in 1.. {
        let label = if n == 1 { " (restored)".to_string() } else { format!(" (restored {n})") };
        let cand = parent.join(format!("{stem}{label}{ext}"));
        if std::fs::symlink_metadata(&cand).is_err() {
            return cand;
        }
    }
    unreachable!()
}

pub fn restore(conn: &Connection, ids: &[i64]) -> Result<Vec<ItemOutcome>> {
    let batch = new_batch_id();
    let mut out = Vec::new();
    for &id in ids {
        let row: Option<(String, String, String)> = conn
            .query_row("SELECT original_path, held_path, status FROM holding WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?;
        let Some((original, held, status)) = row else { continue };
        if status != "held" {
            out.push(ItemOutcome { path: original, ok: false, detail: format!("Item is {status}") });
            continue;
        }
        let held_p = Path::new(&held);
        if std::fs::symlink_metadata(held_p).is_err() {
            conn.execute("UPDATE holding SET status = 'missing' WHERE id = ?1", [id])?;
            log(conn, &batch, "error", &original, "Held copy is missing", false)?;
            out.push(ItemOutcome { path: original, ok: false, detail: "Held copy is missing".into() });
            continue;
        }
        let dest = free_destination(Path::new(&original));
        let res = dest
            .parent()
            .map(|p| std::fs::create_dir_all(p))
            .unwrap_or(Ok(()))
            .and_then(|_| std::fs::rename(held_p, &dest));
        match res {
            Ok(()) => {
                conn.execute("UPDATE holding SET status = 'restored' WHERE id = ?1", [id])?;
                // Tidy the now-empty per-item and batch folders.
                if let Some(item_dir) = held_p.parent() {
                    let _ = std::fs::remove_dir(item_dir);
                    if let Some(batch_dir) = item_dir.parent() {
                        let _ = std::fs::remove_dir(batch_dir);
                    }
                }
                let detail = if dest == Path::new(&original) {
                    "Restored".to_string()
                } else {
                    format!("Restored as {}", dest.display())
                };
                log(conn, &batch, "restore", &original, &detail, false)?;
                out.push(ItemOutcome { path: dest.to_string_lossy().to_string(), ok: true, detail });
            }
            Err(e) => {
                log(conn, &batch, "error", &original, &format!("Restore failed: {e}"), false)?;
                out.push(ItemOutcome { path: original, ok: false, detail: format!("Restore failed: {e}") });
            }
        }
    }
    Ok(out)
}

/// Send expired held items to the Recycle Bin (never a permanent delete).
/// `send_to_trash` is injectable so tests don't touch the real Recycle Bin.
pub fn purge_expired(
    conn: &Connection,
    at: i64,
    send_to_trash: impl Fn(&Path) -> Result<(), String>,
) -> Result<usize> {
    let mut stmt = conn.prepare("SELECT id, original_path, held_path FROM holding WHERE status = 'held' AND expires_at <= ?1")?;
    let rows: Vec<(i64, String, String)> =
        stmt.query_map([at], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
    let batch = new_batch_id();
    let mut n = 0;
    for (id, original, held) in rows {
        let held_p = Path::new(&held);
        if std::fs::symlink_metadata(held_p).is_err() {
            conn.execute("UPDATE holding SET status = 'missing' WHERE id = ?1", [id])?;
            continue;
        }
        match send_to_trash(held_p) {
            Ok(()) => {
                conn.execute("UPDATE holding SET status = 'trashed' WHERE id = ?1", [id])?;
                log(conn, &batch, "trash", &original, "Retention ended; sent to Recycle Bin", false)?;
                if let Some(item_dir) = held_p.parent() {
                    let _ = std::fs::remove_dir(item_dir);
                    if let Some(batch_dir) = item_dir.parent() {
                        let _ = std::fs::remove_dir(batch_dir);
                    }
                }
                n += 1;
            }
            Err(e) => {
                log(conn, &batch, "error", &original, &format!("Recycle Bin move failed: {e}"), false)?;
            }
        }
    }
    Ok(n)
}

pub fn system_trash(p: &Path) -> Result<(), String> {
    trash::delete(p).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    struct Fx {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        db: Db,
        holding: HoldingArea,
        blocklist: Blocklist,
    }

    fn fx() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("data");
        std::fs::create_dir_all(&root).unwrap();
        let holding = HoldingArea::new(tmp.path().join("Atlas").join("holding"));
        let blocklist = Blocklist {
            system_roots: vec![tmp.path().join("Windows")],
            own_dirs: holding.all_dirs(),
            ..Default::default()
        };
        let db = Db::open(tmp.path().join("t.db")).unwrap();
        Fx { root, db, holding, blocklist, _tmp: tmp }
    }

    fn item(p: &Path, category: &str) -> CleanItem {
        let size = std::fs::metadata(p).map(|m| if m.is_dir() { 0 } else { m.len() as i64 }).unwrap_or(0);
        CleanItem { path: p.to_string_lossy().to_string(), category: category.into(), size }
    }

    #[test]
    fn hold_and_restore_round_trip() {
        let f = fx();
        let conn = f.db.conn().unwrap();
        let file = f.root.join("project").join("old render.mp4");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"render bytes").unwrap();
        let groups = HashMap::new();
        let ctx = RunContext { blocklist: &f.blocklist, holding: &f.holding, retention_days: 30, dup_groups: &groups };

        let report = run(&conn, &ctx, &[item(&file, "large_cold")], false).unwrap();
        assert_eq!(report.moved, 1, "{:?}", report.outcomes);
        assert!(!file.exists());
        let held = list(&conn, false).unwrap();
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].original_path, file.to_string_lossy());
        assert!(Path::new(&held[0].held_path).exists());
        assert!(platform::is_within(Path::new(&held[0].held_path), &f.holding.primary));
        assert_eq!(held[0].expires_at - held[0].held_at, 30 * 86_400);

        // Even if the original folder was removed meanwhile, restore recreates it.
        std::fs::remove_dir(file.parent().unwrap()).unwrap();
        let out = restore(&conn, &[held[0].id]).unwrap();
        assert!(out[0].ok, "{:?}", out);
        assert_eq!(std::fs::read(&file).unwrap(), b"render bytes");
        assert!(list(&conn, false).unwrap().is_empty());
        // Restoring twice is a no-op with a clear message.
        assert!(!restore(&conn, &[held[0].id]).unwrap()[0].ok);
        let actions: Vec<String> = conn
            .prepare("SELECT action FROM cleanup_log ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(actions, vec!["hold", "restore"]);
    }

    #[test]
    fn restore_never_overwrites() {
        let f = fx();
        let conn = f.db.conn().unwrap();
        let file = f.root.join("report.pdf");
        std::fs::write(&file, b"v1").unwrap();
        let groups = HashMap::new();
        let ctx = RunContext { blocklist: &f.blocklist, holding: &f.holding, retention_days: 30, dup_groups: &groups };
        run(&conn, &ctx, &[item(&file, "junk")], false).unwrap();
        std::fs::write(&file, b"v2 - new file with the same name").unwrap();
        let id = list(&conn, false).unwrap()[0].id;
        let out = restore(&conn, &[id]).unwrap();
        assert!(out[0].ok);
        assert_eq!(std::fs::read(&file).unwrap(), b"v2 - new file with the same name");
        assert_eq!(std::fs::read(f.root.join("report (restored).pdf")).unwrap(), b"v1");
    }

    #[test]
    fn dry_run_touches_nothing() {
        let f = fx();
        let conn = f.db.conn().unwrap();
        let file = f.root.join("a.bak");
        std::fs::write(&file, b"x").unwrap();
        let groups = HashMap::new();
        let ctx = RunContext { blocklist: &f.blocklist, holding: &f.holding, retention_days: 30, dup_groups: &groups };
        let report = run(&conn, &ctx, &[item(&file, "junk")], true).unwrap();
        assert!(report.dry_run);
        assert_eq!(report.moved, 1);
        assert!(file.exists());
        assert!(list(&conn, true).unwrap().is_empty());
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM cleanup_log WHERE dry_run = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn duplicates_keep_at_least_one_copy() {
        let f = fx();
        let conn = f.db.conn().unwrap();
        let a = f.root.join("a.jpg");
        let b = f.root.join("b.jpg");
        std::fs::write(&a, b"same").unwrap();
        std::fs::write(&b, b"same").unwrap();
        let members = vec![a.to_string_lossy().to_string(), b.to_string_lossy().to_string()];
        let mut groups = HashMap::new();
        for m in &members {
            groups.insert(m.clone(), members.clone());
        }
        let ctx = RunContext { blocklist: &f.blocklist, holding: &f.holding, retention_days: 30, dup_groups: &groups };
        // Selecting every copy is refused.
        let report = run(&conn, &ctx, &[item(&a, "duplicates"), item(&b, "duplicates")], false).unwrap();
        assert_eq!(report.moved, 0);
        assert!(a.exists() && b.exists());
        // Selecting one copy works.
        let report = run(&conn, &ctx, &[item(&b, "duplicates")], false).unwrap();
        assert_eq!(report.moved, 1);
        assert!(a.exists() && !b.exists());
        // A duplicate unknown to the analysis is refused.
        let c = f.root.join("c.jpg");
        std::fs::write(&c, b"same").unwrap();
        assert_eq!(run(&conn, &ctx, &[item(&c, "duplicates")], false).unwrap().moved, 0);
    }

    #[test]
    fn blocked_changed_and_nonempty_items_are_skipped() {
        let f = fx();
        let conn = f.db.conn().unwrap();
        let groups = HashMap::new();
        let ctx = RunContext { blocklist: &f.blocklist, holding: &f.holding, retention_days: 30, dup_groups: &groups };
        let git = f.root.join("repo").join(".git").join("config");
        std::fs::create_dir_all(git.parent().unwrap()).unwrap();
        std::fs::write(&git, b"[core]").unwrap();
        let changed = f.root.join("changed.log");
        std::fs::write(&changed, b"12345").unwrap();
        let mut changed_item = item(&changed, "temp");
        changed_item.size = 1;
        let nonempty = f.root.join("notempty");
        std::fs::create_dir_all(nonempty.join("deep")).unwrap();
        std::fs::write(nonempty.join("deep").join("f.txt"), b"x").unwrap();
        let empty = f.root.join("empty");
        std::fs::create_dir_all(empty.join("also-empty")).unwrap();
        let folder_as_junk = f.root.join("folder");
        std::fs::create_dir_all(&folder_as_junk).unwrap();

        let report = run(
            &conn,
            &ctx,
            &[
                item(&git, "junk"),
                changed_item,
                item(&nonempty, "empty_folders"),
                item(&empty, "empty_folders"),
                item(&folder_as_junk, "junk"),
            ],
            false,
        )
        .unwrap();
        assert_eq!(report.moved, 1, "{:?}", report.outcomes);
        assert!(git.exists() && changed.exists() && nonempty.exists() && folder_as_junk.exists());
        assert!(!empty.exists());
        // The empty folder restores intact.
        let id = list(&conn, false).unwrap()[0].id;
        assert!(restore(&conn, &[id]).unwrap()[0].ok);
        assert!(empty.join("also-empty").is_dir());
    }

    #[test]
    fn expired_items_go_to_recycle_bin_hook() {
        let f = fx();
        let conn = f.db.conn().unwrap();
        let file = f.root.join("old.iso");
        std::fs::write(&file, b"iso").unwrap();
        let groups = HashMap::new();
        let ctx = RunContext { blocklist: &f.blocklist, holding: &f.holding, retention_days: 30, dup_groups: &groups };
        run(&conn, &ctx, &[item(&file, "large_cold")], false).unwrap();
        let trashed = std::cell::RefCell::new(Vec::new());
        // Not yet expired.
        assert_eq!(purge_expired(&conn, now(), |p| { trashed.borrow_mut().push(p.to_path_buf()); Ok(()) }).unwrap(), 0);
        // 31 days later.
        let n = purge_expired(&conn, now() + 31 * 86_400, |p| {
            trashed.borrow_mut().push(p.to_path_buf());
            Ok(())
        })
        .unwrap();
        assert_eq!(n, 1);
        assert_eq!(trashed.borrow().len(), 1);
        let status: String = conn.query_row("SELECT status FROM holding", [], |r| r.get(0)).unwrap();
        assert_eq!(status, "trashed");
    }
}
