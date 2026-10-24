//! 游戏目录（`.minecraft`）的布局。

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct GameDir {
    root: PathBuf,
}

impl GameDir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        GameDir { root: root.into() }
    }

    /// 各平台官方启动器使用的默认 `.minecraft` 位置。
    pub fn default_location() -> PathBuf {
        if cfg!(target_os = "windows") {
            dirs::data_dir()
                .map(|d| d.join(".minecraft"))
                .unwrap_or_else(|| PathBuf::from(".minecraft"))
        } else if cfg!(target_os = "macos") {
            dirs::data_dir()
                .map(|d| d.join("minecraft"))
                .unwrap_or_else(|| PathBuf::from("minecraft"))
        } else {
            dirs::home_dir()
                .map(|d| d.join(".minecraft"))
                .unwrap_or_else(|| PathBuf::from(".minecraft"))
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn versions(&self) -> PathBuf {
        self.root.join("versions")
    }

    pub fn version_dir(&self, id: &str) -> PathBuf {
        self.versions().join(id)
    }

    pub fn version_json(&self, id: &str) -> PathBuf {
        self.version_dir(id).join(format!("{id}.json"))
    }

    pub fn version_jar(&self, id: &str) -> PathBuf {
        self.version_dir(id).join(format!("{id}.jar"))
    }

    pub fn natives_dir(&self, id: &str) -> PathBuf {
        self.version_dir(id).join("natives")
    }

    pub fn libraries(&self) -> PathBuf {
        self.root.join("libraries")
    }

    pub fn library(&self, rel: &str) -> PathBuf {
        self.libraries().join(rel)
    }

    pub fn assets(&self) -> PathBuf {
        self.root.join("assets")
    }

    pub fn asset_index(&self, id: &str) -> PathBuf {
        self.assets().join("indexes").join(format!("{id}.json"))
    }

    pub fn asset_object(&self, hash: &str) -> PathBuf {
        self.assets().join("objects").join(&hash[..2]).join(hash)
    }

    /// 1.7.2 之前的「虚拟」资源目录。
    pub fn asset_virtual(&self, index_id: &str) -> PathBuf {
        self.assets().join("virtual").join(index_id)
    }

    pub fn log_configs(&self) -> PathBuf {
        self.assets().join("log_configs")
    }

    pub fn runtime(&self) -> PathBuf {
        self.root.join("runtime")
    }

    /// 实例的游戏运行目录：开启版本隔离时为 `versions/<id>`，否则为根目录。
    pub fn instance_game_dir(&self, id: &str, isolated: bool) -> PathBuf {
        if isolated {
            self.version_dir(id)
        } else {
            self.root.clone()
        }
    }
}

/// 把 Maven 坐标 `group:artifact:version[:classifier][@ext]` 转为相对路径。
pub fn maven_path(name: &str) -> Option<String> {
    let (coord, ext) = match name.split_once('@') {
        Some((c, e)) => (c, e),
        None => (name, "jar"),
    };
    let parts: Vec<&str> = coord.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    let file = match parts.get(3) {
        Some(classifier) => format!("{artifact}-{version}-{classifier}.{ext}"),
        None => format!("{artifact}-{version}.{ext}"),
    };
    Some(format!("{group}/{artifact}/{version}/{file}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maven_paths() {
        assert_eq!(
            maven_path("org.ow2.asm:asm:9.6").unwrap(),
            "org/ow2/asm/asm/9.6/asm-9.6.jar"
        );
        assert_eq!(
            maven_path("org.lwjgl:lwjgl:3.3.1:natives-windows").unwrap(),
            "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar"
        );
        assert_eq!(
            maven_path("de.oceanlabs.mcp:mcp_config:1.20.1@zip").unwrap(),
            "de/oceanlabs/mcp/mcp_config/1.20.1/mcp_config-1.20.1.zip"
        );
        assert!(maven_path("bad").is_none());
    }
}
