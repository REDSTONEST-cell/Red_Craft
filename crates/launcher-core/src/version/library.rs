//! 把版本里的 libraries 展开成具体要下载/加入 classpath/解压的文件。

use std::collections::HashSet;

use super::model::{Artifact, Library};
use super::rules::{rules_allow, Environment};
use crate::paths::maven_path;

pub const MOJANG_LIBRARIES: &str = "https://libraries.minecraft.net/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibKind {
    /// 加入 classpath
    Classpath,
    /// 需要解压到 natives 目录；`classpath` 表示是否同时加入 classpath（1.19+ 写法）
    Native {
        exclude: Vec<String>,
        classpath: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibFile {
    pub name: String,
    /// 相对 libraries 目录的路径
    pub path: String,
    /// 为空表示该文件由安装器生成，无法下载
    pub url: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub kind: LibKind,
}

impl LibFile {
    pub fn on_classpath(&self) -> bool {
        matches!(
            self.kind,
            LibKind::Classpath | LibKind::Native { classpath: true, .. }
        )
    }
}

fn from_artifact(name: &str, a: &Artifact, fallback_path: Option<String>, kind: LibKind) -> Option<LibFile> {
    let path = a.path.clone().or(fallback_path)?;
    Some(LibFile {
        name: name.to_string(),
        path,
        url: a.url.clone(),
        sha1: a.sha1.clone(),
        size: a.size,
        kind,
    })
}

/// 1.19+ natives 的架构后缀，例如 `natives-windows-arm64` → `arm64`。
fn native_arch_suffix(classifier: &str) -> Option<&'static str> {
    if classifier.ends_with("-arm64") || classifier.ends_with("-aarch_64") {
        Some("arm64")
    } else if classifier.ends_with("-x86") {
        Some("x86")
    } else if classifier.ends_with("-arm32") {
        Some("arm32")
    } else {
        None
    }
}

/// 展开 libraries。会根据 rules、操作系统和架构过滤。
pub fn plan_libraries(libs: &[Library], env: &Environment) -> Vec<LibFile> {
    // 当前架构是否存在专门的 1.19+ natives（用于决定是否保留不带后缀的 x86_64 版本）
    let arch_specific: HashSet<String> = libs
        .iter()
        .filter(|l| l.is_new_style_native() && rules_allow(&l.rules, env))
        .filter_map(|l| {
            let c = l.coord()?;
            let cls = c.classifier?;
            (native_arch_suffix(cls) == Some(env.arch.as_str()))
                .then(|| format!("{}:{}:{}", c.group, c.artifact, c.version))
        })
        .collect();

    let mut out = Vec::new();
    for lib in libs {
        if !rules_allow(&lib.rules, env) || lib.clientreq == Some(false) {
            continue;
        }

        // 旧版写法：natives 映射 + classifiers
        if let Some(natives) = &lib.natives {
            let Some(classifier) = natives.get(&env.os_name) else {
                continue;
            };
            let classifier = classifier.replace("${arch}", env.arch_bits());
            let exclude = lib
                .extract
                .as_ref()
                .map(|e| e.exclude.clone())
                .unwrap_or_default();
            let kind = LibKind::Native {
                exclude,
                classpath: false,
            };
            let artifact = lib
                .downloads
                .as_ref()
                .and_then(|d| d.classifiers.as_ref())
                .and_then(|c| c.get(&classifier));
            let fallback = maven_path(&format!("{}:{classifier}", lib.name));
            let file = match artifact {
                Some(a) => from_artifact(&lib.name, a, fallback, kind),
                None => fallback.map(|path| LibFile {
                    name: lib.name.clone(),
                    url: format!("{}{}", repo_base(lib), path),
                    path,
                    sha1: None,
                    size: None,
                    kind,
                }),
            };
            out.extend(file);
            // 有些旧库同时带有普通 artifact
            if let Some(a) = lib.downloads.as_ref().and_then(|d| d.artifact.as_ref()) {
                out.extend(from_artifact(&lib.name, a, maven_path(&lib.name), LibKind::Classpath));
            }
            continue;
        }

        let kind = if lib.is_new_style_native() {
            let coord = lib.coord().expect("is_new_style_native 已检查坐标");
            let cls = coord.classifier.unwrap_or_default();
            match native_arch_suffix(cls) {
                Some(arch) if arch != env.arch => continue,
                None if env.arch != "x86_64"
                    && arch_specific.contains(&format!(
                        "{}:{}:{}",
                        coord.group, coord.artifact, coord.version
                    )) =>
                {
                    continue
                }
                _ => {}
            }
            LibKind::Native {
                exclude: vec!["META-INF/".into()],
                classpath: true,
            }
        } else {
            LibKind::Classpath
        };

        let artifact = lib.downloads.as_ref().and_then(|d| d.artifact.as_ref());
        match artifact {
            Some(a) => out.extend(from_artifact(&lib.name, a, maven_path(&lib.name), kind)),
            None => {
                // Fabric / 旧 Forge 写法：只有坐标和仓库地址
                if let Some(path) = maven_path(&lib.name) {
                    out.push(LibFile {
                        name: lib.name.clone(),
                        url: format!("{}{}", repo_base(lib), path),
                        path,
                        sha1: lib.sha1.clone(),
                        size: lib.size,
                        kind,
                    });
                }
            }
        }
    }
    out
}

