//! 启动器自身的更新：查询 GitHub Releases 的最新正式版，选择适合当前安装方式的文件，
//! 下载后用 GitHub 给出的 SHA256（或 Release 附带的 SHA256SUMS.txt）校验。
//!
//! 国内直连 GitHub 下载往往很慢：先同时向 GitHub 与几个加速线路测速，再用最快的线路分块并行下载，
//! 某一块失败或太慢时换下一条线路重试。无论从哪条线路下载，都必须通过 SHA256 校验才会安装。

use std::io::{Read, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

use crate::download::{CancelToken, Progress, ProgressFn};

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
    /// GitHub 计算的校验值，例如 `sha256:af3a…`（2025 年之后上传的文件都有）
    #[serde(default)]
    pub digest: Option<String>,
}

impl ReleaseAsset {
    /// GitHub 给出的 SHA256（小写）。
    pub fn sha256(&self) -> Option<String> {
        let hex = self.digest.as_deref()?.strip_prefix("sha256:")?;
        (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then(|| hex.to_ascii_lowercase())
    }
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

// ─── 加速下载 ───

/// GitHub 加速线路：在原地址前加上前缀。下载后都会校验 SHA256，线路无法篡改文件。
pub const GITHUB_PROXIES: &[&str] = &["https://ghfast.top/", "https://gh-proxy.com/", "https://ghproxy.net/"];

/// GitHub 直连与各加速线路的地址（直连在前）。
pub fn source_urls(url: &str) -> Vec<String> {
    std::iter::once(url.to_string())
        .chain(GITHUB_PROXIES.iter().map(|p| format!("{p}{url}")))
        .collect()
}

/// 测速时下载的大小。
const PROBE_BYTES: u64 = 256 * 1024;
/// 测速的最长时间。
const PROBE_TIMEOUT: Duration = Duration::from_secs(6);
/// 分块大小。
const CHUNK: u64 = 1024 * 1024;
/// 同时下载的分块数。
const WORKERS: usize = 6;
/// 单个分块的最长下载时间，超时视为线路太慢，换下一条线路。
const CHUNK_TIMEOUT: Duration = Duration::from_secs(25);

/// 一条下载线路的测速结果。
#[derive(Debug, Clone)]
pub struct Source {
    pub url: String,
    /// 字节/秒
    pub speed: f64,
    /// 是否支持分块（Range）请求
    pub ranges: bool,
}

/// 请求文件开头的一段，返回速度与是否支持分块；失败或返回的不是这个文件时返回 None。
async fn probe(http: &reqwest::Client, url: &str, size: u64) -> Option<Source> {
    let started = Instant::now();
    let want = PROBE_BYTES.min(size.max(1));
    let run = async {
        let resp = http.get(url).header("Range", format!("bytes=0-{}", want - 1)).send().await.ok()?;
        let status = resp.status().as_u16();
        let ranges = status == 206
            && resp
                .headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| size == 0 || v.ends_with(&format!("/{size}")));
        // 不支持分块时返回整个文件：大小必须一致，避免把代理的错误页面当成文件
        if !ranges && !(status == 200 && (size == 0 || resp.content_length() == Some(size))) {
            return None;
        }
        let mut got = 0u64;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            got += chunk.ok()?.len() as u64;
            if got >= want {
                break;
            }
        }
        (got >= want).then_some(ranges)
    };
    let ranges = tokio::time::timeout(PROBE_TIMEOUT, run).await.ok()??;
    let secs = started.elapsed().as_secs_f64().max(0.001);
    Some(Source { url: url.to_string(), speed: want as f64 / secs, ranges })
}

/// 同时测速所有线路，按速度从快到慢排序；都不可用时返回空列表。
pub async fn rank_sources(http: &reqwest::Client, urls: &[String], size: u64) -> Vec<Source> {
    let probes = urls.iter().map(|u| probe(http, u, size));
    let mut ok: Vec<Source> = futures_util::future::join_all(probes).await.into_iter().flatten().collect();
    ok.sort_by(|a, b| b.speed.partial_cmp(&a.speed).unwrap_or(std::cmp::Ordering::Equal));
    for s in &ok {
        log::info!("更新下载线路 {}：{:.0} KB/s", s.url, s.speed / 1024.0);
    }
    ok
}

struct Shared {
    done: AtomicU64,
    next: AtomicUsize,
}

