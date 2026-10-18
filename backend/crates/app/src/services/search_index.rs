//! Chunk 级 BM25 检索。
//!
//! 倒排表只负责召回候选 chunk（按 tf 粗排），精确打分在应用层完成，
//! 这样 SQLite / PostgreSQL 共用一套打分逻辑，也不依赖任何扩展。

use std::collections::{HashMap, HashSet};

use sea_orm::{
    sea_query::Expr, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, FromQueryResult,
    QueryFilter, QueryOrder, QuerySelect, Statement,
};
use uuid::Uuid;

use crate::entities::{chunk_term, chunk_vector, doc_chunk};
use crate::services::{embedding, index};

const K1: f64 = 1.2;
const B: f64 = 0.75;
/// 粗排候选数；过滤掉不可见版本后仍能留下足够的精确打分对象。
const MAX_CANDIDATES: u64 = 2000;
const IN_BATCH: usize = 500;

#[derive(Debug, FromQueryResult)]
struct Candidate {
    chunk_id: Uuid,
    #[allow(dead_code)] // SQL 里用别名排序，Rust 侧不需要
    score: i64,
}

#[derive(Debug, FromQueryResult)]
struct TermDf {
    term: String,
    df: i64,
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub chunk_id: Uuid,
    pub document_id: Uuid,
    pub heading: Option<String>,
    pub anchor: Option<String>,
    pub start_line: i32,
    pub end_line: i32,
    pub snippet: String,
    pub score: f64,
}

