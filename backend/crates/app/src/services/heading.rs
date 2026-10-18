//! Markdown 标题工具：提取大纲、生成与前端一致的锚点、按标题切分文档。
//!
//! 锚点规则必须与 `frontend/src/utils/markdown.ts::slugifyHeading` 保持一致，
//! 否则 AI 返回的 `#anchor` 在阅读页里跳不过去。

use std::borrow::Cow;
use std::collections::HashMap;

use comrak::{parse_document, Arena, Options};
use serde::Serialize;

/// 一个标题在文档中的位置。
#[derive(Debug, Clone, Serialize)]
pub struct Heading {
    /// 1-6
    pub level: u8,
    pub title: String,
    /// 与前端 `id` 一致的锚点；重名标题带 `-1`、`-2` 后缀
    pub slug: String,
    /// 1-based 行号
    pub line: usize,
}

/// 前端 `slugifyHeading` 的逐字移植：小写 → 空白变 `-` → 去除非字母数字/`_`/`-`
/// → 合并连续 `-` → 去首尾 `-`；空串回退为 `section`。
pub fn slugify(text: &str) -> String {
    let mut out = String::new();
    let mut pending_space = false;
    for ch in text.trim().to_lowercase().chars() {
        if ch.is_whitespace() {
            // JS 的 `\s+` 把任意长度的空白折叠成一个 `-`
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push('-');
            pending_space = false;
        }
        if ch.is_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
        }
    }
    // 合并连续 `-`、去首尾 `-`（与 JS 的 `.replace(/-{2,}/g, '-')` 等价）
    let mut collapsed = String::with_capacity(out.len());
    for ch in out.chars() {
        if ch == '-' && collapsed.ends_with('-') {
            continue;
        }
        collapsed.push(ch);
    }
    let trimmed = collapsed.trim_matches('-');
    if trimmed.is_empty() {
        "section".to_string()
    } else {
        trimmed.to_string()
    }
}

fn inline_text(node: comrak::Node<'_>) -> String {
    let mut out = String::new();
    for n in node.descendants() {
        let data = n.data.borrow();
        match &data.value {
            comrak::nodes::NodeValue::Text(t) => out.push_str(t),
            comrak::nodes::NodeValue::Code(c) => out.push_str(&c.literal),
            comrak::nodes::NodeValue::SoftBreak | comrak::nodes::NodeValue::LineBreak => {
                out.push(' ')
            }
            _ => {}
        }
    }
    out.trim().to_string()
}

