//! Wallpaper Engine 场景壁纸（scene）的静态渲染。
//!
//! 场景由 `scene.pkg`（或编辑器里未打包的目录）中的 `scene.json`、模型、材质与 `.tex` 纹理组成。
//! 启动器不运行着色器、粒子或脚本，只把场景里的图片图层按位置与缩放合成为一张静态背景；
//! 合成失败时回退到最大的一张纹理，再回退到项目预览图。
//!
//! 格式参考 RePKG（MIT）。所有长度与尺寸都有上限，损坏或恶意文件只会得到错误而不会耗尽内存。

use crate::error::{AppError, AppResult};
use image::{imageops, Rgba, RgbaImage};
use launcher_core::error::IoContext;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

const MAX_ENTRIES: usize = 100_000;
const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
const MAX_JSON_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PIXELS: u64 = 8192 * 8192;
const MAX_LAYERS: usize = 64;
/// 输出画布的最长边；更大的场景按比例缩小。
const MAX_CANVAS: u32 = 3840;

fn bad(message: impl Into<String>) -> AppError {
    AppError::msg(message.into(), "场景文件可能已损坏，已改用预览图")
}

/// 场景文件来源：`scene.pkg` 包，或编辑器中未打包的项目目录。
pub enum Source {
    Package { path: PathBuf, entries: HashMap<String, (u64, u64)> },
    Directory(PathBuf),
}

fn read_u32(reader: &mut impl Read) -> AppResult<u32> {
    let mut b = [0u8; 4];
    reader.read_exact(&mut b).map_err(|_| bad("场景包意外结束"))?;
    Ok(u32::from_le_bytes(b))
}

fn read_i32(reader: &mut impl Read) -> AppResult<i32> {
    read_u32(reader).map(|n| n as i32)
}

/// 包内路径统一使用 `/`，并按小写查找（Wallpaper Engine 在 Windows 上不区分大小写）。
fn normalize(name: &str) -> String {
    slashes(name).to_ascii_lowercase()
}

fn slashes(name: &str) -> String {
    name.replace('\\', "/").trim_start_matches("./").to_string()
}

impl Source {
    pub fn open(root: &Path, declared: Option<&str>) -> AppResult<Self> {
        let pkg = declared
            .filter(|f| f.to_ascii_lowercase().ends_with(".pkg"))
            .unwrap_or("scene.pkg");
        if let Ok(path) = super::store::inside(root, &root.join(pkg)) {
            return Self::package(&path);
        }
        if root.join("scene.json").is_file() {
            return Ok(Self::Directory(root.to_path_buf()));
        }
        Err(AppError::msg("找不到 scene.pkg 或 scene.json", "请确认壁纸已在 Steam 中下载完成"))
    }

    pub fn package(path: &Path) -> AppResult<Self> {
        let mut file = std::io::BufReader::new(std::fs::File::open(path).at(path)?);
        let total = std::fs::metadata(path).at(path)?.len();
        let magic_len = read_u32(&mut file)? as usize;
        if magic_len > 32 {
            return Err(bad("不是 Wallpaper Engine 场景包"));
        }
        let mut magic = vec![0u8; magic_len];
        file.read_exact(&mut magic).map_err(|_| bad("场景包意外结束"))?;
        if !magic.starts_with(b"PKGV") {
            return Err(bad("不是 Wallpaper Engine 场景包"));
        }
        let count = read_u32(&mut file)? as usize;
        if count > MAX_ENTRIES {
            return Err(bad("场景包条目数量异常"));
        }
        let mut raw = Vec::with_capacity(count.min(4096));
        for _ in 0..count {
            let len = read_u32(&mut file)? as usize;
            if len > 255 {
                return Err(bad("场景包文件名过长"));
            }
            let mut name = vec![0u8; len];
            file.read_exact(&mut name).map_err(|_| bad("场景包意外结束"))?;
            let offset = read_u32(&mut file)? as u64;
            let length = read_u32(&mut file)? as u64;
            raw.push((String::from_utf8_lossy(&name).into_owned(), offset, length));
        }
        let data_start = file.stream_position().at(path)?;
        let mut entries = HashMap::new();
        for (name, offset, length) in raw {
            if data_start + offset + length <= total {
                entries.insert(normalize(&name), (data_start + offset, length));
            }
        }
        Ok(Self::Package { path: path.to_path_buf(), entries })
    }

