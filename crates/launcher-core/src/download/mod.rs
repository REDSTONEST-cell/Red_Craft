//! 下载相关：下载源、HTTP 客户端与并发下载器。

mod downloader;
mod http;
mod source;

pub use downloader::{
    is_valid, sha1_bytes, sha1_file, CancelToken, DownloadTask, Downloader, Progress, ProgressFn,
};
pub use http::{user_agent, HttpClient};
pub use source::{mirror_of, DownloadSource, BMCLAPI, MCIM};
