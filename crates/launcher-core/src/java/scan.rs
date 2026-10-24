//! 扫描系统中已安装的 Java。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::JavaInstall;

#[cfg(windows)]
const JAVA_EXE: &str = "java.exe";
#[cfg(not(windows))]
const JAVA_EXE: &str = "java";

/// 扫描所有候选位置并探测版本（阻塞调用，内部并行探测）。
pub fn scan_installed(extra_roots: &[PathBuf]) -> Vec<JavaInstall> {
    let mut candidates = Vec::new();

    if let Some(home) = std::env::var_os("JAVA_HOME") {
        candidates.push(PathBuf::from(home).join("bin").join(JAVA_EXE));
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            candidates.push(dir.join(JAVA_EXE));
        }
    }
    for root in extra_roots {
        collect_runtime_dir(root, &mut candidates);
    }
    platform_candidates(&mut candidates);

    // 去重（按规范化路径）
    let mut seen = HashSet::new();
    let unique: Vec<PathBuf> = candidates
        .into_iter()
        .filter(|p| p.is_file())
        .filter_map(|p| {
            let canon = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
            seen.insert(canon.clone()).then_some(canon)
        })
        .collect();

    let mut found: Vec<JavaInstall> = std::thread::scope(|s| {
        let handles: Vec<_> = unique
            .iter()
            .map(|p| s.spawn(move || super::probe(p)))
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
    });
    found.sort_by(|a, b| b.major.cmp(&a.major).then_with(|| a.path.cmp(&b.path)));
    found
}

/// 在目录下查找 `*/bin/java`，最多向下找几层（用于 runtime、JDK 集合目录）。
fn collect_runtime_dir(root: &Path, out: &mut Vec<PathBuf>) {
    fn walk(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
        let bin = dir.join("bin").join(JAVA_EXE);
        if bin.is_file() {
            out.push(bin);
            return;
        }
        // macOS 的 JDK 目录结构
        let mac = dir.join("Contents").join("Home").join("bin").join(JAVA_EXE);
        if mac.is_file() {
            out.push(mac);
            return;
        }
        if depth == 0 {
            return;
        }
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    walk(&e.path(), depth - 1, out);
                }
            }
        }
    }
    walk(root, 4, out);
}

#[cfg(windows)]
fn platform_candidates(out: &mut Vec<PathBuf>) {
    use std::os::windows::process::CommandExt;

    // 注册表：各发行版写入的安装路径
    let keys = [
        (r"HKLM\SOFTWARE\JavaSoft", "JavaHome"),
        (r"HKLM\SOFTWARE\WOW6432Node\JavaSoft", "JavaHome"),
        (r"HKLM\SOFTWARE\Eclipse Adoptium", "Path"),
        (r"HKLM\SOFTWARE\Eclipse Foundation", "Path"),
        (r"HKLM\SOFTWARE\AdoptOpenJDK", "Path"),
        (r"HKLM\SOFTWARE\Microsoft\JDK", "Path"),
        (r"HKLM\SOFTWARE\Azul Systems\Zulu", "InstallationPath"),
        (r"HKLM\SOFTWARE\BellSoft\Liberica", "InstallationPath"),
    ];
    for (key, value) in keys {
        let output = std::process::Command::new("reg")
            .args(["query", key, "/s", "/v", value])
            .creation_flags(0x0800_0000)
            .output();
        let Ok(output) = output else { continue };
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix(value) {
                if let Some(path) = rest.trim().strip_prefix("REG_SZ") {
                    out.push(PathBuf::from(path.trim()).join("bin").join(JAVA_EXE));
                }
            }
        }
    }

    // 常见安装目录
    let mut roots = Vec::new();
    for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
        if let Some(pf) = std::env::var_os(var) {
            let pf = PathBuf::from(pf);
            for vendor in [
                "Java",
                "Eclipse Adoptium",
                "Eclipse Foundation",
                "AdoptOpenJDK",
                "Zulu",
                "Microsoft",
                "BellSoft",
                "Amazon Corretto",
                "Semeru",
                "Common Files\\Oracle\\Java",
            ] {
                roots.push(pf.join(vendor));
            }
        }
    }
    if let Some(appdata) = dirs::data_dir() {
        roots.push(appdata.join(".minecraft").join("runtime"));
        roots.push(appdata.join(".hmcl").join("java"));
    }
    if let Some(local) = dirs::data_local_dir() {
        // 官方启动器（微软商店版）自带的 Java
        roots.push(
            local
                .join("Packages")
                .join("Microsoft.4297127D64EC6_8wekyb3d8bbwe")
                .join("LocalCache")
                .join("Local")
                .join("runtime"),
        );
    }
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".jdks"));
    }
    for root in roots {
        collect_runtime_dir(&root, out);
    }
}

#[cfg(target_os = "macos")]
fn platform_candidates(out: &mut Vec<PathBuf>) {
    let mut roots = vec![
        PathBuf::from("/Library/Java/JavaVirtualMachines"),
        PathBuf::from("/opt/homebrew/opt"),
        PathBuf::from("/usr/local/opt"),
    ];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Library/Java/JavaVirtualMachines"));
        roots.push(home.join(".sdkman/candidates/java"));
        roots.push(home.join(".jdks"));
        roots.push(home.join("Library/Application Support/minecraft/runtime"));
    }
    for root in roots {
        collect_runtime_dir(&root, out);
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn platform_candidates(out: &mut Vec<PathBuf>) {
    let mut roots = vec![
        PathBuf::from("/usr/lib/jvm"),
        PathBuf::from("/usr/lib64/jvm"),
        PathBuf::from("/usr/java"),
        PathBuf::from("/opt/java"),
        PathBuf::from("/opt/jdk"),
    ];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".sdkman/candidates/java"));
        roots.push(home.join(".jdks"));
        roots.push(home.join(".minecraft/runtime"));
    }
    for root in roots {
        collect_runtime_dir(&root, out);
    }
}
