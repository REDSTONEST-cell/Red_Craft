//! 下载源：官方源与 BMCLAPI 镜像之间的 URL 映射。

use serde::{Deserialize, Serialize};

pub const BMCLAPI: &str = "https://bmclapi2.bangbang93.com";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadSource {
    /// 优先官方源，失败后回退 BMCLAPI。
    #[default]
    Auto,
    /// 只使用官方源。
    Official,
    /// 优先 BMCLAPI，失败后回退官方源。
    Bmclapi,
}

/// 官方地址前缀 → BMCLAPI 地址前缀。
const MIRRORS: &[(&str, &str)] = &[
    ("https://piston-meta.mojang.com", ""),
    ("https://piston-data.mojang.com", ""),
    ("https://launchermeta.mojang.com", ""),
    ("https://launcher.mojang.com", ""),
    ("https://resources.download.minecraft.net", "/assets"),
    ("https://libraries.minecraft.net", "/maven"),
    ("https://maven.minecraftforge.net", "/maven"),
    ("https://files.minecraftforge.net/maven", "/maven"),
    ("https://maven.neoforged.net/releases", "/maven"),
    ("https://maven.fabricmc.net", "/maven"),
    ("https://meta.fabricmc.net", "/fabric-meta"),
    ("https://maven.quiltmc.org/repository/release", "/maven"),
    ("https://authlib-injector.yushi.moe", "/mirrors/authlib-injector"),
];

/// 返回 URL 对应的 BMCLAPI 镜像地址；没有镜像时返回 `None`。
pub fn mirror_of(url: &str) -> Option<String> {
    MIRRORS.iter().find_map(|(official, path)| {
        url.strip_prefix(official)
            .filter(|rest| rest.is_empty() || rest.starts_with('/'))
            .map(|rest| format!("{BMCLAPI}{path}{rest}"))
    })
}

impl DownloadSource {
    /// 按尝试顺序返回候选 URL。
    pub fn candidates(self, url: &str) -> Vec<String> {
        let mirror = mirror_of(url);
        match (self, mirror) {
            (DownloadSource::Official, _) | (_, None) => vec![url.to_string()],
            (DownloadSource::Auto, Some(m)) => vec![url.to_string(), m],
            (DownloadSource::Bmclapi, Some(m)) => vec![m, url.to_string()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_mapping() {
        assert_eq!(
            mirror_of("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json").unwrap(),
            "https://bmclapi2.bangbang93.com/mc/game/version_manifest_v2.json"
        );
        assert_eq!(
            mirror_of("https://resources.download.minecraft.net/ab/abcdef").unwrap(),
            "https://bmclapi2.bangbang93.com/assets/ab/abcdef"
        );
        assert_eq!(
            mirror_of("https://libraries.minecraft.net/a/b/c.jar").unwrap(),
            "https://bmclapi2.bangbang93.com/maven/a/b/c.jar"
        );
        assert!(mirror_of("https://api.modrinth.com/v2/search").is_none());
        // 前缀必须在路径边界上匹配
        assert!(mirror_of("https://launcher.mojang.comevil.net/x").is_none());
    }

    #[test]
    fn candidate_order() {
        let url = "https://libraries.minecraft.net/x.jar";
        assert_eq!(DownloadSource::Official.candidates(url), vec![url.to_string()]);
        assert_eq!(DownloadSource::Auto.candidates(url)[0], url);
        assert!(DownloadSource::Bmclapi.candidates(url)[0].starts_with(BMCLAPI));
    }
}
