//! `inheritsFrom` 继承链的读取与合并。

use std::cmp::Ordering;
use std::collections::HashMap;

use super::model::{Arguments, VersionJson};
use crate::error::{CoreError, IoContext, Result};
use crate::paths::GameDir;

/// 合并后的版本信息。
#[derive(Debug, Clone)]
pub struct ResolvedVersion {
    /// 合并后的 JSON，`id` 是最外层（实例）的 id
    pub json: VersionJson,
    /// 提供客户端 jar 的版本 id（继承链最底层，或 `jar` 字段指定）
    pub jar_id: String,
    /// 继承链，从最外层到最底层
    pub chain: Vec<String>,
}

pub fn read_version_json(game: &GameDir, id: &str) -> Result<VersionJson> {
    let path = game.version_json(id);
    if !path.exists() {
        return Err(CoreError::VersionNotFound(id.to_string()));
    }
    let data = std::fs::read(&path).at(&path)?;
    serde_json::from_slice(&data).map_err(|e| CoreError::json(format!("版本文件 {id}.json"), e))
}

/// 读取并合并整个继承链。
pub fn resolve_version(game: &GameDir, id: &str) -> Result<ResolvedVersion> {
    resolve_with(id, |v| read_version_json(game, v))
}

/// 与 [`resolve_version`] 相同，但由调用者提供 JSON 读取方式（便于测试）。
pub fn resolve_with(
    id: &str,
    mut load: impl FnMut(&str) -> Result<VersionJson>,
) -> Result<ResolvedVersion> {
    let mut chain = vec![id.to_string()];
    let mut current = load(id)?;
    let mut jar_override = current.jar.clone();
    while let Some(parent_id) = current.inherits_from.clone() {
        if chain.contains(&parent_id) {
            return Err(CoreError::InvalidData(format!(
                "版本 {id} 的 inheritsFrom 存在循环引用（{}）",
                chain.join(" → ")
            )));
        }
        let parent = load(&parent_id)?;
        if jar_override.is_none() {
            jar_override = parent.jar.clone();
        }
        chain.push(parent_id);
        current = merge(current, parent);
    }
    let jar_id = jar_override.unwrap_or_else(|| chain.last().cloned().unwrap_or_default());
    current.inherits_from = None;
    current.id = id.to_string();
    Ok(ResolvedVersion {
        json: current,
        jar_id,
        chain,
    })
}

/// 把 `child` 合并到 `parent` 之上：标量字段子版本优先，参数拼接，库去重。
pub fn merge(child: VersionJson, parent: VersionJson) -> VersionJson {
    let arguments = match (parent.arguments, child.arguments) {
        (None, None) => None,
        (Some(a), None) | (None, Some(a)) => Some(a),
        (Some(p), Some(c)) => Some(Arguments {
            game: p.game.into_iter().chain(c.game).collect(),
            jvm: p.jvm.into_iter().chain(c.jvm).collect(),
        }),
    };

    // 子版本（加载器）的库放在前面，保证其优先出现在 classpath 中
    let libraries = dedup_libraries(child.libraries.into_iter().chain(parent.libraries).collect());

    VersionJson {
        id: child.id,
        inherits_from: parent.inherits_from,
        jar: child.jar.or(parent.jar),
        kind: pick(child.kind, parent.kind),
        main_class: pick(child.main_class, parent.main_class),
        minecraft_arguments: child.minecraft_arguments.or(parent.minecraft_arguments),
        arguments,
        asset_index: child.asset_index.or(parent.asset_index),
        assets: child.assets.or(parent.assets),
        downloads: child.downloads.or(parent.downloads),
        libraries,
        java_version: child.java_version.or(parent.java_version),
        logging: child.logging.or(parent.logging),
        release_time: pick(child.release_time, parent.release_time),
        time: pick(child.time, parent.time),
    }
}

fn pick(a: String, b: String) -> String {
    if a.is_empty() {
        b
    } else {
        a
    }
}