fn boundary_before(text: &str, mut pos: usize) -> usize {
    pos = pos.min(text.len());
    while pos > 0 && !text.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

fn boundary_after(text: &str, mut pos: usize) -> usize {
    pos = pos.min(text.len());
    while pos < text.len() && !text.is_char_boundary(pos) {
        pos += 1;
    }
    pos
}

fn make_snippet(text: &str, query: &str, terms: &[String]) -> String {
    let lower = text.to_lowercase();
    let q = query.trim().to_lowercase();
    let pos = if q.is_empty() { None } else { lower.find(&q) }
        .or_else(|| terms.iter().filter_map(|t| lower.find(t)).min());

    let Some(pos) = pos else {
        return text
            .chars()
            .take(160)
            .collect::<String>()
            .replace('\n', " ");
    };
    let start = boundary_before(text, pos.saturating_sub(160));
    let end = boundary_after(text, (pos + q.len().max(1) + 240).min(text.len()));
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.push_str(text[start..end].trim());
    if end < text.len() {
        out.push('…');
    }
    out.replace('\n', " ")
}

async fn fetch_chunks(
    db: &impl ConnectionTrait,
    ids: &[Uuid],
) -> Result<Vec<doc_chunk::Model>, DbErr> {
    let mut out = Vec::new();
    for batch in ids.chunks(IN_BATCH) {
        out.extend(
            doc_chunk::Entity::find()
                .filter(doc_chunk::Column::Id.is_in(batch.to_vec()))
                .all(db)
                .await?,
        );
    }
    Ok(out)
}

async fn fetch_terms(
    db: &impl ConnectionTrait,
    chunk_ids: &[Uuid],
    terms: &[String],
) -> Result<Vec<chunk_term::Model>, DbErr> {
    let mut out = Vec::new();
    for batch in chunk_ids.chunks(IN_BATCH) {
        out.extend(
            chunk_term::Entity::find()
                .filter(chunk_term::Column::ChunkId.is_in(batch.to_vec()))
                .filter(chunk_term::Column::Term.is_in(terms.to_vec()))
                .all(db)
                .await?,
        );
    }
    Ok(out)
}

/// 全局统计（chunk 总数、平均长度）。索引量大时可加缓存，P1 先每次查询。
async fn global_stats(db: &impl ConnectionTrait) -> Result<(i64, f64), DbErr> {
    let stmt = Statement::from_string(
        db.get_database_backend(),
        "SELECT COUNT(*) AS n, COALESCE(SUM(token_count), 0) AS t FROM doc_chunks".to_string(),
    );
    let Some(row) = db.query_one(stmt).await? else {
        return Ok((0, 1.0));
    };
    let n: i64 = row.try_get("", "n").unwrap_or(0);
    let total: i64 = row.try_get("", "t").unwrap_or(0);
    let avgdl = if n > 0 { total as f64 / n as f64 } else { 1.0 };
    Ok((n, avgdl.max(1.0)))
}

/// 纯 BM25 检索（无 embedding 或向量失败时的基线）。
async fn bm25_search(
    db: &impl ConnectionTrait,
    version_ids: &[Uuid],
    query: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<Hit>, DbErr> {
    if version_ids.is_empty() || limit == 0 {
        return Ok(vec![]);
    }
    let terms = index::query_terms(query);
    if terms.is_empty() {
        return Ok(vec![]);
    }
    let visible: HashSet<Uuid> = version_ids.iter().copied().collect();

    // 1) 按 tf 粗排候选 chunk（走 term 索引）
    let candidates = chunk_term::Entity::find()
        .select_only()
        .column(chunk_term::Column::ChunkId)
        .column_as(chunk_term::Column::Tf.sum(), "score")
        .filter(chunk_term::Column::Term.is_in(terms.clone()))
        .group_by(chunk_term::Column::ChunkId)
        .order_by_desc(Expr::cust("score"))
        .limit(MAX_CANDIDATES)
        .into_model::<Candidate>()
        .all(db)
        .await?;
    if candidates.is_empty() {
        return Ok(vec![]);
    }
    let chunk_ids: Vec<Uuid> = candidates.iter().map(|c| c.chunk_id).collect();

    // 2) 取 chunk 文本并按当前 head 的版本过滤
    let chunks: Vec<doc_chunk::Model> = fetch_chunks(db, &chunk_ids)
        .await?
        .into_iter()
        .filter(|c| visible.contains(&c.version_id))
        .collect();
    if chunks.is_empty() {
        return Ok(vec![]);
    }

    // 3) df / 全局统计
    let dfs: HashMap<String, i64> = chunk_term::Entity::find()
        .select_only()
        .column(chunk_term::Column::Term)
        .column_as(chunk_term::Column::ChunkId.count(), "df")
        .filter(chunk_term::Column::Term.is_in(terms.clone()))
        .group_by(chunk_term::Column::Term)
        .into_model::<TermDf>()
        .all(db)
        .await?
        .into_iter()
        .map(|r| (r.term, r.df))
        .collect();
    let (n, avgdl) = global_stats(db).await?;
    let n = n.max(1) as f64;

    // 4) 精确 BM25
    let chunk_ids: Vec<Uuid> = chunks.iter().map(|c| c.id).collect();
    let term_rows = fetch_terms(db, &chunk_ids, &terms).await?;
    let mut tf_map: HashMap<Uuid, HashMap<String, i32>> = HashMap::new();
    for row in term_rows {
        tf_map
            .entry(row.chunk_id)
            .or_default()
            .insert(row.term, row.tf);
    }

    let mut hits = Vec::new();
    for c in chunks {
        let Some(tfs) = tf_map.get(&c.id) else {
            continue;
        };
        let dl = c.token_count.max(1) as f64;
        let mut score = 0.0;
        for (term, tf) in tfs {
            let df = *dfs.get(term).unwrap_or(&0) as f64;
            if df <= 0.0 {
                continue;
            }
            let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
            let tf = *tf as f64;
            score += idf * (tf * (K1 + 1.0)) / (tf + K1 * (1.0 - B + B * dl / avgdl));
        }
        if score <= 0.0 {
            continue;
        }
        hits.push(Hit {
            chunk_id: c.id,
            document_id: c.document_id,
            heading: c.heading,
            anchor: c.anchor,
            start_line: c.start_line,
            end_line: c.end_line,
            snippet: make_snippet(&c.text, query, &terms),
            score,
        });
    }

    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.chunk_id.cmp(&b.chunk_id))
    });
    let start = offset.min(hits.len());
    hits.truncate(start.saturating_add(limit));
    Ok(hits.into_iter().skip(start).collect())
}

