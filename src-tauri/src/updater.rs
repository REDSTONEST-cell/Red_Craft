//! 启动器自身的检查更新、下载与安装。
//!
//! 流程：检查到新版本 → 前端弹窗确认 → 下载对应安装方式的文件并用 SHA256SUMS.txt 校验
//! → 安装并重启。单文件版与 AppImage 直接替换文件；Windows 安装版运行新的安装包。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use launcher_core::download::DownloadTask;
use launcher_core::update::{self, InstallKind, Release};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// 最近一次检查到的 Release，以及已下载、等待安装的文件。
static LATEST: Mutex<Option<Release>> = Mutex::new(None);
static PENDING: Mutex<Option<(InstallKind, PathBuf)>> = Mutex::new(None);

/// GitHub 在国内经常很慢：官方地址失败时经由代理下载（下载后用官方 API 取得的 SHA256 校验）。
const PROXY: &str = "https://ghfast.top/";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current: String,
    pub notes: String,
    pub page_url: String,
    pub kind: InstallKind,
    /// 能否自动安装（macOS、deb 只能打开下载页）
    pub can_install: bool,
    pub size: u64,
}

fn current_exe() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// 当前的安装方式。
pub fn install_kind() -> InstallKind {
    if cfg!(feature = "portable") {
        return InstallKind::Portable;
    }
    if cfg!(windows) {
        // NSIS 安装版在安装目录中有 uninstall.exe，MSI 没有
        let nsis = current_exe()
            .and_then(|p| p.parent().map(|d| d.join("uninstall.exe").is_file()))
            .unwrap_or(false);
        return if nsis { InstallKind::Nsis } else { InstallKind::Msi };
    }
    if cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_some() {
        return InstallKind::AppImage;
    }
    InstallKind::Manual
}

/// 要被替换的文件：单文件版是 exe 本身，AppImage 是 $APPIMAGE。
fn replace_target(kind: InstallKind) -> Option<PathBuf> {
    match kind {
        InstallKind::Portable => current_exe(),
        InstallKind::AppImage => std::env::var_os("APPIMAGE").map(PathBuf::from),
        _ => None,
    }
}

fn old_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    target.with_file_name(name)
}

fn updates_dir(state: &AppState) -> PathBuf {
    state.paths.config_dir.join("updates")
}

/// 启动时清理上次更新留下的旧文件与安装包。
pub fn cleanup(state: &AppState) {
    if let Some(t) = replace_target(install_kind()) {
        let _ = std::fs::remove_file(old_path(&t));
    }
    let _ = std::fs::remove_dir_all(updates_dir(state));
}

/// 检查是否有新的正式版。没有新版本时返回 `None`。
#[tauri::command]
pub async fn check_update(state: State<'_, AppState>) -> AppResult<Option<UpdateInfo>> {
    let http = state.http()?;
    let release = update::latest_release(http.raw())
        .await
        .map_err(|e| AppError::msg(format!("检查更新失败：{e}"), "请检查网络连接后重试。"))?;
    let current = launcher_core::LAUNCHER_VERSION;
    if release.draft || release.prerelease || !update::is_newer(&release.tag_name, current) {
        return Ok(None);
    }
    let kind = install_kind();
    let asset = release.asset_for(kind);
    let info = UpdateInfo {
        version: release.tag_name.trim_start_matches('v').to_string(),
        current: current.to_string(),
        notes: release.body.clone().unwrap_or_default(),
        page_url: release.html_url.clone(),
        kind,
        can_install: asset.is_some(),
        size: asset.map(|a| a.size).unwrap_or(0),
    };
    if let Ok(mut l) = LATEST.lock() {
        *l = Some(release);
    }
    Ok(Some(info))
}

