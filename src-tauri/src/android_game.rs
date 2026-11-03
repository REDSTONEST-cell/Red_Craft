//! Android 游戏在独立的 JVM Activity 中运行；启动凭据只经过应用私有文件传递。

use std::path::PathBuf;
use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use launcher_core::auth::{AccountKind, LaunchAuth};
use launcher_core::download::CancelToken;
use launcher_core::install::{Reporter, Verify};
use launcher_core::instance::LoaderKind;
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
        // 游戏与安装器都在 :jvm 进程中运行，一个进程只能有一个 JVM
        if JAVA_JOBS.load(Ordering::SeqCst) > 0 {
            return Err(AppError::msg("正在运行安装器", "Forge / NeoForge 等安装完成后再启动游戏。"));
        }
        running.insert(id.clone(), 0);
    }
    let settings = state.settings();
    let config = InstanceConfig::load(&installer.game, &id);
    let session = uuid::Uuid::new_v4().to_string();
    let files = state.paths.config_dir.parent().ok_or_else(|| AppError::msg("无法找到应用数据目录", ""))?.to_path_buf();
    // 每次启动留下一个会话目录（含完整日志）；只保留最近几次
    prune_sessions(&files.join("game-session"), 4);
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
            let ctx = crate::content::instance_context(&state, &id)?;
            let game_dir = ctx.game_dir.clone();
            std::fs::create_dir_all(&game_dir).map_err(|e| AppError::msg(e.to_string(), "无法创建游戏目录"))?;
            if matches!(ctx.loader, Some(LoaderKind::Fabric | LoaderKind::Quilt)) {
                check_mods(&state, &ctx, &reporter, &cancel).await?;
            }
            if settings.default_chinese { launch::ensure_chinese(&game_dir, &resolved.json.release_time)?; }
            let device = launcher_core::system::memory();
            let memory = crate::memory::game_memory_mb(&state, Some(&id), device);
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
                // 写进游戏日志，便于排查卡顿
                "memory": {"heap": memory, "total": device.total_mb, "available": device.available_mb},
            });
            crate::settings::write_json(&directory.join("request.json"), &request)?;
            cancel.check()?;
            reporter.stage("准备手机运行时并启动游戏");
            let window = app2.get_webview_window("main").ok_or_else(|| AppError::msg("无法找到启动器界面", ""))?;
            window.eval(format!("window.RclGame.start('{}')", session)).map_err(|e| AppError::msg(e.to_string(), "无法打开游戏画面"))?;
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
                        let _ = window.eval(format!("window.RclGame.captureExit('{}', {})", session, pid));
                        // 系统退出记录在进程消失后异步产生，稍后再收集最后一段诊断。
                        tokio::time::sleep(Duration::from_millis(1500)).await;
                    }
                    // 把剩下的日志全部发给启动器（最多 8MB）
                    for _ in 0..128 {
                        if emit_logs(&app2, &id, &directory, &mut offset, &opts.auth.access_token) == 0 { break; }
                    }
                    let log = read_tail(&directory.join("game.log"), 1 << 20);
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

