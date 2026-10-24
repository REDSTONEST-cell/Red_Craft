//! 暴露给前端的 Tauri commands。只做参数转换与状态管理，业务逻辑都在 launcher-core。

use std::path::PathBuf;

use launcher_core::auth::{is_standard_name, validate_offline_name, Account, AccountKind};
use launcher_core::install::{validate_instance_name, Verify};
use launcher_core::instance::{is_hidden_base, scan_instances, InstanceSummary, LoaderKind};
use launcher_core::loader::optifine::{self, OptiFineVersion};
use launcher_core::loader::{self, LoaderChoice, LoaderVersion};
use launcher_core::java::{self, JavaInstall};
use launcher_core::system::{self, MemoryInfo};
use launcher_core::version::VersionManifest;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::error::{AppError, AppResult};
use crate::settings::{InstanceConfig, Settings, INSTANCE_CONFIG};
use crate::state::AppState;

pub(crate) async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| AppError::msg(format!("后台任务异常：{e}"), "请重试"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    name: &'static str,
    version: &'static str,
    portable: bool,
    config_dir: PathBuf,
    game_dir: PathBuf,
    default_game_dir: PathBuf,
    memory: MemoryInfo,
    os: &'static str,
    arch: &'static str,
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        name: launcher_core::LAUNCHER_NAME,
        version: launcher_core::LAUNCHER_VERSION,
        portable: state.paths.portable,
        config_dir: state.paths.config_dir.clone(),
        game_dir: state.game_dir().root().to_path_buf(),
        default_game_dir: state.paths.default_game_dir.clone(),
        memory: system::memory(),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> AppResult<()> {
    let mut settings = settings;
    settings.concurrency = settings.concurrency.clamp(1, 256);
    settings.game_dir = settings.game_dir.filter(|p| !p.as_os_str().is_empty());
    settings.java_path = settings.java_path.filter(|p| !p.as_os_str().is_empty());
    state.save_settings(settings)
}

// ───────────── 版本 ─────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionItem {
    id: String,
    kind: String,
    release_time: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionList {
    latest_release: String,
    latest_snapshot: String,
    versions: Vec<VersionItem>,
}

pub(crate) async fn manifest(state: &AppState, refresh: bool) -> AppResult<VersionManifest> {
    let mut guard = state.manifest.lock().await;
    if !refresh {
        if let Some(m) = guard.as_ref() {
            return Ok(m.clone());
        }
    }
    let m = VersionManifest::fetch(&state.http()?).await?;
    *guard = Some(m.clone());
    Ok(m)
}

#[tauri::command]
pub async fn version_list(state: State<'_, AppState>, refresh: bool) -> AppResult<VersionList> {
    let m = manifest(&state, refresh).await?;
    Ok(VersionList {
        latest_release: m.latest.release.clone(),
        latest_snapshot: m.latest.snapshot.clone(),
        versions: m
            .versions
            .into_iter()
            .map(|v| VersionItem {
                id: v.id,
                kind: v.kind,
                release_time: v.release_time,
            })
            .collect(),
    })
}

// ───────────── 实例 ─────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceInfo {
    #[serde(flatten)]
    summary: InstanceSummary,
    isolated: bool,
    game_dir: PathBuf,
    last_played: Option<u64>,
    running: bool,
}

pub fn instance_isolated(state: &AppState, summary: &InstanceSummary, config: &InstanceConfig) -> bool {
    config
        .isolation
        .unwrap_or_else(|| state.settings().isolation.applies(summary.loader.is_some(), summary.is_release()))
}

