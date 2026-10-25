//! 导出整合包（Modrinth `.mrpack`）。
//!
//! 与 PCL 的导出类似：用户勾选要打包的文件夹与文件；能在 Modrinth 上找到的 Mod、资源包、光影包
//! 只写下载地址（整合包体积小），其余文件放进 `overrides/`。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;

use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha512};

use super::PackLoader;
use crate::download::CancelToken;
use crate::error::{CoreError, IoContext, Result};
use crate::instance::LoaderKind;

/// 不在导出列表中出现的条目：游戏本体、启动器文件与运行时生成的缓存。
const HIDDEN: &[&str] = &[
    "versions",
    "libraries",
    "assets",
    "runtime",
    "natives",
    "logs",
    "crash-reports",
    "debug",
    ".cache",
    ".fabric",
    ".mixin.out",
    ".redcraft-tmp",
    "redcraft",
    "redcraft.json",
    "launcher_profiles.json",
    "launcher_accounts.json",
    "usercache.json",
    "usernamecache.json",
    "PCL",
    "PCL.ini",
];

/// 默认勾选的条目（与 PCL 的默认选择接近：Mod、配置、资源包、光影与游戏设置，不含存档）。
const DEFAULT_ON: &[&str] = &[
    "mods",
    "config",
    "defaultconfigs",
    "kubejs",
    "scripts",
    "resourcepacks",
    "shaderpacks",
    "options.txt",
    "optionsof.txt",
    "optionsshaders.txt",
    "servers.dat",
];

/// 可以改为在线下载的文件夹：只看第一层的 jar / zip。
const REMOTE_DIRS: &[&str] = &["mods", "resourcepacks", "shaderpacks"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportEntry {
    pub name: String,
    pub is_dir: bool,
    /// 文件数量（文件夹内递归统计）
    pub files: usize,
    pub size: u64,
    /// 默认是否勾选
    pub default: bool,
}

/// 实例目录中可以导出的第一层条目。`instance_id` 用于隐藏版本自身的 jar / json。
pub fn export_entries(game_dir: &Path, instance_id: &str) -> Result<Vec<ExportEntry>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(game_dir).at(game_dir)?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if hidden(&name, instance_id) {
            continue;
        }
        let path = e.path();
        let is_dir = path.is_dir();
        let (files, size) = if is_dir {
            let mut list = Vec::new();
            walk(&path, "", &mut list);
            (list.len(), list.iter().map(|(_, s)| s).sum())
        } else {
            (1, e.metadata().map(|m| m.len()).unwrap_or(0))
        };
        out.push(ExportEntry {
            default: DEFAULT_ON.contains(&name.as_str()),
            name,
            is_dir,
            files,
            size,
        });
    }
    // 默认勾选的排前面，其余按名称
    out.sort_by(|a, b| {
        b.default
            .cmp(&a.default)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

fn hidden(name: &str, instance_id: &str) -> bool {
    HIDDEN.contains(&name)
        || name == format!("{instance_id}.jar")
        || name == format!("{instance_id}.json")
        || name.ends_with("-natives")
        || name.ends_with(".log")
        || name.ends_with(".tmp")
}

/// 递归列出文件（相对路径使用 `/`），跳过安装记录与已禁用的文件。
fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, u64)>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name == super::INDEX_FILE || name.ends_with(".disabled") {
            continue;
        }
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let path = e.path();
        if path.is_dir() {
            walk(&path, &rel, out);
        } else if let Ok(m) = e.metadata() {
            out.push((rel, m.len()));
        }
    }
}

/// 选中条目展开后的全部文件（相对路径）。
pub fn collect_files(game_dir: &Path, selected: &[String], instance_id: &str) -> Vec<String> {
    let mut out = Vec::new();
    for name in selected {
        if hidden(name, instance_id) || name.contains(['/', '\\']) || name == ".." {
            continue;
        }
        let path = game_dir.join(name);
        if path.is_dir() {
            walk(&path, name, &mut out);
        } else if path.is_file() {
            out.push((name.clone(), 0));
        }
    }
    out.into_iter().map(|(r, _)| r).collect()
}

