//! 联机：集成 Terracotta | 陶瓦联机（https://github.com/burningtnt/Terracotta）。
//!
//! 陶瓦基于 EasyTier 组建虚拟局域网，并实现了 Scaffolding 联机协议，与 HMCL、PCL CE 等启动器互通。
//! 集成方式遵循陶瓦许可证（AGPL-3.0-or-later）的例外条款：启动器内嵌**未经修改**的官方发行包，
//! 以独立进程运行，只通过它提供的本机 HTTP 接口交互，并在联机页面显著位置标明陶瓦的版权信息。
//!
//! 流程：
//! - 第一次使用时把嵌入的发行包解压到配置目录（macOS 需先安装陶瓦官方安装包，见 `multiplayer_install`）；
//! - 以 `--hmcl <端口文件>` 模式启动，陶瓦把本机 HTTP 端口写入端口文件；
//! - 房主：`/state/scanning`，陶瓦找到已「对局域网开放」的游戏后生成房间号；
//! - 房客：`/state/guesting?room=…`，连上后在本机提供 `127.0.0.1:端口` 并广播到游戏的局域网列表；
//! - 退出启动器时调用 `/panic?peaceful=true` 让陶瓦正常退出。

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use launcher_core::error::IoContext;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// 嵌入的陶瓦版本（与 scripts/fetch-terracotta.sh 一致）
pub const VERSION: &str = "0.4.2";

#[cfg(all(terracotta_bundled, not(mobile)))]
const PACKAGE: Option<&[u8]> = Some(include_bytes!(env!("RCL_TERRACOTTA_PKG")));
#[cfg(not(all(terracotta_bundled, not(mobile))))]
const PACKAGE: Option<&[u8]> = None;

/// macOS：陶瓦需要通过官方安装包安装（后台服务 + 本地网络权限）
#[cfg(target_os = "macos")]
const MAC_APP: &str = "/Applications/terracotta.app/Contents/MacOS/terracotta";

/// 正在运行的陶瓦的本机 HTTP 端口
static PORT: Mutex<Option<u16>> = Mutex::new(None);
/// 同一时间只启动一次
static STARTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn port() -> Option<u16> {
    PORT.lock().ok().and_then(|p| *p)
}

fn set_port(value: Option<u16>) {
    if let Ok(mut p) = PORT.lock() {
        *p = value;
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    /// 这个版本的启动器能否使用联机
    pub available: bool,
    /// macOS：需要先安装陶瓦官方安装包
    pub needs_install: bool,
    pub reason: Option<String>,
    pub version: &'static str,
}

fn info() -> Info {
    if cfg!(mobile) {
        return Info { available: false, needs_install: false, reason: Some("手机版暂不支持联机".into()), version: VERSION };
    }
    if PACKAGE.is_none() {
        return Info {
            available: false,
            needs_install: false,
            reason: Some("这个版本的启动器没有包含联机组件".into()),
            version: VERSION,
        };
    }
    #[cfg(target_os = "macos")]
    let needs_install = !Path::new(MAC_APP).is_file();
    #[cfg(not(target_os = "macos"))]
    let needs_install = false;
    Info { available: true, needs_install, reason: None, version: VERSION }
}

// ───────────── 房间号 ─────────────

/// Scaffolding 房间号的字符集（不含容易混淆的 I、O）
const CHARS: &[u8] = b"0123456789ABCDEFGHJKLMNPQRSTUVWXYZ";

/// 规范化并校验房间号（与陶瓦的解析规则一致）：`U/XXXX-XXXX-XXXX-XXXX`，
/// 可以省略 `U/`、大小写不限、I/O 视为 1/0，按 34 进制（第一位为最低位）换算后须能被 7 整除。
pub fn normalize_room(input: &str) -> Option<String> {
    let text: String = input.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_uppercase();
    let body = text.strip_prefix("U/").unwrap_or(&text);
    let chars: Vec<char> = body.chars().collect();
    if chars.len() != 19 {
        return None;
    }
    let mut value: u128 = 0;
    let mut out = String::with_capacity(21);
    for (i, c) in chars.iter().enumerate().rev() {
        if i == 4 || i == 9 || i == 14 {
            if *c != '-' {
                return None;
            }
            continue;
        }
        let c = match c {
            'I' => '1',
            'O' => '0',
            c => *c,
        };
        let digit = CHARS.iter().position(|x| *x as char == c)?;
        value = value * 34 + digit as u128;
    }
    if !value.is_multiple_of(7) {
        return None;
    }
    out.push_str("U/");
    out.extend(chars.iter().map(|c| match c {
        'I' => '1',
        'O' => '0',
        c => *c,
    }));
    Some(out)
}

// ───────────── 安装与启动 ─────────────

fn install_dir(state: &AppState) -> PathBuf {
    state.paths.config_dir.join("terracotta").join(VERSION)
}

/// 解压后的陶瓦可执行文件（发行包里另有 Windows 运行库 VCRUNTIME140.DLL、macOS 安装包）
fn find_executable(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        p.is_file()
            && name.starts_with("terracotta-")
            && !name.ends_with(".pkg")
            && (cfg!(not(windows)) || name.ends_with(".exe"))
    })
}

