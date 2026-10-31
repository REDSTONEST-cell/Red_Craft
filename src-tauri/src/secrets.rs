//! 敏感信息（令牌）存储：优先使用系统密钥库（Windows 凭据管理器 / macOS 钥匙串 / Linux Secret Service）。
//!
//! Windows 凭据管理器单条最多约 2.5KB，微软的 refresh token 加上 Minecraft 令牌可能超过，
//! 因此按固定长度分片保存。系统密钥库不可用时（如没有 Secret Service 的 Linux 桌面），
//! 退回到配置目录下仅当前用户可读的文件，并在日志中提示。
//! 手机上没有可用的系统密钥库，直接保存在应用私有目录中（其他应用无法读取）。

use std::path::PathBuf;

use crate::error::{AppError, AppResult};

#[cfg(desktop)]
const SERVICE: &str = "RedCraft";
#[cfg(desktop)]
const CHUNK: usize = 1000;

pub struct Secrets {
    fallback_dir: PathBuf,
}

impl Secrets {
    pub fn new(config_dir: &std::path::Path) -> Self {
        Secrets {
            fallback_dir: config_dir.join("secrets"),
        }
    }

    pub fn set(&self, key: &str, value: &str) -> AppResult<()> {
        match keyring_set(key, value) {
            Ok(()) => {
                let _ = std::fs::remove_file(self.file(key));
                Ok(())
            }
            Err(e) => {
                log::warn!("系统密钥库不可用（{e}），令牌改为保存在本地文件");
                self.file_set(key, value)
            }
        }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        keyring_get(key).or_else(|| std::fs::read_to_string(self.file(key)).ok())
    }

    pub fn delete(&self, key: &str) {
        keyring_delete(key);
        let _ = std::fs::remove_file(self.file(key));
    }

    fn file(&self, key: &str) -> PathBuf {
        let safe: String = key
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
            .collect();
        self.fallback_dir.join(format!("{safe}.secret"))
    }

    fn file_set(&self, key: &str, value: &str) -> AppResult<()> {
        let path = self.file(key);
        std::fs::create_dir_all(&self.fallback_dir).map_err(|e| launcher_core::CoreError::io(&self.fallback_dir, e))?;
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .map_err(|e| launcher_core::CoreError::io(&path, e))?;
            f.write_all(value.as_bytes())
                .map_err(|e| launcher_core::CoreError::io(&path, e))?;
        }
        #[cfg(not(unix))]
        std::fs::write(&path, value).map_err(|e| launcher_core::CoreError::io(&path, e))?;
        Ok(())
    }
}

#[cfg(mobile)]
fn keyring_set(_key: &str, _value: &str) -> Result<(), AppError> {
    Err(AppError::msg("手机上不使用系统密钥库", ""))
}

#[cfg(mobile)]
fn keyring_get(_key: &str) -> Option<String> {
    None
}

#[cfg(mobile)]
fn keyring_delete(_key: &str) {}

#[cfg(desktop)]
fn entry(key: &str) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, key)
}

#[cfg(desktop)]
fn keyring_set(key: &str, value: &str) -> Result<(), AppError> {
    let map = |e: keyring::Error| AppError::msg(format!("写入系统密钥库失败：{e}"), "");
    keyring_delete(key);
    let chars: Vec<char> = value.chars().collect();
    let chunks: Vec<String> = chars.chunks(CHUNK).map(|c| c.iter().collect()).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        entry(&format!("{key}#{i}")).and_then(|e| e.set_password(chunk)).map_err(map)?;
    }
    // 头部记录分片数量，最后写入，保证读取时数据完整
    entry(key)
        .and_then(|e| e.set_password(&chunks.len().to_string()))
        .map_err(map)?;
    // 确认能读回（部分 Linux 环境写入不报错但实际没有保存）
    match keyring_get(key) {
        Some(v) if v == value => Ok(()),
        _ => Err(AppError::msg("系统密钥库无法读回刚写入的数据", "")),
    }
}

#[cfg(desktop)]
fn keyring_get(key: &str) -> Option<String> {
    let count: usize = entry(key).ok()?.get_password().ok()?.parse().ok()?;
    let mut out = String::new();
    for i in 0..count {
        out.push_str(&entry(&format!("{key}#{i}")).ok()?.get_password().ok()?);
    }
    Some(out)
}

#[cfg(desktop)]
fn keyring_delete(key: &str) {
    let count: usize = entry(key)
        .ok()
        .and_then(|e| e.get_password().ok())
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    for i in 0..count {
        if let Ok(e) = entry(&format!("{key}#{i}")) {
            let _ = e.delete_credential();
        }
    }
    if let Ok(e) = entry(key) {
        let _ = e.delete_credential();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_long_value() {
        // 测试环境通常没有 Secret Service，会走文件后备；两种路径的读写接口一致
        let dir = tempfile::tempdir().unwrap();
        let s = Secrets::new(dir.path());
        let key = format!("test-{}", std::process::id());
        let value = "令牌".repeat(1500);
        s.set(&key, &value).unwrap();
        assert_eq!(s.get(&key).as_deref(), Some(value.as_str()));
        s.delete(&key);
        assert!(s.get(&key).is_none());
    }
}
