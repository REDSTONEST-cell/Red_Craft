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
/// 渲染规则版本：修改合成规则后加一，旧的缓存图片不再使用。
const RENDERER: u32 = 2;
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

fn read_f32(reader: &mut impl Read) -> AppResult<f32> {
    read_u32(reader).map(f32::from_bits)
}

/// 一级 mipmap 的位置与解压信息。
struct Mip {
    width: u32,
    height: u32,
    start: usize,
    len: usize,
    lz4: Option<usize>,
}

/// 解码 `.tex`：取第一张图片的最大一级 mipmap，裁剪到图片实际尺寸；
/// 动图纹理（精灵图）只取第一帧。格式与 RePKG 的 TexReader 一致。
pub fn decode_tex(bytes: &[u8]) -> AppResult<RgbaImage> {
    let mut r = std::io::Cursor::new(bytes);
    if read_cstr(&mut r, 16)? != "TEXV0005" || read_cstr(&mut r, 16)? != "TEXI0001" {
        return Err(bad("不是 Wallpaper Engine 纹理"));
    }
    let format = read_i32(&mut r)?;
    let flags = read_i32(&mut r)?;
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
    if version >= 3 {
        free_image = read_i32(&mut r)?;
    }
    if version == 4 {
        let mp4 = read_i32(&mut r)? == 1;
        if free_image == -1 && mp4 {
            return Err(bad("视频纹理不能作为静态画面"));
        }
        version = 3;
    }
    // 读完所有图片的 mipmap 头部，才能找到动图纹理末尾的帧信息。
    let mut images: Vec<Mip> = Vec::new();
    for _ in 0..image_count {
        let mipmaps = read_i32(&mut r)?;
        if !(1..=32).contains(&mipmaps) {
            return Err(bad("纹理 mipmap 数量异常"));
        }
        for level in 0..mipmaps {
            let width = read_i32(&mut r)?;
            let height = read_i32(&mut r)?;
            let lz4 = if version >= 2 {
                let compressed = read_i32(&mut r)? == 1;
                let size = read_i32(&mut r)?;
                compressed.then_some(size)
            } else {
                None
            };
            let len = read_i32(&mut r)?;
            if width <= 0 || height <= 0 || len < 0 || (width as u64) * (height as u64) > MAX_PIXELS {
                return Err(bad("纹理尺寸异常"));
            }
            let start = r.position() as usize;
            let end = start.checked_add(len as usize).filter(|e| *e <= bytes.len()).ok_or_else(|| bad("纹理数据不完整"))?;
            r.set_position(end as u64);
            if level == 0 {
                if lz4.is_some_and(|n| n <= 0 || n as u64 > MAX_ENTRY_BYTES) {
                    return Err(bad("纹理解压尺寸异常"));
                }
                images.push(Mip { width: width as u32, height: height as u32, start, len: len as usize, lz4: lz4.map(|n| n as usize) });
            }
        }
    }
    // 动图纹理：多张图片拼成精灵图，帧信息说明第一帧在哪张图片的哪个位置。
    let mut frame: Option<(usize, f32, f32, f32, f32)> = None;
    if flags & 4 != 0 {
        if let Ok(magic) = read_cstr(&mut r, 16) {
            let count = read_i32(&mut r).unwrap_or(0);
            if count > 0 && matches!(magic.as_str(), "TEXS0001" | "TEXS0002" | "TEXS0003") {
                if magic == "TEXS0003" {
                    let _ = (read_i32(&mut r), read_i32(&mut r));
                }
                let id = read_i32(&mut r).unwrap_or(0).max(0) as usize;
                let _frametime = read_f32(&mut r);
                let mut v = [0f32; 6];
                for n in &mut v {
                    *n = if magic == "TEXS0001" { read_i32(&mut r).map(|i| i as f32) } else { read_f32(&mut r) }.unwrap_or(0.0);
                }
                let [x, y, width, width_y, height_x, height] = v;
                let w = if width != 0.0 { width } else { height_x };
                let h = if height != 0.0 { height } else { width_y };
                frame = Some((id, x.min(x + w), y.min(y + h), w.abs(), h.abs()));
            }
        }
    }
    let index = frame.map(|f| f.0).filter(|i| *i < images.len()).unwrap_or(0);
    let mip = &images[index];
    let mut data = bytes[mip.start..mip.start + mip.len].to_vec();
    if let Some(size) = mip.lz4 {
        data = lz4_flex::block::decompress(&data, size).map_err(|e| bad(format!("纹理解压失败：{e}")))?;
    }
    let (width, height) = (mip.width, mip.height);
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
    if let Some((_, x, y, w, h)) = frame {
        let (x, y) = (x.max(0.0) as u32, y.max(0.0) as u32);
        let (w, h) = (w as u32, h as u32);
        if w > 0 && h > 0 && x < image.width() && y < image.height() {
            let (w, h) = (w.min(image.width() - x), h.min(image.height() - y));
            return Ok(imageops::crop_imm(&image, x, y, w, h).to_image());
        }
    }
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
//
// 规则参照 linux-wallpaperengine（Almamu/linux-wallpaperengine）的 ObjectParser / CImage：
// - 属性可以是 `{"user": 名称, "value": 默认值}`，实际值取 project.json 的 general.properties；
// - 图层位置沿父对象链累加（位置按父缩放与旋转变换，缩放相乘，角度相加，单位弧度）；
// - 尺寸依次取对象 size、纹理实际尺寸、模型 width/height；fullscreen 模型铺满画布；
// - 纹理取材质第一个 pass 的第一个纹理，对象的 instance.textures 可以覆盖它；
// - 颜色、亮度、不透明度相乘；混合模式 translucent / additive / normal。
// 粒子、文字、着色器特效与 3D 模型不渲染。

/// 读取属性：用户可调属性取 project.json 中的当前值，否则取默认值。
fn prop<'a>(v: &'a Value, props: &'a Value) -> std::borrow::Cow<'a, Value> {
    use std::borrow::Cow;
    let Some(obj) = v.as_object() else { return Cow::Borrowed(v) };
    if let Some(user) = obj.get("user") {
        let (name, condition) = match user {
            Value::String(n) => (Some(n.as_str()), None),
            Value::Object(o) => (o.get("name").and_then(Value::as_str), o.get("condition")),
            _ => (None, None),
        };
        if let Some(current) = name.and_then(|n| props.get(n)).and_then(|p| p.get("value")) {
            if let Some(condition) = condition {
                let same = match (condition, current) {
                    (Value::String(a), b) => b.as_str().map(|b| b == a).unwrap_or_else(|| &b.to_string() == a),
                    (a, b) => a == b,
                };
                return Cow::Owned(Value::Bool(same));
            }
            return Cow::Borrowed(current);
        }
    }
    match obj.get("value") {
        Some(inner) => Cow::Borrowed(inner),
        None => Cow::Borrowed(v),
    }
}

fn floats(v: Option<&Value>, props: &Value) -> Vec<f64> {
    let Some(v) = v else { return Vec::new() };
    match prop(v, props).as_ref() {
        Value::String(s) => s.split_whitespace().filter_map(|n| n.parse().ok()).collect(),
        Value::Number(n) => n.as_f64().into_iter().collect(),
        Value::Array(a) => a.iter().filter_map(Value::as_f64).collect(),
        _ => Vec::new(),
    }
}

fn vec3(v: Option<&Value>, props: &Value, default: f64) -> [f64; 3] {
    let f = floats(v, props);
    match f.as_slice() {
        [x] => [*x, *x, *x],
        _ => [0, 1, 2].map(|i| f.get(i).copied().filter(|n| n.is_finite()).unwrap_or(default)),
    }
}

fn number(v: Option<&Value>, props: &Value, default: f64) -> f64 {
    floats(v, props).first().copied().filter(|n| n.is_finite()).unwrap_or(default)
}

fn flag(v: Option<&Value>, props: &Value, default: bool) -> bool {
    match v.map(|v| prop(v, props)).as_deref() {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(s)) => !matches!(s.as_str(), "false" | "0" | ""),
        _ => default,
    }
}

