//! Fabric 与 Quilt：两者的 meta 接口格式相同，直接提供可继承原版的 profile JSON。

use serde::Deserialize;

use super::LoaderVersion;
use crate::download::HttpClient;
use crate::error::{CoreError, Result};
use crate::instance::LoaderKind;

fn meta_base(kind: LoaderKind) -> &'static str {
    match kind {
        LoaderKind::Quilt => "https://meta.quiltmc.org/v3",
        _ => "https://meta.fabricmc.net/v2",
    }
}

#[derive(Debug, Deserialize)]
struct Entry {
    loader: LoaderInfo,
}

#[derive(Debug, Deserialize)]
struct LoaderInfo {
    version: String,
    /// Quilt 的接口没有 stable 字段，用版本号是否含 beta 判断
    #[serde(default)]
    stable: Option<bool>,
}

pub async fn list(http: &HttpClient, kind: LoaderKind, mc: &str) -> Result<Vec<LoaderVersion>> {
    let url = format!("{}/versions/loader/{mc}", meta_base(kind));
    let entries: Vec<Entry> = http.get_json(&url, &format!("{} 版本列表", kind.display_name())).await?;
    Ok(entries
        .into_iter()
        .map(|e| {
            let stable = e
                .loader
                .stable
                .unwrap_or_else(|| !e.loader.version.contains("beta") && !e.loader.version.contains("pre"));
            LoaderVersion {
                display: e.loader.version.clone(),
                version: e.loader.version,
                stable,
            }
        })
        .collect())
}

/// 获取 profile JSON 并改写为实例名。
pub async fn profile(http: &HttpClient, kind: LoaderKind, mc: &str, version: &str, name: &str) -> Result<serde_json::Value> {
    let url = format!("{}/versions/loader/{mc}/{version}/profile/json", meta_base(kind));
    let mut value: serde_json::Value = http
        .get_json(&url, &format!("{} 版本信息", kind.display_name()))
        .await?;
    if value.get("inheritsFrom").and_then(|v| v.as_str()) != Some(mc) {
        return Err(CoreError::InvalidData(format!(
            "{} {version} 的版本信息与 Minecraft {mc} 不匹配",
            kind.display_name()
        )));
    }
    value["id"] = serde_json::Value::String(name.to_string());
    Ok(value)
}
