use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "mr_reviews")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub mr_id: Uuid,
    pub reviewer_id: Uuid,
    /// approved | rejected | commented
    pub action: String,
    pub comment: Option<String>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::merge_request::Entity",
        from = "Column::MrId",
        to = "super::merge_request::Column::Id"
    )]
    MergeRequest,
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::ReviewerId",
        to = "super::user::Column::Id"
    )]
    Reviewer,
}

impl Related<super::merge_request::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MergeRequest.def()
    }
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Reviewer.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
