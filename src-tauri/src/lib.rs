pub mod cleanup;
pub mod commands;
pub mod dashboard;
pub mod db;
pub mod folders;
pub mod indexer;
pub mod model;
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

use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let data_dir = app.path().app_local_data_dir()?;
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
        .run(tauri::generate_context!())
        .expect("error while running Atlas");
}
