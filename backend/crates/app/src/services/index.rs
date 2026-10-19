//! Chunk 级倒排索引（P1 阶段 1）。
//!
//! 分词不依赖 FTS5 / pg_trgm：中文按 bigram、拉丁字母/数字按单词，
//! 从而在 SQLite 与 PostgreSQL 上行为一致，也能匹配「医院」这类两字词。
//! 索引按 `doc_versions` 建立（版本不可变），检索时用当前 head 的版本集合过滤。

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use sea_orm::{
    sea_query::OnConflict, ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};
use uuid::Uuid;

use crate::entities::{chunk_term, doc_chunk, doc_version, search_index_state};
use crate::services::heading;

/// 单个 chunk 的字节上限（约 1300 个汉字）。
const MAX_CHUNK_BYTES: usize = 4000;
/// 单个 chunk 的行数上限，避免超大 section 变成一个巨块。
const MAX_CHUNK_LINES: usize = 200;
const MAX_TERM_CHARS: usize = 64;
/// `insert_many` 每批行数。
const INSERT_BATCH: usize = 1000;

#[derive(Debug, Clone)]
pub struct Chunk {
    pub heading: Option<String>,
    pub anchor: Option<String>,
    pub level: Option<i32>,
    pub start_line: i32,
    pub end_line: i32,
    pub text: String,
}

fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F
    )
}

fn push_term(out: &mut Vec<String>, term: String) {
    let term = term.trim();
    if term.is_empty() {
        return;
    }
    let term: String = term.chars().take(MAX_TERM_CHARS).collect();
    out.push(term);
}

/// 原始分词（保留重复，用于词频统计）。
fn raw_terms(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut cjk: Vec<char> = Vec::new();

    let flush_word = |out: &mut Vec<String>, word: &mut String| {
        if !word.is_empty() {
            push_term(out, std::mem::take(word));
        }
    };
    let flush_cjk = |out: &mut Vec<String>, cjk: &mut Vec<char>| {
        match cjk.len() {
            0 => {}
            // 单字也建索引，避免「癌」这类单字查询完全落空
            1 => push_term(out, cjk[0].to_string()),
            _ => {
                for pair in cjk.windows(2) {
                    push_term(out, format!("{}{}", pair[0], pair[1]));
                }
            }
        }
        cjk.clear();
    };

    for c in text.chars() {
        if is_cjk(c) {
            flush_word(&mut out, &mut word);
            cjk.push(c);
        } else if c.is_alphanumeric() {
            flush_cjk(&mut out, &mut cjk);
            // 大小写归一；`to_lowercase` 可能展开成多个字符（如 ß→ss）
            for lc in c.to_lowercase() {
                word.push(lc);
            }
        } else {
            flush_word(&mut out, &mut word);
            flush_cjk(&mut out, &mut cjk);
        }
    }
    flush_word(&mut out, &mut word);
    flush_cjk(&mut out, &mut cjk);
    out
}

/// 查询分词：去重并保持顺序。
pub fn query_terms(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for term in raw_terms(text) {
        if seen.insert(term.clone()) {
            out.push(term);
        }
    }
    out
}

/// term -> tf。
pub fn term_counts(text: &str) -> HashMap<String, i32> {
    let mut map = HashMap::new();
    for term in raw_terms(text) {
        *map.entry(term).or_insert(0) += 1;
    }
    map
}

fn push_split(
    out: &mut Vec<Chunk>,
    text: &str,
    start_line: usize,
    end_line: usize,
    heading: Option<String>,
    anchor: Option<String>,
    level: Option<i32>,
) {
    if text.trim().is_empty() {
        return;
    }
    let mut cur = String::new();
    let mut cur_start = start_line;
    let mut cur_lines = 0usize;

    for (i, line) in text.lines().enumerate() {
        let line_no = start_line + i;
        let would_len = cur.len() + line.len() + 1;
        if cur_lines > 0 && (cur_lines >= MAX_CHUNK_LINES || would_len > MAX_CHUNK_BYTES) {
            out.push(Chunk {
                heading: heading.clone(),
                anchor: anchor.clone(),
                level,
                start_line: cur_start as i32,
                end_line: (line_no - 1) as i32,
                text: cur.trim_end().to_string(),
            });
            cur.clear();
            cur_lines = 0;
            cur_start = line_no;
        }
        cur.push_str(line);
        cur.push('\n');
        cur_lines += 1;
    }
    if !cur.trim().is_empty() {
        out.push(Chunk {
            heading,
            anchor,
            level,
            start_line: cur_start as i32,
            end_line: end_line as i32,
            text: cur.trim_end().to_string(),
        });
    }
}

