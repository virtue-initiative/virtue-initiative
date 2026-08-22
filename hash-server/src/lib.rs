pub mod api_version;
pub mod config;
pub mod db;
pub mod error;
pub mod jwt;
pub mod logging;
pub mod routes;
pub mod state;

use std::sync::Arc;

use axum::Router;
use axum::middleware;
use axum::routing::{get, post};
use tower_layer::Layer;

use api_version::{ApiVersion, ApiVersionLayer};
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

/// Wraps the whole `Router` (not `Router::layer`, which only wraps individual matched
/// routes and so runs after routing has already happened) so `ApiVersionLayer` can
/// rewrite the path used for matching. Use `.into_make_service()` (from
/// `axum::ServiceExt`) when handing this to `axum::serve`.
pub fn router(state: AppState) -> ApiVersion<Router> {
    let router = Router::new()
        .route("/", get(routes::status::status))
        .route(
            "/hash",
            post(routes::hash::ingest)
                .get(routes::hash::get_many)
                .delete(routes::hash::reset),
        )
        .layer(middleware::from_fn(logging::log_request))
        .with_state(state);

    ApiVersionLayer.layer(router)
}