/// 兼容入口：纯 BM25（后续 REST 搜索切换时使用）。
#[allow(dead_code)]
pub async fn search(
    db: &impl ConnectionTrait,
    version_ids: &[Uuid],
    query: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<Hit>, DbErr> {
    bm25_search(db, version_ids, query, limit, offset).await
}

const RRF_K: f64 = 60.0;

fn paginate(mut hits: Vec<Hit>, offset: usize, limit: usize) -> Vec<Hit> {
    let start = offset.min(hits.len());
    hits.truncate(start.saturating_add(limit));
    hits.into_iter().skip(start).collect()
}

fn hit_from_chunk(c: &doc_chunk::Model, query: &str, terms: &[String]) -> Hit {
    Hit {
        chunk_id: c.id,
        document_id: c.document_id,
        heading: c.heading.clone(),
        anchor: c.anchor.clone(),
        start_line: c.start_line,
        end_line: c.end_line,
        snippet: make_snippet(&c.text, query, terms),
        score: 0.0,
    }
}

async fn vector_search(
    db: &impl ConnectionTrait,
    version_ids: &[Uuid],
    model: &str,
    query_vec: &[f32],
    limit: usize,
) -> Result<Vec<(Uuid, f32)>, DbErr> {
    let model = model.to_string();
    let mut scored: HashMap<Uuid, f32> = HashMap::new();
    for batch in version_ids.chunks(IN_BATCH) {
        let rows: Vec<(Uuid, Vec<u8>)> = chunk_vector::Entity::find()
            .inner_join(doc_chunk::Entity)
            .filter(doc_chunk::Column::VersionId.is_in(batch.to_vec()))
            .filter(chunk_vector::Column::Model.eq(&model))
            .select_only()
            .column(chunk_vector::Column::ChunkId)
            .column(chunk_vector::Column::Vector)
            .into_tuple::<(Uuid, Vec<u8>)>()
            .all(db)
            .await?;
        for (id, bytes) in rows {
            let v = embedding::decode(&bytes);
            let s = embedding::cosine(query_vec, &v);
            scored
                .entry(id)
                .and_modify(|e| {
                    if s > *e {
                        *e = s;
                    }
                })
                .or_insert(s);
        }
    }
    let mut list: Vec<(Uuid, f32)> = scored.into_iter().collect();
    list.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    list.truncate(limit);
    Ok(list)
}

/// 混合检索：BM25 + 向量，RRF 融合。
///
/// `query_vec` / `model` 为 `None`（未配置 embedding 或查询向量失败）时自动回退到 BM25。
pub async fn search_hybrid(
    db: &impl ConnectionTrait,
    version_ids: &[Uuid],
    query: &str,
    limit: usize,
    offset: usize,
    query_vec: Option<&[f32]>,
    model: Option<&str>,
) -> Result<Vec<Hit>, DbErr> {
    let (Some(query_vec), Some(model)) = (query_vec, model) else {
        return bm25_search(db, version_ids, query, limit, offset).await;
    };
    if version_ids.is_empty() || limit == 0 {
        return Ok(vec![]);
    }

    let candidate_limit = (limit + offset).max(50);
    let bm25 = bm25_search(db, version_ids, query, candidate_limit, 0).await?;
    let bm25_ids: Vec<Uuid> = bm25.iter().map(|h| h.chunk_id).collect();
    let mut by_id: HashMap<Uuid, Hit> = bm25.into_iter().map(|h| (h.chunk_id, h)).collect();

    let vec_hits = match vector_search(db, version_ids, model, query_vec, candidate_limit).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("vector search failed, falling back to BM25: {e}");
            let mut hits: Vec<Hit> = by_id.into_values().collect();
            hits.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            return Ok(paginate(hits, offset, limit));
        }
    };
    if vec_hits.is_empty() {
        let mut hits: Vec<Hit> = by_id.into_values().collect();
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        return Ok(paginate(hits, offset, limit));
    }

    // 向量命中但不在 BM25 候选里的 chunk，补全元数据
    let missing: Vec<Uuid> = vec_hits
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| !by_id.contains_key(id))
        .collect();
    if !missing.is_empty() {
        let terms = index::query_terms(query);
        let visible: HashSet<Uuid> = version_ids.iter().copied().collect();
        for c in fetch_chunks(db, &missing).await? {
            if !visible.contains(&c.version_id) {
                continue;
            }
            by_id.insert(c.id, hit_from_chunk(&c, query, &terms));
        }
    }

    let mut scores: HashMap<Uuid, f64> = HashMap::new();
    for (rank, id) in bm25_ids.iter().enumerate() {
        *scores.entry(*id).or_insert(0.0) += 1.0 / (RRF_K + rank as f64 + 1.0);
    }
    for (rank, (id, _)) in vec_hits.iter().enumerate() {
        *scores.entry(*id).or_insert(0.0) += 1.0 / (RRF_K + rank as f64 + 1.0);
    }

    let mut fused: Vec<Hit> = scores
        .into_iter()
        .filter_map(|(id, s)| {
            by_id.get(&id).cloned().map(|mut h| {
                // 放大到与 BM25 相近的量级，便于阅读；排序仍按 RRF
                h.score = s * 1000.0;
                h
            })
        })
        .collect();
    fused.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.chunk_id.cmp(&b.chunk_id))
    });
    Ok(paginate(fused, offset, limit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_centers_on_match() {
        let text = "前文 ".repeat(100) + "人民医院数据库" + &" 后文".repeat(100);
        let s = make_snippet(&text, "数据库", &[]);
        assert!(s.contains("人民医院数据库"));
        assert!(s.starts_with('…'));
        assert!(s.ends_with('…'));
    }

    #[test]
    fn snippet_without_match_falls_back_to_head() {
        let s = make_snippet("一二三四五", "不存在", &[]);
        assert!(s.starts_with("一二三四五"));
    }
}