/// 把嵌入的发行包解压到 `dir`（已解压过则直接返回）。只接受包内顶层的普通文件。
pub fn unpack(dir: &Path, package: &[u8]) -> AppResult<PathBuf> {
    if dir.join(".complete").is_file() {
        if let Some(exe) = find_executable(dir) {
            return Ok(exe);
        }
    }
    let tmp = dir.with_extension("tmp");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).at(&tmp)?;
    let broken = |e: std::io::Error| AppError::msg(format!("联机组件解压失败：{e}"), "请重新下载启动器");
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(package));
    for entry in archive.entries().map_err(broken)? {
        let mut entry = entry.map_err(broken)?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().map_err(broken)?.into_owned();
        let mut parts = path.components();
        let (Some(std::path::Component::Normal(name)), None) = (parts.next(), parts.next()) else {
            continue;
        };
        let dest = tmp.join(name);
        let mut out = std::fs::File::create(&dest).at(&dest)?;
        std::io::copy(&mut entry, &mut out).at(&dest)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755)).at(&dest)?;
        }
    }
    std::fs::write(tmp.join(".complete"), VERSION).at(&tmp)?;
    let _ = std::fs::remove_dir_all(dir);
    std::fs::rename(&tmp, dir).at(dir)?;
    find_executable(dir).ok_or_else(|| AppError::msg("联机组件不完整", "请重新下载启动器"))
}

fn executable(state: &AppState) -> AppResult<PathBuf> {
    let info = info();
    if !info.available {
        return Err(AppError::msg(info.reason.unwrap_or_default(), "请下载最新的电脑版启动器"));
    }
    #[cfg(target_os = "macos")]
    {
        if !Path::new(MAC_APP).is_file() {
            return Err(AppError::msg("还没有安装陶瓦联机组件", "请在联机页面点击「安装联机组件」"));
        }
        let _ = state;
        Ok(PathBuf::from(MAC_APP))
    }
    #[cfg(not(target_os = "macos"))]
    unpack(&install_dir(state), PACKAGE.unwrap_or_default())
}

fn client() -> AppResult<reqwest::Client> {
    // 只访问本机，绕过系统代理
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| AppError::msg(format!("无法连接联机组件：{e}"), ""))
}

async fn request(port: u16, path: &str, query: &[(&str, &str)]) -> AppResult<(u16, String)> {
    let response = client()?
        .get(format!("http://127.0.0.1:{port}{path}"))
        .query(query)
        .send()
        .await
        .map_err(|e| AppError::msg(format!("联机组件没有响应：{e}"), "请稍后重试"))?;
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    Ok((status, body))
}

async fn alive(port: u16) -> bool {
    matches!(request(port, "/meta", &[]).await, Ok((200, _)))
}

