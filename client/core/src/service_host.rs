use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::CoreResult;

#[derive(Clone, Debug, Default)]
pub struct PersistedServiceState {
    pub monitoring_enabled: bool,
    pub device_id: Option<String>,
}

#[derive(Clone, Debug)]
pub enum CaptureOutcome {
    FramePng(Vec<u8>),
    PermissionMissing,
    SessionUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SleepOutcome {
    Elapsed,
    Interrupted,
}

#[derive(Clone, Debug)]
pub enum ServiceEvent {
    Info(String),
    Warn(String),
    Error(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DaemonAlertEvent {
    pub kind: String,
    pub metadata: Vec<(String, String)>,
    pub created_at: DateTime<Utc>,
    /// Optional explicit device ID. If omitted, daemon uses current persisted device_id.
    pub device_id: Option<String>,
}

/// Minimal host API required by the shared service engine.
/// UI/tray/process management are intentionally out of scope.
#[allow(async_fn_in_trait)]
pub trait ServiceHost {
    fn load_persisted_state(&self) -> CoreResult<PersistedServiceState>;

    fn now_utc(&self) -> DateTime<Utc>;
    async fn sleep_interruptible(&self, duration: Duration) -> CoreResult<SleepOutcome>;
    async fn capture_frame_png(&self) -> CoreResult<CaptureOutcome>;
    fn emit_event(&self, event: ServiceEvent);

    fn should_stop(&self) -> bool {
        false
    }

    fn on_loop_tick(&self) -> CoreResult<()> {
        Ok(())
    }

    /// Returns pending OS/runtime alert events and clears them from host memory.
    fn drain_alert_events(&self) -> CoreResult<Vec<DaemonAlertEvent>> {
        Ok(Vec::new())
    }
}