#[derive(Clone, Copy)]
struct Transform {
    origin: [f64; 3],
    scale: [f64; 3],
    angle: f64,
}

fn local_transform(object: &Value, props: &Value) -> Transform {
    Transform {
        origin: vec3(object.get("origin"), props, 0.0),
        scale: vec3(object.get("scale"), props, 1.0),
        angle: vec3(object.get("angles"), props, 0.0)[2],
    }
}

/// 沿父对象链得到世界变换；父对象不可见时整组隐藏。
fn resolve(object: &Value, by_id: &HashMap<i64, &Value>, props: &Value) -> Option<Transform> {
    let mut chain = vec![object];
    let mut current = object;
    while let Some(parent) = current.get("parent").and_then(Value::as_i64) {
        let Some(next) = by_id.get(&parent) else { break };
        if chain.len() > 32 || chain.iter().any(|c| std::ptr::eq(*c, *next)) {
            break;
        }
        chain.push(next);
        current = next;
    }
    if chain.iter().any(|o| !flag(o.get("visible"), props, true)) {
        return None;
    }
    let mut resolved = local_transform(chain[chain.len() - 1], props);
    for object in chain.iter().rev().skip(1) {
        let local = local_transform(object, props);
        let (x, y) = (local.origin[0] * resolved.scale[0], local.origin[1] * resolved.scale[1]);
        let (sin, cos) = resolved.angle.sin_cos();
        resolved = Transform {
            origin: [
                resolved.origin[0] + x * cos - y * sin,
                resolved.origin[1] + x * sin + y * cos,
                resolved.origin[2] + local.origin[2] * resolved.scale[2],
            ],
            scale: [0, 1, 2].map(|i| local.scale[i] * resolved.scale[i]),
            angle: local.angle + resolved.angle,
        };
    }
    Some(resolved)
}

