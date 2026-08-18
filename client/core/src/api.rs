use std::time::Duration;

use base64::Engine;
use rand_core::{OsRng, TryRngCore};
use serde::Deserialize;
use serde::Serialize;
use ureq::Agent;
use ureq::http::{Response, StatusCode};

use crate::config::Config;
use crate::crypto::derive_password_auth;
use crate::error::{CoreError, CoreResult};
use crate::model::{BatchUpload, DeviceCredentials, DeviceSettings, HashParams, NotifyPayload};

/// The whole codebase shares one version, tracked in `version.properties` (this crate's
/// grandparent directory). This is that version's `/vX`/`/vX.Y` URL-prefix form
/// (api/SPEC.md section 1.4, hash-server/SPEC.md section 1.3) — the same value is used
/// for both the main API and the standalone hash server. Kept in sync by
/// `scripts/update-version.sh`, which is the only thing that should ever edit this line.
const API_VERSION: &str = "v0.1";

#[derive(Debug, Clone, Deserialize)]
pub struct UploadedBatchResponse {
    pub id: String,
    pub settings: DeviceSettings,
    pub hash_token: String,
}

#[derive(Debug, Clone)]
pub struct RegisteredDevice {
    pub credentials: DeviceCredentials,
    pub settings: DeviceSettings,
    pub hash_token: String,
}

#[derive(Debug, Clone)]
pub struct DeviceState {
    pub settings: DeviceSettings,
    pub hash_token: String,
}

/// Raw wire shape of `DeviceSettings` embedded in `POST /d/device`, `GET /d/device`,
/// and `POST /d/batch` responses. Differs from the public `DeviceSettings` only in
/// its `id`/`device_id` field name and the extra `hash_token` (the JWT hash-server
/// token, pulled out separately by each caller below).
#[derive(Deserialize)]
struct DeviceRecipientResponse {
    user_id: String,
    pub_key: String,
}

#[derive(Deserialize)]
struct DeviceSettingsResponse {
    id: String,
    name: String,
    platform: String,
    #[serde(default)]
    wrapping_keys: Vec<DeviceRecipientResponse>,
    #[serde(default)]
    hash_base_url: Option<String>,
    hash_token: String,
}

impl From<DeviceSettingsResponse> for DeviceSettings {
    fn from(response: DeviceSettingsResponse) -> Self {
        DeviceSettings {
            device_id: response.id,
            name: response.name,
            platform: response.platform,
            wrapping_keys: response
                .wrapping_keys
                .into_iter()
                .map(|key| crate::model::BatchRecipient {
                    user_id: key.user_id,
                    pub_key_base64: key.pub_key,
                })
                .collect(),
            hash_base_url: response.hash_base_url,
        }
    }
}

pub trait ApiTransport: Send + Sync {
    fn logout(&self, device_refresh_token: &str) -> CoreResult<()>;
    fn register_device(
        &self,
        email: &str,
        password: &str,
        name: &str,
        platform: &str,
    ) -> CoreResult<RegisteredDevice>;
    fn get_device_settings(&self, device_refresh_token: &str) -> CoreResult<DeviceState>;
    fn upload_batch(
        &self,
        device_refresh_token: &str,
        batch: &BatchUpload,
    ) -> CoreResult<UploadedBatchResponse>;
    /// `unix_time` and `seq` are the wire-format prefix hash-server/SPEC.md §2.1
    /// requires ahead of the 32-byte content hash: `seq` MUST be strictly greater
    /// than the last `seq` accepted for this device (rejected with 409 otherwise).
    fn upload_hash(
        &self,
        hash_base_url: Option<&str>,
        hash_jwt: &str,
        unix_time: u32,
        seq: u32,
        content_hash: &[u8; 32],
    ) -> CoreResult<()>;
}

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct HttpApiClient {
    base_url: String,
    agent: Agent,
}

impl HttpApiClient {
    pub fn new(config: &Config) -> CoreResult<Self> {
        // `http_status_as_error(false)` keeps 4xx/5xx as ordinary responses so
        // `ensure_success` can read the body for the server's error message.
        let agent: Agent = Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .http_status_as_error(false)
            .build()
            .into();
        Ok(Self {
            base_url: config.api_base_url.trim_end_matches('/').to_string(),
            agent,
        })
    }
}

