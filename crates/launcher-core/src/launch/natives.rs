//! natives 解压。

use std::io::Read;
use std::path::Path;

use crate::error::{CoreError, IoContext, Result};
use crate::paths::GameDir;
use crate::version::{plan_libraries, Environment, LibKind, Library};

const NATIVE_EXTS: &[&str] = &[".dll", ".so", ".dylib", ".jnilib"];

/// 把版本需要的 natives 解压到 `target`（阻塞调用）。
///
/// - 旧写法：按 `extract.exclude` 排除后原样解压；
/// - 1.19+ 写法：只取动态库文件并平铺到目录根部（已按架构过滤，不会重名）。
pub fn extract_natives(game: &GameDir, libs: &[Library], env: &Environment, target: &Path) -> Result<()> {
    // 清理旧文件；游戏运行中文件可能被占用，失败时忽略
    if target.exists() {
        let _ = std::fs::remove_dir_all(target);
    }
    std::fs::create_dir_all(target).at(target)?;

    for lib in plan_libraries(libs, env) {
        let LibKind::Native { exclude, classpath } = &lib.kind else {
            continue;
        };
        let jar = game.library(&lib.path);
        let flatten = *classpath;
        extract_one(&jar, target, exclude, flatten)?;
    }
    Ok(())
}

fn extract_one(jar: &Path, target: &Path, exclude: &[String], flatten: bool) -> Result<()> {
    let file = std::fs::File::open(jar).at(jar)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| CoreError::Archive {
        path: jar.to_path_buf(),
        reason: e.to_string(),
    })?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| CoreError::Archive {
            path: jar.to_path_buf(),
            reason: e.to_string(),
        })?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        if exclude.iter().any(|ex| name.starts_with(ex.as_str())) {
            continue;
        }
        let dest = if flatten {
            if !NATIVE_EXTS.iter().any(|ext| name.ends_with(ext)) {
                continue;
            }
            let file_name = name.rsplit('/').next().unwrap_or(&name);
            target.join(file_name)
        } else {
            match entry.enclosed_name() {
                Some(p) => target.join(p),
                None => continue,
            }
        };
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).at(parent)?;
        }
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).map_err(|e| CoreError::Archive {
            path: jar.to_path_buf(),
            reason: e.to_string(),
        })?;
        std::fs::write(&dest, buf).at(&dest)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Write;

    fn make_jar(path: &Path, files: &[(&str, &[u8])]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, data) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn extracts_both_styles() {
        let dir = tempfile::tempdir().unwrap();
        let game = GameDir::new(dir.path());
        make_jar(
            &game.library("old.jar"),
            &[("lwjgl64.dll", b"a"), ("META-INF/MANIFEST.MF", b"m")],
        );
        make_jar(
            &game.library("new.jar"),
            &[("windows/x64/org/lwjgl/lwjgl.dll", b"b"), ("META-INF/x.sha1", b"s"), ("org/lwjgl/Foo.class", b"c")],
        );
        let libs: Vec<Library> = serde_json::from_str(
            r#"[
            {"name": "x:old:1", "natives": {"windows": "natives-windows"}, "extract": {"exclude": ["META-INF/"]},
             "downloads": {"classifiers": {"natives-windows": {"path": "old.jar", "url": "u"}}}},
            {"name": "x:new:1:natives-windows", "downloads": {"artifact": {"path": "new.jar", "url": "u"}}}
        ]"#,
        )
        .unwrap();
        let env = Environment {
            os_name: "windows".into(),
            os_version: "10.0".into(),
            arch: "x86_64".into(),
            features: HashMap::new(),
        };
        let target = dir.path().join("natives");
        extract_natives(&game, &libs, &env, &target).unwrap();
        assert_eq!(std::fs::read(target.join("lwjgl64.dll")).unwrap(), b"a");
        assert_eq!(std::fs::read(target.join("lwjgl.dll")).unwrap(), b"b");
        assert!(!target.join("META-INF").exists());
        assert!(!target.join("Foo.class").exists());
    }
}
