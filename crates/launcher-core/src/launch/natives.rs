//! natives 解压。

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::{CoreError, IoContext, Result};
use crate::paths::GameDir;
use crate::version::{plan_libraries, Environment, LibKind, Library};

const NATIVE_EXTS: &[&str] = &[".dll", ".so", ".dylib", ".jnilib"];

/// 把版本需要的 natives 解压到 `target`（阻塞调用）。
///
/// - 旧写法：按 `extract.exclude` 排除后原样解压；
/// - 1.19+ 写法：只取动态库文件并平铺到目录根部（已按架构过滤，不会重名）。
///
/// 已存在且大小与 CRC 都相同的文件不再重新写入（每次启动都重写会触发杀毒软件重新扫描，拖慢启动），
/// 只删除不再需要的文件。
pub fn extract_natives(game: &GameDir, libs: &[Library], env: &Environment, target: &Path) -> Result<()> {
    std::fs::create_dir_all(target).at(target)?;
    let mut wanted = HashSet::new();
    for lib in plan_libraries(libs, env) {
        let LibKind::Native { exclude, classpath } = &lib.kind else {
            continue;
        };
        let jar = game.library(&lib.path);
        let flatten = *classpath;
        extract_one(&jar, target, exclude, flatten, &mut wanted)?;
    }
    // 清理上次留下但这次不需要的文件；游戏运行中文件可能被占用，失败时忽略
    remove_unwanted(target, &wanted);
    Ok(())
}

fn remove_unwanted(dir: &Path, wanted: &HashSet<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            remove_unwanted(&p, wanted);
            let _ = std::fs::remove_dir(&p);
        } else if !wanted.contains(&p) {
            let _ = std::fs::remove_file(&p);
        }
    }
}

/// 已有文件与压缩包中的条目相同（大小与 CRC32 一致）。
fn same_file(path: &Path, size: u64, crc: u32) -> bool {
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() && m.len() == size => std::fs::read(path).is_ok_and(|data| crc32fast::hash(&data) == crc),
        _ => false,
    }
}

fn extract_one(jar: &Path, target: &Path, exclude: &[String], flatten: bool, wanted: &mut HashSet<PathBuf>) -> Result<()> {
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
        wanted.insert(dest.clone());
        if same_file(&dest, entry.size(), entry.crc32()) {
            continue;
        }
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

        // 再次解压：内容相同的文件不重写，内容不同的会被修正，多余的文件被删除
        let mtime = |p: &Path| std::fs::metadata(p).unwrap().modified().unwrap();
        let before = mtime(&target.join("lwjgl64.dll"));
        std::fs::write(target.join("lwjgl.dll"), b"x").unwrap();
        std::fs::write(target.join("stale.dll"), b"old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        extract_natives(&game, &libs, &env, &target).unwrap();
        assert_eq!(mtime(&target.join("lwjgl64.dll")), before);
        assert_eq!(std::fs::read(target.join("lwjgl.dll")).unwrap(), b"b");
        assert!(!target.join("stale.dll").exists());
    }
}
