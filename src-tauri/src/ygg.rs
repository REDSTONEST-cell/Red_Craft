//! 外置登录（authlib-injector / Yggdrasil）相关的 commands，以及启动前获取有效令牌。

use std::sync::atomic::{AtomicU64, Ordering};

use launcher_core::auth::yggdrasil::{self, YggLogin, YggProfile, YggServer, YggSession};
use launcher_core::auth::{Account, AccountKind, AuthlibArgs, LaunchAuth};
use launcher_core::download::{CancelToken, Downloader};
use launcher_core::install::Reporter;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

static NEXT_FLOW: AtomicU64 = AtomicU64::new(1);

/// 保存在密钥库中的内容：服务器信息与会话。
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    server: YggServer,
    session: YggSession,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    api_root: String,
    name: String,
    register_url: Option<String>,
}

/// 检查认证服务器地址，返回服务器名称（用于输入框下方的提示）。
#[tauri::command]
pub async fn ygg_resolve(state: State<'_, AppState>, server: String) -> AppResult<ServerInfo> {
    let http = state.http()?;
    let s = yggdrasil::resolve_server(http.raw(), &server).await?;
    Ok(ServerInfo {
        api_root: s.api_root,
        name: s.name,
        register_url: s.register_url,
    })
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LoginResult {
    /// 登录完成（只有一个角色，或服务器已绑定角色）
    #[serde(rename_all = "camelCase")]
    Done { account: Account },
    /// 账号下有多个角色，需要用户选择
    #[serde(rename_all = "camelCase")]
    Choose {
        flow_id: u64,
        server_name: String,
        profiles: Vec<YggProfile>,
    },
}

/// 用账号密码登录。密码只用于这一次请求，不会保存。
#[tauri::command]
pub async fn ygg_login(
    state: State<'_, AppState>,
    server: String,
    username: String,
    password: String,
) -> AppResult<LoginResult> {
    let http = state.http()?;
    let server = yggdrasil::resolve_server(http.raw(), &server).await?;
    let login = yggdrasil::authenticate(http.raw(), &server, username.trim(), &password).await?;
    if login.profiles.is_empty() {
        return Err(AppError::msg(
            "这个账号还没有角色",
            format!("请先在「{}」的网站上创建角色，再回来登录。", server.name),
        ));
    }
    if let Some(profile) = login.session.profile.clone() {
        let account = save(&state, &server, &login.session, &profile).await?;
        return Ok(LoginResult::Done { account });
    }
    let flow_id = NEXT_FLOW.fetch_add(1, Ordering::Relaxed);
    let result = LoginResult::Choose {
        flow_id,
        server_name: server.name.clone(),
        profiles: login.profiles.clone(),
    };
    if let Ok(mut f) = state.ygg_flows.lock() {
        f.insert(flow_id, (server, login));
    }
    Ok(result)
}

/// 多角色时选择其中一个，用 refresh 把令牌绑定到该角色。
#[tauri::command]
pub async fn ygg_select(state: State<'_, AppState>, flow_id: u64, profile_id: String) -> AppResult<Account> {
    let (server, login): (YggServer, YggLogin) = state
        .ygg_flows
        .lock()
        .ok()
        .and_then(|mut f| f.remove(&flow_id))
        .ok_or_else(|| AppError::msg("登录流程已失效", "请重新输入账号密码登录。"))?;
    let profile = login
        .profiles
        .iter()
        .find(|p| p.id == profile_id)
        .cloned()
        .ok_or_else(|| AppError::msg("找不到这个角色", "请重新登录。"))?;
    let http = state.http()?;
    let session = yggdrasil::refresh(http.raw(), &server, &login.session, Some(&profile)).await?;
    save(&state, &server, &session, &profile).await
}

#[tauri::command]
pub fn ygg_cancel(state: State<'_, AppState>, flow_id: u64) {
    if let Ok(mut f) = state.ygg_flows.lock() {
        f.remove(&flow_id);
    }
}

/// 保存令牌，新增或更新账号条目，并选中它。
async fn save(state: &AppState, server: &YggServer, session: &YggSession, profile: &YggProfile) -> AppResult<Account> {
    let mut account = Account::yggdrasil(server, profile);
    if let Ok(http) = state.http() {
        account.skin = yggdrasil::skin_data_url(http.raw(), server, &profile.id).await;
    }
    store(state, server, session)?;
    upsert(state, &account)?;
    state.update_settings(|s| s.selected_account = Some(account.id.clone()))?;
    log::info!("外置账号 {}（{}）登录成功", account.name, server.name);
    Ok(account)
}

fn store(state: &AppState, server: &YggServer, session: &YggSession) -> AppResult<()> {
    let profile = session.profile.as_ref().ok_or_else(|| AppError::msg("登录信息缺少角色", "请重新登录。"))?;
    let id = Account::yggdrasil(server, profile).id;
    let json = serde_json::to_string(&Stored {
        server: server.clone(),
        session: session.clone(),
    })
    .map_err(|e| launcher_core::CoreError::json("登录信息", e))?;
    state.secrets.set(&id, &json)?;
    Ok(())
}

fn upsert(state: &AppState, account: &Account) -> AppResult<()> {
    {
        let mut store = state
            .accounts
            .write()
            .map_err(|_| AppError::msg("账号数据被锁定", "请重启启动器"))?;
        match store.accounts.iter_mut().find(|a| a.id == account.id) {
            Some(a) => *a = account.clone(),
            None => store.accounts.push(account.clone()),
        }
    }
    state.save_accounts()
}

fn load(state: &AppState, account: &Account) -> AppResult<Stored> {
    state
        .secrets
        .get(&account.id)
        .and_then(|s| serde_json::from_str::<Stored>(&s).ok())
        .ok_or_else(|| {
            AppError::msg(
                format!("找不到外置账号「{}」的登录信息", account.name),
                "请在「账号」页面删除后重新登录这个账号。",
            )
        })
}

/// 刷新服务器元数据（服务器可能更换了签名公钥），失败时沿用旧的。
async fn refresh_server(http: &reqwest::Client, old: &YggServer) -> YggServer {
    match yggdrasil::resolve_server(http, &old.api_root).await {
        // 保持 API 地址不变，账号 id 依赖它
        Ok(s) => YggServer {
            api_root: old.api_root.clone(),
            ..s
        },
        Err(e) => {
            log::warn!("无法刷新认证服务器信息，使用缓存：{e}");
            old.clone()
        }
    }
}

/// 启动前：校验令牌（必要时刷新）、下载 authlib-injector，返回带 javaagent 参数的凭据。
pub async fn yggdrasil_launch_auth(
    state: &AppState,
    account: &Account,
    reporter: &Reporter,
    cancel: &CancelToken,
) -> AppResult<LaunchAuth> {
    let stored = load(state, account)?;
    let http = state.http()?;
    let server = refresh_server(http.raw(), &stored.server).await;
    let session = yggdrasil::ensure_valid(http.raw(), &server, &stored.session).await?;
    store(state, &server, &session)?;
    let profile = session
        .profile
        .clone()
        .ok_or_else(|| AppError::msg("登录信息缺少角色", "请删除这个账号后重新登录。"))?;

    let downloader = Downloader::new(http, state.settings().concurrency);
    let dir = state.paths.config_dir.join("authlib-injector");
    let jar = yggdrasil::ensure_authlib_injector(&downloader, &dir, reporter, cancel).await?;
    Ok(LaunchAuth::yggdrasil(
        &session,
        &profile,
        AuthlibArgs {
            jar,
            server: server.api_root.clone(),
            prefetched: server.prefetched.clone(),
        },
    ))
}

/// 前端「刷新」按钮：检查登录是否仍然有效，并更新角色名与皮肤。
#[tauri::command]
pub async fn ygg_refresh_account(state: State<'_, AppState>, id: String) -> AppResult<Account> {
    let account = state
        .accounts
        .read()
        .ok()
        .and_then(|a| a.accounts.iter().find(|x| x.id == id).cloned())
        .ok_or_else(|| AppError::msg("账号不存在", "请刷新账号列表。"))?;
    if !matches!(account.kind, AccountKind::Yggdrasil { .. }) {
        return Err(AppError::msg("不是外置登录账号", ""));
    }
    let stored = load(&state, &account)?;
    let http = state.http()?;
    let server = refresh_server(http.raw(), &stored.server).await;
    let session = yggdrasil::refresh(http.raw(), &server, &stored.session, None).await?;
    store(&state, &server, &session)?;
    let profile = session.profile.clone().unwrap_or(YggProfile {
        id: account.uuid.clone(),
        name: account.name.clone(),
    });
    let mut updated = Account::yggdrasil(&server, &profile);
    updated.skin = yggdrasil::skin_data_url(http.raw(), &server, &profile.id).await.or(account.skin);
    upsert(&state, &updated)?;
    Ok(updated)
}
