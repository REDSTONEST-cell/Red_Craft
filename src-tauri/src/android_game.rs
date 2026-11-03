//! Android 游戏在独立的 JVM Activity 中运行；启动凭据只经过应用私有文件传递。

use std::path::PathBuf;
use std::io::{Read, Seek, SeekFrom};
use std::time::{Duration, Instant};
use launcher_core::auth::{AccountKind, LaunchAuth};
use launcher_core::install::Verify;
use launcher_core::launch::{self, GameExit, LaunchOptions, LogLine, ServerAddress};
use tauri::{AppHandle, Emitter, Manager, State};
use crate::error::{AppError, AppResult};
use crate::launch::{GameEvent, GAME_EVENT};
use crate::settings::InstanceConfig;
use crate::state::AppState;

pub async fn launch_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<u64> {
    let account = {
        let accounts = crate::commands::list_accounts(state.clone());
        accounts.selected.and_then(|selected| accounts.accounts.into_iter().find(|a| a.id == selected))
            .ok_or_else(|| AppError::msg("还没有选择账号", "请先在账号页选择账号。"))?
    };
    let installer = state.installer()?;
    let resolved = installer.resolve(&id)?;
    let (required, _) = crate::launch::required_java(&resolved.json);
    let base = resolved.chain.last().cloned().unwrap_or_default();
    let lwjgl = crate::mobile_runtime::LwjglUsage::from_libraries(resolved.json.libraries.iter().map(|l| l.name.as_str()));
    let runtime = crate::mobile_runtime::mobile_runtime(required, &base, &lwjgl)?;
    {
        let mut running = state.running.lock().map_err(|_| AppError::msg("无法读取游戏状态", "请重启启动器。"))?;
        if !running.is_empty() { return Err(AppError::msg("已有游戏正在启动或运行", "请先结束当前游戏。")); }
        running.insert(id.clone(), 0);
    }
    let settings = state.settings();
    let config = InstanceConfig::load(&installer.game, &id);
    let session = uuid::Uuid::new_v4().to_string();
    let files = state.paths.config_dir.parent().ok_or_else(|| AppError::msg("无法找到应用数据目录", ""))?.to_path_buf();
    let directory = files.join("game-session").join(&session);
    let tasks = state.tasks.clone();
    let app2 = app.clone();
    let task_id = tasks.spawn(&app, format!("启动 {id}"), move |reporter, cancel| async move {
        let result: AppResult<()> = async {
            let state = app2.state::<AppState>();
            let auth = match &account.kind {
                AccountKind::Microsoft => {
                    reporter.stage("验证微软账号");
                    crate::auth::microsoft_launch_auth(&state, &account).await?
                }
                AccountKind::Yggdrasil { .. } => crate::ygg::yggdrasil_launch_auth(&state, &account, &reporter, &cancel).await?,
                AccountKind::Offline => LaunchAuth::offline(&account.name),
            };
            installer.complete(&id, Verify::Quick, &reporter, &cancel).await?;
            cancel.check()?;
            let game_dir = installer.game.instance_game_dir(&id, config.isolation.unwrap_or(true));
            std::fs::create_dir_all(&game_dir).map_err(|e| AppError::msg(e.to_string(), "无法创建游戏目录"))?;
            if settings.default_chinese { launch::ensure_chinese(&game_dir, &resolved.json.release_time)?; }
            let memory = config.max_memory_mb.filter(|m| *m > 0).unwrap_or_else(|| {
                if settings.memory_auto {
                    launcher_core::system::auto_memory_mb(launcher_core::system::memory(), resolved.chain.len() > 1)
                } else { settings.max_memory_mb }
            });
            let opts = LaunchOptions {
                java: PathBuf::from(format!("java{}", runtime.java)), game_dir: game_dir.clone(), auth,
                max_memory_mb: memory, min_memory_mb: settings.min_memory_mb.min(memory), java_major: runtime.java,
                gc: settings.gc, extra_jvm_args: launch::split_args(config.jvm_args.as_deref().unwrap_or(&settings.jvm_args)),
                extra_game_args: launch::split_args(&settings.game_args), resolution: Some((1280,720)), fullscreen: false,
                server: ServerAddress::parse(config.server.as_deref().unwrap_or(&settings.server)),
                launcher_name: launcher_core::LAUNCHER_ID.into(), launcher_version: launcher_core::LAUNCHER_VERSION.into(),
            };
            let cmd = launch::build_command(&installer.game, &resolved, &installer.env, &opts)?;
            let request = serde_json::json!({
                "cwd": game_dir, "args": cmd.args, "mainClass": resolved.json.main_class,
                "java": format!("java{}", runtime.java), "renderer": runtime.renderer, "lwjgl": runtime.lwjgl,
                "disableEarlyWindow": runtime.disable_early_window, "angle": runtime.angle,
            });
            crate::settings::write_json(&directory.join("request.json"), &request)?;
            cancel.check()?;
            reporter.stage("准备手机运行时并启动游戏");
            let window = app2.get_webview_window("main").ok_or_else(|| AppError::msg("无法找到启动器界面", ""))?;
            window.eval(&format!("window.RclGame.start('{}')", session)).map_err(|e| AppError::msg(e.to_string(), "无法打开游戏画面"))?;
            let start = Instant::now();
            let mut spawned = false;
            let mut offset = 0;
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                if cancel.is_cancelled() { let _ = window.eval("window.RclGame.stop()"); cancel.check()?; }
                let status: Option<serde_json::Value> = crate::settings::read_json(&directory.join("status.json"));
                let killed = state.killed.lock().is_ok_and(|k| k.contains(&id));
                let pid = status.as_ref().and_then(|s| s["pid"].as_u64()).unwrap_or(0) as u32;
                if pid > 0 && !spawned {
                    spawned = true;
                    if let Ok(mut r) = state.running.lock() { r.insert(id.clone(), pid); }
                    let _ = app2.emit(GAME_EVENT, GameEvent::Spawned { instance: id.clone(), pid, command: format!("Android Java {} · {}", runtime.java, runtime.renderer_name) });
                }
                if spawned { emit_logs(&app2, &id, &directory, &mut offset, &opts.auth.access_token); }
                let code = status.as_ref().and_then(|s| s["code"].as_i64()).map(|c| c as i32);
                // 游戏进程每秒写一次心跳；Android 上无法可靠地通过 /proc 判断其他进程是否存在
                let vanished = spawned && heartbeat_stale(&directory, start);
                if code.is_some() || killed || vanished {
                    if vanished && code.is_none() && !killed {
                        let _ = window.eval(&format!("window.RclGame.captureExit('{}', {})", session, pid));
                        // 系统退出记录在进程消失后异步产生，稍后再收集最后一段诊断。
                        tokio::time::sleep(Duration::from_millis(1500)).await;
                    }
                    emit_logs(&app2, &id, &directory, &mut offset, &opts.auth.access_token);
                    let log = std::fs::read_to_string(directory.join("game.log")).unwrap_or_default();
                    // 正常退出时游戏进程可能在写入退出码之前就结束了；游戏日志中有「Stopping!」说明是玩家正常退出
                    let code = code.or_else(|| stopped_normally(&log).then_some(0));
                    let crashed = !killed && code != Some(0);
                    let tail: Vec<String> = log.lines().rev().take(80).map(|s| redact(s, &opts.auth.access_token)).collect::<Vec<_>>().into_iter().rev().collect();
                    let causes = if crashed { launch::analyze(&tail.join("\n"), "") } else { vec![] };
                    let _ = app2.emit(GAME_EVENT, GameEvent::Exit { instance: id.clone(), exit: GameExit { code, crashed, crash_report: None, causes, tail } });
                    break;
                }
                if !spawned && start.elapsed() > Duration::from_secs(180) {
                    let _ = window.eval("window.RclGame.stop()");
                    return Err(AppError::msg("游戏画面启动超时", "请重新启动并检查游戏日志。"));
                }
            }
            Ok(())
        }.await;
        let state = app2.state::<AppState>();
        if let Ok(mut r) = state.running.lock() { r.remove(&id); }
        if let Ok(mut k) = state.killed.lock() { k.remove(&id); }
        let _ = std::fs::remove_file(directory.join("request.json"));
        result
    });
    Ok(task_id)
}

