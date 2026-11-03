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

/// 游戏对内存的需求（MB）：能正常启动的最低值，以及依次分配的三档目标。
#[derive(Debug, PartialEq, Eq)]
struct MemoryDemand {
    minimum: u64,
    targets: [u64; 3],
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

/// 版本越新、Mod 越多，需要的内存越多（档位参考 PCL：Mod 版本按 Mod 数量增加）。
fn memory_demand(mc: &str, modded: bool, mods: usize) -> MemoryDemand {
    let (minimum, targets) = match version_era(mc) {
        0 => (1024, [1536, 2560, 3584]),
        1 => (1280, [2048, 3072, 4096]),
        _ => (1536, [2560, 3584, 4608]),
    };
    if !modded {
        return MemoryDemand { minimum, targets };
    }
    let n = mods as u64;
    MemoryDemand {
        minimum: minimum + 256 + n * 4,
        targets: [targets[0] + 512 + n * 10, targets[1] + 1024 + n * 20, targets[2] + 1536 + n * 40],
    }
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

/// 手机版自动分配的游戏内存（MB）。
///
/// 1. 按游戏版本与 Mod 数量得到需求：最低值与三档目标；
/// 2. 估计游戏能用的内存：Android 内存不足时会结束后台应用，可用内存（MemAvailable）常常远小于游戏实际能用的，
///    所以至少按总内存的一半计算；再留出 1GB 给 JVM 的堆外内存、区块缓冲与显存（手机的显存就在内存里）；
/// 3. 与 PCL 相同分段分配：第一档全部满足，第二档取剩余的 70%，第三档取剩余的 40%；
/// 4. 不少于最低值，不超过总内存的一半，按 128MB 取整。
pub fn auto_memory_mb(info: launcher_core::system::MemoryInfo, mc: &str, modded: bool, mods: usize) -> u32 {
    let demand = memory_demand(mc, modded, mods);
    let align = |mb: u64| ((mb / 128) * 128).max(512) as u32;
    if info.total_mb == 0 {
        // 读不到手机内存时取第一档，最多 2GB
        return align(demand.targets[0].min(2048).max(demand.minimum));
    }
    let mut rest = info.available_mb.max(info.total_mb / 2).saturating_sub(1024) as f64;
    let (mut give, mut previous) = (0.0, 0.0);
    for (target, ratio) in demand.targets.iter().zip([1.0, 0.7, 0.4]) {
        if rest < 64.0 {
            break;
        }
        let delta = *target as f64 - previous;
        give += (rest * ratio).min(delta);
        rest -= delta / ratio;
        previous = *target as f64;
    }
    align((give as u64).max(demand.minimum).min(info.total_mb / 2))
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
        // 12GB 手机（系统显示约 11.4GB），后台占用多、可用只有 3.5GB 时仍按总内存的一半估计
        let phone = mem(11400, 3500);
        assert_eq!(auto_memory_mb(phone, "26.3", false, 0), 3840);
        assert_eq!(auto_memory_mb(phone, "26.2", true, 1), 4096);
        assert_eq!(auto_memory_mb(phone, "1.21.11", false, 0), 3456);
        assert_eq!(auto_memory_mb(phone, "1.20.1", false, 0), 3200);
        // 8GB / 6GB / 4GB 手机
        assert_eq!(auto_memory_mb(mem(7600, 2500), "26.3", false, 0), 2688);
        assert_eq!(auto_memory_mb(mem(7600, 2500), "1.20.1", false, 0), 2304);
        assert_eq!(auto_memory_mb(mem(5700, 1800), "26.3", false, 0), 1792);
        assert_eq!(auto_memory_mb(mem(3700, 1200), "1.20.1", false, 0), 1024);
        // 低内存手机不超过总内存的一半
        assert_eq!(auto_memory_mb(mem(3000, 800), "26.3", true, 50), 1408);
        // 内存充足时不超过最高一档；Mod 很多时继续增加
        assert_eq!(auto_memory_mb(mem(23000, 12000), "26.3", false, 0), 4608);
        assert_eq!(auto_memory_mb(mem(23000, 12000), "1.21.1", true, 100), 7552);
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
