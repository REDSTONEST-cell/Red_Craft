use super::{display, model::*, next_visibility, scene, steam, store, web};
use launcher_core::download::CancelToken;

#[test]
fn crop_rect_rejects_invalid_and_out_of_bounds_coordinates() {
    let full = CropRect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 };
    assert_eq!(full.pixels(20, 10), Some((0, 0, 20, 10)));
    for bad in [CropRect { x: -0.1, ..full }, CropRect { width: 0.0, ..full },
        CropRect { height: f64::NAN, ..full }, CropRect { x: 0.5, ..full }] {
        assert!(bad.pixels(20, 10).is_none());
    }
}

#[test]
fn crop_keeps_asset_identity_and_preserves_original_pixels() {
    let config = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let path = external.path().join("original.png");
    image::RgbImage::from_fn(20, 10, |x, y| image::Rgb([x as u8, y as u8, 100])).save(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let asset = store::import(config.path(), &path, &CancelToken::new()).unwrap();
    let cropped = store::crop(config.path(), &asset, CropRect { x: 0.25, y: 0.2, width: 0.5, height: 0.5 }).unwrap();
    assert_eq!(asset.id, cropped.id);
    let result = image::open(cropped.media.as_ref().unwrap()).unwrap().to_rgb8();
    assert_eq!(result.dimensions(), (10, 5));
    assert_eq!(result.get_pixel(0, 0), &image::Rgb([5, 2, 100]));
    assert_eq!(result.get_pixel(9, 4), &image::Rgb([14, 6, 100]));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(std::fs::read(asset.media.as_ref().unwrap()).unwrap(), bytes);
    let mut tampered = asset.clone();
    tampered.media = Some(path);
    assert!(store::crop(config.path(), &tampered, CropRect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 }).is_err());
}

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
fn we_scan_lists_image_projects_too() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("steamapps/workshop/content/431960/123");
    std::fs::create_dir_all(&project).unwrap();
    image::RgbImage::new(4, 3)
        .save(project.join("wallpaper.PNG"))
        .unwrap();
    std::fs::write(project.join("project.json"), r#"{"type":"image","file":"wallpaper.PNG"}"#).unwrap();
    let (assets, _) =
        steam::scan_roots(vec![dir.path().into()], &CancelToken::new(), |_| {}).unwrap();
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].kind, "image");
    assert!(assets[0].reason.is_none());
}