/// 可以尝试改为在线下载的文件（mods / resourcepacks / shaderpacks 第一层的 jar、zip）。
pub fn is_remote_candidate(rel: &str) -> bool {
    let Some((dir, name)) = rel.split_once('/') else {
        return false;
    };
    REMOTE_DIRS.contains(&dir)
        && !name.contains('/')
        && (name.ends_with(".jar") || name.ends_with(".zip"))
}

/// 文件的 SHA1、SHA512 与大小（阻塞调用）。
pub fn hash_file(path: &Path) -> Result<(String, String, u64)> {
    let mut file = std::fs::File::open(path).at(path)?;
    let mut s1 = sha1::Sha1::new();
    let mut s512 = Sha512::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let n = file.read(&mut buf).at(path)?;
        if n == 0 {
            break;
        }
        s1.update(&buf[..n]);
        s512.update(&buf[..n]);
        size += n as u64;
    }
    Ok((
        hex::encode(s1.finalize()),
        hex::encode(s512.finalize()),
        size,
    ))
}

/// 在整合包中改为在线下载的文件。
#[derive(Debug, Clone)]
pub struct RemoteFile {
    pub path: String,
    pub url: String,
    pub sha1: String,
    pub sha512: String,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct PackMeta {
    pub name: String,
    pub version: String,
    pub summary: String,
    pub mc_version: String,
    pub loader: Option<PackLoader>,
}

fn loader_key(kind: LoaderKind) -> &'static str {
    match kind {
        LoaderKind::Fabric => "fabric-loader",
        LoaderKind::Quilt => "quilt-loader",
        LoaderKind::Forge => "forge",
        LoaderKind::NeoForge => "neoforge",
    }
}

/// Modrinth 整合包只允许这些域名的下载地址。
pub fn allowed_download(url: &str) -> bool {
    url.starts_with("https://cdn.modrinth.com/")
}

pub fn index_json(meta: &PackMeta, remote: &[RemoteFile]) -> Value {
    let mut deps = serde_json::Map::new();
    deps.insert("minecraft".into(), json!(meta.mc_version));
    if let Some(l) = &meta.loader {
        deps.insert(loader_key(l.kind).into(), json!(l.version));
    }
    let files: Vec<Value> = remote
        .iter()
        .map(|f| {
            json!({
                "path": f.path,
                "hashes": { "sha1": f.sha1, "sha512": f.sha512 },
                "env": { "client": "required", "server": if f.path.starts_with("mods/") { "required" } else { "unsupported" } },
                "downloads": [f.url],
                "fileSize": f.size,
            })
        })
        .collect();
    json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": meta.version,
        "name": meta.name,
        "summary": meta.summary,
        "files": files,
        "dependencies": deps,
    })
}

