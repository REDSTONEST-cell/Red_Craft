//! 搜索结果的整理：关键词清理、相关度重排，以及 Modrinth 与 CurseForge 聚合时的去重合并。
//!
//! 排序思路参考 PCL：名称与关键词的相似度为主，下载量（取对数）为辅；
//! 两个平台的下载量数量级不同，Modrinth 的下载量乘以系数后再比较。

use std::collections::HashSet;

use super::zhdb::{db, name_key};
use super::{ContentType, Platform, ProjectHit, SortBy};

/// 清理搜索关键词：去掉 forge / fabric 等加载器名（它们会让搜索结果偏离），全被去掉时保留原文。
pub fn clean_keywords(text: &str) -> String {
    let raw = text.trim();
    let flat: String = raw.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase();
    // OptiForge / OptiFabric 拆词后会被当成加载器名
    if flat.contains("optiforge") {
        return "optiforge".into();
    }
    if flat.contains("optifabric") {
        return "optifabric".into();
    }
    let mut words: Vec<String> = Vec::new();
    for w in raw.split_whitespace() {
        let w = w.trim_matches(|c| c == '[' || c == ']');
        if w.is_empty() || ["forge", "fabric", "quilt", "neoforge", "for", "mod"].contains(&w.to_lowercase().as_str()) {
            continue;
        }
        if !words.iter().any(|x| x.eq_ignore_ascii_case(w)) {
            words.push(w.to_string());
        }
    }
    if words.is_empty() {
        raw.to_string()
    } else {
        words.join(" ")
    }
}

/// 名称与关键词的相似度（0–1）。
pub fn similarity(query: &str, name: &str) -> f64 {
    let q = name_key(query);
    let n = name_key(name);
    if q.is_empty() || n.is_empty() {
        return 0.0;
    }
    if q == n {
        return 1.0;
    }
    let ratio = q.chars().count().min(n.chars().count()) as f64 / q.chars().count().max(n.chars().count()) as f64;
    if n.contains(&q) || q.contains(&n) {
        return 0.55 + 0.4 * ratio;
    }
    // 按词重合
    let qw: Vec<String> = query.split_whitespace().map(name_key).filter(|w| !w.is_empty()).collect();
    let nw: HashSet<String> = name.split(|c: char| !c.is_alphanumeric()).map(name_key).filter(|w| !w.is_empty()).collect();
    if qw.is_empty() {
        return 0.0;
    }
    let hit = qw.iter().filter(|w| nw.contains(*w) || nw.iter().any(|x| x.starts_with(w.as_str()))).count();
    0.5 * hit as f64 / qw.len() as f64
}

fn download_mult(hit: &ProjectHit) -> f64 {
    match (hit.platform, hit.content_type) {
        (Platform::Curseforge, _) => 1.0,
        (Platform::Modrinth, ContentType::Mod | ContentType::Modpack) => 7.0,
        (Platform::Modrinth, _) => 5.0,
    }
}

/// 两个结果是否是同一个项目（来自不同平台）。
pub fn same_project(a: &ProjectHit, b: &ProjectHit) -> bool {
    if a.platform == b.platform {
        return a.id == b.id;
    }
    if !a.slug.is_empty() && a.slug.eq_ignore_ascii_case(&b.slug) {
        return true;
    }
    let (ka, kb) = (name_key(&a.title), name_key(&b.title));
    if !ka.is_empty() && ka == kb {
        return true;
    }
    if a.content_type == ContentType::Mod {
        let d = db();
        if let (Some(x), Some(y)) = (d.find(&a.title, Some(&a.slug)), d.find(&b.title, Some(&b.slug))) {
            return x.wiki_id != 0 && x.wiki_id == y.wiki_id;
        }
    }
    false
}