    pub fn names(&self) -> Vec<String> {
        match self {
            Self::Package { entries, .. } => entries.keys().cloned().collect(),
            Self::Directory(root) => {
                let mut out = Vec::new();
                let mut stack = vec![(root.clone(), 0)];
                while let Some((dir, depth)) = stack.pop() {
                    let Ok(list) = std::fs::read_dir(&dir) else { continue };
                    for entry in list.flatten() {
                        let path = entry.path();
                        if path.is_dir() && depth < 6 {
                            stack.push((path, depth + 1));
                        } else if let Ok(rel) = path.strip_prefix(root) {
                            out.push(slashes(&rel.to_string_lossy()));
                        }
                        if out.len() > MAX_ENTRIES {
                            return out;
                        }
                    }
                }
                out
            }
        }
    }

    pub fn read(&self, name: &str, limit: u64) -> AppResult<Vec<u8>> {
        match self {
            Self::Package { path, entries } => {
                let name = normalize(name);
                let (offset, length) = *entries
                    .get(&name)
                    .ok_or_else(|| bad(format!("场景包中没有 {name}")))?;
                if length > limit {
                    return Err(bad(format!("{name} 过大")));
                }
                let mut file = std::fs::File::open(path).at(path)?;
                file.seek(SeekFrom::Start(offset)).at(path)?;
                let mut out = vec![0u8; length as usize];
                file.read_exact(&mut out).at(path)?;
                Ok(out)
            }
            Self::Directory(root) => {
                let name = slashes(name);
                let rel = Path::new(&name);
                if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
                    return Err(bad("场景文件路径越界"));
                }
                let path = super::store::inside(root, &root.join(rel))?;
                let size = std::fs::metadata(&path).at(&path)?.len();
                if size > limit {
                    return Err(bad(format!("{name} 过大")));
                }
                Ok(std::fs::read(&path).at(&path)?)
            }
        }
    }

    fn json(&self, name: &str) -> AppResult<Value> {
        let bytes = self.read(name, MAX_JSON_BYTES)?;
        serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))
            .map_err(|e| bad(format!("{name} 不是有效的 JSON：{e}")))
    }
}

// ───────────── .tex 纹理 ─────────────

