//! 已安装 Mod 管理：读取 jar 中的元数据，启用 / 禁用、删除、检查更新。
//!
//! 支持 `fabric.mod.json`、`quilt.mod.json`、`META-INF/mods.toml`、`META-INF/neoforge.mods.toml`
//! 与旧版 Forge 的 `mcmod.info`。

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use super::index::{base_name, ContentIndex, IndexEntry};
use crate::error::{CoreError, IoContext, Result};

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LocalMod {
    /// 磁盘上的文件名（禁用时带 .disabled）
    pub filename: String,
    pub enabled: bool,
    pub mod_id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<String>,
    /// 图标（data URL）
    pub icon: Option<String>,
    /// fabric / quilt / forge / neoforge；无法识别时为空
    pub loader: String,
    pub size: u64,
    /// 通过启动器安装的记录（平台、项目）
    pub source: Option<IndexEntry>,
    /// 中文名（来自 Mod 中文名数据库）
    #[serde(default)]
    pub zh_name: Option<String>,
}

const MAX_ICON: u64 = 256 * 1024;

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str, limit: u64) -> Option<Vec<u8>> {
    let mut e = zip.by_name(name).ok()?;
    if e.size() > limit {
        return None;
    }
    let mut buf = Vec::with_capacity(e.size() as usize);
    e.read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn icon_data_url(zip: &mut zip::ZipArchive<std::fs::File>, path: &str) -> Option<String> {
    use base64::Engine;
    let path = path.trim_start_matches('/');
    let data = read_entry(zip, path, MAX_ICON)?;
    let mime = if path.ends_with(".jpg") || path.ends_with(".jpeg") { "image/jpeg" } else { "image/png" };
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(data)
    ))
}

fn str_of(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// Fabric 的 authors 可以是字符串或 `{ "name": ... }`。
fn people(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|p| p.as_str().map(str::to_string).or_else(|| p.get("name").and_then(Value::as_str).map(str::to_string)))
                .collect()
        })
        .unwrap_or_default()
}

/// fabric.mod.json 的 icon 可以是字符串，也可以是「尺寸 → 路径」的映射（取最大的）。
fn fabric_icon(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Object(m) => m
            .iter()
            .max_by_key(|(k, _)| k.parse::<u32>().unwrap_or(0))
            .and_then(|(_, p)| p.as_str().map(str::to_string)),
        _ => None,
    }
}

