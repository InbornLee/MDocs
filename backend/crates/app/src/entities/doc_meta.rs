use sea_orm::entity::prelude::*;

/// 一个文档版本的 front matter 元数据。
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "doc_meta")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub version_id: Uuid,
    pub document_id: Uuid,
    pub description: Option<String>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
