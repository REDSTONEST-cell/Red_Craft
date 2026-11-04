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

/// 手机版自动分配的游戏内存（MB），与 FCL 相同（FCLGameRepository.getAllocatedMemory）：
/// 可用内存先留出 384MB，8GB 以内取 80%，超过 8GB 的部分取 20%，最多 16GB；
/// 不少于按手机总内存给的下限（MemoryUtils.findBestRAMAllocation：1GB 及以下 512MB，6GB 及以下 1GB，
/// 12GB 及以下 2GB，更多 4GB）。
pub fn auto_memory_mb(info: launcher_core::system::MemoryInfo) -> u32 {
    let minimum = match info.total_mb {
        0..=1024 => 512,
        1025..=6144 => 1024,
        6145..=12288 => 2048,
        _ => 4096,
    };
    let available = info.available_mb.saturating_sub(384);
    if available == 0 {
        return minimum;
    }
    let suggested = if available <= 8192 {
        available * 8 / 10
    } else {
        8192 * 8 / 10 + (available - 8192) * 2 / 10
    };
    (suggested.min(16384) as u32).max(minimum)
}

#[cfg(test)]
mod tests {
    use super::{auto_memory_mb, mobile_runtime, LwjglUsage};

    fn mem(total_mb: u64, available_mb: u64) -> launcher_core::system::MemoryInfo {
        launcher_core::system::MemoryInfo { total_mb, available_mb }
    }

    #[test]
    fn auto_memory_matches_fcl() {
        // 12GB 手机（系统显示约 11.1GB）：下限 2GB，其余按可用内存的 80%
        assert_eq!(auto_memory_mb(mem(11400, 4300)), 3132);
        assert_eq!(auto_memory_mb(mem(11400, 1500)), 2048);
        assert_eq!(auto_memory_mb(mem(16000, 3000)), 4096);
        assert_eq!(auto_memory_mb(mem(4000, 1000)), 1024);
        assert_eq!(auto_memory_mb(mem(24000, 18576)), 8553);
        assert_eq!(auto_memory_mb(mem(64000, 60000)), 16384);
        assert_eq!(auto_memory_mb(mem(0, 0)), 512);
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