fn read_cstr(reader: &mut impl Read, max: usize) -> AppResult<String> {
    let mut out = Vec::new();
    loop {
        let mut b = [0u8; 1];
        reader.read_exact(&mut b).map_err(|_| bad("纹理意外结束"))?;
        if b[0] == 0 {
            break;
        }
        if out.len() >= max {
            return Err(bad("纹理头部异常"));
        }
        out.push(b[0]);
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// 只读取纹理头部得到图片实际尺寸，用于挑选图层，不解码像素。
pub fn tex_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut r = std::io::Cursor::new(bytes);
    if read_cstr(&mut r, 16).ok()? != "TEXV0005" || read_cstr(&mut r, 16).ok()? != "TEXI0001" {
        return None;
    }
    let mut v = [0i32; 6];
    for n in &mut v {
        *n = read_i32(&mut r).ok()?;
    }
    let (w, h) = (v[4], v[5]);
    (w > 0 && h > 0).then_some((w as u32, h as u32))
}

/// 解码 `.tex` 的第一张图片的最大一级 mipmap，裁剪到图片实际尺寸。
pub fn decode_tex(bytes: &[u8]) -> AppResult<RgbaImage> {
    let mut r = std::io::Cursor::new(bytes);
    if read_cstr(&mut r, 16)? != "TEXV0005" || read_cstr(&mut r, 16)? != "TEXI0001" {
        return Err(bad("不是 Wallpaper Engine 纹理"));
    }
    let format = read_i32(&mut r)?;
    let _flags = read_i32(&mut r)?;
    let _texture_w = read_i32(&mut r)?;
    let _texture_h = read_i32(&mut r)?;
    let image_w = read_i32(&mut r)?;
    let image_h = read_i32(&mut r)?;
    let _unknown = read_u32(&mut r)?;
    let container = read_cstr(&mut r, 16)?;
    let image_count = read_i32(&mut r)?;
    if !(1..=100).contains(&image_count) {
        return Err(bad("纹理不含图片"));
    }
    let mut version = match container.as_str() {
        "TEXB0001" => 1,
        "TEXB0002" => 2,
        "TEXB0003" => 3,
        "TEXB0004" => 4,
        _ => return Err(bad(format!("不支持的纹理容器 {container}"))),
    };
    let mut free_image = -1;
    if version == 3 {
        free_image = read_i32(&mut r)?;
    } else if version == 4 {
        free_image = read_i32(&mut r)?;
        let mp4 = read_i32(&mut r)? == 1;
        if free_image == -1 && mp4 {
            return Err(bad("视频纹理不能作为静态画面"));
        }
        version = 3;
    }
    let mipmaps = read_i32(&mut r)?;
    if !(1..=32).contains(&mipmaps) {
        return Err(bad("纹理 mipmap 数量异常"));
    }
    // 第一级 mipmap 是原始尺寸。
    let width = read_i32(&mut r)?;
    let height = read_i32(&mut r)?;
    let (compressed, decompressed) = if version >= 2 {
        (read_i32(&mut r)? == 1, read_i32(&mut r)?)
    } else {
        (false, 0)
    };
    let count = read_i32(&mut r)?;
    if width <= 0 || height <= 0 || count < 0 || (width as u64) * (height as u64) > MAX_PIXELS {
        return Err(bad("纹理尺寸异常"));
    }
    let start = r.position() as usize;
    let end = start.checked_add(count as usize).filter(|e| *e <= bytes.len()).ok_or_else(|| bad("纹理数据不完整"))?;
    let mut data = bytes[start..end].to_vec();
    if compressed {
        if decompressed <= 0 || decompressed as u64 > MAX_ENTRY_BYTES {
            return Err(bad("纹理解压尺寸异常"));
        }
        data = lz4_flex::block::decompress(&data, decompressed as usize)
            .map_err(|e| bad(format!("纹理解压失败：{e}")))?;
    }
    let (width, height) = (width as u32, height as u32);
    let mut image = if free_image != -1 {
        // 纹理里直接嵌入了 PNG / JPEG 等图片文件。
        let mut reader = image::ImageReader::new(std::io::Cursor::new(&data))
            .with_guessed_format()
            .map_err(|e| bad(format!("纹理图片无法识别：{e}")))?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(512 * 1024 * 1024);
        reader.limits(limits);
        reader.decode().map_err(|e| bad(format!("纹理图片无法解码：{e}")))?.to_rgba8()
    } else {
        let pixels = (width * height) as usize;
        let rgba = match format {
            0 => data.get(..pixels * 4).map(<[u8]>::to_vec),
            8 => data.get(..pixels * 2).map(|d| (0..pixels).flat_map(|i| [d[2 * i], d[2 * i], d[2 * i], d[2 * i + 1]]).collect()),
            9 => data.get(..pixels).map(|d| d.iter().flat_map(|&p| [p, p, p, 255]).collect()),
            7 => Some(decode_bc(&data, width, height, Bc::Bc1)?),
            6 => Some(decode_bc(&data, width, height, Bc::Bc2)?),
            4 => Some(decode_bc(&data, width, height, Bc::Bc3)?),
            _ => return Err(bad(format!("不支持的纹理格式 {format}"))),
        }
        .ok_or_else(|| bad("纹理数据不完整"))?;
        RgbaImage::from_raw(width, height, rgba).ok_or_else(|| bad("纹理数据不完整"))?
    };
    // 纹理通常被补齐到 2 的幂，图片实际内容在左上角。
    if image_w > 0 && image_h > 0 && ((image_w as u32) < image.width() || (image_h as u32) < image.height()) {
        image = imageops::crop_imm(&image, 0, 0, (image_w as u32).min(image.width()), (image_h as u32).min(image.height())).to_image();
    }
    Ok(image)
}

#[derive(Clone, Copy, PartialEq)]
enum Bc {
    Bc1,
    Bc2,
    Bc3,
}

fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 31) as u32;
    let g = ((c >> 5) & 63) as u32;
    let b = (c & 31) as u32;
    [((r * 527 + 23) >> 6) as u8, ((g * 259 + 33) >> 6) as u8, ((b * 527 + 23) >> 6) as u8]
}

