//! Red Craft 启动器核心库。不依赖 Tauri，可独立测试。

pub mod auth;
pub mod content;
pub mod download;
pub mod error;
pub mod install;
pub mod instance;
pub mod java;
pub mod launch;
pub mod loader;
pub mod memopt;
pub mod migrate;
pub mod paths;
pub mod system;
pub mod update;
pub mod version;

pub use error::{CoreError, ErrorInfo, Result};

pub const LAUNCHER_NAME: &str = "RCL";
/// 用于 User-Agent 与 `${launcher_name}` 的标识（不含空格）
pub const LAUNCHER_ID: &str = "RedCraft";
pub const LAUNCHER_VERSION: &str = env!("CARGO_PKG_VERSION");
