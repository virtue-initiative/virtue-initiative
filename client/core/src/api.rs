use base64::Engine;
use reqwest::Method;
use reqwest::blocking::multipart::{Form, Part};
use reqwest::blocking::{Client, RequestBuilder, Response};
use serde::Deserialize;
use serde::Serialize;

use crate::config::Config;
use crate::crypto::derive_password_auth;
use crate::error::{CoreError, CoreResult};
use crate::model::{BatchUpload, DeviceCredentials, DeviceSettings, HashParams};

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

/// Raw wire shape for the `{settings, token}` pair embedded in `POST /d/device`,
/// `GET /d/device`, and `POST /d/batch` responses. `DeviceSettingsResponse` differs
/// from the public `DeviceSettings` only in its `id`/`device_id` field name.
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

#[derive(Deserialize)]
struct DeviceStateResponse {
    settings: DeviceSettingsResponse,
    token: String,
}

pub trait ApiTransport: Send + Sync {
    fn logout(&self, device_refresh_token: &str) -> CoreResult<()>;
    /// `pubkey` (base64 raw Ed25519 pubkey) is sent as `X-Device-Pubkey` on
    /// every call that can mint/refresh a device-cert — the server never
    /// persists it, so it must be resent on every such call, not just once
    /// at registration.
    fn register_device(
        &self,
        email: &str,
        password: &str,
        name: &str,
        platform: &str,
        pubkey: &str,
    ) -> CoreResult<RegisteredDevice>;
    fn get_device_settings(
        &self,
        device_refresh_token: &str,
        pubkey: &str,
    ) -> CoreResult<DeviceState>;
    fn upload_batch(
        &self,
        device_refresh_token: &str,
        pubkey: &str,
        batch: &BatchUpload,
    ) -> CoreResult<UploadedBatchResponse>;
    /// Signs the request with `signing_key` (see `crypto::sign_request`) —
    /// always, regardless of whether `hash_jwt` is a `hash-server`- or
    /// `device-cert`-typed token, since the local D1-backed fallback simply
    /// doesn't check for the signature headers while the real hash-server
    /// enforces them.
    fn upload_hash(
        &self,
        hash_base_url: Option<&str>,
        hash_jwt: &str,
        device_id: &str,
        signing_key: &[u8; 32],
        content_hash: &[u8; 32],
    ) -> CoreResult<()>;

    /// Apply an updated API base URL to the transport in place.
    fn reconfigure(&mut self, api_base_url: &str) -> CoreResult<()>;
}

#[derive(Debug, Clone)]
pub struct ReqwestApiClient {
    base_url: String,
    client: Client,
}

impl ReqwestApiClient {
    pub fn new(config: &Config) -> CoreResult<Self> {
        let client = Client::builder()
            .cookie_store(true)
            .timeout(std::time::Duration::from_secs(5))
            .build()?;
        Ok(Self {
            base_url: config.api_base_url.trim_end_matches('/').to_string(),
            client,
        })
    }
}

impl ApiTransport for ReqwestApiClient {
    fn reconfigure(&mut self, api_base_url: &str) -> CoreResult<()> {
        self.base_url = api_base_url.trim_end_matches('/').to_string();
        Ok(())
    }

    fn logout(&self, device_refresh_token: &str) -> CoreResult<()> {
        self.send_empty(
            Method::POST,
            None,
            "/d/logout",
            Some(device_refresh_token),
            None::<&()>,
        )
    }

    fn register_device(
        &self,
        email: &str,
        password: &str,
        name: &str,
        platform: &str,
        pubkey: &str,
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
            refresh_token: String,
            #[serde(flatten)]
            state: DeviceStateResponse,
        }

        let material: LoginMaterialResponse = self.expect_json(
            self.request(Method::GET, None, "/user/login-material", None)
                .query(&[("email", email)])
                .send()?,
        )?;
        let password_salt =
            base64::engine::general_purpose::STANDARD.decode(material.password_salt)?;
        let password_auth = derive_password_auth(password, &password_salt, &material.params)?;

