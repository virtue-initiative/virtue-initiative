pub mod api;
pub mod assembly;
pub mod build_info;
pub mod config;
pub mod controller;
pub mod crypto;
pub mod error;
pub mod events;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod ipc_bridge;
pub mod model;
pub mod module;
pub mod platform;
pub mod state;
pub mod storage;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use assembly::{build_default_modules, build_default_modules_reqwest};
pub use build_info::{BUILD_LABEL, build_label};
pub use config::Config;
pub use controller::ClientController;
pub use error::{CoreError, CoreResult};
pub use events::{
    Emitter, Error as EventError, Event, EventBus, EventChannel, Observer, Ping, StateType,
};
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use events::{IpcError, IpcListener, RemoteEventBus, RemoteSender};
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use ipc_bridge::IpcBridge;
pub use model::{
    AlertReason, DeviceCredentials, DeviceSettings, LifecycleKind, PartialStatus,
    ProcessStoppedReason, Redacted, UploadKind,
};
pub use model::{
    AuthState, BatchUpload, EventData, LogEntry, LoginStatus, LoopOutcome, Screenshot,
    ServiceStatus,
};
pub use module::auth::{
    DeviceSettingsRefreshed, Login, LoginRequested, LoginResult, Logout, LogoutRequested,
    LogoutResult,
};
pub use module::config::ConfigChanged;
pub use module::lifecycle::{
    ComputerResumed, ComputerSuspended, ProcessStarted, ProcessStopped, UserSessionLogin,
    UserSessionLogout, UserStopRequested,
};
pub use module::screenshot::{CaptureFailed, ScreenshotPaused, ScreenshotResumed};
pub use module::status::{StatusRequest, StatusResponse};
pub use module::upload::FlushBatchNow;
pub use module::upload::Upload;
pub use platform::{PlatformHooks, ScreenshotHooks};
pub use state::{load_state, store_state};
