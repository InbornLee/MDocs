use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "link_edges")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub project_id: Uuid,
    pub branch_id: Uuid,
    pub source_doc_id: Uuid,
    pub target_doc_id: Option<Uuid>,
    pub raw_target: String,
    /// md_relative | wiki
    pub link_type: String,
    pub anchor: Option<String>,
    pub display_text: Option<String>,
    pub line_no: Option<i32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::project::Entity",
        from = "Column::ProjectId",
        to = "super::project::Column::Id"
    )]
    Project,
    #[sea_orm(
        belongs_to = "super::branch::Entity",
        from = "Column::BranchId",
        to = "super::branch::Column::Id"
    )]
    Branch,
    #[sea_orm(
        belongs_to = "super::document::Entity",
        from = "Column::SourceDocId",
        to = "super::document::Column::Id"
    )]
    SourceDoc,
}

impl Related<super::project::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Project.def()
    }
}

impl Related<super::branch::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Branch.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
