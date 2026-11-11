//! 扫描 `versions` 目录中的实例，并识别加载器。

use serde::Serialize;

use crate::paths::GameDir;
use crate::version::{resolve_version, ResolvedVersion};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoaderKind {
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}

impl LoaderKind {
    pub fn display_name(self) -> &'static str {
        match self {
            LoaderKind::Fabric => "Fabric",
            LoaderKind::Quilt => "Quilt",
            LoaderKind::Forge => "Forge",
            LoaderKind::NeoForge => "NeoForge",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderInfo {
    pub kind: LoaderKind,
    pub version: String,
}

/// 根据库列表与主类识别加载器。
///
/// 1.20.1 的 NeoForge 仍带有 `net.minecraftforge` 坐标的库，因此 NeoForge 的标志优先于 Forge。
pub fn detect_loader(resolved: &ResolvedVersion) -> Option<LoaderInfo> {
    let (mut fabric, mut quilt, mut neo, mut forge) = (None, None, None, None);
    for lib in &resolved.json.libraries {
        let Some(c) = lib.coord() else { continue };
        match (c.group, c.artifact) {
            ("net.fabricmc", "fabric-loader") => fabric = Some(c.version.to_string()),
            ("org.quiltmc", "quilt-loader") => quilt = Some(c.version.to_string()),
            ("net.neoforged", "neoforge") => neo = Some(c.version.to_string()),
            // 1.20.1 的 NeoForge 沿用 forge 坐标：net.neoforged:forge:1.20.1-47.1.106
            ("net.neoforged", "forge") => {
                neo = Some(c.version.split_once('-').map(|x| x.1).unwrap_or(c.version).to_string())
            }
            ("net.minecraftforge", "forge" | "fmlloader") if forge.is_none() => {
                // 形如 1.20.1-47.2.0 或 1.7.10-10.13.4.1614-1.7.10
                forge = Some(c.version.split('-').nth(1).unwrap_or(c.version).to_string())
            }
            _ => {}
        }
    }
    // 版本号以启动参数为准；1.20.1 的 NeoForge 只能通过 net.neoforged 开头的库识别
    let game_args: Vec<&str> = resolved
        .json
        .arguments
        .as_ref()
        .map(|a| {
            a.game
                .iter()
                .filter_map(|x| match x {
                    crate::version::Argument::Plain(s) => Some(s.as_str()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let arg = |key: &str| {
        game_args
            .iter()
            .position(|a| *a == key)
            .and_then(|i| game_args.get(i + 1))
            .map(|s| s.to_string())
    };
    let has_neo_libs = resolved
        .json
        .libraries
        .iter()
        .any(|l| l.name.starts_with("net.neoforged"));
    if has_neo_libs {
        if let Some(v) = arg("--fml.neoForgeVersion").or_else(|| arg("--fml.forgeVersion")) {
            neo = Some(v);
        } else if neo.is_none() {
            neo = forge.take().or(Some(String::new()));
        }
        forge = None;
    } else if forge.is_some() || resolved.json.main_class.contains("cpw.mods") {
        if let Some(v) = arg("--fml.forgeVersion") {
            forge = Some(v);
        }
    }
    let pick = |kind, version: Option<String>| version.map(|version| LoaderInfo { kind, version });
    if let Some(info) = pick(LoaderKind::NeoForge, neo)
        .or_else(|| pick(LoaderKind::Quilt, quilt))
        .or_else(|| pick(LoaderKind::Fabric, fabric))
        .or_else(|| pick(LoaderKind::Forge, forge))
    {
        return Some(info);
    }
    let main = resolved.json.main_class.as_str();
    if main.contains("fabricmc") {
        return Some(LoaderInfo { kind: LoaderKind::Fabric, version: String::new() });
    }
    if main.contains("quiltmc") {
        return Some(LoaderInfo { kind: LoaderKind::Quilt, version: String::new() });
    }
    if main.contains("cpw.mods") || resolved.json.minecraft_arguments.as_deref().is_some_and(|a| a.contains("FMLTweaker")) {
        return Some(LoaderInfo { kind: LoaderKind::Forge, version: String::new() });
    }
    None
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSummary {
    pub id: String,
    /// Minecraft 版本（优先读取元数据，回退到继承链最底层的 id）
    pub mc_version: String,
    /// `release` / `snapshot` / `old_beta` / `old_alpha`
    pub kind: String,
    pub loader: Option<LoaderInfo>,
    pub release_time: String,
    /// 版本文件无法解析时的原因
    pub error: Option<String>,
    /// 原版 + OptiFine 实例的 OptiFine 版本，例如 `HD_U_I6`
    pub optifine: Option<String>,
    /// 继承的父版本（由近到远），例如 Fabric 实例的原版
    #[serde(skip)]
    pub parents: Vec<String>,
}

impl InstanceSummary {
    pub fn is_release(&self) -> bool {
        self.kind == "release"
    }
}

/// 其他启动器允许重命名版本文件夹，此时版本名不再是游戏版本号。依次尝试：
/// `clientVersion`（PCL / 官方）、HMCL 的 `patches` 中的 game、jar 内的 `version.json`（1.14+）。
pub fn minecraft_version(game: &GameDir, id: &str) -> Option<String> {
    let mut cur = id.to_string();
    // 沿继承链找到最底层的版本
    for _ in 0..8 {
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(game.version_json(&cur)).ok()?).ok()?;
        if let Some(v) = json
            .get("clientVersion")
            .and_then(serde_json::Value::as_str)
            .filter(|v| !v.is_empty())
        {
            return Some(v.to_string());
        }
        if let Some(v) = json
            .get("patches")
            .and_then(serde_json::Value::as_array)
            .and_then(|p| {
                p.iter()
                    .find(|x| x.get("id").and_then(serde_json::Value::as_str) == Some("game"))
            })
            .and_then(|g| g.get("version").and_then(serde_json::Value::as_str))
        {
            return Some(v.to_string());
        }
        match json.get("inheritsFrom").and_then(serde_json::Value::as_str) {
            Some(parent) if game.version_json(parent).is_file() => cur = parent.to_string(),
            Some(parent) => return Some(parent.to_string()),
            None => break,
        }
    }
    jar_version(&game.version_jar(&cur))
}

/// 客户端 jar 内 `version.json`（1.14+）记录的游戏版本。
///
/// 每次列出实例都要读取，而打开一个几十 MB 的 jar 需要解析上万条目录项，
/// 所以按「路径 + 大小 + 修改时间」记住结果，jar 没有变化时不再重新打开。
fn jar_version(jar: &std::path::Path) -> Option<String> {
    type Seen = std::collections::HashMap<std::path::PathBuf, (u64, Option<std::time::SystemTime>, Option<String>)>;
    static SEEN: std::sync::OnceLock<std::sync::Mutex<Seen>> = std::sync::OnceLock::new();
    let meta = std::fs::metadata(jar).ok()?;
    let key = (meta.len(), meta.modified().ok());
    let seen = SEEN.get_or_init(Default::default);
    if key.1.is_some() {
        if let Some((len, modified, version)) = seen.lock().ok()?.get(jar) {
            if (*len, *modified) == key {
                return version.clone();
            }
        }
    }
    let read = || -> Option<String> {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(jar).ok()?).ok()?;
        let mut text = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("version.json").ok()?, &mut text).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        v.get("id")
            .and_then(serde_json::Value::as_str)
            .map(|s| s.split(" / ").next().unwrap_or(s).to_string())
    };
    let version = read();
    if let Ok(mut s) = seen.lock() {
        s.insert(jar.to_path_buf(), (key.0, key.1, version.clone()));
    }
    version
}

/// 扫描所有实例（阻塞调用）。
pub fn scan_instances(game: &GameDir) -> Vec<InstanceSummary> {
    let Ok(rd) = std::fs::read_dir(game.versions()) else {
        return Vec::new();
    };
    let mut out: Vec<InstanceSummary> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let id = e.file_name().to_string_lossy().to_string();
            game.version_json(&id).is_file().then_some(id)
        })
        .map(|id| match resolve_version(game, &id) {
            Ok(r) => InstanceSummary {
                mc_version: minecraft_version(game, &id)
                    .unwrap_or_else(|| r.chain.last().cloned().unwrap_or_else(|| id.clone())),
                kind: r.json.kind.clone(),
                loader: detect_loader(&r),
                release_time: r.json.release_time.clone(),
                error: None,
                optifine: detect_optifine(&r),
                parents: r.chain.iter().skip(1).cloned().collect(),
                id,
            },
            Err(e) => InstanceSummary {
                mc_version: String::new(),
                kind: String::new(),
                loader: None,
                release_time: String::new(),
                error: Some(e.to_string()),
                optifine: None,
                parents: Vec::new(),
                id,
            },
        })
        .collect();
    out.sort_by(|a, b| b.release_time.cmp(&a.release_time).then_with(|| a.id.cmp(&b.id)));
    out
}

/// 重命名实例（阻塞调用）：版本文件夹、其中的 `<名称>.json` / `<名称>.jar` 以及 JSON 里的 `id` 一起改名。
///
/// 实例配置、natives 与版本隔离的存档、Mod 都在版本文件夹里，会跟着一起移动。
/// 被其他实例继承（`inheritsFrom`）的版本不能改名，否则继承它的实例会找不到父版本。
pub fn rename_instance(game: &GameDir, old: &str, new: &str) -> crate::error::Result<()> {
    use crate::error::{CoreError, IoContext};
    crate::install::validate_instance_name(new)?;
    if old == new {
        return Ok(());
    }
    if !game.version_json(old).is_file() {
        return Err(CoreError::VersionNotFound(old.to_string()));
    }
    // Windows 与 macOS 的文件名不区分大小写：只改大小写时目标「已存在」的其实是它自己
    let same_dir = old.to_lowercase() == new.to_lowercase();
    if game.version_dir(new).exists() && !same_dir {
        return Err(CoreError::msg(format!("已存在名为「{new}」的版本"), "请换一个版本名称。"));
    }
    if let Some(child) = scan_instances(game).iter().find(|s| s.parents.iter().any(|p| p == old)) {
        return Err(CoreError::msg(
            format!("「{old}」是「{}」的父版本，不能改名", child.id),
            "安装了加载器的版本依赖它提供游戏本体，改名后那些版本将无法启动。",
        ));
    }
    let json_path = game.version_json(old);
    let text = std::fs::read_to_string(&json_path).at(&json_path)?;
    let mut json: serde_json::Value = serde_json::from_str(&text).map_err(|e| CoreError::json(format!("版本文件 {old}.json"), e))?;
    json["id"] = new.into();
    // 部分启动器用 `jar` 字段指向自己的 jar
    if json.get("jar").and_then(serde_json::Value::as_str) == Some(old) {
        json["jar"] = new.into();
    }
    let data = serde_json::to_vec_pretty(&json).map_err(|e| CoreError::json("版本 JSON", e))?;

    let (from, to) = (game.version_dir(old), game.version_dir(new));
    std::fs::rename(&from, &to).at(&from)?;
    // 文件夹已经改名；里面的文件改名失败时把文件夹改回去，保证实例仍然可用
    let inner = || -> crate::error::Result<()> {
        let old_jar = to.join(format!("{old}.jar"));
        if old_jar.is_file() {
            std::fs::rename(&old_jar, game.version_jar(new)).at(&old_jar)?;
        }
        let new_json = game.version_json(new);
        std::fs::write(&new_json, &data).at(&new_json)?;
        let old_json = to.join(format!("{old}.json"));
        if old_json != new_json && !same_dir {
            std::fs::remove_file(&old_json).at(&old_json)?;
        } else if old_json != new_json {
            // 只改大小写：在不区分大小写的文件系统上两个名字是同一个文件，改名即可
            let _ = std::fs::rename(&old_json, &new_json);
        }
        Ok(())
    };
    if let Err(e) = inner() {
        let new_jar = game.version_jar(new);
        if new_jar.is_file() {
            let _ = std::fs::rename(&new_jar, to.join(format!("{old}.jar")));
        }
        let _ = std::fs::write(to.join(format!("{old}.json")), text);
        if !same_dir {
            let _ = std::fs::remove_file(game.version_json(new));
        }
        let _ = std::fs::rename(&to, &from);
        return Err(e);
    }
    Ok(())
}

/// 识别以 LaunchWrapper 方式安装的 OptiFine（库 `optifine:OptiFine:<mc>_<版本>`）。
pub fn detect_optifine(resolved: &ResolvedVersion) -> Option<String> {
    resolved.json.libraries.iter().find_map(|l| {
        let c = l.coord()?;
        (c.group == "optifine" && c.artifact == "OptiFine")
            .then(|| c.version.split_once('_').map(|x| x.1).unwrap_or(c.version).to_string())
    })
}

/// 安装加载器时作为父版本自动装上的原版：被其他实例继承，且没有 Red Craft 的实例配置（用户没有单独安装或启动过它）。
/// 实例列表中隐藏这些版本。
pub fn is_hidden_base(game: &GameDir, all: &[InstanceSummary], id: &str, config_file: &str) -> bool {
    all.iter().any(|s| s.parents.iter().any(|p| p == id)) && !game.version_dir(id).join(config_file).exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::{resolve_with, VersionJson};

    fn resolved(json: &str) -> ResolvedVersion {
        let v: VersionJson = serde_json::from_str(json).unwrap();
        resolve_with(&v.id.clone(), |_| Ok(v.clone())).unwrap()
    }

    #[test]
    fn hides_auto_installed_parent() {
        let t = tempfile::tempdir().unwrap();
        let game = GameDir::new(t.path());
        let write = |id: &str, json: &str| {
            std::fs::create_dir_all(game.version_dir(id)).unwrap();
            std::fs::write(game.version_json(id), json).unwrap();
        };
        write("1.21.1", r#"{"id":"1.21.1","type":"release","libraries":[]}"#);
        write(
            "1.21.1-Fabric",
            r#"{"id":"1.21.1-Fabric","inheritsFrom":"1.21.1","libraries":[{"name":"net.fabricmc:fabric-loader:0.16.0"}]}"#,
        );
        write("1.20.1", r#"{"id":"1.20.1","type":"release","libraries":[]}"#);
        let all = scan_instances(&game);
        assert!(is_hidden_base(&game, &all, "1.21.1", "redcraft.json"));
        assert!(!is_hidden_base(&game, &all, "1.21.1-Fabric", "redcraft.json"));
        // 没有被继承的原版照常显示
        assert!(!is_hidden_base(&game, &all, "1.20.1", "redcraft.json"));
        // 用户单独安装或启动过（有实例配置）则显示
        std::fs::write(game.version_dir("1.21.1").join("redcraft.json"), "{}").unwrap();
        assert!(!is_hidden_base(&game, &all, "1.21.1", "redcraft.json"));
    }

    #[test]
    fn renames_instance_with_its_files() {
        let t = tempfile::tempdir().unwrap();
        let game = GameDir::new(t.path());
        let write = |id: &str, json: &str| {
            std::fs::create_dir_all(game.version_dir(id)).unwrap();
            std::fs::write(game.version_json(id), json).unwrap();
        };
        write("1.20.1", r#"{"id":"1.20.1","type":"release","mainClass":"a.B","libraries":[]}"#);
        std::fs::write(game.version_jar("1.20.1"), b"jar").unwrap();
        std::fs::create_dir_all(game.version_dir("1.20.1").join("saves/world")).unwrap();
        std::fs::write(game.version_dir("1.20.1").join("redcraft.json"), "{}").unwrap();
        write("fabric", r#"{"id":"fabric","inheritsFrom":"1.21","libraries":[]}"#);
        write("1.21", r#"{"id":"1.21","type":"release","libraries":[]}"#);

        rename_instance(&game, "1.20.1", "我的生存").unwrap();
        assert!(!game.version_dir("1.20.1").exists());
        assert_eq!(std::fs::read(game.version_jar("我的生存")).unwrap(), b"jar");
        assert!(game.version_dir("我的生存").join("saves/world").is_dir());
        assert!(game.version_dir("我的生存").join("redcraft.json").is_file());
        assert!(!game.version_dir("我的生存").join("1.20.1.json").exists());
        let r = resolve_version(&game, "我的生存").unwrap();
        assert_eq!((r.json.id.as_str(), r.jar_id.as_str(), r.json.main_class.as_str()), ("我的生存", "我的生存", "a.B"));

        // 重名、非法名称、被继承的父版本、不存在的版本都拒绝，且不改动任何文件
        assert!(rename_instance(&game, "我的生存", "fabric").is_err());
        assert!(rename_instance(&game, "我的生存", "a/b").is_err());
        assert!(rename_instance(&game, "1.21", "1.21-renamed").is_err());
        assert!(rename_instance(&game, "missing", "x").is_err());
        assert!(game.version_json("我的生存").is_file() && game.version_json("1.21").is_file());
        // 继承别的版本的实例可以改名，父版本不受影响
        rename_instance(&game, "fabric", "Fabric 生存").unwrap();
        assert_eq!(resolve_version(&game, "Fabric 生存").unwrap().chain, vec!["Fabric 生存", "1.21"]);
    }

    #[test]
    fn loaders() {
        let r = resolved(r#"{"id":"a","libraries":[{"name":"net.fabricmc:fabric-loader:0.15.11"}]}"#);
        assert_eq!(detect_loader(&r), Some(LoaderInfo { kind: LoaderKind::Fabric, version: "0.15.11".into() }));
        let r = resolved(r#"{"id":"a","libraries":[{"name":"net.minecraftforge:forge:1.20.1-47.2.0:universal"}]}"#);
        assert_eq!(detect_loader(&r).unwrap().version, "47.2.0");
        let r = resolved(r#"{"id":"a","libraries":[{"name":"net.neoforged:neoforge:21.1.65:universal"}]}"#);
        assert_eq!(detect_loader(&r).unwrap().kind, LoaderKind::NeoForge);
        // NeoForge 1.20.1：同时带有 net.minecraftforge 的库
        let r = resolved(
            r#"{"id":"a","mainClass":"cpw.mods.bootstraplauncher.BootstrapLauncher",
                "arguments":{"game":["--launchTarget","forgeclient","--fml.forgeVersion","47.1.106"],"jvm":[]},
                "libraries":[{"name":"net.neoforged.fancymodloader:loader:47.2.2"}]}"#,
        );
        assert_eq!(detect_loader(&r), Some(LoaderInfo { kind: LoaderKind::NeoForge, version: "47.1.106".into() }));
        // Quilt 带有 fabric 兼容库时仍识别为 Quilt
        let r = resolved(
            r#"{"id":"a","libraries":[{"name":"net.fabricmc:intermediary:1.20.1"},{"name":"org.quiltmc:quilt-loader:0.24.0"}]}"#,
        );
        assert_eq!(detect_loader(&r).unwrap().kind, LoaderKind::Quilt);
        let r = resolved(r#"{"id":"a","mainClass":"net.minecraft.client.main.Main"}"#);
        assert_eq!(detect_loader(&r), None);
    }

    #[test]
    fn scan_uses_game_version_for_standalone_and_renamed_instances() {
        let dir = tempfile::tempdir().unwrap();
        let game = GameDir::new(dir.path());
        let cases = [
            (
                "26.3-Fabric 0.19.5",
                r#"{"clientVersion":"26.3","libraries":[{"name":"net.fabricmc:fabric-loader:0.19.5"}]}"#,
                "26.3",
            ),
            (
                "我的生存",
                r#"{"patches":[{"id":"game","version":"1.21.1"}]}"#,
                "1.21.1",
            ),
            ("1.20.1", r#"{}"#, "1.20.1"),
            ("26.3-custom", r#"{"inheritsFrom":"26.3-Fabric 0.19.5"}"#, "26.3"),
        ];
        for (id, json, _) in &cases {
            std::fs::create_dir_all(game.version_dir(id)).unwrap();
            let mut json: serde_json::Value = serde_json::from_str(json).unwrap();
            json["id"] = (*id).into();
            std::fs::write(game.version_json(id), json.to_string()).unwrap();
        }
        let list = scan_instances(&game);
        for (id, _, expected) in cases {
            let instance = list.iter().find(|i| i.id == id).unwrap();
            assert_eq!(instance.mc_version, expected, "{id}");
        }
        assert_eq!(
            list.iter()
                .find(|i| i.id == "26.3-Fabric 0.19.5")
                .unwrap()
                .loader
                .as_ref()
                .unwrap()
                .kind,
            LoaderKind::Fabric
        );
    }

    #[test]
    fn scan_reads_game_version_from_standalone_client_jar() {
        use std::io::Write;

        let dir = tempfile::tempdir().unwrap();
        let game = GameDir::new(dir.path());
        let id = "26.3-Fabric 0.19.5";
        std::fs::create_dir_all(game.version_dir(id)).unwrap();
        std::fs::write(game.version_json(id), format!(r#"{{"id":"{id}"}}"#)).unwrap();
        let mut jar = zip::ZipWriter::new(std::fs::File::create(game.version_jar(id)).unwrap());
        jar.start_file("version.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        jar.write_all(br#"{"id":"26.3"}"#).unwrap();
        jar.finish().unwrap();

        assert_eq!(scan_instances(&game)[0].mc_version, "26.3");
    }

    #[test]
    fn scan() {
        let dir = tempfile::tempdir().unwrap();
        let game = GameDir::new(dir.path());
        std::fs::create_dir_all(game.version_dir("1.20.1")).unwrap();
        std::fs::write(game.version_json("1.20.1"), r#"{"id":"1.20.1","type":"release","releaseTime":"2023"}"#).unwrap();
        std::fs::create_dir_all(game.version_dir("broken")).unwrap();
        std::fs::write(game.version_json("broken"), "{").unwrap();
        std::fs::create_dir_all(game.version_dir("empty")).unwrap();
        let list = scan_instances(&game);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "1.20.1");
        assert!(list[0].is_release());
        assert!(list[1].error.is_some());
    }
}
