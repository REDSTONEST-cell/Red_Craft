//! Forge 与 NeoForge：从 maven 获取版本列表，下载安装器后安装。
//!
//! - 新版安装器（install_profile.json 带 `spec`）：先用启动器的下载器（支持镜像）预下载全部库，
//!   有客户端处理器时再用 Java 以无界面方式运行安装器（`--installClient` / `--install-client`）；
//! - 旧版安装器（1.12.2 及以下，带 `versionInfo`）：直接解析安装配置并提取 universal jar，不运行安装器。

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};

use super::LoaderVersion;
use crate::download::{CancelToken, DownloadTask, HttpClient};
use crate::error::{CoreError, IoContext, Result};
use crate::install::{Installer, Reporter};
use crate::instance::LoaderKind;
use crate::paths::{maven_path, GameDir};
use crate::version::{compare_versions, plan_libraries, Library};

const FORGE_MAVEN: &str = "https://maven.minecraftforge.net";
const NEO_MAVEN: &str = "https://maven.neoforged.net/releases";

fn parse_versions(xml: &str) -> Vec<String> {
    let re = regex_lite::Regex::new(r"<version>([^<]+)</version>").expect("固定的正则");
    re.captures_iter(xml).map(|c| c[1].trim().to_string()).collect()
}

/// NeoForge 版本号对应的 Minecraft 版本：
/// `21.1.65` → `1.21.1`，`20.2.3-beta` → `1.20.2`，`21.0.x` → `1.21`；
/// 2026 年起按年份命名：`26.1.0.5` → `26.1`，`26.1.2.3` → `26.1.2`。
pub fn neoforge_mc_version(v: &str) -> Option<String> {
    let base = v.split(['-', '+']).next()?;
    let parts: Vec<u32> = base.split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let (a, b) = (*parts.first()?, *parts.get(1)?);
    if a >= 25 {
        let c = parts.get(2).copied().unwrap_or(0);
        Some(if c == 0 { format!("{a}.{b}") } else { format!("{a}.{b}.{c}") })
    } else if b == 0 {
        Some(format!("1.{a}"))
    } else {
        Some(format!("1.{a}.{b}"))
    }
}

fn unstable(v: &str) -> bool {
    let l = v.to_ascii_lowercase();
    l.contains("beta") || l.contains("alpha") || l.contains("pre") || l.contains("rc")
}