/// 确保陶瓦正在运行，返回它的 HTTP 端口。
async fn ensure(app: &AppHandle) -> AppResult<u16> {
    let _guard = STARTING.lock().await;
    if let Some(port) = port() {
        if alive(port).await {
            return Ok(port);
        }
        set_port(None);
    }
    let app2 = app.clone();
    let (exe, port_file) = crate::commands::blocking(move || -> AppResult<(PathBuf, PathBuf)> {
        let state = app2.state::<AppState>();
        let exe = executable(&state)?;
        let dir = state.paths.config_dir.join("terracotta");
        std::fs::create_dir_all(&dir).at(&dir)?;
        let port_file = dir.join("port.json");
        let _ = std::fs::remove_file(&port_file);
        let mut command = std::process::Command::new(&exe);
        command
            .arg("--hmcl")
            .arg(&port_file)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if let Some(parent) = exe.parent() {
            command.current_dir(parent);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // 不弹出控制台窗口
            command.creation_flags(0x0800_0000);
        }
        let mut child = command
            .spawn()
            .map_err(|e| AppError::msg(format!("无法启动联机组件：{e}"), "请检查是否被安全软件拦截"))?;
        // 回收子进程，避免留下僵尸进程（Windows 上这是引导进程，Linux 上就是陶瓦本身）
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok((exe, port_file))
    })
    .await??;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let Ok(text) = std::fs::read_to_string(&port_file) else { continue };
        let Some(port) = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["port"].as_u64()) else { continue };
        let port = port as u16;
        if alive(port).await {
            log::info!("陶瓦联机 {} 已启动，端口 {port}（{}）", VERSION, exe.display());
            set_port(Some(port));
            return Ok(port);
        }
    }
    Err(AppError::msg("联机组件没有启动", "请检查是否被安全软件拦截，然后重试"))
}

/// 回到空闲状态（离开房间 / 关闭房间）
async fn idle(port: u16) {
    let _ = request(port, "/state/ide", &[]).await;
}

// ───────────── 命令 ─────────────

#[tauri::command]
pub fn multiplayer_info() -> Info {
    info()
}

/// 陶瓦当前状态（`/state` 原样返回）；陶瓦没有运行时为 `{"state": "stopped"}`。
#[tauri::command]
pub async fn multiplayer_state() -> AppResult<Value> {
    let Some(port) = port() else { return Ok(json!({ "state": "stopped" })) };
    match request(port, "/state", &[]).await {
        Ok((200, body)) => Ok(serde_json::from_str(&body).unwrap_or_else(|_| json!({ "state": "stopped" }))),
        _ => {
            set_port(None);
            Ok(json!({ "state": "stopped" }))
        }
    }
}

/// 房主：创建房间（陶瓦会等待本机游戏「对局域网开放」，然后生成房间号）
#[tauri::command]
pub async fn multiplayer_host(app: AppHandle, player: Option<String>) -> AppResult<()> {
    let port = ensure(&app).await?;
    idle(port).await;
    let player = player.unwrap_or_default();
    let mut query = Vec::new();
    if !player.trim().is_empty() {
        query.push(("player", player.trim()));
    }
    match request(port, "/state/scanning", &query).await? {
        (200, _) => Ok(()),
        (code, _) => Err(AppError::msg(format!("创建房间失败（{code}）"), "请稍后重试")),
    }
}

/// 房客：输入房间号加入
#[tauri::command]
pub async fn multiplayer_join(app: AppHandle, room: String, player: Option<String>) -> AppResult<String> {
    let room = normalize_room(&room)
        .ok_or_else(|| AppError::msg("房间号不正确", "房间号形如 U/XXXX-XXXX-XXXX-XXXX，请检查是否输错"))?;
    let port = ensure(&app).await?;
    idle(port).await;
    let player = player.unwrap_or_default();
    let mut query = vec![("room", room.as_str())];
    if !player.trim().is_empty() {
        query.push(("player", player.trim()));
    }
    match request(port, "/state/guesting", &query).await? {
        (200, _) => Ok(room),
        (400, _) => Err(AppError::msg("房间号不正确", "请向房主确认房间号")),
        (code, _) => Err(AppError::msg(format!("加入房间失败（{code}）"), "请稍后重试")),
    }
}

/// 离开房间或关闭房间
#[tauri::command]
pub async fn multiplayer_leave() -> AppResult<()> {
    if let Some(port) = port() {
        idle(port).await;
    }
    Ok(())
}

