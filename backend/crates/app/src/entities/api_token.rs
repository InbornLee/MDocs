use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "api_tokens")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    /// sha256(token) 的十六进制，明文只返回一次
    pub token_hash: String,
    /// 供页面辨认的 token 前缀
    pub token_prefix: String,
    pub created_at: DateTimeUtc,
    pub last_used_at: Option<DateTimeUtc>,
    /// 过期时间；`None` 表示永不过期
    pub expires_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
