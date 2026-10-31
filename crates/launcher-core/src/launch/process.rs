//! 启动游戏进程、实时转发日志、退出后分析崩溃原因。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};

use super::args::LaunchCommand;
use crate::error::{CoreError, Result};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub text: String,
    pub stderr: bool,
}

pub type LogFn = Arc<dyn Fn(LogLine) + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameExit {
    pub code: Option<i32>,
    pub crashed: bool,
    pub crash_report: Option<PathBuf>,
    /// 根据日志推测的原因与解决办法
    pub causes: Vec<CrashCause>,
    pub tail: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CrashCause {
    pub reason: String,
    pub solution: String,
}

pub struct RunningGame {
    child: Child,
    game_dir: PathBuf,
    started_at: SystemTime,
    tail: Arc<Mutex<VecDeque<String>>>,
    readers: Vec<tokio::task::JoinHandle<()>>,
}

const TAIL_LINES: usize = 300;

/// 按 POSIX 优先级（LC_ALL > LC_CTYPE > LANG）判断字符编码是否为 UTF-8。
#[cfg_attr(not(unix), allow(dead_code))]
fn locale_is_utf8(get: impl Fn(&str) -> Option<String>) -> bool {
    ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .find_map(|k| get(k).filter(|v| !v.is_empty()))
        .is_some_and(|v| {
            let v = v.to_ascii_lowercase();
            v.contains("utf-8") || v.contains("utf8")
        })
}

pub fn spawn(cmd: &LaunchCommand, on_log: LogFn) -> Result<RunningGame> {
    if !cmd.java.exists() && cmd.java.components().count() > 1 {
        return Err(CoreError::Java(format!("找不到 Java：{}", cmd.java.display())));
    }
    std::fs::create_dir_all(&cmd.cwd).map_err(|e| CoreError::io(&cmd.cwd, e))?;

    let mut command = Command::new(&cmd.java);
    command
        .args(&cmd.args)
        .current_dir(&cmd.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false);
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW：不弹出控制台窗口
        command.creation_flags(0x0800_0000);
    }
    // 系统区域设置不是 UTF-8（例如 C / POSIX）时，Java 无法处理含中文等字符的路径，启动即崩溃
    #[cfg(unix)]
    if !locale_is_utf8(|k| std::env::var(k).ok()) {
        command.env("LC_ALL", "C.UTF-8");
    }
    let mut child = command
        .spawn()
        .map_err(|e| CoreError::Launch(format!("无法运行 Java（{}）：{e}", cmd.java.display())))?;

    let tail = Arc::new(Mutex::new(VecDeque::with_capacity(TAIL_LINES)));
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        readers.push(tokio::spawn(pump(out, false, on_log.clone(), tail.clone())));
    }
    if let Some(err) = child.stderr.take() {
        readers.push(tokio::spawn(pump(err, true, on_log, tail.clone())));
    }
    Ok(RunningGame {
        child,
        game_dir: cmd.cwd.clone(),
        started_at: SystemTime::now(),
        tail,
        readers,
    })
}

async fn pump<R: AsyncRead + Unpin>(r: R, stderr: bool, on_log: LogFn, tail: Arc<Mutex<VecDeque<String>>>) {
    let mut reader = BufReader::new(r);
    let mut buf = Vec::new();
    let mut xml = XmlLog::default();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let raw = String::from_utf8_lossy(&buf).trim_end_matches(['\r', '\n']).to_string();
                for text in xml.feed(&raw) {
                    if let Ok(mut t) = tail.lock() {
                        if t.len() == TAIL_LINES {
                            t.pop_front();
                        }
                        t.push_back(text.clone());
                    }
                    on_log(LogLine { text, stderr });
                }
            }
        }
    }
}

/// 原版的日志配置让游戏以 log4j XML 格式输出（官方启动器会解析它），这里把 XML 事件还原成普通文本行。
#[derive(Default)]
struct XmlLog {
    pending: Option<String>,
}

impl XmlLog {
    fn feed(&mut self, line: &str) -> Vec<String> {
        if let Some(buf) = self.pending.as_mut() {
            buf.push('\n');
            buf.push_str(line);
            if line.contains("</log4j:Event>") {
                let event = self.pending.take().unwrap_or_default();
                return Self::render(&event);
            }
            return Vec::new();
        }
        if line.trim_start().starts_with("<log4j:Event") {
            if line.contains("</log4j:Event>") {
                return Self::render(line);
            }
            self.pending = Some(line.to_string());
            return Vec::new();
        }
        vec![line.to_string()]
    }