pub async fn list(http: &HttpClient, kind: LoaderKind, mc: &str) -> Result<Vec<LoaderVersion>> {
    let mut out: Vec<LoaderVersion> = Vec::new();
    match kind {
        LoaderKind::Forge => {
            let xml = http
                .get_text(&format!("{FORGE_MAVEN}/net/minecraftforge/forge/maven-metadata.xml"))
                .await?;
            let prefix = format!("{mc}-");
            for v in parse_versions(&xml) {
                if let Some(rest) = v.strip_prefix(&prefix) {
                    // 旧版本形如 1.7.10-10.13.4.1614-1.7.10
                    let display = rest.strip_suffix(&format!("-{mc}")).unwrap_or(rest).to_string();
                    out.push(LoaderVersion {
                        stable: !unstable(&display),
                        display,
                        version: v,
                    });
                }
            }
        }
        _ => {
            if mc == "1.20.1" {
                // 1.20.1 的 NeoForge 沿用 forge 坐标
                let xml = http
                    .get_text(&format!("{NEO_MAVEN}/net/neoforged/forge/maven-metadata.xml"))
                    .await?;
                for v in parse_versions(&xml) {
                    if let Some(rest) = v.strip_prefix("1.20.1-") {
                        out.push(LoaderVersion {
                            stable: !unstable(rest),
                            display: rest.to_string(),
                            version: v,
                        });
                    }
                }
            } else {
                let xml = http
                    .get_text(&format!("{NEO_MAVEN}/net/neoforged/neoforge/maven-metadata.xml"))
                    .await?;
                for v in parse_versions(&xml) {
                    if neoforge_mc_version(&v).as_deref() == Some(mc) {
                        out.push(LoaderVersion {
                            stable: !unstable(&v),
                            display: v.clone(),
                            version: v,
                        });
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| compare_versions(&b.display, &a.display));
    Ok(out)
}

fn installer_url(kind: LoaderKind, version: &str) -> String {
    match kind {
        LoaderKind::Forge => format!("{FORGE_MAVEN}/net/minecraftforge/forge/{version}/forge-{version}-installer.jar"),
        _ if version.starts_with("1.20.1-") => {
            format!("{NEO_MAVEN}/net/neoforged/forge/{version}/forge-{version}-installer.jar")
        }
        _ => format!("{NEO_MAVEN}/net/neoforged/neoforge/{version}/neoforge-{version}-installer.jar"),
    }
}

fn zip_err(path: &Path, e: impl std::fmt::Display) -> CoreError {
    CoreError::Archive {
        path: path.to_path_buf(),
        reason: e.to_string(),
    }
}

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, jar: &Path, name: &str) -> Result<Vec<u8>> {
    let mut e = zip
        .by_name(name.trim_start_matches('/'))
        .map_err(|_| CoreError::InvalidData(format!("安装器中缺少 {name}，文件可能已损坏")))?;
    let mut buf = Vec::new();
    e.read_to_end(&mut buf).map_err(|e| zip_err(jar, e))?;
    Ok(buf)
}

/// 安装器中解析出的信息。
struct Parsed {
    /// 改写为实例名后的版本 JSON
    version_json: Value,
    /// 新版安装器：需要预下载的库
    libraries: Vec<Library>,
    /// 是否需要运行安装器（存在客户端处理器）
    run_installer: bool,
    /// 安装器自己会创建的版本 id
    installer_version_id: Option<String>,
}

/// 解析安装器（阻塞调用），并把安装器内置的文件提取到 libraries。
fn parse_installer(jar: &Path, game: &GameDir, mc: &str, name: &str) -> Result<Parsed> {
    let file = std::fs::File::open(jar).at(jar)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| zip_err(jar, e))?;
    let profile: Value = serde_json::from_slice(&read_entry(&mut zip, jar, "install_profile.json")?)
        .map_err(|e| CoreError::json("安装配置", e))?;

    if let Some(info) = profile.get("versionInfo") {
        // 旧版安装器
        let mut vj = info.clone();
        vj["id"] = Value::String(name.to_string());
        if vj.get("inheritsFrom").is_none() {
            vj["inheritsFrom"] = Value::String(mc.to_string());
        }
        if vj.get("jar").is_none() {
            vj["jar"] = Value::String(mc.to_string());
        }
        let install = profile
            .get("install")
            .ok_or_else(|| CoreError::InvalidData("安装配置缺少 install 字段".into()))?;
        let path = install.get("path").and_then(Value::as_str).unwrap_or_default();
        let file_path = install.get("filePath").and_then(Value::as_str).unwrap_or_default();
        if let (Some(rel), false) = (maven_path(path), file_path.is_empty()) {
            let data = read_entry(&mut zip, jar, file_path)?;
            let dest = game.library(&rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).at(parent)?;
            }
            std::fs::write(&dest, data).at(&dest)?;
        }
        return Ok(Parsed {
            version_json: vj,
            libraries: Vec::new(),
            run_installer: false,
            installer_version_id: None,
        });
    }

    // 新版安装器
    let json_name = profile.get("json").and_then(Value::as_str).unwrap_or("/version.json");
    let mut vj: Value =
        serde_json::from_slice(&read_entry(&mut zip, jar, json_name)?).map_err(|e| CoreError::json("版本 JSON", e))?;
    let installer_version_id = vj.get("id").and_then(Value::as_str).map(str::to_string);
    vj["id"] = Value::String(name.to_string());
    if vj.get("inheritsFrom").is_none() {
        vj["inheritsFrom"] = Value::String(mc.to_string());
    }

    // 安装器内置的 maven 文件（如 forge 本体）
    for i in 0..zip.len() {
        let mut e = zip.by_index(i).map_err(|e| zip_err(jar, e))?;
        let Some(rel) = e.name().strip_prefix("maven/").map(str::to_string) else {
            continue;
        };
        if e.is_dir() || rel.is_empty() || rel.split('/').any(|p| p == "..") {
            continue;
        }
        let dest = game.library(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).at(parent)?;
        }
        let mut buf = Vec::new();
        e.read_to_end(&mut buf).map_err(|e| zip_err(jar, e))?;
        std::fs::write(&dest, buf).at(&dest)?;
    }

    let mut libraries: Vec<Library> = serde_json::from_value(profile.get("libraries").cloned().unwrap_or(Value::Array(vec![])))
        .map_err(|e| CoreError::json("安装配置中的库列表", e))?;
    let version_libs: Vec<Library> =
        serde_json::from_value(vj.get("libraries").cloned().unwrap_or(Value::Array(vec![]))).unwrap_or_default();
    libraries.extend(version_libs);

    let run_installer = profile
        .get("processors")
        .and_then(Value::as_array)
        .is_some_and(|ps| {
            ps.iter().any(|p| match p.get("sides").and_then(Value::as_array) {
                Some(sides) => sides.iter().any(|s| s == "client"),
                None => true,
            })
        });
    Ok(Parsed {
        version_json: vj,
        libraries,
        run_installer,
        installer_version_id,
    })
}

