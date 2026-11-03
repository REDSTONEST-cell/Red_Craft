//! 检查 Fabric Mod 声明支持的游戏版本（`fabric.mod.json` 的 `depends.minecraft`）。
//!
//! 版本条件与 Fabric Loader 相同：数组中任一条件满足即可；一个条件中以空格分隔的部分都要满足；
//! 支持 `=`、`>`、`>=`、`<`、`<=`、`~`（主、次版本号相同）、`^`（主版本号相同）、`*` 与 `1.20.x` 这样的通配。
//! 无法判断的条件一律视为兼容，只报告确定不兼容的 Mod。

use std::cmp::Ordering;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// 声明不支持当前游戏版本的 Mod。
#[derive(Debug, Clone, PartialEq)]
pub struct IncompatibleMod {
    pub file: PathBuf,
    pub id: String,
    pub name: String,
    pub version: String,
    /// Mod 声明的游戏版本条件，例如 `~26.3-`
    pub requires: String,
}

#[derive(Debug, Clone, PartialEq)]
struct Version {
    parts: Vec<u64>,
    /// 预发布标识（`-` 之后的部分）；`26.3-` 的预发布标识为空字符串，排在所有预发布版本之前
    pre: Option<String>,
    /// 通配符所在的位置，例如 `1.20.x` 为 2
    wildcard: Option<usize>,
}

fn parse_version(text: &str) -> Option<Version> {
    let text = text.split('+').next()?.trim();
    let (core, pre) = match text.find('-') {
        Some(i) => (&text[..i], Some(text[i + 1..].to_string())),
        None => (text, None),
    };
    let mut parts = Vec::new();
    let mut wildcard = None;
    for (i, part) in core.split('.').enumerate() {
        if matches!(part, "x" | "X" | "*") {
            wildcard = Some(i);
            break;
        }
        parts.push(part.parse().ok()?);
    }
    (!parts.is_empty() || wildcard.is_some()).then_some(Version { parts, pre, wildcard })
}

fn part(v: &Version, i: usize) -> u64 {
    v.parts.get(i).copied().unwrap_or(0)
}

/// 预发布标识按语义化版本比较：数字按大小，其余按字典序，数字排在字母之前。
fn compare_pre(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.split('.'), b.split('.'));
    loop {
        match (x.next(), y.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(p), Some(q)) => {
                let o = match (p.parse::<u64>(), q.parse::<u64>()) {
                    (Ok(m), Ok(n)) => m.cmp(&n),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => p.cmp(q),
                };
                if o != Ordering::Equal {
                    return o;
                }
            }
        }
    }
}

fn compare(a: &Version, b: &Version) -> Ordering {
    for i in 0..a.parts.len().max(b.parts.len()) {
        let o = part(a, i).cmp(&part(b, i));
        if o != Ordering::Equal {
            return o;
        }
    }
    match (&a.pre, &b.pre) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(p), Some(q)) => compare_pre(p, q),
    }
}

/// 单个条件，例如 `>=1.20.1`；无法解析时返回 None。
fn test_one(v: &Version, predicate: &str) -> Option<bool> {
    let predicate = predicate.trim();
    if predicate.is_empty() || predicate == "*" {
        return Some(true);
    }
    let (op, rest) = [">=", "<=", ">", "<", "=", "~", "^"]
        .iter()
        .find_map(|op| predicate.strip_prefix(op).map(|rest| (*op, rest)))
        .unwrap_or(("", predicate));
    let r = parse_version(rest)?;
    if let Some(w) = r.wildcard {
        // 通配只用于「等于」：通配之前的部分相同即可
        return matches!(op, "" | "=").then(|| (0..w).all(|i| part(v, i) == part(&r, i)));
    }
    let o = compare(v, &r);
    Some(match op {
        ">=" => o != Ordering::Less,
        "<=" => o != Ordering::Greater,
        ">" => o == Ordering::Greater,
        "<" => o == Ordering::Less,
        "~" => o != Ordering::Less && part(v, 0) == part(&r, 0) && part(v, 1) == part(&r, 1),
        "^" => o != Ordering::Less && part(v, 0) == part(&r, 0),
        _ => o == Ordering::Equal,
    })
}

