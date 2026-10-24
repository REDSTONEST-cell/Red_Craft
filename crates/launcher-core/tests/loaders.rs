//! 加载器安装的联网集成测试（默认忽略）：
//!
//! ```sh
//! cargo test -p launcher-core --test loaders -- --ignored --nocapture --test-threads 1
//! ```
//!
//! 游戏目录默认为 `target/mc-test`，可用环境变量 `RC_TEST_GAME_DIR` 指定。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use launcher_core::auth::LaunchAuth;
use launcher_core::download::{CancelToken, DownloadSource, Downloader, HttpClient};
use launcher_core::install::{Installer, Reporter};
use launcher_core::instance::{detect_loader, LoaderKind};
use launcher_core::java;
use launcher_core::launch::{self, LaunchOptions};
use launcher_core::loader::{self, LoaderChoice};
use launcher_core::paths::GameDir;
use launcher_core::version::VersionManifest;

fn installer() -> Installer {
    let dir = std::env::var_os("RC_TEST_GAME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/mc-test"));
    let http = HttpClient::new(DownloadSource::Auto).unwrap();
    Installer::new(Downloader::new(http, 64), GameDir::new(dir))
}

fn reporter() -> Reporter {
    Reporter {
        stage: Arc::new(|s| eprintln!("[阶段] {s}")),
        progress: Arc::new(|_| {}),
    }
}

async fn java_for(installer: &Installer, component: &str) -> PathBuf {
    java::install_runtime(
        &installer.downloader,
        &installer.game,
        component,
        &installer.env,
        &reporter(),
        &CancelToken::new(),
    )
    .await
    .unwrap()
}

async fn install(kind: LoaderKind, name: &str) -> Installer {
    install_mc(kind, "1.20.1", "java-runtime-gamma", name).await
}

async fn install_mc(kind: LoaderKind, mc: &str, java_component: &str, name: &str) -> Installer {
    let installer = installer();
    let http = installer.downloader.http().clone();
    let manifest = VersionManifest::fetch(&http).await.unwrap();
    let entry = manifest.find(mc).unwrap().clone();

    let versions = loader::list_versions(&http, kind, mc).await.unwrap();
    assert!(!versions.is_empty(), "{kind:?} 没有 {mc} 的版本");
    let v = versions.iter().find(|v| v.stable).unwrap_or(&versions[0]).clone();
    eprintln!("安装 {kind:?} {}（{}）", v.display, v.version);

    let _ = std::fs::remove_dir_all(installer.game.version_dir(name));
    let java = java_for(&installer, java_component).await;
    loader::install_instance(
        &installer,
        &entry,
        Some(&LoaderChoice { kind, version: v.version.clone() }),
        name,
        Some(&java),
        &reporter(),
        &CancelToken::new(),
    )
    .await
    .unwrap();

    let resolved = installer.resolve(name).unwrap();
    let detected = detect_loader(&resolved).expect("应识别出加载器");
    assert_eq!(detected.kind, kind);
    assert_eq!(resolved.jar_id, mc);
    installer
}

/// 启动游戏，直到日志中出现加载器与 LWJGL 初始化的标志。
async fn launch_until_started(installer: &Installer, name: &str, marker: &str) {
    launch_with(installer, name, marker, "java-runtime-gamma").await
}

async fn launch_with(installer: &Installer, name: &str, marker: &str, java_component: &str) {
    let java = java_for(installer, java_component).await;
    let opts = LaunchOptions {
        java,
        game_dir: installer.game.version_dir(name),
        auth: LaunchAuth::offline("RedCraftTest"),
        max_memory_mb: 3072,
        min_memory_mb: 512,
        java_major: 17,
        extra_jvm_args: vec![],
        extra_game_args: vec![],
        resolution: None,
        fullscreen: false,
        server: None,
        launcher_name: launcher_core::LAUNCHER_ID.into(),
        launcher_version: launcher_core::LAUNCHER_VERSION.into(),
    };
    let cmd = launch::prepare(installer, name, &opts, &reporter(), &CancelToken::new()).await.unwrap();
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let l2 = lines.clone();
    let game = launch::spawn(&cmd, Arc::new(move |l| l2.lock().unwrap().push(l.text))).unwrap();
    let pid = game.pid().unwrap();
    let waiter = tokio::spawn(game.wait());
    let (mut saw_marker, mut saw_lwjgl) = (false, false);
    let _ = tokio::time::timeout(Duration::from_secs(240), async {
        loop {
            {
                let l = lines.lock().unwrap();
                saw_marker = l.iter().any(|x| x.contains(marker));
                saw_lwjgl = l
                    .iter()
                    .any(|x| x.contains("Backend library: LWJGL") || x.contains("LWJGL Version") || x.contains("Setting user:"));
            }
            if (saw_marker && saw_lwjgl) || waiter.is_finished() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    })
    .await;
    let _ = launch::kill_pid(pid);
    let exit = waiter.await.unwrap().unwrap();
    if !(saw_marker && saw_lwjgl) {
        for l in lines.lock().unwrap().iter().rev().take(60).rev() {
            eprintln!("[游戏] {l}");
        }
        panic!("没有看到启动成功的标志（加载器 {saw_marker}，LWJGL {saw_lwjgl}），退出：{:?}", exit.code);
    }
}

#[tokio::test]
#[ignore = "需要网络"]
async fn fabric_1_20_1() {
    let installer = install(LoaderKind::Fabric, "test-fabric-1.20.1").await;
    launch_until_started(&installer, "test-fabric-1.20.1", "Loading Minecraft 1.20.1 with Fabric Loader").await;
}

#[tokio::test]
#[ignore = "需要网络"]
async fn quilt_1_20_1() {
    let installer = install(LoaderKind::Quilt, "test-quilt-1.20.1").await;
    launch_until_started(&installer, "test-quilt-1.20.1", "Quilt Loader").await;
}

#[tokio::test]
#[ignore = "需要网络"]
async fn forge_1_20_1() {
    let installer = install(LoaderKind::Forge, "test-forge-1.20.1").await;
    launch_until_started(&installer, "test-forge-1.20.1", "ModLauncher running").await;
}

#[tokio::test]
#[ignore = "需要网络"]
async fn neoforge_1_20_1() {
    let installer = install(LoaderKind::NeoForge, "test-neoforge-1.20.1").await;
    launch_until_started(&installer, "test-neoforge-1.20.1", "ModLauncher running").await;
}

#[tokio::test]
#[ignore = "需要网络"]
async fn forge_1_12_2() {
    let installer = install_mc(LoaderKind::Forge, "1.12.2", "jre-legacy", "test-forge-1.12.2").await;
    launch_with(&installer, "test-forge-1.12.2", "FMLTweaker", "jre-legacy").await;
}

#[tokio::test]
#[ignore = "需要网络"]
async fn forge_1_7_10() {
    let installer = install_mc(LoaderKind::Forge, "1.7.10", "jre-legacy", "test-forge-1.7.10").await;
    launch_with(&installer, "test-forge-1.7.10", "FMLTweaker", "jre-legacy").await;
}