/// 启动前检查 Fabric Mod 是否支持当前游戏版本（与 FCL 启动前检查 Mod 相同的目的）：
/// Fabric API / QSL 不支持时自动换成支持的版本；其他不支持的 Mod 由玩家处理（游戏也无法加载它们）。
async fn check_mods(
    state: &AppState,
    ctx: &crate::content::InstanceContext,
    reporter: &Reporter,
    cancel: &CancelToken,
) -> AppResult<()> {
    let (dir, mc) = (ctx.game_dir.join("mods"), ctx.mc_version.clone());
    let bad = tokio::task::spawn_blocking(move || launcher_core::content::compat::incompatible_fabric_mods(&dir, &mc))
        .await
        .map_err(|e| AppError::msg(e.to_string(), ""))?;
    let api: &[&str] = match ctx.loader {
        Some(LoaderKind::Quilt) => &["qsl", "quilted_fabric_api"],
        _ => &["fabric-api", "fabric"],
    };
    let (apis, others): (Vec<_>, Vec<_>) = bad.into_iter().partition(|m| api.contains(&m.id.as_str()));
    if !others.is_empty() {
        let list: Vec<String> = others.iter().map(|m| format!("{} {}（需要 Minecraft {}）", m.name, m.version, m.requires)).collect();
        return Err(AppError::msg(
            format!("{} 个 Mod 不支持 Minecraft {}", others.len(), ctx.mc_version),
            format!("{}。请在「资源管理」中删除这些 Mod，或换成支持 {} 的版本。", list.join("、"), ctx.mc_version),
        ));
    }
    if apis.is_empty() {
        return Ok(());
    }
    // 先把不支持的文件移出 mods 文件夹（否则会被当作已安装而跳过），安装失败时放回原处
    let name = if ctx.loader == Some(LoaderKind::Quilt) { "QSL" } else { "Fabric API" };
    reporter.stage(&format!("{name} {} 不支持 Minecraft {}，正在更换", apis[0].version, ctx.mc_version));
    let backup = state.paths.config_dir.join("cache").join("replaced-mods");
    std::fs::create_dir_all(&backup).map_err(|e| AppError::msg(e.to_string(), "无法更换 Mod"))?;
    let mut moved = Vec::new();
    for m in &apis {
        let to = backup.join(m.file.file_name().unwrap_or_default());
        std::fs::rename(&m.file, &to).map_err(|e| AppError::msg(format!("无法移除不支持的 {name}：{e}"), "请在「资源管理」中手动删除它。"))?;
        moved.push((m.file.clone(), to));
    }
    match crate::content::install_loader_api(state, &ctx.id, reporter, cancel).await {
        Ok(()) => {
            for (_, to) in &moved {
                let _ = std::fs::remove_file(to);
            }
            log::info!("已把 {} 中不支持 Minecraft {} 的 {name} 换成支持的版本", ctx.id, ctx.mc_version);
            Ok(())
        }
        Err(e) => {
            for (from, to) in &moved {
                let _ = std::fs::rename(to, from);
            }
            Err(AppError::msg(
                format!("{name} {} 不支持 Minecraft {}，自动更换失败：{}", apis[0].version, ctx.mc_version, e.info().message),
                format!("请在「资源管理」中删除它，然后安装支持 {} 的 {name}。", ctx.mc_version),
            ))
        }
    }
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

/// 正在运行（含排队）的 Java 程序数量。
static JAVA_JOBS: AtomicUsize = AtomicUsize::new(0);

/// Java 程序依次运行：每次运行都会结束上一个 :jvm 进程，同时运行两个会互相结束。
fn java_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

async fn run_java(
    app: AppHandle,
    job: launcher_core::java::external::JavaJob,
    cancel: launcher_core::download::CancelToken,
) -> launcher_core::Result<launcher_core::java::external::JavaOutcome> {
    use launcher_core::CoreError;
    let state = app.state::<AppState>();
    // 运行安装器会结束 :jvm 进程，游戏正在运行时不能运行；与启动游戏时的检查在同一把锁中进行
    {
        let running = state.running.lock().map_err(|_| CoreError::Java("无法读取游戏状态".into()))?;
        if !running.is_empty() {
            return Err(CoreError::Java(format!("游戏正在运行，请先退出游戏再运行{}", job.label)));
        }
        JAVA_JOBS.fetch_add(1, Ordering::SeqCst);
    }
    let _count = scopeguard(|| {
        JAVA_JOBS.fetch_sub(1, Ordering::SeqCst);
    });
    let _turn = java_lock().lock().await;
    let files = state.paths.config_dir.parent().ok_or_else(|| CoreError::Java("无法找到应用数据目录".into()))?.to_path_buf();
    // 依次运行，此时没有其他 Java 程序在运行：清理之前异常中断留下的目录
    prune_sessions(&files.join("java-session"), 0);
    let session = uuid::Uuid::new_v4().to_string();
    let directory = files.join("java-session").join(&session);
    let java = job.java.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "java21".into());
    let request = serde_json::json!({"java": java, "args": job.args, "cwd": job.cwd});
    crate::settings::write_json(&directory.join("request.json"), &request).map_err(|e| CoreError::Java(e.to_string()))?;
    let window = app.get_webview_window("main").ok_or_else(|| CoreError::Java("无法找到启动器界面".into()))?;
    window.eval(format!("window.RclGame.runJava('{session}')")).map_err(|e| CoreError::Java(format!("无法运行{}：{e}", job.label)))?;
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
    let log = read_tail(&directory.join("java.log"), 1 << 20);
    let output: Vec<String> = log.lines().rev().take(40).map(str::to_string).collect::<Vec<_>>().into_iter().rev().collect();
    if !log.is_empty() {
        log::info!("[{}] 输出：\n{}", job.label, output.join("\n"));
    }
    let _ = std::fs::remove_dir_all(&directory);
    Ok(launcher_core::java::external::JavaOutcome { code: result?, output })
}

/// 离开作用域时执行（包括任务被取消时）。
fn scopeguard(f: impl FnOnce()) -> impl Drop {
    struct Guard<F: FnOnce()>(Option<F>);
    impl<F: FnOnce()> Drop for Guard<F> {
        fn drop(&mut self) {
            if let Some(f) = self.0.take() {
                f();
            }
        }
    }
    Guard(Some(f))
}

fn redact(line: &str, token: &str) -> String {
    if token.is_empty() { line.into() } else { line.replace(token, "********") }
}

/// 把新增的游戏日志发给启动器，返回这次读取的字节数。
fn emit_logs(app: &AppHandle, id: &str, directory: &std::path::Path, offset: &mut usize, token: &str) -> usize {
    let Ok(mut file) = std::fs::File::open(directory.join("game.log")) else { return 0 };
    if file.seek(SeekFrom::Start(*offset as u64)).is_err() { return 0; }
    let mut bytes = Vec::new();
    if file.take(65536).read_to_end(&mut bytes).is_err() || bytes.is_empty() { return 0; }
    // 保留尚未读完的行，避免令牌跨读取边界时漏掉遮蔽。
    let Some(end) = bytes.iter().rposition(|b| *b == b'\n') else { return 0 };
    bytes.truncate(end + 1);
    let lines = String::from_utf8_lossy(&bytes).lines()
        .map(|text| LogLine { text: redact(text, token), stderr: false }).collect::<Vec<_>>();
    *offset += bytes.len();
    let _ = app.emit(GAME_EVENT, GameEvent::Log { instance: id.into(), lines });
    bytes.len()
}

/// 读取文件最后 `max` 字节（从完整的一行开始），游戏日志可能很大。
fn read_tail(path: &std::path::Path, max: u64) -> String {
    let Ok(mut file) = std::fs::File::open(path) else { return String::new() };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let cut = len > max && file.seek(SeekFrom::Start(len - max)).is_ok();
    let mut bytes = Vec::new();
    let _ = file.take(max).read_to_end(&mut bytes);
    if cut {
        if let Some(i) = bytes.iter().position(|b| *b == b'\n') { bytes.drain(..=i); }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// 只保留最近修改的 `keep` 个会话目录。
fn prune_sessions(parent: &std::path::Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(parent) else { return };
    let mut dirs: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| (e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), e.path()))
        .collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.0));
    for (_, dir) in dirs.into_iter().skip(keep) {
        let _ = std::fs::remove_dir_all(dir);
    }
}
