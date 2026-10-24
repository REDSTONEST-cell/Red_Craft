//! 资源文件夹里的安装记录（`.redcraft-index.json`）：记住每个文件来自哪个平台的哪个项目，
//! 用于判断前置是否已安装、更新时替换旧文件。

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{ContentVersion, Modrinth, Platform};
use crate::download::{sha1_file, CancelToken, DownloadTask, Downloader};
use crate::error::{CoreError, IoContext, Result};
use crate::install::Reporter;

pub const INDEX_FILE: &str = ".redcraft-index.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IndexEntry {
    pub platform: Platform,
    pub project_id: String,
    pub version_id: String,
    pub title: String,
    pub version_number: String,
    #[serde(default)]
    pub sha1: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContentIndex {
    /// 文件名 → 记录
    #[serde(default)]
    pub files: HashMap<String, IndexEntry>,
}

/// 文件名去掉 `.disabled` 后缀。
pub fn base_name(name: &str) -> &str {
    name.strip_suffix(".disabled").unwrap_or(name)
}

impl ContentIndex {
    pub fn load(folder: &Path) -> Self {
        std::fs::read(folder.join(INDEX_FILE))
            .ok()
            .and_then(|d| serde_json::from_slice(&d).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, folder: &Path) -> Result<()> {
        let path = folder.join(INDEX_FILE);
        let data = serde_json::to_vec_pretty(self).map_err(|e| CoreError::json("安装记录", e))?;
        std::fs::write(&path, data).at(&path)
    }

    /// 去掉文件已被删除的记录。
    pub fn prune(&mut self, folder: &Path) {
        self.files.retain(|name, _| {
            folder.join(name).exists() || folder.join(format!("{name}.disabled")).exists()
        });
    }
}

/// 文件夹中已安装的项目 id。Modrinth 平台额外用文件 SHA1 识别手动放入的 Mod。
pub async fn installed_projects(folder: &Path, platform: Platform, modrinth: Option<&Modrinth>) -> HashSet<String> {
    let mut index = ContentIndex::load(folder);
    index.prune(folder);
    let mut out: HashSet<String> = index
        .files
        .values()
        .filter(|e| e.platform == platform)
        .map(|e| e.project_id.clone())
        .collect();
    if let (Platform::Modrinth, Some(mr)) = (platform, modrinth) {
        let known: HashSet<String> = index.files.keys().cloned().collect();
        let folder = folder.to_path_buf();
        let hashes = tokio::task::spawn_blocking(move || {
            let mut hashes = Vec::new();
            if let Ok(rd) = std::fs::read_dir(&folder) {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.starts_with('.') || known.contains(base_name(&name)) {
                        continue;
                    }
                    if let Ok(h) = sha1_file(&e.path()) {
                        hashes.push(h);
                    }
                }
            }
            hashes
        })
        .await
        .unwrap_or_default();
        if let Ok(found) = mr.versions_by_hash(&hashes).await {
            out.extend(found.into_values().map(|v| v.project_id));
        }
    }
    out
}

/// 下载并安装到资源文件夹；同一项目的旧文件会被替换。返回安装的文件名。
pub async fn install_files(
    downloader: &Downloader,
    folder: &Path,
    items: &[(ContentVersion, String)],
    reporter: &Reporter,
    cancel: &CancelToken,
) -> Result<Vec<String>> {
    tokio::fs::create_dir_all(folder).await.at(folder)?;
    let mut tasks = Vec::new();
    let mut chosen = Vec::new();
    for (version, title) in items {
        let file = version.primary_file().ok_or_else(|| {
            CoreError::InvalidData(format!("「{title}」的版本 {} 没有可下载的文件", version.version_number))
        })?;
        if file.filename.contains(['/', '\\']) || file.filename.starts_with('.') {
            return Err(CoreError::InvalidData(format!("文件名不合法：{}", file.filename)));
        }
        tasks.push(
            DownloadTask::new(&file.url, folder.join(&file.filename))
                .sha1(file.sha1.clone())
                .size(Some(file.size)),
        );
        chosen.push((version, title, file.clone()));
    }
    reporter.stage("下载文件");
    downloader.download_all(tasks, reporter.progress.clone(), cancel).await?;

    let mut index = ContentIndex::load(folder);
    index.prune(folder);
    let mut installed = Vec::new();
    for (version, title, file) in chosen {
        // 删除同一项目的旧版本文件
        let old: Vec<String> = index
            .files
            .iter()
            .filter(|(name, e)| {
                e.platform == version.platform && e.project_id == version.project_id && **name != file.filename
            })
            .map(|(name, _)| name.clone())
            .collect();
        for name in old {
            let _ = std::fs::remove_file(folder.join(&name));
            let _ = std::fs::remove_file(folder.join(format!("{name}.disabled")));
            index.files.remove(&name);
        }
        index.files.insert(
            file.filename.clone(),
            IndexEntry {
                platform: version.platform,
                project_id: version.project_id.clone(),
                version_id: version.id.clone(),
                title: title.clone(),
                version_number: version.version_number.clone(),
                sha1: file.sha1.clone(),
            },
        );
        installed.push(file.filename);
    }
    index.save(folder)?;
    Ok(installed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_roundtrip_and_prune() {
        let dir = tempfile::tempdir().unwrap();
        let mut idx = ContentIndex::default();
        let entry = IndexEntry {
            platform: Platform::Modrinth,
            project_id: "p".into(),
            version_id: "v".into(),
            title: "T".into(),
            version_number: "1".into(),
            sha1: None,
        };
        idx.files.insert("a.jar".into(), entry.clone());
        idx.files.insert("b.jar".into(), entry);
        std::fs::write(dir.path().join("a.jar.disabled"), b"x").unwrap();
        idx.save(dir.path()).unwrap();
        let mut loaded = ContentIndex::load(dir.path());
        assert_eq!(loaded.files.len(), 2);
        loaded.prune(dir.path());
        assert_eq!(loaded.files.len(), 1);
        assert!(loaded.files.contains_key("a.jar"));
        assert_eq!(base_name("x.jar.disabled"), "x.jar");
    }
}
