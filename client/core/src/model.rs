use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

// ── Payload types (shared by event system and upload pipeline) ────────────────

/// Wraps a value so that its `Debug` output is always `[REDACTED]`.
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Redacted<T>(pub T);

impl<T> std::fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[REDACTED]")
    }
}

impl<T: std::ops::Deref<Target = str>> std::ops::Deref for Redacted<T> {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl From<String> for Redacted<String> {
    fn from(s: String) -> Self {
        Redacted(s)
    }
}

impl From<&str> for Redacted<String> {
    fn from(s: &str) -> Self {
        Redacted(s.to_string())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotSkipReason {
    StaticScreen,        // duplicate frame (fingerprint unchanged)
    LockedOrScreensaver, // session locked / screensaver / screen off
}

/// Why the most recent screenshot attempt didn't produce an upload. Kept
/// deliberately separate from [`ScreenshotSkipReason`], which is a wire
/// format the API and web app decode: this one is local status only (CORE-018)
/// and can gain variants — such as an outright capture failure, which is not a
/// "skip" — without touching the uploaded event shape.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StatusSkipReason {
    StaticScreen,
    LockedOrScreensaver,
    CaptureFailed,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum UploadKind {
    Screenshot {
        image: Vec<u8>,
        content_type: String,
        /// Raw skin-tone heuristic score ∈ [0.0, 1.0] from the risk classifier (dev metadata).
        /// `None` on older logs or when no classifier was available.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        skin_detection: Option<f32>,
        /// Raw NSFW model probability ∈ [0.0, 1.0] from the risk classifier (dev metadata).
        /// `None` when the skin gate skipped the model, on older logs, or when no classifier
        /// was available.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nsfw_detection: Option<f32>,
    },
    /// The user explicitly quit the monitor while it was expected to be
    /// running. Always high risk.
    UserStop,
    /// Monitoring resumed after a prior `UserStop`. Always risk 0% — purely
    /// informational, pairing with the `UserStop` it follows.
    UserStart,
    ScreenshotSkipped {
        reason: ScreenshotSkipReason,
    },
    Alert {
        message: String,
    },
    CaptureFailed,
    Dev {
        title: String,
        details: Option<String>,
    },
    Heartbeat,
    /// A single wakeup was more than a minute late, or the sum of recent
    /// lateness (over the last 10 tracked wakeups) exceeded 5 minutes.
    /// Excused near a system login/logout. See CORE-002.
    ScreenshotMissed,
    /// The daemon detected that the last known system login time changed.
    /// Always risk 0%. See CORE-006.
    SystemLogin {
        utc_ms: i64,
    },
    /// The daemon detected that the last known system logout time changed.
    /// Always risk 0%. See CORE-006.
    SystemLogout {
        utc_ms: i64,
    },
    /// The daemon process started more than `RESTART_ALERT_THRESHOLD` times
    /// within a rolling window — evidence of a crash loop or repeated kill.
    /// See CORE-018.
    RepeatedRestarts {
        count: u32,
        window_ms: i64,
    },
}

/// Hand-written so the captured screenshot bytes never reach a log line
/// verbatim — every other field prints exactly as `#[derive(Debug)]` would.
impl std::fmt::Debug for UploadKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UploadKind::Screenshot {
                image,
                content_type,
                skin_detection,
                nsfw_detection,
            } => write!(
                f,
                "Screenshot {{ image: <{} bytes>, content_type: {content_type:?}, skin_detection: {skin_detection:?}, nsfw_detection: {nsfw_detection:?} }}",
                image.len()
            ),
            UploadKind::UserStop => write!(f, "UserStop"),
            UploadKind::UserStart => write!(f, "UserStart"),
            UploadKind::ScreenshotSkipped { reason } => {
                write!(f, "ScreenshotSkipped {{ reason: {reason:?} }}")
            }
            UploadKind::Alert { message } => write!(f, "Alert {{ message: {message:?} }}"),
            UploadKind::CaptureFailed => write!(f, "CaptureFailed"),
            UploadKind::Dev { title, details } => {
                write!(f, "Dev {{ title: {title:?}, details: {details:?} }}")
            }
            UploadKind::Heartbeat => write!(f, "Heartbeat"),
            UploadKind::ScreenshotMissed => write!(f, "ScreenshotMissed"),
            UploadKind::SystemLogin { utc_ms } => {
                write!(f, "SystemLogin {{ utc_ms: {utc_ms:?} }}")
            }
            UploadKind::SystemLogout { utc_ms } => {
                write!(f, "SystemLogout {{ utc_ms: {utc_ms:?} }}")
            }
            UploadKind::RepeatedRestarts { count, window_ms } => {
                write!(
                    f,
                    "RepeatedRestarts {{ count: {count:?}, window_ms: {window_ms:?} }}"
                )
            }
        }
    }
}

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
    /// Total number of events in this batch.
    pub total_count: u32,
    /// Number of events in this batch whose risk fell in the high band (>= 0.7).
    pub high_risk_count: u32,
    /// Number of events in this batch whose risk fell in the medium band (0.4–0.7).
    pub medium_risk_count: u32,
    /// Number of `UploadKind::Screenshot` events in this batch.
    pub screenshot_count: u32,
    /// Alert-email metadata for any high-risk events in this batch, carried
    /// alongside the batch upload instead of a separate notify call.
    #[serde(default)]
    pub notifications: Vec<NotifyPayload>,
}

