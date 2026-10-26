//! Mod 中文名数据库（数据来自 PCL 的 mcmod.buf，原始来源 MC 百科，见 assets/README.md）。
//!
//! 用途：搜索结果与已安装 Mod 显示中文名；中文关键词搜索时换成英文名再到 Modrinth / CurseForge 搜索。

use std::collections::HashMap;
use std::io::Read;
use std::sync::OnceLock;

use serde::Serialize;

const DATA: &[u8] = include_bytes!("../../assets/mcmod.buf");

#[derive(Debug, Clone)]
pub struct ZhEntry {
    /// MC 百科的 Mod ID
    pub wiki_id: u32,
    /// 中文名（可能有多个，用 / 分隔）；没有译名时为空
    pub zh: String,
    /// 英文原名
    pub en: String,
    pub curseforge_slug: Option<String>,
    pub modrinth_slug: Option<String>,
}

pub struct ZhDb {
    entries: Vec<ZhEntry>,
    by_slug: HashMap<String, usize>,
    by_name: HashMap<String, usize>,
}

/// 中文搜索的匹配结果。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ZhMatch {
    pub wiki_id: u32,
    /// 匹配到的中文名
    pub zh: String,
    pub en: String,
    /// 实际用于搜索的英文关键词
    pub keyword: String,
    pub curseforge_slug: Option<String>,
    pub modrinth_slug: Option<String>,
    /// 中文名与搜索词完全相同
    pub exact: bool,
}

fn varint(b: &[u8], i: &mut usize) -> Option<u64> {
    let (mut r, mut s) = (0u64, 0);
    loop {
        let c = *b.get(*i)?;
        *i += 1;
        r |= u64::from(c & 0x7f) << s;
        if c < 0x80 {
            return Some(r);
        }
        s += 7;
        if s > 63 {
            return None;
        }
    }
}

/// 解析一条 protobuf 消息的字段：(字段号, 整数值或字节)。
fn fields(b: &[u8]) -> Vec<(u64, Result<u64, &[u8]>)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let Some(key) = varint(b, &mut i) else { break };
        let (f, wire) = (key >> 3, key & 7);
        match wire {
            0 => match varint(b, &mut i) {
                Some(v) => out.push((f, Ok(v))),
                None => break,
            },
            2 => {
                let Some(len) = varint(b, &mut i) else { break };
                let end = i + len as usize;
                if end > b.len() {
                    break;
                }
                out.push((f, Err(&b[i..end])));
                i = end;
            }
            _ => break,
        }
    }
    out
}

fn has_cjk(s: &str) -> bool {
    s.chars().any(|c| ('\u{3400}'..='\u{9fff}').contains(&c) || ('\u{f900}'..='\u{faff}').contains(&c))
}

