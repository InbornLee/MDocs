//! Project membership: list / add / update / remove collaborators.
//!
//! Only the project owner (or a member with the `owner` role) may mutate the
//! member list, mirroring the review requirement that a *different* editor
//! signs off on merges into the default branch. The single exception is a
//! member removing themselves (退出项目) through `DELETE .../members/me`, which
//! only requires being a member — the caller id comes from the session rather
//! than the URL, so nobody can leave on someone else's behalf.

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Extension, Json, Router,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, IntoActiveModel, QueryFilter, Set,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{project_member, user};
use crate::error::{AppError, AppResult};
use crate::services::{auth::CurrentUser, perm, perm::Role};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/projects/{slug}/members",
            get(list_members).post(add_member),
        )
        .route(
            "/api/projects/{slug}/member-candidates",
            get(list_candidates),
        )
        .route(
            "/api/projects/{slug}/members/me",
            axum::routing::delete(leave_project),
        )
        .route(
            "/api/projects/{slug}/members/{user_id}",
            axum::routing::patch(update_member).delete(remove_member),
        )
}

#[derive(Serialize)]
struct MemberDto {
    user_id: Uuid,
    username: String,
    display_name: String,
    avatar_file: Option<String>,
    role: String,
    is_owner: bool,
    created_at: chrono::DateTime<Utc>,
}

async fn list_members(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(slug): Path<String>,
) -> AppResult<Json<Vec<MemberDto>>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Reader).await?;

    let rows = project_member::Entity::find()
        .filter(project_member::Column::ProjectId.eq(proj.id))
        .all(&state.db)
        .await?;

    let ids: Vec<Uuid> = rows.iter().map(|r| r.user_id).collect();
    let users = if ids.is_empty() {
        vec![]
    } else {
        user::Entity::find()
            .filter(user::Column::Id.is_in(ids))
            .all(&state.db)
            .await?
    };
    let mut by_id: std::collections::HashMap<Uuid, user::Model> =
        users.into_iter().map(|u| (u.id, u)).collect();

    let mut out: Vec<MemberDto> = Vec::with_capacity(rows.len());
    for r in rows {
        let Some(u) = by_id.remove(&r.user_id) else {
            continue;
        };
        out.push(MemberDto {
            user_id: u.id,
            username: u.username,
            display_name: u.display_name,
            avatar_file: u.avatar_file,
            is_owner: r.role == "owner" || u.id == proj.owner_id,
            role: r.role,
            created_at: r.created_at,
        });
    }
    out.sort_by(|a, b| {
        b.is_owner
            .cmp(&a.is_owner)
            .then_with(|| a.username.cmp(&b.username))
    });
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct CandidateQuery {
    /// 搜索词：用户名 / 昵称 / 邮箱的任意片段
    #[serde(default)]
    q: String,
    #[serde(default)]
    limit: Option<u64>,
}

#[derive(Serialize)]
struct CandidateDto {
    username: String,
    display_name: String,
    avatar_file: Option<String>,
}

/// 「添加成员」下拉框的候选用户。只暴露身份最小字段，不返回邮箱等资料；
/// 权限与添加成员一致，仅 owner 可查。
async fn list_candidates(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(slug): Path<String>,
    Query(query): Query<CandidateQuery>,
) -> AppResult<Json<Vec<CandidateDto>>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Owner).await?;

    let term = query.q.trim();
    if term.is_empty() {
        return Ok(Json(Vec::new()));
    }
    let like = format!("%{term}%");
    let mut rows = user::Entity::find()
        .filter(
            Condition::any()
                .add(user::Column::Username.like(&like))
                .add(user::Column::DisplayName.like(&like))
                .add(user::Column::Email.like(&like)),
        )
        .all(&state.db)
        .await?;

    // 用户名完全相等 > 前缀命中 > 其它；同级按用户名排序，保证下拉框顺序稳定
    let needle = term.to_lowercase();
    let rank = |name: &str| {
        let name = name.to_lowercase();
        if name == needle {
            0
        } else if name.starts_with(&needle) {
            1
        } else {
            2
        }
    };
    rows.sort_by(|a, b| {
        rank(&a.username)
            .cmp(&rank(&b.username))
            .then_with(|| a.username.cmp(&b.username))
    });
    rows.truncate(query.limit.unwrap_or(10).clamp(1, 20) as usize);

    Ok(Json(
        rows.into_iter()
            .map(|u| CandidateDto {
                username: u.username,
                display_name: u.display_name,
                avatar_file: u.avatar_file,
            })
            .collect(),
    ))
}

