pub mod api;
pub mod auth;
pub mod build_info;
pub mod config;
pub mod crypto;
pub mod error;
pub mod events;
pub mod model;
pub mod module;
pub mod platform;
pub mod service;
pub mod storage;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use auth::Auth;
pub use build_info::{BUILD_LABEL, build_label};
pub use config::Config;
pub use error::{CoreError, CoreResult};
pub use model::{
    AuthState, BatchLogEntry, BatchUpload, DeviceCredentials, DeviceSettings, EventData, LogEntry,
    LoginStatus, LoopOutcome, Screenshot, ServiceStatus,
};
pub use model::{ServiceStopMarker, StopIntent, UserSessionState};
pub use platform::PlatformHooks;
pub use service::MonitorService;
