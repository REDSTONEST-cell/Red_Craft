//! 手机版运行环境的选择（与平台无关，便于在电脑上测试）。

use crate::error::{AppError, AppResult};

/// 手机版运行环境：打包的 Java 版本、LWJGL 版本与渲染器。
pub struct MobileRuntime {
    pub java: u32,
    pub lwjgl: &'static str,
    pub renderer: &'static str,
    pub renderer_name: &'static str,
    /// 关闭 FML 早期加载画面（26.3 起，与 FCL 相同）
    pub disable_early_window: bool,
    /// MobileGlues 使用 ANGLE（GLES on Vulkan）后端：26.3 的部分着色器直接用驱动的 GLES 无法编译
    pub angle: bool,
}

/// 游戏使用的 LWJGL 情况（从版本的依赖库中读取）。
#[derive(Default)]
pub struct LwjglUsage {
    /// `org.lwjgl:lwjgl` 的版本
    pub version: String,
    /// 使用 SDL 而不是 GLFW 管理窗口（26.3 起）
    pub sdl: bool,
}

impl LwjglUsage {
    pub fn from_libraries<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        let mut usage = LwjglUsage::default();
        let (mut sdl, mut glfw) = (false, false);
        for name in names {
            let mut parts = name.split(':');
            let (group, artifact, version) = (parts.next(), parts.next(), parts.next());
            if group != Some("org.lwjgl") {
                continue;
            }
            match artifact {
                Some("lwjgl") => usage.version = version.unwrap_or_default().to_string(),
                Some("lwjgl-sdl") => sdl = true,
                Some("lwjgl-glfw") => glfw = true,
                _ => {}
            }
        }
        usage.sdl = sdl && !glfw;
        usage
    }
}

/// 按游戏需要的 Java 选择手机上打包的运行时：
/// 1.17 ~ 1.20.4 用 Java 17，1.20.5 ~ 1.21.x 用 Java 21，26.x 用 Java 25 与 LWJGL 3.4.1。
/// 渲染器：GL4ES 只支持到 1.21.4，1.21.5 ~ 1.21.x（以及无法按版本号判断的快照）使用 NG-GL4ES；
/// 26.x 使用 MobileGlues（NG-GL4ES 在 26.2 上半透明方块显示错误且卡顿，26.3 起不再支持），
/// 26.3 起游戏改用 SDL3 管理窗口，并通过 ANGLE 编译新的透明度着色器。
pub fn mobile_runtime(required: u32, base: &str, lwjgl: &LwjglUsage) -> AppResult<MobileRuntime> {
    let java = mobile_java(required, base)?;
    let lwjgl_version = if launcher_core::version::compare_versions(&lwjgl.version, "3.4") == std::cmp::Ordering::Less {
        "3.3.3"
    } else {
        "3.4.1"
    };
    let numeric = !base.is_empty() && base.chars().all(|c| c.is_ascii_digit() || c == '.');
    let legacy_gl = numeric && launcher_core::version::compare_versions(base, "1.21.5") == std::cmp::Ordering::Less;
    let (renderer, renderer_name) = if lwjgl.sdl || lwjgl_version == "3.4.1" {
        ("mobileglues", "MobileGlues")
    } else if legacy_gl {
        ("gl4es", "GL4ES")
    } else {
        ("ng_gl4es", "NG-GL4ES")
    };
    Ok(MobileRuntime { java, lwjgl: lwjgl_version, renderer, renderer_name, disable_early_window: lwjgl.sdl, angle: lwjgl.sdl })
}

/// 游戏（或安装器）需要的 Java 对应手机上打包的哪个 Java：17、21 或 25。
pub fn mobile_java(required: u32, base: &str) -> AppResult<u32> {
    Ok(match required {
        0..=15 => {
            return Err(AppError::msg(
                format!("手机版暂不支持 {base}"),
                "手机版目前支持 1.17 及更新的版本（1.16.5 及更早的版本需要 Java 8，尚未支持）。",
            ))
        }
        16..=17 => 17,
        18..=21 => 21,
        22..=25 => 25,
        _ => {
            return Err(AppError::msg(
                format!("手机版暂不支持 {base}"),
                format!("这个版本需要 Java {required}，手机版目前最高打包 Java 25。"),
            ))
        }
    })
}