/// 写出 `.mrpack`（阻塞调用）：先写临时文件，完成后改名。`progress(done, total)` 按文件报告。
pub fn write_mrpack(
    game_dir: &Path,
    out: &Path,
    meta: &PackMeta,
    remote: &[RemoteFile],
    local: &[String],
    progress: &mut dyn FnMut(usize, usize),
    cancel: &CancelToken,
) -> Result<()> {
    let tmp = out.with_extension("mrpack.part");
    let result = (|| {
        let file = std::fs::File::create(&tmp).at(&tmp)?;
        let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let err = |e: zip::result::ZipError| CoreError::Archive {
            path: tmp.clone(),
            reason: e.to_string(),
        };
        zip.start_file("modrinth.index.json", opts).map_err(err)?;
        let index = serde_json::to_vec_pretty(&index_json(meta, remote))
            .map_err(|e| CoreError::json("整合包信息", e))?;
        zip.write_all(&index).at(&tmp)?;
        let mut buf = vec![0u8; 64 * 1024];
        for (i, rel) in local.iter().enumerate() {
            cancel.check()?;
            let src = game_dir.join(rel);
            let Ok(mut f) = std::fs::File::open(&src) else {
                continue;
            };
            zip.start_file(format!("overrides/{rel}"), opts)
                .map_err(err)?;
            loop {
                let n = f.read(&mut buf).at(&src)?;
                if n == 0 {
                    break;
                }
                zip.write_all(&buf[..n]).at(&tmp)?;
            }
            progress(i + 1, local.len());
        }
        zip.finish().map_err(err)?.flush().at(&tmp)?;
        Ok(())
    })();
    match result {
        Ok(()) => std::fs::rename(&tmp, out).at(out),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// 把文件按「能在线下载」与「打包进 overrides」分开。`found`：SHA1 → 下载地址。
pub fn split_files(
    hashed: Vec<(String, String, String, u64)>,
    found: &HashMap<String, String>,
) -> (Vec<RemoteFile>, Vec<String>) {
    let mut remote = Vec::new();
    let mut local = Vec::new();
    for (path, sha1, sha512, size) in hashed {
        match found.get(&sha1).filter(|u| allowed_download(u)) {
            Some(url) => remote.push(RemoteFile {
                path,
                url: url.clone(),
                sha1,
                sha512,
                size,
            }),
            None => local.push(path),
        }
    }
    (remote, local)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::read_modpack;

    #[test]
    fn export_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let g = dir.path().join("game");
        for (p, d) in [
            ("mods/a.jar", "aaa"),
            ("mods/b.jar.disabled", "bbb"),
            ("mods/.redcraft-index.json", "{}"),
            ("config/x/y.toml", "y=1"),
            ("saves/w/level.dat", "lvl"),
            ("options.txt", "lang:zh_cn"),
            ("inst.jar", "jar"),
            ("logs/latest.log", "log"),
        ] {
            let p = g.join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, d).unwrap();
        }
        let entries = export_entries(&g, "inst").unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(!names.contains(&"inst.jar") && !names.contains(&"logs"));
        assert!(entries.iter().find(|e| e.name == "mods").unwrap().default);
        assert!(!entries.iter().find(|e| e.name == "saves").unwrap().default);
        assert_eq!(entries.iter().find(|e| e.name == "mods").unwrap().files, 1);

        let mut files = collect_files(
            &g,
            &["mods".into(), "config".into(), "options.txt".into()],
            "inst",
        );
        files.sort();
        assert_eq!(files, vec!["config/x/y.toml", "mods/a.jar", "options.txt"]);
        assert!(is_remote_candidate("mods/a.jar") && !is_remote_candidate("config/x/y.toml"));

        let hashed: Vec<_> = files
            .iter()
            .map(|f| {
                let (s1, s5, n) = hash_file(&g.join(f)).unwrap();
                (f.clone(), s1, s5, n)
            })
            .collect();
        let a_sha1 = hashed
            .iter()
            .find(|h| h.0 == "mods/a.jar")
            .unwrap()
            .1
            .clone();
        let found = HashMap::from([(a_sha1, "https://cdn.modrinth.com/data/x/a.jar".to_string())]);
        let (remote, local) = split_files(hashed, &found);
        assert_eq!(remote.len(), 1);
        assert_eq!(local.len(), 2);

        let meta = PackMeta {
            name: "测试".into(),
            version: "1.0.0".into(),
            summary: String::new(),
            mc_version: "1.20.1".into(),
            loader: Some(PackLoader {
                kind: LoaderKind::Fabric,
                version: "0.16.0".into(),
            }),
        };
        let out = dir.path().join("out.mrpack");
        write_mrpack(
            &g,
            &out,
            &meta,
            &remote,
            &local,
            &mut |_, _| {},
            &CancelToken::new(),
        )
        .unwrap();
        let info = read_modpack(&out).unwrap();
        assert_eq!(info.name, "测试");
        assert_eq!(info.mc_version, "1.20.1");
        assert_eq!(info.loader.unwrap().kind, LoaderKind::Fabric);
        assert_eq!(info.file_count, 1);
        let target = dir.path().join("target");
        assert_eq!(extract_overrides_count(&out, &target), 2);
        assert_eq!(
            std::fs::read_to_string(target.join("config/x/y.toml")).unwrap(),
            "y=1"
        );
    }

    fn extract_overrides_count(pack: &Path, to: &Path) -> usize {
        let info = read_modpack(pack).unwrap();
        crate::content::extract_overrides(pack, &info, to).unwrap()
    }
}
