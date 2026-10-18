use axum::{
    extract::State,
    routing::{get, post},
    Extension, Json, Router,
};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    entities::user,
    error::{AppError, AppResult},
    services::auth::{self, CurrentUser},
    state::AppState,
};

#[derive(Deserialize)]
pub struct RegisterReq {
    pub username: String,
    pub email: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct LoginReq {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct UserDto {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub display_name: String,
    pub avatar_file: Option<String>,
    pub bio: Option<String>,
    /// 是否管理员（内置 admin 账号），前端据此显示管理入口
    pub is_admin: bool,
    pub created_at: chrono::DateTime<Utc>,
}

impl From<user::Model> for UserDto {
    fn from(u: user::Model) -> Self {
        let is_admin = auth::is_admin(&u);
        Self {
            id: u.id,
            username: u.username,
            email: u.email,
            display_name: u.display_name,
            avatar_file: u.avatar_file,
            bio: u.bio,
            is_admin,
            created_at: u.created_at,
        }
    }
}

fn validate_username(s: &str) -> AppResult<()> {
    let ok_len = (3..=64).contains(&s.len());
    let ok_chars = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !ok_len || !ok_chars {
        return Err(AppError::BadRequest(
            "username must be 3-64 chars of [a-zA-Z0-9_-]".into(),
        ));
    }
    Ok(())
}

fn validate_email(s: &str) -> AppResult<()> {
    if s.len() > 255 || !s.contains('@') || s.starts_with('@') || s.ends_with('@') {
        return Err(AppError::BadRequest("invalid email".into()));
    }
    Ok(())
}

async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterReq>,
) -> AppResult<Json<serde_json::Value>> {
    validate_username(&req.username)?;
    validate_email(&req.email)?;
    if req.password.len() < 8 {
        return Err(AppError::BadRequest(
            "password must be at least 8 chars".into(),
        ));
    }

    let exists = user::Entity::find()
        .filter(
            user::Column::Username
                .eq(&req.username)
                .or(user::Column::Email.eq(&req.email)),
        )
        .one(&state.db)
        .await?;
    if exists.is_some() {
        return Err(AppError::Conflict("username or email already taken".into()));
    }

    let now = Utc::now();
    let model = user::ActiveModel {
        id: Set(Uuid::now_v7()),
        username: Set(req.username),
        email: Set(req.email),
        password_hash: Set(auth::hash_password(&req.password)?),
        display_name: Set(String::new()),
        avatar_file: Set(None),
        bio: Set(None),
        enabled: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
    };
    let created = model.insert(&state.db).await?;
    let token = auth::create_token(&state.cfg.jwt_secret, created.id)?;
    Ok(Json(
        serde_json::json!({ "token": token, "user": UserDto::from(created) }),
    ))
}

async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginReq>,
) -> AppResult<Json<serde_json::Value>> {
    let u = user::Entity::find()
        .filter(
            user::Column::Username
                .eq(&req.username)
                .or(user::Column::Email.eq(&req.username)),
        )
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::Unauthorized("invalid credentials".into()))?;

    if !auth::verify_password(&req.password, &u.password_hash) {
        return Err(AppError::Unauthorized("invalid credentials".into()));
    }
    if !u.enabled {
        return Err(AppError::Forbidden("account disabled".into()));
    }
    let token = auth::create_token(&state.cfg.jwt_secret, u.id)?;
    Ok(Json(
        serde_json::json!({ "token": token, "user": UserDto::from(u) }),
    ))
}

pub fn public_routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/auth/me", get(me))
}

async fn me(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
) -> AppResult<Json<UserDto>> {
    let u = crate::services::auth::load_user(&state.db, cur.id).await?;
    Ok(Json(UserDto::from(u)))
}
