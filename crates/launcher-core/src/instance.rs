//! 扫描 `versions` 目录中的实例，并识别加载器。

use serde::Serialize;

use crate::paths::GameDir;
use crate::version::{resolve_version, ResolvedVersion};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LoaderKind {
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}

impl LoaderKind {
    pub fn display_name(self) -> &'static str {
        match self {
            LoaderKind::Fabric => "Fabric",
            LoaderKind::Quilt => "Quilt",
            LoaderKind::Forge => "Forge",
            LoaderKind::NeoForge => "NeoForge",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderInfo {
    pub kind: LoaderKind,
    pub version: String,
}

/// 根据库列表与主类识别加载器。
pub fn detect_loader(resolved: &ResolvedVersion) -> Option<LoaderInfo> {
    for lib in &resolved.json.libraries {
        let Some(c) = lib.coord() else { continue };
        let found = match (c.group, c.artifact) {
            ("net.fabricmc", "fabric-loader") => Some((LoaderKind::Fabric, c.version.to_string())),
            ("org.quiltmc", "quilt-loader") => Some((LoaderKind::Quilt, c.version.to_string())),
            ("net.neoforged", "neoforge") => Some((LoaderKind::NeoForge, c.version.to_string())),
            // 1.20.1 的 NeoForge 沿用 forge 坐标
            ("net.neoforged", "forge") => Some((
                LoaderKind::NeoForge,
                c.version.split_once('-').map(|x| x.1).unwrap_or(c.version).to_string(),
            )),
            ("net.minecraftforge", "forge" | "fmlloader") => Some((
                LoaderKind::Forge,
                // 形如 1.20.1-47.2.0 或 1.7.10-10.13.4.1614-1.7.10
                c.version.split('-').nth(1).unwrap_or(c.version).to_string(),
            )),
            _ => None,
        };
        if let Some((kind, version)) = found {
            return Some(LoaderInfo { kind, version });
        }
    }
    let main = resolved.json.main_class.as_str();
    if main.contains("fabricmc") {
        return Some(LoaderInfo { kind: LoaderKind::Fabric, version: String::new() });
    }
    if main.contains("quiltmc") {
        return Some(LoaderInfo { kind: LoaderKind::Quilt, version: String::new() });
    }
    if main.contains("cpw.mods") || resolved.json.minecraft_arguments.as_deref().is_some_and(|a| a.contains("FMLTweaker")) {
        return Some(LoaderInfo { kind: LoaderKind::Forge, version: String::new() });
    }
    None
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSummary {
    pub id: String,
    /// Minecraft 版本（继承链最底层的 id）
    pub mc_version: String,
    /// `release` / `snapshot` / `old_beta` / `old_alpha`
    pub kind: String,
    pub loader: Option<LoaderInfo>,
    pub release_time: String,
    /// 版本文件无法解析时的原因
    pub error: Option<String>,
}

impl InstanceSummary {
    pub fn is_release(&self) -> bool {
        self.kind == "release"
    }
}

/// 扫描所有实例（阻塞调用）。
pub fn scan_instances(game: &GameDir) -> Vec<InstanceSummary> {
    let Ok(rd) = std::fs::read_dir(game.versions()) else {
        return Vec::new();
    };
    let mut out: Vec<InstanceSummary> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let id = e.file_name().to_string_lossy().to_string();
            game.version_json(&id).is_file().then_some(id)
        })
        .map(|id| match resolve_version(game, &id) {
            Ok(r) => InstanceSummary {
                mc_version: r.chain.last().cloned().unwrap_or_else(|| id.clone()),
                kind: r.json.kind.clone(),
                loader: detect_loader(&r),
                release_time: r.json.release_time.clone(),
                error: None,
                id,
            },
            Err(e) => InstanceSummary {
                mc_version: String::new(),
                kind: String::new(),
                loader: None,
                release_time: String::new(),
                error: Some(e.to_string()),
                id,
            },
        })
        .collect();
    out.sort_by(|a, b| b.release_time.cmp(&a.release_time).then_with(|| a.id.cmp(&b.id)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::{resolve_with, VersionJson};

    fn resolved(json: &str) -> ResolvedVersion {
        let v: VersionJson = serde_json::from_str(json).unwrap();
        resolve_with(&v.id.clone(), |_| Ok(v.clone())).unwrap()
    }

    #[test]
    fn loaders() {
        let r = resolved(r#"{"id":"a","libraries":[{"name":"net.fabricmc:fabric-loader:0.15.11"}]}"#);
        assert_eq!(detect_loader(&r), Some(LoaderInfo { kind: LoaderKind::Fabric, version: "0.15.11".into() }));
        let r = resolved(r#"{"id":"a","libraries":[{"name":"net.minecraftforge:forge:1.20.1-47.2.0:universal"}]}"#);
        assert_eq!(detect_loader(&r).unwrap().version, "47.2.0");
        let r = resolved(r#"{"id":"a","libraries":[{"name":"net.neoforged:neoforge:21.1.65:universal"}]}"#);
        assert_eq!(detect_loader(&r).unwrap().kind, LoaderKind::NeoForge);
        let r = resolved(r#"{"id":"a","mainClass":"net.minecraft.client.main.Main"}"#);
        assert_eq!(detect_loader(&r), None);
    }

    #[test]
    fn scan() {
        let dir = tempfile::tempdir().unwrap();
        let game = GameDir::new(dir.path());
        std::fs::create_dir_all(game.version_dir("1.20.1")).unwrap();
        std::fs::write(game.version_json("1.20.1"), r#"{"id":"1.20.1","type":"release","releaseTime":"2023"}"#).unwrap();
        std::fs::create_dir_all(game.version_dir("broken")).unwrap();
        std::fs::write(game.version_json("broken"), "{").unwrap();
        std::fs::create_dir_all(game.version_dir("empty")).unwrap();
        let list = scan_instances(&game);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "1.20.1");
        assert!(list[0].is_release());
        assert!(list[1].error.is_some());
    }
}
