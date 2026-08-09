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
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LifecycleKind {
    /// A suspend interval detected retrospectively via boot-vs-monotonic
    /// clock divergence.
    SuspendDetected { duration_ms: i64 },
    /// Start of a new expected-running window (OS session/user login).
    SystemLogin { utc_ms: i64 },
    /// End of an expected-running window (OS session/user logout).
    SystemLogout { utc_ms: i64 },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotSkipReason {
    StaticScreen,        // duplicate frame (fingerprint unchanged)
    LockedOrScreensaver, // session locked / screensaver / screen off
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum AlertReason {
    /// The process wasn't running during a stretch of awake time between a
    /// known login and the first observed heartbeat.
    UnexpectedStart,
    /// The process stopped running before the session's logout, leaving a
    /// gap between the last heartbeat and the (possibly reconstructed)
    /// logout timestamp.
    UnexpectedStop,
    /// A stretch of awake time (same boot) between two heartbeats with no
    /// sample — crash, force-kill-and-restart, or frozen process.
    UnexpectedGap,
    /// The user explicitly quit the monitor while it was expected to be
    /// running.
    UserStop,
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
    Lifecycle {
        #[serde(flatten)]
        kind: LifecycleKind,
    },
    LifecycleAlert {
        reason: AlertReason,
    },
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
            UploadKind::Lifecycle { kind } => write!(f, "Lifecycle {{ kind: {kind:?} }}"),
            UploadKind::LifecycleAlert { reason } => {
                write!(f, "LifecycleAlert {{ reason: {reason:?} }}")
            }
            UploadKind::ScreenshotSkipped { reason } => {
                write!(f, "ScreenshotSkipped {{ reason: {reason:?} }}")
            }
            UploadKind::Alert { message } => write!(f, "Alert {{ message: {message:?} }}"),
            UploadKind::CaptureFailed => write!(f, "CaptureFailed"),
            UploadKind::Dev { title, details } => {
                write!(f, "Dev {{ title: {title:?}, details: {details:?} }}")
            }
            UploadKind::Heartbeat => write!(f, "Heartbeat"),
        }
    }
}

/// A single piece of `ServiceStatus` reported by one module in response to a
/// `StatusRequest`. Each module emits only the fields it owns; the status
/// module merges them into a complete `ServiceStatus`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum PartialStatus {
    Auth {
        is_authenticated: bool,
        device_id: Option<String>,
    },
    Lifecycle {
        is_running: bool,
        last_loop_at_ms: Option<i64>,
    },
    Upload {
        pending_request_count: usize,
    },
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
    /// Number of events in this batch whose risk fell in the high band (>= 0.7).
    pub high_risk_count: u32,
    /// Number of events in this batch whose risk fell in the medium band (0.4–0.7).
    pub medium_risk_count: u32,
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
    /// Raw Ed25519 private key bytes for signing `POST /hash` requests to the
    /// real hash-server (see `crypto::sign_request`). Generated locally at
    /// registration (`module/auth.rs`'s `do_login`) and never transmitted —
    /// only the derived pubkey is sent. Plaintext in `event_state.json`, same
    /// storage model as `refresh_token` (no OS keychain yet).
    #[serde(with = "serde_bytes")]
    pub signing_key: [u8; 32],
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // `LifecycleKind` must serialize as a bare `"kind"`-tagged object flattened
    // into `UploadKind::Lifecycle`'s `data` — this is exactly the wire shape
    // the web viewer's `getLogCategory`/`getLogMessage` assume, and previously
    // silently drifted from it when the variants gained fields (see #526).
    #[test]
    fn lifecycle_kind_serializes_to_flattened_tagged_shape() {
        assert_eq!(
            serde_json::to_value(LifecycleKind::SuspendDetected {
                duration_ms: 60_033
            })
            .unwrap(),
            json!({ "kind": "suspend_detected", "duration_ms": 60_033 })
        );
        assert_eq!(
            serde_json::to_value(LifecycleKind::SystemLogin { utc_ms: 123 }).unwrap(),
            json!({ "kind": "system_login", "utc_ms": 123 })
        );
        assert_eq!(
            serde_json::to_value(LifecycleKind::SystemLogout { utc_ms: 456 }).unwrap(),
            json!({ "kind": "system_logout", "utc_ms": 456 })
        );
    }

    #[test]
    fn upload_kind_lifecycle_flattens_kind_into_data() {
        let upload = UploadKind::Lifecycle {
            kind: LifecycleKind::SystemLogin { utc_ms: 789 },
        };
        assert_eq!(
            serde_json::to_value(upload).unwrap(),
            json!({
                "type": "lifecycle",
                "data": { "kind": "system_login", "utc_ms": 789 }
            })
        );
    }
}