#[test]
fn scans_every_wallpaper_type_but_never_bundled_defaultprojects() {
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
    std::fs::write(scene.join("project.json"), r#"{"type":"scene","file":"scene.json"}"#).unwrap();
    let web = dir.path().join("steamapps/workshop/content/431960/789");
    std::fs::create_dir_all(&web).unwrap();
    std::fs::write(web.join("project.json"), r#"{"type":"web","file":"index.html"}"#).unwrap();
    std::fs::write(web.join("index.html"), "<html></html>").unwrap();
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
    let mut kinds: Vec<_> = assets.iter().map(|a| a.kind.as_str()).collect();
    kinds.sort();
    assert_eq!(kinds, ["scene", "video", "video", "web"]);
    assert!(assets.iter().all(|a| a.reason.is_none()));
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
    assert!(asset.reason.is_none());
    assert_eq!(asset.cover.as_ref().unwrap().file_name().unwrap(), "preview.jpg");
    // 场景交给实时渲染；损坏的包无法生成静态画面时，静态回退用预览图，而不是把包本身交给前端。
    let config = tempfile::tempdir().unwrap();
    let resolved = display(config.path(), asset.clone(), false).unwrap();
    assert_eq!(resolved.kind, "scene");
    assert_eq!(resolved.media, asset.cover);
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
    assert!(asset.media.is_none());
    // 没有场景包也没有可用预览图：不可用。
    assert!(asset.reason.is_some());
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
fn animated_gif_covers_are_reencoded_as_a_single_static_frame() {
    let dir = tempfile::tempdir().unwrap();
    {
        let file = std::fs::File::create(dir.path().join("preview.gif")).unwrap();
        let mut encoder = image::codecs::gif::GifEncoder::new(file);
        for shade in [0u8, 255] {
            let frame = image::RgbaImage::from_pixel(4, 3, image::Rgba([shade, 0, 0, 255]));
            encoder.encode_frame(image::Frame::new(frame)).unwrap();
        }
    }
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
    let cover = asset.cover.unwrap();
    assert_eq!(cover.file_name().unwrap(), "preview.gif");
    let id = uuid::Uuid::new_v4().to_string();
    let still = store::still_cover(dir.path(), &id, &cover).unwrap();
    assert_eq!(still.extension().unwrap(), "png");
    let (w, h) = image::image_dimensions(&still).unwrap();
    assert_eq!(w * 3, h * 4);
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

// ───────────── 场景与网页壁纸 ─────────────

fn le(n: i32) -> [u8; 4] {
    n.to_le_bytes()
}

/// 按 Wallpaper Engine 的 .tex 格式构造一张单图片、单 mipmap 的纹理。
fn tex(format: i32, container: &str, free_image: i32, size: (i32, i32), image: (i32, i32), data: &[u8], lz4: bool) -> Vec<u8> {
    let mut out = b"TEXV0005\0TEXI0001\0".to_vec();
    for n in [format, 0, size.0, size.1, image.0, image.1, 0] {
        out.extend(le(n));
    }
    out.extend(container.as_bytes());
    out.push(0);
    out.extend(le(1));
    if container == "TEXB0003" {
        out.extend(le(free_image));
    }
    out.extend(le(1));
    out.extend(le(size.0));
    out.extend(le(size.1));
    let payload = if lz4 { lz4_flex::block::compress(data) } else { data.to_vec() };
    if container != "TEXB0001" {
        out.extend(le(lz4 as i32));
        out.extend(le(data.len() as i32));
    }
    out.extend(le(payload.len() as i32));
    out.extend(payload);
    out
}

fn pkg(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(le(8));
    out.extend(b"PKGV0001");
    out.extend(le(files.len() as i32));
    let mut offset = 0;
    for (name, bytes) in files {
        out.extend(le(name.len() as i32));
        out.extend(name.as_bytes());
        out.extend(le(offset));
        out.extend(le(bytes.len() as i32));
        offset += bytes.len() as i32;
    }
    for (_, bytes) in files {
        out.extend(bytes);
    }
    out
}

#[test]
fn tex_decodes_raw_lz4_and_crops_padding() {
    // 4×2 纹理，实际图片 3×1。
    let pixels: Vec<u8> = (0..8u8).flat_map(|i| [i * 10, 1, 2, 255]).collect();
    let image = scene::decode_tex(&tex(0, "TEXB0002", -1, (4, 2), (3, 1), &pixels, true)).unwrap();
    assert_eq!(image.dimensions(), (3, 1));
    assert_eq!(image.get_pixel(2, 0).0, [20, 1, 2, 255]);
    assert_eq!(scene::tex_size(&tex(0, "TEXB0001", -1, (4, 2), (3, 1), &pixels, false)), Some((3, 1)));
}

#[test]
fn tex_decodes_bc1_and_embedded_png() {
    // BC1：两种端点色（纯红、纯蓝），索引 0 与 1 交替。
    let block = [0x00, 0xf8, 0x1f, 0x00, 0x44, 0x44, 0x44, 0x44];
    let image = scene::decode_tex(&tex(7, "TEXB0002", -1, (4, 4), (4, 4), &block, false)).unwrap();
    assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0, 255]);
    assert_eq!(image.get_pixel(1, 0).0, [0, 0, 255, 255]);

    let mut png = Vec::new();
    image::RgbaImage::from_pixel(5, 3, image::Rgba([9, 8, 7, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let image = scene::decode_tex(&tex(0, "TEXB0003", 13, (5, 3), (5, 3), &png, false)).unwrap();
    assert_eq!(image.dimensions(), (5, 3));
    assert_eq!(image.get_pixel(4, 2).0, [9, 8, 7, 255]);
}

#[test]
fn corrupt_tex_and_pkg_are_errors_not_panics() {
    assert!(scene::decode_tex(b"").is_err());
    assert!(scene::decode_tex(b"TEXV0005\0TEXI0001\0").is_err());
    let mut huge = tex(0, "TEXB0002", -1, (4, 4), (4, 4), &[0; 64], false);
    let len = huge.len();
    huge[len - 68..len - 64].copy_from_slice(&le(i32::MAX));
    assert!(scene::decode_tex(&huge).is_err());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scene.pkg");
    std::fs::write(&path, b"\xff\xff\xff\x7fPKGV").unwrap();
    assert!(scene::Source::package(&path).is_err());
}

#[test]
fn scene_layers_are_composited_at_their_scene_positions() {
    // 4×2 场景：灰色底色，右半边放一个 2×2 的红色图层（场景坐标 y 轴向上）。
    let red: Vec<u8> = std::iter::repeat_n([255u8, 0, 0, 255], 4).flatten().collect();
    let files = [
        (
            "scene.json",
            br#"{"general":{"clearcolor":"0.5 0.5 0.5","orthogonalprojection":{"width":4,"height":2}},
                "objects":[{"image":"models/red.json","origin":"3 1 0","size":"2 2","visible":true},
                           {"image":"models/hidden.json","origin":"1 1 0","size":"2 2","visible":{"value":false}},
                           {"particle":"particles/snow.json"}]}"#
                .to_vec(),
        ),
        ("models/red.json", br#"{"material":"materials/red.json"}"#.to_vec()),
        ("models/hidden.json", br#"{"material":"materials/red.json"}"#.to_vec()),
        ("materials/red.json", br#"{"passes":[{"textures":["red"]}]}"#.to_vec()),
        ("materials/red.tex", tex(0, "TEXB0002", -1, (2, 2), (2, 2), &red, false)),
    ];
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("scene.pkg"), pkg(&files)).unwrap();
    let source = scene::Source::open(dir.path(), None).unwrap();
    let image = scene::render(&source, &serde_json::json!({})).unwrap();
    assert_eq!(image.dimensions(), (4, 2));
    assert_eq!(image.get_pixel(0, 0).0, [128, 128, 128, 255]);
    assert_eq!(image.get_pixel(3, 1).0, [255, 0, 0, 255]);

    // 选择壁纸时渲染并缓存为 PNG，项目原文件保持不变。
    std::fs::write(dir.path().join("project.json"), r#"{"type":"scene","file":"scene.pkg"}"#).unwrap();
    let asset = steam::project(dir.path()).unwrap();
    let config = tempfile::tempdir().unwrap();
    let resolved = display(config.path(), asset.clone(), false).unwrap();
    assert_eq!(resolved.kind, "scene");
    let png = resolved.media.unwrap();
    assert!(png.starts_with(std::fs::canonicalize(config.path()).unwrap()));
    assert_eq!(image::image_dimensions(&png).unwrap(), (4, 2));
    scene::cleanup(config.path(), &asset.id, None);
    assert!(!png.exists());
}

#[test]
fn unpacked_scene_directory_renders_and_rejects_traversal() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join("materials")).unwrap();
    std::fs::write(project.join("scene.json"), r#"{"objects":[]}"#).unwrap();
    let blue: Vec<u8> = std::iter::repeat_n([0u8, 0, 255, 255], 6).flatten().collect();
    std::fs::write(project.join("materials/Big.tex"), tex(0, "TEXB0002", -1, (3, 2), (3, 2), &blue, false)).unwrap();
    std::fs::write(dir.path().join("secret.json"), "{}").unwrap();
    let source = scene::Source::open(&project, None).unwrap();
    // 没有图层时回退到最大的纹理。
    assert_eq!(scene::render(&source, &serde_json::json!({})).unwrap().dimensions(), (3, 2));
    assert!(source.read("../secret.json", 1024).is_err());
}

#[test]
fn web_and_application_projects_are_listed_safely() {
    let dir = tempfile::tempdir().unwrap();
    let web = dir.path().join("web");
    std::fs::create_dir_all(web.join("页面 1")).unwrap();
    std::fs::write(web.join("project.json"), r#"{"type":"web","file":"页面 1/index.html"}"#).unwrap();
    std::fs::write(web.join("页面 1/index.html"), "<html></html>").unwrap();
    let asset = steam::project(&web).unwrap();
    assert_eq!(asset.kind, "web");
    let config = tempfile::tempdir().unwrap();
    let resolved = display(config.path(), asset, false).unwrap();
    assert_eq!(resolved.kind, "web");
    assert!(resolved.media.is_none());
    assert_eq!(resolved.entry.as_deref(), Some("%E9%A1%B5%E9%9D%A2%201/index.html"));

    let app = dir.path().join("app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(app.join("project.json"), r#"{"type":"application","file":"run.exe"}"#).unwrap();
    std::fs::write(app.join("run.exe"), b"MZ").unwrap();
    let asset = steam::project(&app).unwrap();
    assert!(asset.media.is_none() && asset.reason.is_some());
    image::RgbImage::new(4, 3).save(app.join("preview.jpg")).unwrap();
    let asset = steam::project(&app).unwrap();
    assert!(asset.media.is_none() && asset.reason.is_none() && asset.scan_warning.is_some());
    let resolved = display(config.path(), asset.clone(), false).unwrap();
    assert_eq!(resolved.kind, "image");
    assert_eq!(resolved.media, asset.cover);
}

#[test]
fn web_protocol_paths_reject_traversal_and_shim_runs_first() {
    assert_eq!(
        web::parse("/we-1/%E9%A1%B5%E9%9D%A2%201/index.html"),
        Some(("we-1".into(), std::path::PathBuf::from("页面 1").join("index.html")))
    );
    for bad in ["/we-1/../x", "/we-1/%2e%2e/x", "/we-1/a%2fb", "/we-1/a%5cb", "/we-1/C%3A", "/we-1/", "/we-1", "//x"] {
        assert!(web::parse(bad).is_none(), "{bad}");
    }
    let html = web::inject(b"<!DOCTYPE html><HTML><Head><script>mine()</script></head></html>", &serde_json::json!({"c": {"value": "</script>"}}));
    let html = String::from_utf8(html).unwrap();
    let shim = html.find("wallpaperPropertyListener").unwrap();
    assert!(html.starts_with("<!DOCTYPE html><HTML><Head><script>"));
    assert!(shim < html.find("mine()").unwrap());
    assert!(html.contains("<\\/script>"));
    assert_eq!(web::mime(std::path::Path::new("a.JS")), "text/javascript; charset=utf-8");
    assert!(!web::CSP.contains("ipc"));
}

#[test]
fn quicktime_and_matroska_videos_are_accepted_by_container_header() {
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in [("a.mov", &b"\x00\x00\x00\x08wide0000"[..]), ("b.mkv", &[0x1a, 0x45, 0xdf, 0xa3, 0][..]), ("c.m4v", &b"\x00\x00\x00\x18ftypM4V "[..])] {
        std::fs::write(dir.path().join(name), bytes).unwrap();
        store::video(&dir.path().join(name)).unwrap();
    }
    std::fs::write(dir.path().join("d.mov"), b"not a movie at all").unwrap();
    assert!(store::video(&dir.path().join("d.mov")).is_err());
}

fn solid_tex(w: i32, h: i32, rgba: [u8; 4]) -> Vec<u8> {
    let pixels: Vec<u8> = std::iter::repeat_n(rgba, (w * h) as usize).flatten().collect();
    tex(0, "TEXB0002", -1, (w, h), (w, h), &pixels, false)
}

/// 按 linux-wallpaperengine 的规则合成一个 8×4 场景（白色底色）。
fn render_scene(scene: &str, extra: Vec<(&str, Vec<u8>)>, props: serde_json::Value) -> image::RgbaImage {
    let mut files = vec![("scene.json", scene.as_bytes().to_vec())];
    files.extend(extra);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("scene.pkg"), pkg(&files)).unwrap();
    let source = scene::Source::open(dir.path(), None).unwrap();
    scene::render(&source, &props).unwrap()
}

fn image_layer(name: &str, texture: Option<&str>, blending: &str) -> Vec<(String, Vec<u8>)> {
    let textures = texture.map(|t| format!(r#"["{t}"]"#)).unwrap_or_else(|| "[null]".into());
    vec![
        (format!("models/{name}.json"), format!(r#"{{"material":"materials/{name}.json"}}"#).into_bytes()),
        (format!("materials/{name}.json"), format!(r#"{{"passes":[{{"blending":"{blending}","textures":{textures}}}]}}"#).into_bytes()),
    ]
}

fn owned(files: Vec<(String, Vec<u8>)>) -> Vec<(&'static str, Vec<u8>)> {
    files.into_iter().map(|(n, b)| (&*Box::leak(n.into_boxed_str()), b)).collect()
}

#[test]
fn scene_children_follow_parent_transform_and_user_properties() {
    // 组（id 1）位于 (4, 2) 且放大 2 倍；子图层相对组偏移 (1, 0)，即世界坐标 (6, 2)，尺寸 1×1 放大为 2×2。
    // 子图层颜色绑定用户属性 tint，project.json 当前值为纯蓝。
    let scene = r#"{"general":{"clearcolor":"1 1 1","orthogonalprojection":{"width":8,"height":4}},"objects":[
        {"id":1,"name":"group","origin":"4 2 0","scale":"2 2 1"},
        {"id":2,"name":"child","parent":1,"image":"models/a.json","origin":"1 0 0","size":"1 1",
         "color":{"user":"tint","value":"1 0 0"}}]}"#;
    let mut files = owned(image_layer("a", Some("w"), "translucent"));
    files.push(("materials/w.tex", solid_tex(2, 2, [255, 255, 255, 255])));
    let image = render_scene(scene, files, serde_json::json!({"tint": {"value": "0 0 1"}}));
    assert_eq!(image.get_pixel(5, 1).0, [0, 0, 255, 255]);
    assert_eq!(image.get_pixel(6, 2).0, [0, 0, 255, 255]);
    assert_eq!(image.get_pixel(3, 1).0, [255, 255, 255, 255]);
    // 父组被用户隐藏时子图层也隐藏。
    let hidden = scene.replace(r#""scale":"2 2 1"}"#, r#""scale":"2 2 1","visible":{"user":"show","value":true}}"#);
    let mut files = owned(image_layer("a", Some("w"), "translucent"));
    files.push(("materials/w.tex", solid_tex(2, 2, [255, 255, 255, 255])));
    let image = render_scene(&hidden, files, serde_json::json!({"show": {"value": false}}));
    assert!(image.pixels().all(|p| p.0 == [255, 255, 255, 255]));
}

#[test]
fn instance_textures_alignment_fullscreen_and_additive_blending() {
    // 底层：fullscreen 模型铺满（instance 纹理覆盖材质中的 null）；
    // 上层：左上对齐、加色混合的 2×2 图层。
    let scene = r#"{"general":{"clearcolor":"0 0 0","orthogonalprojection":{"width":8,"height":4}},"objects":[
        {"id":1,"image":"models/bg.json","instance":{"textures":["dark"]}},
        {"id":2,"image":"models/glow.json","origin":"0 4 0","size":"2 2","alignment":"topleft"}]}"#;
    let mut files = owned(image_layer("glow", Some("glow"), "additive"));
    files.push(("models/bg.json", br#"{"material":"materials/bg.json","fullscreen":true}"#.to_vec()));
    files.push(("materials/bg.json", br#"{"passes":[{"blending":"normal","textures":[null]}]}"#.to_vec()));
    files.push(("materials/dark.tex", solid_tex(1, 1, [100, 0, 0, 255])));
    files.push(("materials/glow.tex", solid_tex(2, 2, [100, 50, 0, 255])));
    let image = render_scene(scene, files, serde_json::json!({}));
    assert_eq!(image.get_pixel(7, 3).0, [100, 0, 0, 255]);
    assert_eq!(image.get_pixel(0, 0).0, [200, 50, 0, 255]);
    assert_eq!(image.get_pixel(1, 1).0, [200, 50, 0, 255]);
    assert_eq!(image.get_pixel(2, 2).0, [100, 0, 0, 255]);
}

#[test]
fn auto_projection_and_rotation() {
    // 自动投影：场景以原点为中心。一个 4×2 的图层旋转 90° 后变成竖直的 2×4。
    let scene = r#"{"general":{"clearcolor":"1 1 1","orthogonalprojection":{"auto":true}},"objects":[
        {"id":1,"image":"models/a.json","origin":"0 0 0","size":"4 4"},
        {"id":2,"image":"models/b.json","origin":"0 0 0","size":"4 2","angles":"0 0 1.5707963"}]}"#;
    let mut files = owned(image_layer("a", Some("white"), "translucent"));
    files.extend(owned(image_layer("b", Some("red"), "translucent")));
    files.push(("materials/white.tex", solid_tex(1, 1, [255, 255, 255, 255])));
    files.push(("materials/red.tex", solid_tex(1, 1, [255, 0, 0, 255])));
    let image = render_scene(scene, files, serde_json::json!({}));
    assert_eq!(image.dimensions(), (4, 4));
    assert_eq!(image.get_pixel(1, 0).0, [255, 0, 0, 255]);
    assert_eq!(image.get_pixel(2, 3).0, [255, 0, 0, 255]);
    assert_eq!(image.get_pixel(0, 1).0, [255, 255, 255, 255]);
}

#[test]
fn animated_texture_uses_its_first_frame() {
    // 4×2 精灵图，左半边红色、右半边绿色；帧信息指向右半边。
    let pixels: Vec<u8> = (0..8).flat_map(|i| if i % 4 < 2 { [255, 0, 0, 255] } else { [0, 255, 0, 255] }).collect();
    let mut bytes = tex(0, "TEXB0002", -1, (4, 2), (4, 2), &pixels, false);
    bytes[22..26].copy_from_slice(&le(4)); // flags：IsGif
    bytes.extend(b"TEXS0003\0");
    for n in [1, 2, 2] {
        bytes.extend(le(n)); // 帧数、gif 宽高
    }
    bytes.extend(le(0)); // 图片序号
    for f in [0.1f32, 2.0, 0.0, 2.0, 0.0, 0.0, 2.0] {
        bytes.extend(f.to_le_bytes());
    }
    let image = scene::decode_tex(&bytes).unwrap();
    assert_eq!(image.dimensions(), (2, 2));
    assert_eq!(image.get_pixel(0, 0).0, [0, 255, 0, 255]);
}

#[test]
fn scene_can_be_shown_as_its_preview_image() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("scene.pkg"), pkg(&[("scene.json", br#"{"objects":[]}"#.to_vec())])).unwrap();
    image::RgbImage::new(4, 3).save(dir.path().join("preview.jpg")).unwrap();
    let asset = steam::project(dir.path()).unwrap();
    let config = tempfile::tempdir().unwrap();
    let resolved = display(config.path(), asset.clone(), true).unwrap();
    assert_eq!(resolved.media, asset.cover);
}

#[test]
fn web_injection_keeps_bytes_and_skips_header_tags() {
    // GBK 编码的「标题」，以及 <header> 标签都不能被破坏或误判。
    let html = b"<html><header>x</header><HEAD><title>\xb1\xea\xcc\xe2</title></HEAD></html>";
    let out = web::inject(html, &serde_json::json!({}));
    let head = out.windows(6).position(|w| w == b"<HEAD>").unwrap();
    assert!(out[..head].windows(7).any(|w| w == b"<script") || out[head..].starts_with(b"<HEAD><script>"));
    assert!(out.windows(4).any(|w| w == b"\xb1\xea\xcc\xe2"));
    assert!(out.starts_with(b"<html><header>x</header><HEAD><script>"));
    assert!(web::shim("null").contains("project.json"));
}

#[test]
fn web_range_requests_are_parsed() {
    assert_eq!(web::range(Some("bytes=0-"), 10), Some((0, 9)));
    assert_eq!(web::range(Some("bytes=2-4"), 10), Some((2, 4)));
    assert_eq!(web::range(Some("bytes=-3"), 10), Some((7, 9)));
    assert_eq!(web::range(Some("bytes=5-100"), 10), Some((5, 9)));
    assert_eq!(web::range(Some("bytes=10-"), 10), None);
    assert_eq!(web::range(Some("bytes=0-1,4-5"), 10), None);
    assert_eq!(web::range(None, 10), None);
}

#[test]
fn steam_folders_without_project_json_are_not_wallpapers() {
    // Steam 根目录下的 graphics / public / resource 里有图片，但不是壁纸项目。
    let dir = tempfile::tempdir().unwrap();
    for folder in ["graphics", "public", "resource"] {
        std::fs::create_dir_all(dir.path().join(folder)).unwrap();
        image::RgbImage::new(4, 3).save(dir.path().join(folder).join("icon.png")).unwrap();
    }
    let (assets, _) = steam::scan_roots(vec![dir.path().into()], &CancelToken::new(), |_| {}).unwrap();
    assert!(assets.is_empty());
    // 用户手动添加的单个项目目录（没有 project.json）仍按扩展名识别。
    let single = dir.path().join("public");
    std::fs::write(single.join("clip.mp4"), b"\x00\x00\x00\x18ftypisom0000").unwrap();
    let (assets, _) = steam::scan_roots(vec![single], &CancelToken::new(), |_| {}).unwrap();
    assert_eq!(assets.len(), 1);
}

#[test]
fn scene_engine_routes_and_fps_limit() {
    let (id, rel) = web::parse("/__engine/scene.html").unwrap();
    assert_eq!(id, web::ENGINE_DIR);
    assert_eq!(rel, std::path::PathBuf::from("scene.html"));
    let mut a: Appearance = serde_json::from_str("{}").unwrap();
    a.normalize();
    assert_eq!(a.fps, 30);
    a.fps = 1000;
    a.normalize();
    assert_eq!(a.fps, 240);
    a.fps = 0;
    a.normalize();
    assert_eq!(a.fps, 30);
}

#[test]
fn engine_is_served_with_its_license_notice() {
    let js = web::engine_bytes_for_test();
    assert!(js.starts_with(b"/*! WebWallGL"));
    assert!(js.windows(11).any(|w| w == b"MIT License"));
    assert!(js.windows(10).any(|w| w == b"WebWallGL="));
}

#[test]
fn background_luminance_is_measured_from_the_still_image() {
    let dir = tempfile::tempdir().unwrap();
    let white = dir.path().join("white.png");
    let black = dir.path().join("black.png");
    image::RgbImage::from_pixel(8, 8, image::Rgb([255, 255, 255])).save(&white).unwrap();
    image::RgbImage::from_pixel(8, 8, image::Rgb([0, 0, 0])).save(&black).unwrap();
    assert!(store::luminance(&white).unwrap() > 0.99);
    assert!(store::luminance(&black).unwrap() < 0.01);
}
