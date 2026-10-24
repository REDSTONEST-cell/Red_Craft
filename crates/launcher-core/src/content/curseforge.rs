//! CurseForge API（https://api.curseforge.com/v1）。
//!
//! 官方接口需要 API Key；没有配置 Key 时只使用 MCIM 镜像（无需 Key）。
//! 配置了 Key 时按下载源设置在官方与镜像之间回退。

use serde::Deserialize;

use super::{
    ContentFile, ContentType, ContentVersion, DepKind, Dependency, Platform, ProjectHit, SearchQuery, SearchResult,
    SortBy, VersionFilter,
};
use crate::download::{mirror_of, HttpClient};
use crate::error::Result;
use crate::instance::LoaderKind;

const API: &str = "https://api.curseforge.com/v1";
const MINECRAFT: u32 = 432;

#[derive(Clone)]
pub struct CurseForge {
    http: HttpClient,
    api_key: Option<String>,
}

fn class_id(t: ContentType) -> u32 {
    match t {
        ContentType::Mod => 6,
        ContentType::Resourcepack => 12,
        ContentType::Shader => 6552,
        ContentType::Modpack => 4471,
    }
}

fn type_of_class(id: u32) -> ContentType {
    match id {
        12 => ContentType::Resourcepack,
        6552 => ContentType::Shader,
        4471 => ContentType::Modpack,
        _ => ContentType::Mod,
    }
}

/// CurseForge 的 modLoaderType。Quilt 实例使用 Fabric 的 Mod（CurseForge 上大多只标记 Fabric）。
fn loader_type(l: LoaderKind) -> u32 {
    match l {
        LoaderKind::Forge => 1,
        LoaderKind::Fabric | LoaderKind::Quilt => 4,
        LoaderKind::NeoForge => 6,
    }
}

const LOADER_NAMES: &[&str] = &["forge", "fabric", "quilt", "neoforge", "liteloader"];

#[derive(Deserialize)]
struct Data<T> {
    data: T,
}