/// BC1 / BC2 / BC3（DXT1 / DXT3 / DXT5）块解码。
fn decode_bc(data: &[u8], width: u32, height: u32, kind: Bc) -> AppResult<Vec<u8>> {
    let block = if kind == Bc::Bc1 { 8 } else { 16 };
    let bw = width.div_ceil(4) as usize;
    let bh = height.div_ceil(4) as usize;
    if data.len() < bw * bh * block {
        return Err(bad("压缩纹理数据不完整"));
    }
    let mut out = vec![0u8; (width * height * 4) as usize];
    for by in 0..bh {
        for bx in 0..bw {
            let b = &data[(by * bw + bx) * block..][..block];
            let (alpha_part, color) = if kind == Bc::Bc1 { (&b[..0], &b[..8]) } else { (&b[..8], &b[8..16]) };
            let c0 = u16::from_le_bytes([color[0], color[1]]);
            let c1 = u16::from_le_bytes([color[2], color[3]]);
            let (p0, p1) = (rgb565(c0), rgb565(c1));
            let mut palette = [[0u8; 4]; 4];
            palette[0] = [p0[0], p0[1], p0[2], 255];
            palette[1] = [p1[0], p1[1], p1[2], 255];
            if kind != Bc::Bc1 || c0 > c1 {
                for i in 0..3 {
                    palette[2][i] = ((2 * p0[i] as u32 + p1[i] as u32) / 3) as u8;
                    palette[3][i] = ((p0[i] as u32 + 2 * p1[i] as u32) / 3) as u8;
                }
                palette[2][3] = 255;
                palette[3][3] = 255;
            } else {
                for i in 0..3 {
                    palette[2][i] = ((p0[i] as u32 + p1[i] as u32) / 2) as u8;
                }
                palette[2][3] = 255;
                palette[3] = [0, 0, 0, 0];
            }
            let indices = u32::from_le_bytes([color[4], color[5], color[6], color[7]]);
            let mut alphas = [255u8; 16];
            match kind {
                Bc::Bc1 => {}
                Bc::Bc2 => {
                    for (i, a) in alphas.iter_mut().enumerate() {
                        let nibble = (alpha_part[i / 2] >> ((i % 2) * 4)) & 15;
                        *a = nibble * 17;
                    }
                }
                Bc::Bc3 => {
                    let (a0, a1) = (alpha_part[0] as u32, alpha_part[1] as u32);
                    let mut table = [0u8; 8];
                    table[0] = a0 as u8;
                    table[1] = a1 as u8;
                    if a0 > a1 {
                        for i in 1..7 {
                            table[i + 1] = (((7 - i as u32) * a0 + i as u32 * a1) / 7) as u8;
                        }
                    } else {
                        for i in 1..5 {
                            table[i + 1] = (((5 - i as u32) * a0 + i as u32 * a1) / 5) as u8;
                        }
                        table[6] = 0;
                        table[7] = 255;
                    }
                    let mut bits = 0u64;
                    for (i, byte) in alpha_part[2..8].iter().enumerate() {
                        bits |= (*byte as u64) << (8 * i);
                    }
                    for (i, a) in alphas.iter_mut().enumerate() {
                        *a = table[((bits >> (3 * i)) & 7) as usize];
                    }
                }
            }
            for py in 0..4 {
                for px in 0..4 {
                    let (x, y) = (bx as u32 * 4 + px, by as u32 * 4 + py);
                    if x >= width || y >= height {
                        continue;
                    }
                    let i = (py * 4 + px) as usize;
                    let mut pixel = palette[((indices >> (2 * i)) & 3) as usize];
                    if kind != Bc::Bc1 {
                        pixel[3] = alphas[i];
                    }
                    let o = ((y * width + x) * 4) as usize;
                    out[o..o + 4].copy_from_slice(&pixel);
                }
            }
        }
    }
    Ok(out)
}

