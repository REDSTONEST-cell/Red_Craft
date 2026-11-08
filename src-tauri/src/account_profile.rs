use launcher_core::auth::{AccountKind, microsoft::MsSession};
use serde::{Deserialize, Serialize};
use tauri::State;
use crate::{auth::microsoft_launch_auth, error::{AppError, AppResult}, state::AppState};

const PROFILE: &str = "https://api.minecraftservices.com/minecraft/profile";
#[derive(Clone, Serialize, Deserialize)]
pub struct Texture { pub id: String, pub state: String, pub url: String, #[serde(default)] pub alias: String }
#[derive(Serialize, Deserialize)]
pub struct Profile { pub id: String, pub name: String, #[serde(default)] pub skins: Vec<Texture>, #[serde(default)] pub capes: Vec<Texture> }
fn failure() -> AppError { AppError::msg("无法连接 Minecraft 账号服务", "请检查网络后重试。") }
async fn request(state: &AppState, id: &str, method: reqwest::Method, suffix: &str, body: Option<serde_json::Value>, skin: Option<(Vec<u8>, String)>) -> AppResult<reqwest::Response> {
    let account = state.accounts.read().ok().and_then(|s| s.accounts.iter().find(|a| a.id == id).cloned())
        .ok_or_else(|| AppError::msg("账号不存在", "请刷新账号列表。"))?;
    if account.kind != AccountKind::Microsoft { return Err(AppError::msg("此功能仅支持微软正版账号", "")); }
    let http = state.http()?;
    for attempt in 0..2 {
        let auth = microsoft_launch_auth(state, &account).await?;
        let mut req = http.raw().request(method.clone(), format!("{PROFILE}{suffix}"))
            .bearer_auth(&auth.access_token).timeout(std::time::Duration::from_secs(45));
        if let Some(body) = &body { req = req.json(body); }
        if let Some((bytes, variant)) = &skin {
            let part = reqwest::multipart::Part::bytes(bytes.clone()).file_name("skin.png").mime_str("image/png").map_err(|_| failure())?;
            req = req.multipart(reqwest::multipart::Form::new().text("variant", variant.clone()).part("file", part));
        }
        let response = req.send().await.map_err(|_| failure())?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
            let stored = state.secrets.get(id).and_then(|s| serde_json::from_str::<MsSession>(&s).ok()).ok_or_else(failure)?;
            let session = launcher_core::auth::microsoft::refresh(http.raw(), &state.settings().effective_client_id(), &stored.ms_refresh_token).await?;
            crate::auth::save_session(state, &session)?;
            continue;
        }
        if !response.status().is_success() {
            return Err(AppError::msg(format!("Minecraft 服务拒绝了操作（{}）", response.status().as_u16()), "请检查登录状态、皮肤文件或稍后重试。"));
        }
        return Ok(response);
    }
    Err(failure())
}
#[tauri::command]
pub async fn account_profile(state: State<'_, AppState>, id: String) -> AppResult<Profile> {
    let profile: Profile = request(&state, &id, reqwest::Method::GET, "", None, None).await?.json().await.map_err(|_| failure())?;
    // Only update an existing entry: an in-flight request must never restore a removed account.
    {
        let mut store = state.accounts.write().map_err(|_| failure())?;
        if let Some(a) = store.accounts.iter_mut().find(|a| a.id == id) {
            a.name = profile.name.clone();
            a.skin = profile.skins.iter().find(|s| s.state == "ACTIVE").map(|s| s.url.clone());
        }
    }
    state.save_accounts()?;
    Ok(profile)
}
#[tauri::command]
pub async fn account_upload_skin(state: State<'_, AppState>, id: String, path: String, variant: String) -> AppResult<()> {
    if variant != "classic" && variant != "slim" { return Err(AppError::msg("请选择正确的皮肤模型", "")); }
    let meta = tokio::fs::metadata(&path).await.map_err(|_| AppError::msg("无法读取皮肤文件", ""))?;
    if meta.len() > 512 * 1024 { return Err(AppError::msg("皮肤文件不能超过 512 KB", "")); }
    let bytes = tokio::fs::read(path).await.map_err(|_| failure())?;
    validate_skin(&bytes)?;
    request(&state, &id, reqwest::Method::POST, "/skins", None, Some((bytes, variant))).await?;
    Ok(())
}
fn validate_skin(bytes: &[u8]) -> AppResult<()> {
    if bytes.len() > 512 * 1024 { return Err(AppError::msg("皮肤文件不能超过 512 KB", "")); }
    let dimensions = image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Png)
        .into_dimensions().map_err(|_| AppError::msg("请选择有效的 PNG 皮肤", ""))?;
    if dimensions.0 != 64 || ![32, 64].contains(&dimensions.1) { return Err(AppError::msg("皮肤尺寸必须为 64×64 或 64×32", "")); }
    image::load_from_memory_with_format(bytes, image::ImageFormat::Png).map_err(|_| AppError::msg("PNG 皮肤已损坏", ""))?;
    Ok(())
}
#[tauri::command]
pub async fn account_set_cape(state: State<'_, AppState>, id: String, cape_id: Option<String>) -> AppResult<()> {
    let (method, body) = match cape_id { Some(id) => (reqwest::Method::PUT, Some(serde_json::json!({"capeId": id}))), None => (reqwest::Method::DELETE, None) };
    request(&state, &id, method, "/capes/active", body, None).await?;
    Ok(())
}
#[tauri::command]
pub async fn account_save_skin(state: State<'_, AppState>, id: String, path: String) -> AppResult<()> {
    let profile = account_profile(state.clone(), id).await?;
    let url = &profile.skins.iter().find(|s| s.state == "ACTIVE").ok_or_else(|| AppError::msg("此账号没有可保存的皮肤", ""))?.url;
    let http = state.http()?;
    let response = http.raw().get(url).timeout(std::time::Duration::from_secs(30)).send().await.map_err(|_| failure())?.error_for_status().map_err(|_| failure())?;
    let bytes = response.bytes().await.map_err(|_| failure())?;
    validate_skin(&bytes)?;
    tokio::fs::write(path, bytes).await.map_err(|_| AppError::msg("保存皮肤失败", "请检查目标文件夹权限。"))?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn rejects_invalid_skin() { assert!(validate_skin(b"not png").is_err()); }
    #[test] fn validates_dimensions() {
        for (w,h,valid) in [(64,64,true),(64,32,true),(32,32,false)] {
            let mut bytes = std::io::Cursor::new(Vec::new());
            image::DynamicImage::new_rgba8(w,h).write_to(&mut bytes, image::ImageFormat::Png).unwrap();
            assert_eq!(validate_skin(bytes.get_ref()).is_ok(),valid);
        }
    }
}

