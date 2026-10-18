use std::sync::Arc;

use sea_orm::DatabaseConnection;

use crate::config::Config;
use crate::services::{rate_limit::RateLimiter, snapshot::SnapshotCache};

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub cfg: Arc<Config>,
    /// head -> 分支文档快照（commit 不可变，命中后无需失效）
    pub snapshots: Arc<SnapshotCache>,
    /// /mcp 限流器
    pub rate_limiter: Arc<RateLimiter>,
}
