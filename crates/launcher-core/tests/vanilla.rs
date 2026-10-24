//! 使用真实 API 的集成测试。会下载数百 MB 文件，默认忽略：
//!
//! ```sh
//! cargo test -p launcher-core --test vanilla -- --ignored --nocapture
//! ```
//!
//! 游戏目录默认为 `target/mc-test`，可用环境变量 `RC_TEST_GAME_DIR` 指定。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use launcher_core::auth::LaunchAuth;
use launcher_core::download::{CancelToken, DownloadSource, Downloader, HttpClient};
use launcher_core::install::{Installer, Reporter};
use launcher_core::java;
use launcher_core::launch::{self, LaunchOptions};
use launcher_core::paths::GameDir;
use launcher_core::version::VersionManifest;

fn game_dir() -> GameDir {
    let dir = std::env::var_os("RC_TEST_GAME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/mc-test"));
    GameDir::new(dir)
}

fn reporter() -> Reporter {
    Reporter {
        stage: Arc::new(|s| eprintln!("[阶段] {s}")),
        progress: Arc::new(|p| {
            if p.done_files % 500 == 0 || p.done_files == p.total_files {
                eprintln!(
                    "[进度] {}/{} 文件，{:.1}/{:.1} MB，{:.1} MB/s",
                    p.done_files,
                    p.total_files,
                    p.done_bytes as f64 / 1e6,
                    p.total_bytes as f64 / 1e6,
                    p.speed as f64 / 1e6
                )
            }
        }),
    }
}

pub fn installer() -> Installer {
    let http = HttpClient::new(DownloadSource::Auto).unwrap();
    Installer::new(Downloader::new(http, 64), game_dir())
}

#[tokio::test]
#[ignore = "需要网络，会下载大量文件"]
async fn install_and_launch_1_20_1() {
    let installer = installer();
    let manifest = VersionManifest::fetch(installer.downloader.http()).await.unwrap();
    assert!(manifest.versions.len() > 500);
    let entry = manifest.find("1.20.1").unwrap();
    let cancel = CancelToken::new();
    installer
        .install_vanilla(entry, "1.20.1", &reporter(), &cancel)
        .await
        .unwrap();

    let resolved = installer.resolve("1.20.1").unwrap();
    let jv = resolved.json.java_version.clone().unwrap();
    assert_eq!(jv.component, "java-runtime-gamma");

    // 自动下载 Mojang 的 Java 17
    let java_path = java::install_runtime(
        &installer.downloader,
        &installer.game,
        &jv.component,
        &installer.env,
        &reporter(),
        &cancel,
    )
    .await
    .unwrap();
    let info = java::probe(&java_path).unwrap();
    assert_eq!(info.major, 17);

    // 再次补全应当全部跳过（快速模式）
    let started = std::time::Instant::now();
    let game_dir = installer.game.instance_game_dir("1.20.1", true);
    let opts = LaunchOptions {
        java: java_path,
        game_dir,
        auth: LaunchAuth::offline("RedCraftTest"),
        max_memory_mb: 2048,
        min_memory_mb: 512,
        java_major: info.major,
        gc: launch::GcMode::Auto,
        extra_jvm_args: vec![],
        extra_game_args: vec![],
        resolution: Some((854, 480)),
        fullscreen: false,
        server: None,
        launcher_name: launcher_core::LAUNCHER_ID.into(),
        launcher_version: launcher_core::LAUNCHER_VERSION.into(),
    };
    let cmd = launch::prepare(&installer, "1.20.1", &opts, &reporter(), &cancel)
        .await
        .unwrap();
    eprintln!("启动准备耗时 {:?}", started.elapsed());
    eprintln!("{}", cmd.display(&opts.auth.access_token));
    assert!(cmd.natives_dir.join("liblwjgl.so").exists() || !cfg!(target_os = "linux"));

    // 启动游戏，看到 LWJGL 初始化日志即视为启动成功
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let l2 = lines.clone();
    let game = launch::spawn(
        &cmd,
        Arc::new(move |l| {
            eprintln!("[游戏] {}", l.text);
            l2.lock().unwrap().push(l.text);
        }),
    )
    .unwrap();
    let pid = game.pid().unwrap();
    let waiter = tokio::spawn(game.wait());

    let ok = tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            let found = lines
                .lock()
                .unwrap()
                .iter()
                .any(|l| l.contains("Setting user: RedCraftTest") || l.contains("Backend library: LWJGL"));
            if found || waiter.is_finished() {
                return found;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .unwrap_or(false);
    let _ = launch::kill_pid(pid);
    let exit = waiter.await.unwrap().unwrap();
    eprintln!("游戏退出：{:?}，原因分析：{:?}", exit.code, exit.causes);
    assert!(ok, "没有在日志中看到游戏启动成功的标志");
}
