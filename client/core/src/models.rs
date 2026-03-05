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
    /// Random 32-byte state (hex) set by the server at batch boundary.
    /// Use this to seed the StateHasher for the next batch.
    pub new_state_hex: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct UploadedBatch {
    pub id: String,
    pub batch_url: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// Response from POST /hash — server verified and accepted the new state.
#[derive(Clone, Debug, Deserialize)]
pub struct HashUploadResponse {
    pub ok: bool,
}

/// Response from GET /hash — the current rolling state for a device.
#[derive(Clone, Debug, Deserialize)]
pub struct StateResponse {
    pub state_hex: String,
}

/// Response from GET /e2ee — the user's encrypted E2EE key blob (base64), or null.
#[derive(Clone, Debug, Deserialize)]
pub struct E2EEKeyResponse {
    #[serde(rename = "encryptedE2EEKey")]
    pub encrypted_e2ee_key: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::BatchUploadResponse;

    #[test]
    fn batch_upload_response_accepts_batch_url_shape() {
        let payload = r#"{
            "batch": {
                "id": "batch-1",
                "batch_url": "https://cdn.example.com/u/a.enc",
                "start_time": "2026-03-04T21:07:30.000Z",
                "end_time": "2026-03-04T21:07:58.000Z",
                "created_at": "2026-03-04T21:08:00.000Z"
            },
            "new_state_hex": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        }"#;

        let parsed: BatchUploadResponse = serde_json::from_str(payload).expect("valid payload");
        assert_eq!(parsed.batch.id, "batch-1");
        assert_eq!(parsed.batch.batch_url, "https://cdn.example.com/u/a.enc");
    }
}
