pub mod model;
mod scene;
#[cfg(windows)]
mod host;
mod steam;
mod store;
mod web;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use model::{Appearance, Asset, Library, Resolved, WallpaperScanResult};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

pub fn load(config: &std::path::Path) -> Library {
    store::load(config)
}

pub use web::serve as serve_web;

fn checked(config: &std::path::Path, asset: &Asset) -> AppResult<Asset> {
    if asset.source == "wallpaperEngine" {
        let fresh = steam::project(&asset.root)?;
        if fresh.id != asset.id {
            return Err(AppError::msg("壁纸路径已改变", "请刷新壁纸库"));
        }
        if !fresh.is_user_we() {
            return Err(AppError::msg("官方自带壁纸不能作为背景", "请选择创意工坊或自己制作的壁纸"));
        }
        if let Some(reason) = &fresh.reason {
            return Err(AppError::msg(reason.clone(), "请刷新壁纸库或选择其它壁纸"));
        }
        if fresh.kind == "image" {
            if let Some(path) = &fresh.media {
                store::decode_image(path)?;
            }
        }
        return Ok(fresh);
    }
    if asset.source != "local" {
        return Err(AppError::msg("未知素材来源", "请重新导入素材"));
    }
    let media = asset
        .media
        .as_ref()
        .ok_or_else(|| AppError::msg("素材没有媒体文件", "请重新导入素材"))?;
    let path = store::inside(&config.join("backgrounds/media"), media)?;
    if path.file_stem().and_then(|s| s.to_str()) != Some(&asset.id) {
        return Err(AppError::msg("素材 ID 与文件不匹配", "请重新导入素材"));
    }
    match asset.kind.as_str() {
        "image" => store::decode_image(&path)?,
        "video" => store::video(&path)?,
        _ => return Err(AppError::msg("不支持的素材类型", "请重新导入素材")),
    }
    let mut asset = asset.clone();
    asset.media = Some(path.clone());
    asset.cover = (asset.kind == "image").then_some(path);
    asset.reason = None;
    Ok(asset)
}

/// 决定壁纸在启动器里怎样显示：视频播放、图片显示、网页在沙盒中运行；
/// 场景渲染成静态画面；应用程序或无法播放的主体使用预览图。
fn display(config: &std::path::Path, asset: Asset, scene_preview: bool) -> AppResult<Resolved> {
    let fallback = |asset: &Asset, why: Option<AppError>| -> AppResult<Resolved> {
        match &asset.cover {
            Some(cover) => Ok(Resolved {
                id: asset.id.clone(),
                kind: "image".into(),
                media: Some(cover.clone()),
                cover: Some(cover.clone()),
                entry: None,
                host: None,
            }),
            None => Err(why.unwrap_or_else(|| AppError::msg("这个壁纸没有可显示的画面", "请选择其它壁纸"))),
        }
    };
    if asset.source != "wallpaperEngine" {
        return Ok(Resolved { id: asset.id, kind: asset.kind, media: asset.media, cover: asset.cover, entry: None, host: None });
    }
    match asset.kind.as_str() {
        "video" | "image" if asset.media.is_some() => {
            Ok(Resolved { id: asset.id, kind: asset.kind, media: asset.media, cover: asset.cover, entry: None, host: None })
        }
        "web" if asset.media.is_some() => {
            let entry = web::entry(&asset.root, asset.media.as_deref().unwrap())?;
            Ok(Resolved { id: asset.id, kind: "web".into(), media: None, cover: asset.cover, entry: Some(entry), host: None })
        }
        // 用户选择了「使用预览图」，或渲染失败：显示预览图（动态模式下 GIF 预览会播放）。
        "scene" if scene_preview && asset.cover.is_some() => fallback(&asset, None),
        "scene" => match scene::rendered(config, &asset.id, &asset.root, steam::declared_file(&asset.root).as_deref(), &steam::user_properties(&asset.root)) {
            Ok(png) => Ok(Resolved {
                id: asset.id,
                kind: "image".into(),
                media: Some(png.clone()),
                cover: Some(png),
                entry: None,
                host: None,
            }),
            Err(e) => {
                log::warn!("场景壁纸 {} 渲染失败，改用预览图：{e}", asset.title);
                fallback(&asset, Some(e))
            }
        },
        _ => fallback(&asset, None),
    }
}