/// 只保留字母与数字（小写），用于比较英文名与 slug。
pub fn name_key(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// 「中文名 (English Name)」拆成中文名与英文名。
fn split_name(full: &str) -> (String, String) {
    let full = full.trim();
    if let Some(open) = full.find(" (") {
        if full.ends_with(')') {
            let zh = full[..open].trim().to_string();
            let en = full[open + 2..full.len() - 1].trim().to_string();
            return (zh, en);
        }
    }
    if has_cjk(full) {
        (full.to_string(), String::new())
    } else {
        (String::new(), full.to_string())
    }
}

impl ZhDb {
    fn load() -> ZhDb {
        let mut raw = Vec::new();
        if flate2::read::GzDecoder::new(DATA).read_to_end(&mut raw).is_err() {
            log::warn!("Mod 中文名数据库解压失败");
            raw.clear();
        }
        let mut entries = Vec::new();
        for (f, v) in fields(&raw) {
            let (1, Err(msg)) = (f, v) else { continue };
            let mut e = ZhEntry { wiki_id: 0, zh: String::new(), en: String::new(), curseforge_slug: None, modrinth_slug: None };
            let mut full = String::new();
            for (f, v) in fields(msg) {
                match (f, v) {
                    (1, Ok(id)) => e.wiki_id = id as u32,
                    (2, Err(s)) => full = String::from_utf8_lossy(s).into_owned(),
                    (3, Err(s)) if !s.is_empty() => e.curseforge_slug = Some(String::from_utf8_lossy(s).to_lowercase()),
                    (4, Err(s)) if !s.is_empty() => e.modrinth_slug = Some(String::from_utf8_lossy(s).to_lowercase()),
                    _ => {}
                }
            }
            (e.zh, e.en) = split_name(&full);
            entries.push(e);
        }
        let mut by_slug = HashMap::new();
        let mut by_name = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            for s in [&e.curseforge_slug, &e.modrinth_slug].into_iter().flatten() {
                by_slug.entry(s.clone()).or_insert(i);
            }
            let k = name_key(&e.en);
            if !k.is_empty() {
                by_name.entry(k).or_insert(i);
            }
        }
        ZhDb { entries, by_slug, by_name }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 按 slug（两个平台通用）或英文名查找。
    pub fn find(&self, title: &str, slug: Option<&str>) -> Option<&ZhEntry> {
        slug.and_then(|s| self.by_slug.get(&s.to_lowercase()))
            .or_else(|| self.by_name.get(&name_key(title)))
            .map(|&i| &self.entries[i])
    }

    /// 资源的中文名（取第一个译名）。
    pub fn zh_name(&self, title: &str, slug: Option<&str>) -> Option<&str> {
        self.find(title, slug)
            .map(|e| e.zh.split('/').next().unwrap_or("").trim())
            .filter(|z| !z.is_empty())
    }

    /// 中文关键词搜索：返回按匹配程度排序的候选（最多 `limit` 个）。
    pub fn search(&self, query: &str, limit: usize) -> Vec<ZhMatch> {
        let q: String = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
        if q.is_empty() || !has_cjk(&q) {
            return Vec::new();
        }
        let qn = q.chars().count() as i64;
        let mut scored: Vec<(i64, usize, &str)> = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            if e.zh.is_empty() {
                continue;
            }
            let mut best = 0i64;
            let mut best_name = "";
            for name in e.zh.split('/') {
                let n: String = name.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
                if n.is_empty() {
                    continue;
                }
                let nn = n.chars().count() as i64;
                let diff = (nn - qn).abs();
                let s = if n == q {
                    1000
                } else if n.starts_with(&q) {
                    700 - diff * 8
                } else if n.contains(&q) {
                    500 - diff * 8
                } else if nn >= 2 && q.contains(&n) {
                    // 搜索词更长，例如「万用皮肤补丁mod」
                    400 - diff * 8
                } else {
                    // 字符重合度：适合少量错字、不同叫法
                    let common = q.chars().filter(|c| n.contains(*c)).count() as i64;
                    if qn >= 3 && common * 10 >= qn * 7 && common * 10 >= nn * 6 {
                        200 + common * 10 - diff * 8
                    } else {
                        0
                    }
                };
                if s > best {
                    best = s;
                    best_name = name.trim();
                }
            }
            if best > 0 {
                // 有平台 slug 的优先（能直接定位），MC 百科 ID 越小通常越知名
                let bonus = if e.modrinth_slug.is_some() || e.curseforge_slug.is_some() { 30 } else { 0 };
                scored.push((best + bonus, i, best_name));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(self.entries[a.1].wiki_id.cmp(&self.entries[b.1].wiki_id)));
        scored
            .into_iter()
            .take(limit)
            .map(|(_, i, name)| {
                let e = &self.entries[i];
                let n: String = name.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
                ZhMatch {
                    wiki_id: e.wiki_id,
                    zh: name.to_string(),
                    en: e.en.clone(),
                    keyword: keyword_of(e),
                    curseforge_slug: e.curseforge_slug.clone(),
                    modrinth_slug: e.modrinth_slug.clone(),
                    exact: n == q,
                }
            })
            .collect()
    }
}

/// 从英文名（或 slug）得到搜索用的英文关键词：去掉方括号缩写、版本后缀与标点。
fn keyword_of(e: &ZhEntry) -> String {
    let mut s = e.en.clone();
    // [CSL] 之类的缩写与「 - 副标题」
    while let (Some(a), Some(b)) = (s.find('['), s.find(']')) {
        if b <= a {
            break;
        }
        s.replace_range(a..=b, " ");
    }
    if let Some(i) = s.find(" - ") {
        s.truncate(i);
    }
    let cleaned: String = s.chars().map(|c| if c.is_alphanumeric() || c == '\'' || c == ' ' { c } else { ' ' }).collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    if !words.is_empty() {
        return words.join(" ");
    }
    e.modrinth_slug
        .as_deref()
        .or(e.curseforge_slug.as_deref())
        .map(|s| s.replace(['-', '_'], " "))
        .unwrap_or_default()
}

pub fn db() -> &'static ZhDb {
    static DB: OnceLock<ZhDb> = OnceLock::new();
    DB.get_or_init(ZhDb::load)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_database() {
        let d = db();
        assert!(d.len() > 20000, "条目数 {}", d.len());
        assert_eq!(d.zh_name("CustomSkinLoader", Some("customskinloader")), Some("万用皮肤补丁"));
        assert_eq!(d.zh_name("Create", None), Some("机械动力"));
        assert_eq!(d.zh_name("Unknown thing", Some("no-such-slug-xyz")), None);
    }

    #[test]
    fn chinese_search() {
        let d = db();
        let r = d.search("万用皮肤补丁", 5);
        assert_eq!(r[0].zh, "万用皮肤补丁");
        assert!(r[0].exact);
        assert_eq!(r[0].keyword, "CustomSkinLoader");
        assert_eq!(r[0].modrinth_slug.as_deref(), Some("customskinloader"));
        // 部分匹配与更长的搜索词
        assert_eq!(d.search("皮肤补丁", 5).iter().filter(|m| m.zh == "万用皮肤补丁").count(), 1);
        assert_eq!(d.search("万用皮肤补丁mod", 3)[0].zh, "万用皮肤补丁");
        assert_eq!(d.search("机械动力", 3)[0].keyword, "Create");
        assert!(d.search("Create", 3).is_empty(), "纯英文不转换");
    }

    #[test]
    fn names() {
        assert_eq!(split_name("工业时代2 (Industrial Craft 2)"), ("工业时代2".into(), "Industrial Craft 2".into()));
        assert_eq!(split_name("Some Mod"), ("".into(), "Some Mod".into()));
        let e = ZhEntry { wiki_id: 1, zh: "x".into(), en: "[CSL] Custom Skin Loader - Fabric".into(), curseforge_slug: None, modrinth_slug: None };
        assert_eq!(keyword_of(&e), "Custom Skin Loader");
    }
}