/// 游戏对内存的需求（MB）：能正常运行的最低值与合适的目标值。
#[derive(Debug, PartialEq, Eq)]
struct MemoryDemand {
    minimum: u64,
    target: u64,
}

/// 按游戏版本划分的时期：0 为 1.17 ~ 1.20.x，1 为 1.21.x（含 24w / 25w 快照），2 为 26.x 起。
fn version_era(mc: &str) -> u8 {
    let bytes = mc.as_bytes();
    // 周快照，例如 25w14a：按年份判断
    if bytes.len() >= 5 && bytes[..2].iter().all(u8::is_ascii_digit) && bytes[2] == b'w' {
        let year: u32 = mc[..2].parse().unwrap_or(0);
        return match year {
            26.. => 2,
            24..=25 => 1,
            _ => 0,
        };
    }
    let mut parts = mc.split(|c: char| !c.is_ascii_digit()).map(|p| p.parse::<u32>().unwrap_or(0));
    match (parts.next().unwrap_or(0), parts.next().unwrap_or(0)) {
        (26.., _) => 2,
        (1, 21..) => 1,
        _ => 0,
    }
}

/// 版本越新、Mod 越多，需要的内存越多。原版不需要很大的内存：堆越大，手机上的内存压力越大。
fn memory_demand(mc: &str, modded: bool, mods: usize) -> MemoryDemand {
    let (minimum, target) = match version_era(mc) {
        0 => (1024, 1536),
        1 => (1024, 2048),
        _ => (1536, 2560),
    };
    if !modded {
        return MemoryDemand { minimum, target };
    }
    MemoryDemand { minimum: minimum + 256, target: (target + 512 + mods as u64 * 16).min(6144) }
}

/// 统计 mods 文件夹中启用的 Mod 数量。
pub fn count_mods(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().to_ascii_lowercase().ends_with(".jar"))
                .count()
        })
        .unwrap_or(0)
}

/// 启动前可用内存中留给游戏堆外内存（区块缓冲、显存——手机的显存就在内存里、JVM 自身）与系统的部分。
const RESERVE_MB: u64 = 1792;

/// 手机版自动分配的游戏内存（MB）：按启动前实际可用的内存（MemAvailable）分配，先留出堆外内存与系统需要的部分，
/// 再限制在游戏需要的范围内（按版本与 Mod 数量），不超过总内存的一半，按 128MB 取整。
///
/// 不能按总内存估计：后台应用占用的内存不会全部让出来（很多手机还会把后台应用压缩到 swap 中保留），
/// 分配超过实际可用的内存时，系统在游戏运行中不断回收内存，声音断断续续、画面卡顿。
pub fn auto_memory_mb(info: launcher_core::system::MemoryInfo, mc: &str, modded: bool, mods: usize) -> u32 {
    let demand = memory_demand(mc, modded, mods);
    let align = |mb: u64| ((mb / 128) * 128).max(512) as u32;
    if info.total_mb == 0 {
        // 读不到手机内存时取目标值，最多 2GB
        return align(demand.target.min(2048));
    }
    let available = if info.available_mb > 0 { info.available_mb } else { info.total_mb / 4 };
    let budget = available.saturating_sub(RESERVE_MB);
    align(budget.clamp(demand.minimum, demand.target).min(info.total_mb / 2))
}

#[cfg(test)]
mod tests {
    use super::{auto_memory_mb, mobile_runtime, LwjglUsage};

    fn mem(total_mb: u64, available_mb: u64) -> launcher_core::system::MemoryInfo {
        launcher_core::system::MemoryInfo { total_mb, available_mb }
    }

    #[test]
    fn version_eras() {
        use super::version_era;
        assert_eq!(version_era("1.17.1"), 0);
        assert_eq!(version_era("1.20.1"), 0);
        assert_eq!(version_era("23w51b"), 0);
        assert_eq!(version_era("1.21"), 1);
        assert_eq!(version_era("1.21.11"), 1);
        assert_eq!(version_era("25w14a"), 1);
        assert_eq!(version_era("26.2"), 2);
        assert_eq!(version_era("26.4-snapshot-7"), 2);
        assert_eq!(version_era("26w02a"), 2);
        assert_eq!(version_era(""), 0);
    }

