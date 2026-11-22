//! 网页壁纸：通过 `rcwall` 协议提供项目目录内的文件，在前端的沙盒 iframe 中运行。
//!
//! 安全边界：
//! - 只提供壁纸库中「用户自己的网页壁纸」项目目录内的文件（规范化路径后检查，拒绝 `..` 与符号链接越界）；
//! - 页面与启动器不同源，且 Tauri 的 IPC 脚本只注入主框架，壁纸页面拿不到调用密钥；
//! - 响应附带 CSP，禁止连接 IPC 地址、嵌套框架与插件；
//! - 注入的垫片把媒体静音，并按 project.json 的默认值调用 Wallpaper Engine 的属性回调。

use super::{steam, store};
use crate::state::AppState;
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};
use std::path::{Component, Path, PathBuf};
use tauri::http::{Request, Response};
use tauri::{AppHandle, Manager};

const SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b'/')
    .add(b'\\');

const MAX_FILE: u64 = 512 * 1024 * 1024;

/// 页面可以使用项目内的资源与公开网络资源（不少网页壁纸从 CDN 加载脚本或字体），
/// 但不能连接启动器的 IPC，也不能再嵌套框架或提交表单。
pub const CSP: &str = "default-src 'self' rcwall: http://rcwall.localhost https://rcwall.localhost https: data: blob: 'unsafe-inline' 'unsafe-eval'; \
connect-src 'self' rcwall: http://rcwall.localhost https://rcwall.localhost https: wss: data: blob:; \
object-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'self'";

/// 入口文件相对项目根目录的地址，每个路径段单独编码。
pub fn entry(root: &Path, media: &Path) -> crate::error::AppResult<String> {
    let rel = media
        .strip_prefix(root)
        .map_err(|_| crate::error::AppError::msg("网页壁纸入口超出项目目录", "请刷新壁纸库"))?;
    Ok(segments(rel))
}

fn segments(rel: &Path) -> String {
    rel.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(utf8_percent_encode(&s.to_string_lossy(), SEGMENT).to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// 解析请求路径 `/<素材 ID>/<相对路径>`；任何可疑路径段都直接拒绝。
pub fn parse(path: &str) -> Option<(String, PathBuf)> {
    let mut parts = path.trim_start_matches('/').split('/');
    let id = percent_decode_str(parts.next()?).decode_utf8().ok()?.into_owned();
    if id.is_empty() {
        return None;
    }
    let mut rel = PathBuf::new();
    for part in parts {
        let part = percent_decode_str(part).decode_utf8().ok()?;
        if part.is_empty() || part == "." || part == ".." || part.contains(['/', '\\', ':', '\0']) {
            return None;
        }
        rel.push(part.as_ref());
    }
    (rel.components().count() > 0).then_some((id, rel))
}

pub fn mime(path: &Path) -> &'static str {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or_default().to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "ogv" => "video/ogg",
        "mp3" => "audio/mpeg",
        "ogg" | "oga" => "audio/ogg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "glsl" | "vert" | "frag" | "shader" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// 在页面最前面插入垫片，保证它先于壁纸自己的脚本执行。
pub fn inject(html: &[u8], properties: &serde_json::Value) -> Vec<u8> {
    let props = serde_json::to_string(properties)
        .unwrap_or_else(|_| "{}".into())
        .replace("</", "<\\/");
    let shim = format!("<script>{}</script>", SHIM.replace("__PROPS__", &props));
    let text = String::from_utf8_lossy(html);
    let lower = text.to_ascii_lowercase();
    let at = ["<head", "<html"]
        .iter()
        .find_map(|tag| {
            let start = lower.find(tag)?;
            lower[start..].find('>').map(|end| start + end + 1)
        })
        // 跳过 doctype，避免页面进入怪异模式。
        .or_else(|| lower.find("<!doctype").and_then(|s| lower[s..].find('>').map(|e| s + e + 1)))
        .unwrap_or(0);
    let mut out = String::with_capacity(text.len() + shim.len());
    out.push_str(&text[..at]);
    out.push_str(&shim);
    out.push_str(&text[at..]);
    out.into_bytes()
}

const SHIM: &str = r#"(function () {
  var props = __PROPS__;
  var mute = function (m) { try { m.muted = true; m.volume = 0; } catch (e) {} };
  var play = HTMLMediaElement.prototype.play;
  HTMLMediaElement.prototype.play = function () { mute(this); return play.apply(this, arguments); };
  document.addEventListener("play", function (e) { if (e.target instanceof HTMLMediaElement) mute(e.target); }, true);
  ["AudioContext", "webkitAudioContext"].forEach(function (name) {
    var C = window[name];
    if (!C) return;
    var Muted = function () {
      var ctx = new (Function.prototype.bind.apply(C, [null].concat([].slice.call(arguments))))();
      try { ctx.suspend(); } catch (e) {}
      ctx.resume = function () { return Promise.resolve(); };
      return ctx;
    };
    Muted.prototype = C.prototype;
    window[name] = Muted;
  });
  var noop = function () {};
  ["wallpaperRegisterAudioListener", "wallpaperRegisterMediaStatusListener", "wallpaperRegisterMediaPropertiesListener",
   "wallpaperRegisterMediaThumbnailListener", "wallpaperRegisterMediaPlaybackListener", "wallpaperRegisterMediaTimelineListener",
   "wallpaperRequestRandomFileForProperty"].forEach(function (name) { if (!window[name]) window[name] = noop; });
  var applied = false;
  var apply = function () {
    var l = window.wallpaperPropertyListener;
    if (!l || applied) return;
    applied = true;
    try { if (l.applyGeneralProperties) l.applyGeneralProperties({ fps: 30 }); } catch (e) { console.error(e); }
    try { if (l.applyUserProperties) l.applyUserProperties(props); } catch (e) { console.error(e); }
    try { if (l.setPaused) l.setPaused(false); } catch (e) {}
  };
  document.addEventListener("DOMContentLoaded", function () { setTimeout(apply, 0); });
  window.addEventListener("load", function () { setTimeout(apply, 0); setTimeout(apply, 500); });
})();"#;

fn status(code: u16) -> Response<Vec<u8>> {
    Response::builder().status(code).body(Vec::new()).unwrap()
}

/// `rcwall` 协议的处理函数。
pub fn serve(app: &AppHandle, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let Some((id, rel)) = parse(request.uri().path()) else { return status(400) };
    let Some(state) = app.try_state::<AppState>() else { return status(503) };
    let asset = match state.backgrounds.lock() {
        Ok(library) => library
            .assets
            .iter()
            .find(|a| a.id == id && a.kind == "web" && a.is_user_we() && a.media.is_some())
            .cloned(),
        Err(_) => return status(503),
    };
    let Some(asset) = asset else { return status(404) };
    let Ok(file) = store::inside(&asset.root, &asset.root.join(&rel)) else { return status(404) };
    if std::fs::metadata(&file).map(|m| m.len() > MAX_FILE).unwrap_or(true) {
        return status(413);
    }
    let Ok(mut bytes) = std::fs::read(&file) else { return status(404) };
    let is_entry = asset.media.as_ref() == Some(&file);
    if is_entry {
        bytes = inject(&bytes, &steam::user_properties(&asset.root));
    }
    Response::builder()
        .header("Content-Type", mime(&file))
        .header("Content-Security-Policy", CSP)
        .header("X-Content-Type-Options", "nosniff")
        .header("Cache-Control", "no-cache")
        .body(bytes)
        .unwrap()
}
