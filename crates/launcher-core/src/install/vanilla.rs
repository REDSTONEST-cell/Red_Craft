//! 原版安装与文件补全。

use std::sync::Arc;

use crate::download::{sha1_bytes, CancelToken, DownloadTask, Downloader, ProgressFn};
use crate::error::{CoreError, IoContext, Result};
use crate::paths::GameDir;
use crate::version::{plan_libraries, resolve_version, Environment, ManifestEntry, ResolvedVersion};

use super::assets::AssetIndex;

/// 任务阶段与进度回调。
#[derive(Clone)]
pub struct Reporter {
    pub stage: Arc<dyn Fn(&str) + Send + Sync>,
    pub progress: ProgressFn,
}

impl Reporter {
    pub fn silent() -> Self {
        Reporter {
            stage: Arc::new(|_| {}),
            progress: Arc::new(|_| {}),
        }
    }
    pub fn stage(&self, s: &str) {
        (self.stage)(s)
    }
}

/// 文件校验强度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verify {
    /// 所有文件都校验 SHA1
    Full,
    /// 资源文件只比较大小（启动前快速检查，避免每次计算数百 MB 的哈希）
    Quick,
}

#[derive(Clone)]
pub struct Installer {
    pub downloader: Downloader,
    pub game: GameDir,
    pub env: Environment,
}

impl Installer {
    pub fn new(downloader: Downloader, game: GameDir) -> Self {
        Installer {
            downloader,
            game,
            env: Environment::current(),
        }
    }

    /// 安装一个原版版本，`name` 为实例名（可与官方 id 不同）。
    pub async fn install_vanilla(
        &self,
        entry: &ManifestEntry,
        name: &str,
        reporter: &Reporter,
        cancel: &CancelToken,
    ) -> Result<()> {
        validate_instance_name(name)?;
        reporter.stage("下载版本信息");
        self.save_version_json(entry, name).await?;
        cancel.check()?;
        self.complete(name, Verify::Full, reporter, cancel).await
    }