#[tauri::command]
pub async fn list_instances(state: State<'_, AppState>) -> AppResult<Vec<InstanceInfo>> {
    let game = state.game_dir();
    let g = game.clone();
    let list = blocking(move || scan_instances(&g)).await?;
    let running = state.running.lock().map(|r| r.clone()).unwrap_or_default();
    let hidden: Vec<String> = list
        .iter()
        .filter(|s| is_hidden_base(&game, &list, &s.id, INSTANCE_CONFIG))
        .map(|s| s.id.clone())
        .collect();
    Ok(list
        .into_iter()
        .filter(|s| !hidden.contains(&s.id))
        .map(|mut summary| {
            let config = InstanceConfig::load(&game, &summary.id);
            if let Some(v) = config.mc_version.clone() {
                summary.mc_version = v;
            }
            let isolated = instance_isolated(&state, &summary, &config);
            InstanceInfo {
                game_dir: game.instance_game_dir(&summary.id, isolated),
                isolated,
                last_played: config.last_played,
                running: running.contains_key(&summary.id),
                summary,
            }
        })
        .collect())
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderArg {
    kind: LoaderKind,
    version: String,
}

/// 列出游戏版本可用的 OptiFine（正式版在前）。
#[tauri::command]
pub async fn optifine_versions(state: State<'_, AppState>, mc: String) -> AppResult<Vec<OptiFineVersion>> {
    Ok(optifine::list(&state.http()?, &mc).await?)
}

/// 列出与游戏版本兼容的加载器版本。
#[tauri::command]
pub async fn loader_versions(state: State<'_, AppState>, kind: LoaderKind, mc: String) -> AppResult<Vec<LoaderVersion>> {
    Ok(loader::list_versions(&state.http()?, kind, &mc).await?)
}

/// 新建实例：原版，或原版 + 加载器；可同时安装 OptiFine（原版 / Forge）与光影 Mod（Iris / Oculus）。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn install_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    version: String,
    name: String,
    loader: Option<LoaderArg>,
    with_api: Option<bool>,
    optifine: Option<OptiFineVersion>,
    shader_mod: Option<bool>,
) -> AppResult<u64> {
    let name = name.trim().to_string();
    validate_instance_name(&name)?;
    let game = state.game_dir();
    // 已作为某个加载器实例的父版本自动安装过：补全文件并让它作为独立实例显示
    if loader.is_none() && optifine.is_none() && name == version && game.version_json(&name).is_file() && !game.version_dir(&name).join(INSTANCE_CONFIG).exists() {
        let installer = state.installer()?;
        InstanceConfig {
            mc_version: Some(version.clone()),
            ..Default::default()
        }
        .save(&game, &name)?;
        return Ok(state.tasks.spawn(&app, format!("安装 {name}"), move |reporter, cancel| async move {
            Ok(installer.complete(&name, Verify::Quick, &reporter, &cancel).await?)
        }));
    }
    if game.version_dir(&name).exists() {
        return Err(AppError::msg(
            format!("已存在名为「{name}」的实例"),
            "请换一个实例名称，或先删除同名实例。",
        ));
    }
    let m = manifest(&state, false).await?;
    let entry = m
        .find(&version)
        .cloned()
        .ok_or_else(|| AppError::msg(format!("版本列表中没有 {version}"), "请刷新版本列表后重试。"))?;
    let installer = state.installer()?;
    let choice = loader.map(|l| LoaderChoice { kind: l.kind, version: l.version });
    if let Some(c) = &choice {
        if optifine.is_some() && c.kind != LoaderKind::Forge {
            return Err(AppError::msg(
                format!("OptiFine 不能与 {} 一起安装", c.kind.display_name()),
                "OptiFine 只支持原版与 Forge；Fabric / Quilt / NeoForge 请改用 Iris 或 Oculus。",
            ));
        }
    }
    let mut extras: Vec<&str> = choice.iter().map(|c| c.kind.display_name()).collect();
    if optifine.is_some() {
        extras.push("OptiFine");
    }
    let title = match extras.is_empty() {
        true => format!("安装 {name}"),
        false => format!("安装 {name}（{}）", extras.join(" + ")),
    };
    // 装了 OptiFine 或光影的实例总是版本隔离
    let isolation = (optifine.is_some() || shader_mod.unwrap_or(false)).then_some(true);
    let app2 = app.clone();
    let id = state.tasks.spawn(&app, title, move |reporter, cancel| async move {
        let state = app2.state::<AppState>();
        create_instance(&state, &installer, &entry, choice.as_ref(), optifine.as_ref(), &name, isolation, &reporter, &cancel).await?;
        // 以下附加内容失败时实例本身仍然保留
        let extra = |what: &str, e: AppError| {
            AppError::msg(format!("实例已安装完成，但{what}失败：{}", e.info().message), e.info().hint)
        };
        // Forge + OptiFine：OptiFine 作为 Mod 放进 mods 文件夹
        if let (Some(of), true) = (&optifine, choice.is_some()) {
            let ctx = crate::content::instance_context(&state, &name)?;
            let dest = ctx.game_dir.join("mods").join(&of.filename);
            optifine::download(&installer, of, &dest, &reporter, &cancel)
                .await
                .map_err(|e| extra("安装 OptiFine ", e.into()))?;
        }
        // Fabric / Quilt 自动安装 Fabric API / QSL
        if with_api.unwrap_or(true) && choice.as_ref().is_some_and(|c| matches!(c.kind, LoaderKind::Fabric | LoaderKind::Quilt)) {
            crate::content::install_loader_api(&state, &name, &reporter, &cancel)
                .await
                .map_err(|e| extra("自动安装 API ", e))?;
        }
        // 光影 Mod（含 Sodium / Embeddium 等前置）
        if shader_mod.unwrap_or(false) {
            crate::content::install_shader_mod(&state, &name, &reporter, &cancel)
                .await
                .map_err(|e| extra("安装光影加载器", e))?;
        }
        Ok(())
    });
    Ok(id)
}

