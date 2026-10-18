//! MCP 端点的固定窗口限流。
//!
//! key 由「客户端 IP + token 摘要」组成：同一个 token 在多个 IP 上共用额度，
//! 匿名请求则按 IP 计数。限额为 0 时完全关闭。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};

use crate::services::vcs::sha256_hex;
use crate::state::AppState;

const WINDOW: Duration = Duration::from_secs(60);
/// 超过这个条目数就顺手清理过期桶，防止内存无限增长。
const SWEEP_THRESHOLD: usize = 4096;

#[derive(Default)]
struct Bucket {
    started: Option<Instant>,
    count: u32,
}

pub struct RateLimiter {
    limit: u32,
    inner: Mutex<HashMap<String, Bucket>>,
}

impl RateLimiter {
    /// `limit_per_min == 0` 表示关闭限流。
    pub fn new(limit_per_min: u32) -> Self {
        Self {
            limit: limit_per_min,
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn enabled(&self) -> bool {
        self.limit > 0
    }

    /// 返回 true 表示本次请求放行。
    pub fn check(&self, key: &str) -> bool {
        if self.limit == 0 {
            return true;
        }
        let now = Instant::now();
        let Ok(mut map) = self.inner.lock() else {
            return true;
        };
        if map.len() > SWEEP_THRESHOLD {
            map.retain(|_, b| {
                b.started
                    .map(|t| now.duration_since(t) < WINDOW)
                    .unwrap_or(false)
            });
        }
        let bucket = map.entry(key.to_string()).or_default();
        match bucket.started {
            Some(start) if now.duration_since(start) < WINDOW => {
                if bucket.count >= self.limit {
                    return false;
                }
                bucket.count += 1;
                true
            }
            _ => {
                bucket.started = Some(now);
                bucket.count = 1;
                true
            }
        }
    }
}

fn client_ip(req: &Request) -> String {
    req.headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            req.headers()
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.trim().to_string())
        })
        // 没有反向代理头时用 TCP 连接信息兜底
        .or_else(|| {
            req.extensions()
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .map(|ci| ci.0.ip().to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn token_key(req: &Request) -> String {
    let raw = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if raw.is_empty() {
        "anon".to_string()
    } else {
        let digest = sha256_hex(raw);
        format!("tok:{}", &digest[..16])
    }
}

/// axum 中间件：挂在 `/mcp` 外层。
pub async fn middleware(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if state.rate_limiter.enabled() {
        let key = format!("{}|{}", client_ip(&req), token_key(&req));
        if !state.rate_limiter.check(&key) {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({
                    "error": "MCP rate limit exceeded, retry in a minute"
                })),
            )
                .into_response();
        }
    }
    next.run(req).await
}