#[derive(Clone, Copy, PartialEq)]
enum Blend {
    Translucent,
    Additive,
}

/// 一个待绘制的图片图层。
struct Layer {
    texture: Option<String>,
    /// 纯色图层（solidlayer 或内置的 util/white）
    solid: bool,
    size: Option<(f64, f64)>,
    model_size: Option<(f64, f64)>,
    fullscreen: bool,
    transform: Transform,
    align: String,
    tint: [f64; 3],
    alpha: f64,
    blend: Blend,
}

fn texture_path(name: &str) -> String {
    format!("materials/{}.tex", name.trim_end_matches(".tex"))
}

fn layer(source: &Source, object: &Value, model: &str, transform: Transform, props: &Value) -> Option<Layer> {
    let model = source.json(model).ok()?;
    if model.get("passthrough").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let solid = model.get("solidlayer").and_then(Value::as_bool) == Some(true);
    let material = model.get("material").and_then(Value::as_str).and_then(|m| source.json(m).ok());
    let pass = material.as_ref().and_then(|m| m.get("passes")).and_then(Value::as_array).and_then(|p| p.first());
    let mut texture = pass
        .and_then(|p| p.get("textures"))
        .and_then(Value::as_array)
        .and_then(|t| t.first())
        .and_then(Value::as_str)
        .map(str::to_owned);
    if let Some(name) = object
        .pointer("/instance/textures")
        .and_then(Value::as_array)
        .and_then(|t| t.first())
        .and_then(Value::as_str)
    {
        texture = Some(name.to_owned());
    }
    let blend = match pass.and_then(|p| p.get("blending")).and_then(Value::as_str) {
        Some("additive") => Blend::Additive,
        _ => Blend::Translucent,
    };
    // util/white 等内置纹理不在壁纸包里：白色纹理相当于纯色图层，其它内置纹理跳过。
    let builtin_white = texture.as_deref().is_some_and(|t| t.eq_ignore_ascii_case("util/white"));
    let texture = texture.filter(|t| !t.starts_with("_rt_") && !t.starts_with("util/"));
    if texture.is_none() && !solid && !builtin_white {
        return None;
    }
    let size = match floats(object.get("size"), props).as_slice() {
        [w, h, ..] if *w > 0.0 && *h > 0.0 => Some((*w, *h)),
        _ => None,
    };
    let model_size = match (model.get("width").and_then(Value::as_f64), model.get("height").and_then(Value::as_f64)) {
        (Some(w), Some(h)) if w > 0.0 && h > 0.0 => Some((w, h)),
        _ => None,
    };
    let color = vec3(object.get("color"), props, 1.0);
    let brightness = number(object.get("brightness"), props, 1.0);
    Some(Layer {
        solid: solid || (builtin_white && texture.is_none()),
        texture: texture.map(|t| texture_path(&t)),
        size,
        model_size,
        fullscreen: model.get("fullscreen").and_then(Value::as_bool) == Some(true),
        transform,
        align: object
            .get("horizontalalign")
            .or_else(|| object.get("alignment"))
            .and_then(Value::as_str)
            .unwrap_or("center")
            .to_ascii_lowercase(),
        tint: color.map(|c| (c * brightness).clamp(0.0, 4.0)),
        alpha: number(object.get("alpha"), props, 1.0).clamp(0.0, 1.0),
        blend,
    })
}

fn clear_color(scene: &Value, props: &Value) -> Rgba<u8> {
    let c = vec3(scene.pointer("/general/clearcolor"), props, 1.0);
    let channel = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgba([channel(c[0]), channel(c[1]), channel(c[2]), 255])
}

