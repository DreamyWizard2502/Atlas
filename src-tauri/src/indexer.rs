//! Full scans (jwalk, parallel) and the helpers the watcher uses for incremental
//! updates. A scan streams rows over a bounded channel to a single writer that
//! commits in transactions of `BATCH` rows, so the UI stays responsive on drives
//! with hundreds of thousands of files.

use crate::classify;
use crate::db::{ext_of, now, subtree_range, to_unix, Db};
use crate::platform;
use crate::settings::Exclusions;
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const BATCH: usize = 5_000;
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

#[derive(Debug, Clone)]
pub struct Row {
    pub path: String,
    pub parent: String,
    pub name: String,
    pub ext: String,
    pub is_dir: bool,
    pub size: i64,
    pub created: Option<i64>,
    pub modified: Option<i64>,
    pub accessed: Option<i64>,
    pub attrs: u32,
    /// Content-confirmed kind ("folder" for directories).
    pub kind: &'static str,
    /// Storage category (see `classify::CATEGORIES`).
    pub cat: u8,
}

impl Row {
    pub fn from_meta(path: &Path, meta: &std::fs::Metadata) -> Row {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        let is_dir = meta.is_dir();
        let path_s = path.to_string_lossy().to_string();
        let ext = if is_dir { String::new() } else { ext_of(&name) };
        let (kind, cat) = if is_dir {
            ("folder", classify::OTHER)
        } else {
            let kind = classify::kind_for(path, &ext);
            (kind, classify::category(&path_s.to_lowercase(), &name.to_lowercase(), &ext, kind))
        };
        Row {
            parent: path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
            path: path_s,
            ext,
            name,
            kind,
            cat,
            is_dir,
            size: if is_dir { 0 } else { meta.len() as i64 },
            created: to_unix(meta.created()),
            modified: to_unix(meta.modified()),
            accessed: to_unix(meta.accessed()),
            attrs: platform::attrs(meta),
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub phase: String,
    pub root: String,
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub current: String,
    pub elapsed_ms: u64,
    pub done: bool,
    pub cancelled: bool,
    pub error: Option<String>,
}

const UPSERT_SQL: &str = "INSERT INTO files
    (path, parent, name, ext, is_dir, size, created, modified, accessed, attrs, root, gen, kind, cat)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
    ON CONFLICT(path) DO UPDATE SET
        size = CASE WHEN excluded.is_dir THEN files.size ELSE excluded.size END,
        is_dir = excluded.is_dir, created = excluded.created, modified = excluded.modified,
        accessed = excluded.accessed, attrs = excluded.attrs, root = excluded.root, gen = excluded.gen,
        kind = excluded.kind, cat = excluded.cat";

pub fn upsert_row(tx: &Connection, row: &Row, root: &str, gen: i64) -> Result<()> {
    let mut stmt = tx.prepare_cached(UPSERT_SQL)?;
    stmt.execute(params![
        row.path, row.parent, row.name, row.ext, row.is_dir, row.size, row.created, row.modified,
        row.accessed, row.attrs, root, gen, row.kind, row.cat
    ])?;
    Ok(())
}

fn write_batch(conn: &mut Connection, rows: &[Row], root: &str, gen: i64) -> Result<()> {
    let tx = conn.transaction()?;
    for r in rows {
        upsert_row(&tx, r, root, gen)?;
    }
    tx.commit()?;
    Ok(())
}

type Walk = jwalk::WalkDirGeneric<((), Option<std::fs::Metadata>)>;

/// Walk `dir` in parallel, sending batches of rows. Symlinks/junctions are skipped.
pub fn walk(dir: &Path, excl: &Exclusions, cancel: &AtomicBool, tx: mpsc::SyncSender<Vec<Row>>) {
    let ex = excl.clone();
    let walker = Walk::new(dir)
        .skip_hidden(false)
        .follow_links(false)
        .sort(false)
        .process_read_dir(move |_depth, _path, _state, children| {
            children.retain(|r| match r {
                Ok(e) => !e.file_type.is_symlink() && !ex.is_excluded(&e.path()),
                Err(_) => false,
            });
            for e in children.iter_mut().flatten() {
                e.client_state = e.metadata().ok();
            }
        });
    let mut batch = Vec::with_capacity(BATCH);
    for entry in walker.into_iter() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let meta = match entry.client_state {
            Some(m) => m,
            None => match std::fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            },
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        batch.push(Row::from_meta(&path, &meta));
        if batch.len() >= BATCH {
            if tx.send(std::mem::replace(&mut batch, Vec::with_capacity(BATCH))).is_err() {
                return;
            }
        }
    }
    if !batch.is_empty() {
        let _ = tx.send(batch);
    }
}

/// Full scan of every root. Calls `progress` at most every 250 ms.
pub fn scan(
    db: &Db,
    roots: &[PathBuf],
    excl: &Exclusions,
    cancel: &AtomicBool,
    mut progress: impl FnMut(&ScanProgress),
) -> Result<ScanProgress> {
    let started = Instant::now();
    let mut conn = db.conn()?;
    let gen = now();
    let mut p = ScanProgress { phase: "scanning".into(), ..Default::default() };
    let mut last_emit = Instant::now() - PROGRESS_EVERY;

    let roots = dedupe_roots(roots);
    for root in &roots {
        let root_s = root.to_string_lossy().to_string();
        p.root = root_s.clone();
        let (tx, rx) = mpsc::sync_channel::<Vec<Row>>(8);
        std::thread::scope(|s| -> Result<()> {
            s.spawn(|| walk(root, excl, cancel, tx));
            for rows in rx {
                for r in &rows {
                    if r.is_dir {
                        p.dirs += 1;
                    } else {
                        p.files += 1;
                        p.bytes += r.size as u64;
                    }
                }
                if let Some(last) = rows.last() {
                    p.current = last.path.clone();
                }
                write_batch(&mut conn, &rows, &root_s, gen)?;
                if last_emit.elapsed() >= PROGRESS_EVERY {
                    p.elapsed_ms = started.elapsed().as_millis() as u64;
                    progress(&p);
                    last_emit = Instant::now();
                }
            }
            Ok(())
        })?;
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        // Sweep rows under this root that were not seen in this generation.
        p.phase = "finalizing".into();
        progress(&p);
        conn.execute("DELETE FROM files WHERE root = ?1 AND gen < ?2", params![root_s, gen])?;
        rollup_dir_sizes(&mut conn, &root_s)?;
    }

    if cancel.load(Ordering::Relaxed) {
        p.cancelled = true;
    } else {
        // Drop rows that belong to roots no longer configured.
        let keep: Vec<String> = roots.iter().map(|r| r.to_string_lossy().to_string()).collect();
        prune_removed_roots(&conn, &keep)?;
        crate::snapshots::take(&conn)?;
        crate::snapshots::estimate_history(&conn, crate::snapshots::HISTORY_DAYS)?;
    }
    p.phase = "done".into();
    p.done = true;
    p.elapsed_ms = started.elapsed().as_millis() as u64;
    progress(&p);
    Ok(p)
}

/// Remove roots nested inside other roots.
pub fn dedupe_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for r in roots {
        if roots.iter().any(|o| o != r && platform::is_within(r, o) && platform::norm(o) != platform::norm(r)) {
            continue;
        }
        if !out.iter().any(|o| platform::norm(o) == platform::norm(r)) {
            out.push(r.clone());
        }
    }
    out
}

