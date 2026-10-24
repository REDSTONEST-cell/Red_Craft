//! 微软登录的联网测试（默认忽略）。需要设置 REDCRAFT_MS_CLIENT_ID：
//!
//! ```sh
//! REDCRAFT_MS_CLIENT_ID=<client id> cargo test -p launcher-core --test microsoft -- --ignored --nocapture
//! ```
//!
//! 只验证第一步（申请设备码），可用于检查 Azure 应用配置是否正确。

use launcher_core::auth::microsoft;

#[tokio::test]
#[ignore = "需要网络与 Azure Client ID"]
async fn device_code_request() {
    let client_id = std::env::var("REDCRAFT_MS_CLIENT_ID").expect("请设置 REDCRAFT_MS_CLIENT_ID");
    let http = reqwest::Client::new();
    let code = microsoft::start_device_flow(&http, &client_id).await.unwrap();
    eprintln!("验证码 {}，网址 {}，有效期 {} 秒", code.user_code, code.verification_uri, code.expires_in);
    assert!(!code.user_code.is_empty());
    assert!(code.verification_uri.starts_with("https://"));
}
