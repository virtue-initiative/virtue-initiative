pub mod api;
pub mod build_info;
pub mod config;
pub mod crypto;
pub mod daemon;
pub mod error;
/// Empty on platforms whose CLI/tray is in-process; see the module's own
/// `#![cfg]`.
pub mod ipc;
pub mod logging;
pub mod model;
pub mod module;
pub mod platform;
pub mod rng;
pub mod state;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use build_info::{BUILD_LABEL, build_label};
pub use config::{
    Config, DEFAULT_API_BASE_URL, default_batch_window_seconds, default_capture_interval_seconds,
};
pub use daemon::{DAEMON_STATE_VERSION, Daemon, DaemonState};
pub use error::{CoreError, CoreResult};
pub use model::{
    AuthState, BatchUpload, EventData, LogEntry, LoopOutcome, Screenshot, ServiceStatus,
};
pub use model::{DeviceCredentials, DeviceSettings, Redacted, ScreenshotSkipReason, UploadKind};
pub use module::upload::Upload;
pub use platform::{LifecycleHooks, PlatformHooks, ScreenshotHooks};
pub use rng::{OsRandomSource, RandomSource};
pub use state::{load_state, store_state};
