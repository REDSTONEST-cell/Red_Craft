//! 启动游戏：参数拼装、natives 解压、启动进程。

mod args;
mod natives;
mod process;

pub use args::{build_command, split_args, substitute, GcMode, LaunchCommand, LaunchOptions, ServerAddress};
pub use natives::extract_natives;
pub use process::{analyze, kill_pid, spawn, CrashCause, GameExit, LogFn, LogLine, RunningGame};

use crate::download::CancelToken;
use crate::error::{CoreError, Result};
use crate::install::{AssetIndex, Installer, Reporter, Verify};

/// 游戏目录中还没有 options.txt（第一次启动）时写入简体中文语言设置；已有的不覆盖。
///
/// 16w32a（2016 年 8 月）之后语言代码改为小写 `zh_cn`，之前是 `zh_CN`。
pub fn ensure_chinese(game_dir: &std::path::Path, release_time: &str) -> Result<()> {
    let options = game_dir.join("options.txt");
    if options.exists() {
        return Ok(());
    }
    let lang = if !release_time.is_empty() && release_time < "2016-08-10" { "zh_CN" } else { "zh_cn" };
    std::fs::create_dir_all(game_dir).map_err(|e| CoreError::io(game_dir, e))?;
    std::fs::write(&options, format!("lang:{lang}\n")).map_err(|e| CoreError::io(&options, e))
}

/// 启动前准备：补全缺失文件、解压 natives、整理旧版资源，然后生成启动命令。
pub async fn prepare(
    installer: &Installer,
    id: &str,
    opts: &LaunchOptions,
    reporter: &Reporter,
    cancel: &CancelToken,
) -> Result<LaunchCommand> {
    installer.complete(id, Verify::Quick, reporter, cancel).await?;
    cancel.check()?;
    let resolved = installer.resolve(id)?;

    reporter.stage("解压 natives");
    let game = installer.game.clone();
    let env = installer.env.clone();
    let libs = resolved.json.libraries.clone();
    let natives = game.natives_dir(id);
    let game_dir = opts.game_dir.clone();
    let asset_id = resolved.json.asset_index.as_ref().map(|a| a.id.clone());
    tokio::task::spawn_blocking(move || -> Result<()> {
        extract_natives(&game, &libs, &env, &natives)?;
        // 1.6 之前的版本从游戏目录的 resources 读取资源
        if let Some(asset_id) = asset_id {
            if let Ok(index) = AssetIndex::read(&game.asset_index(&asset_id)) {
                if index.map_to_resources {
                    index.copy_to_named_dir(&game, &game_dir.join("resources"))?;
                }
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| CoreError::Launch(e.to_string()))??;

    reporter.stage("生成启动参数");
    build_command(&installer.game, &resolved, &installer.env, opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_options() {
        let dir = tempfile::tempdir().unwrap();
        let new = dir.path().join("new");
        ensure_chinese(&new, "2023-06-12T13:25:51+00:00").unwrap();
        assert_eq!(std::fs::read_to_string(new.join("options.txt")).unwrap(), "lang:zh_cn\n");
        let old = dir.path().join("old");
        ensure_chinese(&old, "2014-06-26T11:23:59+00:00").unwrap();
        assert_eq!(std::fs::read_to_string(old.join("options.txt")).unwrap(), "lang:zh_CN\n");
        // 已有设置不覆盖
        std::fs::write(new.join("options.txt"), "lang:en_us\n").unwrap();
        ensure_chinese(&new, "2023").unwrap();
        assert_eq!(std::fs::read_to_string(new.join("options.txt")).unwrap(), "lang:en_us\n");
    }
}
