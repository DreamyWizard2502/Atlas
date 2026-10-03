//! Shared application state and the long-running background jobs (scan,
//! watcher, cleanup analysis, maintenance).

use crate::cleanup::analyze::{self, Analysis, AnalyzeEnv};
use crate::cleanup::blocklist::Blocklist;
use crate::cleanup::holding::{self, HoldingArea};
use crate::db::{kv_get, kv_set, now, Db};
use crate::settings::{Exclusions, Settings};
use crate::watcher::WatcherHandle;
use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

pub struct AppState {
    pub db: Db,
    pub data_dir: PathBuf,
    pub holding: HoldingArea,
    pub scan_cancel: Mutex<Option<Arc<AtomicBool>>>,
    pub rescan_requested: AtomicBool,
    pub watcher: Mutex<Option<WatcherHandle>>,
    pub analysis: Mutex<Option<Analysis>>,
    pub analysis_cancel: Mutex<Option<Arc<AtomicBool>>>,
}

pub type Shared = Arc<AppState>;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalyzeProgress {
    phase: String,
    done: usize,
    total: usize,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Result<Self> {
        let db = Db::open(data_dir.join("atlas.db"))?;
        let holding = HoldingArea::new(data_dir.join("holding"));
        Ok(AppState {
            db,
            data_dir,
            holding,
            scan_cancel: Mutex::new(None),
            rescan_requested: AtomicBool::new(false),
            watcher: Mutex::new(None),
            analysis: Mutex::new(None),
            analysis_cancel: Mutex::new(None),
        })
    }

    pub fn settings(&self) -> Result<Settings> {
        Settings::load(&self.db.conn()?)
    }

    /// Directories Atlas owns; always excluded from indexing and cleanup.
    pub fn own_dirs(&self) -> Vec<PathBuf> {
        let mut v = vec![self.data_dir.clone()];
        v.extend(self.holding.all_dirs());
        v
    }

    pub fn exclusions(&self, s: &Settings) -> Exclusions {
        Exclusions::new(s, &self.own_dirs())
    }

    pub fn blocklist(&self) -> Blocklist {
        Blocklist::for_system(self.own_dirs())
    }

    pub fn is_scanning(&self) -> bool {
        self.scan_cancel.lock().unwrap().is_some()
    }

    pub fn is_analyzing(&self) -> bool {
        self.analysis_cancel.lock().unwrap().is_some()
    }
}

/// Start (or restart) the watcher for the configured roots.
pub fn start_watcher(app: &AppHandle, st: &Shared) -> Result<()> {
    let settings = st.settings()?;
    *st.watcher.lock().unwrap() = None;
    if settings.roots.is_empty() {
        return Ok(());
    }
    let excl = st.exclusions(&settings);
    let roots = crate::indexer::dedupe_roots(&settings.root_paths());
    let app_c = app.clone();
    let app_o = app.clone();
    let st_o = st.clone();
    let handle = crate::watcher::start(
        st.db.clone(),
        roots,
        excl,
        move |n| {
            let _ = app_c.emit("index://changed", n);
        },
        move |_root| {
            // The OS dropped events: reconcile with a full rescan.
            if !st_o.rescan_requested.swap(true, Ordering::SeqCst) {
                start_scan(&app_o, &st_o);
            }
        },
    )?;
    *st.watcher.lock().unwrap() = Some(handle);
    Ok(())
}

