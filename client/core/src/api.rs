use base64::Engine;
use reqwest::Method;
use reqwest::blocking::multipart::{Form, Part};
use reqwest::blocking::{Client, RequestBuilder, Response};
use serde::Deserialize;
use serde::Serialize;

use crate::config::Config;
use crate::crypto::derive_password_auth;
use crate::error::{CoreError, CoreResult};
use crate::model::{BatchUpload, DeviceCredentials, DeviceSettings, HashParams, NotifyPayload};

#[derive(Debug, Clone, Deserialize)]
pub struct UploadedBatchResponse {
    pub id: String,
}

pub trait ApiTransport: Send + Sync {
    fn login(&self, username: &str, password: &str) -> CoreResult<String>;
    fn logout(&self) -> CoreResult<()>;
    fn register_device(
        &self,
        user_refresh_token: &str,
        name: &str,
        platform: &str,
    ) -> CoreResult<DeviceCredentials>;
    fn get_device_settings(&self, device_refresh_token: &str) -> CoreResult<DeviceSettings>;
    fn get_hash_token(&self, device_refresh_token: &str) -> CoreResult<String>;
    fn upload_batch(
        &self,
        device_refresh_token: &str,
        batch: &BatchUpload,
    ) -> CoreResult<UploadedBatchResponse>;
    fn notify(&self, device_refresh_token: &str, payload: &NotifyPayload) -> CoreResult<()>;
    fn upload_hash(
        &self,
        hash_base_url: Option<&str>,
        hash_jwt: &str,
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

    fn login(&self, username: &str, password: &str) -> CoreResult<String> {
        #[derive(Serialize)]
        struct LoginRequest<'a> {
            email: &'a str,
            password_auth: String,
        }

        #[derive(Deserialize)]
        struct LoginMaterialResponse {
            password_salt: String,
            params: HashParams,
        }

        #[derive(Deserialize)]
        struct LoginResponse {
            refresh_token: String,
        }

        let material: LoginMaterialResponse = self.expect_json(
            self.request(Method::GET, None, "/user/login-material", None)
                .query(&[("email", username)])
                .send()?,
        )?;
        let password_salt =
            base64::engine::general_purpose::STANDARD.decode(material.password_salt)?;
        let password_auth = derive_password_auth(password, &password_salt, &material.params)?;

        let response: LoginResponse = self.send_json(
            Method::POST,
            None,
            "/login",
            None,
            Some(&LoginRequest {
                email: username,
                password_auth: base64::engine::general_purpose::STANDARD.encode(password_auth),
            }),
        )?;
        Ok(response.refresh_token)
    }

    fn logout(&self) -> CoreResult<()> {
        self.send_empty(Method::POST, None, "/logout", None, None::<&()>)
    }

    fn register_device(
        &self,
        user_refresh_token: &str,
        name: &str,
        platform: &str,
    ) -> CoreResult<DeviceCredentials> {
        #[derive(Serialize)]
        struct RegisterDeviceRequest<'a> {
            name: &'a str,
            platform: &'a str,
        }

        #[derive(Deserialize)]
        struct RegisterDeviceResponse {
            id: String,
            refresh_token: String,
        }

        let response: RegisterDeviceResponse = self.send_json(
            Method::POST,
            None,
            "/d/device",
            Some(user_refresh_token),
            Some(&RegisterDeviceRequest { name, platform }),
        )?;

        Ok(DeviceCredentials {
            device_id: response.id,
            refresh_token: response.refresh_token,
        })
    }

    fn get_device_settings(&self, device_refresh_token: &str) -> CoreResult<DeviceSettings> {
        #[derive(Deserialize)]
        struct DeviceSettingsResponse {
            id: String,
            name: String,
            platform: String,
            wrapping_keys: Vec<DeviceRecipientResponse>,
            hash_base_url: Option<String>,
        }

        #[derive(Deserialize)]
        struct DeviceRecipientResponse {
            user_id: String,
            pub_key: String,
        }

        let response: DeviceSettingsResponse = self.send_json(
            Method::GET,
            None,
            "/d/device",
            Some(device_refresh_token),
            None::<&()>,
        )?;
        Ok(DeviceSettings {
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
        })
    }

    fn get_hash_token(&self, device_refresh_token: &str) -> CoreResult<String> {
        #[derive(Deserialize)]
        struct HashTokenResponse {
            hash_token: String,
        }

        let response: HashTokenResponse = self.send_json(
            Method::POST,
            None,
            "/d/token",
            Some(device_refresh_token),
            None::<&()>,
        )?;
        Ok(response.hash_token)
    }

    fn upload_batch(
        &self,
        device_refresh_token: &str,
        batch: &BatchUpload,
    ) -> CoreResult<UploadedBatchResponse> {
        #[derive(Serialize)]
        struct AccessKeysPayload<'a> {
            keys: Vec<AccessKeyEntry<'a>>,
        }

        #[derive(Serialize)]
        struct AccessKeyEntry<'a> {
            user_id: &'a str,
            hpke_key: &'a str,
        }

        let part = Part::bytes(batch.bytes.clone())
            .file_name("batch.enc")
            .mime_str("application/octet-stream")?;
        let access_keys = serde_json::to_string(&AccessKeysPayload {
            keys: batch
                .access_keys
                .iter()
                .map(|entry| AccessKeyEntry {
                    user_id: &entry.user_id,
                    hpke_key: &entry.hpke_key_base64,
                })
                .collect(),
        })?;
        let form = Form::new()
            .part("file", part)
            .text("start_time", batch.start_time_ms.to_string())
            .text("end_time", batch.end_time_ms.to_string())
            .text("high_risk_count", batch.high_risk_count.to_string())
            .text("medium_risk_count", batch.medium_risk_count.to_string())
            .text("access_keys", access_keys);

        self.send_form(Method::POST, None, "/d/batch", device_refresh_token, form)
    }

    fn notify(&self, device_refresh_token: &str, payload: &NotifyPayload) -> CoreResult<()> {
        self.send_empty(
            Method::POST,
            None,
            "/d/notify",
            Some(device_refresh_token),
            Some(payload),
        )
    }

    fn upload_hash(
        &self,
        hash_base_url: Option<&str>,
        hash_jwt: &str,
        content_hash: &[u8; 32],
    ) -> CoreResult<()> {
        let response = self
            .request(Method::POST, hash_base_url, "/hash", Some(hash_jwt))
            .header("Content-Type", "application/octet-stream")
            .body(content_hash.to_vec())
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
        body: Option<&TBody>,
    ) -> CoreResult<TResponse>
    where
        TBody: Serialize + ?Sized,
        TResponse: for<'de> Deserialize<'de>,
    {
        let mut request = self.request(method, base_override, path, bearer_token);
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

    fn send_form(
        &self,
        method: Method,
        base_override: Option<&str>,
        path: &str,
        bearer_token: &str,
        form: Form,
    ) -> CoreResult<UploadedBatchResponse> {
        let response = self
            .request(method, base_override, path, Some(bearer_token))
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

        let fallback = response
            .text()
            .unwrap_or_else(|_| format!("HTTP {} error", status.as_u16()));
        let message = match serde_json::from_str::<ApiErrorResponse>(&fallback) {
            Ok(body) => body
                .details
                .as_ref()
                .and_then(format_api_details)
                .or_else(|| body.error.filter(|s| !s.is_empty()))
                .unwrap_or(fallback),
            Err(_) => fallback,
        };

        Err(CoreError::HttpStatus {
            status: status.as_u16(),
            message,
        })
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
