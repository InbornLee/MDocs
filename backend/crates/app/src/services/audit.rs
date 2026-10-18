//! MCP 调用审计。
//!
//! 写入是 best-effort：审计失败只记日志，绝不影响工具本身的返回。

use chrono::Utc;
use sea_orm::{ActiveModelTrait, Set};
use uuid::Uuid;

use crate::entities::mcp_audit;
use crate::state::AppState;

/// 一条审计记录。`detail` 只放短摘要（工具名 + 关键参数），不落文档全文。
pub struct AuditEntry {
    pub user_id: Option<Uuid>,
    pub token_prefix: Option<String>,
    pub client_ip: Option<String>,
    pub tool: String,
    pub project: Option<String>,
    pub detail: Option<String>,
    pub ok: bool,
    pub error: Option<String>,
}

pub fn record(state: &AppState, entry: AuditEntry) {
    let db = state.db.clone();
    tokio::spawn(async move {
        let truncate = |s: Option<String>, n: usize| {
            s.map(|s| {
                let mut out: String = s.chars().take(n).collect();
                if out.chars().count() >= n {
                    out.push('…');
                }
                out
            })
        };
        let model = mcp_audit::ActiveModel {
            id: Set(Uuid::now_v7()),
            user_id: Set(entry.user_id),
            token_prefix: Set(entry.token_prefix),
            client_ip: Set(truncate(entry.client_ip, 64)),
            tool: Set(entry.tool),
            project: Set(truncate(entry.project, 128)),
            detail: Set(truncate(entry.detail, 300)),
            ok: Set(entry.ok),
            error: Set(truncate(entry.error, 300)),
            created_at: Set(Utc::now()),
        };
        if let Err(e) = model.insert(&db).await {
            tracing::warn!("mcp audit insert failed: {e}");
        }
    });
}
