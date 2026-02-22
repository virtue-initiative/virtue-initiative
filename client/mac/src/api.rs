use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use bepure_client_core::resolve_base_api_url;

#[derive(Clone)]
pub struct ApiClient {
    base_url: String,
    client: reqwest::Client,
}

impl ApiClient {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            .build()?;

        Ok(Self {
            base_url: resolve_base_api_url(),
            client,
        })
    }

    pub async fn register_device(
        &self,
        access_token: &str,
        name: &str,
        avg_interval_seconds: u64,
    ) -> Result<DeviceRegistration> {
        let request = RegisterDeviceRequest {
            name: name.to_string(),
            platform: "macos".to_string(),
            avg_interval_seconds,
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

    pub async fn send_log(
        &self,
        access_token: &str,
        event_type: &str,
        device_id: &str,
        image_id: Option<&str>,
        metadata: BTreeMap<String, Value>,
    ) -> Result<CreatedLog> {
        let request = CreateLogRequest {
            event_type: event_type.to_string(),
            device_id: device_id.to_string(),
            image_id: image_id.map(ToString::to_string),
            metadata,
        };

        let url = format!("{}/log", self.base_url);
        let response = self
            .client
            .post(url)
            .bearer_auth(access_token)
            .json(&request)
            .send()
            .await?;

        decode_json(response).await
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct DeviceRegistration {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CreatedLog {}

#[derive(Clone, Debug, Serialize)]
struct RegisterDeviceRequest {
    name: String,
    platform: String,
    avg_interval_seconds: u64,
}

#[derive(Clone, Debug, Serialize)]
struct CreateLogRequest {
    #[serde(rename = "type")]
    event_type: String,
    device_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_id: Option<String>,
    metadata: BTreeMap<String, Value>,
}

async fn decode_json<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!("unexpected response {}: {}", status, body));
    }

    Ok(response.json::<T>().await?)
}