/// 解析单个 jar（阻塞调用）。
pub fn read_mod(path: &Path) -> Result<LocalMod> {
    let filename = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let mut m = LocalMod {
        enabled: !filename.ends_with(".disabled"),
        name: base_name(&filename).trim_end_matches(".jar").to_string(),
        filename,
        size,
        ..Default::default()
    };
    let file = std::fs::File::open(path).at(path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| CoreError::Archive {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;

    if let Some(data) = read_entry(&mut zip, "quilt.mod.json", 1 << 20) {
        if let Ok(v) = serde_json::from_slice::<Value>(&data) {
            let ql = v.get("quilt_loader").cloned().unwrap_or_default();
            let meta = ql.get("metadata").cloned().unwrap_or_default();
            m.loader = "quilt".into();
            m.mod_id = str_of(&ql, "id");
            m.version = str_of(&ql, "version");
            m.name = Some(str_of(&meta, "name")).filter(|s| !s.is_empty()).unwrap_or(m.name);
            m.description = str_of(&meta, "description");
            m.authors = meta
                .get("contributors")
                .and_then(Value::as_object)
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default();
            if let Some(icon) = fabric_icon(meta.get("icon")) {
                m.icon = icon_data_url(&mut zip, &icon);
            }
            return Ok(m);
        }
    }
    if let Some(data) = read_entry(&mut zip, "fabric.mod.json", 1 << 20) {
        // 部分 Mod 的 JSON 带有控制字符，宽松处理
        let text = String::from_utf8_lossy(&data).replace(['\n', '\r', '\t'], " ");
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            m.loader = "fabric".into();
            m.mod_id = str_of(&v, "id");
            m.version = str_of(&v, "version");
            m.name = Some(str_of(&v, "name")).filter(|s| !s.is_empty()).unwrap_or(m.name);
            m.description = str_of(&v, "description");
            m.authors = people(v.get("authors"));
            if let Some(icon) = fabric_icon(v.get("icon")) {
                m.icon = icon_data_url(&mut zip, &icon);
            }
            return Ok(m);
        }
    }
    for (file, loader) in [("META-INF/neoforge.mods.toml", "neoforge"), ("META-INF/mods.toml", "forge")] {
        let Some(data) = read_entry(&mut zip, file, 1 << 20) else { continue };
        let Ok(t) = toml::from_str::<toml::Table>(&String::from_utf8_lossy(&data)) else { continue };
        let first = t
            .get("mods")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_table())
            .cloned()
            .unwrap_or_default();
        let get = |k: &str| first.get(k).and_then(|v| v.as_str()).unwrap_or_default().trim().to_string();
        m.loader = loader.into();
        m.mod_id = get("modId");
        m.version = get("version");
        if m.version.contains("${") {
            // ${file.jarVersion}：版本写在 MANIFEST.MF 中
            m.version = read_entry(&mut zip, "META-INF/MANIFEST.MF", 1 << 20)
                .and_then(|d| {
                    String::from_utf8_lossy(&d)
                        .lines()
                        .find_map(|l| l.strip_prefix("Implementation-Version:").map(|v| v.trim().to_string()))
                })
                .unwrap_or_default();
        }
        let display = get("displayName");
        if !display.is_empty() {
            m.name = display;
        }
        m.description = get("description");
        let authors = first
            .get("authors")
            .or_else(|| t.get("authors"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        m.authors = authors.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        let logo = first
            .get("logoFile")
            .or_else(|| t.get("logoFile"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if let Some(logo) = logo {
            m.icon = icon_data_url(&mut zip, &logo);
        }
        return Ok(m);
    }
    if let Some(data) = read_entry(&mut zip, "mcmod.info", 1 << 20) {
        let v: Value = serde_json::from_slice(&data).unwrap_or_default();
        // 可能是数组，也可能是 { "modList": [...] }
        let first = v
            .as_array()
            .and_then(|a| a.first().cloned())
            .or_else(|| v.get("modList").and_then(Value::as_array).and_then(|a| a.first().cloned()));
        if let Some(info) = first {
            m.loader = "forge".into();
            m.mod_id = str_of(&info, "modid");
            m.version = str_of(&info, "version");
            m.name = Some(str_of(&info, "name")).filter(|s| !s.is_empty()).unwrap_or(m.name);
            m.description = str_of(&info, "description");
            m.authors = people(info.get("authorList"));
            let logo = str_of(&info, "logoFile");
            if !logo.is_empty() {
                m.icon = icon_data_url(&mut zip, &logo);
            }
        }
    }
    Ok(m)
}

fn is_mod_file(name: &str) -> bool {
    let n = base_name(name);
    n.ends_with(".jar") || n.ends_with(".zip")
}

/// 列出文件夹中的全部 Mod（阻塞调用，多线程解析）。
pub fn scan_mods(folder: &Path) -> Vec<LocalMod> {
    let mut index = ContentIndex::load(folder);
    index.prune(folder);
    let files: Vec<std::path::PathBuf> = std::fs::read_dir(folder)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .map(|e| e.path())
                .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(is_mod_file))
                .collect()
        })
        .unwrap_or_default();
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
    let chunk = files.len().div_ceil(threads).max(1);
    let mut mods: Vec<LocalMod> = std::thread::scope(|s| {
        let handles: Vec<_> = files
            .chunks(chunk)
            .map(|c| {
                s.spawn(move || {
                    c.iter()
                        .map(|p| {
                            read_mod(p).unwrap_or_else(|_| LocalMod {
                                filename: p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                                name: p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                                enabled: !p.to_string_lossy().ends_with(".disabled"),
                                ..Default::default()
                            })
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    });
    for m in &mut mods {
        m.source = index.files.get(base_name(&m.filename)).cloned();
        m.zh_name = super::zhdb::db().zh_name(&m.name, Some(&m.mod_id)).map(str::to_string);
    }
    mods.sort_by_key(|m| m.name.to_lowercase());
    mods
}

/// 启用或禁用：在文件名后加上 / 去掉 `.disabled`。返回新的文件名。
pub fn set_enabled(folder: &Path, filename: &str, enabled: bool) -> Result<String> {
    check_name(filename)?;
    let base = base_name(filename).to_string();
    let target = if enabled { base.clone() } else { format!("{base}.disabled") };
    if target == filename {
        return Ok(target);
    }
    let from = folder.join(filename);
    let to = folder.join(&target);
    std::fs::rename(&from, &to).at(&from)?;
    Ok(target)
}

/// 删除 Mod 文件及其安装记录。
pub fn delete(folder: &Path, filename: &str) -> Result<()> {
    check_name(filename)?;
    let path = folder.join(filename);
    std::fs::remove_file(&path).at(&path)?;
    let mut index = ContentIndex::load(folder);
    if index.files.remove(base_name(filename)).is_some() {
        index.save(folder)?;
    }
    Ok(())
}

fn check_name(name: &str) -> Result<()> {
    if name.is_empty() || name.contains(['/', '\\']) || name == ".." || name.starts_with('.') {
        return Err(CoreError::InvalidData(format!("文件名不合法：{name}")));
    }
    Ok(())
}

/// 计算文件夹中每个 Mod 的 SHA1（阻塞调用）：文件名 → SHA1。
pub fn hash_mods(folder: &Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Ok(rd) = std::fs::read_dir(folder) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if is_mod_file(&name) {
                if let Ok(h) = crate::download::sha1_file(&e.path()) {
                    out.insert(name, h);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn jar(dir: &Path, name: &str, files: &[(&str, &[u8])]) -> std::path::PathBuf {
        let p = dir.join(name);
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
        for (n, d) in files {
            zip.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(d).unwrap();
        }
        zip.finish().unwrap();
        p
    }

    #[test]
    fn parse_all_formats() {
        let dir = tempfile::tempdir().unwrap();
        let png = b"\x89PNG fake";
        let fabric = jar(dir.path(), "sodium.jar", &[
            ("fabric.mod.json", br#"{"schemaVersion":1,"id":"sodium","version":"0.5.13","name":"Sodium","description":"fast","authors":["jellysquid3",{"name":"IMS"}],"icon":"assets/sodium/icon.png"}"#),
            ("assets/sodium/icon.png", png),
        ]);
        let m = read_mod(&fabric).unwrap();
        assert_eq!((m.loader.as_str(), m.mod_id.as_str(), m.name.as_str(), m.version.as_str()), ("fabric", "sodium", "Sodium", "0.5.13"));
        assert_eq!(m.authors, vec!["jellysquid3", "IMS"]);
        assert!(m.icon.unwrap().starts_with("data:image/png;base64,"));
        assert!(m.enabled);

        let forge = jar(dir.path(), "jei.jar.disabled", &[
            ("META-INF/mods.toml", b"modLoader=\"javafml\"\nlogoFile=\"logo.png\"\n[[mods]]\nmodId=\"jei\"\nversion=\"${file.jarVersion}\"\ndisplayName=\"Just Enough Items\"\nauthors=\"mezz, Ranger\"\ndescription='''\nView items\n'''\n"),
            ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\r\nImplementation-Version: 15.2.0.27\r\n"),
            ("logo.png", png),
        ]);
        let m = read_mod(&forge).unwrap();
        assert_eq!((m.loader.as_str(), m.mod_id.as_str(), m.name.as_str(), m.version.as_str()), ("forge", "jei", "Just Enough Items", "15.2.0.27"));
        assert_eq!(m.authors, vec!["mezz", "Ranger"]);
        assert_eq!(m.description, "View items");
        assert!(!m.enabled);
        assert!(m.icon.is_some());

        let neo = jar(dir.path(), "neo.jar", &[(
            "META-INF/neoforge.mods.toml",
            b"[[mods]]\nmodId=\"neomod\"\nversion=\"1.0\"\ndisplayName=\"Neo Mod\"\n",
        )]);
        assert_eq!(read_mod(&neo).unwrap().loader, "neoforge");

        let quilt = jar(dir.path(), "q.jar", &[(
            "quilt.mod.json",
            br#"{"quilt_loader":{"id":"qmod","version":"2.0","metadata":{"name":"Q Mod","description":"d","contributors":{"Alice":"Owner"}}}}"#,
        )]);
        let m = read_mod(&quilt).unwrap();
        assert_eq!((m.loader.as_str(), m.name.as_str(), m.authors[0].as_str()), ("quilt", "Q Mod", "Alice"));

        let legacy = jar(dir.path(), "old.jar", &[(
            "mcmod.info",
            br#"[{"modid":"oldmod","name":"Old Mod","version":"1.7.10-1","authorList":["a"]}]"#,
        )]);
        assert_eq!(read_mod(&legacy).unwrap().name, "Old Mod");

        let unknown = jar(dir.path(), "plain.jar", &[("a.txt", b"x")]);
        let m = read_mod(&unknown).unwrap();
        assert_eq!(m.name, "plain");
        assert!(m.loader.is_empty());

        let list = scan_mods(dir.path());
        assert_eq!(list.len(), 6);
        assert_eq!(hash_mods(dir.path()).len(), 6);
    }

    #[test]
    fn enable_disable_delete() {
        let dir = tempfile::tempdir().unwrap();
        jar(dir.path(), "a.jar", &[("x", b"x")]);
        let n = set_enabled(dir.path(), "a.jar", false).unwrap();
        assert_eq!(n, "a.jar.disabled");
        assert!(dir.path().join("a.jar.disabled").exists());
        let n = set_enabled(dir.path(), &n, true).unwrap();
        assert_eq!(n, "a.jar");
        delete(dir.path(), "a.jar").unwrap();
        assert!(!dir.path().join("a.jar").exists());
        assert!(set_enabled(dir.path(), "../x.jar", true).is_err());
    }
}
