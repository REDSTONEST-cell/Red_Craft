//! 从其他启动器导入：识别官方启动器 / HMCL / PCL 的 `.minecraft` 文件夹，以及 Prism Launcher / MultiMC 的实例，
//! 列出其中的实例（游戏版本、加载器、Mod 与存档数量）和离线账号。
//!
//! 导入实例时不直接复制版本文件（不同启动器的版本 JSON、加载器安装方式各不相同），
//! 而是在 Red Craft 中重新安装相同的游戏版本与加载器，再用 [`copy_user_files`] 复制 Mod、存档、配置等用户数据。

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::error::{CoreError, IoContext, Result};
use crate::instance::{scan_instances, LoaderInfo, LoaderKind};
use crate::paths::GameDir;

/// 来源启动器类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LauncherKind {
    Official,
    Hmcl,
    Pcl,
    Prism,
    MultiMc,
    /// 无法判断来源的 `.minecraft` 文件夹
    Folder,
}

impl LauncherKind {
    pub fn display_name(self) -> &'static str {
        match self {
            LauncherKind::Official => "官方启动器",
            LauncherKind::Hmcl => "HMCL",
            LauncherKind::Pcl => "PCL",
            LauncherKind::Prism => "Prism Launcher",
            LauncherKind::MultiMc => "MultiMC",
            LauncherKind::Folder => ".minecraft 文件夹",
        }
    }
}

/// 一个可以扫描的位置。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportLocation {
    pub kind: LauncherKind,
    pub label: String,
    pub path: PathBuf,
}

/// 用户数据目录的布局，决定复制哪些文件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilesLayout {
    /// 实例独占的目录（版本隔离 / Prism 实例）：除启动器自身文件外全部复制
    Isolated,
    /// 多个版本共用的 `.minecraft` 根目录：只复制常见的用户数据
    Shared,
}

/// 可导入的实例。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportInstance {
    pub name: String,
    pub mc_version: String,
    pub loader: Option<LoaderInfo>,
    /// 用户数据所在目录
    pub files_dir: PathBuf,
    pub layout: FilesLayout,
    pub mods: usize,
    pub saves: usize,
    /// 不能导入的原因（版本文件损坏、无法识别游戏版本等）
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub kind: LauncherKind,
    pub label: String,
    pub root: PathBuf,
    pub instances: Vec<ImportInstance>,
    /// 离线账号的玩家名
    pub offline_accounts: Vec<String>,
}

/// 列出本机上常见的其他启动器数据位置（存在的才返回）。`exclude` 为 Red Craft 当前使用的游戏目录。
pub fn detect_locations(exclude: &Path) -> Vec<ImportLocation> {
    let mut out = Vec::new();
    let mut push = |kind: LauncherKind, path: PathBuf| {
        if path.is_dir() && !same_path(&path, exclude) && !out.iter().any(|l: &ImportLocation| same_path(&l.path, &path)) {
            out.push(ImportLocation {
                kind,
                label: kind.display_name().to_string(),
                path,
            });
        }
    };
    let official = GameDir::default_location();
    if official.join("versions").is_dir() {
        push(LauncherKind::Official, official);
    }
    let data = dirs::data_dir();
    let home = dirs::home_dir();
    for base in data.iter() {
        push(LauncherKind::Prism, base.join("PrismLauncher"));
        push(LauncherKind::MultiMc, base.join("MultiMC"));
    }
    if let Some(h) = &home {
        push(
            LauncherKind::Prism,
            h.join(".var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher"),
        );
    }
    // HMCL 的全局账号列表（游戏目录在 HMCL 程序旁边，需要用户手动选择）
    for p in hmcl_global_dirs() {
        if p.join("accounts.json").is_file() {
            push(LauncherKind::Hmcl, p);
        }
    }
    out
}

