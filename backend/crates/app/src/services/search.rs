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
fn find_ci(haystack: &str, needle: &[char], from: usize) -> Option<(usize, usize)> {
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