/// 游戏版本 `version` 是否满足条件 `predicates`（数组中任一满足即可）；无法判断时返回 None。
pub fn satisfies(version: &str, predicates: &[String]) -> Option<bool> {
    let v = parse_version(version).filter(|v| v.pre.is_none() && v.wildcard.is_none())?;
    let mut unknown = false;
    for predicate in predicates {
        let mut all = Some(true);
        for p in predicate.split_whitespace() {
            match test_one(&v, p) {
                Some(true) => {}
                Some(false) => {
                    all = Some(false);
                    break;
                }
                None => all = None,
            }
        }
        match all {
            Some(true) => return Some(true),
            Some(false) => {}
            None => unknown = true,
        }
    }
    if unknown || predicates.is_empty() {
        None
    } else {
        Some(false)
    }
}

/// 读取 jar 中 fabric.mod.json 的 id、名称、版本与 `depends.minecraft`。
fn fabric_info(path: &Path) -> Option<(String, String, String, Vec<String>)> {
    let file = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let mut entry = zip.by_name("fabric.mod.json").ok()?;
    if entry.size() > 1 << 20 {
        return None;
    }
    let mut data = Vec::new();
    entry.read_to_end(&mut data).ok()?;
    // 部分 Mod 的 JSON 带有控制字符，宽松处理
    let text = String::from_utf8_lossy(&data).replace(['\n', '\r', '\t'], " ");
    let v: Value = serde_json::from_str(&text).ok()?;
    let get = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let requires = match v.get("depends").and_then(|d| d.get("minecraft")) {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect(),
        _ => return None,
    };
    let id = get("id");
    let name = Some(get("name")).filter(|n| !n.is_empty()).unwrap_or_else(|| id.clone());
    Some((id, name, get("version"), requires))
}

