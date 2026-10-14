mod config;
mod entities;
mod error;
mod mcp;
mod routes;
mod services;
mod state;
mod static_files;

use axum::{
    routing::get,
    Router,
};
use migration::MigratorTrait;
use sea_orm::{ConnectionTrait, Database};
use tower_http::{
    cors::CorsLayer,
    trace::{DefaultMakeSpan, TraceLayer},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::state::AppState;

async fn health() -> &'static str {
    "ok"
}

fn build_router(state: AppState) -> Router {
    let public = Router::new()
        .route("/api/health", get(health))
        .merge(routes::auth::public_routes())
        .merge(routes::user::public_routes())
        .merge(routes::transfer::public_routes())
        .merge(routes::public::routes());

    let authenticated = routes::auth::routes()
        .merge(routes::user::routes())
        .merge(routes::project::routes())
        .merge(routes::document::routes())
        .merge(routes::transfer::routes())
        .merge(routes::mr::routes())
        .merge(routes::member::routes())
        .merge(routes::admin::routes())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            services::auth::auth_middleware,
        ));

    Router::new()
        .merge(public)
        .merge(authenticated)
        // MCP (Model Context Protocol) endpoint for AI knowledge-base access
        .nest_service("/mcp", mcp::service(state.clone()))
        .fallback(static_files::static_handler)
        .layer(CorsLayer::permissive())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::default().include_headers(true)),
        )
        .with_state(state)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn,tower_http=warn".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cfg = config::Config::from_env();
    tokio::fs::create_dir_all(&cfg.data_dir).await?;
    tokio::fs::create_dir_all(cfg.avatars_dir()).await?;
    tokio::fs::create_dir_all(cfg.attachments_dir()).await?;

    tracing::info!("connecting to database: {}", cfg.database_url);
    let db = Database::connect(&cfg.database_url).await?;

    // SQLite: enable foreign keys pragma (no-op on postgres)
    if cfg.database_url.starts_with("sqlite") {
        db.execute_unprepared("PRAGMA foreign_keys = ON;").await?;
    }

    tracing::info!("running migrations");
    migration::Migrator::up(&db, None).await?;

    // 系统初始化：保证有一个可用的管理员账号
    services::auth::seed_admin(&db).await?;

    let state = AppState {
        db,
        cfg: std::sync::Arc::new(cfg),
    };
    let listen = state.cfg.listen.clone();
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&listen).await?;
    tracing::info!("mdocs listening on http://{}", listen);
    axum::serve(listener, app).await?;
    Ok(())
}
