mod auth;
mod db;
mod routes;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{HeaderValue, Method, header::{AUTHORIZATION, CONTENT_TYPE}},
    routing::{get, post},
};
use jsonwebtoken::DecodingKey;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use std::{str::FromStr, sync::Arc};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{EnvFilter, fmt};

pub struct AppState {
    pub pool: SqlitePool,
    pub decoding_key: DecodingKey,
}

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/hash", post(routes::post_hash).get(routes::get_hash).delete(routes::delete_hash))
        .route("/health", get(routes::health))
        .layer(DefaultBodyLimit::max(64))
        .with_state(state)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("hash_server=info".parse()?))
        .init();

    let jwt_public_key = std::env::var("JWT_PUBLIC_KEY")
        .expect("JWT_PUBLIC_KEY must be set");
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite:hash-states.db".to_owned());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3000);
    let allowed_origins = std::env::var("ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "http://localhost:5173".to_owned());

    let key_pem = jwt_public_key
        .replace("\r\n", "\n")
        .replace("\\n", "\n");
    let decoding_key = DecodingKey::from_ed_pem(key_pem.trim().as_bytes())
        .expect("invalid JWT_PUBLIC_KEY");

    let opts = SqliteConnectOptions::from_str(&database_url)?
        .journal_mode(SqliteJournalMode::Wal)
        .create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .connect_with(opts)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    let origins: Vec<HeaderValue> = allowed_origins
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST, Method::DELETE])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE]);

    let state = Arc::new(AppState { pool, decoding_key });

    let app = build_router(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("listening on port {port}");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;

    Ok(())
}
