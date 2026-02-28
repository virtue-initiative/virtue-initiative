use std::time::Duration;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use virtue_client_core::resolve_base_api_url;

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

    pub async fn get_device(&self, access_token: &str, device_id: &str) -> Result<Device> {
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
            .ok_or_else(|| anyhow!("device {} not found in settings response", device_id))
    }

    pub async fn register_device(
        &self,
        access_token: &str,
        name: &str,
        avg_interval_seconds: u64,
    ) -> Result<DeviceRegistration> {
        let request = RegisterDeviceRequest {
            name: name.to_string(),
            platform: "linux".to_string(),
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
}

#[derive(Clone, Debug, Deserialize)]
pub struct Device {
    pub id: String,
    pub interval_seconds: u64,
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
    avg_interval_seconds: u64,
}

async fn decode_json<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!("unexpected response {}: {}", status, body));
    }
    Ok(response.json::<T>().await?)
}
