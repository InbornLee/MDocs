use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "merge_requests")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub project_id: Uuid,
    pub source_branch_id: Uuid,
    pub target_branch_id: Uuid,
    pub base_commit_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    /// open | merged | closed
    pub status: String,
    /// JSON array of conflicted paths
    pub conflict_files: Option<String>,
    pub merge_commit_id: Option<Uuid>,
    pub created_by: Uuid,
    pub reviewed_by: Option<Uuid>,
    pub merged_at: Option<DateTimeUtc>,
    pub created_at: DateTimeUtc,
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
        from = "Column::SourceBranchId",
        to = "super::branch::Column::Id"
    )]
    SourceBranch,
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::CreatedBy",
        to = "super::user::Column::Id"
    )]
    Creator,
    #[sea_orm(has_many = "super::mr_review::Entity")]
    MrReview,
}

impl Related<super::project::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Project.def()
    }
}

impl Related<super::mr_review::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MrReview.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
