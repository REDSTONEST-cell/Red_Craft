//! 资源管理：实例中的资源包、光影包与存档（Mod 见 content.rs 中的「已安装 Mod 管理」）。
//!
//! 列出、启用资源包 / 切换光影、导入文件、删除（移到回收站）。

use std::path::PathBuf;

use launcher_core::content::files::{self, FileKind, LocalFile};
use launcher_core::instance::detect_optifine;
use tauri::State;

use crate::content::{ensure_not_running, instance_context};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| AppError::msg(format!("后台任务异常：{e}"), "请重试"))
}

#[tauri::command]
pub async fn files_list(state: State<'_, AppState>, instance: String, kind: FileKind) -> AppResult<Vec<LocalFile>> {
    let dir = instance_context(&state, &instance)?.game_dir;
    blocking(move || files::scan(&dir, kind)).await
}

/// 启用 / 停用资源包，或切换光影（`active` 为 false 时关闭光影）。
#[tauri::command]
pub fn file_set_active(state: State<'_, AppState>, instance: String, kind: FileKind, name: String, active: bool) -> AppResult<()> {
    if state.running.lock().is_ok_and(|r| r.contains_key(&instance)) {
        return Err(AppError::msg("游戏正在运行", "游戏退出时会覆盖这项设置，请先关闭游戏，或在游戏内切换。"));
    }
    let ctx = instance_context(&state, &instance)?;
    let game = state.game_dir();
    let resolved = launcher_core::version::resolve_version(&game, &instance)?;
    // 还没有 options.txt 时先写入默认语言，避免第一次启动时被当作已有设置而不再设置中文
    if state.settings().default_chinese {
        launcher_core::launch::ensure_chinese(&ctx.game_dir, &resolved.json.release_time)?;
    }
    files::entry_path(&ctx.game_dir, kind, &name)?;
    match kind {
        FileKind::ResourcePack => {
            files::set_resource_pack(&ctx.game_dir, &name, active, files::legacy_pack_names(&ctx.mc_version))?
        }
        FileKind::Shader => {
            let mods = launcher_core::content::local::scan_mods(&ctx.game_dir.join("mods"));
            let has = |id: &str| mods.iter().any(|m| m.enabled && m.mod_id == id);
            let iris = has("iris") || has("oculus");
            let optifine = detect_optifine(&resolved).is_some() || has("optifine") || has("optifabric");
            files::set_shader(&ctx.game_dir, active.then_some(name.as_str()), iris, optifine)?
        }
        _ => return Err(AppError::msg("这类资源不能启用或停用", "")),
    }
    Ok(())
}

/// 把文件或文件夹导入到实例中，返回导入后的名称。
#[tauri::command]
pub async fn files_import(state: State<'_, AppState>, instance: String, kind: FileKind, paths: Vec<PathBuf>) -> AppResult<Vec<String>> {
    let dir = instance_context(&state, &instance)?.game_dir;
    blocking(move || {
        paths
            .iter()
            .map(|p| files::import(&dir, kind, p).map_err(AppError::from))
            .collect::<AppResult<Vec<String>>>()
    })
    .await?
}

/// 删除：移到系统回收站，误删时可以找回。
#[tauri::command]
pub async fn files_delete(state: State<'_, AppState>, instance: String, kind: FileKind, names: Vec<String>) -> AppResult<()> {
    ensure_not_running(&state, &instance)?;
    let dir = instance_context(&state, &instance)?.game_dir;
    let paths = names
        .iter()
        .map(|n| files::entry_path(&dir, kind, n))
        .collect::<Result<Vec<_>, _>>()?;
    blocking(move || {
        trash::delete_all(&paths).map_err(|e| {
            AppError::msg(
                format!("无法移到回收站：{e}"),
                "文件可能正被游戏或其他程序占用；也可以点击「打开文件夹」手动删除。",
            )
        })
    })
    .await?
}