/// 按相关度重排（只在「相关度」排序时使用）。`zh_query` 为用户输入的中文时，按中文名比较。
pub fn rerank(hits: &mut Vec<ProjectHit>, query: &str, zh_query: Option<&str>) {
    let q = query.trim();
    let mut scored: Vec<(f64, ProjectHit)> = hits
        .drain(..)
        .map(|h| {
            let downloads = ((h.downloads.max(1) as f64) * download_mult(&h)).log10() / 9.0;
            (downloads, h)
        })
        .collect();
    if !q.is_empty() || zh_query.is_some() {
        let sims: Vec<f64> = scored
            .iter()
            .map(|(_, h)| {
                let zh = match (zh_query, h.zh_name.as_deref()) {
                    (Some(zq), Some(name)) => {
                        let a: String = zq.chars().filter(|c| !c.is_whitespace()).collect();
                        let b: String = name.chars().filter(|c| !c.is_whitespace()).collect();
                        if a == b { 1.0f64 } else if b.contains(&a) || a.contains(&b) { 0.7 } else { 0.0 }
                    }
                    _ => 0.0,
                };
                let en = similarity(q, &h.title).max(similarity(q, &h.slug.replace('-', " ")) * 0.9);
                zh.max(en)
            })
            .collect();
        let best = sims.iter().cloned().fold(0.0, f64::max);
        for ((score, h), sim) in scored.iter_mut().zip(sims) {
            if h.zh_name.is_some() {
                *score += 0.2;
            }
            if best > 0.0 {
                *score += sim / best;
            }
        }
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    hits.extend(scored.into_iter().map(|(_, h)| h));
}

/// 聚合两个平台的一页结果：去掉重复项目（保留 Modrinth 的），再按排序方式整理。
pub fn merge(modrinth: Vec<ProjectHit>, curseforge: Vec<ProjectHit>, sort: SortBy, query: &str, zh_query: Option<&str>) -> Vec<ProjectHit> {
    let mut out: Vec<ProjectHit> = Vec::with_capacity(modrinth.len() + curseforge.len());
    // 交替放入，保持两边原有的顺序
    let mut a = modrinth.into_iter();
    let mut b = curseforge.into_iter();
    loop {
        let (x, y) = (a.next(), b.next());
        if x.is_none() && y.is_none() {
            break;
        }
        for h in [x, y].into_iter().flatten() {
            if !out.iter().any(|o| same_project(o, &h)) {
                out.push(h);
            }
        }
    }
    if sort == SortBy::Relevance {
        rerank(&mut out, query, zh_query);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(platform: Platform, id: &str, slug: &str, title: &str, downloads: u64) -> ProjectHit {
        ProjectHit {
            platform,
            id: id.into(),
            slug: slug.into(),
            title: title.into(),
            description: String::new(),
            author: String::new(),
            icon_url: None,
            downloads,
            categories: vec![],
            content_type: ContentType::Mod,
            updated: String::new(),
            zh_name: None,
        }
    }

    #[test]
    fn keywords() {
        assert_eq!(clean_keywords("JEI for Forge"), "JEI");
        assert_eq!(clean_keywords("fabric"), "fabric");
        assert_eq!(clean_keywords("Opti Forge"), "optiforge");
        assert_eq!(clean_keywords("create create mod"), "create");
    }

    #[test]
    fn similarity_order() {
        assert_eq!(similarity("CustomSkinLoader", "CustomSkinLoader"), 1.0);
        assert!(similarity("sodium", "Sodium Extra") > similarity("sodium", "Lithium"));
        assert!(similarity("custom skin loader", "CustomSkinLoader") > 0.5);
    }

    #[test]
    fn merge_dedupes_and_ranks() {
        let mr = vec![
            hit(Platform::Modrinth, "a", "customskinloader", "CustomSkinLoader", 1_000_000),
            hit(Platform::Modrinth, "b", "skin-layers", "Skin Layers", 9_000_000),
        ];
        let cf = vec![
            hit(Platform::Curseforge, "1", "customskinloader", "CustomSkinLoader", 20_000_000),
            hit(Platform::Curseforge, "2", "other", "Other Mod", 100),
        ];
        let out = merge(mr, cf, SortBy::Relevance, "CustomSkinLoader", None);
        assert_eq!(out.len(), 3, "同一个 Mod 只保留一个");
        assert_eq!(out[0].id, "a", "完全匹配的排第一，且保留 Modrinth 的");
        let downloads = merge(
            vec![hit(Platform::Modrinth, "x", "x", "X", 1)],
            vec![hit(Platform::Curseforge, "y", "y", "Y", 1)],
            SortBy::Downloads,
            "",
            None,
        );
        assert_eq!(downloads.len(), 2);
        assert_eq!(downloads[0].id, "x");
    }
}
