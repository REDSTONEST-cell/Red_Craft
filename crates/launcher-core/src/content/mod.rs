//! 资源下载：Mod、材质包、光影包、整合包。数据源为 Modrinth 与 CurseForge，两者统一为同一套模型。

mod curseforge;
pub mod export;
pub mod files;
mod index;
pub mod local;
pub mod media;
pub mod zhdb;
pub mod aggregate;
mod modpack;
mod modrinth;
mod resolve;

use std::future::Future;

use serde::{Deserialize, Serialize};

pub use curseforge::CurseForge;
pub use index::{base_name, install_files, installed_projects, ContentIndex, IndexEntry, INDEX_FILE};
pub use modpack::{download_pack_files, extract_overrides, read_modpack, ModpackInfo, PackFormat, PackLoader};
pub use modrinth::{loader_names, Modrinth};
use modrinth::enc as modrinth_enc;
pub use resolve::{resolve, DepSource, MissingDep, Plan, PlanItem};

use crate::error::Result;
use crate::instance::LoaderKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Modrinth,
    Curseforge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    Mod,
    Resourcepack,
    Shader,
    Modpack,
}

impl ContentType {
    /// 安装到实例游戏目录下的文件夹。整合包不直接安装到文件夹。
    pub fn folder(self) -> &'static str {
        match self {
            ContentType::Mod => "mods",
            ContentType::Resourcepack => "resourcepacks",
            ContentType::Shader => "shaderpacks",
            ContentType::Modpack => "modpacks",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ContentType::Mod => "Mod",
            ContentType::Resourcepack => "材质包",
            ContentType::Shader => "光影包",
            ContentType::Modpack => "整合包",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortBy {
    #[default]
    Relevance,
    Downloads,
    Updated,
    Newest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    #[serde(default)]
    pub query: String,
    pub content_type: ContentType,
    #[serde(default)]
    pub game_version: Option<String>,
    #[serde(default)]
    pub loader: Option<LoaderKind>,
    #[serde(default)]
    pub sort: SortBy,
    #[serde(default)]
    pub offset: u32,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectHit {
    pub platform: Platform,
    pub id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub icon_url: Option<String>,
    pub downloads: u64,
    pub categories: Vec<String>,
    pub content_type: ContentType,
    pub updated: String,
    /// 中文名（来自 Mod 中文名数据库或常用译名表）
    #[serde(default)]
    pub zh_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub hits: Vec<ProjectHit>,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContentFile {
    pub url: String,
    pub filename: String,
    pub sha1: Option<String>,
    pub size: u64,
    pub primary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DepKind {
    Required,
    Optional,
    Incompatible,
    Embedded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Dependency {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub kind: DepKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContentVersion {
    pub platform: Platform,
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    /// `release` / `beta` / `alpha`
    pub release_type: String,
    pub published: String,
    pub downloads: u64,
    pub files: Vec<ContentFile>,
    pub dependencies: Vec<Dependency>,
}

impl ContentVersion {
    /// 主文件（Modrinth 可能有多个文件，取标记为 primary 的）。
    pub fn primary_file(&self) -> Option<&ContentFile> {
        self.files.iter().find(|f| f.primary).or(self.files.first())
    }
}

/// 版本筛选条件：只列出兼容当前实例的版本。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VersionFilter {
    #[serde(default)]
    pub game_version: Option<String>,
    #[serde(default)]
    pub loader: Option<LoaderKind>,
    pub content_type: Option<ContentType>,
}

impl VersionFilter {
    /// 只有 Mod 与整合包需要按加载器筛选。
    pub fn uses_loader(&self) -> bool {
        matches!(self.content_type, None | Some(ContentType::Mod) | Some(ContentType::Modpack))
    }
}

/// 在多个版本中选出最合适的：优先正式版，其次最新发布。
pub fn best_of(mut versions: Vec<ContentVersion>) -> Option<ContentVersion> {
    versions.sort_by(|a, b| b.published.cmp(&a.published));
    let rank = |v: &ContentVersion| match v.release_type.as_str() {
        "release" => 0,
        "beta" => 1,
        _ => 2,
    };
    let best_rank = versions.iter().map(rank).min()?;
    versions.into_iter().find(|v| rank(v) == best_rank)
}

/// 两个平台的统一入口。
#[derive(Clone)]
pub enum Client {
    Modrinth(Modrinth),
    CurseForge(CurseForge),
}

impl Client {
    pub fn platform(&self) -> Platform {
        match self {
            Client::Modrinth(_) => Platform::Modrinth,
            Client::CurseForge(_) => Platform::Curseforge,
        }
    }

    pub async fn search(&self, q: &SearchQuery) -> Result<SearchResult> {
        match self {
            Client::Modrinth(c) => c.search(q).await,
            Client::CurseForge(c) => c.search(q).await,
        }
    }

    pub async fn versions(&self, project_id: &str, filter: &VersionFilter) -> Result<Vec<ContentVersion>> {
        match self {
            Client::Modrinth(c) => c.versions(project_id, filter).await,
            Client::CurseForge(c) => c.versions(project_id, filter).await,
        }
    }

    pub async fn project(&self, project_id: &str) -> Result<ProjectHit> {
        match self {
            Client::Modrinth(c) => c.project(project_id).await,
            Client::CurseForge(c) => c.project(project_id).await,
        }
    }

    pub async fn version(&self, project_id: &str, version_id: &str) -> Result<ContentVersion> {
        match self {
            Client::Modrinth(c) => c.version(version_id).await,
            Client::CurseForge(c) => c.file(project_id, version_id).await,
        }
    }
}

/// 解析依赖时使用的数据源：固定了筛选条件的 [`Client`]。
pub struct FilteredClient<'a> {
    pub client: &'a Client,
    pub filter: VersionFilter,
}

impl DepSource for FilteredClient<'_> {
    fn version(&self, project_id: &str, version_id: &str) -> impl Future<Output = Result<ContentVersion>> + Send {
        self.client.version(project_id, version_id)
    }

    async fn best_version(&self, project_id: &str) -> Result<Option<ContentVersion>> {
        // 依赖按 Mod 的规则筛选（材质包、光影包不会有依赖）
        let mut filter = self.filter.clone();
        filter.content_type = Some(ContentType::Mod);
        Ok(best_of(self.client.versions(project_id, &filter).await?))
    }

    async fn title(&self, project_id: &str) -> Result<String> {
        Ok(self.client.project(project_id).await?.title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(id: &str, kind: &str, date: &str) -> ContentVersion {
        ContentVersion {
            platform: Platform::Modrinth,
            id: id.into(),
            project_id: "p".into(),
            name: id.into(),
            version_number: id.into(),
            game_versions: vec![],
            loaders: vec![],
            release_type: kind.into(),
            published: date.into(),
            downloads: 0,
            files: vec![],
            dependencies: vec![],
        }
    }

    #[test]
    fn best_prefers_latest_release() {
        let best = best_of(vec![
            v("a", "release", "2023-01-01"),
            v("b", "beta", "2024-01-01"),
            v("c", "release", "2023-06-01"),
        ])
        .unwrap();
        assert_eq!(best.id, "c");
        let best = best_of(vec![v("x", "alpha", "2022"), v("y", "beta", "2021")]).unwrap();
        assert_eq!(best.id, "y");
        assert!(best_of(vec![]).is_none());
    }
}