/// 安装原版与加载器并保存实例配置；失败时删除这次新建的实例目录。
#[allow(clippy::too_many_arguments)]
pub async fn create_instance(
    state: &AppState,
    installer: &launcher_core::install::Installer,
    entry: &launcher_core::version::ManifestEntry,
    choice: Option<&LoaderChoice>,
    optifine: Option<&OptiFineVersion>,
    name: &str,
    isolation: Option<bool>,
    reporter: &launcher_core::install::Reporter,
    cancel: &launcher_core::download::CancelToken,
) -> AppResult<()> {
    // 原版 + OptiFine 生成的是独立版本；Forge + OptiFine 由调用方放进 mods
    let vanilla_optifine = optifine.filter(|_| choice.is_none());
    let result: AppResult<()> = async {
        // Forge / NeoForge 的安装器、OptiFine 的补丁程序需要 Java，版本与原版要求的一致
        let needs_java = vanilla_optifine.is_some() || choice.is_some_and(|c| matches!(c.kind, LoaderKind::Forge | LoaderKind::NeoForge));
        let java = match needs_java {
            true => {
                if !installer.game.version_json(&entry.id).exists() {
                    reporter.stage("下载原版版本信息");
                    installer.save_version_json(entry, &entry.id).await?;
                }
                let vanilla = installer.resolve(&entry.id)?;
                let (required, component) = crate::launch::required_java(&vanilla.json);
                reporter.stage("选择 Java");
                let java_override = state.settings().java_path;
                Some(crate::launch::select_java(state, installer, java_override, required, &component, reporter, cancel).await?.0)
            }
            _ => None,
        };
        match vanilla_optifine {
            Some(of) => {
                if name == entry.id {
                    return Err(AppError::msg(
                        format!("安装了 OptiFine 的实例不能命名为「{}」", entry.id),
                        "这个名称要留给原版，请换一个实例名称。",
                    ));
                }
                validate_instance_name(name)?;
                if !installer.game.version_json(&entry.id).exists() {
                    installer.save_version_json(entry, &entry.id).await?;
                }
                let java = java.as_deref().ok_or_else(|| AppError::msg("安装 OptiFine 需要 Java", ""))?;
                optifine::install_vanilla(installer, of, &entry.id, name, java, reporter, cancel).await?;
            }
            None => loader::install_instance(installer, entry, choice, name, java.as_deref(), reporter, cancel).await?,
        }
        InstanceConfig {
            mc_version: Some(entry.id.clone()),
            isolation,
            ..Default::default()
        }
        .save(&installer.game, name)?;
        Ok(())
    }
    .await;
    if result.is_err() {
        // 安装失败时清理掉这次新建的实例目录，避免出现损坏的实例
        let _ = std::fs::remove_dir_all(installer.game.version_dir(name));
    }
    result
}