        let response: RegisterDeviceResponse = self.send_json(
            Method::POST,
            None,
            "/d/device",
            None,
            Some(pubkey),
            Some(&RegisterDeviceRequest {
                email,
                password_auth: base64::engine::general_purpose::STANDARD.encode(password_auth),
                name,
                platform,
            }),
        )?;

        let settings: DeviceSettings = response.state.settings.into();
        Ok(RegisteredDevice {
            credentials: DeviceCredentials {
                device_id: settings.device_id.clone(),
                refresh_token: response.refresh_token,
                // register_device only ever sees the device's pubkey (the
                // `pubkey` param above), never the raw private key — the
                // caller (module/auth.rs's do_login) overwrites this with
                // the real signing key immediately after this call returns.
                signing_key: [0u8; 32],
            },
            settings,
            hash_token: response.state.token,
        })
    }

    fn get_device_settings(
        &self,
        device_refresh_token: &str,
        pubkey: &str,
    ) -> CoreResult<DeviceState> {
        let response: DeviceStateResponse = self.send_json(
            Method::GET,
            None,
            "/d/device",
            Some(device_refresh_token),
            Some(pubkey),
            None::<&()>,
        )?;
        Ok(DeviceState {
            settings: response.settings.into(),
            hash_token: response.token,
        })
    }

    fn upload_batch(
        &self,
        device_refresh_token: &str,
        pubkey: &str,
        batch: &BatchUpload,
    ) -> CoreResult<UploadedBatchResponse> {
        #[derive(Serialize)]
        struct AccessKeysPayload<'a> {
            keys: std::collections::BTreeMap<&'a str, &'a str>,
        }

        #[derive(Deserialize)]
        struct UploadBatchResponse {
            id: String,
            #[serde(flatten)]
            state: DeviceStateResponse,
        }

        let part = Part::bytes(batch.bytes.clone())
            .file_name("batch.enc")
            .mime_str("application/octet-stream")?;
        let access_keys = serde_json::to_string(&AccessKeysPayload {
            keys: batch
                .access_keys
                .iter()
                .map(|entry| (entry.user_id.as_str(), entry.hpke_key_base64.as_str()))
                .collect(),
        })?;
        let mut form = Form::new()
            .part("file", part)
            .text("start_time", batch.start_time_ms.to_string())
            .text("end_time", batch.end_time_ms.to_string())
            .text("high_risk_count", batch.high_risk_count.to_string())
            .text("medium_risk_count", batch.medium_risk_count.to_string())
            .text("access_keys", access_keys);
        if !batch.notifications.is_empty() {
            form = form.text(
                "notifications",
                serde_json::to_string(&batch.notifications)?,
            );
        }

        let response: UploadBatchResponse = self.send_form(
            Method::POST,
            None,
            "/d/batch",
            device_refresh_token,
            pubkey,
            form,
        )?;
        Ok(UploadedBatchResponse {
            id: response.id,
            settings: response.state.settings.into(),
            hash_token: response.state.token,
        })
    }

    fn upload_hash(
        &self,
        hash_base_url: Option<&str>,
        hash_jwt: &str,
        device_id: &str,
        signing_key: &[u8; 32],
        content_hash: &[u8; 32],
    ) -> CoreResult<()> {
        let body = content_hash.to_vec();
        let timestamp_ms = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| CoreError::InvalidState("system clock before unix epoch"))?
                .as_millis(),
        )
        .map_err(|_| CoreError::InvalidState("system clock overflow"))?;
        let signature = crate::crypto::sign_request(
            signing_key,
            timestamp_ms,
            device_id,
            "POST",
            "/hash",
            &body,
        );

        let response = self
            .request(Method::POST, hash_base_url, "/hash", Some(hash_jwt))
            .header("Content-Type", "application/octet-stream")
            .header("X-Signature-Timestamp", timestamp_ms.to_string())
            .header(
                "X-Signature",
                base64::engine::general_purpose::STANDARD.encode(signature),
            )
            .body(body)
            .send()?;
        self.expect_success(response)
    }
}

