pub mod api;
pub mod build_info;
pub mod config;
pub mod controller;
pub mod crypto;
pub mod daemon;
pub mod error;
pub mod events;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod ipc_bridge;
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
pub use controller::ClientController;
pub use daemon::{DAEMON_STATE_VERSION, Daemon, DaemonState};
pub use error::{CoreError, CoreResult};
pub use events::{Error as EventError, Event, EventChannel};
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use events::{IpcError, IpcListener, RemoteEventBus, RemoteSender};
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use ipc_bridge::IpcBridge;
pub use model::{
    AlertReason, DeviceCredentials, DeviceSettings, Redacted, ScreenshotSkipReason, UploadKind,
};
pub use model::{
    AuthState, BatchUpload, EventData, LogEntry, LoopOutcome, Screenshot, ServiceStatus,
};
pub use module::auth::{LoginRequested, LoginResult, Logout, LogoutRequested, LogoutResult};
pub use module::status::{StatusRequest, StatusResponse};
pub use module::upload::{FlushBatchNow, Upload};
pub use platform::{LifecycleHooks, PlatformHooks, ScreenshotHooks};
pub use rng::{OsRandomSource, RandomSource};
pub use state::{load_state, store_state};
