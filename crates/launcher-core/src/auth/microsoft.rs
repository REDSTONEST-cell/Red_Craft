//! 微软正版登录：设备码流程 → Xbox Live → XSTS → Minecraft Services → 检查拥有游戏 → 获取档案。

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::download::CancelToken;
use crate::error::{CoreError, Result};

const MS_DEVICE_CODE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const MS_TOKEN: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const SCOPE: &str = "XboxLive.signin offline_access";
const XBL_AUTH: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTH: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_ENTITLEMENTS: &str = "https://api.minecraftservices.com/entitlements/mcstore";
const MC_PROFILE: &str = "https://api.minecraftservices.com/minecraft/profile";

/// 编译时通过环境变量 `REDCRAFT_MS_CLIENT_ID` 注入的 Azure 应用 Client ID；未设置时为空，需要在设置中填写。
pub const DEFAULT_CLIENT_ID: &str = match option_env!("REDCRAFT_MS_CLIENT_ID") {
    Some(id) => id,
    None => "",
};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCode {
    #[serde(rename(deserialize = "device_code"))]
    pub device_code: String,
    #[serde(rename(deserialize = "user_code"))]
    pub user_code: String,
    #[serde(rename(deserialize = "verification_uri"))]
    pub verification_uri: String,
    #[serde(rename(deserialize = "expires_in"))]
    pub expires_in: u64,
    #[serde(default = "default_interval")]
    pub interval: u64,
}

fn default_interval() -> u64 {
    5
}