impl ReqwestApiClient {
    fn send_json<TBody, TResponse>(
        &self,
        method: Method,
        base_override: Option<&str>,
        path: &str,
        bearer_token: Option<&str>,
        pubkey: Option<&str>,
        body: Option<&TBody>,
    ) -> CoreResult<TResponse>
    where
        TBody: Serialize + ?Sized,
        TResponse: for<'de> Deserialize<'de>,
    {
        let mut request = self.request(method, base_override, path, bearer_token);
        if let Some(pubkey) = pubkey {
            request = request.header("X-Device-Pubkey", pubkey);
        }
        if let Some(body) = body {
            request = request.json(body);
        }

        let response = request.send()?;
        self.expect_json(response)
    }

    fn send_empty<TBody>(
        &self,
        method: Method,
        base_override: Option<&str>,
        path: &str,
        bearer_token: Option<&str>,
        body: Option<&TBody>,
    ) -> CoreResult<()>
    where
        TBody: Serialize + ?Sized,
    {
        let mut request = self.request(method, base_override, path, bearer_token);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send()?;
        self.expect_success(response)
    }

    fn send_form<TResponse>(
        &self,
        method: Method,
        base_override: Option<&str>,
        path: &str,
        bearer_token: &str,
        pubkey: &str,
        form: Form,
    ) -> CoreResult<TResponse>
    where
        TResponse: for<'de> Deserialize<'de>,
    {
        let response = self
            .request(method, base_override, path, Some(bearer_token))
            .header("X-Device-Pubkey", pubkey)
            .multipart(form)
            .send()?;
        self.expect_json(response)
    }

    fn request(
        &self,
        method: Method,
        base_override: Option<&str>,
        path: &str,
        bearer_token: Option<&str>,
    ) -> RequestBuilder {
        let base = base_override
            .unwrap_or(&self.base_url)
            .trim_end_matches('/');
        let url = format!("{base}{path}");
        let mut request = self.client.request(method, url);
        if let Some(token) = bearer_token {
            request = request.bearer_auth(token);
        }
        request
    }

    fn expect_json<T: for<'de> Deserialize<'de>>(&self, response: Response) -> CoreResult<T> {
        let response = self.ensure_success(response)?;
        Ok(response.json()?)
    }

    fn expect_success(&self, response: Response) -> CoreResult<()> {
        let _ = self.ensure_success(response)?;
        Ok(())
    }

    fn ensure_success(&self, response: Response) -> CoreResult<Response> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }

        let body = response.text().ok();
        let message = error_message_from_body(status, body.as_deref());

        Err(CoreError::HttpStatus {
            status: status.as_u16(),
            message,
        })
    }
}

/// Derives a non-empty error message from a non-2xx response body, falling
/// back to a status-line-bearing message (e.g. "HTTP 502 Bad Gateway error")
/// when the body is missing, empty, or unparseable.
fn error_message_from_body(status: reqwest::StatusCode, body: Option<&str>) -> String {
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
    fn error_message_from_body_falls_back_on_empty_body() {
        let message =
            error_message_from_body(reqwest::StatusCode::from_u16(401).unwrap(), Some(""));
        assert!(!message.is_empty());
        assert!(message.contains("401"));
    }

    #[test]
    fn error_message_from_body_falls_back_on_missing_body() {
        let message = error_message_from_body(reqwest::StatusCode::from_u16(500).unwrap(), None);
        assert!(!message.is_empty());
        assert!(message.contains("500"));
    }

    #[test]
    fn error_message_from_body_includes_reason_phrase_on_fallback() {
        let message = error_message_from_body(reqwest::StatusCode::from_u16(502).unwrap(), None);
        assert!(message.contains("502"));
        assert!(message.contains("Bad Gateway"));
    }

    #[test]
    fn error_message_from_body_uses_error_field() {
        let message = error_message_from_body(
            reqwest::StatusCode::from_u16(400).unwrap(),
            Some(r#"{"error":"bad email"}"#),
        );
        assert_eq!(message, "bad email");
    }
}
