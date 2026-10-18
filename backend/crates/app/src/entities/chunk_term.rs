use sea_orm::entity::prelude::*;

/// 倒排表：term 在某个 chunk 中的词频。
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "chunk_terms")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub chunk_id: Uuid,
    pub term: String,
    pub tf: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::doc_chunk::Entity",
        from = "Column::ChunkId",
        to = "super::doc_chunk::Column::Id"
    )]
    DocChunk,
}

impl Related<super::doc_chunk::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DocChunk.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