/// 同组同名（同 classifier）的库只保留版本最高的一个，位置取最先出现处。
pub fn dedup_libraries(libs: Vec<super::model::Library>) -> Vec<super::model::Library> {
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<super::model::Library> = Vec::with_capacity(libs.len());
    for lib in libs {
        // 带 natives 映射/rules 的旧库可能同名但按平台区分，必须全部保留
        if lib.natives.is_some() || !lib.rules.is_empty() {
            out.push(lib);
            continue;
        }
        let key = lib.dedup_key();
        match index.get(&key) {
            Some(&i) => {
                let old_v = out[i].coord().map(|c| c.version).unwrap_or_default();
                let new_v = lib.coord().map(|c| c.version).unwrap_or_default();
                if compare_versions(new_v, old_v) == Ordering::Greater {
                    out[i] = lib;
                }
            }
            None => {
                index.insert(key, out.len());
                out.push(lib);
            }
        }
    }
    out
}

/// 宽松的 Maven 版本比较：按 `.`、`-`、`_`、`+` 分段，数字段按数值比较。
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let split = |s: &str| -> Vec<String> {
        s.split(['.', '-', '_', '+'])
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect()
    };
    let (pa, pb) = (split(a), split(b));
    for i in 0..pa.len().max(pb.len()) {
        let ord = match (pa.get(i), pb.get(i)) {
            (Some(x), Some(y)) => match (x.parse::<u64>(), y.parse::<u64>()) {
                (Ok(nx), Ok(ny)) => nx.cmp(&ny),
                (Ok(_), Err(_)) => Ordering::Greater,
                (Err(_), Ok(_)) => Ordering::Less,
                (Err(_), Err(_)) => x.cmp(y),
            },
            // 1.0 < 1.0.1，但 1.0 > 1.0-beta
            (Some(x), None) => {
                if x.parse::<u64>().is_ok() {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            }
            (None, Some(y)) => {
                if y.parse::<u64>().is_ok() {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            }
            (None, None) => Ordering::Equal,
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::model::{Argument, Library};

    fn v(json: &str) -> VersionJson {
        serde_json::from_str(json).unwrap()
    }

    const VANILLA: &str = r#"{
        "id": "1.20.1", "type": "release", "mainClass": "net.minecraft.client.main.Main",
        "arguments": {"game": ["--username", "${auth_player_name}"], "jvm": ["-cp", "${classpath}"]},
        "assetIndex": {"id": "5", "sha1": "x", "size": 1, "totalSize": 2, "url": "https://a/5.json"},
        "assets": "5",
        "downloads": {"client": {"sha1": "c", "size": 1, "url": "https://c/client.jar"}},
        "javaVersion": {"component": "java-runtime-gamma", "majorVersion": 17},
        "libraries": [
            {"name": "org.ow2.asm:asm:9.3"},
            {"name": "com.google.guava:guava:31.1-jre"},
            {"name": "org.lwjgl:lwjgl:3.3.1"}
        ]
    }"#;

    const FABRIC: &str = r#"{
        "id": "fabric-loader-0.15.0-1.20.1", "inheritsFrom": "1.20.1", "type": "release",
        "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
        "arguments": {"game": [], "jvm": ["-DFabricMcEmu= net.minecraft.client.main.Main "]},
        "libraries": [
            {"name": "org.ow2.asm:asm:9.6", "url": "https://maven.fabricmc.net/"},
            {"name": "com.google.guava:guava:21.0", "url": "https://maven.fabricmc.net/"},
            {"name": "net.fabricmc:fabric-loader:0.15.0", "url": "https://maven.fabricmc.net/"}
        ]
    }"#;

    #[test]
    fn inherits_from_merge() {
        let r = resolve_with("fabric-loader-0.15.0-1.20.1", |id| match id {
            "1.20.1" => Ok(v(VANILLA)),
            "fabric-loader-0.15.0-1.20.1" => Ok(v(FABRIC)),
            other => Err(CoreError::VersionNotFound(other.into())),
        })
        .unwrap();
        let j = &r.json;
        assert_eq!(j.id, "fabric-loader-0.15.0-1.20.1");
        assert_eq!(r.jar_id, "1.20.1");
        assert_eq!(r.chain, vec!["fabric-loader-0.15.0-1.20.1", "1.20.1"]);
        assert_eq!(j.main_class, "net.fabricmc.loader.impl.launch.knot.KnotClient");
        assert_eq!(j.assets.as_deref(), Some("5"));
        assert_eq!(j.java_version.as_ref().unwrap().major_version, 17);
        assert!(j.downloads.as_ref().unwrap().client.is_some());

        // 参数：父在前，子在后
        let args = j.arguments.as_ref().unwrap();
        assert_eq!(args.jvm.len(), 3);
        assert_eq!(args.jvm[2], Argument::Plain("-DFabricMcEmu= net.minecraft.client.main.Main ".into()));
        assert_eq!(args.game.len(), 2);

        // 库：去重并保留高版本，子版本库在前
        let names: Vec<_> = j.libraries.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "org.ow2.asm:asm:9.6",
                "com.google.guava:guava:31.1-jre",
                "net.fabricmc:fabric-loader:0.15.0",
                "org.lwjgl:lwjgl:3.3.1",
            ]
        );
    }

    #[test]
    fn legacy_arguments_override() {
        let parent = v(r#"{"id":"1.7.10","minecraftArguments":"--username ${auth_player_name}","mainClass":"net.minecraft.client.main.Main"}"#);
        let child = v(r#"{"id":"forge","inheritsFrom":"1.7.10","minecraftArguments":"--username ${auth_player_name} --tweakClass cpw.mods.fml.common.launcher.FMLTweaker","mainClass":"net.minecraft.launchwrapper.Launch"}"#);
        let m = merge(child, parent);
        assert!(m.minecraft_arguments.unwrap().contains("FMLTweaker"));
        assert_eq!(m.main_class, "net.minecraft.launchwrapper.Launch");
    }

    #[test]
    fn missing_parent_and_cycle() {
        let err = resolve_with("a", |id| match id {
            "a" => Ok(v(r#"{"id":"a","inheritsFrom":"b"}"#)),
            _ => Err(CoreError::VersionNotFound(id.into())),
        })
        .unwrap_err();
        assert!(matches!(err, CoreError::VersionNotFound(ref s) if s == "b"));

        let err = resolve_with("a", |id| match id {
            "a" => Ok(v(r#"{"id":"a","inheritsFrom":"b"}"#)),
            _ => Ok(v(r#"{"id":"b","inheritsFrom":"a"}"#)),
        })
        .unwrap_err();
        assert!(matches!(err, CoreError::InvalidData(_)));
    }

    #[test]
    fn jar_field_override() {
        let r = resolve_with("forge", |id| match id {
            "forge" => Ok(v(r#"{"id":"forge","inheritsFrom":"1.7.10","jar":"1.7.10"}"#)),
            _ => Ok(v(r#"{"id":"1.7.10"}"#)),
        })
        .unwrap();
        assert_eq!(r.jar_id, "1.7.10");
    }

    #[test]
    fn version_compare() {
        use Ordering::*;
        assert_eq!(compare_versions("9.6", "9.3"), Greater);
        assert_eq!(compare_versions("31.1-jre", "21.0"), Greater);
        assert_eq!(compare_versions("1.10", "1.9"), Greater);
        assert_eq!(compare_versions("1.0", "1.0.1"), Less);
        assert_eq!(compare_versions("1.0", "1.0-beta"), Greater);
        assert_eq!(compare_versions("2.0", "2.0"), Equal);
    }

    #[test]
    fn dedup_keeps_classifiers_separate() {
        let libs: Vec<Library> = serde_json::from_str(
            r#"[{"name":"a:b:1"},{"name":"a:b:1:natives-linux"},{"name":"a:b:2"}]"#,
        )
        .unwrap();
        let out = dedup_libraries(libs);
        let names: Vec<_> = out.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, vec!["a:b:2", "a:b:1:natives-linux"]);
    }
}
