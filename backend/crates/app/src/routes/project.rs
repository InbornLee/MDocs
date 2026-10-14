//! Project CRUD and branch management.

use axum::{
    extract::{Path, State},
    routing::get,
    Extension, Json, Router,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{branch, commit, project, project_member};
use crate::error::{AppError, AppResult};
use crate::services::{auth::CurrentUser, perm, perm::Role, vcs};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects", get(list_projects).post(create_project))
        .route(
            "/api/projects/{slug}",
            get(get_project_detail).patch(update_project).delete(delete_project),
        )
        .route(
            "/api/projects/{slug}/branches",
            get(list_branches).post(create_branch),
        )
}

#[derive(Serialize)]
struct BranchDto {
    id: Uuid,
    name: String,
    head_commit_id: Option<Uuid>,
    is_default: bool,
    created_at: chrono::DateTime<Utc>,
}

impl From<branch::Model> for BranchDto {
    fn from(b: branch::Model) -> Self {
        Self {
            id: b.id,
            name: b.name,
            head_commit_id: b.head_commit_id,
            is_default: b.is_default,
            created_at: b.created_at,
        }
    }
}

#[derive(Serialize)]
struct ProjectDto {
    id: Uuid,
    slug: String,
    name: String,
    description: Option<String>,
    visibility: String,
    role: String,
    /// the caller holds the `owner` role (a co-owner member counts as one)
    is_owner: bool,
    /// the caller is the project's creator — only collaborators can leave,
    /// so the UI needs this apart from `is_owner`
    is_creator: bool,
    default_branch: Option<String>,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
}

fn to_dto(
    p: &project::Model,
    role: Role,
    default_branch: Option<String>,
    viewer: Uuid,
) -> ProjectDto {
    ProjectDto {
        id: p.id,
        slug: p.slug.clone(),
        name: p.name.clone(),
        description: p.description.clone(),
        visibility: p.visibility.clone(),
        role: role.as_str().into(),
        is_owner: role == Role::Owner,
        is_creator: p.owner_id == viewer,
        default_branch,
        created_at: p.created_at,
        updated_at: p.updated_at,
    }
}

fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = true; // trim leading dashes
    for ch in name.to_lowercase().chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[derive(Deserialize)]
struct CreateProjectReq {
    name: String,
    slug: Option<String>,
    description: Option<String>,
    #[serde(default)]
    visibility: Option<String>,
}

async fn create_project(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Json(req): Json<CreateProjectReq>,
) -> AppResult<Json<ProjectDto>> {
    let name = req.name.trim().to_string();
    if name.is_empty() || name.len() > 128 {
        return Err(AppError::BadRequest("name must be 1-128 chars".into()));
    }
    let slug = req
        .slug
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| slugify(&name));
    if slug.is_empty() || slug.len() > 64 || !slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err(AppError::BadRequest("slug must be 1-64 chars of [a-z0-9-]".into()));
    }
    let visibility = match req.visibility.as_deref() {
        Some("public") => "public",
        Some("private") | None => "private",
        _ => return Err(AppError::BadRequest("visibility must be public|private".into())),
    };

    let exists = project::Entity::find()
        .filter(project::Column::Slug.eq(&slug))
        .one(&state.db)
        .await?;
    if exists.is_some() {
        return Err(AppError::Conflict("slug already taken".into()));
    }

    let txn = state.db.begin().await?;
    let now = Utc::now();
    let project_id = Uuid::now_v7();
    let branch_id = Uuid::now_v7();

    let proj = project::ActiveModel {
        id: Set(project_id),
        owner_id: Set(cur.id),
        slug: Set(slug.clone()),
        name: Set(name.clone()),
        description: Set(req.description.clone().filter(|d| !d.trim().is_empty())),
        visibility: Set(visibility.into()),
        default_branch_id: Set(Some(branch_id)),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&txn)
    .await?;

    branch::ActiveModel {
        id: Set(branch_id),
        project_id: Set(project_id),
        name: Set("main".into()),
        head_commit_id: Set(None),
        created_by: Set(cur.id),
        is_default: Set(true),
        created_at: Set(now),
    }
    .insert(&txn)
    .await?;

    project_member::ActiveModel {
        id: Set(Uuid::now_v7()),
        project_id: Set(project_id),
        user_id: Set(cur.id),
        role: Set("owner".into()),
        created_at: Set(now),
    }
    .insert(&txn)
    .await?;

    txn.commit().await?;

    Ok(Json(to_dto(&proj, Role::Owner, Some("main".into()), cur.id)))
}

