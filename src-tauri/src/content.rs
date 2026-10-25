//! 资源下载（Mod / 材质包 / 光影包）相关的 commands。

use std::collections::HashSet;
use std::path::PathBuf;

use launcher_core::content::{
    self, best_of, install_files, installed_projects, Client, ContentType, ContentVersion, CurseForge, FilteredClient,
    Modrinth, Plan, Platform, ProjectHit, SearchQuery, SearchResult, VersionFilter,
};
use launcher_core::download::CancelToken;
use launcher_core::install::Reporter;
use launcher_core::instance::{detect_loader, LoaderKind};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::error::{AppError, AppResult};
use crate::settings::InstanceConfig;
use crate::state::AppState;

pub const FABRIC_API: &str = "P7dR8mSH";
pub const QSL: &str = "qvIfYCYJ";

pub fn client(state: &AppState, platform: Platform) -> AppResult<Client> {
    let http = state.http()?;
    Ok(match platform {
        Platform::Modrinth => Client::Modrinth(Modrinth::new(http)),
        Platform::Curseforge => {
            let key = state.settings().curseforge_api_key;
            Client::CurseForge(CurseForge::new(http, Some(key)))
        }
    })
}

/// 实例的游戏版本、加载器与游戏目录。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceContext {
    pub id: String,
    pub mc_version: String,
    pub loader: Option<LoaderKind>,
    pub game_dir: PathBuf,
}

pub fn instance_context(state: &AppState, id: &str) -> AppResult<InstanceContext> {
    let game = state.game_dir();
    let resolved = launcher_core::version::resolve_version(&game, id)?;
    let config = InstanceConfig::load(&game, id);
    let loader = detect_loader(&resolved).map(|l| l.kind);
    let mc_version = config
        .mc_version
        .clone()
        .unwrap_or_else(|| resolved.chain.last().cloned().unwrap_or_default());
    let release = resolved.json.kind == "release";
    let isolated = config
        .isolation
        .unwrap_or_else(|| state.settings().isolation.applies(loader.is_some(), release));
    Ok(InstanceContext {
        id: id.to_string(),
        mc_version,
        loader,
        game_dir: game.instance_game_dir(id, isolated),
    })
}

fn filter_for(ctx: &InstanceContext, content_type: ContentType) -> VersionFilter {
    VersionFilter {
        game_version: Some(ctx.mc_version.clone()),
        loader: ctx.loader,
        content_type: Some(content_type),
    }
}

#[tauri::command]
pub async fn content_search(state: State<'_, AppState>, platform: Platform, query: SearchQuery) -> AppResult<SearchResult> {
    Ok(client(&state, platform)?.search(&query).await?)
}

#[tauri::command]
pub fn content_instance(state: State<'_, AppState>, id: String) -> AppResult<InstanceContext> {
    instance_context(&state, &id)
}

/// 项目的版本列表；`compatible` 为真时只列出兼容实例的版本。
#[tauri::command]
pub async fn content_versions(
    state: State<'_, AppState>,
    platform: Platform,
    project_id: String,
    content_type: ContentType,
    instance: Option<String>,
) -> AppResult<Vec<ContentVersion>> {
    let filter = match instance {
        Some(id) => filter_for(&instance_context(&state, &id)?, content_type),
        None => VersionFilter {
            content_type: Some(content_type),
            ..Default::default()
        },
    };
    Ok(client(&state, platform)?.versions(&project_id, &filter).await?)
}

