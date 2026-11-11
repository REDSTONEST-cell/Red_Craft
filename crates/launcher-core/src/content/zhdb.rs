//! Mod 中文名数据库（数据来自 PCL 的 mcmod.buf，原始来源 MC 百科，见 assets/README.md）。
//!
//! 用途：搜索结果与已安装 Mod 显示中文名；中文关键词搜索时换成英文名再到 Modrinth / CurseForge 搜索。

use std::io::Read;
use std::sync::OnceLock;

use serde::Serialize;

const DATA: &[u8] = include_bytes!("../../assets/mcmod.buf");

/// 数据库中的一个条目（字符串借用自数据库）。
#[derive(Debug, Clone, Copy)]
pub struct ZhEntry<'a> {
    /// MC 百科的 Mod ID
    pub wiki_id: u32,
    /// 中文名（可能有多个，用 / 分隔）；没有译名时为空
    pub zh: &'a str,
    /// 英文原名
    pub en: &'a str,
    pub curseforge_slug: Option<&'a str>,
    pub modrinth_slug: Option<&'a str>,
}

/// 一段文字在 [`ZhDb::text`] 中的位置；长度为 0 表示没有。
#[derive(Clone, Copy, Default)]
struct Span {
    at: u32,
    len: u32,
}

struct Record {
    wiki_id: u32,
    zh: Span,
    en: Span,
    curseforge_slug: Span,
    modrinth_slug: Span,
}

/// 条目的一个中文名。同一个条目的各个中文名相邻存放。
struct Name {
    entry: u32,
    /// 原样的中文名
    name: Span,
    /// 去掉空白并转小写后的形式（多数与原样相同，此时指向同一段文字）
    norm: Span,
    /// `norm` 的字符数
    chars: u16,
}

/// 数据库加载后常驻内存，所以存得很紧凑：两万多个条目的全部文字放在一个字符串里，
/// 条目只记位置；索引是按哈希排序的数组而不是以字符串为键的哈希表。
/// 相比每个字段各自分配，常驻内存从约 14MB 降到约 3MB。
pub struct ZhDb {
    text: String,
    records: Vec<Record>,
    names: Vec<Name>,
    /// (slug 的哈希, 条目序号)，按哈希、序号排序
    by_slug: Vec<(u32, u32)>,
    /// (英文名 name_key 的哈希, 条目序号)，按哈希、序号排序
    by_name: Vec<(u32, u32)>,
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

/// 去掉空白并转小写。
fn normalize(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect()
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

/// FNV-1a：索引用的哈希，结果固定（不随进程变化）。命中后还会核对原文，冲突不影响正确性。
fn hash(s: &str) -> u32 {
    s.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193))
}

/// 索引中哈希等于 `h` 的条目序号（按序号从小到大）。
fn candidates(index: &[(u32, u32)], h: u32) -> impl Iterator<Item = usize> + '_ {
    let start = index.partition_point(|&(x, _)| x < h);
    index[start..].iter().take_while(move |&&(x, _)| x == h).map(|&(_, i)| i as usize)
}

impl ZhDb {
    fn load() -> ZhDb {
        let mut raw = Vec::new();
        if flate2::read::GzDecoder::new(DATA).read_to_end(&mut raw).is_err() {
            log::warn!("Mod 中文名数据库解压失败");
            raw.clear();
        }
        let mut db = ZhDb { text: String::with_capacity(raw.len()), records: Vec::new(), names: Vec::new(), by_slug: Vec::new(), by_name: Vec::new() };
        for (f, v) in fields(&raw) {
            let (1, Err(msg)) = (f, v) else { continue };
            let mut wiki_id = 0;
            let mut full = String::new();
            let (mut curseforge_slug, mut modrinth_slug) = (Span::default(), Span::default());
            for (f, v) in fields(msg) {
                match (f, v) {
                    (1, Ok(id)) => wiki_id = id as u32,
                    (2, Err(s)) => full = String::from_utf8_lossy(s).into_owned(),
                    (3, Err(s)) if !s.is_empty() => curseforge_slug = db.push(&String::from_utf8_lossy(s).to_lowercase()),
                    (4, Err(s)) if !s.is_empty() => modrinth_slug = db.push(&String::from_utf8_lossy(s).to_lowercase()),
                    _ => {}
                }
            }
            let (zh, en) = split_name(&full);
            let entry = db.records.len() as u32;
            let (zh_span, en_span) = (db.push(&zh), db.push(&en));
            // 各个中文名是 zh 的一部分，直接指向 zh 中的位置
            for n in zh.split('/') {
                let norm = normalize(n);
                if norm.is_empty() {
                    continue;
                }
                let trimmed = n.trim();
                let name = Span { at: zh_span.at + (trimmed.as_ptr() as usize - zh.as_ptr() as usize) as u32, len: trimmed.len() as u32 };
                let chars = norm.chars().count().min(usize::from(u16::MAX)) as u16;
                let norm = if norm == trimmed { name } else { db.push(&norm) };
                db.names.push(Name { entry, name, norm, chars });
            }
            for slug in [curseforge_slug, modrinth_slug] {
                if slug.len > 0 {
                    db.by_slug.push((hash(db.get(slug)), entry));
                }
            }
            let key = name_key(&en);
            if !key.is_empty() {
                db.by_name.push((hash(&key), entry));
            }
            db.records.push(Record { wiki_id, zh: zh_span, en: en_span, curseforge_slug, modrinth_slug });
        }
        db.by_slug.sort_unstable();
        db.by_name.sort_unstable();
        db.text.shrink_to_fit();
        db.records.shrink_to_fit();
        db.names.shrink_to_fit();
        db.by_slug.shrink_to_fit();
        db.by_name.shrink_to_fit();
        db
    }

