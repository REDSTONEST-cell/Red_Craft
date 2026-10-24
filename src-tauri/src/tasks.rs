//! 后台任务：统一的进度事件与取消。

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use launcher_core::download::{CancelToken, Progress};
use launcher_core::install::Reporter;
use launcher_core::ErrorInfo;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::error::AppResult;

pub const TASK_EVENT: &str = "task";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEvent {
    pub id: u64,
    pub title: String,
    pub stage: String,
    pub progress: Option<Progress>,
    pub state: TaskState,
    pub error: Option<ErrorInfo>,
}

#[derive(Default)]
pub struct Tasks {
    next: AtomicU64,
    tokens: Mutex<HashMap<u64, CancelToken>>,
}

impl Tasks {
    pub fn cancel(&self, id: u64) -> bool {
        match self.tokens.lock().ok().and_then(|t| t.get(&id).cloned()) {
            Some(t) => {
                t.cancel();
                true
            }
            None => false,
        }
    }

    /// 启动一个后台任务，立即返回任务 id。进度通过 `task` 事件推送。
    pub fn spawn<F, Fut>(self: &Arc<Self>, app: &AppHandle, title: impl Into<String>, f: F) -> u64
    where
        F: FnOnce(Reporter, CancelToken) -> Fut + Send + 'static,
        Fut: Future<Output = AppResult<()>> + Send + 'static,
    {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let token = CancelToken::new();
        if let Ok(mut t) = self.tokens.lock() {
            t.insert(id, token.clone());
        }
        let title = title.into();
        let base = TaskEvent {
            id,
            title,
            stage: "准备中".into(),
            progress: None,
            state: TaskState::Running,
            error: None,
        };
        let shared = Arc::new(Mutex::new(base.clone()));
        let emit = {
            let app = app.clone();
            move |ev: &TaskEvent| {
                let _ = app.emit(TASK_EVENT, ev);
            }
        };
        emit(&base);

        let reporter = {
            let s1 = shared.clone();
            let s2 = shared.clone();
            let e1 = emit.clone();
            let e2 = emit.clone();
            Reporter {
                stage: Arc::new(move |stage: &str| {
                    if let Ok(mut ev) = s1.lock() {
                        ev.stage = stage.to_string();
                        ev.progress = None;
                        e1(&ev);
                    }
                }),
                progress: Arc::new(move |p: Progress| {
                    if let Ok(mut ev) = s2.lock() {
                        ev.progress = Some(p);
                        e2(&ev);
                    }
                }),
            }
        };

        let tasks = self.clone();
        tauri::async_runtime::spawn(async move {
            let result = f(reporter, token).await;
            let mut ev = shared.lock().map(|e| e.clone()).unwrap_or(base);
            match result {
                Ok(()) => ev.state = TaskState::Done,
                Err(e) if e.is_cancelled() => ev.state = TaskState::Cancelled,
                Err(e) => {
                    log::error!("任务「{}」失败：{e}", ev.title);
                    ev.state = TaskState::Failed;
                    ev.error = Some(e.info());
                }
            }
            emit(&ev);
            if let Ok(mut t) = tasks.tokens.lock() {
                t.remove(&id);
            }
        });
        id
    }
}