async fn list_projects(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
) -> AppResult<Json<Vec<ProjectDto>>> {
    let owned = project::Entity::find()
        .filter(project::Column::OwnerId.eq(cur.id))
        .all(&state.db)
        .await?;
    let mut map: Vec<(project::Model, Role)> =
        owned.into_iter().map(|p| (p, Role::Owner)).collect();

    let memberships = project_member::Entity::find()
        .filter(project_member::Column::UserId.eq(cur.id))
        .all(&state.db)
        .await?;
    let member_project_ids: Vec<Uuid> = memberships.iter().map(|m| m.project_id).collect();
    if !member_project_ids.is_empty() {
        let projects = project::Entity::find()
            .filter(project::Column::Id.is_in(member_project_ids))
            .all(&state.db)
            .await?;
        let roles: std::collections::HashMap<Uuid, String> = memberships
            .into_iter()
            .map(|m| (m.project_id, m.role))
            .collect();
        for p in projects {
            if p.owner_id == cur.id {
                continue; // already in map
            }
            let role = match roles.get(&p.id).map(|s| s.as_str()) {
                Some("editor") => Role::Editor,
                Some("owner") => Role::Owner,
                _ => Role::Reader,
            };
            map.push((p, role));
        }
    }

    let mut out = Vec::with_capacity(map.len());
    for (p, role) in map {
        let default_branch = match p.default_branch_id {
            Some(bid) => branch::Entity::find_by_id(bid)
                .one(&state.db)
                .await?
                .map(|b| b.name),
            None => None,
        };
        out.push(to_dto(&p, role, default_branch, cur.id));
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(Json(out))
}

async fn get_project_detail(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(slug): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    let role = perm::require_role(&state.db, &proj, cur.id, Role::Public).await?;

    let branches = branch::Entity::find()
        .filter(branch::Column::ProjectId.eq(proj.id))
        .all(&state.db)
        .await?;
    let branch_dtos: Vec<BranchDto> = branches.into_iter().map(BranchDto::from).collect();
    let default_branch = branch_dtos
        .iter()
        .find(|b| b.is_default)
        .map(|b| b.name.clone());

    let commit_count = commit::Entity::find()
        .filter(commit::Column::ProjectId.eq(proj.id))
        .count(&state.db)
        .await?;

    Ok(Json(serde_json::json!({
        "project": to_dto(&proj, role, default_branch, cur.id),
        "branches": branch_dtos,
        "commit_count": commit_count,
    })))
}

#[derive(Deserialize)]
struct UpdateProjectReq {
    name: Option<String>,
    description: Option<String>,
    visibility: Option<String>,
}

async fn update_project(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(slug): Path<String>,
    Json(req): Json<UpdateProjectReq>,
) -> AppResult<Json<ProjectDto>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Owner).await?;

    let mut am: project::ActiveModel = proj.clone().into();
    if let Some(name) = &req.name {
        let name = name.trim().to_string();
        if name.is_empty() || name.len() > 128 {
            return Err(AppError::BadRequest("name must be 1-128 chars".into()));
        }
        am.name = Set(name);
    }
    if let Some(desc) = &req.description {
        let d = desc.trim().to_string();
        am.description = Set(if d.is_empty() { None } else { Some(d) });
    }
    if let Some(v) = &req.visibility {
        match v.as_str() {
            "public" | "private" => am.visibility = Set(v.clone()),
            _ => return Err(AppError::BadRequest("visibility must be public|private".into())),
        }
    }
    am.updated_at = Set(Utc::now());
    let updated = am.update(&state.db).await?;

    let default_branch = match updated.default_branch_id {
        Some(bid) => branch::Entity::find_by_id(bid)
            .one(&state.db)
            .await?
            .map(|b| b.name),
        None => None,
    };
    Ok(Json(to_dto(&updated, Role::Owner, default_branch, cur.id)))
}

async fn delete_project(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(slug): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Owner).await?;
    project::Entity::delete_by_id(proj.id)
        .exec(&state.db)
        .await?;
    Ok(Json(serde_json::json!({ "deleted": slug })))
}

async fn list_branches(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(slug): Path<String>,
) -> AppResult<Json<Vec<BranchDto>>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Public).await?;
    let branches = branch::Entity::find()
        .filter(branch::Column::ProjectId.eq(proj.id))
        .all(&state.db)
        .await?;
    Ok(Json(branches.into_iter().map(BranchDto::from).collect()))
}

#[derive(Deserialize)]
struct CreateBranchReq {
    name: String,
    from_branch: Option<String>,
}

async fn create_branch(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(slug): Path<String>,
    Json(req): Json<CreateBranchReq>,
) -> AppResult<Json<BranchDto>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    // Any member (reader+) can fork a branch; phase-6 MR flow extends this to
    // public-project collaborators.
    perm::require_role(&state.db, &proj, cur.id, Role::Reader).await?;

    let name = req.name.trim().to_string();
    if name.is_empty()
        || name.len() > 64
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '/')
    {
        return Err(AppError::BadRequest(
            "branch name must be 1-64 chars of [A-Za-z0-9._/-]".into(),
        ));
    }
    let exists = branch::Entity::find()
        .filter(branch::Column::ProjectId.eq(proj.id))
        .filter(branch::Column::Name.eq(&name))
        .one(&state.db)
        .await?;
    if exists.is_some() {
        return Err(AppError::Conflict("branch already exists".into()));
    }

    let from = vcs::resolve_branch(&state.db, &proj, req.from_branch.as_deref()).await?;
    let now = Utc::now();
    let b = branch::ActiveModel {
        id: Set(Uuid::now_v7()),
        project_id: Set(proj.id),
        name: Set(name),
        head_commit_id: Set(from.head_commit_id),
        created_by: Set(cur.id),
        is_default: Set(false),
        created_at: Set(now),
    }
    .insert(&state.db)
    .await?;
    Ok(Json(BranchDto::from(b)))
}