/// 下载并准备安装新版本，返回任务 id。任务完成后前端调用 `apply_update` 重启。
#[tauri::command]
pub async fn download_update(app: AppHandle, state: State<'_, AppState>) -> AppResult<u64> {
    let release = LATEST
        .lock()
        .ok()
        .and_then(|l| l.clone())
        .ok_or_else(|| AppError::msg("请先检查更新", ""))?;
    let kind = install_kind();
    let asset = release
        .asset_for(kind)
        .cloned()
        .ok_or_else(|| AppError::msg("这个版本没有适合当前系统的自动更新文件", "请前往下载页手动下载。"))?;
    let http = state.http()?;
    let dir = updates_dir(&state);
    let concurrency = state.settings().concurrency;
    let title = format!("下载 Red Craft {}", release.tag_name.trim_start_matches('v'));
    Ok(state.tasks.spawn(&app, title, move |reporter, cancel| async move {
        // 校验值（官方 API 返回的地址，经代理下载的文件也能校验）
        reporter.stage("获取校验信息");
        let expected = match release.sums_asset() {
            Some(s) => {
                let text = http
                    .raw()
                    .get(&s.browser_download_url)
                    .send()
                    .await
                    .ok()
                    .filter(|r| r.status().is_success());
                match text {
                    Some(r) => r.text().await.ok().and_then(|t| update::sum_for(&t, &asset.name)),
                    None => None,
                }
            }
            None => None,
        };

        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| AppError::msg(format!("无法创建更新目录：{e}"), ""))?;
        let file = dir.join(&asset.name);
        let downloader = launcher_core::download::Downloader::new(http.clone(), concurrency);
        reporter.stage("下载新版本");
        let task = |url: &str| DownloadTask::new(url, &file).size(Some(asset.size).filter(|s| *s > 0));
        let mut result = downloader
            .download_all(vec![task(&asset.browser_download_url)], reporter.progress.clone(), &cancel)
            .await;
        // 官方地址失败时经由代理重试；只有能校验时才使用代理
        if result.is_err() && expected.is_some() && !cancel.is_cancelled() {
            log::warn!("从 GitHub 下载更新失败，改用代理：{:?}", result.as_ref().err());
            reporter.stage("下载新版本（备用线路）");
            let proxied = format!("{PROXY}{}", asset.browser_download_url);
            result = downloader
                .download_all(vec![task(&proxied)], reporter.progress.clone(), &cancel)
                .await;
        }
        result?;

        if let Some(expected) = expected {
            reporter.stage("校验文件");
            let f = file.clone();
            let actual = tauri::async_runtime::spawn_blocking(move || update::sha256_file(&f))
                .await
                .map_err(|e| AppError::msg(e.to_string(), ""))??;
            if actual != expected {
                let _ = std::fs::remove_file(&file);
                return Err(AppError::msg("更新文件校验失败，已删除", "请稍后重试。"));
            }
        }

        // 单文件版 / AppImage：现在就替换文件（运行中的程序可以改名），重启后即为新版本
        if let Some(target) = replace_target(kind) {
            reporter.stage("替换程序文件");
            replace_file(&target, &file)?;
        }
        if let Ok(mut p) = PENDING.lock() {
            *p = Some((kind, file));
        }
        log::info!("新版本 {} 已下载，等待重启安装", release.tag_name);
        Ok(())
    }))
}

/// 把运行中的程序改名为 .old，再把新文件放到原位置。失败时恢复。
fn replace_file(target: &Path, new: &Path) -> AppResult<()> {
    let old = old_path(target);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(target, &old).map_err(|e| {
        AppError::msg(
            format!("无法替换程序文件：{e}"),
            "请确认启动器所在文件夹可写，或前往下载页手动更新。",
        )
    })?;
    let placed = std::fs::rename(new, target).or_else(|_| std::fs::copy(new, target).map(|_| ()));
    if let Err(e) = placed {
        let _ = std::fs::rename(&old, target);
        return Err(AppError::msg(format!("无法写入新版本：{e}"), "请前往下载页手动更新。"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

/// 安装已下载的新版本并重启启动器。
#[tauri::command]
pub fn apply_update(app: AppHandle) -> AppResult<()> {
    let (kind, file) = PENDING
        .lock()
        .ok()
        .and_then(|p| p.clone())
        .ok_or_else(|| AppError::msg("还没有下载新版本", ""))?;
    match kind {
        InstallKind::Portable | InstallKind::AppImage => {
            let target = replace_target(kind).ok_or_else(|| AppError::msg("找不到程序文件", ""))?;
            std::process::Command::new(&target)
                .spawn()
                .map_err(|e| AppError::msg(format!("无法启动新版本：{e}"), "请手动重新打开启动器。"))?;
        }
        InstallKind::Nsis | InstallKind::Msi => run_installer(kind, &file)?,
        InstallKind::Manual => return Err(AppError::msg("当前系统需要手动安装更新", "")),
    }
    log::info!("开始安装更新，启动器即将退出");
    app.exit(0);
    Ok(())
}

/// Windows 安装版：等启动器退出后静默运行安装包，完成后重新打开启动器。
#[cfg(windows)]
fn run_installer(kind: InstallKind, file: &Path) -> AppResult<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let exe = current_exe().ok_or_else(|| AppError::msg("找不到程序文件", ""))?;
    let install = match kind {
        InstallKind::Msi => format!("msiexec /i \"{}\" /passive /norestart", file.display()),
        _ => format!("\"{}\" /S", file.display()),
    };
    // ping 用来等待约 2 秒，让启动器先退出
    let script = format!(
        "ping -n 3 127.0.0.1 >nul & start \"\" /wait {install} & start \"\" \"{}\"",
        exe.display()
    );
    std::process::Command::new("cmd")
        .raw_arg(format!("/C {script}"))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| AppError::msg(format!("无法运行安装程序：{e}"), "请前往下载页手动更新。"))?;
    Ok(())
}

#[cfg(not(windows))]
fn run_installer(_kind: InstallKind, _file: &Path) -> AppResult<()> {
    Err(AppError::msg("当前系统需要手动安装更新", ""))
}
