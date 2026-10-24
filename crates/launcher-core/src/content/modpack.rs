//! 整合包：Modrinth（.mrpack）与 CurseForge（manifest.json）。
//!
//! 安装流程：解析整合包 → 安装对应的游戏版本与加载器 → 下载包内文件 → 解压 overrides。

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::CurseForge;
use crate::download::{CancelToken, DownloadTask, Downloader};
use crate::error::{CoreError, IoContext, Result};
use crate::install::Reporter;
use crate::instance::LoaderKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PackFormat {
    Modrinth,
    Curseforge,
}

/// 整合包要求的加载器（版本号为整合包中写的原始值）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackLoader {
    pub kind: LoaderKind,
    pub version: String,
}

/// 整合包中需要下载的文件。
#[derive(Debug, Clone, PartialEq)]
enum PackFile {
    /// Modrinth：相对游戏目录的路径与下载地址
    Direct {
        path: String,
        urls: Vec<String>,
        sha1: Option<String>,
        size: u64,
    },
    /// CurseForge：项目与文件 id，安装时再查询下载地址
    Curse { project: u64, file: u64 },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModpackInfo {
    pub format: PackFormat,
    pub name: String,
    pub version: String,
    pub summary: String,
    pub mc_version: String,
    pub loader: Option<PackLoader>,
    pub file_count: usize,
    #[serde(skip)]
    files: Vec<PackFile>,
    /// 要解压到游戏目录的 overrides 目录（按顺序，后面的覆盖前面的）
    #[serde(skip)]
    overrides: Vec<String>,
}

fn read_zip_entry(zip: &mut zip::ZipArchive<std::fs::File>, path: &Path, name: &str) -> Option<Result<Vec<u8>>> {
    let mut e = zip.by_name(name).ok()?;
    let mut buf = Vec::new();
    Some(
        e.read_to_end(&mut buf)
            .map(|_| buf)
            .map_err(|e| CoreError::Archive {
                path: path.to_path_buf(),
                reason: e.to_string(),
            }),
    )
}

/// 读取整合包信息（阻塞调用）。
pub fn read_modpack(path: &Path) -> Result<ModpackInfo> {
    let file = std::fs::File::open(path).at(path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| CoreError::Archive {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    if let Some(data) = read_zip_entry(&mut zip, path, "modrinth.index.json") {
        let v: Value = serde_json::from_slice(&data?).map_err(|e| CoreError::json("整合包信息", e))?;
        return parse_mrpack(&v);
    }
    if let Some(data) = read_zip_entry(&mut zip, path, "manifest.json") {
        let v: Value = serde_json::from_slice(&data?).map_err(|e| CoreError::json("整合包信息", e))?;
        return parse_curseforge(&v);
    }
    Err(CoreError::InvalidData(
        "无法识别的整合包：缺少 modrinth.index.json 或 manifest.json。目前支持 Modrinth（.mrpack）与 CurseForge 整合包".into(),
    ))
}

fn parse_mrpack(v: &Value) -> Result<ModpackInfo> {
    let deps = v.get("dependencies").and_then(Value::as_object).cloned().unwrap_or_default();
    let mc = deps
        .get("minecraft")
        .and_then(Value::as_str)
        .ok_or_else(|| CoreError::InvalidData("整合包没有指定 Minecraft 版本".into()))?
        .to_string();
    let loader = [
        ("fabric-loader", LoaderKind::Fabric),
        ("quilt-loader", LoaderKind::Quilt),
        ("forge", LoaderKind::Forge),
        ("neoforge", LoaderKind::NeoForge),
    ]
    .iter()
    .find_map(|(key, kind)| {
        deps.get(*key).and_then(Value::as_str).map(|ver| PackLoader {
            kind: *kind,
            version: ver.to_string(),
        })
    });
    let mut files = Vec::new();
    for f in v.get("files").and_then(Value::as_array).cloned().unwrap_or_default() {
        // 客户端不支持的文件（仅服务端）跳过
        if f.pointer("/env/client").and_then(Value::as_str) == Some("unsupported") {
            continue;
        }
        let path = f.get("path").and_then(Value::as_str).unwrap_or_default().to_string();
        if !safe_relative(&path) {
            return Err(CoreError::InvalidData(format!("整合包中的文件路径不安全：{path}")));
        }
        files.push(PackFile::Direct {
            path,
            urls: f
                .get("downloads")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|u| u.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
            sha1: f.pointer("/hashes/sha1").and_then(Value::as_str).map(str::to_string),
            size: f.get("fileSize").and_then(Value::as_u64).unwrap_or(0),
        });
    }
    Ok(ModpackInfo {
        format: PackFormat::Modrinth,
        name: v.get("name").and_then(Value::as_str).unwrap_or("整合包").to_string(),
        version: v.get("versionId").and_then(Value::as_str).unwrap_or_default().to_string(),
        summary: v.get("summary").and_then(Value::as_str).unwrap_or_default().to_string(),
        mc_version: mc,
        loader,
        file_count: files.len(),
        files,
        overrides: vec!["overrides/".into(), "client-overrides/".into()],
    })
}

fn parse_curseforge(v: &Value) -> Result<ModpackInfo> {
    let mc = v
        .pointer("/minecraft/version")
        .and_then(Value::as_str)
        .ok_or_else(|| CoreError::InvalidData("整合包没有指定 Minecraft 版本".into()))?
        .to_string();
    let loaders = v.pointer("/minecraft/modLoaders").and_then(Value::as_array).cloned().unwrap_or_default();
    let primary = loaders
        .iter()
        .find(|l| l.get("primary").and_then(Value::as_bool) == Some(true))
        .or(loaders.first())
        .and_then(|l| l.get("id").and_then(Value::as_str));
    let loader = primary.and_then(|id| {
        let (name, ver) = id.split_once('-')?;
        let kind = match name {
            "forge" => LoaderKind::Forge,
            "fabric" => LoaderKind::Fabric,
            "quilt" => LoaderKind::Quilt,
            "neoforge" => LoaderKind::NeoForge,
            _ => return None,
        };
        Some(PackLoader {
            kind,
            version: ver.to_string(),
        })
    });
    let files: Vec<PackFile> = v
        .get("files")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|f| f.get("required").and_then(Value::as_bool) != Some(false))
        .filter_map(|f| {
            Some(PackFile::Curse {
                project: f.get("projectID")?.as_u64()?,
                file: f.get("fileID")?.as_u64()?,
            })
        })
        .collect();
    let overrides = v.get("overrides").and_then(Value::as_str).unwrap_or("overrides");
    Ok(ModpackInfo {
        format: PackFormat::Curseforge,
        name: v.get("name").and_then(Value::as_str).unwrap_or("整合包").to_string(),
        version: v.get("version").and_then(Value::as_str).unwrap_or_default().to_string(),
        summary: v.get("author").and_then(Value::as_str).map(|a| format!("作者：{a}")).unwrap_or_default(),
        mc_version: mc,
        loader,
        file_count: files.len(),
        files,
        overrides: vec![format!("{}/", overrides.trim_end_matches('/'))],
    })
}

fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.starts_with('\\')
        && !path.contains(':')
        && !path.split(['/', '\\']).any(|p| p == "..")
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfFile {
    id: u64,
    mod_id: u64,
    file_name: String,
    download_url: Option<String>,
    #[serde(default)]
    hashes: Vec<CfHash>,
    #[serde(default)]
    file_length: u64,
}

#[derive(Deserialize)]
struct CfHash {
    value: String,
    algo: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfMod {
    id: u64,
    #[serde(default)]
    class_id: Option<u32>,
}

/// 下载整合包中的文件到游戏目录（Modrinth 按路径，CurseForge 按项目类型放入对应文件夹）。
pub async fn download_pack_files(
    info: &ModpackInfo,
    game_dir: &Path,
    downloader: &Downloader,
    curseforge: &CurseForge,
    reporter: &Reporter,
    cancel: &CancelToken,
) -> Result<()> {
    let mut tasks = Vec::new();
    let mut curse = Vec::new();
    for f in &info.files {
        match f {
            PackFile::Direct { path, urls, sha1, size } => {
                let Some(url) = urls.first() else { continue };
                tasks.push(DownloadTask::new(url, game_dir.join(path)).sha1(sha1.clone()).size(Some(*size)));
            }
            PackFile::Curse { project, file } => curse.push((*project, *file)),
        }
    }

    if !curse.is_empty() {
        reporter.stage("查询 CurseForge 文件");
        let file_ids: Vec<u64> = curse.iter().map(|c| c.1).collect();
        let mod_ids: Vec<u64> = curse.iter().map(|c| c.0).collect();
        let mut files: Vec<CfFile> = Vec::new();
        for chunk in file_ids.chunks(500) {
            let v: Value = curseforge.post("/mods/files", &json!({ "fileIds": chunk })).await?;
            files.extend(serde_json::from_value::<Vec<CfFile>>(v["data"].clone()).map_err(|e| CoreError::json("CurseForge 文件信息", e))?);
        }
        let mut classes: HashMap<u64, u32> = HashMap::new();
        for chunk in mod_ids.chunks(500) {
            let v: Value = curseforge.post("/mods", &json!({ "modIds": chunk })).await?;
            let mods: Vec<CfMod> = serde_json::from_value(v["data"].clone()).map_err(|e| CoreError::json("CurseForge 项目信息", e))?;
            classes.extend(mods.into_iter().map(|m| (m.id, m.class_id.unwrap_or(6))));
        }
        if files.len() < file_ids.len() {
            log::warn!("CurseForge 返回的文件数 {} 少于整合包中的 {}", files.len(), file_ids.len());
        }
        for f in files {
            if f.file_name.contains(['/', '\\']) {
                continue;
            }
            let folder = match classes.get(&f.mod_id) {
                Some(12) => "resourcepacks",
                Some(6552) => "shaderpacks",
                _ => "mods",
            };
            let url = match &f.download_url {
                Some(u) if !u.is_empty() => u.clone(),
                _ => format!("https://edge.forgecdn.net/files/{}/{}/{}", f.id / 1000, f.id % 1000, f.file_name),
            };
            let sha1 = f.hashes.iter().find(|h| h.algo == 1).map(|h| h.value.clone());
            tasks.push(
                DownloadTask::new(url, game_dir.join(folder).join(&f.file_name))
                    .sha1(sha1)
                    .size(Some(f.file_length)),
            );
        }
    }

    reporter.stage(&format!("下载整合包文件（{} 个）", tasks.len()));
    downloader.download_all(tasks, reporter.progress.clone(), cancel).await
}

/// 解压 overrides 到游戏目录（阻塞调用）。
pub fn extract_overrides(pack: &Path, info: &ModpackInfo, game_dir: &Path) -> Result<usize> {
    let file = std::fs::File::open(pack).at(pack)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| CoreError::Archive {
        path: pack.to_path_buf(),
        reason: e.to_string(),
    })?;
    let mut count = 0;
    for prefix in &info.overrides {
        for i in 0..zip.len() {
            let mut e = zip.by_index(i).map_err(|e| CoreError::Archive {
                path: pack.to_path_buf(),
                reason: e.to_string(),
            })?;
            let name = e.name().to_string();
            let Some(rel) = name.strip_prefix(prefix.as_str()) else { continue };
            if rel.is_empty() || e.is_dir() || !safe_relative(rel) {
                continue;
            }
            let dest = game_dir.join(rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).at(parent)?;
            }
            let mut out = std::fs::File::create(&dest).at(&dest)?;
            std::io::copy(&mut e, &mut out).at(&dest)?;
            count += 1;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_modrinth_index() {
        let v: Value = serde_json::from_str(
            r#"{"formatVersion":1,"game":"minecraft","versionId":"1.0.0","name":"测试包","summary":"s",
                "files":[
                  {"path":"mods/a.jar","hashes":{"sha1":"aa","sha512":"x"},"env":{"client":"required","server":"required"},"downloads":["https://cdn.modrinth.com/a.jar"],"fileSize":10},
                  {"path":"mods/server-only.jar","hashes":{"sha1":"bb"},"env":{"client":"unsupported","server":"required"},"downloads":["https://x"],"fileSize":1}
                ],
                "dependencies":{"minecraft":"1.20.1","fabric-loader":"0.15.11"}}"#,
        )
        .unwrap();
        let info = parse_mrpack(&v).unwrap();
        assert_eq!(info.mc_version, "1.20.1");
        assert_eq!(info.loader, Some(PackLoader { kind: LoaderKind::Fabric, version: "0.15.11".into() }));
        assert_eq!(info.file_count, 1);
        assert_eq!(info.overrides, vec!["overrides/", "client-overrides/"]);

        let bad: Value = serde_json::from_str(
            r#"{"files":[{"path":"../evil.jar","downloads":["x"]}],"dependencies":{"minecraft":"1.20.1"}}"#,
        )
        .unwrap();
        assert!(parse_mrpack(&bad).is_err());
    }

    #[test]
    fn parse_curseforge_manifest() {
        let v: Value = serde_json::from_str(
            r#"{"minecraft":{"version":"1.20.1","modLoaders":[{"id":"forge-47.2.0","primary":true}]},
                "manifestType":"minecraftModpack","name":"包","version":"2.0","author":"me",
                "files":[{"projectID":328085,"fileID":1,"required":true},{"projectID":2,"fileID":2,"required":false}],
                "overrides":"overrides"}"#,
        )
        .unwrap();
        let info = parse_curseforge(&v).unwrap();
        assert_eq!(info.loader, Some(PackLoader { kind: LoaderKind::Forge, version: "47.2.0".into() }));
        assert_eq!(info.file_count, 1);
        assert_eq!(info.files[0], PackFile::Curse { project: 328085, file: 1 });
    }

    #[test]
    fn path_safety() {
        assert!(safe_relative("mods/a.jar"));
        assert!(!safe_relative("../a"));
        assert!(!safe_relative("/etc/passwd"));
        assert!(!safe_relative("C:/x"));
        assert!(!safe_relative(""));
    }

    #[test]
    fn read_and_extract() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("p.mrpack");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&pack).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("modrinth.index.json", opts).unwrap();
        zip.write_all(br#"{"name":"P","versionId":"1","files":[],"dependencies":{"minecraft":"1.20.1"}}"#).unwrap();
        zip.start_file("overrides/config/a.toml", opts).unwrap();
        zip.write_all(b"a=1").unwrap();
        zip.start_file("client-overrides/options.txt", opts).unwrap();
        zip.write_all(b"lang:zh_cn").unwrap();
        zip.start_file("overrides/../escape.txt", opts).unwrap();
        zip.write_all(b"x").unwrap();
        zip.finish().unwrap();

        let info = read_modpack(&pack).unwrap();
        assert_eq!(info.name, "P");
        assert!(info.loader.is_none());
        let game = dir.path().join("game");
        assert_eq!(extract_overrides(&pack, &info, &game).unwrap(), 2);
        assert_eq!(std::fs::read_to_string(game.join("config/a.toml")).unwrap(), "a=1");
        assert!(game.join("options.txt").exists());
        assert!(!dir.path().join("escape.txt").exists());
    }
}
