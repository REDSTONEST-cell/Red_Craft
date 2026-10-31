//! 已安装的资源包、光影包与存档：列出、启用 / 切换、导入。
//!
//! - 资源包：读取 `pack.mcmeta` 的描述与 `pack.png`；是否启用来自 `options.txt` 的 `resourcePacks`；
//! - 光影包：正在使用的光影来自 Iris / Oculus 的 `config/iris.properties` 或 OptiFine 的 `optionsshaders.txt`；
//! - 存档：读取 `level.dat`（NBT）中的世界名、游戏模式、版本与最后游玩时间，以及 `icon.png`。

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{CoreError, IoContext, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    Mod,
    ResourcePack,
    Shader,
    Save,
}

impl FileKind {
    /// 游戏目录下对应的文件夹。
    pub fn folder(self) -> &'static str {
        match self {
            FileKind::Mod => "mods",
            FileKind::ResourcePack => "resourcepacks",
            FileKind::Shader => "shaderpacks",
            FileKind::Save => "saves",
        }
    }
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LocalFile {
    /// 磁盘上的文件或文件夹名
    pub name: String,
    /// 显示名（存档为世界名）
    pub title: String,
    pub description: String,
    /// 图标（data URL）
    pub icon: Option<String>,
    /// 字节数；文件夹为其中所有文件的总大小
    pub size: u64,
    /// 修改时间或最后游玩时间（Unix 秒）
    pub modified: u64,
    pub dir: bool,
    /// 资源包：已启用；光影包：正在使用
    pub active: bool,
    /// 存档：游戏模式与版本
    pub detail: Option<String>,
}

const MAX_ICON: u64 = 512 * 1024;

fn data_url(data: &[u8]) -> String {
    use base64::Engine;
    let mime = if data.starts_with(&[0xff, 0xd8]) { "image/jpeg" } else { "image/png" };
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(data))
}

fn mtime(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn dir_size(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

/// 读取文件夹或 zip 中的一个小文件。
fn read_inner(path: &Path, dir: bool, name: &str, limit: u64) -> Option<Vec<u8>> {
    if dir {
        let p = path.join(name);
        if std::fs::metadata(&p).ok()?.len() > limit {
            return None;
        }
        return std::fs::read(p).ok();
    }
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
    let mut e = zip.by_name(name).ok()?;
    if e.size() > limit {
        return None;
    }
    let mut buf = Vec::with_capacity(e.size() as usize);
    e.read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// 文本组件（字符串 / 对象 / 数组）转为纯文本，并去掉 § 格式代码。
pub fn plain_text(v: &Value) -> String {
    fn walk(v: &Value, out: &mut String) {
        match v {
            Value::String(s) => out.push_str(s),
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            Value::Object(o) => {
                if let Some(t) = o.get("text").or_else(|| o.get("translate")) {
                    walk(t, out);
                }
                if let Some(extra) = o.get("extra") {
                    walk(extra, out);
                }
            }
            Value::Number(n) => out.push_str(&n.to_string()),
            _ => {}
        }
    }
    let mut raw = String::new();
    walk(v, &mut raw);
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out.trim().to_string()
}

// ─── options.txt / 光影配置 ───

/// `options.txt` 中启用的资源包（原样，例如 `file/Faithful.zip`）。
fn enabled_packs(game_dir: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(game_dir.join("options.txt")).unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix("resourcePacks:"))
        .and_then(|v| serde_json::from_str::<Vec<String>>(v.trim()).ok())
        .unwrap_or_default()
}

fn pack_enabled(list: &[String], name: &str) -> bool {
    list.iter().any(|p| p == name || p.strip_prefix("file/") == Some(name))
}

/// 1.13 之前的版本：资源包列表直接写文件名，没有 `file/` 前缀与 `vanilla`。
pub fn legacy_pack_names(mc_version: &str) -> bool {
    let mut parts = mc_version.split('.');
    match (parts.next(), parts.next().and_then(|m| m.split(|c: char| !c.is_ascii_digit()).next())) {
        (Some("1"), Some(minor)) => minor.parse::<u32>().is_ok_and(|m| m < 13),
        _ => false,
    }
}

/// 启用或停用资源包：修改 `options.txt` 的 `resourcePacks`（新启用的放在最上层）。
pub fn set_resource_pack(game_dir: &Path, name: &str, enabled: bool, legacy: bool) -> Result<()> {
    let path = game_dir.join("options.txt");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut list: Vec<String> = enabled_packs(game_dir)
        .into_iter()
        .filter(|p| p != name && p.strip_prefix("file/") != Some(name))
        .collect();
    if enabled {
        if !legacy && !list.iter().any(|p| p == "vanilla") {
            list.insert(0, "vanilla".into());
        }
        list.push(if legacy { name.to_string() } else { format!("file/{name}") });
    }
    let line = format!("resourcePacks:{}", serde_json::to_string(&list).unwrap_or_else(|_| "[]".into()));
    let mut found = false;
    let mut lines: Vec<String> = text
        .lines()
        .map(|l| {
            if l.starts_with("resourcePacks:") {
                found = true;
                line.clone()
            } else {
                l.to_string()
            }
        })
        .collect();
    if !found {
        lines.push(line);
    }
    std::fs::create_dir_all(game_dir).at(game_dir)?;
    std::fs::write(&path, lines.join("\n") + "\n").at(&path)
}

/// Java Properties 的值转义与反转义（Iris 用 Properties 保存配置）。
fn prop_escape(v: &str) -> String {
    let mut out = String::new();
    for (i, c) in v.chars().enumerate() {
        match c {
            '\\' | '=' | ':' | '#' | '!' => {
                out.push('\\');
                out.push(c);
            }
            ' ' if i == 0 => out.push_str("\\ "),
            _ => out.push(c),
        }
    }
    out
}

fn prop_unescape(v: &str) -> String {
    let mut out = String::new();
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                out.push(n);
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn read_props(path: &Path) -> Vec<(String, String)> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim_start().starts_with('!'))
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            Some((k.trim().to_string(), v.to_string()))
        })
        .collect()
}