impl ApiTransport for HttpApiClient {
    fn logout(&self, device_refresh_token: &str) -> CoreResult<()> {
        let response = self
            .post(None, "/d/logout", Some(device_refresh_token))
            .send_empty()?;
        self.expect_success(response)
    }

    fn register_device(
        &self,
        email: &str,
        password: &str,
        name: &str,
        platform: &str,
    ) -> CoreResult<RegisteredDevice> {
        #[derive(Deserialize)]
        struct LoginMaterialResponse {
            password_salt: String,
            params: HashParams,
        }

        #[derive(Serialize)]
        struct RegisterDeviceRequest<'a> {
            email: &'a str,
            password_auth: String,
            name: &'a str,
            platform: &'a str,
        }

        #[derive(Deserialize)]
        struct RegisterDeviceResponse {
            token: String,
            settings: DeviceSettingsResponse,
        }

        let material: LoginMaterialResponse = self.expect_json(
            self.get(None, "/user/login-material", None)
                .query("email", email)
                .call()?,
        )?;
        let password_salt =
            base64::engine::general_purpose::STANDARD.decode(material.password_salt)?;
        let password_auth = derive_password_auth(password, &password_salt, &material.params)?;

        let response: RegisterDeviceResponse = self.expect_json(
            self.post(None, "/d/device", None)
                .send_json(RegisterDeviceRequest {
                    email,
                    password_auth: base64::engine::general_purpose::STANDARD.encode(password_auth),
                    name,
                    platform,
                })?,
        )?;

        let hash_token = response.settings.hash_token.clone();
        let settings: DeviceSettings = response.settings.into();
        Ok(RegisteredDevice {
            credentials: DeviceCredentials {
                device_id: settings.device_id.clone(),
                refresh_token: response.token,
            },
            settings,
            hash_token,
        })
    }

    fn get_device_settings(&self, device_refresh_token: &str) -> CoreResult<DeviceState> {
        let response: DeviceSettingsResponse = self.expect_json(
            self.get(None, "/d/device", Some(device_refresh_token))
                .call()?,
        )?;
        let hash_token = response.hash_token.clone();
        Ok(DeviceState {
            settings: response.into(),
            hash_token,
        })
    }

    fn upload_batch(
        &self,
        device_refresh_token: &str,
        batch: &BatchUpload,
    ) -> CoreResult<UploadedBatchResponse> {
        #[derive(Serialize)]
        struct EventCounts {
            total: u32,
            high: u32,
            medium: u32,
            screenshot: u32,
        }

        #[derive(Serialize)]
        struct BatchMetadata<'a> {
            start_time: i64,
            end_time: i64,
            access_keys: std::collections::BTreeMap<&'a str, &'a str>,
            event_counts: EventCounts,
            notifications: &'a [NotifyPayload],
        }

        #[derive(Deserialize)]
        struct UploadBatchResponse {
            id: String,
            settings: DeviceSettingsResponse,
        }

        let metadata = serde_json::to_string(&BatchMetadata {
            start_time: batch.start_time_ms,
            end_time: batch.end_time_ms,
            access_keys: batch
                .access_keys
                .iter()
                .map(|entry| (entry.user_id.as_str(), entry.hpke_key_base64.as_str()))
                .collect(),
            event_counts: EventCounts {
                total: batch.total_count,
                high: batch.high_risk_count,
                medium: batch.medium_risk_count,
                screenshot: batch.screenshot_count,
            },
            notifications: &batch.notifications,
        })?;

        let boundary = multipart_boundary();
        let body = encode_multipart(&boundary, &batch.bytes, &metadata);
        let response: UploadBatchResponse = self.expect_json(
            self.post(None, "/d/batch", Some(device_refresh_token))
                .content_type(format!("multipart/form-data; boundary={boundary}"))
                .send(&body)?,
        )?;
        let hash_token = response.settings.hash_token.clone();
        Ok(UploadedBatchResponse {
            id: response.id,
            settings: response.settings.into(),
            hash_token,
        })
    }

    fn upload_hash(
        &self,
        hash_base_url: Option<&str>,
        hash_jwt: &str,
        unix_time: u32,
        seq: u32,
        content_hash: &[u8; 32],
    ) -> CoreResult<()> {
        // hash-server/SPEC.md §2.1: [unix_time:u32 LE][seq:u32 LE][sha hash:32 bytes].
        let mut body = Vec::with_capacity(40);
        body.extend_from_slice(&unix_time.to_le_bytes());
        body.extend_from_slice(&seq.to_le_bytes());
        body.extend_from_slice(content_hash);

        let response = self
            .post(hash_base_url, "/hash", Some(hash_jwt))
            .content_type("application/octet-stream")
            .send(&body)?;
        self.expect_success(response)
    }
}

