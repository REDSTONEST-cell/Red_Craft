//! 离线登录。

use md5::{Digest, Md5};

use crate::error::{CoreError, Result};

/// 与服务端一致的离线 UUID：对 `OfflinePlayer:<name>` 做 MD5，并按 UUID v3 设置版本与变体位。
pub fn offline_uuid(name: &str) -> String {
    let mut bytes: [u8; 16] = Md5::digest(format!("OfflinePlayer:{name}").as_bytes()).into();
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let h = hex::encode(bytes);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// 检查离线用户名。正版规则是 3~16 位字母、数字、下划线；
/// 更宽松的名字在单机里可以用，但多人游戏可能会被服务器拒绝，这里只拦截确实无法使用的。
pub fn validate_offline_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(CoreError::InvalidData("用户名不能为空".into()));
    }
    if name.chars().count() > 16 {
        return Err(CoreError::InvalidData("用户名不能超过 16 个字符".into()));
    }
    if name.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(CoreError::InvalidData("用户名不能包含空格".into()));
    }
    Ok(())
}

/// 用户名是否符合正版规则（不符合时前端给出提醒）。
pub fn is_standard_name(name: &str) -> bool {
    (3..=16).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// 查询同名正版账号的皮肤，返回 PNG 的 data URL，用作离线账号的头像（界面无需再访问外网）。
/// 查不到（没有同名正版账号或网络失败）时返回 `None`。
pub async fn lookup_skin(http: &reqwest::Client, name: &str) -> Option<String> {
    use base64::Engine;

    #[derive(serde::Deserialize)]
    struct Profile {
        id: String,
        #[serde(default)]
        properties: Vec<Property>,
    }
    #[derive(serde::Deserialize)]
    struct Property {
        name: String,
        value: String,
    }

    let timeout = std::time::Duration::from_secs(10);
    let get_profile = |url: String| async move {
        let r = http.get(&url).timeout(timeout).send().await.ok()?;
        if !r.status().is_success() {
            return None;
        }
        r.json::<Profile>().await.ok()
    };
    // 两个查询接口同时请求，取先成功的（国内网络下 api.mojang.com 经常超时）
    let a = get_profile(format!("https://api.mojang.com/users/profiles/minecraft/{name}"));
    let b = get_profile(format!("https://api.minecraftservices.com/minecraft/profile/lookup/name/{name}"));
    let profile = first_some(a, b).await;
    let Some(profile) = profile else {
        log::info!("没有查到名为 {name} 的正版账号（或网络不可用）");
        return None;
    };
    let full = get_profile(format!(
        "https://sessionserver.mojang.com/session/minecraft/profile/{}",
        profile.id
    ))
    .await?;
    let textures = full.properties.into_iter().find(|p| p.name == "textures")?;
    let decoded = base64::engine::general_purpose::STANDARD.decode(textures.value).ok()?;
    let url = skin_url_from_textures(&decoded)?;
    super::png_data_url(http, &url).await
}

/// 两个 future 同时执行，返回先得到的 `Some`；都为 `None` 时返回 `None`。
async fn first_some<T>(
    a: impl std::future::Future<Output = Option<T>>,
    b: impl std::future::Future<Output = Option<T>>,
) -> Option<T> {
    tokio::pin!(a, b);
    let (mut a_done, mut b_done) = (false, false);
    loop {
        tokio::select! {
            r = &mut a, if !a_done => { if r.is_some() { return r; } a_done = true; }
            r = &mut b, if !b_done => { if r.is_some() { return r; } b_done = true; }
            else => return None,
        }
    }
}

fn skin_url_from_textures(json: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(json).ok()?;
    let url = v.get("textures")?.get("SKIN")?.get("url")?.as_str()?;
    Some(url.replace("http://", "https://"))
}

#[cfg(test)]
mod tests {
    /// 联网测试：`cargo test -p launcher-core -- --ignored lookup_real_skin`
    #[tokio::test]
    #[ignore]
    async fn lookup_real_skin() {
        let http = reqwest::Client::new();
        let skin = super::lookup_skin(&http, "REDSTONE_RT").await.expect("应查到皮肤");
        assert!(skin.starts_with("data:image/png;base64,"));
        assert!(super::lookup_skin(&http, "zz_no_such_player_zz").await.is_none());
    }

    #[test]
    fn textures_payload() {
        let json = br#"{"timestamp":1,"profileId":"x","profileName":"Notch","textures":{"SKIN":{"url":"http://textures.minecraft.net/texture/abc"}}}"#;
        assert_eq!(
            super::skin_url_from_textures(json).as_deref(),
            Some("https://textures.minecraft.net/texture/abc")
        );
        assert!(super::skin_url_from_textures(br#"{"textures":{}}"#).is_none());
    }

    use super::*;

    #[test]
    fn known_offline_uuids() {
        // 与服务端 `UUID.nameUUIDFromBytes("OfflinePlayer:Notch")` 的结果一致
        assert_eq!(offline_uuid("Notch"), "b50ad385-829d-3141-a216-7e7d7539ba7f");
        assert_eq!(offline_uuid("Steve"), "5627dd98-e6be-3c21-b8a8-e92344183641");
        let u = offline_uuid("测试玩家");
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "3");
        assert!(matches!(&u[19..20], "8" | "9" | "a" | "b"));
    }

    #[test]
    fn names() {
        assert!(validate_offline_name("Steve").is_ok());
        assert!(validate_offline_name("").is_err());
        assert!(validate_offline_name("a b").is_err());
        assert!(validate_offline_name("12345678901234567").is_err());
        assert!(is_standard_name("Player_1"));
        assert!(!is_standard_name("玩家"));
        assert!(!is_standard_name("ab"));
    }
}
