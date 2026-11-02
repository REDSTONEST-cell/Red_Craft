//! 手机版运行环境的选择（与平台无关，便于在电脑上测试）。

use crate::error::{AppError, AppResult};

/// 手机版运行环境：打包的 Java 版本、LWJGL 版本与渲染器。
pub struct MobileRuntime {
    pub java: u32,
    pub lwjgl: &'static str,
    pub renderer: &'static str,
    pub renderer_name: &'static str,
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
/// 1.17 ~ 1.20.4 用 Java 17，1.20.5 ~ 1.21.x 用 Java 21，26.1 ~ 26.2 用 Java 25 与 LWJGL 3.4.1。
/// GL4ES 只支持到 1.21.4，更新的版本（以及无法按版本号判断的快照）使用 NG-GL4ES。
pub fn mobile_runtime(required: u32, base: &str, lwjgl: &LwjglUsage) -> AppResult<MobileRuntime> {
    let java = match required {
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
    };
    if lwjgl.sdl {
        return Err(AppError::msg(
            format!("手机版暂不支持 {base}"),
            "这个版本改用 SDL3 管理游戏窗口，手机版目前支持到 26.2。",
        ));
    }
    let lwjgl_version = if launcher_core::version::compare_versions(&lwjgl.version, "3.4") == std::cmp::Ordering::Less {
        "3.3.3"
    } else {
        "3.4.1"
    };
    let numeric = !base.is_empty() && base.chars().all(|c| c.is_ascii_digit() || c == '.');
    let legacy_gl = numeric && launcher_core::version::compare_versions(base, "1.21.5") == std::cmp::Ordering::Less;
    let (renderer, renderer_name) = if legacy_gl { ("gl4es", "GL4ES") } else { ("ng_gl4es", "NG-GL4ES") };
    Ok(MobileRuntime { java, lwjgl: lwjgl_version, renderer, renderer_name })
}

#[cfg(test)]
mod tests {
    use super::{mobile_runtime, LwjglUsage};

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
        assert_eq!((r.java, r.lwjgl, r.renderer), (25, "3.4.1", "ng_gl4es"));

        let sdl = usage(&["org.lwjgl:lwjgl:3.4.3", "org.lwjgl:lwjgl-sdl:3.4.3"]);
        assert!(sdl.sdl);
        assert!(mobile_runtime(25, "26.3", &sdl).is_err());
        assert!(mobile_runtime(26, "27.1", &new).is_err());
    }
}
