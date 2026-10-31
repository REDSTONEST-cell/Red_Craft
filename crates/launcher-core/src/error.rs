//! 统一错误类型。所有错误信息都是面向用户的中文描述，并通过 [`CoreError::hint`]
//! 给出可操作的解决办法。

use std::path::{Path, PathBuf};

pub type Result<T, E = CoreError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("网络请求失败：{reason}（{url}）")]
    Network { url: String, reason: String },

    #[error("服务器返回了错误状态 {status}（{url}）")]
    HttpStatus { url: String, status: u16 },

    #[error("文件校验失败：{path}（期望 SHA1 {expected}，实际 {actual}）")]
    Checksum {
        path: PathBuf,
        expected: String,
        actual: String,
    },

    #[error("读写文件失败：{path}（{source}）")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("解析{what}失败：{source}")]
    Json {
        what: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("解压 {path} 失败：{reason}")]
    Archive { path: PathBuf, reason: String },

    #[error("找不到版本 {0}")]
    VersionNotFound(String),

    #[error("数据格式不正确：{0}")]
    InvalidData(String),

    #[error("{failed} 个文件下载失败（共 {total} 个），首个错误：{first}")]
    DownloadFailed {
        failed: usize,
        total: usize,
        first: Box<CoreError>,
    },

    #[error("Java 错误：{0}")]
    Java(String),

    #[error("启动失败：{0}")]
    Launch(String),

    #[error("登录失败：{message}")]
    Auth { message: String, hint: String },

    #[error("操作已取消")]
    Cancelled,

    /// 其他需要直接展示给用户的错误
    #[error("{message}")]
    Message { message: String, hint: String },
}

impl CoreError {
    pub fn network(url: impl Into<String>, err: reqwest::Error) -> Self {
        let reason = if err.is_timeout() {
            "连接超时".to_string()
        } else if err.is_connect() {
            "无法连接到服务器".to_string()
        } else if err.is_decode() {
            "响应内容无法解析".to_string()
        } else if err.is_body() {
            "传输中断".to_string()
        } else {
            // reqwest 的错误链里通常包含更底层的原因（如 DNS、TLS）
            let mut msg = err.to_string();
            let mut src = std::error::Error::source(&err);
            while let Some(s) = src {
                msg = format!("{msg}: {s}");
                src = s.source();
            }
            msg
        };
        CoreError::Network {
            url: url.into(),
            reason,
        }
    }

    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        CoreError::Io {
            path: path.into(),
            source,
        }
    }

    pub fn json(what: impl Into<String>, source: serde_json::Error) -> Self {
        CoreError::Json {
            what: what.into(),
            source,
        }
    }

    pub fn msg(message: impl Into<String>, hint: impl Into<String>) -> Self {
        CoreError::Message {
            message: message.into(),
            hint: hint.into(),
        }
    }

    pub fn auth(message: impl Into<String>, hint: impl Into<String>) -> Self {
        CoreError::Auth {
            message: message.into(),
            hint: hint.into(),
        }
    }

    /// 给用户的解决建议。
    pub fn hint(&self) -> String {
        match self {
            CoreError::Network { .. } => {
                "请检查网络连接；如果在国内，可以在「设置 → 下载」中把下载源切换为 BMCLAPI 镜像或「自动」。".into()
            }
            CoreError::HttpStatus { status, .. } => match status {
                404 => "服务器上不存在这个文件，可能是版本已被下架；可以尝试切换下载源。".into(),
                403 => "服务器拒绝了请求，可能是访问过于频繁，请稍后重试或切换下载源。".into(),
                429 => "请求过于频繁，请稍等几分钟后再试，或降低「设置 → 下载 → 并发数」。".into(),
                500..=599 => "下载服务器暂时出现故障，请稍后重试或切换下载源。".into(),
                _ => "请稍后重试，或在设置中切换下载源。".into(),
            },
            CoreError::Checksum { .. } => {
                "下载的文件已损坏或被篡改，重新下载通常即可解决；若反复出现，请切换下载源。".into()
            }
            CoreError::Io { source, .. } => match source.kind() {
                std::io::ErrorKind::PermissionDenied => {
                    "没有权限写入该位置。请不要把游戏目录放在 C:\\Program Files 等系统目录，或以管理员身份运行；也可能是文件被游戏或杀毒软件占用。".into()
                }
                std::io::ErrorKind::NotFound => "文件或目录不存在，请检查游戏目录设置是否正确。".into(),
                _ if source.raw_os_error() == Some(112) || source.raw_os_error() == Some(28) => {
                    "磁盘空间不足，请清理磁盘后重试。".into()
                }
                _ => "请确认磁盘空间充足、文件未被其它程序占用。".into(),
            },
            CoreError::Json { .. } => {
                "文件内容已损坏。可以删除对应文件后重新下载；若是版本 JSON 损坏，在「版本」页面点击「补全文件」即可。".into()
            }
            CoreError::Archive { .. } => "压缩包已损坏，删除该文件后重新下载即可。".into(),
            CoreError::VersionNotFound(_) => "请先在「下载」页面安装这个版本，或检查这个版本是否已被删除。".into(),
            CoreError::InvalidData(_) => "请尝试重新下载对应文件。".into(),
            CoreError::DownloadFailed { first, .. } => first.hint(),
            CoreError::Java(_) => {
                "可以在「设置 → Java」里手动指定 Java 路径，或开启「自动下载 Java」。".into()
            }
            CoreError::Launch(_) => "请查看游戏日志了解详细原因。".into(),
            CoreError::Auth { hint, .. } | CoreError::Message { hint, .. } => hint.clone(),
            CoreError::Cancelled => String::new(),
        }
    }
}

/// 给 `std::io::Result` 附加出错路径。
pub trait IoContext<T> {
    fn at(self, path: &Path) -> Result<T>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn at(self, path: &Path) -> Result<T> {
        self.map_err(|e| CoreError::io(path, e))
    }
}

/// 可序列化的错误，用于传给前端。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ErrorInfo {
    pub message: String,
    pub hint: String,
}

impl From<&CoreError> for ErrorInfo {
    fn from(e: &CoreError) -> Self {
        ErrorInfo {
            message: e.to_string(),
            hint: e.hint(),
        }
    }
}