    #[test]
    fn auto_memory() {
        // 12GB 手机（系统显示约 11.4GB），后台占用多、启动前可用 3.5GB：按可用内存分配，不再按总内存估计
        let busy = mem(11400, 3500);
        assert_eq!(auto_memory_mb(busy, "26.3", false, 0), 1664);
        assert_eq!(auto_memory_mb(busy, "26.2", true, 1), 1792);
        assert_eq!(auto_memory_mb(busy, "1.21.11", false, 0), 1664);
        assert_eq!(auto_memory_mb(busy, "1.20.1", false, 0), 1536);
        // 可用内存充足时给到需要的目标值，原版不会分配过多
        let free = mem(11400, 6000);
        assert_eq!(auto_memory_mb(free, "26.3", false, 0), 2560);
        assert_eq!(auto_memory_mb(free, "1.21.11", false, 0), 2048);
        assert_eq!(auto_memory_mb(free, "1.20.1", false, 0), 1536);
        assert_eq!(auto_memory_mb(free, "26.2", true, 1), 3072);
        // 可用内存很少时给能正常运行的最低值
        assert_eq!(auto_memory_mb(mem(11400, 2000), "26.3", false, 0), 1536);
        assert_eq!(auto_memory_mb(mem(11400, 2000), "1.20.1", false, 0), 1024);
        assert_eq!(auto_memory_mb(mem(3700, 1200), "1.20.1", false, 0), 1024);
        // 不超过总内存的一半
        assert_eq!(auto_memory_mb(mem(3000, 800), "26.3", true, 50), 1408);
        // Mod 很多时增加，最多 6GB
        assert_eq!(auto_memory_mb(mem(23000, 12000), "1.21.1", true, 100), 4096);
        assert_eq!(auto_memory_mb(mem(23000, 16000), "1.21.1", true, 400), 6144);
        // 读不到内存
        assert_eq!(auto_memory_mb(mem(0, 0), "26.3", false, 0), 2048);
        assert_eq!(auto_memory_mb(mem(0, 0), "1.20.1", false, 0), 1536);
    }

    fn usage(libs: &[&str]) -> LwjglUsage {
        LwjglUsage::from_libraries(libs.iter().copied())
    }

    #[test]
    fn picks_runtime_by_java_and_version() {
        let old = usage(&["org.lwjgl:lwjgl:3.3.3", "org.lwjgl:lwjgl-glfw:3.3.3"]);
        assert!(mobile_runtime(8, "1.16.5", &old).is_err());
        let r = mobile_runtime(16, "1.17", &old).unwrap();
        assert_eq!((r.java, r.lwjgl, r.renderer), (17, "3.3.3", "gl4es"));
        let r = mobile_runtime(17, "1.20.1", &old).unwrap();
        assert_eq!((r.java, r.renderer), (17, "gl4es"));
        let r = mobile_runtime(21, "1.21.4", &old).unwrap();
        assert_eq!((r.java, r.renderer), (21, "gl4es"));
        let r = mobile_runtime(21, "1.21.11", &old).unwrap();
        assert_eq!((r.java, r.renderer), (21, "ng_gl4es"));
        let r = mobile_runtime(21, "25w14a", &old).unwrap();
        assert_eq!((r.java, r.renderer), (21, "ng_gl4es"));

        let new = usage(&["org.lwjgl:lwjgl:3.4.1", "org.lwjgl:lwjgl-glfw:3.4.1", "org.lwjgl:lwjgl-vulkan:3.4.1"]);
        let r = mobile_runtime(25, "26.2", &new).unwrap();
        assert_eq!((r.java, r.lwjgl, r.renderer), (25, "3.4.1", "mobileglues"));
        assert!(!r.angle);

        assert!(!r.disable_early_window);

        let sdl = usage(&["org.lwjgl:lwjgl:3.4.3", "org.lwjgl:lwjgl-sdl:3.4.3"]);
        assert!(sdl.sdl);
        let r = mobile_runtime(25, "26.3", &sdl).unwrap();
        assert_eq!((r.java, r.lwjgl, r.renderer), (25, "3.4.1", "mobileglues"));
        assert!(r.disable_early_window && r.angle);
        assert!(mobile_runtime(26, "27.1", &new).is_err());
    }
}
