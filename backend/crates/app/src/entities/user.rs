use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub display_name: String,
    pub avatar_file: Option<String>,
    pub bio: Option<String>,
    /// 被管理员禁用后无法登录，已签发的 token 也会失效
    pub enabled: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::project::Entity")]
    Project,
    #[sea_orm(has_many = "super::project_member::Entity")]
    ProjectMember,
    #[sea_orm(has_many = "super::commit::Entity")]
    Commit,
}

impl Related<super::project::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Project.def()
    }
}

impl Related<super::project_member::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ProjectMember.def()
    }
}

impl Related<super::commit::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Commit.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