#[tauri::command]
pub async fn content_project(state: State<'_, AppState>, platform: Platform, project_id: String) -> AppResult<ProjectHit> {
    Ok(client(&state, platform)?.project(&project_id).await?)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionRef {
    pub project_id: String,
    pub version_id: String,
}

async fn plan_for(
    state: &AppState,
    platform: Platform,
    ctx: &InstanceContext,
    content_type: ContentType,
    roots: &[VersionRef],
) -> AppResult<Plan> {
    let client = client(state, platform)?;
    let mut root_versions = Vec::new();
    for r in roots {
        root_versions.push(client.version(&r.project_id, &r.version_id).await?);
    }
    let folder = ctx.game_dir.join(content_type.folder());
    let modrinth = match &client {
        Client::Modrinth(m) => Some(m.clone()),
        _ => None,
    };
    let installed = installed_projects(&folder, platform, modrinth.as_ref()).await;
    let src = FilteredClient {
        client: &client,
        filter: filter_for(ctx, content_type),
    };
    Ok(content::resolve(&src, root_versions, &installed).await?)
}

/// 安装前的确认信息：将一起安装的前置、可选依赖、冲突与缺失。
#[tauri::command]
pub async fn content_plan(
    state: State<'_, AppState>,
    platform: Platform,
    instance: String,
    content_type: ContentType,
    roots: Vec<VersionRef>,
) -> AppResult<Plan> {
    let ctx = instance_context(&state, &instance)?;
    plan_for(&state, platform, &ctx, content_type, &roots).await
}

/// 安装：重新解析根项目（含用户勾选的可选依赖）的全部必需前置，跳过已安装的。
#[tauri::command]
pub async fn content_install(
    app: AppHandle,
    state: State<'_, AppState>,
    platform: Platform,
    instance: String,
    content_type: ContentType,
    roots: Vec<VersionRef>,
) -> AppResult<u64> {
    if content_type == ContentType::Modpack {
        return Err(AppError::msg("整合包需要通过「安装整合包」创建新实例", ""));
    }
    let ctx = instance_context(&state, &instance)?;
    let installer = state.installer()?;
    let title = format!("安装{}到 {instance}", content_type.label());
    let app2 = app.clone();
    Ok(state.tasks.spawn(&app, title, move |reporter, cancel| async move {
        let state = app2.state::<AppState>();
        reporter.stage("解析前置依赖");
        let plan = plan_for(&state, platform, &ctx, content_type, &roots).await?;
        if !plan.missing.is_empty() {
            let names: Vec<String> = plan.missing.iter().map(|m| format!("{}（{} 需要）", m.title, m.required_by)).collect();
            return Err(AppError::msg(
                format!("找不到兼容的前置：{}", names.join("、")),
                "这些前置没有适用于当前游戏版本和加载器的版本，请换一个版本的 Mod 或实例。",
            ));
        }
        let items: Vec<(ContentVersion, String)> = plan
            .items
            .into_iter()
            .filter(|i| !i.installed)
            .map(|i| (i.version, i.title))
            .collect();
        let folder = ctx.game_dir.join(content_type.folder());
        install_files(&installer.downloader, &folder, &items, &reporter, &cancel).await?;
        Ok(())
    }))
}

/// 新建 Fabric / Quilt 实例后自动安装 Fabric API / QSL（及其前置）。
pub async fn install_loader_api(
    state: &AppState,
    instance: &str,
    reporter: &Reporter,
    cancel: &CancelToken,
) -> AppResult<()> {
    let ctx = instance_context(state, instance)?;
    let (project, name) = match ctx.loader {
        Some(LoaderKind::Fabric) => (FABRIC_API, "Fabric API"),
        Some(LoaderKind::Quilt) => (QSL, "QSL"),
        _ => return Ok(()),
    };
    install_best(state, &ctx, project, name, reporter, cancel).await
}

pub const IRIS: &str = "YL57xq9U";
pub const OCULUS: &str = "GchcoXML";

/// 按加载器排列的光影 Mod 候选：Iris 支持 Fabric / Quilt / NeoForge，Oculus 支持 Forge 与旧版 NeoForge。
fn shader_candidates(loader: Option<LoaderKind>) -> &'static [(&'static str, &'static str)] {
    match loader {
        Some(LoaderKind::Fabric | LoaderKind::Quilt) => &[(IRIS, "Iris")],
        Some(LoaderKind::NeoForge) => &[(IRIS, "Iris"), (OCULUS, "Oculus")],
        Some(LoaderKind::Forge) => &[(OCULUS, "Oculus")],
        None => &[],
    }
}

