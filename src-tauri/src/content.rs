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
    let project = match ctx.loader {
        Some(LoaderKind::Fabric) => FABRIC_API,
        Some(LoaderKind::Quilt) => QSL,
        _ => return Ok(()),
    };
    let name = if project == FABRIC_API { "Fabric API" } else { "QSL" };
    reporter.stage(&format!("选择 {name} 版本"));
    let client = client(state, Platform::Modrinth)?;
    let filter = filter_for(&ctx, ContentType::Mod);
    let Some(best) = best_of(client.versions(project, &filter).await?) else {
        return Err(AppError::msg(
            format!("{name} 还没有适用于 Minecraft {} 的版本", ctx.mc_version),
            "实例已经可以启动；需要时可以在「资源」页面手动安装。",
        ));
    };
    let src = FilteredClient { client: &client, filter };
    let plan = content::resolve(&src, vec![best], &HashSet::new()).await?;
    let items: Vec<(ContentVersion, String)> = plan.items.into_iter().map(|i| (i.version, i.title)).collect();
    let installer = state.installer()?;
    reporter.stage(&format!("下载 {name}"));
    install_files(&installer.downloader, &ctx.game_dir.join("mods"), &items, reporter, cancel).await?;
    Ok(())
}