fn write_props(path: &Path, set: &[(&str, String)]) -> Result<()> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    for (key, value) in set {
        let line = format!("{key}={value}");
        match lines.iter_mut().find(|l| l.split_once('=').is_some_and(|(k, _)| k.trim() == *key)) {
            Some(l) => *l = line,
            None => lines.push(line),
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).at(parent)?;
    }
    std::fs::write(path, lines.join("\n") + "\n").at(path)
}

fn iris_config(game_dir: &Path) -> PathBuf {
    game_dir.join("config").join("iris.properties")
}

fn optifine_config(game_dir: &Path) -> PathBuf {
    game_dir.join("optionsshaders.txt")
}

/// 当前使用的光影包文件名。
pub fn active_shader(game_dir: &Path) -> Option<String> {
    let iris = read_props(&iris_config(game_dir));
    let get = |k: &str| iris.iter().find(|(key, _)| key == k).map(|(_, v)| prop_unescape(v));
    if let Some(pack) = get("shaderPack").filter(|p| !p.is_empty()) {
        if get("enableShaders").as_deref() != Some("false") {
            return Some(pack);
        }
    }
    read_props(&optifine_config(game_dir))
        .into_iter()
        .find(|(k, _)| k == "shaderPack")
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| !v.is_empty() && v != "OFF" && v != "(internal)")
}

/// 切换光影：`name` 为 None 时关闭光影。`iris` / `optifine` 表示实例装有对应的光影加载器。
pub fn set_shader(game_dir: &Path, name: Option<&str>, iris: bool, optifine: bool) -> Result<()> {
    let iris = iris || iris_config(game_dir).is_file();
    let optifine = optifine || optifine_config(game_dir).is_file();
    if !iris && !optifine {
        return Err(CoreError::msg(
            "这个版本没有安装光影加载器",
            "请先安装 Iris / Oculus 或 OptiFine：可以在「资源」中搜索 Iris，或安装新版本时勾选 OptiFine。",
        ));
    }
    if iris {
        let mut set = vec![("enableShaders", name.is_some().to_string())];
        if let Some(n) = name {
            set.push(("shaderPack", prop_escape(n)));
        }
        write_props(&iris_config(game_dir), &set)?;
    }
    if optifine {
        write_props(&optifine_config(game_dir), &[("shaderPack", name.unwrap_or("OFF").to_string())])?;
    }
    Ok(())
}

