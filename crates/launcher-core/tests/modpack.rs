//! 整合包安装的联网集成测试（默认忽略）：
//!
//! ```sh
//! xvfb-run -a cargo test -p launcher-core --test modpack -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use launcher_core::content::{self, best_of, Client, ContentType, CurseForge, Modrinth, VersionFilter};
use launcher_core::download::{CancelToken, DownloadSource, DownloadTask, Downloader, HttpClient};
use launcher_core::install::{Installer, Reporter};
use launcher_core::loader::{self, LoaderChoice};
use launcher_core::paths::GameDir;
use launcher_core::version::VersionManifest;

#[tokio::test]
#[ignore = "需要网络"]
async fn fabulously_optimized_1_20_1() {
    let dir = std::env::var_os("RC_TEST_GAME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/mc-test"));
    let http = HttpClient::new(DownloadSource::Auto).unwrap();
    let installer = Installer::new(Downloader::new(http.clone(), 64), GameDir::new(dir));
    let reporter = Reporter {
        stage: Arc::new(|s| eprintln!("[阶段] {s}")),
        progress: Arc::new(|_| {}),
    };
    let cancel = CancelToken::new();

    // 1. 下载整合包
    let client = Client::Modrinth(Modrinth::new(http.clone()));
    let filter = VersionFilter {
        game_version: Some("1.20.1".into()),
        loader: None,
        content_type: Some(ContentType::Modpack),
    };
    let version = best_of(client.versions("fabulously-optimized", &filter).await.unwrap()).unwrap();
    let file = version.primary_file().unwrap().clone();
    let pack = installer.game.root().join(".redcraft-tmp").join(&file.filename);
    installer
        .downloader
        .download_all(
            vec![DownloadTask::new(&file.url, &pack).sha1(file.sha1.clone())],
            reporter.progress.clone(),
            &cancel,
        )
        .await
        .unwrap();

    // 2. 解析
    let info = content::read_modpack(&pack).unwrap();
    eprintln!("整合包 {} {}：Minecraft {}，加载器 {:?}，{} 个文件", info.name, info.version, info.mc_version, info.loader, info.file_count);
    assert_eq!(info.mc_version, "1.20.1");
    let l = info.loader.clone().unwrap();

    // 3. 安装游戏与加载器
    let name = "test-modpack-fo";
    let _ = std::fs::remove_dir_all(installer.game.version_dir(name));
    let manifest = VersionManifest::fetch(&http).await.unwrap();
    let entry = manifest.find("1.20.1").unwrap().clone();
    loader::install_instance(
        &installer,
        &entry,
        Some(&LoaderChoice { kind: l.kind, version: l.version }),
        name,
        None,
        &reporter,
        &cancel,
    )
    .await
    .unwrap();

    // 4. 文件与 overrides
    let game_dir = installer.game.version_dir(name);
    let cf = CurseForge::new(http.clone(), None);
    content::download_pack_files(&info, &game_dir, &installer.downloader, &cf, &reporter, &cancel)
        .await
        .unwrap();
    let n = content::extract_overrides(&pack, &info, &game_dir).unwrap();
    let mods = std::fs::read_dir(game_dir.join("mods")).unwrap().count();
    eprintln!("mods 文件夹中有 {mods} 个文件，解压了 {n} 个配置文件");
    assert!(mods >= info.file_count / 2);
    assert!(n > 0);

    // 5. 启动，直到 Fabric 加载完全部 Mod 并初始化 LWJGL
    let java = launcher_core::java::install_runtime(
        &installer.downloader,
        &installer.game,
        "java-runtime-gamma",
        &installer.env,
        &reporter,
        &cancel,
    )
    .await
    .unwrap();
    let opts = launcher_core::launch::LaunchOptions {
        java,
        game_dir: game_dir.clone(),
        auth: launcher_core::auth::LaunchAuth::offline("RedCraftTest"),
        max_memory_mb: 4096,
        min_memory_mb: 512,
        java_major: 17,
        gc: launcher_core::launch::GcMode::Auto,
        extra_jvm_args: vec![],
        extra_game_args: vec![],
        resolution: None,
        fullscreen: false,
        server: None,
        launcher_name: launcher_core::LAUNCHER_ID.into(),
        launcher_version: launcher_core::LAUNCHER_VERSION.into(),
    };
    let cmd = launcher_core::launch::prepare(&installer, name, &opts, &reporter, &cancel).await.unwrap();
    let lines = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let l2 = lines.clone();
    let game = launcher_core::launch::spawn(&cmd, Arc::new(move |l| l2.lock().unwrap().push(l.text))).unwrap();
    let pid = game.pid().unwrap();
    let waiter = tokio::spawn(game.wait());
    let mut ok = false;
    for _ in 0..600 {
        {
            let l = lines.lock().unwrap();
            let loaded = l.iter().find(|x| x.contains("Loading") && x.contains("mods:"));
            if let Some(line) = loaded {
                if l.iter().any(|x| x.contains("Backend library: LWJGL")) {
                    eprintln!("{line}");
                    ok = true;
                }
            }
        }
        if ok || waiter.is_finished() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    let _ = launcher_core::launch::kill_pid(pid);
    let _ = waiter.await;
    if !ok {
        for l in lines.lock().unwrap().iter().rev().take(40).rev() {
            eprintln!("[游戏] {l}");
        }
    }
    assert!(ok, "整合包没有成功启动");
}
