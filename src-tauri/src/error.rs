//! 返回给前端的错误。序列化为 `{ message, hint }`。

use launcher_core::{CoreError, ErrorInfo};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error("{message}")]
    Message { message: String, hint: String },
}

impl AppError {
    pub fn msg(message: impl Into<String>, hint: impl Into<String>) -> Self {
        AppError::Message {
            message: message.into(),
            hint: hint.into(),
        }
    }

    pub fn info(&self) -> ErrorInfo {
        match self {
            AppError::Core(e) => ErrorInfo::from(e),
            AppError::Message { message, hint } => ErrorInfo {
                message: message.clone(),
                hint: hint.clone(),
            },
        }
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, AppError::Core(CoreError::Cancelled))
    }
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.info().serialize(s)
    }
}

pub type AppResult<T> = Result<T, AppError>;
