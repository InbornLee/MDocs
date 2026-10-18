//! 正文全文检索：跨文档的字面量匹配、行号与片段。
//!
//! 工作台（当前分支所有文档）和公开站点（已发布文档）共用这里的匹配逻辑，
//! 各自的可见性过滤在各自的 handler 里完成。

use serde::Serialize;
use uuid::Uuid;

/// 参与检索的一篇文档
pub struct DocText<'a> {
    pub doc_id: Uuid,
    pub path: &'a str,
    pub title: &'a str,
    pub content: &'a str,
}

#[derive(Serialize)]
pub struct Hit {
    pub doc_id: Uuid,
    pub path: String,
    pub title: String,
    /// 命中所在行号（从 1 开始）
    pub line: usize,
    /// 该文档内的第几处命中（从 1 开始），前端按它在正文里定位高亮
    pub nth: usize,
    pub before: String,
    pub matched: String,
    pub after: String,
}

/// 在 `docs` 里查找 `term`，返回（命中总数、最多 `limit` 条命中、是否被截断）
pub fn run(docs: Vec<DocText<'_>>, term: &str, limit: usize) -> (usize, Vec<Hit>, bool) {
    let needle: Vec<char> = term.chars().map(lower_char).collect();
    if needle.is_empty() {
        return (0, vec![], false);
    }

    let mut hits: Vec<Hit> = Vec::new();
    let mut total = 0usize;
    for doc in docs {
        let mut from = 0usize;
        let mut nth = 0usize;
        while let Some((start, end)) = find_ci(doc.content, &needle, from) {
            nth += 1;
            total += 1;
            if hits.len() < limit {
                let (before, matched, after) = hit_snippet(doc.content, start, end, 40);
                hits.push(Hit {
                    doc_id: doc.doc_id,
                    path: doc.path.to_string(),
                    title: doc.title.to_string(),
                    line: doc.content[..start].matches('\n').count() + 1,
                    nth,
                    before,
                    matched,
                    after,
                });
            }
            from = end.max(start + 1);
        }
    }
    let truncated = total > hits.len();
    (total, hits, truncated)
}

/// 用于大小写不敏感比较的小写形式（只取首字符，避免 ß→ss 这类长度变化）
fn lower_char(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// 大小写不敏感查找，返回命中的字节范围。
/// 逐字符比较：整串 `to_lowercase()` 会改变长度，用它定位会与原文错位。
pub(crate) fn find_ci(haystack: &str, needle: &[char], from: usize) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    let chars: Vec<(usize, char)> = haystack
        .char_indices()
        .filter(|(i, _)| *i >= from)
        .map(|(i, c)| (i, lower_char(c)))
        .collect();
    if chars.len() < needle.len() {
        return None;
    }
    for start in 0..=chars.len() - needle.len() {
        if (0..needle.len()).all(|k| chars[start + k].1 == needle[k]) {
            let begin = chars[start].0;
            let end = chars
                .get(start + needle.len())
                .map(|(i, _)| *i)
                .unwrap_or(haystack.len());
            return Some((begin, end));
        }
    }
    None
}

/// 命中统计：总次数与首次命中的字节位置。
pub struct MatchStats {
    pub occurrences: usize,
    pub first_pos: Option<usize>,
}

/// 统计 `term` 在 `content` 中的出现次数与首次位置。
pub fn count_matches(content: &str, term: &str) -> MatchStats {
    let needle: Vec<char> = term.chars().map(lower_char).collect();
    if needle.is_empty() {
        return MatchStats {
            occurrences: 0,
            first_pos: None,
        };
    }
    let mut count = 0usize;
    let mut first = None;
    let mut from = 0usize;
    while let Some((start, end)) = find_ci(content, &needle, from) {
        if first.is_none() {
            first = Some(start);
        }
        count += 1;
        from = end.max(start + 1);
    }
    MatchStats {
        occurrences: count,
        first_pos: first,
    }
}

/// 用已经算好的命中统计计算相关度，避免同一次检索重复扫描正文。
pub fn relevance_with_stats(
    title: &str,
    path: &str,
    content: &str,
    term: &str,
    stats: &MatchStats,
) -> f64 {
    let needle = term.to_lowercase();
    if needle.trim().is_empty() {
        return 0.0;
    }
    let mut score = 0.0;
    if title.to_lowercase().contains(&needle) {
        score += 12.0;
    }
    if path.to_lowercase().contains(&needle) {
        score += 6.0;
    }
    score += (stats.occurrences.min(10) as f64) * 1.2;
    if let Some(pos) = stats.first_pos {
        let len = content.len().max(1) as f64;
        score += (1.0 - (pos as f64 / len)).clamp(0.0, 1.0) * 2.0;
    }
    score
}

/// 命中处前后各取 `radius` 个字符作为片段（换行折成空格）
fn hit_snippet(content: &str, start: usize, end: usize, radius: usize) -> (String, String, String) {
    let before_raw = &content[..start];
    let after_raw = &content[end..];
    let before_chars = before_raw.chars().count();
    let after_chars = after_raw.chars().count();
    let head: String = before_raw
        .chars()
        .skip(before_chars.saturating_sub(radius))
        .collect();
    let tail: String = after_raw.chars().take(radius).collect();
    let before = format!(
        "{}{}",
        if before_chars > radius { "…" } else { "" },
        head.replace('\n', " ")
    );
    let after = format!(
        "{}{}",
        tail.replace('\n', " "),
        if after_chars > radius { "…" } else { "" }
    );
    (before, content[start..end].to_string(), after)
}
