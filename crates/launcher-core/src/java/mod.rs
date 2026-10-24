//! Java 检测、选择与自动下载。

mod runtime;
mod scan;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use runtime::{component_major, install_runtime, runtime_java_path, runtime_platform, RUNTIME_LIST_URL};
pub use scan::scan_installed;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaInstall {
    /// java 可执行文件
    pub path: PathBuf,
    /// 完整版本号，例如 `17.0.15`、`1.8.0_202`
    pub version: String,
    pub major: u32,
    /// Mojang 写法：`x86_64` / `x86` / `arm64`
    pub arch: String,
}

/// 从版本号解析主版本：`1.8.0_202` → 8，`17.0.1` → 17。
pub fn parse_major(version: &str) -> Option<u32> {
    let mut parts = version.trim().trim_matches('"').split(['.', '_', '-', '+']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

fn normalize_arch(arch: &str) -> String {
    match arch.trim().trim_matches('"') {
        "amd64" | "x86_64" | "x64" => "x86_64",
        "x86" | "i386" | "i586" | "i686" => "x86",
        "aarch64" | "arm64" => "arm64",
        "arm" | "aarch32" => "arm32",
        other => other,
    }
    .to_string()
}

/// 探测某个 java 可执行文件的版本与架构（阻塞调用）。
pub fn probe(java: &Path) -> Option<JavaInstall> {
    if !java.is_file() {
        return None;
    }
    // 优先读取 JDK 目录下的 release 文件，避免启动 JVM
    if let Some(home) = java.parent().and_then(Path::parent) {
        if let Ok(text) = std::fs::read_to_string(home.join("release")) {
            let get = |key: &str| {
                text.lines()
                    .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
                    .map(|v| v.trim().trim_matches('"').to_string())
            };
            if let (Some(version), Some(arch)) = (get("JAVA_VERSION"), get("OS_ARCH")) {
                if let Some(major) = parse_major(&version) {
                    return Some(JavaInstall {
                        path: java.to_path_buf(),
                        version,
                        major,
                        arch: normalize_arch(&arch),
                    });
                }
            }
        }
    }

    let mut cmd = std::process::Command::new(java);
    cmd.args(["-XshowSettings:properties", "-version"])
        .env_remove("JAVA_TOOL_OPTIONS")
        .env_remove("_JAVA_OPTIONS");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let output = cmd.output().ok()?;
    let text = String::from_utf8_lossy(&output.stderr);
    let prop = |key: &str| {
        text.lines().find_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == key).then(|| v.trim().to_string())
        })
    };
    let version = prop("java.version")?;
    Some(JavaInstall {
        path: java.to_path_buf(),
        major: parse_major(&version)?,
        version,
        arch: normalize_arch(&prop("os.arch").unwrap_or_default()),
    })
}

/// 为需要 `required` 主版本的游戏挑选 Java。
///
/// - 优先主版本完全一致、且架构与系统一致的；
/// - 需要 16 及以上时，可以使用更高版本（取最接近的）；
/// - 需要 8 等旧版本时只接受完全一致的版本，因为旧版 Forge/LaunchWrapper 在新 Java 上无法运行。
pub fn pick<'a>(installs: &'a [JavaInstall], required: u32, system_arch: &str) -> Option<&'a JavaInstall> {
    let arch_rank = |j: &JavaInstall| if j.arch == system_arch { 0 } else { 1 };
    // 32 位 Java 无法分配较大内存，只在系统本身是 32 位时使用
    let usable = |j: &&JavaInstall| j.arch != "x86" || system_arch == "x86";
    let exact = installs
        .iter()
        .filter(usable)
        .filter(|j| j.major == required)
        .min_by_key(|j| arch_rank(j));
    if exact.is_some() {
        return exact;
    }
    if required >= 16 {
        return installs
            .iter()
            .filter(usable)
            .filter(|j| j.major > required)
            .min_by_key(|j| (j.major, arch_rank(j)));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn j(major: u32, arch: &str) -> JavaInstall {
        JavaInstall {
            path: format!("/java{major}{arch}").into(),
            version: major.to_string(),
            major,
            arch: arch.into(),
        }
    }

    #[test]
    fn major_parsing() {
        assert_eq!(parse_major("1.8.0_202"), Some(8));
        assert_eq!(parse_major("17.0.15"), Some(17));
        assert_eq!(parse_major("21"), Some(21));
        assert_eq!(parse_major("\"25-ea\""), Some(25));
        assert_eq!(parse_major("abc"), None);
    }

    #[test]
    fn picking() {
        let all = vec![j(8, "x86"), j(8, "x86_64"), j(17, "x86_64"), j(21, "x86_64"), j(25, "arm64")];
        assert_eq!(pick(&all, 8, "x86_64").unwrap().path, PathBuf::from("/java8x86_64"));
        assert_eq!(pick(&all, 17, "x86_64").unwrap().major, 17);
        assert_eq!(pick(&all, 16, "x86_64").unwrap().major, 17);
        assert_eq!(pick(&all, 21, "x86_64").unwrap().major, 21);
        assert_eq!(pick(&all, 25, "x86_64").unwrap().major, 25);
        // 旧版本不使用高版本 Java
        assert!(pick(&[j(17, "x86_64")], 8, "x86_64").is_none());
        // 64 位系统不用 32 位 Java
        assert!(pick(&[j(8, "x86")], 8, "x86_64").is_none());
        assert!(pick(&[j(8, "x86")], 8, "x86").is_some());
    }

    #[test]
    fn probe_release_file() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("java");
        std::fs::write(&exe, b"").unwrap();
        std::fs::write(
            dir.path().join("release"),
            "IMPLEMENTOR=\"Eclipse Adoptium\"\nJAVA_VERSION=\"17.0.9\"\nOS_ARCH=\"amd64\"\n",
        )
        .unwrap();
        let info = probe(&exe).unwrap();
        assert_eq!(info.major, 17);
        assert_eq!(info.arch, "x86_64");
        assert_eq!(info.version, "17.0.9");
    }
}