#[derive(Deserialize)]
struct RawSearch {
    data: Vec<RawMod>,
    pagination: Pagination,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pagination {
    total_count: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMod {
    id: u64,
    #[serde(default)]
    slug: String,
    name: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    download_count: f64,
    logo: Option<Logo>,
    #[serde(default)]
    authors: Vec<Named>,
    #[serde(default)]
    categories: Vec<Named>,
    #[serde(default)]
    class_id: Option<u32>,
    #[serde(default)]
    date_modified: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Logo {
    #[serde(default)]
    thumbnail_url: String,
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFile {
    id: u64,
    mod_id: u64,
    #[serde(default)]
    display_name: String,
    file_name: String,
    #[serde(default)]
    release_type: u32,
    #[serde(default)]
    file_date: String,
    #[serde(default)]
    download_count: f64,
    download_url: Option<String>,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    hashes: Vec<Hash>,
    #[serde(default)]
    file_length: u64,
    #[serde(default)]
    dependencies: Vec<RawDep>,
}

#[derive(Deserialize)]
struct Hash {
    value: String,
    algo: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDep {
    mod_id: u64,
    relation_type: u32,
}

/// 作者关闭了第三方下载时 downloadUrl 为空，按 CDN 规则拼出地址。
fn file_url(f: &RawFile) -> String {
    match &f.download_url {
        Some(u) if !u.is_empty() => u.clone(),
        _ => format!("https://edge.forgecdn.net/files/{}/{}/{}", f.id / 1000, f.id % 1000, f.file_name),
    }
}

impl From<RawFile> for ContentVersion {
    fn from(f: RawFile) -> Self {
        let url = file_url(&f);
        let (loaders, game_versions): (Vec<String>, Vec<String>) = f
            .game_versions
            .iter()
            .map(|g| g.to_ascii_lowercase())
            .partition(|g| LOADER_NAMES.contains(&g.as_str()));
        let game_versions = game_versions
            .into_iter()
            .filter(|g| g.chars().next().is_some_and(|c| c.is_ascii_digit()))
            .collect();
        ContentVersion {
            platform: Platform::Curseforge,
            id: f.id.to_string(),
            project_id: f.mod_id.to_string(),
            name: f.display_name.clone(),
            version_number: f.display_name,
            game_versions,
            loaders,
            release_type: match f.release_type {
                2 => "beta",
                3 => "alpha",
                _ => "release",
            }
            .into(),
            published: f.file_date,
            downloads: f.download_count as u64,
            files: vec![ContentFile {
                url,
                filename: f.file_name,
                sha1: f.hashes.iter().find(|h| h.algo == 1).map(|h| h.value.clone()),
                size: f.file_length,
                primary: true,
            }],
            dependencies: f
                .dependencies
                .into_iter()
                .filter_map(|d| {
                    let kind = match d.relation_type {
                        3 => DepKind::Required,
                        2 => DepKind::Optional,
                        5 => DepKind::Incompatible,
                        1 | 6 => DepKind::Embedded,
                        _ => return None, // 4 = 工具
                    };
                    Some(Dependency {
                        project_id: Some(d.mod_id.to_string()),
                        version_id: None,
                        kind,
                    })
                })
                .collect(),
        }
    }
}

impl From<RawMod> for ProjectHit {
    fn from(m: RawMod) -> Self {
        ProjectHit {
            platform: Platform::Curseforge,
            id: m.id.to_string(),
            slug: m.slug,
            title: m.name,
            description: m.summary,
            author: m.authors.into_iter().map(|a| a.name).collect::<Vec<_>>().join(", "),
            icon_url: m.logo.map(|l| l.thumbnail_url).filter(|u| !u.is_empty()),
            downloads: m.download_count as u64,
            categories: m.categories.into_iter().map(|c| c.name).collect(),
            content_type: type_of_class(m.class_id.unwrap_or(6)),
            updated: m.date_modified,
        }
    }
}

impl CurseForge {
    pub fn new(http: HttpClient, api_key: Option<String>) -> Self {
        CurseForge {
            http,
            api_key: api_key.filter(|k| !k.trim().is_empty()),
        }
    }

    fn urls(&self, path: &str) -> Vec<String> {
        let official = format!("{API}{path}");
        match &self.api_key {
            Some(_) => self.http.candidates(&official),
            None => vec![mirror_of(&official).unwrap_or(official)],
        }
    }

    async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str, what: &str) -> Result<T> {
        let headers: Vec<(&str, &str)> = match &self.api_key {
            Some(k) => vec![("x-api-key", k.as_str())],
            None => vec![],
        };
        self.http.request_json(&self.urls(path), None, &headers, what).await
    }

    pub async fn search(&self, q: &SearchQuery) -> Result<SearchResult> {
        let mut path = format!(
            "/mods/search?gameId={MINECRAFT}&classId={}&index={}&pageSize={}&sortOrder=desc&sortField={}",
            class_id(q.content_type),
            q.offset,
            q.limit.clamp(1, 50),
            match q.sort {
                SortBy::Relevance => 2,
                SortBy::Downloads => 6,
                SortBy::Updated => 3,
                SortBy::Newest => 11,
            }
        );
        if !q.query.trim().is_empty() {
            path.push_str(&format!("&searchFilter={}", super::modrinth_enc(q.query.trim())));
        }
        if let Some(v) = q.game_version.as_deref().filter(|v| !v.is_empty()) {
            path.push_str(&format!("&gameVersion={v}"));
        }
        if matches!(q.content_type, ContentType::Mod | ContentType::Modpack) {
            if let Some(l) = q.loader {
                path.push_str(&format!("&modLoaderType={}", loader_type(l)));
            }
        }
        let raw: RawSearch = self.get(&path, "CurseForge 搜索结果").await?;
        Ok(SearchResult {
            total: raw.pagination.total_count.min(10_000),
            hits: raw.data.into_iter().map(ProjectHit::from).collect(),
        })
    }

    pub async fn project(&self, id: &str) -> Result<ProjectHit> {
        let raw: Data<RawMod> = self.get(&format!("/mods/{id}"), "CurseForge 项目信息").await?;
        Ok(raw.data.into())
    }

    pub async fn versions(&self, project_id: &str, filter: &VersionFilter) -> Result<Vec<ContentVersion>> {
        let mut path = format!("/mods/{project_id}/files?pageSize=50");
        if let Some(v) = filter.game_version.as_deref().filter(|v| !v.is_empty()) {
            path.push_str(&format!("&gameVersion={v}"));
        }
        if filter.uses_loader() {
            if let Some(l) = filter.loader {
                path.push_str(&format!("&modLoaderType={}", loader_type(l)));
            }
        }
        let raw: Data<Vec<RawFile>> = self.get(&path, "CurseForge 文件列表").await?;
        let mut list: Vec<ContentVersion> = raw.data.into_iter().map(ContentVersion::from).collect();
        list.sort_by(|a, b| b.published.cmp(&a.published));
        Ok(list)
    }

    pub async fn file(&self, project_id: &str, file_id: &str) -> Result<ContentVersion> {
        let raw: Data<RawFile> = self
            .get(&format!("/mods/{project_id}/files/{file_id}"), "CurseForge 文件信息")
            .await?;
        Ok(raw.data.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_file() {
        let f: RawFile = serde_json::from_str(
            r#"{"id":4712866,"modId":238222,"displayName":"jei-1.20.1-forge-15.2.0.27.jar","fileName":"jei-1.20.1-forge-15.2.0.27.jar",
                "releaseType":1,"fileDate":"2023-08-28T00:00:00Z","downloadCount":100,"downloadUrl":null,
                "gameVersions":["Forge","1.20.1","NeoForge","Client"],"hashes":[{"value":"aaa","algo":1},{"value":"m","algo":2}],
                "fileLength":1000,"dependencies":[{"modId":1,"relationType":3},{"modId":2,"relationType":4},{"modId":3,"relationType":2}]}"#,
        )
        .unwrap();
        let v: ContentVersion = f.into();
        assert_eq!(v.files[0].url, "https://edge.forgecdn.net/files/4712/866/jei-1.20.1-forge-15.2.0.27.jar");
        assert_eq!(v.files[0].sha1.as_deref(), Some("aaa"));
        assert_eq!(v.game_versions, vec!["1.20.1"]);
        assert_eq!(v.loaders, vec!["forge", "neoforge"]);
        assert_eq!(v.dependencies.len(), 2);
        assert_eq!(v.dependencies[0].kind, DepKind::Required);
        assert_eq!(v.dependencies[1].kind, DepKind::Optional);
    }

    #[test]
    fn mirror_only_without_key() {
        let cf = CurseForge::new(HttpClient::new(crate::download::DownloadSource::Auto).unwrap(), None);
        assert_eq!(cf.urls("/mods/1"), vec!["https://mod.mcimirror.top/curseforge/v1/mods/1".to_string()]);
        let cf = CurseForge::new(HttpClient::new(crate::download::DownloadSource::Auto).unwrap(), Some("k".into()));
        assert_eq!(cf.urls("/mods/1")[0], "https://api.curseforge.com/v1/mods/1");
    }
}
