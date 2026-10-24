//! 带下载源回退的 HTTP 客户端。

use std::time::Duration;

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

#[derive(Debug, Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
    source: DownloadSource,
}

impl HttpClient {
    pub fn new(source: DownloadSource) -> Result<Self> {
        let inner = reqwest::Client::builder()
            .user_agent(user_agent())
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(64)
            .build()
            .map_err(|e| CoreError::network("(初始化 HTTP 客户端)", e))?;
        Ok(HttpClient { inner, source })
    }

    pub fn source(&self) -> DownloadSource {
        self.source
    }

    pub fn with_source(&self, source: DownloadSource) -> Self {
        HttpClient {
            inner: self.inner.clone(),
            source,
        }
    }

    pub fn raw(&self) -> &reqwest::Client {
        &self.inner
    }

    pub fn candidates(&self, url: &str) -> Vec<String> {
        self.source.candidates(url)
    }

    /// 发送 GET 请求，按下载源依次尝试，返回第一个成功的响应。
    pub async fn get(&self, url: &str) -> Result<reqwest::Response> {
        let mut last = None;
        for candidate in self.candidates(url) {
            match self.inner.get(&candidate).send().await {
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
