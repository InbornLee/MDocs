use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use regex::Regex;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    entities::{api_token, user},
    error::{AppError, AppResult},
    routes::auth::UserDto,
    services::{api_token as tokens, auth, auth::CurrentUser, avatar},
    state::AppState,
};

#[derive(Deserialize)]
pub struct ProfileReq {
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub email: Option<String>,
}

async fn update_profile(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Json(req): Json<ProfileReq>,
) -> AppResult<Json<UserDto>> {
    let mut m: user::ActiveModel = user::Entity::find_by_id(cur.id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".into()))?
        .into();

    if let Some(dn) = &req.display_name {
        if dn.len() > 128 {
            return Err(AppError::BadRequest("display_name too long".into()));
        }
        m.display_name = Set(dn.clone());
    }
    if let Some(bio) = &req.bio {
        if bio.len() > 2000 {
            return Err(AppError::BadRequest("bio too long".into()));
        }
        m.bio = Set(if bio.is_empty() { None } else { Some(bio.clone()) });
    }
    if let Some(email) = &req.email {
        if !email.contains('@') || email.len() > 255 {
            return Err(AppError::BadRequest("invalid email".into()));
        }
        let taken = user::Entity::find()
            .filter(user::Column::Email.eq(email))
            .filter(user::Column::Id.ne(cur.id))
            .one(&state.db)
            .await?;
        if taken.is_some() {
            return Err(AppError::Conflict("email already taken".into()));
        }
        m.email = Set(email.clone());
    }
    m.updated_at = Set(Utc::now());
    let updated = m.update(&state.db).await?;
    Ok(Json(UserDto::from(updated)))
}

async fn upload_avatar(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    mut multipart: Multipart,
) -> AppResult<Json<serde_json::Value>> {
    let mut field_found = false;
    let mut name = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("multipart: {e}")))?
    {
        if field.name() != Some("avatar") {
            continue;
        }
        field_found = true;
        let bytes = field
            .bytes()
            .await
            .map_err(|e| AppError::BadRequest(format!("read upload: {e}")))?;
        name = Some(avatar::save_avatar(&state, bytes).await?);
        break;
    }
    let name = if field_found {
        name.ok_or_else(|| AppError::BadRequest("empty avatar".into()))?
    } else {
        return Err(AppError::BadRequest("missing 'avatar' field".into()));
    };

    let m: user::ActiveModel = user::Entity::find_by_id(cur.id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".into()))?
        .into();
    let old = m.avatar_file.clone().unwrap();
    let mut m = m;
    m.avatar_file = Set(Some(name.clone()));
    m.updated_at = Set(Utc::now());
    m.update(&state.db).await?;

    // best-effort remove of previous avatar
    if let Some(old) = old {
        if !old.is_empty() {
            let _ = tokio::fs::remove_file(state.cfg.avatars_dir().join(&old)).await;
        }
    }
    Ok(Json(serde_json::json!({ "avatar_file": name })))
}

/// Public static avatar file. Filename is restricted to `<uuid>.webp`.
async fn get_avatar(Path(file): Path<String>) -> AppResult<Response> {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^[0-9a-f-]{36}\.webp$").unwrap());
    if !re.is_match(&file) {
        return Err(AppError::BadRequest("invalid file name".into()));
    }
    let cfg_dir = std::env::var("APP_DATA_DIR").unwrap_or_else(|_| "./data".into());
    let path = std::path::Path::new(&cfg_dir).join("uploads").join("avatars").join(&file);
    let data = tokio::fs::read(&path)
        .await
        .map_err(|_| AppError::NotFound("avatar not found".into()))?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, HeaderValue::from_static("image/webp")),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            ),
        ],
        data,
    )
        .into_response())
}

#[derive(Deserialize)]
pub struct ChangePasswordReq {
    pub old_password: String,
    pub new_password: String,
}

/// 修改自己的密码。旧密码错返回 400 而不是 401 —— 前端拿到 401 会清掉登录态。
async fn change_password(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Json(req): Json<ChangePasswordReq>,
) -> AppResult<Json<serde_json::Value>> {
    let u = auth::load_user(&state.db, cur.id).await?;
    if !auth::verify_password(&req.old_password, &u.password_hash) {
        return Err(AppError::BadRequest("current password is incorrect".into()));
    }
    if req.new_password.len() < 8 {
        return Err(AppError::BadRequest("password must be at least 8 chars".into()));
    }
    if req.new_password == req.old_password {
        return Err(AppError::BadRequest(
            "new password must differ from the current one".into(),
        ));
    }

    let mut am: user::ActiveModel = u.into();
    am.password_hash = Set(auth::hash_password(&req.new_password)?);
    am.updated_at = Set(Utc::now());
    am.update(&state.db).await?;
    Ok(Json(serde_json::json!({ "updated": true })))
}

// ---------- API token（MCP 调用凭证）----------

#[derive(Serialize)]
struct TokenDto {
    id: Uuid,
    name: String,
    /// 用于辨认的前缀；完整 token 只在创建时返回一次
    prefix: String,
    created_at: DateTime<Utc>,
    last_used_at: Option<DateTime<Utc>>,
}

impl From<api_token::Model> for TokenDto {
    fn from(m: api_token::Model) -> Self {
        Self {
            id: m.id,
            name: m.name,
            prefix: m.token_prefix,
            created_at: m.created_at,
            last_used_at: m.last_used_at,
        }
    }
}

async fn list_tokens(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
) -> AppResult<Json<Vec<TokenDto>>> {
    let rows = tokens::list(&state.db, cur.id).await?;
    Ok(Json(rows.into_iter().map(TokenDto::from).collect()))
}

#[derive(Deserialize)]
pub struct CreateTokenReq {
    pub name: Option<String>,
}

async fn create_token(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Json(req): Json<CreateTokenReq>,
) -> AppResult<Json<serde_json::Value>> {
    let name = req.name.unwrap_or_default().trim().to_string();
    let name = if name.is_empty() {
        "未命名 token".to_string()
    } else {
        name
    };
    if name.chars().count() > 64 {
        return Err(AppError::BadRequest("name too long".into()));
    }

    let (token, model) = tokens::create(&state.db, cur.id, &name).await?;
    Ok(Json(serde_json::json!({
        "id": model.id,
        "name": model.name,
        "token": token,
        "created_at": model.created_at,
    })))
}

async fn revoke_token(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    tokens::revoke(&state.db, cur.id, id).await?;
    Ok(Json(serde_json::json!({ "revoked": id })))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/me/profile", patch(update_profile))
        .route(
            "/api/me/avatar",
            post(upload_avatar).layer(DefaultBodyLimit::max(6 * 1024 * 1024)),
        )
        .route("/api/me/password", put(change_password))
        .route("/api/me/tokens", get(list_tokens).post(create_token))
        .route("/api/me/tokens/{id}", delete(revoke_token))
}

pub fn public_routes() -> Router<AppState> {
    Router::new().route("/uploads/avatars/{file}", get(get_avatar))
}
