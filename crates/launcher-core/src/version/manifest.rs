//! 官方版本清单。

use serde::{Deserialize, Serialize};

use crate::download::HttpClient;
use crate::error::Result;

pub const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifest {
    pub latest: Latest,
    pub versions: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Latest {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    pub id: String,
    /// `release` / `snapshot` / `old_beta` / `old_alpha`
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub time: String,
    #[serde(default)]
    pub release_time: String,
    #[serde(default)]
    pub sha1: String,
}

impl ManifestEntry {
    pub fn is_old(&self) -> bool {
        self.kind.starts_with("old_")
    }
}

impl VersionManifest {
    pub async fn fetch(http: &HttpClient) -> Result<Self> {
        http.get_json(MANIFEST_URL, "版本清单").await
    }

    pub fn find(&self, id: &str) -> Option<&ManifestEntry> {
        self.versions.iter().find(|v| v.id == id)
    }
}
