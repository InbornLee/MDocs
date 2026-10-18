//! Project-level permission checks.

use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::entities::{project, project_member};
use crate::error::{AppError, AppResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    None = 0,
    Public = 1,
    Reader = 2,
    Editor = 3,
    Owner = 4,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::None => "none",
            Role::Public => "public",
            Role::Reader => "reader",
            Role::Editor => "editor",
            Role::Owner => "owner",
        }
    }
}

pub async fn project_role(
    db: &impl ConnectionTrait,
    proj: &project::Model,
    user_id: Uuid,
) -> AppResult<Role> {
    if proj.owner_id == user_id {
        return Ok(Role::Owner);
    }
    let m = project_member::Entity::find()
        .filter(project_member::Column::ProjectId.eq(proj.id))
        .filter(project_member::Column::UserId.eq(user_id))
        .one(db)
        .await
        .map_err(AppError::from)?;
    if let Some(m) = m {
        return Ok(match m.role.as_str() {
            "editor" => Role::Editor,
            "owner" => Role::Owner,
            _ => Role::Reader,
        });
    }
    if proj.visibility == "public" {
        Ok(Role::Public)
    } else {
        Ok(Role::None)
    }
}

pub async fn get_project(db: &impl ConnectionTrait, slug: &str) -> AppResult<project::Model> {
    project::Entity::find()
        .filter(project::Column::Slug.eq(slug))
        .one(db)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::NotFound("project not found".into()))
}

pub async fn require_role(
    db: &impl ConnectionTrait,
    proj: &project::Model,
    user_id: Uuid,
    need: Role,
) -> AppResult<Role> {
    let role = project_role(db, proj, user_id).await?;
    if role < need {
        return Err(AppError::Forbidden(
            "insufficient project permission".into(),
        ));
    }
    Ok(role)
}