fn repo_base(lib: &Library) -> String {
    let base = lib
        .url
        .as_deref()
        .filter(|u| !u.is_empty())
        .unwrap_or(MOJANG_LIBRARIES);
    let base = base.replace("http://files.minecraftforge.net/maven", "https://maven.minecraftforge.net");
    if base.ends_with('/') {
        base
    } else {
        format!("{base}/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(os: &str, arch: &str) -> Environment {
        Environment {
            os_name: os.into(),
            os_version: "10.0".into(),
            arch: arch.into(),
            features: HashMap::new(),
        }
    }

    fn libs(json: &str) -> Vec<Library> {
        serde_json::from_str(json).unwrap()
    }

    const OLD_NATIVES: &str = r#"[{
        "name": "org.lwjgl.lwjgl:lwjgl-platform:2.9.4-nightly-20150209",
        "downloads": {
            "artifact": null,
            "classifiers": {
                "natives-linux": {"path": "l.jar", "sha1": "a", "size": 1, "url": "https://x/l.jar"},
                "natives-windows": {"path": "w.jar", "sha1": "b", "size": 2, "url": "https://x/w.jar"},
                "natives-osx": {"path": "o.jar", "sha1": "c", "size": 3, "url": "https://x/o.jar"}
            }
        },
        "extract": {"exclude": ["META-INF/"]},
        "natives": {"linux": "natives-linux", "osx": "natives-osx", "windows": "natives-windows"}
    }, {
        "name": "tv.twitch:twitch-platform:6.5",
        "downloads": {"classifiers": {
            "natives-windows-32": {"path": "t32.jar", "url": "https://x/t32.jar"},
            "natives-windows-64": {"path": "t64.jar", "url": "https://x/t64.jar"}
        }},
        "natives": {"windows": "natives-windows-${arch}"}
    }]"#;

    #[test]
    fn old_style_natives() {
        let files = plan_libraries(&libs(OLD_NATIVES), &env("windows", "x86_64"));
        let paths: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["w.jar", "t64.jar"]);
        assert!(files.iter().all(|f| !f.on_classpath()));
        assert_eq!(
            files[0].kind,
            LibKind::Native { exclude: vec!["META-INF/".into()], classpath: false }
        );

        let files = plan_libraries(&libs(OLD_NATIVES), &env("windows", "x86"));
        assert_eq!(files[1].path, "t32.jar");

        let files = plan_libraries(&libs(OLD_NATIVES), &env("linux", "x86_64"));
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "l.jar");
    }

    const NEW_NATIVES: &str = r#"[
        {"name": "org.lwjgl:lwjgl:3.3.1", "downloads": {"artifact": {"path": "lwjgl.jar", "url": "u", "sha1": "s", "size": 1}}},
        {"name": "org.lwjgl:lwjgl:3.3.1:natives-windows", "downloads": {"artifact": {"path": "win.jar", "url": "u"}}, "rules": [{"action": "allow", "os": {"name": "windows"}}]},
        {"name": "org.lwjgl:lwjgl:3.3.1:natives-windows-x86", "downloads": {"artifact": {"path": "win86.jar", "url": "u"}}, "rules": [{"action": "allow", "os": {"name": "windows"}}]},
        {"name": "org.lwjgl:lwjgl:3.3.1:natives-windows-arm64", "downloads": {"artifact": {"path": "winarm.jar", "url": "u"}}, "rules": [{"action": "allow", "os": {"name": "windows"}}]},
        {"name": "org.lwjgl:lwjgl:3.3.1:natives-macos", "downloads": {"artifact": {"path": "mac.jar", "url": "u"}}, "rules": [{"action": "allow", "os": {"name": "osx"}}]},
        {"name": "org.lwjgl:lwjgl:3.3.1:natives-macos-arm64", "downloads": {"artifact": {"path": "macarm.jar", "url": "u"}}, "rules": [{"action": "allow", "os": {"name": "osx"}}]}
    ]"#;

    fn paths(os: &str, arch: &str) -> Vec<String> {
        plan_libraries(&libs(NEW_NATIVES), &env(os, arch))
            .into_iter()
            .map(|f| f.path)
            .collect()
    }

    #[test]
    fn new_style_natives_filtered_by_arch() {
        assert_eq!(paths("windows", "x86_64"), vec!["lwjgl.jar", "win.jar"]);
        assert_eq!(paths("windows", "x86"), vec!["lwjgl.jar", "win86.jar"]);
        assert_eq!(paths("windows", "arm64"), vec!["lwjgl.jar", "winarm.jar"]);
        assert_eq!(paths("osx", "arm64"), vec!["lwjgl.jar", "macarm.jar"]);
        assert_eq!(paths("osx", "x86_64"), vec!["lwjgl.jar", "mac.jar"]);
        assert_eq!(paths("linux", "x86_64"), vec!["lwjgl.jar"]);
        let files = plan_libraries(&libs(NEW_NATIVES), &env("windows", "x86_64"));
        assert!(files[1].on_classpath());
        assert!(matches!(files[1].kind, LibKind::Native { .. }));
    }

    #[test]
    fn maven_style_library() {
        let l = libs(r#"[
            {"name": "net.fabricmc:fabric-loader:0.15.0", "url": "https://maven.fabricmc.net/", "sha1": "abc", "size": 10},
            {"name": "net.minecraftforge:forge:1.7.10", "url": "http://files.minecraftforge.net/maven/"},
            {"name": "com.mojang:realms:1.0", "serverreq": true, "clientreq": false},
            {"name": "org.ow2.asm:asm:9.6"}
        ]"#);
        let files = plan_libraries(&l, &env("windows", "x86_64"));
        assert_eq!(files.len(), 3);
        assert_eq!(
            files[0].url,
            "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.15.0/fabric-loader-0.15.0.jar"
        );
        assert_eq!(files[0].sha1.as_deref(), Some("abc"));
        assert!(files[1].url.starts_with("https://maven.minecraftforge.net/net/minecraftforge/forge/"));
        assert_eq!(
            files[2].url,
            "https://libraries.minecraft.net/org/ow2/asm/asm/9.6/asm-9.6.jar"
        );
    }
}
