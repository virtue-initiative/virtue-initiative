pub mod api;
pub mod build_info;
pub mod config;
pub mod controller;
pub mod crypto;
pub mod error;
pub mod events;
pub mod ipc;
pub mod model;
pub mod module;
pub mod platform;
pub mod service;
pub mod storage;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use build_info::{BUILD_LABEL, build_label};
pub use config::Config;
pub use controller::ControllerClient;
pub use error::{CoreError, CoreResult};
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub use ipc::register_connect_tx;
pub use ipc::{IpcError, IpcListener};
pub use model::{
    AuthState, BatchUpload, DeviceCredentials, DeviceSettings, EventData, LogEntry, LoginStatus,
    LoopOutcome, Screenshot, ServiceStatus,
};
pub use platform::PlatformHooks;
pub use service::{ITER_INTERVAL, MonitorService, iter_sleep};