/// 按 heading 切块：前言 + 每个标题到下一个标题之前。
/// 代码块/表格由 comrak 的 outline 保证不会被误判为标题。
pub fn chunk_content(content: &str) -> Vec<Chunk> {
    let headings = heading::outline(content);
    let total = heading::line_count(content).max(1);
    let mut out = Vec::new();

    // front matter 不参与全文索引：标签/描述走独立的 doc_tags / doc_meta
    let fm_end = crate::services::metadata::front_matter_end_line(content).unwrap_or(0);
    let body_start = (fm_end + 1).max(1);

    if headings.is_empty() {
        let text = heading::slice_lines(content, body_start, total);
        push_split(&mut out, &text, body_start, total, None, None, None);
        return out;
    }

    if headings[0].line > body_start {
        let text = heading::slice_lines(content, body_start, headings[0].line - 1);
        push_split(
            &mut out,
            &text,
            body_start,
            headings[0].line - 1,
            None,
            None,
            None,
        );
    }
    for (i, h) in headings.iter().enumerate() {
        let end = headings
            .get(i + 1)
            .map(|n| n.line.saturating_sub(1))
            .unwrap_or(total)
            .max(h.line);
        let text = heading::slice_lines(content, h.line, end);
        push_split(
            &mut out,
            &text,
            h.line,
            end,
            Some(h.title.clone()),
            Some(h.slug.clone()),
            Some(h.level as i32),
        );
    }
    out
}

async fn delete_version(db: &impl ConnectionTrait, version_id: Uuid) -> Result<(), DbErr> {
    let ids: Vec<Uuid> = doc_chunk::Entity::find()
        .select_only()
        .column(doc_chunk::Column::Id)
        .filter(doc_chunk::Column::VersionId.eq(version_id))
        .into_tuple::<Uuid>()
        .all(db)
        .await?;
    for batch in ids.chunks(500) {
        chunk_term::Entity::delete_many()
            .filter(chunk_term::Column::ChunkId.is_in(batch.to_vec()))
            .exec(db)
            .await?;
    }
    doc_chunk::Entity::delete_many()
        .filter(doc_chunk::Column::VersionId.eq(version_id))
        .exec(db)
        .await?;
    search_index_state::Entity::delete_by_id(version_id)
        .exec(db)
        .await?;
    Ok(())
}

/// 为一个文档版本建立索引。幂等；已索引的版本直接返回。
pub async fn index_version(
    db: &impl ConnectionTrait,
    ver: &doc_version::Model,
) -> Result<(), DbErr> {
    // 元数据独立于 chunk 索引：已索引的旧版本回填时也会补上
    crate::services::metadata::ensure(db, ver).await?;
    if search_index_state::Entity::find_by_id(ver.id)
        .one(db)
        .await?
        .is_some()
    {
        return Ok(());
    }

    // 上次可能写到一半：先清掉残缺数据
    delete_version(db, ver.id).await?;

    let chunks = chunk_content(&ver.content);
    let now = Utc::now();
    let mut chunk_models = Vec::with_capacity(chunks.len());
    let mut term_models = Vec::new();

    for (ord, c) in chunks.iter().enumerate() {
        let chunk_id = Uuid::now_v7();
        let counts = term_counts(&c.text);
        let token_count: i32 = counts.values().sum();
        chunk_models.push(doc_chunk::ActiveModel {
            id: Set(chunk_id),
            version_id: Set(ver.id),
            document_id: Set(ver.document_id),
            ord: Set(ord as i32),
            heading: Set(c.heading.clone().map(|s| s.chars().take(1024).collect())),
            anchor: Set(c.anchor.clone().map(|s| s.chars().take(1024).collect())),
            level: Set(c.level),
            start_line: Set(c.start_line),
            end_line: Set(c.end_line),
            text: Set(c.text.clone()),
            token_count: Set(token_count),
            created_at: Set(now),
        });
        for (term, tf) in counts {
            term_models.push(chunk_term::ActiveModel {
                id: Set(Uuid::now_v7()),
                chunk_id: Set(chunk_id),
                term: Set(term),
                tf: Set(tf),
            });
        }
    }

    if !chunk_models.is_empty() {
        doc_chunk::Entity::insert_many(chunk_models)
            .exec(db)
            .await?;
    }
    for batch in term_models.chunks(INSERT_BATCH) {
        chunk_term::Entity::insert_many(batch.to_vec())
            .exec(db)
            .await?;
    }

    search_index_state::Entity::insert(search_index_state::ActiveModel {
        version_id: Set(ver.id),
        chunk_count: Set(chunks.len() as i32),
        indexed_at: Set(now),
    })
    .on_conflict(
        OnConflict::column(search_index_state::Column::VersionId)
            .do_nothing()
            .to_owned(),
    )
    .exec(db)
    .await?;

    Ok(())
}

