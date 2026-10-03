//! Keeps the index live after the initial scan.
//!
//! Events are collected into short batches (quiet period 700 ms, max 3 s) and
//! each distinct path is reconciled against the disk with
//! `indexer::apply_path`, in chunked transactions. Atlas's own files (its data
//! folder with the database, and every holding area) are dropped before
//! anything else, so the watcher can never react to its own writes. A burst
//! larger than `MAX_PENDING` paths is not processed path by path; it becomes a
//! rescan request instead.

use crate::cleanup::holding::HOLDING_DIR_NAME;
use crate::db::{ext_of, now, Db};
use crate::indexer::{apply_path, Change};
use crate::platform;
use crate::settings::Exclusions;
use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::params;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

/// Above this many distinct pending paths, a rescan is cheaper and safer.
pub const MAX_PENDING: usize = 200_000;
/// Paths per transaction when applying a batch.
const CHUNK: usize = 2_000;

pub struct WatcherHandle {
    _watchers: Vec<RecommendedWatcher>,
}

/// Directories Atlas writes to itself. Matched in both long and 8.3 short form,
/// because change notifications may report either.
#[derive(Debug, Clone, Default)]
pub struct OwnPaths {
    dirs: Vec<PathBuf>,
}

impl OwnPaths {
    pub fn new(dirs: &[PathBuf]) -> Self {
        let mut out = Vec::new();
        for d in dirs {
            out.push(d.clone());
            if let Some(short) = platform::short_path(d) {
                if platform::norm(&short) != platform::norm(d) {
                    out.push(short);
                }
            }
        }
        OwnPaths { dirs: out }
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.dirs.iter().any(|d| platform::is_within(path, d))
            || path.components().any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(HOLDING_DIR_NAME))
    }
}

pub fn start(
    db: Db,
    roots: Vec<PathBuf>,
    excl: Exclusions,
    own: OwnPaths,
    on_change: impl Fn(usize) + Send + 'static,
    on_overflow: impl Fn() + Send + 'static,
) -> Result<WatcherHandle> {
    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watchers = Vec::new();
    for root in &roots {
        let mut w = notify::recommended_watcher(tx.clone())?;
        if w.watch(root, RecursiveMode::Recursive).is_ok() {
            watchers.push(w);
        }
    }
    drop(tx);
    std::thread::Builder::new().name("atlas-watcher".into()).spawn(move || {
        let mut pending: BTreeSet<PathBuf> = BTreeSet::new();
        let mut overflow = false;
        let mut first: Option<Instant> = None;
        loop {
            match rx.recv_timeout(Duration::from_millis(700)) {
                Ok(Ok(ev)) => {
                    // Backends that can detect dropped events say so here.
                    overflow |= ev.need_rescan();
                    if matches!(ev.kind, notify::EventKind::Access(_)) {
                        continue;
                    }
                    for p in ev.paths {
                        let p = platform::long_path(&p);
                        if own.contains(&p) || excl.is_excluded(&p) {
                            continue;
                        }
                        if pending.len() >= MAX_PENDING {
                            overflow = true;
                        } else {
                            pending.insert(p);
                        }
                    }
                    // Only ignored paths so far: don't even start the batch timer.
                    if pending.is_empty() && !overflow {
                        continue;
                    }
                    let started = *first.get_or_insert_with(Instant::now);
                    if started.elapsed() < Duration::from_secs(3) {
                        continue;
                    }
                }
                Ok(Err(_)) => continue,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            first = None;
            if overflow {
                overflow = false;
                pending.clear();
                on_overflow();
                continue;
            }
            if pending.is_empty() {
                continue;
            }
            let batch: Vec<PathBuf> = std::mem::take(&mut pending).into_iter().collect();
            if let Ok(n) = process_batch(&db, &roots, &excl, &own, &batch) {
                if n > 0 {
                    on_change(n);
                }
            }
        }
    })?;
    Ok(WatcherHandle { _watchers: watchers })
}

/// Apply a batch of changed paths to the index. Parents are applied before
/// their children; each path runs in its own savepoint so a failure can't leave
/// half-applied size totals behind. Returns how many paths changed the index.
pub fn process_batch(db: &Db, roots: &[PathBuf], excl: &Exclusions, own: &OwnPaths, paths: &[PathBuf]) -> Result<usize> {
    let mut paths: Vec<PathBuf> = paths.iter().map(|p| platform::long_path(p)).filter(|p| !own.contains(p)).collect();
    paths.sort();
    paths.dedup();
    let mut conn = db.conn()?;
    let mut changed = 0;
    for chunk in paths.chunks(CHUNK) {
        let mut tx = conn.transaction()?;
        for path in chunk {
            let Some(root) = roots.iter().find(|r| platform::is_within(path, r)) else { continue };
            let sp = tx.savepoint()?;
            let out = match apply_path(&sp, path, root, excl) {
                Ok(out) => out,
                Err(_) => continue, // savepoint rolls back on drop
            };
            if out.change == Change::Deleted && !out.is_dir {
                // The timeline reads creations/edits from the index itself;
                // deletions are only knowable from here.
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                sp.execute(
                    "INSERT INTO events(ts, kind, path, name, ext, size) VALUES (?1, 'deleted', ?2, ?3, ?4, ?5)",
                    params![now(), path.to_string_lossy(), name, ext_of(&name), out.size],
                )?;
            }
            sp.commit()?;
            if out.change != Change::None {
                changed += 1;
            }
        }
        tx.commit()?;
    }
    Ok(changed)
}
