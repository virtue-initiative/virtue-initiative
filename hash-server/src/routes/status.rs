use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct StatusInfo {
    name: &'static str,
    version: &'static str,
    commit: &'static str,
    status: &'static str,
}

/// `GET /` — see HASH-009.
pub async fn status() -> Json<StatusInfo> {
    Json(StatusInfo {
        name: "Virtue Initiative Hash API",
        version: env!("CARGO_PKG_VERSION"),
        commit: env!("GIT_COMMIT_HASH"),
        status: "ok",
    })
}
