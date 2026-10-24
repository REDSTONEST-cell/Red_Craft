//! 版本 JSON 中 `rules` 的判断。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    Disallow,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 正则，例如 `^10\.`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub action: RuleAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<OsRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<HashMap<String, bool>>,
}

/// 规则判断所需的运行环境。
#[derive(Debug, Clone)]
pub struct Environment {
    /// `windows` / `osx` / `linux`
    pub os_name: String,
    /// 操作系统版本号，例如 `10.0`
    pub os_version: String,
    /// Mojang 写法：`x86`（32 位）、`x86_64`、`arm64`
    pub arch: String,
    pub features: HashMap<String, bool>,
}

impl Environment {
    /// 当前机器的环境。
    pub fn current() -> Self {
        let os_name = match std::env::consts::OS {
            "windows" => "windows",
            "macos" => "osx",
            _ => "linux",
        };
        let arch = match std::env::consts::ARCH {
            "x86" => "x86",
            "aarch64" => "arm64",
            "arm" => "arm32",
            _ => "x86_64",
        };
        Environment {
            os_name: os_name.into(),
            os_version: os_version(),
            arch: arch.into(),
            features: HashMap::new(),
        }
    }

    pub fn with_feature(mut self, key: &str, value: bool) -> Self {
        self.features.insert(key.into(), value);
        self
    }

    pub fn is_64bit(&self) -> bool {
        self.arch != "x86" && self.arch != "arm32"
    }

    /// 旧版 natives 分类中 `${arch}` 的值。
    pub fn arch_bits(&self) -> &'static str {
        if self.is_64bit() {
            "64"
        } else {
            "32"
        }
    }
}

fn os_version() -> String {
    #[cfg(target_os = "windows")]
    {
        // Windows 10/11 的内部版本号都是 10.0
        "10.0".to_string()
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    }
}

impl OsRule {
    fn matches(&self, env: &Environment) -> bool {
        if let Some(name) = &self.name {
            if name != &env.os_name {
                return false;
            }
        }
        if let Some(arch) = &self.arch {
            // 规则里的 x86 指 32 位
            let ok = match arch.as_str() {
                "x86" => env.arch == "x86",
                "x86_64" | "amd64" => env.arch == "x86_64",
                "arm64" | "aarch64" => env.arch == "arm64",
                other => other == env.arch,
            };
            if !ok {
                return false;
            }
        }
        if let Some(pattern) = &self.version {
            match regex_lite::Regex::new(pattern) {
                Ok(re) if re.is_match(&env.os_version) => {}
                Ok(_) => return false,
                Err(_) => return false,
            }
        }
        true
    }
}

impl Rule {
    fn matches(&self, env: &Environment) -> bool {
        if let Some(os) = &self.os {
            if !os.matches(env) {
                return false;
            }
        }
        if let Some(features) = &self.features {
            for (k, v) in features {
                if env.features.get(k).copied().unwrap_or(false) != *v {
                    return false;
                }
            }
        }
        true
    }
}

/// 判断一组规则是否允许。没有规则时默认允许；
/// 有规则时默认不允许，按顺序由最后一条匹配的规则决定结果。
pub fn rules_allow(rules: &[Rule], env: &Environment) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        if rule.matches(env) {
            allowed = rule.action == RuleAction::Allow;
        }
    }
    allowed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(os: &str, arch: &str) -> Environment {
        Environment {
            os_name: os.into(),
            os_version: "10.0".into(),
            arch: arch.into(),
            features: HashMap::new(),
        }
    }

    fn parse(json: &str) -> Vec<Rule> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn empty_rules_allow() {
        assert!(rules_allow(&[], &env("windows", "x86_64")));
    }

    #[test]
    fn allow_except_osx() {
        // 旧版 LWJGL 2 的典型写法
        let rules = parse(r#"[{"action":"allow"},{"action":"disallow","os":{"name":"osx"}}]"#);
        assert!(rules_allow(&rules, &env("windows", "x86_64")));
        assert!(rules_allow(&rules, &env("linux", "x86_64")));
        assert!(!rules_allow(&rules, &env("osx", "x86_64")));
    }

    #[test]
    fn only_osx() {
        let rules = parse(r#"[{"action":"allow","os":{"name":"osx"}}]"#);
        assert!(rules_allow(&rules, &env("osx", "arm64")));
        assert!(!rules_allow(&rules, &env("windows", "x86_64")));
    }

    #[test]
    fn arch_rule() {
        let rules = parse(r#"[{"action":"allow","os":{"arch":"x86"}}]"#);
        assert!(rules_allow(&rules, &env("windows", "x86")));
        assert!(!rules_allow(&rules, &env("windows", "x86_64")));
    }

    #[test]
    fn os_version_regex() {
        let rules = parse(r#"[{"action":"allow","os":{"name":"windows","version":"^10\\."}}]"#);
        assert!(rules_allow(&rules, &env("windows", "x86_64")));
        let mut old = env("windows", "x86_64");
        old.os_version = "6.1".into();
        assert!(!rules_allow(&rules, &old));
    }

    #[test]
    fn features() {
        let rules = parse(r#"[{"action":"allow","features":{"has_custom_resolution":true}}]"#);
        let e = env("windows", "x86_64");
        assert!(!rules_allow(&rules, &e));
        assert!(rules_allow(&rules, &e.with_feature("has_custom_resolution", true)));

        let demo = parse(r#"[{"action":"allow","features":{"is_demo_user":true}}]"#);
        assert!(!rules_allow(&demo, &env("linux", "x86_64")));
    }
}
