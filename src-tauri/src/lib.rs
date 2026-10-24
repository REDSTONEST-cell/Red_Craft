//! Tauri 外壳：把 launcher-core 包装成 commands 与 events。

mod auth;
mod commands;
mod error;
mod launch;
mod logger;
mod secrets;
mod settings;
mod state;
mod tasks;

use state::AppState;

pub fn run() {
    let state = AppState::load();
    logger::init(&state.paths.config_dir);
    log::info!(
        "Red Craft {} 启动，游戏目录 {}，便携模式 {}",
        launcher_core::LAUNCHER_VERSION,
        state.game_dir().root().display(),
        state.paths.portable
    );
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .setup(|app| {
            // 保险：前端若因异常没有通知就绪，2 秒后仍然显示窗口
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                if let Some(w) = tauri::Manager::get_webview_window(&handle, "main") {
                    if !w.is_visible().unwrap_or(true) {
                        let _ = w.show();
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::app_ready,
            commands::get_settings,
            commands::save_settings,
            commands::version_list,
            commands::list_instances,
            commands::loader_versions,
            commands::install_instance,
            commands::repair_instance,
            commands::delete_instance,
            commands::get_instance_config,
            commands::save_instance_config,
            commands::select_instance,
            commands::open_instance_dir,
            commands::open_game_dir,
            commands::open_file,
            commands::read_game_file,
            commands::list_accounts,
            commands::add_offline_account,
            commands::remove_account,
            commands::select_account,
            commands::scan_java,
            commands::probe_java,
            commands::cancel_task,
            auth::ms_login_start,
            auth::ms_login_finish,
            auth::ms_login_cancel,
            auth::ms_refresh_account,
            launch::launch_instance,
            launch::kill_game,
        ])
        .run(tauri::generate_context!())
        .expect("启动 Red Craft 失败");
}
