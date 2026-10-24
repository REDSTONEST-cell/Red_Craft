//! 启动游戏：选择 Java、计算内存、准备文件、启动进程并转发日志。

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use launcher_core::auth::{AccountKind, LaunchAuth};
use launcher_core::instance::detect_loader;
use launcher_core::java::{self, component_major};
use launcher_core::launch::{self, split_args, GameExit, LaunchOptions, LogLine, ServerAddress};
use launcher_core::system;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::{instance_isolated, java_list};
use crate::error::{AppError, AppResult};
use crate::settings::{AfterLaunch, InstanceConfig};
use crate::state::AppState;

pub const GAME_EVENT: &str = "game";

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum GameEvent {
    /// 进程已创建
    Spawned { instance: String, pid: u32, command: String },
    /// 游戏窗口即将出现（日志中出现了初始化完成的标志）
    Started { instance: String },
    Log { instance: String, lines: Vec<LogLine> },
    Exit { instance: String, exit: GameExit },
}

/// 1.6 之前的旧版本 JSON 中常常没有 javaVersion，默认使用 Java 8。
pub fn required_java(json: &launcher_core::version::VersionJson) -> (u32, String) {
    match &json.java_version {
        Some(j) if j.major_version > 0 => (j.major_version, j.component.clone()),
        Some(j) if !j.component.is_empty() => (component_major(&j.component), j.component.clone()),
        _ => (8, "jre-legacy".into()),
    }
}

fn component_for(major: u32, fallback: &str) -> String {
    if !fallback.is_empty() {
        return fallback.to_string();
    }
    match major {
        0..=8 => "jre-legacy",
        9..=16 => "java-runtime-alpha",
        17..=20 => "java-runtime-gamma",
        21..=24 => "java-runtime-delta",
        _ => "java-runtime-epsilon",
    }
    .into()
}

