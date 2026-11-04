//! 并发下载器：Semaphore 控制并发、SHA1 校验、失败重试与下载源切换。
//!
//! 大文件参照 PCL 的下载引擎分块多线程下载：1MB 以上的文件动态拆分，新的连接接手剩余最大的一块，
//! 并在各个下载源之间轮换，官方源与镜像可以同时下载；某个连接太慢或出错时，剩余部分换下一个源继续。

use std::collections::{HashMap, HashSet};
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
    total_files: AtomicUsize,
    done_files: AtomicUsize,
    failed_files: AtomicUsize,
    total_bytes: AtomicU64,
    done_bytes: AtomicU64,
    /// 实际从网络读取的字节数，用于计算速度
    net_bytes: AtomicU64,
}

impl Counters {
    /// 还没完成（含正在下载）的文件数
    fn remaining_files(&self) -> usize {
        let finished = self.done_files.load(Ordering::Relaxed) + self.failed_files.load(Ordering::Relaxed);
        self.total_files.load(Ordering::Relaxed).saturating_sub(finished)
    }

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

/// 已校验文件的记录：路径 → (大小, 修改时间, SHA1)。
///
/// 启动前补全文件时，大小与修改时间都没变的文件直接视为校验通过，不再重新计算哈希
/// （依赖库常有几百 MB，每次都计算会明显拖慢启动，Windows 上读取时还会被杀毒软件扫描）。
/// 文件被替换或修改后修改时间会变，仍会重新校验。
#[derive(Default)]
pub struct VerifyCache {
    file: PathBuf,
    map: std::sync::Mutex<HashMap<PathBuf, (u64, u128, String)>>,
    dirty: AtomicBool,
}

impl VerifyCache {
    /// 读取记录文件；不存在或损坏时从空记录开始。
    pub fn load(file: PathBuf) -> Self {
        let mut map = HashMap::new();
        if let Ok(text) = std::fs::read_to_string(&file) {
            for line in text.lines() {
                let mut it = line.splitn(4, '\t');
                if let (Some(sha1), Some(size), Some(mtime), Some(path)) = (it.next(), it.next(), it.next(), it.next()) {
                    if let (Ok(size), Ok(mtime)) = (size.parse(), mtime.parse()) {
                        map.insert(PathBuf::from(path), (size, mtime, sha1.to_string()));
                    }
                }
            }
        }
        VerifyCache { file, map: std::sync::Mutex::new(map), dirty: AtomicBool::new(false) }
    }

    fn stamp(meta: &std::fs::Metadata) -> u128 {
        meta.modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    }

    fn known(&self, path: &Path, meta: &std::fs::Metadata, sha1: &str) -> bool {
        let stamp = Self::stamp(meta);
        self.map
            .lock()
            .is_ok_and(|m| m.get(path).is_some_and(|(size, mtime, h)| *size == meta.len() && *mtime == stamp && stamp != 0 && h.eq_ignore_ascii_case(sha1)))
    }

    fn record(&self, path: &Path, sha1: &str) {
        let Ok(meta) = std::fs::metadata(path) else { return };
        if let Ok(mut m) = self.map.lock() {
            m.insert(path.to_path_buf(), (meta.len(), Self::stamp(&meta), sha1.to_ascii_lowercase()));
            self.dirty.store(true, Ordering::Relaxed);
        }
    }

    /// 有变化时写回记录文件（先写临时文件再替换）。
    pub fn save(&self) {
        if !self.dirty.swap(false, Ordering::Relaxed) {
            return;
        }
        let Ok(m) = self.map.lock() else { return };
        let mut text = String::with_capacity(m.len() * 120);
        for (path, (size, mtime, sha1)) in m.iter() {
            if path.exists() {
                text.push_str(&format!("{sha1}\t{size}\t{mtime}\t{}\n", path.display()));
            }
        }
        drop(m);
        let tmp = self.file.with_extension("tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &self.file);
        }
    }
}

#[derive(Clone)]
pub struct Downloader {
    http: HttpClient,
    concurrency: usize,
    retries: u32,
    /// 分块下载额外占用的连接数（所有文件合计，不超过并发数）
    extra: Arc<AtomicUsize>,
    /// 已校验文件的记录（可选）
    verified: Option<Arc<VerifyCache>>,
}

impl Downloader {
    pub fn new(http: HttpClient, concurrency: usize) -> Self {
        Downloader {
            http,
            concurrency: concurrency.clamp(1, 256),
            retries: 3,
            extra: Arc::new(AtomicUsize::new(0)),
            verified: None,
        }
    }

