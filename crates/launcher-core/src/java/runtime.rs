//! 从 Mojang Java runtime 清单自动下载 Java。目录布局与官方启动器一致，可互相复用。

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

use crate::download::{CancelToken, DownloadTask, Downloader};
use crate::error::{CoreError, IoContext, Result};
use crate::install::Reporter;
use crate::paths::GameDir;
use crate::version::Environment;

pub const RUNTIME_LIST_URL: &str =
    "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

/// 平台 → 组件 → 可用版本
type RuntimeList = HashMap<String, HashMap<String, Vec<RuntimeEntry>>>;

#[derive(Debug, Deserialize)]
struct RuntimeEntry {
    manifest: ManifestRef,
    version: RuntimeVersion,
}

#[derive(Debug, Deserialize)]
struct ManifestRef {
    sha1: String,
    size: u64,
    url: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeVersion {
    name: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeManifest {
    files: HashMap<String, RuntimeFile>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum RuntimeFile {
    File {
        #[serde(default)]
        executable: bool,
        downloads: FileDownloads,
    },
    Directory,
    Link {
        #[cfg_attr(not(unix), allow(dead_code))]
        target: String,
    },
}

#[derive(Debug, Deserialize)]
struct FileDownloads {
    raw: RawDownload,
}

#[derive(Debug, Deserialize)]
struct RawDownload {
    sha1: String,
    size: u64,
    url: String,
}

/// 组件对应的 Java 主版本。
pub fn component_major(component: &str) -> u32 {
    match component {
        "jre-legacy" => 8,
        "java-runtime-alpha" => 16,
        "java-runtime-beta" | "java-runtime-gamma" | "java-runtime-gamma-snapshot" => 17,
        "java-runtime-delta" => 21,
        "java-runtime-epsilon" => 25,
        _ => 0,
    }
}

/// 当前系统在清单中的平台名，以及可回退的平台（通过系统转译运行）。
pub fn runtime_platform(env: &Environment) -> Vec<&'static str> {
    match (env.os_name.as_str(), env.arch.as_str()) {
        ("windows", "x86") => vec!["windows-x86"],
        ("windows", "arm64") => vec!["windows-arm64", "windows-x64"],
        ("windows", _) => vec!["windows-x64"],
        ("osx", "arm64") => vec!["mac-os-arm64", "mac-os"],
        ("osx", _) => vec!["mac-os"],
        ("linux", "x86") => vec!["linux-i386"],
        _ => vec!["linux"],
    }
}

fn runtime_dir(game: &GameDir, component: &str, platform: &str) -> PathBuf {
    game.runtime().join(component).join(platform).join(component)
}

fn java_in(dir: &std::path::Path, platform: &str) -> PathBuf {
    if platform.starts_with("windows") {
        dir.join("bin").join("java.exe")
    } else if platform.starts_with("mac") {
        dir.join("jre.bundle/Contents/Home/bin/java")
    } else {
        dir.join("bin").join("java")
    }
}

/// 已下载的 runtime 中 java 的路径（不存在时返回 `None`）。
pub fn runtime_java_path(game: &GameDir, component: &str, env: &Environment) -> Option<PathBuf> {
    runtime_platform(env).into_iter().find_map(|p| {
        let java = java_in(&runtime_dir(game, component, p), p);
        java.is_file().then_some(java)
    })
}

/// 下载指定组件，返回 java 可执行文件路径。组件在本平台不可用时，
/// 会退回到主版本更高的组件（仅对 Java 16+ 适用）或转译平台。
pub async fn install_runtime(
    downloader: &Downloader,
    game: &GameDir,
    component: &str,
    env: &Environment,
    reporter: &Reporter,
    cancel: &CancelToken,
) -> Result<PathBuf> {
    reporter.stage("获取 Java 列表");
    let list: RuntimeList = downloader
        .http()
        .get_json(RUNTIME_LIST_URL, "Java 运行时清单")
        .await?;

    let required = component_major(component);
    let (platform, comp, entry) = choose(&list, component, required, env).ok_or_else(|| {
        CoreError::Java(format!(
            "Mojang 没有为当前系统（{} {}）提供 Java {required}",
            env.os_name, env.arch
        ))
    })?;

    let dir = runtime_dir(game, &comp, platform);
    let java = java_in(&dir, platform);
    let version_file = game.runtime().join(&comp).join(platform).join(".version");
    if java.is_file()
        && std::fs::read_to_string(&version_file).is_ok_and(|v| v.trim() == entry.version.name)
    {
        return Ok(java);
    }

    reporter.stage(&format!("下载 Java {}", entry.version.name));
    let manifest: RuntimeManifest = {
        let bytes = downloader.http().get_bytes(&entry.manifest.url).await?;
        let actual = crate::download::sha1_bytes(&bytes);
        if !actual.eq_ignore_ascii_case(&entry.manifest.sha1) || bytes.len() as u64 != entry.manifest.size {
            return Err(CoreError::Checksum {
                path: PathBuf::from(&entry.manifest.url),
                expected: entry.manifest.sha1.clone(),
                actual,
            });
        }
        serde_json::from_slice(&bytes).map_err(|e| CoreError::json("Java 文件清单", e))?
    };

    let mut tasks = Vec::new();
    let mut executables = Vec::new();
    let mut links = Vec::new();
    for (rel, file) in &manifest.files {
        if rel.split('/').any(|p| p == "..") {
            continue;
        }
        let path = dir.join(rel);
        match file {
            RuntimeFile::Directory => {
                tokio::fs::create_dir_all(&path).await.at(&path)?;
            }
            RuntimeFile::File { executable, downloads } => {
                if *executable {
                    executables.push(path.clone());
                }
                tasks.push(
                    DownloadTask::new(&downloads.raw.url, path)
                        .sha1(Some(downloads.raw.sha1.clone()))
                        .size(Some(downloads.raw.size)),
                );
            }
            RuntimeFile::Link { .. } => links.push((rel.clone(), file)),
        }
    }
    downloader
        .download_all(tasks, reporter.progress.clone(), cancel)
        .await?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for p in &executables {
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).at(p)?;
        }
        for (rel, file) in links {
            if let RuntimeFile::Link { target } = file {
                let link = dir.join(&rel);
                if link.symlink_metadata().is_err() {
                    if let Some(parent) = link.parent() {
                        std::fs::create_dir_all(parent).at(parent)?;
                    }
                    std::os::unix::fs::symlink(target, &link).at(&link)?;
                }
            }
        }
    }
    #[cfg(not(unix))]
    let _ = (executables, links);