/// macOS：打开嵌入的陶瓦官方安装包（安装后台服务，需要管理员授权）
#[tauri::command]
pub async fn multiplayer_install(app: AppHandle) -> AppResult<()> {
    #[cfg(target_os = "macos")]
    {
        return crate::commands::blocking(move || {
            let state = app.state::<AppState>();
            let package = PACKAGE.ok_or_else(|| AppError::msg("这个版本的启动器没有包含联机组件", ""))?;
            let dir = install_dir(&state);
            unpack(&dir, package).ok();
            let pkg = std::fs::read_dir(&dir)
                .at(&dir)?
                .flatten()
                .map(|e| e.path())
                .find(|p| p.extension().is_some_and(|e| e == "pkg"))
                .ok_or_else(|| AppError::msg("联机组件安装包缺失", "请重新下载启动器"))?;
            std::process::Command::new("open")
                .arg(&pkg)
                .spawn()
                .map_err(|e| AppError::msg(format!("无法打开安装包：{e}"), ""))?;
            Ok(())
        })
        .await?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Err(AppError::msg("只有 macOS 需要单独安装联机组件", ""))
    }
}

// ───────────── 日志与诊断 ─────────────

/// 陶瓦的数据目录：每次启动在这里建一个「日期-时间-进程号」文件夹，日志为其中的 application.log
fn terracotta_root() -> PathBuf {
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join("terracotta");
    }
    std::env::temp_dir().join("terracotta")
}

/// 最近一次运行的陶瓦日志文件
fn latest_log_file() -> Option<PathBuf> {
    std::fs::read_dir(terracotta_root())
        .ok()?
        .flatten()
        .map(|e| e.path().join("application.log"))
        .filter(|p| p.is_file())
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

/// 陶瓦日志：陶瓦在运行时从它的接口读取当前进程的日志，否则读最近一次的日志文件
async fn log_text() -> Option<String> {
    if let Some(port) = port() {
        if let Ok((200, body)) = request(port, "/log", &[("fetch", "true")]).await {
            if !body.is_empty() {
                return Some(body);
            }
        }
    }
    let file = latest_log_file()?;
    let bytes = std::fs::read(file).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    /// 本机无法在房主的游戏端口上建立转发（Windows 上多为该端口被系统保留）
    pub port_blocked: bool,
    /// 房主的游戏端口
    pub port: Option<u16>,
}

/// 从陶瓦日志找出加入失败的具体原因
pub fn diagnose(log: &str) -> Diagnosis {
    // 只看最后一次加入房间的部分
    let tail = log.rfind("Connecting to room").map_or(log, |i| &log[i..]);
    let marker = "Cannot create a port-forward ";
    let Some(line) = tail.lines().rev().find(|l| l.contains(marker) && l.contains("for MC Connection")) else {
        return Diagnosis::default();
    };
    // 形如：Cannot create a port-forward 61823 -> 61823 for MC Connection.
    let port = line
        .split("->")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|p| p.parse().ok());
    Diagnosis { port_blocked: true, port }
}

/// 分析最近一次联机失败的原因
#[tauri::command]
pub async fn multiplayer_diagnose() -> AppResult<Diagnosis> {
    Ok(log_text().await.map(|log| diagnose(&log)).unwrap_or_default())
}

// ───────────── 连接方式 ─────────────

#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    /// "IPv4" / "IPv6"；无法判断时为空
    pub ip_version: Option<&'static str>,
    /// 对方是局域网地址（同一个路由器下）
    pub lan: bool,
    /// 隧道协议：tcp / udp / quic / wg 等
    pub protocol: String,
    /// 与房主之间的延迟（毫秒）
    pub latency_ms: Option<f64>,
    /// 经过其他成员中转，而不是与房主直连
    pub relayed: bool,
}

/// 从日志取出当前这次加入所用的组件信息：EasyTier 所在目录、RPC 端口、房主的虚拟 IP
fn link_context(log: &str) -> Option<(PathBuf, u16, Option<std::net::Ipv4Addr>)> {
    let dir = log
        .lines()
        .rev()
        .find_map(|l| l.split("Releasing easytier to ").nth(1))
        .map(|p| PathBuf::from(p.trim()))?;
    // 只看最后一次加入房间的部分
    let tail = log.rfind("Connecting to room").map_or(log, |i| &log[i..]);
    let rpc = tail
        .lines()
        .rev()
        .find_map(|l| l.rsplit_once("rpc=").and_then(|(_, p)| p.trim().parse().ok()))?;
    let host = tail.lines().rev().find_map(|l| {
        let rest = l.split("Scaffolding Server is at ").nth(1)?;
        rest.trim().split(':').next()?.parse().ok()
    });
    Some((dir, rpc, host))
}

