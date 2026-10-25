//! 启动器自身的更新：查询 GitHub Releases 的最新正式版，选择适合当前安装方式的文件，
//! 下载后用 Release 附带的 SHA256SUMS.txt 校验。

use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{CoreError, IoContext, Result};

pub const REPO: &str = "REDSTONEST-cell/Red_Craft";
/// Release 中的校验文件：每行「sha256  文件名」。
pub const SUMS_FILE: &str = "SHA256SUMS.txt";

/// 当前的安装方式，决定下载哪个文件、怎样安装。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallKind {
    /// Windows NSIS 安装版
    Nsis,
    /// Windows MSI 安装版
    Msi,
    /// Windows 单文件版
    Portable,
    /// Linux AppImage
    AppImage,
    /// 其他（macOS、deb）：只能打开下载页手动安装
    Manual,
}

impl InstallKind {
    /// Release 中对应的文件名后缀。
    fn suffix(self) -> Option<&'static str> {
        match self {
            InstallKind::Nsis => Some("-setup.exe"),
            InstallKind::Msi => Some(".msi"),
            InstallKind::Portable => Some("_portable.exe"),
            InstallKind::AppImage => Some(".AppImage"),
            InstallKind::Manual => None,
        }
    }
}

/// 解析 `v1.2.3` / `1.2.3` / `1.2.3-beta.1`，预发布版本比正式版小。
pub fn parse_version(s: &str) -> Option<(u64, u64, u64, bool)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let (core, pre) = match s.split_once(['-', '+']) {
        Some((c, rest)) => (c, !rest.is_empty() && s.contains('-')),
        None => (s, false),
    };
    let mut it = core.split('.').map(|p| p.parse::<u64>());
    let major = it.next()?.ok()?;
    let minor = it.next().unwrap_or(Ok(0)).ok()?;
    let patch = it.next().unwrap_or(Ok(0)).ok()?;
    Some((major, minor, patch, pre))
}

/// `latest` 是否比 `current` 新。
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some((a, b, c, pa)), Some((x, y, z, px))) => {
            // 正式版（!pre）大于同号的预发布版
            (a, b, c, !pa) > (x, y, z, !px)
        }
        _ => false,
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    pub html_url: String,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

impl Release {
    /// 当前安装方式对应的文件。Windows 只选 x64 / 当前架构的文件。
    pub fn asset_for(&self, kind: InstallKind) -> Option<&ReleaseAsset> {
        let suffix = kind.suffix()?;
        self.assets.iter().find(|a| a.name.ends_with(suffix))
    }

    pub fn sums_asset(&self) -> Option<&ReleaseAsset> {
        self.assets.iter().find(|a| a.name == SUMS_FILE)
    }
}

/// 查询最新正式版（GitHub 的 `releases/latest` 不包含预发布版）。
pub async fn latest_release(http: &reqwest::Client) -> Result<Release> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let resp = http
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| CoreError::network(&url, e))?;
    if !resp.status().is_success() {
        return Err(CoreError::HttpStatus {
            url,
            status: resp.status().as_u16(),
        });
    }
    let release: Release = resp.json().await.map_err(|e| CoreError::network(&url, e))?;
    Ok(release)
}

/// 解析 SHA256SUMS.txt，返回文件名对应的哈希（小写）。
/// GitHub 上传时会把文件名中的空格换成点，比较时两边都这样处理。
pub fn sum_for(sums: &str, file: &str) -> Option<String> {
    let norm = |s: &str| s.trim().trim_start_matches('*').replace(' ', ".");
    let want = norm(file);
    sums.lines().find_map(|line| {
        let (hash, name) = line.trim().split_once(char::is_whitespace)?;
        (hash.len() == 64 && norm(name) == want).then(|| hash.to_ascii_lowercase())
    })
}

/// 文件的 SHA256（阻塞调用）。
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path).at(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).at(path)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("v1.0.1", "1.0.0"));
        assert!(is_newer("v1.1.0", "1.0.9"));
        assert!(is_newer("2.0.0", "1.99.99"));
        assert!(!is_newer("v1.0.0", "1.0.0"));
        assert!(!is_newer("v0.2.4", "1.0.0"));
        assert!(is_newer("1.0.0", "1.0.0-beta.2"));
        assert!(!is_newer("1.0.0-beta.2", "1.0.0"));
        assert!(!is_newer("garbage", "1.0.0"));
        assert_eq!(parse_version("v1.2"), Some((1, 2, 0, false)));
    }

    #[test]
    fn assets_and_sums() {
        let json = r#"{"tag_name":"v1.0.1","html_url":"https://github.com/x/y/releases/tag/v1.0.1","assets":[
            {"name":"Red.Craft_1.0.1_x64-setup.exe","browser_download_url":"https://e/a","size":1},
            {"name":"Red.Craft_1.0.1_x64_zh-CN.msi","browser_download_url":"https://e/b","size":2},
            {"name":"RedCraft_1.0.1_portable.exe","browser_download_url":"https://e/c","size":3},
            {"name":"Red.Craft_1.0.1_amd64.AppImage","browser_download_url":"https://e/d","size":4},
            {"name":"SHA256SUMS.txt","browser_download_url":"https://e/s","size":5}]}"#;
        let r: Release = serde_json::from_str(json).unwrap();
        assert_eq!(r.asset_for(InstallKind::Nsis).unwrap().browser_download_url, "https://e/a");
        assert_eq!(r.asset_for(InstallKind::Msi).unwrap().browser_download_url, "https://e/b");
        assert_eq!(r.asset_for(InstallKind::Portable).unwrap().browser_download_url, "https://e/c");
        assert_eq!(r.asset_for(InstallKind::AppImage).unwrap().browser_download_url, "https://e/d");
        assert!(r.asset_for(InstallKind::Manual).is_none());
        assert!(r.sums_asset().is_some());

        let h = "a".repeat(64);
        let sums = format!("{h}  RedCraft_1.0.1_portable.exe\n{}  *other.msi\n", "b".repeat(64));
        assert_eq!(sum_for(&sums, "RedCraft_1.0.1_portable.exe"), Some(h.clone()));
        assert_eq!(sum_for(&sums, "other.msi"), Some("b".repeat(64)));
        assert_eq!(sum_for(&sums, "missing"), None);
        // 文件名带空格（1.0.0 的 SHA256SUMS.txt）与 GitHub 改名后的资产名
        let spaced = format!("{h}  Red Craft_1.0.0_x64-setup.exe\n");
        assert_eq!(sum_for(&spaced, "Red.Craft_1.0.0_x64-setup.exe"), Some(h.clone()));
        let dotted = format!("{h}  Red.Craft_1.0.0_x64-setup.exe\n");
        assert_eq!(sum_for(&dotted, "Red.Craft_1.0.0_x64-setup.exe"), Some(h));
    }

    #[test]
    fn sha256() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(
            sha256_file(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
