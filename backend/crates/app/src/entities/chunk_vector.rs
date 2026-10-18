use sea_orm::entity::prelude::*;

/// 一个 chunk 的 embedding（f32 小端 BLOB）。
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "chunk_vectors")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub chunk_id: Uuid,
    pub model: String,
    pub dim: i32,
    pub vector: Vec<u8>,
    pub created_at: DateTimeUtc,
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
