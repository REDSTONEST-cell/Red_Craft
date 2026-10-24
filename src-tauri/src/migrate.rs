//! 从其他启动器导入实例与离线账号。

use std::path::PathBuf;

use launcher_core::auth::Account;
use launcher_core::content::PackLoader;
use launcher_core::instance::LoaderKind;
use launcher_core::migrate::{self, FilesLayout, ImportLocation, ScanResult};
use serde::Deserialize;
use tauri::{AppHandle, Manager, State};

use crate::commands::{blocking, create_instance, manifest};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// 本机上能找到的其他启动器数据位置。
#[tauri::command]
pub async fn import_locations(state: State<'_, AppState>) -> AppResult<Vec<ImportLocation>> {
    let current = state.game_dir().root().to_path_buf();
    blocking(move || migrate::detect_locations(&current)).await
}

#[tauri::command]
pub async fn import_scan(path: PathBuf) -> AppResult<ScanResult> {
    Ok(blocking(move || migrate::scan(&path)).await??)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportLoader {
    kind: LoaderKind,
    version: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportItem {
    /// 原启动器中的名称（`.minecraft` 中即版本名）
    source_name: String,
    mc_version: String,
    loader: Option<ImportLoader>,
    files_dir: PathBuf,
    layout: FilesLayout,
}

/// 生成不与现有实例重名、且符合命名规则的实例名。
fn unique_name(state: &AppState, name: &str) -> String {
    const BAD: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    let base: String = name
        .chars()
        .map(|c| if BAD.contains(&c) || c.is_control() { '_' } else { c })
        .collect::<String>()
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .to_string();
    let base = if base.is_empty() { "导入的实例".to_string() } else { base };
    let game = state.game_dir();
    if !game.version_dir(&base).exists() {
        return base;
    }
    (2..)
        .map(|i| format!("{base} ({i})"))
        .find(|n| !game.version_dir(n).exists())
        .unwrap_or(base)
}

/// 每个实例一个后台任务：安装相同的游戏版本与加载器（强制版本隔离），再复制用户数据。返回任务 id。
#[tauri::command]
pub async fn import_instances(app: AppHandle, state: State<'_, AppState>, items: Vec<ImportItem>) -> AppResult<Vec<u64>> {
    let m = manifest(&state, false).await?;
    let mut ids = Vec::new();
    // 先占用名称，避免同一批次中重名
    let mut taken: Vec<String> = Vec::new();
    for item in items {
        let mut name = unique_name(&state, &item.source_name);
        let mut i = 2;
        while taken.contains(&name) {
            name = format!("{} ({i})", unique_name(&state, &item.source_name));
            i += 1;
        }
        taken.push(name.clone());
        let entry = m.find(&item.mc_version).cloned().ok_or_else(|| {
            AppError::msg(
                format!("「{}」的游戏版本 {} 不在版本列表中", item.source_name, item.mc_version),
                "请刷新版本列表后重试；如果是自定义版本，请在原启动器中确认它对应的游戏版本。",
            )
        })?;
        let installer = state.installer()?;
        let app2 = app.clone();
        let id = state.tasks.spawn(&app, format!("导入 {name}"), move |reporter, cancel| async move {
            let state = app2.state::<AppState>();
            let choice = match &item.loader {
                Some(l) => Some(
                    crate::content::loader_choice(
                        &state,
                        &entry.id,
                        &PackLoader {
                            kind: l.kind,
                            version: l.version.clone(),
                        },
                    )
                    .await?,
                ),
                None => None,
            };
            create_instance(&state, &installer, &entry, choice.as_ref(), &name, Some(true), &reporter, &cancel).await?;

            reporter.stage("复制 Mod、存档与配置");
            let to = installer.game.instance_game_dir(&name, true);
            let (from, layout, source) = (item.files_dir.clone(), item.layout, item.source_name.clone());
            let (r2, c2) = (reporter.clone(), cancel.clone());
            let result = blocking(move || {
                let entries = migrate::files_to_copy(&from, layout, Some(&source));
                migrate::copy_user_files(
                    &entries,
                    &from,
                    &to,
                    &mut |done, total| r2.stage(&format!("复制 Mod、存档与配置（{done}/{total}）")),
                    &|| c2.is_cancelled(),
                )
            })
            .await
            .and_then(|r| r.map_err(AppError::from));
            match result {
                Ok(stats) => {
                    log::info!("导入实例 {name}：复制 {} 个文件，{} 字节", stats.files, stats.bytes);
                    Ok(())
                }
                Err(e) => {
                    let _ = std::fs::remove_dir_all(installer.game.version_dir(&name));
                    Err(e)
                }
            }
        });
        ids.push(id);
    }
    Ok(ids)
}

/// 导入离线账号，返回新添加的数量（已存在的跳过）。头像在后台查询。
#[tauri::command]
pub async fn import_accounts(app: AppHandle, state: State<'_, AppState>, names: Vec<String>) -> AppResult<usize> {
    let mut added = 0;
    {
        let mut store = state
            .accounts
            .write()
            .map_err(|_| AppError::msg("账号数据被锁定", "请重启启动器"))?;
        for name in names {
            let name = name.trim();
            if launcher_core::auth::validate_offline_name(name).is_err() {
                continue;
            }
            let account = Account::offline(name);
            if !store.accounts.iter().any(|a| a.id == account.id) {
                store.accounts.push(account);
                added += 1;
            }
        }
    }
    state.save_accounts()?;
    if added > 0 {
        tauri::async_runtime::spawn(async move {
            crate::commands::refresh_missing_skins(&app).await;
        });
    }
    Ok(added)
}