fn library(state: &AppState) -> AppResult<std::sync::MutexGuard<'_, Library>> {
    state
        .backgrounds
        .lock()
        .map_err(|_| AppError::msg("背景库被锁定", "请重新启动启动器"))
}

#[tauri::command]
pub async fn background_list(app: AppHandle) -> AppResult<Library> {
    crate::commands::blocking(move || {
        let state = app.state::<AppState>();
        let mut value = library(&state)?.clone();
        value
            .assets
            .retain(|a| a.source == "local" || a.is_user_we());
        for asset in &mut value.assets {
            if asset.media.as_ref().is_some_and(|p| !p.is_file()) {
                asset.reason = Some("素材丢失或不可读；请重新选择或刷新库".into());
            }
        }
        Ok(value)
    })
    .await?
}

#[tauri::command]
pub async fn background_thumbnail(app: AppHandle, id: String) -> AppResult<Option<PathBuf>> {
    crate::commands::blocking(move || {
        let state = app.state::<AppState>();
        let guard = library(&state)?;
        let asset = guard
            .assets
            .iter()
            .find(|a| a.id == id)
            .cloned()
            .ok_or_else(|| AppError::msg("背景素材不存在", "请刷新素材库"))?;
        let scene_cover;
        let cover = match &asset.cover {
            Some(cover) => cover,
            // 没有预览图的场景：用渲染出的静态画面做缩略图。
            None if asset.source == "wallpaperEngine" && asset.kind == "scene" => {
                scene_cover = scene::rendered(&state.paths.config_dir, &asset.id, &asset.root, steam::declared_file(&asset.root).as_deref(), &steam::user_properties(&asset.root))?;
                &scene_cover
            }
            None => return Ok(None),
        };
        let rendered = guard.crops.get(&id).map(|rect| store::crop(&state.paths.config_dir, &asset, *rect)).transpose()?;
        let cover = if let Some(cropped) = rendered { cropped.cover.unwrap() } else { match asset.source.as_str() {
            "wallpaperEngine" if asset.cover.is_none() => cover.clone(),
            "wallpaperEngine" => store::inside(&asset.root, cover)?,
            "local" => {
                let path = store::inside(&state.paths.config_dir.join("backgrounds/media"), cover)?;
                if path.file_stem().and_then(|s| s.to_str()) != Some(&asset.id) {
                    return Err(AppError::msg("缩略图与素材 ID 不匹配", "请重新导入素材"));
                }
                path
            }
            _ => return Err(AppError::msg("未知素材来源", "请刷新素材库")),
        } };
        let thumbnail = store::thumbnail(&state.paths.config_dir, &asset.id, &cover)?;
        app.asset_protocol_scope()
            .allow_file(&thumbnail)
            .map_err(|e| AppError::msg(format!("无法授权缩略图：{e}"), "请检查文件权限"))?;
        Ok(Some(thumbnail))
    })
    .await?
}

#[tauri::command]
pub async fn background_resolve(app: AppHandle, id: String, original: Option<bool>) -> AppResult<Resolved> {
    crate::commands::blocking(move || {
        let state = app.state::<AppState>();
        let guard = library(&state)?;
        let asset = guard
            .assets
            .iter()
            .find(|a| a.id == id)
            .cloned()
            .ok_or_else(|| AppError::msg("背景素材不存在", "请刷新素材库"))?;
        let mut asset = checked(&state.paths.config_dir, &asset)?;
        if !original.unwrap_or(false) {
            if let Some(rect) = guard.crops.get(&id) { asset = store::crop(&state.paths.config_dir, &asset, *rect)?; }
        }
        let scene_preview = guard.scene_preview.contains(&id);
        drop(guard);
        let mut resolved = display(&state.paths.config_dir, asset, scene_preview)?;
        // Windows：网页壁纸交给 WebView2 的虚拟主机映射直接读取项目目录（与 Lively Wallpaper 的做法相同），
        // 支持大文件与分段读取；其它系统使用 rcwall 协议。
        #[cfg(windows)]
        if resolved.kind == "web" {
            let root = library(&state)?.assets.iter().find(|a| a.id == id).map(|a| a.root.clone());
            if let Some(root) = root {
                resolved.host = Some(host::map(&app, &resolved.id, &root)?);
            }
        }
        let scope = app.asset_protocol_scope();
        resolved.cover = resolved
            .cover
            .as_ref()
            .and_then(|path| store::still_cover(&state.paths.config_dir, &resolved.id, path).ok());
        for path in resolved.media.iter().chain(resolved.cover.iter()) {
            scope
                .allow_file(path)
                .map_err(|e| AppError::msg(format!("无法授权背景媒体：{e}"), "请检查文件权限"))?;
        }
        Ok(resolved)
    })
    .await?
}

