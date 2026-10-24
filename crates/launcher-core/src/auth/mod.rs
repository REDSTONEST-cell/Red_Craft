//! 账号与登录。

pub mod microsoft;
pub mod yggdrasil;
mod offline;

use serde::{Deserialize, Serialize};

pub use offline::{is_standard_name, lookup_skin, offline_uuid, validate_offline_name};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AccountKind {
    Offline,
    Microsoft,
    Yggdrasil {
        /// 认证服务器 API 根地址
        server: String,
        #[serde(default, alias = "server_name")]
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
    /// 正版账号的皮肤地址，用于显示头像
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skin: Option<String>,
}

impl Account {
    pub fn offline(name: &str) -> Self {
        let uuid = offline_uuid(name);
        Account {
            id: format!("offline-{uuid}"),
            kind: AccountKind::Offline,
            name: name.to_string(),
            uuid,
            skin: None,
        }
    }

    pub fn microsoft(profile: &microsoft::McProfile) -> Self {
        Account {
            id: format!("ms-{}", profile.id),
            kind: AccountKind::Microsoft,
            name: profile.name.clone(),
            uuid: profile.id.clone(),
            skin: profile.skin.clone(),
        }
    }

    /// 外置登录账号：id 由服务器地址与角色 UUID 共同决定，同一角色在不同服务器上互不冲突。
    pub fn yggdrasil(server: &yggdrasil::YggServer, profile: &yggdrasil::YggProfile) -> Self {
        let host = crate::download::sha1_bytes(server.api_root.as_bytes());
        Account {
            id: format!("ygg-{}-{}", &host[..8], profile.id),
            kind: AccountKind::Yggdrasil {
                server: server.api_root.clone(),
                server_name: server.name.clone(),
            },
            name: profile.name.clone(),
            uuid: profile.id.clone(),
            skin: None,
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
    pub fn microsoft(session: &microsoft::MsSession) -> Self {
        LaunchAuth {
            name: session.profile.name.clone(),
            uuid: session.profile.id.replace('-', ""),
            access_token: session.mc_access_token.clone(),
            user_type: "msa".into(),
            xuid: if session.xuid.is_empty() { "0".into() } else { session.xuid.clone() },
            authlib: None,
        }
    }

    pub fn yggdrasil(session: &yggdrasil::YggSession, profile: &yggdrasil::YggProfile, authlib: AuthlibArgs) -> Self {
        LaunchAuth {
            name: profile.name.clone(),
            uuid: profile.id.replace('-', ""),
            access_token: session.access_token.clone(),
            user_type: "mojang".into(),
            xuid: "0".into(),
            authlib: Some(authlib),
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yggdrasil_kind_json() {
        let kind = AccountKind::Yggdrasil {
            server: "https://a.com/api/".into(),
            server_name: "A".into(),
        };
        let v = serde_json::to_value(&kind).unwrap();
        assert_eq!(v["type"], "yggdrasil");
        assert_eq!(v["serverName"], "A");
        // 兼容旧的字段名
        let old: AccountKind =
            serde_json::from_str(r#"{"type":"yggdrasil","server":"s","server_name":"B"}"#).unwrap();
        assert!(matches!(old, AccountKind::Yggdrasil { server_name, .. } if server_name == "B"));
    }
}
