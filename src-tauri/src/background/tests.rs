use super::{model::*, next_visibility, steam, store};
use launcher_core::download::CancelToken;

#[test]
fn broken_json_falls_back_to_video_without_treating_preview_as_media() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.json"), "{broken").unwrap();
    std::fs::write(
        dir.path().join("movie.mp4"),
        b"\x00\x00\x00\x18ftypisom0000",
    )
    .unwrap();
    image::RgbImage::new(4, 3)
        .save(dir.path().join("preview.jpg"))
        .unwrap();
    let asset = steam::project(dir.path()).unwrap();
    assert_eq!(asset.kind, "video");
    assert_eq!(asset.media.unwrap().file_name().unwrap(), "movie.mp4");
    assert_eq!(asset.cover.unwrap().file_name().unwrap(), "preview.jpg");
}

#[test]
fn we_scan_omits_image_projects_even_when_extension_is_supported() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("steamapps/workshop/content/431960/123");
    std::fs::create_dir_all(&project).unwrap();
    image::RgbImage::new(4, 3)
        .save(project.join("wallpaper.PNG"))
        .unwrap();
    let (assets, _) =
        steam::scan_roots(vec![dir.path().into()], &CancelToken::new(), |_| {}).unwrap();
    assert!(assets.is_empty());
}

#[test]
fn scans_only_non_bundled_videos_even_when_defaultprojects_is_added_manually() {
    let dir = tempfile::tempdir().unwrap();
    let install = dir
        .path()
        .join("steamapps/common/wallpaper_engine/projects");
    let own = install.join("myprojects/own");
    let official = install.join("defaultprojects/demo");
    let workshop = dir.path().join("steamapps/workshop/content/431960/123");
    for path in [&own, &official, &workshop] {
        std::fs::create_dir_all(path).unwrap();
        std::fs::write(
            path.join("project.json"),
            r#"{"type":"video","file":"clip.mp4"}"#,
        )
        .unwrap();
        std::fs::write(path.join("clip.mp4"), b"\x00\x00\x00\x18ftypisom0000").unwrap();
    }
    let scene = dir.path().join("steamapps/workshop/content/431960/456");
    std::fs::create_dir_all(&scene).unwrap();
    std::fs::write(scene.join("scene.pkg"), b"unused").unwrap();
    let (assets, _) = steam::scan_roots(
        vec![
            dir.path().into(),
            official.clone(),
            install.join("defaultprojects"),
        ],
        &CancelToken::new(),
        |_| {},
    )
    .unwrap();
    assert_eq!(assets.len(), 2);
    assert!(assets.iter().all(|a| a.kind == "video"));
    assert!(!assets.iter().any(|a| a
        .root
        .starts_with(std::fs::canonicalize(&official).unwrap())));
}

#[test]
fn scene_pkg_is_classified_without_json_and_never_served_as_media() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("scene.pkg"), b"do not unpack").unwrap();
    image::RgbImage::new(4, 3)
        .save(dir.path().join("preview.jpg"))
        .unwrap();
    let asset = steam::project(dir.path()).unwrap();
    assert_eq!(asset.kind, "scene");
    assert!(asset.media.is_none());
    assert!(asset.reason.is_some());
    assert_eq!(asset.cover.unwrap().file_name().unwrap(), "preview.jpg");
}

#[test]
fn image_project_metadata_selects_the_image_instead_of_preview() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.json"),
        r#"{"title":"Photo","type":"image","file":"photo.webp"}"#,
    )
    .unwrap();
    image::RgbImage::new(4, 3)
        .save(dir.path().join("photo.webp"))
        .unwrap();
    let asset = steam::project(dir.path()).unwrap();
    assert_eq!(asset.title, "Photo");
    assert_eq!(asset.kind, "image");
    assert_eq!(asset.media.unwrap().file_name().unwrap(), "photo.webp");
    assert!(asset.reason.is_none());
}

#[test]
fn scan_dto_has_exact_public_fields_and_never_exposes_a_scene_package() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("scene.pkg"), b"unused").unwrap();
    image::RgbImage::new(4, 3)
        .save(dir.path().join("preview.jpg"))
        .unwrap();
    let asset = steam::project(dir.path()).unwrap();
    let entry = serde_json::to_value(WallpaperEntry::from(&asset)).unwrap();
    let fields: Vec<_> = entry
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        fields,
        [
            "file_path",
            "id",
            "kind",
            "source",
            "thumbnail_path",
            "title"
        ]
    );
    assert_eq!(entry["source"], "wallpaper_engine");
    assert_eq!(entry["kind"], "scene");
    assert!(entry["file_path"].is_null());
    assert!(entry["thumbnail_path"]
        .as_str()
        .unwrap()
        .ends_with("preview.jpg"));
}

