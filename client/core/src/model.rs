use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::events::UploadKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Screenshot {
    pub captured_at_ms: i64,
    #[serde(with = "serde_bytes")]
    pub bytes: Vec<u8>,
    pub content_type: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(transparent)]
pub struct EventData(pub BTreeMap<String, Value>);

impl EventData {
    pub fn insert(&mut self, key: impl Into<String>, value: Value) {
        self.0.insert(key.into(), value);
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    pub fn object(&self) -> Map<String, Value> {
        self.0.clone().into_iter().collect()
    }

    pub fn from_pairs(pairs: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut data = Self::default();
        for (key, value) in pairs {
            data.insert(key, Value::String(value));
        }
        data
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub ts: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<f32>,
    #[serde(flatten)]
    pub event: UploadKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchUpload {
    pub start_time_ms: i64,
    pub end_time_ms: i64,
    #[serde(with = "serde_bytes")]
    pub bytes: Vec<u8>,
    pub access_keys: Vec<BatchAccessKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchAccessKey {
    pub user_id: String,
    pub hpke_key_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCredentials {
    pub device_id: String,
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSettings {
    pub device_id: String,
    pub name: String,
    pub platform: String,
    #[serde(default)]
    pub owner: Option<BatchRecipient>,
    #[serde(default)]
    pub partners: Vec<BatchRecipient>,
    #[serde(default)]
    pub hash_base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRecipient {
    pub user_id: String,
    pub pub_key_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HashParams {
    pub version: String,
    pub algorithm: String,
    pub memory_cost_kib: u32,
    pub time_cost: u32,
    pub parallelism: u32,
    pub salt_length: u32,
    pub hkdf_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginStatus {
    pub access_token: String,
    pub device: Option<DeviceCredentials>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthState {
    pub user_access_token: Option<String>,
    pub device_credentials: Option<DeviceCredentials>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceStatus {
    pub is_authenticated: bool,
    pub is_running: bool,
    pub device_id: Option<String>,
    pub last_loop_at_ms: Option<i64>,
    pub pending_request_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopOutcome {
    pub ran_at_ms: i64,
    pub status: ServiceStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum UserSessionState {
    LoggedIn,
    LoggedOut,
    #[default]
    Unknown,
}

impl UserSessionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LoggedIn => "logged_in",
            Self::LoggedOut => "logged_out",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct StopIntent {
    pub source: String,
    pub requested_at_ms: i64,
}

impl Default for StopIntent {
    fn default() -> Self {
        Self {
            source: String::new(),
            requested_at_ms: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ServiceStopMarker {
    pub origin: String,
    pub stopped_at_ms: i64,
}

impl Default for ServiceStopMarker {
    fn default() -> Self {
        Self {
            origin: String::new(),
            stopped_at_ms: 0,
        }
    }
}