impl HttpApiClient {
    fn get(
        &self,
        base_override: Option<&str>,
        path: &str,
        bearer_token: Option<&str>,
    ) -> ureq::RequestBuilder<ureq::typestate::WithoutBody> {
        let builder = self.agent.get(self.url(base_override, path));
        match bearer_token {
            Some(token) => builder.header("Authorization", format!("Bearer {token}")),
            None => builder,
        }
    }

    fn post(
        &self,
        base_override: Option<&str>,
        path: &str,
        bearer_token: Option<&str>,
    ) -> ureq::RequestBuilder<ureq::typestate::WithBody> {
        let builder = self.agent.post(self.url(base_override, path));
        match bearer_token {
            Some(token) => builder.header("Authorization", format!("Bearer {token}")),
            None => builder,
        }
    }

    fn url(&self, base_override: Option<&str>, path: &str) -> String {
        let base = base_override
            .unwrap_or(&self.base_url)
            .trim_end_matches('/');
        format!("{base}/{API_VERSION}{path}")
    }

    fn expect_json<T: for<'de> Deserialize<'de>>(
        &self,
        response: Response<ureq::Body>,
    ) -> CoreResult<T> {
        let mut response = self.ensure_success(response)?;
        Ok(response.body_mut().read_json()?)
    }

    fn expect_success(&self, response: Response<ureq::Body>) -> CoreResult<()> {
        let _ = self.ensure_success(response)?;
        Ok(())
    }

    fn ensure_success(&self, response: Response<ureq::Body>) -> CoreResult<Response<ureq::Body>> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }

        let mut response = response;
        let body = response.body_mut().read_to_string().ok();
        let message = error_message_from_body(status, body.as_deref());

        Err(CoreError::HttpStatus {
            status: status.as_u16(),
            message,
        })
    }
}

