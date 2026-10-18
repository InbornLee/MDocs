//! Front matter 元数据：tags 与 description（P1 阶段 3）。
//!
//! 只解析 YAML 的常用子集（`tags: [a, b]`、`tags:\n  - a`、`description:`），
//! 不引入 YAML 依赖；解析失败时静默忽略，不影响文档保存。

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use sea_orm::{
    sea_query::OnConflict, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, Set,
};
use uuid::Uuid;

use crate::entities::{doc_meta, doc_tag, doc_version};

const MAX_TAGS: usize = 32;
const MAX_TAG_CHARS: usize = 64;

#[derive(Debug, Clone, Default)]
pub struct ParsedMeta {
    pub description: Option<String>,
    pub tags: Vec<String>,
}

fn unquote(s: &str) -> String {
    s.trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string()
}

fn push_tag(tags: &mut Vec<String>, raw: &str) {
    let tag = unquote(raw).trim_start_matches('#').trim().to_string();
    if tag.is_empty() || tag.chars().count() > MAX_TAG_CHARS || tags.len() >= MAX_TAGS {
        return;
    }
    if !tags.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
        tags.push(tag);
    }
}

/// 取出开头的 YAML front matter 文本（不含分隔线）。
fn front_matter(content: &str) -> Option<String> {
    let mut iter = content.lines();
    if iter.next()?.trim_end() != "---" {
        return None;
    }
    let mut body = Vec::new();
    for line in iter.take(50) {
        if line.trim_end() == "---" {
            // 与前端一致：至少有一个 `key:` 才算 front matter
            if !body.iter().any(|l: &&str| l.contains(':')) {
                return None;
            }
            return Some(body.join("\n"));
        }
        body.push(line);
    }
    None
}

/// 结束分隔线所在的 1-based 行号；不是 front matter 时返回 `None`。
pub fn front_matter_end_line(content: &str) -> Option<usize> {
    let mut iter = content.lines();
    if iter.next()?.trim_end() != "---" {
        return None;
    }
    let mut body = Vec::new();
    for (i, line) in iter.take(50).enumerate() {
        if line.trim_end() == "---" {
            if !body.iter().any(|l: &&str| l.contains(':')) {
                return None;
            }
            return Some(i + 2);
        }
        body.push(line);
    }
    None
}