// ─── NBT（只读，用于 level.dat） ───

#[derive(Debug, Clone, PartialEq)]
enum Tag {
    Int(i64),
    Str(String),
    Compound(Vec<(String, Tag)>),
    Other,
}

impl Tag {
    fn get(&self, key: &str) -> Option<&Tag> {
        match self {
            Tag::Compound(v) => v.iter().find(|(k, _)| k == key).map(|(_, t)| t),
            _ => None,
        }
    }
    fn int(&self) -> Option<i64> {
        match self {
            Tag::Int(v) => Some(*v),
            _ => None,
        }
    }
    fn str(&self) -> Option<&str> {
        match self {
            Tag::Str(v) => Some(v),
            _ => None,
        }
    }
}

struct Nbt<'a> {
    b: &'a [u8],
    i: usize,
    depth: u32,
}

impl Nbt<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.i..self.i.checked_add(n)?)?;
        self.i += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|s| s[0])
    }
    fn i16(&mut self) -> Option<i16> {
        self.take(2).map(|s| i16::from_be_bytes([s[0], s[1]]))
    }
    fn i32(&mut self) -> Option<i32> {
        self.take(4).map(|s| i32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn len(&mut self) -> Option<usize> {
        usize::try_from(self.i32()?).ok()
    }
    fn string(&mut self) -> Option<String> {
        let n = self.i16()? as u16 as usize;
        // Java 的 modified UTF-8 与 UTF-8 在常见字符上一致
        Some(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn payload(&mut self, ty: u8) -> Option<Tag> {
        Some(match ty {
            1 => Tag::Int(self.u8()? as i8 as i64),
            2 => Tag::Int(self.i16()? as i64),
            3 => Tag::Int(self.i32()? as i64),
            4 => Tag::Int(i64::from_be_bytes(self.take(8)?.try_into().ok()?)),
            5 => {
                self.take(4)?;
                Tag::Other
            }
            6 => {
                self.take(8)?;
                Tag::Other
            }
            7 => {
                let n = self.len()?;
                self.take(n)?;
                Tag::Other
            }
            8 => Tag::Str(self.string()?),
            9 => {
                let inner = self.u8()?;
                let n = self.len()?;
                self.nested(|p| {
                    for _ in 0..n {
                        p.payload(inner)?;
                    }
                    Some(())
                })?;
                Tag::Other
            }
            10 => {
                let mut out = Vec::new();
                self.nested(|p| {
                    loop {
                        let t = p.u8()?;
                        if t == 0 {
                            break;
                        }
                        let name = p.string()?;
                        let v = p.payload(t)?;
                        if v != Tag::Other {
                            out.push((name, v));
                        }
                    }
                    Some(())
                })?;
                Tag::Compound(out)
            }
            11 => {
                let n = self.len()?;
                self.take(n.checked_mul(4)?)?;
                Tag::Other
            }
            12 => {
                let n = self.len()?;
                self.take(n.checked_mul(8)?)?;
                Tag::Other
            }
            _ => return None,
        })
    }
    fn nested(&mut self, f: impl FnOnce(&mut Self) -> Option<()>) -> Option<()> {
        self.depth += 1;
        if self.depth > 64 {
            return None;
        }
        let r = f(self);
        self.depth -= 1;
        r
    }
}

fn read_level(path: &Path) -> Option<Tag> {
    let raw = std::fs::read(path).ok()?;
    let mut data = Vec::new();
    if raw.starts_with(&[0x1f, 0x8b]) {
        flate2::read::GzDecoder::new(&raw[..]).take(64 * 1024 * 1024).read_to_end(&mut data).ok()?;
    } else {
        data = raw;
    }
    let mut p = Nbt { b: &data, i: 0, depth: 0 };
    if p.u8()? != 10 {
        return None;
    }
    p.string()?;
    p.payload(10)
}

const GAME_MODES: [&str; 4] = ["生存", "创造", "冒险", "旁观"];

fn read_save(path: &Path, name: &str, meta: &std::fs::Metadata) -> Option<LocalFile> {
    let level = path.join("level.dat");
    if !level.is_file() {
        return None;
    }
    let mut f = LocalFile {
        name: name.to_string(),
        title: name.to_string(),
        dir: true,
        modified: mtime(meta),
        size: dir_size(path),
        icon: read_inner(path, true, "icon.png", MAX_ICON).map(|d| data_url(&d)),
        ..Default::default()
    };
    if let Some(data) = read_level(&level).as_ref().and_then(|t| t.get("Data")) {
        if let Some(n) = data.get("LevelName").and_then(Tag::str).filter(|s| !s.trim().is_empty()) {
            f.title = n.to_string();
        }
        if let Some(t) = data.get("LastPlayed").and_then(Tag::int).filter(|t| *t > 0) {
            f.modified = (t / 1000) as u64;
        }
        let hardcore = data.get("hardcore").and_then(Tag::int) == Some(1);
        let mode = data
            .get("GameType")
            .and_then(Tag::int)
            .and_then(|m| GAME_MODES.get(m as usize))
            .map(|m| if hardcore { "极限" } else { m });
        let version = data.get("Version").and_then(|v| v.get("Name")).and_then(Tag::str);
        let parts: Vec<String> = [mode.map(|m| format!("{m}模式")), version.map(|v| format!("Minecraft {v}"))]
            .into_iter()
            .flatten()
            .collect();
        if !parts.is_empty() {
            f.detail = Some(parts.join(" · "));
        }
    }
    Some(f)
}

fn read_pack(path: &Path, name: &str, meta: &std::fs::Metadata, enabled: &[String]) -> Option<LocalFile> {
    let dir = meta.is_dir();
    if !dir && !name.to_lowercase().ends_with(".zip") {
        return None;
    }
    let mcmeta = read_inner(path, dir, "pack.mcmeta", 1024 * 1024)?;
    let v: Value = serde_json::from_slice(&mcmeta)
        .or_else(|_| serde_json::from_str(String::from_utf8_lossy(&mcmeta).trim_start_matches('\u{feff}')))
        .unwrap_or(Value::Null);
    Some(LocalFile {
        name: name.to_string(),
        title: name.strip_suffix(".zip").or_else(|| name.strip_suffix(".ZIP")).unwrap_or(name).to_string(),
        description: v.get("pack").and_then(|p| p.get("description")).map(plain_text).unwrap_or_default(),
        icon: read_inner(path, dir, "pack.png", MAX_ICON).map(|d| data_url(&d)),
        size: if dir { dir_size(path) } else { meta.len() },
        modified: mtime(meta),
        dir,
        active: pack_enabled(enabled, name),
        detail: None,
    })
}

fn read_shader(path: &Path, name: &str, meta: &std::fs::Metadata, active: Option<&str>) -> Option<LocalFile> {
    let dir = meta.is_dir();
    // OptiFine 会在旁边保存「光影名.txt」作为设置
    if !dir && !name.to_lowercase().ends_with(".zip") {
        return None;
    }
    if dir && !path.join("shaders").is_dir() {
        return None;
    }
    Some(LocalFile {
        name: name.to_string(),
        title: name.strip_suffix(".zip").or_else(|| name.strip_suffix(".ZIP")).unwrap_or(name).to_string(),
        size: if dir { dir_size(path) } else { meta.len() },
        modified: mtime(meta),
        dir,
        active: active == Some(name),
        ..Default::default()
    })
}

/// 列出游戏目录中某类资源（不含 Mod，Mod 见 [`super::local`]）。按最近修改排序，存档按最后游玩时间。
pub fn scan(game_dir: &Path, kind: FileKind) -> Vec<LocalFile> {
    let folder = game_dir.join(kind.folder());
    let Ok(rd) = std::fs::read_dir(&folder) else { return Vec::new() };
    let enabled = if kind == FileKind::ResourcePack { enabled_packs(game_dir) } else { Vec::new() };
    let shader = if kind == FileKind::Shader { active_shader(game_dir) } else { None };
    let mut out: Vec<LocalFile> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let meta = std::fs::metadata(e.path()).ok()?;
            match kind {
                FileKind::ResourcePack => read_pack(&e.path(), &name, &meta, &enabled),
                FileKind::Shader => read_shader(&e.path(), &name, &meta, shader.as_deref()),
                FileKind::Save => meta.is_dir().then(|| read_save(&e.path(), &name, &meta)).flatten(),
                FileKind::Mod => None,
            }
        })
        .collect();
    out.sort_by(|a, b| b.active.cmp(&a.active).then(b.modified.cmp(&a.modified)));
    out
}

/// 检查名称只是文件夹中的一项（不含路径分隔符），返回完整路径。
pub fn entry_path(game_dir: &Path, kind: FileKind, name: &str) -> Result<PathBuf> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err(CoreError::msg(format!("无效的文件名：{name}"), ""));
    }
    Ok(game_dir.join(kind.folder()).join(name))
}

