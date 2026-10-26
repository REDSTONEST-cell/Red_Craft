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
    pub backgrounds: Mutex<crate::background::model::Library>,
    pub background_scan: tokio::sync::Mutex<()>,
    /// 上一次通知前端的背景窗口可见性，用于合并频繁的窗口事件
    pub background_visible: Mutex<Option<bool>>,
    /// 资源简介的中文翻译缓存：「平台:id」→ 译文
    pub translations: Mutex<HashMap<String, (String, u64)>>,
    pub paths: AppPaths,
    pub settings: RwLock<Settings>,
    pub accounts: RwLock<AccountStore>,
    pub tasks: Arc<Tasks>,
    http: Mutex<Option<Clients>>,
    pub manifest: tokio::sync::Mutex<Option<VersionManifest>>,
    pub java: Mutex<Option<Vec<JavaInstall>>>,
    /// 实例 id → 正在运行的游戏进程 pid
    pub running: Mutex<HashMap<String, u32>>,
    /// 被用户手动结束的实例，退出时不按崩溃处理
    pub killed: Mutex<HashSet<String>>,
    pub secrets: Secrets,
    /// 进行中的微软设备码登录
    pub ms_flows: Mutex<HashMap<u64, (launcher_core::download::CancelToken, launcher_core::auth::microsoft::DeviceCode)>>,
    /// 进行中的微软浏览器登录（等待浏览器跳转回来）
    pub ms_browser: Mutex<HashMap<u64, (launcher_core::download::CancelToken, Option<launcher_core::auth::microsoft::BrowserLogin>)>>,
    /// 外置登录：等待选择角色的登录结果
    pub ygg_flows: Mutex<HashMap<u64, (launcher_core::auth::yggdrasil::YggServer, launcher_core::auth::yggdrasil::YggLogin)>>,
}

/// 三类下载源各用一个客户端，共用连接池与限速。
#[derive(Clone)]
struct Clients {
    files: HttpClient,
    meta: HttpClient,
    community: HttpClient,
}

fn speed_limit(s: &Settings) -> u64 {
    u64::from(s.speed_limit_kb) * 1024
}

impl AppState {
    pub fn load() -> Self {
        let paths = AppPaths::detect();
        let settings: Settings = read_json(&paths.settings_file()).unwrap_or_default();
        let accounts: AccountStore = read_json(&paths.accounts_file()).unwrap_or_default();
        AppState {
            backgrounds: Mutex::new(crate::background::load(&paths.config_dir)),
            background_scan: tokio::sync::Mutex::new(()),
            background_visible: Mutex::new(None),
            translations: Mutex::new(HashMap::new()),
            secrets: Secrets::new(&paths.config_dir),
            ms_flows: Mutex::new(HashMap::new()),
            ms_browser: Mutex::new(HashMap::new()),
            ygg_flows: Mutex::new(HashMap::new()),
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
                || old.version_source != new.version_source
                || old.community_source != new.community_source
        };
        let rate = speed_limit(&new);
        write_json(&self.paths.settings_file(), &new)?;
        if let Ok(mut s) = self.settings.write() {
            *s = new;
        }
        if let Ok(mut h) = self.http.lock() {
            if changed_source {
                *h = None;
            } else if let Some(c) = h.as_ref() {
                // 限速立即对进行中的下载生效
                c.files.limiter().set_rate(rate);
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

    fn clients(&self) -> AppResult<Clients> {
        let mut guard = self
            .http
            .lock()
            .map_err(|_| AppError::msg("网络模块异常", "请重启启动器"))?;
        if let Some(c) = guard.as_ref() {
            return Ok(c.clone());
        }
        let s = self.settings();
        let files = HttpClient::new(s.download_source)?;
        files.limiter().set_rate(speed_limit(&s));
        let c = Clients {
            meta: files.with_source(s.version_source),
            community: files.with_source(s.community_source),
            files,
        };
        *guard = Some(c.clone());
        Ok(c)
    }

    /// 游戏文件、加载器、账号等使用的客户端（文件下载源）。
    pub fn http(&self) -> AppResult<HttpClient> {
        Ok(self.clients()?.files)
    }

    /// 版本列表、加载器列表使用的客户端（版本列表源）。
    pub fn http_meta(&self) -> AppResult<HttpClient> {
        Ok(self.clients()?.meta)
    }

    /// Modrinth / CurseForge 使用的客户端（社区资源来源）。
    pub fn http_community(&self) -> AppResult<HttpClient> {
        Ok(self.clients()?.community)
    }

    pub fn installer(&self) -> AppResult<Installer> {
        let downloader = Downloader::new(self.http()?, self.settings().concurrency);
        Ok(Installer::new(downloader, self.game_dir()))
    }

    /// 下载社区资源（Mod、资源包、整合包文件）使用的下载器。
    pub fn community_downloader(&self) -> AppResult<Downloader> {
        Ok(Downloader::new(self.http_community()?, self.settings().concurrency))
    }
}
