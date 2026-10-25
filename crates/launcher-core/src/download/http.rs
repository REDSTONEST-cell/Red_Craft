//! 带下载源回退的 HTTP 客户端。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;

use super::source::DownloadSource;
use crate::error::{CoreError, Result};

pub fn user_agent() -> String {
    format!(
        "{}/{} (ji_hongshi@outlook.com)",
        crate::LAUNCHER_ID,
        crate::LAUNCHER_VERSION
    )
}

/// 全局下载限速（令牌桶）。速率为 0 表示不限速；所有共享同一个限速器的客户端合计受限。
#[derive(Debug, Default)]
pub struct RateLimiter {
    /// 字节/秒
    rate: AtomicU64,
    state: Mutex<Option<(f64, Instant)>>,
}

impl RateLimiter {
    pub fn set_rate(&self, bytes_per_sec: u64) {
        self.rate.store(bytes_per_sec, Ordering::Relaxed);
    }

    pub fn rate(&self) -> u64 {
        self.rate.load(Ordering::Relaxed)
    }

    /// 取得 `n` 字节的额度，不够时等待。
    pub async fn take(&self, n: u64) {
        loop {
            let rate = self.rate();
            if rate == 0 {
                return;
            }
            let wait = {
                let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
                let now = Instant::now();
                // 桶容量为 1 秒的额度
                let (tokens, last) = guard.get_or_insert((rate as f64, now));
                *tokens = (*tokens + now.duration_since(*last).as_secs_f64() * rate as f64)
                    .min(rate as f64);
                *last = now;
                if *tokens >= n as f64 || *tokens >= rate as f64 {
                    *tokens -= n as f64;
                    return;
                }
                Duration::from_secs_f64(((n as f64 - *tokens) / rate as f64).min(1.0))
            };
            tokio::time::sleep(wait).await;
        }
    }
}

#[derive(Debug, Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
    source: DownloadSource,
    limiter: Arc<RateLimiter>,
    /// 「优先官方源，缓慢时换用镜像」模式下，官方源被判定为缓慢后改为镜像优先
    official_slow: Arc<AtomicBool>,
}

/// 官方源响应超过这个时间即视为缓慢（仅在有镜像可用时生效）。
const SLOW_RESPONSE: Duration = Duration::from_secs(6);

impl HttpClient {
    pub fn new(source: DownloadSource) -> Result<Self> {
        let inner = reqwest::Client::builder()
            .user_agent(user_agent())
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(64)
            .build()
            .map_err(|e| CoreError::network("(初始化 HTTP 客户端)", e))?;
        Ok(HttpClient {
            inner,
            source,
            limiter: Arc::new(RateLimiter::default()),
            official_slow: Arc::new(AtomicBool::new(false)),
        })
    }

    /// 全局限速器（与 `with_source` 派生出的客户端共享）。
    pub fn limiter(&self) -> &Arc<RateLimiter> {
        &self.limiter
    }

    /// 标记官方源缓慢：之后的请求改为镜像优先。
    pub fn mark_official_slow(&self) {
        if self.source == DownloadSource::Auto && !self.official_slow.swap(true, Ordering::Relaxed)
        {
            log::info!("官方源响应缓慢，已切换为镜像源优先");
        }
    }

    /// 是否处于「官方优先、缓慢时换镜像」且还在使用官方源的阶段。
    pub fn checks_slowness(&self) -> bool {
        self.source == DownloadSource::Auto && !self.official_slow.load(Ordering::Relaxed)
    }

    pub fn source(&self) -> DownloadSource {
        self.source
    }