/// 目标位置已有同名项时加上「 (2)」等序号。
fn free_name(folder: &Path, name: &str) -> String {
    if !folder.join(name).exists() {
        return name.to_string();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (2..)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(|n| !folder.join(n).exists())
        .unwrap_or_else(|| name.to_string())
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).at(to)?;
    for e in std::fs::read_dir(from).at(from)?.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type().at(&src)?.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).at(&dst)?;
        }
    }
    Ok(())
}

/// 从 zip 中解压存档：以 level.dat 所在的目录为存档根目录。
fn extract_save(zip_path: &Path, saves: &Path) -> Result<String> {
    let file = std::fs::File::open(zip_path).at(zip_path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| CoreError::msg(format!("无法读取压缩包：{e}"), ""))?;
    let root = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().and_then(|e| e.enclosed_name()))
        .filter(|p| p.file_name().is_some_and(|n| n == "level.dat"))
        .map(|p| p.parent().map(Path::to_path_buf).unwrap_or_default())
        .min_by_key(|p| p.components().count())
        .ok_or_else(|| CoreError::msg("压缩包中没有找到存档（缺少 level.dat）", ""))?;
    let fallback = zip_path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let base = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(fallback);
    let name = free_name(saves, &base);
    let target = saves.join(&name);
    for i in 0..zip.len() {
        let mut e = zip.by_index(i).map_err(|e| CoreError::msg(e.to_string(), ""))?;
        let Some(rel) = e.enclosed_name().and_then(|p| p.strip_prefix(&root).ok().map(Path::to_path_buf)) else {
            continue;
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = target.join(rel);
        if e.is_dir() {
            std::fs::create_dir_all(&out).at(&out)?;
            continue;
        }
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p).at(p)?;
        }
        let mut f = std::fs::File::create(&out).at(&out)?;
        std::io::copy(&mut e, &mut f).at(&out)?;
    }
    Ok(name)
}

