//! 管理员接口：站点配置、注册成员管理、公开项目治理。
//!
//! 管理员身份来自内置的 `admin` 账号（见 `services::auth::ADMIN_USERNAME`），
//! 每个 handler 都自己再校验一次权限，不依赖前端隐藏入口。

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    routing::{get, patch, post, put},
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use rand::Rng;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, IntoActiveModel, PaginatorTrait,
    QueryFilter, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{project, user};
use crate::error::{AppError, AppResult};
use crate::services::{
    auth::{self, CurrentUser},
    perm, site,
};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/users", get(list_users))
        .route("/api/admin/users/{id}", patch(update_user))
        .route("/api/admin/users/{id}/reset-password", post(reset_password))
        .route("/api/admin/projects", get(list_projects))
        .route("/api/admin/projects/{slug}", patch(unpublish_project))
        .route("/api/admin/site", put(update_site))
}

#[derive(Deserialize)]
struct PageQuery {
    query: Option<String>,
    page: Option<u64>,
    page_size: Option<u64>,
}

impl PageQuery {
    fn paging(&self) -> (u64, u64) {
        (
            self.page.unwrap_or(1).max(1),
            self.page_size.unwrap_or(20).clamp(1, 100),
        )
    }

    fn keyword(&self) -> Option<String> {
        self.query
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| format!("%{s}%"))
    }
}

// ---------- 注册成员 ----------

