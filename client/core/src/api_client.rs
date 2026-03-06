use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::error::{CoreError, CoreResult};
use crate::models::{Device, DeviceRegistration};
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

    pub async fn get_hash_server_url(&self, access_token: &str, device_id: &str) -> CoreResult<String> {
        let device = self.get_device(access_token, device_id).await?;
        device.hash_base_url.ok_or_else(|| {
            CoreError::NotFound("device settings did not include hash_base_url".to_string())
        })
    }

    pub async fn get_device(&self, access_token: &str, device_id: &str) -> CoreResult<Device> {
        let url = format!("{}/d/device", self.base_url);
        let response = self
            .client
            .get(url)
            .bearer_auth(access_token)
            .send()
            .await?;

        let device: Device = decode_json(response).await?;
        if !device_id.is_empty() && device.id != device_id {
            return Err(CoreError::NotFound(format!("device {} not found", device_id)));
        }
        Ok(device)
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

        let url = format!("{}/d/device", self.base_url);
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
        _device_id: &str,
        kind: &str,
        metadata: &[(String, String)],
        created_at: DateTime<Utc>,
    ) -> CoreResult<()> {
        let request = CreateAlertLogRequest {
            ts: created_at.timestamp_millis(),
            type_: kind.to_string(),
            data: metadata
                .iter()
                .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                .collect(),
        };

        let url = format!("{}/d/log", self.base_url);
        let response = self
            .client
            .post(url)
            .bearer_auth(access_token)
            .json(&request)
            .send()
            .await?;

        let _: Value = decode_json(response).await?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
struct RegisterDeviceRequest {
    name: String,
    platform: String,
}

#[derive(Clone, Debug, Serialize)]
struct CreateAlertLogRequest {
    ts: i64,
    #[serde(rename = "type")]
    type_: String,
    data: serde_json::Map<String, Value>,
}

async fn decode_json<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> CoreResult<T> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(CoreError::UnexpectedResponse { status, body });
    }
    Ok(response.json::<T>().await?)
}