    /// 下载官方版本 JSON 并以 `name` 保存。
    pub async fn save_version_json(&self, entry: &ManifestEntry, name: &str) -> Result<()> {
        let bytes = self.downloader.http().get_bytes(&entry.url).await?;
        if !entry.sha1.is_empty() {
            let actual = sha1_bytes(&bytes);
            if !actual.eq_ignore_ascii_case(&entry.sha1) {
                return Err(CoreError::Checksum {
                    path: self.game.version_json(name),
                    expected: entry.sha1.clone(),
                    actual,
                });
            }
        }
        let bytes = if name == entry.id {
            bytes
        } else {
            let mut value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|e| CoreError::json("版本 JSON", e))?;
            value["id"] = serde_json::Value::String(name.to_string());
            serde_json::to_vec_pretty(&value).map_err(|e| CoreError::json("版本 JSON", e))?
        };
        let path = self.game.version_json(name);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.at(parent)?;
        }
        tokio::fs::write(&path, bytes).await.at(&path)
    }

    /// 补全版本运行所需的全部文件：客户端 jar、库、natives、资源索引与资源、日志配置。
    pub async fn complete(
        &self,
        id: &str,
        verify: Verify,
        reporter: &Reporter,
        cancel: &CancelToken,
    ) -> Result<()> {
        let resolved = resolve_version(&self.game, id)?;
        let json = &resolved.json;
        let mut tasks = Vec::new();

        // 客户端 jar
        if let Some(client) = json.downloads.as_ref().and_then(|d| d.client.as_ref()) {
            tasks.push(
                DownloadTask::new(&client.url, self.game.version_jar(&resolved.jar_id))
                    .sha1(client.sha1.clone())
                    .size(client.size),
            );
        } else if !self.game.version_jar(&resolved.jar_id).exists() {
            return Err(CoreError::InvalidData(format!(
                "版本 {} 缺少客户端 jar 的下载信息",
                resolved.jar_id
            )));
        }

        // 库与 natives
        let mut missing_generated = Vec::new();
        for lib in plan_libraries(&json.libraries, &self.env) {
            let path = self.game.library(&lib.path);
            if lib.url.is_empty() {
                // 由 Forge 等安装器生成的文件，无法直接下载
                if !path.exists() {
                    missing_generated.push(lib.name);
                }
                continue;
            }
            tasks.push(DownloadTask::new(lib.url, path).sha1(lib.sha1).size(lib.size));
        }
        if !missing_generated.is_empty() {
            return Err(CoreError::InvalidData(format!(
                "缺少由加载器安装器生成的文件（{}），请重新安装该加载器",
                missing_generated.join("，")
            )));
        }

        // 日志配置
        if let Some(log) = json.logging.as_ref().and_then(|l| l.client.as_ref()) {
            tasks.push(
                DownloadTask::new(&log.file.url, self.game.log_configs().join(&log.file.id))
                    .sha1(Some(log.file.sha1.clone()))
                    .size(Some(log.file.size)),
            );
        }

        // 资源索引需要先下载才能知道有哪些资源
        let mut index = None;
        if let Some(ai) = &json.asset_index {
            reporter.stage("下载资源索引");
            let path = self.game.asset_index(&ai.id);
            let task = DownloadTask::new(&ai.url, &path)
                .sha1(Some(ai.sha1.clone()))
                .size(Some(ai.size));
            self.downloader
                .download_all(vec![task], Arc::new(|_| {}), cancel)
                .await?;
            let parsed = {
                let path = path.clone();
                tokio::task::spawn_blocking(move || AssetIndex::read(&path))
                    .await
                    .map_err(|e| CoreError::InvalidData(e.to_string()))??
            };
            for obj in parsed.objects.values() {
                let sha1 = (verify == Verify::Full).then(|| obj.hash.clone());
                tasks.push(
                    DownloadTask::new(obj.url(), self.game.asset_object(&obj.hash))
                        .sha1(sha1)
                        .size(Some(obj.size)),
                );
            }
            index = Some((ai.id.clone(), parsed));
        }

        reporter.stage(if verify == Verify::Full {
            "下载游戏文件"
        } else {
            "检查游戏文件"
        });
        self.downloader
            .download_all(tasks, reporter.progress.clone(), cancel)
            .await?;

        if let Some((index_id, idx)) = index {
            if idx.is_virtual {
                reporter.stage("整理旧版资源");
                let game = self.game.clone();
                let target = self.game.asset_virtual(&index_id);
                tokio::task::spawn_blocking(move || idx.copy_to_named_dir(&game, &target))
                    .await
                    .map_err(|e| CoreError::InvalidData(e.to_string()))??;
            }
        }
        Ok(())
    }

    pub fn resolve(&self, id: &str) -> Result<ResolvedVersion> {
        resolve_version(&self.game, id)
    }
}

/// 实例名会作为目录名，需要检查非法字符。
pub fn validate_instance_name(name: &str) -> Result<()> {
    const BAD: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    if name.trim().is_empty() {
        return Err(CoreError::InvalidData("版本名称不能为空".into()));
    }
    if name.chars().any(|c| BAD.contains(&c) || c.is_control()) {
        return Err(CoreError::InvalidData(
            "版本名称不能包含 / \\ : * ? \" < > | 等字符".into(),
        ));
    }
    if name.ends_with('.') || name.ends_with(' ') || name.starts_with('.') {
        return Err(CoreError::InvalidData("版本名称不能以点或空格开头/结尾".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_names() {
        assert!(validate_instance_name("1.20.1").is_ok());
        assert!(validate_instance_name("我的 生存 整合包").is_ok());
        assert!(validate_instance_name("a/b").is_err());
        assert!(validate_instance_name("  ").is_err());
        assert!(validate_instance_name("x.").is_err());
        assert!(validate_instance_name("..").is_err());
    }
}