    pub fn with_source(&self, source: DownloadSource) -> Self {
        HttpClient {
            inner: self.inner.clone(),
            source,
            limiter: self.limiter.clone(),
            official_slow: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn raw(&self) -> &reqwest::Client {
        &self.inner
    }

    pub fn candidates(&self, url: &str) -> Vec<String> {
        let mut list = self.source.candidates(url);
        if self.source == DownloadSource::Auto && self.official_slow.load(Ordering::Relaxed) {
            list.reverse();
        }
        list
    }

    /// 发送 GET 请求，按下载源依次尝试，返回第一个成功的响应。
    pub async fn get(&self, url: &str) -> Result<reqwest::Response> {
        let mut last = None;
        let candidates = self.candidates(url);
        let has_fallback = candidates.len() > 1;
        for (i, candidate) in candidates.into_iter().enumerate() {
            // 官方优先模式：官方源迟迟不响应时换用镜像
            let send = self.inner.get(&candidate).send();
            let result = if i == 0 && has_fallback && self.checks_slowness() {
                match tokio::time::timeout(SLOW_RESPONSE, send).await {
                    Ok(r) => r,
                    Err(_) => {
                        self.mark_official_slow();
                        last = Some(CoreError::InvalidData(format!("{candidate} 响应缓慢")));
                        continue;
                    }
                }
            } else {
                send.await
            };
            match result {
                Ok(resp) if resp.status().is_success() => return Ok(resp),
                Ok(resp) => {
                    last = Some(CoreError::HttpStatus {
                        url: candidate,
                        status: resp.status().as_u16(),
                    })
                }
                Err(e) => last = Some(CoreError::network(candidate, e)),
            }
        }
        Err(last.unwrap_or_else(|| CoreError::InvalidData(format!("无效的地址 {url}"))))
    }

    pub async fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let resp = self.get(url).await?;
        let final_url = resp.url().to_string();
        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| CoreError::network(final_url, e))
    }

    pub async fn get_text(&self, url: &str) -> Result<String> {
        let bytes = self.get_bytes(url).await?;
        String::from_utf8(bytes).map_err(|_| CoreError::InvalidData(format!("{url} 返回的不是文本")))
    }

    /// 按候选地址依次发送请求（可带请求头与 JSON 请求体），返回解析后的 JSON。
    pub async fn request_json<T: DeserializeOwned>(
        &self,
        urls: &[String],
        body: Option<&serde_json::Value>,
        headers: &[(&str, &str)],
        what: &str,
    ) -> Result<T> {
        let mut last = None;
        for url in urls {
            let mut req = match body {
                Some(b) => self.inner.post(url).json(b),
                None => self.inner.get(url),
            };
            for (k, v) in headers {
                req = req.header(*k, *v);
            }
            match req.send().await {
                Ok(resp) if resp.status().is_success() => {
                    let bytes = resp.bytes().await.map_err(|e| CoreError::network(url.clone(), e))?;
                    return serde_json::from_slice(&bytes).map_err(|e| CoreError::json(what, e));
                }
                Ok(resp) => {
                    last = Some(CoreError::HttpStatus {
                        url: url.clone(),
                        status: resp.status().as_u16(),
                    })
                }
                Err(e) => last = Some(CoreError::network(url.clone(), e)),
            }
        }
        Err(last.unwrap_or_else(|| CoreError::InvalidData("没有可用的请求地址".into())))
    }

    pub async fn post_json<T: DeserializeOwned>(&self, url: &str, body: &serde_json::Value, what: &str) -> Result<T> {
        self.request_json(&self.candidates(url), Some(body), &[], what).await
    }

    pub async fn get_json<T: DeserializeOwned>(&self, url: &str, what: &str) -> Result<T> {
        let bytes = self.get_bytes(url).await?;
        serde_json::from_slice(&bytes).map_err(|e| CoreError::json(what, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rate_limiter_waits_when_bucket_is_empty() {
        let l = RateLimiter::default();
        let t = Instant::now();
        l.take(1_000_000).await; // 不限速
        assert!(t.elapsed() < Duration::from_millis(50));
        l.set_rate(50_000);
        let t = Instant::now();
        l.take(50_000).await; // 桶满，立即通过
        l.take(25_000).await; // 需等待约 0.5 秒
        let e = t.elapsed();
        assert!(
            e >= Duration::from_millis(400) && e < Duration::from_millis(1500),
            "{e:?}"
        );
    }

    #[test]
    fn slow_official_switches_to_mirror_first() {
        let http = HttpClient::new(DownloadSource::Auto).unwrap();
        let url = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
        assert!(http.candidates(url)[0].starts_with("https://piston-meta"));
        assert!(http.checks_slowness());
        http.mark_official_slow();
        assert!(http.candidates(url)[0].starts_with("https://bmclapi2"));
        assert!(!http.checks_slowness());
        // 派生的客户端共享限速器，但各自判断缓慢
        let other = http.with_source(DownloadSource::Auto);
        assert!(Arc::ptr_eq(other.limiter(), http.limiter()));
        assert!(other.checks_slowness());
    }
}
