use std::sync::Arc;

use sea_orm::DatabaseConnection;

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub cfg: Arc<Config>,
}
