//! OptiFine：版本列表与下载来自 BMCLAPI（OptiFine 官网没有可直接下载的地址）。
//!
//! - 原版 + OptiFine：与 HMCL 相同的做法。用 OptiFine 自带的 `optifine.Patcher` 根据原版 jar 生成补丁库，
//!   从安装器中取出 `launchwrapper-of`，再写一个继承原版、通过 LaunchWrapper 启动的版本 JSON。
//! - Forge + OptiFine：OptiFine 本身就是 Forge Mod，直接放进 mods 文件夹（见 [`download`]）。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::download::{CancelToken, DownloadTask, HttpClient, BMCLAPI};
use crate::error::{CoreError, IoContext, Result};
use crate::install::{Installer, Reporter, Verify};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OptiFineVersion {
    pub mc_version: String,
    /// 例如 `HD_U`；预览版为 `HD_U_I6`
    #[serde(rename = "type")]
    pub kind: String,
    /// 例如 `I6`；预览版为 `pre1`
    pub patch: String,
    pub filename: String,
    /// 推荐搭配的 Forge 版本说明，`None` 表示不兼容 Forge
    pub forge: Option<String>,
    pub preview: bool,
}

impl OptiFineVersion {
    /// 显示名称，例如 `HD U I6`、`HD U I6 pre1`
    pub fn display(&self) -> String {
        format!("{} {}", self.kind, self.patch).replace('_', " ")
    }

    /// 库坐标中的版本部分，例如 `1.20.1_HD_U_I6`
    pub fn lib_version(&self) -> String {
        format!("{}_{}_{}", self.mc_version, self.kind, self.patch)
    }

    fn download_url(&self) -> String {
        format!("{BMCLAPI}/optifine/{}/{}/{}", self.mc_version, self.kind, self.patch)
    }
}

#[derive(Deserialize)]
struct RawVersion {
    mcversion: String,
    #[serde(rename = "type")]
    kind: String,
    patch: String,
    filename: String,
    #[serde(default)]
    forge: Option<String>,
}

/// 列出某个游戏版本可用的 OptiFine，正式版在前、新版本在前。
pub async fn list(http: &HttpClient, mc: &str) -> Result<Vec<OptiFineVersion>> {
    let raw: Vec<RawVersion> = http
        .get_json(&format!("{BMCLAPI}/optifine/{mc}"), "OptiFine 版本列表")
        .await?;
    let mut out: Vec<OptiFineVersion> = raw
        .into_iter()
        .filter(|r| r.mcversion == mc)
        .map(|r| OptiFineVersion {
            preview: r.filename.starts_with("preview"),
            forge: r.forge.filter(|f| !f.contains("N/A")),
            mc_version: r.mcversion,
            kind: r.kind,
            patch: r.patch,
            filename: r.filename,
        })
        .collect();
    out.sort_by(|a, b| {
        a.preview
            .cmp(&b.preview)
            .then_with(|| sort_key(b).cmp(&sort_key(a)))
    });
    Ok(out)
}

/// 排序用：`HD_U` + `I6` → `I6`，预览版 `HD_U_I6` + `pre3` → `I6pre03`（同一版本的预览排在正式版之后）。
fn sort_key(v: &OptiFineVersion) -> String {
    if v.preview {
        let base = v.kind.rsplit('_').next().unwrap_or_default();
        let n: u32 = v.patch.trim_start_matches("pre").parse().unwrap_or(0);
        format!("{base}pre{n:02}")
    } else {
        v.patch.clone()
    }
}

/// 下载 OptiFine jar 到指定位置（Forge 实例放到 mods 文件夹）。
pub async fn download(installer: &Installer, v: &OptiFineVersion, dest: &Path, reporter: &Reporter, cancel: &CancelToken) -> Result<()> {
    reporter.stage("下载 OptiFine");
    installer
        .downloader
        .download_all(vec![DownloadTask::new(v.download_url(), dest)], reporter.progress.clone(), cancel)
        .await
}