/// 与前端 `stripFrontMatter` 一致：把文档开头的 YAML front matter 行替换为
/// 空行（保留行号，避免 sourcepos 偏移）。
fn mask_front_matter(content: &str) -> Cow<'_, str> {
    let mut lines = content.lines();
    if lines.next().map(|l| l.trim_end()) != Some("---") {
        return Cow::Borrowed(content);
    }
    let body: Vec<&str> = lines.collect();
    let Some(end) = body.iter().take(50).position(|l| l.trim_end() == "---") else {
        return Cow::Borrowed(content);
    };
    // 至少一个 `key:` 才当作 front matter，否则只是普通分隔线 + 文本
    let has_key = body[..end].iter().any(|l| {
        l.split_once(':')
            .map(|(k, _)| {
                let mut chars = k.chars();
                matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
                    && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
            })
            .unwrap_or(false)
    });
    if !has_key {
        return Cow::Borrowed(content);
    }

    let mut out = String::with_capacity(content.len());
    for (i, line) in content.lines().enumerate() {
        if i <= end + 1 {
            out.push('\n');
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    Cow::Owned(out)
}

/// 解析出的标题列表（按文档顺序）。
pub fn outline(content: &str) -> Vec<Heading> {
    let masked = mask_front_matter(content);
    let content: &str = masked.as_ref();
    let arena = Arena::new();
    let root = parse_document(&arena, content, &Options::default());
    let mut out = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();

    for node in root.descendants() {
        let (level, line) = {
            let data = node.data.borrow();
            match &data.value {
                comrak::nodes::NodeValue::Heading(h) => (h.level, data.sourcepos.start.line),
                _ => continue,
            }
        };
        let title = inline_text(node);
        if title.is_empty() {
            continue;
        }
        let base = slugify(&title);
        let n = seen.entry(base.clone()).or_insert(0);
        let slug = if *n == 0 {
            base.clone()
        } else {
            format!("{base}-{n}")
        };
        *n += 1;
        out.push(Heading {
            level,
            title,
            slug,
            line: line.max(1),
        });
    }
    out
}

/// 行数（与 `str::lines` 一致：末尾换行不额外算一行）。
pub fn line_count(content: &str) -> usize {
    content.lines().count()
}

/// 取 1-based 闭区间 `[start, end]` 的行，重新用 `\n` 拼接。
pub fn slice_lines(content: &str, start: usize, end: usize) -> String {
    if start == 0 || end < start {
        return String::new();
    }
    content
        .lines()
        .skip(start - 1)
        .take(end - start + 1)
        .collect::<Vec<_>>()
        .join("\n")
}

/// 按 `section` 定位一节的范围：`(start_line, end_line, heading)`。
///
/// 匹配顺序：slug 精确 → 标题精确（忽略大小写）→ 标题包含（忽略大小写）。
/// 结束行是下一个同级或更高级标题的上一行（没有则到文末）。
pub fn section_range(content: &str, selector: &str) -> Option<(usize, usize, Heading)> {
    let sel = selector.trim().trim_start_matches('#');
    if sel.is_empty() {
        return None;
    }
    let sel_lower = sel.to_lowercase();
    let headings = outline(content);
    let idx = headings
        .iter()
        .position(|h| h.slug.eq_ignore_ascii_case(sel))
        .or_else(|| {
            headings
                .iter()
                .position(|h| h.title.to_lowercase() == sel_lower)
        })
        .or_else(|| {
            headings
                .iter()
                .position(|h| h.title.to_lowercase().contains(&sel_lower))
        })?;

    let heading = headings[idx].clone();
    let total = line_count(content).max(1);
    let end = headings
        .iter()
        .skip(idx + 1)
        .find(|h| h.level <= heading.level)
        .map(|h| h.line.saturating_sub(1))
        .unwrap_or(total);
    Some((heading.line, end.max(heading.line), heading))
}

/// 按字节上限截断，返回 `(截断后的文本, 是否截断)`；保证落在字符边界。
pub fn truncate_bytes(content: &str, max_bytes: usize) -> (String, bool) {
    if content.len() <= max_bytes {
        return (content.to_string(), false);
    }
    let mut end = max_bytes;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    (content[..end].to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_matches_frontend() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("  安装 与 部署  "), "安装-与-部署");
        assert_eq!(slugify("API: auth/login"), "api-authlogin");
        assert_eq!(slugify("!!!"), "section");
        assert_eq!(slugify("标题 1"), "标题-1");
    }

    #[test]
    fn outline_skips_code_fences() {
        let md = "# A\n\n```\n# not a heading\n```\n\n## B\n";
        let hs = outline(md);
        assert_eq!(hs.len(), 2);
        assert_eq!(hs[1].slug, "b");
    }

    #[test]
    fn section_stops_at_same_level() {
        let md = "# A\nline1\n## A1\nline2\n# B\nline3\n";
        let (s, e, h) = section_range(md, "a1").unwrap();
        assert_eq!((s, e), (3, 4));
        assert_eq!(h.slug, "a1");

        let (s, e, _) = section_range(md, "a").unwrap();
        assert_eq!((s, e), (1, 4));
    }

    #[test]
    fn front_matter_is_not_a_heading() {
        let md = "---\ntitle: 安装指南\ntags: [a, b]\n---\n\n# 安装指南\n\n## 步骤\n";
        let hs = outline(md);
        assert_eq!(hs.len(), 2);
        assert_eq!(hs[0].title, "安装指南");
        assert_eq!(hs[0].line, 6);
        assert_eq!(hs[1].slug, "步骤");
    }

    #[test]
    fn plain_rule_is_kept() {
        let md = "---\n\n# 标题\n";
        let hs = outline(md);
        assert_eq!(hs.len(), 1);
        assert_eq!(hs[0].title, "标题");
    }

    #[test]
    fn truncation_respects_char_boundary() {
        let s = "中文中文";
        let (t, cut) = truncate_bytes(s, 7);
        assert!(cut);
        assert_eq!(t, "中文");
    }
}