fn hmcl_global_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if cfg!(target_os = "windows") {
        v.extend(dirs::data_dir().map(|d| d.join(".hmcl")));
    } else if cfg!(target_os = "macos") {
        v.extend(dirs::data_dir().map(|d| d.join("hmcl")));
    } else {
        v.extend(dirs::data_dir().map(|d| d.join("hmcl")));
        v.extend(dirs::home_dir().map(|d| d.join(".hmcl")));
    }
    v
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// 扫描用户选择的文件夹。可以是 `.minecraft`、含有 `.minecraft` 的启动器文件夹（HMCL / PCL），
/// Prism / MultiMC 的数据目录，或 HMCL 的全局配置目录。
pub fn scan(path: &Path) -> Result<ScanResult> {
    if path.join("instances").is_dir() && (path.join("prismlauncher.cfg").is_file() || path.join("multimc.cfg").is_file())
        || path.join("mmc-pack.json").is_file()
    {
        return scan_prism(path);
    }
    let mc_dir = if path.join("versions").is_dir() {
        Some(path.to_path_buf())
    } else if path.join(".minecraft").join("versions").is_dir() {
        Some(path.join(".minecraft"))
    } else if path.join("minecraft").join("versions").is_dir() {
        Some(path.join("minecraft"))
    } else {
        None
    };
    // 只有账号的 HMCL 全局目录
    if mc_dir.is_none() && path.join("accounts.json").is_file() {
        return Ok(ScanResult {
            kind: LauncherKind::Hmcl,
            label: "HMCL（账号）".into(),
            root: path.to_path_buf(),
            instances: Vec::new(),
            offline_accounts: hmcl_accounts(path),
        });
    }
    let Some(mc_dir) = mc_dir else {
        return Err(CoreError::InvalidData(format!(
            "「{}」里没有找到游戏版本：请选择 .minecraft 文件夹、启动器所在的文件夹，或 Prism / MultiMC 的数据目录",
            path.display()
        )));
    };
    scan_minecraft_dir(&mc_dir)
}

/// 根据 `.minecraft` 旁边的文件判断它属于哪个启动器。
fn guess_launcher(mc_dir: &Path) -> LauncherKind {
    let parent = mc_dir.parent().unwrap_or(mc_dir);
    let names: Vec<String> = std::fs::read_dir(parent)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_lowercase())
        .collect();
    let has = |f: &dyn Fn(&str) -> bool| names.iter().any(|n| f(n));
    if mc_dir.join("PCL").is_dir() || has(&|n| n == "pcl" || n.starts_with("plain craft launcher") || n.starts_with("pcl")) {
        LauncherKind::Pcl
    } else if has(&|n| n == "hmcl.json" || n == ".hmcl.json" || n.starts_with("hmcl")) {
        LauncherKind::Hmcl
    } else if same_path(mc_dir, &GameDir::default_location()) || mc_dir.join("launcher_profiles.json").is_file() {
        LauncherKind::Official
    } else {
        LauncherKind::Folder
    }
}

fn scan_minecraft_dir(mc_dir: &Path) -> Result<ScanResult> {
    let kind = guess_launcher(mc_dir);
    let game = GameDir::new(mc_dir);
    let mut instances = Vec::new();
    for s in scan_instances(&game) {
        let vdir = game.version_dir(&s.id);
        let isolated = ["mods", "saves", "config", "resourcepacks", "options.txt"]
            .iter()
            .any(|f| vdir.join(f).exists());
        let (files_dir, layout) = if isolated {
            (vdir, FilesLayout::Isolated)
        } else {
            (mc_dir.to_path_buf(), FilesLayout::Shared)
        };
        let problem = s
            .error
            .clone()
            .map(|e| format!("版本文件无法解析：{e}"))
            .or_else(|| s.mc_version.is_empty().then(|| "无法识别游戏版本".to_string()));
        let mc_version = real_mc_version(&game, &s.id).unwrap_or_else(|| s.mc_version.clone());
        instances.push(ImportInstance {
            name: s.id.clone(),
            mc_version,
            loader: s.loader.clone(),
            mods: count_mods(&files_dir),
            saves: count_dirs(&files_dir.join("saves")),
            files_dir,
            layout,
            problem,
        });
    }
    let parent = mc_dir.parent().unwrap_or(mc_dir);
    let mut accounts = hmcl_accounts(parent);
    if kind == LauncherKind::Hmcl {
        for d in hmcl_global_dirs() {
            accounts.extend(hmcl_accounts(&d));
        }
    }
    dedup(&mut accounts);
    Ok(ScanResult {
        kind,
        label: kind.display_name().to_string(),
        root: mc_dir.to_path_buf(),
        instances,
        offline_accounts: accounts,
    })
}

