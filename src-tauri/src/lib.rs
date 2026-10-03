pub mod classify;
pub mod cleanup;
pub mod commands;
pub mod crashlog;
pub mod dashboard;
pub mod db;
pub mod folders;
pub mod indexer;
pub mod model;
pub mod noise;
pub mod platform;
pub mod preview;
pub mod search;
pub mod settings;
pub mod snapshots;
pub mod spaces;
pub mod state;
pub mod timeline;
pub mod watcher;

#[cfg(test)]
mod smoke_tests;
#[cfg(test)]
mod watcher_tests;

use std::path::PathBuf;
use std::sync::Arc;
use tauri::Manager;

/// Where Atlas keeps its data: `ATLAS_DATA_DIR` (an isolated profile, e.g. for
/// QA next to a normal instance) or the per-user local app data folder for
/// this app's identifier (the same folder Tauri's `app_local_data_dir` uses).
fn data_dir_for(identifier: &str) -> PathBuf {
    match std::env::var_os("ATLAS_DATA_DIR") {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join(identifier),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let context = tauri::generate_context!();
    let data_dir = data_dir_for(&context.config().identifier);
    // Crash logging starts before anything else so even startup panics land
    // on disk; a leftover session marker means the last run crashed natively.
    let log = Arc::new(crashlog::CrashLog::new(
        data_dir.join("logs"),
        crashlog::DEFAULT_MAX_BYTES,
        crashlog::DEFAULT_KEEP,
    ));
    crashlog::install(log.clone());
    log.begin_session();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(move |app| {
            let st: state::Shared = Arc::new(state::AppState::new(data_dir)?);
            app.manage(st.clone());

            let handle = app.handle().clone();
            if st.settings()?.onboarded {
                // Catch up on changes made while Atlas was closed, then watch.
                let _ = state::start_watcher(&handle, &st);
                state::start_scan(&handle, &st);
            }

            // Hourly maintenance: daily snapshot, holding retention, pruning.
            let st_m = st.clone();
            tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
                loop {
                    tick.tick().await;
                    let st = st_m.clone();
                    let _ = tauri::async_runtime::spawn_blocking(move || state::maintenance(&st)).await;
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::default_settings,
            commands::save_settings,
            commands::save_appearance,
            commands::log_client_error,
            commands::open_logs_folder,
            commands::app_status,
            commands::start_scan,
            commands::cancel_scan,
            commands::search,
            commands::dashboard_metrics,
            commands::jump_back_in,
            commands::treemap,
            commands::growth_series,
            commands::inbox_items,
            commands::inbox_dismiss,
            commands::archive_files,
            commands::open_file,
            commands::reveal_file,
            commands::file_details,
            commands::read_text_preview,
            commands::preview_probe,
            commands::list_tags,
            commands::add_tag,
            commands::remove_tag,
            commands::list_spaces,
            commands::save_space,
            commands::delete_space,
            commands::pin_space,
            commands::space_files,
            commands::add_to_space,
            commands::remove_from_space,
            commands::timeline,
            commands::places,
            commands::list_dir,
            commands::list_subdirs,
            commands::cleanup_analyze,
            commands::cleanup_cancel,
            commands::cleanup_summary,
            commands::cleanup_items,
            commands::cleanup_run,
            commands::move_to_holding,
            commands::holding_list,
            commands::holding_restore,
            commands::holding_purge_expired,
            commands::cleanup_log,
        ])
        .build(context)
        .expect("error while building Atlas");
    app.run(move |_, event| {
        if let tauri::RunEvent::Exit = event {
            log.end_session();
        }
    });
}
