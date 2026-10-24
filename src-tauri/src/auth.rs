//! 微软正版登录相关的 commands，以及启动前获取有效令牌。

use std::sync::atomic::{AtomicU64, Ordering};

use launcher_core::auth::microsoft::{self, DeviceCode, MsSession};
use launcher_core::auth::{Account, LaunchAuth};
use launcher_core::download::CancelToken;
use serde::Serialize;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

static NEXT_FLOW: AtomicU64 = AtomicU64::new(1);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MsLoginStart {
    flow_id: u64,
    #[serde(flatten)]
    code: DeviceCode,
}

/// 第一步：获取设备码，前端展示验证码并引导用户打开网页。
#[tauri::command]
pub async fn ms_login_start(state: State<'_, AppState>) -> AppResult<MsLoginStart> {
    let client_id = state.settings().effective_client_id();
    let http = state.http()?;
    let code = microsoft::start_device_flow(http.raw(), &client_id).await?;
    let flow_id = NEXT_FLOW.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut f) = state.ms_flows.lock() {
        f.insert(flow_id, (CancelToken::new(), code.clone()));
    }
    Ok(MsLoginStart { flow_id, code })
}

/// 第二步：等待用户在网页上完成登录，然后完成 Xbox / Minecraft 登录并保存账号。
#[tauri::command]
pub async fn ms_login_finish(state: State<'_, AppState>, flow_id: u64) -> AppResult<Account> {
    let (cancel, code) = state
        .ms_flows
        .lock()
        .ok()
        .and_then(|f| f.get(&flow_id).cloned())
        .ok_or_else(|| AppError::msg("登录流程已失效", "请重新点击「微软登录」。"))?;
    let client_id = state.settings().effective_client_id();
    let http = state.http()?;
    let result = microsoft::finish_device_flow(http.raw(), &client_id, &code, &cancel).await;
    if let Ok(mut f) = state.ms_flows.lock() {
        f.remove(&flow_id);
    }
    let session = result?;
    let account = save_session(&state, &session)?;
    state.update_settings(|s| s.selected_account = Some(account.id.clone()))?;
    log::info!("微软账号 {} 登录成功", account.name);
    Ok(account)
}

#[tauri::command]
pub fn ms_login_cancel(state: State<'_, AppState>, flow_id: u64) {
    if let Some((t, _)) = state.ms_flows.lock().ok().and_then(|mut f| f.remove(&flow_id)) {
        t.cancel();
    }
    if let Some((t, _)) = state.ms_browser.lock().ok().and_then(|mut f| f.remove(&flow_id)) {
        t.cancel();
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserStart {
    flow_id: u64,
    url: String,
}

/// 浏览器登录第一步：在本机打开回调端口，并用系统浏览器打开微软登录页。
#[tauri::command]
pub async fn ms_browser_start(app: tauri::AppHandle, state: State<'_, AppState>) -> AppResult<BrowserStart> {
    use tauri_plugin_opener::OpenerExt;
    let client_id = state.settings().effective_client_id();
    let login = microsoft::start_browser_login(&client_id).await?;
    let url = login.url.clone();
    let flow_id = NEXT_FLOW.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut f) = state.ms_browser.lock() {
        f.insert(flow_id, (CancelToken::new(), Some(login)));
    }
    if let Err(e) = app.opener().open_url(&url, None::<&str>) {
        log::warn!("无法打开浏览器：{e}");
    }
    Ok(BrowserStart { flow_id, url })
}

/// 浏览器登录第二步：等待浏览器跳转回来并完成登录。
#[tauri::command]
pub async fn ms_browser_finish(state: State<'_, AppState>, flow_id: u64) -> AppResult<Account> {
    let (cancel, login) = state
        .ms_browser
        .lock()
        .ok()
        .and_then(|mut f| f.get_mut(&flow_id).map(|(c, l)| (c.clone(), l.take())))
        .ok_or_else(|| AppError::msg("登录流程已失效", "请重新点击「微软登录」。"))?;
    let login = login.ok_or_else(|| AppError::msg("登录流程已在进行中", ""))?;
    let client_id = state.settings().effective_client_id();
    let http = state.http()?;
    let result = microsoft::finish_browser_login(http.raw(), &client_id, login, &cancel).await;
    if let Ok(mut f) = state.ms_browser.lock() {
        f.remove(&flow_id);
    }
    let session = result?;
    let account = save_session(&state, &session)?;
    state.update_settings(|s| s.selected_account = Some(account.id.clone()))?;
    log::info!("微软账号 {} 登录成功（浏览器）", account.name);
    Ok(account)
}

/// 保存令牌到密钥库，并新增或更新账号列表中的条目（角色名、皮肤可能变化）。
fn save_session(state: &AppState, session: &MsSession) -> AppResult<Account> {
    let account = Account::microsoft(&session.profile);
    let json = serde_json::to_string(session).map_err(|e| launcher_core::CoreError::json("登录信息", e))?;
    state.secrets.set(&account.id, &json)?;
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
    state.save_accounts()?;
    Ok(account)
}

/// 启动前获取可用的令牌：未过期直接使用，否则用 refresh token 重新登录。
pub async fn microsoft_launch_auth(state: &AppState, account: &Account) -> AppResult<LaunchAuth> {
    let stored = state
        .secrets
        .get(&account.id)
        .and_then(|s| serde_json::from_str::<MsSession>(&s).ok())
        .ok_or_else(|| {
            AppError::msg(
                format!("找不到微软账号「{}」的登录信息", account.name),
                "请在「账号」页面删除后重新登录这个账号。",
            )
        })?;
    if stored.is_valid() {
        return Ok(LaunchAuth::microsoft(&stored));
    }
    log::info!("微软账号 {} 的令牌已过期，正在刷新", account.name);
    let client_id = state.settings().effective_client_id();
    let http = state.http()?;
    let session = microsoft::refresh(http.raw(), &client_id, &stored.ms_refresh_token).await?;
    save_session(state, &session)?;
    Ok(LaunchAuth::microsoft(&session))
}

/// 前端「刷新」按钮：检查登录是否仍然有效，并更新角色名与皮肤。
#[tauri::command]
pub async fn ms_refresh_account(state: State<'_, AppState>, id: String) -> AppResult<Account> {
    let account = state
        .accounts
        .read()
        .ok()
        .and_then(|a| a.accounts.iter().find(|x| x.id == id).cloned())
        .ok_or_else(|| AppError::msg("账号不存在", "请刷新账号列表。"))?;
    let stored = state
        .secrets
        .get(&account.id)
        .and_then(|s| serde_json::from_str::<MsSession>(&s).ok())
        .ok_or_else(|| AppError::msg("找不到这个账号的登录信息", "请删除后重新登录。"))?;
    let client_id = state.settings().effective_client_id();
    let http = state.http()?;
    let session = microsoft::refresh(http.raw(), &client_id, &stored.ms_refresh_token).await?;
    save_session(&state, &session)
}
