use sea_orm::entity::prelude::*;

/// 文档版本上的标签（来自 front matter）。
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "doc_tags")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub version_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false)]
    pub tag: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
