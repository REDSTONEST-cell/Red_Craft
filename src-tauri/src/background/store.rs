use super::model::{Asset, Library};
use crate::error::{AppError, AppResult};
use launcher_core::download::CancelToken;
use launcher_core::error::IoContext;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub fn inside(root: &Path, file: &Path) -> AppResult<PathBuf> {
    let root = std::fs::canonicalize(root).at(root)?;
    let file = std::fs::canonicalize(file).at(file)?;
    if !file.starts_with(&root) || !file.is_file() {
        return Err(AppError::msg(
            "壁纸文件超出项目范围或不是文件",
            "请重新选择完整的壁纸项目目录",
        ));
    }
    Ok(file)
}

pub fn decode_image(path: &Path) -> AppResult<()> {
    read_image(path).map(|_| ())
}

fn read_image(path: &Path) -> AppResult<image::DynamicImage> {
    let mut reader = image::ImageReader::open(path)
        .at(path)?
        .with_guessed_format()
        .at(path)?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(256 * 1024 * 1024);
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    reader.limits(limits);
    reader.decode().map_err(|e| {
        AppError::msg(
            format!("图片无法解码：{e}"),
            "请选择有效的 JPG、PNG 或 WebP 图片",
        )
    })
}

pub fn still_cover(config: &Path, id: &str, path: &Path) -> AppResult<PathBuf> {
    static_image(config, id, path, 1920, 1080)
}

pub fn thumbnail(config: &Path, id: &str, path: &Path) -> AppResult<PathBuf> {
    static_image(config, id, path, 480, 270)
}

fn static_image(
    config: &Path,
    id: &str,
    path: &Path,
    width: u32,
    height: u32,
) -> AppResult<PathBuf> {
    if uuid::Uuid::parse_str(id.strip_prefix("we-").unwrap_or(id)).is_err() {
        return Err(AppError::msg("素材 ID 无效", "请重新导入或扫描素材"));
    }
    let meta = std::fs::metadata(path).at(path)?;
    let stamp = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|t| t.as_nanos())
        .unwrap_or(0);
    let dir = config.join("backgrounds/covers");
    std::fs::create_dir_all(&dir).at(&dir)?;
    let dest = dir.join(format!("{id}-{width}x{height}-{stamp}-{}.png", meta.len()));
    if !dest.is_file() {
        // Decoding a single image and writing PNG makes APNG/WebP covers static
        // too; native <img> animation cannot bypass reduced-motion preferences.
        let image = read_image(path)?.thumbnail(width, height);
        let tmp = tempfile::NamedTempFile::new_in(&dir).at(&dir)?;
        image
            .save_with_format(tmp.path(), image::ImageFormat::Png)
            .map_err(|e| {
                AppError::msg(format!("无法生成静态封面：{e}"), "请检查背景目录写入权限")
            })?;
        tmp.persist(&dest)
            .map_err(|e| launcher_core::CoreError::Io {
                path: dest.clone(),
                source: e.error,
            })?;
    }
    std::fs::canonicalize(&dest).at(&dest).map_err(Into::into)
}

pub fn video(path: &Path) -> AppResult<()> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_lowercase();
    let mut header = [0u8; 32];
    let n = std::fs::File::open(path)
        .at(path)?
        .read(&mut header)
        .at(path)?;
    let valid = match ext.as_str() {
        "mp4" => n >= 12 && &header[4..8] == b"ftyp",
        "webm" => n >= 4 && header[..4] == [0x1a, 0x45, 0xdf, 0xa3],
        _ => false,
    };
    if !valid {
        return Err(AppError::msg(
            "不是有效的 MP4 / WebM 视频容器",
            "建议使用 H.264 MP4；是否可播放以 WebView2 实际解码为准",
        ));
    }
    Ok(())
}

pub fn load(config: &Path) -> Library {
    let path = config.join("backgrounds/library.json");
    let mut library: Library = crate::settings::read_json(&path).unwrap_or_default();
    library.appearance.normalize();
    // Upgrade old mixed-type scan caches without touching source wallpaper files.
    library
        .assets
        .retain(|a| a.source == "local" || a.is_user_we_video());
    if library
        .appearance
        .selection
        .as_ref()
        .is_some_and(|id| !library.assets.iter().any(|a| &a.id == id))
    {
        library.appearance.selection = None;
    }
    if library.scan_revision != super::model::SCAN_REVISION {
        library.scan_completed = false;
        library.scan_notes.clear();
    }
    library
}

pub fn save(config: &Path, library: &Library) -> AppResult<()> {
    let dir = config.join("backgrounds");
    std::fs::create_dir_all(&dir).at(&dir)?;
    let bytes = serde_json::to_vec_pretty(library)
        .map_err(|e| launcher_core::CoreError::json("背景素材库", e))?;
    let mut tmp = tempfile::NamedTempFile::new_in(&dir).at(&dir)?;
    tmp.write_all(&bytes).at(tmp.path())?;
    tmp.as_file().sync_all().at(tmp.path())?;
    let dest = dir.join("library.json");
    tmp.persist(&dest)
        .map_err(|e| launcher_core::CoreError::Io {
            path: dest,
            source: e.error,
        })?;
    Ok(())
}

