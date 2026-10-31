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

/// 界面形状：利落（直角、硬阴影，报刊风）、圆润（圆角、柔和阴影，类似 PCL2）或毛玻璃（半透明虚化面板，Apple 风格）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Corner {
    Sharp,
    #[default]
    Rounded,
    Glass,
}

/// 界面布局：标准，或只突出主要内容、字号更大的极简。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Layout {
    Standard,
    #[default]
    Minimal,
}

/// 下载 Mod 时的文件名格式（与 PCL 类似，可以在文件名里加上中文名）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ModFilename {
    /// 保持原文件名：create-1.21.1-6.0.4.jar
    #[default]
    Original,
    /// [机械动力] create-1.21.1-6.0.4.jar
    ZhPrefix,
    /// create-1.21.1-6.0.4-机械动力.jar
    ZhSuffix,
}

/// 已安装 Mod 列表的显示方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ModTitle {
    /// 标题显示译名，详情显示文件名
    #[default]
    Translated,
    /// 标题显示文件名，详情显示译名
    Filename,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// 自定义游戏目录；为空时使用便携目录或标准 `.minecraft`
    pub game_dir: Option<PathBuf>,
    /// 文件下载源（游戏文件、加载器）
    pub download_source: DownloadSource,
    /// 版本列表源（版本、加载器、OptiFine 列表）
    pub version_source: DownloadSource,
    /// 社区资源来源（Modrinth / CurseForge 的搜索与文件下载）
    pub community_source: DownloadSource,
    pub concurrency: usize,
    /// 下载速度限制（KB/s），0 表示不限速
    pub speed_limit_kb: u32,
    pub mod_filename: ModFilename,
    pub mod_title: ModTitle,
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
    pub corner: Corner,
    pub layout: Layout,
    /// 毛玻璃面板的不透明度（0.2–0.95）
    pub glass_opacity: f64,
    /// 毛玻璃的模糊度（像素，0–40）
    pub glass_blur: f64,
    pub selected_instance: Option<String>,
    pub selected_account: Option<String>,
    /// 微软登录使用的 Azure 应用 Client ID；为空时使用编译时内置的值
    pub ms_client_id: String,
    /// 第一次启动实例时把游戏语言设为简体中文
    pub default_chinese: bool,
    /// CurseForge 官方 API Key；为空时通过 MCIM 镜像访问
    pub curseforge_api_key: String,
    /// 内存回收器
    pub gc: launcher_core::launch::GcMode,
    /// 启动前执行的命令（可使用 {instance} {mc} {game_dir} {java} 占位符）
    pub pre_launch_command: String,
    /// 启动前等待命令结束
    pub pre_launch_wait: bool,
    /// Windows：让 Java 使用高性能显卡
    pub high_performance_gpu: bool,
    /// 启动时检查启动器更新（发现新版本时弹窗确认）
    pub auto_check_update: bool,
    /// 启动游戏前进行内存优化（Windows）
    pub optimize_memory: bool,
    /// 用户选择跳过的版本，不再提示
    pub skipped_update: String,
}

impl Settings {
    pub fn effective_client_id(&self) -> String {
        let id = self.ms_client_id.trim();
        if id.is_empty() {
            launcher_core::auth::microsoft::DEFAULT_CLIENT_ID.to_string()
        } else {
            id.to_string()
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            game_dir: None,
            download_source: DownloadSource::Auto,
            version_source: DownloadSource::Auto,
            community_source: DownloadSource::Auto,
            concurrency: 64,
            speed_limit_kb: 0,
            mod_filename: ModFilename::default(),
            mod_title: ModTitle::default(),
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
            // 手机版默认浅色
            theme: if cfg!(mobile) { Theme::Light } else { Theme::System },
            accent: "#E8590C".into(),
            corner: Corner::default(),
            layout: Layout::default(),
            glass_opacity: 0.58,
            glass_blur: 24.0,
            selected_instance: None,
            selected_account: None,
            ms_client_id: String::new(),
            default_chinese: true,
            curseforge_api_key: String::new(),
            gc: launcher_core::launch::GcMode::Auto,
            pre_launch_command: String::new(),
            pre_launch_wait: true,
            high_performance_gpu: true,
            auto_check_update: true,
            optimize_memory: false,
            skipped_update: String::new(),
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
    /// 便携模式：启动器所在目录有 `.minecraft`，或使用便携版单文件 exe
    pub portable: bool,
    /// 未自定义时使用的游戏目录
    pub default_game_dir: PathBuf,
}

impl AppPaths {
    pub fn detect() -> Self {
        // 手机版：数据放在应用私有目录（其他应用无法访问，卸载时一并删除）
        #[cfg(target_os = "android")]
        if let Some(files) = android_files_dir() {
            // 系统默认的临时目录 /data/local/tmp 应用无权写入
            let tmp = files.with_file_name("cache");
            let _ = std::fs::create_dir_all(&tmp);
            std::env::set_var("TMPDIR", &tmp);
            return AppPaths {
                config_dir: files.join("config"),
                portable: false,
                default_game_dir: files.join(".minecraft"),
            };
        }
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf));
        if let Some(dir) = exe_dir {
            let local = dir.join(".minecraft");
            // 便携版：第一次运行时在 exe 旁边创建 .minecraft；目录不可写时退回标准位置
            if cfg!(feature = "portable") && !local.exists() {
                let _ = std::fs::create_dir_all(&local);
            }
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

/// Android 应用的私有文件目录 `/data/user/<用户>/<包名>/files`。
/// 包名取自进程名（子进程形如「包名:进程名」），用户编号由 uid 计算（每个用户 100000 个 uid）。
#[cfg(target_os = "android")]
fn android_files_dir() -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let cmdline = std::fs::read("/proc/self/cmdline").ok()?;
    let name = String::from_utf8_lossy(cmdline.split(|b| *b == 0).next()?).into_owned();
    let package = name.split(':').next().filter(|p| !p.is_empty())?.to_string();
    let user = std::fs::metadata("/proc/self").ok()?.uid() / 100_000;
    let dir = PathBuf::from(format!("/data/user/{user}/{package}/files"));
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
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
        assert_eq!(s.corner, Corner::Rounded);
        assert_eq!(s.layout, Layout::Minimal);
        assert_eq!(s.speed_limit_kb, 0);
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
