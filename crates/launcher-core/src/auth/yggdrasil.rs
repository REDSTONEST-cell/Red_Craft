//! 外置登录（Yggdrasil / authlib-injector），例如 LittleSkin。
//!
//! - 地址支持 ALI（API 地址指示）：请求用户输入的地址，若响应头带有
//!   `X-Authlib-Injector-API-Location`，则以它为准；
//! - 登录：`/authserver/authenticate`，多个角色时由用户选择后 `/authserver/refresh` 绑定；
//! - 启动前：`/authserver/validate` 检查令牌，失效时 `/authserver/refresh` 刷新；
//! - 启动参数：`-javaagent:authlib-injector.jar=<API 地址>` 与
//!   `-Dauthlibinjector.yggdrasil.prefetched=<base64 元数据>`。

use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::download::{CancelToken, DownloadTask, Downloader};
use crate::error::{CoreError, IoContext, Result};
use crate::install::Reporter;

const TIMEOUT: Duration = Duration::from_secs(15);
const AUTHLIB_LATEST: &str = "https://authlib-injector.yushi.moe/artifact/latest.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct YggProfile {
    /// 不带连字符的 UUID
    pub id: String,
    pub name: String,
}

/// 认证服务器信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YggServer {
    /// API 根地址（以 / 结尾）
    pub api_root: String,
    pub name: String,
    /// 服务器元数据的 base64，用于 `-Dauthlibinjector.yggdrasil.prefetched`
    pub prefetched: String,
    /// 注册页面（有的话）
    pub register_url: Option<String>,
}

/// 保存在密钥库中的会话。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YggSession {
    pub access_token: String,
    pub client_token: String,
    pub profile: Option<YggProfile>,
}

/// 登录结果：角色列表与（可能已绑定的）选中角色。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YggLogin {
    pub session: YggSession,
    pub profiles: Vec<YggProfile>,
}

fn http_err(url: &str, e: reqwest::Error) -> CoreError {
    CoreError::network(url, e)
}

/// 规范化用户输入的地址：补全 https://，去掉多余空白。
pub fn normalize_url(input: &str) -> String {
    let s = input.trim().trim_end_matches('/');
    if s.starts_with("http://") || s.starts_with("https://") {
        s.to_string()
    } else {
        format!("https://{s}")
    }
}

fn with_slash(s: &str) -> String {
    if s.ends_with('/') {
        s.to_string()
    } else {
        format!("{s}/")
    }
}

/// 解析 ALI 指向的地址（可能是相对地址）。
fn resolve_location(base: &str, location: &str) -> String {
    if location.starts_with("http://") || location.starts_with("https://") {
        return location.to_string();
    }
    match reqwest::Url::parse(&with_slash(base)).and_then(|b| b.join(location)) {
        Ok(u) => u.to_string(),
        Err(_) => location.to_string(),
    }
}

