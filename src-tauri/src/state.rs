//! 全局状态。

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};

use launcher_core::download::{Downloader, HttpClient};
use launcher_core::install::Installer;
use launcher_core::java::JavaInstall;
use launcher_core::paths::GameDir;
use launcher_core::version::VersionManifest;

use crate::error::{AppError, AppResult};
use crate::secrets::Secrets;
use crate::settings::{read_json, write_json, AccountStore, AppPaths, Settings};
use crate::tasks::Tasks;

pub struct AppState {
    pub paths: AppPaths,
    pub settings: RwLock<Settings>,
    pub accounts: RwLock<AccountStore>,
    pub tasks: Arc<Tasks>,
    http: Mutex<Option<HttpClient>>,
    pub manifest: tokio::sync::Mutex<Option<VersionManifest>>,
    pub java: Mutex<Option<Vec<JavaInstall>>>,
    /// 实例 id → 正在运行的游戏进程 pid
    pub running: Mutex<HashMap<String, u32>>,
    /// 被用户手动结束的实例，退出时不按崩溃处理
    pub killed: Mutex<HashSet<String>>,
    pub secrets: Secrets,
    /// 进行中的微软设备码登录
    pub ms_flows: Mutex<HashMap<u64, (launcher_core::download::CancelToken, launcher_core::auth::microsoft::DeviceCode)>>,
}

impl AppState {
    pub fn load() -> Self {
        let paths = AppPaths::detect();
        let settings: Settings = read_json(&paths.settings_file()).unwrap_or_default();
        let accounts: AccountStore = read_json(&paths.accounts_file()).unwrap_or_default();
        AppState {
            secrets: Secrets::new(&paths.config_dir),
            ms_flows: Mutex::new(HashMap::new()),
            paths,
            settings: RwLock::new(settings),
            accounts: RwLock::new(accounts),
            tasks: Arc::new(Tasks::default()),
            http: Mutex::new(None),
            manifest: tokio::sync::Mutex::new(None),
            java: Mutex::new(None),
            running: Mutex::new(HashMap::new()),
            killed: Mutex::new(HashSet::new()),
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn save_settings(&self, new: Settings) -> AppResult<()> {
        let changed_source = {
            let old = self.settings();
            old.download_source != new.download_source
        };
        write_json(&self.paths.settings_file(), &new)?;
        if let Ok(mut s) = self.settings.write() {
            *s = new;
        }
        if changed_source {
            if let Ok(mut h) = self.http.lock() {
                *h = None;
            }
        }
        Ok(())
    }

    pub fn update_settings(&self, f: impl FnOnce(&mut Settings)) -> AppResult<()> {
        let mut s = self.settings();
        f(&mut s);
        self.save_settings(s)
    }

    pub fn save_accounts(&self) -> AppResult<()> {
        let store = self
            .accounts
            .read()
            .map(|a| a.clone())
            .map_err(|_| AppError::msg("账号数据被锁定", "请重启启动器"))?;
        write_json(&self.paths.accounts_file(), &store)?;
        Ok(())
    }

    pub fn game_dir(&self) -> GameDir {
        let s = self.settings();
        GameDir::new(s.game_dir.unwrap_or_else(|| self.paths.default_game_dir.clone()))
    }

    pub fn http(&self) -> AppResult<HttpClient> {
        let mut guard = self
            .http
            .lock()
            .map_err(|_| AppError::msg("网络模块异常", "请重启启动器"))?;
        if let Some(h) = guard.as_ref() {
            return Ok(h.clone());
        }
        let h = HttpClient::new(self.settings().download_source)?;
        *guard = Some(h.clone());
        Ok(h)
    }

    pub fn installer(&self) -> AppResult<Installer> {
        let downloader = Downloader::new(self.http()?, self.settings().concurrency);
        Ok(Installer::new(downloader, self.game_dir()))
    }
}