    /// 使用已校验文件的记录，跳过没有变化的文件的哈希计算。
    pub fn with_verify_cache(mut self, cache: Arc<VerifyCache>) -> Self {
        self.verified = Some(cache);
        self
    }

    /// 文件是否存在且（如提供）大小与 SHA1 都匹配；有记录且没有变化时不重新计算哈希。
    async fn is_valid_cached(&self, task: &DownloadTask) -> Result<bool> {
        if let (Some(cache), Some(sha1)) = (&self.verified, task.sha1.as_deref()) {
            if let Ok(meta) = tokio::fs::metadata(&task.path).await {
                if meta.is_file() && task.size.is_none_or(|s| s == meta.len()) && cache.known(&task.path, &meta, sha1) {
                    return Ok(true);
                }
            }
            let ok = is_valid(&task.path, Some(sha1), task.size).await?;
            if ok {
                cache.record(&task.path, sha1);
            }
            return Ok(ok);
        }
        is_valid(&task.path, task.sha1.as_deref(), task.size).await
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
        counters.total_files.store(total_files, Ordering::Relaxed);
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
                if r.is_ok() {
                    if let (Some(cache), Some(sha1)) = (&this.verified, task.sha1.as_deref()) {
                        cache.record(&task.path, sha1);
                    }
                }
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
        if let Some(cache) = &self.verified {
            let cache = cache.clone();
            let _ = tokio::task::spawn_blocking(move || cache.save()).await;
        }

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
        counters: &Arc<Counters>,
        cancel: &CancelToken,
    ) -> Result<()> {
        if self.is_valid_cached(task).await? {
            if let Some(size) = task.size {
                counters.done_bytes.fetch_add(size, Ordering::Relaxed);
            }
            return Ok(());
        }
        if let Some(parent) = task.path.parent() {
            tokio::fs::create_dir_all(parent).await.at(parent)?;
        }

        let candidates = self.http.candidates(&task.url);
        // 大文件先尝试分块多线程下载；失败时（例如校验不通过）退回逐个源单线程下载
        if task.size.is_some_and(|s| s >= SPLIT_MIN) {
            let mut written = 0u64;
            match self.download_split(task, &candidates, counters, cancel, &mut written).await {
                Ok(()) => return Ok(()),
                Err(CoreError::Cancelled) => return Err(CoreError::Cancelled),
                Err(e) => {
                    counters.done_bytes.fetch_sub(written, Ordering::Relaxed);
                    log::info!("{}：分块下载失败，改为单线程下载：{e}", task.path.display());
                }
            }
        }
        let mut last_err = None;
        'attempts: for attempt in 0..self.retries {
            for (i, url) in candidates.iter().enumerate() {
                cancel.check()?;
                let mut written = 0u64;
                // 官方优先模式下，第一个（官方）地址缓慢时可以换用后面的镜像
                let can_switch = i == 0 && candidates.len() > 1 && self.http.checks_slowness();
                // 最后一次尝试不再因为缓慢而中断，网络本身很慢时至少能下完
                let last_try = attempt + 1 == self.retries && i + 1 == candidates.len();
                match self
                    .fetch_to(url, task, counters, cancel, &mut written, can_switch, !last_try)
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

    #[allow(clippy::too_many_arguments)]
    async fn fetch_to(
        &self,
        url: &str,
        task: &DownloadTask,
        counters: &Counters,
        cancel: &CancelToken,
        written: &mut u64,
        can_switch: bool,
        can_restart: bool,
    ) -> Result<()> {
        let slow = || {
            self.http.mark_official_slow();
            CoreError::InvalidData(format!("{url} 下载缓慢，换用镜像源"))
        };
        let stalled = || CoreError::InvalidData(format!("{url} 下载停滞，重新连接"));
        let send = self.http.raw().get(url).send();
        let resp = if can_switch {
            match tokio::time::timeout(SLOW_RESPONSE, send).await {
                Ok(r) => r,
                Err(_) => return Err(slow()),
            }
        } else {
            match tokio::time::timeout(STALL, send).await {
                Ok(r) => r,
                Err(_) => return Err(stalled()),
            }
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
        // 写入缓冲：tokio 的每次文件写入都要切换到阻塞线程池，攒到 256KB 再写可以大幅减少切换
        let mut file = tokio::io::BufWriter::with_capacity(256 * 1024, tokio::fs::File::create(&part).await.at(&part)?);
        let mut hasher = Sha1::new();
        let mut stream = resp.bytes_stream();
        loop {
            // 连接停滞（服务器不再发送数据）时断开重试，不等网络层 30 秒的超时
            let next = match tokio::time::timeout(STALL, stream.next()).await {
                Ok(next) => next,
                Err(_) => {
                    drop(file);
                    let _ = tokio::fs::remove_file(&part).await;
                    return Err(stalled());
                }
            };
            let Some(chunk) = next else { break };
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
            // 批量下载的最后几个文件：带宽都留给它们了，仍然很慢说明这个连接有问题，断开重连或换源
            if can_restart
                && self.http.limiter().rate() == 0
                && counters.total_files.load(Ordering::Relaxed) > TAIL_FILES
                && tail_too_slow(counters.remaining_files(), started.elapsed(), *written, expected_len)
            {
                drop(file);
                let _ = tokio::fs::remove_file(&part).await;
                return Err(CoreError::InvalidData(format!("{url} 下载缓慢，重新连接")));
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

/// 连接多久没有收到数据视为停滞，断开重试。
const STALL: Duration = Duration::from_secs(10);
/// 剩余文件不多于这个数时进入收尾阶段。
const TAIL_FILES: usize = 8;
/// 收尾阶段：下载超过这个时间仍低于 TAIL_SPEED 的文件断开重连。
const TAIL_CHECK_AFTER: Duration = Duration::from_secs(6);
const TAIL_SPEED: f64 = 32.0 * 1024.0;

/// 收尾阶段的文件是否慢得应该重新连接。
fn tail_too_slow(remaining_files: usize, elapsed: Duration, written: u64, expected: u64) -> bool {
    if remaining_files > TAIL_FILES || elapsed < TAIL_CHECK_AFTER {
        return false;
    }
    if expected > 0 && written >= expected {
        return false;
    }
    (written as f64 / elapsed.as_secs_f64()) < TAIL_SPEED
}

/// 官方源响应超过这个时间视为缓慢（与 PCL 相同为 4 秒）。
const SLOW_RESPONSE: Duration = Duration::from_secs(4);
/// 开始下载多久后检查速度（秒）。
const SLOW_CHECK_AFTER: f64 = 4.0;
/// 低于这个速度（字节/秒）视为缓慢。
const SLOW_SPEED: f64 = 64.0 * 1024.0;
/// 剩余不足这个大小时不再切换，直接下完。
const SLOW_MIN_REMAINING: u64 = 512 * 1024;

// ─── 分块多线程下载（参照 PCL 的下载引擎） ───

/// 不小于这个大小的文件才分块（PCL：小于 1MB 的文件不分割）。
const SPLIT_MIN: u64 = 1024 * 1024;
/// 剩余不足这个大小的块不再拆分。
const PIECE_MIN: u64 = 256 * 1024;
/// 每个文件最多同时使用的连接数。
const MAX_PIECES: usize = 8;
/// 连接多久没有收到数据就断开，剩余部分换下一个源。
const PIECE_STALL: Duration = Duration::from_secs(8);
/// 每攒够这么多数据写入一次磁盘。
const PIECE_FLUSH: usize = 256 * 1024;
/// 与 PCL 一样只在速度不够时增开连接：平均每个连接低于这个速度（字节/秒）才拆分。
/// 网络很快时单个连接就能跑满带宽，拆分反而增加建立连接的开销。
const SPLIT_BELOW_PER_CONN: f64 = 1.5 * 1024.0 * 1024.0;
/// 收到数据后至少测量这么久的实际速度，才决定是否拆分（不把建立连接的时间算进去）。
const SPLIT_AFTER: Duration = Duration::from_secs(1);

/// 不允许多线程下载的源（与 PCL 相同：BMCLAPI 等在高频请求下会限流）。
fn splittable(url: &str) -> bool {
    !["bmclapi", "github.com", "optifine.net"].iter().any(|k| url.contains(k))
}

struct SplitSource {
    url: String,
    fails: u32,
    disabled: bool,
}

struct Piece {
    /// 下一个要写入的位置
    pos: u64,
    /// 结束位置（不含），拆分时会缩小
    end: u64,
    active: bool,
    source: usize,
}

struct SplitState {
    sources: Vec<SplitSource>,
    pieces: Vec<Piece>,
    /// 服务器不支持分块请求（返回了整个文件）
    no_ranges: bool,
    /// 已经有连接收到过数据
    started: bool,
    next_source: usize,
    last_error: Option<CoreError>,
}

impl SplitState {
    /// 从 `from` 开始的第一个可用源；`split_only` 时只选允许多线程的源。
    fn pick(&mut self, from: usize, split_only: bool) -> Option<usize> {
        let n = self.sources.len();
        (0..n)
            .map(|i| (from + i) % n)
            .find(|&i| !self.sources[i].disabled && (!split_only || splittable(&self.sources[i].url)))
    }
}

/// 出错后是否不再使用这个源（PCL：404、416、502、无法解析域名等直接停用；BMCLAPI 的 403/429 只是限流）。
fn fatal_for_source(e: &CoreError, url: &str) -> bool {
    match e {
        CoreError::HttpStatus { status, .. } => {
            matches!(status, 404 | 410 | 416 | 502) || (matches!(status, 403 | 429) && !url.contains("bmclapi"))
        }
        CoreError::Network { reason, .. } => reason.contains("无法连接") || reason.contains("dns"),
        _ => false,
    }
}

impl Downloader {
    async fn download_split(
        &self,
        task: &DownloadTask,
        candidates: &[String],
        counters: &Arc<Counters>,
        cancel: &CancelToken,
        written: &mut u64,
    ) -> Result<()> {
        let size = task.size.unwrap_or(0);
        let part = part_path(&task.path);
        {
            let f = tokio::fs::File::create(&part).await.at(&part)?;
            f.set_len(size).await.at(&part)?;
        }
        let state = Arc::new(std::sync::Mutex::new(SplitState {
            sources: candidates.iter().map(|u| SplitSource { url: u.clone(), fails: 0, disabled: false }).collect(),
            pieces: vec![Piece { pos: 0, end: size, active: false, source: 0 }],
            no_ranges: false,
            started: false,
            next_source: 0,
            last_error: None,
        }));
        let done_total = Arc::new(AtomicU64::new(0));
        let mut workers: JoinSet<()> = JoinSet::new();
        let mut extra_held = 0usize;
        let mut last_split = Instant::now();
        // 实际传输速度：从收到第一批数据开始测量，(开始时间, 当时已下载字节)
        let mut sample: Option<(Instant, u64)> = None;
        let mut speed = f64::MAX;
        let result = loop {
            if cancel.is_cancelled() {
                break Err(CoreError::Cancelled);
            }
            // 启动空闲的块：失败或被断开的块从下一个源继续
            let mut launch = Vec::new();
            {
                let mut st = state.lock().expect("split state");
                if st.pieces.iter().all(|p| p.pos >= p.end) {
                    break Ok(());
                }
                let no_ranges = st.no_ranges;
                for i in 0..st.pieces.len() {
                    let (pos, end, active, prev) = {
                        let p = &st.pieces[i];
                        (p.pos, p.end, p.active, p.source)
                    };
                    if active || pos >= end {
                        continue;
                    }
                    // 不支持分块的服务器只能从头整个下载
                    if no_ranges && (i != 0 || pos != 0) {
                        continue;
                    }
                    let from = if pos == 0 && i == 0 && !st.started { prev } else { prev + 1 };
                    let Some(src) = st.pick(from, false) else { continue };
                    st.pieces[i].active = true;
                    st.pieces[i].source = src;
                    launch.push((i, st.sources[src].url.clone()));
                }
                let active = st.pieces.iter().filter(|p| p.active).count();
                if active == 0 && launch.is_empty() {
                    break Err(st.last_error.take().unwrap_or_else(|| CoreError::InvalidData("所有下载源都不可用".into())));
                }
                // 每测满 1 秒更新一次速度
                let now = done_total.load(Ordering::Relaxed);
                match sample {
                    None if now > 0 => sample = Some((Instant::now(), now)),
                    Some((at, from)) if at.elapsed() >= SPLIT_AFTER => {
                        speed = (now - from) as f64 / at.elapsed().as_secs_f64();
                        sample = Some((Instant::now(), now));
                    }
                    _ => {}
                }
                let per_conn = speed / active.max(1) as f64;
                // 拆分：把剩余最大的块从 60% 处切开，交给下一个可以多线程下载的源
                if st.started
                    && per_conn < SPLIT_BELOW_PER_CONN
                    && !st.no_ranges
                    && st.pieces.len() < MAX_PIECES
                    && last_split.elapsed() >= Duration::from_millis(300)
                    && self.extra.load(Ordering::Relaxed) < self.concurrency
                {
                    let largest = st
                        .pieces
                        .iter()
                        .enumerate()
                        .filter(|(_, p)| p.active)
                        .max_by_key(|(_, p)| p.end - p.pos)
                        .map(|(i, p)| (i, p.end - p.pos));
                    if let Some((i, remaining)) = largest.filter(|(_, r)| *r >= PIECE_MIN * 2) {
                        let next = st.next_source;
                        if let Some(src) = st.pick(next, true) {
                            st.next_source = src + 1;
                            let end = st.pieces[i].end;
                            let start = end - remaining * 2 / 5;
                            st.pieces[i].end = start;
                            st.pieces.push(Piece { pos: start, end, active: true, source: src });
                            launch.push((st.pieces.len() - 1, st.sources[src].url.clone()));
                            self.extra.fetch_add(1, Ordering::Relaxed);
                            extra_held += 1;
                            last_split = Instant::now();
                        }
                    }
                }
            }
            for (index, url) in launch {
                let this = self.clone();
                let state = state.clone();
                let part = part.clone();
                let cancel = cancel.clone();
                let done_total = done_total.clone();
                let counters = counters.clone();
                workers.spawn(async move {
                    let r = this.fetch_piece(&url, &state, index, &part, size, &counters, &done_total, &cancel).await;
                    let mut st = state.lock().expect("split state");
                    st.pieces[index].active = false;
                    if let Err(e) = r {
                        let src = st.pieces[index].source;
                        let fatal = fatal_for_source(&e, &url);
                        let s = &mut st.sources[src];
                        s.fails += 1;
                        if fatal || s.fails > 4 {
                            s.disabled = true;
                            log::debug!("下载源已停用：{url}（{e}）");
                        }
                        if !matches!(e, CoreError::Cancelled) {
                            st.last_error = Some(e);
                        }
                    }
                });
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        // 结束所有连接
        workers.abort_all();
        while workers.join_next().await.is_some() {}
        self.extra.fetch_sub(extra_held, Ordering::Relaxed);
        *written = done_total.load(Ordering::Relaxed);
        if let Err(e) = result {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(e);
        }
        // 分块写入无法边下边算，下载完成后统一校验
        if let Some(expected) = &task.sha1 {
            let p = part.clone();
            let actual = tokio::task::spawn_blocking(move || sha1_file(&p))
                .await
                .map_err(|e| CoreError::InvalidData(e.to_string()))??;
            if !expected.eq_ignore_ascii_case(&actual) {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(CoreError::Checksum { path: task.path.clone(), expected: expected.clone(), actual });
            }
        }
        tokio::fs::rename(&part, &task.path).await.at(&task.path)
    }

    /// 下载一个块：从块的当前位置读到它的结束位置（结束位置可能因拆分而缩小）。
    #[allow(clippy::too_many_arguments)]
    async fn fetch_piece(
        &self,
        url: &str,
        state: &std::sync::Mutex<SplitState>,
        index: usize,
        part: &Path,
        size: u64,
        counters: &Counters,
        done_total: &AtomicU64,
        cancel: &CancelToken,
    ) -> Result<()> {
        use tokio::io::AsyncSeekExt;
        let (pos, end) = {
            let st = state.lock().expect("split state");
            (st.pieces[index].pos, st.pieces[index].end)
        };
        let req = self.http.raw().get(url);
        let req = if pos == 0 && end == size { req.header("Range", "bytes=0-") } else { req.header("Range", format!("bytes={pos}-{}", end - 1)) };
        let resp = tokio::time::timeout(PIECE_STALL * 2, req.send())
            .await
            .map_err(|_| CoreError::InvalidData(format!("{url} 连接超时")))?
            .map_err(|e| CoreError::network(url, e))?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(CoreError::HttpStatus { url: url.to_string(), status });
        }
        if status != 206 {
            // 服务器不支持分块：只有从头开始的块可以继续（整个文件单线程）
            if pos != 0 {
                return Err(CoreError::HttpStatus { url: url.to_string(), status: 416 });
            }
            state.lock().expect("split state").no_ranges = true;
        }
        let mut file = tokio::fs::OpenOptions::new().write(true).open(part).await.at(part)?;
        file.seek(std::io::SeekFrom::Start(pos)).await.at(part)?;
        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::with_capacity(PIECE_FLUSH);
        let mut at = pos;
        loop {
            if cancel.is_cancelled() {
                return Err(CoreError::Cancelled);
            }
            let next = tokio::time::timeout(PIECE_STALL, stream.next()).await;
            let chunk = match next {
                Err(_) => Err(CoreError::InvalidData(format!("{url} 下载缓慢，换用其他下载源"))),
                Ok(None) => Ok(None),
                Ok(Some(Err(e))) => Err(CoreError::network(url, e)),
                Ok(Some(Ok(c))) => Ok(Some(c)),
            };
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    // 已经收到的数据先写入，下一个连接从这里继续
                    flush_piece(&mut file, &mut buf, state, index, &mut at, part, counters, done_total).await?;
                    return Err(e);
                }
            };
            let Some(chunk) = chunk else { break };
            self.http.limiter().take(chunk.len() as u64).await;
            counters.net_bytes.fetch_add(chunk.len() as u64, Ordering::Relaxed);
            // 只写到当前的结束位置（块可能已被拆分）
            let limit = state.lock().expect("split state").pieces[index].end;
            let room = limit.saturating_sub(at + buf.len() as u64) as usize;
            buf.extend_from_slice(&chunk[..chunk.len().min(room)]);
            {
                let mut st = state.lock().expect("split state");
                st.started = true;
            }
            if buf.len() >= PIECE_FLUSH || chunk.len() > room {
                flush_piece(&mut file, &mut buf, state, index, &mut at, part, counters, done_total).await?;
            }
            if at >= limit {
                return Ok(());
            }
        }
        flush_piece(&mut file, &mut buf, state, index, &mut at, part, counters, done_total).await?;
        let st = state.lock().expect("split state");
        if st.pieces[index].pos < st.pieces[index].end {
            return Err(CoreError::InvalidData(format!("{url} 返回的数据不完整")));
        }
        Ok(())
    }
}

/// 把缓冲的数据写入文件，并推进块的位置与进度。
#[allow(clippy::too_many_arguments)]
async fn flush_piece(
    file: &mut tokio::fs::File,
    buf: &mut Vec<u8>,
    state: &std::sync::Mutex<SplitState>,
    index: usize,
    at: &mut u64,
    part: &Path,
    counters: &Counters,
    done_total: &AtomicU64,
) -> Result<()> {
    if buf.is_empty() {
        return Ok(());
    }
    file.write_all(buf).await.at(part)?;
    let n = buf.len() as u64;
    *at += n;
    buf.clear();
    state.lock().expect("split state").pieces[index].pos = *at;
    counters.done_bytes.fetch_add(n, Ordering::Relaxed);
    done_total.fetch_add(n, Ordering::Relaxed);
    Ok(())
}

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
    async fn verify_cache_skips_unchanged_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lib.jar");
        std::fs::write(&path, b"library").unwrap();
        let sha1 = sha1_bytes(b"library");
        let cache = Arc::new(VerifyCache::load(dir.path().join(".rcl-verified")));
        let d = Downloader::new(HttpClient::new(crate::download::DownloadSource::Official).unwrap(), 4).with_verify_cache(cache.clone());
        let task = DownloadTask::new("http://127.0.0.1:9/never", &path).sha1(Some(sha1.clone())).size(Some(7));
        assert!(d.is_valid_cached(&task).await.unwrap());
        cache.save();
        // 重新读取记录：没有变化的文件直接通过
        let cache2 = VerifyCache::load(dir.path().join(".rcl-verified"));
        let meta = std::fs::metadata(&path).unwrap();
        assert!(cache2.known(&path, &meta, &sha1));
        // 内容被改动（修改时间变化）后不再信任记录
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&path, b"libraryX").unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        assert!(!cache2.known(&path, &meta, &sha1));
        let d2 = Downloader::new(HttpClient::new(crate::download::DownloadSource::Official).unwrap(), 4).with_verify_cache(Arc::new(cache2));
        let task = DownloadTask { size: None, ..task };
        assert!(!d2.is_valid_cached(&task).await.unwrap());
    }

    /// 本地 HTTP 服务：支持 Range，每个连接限速（模拟单连接很慢的网络）；路径为 /bad 时返回 404。
    async fn serve(data: Arc<Vec<u8>>, per_conn_bps: usize) -> (String, Arc<AtomicUsize>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let conns = Arc::new(AtomicUsize::new(0));
        let c = conns.clone();
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let data = data.clone();
                c.fetch_add(1, Ordering::Relaxed);
                tokio::spawn(async move {
                    let mut req = Vec::new();
                    let mut b = [0u8; 1024];
                    while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                        let n = sock.read(&mut b).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        req.extend_from_slice(&b[..n]);
                    }
                    let text = String::from_utf8_lossy(&req).to_string();
                    if text.starts_with("GET /bad") {
                        let _ = sock.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").await;
                        return;
                    }
                    let range = text.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("range: bytes=").map(str::to_string));
                    let (start, end) = match range {
                        Some(r) => {
                            let (a, b) = r.trim().split_once('-').unwrap();
                            let a: usize = a.parse().unwrap();
                            let b: usize = if b.is_empty() { data.len() - 1 } else { b.parse().unwrap() };
                            (a, b.min(data.len() - 1))
                        }
                        None => (0, data.len() - 1),
                    };
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\n\r\n",
                        end - start + 1,
                        data.len()
                    );
                    if sock.write_all(head.as_bytes()).await.is_err() {
                        return;
                    }
                    for chunk in data[start..=end].chunks(per_conn_bps / 20) {
                        if sock.write_all(chunk).await.is_err() {
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                });
            }
        });
        (format!("http://{addr}"), conns)
    }

