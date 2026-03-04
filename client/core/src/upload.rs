use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use reqwest::multipart::{Form, Part};
use sha2::{Digest, Sha256};

use crate::batch::BatchBlob;
use crate::error::{CoreError, CoreResult};
use crate::models::{BatchUploadResponse, HashUploadResponse, StateResponse};
use crate::resolve_base_api_url;

#[derive(Clone, Debug)]
pub struct UploadClientConfig {
    pub base_url: String,
    /// Base URL used for hash upload/retrieval (`/hash` endpoints).
    /// Falls back to `base_url` if not set.
    pub hash_base_url: Option<String>,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
}

impl Default for UploadClientConfig {
    fn default() -> Self {
        Self {
            base_url: resolve_base_api_url(),
            hash_base_url: None,
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(60),
        }
    }
}

impl UploadClientConfig {
    fn effective_hash_base_url(&self) -> &str {
        self.hash_base_url.as_deref().unwrap_or(&self.base_url)
    }
}

#[derive(Clone)]
pub struct UploadClient {
    client: reqwest::Client,
    config: UploadClientConfig,
}

impl UploadClient {
    pub fn new() -> CoreResult<Self> {
        Self::with_config(UploadClientConfig::default())
    }

    pub fn with_config(config: UploadClientConfig) -> CoreResult<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(config.connect_timeout)
            .timeout(config.request_timeout)
            .build()?;
        Ok(Self { client, config })
    }

    /// Encrypt, compress and upload a batch blob to the API.
    /// Returns the server response including `new_state_hex` to seed the next batch.
    pub async fn upload_batch(
        &self,
        access_token: &str,
        device_id: &str,
        blob: &BatchBlob,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        key: &[u8; 32],
    ) -> CoreResult<BatchUploadResponse> {
        let encrypted = blob.encode_encrypted(key)?;
        let item_count = blob.items.len();
        let size_bytes = encrypted.len();

        let file_part = Part::bytes(encrypted)
            .file_name("batch.enc")
            .mime_str("application/octet-stream")?;

        let form = Form::new()
            .text("device_id", device_id.to_string())
            .text(
                "start_time",
                start_time.to_rfc3339_opts(SecondsFormat::Millis, true),
            )
            .text(
                "end_time",
                end_time.to_rfc3339_opts(SecondsFormat::Millis, true),
            )
            .text("item_count", item_count.to_string())
            .text("size_bytes", size_bytes.to_string())
            .part("file", file_part);

        let url = format!("{}/batch", self.config.base_url);
        let response = self
            .client
            .post(url)
            .bearer_auth(access_token)
            .multipart(form)
            .send()
            .await?;

        decode_response(response).await
    }

    /// Upload a log's content hash to POST /hash.
    ///
    /// Body: 48 bytes — `device_id_bytes[16] || content_hash[32]`
    ///
    /// The server computes and stores `new_state = sha256(current_state || content_hash)`.
    pub async fn upload_hash(
        &self,
        access_token: &str,
        device_id_bytes: &[u8; 16],
        content_hash: &[u8; 32],
    ) -> CoreResult<HashUploadResponse> {
        let mut body = Vec::with_capacity(48);
        body.extend_from_slice(device_id_bytes);
        body.extend_from_slice(content_hash);

        let url = format!("{}/hash", self.config.effective_hash_base_url());
        let response = self
            .client
            .post(url)
            .bearer_auth(access_token)
            .header("Content-Type", "application/octet-stream")
            .body(body)
            .send()
            .await?;

        decode_response(response).await
    }

    /// Retrieve the current rolling state for a device from GET /hash.
    pub async fn get_state(
        &self,
        access_token: &str,
        device_id: &str,
    ) -> CoreResult<StateResponse> {
        let url = format!(
            "{}/hash?device_id={}",
            self.config.effective_hash_base_url(),
            device_id
        );
        let response = self
            .client
            .get(url)
            .bearer_auth(access_token)
            .send()
            .await?;

        decode_response(response).await
    }
}

async fn decode_response<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> CoreResult<T> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(CoreError::UnexpectedResponse { status, body });
    }
    Ok(response.json::<T>().await?)
}

pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

pub fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Converts a UUID string ("xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx") to 16 raw bytes.
pub fn uuid_str_to_bytes(uuid: &str) -> Option<[u8; 16]> {
    let hex: String = uuid.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let hi = (chunk[0] as char).to_digit(16)?;
        let lo = (chunk[1] as char).to_digit(16)?;
        bytes[i] = (hi * 16 + lo) as u8;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::{sha256_hex, uuid_str_to_bytes};

    #[test]
    fn sha256_helpers_are_stable() {
        assert_eq!(
            sha256_hex(b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn uuid_str_to_bytes_roundtrip() {
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        let bytes = uuid_str_to_bytes(uuid).expect("valid uuid");
        assert_eq!(bytes[0], 0x55);
        assert_eq!(bytes[1], 0x0e);
        assert_eq!(bytes.len(), 16);
    }
}