// ───────────── 场景合成 ─────────────

/// 读取可能被包成 `{"value": …}` 的用户可调属性。
fn value(v: &Value) -> &Value {
    match v.get("value") {
        Some(inner) if v.is_object() => inner,
        _ => v,
    }
}

fn floats(v: Option<&Value>) -> Vec<f64> {
    let Some(v) = v.map(value) else { return Vec::new() };
    match v {
        Value::String(s) => s.split_whitespace().filter_map(|n| n.parse().ok()).collect(),
        Value::Number(n) => n.as_f64().into_iter().collect(),
        Value::Array(a) => a.iter().filter_map(Value::as_f64).collect(),
        _ => Vec::new(),
    }
}

fn visible(object: &Value) -> bool {
    object.get("visible").map(value).and_then(Value::as_bool).unwrap_or(true)
}

/// 图片图层：模型 → 材质 → 第一个纹理。内置纹理（util/…）不在包里，直接跳过。
fn layer_texture(source: &Source, model: &str) -> Option<String> {
    let model = source.json(model).ok()?;
    let material = source.json(model.get("material")?.as_str()?).ok()?;
    let texture = material.get("passes")?.as_array()?.first()?.get("textures")?.as_array()?.first()?.as_str()?;
    let name = format!("materials/{}.tex", texture.trim_end_matches(".tex"));
    let exists = match source {
        Source::Package { entries, .. } => entries.contains_key(&normalize(&name)),
        Source::Directory(root) => root.join(slashes(&name)).is_file(),
    };
    exists.then_some(name)
}

