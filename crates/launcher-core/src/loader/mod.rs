//! 模组加载器：Fabric / Quilt / Forge / NeoForge。
//!
//! 加载器的版本 JSON 通过 `inheritsFrom` 继承原版，原版以 Minecraft 版本号为 id 安装在旁边。

mod fabric;
mod forge;
pub mod optifine;

use std::path::Path;

use serde::Serialize;

pub use forge::neoforge_mc_version;

use crate::download::{CancelToken, HttpClient};
use crate::error::{CoreError, IoContext, Result};
use crate::install::{validate_instance_name, Installer, Reporter, Verify};
use crate::instance::LoaderKind;
use crate::version::ManifestEntry;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LoaderVersion {
    /// 安装时使用的版本号（Forge 为完整的 maven 版本，如 `1.20.1-47.3.0`）
    pub version: String,
    /// 显示用的版本号
    pub display: String,
    pub stable: bool,
}

/// 列出与 Minecraft 版本兼容的加载器版本，新版本在前。
pub async fn list_versions(http: &HttpClient, kind: LoaderKind, mc: &str) -> Result<Vec<LoaderVersion>> {
    match kind {
        LoaderKind::Fabric | LoaderKind::Quilt => fabric::list(http, kind, mc).await,
        LoaderKind::Forge | LoaderKind::NeoForge => forge::list(http, kind, mc).await,
    }
}

/// 要安装的加载器。
#[derive(Debug, Clone)]
pub struct LoaderChoice {
    pub kind: LoaderKind,
    pub version: String,
}

/// 新建实例：安装原版（作为父版本），再安装加载器，最后补全全部文件。
///
/// `java` 只在 Forge / NeoForge 需要运行安装器时使用。
pub async fn install_instance(
    installer: &Installer,
    entry: &ManifestEntry,
    loader: Option<&LoaderChoice>,
    name: &str,
    java: Option<&Path>,
    reporter: &Reporter,
    cancel: &CancelToken,
) -> Result<()> {
    validate_instance_name(name)?;
    let Some(loader) = loader else {
        return installer.install_vanilla(entry, name, reporter, cancel).await;
    };
    let mc = entry.id.as_str();
    if name == mc {
        return Err(CoreError::InvalidData(format!(
            "安装了加载器的实例不能命名为「{mc}」，这个名称要留给原版"
        )));
    }

    // 原版父版本
    let game = &installer.game;
    if !game.version_json(mc).exists() {
        reporter.stage("下载原版版本信息");
        installer.save_version_json(entry, mc).await?;
    }
    cancel.check()?;

    match loader.kind {
        LoaderKind::Fabric | LoaderKind::Quilt => {
            reporter.stage(&format!("获取 {} 版本信息", loader.kind.display_name()));
            let profile = fabric::profile(installer.downloader.http(), loader.kind, mc, &loader.version, name).await?;
            let path = game.version_json(name);
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await.at(parent)?;
            }
            let data = serde_json::to_vec_pretty(&profile).map_err(|e| CoreError::json("版本 JSON", e))?;
            tokio::fs::write(&path, data).await.at(&path)?;
        }
        LoaderKind::Forge | LoaderKind::NeoForge => {
            // 安装器需要完整的原版
            installer.complete(mc, Verify::Quick, reporter, cancel).await?;
            let java = java.ok_or_else(|| CoreError::Java("安装 Forge / NeoForge 需要 Java".into()))?;
            forge::install(installer, loader, mc, name, java, reporter, cancel).await?;
        }
    }
    cancel.check()?;
    installer.complete(name, Verify::Full, reporter, cancel).await
}
