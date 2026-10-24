//! 启动器设置、账号列表与实例配置的持久化。令牌等敏感信息不写入这些文件。

use std::path::{Path, PathBuf};

use launcher_core::auth::Account;
use launcher_core::download::DownloadSource;
use launcher_core::error::{CoreError, IoContext, Result};
use launcher_core::paths::GameDir;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// 版本隔离策略（与 PCL 的选项一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Isolation {
    /// 关闭
    Off,
    /// 隔离可安装 Mod 的版本
    Modded,
    /// 隔离非正式版
    NonRelease,
    /// 隔离可安装 Mod 的版本与非正式版
    ModdedAndNonRelease,
    /// 隔离所有版本
    All,
}

impl Isolation {
    pub fn applies(self, modded: bool, release: bool) -> bool {
        match self {
            Isolation::Off => false,
            Isolation::Modded => modded,
            Isolation::NonRelease => !release,
            Isolation::ModdedAndNonRelease => modded || !release,
            Isolation::All => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AfterLaunch {
    Keep,
    Hide,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// 自定义游戏目录；为空时使用便携目录或标准 `.minecraft`
    pub game_dir: Option<PathBuf>,
    pub download_source: DownloadSource,
    pub concurrency: usize,
    pub isolation: Isolation,
    /// 为空表示自动选择
    pub java_path: Option<PathBuf>,
    pub auto_download_java: bool,
    pub memory_auto: bool,
    pub max_memory_mb: u32,
    pub min_memory_mb: u32,
    pub jvm_args: String,
    pub game_args: String,
    pub custom_resolution: bool,
    pub window_width: u32,
    pub window_height: u32,
    pub fullscreen: bool,
    /// 启动后自动进入的服务器
    pub server: String,
    pub after_launch: AfterLaunch,
    pub theme: Theme,
    pub accent: String,
    pub selected_instance: Option<String>,
    pub selected_account: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            game_dir: None,
            download_source: DownloadSource::Auto,
            concurrency: 64,
            isolation: Isolation::ModdedAndNonRelease,
            java_path: None,
            auto_download_java: true,
            memory_auto: true,
            max_memory_mb: 4096,
            min_memory_mb: 512,
            jvm_args: String::new(),
            game_args: String::new(),
            custom_resolution: false,
            window_width: 1280,
            window_height: 720,
            fullscreen: false,
            server: String::new(),
            after_launch: AfterLaunch::Hide,
            theme: Theme::System,
            accent: "#E8590C".into(),
            selected_instance: None,
            selected_account: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AccountStore {
    pub accounts: Vec<Account>,
}

/// 实例自身的配置，保存在 `versions/<id>/redcraft.json`。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceConfig {
    /// 对应的 Minecraft 版本号
    pub mc_version: Option<String>,
    /// 覆盖全局的版本隔离设置
    pub isolation: Option<bool>,
    pub java_path: Option<PathBuf>,
    pub max_memory_mb: Option<u32>,
    pub jvm_args: Option<String>,
    pub server: Option<String>,
    pub last_played: Option<u64>,
}

pub const INSTANCE_CONFIG: &str = "redcraft.json";

impl InstanceConfig {
    pub fn load(game: &GameDir, id: &str) -> Self {
        read_json(&game.version_dir(id).join(INSTANCE_CONFIG)).unwrap_or_default()
    }
    pub fn save(&self, game: &GameDir, id: &str) -> Result<()> {
        write_json(&game.version_dir(id).join(INSTANCE_CONFIG), self)
    }
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let data = std::fs::read(path).ok()?;
    match serde_json::from_slice(&data) {
        Ok(v) => Some(v),
        Err(e) => {
            log::warn!("配置文件 {} 已损坏：{e}", path.display());
            None
        }
    }
}

/// 先写临时文件再改名，避免写到一半崩溃导致配置损坏。
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).at(parent)?;
    }
    let data = serde_json::to_vec_pretty(value).map_err(|e| CoreError::json("配置", e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, data).at(&tmp)?;
    std::fs::rename(&tmp, path).at(path)
}

/// 启动器使用的目录。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPaths {
    /// 设置与账号等文件所在目录
    pub config_dir: PathBuf,
    /// 启动器所在目录有 `.minecraft` 时为便携模式
    pub portable: bool,
    /// 未自定义时使用的游戏目录
    pub default_game_dir: PathBuf,
}

impl AppPaths {
    pub fn detect() -> Self {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf));
        if let Some(dir) = exe_dir {
            let local = dir.join(".minecraft");
            if local.is_dir() {
                return AppPaths {
                    config_dir: local.join("redcraft"),
                    portable: true,
                    default_game_dir: local,
                };
            }
        }
        let config_dir = dirs::config_dir()
            .map(|d| d.join("RedCraft"))
            .unwrap_or_else(|| PathBuf::from(".redcraft"));
        AppPaths {
            config_dir,
            portable: false,
            default_game_dir: GameDir::default_location(),
        }
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }

    pub fn accounts_file(&self) -> PathBuf {
        self.config_dir.join("accounts.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolation_rules() {
        use Isolation::*;
        assert!(!Off.applies(true, false));
        assert!(Modded.applies(true, true) && !Modded.applies(false, false));
        assert!(NonRelease.applies(false, false) && !NonRelease.applies(true, true));
        assert!(ModdedAndNonRelease.applies(true, true) && ModdedAndNonRelease.applies(false, false));
        assert!(!ModdedAndNonRelease.applies(false, true));
        assert!(All.applies(false, true));
    }

    #[test]
    fn settings_tolerate_missing_fields() {
        let s: Settings = serde_json::from_str(r#"{"concurrency": 8}"#).unwrap();
        assert_eq!(s.concurrency, 8);
        assert_eq!(s.accent, "#E8590C");
        assert_eq!(s.download_source, DownloadSource::Auto);
    }

    #[test]
    fn atomic_write_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a/settings.json");
        write_json(&p, &Settings::default()).unwrap();
        let s: Settings = read_json(&p).unwrap();
        assert_eq!(s.concurrency, 64);
    }
}