/// 解析认证服务器：处理 ALI，获取元数据。
pub async fn resolve_server(http: &reqwest::Client, input: &str) -> Result<YggServer> {
    let url = normalize_url(input);
    if url == "https://" {
        return Err(CoreError::auth("请输入认证服务器地址", "例如 littleskin.cn。"));
    }
    let resp = http
        .get(&url)
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|e| http_err(&url, e))?;
    let api_root = match resp
        .headers()
        .get("x-authlib-injector-api-location")
        .and_then(|v| v.to_str().ok())
    {
        Some(loc) => with_slash(&resolve_location(&url, loc)),
        None => with_slash(&url),
    };
    // ALI 指向其它地址时重新获取元数据
    let body = if api_root.trim_end_matches('/') == url.trim_end_matches('/') {
        resp.bytes().await.map_err(|e| http_err(&url, e))?.to_vec()
    } else {
        http.get(&api_root)
            .timeout(TIMEOUT)
            .send()
            .await
            .map_err(|e| http_err(&api_root, e))?
            .bytes()
            .await
            .map_err(|e| http_err(&api_root, e))?
            .to_vec()
    };
    let meta: Value = serde_json::from_slice(&body).map_err(|_| {
        CoreError::auth(
            format!("{url} 不是有效的外置登录服务器"),
            "请检查地址是否正确，例如 LittleSkin 填写 littleskin.cn；地址应当是认证服务器而不是皮肤站首页以外的页面。",
        )
    })?;
    if meta.get("meta").is_none() && meta.get("signaturePublickey").is_none() {
        return Err(CoreError::auth(
            format!("{url} 不是有效的外置登录服务器"),
            "请检查地址是否正确，例如 LittleSkin 填写 littleskin.cn。",
        ));
    }
    Ok(YggServer {
        name: meta
            .pointer("/meta/serverName")
            .and_then(Value::as_str)
            .unwrap_or("外置登录服务器")
            .to_string(),
        register_url: meta.pointer("/meta/links/register").and_then(Value::as_str).map(str::to_string),
        prefetched: base64::engine::general_purpose::STANDARD.encode(&body),
        api_root,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct YggError {
    #[serde(default)]
    error: String,
    #[serde(default)]
    error_message: String,
}

async fn post(http: &reqwest::Client, url: &str, body: &Value) -> Result<reqwest::Response> {
    http.post(url)
        .timeout(TIMEOUT)
        .json(body)
        .send()
        .await
        .map_err(|e| http_err(url, e))
}

async fn ygg_error(resp: reqwest::Response) -> CoreError {
    let status = resp.status().as_u16();
    let err: YggError = resp.json().await.unwrap_or(YggError {
        error: String::new(),
        error_message: String::new(),
    });
    let msg = err.error_message.to_lowercase();
    if err.error == "ForbiddenOperationException" && (msg.contains("invalid credentials") || msg.contains("密码")) {
        return CoreError::auth("用户名或密码错误", "请检查输入；LittleSkin 等皮肤站可以使用邮箱或角色名登录。");
    }
    if err.error == "ForbiddenOperationException" && msg.contains("token") {
        return CoreError::auth("登录已过期", "请在「账号」页面删除这个账号后重新登录。");
    }
    if status == 429 || msg.contains("too many") {
        return CoreError::auth("登录尝试过于频繁", "请等待几分钟后再试。");
    }
    let detail = if err.error_message.is_empty() { err.error } else { err.error_message };
    CoreError::auth(
        format!("外置登录失败（状态 {status}）{}", if detail.is_empty() { String::new() } else { format!("：{detail}") }),
        "请稍后重试；如果持续失败，请到皮肤站确认账号状态。",
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthResponse {
    access_token: String,
    client_token: String,
    #[serde(default)]
    available_profiles: Vec<YggProfile>,
    selected_profile: Option<YggProfile>,
}

fn random_client_token() -> String {
    let mut buf = [0u8; 16];
    let _ = getrandom::fill(&mut buf);
    hex::encode(buf)
}

/// 用账号密码登录。密码只用于这一次请求，不会保存。
pub async fn authenticate(http: &reqwest::Client, server: &YggServer, username: &str, password: &str) -> Result<YggLogin> {
    if username.trim().is_empty() || password.is_empty() {
        return Err(CoreError::auth("请输入账号和密码", ""));
    }
    let url = format!("{}authserver/authenticate", server.api_root);
    let resp = post(
        http,
        &url,
        &json!({
            "agent": { "name": "Minecraft", "version": 1 },
            "username": username.trim(),
            "password": password,
            "clientToken": random_client_token(),
            "requestUser": true,
        }),
    )
    .await?;
    if !resp.status().is_success() {
        return Err(ygg_error(resp).await);
    }
    let r: AuthResponse = resp.json().await.map_err(|e| http_err(&url, e))?;
    if r.available_profiles.is_empty() && r.selected_profile.is_none() {
        return Err(CoreError::auth(
            "这个账号还没有角色",
            "请先到皮肤站创建一个角色，然后重新登录。",
        ));
    }
    let selected = r
        .selected_profile
        .or_else(|| (r.available_profiles.len() == 1).then(|| r.available_profiles[0].clone()));
    Ok(YggLogin {
        session: YggSession {
            access_token: r.access_token,
            client_token: r.client_token,
            profile: selected,
        },
        profiles: r.available_profiles,
    })
}

/// 刷新令牌；`select` 为要绑定的角色（多角色登录后选择角色时使用）。
pub async fn refresh(http: &reqwest::Client, server: &YggServer, session: &YggSession, select: Option<&YggProfile>) -> Result<YggSession> {
    let url = format!("{}authserver/refresh", server.api_root);
    let mut body = json!({
        "accessToken": session.access_token,
        "clientToken": session.client_token,
        "requestUser": true,
    });
    if let Some(p) = select {
        body["selectedProfile"] = json!({ "id": p.id, "name": p.name });
    }
    let resp = post(http, &url, &body).await?;
    if !resp.status().is_success() {
        let e = ygg_error(resp).await;
        return Err(match e {
            CoreError::Auth { .. } if select.is_none() => CoreError::auth(
                "外置登录已过期",
                "为了安全，启动器不保存密码。请在「账号」页面删除这个账号后重新登录。",
            ),
            other => other,
        });
    }
    let r: AuthResponse = resp.json().await.map_err(|e| http_err(&url, e))?;
    Ok(YggSession {
        access_token: r.access_token,
        client_token: r.client_token,
        profile: r.selected_profile.or_else(|| select.cloned()).or_else(|| session.profile.clone()),
    })
}

/// 令牌是否仍然有效（`/authserver/validate` 返回 204）。
pub async fn validate(http: &reqwest::Client, server: &YggServer, session: &YggSession) -> bool {
    let url = format!("{}authserver/validate", server.api_root);
    match post(
        http,
        &url,
        &json!({ "accessToken": session.access_token, "clientToken": session.client_token }),
    )
    .await
    {
        Ok(r) => r.status().is_success(),
        Err(_) => false,
    }
}

/// 启动前确保令牌可用：有效则直接使用，否则刷新。
pub async fn ensure_valid(http: &reqwest::Client, server: &YggServer, session: &YggSession) -> Result<YggSession> {
    if validate(http, server, session).await {
        return Ok(session.clone());
    }
    refresh(http, server, session, None).await
}

/// 获取角色皮肤图片（返回 data URL，避免界面跨域加载第三方图片）。失败时返回 `None`。
pub async fn skin_data_url(http: &reqwest::Client, server: &YggServer, profile_id: &str) -> Option<String> {
    let url = format!("{}sessionserver/session/minecraft/profile/{profile_id}", server.api_root);
    let v: Value = http.get(&url).timeout(TIMEOUT).send().await.ok()?.json().await.ok()?;
    let textures = v
        .get("properties")?
        .as_array()?
        .iter()
        .find(|p| p.get("name").and_then(Value::as_str) == Some("textures"))?
        .get("value")?
        .as_str()?;
    let decoded = base64::engine::general_purpose::STANDARD.decode(textures).ok()?;
    let t: Value = serde_json::from_slice(&decoded).ok()?;
    let skin = t.pointer("/textures/SKIN/url")?.as_str()?;
    let bytes = http.get(skin).timeout(TIMEOUT).send().await.ok()?.bytes().await.ok()?;
    if bytes.len() > 512 * 1024 {
        return None;
    }
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    ))
}

#[derive(Deserialize)]
struct AuthlibLatest {
    version: String,
    download_url: String,
    checksums: AuthlibChecksums,
}

#[derive(Deserialize)]
struct AuthlibChecksums {
    sha256: String,
}

fn sha256_file(path: &Path) -> Result<String> {
    use sha2::Digest;
    let data = std::fs::read(path).at(path)?;
    Ok(hex::encode(sha2::Sha256::digest(&data)))
}

/// 确保 authlib-injector 已下载到 `dir`，返回 jar 路径。网络不可用时使用已有的最新版本。
pub async fn ensure_authlib_injector(
    downloader: &Downloader,
    dir: &Path,
    reporter: &Reporter,
    cancel: &CancelToken,
) -> Result<PathBuf> {
    let latest: std::result::Result<AuthlibLatest, CoreError> =
        downloader.http().get_json(AUTHLIB_LATEST, "authlib-injector 版本信息").await;
    let latest = match latest {
        Ok(l) => l,
        Err(e) => {
            // 离线时退回已下载的版本
            let existing = std::fs::read_dir(dir)
                .ok()
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "jar"))
                .max();
            return existing.ok_or(e);
        }
    };
    let path = dir.join(format!("authlib-injector-{}.jar", latest.version));
    if path.is_file() && sha256_file(&path)?.eq_ignore_ascii_case(&latest.checksums.sha256) {
        return Ok(path);
    }
    reporter.stage("下载 authlib-injector");
    downloader
        .download_all(vec![DownloadTask::new(&latest.download_url, &path)], reporter.progress.clone(), cancel)
        .await?;
    let actual = sha256_file(&path)?;
    if !actual.eq_ignore_ascii_case(&latest.checksums.sha256) {
        let _ = std::fs::remove_file(&path);
        return Err(CoreError::Checksum {
            path,
            expected: latest.checksums.sha256,
            actual,
        });
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn urls() {
        assert_eq!(normalize_url(" littleskin.cn/ "), "https://littleskin.cn");
        assert_eq!(normalize_url("http://127.0.0.1:8080"), "http://127.0.0.1:8080");
        assert_eq!(resolve_location("https://a.com", "/api/yggdrasil"), "https://a.com/api/yggdrasil");
        assert_eq!(resolve_location("https://a.com/x", "https://b.com/y"), "https://b.com/y");
    }

    /// 极简的 Yggdrasil 模拟服务器：根路径用 ALI 指向 /api/，账号有两个角色。
    async fn mock_server() -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let log = Arc::new(Mutex::new(Vec::new()));
        let l2 = log.clone();
        tokio::spawn(async move {
            let tokens: Arc<Mutex<HashMap<String, bool>>> = Arc::new(Mutex::new(HashMap::new()));
            loop {
                let Ok((mut s, _)) = listener.accept().await else { break };
                let log = l2.clone();
                let tokens = tokens.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 65536];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let line = req.lines().next().unwrap_or_default().to_string();
                    let body = req.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
                    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                    log.lock().unwrap().push(path.clone());
                    let j: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                    let (status, extra, out) = match path.as_str() {
                        "/" => ("200 OK", "X-Authlib-Injector-API-Location: /api/\r\n", "<html></html>".to_string()),
                        "/api/" => ("200 OK", "", r#"{"meta":{"serverName":"模拟皮肤站"},"skinDomains":[]}"#.to_string()),
                        "/api/authserver/authenticate" => {
                            if j["password"] == "right" {
                                tokens.lock().unwrap().insert("t1".into(), true);
                                ("200 OK", "", json!({
                                    "accessToken": "t1", "clientToken": j["clientToken"],
                                    "availableProfiles": [{"id": "aaa", "name": "Alice"}, {"id": "bbb", "name": "Bob"}]
                                }).to_string())
                            } else {
                                ("403 Forbidden", "", r#"{"error":"ForbiddenOperationException","errorMessage":"Invalid credentials. Invalid username or password."}"#.to_string())
                            }
                        }
                        "/api/authserver/refresh" => {
                            let ok = tokens.lock().unwrap().remove(j["accessToken"].as_str().unwrap_or("")).is_some();
                            if ok {
                                tokens.lock().unwrap().insert("t2".into(), true);
                                let sel = if j["selectedProfile"].is_null() { json!({"id": "bbb", "name": "Bob"}) } else { j["selectedProfile"].clone() };
                                ("200 OK", "", json!({"accessToken": "t2", "clientToken": j["clientToken"], "selectedProfile": sel}).to_string())
                            } else {
                                ("403 Forbidden", "", r#"{"error":"ForbiddenOperationException","errorMessage":"Invalid token."}"#.to_string())
                            }
                        }
                        "/api/authserver/validate" => {
                            if tokens.lock().unwrap().contains_key(j["accessToken"].as_str().unwrap_or("")) {
                                ("204 No Content", "", String::new())
                            } else {
                                ("403 Forbidden", "", r#"{"error":"ForbiddenOperationException","errorMessage":"Invalid token."}"#.to_string())
                            }
                        }
                        _ => ("404 Not Found", "", String::new()),
                    };
                    let resp = format!(
                        "HTTP/1.1 {status}\r\n{extra}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
                        out.len()
                    );
                    let _ = s.write_all(resp.as_bytes()).await;
                });
            }
        });
        (base, log)
    }

    #[tokio::test]
    async fn full_flow_against_mock() {
        let (base, log) = mock_server().await;
        let http = reqwest::Client::builder().no_proxy().build().unwrap();

        let server = resolve_server(&http, &base).await.unwrap();
        assert_eq!(server.api_root, format!("{base}/api/"));
        assert_eq!(server.name, "模拟皮肤站");
        let meta = base64::engine::general_purpose::STANDARD.decode(&server.prefetched).unwrap();
        assert!(String::from_utf8(meta).unwrap().contains("模拟皮肤站"));

        let err = authenticate(&http, &server, "a@b.c", "wrong").await.unwrap_err();
        assert!(err.to_string().contains("用户名或密码错误"));

        // 两个角色：需要选择后绑定
        let login = authenticate(&http, &server, "a@b.c", "right").await.unwrap();
        assert_eq!(login.profiles.len(), 2);
        assert!(login.session.profile.is_none());
        let bob = login.profiles[1].clone();
        let session = refresh(&http, &server, &login.session, Some(&bob)).await.unwrap();
        assert_eq!(session.profile.as_ref().unwrap().name, "Bob");
        assert_eq!(session.access_token, "t2");

        // 有效令牌直接使用；失效令牌刷新失败时提示重新登录
        assert!(validate(&http, &server, &session).await);
        let same = ensure_valid(&http, &server, &session).await.unwrap();
        assert_eq!(same.access_token, "t2");
        let stale = YggSession { access_token: "gone".into(), ..session.clone() };
        let err = ensure_valid(&http, &server, &stale).await.unwrap_err();
        assert!(err.to_string().contains("过期"));

        let paths = log.lock().unwrap().clone();
        assert!(paths.contains(&"/api/authserver/validate".to_string()));
    }
}
