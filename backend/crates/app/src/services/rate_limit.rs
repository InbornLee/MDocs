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

/// 一次限流检查的结果，直接映射到 `RateLimit-*` / `Retry-After` 响应头。
pub struct LimitInfo {
    pub allowed: bool,
    pub limit: u32,
    pub remaining: u32,
    /// 当前窗口剩余秒数
    pub reset_after: u64,
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

    /// 检查并消耗一次额度；返回值同时用于响应头。
    pub fn check(&self, key: &str) -> LimitInfo {
        if self.limit == 0 {
            return LimitInfo {
                allowed: true,
                limit: 0,
                remaining: 0,
                reset_after: 0,
            };
        }
        let now = Instant::now();
        let Ok(mut map) = self.inner.lock() else {
            // 锁中毒时放行，避免限流器故障拖垮整个端点
            return LimitInfo {
                allowed: true,
                limit: self.limit,
                remaining: self.limit,
                reset_after: WINDOW.as_secs(),
            };
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
                let elapsed = now.duration_since(start);
                let reset_after = WINDOW.saturating_sub(elapsed).as_secs().max(1);
                if bucket.count >= self.limit {
                    LimitInfo {
                        allowed: false,
                        limit: self.limit,
                        remaining: 0,
                        reset_after,
                    }
                } else {
                    bucket.count += 1;
                    LimitInfo {
                        allowed: true,
                        limit: self.limit,
                        remaining: self.limit - bucket.count,
                        reset_after,
                    }
                }
            }
            _ => {
                bucket.started = Some(now);
                bucket.count = 1;
                LimitInfo {
                    allowed: true,
                    limit: self.limit,
                    remaining: self.limit.saturating_sub(1),
                    reset_after: WINDOW.as_secs(),
                }
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

fn header_value(n: u64) -> axum::http::HeaderValue {
    axum::http::HeaderValue::from_str(&n.to_string())
        .unwrap_or_else(|_| axum::http::HeaderValue::from_static("0"))
}

/// axum 中间件：挂在 `/mcp` 外层。
pub async fn middleware(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if !state.rate_limiter.enabled() {
        return next.run(req).await;
    }

    let key = format!("{}|{}", client_ip(&req), token_key(&req));
    let info = state.rate_limiter.check(&key);

    let mut resp = if info.allowed {
        next.run(req).await
    } else {
        (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "error": "MCP rate limit exceeded, retry later",
                "retry_after_seconds": info.reset_after,
            })),
        )
            .into_response()
    };

    let headers = resp.headers_mut();
    headers.insert("ratelimit-limit", header_value(info.limit as u64));
    headers.insert("ratelimit-remaining", header_value(info.remaining as u64));
    headers.insert("ratelimit-reset", header_value(info.reset_after));
    if !info.allowed {
        headers.insert(header::RETRY_AFTER, header_value(info.reset_after));
    }
    resp
}