    fn push(&mut self, s: &str) -> Span {
        let at = self.text.len() as u32;
        self.text.push_str(s);
        Span { at, len: s.len() as u32 }
    }

    fn get(&self, span: Span) -> &str {
        &self.text[span.at as usize..(span.at + span.len) as usize]
    }

    fn entry(&self, i: usize) -> ZhEntry<'_> {
        let r = &self.records[i];
        let opt = |s: Span| (s.len > 0).then(|| self.get(s));
        ZhEntry {
            wiki_id: r.wiki_id,
            zh: self.get(r.zh),
            en: self.get(r.en),
            curseforge_slug: opt(r.curseforge_slug),
            modrinth_slug: opt(r.modrinth_slug),
        }
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 按 slug（两个平台通用）或英文名查找。有多个条目符合时取最靠前的一个。
    pub fn find(&self, title: &str, slug: Option<&str>) -> Option<ZhEntry<'_>> {
        let by_slug = |slug: &str| {
            let slug = slug.to_lowercase();
            if slug.is_empty() {
                return None;
            }
            candidates(&self.by_slug, hash(&slug)).find(|&i| {
                let r = &self.records[i];
                [r.curseforge_slug, r.modrinth_slug].iter().any(|s| s.len > 0 && self.get(*s) == slug)
            })
        };
        let by_name = || {
            let key = name_key(title);
            if key.is_empty() {
                return None;
            }
            candidates(&self.by_name, hash(&key)).find(|&i| name_key(self.get(self.records[i].en)) == key)
        };
        slug.and_then(by_slug).or_else(by_name).map(|i| self.entry(i))
    }

    /// 资源的中文名（取第一个译名）。
    pub fn zh_name(&self, title: &str, slug: Option<&str>) -> Option<&str> {
        self.find(title, slug)
            .map(|e| e.zh.split('/').next().unwrap_or("").trim())
            .filter(|z| !z.is_empty())
    }

    /// 中文关键词搜索：返回按匹配程度排序的候选（最多 `limit` 个）。
    pub fn search(&self, query: &str, limit: usize) -> Vec<ZhMatch> {
        let q = normalize(query);
        if q.is_empty() || !has_cjk(&q) {
            return Vec::new();
        }
        let qn = q.chars().count() as i64;
        let mut scored: Vec<(i64, usize, &str)> = Vec::new();
        let mut rest = self.names.as_slice();
        while let Some(first) = rest.first() {
            // 同一个条目的各个中文名相邻，取其中得分最高的
            let i = first.entry as usize;
            let count = rest.iter().take_while(|n| n.entry == first.entry).count();
            let (names, tail) = rest.split_at(count);
            rest = tail;
            let mut best = 0i64;
            let mut best_name = "";
            for name in names {
                let (n, nn) = (self.get(name.norm), i64::from(name.chars));
                let diff = (nn - qn).abs();
                let s = if n == q {
                    1000
                } else if n.starts_with(q.as_str()) {
                    700 - diff * 8
                } else if n.contains(q.as_str()) {
                    500 - diff * 8
                } else if nn >= 2 && q.contains(n) {
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
                    best_name = self.get(name.name);
                }
            }
            if best > 0 {
                // 有平台 slug 的优先（能直接定位），MC 百科 ID 越小通常越知名
                let r = &self.records[i];
                let bonus = if r.modrinth_slug.len > 0 || r.curseforge_slug.len > 0 { 30 } else { 0 };
                scored.push((best + bonus, i, best_name));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(self.records[a.1].wiki_id.cmp(&self.records[b.1].wiki_id)));
        scored
            .into_iter()
            .take(limit)
            .map(|(_, i, name)| {
                let e = self.entry(i);
                ZhMatch {
                    wiki_id: e.wiki_id,
                    zh: name.to_string(),
                    en: e.en.to_string(),
                    keyword: keyword_of(&e),
                    curseforge_slug: e.curseforge_slug.map(str::to_string),
                    modrinth_slug: e.modrinth_slug.map(str::to_string),
                    exact: normalize(name) == q,
                }
            })
            .collect()
    }
}

/// 从英文名（或 slug）得到搜索用的英文关键词：去掉方括号缩写、版本后缀与标点。
fn keyword_of(e: &ZhEntry) -> String {
    let mut s = e.en.to_string();
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
        .or(e.curseforge_slug)
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

    /// 数据库常驻内存的大小（Linux，读取进程 RSS），默认忽略。
    #[test]
    #[ignore]
    fn memory_footprint() {
        let rss = || {
            std::fs::read_to_string("/proc/self/status")
                .unwrap()
                .lines()
                .find(|l| l.starts_with("VmRSS"))
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap()
                .parse::<u64>()
                .unwrap()
        };
        let before = rss();
        let d = db();
        println!("条目 {}，内存增加约 {} MB", d.len(), (rss() - before) / 1024);
    }

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
        let e = ZhEntry { wiki_id: 1, zh: "x", en: "[CSL] Custom Skin Loader - Fabric", curseforge_slug: None, modrinth_slug: None };
        assert_eq!(keyword_of(&e), "Custom Skin Loader");
    }
}
