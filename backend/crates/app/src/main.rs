mod config;
mod entities;
mod error;
mod mcp;
mod routes;
mod services;
mod state;
mod static_files;

use std::sync::Arc;

use axum::{
    extract::State,
    routing::get,
    Json, Router,
};
use migration::MigratorTrait;
use sea_orm::{ConnectionTrait, Database};
use tower_http::{
    compression::CompressionLayer,
    cors::CorsLayer,
    trace::{DefaultMakeSpan, TraceLayer},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::services::{rate_limit::RateLimiter, snapshot::SnapshotCache};
use crate::state::AppState;

async fn health() -> &'static str {
    "ok"
}

async fn version(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "name": "mdocs",
        "version": env!("CARGO_PKG_VERSION"),
        "mcp": {
            "transport": "streamable-http",
            "path": "/mcp",
            "tools": [
                "list_projects",
                "list_branches",
                "list_documents",
                "get_document_outline",
                "read_document",
                "search_documents",
                "get_link_graph"
            ],
            "resources": true,
            "prompts": ["answer_with_citations"],
            "rate_limit_per_min": state.cfg.mcp_rate_limit_per_min,
        }
    }))
}

fn build_router(state: AppState) -> Router {
    let public = Router::new()
        .route("/api/health", get(health))
        .route("/api/version", get(version))
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
        .merge(
            Router::new()
                .nest_service("/mcp", mcp::service(state.clone()))
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    services::rate_limit::middleware,
                )),
        )
        .fallback(static_files::static_handler)
        .layer(CorsLayer::permissive())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::default().include_headers(true)),
        )
        // SSE 由 tower-http 默认排除，不会破坏 MCP 的流式响应
        .layer(CompressionLayer::new())
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
        snapshots: Arc::new(SnapshotCache::new(cfg.snapshot_cache_size)),
        rate_limiter: Arc::new(RateLimiter::new(cfg.mcp_rate_limit_per_min)),
        db,
        cfg: Arc::new(cfg),
    };
    let listen = state.cfg.listen.clone();
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&listen).await?;
    tracing::info!("mdocs listening on http://{}", listen);
    // ConnectInfo 让 MCP 审计 / 限流能拿到真实客户端 IP（反代未传 X-Forwarded-For 时兜底）
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}
