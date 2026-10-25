use super::{
    model::{is_bundled_project, Asset},
    store,
};
use crate::error::{AppError, AppResult};
use launcher_core::download::CancelToken;
use launcher_core::error::IoContext;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// VDF quoted tokens, escapes and comments. Only discovery values are consumed;
// nested application IDs and sizes are never interpreted as filesystem paths.
fn tokens(input: &str) -> Vec<String> {
    let mut chars = input.trim_start_matches('\u{feff}').chars().peekable();
    let mut out = Vec::new();
    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '"' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    if c == '"' {
                        break;
                    }
                    if c == '\\' {
                        match chars.peek() {
                            Some('"' | '\\') => s.push(chars.next().unwrap()),
                            _ => s.push(c),
                        }
                    } else {
                        s.push(c);
                    }
                }
                out.push(s);
            }
            '{' | '}' => out.push(c.to_string()),
            c if c.is_whitespace() => {}
            _ => {
                let mut s = c.to_string();
                while chars
                    .peek()
                    .is_some_and(|c| !c.is_whitespace() && *c != '{' && *c != '}')
                {
                    s.push(chars.next().unwrap());
                }
                out.push(s);
            }
        }
    }
    out
}

pub fn library_paths(vdf: &str) -> Vec<PathBuf> {
    tokens(vdf)
        .windows(2)
        .filter(|p| {
            (p[0].eq_ignore_ascii_case("path") || p[0].parse::<u32>().is_ok())
                && (p[1].contains(':') || p[1].starts_with('/'))
        })
        .map(|p| PathBuf::from(&p[1]))
        .collect()
}

fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Registry::*;
        for (hive, key, value) in [
            (HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath"),
            (
                HKEY_LOCAL_MACHINE,
                r"SOFTWARE\WOW6432Node\Valve\Steam",
                "InstallPath",
            ),
        ] {
            let key: Vec<u16> = key.encode_utf16().chain(Some(0)).collect();
            let value: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
            let mut text = vec![0u16; 32768];
            let mut bytes = (text.len() * 2) as u32;
            // Read UTF-16 directly: reg.exe output follows the system code page
            // and would corrupt Steam paths containing Chinese characters.
            let status = unsafe {
                RegGetValueW(
                    hive,
                    key.as_ptr(),
                    value.as_ptr(),
                    RRF_RT_REG_SZ,
                    std::ptr::null_mut(),
                    text.as_mut_ptr().cast(),
                    &mut bytes,
                )
            };
            if status == 0 {
                let length = (bytes as usize / 2).min(text.len());
                let text = &text[..length];
                let end = text.iter().position(|c| *c == 0).unwrap_or(text.len());
                if end > 0 {
                    use std::os::windows::ffi::OsStringExt;
                    roots.push(PathBuf::from(std::ffi::OsString::from_wide(&text[..end])));
                }
            }
        }
        if let Some(base) = std::env::var_os("ProgramFiles(x86)") {
            roots.push(PathBuf::from(base).join("Steam"));
        }
    }
    #[cfg(not(windows))]
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".steam/steam"));
        roots.push(home.join("Library/Application Support/Steam"));
    }
    roots
}

fn key(path: &Path) -> String {
    let s = path.to_string_lossy().into_owned();
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

pub fn normalize_roots(roots: &[PathBuf]) -> AppResult<Vec<PathBuf>> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for path in roots {
        let path = std::fs::canonicalize(path).at(path)?;
        if !path.is_dir() {
            return Err(AppError::msg(
                "请选择目录",
                "可选择 Steam 库或 Wallpaper Engine 项目目录",
            ));
        }
        if seen.insert(key(&path)) {
            result.push(path);
        }
    }
    result.sort_by_key(|p| key(p));
    Ok(result)
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn media_kind(path: &Path) -> Option<&'static str> {
    match extension(path).as_str() {
        "mp4" | "webm" => Some("video"),
        "jpg" | "jpeg" | "png" | "webp" => Some("image"),
        _ => None,
    }
}

fn is_preview(path: &Path) -> bool {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    ["preview", "thumbnail", "poster"]
        .iter()
        .any(|prefix| stem == *prefix || stem.starts_with(&format!("{prefix}_")))
}