/// 安装器要求游戏目录中存在 launcher_profiles.json。
fn ensure_launcher_profiles(game: &GameDir) -> Result<()> {
    let p = game.root().join("launcher_profiles.json");
    if !p.exists() {
        std::fs::create_dir_all(game.root()).at(game.root())?;
        std::fs::write(&p, r#"{"profiles":{},"selectedProfile":"(Default)","version":3}"#).at(&p)?;
    }
    Ok(())
}

/// 安装 Forge / NeoForge。调用前需保证原版 `mc` 已安装完整（安装器需要原版 jar）。
pub async fn install(
    installer: &Installer,
    choice: &super::LoaderChoice,
    mc: &str,
    name: &str,
    java: &Path,
    reporter: &Reporter,
    cancel: &CancelToken,
) -> Result<()> {
    let (kind, version) = (choice.kind, choice.version.as_str());
    let game = &installer.game;
    let jar = tmp_dir(game).join(format!("{}-{version}-installer.jar", kind.display_name().to_lowercase()));

    reporter.stage(&format!("下载 {} 安装器", kind.display_name()));
    installer
        .downloader
        .download_all(vec![DownloadTask::new(installer_url(kind, version), &jar)], reporter.progress.clone(), cancel)
        .await?;

    reporter.stage("解析安装器");
    let parsed = {
        let (jar, game, mc, name) = (jar.clone(), game.clone(), mc.to_string(), name.to_string());
        tokio::task::spawn_blocking(move || parse_installer(&jar, &game, &mc, &name))
            .await
            .map_err(|e| CoreError::InvalidData(e.to_string()))??
    };

    // 预下载库（走启动器的下载源与镜像），安装器发现文件已存在且校验通过就不会重复下载
    if !parsed.libraries.is_empty() {
        reporter.stage("下载加载器依赖库");
        let mut seen = HashSet::new();
        let tasks: Vec<DownloadTask> = plan_libraries(&parsed.libraries, &installer.env)
            .into_iter()
            .filter(|l| !l.url.is_empty() && seen.insert(l.path.clone()))
            .map(|l| DownloadTask::new(l.url, game.library(&l.path)).sha1(l.sha1).size(l.size))
            .collect();
        installer
            .downloader
            .download_all(tasks, reporter.progress.clone(), cancel)
            .await?;
    }

    let target_json = game.version_json(name);
    if parsed.run_installer {
        ensure_launcher_profiles(game)?;
        let orig_id = parsed.installer_version_id.clone().unwrap_or_default();
        let existed_before = !orig_id.is_empty() && game.version_json(&orig_id).exists();
        run_installer(java, &jar, game.root(), kind, reporter, cancel).await?;
        // 安装器以自己的版本名写入，这里改成实例名
        if !orig_id.is_empty() && orig_id != name {
            let src = game.version_json(&orig_id);
            if !src.exists() {
                return Err(CoreError::InvalidData(format!(
                    "{} 安装器运行完成，但没有生成版本 {orig_id}",
                    kind.display_name()
                )));
            }
            if !existed_before {
                let _ = std::fs::remove_dir_all(game.version_dir(&orig_id));
            }
        }
    }
    // 版本 JSON 以安装器内的为准写入实例目录
    if let Some(parent) = target_json.parent() {
        tokio::fs::create_dir_all(parent).await.at(parent)?;
    }
    let data = serde_json::to_vec_pretty(&parsed.version_json).map_err(|e| CoreError::json("版本 JSON", e))?;
    tokio::fs::write(&target_json, data).await.at(&target_json)?;
    let _ = tokio::fs::remove_file(&jar).await;
    Ok(())
}

async fn run_installer(
    java: &Path,
    jar: &Path,
    root: &Path,
    kind: LoaderKind,
    reporter: &Reporter,
    cancel: &CancelToken,
) -> Result<()> {
    let flag = match kind {
        LoaderKind::NeoForge => "--install-client",
        _ => "--installClient",
    };
    reporter.stage(&format!("运行 {} 安装器", kind.display_name()));
    let mut cmd = tokio::process::Command::new(java);
    cmd.arg("-jar")
        .arg(jar)
        .arg(flag)
        .arg(root)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let mut child = cmd
        .spawn()
        .map_err(|e| CoreError::Java(format!("无法运行 {}：{e}", java.display())))?;

    let tail = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let mut readers = Vec::new();
    for stream in [
        child.stdout.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let tail = tail.clone();
        let stage = reporter.stage.clone();
        let name = kind.display_name();
        readers.push(tokio::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            let mut last = std::time::Instant::now();
            while let Ok(Some(line)) = lines.next_line().await {
                log::debug!("[{name} 安装器] {line}");
                // 安装器输出很多，界面上每 200ms 更新一次
                let short: String = line.trim().chars().take(80).collect();
                if !short.is_empty() && last.elapsed() > std::time::Duration::from_millis(200) {
                    last = std::time::Instant::now();
                    stage(&format!("运行 {name} 安装器：{short}"));
                }
                if let Ok(mut t) = tail.lock() {
                    t.push(line);
                    if t.len() > 40 {
                        t.remove(0);
                    }
                }
            }
        }));
    }

    let status = loop {
        tokio::select! {
            s = child.wait() => break s.map_err(|e| CoreError::Java(e.to_string()))?,
            _ = tokio::time::sleep(std::time::Duration::from_millis(300)) => {
                if cancel.is_cancelled() {
                    let _ = child.kill().await;
                    return Err(CoreError::Cancelled);
                }
            }
        }
    };
    for r in readers {
        let _ = r.await;
    }
    if !status.success() {
        let tail = tail.lock().map(|t| t.join("\n")).unwrap_or_default();
        let hint_line = tail
            .lines()
            .rev()
            .find(|l| l.contains("Exception") || l.contains("Error") || l.contains("failed"))
            .unwrap_or("")
            .to_string();
        log::error!("{} 安装器输出：\n{tail}", kind.display_name());
        return Err(CoreError::InvalidData(format!(
            "{} 安装器运行失败（退出码 {}）{}",
            kind.display_name(),
            status.code().map(|c| c.to_string()).unwrap_or_else(|| "未知".into()),
            if hint_line.is_empty() { String::new() } else { format!("：{hint_line}") }
        )));
    }
    Ok(())
}