/// 其他启动器允许重命名版本文件夹，此时版本名不再是游戏版本号。依次尝试：
/// `clientVersion`（PCL / 官方）、HMCL 的 `patches` 中的 game、jar 内的 `version.json`（1.14+）。
fn real_mc_version(game: &GameDir, id: &str) -> Option<String> {
    let mut cur = id.to_string();
    // 沿继承链找到最底层的版本
    for _ in 0..8 {
        let json: Value = serde_json::from_str(&std::fs::read_to_string(game.version_json(&cur)).ok()?).ok()?;
        if let Some(v) = json.get("clientVersion").and_then(Value::as_str).filter(|v| !v.is_empty()) {
            return Some(v.to_string());
        }
        if let Some(v) = json
            .get("patches")
            .and_then(Value::as_array)
            .and_then(|p| p.iter().find(|x| x.get("id").and_then(Value::as_str) == Some("game")))
            .and_then(|g| g.get("version").and_then(Value::as_str))
        {
            return Some(v.to_string());
        }
        match json.get("inheritsFrom").and_then(Value::as_str) {
            Some(parent) if game.version_json(parent).is_file() => cur = parent.to_string(),
            Some(parent) => return Some(parent.to_string()),
            None => break,
        }
    }
    let jar = std::fs::File::open(game.version_jar(&cur)).ok()?;
    let mut zip = zip::ZipArchive::new(jar).ok()?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("version.json").ok()?, &mut text).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.get("id").and_then(Value::as_str).map(|s| s.split(" / ").next().unwrap_or(s).to_string())
}

/// HMCL 的离线账号：`accounts.json`（全局）或 `hmcl.json` / `.hmcl.json` 中的 `accounts`。
fn hmcl_accounts(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for f in ["accounts.json", "hmcl.json", ".hmcl.json"] {
        let Ok(text) = std::fs::read_to_string(dir.join(f)) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
        let list = match &v {
            Value::Array(a) => a.clone(),
            Value::Object(o) => o.get("accounts").and_then(Value::as_array).cloned().unwrap_or_default(),
            _ => Vec::new(),
        };
        for a in list {
            let offline = a.get("type").and_then(Value::as_str).is_some_and(|t| t.eq_ignore_ascii_case("offline"));
            if let (true, Some(name)) = (offline, a.get("username").and_then(Value::as_str)) {
                out.push(name.to_string());
            }
        }
    }
    out
}

fn scan_prism(path: &Path) -> Result<ScanResult> {
    // 选中的是单个实例
    if path.join("mmc-pack.json").is_file() {
        let inst = prism_instance(path).ok_or_else(|| CoreError::InvalidData("无法读取这个 Prism 实例".into()))?;
        return Ok(ScanResult {
            kind: LauncherKind::Prism,
            label: LauncherKind::Prism.display_name().into(),
            root: path.to_path_buf(),
            instances: vec![inst],
            offline_accounts: Vec::new(),
        });
    }
    let kind = if path.join("multimc.cfg").is_file() && !path.join("prismlauncher.cfg").is_file() {
        LauncherKind::MultiMc
    } else {
        LauncherKind::Prism
    };
    let mut instances: Vec<ImportInstance> = std::fs::read_dir(path.join("instances"))
        .at(&path.join("instances"))?
        .flatten()
        .filter_map(|e| prism_instance(&e.path()))
        .collect();
    instances.sort_by_key(|i| i.name.to_lowercase());
    let mut accounts = prism_accounts(path);
    dedup(&mut accounts);
    Ok(ScanResult {
        kind,
        label: kind.display_name().into(),
        root: path.to_path_buf(),
        instances,
        offline_accounts: accounts,
    })
}

fn prism_instance(dir: &Path) -> Option<ImportInstance> {
    let pack: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("mmc-pack.json")).ok()?).ok()?;
    let mut mc = String::new();
    let mut loader = None;
    for c in pack.get("components")?.as_array()? {
        let uid = c.get("uid").and_then(Value::as_str).unwrap_or_default();
        let version = c.get("version").and_then(Value::as_str).unwrap_or_default().to_string();
        let kind = match uid {
            "net.minecraft" => {
                mc = version;
                continue;
            }
            "net.fabricmc.fabric-loader" => LoaderKind::Fabric,
            "org.quiltmc.quilt-loader" => LoaderKind::Quilt,
            "net.minecraftforge" => LoaderKind::Forge,
            "net.neoforged" => LoaderKind::NeoForge,
            _ => continue,
        };
        loader = Some(LoaderInfo { kind, version });
    }
    let cfg = std::fs::read_to_string(dir.join("instance.cfg")).unwrap_or_default();
    let name = cfg
        .lines()
        .find_map(|l| l.strip_prefix("name="))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
    let files_dir = [".minecraft", "minecraft"]
        .iter()
        .map(|d| dir.join(d))
        .find(|d| d.is_dir())
        .unwrap_or_else(|| dir.join(".minecraft"));
    Some(ImportInstance {
        problem: mc.is_empty().then(|| "无法识别游戏版本".to_string()),
        name,
        mc_version: mc,
        loader,
        mods: count_mods(&files_dir),
        saves: count_dirs(&files_dir.join("saves")),
        files_dir,
        layout: FilesLayout::Isolated,
    })
}

