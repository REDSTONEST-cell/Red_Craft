//! 离线账号同名正版皮肤查询（联网，默认忽略）。

#[tokio::test]
#[ignore = "需要网络"]
async fn lookup_notch_skin() {
    let http = reqwest::Client::new();
    let skin = launcher_core::auth::lookup_skin(&http, "Notch").await;
    eprintln!("Notch 的皮肤：{skin:?}");
    assert!(skin.unwrap().starts_with("https://textures.minecraft.net/texture/"));
    // 不存在的正版名
    assert!(launcher_core::auth::lookup_skin(&http, "zz_no_such_player_x").await.is_none());
}
