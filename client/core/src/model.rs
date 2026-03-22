use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Screenshot {
    pub captured_at_ms: i64,
    #[serde(with = "serde_bytes")]
    pub bytes: Vec<u8>,
    pub content_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchEventData {
    #[serde(with = "serde_bytes")]
    pub image: Vec<u8>,
    pub content_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchEvent {
    pub ts: i64,
    #[serde(rename = "type")]
    pub kind: String,
    pub data: BatchEventData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferedScreenshot {
    pub event: BatchEvent,
    pub content_hash: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub ts_ms: i64,
    pub kind: String,
    pub risk: Option<f32>,
    pub data: serde_json::Value,
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
    pub enabled: bool,
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
pub struct BatchBufferState {
    pub screenshots: Vec<BufferedScreenshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub is_authenticated: bool,
    pub is_running: bool,
    pub device_id: Option<String>,
    pub last_loop_at_ms: Option<i64>,
    pub last_screenshot_at_ms: Option<i64>,
    pub last_batch_at_ms: Option<i64>,
    pub pending_request_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RequestKind {
    DeviceSettings,
    UploadBatch,
    UploadLog,
    UploadHash,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingRequest {
    pub id: String,
    pub kind: RequestKind,
    pub payload: serde_json::Value,
    pub last_tried_at_ms: Option<i64>,
    pub try_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RequestDisposition {
    Completed,
    Deferred,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopOutcome {
    pub ran_at_ms: i64,
    pub next_run_at_ms: i64,
    pub status: ServiceStatus,
}
