//! 账号与登录。

mod offline;

use serde::{Deserialize, Serialize};

pub use offline::{is_standard_name, offline_uuid, validate_offline_name};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AccountKind {
    Offline,
    Microsoft,
    Yggdrasil {
        /// 认证服务器 API 根地址
        server: String,
        #[serde(default)]
        server_name: String,
    },
}

/// 保存在配置文件中的账号信息（不含任何令牌）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// 本地唯一 id
    pub id: String,
    pub kind: AccountKind,
    pub name: String,
    /// 不带连字符或带连字符均可，启动时统一处理
    pub uuid: String,
}

impl Account {
    pub fn offline(name: &str) -> Self {
        let uuid = offline_uuid(name);
        Account {
            id: format!("offline-{uuid}"),
            kind: AccountKind::Offline,
            name: name.to_string(),
            uuid,
        }
    }
}

/// 启动游戏时使用的登录凭据。
#[derive(Debug, Clone)]
pub struct LaunchAuth {
    pub name: String,
    /// 不带连字符的 UUID
    pub uuid: String,
    pub access_token: String,
    /// `msa` / `mojang` / `legacy`
    pub user_type: String,
    pub xuid: String,
    /// 外置登录：authlib-injector 参数（jar 路径、服务器地址、预取元数据）
    pub authlib: Option<AuthlibArgs>,
}

#[derive(Debug, Clone)]
pub struct AuthlibArgs {
    pub jar: std::path::PathBuf,
    pub server: String,
    pub prefetched: String,
}

impl LaunchAuth {
    pub fn offline(name: &str) -> Self {
        LaunchAuth {
            name: name.to_string(),
            uuid: offline_uuid(name).replace('-', ""),
            // 离线模式下令牌只是占位，不能为空，否则部分版本会报错
            access_token: "0".repeat(32),
            user_type: "legacy".into(),
            xuid: "0".into(),
            authlib: None,
        }
    }
}
