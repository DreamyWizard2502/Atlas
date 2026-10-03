//! Keeps the index live after the initial scan. Events are collected into short
//! batches (quiet period 700 ms, max 3 s) and each distinct path is reconciled
//! against the disk with `indexer::apply_path`.

use crate::db::{ext_of, now, Db};
use crate::indexer::{apply_path, Change};
use crate::platform;
use crate::settings::Exclusions;
use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::params;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

pub struct WatcherHandle {
    _watchers: Vec<RecommendedWatcher>,
}

pub fn start(
    db: Db,
    roots: Vec<PathBuf>,
    excl: Exclusions,
    on_change: impl Fn(usize) + Send + 'static,
    on_overflow: impl Fn(PathBuf) + Send + 'static,
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
        let mut first: Option<Instant> = None;
        loop {
            match rx.recv_timeout(Duration::from_millis(700)) {
                Ok(Ok(ev)) => {
                    if ev.need_rescan() {
                        for r in &roots {
                            on_overflow(r.clone());
                        }
                    }
                    if matches!(ev.kind, notify::EventKind::Access(_)) {
                        continue;
                    }
                    for p in ev.paths {
                        if !excl.is_excluded(&p) {
                            pending.insert(p);
                        }
                    }
                    first.get_or_insert_with(Instant::now);
                    if first.map(|f| f.elapsed() < Duration::from_secs(3)).unwrap_or(true) {
                        continue;
                    }
                }
                Ok(Err(_)) => continue,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if pending.is_empty() {
                continue;
            }
            let batch = std::mem::take(&mut pending);
            first = None;
            match process(&db, &roots, &excl, batch) {
                Ok(n) if n > 0 => on_change(n),
                _ => {}
            }
        }
    })?;
    Ok(WatcherHandle { _watchers: watchers })
}

fn process(db: &Db, roots: &[PathBuf], excl: &Exclusions, batch: BTreeSet<PathBuf>) -> Result<usize> {
    let mut conn = db.conn()?;
    let mut changed = 0;
    for path in batch {
        let Some(root) = roots.iter().find(|r| platform::is_within(&path, r)) else { continue };
        // Ignore anything inside an excluded ancestor.
        if path.ancestors().take_while(|a| platform::is_within(a, root)).any(|a| excl.is_excluded(a)) {
            continue;
        }
        let change = match apply_path(&mut conn, &path, root, excl) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if change == Change::None {
            continue;
        }
        changed += 1;
        let is_file = std::fs::metadata(&path).map(|m| m.is_file()).unwrap_or(true);
        if !is_file {
            continue;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let size = std::fs::metadata(&path).map(|m| m.len() as i64).unwrap_or(0);
        let kind = match change {
            Change::Created => "created",
            Change::Modified => "modified",
            Change::Deleted => "deleted",
            Change::None => continue,
        };
        conn.execute(
            "INSERT INTO events(ts, kind, path, name, ext, size) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![now(), kind, path.to_string_lossy(), name, ext_of(&name), size],
        )?;
    }
    Ok(changed)
}