fn looks_like_project(root: &Path) -> bool {
    if !root.is_dir() {
        return false;
    }
    if root.join("project.json").is_file() {
        return true;
    }
    std::fs::read_dir(root).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry.path().is_file()
                && (extension(&entry.path()) == "pkg"
                    || (media_kind(&entry.path()).is_some() && !is_preview(&entry.path())))
        })
    })
}

pub fn project(root: &Path) -> AppResult<Asset> {
    let root = std::fs::canonicalize(root).at(root)?;
    let metadata = (|| -> AppResult<serde_json::Value> {
        let path = store::inside(&root, &root.join("project.json"))?;
        if std::fs::metadata(&path).at(&path)?.len() > 2 * 1024 * 1024 {
            return Err(AppError::msg("project.json 过大", "已按媒体扩展名识别"));
        }
        let bytes = std::fs::read(&path).at(&path)?;
        let value: serde_json::Value =
            serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))
                .map_err(|e| launcher_core::CoreError::json("Wallpaper 项目", e))?;
        if !value.is_object() {
            return Err(AppError::msg("project.json 不是对象", "已按媒体扩展名识别"));
        }
        Ok(value)
    })();
    let warning = metadata
        .as_ref()
        .err()
        .map(|e| format!("project.json 不可用，已按扩展名回退：{e}"));
    let value = metadata.unwrap_or_default();
    let mut files: Vec<_> = std::fs::read_dir(&root)
        .at(&root)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    let declared_type = value["type"]
        .as_str()
        .unwrap_or("unknown")
        .to_ascii_lowercase();
    let declared_file = value["file"].as_str();
    let scene = declared_type == "scene"
        || declared_file.is_some_and(|p| extension(Path::new(p)) == "pkg")
        || (!matches!(
            declared_type.as_str(),
            "image" | "video" | "web" | "application"
        ) && files.iter().any(|p| extension(p) == "pkg"));
    let kind = if scene {
        "scene"
    } else {
        declared_type.as_str()
    };
    let mut asset = Asset {
        id: format!(
            "we-{}",
            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, key(&root).as_bytes())
        ),
        title: value["title"]
            .as_str()
            .unwrap_or_else(|| {
                root.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("未命名壁纸")
            })
            .into(),
        source: "wallpaperEngine".into(),
        kind: kind.into(),
        root: root.clone(),
        media: None,
        cover: None,
        reason: None,
        scan_warning: warning,
    };
    // Scene 只用 preview.jpg；绝不把包或旁边的纹理当成媒体。
    let previews: Vec<_> = if scene {
        vec!["preview.jpg"]
    } else {
        value["preview"]
            .as_str()
            .into_iter()
            .chain(["preview.jpg", "preview.png", "preview.webp"])
            .collect()
    };
    asset.cover = previews.into_iter().find_map(|name| {
        store::inside(&root, &root.join(name))
            .ok()
            .filter(|p| media_kind(p) == Some("image"))
    });
    if scene {
        asset.reason = Some("场景壁纸：仅显示 preview.jpg，不解包或播放 .pkg".into());
        return Ok(asset);
    }
    if matches!(kind, "web" | "application") {
        asset.reason = Some(format!("{kind} 壁纸仅显示信息，不执行项目代码"));
        return Ok(asset);
    }

    let candidate = if let Some(name) = declared_file {
        // 显式越界路径不能被扩展名回退掩盖。
        match store::inside(&root, &root.join(name)) {
            Ok(path) => Some(path),
            Err(e) => {
                asset.reason = Some(e.to_string());
                return Ok(asset);
            }
        }
    } else {
        files
            .into_iter()
            .filter(|p| {
                !is_preview(p) && asset.cover.as_ref() != std::fs::canonicalize(p).ok().as_ref()
            })
            .filter(|p| {
                media_kind(p).is_some_and(|k| !matches!(kind, "image" | "video") || k == kind)
            })
            .find_map(|p| store::inside(&root, &p).ok())
    };
    if let Some(path) = candidate {
        match media_kind(&path) {
            Some(actual) if !matches!(kind, "image" | "video") || kind == actual => {
                asset.kind = actual.into();
                if actual == "video" {
                    if let Err(e) = store::video(&path) {
                        asset.reason = Some(e.to_string());
                        return Ok(asset);
                    }
                }
                if actual == "image" && asset.cover.is_none() {
                    asset.cover = Some(path.clone());
                }
                asset.media = Some(path);
            }
            _ => asset.reason = Some("项目主体不是匹配的 JPG/PNG/WebP/MP4/WebM 媒体".into()),
        }
    } else {
        asset.reason = Some("未发现可识别的媒体主体；封面不作为背景主体".into());
        if kind == "unknown" {
            return Err(AppError::msg(
                "未发现可识别的媒体主体",
                "project.json 不可用且目录内没有支持的媒体文件",
            ));
        }
    }
    Ok(asset)
}