#[tauri::command]
pub async fn background_apply(app: AppHandle, mut appearance: Appearance) -> AppResult<Appearance> {
    crate::commands::blocking(move || {
        let state = app.state::<AppState>();
        appearance.normalize();
        let mut guard = library(&state)?;
        if let Some(id) = &appearance.selection {
            let asset = guard
                .assets
                .iter()
                .find(|a| &a.id == id)
                .ok_or_else(|| AppError::msg("背景素材不存在", "请刷新素材库"))?;
            let fresh = checked(&state.paths.config_dir, asset)?;
            if let Some(reason) = fresh.reason {
                return Err(AppError::msg(reason, "请选择可用的图片或 Video 项目"));
            }
        }
        let mut next = guard.clone();
        next.appearance = appearance.clone();
        store::save(&state.paths.config_dir, &next)?;
        *guard = next;
        Ok(appearance)
    })
    .await?
}

#[tauri::command]
pub fn background_import(
    app: AppHandle,
    state: State<'_, AppState>,
    path: PathBuf,
) -> AppResult<u64> {
    let handle = app.clone();
    Ok(state
        .tasks
        .spawn(&app, "导入背景", move |reporter, cancel| async move {
            crate::commands::blocking(move || {
                reporter.stage("校验并复制背景文件");
                let state = handle.state::<AppState>();
                let asset = store::import(&state.paths.config_dir, &path, &cancel)?;
                let result = (|| {
                    let mut guard = library(&state)?;
                    cancel.check()?;
                    let mut next = guard.clone();
                    next.assets.push(asset.clone());
                    store::save(&state.paths.config_dir, &next)?;
                    *guard = next;
                    Ok(())
                })();
                if result.is_err() {
                    let _ = store::remove_copy(&state.paths.config_dir, &asset);
                }
                result
            })
            .await?
        }))
}

#[tauri::command]
pub async fn background_scan(
    app: AppHandle,
    roots: Vec<PathBuf>,
    force_refresh: Option<bool>,
) -> AppResult<WallpaperScanResult> {
    let state = app.state::<AppState>();
    // 独立扫描锁：多个进入/刷新请求合并为顺序操作，不占用库锁做遍历。
    let _scan_guard = state.background_scan.lock().await;
    let handle = app.clone();
    crate::commands::blocking(move || {
        let roots = steam::normalize_roots(&roots)?;
        let state = handle.state::<AppState>();
        if let Some(result) = library(&state)?.cached_scan(&roots, force_refresh.unwrap_or(false)) {
            return Ok(result);
        }
        let cancel = launcher_core::download::CancelToken::new();
        let (assets, notes) = steam::scan(&roots, &cancel, |_| {})?;
        let mut guard = library(&state)?;
        let mut next = guard.clone();
        let selected = next.appearance.selection.clone();
        next.assets
            .retain(|a| a.source == "local" || selected.as_ref() == Some(&a.id));
        for asset in assets {
            next.assets.retain(|a| a.id != asset.id);
            next.assets.push(asset);
        }
        next.roots = roots;
        next.scan_notes = notes;
        next.scan_completed = true;
        next.scan_revision = model::SCAN_REVISION;
        store::save(&state.paths.config_dir, &next)?;
        let result = next.scan_result(false);
        *guard = next;
        Ok(result)
    })
    .await?
}