    fn attr<'a>(event: &'a str, name: &str) -> &'a str {
        let key = format!("{name}=\"");
        event
            .find(&key)
            .and_then(|i| {
                let rest = &event[i + key.len()..];
                rest.find('"').map(|j| &rest[..j])
            })
            .unwrap_or("")
    }

    fn cdata<'a>(event: &'a str, tag: &str) -> Option<&'a str> {
        let start = event.find(&format!("<log4j:{tag}>"))?;
        let rest = &event[start..];
        let a = rest.find("<![CDATA[")? + 9;
        let b = rest[a..].find("]]>")?;
        Some(&rest[a..a + b])
    }

    fn render(event: &str) -> Vec<String> {
        let level = Self::attr(event, "level");
        let thread = Self::attr(event, "thread");
        let mut out = Vec::new();
        let message = Self::cdata(event, "Message").unwrap_or("");
        let mut lines = message.lines();
        let first = lines.next().unwrap_or("");
        out.push(format!("[{thread}/{level}]: {first}"));
        out.extend(lines.map(str::to_string));
        if let Some(t) = Self::cdata(event, "Throwable") {
            out.extend(t.lines().map(str::to_string));
        }
        out
    }
}

impl RunningGame {
    pub fn pid(&self) -> Option<u32> {
        self.child.id()
    }

    /// 等待游戏退出并分析结果。
    pub async fn wait(mut self) -> Result<GameExit> {
        let status = self
            .child
            .wait()
            .await
            .map_err(|e| CoreError::Launch(format!("等待游戏进程失败：{e}")))?;
        for r in self.readers.drain(..) {
            let _ = r.await;
        }
        let tail: Vec<String> = self.tail.lock().map(|t| t.iter().cloned().collect()).unwrap_or_default();
        let code = status.code();
        let crash_report = find_crash_report(&self.game_dir, self.started_at);
        let report_text = crash_report
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default();
        // 正常关闭游戏时退出码为 0；被信号结束（如内存不足被系统杀掉）时没有退出码，也按崩溃处理。
        // 用户在启动器里手动结束的情况由调用方区分。
        let crashed = code != Some(0) || crash_report.is_some();
        let causes = if crashed {
            analyze(&tail.join("\n"), &report_text)
        } else {
            Vec::new()
        };
        Ok(GameExit {
            code,
            crashed,
            crash_report,
            causes,
            tail,
        })
    }
}

/// 强制结束指定进程。
pub fn kill_pid(pid: u32) -> Result<()> {
    #[cfg(windows)]
    let status = std::process::Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .status();
    #[cfg(not(windows))]
    let status = std::process::Command::new("kill").args(["-9", &pid.to_string()]).status();
    match status {
        Ok(s) if s.success() => Ok(()),
        _ => Err(CoreError::Launch("无法结束游戏进程，它可能已经退出".into())),
    }
}

fn find_crash_report(game_dir: &Path, since: SystemTime) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(rd) = std::fs::read_dir(game_dir.join("crash-reports")) {
        candidates.extend(rd.flatten().map(|e| e.path()));
    }
    if let Ok(rd) = std::fs::read_dir(game_dir) {
        candidates.extend(
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("hs_err_pid"))),
        );
    }
    candidates
        .into_iter()
        .filter_map(|p| {
            let m = std::fs::metadata(&p).ok()?.modified().ok()?;
            (m >= since).then_some((m, p))
        })
        .max_by_key(|(m, _)| *m)
        .map(|(_, p)| p)
}