/// 从 `sources`（已按速度排序）下载 `size` 字节的文件到 `dest`。
/// 最快的线路支持分块时分块并行下载，否则整个文件依次尝试各条线路。
pub async fn download_fast(
    http: &reqwest::Client,
    sources: &[Source],
    dest: &Path,
    size: u64,
    on_progress: ProgressFn,
    cancel: &CancelToken,
) -> Result<()> {
    if sources.is_empty() {
        return Err(CoreError::msg("所有下载线路都无法连接", "请检查网络连接后重试。"));
    }
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await.at(parent)?;
    }
    let shared = Arc::new(Shared { done: AtomicU64::new(0), next: AtomicUsize::new(0) });
    let reporter = {
        let shared = shared.clone();
        let on_progress = on_progress.clone();
        tokio::spawn(async move {
            let (mut last, mut last_at, mut speed) = (0u64, Instant::now(), 0f64);
            let mut ticker = tokio::time::interval(Duration::from_millis(200));
            loop {
                ticker.tick().await;
                let done = shared.done.load(Ordering::Relaxed);
                let dt = last_at.elapsed().as_secs_f64().max(0.001);
                let inst = done.saturating_sub(last) as f64 / dt;
                speed = if speed == 0.0 { inst } else { speed * 0.7 + inst * 0.3 };
                (last, last_at) = (done, Instant::now());
                on_progress(Progress {
                    total_files: 1,
                    done_files: 0,
                    failed_files: 0,
                    total_bytes: size,
                    done_bytes: done,
                    speed: speed as u64,
                });
            }
        })
    };
    let result = if size > CHUNK && sources[0].ranges {
        download_chunks(http, sources, dest, size, &shared, cancel).await
    } else {
        download_whole(http, sources, dest, &shared, cancel).await
    };
    reporter.abort();
    on_progress(Progress {
        total_files: 1,
        done_files: result.is_ok() as usize,
        failed_files: result.is_err() as usize,
        total_bytes: size,
        done_bytes: shared.done.load(Ordering::Relaxed),
        speed: 0,
    });
    if result.is_err() {
        let _ = tokio::fs::remove_file(dest).await;
    }
    result
}

