use sea_orm::entity::prelude::*;

/// 已完成索引的文档版本；用于回填与懒索引判断。
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "search_index_state")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub version_id: Uuid,
    pub chunk_count: i32,
    pub indexed_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