/// 原版 + OptiFine：生成补丁库与版本 JSON。`mc` 是原版版本（需已安装），`name` 是新实例名。
pub async fn install_vanilla(
    installer: &Installer,
    v: &OptiFineVersion,
    mc: &str,
    name: &str,
    java: &Path,
    reporter: &Reporter,
    cancel: &CancelToken,
) -> Result<()> {
    let game = &installer.game;
    installer.complete(mc, Verify::Quick, reporter, cancel).await?;
    let tmp = game.root().join(".redcraft-temp").join(&v.filename);
    download(installer, v, &tmp, reporter, cancel).await?;
    let result = build_version(installer, v, mc, name, java, &tmp, reporter, cancel).await;
    let _ = std::fs::remove_file(&tmp);
    result?;
    cancel.check()?;
    installer.complete(name, Verify::Full, reporter, cancel).await
}

#[allow(clippy::too_many_arguments)]
async fn build_version(
    installer: &Installer,
    v: &OptiFineVersion,
    mc: &str,
    name: &str,
    java: &Path,
    jar: &Path,
    reporter: &Reporter,
    cancel: &CancelToken,
) -> Result<()> {
    let game = &installer.game;
    let lib_ver = v.lib_version();
    let of_rel = format!("optifine/OptiFine/{lib_ver}/OptiFine-{lib_ver}.jar");
    let of_path = game.library(&of_rel);
    if let Some(p) = of_path.parent() {
        std::fs::create_dir_all(p).at(p)?;
    }

    let (has_patcher, wrapper) = inspect(jar)?;
    if has_patcher {
        reporter.stage("生成 OptiFine 补丁");
        patch(java, jar, &game.version_jar(mc), &of_path, cancel).await?;
    } else {
        std::fs::copy(jar, &of_path).at(&of_path)?;
    }

    // LaunchWrapper：新版 OptiFine 自带 launchwrapper-of，旧版使用 Mojang 的 launchwrapper 1.12
    let wrapper_lib = match wrapper {
        Some((ver, bytes)) => {
            let rel = format!("optifine/launchwrapper-of/{ver}/launchwrapper-of-{ver}.jar");
            let path = game.library(&rel);
            if let Some(p) = path.parent() {
                std::fs::create_dir_all(p).at(p)?;
            }
            std::fs::write(&path, bytes).at(&path)?;
            generated_lib(&format!("optifine:launchwrapper-of:{ver}"), &rel)
        }
        None => json!({ "name": "net.minecraft:launchwrapper:1.12" }),
    };

    let parent: Value = serde_json::from_str(&std::fs::read_to_string(game.version_json(mc)).at(&game.version_json(mc))?)
        .map_err(|e| CoreError::json("原版版本 JSON", e))?;
    let mut version = json!({
        "id": name,
        "inheritsFrom": mc,
        "type": parent.get("type").cloned().unwrap_or(json!("release")),
        "time": parent.get("time").cloned().unwrap_or(json!("")),
        "releaseTime": parent.get("releaseTime").cloned().unwrap_or(json!("")),
        "mainClass": "net.minecraft.launchwrapper.Launch",
        "libraries": [generated_lib(&format!("optifine:OptiFine:{lib_ver}"), &of_rel), wrapper_lib],
    });
    // 1.13 之前用 minecraftArguments（子版本会整体覆盖父版本，需要带上原有参数）
    match parent.get("minecraftArguments").and_then(Value::as_str) {
        Some(args) if parent.get("arguments").is_none() => {
            version["minecraftArguments"] = json!(format!("{args} --tweakClass optifine.OptiFineTweaker"));
        }
        _ => {
            version["arguments"] = json!({ "game": ["--tweakClass", "optifine.OptiFineTweaker"] });
        }
    }
    let path = game.version_json(name);
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).at(p)?;
    }
    let data = serde_json::to_vec_pretty(&version).map_err(|e| CoreError::json("版本 JSON", e))?;
    std::fs::write(&path, data).at(&path)?;
    Ok(())
}

/// 由启动器生成、无法下载的库（下载地址留空）。
fn generated_lib(name: &str, rel: &str) -> Value {
    json!({ "name": name, "downloads": { "artifact": { "path": rel, "url": "" } } })
}

/// 安装器内置的 launchwrapper-of：版本与 jar 内容。
type EmbeddedWrapper = (String, Vec<u8>);