#[tauri::command]
pub async fn launch_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<u64> {
    if state.running.lock().is_ok_and(|r| r.contains_key(&id)) {
        return Err(AppError::msg("这个实例已经在运行了", "如需重新启动，请先关闭正在运行的游戏。"));
    }
    let settings = state.settings();
    let accounts = crate::commands::list_accounts(state.clone());
    let account = accounts
        .selected
        .and_then(|sel| accounts.accounts.into_iter().find(|a| a.id == sel))
        .ok_or_else(|| AppError::msg("还没有选择账号", "请先在「账号」页面添加并选择一个账号。"))?;
    if matches!(account.kind, AccountKind::Yggdrasil { .. }) {
        return Err(AppError::msg("暂不支持外置登录账号", "外置登录将在后续版本中提供。"));
    }

    let installer = state.installer()?;
    let game = installer.game.clone();
    let resolved = installer.resolve(&id)?;
    let config = InstanceConfig::load(&game, &id);
    let summary = launcher_core::instance::InstanceSummary {
        id: id.clone(),
        mc_version: String::new(),
        kind: resolved.json.kind.clone(),
        loader: detect_loader(&resolved),
        release_time: String::new(),
        error: None,
    };
    let modded = summary.loader.is_some();
    let isolated = instance_isolated(&state, &summary, &config);
    let game_dir = game.instance_game_dir(&id, isolated);
    let (required, component) = required_java(&resolved.json);
    let release_time = resolved.json.release_time.clone();
    let java_override = config.java_path.clone().or(settings.java_path.clone());

    let memory = system::memory();
    let max_memory_mb = match config.max_memory_mb {
        Some(m) if m > 0 => m,
        _ if settings.memory_auto => system::auto_memory_mb(memory, modded),
        _ => settings.max_memory_mb,
    };
    let jvm_args = config.jvm_args.clone().unwrap_or(settings.jvm_args.clone());
    let server = config.server.clone().filter(|s| !s.is_empty()).unwrap_or(settings.server.clone());

    let tasks = state.tasks.clone();
    let app2 = app.clone();
    let title = format!("启动 {id}");
    let task_id = tasks.spawn(&app, title, move |reporter, cancel| async move {
        let state = app2.state::<AppState>();

        // 登录凭据：正版账号可能需要先刷新令牌
        let auth = match &account.kind {
            AccountKind::Microsoft => {
                reporter.stage("验证微软账号");
                crate::auth::microsoft_launch_auth(&state, &account).await?
            }
            _ => LaunchAuth::offline(&account.name),
        };
        cancel.check()?;

        // 选择 Java
        reporter.stage("选择 Java");
        let (java_path, java_major) =
            select_java(&state, &installer, java_override, required, &component, &reporter, &cancel).await?;

        let opts = LaunchOptions {
            java: java_path,
            game_dir: game_dir.clone(),
            auth,
            max_memory_mb,
            min_memory_mb: settings.min_memory_mb.min(max_memory_mb),
            java_major,
            extra_jvm_args: split_args(&jvm_args),
            extra_game_args: split_args(&settings.game_args),
            resolution: settings
                .custom_resolution
                .then_some((settings.window_width, settings.window_height)),
            fullscreen: settings.fullscreen,
            server: ServerAddress::parse(&server),
            launcher_name: launcher_core::LAUNCHER_ID.into(),
            launcher_version: launcher_core::LAUNCHER_VERSION.into(),
        };

        if settings.default_chinese {
            launch::ensure_chinese(&game_dir, &release_time)?;
        }
        let cmd = launch::prepare(&installer, &id, &opts, &reporter, &cancel).await?;
        cancel.check()?;
        reporter.stage("启动游戏");
        let command_line = cmd.display(&opts.auth.access_token);
        log::info!("启动命令：{command_line}");

        // 日志批量推送，避免大量 IPC 消息拖慢界面
        let buffer: Arc<Mutex<Vec<LogLine>>> = Arc::new(Mutex::new(Vec::new()));
        let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let on_log = {
            let buffer = buffer.clone();
            let started = started.clone();
            let app = app2.clone();
            let id = id.clone();
            Arc::new(move |line: LogLine| {
                if !started.load(std::sync::atomic::Ordering::Relaxed)
                    && (line.text.contains("Setting user:")
                        || line.text.contains("LWJGL Version")
                        || line.text.contains("Backend library: LWJGL"))
                {
                    started.store(true, std::sync::atomic::Ordering::Relaxed);
                    let _ = app.emit(GAME_EVENT, GameEvent::Started { instance: id.clone() });
                    after_started(&app);
                }
                if let Ok(mut b) = buffer.lock() {
                    b.push(line);
                }
            })
        };

        let running = launch::spawn(&cmd, on_log)?;
        let pid = running.pid().unwrap_or_default();
        if let Ok(mut r) = state.running.lock() {
            r.insert(id.clone(), pid);
        }
        let mut config = config;
        config.last_played = Some(SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
        let _ = config.save(&game, &id);
        let _ = app2.emit(
            GAME_EVENT,
            GameEvent::Spawned {
                instance: id.clone(),
                pid,
                command: command_line,
            },
        );

        // 后台等待游戏退出；启动任务本身到此结束
        let app3 = app2.clone();
        tauri::async_runtime::spawn(async move {
            let flusher = {
                let buffer = buffer.clone();
                let app = app3.clone();
                let id = id.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_millis(120)).await;
                        flush(&app, &id, &buffer);
                    }
                })
            };
            let exit = running.wait().await;
            flusher.abort();
            flush(&app3, &id, &buffer);
            let state = app3.state::<AppState>();
            if let Ok(mut r) = state.running.lock() {
                r.remove(&id);
            }
            let killed = state.killed.lock().is_ok_and(|mut k| k.remove(&id));
            let mut exit = exit.unwrap_or_else(|e| GameExit {
                code: None,
                crashed: true,
                crash_report: None,
                causes: vec![],
                tail: vec![e.to_string()],
            });
            if killed {
                exit.crashed = false;
                exit.causes.clear();
            }
            // 隐藏了启动器的话，游戏退出后重新显示
            if let Some(w) = app3.get_webview_window("main") {
                if exit.crashed || !w.is_visible().unwrap_or(true) {
                    let _ = w.show();
                    let _ = w.unminimize();
                    let _ = w.set_focus();
                }
            }
            let _ = app3.emit(GAME_EVENT, GameEvent::Exit { instance: id, exit });
        });
        Ok(())
    });
    Ok(task_id)
}