/// 找到第一个有兼容版本的光影 Mod，返回（项目 id，名称，最佳版本）。
async fn find_shader_mod(
    state: &AppState,
    mc: &str,
    loader: Option<LoaderKind>,
) -> AppResult<Option<(&'static str, &'static str, ContentVersion)>> {
    let client = client(state, Platform::Modrinth)?;
    let ctx = InstanceContext {
        id: String::new(),
        mc_version: mc.to_string(),
        loader,
        game_dir: PathBuf::new(),
    };
    let filter = filter_for(&ctx, ContentType::Mod);
    for &(project, name) in shader_candidates(loader) {
        if let Some(best) = best_of(client.versions(project, &filter).await?) {
            return Ok(Some((project, name, best)));
        }
    }
    Ok(None)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderModInfo {
    name: String,
    version: String,
}

/// 新建实例时查询可用的光影 Mod（没有时返回 null）。
#[tauri::command]
pub async fn shader_mod_for(state: State<'_, AppState>, mc: String, loader: Option<LoaderKind>) -> AppResult<Option<ShaderModInfo>> {
    Ok(find_shader_mod(&state, &mc, loader).await?.map(|(_, name, v)| ShaderModInfo {
        name: name.to_string(),
        version: v.version_number,
    }))
}

/// 安装光影 Mod 及其前置（Sodium / Embeddium 等）。
pub async fn install_shader_mod(state: &AppState, instance: &str, reporter: &Reporter, cancel: &CancelToken) -> AppResult<()> {
    let ctx = instance_context(state, instance)?;
    reporter.stage("选择光影加载器版本");
    let Some((project, name, _)) = find_shader_mod(state, &ctx.mc_version, ctx.loader).await? else {
        return Err(AppError::msg(
            format!("没有适用于 Minecraft {} 的 Iris / Oculus", ctx.mc_version),
            "实例已经可以启动；可以在「资源」页面手动安装其他光影 Mod。",
        ));
    };
    install_best(state, &ctx, project, name, reporter, cancel).await
}

/// 安装某个 Modrinth 项目的最佳兼容版本，连同必需的前置。
async fn install_best(
    state: &AppState,
    ctx: &InstanceContext,
    project: &str,
    name: &str,
    reporter: &Reporter,
    cancel: &CancelToken,
) -> AppResult<()> {
    reporter.stage(&format!("选择 {name} 版本"));
    let client = client(state, Platform::Modrinth)?;
    let filter = filter_for(ctx, ContentType::Mod);
    let Some(best) = best_of(client.versions(project, &filter).await?) else {
        return Err(AppError::msg(
            format!("{name} 还没有适用于 Minecraft {} 的版本", ctx.mc_version),
            "实例已经可以启动；需要时可以在「资源」页面手动安装。",
        ));
    };
    // 已安装的项目（例如先装好的 Fabric API）不再重复安装
    let installed = content::installed_projects(&ctx.game_dir.join("mods"), Platform::Modrinth, None).await;
    let src = FilteredClient { client: &client, filter };
    let plan = content::resolve(&src, vec![best], &installed).await?;
    let items: Vec<(ContentVersion, String)> = plan.items.into_iter().map(|i| (i.version, i.title)).collect();
    let installer = state.installer()?;
    reporter.stage(&format!("下载 {name}"));
    install_files(&installer.downloader, &ctx.game_dir.join("mods"), &items, reporter, cancel).await?;
    Ok(())
}

/// 整合包来源：本地文件，或 Modrinth / CurseForge 上的整合包版本。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PackSource {
    File {
        path: PathBuf,
    },
    #[serde(rename_all = "camelCase")]
    Online {
        platform: Platform,
        project_id: String,
        version_id: String,
    },
}

/// 读取本地整合包信息（导入前预览）。
#[tauri::command]
pub async fn read_modpack_file(path: PathBuf) -> AppResult<content::ModpackInfo> {
    tauri::async_runtime::spawn_blocking(move || content::read_modpack(&path))
        .await
        .map_err(|e| AppError::msg(e.to_string(), ""))?
        .map_err(Into::into)
}

/// 把整合包名整理成可用的实例名，并避免与已有实例重名。
fn instance_name_for(state: &AppState, wanted: &str) -> String {
    let cleaned: String = wanted
        .trim()
        .chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    let base = cleaned.trim_matches(['.', ' ']).to_string();
    let base = if base.is_empty() { "整合包".to_string() } else { base };
    let game = state.game_dir();
    let mut name = base.clone();
    let mut i = 2;
    while game.version_dir(&name).exists() {
        name = format!("{base}-{i}");
        i += 1;
    }
    name
}

