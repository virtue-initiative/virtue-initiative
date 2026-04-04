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
    #[serde(default, with = "serde_bytes", skip_serializing_if = "Vec::is_empty")]
    pub image: Vec<u8>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content_type: String,
}

impl Default for BatchEventData {
    fn default() -> Self {
        Self {
            image: Vec::new(),
            content_type: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchEvent {
    pub ts: i64,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metadata: Vec<(String, String)>,
    #[serde(default)]
    pub data: BatchEventData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferedBatchEvent {
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuditLogPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direct_log: Option<LogEntry>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "batch_screenshot")]
    pub batch_event: Option<BufferedBatchEvent>,
}

impl AuditLogPayload {
    pub fn for_direct_log(log: LogEntry) -> Self {
        Self {
            direct_log: Some(log),
            batch_event: None,
        }
    }

    pub fn for_batch_event(event: BufferedBatchEvent) -> Self {
        Self {
            direct_log: None,
            batch_event: Some(event),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuditRecord {
    Log {
        local_id: String,
        should_be_in_batch: bool,
        #[serde(default)]
        requires_hash_upload: bool,
        log: AuditLogPayload,
    },
    HashUploaded {
        local_id: String,
    },
    LogUploaded {
        local_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        server_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        batch_id: Option<String>,
    },
    BatchUploaded {
        server_id: String,
    },
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
    #[serde(default)]
    pub post_login_proof_batches_remaining: u32,
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
pub struct LoopOutcome {
    pub ran_at_ms: i64,
    pub next_run_at_ms: i64,
    pub status: ServiceStatus,
}

#[derive(Debug, Clone)]
pub struct AuditLogItem {
    pub local_id: String,
    pub should_be_in_batch: bool,
    pub requires_hash_upload: bool,
    pub payload: AuditLogPayload,
}

#[derive(Debug, Clone, Default)]
pub struct AuditState {
    pub items: Vec<AuditLogItem>,
    pub pending_hash_uploads: Vec<AuditLogItem>,
    pub pending_direct_uploads: Vec<AuditLogItem>,
    pub pending_batch_uploads: Vec<AuditLogItem>,
    pub pending_request_count: usize,
}
