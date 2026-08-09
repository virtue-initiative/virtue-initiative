mod auth;
mod db;
mod routes;
mod writer;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{HeaderValue, Method, header::{AUTHORIZATION, CONTENT_TYPE}},
    routing::{get, post},
};
use dashmap::DashMap;
use jsonwebtoken::DecodingKey;
use sqlx::{
    Connection,
    sqlite::{SqliteConnectOptions, SqliteConnection, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous},
};
use std::{str::FromStr, sync::Arc, time::Duration};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{EnvFilter, fmt};

pub struct AppState {
    pub read_pool: SqlitePool,
    pub write: writer::WriteHandle,
    pub decoding_key: DecodingKey,
    /// Per-device last-accepted signature timestamp (ms), used by
    /// `auth::verify_signature` to reject replayed/non-increasing
    /// timestamps on signed `POST /hash` requests. In-memory only — reset
    /// on restart is fine, since a device's next signed request just
    /// re-establishes its watermark. At 1M devices (UUID-length String key
    /// plus i64 value plus DashMap's per-entry/shard overhead), this is
    /// roughly a hundred to a hundred fifty megabytes.
    pub replay_guard: DashMap<String, i64>,
}

/// Write queue capacity: once this many writes are queued waiting for the
/// single writer connection, new writes are rejected immediately (503)
/// instead of piling up behind an ever-growing backlog.
const WRITE_QUEUE_CAPACITY: usize = 4096;

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
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(30))
        .create_if_missing(true);

    // All writes go through this single dedicated connection (see
    // writer.rs) — SQLite only allows one writer at a time regardless of
    // pool size, so a pool of writer connections just adds lock-contention
    // overhead. Migrations run on it first since it's the connection that
    // keeps the (possibly in-memory, in tests) database alive.
    let mut write_conn = SqliteConnection::connect_with(&opts).await?;
    sqlx::migrate!("./migrations").run(&mut write_conn).await?;
    let write = writer::spawn(write_conn, WRITE_QUEUE_CAPACITY);

    // Reads never need the write lock (WAL mode lets readers proceed
    // concurrently with the writer), so they get their own pool and never
    // queue behind writes. A short acquire timeout means reads fail fast
    // under extreme overload instead of hanging.
    let read_pool = SqlitePoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(opts)
        .await?;

    let origins: Vec<HeaderValue> = allowed_origins
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST, Method::DELETE])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE]);

    let state = Arc::new(AppState { read_pool, write, decoding_key, replay_guard: DashMap::new() });

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
