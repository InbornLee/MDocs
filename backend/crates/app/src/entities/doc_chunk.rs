use sea_orm::entity::prelude::*;

/// 一个文档版本按 heading 切出的检索块。
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "doc_chunks")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub version_id: Uuid,
    pub document_id: Uuid,
    pub ord: i32,
    pub heading: Option<String>,
    pub anchor: Option<String>,
    pub level: Option<i32>,
    pub start_line: i32,
    pub end_line: i32,
    pub text: String,
    pub token_count: i32,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::chunk_term::Entity")]
    ChunkTerm,
}

impl Related<super::chunk_term::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ChunkTerm.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