#[derive(Serialize)]
struct AdminUserDto {
    id: Uuid,
    username: String,
    email: String,
    display_name: String,
    enabled: bool,
    is_admin: bool,
    /// 作为拥有者的项目数
    project_count: usize,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct UserList {
    items: Vec<AdminUserDto>,
    total: usize,
    page: u64,
    page_size: u64,
}

async fn list_users(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Query(q): Query<PageQuery>,
) -> AppResult<Json<UserList>> {
    auth::require_admin(&state.db, cur.id).await?;
    let (page, page_size) = q.paging();

    let mut cond = Condition::all();
    if let Some(like) = q.keyword() {
        cond = cond.add(
            Condition::any()
                .add(user::Column::Username.like(&like))
                .add(user::Column::Email.like(&like))
                .add(user::Column::DisplayName.like(&like)),
        );
    }

    let total = user::Entity::find()
        .filter(cond.clone())
        .count(&state.db)
        .await? as usize;

    let rows = user::Entity::find()
        .filter(cond)
        .order_by_asc(user::Column::CreatedAt)
        .paginate(&state.db, page_size)
        .fetch_page(page - 1)
        .await?;

    let ids: Vec<Uuid> = rows.iter().map(|u| u.id).collect();
    let owned = if ids.is_empty() {
        vec![]
    } else {
        project::Entity::find()
            .filter(project::Column::OwnerId.is_in(ids))
            .all(&state.db)
            .await?
    };
    let mut counts: HashMap<Uuid, usize> = HashMap::new();
    for p in owned {
        *counts.entry(p.owner_id).or_default() += 1;
    }

    let items = rows
        .into_iter()
        .map(|u| AdminUserDto {
            project_count: counts.get(&u.id).copied().unwrap_or(0),
            is_admin: auth::is_admin(&u),
            id: u.id,
            username: u.username,
            email: u.email,
            display_name: u.display_name,
            enabled: u.enabled,
            created_at: u.created_at,
        })
        .collect();
    Ok(Json(UserList {
        items,
        total,
        page,
        page_size,
    }))
}

#[derive(Deserialize)]
struct UpdateUserReq {
    enabled: bool,
}

/// 禁用 / 启用账号。内置 admin 账号不允许被禁用，避免把自己锁在门外。
async fn update_user(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateUserReq>,
) -> AppResult<Json<serde_json::Value>> {
    auth::require_admin(&state.db, cur.id).await?;
    let target = user::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".into()))?;
    if auth::is_admin(&target) {
        return Err(AppError::BadRequest(
            "the built-in administrator cannot be disabled".into(),
        ));
    }
    let mut am = target.into_active_model();
    am.enabled = Set(req.enabled);
    am.updated_at = Set(Utc::now());
    let updated = am.update(&state.db).await?;
    Ok(Json(serde_json::json!({
        "id": updated.id,
        "enabled": updated.enabled,
    })))
}

/// 随机密码字符集：去掉 0/O、1/l/I 这些容易看错的字符
const PASSWORD_CHARSET: &[u8] = b"abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
/// 重置出来的密码长度
const RESET_PASSWORD_LEN: usize = 8;

fn random_password() -> String {
    let mut rng = rand::rng();
    (0..RESET_PASSWORD_LEN)
        .map(|_| PASSWORD_CHARSET[rng.random_range(0..PASSWORD_CHARSET.len())] as char)
        .collect()
}

/// 给某个注册成员重置密码，返回的明文只在这里出现一次。
/// 内置 admin 账号不能重置（管理员改自己的密码请走个人设置）。
async fn reset_password(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    auth::require_admin(&state.db, cur.id).await?;
    let target = user::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".into()))?;
    if auth::is_admin(&target) {
        return Err(AppError::BadRequest(
            "the built-in administrator's password cannot be reset".into(),
        ));
    }

    let password = random_password();
    let mut am = target.into_active_model();
    am.password_hash = Set(auth::hash_password(&password)?);
    am.updated_at = Set(Utc::now());
    let updated = am.update(&state.db).await?;
    Ok(Json(serde_json::json!({
        "id": updated.id,
        "username": updated.username,
        "password": password,
    })))
}

// ---------- 公开项目 ----------

#[derive(Serialize)]
struct AdminProjectDto {
    id: Uuid,
    slug: String,
    name: String,
    description: Option<String>,
    owner_name: String,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct ProjectList {
    items: Vec<AdminProjectDto>,
    total: usize,
    page: u64,
    page_size: u64,
}

/// 只列公开项目：管理员在这里做的是“公开内容治理”，不是接管别人的私有项目。
async fn list_projects(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Query(q): Query<PageQuery>,
) -> AppResult<Json<ProjectList>> {
    auth::require_admin(&state.db, cur.id).await?;
    let (page, page_size) = q.paging();

    let mut cond = Condition::all().add(project::Column::Visibility.eq("public"));
    if let Some(like) = q.keyword() {
        cond = cond.add(
            Condition::any()
                .add(project::Column::Name.like(&like))
                .add(project::Column::Slug.like(&like))
                .add(project::Column::Description.like(&like)),
        );
    }

    let total = project::Entity::find()
        .filter(cond.clone())
        .count(&state.db)
        .await? as usize;

    let rows = project::Entity::find()
        .filter(cond)
        .order_by_desc(project::Column::UpdatedAt)
        .paginate(&state.db, page_size)
        .fetch_page(page - 1)
        .await?;

    let owner_ids: Vec<Uuid> = rows.iter().map(|p| p.owner_id).collect();
    let owners = if owner_ids.is_empty() {
        vec![]
    } else {
        user::Entity::find()
            .filter(user::Column::Id.is_in(owner_ids))
            .all(&state.db)
            .await?
    };
    let by_owner: HashMap<Uuid, user::Model> = owners.into_iter().map(|u| (u.id, u)).collect();

    let items = rows
        .into_iter()
        .map(|p| {
            let owner_name = by_owner
                .get(&p.owner_id)
                .map(|u| {
                    if u.display_name.is_empty() {
                        u.username.clone()
                    } else {
                        u.display_name.clone()
                    }
                })
                .unwrap_or_default();
            AdminProjectDto {
                id: p.id,
                slug: p.slug,
                name: p.name,
                description: p.description,
                owner_name,
                updated_at: p.updated_at,
            }
        })
        .collect();
    Ok(Json(ProjectList {
        items,
        total,
        page,
        page_size,
    }))
}

#[derive(Deserialize)]
struct UpdateProjectReq {
    visibility: String,
}

/// 取消公开：把项目改回私有，首页与公开文档路由立刻不可见。
/// 管理员没有“替别人公开”的入口，所以这里只接受 private。
async fn unpublish_project(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(slug): Path<String>,
    Json(req): Json<UpdateProjectReq>,
) -> AppResult<Json<serde_json::Value>> {
    auth::require_admin(&state.db, cur.id).await?;
    if req.visibility != "private" {
        return Err(AppError::BadRequest(
            "admin can only set visibility=private".into(),
        ));
    }
    let proj = perm::get_project(&state.db, &slug).await?;
    if proj.visibility != "private" {
        let mut am = proj.into_active_model();
        am.visibility = Set("private".to_string());
        am.updated_at = Set(Utc::now());
        am.update(&state.db).await?;
    }
    Ok(Json(
        serde_json::json!({ "slug": slug, "visibility": "private" }),
    ))
}

// ---------- 站点设置 ----------

#[derive(Deserialize)]
struct SiteReq {
    home_title: String,
    /// 选填的仓库地址 / 反馈入口；传空串表示清掉这一项，字段不传则保持原样
    #[serde(default)]
    repo_url: Option<String>,
    #[serde(default)]
    feedback_url: Option<String>,
    #[serde(default)]
    feedback_email: Option<String>,
}

async fn update_site(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Json(req): Json<SiteReq>,
) -> AppResult<Json<site::SiteInfo>> {
    auth::require_admin(&state.db, cur.id).await?;

    let title = req.home_title.trim();
    let len = title.chars().count();
    if len == 0 || len > 64 {
        return Err(AppError::BadRequest(
            "home_title must be 1-64 characters".into(),
        ));
    }
    site::set(&state.db, site::HOME_TITLE, title).await?;

    set_optional_url(&state, site::REPO_URL, req.repo_url.as_deref(), "repo_url").await?;
    set_optional_url(
        &state,
        site::FEEDBACK_URL,
        req.feedback_url.as_deref(),
        "feedback_url",
    )
    .await?;
    set_email(&state, site::FEEDBACK_EMAIL, req.feedback_email.as_deref()).await?;

    Ok(Json(site::site_info(&state.db).await?))
}

/// 选填链接：留空清掉，否则必须是 http(s)
async fn set_optional_url(
    state: &AppState,
    key: &str,
    value: Option<&str>,
    field: &str,
) -> AppResult<()> {
    let Some(raw) = value.map(str::trim) else {
        return Ok(());
    };
    if raw.is_empty() {
        return site::remove(&state.db, key).await;
    }
    if !(raw.starts_with("http://") || raw.starts_with("https://")) || raw.chars().count() > 300 {
        return Err(AppError::BadRequest(format!(
            "{field} must be an http(s) URL within 300 chars"
        )));
    }
    site::set(&state.db, key, raw).await
}

/// 选填邮箱：留空清掉，否则只做最基本的形状检查
async fn set_email(state: &AppState, key: &str, value: Option<&str>) -> AppResult<()> {
    let Some(raw) = value.map(str::trim) else {
        return Ok(());
    };
    if raw.is_empty() {
        return site::remove(&state.db, key).await;
    }
    if !raw.contains('@') || raw.chars().count() > 255 {
        return Err(AppError::BadRequest("feedback_email looks invalid".into()));
    }
    site::set(&state.db, key, raw).await
}