/// Prism 的 `accounts.json`：`{"accounts":[{"type":"Offline","profile":{"name":"Steve"}}]}`。
fn prism_accounts(dir: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(dir.join("accounts.json")) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    v.get("accounts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|a| a.get("type").and_then(Value::as_str).is_some_and(|t| t.eq_ignore_ascii_case("offline")))
        .filter_map(|a| a.pointer("/profile/name").and_then(Value::as_str).map(str::to_string))
        .collect()
}

fn dedup(v: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    v.retain(|n| !n.trim().is_empty() && seen.insert(n.to_lowercase()));
}

fn count_mods(dir: &Path) -> usize {
    std::fs::read_dir(dir.join("mods"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_lowercase();
            n.ends_with(".jar") || n.ends_with(".jar.disabled")
        })
        .count()
}

fn count_dirs(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .count()
}

/// 共用 `.minecraft` 根目录中复制的用户数据。
const SHARED_ENTRIES: &[&str] = &[
    "mods",
    "config",
    "defaultconfigs",
    "saves",
    "resourcepacks",
    "texturepacks",
    "shaderpacks",
    "schematics",
    "kubejs",
    "scripts",
    "journeymap",
    "XaeroWaypoints",
    "XaeroWorldMap",
    "screenshots",
    "options.txt",
    "optionsof.txt",
    "optionsshaders.txt",
    "servers.dat",
];

/// 实例独占目录中不复制的启动器 / 运行时文件。
const SKIPPED_ENTRIES: &[&str] = &[
    "natives",
    "logs",
    "crash-reports",
    "versions",
    "libraries",
    "assets",
    "PCL",
    ".fabric",
    ".mixin.out",
    "hmclversion.cfg",
    "redcraft.json",
    "usercache.json",
    "usernamecache.json",
];

/// 计算要复制的顶层条目。`version_id` 是 `.minecraft` 版本隔离目录的版本名，用于跳过它的 json / jar。
pub fn files_to_copy(from: &Path, layout: FilesLayout, version_id: Option<&str>) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(from) else {
        return Vec::new();
    };
    rd.flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            match layout {
                FilesLayout::Shared => SHARED_ENTRIES.contains(&name.as_str()),
                FilesLayout::Isolated => {
                    !SKIPPED_ENTRIES.contains(&name.as_str())
                        && !name.ends_with("-natives")
                        && !version_id.is_some_and(|id| name == format!("{id}.json") || name == format!("{id}.jar"))
                }
            }
        })
        .map(|e| e.path())
        .collect()
}

/// 复制统计。
#[derive(Debug, Default, Clone, Copy)]
pub struct CopyStats {
    pub files: u64,
    pub bytes: u64,
}

/// 把用户数据复制到 Red Craft 实例目录（已存在的文件会被覆盖）。`progress(已复制文件数, 总文件数)`。
pub fn copy_user_files(
    entries: &[PathBuf],
    from: &Path,
    to: &Path,
    progress: &mut dyn FnMut(u64, u64),
    cancelled: &dyn Fn() -> bool,
) -> Result<CopyStats> {
    let mut files = Vec::new();
    for e in entries {
        collect_files(e, &mut files);
    }
    let total = files.len() as u64;
    let mut stats = CopyStats::default();
    for f in files {
        if cancelled() {
            return Err(CoreError::Cancelled);
        }
        let Ok(rel) = f.strip_prefix(from) else { continue };
        let dest = to.join(rel);
        if let Some(p) = dest.parent() {
            std::fs::create_dir_all(p).at(p)?;
        }
        stats.bytes += std::fs::copy(&f, &dest).at(&f)?;
        stats.files += 1;
        if stats.files % 64 == 0 || stats.files == total {
            progress(stats.files, total);
        }
    }
    Ok(stats)
}

