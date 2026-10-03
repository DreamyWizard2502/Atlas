//! Thin `#[tauri::command]` layer. Every command is async and pushes its work
//! onto a blocking thread so the UI thread never waits on disk or SQLite.

use crate::cleanup::holding::{self, RunContext};
use crate::cleanup::{Candidate, CleanItem, CleanupSummary, ItemOutcome, RunReport};
use crate::db::{kv_get, now};
use crate::settings::Settings;
use crate::state::{self, Shared};
use crate::{dashboard, folders, preview, search, spaces, timeline};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, State};

type CmdResult<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

// ---------------- Settings & status ----------------

#[tauri::command]
pub async fn get_settings(st: State<'_, Shared>) -> CmdResult<Settings> {
    let st = st.inner().clone();
    blocking(move || st.settings()).await
}

#[tauri::command]
pub async fn default_settings() -> CmdResult<Settings> {
    Ok(Settings::default())
}

#[tauri::command]
pub async fn save_settings(app: AppHandle, st: State<'_, Shared>, settings: Settings) -> CmdResult<Settings> {
    let st = st.inner().clone();
    let st2 = st.clone();
    let (saved, rescan) = blocking(move || {
        let conn = st.db.conn()?;
        let old = Settings::load(&conn)?;
        let mut s = settings;
        s.roots.retain(|r| !r.trim().is_empty());
        s.excluded.retain(|r| !r.trim().is_empty());
        s.save(&conn)?;
        let rescan = s.onboarded && (old.roots != s.roots || old.excluded != s.excluded || !old.onboarded);
        Ok((s, rescan))
    })
    .await?;
    if rescan {
        state::cancel_scan(&st2);
        // Wait briefly for a running scan to stop, then start fresh.
        let st3 = st2.clone();
        tauri::async_runtime::spawn(async move {
            for _ in 0..50 {
                if !st3.is_scanning() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            state::start_scan(&app, &st3);
        });
    }
    Ok(saved)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    onboarded: bool,
    scanning: bool,
    analyzing: bool,
    indexed_files: i64,
    last_scan_at: Option<i64>,
    data_dir: String,
    holding_dir: String,
    home: Option<String>,
}

#[tauri::command]
pub async fn app_status(st: State<'_, Shared>) -> CmdResult<AppStatus> {
    let st = st.inner().clone();
    blocking(move || {
        let conn = st.db.conn()?;
        let s = Settings::load(&conn)?;
        let indexed_files: i64 =
            conn.query_row("SELECT COALESCE(SUM(file_count), 0) FROM files WHERE path = root", [], |r| r.get(0))?;
        Ok(AppStatus {
            onboarded: s.onboarded,
            scanning: st.is_scanning(),
            analyzing: st.is_analyzing(),
            indexed_files,
            last_scan_at: kv_get(&conn, "last_scan_at")?.and_then(|v| v.parse().ok()),
            data_dir: st.data_dir.to_string_lossy().to_string(),
            holding_dir: st.holding.primary.to_string_lossy().to_string(),
            home: crate::platform::home_dir().map(|h| h.to_string_lossy().to_string()),
        })
    })
    .await
}

#[tauri::command]
pub async fn start_scan(app: AppHandle, st: State<'_, Shared>) -> CmdResult<()> {
    state::start_scan(&app, st.inner());
    Ok(())
}

#[tauri::command]
pub async fn cancel_scan(st: State<'_, Shared>) -> CmdResult<()> {
    state::cancel_scan(st.inner());
    Ok(())
}

// ---------------- Search ----------------

#[tauri::command]
pub async fn search(st: State<'_, Shared>, query: String, limit: Option<i64>) -> CmdResult<search::SearchResult> {
    let st = st.inner().clone();
    blocking(move || search::run(&st.db.conn()?, &query, limit.unwrap_or(50).clamp(1, 500))).await
}

// ---------------- Dashboard ----------------

#[tauri::command]
pub async fn dashboard_metrics(st: State<'_, Shared>) -> CmdResult<dashboard::Metrics> {
    let st = st.inner().clone();
    blocking(move || {
        let conn = st.db.conn()?;
        dashboard::metrics(&conn, &Settings::load(&conn)?)
    })
    .await
}

#[tauri::command]
pub async fn jump_back_in(st: State<'_, Shared>, limit: Option<usize>) -> CmdResult<Vec<dashboard::Session>> {
    let st = st.inner().clone();
    blocking(move || {
        let conn = st.db.conn()?;
        let s = Settings::load(&conn)?;
        dashboard::jump_back_in(&conn, limit.unwrap_or(8), s.session_gap_min)
    })
    .await
}

#[tauri::command]
pub async fn treemap(st: State<'_, Shared>, path: Option<String>) -> CmdResult<dashboard::Treemap> {
    let st = st.inner().clone();
    blocking(move || dashboard::treemap(&st.db.conn()?, path.filter(|p| !p.is_empty()), 60)).await
}

#[tauri::command]
pub async fn growth_series(st: State<'_, Shared>, days: Option<i64>) -> CmdResult<dashboard::Growth> {
    let st = st.inner().clone();
    blocking(move || dashboard::growth(&st.db.conn()?, days.unwrap_or(90))).await
}

#[tauri::command]
pub async fn inbox_items(st: State<'_, Shared>, limit: Option<usize>) -> CmdResult<dashboard::Inbox> {
    let st = st.inner().clone();
    blocking(move || {
        let conn = st.db.conn()?;
        dashboard::inbox(&conn, &Settings::load(&conn)?, limit.unwrap_or(500))
    })
    .await
}

#[tauri::command]
pub async fn inbox_dismiss(st: State<'_, Shared>, paths: Vec<String>) -> CmdResult<()> {
    let st = st.inner().clone();
    blocking(move || {
        let conn = st.db.conn()?;
        for p in paths {
            conn.execute(
                "INSERT OR REPLACE INTO inbox_dismissed(path, at) VALUES (?1, ?2)",
                rusqlite::params![p, now()],
            )?;
        }
        Ok(())
    })
    .await
}

/// Move files into the archive folder (a dated subfolder). A real move, never
/// a delete; cross-drive moves are refused rather than copied.
#[tauri::command]
pub async fn archive_files(st: State<'_, Shared>, paths: Vec<String>) -> CmdResult<Vec<ItemOutcome>> {
    let st = st.inner().clone();
    blocking(move || {
        let conn = st.db.conn()?;
        let s = Settings::load(&conn)?;
        if s.archive_dir.trim().is_empty() {
            anyhow::bail!("Set an archive folder in Settings first");
        }
        let dest_dir = PathBuf::from(&s.archive_dir).join(chrono::Local::now().format("%Y-%m").to_string());
        std::fs::create_dir_all(&dest_dir)?;
        let batch = holding::new_batch_id();
        let bl = st.blocklist();
        let mut out = Vec::new();
        for p in paths {
            let src = Path::new(&p);
            if let Err(reason) = bl.check_live(src) {
                out.push(ItemOutcome { path: p, ok: false, detail: reason });
                continue;
            }
            let name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let mut dest = dest_dir.join(&name);
            let mut n = 2;
            while dest.exists() {
                let stem = Path::new(&name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                let ext = Path::new(&name).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
                dest = dest_dir.join(format!("{stem} ({n}){ext}"));
                n += 1;
            }
            match std::fs::rename(src, &dest) {
                Ok(()) => {
                    holding::log(&conn, &batch, "archive", &p, &dest.to_string_lossy(), false)?;
                    out.push(ItemOutcome { path: p, ok: true, detail: format!("Archived to {}", dest.display()) });
                }
                Err(e) => {
                    let detail = if e.raw_os_error() == Some(17) {
                        "Archive folder is on another drive; not moving".to_string()
                    } else {
                        format!("Could not move: {e}")
                    };
                    out.push(ItemOutcome { path: p, ok: false, detail });
                }
            }
        }
        Ok(out)
    })
    .await
}

// ---------------- Files ----------------

#[tauri::command]
pub async fn open_file(st: State<'_, Shared>, path: String) -> CmdResult<()> {
    let st = st.inner().clone();
    blocking(move || {
        tauri_plugin_opener::open_path(&path, None::<&str>)?;
        let conn = st.db.conn()?;
        conn.execute(
            "INSERT INTO activity(path, opened_at) VALUES (?1, ?2) ON CONFLICT(path) DO UPDATE SET opened_at = excluded.opened_at",
            rusqlite::params![path, now()],
        )?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn reveal_file(path: String) -> CmdResult<()> {
    blocking(move || Ok(tauri_plugin_opener::reveal_item_in_dir(&path)?)).await
}

#[tauri::command]
pub async fn file_details(st: State<'_, Shared>, path: String) -> CmdResult<preview::FileDetails> {
    let st = st.inner().clone();
    blocking(move || preview::details(&st.db.conn()?, &path)).await
}

#[tauri::command]
pub async fn read_text_preview(path: String, max_bytes: Option<usize>) -> CmdResult<preview::TextPreview> {
    blocking(move || preview::read_text(&path, max_bytes.unwrap_or(256 * 1024).min(4 << 20))).await
}

// ---------------- Tags ----------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagInfo {
    id: i64,
    name: String,
    count: i64,
}

#[tauri::command]
pub async fn list_tags(st: State<'_, Shared>) -> CmdResult<Vec<TagInfo>> {
    let st = st.inner().clone();
    blocking(move || {
        Ok(spaces::list_tags(&st.db.conn()?)?
            .into_iter()
            .map(|(id, name, count)| TagInfo { id, name, count })
            .collect())
    })
    .await
}

#[tauri::command]
pub async fn add_tag(st: State<'_, Shared>, paths: Vec<String>, tag: String) -> CmdResult<()> {
    let st = st.inner().clone();
    blocking(move || {
        let conn = st.db.conn()?;
        for p in paths {
            spaces::add_tag(&conn, &p, &tag)?;
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn remove_tag(st: State<'_, Shared>, path: String, tag: String) -> CmdResult<()> {
    let st = st.inner().clone();
    blocking(move || spaces::remove_tag(&st.db.conn()?, &path, &tag)).await
}

// ---------------- Spaces ----------------

#[tauri::command]
pub async fn list_spaces(st: State<'_, Shared>) -> CmdResult<Vec<spaces::Space>> {
    let st = st.inner().clone();
    blocking(move || spaces::list(&st.db.conn()?)).await
}

#[tauri::command]
pub async fn save_space(st: State<'_, Shared>, space: spaces::SpaceInput) -> CmdResult<i64> {
    let st = st.inner().clone();
    blocking(move || spaces::save(&st.db.conn()?, &space)).await
}

#[tauri::command]
pub async fn delete_space(st: State<'_, Shared>, id: i64) -> CmdResult<()> {
    let st = st.inner().clone();
    blocking(move || spaces::delete(&st.db.conn()?, id)).await
}

#[tauri::command]
pub async fn pin_space(st: State<'_, Shared>, id: i64, pinned: bool) -> CmdResult<()> {
    let st = st.inner().clone();
    blocking(move || spaces::set_pinned(&st.db.conn()?, id, pinned)).await
}

#[tauri::command]
pub async fn space_files(
    st: State<'_, Shared>,
    id: i64,
    sort: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> CmdResult<spaces::SpaceFiles> {
    let st = st.inner().clone();
    blocking(move || {
        spaces::files(&st.db.conn()?, id, sort.as_deref().unwrap_or("recent"), limit.unwrap_or(500), offset.unwrap_or(0))
    })
    .await
}

#[tauri::command]
pub async fn add_to_space(st: State<'_, Shared>, id: i64, paths: Vec<String>) -> CmdResult<()> {
    let st = st.inner().clone();
    blocking(move || {
        let conn = st.db.conn()?;
        for p in paths {
            spaces::add_item(&conn, id, &p)?;
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn remove_from_space(st: State<'_, Shared>, id: i64, path: String) -> CmdResult<()> {
    let st = st.inner().clone();
    blocking(move || spaces::remove_item(&st.db.conn()?, id, &path)).await
}

// ---------------- Timeline ----------------

#[tauri::command]
pub async fn timeline(
    st: State<'_, Shared>,
    before: Option<i64>,
    limit: Option<i64>,
    filter: Option<timeline::TimelineFilter>,
) -> CmdResult<Vec<timeline::TimelineEntry>> {
    let st = st.inner().clone();
    blocking(move || timeline::page(&st.db.conn()?, before, limit.unwrap_or(200), &filter.unwrap_or_default())).await
}

// ---------------- Folders ----------------

#[tauri::command]
pub async fn places(st: State<'_, Shared>) -> CmdResult<Vec<folders::Place>> {
    let st = st.inner().clone();
    blocking(move || Ok(folders::places(&st.settings()?))).await
}

#[tauri::command]
pub async fn list_dir(st: State<'_, Shared>, path: String) -> CmdResult<Vec<folders::DirEntry>> {
    let st = st.inner().clone();
    blocking(move || folders::list_dir(&st.db.conn()?, &path)).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Subdir {
    name: String,
    path: String,
}

#[tauri::command]
pub async fn list_subdirs(path: String) -> CmdResult<Vec<Subdir>> {
    blocking(move || {
        Ok(folders::list_subdirs(&path)?.into_iter().map(|(name, path)| Subdir { name, path }).collect())
    })
    .await
}

// ---------------- Cleanup ----------------

#[tauri::command]
pub async fn cleanup_analyze(app: AppHandle, st: State<'_, Shared>) -> CmdResult<()> {
    state::start_analysis(&app, st.inner());
    Ok(())
}

#[tauri::command]
pub async fn cleanup_cancel(st: State<'_, Shared>) -> CmdResult<()> {
    if let Some(c) = st.analysis_cancel.lock().unwrap().as_ref() {
        c.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupState {
    summary: Option<CleanupSummary>,
    /// True when item lists are in memory (summary alone may be from a previous session).
    has_items: bool,
    analyzing: bool,
}

#[tauri::command]
pub async fn cleanup_summary(st: State<'_, Shared>) -> CmdResult<CleanupState> {
    let st = st.inner().clone();
    blocking(move || {
        let has_items = st.analysis.lock().unwrap().is_some();
        let summary = match st.analysis.lock().unwrap().as_ref() {
            Some(a) => Some(a.summary.clone()),
            None => kv_get(&st.db.conn()?, "cleanup_summary")?.and_then(|s| serde_json::from_str(&s).ok()),
        };
        Ok(CleanupState { summary, has_items, analyzing: st.is_analyzing() })
    })
    .await
}

#[tauri::command]
pub async fn cleanup_items(st: State<'_, Shared>, category: String) -> CmdResult<Vec<Candidate>> {
    let guard = st.analysis.lock().unwrap();
    Ok(guard.as_ref().and_then(|a| a.items.get(&category).cloned()).unwrap_or_default())
}

#[tauri::command]
pub async fn cleanup_run(st: State<'_, Shared>, items: Vec<CleanItem>, dry_run: bool) -> CmdResult<RunReport> {
    let st = st.inner().clone();
    blocking(move || {
        let conn = st.db.conn()?;
        let settings = Settings::load(&conn)?;
        let groups: HashMap<String, Vec<String>> =
            st.analysis.lock().unwrap().as_ref().map(|a| a.dup_groups.clone()).unwrap_or_default();
        let bl = st.blocklist();
        let ctx = RunContext { blocklist: &bl, holding: &st.holding, retention_days: settings.retention_days, dup_groups: &groups };
        let report = holding::run(&conn, &ctx, &items, dry_run)?;
        if !dry_run {
            // Drop cleaned items from the in-memory review lists and summary.
            let moved: std::collections::HashSet<&str> =
                report.outcomes.iter().filter(|o| o.ok).map(|o| o.path.as_str()).collect();
            if let Some(a) = st.analysis.lock().unwrap().as_mut() {
                for list in a.items.values_mut() {
                    list.retain(|c| !moved.contains(c.path.as_str()));
                }
                for cat in a.summary.categories.iter_mut() {
                    let list = a.items.get(&cat.id).map(|v| v.as_slice()).unwrap_or(&[]);
                    if cat.id == "duplicates" {
                        let mut by_group: HashMap<u32, (i64, i64)> = HashMap::new();
                        for c in list {
                            let e = by_group.entry(c.group.unwrap_or(0)).or_default();
                            e.0 = c.size;
                            e.1 += 1;
                        }
                        cat.count = by_group.values().filter(|(_, n)| *n > 1).count();
                        cat.reclaimable = by_group.values().map(|(s, n)| s * (n - 1).max(0)).sum();
                    } else {
                        cat.count = list.len();
                        cat.reclaimable = list.iter().map(|c| c.size).sum();
                    }
                }
                a.summary.total_reclaimable = (a.summary.total_reclaimable - report.bytes).max(0);
                let _ = crate::db::kv_set(&conn, "cleanup_summary", &serde_json::to_string(&a.summary)?);
            }
        }
        Ok(report)
    })
    .await
}

/// Context-menu "move to holding area" for individual files.
#[tauri::command]
pub async fn move_to_holding(st: State<'_, Shared>, paths: Vec<String>) -> CmdResult<RunReport> {
    let st = st.inner().clone();
    blocking(move || {
        let conn = st.db.conn()?;
        let settings = Settings::load(&conn)?;
        let items: Vec<CleanItem> = paths
            .into_iter()
            .map(|p| {
                let size = std::fs::metadata(&p).map(|m| m.len() as i64).unwrap_or(-1);
                CleanItem { path: p, category: "manual".into(), size }
            })
            .collect();
        let groups = HashMap::new();
        let bl = st.blocklist();
        let ctx = RunContext { blocklist: &bl, holding: &st.holding, retention_days: settings.retention_days, dup_groups: &groups };
        holding::run(&conn, &ctx, &items, false)
    })
    .await
}

#[tauri::command]
pub async fn holding_list(st: State<'_, Shared>, include_done: Option<bool>) -> CmdResult<Vec<holding::HeldItem>> {
    let st = st.inner().clone();
    blocking(move || holding::list(&st.db.conn()?, include_done.unwrap_or(false))).await
}

#[tauri::command]
pub async fn holding_restore(st: State<'_, Shared>, ids: Vec<i64>) -> CmdResult<Vec<ItemOutcome>> {
    let st = st.inner().clone();
    blocking(move || holding::restore(&st.db.conn()?, &ids)).await
}

#[tauri::command]
pub async fn holding_purge_expired(st: State<'_, Shared>) -> CmdResult<usize> {
    let st = st.inner().clone();
    blocking(move || holding::purge_expired(&st.db.conn()?, now(), holding::system_trash)).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    id: i64,
    ts: i64,
    batch_id: String,
    action: String,
    path: String,
    detail: String,
    dry_run: bool,
}

#[tauri::command]
pub async fn cleanup_log(st: State<'_, Shared>, limit: Option<i64>) -> CmdResult<Vec<LogEntry>> {
    let st = st.inner().clone();
    blocking(move || {
        let conn = st.db.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, ts, batch_id, action, path, detail, dry_run FROM cleanup_log ORDER BY id DESC LIMIT ?1",
        )?;
        let v = stmt
            .query_map([limit.unwrap_or(500)], |r| {
                Ok(LogEntry {
                    id: r.get(0)?,
                    ts: r.get(1)?,
                    batch_id: r.get(2)?,
                    action: r.get(3)?,
                    path: r.get(4)?,
                    detail: r.get(5)?,
                    dry_run: r.get(6)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(v)
    })
    .await
}