pub fn import(config: &Path, source: &Path, cancel: &CancelToken) -> AppResult<Asset> {
    cancel.check()?;
    let ext = source
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_lowercase();
    let kind = match ext.as_str() {
        "jpg" | "jpeg" | "png" | "webp" => "image",
        "mp4" | "webm" => "video",
        _ => {
            return Err(AppError::msg(
                "不支持这个背景文件类型",
                "图片支持 JPG / PNG / WebP，视频支持 MP4 / WebM",
            ))
        }
    };
    let dir = config.join("backgrounds/media");
    std::fs::create_dir_all(&dir).at(&dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(&dir).at(&dir)?;
    let mut input = std::fs::File::open(source).at(source)?;
    let size = input.metadata().at(source)?.len();
    if kind == "image" && size > 64 * 1024 * 1024 {
        return Err(AppError::msg("图片超过 64 MiB", "请缩小图片后重新导入"));
    }
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        cancel.check()?;
        let n = input.read(&mut buffer).at(source)?;
        if n == 0 {
            break;
        }
        tmp.write_all(&buffer[..n]).at(tmp.path())?;
    }
    if kind == "image" {
        decode_image(tmp.path())?;
        let format = image::ImageReader::open(tmp.path())
            .at(tmp.path())?
            .with_guessed_format()
            .at(tmp.path())?
            .format();
        if !matches!(
            format,
            Some(image::ImageFormat::Jpeg | image::ImageFormat::Png | image::ImageFormat::WebP)
        ) {
            return Err(AppError::msg(
                "图片内容不是 JPG / PNG / WebP",
                "请使用受支持的图片格式",
            ));
        }
    } else {
        // A temporary path has no video extension, validate content through the
        // final container name after copying, before registering the asset.
        let mut h = [0u8; 12];
        let n = std::fs::File::open(tmp.path())
            .at(tmp.path())?
            .read(&mut h)
            .at(tmp.path())?;
        if !((ext == "mp4" && n >= 12 && &h[4..8] == b"ftyp")
            || (ext == "webm" && n >= 4 && h[..4] == [0x1a, 0x45, 0xdf, 0xa3]))
        {
            return Err(AppError::msg(
                "视频容器损坏或格式不符",
                "请选择可播放的 H.264 MP4 或 WebM",
            ));
        }
    }
    cancel.check()?;
    tmp.as_file().sync_all().at(tmp.path())?;
    let id = uuid::Uuid::new_v4().to_string();
    let path = dir.join(format!("{id}.{ext}"));
    tmp.persist_noclobber(&path)
        .map_err(|e| launcher_core::CoreError::Io {
            path: path.clone(),
            source: e.error,
        })?;
    let path = std::fs::canonicalize(&path).at(&path)?;
    Ok(Asset {
        scan_warning: None,
        id,
        title: source
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        source: "local".into(),
        kind: kind.into(),
        root: dir,
        media: Some(path.clone()),
        cover: (kind == "image").then_some(path),
        reason: None,
    })
}

pub fn remove_copy(config: &Path, asset: &Asset) -> AppResult<()> {
    if asset.source == "local" {
        if let Some(media) = &asset.media {
            if media.exists() {
                let path = inside(&config.join("backgrounds/media"), media)?;
                // Owned filenames are generated IDs; a modified record cannot
                // redirect removal to another imported asset or a personal file.
                if path.file_stem().and_then(|s| s.to_str()) != Some(&asset.id) {
                    return Err(AppError::msg("素材副本路径不匹配", "请重新导入素材"));
                }
                std::fs::remove_file(&path).at(&path)?;
            }
        }
    }
    Ok(())
}

/// 删除某个素材 id 生成的全部封面/缩略图缓存。
/// 缓存文件名固定为 `<id>-<宽>x<高>-<mtime>-<size>.png`，只匹配 `<id>-` 前缀，
/// 因此不会碰到其它素材或用户自己的文件。
pub fn remove_covers(config: &Path, id: &str) -> AppResult<()> {
    let dir = config.join("backgrounds/covers");
    if !dir.is_dir() {
        return Ok(());
    }
    let prefix = format!("{id}-");
    for entry in std::fs::read_dir(&dir).at(&dir)? {
        let entry = entry.at(&dir)?;
        if !entry.file_name().to_string_lossy().starts_with(&prefix) {
            continue;
        }
        if entry.file_type().at(&entry.path())?.is_file() {
            std::fs::remove_file(entry.path()).at(&entry.path())?;
        }
    }
    Ok(())
}