/// 根据日志和崩溃报告推测常见崩溃原因。
pub fn analyze(log: &str, report: &str) -> Vec<CrashCause> {
    let text = format!("{log}\n{report}");
    let rules: &[(&[&str], &str, &str)] = &[
        (
            &["java.lang.OutOfMemoryError", "Out of Memory Error"],
            "游戏内存不足",
            "在「设置 → 游戏」中调大最大内存；同时关闭不必要的程序。安装大量 Mod 时建议至少 4GB。",
        ),
        (
            &["Could not reserve enough space", "Invalid maximum heap size", "Invalid initial heap size"],
            "Java 无法分配设置的内存",
            "设置的内存超过了可用内存，或正在使用 32 位 Java。请降低内存设置或改用 64 位 Java。",
        ),
        (
            &["UnsupportedClassVersionError", "has been compiled by a more recent version of the Java Runtime"],
            "Java 版本过低",
            "这个版本需要更高版本的 Java。请在「设置 → Java」中选择「自动」，启动器会自动选择或下载合适的 Java。",
        ),
        (
            &["java.lang.ClassCastException: class jdk.internal.loader", "Unable to make protected final java.lang.Class java.lang.ClassLoader.defineClass"],
            "Java 版本过高",
            "旧版本游戏或 Forge 只支持 Java 8。请在「设置 → Java」中选择「自动」或手动指定 Java 8。",
        ),
        (
            &["Pixel format not accelerated", "GLFW error 65542", "WGL: The driver does not appear to support OpenGL", "OpenGL 3.2 core profile"],
            "显卡驱动不支持 OpenGL",
            "请到显卡厂商官网更新显卡驱动；笔记本请确认使用的是独立显卡。",
        ),
        (
            &["EXCEPTION_ACCESS_VIOLATION", "SIGSEGV"],
            "Java 虚拟机崩溃",
            "通常由显卡驱动或不兼容的 Mod 引起。请更新显卡驱动，或尝试移除最近添加的 Mod。",
        ),
        (
            &["java.lang.ClassNotFoundException", "NoClassDefFoundError", "Could not find or load main class"],
            "游戏文件缺失或损坏",
            "在「版本」页面点击「补全文件」后重试；如果安装了 Mod，检查是否缺少前置 Mod。",
        ),
        (
            &["Missing or unsupported mandatory dependencies", "requires any version of", "which is missing!"],
            "缺少前置 Mod",
            "根据日志中提示的 Mod 名称，在「Mod」页面搜索并安装缺少的前置。",
        ),
        (
            &["Found a duplicate mod", "DuplicateModsFoundException", "Duplicate mods found"],
            "存在重复的 Mod",
            "mods 文件夹里有同一个 Mod 的多个版本，请删除多余的。",
        ),
        (
            &["Mixin apply failed", "MixinApplyError", "Mixin transformation of"],
            "Mod 之间不兼容（Mixin 注入失败）",
            "某个 Mod 与当前游戏版本或其它 Mod 不兼容，请根据日志中的 Mod 名称移除或更新它。",
        ),
    ];
    rules
        .iter()
        .filter(|(keys, _, _)| keys.iter().any(|k| text.contains(k)))
        .map(|(_, r, s)| CrashCause {
            reason: r.to_string(),
            solution: s.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_locale() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
        };
        assert!(locale_is_utf8(env(&[("LANG", "zh_CN.UTF-8")])));
        assert!(locale_is_utf8(env(&[("LC_ALL", "C.utf8"), ("LANG", "C")])));
        assert!(!locale_is_utf8(env(&[("LC_ALL", "C"), ("LANG", "zh_CN.UTF-8")])));
        assert!(!locale_is_utf8(env(&[])));
        assert!(!locale_is_utf8(env(&[("LANG", "")])));
    }

    #[test]
    fn xml_log_events() {
        let mut x = XmlLog::default();
        assert_eq!(x.feed("plain line"), vec!["plain line"]);
        assert!(x
            .feed(r#"<log4j:Event logger="net.minecraft.client.Minecraft" timestamp="1" level="INFO" thread="Render thread">"#)
            .is_empty());
        assert!(x.feed("  <log4j:Message><![CDATA[Setting user: Steve]]></log4j:Message>").is_empty());
        assert_eq!(x.feed("</log4j:Event>"), vec!["[Render thread/INFO]: Setting user: Steve"]);
        let one = r#"<log4j:Event logger="a" level="ERROR" thread="main"><log4j:Message><![CDATA[boom
second]]></log4j:Message><log4j:Throwable><![CDATA[java.lang.RuntimeException: x
	at a.b]]></log4j:Throwable></log4j:Event>"#;
        let mut out = Vec::new();
        for l in one.lines() {
            out.extend(x.feed(l));
        }
        assert_eq!(out, vec!["[main/ERROR]: boom", "second", "java.lang.RuntimeException: x", "\tat a.b"]);
    }

    #[test]
    fn crash_analysis() {
        let causes = analyze("Exception in thread \"main\" java.lang.OutOfMemoryError: Java heap space", "");
        assert_eq!(causes.len(), 1);
        assert_eq!(causes[0].reason, "游戏内存不足");
        let causes = analyze("", "java.lang.UnsupportedClassVersionError: x has been compiled by a more recent version");
        assert_eq!(causes[0].reason, "Java 版本过低");
        assert!(analyze("all good", "").is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn spawn_and_capture_logs() {
        let dir = tempfile::tempdir().unwrap();
        let cmd = LaunchCommand {
            java: "/bin/sh".into(),
            args: vec!["-c".into(), "echo 你好; echo err 1>&2; exit 3".into()],
            cwd: dir.path().to_path_buf(),
            natives_dir: dir.path().to_path_buf(),
        };
        let lines = Arc::new(Mutex::new(Vec::new()));
        let l2 = lines.clone();
        let game = spawn(&cmd, Arc::new(move |l| l2.lock().unwrap().push(l))).unwrap();
        let exit = game.wait().await.unwrap();
        assert_eq!(exit.code, Some(3));
        assert!(exit.crashed);
        let lines = lines.lock().unwrap();
        assert!(lines.iter().any(|l| l.text == "你好" && !l.stderr));
        assert!(lines.iter().any(|l| l.text == "err" && l.stderr));
    }
}
