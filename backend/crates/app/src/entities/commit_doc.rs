use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "commit_docs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub commit_id: Uuid,
    pub document_id: Uuid,
    /// added | modified | deleted | renamed
    pub change_type: String,
    pub version_id: Option<Uuid>,
    pub old_path: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::commit::Entity",
        from = "Column::CommitId",
        to = "super::commit::Column::Id"
    )]
    Commit,
    #[sea_orm(
        belongs_to = "super::document::Entity",
        from = "Column::DocumentId",
        to = "super::document::Column::Id"
    )]
    Document,
    #[sea_orm(
        belongs_to = "super::doc_version::Entity",
        from = "Column::VersionId",
        to = "super::doc_version::Column::Id"
    )]
    DocVersion,
}

impl Related<super::commit::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Commit.def()
    }
}

impl Related<super::document::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Document.def()
    }
}

impl Related<super::doc_version::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DocVersion.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
