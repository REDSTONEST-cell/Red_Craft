//! 并发下载器：Semaphore 控制并发、SHA1 校验、失败重试与下载源切换。

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::Serialize;
use sha1::{Digest, Sha1};
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use super::http::HttpClient;
use crate::error::{CoreError, IoContext, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadTask {
    pub url: String,
    pub path: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

impl DownloadTask {
    pub fn new(url: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        DownloadTask {
            url: url.into(),
            path: path.into(),
            sha1: None,
            size: None,
        }
    }

    pub fn sha1(mut self, sha1: Option<impl Into<String>>) -> Self {
        self.sha1 = sha1.map(Into::into).filter(|s: &String| !s.is_empty());
        self
    }

    pub fn size(mut self, size: Option<u64>) -> Self {
        self.size = size.filter(|s| *s > 0);
        self
    }
}

/// 取消标记，可在多个任务间共享。
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(CoreError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub total_files: usize,
    pub done_files: usize,
    pub failed_files: usize,
    pub total_bytes: u64,
    pub done_bytes: u64,
    /// 字节/秒
    pub speed: u64,
}

#[derive(Default)]
struct Counters {
    done_files: AtomicUsize,
    failed_files: AtomicUsize,
    total_bytes: AtomicU64,
    done_bytes: AtomicU64,
    /// 实际从网络读取的字节数，用于计算速度
    net_bytes: AtomicU64,
}

impl Counters {
    fn snapshot(&self, total_files: usize, speed: u64) -> Progress {
        Progress {
            total_files,
            done_files: self.done_files.load(Ordering::Relaxed),
            failed_files: self.failed_files.load(Ordering::Relaxed),
            total_bytes: self.total_bytes.load(Ordering::Relaxed),
            done_bytes: self.done_bytes.load(Ordering::Relaxed),
            speed,
        }
    }
}

pub type ProgressFn = Arc<dyn Fn(Progress) + Send + Sync>;

#[derive(Clone)]
pub struct Downloader {
    http: HttpClient,
    concurrency: usize,
    retries: u32,
}

impl Downloader {
    pub fn new(http: HttpClient, concurrency: usize) -> Self {
        Downloader {
            http,
            concurrency: concurrency.clamp(1, 256),
            retries: 3,
        }
    }

    pub fn http(&self) -> &HttpClient {
        &self.http
    }

    /// 下载全部任务。已存在且校验通过的文件会被跳过。
    pub async fn download_all(
        &self,
        tasks: Vec<DownloadTask>,
        on_progress: ProgressFn,
        cancel: &CancelToken,
    ) -> Result<()> {
        // 同一路径只下载一次
        let mut seen = HashSet::new();
        let tasks: Vec<_> = tasks
            .into_iter()
            .filter(|t| seen.insert(t.path.clone()))
            .collect();
        let total_files = tasks.len();
        let counters = Arc::new(Counters::default());
        counters.total_bytes.store(
            tasks.iter().filter_map(|t| t.size).sum(),
            Ordering::Relaxed,
        );

        // 进度上报：每 200ms 一次，速度做指数平滑
        let reporter = {
            let counters = counters.clone();
            let on_progress = on_progress.clone();
            tokio::spawn(async move {
                let mut last_bytes = 0u64;
                let mut last_at = Instant::now();
                let mut speed = 0f64;
                let mut ticker = tokio::time::interval(Duration::from_millis(200));
                loop {
                    ticker.tick().await;
                    let now = Instant::now();
                    let bytes = counters.net_bytes.load(Ordering::Relaxed);
                    let dt = now.duration_since(last_at).as_secs_f64().max(0.001);
                    let inst = bytes.saturating_sub(last_bytes) as f64 / dt;
                    speed = if speed == 0.0 { inst } else { speed * 0.7 + inst * 0.3 };
                    last_bytes = bytes;
                    last_at = now;
                    on_progress(counters.snapshot(total_files, speed as u64));
                }
            })
        };

        let semaphore = Arc::new(Semaphore::new(self.concurrency));
        let mut set = JoinSet::new();
        for task in tasks {
            let sem = semaphore.clone();
            let this = self.clone();
            let counters = counters.clone();
            let cancel = cancel.clone();
            set.spawn(async move {
                let _permit = sem.acquire_owned().await.expect("semaphore closed");
                cancel.check()?;
                let r = this.download_one(&task, &counters, &cancel).await;
                match &r {
                    Ok(()) => counters.done_files.fetch_add(1, Ordering::Relaxed),
                    Err(_) => counters.failed_files.fetch_add(1, Ordering::Relaxed),
                };
                r
            });
        }

        let mut first_err: Option<CoreError> = None;
        let mut failed = 0;
        while let Some(joined) = set.join_next().await {
            let r = joined.unwrap_or_else(|e| Err(CoreError::InvalidData(format!("下载任务异常退出：{e}"))));
            if let Err(e) = r {
                if matches!(e, CoreError::Cancelled) {
                    set.abort_all();
                    reporter.abort();
                    return Err(CoreError::Cancelled);
                }
                log::warn!("下载失败: {e}");
                failed += 1;
                first_err.get_or_insert(e);
            }
        }
        reporter.abort();
        on_progress(counters.snapshot(total_files, 0));

        match first_err {
            None => Ok(()),
            Some(e) if total_files == 1 => Err(e),
            Some(e) => Err(CoreError::DownloadFailed {
                failed,
                total: total_files,
                first: Box::new(e),
            }),
        }
    }

    async fn download_one(
        &self,
        task: &DownloadTask,
        counters: &Counters,
        cancel: &CancelToken,
    ) -> Result<()> {
        if is_valid(&task.path, task.sha1.as_deref(), task.size).await? {
            if let Some(size) = task.size {
                counters.done_bytes.fetch_add(size, Ordering::Relaxed);
            }
            return Ok(());
        }
        if let Some(parent) = task.path.parent() {
            tokio::fs::create_dir_all(parent).await.at(parent)?;
        }

        let candidates = self.http.candidates(&task.url);
        let mut last_err = None;
        'attempts: for attempt in 0..self.retries {
            for (i, url) in candidates.iter().enumerate() {
                cancel.check()?;
                let mut written = 0u64;
                // 官方优先模式下，第一个（官方）地址缓慢时可以换用后面的镜像
                let can_switch = i == 0 && candidates.len() > 1 && self.http.checks_slowness();
                match self
                    .fetch_to(url, task, counters, cancel, &mut written, can_switch)
                    .await
                {
                    Ok(()) => return Ok(()),
                    Err(CoreError::Cancelled) => return Err(CoreError::Cancelled),
                    Err(e) => {
                        // 回滚本次尝试计入的进度
                        counters.done_bytes.fetch_sub(written, Ordering::Relaxed);
                        log::debug!("下载 {url} 第 {} 次失败: {e}", attempt + 1);
                        // 404 在其它镜像上可能存在，但重试同一地址没有意义
                        let fatal = matches!(e, CoreError::HttpStatus { status: 404, .. });
                        last_err = Some(e);
                        if fatal && candidates.len() == 1 {
                            break 'attempts;
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(500 * 2u64.pow(attempt))).await;
        }
        Err(last_err.unwrap_or(CoreError::Cancelled))
    }

    async fn fetch_to(
        &self,
        url: &str,
        task: &DownloadTask,
        counters: &Counters,
        cancel: &CancelToken,
        written: &mut u64,
        can_switch: bool,
    ) -> Result<()> {
        let slow = || {
            self.http.mark_official_slow();
            CoreError::InvalidData(format!("{url} 下载缓慢，换用镜像源"))
        };
        let send = self.http.raw().get(url).send();
        let resp = if can_switch {
            match tokio::time::timeout(SLOW_RESPONSE, send).await {
                Ok(r) => r,
                Err(_) => return Err(slow()),
            }
        } else {
            send.await
        }
        .map_err(|e| CoreError::network(url, e))?;
        if !resp.status().is_success() {
            return Err(CoreError::HttpStatus {
                url: url.to_string(),
                status: resp.status().as_u16(),
            });
        }
        if task.size.is_none() {
            if let Some(len) = resp.content_length() {
                counters.total_bytes.fetch_add(len, Ordering::Relaxed);
            }
        }

        let expected_len = task.size.or(resp.content_length()).unwrap_or(0);
        let started = Instant::now();
        let part = part_path(&task.path);
        let mut file = tokio::fs::File::create(&part).await.at(&part)?;
        let mut hasher = Sha1::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            if cancel.is_cancelled() {
                drop(file);
                let _ = tokio::fs::remove_file(&part).await;
                return Err(CoreError::Cancelled);
            }
            let chunk = chunk.map_err(|e| CoreError::network(url, e))?;
            self.http.limiter().take(chunk.len() as u64).await;
            hasher.update(&chunk);
            file.write_all(&chunk).await.at(&part)?;
            let n = chunk.len() as u64;
            *written += n;
            counters.done_bytes.fetch_add(n, Ordering::Relaxed);
            counters.net_bytes.fetch_add(n, Ordering::Relaxed);
            // 大文件持续低速：放弃官方源，换用镜像（限速时不判断）
            if can_switch && self.http.limiter().rate() == 0 {
                let secs = started.elapsed().as_secs_f64();
                let remaining = expected_len.saturating_sub(*written);
                if secs > SLOW_CHECK_AFTER
                    && remaining > SLOW_MIN_REMAINING
                    && (*written as f64 / secs) < SLOW_SPEED
                {
                    drop(file);
                    let _ = tokio::fs::remove_file(&part).await;
                    return Err(slow());
                }
            }
        }
        file.flush().await.at(&part)?;
        drop(file);

        let actual = hex::encode(hasher.finalize());
        if let Some(expected) = &task.sha1 {
            if !expected.eq_ignore_ascii_case(&actual) {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(CoreError::Checksum {
                    path: task.path.clone(),
                    expected: expected.clone(),
                    actual,
                });
            }
        }
        tokio::fs::rename(&part, &task.path).await.at(&task.path)?;
        Ok(())
    }
}

/// 官方源响应超过这个时间视为缓慢。
const SLOW_RESPONSE: Duration = Duration::from_secs(6);
/// 开始下载多久后检查速度（秒）。
const SLOW_CHECK_AFTER: f64 = 4.0;
/// 低于这个速度（字节/秒）视为缓慢。
const SLOW_SPEED: f64 = 64.0 * 1024.0;
/// 剩余不足这个大小时不再切换，直接下完。
const SLOW_MIN_REMAINING: u64 = 512 * 1024;

fn part_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    path.with_file_name(name)
}

/// 文件是否存在且（如提供）大小与 SHA1 都匹配。
pub async fn is_valid(path: &Path, sha1: Option<&str>, size: Option<u64>) -> Result<bool> {
    let meta = match tokio::fs::metadata(path).await {
        Ok(m) if m.is_file() => m,
        _ => return Ok(false),
    };
    if let Some(size) = size {
        if meta.len() != size {
            return Ok(false);
        }
    }
    match sha1 {
        None => Ok(true),
        Some(expected) => {
            let path = path.to_path_buf();
            let actual = tokio::task::spawn_blocking(move || sha1_file(&path))
                .await
                .map_err(|e| CoreError::InvalidData(e.to_string()))??;
            Ok(actual.eq_ignore_ascii_case(expected))
        }
    }
}

/// 计算文件 SHA1（阻塞调用）。
pub fn sha1_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path).at(path)?;
    let mut hasher = Sha1::new();
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

pub fn sha1_bytes(data: &[u8]) -> String {
    hex::encode(Sha1::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn validity_check() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        assert!(!is_valid(&p, None, None).await.unwrap());
        std::fs::write(&p, b"hello").unwrap();
        let h = sha1_bytes(b"hello");
        assert_eq!(h, "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");
        assert!(is_valid(&p, Some(&h), Some(5)).await.unwrap());
        assert!(!is_valid(&p, Some(&h), Some(6)).await.unwrap());
        assert!(!is_valid(&p, Some("0000"), None).await.unwrap());
        assert_eq!(sha1_file(&p).unwrap(), h);
    }

    #[test]
    fn part_suffix() {
        assert_eq!(
            part_path(Path::new("/a/b/c.jar")),
            PathBuf::from("/a/b/c.jar.part")
        );
    }
}