/// 完整校验并补全实例文件。
#[tauri::command]
pub fn repair_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<u64> {
    let installer = state.installer()?;
    Ok(state.tasks.spawn(&app, format!("补全 {id}"), move |reporter, cancel| async move {
        Ok(installer.complete(&id, Verify::Full, &reporter, &cancel).await?)
    }))
}

#[tauri::command]
pub async fn delete_instance(state: State<'_, AppState>, id: String) -> AppResult<()> {
    if state.running.lock().is_ok_and(|r| r.contains_key(&id)) {
        return Err(AppError::msg("实例正在运行", "请先关闭游戏再删除。"));
    }
    validate_instance_name(&id)?;
    let game = state.game_dir();
    // 同时删除只为这个实例自动安装的原版父版本（没有其他实例继承它时）
    let g = game.clone();
    let before = blocking(move || scan_instances(&g)).await?;
    let orphans: Vec<String> = before
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.parents.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|p| {
            is_hidden_base(&game, &before, p, INSTANCE_CONFIG)
                && !before.iter().any(|o| o.id != id && o.parents.contains(p))
                && !state.running.lock().is_ok_and(|r| r.contains_key(p))
        })
        .collect();
    let dir = game.version_dir(&id);
    blocking(move || std::fs::remove_dir_all(&dir).map_err(|e| launcher_core::CoreError::io(&dir, e)))
        .await??;
    for p in orphans {
        let d = game.version_dir(&p);
        if let Err(e) = std::fs::remove_dir_all(&d) {
            log::warn!("删除父版本 {p} 失败：{e}");
        }
    }
    if state.settings().selected_instance.as_deref() == Some(id.as_str()) {
        state.update_settings(|s| s.selected_instance = None)?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_instance_config(state: State<'_, AppState>, id: String) -> InstanceConfig {
    InstanceConfig::load(&state.game_dir(), &id)
}

#[tauri::command]
pub fn save_instance_config(state: State<'_, AppState>, id: String, config: InstanceConfig) -> AppResult<()> {
    validate_instance_name(&id)?;
    Ok(config.save(&state.game_dir(), &id)?)
}

#[tauri::command]
pub fn select_instance(state: State<'_, AppState>, id: Option<String>) -> AppResult<()> {
    state.update_settings(|s| s.selected_instance = id)
}

/// 打开实例相关目录：`root`（游戏运行目录）、`mods`、`saves`、`resourcepacks`、`screenshots`、`logs`。
#[tauri::command]
pub async fn open_instance_dir(app: AppHandle, state: State<'_, AppState>, id: String, sub: String) -> AppResult<()> {
    let list = list_instances(state.clone()).await?;
    let inst = list
        .into_iter()
        .find(|i| i.summary.id == id)
        .ok_or_else(|| AppError::msg(format!("找不到实例 {id}"), "实例可能已被删除，请刷新列表。"))?;
    let allowed = ["mods", "saves", "resourcepacks", "shaderpacks", "screenshots", "logs", "crash-reports", "config"];
    let dir = if sub == "root" || sub.is_empty() {
        inst.game_dir
    } else if allowed.contains(&sub.as_str()) {
        inst.game_dir.join(&sub)
    } else {
        return Err(AppError::msg("无效的目录", ""));
    };
    std::fs::create_dir_all(&dir).map_err(|e| launcher_core::CoreError::io(&dir, e))?;
    open_path(&app, &dir)
}

#[tauri::command]
pub fn open_game_dir(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let dir = state.game_dir().root().to_path_buf();
    std::fs::create_dir_all(&dir).map_err(|e| launcher_core::CoreError::io(&dir, e))?;
    open_path(&app, &dir)
}

/// 打开崩溃报告等文件，只允许游戏目录内的路径。
#[tauri::command]
pub fn open_file(app: AppHandle, state: State<'_, AppState>, path: PathBuf) -> AppResult<()> {
    let root = std::fs::canonicalize(state.game_dir().root()).unwrap_or_default();
    let target = std::fs::canonicalize(&path).map_err(|e| launcher_core::CoreError::io(&path, e))?;
    if !target.starts_with(&root) {
        return Err(AppError::msg("只能打开游戏目录中的文件", ""));
    }
    open_path(&app, &target)
}

/// 读取游戏目录中的文本文件（崩溃报告），最多 1MB。
#[tauri::command]
pub async fn read_game_file(state: State<'_, AppState>, path: PathBuf) -> AppResult<String> {
    let root = std::fs::canonicalize(state.game_dir().root()).unwrap_or_default();
    blocking(move || -> AppResult<String> {
        let target = std::fs::canonicalize(&path).map_err(|e| launcher_core::CoreError::io(&path, e))?;
        if !target.starts_with(&root) {
            return Err(AppError::msg("只能读取游戏目录中的文件", ""));
        }
        let data = std::fs::read(&target).map_err(|e| launcher_core::CoreError::io(&target, e))?;
        let data = &data[..data.len().min(1 << 20)];
        Ok(String::from_utf8_lossy(data).into_owned())
    })
    .await?
}

fn open_path(app: &AppHandle, path: &std::path::Path) -> AppResult<()> {
    app.opener()
        .open_path(path.display().to_string(), None::<&str>)
        .map_err(|e| AppError::msg(format!("无法打开 {}：{e}", path.display()), "请手动打开该目录。"))
}

// ───────────── 账号 ─────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountList {
    pub accounts: Vec<Account>,
    pub selected: Option<String>,
}