/// 确保这些版本已索引；用于搜索时的懒索引。
pub async fn ensure_versions(db: &impl ConnectionTrait, ids: &[Uuid]) -> Result<(), DbErr> {
    if ids.is_empty() {
        return Ok(());
    }
    let mut indexed = HashSet::new();
    for batch in ids.chunks(500) {
        let rows = search_index_state::Entity::find()
            .filter(search_index_state::Column::VersionId.is_in(batch.to_vec()))
            .all(db)
            .await?;
        indexed.extend(rows.into_iter().map(|r| r.version_id));
    }
    for id in ids {
        if indexed.contains(id) {
            continue;
        }
        if let Some(v) = doc_version::Entity::find_by_id(*id).one(db).await? {
            index_version(db, &v).await?;
        }
    }
    Ok(())
}

/// 启动时后台回填历史版本。失败只记日志，不阻塞服务。
pub async fn backfill(db: DatabaseConnection) {
    let mut page = 1u64;
    loop {
        let versions = match doc_version::Entity::find()
            .order_by_asc(doc_version::Column::CreatedAt)
            .paginate(&db, 100)
            .fetch_page(page - 1)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("index backfill page {page} failed: {e}");
                return;
            }
        };
        if versions.is_empty() {
            break;
        }
        let last = versions.len() < 100;
        let ids: Vec<Uuid> = versions.iter().map(|v| v.id).collect();
        let mut indexed = HashSet::new();
        for batch in ids.chunks(500) {
            match search_index_state::Entity::find()
                .filter(search_index_state::Column::VersionId.is_in(batch.to_vec()))
                .all(&db)
                .await
            {
                Ok(rows) => indexed.extend(rows.into_iter().map(|r| r.version_id)),
                Err(e) => tracing::warn!("index backfill state lookup failed: {e}"),
            }
        }

        let mut done = 0usize;
        for v in versions {
            if indexed.contains(&v.id) {
                if let Err(e) = crate::services::metadata::ensure(&db, &v).await {
                    tracing::warn!("metadata backfill version {} failed: {e}", v.id);
                }
                continue;
            }
            match index_version(&db, &v).await {
                Ok(()) => done += 1,
                Err(e) => tracing::warn!("index backfill version {} failed: {e}", v.id),
            }
        }
        if done > 0 {
            tracing::info!("index backfill page {page}: {done} versions indexed");
        }
        if last {
            break;
        }
        page += 1;
    }
    tracing::info!("index backfill finished");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_bigrams() {
        assert_eq!(query_terms("人民医院"), vec!["人民", "民医", "医院"]);
        assert_eq!(query_terms("医"), vec!["医"]);
    }

    #[test]
    fn latin_words_and_mixed() {
        assert_eq!(
            query_terms("Rust API 与 SQLite"),
            vec!["rust", "api", "与", "sqlite"]
        );
        assert_eq!(query_terms("C++"), vec!["c"]);
    }

    #[test]
    fn term_counts_dedup_is_per_occurrence() {
        let c = term_counts("医院 医院 数据库");
        assert_eq!(c.get("医院"), Some(&2));
        assert_eq!(c.get("数据"), Some(&1));
    }

    #[test]
    fn chunks_follow_headings_and_skip_code_fence() {
        let md = "---\ntitle: t\n---\n\n# A\ntext\n\n```\n# not heading\n```\n\n## B\nmore\n";
        let chunks = chunk_content(md);
        let headings: Vec<_> = chunks
            .iter()
            .map(|c| c.heading.as_deref().unwrap_or(""))
            .collect();
        assert!(headings.contains(&"A"));
        assert!(headings.contains(&"B"));
        assert!(!headings.contains(&"not heading"));
        assert!(chunks.iter().all(|c| c.start_line <= c.end_line));
    }

    #[test]
    fn front_matter_not_indexed() {
        let md = "---\ntitle: t\ntags: [his]\n---\n\n# A\n正文\n";
        let chunks = chunk_content(md);
        assert!(chunks
            .iter()
            .all(|c| !c.text.contains("tags:") && !c.text.contains("title:")));
        assert!(chunks.iter().any(|c| c.text.contains("正文")));
    }

    #[test]
    fn long_section_is_split() {
        let mut md = String::from("# 长文\n");
        for i in 0..500 {
            md.push_str(&format!("第 {i} 行内容\n"));
        }
        let chunks = chunk_content(&md);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|c| c.text.len() <= MAX_CHUNK_BYTES + 200));
    }
}