/// 检查安装器：是否带有 Patcher，以及内置的 launchwrapper-of。
fn inspect(jar: &Path) -> Result<(bool, Option<EmbeddedWrapper>)> {
    let file = std::fs::File::open(jar).at(jar)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| CoreError::InvalidData(format!("OptiFine 文件损坏：{e}")))?;
    let has_patcher = zip.by_name("optifine/Patcher.class").is_ok();
    let mut ver = String::new();
    if let Ok(mut f) = zip.by_name("launchwrapper-of.txt") {
        f.read_to_string(&mut ver).at(jar)?;
    }
    let ver = ver.trim().to_string();
    if ver.is_empty() {
        return Ok((has_patcher, None));
    }
    let mut bytes = Vec::new();
    zip.by_name(&format!("launchwrapper-of-{ver}.jar"))
        .map_err(|_| CoreError::InvalidData(format!("OptiFine 安装器中缺少 launchwrapper-of-{ver}.jar")))?
        .read_to_end(&mut bytes)
        .at(jar)?;
    Ok((has_patcher, Some((ver, bytes))))
}

/// 运行 `java -cp OptiFine.jar optifine.Patcher <原版 jar> <OptiFine.jar> <输出>`。
async fn patch(java: &Path, installer: &Path, vanilla: &Path, out: &PathBuf, cancel: &CancelToken) -> Result<()> {
    let mut cmd = tokio::process::Command::new(java);
    cmd.arg("-cp")
        .arg(installer)
        .arg("optifine.Patcher")
        .arg(vanilla)
        .arg(installer)
        .arg(out)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let child = cmd
        .spawn()
        .map_err(|e| CoreError::Java(format!("无法运行 {}：{e}", java.display())))?;
    // 等待结束，期间响应取消（丢弃 future 时 kill_on_drop 会结束进程）
    let wait = child.wait_with_output();
    tokio::pin!(wait);
    let output = loop {
        tokio::select! {
            o = &mut wait => break o.map_err(|e| CoreError::Java(format!("OptiFine 补丁程序异常：{e}")))?,
            _ = tokio::time::sleep(std::time::Duration::from_millis(200)) => cancel.check()?,
        }
    };
    if !output.status.success() || !out.is_file() {
        let tail: String = String::from_utf8_lossy(&output.stderr)
            .lines()
            .chain(String::from_utf8_lossy(&output.stdout).lines())
            .filter(|l| !l.trim().is_empty())
            .take(8)
            .collect::<Vec<_>>()
            .join("\n");
        let _ = std::fs::remove_file(out);
        return Err(CoreError::InvalidData(format!("OptiFine 补丁生成失败：\n{tail}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(kind: &str, patch: &str, preview: bool) -> OptiFineVersion {
        OptiFineVersion {
            mc_version: "1.20.1".into(),
            kind: kind.into(),
            patch: patch.into(),
            filename: String::new(),
            forge: None,
            preview,
        }
    }

    #[test]
    fn names_and_order() {
        let a = v("HD_U", "I6", false);
        assert_eq!(a.display(), "HD U I6");
        assert_eq!(a.lib_version(), "1.20.1_HD_U_I6");
        assert_eq!(a.download_url(), format!("{BMCLAPI}/optifine/1.20.1/HD_U/I6"));
        let p = v("HD_U_I6", "pre1", true);
        assert_eq!(p.display(), "HD U I6 pre1");
        assert_eq!(p.lib_version(), "1.20.1_HD_U_I6_pre1");
        assert!(sort_key(&v("HD_U_I6", "pre10", true)) > sort_key(&v("HD_U_I6", "pre9", true)));
        assert!(sort_key(&a) > sort_key(&v("HD_U", "I5", false)));
    }

    #[tokio::test]
    #[ignore]
    async fn list_real() {
        let http = HttpClient::new(crate::download::DownloadSource::Auto).unwrap();
        let l = list(&http, "1.20.1").await.unwrap();
        assert_eq!(l[0].patch, "I6");
        assert!(!l[0].preview);
        assert!(l.iter().any(|x| x.preview));
    }
}