/// Kick off a background scan unless one is already running.
pub fn start_scan(app: &AppHandle, st: &Shared) {
    let cancel = {
        let mut guard = st.scan_cancel.lock().unwrap();
        if guard.is_some() {
            return;
        }
        let c = Arc::new(AtomicBool::new(false));
        *guard = Some(c.clone());
        c
    };
    let app = app.clone();
    let st = st.clone();
    std::thread::Builder::new()
        .name("atlas-scan".into())
        .spawn(move || {
            let result = (|| -> Result<crate::indexer::ScanProgress> {
                let settings = st.settings()?;
                let excl = st.exclusions(&settings);
                crate::indexer::scan(&st.db, &settings.root_paths(), &excl, &cancel, |p| {
                    let _ = app.emit("scan://progress", p);
                })
            })();
            *st.scan_cancel.lock().unwrap() = None;
            st.rescan_requested.store(false, Ordering::SeqCst);
            match result {
                Ok(p) => {
                    if let Ok(conn) = st.db.conn() {
                        let _ = kv_set(&conn, "last_scan_at", &now().to_string());
                    }
                    let _ = app.emit("scan://done", &p);
                    let _ = start_watcher(&app, &st);
                    // Refresh the reclaimable estimate if it is stale (>12 h).
                    let stale = st
                        .db
                        .conn()
                        .ok()
                        .and_then(|c| kv_get(&c, "cleanup_summary").ok().flatten())
                        .and_then(|s| serde_json::from_str::<crate::cleanup::CleanupSummary>(&s).ok())
                        .map(|s| now() - s.analyzed_at > 12 * 3600)
                        .unwrap_or(true);
                    if stale && !p.cancelled {
                        start_analysis(&app, &st);
                    }
                }
                Err(e) => {
                    let p = crate::indexer::ScanProgress {
                        phase: "error".into(),
                        done: true,
                        error: Some(e.to_string()),
                        ..Default::default()
                    };
                    let _ = app.emit("scan://done", &p);
                }
            }
        })
        .expect("spawn scan thread");
}

pub fn cancel_scan(st: &Shared) {
    if let Some(c) = st.scan_cancel.lock().unwrap().as_ref() {
        c.store(true, Ordering::SeqCst);
    }
}

/// Run a cleanup analysis in the background; results are kept in memory for
/// the review screen and the summary is cached in the DB for the dashboard.
pub fn start_analysis(app: &AppHandle, st: &Shared) {
    let cancel = {
        let mut guard = st.analysis_cancel.lock().unwrap();
        if guard.is_some() {
            return;
        }
        let c = Arc::new(AtomicBool::new(false));
        *guard = Some(c.clone());
        c
    };
    let app = app.clone();
    let st = st.clone();
    std::thread::Builder::new()
        .name("atlas-analyze".into())
        .spawn(move || {
            let result = (|| -> Result<Analysis> {
                let conn = st.db.conn()?;
                let settings = Settings::load(&conn)?;
                let env = AnalyzeEnv::system();
                let bl = st.blocklist();
                let mut last = std::time::Instant::now();
                analyze::run(&conn, &settings, &env, &bl, &cancel, |phase, done, total| {
                    if last.elapsed().as_millis() > 200 || done == 0 {
                        last = std::time::Instant::now();
                        let _ = app.emit("cleanup://progress", AnalyzeProgress { phase: phase.into(), done, total });
                    }
                })
            })();
            let cancelled = cancel.load(Ordering::SeqCst);
            *st.analysis_cancel.lock().unwrap() = None;
            match result {
                Ok(a) if !cancelled => {
                    if let Ok(conn) = st.db.conn() {
                        let _ = kv_set(&conn, "cleanup_summary", &serde_json::to_string(&a.summary).unwrap_or_default());
                    }
                    let summary = a.summary.clone();
                    *st.analysis.lock().unwrap() = Some(a);
                    let _ = app.emit("cleanup://done", summary);
                }
                Ok(_) => {
                    let _ = app.emit("cleanup://done", serde_json::Value::Null);
                }
                Err(e) => {
                    let _ = app.emit("cleanup://error", e.to_string());
                }
            }
        })
        .expect("spawn analysis thread");
}

/// Hourly housekeeping: daily snapshot, holding-area retention, event pruning.
pub fn maintenance(st: &Shared) -> Result<()> {
    let conn = st.db.conn()?;
    if !st.is_scanning() && !crate::snapshots::has_today(&conn)? {
        let has_index: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM files)", [], |r| r.get(0))?;
        if has_index {
            crate::snapshots::take(&conn)?;
        }
    }
    holding::purge_expired(&conn, now(), holding::system_trash)?;
    conn.execute("DELETE FROM events WHERE ts < ?1", [now() - 180 * 86_400])?;
    conn.execute("DELETE FROM cleanup_log WHERE ts < ?1", [now() - 365 * 86_400])?;
    Ok(())
}
