//! Tauri 外壳：把 launcher-core 包装成 commands 与 events。

mod auth;
mod commands;
mod content;
mod error;
mod launch;
mod logger;
mod migrate;
mod secrets;
mod settings;
mod state;
mod tasks;
mod ygg;

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
            // 后台补全离线账号的同名正版皮肤（添加账号时可能因网络问题没有查到）
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                commands::refresh_missing_skins(&handle).await;
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
            commands::optifine_versions,
            content::shader_mod_for,
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
            commands::refresh_offline_skin,
            commands::select_account,
            commands::scan_java,
            commands::probe_java,
            commands::cancel_task,
            auth::ms_login_start,
            auth::ms_login_finish,
            auth::ms_login_cancel,
            auth::ms_browser_start,
            auth::ms_browser_finish,
            auth::ms_refresh_account,
            migrate::import_locations,
            migrate::import_scan,
            migrate::import_instances,
            migrate::import_accounts,
            ygg::ygg_resolve,
            ygg::ygg_login,
            ygg::ygg_select,
            ygg::ygg_cancel,
            ygg::ygg_refresh_account,
            content::content_search,
            content::content_instance,
            content::content_versions,
            content::content_project,
            content::content_plan,
            content::content_install,
            content::read_modpack_file,
            content::install_modpack,
            content::mods_list,
            content::mod_set_enabled,
            content::mod_delete,
            content::mods_check_updates,
            content::mods_update,
            launch::launch_instance,
            launch::kill_game,
        ])
        .run(tauri::generate_context!())
        .expect("启动 Red Craft 失败");
}
