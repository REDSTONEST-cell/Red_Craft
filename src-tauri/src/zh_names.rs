//! 常见 Mod 的中文名（与前端共用 `src/lib/zh-names.json`），用于给下载的 Mod 文件名加上中文名。

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::settings::ModFilename;

const TABLE: &str = include_str!("../../src/lib/zh-names.json");

fn key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn names() -> &'static HashMap<String, String> {
    static NAMES: OnceLock<HashMap<String, String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let rows: Vec<Vec<String>> = serde_json::from_str(TABLE).unwrap_or_default();
        let mut map = HashMap::new();
        for row in rows {
            let [en, zh, aliases @ ..] = row.as_slice() else {
                continue;
            };
            for k in std::iter::once(en).chain(aliases) {
                map.insert(key(k), zh.clone());
            }
        }
        map
    })
}

/// 返回 Mod 的中文名；没有收录时返回 `None`。
pub fn zh_name(title: &str) -> Option<&'static str> {
    names().get(&key(title)).map(String::as_str)
}

/// 按文件名格式加上中文名：`[机械动力] create-6.0.4.jar` 或 `create-6.0.4-机械动力.jar`。
pub fn format_filename(format: ModFilename, title: &str, filename: &str) -> String {
    let Some(zh) = zh_name(title) else {
        return filename.to_string();
    };
    if filename.contains(zh) {
        return filename.to_string();
    }
    match format {
        ModFilename::Original => filename.to_string(),
        ModFilename::ZhPrefix => format!("[{zh}] {filename}"),
        ModFilename::ZhSuffix => match filename.rsplit_once('.') {
            Some((stem, ext)) => format!("{stem}-{zh}.{ext}"),
            None => format!("{filename}-{zh}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(zh_name("Create"), Some("机械动力"));
        assert_eq!(zh_name("tinkers-construct"), Some("匠魂"));
        assert_eq!(
            format_filename(ModFilename::ZhPrefix, "Create", "create-6.0.4.jar"),
            "[机械动力] create-6.0.4.jar"
        );
        assert_eq!(
            format_filename(ModFilename::ZhSuffix, "Create", "create-6.0.4.jar"),
            "create-6.0.4-机械动力.jar"
        );
        assert_eq!(
            format_filename(ModFilename::Original, "Create", "create.jar"),
            "create.jar"
        );
        assert_eq!(
            format_filename(ModFilename::ZhPrefix, "Unknown Mod", "u.jar"),
            "u.jar"
        );
        // 已经带中文名时不重复添加
        assert_eq!(
            format_filename(ModFilename::ZhPrefix, "Create", "[机械动力] c.jar"),
            "[机械动力] c.jar"
        );
    }
}