/// 导入文件或文件夹到对应的资源文件夹，返回导入后的名称。
pub fn import(game_dir: &Path, kind: FileKind, src: &Path) -> Result<String> {
    let folder = game_dir.join(kind.folder());
    std::fs::create_dir_all(&folder).at(&folder)?;
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| CoreError::msg("无效的文件", ""))?;
    let lower = name.to_lowercase();
    let is_dir = src.is_dir();
    match kind {
        FileKind::Mod if is_dir || !(lower.ends_with(".jar") || lower.ends_with(".jar.disabled")) => {
            return Err(CoreError::msg(format!("「{name}」不是 Mod 文件（.jar）"), ""));
        }
        FileKind::ResourcePack | FileKind::Shader if !is_dir && !lower.ends_with(".zip") => {
            return Err(CoreError::msg(format!("「{name}」不是 zip 压缩包或文件夹"), ""));
        }
        FileKind::Save if !is_dir => return extract_save(src, &folder),
        FileKind::Save if !src.join("level.dat").is_file() => {
            return Err(CoreError::msg(format!("「{name}」不是存档文件夹（缺少 level.dat）"), ""));
        }
        _ => {}
    }
    let target_name = free_name(&folder, &name);
    let target = folder.join(&target_name);
    if is_dir {
        copy_dir(src, &target)?;
    } else {
        std::fs::copy(src, &target).at(&target)?;
    }
    Ok(target_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn nbt_string(out: &mut Vec<u8>, s: &str) {
        out.extend((s.len() as u16).to_be_bytes());
        out.extend(s.as_bytes());
    }

    /// 生成一个最小的 level.dat（gzip 压缩的 NBT）。
    fn level_dat(name: &str, mode: i32) -> Vec<u8> {
        let mut b = vec![10];
        nbt_string(&mut b, "");
        b.push(10);
        nbt_string(&mut b, "Data");
        b.push(8);
        nbt_string(&mut b, "LevelName");
        nbt_string(&mut b, name);
        b.push(3);
        nbt_string(&mut b, "GameType");
        b.extend(mode.to_be_bytes());
        b.push(4);
        nbt_string(&mut b, "LastPlayed");
        b.extend(1_700_000_000_000i64.to_be_bytes());
        b.push(9);
        nbt_string(&mut b, "ServerBrands");
        b.push(8);
        b.extend(1i32.to_be_bytes());
        nbt_string(&mut b, "fabric");
        b.push(10);
        nbt_string(&mut b, "Version");
        b.push(8);
        nbt_string(&mut b, "Name");
        nbt_string(&mut b, "1.20.1");
        b.push(0);
        b.push(0);
        b.push(0);
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&b).unwrap();
        gz.finish().unwrap()
    }

    #[test]
    fn saves() {
        let t = tempfile::tempdir().unwrap();
        let w = t.path().join("saves/New World");
        std::fs::create_dir_all(&w).unwrap();
        std::fs::write(w.join("level.dat"), level_dat("我的世界", 1)).unwrap();
        std::fs::create_dir_all(t.path().join("saves/not a world")).unwrap();
        let list = scan(t.path(), FileKind::Save);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "New World");
        assert_eq!(list[0].title, "我的世界");
        assert_eq!(list[0].modified, 1_700_000_000);
        assert_eq!(list[0].detail.as_deref(), Some("创造模式 · Minecraft 1.20.1"));
    }

    #[test]
    fn resource_packs() {
        let t = tempfile::tempdir().unwrap();
        let rp = t.path().join("resourcepacks");
        std::fs::create_dir_all(rp.join("Folder Pack")).unwrap();
        std::fs::write(rp.join("Folder Pack/pack.mcmeta"), r#"{"pack":{"pack_format":15,"description":"§6Gold §rpack"}}"#).unwrap();
        let mut zip = zip::ZipWriter::new(std::fs::File::create(rp.join("Faithful.zip")).unwrap());
        zip.start_file("pack.mcmeta", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(br#"{"pack":{"description":[{"text":"32x "},{"text":"faithful"}]}}"#).unwrap();
        zip.finish().unwrap();
        std::fs::write(rp.join("readme.txt"), "x").unwrap();

        let list = scan(t.path(), FileKind::ResourcePack);
        assert_eq!(list.len(), 2);
        assert!(list.iter().all(|p| !p.active));
        let folder = list.iter().find(|p| p.dir).unwrap();
        assert_eq!(folder.description, "Gold pack");
        assert_eq!(list.iter().find(|p| !p.dir).unwrap().description, "32x faithful");

        std::fs::write(t.path().join("options.txt"), "lang:zh_cn\nresourcePacks:[\"vanilla\"]\nfov:0.0\n").unwrap();
        set_resource_pack(t.path(), "Faithful.zip", true, false).unwrap();
        let text = std::fs::read_to_string(t.path().join("options.txt")).unwrap();
        assert!(text.contains(r#"resourcePacks:["vanilla","file/Faithful.zip"]"#), "{text}");
        assert!(text.contains("lang:zh_cn") && text.contains("fov:0.0"));
        assert!(scan(t.path(), FileKind::ResourcePack).iter().find(|p| p.name == "Faithful.zip").unwrap().active);
        set_resource_pack(t.path(), "Faithful.zip", false, false).unwrap();
        assert!(std::fs::read_to_string(t.path().join("options.txt")).unwrap().contains(r#"resourcePacks:["vanilla"]"#));
        // 1.12.2 及更早
        set_resource_pack(t.path(), "Folder Pack", true, true).unwrap();
        assert!(std::fs::read_to_string(t.path().join("options.txt")).unwrap().contains(r#"resourcePacks:["vanilla","Folder Pack"]"#));
        assert!(legacy_pack_names("1.12.2") && legacy_pack_names("1.8.9") && !legacy_pack_names("1.20.1") && !legacy_pack_names("26.3"));
    }

    #[test]
    fn shaders() {
        let t = tempfile::tempdir().unwrap();
        let sp = t.path().join("shaderpacks");
        std::fs::create_dir_all(&sp).unwrap();
        std::fs::write(sp.join("BSL_v8.2.09.zip"), "x").unwrap();
        std::fs::write(sp.join("BSL_v8.2.09.zip.txt"), "x").unwrap();
        std::fs::write(sp.join("Complementary: A=B.zip"), "x").unwrap();
        assert_eq!(scan(t.path(), FileKind::Shader).len(), 2);
        assert!(set_shader(t.path(), Some("BSL_v8.2.09.zip"), false, false).is_err());

        set_shader(t.path(), Some("Complementary: A=B.zip"), true, false).unwrap();
        assert_eq!(active_shader(t.path()).as_deref(), Some("Complementary: A=B.zip"));
        set_shader(t.path(), None, true, true).unwrap();
        assert_eq!(active_shader(t.path()), None);
        set_shader(t.path(), Some("BSL_v8.2.09.zip"), false, false).unwrap();
        assert_eq!(std::fs::read_to_string(t.path().join("optionsshaders.txt")).unwrap(), "shaderPack=BSL_v8.2.09.zip\n");
        let list = scan(t.path(), FileKind::Shader);
        assert!(list[0].active && list[0].name == "BSL_v8.2.09.zip");
    }

    #[test]
    fn import_files() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src");
        std::fs::create_dir_all(src.join("pack/world/region")).unwrap();
        std::fs::write(src.join("pack/world/level.dat"), level_dat("W", 0)).unwrap();
        std::fs::write(src.join("pack/world/region/r.0.0.mca"), "r").unwrap();
        let zip_path = src.join("world.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        zip.start_file("pack/world/level.dat", o).unwrap();
        zip.write_all(&level_dat("W", 0)).unwrap();
        zip.start_file("pack/world/region/r.0.0.mca", o).unwrap();
        zip.write_all(b"r").unwrap();
        zip.start_file("pack/readme.txt", o).unwrap();
        zip.finish().unwrap();

        let game = t.path().join("game");
        assert_eq!(import(&game, FileKind::Save, &src.join("pack/world")).unwrap(), "world");
        assert_eq!(import(&game, FileKind::Save, &zip_path).unwrap(), "world (2)");
        assert!(game.join("saves/world (2)/region/r.0.0.mca").is_file());
        assert!(!game.join("saves/world (2)/readme.txt").exists());
        assert!(import(&game, FileKind::Save, &src.join("pack")).is_err());

        std::fs::write(src.join("a.jar"), "j").unwrap();
        assert_eq!(import(&game, FileKind::Mod, &src.join("a.jar")).unwrap(), "a.jar");
        assert_eq!(import(&game, FileKind::Mod, &src.join("a.jar")).unwrap(), "a (2).jar");
        assert!(import(&game, FileKind::ResourcePack, &src.join("a.jar")).is_err());
        assert!(entry_path(&game, FileKind::Save, "../x").is_err());
    }
}
