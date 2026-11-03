//! 外部 Java 执行器：在不能直接启动 Java 进程的平台（手机）上，由宿主提供运行方式。
//!
//! 电脑版不设置执行器，安装器等仍以子进程运行；手机版由应用在独立进程的内置 JVM 中运行。

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use crate::download::CancelToken;
use crate::error::Result;

/// 一次 Java 运行：`java <args>`，工作目录为 `cwd`。
pub struct JavaJob {
    /// 调用方选择的 Java（手机版为运行时名称，如 `java21`）
    pub java: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// 显示用的名称，如「Forge 安装器」
    pub label: String,
}

/// 运行结果。`code` 为 `None` 表示进程已结束但没有得到退出码，调用方应检查输出文件判断是否成功。
pub struct JavaOutcome {
    pub code: Option<i32>,
    /// 最后若干行输出
    pub output: Vec<String>,
}

pub type JavaRunner =
    Arc<dyn Fn(JavaJob, CancelToken) -> Pin<Box<dyn Future<Output = Result<JavaOutcome>> + Send>> + Send + Sync>;

static RUNNER: OnceLock<JavaRunner> = OnceLock::new();

/// 设置外部执行器（启动时调用一次）。
pub fn set_runner(runner: JavaRunner) {
    let _ = RUNNER.set(runner);
}

/// 当前的外部执行器；未设置时调用方直接启动 Java 子进程。
pub fn runner() -> Option<JavaRunner> {
    RUNNER.get().cloned()
}
