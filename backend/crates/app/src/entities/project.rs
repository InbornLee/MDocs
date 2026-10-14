use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "projects")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub owner_id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    /// private | public
    pub visibility: String,
    pub default_branch_id: Option<Uuid>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::OwnerId",
        to = "super::user::Column::Id"
    )]
    Owner,
    #[sea_orm(has_many = "super::project_member::Entity")]
    ProjectMember,
    #[sea_orm(has_many = "super::branch::Entity")]
    Branch,
    #[sea_orm(has_many = "super::commit::Entity")]
    Commit,
    #[sea_orm(has_many = "super::document::Entity")]
    Document,
    #[sea_orm(has_many = "super::merge_request::Entity")]
    MergeRequest,
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Owner.def()
    }
}

impl Related<super::project_member::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ProjectMember.def()
    }
}

impl Related<super::branch::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Branch.def()
    }
}

impl Related<super::document::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Document.def()
    }
}

impl Related<super::merge_request::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MergeRequest.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
