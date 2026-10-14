//! 站点级配置读写。表结构是 key-value，缺省值由读取方兜底。

use chrono::Utc;
use sea_orm::{ActiveModelTrait, ConnectionTrait, EntityTrait, Set};
use serde::Serialize;

use crate::entities::setting;
use crate::error::AppResult;

/// 首页大标题的配置键
pub const HOME_TITLE: &str = "home_title";
/// 未配置过时首页显示的默认标题
pub const DEFAULT_HOME_TITLE: &str = "公开项目";
/// 关于页的仓库地址 / 反馈入口，都可留空
pub const REPO_URL: &str = "repo_url";
pub const FEEDBACK_URL: &str = "feedback_url";
pub const FEEDBACK_EMAIL: &str = "feedback_email";

/// 站点公开配置，`None` 表示这一项没配置（前端据此决定显不显示）
#[derive(Debug, Serialize)]
pub struct SiteInfo {
    pub home_title: String,
    pub repo_url: Option<String>,
    pub feedback_url: Option<String>,
    pub feedback_email: Option<String>,
}

pub async fn get(db: &impl ConnectionTrait, key: &str) -> AppResult<Option<String>> {
    Ok(setting::Entity::find_by_id(key.to_string())
        .one(db)
        .await?
        .map(|m| m.value)
        .filter(|v| !v.trim().is_empty()))
}

pub async fn set(db: &impl ConnectionTrait, key: &str, value: &str) -> AppResult<()> {
    let now = Utc::now();
    match setting::Entity::find_by_id(key.to_string()).one(db).await? {
        Some(m) => {
            let mut am: setting::ActiveModel = m.into();
            am.value = Set(value.to_string());
            am.updated_at = Set(now);
            am.update(db).await?;
        }
        None => {
            setting::ActiveModel {
                key: Set(key.to_string()),
                value: Set(value.to_string()),
                updated_at: Set(now),
            }
            .insert(db)
            .await?;
        }
    }
    Ok(())
}

/// 清掉某一项配置（管理界面里把内容清空时走这里）
pub async fn remove(db: &impl ConnectionTrait, key: &str) -> AppResult<()> {
    setting::Entity::delete_by_id(key.to_string())
        .exec(db)
        .await?;
    Ok(())
}

/// 首页标题，未配置时返回默认值
pub async fn home_title(db: &impl ConnectionTrait) -> AppResult<String> {
    Ok(get(db, HOME_TITLE)
        .await?
        .unwrap_or_else(|| DEFAULT_HOME_TITLE.to_string()))
}

/// 一次取齐公开配置（配置项很少，逐条查足够）
pub async fn site_info(db: &impl ConnectionTrait) -> AppResult<SiteInfo> {
    Ok(SiteInfo {
        home_title: home_title(db).await?,
        repo_url: get(db, REPO_URL).await?,
        feedback_url: get(db, FEEDBACK_URL).await?,
        feedback_email: get(db, FEEDBACK_EMAIL).await?,
    })
}