#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> AccountList {
    let accounts = state.accounts.read().map(|a| a.accounts.clone()).unwrap_or_default();
    let mut selected = state.settings().selected_account;
    if selected.as_ref().is_none_or(|s| !accounts.iter().any(|a| &a.id == s)) {
        selected = accounts.first().map(|a| a.id.clone());
    }
    AccountList { accounts, selected }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedAccount {
    account: Account,
    /// 用户名不符合正版规则时的提醒
    warning: Option<String>,
}

#[tauri::command]
pub async fn add_offline_account(state: State<'_, AppState>, name: String) -> AppResult<AddedAccount> {
    let name = name.trim().to_string();
    validate_offline_name(&name)?;
    let mut account = Account::offline(&name);
    if state.accounts.read().is_ok_and(|s| s.accounts.iter().any(|a| a.id == account.id)) {
        return Err(AppError::msg(format!("离线账号「{name}」已存在"), "可以直接在列表中选择它。"));
    }
    // 有同名正版账号时使用它的皮肤做头像（查询失败不影响添加）
    account.skin = launcher_core::auth::lookup_skin(state.http()?.raw(), &name).await;
    {
        let mut store = state
            .accounts
            .write()
            .map_err(|_| AppError::msg("账号数据被锁定", "请重启启动器"))?;
        if store.accounts.iter().any(|a| a.id == account.id) {
            return Err(AppError::msg(format!("离线账号「{name}」已存在"), "可以直接在列表中选择它。"));
        }
        store.accounts.push(account.clone());
    }
    state.save_accounts()?;
    state.update_settings(|s| s.selected_account = Some(account.id.clone()))?;
    let warning = (!is_standard_name(&name)).then(|| {
        "用户名包含字母、数字、下划线以外的字符或长度不足 3 位，单人游戏可以正常使用，但部分服务器可能拒绝登录。".to_string()
    });
    Ok(AddedAccount { account, warning })
}