    #[tokio::test]
    async fn split_download_uses_multiple_connections() {
        use crate::download::DownloadSource;
        let data: Arc<Vec<u8>> = Arc::new((0..4 * 1024 * 1024u32).map(|i| (i * 7 % 251) as u8).collect());
        let sha1 = sha1_bytes(&data);
        // 每个连接 1MB/s：单线程需要 4 秒
        let (base, conns) = serve(data.clone(), 1024 * 1024).await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.bin");
        let d = Downloader::new(HttpClient::new(DownloadSource::Official).unwrap(), 64);
        let task = DownloadTask::new(format!("{base}/file"), &path).sha1(Some(sha1.clone())).size(Some(data.len() as u64));
        let started = Instant::now();
        d.download_all(vec![task], Arc::new(|_| {}), &CancelToken::new()).await.unwrap();
        let took = started.elapsed();
        assert_eq!(std::fs::read(&path).unwrap(), *data);
        assert!(conns.load(Ordering::Relaxed) > 2, "应当使用多个连接");
        assert!(took < Duration::from_millis(3000), "分块下载应快于单线程：{took:?}");
        assert_eq!(d.extra.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn split_download_skips_broken_source() {
        let data: Arc<Vec<u8>> = Arc::new((0..2 * 1024 * 1024u32).map(|i| (i % 253) as u8).collect());
        let (base, _) = serve(data.clone(), 8 * 1024 * 1024).await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.bin");
        let d = Downloader::new(HttpClient::new(crate::download::DownloadSource::Official).unwrap(), 64);
        let task = DownloadTask::new("x", &path).sha1(Some(sha1_bytes(&data))).size(Some(data.len() as u64));
        let counters = Arc::new(Counters::default());
        let mut written = 0;
        // 第一个源返回 404，应当停用并从第二个源下载
        d.download_split(&task, &[format!("{base}/bad"), format!("{base}/good")], &counters, &CancelToken::new(), &mut written)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), *data);
        assert_eq!(written, data.len() as u64);
    }