/// mods 文件夹中（不含已禁用的）确定不支持游戏版本 `mc` 的 Fabric Mod。
/// 同一个 Mod 还有其他可用的文件时不报告（Fabric 会选用可用的那个）。
/// 只检查正式版（例如 26.2、1.21.11）；快照版本号与 Fabric 的写法不同，不检查。
pub fn incompatible_fabric_mods(dir: &Path, mc: &str) -> Vec<IncompatibleMod> {
    if mc.is_empty() || !mc.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mods: Vec<(IncompatibleMod, Option<bool>)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("jar")) && p.is_file())
        .filter_map(|file| {
            let (id, name, version, requires) = fabric_info(&file)?;
            let ok = satisfies(mc, &requires);
            Some((IncompatibleMod { file, id, name, version, requires: requires.join(" 或 ") }, ok))
        })
        .collect();
    let usable = |id: &str| mods.iter().any(|(m, ok)| m.id == id && *ok != Some(false));
    let mut out: Vec<IncompatibleMod> = mods
        .iter()
        .filter(|(m, ok)| *ok == Some(false) && !usable(&m.id))
        .map(|(m, _)| m.clone())
        .collect();
    out.sort_by_key(|m| m.name.to_lowercase());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(version: &str, predicates: &[&str]) -> Option<bool> {
        satisfies(version, &predicates.iter().map(|p| p.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn fabric_predicates() {
        // Fabric API 0.165.2+26.3 的写法
        assert_eq!(s("26.2", &["~26.3-"]), Some(false));
        assert_eq!(s("26.3", &["~26.3-"]), Some(true));
        assert_eq!(s("26.3.1", &["~26.3-"]), Some(true));
        assert_eq!(s("26.4", &["~26.3-"]), Some(false));
        assert_eq!(s("1.21.11", &["~1.21.11"]), Some(true));
        assert_eq!(s("1.21.10", &["~1.21.11"]), Some(false));
        assert_eq!(s("1.20.1", &[">=1.20 <1.21"]), Some(true));
        assert_eq!(s("1.21", &[">=1.20 <1.21"]), Some(false));
        assert_eq!(s("1.21", &[">=1.20 <1.21-"]), Some(false));
        assert_eq!(s("1.20.6", &[">=1.20.5-alpha.24.6.a"]), Some(true));
        assert_eq!(s("1.20.4", &[">=1.20.5-alpha.24.6.a"]), Some(false));
        assert_eq!(s("1.20.1", &["1.20.1"]), Some(true));
        assert_eq!(s("1.20", &["1.20.0"]), Some(true));
        assert_eq!(s("1.20.2", &["1.20.1", "1.20.2"]), Some(true));
        assert_eq!(s("1.20.3", &["1.20.1", "1.20.2"]), Some(false));
        assert_eq!(s("1.20.4", &["1.20.x"]), Some(true));
        assert_eq!(s("1.21.4", &["1.20.x"]), Some(false));
        assert_eq!(s("26.2", &["*"]), Some(true));
        assert_eq!(s("1.21.1", &["^1.20"]), Some(true));
        assert_eq!(s("2.0", &["^1.20"]), Some(false));
        assert_eq!(s("1.21.4", &[">1.21.4"]), Some(false));
        assert_eq!(s("1.21.4", &["<=1.21.4"]), Some(true));
        assert_eq!(s("26.2", &["26.3-snapshot-x"]), Some(false));
        // 无法判断时不当作不兼容
        assert_eq!(s("26.2", &["26.y"]), None);
        assert_eq!(s("26.2", &[">=foo"]), None);
        assert_eq!(s("26.2", &[]), None);
        assert_eq!(s("26.2-snapshot-1", &["~26.3-"]), None);
    }

    #[test]
    fn finds_incompatible_jars() {
        let dir = std::env::temp_dir().join(format!("rcl-compat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let jar = |name: &str, json: &str| {
            let file = std::fs::File::create(dir.join(name)).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file::<_, ()>("fabric.mod.json", zip::write::FileOptions::default()).unwrap();
            std::io::Write::write_all(&mut zip, json.as_bytes()).unwrap();
            zip.finish().unwrap();
        };
        jar("fabric-api.jar", r#"{"id":"fabric-api","name":"Fabric API","version":"0.165.2+26.3","depends":{"minecraft":"~26.3-"}}"#);
        jar("sodium.jar", r#"{"id":"sodium","name":"Sodium","version":"0.7","depends":{"minecraft":["26.2","26.3"]}}"#);
        jar("lib.jar", r#"{"id":"lib","version":"1"}"#);
        jar("old.jar.disabled", r#"{"id":"old","version":"1","depends":{"minecraft":"1.20.1"}}"#);
        // 同一个 Mod 有可用的文件时不报告不兼容的那个
        jar("iris-26.3.jar", r#"{"id":"iris","name":"Iris","version":"1.9+26.3","depends":{"minecraft":"~26.3"}}"#);
        jar("iris-26.2.jar", r#"{"id":"iris","name":"Iris","version":"1.9+26.2","depends":{"minecraft":"~26.2"}}"#);
        let bad = incompatible_fabric_mods(&dir, "26.2");
        assert_eq!(bad.len(), 1);
        assert_eq!((bad[0].id.as_str(), bad[0].requires.as_str()), ("fabric-api", "~26.3-"));
        assert!(incompatible_fabric_mods(&dir, "26.3").is_empty());
        // 同一个 Mod 的文件都不支持时全部报告
        let bad = incompatible_fabric_mods(&dir, "26.4");
        assert_eq!(bad.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), vec!["fabric-api", "iris", "iris", "sodium"]);
        assert!(incompatible_fabric_mods(&dir, "26.4-snapshot-7").is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
