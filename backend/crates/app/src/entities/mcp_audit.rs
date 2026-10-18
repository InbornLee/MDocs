use sea_orm::entity::prelude::*;

/// MCP 工具调用审计。只记录短摘要，不落文档正文。
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "mcp_audit_log")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// 使用 API token 时为该用户；匿名调用为空
    pub user_id: Option<Uuid>,
    pub token_prefix: Option<String>,
    pub client_ip: Option<String>,
    /// 工具名，如 search_documents
    pub tool: String,
    /// 涉及的项目 slug（跨项目检索时为首个项目）
    pub project: Option<String>,
    /// 关键参数的短摘要
    pub detail: Option<String>,
    pub ok: bool,
    pub error: Option<String>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
