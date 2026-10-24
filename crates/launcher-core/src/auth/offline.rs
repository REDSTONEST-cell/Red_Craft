//! 离线登录。

use md5::{Digest, Md5};

use crate::error::{CoreError, Result};

/// 与服务端一致的离线 UUID：对 `OfflinePlayer:<name>` 做 MD5，并按 UUID v3 设置版本与变体位。
pub fn offline_uuid(name: &str) -> String {
    let mut bytes: [u8; 16] = Md5::digest(format!("OfflinePlayer:{name}").as_bytes()).into();
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let h = hex::encode(bytes);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// 检查离线用户名。正版规则是 3~16 位字母、数字、下划线；
/// 更宽松的名字在单机里可以用，但多人游戏可能会被服务器拒绝，这里只拦截确实无法使用的。
pub fn validate_offline_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(CoreError::InvalidData("用户名不能为空".into()));
    }
    if name.chars().count() > 16 {
        return Err(CoreError::InvalidData("用户名不能超过 16 个字符".into()));
    }
    if name.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(CoreError::InvalidData("用户名不能包含空格".into()));
    }
    Ok(())
}

/// 用户名是否符合正版规则（不符合时前端给出提醒）。
pub fn is_standard_name(name: &str) -> bool {
    (3..=16).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_offline_uuids() {
        // 与服务端 `UUID.nameUUIDFromBytes("OfflinePlayer:Notch")` 的结果一致
        assert_eq!(offline_uuid("Notch"), "b50ad385-829d-3141-a216-7e7d7539ba7f");
        assert_eq!(offline_uuid("Steve"), "5627dd98-e6be-3c21-b8a8-e92344183641");
        let u = offline_uuid("测试玩家");
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "3");
        assert!(matches!(&u[19..20], "8" | "9" | "a" | "b"));
    }

    #[test]
    fn names() {
        assert!(validate_offline_name("Steve").is_ok());
        assert!(validate_offline_name("").is_err());
        assert!(validate_offline_name("a b").is_err());
        assert!(validate_offline_name("12345678901234567").is_err());
        assert!(is_standard_name("Player_1"));
        assert!(!is_standard_name("玩家"));
        assert!(!is_standard_name("ab"));
    }
}