pub fn parse(content: &str) -> ParsedMeta {
    let Some(fm) = front_matter(content) else {
        return ParsedMeta::default();
    };
    let lines: Vec<&str> = fm.lines().collect();
    let mut meta = ParsedMeta::default();
    let mut i = 0usize;

    while i < lines.len() {
        let line = lines[i];
        let Some((key, rest)) = line.split_once(':') else {
            i += 1;
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let rest = rest.trim();
        match key.as_str() {
            "description" | "summary" => {
                if !rest.is_empty() {
                    meta.description = Some(unquote(rest));
                }
            }
            "tags" | "tag" => {
                if rest.is_empty() {
                    // 列表形式：后续若干 `- tag`
                    let mut j = i + 1;
                    while j < lines.len() {
                        let l = lines[j].trim();
                        if let Some(t) = l.strip_prefix('-') {
                            push_tag(&mut meta.tags, t);
                            j += 1;
                        } else if l.is_empty() {
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    i = j;
                    continue;
                }
                let inner = rest
                    .strip_prefix('[')
                    .and_then(|r| r.strip_suffix(']'))
                    .unwrap_or(rest);
                for part in inner.split(',') {
                    push_tag(&mut meta.tags, part);
                }
            }
            _ => {}
        }
        i += 1;
    }
    meta
}

/// 为版本补元数据；已存在则跳过。幂等。
pub async fn ensure(db: &impl ConnectionTrait, ver: &doc_version::Model) -> Result<(), DbErr> {
    if doc_meta::Entity::find_by_id(ver.id)
        .one(db)
        .await?
        .is_some()
    {
        return Ok(());
    }
    let parsed = parse(&ver.content);
    doc_meta::Entity::insert(doc_meta::ActiveModel {
        version_id: Set(ver.id),
        document_id: Set(ver.document_id),
        description: Set(parsed.description),
        created_at: Set(Utc::now()),
    })
    .on_conflict(
        OnConflict::column(doc_meta::Column::VersionId)
            .do_nothing()
            .to_owned(),
    )
    .exec(db)
    .await?;

    doc_tag::Entity::delete_many()
        .filter(doc_tag::Column::VersionId.eq(ver.id))
        .exec(db)
        .await?;
    for tag in parsed.tags {
        doc_tag::Entity::insert(doc_tag::ActiveModel {
            version_id: Set(ver.id),
            tag: Set(tag),
        })
        .on_conflict(
            OnConflict::columns([doc_tag::Column::VersionId, doc_tag::Column::Tag])
                .do_nothing()
                .to_owned(),
        )
        .exec(db)
        .await?;
    }
    Ok(())
}

/// 版本 -> 标签（已排序）。
pub async fn tags_for_versions(
    db: &impl ConnectionTrait,
    version_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<String>>, DbErr> {
    let mut out: HashMap<Uuid, Vec<String>> = HashMap::new();
    for batch in version_ids.chunks(500) {
        let rows = doc_tag::Entity::find()
            .filter(doc_tag::Column::VersionId.is_in(batch.to_vec()))
            .all(db)
            .await?;
        for r in rows {
            out.entry(r.version_id).or_default().push(r.tag);
        }
    }
    for tags in out.values_mut() {
        tags.sort();
    }
    Ok(out)
}

/// 版本 -> description。
pub async fn descriptions_for_versions(
    db: &impl ConnectionTrait,
    version_ids: &[Uuid],
) -> Result<HashMap<Uuid, Option<String>>, DbErr> {
    let mut out = HashMap::new();
    for batch in version_ids.chunks(500) {
        let rows = doc_meta::Entity::find()
            .filter(doc_meta::Column::VersionId.is_in(batch.to_vec()))
            .all(db)
            .await?;
        for r in rows {
            out.insert(r.version_id, r.description);
        }
    }
    Ok(out)
}

/// 在这些版本里，带指定标签的版本集合（大小写不敏感）。
pub async fn versions_with_tag(
    db: &impl ConnectionTrait,
    version_ids: &[Uuid],
    tag: &str,
) -> Result<HashSet<Uuid>, DbErr> {
    let tag = tag.trim().trim_start_matches('#');
    if tag.is_empty() {
        return Ok(version_ids.iter().copied().collect());
    }
    let mut out = HashSet::new();
    for batch in version_ids.chunks(500) {
        let rows = doc_tag::Entity::find()
            .filter(doc_tag::Column::VersionId.is_in(batch.to_vec()))
            .all(db)
            .await?;
        for r in rows {
            if r.tag.eq_ignore_ascii_case(tag) {
                out.insert(r.version_id);
            }
        }
    }
    Ok(out)
}

/// 标签及文档数，按数量降序、名称升序。
pub async fn tag_counts(
    db: &impl ConnectionTrait,
    version_ids: &[Uuid],
) -> Result<Vec<(String, i64)>, DbErr> {
    let by_version = tags_for_versions(db, version_ids).await?;
    let mut counts: HashMap<String, i64> = HashMap::new();
    for tags in by_version.values() {
        for tag in tags {
            *counts.entry(tag.clone()).or_insert(0) += 1;
        }
    }
    let mut out: Vec<(String, i64)> = counts.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_inline_and_list_tags() {
        let md =
            "---\ntitle: t\ntags: [his, 就诊, \"EMR\"]\ndescription: 医院信息系统\n---\n\n# 正文\n";
        let m = parse(md);
        assert_eq!(m.tags, vec!["his", "就诊", "EMR"]);
        assert_eq!(m.description.as_deref(), Some("医院信息系统"));

        let md2 = "---\ntags:\n  - a\n  - b\n---\n";
        assert_eq!(parse(md2).tags, vec!["a", "b"]);
    }

    #[test]
    fn ignores_non_front_matter_and_duplicates() {
        assert!(parse("# 标题\n\ntags: [a]\n").tags.is_empty());
        let m = parse("---\ntags: [a, A, a]\n---\n");
        assert_eq!(m.tags, vec!["a"]);
    }

    #[test]
    fn tag_count_limits() {
        let many: Vec<String> = (0..50).map(|i| format!("t{i}")).collect();
        let md = format!("---\ntags: [{}]\n---\n", many.join(","));
        assert_eq!(parse(&md).tags.len(), MAX_TAGS);
    }
}