pub fn prune_removed_roots(conn: &Connection, keep: &[String]) -> Result<()> {
    let mut stmt = conn.prepare("SELECT DISTINCT root FROM files")?;
    let existing: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    for root in existing {
        if !keep.contains(&root) {
            conn.execute("DELETE FROM files WHERE root = ?1", [&root])?;
        }
    }
    Ok(())
}

fn depth(p: &str) -> usize {
    p.matches(std::path::MAIN_SEPARATOR).count()
}

/// Recursive totals of a folder (or one file): bytes, file count and bytes per
/// storage category.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    pub size: i64,
    pub count: i64,
    pub cb: [i64; 8],
}

impl Totals {
    pub fn file(size: i64, cat: u8) -> Totals {
        let mut cb = [0; 8];
        cb[(cat as usize).min(7)] = size;
        Totals { size, count: 1, cb }
    }

    pub fn add(&mut self, o: &Totals) {
        self.size += o.size;
        self.count += o.count;
        for i in 0..8 {
            self.cb[i] += o.cb[i];
        }
    }

    pub fn minus(&self, o: &Totals) -> Totals {
        let mut t = *self;
        t.size -= o.size;
        t.count -= o.count;
        for i in 0..8 {
            t.cb[i] -= o.cb[i];
        }
        t
    }

    pub fn neg(&self) -> Totals {
        Totals::default().minus(self)
    }

    fn is_zero(&self) -> bool {
        *self == Totals::default()
    }
}