async fn download_whole(
    http: &reqwest::Client,
    sources: &[Source],
    dest: &Path,
    shared: &Shared,
    cancel: &CancelToken,
) -> Result<()> {
    let mut last_err = None;
    for s in sources {
        cancel.check()?;
        let before = shared.done.load(Ordering::Relaxed);
        let r = async {
            let resp = http.get(&s.url).send().await.map_err(|e| CoreError::network(&s.url, e))?;
            if !resp.status().is_success() {
                return Err(CoreError::HttpStatus { url: s.url.clone(), status: resp.status().as_u16() });
            }
            let mut file = tokio::io::BufWriter::with_capacity(256 * 1024, tokio::fs::File::create(dest).await.at(dest)?);
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                cancel.check()?;
                let chunk = chunk.map_err(|e| CoreError::network(&s.url, e))?;
                file.write_all(&chunk).await.at(dest)?;
                shared.done.fetch_add(chunk.len() as u64, Ordering::Relaxed);
            }
            file.flush().await.at(dest)
        }
        .await;
        match r {
            Ok(()) => return Ok(()),
            Err(CoreError::Cancelled) => return Err(CoreError::Cancelled),
            Err(e) => {
                log::warn!("从 {} 下载更新失败：{e}", s.url);
                shared.done.store(before, Ordering::Relaxed);
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or(CoreError::Cancelled))
}

async fn download_chunks(
    http: &reqwest::Client,
    sources: &[Source],
    dest: &Path,
    size: u64,
    shared: &Arc<Shared>,
    cancel: &CancelToken,
) -> Result<()> {
    {
        let f = tokio::fs::File::create(dest).await.at(dest)?;
        f.set_len(size).await.at(dest)?;
    }
    // 只用支持分块的线路
    let sources: Arc<Vec<Source>> = Arc::new(sources.iter().filter(|s| s.ranges).cloned().collect());
    let chunks = size.div_ceil(CHUNK) as usize;
    // 速度不低于最快线路一半的几条线路一起用：加速线路常对单条连接限速，分摊到多条线路速度可以叠加
    let fast = sources.iter().filter(|s| s.speed >= sources[0].speed * 0.5).count().max(1);
    let mut set = tokio::task::JoinSet::new();
    for w in 0..WORKERS.min(chunks) {
        // 每个下载连接先用分到的线路，失败时再按速度顺序尝试其他线路
        let mut order: Vec<Source> = sources.to_vec();
        order.swap(0, w % fast);
        let sources = Arc::new(order);
        let (http, shared, cancel) = (http.clone(), shared.clone(), cancel.clone());
        let dest = dest.to_path_buf();
        set.spawn(async move {
            let mut file = tokio::fs::OpenOptions::new().write(true).open(&dest).await.at(&dest)?;
            loop {
                let i = shared.next.fetch_add(1, Ordering::Relaxed);
                if i >= chunks {
                    return Ok::<(), CoreError>(());
                }
                let start = i as u64 * CHUNK;
                let end = (start + CHUNK).min(size) - 1;
                fetch_chunk(&http, &sources, &mut file, &dest, start, end, &shared, &cancel).await?;
            }
        });
    }
    while let Some(r) = set.join_next().await {
        let r = r.unwrap_or_else(|e| Err(CoreError::msg(format!("下载任务异常退出：{e}"), "")));
        if let Err(e) = r {
            set.abort_all();
            return Err(e);
        }
    }
    Ok(())
}

/// 下载一个分块：按速度顺序尝试各条线路，每条线路最多两次。
#[allow(clippy::too_many_arguments)]
async fn fetch_chunk(
    http: &reqwest::Client,
    sources: &[Source],
    file: &mut tokio::fs::File,
    dest: &Path,
    start: u64,
    end: u64,
    shared: &Shared,
    cancel: &CancelToken,
) -> Result<()> {
    let mut last_err = None;
    for round in 0..2 {
        for s in sources {
            cancel.check()?;
            let mut got = 0u64;
            let run = async {
                let resp = http
                    .get(&s.url)
                    .header("Range", format!("bytes={start}-{end}"))
                    .send()
                    .await
                    .map_err(|e| CoreError::network(&s.url, e))?;
                if resp.status().as_u16() != 206 {
                    return Err(CoreError::HttpStatus { url: s.url.clone(), status: resp.status().as_u16() });
                }
                let mut buf = Vec::with_capacity((end - start + 1) as usize);
                let mut stream = resp.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    cancel.check()?;
                    let chunk = chunk.map_err(|e| CoreError::network(&s.url, e))?;
                    buf.extend_from_slice(&chunk);
                    got += chunk.len() as u64;
                    shared.done.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                }
                if buf.len() as u64 != end - start + 1 {
                    return Err(CoreError::msg(format!("分块大小不符（{}）", s.url), ""));
                }
                Ok(buf)
            };
            // 第一轮限时，太慢就换线路；第二轮不限时，网络整体很慢时也能下完
            let r = if round == 0 {
                match tokio::time::timeout(CHUNK_TIMEOUT, run).await {
                    Ok(r) => r,
                    Err(_) => Err(CoreError::msg(format!("线路太慢（{}）", s.url), "")),
                }
            } else {
                run.await
            };
            match r {
                Ok(buf) => {
                    file.seek(SeekFrom::Start(start)).await.at(dest)?;
                    file.write_all(&buf).await.at(dest)?;
                    return Ok(());
                }
                Err(CoreError::Cancelled) => return Err(CoreError::Cancelled),
                Err(e) => {
                    log::debug!("分块 {start}-{end} 下载失败：{e}");
                    shared.done.fetch_sub(got, Ordering::Relaxed);
                    last_err = Some(e);
                }
            }
        }
    }
    Err(last_err.unwrap_or(CoreError::Cancelled))
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
    fn asset_digest() {
        let a: ReleaseAsset = serde_json::from_str(
            r#"{"name":"a.exe","browser_download_url":"u","size":1,"digest":"sha256:AF3A737614940D81395CB8D0B98ADE9ACAD1173FA3850BB9A2D62D4D5E415B32"}"#,
        )
        .unwrap();
        assert_eq!(a.sha256().as_deref(), Some("af3a737614940d81395cb8d0b98ade9acad1173fa3850bb9a2d62d4d5e415b32"));
        let b: ReleaseAsset = serde_json::from_str(r#"{"name":"a.exe","browser_download_url":"u"}"#).unwrap();
        assert_eq!(b.sha256(), None);
        assert_eq!(source_urls("https://github.com/x").len(), 1 + GITHUB_PROXIES.len());
    }

    /// 联网测试：从最新 Release 测速并分块下载 Windows 安装包，校验 SHA256。
    #[tokio::test]
    #[ignore]
    async fn fast_download_latest() {
        let http = reqwest::Client::builder().user_agent("RCL-test").build().unwrap();
        let release = latest_release(&http).await.unwrap();
        let asset = release.asset_for(InstallKind::Nsis).unwrap().clone();
        let sources = rank_sources(&http, &source_urls(&asset.browser_download_url), asset.size).await;
        println!("{sources:#?}");
        assert!(!sources.is_empty());
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join(&asset.name);
        let started = Instant::now();
        let progress: ProgressFn = Arc::new(|_| {});
        download_fast(&http, &sources, &dest, asset.size, progress, &CancelToken::new()).await.unwrap();
        println!("{} 字节，用时 {:?}", asset.size, started.elapsed());
        assert_eq!(sha256_file(&dest).unwrap(), asset.sha256().unwrap());
    }

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
            {"name":"RCL_1.0.1_portable.exe","browser_download_url":"https://e/c","size":3},
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
        let sums = format!("{h}  RCL_1.0.1_portable.exe\n{}  *other.msi\n", "b".repeat(64));
        assert_eq!(sum_for(&sums, "RCL_1.0.1_portable.exe"), Some(h.clone()));
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
