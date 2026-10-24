//! 资源下载的联网测试（默认忽略）：
//!
//! ```sh
//! cargo test -p launcher-core --test content -- --ignored --nocapture
//! ```

use std::collections::HashSet;
use std::sync::Arc;

use launcher_core::content::{
    best_of, install_files, resolve, Client, ContentType, CurseForge, FilteredClient, Modrinth, SearchQuery, SortBy,
    VersionFilter,
};
use launcher_core::download::{CancelToken, DownloadSource, Downloader, HttpClient};
use launcher_core::install::Reporter;
use launcher_core::instance::LoaderKind;

fn http(source: DownloadSource) -> HttpClient {
    HttpClient::new(source).unwrap()
}

fn filter(loader: LoaderKind) -> VersionFilter {
    VersionFilter {
        game_version: Some("1.20.1".into()),
        loader: Some(loader),
        content_type: Some(ContentType::Mod),
    }
}

async fn plan_titles(client: &Client, project: &str, loader: LoaderKind) -> (Vec<String>, launcher_core::content::Plan) {
    let f = filter(loader);
    let root = best_of(client.versions(project, &f).await.unwrap()).expect("没有兼容版本");
    let src = FilteredClient { client, filter: f };
    let plan = resolve(&src, vec![root], &HashSet::new()).await.unwrap();
    let titles = plan.items.iter().map(|i| i.title.clone()).collect();
    (titles, plan)
}

#[tokio::test]
#[ignore = "需要网络"]
async fn modrinth_search_and_resolve() {
    let client = Client::Modrinth(Modrinth::new(http(DownloadSource::Auto)));
    let res = client
        .search(&SearchQuery {
            query: "sodium".into(),
            content_type: ContentType::Mod,
            game_version: Some("1.20.1".into()),
            loader: Some(LoaderKind::Fabric),
            sort: SortBy::Relevance,
            offset: 0,
            limit: 10,
        })
        .await
        .unwrap();
    assert!(res.total > 0 && res.hits.iter().any(|h| h.slug == "sodium"));

    let (titles, plan) = plan_titles(&client, "sodium-extra", LoaderKind::Fabric).await;
    eprintln!("Sodium Extra 将安装：{titles:?}，可选：{:?}", plan.optional.iter().map(|o| &o.title).collect::<Vec<_>>());
    assert!(titles.iter().any(|t| t.contains("Sodium") && !t.contains("Extra")), "{titles:?}");

    // 下载到临时文件夹，并校验安装记录
    let dir = tempfile::tempdir().unwrap();
    let items: Vec<_> = plan.items.iter().map(|i| (i.version.clone(), i.title.clone())).collect();
    let reporter = Reporter { stage: Arc::new(|_| {}), progress: Arc::new(|_| {}) };
    let files = install_files(&Downloader::new(http(DownloadSource::Auto), 16), dir.path(), &items, &reporter, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(files.len(), items.len());
    let installed =
        launcher_core::content::installed_projects(dir.path(), launcher_core::content::Platform::Modrinth, None).await;
    assert!(installed.contains(&plan.items[0].project_id));
}

#[tokio::test]
#[ignore = "需要网络"]
async fn modrinth_through_mirror() {
    let client = Client::Modrinth(Modrinth::new(http(DownloadSource::Bmclapi)));
    let res = client
        .search(&SearchQuery {
            query: "".into(),
            content_type: ContentType::Shader,
            game_version: None,
            loader: None,
            sort: SortBy::Downloads,
            offset: 0,
            limit: 5,
        })
        .await
        .unwrap();
    assert_eq!(res.hits.len(), 5);
    assert!(res.hits.iter().all(|h| h.content_type == ContentType::Shader));
}

#[tokio::test]
#[ignore = "需要网络"]
async fn curseforge_search_and_resolve() {
    let client = Client::CurseForge(CurseForge::new(http(DownloadSource::Auto), None));
    let res = client
        .search(&SearchQuery {
            query: "jei".into(),
            content_type: ContentType::Mod,
            game_version: Some("1.20.1".into()),
            loader: Some(LoaderKind::Forge),
            sort: SortBy::Relevance,
            offset: 0,
            limit: 10,
        })
        .await
        .unwrap();
    assert!(res.hits.iter().any(|h| h.id == "238222"), "{:?}", res.hits.iter().map(|h| &h.title).collect::<Vec<_>>());

    // Create: Steam 'n' Rails（688231）需要 Create（328085）
    let (titles, plan) = plan_titles(&client, "688231", LoaderKind::Forge).await;
    eprintln!("Steam 'n' Rails 将安装：{titles:?}");
    assert!(plan.items.iter().any(|i| i.project_id == "328085"), "{titles:?}");
}