/// Minimal metadata sent alongside a batch upload to trigger an alert email for a
/// high-risk event. The event body itself is uploaded end-to-end encrypted via the
/// same batch; this payload carries only what the notification email needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotifyPayload {
    pub ts: i64,
    #[serde(rename = "type")]
    pub event_type: String,
    pub risk: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchAccessKey {
    pub user_id: String,
    pub hpke_key_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCredentials {
    pub device_id: String,
    pub refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSettings {
    pub device_id: String,
    pub name: String,
    pub platform: String,
    /// Every recipient the device must wrap batch keys for: the owner (when they
    /// have a public key) followed by all accepted partners.
    #[serde(default)]
    pub wrapping_keys: Vec<BatchRecipient>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthState {
    pub device_credentials: Option<DeviceCredentials>,
    /// The email the device was registered with, kept so every platform's
    /// status page can name the signed-in account (CORE-010) instead of each
    /// one stashing it separately. `None` on states written before this
    /// existed, and cleared on logout.
    #[serde(default)]
    pub account_email: Option<String>,
}

/// One entry in the daemon's recent-errors ring (CORE-018). `context` is a
/// short stable identifier for the failing phase (`"batch_upload"`,
/// `"screenshot_capture"`, …) so a UI can group or filter without parsing
/// `message`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatusError {
    pub at_ms: i64,
    pub context: String,
    pub message: String,
}

/// Everything a platform's status page shows, assembled by
/// `module::status::build`. See CORE-010; every field but `is_running` is
/// derivable from persisted state plus compile-time config, so a client whose
/// daemon isn't running reports the same data from disk.
///
/// New fields are all `#[serde(default)]`: this struct is the IPC wire shape
/// between a client and a possibly-older daemon process (`ipc.rs`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceStatus {
    pub is_authenticated: bool,
    pub is_running: bool,
    #[serde(default)]
    pub account_email: Option<String>,
    pub device_id: Option<String>,
    #[serde(default)]
    pub device_name: Option<String>,
    /// Wrapping keys minus the owner's own key. `None` until device settings
    /// have been fetched at least once — distinct from a real count of zero.
    #[serde(default)]
    pub partner_count: Option<usize>,
    #[serde(default)]
    pub pending_hash_count: usize,
    #[serde(default)]
    pub pending_batch_count: usize,
    pub pending_request_count: usize,
    pub last_loop_at_ms: Option<i64>,
    #[serde(default)]
    pub last_screenshot_attempt_at_ms: Option<i64>,
    #[serde(default)]
    pub last_screenshot_at_ms: Option<i64>,
    #[serde(default)]
    pub last_skip_reason: Option<StatusSkipReason>,
    #[serde(default)]
    pub last_batch_at_ms: Option<i64>,
    #[serde(default)]
    pub recent_errors: Vec<StatusError>,
    #[serde(default)]
    pub api_base_url: String,
    #[serde(default)]
    pub hash_base_url: Option<String>,
    #[serde(default)]
    pub capture_interval_seconds: u64,
    #[serde(default)]
    pub batch_window_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopOutcome {
    pub ran_at_ms: i64,
    pub status: ServiceStatus,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn upload_kind_user_stop_serializes_to_tagged_shape() {
        let upload = UploadKind::UserStop;
        assert_eq!(
            serde_json::to_value(upload).unwrap(),
            json!({ "type": "user_stop" })
        );
    }

    #[test]
    fn upload_kind_screenshot_missed_serializes_to_tagged_shape() {
        // CORE-002: "The late wakeup event SHOULD be called
        // \"screenshot_missed\"."
        let upload = UploadKind::ScreenshotMissed;
        assert_eq!(
            serde_json::to_value(upload).unwrap(),
            json!({ "type": "screenshot_missed" })
        );
    }

    #[test]
    fn upload_kind_system_login_serializes_to_tagged_shape() {
        let upload = UploadKind::SystemLogin { utc_ms: 1_000 };
        assert_eq!(
            serde_json::to_value(upload).unwrap(),
            json!({
                "type": "system_login",
                "data": { "utc_ms": 1_000 }
            })
        );
    }

    #[test]
    fn upload_kind_system_logout_serializes_to_tagged_shape() {
        let upload = UploadKind::SystemLogout { utc_ms: 2_000 };
        assert_eq!(
            serde_json::to_value(upload).unwrap(),
            json!({
                "type": "system_logout",
                "data": { "utc_ms": 2_000 }
            })
        );
    }
}
