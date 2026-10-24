//! 启动游戏：参数拼装、natives 解压、启动进程。

mod args;
mod natives;
mod process;

pub use args::{build_command, split_args, substitute, LaunchCommand, LaunchOptions, ServerAddress};
pub use natives::extract_natives;
pub use process::{analyze, kill_pid, spawn, CrashCause, GameExit, LogFn, LogLine, RunningGame};

use crate::download::CancelToken;
use crate::error::{CoreError, Result};
use crate::install::{AssetIndex, Installer, Reporter, Verify};

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
