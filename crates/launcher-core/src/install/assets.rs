//! 资源索引与资源文件。

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::error::{CoreError, IoContext, Result};
use crate::paths::GameDir;

pub const RESOURCES_URL: &str = "https://resources.download.minecraft.net";

#[derive(Debug, Clone, Deserialize)]
pub struct AssetIndex {
    #[serde(default)]
    pub objects: HashMap<String, AssetObject>,
    /// 1.6 ~ 1.7.2 使用的虚拟目录（`legacy`）
    #[serde(default, rename = "virtual")]
    pub is_virtual: bool,
    /// 1.6 之前需要把资源复制到游戏目录的 `resources` 下（`pre-1.6`）
    #[serde(default)]
    pub map_to_resources: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

impl AssetObject {
    pub fn url(&self) -> String {
        format!("{RESOURCES_URL}/{}/{}", self.hash.get(..2).unwrap_or(&self.hash), self.hash)
    }

    /// 哈希是否为 40 位十六进制（同时保证它可以安全地用作文件名）。
    pub fn valid_hash(&self) -> bool {
        self.hash.len() == 40 && self.hash.bytes().all(|b| b.is_ascii_hexdigit())
    }
}

/// 资源索引中决定资源目录布局的两个标志（见 [`AssetIndex`]）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct AssetLayout {
    #[serde(default, rename = "virtual")]
    pub is_virtual: bool,
    #[serde(default)]
    pub map_to_resources: bool,
}

impl AssetLayout {
    /// 只读取布局标志：不为索引里的几千个资源条目分配内存（启动游戏时每次都要读）。
    pub fn read(path: &Path) -> Result<Self> {
        let data = std::fs::read(path).at(path)?;
        serde_json::from_slice(&data).map_err(|e| CoreError::json("资源索引", e))
    }
}

impl AssetIndex {
    pub fn read(path: &Path) -> Result<Self> {
        let data = std::fs::read(path).at(path)?;
        serde_json::from_slice(&data).map_err(|e| CoreError::json("资源索引", e))
    }

    /// 是否需要准备虚拟目录或 resources 目录。
    pub fn needs_copy(&self) -> bool {
        self.is_virtual || self.map_to_resources
    }

    /// 把资源从 objects 复制到按文件名组织的目录（阻塞调用）。已存在且大小一致的跳过。
    pub fn copy_to_named_dir(&self, game: &GameDir, target: &Path) -> Result<()> {
        for (name, obj) in &self.objects {
            // 防止索引中的路径逃逸出目标目录
            if name.split(['/', '\\']).any(|p| p == "..") {
                continue;
            }
            if !obj.valid_hash() {
                continue;
            }
            let dest = target.join(name);
            if std::fs::metadata(&dest).is_ok_and(|m| m.len() == obj.size) {
                continue;
            }
            let src = game.asset_object(&obj.hash);
            if !src.exists() {
                continue;
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).at(parent)?;
            }
            std::fs::copy(&src, &dest).at(&dest)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_legacy_index() {
        let idx: AssetIndex = serde_json::from_str(
            r#"{"virtual": true, "objects": {"sounds/a.ogg": {"hash": "abcdef0123", "size": 3}}}"#,
        )
        .unwrap();
        assert!(idx.is_virtual && !idx.map_to_resources && idx.needs_copy());
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("legacy.json");
        std::fs::write(&file, r#"{"objects": {"a": {"hash": "abcdef0123", "size": 3}}, "virtual": true}"#).unwrap();
        assert_eq!(AssetLayout::read(&file).unwrap(), AssetLayout { is_virtual: true, map_to_resources: false });
        std::fs::write(&file, r#"{"objects": {}}"#).unwrap();
        assert_eq!(AssetLayout::read(&file).unwrap(), AssetLayout::default());
        assert_eq!(
            idx.objects["sounds/a.ogg"].url(),
            "https://resources.download.minecraft.net/ab/abcdef0123"
        );
    }

    #[test]
    fn copy_virtual() {
        let dir = tempfile::tempdir().unwrap();
        let game = GameDir::new(dir.path());
        let hash = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let obj = game.asset_object(hash);
        std::fs::create_dir_all(obj.parent().unwrap()).unwrap();
        std::fs::write(&obj, b"hello").unwrap();
        let idx: AssetIndex = serde_json::from_str(&format!(
            r#"{{"map_to_resources": true, "objects": {{"a/b.txt": {{"hash": "{hash}", "size": 5}}, "../evil": {{"hash": "{hash}", "size": 5}}}}}}"#
        ))
        .unwrap();
        let target = dir.path().join("resources");
        idx.copy_to_named_dir(&game, &target).unwrap();
        assert_eq!(std::fs::read(target.join("a/b.txt")).unwrap(), b"hello");
        assert!(!dir.path().join("evil").exists());
    }
}