    /// 联网测试：分块下载与单线程下载同一个大文件，对比耗时并校验 SHA1。
    #[tokio::test]
    #[ignore]
    async fn split_download_real_file() {
        use crate::download::DownloadSource;
        // Minecraft 1.20.1 客户端（约 23MB）
        let url = "https://piston-data.mojang.com/v1/objects/0c3ec587af28e5a785c0b4a7b8a30f9a8f78f838/client.jar";
        let sha1 = "0c3ec587af28e5a785c0b4a7b8a30f9a8f78f838";
        let size = 23028853;
        let dir = tempfile::tempdir().unwrap();
        for (name, split) in [("分块", true), ("单线程", false)] {
            let d = Downloader::new(HttpClient::new(DownloadSource::Official).unwrap(), 64);
            let path = dir.path().join(format!("{split}.jar"));
            let task = DownloadTask::new(url, &path).sha1(Some(sha1)).size(Some(size));
            let task = if split { task } else { DownloadTask { size: None, ..task } };
            let started = Instant::now();
            d.download_all(vec![task], Arc::new(|_| {}), &CancelToken::new()).await.unwrap();
            println!("{name}：{:?}", started.elapsed());
            assert_eq!(sha1_file(&path).unwrap(), sha1);
        }
    }

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
    fn tail_restart_rule() {
        let s = Duration::from_secs;
        // 收尾阶段、超过 6 秒、低于 32KB/s：重新连接
        assert!(tail_too_slow(4, s(8), 40 * 1024, 800 * 1024));
        // 还有很多文件在下载：带宽被分摊，慢是正常的
        assert!(!tail_too_slow(500, s(30), 1024, 800 * 1024));
        // 刚开始下载
        assert!(!tail_too_slow(4, s(3), 0, 800 * 1024));
        // 速度正常
        assert!(!tail_too_slow(4, s(8), 8 * 1024 * 1024, 20 * 1024 * 1024));
        // 已经下完
        assert!(!tail_too_slow(4, s(8), 1024, 1024));
    }

    #[test]
    fn part_suffix() {
        assert_eq!(
            part_path(Path::new("/a/b/c.jar")),
            PathBuf::from("/a/b/c.jar.part")
        );
    }
}