/// 整合包写的加载器版本可能是简写（如 Forge 的 47.2.0），这里换成安装用的完整版本号。
pub(crate) async fn loader_choice(
    state: &AppState,
    mc: &str,
    loader: &content::PackLoader,
) -> AppResult<launcher_core::loader::LoaderChoice> {
    use launcher_core::loader::{list_versions, LoaderChoice};
    if matches!(loader.kind, LoaderKind::Fabric | LoaderKind::Quilt) {
        return Ok(LoaderChoice {
            kind: loader.kind,
            version: loader.version.clone(),
        });
    }
    let list = list_versions(&state.http()?, loader.kind, mc).await?;
    let want = loader.version.as_str();
    let found = list
        .iter()
        .find(|v| v.version == want || v.display == want || v.version == format!("{mc}-{want}"))
        .ok_or_else(|| {
            AppError::msg(
                format!("找不到整合包需要的 {} {want}（Minecraft {mc}）", loader.kind.display_name()),
                "这个加载器版本可能已被下架，可以尝试安装该整合包的其它版本。",
            )
        })?;
    Ok(LoaderChoice {
        kind: loader.kind,
        version: found.version.clone(),
    })
}

/// 安装整合包：创建新实例（强制版本隔离），装好对应的游戏版本与加载器，再下载文件并解压 overrides。
#[tauri::command]
pub async fn install_modpack(
    app: AppHandle,
    state: State<'_, AppState>,
    source: PackSource,
    name: Option<String>,
) -> AppResult<u64> {
    let installer = state.installer()?;
    let app2 = app.clone();
    Ok(state.tasks.spawn(&app, "安装整合包", move |reporter, cancel| async move {
        let state = app2.state::<AppState>();
        let tmp = installer.game.root().join(".redcraft-tmp");

        // 1. 获取整合包文件
        let (pack_path, downloaded) = match &source {
            PackSource::File { path } => (path.clone(), false),
            PackSource::Online { platform, project_id, version_id } => {
                reporter.stage("获取整合包信息");
                let version = client(&state, *platform)?.version(project_id, version_id).await?;
                let file = version
                    .primary_file()
                    .ok_or_else(|| AppError::msg("这个整合包版本没有可下载的文件", ""))?
                    .clone();
                let path = tmp.join(&file.filename);
                reporter.stage("下载整合包");
                installer
                    .downloader
                    .download_all(
                        vec![launcher_core::download::DownloadTask::new(&file.url, &path)
                            .sha1(file.sha1.clone())
                            .size(Some(file.size))],
                        reporter.progress.clone(),
                        &cancel,
                    )
                    .await?;
                (path, true)
            }
        };
        let info = {
            let p = pack_path.clone();
            tauri::async_runtime::spawn_blocking(move || content::read_modpack(&p))
                .await
                .map_err(|e| AppError::msg(e.to_string(), ""))??
        };
        let name = instance_name_for(&state, name.as_deref().unwrap_or(&info.name));
        log::info!("安装整合包 {} {} 为实例 {name}", info.name, info.version);

        // 2. 游戏版本与加载器
        let manifest = launcher_core::version::VersionManifest::fetch(&state.http()?).await?;
        let entry = manifest.find(&info.mc_version).cloned().ok_or_else(|| {
            AppError::msg(format!("版本列表中没有整合包需要的 Minecraft {}", info.mc_version), "请稍后重试。")
        })?;
        let choice = match &info.loader {
            Some(l) => Some(loader_choice(&state, &info.mc_version, l).await?),
            None => None,
        };
        crate::commands::create_instance(&state, &installer, &entry, choice.as_ref(), None, &name, Some(true), &reporter, &cancel)
            .await?;

        // 3. 文件与 overrides；失败时删除新建的实例
        let game_dir = installer.game.version_dir(&name);
        let result: AppResult<()> = async {
            let cf = CurseForge::new(state.http()?, Some(state.settings().curseforge_api_key));
            content::download_pack_files(&info, &game_dir, &installer.downloader, &cf, &reporter, &cancel).await?;
            reporter.stage("解压整合包配置");
            let (p, i, g) = (pack_path.clone(), info.clone(), game_dir.clone());
            tauri::async_runtime::spawn_blocking(move || content::extract_overrides(&p, &i, &g))
                .await
                .map_err(|e| AppError::msg(e.to_string(), ""))??;
            Ok(())
        }
        .await;
        if downloaded {
            let _ = std::fs::remove_file(&pack_path);
        }
        if result.is_err() {
            let _ = std::fs::remove_dir_all(installer.game.version_dir(&name));
        }
        result
    }))
}