/// 选择 Java：优先使用手动指定的；否则从已安装的里挑选，没有合适的就从 Mojang 下载。
pub async fn select_java(
    state: &AppState,
    installer: &launcher_core::install::Installer,
    java_override: Option<std::path::PathBuf>,
    required: u32,
    component: &str,
    reporter: &launcher_core::install::Reporter,
    cancel: &launcher_core::download::CancelToken,
) -> AppResult<(std::path::PathBuf, u32)> {
    if let Some(path) = java_override {
        let p = path.clone();
        let info = tauri::async_runtime::spawn_blocking(move || java::probe(&p))
            .await
            .ok()
            .flatten()
            .ok_or_else(|| {
                AppError::msg(
                    format!("设置中指定的 Java 无效：{}", path.display()),
                    "请在「设置 → Java」中重新选择，或改为「自动选择」。",
                )
            })?;
        if info.major < required {
            return Err(AppError::msg(
                format!("这个版本需要 Java {required}，但设置中指定的是 Java {}", info.major),
                "请在「设置 → Java」中改为「自动选择」，或指定更高版本的 Java。",
            ));
        }
        return Ok((info.path, info.major));
    }
    let installed = java_list(state, false).await?;
    if let Some(j) = java::pick(&installed, required, &installer.env.arch) {
        return Ok((j.path.clone(), j.major));
    }
    let component = component_for(required, component);
    if let Some(p) = java::runtime_java_path(&installer.game, &component, &installer.env) {
        return Ok((p, required));
    }
    if !state.settings().auto_download_java {
        return Err(AppError::msg(
            format!("没有找到可用的 Java {required}"),
            "请安装对应版本的 Java，或在「设置 → Java」中开启「自动下载 Java」。",
        ));
    }
    let p = java::install_runtime(&installer.downloader, &installer.game, &component, &installer.env, reporter, cancel).await?;
    if let Ok(mut j) = state.java.lock() {
        *j = None;
    }
    Ok((p, required))
}

fn flush(app: &AppHandle, id: &str, buffer: &Mutex<Vec<LogLine>>) {
    let lines = match buffer.lock() {
        Ok(mut b) if !b.is_empty() => std::mem::take(&mut *b),
        _ => return,
    };
    let _ = app.emit(
        GAME_EVENT,
        GameEvent::Log {
            instance: id.to_string(),
            lines,
        },
    );
}

fn after_started(app: &AppHandle) {
    let state = app.state::<AppState>();
    match state.settings().after_launch {
        AfterLaunch::Keep => {}
        AfterLaunch::Hide => {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.hide();
            }
        }
        AfterLaunch::Close => {
            // 稍等片刻，让最后的事件发送出去
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_millis(500)).await;
                app.exit(0);
            });
        }
    }
}

#[tauri::command]
pub fn kill_game(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let pid = state
        .running
        .lock()
        .ok()
        .and_then(|r| r.get(&id).copied())
        .ok_or_else(|| AppError::msg("游戏没有在运行", ""))?;
    if let Ok(mut k) = state.killed.lock() {
        k.insert(id);
    }
    Ok(launch::kill_pid(pid)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_requirements() {
        let v: launcher_core::version::VersionJson = serde_json::from_str(r#"{"id":"a"}"#).unwrap();
        assert_eq!(required_java(&v), (8, "jre-legacy".into()));
        let v: launcher_core::version::VersionJson =
            serde_json::from_str(r#"{"id":"a","javaVersion":{"component":"java-runtime-delta","majorVersion":21}}"#).unwrap();
        assert_eq!(required_java(&v), (21, "java-runtime-delta".into()));
        assert_eq!(component_for(17, ""), "java-runtime-gamma");
        assert_eq!(component_for(25, ""), "java-runtime-epsilon");
    }
}