#[test]
fn scan_cache_survives_restart_but_force_and_root_changes_invalidate_it() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("clip.mp4"), b"\x00\x00\x00\x18ftypisom0000").unwrap();
    let asset = steam::project(&project).unwrap();
    let roots = steam::normalize_roots(&[dir.path().into()]).unwrap();
    let saved = Library {
        assets: vec![asset],
        roots: roots.clone(),
        scan_completed: true,
        scan_revision: SCAN_REVISION,
        ..Library::default()
    };
    store::save(dir.path(), &saved).unwrap();
    let restored = store::load(dir.path());
    let hit = restored.cached_scan(&roots, false).unwrap();
    assert!(hit.from_cache);
    assert_eq!(hit.entries.len(), 1);
    assert!(restored.cached_scan(&roots, true).is_none());
    assert!(restored.cached_scan(&[], false).is_none());
    assert!(Library::default().cached_scan(&[], false).is_none());
    let mut old = restored.clone();
    old.scan_revision = 0;
    assert!(old.cached_scan(&roots, false).is_none());
}

#[test]
fn opacity_clamps_without_changing_legacy_brightness_or_text_panel() {
    let mut value: Appearance =
        serde_json::from_str(r#"{"brightness":0.6,"backgroundOpacity":99,"panelOpacity":0.94}"#)
            .unwrap();
    value.normalize();
    assert_eq!(value.background_opacity, 1.0);
    assert_eq!(value.brightness, 0.6);
    assert_eq!(value.panel_opacity, 0.94);
    value.background_opacity = f64::NAN;
    value.normalize();
    assert_eq!(value.background_opacity, 0.75);
    assert_eq!(
        serde_json::from_str::<Appearance>("{}")
            .unwrap()
            .background_opacity,
        0.75
    );
}

#[test]
fn grid_thumbnail_is_small_and_distinct_from_full_background_cover() {
    let dir = tempfile::tempdir().unwrap();
    let image_path = dir.path().join("image.png");
    image::RgbImage::new(1000, 800).save(&image_path).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let thumb = store::thumbnail(dir.path(), &id, &image_path).unwrap();
    let cover = store::still_cover(dir.path(), &id, &image_path).unwrap();
    assert_ne!(thumb, cover);
    let size = image::image_dimensions(&thumb).unwrap();
    assert!(size.0 <= 480 && size.1 <= 270);
    assert_eq!(
        store::thumbnail(dir.path(), &id, &image_path).unwrap(),
        thumb
    );
}

#[test]
fn rejects_media_outside_project_including_parent_paths() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(dir.path().join("outside.mp4"), b"secret").unwrap();
    assert!(store::inside(&project, &project.join("../outside.mp4")).is_err());
}

#[test]
fn scene_with_gif_preview_is_never_a_video() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.json"),
        r#"{"title":"Scene","type":"scene","file":"scene.pkg","preview":"preview.gif"}"#,
    )
    .unwrap();
    let asset = steam::project(dir.path()).unwrap();
    assert_eq!(asset.kind, "scene");
    assert!(asset.reason.is_some());
    assert!(asset.media.is_none());
}

#[test]
fn parses_multilibrary_vdf_with_escaped_windows_paths() {
    let paths = steam::library_paths(
        r#""libraryfolders" { "0" { "path" "C:\\Steam" "apps" { "431960" "123" } } "1" { "path" "D:\\Steam Library" } }"#,
    );
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[1].to_string_lossy(), r"D:\Steam Library");
}

#[test]
fn manual_roots_are_canonicalized_and_deduplicated() {
    let dir = tempfile::tempdir().unwrap();
    let roots = steam::normalize_roots(&[dir.path().into(), dir.path().join(".")]).unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0], std::fs::canonicalize(dir.path()).unwrap());
}

#[test]
fn corrupt_image_is_not_imported_and_original_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("broken.png");
    std::fs::write(&source, b"not an image").unwrap();
    assert!(store::import(dir.path(), &source, &CancelToken::new()).is_err());
    assert!(source.exists());
}

#[test]
fn image_copy_restore_and_remove_never_delete_original() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("中文 image.png");
    image::RgbImage::new(4, 3).save(&source).unwrap();
    let asset = store::import(dir.path(), &source, &CancelToken::new()).unwrap();
    let copy = asset.media.as_ref().unwrap();
    assert!(copy.exists());
    assert_ne!(copy, &source);
    let mut library = Library::default();
    library.appearance.selection = Some(asset.id.clone());
    library.assets.push(asset.clone());
    store::save(dir.path(), &library).unwrap();
    let restored = store::load(dir.path());
    assert_eq!(restored.appearance.selection, Some(asset.id.clone()));
    store::remove_copy(dir.path(), &asset).unwrap();
    assert!(!copy.exists());
    assert!(source.exists());
}

#[test]
fn cancelled_import_leaves_no_registered_media() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("image.png");
    image::RgbImage::new(4, 3).save(&source).unwrap();
    let token = CancelToken::new();
    token.cancel();
    assert!(store::import(dir.path(), &source, &token).is_err());
    assert!(store::load(dir.path()).assets.is_empty());
}