/// Minecraft 正常退出（玩家点击「退出游戏」）时会记录「Stopping!」。
fn stopped_normally(log: &str) -> bool {
    log.lines().rev().take(400).any(|l| l.contains("Stopping!"))
}

/// 心跳超过 10 秒没有更新，说明游戏进程已经结束（包括被系统结束、崩溃）。
/// 进程刚启动、心跳文件还没出现时，从启动请求算起给 30 秒。
fn heartbeat_stale(directory: &std::path::Path, start: Instant) -> bool {
    match std::fs::metadata(directory.join("heartbeat")).and_then(|m| m.modified()) {
        Ok(time) => time.elapsed().unwrap_or_default() > Duration::from_secs(10),
        Err(_) => start.elapsed() > Duration::from_secs(30),
    }
}

/// 手机上不能直接启动 Java 进程：Forge / NeoForge 安装器、OptiFine 补丁程序等交给 Android 端，
/// 在独立的 :jvm 进程中用应用内置的 JVM 运行（见 RCL_Android 的 JavaService）。
pub fn register_java_runner(app: AppHandle) {
    launcher_core::java::external::set_runner(std::sync::Arc::new(move |job, cancel| {
        let app = app.clone();
        Box::pin(async move { run_java(app, job, cancel).await })
    }));
}