#[derive(Debug, Clone, Deserialize)]
struct OAuthToken {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthError {
    error: String,
    #[serde(default)]
    error_description: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct XboxResponse {
    token: String,
    display_claims: XboxClaims,
}

#[derive(Debug, Deserialize)]
struct XboxClaims {
    xui: Vec<XboxUser>,
}

#[derive(Debug, Deserialize)]
struct XboxUser {
    uhs: String,
    #[serde(default)]
    xid: Option<String>,
}

#[derive(Debug, Deserialize)]
struct XstsError {
    #[serde(rename = "XErr", default)]
    xerr: u64,
}

#[derive(Debug, Deserialize)]
struct McLogin {
    access_token: String,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct Entitlements {
    #[serde(default)]
    items: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McProfile {
    /// 不带连字符的 UUID
    pub id: String,
    pub name: String,
    /// 当前使用的皮肤地址
    #[serde(default)]
    pub skin: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawProfile {
    id: String,
    name: String,
    #[serde(default)]
    skins: Vec<RawSkin>,
}

#[derive(Debug, Deserialize)]
struct RawSkin {
    url: String,
    #[serde(default)]
    state: String,
}

/// 完整登录得到的结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MsSession {
    pub profile: McProfile,
    pub mc_access_token: String,
    /// Minecraft 令牌过期时间（Unix 秒）
    pub mc_expires_at: u64,
    pub ms_refresh_token: String,
    pub xuid: String,
}

impl MsSession {
    /// 令牌是否还能用（留出 10 分钟余量）。
    pub fn is_valid(&self) -> bool {
        self.mc_expires_at > now() + 600
    }
}

fn require_client_id(client_id: &str) -> Result<()> {
    if client_id.trim().is_empty() {
        return Err(CoreError::auth(
            "没有配置微软登录所需的 Azure 应用 Client ID",
            "请在「设置 → 账号」中填写 Client ID，申请方法见 README。",
        ));
    }
    Ok(())
}

async fn read_json<T: for<'de> Deserialize<'de>>(resp: reqwest::Response, what: &str) -> Result<T> {
    let url = resp.url().to_string();
    let bytes = resp.bytes().await.map_err(|e| CoreError::network(url, e))?;
    serde_json::from_slice(&bytes).map_err(|e| CoreError::json(what, e))
}

/// 第一步：申请设备码。界面上展示 `user_code` 与 `verification_uri`，让用户在浏览器中登录。
pub async fn start_device_flow(http: &reqwest::Client, client_id: &str) -> Result<DeviceCode> {
    require_client_id(client_id)?;
    let resp = http
        .post(MS_DEVICE_CODE)
        .form(&[("client_id", client_id), ("scope", SCOPE)])
        .send()
        .await
        .map_err(|e| CoreError::network(MS_DEVICE_CODE, e))?;
    if !resp.status().is_success() {
        let err: OAuthError = read_json(resp, "微软登录响应").await?;
        return Err(oauth_error(&err));
    }
    read_json(resp, "设备码").await
}

/// 第二步：轮询，直到用户在浏览器中完成登录，然后走完后续全部步骤。
pub async fn finish_device_flow(
    http: &reqwest::Client,
    client_id: &str,
    code: &DeviceCode,
    cancel: &CancelToken,
) -> Result<MsSession> {
    let deadline = now() + code.expires_in;
    let mut interval = code.interval.max(1);
    loop {
        cancel.check()?;
        tokio::time::sleep(Duration::from_secs(interval)).await;
        cancel.check()?;
        if now() > deadline {
            return Err(CoreError::auth("验证码已过期", "请重新点击「微软登录」获取新的验证码。"));
        }
        let resp = http
            .post(MS_TOKEN)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", client_id),
                ("device_code", code.device_code.as_str()),
            ])
            .send()
            .await
            .map_err(|e| CoreError::network(MS_TOKEN, e))?;
        if resp.status().is_success() {
            let token: OAuthToken = read_json(resp, "微软令牌").await?;
            return login_with_ms_token(http, token).await;
        }
        let err: OAuthError = read_json(resp, "微软登录响应").await?;
        match err.error.as_str() {
            "authorization_pending" => continue,
            "slow_down" => interval += 5,
            _ => return Err(oauth_error(&err)),
        }
    }
}

/// 用保存的 refresh token 重新登录（启动游戏前令牌过期时调用）。
pub async fn refresh(http: &reqwest::Client, client_id: &str, refresh_token: &str) -> Result<MsSession> {
    require_client_id(client_id)?;
    let resp = http
        .post(MS_TOKEN)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token),
            ("scope", SCOPE),
        ])
        .send()
        .await
        .map_err(|e| CoreError::network(MS_TOKEN, e))?;
    if !resp.status().is_success() {
        let err: OAuthError = read_json(resp, "微软登录响应").await?;
        if err.error == "invalid_grant" {
            return Err(CoreError::auth(
                "微软账号的登录状态已失效",
                "可能修改过密码或长时间未使用，请在「账号」页面重新登录这个微软账号。",
            ));
        }
        return Err(oauth_error(&err));
    }
    let mut token: OAuthToken = read_json(resp, "微软令牌").await?;
    if token.refresh_token.is_none() {
        token.refresh_token = Some(refresh_token.to_string());
    }
    login_with_ms_token(http, token).await
}