/// 临时目录（下载的安装器）。
pub fn tmp_dir(game: &GameDir) -> PathBuf {
    game.root().join(".redcraft-tmp")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neoforge_versions() {
        assert_eq!(neoforge_mc_version("21.1.65").as_deref(), Some("1.21.1"));
        assert_eq!(neoforge_mc_version("20.2.3-beta").as_deref(), Some("1.20.2"));
        assert_eq!(neoforge_mc_version("21.0.143").as_deref(), Some("1.21"));
        assert_eq!(neoforge_mc_version("26.1.0.5").as_deref(), Some("26.1"));
        assert_eq!(neoforge_mc_version("26.1.2.3").as_deref(), Some("26.1.2"));
        assert_eq!(neoforge_mc_version("x"), None);
    }

    #[test]
    fn maven_metadata() {
        let xml = "<metadata><versioning><versions><version>1.20.1-47.3.0</version>\n<version>1.7.10-10.13.4.1614-1.7.10</version></versions></versioning></metadata>";
        assert_eq!(parse_versions(xml), vec!["1.20.1-47.3.0", "1.7.10-10.13.4.1614-1.7.10"]);
    }

    #[test]
    fn installer_urls() {
        assert_eq!(
            installer_url(LoaderKind::Forge, "1.20.1-47.3.0"),
            "https://maven.minecraftforge.net/net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-installer.jar"
        );
        assert!(installer_url(LoaderKind::NeoForge, "1.20.1-47.1.106").contains("/net/neoforged/forge/"));
        assert!(installer_url(LoaderKind::NeoForge, "21.1.65").ends_with("neoforge-21.1.65-installer.jar"));
    }
}
