//! 版本 JSON 的数据结构。只解析启动器需要的字段，未知字段忽略。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::rules::Rule;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherits_from: Option<String>,
    /// 部分旧版 Forge 用 `jar` 指定使用哪个版本的客户端 jar
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jar: Option<String>,
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub main_class: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minecraft_arguments: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Arguments>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_index: Option<AssetIndexRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<Downloads>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_version: Option<JavaVersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging: Option<Logging>,
    #[serde(default)]
    pub release_time: String,
    #[serde(default)]
    pub time: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Argument>,
    #[serde(default)]
    pub jvm: Vec<Argument>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Argument {
    Plain(String),
    Conditional { rules: Vec<Rule>, value: ArgValue },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgValue {
    One(String),
    Many(Vec<String>),
}

impl ArgValue {
    pub fn values(&self) -> Vec<&str> {
        match self {
            ArgValue::One(s) => vec![s.as_str()],
            ArgValue::Many(v) => v.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexRef {
    pub id: String,
    #[serde(default)]
    pub sha1: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub total_size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Downloads {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<Artifact>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default)]
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    #[serde(default)]
    pub component: String,
    #[serde(default)]
    pub major_version: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Logging {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<LoggingClient>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingClient {
    pub argument: String,
    pub file: LoggingFile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingFile {
    pub id: String,
    #[serde(default)]
    pub sha1: String,
    #[serde(default)]
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<LibraryDownloads>,
    /// Maven 仓库地址（Fabric、旧版 Forge 等使用）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// 旧版 natives 写法：系统名 → classifier（可能包含 `${arch}`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub natives: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract: Option<Extract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<Rule>,
    /// 旧版 Forge：是否客户端需要
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clientreq: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LibraryDownloads {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<Artifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifiers: Option<HashMap<String, Artifact>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Extract {
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// Maven 坐标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coord<'a> {
    pub group: &'a str,
    pub artifact: &'a str,
    pub version: &'a str,
    pub classifier: Option<&'a str>,
}

impl Library {
    pub fn coord(&self) -> Option<Coord<'_>> {
        let coord = self.name.split('@').next().unwrap_or(&self.name);
        let mut parts = coord.split(':');
        Some(Coord {
            group: parts.next()?,
            artifact: parts.next()?,
            version: parts.next()?,
            classifier: parts.next(),
        })
    }

    /// 用于去重的键：`group:artifact[:classifier]`。
    pub fn dedup_key(&self) -> String {
        match self.coord() {
            Some(c) => match c.classifier {
                Some(cls) => format!("{}:{}:{}", c.group, c.artifact, cls),
                None => format!("{}:{}", c.group, c.artifact),
            },
            None => self.name.clone(),
        }
    }

    /// 1.19+ 写法的 natives 库（坐标里的 classifier 以 `natives-` 开头）。
    pub fn is_new_style_native(&self) -> bool {
        self.coord()
            .and_then(|c| c.classifier)
            .is_some_and(|c| c.starts_with("natives-"))
    }
}