#[tauri::command]
pub async fn background_remove(app: AppHandle, id: String) -> AppResult<()> {
    crate::commands::blocking(move || {
        let state = app.state::<AppState>();
        let mut guard = library(&state)?;
        let asset = guard
            .assets
            .iter()
            .find(|a| a.id == id)
            .cloned()
            .ok_or_else(|| AppError::msg("背景素材不存在", "请刷新素材库"))?;
        let mut next = guard.clone();
        next.assets.retain(|a| a.id != id);
        next.crops.remove(&id);
        next.scene_preview.remove(&id);
        if next.appearance.selection.as_deref() == Some(&id) {
            next.appearance.selection = None;
        }
        store::save(&state.paths.config_dir, &next)?;
        *guard = next;
        drop(guard);
        let removed = store::remove_copy(&state.paths.config_dir, &asset);
        // 封面缓存与素材一起移除；清缓存失败不应让已完成的移除报错。
        if let Err(e) = store::remove_covers(&state.paths.config_dir, &id) {
            log::warn!("清理背景封面缓存失败：{e}");
        }
        if let Err(e) = store::cleanup_crop(&state.paths.config_dir, &id, None) { log::warn!("清理裁剪缓存失败：{e}"); }
        scene::cleanup(&state.paths.config_dir, &id, None);
        removed
    })
    .await?
}

#[tauri::command]
pub async fn background_crop(app: AppHandle, id: String, rect: Option<model::CropRect>) -> AppResult<()> {
    crate::commands::blocking(move || {
        let state = app.state::<AppState>();
        let mut guard = library(&state)?;
        let source = guard.assets.iter().find(|a| a.id == id)
            .ok_or_else(|| AppError::msg("背景素材不存在", "请刷新素材库"))?;
        if source.source != "local" || source.kind != "image" {
            return Err(AppError::msg("只能裁剪本地图片", "请选择图片素材"));
        }
        let rendered = rect.map(|r| store::crop(&state.paths.config_dir, source, r)).transpose()?;
        let mut next = guard.clone();
        if let Some(rect) = rect { next.crops.insert(id.clone(), rect); }
        else { next.crops.remove(&id); }
        store::save(&state.paths.config_dir, &next)?;
        *guard = next;
        let keep = rendered.as_ref().and_then(|a| a.media.as_deref());
        if let Err(e) = store::cleanup_crop(&state.paths.config_dir, &id, keep) { log::warn!("清理裁剪缓存失败：{e}"); }
        if let Err(e) = store::remove_covers(&state.paths.config_dir, &id) { log::warn!("清理封面缓存失败：{e}"); }
        Ok(())
    }).await?
}

/// 场景壁纸使用预览图还是渲染出的静态画面。
#[tauri::command]
pub async fn background_scene_mode(app: AppHandle, id: String, preview: bool) -> AppResult<()> {
    crate::commands::blocking(move || {
        let state = app.state::<AppState>();
        let mut guard = library(&state)?;
        if !guard.assets.iter().any(|a| a.id == id && a.kind == "scene") {
            return Err(AppError::msg("这不是场景壁纸", "请刷新素材库"));
        }
        let mut next = guard.clone();
        if preview {
            next.scene_preview.insert(id);
        } else {
            next.scene_preview.remove(&id);
        }
        store::save(&state.paths.config_dir, &next)?;
        *guard = next;
        Ok(())
    })
    .await?
}

#[tauri::command]
pub fn background_window_state(window: tauri::WebviewWindow) -> bool {
    window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(true)
}

/// 记录可见性并返回是否发生变化。窗口移动/缩放/聚焦等事件非常频繁，
/// 只有可见性真正变化时才通知前端，避免拖动时每秒几十次 IPC。
fn next_visibility(last: &mut Option<bool>, visible: bool) -> bool {
    let changed = *last != Some(visible);
    *last = Some(visible);
    changed
}

pub fn window_changed(app: &AppHandle) {
    use tauri::Emitter;
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let visible = window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(true);
    let changed = match app.try_state::<AppState>() {
        Some(state) => match state.background_visible.lock() {
            Ok(mut last) => next_visibility(&mut last, visible),
            Err(_) => true,
        },
        None => true,
    };
    if changed {
        let _ = app.emit("background-window-visible", visible);
    }
}

#[cfg(test)]
mod tests;
