use reqwest::Method;
use reqwest::blocking::multipart::{Form, Part};
use reqwest::blocking::{Client, RequestBuilder, Response};
use serde::Deserialize;
use serde::Serialize;
use base64::Engine;

use crate::config::Config;
use crate::crypto::derive_password_auth;
use crate::error::{CoreError, CoreResult};
use crate::model::{BatchUpload, DeviceCredentials, DeviceSettings, HashParams, LogEntry};

#[derive(Debug, Clone)]
pub struct ApiClient {
    base_url: String,
    client: Client,
}

impl ApiClient {
    pub fn new(config: &Config) -> CoreResult<Self> {
        let client = Client::builder().cookie_store(true).build()?;
        Ok(Self {
            base_url: config.api_base_url.trim_end_matches('/').to_string(),
            client,
        })
    }

    pub fn login(&self, username: &str, password: &str) -> CoreResult<String> {
        #[derive(Serialize)]
        struct LoginRequest<'a> {
            email: &'a str,
            password_auth: String,
        }

        #[derive(Deserialize)]
        struct LoginResponse {
            access_token: String,
        }

        #[derive(Deserialize)]
        struct LoginMaterialResponse {
            password_salt: String,
            params: HashParams,
        }

        let material: LoginMaterialResponse = self.expect_json(
            self.request(Method::GET, None, "/user/login-material", None)
                .query(&[("email", username)])
                .send()?,
        )?;
        let password_salt = base64::engine::general_purpose::STANDARD
            .decode(material.password_salt)?;
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
        Ok(response.access_token)
    }

    pub fn logout(&self, access_token: &str) -> CoreResult<()> {
        self.send_empty(Method::POST, None, "/logout", Some(access_token))
    }

    pub fn register_device(
        &self,
        access_token: &str,
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
            access_token: String,
            refresh_token: String,
        }

        let response: RegisterDeviceResponse = self.send_json(
            Method::POST,
            None,
            "/d/device",
            Some(access_token),
            Some(&RegisterDeviceRequest { name, platform }),
        )?;

        Ok(DeviceCredentials {
            device_id: response.id,
            access_token: response.access_token,
            refresh_token: response.refresh_token,
        })
    }

    pub fn get_device_settings(&self, device_access_token: &str) -> CoreResult<DeviceSettings> {
        #[derive(Deserialize)]
        struct DeviceSettingsResponse {
            id: String,
            name: String,
            platform: String,
            enabled: bool,
            owner: Option<DeviceRecipientResponse>,
            partners: Vec<DeviceRecipientResponse>,
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
            Some(device_access_token),
            None::<&()>,
        )?;
        Ok(DeviceSettings {
            device_id: response.id,
            name: response.name,
            platform: response.platform,
            enabled: response.enabled,
            owner: response.owner.map(|owner| crate::model::BatchRecipient {
                user_id: owner.user_id,
                pub_key_base64: owner.pub_key,
            }),
            partners: response
                .partners
                .into_iter()
                .map(|partner| crate::model::BatchRecipient {
                    user_id: partner.user_id,
                    pub_key_base64: partner.pub_key,
                })
                .collect(),
            hash_base_url: response.hash_base_url,
        })
    }

    pub fn refresh_device_token(&self, refresh_token: &str) -> CoreResult<String> {
        #[derive(Serialize)]
        struct RefreshRequest<'a> {
            refresh_token: &'a str,
        }

        #[derive(Deserialize)]
        struct RefreshResponse {
            access_token: String,
        }

        let response: RefreshResponse = self.send_json(
            Method::POST,
            None,
            "/d/token",
            None,
            Some(&RefreshRequest { refresh_token }),
        )?;
        Ok(response.access_token)
    }

    pub fn upload_batch(&self, device_access_token: &str, batch: &BatchUpload) -> CoreResult<()> {
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
            .text("access_keys", access_keys);

        self.send_form(Method::POST, None, "/d/batch", device_access_token, form)
    }

    pub fn upload_log(&self, device_access_token: &str, log: &LogEntry) -> CoreResult<()> {
        #[derive(Serialize)]
        struct UploadLogRequest<'a> {
            ts: i64,
            #[serde(rename = "type")]
            kind: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            risk: Option<f32>,
            data: &'a serde_json::Value,
        }

        self.send_empty_with_json(
            Method::POST,
            None,
            "/d/log",
            device_access_token,
            &UploadLogRequest {
                ts: log.ts_ms,
                kind: &log.kind,
                risk: log.risk,
                data: &log.data,
            },
        )
    }

    pub fn upload_hash(
        &self,
        hash_base_url: Option<&str>,
        device_access_token: &str,
        content_hash: &[u8; 32],
    ) -> CoreResult<()> {
        let response = self
            .request(
                Method::POST,
                hash_base_url,
                "/hash",
                Some(device_access_token),
            )
            .header("Content-Type", "application/octet-stream")
            .body(content_hash.to_vec())
            .send()?;
        self.expect_success(response)
    }

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

    fn send_empty_with_json<TBody: Serialize + ?Sized>(
        &self,
        method: Method,
        base_override: Option<&str>,
        path: &str,
        bearer_token: &str,
        body: &TBody,
    ) -> CoreResult<()> {
        let response = self
            .request(method, base_override, path, Some(bearer_token))
            .json(body)
            .send()?;
        self.expect_success(response)
    }

    fn send_empty(
        &self,
        method: Method,
        base_override: Option<&str>,
        path: &str,
        bearer_token: Option<&str>,
    ) -> CoreResult<()> {
        let response = self
            .request(method, base_override, path, bearer_token)
            .send()?;
        self.expect_success(response)
    }

    fn send_form(
        &self,
        method: Method,
        base_override: Option<&str>,
        path: &str,
        bearer_token: &str,
        form: Form,
    ) -> CoreResult<()> {
        let response = self
            .request(method, base_override, path, Some(bearer_token))
            .multipart(form)
            .send()?;
        self.expect_success(response)
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
            .unwrap_or_else(|_| String::from("request failed"));
        let message = serde_json::from_str::<ApiErrorResponse>(&fallback)
            .ok()
            .and_then(|body| body.error)
            .filter(|message| !message.is_empty())
            .unwrap_or(fallback);

        Err(CoreError::HttpStatus {
            status: status.as_u16(),
            message,
        })
    }
}

#[derive(Deserialize)]
struct ApiErrorResponse {
    error: Option<String>,
}