fn clear_color(scene: &Value) -> Rgba<u8> {
    let c = floats(scene.pointer("/general/clearcolor"));
    let channel = |i: usize| (c.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgba([channel(0), channel(1), channel(2), 255])
}

/// 把场景渲染成一张静态图片。
pub fn render(source: &Source) -> AppResult<RgbaImage> {
    let scene = source.json("scene.json")?;
    let projection = scene.pointer("/general/orthogonalprojection");
    let (mut sw, mut sh) = (
        projection.and_then(|p| p.get("width")).and_then(Value::as_f64).unwrap_or(0.0),
        projection.and_then(|p| p.get("height")).and_then(Value::as_f64).unwrap_or(0.0),
    );
    let mut layers = Vec::new();
    for object in scene.get("objects").and_then(Value::as_array).into_iter().flatten() {
        if layers.len() >= MAX_LAYERS {
            break;
        }
        let Some(model) = object.get("image").and_then(Value::as_str) else { continue };
        if !visible(object) {
            continue;
        }
        if let Some(texture) = layer_texture(source, model) {
            layers.push((object, texture));
        }
    }
    let mut textures: HashMap<String, RgbaImage> = HashMap::new();
    let mut decoded = Vec::new();
    for (object, name) in &layers {
        if !textures.contains_key(name) {
            match source.read(name, MAX_ENTRY_BYTES).and_then(|b| decode_tex(&b)) {
                Ok(image) => {
                    textures.insert(name.clone(), image);
                }
                Err(e) => {
                    log::debug!("跳过场景图层 {name}：{e}");
                    continue;
                }
            }
        }
        decoded.push((*object, name.clone()));
    }
    if decoded.is_empty() {
        return largest_texture(source);
    }
    if !(sw > 0.0 && sh > 0.0) {
        // 没有正交投影信息：以第一个图层的尺寸作为画布。
        let first = &textures[&decoded[0].1];
        sw = first.width() as f64;
        sh = first.height() as f64;
    }
    let ratio = (MAX_CANVAS as f64 / sw.max(sh)).min(1.0);
    let (cw, ch) = ((sw * ratio).round().max(1.0) as u32, (sh * ratio).round().max(1.0) as u32);
    let mut canvas = RgbaImage::from_pixel(cw, ch, clear_color(&scene));
    let mut drawn = 0;
    for (object, name) in decoded {
        let texture = &textures[&name];
        let size = floats(object.get("size"));
        let scale = floats(object.get("scale"));
        let origin = floats(object.get("origin"));
        let (mut w, mut h) = match size.as_slice() {
            [w, h, ..] if *w > 0.0 && *h > 0.0 => (*w, *h),
            _ => (texture.width() as f64, texture.height() as f64),
        };
        if let [x, y, ..] = scale.as_slice() {
            w *= x.abs();
            h *= y.abs();
        }
        let (cx, cy) = match origin.as_slice() {
            [x, y, ..] => (*x, *y),
            _ => (sw / 2.0, sh / 2.0),
        };
        let (w, h) = (w * ratio, h * ratio);
        if !(w >= 1.0 && h >= 1.0) || w > 4.0 * MAX_CANVAS as f64 || h > 4.0 * MAX_CANVAS as f64 {
            continue;
        }
        // 场景坐标原点在左下角，y 轴向上。
        let left = (cx * ratio - w / 2.0).round() as i64;
        let top = (ch as f64 - cy * ratio - h / 2.0).round() as i64;
        let mut layer = imageops::resize(texture, w.round() as u32, h.round() as u32, imageops::FilterType::Triangle);
        let alpha = object.get("alpha").map(value).and_then(Value::as_f64).unwrap_or(1.0).clamp(0.0, 1.0);
        if alpha < 1.0 {
            for p in layer.pixels_mut() {
                p[3] = (p[3] as f64 * alpha).round() as u8;
            }
        }
        imageops::overlay(&mut canvas, &layer, left, top);
        drawn += 1;
    }
    if drawn == 0 {
        return largest_texture(source);
    }
    Ok(canvas)
}

/// 合成失败时：挑选包里像素最多的一张纹理（通常就是背景图）。
fn largest_texture(source: &Source) -> AppResult<RgbaImage> {
    let mut best: Option<(u64, String)> = None;
    for name in source.names() {
        if !name.ends_with(".tex") || name.contains("mask") {
            continue;
        }
        let Ok(bytes) = source.read(&name, MAX_ENTRY_BYTES) else { continue };
        let Some((w, h)) = tex_size(&bytes) else { continue };
        let area = w as u64 * h as u64;
        if best.as_ref().is_none_or(|(a, _)| area > *a) {
            best = Some((area, name));
        }
    }
    let (_, name) = best.ok_or_else(|| bad("场景中没有可显示的图片图层"))?;
    decode_tex(&source.read(&name, MAX_ENTRY_BYTES)?)
}

/// 渲染并缓存场景静态画面，返回缓存的 PNG。缓存按场景文件的大小与修改时间区分。
pub fn rendered(config: &Path, id: &str, root: &Path, declared: Option<&str>) -> AppResult<PathBuf> {
    let source = Source::open(root, declared)?;
    let stamp_path = match &source {
        Source::Package { path, .. } => path.clone(),
        Source::Directory(root) => root.join("scene.json"),
    };
    let meta = std::fs::metadata(&stamp_path).at(&stamp_path)?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|t| t.as_nanos())
        .unwrap_or(0);
    let dir = config.join("backgrounds/scenes");
    std::fs::create_dir_all(&dir).at(&dir)?;
    let dest = dir.join(format!("{id}-{modified}-{}.png", meta.len()));
    if !dest.is_file() {
        let image = render(&source)?;
        let tmp = tempfile::NamedTempFile::new_in(&dir).at(&dir)?;
        image
            .save_with_format(tmp.path(), image::ImageFormat::Png)
            .map_err(|e| AppError::msg(format!("无法保存场景画面：{e}"), "请检查背景目录写入权限"))?;
        tmp.persist(&dest).map_err(|e| launcher_core::CoreError::Io { path: dest.clone(), source: e.error })?;
        cleanup(config, id, Some(&dest));
    }
    std::fs::canonicalize(&dest).at(&dest).map_err(Into::into)
}

/// 删除某个素材的场景渲染缓存（可保留一个当前文件）。
pub fn cleanup(config: &Path, id: &str, keep: Option<&Path>) {
    let dir = config.join("backgrounds/scenes");
    let Ok(list) = std::fs::read_dir(&dir) else { return };
    let prefix = format!("{id}-");
    for entry in list.flatten() {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with(&prefix) && Some(path.as_path()) != keep && path.is_file() {
            let _ = std::fs::remove_file(&path);
        }
    }
}
