use std::time::Duration;

use base64::Engine;
use chrono::{Duration as ChronoDuration, Utc};
use rand::thread_rng;
use reqwest::header::CONTENT_TYPE;
use sha2::{Digest, Sha256};

use crate::error::{CoreError, CoreResult};
use crate::models::{CreateImageRequest, CreateImageResponse};
use crate::queue::PersistentQueue;
use crate::resolve_base_api_url;
use crate::schedule::RetryPolicy;

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
            request_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct QueueProcessReport {
    pub uploaded: usize,
    pub retried: usize,
    pub dropped: usize,
    pub remaining: usize,
    pub last_error: Option<String>,
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

    pub async fn create_signed_upload_url(
        &self,
        access_token: &str,
        request: &CreateImageRequest,
    ) -> CoreResult<CreateImageResponse> {
        let url = format!("{}/image", self.config.base_url);
        let response = self
            .client
            .post(url)
            .bearer_auth(access_token)
            .json(request)
            .send()
            .await?;

        decode_response(response).await
    }

    pub async fn upload_to_signed_url(
        &self,
        upload_url: &str,
        payload: &[u8],
        content_type: &str,
        expected_sha256_hex: &str,
    ) -> CoreResult<()> {
        let computed_sha256 = sha256_hex(payload);
        if !computed_sha256.eq_ignore_ascii_case(expected_sha256_hex) {
            return Err(CoreError::ChecksumMismatch {
                expected: expected_sha256_hex.to_string(),
                actual: computed_sha256,
            });
        }

        let checksum_b64 = sha256_base64(payload);

        let response = self
            .client
            .put(upload_url)
            .header(CONTENT_TYPE, content_type)
            .header("x-amz-checksum-sha256", checksum_b64)
            .body(payload.to_vec())
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(CoreError::UnexpectedResponse { status, body });
        }

        Ok(())
    }

    pub async fn create_and_upload(
        &self,
        access_token: &str,
        request: CreateImageRequest,
        payload: &[u8],
    ) -> CoreResult<CreateImageResponse> {
        let created = self
            .create_signed_upload_url(access_token, &request)
            .await?;

        self.upload_to_signed_url(
            &created.upload_url,
            payload,
            &request.content_type,
            &request.sha256,
        )
        .await?;

        Ok(created)
    }

    pub async fn process_upload_queue(
        &self,
        queue: &PersistentQueue,
        retry_policy: &RetryPolicy,
        access_token: &str,
        max_items: usize,
    ) -> CoreResult<QueueProcessReport> {
        let mut report = QueueProcessReport::default();
        let mut rng = thread_rng();

        for _ in 0..max_items {
            if !queue.front_is_ready(Utc::now())? {
                break;
            }

            let Some(next_item) = queue.peek_front()? else {
                break;
            };

            let request = CreateImageRequest {
                device_id: next_item.device_id.clone(),
                sha256: next_item.sha256_hex.clone(),
                content_type: next_item.content_type.clone(),
                size_bytes: next_item.payload.len() as u64,
                taken_at: next_item.taken_at,
            };

            match self
                .create_and_upload(access_token, request, &next_item.payload)
                .await
            {
                Ok(_) => {
                    queue.pop_front()?;
                    report.uploaded += 1;
                }
                Err(err) => {
                    report.last_error = Some(err.to_string());

                    let next_attempt = next_item.attempts.saturating_add(1);
                    let retriable = is_retriable_error(&err);
                    let attempts_remaining = next_attempt < retry_policy.max_attempts;

                    if retriable && attempts_remaining {
                        let delay = retry_policy.next_delay(next_attempt, &mut rng);
                        let chrono_delay = ChronoDuration::from_std(delay)
                            .map_err(|e| CoreError::Time(e.to_string()))?;
                        queue.mark_front_retry(Utc::now() + chrono_delay)?;
                        report.retried += 1;
                    } else {
                        queue.pop_front()?;
                        report.dropped += 1;
                    }
                }
            }
        }

        report.remaining = queue.len()?;
        Ok(report)
    }
}

fn is_retriable_error(err: &CoreError) -> bool {
    match err {
        CoreError::Http(inner) => inner.is_timeout() || inner.is_connect(),
        CoreError::UnexpectedResponse { status, .. } => {
            *status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
        }
        CoreError::Io(_) => true,
        _ => false,
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
    let digest = hasher.finalize();
    hex::encode(digest)
}

pub fn sha256_base64(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize();
    base64::engine::general_purpose::STANDARD.encode(digest)
}

#[cfg(test)]
mod tests {
    use super::{sha256_base64, sha256_hex};

    #[test]
    fn sha256_helpers_are_stable() {
        let bytes = b"hello";

        assert_eq!(
            sha256_hex(bytes),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(
            sha256_base64(bytes),
            "LPJNul+wow4m6DsqxbninhsWHlwfp0JecwQzYpOLmCQ="
        );
    }
}