/// Sum file totals up the folder tree. `top` (if given) is the subtree root,
/// whose totals are not pushed further up.
fn sum_up(files: Vec<(String, i64, u8)>, dirs: &mut [(String, String)], top: Option<&str>) -> HashMap<String, Totals> {
    let mut direct: HashMap<String, Totals> = HashMap::new();
    for (parent, size, cat) in files {
        direct.entry(parent).or_default().add(&Totals::file(size, cat));
    }
    dirs.sort_by_key(|(p, _)| std::cmp::Reverse(depth(p)));
    let mut totals: HashMap<String, Totals> = HashMap::with_capacity(dirs.len());
    for (path, parent) in dirs.iter() {
        let own = direct.get(path).copied().unwrap_or_default();
        let t = totals.entry(path.clone()).or_default();
        t.add(&own);
        let t = *t;
        if Some(path.as_str()) != top {
            totals.entry(parent.clone()).or_default().add(&t);
        }
    }
    totals
}

const WRITE_TOTALS: &str = "UPDATE files SET size = ?1, file_count = ?2, cb0 = ?3, cb1 = ?4, cb2 = ?5, cb3 = ?6,
    cb4 = ?7, cb5 = ?8, cb6 = ?9, cb7 = ?10 WHERE path = ?11";

fn write_totals(conn: &Connection, dirs: &[(String, String)], totals: &HashMap<String, Totals>) -> Result<()> {
    let mut stmt = conn.prepare_cached(WRITE_TOTALS)?;
    for (path, _) in dirs {
        let t = totals.get(path).copied().unwrap_or_default();
        let c = t.cb;
        stmt.execute(params![t.size, t.count, c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], path])?;
    }
    Ok(())
}