fn parse_role(s: &str) -> AppResult<&'static str> {
    match s {
        "owner" => Ok("owner"),
        "editor" => Ok("editor"),
        "reader" => Ok("reader"),
        _ => Err(AppError::BadRequest(
            "role must be owner|editor|reader".into(),
        )),
    }
}

#[derive(Deserialize)]
struct AddMemberReq {
    /// accept either username or email
    user: String,
    #[serde(default)]
    role: Option<String>,
}

async fn add_member(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(slug): Path<String>,
    Json(req): Json<AddMemberReq>,
) -> AppResult<Json<MemberDto>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Owner).await?;

    let role = parse_role(req.role.as_deref().unwrap_or("editor"))?;
    let key = req.user.trim();
    if key.is_empty() {
        return Err(AppError::BadRequest(
            "user (username or email) required".into(),
        ));
    }

    let target = user::Entity::find()
        .filter(
            sea_orm::Condition::any()
                .add(user::Column::Username.eq(key))
                .add(user::Column::Email.eq(key)),
        )
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".into()))?;

    if target.id == proj.owner_id {
        return Err(AppError::Conflict("owner is already a member".into()));
    }
    let exists = project_member::Entity::find()
        .filter(project_member::Column::ProjectId.eq(proj.id))
        .filter(project_member::Column::UserId.eq(target.id))
        .one(&state.db)
        .await?;
    if exists.is_some() {
        return Err(AppError::Conflict("user is already a member".into()));
    }

    let m = project_member::ActiveModel {
        id: Set(Uuid::now_v7()),
        project_id: Set(proj.id),
        user_id: Set(target.id),
        role: Set(role.to_string()),
        created_at: Set(Utc::now()),
    }
    .insert(&state.db)
    .await?;

    Ok(Json(MemberDto {
        user_id: target.id,
        username: target.username,
        display_name: target.display_name,
        avatar_file: target.avatar_file,
        is_owner: false,
        role: m.role,
        created_at: m.created_at,
    }))
}

#[derive(Deserialize)]
struct UpdateMemberReq {
    role: String,
}

async fn update_member(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path((slug, user_id)): Path<(String, Uuid)>,
    Json(req): Json<UpdateMemberReq>,
) -> AppResult<Json<serde_json::Value>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Owner).await?;
    if user_id == proj.owner_id {
        return Err(AppError::BadRequest(
            "cannot change the owner's role".into(),
        ));
    }
    let role = parse_role(&req.role)?;
    let m = project_member::Entity::find()
        .filter(project_member::Column::ProjectId.eq(proj.id))
        .filter(project_member::Column::UserId.eq(user_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("member not found".into()))?;
    let mut am = m.into_active_model();
    am.role = Set(role.to_string());
    let updated = am.update(&state.db).await?;
    Ok(Json(
        serde_json::json!({ "user_id": user_id, "role": updated.role }),
    ))
}

async fn remove_member(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path((slug, user_id)): Path<(String, Uuid)>,
) -> AppResult<Json<serde_json::Value>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Owner).await?;
    if user_id == proj.owner_id {
        return Err(AppError::BadRequest("cannot remove the owner".into()));
    }
    let res = project_member::Entity::delete_many()
        .filter(project_member::Column::ProjectId.eq(proj.id))
        .filter(project_member::Column::UserId.eq(user_id))
        .exec(&state.db)
        .await?;
    if res.rows_affected == 0 {
        return Err(AppError::NotFound("member not found".into()));
    }
    Ok(Json(serde_json::json!({ "removed": user_id })))
}

/// 退出项目：把当前登录用户自己从成员列表里移除，不需要 owner 权限。
/// 项目的真正拥有者不能退出，只能删项目。
async fn leave_project(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(slug): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    if cur.id == proj.owner_id {
        return Err(AppError::BadRequest(
            "project owner cannot leave, delete the project instead".into(),
        ));
    }
    let res = project_member::Entity::delete_many()
        .filter(project_member::Column::ProjectId.eq(proj.id))
        .filter(project_member::Column::UserId.eq(cur.id))
        .exec(&state.db)
        .await?;
    if res.rows_affected == 0 {
        return Err(AppError::NotFound("member not found".into()));
    }
    Ok(Json(serde_json::json!({ "removed": cur.id })))
}
