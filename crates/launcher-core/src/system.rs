//! 系统信息：物理内存。

#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInfo {
    pub total_mb: u64,
    pub available_mb: u64,
}

pub fn memory() -> MemoryInfo {
    imp::memory()
}

/// 自动分配内存：原版约 2.5GB，有 Mod 时约 4~6GB，不超过总内存的一半，并参考当前可用内存。
pub fn auto_memory_mb(info: MemoryInfo, modded: bool) -> u32 {
    let want: u64 = if modded { 6144 } else { 2560 };
    let mut mb = want;
    if info.total_mb > 0 {
        mb = mb.min(info.total_mb / 2);
    }
    if info.available_mb > 0 {
        // 至少留出 1GB 给系统
        mb = mb.min(info.available_mb.saturating_sub(1024).max(1024));
    }
    // 整理成 256MB 的整数倍，下限 1GB
    ((mb / 256) * 256).max(1024) as u32
}

#[cfg(target_os = "linux")]
mod imp {
    use super::MemoryInfo;

    pub fn memory() -> MemoryInfo {
        let text = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
        let field = |key: &str| {
            text.lines()
                .find(|l| l.starts_with(key))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0)
                / 1024
        };
        MemoryInfo {
            total_mb: field("MemTotal:"),
            available_mb: field("MemAvailable:"),
        }
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use super::MemoryInfo;
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    pub fn memory() -> MemoryInfo {
        // SAFETY: MEMORYSTATUSEX 是纯数据结构，按文档设置 dwLength 后传入即可
        unsafe {
            let mut s: MEMORYSTATUSEX = std::mem::zeroed();
            s.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
            if GlobalMemoryStatusEx(&mut s) == 0 {
                return MemoryInfo::default();
            }
            MemoryInfo {
                total_mb: s.ullTotalPhys / 1024 / 1024,
                available_mb: s.ullAvailPhys / 1024 / 1024,
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::MemoryInfo;

    pub fn memory() -> MemoryInfo {
        let total = std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);
        // macOS 会积极使用缓存，可用内存难以准确获取，这里只用总内存
        MemoryInfo {
            total_mb: total / 1024 / 1024,
            available_mb: 0,
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
mod imp {
    pub fn memory() -> super::MemoryInfo {
        super::MemoryInfo::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_memory() {
        let big = MemoryInfo { total_mb: 32768, available_mb: 20000 };
        assert_eq!(auto_memory_mb(big, false), 2560);
        assert_eq!(auto_memory_mb(big, true), 6144);
        let small = MemoryInfo { total_mb: 4096, available_mb: 1500 };
        assert_eq!(auto_memory_mb(small, true), 1024);
        let mid = MemoryInfo { total_mb: 8192, available_mb: 5000 };
        assert_eq!(auto_memory_mb(mid, true), 3840);
        assert!(memory().total_mb > 0 || cfg!(not(target_os = "linux")));
    }
}