    if !java.is_file() {
        return Err(CoreError::Java(format!(
            "Java 下载完成，但找不到 {}",
            java.display()
        )));
    }
    std::fs::write(&version_file, &entry.version.name).at(&version_file)?;
    Ok(java)
}

fn choose<'a>(
    list: &'a RuntimeList,
    component: &str,
    required: u32,
    env: &Environment,
) -> Option<(&'static str, String, &'a RuntimeEntry)> {
    // 按平台优先级：先找首选组件，Java 16+ 再找主版本更高的组件（向上兼容），
    // 都没有时才使用转译平台
    for p in runtime_platform(env) {
        let Some(comps) = list.get(p) else { continue };
        if let Some(e) = comps.get(component).and_then(|v| v.first()) {
            return Some((p, component.to_string(), e));
        }
        if required >= 16 {
            let best = comps
                .iter()
                .filter(|(name, v)| !v.is_empty() && component_major(name) >= required && !name.contains("snapshot"))
                .min_by_key(|(name, _)| component_major(name));
            if let Some((name, v)) = best {
                return Some((p, name.clone(), &v[0]));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(os: &str, arch: &str) -> Environment {
        Environment {
            os_name: os.into(),
            os_version: String::new(),
            arch: arch.into(),
            features: HashMap::new(),
        }
    }

    const LIST: &str = r#"{
        "mac-os": {"jre-legacy": [{"manifest": {"sha1": "a", "size": 1, "url": "u1"}, "version": {"name": "8u74"}}],
                   "java-runtime-alpha": [{"manifest": {"sha1": "a", "size": 1, "url": "u2"}, "version": {"name": "16"}}]},
        "mac-os-arm64": {"jre-legacy": [], "java-runtime-alpha": [],
                   "java-runtime-gamma": [{"manifest": {"sha1": "a", "size": 1, "url": "u3"}, "version": {"name": "17"}}],
                   "java-runtime-delta": [{"manifest": {"sha1": "a", "size": 1, "url": "u4"}, "version": {"name": "21"}}]}
    }"#;

    #[test]
    fn choose_with_fallbacks() {
        let list: RuntimeList = serde_json::from_str(LIST).unwrap();
        // arm64 Mac 没有 Java 8，回退到 Intel 版（Rosetta）
        let (p, c, _) = choose(&list, "jre-legacy", 8, &env("osx", "arm64")).unwrap();
        assert_eq!((p, c.as_str()), ("mac-os", "jre-legacy"));
        // 没有 Java 16 时用最接近的更高版本
        let (p, c, _) = choose(&list, "java-runtime-alpha", 16, &env("osx", "arm64")).unwrap();
        assert_eq!((p, c.as_str()), ("mac-os-arm64", "java-runtime-gamma"));
        assert!(choose(&list, "jre-legacy", 8, &env("linux", "x86_64")).is_none());
    }

    #[test]
    fn layout_matches_official_launcher() {
        let g = GameDir::new("/mc");
        assert_eq!(
            java_in(&runtime_dir(&g, "java-runtime-gamma", "windows-x64"), "windows-x64"),
            PathBuf::from("/mc/runtime/java-runtime-gamma/windows-x64/java-runtime-gamma/bin/java.exe")
        );
    }

    #[test]
    fn parse_manifest() {
        let m: RuntimeManifest = serde_json::from_str(r#"{"files": {
            "bin": {"type": "directory"},
            "bin/java": {"type": "file", "executable": true, "downloads": {"raw": {"sha1": "s", "size": 1, "url": "u"}}},
            "lib/x": {"type": "link", "target": "../y"}
        }}"#).unwrap();
        assert_eq!(m.files.len(), 3);
        assert!(matches!(m.files["bin/java"], RuntimeFile::File { executable: true, .. }));
    }
}
