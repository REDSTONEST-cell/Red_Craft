//! 启动器日志：写入配置目录下的 logs/latest.log，调试构建同时输出到终端。

use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

struct Logger {
    file: Option<Mutex<std::fs::File>>,
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info || metadata.target().starts_with("launcher_core")
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = format!("[{secs}] [{}] {}: {}\n", record.level(), record.target(), record.args());
        if cfg!(debug_assertions) {
            eprint!("{line}");
        }
        if let Some(f) = &self.file {
            if let Ok(mut f) = f.lock() {
                let _ = f.write_all(line.as_bytes());
            }
        }
    }

    fn flush(&self) {}
}

pub fn init(config_dir: &Path) {
    let dir = config_dir.join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let latest = dir.join("latest.log");
    // 保留上一次的日志，方便排查问题
    let _ = std::fs::rename(&latest, dir.join("previous.log"));
    let file = std::fs::File::create(&latest).ok().map(Mutex::new);
    let logger = Box::leak(Box::new(Logger { file }));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
}