/// Builds the `multipart/form-data` body `POST /d/batch` expects: the encrypted
/// batch as a `file` part plus a `metadata` text field. Hand-rolled because
/// ureq's own multipart support lives under its semver-exempt `unversioned`
/// module, and this form never varies.
fn encode_multipart(boundary: &str, batch: &[u8], metadata: &str) -> Vec<u8> {
    let mut body = Vec::with_capacity(batch.len() + metadata.len() + 256);
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"batch.enc\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(batch);
    body.extend_from_slice(format!("\r\n--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"metadata\"\r\n\r\n");
    body.extend_from_slice(metadata.as_bytes());
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

/// A random boundary, so it cannot collide with the encrypted batch bytes it
/// delimits.
fn multipart_boundary() -> String {
    let mut raw = [0u8; 16];
    OsRng.try_fill_bytes(&mut raw).expect("OS RNG unavailable");
    let mut boundary = String::with_capacity(38);
    boundary.push_str("virtue-");
    for byte in raw {
        boundary.push_str(&format!("{byte:02x}"));
    }
    boundary
}

/// Derives a non-empty error message from a non-2xx response body, falling
/// back to a status-line-bearing message (e.g. "HTTP 502 Bad Gateway error")
/// when the body is missing, empty, or unparseable.
fn error_message_from_body(status: StatusCode, body: Option<&str>) -> String {
    let fallback = body
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("HTTP {status} error"));

    match serde_json::from_str::<ApiErrorResponse>(&fallback) {
        Ok(body) => body
            .details
            .as_ref()
            .and_then(format_api_details)
            .or_else(|| body.error.filter(|s| !s.is_empty()))
            .unwrap_or(fallback),
        Err(_) => fallback,
    }
}

fn format_api_details(details: &serde_json::Value) -> Option<String> {
    // Manual format: {"errors": ["msg"]}
    if let Some(errors) = details.get("errors").and_then(|e| e.as_array()) {
        let msgs: Vec<&str> = errors.iter().filter_map(|e| e.as_str()).collect();
        if !msgs.is_empty() {
            return Some(msgs.join("; "));
        }
    }

    // Zod treeify format: {_errors: [...], field: {_errors: [...]}}
    let mut parts: Vec<String> = Vec::new();
    if let Some(obj) = details.as_object() {
        if let Some(errs) = obj.get("_errors").and_then(|e| e.as_array()) {
            for e in errs.iter().filter_map(|e| e.as_str()) {
                parts.push(e.to_string());
            }
        }
        for (key, value) in obj {
            if key == "_errors" {
                continue;
            }
            if let Some(field_errs) = value.get("_errors").and_then(|e| e.as_array()) {
                let msgs: Vec<&str> = field_errs.iter().filter_map(|e| e.as_str()).collect();
                if !msgs.is_empty() {
                    parts.push(msgs.join(", "));
                }
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

#[derive(Deserialize)]
struct ApiErrorResponse {
    error: Option<String>,
    details: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_body_has_both_parts_in_order() {
        let body = encode_multipart("BOUND", &[0xde, 0xad], "{\"a\":1}");
        let text = String::from_utf8_lossy(&body);

        assert!(text.starts_with("--BOUND\r\n"));
        assert!(text.contains(
            "Content-Disposition: form-data; name=\"file\"; filename=\"batch.enc\"\r\n\
             Content-Type: application/octet-stream\r\n\r\n"
        ));
        assert!(text.contains("Content-Disposition: form-data; name=\"metadata\"\r\n\r\n"));
        assert!(text.ends_with("\r\n--BOUND--\r\n"));
        assert!(
            text.find("name=\"file\"") < text.find("name=\"metadata\""),
            "the file part must come first"
        );
    }

    #[test]
    fn multipart_body_keeps_batch_bytes_verbatim() {
        // The batch is AES-GCM ciphertext: arbitrary bytes, including ones that
        // look like CRLFs and boundary markers. None of it may be escaped or
        // re-encoded on the way out.
        let batch: Vec<u8> = (0u8..=255).collect();
        let body = encode_multipart("BOUND", &batch, "{}");

        let header_end = body
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("file part headers")
            + 4;
        assert_eq!(&body[header_end..header_end + batch.len()], &batch[..]);
    }

    #[test]
    fn multipart_boundaries_are_unique_and_well_formed() {
        let a = multipart_boundary();
        let b = multipart_boundary();
        assert_ne!(a, b, "each request needs a fresh boundary");
        // RFC 2046 §5.1.1: boundaries are at most 70 chars from a restricted set.
        assert!(a.len() <= 70);
        assert!(
            a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "unexpected character in boundary {a}"
        );
    }

    #[test]
    fn error_message_from_body_falls_back_on_empty_body() {
        let message = error_message_from_body(StatusCode::from_u16(401).unwrap(), Some(""));
        assert!(!message.is_empty());
        assert!(message.contains("401"));
    }

    #[test]
    fn error_message_from_body_falls_back_on_missing_body() {
        let message = error_message_from_body(StatusCode::from_u16(500).unwrap(), None);
        assert!(!message.is_empty());
        assert!(message.contains("500"));
    }

    #[test]
    fn error_message_from_body_includes_reason_phrase_on_fallback() {
        let message = error_message_from_body(StatusCode::from_u16(502).unwrap(), None);
        assert!(message.contains("502"));
        assert!(message.contains("Bad Gateway"));
    }

    #[test]
    fn error_message_from_body_uses_error_field() {
        let message = error_message_from_body(
            StatusCode::from_u16(400).unwrap(),
            Some(r#"{"error":"bad email"}"#),
        );
        assert_eq!(message, "bad email");
    }
}
