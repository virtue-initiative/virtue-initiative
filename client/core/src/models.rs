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

#[derive(Clone, Debug, Serialize)]
pub struct CreateImageRequest {
    pub device_id: String,
    pub sha256: String,
    pub taken_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CreateImageResponse {
    pub image: UploadedImage,
}

#[derive(Clone, Debug, Deserialize)]
pub struct UploadedImage {
    pub id: String,
    pub status: String,
    pub r2_key: String,
    pub taken_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}
