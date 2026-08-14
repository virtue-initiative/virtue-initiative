use std::env;
use std::time::Duration;

pub struct Config {
    pub bind_addr: String,
    pub database_path: String,
    pub jwt_public_key_pem: String,
    pub write_batch_window: Duration,
}

impl Config {
    pub fn from_env() -> Self {
        let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = env::var("PORT").unwrap_or_else(|_| "8788".to_string());

        let write_batch_window_ms: u64 = env::var("WRITE_BATCH_WINDOW_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);

        Config {
            bind_addr: format!("{host}:{port}"),
            database_path: env::var("DATABASE_PATH")
                .unwrap_or_else(|_| "hash-server.sqlite".to_string()),
            jwt_public_key_pem: env::var("JWT_PUBLIC_KEY")
                .expect("JWT_PUBLIC_KEY must be set (Ed25519 SPKI PEM)")
                .replace("\\n", "\n"),
            write_batch_window: Duration::from_millis(write_batch_window_ms),
        }
    }
}