/// 双线性取样（坐标以像素为单位，越界返回 None）。
fn sample(texture: &RgbaImage, x: f64, y: f64) -> Option<[f64; 4]> {
    let (w, h) = (texture.width() as f64, texture.height() as f64);
    if !(0.0..w).contains(&x) || !(0.0..h).contains(&y) {
        return None;
    }
    let (fx, fy) = ((x - 0.5).max(0.0), (y - 0.5).max(0.0));
    let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(texture.width() - 1), (y0 + 1).min(texture.height() - 1));
    let (tx, ty) = (fx - x0 as f64, fy - y0 as f64);
    let p = |x, y| texture.get_pixel(x, y).0.map(|c| c as f64);
    let (a, b, c, d) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
    Some([0, 1, 2, 3].map(|i| (a[i] * (1.0 - tx) + b[i] * tx) * (1.0 - ty) + (c[i] * (1.0 - tx) + d[i] * tx) * ty))
}

/// 把图层画到画布上。`center` 与 `size` 已换算为画布像素；画布 y 轴向下，所以场景中的逆时针旋转在画布上是顺时针。
fn draw(canvas: &mut RgbaImage, texture: Option<&RgbaImage>, center: (f64, f64), size: (f64, f64), angle: f64, layer: &Layer) {
    let (hw, hh) = (size.0.abs() / 2.0, size.1.abs() / 2.0);
    let (sin, cos) = angle.sin_cos();
    // 旋转后的外接矩形
    let ex = hw * cos.abs() + hh * sin.abs();
    let ey = hw * sin.abs() + hh * cos.abs();
    let (cw, ch) = (canvas.width() as f64, canvas.height() as f64);
    let x0 = (center.0 - ex).floor().clamp(0.0, cw) as u32;
    let x1 = (center.0 + ex).ceil().clamp(0.0, cw) as u32;
    let y0 = (center.1 - ey).floor().clamp(0.0, ch) as u32;
    let y1 = (center.1 + ey).ceil().clamp(0.0, ch) as u32;
    for py in y0..y1 {
        for px in x0..x1 {
            // 画布 → 场景方向（y 向上）→ 图层本地坐标
            let (dx, dy) = (px as f64 + 0.5 - center.0, -(py as f64 + 0.5 - center.1));
            let (lx, ly) = (dx * cos + dy * sin, -dx * sin + dy * cos);
            let (u, v) = (lx / size.0 + 0.5, 0.5 - ly / size.1);
            if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                continue;
            }
            let src = match texture {
                Some(t) => match sample(t, u * t.width() as f64, v * t.height() as f64) {
                    Some(s) => s,
                    None => continue,
                },
                None => [255.0; 4],
            };
            let a = src[3] / 255.0 * layer.alpha;
            if a <= 0.0 {
                continue;
            }
            let dst = canvas.get_pixel_mut(px, py);
            for i in 0..3 {
                let s = (src[i] * layer.tint[i]).min(255.0);
                let d = dst.0[i] as f64;
                let out = match layer.blend {
                    Blend::Translucent => s * a + d * (1.0 - a),
                    Blend::Additive => d + s * a,
                };
                dst.0[i] = out.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// 把场景渲染成一张静态图片。
pub fn render(source: &Source, props: &Value) -> AppResult<RgbaImage> {
    let scene = source.json("scene.json")?;
    let objects: Vec<&Value> = scene.get("objects").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
    let by_id: HashMap<i64, &Value> = objects.iter().filter_map(|o| Some((o.get("id")?.as_i64()?, *o))).collect();
    let mut layers = Vec::new();
    for object in &objects {
        if layers.len() >= MAX_LAYERS {
            break;
        }
        let Some(model) = object.get("image").and_then(Value::as_str) else { continue };
        let Some(transform) = resolve(object, &by_id, props) else { continue };
        if let Some(layer) = layer(source, object, model, transform, props) {
            layers.push(layer);
        }
    }
    let mut textures: HashMap<String, Option<RgbaImage>> = HashMap::new();
    for name in layers.iter().filter_map(|l| l.texture.clone()) {
        if let std::collections::hash_map::Entry::Vacant(slot) = textures.entry(name) {
            let decoded = source.read(slot.key(), MAX_ENTRY_BYTES).and_then(|b| decode_tex(&b));
            if let Err(e) = &decoded {
                log::debug!("跳过场景纹理 {}：{e}", slot.key());
            }
            slot.insert(decoded.ok());
        }
    }
    // 纹理缺失的图层直接丢弃（纯色图层除外）。
    layers.retain(|l| l.solid || l.texture.as_ref().is_some_and(|t| textures.get(t).is_some_and(Option::is_some)));
    if layers.is_empty() {
        return largest_texture(source);
    }
    let layer_size = |l: &Layer| -> Option<(f64, f64)> {
        l.size
            .or_else(|| l.texture.as_ref().and_then(|t| textures[t].as_ref()).map(|t| (t.width() as f64, t.height() as f64)))
            .or(l.model_size)
    };
    let projection = scene.pointer("/general/orthogonalprojection");
    let auto = projection.and_then(|p| p.get("auto")).and_then(Value::as_bool) == Some(true);
    let (mut sw, mut sh) = (
        projection.and_then(|p| p.get("width")).and_then(Value::as_f64).unwrap_or(0.0),
        projection.and_then(|p| p.get("height")).and_then(Value::as_f64).unwrap_or(0.0),
    );
    // 自动投影：场景以原点为中心，画布取所有图层的最大范围。
    let mut offset = (0.0, 0.0);
    if auto || !(sw > 0.0 && sh > 0.0) {
        let (mut ex, mut ey) = (0.0f64, 0.0f64);
        for l in &layers {
            if let Some((w, h)) = layer_size(l) {
                ex = ex.max(l.transform.origin[0].abs() + w * l.transform.scale[0].abs() / 2.0);
                ey = ey.max(l.transform.origin[1].abs() + h * l.transform.scale[1].abs() / 2.0);
            }
        }
        if !(ex > 0.0 && ey > 0.0) {
            return largest_texture(source);
        }
        sw = ex * 2.0;
        sh = ey * 2.0;
        offset = (ex, ey);
    }
    let ratio = (MAX_CANVAS as f64 / sw.max(sh)).min(1.0);
    let (cw, ch) = ((sw * ratio).round().max(1.0) as u32, (sh * ratio).round().max(1.0) as u32);
    let mut canvas = RgbaImage::from_pixel(cw, ch, clear_color(&scene, props));
    let mut drawn = 0;
    for l in &layers {
        let (mut w, mut h, mut cx, mut cy) = match (l.fullscreen, layer_size(l)) {
            (true, _) => (sw, sh, sw / 2.0 - offset.0, sh / 2.0 - offset.1),
            (false, Some((w, h))) => (w, h, l.transform.origin[0], l.transform.origin[1]),
            (false, None) if l.solid => (sw, sh, sw / 2.0 - offset.0, sh / 2.0 - offset.1),
            _ => continue,
        };
        if !l.fullscreen {
            w *= l.transform.scale[0];
            h *= l.transform.scale[1];
            if l.align.contains("top") {
                cy -= h.abs() / 2.0;
            } else if l.align.contains("bottom") {
                cy += h.abs() / 2.0;
            }
            if l.align.contains("left") {
                cx += w.abs() / 2.0;
            } else if l.align.contains("right") {
                cx -= w.abs() / 2.0;
            }
        }
        let (w, h) = (w * ratio, h * ratio);
        if !(w.abs() >= 1.0 && h.abs() >= 1.0) || w.abs() > 8.0 * MAX_CANVAS as f64 || h.abs() > 8.0 * MAX_CANVAS as f64 {
            continue;
        }
        let center = ((cx + offset.0) * ratio, ch as f64 - (cy + offset.1) * ratio);
        let texture = l.texture.as_ref().and_then(|t| textures[t].as_ref());
        // 纹理远大于目标尺寸时先缩小，避免取样产生锯齿。
        let scaled;
        let texture = match texture {
            Some(t) if t.width() as f64 > 2.0 * w.abs() && t.height() as f64 > 2.0 * h.abs() => {
                scaled = imageops::resize(t, w.abs().ceil() as u32, h.abs().ceil() as u32, imageops::FilterType::Triangle);
                Some(&scaled)
            }
            other => other,
        };
        let angle = if l.fullscreen { 0.0 } else { l.transform.angle };
        draw(&mut canvas, texture, center, (w, h), angle, l);
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
pub fn rendered(config: &Path, id: &str, root: &Path, declared: Option<&str>, props: &Value) -> AppResult<PathBuf> {
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
    // 缓存名包含渲染器版本与用户属性：渲染规则或壁纸设置变化后重新渲染。
    let settings = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, props.to_string().as_bytes()).simple().to_string();
    let dest = dir.join(format!("{id}-r{RENDERER}-{modified}-{}-{}.png", meta.len(), &settings[..8]));
    if !dest.is_file() {
        let image = render(&source, props)?;
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