/// Recompute recursive totals for every directory under `root`.
pub fn rollup_dir_sizes(conn: &mut Connection, root: &str) -> Result<()> {
    let files: Vec<(String, i64, u8)> = {
        let mut stmt = conn.prepare("SELECT parent, size, cat FROM files WHERE root = ?1 AND is_dir = 0")?;
        let v = stmt.query_map([root], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        v
    };
    let mut dirs: Vec<(String, String)> = {
        let mut stmt = conn.prepare("SELECT path, parent FROM files WHERE root = ?1 AND is_dir = 1")?;
        let v = stmt.query_map([root], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
        v
    };
    let totals = sum_up(files, &mut dirs, None);
    let tx = conn.transaction()?;
    write_totals(&tx, &dirs, &totals)?;
    tx.commit()?;
    Ok(())
}

/// Add `delta` to every indexed ancestor of `path` within `root`.
pub fn propagate(conn: &Connection, path: &Path, root: &Path, delta: &Totals) -> Result<()> {
    if delta.is_zero() {
        return Ok(());
    }
    let mut stmt = conn.prepare_cached(
        "UPDATE files SET size = size + ?1, file_count = file_count + ?2, cb0 = cb0 + ?3, cb1 = cb1 + ?4,
         cb2 = cb2 + ?5, cb3 = cb3 + ?6, cb4 = cb4 + ?7, cb5 = cb5 + ?8, cb6 = cb6 + ?9, cb7 = cb7 + ?10
         WHERE path = ?11",
    )?;
    let c = delta.cb;
    for anc in path.ancestors().skip(1) {
        if !platform::is_within(anc, root) {
            break;
        }
        stmt.execute(params![delta.size, delta.count, c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], anc.to_string_lossy()])?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Created,
    Modified,
    Deleted,
    None,
}

/// Result of reconciling one watcher path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    pub change: Change,
    pub is_dir: bool,
    pub size: i64,
}

impl Outcome {
    const NONE: Outcome = Outcome { change: Change::None, is_dir: false, size: 0 };
}

/// An indexed row as far as reconciliation cares.
#[derive(Debug, Clone, Copy)]
struct Existing {
    is_dir: bool,
    /// For files: the file itself; for folders: recursive totals.
    totals: Totals,
    modified: Option<i64>,
}

fn lookup(conn: &Connection, path: &str) -> Result<Option<Existing>> {
    let mut stmt = conn.prepare_cached(
        "SELECT is_dir, size, file_count, modified, cat, cb0, cb1, cb2, cb3, cb4, cb5, cb6, cb7 FROM files WHERE path = ?1",
    )?;
    Ok(stmt
        .query_row([path], |r| {
            let is_dir: bool = r.get(0)?;
            let size: i64 = r.get(1)?;
            let totals = if is_dir {
                let mut cb = [0i64; 8];
                for (i, v) in cb.iter_mut().enumerate() {
                    *v = r.get(5 + i)?;
                }
                Totals { size, count: r.get(2)?, cb }
            } else {
                Totals::file(size, r.get(4)?)
            };
            Ok(Existing { is_dir, totals, modified: r.get(3)? })
        })
        .optional()?)
}

fn is_indexed(conn: &Connection, path: &Path) -> Result<bool> {
    let mut stmt = conn.prepare_cached("SELECT 1 FROM files WHERE path = ?1")?;
    Ok(stmt.exists([path.to_string_lossy()])?)
}

/// True if `path` or any ancestor strictly below `root` is excluded. Mirrors
/// the full scan, which never excludes the root itself.
pub fn excluded_under(path: &Path, root: &Path, excl: &Exclusions) -> bool {
    let root_n = platform::norm(root);
    path.ancestors()
        .take_while(|a| platform::is_within(a, root))
        .any(|a| platform::norm(a) != root_n && excl.is_excluded(a))
}

/// Bring the index in line with the disk for one path reported by the watcher,
/// inside the caller's transaction.
///
/// Deliberately iterative. If the path's ancestors aren't indexed yet, climb
/// (in a bounded loop) to the nearest indexed ancestor and reconcile the
/// topmost missing directory exactly once; indexing it walks its whole subtree,
/// which includes `path`. If that directory can't be indexed (access denied,
/// a junction, already gone) the event is dropped, which is exactly what a full
/// scan does with such a directory. Nothing here recurses, so neither deep
/// trees nor unreadable parents can grow the stack.
pub fn apply_path(conn: &Connection, path: &Path, root: &Path, excl: &Exclusions) -> Result<Outcome> {
    // Index rows always use the native separator.
    let normalized = PathBuf::from(path.to_string_lossy().replace('/', std::path::MAIN_SEPARATOR_STR));
    let path = normalized.as_path();
    if !platform::is_within(path, root) || excluded_under(path, root, excl) {
        return Ok(Outcome::NONE);
    }
    let mut target = path;
    if !is_indexed(conn, path)? {
        loop {
            let Some(parent) = target.parent() else { return Ok(Outcome::NONE) };
            if !platform::is_within(parent, root) {
                // Not even the root is indexed yet; the scan owns this.
                return Ok(Outcome::NONE);
            }
            if is_indexed(conn, parent)? {
                break;
            }
            target = parent;
        }
    }
    apply_one(conn, target, root, excl)
}

/// Reconcile a single path whose parent is indexed (or which is the root).
fn apply_one(conn: &Connection, path: &Path, root: &Path, excl: &Exclusions) -> Result<Outcome> {
    let path_s = path.to_string_lossy().to_string();
    let root_s = root.to_string_lossy().to_string();
    let existing = lookup(conn, &path_s)?;
    let meta = std::fs::symlink_metadata(path).ok().filter(|m| !m.file_type().is_symlink());

    // Drop the old row(s) when the path is gone or switched between file and folder.
    let mut removed: Option<Existing> = None;
    if let Some(old) = existing {
        if meta.as_ref().map(|m| m.is_dir() != old.is_dir).unwrap_or(true) {
            if old.is_dir {
                let (lo, hi) = subtree_range(&path_s);
                conn.execute("DELETE FROM files WHERE path >= ?1 AND path < ?2", params![lo, hi])?;
            }
            conn.execute("DELETE FROM files WHERE path = ?1", [&path_s])?;
            propagate(conn, path, root, &old.totals.neg())?;
            removed = Some(old);
        }
    }
    let Some(meta) = meta else {
        return Ok(match removed {
            Some(old) => Outcome { change: Change::Deleted, is_dir: old.is_dir, size: old.totals.size },
            None => Outcome::NONE,
        });
    };

    let row = Row::from_meta(path, &meta);
    let gen = now();
    if let (Some(old), None) = (existing, removed) {
        // Same kind as before: refresh metadata.
        upsert_row(conn, &row, &root_s, gen)?;
        if !row.is_dir {
            propagate(conn, path, root, &Totals::file(row.size, row.cat).minus(&old.totals))?;
        }
        let change = if !row.is_dir && old.modified != row.modified { Change::Modified } else { Change::None };
        return Ok(Outcome { change, is_dir: row.is_dir, size: row.size });
    }

    upsert_row(conn, &row, &root_s, gen)?;
    if !row.is_dir {
        propagate(conn, path, root, &Totals::file(row.size, row.cat))?;
        return Ok(Outcome { change: Change::Created, is_dir: false, size: row.size });
    }
    // A new directory (created or moved in): index its whole subtree.
    let total = index_subtree(conn, path, &path_s, &root_s, excl, gen)?;
    rollup_subtree(conn, &path_s)?;
    propagate(conn, path, root, &total)?;
    Ok(Outcome { change: Change::Created, is_dir: true, size: total.size })
}

/// Walk `dir` (excluding `dir` itself) into the index; returns its totals.
fn index_subtree(conn: &Connection, dir: &Path, dir_s: &str, root_s: &str, excl: &Exclusions, gen: i64) -> Result<Totals> {
    let (tx_rows, rx) = mpsc::sync_channel::<Vec<Row>>(4);
    let cancel = AtomicBool::new(false);
    std::thread::scope(|s| -> Result<Totals> {
        s.spawn(|| walk(dir, excl, &cancel, tx_rows));
        let mut total = Totals::default();
        for rows in rx {
            for r in rows.iter().filter(|r| r.path != dir_s) {
                upsert_row(conn, r, root_s, gen)?;
                if !r.is_dir {
                    total.add(&Totals::file(r.size, r.cat));
                }
            }
        }
        Ok(total)
    })
}

/// Recompute totals for the directory `dir` and everything beneath it
/// (inside the caller's transaction).
fn rollup_subtree(conn: &Connection, dir: &str) -> Result<()> {
    let (lo, hi) = subtree_range(dir);
    let mut files = Vec::new();
    let mut dirs: Vec<(String, String)> = vec![];
    {
        let mut stmt = conn.prepare(
            "SELECT path, parent, is_dir, size, cat FROM files WHERE (path >= ?1 AND path < ?2) OR path = ?3",
        )?;
        let mut rows = stmt.query(params![lo, hi, dir])?;
        while let Some(r) = rows.next()? {
            let (path, parent, is_dir): (String, String, bool) = (r.get(0)?, r.get(1)?, r.get(2)?);
            if is_dir {
                dirs.push((path, parent));
            } else {
                files.push((parent, r.get(3)?, r.get(4)?));
            }
        }
    }
    let totals = sum_up(files, &mut dirs, Some(dir));
    write_totals(conn, &dirs, &totals)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_indexes_and_rolls_up_sizes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::create_dir_all(root.join("skipme")).unwrap();
        std::fs::write(root.join("a/one.txt"), vec![0u8; 100]).unwrap();
        std::fs::write(root.join("a/b/two.pdf"), vec![0u8; 50]).unwrap();
        std::fs::write(root.join("skipme/x.bin"), vec![0u8; 999]).unwrap();
        let db = Db::open(tmp.path().join("t.db")).unwrap();
        let settings = crate::settings::Settings { excluded: vec!["skipme".into()], ..Default::default() };
        let excl = Exclusions::new(&settings, &[]);
        let cancel = AtomicBool::new(false);
        let res = scan(&db, &[root.clone()], &excl, &cancel, |_| {}).unwrap();
        assert_eq!(res.files, 2);
        let conn = db.conn().unwrap();
        let size: i64 = conn
            .query_row("SELECT size FROM files WHERE path = ?1", [root.to_string_lossy()], |r| r.get(0))
            .unwrap();
        assert_eq!(size, 150);
        let a: (i64, i64) = conn
            .query_row("SELECT size, file_count FROM files WHERE path = ?1", [root.join("a").to_string_lossy()], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(a, (150, 2));

        // Incremental: add a file, delete one, and check rollups.
        let conn = db.conn().unwrap();
        std::fs::write(root.join("a/b/three.txt"), vec![0u8; 10]).unwrap();
        assert_eq!(apply_path(&conn, &root.join("a/b/three.txt"), &root, &excl).unwrap().change, Change::Created);
        std::fs::remove_file(root.join("a/one.txt")).unwrap();
        assert_eq!(apply_path(&conn, &root.join("a/one.txt"), &root, &excl).unwrap().change, Change::Deleted);
        let size: i64 = conn
            .query_row("SELECT size FROM files WHERE path = ?1", [root.to_string_lossy()], |r| r.get(0))
            .unwrap();
        assert_eq!(size, 60);

        // A new directory with contents gets indexed recursively.
        std::fs::create_dir_all(root.join("new/deep")).unwrap();
        std::fs::write(root.join("new/deep/f.txt"), vec![0u8; 40]).unwrap();
        apply_path(&conn, &root.join("new"), &root, &excl).unwrap();
        let size: i64 = conn
            .query_row("SELECT size FROM files WHERE path = ?1", [root.to_string_lossy()], |r| r.get(0))
            .unwrap();
        assert_eq!(size, 100);
        let fts: i64 = conn
            .query_row("SELECT count(*) FROM files_fts WHERE files_fts MATCH 'three*'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fts, 1);
    }
}