pub fn scan(
    manual: &[PathBuf],
    cancel: &CancelToken,
    mut stage: impl FnMut(&str),
) -> AppResult<(Vec<Asset>, Vec<String>)> {
    let mut inputs = steam_roots();
    inputs.extend(manual.iter().cloned());
    scan_roots(inputs, cancel, &mut stage)
}

pub fn scan_roots(
    mut inputs: Vec<PathBuf>,
    cancel: &CancelToken,
    mut stage: impl FnMut(&str),
) -> AppResult<(Vec<Asset>, Vec<String>)> {
    // Expand libraryfolders once per Steam root, not across the entire disk.
    for root in inputs.clone() {
        if let Ok(vdf) = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
            inputs.extend(library_paths(&vdf));
        }
    }
    let mut candidates = Vec::new();
    let mut roots_seen = HashSet::new();
    let mut notes = Vec::new();
    for input in inputs {
        cancel.check()?;
        let Ok(root) = std::fs::canonicalize(&input) else {
            continue;
        };
        if !roots_seen.insert(key(&root)) {
            continue;
        }
        if is_bundled_project(&root) {
            continue;
        }
        if root.join("project.json").is_file()
            || (!root.join("steamapps").is_dir()
                && !root.join("projects").is_dir()
                && looks_like_project(&root))
        {
            candidates.push(root.clone());
            continue;
        }
        let mut containers = vec![
            root.clone(),
            root.join("steamapps/workshop/content/431960"),
            root.join("projects/myprojects"),
        ];
        let install_name = std::fs::read_to_string(root.join("steamapps/appmanifest_431960.acf"))
            .ok()
            .and_then(|s| {
                tokens(&s)
                    .windows(2)
                    .find(|p| p[0] == "installdir")
                    .map(|p| p[1].clone())
            })
            .unwrap_or_else(|| "wallpaper_engine".into());
        // Installation directory names are a single path component.
        if matches!(
            Path::new(&install_name).components().next(),
            Some(std::path::Component::Normal(_))
        ) && Path::new(&install_name).components().count() == 1
            && !install_name.contains(['/', '\\', ':'])
        {
            containers.push(
                root.join("steamapps/common")
                    .join(&install_name)
                    .join("projects/myprojects"),
            );
        }
        for container in containers {
            if let Ok(entries) = std::fs::read_dir(&container) {
                for entry in entries.flatten() {
                    cancel.check()?;
                    if looks_like_project(&entry.path()) {
                        candidates.push(entry.path());
                    }
                }
            }
        }
    }
    let mut assets = Vec::new();
    let mut seen = HashSet::new();
    for path in candidates {
        cancel.check()?;
        let Ok(path) = std::fs::canonicalize(&path) else {
            continue;
        };
        if is_bundled_project(&path) {
            continue;
        }
        if !seen.insert(key(&path)) {
            continue;
        }
        stage(&format!("扫描壁纸 {}", assets.len() + 1));
        match project(&path) {
            Ok(asset) => {
                if !asset.is_user_we_video() {
                    continue;
                }
                if let Some(warning) = &asset.scan_warning {
                    notes.push(format!("{}：{warning}", asset.title));
                }
                if let Some(reason) = &asset.reason {
                    notes.push(format!("{}：{reason}", asset.title));
                }
                assets.push(asset);
            }
            Err(e) => notes.push(format!("{}：{e}", path.display())),
        }
    }
    if assets.is_empty() {
        notes.push("未找到本地壁纸。可手动添加 Steam 库、项目根目录或单个项目目录。".into());
    }
    Ok((assets, notes))
}
