use argon2::{
    password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier},
    Argon2,
};
use axum::{
    extract::{Request, State},
    http::header::AUTHORIZATION,
    middleware::Next,
    response::Response,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{entities::user, error::AppError, state::AppState};

/// 内置管理员账号名。系统首次启动时自动创建（`seed_admin`），
/// 也是当前唯一的“是不是管理员”判定依据。
pub const ADMIN_USERNAME: &str = "admin";
/// 初始化管理员时的默认密码，登录后应当尽快在个人设置里改掉
pub const DEFAULT_ADMIN_PASSWORD: &str = "admin";
/// 没有邮箱可用时给管理员账号填的占位邮箱
const ADMIN_EMAIL: &str = "admin@localhost";

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: i64,
    pub iat: i64,
}

pub fn hash_password(password: &str) -> Result<String, AppError> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(format!("hash password: {e}")))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

pub fn create_token(secret: &str, user_id: Uuid) -> Result<String, AppError> {
    let now = Utc::now();
    let claims = Claims {
        sub: user_id.to_string(),
        iat: now.timestamp(),
        exp: (now + Duration::days(7)).timestamp(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(format!("create token: {e}")))
}

pub fn verify_token(secret: &str, token: &str) -> Result<Claims, AppError> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map(|d| d.claims)
    .map_err(|_| AppError::Unauthorized("invalid token".into()))
}

/// Identity attached to request extensions after auth middleware.
#[derive(Clone, Debug)]
pub struct CurrentUser {
    pub id: Uuid,
}

pub async fn load_user(db: &DatabaseConnection, id: Uuid) -> Result<user::Model, AppError> {
    user::Entity::find_by_id(id)
        .one(db)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::Unauthorized("user no longer exists".into()))
}

/// 是不是管理员账号
pub fn is_admin(u: &user::Model) -> bool {
    u.username == ADMIN_USERNAME
}

/// 管理员专用接口的权限校验：不是管理员直接 403
pub async fn require_admin(db: &DatabaseConnection, id: Uuid) -> Result<user::Model, AppError> {
    let u = load_user(db, id).await?;
    if !is_admin(&u) {
        return Err(AppError::Forbidden("administrator only".into()));
    }
    Ok(u)
}

/// 系统初始化：没有 `admin` 账号时创建它（密码 `admin`）。
/// 已存在则什么都不做，避免覆盖用户改过的密码。
pub async fn seed_admin(db: &DatabaseConnection) -> Result<(), AppError> {
    let exists = user::Entity::find()
        .filter(user::Column::Username.eq(ADMIN_USERNAME))
        .one(db)
        .await?;
    if exists.is_some() {
        return Ok(());
    }
    let now = Utc::now();
    user::ActiveModel {
        id: Set(Uuid::now_v7()),
        username: Set(ADMIN_USERNAME.to_string()),
        email: Set(ADMIN_EMAIL.to_string()),
        password_hash: Set(hash_password(DEFAULT_ADMIN_PASSWORD)?),
        display_name: Set("管理员".to_string()),
        avatar_file: Set(None),
        bio: Set(None),
        enabled: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;
    tracing::warn!(
        "created the built-in administrator account: {ADMIN_USERNAME} / {DEFAULT_ADMIN_PASSWORD} (please change the password after signing in)"
    );
    Ok(())
}

/// Bearer-token auth middleware. Sets CurrentUser extension.
pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| AppError::Unauthorized("missing bearer token".into()))?;

    let claims = verify_token(&state.cfg.jwt_secret, token)?;
    let uid: Uuid = claims
        .sub
        .parse()
        .map_err(|_| AppError::Unauthorized("invalid token subject".into()))?;
    let u = load_user(&state.db, uid).await?;
    // 被禁用的账号手上的 token 立刻作废
    if !u.enabled {
        return Err(AppError::Unauthorized("account disabled".into()));
    }

    req.extensions_mut().insert(CurrentUser { id: u.id });
    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_roundtrip() {
        let h = hash_password("password123").expect("hash");
        assert!(verify_password("password123", &h));
        assert!(!verify_password("wrong", &h));
    }

    #[test]
    fn token_roundtrip() {
        let uid = Uuid::now_v7();
        let t = create_token("secret", uid).expect("token");
        let claims = verify_token("secret", &t).expect("verify");
        assert_eq!(claims.sub, uid.to_string());
    }
}