async fn login_with_ms_token(http: &reqwest::Client, token: OAuthToken) -> Result<MsSession> {
    let refresh_token = token.refresh_token.clone().unwrap_or_default();

    // Xbox Live
    let resp = http
        .post(XBL_AUTH)
        .header("Accept", "application/json")
        .json(&json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={}", token.access_token),
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        }))
        .send()
        .await
        .map_err(|e| CoreError::network(XBL_AUTH, e))?;
    if !resp.status().is_success() {
        return Err(CoreError::auth(
            format!("Xbox Live 登录失败（状态 {}）", resp.status().as_u16()),
            "请稍后重试；如果持续失败，请确认该微软账号可以正常登录 xbox.com。",
        ));
    }
    let xbl: XboxResponse = read_json(resp, "Xbox Live 响应").await?;

    // XSTS
    let resp = http
        .post(XSTS_AUTH)
        .header("Accept", "application/json")
        .json(&json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl.token] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT",
        }))
        .send()
        .await
        .map_err(|e| CoreError::network(XSTS_AUTH, e))?;
    if resp.status().as_u16() == 401 {
        let err: XstsError = read_json(resp, "XSTS 响应").await.unwrap_or(XstsError { xerr: 0 });
        return Err(xsts_error(err.xerr));
    }
    if !resp.status().is_success() {
        return Err(CoreError::auth(
            format!("XSTS 授权失败（状态 {}）", resp.status().as_u16()),
            "请稍后重试。",
        ));
    }
    let xsts: XboxResponse = read_json(resp, "XSTS 响应").await?;
    let user = xsts
        .display_claims
        .xui
        .first()
        .ok_or_else(|| CoreError::auth("XSTS 响应中缺少用户信息", "请稍后重试。"))?;
    let uhs = user.uhs.clone();
    let xuid = user.xid.clone().unwrap_or_default();

    // Minecraft Services
    let resp = http
        .post(MC_LOGIN)
        .json(&json!({ "identityToken": format!("XBL3.0 x={uhs};{}", xsts.token) }))
        .send()
        .await
        .map_err(|e| CoreError::network(MC_LOGIN, e))?;
    match resp.status().as_u16() {
        200 => {}
        403 => {
            return Err(CoreError::auth(
                "Minecraft 拒绝了这个启动器的登录请求（应用尚未获得授权）",
                "Azure 应用需要先通过 Minecraft API 权限审核（https://aka.ms/mce-reviewappid）。审核通过前无法使用微软登录，可以先使用离线账号。",
            ))
        }
        429 => {
            return Err(CoreError::auth(
                "登录过于频繁",
                "请等待几分钟后再试。",
            ))
        }
        s => {
            return Err(CoreError::auth(
                format!("Minecraft 登录失败（状态 {s}）"),
                "Minecraft 服务可能暂时不可用，请稍后重试。",
            ))
        }
    }
    let mc: McLogin = read_json(resp, "Minecraft 登录响应").await?;
    let bearer = format!("Bearer {}", mc.access_token);

    // 档案（同时用于判断是否拥有游戏）
    let resp = http
        .get(MC_PROFILE)
        .header("Authorization", &bearer)
        .send()
        .await
        .map_err(|e| CoreError::network(MC_PROFILE, e))?;
    if resp.status().as_u16() == 404 {
        // 没有档案：要么没买游戏，要么买了但还没设置角色名
        let owns = http
            .get(MC_ENTITLEMENTS)
            .header("Authorization", &bearer)
            .send()
            .await
            .ok()
            .filter(|r| r.status().is_success());
        let owns = match owns {
            Some(r) => read_json::<Entitlements>(r, "游戏授权").await.is_ok_and(|e| !e.items.is_empty()),
            None => false,
        };
        return Err(if owns {
            CoreError::auth(
                "这个账号还没有创建 Minecraft 角色",
                "请先在 minecraft.net 或官方启动器中设置角色名，然后重新登录。",
            )
        } else {
            CoreError::auth(
                "这个微软账号没有购买 Minecraft: Java 版",
                "请确认登录的是购买了游戏的账号；Xbox Game Pass 用户需要先在官方启动器中启动一次 Java 版。",
            )
        });
    }
    if !resp.status().is_success() {
        return Err(CoreError::auth(
            format!("获取角色档案失败（状态 {}）", resp.status().as_u16()),
            "请稍后重试。",
        ));
    }
    let raw: RawProfile = read_json(resp, "角色档案").await?;
    Ok(MsSession {
        profile: profile_from_raw(raw),
        mc_access_token: mc.access_token,
        mc_expires_at: now() + mc.expires_in,
        ms_refresh_token: refresh_token,
        xuid,
    })
}

fn profile_from_raw(raw: RawProfile) -> McProfile {
    let skin = raw
        .skins
        .iter()
        .find(|s| s.state == "ACTIVE")
        .or(raw.skins.first())
        .map(|s| s.url.replace("http://", "https://"));
    McProfile {
        id: raw.id,
        name: raw.name,
        skin,
    }
}

fn oauth_error(err: &OAuthError) -> CoreError {
    match err.error.as_str() {
        "expired_token" => CoreError::auth("验证码已过期", "请重新点击「微软登录」获取新的验证码。"),
        "authorization_declined" | "access_denied" => {
            CoreError::auth("你在网页上拒绝了授权", "如需登录，请重新开始并在网页上点击「是」允许访问。")
        }
        "invalid_client" | "unauthorized_client" => CoreError::auth(
            "Azure 应用配置不正确",
            "请确认 Client ID 填写正确，并在 Azure 应用的「验证」页面开启「允许公共客户端流」。",
        ),
        "invalid_grant" => CoreError::auth("登录凭据已失效", "请重新登录这个微软账号。"),
        _ => CoreError::auth(
            format!("微软登录失败：{}", err.error),
            if err.error_description.is_empty() {
                "请稍后重试。".to_string()
            } else {
                format!("服务器返回：{}", err.error_description.lines().next().unwrap_or_default())
            },
        ),
    }
}

