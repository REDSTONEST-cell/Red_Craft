//! Windows：网页壁纸通过 WebView2 的虚拟主机映射加载（Lively Wallpaper 也是这样加载 Wallpaper Engine 网页壁纸的）。
//!
//! 每个壁纸映射到独立的主机名 `https://<素材 ID>.wallpaper.example/`，由 WebView2 直接读取项目目录：
//! 正确的 MIME 类型、Range 分段读取与大文件流式读取都由 WebView2 处理。
//! 页面与启动器不同源，Tauri 把它视为远程页面，不授予任何 IPC 权限；Tauri 的 IPC 脚本也只注入主框架。
//! Wallpaper Engine 的运行环境垫片通过「文档创建时执行的脚本」注入，只在这些主机名下生效。

use crate::error::{AppError, AppResult};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager};

pub const DOMAIN: &str = "wallpaper.example";

static SHIM_ADDED: AtomicBool = AtomicBool::new(false);

/// `\\?\C:\…` 形式的规范化路径转回普通路径（WebView2 不接受 verbatim 路径）。
fn plain(path: &Path) -> String {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s.into_owned()
    }
}

/// 把壁纸目录映射为虚拟主机，返回主机名。同步等待映射完成，保证 iframe 加载时已经可用。
pub fn map(app: &AppHandle, id: &str, root: &Path) -> AppResult<String> {
    let host = format!("{}.{DOMAIN}", id.to_ascii_lowercase());
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| AppError::msg("找不到启动器窗口", "请重新启动启动器"))?;
    let folder = plain(root);
    let (tx, rx) = std::sync::mpsc::channel();
    let target = host.clone();
    window
        .with_webview(move |pw| {
            use webview2_com::AddScriptToExecuteOnDocumentCreatedCompletedHandler;
            use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2_3, COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_ALLOW};
            use windows_core::{Interface, HSTRING};
            // SAFETY: with_webview 的回调在 WebView 所在线程执行。
            let result = unsafe {
                (|| -> windows_core::Result<()> {
                    let core = pw.controller().CoreWebView2()?;
                    if !SHIM_ADDED.swap(true, Ordering::SeqCst) {
                        let script = format!(
                            "if (/\\.{}$/i.test(location.hostname)) {{ {} }}",
                            DOMAIN.replace('.', "\\."),
                            super::web::shim("null")
                        );
                        let handler = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(|_, _| Ok(())));
                        if let Err(e) = core.AddScriptToExecuteOnDocumentCreated(&HSTRING::from(script), &handler) {
                            SHIM_ADDED.store(false, Ordering::SeqCst);
                            return Err(e);
                        }
                    }
                    core.cast::<ICoreWebView2_3>()?.SetVirtualHostNameToFolderMapping(
                        &HSTRING::from(target.as_str()),
                        &HSTRING::from(folder.as_str()),
                        COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_ALLOW,
                    )
                })()
            };
            let _ = tx.send(result.map_err(|e| e.message().to_string()));
        })
        .map_err(|e| AppError::msg(format!("无法访问 WebView：{e}"), "请重新启动启动器"))?;
    match rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(())) => Ok(host),
        Ok(Err(e)) => Err(AppError::msg(format!("无法加载网页壁纸：{e}"), "请更新 Microsoft Edge WebView2 运行时后重试")),
        Err(_) => Err(AppError::msg("加载网页壁纸超时", "请稍后重试")),
    }
}
