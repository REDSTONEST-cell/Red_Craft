//! 手机版运行环境的选择（与平台无关，便于在电脑上测试）。

use crate::error::{AppError, AppResult};

/// 手机版运行环境：打包的 Java 版本与渲染器。
pub struct MobileRuntime {
    pub java: u32,
    pub renderer: &'static str,
    pub renderer_name: &'static str,
}

/// 按游戏需要的 Java 选择手机上打包的运行时：1.17 ~ 1.20.4 用 Java 17，1.20.5 ~ 1.21.x 用 Java 21。
/// GL4ES 只支持到 1.21.4，更新的版本（以及无法按版本号判断的快照）使用 NG-GL4ES。
pub fn mobile_runtime(required: u32, base: &str) -> AppResult<MobileRuntime> {
    let java = match required {
        0..=15 => {
            return Err(AppError::msg(
                format!("手机版暂不支持 {base}"),
                "手机版目前支持 1.17 及更新的版本（1.16.5 及更早的版本需要 Java 8，尚未支持）。",
            ))
        }
        16..=17 => 17,
        18..=21 => 21,
        _ => {
            return Err(AppError::msg(
                format!("手机版暂不支持 {base}"),
                "这个版本需要 Java 25，手机版目前支持到 1.21.x。",
            ))
        }
    };
    let numeric = !base.is_empty() && base.chars().all(|c| c.is_ascii_digit() || c == '.');
    let legacy_gl = numeric && launcher_core::version::compare_versions(base, "1.21.5") == std::cmp::Ordering::Less;
    Ok(if legacy_gl {
        MobileRuntime { java, renderer: "gl4es", renderer_name: "GL4ES" }
    } else {
        MobileRuntime { java, renderer: "ng_gl4es", renderer_name: "NG-GL4ES" }
    })
}

#[cfg(test)]
mod tests {
    use super::mobile_runtime;

    #[test]
    fn picks_runtime_by_java_and_version() {
        assert!(mobile_runtime(8, "1.16.5").is_err());
        assert!(mobile_runtime(25, "26.1").is_err());
        let r = mobile_runtime(16, "1.17").unwrap();
        assert_eq!((r.java, r.renderer), (17, "gl4es"));
        let r = mobile_runtime(17, "1.20.1").unwrap();
        assert_eq!((r.java, r.renderer), (17, "gl4es"));
        let r = mobile_runtime(21, "1.21.4").unwrap();
        assert_eq!((r.java, r.renderer), (21, "gl4es"));
        let r = mobile_runtime(21, "1.21.5").unwrap();
        assert_eq!((r.java, r.renderer), (21, "ng_gl4es"));
        let r = mobile_runtime(21, "1.21.11").unwrap();
        assert_eq!((r.java, r.renderer), (21, "ng_gl4es"));
        let r = mobile_runtime(21, "25w14a").unwrap();
        assert_eq!((r.java, r.renderer), (21, "ng_gl4es"));
    }
}
