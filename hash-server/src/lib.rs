pub mod config;
pub mod db;
pub mod error;
pub mod jwt;
pub mod routes;
pub mod state;

use std::sync::Arc;

use axum::Router;
use axum::routing::post;

use config::Config;
use db::WriteHandle;
use jwt::JwtVerifier;
use state::SharedDevices;

#[derive(Clone)]
pub struct AppState {
    pub devices: SharedDevices,
    pub writer: WriteHandle,
    pub jwt: Arc<JwtVerifier>,
}

pub fn init(config: &Config) -> AppState {
    let devices: SharedDevices = Arc::default();
    let writer = db::spawn_writer(
        &config.database_path,
        config.write_batch_window,
        devices.clone(),
    );
    let jwt = Arc::new(JwtVerifier::new(&config.jwt_public_key_pem));

    AppState {
        devices,
        writer,
        jwt,
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/hash",
            post(routes::hash::ingest)
                .get(routes::hash::get_many)
                .delete(routes::hash::reset),
        )
        .with_state(state)
}
