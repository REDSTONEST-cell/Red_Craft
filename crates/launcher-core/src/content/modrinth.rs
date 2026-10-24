//! Modrinth API（https://api.modrinth.com/v2）。下载源设置为镜像时走 MCIM 镜像。

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::json;

use super::{
    ContentFile, ContentType, ContentVersion, DepKind, Dependency, Platform, ProjectHit, SearchQuery, SearchResult,
    SortBy, VersionFilter,
};
use crate::download::HttpClient;
use crate::error::Result;
use crate::instance::LoaderKind;

const API: &str = "https://api.modrinth.com/v2";

#[derive(Clone)]
pub struct Modrinth {
    http: HttpClient,
}

/// 实例加载器对应的 Modrinth 加载器名。Quilt 可以运行 Fabric Mod，1.20.1 的 NeoForge 可以运行 Forge Mod。
pub fn loader_names(loader: LoaderKind, mc: Option<&str>) -> Vec<&'static str> {
    match loader {
        LoaderKind::Fabric => vec!["fabric"],
        LoaderKind::Quilt => vec!["quilt", "fabric"],
        LoaderKind::Forge => vec!["forge"],
        LoaderKind::NeoForge if mc == Some("1.20.1") => vec!["neoforge", "forge"],
        LoaderKind::NeoForge => vec!["neoforge"],
    }
}

fn project_type(t: ContentType) -> &'static str {
    match t {
        ContentType::Mod => "mod",
        ContentType::Resourcepack => "resourcepack",
        ContentType::Shader => "shader",
        ContentType::Modpack => "modpack",
    }
}

fn parse_type(s: &str) -> ContentType {
    match s {
        "resourcepack" => ContentType::Resourcepack,
        "shader" => ContentType::Shader,
        "modpack" => ContentType::Modpack,
        _ => ContentType::Mod,
    }
}

pub(super) fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[derive(Deserialize)]
struct RawSearch {
    hits: Vec<RawHit>,
    total_hits: u64,
}

#[derive(Deserialize)]
struct RawHit {
    project_id: String,
    slug: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    author: String,
    icon_url: Option<String>,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    display_categories: Vec<String>,
    #[serde(default)]
    categories: Vec<String>,
    project_type: String,
    #[serde(default)]
    date_modified: String,
}

#[derive(Deserialize)]
struct RawProject {
    id: String,
    slug: String,
    title: String,
    #[serde(default)]
    description: String,
    icon_url: Option<String>,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    categories: Vec<String>,
    project_type: String,
    #[serde(default)]
    updated: String,
}

#[derive(Deserialize)]
pub(super) struct RawVersion {
    id: String,
    project_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version_number: String,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(default)]
    version_type: String,
    #[serde(default)]
    date_published: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    files: Vec<RawFile>,
    #[serde(default)]
    dependencies: Vec<RawDep>,
}

#[derive(Deserialize)]
struct RawFile {
    url: String,
    filename: String,
    #[serde(default)]
    hashes: HashMap<String, String>,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    primary: bool,
}

#[derive(Deserialize)]
struct RawDep {
    version_id: Option<String>,
    project_id: Option<String>,
    dependency_type: String,
}

impl From<RawVersion> for ContentVersion {
    fn from(v: RawVersion) -> Self {
        ContentVersion {
            platform: Platform::Modrinth,
            id: v.id,
            project_id: v.project_id,
            name: v.name,
            version_number: v.version_number,
            game_versions: v.game_versions,
            loaders: v.loaders,
            release_type: v.version_type,
            published: v.date_published,
            downloads: v.downloads,
            files: v
                .files
                .into_iter()
                .map(|f| ContentFile {
                    sha1: f.hashes.get("sha1").cloned(),
                    url: f.url,
                    filename: f.filename,
                    size: f.size,
                    primary: f.primary,
                })
                .collect(),
            dependencies: v
                .dependencies
                .into_iter()
                .map(|d| Dependency {
                    project_id: d.project_id,
                    version_id: d.version_id,
                    kind: match d.dependency_type.as_str() {
                        "required" => DepKind::Required,
                        "incompatible" => DepKind::Incompatible,
                        "embedded" => DepKind::Embedded,
                        _ => DepKind::Optional,
                    },
                })
                .collect(),
        }
    }
}

impl Modrinth {
    pub fn new(http: HttpClient) -> Self {
        Modrinth { http }
    }

    pub async fn search(&self, q: &SearchQuery) -> Result<SearchResult> {
        let mut facets: Vec<Vec<String>> = vec![vec![format!("project_type:{}", project_type(q.content_type))]];
        if let Some(v) = q.game_version.as_deref().filter(|v| !v.is_empty()) {
            facets.push(vec![format!("versions:{v}")]);
        }
        if matches!(q.content_type, ContentType::Mod | ContentType::Modpack) {
            if let Some(l) = q.loader {
                facets.push(
                    loader_names(l, q.game_version.as_deref())
                        .into_iter()
                        .map(|n| format!("categories:{n}"))
                        .collect(),
                );
            }
        }
        let index = match q.sort {
            SortBy::Relevance => "relevance",
            SortBy::Downloads => "downloads",
            SortBy::Updated => "updated",
            SortBy::Newest => "newest",
        };
        let url = format!(
            "{API}/search?query={}&facets={}&index={index}&offset={}&limit={}",
            enc(&q.query),
            enc(&serde_json::to_string(&facets).unwrap_or_default()),
            q.offset,
            q.limit.clamp(1, 100)
        );
        let raw: RawSearch = self.http.get_json(&url, "Modrinth 搜索结果").await?;
        Ok(SearchResult {
            total: raw.total_hits,
            hits: raw
                .hits
                .into_iter()
                .map(|h| ProjectHit {
                    platform: Platform::Modrinth,
                    content_type: parse_type(&h.project_type),
                    id: h.project_id,
                    slug: h.slug,
                    title: h.title,
                    description: h.description,
                    author: h.author,
                    icon_url: h.icon_url.filter(|u| !u.is_empty()),
                    downloads: h.downloads,
                    categories: if h.display_categories.is_empty() { h.categories } else { h.display_categories },
                    updated: h.date_modified,
                })
                .collect(),
        })
    }