// ───────────── 已安装 Mod 管理 ─────────────

fn mods_dir(state: &AppState, instance: &str) -> AppResult<PathBuf> {
    Ok(instance_context(state, instance)?.game_dir.join("mods"))
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| AppError::msg(format!("后台任务异常：{e}"), "请重试"))
}

#[tauri::command]
pub async fn mods_list(state: State<'_, AppState>, instance: String) -> AppResult<Vec<content::local::LocalMod>> {
    let dir = mods_dir(&state, &instance)?;
    blocking(move || content::local::scan_mods(&dir)).await
}

/// 启用 / 禁用，返回新的文件名。
#[tauri::command]
pub fn mod_set_enabled(state: State<'_, AppState>, instance: String, filename: String, enabled: bool) -> AppResult<String> {
    ensure_not_running(&state, &instance)?;
    Ok(content::local::set_enabled(&mods_dir(&state, &instance)?, &filename, enabled)?)
}

#[tauri::command]
pub fn mod_delete(state: State<'_, AppState>, instance: String, filenames: Vec<String>) -> AppResult<()> {
    ensure_not_running(&state, &instance)?;
    let dir = mods_dir(&state, &instance)?;
    for f in filenames {
        content::local::delete(&dir, &f)?;
    }
    Ok(())
}

