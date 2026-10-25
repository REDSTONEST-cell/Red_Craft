//! 资源图标与中文简介。
//!
//! 国内网络下 `cdn.modrinth.com` / `media.forgecdn.net` 经常无法访问，图标改由启动器后端获取：
//! 官方地址与 MCIM 镜像同时请求，取先成功的一个。简介的中文翻译来自 MCIM 的翻译接口。

use std::collections::HashMap;
use std::time::Duration;

use futures_util::future::select_ok;
use serde::Deserialize;
use serde_json::json;

use super::Platform;
use crate::download::{mirror_of, MCIM};

/// 允许代理的图标域名，避免把任意地址交给后端请求。
const ICON_HOSTS: &[&str] = &[
    "https://cdn.modrinth.com/",
    "https://media.forgecdn.net/",
    "https://edge.forgecdn.net/",
    "https://mediafilez.forgecdn.net/",
];

const ICON_MAX: usize = 2 * 1024 * 1024;

pub fn is_icon_url(url: &str) -> bool {
    ICON_HOSTS.iter().any(|h| url.starts_with(h))
}

/// 根据文件头判断图片类型；不是图片时返回 `None`。
pub fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.starts_with(b"GIF8") {
        Some("image/gif")
    } else {
        None
    }
}

/// 同时请求官方地址与镜像，返回先成功的图片内容。
pub async fn fetch_icon(http: &reqwest::Client, url: &str) -> Option<Vec<u8>> {
    if !is_icon_url(url) {
        return None;
    }
    let mut urls = vec![url.to_string()];
    urls.extend(mirror_of(url));
    let tasks = urls.into_iter().map(|u| {
        Box::pin(async move {
            let resp = http
                .get(&u)
                .timeout(Duration::from_secs(12))
                .send()
                .await
                .map_err(|_| ())?;
            if !resp.status().is_success() {
                return Err(());
            }
            let bytes = resp.bytes().await.map_err(|_| ())?;
            if bytes.len() > ICON_MAX || image_mime(&bytes).is_none() {
                return Err(());
            }
            Ok(bytes.to_vec())
        })
    });
    select_ok(tasks).await.ok().map(|(b, _)| b)
}

#[derive(Deserialize)]
struct Translation {
    #[serde(alias = "project_id", alias = "modid")]
    id: serde_json::Value,
    translated: Option<String>,
}

/// 批量获取简介的中文翻译（MCIM），返回 id → 中文简介。失败时返回空表，不影响搜索。
pub async fn translations(http: &reqwest::Client, platform: Platform, ids: &[String]) -> HashMap<String, String> {
    if ids.is_empty() {
        return HashMap::new();
    }
    let (url, body) = match platform {
        Platform::Modrinth => (format!("{MCIM}/translate/modrinth"), json!({ "project_ids": ids })),
        Platform::Curseforge => {
            let nums: Vec<u64> = ids.iter().filter_map(|i| i.parse().ok()).collect();
            (format!("{MCIM}/translate/curseforge"), json!({ "modids": nums }))
        }
    };
    let Ok(resp) = http.post(&url).json(&body).timeout(Duration::from_secs(10)).send().await else {
        return HashMap::new();
    };
    let list: Vec<Translation> = resp.json().await.unwrap_or_default();
    list.into_iter()
        .filter_map(|t| {
            let id = match t.id {
                serde_json::Value::String(s) => s,
                serde_json::Value::Number(n) => n.to_string(),
                _ => return None,
            };
            let text = t.translated?.trim().to_string();
            (!text.is_empty()).then_some((id, text))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_hosts_and_types() {
        assert!(is_icon_url("https://cdn.modrinth.com/data/AANobbMI/icon.png"));
        assert!(is_icon_url("https://media.forgecdn.net/avatars/1.png"));
        assert!(!is_icon_url("https://example.com/a.png"));
        assert!(!is_icon_url("https://cdn.modrinth.com.evil.com/a.png"));
        assert_eq!(image_mime(b"\x89PNG\r\n"), Some("image/png"));
        assert_eq!(image_mime(b"RIFF0000WEBPVP8 "), Some("image/webp"));
        assert_eq!(image_mime(b"<html>"), None);
    }

    #[tokio::test]
    #[ignore]
    async fn real_translations() {
        let http = reqwest::Client::new();
        let t = translations(&http, Platform::Modrinth, &["AANobbMI".into()]).await;
        assert!(t["AANobbMI"].chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));
        let t = translations(&http, Platform::Curseforge, &["238222".into()]).await;
        assert!(t.contains_key("238222"));
        let icon = fetch_icon(&http, "https://cdn.modrinth.com/data/AANobbMI/295862f4724dc3f78df3447ad6072b2dcd3ef0c9_96.webp").await;
        assert!(icon.is_some());
    }
}
