use serde::{Deserialize, Serialize};

use crate::model::ServiceStatus;
pub use crate::model::{
    AlertReason, DeviceCredentials, DeviceSettings, LifecycleKind, PartialStatus,
    ProcessStoppedReason, Redacted, UploadKind,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ping;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessStarted;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessStopped(pub ProcessStoppedReason);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerSuspended;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerResumed;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSessionLogin;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSessionLogout;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Login {
    pub credentials: DeviceCredentials,
    pub settings: DeviceSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Logout;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSettingsRefreshed {
    pub settings: DeviceSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Upload {
    pub risk: f32,
    pub kind: UploadKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureFailed;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusRequest;

// PartialStatus from model is used directly as an event type.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub status: ServiceStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequested {
    pub email: String,
    pub password: Redacted<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogoutRequested;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserStopRequested {
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResult {
    pub success: bool,
    pub error: Option<String>,
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogoutResult {
    pub success: bool,
    pub error: Option<String>,
}

/// Emitted by ConfigModule when config_override.json changes.
/// Screenshot/Upload/Auth subscribe to update intervals and API base URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChanged {
    pub api_base_url: String,
    pub screenshot_interval_ms: u64,
    pub batch_interval_ms: u64,
}

/// Replaces UploadObserver::force_upload_now; UploadModule flushes its pending
/// batch on receipt. Mac daemon emits this post-wake.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlushBatchNow;
