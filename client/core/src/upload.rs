use std::time::Duration;

use chrono::{DateTime, Utc};
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
    fn hash_base_url(&self) -> CoreResult<&str> {
        self.hash_base_url.as_deref().ok_or_else(|| {
            CoreError::TokenStore(
                "hash_base_url is not configured; fetch /d/device before calling /hash".to_string(),
            )
        })
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

    /// Encrypt, compress and upload a batch blob to the device API.
    /// Returns the uploaded batch metadata from `POST /d/batch`.
    pub async fn upload_batch(
        &self,
        access_token: &str,
        _device_id: &str,
        blob: &BatchBlob,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        key: &[u8; 32],
    ) -> CoreResult<BatchUploadResponse> {
        let encrypted = blob.encode_encrypted(key)?;

        let file_part = Part::bytes(encrypted)
            .file_name("batch.enc")
            .mime_str("application/octet-stream")?;

        let form = Form::new()
            .text("start_time", start_time.timestamp_millis().to_string())
            .text("end_time", end_time.timestamp_millis().to_string())
            .part("file", file_part);

        let url = format!("{}/d/batch", self.config.base_url);
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
    /// The server computes and stores `new_state = sha256(current_state || content_hash)`.
    pub async fn upload_hash(
        &self,
        access_token: &str,
        content_hash: &[u8; 32],
    ) -> CoreResult<HashUploadResponse> {
        let url = format!("{}/hash", self.config.hash_base_url()?);
        let response = self
            .client
            .post(url)
            .bearer_auth(access_token)
            .header("Content-Type", "application/octet-stream")
            .body(content_hash.to_vec())
            .send()
            .await?;

        decode_response(response).await
    }

    /// Retrieve the current rolling state for a device from GET /hash.
    pub async fn get_state(&self, access_token: &str) -> CoreResult<StateResponse> {
        let url = format!("{}/hash", self.config.hash_base_url()?);
        let response = self
            .client
            .get(url)
            .bearer_auth(access_token)
            .send()
            .await?;

        decode_bytes_response(response).await
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

async fn decode_bytes_response(response: reqwest::Response) -> CoreResult<StateResponse> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(CoreError::UnexpectedResponse { status, body });
    }

    let body = response.bytes().await?;
    let bytes: [u8; 32] = body
        .as_ref()
        .try_into()
        .map_err(|_| CoreError::UnexpectedResponse {
            status: reqwest::StatusCode::OK,
            body: format!("expected 32 bytes from /hash, got {}", body.len()),
        })?;
    Ok(StateResponse { state: bytes })
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

#[cfg(test)]
mod tests {
    use super::sha256_hex;

    #[test]
    fn sha256_helpers_are_stable() {
        assert_eq!(
            sha256_hex(b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}
