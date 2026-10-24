//! 版本清单、版本 JSON 解析、继承合并与规则判断。

mod library;
mod manifest;
mod merge;
mod model;
mod rules;

pub use library::{plan_libraries, LibFile, LibKind, MOJANG_LIBRARIES};
pub use manifest::{Latest, ManifestEntry, VersionManifest, MANIFEST_URL};
pub use merge::{compare_versions, dedup_libraries, merge, read_version_json, resolve_version, resolve_with, ResolvedVersion};
pub use model::*;
pub use rules::{rules_allow, Environment, OsRule, Rule, RuleAction};