async fn run_java(
    app: AppHandle,
    job: launcher_core::java::external::JavaJob,
    cancel: launcher_core::download::CancelToken,
) -> launcher_core::Result<launcher_core::java::external::JavaOutcome> {
    use launcher_core::CoreError;
    let state = app.state::<AppState>();
    let files = state.paths.config_dir.parent().ok_or_else(|| CoreError::Java("无法找到应用数据目录".into()))?.to_path_buf();
    let session = uuid::Uuid::new_v4().to_string();
    let directory = files.join("java-session").join(&session);
    let java = job.java.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "java21".into());
    let request = serde_json::json!({"java": java, "args": job.args, "cwd": job.cwd});
    crate::settings::write_json(&directory.join("request.json"), &request).map_err(|e| CoreError::Java(e.to_string()))?;
    let window = app.get_webview_window("main").ok_or_else(|| CoreError::Java("无法找到启动器界面".into()))?;
    window.eval(&format!("window.RclGame.runJava('{session}')")).map_err(|e| CoreError::Java(format!("无法运行{}：{e}", job.label)))?;
    let start = Instant::now();
    let result = loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if cancel.is_cancelled() {
            let _ = window.eval("window.RclGame.stop()");
            break Err(CoreError::Cancelled);
        }
        let status: Option<serde_json::Value> = crate::settings::read_json(&directory.join("status.json"));
        if let Some(code) = status.as_ref().and_then(|s| s["code"].as_i64()) {
            break Ok(Some(code as i32));
        }
        let started = status.is_some();
        if started && heartbeat_stale(&directory, start) {
            // 正常结束时进程可能来不及写入退出码，由调用方检查输出判断是否成功
            break Ok(None);
        }
        if !started && start.elapsed() > Duration::from_secs(60) {
            let _ = window.eval("window.RclGame.stop()");
            break Err(CoreError::Java(format!("{}没有启动，请重试", job.label)));
        }
    };
    let log = std::fs::read_to_string(directory.join("java.log")).unwrap_or_default();
    let output: Vec<String> = log.lines().rev().take(40).map(str::to_string).collect::<Vec<_>>().into_iter().rev().collect();
    if !log.is_empty() {
        log::info!("[{}] 输出：\n{}", job.label, output.join("\n"));
    }
    let _ = std::fs::remove_dir_all(&directory);
    Ok(launcher_core::java::external::JavaOutcome { code: result?, output })
}

fn redact(line: &str, token: &str) -> String {
    if token.is_empty() { line.into() } else { line.replace(token, "********") }
}

fn emit_logs(app: &AppHandle, id: &str, directory: &std::path::Path, offset: &mut usize, token: &str) {
    if let Ok(mut file) = std::fs::File::open(directory.join("game.log")) {
        if file.seek(SeekFrom::Start(*offset as u64)).is_err() { return; }
        let mut bytes = Vec::new();
        if file.take(65536).read_to_end(&mut bytes).is_err() || bytes.is_empty() { return; }
        // 保留尚未读完的行，避免令牌跨读取边界时漏掉遮蔽。
        let Some(end) = bytes.iter().rposition(|b| *b == b'\n') else { return; };
        bytes.truncate(end + 1);
        let lines = String::from_utf8_lossy(&bytes).lines()
            .map(|text| LogLine { text: redact(text, token), stderr: false }).collect::<Vec<_>>();
        *offset += bytes.len();
        let _ = app.emit(GAME_EVENT, GameEvent::Log { instance: id.into(), lines });
    }
}
