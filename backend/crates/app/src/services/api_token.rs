//! 个人 API token：给 MCP 等外部客户端使用的长期凭证。
//!
//! 明文只在创建时返回一次，库里只存 sha256 十六进制与前缀。

use chrono::Utc;
use rand::RngCore;
use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, Set};
use uuid::Uuid;

use crate::entities::api_token;
use crate::error::{AppError, AppResult};
use crate::services::vcs;

/// token 前缀，便于在界面上辨认
const TOKEN_PREFIX: &str = "mdocs_";
/// 随机部分字节数（16 字节 → 32 位十六进制）
const SECRET_BYTES: usize = 16;
/// 页面上展示的随机部分长度
const DISPLAY_CHARS: usize = 6;

fn hash(token: &str) -> String {
    vcs::sha256_hex(token)
}

/// 供页面 / 审计辨认的 token 前缀（与 `create` 写入的 `token_prefix` 一致）。
pub fn display_prefix(token: &str) -> String {
    let body = token.trim().strip_prefix(TOKEN_PREFIX).unwrap_or(token.trim());
    format!(
        "{TOKEN_PREFIX}{}",
        body.chars().take(DISPLAY_CHARS).collect::<String>()
    )
}

/// 新建一条 token，返回（明文、记录）。明文此后不再可得。
///
/// `expires_in_days` 为 `None` 或 `<= 0` 表示永不过期。
pub async fn create(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    name: &str,
    expires_in_days: Option<i64>,
) -> AppResult<(String, api_token::Model)> {
    let mut buf = [0u8; SECRET_BYTES];
    rand::rng().fill_bytes(&mut buf);
    let secret: String = buf.iter().map(|b| format!("{b:02x}")).collect();
    let token = format!("{TOKEN_PREFIX}{secret}");

    let now = Utc::now();
    let expires_at = expires_in_days
        .filter(|d| *d > 0)
        .map(|d| now + chrono::Duration::days(d));

    let model = api_token::ActiveModel {
        id: Set(Uuid::now_v7()),
        user_id: Set(user_id),
        name: Set(name.to_string()),
        token_hash: Set(hash(&token)),
        token_prefix: Set(format!("{TOKEN_PREFIX}{}", &secret[..DISPLAY_CHARS])),
        created_at: Set(now),
        last_used_at: Set(None),
        expires_at: Set(expires_at),
    }
    .insert(db)
    .await?;
    Ok((token, model))
}

/// 校验 token 并返回所属用户；顺带刷新 last_used_at（失败不影响鉴权）。
pub async fn resolve(db: &sea_orm::DatabaseConnection, token: &str) -> AppResult<Option<Uuid>> {
    let token = token.trim();
    if token.is_empty() {
        return Ok(None);
    }
    let Some(row) = api_token::Entity::find()
        .filter(api_token::Column::TokenHash.eq(hash(token)))
        .one(db)
        .await?
    else {
        return Ok(None);
    };

    let user_id = row.user_id;
    // 过期 token 与不存在同样处理，但给出明确原因，方便客户端排查。
    if let Some(exp) = row.expires_at {
        if exp <= Utc::now() {
            return Err(AppError::Unauthorized(
                "API token expired, create a new one in Settings → API token".into(),
            ));
        }
    }
    let mut am: api_token::ActiveModel = row.into();
    am.last_used_at = Set(Some(Utc::now()));
    if let Err(e) = am.update(db).await {
        tracing::warn!("failed to touch api token last_used_at: {e}");
    }
    Ok(Some(user_id))
}

/// 某个用户的全部 token，最近创建的在前。
pub async fn list(db: &impl ConnectionTrait, user_id: Uuid) -> AppResult<Vec<api_token::Model>> {
    Ok(api_token::Entity::find()
        .filter(api_token::Column::UserId.eq(user_id))
        .order_by_desc(api_token::Column::CreatedAt)
        .all(db)
        .await?)
}

/// 删除（作废）一条 token，只能删自己的。
pub async fn revoke(db: &impl ConnectionTrait, user_id: Uuid, id: Uuid) -> AppResult<()> {
    let res = api_token::Entity::delete_many()
        .filter(api_token::Column::Id.eq(id))
        .filter(api_token::Column::UserId.eq(user_id))
        .exec(db)
        .await?;
    if res.rows_affected == 0 {
        return Err(AppError::NotFound("token not found".into()));
    }
    Ok(())
}
