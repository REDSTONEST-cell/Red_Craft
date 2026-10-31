use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const SCAN_REVISION: u32 = 2;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CropRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl CropRect {
    pub fn pixels(self, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
        if width == 0 || height == 0
            || ![self.x, self.y, self.width, self.height].iter().all(|n| n.is_finite())
            || self.x < 0.0 || self.y < 0.0 || self.width <= 0.0 || self.height <= 0.0
            || self.x + self.width > 1.0 + 1e-9 || self.y + self.height > 1.0 + 1e-9
        {
            return None;
        }
        let x = (self.x * width as f64).floor() as u32;
        let y = (self.y * height as f64).floor() as u32;
        let right = ((self.x + self.width) * width as f64).ceil().min(width as f64) as u32;
        let bottom = ((self.y + self.height) * height as f64).ceil().min(height as f64) as u32;
        (right > x && bottom > y).then_some((x, y, right.saturating_sub(x), bottom.saturating_sub(y)))
    }
}

pub fn is_bundled_project(path: &std::path::Path) -> bool {
    path.components().any(|p| {
        p.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case("defaultprojects")
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Appearance {
    pub selection: Option<String>,
    pub fit: String,
    pub brightness: f64,
    pub background_opacity: f64,
    pub blur_px: f64,
    pub overlay_opacity: f64,
    pub panel_opacity: f64,
    pub motion: String,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            selection: None,
            fit: "cover".into(),
            brightness: 0.85,
            background_opacity: 0.75,
            blur_px: 0.0,
            overlay_opacity: 0.25,
            panel_opacity: 0.94,
            motion: "auto".into(),
        }
    }
}

impl Appearance {
    pub fn normalize(&mut self) {
        fn finite(n: f64, default: f64, low: f64, high: f64) -> f64 {
            if n.is_finite() {
                n.clamp(low, high)
            } else {
                default
            }
        }
        if self.fit != "contain" {
            self.fit = "cover".into();
        }
        if self.motion != "static" {
            self.motion = "auto".into();
        }
        self.brightness = finite(self.brightness, 0.85, 0.2, 1.2);
        self.background_opacity = finite(self.background_opacity, 0.75, 0.0, 1.0);
        self.blur_px = finite(self.blur_px, 0.0, 0.0, 24.0);
        self.overlay_opacity = finite(self.overlay_opacity, 0.25, 0.0, 0.8);
        self.panel_opacity = finite(self.panel_opacity, 0.94, 0.85, 1.0);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    pub title: String,
    pub source: String,
    pub kind: String,
    pub root: PathBuf,
    pub media: Option<PathBuf>,
    pub cover: Option<PathBuf>,
    pub reason: Option<String>,
    #[serde(default)]
    pub scan_warning: Option<String>,
}

impl Asset {
    pub fn is_user_we_video(&self) -> bool {
        self.source == "wallpaperEngine"
            && self.kind == "video"
            && self.media.is_some()
            && !is_bundled_project(&self.root)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Library {
    pub crops: std::collections::HashMap<String, CropRect>,
    pub appearance: Appearance,
    pub assets: Vec<Asset>,
    pub roots: Vec<PathBuf>,
    pub scan_notes: Vec<String>,
    pub scan_completed: bool,
    pub scan_revision: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WallpaperEntry {
    pub id: String,
    pub source: String,
    pub kind: String,
    pub title: String,
    pub file_path: Option<PathBuf>,
    pub thumbnail_path: Option<PathBuf>,
}

impl From<&Asset> for WallpaperEntry {
    fn from(asset: &Asset) -> Self {
        Self {
            id: asset.id.clone(),
            source: if asset.source == "wallpaperEngine" {
                "wallpaper_engine"
            } else {
                "local"
            }
            .into(),
            kind: asset.kind.clone(),
            title: asset.title.clone(),
            file_path: asset.media.clone(),
            thumbnail_path: asset.cover.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WallpaperScanResult {
    pub entries: Vec<WallpaperEntry>,
    pub warnings: Vec<String>,
    pub from_cache: bool,
}

impl Library {
    pub fn scan_result(&self, from_cache: bool) -> WallpaperScanResult {
        WallpaperScanResult {
            entries: self
                .assets
                .iter()
                .filter(|a| a.is_user_we_video())
                .map(WallpaperEntry::from)
                .collect(),
            warnings: self.scan_notes.clone(),
            from_cache,
        }
    }

    pub fn cached_scan(
        &self,
        roots: &[PathBuf],
        force_refresh: bool,
    ) -> Option<WallpaperScanResult> {
        (self.scan_completed
            && self.scan_revision == SCAN_REVISION
            && self.roots == roots
            && !force_refresh)
            .then(|| self.scan_result(true))
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resolved {
    pub id: String,
    pub kind: String,
    pub media: Option<PathBuf>,
    pub cover: Option<PathBuf>,
}