/// 解析 EasyTier 地址（如 udp://[240e::1]:1234、tcp://1.2.3.4:5）里的 IP
fn url_ip(url: &str) -> Option<std::net::IpAddr> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = if let Some(v6) = rest.strip_prefix('[') {
        v6.split(']').next()?
    } else {
        rest.split(['/', ':']).next()?
    };
    let ip: std::net::IpAddr = host.parse().ok()?;
    Some(match ip {
        std::net::IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, std::net::IpAddr::V4),
        v4 => v4,
    })
}

fn is_lan(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.is_loopback(),
        std::net::IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            // fc00::/7 唯一本地地址、fe80::/10 链路本地地址
            (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80 || v6.is_loopback()
        }
    }
}

/// 从 `easytier-cli -v -o json peer list` 的输出里找出与房主之间的连接
pub fn link_from_peers(peers: &Value, host: Option<std::net::Ipv4Addr>) -> Option<Link> {
    let pairs = peers.as_array()?;
    let is_public = |p: &Value| {
        p.pointer("/route/hostname")
            .and_then(Value::as_str)
            .is_some_and(|h| h.starts_with("PublicServer_"))
    };
    let pair = host
        .and_then(|ip| {
            let want = u32::from(ip);
            pairs.iter().find(|p| p.pointer("/route/ipv4_addr/address/addr").and_then(Value::as_u64) == Some(want.into()))
        })
        .or_else(|| pairs.iter().find(|p| !is_public(p) && p.get("peer").is_some_and(|v| !v.is_null())))?;

    let conns: Vec<&Value> = pair
        .pointer("/peer/conns")
        .and_then(Value::as_array)
        .map(|c| c.iter().filter(|c| c.get("is_closed").and_then(Value::as_bool) != Some(true)).collect())
        .unwrap_or_default();
    if conns.is_empty() {
        // 没有直接连接，经由其他节点转发
        let latency = pair.pointer("/route/path_latency").and_then(Value::as_f64).filter(|l| *l > 0.0);
        return Some(Link { relayed: true, latency_ms: latency, ..Link::default() });
    }
    // EasyTier 当前使用的连接（default_conn_id 拆成 4 段 u32 存储）
    let default_id = pair.pointer("/peer/default_conn_id").and_then(|u| {
        let part = |k: &str| u.get(k).and_then(Value::as_u64);
        Some(format!("{:08x}{:08x}{:08x}{:08x}", part("part1")?, part("part2")?, part("part3")?, part("part4")?))
    });
    let conn = default_id
        .and_then(|id| {
            conns.iter().find(|c| c.get("conn_id").and_then(Value::as_str).map(|s| s.replace('-', "").to_lowercase()) == Some(id.clone()))
        })
        .unwrap_or(&conns[0]);

    let tunnel = conn.get("tunnel");
    let url = |k: &str| tunnel.and_then(|t| t.pointer(&format!("/{k}/url"))).and_then(Value::as_str);
    let ip = url("remote_addr").and_then(url_ip).or_else(|| url("local_addr").and_then(url_ip));
    let protocol = tunnel
        .and_then(|t| t.get("tunnel_type"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| url("remote_addr").and_then(|u| u.split_once("://")).map(|(s, _)| s.to_string()))
        .unwrap_or_default();
    let latency = conn
        .pointer("/stats/latency_us")
        .and_then(Value::as_f64)
        .filter(|l| *l > 0.0)
        .map(|us| us / 1000.0);
    Some(Link {
        ip_version: ip.map(|ip| if ip.is_ipv6() { "IPv6" } else { "IPv4" }),
        lan: ip.is_some_and(is_lan),
        protocol,
        latency_ms: latency,
        relayed: false,
    })
}

/// 查询加入房间后与房主之间的连接方式（IPv4 / IPv6、协议、延迟）
#[tauri::command]
pub async fn multiplayer_link() -> AppResult<Option<Link>> {
    let Some(log) = log_text().await else { return Ok(None) };
    let Some((dir, rpc, host)) = link_context(&log) else { return Ok(None) };
    let cli = std::fs::read_dir(&dir).ok().and_then(|entries| {
        entries
            .flatten()
            .map(|e| e.path())
            .find(|p| p.file_stem().is_some_and(|s| s == "easytier-cli"))
    });
    let Some(cli) = cli else { return Ok(None) };
    let mut command = tokio::process::Command::new(cli);
    command
        .args(["-p", &format!("127.0.0.1:{rpc}"), "-v", "-o", "json", "peer", "list"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let Ok(Ok(output)) = tokio::time::timeout(Duration::from_secs(4), command.output()).await else {
        return Ok(None);
    };
    let Ok(peers) = serde_json::from_slice::<Value>(&output.stdout) else { return Ok(None) };
    Ok(link_from_peers(&peers, host))
}

/// 导出陶瓦日志到启动器的 logs 文件夹（与启动器日志放在一起），并打开该文件夹
#[tauri::command]
pub async fn multiplayer_export_log(app: AppHandle) -> AppResult<String> {
    let text = log_text()
        .await
        .ok_or_else(|| AppError::msg("还没有联机日志", "请先使用一次联机功能"))?;
    let state = app.state::<AppState>();
    let dir = state.paths.config_dir.join("logs");
    std::fs::create_dir_all(&dir).at(&dir)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let file = dir.join(format!("terracotta-{stamp}.log"));
    std::fs::write(&file, text).at(&file)?;
    crate::commands::open_path(&app, &dir)?;
    Ok(file.display().to_string())
}

/// 退出启动器时让陶瓦正常退出（房间随之关闭）。
pub fn shutdown() {
    let Some(port) = port() else { return };
    set_port(None);
    use std::io::Write;
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    if let Ok(mut stream) = std::net::TcpStream::connect_timeout(&address, Duration::from_millis(500)) {
        let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
        let _ = stream.write_all(b"GET /panic?peaceful=true HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
        let _ = stream.flush();
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 与陶瓦 `from_value` 相同的编码方式，用于构造测试房间号
    fn encode(mut value: u128) -> String {
        let mut code = String::from("U/");
        for i in 0..16 {
            if i == 4 || i == 8 || i == 12 {
                code.push('-');
            }
            code.push(CHARS[(value % 34) as usize] as char);
            value /= 34;
        }
        code
    }

    #[test]
    fn room_codes_are_validated_like_terracotta() {
        let code = encode(7 * 123_456_789_012_345);
        assert_eq!(normalize_room(&code).as_deref(), Some(code.as_str()));
        // 省略 U/、小写、多余空格都可以
        assert_eq!(normalize_room(&format!("  {}  ", code[2..].to_lowercase())).as_deref(), Some(code.as_str()));
        // 真实陶瓦 0.4.2 生成的房间号
        assert_eq!(normalize_room("U/0AMH-FGZJ-EF82-C6CA").as_deref(), Some("U/0AMH-FGZJ-EF82-C6CA"));
        // 校验位不对（陶瓦对它返回 400）
        assert!(normalize_room("U/AAAA-AAAA-AAAA-AAAB").is_none());
        assert!(normalize_room(&encode(7 * 123_456_789_012_345 + 1)).is_none());
        // 格式不对
        assert!(normalize_room("U/ABCD-EFGH").is_none());
        assert!(normalize_room("U/ABCD_EFGH_JKLM_NPQR").is_none());
        // I / O 视为 1 / 0
        let with_one = encode(7 * 34_u128.pow(3) + 7);
        let tricky = with_one.replace('1', "I").replace('0', "O");
        assert_eq!(normalize_room(&tricky).as_deref(), Some(with_one.as_str()));
    }

    /// 用嵌入的真实陶瓦发行包走一遍：解压、`--hmcl` 启动、读端口、房主开始寻找游戏、回到空闲、正常退出。
    #[cfg(all(terracotta_bundled, target_os = "linux"))]
    #[test]
    fn bundled_terracotta_starts_and_answers() {
        let dir = tempfile::tempdir().unwrap();
        let exe = unpack(&dir.path().join(VERSION), PACKAGE.unwrap()).unwrap();
        let port_file = dir.path().join("port.json");
        let mut child = std::process::Command::new(&exe).arg("--hmcl").arg(&port_file).spawn().unwrap();
        let port = (0..100)
            .find_map(|_| {
                std::thread::sleep(Duration::from_millis(200));
                let text = std::fs::read_to_string(&port_file).ok()?;
                serde_json::from_str::<Value>(&text).ok()?["port"].as_u64()
            })
            .expect("陶瓦没有写出端口") as u16;
        set_port(Some(port));
        tauri::async_runtime::block_on(async {
            assert!(alive(port).await);
            let state = multiplayer_state().await.unwrap();
            assert_eq!(state["state"], "waiting");
            assert_eq!(request(port, "/state/scanning", &[("player", "测试")]).await.unwrap().0, 200);
            assert_eq!(multiplayer_state().await.unwrap()["state"], "host-scanning");
            multiplayer_leave().await.unwrap();
            assert_eq!(multiplayer_state().await.unwrap()["state"], "waiting");
            assert_eq!(request(port, "/state/guesting", &[("room", "U/AAAA-AAAA-AAAA-AAAB")]).await.unwrap().0, 400);
            // 能读到当前陶瓦进程的日志
            let log = log_text().await.expect("读不到陶瓦日志");
            assert!(log.contains("Welcome using Terracotta"), "{log}");
            assert_eq!(multiplayer_diagnose().await.unwrap(), Diagnosis::default());
            // 没有加入房间时没有连接信息
            assert_eq!(multiplayer_link().await.unwrap(), None);
        });
        shutdown();
        let status = (0..50).find_map(|_| {
            std::thread::sleep(Duration::from_millis(100));
            child.try_wait().ok().flatten()
        });
        assert!(status.is_some(), "陶瓦没有退出");
        assert_eq!(super::port(), None);
    }

    #[test]
    fn diagnosis_finds_the_blocked_port_in_the_last_attempt() {
        // 摘自真实日志：Windows 上 UDP / IPv6 转发被拒绝（os error 10013）
        let failed = "[Core]: Connecting to room, code=U/BCXP-MTR9-XHGJ-ET72\n\
            [RoomExperiment]: MC server is at 61823\n\
            [EasyTier CLI]:     2: 嘗試存取通訊端被拒絕，因為存取權限不足。 (os error 10013)\n\
            [EasyTier CLI]: Cannot adding port-forward rules: [::]:61823 -> 10.144.144.1:61823 (udp)\n\
            [RoomExperiment]: Cannot create a port-forward 61823 -> 61823 for MC Connection.\n\
            [State]: Switch to AppState::Exception { kind: GuestEasytierCrash }\n";
        assert_eq!(diagnose(failed), Diagnosis { port_blocked: true, port: Some(61823) });
        // 之后又成功加入了一次：以最后一次为准
        let later = format!("{failed}[Core]: Connecting to room, code=U/AAAA-AAAA-AAAA-AAAA\n[State]: Switch to AppState::GuestOk\n");
        assert_eq!(diagnose(&later), Diagnosis::default());
        assert_eq!(diagnose(""), Diagnosis::default());
    }

    #[test]
    fn link_reports_ip_version_of_the_connection_to_the_host() {
        // 摘自真实日志
        let log = "[Easytier]: Releasing easytier to C:\\Users\\ADMINI~1\\AppData\\Local\\Temp\\terracotta\\2026-11-25-21-39-47-10972\\embedded-easytier\n\
            [Core]: Connecting to room, code=U/BCXP-MTR9-XHGJ-ET72\n\
            [Easytier]: Starting easytier: [\"--network-name\", \"scaffolding-mc-BCXP-MTR9\"], rpc=35823\n\
            [RoomExperiment]: Scaffolding Server is at 10.144.144.1:13448\n";
        let (dir, rpc, host) = link_context(log).unwrap();
        assert!(dir.to_string_lossy().ends_with("10972\\embedded-easytier"));
        assert_eq!(rpc, 35823);
        assert_eq!(host, Some(std::net::Ipv4Addr::new(10, 144, 144, 1)));
        assert_eq!(link_context("[UI]: Welcome"), None);

        // `easytier-cli -v -o json peer list` 的输出结构（EasyTier 2.5.0 的 PeerRoutePair）
        let peers = json!([
            {
                "route": { "peer_id": 294518607, "hostname": "PublicServer_node1", "ipv4_addr": null, "path_latency": 30 },
                "peer": { "peer_id": 294518607, "conns": [
                    { "conn_id": "11111111-2222-3333-4444-555555555555", "is_closed": false,
                      "tunnel": { "tunnel_type": "BodyUrls-tcp://183.66.27.22:39409",
                                  "local_addr": { "url": "tcp://192.168.0.103:35839" },
                                  "remote_addr": { "url": "https://etnode.zkitefly.eu.org/node1" } } }
                ] }
            },
            {
                "route": { "peer_id": 597283946, "hostname": "host-pc", "path_latency": 12,
                           "ipv4_addr": { "address": { "addr": 0x0a90_9001u32 }, "network_length": 24 } },
                "peer": { "peer_id": 597283946,
                  "default_conn_id": { "part1": 0x6ba7_b810u32, "part2": 0x9dad_11d1u32, "part3": 0x80b4_00c0u32, "part4": 0x4fd4_30c8u32 },
                  "conns": [
                    { "conn_id": "aaaaaaaa-0000-0000-0000-000000000000", "is_closed": false,
                      "stats": { "latency_us": 80000 },
                      "tunnel": { "tunnel_type": "udp",
                                  "local_addr": { "url": "udp://192.168.0.103:51254" },
                                  "remote_addr": { "url": "udp://36.1.2.3:40000" } } },
                    { "conn_id": "6ba7b810-9dad-11d1-80b4-00c04fd430c8", "is_closed": false,
                      "stats": { "latency_us": 23500 },
                      "tunnel": { "tunnel_type": "tcp",
                                  "local_addr": { "url": "tcp://[240e:3b9:34b3:30c0:e581:2bfb:9dd9:ef4]:35851" },
                                  "remote_addr": { "url": "tcp://[2409:8a20:1462:2a37:45bb:1a9d:f22f:533]:58202" } } }
                  ] }
            }
        ]);
        let host = Some(std::net::Ipv4Addr::new(10, 144, 144, 1));
        assert_eq!(
            link_from_peers(&peers, host),
            Some(Link { ip_version: Some("IPv6"), lan: false, protocol: "tcp".into(), latency_ms: Some(23.5), relayed: false })
        );
        // 不知道房主的虚拟 IP 时，跳过公共节点
        assert_eq!(link_from_peers(&peers, None).unwrap().ip_version, Some("IPv6"));

        // 局域网 IPv4、IPv4 映射地址
        assert!(is_lan(url_ip("udp://192.168.1.5:1").unwrap()));
        assert_eq!(url_ip("tcp://[::ffff:1.2.3.4]:5"), Some("1.2.3.4".parse().unwrap()));
        assert_eq!(url_ip("udp://8.8.8.8:53"), Some("8.8.8.8".parse().unwrap()));
        assert!(is_lan(url_ip("udp://[fe80::1]:5").unwrap()));

        // 没有直接连接：经由其他节点转发
        let relayed = json!([{ "route": { "hostname": "host-pc", "path_latency": 40,
            "ipv4_addr": { "address": { "addr": 0x0a90_9001u32 } } }, "peer": null }]);
        assert_eq!(
            link_from_peers(&relayed, host),
            Some(Link { relayed: true, latency_ms: Some(40.0), ..Link::default() })
        );
        assert_eq!(link_from_peers(&json!([]), host), None);
    }

    #[test]
    fn unpack_keeps_only_top_level_files() {
        // 构造一个包含可执行文件、运行库和越界路径的发行包
        let mut tarball = Vec::new();
        {
            let gz = flate2::write::GzEncoder::new(&mut tarball, flate2::Compression::fast());
            let mut builder = tar::Builder::new(gz);
            let name = if cfg!(windows) { "terracotta-0.4.2-windows-x86_64.exe" } else { "terracotta-0.4.2-linux-x86_64" };
            for (path, data) in [(name, &b"exe"[..]), ("VCRUNTIME140.DLL", b"dll"), ("nested/evil", b"x")] {
                let mut header = tar::Header::new_gnu();
                header.set_size(data.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                builder.append_data(&mut header, path, data).unwrap();
            }
            builder.into_inner().unwrap().finish().unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("0.4.2");
        let exe = unpack(&target, &tarball).unwrap();
        assert!(exe.file_name().unwrap().to_string_lossy().starts_with("terracotta-"));
        assert!(target.join("VCRUNTIME140.DLL").is_file());
        assert!(!target.join("nested").exists() && !target.join("evil").exists());
        // 已解压过时直接复用
        assert_eq!(unpack(&target, b"not used").unwrap(), exe);
    }
}