#[test]
fn appearance_defaults_and_limits_are_compatible() {
    let mut a: Appearance = serde_json::from_str(r#"{"brightness":99,"panelOpacity":0}"#).unwrap();
    a.normalize();
    assert_eq!(a.brightness, 1.2);
    assert_eq!(a.panel_opacity, 0.85);
    assert_eq!(a.fit, "cover");
    assert!(a.selection.is_none());
}

#[test]
fn secondary_library_scans_once_and_bad_projects_do_not_abort_it() {
    let dir = tempfile::tempdir().unwrap();
    let steam = dir.path().join("Steam");
    let secondary = dir.path().join("Second Library 中文");
    std::fs::create_dir_all(steam.join("steamapps")).unwrap();
    let video = secondary.join("steamapps/workshop/content/431960/42");
    let broken = secondary.join("steamapps/workshop/content/431960/43");
    std::fs::create_dir_all(&video).unwrap();
    std::fs::create_dir_all(&broken).unwrap();
    let escaped = secondary.to_string_lossy().replace('\\', "\\\\");
    std::fs::write(
        steam.join("steamapps/libraryfolders.vdf"),
        format!(r#""libraryfolders" {{ "1" {{ "path" "{escaped}" }} }}"#),
    )
    .unwrap();
    std::fs::write(
        video.join("project.json"),
        r#"{"title":"Video","type":"video","file":"movie.mp4"}"#,
    )
    .unwrap();
    std::fs::write(video.join("movie.mp4"), b"\x00\x00\x00\x18ftypisom0000").unwrap();
    std::fs::write(broken.join("project.json"), "not json").unwrap();
    let (assets, notes) = steam::scan_roots(
        vec![steam, secondary.clone(), secondary],
        &CancelToken::new(),
        |_| {},
    )
    .unwrap();
    assert_eq!(assets.len(), 1);
    assert!(assets[0].reason.is_none());
    assert_eq!(notes.len(), 1);
    assert!(notes[0].contains("43"));
}

#[test]
fn video_and_cover_parent_traversal_are_never_authorized() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(
        dir.path().join("outside.mp4"),
        b"\x00\x00\x00\x18ftypisom0000",
    )
    .unwrap();
    image::RgbImage::new(4, 3)
        .save(dir.path().join("outside.png"))
        .unwrap();
    std::fs::write(
        project.join("project.json"),
        r#"{"type":"video","file":"../outside.mp4","preview":"../outside.png"}"#,
    )
    .unwrap();
    let asset = steam::project(&project).unwrap();
    assert!(asset.media.is_none());
    assert!(asset.cover.is_none());
    assert!(asset.reason.is_some());
}

#[test]
fn animated_gif_covers_fall_back_instead_of_bypassing_static_mode() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("preview.gif"), b"GIF89a").unwrap();
    std::fs::write(
        dir.path().join("movie.mp4"),
        b"\x00\x00\x00\x18ftypisom0000",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("project.json"),
        r#"{"type":"video","file":"movie.mp4","preview":"preview.gif"}"#,
    )
    .unwrap();
    let asset = steam::project(dir.path()).unwrap();
    assert!(asset.media.is_some());
    assert!(asset.cover.is_none());
}

#[test]
fn static_cover_is_an_owned_single_frame_png() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.webp");
    image::RgbImage::new(16, 9).save(&source).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let cover = store::still_cover(dir.path(), &id, &source).unwrap();
    assert_eq!(cover.extension().unwrap(), "png");
    assert!(
        cover.starts_with(std::fs::canonicalize(dir.path().join("backgrounds/covers")).unwrap())
    );
    assert!(source.exists());
    assert_eq!(store::still_cover(dir.path(), &id, &source).unwrap(), cover);
}

#[test]
fn window_visibility_is_only_broadcast_when_it_changes() {
    let mut last = None;
    assert!(next_visibility(&mut last, true));
    assert!(!next_visibility(&mut last, true));
    assert!(next_visibility(&mut last, false));
    assert!(!next_visibility(&mut last, false));
}

#[test]
fn removing_an_asset_cleans_only_its_cover_cache() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.png");
    image::RgbImage::new(16, 9).save(&source).unwrap();
    let kept = uuid::Uuid::new_v4().to_string();
    let gone = uuid::Uuid::new_v4().to_string();
    let kept_cover = store::still_cover(dir.path(), &kept, &source).unwrap();
    let gone_cover = store::still_cover(dir.path(), &gone, &source).unwrap();
    store::remove_covers(dir.path(), &gone).unwrap();
    assert!(!gone_cover.exists());
    assert!(kept_cover.exists());
    // 没有缓存目录时也应是幂等的成功。
    store::remove_covers(&dir.path().join("missing"), &gone).unwrap();
}

#[cfg(windows)]
#[test]
fn failed_library_replacement_preserves_previous_config() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    let mut original = Library::default();
    original.appearance.brightness = 0.6;
    store::save(dir.path(), &original).unwrap();
    let path = dir.path().join("backgrounds/library.json");
    let bytes = std::fs::read(&path).unwrap();
    let locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    assert!(store::save(dir.path(), &Library::default()).is_err());
    drop(locked);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(store::load(dir.path()).appearance.brightness, 0.6);
}