/// 启动时为还没有头像（或旧版本保存的是网络地址）的离线账号查询同名正版皮肤，完成后通知前端刷新。
pub async fn refresh_missing_skins(app: &tauri::AppHandle) {
    use tauri::Emitter;
    let state = app.state::<AppState>();
    let pending: Vec<Account> = state
        .accounts
        .read()
        .map(|s| {
            s.accounts
                .iter()
                .filter(|a| {
                    matches!(a.kind, AccountKind::Offline)
                        && !a.skin.as_deref().is_some_and(|s| s.starts_with("data:"))
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    if pending.is_empty() {
        return;
    }
    let Ok(http) = state.http() else { return };
    let mut changed = false;
    for acc in pending {
        if let Some(skin) = launcher_core::auth::lookup_skin(http.raw(), &acc.name).await {
            if let Ok(mut store) = state.accounts.write() {
                if let Some(a) = store.accounts.iter_mut().find(|a| a.id == acc.id) {
                    a.skin = Some(skin);
                    changed = true;
                }
            }
        }
    }
    if changed && state.save_accounts().is_ok() {
        let _ = app.emit("accounts-changed", ());
    }
}

/// 重新查询离线账号同名正版账号的皮肤。
#[tauri::command]
pub async fn refresh_offline_skin(state: State<'_, AppState>, id: String) -> AppResult<Account> {
    let account = state
        .accounts
        .read()
        .ok()
        .and_then(|a| a.accounts.iter().find(|x| x.id == id).cloned())
        .ok_or_else(|| AppError::msg("账号不存在", "请刷新账号列表。"))?;
    let skin = launcher_core::auth::lookup_skin(state.http()?.raw(), &account.name).await;
    let mut updated = account.clone();
    // 查询失败时保留原来的头像
    if skin.is_some() || account.skin.is_none() {
        updated.skin = skin;
    }
    if let Ok(mut store) = state.accounts.write() {
        if let Some(a) = store.accounts.iter_mut().find(|a| a.id == id) {
            *a = updated.clone();
        }
    }
    state.save_accounts()?;
    Ok(updated)
}

#[tauri::command]
pub fn remove_account(state: State<'_, AppState>, id: String) -> AppResult<()> {
    if let Ok(mut store) = state.accounts.write() {
        store.accounts.retain(|a| a.id != id);
    }
    state.secrets.delete(&id);
    state.save_accounts()?;
    if state.settings().selected_account.as_deref() == Some(id.as_str()) {
        state.update_settings(|s| s.selected_account = None)?;
    }
    Ok(())
}

#[tauri::command]
pub fn select_account(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.update_settings(|s| s.selected_account = Some(id))
}

// ───────────── Java ─────────────

pub async fn java_list(state: &AppState, refresh: bool) -> AppResult<Vec<JavaInstall>> {
    if !refresh {
        if let Some(list) = state.java.lock().ok().and_then(|j| j.clone()) {
            return Ok(list);
        }
    }
    let runtime = state.game_dir().runtime();
    let list = blocking(move || java::scan_installed(&[runtime])).await?;
    if let Ok(mut j) = state.java.lock() {
        *j = Some(list.clone());
    }
    Ok(list)
}

#[tauri::command]
pub async fn scan_java(state: State<'_, AppState>, refresh: bool) -> AppResult<Vec<JavaInstall>> {
    java_list(&state, refresh).await
}

/// 检查用户手动选择的 Java。
#[tauri::command]
pub async fn probe_java(path: PathBuf) -> AppResult<JavaInstall> {
    let p = path.clone();
    blocking(move || java::probe(&p)).await?.ok_or_else(|| {
        AppError::msg(
            format!("{} 不是有效的 Java", path.display()),
            "请选择 Java 安装目录下 bin 文件夹中的 java.exe（或 javaw.exe）。",
        )
    })
}

// ───────────── 任务与窗口 ─────────────

#[tauri::command]
pub fn cancel_task(state: State<'_, AppState>, id: u64) -> bool {
    state.tasks.cancel(id)
}

/// 前端完成首次渲染后再显示窗口，避免启动时白屏闪烁。
#[tauri::command]
pub fn app_ready(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}
