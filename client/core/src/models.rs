use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize)]
pub struct LoginRequest<'a> {
    pub email: &'a str,
    pub password: &'a str,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BatchUploadResponse {
    pub batch: UploadedBatch,
}

#[derive(Clone, Debug, Deserialize)]
pub struct UploadedBatch {
    pub id: String,
    pub r2_key: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct HashUploadResponse {
    pub id: String,
    pub timestamp: String,
}
