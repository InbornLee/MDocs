use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "documents")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub project_id: Uuid,
    /// relative path with forward slashes, e.g. "docs/arch/db.md"
    pub path: String,
    pub title: String,
    /// listed on the public site when the project is public
    pub published: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::project::Entity",
        from = "Column::ProjectId",
        to = "super::project::Column::Id"
    )]
    Project,
    #[sea_orm(has_many = "super::doc_version::Entity")]
    DocVersion,
}

impl Related<super::project::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Project.def()
    }
}

impl Related<super::doc_version::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DocVersion.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
