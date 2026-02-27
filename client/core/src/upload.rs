use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use reqwest::multipart::{Form, Part};
use sha2::{Digest, Sha256};

use crate::batch::BatchBlob;
use crate::error::{CoreError, CoreResult};
use crate::models::{BatchUploadResponse, HashUploadResponse};
use crate::resolve_base_api_url;

#[derive(Clone, Debug)]
pub struct UploadClientConfig {
    pub base_url: String,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
}

impl Default for UploadClientConfig {
    fn default() -> Self {
        Self {
            base_url: resolve_base_api_url(),
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(60),
        }
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
    pub async fn upload_batch(
        &self,
        access_token: &str,
        device_id: &str,
        blob: &BatchBlob,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        start_chain_hash: &[u8; 32],
        end_chain_hash: &[u8; 32],
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
            .text("start_time", start_time.to_rfc3339_opts(SecondsFormat::Millis, true))
            .text("end_time", end_time.to_rfc3339_opts(SecondsFormat::Millis, true))
            .text("start_chain_hash", hex::encode(start_chain_hash))
            .text("end_chain_hash", hex::encode(end_chain_hash))
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

    /// Upload a raw 32-byte chain hash to the API.
    pub async fn upload_hash(
        &self,
        access_token: &str,
        device_id: &str,
        hash: &[u8; 32],
        client_timestamp: DateTime<Utc>,
    ) -> CoreResult<HashUploadResponse> {
        let url = format!("{}/hash", self.config.base_url);
        let response = self
            .client
            .post(url)
            .bearer_auth(access_token)
            .header("X-Device-ID", device_id)
            .header(
                "X-Client-Timestamp",
                client_timestamp.to_rfc3339_opts(SecondsFormat::Millis, true),
            )
            .header("Content-Type", "application/octet-stream")
            .body(hash.to_vec())
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
