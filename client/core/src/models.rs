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
pub struct DeviceRegistration {
    pub id: String,
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub enabled: bool,
    #[serde(default)]
    pub e2ee_key: Option<String>,
    #[serde(default)]
    pub hash_base_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BatchUploadResponse {
    pub id: String,
    pub start: i64,
    pub end: i64,
    pub end_hash: String,
    pub url: String,
}

/// Response from POST /hash — server verified and accepted the new state.
#[derive(Clone, Debug, Deserialize)]
pub struct HashUploadResponse {
    pub ok: bool,
}

/// Response from GET /hash — the current rolling state for a device.
#[derive(Clone, Debug)]
pub struct StateResponse {
    pub state: [u8; 32],
}

#[cfg(test)]
mod tests {
    use super::{BatchUploadResponse, Device};

    #[test]
    fn batch_upload_response_accepts_device_batch_shape() {
        let payload = r#"{
            "id": "batch-1",
            "start": 1741122450000,
            "end": 1741122478000,
            "end_hash": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "url": "https://cdn.example.com/u/a.enc"
        }"#;

        let parsed: BatchUploadResponse = serde_json::from_str(payload).expect("valid payload");
        assert_eq!(parsed.id, "batch-1");
        assert_eq!(parsed.url, "https://cdn.example.com/u/a.enc");
    }

    #[test]
    fn device_settings_accepts_optional_key_fields() {
        let payload = r#"{
            "id": "device-1",
            "name": "Laptop",
            "platform": "linux",
            "enabled": true,
            "e2ee_key": "Zm9v",
            "hash_base_url": "https://hash.example.com"
        }"#;

        let parsed: Device = serde_json::from_str(payload).expect("valid payload");
        assert_eq!(parsed.id, "device-1");
        assert_eq!(parsed.e2ee_key.as_deref(), Some("Zm9v"));
        assert_eq!(parsed.hash_base_url.as_deref(), Some("https://hash.example.com"));
    }
}