    pub async fn project(&self, id: &str) -> Result<ProjectHit> {
        let p: RawProject = self.http.get_json(&format!("{API}/project/{}", enc(id)), "Modrinth 项目信息").await?;
        Ok(ProjectHit {
            platform: Platform::Modrinth,
            content_type: parse_type(&p.project_type),
            id: p.id,
            slug: p.slug,
            title: p.title,
            description: p.description,
            author: String::new(),
            icon_url: p.icon_url.filter(|u| !u.is_empty()),
            downloads: p.downloads,
            categories: p.categories,
            updated: p.updated,
        })
    }

    /// 兼容筛选条件的版本，新版本在前。
    pub async fn versions(&self, project_id: &str, filter: &VersionFilter) -> Result<Vec<ContentVersion>> {
        let mut params = Vec::new();
        if filter.uses_loader() {
            if let Some(l) = filter.loader {
                let names = loader_names(l, filter.game_version.as_deref());
                params.push(format!("loaders={}", enc(&serde_json::to_string(&names).unwrap_or_default())));
            }
        }
        if let Some(v) = filter.game_version.as_deref().filter(|v| !v.is_empty()) {
            params.push(format!("game_versions={}", enc(&serde_json::to_string(&[v]).unwrap_or_default())));
        }
        let url = format!("{API}/project/{}/version?{}", enc(project_id), params.join("&"));
        let raw: Vec<RawVersion> = self.http.get_json(&url, "Modrinth 版本列表").await?;
        Ok(raw.into_iter().map(ContentVersion::from).collect())
    }

    pub async fn version(&self, id: &str) -> Result<ContentVersion> {
        let raw: RawVersion = self.http.get_json(&format!("{API}/version/{}", enc(id)), "Modrinth 版本信息").await?;
        Ok(raw.into())
    }

    /// 用文件 SHA1 查询对应的版本（识别已安装的 Mod）。
    pub async fn versions_by_hash(&self, hashes: &[String]) -> Result<HashMap<String, ContentVersion>> {
        if hashes.is_empty() {
            return Ok(HashMap::new());
        }
        let raw: HashMap<String, RawVersion> = self
            .http
            .post_json(
                &format!("{API}/version_files"),
                &json!({ "hashes": hashes, "algorithm": "sha1" }),
                "Modrinth 文件查询结果",
            )
            .await?;
        Ok(raw.into_iter().map(|(k, v)| (k, v.into())).collect())
    }

    /// 检查更新：返回每个文件 SHA1 对应的、兼容筛选条件的最新版本。
    pub async fn latest_by_hash(&self, hashes: &[String], filter: &VersionFilter) -> Result<HashMap<String, ContentVersion>> {
        if hashes.is_empty() {
            return Ok(HashMap::new());
        }
        let mut body = json!({ "hashes": hashes, "algorithm": "sha1" });
        if let Some(l) = filter.loader.filter(|_| filter.uses_loader()) {
            body["loaders"] = json!(loader_names(l, filter.game_version.as_deref()));
        }
        if let Some(v) = &filter.game_version {
            body["game_versions"] = json!([v]);
        }
        let raw: HashMap<String, RawVersion> = self
            .http
            .post_json(&format!("{API}/version_files/update"), &body, "Modrinth 更新查询结果")
            .await?;
        Ok(raw.into_iter().map(|(k, v)| (k, v.into())).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version() {
        let raw: RawVersion = serde_json::from_str(
            r#"{"id":"v1","project_id":"p1","name":"Sodium Extra 0.5","version_number":"0.5.0","game_versions":["1.20.1"],
                "loaders":["fabric","quilt"],"version_type":"release","date_published":"2023-07-01T00:00:00Z","downloads":5,
                "files":[{"url":"https://cdn.modrinth.com/data/p1/versions/v1/a.jar","filename":"a.jar","hashes":{"sha1":"abc","sha512":"x"},"size":10,"primary":true}],
                "dependencies":[{"version_id":null,"project_id":"AANobbMI","file_name":null,"dependency_type":"required"},
                                {"version_id":null,"project_id":"x","dependency_type":"incompatible"}]}"#,
        )
        .unwrap();
        let v: ContentVersion = raw.into();
        assert_eq!(v.primary_file().unwrap().sha1.as_deref(), Some("abc"));
        assert_eq!(v.dependencies[0].kind, DepKind::Required);
        assert_eq!(v.dependencies[1].kind, DepKind::Incompatible);
    }

    #[test]
    fn loaders() {
        assert_eq!(loader_names(LoaderKind::Quilt, None), vec!["quilt", "fabric"]);
        assert_eq!(loader_names(LoaderKind::NeoForge, Some("1.20.1")), vec!["neoforge", "forge"]);
        assert_eq!(loader_names(LoaderKind::NeoForge, Some("1.21.1")), vec!["neoforge"]);
        assert_eq!(enc("a b+c"), "a%20b%2Bc");
    }
}
