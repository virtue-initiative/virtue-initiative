use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::resolve_base_api_url;

#[derive(Clone)]
pub struct ApiClient {
    base_url: String,
    client: reqwest::Client,
}

impl ApiClient {
    pub fn new() -> CoreResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            .build()?;

        Ok(Self {
            base_url: resolve_base_api_url(),
            client,
        })
    }

    pub async fn get_hash_server_url(
        &self,
        access_token: &str,
        device_id: &str,
    ) -> CoreResult<String> {
        let url = format!("{}/hash-server?deviceId={}", self.base_url, device_id);
        let response = self
            .client
            .get(url)
            .bearer_auth(access_token)
            .send()
            .await?;

        let body: HashServerResponse = decode_json(response).await?;
        Ok(body.url)
    }

    pub async fn get_device(&self, access_token: &str, device_id: &str) -> CoreResult<Device> {
        let url = format!("{}/device", self.base_url);
        let response = self
            .client
            .get(url)
            .bearer_auth(access_token)
            .send()
            .await?;

        let devices: Vec<Device> = decode_json(response).await?;
        devices
            .into_iter()
            .find(|d| d.id == device_id)
            .ok_or_else(|| CoreError::NotFound(format!("device {} not found", device_id)))
    }

    pub async fn register_device(
        &self,
        access_token: &str,
        name: &str,
        platform: &str,
    ) -> CoreResult<DeviceRegistration> {
        let request = RegisterDeviceRequest {
            name: name.to_string(),
            platform: platform.to_string(),
        };

        let url = format!("{}/device", self.base_url);
        let response = self
            .client
            .post(url)
            .bearer_auth(access_token)
            .json(&request)
            .send()
            .await?;

        decode_json(response).await
    }

    pub async fn create_alert_log(
        &self,
        access_token: &str,
        device_id: &str,
        kind: &str,
        metadata: &[(String, String)],
        created_at: DateTime<Utc>,
    ) -> CoreResult<()> {
        let request = CreateAlertLogRequest {
            device_id: device_id.to_string(),
            kind: kind.to_string(),
            metadata: metadata.to_vec(),
            created_at: created_at.to_rfc3339_opts(SecondsFormat::Millis, true),
        };

        let url = format!("{}/logs", self.base_url);
        let response = self
            .client
            .post(url)
            .bearer_auth(access_token)
            .json(&request)
            .send()
            .await?;

        let _: CreateAlertLogResponse = decode_json(response).await?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Device {
    pub id: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DeviceRegistration {
    pub id: String,
}

#[derive(Clone, Debug, Serialize)]
struct RegisterDeviceRequest {
    name: String,
    platform: String,
}

#[derive(Clone, Debug, Serialize)]
struct CreateAlertLogRequest {
    device_id: String,
    kind: String,
    metadata: Vec<(String, String)>,
    created_at: String,
}

#[derive(Clone, Debug, Deserialize)]
struct HashServerResponse {
    url: String,
}

#[derive(Clone, Debug, Deserialize)]
struct CreateAlertLogResponse {
    #[allow(dead_code)]
    log: CreatedAlertLog,
}

#[derive(Clone, Debug, Deserialize)]
struct CreatedAlertLog {
    #[allow(dead_code)]
    id: String,
}

async fn decode_json<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> CoreResult<T> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(CoreError::UnexpectedResponse { status, body });
    }
    Ok(response.json::<T>().await?)
}