/// XSTS 常见错误码。
fn xsts_error(code: u64) -> CoreError {
    match code {
        2148916233 => CoreError::auth(
            "这个微软账号还没有 Xbox 档案",
            "请先访问 xbox.com 登录一次并创建 Xbox 档案，然后重新登录。",
        ),
        2148916235 => CoreError::auth(
            "你所在的国家或地区不支持 Xbox Live",
            "这是微软的地区限制，无法通过启动器解决。",
        ),
        2148916236 | 2148916237 => CoreError::auth(
            "这个账号需要完成成人验证",
            "根据韩国法规，请先在 xbox.com 完成成人验证。",
        ),
        2148916238 => CoreError::auth(
            "这是未成年人账号，需要家长同意",
            "请让家长在 account.microsoft.com/family 中把这个账号加入家庭组并允许其使用 Xbox。",
        ),
        2148916227 => CoreError::auth("这个 Xbox 账号已被封禁", "请联系 Xbox 客服。"),
        _ => CoreError::auth(
            format!("Xbox 授权失败（错误码 {code}）"),
            "请访问 xbox.com 确认账号状态正常后重试。",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_device_code() {
        let d: DeviceCode = serde_json::from_str(
            r#"{"device_code":"DC","user_code":"ABCD1234","verification_uri":"https://www.microsoft.com/link","expires_in":900,"interval":5,"message":"..."}"#,
        )
        .unwrap();
        assert_eq!(d.user_code, "ABCD1234");
        assert_eq!(d.expires_in, 900);
        // 发给前端时使用 camelCase
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["userCode"], "ABCD1234");
        assert_eq!(v["verificationUri"], "https://www.microsoft.com/link");
    }

    #[test]
    fn parse_xbox_and_profile() {
        let x: XboxResponse = serde_json::from_str(
            r#"{"IssueInstant":"x","NotAfter":"y","Token":"T","DisplayClaims":{"xui":[{"uhs":"123","xid":"2535"}]}}"#,
        )
        .unwrap();
        assert_eq!(x.token, "T");
        assert_eq!(x.display_claims.xui[0].uhs, "123");

        let p: RawProfile = serde_json::from_str(
            r#"{"id":"069a79f444e94726a5befca90e38aaf5","name":"Notch","skins":[
                {"id":"a","state":"INACTIVE","url":"http://textures.minecraft.net/texture/old","variant":"CLASSIC"},
                {"id":"b","state":"ACTIVE","url":"http://textures.minecraft.net/texture/new","variant":"CLASSIC"}],"capes":[]}"#,
        )
        .unwrap();
        let p = profile_from_raw(p);
        assert_eq!(p.name, "Notch");
        assert_eq!(p.skin.as_deref(), Some("https://textures.minecraft.net/texture/new"));
    }

    #[test]
    fn error_messages() {
        let e = xsts_error(2148916238);
        assert!(e.to_string().contains("未成年"));
        assert!(e.hint().contains("家庭"));
        assert!(xsts_error(2148916233).to_string().contains("Xbox 档案"));
        let e = oauth_error(&OAuthError { error: "invalid_client".into(), error_description: String::new() });
        assert!(e.hint().contains("公共客户端"));
        assert!(require_client_id(" ").is_err());
    }

    #[test]
    fn session_validity() {
        let mut s = MsSession {
            profile: McProfile { id: "a".into(), name: "b".into(), skin: None },
            mc_access_token: "t".into(),
            mc_expires_at: now() + 3600,
            ms_refresh_token: "r".into(),
            xuid: String::new(),
        };
        assert!(s.is_valid());
        s.mc_expires_at = now() + 60;
        assert!(!s.is_valid());
    }
}
