use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "doc_versions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub document_id: Uuid,
    pub content: String,
    pub content_hash: String,
    pub size_bytes: i32,
    pub created_by: Uuid,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::document::Entity",
        from = "Column::DocumentId",
        to = "super::document::Column::Id"
    )]
    Document,
    #[sea_orm(has_many = "super::commit_doc::Entity")]
    CommitDoc,
}

impl Related<super::document::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Document.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
