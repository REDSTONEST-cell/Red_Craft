//! 内存状态与内存优化的 commands。
//!
//! 完整优化需要管理员权限：设置页的「立即优化」以管理员身份运行一次启动器自身
//! （带 `--optimize-memory <结果文件>` 参数，只做优化然后退出），结果写入文件后读回。

use std::path::PathBuf;

use launcher_core::memopt::{self, OptimizeResult};
use launcher_core::system;
use serde::Serialize;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::settings::InstanceConfig;
use crate::state::AppState;

pub const HELPER_ARG: &str = "--optimize-memory";

/// 以 `--optimize-memory <文件>` 启动时只做内存优化，结果写入文件。返回 true 表示应直接退出。
pub fn run_helper_if_requested() -> bool {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some(HELPER_ARG) {
        return false;
    }
    let result = memopt::optimize();
    if let Some(out) = args.get(2) {
        let _ = std::fs::write(out, serde_json::to_vec(&result).unwrap_or_default());
    }
    true
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStatus {
    pub total_mb: u64,
    pub available_mb: u64,
    /// 启动当前选中实例时会分配给游戏的内存
    pub game_mb: u32,
    /// 是否支持内存优化（仅 Windows）
    pub supported: bool,
}

/// 启动实例时分配给游戏的内存（与启动流程使用同一规则）。
pub fn game_memory_mb(state: &AppState, instance: Option<&str>, info: system::MemoryInfo) -> u32 {
    let settings = state.settings();
    let config = instance.map(|id| InstanceConfig::load(&state.game_dir(), id)).unwrap_or_default();
    if let Some(m) = config.max_memory_mb.filter(|m| *m > 0) {
        return m;
    }
    if !settings.memory_auto {
        return settings.max_memory_mb;
    }
    if cfg!(target_os = "android") {
        // 手机版与 FCL 相同，只按手机内存分配（见 mobile_runtime）
        return crate::mobile_runtime::auto_memory_mb(info);
    }
    let modded = instance
        .and_then(|id| crate::content::instance_context(state, id).ok())
        .is_some_and(|c| c.loader.is_some());
    system::auto_memory_mb(info, modded)
}

#[tauri::command]
pub async fn memory_status(state: State<'_, AppState>) -> AppResult<MemoryStatus> {
    let info = tauri::async_runtime::spawn_blocking(system::memory)
        .await
        .map_err(|e| AppError::msg(e.to_string(), ""))?;
    let selected = state.settings().selected_instance;
    Ok(MemoryStatus {
        total_mb: info.total_mb,
        available_mb: info.available_mb,
        game_mb: game_memory_mb(&state, selected.as_deref(), info),
        supported: memopt::supported(),
    })
}

/// 立即优化内存。`elevate` 为 true 且当前不是管理员时，以管理员身份运行一次完整优化（会弹出 UAC）。
#[tauri::command]
pub async fn optimize_memory(state: State<'_, AppState>, elevate: bool) -> AppResult<OptimizeResult> {
    if !memopt::supported() {
        return Err(AppError::msg("内存优化仅支持 Windows", ""));
    }
    let local = tauri::async_runtime::spawn_blocking(memopt::optimize)
        .await
        .map_err(|e| AppError::msg(e.to_string(), ""))?;
    if local.full || !elevate {
        return Ok(local);
    }
    let out = state.paths.config_dir.join("cache").join("memopt-result.json");
    let _ = std::fs::create_dir_all(out.parent().unwrap_or(&state.paths.config_dir));
    let _ = std::fs::remove_file(&out);
    let o2 = out.clone();
    tauri::async_runtime::spawn_blocking(move || run_elevated(&o2))
        .await
        .map_err(|e| AppError::msg(e.to_string(), ""))??;
    let full: OptimizeResult = std::fs::read(&out)
        .ok()
        .and_then(|d| serde_json::from_slice(&d).ok())
        .ok_or_else(|| AppError::msg("内存优化没有完成", "请重试，或以管理员身份运行启动器。"))?;
    let _ = std::fs::remove_file(&out);
    // 把两次优化合并成一次结果：从第一次之前到第二次之后
    Ok(OptimizeResult { before: local.before, after: full.after, full: full.full })
}

#[cfg(windows)]
fn run_elevated(out: &std::path::Path) -> AppResult<()> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::WaitForSingleObject;
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let exe: PathBuf = std::env::current_exe().map_err(|e| AppError::msg(e.to_string(), ""))?;
    let verb = wide("runas");
    let file = wide(&exe.to_string_lossy());
    let params = wide(&format!("{HELPER_ARG} \"{}\"", out.display()));
    // SAFETY: 结构体按文档初始化，所有字符串在调用期间保持有效；进程句柄用完后关闭
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&mut info) == 0 {
            return Err(AppError::msg("已取消管理员权限请求", "完整的内存优化需要管理员权限。"));
        }
        if !info.hProcess.is_null() {
            WaitForSingleObject(info.hProcess, 60_000);
            CloseHandle(info.hProcess);
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn run_elevated(_out: &std::path::Path) -> AppResult<()> {
    let _: Option<PathBuf> = None;
    Err(AppError::msg("内存优化仅支持 Windows", ""))
}

/// 启动器当前是否处于隐藏 / 最小化（此时 WebView2 保持低内存模式）
#[cfg(windows)]
static HIDDEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 设置 WebView2 的内存使用目标：LOW 时释放缓存、压缩内存。只在 Windows 上有效，旧版 WebView2 不支持时忽略。
#[cfg(windows)]
fn set_webview_memory_low(webview: &tauri::WebviewWindow, low: bool) {
    let _ = webview.with_webview(move |pw| {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        };
        use windows_core::Interface;
        let level = if low { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW } else { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL };
        // SAFETY: COM 调用在 WebView 所在线程执行；旧版 WebView2 不支持该接口时忽略
        unsafe {
            if let Ok(core) = pw.controller().CoreWebView2() {
                if let Ok(core) = core.cast::<ICoreWebView2_19>() {
                    let _ = core.SetMemoryUsageTargetLevel(level);
                }
            }
        }
    });
}

/// 启动器隐藏或最小化时（例如游戏运行期间）让 WebView2 降低内存占用：释放缓存、压缩内存，
/// 重新显示时恢复正常。只在 Windows 上有效。
pub fn webview_memory_follow_window(window: &tauri::Window) {
    #[cfg(windows)]
    {
        use tauri::Manager;
        let hidden = !window.is_visible().unwrap_or(true) || window.is_minimized().unwrap_or(false);
        if HIDDEN.swap(hidden, std::sync::atomic::Ordering::Relaxed) == hidden {
            return;
        }
        if let Some(webview) = window.app_handle().get_webview_window(window.label()) {
            set_webview_memory_low(&webview, hidden);
        }
    }
    #[cfg(not(windows))]
    let _ = window;
}

/// 视频背景被移除后，WebView2 不会马上把解码器占用的内存还给系统（#7）。
/// 短暂切换到低内存模式促使它回收，再恢复正常；启动器隐藏期间保持低内存模式不变。
#[tauri::command]
pub async fn trim_webview_memory(webview: tauri::WebviewWindow) {
    #[cfg(windows)]
    {
        if HIDDEN.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        set_webview_memory_low(&webview, true);
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        if !HIDDEN.load(std::sync::atomic::Ordering::Relaxed) {
            set_webview_memory_low(&webview, false);
        }
    }
    #[cfg(not(windows))]
    let _ = webview;
}