fn collect_files(path: &Path, out: &mut Vec<PathBuf>) {
    // 不跟随符号链接，避免循环
    let Ok(meta) = std::fs::symlink_metadata(path) else { return };
    if meta.is_dir() {
        for e in std::fs::read_dir(path).into_iter().flatten().flatten() {
            collect_files(&e.path(), out);
        }
    } else if meta.is_file() {
        out.push(path.to_path_buf());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(p: &Path, s: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, s).unwrap();
    }

    #[test]
    fn scans_prism() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        write(&root.join("prismlauncher.cfg"), "");
        write(
            &root.join("accounts.json"),
            r#"{"accounts":[{"type":"MSA","profile":{"name":"Real"}},{"type":"Offline","profile":{"name":"Steve"}}],"formatVersion":3}"#,
        );
        let inst = root.join("instances/abc");
        write(&inst.join("instance.cfg"), "[General]\nname=我的生存\n");
        write(
            &inst.join("mmc-pack.json"),
            r#"{"components":[{"uid":"net.minecraft","version":"1.20.1"},{"uid":"net.fabricmc.intermediary","version":"1.20.1"},{"uid":"net.fabricmc.fabric-loader","version":"0.16.10"}]}"#,
        );
        write(&inst.join(".minecraft/mods/a.jar"), "");
        write(&inst.join(".minecraft/mods/b.jar.disabled"), "");
        write(&inst.join(".minecraft/saves/w1/level.dat"), "x");
        write(&inst.join(".minecraft/logs/latest.log"), "");

        let r = scan(root).unwrap();
        assert_eq!(r.kind, LauncherKind::Prism);
        assert_eq!(r.offline_accounts, vec!["Steve"]);
        let i = &r.instances[0];
        assert_eq!(i.name, "我的生存");
        assert_eq!(i.mc_version, "1.20.1");
        assert_eq!(i.loader.as_ref().unwrap().kind, LoaderKind::Fabric);
        assert_eq!(i.loader.as_ref().unwrap().version, "0.16.10");
        assert_eq!((i.mods, i.saves), (2, 1));
        assert!(i.problem.is_none());

        let entries = files_to_copy(&i.files_dir, i.layout, None);
        let dest = t.path().join("dest");
        let stats = copy_user_files(&entries, &i.files_dir, &dest, &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(stats.files, 3);
        assert!(dest.join("saves/w1/level.dat").is_file());
        assert!(!dest.join("logs").exists());
    }

    #[test]
    fn scans_minecraft_dir() {
        let t = tempfile::tempdir().unwrap();
        let launcher = t.path().join("HMCL");
        write(&launcher.join("HMCL-3.6.exe"), "");
        write(
            &launcher.join("hmcl.json"),
            r#"{"accounts":[{"type":"offline","username":"Alex","uuid":"x"},{"type":"microsoft","username":"m"}]}"#,
        );
        let mc = launcher.join(".minecraft");
        write(
            &mc.join("versions/1.20.1/1.20.1.json"),
            r#"{"id":"1.20.1","type":"release","mainClass":"net.minecraft.client.main.Main","libraries":[]}"#,
        );
        write(
            &mc.join("versions/Fab/Fab.json"),
            r#"{"id":"Fab","inheritsFrom":"1.20.1","type":"release","mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient","libraries":[{"name":"net.fabricmc:fabric-loader:0.16.10"}]}"#,
        );
        write(&mc.join("versions/Fab/mods/x.jar"), "");
        write(&mc.join("versions/Fab/Fab.jar"), "");
        write(&mc.join("versions/Fab/natives/a.dll"), "");
        write(&mc.join("saves/root/level.dat"), "");
        write(&mc.join("launcher_profiles.json"), "{}");

        write(
            &mc.join("versions/我的生存/我的生存.json"),
            r#"{"id":"我的生存","type":"release","mainClass":"net.minecraft.client.main.Main","libraries":[],"patches":[{"id":"game","version":"1.21.1"}]}"#,
        );

        // 选择启动器所在文件夹也能找到 .minecraft
        let r = scan(&launcher).unwrap();
        assert_eq!(r.kind, LauncherKind::Hmcl);
        assert_eq!(r.offline_accounts, vec!["Alex"]);
        let fab = r.instances.iter().find(|i| i.name == "Fab").unwrap();
        assert_eq!(fab.layout, FilesLayout::Isolated);
        assert_eq!(fab.loader.as_ref().unwrap().kind, LoaderKind::Fabric);
        assert_eq!(fab.mc_version, "1.20.1");
        assert_eq!(fab.mods, 1);
        let entries = files_to_copy(&fab.files_dir, fab.layout, Some("Fab"));
        let names: Vec<_> = entries.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names, vec!["mods"]);

        let renamed = r.instances.iter().find(|i| i.name == "我的生存").unwrap();
        assert_eq!(renamed.mc_version, "1.21.1");

        let vanilla = r.instances.iter().find(|i| i.name == "1.20.1").unwrap();
        assert_eq!(vanilla.layout, FilesLayout::Shared);
        assert_eq!(vanilla.saves, 1);
        let entries = files_to_copy(&vanilla.files_dir, vanilla.layout, None);
        assert_eq!(entries.len(), 1); // 只有 saves，不含 versions / launcher_profiles.json
    }

    #[test]
    fn rejects_unknown_folder() {
        let t = tempfile::tempdir().unwrap();
        assert!(scan(t.path()).is_err());
    }
}