fn ensure_not_running(state: &AppState, instance: &str) -> AppResult<()> {
    if state.running.lock().is_ok_and(|r| r.contains_key(instance)) {
        return Err(AppError::msg("游戏正在运行", "Mod 文件被游戏占用，请先关闭游戏再修改。"));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModUpdate {
    pub filename: String,
    pub title: String,
    pub current: String,
    pub latest: ContentVersion,
}

/// 检查更新：Modrinth 用文件 SHA1 查询（/version_files/update），
/// 从 CurseForge 安装的 Mod 按安装记录查询最新兼容版本。
#[tauri::command]
pub async fn mods_check_updates(state: State<'_, AppState>, instance: String) -> AppResult<Vec<ModUpdate>> {
    let ctx = instance_context(&state, &instance)?;
    let dir = ctx.game_dir.join("mods");
    let filter = filter_for(&ctx, ContentType::Mod);
    let (hashes, index) = {
        let d = dir.clone();
        blocking(move || {
            let mut index = content::ContentIndex::load(&d);
            index.prune(&d);
            (content::local::hash_mods(&d), index)
        })
        .await?
    };
    let mut updates = Vec::new();

    let modrinth = Modrinth::new(state.http()?);
    let all: Vec<String> = hashes.values().cloned().collect();
    let current = modrinth.versions_by_hash(&all).await?;
    let latest = modrinth.latest_by_hash(&all, &filter).await?;
    let mut handled = HashSet::new();
    for (file, hash) in &hashes {
        let (Some(cur), Some(new)) = (current.get(hash), latest.get(hash)) else { continue };
        handled.insert(file.clone());
        if cur.id != new.id && new.published > cur.published {
            updates.push(ModUpdate {
                filename: file.clone(),
                title: index
                    .files
                    .get(content::base_name(file))
                    .map(|e| e.title.clone())
                    .unwrap_or_else(|| cur.name.clone()),
                current: cur.version_number.clone(),
                latest: new.clone(),
            });
        }
    }

    let cf = client(&state, Platform::Curseforge)?;
    for (file, _) in hashes.iter().filter(|(f, _)| !handled.contains(*f)) {
        let Some(entry) = index.files.get(content::base_name(file)) else { continue };
        if entry.platform != Platform::Curseforge {
            continue;
        }
        let Ok(list) = cf.versions(&entry.project_id, &filter).await else { continue };
        if let Some(best) = best_of(list) {
            if best.id != entry.version_id {
                updates.push(ModUpdate {
                    filename: file.clone(),
                    title: entry.title.clone(),
                    current: entry.version_number.clone(),
                    latest: best,
                });
            }
        }
    }
    updates.sort_by_key(|u| u.title.to_lowercase());
    Ok(updates)
}

/// 应用更新：下载新版本，删除旧文件；原来禁用的 Mod 更新后仍保持禁用。
#[tauri::command]
pub async fn mods_update(
    app: AppHandle,
    state: State<'_, AppState>,
    instance: String,
    updates: Vec<ModUpdate>,
) -> AppResult<u64> {
    ensure_not_running(&state, &instance)?;
    let dir = mods_dir(&state, &instance)?;
    let installer = state.installer()?;
    Ok(state.tasks.spawn(&app, format!("更新 {} 个 Mod", updates.len()), move |reporter, cancel| async move {
        let items: Vec<(ContentVersion, String)> = updates.iter().map(|u| (u.latest.clone(), u.title.clone())).collect();
        let installed = install_files(&installer.downloader, &dir, &items, &reporter, &cancel).await?;
        for (u, new_name) in updates.iter().zip(installed) {
            let was_disabled = u.filename.ends_with(".disabled");
            // 旧文件（包括同名但带 .disabled 的）在新文件下载完成后删除
            if u.filename != new_name {
                let _ = std::fs::remove_file(dir.join(&u.filename));
                if content::base_name(&u.filename) != new_name {
                    let mut index = content::ContentIndex::load(&dir);
                    index.files.remove(content::base_name(&u.filename));
                    let _ = index.save(&dir);
                }
            }
            if was_disabled {
                let _ = content::local::set_enabled(&dir, &new_name, false);
            }
        }
        Ok(())
    }))
}

// ───────────── 图标与中文简介 ─────────────

/// 批量获取简介的中文翻译（MCIM）。按平台与 id 缓存在内存中，失败的 id 下次再试。
#[tauri::command]
pub async fn content_translations(
    state: State<'_, AppState>,
    platform: Platform,
    ids: Vec<String>,
) -> AppResult<std::collections::HashMap<String, String>> {
    let key = |id: &str| format!("{platform:?}:{id}");
    let mut out = std::collections::HashMap::new();
    let mut missing = Vec::new();
    if let Ok(cache) = state.translations.lock() {
        for id in ids {
            match cache.get(&key(&id)) {
                Some(t) => {
                    out.insert(id, t.clone());
                }
                None => missing.push(id),
            }
        }
    }
    if !missing.is_empty() {
        let http = state.http()?;
        let fetched = launcher_core::content::media::translations(http.raw(), platform, &missing).await;
        if let Ok(mut cache) = state.translations.lock() {
            for (id, t) in &fetched {
                cache.insert(key(id), t.clone());
            }
        }
        out.extend(fetched);
    }
    Ok(out)
}

/// `rcicon://` 协议：前端以 `rcicon://localhost/<编码后的图标地址>` 加载资源图标。
/// 先读磁盘缓存，没有时同时请求官方地址与 MCIM 镜像。
pub async fn serve_icon(app: AppHandle, request: tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    use launcher_core::content::media::{fetch_icon, image_mime, is_icon_url};
    let not_found = || tauri::http::Response::builder().status(404).body(Vec::new()).unwrap();
    let path = request.uri().path().trim_start_matches('/');
    let Ok(url) = percent_encoding::percent_decode_str(path).decode_utf8() else {
        return not_found();
    };
    let url = url.to_string();
    if !is_icon_url(&url) {
        return not_found();
    }
    let state = app.state::<AppState>();
    let dir = state.paths.config_dir.join("cache").join("icons");
    let file = dir.join(launcher_core::download::sha1_bytes(url.as_bytes()));
    let bytes = match tokio::fs::read(&file).await {
        Ok(b) if image_mime(&b).is_some() => Some(b),
        _ => {
            let Ok(http) = state.http() else { return not_found() };
            let fetched = fetch_icon(http.raw(), &url).await;
            if let Some(b) = &fetched {
                let _ = tokio::fs::create_dir_all(&dir).await;
                let _ = tokio::fs::write(&file, b).await;
            }
            fetched
        }
    };
    match bytes {
        Some(b) => tauri::http::Response::builder()
            .header("Content-Type", image_mime(&b).unwrap_or("application/octet-stream"))
            .header("Cache-Control", "max-age=604800")
            .body(b)
            .unwrap(),
        None => not_found(),
    }
}
