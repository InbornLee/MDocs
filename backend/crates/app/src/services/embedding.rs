//! 可选的 embedding 客户端与向量回填 worker（P1 阶段 2）。
//!
//! 使用 OpenAI 兼容的 `/v1/embeddings` 协议，可指向 Ollama / vLLM / 云端服务。
//! 没配置 `APP_EMBEDDING__URL` 时整个模块不参与检索，系统保持纯 BM25。

use std::time::Duration;

use chrono::Utc;
use reqwest::Client;
use sea_orm::{
    sea_query::{OnConflict, Query},
    ColumnTrait, DbErr, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::entities::{chunk_vector, doc_chunk};
use crate::state::AppState;

/// 单条文本送给模型前截断的字符数（中文按字符计）。
const MAX_EMBED_CHARS: usize = 2000;
/// 每轮补齐的 chunk 数；worker 会连续处理直到补齐。
const FILL_BATCH: u64 = 128;

#[derive(Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[derive(Deserialize)]
struct EmbeddingItem {
    embedding: Vec<f32>,
    #[serde(default)]
    index: Option<usize>,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingItem>,
}

#[derive(Clone)]
pub struct EmbeddingClient {
    http: Client,
    url: String,
    model: String,
    api_key: Option<String>,
    batch_size: usize,
}

impl EmbeddingClient {
    /// 未配置 URL 时返回 `None`，调用方据此降级。
    pub fn from_config(cfg: &Config) -> Option<Self> {
        let url = cfg.embedding.url.clone()?;
        let http = Client::builder()
            .timeout(Duration::from_secs(cfg.embedding.timeout_secs))
            .build()
            .ok()?;
        Some(Self {
            http,
            url,
            model: cfg.embedding.model.clone(),
            api_key: cfg.embedding.api_key.clone(),
            batch_size: cfg.embedding.batch_size.max(1),
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let mut req = self.http.post(&self.url).json(&EmbeddingRequest {
            model: &self.model,
            input: texts,
        });
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("embedding request failed: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body: String = resp.text().await.unwrap_or_default();
            return Err(format!(
                "embedding HTTP {status}: {}",
                body.chars().take(200).collect::<String>()
            ));
        }
        let data: EmbeddingResponse = resp
            .json()
            .await
            .map_err(|e| format!("embedding decode failed: {e}"))?;
        let mut out = vec![Vec::new(); texts.len()];
        for (i, item) in data.data.into_iter().enumerate() {
            let idx = item.index.unwrap_or(i);
            if idx < out.len() {
                out[idx] = item.embedding;
            }
        }
        if out.iter().any(|v| v.is_empty()) {
            return Err("embedding response missing vectors".into());
        }
        Ok(out)
    }

    pub async fn embed_query(&self, text: &str) -> Result<Vec<f32>, String> {
        let text: String = text.chars().take(MAX_EMBED_CHARS).collect();
        let mut v = self.embed(&[text]).await?;
        v.pop().ok_or_else(|| "empty embedding".to_string())
    }
}

pub fn encode(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

pub fn decode(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..n {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na <= 0.0 || nb <= 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

async fn fill_missing(state: &AppState, client: &EmbeddingClient) -> Result<usize, DbErr> {
    let model = client.model().to_string();
    let sub = Query::select()
        .column(chunk_vector::Column::ChunkId)
        .from(chunk_vector::Entity)
        .and_where(chunk_vector::Column::Model.eq(&model))
        .to_owned();
    let chunks = doc_chunk::Entity::find()
        .filter(doc_chunk::Column::Id.not_in_subquery(sub))
        .order_by_asc(doc_chunk::Column::Id)
        .limit(FILL_BATCH)
        .all(&state.db)
        .await?;
    if chunks.is_empty() {
        return Ok(0);
    }

    let mut done = 0usize;
    for batch in chunks.chunks(client.batch_size) {
        let texts: Vec<String> = batch
            .iter()
            .map(|c| c.text.chars().take(MAX_EMBED_CHARS).collect())
            .collect();
        let vectors = match client.embed(&texts).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("embedding batch failed: {e}");
                break;
            }
        };
        for (chunk, vector) in batch.iter().zip(vectors) {
            let dim = vector.len() as i32;
            let am = chunk_vector::ActiveModel {
                chunk_id: Set(chunk.id),
                model: Set(model.clone()),
                dim: Set(dim),
                vector: Set(encode(&vector)),
                created_at: Set(Utc::now()),
            };
            chunk_vector::Entity::insert(am)
                .on_conflict(
                    OnConflict::column(chunk_vector::Column::ChunkId)
                        .update_columns([
                            chunk_vector::Column::Model,
                            chunk_vector::Column::Dim,
                            chunk_vector::Column::Vector,
                            chunk_vector::Column::CreatedAt,
                        ])
                        .to_owned(),
                )
                .exec(&state.db)
                .await?;
            done += 1;
        }
    }
    Ok(done)
}

/// 后台补齐缺失向量：有活就连续处理，闲着每 30s 检查一次。
pub async fn run_worker(state: AppState) {
    let Some(client) = EmbeddingClient::from_config(&state.cfg) else {
        tracing::info!("embedding disabled (APP_EMBEDDING__URL not set); BM25 only");
        return;
    };
    tracing::info!(
        "embedding worker started: model={} url={}",
        client.model(),
        client.url
    );
    loop {
        match fill_missing(&state, &client).await {
            Ok(n) if n > 0 => {
                tracing::info!("embedded {n} chunks");
                continue;
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("embedding worker: {e}"),
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_roundtrip() {
        let v = vec![0.5f32, -1.25, 3.0];
        assert_eq!(decode(&encode(&v)), v);
    }

    #[test]
    fn cosine_basics() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
    }
}
